//! Retirement at a Crucible: the refund and its pricing against repair, no
//! wreck or death, fire winning the completion tick, a lost Crucible, a
//! loaded transport, landing and hovering aircraft, heals that skip a
//! retiring machine, recovery, refusals, and mirrored retirements.

use crate::common;
use common::{cmd, run_until, unit};

use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::{BuildingSpec, ScenarioMode, UnitSpec};
use oxide_sim::stats::{
    BuildingKind, REPAIR_COST_PERMILLE, RETIRE_REFUND_PERMILLE, RETIRE_TICKS, retire_refund,
};
use oxide_sim::{
    BuildingId, Command, Event, Order, PlayerId, Recovery, Scenario, State, UnitId, UnitKind,
};
use serde_json::{Value, json};

const ROSTER: [UnitKind; 16] = [
    UnitKind::Harvester,
    UnitKind::Sentinel,
    UnitKind::Scuttler,
    UnitKind::Lancer,
    UnitKind::Bombard,
    UnitKind::Flakhound,
    UnitKind::Buzzard,
    UnitKind::Talon,
    UnitKind::Warden,
    UnitKind::Excavator,
    UnitKind::Kestrel,
    UnitKind::Condor,
    UnitKind::Breaker,
    UnitKind::Avalanche,
    UnitKind::Skyhook,
    UnitKind::Sapper,
];

fn crucible(player: u8, x: i32, y: i32) -> BuildingSpec {
    BuildingSpec {
        player,
        kind: BuildingKind::Crucible,
        x,
        y,
    }
}

/// An open sandbox field with no Foundry, so no income drip blurs a bank.
fn field(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>, scrap: u32) -> Scenario {
    let (width, height) = (40, 20);
    let map = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    if x == 0 || y == 0 || x == width - 1 || y == height - 1 {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    Scenario {
        mode: ScenarioMode::Sandbox,
        name: "retire-field".into(),
        map,
        players: common::players(scrap),
        units,
        buildings,
        meta: None,
    }
}

fn build(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>, scrap: u32) -> State {
    field(units, buildings, scrap).build().unwrap()
}

fn ids(state: &State) -> Vec<UnitId> {
    state.units().iter().map(|u| u.id).collect()
}

fn crucibles(state: &State) -> Vec<BuildingId> {
    state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Crucible)
        .map(|b| b.id)
        .collect()
}

fn retire(units: Vec<UnitId>) -> Command {
    Command::Retire {
        units,
        queue: false,
    }
}

fn rejected(events: &[Event]) -> Vec<RejectReason> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::CommandRejected { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
}

fn retired(events: &[Event], id: UnitId) -> Option<(u32, u32)> {
    events.iter().find_map(|event| match event {
        Event::UnitRetired {
            unit,
            refund,
            cargo,
            ..
        } if *unit == id => Some((*refund, *cargo)),
        _ => None,
    })
}

fn edit(state: &mut State, poke: impl FnOnce(&mut Value)) {
    let mut data = serde_json::to_value(&*state).unwrap();
    poke(&mut data);
    *state = serde_json::from_value(data).unwrap();
}

fn unit_row(data: &mut Value, id: UnitId) -> &mut Value {
    data["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == json!(id))
        .unwrap()
}

#[test]
fn the_refund_scales_with_hp_and_never_beats_repair() {
    const { assert!(RETIRE_REFUND_PERMILLE < REPAIR_COST_PERMILLE) };
    for kind in ROSTER {
        let stats = kind.stats();
        let full = retire_refund(kind, stats.max_hp);
        assert_eq!(
            u64::from(full),
            u64::from(stats.cost) * RETIRE_REFUND_PERMILLE / 1000,
            "{kind:?} at full hp"
        );
        for hp in 1..stats.max_hp {
            let welded = u64::from(stats.max_hp - hp);
            // The least any weld of `welded` hp bills: the meter only ever
            // rounds up.
            let repair = (welded * u64::from(stats.cost) * REPAIR_COST_PERMILLE)
                .div_ceil(u64::from(stats.max_hp) * 1000);
            let gain = u64::from(full - retire_refund(kind, hp));
            assert!(
                gain <= repair,
                "{kind:?}: welding from {hp} hp costs {repair} but retires for {gain} more"
            );
        }
    }
}

#[test]
fn a_retired_machine_banks_its_refund_and_leaves_no_wreck() {
    let mut state = build(
        vec![unit(0, UnitKind::Sentinel, 14, 10)],
        vec![crucible(0, 20, 9)],
        0,
    );
    let id = state.units()[0].id;
    edit(&mut state, |d| unit_row(d, id)["hp"] = json!(30));
    let report = state.tick(&[cmd(0, retire(vec![id]))]);
    assert!(rejected(&report.events).is_empty());
    let events = run_until(&mut state, 600, |_, events| retired(events, id).is_some());
    let (refund, cargo) = retired(&events, id).unwrap();
    assert_eq!(refund, retire_refund(UnitKind::Sentinel, 30));
    assert_eq!(cargo, 0);
    assert_eq!(state.player(PlayerId(0)).scrap, refund);
    assert!(state.unit(id).is_none());
    assert!(
        !events.iter().any(|e| matches!(e, Event::UnitDied { .. })),
        "no death"
    );
    let total_wreck: u32 = (0..20)
        .flat_map(|y| (0..40).map(move |x| TilePos::new(x, y)))
        .map(|tile| state.map().wreck_at(tile))
        .sum();
    assert_eq!(total_wreck, 0, "no wreck");
}

#[test]
fn fire_wins_the_completion_tick() {
    // One tick from done, at one hp, beside an enemy whose shot lands the
    // same tick: the machine dies, wrecks, and refunds nothing.
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 19, 10),
            unit(1, UnitKind::Sentinel, 17, 10),
        ],
        vec![crucible(0, 20, 9)],
        0,
    );
    let [id, enemy] = ids(&state)[..] else {
        unreachable!()
    };
    let forge = crucibles(&state)[0];
    state.tick(&[cmd(0, retire(vec![id]))]);
    run_until(&mut state, 100, |s, _| s.unit(id).unwrap().progress > 0);
    edit(&mut state, |d| {
        let row = unit_row(d, id);
        row["hp"] = json!(1);
        row["progress"] = json!(RETIRE_TICKS - 1);
        let shooter = unit_row(d, enemy);
        shooter["cooldowns"] = json!([0, 0]);
        shooter["order"] =
            json!({"order": "attack", "target": {"kind": "unit", "id": id}, "pursue": true});
    });
    assert_eq!(
        state.unit(id).unwrap().order,
        Order::Retire {
            crucible: forge,
            pad: None
        }
    );
    let report = state.tick(&[]);
    assert!(
        report
            .events
            .iter()
            .any(|e| matches!(e, Event::UnitDied { unit, .. } if *unit == id)),
        "premise: the shot landed: {:?}",
        report.events
    );
    assert!(retired(&report.events, id).is_none());
    assert_eq!(state.player(PlayerId(0)).scrap, 0);
}

#[test]
fn a_lost_crucible_sends_the_machine_to_the_next_or_ends_the_order() {
    let mut state = build(
        vec![unit(0, UnitKind::Sentinel, 10, 10)],
        vec![crucible(0, 14, 9), crucible(0, 30, 9)],
        0,
    );
    let id = state.units()[0].id;
    let [near, far] = crucibles(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, retire(vec![id]))]);
    assert_eq!(
        state.unit(id).unwrap().order,
        Order::Retire {
            crucible: near,
            pad: None
        }
    );
    edit(&mut state, |d| {
        d["buildings"]
            .as_array_mut()
            .unwrap()
            .retain(|b| b["id"] != json!(near));
    });
    state.tick(&[]);
    assert_eq!(
        state.unit(id).unwrap().order,
        Order::Retire {
            crucible: far,
            pad: None
        },
        "the nearest standing Crucible takes over"
    );
    edit(&mut state, |d| {
        d["buildings"]
            .as_array_mut()
            .unwrap()
            .retain(|b| b["id"] != json!(far));
    });
    let report = state.tick(&[]);
    assert_eq!(state.unit(id).unwrap().order, Order::Idle);
    assert!(
        report
            .events
            .iter()
            .any(|e| matches!(e, Event::OrderStalled { unit, .. } if *unit == id)),
        "with none left, the order ends and says so"
    );
}

#[test]
fn a_loaded_transport_sets_its_riders_down_then_retires() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Skyhook, 12, 10),
            unit(0, UnitKind::Sentinel, 11, 10),
            unit(0, UnitKind::Sentinel, 12, 12),
        ],
        vec![crucible(0, 20, 9)],
        0,
    );
    let [sling, rider, latecomer] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(
        0,
        Command::Load {
            units: vec![rider],
            transport: sling,
            queue: false,
        },
    )]);
    run_until(&mut state, 200, |s, _| s.unit(rider).is_none());
    // A rider already on its way when the sling retires is turned away.
    state.tick(&[cmd(
        0,
        Command::Load {
            units: vec![latecomer],
            transport: sling,
            queue: false,
        },
    )]);
    state.tick(&[cmd(0, retire(vec![sling]))]);
    let report = state.tick(&[cmd(
        0,
        Command::Load {
            units: vec![latecomer],
            transport: sling,
            queue: false,
        },
    )]);
    assert_eq!(rejected(&report.events), [RejectReason::InvalidTarget]);
    let events = run_until(&mut state, 600, |_, events| {
        retired(events, sling).is_some()
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::UnitUnloaded { unit, .. } if *unit == rider)),
        "the rider was set down first"
    );
    let set_down = state.unit(rider).expect("the rider is back in the world");
    assert_eq!(set_down.order, Order::Idle);
    let crucible = state.building(crucibles(&state)[0]).unwrap();
    assert!(
        set_down
            .tile()
            .chebyshev(TilePos::containing(crucible.center()))
            <= 4,
        "beside the Crucible"
    );
    assert_eq!(
        state.unit(latecomer).unwrap().order,
        Order::Idle,
        "the boarder stood down"
    );
}

#[test]
fn a_condor_lands_beside_the_crucible_and_hover_craft_hold_over_it() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Condor, 6, 4),
            unit(0, UnitKind::Talon, 6, 16),
        ],
        vec![crucible(0, 24, 9)],
        0,
    );
    let [bomber, fighter] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, retire(vec![bomber, fighter]))]);
    let (mut landed, mut fighter_landed) = (false, false);
    let mut done = std::collections::BTreeSet::new();
    for _ in 0..1_500 {
        let report = state.tick(&[]);
        landed |= state.unit(bomber).is_some_and(oxide_sim::Unit::landed);
        fighter_landed |= state.unit(fighter).is_some_and(oxide_sim::Unit::landed);
        for id in [bomber, fighter] {
            if retired(&report.events, id).is_some() {
                done.insert(id);
            }
        }
        if done.len() == 2 {
            break;
        }
    }
    assert_eq!(done.len(), 2, "both were taken apart");
    assert!(landed, "the Condor set down first");
    assert!(!fighter_landed, "the Talon never lands");
}

#[test]
fn no_repair_source_mends_a_retiring_machine() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 18, 10),
            unit(0, UnitKind::Harvester, 17, 12),
        ],
        vec![
            crucible(0, 20, 9),
            BuildingSpec {
                player: 0,
                kind: BuildingKind::RepairBay,
                x: 16,
                y: 7,
            },
        ],
        500,
    );
    let [patient, welder] = ids(&state)[..] else {
        unreachable!()
    };
    edit(&mut state, |d| unit_row(d, patient)["hp"] = json!(20));
    state.tick(&[cmd(0, retire(vec![patient]))]);
    let report = state.tick(&[cmd(
        0,
        Command::RepairUnit {
            units: vec![welder],
            target: patient,
            queue: false,
        },
    )]);
    assert_eq!(rejected(&report.events), [RejectReason::InvalidTarget]);
    let bank = state.player(PlayerId(0)).scrap;
    let events = run_until(&mut state, 400, |s, events| {
        if let Some(u) = s.unit(patient) {
            assert_eq!(u.hp, 20, "the aura passes it over");
        }
        retired(events, patient).is_some()
    });
    assert_eq!(
        state.player(PlayerId(0)).scrap,
        bank + retired(&events, patient).unwrap().0,
        "nothing was spent on it"
    );
}

#[test]
fn only_carried_scrap_rearms_recovery() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 14, 9),
            unit(0, UnitKind::Sentinel, 14, 11),
        ],
        vec![crucible(0, 20, 9)],
        0,
    );
    let [hauler, guard] = ids(&state)[..] else {
        unreachable!()
    };
    let active = json!({"recovery": "active", "target": 140, "allowance": 140});
    edit(&mut state, |d| d["players"][0]["recovery"] = active.clone());
    state.tick(&[cmd(0, retire(vec![guard]))]);
    run_until(&mut state, 600, |_, events| {
        retired(events, guard).is_some()
    });
    assert!(
        matches!(state.player(PlayerId(0)).recovery, Recovery::Active { .. }),
        "a refund is no delivery"
    );
    edit(&mut state, |d| {
        unit_row(d, hauler)["worker"]["carrying"] = json!(5);
    });
    state.tick(&[cmd(0, retire(vec![hauler]))]);
    let events = run_until(&mut state, 600, |_, events| {
        retired(events, hauler).is_some()
    });
    assert_eq!(retired(&events, hauler).unwrap().1, 5);
    assert_eq!(state.player(PlayerId(0)).recovery, Recovery::Ready);
}

#[test]
fn retire_refuses_without_a_reachable_own_crucible() {
    let refusal = |state: &mut State, units: Vec<UnitId>, reason: RejectReason| {
        let before = state.clone();
        let report = state.tick(&[cmd(0, retire(units))]);
        assert_eq!(rejected(&report.events), [reason]);
        let mut idle = before;
        idle.tick(&[]);
        assert_eq!(state.hash(), idle.hash(), "a refusal changes nothing");
    };
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 10, 10),
            unit(1, UnitKind::Sentinel, 30, 10),
        ],
        vec![crucible(1, 32, 4)],
        0,
    );
    let [own, enemy] = ids(&state)[..] else {
        unreachable!()
    };
    refusal(&mut state, vec![own], RejectReason::MissingPrerequisite);

    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 10, 10),
            unit(1, UnitKind::Sentinel, 30, 10),
        ],
        vec![crucible(0, 20, 9)],
        0,
    );
    refusal(&mut state, vec![enemy], RejectReason::NoValidUnits);

    // A Crucible walled off from the machine.
    let mut scenario = field(
        vec![unit(0, UnitKind::Sentinel, 10, 10)],
        vec![crucible(0, 24, 9)],
        0,
    );
    for row in scenario.map.iter_mut().take(19).skip(1) {
        row.replace_range(18..19, "#");
    }
    let mut state = scenario.build().unwrap();
    refusal(&mut state, vec![own], RejectReason::UnreachableGoal);
}

#[test]
fn mirrored_machines_retire_in_mirrored_steps() {
    let (width, height) = (36, 18);
    let map: Vec<String> = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    if x == 0 || y == 0 || x == width - 1 || y == height - 1 {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    let mirror = |tile: TilePos| TilePos::new(width - 1 - tile.x, height - 1 - tile.y);
    let mirror_pos =
        |pos: Vec2Fx| Vec2Fx::new(Fx::from_num(width) - pos.x, Fx::from_num(height) - pos.y);
    let west = [
        (UnitKind::Sentinel, TilePos::new(4, 5)),
        (UnitKind::Scuttler, TilePos::new(4, 11)),
        (UnitKind::Talon, TilePos::new(3, 8)),
        (UnitKind::Skyhook, TilePos::new(6, 14)),
        (UnitKind::Sentinel, TilePos::new(5, 14)),
        (UnitKind::Sentinel, TilePos::new(5, 15)),
    ];
    let units = west
        .iter()
        .flat_map(|&(kind, at)| {
            let away = mirror(at);
            [unit(0, kind, at.x, at.y), unit(1, kind, away.x, away.y)]
        })
        .collect();
    let mut players = common::players(0);
    for player in &mut players {
        player.team = Some(0);
    }
    let home = TilePos::new(10, 7);
    let away = TilePos::new(width - 2 - home.x, height - 2 - home.y);
    let mut state = Scenario {
        mode: ScenarioMode::Sandbox,
        name: "retire-mirror".into(),
        map,
        players,
        units,
        buildings: vec![crucible(0, home.x, home.y), crucible(1, away.x, away.y)],
        meta: None,
    }
    .build()
    .unwrap();
    let pairs: Vec<(UnitId, UnitId)> = state
        .units()
        .chunks(2)
        .map(|pair| (pair[0].id, pair[1].id))
        .collect();
    let (west_ids, east_ids): (Vec<_>, Vec<_>) = pairs.iter().copied().unzip();
    // The last pair rides the Skyhooks to their Crucibles and is set down
    // there, so the drop ring's frame is mirrored too.
    let (slings, riders) = (pairs[3], [pairs[4], pairs[5]]);
    let load = |player: u8, riders: Vec<UnitId>, sling: UnitId| {
        cmd(
            player,
            Command::Load {
                units: riders,
                transport: sling,
                queue: false,
            },
        )
    };
    state.tick(&[
        load(0, riders.iter().map(|r| r.0).collect(), slings.0),
        load(1, riders.iter().map(|r| r.1).collect(), slings.1),
    ]);
    run_until(&mut state, 300, |s, _| {
        riders
            .iter()
            .all(|&(w, e)| s.unit(w).is_none() && s.unit(e).is_none())
    });
    state.tick(&[
        cmd(0, retire(west_ids[..4].to_vec())),
        cmd(1, retire(east_ids[..4].to_vec())),
    ]);
    for tick in 0..400 {
        let report = state.tick(&[]);
        for &(w, e) in &pairs {
            assert_eq!(
                retired(&report.events, w).is_some(),
                retired(&report.events, e).is_some(),
                "tick {tick}: {w:?} and {e:?} come apart together"
            );
            match (state.unit(w), state.unit(e)) {
                (Some(a), Some(b)) => {
                    assert_eq!(mirror_pos(a.pos), b.pos, "tick {tick}: {w:?} position");
                    assert_eq!(a.progress, b.progress, "tick {tick}: {w:?} meter");
                }
                (None, None) => {}
                _ => panic!("tick {tick}: only one of {w:?} and {e:?} retired"),
            }
        }
    }
    let left: Vec<UnitId> = state.units().iter().map(|u| u.id).collect();
    let mut set_down: Vec<UnitId> = riders.iter().flat_map(|&(w, e)| [w, e]).collect();
    set_down.sort_unstable();
    assert_eq!(left, set_down, "only the set-down riders remain");
}
