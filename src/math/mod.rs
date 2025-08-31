pub mod angle;

use std::f64::consts::PI;

use crate::model::{Location, LocationRadians};

#[inline]
pub fn calculate_heading(origin: Location, dest: Location) -> f32 {
    (calculate_heading_radians(origin.to_radians(), dest.to_radians()).to_degrees() + 360.) % 360.
}
#[inline]
pub fn calculate_heading_radians(origin: LocationRadians, dest: LocationRadians) -> f32 {
    // based on `geo::Haversine.bearing(start, end)`

    let (a_lng, a_lat) = (origin.lng, origin.lat);
    let (b_lng, b_lat) = (dest.lng, dest.lat);
    let delta_lng = b_lng - a_lng;

    let (a_lat_sin, a_lat_cos) = a_lat.sin_cos();
    let (b_lat_sin, b_lat_cos) = b_lat.sin_cos();
    let (delta_lng_sin, delta_lng_cos) = delta_lng.sin_cos();

    let s = delta_lng_sin * b_lat_cos;
    let c = a_lat_cos * b_lat_sin - a_lat_sin * b_lat_cos * delta_lng_cos;

    s.atan2(c) as f32
}
pub fn calculate_heading_diff(a: f32, b: f32) -> f32 {
    let a = a % 360.;
    let b = b % 360.;

    let mut diff = (a - b).abs();
    if diff > 180. {
        diff = 360. - diff;
    }
    diff
}

/// An alternative to [`distance`] that has faster checks for checking if the
/// coordinates are far enough apart from each other.
pub fn distance_if_within_radius(a: Location, b: Location, radius: f64) -> Option<f64> {
    if !is_at_least_within_radius(a, b, radius, a.calculate_lng_m_per_degree()) {
        return None;
    }

    let dist = distance(a, b);
    if dist > radius {
        return None;
    }

    Some(dist)
}

/// In meters, copied from Google Maps's code.
const EARTH_RADIUS: f64 = 6_378_137.;

#[inline]
pub fn is_at_least_within_radius(
    a: Location,
    b: Location,
    radius: f64,
    approx_lng_m_per_degree: f64,
) -> bool {
    is_at_least_within_radius_sqr(a, b, radius.powi(2), approx_lng_m_per_degree)
}

#[inline]
pub fn is_at_least_within_radius_sqr(
    a: Location,
    b: Location,
    radius: f64,
    approx_lng_m_per_degree: f64,
) -> bool {
    underestimate_distance_sqr(a, b, approx_lng_m_per_degree) <= radius
}

/// Latitude lines are always spaced evenly apart, so this doesn't need to be an
/// approximation.
pub const LAT_M_PER_DEGREE: f64 = EARTH_RADIUS * (PI / 180.);

#[inline]
pub fn calculate_lng_m_per_degree(lat: f64) -> f64 {
    LAT_M_PER_DEGREE * lat.to_radians().cos()
}

#[inline]
pub fn underestimate_distance_sqr(a: Location, b: Location, approx_lng_m_per_degree: f64) -> f64 {
    let lat_diff = (a.lat - b.lat).to_deg();
    let lng_diff = (a.lng - b.lng).to_deg();

    (lat_diff * (LAT_M_PER_DEGREE * 0.999)).powi(2)
        + (lng_diff * (approx_lng_m_per_degree * 0.999)).powi(2)
}
#[inline]
pub fn approx_distance_sqr(a: Location, b: Location, approx_lng_m_per_degree: f64) -> f64 {
    let lat_diff = (a.lat - b.lat).to_deg();
    let lng_diff = (a.lng - b.lng).to_deg();

    (lat_diff * LAT_M_PER_DEGREE).powi(2) + (lng_diff * approx_lng_m_per_degree).powi(2)
}

#[inline]
pub fn distance(a: Location, b: Location) -> f64 {
    // based on geo::Haversine.distance(a, b)

    let a_lat_rad = a.lat_rad();
    let a_lng_rad = a.lng_rad();

    let b_lat_rad = b.lat_rad();
    let b_lng_rad = b.lng_rad();

    let theta1 = a_lat_rad as f32;
    let theta2 = b_lat_rad as f32;
    let delta_theta = (b_lat_rad - a_lat_rad) as f32;
    let delta_lambda = (b_lng_rad - a_lng_rad) as f32;

    let a = (delta_theta / 2.).sin().powi(2)
        + theta1.cos() * theta2.cos() * (delta_lambda / 2.).sin().powi(2);
    let c = 2. * a.sqrt().asin();
    EARTH_RADIUS * (c as f64)
}

pub fn point_at_distance(pos: Location, direction: f32, distance: f64) -> Location {
    point_at_distance_radians(pos, (direction as f64).to_radians(), distance)
}

pub fn point_at_distance_radians(pos: Location, direction_radians: f64, distance: f64) -> Location {
    let lat = pos.lat_rad();
    let lng = pos.lng_rad();

    let d = distance / EARTH_RADIUS;
    let cos_d = d.cos();
    let sin_d = d.sin();
    let cos_lat = lat.cos();
    let sin_lat = lat.sin();
    let sin_d_cos_lat = sin_d * cos_lat;
    let return_lat = (cos_d * sin_lat + sin_d_cos_lat * direction_radians.cos()).asin();
    let return_lng = lng
        + f64::atan2(
            direction_radians.sin() * sin_d_cos_lat,
            cos_d - sin_lat * return_lat.sin(),
        );

    Location::new_deg(return_lat.to_degrees(), return_lng.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overestimate_distance_sqr() {
        for lat in 20..60 {
            for lng in -80..-40 {
                let loc_a = Location::new_deg(lat as f64, lng as f64);
                let approx_lng_m_per_degree = loc_a.calculate_lng_m_per_degree();
                for offset_lat in -10..10 {
                    for offset_lng in -10..10 {
                        let loc_b = Location::new_deg(
                            loc_a.lat_deg() + (offset_lat as f64) / 100.,
                            loc_a.lng_deg() + (offset_lng as f64) / 100.,
                        );

                        let calculated_underestimate =
                            underestimate_distance_sqr(loc_a, loc_b, approx_lng_m_per_degree)
                                .sqrt();
                        let calculated_actual = loc_a.distance_to(loc_b);

                        assert!(
                            calculated_underestimate <= calculated_actual,
                            "{calculated_underestimate} <= {calculated_actual}"
                        );
                    }
                }
            }
        }
    }
}
