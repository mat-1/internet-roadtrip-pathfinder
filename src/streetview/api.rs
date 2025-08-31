use std::fmt::Write;
use std::time::Duration;
use std::{
    borrow::Cow,
    sync::{LazyLock, OnceLock},
};

use base64::{Engine, prelude::BASE64_STANDARD};
use coarsetime::Instant;
use eyre::bail;
use http::HeaderMap;
use reqwest::Url;
use simd_json::{
    base::{ValueAsArray, ValueAsScalar},
    derived::TypedArrayValue,
    json,
};
use tokio::fs;
use tokio::time::sleep;
use tracing::{debug, error, info, trace, warn};

use crate::model::{
    ApiPano, ApiPanoId, GetMetadataResponse, Location, Pano, PanoLink, SMALL_TILE_SIZE, SizedTile,
};

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::ClientBuilder::new()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0")
        .default_headers({
            let mut headers = HeaderMap::new();
            headers.insert("Accept-Language", "en-US,en;q=0.5".parse().unwrap());
            if let Ok(nid) = std::fs::read_to_string("nid.txt") {
                headers.insert("Cookie", format!("NID={nid}").parse().unwrap());
            }
            headers
        })
        .cookie_store(true)
        .build()
        .unwrap()
});

pub async fn try_get_panos_at_tile(tile: SizedTile) -> eyre::Result<Option<Box<[ApiPano]>>> {
    // use panos_near_coords
    let tile_center_coords = tile.coords_at_center();
    let tile_corner1_coords = tile.to_coords();
    let tile_corner2_coords = SizedTile {
        x: tile.x + 1,
        y: tile.y + 1,
        size: tile.size,
    }
    .to_coords();

    let tile_min_coords = Location {
        lat: tile_corner1_coords.lat.min(tile_corner2_coords.lat),
        lng: tile_corner1_coords.lng.min(tile_corner2_coords.lng),
    };
    let tile_max_coords = Location {
        lat: tile_corner1_coords.lat.max(tile_corner2_coords.lat),
        lng: tile_corner1_coords.lng.max(tile_corner2_coords.lng),
    };

    // add 5 meters just in case
    let radius_meters = (tile.distance_from_corner_to_center() + 5.).ceil() as u32;

    let Some(panos) = panos_near_coords(
        &tile_center_coords,
        radius_meters,
        tile.size != SMALL_TILE_SIZE,
    )
    .await?
    else {
        return Ok(None);
    };

    trace!("original: {}", panos.len());

    trace!("tile loc: {tile_min_coords} - {tile_max_coords}");

    let mut panos = panos
        .into_iter()
        .filter(|pano| {
            let lat = pano.loc.lat;
            let lng = pano.loc.lng;

            lat >= tile_min_coords.lat
                && lat <= tile_max_coords.lat
                && lng >= tile_min_coords.lng
                && lng <= tile_max_coords.lng
        })
        .collect::<Box<[_]>>();

    // this is important for the optimization that does binary search on panos to
    // find nearby ones
    panos.sort_by(|a, b| a.loc.lat.cmp(&b.loc.lat));

    trace!("filtered: {}", panos.len());

    Ok(Some(panos))
}

pub(super) async fn fetch_getmetadata_responses(
    pano_ids: &[ApiPanoId],
) -> eyre::Result<Vec<GetMetadataResponse>> {
    let pano_ids = pano_ids
        .iter()
        .map(|id| decode_protobuf_pano(&id.0))
        .collect::<Vec<_>>();

    debug!("requesting getmetadata links for {} panos", pano_ids.len());

    let mut attempt_number = 0;

    loop {
        attempt_number += 1;

        let url = "https://maps.googleapis.com/$rpc/google.internal.maps.mapsjs.v1.MapsJsInternalService/GetMetadata";
        let request_data = build_getmetadata_request(&pano_ids);
        let res = CLIENT
            .post(url)
            .header("content-type", "application/json+protobuf")
            .json(&request_data)
            .send()
            .await?;

        let text = res.text().await?;
        let mut text_bytes = text.into_bytes();
        let Ok(json) = simd_json::from_slice::<simd_json::OwnedValue>(&mut text_bytes) else {
            error!(
                "Failed to parse JSON response: {:?}",
                String::from_utf8_lossy(&text_bytes)
            );
            bail!("Failed to parse JSON response");
        };

        trace!("json: {}", simd_json::to_string(&json).unwrap());

        if !json[1].is_array() {
            // [14, "The service is currently unavailable."]

            warn!(
                "GetMetadata response didn't have array, response: {json}. request_data: {request_data}"
            );
            // retry 10 times
            if attempt_number > 10 {
                bail!("Invalid GetMetadata response: {json}");
            } else {
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        return parse_getmetadata_response(&json[1]);
    }
}

fn parse_getmetadata_response(
    all_responses: &simd_json::OwnedValue,
) -> eyre::Result<Vec<GetMetadataResponse>> {
    let all_responses = all_responses.as_array().expect("is_array was checked");

    trace!(
        "Got number of replies from GetMetadata: {}",
        all_responses.len()
    );
    let mut results = Vec::new();
    for pano_res in all_responses {
        // println!("{}", simd_json::to_string(&pano_res).unwrap());

        let pano_id = pano_res[1][1].as_str().unwrap();

        let pano_lat = pano_res[5]
            .as_array()
            .and_then(|p| p.first())
            .and_then(|p| p.as_array())
            .and_then(|p| p.get(1))
            .and_then(|p| p.as_array())
            .and_then(|p| p.first())
            .and_then(|p| p.as_array())
            .and_then(|p| p.get(2));
        let Some(pano_lat) = pano_lat else {
            debug!(
                "pano without game coords: {}",
                simd_json::to_string(&pano_res).unwrap()
            );
            // pano has no game coords (originalLat/originalLng), huh
            continue;
        };
        let pano_lat = pano_lat.as_f64().unwrap();
        let pano_lng = pano_res[5][0][1][0][3].as_f64().unwrap();

        let mut links = Vec::new();
        trace!("{}", simd_json::to_string(&pano_res[5][0]).unwrap());
        if let Some(immediate_links_data) = &pano_res[5][0]
            .as_array()
            .and_then(|e| e.get(6))
            .and_then(|e| e.as_array())
        {
            let all_links_data = &pano_res[5][0][3][0];

            for immediate_link_data in immediate_links_data.iter() {
                let index = immediate_link_data[0].as_u64().unwrap() as usize;
                let heading = &immediate_link_data[1][3];
                let heading = heading
                    .as_f64()
                    .or_else(|| heading.as_u64().map(|h| h as f64))
                    .or_else(|| heading.as_i64().map(|h| h as f64));
                let Some(heading) = heading else {
                    warn!("link missing heading: {immediate_link_data}");
                    continue;
                };

                let link_data = &all_links_data[index];
                trace!("  link_data: {link_data}");

                let link_pano_id = link_data[0][1].as_str().unwrap();
                let lat = link_data[2][0][2].as_f64().unwrap();
                let lng = link_data[2][0][3].as_f64().unwrap();

                let link = PanoLink {
                    pano: Pano {
                        id: link_pano_id.into(),
                        loc: Location::new_deg(lat, lng),
                    },
                    heading: heading as f32,
                };
                trace!("  link: {link:?}");
                links.push(link)
            }
        }

        results.push(GetMetadataResponse {
            id: pano_id.into(),
            loc: Location::new_deg(pano_lat, pano_lng),
            links,
        });
    }

    Ok(results)
}

pub async fn panos_near_coords(
    coords: &Location,
    radius_meters: u32,
    bail_on_too_many_panos: bool,
) -> eyre::Result<Option<Vec<ApiPano>>> {
    ensure_nid_cookie_set().await?;

    let url = build_listentityphotos_request(coords, radius_meters);
    debug!("url: {url}");
    let start = Instant::now();
    let res = CLIENT.get(url).send().await?;
    let text = res.text().await?;
    let mut text_bytes = text.into_bytes();

    let Ok(json) = simd_json::from_slice::<simd_json::OwnedValue>(&mut text_bytes[4..]) else {
        error!(
            "Failed to parse JSON response: {:?}",
            String::from_utf8_lossy(&text_bytes)
        );
        bail!("Failed to parse JSON response");
    };

    debug!("Request for listentityphotos took: {:?}", start.elapsed());

    let nearby_panos = &json[0];
    let mut panos = Vec::new();
    if let Some(nearby_panos) = nearby_panos.as_array() {
        trace!("Number of nearby panos: {}", nearby_panos.len());
        // it doesn't always cut off at exactly 3000 for some reason
        if nearby_panos.len() >= 2900 {
            if bail_on_too_many_panos {
                // too many panos! this response shouldn't be used.
                trace!("too many panos");
                return Ok(None);
            } else {
                trace!("too many panos, but we're already at the smallest pano size");
            }
        }

        for nearby_pano in nearby_panos {
            let Some(pano_id) = nearby_pano.as_array().and_then(|p| p.first()) else {
                continue;
            };
            let pano_id = pano_id.as_str().unwrap();
            let lat = nearby_pano[21][5][0][1][0][2].as_f64().unwrap();
            let lng = nearby_pano[21][5][0][1][0][3].as_f64().unwrap();
            let coords = Location::new_deg(lat, lng);

            let nearby_pano = ApiPano {
                id: pano_id.into(),
                loc: coords,
            };
            panos.push(nearby_pano);
        }
    } else {
        trace!(
            "listentityphotos response: {}",
            simd_json::to_string(&json).unwrap()
        );
        debug!("No nearby panos found");
    }
    debug!("Finished request for {coords:?}");

    Ok(Some(panos))
}

fn build_listentityphotos_request(coords: &Location, radius_meters: u32) -> Url {
    let mut pb = String::new();

    let num_panos = 3000;

    pb.push_str("!1e3"); // unknown, maybe it means request source apiv3?
    let requested_panos = [
        // copied from the SingleImageSearch request that's made when you use
        // streetViewService.getPanorama
        (2, 1, 2),
        (3, 1, 2),
        (10, 1, 2),
    ];
    write!(pb, "!5m{}", requested_panos.len() * 4 + 7).unwrap();
    {
        pb.push_str("!2m2");
        {
            // unknown
            pb.push_str("!1i203");
            pb.push_str("!1i100");
        }
        pb.push_str("!3m1");
        {
            write!(pb, "!2i{num_panos}").unwrap();
        }
        write!(pb, "!7m{}", requested_panos.len() * 4 + 1).unwrap();
        for (pano_type, tiled, image_format) in requested_panos {
            pb.push_str("!1m3");
            write!(pb, "!1e{pano_type}").unwrap();
            write!(pb, "!2b{tiled}").unwrap();
            write!(pb, "!3e{image_format}").unwrap();
        }
        pb.push_str("!2b1"); // unknown
    }
    pb.push_str("!9m2");
    {
        pb.push_str(&format!("!2d{}", coords.lng_deg()));
        pb.push_str(&format!("!3d{}", coords.lat_deg()));
    }
    pb.push_str(&format!("!10d{radius_meters}"));

    Url::parse_with_params(
        "https://www.google.com/maps/rpc/photo/listentityphotos",
        &[("authuser", "0"), ("hl", "en"), ("gl", "us"), ("pb", &pb)],
    )
    .unwrap()
}

fn build_getmetadata_request(pano_ids: &[Cow<'_, str>]) -> simd_json::OwnedValue {
    let mut queries = Vec::new();
    for pano_id in pano_ids {
        let is_third_party = is_third_party_pano(pano_id);
        let frontend = if is_third_party { 10 } else { 2 };
        // make sure it's decoded
        let pano_id = decode_protobuf_pano(pano_id);
        queries.push(json!([[frontend, pano_id]]));
    }

    #[rustfmt::skip]
    // the thing with null and [[0]] is required to make it always give us a heading
    json!([["apiv3",null,null,null,"US",null,null,null,null,null,[[0]]], ["en", "US"], queries, [6]])
}

pub fn is_third_party_pano(pano_id: &str) -> bool {
    // CIAB seems to be new, started being used likely some time before 2025-04
    pano_id.starts_with("CIHM0og") || pano_id.starts_with("CIAB") || pano_id.len() > 22
}
pub fn decode_protobuf_pano(pano_id: &str) -> Cow<'_, str> {
    if !pano_id.starts_with("CAoS") || pano_id.len() <= 22 {
        return pano_id.into();
    }
    // base64 decode
    let Ok(bytes) = BASE64_STANDARD.decode(pano_id.replace('.', "=")) else {
        return pano_id.into();
    };

    // find \x12
    let Some(tag_pos) = bytes.iter().position(|&b| b == 0x12) else {
        return pano_id.into();
    };
    let Some(&length) = bytes.get(tag_pos + 1) else {
        return pano_id.into();
    };
    let string_start = tag_pos + 2;
    let pano_id = &bytes[string_start..string_start + length as usize];
    String::from_utf8_lossy(pano_id).to_string().into()
}
pub fn encode_protobuf_pano(pano_id: &str) -> Cow<'_, str> {
    if pano_id.starts_with("CAoS") {
        return pano_id.into();
    }

    let mut data = vec![8, 10, 18];
    data.push(pano_id.len() as u8);
    data.extend(pano_id.as_bytes());
    let pano_id = BASE64_STANDARD.encode(data).replace("=", ".");
    pano_id.into()
}

static REQUESTED_GOOGLE_MAPS: OnceLock<()> = OnceLock::new();
async fn ensure_nid_cookie_set() -> eyre::Result<()> {
    if REQUESTED_GOOGLE_MAPS.get().is_some() {
        return Ok(());
    }

    info!("doing ensure_nid_cookie_set");
    let url = "https://www.google.com/maps";
    let res = CLIENT.head(url).send().await?;
    if let Some(nid) = res.cookies().find(|c| c.name() == "NID") {
        let nid = nid.value();
        fs::write("nid.txt", nid).await?;
    }
    let _ = REQUESTED_GOOGLE_MAPS.set(());

    info!("got nid cookie");

    Ok(())
}
