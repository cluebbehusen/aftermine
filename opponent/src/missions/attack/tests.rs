use super::*;
use aftermine_sim::{PlayerId, Scenario};

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
    use aftermine_sim::scenario::{BotConfig, BotStance};
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

/// A Standard seat's view of Skirmish with no enemy building known, its
/// map and its frame.
fn skirmish() -> (ObservationData, MapModel, HomeFrame) {
    let scenario = Scenario::skirmish();
    let state = scenario.build().unwrap();
    let map = MapModel::from_scenario(&scenario).unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    observation.enemy_buildings.clear();
    let frame = HomeFrame::of(&observation, &map).unwrap();
    (observation, map, frame)
}

fn scratch(observation: &ObservationData, map: &MapModel, frame: HomeFrame) -> Scratch {
    Scratch::new(
        observation,
        map,
        frame,
        &Memory::default(),
        BotStance::Balanced,
        crate::tests::middling(),
        &Missions::default(),
    )
}

#[test]
fn a_start_scouting_sees_later_in_the_decision_is_no_longer_presumed_held() {
    let (mut observation, map, frame) = skirmish();
    observation.tick = 600;
    let profile = ResolvedProfile::resolve(aftermine_sim::scenario::BotConfig::default());
    let scratch = scratch(&observation, &map, frame);
    let mut memory = Memory::default();
    let start = map.start(PlayerId(1));
    let best = |memory: &Memory| {
        Plan::new(&observation, &map, &profile, memory, &scratch)
            .best(None)
            .map(|target| target.anchor)
    };
    assert_eq!(best(&memory), start, "premise: the unseen start is held");
    memory.scouted(super::super::points(&map, observation.me).len());
    memory.saw(0, observation.tick);
    assert_eq!(best(&memory), None);
}

/// [`Plan::need_at`] as it walked the way to every target.
fn need_walking_the_way(plan: &Plan<'_>, target: Target, breach: u64) -> u64 {
    let (units, mut buildings) =
        opposed_parts(plan.observation, plan.map, plan.memory, &[target.approach]);
    if surveys(plan.difficulty) {
        buildings = plan
            .on_the_way(target, &plan.way(target))
            .into_iter()
            .map(building_value)
            .sum();
    }
    ((units + buildings.saturating_sub(breach)) * plan.misjudge / 1_000 * plan.margin / 1_000)
        .max(plan.minimum)
}

#[test]
fn whether_an_army_meets_a_need_never_hangs_on_skipping_the_way_there() {
    let (mut observation, map, frame) = skirmish();
    let (width, height) = map.size();
    let mut rng = Pcg32::new(3, 0);
    let mut draw = |bound: i32| i32::try_from(rng.next_below(bound.unsigned_abs())).unwrap();
    let memory = Memory::default();
    // Needs the way raised over the defenses around the target, and needs
    // under every known defense.
    let mut open = [0; 2];
    for difficulty in [BotDifficulty::Scrapheap, BotDifficulty::Standard] {
        let profile = ResolvedProfile::resolve(aftermine_sim::scenario::BotConfig::new(
            difficulty,
            BotStance::Balanced,
            0,
        ));
        for _ in 0..20 {
            let template = BuildingObs {
                player: PlayerId(1),
                kind: BuildingKind::Turret,
                hp: BuildingKind::Turret.base_stats().max_hp,
                built: true,
                ..observation.my_buildings[0].clone()
            };
            observation.enemy_buildings = (0..12)
                .map(|_| BuildingObs {
                    anchor: TilePos::new(draw(width), draw(height)),
                    ..template.clone()
                })
                .collect();
            let scratch = scratch(&observation, &map, frame);
            let plan = Plan::new(&observation, &map, &profile, &memory, &scratch);
            for building in &observation.enemy_buildings {
                let Some(target) = plan.target(building.player, building.kind, building.anchor)
                else {
                    continue;
                };
                for breach in [0, 150] {
                    let need = need_walking_the_way(&plan, target, breach);
                    assert_eq!(plan.need_at(target, breach, plan.margin), need);
                    let (units, sure, beyond) = plan.opposing(target);
                    let most: u64 = beyond.iter().map(|building| building_value(building)).sum();
                    open[0] += usize::from(plan.scaled(units, sure, breach, plan.margin) < need);
                    open[1] +=
                        usize::from(need < plan.scaled(units, sure + most, breach, plan.margin));
                    for strength in (0..need + 400).step_by(50).chain([need - 1, need]) {
                        assert_eq!(
                            plan.meets(strength, target, breach, plan.margin),
                            strength >= need,
                            "{difficulty:?}, {strength} against {need} at {:?}",
                            target.anchor
                        );
                    }
                }
            }
        }
    }
    assert!(open.iter().all(|cases| *cases > 0), "{open:?}");
}
