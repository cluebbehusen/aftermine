use super::attack::{WEST, armed, attack, phases, restaged};
use super::*;

/// East Turrets across the field's middle, on the way from West's home to
/// East's Foundry.
const LINE: [(i32, i32); 2] = [(30, 10), (30, 12)];

/// Where an attack on East's Foundry stops short of `LINE`.
const STAGE: (i32, i32) = (21, 11);

/// West's army at home, East's Turrets at `turrets`, and a West Kestrel that
/// has seen them, West deciding at `difficulty`.
fn guarded(difficulty: BotDifficulty, turrets: &[(i32, i32)], kestrel: (i32, i32)) -> Scenario {
    let mut scenario = armed(8, &[]);
    scenario
        .units
        .push(unit(0, UnitKind::Kestrel, kestrel.0, kestrel.1));
    for (x, y) in turrets {
        scenario.buildings.push(BuildingSpec {
            player: 1,
            kind: BuildingKind::Turret,
            x: *x,
            y: *y,
        });
    }
    scenario.players[0].bot_config = Some(BotConfig::new(difficulty, BotStance::Balanced, 11));
    scenario
}

/// Whether `tile` stands within `space` tiles of the reach of a Turret of
/// `LINE`.
fn in_reach(tile: TilePos, space: i32) -> bool {
    let reach = BuildingKind::Turret.base_stats().weapons[0]
        .range
        .ceil()
        .to_num::<i32>();
    LINE.iter()
        .any(|(x, y)| crate::frame::gap(TilePos::new(*x, *y), (1, 1), tile, (1, 1)) < reach + space)
}

#[test]
fn an_attack_gathers_short_of_known_defenses_then_goes_in_together() {
    let scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    let mut state = scenario.build().unwrap();
    let army: Vec<UnitId> = WEST.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    let mut opponent = seat_with(&scenario, 0, scenario.players[0].bot_config.unwrap());
    let mut traces = Vec::new();
    let mut push = None;
    while push.is_none() {
        assert!(state.current_tick() < 2_400, "{:?}", phases(&traces));
        for id in &army {
            let unit = state.unit(*id).unwrap();
            assert!(
                !in_reach(unit.tile(), 0),
                "nobody walks into the Turrets' fire before the army goes in: {:?}",
                unit.tile()
            );
        }
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        if let Some(trace) = trace {
            let staged = phases(&traces).contains(&Phase::Stage);
            if staged
                && attack(&trace.missions).is_some_and(|mission| mission.phase == Phase::Travel)
            {
                push = Some(commands.clone());
            }
            traces.push(trace);
        }
        state.tick(&commands);
    }
    assert_eq!(
        phases(&traces),
        [Phase::Gather, Phase::Travel, Phase::Stage, Phase::Travel]
    );
    let stage = traces
        .iter()
        .find_map(|trace| attack(&trace.missions).filter(|mission| mission.phase == Phase::Stage))
        .unwrap()
        .goal;
    assert!(
        !in_reach(stage, 3),
        "it gathers well clear of the Turrets' reach: {stage:?}"
    );
    let [(going, goal)] = &hunts(&push.unwrap())[..] else {
        panic!("one order sends the army in");
    };
    let target = attack(&traces.last().unwrap().missions).unwrap().goal;
    assert_eq!(*goal, target);
    let mut army = army;
    army.sort_unstable();
    assert_eq!(*going, army, "the whole army goes in together");
}

#[test]
fn the_lowest_rung_walks_straight_in() {
    let scenario = guarded(BotDifficulty::Scrapheap, &LINE, (28, 11));
    let mut state = scenario.build().unwrap();
    let mut opponent = seat_with(&scenario, 0, scenario.players[0].bot_config.unwrap());
    let traces = play(&mut opponent, &mut state, 1_200, &[]);
    let seen = phases(&traces);
    assert!(seen.contains(&Phase::Engage), "{seen:?}");
    assert!(!seen.contains(&Phase::Stage), "{seen:?}");
}

#[test]
fn known_defenses_on_the_way_count_toward_an_attack_s_need() {
    // Four Turrets worth more than half the army that does not count them:
    // across the way, or off it beside the field's edge.
    let across: Vec<(i32, i32)> = (9..13).map(|y| (24, y)).collect();
    let aside: Vec<(i32, i32)> = (21..25).map(|x| (x, 2)).collect();
    let launches = |difficulty, turrets: &[(i32, i32)], kestrel| {
        // A second Kestrel shows East's Foundry, the target worth going for.
        let mut scenario = guarded(difficulty, turrets, kestrel);
        scenario.units.push(unit(0, UnitKind::Kestrel, 42, 9));
        let state = scenario.build().unwrap();
        let config = scenario.players[0].bot_config.unwrap();
        let (_, trace) =
            seat_with(&scenario, 0, config).act_traced(&state, &mut OwnEvents::default());
        attack(&trace.unwrap().missions).is_some()
    };
    assert!(!launches(BotDifficulty::Standard, &across, (26, 11)));
    assert!(
        launches(BotDifficulty::Standard, &aside, (23, 4)),
        "those off the way do not count"
    );
    assert!(
        launches(BotDifficulty::Scrapheap, &across, (26, 11)),
        "the lowest rung counts only those around its target"
    );
}

#[test]
fn an_attack_launched_on_a_long_wait_goes_in_at_the_margin_it_launched_on() {
    // East's Sentinels by its Foundry, which a second Kestrel watches from
    // out of their reach, and the Turrets on the way ask more than West's
    // army at the full margin, but not at the margin a long wait has earned.
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    scenario.units.push(unit(0, UnitKind::Kestrel, 41, 4));
    for y in 10..15 {
        scenario.units.push(unit(1, UnitKind::Sentinel, 41, y));
    }
    let mut state = scenario.build().unwrap();
    advance_to(&mut state, 9_600, &[]);
    let now = state.current_tick();
    let mut members: Vec<UnitId> = WEST.iter().map(|(x, y)| at(&state, *x, *y)).collect();
    members.sort_unstable();
    let staged = |waiting: Option<u64>| {
        restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
            json["missions"] = serde_json::json!({
                "next": 1,
                "list": [{
                    "id": 0,
                    "task": {
                        "task": "attack",
                        "target": {"owner": 1, "building": "foundry", "anchor": {"x": 43, "y": 11}},
                        "phase": "gather",
                    },
                    "since": now,
                    "units": members,
                    "goal": {"x": 11, "y": 11},
                }],
                "waiting": waiting,
            });
        })
    };
    let seen = |waiting: Option<u64>| {
        let mut state = state.clone();
        phases(&play(&mut staged(waiting), &mut state, now + 2_400, &[]))
    };
    assert_eq!(
        seen(None),
        [Phase::Gather],
        "premise: the full margin holds it"
    );
    let seen = seen(Some(0));
    assert!(
        seen.starts_with(&[Phase::Travel, Phase::Stage, Phase::Travel, Phase::Engage]),
        "it sets out, gathers short of the Turrets, then goes in: {seen:?}"
    );
}

/// The guarded field at tick 408 with an attack on East's Foundry `phase`
/// since `since`: Sentinels at `there` beside `STAGE`, and the rest of the
/// army's eight at home, on their way since the decision before when
/// `coming`. Four more Sentinels stand free at home.
fn staging(
    phase: &str,
    since: u64,
    there: &[(i32, i32)],
    coming: bool,
) -> (State, Opponent, Vec<UnitId>) {
    staging_on(
        guarded(BotDifficulty::Standard, &LINE, (28, 11)),
        phase,
        since,
        there,
        coming,
    )
}

/// `staging` on `scenario`.
fn staging_on(
    scenario: Scenario,
    phase: &str,
    since: u64,
    there: &[(i32, i32)],
    coming: bool,
) -> (State, Opponent, Vec<UnitId>) {
    staging_with(scenario, phase, since, there, coming, &[])
}

/// `staging_on` with the West units `scenario` places at `extra` among the
/// members at home.
fn staging_with(
    mut scenario: Scenario,
    phase: &str,
    since: u64,
    there: &[(i32, i32)],
    coming: bool,
    extra: &[(i32, i32)],
) -> (State, Opponent, Vec<UnitId>) {
    for (x, y) in there {
        scenario.units.push(unit(0, UnitKind::Sentinel, *x, *y));
    }
    let mut state = scenario.build().unwrap();
    let home: Vec<UnitId> = WEST[..8 - there.len()]
        .iter()
        .chain(extra)
        .map(|(x, y)| at(&state, *x, *y))
        .collect();
    let mut members: Vec<UnitId> = there
        .iter()
        .map(|(x, y)| at(&state, *x, *y))
        .chain(home.iter().copied())
        .collect();
    members.sort_unstable();
    advance_to(&mut state, 396, &[]);
    let orders = if coming {
        vec![run(0, home, STAGE.0, STAGE.1)]
    } else {
        Vec::new()
    };
    advance_to(&mut state, 408, &orders);
    let opponent = restaged(&scenario, &state, staged_attack(phase, since, &members));
    (state, opponent, members)
}

/// An attack of `members` on East's Foundry, `phase` at `STAGE` since
/// `since`.
fn staged_attack(phase: &str, since: u64, members: &[UnitId]) -> serde_json::Value {
    serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {
                "task": "attack",
                "target": {"owner": 1, "building": "foundry", "anchor": {"x": 43, "y": 11}},
                "phase": phase,
            },
            "since": since,
            "units": members,
            "goal": {"x": STAGE.0, "y": STAGE.1},
        }],
    })
}

/// Spots beside `STAGE`.
const THERE: [(i32, i32); 7] = [
    (20, 10),
    (21, 10),
    (20, 11),
    (21, 11),
    (20, 12),
    (21, 12),
    (22, 10),
];

/// West's Maw on `state`.
fn maw_of(state: &State) -> &oxide_sim::state::Unit {
    let maw = state
        .units()
        .iter()
        .find(|unit| unit.player == PlayerId(0) && unit.kind == UnitKind::Maw);
    maw.unwrap()
}

/// Whether `commands` send `members` in at East's Foundry.
fn goes_in(commands: &[PlayerCommand], members: &[UnitId]) -> bool {
    hunts(commands)
        .iter()
        .any(|(units, goal)| goal.chebyshev(TilePos::new(43, 11)) <= 2 && units == members)
}

#[test]
fn a_staging_army_waits_while_the_rest_are_on_their_way() {
    // Half the army has arrived; the rest is still on its way.
    let (state, mut opponent, members) = staging("stage", 408, &THERE[..4], true);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(!goes_in(&commands, &members), "{commands:?}");
    assert_eq!(
        attack(&trace.unwrap().missions).unwrap().phase,
        Phase::Stage
    );

    // The rest stand still at home: nothing is worth waiting for.
    let (state, mut opponent, members) = staging("stage", 408, &THERE[..4], false);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(goes_in(&commands, &members), "{commands:?}");
    assert_eq!(
        attack(&trace.unwrap().missions).unwrap().phase,
        Phase::Travel
    );
}

#[test]
fn a_staging_army_waits_for_a_breacher_on_its_way() {
    // Seven of the eight Sentinels have arrived, more than the share that
    // goes in; a Maw is still on its way, or stands still at home.
    for (coming, goes) in [(true, false), (false, true)] {
        let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
        scenario.units.push(unit(0, UnitKind::Maw, 8, 16));
        let (state, mut opponent, members) =
            staging_with(scenario, "stage", 408, &THERE, coming, &[(8, 16)]);
        let (commands, _) = opponent.act_traced(&state, &mut OwnEvents::default());
        assert_eq!(
            goes_in(&commands, &members),
            goes,
            "coming {coming}: {commands:?}"
        );
    }
}

#[test]
fn a_breacher_leads_the_army_in() {
    // Seven of the eight Sentinels and a Maw stand at the staging spot: the
    // Maw goes in alone, and the rest stay.
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    scenario.units.push(unit(0, UnitKind::Maw, 18, 14));
    let (state, mut opponent, _) = staging_with(scenario, "stage", 408, &THERE, false, &[(18, 14)]);
    let maw = maw_of(&state).id;
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let [(units, goal)] = &hunts(&commands)[..] else {
        panic!("{commands:?}");
    };
    assert_eq!(*units, [maw], "{commands:?}");
    assert!(goal.chebyshev(TilePos::new(43, 11)) <= 2, "{goal:?}");
    assert_eq!(
        attack(&trace.unwrap().missions).unwrap().phase,
        Phase::Stage
    );

    // Once the Maw has come up to the Turrets, the rest follow it in.
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    scenario.units.push(unit(0, UnitKind::Maw, 24, 18));
    let (state, mut opponent, members) =
        staging_with(scenario, "stage", 408, &THERE, false, &[(24, 18)]);
    assert_eq!(maw_of(&state).tile(), TilePos::new(24, 18), "premise");
    let (commands, _) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert!(goes_in(&commands, &members), "{commands:?}");

    // Two East Maws seen by the Foundry outweigh the army the first Maw drew
    // in: the whole army turns home, that Maw with it.
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    scenario.units.push(unit(0, UnitKind::Maw, 24, 18));
    scenario.units.push(unit(0, UnitKind::Kestrel, 42, 9));
    scenario.units.push(unit(1, UnitKind::Maw, 44, 6));
    scenario.units.push(unit(1, UnitKind::Maw, 44, 16));
    let (state, mut opponent, _) = staging_with(scenario, "stage", 408, &THERE, false, &[(24, 18)]);
    let maw = maw_of(&state).id;
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert_eq!(
        attack(&trace.unwrap().missions).unwrap().phase,
        Phase::Recover,
        "{commands:?}"
    );
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, _)| units.contains(&maw)),
        "{commands:?}"
    );
}

#[test]
fn a_breacher_that_cannot_move_on_holds_no_one_up() {
    // Seven of the eight Sentinels and a Maw stand at the staging spot, the
    // Maw stalled for want of a route: the rest go in without it.
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    scenario.units.push(unit(0, UnitKind::Maw, 18, 14));
    let (state, mut opponent, members) =
        staging_with(scenario, "stage", 408, &THERE, false, &[(18, 14)]);
    let maw = maw_of(&state);
    let mut events = OwnEvents::default();
    events.record(
        PlayerId(0),
        &[oxide_sim::Event::OrderStalled {
            unit: maw.id,
            player: PlayerId(0),
            pos: maw.pos,
            reason: oxide_sim::StallReason::NoRoute,
        }],
    );
    let (commands, _) = opponent.act_traced(&state, &mut events);
    let rest: Vec<UnitId> = members.iter().copied().filter(|id| *id != maw.id).collect();
    assert!(goes_in(&commands, &rest), "{commands:?}");
}

#[test]
fn a_staging_army_takes_none_of_the_units_freed_at_home() {
    let scenario = guarded(BotDifficulty::Veteran, &LINE, (28, 11));
    for (phase, joins) in [("stage", false), ("travel", true)] {
        let (state, mut opponent, _) = staging_on(scenario.clone(), phase, 408, &THERE[..4], true);
        let free: Vec<UnitId> = WEST[4..].iter().map(|(x, y)| at(&state, *x, *y)).collect();
        let (commands, _) = opponent.act_traced(&state, &mut OwnEvents::default());
        let sent = hunts(&commands)
            .iter()
            .any(|(units, _)| units.iter().any(|unit| free.contains(unit)));
        assert_eq!(sent, joins, "{phase}: {commands:?}");
    }
}

#[test]
fn an_army_shelled_where_it_gathers_falls_back_out_of_reach() {
    // An East Avalanche behind the Turrets, in sight of West's Kestrel but
    // beyond contact, shells a Sentinel at the staging spot once an East
    // Kestrel flies in to spot for it.
    let (gun, spotter) = (TilePos::new(34, 11), TilePos::new(2, 22));
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (28, 11));
    scenario
        .units
        .push(unit(1, UnitKind::Avalanche, gun.x, gun.y));
    scenario
        .units
        .push(unit(1, UnitKind::Kestrel, spotter.x, spotter.y));
    let (mut state, _, members) = staging_on(scenario.clone(), "stage", 408, &THERE[..4], true);
    let of = |kind: UnitKind| {
        let unit = state
            .units()
            .iter()
            .find(|unit| unit.player == PlayerId(1) && unit.kind == kind);
        unit.unwrap().id
    };
    let (gun, spotter) = (of(UnitKind::Avalanche), of(UnitKind::Kestrel));
    let shelled = *members
        .iter()
        .find(|id| {
            let tile = state.unit(**id).unwrap().tile();
            tile.chebyshev(TilePos::new(STAGE.0, STAGE.1)) <= 1
        })
        .unwrap();
    let east = |command| PlayerCommand {
        player: PlayerId(1),
        command,
    };
    state.tick(&[
        east(Command::Run {
            units: vec![spotter],
            goal: TilePos::new(STAGE.0, 3),
            queue: false,
        }),
        east(Command::Attack {
            units: vec![gun],
            target: AttackTarget::Unit(shelled),
            queue: false,
        }),
    ]);
    let impacts = |state: &State| ObservationData::fog_honest(state, PlayerId(0)).incoming_shells;
    while state.current_tick() % 12 != 0 || impacts(&state).is_empty() {
        assert!(state.current_tick() < 1_000, "premise: the Avalanche fires");
        state.tick(&[]);
    }
    let mut opponent = restaged(&scenario, &state, staged_attack("stage", 408, &members));
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    let mission = attack(&trace.unwrap().missions).unwrap();
    assert_eq!(mission.phase, Phase::Travel, "it heads for a new spot");
    let back = mission.goal;
    assert!(back.x < STAGE.0, "nearer home: {back:?}");
    let reach = crate::defenses::reach(UnitKind::Avalanche)
        .ceil()
        .to_num::<i32>();
    for impact in impacts(&state) {
        assert!(back.chebyshev(impact) >= reach + 3, "{back:?} {impact:?}");
    }
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, goal)| *goal == back && *units == members),
        "{commands:?}"
    );
}

#[test]
fn mirrored_seats_stage_at_mirrored_spots() {
    // Each seat's Kestrel sees the other's Turrets, not its army, then flies
    // home before the armies gather.
    let flip = |(x, y): (i32, i32)| (47 - x, 23 - y);
    let mut scenario = guarded(BotDifficulty::Standard, &LINE, (27, 11));
    for spot in GARRISON.into_iter().chain(WEST) {
        let (x, y) = flip(spot);
        scenario.units.push(unit(1, UnitKind::Sentinel, x, y));
    }
    let (x, y) = flip((27, 11));
    scenario.units.push(unit(1, UnitKind::Kestrel, x, y));
    for spot in LINE {
        let (x, y) = flip(spot);
        scenario.buildings.push(BuildingSpec {
            player: 0,
            kind: BuildingKind::Turret,
            x,
            y,
        });
    }
    let mut state = scenario.build().unwrap();
    let kestrels: Vec<PlayerCommand> = [(0, (27, 11)), (1, flip((27, 11)))]
        .into_iter()
        .zip([(2, 2), flip((2, 2))])
        .map(|((player, (x, y)), home)| run(player, vec![at(&state, x, y)], home.0, home.1))
        .collect();
    let (mut west, mut east) = (seat(&scenario, 0), seat(&scenario, 1));
    loop {
        assert!(state.current_tick() < 2_400, "premise: West sets out");
        let (west_commands, trace) = west.act_traced(&state, &mut OwnEvents::default());
        let east_commands = east.act(&state, &mut OwnEvents::default());
        if let Some(mission) = trace
            .and_then(|trace| attack(&trace.missions))
            .filter(|mission| mission.phase == Phase::Travel)
        {
            assert!(!in_reach(mission.goal, 3), "premise: it stages");
            assert!(
                !hunts(&west_commands).is_empty(),
                "premise: it sets out now"
            );
            assert_eq!(mirror(&state, west_commands), east_commands);
            break;
        }
        let mut commands = [west_commands, east_commands].concat();
        if state.current_tick() == 0 {
            commands.extend(kestrels.iter().cloned());
        }
        state.tick(&commands);
    }
}
