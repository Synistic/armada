use std::sync::Arc;

use crate::{
    MAX_TEAMS, TICK_HZ,
    command::{Command, TimedCommand},
    defs::{Defs, HardpointKind},
    fixed::Fx,
    vec::Vec2,
    world::{Event, FLAG_ENGINE_DOWN, FLAG_SHIELD_DOWN, NONE, Order, World},
};

/// Ships re-evaluate their target every N ticks, staggered by id, so the cost
/// is spread evenly instead of spiking.
const RETARGET_INTERVAL: u32 = 8;
const SEPARATION_CHECKS: usize = 12;
/// Ships start braking this many ticks of travel before their goal.
const ARRIVE_TICKS: i64 = 8;

/// One tick. Fixed phase order, fixed iteration order, no floats.
pub fn step(w: &mut World, commands: &[TimedCommand]) {
    w.events.clear();
    for c in commands {
        apply_command(w, c);
    }
    w.grid.rebuild(&w.ships.pos, &w.ships.alive);
    let centroids = enemy_centroids(w);
    retarget(w);
    movement(w, &centroids);
    weapons(w);
    shields(w);
    deaths(w);
    w.tick += 1;
}

fn apply_command(w: &mut World, tc: &TimedCommand) {
    let s = &mut w.ships;
    let (ships, order) = match &tc.command {
        Command::Move { ships, to } => (ships, Order::MoveTo(to.clamp_box(w.arena_half))),
        Command::Attack { ships, target } => {
            let t = *target as usize;
            if t >= s.len() || !s.alive[t] || s.team[t] == tc.team {
                return;
            }
            (ships, Order::Attack(*target))
        }
        Command::Stop { ships } => (ships, Order::Engage),
    };
    for &id in ships {
        let i = id as usize;
        if i < s.len() && s.alive[i] && s.team[i] == tc.team {
            s.order[i] = order;
        }
    }
}

fn enemy_centroids(w: &World) -> [Option<Vec2>; MAX_TEAMS] {
    let s = &w.ships;
    let mut sum = [(0i64, 0i64); MAX_TEAMS];
    let mut count = [0i64; MAX_TEAMS];
    for i in 0..s.len() {
        if s.alive[i] {
            let t = usize::from(s.team[i]);
            sum[t].0 += s.pos[i].x.0;
            sum[t].1 += s.pos[i].y.0;
            count[t] += 1;
        }
    }
    let total_x: i64 = sum.iter().map(|v| v.0).sum();
    let total_y: i64 = sum.iter().map(|v| v.1).sum();
    let total: i64 = count.iter().sum();

    let mut out = [None; MAX_TEAMS];
    for t in 0..MAX_TEAMS {
        let n = total - count[t];
        if n > 0 {
            out[t] = Some(Vec2::new(
                Fx((total_x - sum[t].0) / n),
                Fx((total_y - sum[t].1) / n),
            ));
        }
    }
    out
}

fn retarget(w: &mut World) {
    for i in 0..w.ships.len() {
        if !w.ships.alive[i] {
            continue;
        }
        if let Order::Attack(t) = w.ships.order[i] {
            if w.ships.alive[t as usize] {
                w.ships.target[i] = t;
                continue;
            }
            w.ships.order[i] = Order::Engage;
        }
        let current = w.ships.target[i];
        let lost = current == NONE || !w.ships.alive[current as usize];
        let due = w.tick.wrapping_add(i as u32).is_multiple_of(RETARGET_INTERVAL);
        if lost || due {
            let t = nearest_enemy(w, i);
            w.ships.target[i] = t;
        }
    }
}

fn nearest_enemy(w: &World, i: usize) -> u32 {
    let s = &w.ships;
    let p = s.pos[i];
    let team = s.team[i];
    let range = w.defs.ships[usize::from(s.def[i])].sensor_range;
    let mut best = NONE;
    let mut best_d = range * range;
    w.grid.for_each_near(p, range, |j| {
        let ju = j as usize;
        if s.team[ju] == team || !s.alive[ju] {
            return;
        }
        let d = (s.pos[ju] - p).len_sq();
        if d < best_d {
            best_d = d;
            best = j;
        }
    });
    best
}

fn movement(w: &mut World, centroids: &[Option<Vec2>; MAX_TEAMS]) {
    let defs = &*w.defs;
    let grid = &w.grid;
    let half = w.arena_half;
    let s = &mut w.ships;

    // Pass 1: velocities from current positions only, so update order does not
    // leak into the result.
    for i in 0..s.len() {
        if !s.alive[i] {
            continue;
        }
        let def = &defs.ships[usize::from(s.def[i])];
        let pos = s.pos[i];
        let target = s.target[i];
        let has_target = target != NONE && s.alive[target as usize];

        let goal = match s.order[i] {
            Order::MoveTo(p) => Some((p, Fx::ZERO)),
            Order::Attack(_) | Order::Engage if has_target => {
                Some((s.pos[target as usize], def.engage_range))
            }
            Order::Engage => centroids[usize::from(s.team[i])].map(|c| (c, Fx::ZERO)),
            Order::Attack(_) => None,
        };

        let max_speed = if s.flags[i] & FLAG_ENGINE_DOWN != 0 {
            def.max_speed / 2
        } else {
            def.max_speed
        };

        let desired = match goal {
            Some((g, standoff)) => {
                let to = g - pos;
                let gap = to.len() - standoff;
                if gap <= Fx::ZERO {
                    Vec2::ZERO
                } else {
                    to.normalize_or_zero()
                        .scale(max_speed.min(gap / ARRIVE_TICKS))
                }
            }
            None => Vec2::ZERO,
        };

        let mut push = Vec2::ZERO;
        for &j in grid.cell_members(pos).iter().take(SEPARATION_CHECKS + 1) {
            let ju = j as usize;
            if ju == i {
                continue;
            }
            let min_d = def.radius + defs.ships[usize::from(s.def[ju])].radius;
            let away = pos - s.pos[ju];
            if away.len_sq() < min_d * min_d {
                push += if away == Vec2::ZERO {
                    // Exact overlap: split along x by id so both sides agree.
                    let dir = if i < ju { -Fx::ONE } else { Fx::ONE };
                    Vec2::new(dir, Fx::ZERO)
                } else {
                    away.normalize_or_zero()
                };
            }
        }

        let steer = (desired - s.vel[i]).clamp_len(def.accel) + push.scale(def.accel);
        s.vel[i] = (s.vel[i] + steer).clamp_len(max_speed);
    }

    // Pass 2: integrate.
    for i in 0..s.len() {
        if s.alive[i] {
            s.pos[i] = (s.pos[i] + s.vel[i]).clamp_box(half);
        }
    }
}

fn weapons(w: &mut World) {
    let defs = Arc::clone(&w.defs);
    for h in 0..w.hardpoints.owner.len() {
        if w.hardpoints.health[h] <= 0 {
            continue;
        }
        let owner = w.hardpoints.owner[h] as usize;
        // `hull <= 0` catches ships that died earlier in this same tick.
        if !w.ships.alive[owner] || w.ships.hull[owner] <= 0 {
            continue;
        }
        let sdef = &defs.ships[usize::from(w.ships.def[owner])];
        let HardpointKind::Weapon(weapon) = &sdef.hardpoints[usize::from(w.hardpoints.slot[h])].kind
        else {
            continue;
        };
        if w.hardpoints.cooldown[h] > 0 {
            w.hardpoints.cooldown[h] -= 1;
            continue;
        }

        let target = w.ships.target[owner];
        if target == NONE {
            continue;
        }
        let t = target as usize;
        if !w.ships.alive[t] || w.ships.hull[t] <= 0 {
            continue;
        }
        let reach = weapon.range + defs.ships[usize::from(w.ships.def[t])].radius;
        if (w.ships.pos[t] - w.ships.pos[owner]).len_sq() > reach * reach {
            continue;
        }

        w.hardpoints.cooldown[h] = weapon.cooldown_ticks;
        let hit = w.rng.below(100) < u32::from(weapon.accuracy_pct);
        w.events.push(Event::Shot {
            hardpoint: h as u32,
            target,
            hit,
        });
        if hit {
            apply_damage(w, &defs, t, weapon.damage);
        }
    }
}

/// Shield first. What gets through hits a random intact hardpoint at full
/// strength and the hull at half. No hardpoints left: the hull takes it all.
fn apply_damage(w: &mut World, defs: &Defs, t: usize, damage: i32) {
    let s = &mut w.ships;
    let absorbed = damage.min(s.shield[t]);
    s.shield[t] -= absorbed;
    let rest = damage - absorbed;
    if rest <= 0 {
        return;
    }

    let first = s.hp_first[t] as usize;
    let count = usize::from(s.hp_count[t]);
    if count > 0 {
        let start = w.rng.below(count as u32) as usize;
        for k in 0..count {
            let h = first + (start + k) % count;
            if w.hardpoints.health[h] <= 0 {
                continue;
            }
            w.hardpoints.health[h] -= rest;
            if w.hardpoints.health[h] <= 0 {
                w.events.push(Event::HardpointDestroyed(h as u32));
                let slot = usize::from(w.hardpoints.slot[h]);
                match defs.ships[usize::from(s.def[t])].hardpoints[slot].kind {
                    HardpointKind::ShieldGenerator => s.flags[t] |= FLAG_SHIELD_DOWN,
                    HardpointKind::Engine => s.flags[t] |= FLAG_ENGINE_DOWN,
                    HardpointKind::Weapon(_) => {}
                }
            }
            s.hull[t] -= rest / 2;
            return;
        }
    }
    s.hull[t] -= rest;
}

fn shields(w: &mut World) {
    let s = &mut w.ships;
    for i in 0..s.len() {
        if !s.alive[i] || s.flags[i] & FLAG_SHIELD_DOWN != 0 {
            continue;
        }
        if !w.tick.wrapping_add(i as u32).is_multiple_of(TICK_HZ) {
            continue;
        }
        let def = &w.defs.ships[usize::from(s.def[i])];
        s.shield[i] = (s.shield[i] + def.shield_regen).min(def.shield);
    }
}

fn deaths(w: &mut World) {
    let s = &mut w.ships;
    for i in 0..s.len() {
        if s.alive[i] && s.hull[i] <= 0 {
            s.alive[i] = false;
            s.vel[i] = Vec2::ZERO;
            w.events.push(Event::ShipDestroyed(i as u32));
        }
    }
}
