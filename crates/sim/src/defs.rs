use crate::{fixed::Fx, vec::Vec2};

/// Compiled, sim-ready definitions. Units are already converted to ticks and
/// meters per tick, so the tick never touches seconds or floats.
#[derive(Clone, Debug, Default)]
pub struct Defs {
    pub ships: Vec<ShipDef>,
}

impl Defs {
    pub fn ship_index(&self, name: &str) -> Option<u16> {
        self.ships
            .iter()
            .position(|s| s.name == name)
            .map(|i| i as u16)
    }
}

#[derive(Clone, Debug)]
pub struct ShipDef {
    pub name: String,
    pub hull: i32,
    pub shield: i32,
    /// Restored once per second while the shield generator is intact.
    pub shield_regen: i32,
    /// Meters per tick.
    pub max_speed: Fx,
    /// Meters per tick squared.
    pub accel: Fx,
    pub radius: Fx,
    pub sensor_range: Fx,
    /// Distance the ship holds to its target.
    pub engage_range: Fx,
    pub hardpoints: Vec<HardpointDef>,
}

#[derive(Clone, Debug)]
pub struct HardpointDef {
    pub name: String,
    pub health: i32,
    pub offset: Vec2,
    pub kind: HardpointKind,
}

#[derive(Clone, Debug)]
pub enum HardpointKind {
    Weapon(WeaponDef),
    /// Destroyed: shield stops regenerating.
    ShieldGenerator,
    /// Destroyed: max speed halves.
    Engine,
}

#[derive(Clone, Debug)]
pub struct WeaponDef {
    pub damage: i32,
    pub range: Fx,
    pub cooldown_ticks: u16,
    pub accuracy_pct: u8,
}
