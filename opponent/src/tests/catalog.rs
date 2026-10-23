//! Every building, upgrade and unit the seat can buy, each staged where it
//! buys it. Army units are covered by the composition test that every one of
//! them gets its turn; the rest are here.

use super::*;
use crate::defenses;
use crate::investments::{ADOPT, Investment, Situation, candidates};
use crate::memory::Memory;
use crate::profile::PersonalityTraits;
use oxide_sim::observation::ObservationData;

/// What West would save for on `scenario` at `tick` once it scored best,
/// with its economy up, no army to speak of, every trait high and the
/// lowest rung's economy: every investment that scores enough to be adopted,
/// which every rung then buys somewhere.
fn offered(scenario: &Scenario, state: &State, tick: u64) -> Vec<Investment> {
    offered_lifting(scenario, state, tick, None)
}

/// As [`offered`], with West's ground reaching no enemy and `lift` the known
/// anti-air around its next lift's target, when given.
fn offered_lifting(
    scenario: &Scenario,
    state: &State,
    tick: u64,
    lift: Option<u64>,
) -> Vec<Investment> {
    let mut observation = ObservationData::fog_honest(state, PlayerId(0));
    observation.tick = tick;
    let model = map(scenario);
    let mut memory = Memory::default();
    memory.observe(&observation);
    let situation = Situation {
        observation: &observation,
        map: &model,
        memory: &memory,
        traits: PersonalityTraits {
            air: 100,
            siege: 100,
            support: 100,
            fortification: 100,
            greed: 100,
            guile: 100,
        },
        saturation: 1_000,
        income: 1_000,
        // Home scrap has begun to run down.
        depletion: 1,
        pull: Vec::new(),
        exposed: true,
        stakes: defenses::Stakes::default(),
        severed: lift.is_some(),
        wanted: vec![
            crate::composition::Role::Line,
            crate::composition::Role::Siege,
            crate::composition::Role::AntiAir,
            crate::composition::Role::AirStrike,
            crate::composition::Role::AntiHeavy,
        ],
        units: Vec::new(),
        waiting: None,
        margin: crate::missions::margin(oxide_sim::scenario::BotDifficulty::Standard),
        economy: crate::economy(oxide_sim::scenario::BotDifficulty::Scrapheap),
        extractors_first: crate::extractors_first(oxide_sim::scenario::BotDifficulty::Scrapheap),
        lift_flak: lift.unwrap_or(0),
    };
    candidates(&situation)
        .into_iter()
        .filter(|candidate| candidate.score >= ADOPT)
        .map(|candidate| candidate.investment)
        .collect()
}

/// The building kinds and upgrade rungs among `investments`, each rung as
/// its building's kind and the tier it reaches.
fn bought(state: &State, investments: &[Investment]) -> Vec<(BuildingKind, u8)> {
    investments
        .iter()
        .filter_map(|investment| match *investment {
            Investment::Tech(kind) | Investment::Capacity(kind) => Some((kind, 0)),
            Investment::Defense { kind, .. } => Some((kind, 0)),
            Investment::Expansion(_) => Some((BuildingKind::Foundry, 0)),
            Investment::Extractor(_) => Some((BuildingKind::Extractor, 0)),
            Investment::Reclaimer => Some((BuildingKind::Reclaimer, 0)),
            Investment::Unit(_) | Investment::Round(_) => None,
            Investment::Upgrade { building, tier } => state
                .buildings()
                .iter()
                .find(|each| each.id == building)
                .map(|each| (each.kind, tier)),
        })
        .collect()
}

/// `state` with every West `kind` at `tier`.
fn tiered(state: &State, kind: BuildingKind, tier: u8) -> State {
    let mut value = serde_json::to_value(state).unwrap();
    let name = serde_json::to_value(kind).unwrap();
    for entry in value["buildings"].as_array_mut().unwrap() {
        if entry["player"] == 0 && entry["kind"] == name {
            entry["tier"] = tier.into();
        }
    }
    serde_json::from_value(value).unwrap()
}

/// West units of `kind` trained by the decision on `scenario`, played to
/// `tick`.
fn trained(scenario: &Scenario, tick: u64, kind: UnitKind) -> bool {
    let mut state = scenario.build().unwrap();
    if tick > 0 {
        advance_to(&mut state, tick, &[]);
    }
    trains(&seat_with(scenario, 0, thrifty()).act(&state, &mut OwnEvents::default()))
        .iter()
        .any(|(_, each)| *each == kind)
}

#[test]
fn a_seat_in_an_emergency_trains_defenders_before_scouts() {
    let mut pressed = field();
    pressed.players[0].scrap = 1_000;
    pressed
        .buildings
        .push(building(0, BuildingKind::Fabricator, 6, 16));
    pressed.units.push(unit(1, UnitKind::Sentinel, 40, 12));
    let mut state = pressed.build().unwrap();
    let raider = at(&state, 40, 12);
    // Scouting points go stale before an East Sentinel walks up to the West
    // Foundry.
    advance_to(&mut state, 1_800, &[]);
    state.tick(&[run(1, vec![raider], 9, 12)]);
    while state
        .units()
        .iter()
        .any(|unit| unit.id == raider && unit.tile().x > 10)
    {
        state.tick(&[]);
    }
    let interval = crate::decision_interval(thrifty().difficulty);
    while !state.current_tick().is_multiple_of(interval) {
        state.tick(&[]);
    }
    let trained = trains(&seat_with(&pressed, 0, thrifty()).act(&state, &mut OwnEvents::default()));
    assert!(
        trained
            .iter()
            .any(|(_, kind)| kind.stats().can_target(oxide_sim::stats::Domain::Ground)),
        "premise: the seat arms against the raider: {trained:?}"
    );
    assert!(
        trained.iter().all(|(_, kind)| *kind != UnitKind::Surveyor),
        "{trained:?}"
    );
}

/// West with its tech, a Deep Bore and a standing army on the open field,
/// facing a ring of East Bastions it sees, worth more than that army: ground
/// its army cannot take.
fn fortress() -> Scenario {
    let mut fortress = field();
    fortress.map[4].replace_range(8..9, "E");
    fortress.players[0].scrap = 2 * UnitKind::Maw.stats().cost;
    fortress
        .units
        .extend((5..10).map(|x| unit(0, UnitKind::Sentinel, x, 14)));
    fortress
        .units
        .extend((5..10).map(|x| unit(0, UnitKind::Excavator, x, 18)));
    // A Kestrel over East's base, so West knows the guns.
    fortress.units.push(unit(0, UnitKind::Kestrel, 41, 11));
    fortress.buildings.extend([
        building(0, BuildingKind::Crucible, 3, 16),
        building(0, BuildingKind::Extractor, 8, 4),
        building(0, BuildingKind::Fabricator, 3, 6),
        building(0, BuildingKind::Airworks, 6, 6),
    ]);
    for (x, y) in [
        (38, 4),
        (41, 4),
        (44, 4),
        (38, 8),
        (38, 14),
        (38, 18),
        (41, 18),
        (44, 18),
        (40, 8),
        (40, 14),
    ] {
        fortress
            .buildings
            .push(building(1, BuildingKind::Bastion, x, y));
    }
    fortress
}

#[test]
fn every_building_and_upgrade_is_bought_somewhere() {
    let mut covered: Vec<(BuildingKind, u8)> = Vec::new();
    let mut offer = |scenario: &Scenario, state: &State, tick: u64| {
        covered.extend(bought(state, &offered(scenario, state, tick)));
    };

    // Tech, an Extractor on a free frame, and a Turret, Bastion, Array and
    // Flak Turret against the hostile start and a known enemy Airworks.
    let mut home = arena(0);
    home.map[1].replace_range(6..7, "E");
    home.units.extend([harvester(0, 5, 7), harvester(0, 4, 7)]);
    home.buildings
        .push(building(1, BuildingKind::Airworks, 10, 8));
    let state = home.build().unwrap();
    offer(&home, &state, 600);

    // An expansion Foundry.
    let mut frontier = arena(0);
    frontier.map = super::expansion::FRONTIER.map(str::to_owned).to_vec();
    frontier.units = vec![harvester(0, 4, 3), harvester(0, 5, 3)];
    let state = frontier.build().unwrap();
    offer(&frontier, &state, 3_000);

    // Reclaimers, Refineries, a Barricade in front of the Turret, Scuttle
    // Charges, and the base-tier upgrades once a Fabricator stands, with an
    // army and aircraft in sight beyond the defenses' reach that outweigh
    // the guns standing.
    let mut built = home.clone();
    built.units.extend([
        unit(1, UnitKind::Sentinel, 18, 8),
        unit(0, UnitKind::Kestrel, 17, 8),
    ]);
    for (x, y) in [(19, 8), (20, 8), (21, 8), (19, 9), (20, 9), (21, 9)] {
        built.units.push(unit(1, UnitKind::Warden, x, y));
    }
    for x in 18..=21 {
        for y in 1..=3 {
            built.units.push(unit(1, UnitKind::Buzzard, x, y));
        }
    }
    built.buildings.extend([
        building(0, BuildingKind::Fabricator, 3, 1),
        building(0, BuildingKind::Reclaimer, 1, 9),
        building(0, BuildingKind::Turret, 7, 5),
        building(0, BuildingKind::FlakTurret, 7, 7),
        building(0, BuildingKind::Array, 9, 3),
    ]);
    let state = built.build().unwrap();
    offer(&built, &state, 3_000);

    // Bulwarks and Deep Arrays once a Crucible stands, the Array moved to
    // where the approach runs just beyond its sight today.
    let mut crucible = built.clone();
    crucible
        .buildings
        .retain(|spec| spec.kind != BuildingKind::Array);
    crucible
        .buildings
        .push(building(0, BuildingKind::Array, 8, 2));
    // Behind the Foundry, so the guns standing still face the base's edge.
    crucible
        .buildings
        .push(building(0, BuildingKind::Crucible, 1, 5));
    let state = tiered(&crucible.build().unwrap(), BuildingKind::Turret, 1);
    offer(&crucible, &state, 3_000);

    // A Citadel once a Crucible stands, its longer reach covering more of
    // the way in, from a corner clear of the aircraft.
    let mut citadel = crucible.clone();
    citadel
        .buildings
        .push(building(0, BuildingKind::Bastion, 1, 1));
    let state = citadel.build().unwrap();
    offer(&citadel, &state, 3_000);

    // A Deep Bore on the home frame, which the home Foundry supports.
    let mut bore = crucible.clone();
    bore.buildings
        .push(building(0, BuildingKind::Extractor, 6, 1));
    let state = bore.build().unwrap();
    offer(&bore, &state, 3_000);

    // A Spire once a Crucible and a Deep Bore stand and a block of seen
    // enemy works is worth a warhead.
    let mut strike = bore.clone();
    strike.buildings.extend([
        building(1, BuildingKind::Crucible, 12, 8),
        building(1, BuildingKind::Crucible, 14, 8),
    ]);
    let state = tiered(&strike.build().unwrap(), BuildingKind::Extractor, 1);
    offer(&strike, &state, 3_000);

    // A Maw cradle once a Crucible and a Deep Bore stand and known enemy guns
    // outweigh the army.
    let breach = fortress();
    let state = tiered(&breach.build().unwrap(), BuildingKind::Extractor, 1);
    offer(&breach, &state, 3_000);

    // A Lattice over the base against a seen enemy Spire.
    let mut missile = crucible.clone();
    missile
        .buildings
        .push(building(1, BuildingKind::Spire, 12, 8));
    let state = missile.build().unwrap();
    offer(&missile, &state, 3_000);

    // A Repair Bay where the wounded gather.
    let mut hurt = arena(0);
    let squad = [
        (2, 8),
        (3, 8),
        (4, 8),
        (5, 8),
        (2, 10),
        (3, 10),
        (4, 10),
        (5, 10),
    ];
    for (x, y) in squad {
        hurt.units.push(unit(0, UnitKind::Sentinel, x, y));
    }
    let mut state = hurt.build().unwrap();
    for (x, y) in squad {
        state = wounded(&state, at(&state, x, y), 1);
    }
    offer(&hurt, &state, 3_000);

    // A Roc cradle once a Crucible, Deep Bore and Airworks stand, while the
    // seat's ground reaches no enemy and known anti-air guards its next
    // lift's target.
    let breach = fortress();
    let state = tiered(&breach.build().unwrap(), BuildingKind::Extractor, 1);
    covered.extend(bought(
        &state,
        &offered_lifting(&breach, &state, 3_000, Some(guarded_lift())),
    ));

    let missing: Vec<(BuildingKind, u8)> = BuildingKind::ALL
        .into_iter()
        .flat_map(|kind| {
            (0..u8::try_from(kind.tiers().len()).unwrap()).map(move |tier| (kind, tier))
        })
        .filter(|rung| !covered.contains(rung))
        .collect();
    assert!(missing.is_empty(), "never bought: {missing:?}");
}

/// Known anti-air around a lift's target: four Flak Turrets' worth.
fn guarded_lift() -> u64 {
    4 * u64::from(BuildingKind::FlakTurret.invested_cost(0))
}

/// Whether West, with income enough to raid, trains a Scuttler to raid an
/// unguarded East Foundry its Kestrel can see.
fn raids() -> bool {
    let mut scenario = field();
    garrison(&mut scenario);
    scenario.units.push(unit(0, UnitKind::Kestrel, 18, 10));
    scenario.units.push(harvester(0, 2, 2));
    scenario
        .buildings
        .push(building(1, BuildingKind::Foundry, 22, 10));
    scenario.players[0].scrap = 1_000;
    let mut state = scenario.build().unwrap();
    advance_to(&mut state, 120, &[]);
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["income"] = serde_json::json!({"previous": null, "per_minute": 2_000});
    });
    trains(&opponent.act(&state, &mut OwnEvents::default()))
        .iter()
        .any(|(_, kind)| *kind == UnitKind::Scuttler)
}

#[test]
fn every_unit_outside_the_army_is_trained_somewhere() {
    let mut rows: Vec<(UnitKind, bool)> = Vec::new();

    let mut armed = arena(200);
    armed.units.extend(standing_army(0));
    rows.push((UnitKind::Harvester, trained(&armed, 0, UnitKind::Harvester)));

    let mut excavating = arena(1_000);
    excavating.units.extend(standing_army(0));
    excavating
        .buildings
        .push(building(0, BuildingKind::Fabricator, 3, 1));
    rows.push((
        UnitKind::Excavator,
        trained(&excavating, 0, UnitKind::Excavator),
    ));

    let mut scouting = field();
    scouting.players[0].scrap = 1_000;
    rows.push((
        UnitKind::Surveyor,
        trained(&scouting, 1_800, UnitKind::Surveyor),
    ));
    scouting.buildings.extend([
        building(0, BuildingKind::Fabricator, 6, 16),
        building(0, BuildingKind::Airworks, 10, 16),
    ]);
    rows.push((
        UnitKind::Kestrel,
        trained(&scouting, 1_800, UnitKind::Kestrel),
    ));

    rows.push((UnitKind::Scuttler, raids()));

    let mut lifting = super::lift::strait();
    lifting.units.retain(|unit| unit.kind != UnitKind::Skyhook);
    lifting.players[0].scrap = 1_000;
    rows.push((UnitKind::Skyhook, trained(&lifting, 0, UnitKind::Skyhook)));

    let mut sapping = field();
    sapping.players[0].scrap = 1_000;
    sapping.buildings.extend([
        building(0, BuildingKind::Fabricator, 6, 16),
        building(1, BuildingKind::Turret, 9, 9),
    ]);
    rows.push((UnitKind::Sapper, trained(&sapping, 0, UnitKind::Sapper)));

    // A Maw is assembled, not trained: buying its cradle stands for it.
    let breach = fortress();
    let state = tiered(&breach.build().unwrap(), BuildingKind::Extractor, 1);
    rows.push((
        UnitKind::Maw,
        offered(&breach, &state, 3_000).contains(&Investment::Tech(BuildingKind::MawCradle)),
    ));

    // A drone is built in a Roc's bay, which the seat leaves refilling.
    let mut bay = field();
    bay.players[0].scrap = 1_000;
    bay.units.push(unit(0, UnitKind::Roc, 10, 11));
    let mut built = bay.build().unwrap();
    let mut refilling = seat_with(&bay, 0, thrifty());
    for _ in 0..u64::from(UnitKind::Drone.stats().train_ticks) + 20 {
        let commands = refilling.act(&built, &mut OwnEvents::default());
        built.tick(&commands);
    }
    rows.push((
        UnitKind::Drone,
        built
            .units()
            .iter()
            .any(|u| u.player == PlayerId(0) && u.bay.is_some_and(|bay| bay.docked > 0)),
    ));

    // A Roc is assembled too: buying its cradle stands for it.
    rows.push((
        UnitKind::Roc,
        offered_lifting(&breach, &state, 3_000, Some(guarded_lift()))
            .contains(&Investment::Tech(BuildingKind::RocCradle)),
    ));

    let failed: Vec<UnitKind> = rows
        .iter()
        .filter(|(_, trained)| !trained)
        .map(|(kind, _)| *kind)
        .collect();
    assert!(failed.is_empty(), "not trained where staged: {failed:?}");
    let missing: Vec<UnitKind> = UnitKind::ALL
        .into_iter()
        .filter(|kind| crate::composition::role(*kind).is_none())
        .filter(|kind| !rows.iter().any(|(row, _)| row == kind))
        .collect();
    assert!(missing.is_empty(), "no staging for {missing:?}");
}
