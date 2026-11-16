use armada_sim::{
    Command, Delivery, HardpointKind, Order, TICK_HZ,
    economy::{income, owned_points, pop_used},
};
use bevy::{picking::hover::Hovered, prelude::*, text::FontSize};

use crate::{
    Screen,
    input::Selection,
    reset,
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
Z X C V   bauen     Rechtsklick auf Bauknopf   abbrechen\n\
Rechtsklick mit Station gewählt   Sammelpunkt\n\
Leertaste   Pause     + / -   Tempo     F5   Replay speichern     F10   Menü";
const BUILD_KEYS: [KeyCode; 4] = [KeyCode::KeyZ, KeyCode::KeyX, KeyCode::KeyC, KeyCode::KeyV];
const BUILD_KEY_LABELS: [&str; 4] = ["Z", "X", "C", "V"];
const PANEL_BG: Color = Color::srgba(0.05, 0.06, 0.09, 0.85);
const PANEL_BORDER: Color = Color::srgba(0.6, 0.7, 0.9, 0.25);

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

/// Bevy's built-in font is an ASCII subset and has no umlauts.
#[derive(Resource)]
pub struct UiFont(pub Handle<Font>);

impl FromWorld for UiFont {
    fn from_world(world: &mut World) -> Self {
        let bytes = include_bytes!("../assets/fonts/FiraSans-Regular.ttf").to_vec();
        Self(
            world
                .resource_mut::<Assets<Font>>()
                .add(Font::from_bytes(bytes)),
        )
    }
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct SelectionText;

#[derive(Component)]
struct HelpText;

#[derive(Component)]
struct OutcomeText;

#[derive(Component)]
struct BuildButton {
    ship: u16,
    key: KeyCode,
}

#[derive(Component)]
struct BuildLabel(u16);

#[derive(Component)]
struct BuildProgress(u16);

pub fn plugin(app: &mut App) {
    app.init_resource::<Toast>()
        .init_resource::<UiFont>()
        .add_systems(OnEnter(Screen::Game), spawn)
        .add_systems(
            Update,
            (status, selection, help, build_input, build_bar, outcome)
                .run_if(in_state(Screen::Game)),
        )
        .add_systems(OnExit(Screen::Game), reset::<Toast>);
}

fn text_bundle(font: &Handle<Font>, size: f32, node: Node) -> impl Bundle {
    (
        Text::new(""),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgba(0.9, 0.93, 1.0, 0.95)),
        node,
    )
}

fn spawn(mut commands: Commands, font: Res<UiFont>, sim: Res<Sim>) {
    let font = &font.0;
    commands.spawn((
        DespawnOnExit(Screen::Game),
        StatusText,
        text_bundle(
            font,
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
        DespawnOnExit(Screen::Game),
        SelectionText,
        text_bundle(
            font,
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
        DespawnOnExit(Screen::Game),
        HelpText,
        Text::new(HELP),
        TextFont {
            font: font.clone().into(),
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

    commands.spawn((
        DespawnOnExit(Screen::Game),
        OutcomeText,
        Text::new(""),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(64.0),
            ..default()
        },
        TextColor(Color::srgba(0.95, 0.97, 1.0, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(32.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));

    // Build bar, bottom center. Only with a station of our own to build at.
    let Some(producer) = sim.player_producer().filter(|_| sim.mode == Mode::Live) else {
        return;
    };
    let w = &sim.world;
    let builds = w.defs.ships[usize::from(w.ships.def[producer as usize])]
        .builds
        .clone();
    commands
        .spawn((
            DespawnOnExit(Screen::Game),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
        ))
        .with_children(|bar| {
            for (k, &ship) in builds.iter().enumerate().take(BUILD_KEYS.len()) {
                bar.spawn((
                    Hovered::default(),
                    BuildButton {
                        ship,
                        key: BUILD_KEYS[k],
                    },
                    Node {
                        width: Val::Px(150.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(PANEL_BG),
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|b| {
                    b.spawn((BuildLabel(ship), text_bundle(font, 14.0, Node::default())));
                    b.spawn((
                        Node {
                            height: Val::Px(3.0),
                            width: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                    ))
                    .with_children(|track| {
                        track.spawn((
                            BuildProgress(ship),
                            Node {
                                height: Val::Percent(100.0),
                                width: Val::Percent(0.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.55, 0.8, 1.0)),
                        ));
                    });
                });
            }
        });
}

fn build_input(
    buttons: Query<(&Hovered, &BuildButton)>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
) {
    let Some(producer) = sim.player_producer() else {
        return;
    };
    for (hover, b) in &buttons {
        let hovered = hover.get();
        if (hovered && mouse.just_pressed(MouseButton::Left)) || keys.just_pressed(b.key) {
            sim.issue(Command::Build {
                producer,
                ship: b.ship,
            });
        }
        if hovered && mouse.just_pressed(MouseButton::Right) {
            sim.issue(Command::CancelBuild { producer });
        }
    }
}

fn build_bar(
    sim: Res<Sim>,
    mut labels: Query<(&BuildLabel, &mut Text, &mut TextColor)>,
    mut bars: Query<(&BuildProgress, &mut Node)>,
) {
    let w = &sim.world;
    let producer = sim.player_producer().map(|p| p as usize);
    let queue: &[u16] = producer.map_or(&[], |p| &w.ships.queue[p]);
    let credits = w.teams[usize::from(PLAYER_TEAM)].credits;
    for (label, mut text, mut color) in &mut labels {
        let def = &w.defs.ships[usize::from(label.0)];
        let queued = queue.iter().filter(|&&q| q == label.0).count();
        let count = if queued > 0 {
            format!("   {queued}x")
        } else {
            String::new()
        };
        let key = def_key(w, producer, label.0);
        text.0 = format!(
            "{}{count}\n{} Cr   Pop {}   {key}",
            def.display_name, def.cost, def.pop,
        );
        let alpha = if producer.is_some() && credits >= def.cost {
            0.95
        } else {
            0.4
        };
        color.0 = Color::srgba(0.9, 0.93, 1.0, alpha);
    }
    for (bar, mut node) in &mut bars {
        let frac = match (producer, queue.first()) {
            (Some(p), Some(&front)) if front == bar.0 => {
                let total = w.defs.ships[usize::from(front)].build_ticks.max(1);
                w.ships.build_progress[p] as f32 / total as f32
            }
            _ => 0.0,
        };
        node.width = Val::Percent(frac * 100.0);
    }
}

/// Hotkey label for a buildable ship: its slot in the producer's build list.
fn def_key(w: &armada_sim::World, producer: Option<usize>, ship: u16) -> &'static str {
    producer
        .and_then(|p| {
            w.defs.ships[usize::from(w.ships.def[p])]
                .builds
                .iter()
                .position(|&b| b == ship)
        })
        .and_then(|k| BUILD_KEY_LABELS.get(k).copied())
        .unwrap_or("")
}

fn outcome(sim: Res<Sim>, mut q: Query<&mut Text, With<OutcomeText>>) {
    let Ok(mut text) = q.single_mut() else {
        return;
    };
    text.0 = match sim.world.winner {
        Some(t) if t == PLAYER_TEAM => "Sieg".into(),
        Some(_) => "Niederlage".into(),
        None => String::new(),
    };
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
    let toast_line = if toast.timer > 0.0 {
        toast.text.as_str()
    } else {
        ""
    };
    let economy = if w.rules.income > 0 || !w.points.is_empty() {
        format!(
            "\n{} Cr   +{}/s   Pop {} / {}   Punkte {} / {}",
            w.teams[usize::from(PLAYER_TEAM)].credits,
            income(w, PLAYER_TEAM),
            pop_used(w, PLAYER_TEAM),
            w.rules.pop_cap,
            owned_points(w, PLAYER_TEAM),
            w.points.len(),
        )
    } else {
        String::new()
    };
    text.0 = format!(
        "{:02}:{:02}  {}x{mode}{paused}\nBlau {}   Rot {}   F1 Hilfe   F10 Menü{economy}\n{toast_line}",
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
            let side = if w.ships.team[i] == PLAYER_TEAM {
                ""
            } else {
                "  (Gegner)"
            };
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
