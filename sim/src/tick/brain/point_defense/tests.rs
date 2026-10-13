use super::*;
use crate::ids::{PlayerId, Target, UnitId};
use crate::state::Shell;
use crate::stats::{DomainMask, ProjectileKind, UnitKind};
use chassis::fx::Vec2Fx;

fn at(x: &str, y: &str) -> Vec2Fx {
    Vec2Fx::new(Fx::lit(x), Fx::lit(y))
}

/// A skirmish world emptied of machines and shells, so each test stages
/// only the defenders and shells it reasons about.
fn world() -> State {
    let mut state = crate::Scenario::skirmish()
        .build()
        .expect("skirmish builds");
    state.units.clear();
    state.shells.clear();
    state.tick = 100;
    state
}

fn mantle(state: &mut State, player: u8, pos: Vec2Fx) -> UnitId {
    state.spawn_unit(PlayerId(player), UnitKind::Mantle, pos)
}

fn charges(state: &State, id: UnitId) -> (u8, u16) {
    let held = state.unit(id).unwrap().point_defense.unwrap();
    (held.charges, held.recharge)
}

/// A hostile 45-damage shell from seat `player`, launched on tick 90 and
/// landing on tick 110, flying a straight line from `launch` to `impact`.
fn shell(player: u8, launch: Vec2Fx, impact: Vec2Fx) -> Shell {
    Shell {
        kind: ProjectileKind::Shell,
        shooter: Target::Unit(UnitId(9_000)),
        player: PlayerId(player),
        launch,
        impact,
        launched_at: 90,
        arrival: 110,
        damage: 45,
        targets: DomainMask::GROUND,
        splash: None,
    }
}

fn intercepted(events: &[Event]) -> Vec<(UnitId, Vec2Fx)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ShellIntercepted {
                defender, impact, ..
            } => Some((*defender, *impact)),
            _ => None,
        })
        .collect()
}

#[test]
fn charges_return_one_at_a_time_and_the_counter_rests_while_full() {
    let mut state = world();
    let id = mantle(&mut state, 0, at("10.5", "10.5"));
    let stats = UnitKind::Mantle.stats().point_defense.unwrap();
    assert_eq!(charges(&state, id), (stats.charges, 0));
    state.units[0].point_defense.as_mut().unwrap().charges = 0;
    let mut events = Vec::new();
    for charge in 1..=stats.charges {
        for _ in 1..stats.recharge_ticks {
            run(&mut state, &mut events);
        }
        assert_eq!(charges(&state, id), (charge - 1, stats.recharge_ticks - 1));
        run(&mut state, &mut events);
        assert_eq!(charges(&state, id), (charge, 0));
    }
    run(&mut state, &mut events);
    assert_eq!(charges(&state, id), (stats.charges, 0));
    assert!(events.is_empty());
}

#[test]
fn spending_a_charge_keeps_a_running_recharge() {
    let mut state = world();
    let id = mantle(&mut state, 0, at("10.5", "10.5"));
    state.units[0].point_defense.as_mut().unwrap().charges = 1;
    state.units[0].point_defense.as_mut().unwrap().recharge = 10;
    state
        .shells
        .push(shell(1, at("11", "10.5"), at("10.5", "10.5")));
    let mut events = Vec::new();
    run(&mut state, &mut events);
    assert_eq!(intercepted(&events).len(), 1);
    assert_eq!(charges(&state, id), (0, 11));
}

#[test]
fn a_shell_aimed_inside_is_stopped_only_once_its_flight_enters_the_reach() {
    let mut state = world();
    let id = mantle(&mut state, 0, at("10", "10"));
    // Launched 20 tiles east at tick 90, landing on the defender at 110: one
    // tile of track per tick, so the shell crosses the 4.5-tile reach between
    // ticks 105 and 106.
    state.shells.push(shell(1, at("30", "10"), at("10", "10")));
    let mut events = Vec::new();
    while state.tick < 105 {
        run(&mut state, &mut events);
        state.tick += 1;
    }
    run(&mut state, &mut events);
    assert!(events.is_empty(), "5 tiles out is beyond reach");
    state.tick = 106;
    run(&mut state, &mut events);
    assert_eq!(intercepted(&events), vec![(id, at("10", "10"))]);
    assert!(state.shells.is_empty());
    let Event::ShellIntercepted { at: point, .. } = events[0] else {
        unreachable!("only an interception was reported");
    };
    assert!(
        point.dist_sq(at("14", "10")) < Fx::lit("0.0001"),
        "{point:?}"
    );
}

#[test]
fn a_shell_aimed_outside_the_reach_passes_through_it() {
    let mut state = world();
    mantle(&mut state, 0, at("10", "10"));
    state.shells.push(shell(1, at("4", "10"), at("16", "10")));
    let mut events = Vec::new();
    for tick in 91..=110 {
        state.tick = tick;
        run(&mut state, &mut events);
    }
    assert!(events.is_empty());
    assert_eq!(state.shells.len(), 1);
}

#[test]
fn the_defenders_own_shells_are_left_alone() {
    let mut state = world();
    mantle(&mut state, 0, at("10", "10"));
    state.shells.push(shell(0, at("11", "10"), at("10", "10")));
    let mut events = Vec::new();
    run(&mut state, &mut events);
    assert!(events.is_empty());
    assert_eq!(state.shells.len(), 1);
}

#[test]
fn every_charge_may_fire_in_one_tick_and_the_rest_land() {
    let mut state = world();
    let id = mantle(&mut state, 0, at("10", "10"));
    for offset in ["9.5", "10", "10.5", "11"] {
        state
            .shells
            .push(shell(1, at("12", offset), at("10", offset)));
    }
    let mut events = Vec::new();
    run(&mut state, &mut events);
    assert_eq!(intercepted(&events).len(), 3);
    assert_eq!(charges(&state, id).0, 0);
    // The nearest three go first; the farthest, aimed at y = 11, flies on.
    assert_eq!(state.shells.len(), 1);
    assert_eq!(state.shells[0].impact, at("10", "11"));
}

#[test]
fn the_earliest_launch_takes_a_scarce_charge_before_a_nearer_shell() {
    let mut state = world();
    mantle(&mut state, 0, at("10", "10"));
    state.units[0].point_defense.as_mut().unwrap().charges = 1;
    let mut late = shell(1, at("10.5", "10"), at("10", "10"));
    late.launched_at = 95;
    let early = shell(1, at("13", "10"), at("12", "10"));
    state.shells = vec![late, early];
    let mut events = Vec::new();
    run(&mut state, &mut events);
    assert_eq!(state.shells.len(), 1);
    assert_eq!(state.shells[0].launched_at, 95);
}

#[test]
fn the_nearest_defender_spends_for_a_shell_both_could_stop() {
    let mut state = world();
    let near = mantle(&mut state, 0, at("10", "10"));
    let far = mantle(&mut state, 0, at("10", "13"));
    state
        .shells
        .push(shell(1, at("11", "10"), at("10", "10.5")));
    let mut events = Vec::new();
    run(&mut state, &mut events);
    assert_eq!(intercepted(&events), vec![(near, at("10", "10.5"))]);
    assert_eq!(charges(&state, near).0, 2);
    assert_eq!(charges(&state, far).0, 3);
}

#[test]
fn equidistant_ties_follow_each_defenders_own_map_frame() {
    // Two defenders mirrored through the map center, each with one charge
    // and two hostile shells mirrored about it. Each must stop the shell on
    // the same side of itself, as seen from its own half of the map.
    let mut state = world();
    let (width, height) = (state.map.width(), state.map.height());
    let mirror = |p: Vec2Fx| Vec2Fx::new(Fx::from_num(width), Fx::from_num(height)) - p;
    let west = at("10", "10");
    let east = mirror(west);
    let a = mantle(&mut state, 0, west);
    let b = mantle(&mut state, 1, east);
    for slot in 0..2 {
        state.units[slot].point_defense.as_mut().unwrap().charges = 1;
    }
    let ahead = at("11", "10");
    let behind = at("9", "10");
    state.shells = vec![
        shell(1, ahead, ahead),
        shell(1, behind, behind),
        shell(0, mirror(ahead), mirror(ahead)),
        shell(0, mirror(behind), mirror(behind)),
    ];
    let mut events = Vec::new();
    run(&mut state, &mut events);
    let stopped = intercepted(&events);
    assert_eq!(stopped.len(), 2);
    let by_a = stopped.iter().find(|(d, _)| *d == a).unwrap().1;
    let by_b = stopped.iter().find(|(d, _)| *d == b).unwrap().1;
    assert_eq!(mirror(by_a), by_b);
}

#[test]
fn forged_shell_ticks_never_break_the_pass() {
    let mut state = world();
    mantle(&mut state, 0, at("10", "10"));
    let mut far_future = shell(1, at("40", "10"), at("10", "10"));
    far_future.arrival = u64::MAX;
    let mut unlaunched = shell(1, at("40", "10"), at("10.5", "10"));
    unlaunched.launched_at = 500;
    unlaunched.arrival = 600;
    let mut instant = shell(1, at("40", "10"), at("11", "10"));
    instant.launched_at = 110;
    instant.arrival = 110;
    state.shells = vec![far_future, unlaunched, instant];
    let mut events = Vec::new();
    run(&mut state, &mut events);
    // Only the zero-span shell stands at its impact; the others are still
    // at their launch points, outside the reach.
    assert_eq!(
        intercepted(&events),
        vec![(state.units[0].id, at("11", "10"))]
    );
}
