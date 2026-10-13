//! The Scout order: a scout goes where it is sent while the way is safe by
//! its team's knowledge, stops short where it can still safely see, and
//! waits or gives up when it cannot.

use crate::common;
use common::{building, cmd, open_arena, run_until, unit};

use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::{BuildingSpec, UnitSpec};
use oxide_sim::stats::SCOUT_HOLD_TICKS;
use oxide_sim::{
    BuildingKind, Command, Event, Order, OrderKey, PlayerId, Scenario, StallReason, State, UnitId,
    UnitKind,
};

const WIDTH: usize = 40;
const HEIGHT: usize = 24;

fn field(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>) -> Scenario {
    let mut scenario = open_arena(WIDTH, HEIGHT, units);
    scenario.buildings = buildings;
    scenario
}

fn scout_order(scout: UnitId, goal: TilePos, queue: bool) -> Command {
    Command::Scout {
        units: vec![scout],
        goal,
        queue,
    }
}

/// Sends `scout` to `goal` and plays until it idles, checking every tick's
/// state and collecting the events.
fn scout_until_idle(state: &mut State, scout: UnitId, goal: TilePos, ticks: u64) -> Vec<Event> {
    let mut events = state
        .tick(&[cmd(0, scout_order(scout, goal, false))])
        .events;
    events.extend(run_until(state, ticks, |state, _| {
        state.validate_invariants().unwrap();
        state.unit(scout).unwrap().order == Order::Idle
    }));
    events
}

fn stalls(events: &[Event]) -> Vec<StallReason> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::OrderStalled { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
}

fn sees(from: TilePos, target: TilePos) -> bool {
    let vision = UnitKind::Surveyor.stats().vision;
    let (dx, dy) = (from.x - target.x, from.y - target.y);
    dx * dx + dy * dy <= vision * vision
}

/// Squared distance from a tile center to the nearest point of a footprint,
/// in quarter tiles so it stays exact.
fn footprint_distance_sq(tile: TilePos, anchor: TilePos, size: (i32, i32)) -> i32 {
    let (cx, cy) = (tile.x * 2 + 1, tile.y * 2 + 1);
    let clamp = |v: i32, low: i32, high: i32| v.clamp(low * 2, high * 2);
    let dx = cx - clamp(cx, anchor.x, anchor.x + size.0);
    let dy = cy - clamp(cy, anchor.y, anchor.y + size.1);
    dx * dx + dy * dy
}

fn outside_reach(tile: TilePos, anchor: TilePos, size: (i32, i32), reach: i32) -> bool {
    footprint_distance_sq(tile, anchor, size) > 4 * reach * reach
}

#[test]
fn a_scout_goes_all_the_way_to_a_safe_tile() {
    let mut state = field(vec![unit(0, UnitKind::Surveyor, 4, 6)], Vec::new())
        .build()
        .unwrap();
    let scout = state.units()[0].id;
    let goal = TilePos::new(30, 15);
    let events = scout_until_idle(&mut state, scout, goal, 400);
    assert_eq!(state.unit(scout).unwrap().tile(), goal);
    assert!(stalls(&events).is_empty(), "{:?}", stalls(&events));
}

#[test]
fn a_known_turret_stops_the_scout_where_it_still_sees_the_target() {
    let turret = TilePos::new(24, 8);
    let mut state = field(
        vec![unit(0, UnitKind::Surveyor, 16, 8)],
        vec![building(1, BuildingKind::Turret, turret.x, turret.y)],
    )
    .build()
    .unwrap();
    let scout = state.units()[0].id;
    let goal = TilePos::new(25, 8);
    let mut events = state
        .tick(&[cmd(0, scout_order(scout, goal, false))])
        .events;
    // The follower may cut a corner of the planned detour into the margin
    // around the gun, but never into its range.
    let range = 5;
    events.extend(run_until(&mut state, 400, |state, _| {
        let tile = state.unit(scout).unwrap().tile();
        assert!(
            outside_reach(tile, turret, (1, 1), range),
            "the scout came within the turret's range at {tile}"
        );
        state.unit(scout).unwrap().order == Order::Idle
    }));
    let unit = state.unit(scout).unwrap();
    assert!(sees(unit.tile(), goal), "it stopped where it sees {goal}");
    assert_eq!(unit.hp, UnitKind::Surveyor.stats().max_hp);
    assert_eq!(stalls(&events), [StallReason::SafeVantage]);
}

/// A Bastion whose known reach covers every tile that sees the tile beside
/// it, kept in sight by a Kestrel it cannot shoot.
fn bastion_field(scout: (i32, i32)) -> (State, UnitId, TilePos) {
    let state = field(
        vec![
            unit(0, UnitKind::Surveyor, scout.0, scout.1),
            unit(0, UnitKind::Kestrel, 24, 2),
        ],
        vec![building(1, BuildingKind::Bastion, 24, 8)],
    )
    .build()
    .unwrap();
    let id = state.units()[0].id;
    (state, id, TilePos::new(23, 9))
}

#[test]
fn a_scout_with_no_safe_view_waits_then_gives_the_leg_up() {
    let (mut state, scout, unsafe_goal) = bastion_field((8, 9));
    let fallback = TilePos::new(8, 16);
    let mut events = state
        .tick(&[
            cmd(0, scout_order(scout, unsafe_goal, false)),
            cmd(0, scout_order(scout, fallback, true)),
        ])
        .events;
    events.extend(run_until(
        &mut state,
        u64::from(SCOUT_HOLD_TICKS) + 300,
        |state, _| {
            state.validate_invariants().unwrap();
            state.unit(scout).unwrap().order == Order::Idle
        },
    ));
    let reasons = stalls(&events);
    assert_eq!(
        reasons.first(),
        Some(&StallReason::DangerHold),
        "the wait is reported as it starts: {reasons:?}"
    );
    assert!(reasons.contains(&StallReason::NoSafeVantage), "{reasons:?}");
    assert_eq!(state.unit(scout).unwrap().tile(), fallback);
}

#[test]
fn a_patrolling_scout_skips_an_unsafe_waypoint_silently() {
    let (mut state, scout, unsafe_goal) = bastion_field((8, 9));
    let safe = TilePos::new(8, 16);
    let mut events = state
        .tick(&[cmd(
            0,
            Command::Patrol {
                units: vec![scout],
                waypoints: vec![unsafe_goal, safe],
            },
        )])
        .events;
    assert!(matches!(
        state.unit(scout).unwrap().order,
        Order::Scout { tile } if tile == unsafe_goal
    ));
    for _ in 0..3 * (u64::from(SCOUT_HOLD_TICKS) + 100) {
        events.extend(state.tick(&[]).events);
    }
    assert!(stalls(&events).is_empty(), "{:?}", stalls(&events));
    assert!(state.unit(scout).unwrap().looping);
}

#[test]
fn a_scout_backs_out_of_a_visible_enemys_reach_and_goes_around_it() {
    // Six tiles from an idle Sentinel: inside the reach a scout keeps clear
    // of (its range plus a margin), outside the Sentinel's aggro.
    let sentinel_at = TilePos::new(16, 10);
    let mut state = field(
        vec![
            unit(0, UnitKind::Surveyor, 10, 10),
            unit(1, UnitKind::Sentinel, sentinel_at.x, sentinel_at.y),
        ],
        Vec::new(),
    )
    .build()
    .unwrap();
    let scout = state.units()[0].id;
    let goal = TilePos::new(30, 10);
    let start = state.unit(scout).unwrap().tile();
    let mut events = state
        .tick(&[cmd(0, scout_order(scout, goal, false))])
        .events;
    let first = state.unit(scout).unwrap().path.clone().expect("a retreat");
    assert!(
        first.waypoints.iter().all(|tile| {
            let (dx, dy) = (tile.x - sentinel_at.x, tile.y - sentinel_at.y);
            let (sx, sy) = (start.x - sentinel_at.x, start.y - sentinel_at.y);
            dx * dx + dy * dy >= sx * sx + sy * sy
        }),
        "a retreat never closes on the enemy: {first:?}"
    );
    events.extend(run_until(&mut state, 600, |state, _| {
        state.validate_invariants().unwrap();
        state.unit(scout).unwrap().order == Order::Idle
    }));
    let unit = state.unit(scout).unwrap();
    assert_eq!(unit.hp, UnitKind::Surveyor.stats().max_hp);
    assert_eq!(unit.tile(), goal, "{:?}", stalls(&events));
}

#[test]
fn a_crowd_of_scouts_sent_to_one_tile_all_arrive() {
    let units: Vec<UnitSpec> = (4..10).map(|y| unit(0, UnitKind::Surveyor, 4, y)).collect();
    let mut state = field(units, Vec::new()).build().unwrap();
    let scouts: Vec<UnitId> = state.units().iter().map(|unit| unit.id).collect();
    let goal = TilePos::new(26, 14);
    let mut events = state
        .tick(&[cmd(
            0,
            Command::Scout {
                units: scouts.clone(),
                goal,
                queue: false,
            },
        )])
        .events;
    events.extend(run_until(&mut state, 600, |state, _| {
        scouts
            .iter()
            .all(|id| state.unit(*id).unwrap().order == Order::Idle)
    }));
    assert!(stalls(&events).is_empty(), "{:?}", stalls(&events));
    for id in scouts {
        assert!(sees(state.unit(id).unwrap().tile(), goal));
    }
}

/// A charge on the straight line from the scout to its goal, within the
/// scout's detection radius from the start.
fn charge_lane() -> (State, UnitId, TilePos) {
    let state = field(
        vec![unit(0, UnitKind::Surveyor, 12, 10)],
        vec![building(1, BuildingKind::ScuttleCharge, 16, 10)],
    )
    .build()
    .unwrap();
    let id = state.units()[0].id;
    (state, id, TilePos::new(26, 10))
}

#[test]
fn a_scout_walks_around_a_charge_it_detected() {
    let (mut state, scout, goal) = charge_lane();
    assert!(
        state
            .vision(PlayerId(0))
            .ghosts()
            .iter()
            .any(|ghost| ghost.kind == BuildingKind::ScuttleCharge),
        "test premise: the scout detects the charge before it moves"
    );
    let events = scout_until_idle(&mut state, scout, goal, 400);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ChargeDetonated { .. }))
    );
    assert_eq!(state.unit(scout).unwrap().tile(), goal);
}

#[test]
fn a_plain_run_ignores_the_scouts_caution() {
    let (mut state, scout, goal) = charge_lane();
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![scout],
            goal,
            queue: false,
        },
    )]);
    let events = run_until(&mut state, 400, |state, _| state.unit(scout).is_none());
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ChargeDetonated { .. }))
    );
}

/// The scout's first planned path toward `goal`.
fn first_path(mut state: State, goal: TilePos) -> Vec<TilePos> {
    let scout = state.units()[0].id;
    state.tick(&[cmd(0, scout_order(scout, goal, false))]);
    state.tick(&[]);
    state
        .unit(scout)
        .unwrap()
        .path
        .as_ref()
        .expect("a planned walk")
        .waypoints
        .clone()
}

#[test]
fn only_known_danger_bends_a_scouts_route() {
    let goal = TilePos::new(34, 8);
    let scout = vec![unit(0, UnitKind::Surveyor, 4, 8)];
    let open = field(scout.clone(), Vec::new()).build().unwrap();
    let hidden = field(
        scout.clone(),
        vec![building(1, BuildingKind::Turret, 20, 8)],
    )
    .build()
    .unwrap();
    assert!(hidden.vision(PlayerId(0)).ghosts().is_empty());
    assert_eq!(
        first_path(hidden, goal),
        first_path(open.clone(), goal),
        "a gun the team never saw cannot steer the scout"
    );

    // The same gun seen once and since gone: its ghost bends the plan until
    // the scout sees the empty ground.
    let mut remembered = field(
        vec![unit(0, UnitKind::Surveyor, 13, 8)],
        vec![building(1, BuildingKind::Turret, 20, 8)],
    )
    .build()
    .unwrap();
    assert!(!remembered.vision(PlayerId(0)).ghosts().is_empty());
    let mut data = serde_json::to_value(&remembered).unwrap();
    data["buildings"]
        .as_array_mut()
        .unwrap()
        .retain(|b| b["kind"] != serde_json::json!("turret"));
    remembered = serde_json::from_value(data).unwrap();
    let scout = remembered.units()[0].id;
    let path = first_path(remembered.clone(), goal);
    assert!(
        path.iter()
            .all(|tile| outside_reach(*tile, TilePos::new(20, 8), (1, 1), 6)),
        "the remembered gun bends the first plan: {path:?}"
    );
    scout_until_idle(&mut remembered, scout, goal, 600);
    assert_eq!(remembered.unit(scout).unwrap().tile(), goal);
}

#[test]
fn mirrored_scouts_stop_on_mirrored_tiles() {
    let (width, height) = (
        i32::try_from(WIDTH).unwrap(),
        i32::try_from(HEIGHT).unwrap(),
    );
    let mirror = |(x, y): (i32, i32)| (width - 1 - x, height - 1 - y);
    let settle = |scout: (i32, i32), turret: (i32, i32), goal: (i32, i32)| {
        let mut state = field(
            vec![unit(0, UnitKind::Surveyor, scout.0, scout.1)],
            vec![building(1, BuildingKind::Turret, turret.0, turret.1)],
        )
        .build()
        .unwrap();
        let id = state.units()[0].id;
        scout_until_idle(&mut state, id, TilePos::new(goal.0, goal.1), 400);
        let tile = state.unit(id).unwrap().tile();
        (tile.x, tile.y)
    };
    let (scout, turret, goal) = ((12, 9), (20, 9), (21, 9));
    let west = settle(scout, turret, goal);
    let east = settle(mirror(scout), mirror(turret), mirror(goal));
    assert_eq!(mirror(west), east);
}

#[test]
fn only_scouts_take_the_order_and_refusals_change_nothing() {
    let mut state = field(
        vec![
            unit(0, UnitKind::Sentinel, 4, 6),
            unit(0, UnitKind::Surveyor, 4, 8),
        ],
        Vec::new(),
    )
    .build()
    .unwrap();
    let (sentinel, scout) = (state.units()[0].id, state.units()[1].id);
    let goal = TilePos::new(20, 8);
    let refused = |state: &mut State, command: Command, reason: RejectReason| {
        let mut probe = state.clone();
        let report = probe.tick(&[cmd(0, command)]);
        assert!(
            report.events.iter().any(
                |event| matches!(event, Event::CommandRejected { reason: r, .. } if *r == reason)
            ),
            "{reason:?}: {:?}",
            report.events
        );
        let mut idle = state.clone();
        idle.tick(&[]);
        assert_eq!(probe.hash(), idle.hash(), "{reason:?} changed the world");
    };
    refused(
        &mut state,
        Command::Scout {
            units: vec![sentinel],
            goal,
            queue: false,
        },
        RejectReason::NoValidUnits,
    );
    refused(
        &mut state,
        scout_order(scout, TilePos::new(-1, 8), false),
        RejectReason::OutOfBounds,
    );
    state.tick(&[cmd(
        0,
        Command::Scout {
            units: vec![sentinel, scout],
            goal,
            queue: false,
        },
    )]);
    assert_eq!(
        state.unit(scout).unwrap().order,
        Order::Scout { tile: goal }
    );
    assert_eq!(state.unit(sentinel).unwrap().order, Order::Idle);
}

#[test]
fn a_queued_scout_leg_cancels_like_a_walk() {
    let mut state = field(vec![unit(0, UnitKind::Surveyor, 4, 8)], Vec::new())
        .build()
        .unwrap();
    let scout = state.units()[0].id;
    let (first, second) = (TilePos::new(20, 8), TilePos::new(20, 16));
    state.tick(&[
        cmd(0, scout_order(scout, first, false)),
        cmd(0, scout_order(scout, second, true)),
    ]);
    state.tick(&[cmd(
        0,
        Command::CancelOrder {
            unit: scout,
            key: OrderKey::Walk { tile: second },
            from_end: 0,
            units: Vec::new(),
        },
    )]);
    let unit = state.unit(scout).unwrap();
    assert_eq!(unit.order, Order::Scout { tile: first });
    assert!(unit.queue.is_empty());
}

#[test]
fn a_scout_that_arrives_in_danger_backs_off_instead_of_finishing() {
    // The scout already stands on its goal, two tiles from a charge it
    // detects: inside the berth a scout keeps, outside the trigger.
    let charge = TilePos::new(12, 10);
    let mut state = field(
        vec![unit(0, UnitKind::Surveyor, 10, 10)],
        vec![building(1, BuildingKind::ScuttleCharge, charge.x, charge.y)],
    )
    .build()
    .unwrap();
    let scout = state.units()[0].id;
    let goal = state.unit(scout).unwrap().tile();
    let events = scout_until_idle(&mut state, scout, goal, 400);
    let unit = state.unit(scout).unwrap();
    assert!(
        unit.tile().chebyshev(charge) > oxide_sim::stats::SCOUT_CHARGE_AVOID_RADIUS,
        "it left the dangerous goal: {:?}",
        unit.tile()
    );
    assert!(sees(unit.tile(), goal), "and still watches it");
    assert_eq!(stalls(&events), [StallReason::SafeVantage]);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ChargeDetonated { .. }))
    );
}
