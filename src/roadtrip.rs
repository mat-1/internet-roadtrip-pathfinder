use std::{hash::BuildHasherDefault, sync::LazyLock};

use quick_cache::{UnitWeighter, sync::Cache};
use rustc_hash::FxHasher;
use tracing::{debug, trace};

use crate::{
    math::{self, calculate_heading, calculate_heading_diff},
    model::{Location, Pano, PanoId, PanoWithBothLocations},
    streetview::{self},
};

/// The option cache makes consecutive searches a lot faster, but it also makes
/// benchmarking harder.
const ENABLE_OPTION_CACHE: bool = true;
const OPTION_CACHE_SIZE: usize = 1024 * 1024 * 8;

// most accurate value is ceil(30 / 0.707 * 2)=85, but lowering it a little
// doesn't hurt
const MAX_SEARCH_RADIUS: f64 = 82.;

pub async fn get_options(
    cur_pano: &Pano,
    cur_heading: f32,
    allow_turnaround: bool,
    use_option_cache: bool,
) -> eyre::Result<PanoOptionsRes> {
    let mut turnaround = false;
    let mut res = get_options_no_turnaround(cur_pano, cur_heading, use_option_cache).await?;

    // turnaround
    if allow_turnaround && res.options.is_empty() {
        res = get_options_no_turnaround(cur_pano, cur_heading + 180., use_option_cache).await?;
        turnaround = true;
    }

    Ok(PanoOptionsRes {
        options: res.options,
        turnaround,
    })
}

#[allow(clippy::type_complexity)]
static GET_OPTIONS_CACHE: LazyLock<
    Cache<(u32, PanoId), BasePanoOptionsRes, UnitWeighter, BuildHasherDefault<FxHasher>>,
> = LazyLock::new(|| {
    Cache::with(
        OPTION_CACHE_SIZE,
        OPTION_CACHE_SIZE as u64,
        Default::default(),
        Default::default(),
        Default::default(),
    )
});

pub async fn get_options_no_turnaround(
    cur_pano: &Pano,
    cur_heading: f32,
    use_option_cache: bool,
) -> eyre::Result<BasePanoOptionsRes> {
    if ENABLE_OPTION_CACHE
        && use_option_cache
        && let Some(res) = GET_OPTIONS_CACHE.get(&(cur_heading.to_bits(), cur_pano.id))
    {
        return Ok(res.clone());
    }

    debug!("Doing get_options with current pano {cur_pano:?} and heading {cur_heading}");

    // this has to be done before get_getmetadata_links to make sure that all the
    // panos are cached
    let nearby_panos = streetview::get_nearby_panos(cur_pano.loc, MAX_SEARCH_RADIUS)
        .await?
        .into_iter()
        .collect::<Box<_>>();
    // we need to know this info for an optimization in get_closest_pano_forward
    // that allows us to skip panos that have their underestimated distance is too
    // high
    let origin_pano_offset =
        if let Some(origin_pano) = nearby_panos.iter().find(|p| p.id == cur_pano.id) {
            math::distance(origin_pano.actual_loc, origin_pano.search_loc)
        } else {
            0.
        };

    let mut options = Vec::<PanoOptionRes>::new();

    if let Some(links) = streetview::get_getmetadata_links(&cur_pano.id) {
        for link in links {
            let heading_diff = math::calculate_heading_diff(link.heading, cur_heading);
            if heading_diff > 100. {
                continue;
            }

            trace!("gotten link: {link:?}");
            options.push(PanoOptionRes {
                pano: link.pano,
                heading: link.heading,
            })
        }
    } else {
        debug!("get_getmetadata_links failed for {cur_pano:?}");
    }

    for direction in [0., -45., 45., 90., -90.] {
        let pano: Option<Pano> = get_closest_pano_forward(
            cur_pano.loc,
            origin_pano_offset,
            cur_heading + direction,
            13.,
            &nearby_panos,
        );

        if let Some(pano) = pano {
            if pano.id == cur_pano.id {
                continue;
            }

            let heading = calculate_heading(cur_pano.loc, pano.loc);
            let heading_diff = calculate_heading_diff(cur_heading, heading);
            if heading_diff > 100. {
                continue;
            }

            let mut too_close_to_existing_option = false;
            for option in &options {
                if option.pano.id == pano.id || option.pano.loc == pano.loc {
                    // already an option
                    too_close_to_existing_option = true;
                    break;
                }
                if calculate_heading_diff(option.heading, heading) < 15. {
                    // skip if the heading is too close to an existing one
                    too_close_to_existing_option = true;
                    break;
                }
            }

            if !too_close_to_existing_option {
                options.push(PanoOptionRes { pano, heading })
            }
        }
    }

    maybe_get_further_straight(
        cur_pano,
        origin_pano_offset,
        cur_heading,
        &mut options,
        &nearby_panos,
    );

    debug!("  options: {options:?}\n");

    let res = BasePanoOptionsRes {
        options: options.into(),
    };
    if ENABLE_OPTION_CACHE && use_option_cache {
        GET_OPTIONS_CACHE.insert((cur_heading.to_bits(), cur_pano.id), res.clone());
    }
    Ok(res)
}

fn maybe_get_further_straight(
    cur_pano: &Pano,
    origin_pano_offset: f64,
    cur_heading: f32,
    options: &mut Vec<PanoOptionRes>,
    nearby_panos: &[PanoWithBothLocations],
) {
    if options.len() > 1 {
        return;
    }
    let only_option = options.first();
    let side_check = if let Some(only_option) = only_option {
        if calculate_heading_diff(only_option.heading, cur_heading) >= 20. {
            return;
        }
        true
    } else {
        false
    };

    let distance = if side_check { 30. } else { 20. };

    let Some(further_straight) = get_closest_pano_forward(
        cur_pano.loc,
        origin_pano_offset,
        cur_heading,
        distance,
        nearby_panos,
    ) else {
        return;
    };
    if let Some(only_option) = only_option
        && only_option.pano.id == further_straight.id
    {
        // the pano further ahead is already the only option, so no point in doing any
        // of this
        return;
    }
    if further_straight.id == cur_pano.id {
        // there is no pano ahead
        return;
    }
    let further_straight_heading = calculate_heading(cur_pano.loc, further_straight.loc);
    if calculate_heading_diff(cur_heading, further_straight_heading) > 100. {
        return;
    }

    let straight_pano = PanoOptionRes {
        pano: further_straight,
        heading: further_straight_heading,
    };

    if !side_check {
        options.clear();
        options.push(straight_pano);
        return;
    }

    let mut filtered_side_panos_count = 0_usize;
    for direction in [-45., 45.] {
        let Some(pano) = get_closest_pano_forward(
            cur_pano.loc,
            origin_pano_offset,
            cur_heading + direction,
            distance / 0.707,
            nearby_panos,
        ) else {
            continue;
        };
        if pano.id == straight_pano.pano.id || pano.id == cur_pano.id {
            continue;
        }
        filtered_side_panos_count += 1;
    }

    if filtered_side_panos_count == 0 {
        options.clear();
        options.push(straight_pano);
    }
}

fn get_closest_pano_forward(
    origin_loc: Location,
    origin_pano_offset: f64,
    direction: f32,
    forward_distance: f64,
    candidate_panos: &[PanoWithBothLocations],
) -> Option<Pano> {
    let forward = math::point_at_distance(origin_loc, direction, forward_distance);

    let approx_lng_m_per_degree = origin_loc.calculate_lng_m_per_degree();

    let closest_pano = find_closest_pano(
        candidate_panos,
        forward,
        // max distance can be more than forward_distance*2 if the "search" coordinate for the
        // current position is offset by a lot
        forward_distance * 2. + origin_pano_offset,
        approx_lng_m_per_degree,
    );

    // since this function is equivalent to SingleImageSearch, we need to return
    // actual coords instead of search coords (this also makes portals possible)
    closest_pano.map(|pano| Pano {
        id: pano.id,
        loc: pano.actual_loc,
    })
}

fn find_closest_pano(
    candidate_panos: &[PanoWithBothLocations],
    loc: Location,
    max_dist: f64,
    approx_lng_m_per_degree: f64,
) -> Option<&PanoWithBothLocations> {
    let mut closest_pano: Option<&PanoWithBothLocations> = None;

    let original_max_dist_sqr = max_dist.powi(2);

    let underestimated_dists_sqr = candidate_panos
        .iter()
        .map(|p| math::underestimate_distance_sqr(p.search_loc, loc, approx_lng_m_per_degree))
        .collect::<Box<_>>();

    let nearest_underestimated_dist_sqr = *underestimated_dists_sqr
        .iter()
        .min_by_key(|d| d.to_bits())?;
    if nearest_underestimated_dist_sqr > original_max_dist_sqr {
        return None;
    }

    // 1.001 is enough to make it an overestimate
    let mut max_dist_sqr = (nearest_underestimated_dist_sqr * 1.001).min(original_max_dist_sqr);
    let mut max_dist = max_dist_sqr.sqrt();
    for (candidate_pano, &underestimated_distance_sqr) in
        candidate_panos.iter().zip(&underestimated_dists_sqr)
    {
        // fast path: if this pano is definitely farther away than closest_pano, skip
        if underestimated_distance_sqr > max_dist_sqr {
            continue;
        }

        let distance_to_forward = math::distance(candidate_pano.search_loc, loc);

        if let Some(closest_pano) = &mut closest_pano {
            if distance_to_forward < max_dist {
                *closest_pano = candidate_pano;
                max_dist = distance_to_forward;
                max_dist_sqr = max_dist.powi(2);
            }
        } else {
            closest_pano = Some(candidate_pano);
            max_dist = distance_to_forward;
            max_dist_sqr = max_dist.powi(2);
        }
    }

    if closest_pano.is_some() {
        return closest_pano;
    }

    panic!(
        "overestimated pano wasn't actually an overestimate: {nearest_underestimated_dist_sqr}, {loc:?}, {candidate_panos:?}"
    )
}

#[derive(Debug, Clone)]
pub struct BasePanoOptionsRes {
    pub options: Box<[PanoOptionRes]>,
}
#[derive(Debug, Clone)]
pub struct PanoOptionsRes {
    pub options: Box<[PanoOptionRes]>,
    pub turnaround: bool,
}
#[derive(Debug, Clone)]
pub struct PanoOptionRes {
    pub pano: Pano,
    pub heading: f32,
}
