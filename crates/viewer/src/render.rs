use std::f32::consts::FRAC_PI_2;

use armada_sim::{Delivery, Event, HardpointKind, NO_TEAM, NONE, Order, World};
use bevy::prelude::*;

use crate::{
    Screen,
    input::{MarkerKind, Markers, Selection},
    models,
    sim::{FrameEvents, Sim},
};

/// Render units per sim meter.
pub const SCALE: f32 = 0.1;
pub const TEAM_COLORS: [(f32, f32, f32); 2] = [(0.45, 0.7, 1.0), (1.0, 0.5, 0.35)];
const STARS: usize = 1800;
const STAR_SHELL: f32 = 7000.0;
const MARKER_LIFE: f32 = 0.6;
/// Above this many selected ships, hardpoint dots are skipped.
const HARDPOINT_DOTS_MAX: usize = 12;
/// Above this many selected ships, order lines turn into clutter.
const ORDER_LINES_MAX: usize = 60;

pub fn to_render(v: armada_sim::Vec2) -> Vec3 {
    Vec3::new(v.x.to_f32(), 0.0, v.y.to_f32()) * SCALE
}

pub fn team_color(team: u8, intensity: f32, alpha: f32) -> Color {
    let (r, g, b) = TEAM_COLORS[usize::from(team) % TEAM_COLORS.len()];
    Color::LinearRgba(LinearRgba::new(
        r * intensity,
        g * intensity,
        b * intensity,
        alpha,
    ))
}

#[derive(Component)]
struct ShipView(usize);

#[derive(Component)]
struct Explosion {
    age: f32,
    life: f32,
    size: f32,
}

/// Hull meshes per class and team with the colors baked in, so one material
/// covers every hull and everything batches. Engines and lights are a second
/// mesh per class with one glowing material per team.
#[derive(Resource)]
struct ShipAssets {
    hulls: Vec<Vec<Handle<Mesh>>>,
    glows: Vec<Handle<Mesh>>,
    hull_material: Handle<StandardMaterial>,
    glow_materials: Vec<Handle<StandardMaterial>>,
    /// Ships with a view entity. Ids only grow, so this is a high-water mark.
    spawned: usize,
}

#[derive(Resource)]
struct FxAssets {
    sphere: Handle<Mesh>,
    blast: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
}

pub fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Screen::Game), setup).add_systems(
        Update,
        (
            spawn_views,
            sync_ships,
            spawn_explosions,
            animate_explosions,
            draw_beams,
            draw_projectiles,
            draw_selection,
            draw_markers,
            draw_points,
        )
            .run_if(in_state(Screen::Game)),
    );
}

fn setup(
    mut commands: Commands,
    sim: Res<Sim>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let w = &sim.world;

    let mut hulls = Vec::new();
    let mut glows = Vec::new();
    for def in &w.defs.ships {
        let mut per_team = Vec::new();
        for &(r, g, b) in &TEAM_COLORS {
            let accent = Color::srgb(r, g, b).to_linear();
            let accent = Vec3::new(accent.red, accent.green, accent.blue);
            let (hull, glow) = models::build(def, accent, SCALE);
            per_team.push(meshes.add(hull));
            if per_team.len() == 1 {
                glows.push(meshes.add(glow));
            }
        }
        hulls.push(per_team);
    }
    let glow_materials = TEAM_COLORS
        .iter()
        .map(|&(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                emissive: Color::srgb(r, g, b).to_linear() * 5.0,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(ShipAssets {
        hulls,
        glows,
        hull_material: materials.add(StandardMaterial {
            perceptual_roughness: 0.6,
            metallic: 0.2,
            ..default()
        }),
        glow_materials,
        spawned: 0,
    });

    // Key light from above, a dim blue one from below so undersides and
    // shadow sides keep their shape.
    commands.spawn((
        DespawnOnExit(Screen::Game),
        DirectionalLight {
            illuminance: 9_000.0,
            ..default()
        },
        Transform::from_xyz(300.0, 800.0, 200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DespawnOnExit(Screen::Game),
        DirectionalLight {
            illuminance: 1_500.0,
            color: Color::srgb(0.6, 0.75, 1.0),
            ..default()
        },
        Transform::from_xyz(-400.0, -300.0, -500.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.82, 1.0),
        brightness: 110.0,
        ..default()
    });

    let sphere = meshes.add(Sphere::new(1.0).mesh().ico(2).expect("valid ico sphere"));
    let star = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(1.5, 1.5, 1.7),
        unlit: true,
        ..default()
    });
    // Fixed seed: the sky looks the same every start.
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    for _ in 0..STARS {
        let dir = Vec3::new(rand() - 0.5, rand() - 0.5, rand() - 0.5).normalize_or(Vec3::NEG_Y);
        let size = 3.0 + rand() * rand() * 12.0;
        commands.spawn((
            DespawnOnExit(Screen::Game),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(star.clone()),
            Transform::from_translation(dir * STAR_SHELL).with_scale(Vec3::splat(size)),
        ));
    }

    let blast = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.7, 0.3),
        emissive: LinearRgba::rgb(9.0, 4.0, 1.2),
        unlit: true,
        ..default()
    });
    let spark = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.9, 0.7),
        emissive: LinearRgba::rgb(6.0, 5.0, 3.0),
        unlit: true,
        ..default()
    });
    commands.insert_resource(FxAssets {
        sphere,
        blast,
        spark,
    });
}

/// View entities for ships that exist in the sim but not yet on screen:
/// everything at start, then whatever stations build.
fn spawn_views(mut commands: Commands, sim: Res<Sim>, assets: Option<ResMut<ShipAssets>>) {
    let Some(mut assets) = assets else {
        return;
    };
    let s = &sim.world.ships;
    for i in assets.spawned..s.len() {
        let team = usize::from(s.team[i]) % TEAM_COLORS.len();
        let def = usize::from(s.def[i]);
        commands.spawn((
            DespawnOnExit(Screen::Game),
            ShipView(i),
            Mesh3d(assets.hulls[def][team].clone()),
            MeshMaterial3d(assets.hull_material.clone()),
            Transform::from_translation(to_render(s.pos[i])),
            children![(
                Mesh3d(assets.glows[def].clone()),
                MeshMaterial3d(assets.glow_materials[team].clone()),
            )],
        ));
    }
    assets.spawned = s.len();
}

fn sync_ships(
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    mut q: Query<(&ShipView, &mut Transform, &mut Visibility)>,
) {
    let alpha = if sim.paused {
        1.0
    } else {
        fixed.overstep_fraction()
    };
    let s = &sim.world.ships;
    for (view, mut tf, mut vis) in &mut q {
        let i = view.0;
        if !s.alive[i] {
            *vis = Visibility::Hidden;
            continue;
        }
        // Ships built this tick have no previous position yet.
        let prev = sim.prev_pos.get(i).copied().unwrap_or(s.pos[i]);
        tf.translation = to_render(prev).lerp(to_render(s.pos[i]), alpha);
        let h = s.heading[i];
        tf.rotation = Quat::from_rotation_y(h.x.to_f32().atan2(h.y.to_f32()));
    }
}

fn spawn_explosions(
    mut commands: Commands,
    sim: Res<Sim>,
    mut events: ResMut<FrameEvents>,
    fx: Option<Res<FxAssets>>,
) {
    let Some(fx) = fx else {
        return;
    };
    let w = &sim.world;
    for ev in events.0.drain(..) {
        let (pos, size, life, material) = match ev {
            Event::ShipDestroyed(i) => {
                let r = w.defs.ships[usize::from(w.ships.def[i as usize])]
                    .radius
                    .to_f32();
                (
                    to_render(w.ships.pos[i as usize]),
                    r * SCALE * 2.2,
                    1.4,
                    &fx.blast,
                )
            }
            Event::HardpointDestroyed(h) => {
                (to_render(w.hardpoint_pos(h as usize)), 3.0, 0.6, &fx.blast)
            }
            Event::Impact { pos, .. } => (to_render(pos), 2.5, 0.35, &fx.spark),
            _ => continue,
        };
        commands.spawn((
            DespawnOnExit(Screen::Game),
            Explosion {
                age: 0.0,
                life,
                size,
            },
            Mesh3d(fx.sphere.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(pos + Vec3::Y).with_scale(Vec3::splat(0.01)),
        ));
    }
}

fn animate_explosions(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut q: Query<(Entity, &mut Explosion, &mut Transform)>,
) {
    if sim.paused {
        return;
    }
    let dt = time.delta_secs();
    for (e, mut ex, mut tf) in &mut q {
        ex.age += dt;
        let t = ex.age / ex.life;
        if t >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        // Fast swell, slow collapse.
        let scale = if t < 0.25 {
            t / 0.25
        } else {
            1.0 - (t - 0.25) / 0.75
        };
        tf.scale = Vec3::splat((ex.size * scale).max(0.01));
    }
}

fn draw_beams(sim: Res<Sim>, mut gizmos: Gizmos) {
    let w = &sim.world;
    for ev in &w.events {
        let Event::Shot {
            hardpoint,
            target,
            hit,
        } = *ev
        else {
            continue;
        };
        let owner = w.hardpoints.owner[hardpoint as usize] as usize;
        let from = to_render(w.hardpoint_pos(hardpoint as usize));
        let mut to = to_render(w.ships.pos[target as usize]);
        if !hit {
            // Misses fly past the target.
            to = from + (to - from) * 1.15 + Vec3::new(3.0, 0.0, -3.0);
        }
        let color = team_color(
            w.ships.team[owner],
            if hit { 3.0 } else { 1.2 },
            if hit { 0.9 } else { 0.4 },
        );
        // Leave from deck height, not from inside the hull.
        let deck = |ship: usize| {
            let r = w.defs.ships[usize::from(w.ships.def[ship])].radius.to_f32();
            Vec3::Y * (r * SCALE * 0.22).max(0.5)
        };
        gizmos.line(from + deck(owner), to + deck(target as usize) * 0.5, color);
    }
}

fn draw_projectiles(sim: Res<Sim>, fixed: Res<Time<Fixed>>, mut gizmos: Gizmos) {
    let p = &sim.world.projectiles;
    let alpha = if sim.paused {
        1.0
    } else {
        fixed.overstep_fraction()
    };
    for i in 0..p.len() {
        let vel = Vec3::new(p.vel[i].x.to_f32(), 0.0, p.vel[i].y.to_f32()) * SCALE;
        let head = to_render(p.pos[i]) - vel * (1.0 - alpha) + Vec3::Y;
        // Trail of two ticks of travel, at least a few units so it stays visible.
        let tail = head - vel.normalize_or_zero() * (vel.length() * 2.0).max(4.0);
        let core = team_color(p.team[i], 6.0, 1.0);
        gizmos.line(tail, head, core);
    }
}

fn order_line(w: &World, i: usize) -> Option<(Vec3, Color)> {
    match w.ships.order[i] {
        Order::MoveTo { to, .. } => Some((to_render(to), Color::srgba(0.8, 0.9, 1.0, 0.25))),
        Order::AttackMove { to, .. } => Some((to_render(to), Color::srgba(1.0, 0.75, 0.3, 0.3))),
        Order::Attack(t) if w.ships.alive[t as usize] => Some((
            to_render(w.ships.pos[t as usize]),
            Color::srgba(1.0, 0.35, 0.3, 0.45),
        )),
        _ => None,
    }
}

fn draw_selection(
    sim: Res<Sim>,
    sel: Res<Selection>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    let Ok(cam) = cams.single() else {
        return;
    };
    let right = *cam.right();
    let w = &sim.world;
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    let dots = sel.ships.len() <= HARDPOINT_DOTS_MAX;
    let order_lines = sel.ships.len() <= ORDER_LINES_MAX;

    for &id in &sel.ships {
        let i = id as usize;
        let def = &w.defs.ships[usize::from(w.ships.def[i])];
        let pos = to_render(w.ships.pos[i]);
        let r = def.radius.to_f32() * SCALE;
        let ring = if w.ships.team[i] == crate::sim::PLAYER_TEAM {
            Color::srgba(0.85, 1.0, 0.85, 0.8)
        } else {
            Color::srgba(1.0, 0.45, 0.4, 0.8)
        };
        gizmos.circle(Isometry3d::new(pos, flat), r * 1.5 + 1.5, ring);

        // Hull and shield bars, facing the camera.
        let len = (r * 2.4).max(8.0);
        let base = pos + Vec3::Y * (r + 4.0) - right * len / 2.0;
        let hull = (w.ships.hull[i] as f32 / def.hull as f32).clamp(0.0, 1.0);
        let shield = if def.shield > 0 {
            (w.ships.shield[i] as f32 / def.shield as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let up = Vec3::Y * 1.6;
        gizmos.line(base, base + right * len, Color::srgba(0.2, 0.2, 0.2, 0.8));
        gizmos.line(
            base,
            base + right * len * hull,
            Color::srgb(0.45, 0.95, 0.5),
        );
        gizmos.line(
            base + up,
            base + up + right * len,
            Color::srgba(0.15, 0.15, 0.25, 0.8),
        );
        gizmos.line(
            base + up,
            base + up + right * len * shield,
            Color::srgb(0.45, 0.75, 1.0),
        );

        if order_lines && let Some((to, color)) = order_line(w, i) {
            gizmos.line(pos, to, color);
        }
        if !def.builds.is_empty() {
            let rally = to_render(w.ships.rally[i]);
            let color = Color::srgba(0.85, 1.0, 0.85, 0.35);
            gizmos.line(pos, rally, color);
            gizmos.circle(Isometry3d::new(rally + Vec3::Y, flat), 6.0, color);
        }

        if dots {
            let first = w.ships.hp_first[i] as usize;
            for (k, hp_def) in def.hardpoints.iter().enumerate() {
                let h = first + k;
                let frac = w.hardpoints.health[h] as f32 / hp_def.health as f32;
                let color = if frac <= 0.0 {
                    Color::srgb(0.9, 0.15, 0.1)
                } else if frac < 0.5 {
                    Color::srgb(1.0, 0.75, 0.2)
                } else {
                    Color::srgb(0.9, 0.95, 1.0)
                };
                let size = match hp_def.kind {
                    HardpointKind::Weapon(ref wd)
                        if matches!(wd.delivery, Delivery::Projectile { .. }) =>
                    {
                        0.9
                    }
                    HardpointKind::Weapon(_) => 0.6,
                    _ => 1.1,
                };
                gizmos.sphere(
                    Isometry3d::from_translation(to_render(w.hardpoint_pos(h)) + Vec3::Y * 2.0),
                    size,
                    color,
                );
            }
        }

        // Show what an inspected enemy is shooting at.
        if w.ships.team[i] != crate::sim::PLAYER_TEAM {
            let t = w.ships.target[i];
            if t != NONE && w.ships.alive[t as usize] {
                gizmos.line(
                    pos,
                    to_render(w.ships.pos[t as usize]),
                    Color::srgba(1.0, 0.4, 0.3, 0.3),
                );
            }
        }
    }
}

fn draw_markers(time: Res<Time>, mut markers: ResMut<Markers>, mut gizmos: Gizmos) {
    let dt = time.delta_secs();
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    markers.0.retain_mut(|m| {
        m.age += dt;
        m.age < MARKER_LIFE
    });
    for m in &markers.0 {
        let t = m.age / MARKER_LIFE;
        let color = match m.kind {
            MarkerKind::Move => Color::srgba(0.85, 1.0, 0.85, 1.0 - t),
            MarkerKind::AttackMove => Color::srgba(1.0, 0.75, 0.3, 1.0 - t),
            MarkerKind::Attack => Color::srgba(1.0, 0.35, 0.3, 1.0 - t),
        };
        gizmos.circle(
            Isometry3d::new(m.pos + Vec3::Y, flat),
            14.0 * (1.0 - t) + 3.0,
            color,
        );
    }
}

/// Capture radius in the owner's color, inner ring grows with progress.
fn draw_points(sim: Res<Sim>, mut gizmos: Gizmos) {
    let w = &sim.world;
    let p = &w.points;
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    let r = w.rules.capture_radius.to_f32() * SCALE;
    let full = f32::from(w.rules.capture_secs.max(1));
    let color = |team: u8, intensity: f32, alpha: f32| {
        if team == NO_TEAM {
            Color::srgba(0.6, 0.6, 0.65, alpha)
        } else {
            team_color(team, intensity, alpha)
        }
    };
    for k in 0..p.len() {
        let at = Isometry3d::new(to_render(p.pos[k]) + Vec3::Y * 0.5, flat);
        gizmos.circle(at, r, color(p.owner[k], 1.5, 0.6));
        gizmos.circle(at, r * 0.12, color(p.owner[k], 2.5, 0.9));
        if p.holder[k] != NO_TEAM && p.owner[k] != p.holder[k] {
            let frac = f32::from(p.progress[k]) / full;
            gizmos.circle(at, (r * frac).max(1.0), color(p.holder[k], 2.0, 0.7));
        }
    }
}
