//! Assembly cradles and the Maw: no cap on how many a player raises, the
//! crew a cradle admits, the public progress every seat watches, the machine that stands
//! up at the cradle's centre, and the breacher's own rules (buildings first,
//! no pursuit, structure damage, healing only by its kiln).

use crate::common::{building, cmd, face_toward, open_arena_with, players, run, run_until, unit};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::{TilePos, as_index};
use oxide_sim::command::RejectReason;
use oxide_sim::observation::ObservationData;
use oxide_sim::scenario::{BuildingSpec, ScenarioMode, UnitSpec};
use oxide_sim::stats::{BuildingKind, KILN_BITE_PERIOD, Mend};
use oxide_sim::{
    AttackTarget, Command, Event, Order, PlayerCommand, PlayerId, StallReason, State, Target,
    TickReport, UnitId, UnitKind, UnitRepairSource,
};

const WIDTH: usize = 36;
const HEIGHT: usize = 20;
/// West's Extractor frame, refitted into a Deep Bore by [`yard`].
const FRAME: TilePos = TilePos::new(8, 1);
/// Where West raises its cradle, in sight of the workers [`workers`] lines
/// up and out of the Crucible's smelting reach.
const CRADLE: TilePos = TilePos::new(4, 8);

/// An open yard where West has the cradle's prerequisites (a Crucible and a
/// Deep Bore) and both seats bank enough for a Maw.
fn yard(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>) -> State {
    let mut scenario = open_arena_with(WIDTH, HEIGHT, units, |rows| {
        rows[as_index(FRAME.y)][as_index(FRAME.x)] = 'E';
    });
    scenario.players = players(2 * UnitKind::Maw.stats().cost);
    scenario.buildings = vec![
        building(0, BuildingKind::Extractor, FRAME.x, FRAME.y),
        building(0, BuildingKind::Crucible, 12, 1),
    ];
    scenario.buildings.extend(buildings);
    let mut doc = serde_json::to_value(scenario.build().unwrap()).unwrap();
    for b in doc["buildings"].as_array_mut().unwrap() {
        if b["kind"] == serde_json::json!("extractor") {
            b["tier"] = serde_json::json!(1);
        }
    }
    serde_json::from_value(doc).unwrap()
}

fn workers(player: u8, kind: UnitKind, count: i32, y: i32) -> Vec<UnitSpec> {
    (0..count).map(|i| unit(player, kind, 3 + i, y)).collect()
}

fn ids(state: &State, player: u8, kind: UnitKind) -> Vec<UnitId> {
    state
        .units()
        .iter()
        .filter(|u| u.player == PlayerId(player) && u.kind == kind)
        .map(|u| u.id)
        .collect()
}

fn raise(units: &[UnitId], anchor: TilePos, queue: bool) -> PlayerCommand {
    cmd(
        0,
        Command::Build {
            units: units.to_vec(),
            kind: BuildingKind::MawCradle,
            anchor,
            queue,
            defer: false,
        },
    )
}

/// East's fog-honest knowledge of West's cradle, with no sight of it.
fn known_cradles(state: &State) -> Vec<oxide_sim::PublicBuilding> {
    ObservationData::fog_honest(state, PlayerId(1))
        .public_buildings
        .into_iter()
        .filter(|b| b.kind == BuildingKind::MawCradle)
        .collect()
}

fn rejected(report: &TickReport) -> Option<RejectReason> {
    report.events.iter().find_map(|event| match event {
        Event::CommandRejected { reason, .. } => Some(*reason),
        _ => None,
    })
}

fn cradle(state: &State) -> Option<&oxide_sim::Building> {
    state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::MawCradle)
}

/// Whether `id`'s current order holds West's cradle.
fn holds(state: &State, id: UnitId) -> bool {
    let unit = state.unit(id).unwrap();
    match unit.order {
        Order::Build { site } => cradle(state).is_some_and(|c| c.id == site),
        Order::Found { kind, anchor } => kind == BuildingKind::MawCradle && anchor == CRADLE,
        _ => false,
    }
}

/// The cradle's centre, where its machine stands up.
fn centre(anchor: TilePos) -> Vec2Fx {
    let (w, h) = BuildingKind::MawCradle.size();
    Vec2Fx::new(
        Fx::from_num(anchor.x * 2 + w) / 2,
        Fx::from_num(anchor.y * 2 + h) / 2,
    )
}

/// A state with the cradle's phase, progress and health forged.
fn forge_cradle(state: &State, progress: u32, hp: u32) -> State {
    let mut doc = serde_json::to_value(state).unwrap();
    for b in doc["buildings"].as_array_mut().unwrap() {
        if b["kind"] == serde_json::json!("maw_cradle") {
            b["phase"] = serde_json::json!({"phase": "site", "progress": progress});
            b["hp"] = serde_json::json!(hp);
        }
    }
    serde_json::from_value(doc).unwrap()
}

/// A state with `id`'s health forged.
fn forge_hp(state: &State, id: UnitId, hp: u32) -> State {
    let mut doc = serde_json::to_value(state).unwrap();
    for u in doc["units"].as_array_mut().unwrap() {
        if u["id"] == serde_json::json!(id) {
            u["hp"] = serde_json::json!(hp);
        }
    }
    serde_json::from_value(doc).unwrap()
}

/// A state with `amount` wreck salvage on each of `tiles`.
fn forge_wreck(state: &State, tiles: &[TilePos], amount: u32) -> State {
    let mut doc = serde_json::to_value(state).unwrap();
    for tile in tiles {
        let index = as_index(tile.y) * WIDTH + as_index(tile.x);
        doc["map"]["grid"]["cells"][index]["wreck"] = serde_json::json!(amount);
    }
    serde_json::from_value(doc).unwrap()
}

/// Runs idle ticks until the next tick is a kiln bite.
fn to_bite(state: &mut State) {
    while !state.current_tick().is_multiple_of(KILN_BITE_PERIOD) {
        state.tick(&[]);
    }
}

#[test]
fn a_player_raises_another_cradle_beside_one_or_a_living_maw() {
    let mut state = yard(workers(0, UnitKind::Excavator, 2, 12), vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    assert_eq!(
        rejected(&state.tick(&[raise(&crew[..1], CRADLE, false)])),
        None
    );
    let (width, height) = (
        i32::try_from(WIDTH).unwrap(),
        i32::try_from(HEIGHT).unwrap(),
    );
    let elsewhere = (0..height)
        .flat_map(|y| (0..width).map(move |x| TilePos::new(x, y)))
        .find(|&anchor| {
            state
                .place_refusal(PlayerId(0), BuildingKind::MawCradle, anchor)
                .is_none()
        })
        .expect("room in sight for a second cradle");
    assert_eq!(
        rejected(&state.tick(&[raise(&crew[1..], elsewhere, false)])),
        None
    );
    let cradles = |state: &State| {
        state
            .buildings()
            .iter()
            .filter(|b| b.player == PlayerId(0) && b.kind == BuildingKind::MawCradle)
            .count()
    };
    assert_eq!(cradles(&state), 2);
    assert!(holds(&state, crew[0]), "the first crew keeps its cradle");
    state.validate_invariants().unwrap();

    let mut units = workers(0, UnitKind::Excavator, 1, 12);
    units.push(unit(0, UnitKind::Maw, 24, 10));
    let mut state = yard(units, vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    assert_eq!(rejected(&state.tick(&[raise(&crew, CRADLE, false)])), None);
    assert_eq!(cradles(&state), 1);
}

#[test]
fn a_faster_worker_takes_a_slower_holders_place_on_a_full_cradle() {
    let mut units = workers(0, UnitKind::Harvester, 4, 12);
    units.push(unit(0, UnitKind::Excavator, 3, 14));
    let mut state = yard(units, vec![]);
    let haulers = ids(&state, 0, UnitKind::Harvester);
    let excavator = ids(&state, 0, UnitKind::Excavator)[0];
    let aside = TilePos::new(20, 14);
    state.tick(&[
        raise(&haulers, CRADLE, false),
        cmd(
            0,
            Command::Run {
                units: vec![haulers[3]],
                goal: aside,
                queue: true,
            },
        ),
    ]);
    assert!(haulers.iter().all(|&id| holds(&state, id)));

    let report = state.tick(&[raise(&[excavator], CRADLE, false)]);
    assert_eq!(rejected(&report), None);
    assert!(holds(&state, excavator), "the Excavator joins the crew");
    assert!(
        haulers[..3].iter().all(|&id| holds(&state, id)),
        "the other holders keep their places"
    );
    // The last-ranked Harvester gives way and takes its next order.
    let bumped = state.unit(haulers[3]).unwrap();
    assert!(
        matches!(bumped.order, Order::Run { goal } if goal.tile() == aside),
        "{:?}",
        bumped.order
    );
}

#[test]
fn a_worker_no_faster_than_the_crew_is_refused_and_keeps_its_program() {
    let mut units = workers(0, UnitKind::Excavator, 5, 12);
    units.push(unit(0, UnitKind::Harvester, 3, 14));
    let mut state = yard(units, vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    let hauler = ids(&state, 0, UnitKind::Harvester)[0];
    state.tick(&[
        raise(&crew[..4], CRADLE, false),
        cmd(
            0,
            Command::Run {
                units: vec![crew[4], hauler],
                goal: TilePos::new(24, 14),
                queue: false,
            },
        ),
    ]);
    let program = state.unit(crew[4]).unwrap().order;
    let mut idle = state.clone();
    idle.tick(&[]);
    let report = state.tick(&[raise(&[crew[4], hauler], CRADLE, false)]);
    assert_eq!(rejected(&report), Some(RejectReason::CrewFull));
    assert_eq!(state.hash(), idle.hash(), "a full crew changes nothing");
    assert_eq!(state.unit(crew[4]).unwrap().order, program);
}

#[test]
fn a_queued_join_is_judged_when_its_turn_comes() {
    // A slower worker whose queued build comes up on a full cradle moves on
    // and says why; a faster one takes the last holder's place.
    let mut units = workers(0, UnitKind::Excavator, 4, 12);
    units.extend(workers(0, UnitKind::Harvester, 4, 16));
    let mut state = yard(units, vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    let haulers = ids(&state, 0, UnitKind::Harvester);
    let step = TilePos::new(4, 14);
    state.tick(&[
        raise(&crew, CRADLE, false),
        cmd(
            0,
            Command::Run {
                units: vec![haulers[0]],
                goal: step,
                queue: false,
            },
        ),
    ]);
    assert_eq!(
        rejected(&state.tick(&[raise(&[haulers[0]], CRADLE, true)])),
        None
    );
    let events = run_until(&mut state, 400, |state, _| {
        !matches!(state.unit(haulers[0]).unwrap().order, Order::Run { .. })
    });
    assert!(!holds(&state, haulers[0]));
    assert!(crew.iter().all(|&id| holds(&state, id)));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::OrderStalled { unit, reason: StallReason::CrewFull, .. } if *unit == haulers[0]
    )));

    // Now a crew of Harvesters, and an Excavator queued to join.
    let mut units = workers(0, UnitKind::Harvester, 4, 12);
    units.push(unit(0, UnitKind::Excavator, 3, 16));
    let mut state = yard(units, vec![]);
    let haulers = ids(&state, 0, UnitKind::Harvester);
    let excavator = ids(&state, 0, UnitKind::Excavator)[0];
    state.tick(&[
        raise(&haulers, CRADLE, false),
        cmd(
            0,
            Command::Run {
                units: vec![excavator],
                goal: step,
                queue: false,
            },
        ),
    ]);
    state.tick(&[raise(&[excavator], CRADLE, true)]);
    run_until(&mut state, 400, |state, _| {
        !matches!(state.unit(excavator).unwrap().order, Order::Run { .. })
    });
    assert!(holds(&state, excavator));
    assert!(haulers[..3].iter().all(|&id| holds(&state, id)));
    assert!(
        !holds(&state, haulers[3]),
        "the last-ranked hauler gave way"
    );
}

#[test]
fn every_seat_watches_a_started_cradle_and_forgets_it_when_cancelled() {
    let mut state = yard(workers(0, UnitKind::Excavator, 2, 12), vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    state.tick(&[raise(&crew, CRADLE, false)]);
    assert!(cradle(&state).is_some());
    assert!(
        known_cradles(&state).is_empty(),
        "a paid, unstarted cradle stays private"
    );

    run_until(&mut state, 600, |state, _| {
        cradle(state).is_some_and(|c| !c.unstarted())
    });
    let site = cradle(&state).unwrap();
    assert!(
        site.tiles().all(|t| !state.vision(PlayerId(1)).visible(t)),
        "East has no sight of the cradle"
    );
    let known = known_cradles(&state);
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].owner, PlayerId(0));
    assert_eq!(known[0].anchor, CRADLE);
    assert_eq!(known[0].progress, site.construction_progress());
    assert!(known[0].progress.is_some_and(|p| p > 0));

    let id = site.id;
    state.tick(&[cmd(0, Command::Cancel { building: id })]);
    assert!(cradle(&state).is_none());
    assert!(known_cradles(&state).is_empty());
    assert!(
        ObservationData::fog_honest(&state, PlayerId(1))
            .enemy_buildings
            .iter()
            .all(|b| b.anchor != CRADLE),
        "no memory of the cradle survives"
    );
}

#[test]
fn a_finished_cradle_stands_up_its_maw_at_its_centre() {
    let mut state = yard(workers(0, UnitKind::Excavator, 4, 12), vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    state.tick(&[raise(&crew, CRADLE, false)]);
    run_until(&mut state, 600, |state, _| {
        cradle(state).is_some_and(|c| !c.unstarted())
    });
    // Nearly done, but only half as healthy as its work raised it.
    let stats = BuildingKind::MawCradle.base_stats();
    let build = stats.construction.unwrap().build_ticks;
    let mut state = forge_cradle(&state, build - 4, stats.max_hp / 2);
    let id = cradle(&state).unwrap().id;
    let events = run_until(&mut state, 200, |_, events| {
        events
            .iter()
            .any(|e| matches!(e, Event::UnitAssembled { .. }))
    });
    assert!(cradle(&state).is_none(), "the cradle leaves the world");
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::BuildingCompleted { building, .. } if *building == id
        )),
        "a cradle never completes as a building"
    );
    let (maw, at) = events
        .iter()
        .find_map(|e| match e {
            Event::UnitAssembled {
                cradle,
                unit,
                kind: UnitKind::Maw,
                player: PlayerId(0),
                pos,
            } if *cradle == id => Some((*unit, *pos)),
            _ => None,
        })
        .expect("the Maw stands up");
    assert_eq!(at, centre(CRADLE));
    let body = state.unit(maw).unwrap();
    assert!(
        body.pos.dist(centre(CRADLE)) < Fx::lit("0.05"),
        "the Maw stands where its cradle stood: {:?}",
        body.pos
    );
    let max = UnitKind::Maw.stats().max_hp;
    assert!(
        body.hp >= max / 2 && body.hp < max,
        "the Maw keeps the cradle's share of health: {}",
        body.hp
    );
    assert!(crew.iter().all(|&id| !holds(&state, id)));
    assert!(
        known_cradles(&state).is_empty(),
        "every seat forgets the cradle"
    );
    assert!(state.validate_invariants().is_ok());
}

#[test]
fn a_destroyed_cradle_leaves_a_wreck_for_the_work_its_crew_raised() {
    let mut units = workers(0, UnitKind::Excavator, 1, 12);
    units.push(unit(1, UnitKind::Sentinel, 8, 9));
    let mut state = yard(units, vec![]);
    let crew = ids(&state, 0, UnitKind::Excavator);
    state.tick(&[raise(&crew, CRADLE, false)]);
    run_until(&mut state, 600, |state, _| {
        cradle(state).is_some_and(|c| !c.unstarted())
    });
    let stats = BuildingKind::MawCradle.base_stats();
    let build = stats.construction.unwrap().build_ticks;
    let mut state = forge_cradle(&state, build / 2, 1);
    let id = cradle(&state).unwrap().id;
    let sentinel = ids(&state, 1, UnitKind::Sentinel)[0];
    let mut progress = 0;
    let mut report = state.tick(&[cmd(
        1,
        Command::Attack {
            units: vec![sentinel],
            target: Target::Building(id).into(),
            queue: false,
        },
    )]);
    for _ in 0..200 {
        if report
            .events
            .iter()
            .any(|e| matches!(e, Event::BuildingDestroyed { building, .. } if *building == id))
        {
            break;
        }
        progress = cradle(&state).unwrap().construction_progress().unwrap();
        report = state.tick(&[]);
    }
    assert!(cradle(&state).is_none(), "the cradle fell");
    let start = u64::from(stats.max_hp / 5);
    let raised = start + (u64::from(stats.max_hp) - start) * u64::from(progress) / u64::from(build);
    let full = u64::from(
        UnitKind::Maw.stats().cost * oxide_sim::stats::WRECK_VALUE_NUM
            / oxide_sim::stats::WRECK_VALUE_DEN,
    );
    let (w, h) = BuildingKind::MawCradle.size();
    // Each footprint tile takes a whole share.
    let tiles = u64::try_from(w * h).unwrap();
    let expected = full * raised / u64::from(stats.max_hp) / tiles * tiles;
    let left: u64 = (0..h)
        .flat_map(|dy| (0..w).map(move |dx| CRADLE.offset(dx, dy)))
        .map(|t| u64::from(state.map().wreck_at(t)))
        .sum();
    assert_eq!(left, expected, "the wreck holds the share the crew raised");
    assert!(
        left < full,
        "a half-raised cradle leaves less than a full wreck"
    );
}

#[test]
fn a_wounded_maw_eats_wreck_beside_its_hull_and_nothing_further() {
    let state = yard(vec![unit(0, UnitKind::Maw, 24, 10)], vec![]);
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let Mend::Kiln(kiln) = UnitKind::Maw.stats().mend else {
        panic!("the Maw has a kiln");
    };
    let beside = TilePos::new(26, 10);
    let beyond = TilePos::new(29, 10);
    let mut state = forge_wreck(&state, &[beside, beyond], 5);

    // At full health the kiln stays cold: the wreck beside it decays only
    // as the one out of reach does.
    to_bite(&mut state);
    state.tick(&[]);
    assert_eq!(state.map().wreck_at(beside), state.map().wreck_at(beyond));
    assert_eq!(state.kiln_fuel(maw), None);

    let max = UnitKind::Maw.stats().max_hp;
    let mut state = forge_hp(&state, maw, max - 50);
    assert_eq!(state.kiln_fuel(maw), Some(beside));
    to_bite(&mut state);
    let report = state.tick(&[]);
    assert_eq!(state.unit(maw).unwrap().hp, max - 50 + kiln.hp_per_wreck);
    assert_eq!(
        state.map().wreck_at(beside) + 1,
        state.map().wreck_at(beyond),
        "one unit of wreck per bite, none from beyond the intake's reach"
    );
    assert!(report.events.iter().any(|e| matches!(
        e,
        Event::UnitRepaired {
            unit,
            source: UnitRepairSource::Kiln { tile },
            amount,
            ..
        } if *unit == maw && *tile == beside && *amount == kiln.hp_per_wreck
    )));
}

#[test]
fn mirrored_maws_eat_mirrored_wreck() {
    // Two wrecks at equal distance from each Maw, one above and one below,
    // so only the tie-break decides which feeds first.
    let west = TilePos::new(14, 14);
    let mirror = |t: TilePos| {
        TilePos::new(
            i32::try_from(WIDTH).unwrap() - 1 - t.x,
            i32::try_from(HEIGHT).unwrap() - 1 - t.y,
        )
    };
    let east = mirror(west);
    let state = yard(
        vec![
            unit(0, UnitKind::Maw, west.x, west.y),
            unit(1, UnitKind::Maw, east.x, east.y),
        ],
        vec![],
    );
    let maws = [
        ids(&state, 0, UnitKind::Maw)[0],
        ids(&state, 1, UnitKind::Maw)[0],
    ];
    let above = TilePos::new(west.x, west.y - 2);
    let below = TilePos::new(west.x, west.y + 2);
    let mut state = forge_wreck(&state, &[above, below, mirror(above), mirror(below)], 5);
    let max = UnitKind::Maw.stats().max_hp;
    for maw in maws {
        state = forge_hp(&state, maw, max - 50);
    }
    to_bite(&mut state);
    state.tick(&[]);
    let Mend::Kiln(kiln) = UnitKind::Maw.stats().mend else {
        panic!("the Maw has a kiln");
    };
    for maw in maws {
        assert_eq!(state.unit(maw).unwrap().hp, max - 50 + kiln.hp_per_wreck);
    }
    for tile in [above, below] {
        assert_eq!(
            state.map().wreck_at(tile),
            state.map().wreck_at(mirror(tile)),
            "mirrored Maws ate different tiles"
        );
    }
}

#[test]
fn a_maw_takes_no_weld_no_repair_bay_and_no_retirement() {
    let state = yard(
        vec![
            unit(0, UnitKind::Maw, 24, 10),
            unit(0, UnitKind::Excavator, 21, 12),
            unit(0, UnitKind::Sentinel, 24, 13),
        ],
        vec![building(0, BuildingKind::RepairBay, 26, 12)],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let welder = ids(&state, 0, UnitKind::Excavator)[0];
    // A wounded Sentinel beside the Maw shows the Bay at work.
    let sentinel = ids(&state, 0, UnitKind::Sentinel)[0];
    let max = UnitKind::Maw.stats().max_hp;
    let state = forge_hp(&state, sentinel, UnitKind::Sentinel.stats().max_hp / 2);
    let mut state = forge_hp(&state, maw, max - 500);
    let report = state.tick(&[cmd(
        0,
        Command::RepairUnit {
            units: vec![welder],
            target: maw,
            queue: false,
        },
    )]);
    assert_eq!(rejected(&report), Some(RejectReason::InvalidTarget));
    let report = state.tick(&[cmd(
        0,
        Command::Retire {
            units: vec![maw],
            queue: false,
        },
    )]);
    assert_eq!(rejected(&report), Some(RejectReason::NoValidUnits));
    let events = run(&mut state, 400);
    assert_eq!(
        state.unit(maw).unwrap().hp,
        max - 500,
        "no Bay pulse and no weld"
    );
    assert!(state.unit(sentinel).unwrap().hp > UnitKind::Sentinel.stats().max_hp / 2);
    assert!(!events.iter().any(|e| matches!(
        e,
        Event::UnitRepaired { unit, .. } if *unit == maw
    )));
}

#[test]
fn a_sapper_charge_strikes_a_maw_as_a_structure() {
    let mut state = yard(
        vec![
            unit(0, UnitKind::Maw, 24, 10),
            unit(1, UnitKind::Sapper, 28, 10),
        ],
        vec![],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let sapper = ids(&state, 1, UnitKind::Sapper)[0];
    state.tick(&[cmd(
        1,
        Command::Attack {
            units: vec![sapper],
            target: Target::Unit(maw).into(),
            queue: false,
        },
    )]);
    run_until(&mut state, 400, |state, _| state.unit(sapper).is_none());
    let charge = UnitKind::Sapper.stats().demolition.unwrap();
    assert_eq!(
        state.unit(maw).unwrap().hp,
        UnitKind::Maw.stats().max_hp - charge.structure_damage
    );
}

#[test]
fn a_maw_cuts_the_nearest_building_before_any_machine() {
    // The Sentinel stands closer, yet the Maw takes the Turret.
    let state = yard(
        vec![
            unit(0, UnitKind::Maw, 24, 10),
            unit(1, UnitKind::Sentinel, 22, 10),
        ],
        vec![building(1, BuildingKind::Turret, 27, 10)],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let turret = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Turret)
        .unwrap()
        .id;
    let mut state = state;
    state.tick(&[]);
    let order = state.unit(maw).unwrap().order;
    let Order::Attack { target, .. } = order else {
        panic!("the Maw picked no fight: {order:?}");
    };
    assert_eq!(
        state
            .attack_view(PlayerId(0), target)
            .and_then(|view| view.entity),
        Some(Target::Building(turret)),
        "{order:?}"
    );
}

#[test]
fn a_maw_never_turns_on_a_machine() {
    // Fire from beyond its reach draws no answer.
    let mut state = yard(
        vec![
            unit(0, UnitKind::Maw, 24, 10),
            unit(1, UnitKind::Lancer, 30, 10),
        ],
        vec![],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let lancer = ids(&state, 1, UnitKind::Lancer)[0];
    let start = state.unit(maw).unwrap().pos;
    state.tick(&[cmd(
        1,
        Command::Attack {
            units: vec![lancer],
            target: Target::Unit(maw).into(),
            queue: false,
        },
    )]);
    run_until(&mut state, 400, |state, _| {
        state.unit(maw).unwrap().hp < UnitKind::Maw.stats().max_hp
    });
    run(&mut state, 100);
    let body = state.unit(maw).unwrap();
    assert_eq!(body.order, Order::Idle, "the Maw never answers the Lancer");
    assert_eq!(body.pos, start);
}

#[test]
fn a_maw_crushes_a_small_machine_under_its_front() {
    // A Sentinel driven against the Maw's front is crushed where it stands;
    // one against its back is left alone. The Maw neither turns nor moves
    // for either.
    for (x, front) in [(28, true), (20, false)] {
        let mut state = yard(
            vec![
                unit(0, UnitKind::Maw, 24, 10),
                unit(1, UnitKind::Sentinel, x, 10),
            ],
            vec![],
        );
        let maw = ids(&state, 0, UnitKind::Maw)[0];
        let sentinel = ids(&state, 1, UnitKind::Sentinel)[0];
        let start = state.unit(maw).unwrap().pos;
        face_toward(&mut state, maw, start + Vec2Fx::new(Fx::ONE, Fx::ZERO));
        let heading = state.unit(maw).unwrap().heading;
        state.tick(&[cmd(
            1,
            Command::Run {
                units: vec![sentinel],
                goal: TilePos::new(24, 10),
                queue: false,
            },
        )]);
        let mut crushed = 0;
        for _ in 0..200 {
            let report = state.tick(&[]);
            crushed += report
                .events
                .iter()
                .filter(|e| {
                    matches!(e, Event::AttackHit { attacker, target: Some(Target::Unit(t)), .. }
                        if *attacker == maw && *t == sentinel)
                })
                .count();
            let body = state.unit(maw).unwrap();
            assert_eq!((body.pos, body.heading), (start, heading));
            assert_eq!(body.order, Order::Idle);
        }
        if front {
            assert!(state.unit(sentinel).is_none(), "{crushed} crushing hits");
        } else {
            assert_eq!(crushed, 0);
            let sentinel = state.unit(sentinel).unwrap();
            assert_eq!(sentinel.hp, UnitKind::Sentinel.stats().max_hp);
        }
    }
}

#[test]
fn a_maw_holds_its_ground_against_small_bodies() {
    let mut state = yard(
        std::iter::once(unit(0, UnitKind::Maw, 24, 10))
            .chain((7..14).map(|y| unit(0, UnitKind::Sentinel, 18, y)))
            .collect(),
        vec![],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let sentinels = ids(&state, 0, UnitKind::Sentinel);
    let start = state.unit(maw).unwrap().pos;
    state.tick(&[cmd(
        0,
        Command::Run {
            units: sentinels,
            goal: TilePos::new(30, 10),
            queue: false,
        },
    )]);
    for _ in 0..300 {
        state.tick(&[]);
        assert_eq!(state.unit(maw).unwrap().pos, start);
    }
}

#[test]
fn a_maw_cutter_deals_structure_damage_to_structures() {
    let hits = |victim: UnitSpec, buildings: Vec<BuildingSpec>| {
        let ordered = buildings.is_empty();
        let mut state = yard(vec![unit(0, UnitKind::Maw, 24, 10), victim], buildings);
        let maw = ids(&state, 0, UnitKind::Maw)[0];
        let victims: Vec<UnitId> = state
            .units()
            .iter()
            .filter(|u| u.player == PlayerId(1))
            .map(|u| u.id)
            .collect();
        // A Maw cuts a machine only when ordered to, and a building of its
        // own accord.
        let mut orders = vec![cmd(
            1,
            Command::Run {
                units: victims.clone(),
                goal: TilePos::new(24, 10),
                queue: false,
            },
        )];
        if ordered {
            orders.push(cmd(
                0,
                Command::Attack {
                    units: vec![maw],
                    target: Target::Unit(victims[0]).into(),
                    queue: false,
                },
            ));
        }
        state.tick(&orders);
        let mut lost = Vec::new();
        for _ in 0..200 {
            let before: Vec<(Target, u32)> = state
                .units()
                .iter()
                .filter(|u| u.player == PlayerId(1))
                .map(|u| (Target::Unit(u.id), u.hp))
                .chain(
                    state
                        .buildings()
                        .iter()
                        .filter(|b| b.player == PlayerId(1) && b.kind != BuildingKind::Foundry)
                        .map(|b| (Target::Building(b.id), b.hp)),
                )
                .collect();
            let report = state.tick(&[]);
            for event in &report.events {
                if let Event::AttackHit {
                    attacker,
                    target: Some(target),
                    ..
                } = event
                    && *attacker == maw
                {
                    let (_, hp) = before.iter().find(|(t, _)| t == target).unwrap();
                    let now = match target {
                        Target::Unit(id) => state.unit(*id).map_or(0, |u| u.hp),
                        Target::Building(id) => state.building(*id).map_or(0, |b| b.hp),
                    };
                    lost.push(hp - now);
                }
            }
            if !lost.is_empty() {
                return lost[0];
            }
        }
        panic!("the Maw never cut");
    };
    let cutter = UnitKind::Maw.stats().weapons[0];
    assert_eq!(
        hits(unit(1, UnitKind::Breaker, 27, 10), vec![]),
        cutter.damage
    );
    assert_eq!(
        hits(unit(1, UnitKind::Maw, 27, 10), vec![]),
        cutter.building_damage.unwrap(),
        "an enemy Maw is a structure to the cutter"
    );
    assert_eq!(
        hits(
            unit(1, UnitKind::Harvester, 32, 3),
            vec![building(1, BuildingKind::Barricade, 26, 10)]
        ),
        cutter.building_damage.unwrap()
    );
}

/// A Maw's first cut on a building.
struct Cut {
    /// Where the Maw stood.
    stood: Vec2Fx,
    /// Which way its hull faced.
    heading: u8,
    /// What it aimed at.
    aim: Vec2Fx,
    /// The surface as the cut's tick began. An upgrade's clock can redraw
    /// the outline later in the cut's own tick.
    met: oxide_sim::building_contact::Surface,
}

/// The outward normal of a side from `a` to `b` of an outline, which winds
/// clockwise on screen.
fn outward((a, b): (Vec2Fx, Vec2Fx)) -> Vec2Fx {
    let along = b - a;
    Vec2Fx::new(along.y, -along.x) / along.length()
}

/// Whether a hull facing `heading` stands square to a side of `surface`.
fn square_to(surface: oxide_sim::building_contact::Surface, heading: u8) -> bool {
    let forward = chassis::compass::dir(heading);
    surface.sides().into_iter().any(|side| {
        let inward = -outward(side);
        forward.x * inward.x + forward.y * inward.y >= Fx::lit("0.99")
    })
}

/// Whether a Maw standing at `stood`, its hull facing `heading`, cut `aim`
/// with its drum: in the band ahead of its hull's front no deeper than the
/// drum presses, and within the drum's width.
fn on_drum(stood: Vec2Fx, heading: u8, aim: Vec2Fx) -> bool {
    let stats = UnitKind::Maw.stats();
    let forward = chassis::compass::dir(heading);
    let out = aim - stood;
    let ahead = out.x * forward.x + out.y * forward.y;
    let across = out.x * forward.y - out.y * forward.x;
    ahead > Fx::ONE
        && ahead <= stats.radius + stats.contact_reach.unwrap() + Fx::lit("0.04")
        && across.abs() < Fx::ONE
}

/// A lone Maw ordered from `from` onto an enemy `kind` at `anchor`, on a
/// sandbox field edited by `carve`, with the building's record edited by
/// `forge`, run to its first cut.
fn first_cut(
    kind: BuildingKind,
    anchor: TilePos,
    from: TilePos,
    carve: impl Fn(&mut Vec<Vec<char>>),
    forge: impl Fn(&mut serde_json::Value),
) -> Cut {
    let mut scenario = open_arena_with(32, 32, vec![unit(0, UnitKind::Maw, from.x, from.y)], carve);
    scenario.mode = ScenarioMode::Sandbox;
    scenario.buildings = vec![building(1, kind, anchor.x, anchor.y)];
    let mut doc = serde_json::to_value(scenario.build().unwrap()).unwrap();
    for b in doc["buildings"].as_array_mut().unwrap() {
        if b["anchor"] == serde_json::json!(anchor) {
            forge(b);
        }
    }
    let mut state: State = serde_json::from_value(doc).unwrap();
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let target = state
        .buildings()
        .iter()
        .find(|b| b.anchor == anchor)
        .unwrap()
        .id;
    cut_by(&mut state, maw, target)
}

/// The first cut of `maw` ordered onto `target`.
fn cut_by(state: &mut State, maw: UnitId, target: oxide_sim::BuildingId) -> Cut {
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![maw],
            target: AttackTarget::Building(target),
            queue: false,
        },
    )]);
    let surface = |state: &State| state.contact_surface(state.building(target).unwrap());
    let mut met = surface(state);
    let mut cut = None;
    run_until(state, 400, |state, events| {
        cut = events.iter().find_map(|event| match event {
            Event::AttackHit {
                attacker,
                attacker_pos,
                target_pos,
                ..
            } if *attacker == maw => Some((*attacker_pos, *target_pos)),
            _ => None,
        });
        if cut.is_none() {
            met = surface(state);
        }
        cut.is_some()
    });
    let (stood, aim) = cut.unwrap();
    Cut {
        stood,
        heading: state.unit(maw).unwrap().heading,
        aim,
        met,
    }
}

#[test]
fn a_maw_cuts_a_building_square_to_a_side_from_any_side() {
    // Even driving in past a corner, it cuts no corner on the way: it cuts
    // with its drum square to a side, at every tier, built or raised to its
    // first two construction stages.
    let anchor = TilePos::new(15, 15);
    for kind in BuildingKind::ALL {
        if kind.is_stealthy() || kind.assembles().is_some() {
            continue;
        }
        for tier in 0..u8::try_from(kind.tiers().len()).unwrap() {
            let labor = kind
                .tier_stats(tier)
                .construction
                .map_or(0, |construction| construction.build_ticks);
            let phase = if tier == 0 { "site" } else { "upgrading" };
            // A site no work has touched is abandoned without a builder.
            for progress in [None, Some(1), Some(labor.div_ceil(3))] {
                let forge = |b: &mut serde_json::Value| {
                    b["tier"] = serde_json::json!(tier);
                    if let Some(progress) = progress {
                        b["phase"] = serde_json::json!({"phase": phase, "progress": progress});
                    }
                };
                for (dx, dy) in [
                    (5, 0),
                    (4, 4),
                    (0, 5),
                    (-4, 4),
                    (-5, 0),
                    (-4, -4),
                    (0, -5),
                    (4, -4),
                ] {
                    let from = anchor.offset(dx, dy);
                    let shown = format!("{kind:?} tier {tier} at {progress:?} from {from:?}");
                    let cut = first_cut(kind, anchor, from, |_| {}, forge);
                    assert!(
                        square_to(cut.met, cut.heading),
                        "{shown} faced {}, askew to every side",
                        cut.heading
                    );
                    assert!(
                        on_drum(cut.stood, cut.heading, cut.aim),
                        "{shown} stood at {:?} facing {}, cut {:?} off its drum",
                        cut.stood,
                        cut.heading,
                        cut.aim
                    );
                }
            }
        }
    }
}

/// A Maw for seat 0 at rest at `pos` facing `heading`, beside the
/// `buildings` (owner, kind, anchor), the first of them its target.
fn maw_beside(
    pos: Vec2Fx,
    heading: u8,
    buildings: &[(u8, BuildingKind, TilePos)],
) -> (State, UnitId, oxide_sim::BuildingId) {
    let tile = TilePos::containing(pos);
    let mut scenario =
        open_arena_with(32, 32, vec![unit(0, UnitKind::Maw, tile.x, tile.y)], |_| {});
    scenario.mode = ScenarioMode::Sandbox;
    scenario.buildings = buildings
        .iter()
        .map(|&(owner, kind, at)| building(owner, kind, at.x, at.y))
        .collect();
    let mut doc = serde_json::to_value(scenario.build().unwrap()).unwrap();
    doc["units"][0]["pos"] = serde_json::json!(pos);
    doc["units"][0]["heading"] = serde_json::json!(heading);
    let state: State = serde_json::from_value(doc).unwrap();
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let target = state
        .buildings()
        .iter()
        .find(|b| b.anchor == buildings[0].2)
        .unwrap()
        .id;
    (state, maw, target)
}

/// The middle of the east side of a `kind` at `anchor`, and its outward
/// normal.
fn east_side(kind: BuildingKind, anchor: TilePos) -> (Vec2Fx, Vec2Fx) {
    let mut scenario = open_arena_with(32, 32, vec![], |_| {});
    scenario.mode = ScenarioMode::Sandbox;
    scenario.buildings = vec![building(1, kind, anchor.x, anchor.y)];
    let state = scenario.build().unwrap();
    let b = state
        .buildings()
        .iter()
        .find(|b| b.anchor == anchor)
        .unwrap();
    let side = state
        .contact_surface(b)
        .sides()
        .into_iter()
        .max_by_key(|&side| outward(side).x)
        .unwrap();
    ((side.0 + side.1) / Fx::from_num(2), outward(side))
}

/// Every building `maw` strikes, in order, with its first stroke once
/// ordered onto `target`.
fn first_stroke(
    state: &mut State,
    maw: UnitId,
    target: oxide_sim::BuildingId,
    within: u64,
) -> Vec<oxide_sim::BuildingId> {
    let struck = |events: &[Event]| -> Vec<oxide_sim::BuildingId> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::AttackHit {
                    attacker,
                    target: Some(Target::Building(id)),
                    ..
                } if *attacker == maw => Some(*id),
                _ => None,
            })
            .collect()
    };
    let report = state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![maw],
            target: AttackTarget::Building(target),
            queue: false,
        },
    )]);
    let mut first = struck(&report.events);
    if first.is_empty() {
        run_until(state, within, |_, events| {
            first = struck(events);
            !first.is_empty()
        });
    }
    first
}

#[test]
fn a_maw_pressed_flat_against_a_side_off_its_middle_cuts_where_it_stands() {
    // Square to a Bastion's east side a third of a tile off its middle, its
    // drum within reach of the side.
    let anchor = TilePos::new(15, 15);
    let (middle, outward) = east_side(BuildingKind::Bastion, anchor);
    let stats = UnitKind::Maw.stats();
    let pos = middle
        + outward * (stats.radius + stats.contact_reach.unwrap() / 2)
        + Vec2Fx::new(Fx::ZERO, Fx::lit("0.33"));
    let heading = chassis::compass::heading_of(-outward);
    let (mut state, maw, bastion) = maw_beside(pos, heading, &[(1, BuildingKind::Bastion, anchor)]);
    assert_eq!(first_stroke(&mut state, maw, bastion, 40), vec![bastion]);
    let unit = state.unit(maw).unwrap();
    assert_eq!((unit.pos, unit.heading), (pos, heading));
}

#[test]
fn a_maw_askew_to_a_side_squares_up_before_it_cuts() {
    // Within reach of a Bastion's east side but turned an eighth of a turn
    // from it, the Maw turns square where it stands and only then cuts.
    let anchor = TilePos::new(15, 15);
    let (middle, outward) = east_side(BuildingKind::Bastion, anchor);
    let stats = UnitKind::Maw.stats();
    let pos = middle + outward * (stats.radius + stats.contact_reach.unwrap() / 2);
    let square = chassis::compass::heading_of(-outward);
    let (mut state, maw, bastion) = maw_beside(
        pos,
        square.wrapping_add(32),
        &[(1, BuildingKind::Bastion, anchor)],
    );
    assert_eq!(first_stroke(&mut state, maw, bastion, 200), vec![bastion]);
    let unit = state.unit(maw).unwrap();
    assert_eq!(unit.pos, pos);
    assert!(square_to(
        state.contact_surface(state.building(bastion).unwrap()),
        unit.heading
    ));
}

#[test]
fn a_maw_off_a_corner_drives_to_a_side_before_it_cuts() {
    // Stopped off a Barricade's corner, within reach of it but with no side
    // under its drum, the Maw never cuts from there: it drives on until its
    // drum presses flat against a side.
    let stood = Vec2Fx::new(Fx::lit("14"), Fx::lit("14.203125"));
    let mut scenario = open_arena_with(32, 32, vec![unit(0, UnitKind::Maw, 8, 8)], |_| {});
    scenario.mode = ScenarioMode::Sandbox;
    scenario.buildings = vec![building(1, BuildingKind::Barricade, 15, 15)];
    let mut doc = serde_json::to_value(scenario.build().unwrap()).unwrap();
    doc["units"][0]["pos"] = serde_json::json!(stood);
    doc["units"][0]["heading"] = serde_json::json!(0);
    let mut state: State = serde_json::from_value(doc).unwrap();
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let barricade = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Barricade)
        .unwrap()
        .id;
    let cut = cut_by(&mut state, maw, barricade);
    assert_ne!(cut.stood, stood);
    assert!(square_to(cut.met, cut.heading));
    assert!(on_drum(cut.stood, cut.heading, cut.aim), "{:?}", cut.aim);
}

#[test]
fn a_maw_cuts_every_hostile_building_its_drum_presses_flat_in_one_stroke() {
    // Two Barricades stand one above the other. A Maw square to their east
    // sides, its drum across both, cuts the lower with each stroke at the
    // upper, unless the lower is its own.
    let (upper, lower) = (TilePos::new(15, 15), TilePos::new(15, 16));
    let (middle, outward) = east_side(BuildingKind::Barricade, upper);
    let stats = UnitKind::Maw.stats();
    let pos = Vec2Fx::new(
        middle.x + stats.radius + stats.contact_reach.unwrap() / 2,
        Fx::from_num(lower.y),
    );
    let cut = stats.weapons[0].building_damage.unwrap();
    for owner in [1, 0] {
        let (mut state, maw, target) = maw_beside(
            pos,
            chassis::compass::heading_of(-outward),
            &[
                (1, BuildingKind::Barricade, upper),
                (owner, BuildingKind::Barricade, lower),
            ],
        );
        let beside = state
            .buildings()
            .iter()
            .find(|b| b.anchor == lower)
            .unwrap()
            .id;
        let hp = |state: &State, id| state.building(id).unwrap().hp;
        let full = (hp(&state, target), hp(&state, beside));
        let struck = first_stroke(&mut state, maw, target, 40);
        let after = (hp(&state, target), hp(&state, beside));
        if owner == 1 {
            assert_eq!(struck.len(), 2);
            assert!(struck.contains(&target) && struck.contains(&beside));
            assert_eq!(after, (full.0 - cut, full.1 - cut));
        } else {
            assert_eq!(struck, vec![target]);
            assert_eq!(after, (full.0 - cut, full.1));
        }
    }
}

#[test]
fn a_maw_cuts_a_way_through_a_fence_it_cannot_pass() {
    // Barricades on every other tile fence the Maw off from a Turret: no
    // gap is wide enough for its hull. It cuts through the fence where it
    // stands, then cuts the Turret down, leaving the rest of the fence.
    let turret_at = TilePos::new(15, 16);
    let mut buildings = vec![(1, BuildingKind::Turret, turret_at)];
    buildings.extend(
        (1..31)
            .step_by(2)
            .map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 14))),
    );
    let (mut state, maw, turret) = maw_beside(
        Vec2Fx::new(Fx::lit("15.5"), Fx::lit("11.5")),
        64,
        &buildings,
    );
    let first = first_stroke(&mut state, maw, turret, 200);
    let struck = state.building(first[0]).unwrap();
    assert_eq!(struck.kind, BuildingKind::Barricade);
    assert!((13..=17).contains(&struck.anchor.x), "{:?}", struck.anchor);
    run_until(&mut state, 600, |state, _| state.building(turret).is_none());
    let left = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Barricade)
        .count();
    assert!(
        left + 4 >= buildings.len() - 1,
        "it cut a way through, not the fence: {left} left"
    );
}

#[test]
fn a_maw_cuts_through_a_wall_in_its_way() {
    // A Barricade wall stands between the Maw and a Turret, sealed against
    // the arena's edges or open at both ends. Either way the Maw presses
    // flat on the wall where its way meets it rather than driving around,
    // cuts a way through, then cuts the Turret down.
    for wall in [1..31, 5..27] {
        let turret_at = TilePos::new(15, 16);
        let mut buildings = vec![(1, BuildingKind::Turret, turret_at)];
        buildings.extend(wall.map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 14))));
        let (mut state, maw, turret) = maw_beside(
            Vec2Fx::new(Fx::lit("15.5"), Fx::lit("11.5")),
            64,
            &buildings,
        );
        let first = first_stroke(&mut state, maw, turret, 200);
        let struck = state.building(first[0]).unwrap();
        assert_eq!(struck.kind, BuildingKind::Barricade);
        assert!((13..=17).contains(&struck.anchor.x), "{:?}", struck.anchor);
        run_until(&mut state, 600, |state, _| state.building(turret).is_none());
        let left = state
            .buildings()
            .iter()
            .filter(|b| b.kind == BuildingKind::Barricade)
            .count();
        assert!(
            left + 4 >= buildings.len() - 1,
            "it cut a way through, not the wall: {left} left"
        );
    }
}

#[test]
fn a_maw_ordered_at_a_wall_cuts_in_wherever_its_way_meets_it() {
    // Ordered from a few tiles off at one Barricade of a sealed wall, the
    // Maw may first press on a neighbour of its target. It meets every
    // Barricade by its outline, so it cuts in there and on to its target.
    let target_at = TilePos::new(15, 14);
    let mut buildings = vec![(1, BuildingKind::Barricade, target_at)];
    buildings.extend(
        (1..31)
            .filter(|&x| x != target_at.x)
            .map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 14))),
    );
    for (pos, heading) in [
        (Vec2Fx::new(Fx::lit("15.5"), Fx::lit("9.5")), 64),
        (Vec2Fx::new(Fx::lit("16.5"), Fx::lit("9.5")), 40),
        (Vec2Fx::new(Fx::lit("15.5"), Fx::lit("8.5")), 72),
    ] {
        let (mut state, maw, target) = maw_beside(pos, heading, &buildings);
        first_stroke(&mut state, maw, target, 300);
        run_until(&mut state, 600, |state, _| state.building(target).is_none());
    }
}

/// The Barricades of `state` still standing.
fn barricades_left(state: &State) -> usize {
    state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Barricade)
        .count()
}

#[test]
fn a_hunting_maw_cuts_a_way_through_a_wall_to_its_goal() {
    // A Barricade wall, sealed against the arena's edges or open at both
    // ends, stands across a hunting Maw's way. It cuts a way through where
    // its way meets the wall and drives on to its goal, rather than cutting
    // along the wall one Barricade after another.
    for wall in [1..31, 5..27] {
        let buildings: Vec<_> = wall
            .map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 14)))
            .collect();
        let (mut state, maw, _) = maw_beside(
            Vec2Fx::new(Fx::lit("15.5"), Fx::lit("11.5")),
            64,
            &buildings,
        );
        let goal = TilePos::new(15, 22);
        state.tick(&[cmd(
            0,
            Command::Hunt {
                units: vec![maw],
                goal,
                queue: false,
            },
        )]);
        run_until(&mut state, 600, |state, _| {
            state.unit(maw).unwrap().tile() == goal
        });
        let left = barricades_left(&state);
        assert!(
            left + 4 >= buildings.len(),
            "it cut a way through, not the wall: {left} left"
        );
    }
}

#[test]
fn a_hunting_maw_cuts_into_a_wall_with_another_out_of_sight_beyond() {
    // Two sealed walls stand across a hunting Maw's way, the second out of
    // its sight. Its way through the first counts the unseen ground beyond
    // as open, so it cuts in rather than settling short of its goal.
    let mut buildings: Vec<_> = (1..31)
        .map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 9)))
        .collect();
    buildings.extend((1..31).map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 18))));
    let (mut state, maw, _) =
        maw_beside(Vec2Fx::new(Fx::lit("15.5"), Fx::lit("4.5")), 64, &buildings);
    let goal = TilePos::new(15, 26);
    state.tick(&[cmd(
        0,
        Command::Hunt {
            units: vec![maw],
            goal,
            queue: false,
        },
    )]);
    let events = run_until(&mut state, 1_200, |state, _| {
        state.unit(maw).unwrap().tile() == goal
    });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::OrderStalled { unit, .. } if *unit == maw))
    );
    assert!(barricades_left(&state) + 10 >= buildings.len());
}

#[test]
fn a_hunting_maw_passes_a_wall_beside_its_way() {
    let buildings: Vec<_> = (3..29)
        .map(|x| (1, BuildingKind::Barricade, TilePos::new(x, 14)))
        .collect();
    let (mut state, maw, _) =
        maw_beside(Vec2Fx::new(Fx::lit("2.5"), Fx::lit("11.5")), 0, &buildings);
    let goal = TilePos::new(29, 11);
    state.tick(&[cmd(
        0,
        Command::Hunt {
            units: vec![maw],
            goal,
            queue: false,
        },
    )]);
    run_until(&mut state, 900, |state, _| {
        state.unit(maw).unwrap().tile() == goal
    });
    assert_eq!(barricades_left(&state), buildings.len());
}

#[test]
fn a_maws_drum_jams_the_gun_it_presses_on() {
    // A Turret fires on the Maw driving in, and never again once the drum
    // presses on it.
    let mut state = yard(
        vec![unit(0, UnitKind::Maw, 22, 10)],
        vec![building(1, BuildingKind::Turret, 27, 10)],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let turret = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Turret)
        .unwrap()
        .id;
    let (mut fired, mut cut) = (Vec::new(), None);
    for t in 0..600 {
        let report = state.tick(&[]);
        for event in &report.events {
            match event {
                Event::TurretFired { turret: id, .. } if *id == turret => fired.push(t),
                Event::AttackHit { attacker, .. } if *attacker == maw => {
                    cut.get_or_insert(t);
                }
                _ => {}
            }
        }
        if state.building(turret).is_none() {
            break;
        }
    }
    let cut = cut.expect("the Maw cut the Turret");
    assert!(!fired.is_empty(), "the Turret fired on the Maw driving in");
    assert!(
        fired.iter().all(|&t| t < cut),
        "fired at {fired:?}, first cut at {cut}"
    );
}

/// A lone Maw at `from` ordered onto an enemy Bastion at `anchor` climbing
/// to its first upgrade, `progress` ticks into it.
fn upgrading_bastion(
    anchor: TilePos,
    from: TilePos,
    progress: u32,
) -> (State, UnitId, oxide_sim::BuildingId) {
    let mut scenario =
        open_arena_with(32, 32, vec![unit(0, UnitKind::Maw, from.x, from.y)], |_| {});
    scenario.mode = ScenarioMode::Sandbox;
    scenario.buildings = vec![building(1, BuildingKind::Bastion, anchor.x, anchor.y)];
    let state = forge_upgrade(&scenario.build().unwrap(), anchor, progress);
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let target = state
        .buildings()
        .iter()
        .find(|b| b.anchor == anchor)
        .unwrap()
        .id;
    (state, maw, target)
}

/// `state` with the building at `anchor` `progress` ticks into an upgrade
/// to tier one.
fn forge_upgrade(state: &State, anchor: TilePos, progress: u32) -> State {
    let mut doc = serde_json::to_value(state).unwrap();
    for b in doc["buildings"].as_array_mut().unwrap() {
        if b["anchor"] == serde_json::json!(anchor) {
            b["tier"] = serde_json::json!(1);
            b["phase"] = serde_json::json!({"phase": "upgrading", "progress": progress});
        }
    }
    serde_json::from_value(doc).unwrap()
}

/// An upgrade's clock redraws the outline under a Maw already cutting it:
/// at the upgrade's first construction stage, and again at completion. The
/// Maw cuts on at its cadence, wherever its drum now meets, until the
/// works fall.
#[test]
fn a_maw_cuts_on_while_an_upgrade_redraws_the_outline_under_it() {
    let anchor = TilePos::new(15, 15);
    let labor = BuildingKind::Bastion
        .tier_stats(1)
        .construction
        .unwrap()
        .build_ticks;
    for redraw in [labor.div_ceil(3), labor] {
        let (mut state, maw, target) = upgrading_bastion(anchor, TilePos::new(19, 19), 1);
        let attack = Command::Attack {
            units: vec![maw],
            target: AttackTarget::Building(target),
            queue: false,
        };
        state.tick(&[cmd(0, attack)]);
        let hit = |events: &[Event]| {
            events
                .iter()
                .any(|event| matches!(event, Event::AttackHit { attacker, .. } if *attacker == maw))
        };
        run_until(&mut state, 400, |_, events| hit(events));
        // The clock redraws the outline two ticks from now.
        let mut state = forge_upgrade(&state, anchor, redraw - 2);
        let shown = state.building(target).unwrap().construction_stage();
        let (mut hits, mut redrawn) = (vec![0], None);
        for t in 1..200 {
            let report = state.tick(&[]);
            if hit(&report.events) {
                hits.push(t);
            }
            let Some(b) = state.building(target) else {
                break;
            };
            if redrawn.is_none() && b.construction_stage() != shown {
                redrawn = Some(t);
            }
        }
        assert!(
            state.building(target).is_none(),
            "the works stand: {hits:?}"
        );
        let redrawn = redrawn.unwrap_or_else(|| panic!("never redrawn: {hits:?}"));
        assert!(hits.iter().any(|&t| t > redrawn), "{redrawn} {hits:?}");
        assert!(
            hits.windows(2).all(|pair| pair[1] - pair[0] == hits[1]),
            "the cadence broke at {redrawn}: {hits:?}"
        );
    }
}

/// When an upgrade completes while a Maw drives in, the finished outline
/// moves the side it was making for: it cuts square to one of the new
/// outline's instead.
#[test]
fn a_maw_driving_in_cuts_square_to_the_outline_an_upgrade_redraws() {
    let anchor = TilePos::new(15, 15);
    let labor = BuildingKind::Bastion
        .tier_stats(1)
        .construction
        .unwrap()
        .build_ticks;
    let (mut state, maw, target) =
        upgrading_bastion(anchor, TilePos::new(15, 10), labor * 2 / 3 + 1);
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![maw],
            target: AttackTarget::Building(target),
            queue: false,
        },
    )]);
    run_until(&mut state, 400, |state, _| {
        state
            .unit(maw)
            .unwrap()
            .path
            .as_ref()
            .is_some_and(|path| path.final_point.is_some())
    });
    // The works stand up next tick.
    let mut state = forge_upgrade(&state, anchor, labor - 1);
    let mut cut = None;
    run_until(&mut state, 400, |_, events| {
        cut = events.iter().find_map(|event| match event {
            Event::AttackHit {
                attacker,
                attacker_pos,
                target_pos,
                ..
            } if *attacker == maw => Some((*attacker_pos, *target_pos)),
            _ => None,
        });
        cut.is_some()
    });
    let (stood, aim) = cut.unwrap();
    let b = state.building(target).unwrap();
    assert!(b.built());
    let heading = state.unit(maw).unwrap().heading;
    assert!(square_to(state.contact_surface(b), heading), "{heading}");
    assert!(on_drum(stood, heading, aim), "{stood:?} {heading} {aim:?}");
}

#[test]
fn a_maw_hemmed_in_by_rock_still_cuts_square() {
    // Rock off a Turret's north face and right under it leaves the Maw no
    // room in front of either face: it squares up to a side where it can
    // stand rather than cut a corner or give up.
    let anchor = TilePos::new(15, 15);
    let cut = first_cut(
        BuildingKind::Turret,
        anchor,
        TilePos::new(15, 10),
        |rows| {
            rows[12][15] = '#';
            rows[16][15] = '#';
        },
        |_| {},
    );
    assert!(square_to(cut.met, cut.heading), "{}", cut.heading);
    assert!(
        on_drum(cut.stood, cut.heading, cut.aim),
        "{:?} {} {:?}",
        cut.stood,
        cut.heading,
        cut.aim
    );
}

#[test]
fn a_scenario_cannot_author_a_cradle() {
    let mut scenario = open_arena_with(WIDTH, HEIGHT, Vec::new(), |_| {});
    scenario.buildings = vec![building(0, BuildingKind::MawCradle, CRADLE.x, CRADLE.y)];
    assert!(matches!(
        scenario.build(),
        Err(oxide_sim::scenario::ScenarioError::BadBuilding(0))
    ));
}

#[test]
fn a_maw_answers_a_gun_that_fires_on_it_from_beyond_its_aggro() {
    // The Bastion's edge stands three and a half tiles from the Maw's centre:
    // outside its aggro, inside the Bastion's reach.
    let mut state = yard(
        vec![unit(0, UnitKind::Maw, 24, 10)],
        vec![building(1, BuildingKind::Bastion, 28, 10)],
    );
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let bastion = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Bastion)
        .unwrap()
        .id;
    run_until(&mut state, 600, |state, _| {
        state.unit(maw).unwrap().hp < UnitKind::Maw.stats().max_hp
    });
    run_until(&mut state, 2_400, |state, _| {
        state.building(bastion).is_none()
    });
    assert!(state.unit(maw).is_some(), "the Maw cut the gun down");
}

/// The tiles each of West's Excavators stands on once the crew has been at
/// work a while, and how far the cradle has come.
fn crew_at_work(buildings: Vec<BuildingSpec>) -> (Vec<TilePos>, u32) {
    let mut state = yard(workers(0, UnitKind::Excavator, 4, 14), buildings);
    let crew = ids(&state, 0, UnitKind::Excavator);
    state.tick(&[raise(&crew, CRADLE, false)]);
    run(&mut state, 600);
    let progress = cradle(&state).unwrap().construction_progress().unwrap();
    let mut tiles: Vec<TilePos> = crew
        .iter()
        .map(|id| state.unit(*id).unwrap().tile())
        .collect();
    tiles.sort_by_key(|tile| (tile.y, tile.x));
    (tiles, progress)
}

#[test]
fn a_full_crew_works_from_the_cradles_four_pads() {
    let (tiles, progress) = crew_at_work(vec![]);
    assert!(progress > 0);
    let pads: Vec<TilePos> = BuildingKind::MawCradle
        .crew_pads()
        .unwrap()
        .iter()
        .map(|&(dx, dy)| CRADLE.offset(dx, dy))
        .collect();
    assert_eq!(
        tiles, pads,
        "two pads off the north face, two off the south"
    );
}

#[test]
fn a_pad_lost_to_a_building_returns_the_crew_to_the_ordinary_ring() {
    let (north_east, _) = BuildingKind::MawCradle.crew_pads().unwrap()[1];
    let blocked = CRADLE.offset(north_east, -1);
    let (tiles, progress) = crew_at_work(vec![building(
        0,
        BuildingKind::Barricade,
        blocked.x,
        blocked.y,
    )]);
    let (unblocked, _) = crew_at_work(vec![]);
    assert!(progress > 0, "the crew still raises the cradle");
    assert!(!tiles.contains(&blocked));
    assert_ne!(tiles, unblocked);
}

#[test]
fn a_maw_passes_over_a_structure_it_cannot_reach() {
    // A Barricade walled in by rock stands nearest the Maw, a second one in
    // the open just as near. The Maw can meet only the second; with it gone,
    // the Maw stands idle instead of picking the walled one every tick.
    let scene = |open_barricade: bool| {
        let mut scenario = open_arena_with(24, 16, vec![unit(0, UnitKind::Maw, 10, 9)], |rows| {
            for (x, y) in [
                (9, 5),
                (10, 5),
                (11, 5),
                (9, 6),
                (11, 6),
                (9, 7),
                (10, 7),
                (11, 7),
            ] {
                rows[y][x] = '#';
            }
        });
        scenario.buildings = vec![building(1, BuildingKind::Barricade, 10, 6)];
        if open_barricade {
            scenario
                .buildings
                .push(building(1, BuildingKind::Barricade, 13, 9));
        }
        scenario.build().unwrap()
    };
    let mut state = scene(true);
    let maw = ids(&state, 0, UnitKind::Maw)[0];
    let open = state
        .buildings()
        .iter()
        .find(|b| b.kind == BuildingKind::Barricade && b.anchor == TilePos::new(13, 9))
        .unwrap()
        .id;
    state.tick(&[]);
    let Order::Attack { target, .. } = state.unit(maw).unwrap().order else {
        panic!("the Maw picked no fight");
    };
    assert_eq!(
        state
            .attack_view(PlayerId(0), target)
            .and_then(|view| view.entity),
        Some(Target::Building(open))
    );
    let mut state = scene(false);
    for _ in 0..30 {
        let report = state.tick(&[]);
        assert_eq!(state.unit(maw).unwrap().order, Order::Idle);
        assert!(
            !report
                .events
                .iter()
                .any(|e| matches!(e, Event::OrderStalled { unit, .. } if *unit == maw)),
            "the Maw never sets out for what it cannot meet"
        );
    }
}
