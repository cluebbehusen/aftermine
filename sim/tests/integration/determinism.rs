//! Command replay and serialized state determinism.
use crate::common;
use crate::{levels, peaks, pits, shells};
use aftermine_sim::{
    Command, Order, PlayerCommand, PlayerId, SIM_VERSION, Scenario, State, UnitKind,
};
use chassis::grid::TilePos;
use chassis::replay::Replay;
use common::{cmd, commanded, open_arena, orders, run, unit, unit_id};

#[test]
fn replay_roundtrip_executes_and_reproduces_advance() {
    let scenario = open_arena(24, 12, vec![unit(0, UnitKind::Sentinel, 3, 6)]);
    let mut live = scenario.build().unwrap();
    let mover = live.units()[0].id;
    let goal = TilePos::new(18, 6);
    let advance = cmd(0, orders::advance([mover], goal));
    let mut replay = Replay::<_, _, ()>::new(SIM_VERSION, "test", scenario);
    replay.record(live.current_tick(), advance.clone());
    live.tick(&[advance]);
    assert_eq!(
        commanded(live.unit(mover).unwrap().order),
        Order::Advance { goal: goal.into() }
    );
    for _ in 1..100 {
        live.tick(&[]);
    }
    let live_hash = live.hash();

    let replay: Replay<Scenario, PlayerCommand> =
        serde_json::from_str(&serde_json::to_string(&replay).unwrap()).unwrap();
    assert!(matches!(
        replay.commands.as_slice(),
        [stamped] if matches!(
            stamped.command.command,
            Command::Advance { goal: recorded, .. } if recorded == goal
        )
    ));

    let mut replayed = replay.setup.build().unwrap();
    let mut cursor = replay.cursor();
    for tick in 0..100 {
        let commands: Vec<PlayerCommand> = cursor
            .take_tick(replayed.current_tick())
            .iter()
            .map(|stamped| stamped.command.clone())
            .collect();
        replayed.tick(&commands);
        if tick == 0 {
            assert_eq!(
                commanded(replayed.unit(mover).unwrap().order),
                Order::Advance { goal: goal.into() }
            );
        }
    }
    assert!(cursor.is_finished());
    assert_eq!(replayed.hash(), live_hash);
    assert_eq!(
        commanded(replayed.unit(mover).unwrap().order),
        Order::Advance { goal: goal.into() }
    );
}

#[test]
fn serde_roundtrip_preserves_queued_programs() {
    // Non-empty queues serialize through skip-if-default fields. A
    // mid-patrol snapshot must survive losslessly and keep ticking
    // identically.
    use aftermine_sim::{PlayerCommand, PlayerId};
    use chassis::grid::TilePos;
    let mut state = Scenario::skirmish().build().unwrap();
    let movers: Vec<_> = state.units().iter().map(|u| u.id).collect();
    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: orders::patrol(movers, [TilePos::new(10, 10), TilePos::new(14, 6)]),
    }]);
    for _ in 0..100 {
        state.tick(&[]);
    }
    assert!(
        state.units().iter().any(|u| u.looping),
        "test premise: someone is patrolling"
    );
    let json = serde_json::to_string(&state).unwrap();
    let mut restored: State = serde_json::from_str(&json).unwrap();
    assert_eq!(state.hash(), restored.hash(), "roundtrip must be lossless");
    for _ in 0..100 {
        state.tick(&[]);
        restored.tick(&[]);
    }
    assert_eq!(state.hash(), restored.hash());
}

/// `scenario` after every unit of player 0 hunts toward `goal` for 300 ticks.
fn hunted(scenario: &Scenario, goal: TilePos) -> State {
    let mut state = scenario.build().unwrap();
    let hunters: Vec<_> = state
        .units()
        .iter()
        .filter(|u| u.player == PlayerId(0))
        .map(|u| u.id)
        .collect();
    state.tick(&[cmd(0, orders::hunt(hunters, goal))]);
    run(&mut state, 300);
    state
}

/// Two plays of each staged match agree bit for bit, and a save of the
/// result restores the same bits.
#[test]
fn staged_matches_replay_bit_identically() {
    type Play = fn() -> State;
    let rows: [(&str, Play); 4] = [
        ("chasm", || {
            let units = vec![
                unit(0, UnitKind::Sentinel, 10, 4),
                unit(1, UnitKind::Sentinel, 15, 6),
                unit(0, UnitKind::Talon, 9, 7),
            ];
            hunted(&pits::chasm(2, units), TilePos::new(18, 6))
        }),
        ("ridge", || {
            let mut state = peaks::ridge(
                true,
                vec![
                    unit(0, UnitKind::Scuttler, 8, 3),
                    unit(0, UnitKind::Talon, 9, 8),
                    unit(1, UnitKind::Scuttler, 16, 3),
                    unit(1, UnitKind::Buzzard, 15, 8),
                ],
            )
            .build()
            .unwrap();
            let talon = unit_id(&state, |u| u.kind == UnitKind::Talon);
            let buzzard = unit_id(&state, |u| u.kind == UnitKind::Buzzard);
            state.tick(&[
                cmd(0, orders::run([talon], TilePos::new(18, 8))),
                cmd(1, orders::run([buzzard], TilePos::new(5, 8))),
            ]);
            run(&mut state, 300);
            state
        }),
        ("tiers", || {
            let units = vec![
                unit(0, UnitKind::Sentinel, 4, 2),
                unit(0, UnitKind::Talon, 6, 3),
                unit(1, UnitKind::Sentinel, 16, 10),
            ];
            hunted(&levels::stair(3, units), TilePos::new(16, 10))
        }),
        ("shells in flight", || {
            let (mut state, _) = shells::open_fire();
            run(&mut state, 17);
            assert!(!state.shells().is_empty(), "premise: a shell is airborne");
            run(&mut state, 100);
            state
        }),
    ];
    for (name, play) in rows {
        let state = play();
        assert_eq!(state.hash(), play().hash(), "{name}: two plays differ");
        let restored: State =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.hash(), state.hash(), "{name}: the save differs");
    }
}
