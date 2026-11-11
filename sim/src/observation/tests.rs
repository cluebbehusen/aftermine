use super::ObservationData as Observation;
use super::*;
use crate::command::{Command, PlayerCommand};
use crate::scenario::{Scenario, UnitSpec};
use crate::stats::UnitKind;
use chassis::grid::TilePos;

/// Every unit, structure, resource, shell and warhead as `me` would see
/// them with no fog.
fn omniscient(state: &State, me: PlayerId) -> Observation {
    let mut obs = Observation::base(state, me);
    for u in state.units() {
        if u.hp == 0 {
            continue;
        }
        if u.player == me {
            obs.my_units.push(own_unit(state, u));
            obs.observe_own_cargo(u);
            if let Some(target) = own_repair_target(&u.order) {
                obs.my_repair_targets.push((u.id, target));
            }
        } else if !state.hostile(me, u.player) {
            obs.ally_units.push(enemy_unit(u));
        } else {
            obs.enemy_units.push(enemy_unit(u));
        }
    }
    for b in state.buildings() {
        if b.player == me {
            obs.my_buildings.push(own_building(b));
            obs.my_queues.push(b.queue.iter().copied().collect());
            obs.my_queue_progress.push(if b.queue.is_empty() {
                0
            } else {
                b.training_progress()
            });
        } else if !state.hostile(me, b.player) {
            obs.ally_buildings.push(BuildingObs {
                provisional: b.provisional(),
                id: b.id,
                player: b.player,
                kind: b.kind,
                anchor: b.anchor,
                hp: b.hp,
                built: b.built(),
                seen: true,
                tier: b.tier,
            });
        } else {
            obs.enemy_buildings.push(BuildingObs {
                provisional: b.provisional(),
                id: b.id,
                player: b.player,
                kind: b.kind,
                anchor: b.anchor,
                hp: b.hp,
                built: b.built(),
                seen: true,
                tier: b.tier,
            });
        }
    }
    for (pos, tile) in state.map().iter() {
        if tile.scrap > 0 {
            obs.known_scrap.push((pos, tile.scrap));
        }
        if tile.wreck > 0 {
            obs.known_wrecks.push((pos, tile.wreck));
        }
        if tile.terrain.blocks_ground() {
            obs.known_rock.push(pos);
        }
        if state.map().is_extractor_frame(pos) {
            obs.known_frames.push(pos);
        }
    }
    let tile_count = state.map().iter().count();
    obs.visible = vec![true; tile_count];
    obs.explored = vec![true; tile_count];
    obs.incoming_shells = state
        .shells()
        .iter()
        .filter(|s| state.hostile(me, s.player))
        .map(|s| TilePos::containing(s.impact))
        .collect();
    obs.incoming_shells.sort_by_key(|p| (p.y, p.x));
    obs.warheads = state.omniscient_warheads(me);
    obs
}

#[test]
fn paid_provisional_sites_are_marked_for_their_team_and_hidden_from_enemies() {
    for allied in [false, true] {
        let mut scenario = Scenario::skirmish();
        if allied {
            for player in &mut scenario.players {
                player.team = Some(0);
            }
            let mut opponent = scenario.players[1].clone();
            opponent.team = Some(1);
            scenario.players.push(opponent);
            scenario.map[1].replace_range(1..2, "3");
        }
        let mut state = scenario.build().unwrap();
        let worker = state
            .units
            .iter()
            .find(|u| u.player == PlayerId(0) && u.kind.stats().harvest.is_some())
            .unwrap()
            .id;
        let anchor = state.unit(worker).unwrap().tile();
        let kind = BuildingKind::Turret;
        let site = state.place_provisional_site(PlayerId(0), kind, anchor);
        state.unit_mut(worker).unwrap().order = Order::Found { kind, anchor };
        let own = ObservationData::fog_honest(&state, PlayerId(0));
        assert_eq!(own.version, OBSERVATION_VERSION);
        assert!(
            own.my_buildings
                .iter()
                .find(|b| b.id == site)
                .unwrap()
                .provisional
        );
        let observed_worker = own.my_units.iter().find(|u| u.id == worker).unwrap();
        assert_eq!(observed_worker.site, Some(site));
        let roundtrip: Observation =
            serde_json::from_value(serde_json::to_value(&own).unwrap()).unwrap();
        assert_eq!(roundtrip, own);
        let other = ObservationData::fog_honest(&state, PlayerId(1));
        assert!(!other.enemy_buildings.iter().any(|b| b.id == site));
        assert_eq!(
            other
                .ally_buildings
                .iter()
                .any(|b| b.id == site && b.provisional),
            allied
        );
    }
}

fn transport_scenario() -> Scenario {
    let mut scenario = Scenario::skirmish();
    scenario.name = "observation-transport".into();
    scenario.units = vec![
        UnitSpec {
            player: 0,
            kind: UnitKind::Skyhook,
            x: 8,
            y: 5,
        },
        UnitSpec {
            player: 0,
            kind: UnitKind::Harvester,
            x: 7,
            y: 5,
        },
        UnitSpec {
            player: 0,
            kind: UnitKind::Sentinel,
            x: 9,
            y: 5,
        },
        UnitSpec {
            player: 0,
            kind: UnitKind::Lancer,
            x: 8,
            y: 6,
        },
    ];
    scenario
}

#[test]
fn exact_repair_targets_are_owner_only_and_round_trip() {
    let mut state = Scenario::skirmish().build().unwrap();
    let worker = state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0))
        .unwrap()
        .id;
    let patient = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0))
        .unwrap()
        .id;
    state.unit_mut(worker).unwrap().order = Order::Repair { building: patient };
    for own in [
        ObservationData::fog_honest(&state, PlayerId(0)),
        omniscient(&state, PlayerId(0)),
    ] {
        assert_eq!(own.repair_target(worker), Some(Target::Building(patient)));
        assert_eq!(own.version, OBSERVATION_VERSION);
        let mut missing_targets = serde_json::to_value(&own).unwrap();
        missing_targets
            .as_object_mut()
            .unwrap()
            .remove("my_repair_targets");
        assert!(serde_json::from_value::<Observation>(missing_targets).is_err());
        assert_eq!(
            serde_json::from_str::<Observation>(&serde_json::to_string(&own).unwrap()).unwrap(),
            own
        );
    }
    for other in [
        ObservationData::fog_honest(&state, PlayerId(1)),
        omniscient(&state, PlayerId(1)),
    ] {
        assert_eq!(other.repair_target(worker), None);
        assert!(!other.enemy_units.iter().any(|unit| unit.repair.is_some()));
    }
    state.unit_mut(worker).unwrap().order = Order::Idle;
    assert_eq!(
        ObservationData::fog_honest(&state, PlayerId(0)).repair_target(worker),
        None
    );
}

#[test]
fn owner_training_progress_is_exact_aligned_and_required_by_the_schema() {
    let mut state = Scenario::skirmish()
        .build()
        .expect("the skirmish scenario builds");
    let producer = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == BuildingKind::Foundry)
        .expect("player zero has a Foundry")
        .id;
    let foundry = state
        .building_mut(producer)
        .expect("the Foundry remains live");
    foundry.queue.clear();
    foundry.queue.push_back(UnitKind::Harvester);
    foundry.phase = crate::state::BuildingPhase::Built { training: 17 };

    for observation in [
        ObservationData::fog_honest(&state, PlayerId(0)),
        omniscient(&state, PlayerId(0)),
    ] {
        let index = observation
            .my_buildings
            .iter()
            .position(|building| building.id == producer)
            .expect("the owner observes its producer");
        assert_eq!(observation.my_queues[index], vec![UnitKind::Harvester]);
        assert_eq!(observation.my_queue_progress[index], 17);
        assert_eq!(observation.own_queue_progress(index), Some(17));
        assert_eq!(observation.my_buildings.len(), observation.my_queues.len());
        assert_eq!(
            observation.my_buildings.len(),
            observation.my_queue_progress.len()
        );

        let encoded = serde_json::to_value(&observation).expect("the observation serializes");
        assert_eq!(
            serde_json::from_value::<Observation>(encoded.clone())
                .expect("the current observation schema round-trips"),
            observation
        );
        let mut obsolete = encoded;
        obsolete
            .as_object_mut()
            .expect("an observation serializes as an object")
            .remove("my_queue_progress");
        assert!(
            serde_json::from_value::<Observation>(obsolete).is_err(),
            "an older snapshot must not silently invent exact progress"
        );
    }
}

#[test]
fn hostile_training_progress_never_enters_an_observation() {
    let control = Scenario::skirmish()
        .build()
        .expect("the skirmish scenario builds");
    let mut variant = control.clone();
    let hostile_producer = variant
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(1) && building.kind == BuildingKind::Foundry)
        .expect("player one has a Foundry")
        .id;
    let foundry = variant
        .building_mut(hostile_producer)
        .expect("the hostile Foundry remains live");
    foundry.queue.push_back(UnitKind::Harvester);
    foundry.phase = crate::state::BuildingPhase::Built { training: 31 };

    assert_eq!(
        ObservationData::fog_honest(&control, PlayerId(0)),
        ObservationData::fog_honest(&variant, PlayerId(0)),
        "fog-honest policy input must not expose hostile production intent"
    );
}

#[test]
fn a_producer_shows_only_that_it_trains_and_an_airworks_its_bay() {
    let state = Scenario::skirmish()
        .build()
        .expect("the skirmish scenario builds");
    let mut foundry = state
        .buildings()
        .iter()
        .find(|building| building.kind == BuildingKind::Foundry)
        .expect("a Foundry stands")
        .clone();
    let built = |training| crate::state::BuildingPhase::Built { training };
    foundry.queue.clear();
    assert_eq!(Works::of(&foundry), None, "an empty queue idles");
    foundry
        .queue
        .extend([UnitKind::Sentinel, UnitKind::Harvester]);
    let total = UnitKind::Sentinel.stats().train_ticks;
    for training in [0, total / 2, total - 1] {
        foundry.phase = built(training);
        assert_eq!(Works::of(&foundry), Some(Works::Training));
    }
    foundry.phase = built(total);
    assert_eq!(
        Works::of(&foundry),
        None,
        "a finished unit waiting to leave"
    );
    foundry.phase = crate::state::BuildingPhase::Upgrading { progress: 1 };
    assert_eq!(Works::of(&foundry), None, "works under refit stand still");

    let mut airworks = foundry;
    airworks.kind = BuildingKind::Airworks;
    airworks.queue = [UnitKind::Buzzard].into();
    let total = UnitKind::Buzzard.stats().train_ticks;
    for (left, works) in [
        (AIRWORKS_BAY_TICKS + 1, Works::Training),
        (AIRWORKS_BAY_TICKS, Works::BayOpening),
        (AIRWORKS_BAY_TICKS / 2 + 1, Works::BayOpening),
        (AIRWORKS_BAY_TICKS / 2, Works::BayOpen),
        (1, Works::BayOpen),
    ] {
        airworks.phase = built(total - left);
        assert_eq!(Works::of(&airworks), Some(works), "{left} ticks left");
    }
}

#[test]
fn a_seen_hostile_producer_shows_its_works_and_nothing_of_its_queue() {
    let mut scenario = Scenario::skirmish();
    scenario.units.push(UnitSpec {
        player: 0,
        kind: UnitKind::Surveyor,
        x: 31,
        y: 19,
    });
    let state = scenario.build().expect("the skirmish scenario builds");
    let producer = state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(1) && building.kind == BuildingKind::Foundry)
        .expect("player one has a Foundry")
        .id;
    let training = |queue: &[UnitKind], training: u32| {
        let mut state = state.clone();
        let foundry = state.building_mut(producer).expect("the Foundry stands");
        foundry.queue.extend(queue);
        foundry.phase = crate::state::BuildingPhase::Built { training };
        ObservationData::fog_honest(&state, PlayerId(0))
    };
    let seen = training(&[UnitKind::Harvester], 31);
    assert!(
        seen.enemy_buildings
            .iter()
            .any(|building| building.id == producer),
        "premise: the Foundry is in sight"
    );
    assert_eq!(seen.works, [(producer, Works::Training)]);
    assert_eq!(
        training(&[UnitKind::Sentinel; 3], 5),
        seen,
        "neither the unit, its progress nor the queue's length shows"
    );
}

#[test]
fn owner_observation_reports_exact_sling_occupancy_without_leaking_the_manifest() {
    let mut state = transport_scenario()
        .build()
        .expect("the transport scenario builds");
    let skyhook = state
        .units()
        .iter()
        .find(|unit| unit.kind == UnitKind::Skyhook)
        .expect("the Skyhook exists")
        .id;
    let riders: Vec<_> = state
        .units()
        .iter()
        .filter(|unit| unit.id != skyhook)
        .map(|unit| unit.id)
        .collect();
    let expected_occupancy: u8 = state
        .units()
        .iter()
        .filter(|unit| unit.id != skyhook)
        .map(|unit| unit.kind.stats().transport_size)
        .sum();
    assert_eq!(expected_occupancy, 4, "the authored squad fills the sling");

    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Load {
            units: riders,
            transport: skyhook,
            queue: false,
        },
    }]);
    for _ in 0..200 {
        if state
            .unit(skyhook)
            .is_some_and(|transport| transport.cargo.len() == 3)
        {
            break;
        }
        state.tick(&[]);
    }
    assert_eq!(
        state
            .unit(skyhook)
            .expect("the loaded Skyhook remains alive")
            .cargo
            .len(),
        3,
        "the whole mixed-size squad boards"
    );

    for observation in [
        ObservationData::fog_honest(&state, PlayerId(0)),
        omniscient(&state, PlayerId(0)),
    ] {
        let transport = observation
            .my_units
            .iter()
            .find(|unit| unit.id == skyhook)
            .expect("the owner observes its Skyhook");
        assert_eq!(transport.cargo, expected_occupancy);
        assert_eq!(observation.my_carried_units.len(), 3);
        for passenger in &observation.my_carried_units {
            assert_eq!(passenger.carrier, skyhook);
            assert!(
                !observation
                    .my_units
                    .iter()
                    .any(|unit| unit.id == passenger.id)
            );
            let actual = state
                .unit(skyhook)
                .unwrap()
                .cargo
                .iter()
                .find(|unit| unit.id == passenger.id)
                .unwrap();
            assert_eq!((passenger.kind, passenger.hp), (actual.kind, actual.hp));
        }
        assert_eq!(
            serde_json::from_str::<Observation>(&serde_json::to_string(&observation).unwrap())
                .unwrap(),
            observation
        );
    }

    let opponent = omniscient(&state, PlayerId(1));
    assert!(opponent.my_carried_units.is_empty());
    assert!(
        ObservationData::fog_honest(&state, PlayerId(1))
            .my_carried_units
            .is_empty()
    );
    let transport = opponent
        .enemy_units
        .iter()
        .find(|unit| unit.id == skyhook)
        .expect("the complete test view includes the hostile Skyhook");
    assert_eq!(
        transport.cargo, 0,
        "an opponent may see the airframe, but not its sealed manifest"
    );
}

#[test]
fn a_parked_airframe_is_grounded_for_its_owner_and_for_anyone_who_sees_it() {
    let mut scenario = Scenario::skirmish();
    scenario.name = "observation-landing".into();
    scenario.units = vec![
        // Mid-basin, clear of both Foundries: nothing in the Condor's
        // acquisition range, so its move ends in a landing. The
        // Sentinel watches from inside its sight and outside its aggro.
        UnitSpec {
            player: 0,
            kind: UnitKind::Condor,
            x: 20,
            y: 12,
        },
        UnitSpec {
            player: 1,
            kind: UnitKind::Sentinel,
            x: 27,
            y: 12,
        },
    ];
    let mut state = scenario.build().expect("the landing scenario builds");
    let condor = state
        .units()
        .iter()
        .find(|unit| unit.kind == UnitKind::Condor)
        .expect("the Condor exists")
        .id;
    let goal = TilePos::new(20, 12);

    let airborne = omniscient(&state, PlayerId(1));
    let flying = airborne
        .enemy_units
        .iter()
        .find(|unit| unit.id == condor)
        .expect("the complete test view includes the hostile Condor");
    assert!(!flying.grounded);
    assert_eq!(flying.body_domain(), crate::stats::Domain::Air);

    state.tick(&[PlayerCommand {
        player: PlayerId(0),
        command: Command::Run {
            units: vec![condor],
            goal,
            queue: false,
        },
    }]);
    for _ in 0..1_500 {
        if state.unit(condor).is_some_and(crate::state::Unit::landed) {
            break;
        }
        state.tick(&[]);
    }
    let parked = state.unit(condor).expect("the Condor survives its landing");
    assert!(
        parked.landed(),
        "the Condor sets down within the flight budget"
    );
    let tile = parked.tile();
    assert!(
        state.vision(PlayerId(1)).visible(tile),
        "the opponent's Sentinel watches the landing tile"
    );

    for observation in [
        ObservationData::fog_honest(&state, PlayerId(0)),
        omniscient(&state, PlayerId(0)),
    ] {
        let own = observation
            .my_units
            .iter()
            .find(|unit| unit.id == condor)
            .expect("the owner observes its Condor");
        assert!(own.grounded);
        assert_eq!(own.body_domain(), crate::stats::Domain::Ground);
    }
    for observation in [
        ObservationData::fog_honest(&state, PlayerId(1)),
        omniscient(&state, PlayerId(1)),
    ] {
        let seen = observation
            .enemy_units
            .iter()
            .find(|unit| unit.id == condor)
            .expect("the opponent sees the parked Condor");
        assert!(
            seen.grounded,
            "a parked airframe is a physical fact, not private intent"
        );
        assert_eq!(seen.body_domain(), crate::stats::Domain::Ground);
    }
}

#[test]
fn owner_observation_exposes_the_active_harvest_node_without_leaking_enemy_orders() {
    let mut state = Scenario::skirmish()
        .build()
        .expect("the skirmish scenario builds");
    let worker = |player| {
        state
            .units()
            .iter()
            .find(|unit| unit.player == player && unit.kind == UnitKind::Harvester)
            .expect("each skirmish seat starts with a Harvester")
            .id
    };
    let known_node = |player| {
        state
            .map()
            .iter()
            .find(|(tile, cell)| cell.scrap > 0 && state.vision(player).visible(*tile))
            .map(|(tile, _)| tile)
            .expect("each skirmish seat starts with visible salvage")
    };
    let p0_worker = worker(PlayerId(0));
    let p1_worker = worker(PlayerId(1));
    let p0_node = known_node(PlayerId(0));
    let p1_node = known_node(PlayerId(1));

    state.tick(&[
        PlayerCommand {
            player: PlayerId(0),
            command: Command::Harvest {
                units: vec![p0_worker],
                node: p0_node,
                queue: false,
            },
        },
        PlayerCommand {
            player: PlayerId(1),
            command: Command::Harvest {
                units: vec![p1_worker],
                node: p1_node,
                queue: false,
            },
        },
    ]);

    for (observer, own_worker, own_node, hostile_worker) in [
        (PlayerId(0), p0_worker, p0_node, p1_worker),
        (PlayerId(1), p1_worker, p1_node, p0_worker),
    ] {
        let observation = omniscient(&state, observer);
        assert_eq!(
            observation
                .my_units
                .iter()
                .find(|unit| unit.id == own_worker)
                .expect("the owner observes its Harvester")
                .harvesting,
            Some(own_node)
        );
        assert_eq!(
            observation
                .enemy_units
                .iter()
                .find(|unit| unit.id == hostile_worker)
                .expect("the complete test view includes the hostile Harvester")
                .harvesting,
            None,
            "an enemy's Harvest order remains private even in a complete test view"
        );
    }
}

#[test]
fn pit_rock_is_exploration_limited_and_serialized() {
    let mut scenario = Scenario::skirmish();
    let mut rows = vec![vec!['.'; 40]; 24];
    rows[4][4] = '1';
    rows[18][34] = '2';
    rows[4][10] = '~';
    rows[12][20] = '~';
    scenario.map = rows
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect();
    scenario.units.clear();
    let state = scenario.build().unwrap();
    let local = TilePos::new(10, 4);
    let hidden = TilePos::new(20, 12);
    let fog = ObservationData::fog_honest(&state, PlayerId(0));
    assert!(fog.known_rock_at(local));
    assert!(!fog.known_rock_at(hidden));
    assert!(omniscient(&state, PlayerId(0)).known_rock_at(hidden));
    let restored: Observation =
        serde_json::from_str(&serde_json::to_string(&fog).unwrap()).unwrap();
    assert_eq!(restored, fog);
}

/// Seat 0's Mantle mid-basin with two charges and seat 1's Kestrel at
/// `watcher`, a tick into the match.
fn watched_mantle(watcher: (i32, i32)) -> State {
    let mut scenario = Scenario::skirmish();
    scenario.units = vec![
        UnitSpec {
            player: 0,
            kind: UnitKind::Mantle,
            x: 24,
            y: 12,
        },
        UnitSpec {
            player: 1,
            kind: UnitKind::Kestrel,
            x: watcher.0,
            y: watcher.1,
        },
    ];
    let mut state = scenario.build().expect("the scenario builds");
    state.units[0].point_defense.as_mut().unwrap().charges = 2;
    state.tick(&[]);
    state
}

#[test]
fn a_seen_point_defenders_charges_reach_owner_and_enemy_alike() {
    let state = watched_mantle((20, 12));
    let own = Observation::fog_honest(&state, PlayerId(0));
    assert_eq!(own.my_units[0].charges, Some(2));
    let enemy = Observation::fog_honest(&state, PlayerId(1));
    assert_eq!(enemy.enemy_units[0].charges, Some(2));
    assert_eq!(enemy.my_units[0].charges, None);
    let blind = Observation::fog_honest(&watched_mantle((2, 2)), PlayerId(1));
    assert!(blind.enemy_units.is_empty(), "unseen, unshown");
}
