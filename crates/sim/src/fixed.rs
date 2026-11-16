use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

const FRAC_BITS: u32 = 16;
const ONE_RAW: i64 = 1 << FRAC_BITS;

/// 48.16 fixed point. Range is about ±1.4e14 m with 1/65536 m resolution,
/// so a 100 km arena has plenty of headroom even for squared distances.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Fx(pub i64);

impl Fx {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(ONE_RAW);

    pub const fn from_int(v: i64) -> Self {
        Self(v << FRAC_BITS)
    }

    pub const fn to_int(self) -> i64 {
        self.0 >> FRAC_BITS
    }

    pub fn sqrt(self) -> Self {
        if self.0 <= 0 {
            return Self::ZERO;
        }
        Self((((self.0 as u128) << FRAC_BITS).isqrt()) as i64)
    }

    /// Load time only. IEEE multiply and round are exactly specified, so this is
    /// still deterministic, but floats never run inside a tick.
    #[allow(clippy::float_arithmetic)]
    pub fn from_f64(v: f64) -> Self {
        Self((v * ONE_RAW as f64).round() as i64)
    }

    /// Presentation only.
    #[allow(clippy::float_arithmetic)]
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / ONE_RAW as f32
    }
}

impl Add for Fx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Fx {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Fx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Fx {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Neg for Fx {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Mul for Fx {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(((i128::from(self.0) * i128::from(rhs.0)) >> FRAC_BITS) as i64)
    }
}

impl Div for Fx {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self(((i128::from(self.0) << FRAC_BITS) / i128::from(rhs.0)) as i64)
    }
}

impl Mul<i64> for Fx {
    type Output = Self;
    fn mul(self, rhs: i64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Div<i64> for Fx {
    type Output = Self;
    fn div(self, rhs: i64) -> Self {
        Self(self.0 / rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::Fx;

    #[test]
    fn arithmetic() {
        let a = Fx::from_int(6);
        let b = Fx::from_int(4);
        assert_eq!(a * b, Fx::from_int(24));
        assert_eq!(a / b, Fx::from_f64(1.5));
        assert_eq!(Fx::from_int(81).sqrt(), Fx::from_int(9));
        assert_eq!((-a).to_int(), -6);
    }
}
