use super::*;

fn crucible(x: i32, y: i32) -> BuildingObs {
    BuildingObs {
        provisional: false,
        id: BuildingId(1),
        player: aftermine_sim::PlayerId(1),
        kind: BuildingKind::Crucible,
        anchor: TilePos::new(x, y),
        hp: BuildingKind::Crucible.base_stats().max_hp,
        built: true,
        seen: true,
        tier: 0,
    }
}

#[test]
fn a_volley_takes_what_the_simulation_s_falloff_deals_and_no_more_than_is_left() {
    let mark = Mark::of(&crucible(10, 10));
    let paid = u64::from(BuildingKind::Crucible.invested_cost(0));
    let max = u64::from(BuildingKind::Crucible.base_stats().max_hp);
    let peak = u64::from(WARHEAD.peak_damage);
    assert_eq!(volley(&[mark], TilePos::new(10, 10), 1), paid * peak / max);
    // A second warhead takes only what the first left.
    assert_eq!(
        volley(&[mark], TilePos::new(10, 10), 2),
        paid * peak / max + paid * (max - peak) / max
    );
    // Four tiles from its nearest edge, nothing.
    assert_eq!(volley(&[mark], TilePos::new(16, 10), 1), 0);
}

#[test]
fn a_foundry_counts_its_resistance() {
    let mut foundry = crucible(10, 10);
    foundry.kind = BuildingKind::Foundry;
    foundry.hp = BuildingKind::Foundry.base_stats().max_hp;
    let mark = Mark::of(&foundry);
    let taken = u64::from(BuildingKind::Foundry.warhead_taken(WARHEAD.peak_damage));
    let paid = u64::from(BuildingKind::Foundry.invested_cost(0));
    let max = u64::from(BuildingKind::Foundry.base_stats().max_hp);
    assert_eq!(volley(&[mark], TilePos::new(11, 11), 1), paid * taken / max);
}

fn owned(kind: BuildingKind, x: i32, y: i32) -> Mark {
    Mark::of(&BuildingObs {
        kind,
        hp: kind.base_stats().max_hp,
        ..crucible(x, y)
    })
}

/// The repair bill for `missing` health of a building whose weld bills
/// against `basis`.
fn bill(basis: u64, missing: u32, max: u32) -> u64 {
    basis * u64::from(missing) * stats::REPAIR_COST_PERMILLE / (1000 * u64::from(max))
}

#[test]
fn a_hit_costs_its_repair_bill_and_a_kill_the_rebuild_less_the_repair_owed() {
    let mark = owned(BuildingKind::Crucible, 10, 10);
    let paid = u64::from(BuildingKind::Crucible.invested_cost(0));
    let max = BuildingKind::Crucible.base_stats().max_hp;
    let peak = WARHEAD.peak_damage;
    assert_eq!(
        recovered(&[mark], &[], TilePos::new(10, 10), 2),
        [bill(paid, peak, max), paid]
    );
    let worn = Mark { hp: 400, ..mark };
    assert_eq!(
        recovered(&[worn], &[], TilePos::new(10, 10), 1),
        [paid - bill(paid, max - 400, max)]
    );
}

#[test]
fn a_hit_bills_the_weld_the_simulation_charges_and_not_the_price_paid() {
    let foundry = owned(BuildingKind::Foundry, 10, 10);
    let max = BuildingKind::Foundry.base_stats().max_hp;
    let taken = BuildingKind::Foundry.warhead_taken(WARHEAD.peak_damage);
    let basis = u64::from(stats::FOUNDRY_REPAIR_PRICE);
    assert_eq!(
        recovered(&[foundry], &[], TilePos::new(11, 11), 1),
        [bill(basis, taken, max)]
    );
    let bore = Mark::of(&BuildingObs {
        kind: BuildingKind::Extractor,
        tier: 1,
        hp: BuildingKind::Extractor.tier_stats(1).max_hp,
        ..crucible(10, 10)
    });
    let max = BuildingKind::Extractor.tier_stats(1).max_hp;
    let basis = BuildingKind::Extractor.tier_stats(1).construction.cost;
    assert_ne!(u64::from(basis), bore.paid, "the upgrade alone bills");
    assert_eq!(
        recovered(&[bore], &[], TilePos::new(10, 10), 1),
        [bill(u64::from(basis), WARHEAD.peak_damage.min(max), max)]
    );
}

#[test]
fn a_site_counts_only_what_a_strike_takes_and_its_whole_price_when_felled() {
    let paid = u64::from(BuildingKind::MawCradle.invested_cost(0));
    let max = BuildingKind::MawCradle.base_stats().max_hp;
    let site = Mark {
        built: false,
        hp: 400,
        ..owned(BuildingKind::MawCradle, 10, 10)
    };
    assert_eq!(recovered(&[site], &[], TilePos::new(11, 11), 1), [paid]);
    let rising = Mark { hp: 1500, ..site };
    let peak = WARHEAD.peak_damage;
    assert_eq!(
        recovered(&[rising], &[], TilePos::new(11, 11), 1),
        [bill(paid, peak, max)]
    );
}

#[test]
fn losing_an_earner_costs_the_income_it_stops_until_it_stands_again() {
    let mut marks = [
        owned(BuildingKind::Foundry, 10, 10),
        owned(BuildingKind::Foundry, 40, 40),
        owned(BuildingKind::Extractor, 14, 10),
        owned(BuildingKind::Extractor, 70, 70),
        owned(BuildingKind::Reclaimer, 20, 20),
        Mark {
            rounds: 1,
            ..owned(BuildingKind::Spire, 30, 30)
        },
    ];
    recovery(&mut marks, &[]);
    let ticks = |kind: BuildingKind| u64::from(kind.base_stats().construction.build_ticks);
    let (foundry, extractor) = (ticks(BuildingKind::Foundry), ticks(BuildingKind::Extractor));
    let (supported, remote) = (yields(0, true, foundry), yields(0, false, foundry));
    assert_eq!(
        marks.map(|mark| mark.extra),
        [
            foundry / stats::FOUNDRY_DRIP_PERIOD + supported - remote,
            foundry / stats::FOUNDRY_DRIP_PERIOD,
            yields(0, true, extractor),
            yields(0, false, extractor),
            ticks(BuildingKind::Reclaimer) / stats::RECLAIMER_PERIOD,
            price(1),
        ]
    );
}

#[test]
fn a_seat_s_only_known_foundry_carries_all_else_it_is_known_to_hold() {
    let mut marks = [
        owned(BuildingKind::Foundry, 10, 10),
        owned(BuildingKind::Crucible, 20, 20),
        owned(BuildingKind::Crucible, 30, 30),
    ];
    let units: Vec<UnitObs> = serde_json::from_value(serde_json::json!([
        {"id": 1, "player": 1, "kind": "lancer", "tile": {"x": 5, "y": 5}, "hp": 50,
         "idle": false, "carrying": 0, "site": null},
    ]))
    .unwrap();
    recovery(&mut marks, &units);
    let crucible = u64::from(BuildingKind::Crucible.invested_cost(0));
    let lancer = u64::from(UnitKind::Lancer.stats().cost);
    assert_eq!(marks[0].extra, 2 * crucible + lancer);
}

#[test]
fn the_volley_that_ends_a_seat_counts_nothing_else_of_it_from_the_fall_on() {
    let mut marks = [
        owned(BuildingKind::Foundry, 10, 10),
        owned(BuildingKind::Crucible, 13, 10),
    ];
    recovery(&mut marks, &[]);
    let tile = TilePos::new(11, 11);
    let worth =
        u64::from(BuildingKind::Foundry.invested_cost(0) + BuildingKind::Crucible.invested_cost(0));
    let foundry = |hits: u32| {
        bill(
            u64::from(stats::FOUNDRY_REPAIR_PRICE),
            hits * BuildingKind::Foundry.warhead_taken(WARHEAD.peak_damage),
            BuildingKind::Foundry.base_stats().max_hp,
        )
    };
    let crucible = u64::from(BuildingKind::Crucible.invested_cost(0));
    let max = BuildingKind::Crucible.base_stats().max_hp;
    let struck = bill(crucible, WARHEAD.peak_damage, max);
    assert_eq!(
        recovered(&marks, &[], tile, 4),
        [
            foundry(1) + struck,
            foundry(2) + crucible,
            foundry(3) + crucible,
            worth,
        ]
    );
    let body = Body {
        kind: UnitKind::Excavator,
        owner: aftermine_sim::PlayerId(1),
        tile: TilePos::new(12, 13),
        hp: UnitKind::Excavator.stats().max_hp,
    };
    let (alone, beside) = (
        recovered(&marks, &[], tile, 4),
        recovered(&marks, &[body], tile, 4),
    );
    assert!(beside[0] > alone[0], "a unit counts until the seat falls");
    assert_eq!(beside[3], alone[3], "and is the seat's from then on");
}

#[test]
fn a_strike_on_a_tile_centre_takes_what_the_falloff_deals_at_the_closest_point() {
    for kind in BuildingKind::ALL {
        let mark = owned(kind, 20, 20);
        for y in 10..32 {
            for x in 10..32 {
                let tile = TilePos::new(x, y);
                assert_eq!(
                    mark.taken_at(tile),
                    mark.taken(tile.center()),
                    "{kind:?} struck at {tile:?}"
                );
            }
        }
    }
}

/// [`needed`] as it scored every volley at every aim.
fn needed_by_every_volley(aims: &[Aim], marks: &[Mark], have: u64) -> Option<(u64, u64)> {
    let spire = u64::from(BuildingKind::Spire.invested_cost(0));
    aims.iter()
        .flat_map(|aim| {
            recovered(marks, &[], aim.tile, most_hits())
                .into_iter()
                .zip(aim.interceptors + 1..)
                .filter(move |(value, spent)| {
                    value + aim.drawn >= price(*spent) + spire * spent.saturating_sub(have)
                })
                .map(|(value, spent)| (spent, value))
        })
        .min_by_key(|(spires, value)| (*spires, Reverse(*value)))
}

#[test]
fn the_spires_a_strike_needs_skip_no_volley_that_could_better_the_best() {
    let mut rng = chassis::rng::Pcg32::new(7, 0);
    let mut draw = |bound: usize| rng.next_index(bound);
    let tile = |x: usize, y: usize| {
        TilePos::new(
            i32::try_from(x).expect("small"),
            i32::try_from(y).expect("small"),
        )
    };
    let mut found = [0; 2];
    for _ in 0..400 {
        let marks: Vec<Mark> = (0..=draw(12))
            .map(|_| {
                let kind = BuildingKind::ALL[draw(BuildingKind::ALL.len())];
                let at = tile(10 + draw(12), 10 + draw(12));
                let mark = owned(kind, at.x, at.y);
                Mark {
                    owner: aftermine_sim::PlayerId(if draw(2) == 0 { 1 } else { 2 }),
                    hp: mark.max_hp - u32::try_from(draw(mark.max_hp as usize)).expect("health"),
                    built: draw(4) > 0,
                    extra: draw(2_000) as u64,
                    last: kind == BuildingKind::Foundry && draw(2) == 0,
                    ..mark
                }
            })
            .collect();
        let marks = by_column(marks);
        let aims: Vec<Aim> = (0..=draw(16))
            .map(|_| Aim {
                tile: tile(8 + draw(16), 8 + draw(16)),
                interceptors: draw(4) as u64,
                drawn: draw(2_000) as u64,
            })
            .collect();
        let have = draw(4) as u64;
        let needs = needed(&aims, &marks, have);
        assert_eq!(needs, needed_by_every_volley(&aims, &marks, have));
        found[usize::from(needs.is_some())] += 1;
    }
    assert!(found.iter().all(|cases| *cases > 20), "{found:?}");
}
