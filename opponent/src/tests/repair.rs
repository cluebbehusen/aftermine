use super::*;
use crate::frame::gap;

/// West's Repair Bay on the arena, clear of scrap and the start.
const BAY: (i32, i32) = (9, 7);

/// West units well outside the Bay's reach and away from home.
const AWAY: [(i32, i32); 4] = [(13, 2), (14, 2), (15, 3), (14, 4)];

fn bay(player: u8, x: i32, y: i32) -> BuildingSpec {
    BuildingSpec {
        player,
        kind: BuildingKind::RepairBay,
        x,
        y,
    }
}

/// The arena with scrap to pay for repair and West's Bay.
fn bay_arena() -> Scenario {
    let mut scenario = arena(1_000);
    scenario.buildings.push(bay(0, BAY.0, BAY.1));
    scenario
}

/// West's units in `state` at the given spots, each cut to its `hp`.
fn hurt(state: &State, spots: &[((i32, i32), u32)]) -> (State, Vec<UnitId>) {
    let ids: Vec<UnitId> = spots.iter().map(|((x, y), _)| at(state, *x, *y)).collect();
    let state = ids
        .iter()
        .zip(spots)
        .fold(state.clone(), |state, (id, (_, hp))| {
            wounded(&state, *id, *hp)
        });
    (state, ids)
}

/// The units each Run in `commands` sends into reach of the Bay at `anchor`.
fn to_bay(commands: &[PlayerCommand], anchor: (i32, i32)) -> Vec<Vec<UnitId>> {
    runs(commands)
        .into_iter()
        .filter(|(_, goal)| {
            gap(
                TilePos::new(anchor.0, anchor.1),
                BuildingKind::RepairBay.size(),
                *goal,
                (1, 1),
            ) <= 0
        })
        .map(|(units, _)| units)
        .collect()
}

fn welds(commands: &[PlayerCommand]) -> Vec<UnitId> {
    commands
        .iter()
        .filter_map(|command| match &command.command {
            Command::RepairUnit { target, .. } => Some(*target),
            _ => None,
        })
        .collect()
}

#[test]
fn a_wounded_heavy_drives_to_a_calm_bay() {
    let mut scenario = bay_arena();
    scenario.units.push(unit(0, UnitKind::Breaker, 15, 3));
    let state = scenario.build().unwrap();
    let max = UnitKind::Breaker.stats().max_hp;
    let sent = |hp: u32| {
        let (state, ids) = hurt(&state, &[((15, 3), hp)]);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        (to_bay(&commands, BAY), ids[0])
    };
    let (mauled, breaker) = sent(max / 3);
    assert_eq!(mauled, [vec![breaker]]);
    assert!(sent(max).0.is_empty(), "a whole Breaker stays put");
    assert!(
        sent(max - max / 20).0.is_empty(),
        "a scratch is not worth the trip"
    );
}

#[test]
fn a_bay_takes_only_the_most_wounded_its_free_slots_hold() {
    let mut scenario = bay_arena();
    // One patient already in reach takes a slot of the four.
    scenario.units.push(unit(0, UnitKind::Sentinel, 11, 7));
    for (x, y) in AWAY {
        scenario.units.push(unit(0, UnitKind::Sentinel, x, y));
    }
    let state = scenario.build().unwrap();
    let mut spots = vec![((11, 7), 30)];
    spots.extend(
        AWAY.iter()
            .zip([10, 40, 20, 30])
            .map(|(spot, hp)| (*spot, hp)),
    );
    let (state, ids) = hurt(&state, &spots);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    let mut expected = vec![ids[1], ids[3], ids[4]];
    expected.sort_unstable();
    assert_eq!(to_bay(&commands, BAY), [expected], "{commands:?}");
}

#[test]
fn no_unit_goes_to_a_bay_under_threat() {
    let pulled = |threat: bool| {
        let mut scenario = bay_arena();
        scenario.units.push(unit(0, UnitKind::Breaker, 15, 3));
        if threat {
            // Anti-air only, so home defense leaves the Breaker free; armed
            // all the same, so the Bay is no place to heal.
            scenario.units.push(unit(1, UnitKind::Flakhound, 12, 9));
        }
        let state = scenario.build().unwrap();
        let (state, _) = hurt(&state, &[((15, 3), 300)]);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        to_bay(&commands, BAY)
    };
    assert!(!pulled(false).is_empty(), "premise: a calm Bay draws it");
    assert!(pulled(true).is_empty());
}

#[test]
fn welders_leave_units_at_or_bound_for_a_bay_to_it() {
    // Both stand at home: one out of the Bay's reach, one in it.
    for (spot, sent) in [((4, 10), true), ((8, 8), false)] {
        let staged = |with_bay: bool| {
            let mut scenario = if with_bay { bay_arena() } else { arena(1_000) };
            scenario
                .units
                .push(unit(0, UnitKind::Sentinel, spot.0, spot.1));
            let state = scenario.build().unwrap();
            let (state, ids) = hurt(&state, &[(spot, 10)]);
            let commands =
                seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
            (commands, ids[0])
        };
        let (without, patient) = staged(false);
        assert_eq!(welds(&without), [patient], "premise: welded at home");
        let (with, patient) = staged(true);
        assert!(welds(&with).is_empty(), "{spot:?}: {with:?}");
        let expected: Vec<Vec<UnitId>> = if sent { vec![vec![patient]] } else { vec![] };
        assert_eq!(to_bay(&with, BAY), expected, "{spot:?}");
    }
}

#[test]
fn an_attack_leaves_a_unit_healing_at_a_bay() {
    let launch = |with_bay: bool| {
        let mut scenario = super::attack::armed(8, &[]);
        for player in &mut scenario.players {
            player.scrap = 1_000;
        }
        scenario.units.push(unit(0, UnitKind::Sentinel, 10, 11));
        if with_bay {
            scenario.buildings.push(bay(0, 11, 11));
        }
        let state = scenario.build().unwrap();
        let patient = at(&state, 10, 11);
        let state = wounded(&state, patient, UnitKind::Sentinel.stats().max_hp * 3 / 5);
        let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
        (hunts(&commands), patient)
    };
    let (without, patient) = launch(false);
    assert!(
        without.iter().any(|(units, _)| units.contains(&patient)),
        "premise: fit enough to join: {without:?}"
    );
    let (with, patient) = launch(true);
    assert!(!with.is_empty(), "the attack still launches");
    assert!(
        with.iter().all(|(units, _)| !units.contains(&patient)),
        "{with:?}"
    );
}

#[test]
fn aircraft_heal_landed_or_hovering_in_reach() {
    for (kind, hp) in [(UnitKind::Condor, 100), (UnitKind::Talon, 30)] {
        let mut scenario = bay_arena();
        scenario.units.push(unit(0, kind, 15, 3));
        let state = scenario.build().unwrap();
        let (mut state, ids) = hurt(&state, &[((15, 3), hp)]);
        let mut opponent = seat_with(&scenario, 0, thrifty());
        while state.unit(ids[0]).is_some_and(|unit| unit.hp <= hp) {
            assert!(state.current_tick() < 1_200, "{kind:?} never healed");
            let next = state.current_tick() + 1;
            play(&mut opponent, &mut state, next, &[]);
        }
        let unit = state.unit(ids[0]).expect("the patient lives");
        assert_eq!(
            unit.landed(),
            kind == UnitKind::Condor,
            "{kind:?} rests as it heals"
        );
        assert!(
            gap(
                TilePos::new(BAY.0, BAY.1),
                BuildingKind::RepairBay.size(),
                unit.tile(),
                (1, 1)
            ) < 4,
            "{kind:?} heals in reach at {:?}",
            unit.tile()
        );
    }
}

#[test]
fn mirrored_seats_send_mirrored_patients() {
    let mut scenario = field();
    for player in &mut scenario.players {
        player.scrap = 1_000;
    }
    let (width, height) = (48, 24);
    scenario
        .buildings
        .extend([bay(0, 8, 14), bay(1, width - 1 - 9, height - 1 - 15)]);
    let spots = [(14, 17), (15, 17), (16, 19)];
    for (x, y) in spots {
        scenario.units.push(unit(0, UnitKind::Sentinel, x, y));
        scenario
            .units
            .push(unit(1, UnitKind::Sentinel, width - 1 - x, height - 1 - y));
    }
    let state = scenario.build().unwrap();
    let hurt_spots: Vec<((i32, i32), u32)> = spots
        .iter()
        .zip([10, 30, 20])
        .flat_map(|((x, y), hp)| [((*x, *y), hp), ((width - 1 - x, height - 1 - y), hp)])
        .collect();
    let (state, _) = hurt(&state, &hurt_spots);
    let west = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let east = seat(&scenario, 1).act(&state, &mut OwnEvents::default());
    assert!(!to_bay(&west, (8, 14)).is_empty(), "premise: {west:?}");
    assert_eq!(mirror(&state, west), east);
}

#[test]
fn a_full_bay_leaves_the_next_wounded_to_an_overlapping_one() {
    let second = (11, 7);
    let mut scenario = bay_arena();
    scenario.buildings.push(bay(0, second.0, second.1));
    let around = [(10, 6), (11, 6), (10, 9), (11, 9)];
    for (x, y) in around {
        scenario.units.push(unit(0, UnitKind::Sentinel, x, y));
    }
    scenario.units.push(unit(0, UnitKind::Sentinel, 15, 3));
    let state = scenario.build().unwrap();
    let mut spots: Vec<((i32, i32), u32)> = around.iter().map(|spot| (*spot, 20)).collect();
    spots.push(((15, 3), 20));
    let (state, ids) = hurt(&state, &spots);
    let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
    let sent: Vec<UnitId> = to_bay(&commands, BAY)
        .into_iter()
        .chain(to_bay(&commands, second))
        .flatten()
        .collect();
    assert_eq!(sent, [ids[4]], "{commands:?}");
}

#[test]
fn a_condor_keeps_off_a_bay_under_ground_fire_that_a_talon_may_use() {
    let sent = |kind: UnitKind, hp: u32| {
        let mut scenario = bay_arena();
        // Fires on the ground only: a landed Condor is in its sights, a
        // hovering Talon is not.
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind: BuildingKind::Turret,
            x: 13,
            y: 9,
        });
        scenario.units.push(unit(0, kind, 15, 3));
        let state = scenario.build().unwrap();
        let (state, _) = hurt(&state, &[((15, 3), hp)]);
        let commands = seat_with(&scenario, 0, thrifty()).act(&state, &mut OwnEvents::default());
        to_bay(&commands, BAY)
    };
    assert!(
        !sent(UnitKind::Talon, 30).is_empty(),
        "premise: the Talon goes"
    );
    assert!(sent(UnitKind::Condor, 100).is_empty());
}

#[test]
fn each_aircraft_flies_to_a_bay_around_the_anti_air_on_its_own_way() {
    let mut scenario = field();
    for player in &mut scenario.players {
        player.scrap = 1_000;
    }
    let anchor = (10, 11);
    scenario.buildings.extend([
        bay(0, anchor.0, anchor.1),
        BuildingSpec {
            player: 1,
            kind: BuildingKind::FlakTurret,
            x: 24,
            y: 4,
        },
    ]);
    // The more wounded Talon's straight way is clear; the other starts out
    // of the Flak Turret's reach but its straight way crosses it. A Sentinel
    // has the Turret in sight.
    let (clear, crossing) = ((10, 3), (36, 5));
    for (x, y) in [clear, crossing] {
        scenario.units.push(unit(0, UnitKind::Talon, x, y));
    }
    scenario.units.push(unit(0, UnitKind::Sentinel, 28, 9));
    let state = scenario.build().unwrap();
    let (state, ids) = hurt(&state, &[(clear, 20), (crossing, 30)]);
    let observation = oxide_sim::observation::ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_buildings
            .iter()
            .any(|building| building.kind == BuildingKind::FlakTurret),
        "premise: the Flak Turret is known"
    );
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let goal = TilePos::new(anchor.0, anchor.1);
    let flights = runs(&commands);
    let legs = |id: UnitId| -> Vec<TilePos> {
        flights
            .iter()
            .filter(|(units, _)| units.contains(&id))
            .map(|(_, to)| *to)
            .collect()
    };
    assert_eq!(legs(ids[0]), [goal], "{commands:?}");
    let detour = legs(ids[1]);
    assert_eq!(detour.len(), 2, "{commands:?}");
    assert_ne!(detour[0], goal, "the crossing Talon swings around first");
    assert_eq!(detour[1], goal);
}
