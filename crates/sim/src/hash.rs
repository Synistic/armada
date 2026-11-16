use crate::world::{Order, World};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

struct Fnv(u64);

impl Fnv {
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
}

/// Hash over everything that influences future ticks. Peers compare this to
/// detect desyncs, replays compare it to detect drift.
pub fn state_hash(w: &World) -> u64 {
    let mut h = Fnv(FNV_OFFSET);
    h.u64(u64::from(w.tick));
    h.u64(w.rng.state());

    let s = &w.ships;
    for i in 0..s.len() {
        h.u64(u64::from(s.alive[i]));
        if !s.alive[i] {
            continue;
        }
        h.i64(s.pos[i].x.0);
        h.i64(s.pos[i].y.0);
        h.i64(s.heading[i].x.0);
        h.i64(s.heading[i].y.0);
        h.i64(s.speed[i].0);
        h.i64(s.vel[i].x.0);
        h.i64(s.vel[i].y.0);
        h.i64(i64::from(s.hull[i]));
        h.i64(i64::from(s.shield[i]));
        h.u64(u64::from(s.flags[i]));
        h.u64(u64::from(s.target[i]));
        match s.order[i] {
            Order::Engage => h.u64(0),
            Order::Hold => h.u64(1),
            Order::MoveTo { to, cap } | Order::AttackMove { to, cap } => {
                h.u64(if matches!(s.order[i], Order::MoveTo { .. }) {
                    2
                } else {
                    3
                });
                h.i64(to.x.0);
                h.i64(to.y.0);
                h.i64(cap.0);
            }
            Order::Attack(t) => {
                h.u64(4);
                h.u64(u64::from(t));
            }
        }
        h.u64(s.queue[i].len() as u64);
        for &q in &s.queue[i] {
            h.u64(u64::from(q));
        }
        h.u64(u64::from(s.build_progress[i]));
        h.i64(s.rally[i].x.0);
        h.i64(s.rally[i].y.0);
    }

    for t in &w.teams {
        h.u64(u64::from(t.present) | u64::from(t.has_critical) << 1 | u64::from(t.defeated) << 2);
        h.i64(i64::from(t.credits));
    }
    let pts = &w.points;
    for k in 0..pts.len() {
        h.u64(u64::from(pts.owner[k]));
        h.u64(u64::from(pts.holder[k]));
        h.u64(u64::from(pts.progress[k]));
    }
    h.u64(w.winner.map_or(u64::MAX, u64::from));

    let hp = &w.hardpoints;
    for i in 0..hp.owner.len() {
        h.i64(i64::from(hp.health[i]));
        h.u64(u64::from(hp.cooldown[i]));
        h.u64(u64::from(hp.target[i]));
    }

    let p = &w.projectiles;
    h.u64(p.len() as u64);
    for i in 0..p.len() {
        h.i64(p.pos[i].x.0);
        h.i64(p.pos[i].y.0);
        h.i64(p.vel[i].x.0);
        h.i64(p.vel[i].y.0);
        h.u64(u64::from(p.target[i]));
        h.u64(u64::from(p.ttl[i]));
    }
    h.0
}
