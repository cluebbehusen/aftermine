//! A gun with nothing to aim at and its reload done turns back to rest.

use crate::common;

use aftermine_sim::scenario::UnitSpec;
use aftermine_sim::{BuildingId, BuildingKind, Command, Event, State, Target, UnitId, UnitKind};
use chassis::compass::heading_of;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use serde_json::{Value, json};

use common::{aim_defense, building, cmd, open_arena, unit};

const WIDTH: usize = 32;
const HEIGHT: usize = 22;
const TURRETED: [UnitKind; 3] = [UnitKind::Sentinel, UnitKind::Lancer, UnitKind::Warden];

fn centre() -> Vec2Fx {
    Vec2Fx::new(Fx::from_num(WIDTH / 2), Fx::from_num(HEIGHT / 2))
}

/// An East machine beyond every gun's reach.
fn bystander() -> UnitSpec {
    unit(1, UnitKind::Harvester, 28, 19)
}

fn forge(state: &mut State, table: &str, id: &Value, edit: impl FnOnce(&mut Value)) {
    let mut document = serde_json::to_value(&*state).unwrap();
    let row = document[table]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == *id)
        .unwrap();
    edit(row);
    *state = serde_json::from_value(document).unwrap();
}

fn find_defense(state: &State, kind: BuildingKind) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|b| b.kind == kind)
        .unwrap()
        .id
}

/// A lone defense whose gun faces away from the map centre.
fn defense(kind: BuildingKind, tier: usize) -> (State, BuildingId) {
    let mut scenario = open_arena(WIDTH, HEIGHT, vec![bystander()]);
    scenario.buildings = vec![building(0, kind, 5, 6)];
    let mut state = scenario.build().unwrap();
    let id = find_defense(&state, kind);
    forge(&mut state, "buildings", &json!(id), |row| {
        row["tier"] = json!(tier);
    });
    face_away_from_centre(&mut state, id);
    (state, id)
}

fn face_away_from_centre(state: &mut State, id: BuildingId) {
    let center = state.building(id).unwrap().center();
    aim_defense(state, id, center + (center - centre()));
}

fn bearing(state: &State, id: BuildingId) -> u8 {
    state.building(id).unwrap().gun_heading.unwrap()
}

/// Signed compass steps from `from` to `to`, the short way.
fn turn(from: u8, to: u8) -> i16 {
    i16::from(to.wrapping_sub(from).cast_signed())
}

/// Swings the ground turret of `id` a hundred steps off its hull.
fn swing_turret(state: &mut State, id: UnitId) {
    let nose = state.unit(id).unwrap().heading;
    forge(state, "units", &json!(id), |row| {
        row["turret_heading"] = json!(nose.wrapping_add(100));
    });
}

/// A lone unit whose ground turret stands a hundred steps off its hull.
fn turreted(kind: UnitKind) -> (State, UnitId) {
    let mut state = open_arena(WIDTH, HEIGHT, vec![unit(0, kind, 8, 10), bystander()])
        .build()
        .unwrap();
    let id = state.units()[0].id;
    swing_turret(&mut state, id);
    (state, id)
}

/// Swings each of the Roc's guns to the starboard end of its arc.
fn swing_mounts(state: &mut State, id: UnitId) {
    let guns = UnitKind::Roc.stats().mounts.unwrap();
    forge(state, "units", &json!(id), |row| {
        for (slot, mount) in guns.mounts.iter().enumerate() {
            row["mounts"]["bearings"][slot] = json!(
                mount
                    .arc_centre
                    .cast_unsigned()
                    .wrapping_add(mount.arc_half)
            );
        }
    });
}

#[test]
fn every_resting_defense_tier_returns_to_the_map_centre() {
    let resting = BuildingKind::ALL
        .into_iter()
        .filter(|kind| kind.fires() && kind.gun_rests());
    for kind in resting {
        for tier in 0..kind.tiers().len() {
            let (mut state, id) = defense(kind, tier);
            let rest = heading_of(centre() - state.building(id).unwrap().center());
            let rate = kind.tiers()[tier].traverse;
            let off = turn(bearing(&state, id), rest).unsigned_abs();
            assert!(off >= 120, "{kind:?} staged facing away");
            for _ in 0..off.div_ceil(u16::from(rate)) {
                let before = bearing(&state, id);
                state.tick(&[]);
                let step = turn(before, bearing(&state, id));
                assert!(
                    step.unsigned_abs() <= u16::from(rate),
                    "{kind:?} turned {step}"
                );
            }
            assert_eq!(bearing(&state, id), rest, "{kind:?} tier {tier}");
            for _ in 0..20 {
                state.tick(&[]);
                assert_eq!(
                    bearing(&state, id),
                    rest,
                    "{kind:?} tier {tier} left its rest"
                );
            }
        }
    }
}

#[test]
fn bastion_and_citadel_hold_their_bearing() {
    assert!(BuildingKind::Bastion.fires() && !BuildingKind::Bastion.gun_rests());
    for tier in 0..BuildingKind::Bastion.tiers().len() {
        let (mut state, id) = defense(BuildingKind::Bastion, tier);
        let held = bearing(&state, id);
        for _ in 0..200 {
            state.tick(&[]);
            assert_eq!(bearing(&state, id), held, "tier {tier}");
        }
    }
}

#[test]
fn a_defense_gun_waits_for_its_reload_before_resting() {
    let (mut state, id) = defense(BuildingKind::Turret, 0);
    let held = bearing(&state, id);
    forge(&mut state, "buildings", &json!(id), |row| {
        row["cooldown"] = json!(5);
    });
    for tick in 1..5 {
        state.tick(&[]);
        assert_eq!(bearing(&state, id), held, "moved on reload tick {tick}");
    }
    state.tick(&[]);
    assert_ne!(bearing(&state, id), held, "the reload ran out");
}

#[test]
fn a_defense_gun_on_its_target_stays_on_it_through_the_reload() {
    let mut scenario = open_arena(WIDTH, HEIGHT, vec![unit(1, UnitKind::Harvester, 8, 9)]);
    scenario.buildings = vec![building(0, BuildingKind::Turret, 5, 6)];
    let mut state = scenario.build().unwrap();
    let id = find_defense(&state, BuildingKind::Turret);
    let target = state.units()[0].pos;
    aim_defense(&mut state, id, target);
    let held = bearing(&state, id);
    assert!(
        state
            .tick(&[])
            .events
            .iter()
            .any(|event| matches!(event, Event::TurretFired { turret, .. } if *turret == id))
    );
    for tick in 1..BuildingKind::Turret.base_stats().weapons[0].cooldown_ticks {
        state.tick(&[]);
        assert_eq!(bearing(&state, id), held, "moved on reload tick {tick}");
    }
}

#[test]
fn a_jammed_gun_holds_its_bearing() {
    let mut scenario = open_arena(WIDTH, HEIGHT, vec![unit(0, UnitKind::Maw, 4, 10)]);
    scenario.buildings = vec![building(1, BuildingKind::Turret, 10, 10)];
    let mut state = scenario.build().unwrap();
    let maw = state.units()[0].id;
    let gun = find_defense(&state, BuildingKind::Turret);
    let rest = heading_of(centre() - state.building(gun).unwrap().center());
    state.tick(&[cmd(
        0,
        Command::Attack {
            units: vec![maw],
            target: Target::Building(gun).into(),
            queue: false,
        },
    )]);
    let mut held = None;
    for _ in 0..600 {
        let report = state.tick(&[]);
        if state.building(gun).is_none() {
            break;
        }
        let now = bearing(&state, gun);
        let cut = report
            .events
            .iter()
            .any(|event| matches!(event, Event::AttackHit { attacker, .. } if *attacker == maw));
        if cut {
            held.get_or_insert(now);
        }
        if let Some(held) = held {
            assert_eq!(now, held, "a jammed gun turned");
        }
    }
    let held = held.expect("the Maw cut the Turret");
    assert!(
        turn(held, rest).unsigned_abs() > 2,
        "staged off its rest, so a swing back would show"
    );
}

#[test]
fn a_turret_returns_to_the_nose_and_then_follows_the_hull() {
    for kind in TURRETED {
        let (mut state, id) = turreted(kind);
        let nose = state.unit(id).unwrap().heading;
        let rate = kind.stats().turret_turn_rate;
        for tick in 1..=100_u16.div_ceil(u16::from(rate)) {
            let before = state.unit(id).unwrap().weapon_heading();
            state.tick(&[]);
            let unit = state.unit(id).unwrap();
            assert_eq!(unit.heading, nose, "{kind:?} turned its hull");
            let step = turn(before, unit.weapon_heading());
            assert!(
                step < 0 && step.unsigned_abs() <= u16::from(rate),
                "{kind:?} stepped {step} on tick {tick}"
            );
        }
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.weapon_heading(), nose, "{kind:?}");
        assert_eq!(
            unit.turret_heading, None,
            "{kind:?} keeps no bearing at rest"
        );

        let mut turned = false;
        state.tick(&[cmd(
            0,
            Command::Run {
                units: vec![id],
                goal: TilePos::new(2, 18),
                queue: false,
            },
        )]);
        for _ in 0..40 {
            state.tick(&[]);
            let unit = state.unit(id).unwrap();
            assert_eq!(unit.weapon_heading(), unit.heading, "{kind:?}");
            turned |= unit.heading != nose;
        }
        assert!(turned, "{kind:?} staged a turn");
    }
}

#[test]
fn a_turret_waits_for_every_reload_before_resting() {
    let weapons = UnitKind::Sentinel.stats().weapons.len();
    assert!(weapons > 1, "the Sentinel carries a second weapon");
    for slot in 0..weapons {
        let (mut state, id) = turreted(UnitKind::Sentinel);
        let held = state.unit(id).unwrap().weapon_heading();
        forge(&mut state, "units", &json!(id), |row| {
            row["cooldowns"][slot] = json!(5);
        });
        for tick in 1..5 {
            state.tick(&[]);
            assert_eq!(
                state.unit(id).unwrap().weapon_heading(),
                held,
                "slot {slot} moved on reload tick {tick}"
            );
        }
        state.tick(&[]);
        assert_ne!(
            state.unit(id).unwrap().weapon_heading(),
            held,
            "slot {slot}"
        );
    }
}

#[test]
fn a_turret_on_its_target_holds_through_each_reload() {
    for kind in TURRETED {
        let mut scenario = open_arena(WIDTH, HEIGHT, vec![unit(0, kind, 8, 10), bystander()]);
        scenario.buildings = vec![building(1, BuildingKind::Fabricator, 10, 9)];
        let mut state = scenario.build().unwrap();
        let id = state.units()[0].id;
        let (mut shots, mut held) = (0, None);
        for _ in 0..400 {
            let report = state.tick(&[]);
            let now = state.unit(id).unwrap().weapon_heading();
            if let Some(held) = held {
                assert_eq!(now, held, "{kind:?} left its target");
            }
            if report
                .events
                .iter()
                .any(|event| matches!(event, Event::AttackHit { attacker, .. } if *attacker == id))
            {
                held.get_or_insert(now);
                shots += 1;
                if shots == 3 {
                    break;
                }
            }
        }
        assert_eq!(shots, 3, "{kind:?}");
    }
}

#[test]
fn roc_guns_return_to_the_bearing_they_start_at() {
    let mut state = open_arena(
        WIDTH,
        HEIGHT,
        vec![unit(0, UnitKind::Roc, 10, 10), bystander()],
    )
    .build()
    .unwrap();
    let id = state.units()[0].id;
    let rest = state.unit(id).unwrap().mounts.unwrap().bearings;
    let guns = UnitKind::Roc.stats().mounts.unwrap();
    swing_mounts(&mut state, id);
    forge(&mut state, "units", &json!(id), |row| {
        row["cooldowns"][0] = json!(5);
    });
    let bearings = |state: &State| state.unit(id).unwrap().mounts.unwrap().bearings;
    let swung = bearings(&state);
    assert_ne!(swung, rest, "staged off its rest");
    for tick in 1..5 {
        state.tick(&[]);
        assert_eq!(bearings(&state)[0], swung[0], "moved on reload tick {tick}");
        assert_ne!(
            bearings(&state)[1],
            swung[1],
            "the other gun's reload is done"
        );
    }
    let widest = guns
        .mounts
        .iter()
        .map(|mount| mount.arc_half)
        .max()
        .unwrap();
    let ticks = 5 + (2 * u16::from(widest)).div_ceil(u16::from(guns.traverse));
    for _ in 0..ticks {
        let before = bearings(&state);
        state.tick(&[]);
        let now = bearings(&state);
        for (slot, mount) in guns.mounts.iter().enumerate() {
            assert!(mount.covers(now[slot]), "slot {slot} left its arc");
            assert!(
                mount
                    .arc_offset(now[slot])
                    .abs_diff(mount.arc_offset(before[slot]))
                    <= u16::from(guns.traverse),
                "slot {slot}"
            );
        }
    }
    assert_eq!(bearings(&state), rest);
}

#[test]
fn half_turned_guns_rest_in_step() {
    let (w, h) = (
        i32::try_from(WIDTH).unwrap(),
        i32::try_from(HEIGHT).unwrap(),
    );
    let mut scenario = open_arena(
        WIDTH,
        HEIGHT,
        vec![
            unit(0, UnitKind::Sentinel, 8, 10),
            unit(1, UnitKind::Sentinel, w - 1 - 8, h - 1 - 10),
        ],
    );
    scenario.buildings = vec![
        building(0, BuildingKind::Turret, 5, 6),
        building(1, BuildingKind::Turret, w - 1 - 5, h - 1 - 6),
    ];
    let mut state = scenario.build().unwrap();
    let turrets: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let defenses: Vec<BuildingId> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Turret)
        .map(|b| b.id)
        .collect();
    for &id in &turrets {
        swing_turret(&mut state, id);
    }
    for &id in &defenses {
        face_away_from_centre(&mut state, id);
    }
    let reading = |state: &State| {
        (
            turrets
                .iter()
                .map(|&id| state.unit(id).unwrap().weapon_heading())
                .collect::<Vec<_>>(),
            defenses
                .iter()
                .map(|&id| bearing(state, id))
                .collect::<Vec<_>>(),
        )
    };
    let mut moved = false;
    for tick in 0..40 {
        let before = reading(&state);
        state.tick(&[]);
        let now = reading(&state);
        let unit_step = |slot: usize| turn(before.0[slot], now.0[slot]);
        let gun_step = |slot: usize| turn(before.1[slot], now.1[slot]);
        assert_eq!(unit_step(0), unit_step(1), "turrets on tick {tick}");
        assert_eq!(gun_step(0), gun_step(1), "defenses on tick {tick}");
        moved |= unit_step(0) != 0 && gun_step(0) != 0;
    }
    assert!(moved, "both kinds of gun swung");
}

#[test]
fn a_save_mid_swing_resumes_the_same_swing() {
    let mut scenario = open_arena(
        WIDTH,
        HEIGHT,
        vec![
            unit(0, UnitKind::Sentinel, 8, 10),
            unit(0, UnitKind::Roc, 12, 10),
            bystander(),
        ],
    );
    scenario.buildings = vec![building(0, BuildingKind::Turret, 5, 6)];
    let mut state = scenario.build().unwrap();
    let (sentinel, roc) = (state.units()[0].id, state.units()[1].id);
    swing_turret(&mut state, sentinel);
    swing_mounts(&mut state, roc);
    let gun = find_defense(&state, BuildingKind::Turret);
    face_away_from_centre(&mut state, gun);
    for _ in 0..3 {
        state.tick(&[]);
    }
    assert!(state.unit(sentinel).unwrap().turret_heading.is_some());
    let mut resumed: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    for _ in 0..30 {
        assert_eq!(state.tick(&[]).events, resumed.tick(&[]).events);
        assert_eq!(state.hash(), resumed.hash());
    }
    assert_eq!(state.unit(sentinel).unwrap().turret_heading, None);
}
