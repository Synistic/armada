//! Deterministic battle simulation. No floats, no hash maps, no threads, no clock.
//! Same inputs produce the same bits on every machine, which is what lockstep
//! multiplayer and replays depend on.

pub mod command;
pub mod defs;
pub mod fixed;
pub mod formation;
pub mod grid;
pub mod hash;
pub mod rng;
pub mod step;
pub mod vec;
pub mod world;

pub use command::{Command, CommandLog, TimedCommand};
pub use defs::{Defs, Delivery, HardpointDef, HardpointKind, ShipDef, WeaponDef};
pub use fixed::Fx;
pub use hash::state_hash;
pub use step::step;
pub use vec::Vec2;
pub use world::{Event, NONE, Order, World};

pub const TICK_HZ: u32 = 15;
pub const MAX_TEAMS: usize = 8;

/// Runs `ticks` ticks and returns `(tick, state_hash)` every `checkpoint_every` ticks.
pub fn run(
    world: &mut World,
    log: &CommandLog,
    ticks: u32,
    checkpoint_every: u32,
) -> Vec<(u32, u64)> {
    let mut checkpoints = Vec::new();
    for _ in 0..ticks {
        step(world, log.for_tick(world.tick));
        if world.tick.is_multiple_of(checkpoint_every) {
            checkpoints.push((world.tick, state_hash(world)));
        }
    }
    checkpoints
}
