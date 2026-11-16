use std::sync::Arc;

use crate::{
    MAX_TEAMS,
    defs::{Defs, HardpointKind},
    fixed::Fx,
    grid::Grid,
    rng::Rng,
    vec::Vec2,
};

pub const NONE: u32 = u32::MAX;

pub const FLAG_SHIELD_DOWN: u8 = 1;
pub const FLAG_ENGINE_DOWN: u8 = 2;

const GRID_CELL_M: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// Default stance: pick the nearest enemy, close to engage range, otherwise
    /// advance on the enemy centroid.
    Engage,
    /// Go there and hold. Still fires at whatever is in range.
    MoveTo(Vec2),
    Attack(u32),
}

/// Struct of arrays. Ship ids are indices and never get reused during a
/// battle, dead ships stay as tombstones. Battles are short, so that is cheaper
/// than generational ids for now.
#[derive(Clone, Debug, Default)]
pub struct Ships {
    pub def: Vec<u16>,
    pub team: Vec<u8>,
    pub alive: Vec<bool>,
    pub pos: Vec<Vec2>,
    /// Unit vector. Ships only thrust along it.
    pub heading: Vec<Vec2>,
    /// Scalar speed along the heading, meters per tick.
    pub speed: Vec<Fx>,
    /// Actual displacement of the last tick, for presentation.
    pub vel: Vec<Vec2>,
    pub hull: Vec<i32>,
    pub shield: Vec<i32>,
    pub flags: Vec<u8>,
    pub order: Vec<Order>,
    pub target: Vec<u32>,
    pub hp_first: Vec<u32>,
    pub hp_count: Vec<u16>,
}

impl Ships {
    pub fn len(&self) -> usize {
        self.def.len()
    }

    pub fn is_empty(&self) -> bool {
        self.def.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Hardpoints {
    pub owner: Vec<u32>,
    /// Index into the owner's `ShipDef::hardpoints`.
    pub slot: Vec<u16>,
    pub health: Vec<i32>,
    pub cooldown: Vec<u16>,
    /// Target of opportunity, used when the ship's own target is out of arc.
    pub target: Vec<u32>,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Shot { hardpoint: u32, target: u32, hit: bool },
    HardpointDestroyed(u32),
    ShipDestroyed(u32),
}

#[derive(Clone)]
pub struct World {
    pub tick: u32,
    pub defs: Arc<Defs>,
    pub arena_half: Fx,
    pub ships: Ships,
    pub hardpoints: Hardpoints,
    pub rng: Rng,
    pub grid: Grid,
    /// Output of the last tick for presentation. The sim never reads it.
    pub events: Vec<Event>,
}

impl World {
    pub fn new(defs: Arc<Defs>, seed: u64, arena_half: Fx) -> Self {
        Self {
            tick: 0,
            defs,
            arena_half,
            ships: Ships::default(),
            hardpoints: Hardpoints::default(),
            rng: Rng::new(seed),
            grid: Grid::new(arena_half, Fx::from_int(GRID_CELL_M)),
            events: Vec::new(),
        }
    }

    /// `heading` must be a unit vector.
    pub fn spawn_ship(&mut self, def: u16, team: u8, pos: Vec2, heading: Vec2) -> u32 {
        assert!(usize::from(team) < MAX_TEAMS, "team {team} out of range");
        let defs = Arc::clone(&self.defs);
        let d = &defs.ships[usize::from(def)];
        let id = self.ships.len() as u32;

        let s = &mut self.ships;
        s.def.push(def);
        s.team.push(team);
        s.alive.push(true);
        s.pos.push(pos.clamp_box(self.arena_half));
        s.heading.push(heading);
        s.speed.push(Fx::ZERO);
        s.vel.push(Vec2::ZERO);
        s.hull.push(d.hull);
        s.shield.push(d.shield);
        s.flags.push(0);
        s.order.push(Order::Engage);
        s.target.push(NONE);
        s.hp_first.push(self.hardpoints.owner.len() as u32);
        s.hp_count.push(d.hardpoints.len() as u16);

        for (slot, hp) in d.hardpoints.iter().enumerate() {
            // Random initial cooldown so a fleet does not fire in perfect volleys.
            let cooldown = match &hp.kind {
                HardpointKind::Weapon(w) => self.rng.below(u32::from(w.cooldown_ticks) + 1) as u16,
                _ => 0,
            };
            self.hardpoints.owner.push(id);
            self.hardpoints.slot.push(slot as u16);
            self.hardpoints.health.push(hp.health);
            self.hardpoints.cooldown.push(cooldown);
            self.hardpoints.target.push(NONE);
        }
        id
    }

    pub fn alive_count(&self, team: u8) -> usize {
        self.ships
            .team
            .iter()
            .zip(&self.ships.alive)
            .filter(|(t, a)| **t == team && **a)
            .count()
    }
}
