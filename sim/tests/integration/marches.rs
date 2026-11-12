//! Marches: the units one Advance or Hunt command sends off together keep
//! pace, while Run and every other order leave each at its own top speed.

use crate::common::*;
use aftermine_sim::{Command, MarchId, Order, State, UnitId, UnitKind};
use chassis::fx::Fx;
use chassis::grid::TilePos;

/// Where every column in this suite is sent.
const GOAL: TilePos = TilePos::new(50, 7);

/// A column side by side at the west end of an open arena: a Scuttler, a
/// Sentinel, a Lancer and a Bombard, the fastest first.
fn column() -> (State, Vec<UnitId>) {
    let units = vec![
        unit(0, UnitKind::Scuttler, 9, 6),
        unit(0, UnitKind::Sentinel, 9, 8),
        unit(0, UnitKind::Lancer, 8, 6),
        unit(0, UnitKind::Bombard, 8, 8),
    ];
    let state = open_arena(64, 16, units).build().unwrap();
    let ids = state.units().iter().map(|u| u.id).collect();
    (state, ids)
}

/// The tick each of `ids` finished its walk, in `ids` order.
fn arrivals(state: &mut State, ids: &[UnitId], command: Command) -> Vec<u64> {
    let start = state.current_tick();
    state.tick(&[cmd(0, command)]);
    let mut done: Vec<Option<u64>> = vec![None; ids.len()];
    run_until(state, 1_500, |s, _| {
        for (slot, id) in ids.iter().enumerate() {
            if done[slot].is_none() && s.unit(*id).unwrap().order == Order::Idle {
                done[slot] = Some(s.current_tick() - start);
            }
        }
        done.iter().all(Option::is_some)
    });
    done.into_iter().map(Option::unwrap).collect()
}

/// How far the furthest of `ids` still is from [`GOAL`] when the first of
/// them finishes its walk.
fn trailing(state: &mut State, ids: &[UnitId], command: Command) -> Fx {
    state.tick(&[cmd(0, command)]);
    run_until(state, 1_500, |s, _| {
        ids.iter()
            .any(|id| s.unit(*id).unwrap().order == Order::Idle)
    });
    ids.iter()
        .map(|id| state.unit(*id).unwrap().pos.dist(GOAL.center()))
        .max()
        .unwrap()
}

fn spread(ticks: &[u64]) -> u64 {
    ticks.iter().max().unwrap() - ticks.iter().min().unwrap()
}

fn march_of(state: &State, id: UnitId) -> Option<MarchId> {
    match state.unit(id).unwrap().order {
        Order::Run { goal } | Order::Hunt { goal } | Order::Advance { goal } => goal.march,
        ref other => panic!("{id:?} is not walking: {other:?}"),
    }
}

#[test]
fn a_mixed_move_arrives_together_and_a_run_does_not() {
    let (state, ids) = column();
    let run = trailing(&mut state.clone(), &ids, orders::run(ids.clone(), GOAL));
    let moved = trailing(&mut state.clone(), &ids, orders::advance(ids.clone(), GOAL));
    assert!(
        run > Fx::from_num(20),
        "Run leaves the Bombard behind: {run}"
    );
    assert!(moved < Fx::from_num(4), "the move keeps together: {moved}");
    // Keeping together costs the slowest member almost nothing; the last
    // of it is the crowd it meets settling round the clicked tile.
    let run = arrivals(&mut state.clone(), &ids, orders::run(ids.clone(), GOAL));
    let moved = arrivals(&mut state.clone(), &ids, orders::advance(ids.clone(), GOAL));
    assert!(
        moved[3] <= run[3] + 40,
        "the Bombard's walk: {moved:?} against {run:?}"
    );
}

#[test]
fn a_mixed_hunt_arrives_together_too() {
    let (mut state, ids) = column();
    let hunted = trailing(&mut state, &ids, orders::hunt(ids.clone(), GOAL));
    assert!(hunted < Fx::from_num(4), "{hunted}");
}

#[test]
fn a_member_behind_hurries_to_arrive_with_the_rest() {
    let units = vec![
        unit(0, UnitKind::Sentinel, 6, 7),
        unit(0, UnitKind::Bombard, 16, 7),
    ];
    let mut state = open_arena(64, 16, units).build().unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let mut fastest = Fx::ZERO;
    let start = state.current_tick();
    state.tick(&[cmd(0, orders::advance(ids.clone(), GOAL))]);
    let mut landed = [None, None];
    run_until(&mut state, 1_500, |s, _| {
        fastest = fastest.max(s.unit(ids[0]).unwrap().drive_speed());
        for (slot, id) in ids.iter().enumerate() {
            if landed[slot].is_none() && s.unit(*id).unwrap().order == Order::Idle {
                landed[slot] = Some(s.current_tick() - start);
            }
        }
        landed.iter().all(Option::is_some)
    });
    let [Some(sentinel), Some(bombard)] = landed else {
        unreachable!("run_until waited for both");
    };
    assert!(
        fastest > UnitKind::Bombard.stats().speed,
        "the Sentinel outran the Bombard's pace to catch up: {fastest}"
    );
    assert!(
        sentinel.abs_diff(bombard) <= 40,
        "{sentinel} against {bombard}"
    );
}

#[test]
fn the_pace_lifts_when_the_slowest_member_leaves() {
    let (mut state, ids) = column();
    let (sentinel, bombard) = (ids[1], ids[3]);
    state.tick(&[cmd(0, orders::advance(ids.clone(), GOAL))]);
    run(&mut state, 80);
    let pace = UnitKind::Bombard.stats().speed;
    assert!(state.unit(sentinel).unwrap().drive_speed() <= pace);
    state.tick(&[cmd(0, orders::stop([bombard]))]);
    run_until(&mut state, 60, |s, _| {
        s.unit(sentinel).unwrap().drive_speed() > UnitKind::Lancer.stats().speed
            || s.unit(ids[2]).unwrap().drive_speed() > pace
    });
}

#[test]
fn only_mixed_moves_and_hunts_march() {
    let (mut state, ids) = column();
    state.tick(&[cmd(0, orders::advance(ids.clone(), GOAL))]);
    let march = march_of(&state, ids[0]);
    assert!(march.is_some());
    assert!(ids.iter().all(|id| march_of(&state, *id) == march));
    // A second leg is a march of its own.
    state.tick(&[cmd(
        0,
        orders::advance(ids.clone(), TilePos::new(40, 12)).queued(),
    )]);
    let Order::Advance { goal } = state.unit(ids[1]).unwrap().queue[0] else {
        panic!("the leg queued");
    };
    assert!(goal.march.is_some() && goal.march != march);

    let (mut state, ids) = column();
    state.tick(&[cmd(0, orders::run(ids.clone(), GOAL))]);
    assert!(ids.iter().all(|id| march_of(&state, *id).is_none()));

    let units = vec![
        unit(0, UnitKind::Sentinel, 9, 6),
        unit(0, UnitKind::Sentinel, 9, 8),
        unit(0, UnitKind::Condor, 8, 6),
        unit(0, UnitKind::Buzzard, 8, 8),
    ];
    let mut state = open_arena(64, 16, units).build().unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    state.tick(&[cmd(0, orders::advance(ids.clone(), GOAL))]);
    assert!(
        ids.iter().all(|id| march_of(&state, *id).is_none()),
        "even Sentinels, a Condor that cannot slow and a lone Buzzard keep their own speed"
    );
}

#[test]
fn a_mixed_flight_arrives_together() {
    let units = vec![
        unit(0, UnitKind::Talon, 9, 6),
        unit(0, UnitKind::Buzzard, 8, 8),
    ];
    let mut state = open_arena(64, 16, units).build().unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let alone = arrivals(&mut state.clone(), &ids, orders::run(ids.clone(), GOAL));
    let together = arrivals(&mut state, &ids, orders::advance(ids.clone(), GOAL));
    assert!(spread(&alone) > 100, "{alone:?}");
    assert!(spread(&together) <= 30, "{together:?}");
}

#[test]
fn a_march_survives_a_save_mid_walk() {
    let (mut state, ids) = column();
    state.tick(&[cmd(0, orders::advance(ids.clone(), GOAL))]);
    run(&mut state, 50);
    let mut restored: State = serde_json::from_str(&serde_json::to_string(&state).unwrap())
        .expect("a marching state loads");
    assert_eq!(restored, state);
    for _ in 0..200 {
        state.tick(&[]);
        restored.tick(&[]);
    }
    assert_eq!(restored.hash(), state.hash());
}

#[test]
fn a_refused_move_takes_no_march() {
    let (mut state, ids) = column();
    state.tick(&[cmd(0, orders::advance(ids.clone(), GOAL))]);
    for _ in 0..aftermine_sim::stats::ORDER_QUEUE_CAP {
        state.tick(&[cmd(
            0,
            orders::advance(ids.clone(), TilePos::new(30, 4)).queued(),
        )]);
    }
    let before = state.clone();
    let report = state.tick(&[cmd(
        0,
        orders::advance(ids.clone(), TilePos::new(30, 12)).queued(),
    )]);
    assert!(
        report
            .events
            .iter()
            .any(|e| matches!(e, aftermine_sim::Event::CommandRejected { .. })),
        "every queue is full"
    );
    let mut expected = before;
    expected.tick(&[]);
    assert_eq!(state, expected, "the refusal changed nothing");
}
