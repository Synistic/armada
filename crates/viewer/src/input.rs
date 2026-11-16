use armada_sim::Command;
use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    camera::Rig,
    hud::Toast,
    render::{SCALE, to_render},
    sim::{Mode, PLAYER_TEAM, Sim},
};

/// Below this many pixels a left drag counts as a click.
const CLICK_PX: f32 = 5.0;
const GROUP_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

#[derive(Resource, Default)]
pub struct Selection {
    /// Sorted ship ids. Own ships, or a single enemy ship for inspection.
    pub ships: Vec<u32>,
    groups: [Vec<u32>; 9],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    Move,
    AttackMove,
    Attack,
}

/// Short-lived feedback where an order went.
pub struct Marker {
    pub pos: Vec3,
    pub age: f32,
    pub kind: MarkerKind,
}

#[derive(Resource, Default)]
pub struct Markers(pub Vec<Marker>);

#[derive(Resource, Default)]
struct Drag {
    start: Option<Vec2>,
}

#[derive(Component)]
struct SelectionBox;

pub fn plugin(app: &mut App) {
    app.init_resource::<Selection>()
        .init_resource::<Markers>()
        .init_resource::<Drag>()
        .add_systems(Startup, spawn_box)
        .add_systems(
            Update,
            (prune_dead, mouse_select, mouse_orders, hotkeys).chain(),
        );
}

fn spawn_box(mut commands: Commands) {
    commands.spawn((
        SelectionBox,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::srgba(0.85, 0.9, 1.0, 0.8)),
        BackgroundColor(Color::srgba(0.6, 0.75, 1.0, 0.08)),
        Visibility::Hidden,
    ));
}

fn ctrl(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
}

fn shift(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
}

/// Cursor ray hit on the battle plane.
fn ground(window: &Window, camera: &Camera, cam_tf: &GlobalTransform) -> Option<Vec3> {
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let d = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    Some(ray.get_point(d))
}

/// Closest alive ship under `p`. The pick radius grows with zoom so small
/// ships stay clickable from far out.
fn ship_at(sim: &Sim, p: Vec3, cam_distance: f32, team_ok: impl Fn(u8) -> bool) -> Option<u32> {
    let w = &sim.world;
    let mut best = None;
    let mut best_d = f32::MAX;
    for i in 0..w.ships.len() {
        if !w.ships.alive[i] || !team_ok(w.ships.team[i]) {
            continue;
        }
        let r = w.defs.ships[usize::from(w.ships.def[i])].radius.to_f32() * SCALE;
        let pick = (r * 1.3).max(cam_distance * 0.012);
        let d = to_render(w.ships.pos[i]).distance(p);
        if d < pick && d < best_d {
            best_d = d;
            best = Some(i as u32);
        }
    }
    best
}

fn prune_dead(sim: Res<Sim>, mut sel: ResMut<Selection>) {
    let alive = &sim.world.ships.alive;
    sel.ships.retain(|&i| alive[i as usize]);
}

#[allow(clippy::too_many_arguments)]
fn mouse_select(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform, &Rig)>,
    sim: Res<Sim>,
    mut drag: ResMut<Drag>,
    mut sel: ResMut<Selection>,
    mut boxes: Query<(&mut Node, &mut Visibility), With<SelectionBox>>,
) {
    let (Ok(window), Ok((camera, cam_tf, rig)), Ok((mut node, mut vis))) =
        (windows.single(), cams.single(), boxes.single_mut())
    else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    if buttons.just_pressed(MouseButton::Left) {
        drag.start = Some(cursor);
    }
    if let Some(start) = drag.start {
        let (min, max) = (start.min(cursor), start.max(cursor));
        node.left = Val::Px(min.x);
        node.top = Val::Px(min.y);
        node.width = Val::Px(max.x - min.x);
        node.height = Val::Px(max.y - min.y);
        *vis = if start.distance(cursor) > CLICK_PX {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    let Some(start) = drag.start.take() else {
        return;
    };
    *vis = Visibility::Hidden;

    let w = &sim.world;
    let mut picked = Vec::new();
    if start.distance(cursor) <= CLICK_PX {
        if let Some(p) = ground(window, camera, cam_tf) {
            // Own ships first; an enemy can be selected alone to inspect it.
            let own = ship_at(&sim, p, rig.distance, |t| t == PLAYER_TEAM);
            let any = || ship_at(&sim, p, rig.distance, |_| true);
            if let Some(id) = own.or_else(any) {
                picked.push(id);
            }
        }
    } else {
        let (min, max) = (start.min(cursor), start.max(cursor));
        for i in 0..w.ships.len() {
            if !w.ships.alive[i] || w.ships.team[i] != PLAYER_TEAM {
                continue;
            }
            if let Ok(sp) = camera.world_to_viewport(cam_tf, to_render(w.ships.pos[i]))
                && sp.cmpge(min).all()
                && sp.cmple(max).all()
            {
                picked.push(i as u32);
            }
        }
    }

    // An inspected enemy is always selected alone, never mixed with own ships.
    let only_enemy = picked.len() == 1 && w.ships.team[picked[0] as usize] != PLAYER_TEAM;
    if only_enemy {
        sel.ships = picked;
        return;
    }
    if !shift(&keys) {
        sel.ships.clear();
    }
    sel.ships.retain(|&i| w.ships.team[i as usize] == PLAYER_TEAM);
    sel.ships.extend(picked);
    sel.ships.sort_unstable();
    sel.ships.dedup();
}

fn own_selected(sim: &Sim, sel: &Selection) -> Vec<u32> {
    sel.ships
        .iter()
        .copied()
        .filter(|&i| sim.world.ships.team[i as usize] == PLAYER_TEAM)
        .collect()
}

fn mouse_orders(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform, &Rig)>,
    mut sim: ResMut<Sim>,
    sel: Res<Selection>,
    mut markers: ResMut<Markers>,
) {
    if !buttons.just_pressed(MouseButton::Right) || sim.mode != Mode::Live {
        return;
    }
    let (Ok(window), Ok((camera, cam_tf, rig))) = (windows.single(), cams.single()) else {
        return;
    };
    let ships = own_selected(&sim, &sel);
    let Some(p) = ground(window, camera, cam_tf) else {
        return;
    };
    if ships.is_empty() {
        return;
    }

    let (command, kind, at) =
        if let Some(target) = ship_at(&sim, p, rig.distance, |t| t != PLAYER_TEAM) {
            let at = to_render(sim.world.ships.pos[target as usize]);
            (Command::Attack { ships, target }, MarkerKind::Attack, at)
        } else {
            let to = armada_sim::Vec2::new(
                armada_sim::Fx::from_f64(f64::from(p.x / SCALE)),
                armada_sim::Fx::from_f64(f64::from(p.z / SCALE)),
            );
            if ctrl(&keys) {
                (Command::AttackMove { ships, to }, MarkerKind::AttackMove, p)
            } else {
                (Command::Move { ships, to }, MarkerKind::Move, p)
            }
        };
    sim.issue(command);
    markers.0.push(Marker {
        pos: at,
        age: 0.0,
        kind,
    });
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut sel: ResMut<Selection>,
    mut rigs: Query<&mut Rig>,
    mut toast: ResMut<Toast>,
) {
    let own = own_selected(&sim, &sel);
    if keys.just_pressed(KeyCode::KeyH) && !own.is_empty() {
        sim.issue(Command::Stop { ships: own.clone() });
    }
    if keys.just_pressed(KeyCode::KeyG) && !own.is_empty() {
        sim.issue(Command::Engage { ships: own.clone() });
    }
    if keys.just_pressed(KeyCode::Escape) {
        sel.ships.clear();
    }
    if ctrl(&keys) && keys.just_pressed(KeyCode::KeyA) {
        let w = &sim.world;
        sel.ships = (0..w.ships.len() as u32)
            .filter(|&i| w.ships.alive[i as usize] && w.ships.team[i as usize] == PLAYER_TEAM)
            .collect();
    }

    for (g, key) in GROUP_KEYS.iter().enumerate() {
        if !keys.just_pressed(*key) {
            continue;
        }
        if ctrl(&keys) {
            sel.groups[g] = own.clone();
            toast.show(format!("Gruppe {} gespeichert", g + 1));
        } else {
            let alive = &sim.world.ships.alive;
            let group: Vec<u32> = sel.groups[g]
                .iter()
                .copied()
                .filter(|&i| alive[i as usize])
                .collect();
            sel.ships = group;
        }
    }

    if keys.just_pressed(KeyCode::KeyF)
        && !sel.ships.is_empty()
        && let Ok(mut rig) = rigs.single_mut()
    {
        let sum: Vec3 = sel
            .ships
            .iter()
            .map(|&i| to_render(sim.world.ships.pos[i as usize]))
            .sum();
        rig.focus = sum / sel.ships.len() as f32;
    }

    if keys.just_pressed(KeyCode::F5) {
        match sim.save_replay() {
            Ok(path) => toast.show(format!("Replay gespeichert: {}", path.display())),
            Err(e) => toast.show(format!("Replay nicht gespeichert: {e}")),
        }
    }
}
