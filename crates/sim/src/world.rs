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
    /// Autonomous: pick the nearest enemy, close to engage range, otherwise
    /// advance on the enemy centroid.
    Engage,
    /// Stay put, turn the best arc toward the current target.
    Hold,
    /// Go to a formation slot at no more than `cap` meters per tick.
    MoveTo { to: Vec2, cap: Fx },
    /// Like `MoveTo`, but engage anything picked up on the way.
    AttackMove { to: Vec2, cap: Fx },
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

/// Projectiles in flight. Removed with swap_remove while iterating forward,
/// which is order dependent but the same on every peer.
#[derive(Clone, Debug, Default)]
pub struct Projectiles {
    pub pos: Vec<Vec2>,
    pub vel: Vec<Vec2>,
    /// `NONE` for a miss: it flies on until it expires.
    pub target: Vec<u32>,
    pub damage: Vec<i32>,
    pub bypass: Vec<u8>,
    pub ttl: Vec<u16>,
    pub team: Vec<u8>,
}

impl Projectiles {
    pub fn len(&self) -> usize {
        self.pos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    pub fn swap_remove(&mut self, i: usize) {
        self.pos.swap_remove(i);
        self.vel.swap_remove(i);
        self.target.swap_remove(i);
        self.damage.swap_remove(i);
        self.bypass.swap_remove(i);
        self.ttl.swap_remove(i);
        self.team.swap_remove(i);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Shot { hardpoint: u32, target: u32, hit: bool },
    Launch { hardpoint: u32 },
    Impact { pos: Vec2, target: u32 },
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
    pub projectiles: Projectiles,
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
            projectiles: Projectiles::default(),
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

    /// World position of a hardpoint, following its ship's heading.
    pub fn hardpoint_pos(&self, h: usize) -> Vec2 {
        let owner = self.hardpoints.owner[h] as usize;
        let def = &self.defs.ships[usize::from(self.ships.def[owner])];
        let offset = def.hardpoints[usize::from(self.hardpoints.slot[h])].offset;
        self.ships.pos[owner] + offset.from_local(self.ships.heading[owner])
    }

    /// Max speed right now, halved with a destroyed engine.
    pub fn max_speed(&self, i: usize) -> Fx {
        let base = self.defs.ships[usize::from(self.ships.def[i])].max_speed;
        if self.ships.flags[i] & FLAG_ENGINE_DOWN != 0 {
            base / 2
        } else {
            base
        }
    }
}
