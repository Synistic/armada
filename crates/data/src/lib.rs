//! Content loading: RON files in human units (seconds, meters per second)
//! compiled into sim-ready definitions. Floats are allowed here and only here,
//! because this runs once before the first tick.
#![allow(clippy::float_arithmetic)]

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use armada_sim::{
    CommandLog, Defs, Fx, HardpointDef, HardpointKind, MAX_TEAMS, ShipDef, TICK_HZ, Vec2,
    WeaponDef, World,
};
use serde::{Deserialize, Serialize};

const HZ: f64 = TICK_HZ as f64;
/// Ships hold this fraction of their shortest weapon range to their target.
const ENGAGE_FRACTION: f64 = 0.85;
pub const REPLAY_FORMAT: u32 = 1;

#[derive(Deserialize)]
struct ShipFile {
    ships: Vec<ShipSpec>,
}

#[derive(Deserialize)]
struct ShipSpec {
    name: String,
    hull: i32,
    shield: i32,
    shield_regen: i32,
    speed: f64,
    accel: f64,
    radius: f64,
    sensor_range: f64,
    hardpoints: Vec<HardpointSpec>,
}

#[derive(Deserialize)]
struct HardpointSpec {
    name: String,
    health: i32,
    offset: (f64, f64),
    kind: HardpointKindSpec,
}

#[derive(Deserialize)]
enum HardpointKindSpec {
    Weapon {
        damage: i32,
        range: f64,
        cooldown: f64,
        accuracy: u8,
    },
    ShieldGenerator,
    Engine,
}

#[derive(Deserialize)]
pub struct ScenarioSpec {
    pub seed: u64,
    pub arena_half: f64,
    pub ticks: u32,
    /// Relative to the scenario file. Later files override ships of the same name.
    pub ship_files: Vec<String>,
    fleets: Vec<FleetSpec>,
}

#[derive(Deserialize)]
struct FleetSpec {
    team: u8,
    groups: Vec<GroupSpec>,
}

/// A block formation: `columns` wide, rows stacked along `row_spacing`
/// (signed, so each side can stack away from the enemy).
#[derive(Deserialize)]
struct GroupSpec {
    ship: String,
    count: u32,
    columns: u32,
    spacing: f64,
    row_spacing: f64,
    origin: (f64, f64),
}

/// Raw file contents. A replay embeds these, so it stays valid when the
/// content on disk changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sources {
    pub scenario: String,
    pub ship_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Replay {
    pub format: u32,
    pub sources: Sources,
    pub ticks: u32,
    pub commands: CommandLog,
    pub checkpoints: Vec<(u32, u64)>,
}

#[derive(Debug)]
pub enum LoadError {
    Io(PathBuf, io::Error),
    Parse(String, Box<ron::error::SpannedError>),
    Write(ron::Error),
    UnknownShip(String),
    BadTeam(u8),
    BadReplayFormat(u32),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Parse(what, e) => write!(f, "{what}: {e}"),
            Self::Write(e) => write!(f, "serialize: {e}"),
            Self::UnknownShip(n) => write!(f, "unknown ship type '{n}'"),
            Self::BadTeam(t) => write!(f, "team {t} out of range (max {})", MAX_TEAMS - 1),
            Self::BadReplayFormat(v) => {
                write!(f, "replay format {v}, expected {REPLAY_FORMAT}")
            }
        }
    }
}

impl std::error::Error for LoadError {}

fn read(path: &Path) -> Result<String, LoadError> {
    fs::read_to_string(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))
}

fn parse_scenario(text: &str) -> Result<ScenarioSpec, LoadError> {
    ron::from_str(text).map_err(|e| LoadError::Parse("scenario".into(), Box::new(e)))
}

pub fn read_sources(scenario_path: &Path) -> Result<Sources, LoadError> {
    let scenario = read(scenario_path)?;
    let spec = parse_scenario(&scenario)?;
    let dir = scenario_path.parent().unwrap_or(Path::new("."));
    let ship_files = spec
        .ship_files
        .iter()
        .map(|f| read(&dir.join(f)))
        .collect::<Result<_, _>>()?;
    Ok(Sources {
        scenario,
        ship_files,
    })
}

fn compile_ship(spec: ShipSpec) -> ShipDef {
    let mut min_range: Option<f64> = None;
    let hardpoints = spec
        .hardpoints
        .into_iter()
        .map(|h| {
            let kind = match h.kind {
                HardpointKindSpec::Weapon {
                    damage,
                    range,
                    cooldown,
                    accuracy,
                } => {
                    min_range = Some(min_range.map_or(range, |m| m.min(range)));
                    HardpointKind::Weapon(WeaponDef {
                        damage,
                        range: Fx::from_f64(range),
                        cooldown_ticks: (cooldown * HZ).round().max(1.0) as u16,
                        accuracy_pct: accuracy.min(100),
                    })
                }
                HardpointKindSpec::ShieldGenerator => HardpointKind::ShieldGenerator,
                HardpointKindSpec::Engine => HardpointKind::Engine,
            };
            HardpointDef {
                name: h.name,
                health: h.health,
                offset: Vec2::new(Fx::from_f64(h.offset.0), Fx::from_f64(h.offset.1)),
                kind,
            }
        })
        .collect();

    ShipDef {
        name: spec.name,
        hull: spec.hull,
        shield: spec.shield,
        shield_regen: spec.shield_regen,
        max_speed: Fx::from_f64(spec.speed / HZ),
        accel: Fx::from_f64(spec.accel / (HZ * HZ)),
        radius: Fx::from_f64(spec.radius),
        sensor_range: Fx::from_f64(spec.sensor_range),
        engage_range: Fx::from_f64(min_range.unwrap_or(spec.sensor_range * 0.5) * ENGAGE_FRACTION),
        hardpoints,
    }
}

fn compile_defs(ship_files: &[String]) -> Result<Defs, LoadError> {
    let mut defs = Defs::default();
    for (i, text) in ship_files.iter().enumerate() {
        let file: ShipFile =
            ron::from_str(text).map_err(|e| LoadError::Parse(format!("ship file {i}"), Box::new(e)))?;
        for spec in file.ships {
            let ship = compile_ship(spec);
            match defs.ship_index(&ship.name) {
                Some(idx) => defs.ships[usize::from(idx)] = ship,
                None => defs.ships.push(ship),
            }
        }
    }
    Ok(defs)
}

pub fn build(sources: &Sources) -> Result<(World, ScenarioSpec), LoadError> {
    let spec = parse_scenario(&sources.scenario)?;
    let defs = Arc::new(compile_defs(&sources.ship_files)?);
    let mut world = World::new(Arc::clone(&defs), spec.seed, Fx::from_f64(spec.arena_half));

    for fleet in &spec.fleets {
        if usize::from(fleet.team) >= MAX_TEAMS {
            return Err(LoadError::BadTeam(fleet.team));
        }
        for g in &fleet.groups {
            let def = defs
                .ship_index(&g.ship)
                .ok_or_else(|| LoadError::UnknownShip(g.ship.clone()))?;
            let columns = g.columns.max(1);
            let origin = Vec2::new(Fx::from_f64(g.origin.0), Fx::from_f64(g.origin.1));
            let spacing = Fx::from_f64(g.spacing);
            let row_spacing = Fx::from_f64(g.row_spacing);
            // Center the block on its origin along x.
            let left = spacing * i64::from(columns - 1) / 2;
            for k in 0..g.count {
                let col = i64::from(k % columns);
                let row = i64::from(k / columns);
                let pos = origin + Vec2::new(spacing * col - left, row_spacing * row);
                world.spawn_ship(def, fleet.team, pos);
            }
        }
    }
    Ok((world, spec))
}

pub fn save_replay(replay: &Replay) -> Result<String, LoadError> {
    ron::ser::to_string_pretty(replay, ron::ser::PrettyConfig::default()).map_err(LoadError::Write)
}

pub fn load_replay(path: &Path) -> Result<Replay, LoadError> {
    let replay: Replay =
        ron::from_str(&read(path)?).map_err(|e| LoadError::Parse("replay".into(), Box::new(e)))?;
    if replay.format != REPLAY_FORMAT {
        return Err(LoadError::BadReplayFormat(replay.format));
    }
    Ok(replay)
}

/// Path to the bundled content, independent of the working directory.
pub fn content_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}
