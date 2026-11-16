//! Skirmish AI. It reads the world and answers with commands, nothing else:
//! the same power a player has, and its moves land in the replay like theirs.
//! Integer math only, so it can also run in lockstep on every peer.

use armada_sim::{Command, Fx, NONE, Order, Vec2, World, economy::pop_used};

/// Think once per second, staggered by team.
const THINK_EVERY: u32 = 15;
const BUILD_ORDER: [&str; 10] = [
    "corvette",
    "corvette",
    "frigate",
    "corvette",
    "frigate",
    "cruiser",
    "corvette",
    "frigate",
    "cruiser",
    "dreadnought",
];
/// Keep this many ships queued so the station never idles.
const QUEUE_TARGET: usize = 2;
/// Ships closer than this to the station count as waiting at home.
const HOME_RADIUS: i64 = 2500;
/// Enemies closer than this to the station pull everything back.
const DEFEND_RADIUS: i64 = 3500;
/// Credits worth of ships that make up the first attack wave.
const FIRST_WAVE: i32 = 900;
const WAVE_STEP: i32 = 500;
const MAX_WAVE: i32 = 6000;
/// Population share in percent at which every waiting ship attacks.
const MAXED_PCT: u32 = 85;
/// A ship this close to its attack-move point has arrived.
const ARRIVED: i64 = 600;

pub struct Ai {
    team: u8,
    cursor: usize,
    wave: i32,
}

impl Ai {
    pub fn new(team: u8) -> Self {
        Self {
            team,
            cursor: 0,
            wave: FIRST_WAVE,
        }
    }

    pub fn team(&self) -> u8 {
        self.team
    }

    /// Commands for this tick. Empty most ticks.
    pub fn think(&mut self, w: &World) -> Vec<Command> {
        let mut out = Vec::new();
        if w.winner.is_some()
            || !w
                .tick
                .wrapping_add(u32::from(self.team))
                .is_multiple_of(THINK_EVERY)
        {
            return out;
        }
        // Without a station this is a plain fleet battle: leave the ships to
        // their own engage logic.
        let Some(home) = self.producer(w) else {
            return out;
        };
        let home_pos = w.ships.pos[home as usize];

        if let Some(cmd) = self.build(w, home) {
            out.push(cmd);
        }

        let s = &w.ships;
        let mobile: Vec<u32> = (0..s.len() as u32)
            .filter(|&i| {
                let i = i as usize;
                s.alive[i]
                    && s.team[i] == self.team
                    && w.defs.ships[usize::from(s.def[i])].is_mobile()
            })
            .collect();

        if let Some(threat) = self.nearest_enemy(w, home_pos, Fx::from_int(DEFEND_RADIUS)) {
            let defenders: Vec<u32> = mobile
                .iter()
                .copied()
                .filter(|&i| {
                    let i = i as usize;
                    let near_home = within(s.pos[i], home_pos, DEFEND_RADIUS * 2);
                    let already = matches!(s.order[i], Order::AttackMove { to, .. } if within(to, threat, 1000));
                    near_home && !already
                })
                .collect();
            if !defenders.is_empty() {
                out.push(Command::AttackMove {
                    ships: defenders,
                    to: threat,
                });
            }
            return out;
        }

        // New wave once enough ships wait at home.
        let idle: Vec<u32> = mobile
            .iter()
            .copied()
            .filter(|&i| {
                let i = i as usize;
                within(s.pos[i], home_pos, HOME_RADIUS)
                    && matches!(
                        s.order[i],
                        Order::Hold | Order::MoveTo { .. } | Order::Engage
                    )
            })
            .collect();
        let strength: i32 = idle
            .iter()
            .map(|&i| w.defs.ships[usize::from(s.def[i as usize])].cost.max(50))
            .sum();
        // Close to the population cap there is nothing left to wait for.
        let maxed = pop_used(w, self.team) * 100 >= u32::from(w.rules.pop_cap) * MAXED_PCT;
        if !idle.is_empty()
            && (strength >= self.wave || maxed)
            && let Some(to) = self.objective(w, home_pos)
        {
            out.push(Command::AttackMove { ships: idle, to });
            self.wave = (self.wave + WAVE_STEP).min(MAX_WAVE);
        }

        // Ships that reached their point and found nothing to shoot move on.
        let arrived: Vec<u32> = mobile
            .iter()
            .copied()
            .filter(|&i| {
                let i = i as usize;
                matches!(s.order[i], Order::AttackMove { to, .. } if within(s.pos[i], to, ARRIVED))
                    && s.target[i] == NONE
            })
            .collect();
        if let Some(c) = centroid(w, &arrived)
            && let Some(to) = self.objective(w, c)
            && !within(c, to, w.rules.capture_radius.to_int().max(ARRIVED))
        {
            out.push(Command::AttackMove { ships: arrived, to });
        }
        out
    }

    fn producer(&self, w: &World) -> Option<u32> {
        let s = &w.ships;
        (0..s.len()).find_map(|i| {
            let ok = s.alive[i]
                && s.team[i] == self.team
                && !w.defs.ships[usize::from(s.def[i])].builds.is_empty();
            ok.then_some(i as u32)
        })
    }

    /// Next ship from the build order, if it is affordable. Waits for the
    /// money instead of skipping, so expensive ships do get built.
    fn build(&mut self, w: &World, home: u32) -> Option<Command> {
        let h = home as usize;
        if w.ships.queue[h].len() >= QUEUE_TARGET {
            return None;
        }
        let builds = &w.defs.ships[usize::from(w.ships.def[h])].builds;
        for _ in 0..BUILD_ORDER.len() {
            let name = BUILD_ORDER[self.cursor % BUILD_ORDER.len()];
            match w.defs.ship_index(name) {
                Some(id) if builds.contains(&id) => {
                    let d = &w.defs.ships[usize::from(id)];
                    let pop_ok =
                        pop_used(w, self.team) + u32::from(d.pop) <= u32::from(w.rules.pop_cap);
                    if !pop_ok {
                        // Does not fit, try something smaller.
                        self.cursor += 1;
                        continue;
                    }
                    if w.teams[usize::from(self.team)].credits < d.cost {
                        return None;
                    }
                    self.cursor += 1;
                    return Some(Command::Build {
                        producer: home,
                        ship: id,
                    });
                }
                _ => self.cursor += 1,
            }
        }
        None
    }

    /// Nearest point not ours, else the nearest enemy station, else the
    /// nearest enemy ship.
    fn objective(&self, w: &World, from: Vec2) -> Option<Vec2> {
        let p = &w.points;
        let point = (0..p.len())
            .filter(|&k| p.owner[k] != self.team)
            .map(|k| p.pos[k])
            .min_by_key(|&pos| (pos - from).len_sq());
        if point.is_some() {
            return point;
        }
        let s = &w.ships;
        let station = (0..s.len())
            .filter(|&i| {
                s.alive[i] && s.team[i] != self.team && w.defs.ships[usize::from(s.def[i])].critical
            })
            .map(|i| s.pos[i])
            .min_by_key(|&pos| (pos - from).len_sq());
        station.or_else(|| self.nearest_enemy(w, from, Fx(i64::MAX / 4)))
    }

    fn nearest_enemy(&self, w: &World, from: Vec2, range: Fx) -> Option<Vec2> {
        let s = &w.ships;
        (0..s.len())
            .filter(|&i| s.alive[i] && s.team[i] != self.team)
            .map(|i| s.pos[i])
            .filter(|&pos| (pos - from).len() <= range)
            .min_by_key(|&pos| (pos - from).len_sq())
    }
}

fn within(a: Vec2, b: Vec2, meters: i64) -> bool {
    let r = Fx::from_int(meters);
    (a - b).len_sq() <= r * r
}

fn centroid(w: &World, ids: &[u32]) -> Option<Vec2> {
    if ids.is_empty() {
        return None;
    }
    let (mut x, mut y) = (0i64, 0i64);
    for &i in ids {
        let p = w.ships.pos[i as usize];
        x += p.x.0;
        y += p.y.0;
    }
    let n = ids.len() as i64;
    Some(Vec2::new(Fx(x / n), Fx(y / n)))
}
