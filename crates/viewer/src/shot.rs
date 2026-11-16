//! Photo mode: `--shot out.png --tick N [--at x,y] [--dist D] [--yaw DEG] [--select N]`.
//!
//! Fast-forwards the battle, runs the last ticks live so beams and explosions
//! are on screen, saves one frame and quits. Both fleets fight on their own.
//! The sim is deterministic, so the same flags always give the same picture.

use std::path::PathBuf;

use armada_sim::Fx;
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use crate::{
    camera::Rig,
    input::Selection,
    render::SCALE,
    sim::{PLAYER_TEAM, Sim},
};

/// Ticks rendered live before the picture is taken.
pub const LIVE_TICKS: u32 = 20;
/// Frames to wait after requesting the screenshot before quitting.
const EXIT_DELAY: u32 = 15;

#[derive(Resource, Clone)]
pub struct Shot {
    pub out: PathBuf,
    pub tick: u32,
    at: Vec2,
    dist: f32,
    yaw: f32,
    select: usize,
}

impl Shot {
    /// Takes the photo flags out of `args` and leaves the rest for the sim.
    pub fn parse(args: &mut Vec<String>) -> Result<Option<Self>, String> {
        let mut rest = Vec::new();
        let mut shot: Option<Self> = None;
        let mut it = std::mem::take(args).into_iter();
        while let Some(arg) = it.next() {
            if !matches!(
                arg.as_str(),
                "--shot" | "--tick" | "--at" | "--dist" | "--yaw" | "--select"
            ) {
                rest.push(arg);
                continue;
            }
            let value = it.next().ok_or(format!("{arg} needs a value"))?;
            let bad = || format!("bad value for {arg}: {value}");
            let s = shot.get_or_insert_with(|| Self {
                out: PathBuf::new(),
                tick: 600,
                at: Vec2::ZERO,
                dist: 900.0,
                yaw: 180.0,
                select: 0,
            });
            match arg.as_str() {
                "--shot" => s.out = PathBuf::from(&value),
                "--tick" => s.tick = value.parse().map_err(|_| bad())?,
                "--dist" => s.dist = value.parse().map_err(|_| bad())?,
                "--yaw" => s.yaw = value.parse().map_err(|_| bad())?,
                "--select" => s.select = value.parse().map_err(|_| bad())?,
                _ => {
                    let (x, y) = value
                        .split_once(',')
                        .ok_or(format!("--at wants x,y: {value}"))?;
                    s.at = Vec2::new(
                        x.parse().map_err(|_| format!("bad x: {x}"))?,
                        y.parse().map_err(|_| format!("bad y: {y}"))?,
                    );
                }
            }
        }
        *args = rest;
        match shot {
            Some(s) if s.out.as_os_str().is_empty() => Err("photo flags need --shot <file>".into()),
            other => Ok(other),
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(PostStartup, setup)
        .add_systems(Update, capture);
}

fn setup(shot: Res<Shot>, sim: Res<Sim>, mut sel: ResMut<Selection>, mut q: Query<&mut Rig>) {
    if let Ok(mut rig) = q.single_mut() {
        rig.focus = Vec3::new(shot.at.x, 0.0, shot.at.y) * SCALE;
        rig.distance = shot.dist;
        rig.yaw = shot.yaw.to_radians();
    }
    if shot.select == 0 {
        return;
    }
    let ships = &sim.world.ships;
    let at = armada_sim::Vec2 {
        x: Fx::from_f64(f64::from(shot.at.x)),
        y: Fx::from_f64(f64::from(shot.at.y)),
    };
    let mut near: Vec<(Fx, u32)> = (0..ships.len())
        .filter(|&i| ships.alive[i] && ships.team[i] == PLAYER_TEAM)
        .map(|i| ((ships.pos[i] - at).len_sq(), i as u32))
        .collect();
    near.sort_unstable();
    sel.ships = near.into_iter().take(shot.select).map(|(_, i)| i).collect();
    sel.ships.sort_unstable();
}

fn capture(
    mut commands: Commands,
    shot: Res<Shot>,
    sim: Res<Sim>,
    mut taken: Local<Option<u32>>,
    mut exit: MessageWriter<AppExit>,
) {
    match *taken {
        None if sim.world.tick >= shot.tick => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(shot.out.clone()));
            *taken = Some(0);
        }
        Some(n) if n >= EXIT_DELAY => {
            exit.write(AppExit::Success);
        }
        Some(n) => *taken = Some(n + 1),
        None => {}
    }
}
