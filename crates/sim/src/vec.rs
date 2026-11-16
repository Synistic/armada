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

    /// Positive when `o` lies counterclockwise of `self`.
    pub fn cross(self, o: Self) -> Fx {
        self.x * o.y - self.y * o.x
    }

    /// Counterclockwise rotation by the angle whose cosine and sine are given.
    pub fn rotate(self, cos: Fx, sin: Fx) -> Self {
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    // Ship frame: +y is forward (the heading), +x is starboard.

    /// World direction into the frame of a ship with unit `heading`.
    pub fn to_local(self, heading: Self) -> Self {
        let right = Self::new(heading.y, -heading.x);
        Self::new(self.dot(right), self.dot(heading))
    }

    /// Ship frame offset into world space for a ship with unit `heading`.
    pub fn from_local(self, heading: Self) -> Self {
        Self::new(
            self.x * heading.y + self.y * heading.x,
            self.y * heading.y - self.x * heading.x,
        )
    }

    /// The heading that puts world direction `self` at ship frame `bearing`.
    pub fn heading_for_bearing(self, bearing: Self) -> Self {
        Self::new(
            bearing.y * self.x - bearing.x * self.y,
            bearing.x * self.x + bearing.y * self.y,
        )
    }

    /// Turns unit vector `self` toward unit `goal` by at most one step.
    pub fn turn_towards(self, goal: Self, step_cos: Fx, step_sin: Fx) -> Self {
        if goal == Self::ZERO {
            return self;
        }
        if self.dot(goal) >= step_cos {
            return goal;
        }
        let sin = if self.cross(goal) > Fx::ZERO {
            step_sin
        } else {
            -step_sin
        };
        self.rotate(step_cos, sin).normalize_or_zero()
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

#[cfg(test)]
mod tests {
    use super::Vec2;
    use crate::fixed::Fx;

    fn v(x: i64, y: i64) -> Vec2 {
        Vec2::new(Fx::from_int(x), Fx::from_int(y))
    }

    #[test]
    fn frames_roundtrip() {
        let west = v(-1, 0);
        // Facing west, starboard is north.
        assert_eq!(v(0, 1).to_local(west), v(1, 0));
        assert_eq!(v(1, 0).from_local(west), v(0, 1));
        // Target north on the starboard beam means heading west.
        assert_eq!(v(0, 1).heading_for_bearing(v(1, 0)), west);
    }
}
