//! The Roc: two guns on fixed pivots that pick their own ground targets
//! inside their arcs, the helm rules that turn its hull for them, its
//! transport and crash, and its cradle.

use crate::common::{building, cmd, face_toward, open_arena, players, run, unit};
use aftermine_sim::scenario::UnitSpec;
use aftermine_sim::state::IdleFacing;
use aftermine_sim::{
    AttackTarget, BuildingKind, Command, Event, Order, PlayerId, Scenario, State, Target, UnitId,
    UnitKind,
};
use chassis::compass::{dir, heading_of};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

const WIDTH: usize = 30;
const HEIGHT: usize = 22;

fn staged(units: Vec<UnitSpec>) -> State {
    let mut scenario = open_arena(WIDTH, HEIGHT, units);
    scenario.players = players(0);
    scenario.build().unwrap()
}

fn ids(state: &State, player: u8, kind: UnitKind) -> Vec<UnitId> {
    state
        .units()
        .iter()
        .filter(|u| u.player.0 == player && u.kind == kind)
        .map(|u| u.id)
        .collect()
}

fn roc(state: &State) -> UnitId {
    ids(state, 0, UnitKind::Roc)[0]
}

/// The Roc turned to face `point`.
fn facing(state: &mut State, id: UnitId, point: Vec2Fx) {
    face_toward(state, id, point);
}

/// The Roc's idle facing forged to `facing`.
fn hold_facing(state: &mut State, id: UnitId, facing: &str) {
    let mut value = serde_json::to_value(&*state).unwrap();
    let row = value["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == serde_json::json!(id))
        .unwrap();
    row["mounts"]["facing"] = serde_json::from_str(facing).unwrap();
    *state = serde_json::from_value(value).unwrap();
}

/// Each of `id`'s shots: its weapon slot, origin and victim.
fn shots(events: &[Event], id: UnitId) -> Vec<(usize, Vec2Fx, Option<Target>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::AttackHit {
                attacker,
                weapon,
                attacker_pos,
                target,
                ..
            } if *attacker == id => Some((*weapon, *attacker_pos, *target)),
            _ => None,
        })
        .collect()
}

fn slots(events: &[Event], id: UnitId) -> Vec<usize> {
    let mut slots: Vec<usize> = shots(events, id).iter().map(|(slot, ..)| *slot).collect();
    slots.sort_unstable();
    slots.dedup();
    slots
}

/// Where gun `slot` of `id` stands now.
fn pivot(state: &State, id: UnitId, slot: usize) -> Vec2Fx {
    let unit = state.unit(id).unwrap();
    unit.kind.stats().mounts.unwrap().mounts[slot].pivot(unit.pos, unit.heading)
}

#[test]
fn both_guns_fire_ahead_each_from_its_own_pivot() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 8, 10),
        unit(1, UnitKind::Breaker, 12, 10),
    ]);
    let id = roc(&state);
    let breaker = ids(&state, 1, UnitKind::Breaker)[0];
    let ahead = state.unit(breaker).unwrap().pos;
    facing(&mut state, id, ahead);
    let pivots = [pivot(&state, id, 0), pivot(&state, id, 1)];
    let events = run(&mut state, 30);
    let fired = shots(&events, id);
    assert_eq!(slots(&events, id), [0, 1], "{fired:?}");
    for (slot, origin, target) in fired {
        assert_eq!(origin, pivots[slot], "a shot leaves its own gun's pivot");
        assert_eq!(target, Some(Target::Unit(breaker)));
    }
    assert_ne!(pivots[0], pivots[1]);
}

#[test]
fn a_gun_holds_fire_on_its_blind_quarter_while_the_other_bears() {
    // A Breaker behind the starboard wing sits in the port gun's blind
    // quarter and the starboard gun's arc.
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 10, 10),
        unit(1, UnitKind::Breaker, 8, 12),
    ]);
    let id = roc(&state);
    let pos = state.unit(id).unwrap().pos;
    // Nose east: the Breaker is aft and to starboard (+y).
    facing(&mut state, id, pos + Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    hold_facing(&mut state, id, r#"{"facing": "holding", "ticks": 60}"#);
    let events = run(&mut state, 40);
    assert_eq!(slots(&events, id), [1], "{:?}", shots(&events, id));
}

#[test]
fn an_idle_roc_turns_to_feed_a_starved_gun_and_then_holds_its_facing() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 10, 10),
        unit(1, UnitKind::Breaker, 8, 12),
    ]);
    let id = roc(&state);
    let pos = state.unit(id).unwrap().pos;
    facing(&mut state, id, pos + Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    let start = state.unit(id).unwrap().heading;
    let mut both = None;
    for tick in 0..200 {
        let events = state.tick(&[]).events;
        if slots(&events, id) == [0] || both.is_some() {
            both.get_or_insert(tick);
        }
        if let Some(at) = both
            && tick > at + 5
        {
            break;
        }
    }
    let turned = state.unit(id).unwrap().heading;
    assert_ne!(turned, start, "the hull yawed to feed the port gun");
    assert!(
        both.is_some(),
        "the port gun bore on the Breaker once the hull turned"
    );
    // Toward the starboard wing, where the port gun's blind quarter lies.
    assert!(turned.wrapping_sub(start).cast_signed() > 0);
    assert!(matches!(
        state.unit(id).unwrap().mounts.unwrap().facing,
        IdleFacing::Holding { .. }
    ));
    let held = state.unit(id).unwrap().heading;
    run(&mut state, 30);
    assert_eq!(state.unit(id).unwrap().heading, held, "it keeps its facing");
}

#[test]
fn a_moving_roc_never_turns_for_an_enemy_and_fires_on_the_way() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 4, 8),
        unit(1, UnitKind::Breaker, 12, 11),
    ]);
    let id = roc(&state);
    let goal = TilePos::new(24, 8);
    let events = {
        let mut all = state
            .tick(&[cmd(
                0,
                Command::Run {
                    units: vec![id],
                    goal,
                    queue: false,
                },
            )])
            .events;
        all.extend(run(&mut state, 160));
        all
    };
    assert!(!shots(&events, id).is_empty(), "its guns fire on a run");
    let unit = state.unit(id).unwrap();
    assert!(
        unit.heading.wrapping_sub(0).cast_signed().unsigned_abs() <= 4,
        "the nose stays on its travel east: {}",
        unit.heading
    );
    assert!(unit.pos.x > Fx::from_num(14), "it flew on past the Breaker");
}

#[test]
fn an_attack_turns_the_roc_nose_on_and_holds_station() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 10, 6),
        unit(1, UnitKind::Breaker, 10, 10),
    ]);
    let id = roc(&state);
    let breaker = ids(&state, 1, UnitKind::Breaker)[0];
    let pos = state.unit(id).unwrap().pos;
    facing(&mut state, id, pos + Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![id],
            target: AttackTarget::Unit(breaker),
            queue: false,
        },
    )]);
    let events = run(&mut state, 90);
    let unit = state.unit(id).unwrap();
    let toward = heading_of(state.unit(breaker).unwrap().pos - unit.pos);
    assert!(
        toward
            .wrapping_sub(unit.heading)
            .cast_signed()
            .unsigned_abs()
            <= 2,
        "nose-on: heading {} toward {toward}",
        unit.heading
    );
    assert!(unit.path.is_none(), "it holds station in reach");
    assert!(
        unit.pos.dist(pos) < Fx::lit("0.5"),
        "it never closed further"
    );
    assert_eq!(slots(&events, id), [0, 1]);
}

#[test]
fn an_attack_closes_until_a_gun_reaches() {
    // A Kestrel over the Breaker keeps it in sight.
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 3, 10),
        unit(1, UnitKind::Breaker, 20, 10),
        unit(0, UnitKind::Kestrel, 21, 8),
    ]);
    let id = roc(&state);
    let breaker = ids(&state, 1, UnitKind::Breaker)[0];
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![id],
            target: AttackTarget::Unit(breaker),
            queue: false,
        },
    )]);
    let mut fired_at = None;
    for _ in 0..600 {
        let events = state.tick(&[]).events;
        if !shots(&events, id).is_empty() {
            fired_at = Some(state.unit(id).unwrap().pos);
            break;
        }
    }
    let at = fired_at.expect("it closed and fired");
    let target = state.unit(breaker).unwrap().pos;
    assert!(
        at.dist(target) <= Fx::lit("6.5"),
        "it stopped once in reach"
    );
    assert!(
        at.dist(target) >= Fx::lit("4"),
        "it never closed past its reach"
    );
}

#[test]
fn an_attack_on_a_target_under_the_rear_hull_flies_forward_then_fires() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 10, 10),
        unit(1, UnitKind::Breaker, 10, 10),
    ]);
    let id = roc(&state);
    let breaker = ids(&state, 1, UnitKind::Breaker)[0];
    let pos = state.unit(id).unwrap().pos;
    facing(&mut state, id, pos + Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![id],
            target: AttackTarget::Unit(breaker),
            queue: false,
        },
    )]);
    let events = run(&mut state, 240);
    assert!(
        shots(&events, id)
            .iter()
            .any(|(_, _, target)| *target == Some(Target::Unit(breaker))),
        "it moved off the Breaker and brought its guns to bear"
    );
}

#[test]
fn an_attack_move_stops_to_fight_what_it_meets_and_then_flies_on() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 3, 10),
        unit(1, UnitKind::Lancer, 13, 12),
    ]);
    let id = roc(&state);
    let lancer = ids(&state, 1, UnitKind::Lancer)[0];
    let goal = TilePos::new(26, 10);
    state.tick(&[cmd(
        0,
        Command::Hunt {
            units: vec![id],
            goal,
            queue: false,
        },
    )]);
    let mut stopped = false;
    for _ in 0..400 {
        state.tick(&[]);
        let unit = state.unit(id).unwrap();
        if state.unit(lancer).is_some() && unit.path.is_none() {
            stopped = true;
        }
        if state.unit(lancer).is_none() {
            break;
        }
    }
    assert!(stopped, "it stopped while the Lancer stood in reach");
    assert!(state.unit(lancer).is_none(), "its guns killed the Lancer");
    run(&mut state, 600);
    let unit = state.unit(id).unwrap();
    assert!(
        unit.pos.dist(goal.center()) < Fx::from_num(2),
        "then it flew on to its goal: {:?}",
        unit.pos
    );
}

#[test]
fn a_hostile_that_can_hit_the_hull_outranks_a_nearer_machine() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 8, 10),
        unit(1, UnitKind::Breaker, 10, 10),
        unit(1, UnitKind::Flakhound, 12, 11),
    ]);
    let id = roc(&state);
    let flak = ids(&state, 1, UnitKind::Flakhound)[0];
    let pos = state.unit(id).unwrap().pos;
    facing(&mut state, id, pos + Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    let events = run(&mut state, 20);
    let fired = shots(&events, id);
    assert!(!fired.is_empty());
    assert!(
        fired
            .iter()
            .all(|(_, _, target)| *target == Some(Target::Unit(flak))),
        "{fired:?}"
    );
}

#[test]
fn an_ordered_target_outranks_a_hostile_that_can_hit_the_hull() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 8, 10),
        unit(1, UnitKind::Breaker, 12, 10),
        unit(1, UnitKind::Flakhound, 11, 11),
    ]);
    let id = roc(&state);
    let breaker = ids(&state, 1, UnitKind::Breaker)[0];
    let pos = state.unit(id).unwrap().pos;
    facing(&mut state, id, pos + Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![id],
            target: AttackTarget::Unit(breaker),
            queue: false,
        },
    )]);
    let events = run(&mut state, 20);
    let fired = shots(&events, id);
    assert!(!fired.is_empty());
    assert!(
        fired
            .iter()
            .all(|(_, _, target)| *target == Some(Target::Unit(breaker))),
        "{fired:?}"
    );
}

#[test]
fn fire_from_beyond_its_guns_never_pulls_it_off_station() {
    // A Flak Turret outranges the guns: it shoots the Roc, which stays.
    let mut scenario = open_arena(WIDTH, HEIGHT, vec![unit(0, UnitKind::Roc, 8, 10)]);
    scenario.players = players(0);
    scenario
        .buildings
        .push(building(1, BuildingKind::FlakTurret, 14, 10));
    let mut state = scenario.build().unwrap();
    let id = roc(&state);
    let pos = state.unit(id).unwrap().pos;
    // Nose away: its guns reach no farther than 5 from pivots ahead of the
    // centre, while the turret reaches the hull's edge from 5.5.
    facing(&mut state, id, pos - Vec2Fx::new(Fx::from_num(4), Fx::ZERO));
    let events = run(&mut state, 120);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::TurretFired { .. } | Event::AttackHit { .. })),
        "premise: the turret engages"
    );
    let unit = state.unit(id).unwrap();
    assert!(unit.hp < unit.kind.stats().max_hp, "premise: it was hit");
    assert_eq!(unit.order, Order::Idle);
    assert!(unit.pos.dist(pos) < Fx::lit("0.5"));
    assert!(shots(&events, id).is_empty());
}

#[test]
fn half_turned_rocs_fight_half_turned_targets_alike() {
    let width = i32::try_from(WIDTH).unwrap();
    let height = i32::try_from(HEIGHT).unwrap();
    let mirror = |x: i32, y: i32| (width - 1 - x, height - 1 - y);
    let (rx, ry) = mirror(8, 7);
    let (bx, by) = mirror(10, 9);
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 8, 7),
        unit(1, UnitKind::Breaker, 10, 9),
        unit(1, UnitKind::Roc, rx, ry),
        unit(0, UnitKind::Breaker, bx, by),
    ]);
    let west = ids(&state, 0, UnitKind::Roc)[0];
    let east = ids(&state, 1, UnitKind::Roc)[0];
    let pos = state.unit(west).unwrap().pos;
    facing(&mut state, west, pos + dir(0) * Fx::from_num(4));
    let pos = state.unit(east).unwrap().pos;
    facing(&mut state, east, pos + dir(128) * Fx::from_num(4));
    let events = run(&mut state, 80);
    let (w, e) = (shots(&events, west), shots(&events, east));
    assert!(!w.is_empty());
    assert_eq!(w.len(), e.len());
    let centre = Vec2Fx::new(Fx::from_num(width), Fx::from_num(height));
    for ((ws, wp, _), (es, ep, _)) in w.iter().zip(&e) {
        assert_eq!(ws, es, "the same gun of each hull fires");
        assert_eq!(*wp, centre - *ep, "from half-turned pivots");
    }
    let (wh, eh) = (
        state.unit(west).unwrap().heading,
        state.unit(east).unwrap().heading,
    );
    assert_eq!(wh.wrapping_add(128), eh);
}

fn load(state: &mut State, riders: Vec<UnitId>, transport: UnitId) {
    state.tick(&[cmd(
        0,
        Command::Load {
            units: riders,
            transport,
            queue: false,
        },
    )]);
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

#[test]
fn a_roc_carries_twelve_slots_and_turns_the_thirteenth_away() {
    let mut units = vec![unit(0, UnitKind::Roc, 10, 10)];
    units.extend((4..17).map(|x| unit(0, UnitKind::Sentinel, x, 13)));
    let mut state = staged(units);
    let id = roc(&state);
    let riders = ids(&state, 0, UnitKind::Sentinel);
    load(&mut state, riders.clone(), id);
    let mut turned_away = false;
    for _ in 0..400 {
        let events = state.tick(&[]).events;
        turned_away |= events.iter().any(|event| {
            matches!(
                event,
                Event::OrderStalled {
                    reason: aftermine_sim::event::StallReason::TransportFull,
                    ..
                }
            )
        });
        if state.unit(id).unwrap().cargo.len() == 12 && turned_away {
            break;
        }
    }
    assert_eq!(state.unit(id).unwrap().cargo.len(), 12);
    assert!(turned_away, "the thirteenth finds the hold full");
    assert_eq!(ids(&state, 0, UnitKind::Sentinel).len(), 1);
}

#[test]
fn riders_board_a_roc_hovering_over_a_pit_from_its_rim() {
    let mut scenario = crate::common::open_arena_with(
        WIDTH,
        HEIGHT,
        vec![
            unit(0, UnitKind::Roc, 10, 10),
            unit(0, UnitKind::Sentinel, 8, 10),
            unit(0, UnitKind::Sentinel, 12, 10),
        ],
        |rows| {
            for row in &mut rows[9..12] {
                for cell in &mut row[9..12] {
                    *cell = '~';
                }
            }
        },
    );
    scenario.players = players(0);
    let mut state = scenario.build().unwrap();
    let id = roc(&state);
    let riders = ids(&state, 0, UnitKind::Sentinel);
    load(&mut state, riders, id);
    run(&mut state, 120);
    assert_eq!(
        state.unit(id).unwrap().cargo.len(),
        2,
        "riders on the pit's rim reach a wide hull over its middle"
    );
}

#[test]
fn riders_die_with_a_roc_whose_crash_blasts_the_ground_around_it() {
    let mut state = staged(vec![
        unit(0, UnitKind::Roc, 10, 10),
        unit(0, UnitKind::Sentinel, 10, 12),
        unit(1, UnitKind::Flakhound, 14, 10),
        unit(1, UnitKind::Breaker, 11, 8),
    ]);
    let id = roc(&state);
    let rider = ids(&state, 0, UnitKind::Sentinel)[0];
    let breaker = ids(&state, 1, UnitKind::Breaker)[0];
    load(&mut state, vec![rider], id);
    run_until_aboard(&mut state, id, 1);
    let mut state = forge_hp(&state, id, 1);
    let before = state.unit(breaker).unwrap().hp;
    let mut deaths = Vec::new();
    let mut impacted = false;
    for _ in 0..200 {
        for event in state.tick(&[]).events {
            match event {
                Event::UnitDied { unit, .. } => deaths.push(unit),
                Event::AircraftImpacted { .. } => impacted = true,
                _ => {}
            }
        }
        if impacted {
            break;
        }
    }
    assert!(deaths.contains(&id), "the Roc falls");
    assert!(deaths.contains(&rider), "its rider dies with it");
    assert!(impacted);
    let crash = UnitKind::Roc.stats().crash.unwrap();
    assert!(
        state
            .unit(breaker)
            .is_none_or(|b| b.hp <= before - crash.damage),
        "the crash hits a hostile machine under it"
    );
}

fn run_until_aboard(state: &mut State, id: UnitId, riders: usize) {
    for _ in 0..200 {
        if state.unit(id).unwrap().cargo.len() == riders {
            return;
        }
        state.tick(&[]);
    }
    panic!("riders never boarded");
}

/// Where West raises a Roc cradle in [`roc_yard`].
const CRADLE: TilePos = TilePos::new(4, 8);

/// An open yard where West banks two Rocs and has a Crucible and a Deep
/// Bore, and an Airworks when `airworks` is set.
fn roc_yard(airworks: bool) -> State {
    let frame = TilePos::new(8, 1);
    let mut units: Vec<UnitSpec> = (0..4)
        .map(|i| unit(0, UnitKind::Excavator, 3 + i, 12))
        .collect();
    units.push(unit(1, UnitKind::Sentinel, 26, 18));
    let mut scenario = crate::common::open_arena_with(WIDTH, HEIGHT, units, |rows| {
        rows[1][8] = 'E';
    });
    scenario.players = players(2 * UnitKind::Roc.stats().cost);
    scenario.buildings = vec![
        building(0, BuildingKind::Extractor, frame.x, frame.y),
        building(0, BuildingKind::Crucible, 12, 1),
    ];
    if airworks {
        scenario
            .buildings
            .push(building(0, BuildingKind::Airworks, 15, 4));
    }
    let mut doc = serde_json::to_value(scenario.build().unwrap()).unwrap();
    for b in doc["buildings"].as_array_mut().unwrap() {
        if b["kind"] == serde_json::json!("extractor") {
            b["tier"] = serde_json::json!(1);
        }
    }
    serde_json::from_value(doc).unwrap()
}

fn raise_roc(state: &mut State) -> Vec<Event> {
    let crew = ids(state, 0, UnitKind::Excavator);
    state
        .tick(&[cmd(
            0,
            Command::Build {
                units: crew,
                kind: BuildingKind::RocCradle,
                anchor: CRADLE,
                queue: false,
                defer: false,
            },
        )])
        .events
}

#[test]
fn a_roc_cradle_needs_an_airworks_beside_the_maws_prerequisites() {
    let mut bare = roc_yard(false);
    let events = raise_roc(&mut bare);
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::CommandRejected {
                reason: aftermine_sim::command::RejectReason::MissingPrerequisite,
                ..
            }
        )),
        "{events:?}"
    );
    let mut ready = roc_yard(true);
    let events = raise_roc(&mut ready);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::CommandRejected { .. })),
        "{events:?}"
    );
    assert!(
        ready
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::RocCradle)
    );
}

#[test]
fn a_finished_roc_cradle_lifts_its_roc_off_from_its_centre() {
    let mut state = roc_yard(true);
    raise_roc(&mut state);
    let started = |state: &State| {
        state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::RocCradle && !b.unstarted())
    };
    for _ in 0..600 {
        if started(&state) {
            break;
        }
        state.tick(&[]);
    }
    assert!(started(&state), "the crew started the cradle");
    let build = BuildingKind::RocCradle
        .base_stats()
        .construction
        .unwrap()
        .build_ticks;
    let mut doc = serde_json::to_value(&state).unwrap();
    for b in doc["buildings"].as_array_mut().unwrap() {
        if b["kind"] == serde_json::json!("roc_cradle") {
            b["phase"] = serde_json::json!({"phase": "site", "progress": build - 4});
        }
    }
    let mut state: State = serde_json::from_value(doc).unwrap();
    let mut stood = None;
    for _ in 0..200 {
        for event in state.tick(&[]).events {
            if let Event::UnitAssembled {
                unit,
                kind: UnitKind::Roc,
                pos,
                ..
            } = event
            {
                stood = Some((unit, pos));
            }
        }
        if stood.is_some() {
            break;
        }
    }
    let (id, at) = stood.expect("the Roc stands up");
    let (w, h) = BuildingKind::RocCradle.size();
    let centre = Vec2Fx::new(
        Fx::from_num(CRADLE.x * 2 + w) / 2,
        Fx::from_num(CRADLE.y * 2 + h) / 2,
    );
    assert_eq!(at, centre);
    let body = state.unit(id).unwrap();
    assert_eq!(
        body.domain(),
        aftermine_sim::stats::Domain::Air,
        "it lifts off airborne"
    );
    assert!(body.mounts.is_some(), "with its guns");
    assert!(
        !state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::RocCradle)
    );
    assert!(state.validate_invariants().is_ok());
}

#[test]
fn no_gun_fires_through_a_peak_at_a_building_flush_against_it() {
    // A building's aim point is its footprint's closest point, an exact
    // edge that falls in the neighbouring tile. Flush against the ridge's
    // west face with the Roc to the east, that tile is the peak, which the
    // line trace skips as its end tile.
    let mut map = vec!["############^###########".to_string()];
    for y in 1..11 {
        let mut row = String::from("#");
        for x in 1..23 {
            row.push(match (x, y) {
                (1, 1) => '1',
                (10, 5) => '2', // footprint (10,5)-(11,6), flush at x=12
                (12, _) => '^',
                _ => '.',
            });
        }
        row.push('#');
        map.push(row);
    }
    map.push("############^###########".to_string());
    let mut state = Scenario {
        levels: Vec::new(),
        mode: aftermine_sim::scenario::ScenarioMode::Sandbox,
        name: "flush".into(),
        map,
        players: players(0),
        units: vec![unit(0, UnitKind::Roc, 15, 5)],
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap();
    let roc = roc(&state);
    let west = state
        .buildings()
        .iter()
        .find(|b| b.player == PlayerId(1))
        .unwrap()
        .id;
    let hp = state.building(west).unwrap().hp;
    for _ in 0..120 {
        let events = state.tick(&[]).events;
        assert!(
            shots(&events, roc).is_empty(),
            "no gun fires across the ridge"
        );
    }
    assert_eq!(
        state.unit(roc).unwrap().pos.x,
        Fx::from_num(15) + Fx::lit("0.5")
    );
    assert_eq!(state.building(west).unwrap().hp, hp);
}
