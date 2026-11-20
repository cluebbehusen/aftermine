use super::*;
use aftermine_kit::controller::{record_events, seat_controllers};
use aftermine_sim::scenario::{BotConfig, BotDifficulty, BotStance, UnitSpec};
use aftermine_sim::{BuildingKind, Command, Event, UnitKind};
use chassis::grid::TilePos;

/// One challenger game: the challenger's commands by tick, how many the
/// game rejected, and the final state.
struct Played {
    log: Vec<(u64, Vec<PlayerCommand>)>,
    rejected: usize,
    state: State,
}

/// Skirmish with every seat a passive chair: nobody but the challenger acts.
fn passive_skirmish() -> Scenario {
    let mut scenario = Scenario::skirmish();
    for seat in &mut scenario.players {
        seat.bot = false;
        seat.bot_config = None;
    }
    scenario
}

/// Skirmish with seat 1 the Prime bot.
fn skirmish_against_prime(seed: u64) -> Scenario {
    let mut scenario = passive_skirmish();
    scenario.players[1].bot = true;
    scenario.players[1].bot_config = Some(BotConfig::new(
        BotDifficulty::Prime,
        BotStance::Balanced,
        seed + 9000,
    ));
    scenario
}

/// Plays seat 0 as `family` from the start of `scenario`, with its
/// configured bots, until `ticks` or a result.
fn play(family: Family, seed: u64, scenario: &Scenario, ticks: u64) -> Played {
    let mut state = scenario.build().unwrap();
    let mut challenger = Challenger::new(family, PlayerId(0), seed, scenario).unwrap();
    let mut bots = seat_controllers(scenario).unwrap();
    let mut log = Vec::new();
    let mut rejected = 0;
    while state.current_tick() < ticks && state.result().is_none() {
        let mine = challenger.act(&state);
        let mut commands = mine.clone();
        commands.extend(aftermine_kit::bot_execution::commands(&state, &mut bots));
        if !mine.is_empty() {
            log.push((state.current_tick(), mine));
        }
        let report = state.tick(&commands);
        record_events(&mut bots, &report);
        rejected += report
            .events
            .iter()
            .filter(
                |e| matches!(e, Event::CommandRejected { player, .. } if *player == PlayerId(0)),
            )
            .count();
    }
    Played {
        log,
        rejected,
        state,
    }
}

fn issued(played: &Played, matches: impl Fn(&Command) -> bool) -> bool {
    played
        .log
        .iter()
        .flat_map(|(_, commands)| commands)
        .any(|c| matches(&c.command))
}

fn finished(state: &State, kind: BuildingKind) -> usize {
    state
        .buildings()
        .iter()
        .filter(|b| b.player == PlayerId(0) && b.kind == kind && b.built() && b.hp > 0)
        .count()
}

fn units(state: &State, kind: UnitKind) -> usize {
    state
        .units()
        .iter()
        .filter(|u| u.player == PlayerId(0) && u.kind == kind && u.hp > 0)
        .count()
}

#[test]
fn families_parse_from_and_print_as_their_kebab_case_names() {
    for family in Family::ALL {
        assert_eq!(family.name().parse::<Family>().unwrap(), family);
        assert_eq!(family.to_string(), family.name());
        assert_eq!(
            serde_json::to_string(&family).unwrap(),
            format!("\"{}\"", family.name())
        );
    }
    assert!("forward_fabricator".parse::<Family>().is_err());
    assert_eq!(
        Family::TUNING.len() + Family::HELD_OUT.len() + Family::YARDSTICK.len(),
        Family::ALL.len()
    );
    assert!(Family::HELD_OUT.iter().all(|f| f.held_out()));
    assert!(Family::TUNING.iter().all(|f| !f.held_out()));
}

#[test]
fn each_family_opens_its_plan_against_a_passive_seat() {
    let scenario = passive_skirmish();
    let start = scenario.build().unwrap();
    let foundry = |seat: u8| {
        start
            .buildings()
            .iter()
            .find(|b| b.player == PlayerId(seat) && b.kind == BuildingKind::Foundry)
            .unwrap()
            .anchor
    };
    let (home, enemy) = (foundry(0), foundry(1));
    for family in Family::ALL {
        let played = play(family, 3, &scenario, 4800);
        let state = &played.state;
        let commands: usize = played.log.iter().map(|(_, c)| c.len()).sum();
        assert!(
            issued(&played, |c| matches!(c, Command::Harvest { .. })),
            "{family} never harvests"
        );
        assert!(
            issued(&played, |c| matches!(c, Command::Train { .. })),
            "{family} never trains"
        );
        assert!(
            finished(state, BuildingKind::Extractor) >= 1,
            "{family} restores no Extractor"
        );
        assert!(
            played.rejected * 20 <= commands,
            "{family}: {} of {commands} commands rejected",
            played.rejected
        );
        for (tick, commands) in &played.log {
            assert_eq!(tick % DECISION_TICKS, 0, "{family} decided off its cadence");
            assert!(
                commands.len() <= turn::COMMANDS_PER_DECISION,
                "{family} issued {} commands at tick {tick}",
                commands.len()
            );
        }
        let won = state.winners().contains(&PlayerId(0));
        let key = match family {
            Family::ForwardFabricator => {
                let fabricator = state
                    .buildings()
                    .iter()
                    .find(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Fabricator);
                let fabricator = fabricator.map(|b| b.anchor);
                let forward = fabricator
                    .is_some_and(|anchor| anchor.chebyshev(enemy) < anchor.chebyshev(home));
                won || (forward && units(state, UnitKind::Lancer) >= 1)
            }
            Family::ScuttlerRaid => units(state, UnitKind::Scuttler) >= 1 || won,
            Family::LancerHarass => {
                finished(state, BuildingKind::Fabricator) >= 1
                    && (units(state, UnitKind::Lancer) >= 1 || won)
            }
            Family::HeldArmy => {
                finished(state, BuildingKind::Fabricator) >= 1
                    && finished(state, BuildingKind::Turret) >= 1
            }
            Family::BuzzardHarass => {
                finished(state, BuildingKind::Airworks) >= 1
                    && (units(state, UnitKind::Buzzard) >= 1 || won)
            }
            Family::BombardSiege => {
                units(state, UnitKind::Bombard) >= 1 && units(state, UnitKind::Kestrel) >= 1
            }
            Family::BallRush => {
                won || (units(state, UnitKind::Sentinel) >= 1
                    && units(state, UnitKind::Scuttler) >= 1)
            }
            Family::KestrelPush => {
                units(state, UnitKind::Kestrel) >= 1 && units(state, UnitKind::Lancer) >= 1
            }
            Family::RangedHold => {
                finished(state, BuildingKind::Fabricator) >= 1
                    && state.units().iter().any(|u| {
                        u.player == PlayerId(0)
                            && u.kind == UnitKind::Lancer
                            && u.tile().chebyshev(home) * 3 >= home.chebyshev(enemy)
                    })
            }
            Family::Novice => state
                .buildings()
                .iter()
                .any(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Fabricator),
        };
        assert!(
            key,
            "{family} did not reach its plan's key step by tick {}",
            state.current_tick()
        );
    }
}

#[test]
fn the_held_army_stays_home_until_its_hour() {
    let scenario = passive_skirmish();
    let mut state = scenario.build().unwrap();
    let mut challenger = Challenger::new(Family::HeldArmy, PlayerId(0), 5, &scenario).unwrap();
    let map = map::MapFacts::new(&scenario, PlayerId(0)).unwrap();
    while state.current_tick() < 4700 {
        let commands = challenger.act(&state);
        state.tick(&commands);
        for unit in state
            .units()
            .iter()
            .filter(|u| u.player == PlayerId(0) && turn::fighter(u.kind))
        {
            let tile = unit.tile();
            assert!(
                map.steps_from_home(tile) <= map.steps_from_enemy(tile),
                "{:?} left home at tick {}",
                unit.kind,
                state.current_tick()
            );
        }
    }
}

#[test]
fn the_ranged_hold_holds_a_line_in_the_middle_until_it_marches() {
    let scenario = passive_skirmish();
    let mut state = scenario.build().unwrap();
    let mut challenger = Challenger::new(Family::RangedHold, PlayerId(0), 5, &scenario).unwrap();
    let map = map::MapFacts::new(&scenario, PlayerId(0)).unwrap();
    let route = map.route_length();
    let army = |state: &State| -> Vec<TilePos> {
        state
            .units()
            .iter()
            .filter(|u| {
                u.player == PlayerId(0) && matches!(u.kind, UnitKind::Lancer | UnitKind::Warden)
            })
            .map(aftermine_sim::Unit::tile)
            .collect()
    };
    while state.current_tick() < 8800 {
        let commands = challenger.act(&state);
        state.tick(&commands);
        for tile in army(&state) {
            assert!(
                map.steps_from_enemy(tile) * 100 >= route * 30,
                "the army passed the line at tick {}",
                state.current_tick()
            );
        }
    }
    let out = army(&state)
        .into_iter()
        .filter(|tile| map.steps_from_home(*tile) * 100 >= route * 35)
        .count();
    assert!(out >= 3, "{out} of the army stands out on the line");
}

#[test]
fn the_same_seed_against_the_bot_issues_the_same_commands() {
    for family in Family::ALL {
        let scenario = skirmish_against_prime(4);
        let first = play(family, 4, &scenario, 1600);
        let second = play(family, 4, &scenario, 1600);
        assert!(!first.log.is_empty(), "{family} issued nothing");
        assert_eq!(first.log, second.log, "{family} is not deterministic");
        assert_eq!(first.state.hash(), second.state.hash());
    }
}

/// Shifts every unit id above `hidden` down by one, so a run with one extra
/// unit staged at id `hidden` reads like a run without it.
fn without_unit(value: &mut serde_json::Value, hidden: u64) {
    let shift = |v: &mut serde_json::Value| {
        if let Some(id) = v.as_u64() {
            assert_ne!(id, hidden, "a command named the hidden unit");
            if id > hidden {
                *v = serde_json::Value::from(id - 1);
            }
        }
    };
    match value {
        serde_json::Value::Object(map) => {
            let unit_target = map.get("kind").and_then(|k| k.as_str()) == Some("unit");
            for (key, v) in map.iter_mut() {
                match key.as_str() {
                    "units" => v.as_array_mut().into_iter().flatten().for_each(shift),
                    "unit" | "transport" => shift(v),
                    "target" if v.is_number() => shift(v),
                    "id" if unit_target => shift(v),
                    _ => without_unit(v, hidden),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| without_unit(v, hidden)),
        _ => {}
    }
}

/// Plays seat 0 as `family` from the start of `plain` and of `staged`, which
/// differ only in enemy state seat 0 cannot see, and checks each decision
/// is the same in both while `hidden` (an extra enemy unit in `staged`, if
/// any) stays out of sight. Returns the ticks compared.
fn compare_hidden(family: Family, plain: &Scenario, staged: &Scenario, hidden: Option<u64>) -> u64 {
    let mut a = plain.build().unwrap();
    let mut b = staged.build().unwrap();
    let mut first = Challenger::new(family, PlayerId(0), 7, plain).unwrap();
    let mut second = Challenger::new(family, PlayerId(0), 7, staged).unwrap();
    while a.current_tick() < 2400 {
        if let Some(id) = hidden {
            let unit = b
                .unit(aftermine_sim::UnitId(u32::try_from(id).unwrap()))
                .unwrap();
            if b.sees_unit(PlayerId(0), unit) {
                break;
            }
        }
        let left = first.act(&a);
        let right = second.act(&b);
        let mut right_json = serde_json::to_value(&right).unwrap();
        if let Some(id) = hidden {
            without_unit(&mut right_json, id);
        }
        assert_eq!(
            serde_json::to_value(&left).unwrap(),
            right_json,
            "{family} at tick {} read hidden enemy state",
            a.current_tick()
        );
        a.tick(&left);
        b.tick(&right);
    }
    a.current_tick()
}

#[test]
fn hidden_enemy_state_does_not_change_a_challengers_commands() {
    let plain = deep_cut();
    // A rich enemy bank, never in sight.
    let mut rich = plain.clone();
    rich.players[1].scrap = 5000;
    // An extra enemy Surveyor parked in the corner far from both bases,
    // compared until seat 0 first sees it.
    let mut scouted = rich.clone();
    scouted.units.push(UnitSpec {
        player: 1,
        kind: UnitKind::Surveyor,
        x: 62,
        y: 1,
    });
    let hidden = u64::try_from(plain.units.len()).unwrap();
    for family in Family::ALL {
        assert_eq!(compare_hidden(family, &plain, &rich, None), 2400);
        let compared = compare_hidden(family, &plain, &scouted, Some(hidden));
        assert!(
            compared >= 1200,
            "{family}: compared only to tick {compared}"
        );
    }
}

#[test]
fn seeds_vary_what_a_family_does() {
    for family in Family::ALL {
        let draws: Vec<String> = (0..6)
            .map(|seed| {
                Challenger::new(family, PlayerId(0), seed, &Scenario::skirmish())
                    .unwrap()
                    .parameters()
            })
            .collect();
        assert!(
            draws.iter().any(|d| *d != draws[0]),
            "{family} draws the same plan from every seed"
        );
        let scenario = passive_skirmish();
        let logs: Vec<_> = (0..4)
            .map(|seed| play(family, seed, &scenario, 3000).log)
            .collect();
        assert!(
            logs.iter().any(|log| *log != logs[0]),
            "{family} plays seeds 0 to 3 alike"
        );
    }
    // The forward Fabricator's distance and side vary where it stands.
    let scenario = deep_cut();
    let anchors: std::collections::BTreeSet<_> = (0..6)
        .filter_map(|seed| {
            play(Family::ForwardFabricator, seed, &scenario, 1600)
                .log
                .iter()
                .flat_map(|(_, commands)| commands)
                .find_map(|c| match c.command {
                    Command::Build {
                        kind: BuildingKind::Fabricator,
                        anchor,
                        ..
                    } => Some(anchor),
                    _ => None,
                })
        })
        .collect();
    assert!(anchors.len() >= 3, "forward Fabricators at {anchors:?}");
}

fn deep_cut() -> Scenario {
    let mut scenario = Scenario::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scenarios/the-deep-cut.json"
    ))
    .unwrap();
    for seat in &mut scenario.players {
        seat.bot = false;
        seat.bot_config = None;
    }
    scenario
}

#[test]
fn a_challenger_takes_over_a_seat_mid_game() {
    let mut scenario = skirmish_against_prime(2);
    scenario.players[0].bot = true;
    scenario.players[0].bot_config = Some(BotConfig::new(
        BotDifficulty::Standard,
        BotStance::Balanced,
        11,
    ));
    let mut state = scenario.build().unwrap();
    let mut bots = seat_controllers(&scenario).unwrap();
    while state.current_tick() < 3000 {
        let commands = aftermine_kit::bot_execution::commands(&state, &mut bots);
        let report = state.tick(&commands);
        record_events(&mut bots, &report);
    }
    bots.retain(|bot| bot.player() != PlayerId(0));
    for family in Family::ALL {
        let mut state = state.clone();
        let mut bots = bots.clone();
        let mut challenger = Challenger::new(family, PlayerId(0), 1, &scenario).unwrap();
        let (mut issued, mut rejected) = (0, 0);
        while state.current_tick() < 3600 && state.result().is_none() {
            let mine = challenger.act(&state);
            issued += mine.len();
            let mut commands = mine;
            commands.extend(aftermine_kit::bot_execution::commands(&state, &mut bots));
            let report = state.tick(&commands);
            record_events(&mut bots, &report);
            rejected += report
                .events
                .iter()
                .filter(|e| matches!(e, Event::CommandRejected { player, .. } if *player == PlayerId(0)))
                .count();
        }
        assert!(issued > 0, "{family} sat still after taking over");
        assert!(
            rejected * 10 <= issued,
            "{family}: {rejected} of {issued} rejected"
        );
    }
}

#[test]
fn a_family_needs_a_hostile_starting_foundry() {
    let mut scenario = Scenario::skirmish();
    scenario.players[1].team = Some(0);
    scenario.players[0].team = Some(0);
    assert!(Challenger::new(Family::BallRush, PlayerId(0), 0, &scenario).is_err());
}
