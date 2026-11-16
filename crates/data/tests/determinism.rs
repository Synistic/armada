use armada_data::{Replay, Sources, build, content_dir, load_replay, read_sources, save_replay};
use armada_sim::{Command, CommandLog, TimedCommand, Vec2, Fx, run, state_hash};

const TICKS: u32 = 600;

fn sources() -> Sources {
    read_sources(&content_dir().join("scenarios/fleet_clash.ron")).expect("load scenario")
}

fn scripted_commands() -> CommandLog {
    let mut log = CommandLog::default();
    // Team 0 corvettes (ids 0..600) swing east, then get released again.
    log.push(TimedCommand {
        tick: 30,
        team: 0,
        command: Command::Move {
            ships: (0..600).collect(),
            to: Vec2::new(Fx::from_int(3000), Fx::from_int(-1000)),
        },
    });
    // Team 1 dreadnoughts focus the first team 0 dreadnought.
    log.push(TimedCommand {
        tick: 90,
        team: 1,
        command: Command::Attack {
            ships: (1985..2000).collect(),
            target: 980,
        },
    });
    log.push(TimedCommand {
        tick: 240,
        team: 0,
        command: Command::Stop {
            ships: (0..600).collect(),
        },
    });
    log
}

#[test]
fn same_input_same_hash() {
    let src = sources();
    let log = scripted_commands();
    let (mut a, _) = build(&src).unwrap();
    let (mut b, _) = build(&src).unwrap();
    let ca = run(&mut a, &log, TICKS, 15);
    let cb = run(&mut b, &log, TICKS, 15);
    assert_eq!(ca, cb);
    assert!(a.ships.alive.iter().any(|x| !x), "no losses after {TICKS} ticks");
}

#[test]
fn commands_change_the_outcome() {
    let src = sources();
    let (mut a, _) = build(&src).unwrap();
    let (mut b, _) = build(&src).unwrap();
    run(&mut a, &CommandLog::default(), TICKS, 15);
    run(&mut b, &scripted_commands(), TICKS, 15);
    assert_ne!(state_hash(&a), state_hash(&b));
}

#[test]
fn snapshot_continues_identically() {
    let src = sources();
    let log = scripted_commands();
    let (mut a, _) = build(&src).unwrap();
    run(&mut a, &log, TICKS / 2, 15);
    let mut b = a.clone();
    let ca = run(&mut a, &log, TICKS / 2, 15);
    let cb = run(&mut b, &log, TICKS / 2, 15);
    assert_eq!(ca, cb);
}

#[test]
fn replay_roundtrip() {
    let src = sources();
    let log = scripted_commands();
    let (mut world, _) = build(&src).unwrap();
    let checkpoints = run(&mut world, &log, TICKS, 15);

    let replay = Replay {
        format: armada_data::REPLAY_FORMAT,
        sources: src,
        ticks: TICKS,
        commands: log,
        checkpoints,
    };
    let path = std::env::temp_dir().join("armada_roundtrip.replay.ron");
    std::fs::write(&path, save_replay(&replay).unwrap()).unwrap();
    let loaded = load_replay(&path).unwrap();

    let (mut again, _) = build(&loaded.sources).unwrap();
    let checkpoints = run(&mut again, &loaded.commands, loaded.ticks, 15);
    assert_eq!(checkpoints, loaded.checkpoints);
}
