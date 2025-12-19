pub mod api;

use std::{
    cmp::Ordering,
    sync::{Arc, LazyLock},
};

use coarsetime::Instant;
use quick_cache::sync::Cache;
use tracing::{debug, trace, warn};

use crate::{
    db::DB,
    math::{self, LAT_M_PER_DEGREE, angle::Angle},
    model::{
        ApiPanoId, GetMetadataResponse, Location, Pano, PanoId, PanoLink, PanoWithBothLocations,
        SizedTile, SmallTile,
    },
};

pub fn get_getmetadata_links(pano_id: &PanoId) -> Option<Box<[PanoLink]>> {
    DB.lookup_getmetadata(pano_id).map(|(_, l)| l)
}

pub async fn get_nearest_pano(loc: Location, max_distance: f64) -> eyre::Result<Option<Pano>> {
    let panos = get_nearby_panos(loc, max_distance).await?;
    Ok(get_nearest_pano_in_array(&panos, loc, None))
}

pub async fn get_nearby_panos(
    loc: Location,
    min_distance: f64,
) -> eyre::Result<Box<[PanoWithBothLocations]>> {
    let mut found_panos = Vec::<PanoWithBothLocations>::new();
    let mut checked_tiles = Vec::new();

    let origin_tile = SmallTile::from_loc(loc);
    let (min_lat, max_lat) = calculate_lat_bounds(loc, min_distance);
    let (min_tile, max_tile) = calculate_tile_bounds(loc, min_distance);

    for x in min_tile.x..=max_tile.x {
        for y in min_tile.y..=max_tile.y {
            let tile = SmallTile { x, y };
            if tile != origin_tile && !tile.is_maybe_within_radius(loc, min_distance) {
                continue;
            }

            // note if you're trying to optimize this: for normal pathfinding, it's not
            // faster to spawn these as tasks
            let (checked_sized_tile, panos_at_this_tile) = get_panos_at_tile(tile).await?;
            if checked_tiles.contains(&checked_sized_tile) {
                continue;
            }
            checked_tiles.push(checked_sized_tile);

            filter_panos_at_tile_into(
                loc,
                &panos_at_this_tile,
                min_lat,
                max_lat,
                min_distance,
                &mut found_panos,
            );
        }
    }

    Ok(found_panos.into())
}

/// Re-download the panos within at least min_distance meters of the given
/// location.
pub async fn reset_cache_nearby(loc: Location, min_distance: f64) -> eyre::Result<()> {
    debug!("doing reset_cache_nearby at {loc:?}");

    let mut checked_tiles = Vec::new();

    let origin_tile = SmallTile::from_loc(loc);
    let (min_tile, max_tile) = calculate_tile_bounds(loc, min_distance);

    for x in min_tile.x..=max_tile.x {
        for y in min_tile.y..=max_tile.y {
            let tile = SmallTile { x, y };
            if tile != origin_tile && !tile.is_maybe_within_radius(loc, min_distance) {
                continue;
            }

            let (checked_sized_tile, _) = get_panos_at_tile(tile).await?;
            if checked_tiles.contains(&checked_sized_tile) {
                continue;
            }
            checked_tiles.push(checked_sized_tile);

            // only refetch the one with content
            PANOS_AT_TILE_CACHE.remove(&checked_sized_tile);
            // it's possible for the tile to be too big now (>3000 panos), but that's fine
            // since the smaller tile would get requested when next time it's needed anyways
            uncached_get_panos_at_sized_tile(checked_sized_tile).await?;
        }
    }

    Ok(())
}

fn calculate_tile_bounds(loc: Location, min_distance: f64) -> (SmallTile, SmallTile) {
    let (min_lat, max_lat) = calculate_lat_bounds(loc, min_distance);
    let (min_lng, max_lng) = calculate_lng_bounds(loc, min_distance);

    let tile_a = SmallTile::from_loc(Location::new(min_lat, min_lng));
    let tile_b = SmallTile::from_loc(Location::new(max_lat, max_lng));

    let min_tile = SmallTile {
        x: tile_a.x.min(tile_b.x),
        y: tile_a.y.min(tile_b.y),
    };
    let max_tile = SmallTile {
        x: tile_a.x.max(tile_b.x),
        y: tile_a.y.max(tile_b.y),
    };

    (min_tile, max_tile)
}

#[inline(always)]
fn calculate_lng_bounds(loc: Location, min_distance: f64) -> (Angle, Angle) {
    let max_lat = calculate_lat_bounds(loc, min_distance).1;

    // calculate lng accurately
    let point_up = math::point_at_distance(loc.with_lat(max_lat), 90., min_distance * 1.01);
    let point_down = math::point_at_distance(loc, 270., min_distance * 1.01);
    let max_lng = point_up.lng;
    let min_lng = point_down.lng;

    (min_lng, max_lng)
}

#[inline(always)]
fn calculate_lat_bounds(loc: Location, min_distance: f64) -> (Angle, Angle) {
    let lat_diff = Angle::from_deg((min_distance * 1.01) / LAT_M_PER_DEGREE);
    let max_lat = loc.lat + lat_diff;
    let min_lat = loc.lat - lat_diff;

    (min_lat, max_lat)
}

fn filter_panos_at_tile_into(
    loc: Location,
    panos_at_tile: &[PanoWithBothLocations],
    min_lat: Angle,
    max_lat: Angle,
    max_distance: f64,
    collect_into: &mut Vec<PanoWithBothLocations>,
) {
    let lng_m_per_degree = loc.calculate_lng_m_per_degree();

    // this optimization brings down the number of panos to check with
    // is_at_least_within_radius from ~200-2000 to ~30-150. yippee!
    let first_within_lat = panos_at_tile
        .binary_search_by(|p| match p.search_loc.lat.cmp(&min_lat) {
            Ordering::Equal => Ordering::Less,
            o => o,
        })
        .unwrap_err();
    if first_within_lat >= panos_at_tile.len() {
        // no panos here
        return;
    }

    let first_outside_lat = panos_at_tile
        .binary_search_by(|p| match p.search_loc.lat.cmp(&max_lat) {
            Ordering::Equal => Ordering::Less,
            o => o,
        })
        .unwrap_err();

    collect_into.extend(
        panos_at_tile[first_within_lat..first_outside_lat]
            .iter()
            .filter_map(|p| {
                if math::is_at_least_within_radius(
                    loc,
                    p.search_loc,
                    max_distance,
                    lng_m_per_degree,
                ) {
                    Some(p.clone())
                } else {
                    None
                }
            }),
    );
}

#[must_use]
pub fn get_nearest_pano_in_array(
    panos: &[PanoWithBothLocations],
    origin: Location,
    max_distance: Option<f64>,
) -> Option<Pano> {
    let approximate_lng_per_degrees = origin.calculate_lng_m_per_degree();

    let mut nearest_pano = None;
    let mut nearest_pano_distance = f64::MAX;

    for pano in panos {
        let search_loc = pano.search_loc;
        let pano = Pano {
            id: pano.id,
            loc: pano.actual_loc,
        };

        let dist = if let Some(max_distance) = max_distance {
            math::distance_if_within_radius(origin, search_loc, max_distance).unwrap_or(f64::MAX)
        } else {
            // this isn't perfect, but it doesn't significantly hurt accuracy and it
            // provides a modest speedup (~8%)
            math::underestimate_distance_sqr(origin, search_loc, approximate_lng_per_degrees)
        };
        if dist < nearest_pano_distance {
            nearest_pano_distance = dist;
            nearest_pano = Some(pano);
        }
    }

    nearest_pano
}

#[allow(clippy::type_complexity)]
static PANOS_AT_TILE_CACHE: LazyLock<Cache<SizedTile, Option<Arc<[PanoWithBothLocations]>>>> =
    LazyLock::new(|| Cache::new(1024));

/// Returns a list of panos that are at least in the tile (but might be in
/// surrounding ones), as well as the [`SizedTile`] that contains these tiles.
pub async fn get_panos_at_tile(
    base_tile: SmallTile,
) -> eyre::Result<(SizedTile, Arc<[PanoWithBothLocations]>)> {
    let mut found_tile_and_res = None;

    for tile in base_tile.get_all_sizes() {
        trace!("internal_get_panos_at_tile {tile:?}");
        if let Some(res) = PANOS_AT_TILE_CACHE.get(&tile) {
            if let Some(res) = res {
                trace!("got from cache ({} panos), returning", res.len());
                found_tile_and_res = Some((tile, res.clone()));
                break;
            }
            continue;
        }

        if let Some(res) = DB.lookup_listentityphotos(&tile) {
            PANOS_AT_TILE_CACHE.insert(tile, res.clone());
            if let Some(res) = res {
                trace!("got from cache ({} panos), returning", res.len());
                found_tile_and_res = Some((tile, res));
                break;
            }
            trace!("got from cache (too many panos), continuing");
        } else if let Some(res) = uncached_get_panos_at_sized_tile(tile).await? {
            found_tile_and_res = Some((tile, res));
            break;
        }

        // it was None so keep checking
    }

    let (tile, res) = found_tile_and_res.unwrap_or_else(|| {
        panic!("tile {base_tile:?} had too many panos? SMALL_TILE_SIZE might have to be changed")
    });

    Ok((tile, res))
}

async fn uncached_get_panos_at_sized_tile(
    tile: SizedTile,
) -> eyre::Result<Option<Arc<[PanoWithBothLocations]>>> {
    debug!("uncached_get_panos_at_sized_tile at {tile:?}");
    let res = api::try_get_panos_at_tile(tile).await;

    let api_res = match res {
        Ok(r) => r,
        Err(err) => {
            warn!("api request returned an error: {err}");
            return Err(err);
        }
    };

    // convert the streetview ids (strings) into pathfinder ones (u32s)
    if let Some(api_res) = api_res {
        let mut txn = DB.write_txn();
        let mut converted_res = Vec::new();
        for pano in api_res.iter() {
            converted_res.push(Pano {
                id: DB.get_pano_id_with_txn(&mut txn, &pano.id.0),
                loc: pano.loc,
            })
        }
        txn.commit()?;

        // do GetMetadata lookups on all the panos and save them in the db
        let pano_ids = api_res.iter().map(|p| &p.id).cloned().collect::<Box<[_]>>();
        fetch_getmetadata_with_pano_ids(&pano_ids).await?;

        // now add both types of locations to our panos
        let res = fetch_actual_locations_for_panos(tile, &converted_res);

        // we include both types of coordinates when we save the listentityphotos
        // response to reduce the number of lookups we have to do later
        DB.save_listentityphotos(&tile, Some(res.clone()))?;

        return Ok(Some(res));
    }

    DB.save_listentityphotos(&tile, None)?;

    Ok(None)
}

fn fetch_actual_locations_for_panos(
    tile: SizedTile,
    panos: &[Pano],
) -> Arc<[PanoWithBothLocations]> {
    let txn = DB.read_txn();
    let res = panos
        .iter()
        .map(|p| {
            let actual_loc = DB
                .lookup_getmetadata_location_with_txn(&txn, &p.id)
                .unwrap_or(p.loc);

            PanoWithBothLocations {
                id: p.id,
                search_loc: p.loc,
                actual_loc,
            }
        })
        .collect::<Arc<_>>();
    PANOS_AT_TILE_CACHE.insert(tile, Some(res.clone()));
    res
}

async fn fetch_getmetadata_with_pano_ids(
    pano_ids: &[ApiPanoId],
) -> eyre::Result<Arc<[GetMetadataResponse]>> {
    let start = Instant::now();

    let mut getmetadata_responses = Vec::new();

    let mut tasks = Vec::new();
    // getmetadata refuses to reply if we request more than 200 at a time
    for chunk in pano_ids.chunks(200) {
        let chunk = chunk.to_vec();
        tasks.push(tokio::spawn(async move {
            api::fetch_getmetadata_responses(&chunk).await
        }));
        // all_links.extend(api::fetch_getmetadata_links(&chunk).await?);
    }
    for task in tasks {
        getmetadata_responses.extend(task.await??);
    }

    debug!("Requests for GetMetadata took: {:?}", start.elapsed());

    let mut txn = DB.write_txn();
    for getmetadata_response in &getmetadata_responses {
        DB.save_getmetadata_with_txn(&mut txn, getmetadata_response)?;
    }
    txn.commit()?;

    Ok(Arc::<[GetMetadataResponse]>::from(getmetadata_responses))
}
