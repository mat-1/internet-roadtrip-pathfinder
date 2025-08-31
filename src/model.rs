use std::{
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use compact_str::CompactString;
use serde::Serialize;

use crate::{
    db::DB,
    math::{self, angle::Angle},
};

#[derive(Debug, Clone, Copy, PartialEq, Hash, Serialize)]
pub struct Location {
    /// y
    ///
    /// Represented in a way where 180° -> i32::MAX, etc.
    pub lat: Angle,
    /// x
    pub lng: Angle,
}
impl Location {
    #[inline]
    pub const fn new_deg(lat: f64, lng: f64) -> Self {
        Self {
            lat: Angle::from_deg(lat),
            lng: Angle::from_deg(lng),
        }
    }

    #[inline]
    pub const fn new(lat: Angle, lng: Angle) -> Self {
        Location { lat, lng }
    }

    #[inline]
    pub const fn lat_deg(self) -> f64 {
        self.lat.to_deg()
    }
    #[inline]
    pub const fn lng_deg(self) -> f64 {
        self.lng.to_deg()
    }

    #[inline]
    pub const fn lat_rad(self) -> f64 {
        self.lat.to_rad()
    }
    #[inline]
    pub const fn lng_rad(self) -> f64 {
        self.lng.to_rad()
    }

    pub fn to_geojson(&self) -> [f32; 2] {
        [self.lng_deg() as f32, self.lat_deg() as f32]
    }

    pub fn from_latlng(latlng: [f64; 2]) -> Self {
        Location::new_deg(latlng[0], latlng[1])
    }

    #[inline]
    pub fn to_radians(&self) -> LocationRadians {
        LocationRadians {
            lat: self.lat_rad(),
            lng: self.lng_rad(),
        }
    }

    #[inline]
    pub fn distance_to(&self, other: Location) -> f64 {
        math::distance(*self, other)
    }

    #[inline]
    pub const fn with_lat(self, lat: Angle) -> Self {
        Self { lat, ..self }
    }
    #[inline]
    pub const fn with_lng(self, lng: Angle) -> Self {
        Self { lng, ..self }
    }

    /// Returns the derivative of the longitude/degree for the current location
    /// (based on the latitude).
    ///
    /// This is used for calculating approximations of short distances.
    #[inline]
    pub fn calculate_lng_m_per_degree(self) -> f64 {
        self.lat.calculate_lng_m_per_degree()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocationRadians {
    /// y
    pub lat: f64,
    /// x
    pub lng: f64,
}

impl Eq for Location {}
impl Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.lat_deg(), self.lng_deg())
    }
}

// pano ids are converted into a u32 (through the database) and kept that way
// for efficiency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct PanoId(pub u32);
impl PanoId {
    /// Whether the pano is a photosphere (id starts with CIHM/CIAB).
    pub fn is_photosphere(&self) -> bool {
        (self.0 >> 31) == 1
    }
}
impl From<&str> for PanoId {
    fn from(value: &str) -> Self {
        DB.get_pano_id(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiPanoId(pub CompactString);
impl From<&str> for ApiPanoId {
    fn from(value: &str) -> Self {
        Self(CompactString::from(value))
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Pano {
    pub id: PanoId,
    pub loc: Location,
}
impl Eq for Pano {}
impl Hash for Pano {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        // self.loc.hash(state);
    }
}
impl PartialEq for Pano {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
        // && self.loc == other.loc
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PanoWithBothLocations {
    pub id: PanoId,
    pub search_loc: Location,
    pub actual_loc: Location,
}
impl Hash for PanoWithBothLocations {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl PartialEq for PanoWithBothLocations {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for PanoWithBothLocations {}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct ApiPano {
    pub id: ApiPanoId,
    pub loc: Location,
}

#[derive(Debug, Clone)]
pub struct GetMetadataResponse {
    pub id: PanoId,
    pub loc: Location,
    pub links: Vec<PanoLink>,
}
#[derive(Debug, Clone)]
pub struct PanoLink {
    /// For GetMetadata links, the location will be an "actual" loc.
    pub pano: Pano,
    pub heading: f32,
}
#[derive(Debug, Clone)]
pub struct PanoWithTile {
    pub id: PanoId,
    pub tile: SmallTile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmallTile {
    /// lng
    pub x: u32,
    /// lat
    pub y: u32,
}

// google uses 17 (corner is ~157m from center)
pub const SMALL_TILE_SIZE: u8 = 16;
// the optimal value for this depends on how many panos are around the location
// that it's pathing through. 13 or 14 is usually best.
pub const LARGEST_TILE_SIZE: u8 = 13;
const SMALL_SCALE: f64 = (1 << SMALL_TILE_SIZE) as f64;
const PI: f64 = std::f64::consts::PI;

impl SmallTile {
    pub fn from_loc(loc: Location) -> Self {
        let lat_rad = loc.lat_rad();
        let x = (loc.lng_deg() + 180.) * SMALL_SCALE / 360.;
        let y = (1.0 - lat_rad.tan().asinh() / PI) * SMALL_SCALE / 2.;

        Self {
            x: x as u32,
            y: y as u32,
        }
    }

    pub fn is_maybe_within_radius(&self, loc: Location, radius: f64) -> bool {
        let base_tile_loc = self.to_loc();
        let down_right_tile_loc = self.down().right().to_loc();

        let min_coords = Location::new(
            base_tile_loc.lat.min(down_right_tile_loc.lat),
            base_tile_loc.lng.min(down_right_tile_loc.lng),
        );
        let max_coords = Location::new(
            base_tile_loc.lat.max(down_right_tile_loc.lat),
            base_tile_loc.lng.max(down_right_tile_loc.lng),
        );

        // fast check for if the coords are within the tile
        if loc.lat >= min_coords.lat
            && loc.lat <= max_coords.lat
            && loc.lng >= min_coords.lng
            && loc.lng <= max_coords.lng
        {
            return true;
        }

        let closest_coord_in_tile_to_coords = Location::new(
            loc.lat.clamp(min_coords.lat, max_coords.lat),
            loc.lng.clamp(min_coords.lng, max_coords.lng),
        );

        math::is_at_least_within_radius(
            loc,
            closest_coord_in_tile_to_coords,
            radius,
            loc.calculate_lng_m_per_degree(),
        )
    }

    #[inline]
    pub fn to_loc(&self) -> Location {
        let lng = self.x as f64 / SMALL_SCALE * 360. - 180.;

        let lat = (PI * (1.0 - 2.0 * self.y as f64 / SMALL_SCALE))
            .sinh()
            .atan()
            .to_degrees();

        Location::new_deg(lat, lng)
    }

    pub fn down(&self) -> Self {
        Self {
            x: self.x,
            y: self.y + 1,
        }
    }
    pub fn up(&self) -> Self {
        Self {
            x: self.x,
            y: self.y - 1,
        }
    }
    pub fn left(&self) -> Self {
        Self {
            x: self.x - 1,
            y: self.y,
        }
    }
    pub fn right(&self) -> Self {
        Self {
            x: self.x + 1,
            y: self.y,
        }
    }

    pub fn get_all_sizes(&self) -> Box<[SizedTile]> {
        let mut all_sizes = Vec::new();

        let mut cur = SizedTile::from(*self);
        all_sizes.push(cur);
        while cur.size != LARGEST_TILE_SIZE {
            cur = cur.next_larger();
            all_sizes.push(cur);
        }

        // largest first
        all_sizes.reverse();

        all_sizes.into_boxed_slice()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SizedTile {
    pub size: u8,
    /// lng
    pub x: u32,
    /// lat
    pub y: u32,
}
impl SizedTile {
    pub fn next_larger(&self) -> SizedTile {
        SizedTile {
            size: self.size - 1,
            x: self.x / 2,
            y: self.y / 2,
        }
    }
    pub fn coords_at_center(&self) -> Location {
        let scale = self.scale();

        let lat_rad = f64::atan(f64::sinh(PI * (1.0 - 2.0 * (self.y as f64 + 0.5) / scale)));
        let lng_deg = (self.x as f64 + 0.5) / scale * 360.0 - 180.0;

        Location::new(Angle::from_rad(lat_rad), Angle::from_deg(lng_deg))
    }

    pub fn to_coords(&self) -> Location {
        let scale = self.scale();

        let lat_rad = (PI * (1.0 - 2.0 * self.y as f64 / scale)).sinh().atan();
        let lng_deg = self.x as f64 / scale * 360. - 180.;

        Location::new(Angle::from_rad(lat_rad), Angle::from_deg(lng_deg))
    }

    pub fn distance_from_corner_to_center(&self) -> f64 {
        self.to_coords().distance_to(self.coords_at_center())
    }

    fn scale(&self) -> f64 {
        (1 << self.size) as f64
    }
}
impl From<SmallTile> for SizedTile {
    fn from(t: SmallTile) -> Self {
        SizedTile {
            size: SMALL_TILE_SIZE,
            x: t.x,
            y: t.y,
        }
    }
}

#[cfg(test)]
mod tests {
    use geo::Distance as _;

    use super::*;

    #[test]
    fn test_tile_to_and_from_coords_matches() {
        for lat in -100..100 {
            for lng in -100..100 {
                let lat = lat as f64 / 100.;
                let lng = lng as f64 / 100.;
                let loc = Location::new_deg(lat, lng);

                let tile = SmallTile::from_loc(loc);
                println!("location: {loc:?}");
                assert!(
                    tile.is_maybe_within_radius(loc, 1.),
                    "{loc:?} wasn't in {tile:?}"
                );
                println!("---");
            }
        }
    }

    #[test]
    fn test_location_accuracy() {
        let (lat, lng) = (47.45647413331853, -69.99669220097549);
        let loc = Location::new_deg(lat, lng);
        let (new_lat, new_lng) = (loc.lat_deg(), loc.lng_deg());

        let dist =
            geo::Haversine.distance(geo::Point::new(lng, lat), geo::Point::new(new_lng, new_lat));
        assert!(dist < 0.01);
    }
}
