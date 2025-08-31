use std::{
    f64::consts,
    fmt::{self, Debug, Display},
    num,
    ops::{Add, Sub},
    str::FromStr,
};

use serde::Serialize;

use crate::math::LAT_M_PER_DEGREE;

/// A precise and compact representation of an angle, as an alternative to an
/// f64 that's between -180 to 180.
#[derive(Clone, Copy, PartialEq, Hash, Eq, PartialOrd, Ord, Serialize)]
pub struct Angle(i32);
impl Angle {
    #[inline]
    pub const fn from_deg(deg: f64) -> Self {
        Self((deg * (i32::MAX as f64 / 180.)) as i32)
    }
    #[inline]
    pub const fn from_rad(rad: f64) -> Self {
        Self((rad * (i32::MAX as f64 / consts::PI)) as i32)
    }
    #[inline]
    pub const fn to_deg(self) -> f64 {
        (self.0 as f64) * (180. / i32::MAX as f64)
    }
    #[inline]
    pub const fn to_rad(self) -> f64 {
        (self.0 as f64) * (consts::PI / i32::MAX as f64)
    }

    /// Returns the derivative of the longitude/degree, assuming that this angle
    /// is for the latitude.
    ///
    /// This is used for calculating approximations of short distances.
    #[inline]
    pub fn calculate_lng_m_per_degree(self) -> f64 {
        LAT_M_PER_DEGREE * self.to_rad().cos()
    }

    pub fn from_bits(i: i32) -> Self {
        Angle(i)
    }
    /// Returns the internal representation of the angle.
    #[inline]
    pub fn to_bits(self) -> i32 {
        self.0
    }
}
impl Debug for Angle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}°", self.to_deg())
    }
}
impl Display for Angle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}°", self.to_deg())
    }
}
impl FromStr for Angle {
    type Err = num::ParseFloatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        f64::from_str(s).map(Angle::from_deg)
    }
}

impl Add for Angle {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Angle(self.0 + rhs.0)
    }
}
impl Sub for Angle {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Angle(self.0 - rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::calculate_lng_m_per_degree;

    #[test]
    fn test_i_angle_accuracy() {
        for deg in -179..180 {
            let deg = deg as f64;
            let deg_i = Angle::from_deg(deg);
            let deg_i_to_deg = deg_i.to_deg();
            let deg_i_to_deg_to_i = Angle::from_deg(deg_i_to_deg);

            assert_eq!(deg_i, deg_i_to_deg_to_i);

            let lng_m_per_degree = calculate_lng_m_per_degree(deg);
            let lng_m_per_degree_i = deg_i.calculate_lng_m_per_degree();

            assert!(
                (lng_m_per_degree - lng_m_per_degree_i).abs() < 0.001,
                "{lng_m_per_degree} - {lng_m_per_degree_i}"
            );
        }
    }
}
