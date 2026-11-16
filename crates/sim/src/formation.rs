use crate::{defs::Defs, fixed::Fx, vec::Vec2, world::Ships};

/// Slots for a group moving to `to`: ranks across the direction of travel,
/// light ships in front, heavy ones behind. Inside a rank ships keep their
/// left-to-right order, so paths do not cross. `ids` must be sorted and alive.
pub fn line_formation(ships: &Ships, defs: &Defs, ids: &[u32], to: Vec2) -> Vec<(u32, Vec2)> {
    let Some(&first) = ids.first() else {
        return Vec::new();
    };
    let n = ids.len() as i64;
    let (sx, sy) = ids.iter().fold((0i64, 0i64), |acc, &i| {
        let p = ships.pos[i as usize];
        (acc.0 + p.x.0, acc.1 + p.y.0)
    });
    let centroid = Vec2::new(Fx(sx / n), Fx(sy / n));
    let forward = match (to - centroid).normalize_or_zero() {
        Vec2::ZERO => ships.heading[first as usize],
        d => d,
    };
    let right = Vec2::new(forward.y, -forward.x);
    let radius = |i: u32| defs.ships[usize::from(ships.def[i as usize])].radius;

    let mut by_size = ids.to_vec();
    by_size.sort_by_key(|&i| (radius(i), i));
    // Roughly twice as wide as deep.
    let columns = ((ids.len() as u64 * 2).isqrt() as usize).max(1);

    let mut out = Vec::with_capacity(ids.len());
    let mut row_y = Fx::ZERO;
    let mut prev_spacing = None;
    for rank in by_size.chunks(columns) {
        let spacing = rank.iter().map(|&i| radius(i)).max().unwrap_or(Fx::ONE) * 3;
        if let Some(prev) = prev_spacing {
            row_y -= (prev + spacing) / 2;
        }
        prev_spacing = Some(spacing);

        let mut rank = rank.to_vec();
        rank.sort_by_key(|&i| ((ships.pos[i as usize] - centroid).dot(right), i));
        let width = spacing * (rank.len() as i64 - 1);
        for (k, &id) in rank.iter().enumerate() {
            let local = Vec2::new(spacing * k as i64 - width / 2, row_y);
            out.push((id, to + local.from_local(forward)));
        }
    }
    out
}
