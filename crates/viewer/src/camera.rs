use bevy::{
    camera::Hdr, input::mouse::AccumulatedMouseScroll, post_process::bloom::Bloom, prelude::*,
};

use crate::{
    Screen,
    render::{SCALE, to_render},
    sim::Sim,
};

const PITCH_DEG: f32 = 55.0;

#[derive(Component)]
pub struct Rig {
    pub focus: Vec3,
    pub distance: f32,
    pub yaw: f32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Screen::Game), spawn)
        .add_systems(Update, control.run_if(in_state(Screen::Game)));
}

pub fn spawn(mut commands: Commands, sim: Res<Sim>) {
    // Start over the player's station if there is one.
    let focus = sim
        .player_producer()
        .map_or(Vec3::new(0.0, 0.0, -450.0), |i| {
            to_render(sim.world.ships.pos[i as usize]) + Vec3::Z * 120.0
        });
    commands.spawn((
        DespawnOnExit(Screen::Game),
        Camera3d::default(),
        Hdr,
        Bloom::NATURAL,
        Projection::Perspective(PerspectiveProjection {
            far: 30_000.0,
            ..default()
        }),
        Transform::default(),
        Rig {
            focus,
            distance: 1600.0,
            // Behind team 0, which starts on negative y.
            yaw: std::f32::consts::PI,
        },
    ));
}

fn control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    sim: Res<Sim>,
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
    rig.distance = (rig.distance * (1.0 - scroll.delta.y * 0.1)).clamp(40.0, 6000.0);

    let pitch = PITCH_DEG.to_radians();
    let offset =
        Quat::from_rotation_y(rig.yaw) * Vec3::new(0.0, pitch.sin(), pitch.cos()) * rig.distance;
    let forward = Vec3::new(-offset.x, 0.0, -offset.z).normalize_or_zero();
    let right = forward.cross(Vec3::Y);

    let mut pan = Vec3::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        pan += forward;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        pan -= forward;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        pan += right;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        pan -= right;
    }
    let bound = sim.world.arena_half.to_f32() * SCALE;
    let focus = rig.focus + pan.normalize_or_zero() * rig.distance * 0.8 * dt;
    rig.focus = focus.clamp(Vec3::splat(-bound), Vec3::splat(bound));

    *tf = Transform::from_translation(rig.focus + offset).looking_at(rig.focus, Vec3::Y);
}
