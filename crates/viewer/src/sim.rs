use std::{
    error::Error,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use armada_ai::Ai;
use armada_data::{
    REPLAY_FORMAT, Replay, Sources, build, content_dir, load_replay, read_sources, save_replay,
};
use armada_sim::{
    Command, CommandLog, Event, TICK_HZ, TimedCommand, Vec2, World, state_hash, step,
};
use bevy::prelude::*;

pub const PLAYER_TEAM: u8 = 0;
pub const SPEEDS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
const CHECKPOINT_EVERY: u32 = 15;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Live,
    Replay,
}

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    pub prev_pos: Vec<Vec2>,
    pub mode: Mode,
    pub paused: bool,
    pub speed: usize,
    log: CommandLog,
    sources: Sources,
    checkpoints: Vec<(u32, u64)>,
    /// Computer players. Their commands go into the log like the player's.
    ais: Vec<Ai>,
}

/// Events of every tick since the last frame. `World::events` only holds the
/// last tick, and several ticks can run per frame at high speed.
#[derive(Resource, Default)]
pub struct FrameEvents(pub Vec<Event>);

impl Sim {
    /// `auto`: the AI plays the player's side too, for the photo mode.
    pub fn load(args: &[String], auto: bool) -> Result<Self, Box<dyn Error>> {
        let (sources, log, mode) = if args.first().map(String::as_str) == Some("--replay") {
            let path = PathBuf::from(args.get(1).ok_or("--replay needs a path")?);
            let replay = load_replay(&path)?;
            (replay.sources, replay.commands, Mode::Replay)
        } else {
            let scenario = args
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| content_dir().join("scenarios/skirmish.ron"));
            (read_sources(&scenario)?, CommandLog::default(), Mode::Live)
        };
        let (world, _) = build(&sources)?;

        let mut sim = Self {
            prev_pos: world.ships.pos.clone(),
            world,
            mode,
            paused: false,
            speed: 1,
            log,
            sources,
            checkpoints: Vec::new(),
            ais: Vec::new(),
        };
        if mode == Mode::Live {
            if auto {
                sim.ais.push(Ai::new(PLAYER_TEAM));
            }
            sim.ais.push(Ai::new(1));
        }
        if mode == Mode::Live && !auto {
            // The player's fleet waits for orders instead of charging.
            let ships = (0..sim.world.ships.len() as u32)
                .filter(|&i| sim.world.ships.team[i as usize] == PLAYER_TEAM)
                .collect();
            sim.issue(Command::Stop { ships });
        }
        Ok(sim)
    }

    /// Run ticks without rendering, up to `to`.
    pub fn fast_forward(&mut self, to: u32) {
        while self.world.tick < to {
            self.advance();
        }
        self.prev_pos.clone_from(&self.world.ships.pos);
    }

    fn advance(&mut self) {
        let tick = self.world.tick;
        for ai in &mut self.ais {
            for command in ai.think(&self.world) {
                self.log.push(TimedCommand {
                    tick,
                    team: ai.team(),
                    command,
                });
            }
        }
        step(&mut self.world, self.log.for_tick(tick));
        if self.world.tick.is_multiple_of(CHECKPOINT_EVERY) {
            self.checkpoints
                .push((self.world.tick, state_hash(&self.world)));
        }
    }

    /// Player's first producer, if it still stands.
    pub fn player_producer(&self) -> Option<u32> {
        let s = &self.world.ships;
        (0..s.len()).find_map(|i| {
            let ok = s.alive[i]
                && s.team[i] == PLAYER_TEAM
                && !self.world.defs.ships[usize::from(s.def[i])]
                    .builds
                    .is_empty();
            ok.then_some(i as u32)
        })
    }

    /// Queue a player command for the next tick. Ignored in replays.
    pub fn issue(&mut self, command: Command) {
        if self.mode == Mode::Live {
            self.log.push(TimedCommand {
                tick: self.world.tick,
                team: PLAYER_TEAM,
                command,
            });
        }
    }

    pub fn save_replay(&self) -> Result<PathBuf, Box<dyn Error>> {
        let dir = PathBuf::from("replays");
        fs::create_dir_all(&dir)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let path = dir.join(format!("battle_{stamp}.replay.ron"));
        let replay = Replay {
            format: REPLAY_FORMAT,
            sources: self.sources.clone(),
            ticks: self.world.tick,
            commands: self.log.clone(),
            checkpoints: self.checkpoints.clone(),
        };
        fs::write(&path, save_replay(&replay)?)?;
        Ok(path)
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<FrameEvents>()
        .add_systems(FixedUpdate, tick)
        .add_systems(Update, time_controls);
}

fn tick(mut sim: ResMut<Sim>, mut events: ResMut<FrameEvents>) {
    if sim.paused {
        return;
    }
    let sim = &mut *sim;
    sim.prev_pos.clone_from(&sim.world.ships.pos);
    sim.advance();
    events.0.extend_from_slice(&sim.world.events);
}

fn time_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut fixed: ResMut<Time<Fixed>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    let mut speed = sim.speed;
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        speed = (speed + 1).min(SPEEDS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        speed = speed.saturating_sub(1);
    }
    if speed != sim.speed {
        sim.speed = speed;
        fixed.set_timestep_hz(f64::from(TICK_HZ) * SPEEDS[speed]);
    }
}
