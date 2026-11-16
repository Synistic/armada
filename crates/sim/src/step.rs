use std::sync::Arc;

use crate::{
    MAX_TEAMS, TICK_HZ,
    command::{Command, TimedCommand},
    defs::{Defs, Delivery, HardpointKind, WeaponDef},
    fixed::Fx,
    formation::line_formation,
    grid::Grid,
    vec::Vec2,
    world::{Event, FLAG_ENGINE_DOWN, FLAG_SHIELD_DOWN, NONE, Order, Ships, World},
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
    projectiles(w);
    shields(w);
    deaths(w);
    w.tick += 1;
}

/// Alive ships of `team` from `ids`, sorted and deduplicated so the result
/// does not depend on how the client built the list.
fn own_ships(s: &Ships, team: u8, ids: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|&id| {
            let i = id as usize;
            i < s.len() && s.alive[i] && s.team[i] == team
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn apply_command(w: &mut World, tc: &TimedCommand) {
    match &tc.command {
        Command::Move { ships, to } | Command::AttackMove { ships, to } => {
            let ids = own_ships(&w.ships, tc.team, ships);
            let Some(cap) = ids.iter().map(|&i| w.max_speed(i as usize)).min() else {
                return;
            };
            let attack = matches!(tc.command, Command::AttackMove { .. });
            let half = w.arena_half;
            for (id, slot) in line_formation(&w.ships, &w.defs, &ids, to.clamp_box(half)) {
                let to = slot.clamp_box(half);
                w.ships.order[id as usize] = if attack {
                    Order::AttackMove { to, cap }
                } else {
                    Order::MoveTo { to, cap }
                };
            }
        }
        Command::Attack { ships, target } => {
            let s = &w.ships;
            let t = *target as usize;
            if t >= s.len() || !s.alive[t] || s.team[t] == tc.team {
                return;
            }
            for id in own_ships(s, tc.team, ships) {
                w.ships.order[id as usize] = Order::Attack(*target);
            }
        }
        Command::Stop { ships } => {
            for id in own_ships(&w.ships, tc.team, ships) {
                w.ships.order[id as usize] = Order::Hold;
            }
        }
        Command::Engage { ships } => {
            for id in own_ships(&w.ships, tc.team, ships) {
                w.ships.order[id as usize] = Order::Engage;
            }
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
            w.ships.order[i] = Order::Hold;
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

/// What a ship wants to do with its hull this tick.
enum Goal {
    /// Travel to a point at no more than this speed.
    Point(Vec2, Fx),
    /// Close to engage range of the target, then present the best arc.
    Fight(Vec2),
    /// Do not move, only turn the best arc toward the target.
    Face(Vec2),
    Stay,
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
        let heading = s.heading[i];
        let target = s.target[i];
        let target_pos = (target != NONE && s.alive[target as usize]).then(|| s.pos[target as usize]);
        let max_speed = if s.flags[i] & FLAG_ENGINE_DOWN != 0 {
            def.max_speed / 2
        } else {
            def.max_speed
        };

        let goal = match (s.order[i], target_pos) {
            (Order::MoveTo { to, cap }, _) => Goal::Point(to, cap),
            (Order::AttackMove { .. } | Order::Attack(_) | Order::Engage, Some(t)) => Goal::Fight(t),
            (Order::AttackMove { to, cap }, None) => Goal::Point(to, cap),
            (Order::Engage, None) => match centroids[usize::from(s.team[i])] {
                Some(c) => Goal::Point(c, max_speed),
                None => Goal::Stay,
            },
            (Order::Hold, Some(t)) => Goal::Face(t),
            (Order::Hold | Order::Attack(_), None) => Goal::Stay,
        };

        let face = |t: Vec2| (t - pos).normalize_or_zero().heading_for_bearing(def.preferred_bearing);
        let (dir, want_speed) = match goal {
            Goal::Point(p, cap) => {
                let to = p - pos;
                let dist = to.len();
                if dist <= def.radius {
                    (heading, Fx::ZERO)
                } else {
                    let speed = max_speed.min(cap).min(dist / ARRIVE_TICKS);
                    (to.normalize_or_zero(), speed)
                }
            }
            Goal::Fight(t) => {
                let to = t - pos;
                let gap = to.len() - def.engage_range;
                if gap > Fx::ZERO {
                    (to.normalize_or_zero(), max_speed.min(gap / ARRIVE_TICKS))
                } else {
                    (face(t), Fx::ZERO)
                }
            }
            Goal::Face(t) => (face(t), Fx::ZERO),
            Goal::Stay => (heading, Fx::ZERO),
        };

        let heading = heading.turn_towards(dir, def.turn_cos, def.turn_sin);
        // Thrust only along the heading: a ship facing the wrong way slows
        // down and turns first.
        let align = heading.dot(dir).max(Fx::ZERO);
        let target_speed = want_speed * align;
        let speed = s.speed[i] + (target_speed - s.speed[i]).clamp(-def.accel, def.accel);

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

        let nudge = push.clamp_len(Fx::ONE).scale(max_speed / 4);
        s.heading[i] = heading;
        s.speed[i] = speed;
        s.vel[i] = heading.scale(speed) + nudge;
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

        // Ship target first, then the cached target of opportunity, then a
        // fresh search. The search is staggered like ship retargeting.
        let (target, mount_pos) = {
            let s = &w.ships;
            let mount = Mount {
                pos: w.hardpoint_pos(h),
                heading: s.heading[owner],
                team: s.team[owner],
                weapon,
            };
            let ship_target = s.target[owner];
            let cached = w.hardpoints.target[h];
            let target = if mount.can_hit(s, &defs, ship_target) {
                ship_target
            } else if mount.can_hit(s, &defs, cached) {
                cached
            } else if cached != NONE
                || w.tick.wrapping_add(h as u32).is_multiple_of(RETARGET_INTERVAL)
            {
                mount.find_target(s, &defs, &w.grid)
            } else {
                NONE
            };
            (target, mount.pos)
        };
        if target != w.ships.target[owner] {
            w.hardpoints.target[h] = target;
        }
        if target == NONE {
            continue;
        }
        let t = target as usize;

        w.hardpoints.cooldown[h] = weapon.cooldown_ticks;
        let hit = w.rng.below(100) < u32::from(weapon.accuracy_pct);
        match weapon.delivery {
            Delivery::Beam => {
                w.events.push(Event::Shot {
                    hardpoint: h as u32,
                    target,
                    hit,
                });
                if hit {
                    apply_damage(w, &defs, t, weapon.damage, weapon.shield_bypass_pct);
                }
            }
            Delivery::Projectile { speed, ttl_ticks } => {
                let dir = (w.ships.pos[t] - mount_pos).normalize_or_zero();
                let p = &mut w.projectiles;
                p.pos.push(mount_pos);
                p.vel.push(dir.scale(speed));
                // A miss keeps flying straight and expires.
                p.target.push(if hit { target } else { NONE });
                p.damage.push(weapon.damage);
                p.bypass.push(weapon.shield_bypass_pct);
                p.ttl.push(ttl_ticks.max(1));
                p.team.push(w.ships.team[owner]);
                w.events.push(Event::Launch {
                    hardpoint: h as u32,
                });
            }
        }
    }
}

/// Homing: steer straight at the target each tick, impact when within one
/// tick of travel plus the target's radius.
fn projectiles(w: &mut World) {
    let defs = Arc::clone(&w.defs);
    let mut i = 0;
    while i < w.projectiles.len() {
        let t = w.projectiles.target[i];
        if t != NONE {
            let tu = t as usize;
            if w.ships.alive[tu] && w.ships.hull[tu] > 0 {
                let speed = w.projectiles.vel[i].len();
                let to = w.ships.pos[tu] - w.projectiles.pos[i];
                let reach = speed + defs.ships[usize::from(w.ships.def[tu])].radius;
                if to.len_sq() <= reach * reach {
                    let (damage, bypass) = (w.projectiles.damage[i], w.projectiles.bypass[i]);
                    w.events.push(Event::Impact {
                        pos: w.ships.pos[tu],
                        target: t,
                    });
                    apply_damage(w, &defs, tu, damage, bypass);
                    w.projectiles.swap_remove(i);
                    continue;
                }
                w.projectiles.vel[i] = to.normalize_or_zero().scale(speed);
            } else {
                w.projectiles.target[i] = NONE;
            }
        }
        let next = w.projectiles.pos[i] + w.projectiles.vel[i];
        w.projectiles.pos[i] = next;
        w.projectiles.ttl[i] -= 1;
        if w.projectiles.ttl[i] == 0 {
            w.projectiles.swap_remove(i);
            continue;
        }
        i += 1;
    }
}

/// A weapon hardpoint in world space for one tick.
struct Mount<'a> {
    pos: Vec2,
    heading: Vec2,
    team: u8,
    weapon: &'a WeaponDef,
}

impl Mount<'_> {
    fn can_hit(&self, s: &Ships, defs: &Defs, t: u32) -> bool {
        if t == NONE {
            return false;
        }
        let t = t as usize;
        if !s.alive[t] || s.hull[t] <= 0 || s.team[t] == self.team {
            return false;
        }
        let to = s.pos[t] - self.pos;
        let reach = self.weapon.range + defs.ships[usize::from(s.def[t])].radius;
        if to.len_sq() > reach * reach {
            return false;
        }
        self.weapon.arc_cos <= -Fx::ONE
            || to.to_local(self.heading).normalize_or_zero().dot(self.weapon.facing)
                >= self.weapon.arc_cos
    }

    fn find_target(&self, s: &Ships, defs: &Defs, grid: &Grid) -> u32 {
        let mut best = NONE;
        let mut best_d = Fx(i64::MAX);
        grid.for_each_near(self.pos, self.weapon.range, |j| {
            if self.can_hit(s, defs, j) {
                let d = (s.pos[j as usize] - self.pos).len_sq();
                if d < best_d {
                    best_d = d;
                    best = j;
                }
            }
        });
        best
    }
}

/// The bypass share skips the shield. The shield absorbs what it can of the
/// rest. Whatever gets through hits a random intact hardpoint at full strength
/// and the hull at half; with no hardpoints left the hull takes it all.
fn apply_damage(w: &mut World, defs: &Defs, t: usize, damage: i32, bypass_pct: u8) {
    let s = &mut w.ships;
    let bypass = damage * i32::from(bypass_pct) / 100;
    let absorbed = (damage - bypass).min(s.shield[t]);
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
            s.speed[i] = Fx::ZERO;
            s.vel[i] = Vec2::ZERO;
            w.events.push(Event::ShipDestroyed(i as u32));
        }
    }
}
