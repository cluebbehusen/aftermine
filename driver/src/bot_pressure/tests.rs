use super::*;
use aftermine_sim::UnitKind;
use aftermine_sim::scenario::UnitSpec;

/// The shipped files' own defender, as authored.
const AUTHORED: Variant = Variant {
    difficulty: BotDifficulty::Standard,
    seed: 0,
    mirrored: false,
};

fn shipped() -> Vec<PressureScenario> {
    load_all(&Path::new(env!("CARGO_MANIFEST_DIR")).join("evaluation/pressure")).unwrap()
}

fn named(name: &str) -> PressureScenario {
    shipped()
        .into_iter()
        .find(|pressure| pressure.name == name)
        .unwrap()
}

/// Unit ids a scripted command names, from its serialized form.
fn named_units(command: &Command) -> Vec<u64> {
    let value = serde_json::to_value(command).unwrap();
    let mut ids: Vec<u64> = value["units"]
        .as_array()
        .map(|units| units.iter().filter_map(serde_json::Value::as_u64).collect())
        .unwrap_or_default();
    ids.extend(value["transport"].as_u64());
    ids
}

#[test]
fn shipped_scenarios_script_only_the_attackers_units() {
    let scenarios = shipped();
    assert_eq!(scenarios.len(), 5);
    for pressure in &scenarios {
        pressure.scenario.build().unwrap();
        let seats = &pressure.scenario.players;
        assert!(
            seats[usize::from(pressure.defender)].bot,
            "{}",
            pressure.name
        );
        assert!(
            !seats[usize::from(pressure.attacker)].bot,
            "{}",
            pressure.name
        );
        assert!(!pressure.script.is_empty(), "{}", pressure.name);
        for scripted in &pressure.script {
            assert!(scripted.tick < pressure.deadline, "{}", pressure.name);
            for id in named_units(&scripted.command) {
                let unit = pressure.scenario.units[usize::try_from(id).unwrap()];
                assert_eq!(
                    unit.player, pressure.attacker,
                    "{} unit {id}",
                    pressure.name
                );
            }
        }
    }
}

#[test]
fn anti_air_means_a_dedicated_unit_or_a_built_flak_turret() {
    let pressure = named("air switch");
    let state = pressure.scenario.build().unwrap();
    let defender = PlayerId(pressure.defender);
    assert!(
        !owns_anti_air(&state, defender),
        "Sentinels alone do not count"
    );
    let mut armed = pressure.scenario.clone();
    armed.units.push(UnitSpec {
        player: pressure.defender,
        kind: UnitKind::Flakhound,
        x: 12,
        y: 10,
    });
    assert!(owns_anti_air(&armed.build().unwrap(), defender));
}

#[test]
fn a_standing_foundry_passes_and_an_unstaged_threat_is_an_error() {
    let pressure = named("early rush");
    let state = pressure.scenario.build().unwrap();
    let (defender, attacker) = (PlayerId(pressure.defender), PlayerId(pressure.attacker));
    let (passed, _) = Watch::new(Check::KeepsFoundry, &state, defender)
        .verdict(&state, defender, attacker)
        .unwrap();
    assert!(passed);
    assert!(
        Watch::new(Check::ClearsLanding, &state, defender)
            .verdict(&state, defender, attacker)
            .is_err()
    );
    assert!(
        Watch::new(Check::AnswersAir { within: 1 }, &state, defender)
            .verdict(&state, defender, attacker)
            .is_err()
    );
}

#[test]
fn a_carrier_downed_before_landing_passes_and_one_still_flying_is_an_error() {
    let pressure = named("lift drop");
    let state = pressure.scenario.build().unwrap();
    let (defender, attacker) = (PlayerId(pressure.defender), PlayerId(pressure.attacker));
    let skyhook = state
        .units()
        .iter()
        .find(|unit| unit.kind == UnitKind::Skyhook)
        .unwrap()
        .id;
    let mut watch = Watch::new(Check::ClearsLanding, &state, defender);
    watch.carriers.push(skyhook);
    assert!(watch.verdict(&state, defender, attacker).is_err());
    watch.carriers = vec![UnitId(u32::MAX)];
    let (passed, detail) = watch.verdict(&state, defender, attacker).unwrap();
    assert!(passed, "{detail}");
}

#[test]
fn a_recorded_replay_lasts_until_the_run_stopped() {
    let mut pressure = named("early rush");
    pressure.deadline = 36;
    let (outcome, replay) = run(&pressure, AUTHORED, true).unwrap();
    assert_eq!(outcome.ticks, 36);
    assert_eq!(replay.unwrap().meta.ticks, Some(36));
}

#[test]
fn a_bot_attacker_or_a_defender_without_a_bot_is_refused() {
    let mut pressure = named("early rush");
    pressure.scenario.players[usize::from(pressure.attacker)].bot = true;
    assert!(run(&pressure, AUTHORED, false).is_err());
    let mut pressure = named("early rush");
    pressure.scenario.players[usize::from(pressure.defender)].bot_config = None;
    assert!(run(&pressure, AUTHORED, false).is_err());
}

#[test]
fn only_a_foundry_that_stood_built_at_the_start_counts() {
    let pressure = named("breach");
    let mut state = pressure.scenario.build().unwrap();
    let (defender, attacker) = (PlayerId(pressure.defender), PlayerId(pressure.attacker));
    let start = Watch::new(Check::KeepsFoundry, &state, defender);
    assert_eq!(start.foundries.len(), 1);
    let harvester = state
        .units()
        .iter()
        .find(|unit| unit.player == defender && unit.kind == UnitKind::Harvester)
        .unwrap()
        .id;
    state.tick(&[PlayerCommand {
        player: defender,
        command: Command::Build {
            units: vec![harvester],
            kind: BuildingKind::Foundry,
            anchor: TilePos::new(14, 15),
            queue: false,
            defer: false,
        },
    }]);
    assert!(
        state
            .buildings()
            .iter()
            .any(|building| building.player == defender
                && building.kind == BuildingKind::Foundry
                && !building.built()),
        "the defender placed a Foundry site"
    );
    assert_eq!(
        Watch::new(Check::KeepsFoundry, &state, defender).foundries,
        start.foundries,
        "an unfinished site is not a starting Foundry"
    );
    let (passed, _) = start.verdict(&state, defender, attacker).unwrap();
    assert!(passed, "the starting Foundry still stands");

    // Once the starting Foundry is gone, neither a site nor a finished
    // Foundry placed after the start keeps the base.
    let mut gone = Watch::new(Check::KeepsFoundry, &state, defender);
    gone.foundries = vec![BuildingId(u32::MAX)];
    let (passed, detail) = gone.verdict(&state, defender, attacker).unwrap();
    assert!(!passed, "{detail}");
    assert_eq!(detail, "the starting Foundry fell");
}

#[test]
fn variants_cover_every_difficulty_seed_and_orientation() {
    use BotDifficulty::{Prime, Scrapheap};
    let grid = variants(&[Prime, Scrapheap], 4..7, true);
    assert_eq!(grid.len(), 2 * 3 * 2);
    assert_eq!(
        grid[..3],
        [
            Variant {
                difficulty: Prime,
                seed: 4,
                mirrored: false,
            },
            Variant {
                difficulty: Prime,
                seed: 4,
                mirrored: true,
            },
            Variant {
                difficulty: Prime,
                seed: 5,
                mirrored: false,
            },
        ]
    );
    assert!(
        variants(&[Prime], 0..3, false)
            .iter()
            .all(|variant| !variant.mirrored)
    );
    assert_eq!(
        grid[1].replay_name("early rush"),
        "early-rush-prime-seed4-mirrored.json"
    );
}

#[test]
fn a_variant_sets_the_defender_and_keeps_the_stance() {
    use aftermine_sim::scenario::{BotConfig, BotStance};
    let mut pressure = named("siege");
    let seat = usize::from(pressure.defender);
    pressure.scenario.players[seat].bot_config = Some(BotConfig::new(
        BotDifficulty::Standard,
        BotStance::Turtle,
        0,
    ));
    let staged = pressure
        .staged(Variant {
            difficulty: BotDifficulty::Prime,
            seed: 7,
            mirrored: false,
        })
        .unwrap();
    assert_eq!(
        staged.scenario.players[seat].bot_config,
        Some(BotConfig::new(BotDifficulty::Prime, BotStance::Turtle, 7))
    );
    assert_eq!(staged.scenario.map, pressure.scenario.map);
}

#[test]
fn mirroring_reflects_the_staging_and_twice_gives_it_back() {
    for pressure in shipped() {
        let mirrored = pressure.mirrored().unwrap();
        assert_ne!(mirrored, pressure, "{}", pressure.name);
        assert_eq!(mirrored.mirrored().unwrap(), pressure, "{}", pressure.name);
        let width = i32::try_from(pressure.scenario.map[0].len()).unwrap();
        let (authored, reflected) = (
            pressure.scenario.build().unwrap(),
            mirrored.scenario.build().unwrap(),
        );
        assert_eq!(authored.buildings().len(), reflected.buildings().len());
        for (before, after) in authored.buildings().iter().zip(reflected.buildings()) {
            assert_eq!(
                (after.id, after.kind, after.player, after.built()),
                (before.id, before.kind, before.player, before.built())
            );
            assert_eq!(
                after.anchor,
                TilePos::new(
                    width - before.kind.size().0 - before.anchor.x,
                    before.anchor.y
                ),
                "{} {:?}",
                pressure.name,
                before.kind
            );
        }
        assert_eq!(authored.units().len(), reflected.units().len());
        for (before, after) in authored.units().iter().zip(reflected.units()) {
            assert_eq!((after.id, after.kind), (before.id, before.kind));
            let tile = before.tile();
            assert_eq!(after.tile(), TilePos::new(width - 1 - tile.x, tile.y));
        }
    }
    let breach = named("breach").mirrored().unwrap();
    assert_eq!(
        breach.script[0].command,
        Command::Hunt {
            units: vec![UnitId(10)],
            goal: TilePos::new(33, 11),
            queue: false,
        }
    );
    let air = named("air switch").mirrored().unwrap();
    let Command::Patrol { waypoints, .. } = &air.script[0].command else {
        panic!("{:?}", air.script[0].command);
    };
    assert_eq!(waypoints, &[TilePos::new(29, 4), TilePos::new(36, 6)]);
    let drop = named("lift drop").mirrored().unwrap();
    assert!(matches!(
        drop.script[1].command,
        Command::Unload { at, .. } if at == TilePos::new(32, 15)
    ));
}

#[test]
fn a_script_command_that_cannot_be_mirrored_is_refused() {
    let mut pressure = named("early rush");
    pressure.script.push(Scripted {
        tick: 1,
        command: Command::CancelOrder {
            unit: UnitId(6),
            key: aftermine_sim::OrderKey::Walk {
                tile: TilePos::new(6, 11),
            },
            from_end: 0,
            units: Vec::new(),
        },
    });
    let error = format!("{:#}", pressure.mirrored().unwrap_err());
    assert!(error.contains("cancel_order"), "{error}");
    assert!(
        run(
            &pressure,
            Variant {
                mirrored: true,
                ..AUTHORED
            },
            false
        )
        .is_err()
    );
}

#[test]
fn the_report_counts_passes_by_difficulty_and_orientation() {
    let outcome = |difficulty, mirrored, passed| PressureOutcome {
        name: "siege".into(),
        check: Check::SilencesArtillery,
        variant: Variant {
            difficulty,
            seed: 0,
            mirrored,
        },
        passed,
        detail: if passed {
            "no artillery in range of the base".into()
        } else {
            "the starting Foundry fell".into()
        },
        ticks: 3_600,
    };
    let text = report(&[
        outcome(BotDifficulty::Standard, false, true),
        outcome(BotDifficulty::Standard, true, false),
        outcome(BotDifficulty::Prime, false, true),
        outcome(BotDifficulty::Prime, true, true),
    ]);
    let row = text.lines().find(|line| line.starts_with("siege")).unwrap();
    let cells: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(
        cells,
        [
            "siege",
            "silences_artillery",
            "3/4",
            "[30-95%]",
            "1/2",
            "2/2",
            "2/2",
            "1/2"
        ],
        "{text}"
    );
    assert!(
        text.contains("standard seed 0 mirrored") && text.contains("the starting Foundry fell"),
        "{text}"
    );
    assert!(text.ends_with("3/4 runs passed\n"), "{text}");
}
