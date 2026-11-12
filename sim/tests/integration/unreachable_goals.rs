//! Walks whose goal cannot be reached end as close as they can get, report
//! it once, and let the rest of the program run. Crowds settling at such an
//! endpoint never count as unreachable, and reachable walks keep their
//! ordinary arrival. One table pins what an order that finds no route
//! leaves of a looping program, and whether it says so: walks, work orders,
//! drop-off runs, ordered attacks, boarding, unloading and retirement. A
//! landing never belongs to a looping program, so the table leaves it out;
//! a walk that turns into a landing has a test of its own.

use crate::common;
use common::{cmd, run_until};

use aftermine_sim::event::StallReason;
use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::stats::BuildingKind;
use aftermine_sim::{Command, Event, Order, PlayerId, Scenario, State, UnitId, UnitKind};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

/// A home shore whose eastern edge recedes at 45 degrees from a convex tip
/// at (14, 5), and an island (columns 21-28) across the rock. The tip is the
/// home tile nearest every island tile, and only a few bodies fit within
/// arrival reach of it, so a parked crowd there reaches well back from it.
const SHORE: [&str; 11] = [
    "##############################",
    "#..........##########........#",
    "#...........#########........#",
    "#............########........#",
    "#.............#######........#",
    "#..............######........#",
    "#.............#######........#",
    "#............########........#",
    "#...........#########........#",
    "#..........##########........#",
    "##############################",
];

const TIP: TilePos = TilePos::new(14, 5);
const ISLAND: TilePos = TilePos::new(24, 5);

fn sandbox(map: &[&str], units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>) -> Scenario {
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Sandbox,
        name: "unreachable-goals".into(),
        map: map.iter().map(|row| (*row).to_owned()).collect(),
        players: vec![PlayerSpec {
            name: "Home".into(),
            team: None,
            scrap: 5_000,
            bot: false,
            bot_config: None,
        }],
        units,
        buildings,
        meta: None,
    }
}

fn unit(kind: UnitKind, x: i32, y: i32) -> UnitSpec {
    UnitSpec {
        player: 0,
        kind,
        x,
        y,
    }
}

fn move_to(units: Vec<UnitId>, goal: TilePos, queue: bool) -> aftermine_sim::PlayerCommand {
    cmd(0, Command::Run { units, goal, queue })
}

/// Every `NoRoute` stall in `events`, by unit, where the unit stood.
fn no_route_positions(events: &[Event]) -> Vec<(UnitId, Vec2Fx)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::OrderStalled {
                unit,
                pos,
                reason: StallReason::NoRoute,
                ..
            } => Some((*unit, *pos)),
            _ => None,
        })
        .collect()
}

/// Every `NoRoute` stall in `events`, by unit, with the tile it stood on.
fn no_routes(events: &[Event]) -> Vec<(UnitId, TilePos)> {
    no_route_positions(events)
        .into_iter()
        .map(|(unit, pos)| (unit, TilePos::containing(pos)))
        .collect()
}

/// Asserts each of `units` reported its shortfall exactly once, strictly
/// nearer the tip than where it set out.
fn each_reported_once_nearer_the_tip(events: &[Event], units: &[(UnitId, TilePos)], label: &str) {
    let reports = no_routes(events);
    assert_eq!(reports.len(), units.len(), "{label}");
    for &(id, start) in units {
        let mine: Vec<TilePos> = reports
            .iter()
            .filter(|(unit, _)| *unit == id)
            .map(|&(_, tile)| tile)
            .collect();
        assert_eq!(
            mine.len(),
            1,
            "{label}: unit {id:?} reports its shortfall exactly once"
        );
        assert!(
            mine[0].chebyshev(TIP) < start.chebyshev(TIP),
            "{label}: unit {id:?} reported at {:?}, no nearer the tip than {start:?}",
            mine[0]
        );
    }
}

/// Whether some shortfall was reported farther from the tip than the
/// ordinary arrival wave reaches: only a parked crowd reaching back from the
/// tip ends a walk there.
fn a_report_came_from_the_crowd(events: &[Event]) -> bool {
    let beyond = Fx::lit("2.5");
    no_route_positions(events)
        .iter()
        .any(|(_, pos)| pos.dist(TIP.center()) > beyond)
}

fn stalls(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Event::OrderStalled { .. }))
        .count()
}

fn idle(state: &State, ids: &[UnitId]) -> bool {
    ids.iter()
        .all(|id| state.unit(*id).is_some_and(|u| u.order == Order::Idle))
}

#[test]
fn an_island_move_stops_at_the_nearest_shore_then_the_next_leg_runs() {
    let mut state = sandbox(&SHORE, vec![unit(UnitKind::Sentinel, 2, 2)], vec![])
        .build()
        .unwrap();
    let walker = state.units()[0].id;
    let home = TilePos::new(3, 8);
    let mut events = state
        .tick(&[
            move_to(vec![walker], ISLAND, false),
            move_to(vec![walker], home, true),
        ])
        .events;
    events.extend(run_until(&mut state, 800, |state, _| {
        let u = state.unit(walker).unwrap();
        u.order == Order::Idle && u.tile() == home
    }));
    assert_eq!(
        no_routes(&events),
        vec![(walker, TIP)],
        "one report, from the nearest shore"
    );
    assert!(state.unit(walker).unwrap().queue.is_empty());
}

/// Sentinels eight abreast on the home shore, filling rows from `top` down.
fn crowd(count: i32, top: i32) -> Vec<UnitSpec> {
    (0..count)
        .map(|i| unit(UnitKind::Sentinel, 2 + i % 8, top + i / 8))
        .collect()
}

/// Orders every unit to the island, as one group or one Run each.
fn to_the_island(ids: &[UnitId], group: bool) -> Vec<aftermine_sim::PlayerCommand> {
    if group {
        vec![move_to(ids.to_vec(), ISLAND, false)]
    } else {
        ids.iter()
            .map(|id| move_to(vec![*id], ISLAND, false))
            .collect()
    }
}

#[test]
fn a_crowd_sent_to_an_island_settles_at_the_tip_and_carries_on() {
    for group in [true, false] {
        for follow_up in [false, true] {
            let mut state = sandbox(&SHORE, crowd(24, 2), vec![]).build().unwrap();
            let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
            let homes: Vec<TilePos> = state
                .units()
                .iter()
                .map(aftermine_sim::Unit::tile)
                .collect();
            let mut commands = to_the_island(&ids, group);
            if follow_up {
                // Distinct reachable tiles, so the follow-up's ordinary
                // arrival is not what is under test.
                commands.extend(
                    ids.iter()
                        .zip(&homes)
                        .map(|(id, home)| move_to(vec![*id], *home, true)),
                );
            }
            let label = format!("group {group}, follow-up {follow_up}");
            let mut events = state.tick(&commands).events;
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Event::CommandRejected { .. })),
                "{label}: every command lands"
            );
            // The follow-up legs end by the ordinary arrival rule, which
            // lets a crowd park beside its tiles, so the test only asks that
            // everyone came back from the tip.
            events.extend(run_until(&mut state, 600, |state, _| {
                idle(state, &ids)
                    && (!follow_up || ids.iter().all(|id| state.unit(*id).unwrap().tile().x < 11))
            }));
            let starts: Vec<(UnitId, TilePos)> = ids.iter().copied().zip(homes).collect();
            each_reported_once_nearer_the_tip(&events, &starts, &label);
            // A unit with a follow-up leaves the tip at once, so no crowd
            // parks there and each walker reaches the tip itself.
            if !follow_up {
                assert!(a_report_came_from_the_crowd(&events), "{label}");
            }
        }
    }
}

#[test]
fn a_large_crowd_straddling_the_target_row_settles_promptly() {
    // Rows 1-8 lie on both sides of the target's row, so the units' endpoint
    // scans run in both half-turn frames.
    for group in [true, false] {
        let label = format!("group {group}");
        let mut state = sandbox(&SHORE, crowd(64, 1), vec![]).build().unwrap();
        let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
        let starts: Vec<(UnitId, TilePos)> =
            state.units().iter().map(|u| (u.id, u.tile())).collect();
        let mut events = state.tick(&to_the_island(&ids, group)).events;
        // Measured at 63 ticks; without the crowd reaching back from the tip,
        // the last bodies only settle after about 110.
        events.extend(run_until(&mut state, 85, |state, _| idle(state, &ids)));
        each_reported_once_nearer_the_tip(&events, &starts, &label);
        assert!(a_report_came_from_the_crowd(&events), "{label}");
    }
}

/// A walled enclosure around (20, 5). Every spread slot of a small group
/// sent to its centre lies inside, and several tiles outside the wall tie
/// for nearest to many of them.
const ENCLOSURE: [&str; 11] = [
    "##############################",
    "#............................#",
    "#................#######.....#",
    "#................#.....#.....#",
    "#................#.....#.....#",
    "#................#.....#.....#",
    "#................#.....#.....#",
    "#................#.....#.....#",
    "#................#######.....#",
    "#............................#",
    "##############################",
];

#[test]
fn mirrored_groups_sent_into_mirrored_enclosures_settle_symmetrically() {
    // The enclosure above and its half-turn below. Seat 0's order is listed
    // first in the tick.
    let map: Vec<String> = ENCLOSURE
        .iter()
        .map(|row| (*row).to_owned())
        .chain(
            ENCLOSURE
                .iter()
                .rev()
                .map(|row| row.chars().rev().collect()),
        )
        .collect();
    let (width, height) = (
        i32::try_from(ENCLOSURE[0].len()).unwrap(),
        i32::try_from(map.len()).unwrap(),
    );
    let inside = TilePos::new(20, 5);
    let mirror = |tile: TilePos| TilePos::new(width - 1 - tile.x, height - 1 - tile.y);
    let mirror_pos =
        |pos: Vec2Fx| Vec2Fx::new(Fx::from_num(width) - pos.x, Fx::from_num(height) - pos.y);
    let seat = |player: u8| PlayerSpec {
        name: format!("Seat {player}"),
        team: None,
        scrap: 0,
        bot: false,
        bot_config: None,
    };
    let mut units = Vec::new();
    for i in 0..8 {
        let home = TilePos::new(2 + i % 4, 2 + i / 4);
        let away = mirror(home);
        units.push(unit(UnitKind::Sentinel, home.x, home.y));
        units.push(UnitSpec {
            player: 1,
            ..unit(UnitKind::Sentinel, away.x, away.y)
        });
    }
    let mut state = Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Sandbox,
        name: "mirrored-islands".into(),
        map,
        players: vec![seat(0), seat(1)],
        units,
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap();
    let pairs: Vec<(UnitId, UnitId)> = state
        .units()
        .chunks(2)
        .map(|pair| (pair[0].id, pair[1].id))
        .collect();
    let (left, right): (Vec<UnitId>, Vec<UnitId>) = pairs.iter().copied().unzip();
    let symmetric = |state: &State, stage: &str| {
        for &(l, r) in &pairs {
            let (a, b) = (state.unit(l).unwrap(), state.unit(r).unwrap());
            assert_eq!(
                mirror_pos(a.pos),
                b.pos,
                "{stage}: {l:?} and {r:?} stand mirrored"
            );
            assert_eq!(a.path.is_some(), b.path.is_some(), "{stage}: {l:?} routes");
            match (a.order, b.order) {
                (Order::Run { goal: ga }, Order::Run { goal: gb }) => {
                    assert_eq!(mirror(ga.tile()), gb.tile(), "{stage}: {l:?} goal");
                    assert_eq!(
                        ga.endpoint.map(mirror),
                        gb.endpoint,
                        "{stage}: {l:?} endpoint"
                    );
                }
                (Order::Idle, Order::Idle) => {}
                orders => panic!("{stage}: {l:?} orders are not paired: {orders:?}"),
            }
        }
    };
    let mut events = state
        .tick(&[
            cmd(
                0,
                Command::Run {
                    units: left.clone(),
                    goal: inside,
                    queue: false,
                },
            ),
            cmd(
                1,
                Command::Run {
                    units: right.clone(),
                    goal: mirror(inside),
                    queue: false,
                },
            ),
        ])
        .events;
    let mut endpoints = std::collections::BTreeSet::new();
    for tick in 1..=400 {
        if tick > 1 {
            events.extend(state.tick(&[]).events);
        }
        symmetric(&state, &format!("tick {tick}"));
        endpoints.extend(
            left.iter()
                .filter_map(|id| match state.unit(*id).unwrap().order {
                    Order::Run { goal } => goal.endpoint.map(|tile| (tile.y, tile.x)),
                    _ => None,
                }),
        );
        if idle(&state, &left) && idle(&state, &right) {
            break;
        }
    }
    assert!(
        idle(&state, &left) && idle(&state, &right),
        "both groups settle"
    );
    assert!(
        endpoints.len() > 1,
        "premise: the group settles for several tiles around the wall: {endpoints:?}"
    );
    let reports = no_route_positions(&events);
    for &(l, r) in &pairs {
        let at = |id: UnitId| -> Vec<Vec2Fx> {
            reports
                .iter()
                .filter(|(unit, _)| *unit == id)
                .map(|&(_, pos)| pos)
                .collect()
        };
        let (a, b) = (at(l), at(r));
        assert_eq!(a.len(), 1, "{l:?} reports its shortfall once");
        assert_eq!(
            a.iter().copied().map(mirror_pos).collect::<Vec<_>>(),
            b,
            "{r:?} mirrors {l:?}"
        );
    }
}

#[test]
fn newborns_rallied_onto_unreachable_ground_all_settle() {
    let mut map = SHORE.map(str::to_owned);
    map[2].replace_range(2..3, "1");
    let map: Vec<&str> = map.iter().map(String::as_str).collect();
    let mut state = sandbox(
        &map,
        vec![],
        vec![
            BuildingSpec {
                player: 0,
                kind: BuildingKind::Foundry,
                x: 2,
                y: 6,
            },
            BuildingSpec {
                player: 0,
                kind: BuildingKind::Foundry,
                x: 7,
                y: 2,
            },
        ],
    )
    .build()
    .unwrap();
    let foundries: Vec<_> = state.buildings().iter().map(|b| b.id).collect();
    assert_eq!(foundries.len(), 3);
    let rallies: Vec<_> = foundries
        .iter()
        .map(|&building| {
            cmd(
                0,
                Command::SetRally {
                    building,
                    rally: Some(aftermine_sim::Rally::Tile(ISLAND)),
                },
            )
        })
        .collect();
    let mut events = state.tick(&rallies).events;
    let mut ordered = 0;
    let mut spawns = Vec::new();
    for _ in 0..3_000 {
        let mut commands = Vec::new();
        for &building in &foundries {
            if ordered < 21 && state.building(building).unwrap().queue.is_empty() {
                ordered += 1;
                commands.push(cmd(
                    0,
                    Command::Train {
                        building,
                        kind: UnitKind::Scuttler,
                    },
                ));
            }
        }
        let report = state.tick(&commands);
        spawns.extend(report.events.iter().filter_map(|event| match event {
            Event::UnitTrained { unit, .. } => Some((*unit, state.unit(*unit).unwrap().tile())),
            _ => None,
        }));
        events.extend(report.events);
        let newborns: Vec<UnitId> = spawns.iter().map(|&(unit, _)| unit).collect();
        if newborns.len() == 21 && idle(&state, &newborns) {
            break;
        }
    }
    assert_eq!(spawns.len(), 21, "every ordered unit was trained");
    let newborns: Vec<UnitId> = spawns.iter().map(|&(unit, _)| unit).collect();
    assert!(idle(&state, &newborns), "every newborn settled");
    each_reported_once_nearer_the_tip(&events, &spawns, "rally");
    for &(_, tile) in &no_routes(&events) {
        assert!(
            tile.chebyshev(TIP) <= 6,
            "a newborn settled near the tip, not at {tile:?}"
        );
    }
    for id in &newborns {
        assert!(
            state.unit(*id).unwrap().tile().x <= TIP.x,
            "nothing crossed the rock"
        );
    }
}

#[test]
fn a_group_sent_to_open_ground_settles_without_stalling() {
    let mut state = sandbox(
        &[
            "########################",
            "#......................#",
            "#......................#",
            "#......................#",
            "#......................#",
            "#......................#",
            "#......................#",
            "#......................#",
            "#......................#",
            "########################",
        ],
        (0..16)
            .map(|i| unit(UnitKind::Sentinel, 2 + i % 4, 2 + i / 4))
            .collect(),
        vec![],
    )
    .build()
    .unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let mut events = state
        .tick(&[move_to(ids.clone(), TilePos::new(17, 5), false)])
        .events;
    events.extend(run_until(&mut state, 600, |state, _| idle(state, &ids)));
    assert_eq!(stalls(&events), 0, "a reachable crowd never reports");
    for id in &ids {
        let unit = state.unit(*id).unwrap();
        assert!(unit.tile().x >= 12, "{id:?} arrived: {:?}", unit.tile());
    }
}

#[test]
fn a_barricade_sealing_the_goal_mid_walk_ends_the_walk_short() {
    let mut state = sandbox(
        &[
            "####################",
            "#........#.........#",
            "#........#.........#",
            "#..................#",
            "#........#.........#",
            "#........#.........#",
            "####################",
        ],
        vec![
            unit(UnitKind::Sentinel, 1, 1),
            unit(UnitKind::Harvester, 8, 2),
        ],
        vec![],
    )
    .build()
    .unwrap();
    let (walker, builder) = (state.units()[0].id, state.units()[1].id);
    let goal = TilePos::new(16, 5);
    let mut events = state.tick(&[move_to(vec![walker], goal, false)]).events;
    assert!(
        state.unit(walker).unwrap().path.is_some(),
        "premise: the goal is reachable at first"
    );
    events.extend(
        state
            .tick(&[cmd(
                0,
                Command::Build {
                    units: vec![builder],
                    kind: BuildingKind::Barricade,
                    anchor: TilePos::new(9, 3),
                    queue: false,
                    defer: false,
                },
            )])
            .events,
    );
    assert!(
        !state.passable(TilePos::new(9, 3)),
        "premise: the site closes the only gap"
    );
    events.extend(run_until(&mut state, 600, |state, _| {
        state.unit(walker).unwrap().order == Order::Idle
    }));
    let reports: Vec<_> = no_routes(&events)
        .into_iter()
        .filter(|(unit, _)| *unit == walker)
        .collect();
    assert_eq!(
        reports,
        vec![(walker, TilePos::new(8, 5))],
        "the walk ends on the near shore nearest its goal"
    );
}

#[test]
fn a_patrol_with_one_unreachable_leg_keeps_looping_silently() {
    let mut state = sandbox(&SHORE, vec![unit(UnitKind::Sentinel, 3, 2)], vec![])
        .build()
        .unwrap();
    let guard = state.units()[0].id;
    let (a, c) = (TilePos::new(3, 2), TilePos::new(8, 8));
    let mut events = state
        .tick(&[cmd(
            0,
            Command::Patrol {
                units: vec![guard],
                waypoints: vec![a, ISLAND, c],
            },
        )])
        .events;
    let mut visits = Vec::new();
    for _ in 0..2_400 {
        let report = state.tick(&[]);
        events.extend(report.events);
        let tile = state.unit(guard).unwrap().tile();
        for stop in [a, TIP, c] {
            if tile == stop && visits.last() != Some(&stop) {
                visits.push(stop);
            }
        }
    }
    assert_eq!(stalls(&events), 0, "patrol laps stay silent");
    assert!(state.unit(guard).unwrap().looping);
    let laps = visits.windows(3).filter(|w| w == &[TIP, c, a]).count();
    assert!(laps >= 2, "the patrol keeps cycling: {visits:?}");
}

#[test]
fn a_patrol_with_every_leg_unreachable_holds_still_silently() {
    let mut state = sandbox(&SHORE, vec![unit(UnitKind::Sentinel, 3, 5)], vec![])
        .build()
        .unwrap();
    let guard = state.units()[0].id;
    let mut events = state
        .tick(&[cmd(
            0,
            Command::Patrol {
                units: vec![guard],
                waypoints: vec![
                    TilePos::new(22, 5),
                    TilePos::new(24, 3),
                    TilePos::new(24, 7),
                ],
            },
        )])
        .events;
    events.extend(run_until(&mut state, 400, |state, _| {
        state.unit(guard).unwrap().tile() == TIP
    }));
    let parked = state.unit(guard).unwrap().pos;
    for _ in 0..300 {
        events.extend(state.tick(&[]).events);
        assert_eq!(state.unit(guard).unwrap().tile(), TIP);
    }
    assert!(state.unit(guard).unwrap().looping);
    assert!(
        state.unit(guard).unwrap().pos.dist(parked) < chassis::fx::Fx::lit("0.5"),
        "the guard holds still"
    );
    assert_eq!(stalls(&events), 0, "an unreachable circuit stays silent");
}

#[test]
fn a_repair_that_cannot_be_reached_advances_to_a_queued_move() {
    let state = sandbox(
        &SHORE,
        vec![unit(UnitKind::Harvester, 3, 3)],
        vec![BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x: 24,
            y: 5,
        }],
    )
    .build()
    .unwrap();
    // Wound the stranded turret so a weld is a legal order.
    let mut doc = serde_json::to_value(&state).unwrap();
    doc["buildings"][0]["hp"] = serde_json::json!(100);
    let mut state: State = serde_json::from_value(doc).unwrap();
    let (welder, turret) = (state.units()[0].id, state.buildings()[0].id);
    let home = TilePos::new(6, 8);
    let mut events = state
        .tick(&[
            cmd(
                0,
                Command::Repair {
                    units: vec![welder],
                    building: turret,
                    queue: false,
                },
            ),
            move_to(vec![welder], home, true),
        ])
        .events;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::CommandRejected { .. }))
    );
    events.extend(run_until(&mut state, 400, |state, _| {
        let u = state.unit(welder).unwrap();
        u.order == Order::Idle && u.tile() == home
    }));
    assert_eq!(
        no_routes(&events).len(),
        1,
        "the unreachable weld reports once and yields to the queued walk"
    );
}

/// Home ground and far ground on either side of an unbroken rock wall, with
/// a scrap node on the far side, a ring of peaks there whose pocket even
/// aircraft cannot reach, and a one-tile cell walled off from home ground
/// against the wall.
const SPLIT: [&str; 12] = [
    "##############################",
    "#.........#..................#",
    "#.........#..................#",
    "#.........#..................#",
    "#.........#..................#",
    "#.........#..................#",
    "#.........#.............s....#",
    "#.........#..................#",
    "#.......###.........^^^......#",
    "#.......#.#.........^.^......#",
    "#.......###.........^^^......#",
    "##############################",
];

/// Where every staged unit starts on home ground.
const NEAR: (i32, i32) = (7, 4);
/// The walk home that follows the stalled order round the loop.
const HOME: TilePos = TilePos::new(4, 6);
const FAR: TilePos = TilePos::new(16, 4);
const NODE: TilePos = TilePos::new(24, 6);
const POCKET: TilePos = TilePos::new(21, 9);
const CELL: TilePos = TilePos::new(9, 9);

/// What a stalled order leaves of a looping program whose next order is the
/// walk home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Left {
    /// The order goes to the back of the loop and the walk home runs.
    Rotated,
    /// The order is gone and the walk home runs.
    Dropped,
    /// The whole program is gone.
    Cleared,
}

/// SPLIT in a sandbox for Home and an enemy, Home's Foundry on home ground.
fn split(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>) -> State {
    let mut scenario = sandbox(&SPLIT, units, buildings);
    scenario.players.push(PlayerSpec {
        name: "Far".into(),
        team: None,
        scrap: 5_000,
        bot: false,
        bot_config: None,
    });
    scenario
        .buildings
        .push(common::building(0, BuildingKind::Foundry, 1, 1));
    scenario.build().unwrap()
}

fn near(kind: UnitKind) -> UnitSpec {
    unit(kind, NEAR.0, NEAR.1)
}

fn unit_at(state: &State, x: i32, y: i32) -> UnitId {
    let tile = TilePos::new(x, y);
    state
        .units()
        .iter()
        .find(|u| u.tile() == tile)
        .unwrap_or_else(|| panic!("no unit on {tile:?}"))
        .id
}

fn building_at(state: &State, x: i32, y: i32) -> aftermine_sim::BuildingId {
    let tile = TilePos::new(x, y);
    state
        .buildings()
        .iter()
        .find(|b| b.anchor == tile)
        .unwrap_or_else(|| panic!("no building on {tile:?}"))
        .id
}

/// `state` with the serialized unit `id` edited by `edit`.
fn edited(state: &State, id: UnitId, edit: impl FnOnce(&mut serde_json::Value)) -> State {
    let mut doc = serde_json::to_value(state).unwrap();
    let row = doc["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == serde_json::json!(id))
        .unwrap();
    edit(row);
    serde_json::from_value(doc).unwrap()
}

/// The order `command` gives `unit`, read from the command phase.
fn ordered(state: &State, unit: UnitId, command: Command) -> Order {
    state.inspect_command_phase(&[cmd(0, command)], |view| view.unit(unit).unwrap().order)
}

/// An order to attack `target` as the player gives it.
fn attack(state: &State, unit: UnitId, target: aftermine_sim::Target) -> Order {
    let order = ordered(
        state,
        unit,
        Command::Attack {
            units: vec![unit],
            target: target.into(),
            queue: false,
        },
    );
    assert!(matches!(order, Order::Attack { .. }), "premise: {order:?}");
    order
}

/// Gives `unit` the loop of `order` then the walk home, ticks until `order`
/// ends, and returns what is left of the loop and whether the unit
/// reported `NoRoute`.
fn left_of_the_loop(state: &State, unit: UnitId, order: Order) -> (Left, bool) {
    let home = Order::Run {
        goal: aftermine_sim::Goal::at(HOME),
    };
    let mut state = edited(state, unit, |row| {
        row["order"] = serde_json::to_value(order).unwrap();
        row["queue"] = serde_json::json!([home]);
        row["looping"] = serde_json::json!(true);
        row["path"] = serde_json::Value::Null;
    });
    let mut events = Vec::new();
    while common::commanded(state.unit(unit).unwrap().order) == common::commanded(order) {
        assert!(state.current_tick() < 600, "{order:?} never ended");
        events.extend(state.tick(&[]).events);
    }
    let reported = no_routes(&events).iter().any(|(id, _)| *id == unit);
    let u = state.unit(unit).unwrap();
    let queue: Vec<Order> = u.queue.iter().copied().map(common::commanded).collect();
    let left = if u.order == Order::Idle && queue.is_empty() && !u.looping {
        Left::Cleared
    } else if common::commanded(u.order) == home && queue == [common::commanded(order)] {
        Left::Rotated
    } else if common::commanded(u.order) == home && queue.is_empty() {
        Left::Dropped
    } else {
        panic!("{order:?} left {:?} then {:?}", u.order, u.queue);
    };
    (left, reported)
}

fn joining_a_far_site() -> (State, UnitId, Order) {
    let mut state = split(
        vec![near(UnitKind::Harvester), unit(UnitKind::Harvester, 13, 2)],
        vec![],
    );
    let founder = unit_at(&state, 13, 2);
    let report = state.tick(&[cmd(
        0,
        Command::Build {
            units: vec![founder],
            kind: BuildingKind::Turret,
            anchor: TilePos::new(15, 2),
            queue: false,
            defer: false,
        },
    )]);
    assert_eq!(common::rejected(&report.events), []);
    let site = building_at(&state, 15, 2);
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, Order::Build { site })
}

/// `state` once the Kestrel `scout` has flown home, so what it saw on far
/// ground is memory.
fn flown_home(mut state: State, scout: UnitId) -> State {
    let report = state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![scout],
            goal: TilePos::new(2, 10),
            queue: false,
        },
    )]);
    assert_eq!(common::rejected(&report.events), []);
    run_until(&mut state, 600, |state, _| {
        state.unit(scout).unwrap().order == Order::Idle
    });
    state
}

fn founding_a_far_scaffold() -> (State, UnitId, Order) {
    // A Kestrel explores the corner, then leaves it out of sight, so a
    // deferred build there pays for a scaffold that stays provisional.
    let state = split(
        vec![
            near(UnitKind::Harvester),
            unit(UnitKind::Harvester, 12, 6),
            unit(UnitKind::Kestrel, 26, 2),
        ],
        vec![],
    );
    let scout = unit_at(&state, 26, 2);
    let mut state = flown_home(state, scout);
    let (kind, anchor) = (BuildingKind::Turret, TilePos::new(27, 1));
    let report = state.tick(&[cmd(
        0,
        Command::Build {
            units: vec![unit_at(&state, 12, 6)],
            kind,
            anchor,
            queue: false,
            defer: true,
        },
    )]);
    assert_eq!(common::rejected(&report.events), []);
    let scaffold = state.building(building_at(&state, 27, 1)).unwrap();
    assert!(scaffold.provisional(), "premise: the scaffold is unseen");
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, Order::Found { kind, anchor })
}

/// `state` with the building at `x, y` at `hp`.
fn damaged(state: &State, x: i32, y: i32, hp: u32) -> State {
    let id = building_at(state, x, y);
    let mut doc = serde_json::to_value(state).unwrap();
    let row = doc["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == serde_json::json!(id))
        .unwrap();
    row["hp"] = serde_json::json!(hp);
    serde_json::from_value(doc).unwrap()
}

fn welding_a_far_building() -> (State, UnitId, Order) {
    let state = split(
        vec![near(UnitKind::Harvester)],
        vec![common::building(0, BuildingKind::Turret, FAR.x, FAR.y)],
    );
    let state = damaged(&state, FAR.x, FAR.y, 100);
    let building = building_at(&state, FAR.x, FAR.y);
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, Order::Repair { building })
}

fn welding_a_far_machine() -> (State, UnitId, Order) {
    let state = split(
        vec![
            near(UnitKind::Harvester),
            unit(UnitKind::Harvester, FAR.x, FAR.y),
        ],
        vec![],
    );
    let patient = unit_at(&state, FAR.x, FAR.y);
    let state = edited(&state, patient, |row| row["hp"] = serde_json::json!(20));
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, Order::RepairUnit { unit: patient })
}

fn salvaging_a_far_building() -> (State, UnitId, Order) {
    let state = split(
        vec![near(UnitKind::Harvester)],
        vec![common::building(0, BuildingKind::Barricade, FAR.x, FAR.y)],
    );
    let building = building_at(&state, FAR.x, FAR.y);
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, Order::Salvage { building })
}

/// A Harvester beside the far node carrying `load`.
fn far_harvester(load: u32) -> (State, UnitId) {
    let state = split(vec![unit(UnitKind::Harvester, NODE.x - 1, NODE.y)], vec![]);
    let harvester = unit_at(&state, NODE.x - 1, NODE.y);
    let state = edited(&state, harvester, |row| {
        row["worker"]["carrying"] = serde_json::json!(load);
    });
    (state, harvester)
}

fn returning_cargo_from_far_ground() -> (State, UnitId, Order) {
    let (state, harvester) = far_harvester(5);
    let foundry = building_at(&state, 1, 1);
    (
        state,
        harvester,
        Order::ReturnCargo {
            foundry,
            repair: false,
        },
    )
}

fn delivering_a_full_load_from_far_ground() -> (State, UnitId, Order) {
    let capacity = UnitKind::Harvester.stats().harvest.unwrap().capacity;
    let (state, harvester) = far_harvester(capacity);
    (
        state,
        harvester,
        Order::Harvest {
            node: NODE,
            anchor: NODE,
            withdrawing: false,
        },
    )
}

fn withdrawing_from_far_ground() -> (State, UnitId, Order) {
    let (state, harvester) = far_harvester(0);
    (
        state,
        harvester,
        Order::Harvest {
            node: NODE,
            anchor: NODE,
            withdrawing: true,
        },
    )
}

/// `kind` on home ground, a Home Kestrel in sight of an enemy Harvester and
/// an enemy Barricade on far ground.
fn far_enemies(kind: UnitKind) -> State {
    split(
        vec![
            near(kind),
            unit(UnitKind::Kestrel, FAR.x, FAR.y - 2),
            common::unit(1, UnitKind::Harvester, FAR.x, FAR.y + 2),
        ],
        vec![common::building(1, BuildingKind::Barricade, FAR.x, FAR.y)],
    )
}

fn chasing_a_far_machine() -> (State, UnitId, Order) {
    let state = far_enemies(UnitKind::Sentinel);
    let (shooter, victim) = (
        unit_at(&state, NEAR.0, NEAR.1),
        unit_at(&state, FAR.x, FAR.y + 2),
    );
    let order = attack(&state, shooter, aftermine_sim::Target::Unit(victim));
    (state, shooter, order)
}

fn shooting_a_far_building(kind: UnitKind) -> (State, UnitId, Order) {
    let state = far_enemies(kind);
    let shooter = unit_at(&state, NEAR.0, NEAR.1);
    let target = aftermine_sim::Target::Building(building_at(&state, FAR.x, FAR.y));
    let order = attack(&state, shooter, target);
    (state, shooter, order)
}

fn shelling_a_far_memory() -> (State, UnitId, Order) {
    let (state, shooter, order) = shooting_a_far_building(UnitKind::Sentinel);
    let scout = unit_at(&state, FAR.x, FAR.y - 2);
    let state = flown_home(state, scout);
    assert!(
        !state.can_see(PlayerId(0), FAR),
        "premise: only memory holds the Barricade"
    );
    (state, shooter, order)
}

/// `kind` on home ground ordered at an enemy Barricade in the peak pocket,
/// which a Home Kestrel sees from outside the ring.
fn attacking_into_the_pocket(kind: UnitKind) -> (State, UnitId, Order) {
    let state = split(
        vec![near(kind), unit(UnitKind::Kestrel, POCKET.x, POCKET.y - 3)],
        vec![common::building(
            1,
            BuildingKind::Barricade,
            POCKET.x,
            POCKET.y,
        )],
    );
    let shooter = unit_at(&state, NEAR.0, NEAR.1);
    let target = aftermine_sim::Target::Building(building_at(&state, POCKET.x, POCKET.y));
    let order = attack(&state, shooter, target);
    (state, shooter, order)
}

/// An Avalanche shut in the cell, ordered at an enemy charge a Kestrel
/// detected inside the gun's minimum range across the wall: the gun sees
/// the ground but not the charge, and every tile it could fire from lies
/// outside the cell.
fn shelling_a_charge_from_the_dead_zone() -> (State, UnitId, Order) {
    let state = split(
        vec![
            unit(UnitKind::Avalanche, CELL.x, CELL.y),
            unit(UnitKind::Kestrel, 12, 7),
        ],
        vec![common::building(1, BuildingKind::ScuttleCharge, 12, 9)],
    );
    let (gun, charge) = (unit_at(&state, CELL.x, CELL.y), building_at(&state, 12, 9));
    let order = attack(&state, gun, aftermine_sim::Target::Building(charge));
    let scout = unit_at(&state, 12, 7);
    let state = flown_home(state, scout);
    let charge = state.building(charge).unwrap();
    assert!(
        !state.building_apparent(PlayerId(0), charge),
        "premise: only memory holds the charge"
    );
    (state, gun, order)
}

fn walking_to_far_ground() -> (State, UnitId, Order) {
    let state = split(vec![near(UnitKind::Sentinel)], vec![]);
    let walker = unit_at(&state, NEAR.0, NEAR.1);
    let order = Order::Run {
        goal: aftermine_sim::Goal::at(FAR),
    };
    (state, walker, order)
}

fn unloading_in_the_pocket() -> (State, UnitId, Order) {
    let state = split(vec![near(UnitKind::Skyhook)], vec![]);
    let transport = unit_at(&state, NEAR.0, NEAR.1);
    let order = Order::Unload {
        at: aftermine_sim::Goal::at(POCKET),
        reverse: false,
    };
    (state, transport, order)
}

fn boarding_over_far_ground() -> (State, UnitId, Order) {
    let state = split(
        vec![
            near(UnitKind::Sentinel),
            unit(UnitKind::Skyhook, FAR.x, FAR.y),
        ],
        vec![],
    );
    let transport = unit_at(&state, FAR.x, FAR.y);
    let order = Order::Board { transport };
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, order)
}

fn retiring_at_a_far_crucible() -> (State, UnitId, Order) {
    let state = split(
        vec![near(UnitKind::Sentinel)],
        vec![common::building(0, BuildingKind::Crucible, FAR.x, FAR.y)],
    );
    let crucible = building_at(&state, FAR.x, FAR.y);
    let order = Order::Retire {
        crucible,
        pad: None,
    };
    let unit = unit_at(&state, NEAR.0, NEAR.1);
    (state, unit, order)
}

type Scene = fn() -> (State, UnitId, Order);

#[test]
fn what_an_order_with_no_route_leaves_of_its_loop() {
    use Left::{Cleared, Dropped, Rotated};
    let rows: [(&str, Scene, Left, bool); 19] = [
        ("build", joining_a_far_site, Dropped, true),
        ("found", founding_a_far_scaffold, Dropped, true),
        ("repair", welding_a_far_building, Dropped, true),
        ("repair unit", welding_a_far_machine, Dropped, true),
        ("salvage", salvaging_a_far_building, Dropped, true),
        (
            "return cargo",
            returning_cargo_from_far_ground,
            Rotated,
            true,
        ),
        (
            "deliver",
            delivering_a_full_load_from_far_ground,
            Rotated,
            true,
        ),
        ("withdraw", withdrawing_from_far_ground, Rotated, true),
        ("chase", chasing_a_far_machine, Cleared, true),
        (
            "attack a building",
            || shooting_a_far_building(UnitKind::Sentinel),
            Cleared,
            true,
        ),
        (
            "demolish",
            || shooting_a_far_building(UnitKind::Sapper),
            Cleared,
            true,
        ),
        ("attack a memory", shelling_a_far_memory, Cleared, true),
        (
            "attack from the dead zone",
            shelling_a_charge_from_the_dead_zone,
            Cleared,
            true,
        ),
        (
            "mounted guns",
            || attacking_into_the_pocket(UnitKind::Roc),
            Cleared,
            true,
        ),
        (
            "bomber run-in",
            || attacking_into_the_pocket(UnitKind::Condor),
            Cleared,
            true,
        ),
        ("walk", walking_to_far_ground, Rotated, false),
        ("unload", unloading_in_the_pocket, Rotated, false),
        ("board", boarding_over_far_ground, Dropped, true),
        ("retire", retiring_at_a_far_crucible, Dropped, true),
    ];
    let mut wrong = Vec::new();
    for (name, scene, left, reports) in rows {
        let (state, unit, order) = scene();
        let seen = left_of_the_loop(&state, unit, order);
        if seen != (left, reports) {
            wrong.push(format!("{name}: {seen:?}, not {:?}", (left, reports)));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn a_flier_short_of_its_goal_lands_instead_and_says_so_once() {
    // A landing takes over only a walk with nothing after it, so this case
    // stands outside the loop table.
    let mut state = split(vec![near(UnitKind::Condor)], vec![]);
    let condor = unit_at(&state, NEAR.0, NEAR.1);
    let mut events = state.tick(&[move_to(vec![condor], POCKET, false)]).events;
    events.extend(run_until(&mut state, 600, |state, _| {
        matches!(state.unit(condor).unwrap().order, Order::Land { .. })
    }));
    let u = state.unit(condor).unwrap();
    let Order::Land { goal, from } = u.order else {
        unreachable!();
    };
    assert_eq!(from, Some(POCKET), "the landing keeps the clicked tile");
    assert!(
        goal.chebyshev(POCKET) > 1,
        "on open ground outside the ring"
    );
    assert!(u.queue.is_empty());
    let reports: Vec<UnitId> = no_routes(&events).iter().map(|(id, _)| *id).collect();
    assert_eq!(reports, [condor], "one report");
}
