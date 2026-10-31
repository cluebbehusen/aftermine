//! The Repair Bay's welding aura: billed sustain in a ring, through the
//! same buffered resolvers as every unit and building heal. Headless
//! scenarios through the public API only, like `repair_unit.rs` — billing
//! exactness against the hp-anchored meter, broke-owner starvation, the
//! ring's edge, air and structure patients, own-entity-only scope, and fire
//! winning the tick.

use crate::common;
use common::{cmd, run_until, unit};

use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::stats::FOUNDRY_RECOVERY_RESERVE;
use aftermine_sim::{
    BuildingId, BuildingKind, Command, Event, PlayerCommand, PlayerId, Scenario, State, Target,
    UnitId, UnitKind,
};
use chassis::grid::TilePos;

/// Bay footprint anchored at (4,4): its rect spans world [4,6]x[4,6], so
/// a tile-centered patient sits at distance k + 0.5 from the east face —
/// 3.5 tiles at x=9 (inside the 4.0 ring), 4.5 at x=10 (outside).
const BAY_ANCHOR: (i32, i32) = (4, 4);

/// In the ring, one tile off the bay's east face.
const RING: TilePos = TilePos { x: 7, y: 4 };

/// Well outside the ring — where the wounding happens, so no pulse
/// bills while the raider works.
const FAR: TilePos = TilePos { x: 14, y: 4 };

fn arena(units: Vec<UnitSpec>, scrap: u32, bay: bool) -> Scenario {
    let buildings = if bay {
        vec![BuildingSpec {
            player: 0,
            kind: BuildingKind::RepairBay,
            x: BAY_ANCHOR.0,
            y: BAY_ANCHOR.1,
        }]
    } else {
        Vec::new()
    };
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "bay-arena".into(),
        map: vec![
            "####################".into(),
            "#1.................#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "#................2.#".into(),
            "#..................#".into(),
            "#..................#".into(),
            "####################".into(),
        ],
        players: vec![
            PlayerSpec {
                name: "Owner".into(),
                team: None,
                scrap,
                bot: false,
                bot_config: None,
            },
            PlayerSpec {
                name: "Raider".into(),
                team: None,
                scrap: 200,
                bot: false,
                bot_config: None,
            },
        ],
        units,
        buildings,
        meta: None,
    }
}

use common::building as structure;

fn walk(player: u8, units: Vec<UnitId>, goal: TilePos) -> PlayerCommand {
    cmd(
        player,
        Command::Run {
            units,
            goal,
            queue: false,
        },
    )
}

/// Walks the raider beside the patient at FAR, lets auto-acquire gnaw to
/// at most `floor` hp, then pulls the raider back to its corner. Returns
/// the wound.
fn wound(state: &mut State, patient: UnitId, raider: UnitId, floor: u32) -> u32 {
    let max = state.unit(patient).unwrap().kind.stats().max_hp;
    state.tick(&[walk(1, vec![raider], TilePos::new(FAR.x, FAR.y - 2))]);
    run_until(state, 3_000, |s, _| s.unit(patient).unwrap().hp <= floor);
    state.tick(&[walk(1, vec![raider], TilePos::new(17, 10))]);
    run_until(state, 800, |s, _| {
        s.unit(raider).unwrap().tile() == TilePos::new(17, 10)
    });
    let hp = state.unit(patient).unwrap().hp;
    assert!(
        hp > 0 && hp < max,
        "test premise: the gnawing must leave a live patient (hp {hp})"
    );
    hp
}

/// The aura's whole bill for healing `from..=max` hp of a kind, in
/// scrap: the hp-anchored milli-scrap meter's ceiling telescoped across
/// the span — the same integer arithmetic the sim runs.
fn aura_bill(kind: UnitKind, from: u32, to: u32) -> u32 {
    let stats = kind.stats();
    let millis = |hp: u32| -> u64 {
        u64::from(hp) * u64::from(stats.cost) * aftermine_sim::stats::REPAIR_COST_PERMILLE
            / u64::from(stats.max_hp)
    };
    u32::try_from(millis(to).div_ceil(1000) - millis(from).div_ceil(1000)).unwrap()
}

fn building_aura_bill(kind: BuildingKind, tier: u8, from: u32, to: u32) -> u32 {
    let stats = kind.tier_stats(tier);
    let basis = if kind == BuildingKind::Foundry {
        aftermine_sim::stats::FOUNDRY_REPAIR_PRICE
    } else {
        stats
            .construction
            .map_or(aftermine_sim::stats::FOUNDRY_REPAIR_PRICE, |row| row.cost)
    };
    let millis = |hp: u32| -> u64 {
        u64::from(hp) * u64::from(basis) * aftermine_sim::stats::REPAIR_COST_PERMILLE
            / u64::from(stats.max_hp)
    };
    u32::try_from(millis(to).div_ceil(1000) - millis(from).div_ceil(1000)).unwrap()
}

fn building_at(state: &State, kind: BuildingKind, anchor: TilePos, player: PlayerId) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|building| {
            building.kind == kind && building.anchor == anchor && building.player == player
        })
        .expect("expected authored building")
        .id
}

fn forge_buildings(
    state: State,
    edits: &[(BuildingId, u32, bool, u8)],
    tick: Option<u64>,
) -> State {
    let mut json = serde_json::to_value(state).unwrap();
    if let Some(tick) = tick {
        json["tick"] = serde_json::json!(tick);
    }
    for (id, hp, built, tier) in edits {
        let building = json["buildings"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|building| building["id"] == serde_json::json!(id))
            .expect("building id survives serialization");
        building["hp"] = serde_json::json!(hp);
        building["tier"] = serde_json::json!(tier);
        building["phase"] = if *built {
            serde_json::json!({"phase": "built"})
        } else if *tier > 0 {
            serde_json::json!({"phase": "upgrading", "progress": 1})
        } else {
            serde_json::json!({"phase": "site", "progress": 1})
        };
    }
    serde_json::from_value(json).unwrap()
}

fn structure_arena(scrap: u32, mut buildings: Vec<BuildingSpec>) -> State {
    let mut scenario = arena(vec![unit(0, UnitKind::Harvester, 2, 7)], scrap, true);
    scenario.buildings.append(&mut buildings);
    scenario.build().unwrap()
}

fn wounded_salvage_patient(harvester: TilePos) -> (State, UnitId, BuildingId, u32) {
    let mut scenario = arena(
        vec![unit(0, UnitKind::Harvester, harvester.x, harvester.y)],
        50,
        true,
    );
    scenario
        .buildings
        .push(structure(0, BuildingKind::Turret, 9, 4));
    let state = scenario.build().unwrap();
    let worker = state.units()[0].id;
    let patient = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 4),
        PlayerId(0),
    );
    let max = BuildingKind::Turret.base_stats().max_hp;
    let hurt = (1..max)
        .rev()
        .find(|hp| building_aura_bill(BuildingKind::Turret, 0, *hp, *hp + 1) == 1)
        .expect("some Turret hp step costs one scrap");
    (
        forge_buildings(state, &[(patient, hurt, true, 0)], None),
        worker,
        patient,
        hurt,
    )
}

fn wounded_ring_patient(kind: UnitKind, hp: u32, scrap: u32, overlap: bool) -> State {
    let pos = if overlap {
        TilePos::new(BAY_ANCHOR.0 + 2, BAY_ANCHOR.1 + 3)
    } else {
        RING
    };
    let mut scenario = arena(
        vec![
            unit(0, kind, pos.x, pos.y),
            // Make the captured package worker-sized. Artillery
            // cannot escort a replacement Harvester, so the wounded Bombard
            // alone must not count as the recovery screen.
            unit(0, UnitKind::Sentinel, 10, 9),
        ],
        scrap,
        true,
    );
    if overlap {
        scenario.buildings.push(common::building(
            0,
            BuildingKind::RepairBay,
            BAY_ANCHOR.0 + 3,
            BAY_ANCHOR.1,
        ));
    }
    let mut json = serde_json::to_value(scenario.build().unwrap()).unwrap();
    json["units"][0]["hp"] = serde_json::json!(hp);
    serde_json::from_value(json).unwrap()
}

#[test]
fn the_aura_heals_the_ring_to_whole_and_bills_the_welders_exact_price() {
    let units = vec![
        unit(0, UnitKind::Harvester, FAR.x, FAR.y), // patient
        unit(1, UnitKind::Scuttler, 17, 10),        // raider
    ];
    let mut state = arena(units, 500, true).build().unwrap();
    let (patient, raider) = (state.units()[0].id, state.units()[1].id);
    let hurt = wound(&mut state, patient, raider, 30);
    let bank = state.player(PlayerId(0)).scrap;
    state.tick(&[walk(0, vec![patient], RING)]);
    run_until(&mut state, 4_000, |s, _| {
        s.unit(patient).unwrap().hp == UnitKind::Harvester.stats().max_hp
    });
    let max = UnitKind::Harvester.stats().max_hp;
    let billed = bank - state.player(PlayerId(0)).scrap;
    assert_eq!(
        billed,
        aura_bill(UnitKind::Harvester, hurt, max),
        "a full aura heal must telescope to the meter's exact price"
    );
    // The dearest-verb doctrine holds in the ring: sustaining hp costs
    // strictly more per hp than salvage's 800 permille refunds, so no
    // heal-then-liquidate loop can profit.
    let healed = max - hurt;
    let salvage_value = u32::try_from(
        u64::from(healed)
            * u64::from(UnitKind::Harvester.stats().cost)
            * aftermine_sim::stats::SALVAGE_REFUND_PERMILLE
            / (1000 * u64::from(max)),
    )
    .unwrap();
    assert!(
        billed > salvage_value,
        "aura sustain ({billed}) must out-price a salvage-permille valuation ({salvage_value})"
    );
    // Full means done: the bank holds still once nothing is wounded.
    let settled = state.player(PlayerId(0)).scrap;
    for _ in 0..64 {
        state.tick(&[]);
    }
    assert_eq!(
        state.player(PlayerId(0)).scrap,
        settled,
        "a whole ring bills nothing"
    );
}

#[test]
fn overlapping_bays_heal_a_shared_patient_once_and_bill_it_exactly() {
    // Two bays whose auras both cover the patient's parking spot: only the
    // first-built takes it, so it heals one step per pulse and the bill
    // telescopes as one meter.
    let mut scenario = arena(
        vec![
            unit(0, UnitKind::Harvester, FAR.x, FAR.y),
            unit(1, UnitKind::Scuttler, 17, 10),
        ],
        500,
        true,
    );
    scenario.buildings.push(common::building(
        0,
        BuildingKind::RepairBay,
        BAY_ANCHOR.0 + 3,
        BAY_ANCHOR.1,
    ));
    let mut state = scenario.build().unwrap();
    assert_eq!(
        state
            .buildings()
            .iter()
            .filter(|b| b.kind == BuildingKind::RepairBay && b.built())
            .count(),
        2,
        "the second bay must actually stand"
    );
    let (patient, raider) = (state.units()[0].id, state.units()[1].id);
    let hurt = wound(&mut state, patient, raider, 20);
    let bank = state.player(PlayerId(0)).scrap;
    // Park between the two footprints, inside both auras.
    let overlap = TilePos::new(BAY_ANCHOR.0 + 2, BAY_ANCHOR.1 + 3);
    state.tick(&[walk(0, vec![patient], overlap)]);
    // The march grazes single-coverage fringe (auras heal walkers
    // too), so anchor the measurement to ARRIVAL, then to a pulse
    // boundary.
    run_until(&mut state, 2_000, |s, _| {
        s.unit(patient).unwrap().tile() == overlap
            && s.unit(patient).unwrap().order == aftermine_sim::Order::Idle
    });
    run_until(&mut state, 32, |s, _| {
        s.current_tick()
            .is_multiple_of(aftermine_sim::stats::REPAIR_BAY_PERIOD)
    });
    // One full pulse window inside both auras heals one Bay's step.
    let h0 = state.unit(patient).unwrap().hp;
    let max = UnitKind::Harvester.stats().max_hp;
    if h0 + 2 * aftermine_sim::stats::REPAIR_BAY_STEP <= max {
        for _ in 0..aftermine_sim::stats::REPAIR_BAY_PERIOD {
            state.tick(&[]);
        }
        assert_eq!(
            state.unit(patient).unwrap().hp,
            h0 + aftermine_sim::stats::REPAIR_BAY_STEP,
            "the second aura must not stack on a taken patient"
        );
    }
    run_until(&mut state, 4_000, |s, _| s.unit(patient).unwrap().hp == max);
    let billed = bank - state.player(PlayerId(0)).scrap;
    assert_eq!(
        billed,
        aura_bill(UnitKind::Harvester, hurt, max),
        "overlapping auras must bill the meter exactly once end to end"
    );
}

#[test]
fn a_broke_owner_gets_no_healing() {
    let units = vec![
        unit(0, UnitKind::Harvester, FAR.x, FAR.y),
        unit(1, UnitKind::Scuttler, 17, 10),
    ];
    let mut state = arena(units, 0, true).build().unwrap();
    let (patient, raider) = (state.units()[0].id, state.units()[1].id);
    // The scuttler's 3-damage bites land the harvester on 18 hp, where
    // the next step's ceiling-diff is a whole scrap — no free fractional
    // step for a broke bank to ride.
    let hurt = wound(&mut state, patient, raider, 20);
    assert_eq!(aura_bill(UnitKind::Harvester, hurt, hurt + 1), 1);
    state.tick(&[walk(0, vec![patient], RING)]);
    run_until(&mut state, 800, |s, _| {
        s.unit(patient).unwrap().tile() == RING
    });
    for _ in 0..200 {
        state.tick(&[]);
    }
    assert_eq!(
        state.unit(patient).unwrap().hp,
        hurt,
        "no scrap, no welding"
    );
    assert_eq!(state.player(PlayerId(0)).scrap, 0);
}

#[test]
fn the_aura_cannot_spend_the_emergency_recovery_reserve() {
    // An anti-air crawler cannot escort a ground worker. Put one wounded
    // in the ring through the validated state boundary: on tick zero the
    // Foundry supplies the final package scrap before the aura runs, and
    // the automatic repair must leave that coin and the patient alone.
    let scenario = arena(
        vec![unit(0, UnitKind::Flakhound, RING.x, RING.y)],
        FOUNDRY_RECOVERY_RESERVE - 1,
        true,
    );
    let state = scenario.build().unwrap();
    let max = UnitKind::Flakhound.stats().max_hp;
    let hurt = (1..max)
        .find(|hp| aura_bill(UnitKind::Flakhound, *hp, *hp + 1) == 1)
        .expect("some Flakhound hp step costs one scrap");
    let mut json = serde_json::to_value(state).unwrap();
    json["units"][0]["hp"] = serde_json::json!(hurt);
    let mut state: State = serde_json::from_value(json).unwrap();

    state.tick(&[]);
    assert_eq!(
        state.player(PlayerId(0)).scrap,
        FOUNDRY_RECOVERY_RESERVE,
        "the final recovery scrap must survive the same tick's aura"
    );
    assert_eq!(
        state.units()[0].hp,
        hurt,
        "paid sustain pauses while the economy is stranded"
    );
}

#[test]
fn multi_scrap_pulses_preserve_the_captured_worker_reserve() {
    let reserve = UnitKind::Harvester.stats().cost;
    // Bombard hp steps cost either two or three scrap. One scrap less
    // than the required surplus must block the whole pulse; exactly the
    // required surplus may be spent down to the intact reserve.
    for (hp, due) in [(1, 2), (8, 3)] {
        assert_eq!(aura_bill(UnitKind::Bombard, hp, hp + 1), due);

        let mut blocked = wounded_ring_patient(UnitKind::Bombard, hp, reserve + due - 1, false);
        blocked.tick(&[]);
        assert_eq!(blocked.units()[0].hp, hp);
        assert_eq!(
            blocked.player(PlayerId(0)).scrap,
            reserve + due - 1,
            "a {due}-scrap pulse must not borrow from the reserve"
        );

        let mut funded = wounded_ring_patient(UnitKind::Bombard, hp, reserve + due, false);
        funded.tick(&[]);
        assert_eq!(funded.units()[0].hp, hp + 1);
        assert_eq!(
            funded.player(PlayerId(0)).scrap,
            reserve,
            "the exact surplus remains spendable"
        );
    }
}

#[test]
fn overlapping_bays_recheck_the_reserve_after_each_charge() {
    let reserve = UnitKind::Harvester.stats().cost;
    let hp = 1;
    assert_eq!(aura_bill(UnitKind::Bombard, hp, hp + 1), 2);
    assert_eq!(aura_bill(UnitKind::Bombard, hp + 1, hp + 2), 2);

    // The first bay may spend two of the three surplus scrap. The second
    // must price from that post-charge bank and leave the remaining surplus
    // untouched instead of taking it below the replacement price.
    let mut state = wounded_ring_patient(UnitKind::Bombard, hp, reserve + 3, true);
    state.tick(&[]);
    assert_eq!(state.units()[0].hp, hp + 1);
    assert_eq!(state.player(PlayerId(0)).scrap, reserve + 1);
}

#[test]
fn partial_scrap_heals_the_most_wounded_first_then_starves() {
    let units = vec![
        unit(0, UnitKind::Harvester, FAR.x, FAR.y), // lighter wound
        unit(0, UnitKind::Harvester, FAR.x, FAR.y + 2), // heavier wound
        // Parked beyond its own aggro reach of both patients' walk to the ring,
        // so the raider never picks the chase back up.
        unit(1, UnitKind::Scuttler, 18, 10),
    ];
    let state = arena(units, 5, true).build().unwrap();
    let (a, b, raider) = (
        state.units()[0].id,
        state.units()[1].id,
        state.units()[2].id,
    );
    // Only the Bay heals here: the wounded workers do not weld each other.
    let mut state = common::idle_welds_off(&state, &[a, b]);
    let hurt_a = wound(&mut state, a, raider, 40);
    let hurt_b = {
        state.tick(&[walk(1, vec![raider], TilePos::new(FAR.x, FAR.y + 4))]);
        run_until(&mut state, 3_000, |s, _| s.unit(b).unwrap().hp <= 15);
        state.tick(&[walk(1, vec![raider], TilePos::new(18, 10))]);
        run_until(&mut state, 800, |s, _| {
            s.unit(raider).unwrap().tile() == TilePos::new(18, 10)
        });
        state.unit(b).unwrap().hp
    };
    state.tick(&[walk(0, vec![a], RING)]);
    state.tick(&[walk(0, vec![b], TilePos::new(RING.x, RING.y + 1))]);
    run_until(&mut state, 400, |s, _| patients(s, bay_id(s)).len() == 2);
    let order: Vec<UnitId> = patients(&state, bay_id(&state))
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(order, vec![b, a], "the worse wound comes first");
    // Five scrap buys a handful of pulses, then the bank is dry.
    run_until(&mut state, 2_000, |s, _| s.player(PlayerId(0)).scrap == 0);
    for _ in 0..200 {
        state.tick(&[]);
    }
    let (hp_a, hp_b) = (state.unit(a).unwrap().hp, state.unit(b).unwrap().hp);
    let max = UnitKind::Harvester.stats().max_hp;
    assert!(hurt_b < hurt_a, "premise: the later id is the worse wound");
    assert!(hp_a > hurt_a && hp_b > hurt_b, "both heal while coins last");
    assert!(hp_a < max && hp_b < max, "five scrap fully heals nobody");
}

#[test]
fn the_ring_ends_where_the_radius_says() {
    let units = vec![
        unit(0, UnitKind::Harvester, FAR.x, FAR.y),
        unit(1, UnitKind::Scuttler, 17, 10),
    ];
    let mut state = arena(units, 500, true).build().unwrap();
    let (patient, raider) = (state.units()[0].id, state.units()[1].id);
    let hurt = wound(&mut state, patient, raider, 20);
    // 4.5 tiles off the bay's east face: outside the 4.0 ring, starved.
    let outside = TilePos::new(BAY_ANCHOR.0 + 6, RING.y);
    state.tick(&[walk(0, vec![patient], outside)]);
    run_until(&mut state, 800, |s, _| {
        s.unit(patient).unwrap().tile() == outside
    });
    for _ in 0..200 {
        state.tick(&[]);
    }
    assert_eq!(
        state.unit(patient).unwrap().hp,
        hurt,
        "4.5 tiles out is outside the ring"
    );
    // One tile closer — 3.5 out — is inside, and the torch lights.
    let inside = TilePos::new(BAY_ANCHOR.0 + 5, RING.y);
    state.tick(&[walk(0, vec![patient], inside)]);
    run_until(&mut state, 800, |s, _| {
        s.unit(patient).unwrap().tile() == inside
    });
    run_until(&mut state, 200, |s, _| s.unit(patient).unwrap().hp > hurt);
}

#[test]
fn the_aura_serves_the_sky_too() {
    // The talon parks over the bay itself — ground occupancy means
    // nothing to a flyer, and the ring reads pure distance.
    let units = vec![
        unit(0, UnitKind::Talon, FAR.x, FAR.y),
        unit(1, UnitKind::Flakhound, 17, 10),
    ];
    let mut state = arena(units, 500, true).build().unwrap();
    let (patient, raider) = (state.units()[0].id, state.units()[1].id);
    let hurt = wound(&mut state, patient, raider, 25);
    state.tick(&[walk(
        0,
        vec![patient],
        TilePos::new(BAY_ANCHOR.0, BAY_ANCHOR.1),
    )]);
    run_until(&mut state, 1_000, |s, _| s.unit(patient).unwrap().hp > hurt);
}

#[test]
fn the_ring_welds_own_machines_only() {
    let units = vec![
        unit(0, UnitKind::Scuttler, 17, 10),        // seat 0's wounder
        unit(1, UnitKind::Harvester, FAR.x, FAR.y), // seat 1's wounded
    ];
    let mut state = arena(units, 500, true).build().unwrap();
    let (wounder, foreign) = (state.units()[0].id, state.units()[1].id);
    // Gnaw seat 1's harvester with seat 0's scuttler, then send both
    // ways: the scuttler home, the foreign patient into the ring.
    state.tick(&[walk(0, vec![wounder], TilePos::new(FAR.x, FAR.y - 2))]);
    run_until(&mut state, 3_000, |s, _| s.unit(foreign).unwrap().hp <= 30);
    state.tick(&[walk(0, vec![wounder], TilePos::new(17, 10))]);
    run_until(&mut state, 800, |s, _| {
        s.unit(wounder).unwrap().tile() == TilePos::new(17, 10)
    });
    let hurt = state.unit(foreign).unwrap().hp;
    let bank = state.player(PlayerId(0)).scrap;
    state.tick(&[walk(1, vec![foreign], RING)]);
    run_until(&mut state, 800, |s, _| {
        s.unit(foreign).unwrap().tile() == RING
    });
    for _ in 0..200 {
        state.tick(&[]);
    }
    assert_eq!(
        state.unit(foreign).unwrap().hp,
        hurt,
        "a hostile machine in the ring is not a patient"
    );
    assert_eq!(
        state.player(PlayerId(0)).scrap,
        bank,
        "and nobody was billed for it"
    );
}

#[test]
fn an_unbuilt_bay_is_inert() {
    let units = vec![
        unit(0, UnitKind::Harvester, FAR.x, FAR.y), // patient
        unit(0, UnitKind::Harvester, 2, 6),         // founder
        unit(1, UnitKind::Scuttler, 17, 10),
    ];
    let state = arena(units, 500, false).build().unwrap();
    let (patient, founder, raider) = (
        state.units()[0].id,
        state.units()[1].id,
        state.units()[2].id,
    );
    // The idle founder must not weld the patient the scaffold fails to heal.
    let mut state = common::idle_welds_off(&state, &[patient, founder]);
    // Begin work before abandoning the bay so its inert scaffold remains.
    state.tick(&[cmd(
        0,
        Command::Build {
            units: vec![founder],
            kind: BuildingKind::RepairBay,
            anchor: TilePos::new(BAY_ANCHOR.0, BAY_ANCHOR.1),
            queue: false,
            defer: false,
        },
    )]);
    run_until(&mut state, 500, |s, _| {
        s.buildings().iter().any(|b| {
            b.kind == BuildingKind::RepairBay && b.construction_progress().unwrap_or(0) > 0
        })
    });
    state.tick(&[cmd(
        0,
        Command::Stop {
            units: vec![founder],
        },
    )]);
    assert!(
        state
            .buildings()
            .iter()
            .any(|b| b.kind == BuildingKind::RepairBay && !b.built()),
        "test premise: the site stands unbuilt"
    );
    let hurt = wound(&mut state, patient, raider, 20);
    let bank = state.player(PlayerId(0)).scrap;
    state.tick(&[walk(0, vec![patient], RING)]);
    run_until(&mut state, 800, |s, _| {
        s.unit(patient).unwrap().tile() == RING
    });
    for _ in 0..200 {
        state.tick(&[]);
    }
    assert_eq!(
        state.unit(patient).unwrap().hp,
        hurt,
        "scaffolding heals nothing"
    );
    assert_eq!(state.player(PlayerId(0)).scrap, bank);
}

#[test]
fn the_aura_repairs_a_completed_structure_and_reports_the_accepted_pulse() {
    let mut state = structure_arena(50, vec![structure(0, BuildingKind::Turret, 9, 4)]);
    let bay = building_at(
        &state,
        BuildingKind::RepairBay,
        TilePos::new(BAY_ANCHOR.0, BAY_ANCHOR.1),
        PlayerId(0),
    );
    let patient = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 4),
        PlayerId(0),
    );
    let max = BuildingKind::Turret.base_stats().max_hp;
    let hurt = (1..max)
        .find(|hp| building_aura_bill(BuildingKind::Turret, 0, *hp, *hp + 1) == 1)
        .expect("some Turret hp step costs one scrap");
    state = forge_buildings(state, &[(patient, hurt, true, 0)], None);

    let bank = state.player(PlayerId(0)).scrap;
    let report = state.tick(&[]);
    assert_eq!(state.building(patient).unwrap().hp, hurt + 1);
    assert_eq!(state.player(PlayerId(0)).scrap, bank - 1);
    assert!(report.events.iter().any(|event| {
        matches!(
            event,
            Event::BuildingRepaired {
                building,
                player: PlayerId(0),
                repair_bay,
                amount: 1,
            } if *building == patient && *repair_bay == bay
        )
    }));
}

#[test]
fn a_single_paid_pulse_repairs_a_unit_before_a_structure() {
    let mut state = structure_arena(1, vec![structure(0, BuildingKind::Turret, 9, 4)]);
    let unit_patient = state.units()[0].id;
    let building_patient = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 4),
        PlayerId(0),
    );
    let unit_max = UnitKind::Harvester.stats().max_hp;
    let unit_hurt = (1..unit_max)
        .find(|hp| aura_bill(UnitKind::Harvester, *hp, *hp + 1) == 1)
        .expect("some Harvester hp step costs one scrap");
    let building_max = BuildingKind::Turret.base_stats().max_hp;
    let building_hurt = (1..building_max)
        .find(|hp| building_aura_bill(BuildingKind::Turret, 0, *hp, *hp + 1) == 1)
        .expect("some Turret hp step costs one scrap");

    state = forge_buildings(state, &[(building_patient, building_hurt, true, 0)], None);
    let mut json = serde_json::to_value(state).unwrap();
    json["units"][0]["hp"] = serde_json::json!(unit_hurt);
    let mut state: State = serde_json::from_value(json).unwrap();

    let report = state.tick(&[]);
    assert_eq!(state.unit(unit_patient).unwrap().hp, unit_hurt + 1);
    assert_eq!(state.building(building_patient).unwrap().hp, building_hurt);
    assert_eq!(state.player(PlayerId(0)).scrap, 0);
    assert!(report.events.iter().any(|event| {
        matches!(
            event,
            Event::UnitRepaired {
                unit,
                source: aftermine_sim::UnitRepairSource::RepairBay { .. },
                amount: 1,
                ..
            } if *unit == unit_patient
        )
    }));
    assert!(!report.events.iter().any(|event| {
        matches!(event, Event::BuildingRepaired { building, .. } if *building == building_patient)
    }));
}

#[test]
fn structure_range_uses_footprint_edges_and_pulses_only_on_cadence() {
    let mut state = structure_arena(
        500,
        vec![
            structure(0, BuildingKind::Turret, 10, 4),
            structure(0, BuildingKind::Turret, 11, 4),
        ],
    );
    let inside = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(10, 4),
        PlayerId(0),
    );
    let outside = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(11, 4),
        PlayerId(0),
    );
    let max = BuildingKind::Turret.base_stats().max_hp;
    let hurt = max - 20;
    state = forge_buildings(
        state,
        &[(inside, hurt, true, 0), (outside, hurt, true, 0)],
        Some(1),
    );

    // Starting ticks 1 through 7 are not pulse ticks.
    for _ in 0..7 {
        state.tick(&[]);
    }
    assert_eq!(state.current_tick(), 8);
    assert_eq!(state.building(inside).unwrap().hp, hurt);
    assert_eq!(state.building(outside).unwrap().hp, hurt);

    // The source Bay spans x=[4,6]. A Turret beginning at x=10 is exactly
    // four tiles from that footprint; x=11 is five tiles away.
    state.tick(&[]);
    assert_eq!(state.building(inside).unwrap().hp, hurt + 1);
    assert_eq!(state.building(outside).unwrap().hp, hurt);
}

#[test]
fn the_aura_ignores_its_source_foreign_structures_and_unfinished_sites() {
    let mut state = structure_arena(
        500,
        vec![
            structure(1, BuildingKind::Turret, 9, 4),
            structure(0, BuildingKind::Turret, 9, 5),
        ],
    );
    let bay = building_at(
        &state,
        BuildingKind::RepairBay,
        TilePos::new(BAY_ANCHOR.0, BAY_ANCHOR.1),
        PlayerId(0),
    );
    let foreign = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 4),
        PlayerId(1),
    );
    let unfinished = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 5),
        PlayerId(0),
    );
    let bay_hurt = BuildingKind::RepairBay.base_stats().max_hp - 20;
    let turret_hurt = BuildingKind::Turret.base_stats().max_hp - 20;
    let foundation_hp = BuildingKind::Turret.base_stats().max_hp / 5;
    state = forge_buildings(
        state,
        &[
            (bay, bay_hurt, true, 0),
            (foreign, turret_hurt, true, 0),
            (unfinished, foundation_hp, false, 0),
        ],
        None,
    );

    let report = state.tick(&[]);
    assert_eq!(state.building(bay).unwrap().hp, bay_hurt);
    assert_eq!(state.building(foreign).unwrap().hp, turret_hurt);
    assert!(state.building(unfinished).unwrap().hp <= foundation_hp);
    assert!(!report.events.iter().any(|event| {
        matches!(
            event,
            Event::BuildingRepaired { building, .. }
                if [bay, foreign, unfinished].contains(building)
        )
    }));
}

#[test]
fn the_aura_does_not_fight_an_active_salvage_job() {
    let (mut state, worker, patient, hurt) = wounded_salvage_patient(TilePos::new(8, 4));
    let bank = state.player(PlayerId(0)).scrap;
    let mut events = state
        .tick(&[cmd(
            0,
            Command::Salvage {
                units: vec![worker],
                building: patient,
                queue: false,
            },
        )])
        .events;

    assert!(matches!(
        state.unit(worker).unwrap().order,
        aftermine_sim::Order::Salvage { building } if building == patient
    ));
    assert!(state.building(patient).unwrap().hp <= hurt);
    assert!(
        state.player(PlayerId(0)).scrap >= bank,
        "the automatic aura must not charge while teardown owns the target"
    );

    for _ in 0..64 {
        if state.building(patient).unwrap().hp < hurt {
            break;
        }
        events.extend(state.tick(&[]).events);
    }
    assert!(
        state.building(patient).unwrap().hp < hurt,
        "salvage must make visible progress instead of being offset by the aura"
    );
    assert!(!events.iter().any(|event| {
        matches!(event, Event::BuildingRepaired { building, .. } if *building == patient)
    }));
}

#[test]
fn the_aura_respects_a_queued_salvage_commitment() {
    let (mut state, worker, patient, hurt) = wounded_salvage_patient(TilePos::new(2, 7));
    let bank = state.player(PlayerId(0)).scrap;
    let report = state.tick(&[
        walk(0, vec![worker], TilePos::new(2, 9)),
        cmd(
            0,
            Command::Salvage {
                units: vec![worker],
                building: patient,
                queue: true,
            },
        ),
    ]);

    assert!(matches!(
        state.unit(worker).unwrap().order,
        aftermine_sim::Order::Run { .. }
    ));
    assert!(state.unit(worker).unwrap().queue.iter().any(
        |order| matches!(order, aftermine_sim::Order::Salvage { building } if *building == patient)
    ));
    assert_eq!(state.building(patient).unwrap().hp, hurt);
    assert_eq!(state.player(PlayerId(0)).scrap, bank);
    assert!(!report.events.iter().any(|event| {
        matches!(event, Event::BuildingRepaired { building, .. } if *building == patient)
    }));
}

#[test]
fn overlapping_bays_can_repair_each_other_but_never_themselves() {
    let mut state = structure_arena(500, vec![structure(0, BuildingKind::RepairBay, 7, 4)]);
    let patient = building_at(
        &state,
        BuildingKind::RepairBay,
        TilePos::new(BAY_ANCHOR.0, BAY_ANCHOR.1),
        PlayerId(0),
    );
    let helper = building_at(
        &state,
        BuildingKind::RepairBay,
        TilePos::new(7, 4),
        PlayerId(0),
    );
    let hurt = BuildingKind::RepairBay.base_stats().max_hp - 10;
    state = forge_buildings(state, &[(patient, hurt, true, 0)], None);

    let report = state.tick(&[]);
    assert_eq!(state.building(patient).unwrap().hp, hurt + 1);
    assert!(report.events.iter().any(|event| {
        matches!(
            event,
            Event::BuildingRepaired {
                building,
                repair_bay,
                amount: 1,
                ..
            } if *building == patient && *repair_bay == helper
        )
    }));
}

#[test]
fn active_tier_sets_structure_billing_and_a_broke_bank_skips_the_pulse() {
    let tier = 2;
    let stats = BuildingKind::Turret.tier_stats(tier);
    let hurt = (1..stats.max_hp)
        .find(|hp| building_aura_bill(BuildingKind::Turret, tier, *hp, *hp + 1) == 1)
        .expect("some Bulwark hp step costs one scrap");

    for (scrap, expected_hp) in [(0, hurt), (1, hurt + 1)] {
        let mut state = structure_arena(scrap, vec![structure(0, BuildingKind::Turret, 9, 4)]);
        let patient = building_at(
            &state,
            BuildingKind::Turret,
            TilePos::new(9, 4),
            PlayerId(0),
        );
        state = forge_buildings(state, &[(patient, hurt, true, tier)], None);
        state.tick(&[]);
        assert_eq!(state.building(patient).unwrap().hp, expected_hp);
        assert_eq!(state.player(PlayerId(0)).scrap, 0);
    }
}

#[test]
fn limited_scrap_repairs_structures_in_building_id_order() {
    let mut state = structure_arena(
        1,
        vec![
            structure(0, BuildingKind::Turret, 9, 4),
            structure(0, BuildingKind::Turret, 9, 5),
        ],
    );
    let first = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 4),
        PlayerId(0),
    );
    let second = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 5),
        PlayerId(0),
    );
    assert!(first < second, "authored order supplies the id order");
    let max = BuildingKind::Turret.base_stats().max_hp;
    let hurt = (1..max)
        .find(|hp| building_aura_bill(BuildingKind::Turret, 0, *hp, *hp + 1) == 1)
        .unwrap();
    state = forge_buildings(
        state,
        &[(first, hurt, true, 0), (second, hurt, true, 0)],
        None,
    );

    state.tick(&[]);
    assert_eq!(state.building(first).unwrap().hp, hurt + 1);
    assert_eq!(state.building(second).unwrap().hp, hurt);
    assert_eq!(state.player(PlayerId(0)).scrap, 0);
}

#[test]
fn lethal_fire_wins_a_structure_repair_pulse_and_forfeits_its_coin() {
    let mut scenario = arena(
        vec![
            unit(0, UnitKind::Harvester, 2, 7),
            unit(1, UnitKind::Sentinel, 11, 4),
        ],
        50,
        true,
    );
    scenario
        .buildings
        .push(structure(0, BuildingKind::Turret, 9, 4));
    let mut state = scenario.build().unwrap();
    let patient = building_at(
        &state,
        BuildingKind::Turret,
        TilePos::new(9, 4),
        PlayerId(0),
    );
    let attacker = state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(1))
        .unwrap()
        .id;
    let damage = UnitKind::Sentinel.stats().weapons[0].damage;
    let hurt = (1..=damage)
        .find(|hp| building_aura_bill(BuildingKind::Turret, 0, *hp, *hp + 1) == 1)
        .expect("a lethal-range Turret hp step costs one scrap");
    state = forge_buildings(state, &[(patient, hurt, true, 0)], None);
    let bank = state.player(PlayerId(0)).scrap;

    common::face_target(&mut state, attacker, Target::Building(patient));
    let report = state.tick(&[cmd(
        1,
        Command::Attack {
            units: vec![attacker],
            target: Target::Building(patient).into(),
            queue: false,
        },
    )]);
    assert!(
        state.building(patient).is_none(),
        "fire destroys the target"
    );
    assert_eq!(
        state.player(PlayerId(0)).scrap,
        bank - 1,
        "a lethal same-tick volley forfeits the prepaid repair coin"
    );
    assert!(!report.events.iter().any(|event| {
        matches!(event, Event::BuildingRepaired { building, .. } if *building == patient)
    }));
}

#[test]
fn fire_wins_the_tick_and_the_dead_forfeit_the_aura() {
    let units = vec![
        unit(0, UnitKind::Harvester, FAR.x, FAR.y),
        unit(1, UnitKind::Scuttler, 17, 10),
        unit(1, UnitKind::Scuttler, 15, 10),
    ];
    let mut state = arena(units, 500, true).build().unwrap();
    let (patient, r1, r2) = (
        state.units()[0].id,
        state.units()[1].id,
        state.units()[2].id,
    );
    let hurt = wound(&mut state, patient, r1, 30);
    state.tick(&[walk(0, vec![patient], RING)]);
    run_until(&mut state, 400, |s, _| s.unit(patient).unwrap().hp > hurt);
    // Two gnawing scuttlers (20 hp/s) walk into the ring and out-pace
    // the aura's slow rate under fire: buffered heals land only on machines the
    // volley left standing, so the ring never outbids the guns on the
    // tick they win.
    for raider in [r1, r2] {
        state.tick(&[walk(1, vec![raider], TilePos::new(RING.x + 1, RING.y))]);
    }
    run_until(&mut state, 2_000, |_, events| {
        events
            .iter()
            .any(|e| matches!(e, Event::UnitDied { unit, .. } if *unit == patient))
    });
    assert!(state.unit(patient).is_none(), "nothing resurrects");
    // A corpse leaves the ring's books: the bank holds still once the
    // raiders are the only machines near the bay.
    let bank = state.player(PlayerId(0)).scrap;
    for _ in 0..100 {
        state.tick(&[]);
    }
    assert_eq!(
        state.player(PlayerId(0)).scrap,
        bank,
        "the aura bills no dead patient"
    );
}

/// `state` with each `(unit, hp, damaged_at)` forged in.
fn forge_units(state: State, edits: &[(UnitId, u32, Option<u64>)]) -> State {
    let mut json = serde_json::to_value(state).unwrap();
    for (id, hp, damaged_at) in edits {
        let unit = json["units"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|unit| unit["id"] == serde_json::json!(id))
            .expect("unit id survives serialization");
        unit["hp"] = serde_json::json!(hp);
        if let Some(at) = damaged_at {
            unit["damaged_at"] = serde_json::json!(at);
        }
    }
    serde_json::from_value(json).unwrap()
}

/// The machines `bay` services this pulse.
fn patients(state: &State, bay: BuildingId) -> Vec<(UnitId, aftermine_sim::state::RepairPace)> {
    state
        .repair_bay_patients()
        .into_iter()
        .find(|(id, _)| *id == bay)
        .map(|(_, patients)| patients)
        .unwrap_or_default()
}

fn bay_id(state: &State) -> BuildingId {
    building_at(
        state,
        BuildingKind::RepairBay,
        TilePos::new(BAY_ANCHOR.0, BAY_ANCHOR.1),
        PlayerId(0),
    )
}

/// Ticks until the next aura pulse fires, plus that pulse.
fn to_next_pulse(state: &mut State) {
    state.tick(&[]);
    while !state
        .current_tick()
        .is_multiple_of(aftermine_sim::stats::REPAIR_BAY_PERIOD)
    {
        state.tick(&[]);
    }
    state.tick(&[]);
}

#[test]
fn a_bay_services_its_four_most_wounded_and_the_fifth_waits() {
    // Five Sentinels around the bay, each missing a different share.
    let spots = [(7, 4), (7, 5), (4, 7), (5, 7), (2, 4)];
    let units = spots
        .iter()
        .map(|&(x, y)| unit(0, UnitKind::Sentinel, x, y))
        .chain([unit(0, UnitKind::Harvester, 2, 9)])
        .collect();
    let state = arena(units, 500, true).build().unwrap();
    let ids: Vec<UnitId> = state.units()[..5].iter().map(|unit| unit.id).collect();
    // The first spawned is the least wounded, so id order alone would pick it.
    let hps = [50, 10, 20, 30, 40];
    let edits: Vec<_> = ids
        .iter()
        .zip(hps)
        .map(|(&id, hp)| (id, hp, None))
        .collect();
    let mut state = forge_units(state, &edits);
    let chosen: Vec<UnitId> = patients(&state, bay_id(&state))
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        chosen,
        vec![ids[1], ids[2], ids[3], ids[4]],
        "most missing first, four at most"
    );
    to_next_pulse(&mut state);
    let hp = |id| state.unit(id).unwrap().hp;
    assert_eq!(hp(ids[0]), 50, "the least wounded waits for a slot");
    for (&id, before) in ids[1..].iter().zip(&hps[1..]) {
        assert!(
            hp(id) > *before,
            "a calm patient this light still gains the slow step at least"
        );
    }
}

#[test]
fn a_calm_breaker_heals_fast_and_one_just_hit_waits_out_the_slow_rate() {
    let units = vec![
        unit(0, UnitKind::Breaker, RING.x, RING.y),
        unit(0, UnitKind::Harvester, 2, 9),
    ];
    let mut state = arena(units, 5_000, true).build().unwrap();
    state.tick(&[]);
    let breaker = state.units()[0].id;
    let max = UnitKind::Breaker.stats().max_hp;
    let half = max / 2;
    // Hit on the tick just run.
    let hit_at = state.current_tick() - 1;
    let mut state = forge_units(state, &[(breaker, half, Some(hit_at))]);
    // Inside the calm window: the slow step.
    to_next_pulse(&mut state);
    assert_eq!(
        patients(&state, bay_id(&state)),
        vec![(breaker, aftermine_sim::state::RepairPace::Slow)]
    );
    let slow = state.unit(breaker).unwrap().hp - half;
    assert_eq!(slow, aftermine_sim::stats::REPAIR_BAY_STEP);
    // Half a Breaker back in about 20 s, the calm wait included.
    let deadline = hit_at + 20 * u64::from(aftermine_sim::TICKS_PER_SECOND);
    run_until(&mut state, 600, |s, _| s.unit(breaker).unwrap().hp == max);
    assert!(
        state.current_tick() <= deadline,
        "whole at tick {} against a deadline of {deadline}",
        state.current_tick()
    );
    assert!(
        state.current_tick() >= hit_at + aftermine_sim::stats::REPAIR_BAY_CALM_TICKS,
        "the calm wait came first"
    );
}

#[test]
fn a_patient_the_bank_cannot_fully_pay_for_heals_what_it_can() {
    let units = vec![
        unit(0, UnitKind::Breaker, RING.x, RING.y),
        unit(0, UnitKind::Harvester, 2, 9),
    ];
    let state = arena(units, 3, true).build().unwrap();
    let breaker = state.units()[0].id;
    let half = UnitKind::Breaker.stats().max_hp / 2;
    let mut state = forge_units(state, &[(breaker, half, None)]);
    to_next_pulse(&mut state);
    let healed = state.unit(breaker).unwrap().hp - half;
    assert!(healed > 0, "three scrap buys part of the step");
    assert_eq!(
        aura_bill(UnitKind::Breaker, half, half + healed),
        3,
        "and spends exactly what the bank held"
    );
    assert_eq!(state.player(PlayerId(0)).scrap, 0);
}

#[test]
fn a_condor_heals_only_landed_and_a_talon_while_it_hovers() {
    let units = vec![
        unit(0, UnitKind::Condor, RING.x, RING.y),
        unit(0, UnitKind::Talon, RING.x, RING.y + 2),
        unit(0, UnitKind::Harvester, 2, 9),
    ];
    let state = arena(units, 500, true).build().unwrap();
    let (condor, talon) = (state.units()[0].id, state.units()[1].id);
    let state = forge_units(state, &[(condor, 100, None), (talon, 40, None)]);
    let chosen = |state: &State| -> Vec<UnitId> {
        patients(state, bay_id(state))
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    assert!(!state.unit(condor).unwrap().landed(), "premise: aloft");
    assert_eq!(
        chosen(&state),
        vec![talon],
        "a flying Condor is passed over"
    );
    let mut json = serde_json::to_value(&state).unwrap();
    json["units"][0]["motor"] = serde_json::json!({"motor": "landed"});
    let state: State = serde_json::from_value(json).unwrap();
    assert_eq!(
        chosen(&state),
        vec![condor, talon],
        "a landed one is served"
    );
}

#[test]
fn overlapping_bays_serve_different_patients() {
    let spots = [(6, 7), (7, 7), (8, 7), (9, 7), (7, 8)];
    let mut scenario = arena(
        spots
            .iter()
            .map(|&(x, y)| unit(0, UnitKind::Sentinel, x, y))
            .chain([unit(0, UnitKind::Harvester, 2, 9)])
            .collect(),
        500,
        true,
    );
    scenario
        .buildings
        .push(structure(0, BuildingKind::RepairBay, 9, 4));
    let state = scenario.build().unwrap();
    let ids: Vec<UnitId> = state.units()[..5].iter().map(|unit| unit.id).collect();
    let edits: Vec<_> = ids.iter().map(|&id| (id, 30, None)).collect();
    let state = forge_units(state, &edits);
    let second = building_at(
        &state,
        BuildingKind::RepairBay,
        TilePos::new(9, 4),
        PlayerId(0),
    );
    let first = patients(&state, bay_id(&state));
    let later = patients(&state, second);
    assert_eq!(first.len(), 4, "the first-built Bay fills its slots");
    assert_eq!(later.len(), 1, "the later Bay takes the one left over");
    assert!(
        first.iter().all(|patient| !later.contains(patient)),
        "no machine takes two slots"
    );
}

#[test]
fn mirrored_seats_choose_mirrored_patients() {
    // Both seats own a Bay and the same wounded crew, a half-turn apart.
    let map: Vec<String> = arena(vec![], 0, false).map;
    let (w, h) = (
        i32::try_from(map[0].len()).unwrap(),
        i32::try_from(map.len()).unwrap(),
    );
    let mirror = |x: i32, y: i32| (w - 1 - x, h - 1 - y);
    let spots = [(7, 4), (7, 5), (4, 7), (5, 7), (2, 4)];
    let mut units = Vec::new();
    for player in [0, 1] {
        for &(x, y) in &spots {
            let (x, y) = if player == 0 { (x, y) } else { mirror(x, y) };
            units.push(unit(player, UnitKind::Sentinel, x, y));
        }
    }
    let mut scenario = arena(units, 500, false);
    let (bx, by) = mirror(BAY_ANCHOR.0 + 1, BAY_ANCHOR.1 + 1);
    scenario.buildings = vec![
        structure(0, BuildingKind::RepairBay, BAY_ANCHOR.0, BAY_ANCHOR.1),
        structure(1, BuildingKind::RepairBay, bx, by),
    ];
    let state = scenario.build().unwrap();
    let hps = [30, 30, 20, 20, 40];
    let edits: Vec<_> = state
        .units()
        .iter()
        .enumerate()
        .map(|(i, unit)| (unit.id, hps[i % 5], None))
        .collect();
    let state = forge_units(state, &edits);
    let chosen = |player: u8, x, y| -> Vec<usize> {
        let bay = building_at(
            &state,
            BuildingKind::RepairBay,
            TilePos::new(x, y),
            PlayerId(player),
        );
        patients(&state, bay)
            .into_iter()
            .map(|(id, _)| {
                state
                    .units()
                    .iter()
                    .filter(|unit| unit.player == PlayerId(player))
                    .position(|unit| unit.id == id)
                    .unwrap()
            })
            .collect()
    };
    assert_eq!(
        chosen(0, BAY_ANCHOR.0, BAY_ANCHOR.1),
        chosen(1, bx, by),
        "each seat picks the same spots of its crew"
    );
}

#[test]
fn a_hit_stamps_its_victim_with_the_tick_it_landed() {
    let units = vec![
        unit(0, UnitKind::Sentinel, FAR.x, FAR.y),
        unit(1, UnitKind::Scuttler, FAR.x + 1, FAR.y),
    ];
    let mut state = arena(units, 0, false).build().unwrap();
    let victim = state.units()[0].id;
    assert_eq!(state.unit(victim).unwrap().damaged_at, None);
    let mut hit = None;
    for _ in 0..200 {
        let tick = state.current_tick();
        let report = state.tick(&[]);
        if report.events.iter().any(
            |event| matches!(event, Event::DamageTaken { player, .. } if *player == PlayerId(0)),
        ) {
            hit = Some(tick);
            break;
        }
    }
    let hit = hit.expect("premise: the Scuttler bites");
    assert_eq!(state.unit(victim).unwrap().damaged_at, Some(hit));
}
