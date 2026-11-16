//! Skirmish rules: credits, population, build queues, capture points and who
//! won. All of it runs inside the tick like everything else.

use std::sync::Arc;

use crate::{
    MAX_TEAMS, TICK_HZ,
    fixed::Fx,
    world::{Event, Order, World},
};

/// No team. Used for neutral capture points.
pub const NO_TEAM: u8 = u8::MAX;
/// Items a single producer can have queued.
pub const MAX_QUEUE: usize = 5;

/// Battle-wide economy settings from the scenario. The defaults turn the
/// economy off, which is what a plain fleet battle wants.
#[derive(Clone, Debug)]
pub struct Rules {
    /// Credits per second for every team still in the battle.
    pub income: i32,
    /// Extra credits per second for each owned capture point.
    pub point_income: i32,
    pub pop_cap: u16,
    pub capture_radius: Fx,
    /// Seconds a lone team needs inside the radius to take a neutral point.
    pub capture_secs: u16,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            income: 0,
            point_income: 0,
            pop_cap: u16::MAX,
            capture_radius: Fx::ZERO,
            capture_secs: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Team {
    /// Spawned at least one ship, so it takes part in the battle.
    pub present: bool,
    /// Had a critical ship (a station). Then losing all of them loses the
    /// battle, otherwise losing every ship does.
    pub has_critical: bool,
    pub defeated: bool,
    pub credits: i32,
}

/// Capture points, struct of arrays.
#[derive(Clone, Debug, Default)]
pub struct Points {
    pub pos: Vec<crate::vec::Vec2>,
    /// Team that owns the point and gets its income, or `NO_TEAM`.
    pub owner: Vec<u8>,
    /// Team whose progress is on the point, or `NO_TEAM`.
    pub holder: Vec<u8>,
    /// Seconds of progress for `holder`, up to `Rules::capture_secs`.
    pub progress: Vec<u16>,
}

impl Points {
    pub fn len(&self) -> usize {
        self.pos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    pub fn add(&mut self, pos: crate::vec::Vec2) {
        self.pos.push(pos);
        self.owner.push(NO_TEAM);
        self.holder.push(NO_TEAM);
        self.progress.push(0);
    }
}

/// Population a team uses: alive ships plus everything in its queues.
pub fn pop_used(w: &World, team: u8) -> u32 {
    let s = &w.ships;
    let mut used = 0u32;
    for i in 0..s.len() {
        if !s.alive[i] || s.team[i] != team {
            continue;
        }
        used += u32::from(w.defs.ships[usize::from(s.def[i])].pop);
        for &q in &s.queue[i] {
            used += u32::from(w.defs.ships[usize::from(q)].pop);
        }
    }
    used
}

pub fn owned_points(w: &World, team: u8) -> usize {
    w.points.owner.iter().filter(|&&o| o == team).count()
}

/// Credits per second the team gets right now.
pub fn income(w: &World, team: u8) -> i32 {
    w.rules.income + w.rules.point_income * owned_points(w, team) as i32
}

/// Queue a ship. Pays up front, so a full queue cannot outspend the bank.
pub fn build(w: &mut World, team: u8, producer: u32, ship: u16) {
    let p = producer as usize;
    let s = &w.ships;
    if p >= s.len() || !s.alive[p] || s.team[p] != team || s.queue[p].len() >= MAX_QUEUE {
        return;
    }
    let pdef = &w.defs.ships[usize::from(s.def[p])];
    if !pdef.builds.contains(&ship) {
        return;
    }
    let def = &w.defs.ships[usize::from(ship)];
    let t = usize::from(team);
    if w.teams[t].credits < def.cost
        || pop_used(w, team) + u32::from(def.pop) > u32::from(w.rules.pop_cap)
    {
        return;
    }
    w.teams[t].credits -= def.cost;
    w.ships.queue[p].push(ship);
}

/// Remove the last queued ship and refund it in full.
pub fn cancel(w: &mut World, team: u8, producer: u32) {
    let p = producer as usize;
    let s = &mut w.ships;
    if p >= s.len() || !s.alive[p] || s.team[p] != team {
        return;
    }
    let Some(ship) = s.queue[p].pop() else {
        return;
    };
    if s.queue[p].is_empty() {
        s.build_progress[p] = 0;
    }
    w.teams[usize::from(team)].credits += w.defs.ships[usize::from(ship)].cost;
}

/// Once per second: points with exactly one team inside move toward that team.
/// A point held by someone else first drains to neutral.
pub fn capture(w: &mut World) {
    if w.points.is_empty() || !w.tick.is_multiple_of(TICK_HZ) {
        return;
    }
    let radius = w.rules.capture_radius;
    let full = w.rules.capture_secs.max(1);
    for k in 0..w.points.len() {
        let pos = w.points.pos[k];
        let mut present: Option<u8> = None;
        let mut contested = false;
        let s = &w.ships;
        w.grid.for_each_near(pos, radius, |j| {
            let j = j as usize;
            if !s.alive[j] || (s.pos[j] - pos).len_sq() > radius * radius {
                return;
            }
            match present {
                None => present = Some(s.team[j]),
                Some(t) if t != s.team[j] => contested = true,
                Some(_) => {}
            }
        });
        let (Some(team), false) = (present, contested) else {
            continue;
        };
        let pts = &mut w.points;
        if pts.holder[k] == team || pts.holder[k] == NO_TEAM {
            pts.holder[k] = team;
            pts.progress[k] = (pts.progress[k] + 1).min(full);
            if pts.progress[k] == full {
                pts.owner[k] = team;
            }
        } else {
            pts.progress[k] = pts.progress[k].saturating_sub(1);
            if pts.progress[k] == 0 {
                pts.holder[k] = NO_TEAM;
                pts.owner[k] = NO_TEAM;
            }
        }
    }
}

/// Once per second, for every team still fighting.
pub fn pay(w: &mut World) {
    if !w.tick.is_multiple_of(TICK_HZ) {
        return;
    }
    for t in 0..MAX_TEAMS {
        if w.teams[t].present && !w.teams[t].defeated {
            let gain = income(w, t as u8);
            w.teams[t].credits += gain;
        }
    }
}

/// Advance the front of every queue, spawn finished ships in front of their
/// producer and send them to its rally point.
pub fn produce(w: &mut World) {
    let defs = Arc::clone(&w.defs);
    let count = w.ships.len();
    for p in 0..count {
        if !w.ships.alive[p] || w.ships.queue[p].is_empty() {
            continue;
        }
        let ship = w.ships.queue[p][0];
        let def = &defs.ships[usize::from(ship)];
        w.ships.build_progress[p] += 1;
        if w.ships.build_progress[p] < def.build_ticks.max(1) {
            continue;
        }
        w.ships.queue[p].remove(0);
        w.ships.build_progress[p] = 0;

        let heading = w.ships.heading[p];
        let pdef = &defs.ships[usize::from(w.ships.def[p])];
        let gap = pdef.radius + def.radius + Fx::from_int(30);
        let pos = w.ships.pos[p] + heading.scale(gap);
        let rally = w.ships.rally[p];
        let team = w.ships.team[p];
        let id = w.spawn_ship(ship, team, pos, heading);
        w.ships.order[id as usize] = Order::MoveTo {
            to: rally,
            cap: def.max_speed,
        };
        w.events.push(Event::ShipBuilt(id));
    }
}

/// Mark beaten teams and the winner once only one team is left.
pub fn outcome(w: &mut World) {
    if w.winner.is_some() {
        return;
    }
    let mut alive = [false; MAX_TEAMS];
    let mut critical = [false; MAX_TEAMS];
    let s = &w.ships;
    for i in 0..s.len() {
        if s.alive[i] {
            let t = usize::from(s.team[i]);
            alive[t] = true;
            critical[t] |= w.defs.ships[usize::from(s.def[i])].critical;
        }
    }
    let mut left = Vec::new();
    for t in 0..MAX_TEAMS {
        let team = &mut w.teams[t];
        if !team.present || team.defeated {
            continue;
        }
        team.defeated = if team.has_critical {
            !critical[t]
        } else {
            !alive[t]
        };
        if !team.defeated {
            left.push(t as u8);
        }
    }
    if let [winner] = left[..] {
        w.winner = Some(winner);
    }
}
