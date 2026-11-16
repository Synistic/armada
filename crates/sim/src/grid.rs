use crate::{fixed::Fx, vec::Vec2};

/// Uniform grid over the arena, rebuilt every tick. Dense vectors instead of a
/// hash map, so iteration order is fixed and the cost is predictable.
#[derive(Clone, Debug)]
pub struct Grid {
    origin: Fx,
    cell: Fx,
    dim: i32,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    pub fn new(arena_half: Fx, cell: Fx) -> Self {
        let dim = ((arena_half * 2) / cell).to_int() as i32 + 1;
        Self {
            origin: -arena_half,
            cell,
            dim,
            cells: vec![Vec::new(); (dim * dim) as usize],
        }
    }

    fn coord(&self, v: Fx) -> i32 {
        (((v - self.origin) / self.cell).to_int() as i32).clamp(0, self.dim - 1)
    }

    fn index(&self, cx: i32, cy: i32) -> usize {
        (cy * self.dim + cx) as usize
    }

    pub fn rebuild(&mut self, pos: &[Vec2], alive: &[bool]) {
        for c in &mut self.cells {
            c.clear();
        }
        for (i, (p, a)) in pos.iter().zip(alive).enumerate() {
            if *a {
                let idx = self.index(self.coord(p.x), self.coord(p.y));
                self.cells[idx].push(i as u32);
            }
        }
    }

    pub fn cell_members(&self, p: Vec2) -> &[u32] {
        &self.cells[self.index(self.coord(p.x), self.coord(p.y))]
    }

    /// Visits every ship in cells overlapping the square around `p`. Callers do
    /// the exact distance check.
    pub fn for_each_near(&self, p: Vec2, radius: Fx, mut f: impl FnMut(u32)) {
        let (x0, x1) = (self.coord(p.x - radius), self.coord(p.x + radius));
        let (y0, y1) = (self.coord(p.y - radius), self.coord(p.y + radius));
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                for &id in &self.cells[self.index(cx, cy)] {
                    f(id);
                }
            }
        }
    }
}
