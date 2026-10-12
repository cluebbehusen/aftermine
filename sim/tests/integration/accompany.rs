//! Escort duty: the Accompany order. Headless scenarios through the public
//! API: holding slots around a moving unit without slowing it, fighting
//! near it inside the break radius, welding only while it stands, the end
//! of duty, following it aboard a transport, loop refusal, and mirrored
//! slots.

use crate::common;
use common::{cmd, open_arena, run, run_until, unit};

use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use oxide_sim::command::RejectReason;
use oxide_sim::scenario::{ScenarioMode, UnitSpec};
use oxide_sim::stats::ESCORT_BREAK_RADIUS;
use oxide_sim::{Command, Event, Order, Resume, Scenario, State, UnitId, UnitKind};
use serde_json::json;

fn build(units: Vec<UnitSpec>, scrap: u32) -> State {
    let mut scenario = open_arena(40, 20, units);
    for player in &mut scenario.players {
        player.scrap = scrap;
    }
    scenario.build().unwrap()
}

fn ids(state: &State) -> Vec<UnitId> {
    state.units().iter().map(|u| u.id).collect()
}

fn accompany(units: Vec<UnitId>, target: UnitId, queue: bool) -> Command {
    Command::Accompany {
        units,
        target,
        queue,
    }
}

fn run_to(units: Vec<UnitId>, x: i32, y: i32) -> Command {
    Command::Run {
        units,
        goal: TilePos::new(x, y),
        queue: false,
    }
}

fn escorting(state: &State, escort: UnitId, escorted: UnitId) -> bool {
    state
        .unit(escort)
        .is_some_and(|u| matches!(u.order, Order::Accompany { unit, .. } if unit == escorted))
}

fn fighting_for(state: &State, escort: UnitId, escorted: UnitId) -> bool {
    state.unit(escort).is_some_and(|u| {
        matches!(u.order, Order::Attack { resume: Some(Resume::Accompany(unit)), .. } if unit == escorted)
    })
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

fn set_hp(state: &mut State, id: UnitId, hp: u32) {
    let mut data = serde_json::to_value(&*state).unwrap();
    let row = data["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == json!(id))
        .unwrap();
    row["hp"] = json!(hp);
    *state = serde_json::from_value(data).unwrap();
}

fn stands_at(state: &State, id: UnitId, tile: TilePos) -> bool {
    state
        .unit(id)
        .is_some_and(|u| u.tile() == tile && u.path.is_none() && u.drive_speed() == Fx::ZERO)
}

fn apart(state: &State, a: UnitId, b: UnitId) -> i32 {
    state
        .unit(a)
        .unwrap()
        .tile()
        .chebyshev(state.escorted(b).unwrap().tile())
}

#[test]
fn escorts_hold_slots_around_a_moving_unit() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Lancer, 6, 10),
            unit(0, UnitKind::Scuttler, 4, 7),
            unit(0, UnitKind::Sentinel, 3, 13),
            unit(0, UnitKind::Talon, 5, 5),
        ],
        0,
    );
    let [lead, fast, steady, flyer] = ids(&state)[..] else {
        unreachable!()
    };
    let report = state.tick(&[cmd(0, accompany(vec![fast, steady, flyer], lead, false))]);
    assert!(rejected(&report.events).is_empty());
    run(&mut state, 60);
    for escort in [fast, steady, flyer] {
        assert!(apart(&state, escort, lead) <= 2, "{escort:?} took its slot");
    }

    state.tick(&[cmd(0, run_to(vec![lead], 30, 10))]);
    run_until(&mut state, 1_000, |s, _| {
        stands_at(s, lead, TilePos::new(30, 10))
    });
    run(&mut state, 80);
    for escort in [fast, steady, flyer] {
        assert!(escorting(&state, escort, lead), "{escort:?} stays on duty");
        assert!(
            apart(&state, escort, lead) <= 2,
            "{escort:?} followed to the new stop"
        );
    }
    let slots: std::collections::BTreeSet<TilePos> = [fast, steady]
        .iter()
        .map(|id| state.unit(*id).unwrap().tile())
        .collect();
    assert_eq!(slots.len(), 2, "ground escorts hold distinct slots");
}

#[test]
fn escorts_never_slow_the_escorted_unit() {
    let arrival = |escorted: bool| {
        let mut state = build(
            vec![
                unit(0, UnitKind::Lancer, 6, 10),
                unit(0, UnitKind::Sentinel, 4, 4),
                unit(0, UnitKind::Sentinel, 4, 16),
            ],
            0,
        );
        let [lead, a, b] = ids(&state)[..] else {
            unreachable!()
        };
        let mut commands = vec![cmd(0, run_to(vec![lead], 34, 10))];
        if escorted {
            commands.push(cmd(0, accompany(vec![a, b], lead, false)));
        }
        state.tick(&commands);
        let mut ticks = 1;
        while !stands_at(&state, lead, TilePos::new(34, 10)) {
            state.tick(&[]);
            ticks += 1;
            assert!(ticks < 1_000, "the lead never arrived");
        }
        ticks
    };
    assert_eq!(arrival(true), arrival(false));
}

#[test]
fn an_escort_fights_a_threat_near_the_escorted_unit_then_returns() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 10, 10),
            unit(0, UnitKind::Sentinel, 8, 10),
            unit(1, UnitKind::Scuttler, 25, 10),
        ],
        0,
    );
    let [lead, guard, raider] = ids(&state)[..] else {
        unreachable!()
    };
    set_hp(&mut state, raider, 10);
    state.tick(&[
        cmd(0, accompany(vec![guard], lead, false)),
        cmd(1, run_to(vec![raider], 13, 11)),
    ]);
    run_until(&mut state, 300, |s, _| fighting_for(s, guard, lead));
    run_until(&mut state, 400, |s, _| s.unit(raider).is_none());
    run_until(&mut state, 5, |s, _| escorting(s, guard, lead));
    assert!(state.unit(lead).is_some(), "the escorted unit survived");
}

#[test]
fn an_escort_breaks_off_a_victim_that_strays_past_the_break_radius() {
    // The scout keeps the runner in sight the whole way, so only the break
    // radius can end the chase.
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 10, 10),
            unit(0, UnitKind::Sentinel, 9, 10),
            unit(1, UnitKind::Scuttler, 13, 10),
            unit(0, UnitKind::Kestrel, 24, 10),
        ],
        0,
    );
    let [lead, guard, raider, _scout] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, accompany(vec![guard], lead, false))]);
    run_until(&mut state, 60, |s, _| fighting_for(s, guard, lead));
    state.tick(&[cmd(1, run_to(vec![raider], 36, 10))]);
    let bound = ESCORT_BREAK_RADIUS.to_num::<i32>() + 2;
    run_until(&mut state, 300, |s, _| {
        assert!(
            apart(s, guard, lead) <= bound,
            "the chase stayed near the escorted unit"
        );
        escorting(s, guard, lead)
    });
    assert!(state.unit(raider).is_some(), "the runner got away");
    run(&mut state, 120);
    assert!(apart(&state, guard, lead) <= 2, "the escort came back");
}

#[test]
fn an_escort_shelled_from_beyond_its_engage_radius_holds_its_slot() {
    // A Bombard nine tiles out, spotted by a scout too far from the
    // escorted unit to be picked, shells the escort. An own scout keeps the
    // Bombard in sight, so only the engage radius stops an answer.
    let mut state = build(
        vec![
            unit(0, UnitKind::Harvester, 10, 10),
            unit(0, UnitKind::Sentinel, 11, 10),
            unit(1, UnitKind::Bombard, 19, 10),
            unit(1, UnitKind::Kestrel, 15, 4),
            unit(0, UnitKind::Kestrel, 18, 14),
        ],
        0,
    );
    let [lead, guard, ..] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, accompany(vec![guard], lead, false))]);
    let full = state.unit(guard).unwrap().hp;
    let mut hit = false;
    for _ in 0..300 {
        state.tick(&[]);
        let Some(u) = state.unit(guard) else { break };
        hit |= u.hp < full;
        assert!(!fighting_for(&state, guard, lead), "it never answers");
    }
    assert!(hit, "premise: the shells landed");
}

#[test]
fn a_talon_escort_rises_only_against_aircraft() {
    let duty = |threat: UnitKind| {
        let mut state = build(
            vec![
                unit(0, UnitKind::Harvester, 10, 10),
                unit(0, UnitKind::Talon, 9, 9),
                unit(1, threat, 30, 10),
            ],
            0,
        );
        let [lead, talon, enemy] = ids(&state)[..] else {
            unreachable!()
        };
        state.tick(&[
            cmd(0, accompany(vec![talon], lead, false)),
            cmd(1, run_to(vec![enemy], 13, 10)),
        ]);
        let mut rose = false;
        for _ in 0..200 {
            state.tick(&[]);
            rose |= fighting_for(&state, talon, lead);
        }
        rose
    };
    assert!(
        !duty(UnitKind::Scuttler),
        "ground threats are not a Talon's"
    );
    assert!(
        duty(UnitKind::Buzzard),
        "aircraft near the escorted unit are"
    );
}

#[test]
fn a_welder_escort_mends_the_party_only_while_the_escorted_unit_stands() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 10, 10),
            unit(0, UnitKind::Harvester, 8, 10),
        ],
        500,
    );
    let [lead, welder] = ids(&state)[..] else {
        unreachable!()
    };
    set_hp(&mut state, lead, 20);
    state.tick(&[
        cmd(0, accompany(vec![welder], lead, false)),
        cmd(0, run_to(vec![lead], 30, 10)),
    ]);
    run_until(&mut state, 1_000, |s, _| {
        assert_eq!(s.unit(lead).unwrap().hp, 20, "no weld while it moves");
        assert!(
            matches!(
                s.unit(welder).unwrap().order,
                Order::Accompany { weld: None, .. }
            ),
            "a moving party keeps the welder in its slot"
        );
        stands_at(s, lead, TilePos::new(30, 10))
    });
    let bank = state.player(oxide_sim::PlayerId(0)).scrap;
    let max_hp = UnitKind::Sentinel.stats().max_hp;
    run_until(&mut state, 3_000, |s, _| s.unit(lead).unwrap().hp == max_hp);
    assert!(
        state.player(oxide_sim::PlayerId(0)).scrap < bank,
        "welding bills"
    );
    run(&mut state, 5);
    assert_eq!(
        state.unit(welder).unwrap().order,
        Order::Accompany {
            unit: lead,
            weld: None
        },
        "the torch goes out once the party is whole"
    );
}

#[test]
fn a_broke_welder_escort_waits_without_leaving_duty() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 10, 10),
            unit(0, UnitKind::Harvester, 8, 10),
        ],
        0,
    );
    let [lead, welder] = ids(&state)[..] else {
        unreachable!()
    };
    set_hp(&mut state, lead, 20);
    state.tick(&[cmd(0, accompany(vec![welder], lead, false))]);
    let events = run(&mut state, 300);
    assert_eq!(state.unit(lead).unwrap().hp, 20);
    assert!(escorting(&state, welder, lead));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::OrderStalled { unit, .. } if *unit == welder)),
        "an empty bank stalls nothing"
    );
}

#[test]
fn a_fight_no_route_reaches_returns_the_escort_to_duty() {
    // A victim sealed in rock, inside the break radius and in a scout's
    // sight, but out of the escort's gun range: the chase can only stall.
    let mut state = common::open_arena_with(
        40,
        20,
        vec![
            unit(0, UnitKind::Harvester, 13, 10),
            unit(0, UnitKind::Sentinel, 12, 10),
            unit(1, UnitKind::Harvester, 20, 10),
            unit(0, UnitKind::Kestrel, 18, 7),
        ],
        |rows| {
            for (x, y) in [
                (19, 9),
                (20, 9),
                (21, 9),
                (19, 10),
                (21, 10),
                (19, 11),
                (20, 11),
                (21, 11),
            ] {
                rows[y][x] = '#';
            }
        },
    )
    .build()
    .unwrap();
    let [lead, guard, sealed, _scout] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, accompany(vec![guard], lead, false))]);
    let mut data = serde_json::to_value(&state).unwrap();
    let row = data["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == json!(guard))
        .unwrap();
    row["order"] = json!({
        "order": "attack",
        "target": {"kind": "unit", "id": sealed},
        "resume": {"accompany": lead},
    });
    state = serde_json::from_value(data).unwrap();
    let events = run(&mut state, 3);
    assert!(
        escorting(&state, guard, lead),
        "the stall returns it to duty"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::OrderStalled { unit, .. } if *unit == guard)),
        "quietly: escort duty picked that fight, not the player"
    );
}

#[test]
fn an_escort_never_picks_a_threat_it_can_neither_shoot_nor_reach() {
    // Sealed in rock five tiles from the escorted unit: inside the engage
    // radius, beyond every slot's gun range, and walled off. Picking it
    // would stall the chase and pick it again every tick.
    let mut state = common::open_arena_with(
        40,
        20,
        vec![
            unit(0, UnitKind::Harvester, 13, 10),
            unit(0, UnitKind::Sentinel, 12, 10),
            unit(1, UnitKind::Harvester, 18, 10),
            unit(0, UnitKind::Kestrel, 18, 7),
        ],
        |rows| {
            for (x, y) in [
                (17, 9),
                (18, 9),
                (19, 9),
                (17, 10),
                (19, 10),
                (17, 11),
                (18, 11),
                (19, 11),
            ] {
                rows[y][x] = '#';
            }
        },
    )
    .build()
    .unwrap();
    let [lead, guard, ..] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[cmd(0, accompany(vec![guard], lead, false))]);
    for _ in 0..100 {
        state.tick(&[]);
        assert!(escorting(&state, guard, lead), "no fight is ever picked");
    }
}

#[test]
fn escorts_take_their_next_order_or_idle_when_the_escorted_unit_leaves_play() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 10, 10),
            unit(0, UnitKind::Sentinel, 8, 8),
            unit(0, UnitKind::Sentinel, 8, 12),
        ],
        0,
    );
    let [lead, queued, plain] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[
        cmd(0, accompany(vec![queued, plain], lead, false)),
        cmd(
            0,
            Command::Run {
                units: vec![queued],
                goal: TilePos::new(20, 4),
                queue: true,
            },
        ),
    ]);
    run(&mut state, 60);
    let rest = state.unit(plain).unwrap().pos;
    let mut data = serde_json::to_value(&state).unwrap();
    data["units"]
        .as_array_mut()
        .unwrap()
        .retain(|u| u["id"] != json!(lead));
    state = serde_json::from_value(data).unwrap();
    state.tick(&[]);
    assert!(
        matches!(state.unit(queued).unwrap().order, Order::Run { goal } if goal.tile() == TilePos::new(20, 4)),
        "the queued order takes over"
    );
    let idle = state.unit(plain).unwrap();
    assert_eq!(idle.order, Order::Idle, "nothing queued: idle");
    assert_eq!(idle.pos, rest, "where it stands");
}

#[test]
fn escorts_follow_the_escorted_unit_aboard_a_transport() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 6, 10),
            unit(0, UnitKind::Skyhook, 8, 10),
            unit(0, UnitKind::Sentinel, 5, 7),
        ],
        0,
    );
    let [lead, sling, escort] = ids(&state)[..] else {
        unreachable!()
    };
    state.tick(&[
        cmd(0, accompany(vec![escort], lead, false)),
        cmd(
            0,
            Command::Load {
                units: vec![lead],
                transport: sling,
                queue: false,
            },
        ),
    ]);
    run_until(&mut state, 200, |s, _| s.unit(lead).is_none());
    state.tick(&[cmd(
        0,
        Command::Unload {
            transport: sling,
            at: TilePos::new(28, 10),
            queue: false,
        },
    )]);
    run_until(&mut state, 800, |s, _| {
        assert!(escorting(s, escort, lead), "duty survives the ride");
        s.unit(lead).is_some()
    });
    run(&mut state, 150);
    assert!(escorting(&state, escort, lead));
    assert!(apart(&state, escort, lead) <= 2, "the escort kept up");
}

#[test]
fn a_transport_carrying_the_unit_it_escorts_holds_still() {
    // Eight flyers ahead of the sling in rank push its slot two tiles out,
    // past the arrival tolerance, so only the carrier rule keeps it from
    // chasing a slot around itself.
    let mut units = vec![unit(0, UnitKind::Sentinel, 6, 10)];
    units.extend((0..8).map(|i| unit(0, UnitKind::Talon, 3 + i, 4)));
    units.push(unit(0, UnitKind::Skyhook, 8, 10));
    let mut state = build(units, 0);
    let all = ids(&state);
    let (lead, sling) = (all[0], all[9]);
    state.tick(&[
        cmd(0, accompany(all[1..].to_vec(), lead, false)),
        cmd(
            0,
            Command::Load {
                units: vec![lead],
                transport: sling,
                queue: false,
            },
        ),
    ]);
    run_until(&mut state, 200, |s, _| s.unit(lead).is_none());
    run(&mut state, 20);
    let parked = state.unit(sling).unwrap().tile();
    for _ in 0..200 {
        state.tick(&[]);
        assert!(
            state.unit(sling).unwrap().tile().chebyshev(parked) <= 1,
            "no chase of its own slot; the flyers around it only jostle it"
        );
    }
    assert!(escorting(&state, sling, lead));
}

#[test]
fn escort_loops_are_refused_at_dispatch() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 6, 6),
            unit(0, UnitKind::Sentinel, 6, 9),
            unit(0, UnitKind::Sentinel, 6, 12),
            unit(0, UnitKind::Sentinel, 6, 15),
        ],
        0,
    );
    let [a, b, c, d] = ids(&state)[..] else {
        unreachable!()
    };
    let report = state.tick(&[
        cmd(0, accompany(vec![a], b, false)),
        cmd(0, accompany(vec![b], d, false)),
    ]);
    assert!(rejected(&report.events).is_empty());
    let refuse = |state: &mut State, command: Command| {
        let before = state.clone();
        let report = state.tick(&[cmd(0, command)]);
        assert_eq!(rejected(&report.events), [RejectReason::NoValidUnits]);
        let mut idle = before;
        idle.tick(&[]);
        assert_eq!(state.hash(), idle.hash(), "a refusal changes nothing");
    };
    // Directly, and through the chain a -> b -> d.
    refuse(&mut state, accompany(vec![b], a, false));
    refuse(&mut state, accompany(vec![d], a, false));
    // Through an escort waiting in a program.
    state.tick(&[cmd(0, run_to(vec![c], 20, 4))]);
    state.tick(&[cmd(0, accompany(vec![c], a, true))]);
    refuse(&mut state, accompany(vec![a], c, false));
}

#[test]
fn accompany_refuses_foreign_and_absent_targets() {
    let mut state = build(
        vec![
            unit(0, UnitKind::Sentinel, 6, 6),
            unit(0, UnitKind::Sentinel, 6, 9),
            unit(1, UnitKind::Sentinel, 30, 15),
        ],
        0,
    );
    let [own, mate, enemy] = ids(&state)[..] else {
        unreachable!()
    };
    for (target, reason) in [
        (enemy, RejectReason::InvalidTarget),
        (UnitId(999), RejectReason::InvalidTarget),
        (own, RejectReason::NoValidUnits),
    ] {
        let before = state.clone();
        let report = state.tick(&[cmd(0, accompany(vec![own], target, false))]);
        assert_eq!(rejected(&report.events), [reason]);
        let mut idle = before;
        idle.tick(&[]);
        assert_eq!(state.hash(), idle.hash());
    }
    // The target never escorts itself; the rest of the list does.
    let report = state.tick(&[cmd(0, accompany(vec![own, mate], own, false))]);
    assert!(rejected(&report.events).is_empty());
    assert!(escorting(&state, mate, own));
    assert_eq!(state.unit(own).unwrap().order, Order::Idle);
}

#[test]
fn mirrored_escorts_take_mirrored_slots() {
    let (width, height) = (36, 18);
    let map: Vec<String> = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    let border = x == 0 || y == 0 || x == width - 1 || y == height - 1;
                    if border || (x, y) == (14, 7) || (x, y) == (width - 15, height - 8) {
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
        (UnitKind::Lancer, TilePos::new(5, 8)),
        (UnitKind::Sentinel, TilePos::new(3, 5)),
        (UnitKind::Scuttler, TilePos::new(3, 11)),
        (UnitKind::Harvester, TilePos::new(2, 8)),
        (UnitKind::Talon, TilePos::new(4, 3)),
    ];
    let units = west
        .iter()
        .flat_map(|&(kind, at)| {
            let away = mirror(at);
            [unit(0, kind, at.x, at.y), unit(1, kind, away.x, away.y)]
        })
        .collect();
    // One team, so the mirrored armies crossing mid-map never fight.
    let mut players = common::players(0);
    for player in &mut players {
        player.team = Some(0);
    }
    let mut state = Scenario {
        mode: ScenarioMode::Sandbox,
        name: "escort-mirror".into(),
        map,
        players,
        units,
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap();
    let pairs: Vec<(UnitId, UnitId)> = state
        .units()
        .chunks(2)
        .map(|pair| (pair[0].id, pair[1].id))
        .collect();
    let side = |player: u8| -> Vec<UnitId> {
        pairs
            .iter()
            .map(|&(w, e)| if player == 0 { w } else { e })
            .collect()
    };
    let (west_ids, east_ids) = (side(0), side(1));
    let goal = TilePos::new(26, 6);
    let far = mirror(goal);
    state.tick(&[
        cmd(0, accompany(west_ids[1..].to_vec(), west_ids[0], false)),
        cmd(1, accompany(east_ids[1..].to_vec(), east_ids[0], false)),
        cmd(0, run_to(vec![west_ids[0]], goal.x, goal.y)),
        cmd(1, run_to(vec![east_ids[0]], far.x, far.y)),
    ]);
    for tick in 0..600 {
        for &(w, e) in &pairs {
            let (a, b) = (state.unit(w).unwrap(), state.unit(e).unwrap());
            assert_eq!(mirror_pos(a.pos), b.pos, "tick {tick}: {w:?} position");
            assert_eq!(
                a.path.as_ref().map(|path| mirror(path.goal)),
                b.path.as_ref().map(|path| path.goal),
                "tick {tick}: {w:?} route"
            );
        }
        state.tick(&[]);
    }
    for &escort in &west_ids[1..] {
        assert!(escorting(&state, escort, west_ids[0]));
    }
}
