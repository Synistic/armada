use core::ops::{Add, AddAssign, Neg, Sub};

use serde::{Deserialize, Serialize};

use crate::fixed::Fx;

/// Battle plane coordinates. The tactical sim is 2D on purpose: 3D movement in
/// space looks nice and adds almost nothing to the tactics while costing a lot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: Fx,
    pub y: Fx,
}

impl Vec2 {
    pub const ZERO: Self = Self::new(Fx::ZERO, Fx::ZERO);

    pub const fn new(x: Fx, y: Fx) -> Self {
        Self { x, y }
    }

    pub fn dot(self, o: Self) -> Fx {
        self.x * o.x + self.y * o.y
    }

    pub fn len_sq(self) -> Fx {
        self.dot(self)
    }

    pub fn len(self) -> Fx {
        self.len_sq().sqrt()
    }

    pub fn scale(self, s: Fx) -> Self {
        Self::new(self.x * s, self.y * s)
    }

    pub fn normalize_or_zero(self) -> Self {
        let l = self.len();
        if l == Fx::ZERO {
            Self::ZERO
        } else {
            Self::new(self.x / l, self.y / l)
        }
    }

    pub fn clamp_len(self, max: Fx) -> Self {
        if self.len_sq() <= max * max {
            self
        } else {
            self.normalize_or_zero().scale(max)
        }
    }

    pub fn clamp_box(self, half: Fx) -> Self {
        Self::new(self.x.clamp(-half, half), self.y.clamp(-half, half))
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}
