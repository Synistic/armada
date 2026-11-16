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
    CommandLog, Defs, Delivery, Fx, HardpointDef, HardpointKind, MAX_TEAMS, Rules, ShipDef,
    TICK_HZ, Vec2, WeaponDef, World,
};
use serde::{Deserialize, Serialize};

const HZ: f64 = TICK_HZ as f64;
/// By default ships hold this fraction of their longest weapon range to their target.
const ENGAGE_FRACTION: f64 = 0.85;
pub const REPLAY_FORMAT: u32 = 4;
const PROJECTILE_TTL_FACTOR: f64 = 1.6;
/// Candidate bearings when searching for the best broadside.
const BEARING_STEPS: u32 = 72;

#[derive(Deserialize)]
struct ShipFile {
    ships: Vec<ShipSpec>,
}

#[derive(Deserialize)]
struct ShipSpec {
    name: String,
    display_name: String,
    hull: i32,
    shield: i32,
    shield_regen: i32,
    speed: f64,
    accel: f64,
    /// Degrees per second.
    turn_rate: f64,
    radius: f64,
    sensor_range: f64,
    /// Distance held to the target. Defaults to the longest weapon range
    /// times ENGAGE_FRACTION.
    #[serde(default)]
    engage_range: Option<f64>,
    hardpoints: Vec<HardpointSpec>,
    #[serde(default)]
    cost: i32,
    /// Seconds.
    #[serde(default)]
    build_time: f64,
    #[serde(default = "one")]
    pop: u16,
    #[serde(default)]
    critical: bool,
    /// Names of ship types this one can build.
    #[serde(default)]
    builds: Vec<String>,
}

fn one() -> u16 {
    1
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
        /// Degrees clockwise from the bow: 90 is starboard, 270 is port.
        #[serde(default)]
        facing: f64,
        /// Full firing arc in degrees.
        #[serde(default = "full_arc")]
        arc: f64,
        /// Meters per second. Set: homing projectile. Unset: beam.
        #[serde(default)]
        projectile_speed: Option<f64>,
        /// Percent of the damage that ignores shields.
        #[serde(default)]
        shield_bypass: u8,
    },
    ShieldGenerator,
    Engine,
}

fn full_arc() -> f64 {
    360.0
}

#[derive(Deserialize)]
pub struct ScenarioSpec {
    /// Shown in the start menu.
    #[serde(default)]
    pub name: String,
    pub seed: u64,
    pub arena_half: f64,
    pub ticks: u32,
    /// Relative to the scenario file. Later files override ships of the same name.
    pub ship_files: Vec<String>,
    fleets: Vec<FleetSpec>,
    /// Unset: no economy, a plain fleet battle.
    #[serde(default)]
    economy: Option<EconomySpec>,
    #[serde(default)]
    points: Vec<(f64, f64)>,
}

#[derive(Deserialize)]
struct EconomySpec {
    start_credits: i32,
    /// Credits per second.
    income: i32,
    point_income: i32,
    pop_cap: u16,
    capture_radius: f64,
    /// Seconds.
    capture_time: f64,
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
    /// Degrees clockwise from +y.
    #[serde(default)]
    facing: f64,
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

/// Content files may write `Some(x)` as plain `x` for optional fields.
fn content_ron() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

fn read(path: &Path) -> Result<String, LoadError> {
    fs::read_to_string(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))
}

fn parse_scenario(text: &str) -> Result<ScenarioSpec, LoadError> {
    content_ron()
        .from_str(text)
        .map_err(|e| LoadError::Parse("scenario".into(), Box::new(e)))
}

/// Menu name of a scenario file, the file name if it has none.
pub fn scenario_name(path: &Path) -> Result<String, LoadError> {
    let spec = parse_scenario(&read(path)?)?;
    Ok(if spec.name.is_empty() {
        path.file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
    } else {
        spec.name
    })
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

/// Sine from +, * and round only. `f64::sin` calls the platform libm, which
/// is not guaranteed to give the same bits everywhere; this is.
fn det_sin(x: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let mut x = x - TAU * (x / TAU).round();
    if x > FRAC_PI_2 {
        x = PI - x;
    } else if x < -FRAC_PI_2 {
        x = -PI - x;
    }
    // Taylor to x^13, error below 1e-9 on [-pi/2, pi/2].
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for n in [2.0, 4.0, 6.0, 8.0, 10.0, 12.0] {
        term = -term * x2 / (n * (n + 1.0));
        sum += term;
    }
    sum
}

fn det_cos(x: f64) -> f64 {
    det_sin(x + std::f64::consts::FRAC_PI_2)
}

/// Unit vector for degrees clockwise from +y (or from the bow).
fn dir_deg(deg: f64) -> (f64, f64) {
    let r = deg.to_radians();
    (det_sin(r), det_cos(r))
}

fn fx_vec((x, y): (f64, f64)) -> Vec2 {
    Vec2::new(Fx::from_f64(x), Fx::from_f64(y))
}

struct ArcSpec {
    facing: (f64, f64),
    arc_cos: f64,
    dps: f64,
}

/// Ship frame bearing where the most damage per second bears. Among equal
/// bearings the one where the guns point most centrally wins, otherwise a
/// bearing on the very edge of an arc ties with the middle of it.
fn best_bearing(arcs: &[ArcSpec]) -> (f64, f64) {
    const EPS: f64 = 1e-6;
    let mut best = (0.0, 1.0);
    let mut best_score = (-1.0, -1.0);
    for k in 0..BEARING_STEPS {
        let b = dir_deg(f64::from(k) * 360.0 / f64::from(BEARING_STEPS));
        let (mut dps, mut centered) = (0.0, 0.0);
        for a in arcs {
            let dot = b.0 * a.facing.0 + b.1 * a.facing.1;
            if dot >= a.arc_cos - EPS {
                dps += a.dps;
                centered += a.dps * dot;
            }
        }
        let better =
            dps > best_score.0 + EPS || (dps > best_score.0 - EPS && centered > best_score.1 + EPS);
        if better {
            best_score = (dps, centered);
            best = b;
        }
    }
    best
}

/// Returns the def and the names it builds, resolved once all files are read.
fn compile_ship(spec: ShipSpec) -> (ShipDef, Vec<String>) {
    let mut max_range: Option<f64> = None;
    let mut arcs = Vec::new();
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
                    facing,
                    arc,
                    projectile_speed,
                    shield_bypass,
                } => {
                    max_range = Some(max_range.map_or(range, |m| m.max(range)));
                    let facing = dir_deg(facing);
                    let arc_cos = if arc >= 360.0 {
                        -1.0
                    } else {
                        det_cos((arc / 2.0).to_radians())
                    };
                    let cooldown_ticks = (cooldown * HZ).round().max(1.0) as u16;
                    arcs.push(ArcSpec {
                        facing,
                        arc_cos,
                        dps: f64::from(damage) / f64::from(cooldown_ticks),
                    });
                    let delivery = match projectile_speed {
                        None => Delivery::Beam,
                        Some(speed) => {
                            let per_tick = speed / HZ;
                            // Enough life to chase a target across the full
                            // range with some slack for turning.
                            let ttl = (range * PROJECTILE_TTL_FACTOR / per_tick).ceil();
                            Delivery::Projectile {
                                speed: Fx::from_f64(per_tick),
                                ttl_ticks: ttl.clamp(1.0, f64::from(u16::MAX)) as u16,
                            }
                        }
                    };
                    HardpointKind::Weapon(WeaponDef {
                        damage,
                        range: Fx::from_f64(range),
                        cooldown_ticks,
                        accuracy_pct: accuracy.min(100),
                        shield_bypass_pct: shield_bypass.min(100),
                        delivery,
                        facing: fx_vec(facing),
                        arc_cos: Fx::from_f64(arc_cos),
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
    let turn = (spec.turn_rate / HZ).to_radians();

    let def = ShipDef {
        name: spec.name,
        display_name: spec.display_name,
        hull: spec.hull,
        shield: spec.shield,
        shield_regen: spec.shield_regen,
        max_speed: Fx::from_f64(spec.speed / HZ),
        accel: Fx::from_f64(spec.accel / (HZ * HZ)),
        turn_cos: Fx::from_f64(det_cos(turn)),
        turn_sin: Fx::from_f64(det_sin(turn)),
        preferred_bearing: fx_vec(best_bearing(&arcs)),
        radius: Fx::from_f64(spec.radius),
        sensor_range: Fx::from_f64(spec.sensor_range),
        engage_range: Fx::from_f64(
            spec.engage_range
                .unwrap_or_else(|| max_range.unwrap_or(spec.sensor_range * 0.5) * ENGAGE_FRACTION),
        ),
        hardpoints,
        cost: spec.cost,
        build_ticks: (spec.build_time * HZ).round().max(1.0) as u32,
        pop: spec.pop,
        critical: spec.critical,
        builds: Vec::new(),
    };
    (def, spec.builds)
}

fn compile_defs(ship_files: &[String]) -> Result<Defs, LoadError> {
    let mut defs = Defs::default();
    let mut builds: Vec<Vec<String>> = Vec::new();
    for (i, text) in ship_files.iter().enumerate() {
        let file: ShipFile = content_ron()
            .from_str(text)
            .map_err(|e| LoadError::Parse(format!("ship file {i}"), Box::new(e)))?;
        for spec in file.ships {
            let (ship, names) = compile_ship(spec);
            match defs.ship_index(&ship.name) {
                Some(idx) => {
                    defs.ships[usize::from(idx)] = ship;
                    builds[usize::from(idx)] = names;
                }
                None => {
                    defs.ships.push(ship);
                    builds.push(names);
                }
            }
        }
    }
    for (i, names) in builds.iter().enumerate() {
        let ids = names
            .iter()
            .map(|n| {
                defs.ship_index(n)
                    .ok_or_else(|| LoadError::UnknownShip(n.clone()))
            })
            .collect::<Result<_, _>>()?;
        defs.ships[i].builds = ids;
    }
    Ok(defs)
}

pub fn build(sources: &Sources) -> Result<(World, ScenarioSpec), LoadError> {
    let spec = parse_scenario(&sources.scenario)?;
    let defs = Arc::new(compile_defs(&sources.ship_files)?);
    let mut world = World::new(Arc::clone(&defs), spec.seed, Fx::from_f64(spec.arena_half));
    if let Some(e) = &spec.economy {
        world.rules = Rules {
            income: e.income,
            point_income: e.point_income,
            pop_cap: e.pop_cap,
            capture_radius: Fx::from_f64(e.capture_radius),
            capture_secs: e.capture_time.round().clamp(1.0, f64::from(u16::MAX)) as u16,
        };
    }
    for &p in &spec.points {
        world.points.add(fx_vec(p));
    }

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
            let heading = fx_vec(dir_deg(g.facing));
            for k in 0..g.count {
                let col = i64::from(k % columns);
                let row = i64::from(k / columns);
                let pos = origin + Vec2::new(spacing * col - left, row_spacing * row);
                world.spawn_ship(def, fleet.team, pos, heading);
            }
        }
        if let Some(e) = &spec.economy {
            world.teams[usize::from(fleet.team)].credits = e.start_credits;
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

#[cfg(test)]
mod tests {
    use super::{ArcSpec, best_bearing, det_cos, det_sin};

    #[test]
    fn trig_matches_libm_closely() {
        for i in -720..=720 {
            let x = f64::from(i) * 0.01;
            assert!((det_sin(x) - x.sin()).abs() < 1e-9, "sin {x}");
            assert!((det_cos(x) - x.cos()).abs() < 1e-9, "cos {x}");
        }
    }

    #[test]
    fn broadside_wins_over_single_bow_gun() {
        let side = |facing| ArcSpec {
            facing,
            arc_cos: 0.5,
            dps: 2.0,
        };
        // Narrow bow arc, so it never overlaps the starboard arcs.
        let arcs = [
            ArcSpec {
                facing: (0.0, 1.0),
                arc_cos: 0.95,
                dps: 3.0,
            },
            side((1.0, 0.0)),
            side((1.0, 0.0)),
        ];
        let (x, y) = best_bearing(&arcs);
        assert!(x >= 0.49 && y < 0.9, "got ({x}, {y})");
    }
}
