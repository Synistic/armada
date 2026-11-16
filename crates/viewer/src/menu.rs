//! Start menu: pick a scenario and how good the AI is, then start.

use std::{fs, path::PathBuf};

use armada_ai::Difficulty;
use armada_data::{content_dir, scenario_name};
use bevy::{picking::hover::Hovered, prelude::*, text::FontSize};

use crate::{Screen, hud::UiFont, sim::Sim};

const TEXT: Color = Color::srgba(0.9, 0.93, 1.0, 0.95);
const DIM: Color = Color::srgba(0.75, 0.8, 0.9, 0.6);
const BUTTON_BG: Color = Color::srgba(0.05, 0.06, 0.09, 0.85);
const BUTTON_HOVER: Color = Color::srgba(0.1, 0.12, 0.18, 0.95);
const BORDER: Color = Color::srgba(0.6, 0.7, 0.9, 0.25);
const BORDER_ON: Color = Color::srgb(0.55, 0.8, 1.0);

/// What the menu has selected. Survives a battle, so the next round starts
/// with the same choice.
#[derive(Resource, Default)]
struct Choice {
    scenarios: Vec<(PathBuf, String)>,
    scenario: usize,
    difficulty: Difficulty,
    error: String,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MenuButton {
    Scenario(usize),
    Level(Difficulty),
    Start,
    Quit,
}

#[derive(Component)]
struct ErrorText;

pub fn plugin(app: &mut App) {
    app.init_resource::<Choice>()
        .add_systems(OnEnter(Screen::Menu), spawn)
        .add_systems(Update, (click, highlight).run_if(in_state(Screen::Menu)));
}

/// Every .ron in content/scenarios, skirmish first.
fn scan() -> Vec<(PathBuf, String)> {
    let dir = content_dir().join("scenarios");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|it| {
            it.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "ron"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort_by_key(|p| (!p.ends_with("skirmish.ron"), p.clone()));
    paths
        .into_iter()
        .filter_map(|p| scenario_name(&p).ok().map(|name| (p, name)))
        .collect()
}

fn label(level: Difficulty) -> &'static str {
    match level {
        Difficulty::Easy => "Leicht",
        Difficulty::Normal => "Normal",
        Difficulty::Hard => "Schwer",
    }
}

fn spawn(mut commands: Commands, font: Res<UiFont>, mut choice: ResMut<Choice>) {
    choice.scenarios = scan();
    choice.scenario = choice
        .scenario
        .min(choice.scenarios.len().saturating_sub(1));
    let font = &font.0;
    let text = |s: &str, size: f32, color: Color| {
        (
            Text::new(s),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        )
    };
    let button = |kind: MenuButton, width: f32| {
        (
            kind,
            Hovered::default(),
            Node {
                width: Val::Px(width),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BG),
            BorderColor::all(BORDER),
        )
    };

    commands.spawn((DespawnOnExit(Screen::Menu), Camera2d));
    commands
        .spawn((
            DespawnOnExit(Screen::Menu),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                text("Armada", 56.0, TEXT),
                Node {
                    margin: UiRect::bottom(Val::Px(24.0)),
                    ..default()
                },
            ));

            root.spawn(text("Szenario", 15.0, DIM));
            for (k, (_, name)) in choice.scenarios.iter().enumerate() {
                root.spawn(button(MenuButton::Scenario(k), 360.0))
                    .with_children(|b| {
                        b.spawn(text(name, 17.0, TEXT));
                    });
            }

            root.spawn((
                text("Gegner", 15.0, DIM),
                Node {
                    margin: UiRect::top(Val::Px(18.0)),
                    ..default()
                },
            ));
            root.spawn(Node {
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|row| {
                for level in Difficulty::ALL {
                    row.spawn(button(MenuButton::Level(level), 114.0))
                        .with_children(|b| {
                            b.spawn(text(label(level), 17.0, TEXT));
                        });
                }
            });

            root.spawn(Node {
                column_gap: Val::Px(8.0),
                margin: UiRect::top(Val::Px(28.0)),
                ..default()
            })
            .with_children(|row| {
                row.spawn(button(MenuButton::Start, 176.0))
                    .with_children(|b| {
                        b.spawn(text("Starten", 17.0, TEXT));
                    });
                row.spawn(button(MenuButton::Quit, 176.0))
                    .with_children(|b| {
                        b.spawn(text("Beenden", 17.0, TEXT));
                    });
            });

            root.spawn((
                ErrorText,
                text(&choice.error, 14.0, Color::srgb(1.0, 0.5, 0.45)),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn click(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Hovered, &MenuButton)>,
    mut choice: ResMut<Choice>,
    mut next: ResMut<NextState<Screen>>,
    mut errors: Query<&mut Text, With<ErrorText>>,
    mut exit: MessageWriter<AppExit>,
) {
    let mut pressed = keys
        .just_pressed(KeyCode::Enter)
        .then_some(MenuButton::Start);
    if mouse.just_pressed(MouseButton::Left) {
        pressed = buttons
            .iter()
            .find(|(h, _)| h.get())
            .map(|(_, &b)| b)
            .or(pressed);
    }
    match pressed {
        Some(MenuButton::Scenario(k)) => choice.scenario = k,
        Some(MenuButton::Level(level)) => choice.difficulty = level,
        Some(MenuButton::Quit) => {
            exit.write(AppExit::Success);
        }
        Some(MenuButton::Start) => {
            let Some((path, _)) = choice.scenarios.get(choice.scenario) else {
                return;
            };
            match Sim::scenario(path, choice.difficulty) {
                Ok(sim) => {
                    choice.error.clear();
                    commands.insert_resource(sim);
                    next.set(Screen::Game);
                }
                Err(e) => {
                    choice.error = e.to_string();
                    if let Ok(mut t) = errors.single_mut() {
                        t.0.clone_from(&choice.error);
                    }
                }
            }
        }
        None => {}
    }
}

fn highlight(
    choice: Res<Choice>,
    mut buttons: Query<(
        &Hovered,
        &MenuButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (hovered, &kind, mut bg, mut border) in &mut buttons {
        let on = match kind {
            MenuButton::Scenario(k) => k == choice.scenario,
            MenuButton::Level(level) => level == choice.difficulty,
            MenuButton::Start | MenuButton::Quit => false,
        };
        bg.0 = if hovered.get() {
            BUTTON_HOVER
        } else {
            BUTTON_BG
        };
        *border = BorderColor::all(if on { BORDER_ON } else { BORDER });
    }
}
