use armada_sim::{Delivery, HardpointKind, Order, TICK_HZ};
use bevy::{prelude::*, text::FontSize};

use crate::{
    input::Selection,
    sim::{Mode, PLAYER_TEAM, SPEEDS, Sim},
};

const TOAST_SECS: f32 = 3.0;
/// Single-ship panel lists at most this many hardpoints.
const HARDPOINT_ROWS: usize = 14;

const HELP: &str = "Linksklick / Ziehen   auswählen (Shift: hinzufügen)\n\
Rechtsklick   bewegen in Formation\n\
Rechtsklick auf Gegner   angreifen\n\
Strg + Rechtsklick   Angriffsbewegung\n\
H   halten     G   frei kämpfen\n\
Strg + A   alle eigenen Schiffe\n\
Strg + 1-9   Gruppe speichern     1-9   Gruppe wählen\n\
F   Kamera auf Auswahl     WASD / Q E / Mausrad   Kamera\n\
Leertaste   Pause     + / -   Tempo     F5   Replay speichern";

#[derive(Resource, Default)]
pub struct Toast {
    text: String,
    timer: f32,
}

impl Toast {
    pub fn show(&mut self, text: String) {
        self.text = text;
        self.timer = TOAST_SECS;
    }
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct SelectionText;

#[derive(Component)]
struct HelpText;

pub fn plugin(app: &mut App) {
    app.init_resource::<Toast>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (status, selection, help));
}

fn text_bundle(size: f32, node: Node) -> impl Bundle {
    (
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgba(0.9, 0.93, 1.0, 0.95)),
        node,
    )
}

fn spawn(mut commands: Commands) {
    commands.spawn((
        StatusText,
        text_bundle(
            15.0,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
        ),
    ));
    commands.spawn((
        SelectionText,
        text_bundle(
            14.0,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
        ),
    ));
    commands.spawn((
        HelpText,
        Text::new(HELP),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgba(0.85, 0.88, 0.95, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            right: Val::Px(12.0),
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn status(
    time: Res<Time>,
    sim: Res<Sim>,
    mut toast: ResMut<Toast>,
    mut q: Query<&mut Text, With<StatusText>>,
) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    toast.timer = (toast.timer - time.delta_secs()).max(0.0);
    let w = &sim.world;
    let secs = w.tick / TICK_HZ;
    let mode = match sim.mode {
        Mode::Live => "",
        Mode::Replay => "  Wiedergabe",
    };
    let paused = if sim.paused { "  Pausiert" } else { "" };
    let toast_line = if toast.timer > 0.0 { toast.text.as_str() } else { "" };
    text.0 = format!(
        "{:02}:{:02}  {}x{mode}{paused}\nBlau {}   Rot {}   F1 Hilfe\n{toast_line}",
        secs / 60,
        secs % 60,
        SPEEDS[sim.speed],
        w.alive_count(PLAYER_TEAM),
        w.alive_count(1),
    );
}

fn order_label(order: Order) -> &'static str {
    match order {
        Order::Engage => "Frei kämpfen",
        Order::Hold => "Halten",
        Order::MoveTo { .. } => "Bewegen",
        Order::AttackMove { .. } => "Angriffsbewegung",
        Order::Attack(_) => "Angriff",
    }
}

fn hardpoint_label(kind: &HardpointKind) -> &'static str {
    match kind {
        HardpointKind::Weapon(w) => match w.delivery {
            Delivery::Beam => "Geschütz",
            Delivery::Projectile { .. } => "Werfer",
        },
        HardpointKind::ShieldGenerator => "Schildgenerator",
        HardpointKind::Engine => "Triebwerk",
    }
}

fn selection(sim: Res<Sim>, sel: Res<Selection>, mut q: Query<&mut Text, With<SelectionText>>) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    let w = &sim.world;
    text.0 = match sel.ships.as_slice() {
        [] => String::new(),
        [id] => {
            let i = *id as usize;
            let def = &w.defs.ships[usize::from(w.ships.def[i])];
            let side = if w.ships.team[i] == PLAYER_TEAM { "" } else { "  (Gegner)" };
            let mut out = format!(
                "{}{side}\nRumpf {} / {}   Schild {} / {}\nBefehl: {}\n",
                def.display_name,
                w.ships.hull[i].max(0),
                def.hull,
                w.ships.shield[i],
                def.shield,
                order_label(w.ships.order[i]),
            );
            let first = w.ships.hp_first[i] as usize;
            for (k, hp) in def.hardpoints.iter().enumerate().take(HARDPOINT_ROWS) {
                let health = w.hardpoints.health[first + k];
                let state = if health <= 0 {
                    "zerstört".to_string()
                } else {
                    format!("{} %", health * 100 / hp.health)
                };
                out.push_str(&format!("\n{}  {state}", hardpoint_label(&hp.kind)));
            }
            out
        }
        many => {
            let mut counts = vec![0usize; w.defs.ships.len()];
            for &id in many {
                counts[usize::from(w.ships.def[id as usize])] += 1;
            }
            let mut out = format!("{} Schiffe", many.len());
            for (d, n) in counts.iter().enumerate() {
                if *n > 0 {
                    out.push_str(&format!("\n{}  {n}", w.defs.ships[d].display_name));
                }
            }
            out
        }
    };
}

fn help(keys: Res<ButtonInput<KeyCode>>, mut q: Query<&mut Visibility, With<HelpText>>) {
    if !keys.just_pressed(KeyCode::F1) {
        return;
    }
    if let Ok(mut vis) = q.single_mut() {
        *vis = match *vis {
            Visibility::Hidden => Visibility::Visible,
            _ => Visibility::Hidden,
        };
    }
}
