//! armada-viewer [scenario.ron | --replay file.replay.ron] [photo flags, see shot.rs]
//!
//! Without arguments it opens the start menu. Live: you command team 0, the
//! AI plays team 1. Replay: watch only. The sim runs in FixedUpdate at
//! TICK_HZ and only changes through commands; everything here reads it and
//! interpolates between the last two ticks.
#![allow(clippy::float_arithmetic)]

mod camera;
mod hud;
mod input;
mod menu;
mod models;
mod render;
mod shot;
mod sim;

use bevy::{prelude::*, window::WindowResolution};

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    Menu,
    Game,
}

/// Put a resource back to its default when a battle ends.
pub fn reset<R: Resource + Default>(mut commands: Commands) {
    commands.insert_resource(R::default());
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let loaded = shot::Shot::parse(&mut args)
        .map_err(Into::into)
        .and_then(|shot| {
            if args.is_empty() && shot.as_ref().is_none_or(|s| s.tick == 0) {
                return Ok((None, shot));
            }
            let mut sim = sim::Sim::load(&args, shot.is_some())?;
            if let Some(shot) = &shot {
                sim.fast_forward(shot.tick.saturating_sub(shot::LIVE_TICKS));
            }
            Ok::<_, Box<dyn std::error::Error>>((Some(sim), shot))
        });
    let (sim, shot) = match loaded {
        Ok(loaded) => loaded,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let mut window = Window {
        title: "Armada".into(),
        ..default()
    };
    if shot.is_some() {
        window.resolution = WindowResolution::new(1920, 1080);
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.008, 0.008, 0.015)))
    .insert_resource(Time::<Fixed>::from_hz(f64::from(armada_sim::TICK_HZ)))
    .insert_state(if sim.is_some() {
        Screen::Game
    } else {
        Screen::Menu
    })
    .add_plugins((
        sim::plugin,
        camera::plugin,
        input::plugin,
        render::plugin,
        hud::plugin,
        menu::plugin,
    ));
    if let Some(sim) = sim {
        app.insert_resource(sim);
    }
    if let Some(shot) = shot {
        app.insert_resource(shot).add_plugins(shot::plugin);
    }
    app.run();
}
