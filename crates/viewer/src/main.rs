//! armada-viewer [scenario.ron | --replay file.replay.ron]
//!
//! Read-only view on the sim. The sim runs in FixedUpdate at TICK_HZ, rendering
//! interpolates between the last two ticks. Nothing here writes sim state
//! except through commands.
#![allow(clippy::float_arithmetic)]

use std::path::PathBuf;

use armada_data::{build, content_dir, load_replay, read_sources};
use armada_sim::{CommandLog, Event, TICK_HZ, World, step};
use bevy::{input::mouse::AccumulatedMouseScroll, prelude::*};

/// Render units per sim meter. Keeps the scene in a range where the default
/// camera and lighting behave.
const SCALE: f32 = 0.1;
const SPEEDS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
const TEAM_COLORS: [(f32, f32, f32); 2] = [(0.45, 0.7, 1.0), (1.0, 0.5, 0.35)];

#[derive(Resource)]
struct Sim {
    world: World,
    prev_pos: Vec<armada_sim::Vec2>,
    log: CommandLog,
    paused: bool,
    speed: usize,
}

#[derive(Component)]
struct ShipView(usize);

#[derive(Component)]
struct Rig {
    focus: Vec3,
    distance: f32,
    yaw: f32,
}

#[derive(Component)]
struct Hud;

fn main() {
    let (world, log) = match load() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let prev_pos = world.ships.pos.clone();

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Armada".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.015, 0.015, 0.025)))
        .insert_resource(Time::<Fixed>::from_hz(f64::from(TICK_HZ)))
        .insert_resource(Sim {
            world,
            prev_pos,
            log,
            paused: false,
            speed: 1,
        })
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, tick_sim)
        .add_systems(Update, (controls, camera, sync_ships, draw_shots, hud))
        .run();
}

fn load() -> Result<(World, CommandLog), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--replay") {
        let path = PathBuf::from(args.get(1).ok_or("--replay needs a path")?);
        let replay = load_replay(&path)?;
        let (world, _) = build(&replay.sources)?;
        return Ok((world, replay.commands));
    }
    let scenario = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| content_dir().join("scenarios/fleet_clash.ron"));
    let (world, _) = build(&read_sources(&scenario)?)?;
    Ok((world, CommandLog::default()))
}

fn to_render(v: armada_sim::Vec2) -> Vec3 {
    Vec3::new(v.x.to_f32(), 0.0, v.y.to_f32()) * SCALE
}

fn setup(
    mut commands: Commands,
    sim: Res<Sim>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let w = &sim.world;

    // One mesh per ship class, one material per team: everything batches.
    let class_meshes: Vec<Handle<Mesh>> = w
        .defs
        .ships
        .iter()
        .map(|d| {
            let r = d.radius.to_f32() * SCALE;
            meshes.add(Cuboid::new(r * 0.9, r * 0.35, r * 2.0))
        })
        .collect();
    let team_materials: Vec<Handle<StandardMaterial>> = TEAM_COLORS
        .iter()
        .map(|&(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                emissive: LinearRgba::rgb(r * 0.25, g * 0.25, b * 0.25),
                perceptual_roughness: 0.6,
                ..default()
            })
        })
        .collect();

    for i in 0..w.ships.len() {
        let team = usize::from(w.ships.team[i]) % team_materials.len();
        commands.spawn((
            ShipView(i),
            Mesh3d(class_meshes[usize::from(w.ships.def[i])].clone()),
            MeshMaterial3d(team_materials[team].clone()),
            Transform::from_translation(to_render(w.ships.pos[i])),
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            ..default()
        },
        Transform::from_xyz(300.0, 800.0, 200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 20_000.0,
            ..default()
        }),
        Transform::default(),
        Rig {
            focus: Vec3::ZERO,
            distance: 1400.0,
            // Behind team 0, which starts on negative y.
            yaw: std::f32::consts::PI,
        },
    ));

    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(15.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));
}

fn tick_sim(mut sim: ResMut<Sim>) {
    if sim.paused {
        return;
    }
    let Sim {
        world,
        prev_pos,
        log,
        ..
    } = &mut *sim;
    prev_pos.clone_from(&world.ships.pos);
    step(world, log.for_tick(world.tick));
}

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut fixed: ResMut<Time<Fixed>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    let mut speed = sim.speed;
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        speed = (speed + 1).min(SPEEDS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        speed = speed.saturating_sub(1);
    }
    if speed != sim.speed {
        sim.speed = speed;
        fixed.set_timestep_hz(f64::from(TICK_HZ) * SPEEDS[speed]);
    }
}

fn camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut q: Query<(&mut Rig, &mut Transform)>,
) {
    let Ok((mut rig, mut tf)) = q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    if keys.pressed(KeyCode::KeyQ) {
        rig.yaw += dt * 1.2;
    }
    if keys.pressed(KeyCode::KeyE) {
        rig.yaw -= dt * 1.2;
    }
    rig.distance = (rig.distance * (1.0 - scroll.delta.y * 0.1)).clamp(50.0, 6000.0);

    let pitch = 55f32.to_radians();
    let offset = Quat::from_rotation_y(rig.yaw)
        * Vec3::new(0.0, pitch.sin(), pitch.cos())
        * rig.distance;
    let forward = Vec3::new(-offset.x, 0.0, -offset.z).normalize_or_zero();
    let right = forward.cross(Vec3::Y);

    let mut pan = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        pan += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        pan -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        pan += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        pan -= right;
    }
    let pan_speed = rig.distance * 0.8;
    rig.focus += pan.normalize_or_zero() * pan_speed * dt;

    *tf = Transform::from_translation(rig.focus + offset).looking_at(rig.focus, Vec3::Y);
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
        tf.translation = to_render(sim.prev_pos[i]).lerp(to_render(s.pos[i]), alpha);
        let h = s.heading[i];
        tf.rotation = Quat::from_rotation_y(h.x.to_f32().atan2(h.y.to_f32()));
    }
}

fn draw_shots(sim: Res<Sim>, mut gizmos: Gizmos) {
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
        let hp = hardpoint as usize;
        let owner = w.hardpoints.owner[hp] as usize;
        let def = &w.defs.ships[usize::from(w.ships.def[owner])];
        let offset = def.hardpoints[usize::from(w.hardpoints.slot[hp])].offset;
        let from = to_render(w.ships.pos[owner] + offset.from_local(w.ships.heading[owner]));
        let mut to = to_render(w.ships.pos[target as usize]);
        if !hit {
            // Misses fly past the target.
            to = from + (to - from) * 1.15 + Vec3::new(3.0, 0.0, -3.0);
        }
        let (r, g, b) = TEAM_COLORS[usize::from(w.ships.team[owner]) % TEAM_COLORS.len()];
        let a = if hit { 0.9 } else { 0.35 };
        gizmos.line(from, to, Color::srgba(r, g, b, a));
    }
}

fn hud(sim: Res<Sim>, mut q: Query<&mut Text, With<Hud>>) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let w = &sim.world;
    let secs = w.tick / TICK_HZ;
    let state = if sim.paused { "  Pausiert" } else { "" };
    text.0 = format!(
        "{:02}:{:02}  Tick {}  {}x{state}\nBlau {}   Rot {}\n\nWASD bewegen  Q/E drehen  Mausrad zoomen\nLeertaste Pause  +/- Tempo",
        secs / 60,
        secs % 60,
        w.tick,
        SPEEDS[sim.speed],
        w.alive_count(0),
        w.alive_count(1),
    );
}
