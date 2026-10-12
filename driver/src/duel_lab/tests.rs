use super::*;

fn plan(budget: u32) -> Plan {
    Plan {
        budget,
        units: armed_units(),
        attackers: defense_attackers(),
        defenses: armed_defenses(),
        flanks: true,
        tick_limit: 4_000,
    }
}

/// The deployed army's seat and the flanker's.
fn seats(leg: &FlankLeg) -> (PlayerId, PlayerId) {
    if leg.deployed_west {
        (PlayerId(0), PlayerId(1))
    } else {
        (PlayerId(1), PlayerId(0))
    }
}

fn flank_leg(
    flanker: UnitKind,
    deployed: UnitKind,
    budget: u32,
    flank: Flank,
    formation: Formation,
    deployed_west: bool,
) -> FlankLeg {
    let (flanker, deployed) =
        Side::matched(Contender::Unit(flanker), Contender::Unit(deployed), budget);
    FlankLeg {
        flanker,
        deployed,
        flank,
        formation,
        deployed_west,
    }
}

#[test]
fn sides_round_to_the_nearest_whole_body() {
    let breakers = Side::sized(Contender::Unit(UnitKind::Breaker), 1_800);
    assert_eq!((breakers.count, breakers.spent), (2, 1_800));
    let lancers = Side::sized(Contender::Unit(UnitKind::Lancer), 1_800);
    assert_eq!((lancers.count, lancers.spent), (16, 1_760));
    let turrets = Side::sized(
        Contender::Defense(Defense::base(BuildingKind::Turret)),
        1_800,
    );
    assert_eq!((turrets.count, turrets.spent), (18, 1_800));
    let lone = Side::sized(Contender::Unit(UnitKind::Breaker), 100);
    assert_eq!((lone.count, lone.spent), (1, 900));
}

#[test]
fn every_layout_mirrors_through_the_center() {
    for layout in Layout::ALL {
        let map = layout.map();
        let rows: Vec<Vec<char>> = map.iter().map(|row| row.chars().collect()).collect();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(
                    rows[as_index(y)][as_index(x)],
                    rows[as_index(HEIGHT - 1 - y)][as_index(WIDTH - 1 - x)],
                    "{layout:?} breaks symmetry at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn every_planned_duel_stages_at_small_and_large_budgets() {
    for budget in [600, 1_800, 5_000] {
        for duel in plan(budget).duels() {
            let sc = scenario(&duel);
            sc.build()
                .unwrap_or_else(|e| panic!("{} at {budget}: {e}", sc.name));
        }
    }
}

#[test]
fn pairings_where_nobody_can_hurt_anybody_are_left_out() {
    let duels = plan(1_800).duels();
    let met = |a: UnitKind, b: UnitKind| {
        duels
            .iter()
            .any(|d| (d.a.contender, d.b.contender) == (Contender::Unit(a), Contender::Unit(b)))
    };
    // Anti-air only against ground only.
    assert!(!met(UnitKind::Talon, UnitKind::Lancer));
    // One-sided fights still run.
    assert!(met(UnitKind::Lancer, UnitKind::Buzzard));
    // Sappers only fight buildings.
    assert!(
        duels
            .iter()
            .all(|d| d.a.contender != Contender::Unit(UnitKind::Sapper)
                || matches!(d.b.contender, Contender::Defense(_)))
    );
    // Each pairing runs on every layout, formation and seat.
    let legs = duels
        .iter()
        .filter(|d| {
            (d.a.contender, d.b.contender)
                == (
                    Contender::Unit(UnitKind::Lancer),
                    Contender::Unit(UnitKind::Buzzard),
                )
        })
        .count();
    assert_eq!(legs, Layout::ALL.len() * Formation::ALL.len() * 2);
}

#[test]
fn a_mirror_match_scores_the_same_from_either_seat() {
    let side = Side::sized(Contender::Unit(UnitKind::Sentinel), 900);
    let leg = |a_west| Duel {
        a: side,
        b: side,
        layout: Layout::Open,
        formation: Formation::Line,
        a_west,
    };
    let west = run(&leg(true), 2_000).expect("west leg runs");
    let east = run(&leg(false), 2_000).expect("east leg runs");
    assert_eq!(west.score, -east.score);
    assert_eq!(west.ticks, east.ticks);
}

#[test]
fn a_duel_replays_identically() {
    let duel = Duel {
        a: Side::sized(Contender::Unit(UnitKind::Lancer), 900),
        b: Side::sized(Contender::Unit(UnitKind::Bombard), 900),
        layout: Layout::Cover,
        formation: Formation::Clump,
        a_west: true,
    };
    let first = run(&duel, 2_000).expect("duel runs");
    assert_eq!(first, run(&duel, 2_000).expect("duel runs again"));
    assert!(first.ticks > 1, "the armies met and fought");
}

fn matrix(kinds: &[UnitKind], cells: Vec<Vec<Option<f64>>>) -> Matrix {
    let contenders: Vec<Contender> = kinds.iter().copied().map(Contender::Unit).collect();
    Matrix {
        rows: contenders.clone(),
        columns: contenders,
        cells,
    }
}

#[test]
fn flags_name_dominance_look_alikes_and_tier_margins() {
    use UnitKind::{Breaker, Lancer, Scuttler, Sentinel, Talon, Warden};
    let kinds = [Breaker, Sentinel, Scuttler, Lancer, Warden, Talon];
    // Breaker beats everything it meets, and against everyone else it scores
    // the way Lancer does, only better: it outclasses Lancer. Sentinel and
    // Scuttler score alike against everyone else and evenly against each
    // other. Talon meets nobody on the ground.
    let m = matrix(
        &kinds,
        vec![
            vec![Some(0.0), Some(0.8), Some(0.9), Some(0.3), Some(0.6), None],
            vec![
                Some(-0.8),
                Some(0.0),
                Some(0.05),
                Some(-0.4),
                Some(-0.3),
                None,
            ],
            vec![
                Some(-0.9),
                Some(-0.05),
                Some(0.0),
                Some(-0.5),
                Some(-0.4),
                None,
            ],
            vec![Some(-0.3), Some(0.4), Some(0.5), Some(0.0), Some(0.5), None],
            vec![
                Some(-0.6),
                Some(0.3),
                Some(0.4),
                Some(-0.5),
                Some(0.0),
                None,
            ],
            vec![None, None, None, None, None, Some(0.0)],
        ],
    );
    let flags = flags(&m, &[]);
    assert_eq!(flags.dominant, vec![Breaker]);
    assert!(flags.dominated.is_empty(), "{:?}", flags.dominated);
    assert_eq!(flags.alike.len(), 1, "{:?}", flags.alike);
    assert_eq!((flags.alike[0].a, flags.alike[0].b), (Sentinel, Scuttler));
    assert_eq!(flags.outclassed.len(), 1, "{:?}", flags.outclassed);
    assert_eq!(
        (flags.outclassed[0].a, flags.outclassed[0].b),
        (Breaker, Lancer)
    );
    let t3_vs_t1 = flags
        .tiers
        .iter()
        .find(|t| (t.higher, t.lower) == (3, 1))
        .expect("tier 3 met tier 1");
    assert!((t3_vs_t1.mean - 0.85).abs() < 1e-9, "{t3_vs_t1:?}");
}

#[test]
fn tiers_follow_producers_and_prerequisites() {
    assert_eq!(tier(UnitKind::Sentinel), 1);
    assert_eq!(tier(UnitKind::Lancer), 2);
    assert_eq!(tier(UnitKind::Talon), 2);
    assert_eq!(tier(UnitKind::Breaker), 3);
    assert_eq!(tier(UnitKind::Condor), 3);
}

#[test]
fn upgraded_defenses_refit_before_the_fight() {
    let bulwark = Defense {
        kind: BuildingKind::Turret,
        tier: 2,
    };
    assert_eq!(bulwark.cost(), 100 + 150 + 300);
    assert_eq!(
        bulwark.prerequisites(),
        vec![BuildingKind::Fabricator, BuildingKind::Crucible]
    );
    let duel = Duel {
        a: Side::sized(Contender::Unit(UnitKind::Lancer), 1_100),
        b: Side::sized(Contender::Defense(bulwark), 1_100),
        layout: Layout::Open,
        formation: Formation::Line,
        a_west: true,
    };
    let sc = scenario(&duel);
    let mut state = sc.build().expect("the line stages");
    refit(
        &mut state,
        [
            (PlayerId(0), &Force::of(duel.a)),
            (PlayerId(1), &Force::of(duel.b)),
        ],
    )
    .expect("the line refits");
    let line: Vec<_> = state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Turret)
        .collect();
    assert_eq!(line.len(), 2);
    assert!(
        line.iter()
            .all(|b| b.tier == 2 && b.hp == bulwark.stats().max_hp)
    );
    assert_eq!(
        state.player(PlayerId(1)).scrap,
        0,
        "the seat held exactly the refit cost"
    );
}

#[test]
fn every_armed_defense_tier_is_on_the_roster() {
    let names: Vec<_> = armed_defenses()
        .into_iter()
        .map(|d| d.kind.tier_name(d.tier))
        .collect();
    for name in [
        "turret",
        "heavy turret",
        "bulwark",
        "flak turret",
        "burst flak",
        "bastion",
    ] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
}

#[test]
fn a_small_plan_reports_both_matrices_and_its_flags() {
    let plan = Plan {
        budget: 600,
        units: vec![UnitKind::Lancer, UnitKind::Bombard],
        attackers: vec![UnitKind::Sapper],
        defenses: vec![Defense::base(BuildingKind::Bastion)],
        flanks: true,
        tick_limit: 1_500,
    };
    let jobs = NonZeroUsize::new(2).expect("two workers");
    let outcomes = run_plan(&plan, jobs).expect("the plan runs");
    assert_eq!(
        outcomes.len(),
        4 * Layout::ALL.len() * Formation::ALL.len() * 2
    );
    let flank_outcomes = run_flanks(&plan, jobs).expect("the flanks run");
    assert_eq!(
        flank_outcomes.len(),
        4 * Flank::ALL.len() * Formation::ALL.len() * 2
    );
    let report = report(&plan, outcomes, flank_outcomes);
    assert_eq!(report.units.cells.len(), 2);
    assert!(report.units.cells.iter().flatten().all(Option::is_some));
    assert_eq!(
        report.defenses.cells,
        vec![vec![report.defenses.cells[0][0]]]
    );
    assert!(
        report.defenses.cells[0][0].is_some(),
        "Sappers fought the Bastions"
    );
    assert_eq!(report.flanks.len(), Flank::ALL.len());
    assert!(
        report.flanks.iter().all(|flanks| flanks
            .matrix
            .cells
            .iter()
            .flatten()
            .all(Option::is_some))
    );
    let table = render(&report);
    for section in [
        "units (",
        "defenses (",
        "side flanks, row flanking column (",
        "rear flanks, row flanking column (",
        "lance",
        "basti",
        "legs split by seat",
        "flank legs split by seat",
    ] {
        assert!(
            table.contains(section),
            "{section:?} missing from:\n{table}"
        );
    }
    serde_json::to_string(&report).expect("the report serializes");
}

#[test]
fn tier_margins_skip_matchups_where_one_side_cannot_fire() {
    use UnitKind::{Breaker, Flakhound, Sentinel};
    // Flakhounds shoot only aircraft, so their losses to Sentinels and
    // Breakers are one-sided and say nothing about tiers.
    let m = matrix(
        &[Breaker, Flakhound, Sentinel],
        vec![
            vec![Some(0.0), Some(1.0), Some(0.6)],
            vec![Some(-1.0), None, Some(-1.0)],
            vec![Some(-0.6), Some(1.0), Some(0.0)],
        ],
    );
    let tiers = flags(&m, &[]).tiers;
    assert_eq!(tiers.len(), 1, "{tiers:?}");
    assert_eq!(
        (tiers[0].higher, tiers[0].lower, tiers[0].matchups),
        (3, 1, 1)
    );
    assert!((tiers[0].mean - 0.6).abs() < 1e-9);
}

#[test]
fn a_pairing_against_a_defense_line_scores_the_same_from_either_seat() {
    // Swapping seats rotates the whole scene, so the score must not move.
    // Even footprints have no center tile, so a goal taken from a building's
    // anchor would differ by one tile between the seats.
    let (a, b) = Side::matched(
        Contender::Unit(UnitKind::Avalanche),
        Contender::Defense(Defense::base(BuildingKind::Bastion)),
        1_800,
    );
    let leg = |a_west| Duel {
        a,
        b,
        layout: Layout::Open,
        formation: Formation::Line,
        a_west,
    };
    let west = run(&leg(true), 8_000).expect("west leg runs");
    let east = run(&leg(false), 8_000).expect("east leg runs");
    assert_eq!((west.score, west.ticks), (east.score, east.ticks));
}

#[test]
fn flank_legs_start_out_of_sight_and_reach_at_small_and_large_budgets() {
    let direct_reach = |kind: UnitKind| {
        kind.stats()
            .weapons
            .iter()
            .filter(|weapon| !weapon.indirect)
            .map(|weapon| weapon.range)
            .max()
            .unwrap_or(chassis::fx::Fx::ZERO)
    };
    for budget in [600, 1_800, 5_000] {
        for leg in plan(budget).flank_legs() {
            let scene = stage_flank(&leg).expect("a flank leg fields units");
            let state = scene
                .scenario
                .build()
                .unwrap_or_else(|e| panic!("{} at {budget}: {e}", scene.scenario.name));
            let (deployed, flanker) = seats(&leg);
            let side = |player| state.units().iter().filter(move |u| u.player == player);
            assert!(
                side(deployed).all(|u| state.can_see(deployed, u.tile())),
                "premise: sight is live at the start"
            );
            for f in side(flanker) {
                for d in side(deployed) {
                    let name = &scene.scenario.name;
                    assert!(!state.can_see(deployed, f.tile()), "{name} at {budget}");
                    assert!(!state.can_see(flanker, d.tile()), "{name} at {budget}");
                    let reach = direct_reach(f.kind).max(direct_reach(d.kind));
                    assert!(f.pos.dist(d.pos) > reach, "{name} at {budget}");
                }
            }
        }
    }
}

#[test]
fn every_flank_pairing_runs_both_approaches_formations_and_seats() {
    use UnitKind::{Lancer, Talon, Warden};
    let legs = plan(1_800).flank_legs();
    let of = |flanker: UnitKind, deployed: UnitKind| -> Vec<&FlankLeg> {
        legs.iter()
            .filter(|leg| {
                (leg.flanker.contender, leg.deployed.contender)
                    == (Contender::Unit(flanker), Contender::Unit(deployed))
            })
            .collect()
    };
    let wardens_on_lancers = of(Warden, Lancer);
    assert_eq!(wardens_on_lancers.len(), 8);
    for flank in Flank::ALL {
        for formation in Formation::ALL {
            for deployed_west in [true, false] {
                assert!(wardens_on_lancers.iter().any(|leg| {
                    (leg.flank, leg.formation, leg.deployed_west)
                        == (flank, formation, deployed_west)
                }));
            }
        }
    }
    // Flanking runs one way, so the reverse is its own pairing, and a kind
    // flanks its own kind too.
    assert_eq!(of(Lancer, Warden).len(), 8);
    assert_eq!(of(Warden, Warden).len(), 8);
    assert!(of(Talon, Lancer).is_empty(), "neither can hurt the other");
    let head_on = Plan {
        flanks: false,
        ..plan(1_800)
    };
    assert!(head_on.flank_legs().is_empty());
}

#[test]
fn a_rear_flank_starts_behind_the_army_and_a_side_flank_off_its_end() {
    for formation in Formation::ALL {
        for deployed_west in [true, false] {
            for flank in Flank::ALL {
                let leg = flank_leg(
                    UnitKind::Warden,
                    UnitKind::Lancer,
                    1_800,
                    flank,
                    formation,
                    deployed_west,
                );
                let state = stage_flank(&leg)
                    .expect("a flank leg fields units")
                    .scenario
                    .build()
                    .expect("the leg stages");
                let (deployed, flanker) = seats(&leg);
                let side = |player| state.units().iter().filter(move |u| u.player == player);
                let rows: Vec<i32> = side(deployed).map(|u| u.tile().y).collect();
                let (top, bottom) = (rows.iter().min().unwrap(), rows.iter().max().unwrap());
                for d in side(deployed) {
                    let facing = chassis::compass::dir(d.heading);
                    for f in side(flanker) {
                        let to = f.pos - d.pos;
                        let ahead = facing.x * to.x + facing.y * to.y;
                        match flank {
                            Flank::Rear => assert!(ahead < chassis::fx::Fx::ZERO, "{leg:?}"),
                            Flank::Side => {
                                let y = f.tile().y;
                                assert!(y < *top || y > *bottom, "{leg:?}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_flank_leg_scores_the_same_from_either_seat_and_replays_identically() {
    let leg = |deployed_west| {
        flank_leg(
            UnitKind::Warden,
            UnitKind::Lancer,
            900,
            Flank::Rear,
            Formation::Line,
            deployed_west,
        )
    };
    let west = run_flank(&leg(true), 2_000).expect("west leg runs");
    let east = run_flank(&leg(false), 2_000).expect("east leg runs");
    assert_eq!((west.score, west.ticks), (east.score, east.ticks));
    assert_eq!(west.end, End::Wipe, "the flanker reached the army");
    assert_eq!(
        west,
        run_flank(&leg(true), 2_000).expect("west leg runs again")
    );
}

#[test]
fn a_deployed_army_holds_until_it_sees_the_flanker() {
    let leg = flank_leg(
        UnitKind::Breaker,
        UnitKind::Lancer,
        1_800,
        Flank::Rear,
        Formation::Line,
        true,
    );
    let mut state = stage_flank(&leg)
        .expect("a flank leg fields units")
        .scenario
        .build()
        .expect("the leg stages");
    let (deployed, flanker) = (Force::of(leg.deployed), Force::of(leg.flanker));
    let start: Vec<_> = state
        .units()
        .iter()
        .filter(|u| u.player == PlayerId(0))
        .map(|u| (u.id, u.pos))
        .collect();
    // One seek round: the flanker's idle bodies go hunting, and an army that
    // did not wait for sight would too.
    play(
        &mut state,
        &deployed,
        &flanker,
        &[],
        Some(PlayerId(0)),
        SEEK_PERIOD + 1,
    );
    assert!(
        state
            .units()
            .iter()
            .any(|u| u.player == PlayerId(1) && u.order != Order::Idle),
        "premise: the flanker is on its way"
    );
    assert!(
        !sees(&state, PlayerId(0), PlayerId(1), &flanker),
        "premise: the flanker is still out of sight"
    );
    for (id, pos) in start {
        let unit = state.unit(id).expect("nobody has fought yet");
        assert_eq!((unit.order, unit.pos), (Order::Idle, pos));
    }
}
