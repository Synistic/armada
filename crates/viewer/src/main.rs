//! armada-viewer [scenario.ron | --replay file.replay.ron]
//!
//! Live: you command team 0, team 1 fights on its own. Replay: watch only.
//! The sim runs in FixedUpdate at TICK_HZ and only changes through commands;
//! everything here reads it and interpolates between the last two ticks.
#![allow(clippy::float_arithmetic)]

mod camera;
mod hud;
mod input;
mod render;
mod sim;

use bevy::prelude::*;

fn main() {
    let sim = match sim::Sim::load() {
        Ok(sim) => sim,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Armada".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.008, 0.008, 0.015)))
        .insert_resource(Time::<Fixed>::from_hz(f64::from(armada_sim::TICK_HZ)))
        .insert_resource(sim)
        .add_plugins((
            sim::plugin,
            camera::plugin,
            input::plugin,
            render::plugin,
            hud::plugin,
        ))
        .run();
}
