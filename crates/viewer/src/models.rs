//! Ship models, built in code. A hull is a few cross sections lofted into a
//! skin, the superstructure is boxes and towers, and turrets go where the
//! ship definition has its hardpoints. Faces are flat shaded and get a slight
//! random tint, which reads as hull plating.
//!
//! Model frame: +z bow, +x starboard, +y up, lengths in ship radii.

use std::f32::consts::TAU;

use armada_sim::{Delivery, HardpointKind, ShipDef};
use bevy::{asset::RenderAssetUsages, math::Affine3A, mesh::PrimitiveTopology, prelude::*};

/// Linear colors.
const HULL: Vec3 = Vec3::new(0.2, 0.21, 0.235);
const DARK: Vec3 = Vec3::new(0.03, 0.033, 0.04);
/// Face index of a loft's end caps in the paint callback.
const CAP: usize = usize::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Paint {
    Hull,
    Dark,
    /// Team color.
    Accent,
    /// Engines and lights. Goes into its own mesh with a glowing material.
    Glow,
}

#[derive(Default)]
struct Part {
    pos: Vec<[f32; 3]>,
    normal: Vec<[f32; 3]>,
    color: Vec<[f32; 4]>,
}

impl Part {
    fn mesh(self, scale: f32) -> Mesh {
        let pos: Vec<[f32; 3]> = self
            .pos
            .iter()
            .map(|p| [p[0] * scale, p[1] * scale, p[2] * scale])
            .collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.color)
    }
}

/// Hull cross section: flat plates on top and bottom, a sharp edge with a
/// trench at the sides, and a center strip on top that can carry a stripe.
#[derive(Clone, Copy)]
struct Sec {
    z: f32,
    x: f32,
    /// Half width at the side edge.
    w: f32,
    top: f32,
    bot: f32,
    /// Half width of the top and bottom plate as a share of `w`.
    tw: f32,
    bw: f32,
    /// Half height of the side trench.
    e: f32,
}

fn sec(z: f32, w: f32, top: f32, bot: f32) -> Sec {
    Sec {
        z,
        x: 0.0,
        w,
        top,
        bot,
        tw: 0.5,
        bw: 0.4,
        e: 0.02,
    }
}

impl Sec {
    /// Faces between two rings, by index: 0 and 2 top plate, 1 center strip,
    /// 3 and 9 upper slopes, 4 and 8 side trench, 5 and 7 lower slopes,
    /// 6 bottom.
    fn ring(self) -> Vec<Vec3> {
        let Sec {
            z,
            x,
            w,
            top,
            bot,
            tw,
            bw,
            e,
        } = self;
        let e = e.min(top).min(-bot);
        let strip = w * tw * 0.22;
        [
            (-w * tw, top),
            (-strip, top),
            (strip, top),
            (w * tw, top),
            (w, e),
            (w, -e),
            (w * bw, bot),
            (-w * bw, bot),
            (-w, -e),
            (-w, e),
        ]
        .map(|(px, py)| Vec3::new(x + px, py, z))
        .to_vec()
    }
}

fn rect_z(z: f32, x: f32, y: f32, hx: f32, hy: f32) -> Vec<Vec3> {
    vec![
        Vec3::new(x - hx, y + hy, z),
        Vec3::new(x + hx, y + hy, z),
        Vec3::new(x + hx, y - hy, z),
        Vec3::new(x - hx, y - hy, z),
    ]
}

fn rect_y(y: f32, x: f32, z: f32, hx: f32, hz: f32) -> Vec<Vec3> {
    vec![
        Vec3::new(x - hx, y, z + hz),
        Vec3::new(x + hx, y, z + hz),
        Vec3::new(x + hx, y, z - hz),
        Vec3::new(x - hx, y, z - hz),
    ]
}

fn ngon_y(y: f32, r: f32, n: usize) -> Vec<Vec3> {
    (0..n)
        .map(|k| {
            let a = (k as f32 + 0.5) / n as f32 * TAU;
            Vec3::new(r * a.sin(), y, r * a.cos())
        })
        .collect()
}

fn ngon_z(z: f32, x: f32, y: f32, r: f32, n: usize) -> Vec<Vec3> {
    (0..n)
        .map(|k| {
            let a = (k as f32 + 0.5) / n as f32 * TAU;
            Vec3::new(x + r * a.sin(), y + r * a.cos(), z)
        })
        .collect()
}

fn centroid(points: &[Vec3]) -> Vec3 {
    points.iter().sum::<Vec3>() / points.len() as f32
}

struct Builder {
    hull: Part,
    glow: Part,
    accent: Vec3,
    faces: u32,
    /// Applied to every point, for rotated and shifted parts.
    xf: Affine3A,
}

impl Builder {
    fn new(accent: Vec3) -> Self {
        Self {
            hull: Part::default(),
            glow: Part::default(),
            accent,
            faces: 0,
            xf: Affine3A::IDENTITY,
        }
    }

    /// One flat face. `inside` is any point behind it, which decides the
    /// winding so callers never have to think about it.
    fn poly(&mut self, points: &[Vec3], inside: Vec3, paint: Paint) {
        let pts: Vec<Vec3> = points
            .iter()
            .map(|&p| self.xf.transform_point3(p))
            .collect();
        let inside = self.xf.transform_point3(inside);
        // Newell's method, fine with quads that are not quite planar.
        let mut n = Vec3::ZERO;
        for (i, a) in pts.iter().enumerate() {
            let b = pts[(i + 1) % pts.len()];
            n += Vec3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        if n.length_squared() < 1e-12 {
            return;
        }
        let mut n = n.normalize();
        let flip = n.dot(centroid(&pts) - inside) < 0.0;
        if flip {
            n = -n;
        }
        self.faces += 1;
        let tint = 0.86
            + 0.24 * (self.faces.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32;
        let rgb = match paint {
            Paint::Hull => HULL * tint,
            Paint::Dark => DARK * tint,
            Paint::Accent => self.accent * (0.9 + 0.1 * tint),
            Paint::Glow => Vec3::ONE,
        };
        let part = if paint == Paint::Glow {
            &mut self.glow
        } else {
            &mut self.hull
        };
        for i in 1..pts.len() - 1 {
            let (b, c) = if flip {
                (pts[i + 1], pts[i])
            } else {
                (pts[i], pts[i + 1])
            };
            for p in [pts[0], b, c] {
                part.pos.push(p.to_array());
                part.normal.push(n.to_array());
                part.color.push([rgb.x, rgb.y, rgb.z, 1.0]);
            }
        }
    }

    /// Skin over a row of rings with the same point count. `paint` gets the
    /// segment and the face index within it, `CAP` for the two ends.
    fn loft(&mut self, rings: &[Vec<Vec3>], caps: bool, paint: impl Fn(usize, usize) -> Paint) {
        for (i, pair) in rings.windows(2).enumerate() {
            let n = pair[0].len();
            let axis = (centroid(&pair[0]) + centroid(&pair[1])) / 2.0;
            for k in 0..n {
                let k1 = (k + 1) % n;
                self.poly(
                    &[pair[0][k], pair[0][k1], pair[1][k1], pair[1][k]],
                    axis,
                    paint(i, k),
                );
            }
        }
        if caps {
            let all: Vec<Vec3> = rings.iter().flatten().copied().collect();
            let center = centroid(&all);
            self.poly(&rings[0], center, paint(0, CAP));
            self.poly(&rings[rings.len() - 1], center, paint(rings.len() - 2, CAP));
        }
    }

    /// Main hull from bow to stern. Segments in `stripe` get a team colored
    /// center strip, the side trench is dark with running lights.
    fn hull(&mut self, secs: &[Sec], stripe: std::ops::Range<usize>) {
        let rings: Vec<Vec<Vec3>> = secs.iter().map(|s| s.ring()).collect();
        self.loft(&rings, true, |i, k| match k {
            1 if stripe.contains(&i) => Paint::Accent,
            4 | 8 | CAP => Paint::Dark,
            _ => Paint::Hull,
        });
        for pair in secs.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let count = ((a.z - b.z).abs() / 0.11).floor().max(1.0) as usize;
            for k in 0..count {
                let t = (k as f32 + 0.5) / count as f32;
                let (z, x, w) = (
                    a.z + (b.z - a.z) * t,
                    a.x + (b.x - a.x) * t,
                    a.w + (b.w - a.w) * t,
                );
                for side in [-1.0, 1.0] {
                    let edge = x + side * w;
                    self.block(
                        Vec3::new(edge - 0.008, -0.007, z - 0.02),
                        Vec3::new(edge + 0.008, 0.007, z + 0.02),
                        Paint::Glow,
                    );
                }
            }
        }
    }

    fn block(&mut self, lo: Vec3, hi: Vec3, paint: Paint) {
        let c = (lo + hi) / 2.0;
        let h = (hi - lo) / 2.0;
        self.loft(
            &[
                rect_z(lo.z, c.x, c.y, h.x, h.y),
                rect_z(hi.z, c.x, c.y, h.x, h.y),
            ],
            true,
            |_, _| paint,
        );
    }

    /// Box that can taper and lean: a footprint at `y0` and one at `y1`,
    /// each as (x, z, half x, half z).
    fn tower(&mut self, y0: f32, a: [f32; 4], y1: f32, b: [f32; 4], paint: Paint) {
        self.loft(
            &[
                rect_y(y0, a[0], a[1], a[2], a[3]),
                rect_y(y1, b[0], b[1], b[2], b[3]),
            ],
            true,
            |_, _| paint,
        );
    }

    /// Nozzle sticking out of the stern at `z`, closed by a glowing disc.
    fn engine(&mut self, x: f32, y: f32, z: f32, r: f32) {
        let back = z - r * 0.6;
        self.loft(
            &[ngon_z(z + r, x, y, r * 0.8, 8), ngon_z(back, x, y, r, 8)],
            false,
            |_, _| Paint::Dark,
        );
        self.poly(&ngon_z(back, x, y, r, 8), Vec3::new(x, y, z), Paint::Glow);
    }

    /// Highest hull surface above (x, z), if there is hull there.
    fn top_at(&self, x: f32, z: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for tri in self.hull.pos.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(Vec3::from_array);
            let det = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
            if det.abs() < 1e-9 {
                continue;
            }
            let u = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / det;
            let v = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / det;
            if u < 0.0 || v < 0.0 || u + v > 1.0 {
                continue;
            }
            let y = u * a.y + v * b.y + (1.0 - u - v) * c.y;
            best = Some(best.map_or(y, |h| h.max(y)));
        }
        best
    }

    /// Move the origin to `at` and turn +z towards `dir` (x, z) for the parts
    /// built in `f`.
    fn at(&mut self, at: Vec3, dir: Vec2, f: impl FnOnce(&mut Self)) {
        self.xf = Affine3A::from_translation(at) * Affine3A::from_rotation_y(dir.x.atan2(dir.y));
        f(self);
        self.xf = Affine3A::IDENTITY;
    }

    fn turret(&mut self, at: Vec3, s: f32, dir: Vec2) {
        self.at(at, dir, |b| {
            b.tower(
                -s * 0.5,
                [0.0, 0.0, s * 0.62, s * 0.62],
                s * 0.2,
                [0.0, 0.0, s * 0.55, s * 0.55],
                Paint::Hull,
            );
            b.tower(
                s * 0.2,
                [0.0, -s * 0.05, s * 0.45, s * 0.5],
                s * 0.6,
                [0.0, -s * 0.12, s * 0.3, s * 0.36],
                Paint::Dark,
            );
            for x in [-s * 0.16, s * 0.16] {
                b.block(
                    Vec3::new(x - s * 0.06, s * 0.3, s * 0.2),
                    Vec3::new(x + s * 0.06, s * 0.42, s * 1.7),
                    Paint::Dark,
                );
            }
        });
    }

    fn launcher(&mut self, at: Vec3, s: f32, dir: Vec2) {
        self.at(at, dir, |b| {
            b.block(
                Vec3::new(-s * 0.6, -s * 0.5, -s * 0.8),
                Vec3::new(s * 0.6, s * 0.4, s * 0.8),
                Paint::Dark,
            );
            b.block(
                Vec3::new(-s * 0.45, s * 0.4, -s * 0.6),
                Vec3::new(s * 0.45, s * 0.46, s * 0.6),
                Paint::Accent,
            );
        });
    }

    /// Lit landing pad of a hangar.
    fn pad(&mut self, at: Vec3, s: f32) {
        self.at(at, Vec2::Y, |b| {
            b.block(
                Vec3::new(-s, -s * 0.3, -s * 1.6),
                Vec3::new(s, s * 0.05, s * 1.6),
                Paint::Dark,
            );
            for z in [-s * 1.3, 0.0, s * 1.3] {
                b.block(
                    Vec3::new(-s * 0.8, s * 0.05, z - s * 0.12),
                    Vec3::new(s * 0.8, s * 0.1, z + s * 0.12),
                    Paint::Glow,
                );
            }
        });
    }

    fn dome(&mut self, at: Vec3, s: f32) {
        self.at(at, Vec2::Y, |b| {
            b.loft(
                &[
                    ngon_y(-s * 0.5, s, 8),
                    ngon_y(s * 0.35, s * 0.9, 8),
                    ngon_y(s * 0.8, s * 0.45, 8),
                ],
                true,
                |i, _| if i == 0 { Paint::Hull } else { Paint::Accent },
            );
        });
    }

    /// Turrets, launchers, pads and the shield dome, where the definition
    /// has its hardpoints. Offsets off the hull get a small sponson.
    fn hardpoints(&mut self, def: &ShipDef) {
        let r = def.radius.to_f32();
        // Small ships get relatively bigger turrets, or nobody sees them.
        let class = (200.0 / r).powf(0.25);
        let spots: Vec<(Vec3, bool, &armada_sim::HardpointDef)> = def
            .hardpoints
            .iter()
            .map(|hp| {
                let (x, z) = (hp.offset.x.to_f32() / r, hp.offset.y.to_f32() / r);
                let top = self.top_at(x, z);
                (Vec3::new(x, top.unwrap_or(0.0), z), top.is_some(), hp)
            })
            .collect();
        for (at, on_hull, hp) in spots {
            match &hp.kind {
                HardpointKind::Weapon(_) if hp.name.starts_with("spinal") => {}
                HardpointKind::Weapon(_) if hp.name.starts_with("hangar") => self.pad(at, 0.05),
                HardpointKind::Weapon(weapon) => {
                    let s = (0.03 + 0.003 * (weapon.damage.max(0) as f32).sqrt()) * class;
                    let facing = Vec2::new(weapon.facing.x.to_f32(), weapon.facing.y.to_f32());
                    // All-round weapons point away from the middle.
                    let outward = Vec2::new(at.x, at.z);
                    let dir = if weapon.arc_cos.to_f32() < -0.99 && outward.length() > 0.05 {
                        outward.normalize()
                    } else {
                        facing
                    };
                    if !on_hull {
                        self.block(
                            at - Vec3::new(s, s * 0.7, s * 1.5),
                            at + Vec3::new(s, 0.0, s * 1.5),
                            Paint::Hull,
                        );
                    }
                    match weapon.delivery {
                        Delivery::Beam => self.turret(at, s, dir),
                        Delivery::Projectile { .. } => self.launcher(at, s * 0.55, dir),
                    }
                }
                HardpointKind::ShieldGenerator => self.dome(at, 0.05 * class),
                HardpointKind::Engine => {}
            }
        }
    }
}

/// Hull mesh with baked colors and a second mesh for everything that glows.
pub fn build(def: &ShipDef, accent: Vec3, scale: f32) -> (Mesh, Mesh) {
    let mut b = Builder::new(accent);
    match def.name.as_str() {
        "corvette" => corvette(&mut b),
        "frigate" => frigate(&mut b),
        "cruiser" => cruiser(&mut b),
        "carrier" => carrier(&mut b),
        "destroyer" => destroyer(&mut b),
        "dreadnought" => dreadnought(&mut b),
        _ if def.is_mobile() => generic(&mut b),
        _ => station(&mut b),
    }
    b.hardpoints(def);
    let size = def.radius.to_f32() * scale;
    (b.hull.mesh(size), b.glow.mesh(size))
}

/// Dart with a gun wing up front and two engine pods.
fn corvette(b: &mut Builder) {
    b.hull(
        &[
            sec(1.1, 0.03, 0.02, -0.02),
            sec(0.45, 0.2, 0.12, -0.08),
            sec(-0.35, 0.3, 0.16, -0.1),
            sec(-0.85, 0.24, 0.14, -0.08),
        ],
        1..3,
    );
    b.loft(
        &[
            rect_z(0.7, 0.0, 0.0, 0.42, 0.012),
            rect_z(0.38, 0.0, 0.0, 0.5, 0.03),
        ],
        true,
        |_, _| Paint::Hull,
    );
    b.tower(
        0.1,
        [0.0, 0.05, 0.09, 0.22],
        0.21,
        [0.0, -0.02, 0.05, 0.12],
        Paint::Dark,
    );
    b.block(
        Vec3::new(-0.015, 0.1, -0.82),
        Vec3::new(0.015, 0.36, -0.5),
        Paint::Accent,
    );
    for x in [-0.36, 0.36] {
        b.loft(
            &[
                ngon_z(-0.15, x, 0.0, 0.05, 8),
                ngon_z(-0.45, x, 0.0, 0.1, 8),
                ngon_z(-0.9, x, 0.0, 0.1, 8),
            ],
            true,
            |_, _| Paint::Hull,
        );
        b.engine(x, 0.0, -0.9, 0.085);
    }
}

/// Slim hull with flak pods on stub wings and a bridge aft.
fn frigate(b: &mut Builder) {
    b.hull(
        &[
            sec(1.1, 0.06, 0.04, -0.04),
            sec(0.7, 0.2, 0.12, -0.1),
            sec(0.1, 0.27, 0.16, -0.14),
            sec(-0.55, 0.3, 0.18, -0.16),
            sec(-0.95, 0.25, 0.16, -0.12),
        ],
        0..2,
    );
    for side in [-1.0, 1.0] {
        b.loft(
            &[
                rect_z(0.32, side * 0.5, 0.0, 0.08, 0.05),
                rect_z(-0.32, side * 0.5, 0.0, 0.11, 0.075),
            ],
            true,
            |_, k| if k == 0 { Paint::Accent } else { Paint::Hull },
        );
        b.block(
            Vec3::new(side * 0.33 - 0.12, -0.03, -0.16),
            Vec3::new(side * 0.33 + 0.12, 0.03, 0.16),
            Paint::Dark,
        );
    }
    b.tower(
        0.12,
        [0.0, -0.55, 0.14, 0.22],
        0.3,
        [0.0, -0.6, 0.085, 0.12],
        Paint::Hull,
    );
    b.block(
        Vec3::new(-0.07, 0.25, -0.485),
        Vec3::new(0.07, 0.27, -0.47),
        Paint::Glow,
    );
    b.block(
        Vec3::new(-0.012, 0.3, -0.66),
        Vec3::new(0.012, 0.46, -0.62),
        Paint::Dark,
    );
    for x in [-0.13, 0.13] {
        b.engine(x, 0.0, -0.95, 0.1);
    }
}

/// Two hull lobes with a waist between them: guns forward, engines aft.
fn cruiser(b: &mut Builder) {
    b.hull(
        &[
            sec(1.1, 0.1, 0.05, -0.06),
            sec(0.75, 0.4, 0.14, -0.13),
            sec(0.35, 0.52, 0.18, -0.17),
            sec(0.0, 0.38, 0.17, -0.16),
            sec(-0.45, 0.56, 0.2, -0.19),
            sec(-1.0, 0.48, 0.19, -0.15),
        ],
        0..2,
    );
    b.block(
        Vec3::new(-0.07, 0.12, -0.9),
        Vec3::new(0.07, 0.25, 0.25),
        Paint::Hull,
    );
    b.tower(
        0.15,
        [0.0, -0.62, 0.17, 0.24],
        0.4,
        [0.0, -0.68, 0.09, 0.1],
        Paint::Hull,
    );
    b.block(
        Vec3::new(-0.08, 0.33, -0.56),
        Vec3::new(0.08, 0.35, -0.545),
        Paint::Glow,
    );
    for side in [-1.0, 1.0] {
        // Ventral fins give it a different outline from above and below.
        b.tower(
            -0.15,
            [side * 0.3, -0.6, 0.02, 0.2],
            -0.38,
            [side * 0.36, -0.7, 0.012, 0.1],
            Paint::Accent,
        );
    }
    b.engine(0.0, 0.0, -1.0, 0.13);
    for x in [-0.28, 0.28] {
        b.engine(x, 0.0, -1.0, 0.1);
    }
}

/// Long flat deck with the command island off to starboard, hangar slits
/// along both sides.
fn carrier(b: &mut Builder) {
    let deck = |z, w, top, bot| Sec {
        tw: 0.86,
        bw: 0.5,
        e: 0.04,
        ..sec(z, w, top, bot)
    };
    b.hull(
        &[
            deck(1.1, 0.08, 0.05, -0.05),
            deck(0.6, 0.3, 0.09, -0.13),
            deck(0.0, 0.44, 0.1, -0.16),
            deck(-0.7, 0.5, 0.1, -0.17),
            deck(-1.05, 0.42, 0.1, -0.14),
        ],
        1..4,
    );
    b.tower(
        0.06,
        [0.29, -0.72, 0.055, 0.2],
        0.27,
        [0.3, -0.76, 0.04, 0.13],
        Paint::Hull,
    );
    b.tower(
        0.27,
        [0.3, -0.78, 0.035, 0.07],
        0.34,
        [0.3, -0.8, 0.025, 0.04],
        Paint::Dark,
    );
    b.block(
        Vec3::new(0.262, 0.2, -0.86),
        Vec3::new(0.268, 0.22, -0.66),
        Paint::Glow,
    );
    b.block(
        Vec3::new(0.294, 0.34, -0.81),
        Vec3::new(0.306, 0.5, -0.79),
        Paint::Dark,
    );
    // Ventral hangar block.
    b.tower(
        -0.14,
        [0.0, -0.3, 0.2, 0.45],
        -0.24,
        [0.0, -0.3, 0.14, 0.36],
        Paint::Dark,
    );
    for x in [-0.3, -0.11, 0.11, 0.3] {
        b.engine(x, -0.02, -1.05, 0.08);
    }
}

/// Wedge with a split bow. The slot between the two prongs holds the lance.
fn destroyer(b: &mut Builder) {
    b.hull(
        &[
            sec(0.3, 0.36, 0.12, -0.1),
            sec(-0.4, 0.55, 0.18, -0.15),
            sec(-1.05, 0.66, 0.2, -0.16),
        ],
        0..0,
    );
    for side in [-1.0, 1.0] {
        let prong = |z, x: f32, w, top, bot| Sec {
            x: side * x,
            ..sec(z, w, top, bot)
        };
        b.hull(
            &[
                prong(1.1, 0.09, 0.03, 0.03, -0.03),
                prong(0.7, 0.15, 0.09, 0.08, -0.07),
                prong(0.3, 0.21, 0.15, 0.12, -0.1),
            ],
            0..2,
        );
    }
    b.block(
        Vec3::new(-0.04, -0.035, 0.2),
        Vec3::new(0.04, 0.035, 0.9),
        Paint::Dark,
    );
    b.block(
        Vec3::new(-0.022, -0.02, 0.9),
        Vec3::new(0.022, 0.02, 0.93),
        Paint::Glow,
    );
    // Low citadel in two steps instead of a tower.
    b.tower(
        0.1,
        [0.0, -0.5, 0.2, 0.36],
        0.25,
        [0.0, -0.55, 0.15, 0.28],
        Paint::Hull,
    );
    b.tower(
        0.25,
        [0.0, -0.6, 0.11, 0.18],
        0.32,
        [0.0, -0.64, 0.08, 0.12],
        Paint::Hull,
    );
    b.block(
        Vec3::new(-0.1, 0.275, -0.425),
        Vec3::new(0.1, 0.29, -0.41),
        Paint::Glow,
    );
    for side in [-1.0, 1.0] {
        // Twin fins, leaning outward.
        b.tower(
            0.12,
            [side * 0.34, -0.82, 0.022, 0.17],
            0.46,
            [side * 0.43, -0.93, 0.012, 0.08],
            Paint::Accent,
        );
    }
    for (x, r) in [
        (-0.46, 0.075),
        (-0.24, 0.1),
        (0.0, 0.12),
        (0.24, 0.1),
        (0.46, 0.075),
    ] {
        b.engine(x, 0.0, -1.05, r);
    }
}

/// Blunt slab with a stepped citadel, side galleries and a keel.
fn dreadnought(b: &mut Builder) {
    b.hull(
        &[
            sec(1.1, 0.24, 0.1, -0.1),
            sec(0.8, 0.36, 0.16, -0.15),
            sec(0.3, 0.58, 0.2, -0.2),
            sec(-0.4, 0.62, 0.22, -0.22),
            sec(-1.05, 0.52, 0.2, -0.18),
        ],
        0..2,
    );
    b.tower(
        0.12,
        [0.0, -0.15, 0.24, 0.5],
        0.3,
        [0.0, -0.2, 0.2, 0.42],
        Paint::Hull,
    );
    b.tower(
        0.3,
        [0.0, -0.3, 0.15, 0.28],
        0.42,
        [0.0, -0.33, 0.12, 0.22],
        Paint::Hull,
    );
    b.tower(
        0.42,
        [0.0, -0.4, 0.08, 0.12],
        0.55,
        [0.0, -0.42, 0.05, 0.07],
        Paint::Dark,
    );
    b.block(
        Vec3::new(-0.13, 0.35, -0.095),
        Vec3::new(0.13, 0.37, -0.08),
        Paint::Glow,
    );
    b.block(
        Vec3::new(-0.05, -0.4, -0.7),
        Vec3::new(0.05, -0.15, 0.3),
        Paint::Hull,
    );
    for side in [-1.0, 1.0] {
        b.loft(
            &[
                rect_z(0.25, side * 0.6, 0.0, 0.05, 0.05),
                rect_z(0.1, side * 0.63, 0.0, 0.07, 0.07),
                rect_z(-0.6, side * 0.65, 0.0, 0.07, 0.07),
                rect_z(-0.75, side * 0.6, 0.0, 0.05, 0.05),
            ],
            true,
            |i, k| {
                if i == 1 && k == 0 {
                    Paint::Accent
                } else {
                    Paint::Hull
                }
            },
        );
    }
    for (x, r) in [(-0.38, 0.09), (-0.14, 0.13), (0.14, 0.13), (0.38, 0.09)] {
        b.engine(x, 0.0, -1.05, r);
    }
}

/// Fallback for ship types without a model of their own.
fn generic(b: &mut Builder) {
    b.hull(
        &[sec(1.1, 0.06, 0.03, -0.03), sec(-1.0, 0.45, 0.16, -0.13)],
        0..1,
    );
    b.engine(0.0, 0.0, -1.0, 0.12);
}

/// Hub with a spire, eight arms with weapon platforms and a ring.
fn station(b: &mut Builder) {
    b.loft(
        &[
            ngon_y(-0.14, 0.28, 8),
            ngon_y(-0.06, 0.42, 8),
            ngon_y(0.06, 0.42, 8),
            ngon_y(0.14, 0.28, 8),
        ],
        true,
        |i, _| if i == 1 { Paint::Dark } else { Paint::Hull },
    );
    b.loft(
        &[
            ngon_y(0.14, 0.24, 8),
            ngon_y(0.3, 0.15, 8),
            ngon_y(0.36, 0.1, 8),
        ],
        true,
        |i, _| if i == 0 { Paint::Accent } else { Paint::Hull },
    );
    b.loft(
        &[
            ngon_y(-0.14, 0.22, 8),
            ngon_y(-0.42, 0.1, 8),
            ngon_y(-0.75, 0.03, 8),
        ],
        true,
        |_, _| Paint::Hull,
    );
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU;
        let dir = Vec2::new(a.sin(), a.cos());
        // Straight arms carry the batteries, diagonal ones the rest.
        let (reach, size) = if k % 2 == 0 {
            (0.727, 0.13)
        } else {
            (0.707, 0.1)
        };
        b.at(Vec3::ZERO, dir, |b| {
            b.block(
                Vec3::new(-0.04, -0.035, 0.38),
                Vec3::new(0.04, 0.035, reach),
                Paint::Hull,
            );
            b.block(
                Vec3::new(-0.045, -0.01, 0.42),
                Vec3::new(0.045, 0.01, 0.44),
                Paint::Glow,
            );
        });
        b.at(Vec3::new(dir.x, 0.0, dir.y) * reach, dir, |b| {
            b.loft(
                &[
                    ngon_y(-0.08, size * 0.7, 8),
                    ngon_y(-0.03, size, 8),
                    ngon_y(0.04, size, 8),
                    ngon_y(0.06, size * 0.85, 8),
                ],
                true,
                |i, _| if i == 1 { Paint::Dark } else { Paint::Hull },
            );
        });
        // Ring segment to the next platform.
        let mid = a + TAU / 16.0;
        b.at(Vec3::ZERO, Vec2::new(mid.sin(), mid.cos()), |b| {
            b.block(
                Vec3::new(-0.27, -0.02, 0.645),
                Vec3::new(0.27, 0.02, 0.685),
                Paint::Dark,
            );
            b.block(
                Vec3::new(-0.05, -0.008, 0.685),
                Vec3::new(0.05, 0.008, 0.692),
                Paint::Glow,
            );
        });
    }
}
