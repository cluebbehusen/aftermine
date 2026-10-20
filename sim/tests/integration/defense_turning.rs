//! Defense guns turn onto their targets before they fire, and pick the
//! target they can fire on soonest.

use crate::common;

use chassis::compass::heading_of;
use chassis::fx::Vec2Fx;
use oxide_sim::{BuildingId, BuildingKind, Event, State, Target, UnitKind};

use common::{aim_defense, building, open_arena, unit};

fn defense(state: &State, kind: BuildingKind) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|b| b.kind == kind)
        .unwrap()
        .id
}

fn bearing(state: &State, id: BuildingId) -> u8 {
    state.building(id).unwrap().gun_heading.unwrap()
}

fn shot(events: &[Event], id: BuildingId) -> Option<Target> {
    events.iter().find_map(|event| match event {
        Event::TurretFired { turret, target, .. } if *turret == id => *target,
        Event::ShellLaunched {
            shooter: Target::Building(shooter),
            target,
            ..
        } if *shooter == id => *target,
        _ => None,
    })
}

/// Signed compass steps from `from` to `to`, the short way.
fn turn(from: u8, to: u8) -> i16 {
    i16::from(to.wrapping_sub(from).cast_signed())
}

#[test]
fn every_rung_that_fires_turns_its_gun() {
    for kind in BuildingKind::ALL {
        for (tier, stats) in kind.tiers().iter().enumerate() {
            assert_eq!(
                stats.traverse > 0,
                !stats.weapons.is_empty(),
                "{kind:?} tier {tier}"
            );
            assert_eq!(
                kind.fires(),
                !stats.weapons.is_empty(),
                "{kind:?} tier {tier}"
            );
        }
    }
}

#[test]
fn new_guns_face_the_map_centre() {
    let mut scenario = open_arena(32, 22, Vec::new());
    scenario.buildings = vec![
        building(0, BuildingKind::Turret, 5, 6),
        building(1, BuildingKind::Bastion, 24, 14),
        building(0, BuildingKind::Reclaimer, 8, 8),
    ];
    let state = scenario.build().unwrap();
    let centre = Vec2Fx::new(chassis::fx::Fx::from_num(16), chassis::fx::Fx::from_num(11));
    for b in state.buildings() {
        let expected = b.kind.fires().then(|| heading_of(centre - b.center()));
        assert_eq!(b.gun_heading, expected, "{:?}", b.kind);
    }
}

/// A gun staged facing directly away from a lone target turns onto it at
/// its traverse, one step a tick and never back, and fires on the tick it
/// comes within the firing tolerance.
#[test]
fn a_gun_facing_away_fires_once_it_has_turned() {
    for (kind, victim) in [
        (BuildingKind::Turret, UnitKind::Harvester),
        (BuildingKind::FlakTurret, UnitKind::Skyhook),
        (BuildingKind::Bastion, UnitKind::Harvester),
    ] {
        let mut scenario = open_arena(32, 22, vec![unit(1, victim, 8, 9)]);
        scenario.buildings = vec![building(0, kind, 5, 6)];
        let mut state = scenario.build().unwrap();
        let gun = defense(&state, kind);
        let target = state.units()[0].pos;
        let center = state.building(gun).unwrap().center();
        let toward = heading_of(target - center);
        aim_defense(&mut state, gun, center + (center - target));
        let rate = i16::from(kind.base_stats().traverse);
        let off = turn(bearing(&state, gun), toward).abs();
        assert!(off >= 127, "{kind:?} staged facing away");
        let expected = u32::try_from((off - 2 + rate - 1) / rate).unwrap();
        let mut sense = 0;
        for tick in 1..=expected {
            let before = bearing(&state, gun);
            let fired = shot(&state.tick(&[]).events, gun);
            let step = turn(before, bearing(&state, gun));
            assert!(step.abs() <= rate, "{kind:?} turned {step} in one tick");
            assert!(step * sense >= 0, "{kind:?} reversed on tick {tick}");
            sense = step.signum();
            if tick < expected {
                assert_eq!(step.abs(), rate, "{kind:?} tick {tick}");
                assert_eq!(fired, None, "{kind:?} fired before it bore on tick {tick}");
            } else {
                assert_eq!(fired, Some(Target::Unit(state.units()[0].id)), "{kind:?}");
                assert!(turn(bearing(&state, gun), toward).abs() <= 2, "{kind:?}");
            }
        }
    }
}

/// A ready gun keeps firing at the target it bears on rather than turn away
/// to a nearer one; right after the shot, with a reload long enough for
/// either turn, the nearest wins and the gun turns one way onto it.
#[test]
fn a_ready_gun_takes_the_target_it_can_fire_on_soonest() {
    let mut scenario = open_arena(
        32,
        22,
        vec![
            unit(1, UnitKind::Harvester, 9, 6),
            unit(1, UnitKind::Harvester, 5, 8),
        ],
    );
    scenario.buildings = vec![building(0, BuildingKind::Turret, 5, 5)];
    let mut state = scenario.build().unwrap();
    let gun = defense(&state, BuildingKind::Turret);
    let (far, near) = (state.units()[0].id, state.units()[1].id);
    let center = state.building(gun).unwrap().center();
    assert!(
        center.dist_sq(state.unit(near).unwrap().pos)
            < center.dist_sq(state.unit(far).unwrap().pos)
    );
    let far_pos = state.unit(far).unwrap().pos;
    aim_defense(&mut state, gun, far_pos);

    assert_eq!(shot(&state.tick(&[]).events, gun), Some(Target::Unit(far)));
    let reload = BuildingKind::Turret.base_stats().weapons[0].cooldown_ticks;
    let mut sense = 0;
    for tick in 1..=reload {
        let before = bearing(&state, gun);
        let fired = shot(&state.tick(&[]).events, gun);
        let step = turn(before, bearing(&state, gun));
        assert!(step * sense >= 0, "the gun reversed on tick {tick}");
        if step != 0 {
            sense = step.signum();
        }
        if tick < reload {
            assert_eq!(fired, None, "fired mid-reload on tick {tick}");
        } else {
            assert_eq!(fired, Some(Target::Unit(near)));
        }
    }
    assert_ne!(sense, 0, "the gun turned onto the nearer target");
}

/// Mirrored defenses facing the map centre turn onto mirrored targets and
/// open fire on the same tick, whichever seat they belong to.
#[test]
fn mirrored_defenses_open_fire_on_the_same_tick() {
    for kind in [BuildingKind::Turret, BuildingKind::Bastion] {
        let (w, h) = kind.size();
        let mut scenario = open_arena(
            32,
            22,
            vec![
                unit(1, UnitKind::Harvester, 3, 4),
                unit(0, UnitKind::Harvester, 31 - 3, 21 - 4),
            ],
        );
        scenario.buildings = vec![
            building(0, kind, 6, 7),
            building(1, kind, 32 - w - 6, 22 - h - 7),
        ];
        let mut state = scenario.build().unwrap();
        let guns: Vec<BuildingId> = state
            .buildings()
            .iter()
            .filter(|b| b.kind == kind)
            .map(|b| b.id)
            .collect();
        let mut first = [None; 2];
        for tick in 1..=120u32 {
            let events = state.tick(&[]).events;
            for (slot, gun) in guns.iter().enumerate() {
                if first[slot].is_none() && shot(&events, *gun).is_some() {
                    first[slot] = Some(tick);
                }
            }
        }
        assert!(first[0].is_some_and(|tick| tick > 1), "{kind:?}: {first:?}");
        assert_eq!(first[0], first[1], "{kind:?}");
    }
}
