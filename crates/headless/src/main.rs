//! armada-headless run [scenario.ron] [--ticks N] [--ai easy,hard] [--record out.replay.ron]
//! armada-headless verify <file.replay.ron>
//!
//! `run` lets the AI play both sides and records its commands, so `verify`
//! replays without it.
#![allow(clippy::float_arithmetic)]

use std::{env, error::Error, fs, path::PathBuf, process::ExitCode, time::Instant};

use armada_ai::{Ai, Difficulty};
use armada_data::{
    REPLAY_FORMAT, Replay, build, content_dir, load_replay, read_sources, save_replay,
};
use armada_sim::{CommandLog, TimedCommand, World, run, state_hash, step};

const CHECKPOINT_EVERY: u32 = 15;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("verify") => cmd_verify(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  armada-headless run [scenario.ron] [--ticks N] [--ai easy,hard] [--record out.replay.ron]\n  armada-headless verify <file.replay.ron>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_run(args: &[String]) -> Result<bool, Box<dyn Error>> {
    let mut scenario = content_dir().join("scenarios/fleet_clash.ron");
    let mut ticks = None;
    let mut record = None;
    let mut levels = [Difficulty::Normal; 2];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ticks" => ticks = Some(it.next().ok_or("--ticks needs a value")?.parse::<u32>()?),
            "--record" => record = Some(PathBuf::from(it.next().ok_or("--record needs a path")?)),
            "--ai" => {
                let v = it.next().ok_or("--ai needs <team0>,<team1>")?;
                for (slot, name) in levels.iter_mut().zip(v.split(',')) {
                    *slot = Difficulty::parse(name).ok_or("difficulty is easy, normal or hard")?;
                }
            }
            other => scenario = PathBuf::from(other),
        }
    }

    let sources = read_sources(&scenario)?;
    let (mut world, spec) = build(&sources)?;
    let ticks = ticks.unwrap_or(spec.ticks);
    let mut log = CommandLog::default();
    let mut ais = [Ai::new(0, levels[0]), Ai::new(1, levels[1])];

    println!(
        "{} ships, {} hardpoints, {ticks} ticks",
        world.ships.len(),
        world.hardpoints.owner.len()
    );
    let start = Instant::now();
    let mut checkpoints = Vec::new();
    for _ in 0..ticks {
        let tick = world.tick;
        for ai in &mut ais {
            for command in ai.think(&world) {
                log.push(TimedCommand {
                    tick,
                    team: ai.team(),
                    command,
                });
            }
        }
        step(&mut world, log.for_tick(tick));
        if world.tick.is_multiple_of(CHECKPOINT_EVERY) {
            checkpoints.push((world.tick, state_hash(&world)));
        }
        if world.winner.is_some() {
            break;
        }
    }
    let ticks = world.tick;
    let secs = start.elapsed().as_secs_f64();

    report(&world, ticks, secs);
    if let Some(&(_, hash)) = checkpoints.last() {
        println!("final hash   {hash:016x}");
    }

    if let Some(path) = record {
        let replay = Replay {
            format: REPLAY_FORMAT,
            sources,
            ticks,
            commands: log,
            checkpoints,
        };
        fs::write(&path, save_replay(&replay)?)?;
        println!("replay       {}", path.display());
    }
    Ok(true)
}

fn cmd_verify(args: &[String]) -> Result<bool, Box<dyn Error>> {
    let path = PathBuf::from(args.first().ok_or("verify needs a replay path")?);
    let replay = load_replay(&path)?;
    let (mut world, _) = build(&replay.sources)?;

    let start = Instant::now();
    let checkpoints = run(&mut world, &replay.commands, replay.ticks, CHECKPOINT_EVERY);
    let secs = start.elapsed().as_secs_f64();
    report(&world, replay.ticks, secs);

    let mismatch = checkpoints
        .iter()
        .zip(&replay.checkpoints)
        .find(|(a, b)| a != b);
    match mismatch {
        None if checkpoints.len() == replay.checkpoints.len() => {
            println!("OK           {} checkpoints match", checkpoints.len());
            Ok(true)
        }
        None => {
            println!(
                "FAIL         checkpoint count {} vs {}",
                checkpoints.len(),
                replay.checkpoints.len()
            );
            Ok(false)
        }
        Some((got, want)) => {
            println!(
                "DESYNC       at tick {}: got {:016x}, replay has {:016x}",
                got.0, got.1, want.1
            );
            Ok(false)
        }
    }
}

fn report(world: &World, ticks: u32, secs: f64) {
    let ms_per_tick = secs * 1000.0 / f64::from(ticks.max(1));
    println!(
        "sim time     {secs:.2}s, {ms_per_tick:.3} ms/tick, {:.0} ticks/s",
        f64::from(ticks) / secs
    );
    println!(
        "survivors    team 0: {}, team 1: {}",
        world.alive_count(0),
        world.alive_count(1)
    );
    if !world.points.is_empty() {
        println!("points       {:?}", world.points.owner);
        println!(
            "credits      team 0: {}, team 1: {}",
            world.teams[0].credits, world.teams[1].credits
        );
    }
    match world.winner {
        Some(t) => println!(
            "winner       team {t} at {}:{:02}",
            world.tick / 15 / 60,
            world.tick / 15 % 60
        ),
        None => println!("winner       none yet"),
    }
}
