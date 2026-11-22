use super::*;
use aftermine_sim::observation::UnitObs;

/// `scenario` built with every Maw facing compass step `heading` and each
/// building in `hurt` at that health.
fn staged(scenario: &Scenario, heading: u8, hurt: &[(BuildingKind, u32)]) -> State {
    let mut value = serde_json::to_value(scenario.build().unwrap()).unwrap();
    for entry in value["units"].as_array_mut().unwrap() {
        if entry["kind"] == "maw" {
            entry["heading"] = heading.into();
        }
    }
    for entry in value["buildings"].as_array_mut().unwrap() {
        for (kind, hp) in hurt {
            if entry["player"] == 0 && entry["kind"] == serde_json::to_value(kind).unwrap() {
                entry["hp"] = (*hp).into();
            }
        }
    }
    serde_json::from_value(value).unwrap()
}

fn maw_in_sight(state: &State) -> UnitObs {
    ObservationData::fog_honest(state, PlayerId(0))
        .enemy_units
        .into_iter()
        .find(|enemy| enemy.kind == UnitKind::Maw)
        .expect("premise: West sees the Maw")
}

fn queued_attacks(commands: &[PlayerCommand]) -> Vec<(Vec<UnitId>, AttackTarget)> {
    commands_of(commands, |command| match command {
        Command::Attack {
            units,
            target,
            queue: true,
        } => Some((units.clone(), *target)),
        _ => None,
    })
}

#[test]
fn small_machines_ahead_of_a_maw_step_out_of_its_path_and_the_armed_fire_from_beside_it() {
    // An East Maw faces west at a West Sentinel and Harvester. Each steps out
    // of its path, the Sentinel to where it fires on the Maw from. A
    // Sentinel behind the Maw, and a Breaker ahead of it, too broad to go
    // under its front, stay where they are.
    let mut scenario = field();
    scenario.units.extend([
        unit(1, UnitKind::Maw, 20, 11),
        unit(0, UnitKind::Sentinel, 17, 11),
        harvester(0, 18, 12),
        unit(0, UnitKind::Sentinel, 24, 11),
        unit(0, UnitKind::Breaker, 17, 9),
    ]);
    let state = staged(&scenario, 128, &[]);
    let maw = maw_in_sight(&state);
    let (ahead, worker, behind, broad) = (
        at(&state, 17, 11),
        at(&state, 18, 12),
        at(&state, 24, 11),
        at(&state, 17, 9),
    );
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let ran = runs(&commands);
    for (id, kind) in [(ahead, UnitKind::Sentinel), (worker, UnitKind::Harvester)] {
        let goal = ran
            .iter()
            .find(|(units, _)| units == &[id])
            .unwrap_or_else(|| panic!("{id:?} steps aside: {commands:?}"))
            .1;
        assert!(
            !crate::crush::in_path(&maw, kind, goal, 12),
            "{id:?} goes to {goal:?}, still in the Maw's path"
        );
    }
    assert!(
        queued_attacks(&commands)
            .iter()
            .any(|(units, target)| units == &[ahead] && *target == AttackTarget::Unit(maw.id)),
        "{commands:?}"
    );
    assert!(
        ran.iter()
            .all(|(units, _)| !units.contains(&behind) && !units.contains(&broad)),
        "{commands:?}"
    );
}

#[test]
fn a_maw_at_the_base_is_attacked_not_hunted() {
    // An East Maw drives on the West Foundry. Lancers beside the base, out
    // of its path, attack it where it stands rather than hunt toward it.
    let mut scenario = field();
    scenario.units.extend([
        unit(1, UnitKind::Maw, 9, 11),
        unit(0, UnitKind::Lancer, 5, 6),
        unit(0, UnitKind::Lancer, 6, 6),
    ]);
    let state = staged(&scenario, 128, &[]);
    let maw = maw_in_sight(&state);
    let lancers = [at(&state, 5, 6), at(&state, 6, 6)];
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    assert!(
        attacks(&commands).iter().any(|(units, target)| {
            *target == AttackTarget::Unit(maw.id) && lancers.iter().all(|id| units.contains(id))
        }),
        "{commands:?}"
    );
    assert!(
        hunts(&commands)
            .iter()
            .all(|(units, _)| lancers.iter().all(|id| !units.contains(id))),
        "{commands:?}"
    );
}

#[test]
fn a_foundry_a_maw_cuts_needs_five_welders_and_its_gun_spares_them() {
    // A Maw pressed against the West Foundry cuts it at 300 hp a second; a
    // welder mends 64. Five out-mend it, and the Maw never turns on a
    // welder.
    let mut scenario = field();
    scenario
        .units
        .extend([unit(1, UnitKind::Maw, 6, 11), harvester(0, 2, 9)]);
    let state = staged(&scenario, 128, &[(BuildingKind::Foundry, 1_200)]);
    let observation = mapped(&state, PlayerId(0));
    let map = map(&scenario);
    let foundry = observation
        .my_buildings
        .iter()
        .find(|building| building.kind == BuildingKind::Foundry)
        .unwrap();
    let welder = observation
        .my_units
        .iter()
        .find(|unit| unit.kind == UnitKind::Harvester)
        .unwrap();
    assert_eq!(
        crate::placement::welders(
            &observation,
            &map,
            &crate::memory::Memory::default(),
            foundry,
            welder
        ),
        Some(5)
    );
}

fn welded(extra_foundry: bool, kind: BuildingKind) -> Vec<UnitId> {
    let mut scenario = field();
    // Enough for the welds, not for an emergency Turret, which would take a
    // worker first.
    scenario.players[0].scrap = 90;
    scenario.units.extend([
        unit(1, UnitKind::Maw, 6, 11),
        harvester(0, 1, 8),
        harvester(0, 2, 8),
        harvester(0, 1, 9),
    ]);
    if kind != BuildingKind::Foundry {
        let maw = scenario.units.len() - 4;
        scenario.units[maw].x = 13;
        scenario.buildings.push(building(0, kind, 10, 11));
    }
    if extra_foundry {
        scenario
            .buildings
            .push(building(0, BuildingKind::Foundry, 20, 18));
    }
    let state = staged(&scenario, 128, &[(kind, kind.base_stats().max_hp * 3 / 4)]);
    let target = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == kind)
        .unwrap()
        .id;
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    commands_of(&commands, |command| match command {
        Command::Repair {
            units, building, ..
        } if *building == target => Some(units.clone()),
        _ => None,
    })
    .concat()
}

#[test]
fn the_last_foundry_gets_every_free_welder_even_short_of_five() {
    assert_eq!(welded(false, BuildingKind::Foundry).len(), 3);
    assert!(
        welded(true, BuildingKind::Foundry).is_empty(),
        "with another Foundry, three welders that cannot hold it stay away"
    );
    assert!(welded(false, BuildingKind::Fabricator).is_empty());
}

#[test]
fn no_site_goes_up_in_a_maw_s_path() {
    let mut scenario = field();
    scenario
        .units
        .extend([unit(1, UnitKind::Maw, 20, 11), harvester(0, 15, 6)]);
    let state = staged(&scenario, 128, &[]);
    let observation = mapped(&state, PlayerId(0));
    let map = map(&scenario);
    let crew = observation
        .my_units
        .iter()
        .find(|unit| unit.kind == UnitKind::Harvester)
        .unwrap();
    let stands = |x: i32, y: i32| {
        crate::placement::stands(
            &observation,
            &map,
            &crate::memory::Memory::default(),
            crate::placement::Raising::fresh(BuildingKind::Barricade, TilePos::new(x, y)),
            &[],
            &[crew],
        )
    };
    assert!(!stands(16, 11), "ahead of the Maw");
    assert!(stands(16, 4), "well beside its path");
    assert!(stands(26, 11), "behind it");
}
