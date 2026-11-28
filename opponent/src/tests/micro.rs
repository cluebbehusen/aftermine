//! The top rung handles its fighting units one at a time: it steps them out of
//! an incoming shell's blast, and keeps a reloading gun out of a slower,
//! shorter-reaching enemy's reach.

use super::*;
use chassis::fx::Fx;

/// The field with West's idle `west` and East's `east`, each kind and tile.
fn staged(west: &[(UnitKind, i32, i32)], east: &[(UnitKind, i32, i32)]) -> Scenario {
    let mut scenario = field();
    for &(kind, x, y) in west {
        scenario.units.push(unit(0, kind, x, y));
    }
    for &(kind, x, y) in east {
        scenario.units.push(unit(1, kind, x, y));
    }
    scenario
}

/// The widest blast of any shell, as the bot reckons it.
fn blast() -> Fx {
    UnitKind::ALL
        .iter()
        .flat_map(|kind| kind.stats().weapons)
        .chain(
            BuildingKind::ALL
                .iter()
                .flat_map(|kind| kind.tiers())
                .flat_map(|tier| tier.weapons),
        )
        .filter(|weapon| weapon.projectile.is_some())
        .filter_map(|weapon| weapon.splash)
        .max()
        .unwrap()
}

fn apart(a: TilePos, b: TilePos) -> Fx {
    (a.center() - b.center()).length()
}

/// West's Sentinel far from home, shelled by East's Bombard out of its sight
/// with a Kestrel spotting, played to a decision of `difficulty` on which a
/// shell is seen coming down: the scenario, the state and the Sentinel.
fn shelled(difficulty: BotDifficulty) -> (Scenario, State, UnitId, Opponent) {
    let scenario = staged(
        &[(UnitKind::Sentinel, 20, 4)],
        &[(UnitKind::Bombard, 28, 4), (UnitKind::Kestrel, 20, 10)],
    );
    let mut state = scenario.build().unwrap();
    let (sentinel, bombard) = (at(&state, 20, 4), at(&state, 28, 4));
    state.tick(&[PlayerCommand {
        player: PlayerId(1),
        command: Command::Attack {
            units: vec![bombard],
            target: AttackTarget::Unit(sentinel),
            queue: false,
        },
    }]);
    let opponent = seat_with(&scenario, 0, rung(difficulty));
    while !opponent.decision_due(&state)
        || ObservationData::fog_honest(&state, PlayerId(0))
            .incoming_shells
            .is_empty()
    {
        assert!(state.current_tick() < 600, "premise: the Bombard fires");
        state.tick(&[]);
    }
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    assert!(
        observation
            .enemy_units
            .iter()
            .all(|enemy| enemy.kind != UnitKind::Bombard),
        "premise: the gun is out of sight"
    );
    (scenario, state, sentinel, opponent)
}

#[test]
fn the_top_rung_steps_a_unit_out_of_a_shell_s_blast_and_back_to_where_it_stood() {
    let (_, state, sentinel, mut prime) = shelled(BotDifficulty::Prime);
    let impacts = ObservationData::fog_honest(&state, PlayerId(0)).incoming_shells;
    let commands = prime.act(&state, &mut OwnEvents::default());
    let [(_, refuge)] = &advances(&commands)
        .into_iter()
        .filter(|(units, _)| units == &[sentinel])
        .collect::<Vec<_>>()[..]
    else {
        panic!("{commands:?}");
    };
    assert!(
        impacts
            .iter()
            .all(|impact| apart(*refuge, *impact) > blast()),
        "clear of every blast: {refuge:?} {impacts:?}"
    );
    assert!(
        hunts(&commands)
            .iter()
            .any(|(units, goal)| units == &[sentinel] && *goal == TilePos::new(20, 4)),
        "then back where it stood: {commands:?}"
    );
}

#[test]
fn a_lower_rung_leaves_its_unit_under_the_shell() {
    let (_, state, sentinel, mut veteran) = shelled(BotDifficulty::Veteran);
    let commands = veteran.act(&state, &mut OwnEvents::default());
    assert!(
        advances(&commands)
            .iter()
            .chain(&runs(&commands))
            .all(|(units, _)| !units.contains(&sentinel)),
        "{commands:?}"
    );
}

/// West's Lancer at (20, 4) with `reload` ticks still to wait, and East's
/// `attacker` closing from (25, 4), as a seat of `difficulty` orders them on
/// its first look: the commands and the two units.
fn kited(
    reload: u32,
    attacker: UnitKind,
    difficulty: BotDifficulty,
) -> (Vec<PlayerCommand>, UnitId, UnitId) {
    let scenario = staged(&[(UnitKind::Lancer, 20, 4)], &[(attacker, 25, 4)]);
    let built = scenario.build().unwrap();
    let mut value = serde_json::to_value(&built).unwrap();
    for entry in value["units"].as_array_mut().unwrap() {
        if entry["kind"] == "lancer" {
            entry["cooldowns"][0] = (reload + 1).into();
        }
    }
    let state: State = serde_json::from_value(value).unwrap();
    let (lancer, enemy) = (at(&state, 20, 4), at(&state, 25, 4));
    let commands = seat_with(&scenario, 0, rung(difficulty)).act(&state, &mut OwnEvents::default());
    (commands, lancer, enemy)
}

#[test]
fn the_top_rung_keeps_a_reloading_gun_out_of_a_slower_shorter_reaching_enemy_s_reach() {
    let (commands, lancer, breaker) = kited(40, UnitKind::Breaker, BotDifficulty::Prime);
    let [(_, goal)] = &advances(&commands)
        .into_iter()
        .filter(|(units, _)| units == &[lancer])
        .collect::<Vec<_>>()[..]
    else {
        panic!("{commands:?}");
    };
    let breaker_at = TilePos::new(25, 4);
    let reach = UnitKind::Breaker.stats().weapons[0].range;
    assert!(
        apart(*goal, breaker_at) > reach,
        "out of the Breaker's reach: {goal:?}"
    );
    assert!(
        attacks(&commands)
            .iter()
            .any(|(units, target)| units == &[lancer] && *target == AttackTarget::Unit(breaker)),
        "then on it again: {commands:?}"
    );
}

#[test]
fn a_gun_stands_and_fires_once_ready_or_when_the_enemy_is_faster_or_below_the_top_rung() {
    let stepped = |(commands, lancer, _): (Vec<PlayerCommand>, UnitId, UnitId)| {
        advances(&commands)
            .iter()
            .chain(&runs(&commands))
            .any(|(units, _)| units.contains(&lancer))
    };
    assert!(
        stepped(kited(40, UnitKind::Breaker, BotDifficulty::Prime)),
        "premise"
    );
    assert!(
        !stepped(kited(0, UnitKind::Breaker, BotDifficulty::Prime)),
        "its gun is ready"
    );
    assert!(
        !stepped(kited(40, UnitKind::Sentinel, BotDifficulty::Prime)),
        "a Sentinel outpaces it"
    );
    assert!(
        !stepped(kited(40, UnitKind::Breaker, BotDifficulty::Veteran)),
        "below the top rung"
    );
}

/// What a seat ordered, its missions' rosters after, and the wounded unit.
type PullOut = (Vec<PlayerCommand>, Vec<(u64, Vec<UnitId>)>, UnitId);

/// West's attack in its fight far from home, a Lancer at 15 health and a
/// sound one, against East's `enemy` close by, as a seat of `difficulty`
/// orders it: the commands, the trace's rosters and the wounded Lancer.
fn wounded_in_a_fight(enemy: UnitKind, difficulty: BotDifficulty) -> PullOut {
    let mut scenario = staged(
        &[(UnitKind::Lancer, 30, 4), (UnitKind::Lancer, 30, 5)],
        &[(enemy, 34, 4)],
    );
    scenario.players[0].bot_config = Some(rung(difficulty));
    let built = scenario.build().unwrap();
    let wounded = at(&built, 30, 4);
    let mut value = serde_json::to_value(&built).unwrap();
    for entry in value["units"].as_array_mut().unwrap() {
        if entry["id"] == serde_json::to_value(wounded).unwrap() {
            entry["hp"] = 15.into();
        }
    }
    let state: State = serde_json::from_value(value).unwrap();
    let mut members = vec![wounded, at(&state, 30, 5)];
    members.sort_unstable();
    let missions = serde_json::json!({
        "next": 1,
        "list": [{
            "id": 0,
            "task": {
                "task": "attack",
                "target": {"owner": 1, "building": "foundry", "anchor": {"x": 43, "y": 11}},
                "phase": {"engage": {"focus": null, "progressed": 0}},
            },
            "since": 0,
            "units": members,
            "goal": {"x": 42, "y": 11},
        }],
    });
    let mut opponent = super::attack::restaged(&scenario, &state, missions);
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    (commands, trace.unwrap().members, wounded)
}

#[test]
fn the_top_rung_sends_a_badly_wounded_unit_home_out_of_a_fight_it_can_outrun() {
    let (commands, members, wounded) = wounded_in_a_fight(UnitKind::Breaker, BotDifficulty::Prime);
    assert!(
        runs(&commands)
            .iter()
            .any(|(units, goal)| units == &[wounded] && goal.x < 12),
        "home: {commands:?}"
    );
    assert!(
        members.iter().all(|(_, units)| !units.contains(&wounded)),
        "out of the attack: {members:?}"
    );
}

#[test]
fn a_badly_wounded_unit_stays_in_a_fight_it_cannot_outrun_or_below_the_top_rung() {
    // Pulled out, it leaves alone and out of its attack.
    let home = |(commands, members, wounded): PullOut| {
        runs(&commands).iter().any(|(units, _)| units == &[wounded])
            && members.iter().all(|(_, units)| !units.contains(&wounded))
    };
    assert!(
        home(wounded_in_a_fight(UnitKind::Breaker, BotDifficulty::Prime)),
        "premise"
    );
    assert!(
        !home(wounded_in_a_fight(UnitKind::Sentinel, BotDifficulty::Prime)),
        "a Sentinel outpaces it"
    );
    assert!(
        !home(wounded_in_a_fight(
            UnitKind::Breaker,
            BotDifficulty::Veteran
        )),
        "below the top rung"
    );
}
