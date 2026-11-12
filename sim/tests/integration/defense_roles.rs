//! Static-defense role contracts: what each emplacement stops, what
//! bypasses it, and which siege rung answers it.

use crate::common;

use aftermine_sim::{BuildingKind, Event, PlayerId, Scenario, Target, UnitId, UnitKind};
use chassis::grid::TilePos;

use common::{aim_defense_at, building_id, cmd, open_arena, orders, unit, unit_id};

const WIDTH: i32 = 48;
const HEIGHT: i32 = 20;

fn mirrored_tile(tile: TilePos) -> TilePos {
    TilePos::new(WIDTH - 1 - tile.x, HEIGHT - 1 - tile.y)
}

fn mirrored_anchor(anchor: TilePos, kind: BuildingKind) -> TilePos {
    let (width, height) = kind.size();
    TilePos::new(WIDTH - width - anchor.x, HEIGHT - height - anchor.y)
}

fn role_scenario(
    defender: u8,
    buildings: &[(BuildingKind, TilePos)],
    attackers: &[(UnitKind, TilePos)],
    spotters: &[(u8, UnitKind, TilePos)],
) -> Scenario {
    let attacker = 1 - defender;
    let buildings = buildings
        .iter()
        .map(|(kind, anchor)| {
            let anchor = if defender == 0 {
                *anchor
            } else {
                mirrored_anchor(*anchor, *kind)
            };
            common::building(defender, *kind, anchor.x, anchor.y)
        })
        .collect();
    let mut units: Vec<_> = attackers
        .iter()
        .map(|(kind, tile)| {
            let tile = if defender == 0 {
                *tile
            } else {
                mirrored_tile(*tile)
            };
            unit(attacker, *kind, tile.x, tile.y)
        })
        .collect();
    units.extend(spotters.iter().map(|(owner, kind, tile)| {
        let tile = if defender == 0 {
            *tile
        } else {
            mirrored_tile(*tile)
        };
        unit(*owner, *kind, tile.x, tile.y)
    }));
    let mut scenario = open_arena(WIDTH as usize, HEIGHT as usize, units);
    scenario.buildings = buildings;
    scenario
}

fn attack_building(
    state: &mut aftermine_sim::State,
    attacker: u8,
    units: &[UnitId],
    building: aftermine_sim::BuildingId,
) -> Vec<Event> {
    let mut events = state
        .tick(&[cmd(
            attacker,
            orders::attack(units.to_vec(), Target::Building(building)),
        )])
        .events;
    for _ in 0..3_000u32 {
        if state.building(building).is_none() {
            break;
        }
        events.extend(state.tick(&[]).events);
    }
    events
}

#[test]
fn defense_stats_name_three_distinct_jobs() {
    use aftermine_sim::stats::Prerequisite;

    let flak = BuildingKind::FlakTurret.base_stats().weapons[0];
    assert!(
        flak.targets.air && !flak.targets.ground,
        "flak fires only skyward"
    );

    // The Citadel only reaches further.
    let bastion = BuildingKind::Bastion.base_stats();
    let citadel = BuildingKind::Bastion.tier_stats(1);
    assert_eq!(
        (citadel.max_hp, citadel.vision),
        (bastion.max_hp, bastion.vision)
    );
    let (gun, long_gun) = (bastion.weapons[0], citadel.weapons[0]);
    assert!(long_gun.range > gun.range);
    assert_eq!(
        (
            gun.damage,
            gun.minimum_range,
            gun.cooldown_ticks,
            gun.splash,
            gun.targets
        ),
        (
            long_gun.damage,
            long_gun.minimum_range,
            long_gun.cooldown_ticks,
            long_gun.splash,
            long_gun.targets
        )
    );

    assert_eq!(
        bastion.construction.requires,
        &[Prerequisite::built(BuildingKind::Fabricator)]
    );
    assert_eq!(
        citadel.construction.requires,
        &[Prerequisite::built(BuildingKind::Crucible)]
    );
}

#[test]
fn lancer_sieges_a_turret_from_outside_return_range() {
    for defender in [0, 1] {
        let attacker = 1 - defender;
        let mut state = role_scenario(
            defender,
            &[(BuildingKind::Turret, TilePos::new(20, 9))],
            &[(UnitKind::Lancer, TilePos::new(14, 9))],
            &[],
        )
        .build()
        .unwrap();
        let turret = building_id(&state, |building| building.kind == BuildingKind::Turret);
        let lancer = unit_id(&state, |unit| unit.player == PlayerId(attacker));
        let events = attack_building(&mut state, attacker, &[lancer], turret);

        assert!(state.building(turret).is_none());
        assert_eq!(
            state.unit(lancer).unwrap().hp,
            UnitKind::Lancer.stats().max_hp
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::TurretFired { turret: fired, .. } if *fired == turret)),
            "the five-tile point defense must not answer the 5.5-tile rail"
        );
    }
}

#[test]
fn a_bombard_outranges_a_bastion_and_a_citadel_answers_it_from_full_range() {
    // The Bombard measures to the emplacement's nearest edge and the
    // emplacement from its center, so answering a Bombard from any side takes
    // the footprint's half-diagonal beyond the Bombard's range: the Bastion
    // matches it and is outranged, the Citadel clears it.
    let bombard_range = UnitKind::Bombard.stats().weapons[0].range;
    let half_diagonal = chassis::fx::Fx::lit("1.42");
    for (tier, answers) in [(0u8, false), (1, true)] {
        let range = BuildingKind::Bastion.tier_stats(tier).weapons[0].range;
        assert_eq!(range >= bombard_range + half_diagonal, answers);
        for defender in [0, 1] {
            let attacker = 1 - defender;
            let state = role_scenario(
                defender,
                &[(BuildingKind::Bastion, TilePos::new(20, 8))],
                &[(UnitKind::Bombard, TilePos::new(10, 9))],
                &[
                    (attacker, UnitKind::Talon, TilePos::new(16, 9)),
                    (defender, UnitKind::Harvester, TilePos::new(14, 9)),
                ],
            )
            .build()
            .unwrap();
            let bastion = building_id(&state, |building| building.kind == BuildingKind::Bastion);
            let mut value = serde_json::to_value(&state).unwrap();
            let row = value["buildings"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["id"] == serde_json::json!(bastion))
                .unwrap();
            row["tier"] = serde_json::json!(tier);
            let mut state: aftermine_sim::State = serde_json::from_value(value).unwrap();
            let bombard = unit_id(&state, |unit| {
                unit.player == PlayerId(attacker) && unit.kind == UnitKind::Bombard
            });
            let events = attack_building(&mut state, attacker, &[bombard], bastion);

            let fired_back = events.iter().any(|event| {
                matches!(
                    event,
                    Event::ShellLaunched {
                        shooter: Target::Building(building),
                        ..
                    } if *building == bastion
                )
            });
            assert_eq!(fired_back, answers, "tier {tier}, defender {defender}");
            if answers {
                assert!(state.unit(bombard).is_none(), "the Citadel wins the duel");
                assert!(state.building(bastion).is_some());
            } else {
                assert!(state.building(bastion).is_none(), "the Bastion falls");
                assert_eq!(
                    state.unit(bombard).unwrap().hp,
                    UnitKind::Bombard.stats().max_hp
                );
            }
        }
    }
}

#[test]
fn close_pressure_breaches_an_isolated_bastion_dead_zone() {
    for defender in [0, 1] {
        let attacker = 1 - defender;
        let mut state = role_scenario(
            defender,
            &[(BuildingKind::Bastion, TilePos::new(20, 8))],
            &[
                (UnitKind::Scuttler, TilePos::new(14, 3)),
                (UnitKind::Scuttler, TilePos::new(14, 7)),
                (UnitKind::Scuttler, TilePos::new(14, 11)),
                (UnitKind::Scuttler, TilePos::new(14, 15)),
            ],
            &[(attacker, UnitKind::Talon, TilePos::new(17, 9))],
        )
        .build()
        .unwrap();
        let bastion = building_id(&state, |building| building.kind == BuildingKind::Bastion);
        let scuttlers: Vec<_> = state
            .units()
            .iter()
            .filter(|unit| unit.player == PlayerId(attacker) && unit.kind == UnitKind::Scuttler)
            .map(|unit| unit.id)
            .collect();
        let events = attack_building(&mut state, attacker, &scuttlers, bastion);

        assert!(
            state.building(bastion).is_none(),
            "split close pressure must breach an isolated Bastion from seat {defender}; hp={:?}, survivors={:?}",
            state.building(bastion).map(|building| building.hp),
            scuttlers
                .iter()
                .filter_map(|id| state.unit(*id).map(|unit| (id, unit.hp, unit.pos)))
                .collect::<Vec<_>>()
        );
        assert!(
            scuttlers.iter().any(|id| state.unit(*id).is_some()),
            "predictive fire can stop one straight attacker, not both separated approaches"
        );
        let shots: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::ShellLaunched {
                    shooter: Target::Building(building),
                    from,
                    to,
                    ..
                } if *building == bastion => Some((*from, *to)),
                _ => None,
            })
            .collect();
        assert!(
            !shots.is_empty(),
            "the breach must cross the outer firing envelope before reaching safety"
        );
        let minimum = BuildingKind::Bastion.base_stats().weapons[0].minimum_range;
        assert!(
            shots
                .iter()
                .all(|(from, to)| from.dist_sq(*to) >= minimum * minimum),
            "the Bastion must stop firing after pressure reaches its dead zone"
        );
    }
}

#[test]
fn ground_attack_aircraft_bypass_ground_only_defenses() {
    for defender in [0, 1] {
        let attacker = 1 - defender;
        for kind in [BuildingKind::Turret, BuildingKind::Bastion] {
            let anchor = if kind == BuildingKind::Turret {
                TilePos::new(20, 9)
            } else {
                TilePos::new(20, 8)
            };
            let mut state = role_scenario(
                defender,
                &[(kind, anchor)],
                &[(UnitKind::Buzzard, TilePos::new(16, 9))],
                &[],
            )
            .build()
            .unwrap();
            let defense = building_id(&state, |building| building.kind == kind);
            let flyer = unit_id(&state, |unit| unit.player == PlayerId(attacker));
            let events = attack_building(&mut state, attacker, &[flyer], defense);

            assert!(
                state.building(defense).is_none(),
                "{kind:?} blocks ground only"
            );
            assert_eq!(
                state.unit(flyer).unwrap().hp,
                UnitKind::Buzzard.stats().max_hp
            );
            assert!(!events.iter().any(|event| matches!(
                event,
                Event::TurretFired { turret, .. }
                    | Event::ShellLaunched {
                        shooter: Target::Building(turret),
                        ..
                    } if *turret == defense
            )));
        }
    }
}

#[test]
fn ground_units_bypass_air_only_flak() {
    for defender in [0, 1] {
        let attacker = 1 - defender;
        let mut state = role_scenario(
            defender,
            &[(BuildingKind::FlakTurret, TilePos::new(20, 9))],
            &[(UnitKind::Scuttler, TilePos::new(18, 9))],
            &[],
        )
        .build()
        .unwrap();
        let flak = building_id(&state, |building| building.kind == BuildingKind::FlakTurret);
        let scuttler = unit_id(&state, |unit| unit.player == PlayerId(attacker));
        let events = attack_building(&mut state, attacker, &[scuttler], flak);

        assert!(state.building(flak).is_none());
        assert_eq!(
            state.unit(scuttler).unwrap().hp,
            UnitKind::Scuttler.stats().max_hp
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::TurretFired { turret, .. } if *turret == flak))
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchOutcome {
    DefenseWin,
    AttackWin,
    Censored,
}

fn cost_par_outcome(
    defender: u8,
    kind: BuildingKind,
    anchors: &[TilePos],
    attackers: &[(UnitKind, TilePos)],
) -> MatchOutcome {
    let attacker = 1 - defender;
    let buildings: Vec<_> = anchors.iter().map(|anchor| (kind, *anchor)).collect();
    let mut state = role_scenario(defender, &buildings, attackers, &[])
        .build()
        .unwrap();
    let defenders: Vec<_> = state
        .buildings()
        .iter()
        .filter(|building| building.kind == kind)
        .map(|building| building.id)
        .collect();
    let attackers: Vec<_> = state
        .units()
        .iter()
        .filter(|unit| unit.player == PlayerId(attacker))
        .map(|unit| unit.id)
        .collect();
    let goal = if defender == 0 {
        anchors[anchors.len() / 2]
    } else {
        mirrored_anchor(anchors[anchors.len() / 2], kind)
    };
    state.tick(&[cmd(attacker, orders::hunt(attackers.clone(), goal))]);

    let mut last_total = u64::MAX;
    let mut last_progress = 0u32;
    for tick in 0..3_000u32 {
        let attackers_alive = attackers.iter().filter_map(|id| state.unit(*id)).count();
        let defenders_alive = defenders
            .iter()
            .filter_map(|id| state.building(*id))
            .count();
        if attackers_alive == 0 {
            return MatchOutcome::DefenseWin;
        }
        if defenders_alive == 0 {
            return MatchOutcome::AttackWin;
        }
        let total = attackers
            .iter()
            .filter_map(|id| state.unit(*id))
            .map(|unit| u64::from(unit.hp))
            .chain(
                defenders
                    .iter()
                    .filter_map(|id| state.building(*id))
                    .map(|building| u64::from(building.hp)),
            )
            .sum();
        if total != last_total {
            last_total = total;
            last_progress = tick;
        } else if tick.saturating_sub(last_progress) >= 500 {
            return MatchOutcome::Censored;
        }
        state.tick(&[]);
    }
    MatchOutcome::Censored
}

#[test]
fn cost_par_defenses_hold_their_target_domain() {
    let turret_anchors = [
        TilePos::new(20, 6),
        TilePos::new(20, 9),
        TilePos::new(20, 12),
    ];
    let sentinel_attack = [
        (UnitKind::Sentinel, TilePos::new(13, 6)),
        (UnitKind::Sentinel, TilePos::new(13, 9)),
        (UnitKind::Sentinel, TilePos::new(13, 12)),
    ];
    let flak_anchors = turret_anchors;
    let buzzard_attack = [
        (UnitKind::Buzzard, TilePos::new(13, 7)),
        (UnitKind::Buzzard, TilePos::new(13, 11)),
    ];
    let bastion_anchors = [TilePos::new(20, 6), TilePos::new(20, 11)];
    let bastion_attack = [
        (UnitKind::Sentinel, TilePos::new(12, 5)),
        (UnitKind::Sentinel, TilePos::new(12, 7)),
        (UnitKind::Sentinel, TilePos::new(12, 9)),
        (UnitKind::Sentinel, TilePos::new(12, 11)),
        (UnitKind::Sentinel, TilePos::new(12, 13)),
        (UnitKind::Sentinel, TilePos::new(12, 15)),
    ];

    for defender in [0, 1] {
        for (kind, anchors, attackers) in [
            (
                BuildingKind::Turret,
                turret_anchors.as_slice(),
                sentinel_attack.as_slice(),
            ),
            (
                BuildingKind::FlakTurret,
                flak_anchors.as_slice(),
                buzzard_attack.as_slice(),
            ),
            (
                BuildingKind::Bastion,
                bastion_anchors.as_slice(),
                bastion_attack.as_slice(),
            ),
        ] {
            assert_eq!(
                cost_par_outcome(defender, kind, anchors, attackers),
                MatchOutcome::DefenseWin,
                "{kind:?} must decisively hold its cost-par role from seat {defender}"
            );
        }
    }
}

#[test]
fn changing_course_after_launch_can_dodge_an_unguided_bastion_shell() {
    for defender in [0, 1] {
        let attacker = 1 - defender;
        let mut state = role_scenario(
            defender,
            &[(BuildingKind::Bastion, TilePos::new(20, 8))],
            &[(UnitKind::Scuttler, TilePos::new(14, 9))],
            &[],
        )
        .build()
        .unwrap();
        let scuttler = unit_id(&state, |unit| unit.player == PlayerId(attacker));
        let bastion = building_id(&state, |building| building.kind == BuildingKind::Bastion);
        aim_defense_at(&mut state, bastion, Target::Unit(scuttler));
        let goal = if defender == 0 {
            TilePos::new(14, 15)
        } else {
            mirrored_tile(TilePos::new(14, 15))
        };
        let first = state.tick(&[cmd(attacker, orders::advance([scuttler], goal))]);

        let launched = first.events.iter().any(|event| {
            matches!(
                event,
                Event::ShellLaunched {
                    shooter: Target::Building(_),
                    ..
                }
            )
        });
        assert!(launched, "the Bastion leads the initial straight path");
        let dodge = if defender == 0 {
            TilePos::new(14, 3)
        } else {
            mirrored_tile(TilePos::new(14, 3))
        };
        state.tick(&[cmd(attacker, orders::advance([scuttler], dodge))]);
        let mut landed = first
            .events
            .iter()
            .any(|event| matches!(event, Event::ShellLanded { .. }));
        for _ in 0..300u32 {
            let report = state.tick(&[]);
            landed |= report
                .events
                .iter()
                .any(|event| matches!(event, Event::ShellLanded { .. }));
            if landed {
                break;
            }
        }
        assert!(landed, "the dodge requires a resolved shot");
        assert_eq!(
            state.unit(scuttler).unwrap().hp,
            UnitKind::Scuttler.stats().max_hp,
            "the projectile leads the known path but does not guide after a course change"
        );
    }
}
