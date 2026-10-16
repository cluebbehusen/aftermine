use super::*;
use oxide_sim::{PlayerId, Scenario};

#[test]
fn defenders_around_several_tiles_count_once() {
    let state = Scenario::skirmish().build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let turret = BuildingObs {
        player: PlayerId(1),
        kind: BuildingKind::Turret,
        anchor: TilePos::new(20, 10),
        hp: BuildingKind::Turret.base_stats().max_hp,
        built: true,
        ..observation.my_buildings[0].clone()
    };
    observation.enemy_buildings = vec![turret];
    let memory = Memory::default();
    let (a, b) = (TilePos::new(18, 10), TilePos::new(22, 10));
    let one = defense(&observation, &memory, a);
    assert_eq!(one, 100, "premise: the Turret guards each tile");
    assert_eq!(defense(&observation, &memory, b), one);
    assert_eq!(defense_around(&observation, &memory, &[a, b]), one);
}

#[test]
fn guile_gives_a_fight_up_sooner_and_the_upper_rungs_hold_on_longer() {
    use oxide_sim::scenario::{BotConfig, BotStance};
    let threshold = |difficulty: BotDifficulty, guile: u8| {
        let mut profile =
            ResolvedProfile::resolve(BotConfig::new(difficulty, BotStance::Balanced, 0));
        profile.traits.guile = guile;
        outweighed(&profile)
    };
    for difficulty in [
        BotDifficulty::Scrapheap,
        BotDifficulty::Standard,
        BotDifficulty::Veteran,
        BotDifficulty::Prime,
    ] {
        assert!(threshold(difficulty, 100) < threshold(difficulty, 50));
        assert!(threshold(difficulty, 50) < threshold(difficulty, 0));
    }
    for guile in [0, 50, 100] {
        assert!(threshold(BotDifficulty::Prime, guile) > threshold(BotDifficulty::Standard, guile));
    }
}
