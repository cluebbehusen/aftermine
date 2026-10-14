use super::super::super::spatial::UnitIndex;
use super::*;
use crate::command::{Command, PlayerCommand};
use crate::scenario::{PlayerSpec, Scenario, ScenarioMode, UnitSpec};
use crate::stats::UnitKind;

/// The reference acquisition: a full scan of the unit list. The indexed
/// window prunes candidates and must never change the pick.
fn linear_acquire(state: &State, id: UnitId) -> Option<Target> {
    let unit = state.unit(id).expect("caller checked");
    let stats = unit.kind.stats();
    if !stats.can_fight() {
        return None;
    }
    let (pos, me) = (unit.pos, unit.player);
    let aggro_sq = stats.aggro_range * stats.aggro_range;
    let unit_target = state
        .units
        .iter()
        .filter(|u| {
            state.hostile(me, u.player)
                && u.hp > 0
                && stats.can_target(u.domain())
                && state.can_see(me, u.tile())
        })
        .map(|u| (pos.dist_sq(u.pos), u.id))
        .filter(|(d, uid)| {
            *d <= aggro_sq
                && stats.weapons.iter().any(|weapon| {
                    weapon.targets.covers(
                        state
                            .unit(*uid)
                            .expect("candidate exists")
                            .kind
                            .stats()
                            .domain,
                    ) && *d >= weapon.minimum_range * weapon.minimum_range
                })
        })
        .min();
    if let Some((_, uid)) = unit_target {
        return Some(Target::Unit(uid));
    }
    if !stats.can_target(Domain::Ground) {
        return None;
    }
    state
        .buildings
        .iter()
        .filter(|b| state.hostile(me, b.player) && b.hp > 0)
        .filter(|b| b.tiles().any(|tile| state.can_see(me, tile)))
        .map(|b| (pos.dist_sq(b.closest_point_to(pos)), b.id))
        .filter(|(d, _)| {
            *d <= aggro_sq
                && stats.weapons.iter().any(|weapon| {
                    weapon.targets.covers(Domain::Ground)
                        && *d >= weapon.minimum_range * weapon.minimum_range
                })
        })
        .min()
        .map(|(_, bid)| Target::Building(bid))
}

fn seat(name: &str) -> PlayerSpec {
    PlayerSpec {
        name: name.into(),
        team: None,
        scrap: 0,
        bot: false,
        bot_config: None,
    }
}

fn boundary_duel() -> State {
    Scenario {
        mode: ScenarioMode::Match,
        name: "boundary-duel".into(),
        map: vec![
            "............".into(),
            "............".into(),
            "............".into(),
            "1.........2.".into(),
            "............".into(),
            "............".into(),
            "............".into(),
            "............".into(),
        ],
        players: vec![seat("North"), seat("South")],
        units: vec![
            UnitSpec {
                player: 0,
                kind: UnitKind::Sentinel,
                x: 5,
                y: 1,
            },
            UnitSpec {
                player: 1,
                kind: UnitKind::Sentinel,
                x: 6,
                y: 1,
            },
        ],
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .expect("boundary duel builds")
}

/// The acquisition window reaches past the map's first and last rows. A
/// victim on a border row is picked; one beyond it, which the coordinate
/// envelope accepts, stands on ground nobody can see and never is.
#[test]
fn indexed_acquisition_matches_the_scan_at_north_and_south_rows() {
    let mut state = boundary_duel();
    let height = state.map.height();
    let attacker = state.units[0].id;
    let victim = state.units[1].id;
    let mut index = UnitIndex::new();

    for (attacker_tile, victim_tile, expected) in [
        (
            TilePos::new(5, 1),
            TilePos::new(5, 0),
            Some(Target::Unit(victim)),
        ),
        (
            TilePos::new(5, height - 2),
            TilePos::new(5, height - 1),
            Some(Target::Unit(victim)),
        ),
        (TilePos::new(5, 0), TilePos::new(5, -1), None),
        (TilePos::new(5, height - 1), TilePos::new(5, height), None),
    ] {
        state.units[0].pos = attacker_tile.center();
        state.units[1].pos = victim_tile.center();
        state.refresh_vision();
        state
            .validate_invariants()
            .expect("the accepted coordinate envelope includes border rows");
        index.rebuild(&state.units);
        index.survey(&state);

        let indexed = acquire_target(&state, &index, attacker);
        assert_eq!(indexed, expected);
        assert_eq!(indexed, linear_acquire(&state, attacker));
    }
}

/// Two mixed armies crossing an open arena, compared pick-for-pick
/// against the linear scan every tick — range edges, weapon-mask
/// misses (flak vs ground, ground-only vs flyers), dying candidates,
/// and the building fallback near the far Foundry all churn as the
/// fronts meet and melt.
#[test]
fn indexed_acquisition_matches_the_linear_scan() {
    let width = 31usize;
    let height = 19usize;
    let mut rows = vec![vec!['#'; width]; height];
    for row in rows.iter_mut().take(height - 1).skip(1) {
        for cell in row.iter_mut().take(width - 1).skip(1) {
            *cell = '.';
        }
    }
    rows[1][1] = '1';
    rows[height - 3][width - 3] = '2';
    let west: &[UnitKind] = &[
        UnitKind::Sentinel,
        UnitKind::Sentinel,
        UnitKind::Scuttler,
        UnitKind::Lancer,
        UnitKind::Flakhound,
        UnitKind::Buzzard,
        UnitKind::Talon,
        UnitKind::Harvester,
    ];
    let east: &[UnitKind] = &[
        UnitKind::Sentinel,
        UnitKind::Sentinel,
        UnitKind::Scuttler,
        UnitKind::Lancer,
        UnitKind::Flakhound,
        UnitKind::Buzzard,
        UnitKind::Talon,
        UnitKind::Harvester,
    ];
    let mut units = Vec::new();
    for (&kind, i) in west.iter().zip(0_i32..) {
        let (dx, dy) = (i % 4, i / 4);
        units.push(UnitSpec {
            player: 0,
            kind,
            x: 3 + dx * 2,
            y: 4 + dy * 2,
        });
    }
    for (&kind, i) in east.iter().zip(0_i32..) {
        let (dx, dy) = (i % 4, i / 4);
        units.push(UnitSpec {
            player: 1,
            kind,
            x: 27 - dx * 2,
            y: 14 - dy * 2,
        });
    }
    let scenario = Scenario {
        mode: ScenarioMode::Match,
        name: "acquisition-differential".into(),
        map: rows.into_iter().map(|r| r.into_iter().collect()).collect(),
        players: vec![seat("West"), seat("East")],
        units,
        buildings: Vec::new(),
        meta: None,
    };
    let mut state = scenario.build().expect("arena builds");
    let march = |state: &State, player: u8, goal: TilePos| -> PlayerCommand {
        PlayerCommand {
            player: PlayerId(player),
            command: Command::Hunt {
                units: state
                    .units
                    .iter()
                    .filter(|u| u.player == PlayerId(player))
                    .map(|u| u.id)
                    .collect(),
                goal,
                queue: false,
            },
        }
    };
    let opening = [
        march(&state, 0, TilePos::new(27, 15)),
        march(&state, 1, TilePos::new(3, 3)),
    ];
    state.tick(&opening);

    let mut index = UnitIndex::new();
    let mut picks = 0usize;
    for _ in 0..400 {
        state.tick(&[]);
        index.rebuild(&state.units);
        index.survey(&state);
        for unit in &state.units {
            if unit.hp == 0 {
                continue;
            }
            let indexed = acquire_target(&state, &index, unit.id);
            assert_eq!(
                indexed,
                linear_acquire(&state, unit.id),
                "unit {:?} at tick {}",
                unit.id,
                state.tick
            );
            picks += usize::from(indexed.is_some());
        }
    }
    assert!(picks > 100, "the armies never met ({picks} picks)");
}

/// A plain linear scan: the reference the differential below compares
/// `sidearm_victim`'s windowed search against.
fn linear_sidearm_victim(
    state: &State,
    shooter_pos: Vec2Fx,
    me: PlayerId,
    shooter_domain: Domain,
    weapon: &WeaponStats,
) -> Option<(Fx, UnitId, Vec2Fx, Domain)> {
    let shot_open = |t: TilePos, full: bool| {
        state.map.tile(t).is_some_and(|tile| {
            !tile.terrain.blocks_all_fire() && (!full || !tile.terrain.blocks_direct_fire())
        })
    };
    state
        .units
        .iter()
        .filter(|u| state.hostile(me, u.player) && u.hp > 0 && weapon.targets.covers(u.domain()))
        .filter(|u| state.can_see(me, u.tile()))
        .map(|u| (shooter_pos.dist_sq(u.pos), u.id, u.pos, u.domain()))
        .filter(|(d, ..)| within_weapon_reach(weapon, *d, Fx::ZERO))
        .filter(|(_, _, upos, dom)| {
            let full = traces_terrain(weapon, shooter_domain, *dom);
            !chassis::path::line_blocked(shooter_pos, *upos, |t| shot_open(t, full))
        })
        .min_by_key(|&(d, uid, _, _)| (d, uid))
}

/// The same mixed-armies churn as the acquisition differential, but
/// comparing every multi-weapon unit's sidearm pick — window edges,
/// fog, air-vs-ground weapon masks, and dying candidates included.
#[test]
fn windowed_sidearm_victim_matches_the_linear_scan() {
    let width = 31usize;
    let height = 19usize;
    let mut rows = vec![vec!['#'; width]; height];
    for row in rows.iter_mut().take(height - 1).skip(1) {
        for cell in row.iter_mut().take(width - 1).skip(1) {
            *cell = '.';
        }
    }
    rows[1][1] = '1';
    rows[height - 3][width - 3] = '2';
    let west: &[UnitKind] = &[
        UnitKind::Sentinel,
        UnitKind::Sentinel,
        UnitKind::Sentinel,
        UnitKind::Buzzard,
        UnitKind::Talon,
        UnitKind::Flakhound,
    ];
    let east: &[UnitKind] = &[
        UnitKind::Sentinel,
        UnitKind::Sentinel,
        UnitKind::Talon,
        UnitKind::Talon,
        UnitKind::Buzzard,
        UnitKind::Scuttler,
    ];
    let mut units = Vec::new();
    for (&kind, i) in west.iter().zip(0_i32..) {
        let (dx, dy) = (i % 3, i / 3);
        units.push(UnitSpec {
            player: 0,
            kind,
            x: 3 + dx * 2,
            y: 4 + dy * 2,
        });
    }
    for (&kind, i) in east.iter().zip(0_i32..) {
        let (dx, dy) = (i % 3, i / 3);
        units.push(UnitSpec {
            player: 1,
            kind,
            x: 27 - dx * 2,
            y: 14 - dy * 2,
        });
    }
    let scenario = Scenario {
        mode: ScenarioMode::Match,
        name: "sidearm-differential".into(),
        map: rows.into_iter().map(|r| r.into_iter().collect()).collect(),
        players: vec![seat("West"), seat("East")],
        units,
        buildings: Vec::new(),
        meta: None,
    };
    let mut state = scenario.build().expect("arena builds");
    let march = |state: &State, player: u8, goal: TilePos| -> PlayerCommand {
        PlayerCommand {
            player: PlayerId(player),
            command: Command::Hunt {
                units: state
                    .units
                    .iter()
                    .filter(|u| u.player == PlayerId(player))
                    .map(|u| u.id)
                    .collect(),
                goal,
                queue: false,
            },
        }
    };
    let opening = [
        march(&state, 0, TilePos::new(27, 15)),
        march(&state, 1, TilePos::new(3, 3)),
    ];
    state.tick(&opening);

    let mut index = UnitIndex::new();
    let mut picks = 0usize;
    for _ in 0..400 {
        state.tick(&[]);
        index.rebuild(&state.units);
        for unit in &state.units {
            let stats = unit.kind.stats();
            if unit.hp == 0 || stats.weapons.len() < 2 {
                continue;
            }
            for weapon in stats.weapons {
                let windowed =
                    sidearm_victim(&state, &index, unit.pos, unit.player, stats.domain, weapon);
                assert_eq!(
                    windowed,
                    linear_sidearm_victim(&state, unit.pos, unit.player, stats.domain, weapon),
                    "unit {:?} at tick {}",
                    unit.id,
                    state.tick
                );
                picks += usize::from(windowed.is_some());
            }
        }
    }
    assert!(picks > 50, "no sidearm ever found a victim ({picks} picks)");
}

#[test]
fn motion_snapshot_tracks_motor_speed_through_retargeting_and_coasting() {
    let mut state = boundary_duel();
    let target = &mut state.units[1];
    target.kind = UnitKind::Scuttler;
    target.heading = 0;
    target.set_drive_speed(target.kind.stats().speed / 6);
    target.path = Some(PathFollow {
        final_point: None,
        goal: TilePos::new(3, 1),
        waypoints: vec![TilePos::new(5, 1), TilePos::new(3, 1)],
        next: 0,
    });
    let (id, pos, speed) = (target.id, target.pos, target.drive_speed());
    let expected = pos + Vec2Fx::new(speed, Fx::ZERO);
    assert_eq!(
        MotionSnapshot::capture(&state).position_after(id, pos, 1),
        Some(expected)
    );

    state.units[1].path = None;
    assert_eq!(
        MotionSnapshot::capture(&state).position_after(id, pos, 1),
        Some(expected)
    );
    state.units[1].set_drive_speed(Fx::ZERO);
    state.units[1].path = Some(PathFollow {
        final_point: None,
        goal: TilePos::new(8, 1),
        waypoints: vec![TilePos::new(8, 1)],
        next: 0,
    });
    assert_eq!(
        MotionSnapshot::capture(&state).position_after(id, pos, 1),
        None
    );
}

#[test]
fn motion_snapshot_ignores_later_route_turns() {
    let aim_with_later_turn = |turn: TilePos| {
        let mut state = boundary_duel();
        state.units[0].kind = UnitKind::Bombard;
        state.units[0].pos = TilePos::new(2, 1).center();
        state.units[1].kind = UnitKind::Scuttler;
        state.units[1].pos = TilePos::new(7, 1).center();
        state.units[1].heading = 0;
        state.units[1].set_drive_speed(UnitKind::Scuttler.stats().speed);
        let target = state.units[1].id;
        state.units[1].path = Some(PathFollow {
            final_point: None,
            goal: turn,
            waypoints: vec![TilePos::new(8, 1), turn],
            next: 0,
        });
        let motion = MotionSnapshot::capture(&state);
        let shooter = state.units[0].pos;
        let current = state.units[1].pos;
        projectile_aim(
            &state,
            &motion,
            ProjectileShooter {
                owner: PlayerId(0),
                domain: Domain::Ground,
            },
            shooter,
            Target::Unit(target),
            current,
            &UnitKind::Bombard.stats().weapons[0],
        )
    };

    assert_eq!(
        aim_with_later_turn(TilePos::new(8, 6)),
        aim_with_later_turn(TilePos::new(8, -4)),
        "future A* turns are private intent, not observable velocity"
    );
}

#[test]
fn predictive_splash_aim_uses_the_near_edge_of_the_footprint() {
    let current = Vec2Fx::new(Fx::from_num(4), Fx::from_num(3));
    let predicted = Vec2Fx::new(Fx::from_num(9), Fx::from_num(3));
    let radius = Fx::lit("1.4");

    let aim = aim_at_near_splash_edge(current, predicted, radius);

    let expected = Vec2Fx::new(Fx::lit("7.6"), Fx::from_num(3));
    let tolerance = Fx::DELTA * Fx::from_num(8);
    assert!(
        aim.dist(expected) <= tolerance,
        "fixed-point direction scaling stays within eight ulps of the near edge"
    );
    assert!(
        aim.dist(predicted) <= radius && aim.dist(predicted) >= radius - tolerance,
        "a straight mover sits exactly on the predicted blast edge: distance={}, radius={}, tolerance={}",
        aim.dist(predicted),
        radius,
        tolerance
    );
}

/// Range is measured between positions while sight covers whole tiles, so
/// a victim on the tile just past a Breaker's sight disc can sit well
/// inside its equal aggro range. It must never be picked: the attack order
/// would drop it the next tick, and the hunt would re-acquire it forever.
#[test]
fn a_victim_in_range_but_out_of_sight_is_never_acquired() {
    let width = 30usize;
    let height = 12usize;
    let map = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| match (x, y) {
                    (1, 10) => '1',
                    (28, 1) => '2',
                    _ => '.',
                })
                .collect::<String>()
        })
        .collect();
    let mut state = Scenario {
        mode: ScenarioMode::Match,
        name: "unseen-edge".into(),
        map,
        players: vec![seat("West"), seat("East")],
        units: vec![
            UnitSpec {
                player: 0,
                kind: UnitKind::Breaker,
                x: 14,
                y: 6,
            },
            UnitSpec {
                player: 1,
                kind: UnitKind::Sentinel,
                x: 20,
                y: 7,
            },
        ],
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .expect("unseen-edge builds");
    let breaker = state.units[0].id;
    let me = state.units[0].player;
    // Tile offset (6, 1) lies outside the radius-6 sight disc, yet the
    // positions are only about 5.2 tiles apart.
    state.units[0].pos = chassis::fx::Vec2Fx::new(Fx::lit("14.9"), Fx::lit("6.9"));
    state.units[1].pos = chassis::fx::Vec2Fx::new(Fx::lit("20.1"), Fx::lit("7.1"));
    state.refresh_vision();
    let stats = UnitKind::Breaker.stats();
    assert!(
        state.units[0].pos.dist_sq(state.units[1].pos) <= stats.aggro_range * stats.aggro_range
    );
    assert!(!state.can_see(me, state.units[1].tile()));

    let mut index = UnitIndex::new();
    index.rebuild(&state.units);
    index.survey(&state);
    assert_eq!(acquire_target(&state, &index, breaker), None);
    assert_eq!(linear_acquire(&state, breaker), None);

    let start = state.units[0].pos;
    state.tick(&[PlayerCommand {
        player: me,
        command: Command::Hunt {
            units: vec![breaker],
            goal: TilePos::new(27, 10),
            queue: false,
        },
    }]);
    for _ in 0..100 {
        state.tick(&[]);
    }
    let end = state.unit(breaker).expect("the Breaker lives").pos;
    assert!(
        start.dist_sq(end) > Fx::from_num(1),
        "the hunt never left its tile"
    );
}

/// An East Scuttler and a West `kind` whose centre stands at `pos`, inside
/// the tile of a West Barricade at (6, 2), as an attacking body may in a
/// footprint's empty margin.
fn overhanging(kind: UnitKind, pos: Vec2Fx) -> (State, UnitId, UnitId) {
    let mut state = Scenario {
        mode: ScenarioMode::Match,
        name: "overhang".into(),
        map: vec![
            "................".into(),
            "................".into(),
            "................".into(),
            "1.............2.".into(),
            "................".into(),
            "................".into(),
            "................".into(),
            "................".into(),
        ],
        players: vec![seat("West"), seat("East")],
        units: vec![
            UnitSpec {
                player: 0,
                kind,
                x: 6,
                y: 1,
            },
            UnitSpec {
                player: 1,
                kind: UnitKind::Scuttler,
                x: 9,
                y: 1,
            },
        ],
        buildings: vec![crate::scenario::BuildingSpec {
            player: 0,
            kind: crate::stats::BuildingKind::Barricade,
            x: 6,
            y: 2,
        }],
        meta: None,
    }
    .build()
    .expect("overhang builds");
    let (victim, scuttler) = (state.units[0].id, state.units[1].id);
    state.units[0].pos = pos;
    state.refresh_vision();
    state
        .validate_invariants()
        .expect("a body may overhang a footprint's margin");
    (state, victim, scuttler)
}

/// A Scuttler and an Avalanche whose centre stands just inside the
/// Barricade's tile.
fn overhanging_victim() -> (State, UnitId, UnitId) {
    overhanging(
        UnitKind::Avalanche,
        Vec2Fx::new(Fx::lit("6.74"), Fx::lit("2.02")),
    )
}

/// A contact weapon's reach of a small victim falls short of the weapon's
/// range. The tile above stands within that range of a Scuttler 0.26 tiles
/// inside the Barricade's tile, but beyond the hulls plus contact reach, so
/// a chaser parked there could never fire.
#[test]
fn a_contact_chaser_takes_no_stand_in_beyond_its_reach() {
    let (mut state, victim, scuttler) = overhanging(
        UnitKind::Scuttler,
        Vec2Fx::new(Fx::lit("6.5"), Fx::lit("2.26")),
    );
    let report = state.tick(&[PlayerCommand {
        player: crate::PlayerId(1),
        command: Command::Attack {
            units: vec![scuttler],
            target: Target::Unit(victim).into(),
            queue: false,
        },
    }]);
    assert!(
        report.events.iter().any(|event| matches!(
            event,
            crate::Event::OrderStalled {
                unit,
                reason: crate::StallReason::NoFiringPosition,
                ..
            } if *unit == scuttler
        )),
        "{:?}",
        report.events
    );
}

/// A contact weapon cannot reach the centre of the blocked tile from any
/// tile beside it, but it reaches the body itself from the tile above.
#[test]
fn a_contact_chaser_closes_on_a_victim_overhanging_blocked_ground() {
    let (mut state, avalanche, scuttler) = overhanging_victim();
    assert!(
        !state.passable(state.unit(avalanche).unwrap().tile()),
        "premise: the victim's tile is blocked"
    );
    let report = state.tick(&[PlayerCommand {
        player: crate::PlayerId(1),
        command: Command::Attack {
            units: vec![scuttler],
            target: Target::Unit(avalanche).into(),
            queue: false,
        },
    }]);
    assert!(
        !report
            .events
            .iter()
            .any(|event| matches!(event, crate::Event::OrderStalled { .. })),
        "{:?}",
        report.events
    );
    assert!(matches!(
        state.unit(scuttler).unwrap().order,
        Order::Attack { .. }
    ));
    for _ in 0..100 {
        state.tick(&[]);
    }
    assert!(state.unit(avalanche).unwrap().hp < UnitKind::Avalanche.stats().max_hp);
}

#[test]
fn a_victim_overhanging_blocked_ground_is_acquired_by_a_contact_weapon() {
    let (state, avalanche, scuttler) = overhanging_victim();
    let mut index = UnitIndex::new();
    index.rebuild(&state.units);
    index.survey(&state);
    assert_eq!(
        acquire_target(&state, &index, scuttler),
        Some(Target::Unit(avalanche))
    );
}
