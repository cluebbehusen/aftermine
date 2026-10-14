//! The Spire and the Lattice: prepaid rounds assembled one at a time, a
//! silo deck that comes about before it fires, one falloff splash for every
//! target, and batteries that stop only the warheads their team tracked.

use crate::common::*;

use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::{PlayerSpec, Scenario, UnitSpec};
use oxide_sim::state::Warhead;
use oxide_sim::stats::{BuildingKind, WARHEAD, WARHEAD_LOCK_TICKS, WARHEAD_TRACK_TICKS};
use oxide_sim::{BuildingId, Command, Event, PlayerId, State, UnitKind};
use serde_json::json;

const WIDTH: usize = 72;
const HEIGHT: usize = 28;

/// West's Spire, mid-height on the left edge.
const SPIRE: (i32, i32) = (4, 12);
/// East's Lattice, its ring reaching back to x = 42 along the flight line.
const LATTICE: (i32, i32) = (56, 12);
/// The struck tile, inside the Lattice's ring.
const TARGET: TilePos = TilePos::new(60, 13);

/// An open field holding `buildings` (seat, kind, anchor) and `units`, with
/// every seat banking `scrap`.
fn field(scrap: u32, buildings: &[(u8, BuildingKind, i32, i32)], units: Vec<UnitSpec>) -> Scenario {
    let mut scenario = open_arena(WIDTH, HEIGHT, units);
    for seat in &mut scenario.players {
        seat.scrap = scrap;
    }
    scenario.buildings = buildings
        .iter()
        .map(|&(seat, kind, x, y)| building(seat, kind, x, y))
        .collect();
    scenario
}

/// Every magazine full, as finished assembly leaves it.
fn loaded(state: &State) -> State {
    let mut d = serde_json::to_value(state).unwrap();
    for (b, value) in state
        .buildings()
        .iter()
        .zip(d["buildings"].as_array_mut().unwrap())
    {
        if let Some(stats) = b.kind.magazine() {
            value["magazine"] = json!({"loaded": stats.capacity, "autofill": false});
        }
    }
    serde_json::from_value(d).unwrap()
}

fn scrap(state: &State, seat: u8) -> u32 {
    state.player(PlayerId(seat)).scrap
}

fn id(state: &State, seat: u8, kind: BuildingKind) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(seat) && b.kind == kind)
        .unwrap()
        .id
}

/// Why `command` was refused, if it was. A refusal leaves the world exactly
/// as an idle tick would.
fn refusal(state: &State, seat: u8, command: Command) -> Option<RejectReason> {
    let mut idle = state.clone();
    idle.tick(&[]);
    let mut tried = state.clone();
    let report = tried.tick(&[cmd(seat, command)]);
    let reason = report.events.iter().find_map(|event| match event {
        Event::CommandRejected { reason, .. } => Some(*reason),
        _ => None,
    });
    if reason.is_some() {
        assert_eq!(tried.hash(), idle.hash(), "a refusal changes nothing");
    }
    reason
}

fn launch(silo: BuildingId, target: TilePos) -> Command {
    Command::Launch {
        building: silo,
        target,
    }
}

/// Runs until a warhead launches, returning it as it flies.
/// Orders `silo` to strike `target` and runs until the warhead leaves its
/// deck, returning it.
fn fire(state: &mut State, player: u8, silo: BuildingId, target: TilePos) -> Warhead {
    state.tick(&[cmd(player, launch(silo, target))]);
    launched(state, 200)
}

fn launched(state: &mut State, ticks: u64) -> Warhead {
    let events = run_until(state, ticks, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::WarheadLaunched { .. }))
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::WarheadLaunched { .. })),
        "nothing launched"
    );
    state.warheads().last().unwrap().clone()
}

#[test]
fn a_spire_needs_a_crucible_and_a_finished_deep_bore() {
    let frame = TilePos::new(10, 4);
    let scenario = |crucible: bool| {
        let mut scenario = open_arena_with(WIDTH, HEIGHT, vec![], |rows| {
            rows[usize::try_from(frame.y).unwrap()][usize::try_from(frame.x).unwrap()] = 'E';
        });
        scenario
            .buildings
            .push(building(0, BuildingKind::Extractor, frame.x, frame.y));
        if crucible {
            scenario
                .buildings
                .push(building(0, BuildingKind::Crucible, 20, 4));
        }
        scenario.build().unwrap()
    };
    let deep_bore = |state: &State, phase: serde_json::Value| -> State {
        let mut d = serde_json::to_value(state).unwrap();
        let bore = d["buildings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["kind"] == json!("extractor"))
            .unwrap();
        bore["tier"] = json!(1);
        bore["phase"] = phase;
        bore["hp"] = json!(BuildingKind::Extractor.tier_stats(1).max_hp / 5);
        serde_json::from_value(d).unwrap()
    };
    let west = PlayerId(0);
    let base = scenario(true);
    assert!(
        !base.prerequisites_met(west, BuildingKind::Spire),
        "an Extractor is no Deep Bore"
    );
    let refitting = deep_bore(&base, json!({"phase": "upgrading", "progress": 10}));
    assert!(
        !refitting.prerequisites_met(west, BuildingKind::Spire),
        "a Deep Bore still refitting does not count"
    );
    let finished = deep_bore(&base, json!({"phase": "built"}));
    assert!(finished.prerequisites_met(west, BuildingKind::Spire));
    assert!(
        !deep_bore(&scenario(false), json!({"phase": "built"}))
            .prerequisites_met(west, BuildingKind::Spire),
        "the Crucible is required too"
    );
    assert!(
        !base.prerequisites_met(west, BuildingKind::Lattice),
        "a Lattice needs a Fabricator"
    );
}

#[test]
fn rounds_are_prepaid_and_assemble_one_at_a_time() {
    let mut state = field(
        1000,
        &[(1, BuildingKind::Lattice, LATTICE.0, LATTICE.1)],
        vec![],
    )
    .build()
    .unwrap();
    let lattice = id(&state, 1, BuildingKind::Lattice);
    let stats = BuildingKind::Lattice.magazine().unwrap();
    state.tick(&[
        cmd(
            1,
            Command::SetAutofill {
                buildings: vec![lattice],
                enabled: false,
            },
        ),
        cmd(1, Command::OrderRound { building: lattice }),
        cmd(1, Command::OrderRound { building: lattice }),
    ]);
    assert_eq!(scrap(&state, 1), 1000 - 2 * stats.round_cost);
    assert_eq!(
        refusal(&state, 1, Command::OrderRound { building: lattice }),
        Some(RejectReason::QueueFull),
        "loaded and ordered rounds together fill the magazine"
    );
    let mut assembled = Vec::new();
    for _ in 0..2 * stats.round_ticks + 5 {
        let report = state.tick(&[]);
        assembled.extend(report.events.iter().filter_map(|event| match event {
            Event::RoundAssembled { loaded, .. } => Some((report.tick, *loaded)),
            _ => None,
        }));
    }
    // Ordered on tick 0, each round's first tick of work is its order tick.
    let round = u64::from(stats.round_ticks);
    assert_eq!(
        assembled,
        [(round - 1, 1), (2 * round - 1, 2)],
        "the second round starts only once the first is loaded"
    );
}

#[test]
fn a_lattice_autofills_a_spire_does_not_and_cancelling_stops_autofill() {
    let mut state = field(
        2000,
        &[
            (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
            (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
        ],
        vec![],
    )
    .build()
    .unwrap();
    let (spire, lattice) = (
        id(&state, 0, BuildingKind::Spire),
        id(&state, 1, BuildingKind::Lattice),
    );
    let report = state.tick(&[]);
    assert!(report.events.iter().any(|event| matches!(
        event,
        Event::RoundAutofilled { building, .. } if *building == lattice
    )));
    let lattice_cost = BuildingKind::Lattice.magazine().unwrap().round_cost;
    assert_eq!(scrap(&state, 1), 2000 - lattice_cost);
    assert_eq!(scrap(&state, 0), 2000, "a Spire orders nothing by itself");
    assert_eq!(state.building(spire).unwrap().magazine.unwrap().ordered, 0);

    state.tick(&[cmd(1, Command::CancelRound { building: lattice })]);
    let magazine = state.building(lattice).unwrap().magazine.unwrap();
    assert_eq!(
        (magazine.ordered, magazine.progress, magazine.autofill),
        (0, 0, false)
    );
    assert_eq!(scrap(&state, 1), 2000, "a cancelled round refunds in full");
    run(&mut state, 10);
    assert_eq!(scrap(&state, 1), 2000, "nothing buys it back");
}

#[test]
fn autofill_leaves_a_stranded_seat_its_recovery_package() {
    let cost = BuildingKind::Lattice.magazine().unwrap().round_cost;
    let stage = |units| {
        let mut state = field(
            cost,
            &[(1, BuildingKind::Lattice, LATTICE.0, LATTICE.1)],
            units,
        )
        .build()
        .unwrap();
        state.tick(&[]);
        scrap(&state, 1)
    };
    assert_eq!(stage(vec![unit(1, UnitKind::Harvester, 60, 20)]), 0);
    assert!(
        stage(vec![]) >= cost,
        "with no worker left the bank is the Foundry's recovery package"
    );
}

#[test]
fn refused_round_and_strike_orders_change_nothing() {
    let state = loaded(
        &field(
            100,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (0, BuildingKind::Turret, 10, 4),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let (spire, turret, lattice) = (
        id(&state, 0, BuildingKind::Spire),
        id(&state, 0, BuildingKind::Turret),
        id(&state, 1, BuildingKind::Lattice),
    );
    let mut empty = state.clone();
    fire(&mut empty, 0, spire, TARGET);
    assert_eq!(empty.building(spire).unwrap().magazine.unwrap().loaded, 0);
    for (command, expected) in [
        (
            Command::OrderRound { building: lattice },
            RejectReason::NotYourBuilding,
        ),
        (
            Command::OrderRound { building: turret },
            RejectReason::CannotProduce,
        ),
        (
            Command::OrderRound { building: spire },
            RejectReason::QueueFull,
        ),
        (
            Command::CancelRound { building: spire },
            RejectReason::InvalidTarget,
        ),
        (
            Command::CancelRound { building: turret },
            RejectReason::InvalidTarget,
        ),
        (
            Command::SetAutofill {
                buildings: vec![spire, lattice],
                enabled: true,
            },
            RejectReason::NotYourBuilding,
        ),
        (
            Command::SetAutofill {
                buildings: vec![spire, turret],
                enabled: true,
            },
            RejectReason::InvalidTarget,
        ),
        (
            Command::SetAutofill {
                buildings: vec![],
                enabled: true,
            },
            RejectReason::NotYourBuilding,
        ),
        (launch(turret, TARGET), RejectReason::InvalidTarget),
        (launch(lattice, TARGET), RejectReason::NotYourBuilding),
        (
            launch(spire, TilePos::new(-1, 3)),
            RejectReason::OutOfBounds,
        ),
        (
            launch(spire, TilePos::new(3, i32::try_from(HEIGHT).unwrap())),
            RejectReason::OutOfBounds,
        ),
    ] {
        assert_eq!(
            refusal(&state, 0, command.clone()),
            Some(expected),
            "{command:?}"
        );
    }
    assert_eq!(
        refusal(&empty, 0, launch(spire, TARGET)),
        Some(RejectReason::NothingLoaded)
    );
    assert_eq!(
        refusal(&empty, 0, Command::OrderRound { building: spire }),
        Some(RejectReason::NotEnoughScrap)
    );
    // A strike on unseen ground is a legal order.
    assert_eq!(refusal(&state, 0, launch(spire, TARGET)), None);
}

#[test]
fn a_spire_comes_about_before_it_launches_and_stop_calls_the_strike_off() {
    let state = loaded(
        &field(0, &[(0, BuildingKind::Spire, SPIRE.0, SPIRE.1)], vec![])
            .build()
            .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    let silo = BuildingKind::Spire.silo().unwrap();
    let (turn, ready) = (silo.turn_rate, u64::from(silo.ready_ticks));

    // The deck starts toward the map centre, so an eastward strike is
    // already lined up: it stands ready while its locks release, then
    // fires.
    let mut ahead = state.clone();
    ahead.tick(&[cmd(0, launch(spire, TARGET))]);
    assert_eq!(ahead.building(spire).unwrap().launcher.unwrap().ready, 1);
    assert!(ahead.warheads().is_empty(), "the locks release first");
    assert_eq!(launched(&mut ahead, 200).launched_at, ready - 1);

    // Behind it, the deck swings a half turn first, in plain sight.
    let mut behind = state.clone();
    let behind_tile = TilePos::new(1, 13);
    let deck = behind.building(spire).unwrap();
    let start = deck.launcher.unwrap().heading;
    let swing = chassis::compass::heading_of(behind_tile.center() - deck.center())
        .wrapping_sub(start)
        .cast_signed()
        .unsigned_abs();
    behind.tick(&[cmd(0, launch(spire, behind_tile))]);
    let after = behind.building(spire).unwrap().launcher.unwrap().heading;
    assert_eq!(after.wrapping_sub(start).cast_signed().unsigned_abs(), turn);
    let warhead = launched(&mut behind, 200);
    // It stands ready once within two steps of the bearing, then fires.
    assert_eq!(
        warhead.launched_at,
        u64::from((swing - 2).div_ceil(turn)) - 1 + ready - 1,
        "a {swing}-step swing at {turn} a tick"
    );

    // A new strike lines up afresh.
    let mut retargeted = state.clone();
    retargeted.tick(&[cmd(0, launch(spire, TARGET))]);
    retargeted.tick(&[]);
    retargeted.tick(&[cmd(0, launch(spire, TARGET))]);
    assert_eq!(
        retargeted.building(spire).unwrap().launcher.unwrap().ready,
        1
    );

    // Stop before it comes about and nothing flies.
    let mut stopped = state.clone();
    stopped.tick(&[cmd(0, launch(spire, TilePos::new(1, 13)))]);
    stopped.tick(&[cmd(
        0,
        Command::ClearFocus {
            buildings: vec![spire],
        },
    )]);
    let events = run(&mut stopped, 200);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::WarheadLaunched { .. }))
    );
    assert_eq!(stopped.building(spire).unwrap().magazine.unwrap().loaded, 1);
}

#[test]
fn a_warhead_bursts_on_its_arrival_tick_with_one_falloff_splash() {
    let impact = TARGET.center();
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                // Its own battery under the line never fires on its own
                // warhead.
                (0, BuildingKind::Lattice, 50, 4),
                // One Crucible under the impact, one 2.75 tiles off, one
                // past the outer radius.
                (1, BuildingKind::Crucible, 60, 13),
                (1, BuildingKind::Crucible, 63, 16),
                (1, BuildingKind::Crucible, 60, 4),
                (0, BuildingKind::Barricade, 58, 11),
            ],
            vec![
                unit(1, UnitKind::Harvester, 59, 14),
                unit(1, UnitKind::Kestrel, 61, 12),
                unit(1, UnitKind::Mantle, 61, 15),
                unit(1, UnitKind::Surveyor, 66, 13),
                unit(0, UnitKind::Harvester, 59, 13),
            ],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    let warhead = fire(&mut state, 0, spire, TARGET);
    assert_eq!(
        warhead.arrival - warhead.launched_at,
        WARHEAD.flight_ticks(warhead.launch, impact)
    );
    let crucibles: Vec<_> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Crucible)
        .map(|b| (b.id, b.hp, b.closest_point_to(impact).dist(impact)))
        .collect();
    let landing = run_until(&mut state, 400, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::WarheadLanded { .. }))
    });
    assert_eq!(state.current_tick() - 1, warhead.arrival);
    assert!(!landing.iter().any(|event| matches!(
        event,
        Event::WarheadIntercepted { .. } | Event::ShellIntercepted { .. }
    )));
    for (building, hp, distance) in crucibles {
        assert_eq!(
            state.building(building).unwrap().hp,
            hp - WARHEAD.damage_at(distance),
            "a Crucible {distance:?} from the impact"
        );
    }
    assert!(WARHEAD.damage_at(chassis::fx::Fx::lit("2.75")) > 0);
    let alive = |kind: UnitKind, seat: u8| {
        state
            .units()
            .iter()
            .any(|u| u.kind == kind && u.player == PlayerId(seat))
    };
    assert!(!alive(UnitKind::Harvester, 1), "full damage at the centre");
    assert!(
        !alive(UnitKind::Kestrel, 1),
        "aircraft overhead take it too"
    );
    assert!(
        alive(UnitKind::Surveyor, 1),
        "nothing past the outer radius"
    );
    assert!(
        alive(UnitKind::Harvester, 0),
        "only hostile machines are hurt"
    );
    let barricade = id(&state, 0, BuildingKind::Barricade);
    assert_eq!(
        state.building(barricade).unwrap().hp,
        BuildingKind::Barricade.base_stats().max_hp,
        "nor own buildings"
    );
}

#[test]
fn a_foundry_shrugs_off_half_of_each_warhead() {
    let foundry_tile = TilePos::new(69, 25);
    let mut state = field(0, &[(0, BuildingKind::Spire, SPIRE.0, SPIRE.1)], vec![])
        .build()
        .unwrap();
    let spire = id(&state, 0, BuildingKind::Spire);
    let foundry = id(&state, 1, BuildingKind::Foundry);
    let max = BuildingKind::Foundry.base_stats().max_hp;
    let per_hit = BuildingKind::Foundry.warhead_taken(WARHEAD.peak_damage);
    assert_eq!(per_hit, 400);
    for hit in 1..=4 {
        state = loaded(&state);
        state.tick(&[cmd(0, launch(spire, foundry_tile))]);
        run_until(&mut state, 600, |_, events| {
            events
                .iter()
                .any(|event| matches!(event, Event::WarheadLanded { .. }))
        });
        if hit < 4 {
            assert_eq!(state.building(foundry).unwrap().hp, max - hit * per_hit);
        }
    }
    assert!(
        state.building(foundry).is_none(),
        "four unanswered hits level it"
    );
    assert!(state.result().is_some());
    assert!(state.warheads().is_empty());
}

#[test]
fn a_team_tracks_warheads_by_sight_or_radar_and_keeps_its_exposure() {
    // East's two workers each see one tile of the line, five ticks apiece.
    let mut state = loaded(
        &field(
            0,
            &[(0, BuildingKind::Spire, SPIRE.0, SPIRE.1)],
            vec![
                unit(1, UnitKind::Harvester, 20, 19),
                unit(1, UnitKind::Harvester, 30, 19),
            ],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    let east = PlayerId(1);
    let track = |state: &State| state.vision(east).warheads().first().copied();
    run_until(&mut state, 400, |s, _| track(s).is_some_and(|t| !t.in_view));
    let first = track(&state).unwrap().exposure;
    assert!(first > 0 && first < WARHEAD_TRACK_TICKS);
    run_until(&mut state, 400, |s, _| track(s).is_some_and(|t| t.in_view));
    run_until(&mut state, 400, |s, _| track(s).is_some_and(|t| !t.in_view));
    let second = track(&state).unwrap();
    assert!(
        second.exposure > first && !second.tracked(),
        "sightings add up without resetting"
    );
    assert!(
        state.vision(PlayerId(0)).warheads().is_empty(),
        "nobody tracks their own"
    );

    // An Array's radar holds a warhead its true sight never reaches.
    let mut radar = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&radar, 0, BuildingKind::Spire);
    fire(&mut radar, 0, spire, TARGET);
    run(&mut radar, 40);
    let held = radar.vision(east).warheads()[0];
    let at = TilePos::containing(radar.warheads()[0].position(radar.current_tick() - 1));
    assert!(held.in_view && !radar.can_see(east, at));
    assert!(held.tracked());
}

#[test]
fn teammates_share_one_hold_on_each_warhead() {
    let mut scenario = open_arena_with(WIDTH, HEIGHT, vec![], |rows| rows[24][40] = '3');
    scenario
        .buildings
        .push(building(0, BuildingKind::Spire, SPIRE.0, SPIRE.1));
    scenario.players.push(PlayerSpec {
        name: "North".into(),
        team: Some(1),
        scrap: 0,
        bot: false,
        bot_config: None,
    });
    scenario.players[1].team = Some(1);
    scenario.players[0].team = Some(0);
    scenario.units.push(unit(2, UnitKind::Kestrel, 20, 13));
    let mut state = loaded(&scenario.build().unwrap());
    let spire = id(&state, 0, BuildingKind::Spire);
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    run(&mut state, 60);
    assert!(!state.vision(PlayerId(1)).warheads().is_empty());
    assert_eq!(
        state.vision(PlayerId(1)).warheads(),
        state.vision(PlayerId(2)).warheads()
    );
}

#[test]
fn a_tracked_warhead_is_stopped_where_it_enters_the_ring() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    let lattice = id(&state, 1, BuildingKind::Lattice);
    let ring = BuildingKind::Lattice.battery().unwrap().ring;
    let centre = state.building(lattice).unwrap().center();
    let warhead = fire(&mut state, 0, spire, TARGET);
    let events = run_until(&mut state, 400, |_, events| {
        events.iter().any(|event| {
            matches!(
                event,
                Event::WarheadIntercepted { .. } | Event::WarheadLanded { .. }
            )
        })
    });
    let Some(Event::WarheadIntercepted { battery, pod, .. }) = events
        .iter()
        .find(|event| matches!(event, Event::WarheadIntercepted { .. }))
    else {
        panic!("the warhead landed");
    };
    let now = state.current_tick() - 1;
    let inside = |tick| warhead.position(tick).dist_sq(centre) <= ring * ring;
    assert!(
        inside(now) && !inside(now - 1),
        "stopped on the crossing tick"
    );
    assert_eq!((*battery, *pod), (lattice, 1));
    assert_eq!(state.building(lattice).unwrap().magazine.unwrap().loaded, 1);
    assert!(state.warheads().is_empty());
}

#[test]
fn a_warhead_first_seen_inside_the_ring_is_stopped_once_locked_on() {
    // A Sentinel's sight first finds the warhead inside the ring, far
    // enough out for the battery to lock on before it lands.
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
            ],
            vec![unit(1, UnitKind::Sentinel, 50, 13)],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    let east = PlayerId(1);
    run_until(&mut state, 400, |s, _| {
        !s.vision(east).warheads().is_empty()
    });
    let seen = state.current_tick() - 1;
    let events = run_until(&mut state, 400, |_, events| {
        events.iter().any(|event| {
            matches!(
                event,
                Event::WarheadIntercepted { .. } | Event::WarheadLanded { .. }
            )
        })
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::WarheadIntercepted { .. }))
    );
    assert_eq!(
        state.current_tick() - 1,
        seen + u64::from(WARHEAD_TRACK_TICKS) + u64::from(WARHEAD_LOCK_TICKS),
        "the battery fires the tick after its team locks on"
    );
}

#[test]
fn a_lattice_lets_untracked_and_out_of_ring_warheads_through() {
    let stage = |target: TilePos, array: bool| {
        let mut buildings = vec![
            (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
            (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
        ];
        if array {
            buildings.push((1, BuildingKind::Array, 20, 2));
        }
        let mut state = loaded(&field(0, &buildings, vec![]).build().unwrap());
        let spire = id(&state, 0, BuildingKind::Spire);
        state.tick(&[cmd(0, launch(spire, target))]);
        run_until(&mut state, 600, |_, events| {
            events.iter().any(|event| {
                matches!(
                    event,
                    Event::WarheadIntercepted { .. } | Event::WarheadLanded { .. }
                )
            })
        })
    };
    let landed = |events: &[Event]| {
        events
            .iter()
            .any(|event| matches!(event, Event::WarheadLanded { .. }))
    };
    // Aimed short of the ring, it is flying over someone else's ground.
    assert!(landed(&stage(TilePos::new(36, 13), true)));
    // Seen only inside the ring and too late to finish the track.
    assert!(landed(&stage(TilePos::new(53, 13), false)));
}

#[test]
fn three_warheads_saturate_a_two_round_lattice() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, 4, 6),
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (0, BuildingKind::Spire, 4, 18),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
                (1, BuildingKind::Array, 20, 2),
                (1, BuildingKind::Array, 20, 24),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spires: Vec<BuildingId> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Spire)
        .map(|b| b.id)
        .collect();
    let mut events = state
        .tick(
            &spires
                .iter()
                .map(|&spire| cmd(0, launch(spire, TARGET)))
                .collect::<Vec<_>>(),
        )
        .events;
    events.extend(run(&mut state, 600));
    let count = |pattern: fn(&Event) -> bool| events.iter().filter(|e| pattern(e)).count();
    assert_eq!(count(|e| matches!(e, Event::WarheadLaunched { .. })), 3);
    assert_eq!(count(|e| matches!(e, Event::WarheadIntercepted { .. })), 2);
    assert_eq!(count(|e| matches!(e, Event::WarheadLanded { .. })), 1);
}

#[test]
fn a_battery_between_two_seats_warheads_stops_the_one_its_frame_ranks_first() {
    // One round against equal flights from opposite sides into the
    // battery's centre, which tie on every measure of the pair but where
    // each stands. Swapping the seats that fire keeps the stop on the west.
    let stopped = |west: u8| {
        let east = 3 - west;
        let mut scenario = open_arena_with(WIDTH, HEIGHT, vec![], |rows| rows[1][36] = '3');
        scenario.players.push(PlayerSpec {
            name: "North".into(),
            team: None,
            scrap: 0,
            bot: false,
            bot_config: None,
        });
        let mut spires = [(west, (4, 12)), (east, (66, 14))];
        spires.sort_unstable();
        scenario.buildings = spires
            .iter()
            .map(|&(seat, (x, y))| building(seat, BuildingKind::Spire, x, y))
            .chain([
                building(0, BuildingKind::Lattice, 35, 13),
                building(0, BuildingKind::Array, 12, 12),
                building(0, BuildingKind::Array, 58, 14),
            ])
            .collect();
        let mut state = loaded(&scenario.build().unwrap());
        let mut d = serde_json::to_value(&state).unwrap();
        for (b, value) in state
            .buildings()
            .iter()
            .zip(d["buildings"].as_array_mut().unwrap())
        {
            if b.kind == BuildingKind::Lattice {
                value["magazine"]["loaded"] = json!(1);
            }
        }
        state = serde_json::from_value(d).unwrap();
        let mut commands = vec![
            cmd(
                west,
                launch(id(&state, west, BuildingKind::Spire), TilePos::new(36, 14)),
            ),
            cmd(
                east,
                launch(id(&state, east, BuildingKind::Spire), TilePos::new(35, 13)),
            ),
        ];
        let mut launches = Vec::new();
        let mut stops = Vec::new();
        for _ in 0..600 {
            let report = state.tick(&std::mem::take(&mut commands));
            for event in &report.events {
                match event {
                    Event::WarheadLaunched { .. } => launches.push(report.tick),
                    Event::WarheadIntercepted { player, .. } => stops.push(*player),
                    _ => {}
                }
            }
        }
        assert!(
            launches.len() == 2 && launches[0] == launches[1],
            "both launch together: {launches:?}"
        );
        assert_eq!(stops.len(), 1);
        stops[0]
    };
    assert_eq!(stopped(1), PlayerId(1));
    assert_eq!(stopped(2), PlayerId(2));
}

#[test]
fn a_decided_match_drops_its_warheads_and_tracks() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    run(&mut state, 20);
    assert!(!state.vision(PlayerId(1)).warheads().is_empty());
    state.tick(&[cmd(1, Command::Surrender)]);
    assert!(state.result().is_some());
    assert!(state.warheads().is_empty());
    assert!(state.vision(PlayerId(1)).warheads().is_empty());
    let resumed: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    assert_eq!(resumed.hash(), state.hash());
}

#[test]
fn every_tick_of_a_strike_round_trips() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (0, BuildingKind::Spire, 4, 18),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spires: Vec<BuildingId> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Spire)
        .map(|b| b.id)
        .collect();
    state.tick(&[cmd(0, launch(spires[0], TARGET))]);
    state.tick(&[cmd(0, launch(spires[1], TilePos::new(1, 13)))]);
    for _ in 0..400 {
        let resumed: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(resumed.hash(), state.hash());
        state.tick(&[]);
    }
}

/// Mirrored seats: each Spire strikes into the other's Lattice ring, each
/// team with an Array over the line, half-turned about the map centre.
#[test]
fn half_turned_strikes_turn_fly_and_fall_alike() {
    let mirror = |(x, y): (i32, i32)| {
        let (w, h) = (
            i32::try_from(WIDTH).unwrap(),
            i32::try_from(HEIGHT).unwrap(),
        );
        (w - 2 - x, h - 2 - y)
    };
    let west_spire = (6, 6);
    let west_lattice = (14, 12);
    let west_array = (40, 22);
    let mut buildings = Vec::new();
    for (kind, west) in [
        (BuildingKind::Spire, west_spire),
        (BuildingKind::Lattice, west_lattice),
    ] {
        let east = mirror(west);
        buildings.push((0, kind, west.0, west.1));
        buildings.push((1, kind, east.0, east.1));
    }
    let east_array = (
        i32::try_from(WIDTH).unwrap() - 1 - west_array.0,
        i32::try_from(HEIGHT).unwrap() - 1 - west_array.1,
    );
    buildings.push((0, BuildingKind::Array, west_array.0, west_array.1));
    buildings.push((1, BuildingKind::Array, east_array.0, east_array.1));
    let mut state = loaded(&field(0, &buildings, vec![]).build().unwrap());
    let west_target = TilePos::new(57, 14);
    let east_target = TilePos::new(
        i32::try_from(WIDTH).unwrap() - 1 - west_target.x,
        i32::try_from(HEIGHT).unwrap() - 1 - west_target.y,
    );
    let mut commands = vec![
        cmd(0, launch(id(&state, 0, BuildingKind::Spire), west_target)),
        cmd(1, launch(id(&state, 1, BuildingKind::Spire), east_target)),
    ];
    let mut seen = Vec::new();
    for _ in 0..600 {
        let report = state.tick(&std::mem::take(&mut commands));
        for event in &report.events {
            match event {
                Event::WarheadLaunched { player, .. } => {
                    seen.push((report.tick, "launch", *player, 0));
                }
                Event::WarheadIntercepted {
                    battery_player,
                    pod,
                    ..
                } => seen.push((report.tick, "intercept", *battery_player, *pod)),
                Event::WarheadLanded { player, .. } => seen.push((report.tick, "land", *player, 0)),
                _ => {}
            }
        }
    }
    let side = |seat: u8| -> Vec<_> {
        seen.iter()
            .filter(|(_, _, player, _)| *player == PlayerId(seat))
            .map(|(tick, what, _, pod)| (*tick, *what, *pod))
            .collect()
    };
    assert!(side(0).iter().any(|(_, what, _)| *what == "intercept"));
    assert_eq!(side(0), side(1));
}

#[test]
fn a_hostile_magazine_shows_its_rounds_in_sight_and_never_its_orders() {
    let stage = |eyes: Vec<UnitSpec>| {
        let mut state = loaded(
            &field(2000, &[(0, BuildingKind::Spire, SPIRE.0, SPIRE.1)], eyes)
                .build()
                .unwrap(),
        );
        state.tick(&[]);
        state
    };
    let east = PlayerId(1);
    let dark = stage(vec![]);
    assert!(dark.known_magazines(east).is_empty());
    let own = dark.known_magazines(PlayerId(0));
    assert_eq!(own.len(), 1);
    assert!(own[0].detail.is_some(), "a team knows its own orders");

    let seen = stage(vec![unit(1, UnitKind::Surveyor, 9, 13)]);
    let known = seen.known_magazines(east);
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].loaded, 1);
    assert!(known[0].heading.is_some(), "the deck is in plain sight");
    assert_eq!(known[0].detail, None, "orders and autofill stay hidden");
    assert_eq!(
        oxide_sim::observation::ObservationData::fog_honest(&seen, east).magazines,
        known
    );
}

#[test]
fn a_hostile_warhead_shows_on_radar_its_line_once_tracked_and_its_own_team_forecast() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (0, BuildingKind::Lattice, 50, 4),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    let lattice = id(&state, 1, BuildingKind::Lattice);
    let (west, east) = (PlayerId(0), PlayerId(1));
    fire(&mut state, 0, spire, TARGET);
    state.tick(&[]);
    let early = state.known_warheads(east);
    assert_eq!(early.len(), 1);
    assert!(
        early[0].hostile && early[0].player.is_none(),
        "a radar return names no one"
    );
    assert_eq!(early[0].line, None, "the line waits for the track");
    assert_eq!(early[0].interceptor, None);
    let own = state.known_warheads(west);
    assert!(!own[0].hostile && own[0].line.is_some());

    run(&mut state, u64::from(WARHEAD_TRACK_TICKS));
    let tracked = state.known_warheads(east);
    assert_eq!(
        tracked[0].line.map(|line| line.impact),
        Some(TARGET.center())
    );
    assert_eq!(tracked[0].interceptor, Some(lattice));
    assert_eq!(
        state.known_warheads(west)[0].interceptor,
        None,
        "the launching side is never told which battery waits"
    );
    assert_eq!(
        oxide_sim::observation::ObservationData::fog_honest(&state, east).warheads,
        tracked
    );
}

#[test]
fn an_unseen_seat_ordering_turning_and_launching_changes_nothing_its_enemy_knows() {
    let base = field(
        2000,
        &[
            (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
            (0, BuildingKind::Spire, 4, 18),
        ],
        vec![],
    )
    .build()
    .unwrap();
    let mut quiet = base.clone();
    let mut busy = base.clone();
    let mut d = serde_json::to_value(&busy).unwrap();
    let spires: Vec<BuildingId> = busy
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Spire)
        .map(|b| b.id)
        .collect();
    for (b, value) in busy
        .buildings()
        .iter()
        .zip(d["buildings"].as_array_mut().unwrap())
    {
        if b.id == spires[1] {
            value["magazine"]["loaded"] = json!(1);
        }
    }
    busy = serde_json::from_value(d).unwrap();
    let mut d = serde_json::to_value(&quiet).unwrap();
    for (b, value) in quiet
        .buildings()
        .iter()
        .zip(d["buildings"].as_array_mut().unwrap())
    {
        if b.id == spires[1] {
            value["magazine"]["loaded"] = json!(1);
        }
    }
    quiet = serde_json::from_value(d).unwrap();
    busy.tick(&[
        cmd(
            0,
            Command::OrderRound {
                building: spires[0],
            },
        ),
        cmd(0, launch(spires[1], TilePos::new(1, 20))),
    ]);
    quiet.tick(&[]);
    let east = PlayerId(1);
    let mut launched = false;
    for _ in 0..200 {
        let view = |state: &State| {
            serde_json::to_string(&oxide_sim::observation::ObservationData::fog_honest(
                state, east,
            ))
            .unwrap()
        };
        assert_eq!(view(&busy), view(&quiet));
        launched |= !busy.warheads().is_empty();
        busy.tick(&[]);
        quiet.tick(&[]);
    }
    assert!(launched, "the hidden strike flew");
}

#[test]
fn warhead_damage_holds_at_the_centre_and_fades_linearly_to_the_edge() {
    use chassis::fx::Fx;
    let peak = WARHEAD.peak_damage;
    assert_eq!(WARHEAD.damage_at(Fx::ZERO), peak);
    assert_eq!(WARHEAD.damage_at(WARHEAD.full_radius), peak);
    let middle = (WARHEAD.full_radius + WARHEAD.outer_radius) / 2;
    assert_eq!(WARHEAD.damage_at(middle), peak / 2);
    assert_eq!(WARHEAD.damage_at(WARHEAD.outer_radius), 0);
    assert_eq!(BuildingKind::Crucible.warhead_taken(peak), peak);
}

#[test]
fn a_completed_track_reveals_the_silo_it_came_from() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    let east = PlayerId(1);
    let silo = |state: &State| {
        state
            .vision(east)
            .ghosts()
            .iter()
            .find(|ghost| ghost.kind == BuildingKind::Spire)
            .copied()
    };
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    run_until(&mut state, 100, |s, _| {
        s.vision(east)
            .warheads()
            .first()
            .is_some_and(|track| track.exposure + 1 == WARHEAD_TRACK_TICKS)
    });
    assert_eq!(
        silo(&state),
        None,
        "nothing is known before the track completes"
    );
    state.tick(&[]);
    let ghost = silo(&state).expect("the track gives the silo away");
    assert_eq!(
        (ghost.owner, ghost.anchor, ghost.hp, ghost.built),
        (
            PlayerId(0),
            TilePos::new(SPIRE.0, SPIRE.1),
            BuildingKind::Spire.base_stats().max_hp,
            true
        )
    );
    assert!(
        !state.can_see(east, TilePos::new(SPIRE.0, SPIRE.1)),
        "revealed without sight"
    );
    let known = oxide_sim::observation::ObservationData::fog_honest(&state, east);
    assert!(
        known
            .enemy_buildings
            .iter()
            .any(|b| b.kind == BuildingKind::Spire && !b.seen)
    );
    run_until(&mut state, 600, |s, _| s.warheads().is_empty());
    assert_eq!(
        silo(&state),
        Some(ghost),
        "remembered after the warhead lands"
    );
}

#[test]
fn a_revealed_silo_replaces_the_memories_its_footprint_overlaps() {
    // East last saw a West Turret where the Spire now stands, a West Array
    // beside it, and a North charge under it, which a hostile building may
    // stand over.
    let mut scenario = open_arena_with(WIDTH, HEIGHT, vec![], |rows| rows[1][36] = '3');
    scenario.players.push(PlayerSpec {
        name: "North".into(),
        team: None,
        scrap: 0,
        bot: false,
        bot_config: None,
    });
    scenario.buildings = vec![
        building(0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
        building(1, BuildingKind::Array, 20, 2),
        building(2, BuildingKind::Array, 36, 24),
    ];
    let mut state = loaded(&scenario.build().unwrap());
    let memory = |kind: BuildingKind, owner: u8, x: i32, y: i32| oxide_sim::GhostBuilding {
        kind,
        owner: PlayerId(owner),
        anchor: TilePos::new(x, y),
        hp: kind.base_stats().max_hp,
        built: true,
        tier: 0,
    };
    let mut d = serde_json::to_value(&state).unwrap();
    let mut stale = vec![
        memory(BuildingKind::ScuttleCharge, 2, SPIRE.0, SPIRE.1),
        memory(BuildingKind::Turret, 0, SPIRE.0 + 1, SPIRE.1 + 1),
        memory(BuildingKind::Array, 0, SPIRE.0 + 4, SPIRE.1),
    ];
    stale.sort_unstable_by_key(|g| (g.anchor.y, g.anchor.x, g.owner));
    d["vision"][1]["ghosts"] = serde_json::to_value(&stale).unwrap();
    state = serde_json::from_value(d).unwrap();
    let spire = id(&state, 0, BuildingKind::Spire);
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    run_until(&mut state, 200, |s, _| {
        s.vision(PlayerId(1))
            .warheads()
            .first()
            .is_some_and(oxide_sim::vision::WarheadTrack::tracked)
    });
    let kinds: Vec<_> = state
        .vision(PlayerId(1))
        .ghosts()
        .iter()
        .map(|g| (g.kind, g.owner))
        .collect();
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    for kept in [
        (BuildingKind::ScuttleCharge, PlayerId(2)),
        (BuildingKind::Spire, PlayerId(0)),
        (BuildingKind::Array, PlayerId(0)),
    ] {
        assert!(kinds.contains(&kept), "{kinds:?}");
    }
}

#[test]
fn the_interception_forecast_names_the_stop_before_it_happens() {
    let mut state = loaded(
        &field(
            0,
            &[
                (0, BuildingKind::Spire, SPIRE.0, SPIRE.1),
                (1, BuildingKind::Lattice, LATTICE.0, LATTICE.1),
                (1, BuildingKind::Array, 20, 2),
            ],
            vec![],
        )
        .build()
        .unwrap(),
    );
    let spire = id(&state, 0, BuildingKind::Spire);
    state.tick(&[cmd(0, launch(spire, TARGET))]);
    run_until(&mut state, 100, |s, _| {
        !s.interception_forecast().is_empty()
    });
    let [forecast] = state.interception_forecast()[..] else {
        panic!("one stop is due");
    };
    assert!(forecast.tick > state.current_tick(), "named ahead of time");
    let events = run_until(&mut state, 400, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::WarheadIntercepted { .. }))
    });
    let stop = events
        .iter()
        .find_map(|event| match event {
            Event::WarheadIntercepted {
                warhead,
                battery,
                pod,
                at,
                ..
            } => Some((*warhead, *battery, *pod, *at)),
            _ => None,
        })
        .unwrap();
    assert_eq!(state.current_tick() - 1, forecast.tick);
    assert_eq!(
        stop,
        (
            forecast.warhead,
            forecast.battery,
            forecast.pod,
            forecast.at
        )
    );
}
