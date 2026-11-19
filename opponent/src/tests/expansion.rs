use super::*;
use crate::expansion;
use crate::investments::{ADOPT, Investment};
use crate::map::Site;
use crate::memory::Memory;

/// A half-turn-symmetric frontier. Each seat has two home nodes, a small safe
/// field eight tiles out, and a rich field beside the other seat's start.
pub(super) const FRONTIER: [&str; 14] = [
    "########################################",
    "############...........................#",
    "#...ss......SS............ss...........#",
    "#...........SS............ss...........#",
    "#......................................#",
    "#......................................#",
    "#.1.................................2..#",
    "#......................................#",
    "#......................................#",
    "#......................................#",
    "#...........ss............SS...........#",
    "#...........ss............SS......ss...#",
    "#...........................############",
    "########################################",
];

fn frontier(scrap: u32) -> Scenario {
    let mut scenario = arena(scrap);
    scenario.map = FRONTIER.map(str::to_owned).to_vec();
    scenario.units = vec![
        harvester(0, 4, 3),
        harvester(0, 5, 3),
        harvester(1, 35, 10),
        harvester(1, 34, 10),
    ];
    scenario
}

fn rotate(tile: TilePos) -> TilePos {
    TilePos::new(40 - 1 - tile.x, 14 - 1 - tile.y)
}

/// The site whose field holds `node`.
fn site_with(model: &MapModel, node: TilePos) -> (usize, &Site) {
    model
        .sites()
        .iter()
        .enumerate()
        .find(|(_, site)| site.nodes.iter().any(|(tile, _)| *tile == node))
        .expect("a site holds the node")
}

#[test]
fn sites_and_distances_mirror_between_seats() {
    let model = map(&frontier(0));
    assert!(model.sites().len() >= 4, "home fields are not sites");
    for site in model.sites() {
        let (_, mirror) = site_with(&model, rotate(site.nodes[0].0));
        let rotated: Vec<TilePos> = site
            .anchors
            .iter()
            .map(|anchor| TilePos::new(40 - 2 - anchor.x, 14 - 2 - anchor.y))
            .collect();
        assert_eq!(rotated, mirror.anchors);
        assert!(!site.anchors.is_empty());
    }
    for y in 0..14 {
        for x in 0..40 {
            let tile = TilePos::new(x, y);
            assert_eq!(
                model.distance(PlayerId(0), tile),
                model.distance(PlayerId(1), rotate(tile)),
                "{tile:?}"
            );
        }
    }
}

#[test]
fn greed_trades_a_safe_field_for_a_richer_contested_one() {
    let scenario = frontier(0);
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    let observation = mapped(&state, PlayerId(0));
    let memory = Memory::default();
    let (_, safe) = site_with(&model, TilePos::new(12, 10));
    let (_, rich) = site_with(&model, TilePos::new(26, 10));
    let value =
        |site: &Site, greed| expansion::value(&observation, &model, &memory, site, greed).unwrap();
    assert!(value(safe, 0) > value(rich, 0));
    assert!(value(rich, 100) > value(safe, 100));
}

#[test]
fn a_site_that_failed_at_every_anchor_is_skipped() {
    let scenario = frontier(0);
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    let observation = mapped(&state, PlayerId(0));
    let (_, safe) = site_with(&model, TilePos::new(12, 10));
    let mut memory = Memory::default();
    for anchor in &safe.anchors[1..] {
        memory.fail(BuildingKind::Foundry, *anchor, 0);
    }
    assert!(expansion::value(&observation, &model, &memory, safe, 50).is_some());
    memory.fail(BuildingKind::Foundry, safe.anchors[0], 0);
    assert_eq!(
        expansion::value(&observation, &model, &memory, safe, 50),
        None
    );
}

#[test]
fn an_unexplored_expansion_sends_a_harvester_to_look() {
    let mut scenario = frontier(1_000);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Fabricator,
        x: 6,
        y: 8,
    });
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let (index, far) = site_with(&model, TilePos::new(26, 10));
    assert!(
        far.anchors
            .iter()
            .all(|anchor| !observation.explored(*anchor)),
        "premise: the far field is in fog"
    );
    let nearer: Vec<serde_json::Value> = [TilePos::new(12, 2), TilePos::new(12, 10)]
        .into_iter()
        .flat_map(|node| site_with(&model, node).1.anchors.clone())
        .map(|anchor| serde_json::json!({"kind": "foundry", "anchor": anchor, "at": 0}))
        .collect();
    let mut checkpoint = serde_json::to_value(seat(&scenario, 0).checkpoint()).unwrap();
    checkpoint["memory"]["failures"] = serde_json::Value::Array(nearer);
    checkpoint["saving"] = serde_json::json!({
        "protected": 0,
        "target": {"investment": {"expansion": index}, "attempt": null},
    });
    let checkpoint: Checkpoint = serde_json::from_value(checkpoint).unwrap();
    let mut opponent = Opponent::restore(&checkpoint, &scenario, &state, model.clone()).unwrap();
    let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
    assert_eq!(
        trace.unwrap().target.map(|target| target.investment),
        Some(Investment::Expansion(u16::try_from(index).unwrap()))
    );
    assert!(builds_of(&commands).is_empty(), "nothing to place in fog");
    assert!(
        commands.iter().any(|command| matches!(
            &command.command,
            Command::Run { goal, .. } if *goal == far.anchors[0]
        )),
        "{commands:?}"
    );
}

fn builds_of(commands: &[PlayerCommand]) -> Vec<BuildingKind> {
    builds(commands).into_iter().map(|(kind, _)| kind).collect()
}

#[test]
fn harvesters_work_the_field_of_a_new_foundry() {
    let mut scenario = frontier(0);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 14,
        y: 8,
    });
    scenario.units = vec![harvester(0, 14, 11), harvester(0, 15, 11)];
    let state = scenario.build().unwrap();
    let commands = seat(&scenario, 0).act(&state, &mut OwnEvents::default());
    let field = [
        TilePos::new(12, 10),
        TilePos::new(13, 10),
        TilePos::new(12, 11),
        TilePos::new(13, 11),
    ];
    let sent: Vec<TilePos> = harvests(&commands)
        .into_iter()
        .map(|(node, _)| node)
        .collect();
    assert_eq!(sent.len(), 2, "{commands:?}");
    assert!(sent.iter().all(|node| field.contains(node)), "{sent:?}");
}

#[test]
fn a_free_frame_near_home_gets_an_extractor_at_every_rung() {
    let mut scenario = arena(1_000);
    scenario.map[1].replace_range(6..7, "E");
    scenario
        .units
        .extend([harvester(0, 5, 7), harvester(0, 4, 7)]);
    let frame = TilePos::new(6, 1);
    for difficulty in BotDifficulty::ALL {
        let mut state = scenario.build().unwrap();
        let config = rung(difficulty);
        let mut opponent = seat_with(&scenario, 0, config);
        let mut events = OwnEvents::default();
        let mut placed = false;
        while state.current_tick() < 600 && !placed {
            let commands = opponent.act(&state, &mut events);
            placed = commands.iter().any(|command| {
                matches!(
                    command.command,
                    Command::Build { kind: BuildingKind::Extractor, anchor, .. } if anchor == frame
                )
            });
            events.record(PlayerId(0), &state.tick(&commands).events);
        }
        assert!(placed, "{difficulty:?}");
    }
}

#[test]
fn only_frames_the_seat_has_seen_add_to_a_site() {
    let mut scenario = frontier(0);
    scenario.map[9].replace_range(16..18, "E.");
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    let (_, safe) = site_with(&model, TilePos::new(12, 10));
    assert!(safe.frames.contains(&TilePos::new(16, 9)), "premise");
    let mut observation = mapped(&state, PlayerId(0));
    let memory = Memory::default();
    let seen = expansion::value(&observation, &model, &memory, safe, 50).unwrap();
    observation.known_frames.clear();
    let hidden = expansion::value(&observation, &model, &memory, safe, 50).unwrap();
    assert!(seen > hidden);
}

#[test]
fn a_site_keeps_anchors_on_its_resources_ground_and_can_be_reached_by_lift() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../scenarios/severance.json");
    let scenario = Scenario::load(path).unwrap();
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    for (player, node) in [
        (PlayerId(0), TilePos::new(30, 21)),
        (PlayerId(1), TilePos::new(33, 22)),
    ] {
        let observation = mapped(&state, player);
        let (index, site) = site_with(&model, node);
        let ground = model.component(node);
        let reachable = expansion::anchors(&model, player, site);
        assert!(!reachable.is_empty(), "{player:?}");
        assert!(
            reachable
                .iter()
                .all(|anchor| model.component(*anchor) == ground)
        );
        assert!(expansion::value(&observation, &model, &Memory::default(), site, 50).is_some());
        let placed: Vec<TilePos> = crate::investments::anchors(
            &model,
            &observation,
            Investment::Expansion(u16::try_from(index).unwrap()),
            BuildingKind::Foundry,
        )
        .collect();
        assert_eq!(placed, reachable);
        let other = PlayerId(1 - player.0);
        assert_ne!(
            model.start(other).and_then(|start| model.component(start)),
            ground
        );
        assert!(!expansion::anchors(&model, other, site).is_empty());
    }
}

#[test]
fn a_half_explored_footprint_is_still_scouted() {
    let mut scenario = frontier(1_000);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Fabricator,
        x: 6,
        y: 8,
    });
    let model = map(&scenario);
    let state = scenario.build().unwrap();
    let (index, far) = site_with(&model, TilePos::new(26, 10));
    let nearer: Vec<serde_json::Value> = [TilePos::new(12, 2), TilePos::new(12, 10)]
        .into_iter()
        .flat_map(|node| site_with(&model, node).1.anchors.clone())
        .map(|anchor| serde_json::json!({"kind": "foundry", "anchor": anchor, "at": 0}))
        .collect();
    let mut checkpoint = serde_json::to_value(seat(&scenario, 0).checkpoint()).unwrap();
    checkpoint["memory"]["failures"] = serde_json::Value::Array(nearer);
    checkpoint["saving"] = serde_json::json!({
        "protected": 0,
        "target": {"investment": {"expansion": index}, "attempt": null},
    });
    let checkpoint: Checkpoint = serde_json::from_value(checkpoint).unwrap();
    let opponent = Opponent::restore(&checkpoint, &scenario, &state, model.clone()).unwrap();

    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let anchor = far.anchors[0];
    let index_of =
        |tile: TilePos| usize::try_from(tile.y * observation.map_width + tile.x).unwrap();
    let explored = index_of(anchor);
    observation.explored[explored] = true;
    let mut persistent = opponent.persistent.clone();
    let decision = crate::decision::decide(
        &observation,
        &[],
        &model,
        home_frame(&observation, &model),
        &opponent.profile,
        &mut persistent,
        false,
    );
    assert!(
        decision.commands.iter().any(|command| matches!(
            &command.command,
            Command::Run { goal, .. } if *goal == anchor
        )),
        "{:?}",
        decision.commands
    );
}

#[test]
fn a_lost_extractor_is_rebuilt_a_while_after_its_attacker_is_gone() {
    let mut scenario = arena(1_000);
    scenario.map[1].replace_range(6..7, "E");
    scenario.units.extend([
        harvester(0, 5, 7),
        harvester(0, 4, 7),
        unit(1, UnitKind::Sentinel, 8, 2),
    ]);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Extractor,
        x: 6,
        y: 1,
    });
    let frame = TilePos::new(6, 1);
    let state = scenario.build().unwrap();
    let raider = at(&state, 8, 2);
    let extractor = state
        .buildings()
        .iter()
        .find(|building| building.kind == BuildingKind::Extractor)
        .unwrap()
        .id;
    let mut value = serde_json::to_value(&state).unwrap();
    for building in value["buildings"].as_array_mut().unwrap() {
        if building["id"] == extractor.0 {
            building["hp"] = 1.into();
        }
    }
    let mut state: State = serde_json::from_value(value).unwrap();
    let mut opponent = seat(&scenario, 0);
    let mut events = OwnEvents::default();
    let builds_frame = |commands: &[PlayerCommand]| {
        commands.iter().any(|command| {
            matches!(
                command.command,
                Command::Build { kind: BuildingKind::Extractor, anchor, .. } if anchor == frame
            )
        })
    };
    let attack = PlayerCommand {
        player: PlayerId(1),
        command: Command::Attack {
            units: vec![raider],
            target: AttackTarget::Building(extractor),
            queue: false,
        },
    };
    let mut lost = false;
    let mut held = 0;
    let mut rebuilt = false;
    let mut gone_at = None;
    while state.current_tick() < 8_000 && !rebuilt {
        let mut commands = opponent.act(&state, &mut events);
        let present = state.units().iter().any(|unit| unit.id == raider);
        if lost && present {
            assert!(
                !builds_frame(&commands),
                "rebuilt while its attacker stands beside the frame"
            );
            held += usize::from(!commands.is_empty());
        }
        if lost && !present {
            gone_at.get_or_insert(state.current_tick());
        }
        let ordered = lost && !present && builds_frame(&commands);
        if state.current_tick() == 0 {
            commands.push(attack.clone());
        }
        lost |= state.building(extractor).is_none();
        let report = state.tick(&commands);
        // The order counts only if the simulation took it.
        rebuilt = ordered
            && !report.events.iter().any(|event| {
                matches!(
                    event,
                    Event::CommandRejected {
                        player: PlayerId(0),
                        ..
                    }
                )
            });
        events.record(PlayerId(0), &report.events);
    }
    assert!(lost, "premise: the Extractor falls");
    assert!(held > 0, "premise: the seat acts while the attacker stays");
    assert!(
        rebuilt,
        "a new Extractor goes on the frame once the attacker is gone"
    );
    // Raiders that killed it may be waiting out of sight: the frame rests
    // for a while first.
    let waited = state.current_tick() - gone_at.unwrap();
    assert!(
        waited > 1_200,
        "rebuilt {waited} ticks after the attacker left"
    );
}

/// The arena with a Crucible and a completed Extractor on a frame at `x`
/// in row 5, the home Foundry's row.
fn bore_arena(x: i32) -> Scenario {
    let mut scenario = arena(1_000);
    let column = usize::try_from(x).unwrap();
    scenario.map[5].replace_range(column..=column, "E");
    scenario.buildings.extend([
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Crucible,
            x: 1,
            y: 8,
        },
        BuildingSpec {
            player: 0,
            kind: BuildingKind::Extractor,
            x,
            y: 5,
        },
    ]);
    scenario
}

fn own_extractor(state: &State) -> BuildingId {
    state
        .buildings()
        .iter()
        .find(|building| building.player == PlayerId(0) && building.kind == BuildingKind::Extractor)
        .unwrap()
        .id
}

#[test]
fn foundry_support_reads_the_simulation_reach() {
    // The home Foundry spans columns 3 and 4: an Extractor at column 12 is
    // eight tiles away, at 13 nine.
    for x in 11..=14 {
        let state = bore_arena(x).build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        assert_eq!(
            expansion::supported(&observation, TilePos::new(x, 5)),
            state
                .extractor_income(own_extractor(&state))
                .unwrap()
                .is_supported(),
            "column {x}"
        );
    }
}

#[test]
fn a_remote_extractor_is_never_deepened() {
    let state = bore_arena(13).build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let deep_bore = Investment::Upgrade {
        building: own_extractor(&state),
        tier: 1,
    };
    let model = map(&bore_arena(13));
    let frame = home_frame(&observation, &model);
    assert_eq!(
        crate::investments::step(&observation, frame, deep_bore),
        None
    );
}

#[test]
fn a_supported_extractor_becomes_a_deep_bore_once_a_crucible_stands_at_every_rung() {
    let scenario = bore_arena(12);
    for difficulty in BotDifficulty::ALL {
        let mut state = scenario.build().unwrap();
        let extractor = own_extractor(&state);
        // Deep Bores wait for harvesting to saturate and home scrap to run
        // down.
        let config = rung(difficulty);
        let mut opponent = seat_with(&scenario, 0, config);
        let mut events = OwnEvents::default();
        let mut upgraded = false;
        while state.current_tick() < 6_000 && !upgraded {
            let commands = opponent.act(&state, &mut events);
            upgraded = commands.iter().any(|command| {
                matches!(command.command, Command::UpgradeBuilding { building } if building == extractor)
            });
            let report = state.tick(&commands);
            assert!(
                !report.events.iter().any(|event| matches!(
                    event,
                    Event::CommandRejected {
                        reason: RejectReason::MissingFoundrySupport,
                        ..
                    }
                )),
                "{difficulty:?} tick {}",
                report.tick
            );
            events.record(PlayerId(0), &report.events);
        }
        assert!(upgraded, "{difficulty:?}");
        assert_eq!(state.building(extractor).unwrap().tier, 1, "{difficulty:?}");
    }
}

#[test]
fn a_deep_bore_wanted_before_a_crucible_stands_builds_the_crucible_first() {
    let mut scenario = bore_arena(12);
    scenario
        .buildings
        .retain(|building| building.kind != BuildingKind::Crucible);
    for difficulty in BotDifficulty::ALL {
        let mut state = scenario.build().unwrap();
        let extractor = own_extractor(&state);
        let deep_bore = Investment::Upgrade {
            building: extractor,
            tier: 1,
        };
        let config = rung(difficulty);
        let mut opponent = seat_with(&scenario, 0, config);
        let mut events = OwnEvents::default();
        let mut pulled = None;
        while state.current_tick() < 12_000 && pulled.is_none() {
            let (commands, trace) = opponent.act_traced(&state, &mut events);
            if let Some(trace) = trace
                && trace.purchases.iter().any(|purchase| {
                    matches!(
                        purchase,
                        crate::Purchase::Build {
                            building: BuildingKind::Crucible,
                            ..
                        }
                    )
                })
            {
                pulled = Some(trace.target.map(|target| target.investment));
            }
            let report = state.tick(&commands);
            events.record(PlayerId(0), &report.events);
        }
        assert_eq!(
            pulled,
            Some(Some(deep_bore)),
            "{difficulty:?} builds its Crucible toward the Deep Bore"
        );
    }
}

#[test]
fn a_crucible_makes_a_site_worth_more_for_the_remote_extractor_it_would_deepen() {
    let frame = TilePos::new(16, 9);
    let mut scenario = frontier(0);
    scenario.map[9].replace_range(16..18, "E.");
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Extractor,
        x: frame.x,
        y: frame.y,
    });
    let model = map(&scenario);
    let (_, site) = site_with(&model, TilePos::new(12, 10));
    let anchor = expansion::anchors(&model, PlayerId(0), site)[0];
    assert!(
        aftermine_sim::geometry::foundry_supports(anchor, frame),
        "premise: a Foundry at the site would support the frame"
    );
    let memory = Memory::default();
    let worth = |scenario: &Scenario| {
        let state = scenario.build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        assert!(
            !expansion::supported(&observation, frame),
            "premise: remote"
        );
        expansion::value(&observation, &model, &memory, site, 50).unwrap()
    };
    let without = worth(&scenario);
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Crucible,
        x: 2,
        y: 9,
    });
    assert!(worth(&scenario) > without);
}

#[test]
fn a_lower_rung_wants_the_same_growth_in_the_same_order_but_less() {
    let mut scenario = frontier(0);
    scenario.map[4].replace_range(6..7, "E");
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let model = map(&scenario);
    let memory = Memory::default();
    let growth = |difficulty| -> Vec<(Investment, u32)> {
        crate::investments::candidates(&crate::investments::Situation {
            depletion: 500,
            economy: crate::economy(difficulty),
            ..calm(&observation, &model, &memory)
        })
        .into_iter()
        .filter(|candidate| {
            matches!(
                candidate.investment,
                Investment::Expansion(_) | Investment::Extractor(_) | Investment::Reclaimer
            )
        })
        .map(|candidate| (candidate.investment, candidate.score))
        .collect()
    };
    let rungs: Vec<Vec<(Investment, u32)>> = BotDifficulty::ALL.map(growth).to_vec();
    let investments =
        |growth: &[(Investment, u32)]| growth.iter().map(|(each, _)| *each).collect::<Vec<_>>();
    assert!(
        rungs[3].iter().all(|(_, score)| *score > ADOPT),
        "premise: every rung would adopt each: {rungs:?}"
    );
    for pair in rungs.windows(2) {
        assert_eq!(investments(&pair[0]), investments(&pair[1]));
        for ((_, lower), (_, higher)) in pair[0].iter().zip(&pair[1]) {
            assert!(ADOPT < *lower && lower < higher, "{rungs:?}");
        }
    }
}

/// A half-turn-symmetric open lane with one home node per seat and no field
/// far enough out to be a site, so frames alone make the sites.
const LANE: [&str; 14] = [
    "########################################",
    "#......................................#",
    "#......................................#",
    "#...s..................................#",
    "#......................................#",
    "#......................................#",
    "#.1....................................#",
    "#....................................2.#",
    "#......................................#",
    "#......................................#",
    "#..................................s...#",
    "#......................................#",
    "#......................................#",
    "########################################",
];

/// The lane with a frame at each of `frames`.
fn lane(frames: &[(i32, i32)]) -> Scenario {
    let mut scenario = arena(0);
    scenario.map = LANE.map(str::to_owned).to_vec();
    scenario.units.clear();
    for (x, y) in frames {
        let (x, y) = (usize::try_from(*x).unwrap(), usize::try_from(*y).unwrap());
        scenario.map[y].replace_range(x..=x, "E");
    }
    scenario
}

/// West's Extractor candidate scores by frame.
fn frame_scores(scenario: &Scenario) -> Vec<(TilePos, u32)> {
    let state = scenario.build().unwrap();
    let observation = mapped(&state, PlayerId(0));
    expansion::candidates(&observation, &map(scenario), &Memory::default(), 50, 0)
        .into_iter()
        .filter_map(|(investment, score)| match investment {
            Investment::Extractor(frame) => Some((frame, score)),
            _ => None,
        })
        .collect()
}

fn score_of(scores: &[(TilePos, u32)], x: i32, y: i32) -> u32 {
    scores
        .iter()
        .find(|(frame, _)| *frame == TilePos::new(x, y))
        .map(|(_, score)| *score)
        .expect("the frame is a candidate")
}

#[test]
fn frames_and_deep_bores_rank_by_what_they_pay_back() {
    // Beside West's Foundry, out on West's side, that frame's mirror on
    // East's side, and one at East's door.
    let frames = lane(&[(6, 4), (14, 7), (24, 5), (32, 5)]);
    let scores = frame_scores(&frames);
    let (supported, own_side, enemy_side, door) = (
        score_of(&scores, 6, 4),
        score_of(&scores, 14, 7),
        score_of(&scores, 24, 5),
        score_of(&scores, 32, 5),
    );
    assert!(supported > own_side, "{scores:?}");
    assert!(
        enemy_side < own_side,
        "a frame East reaches first is worth less: {scores:?}"
    );
    assert!(
        door * 2 < enemy_side,
        "and less the nearer East it lies: {scores:?}"
    );
    let bore = |crucible: bool| {
        let mut scenario = lane(&[(6, 4), (14, 7)]);
        scenario
            .buildings
            .push(building(0, BuildingKind::Extractor, 6, 4));
        if crucible {
            scenario
                .buildings
                .push(building(0, BuildingKind::Crucible, 8, 9));
        }
        let state = scenario.build().unwrap();
        let observation = mapped(&state, PlayerId(0));
        let model = map(&scenario);
        let memory = Memory::default();
        let candidates = crate::investments::candidates(&crate::investments::Situation {
            depletion: 500,
            ..calm(&observation, &model, &memory)
        });
        let score = |wanted: fn(&Investment) -> bool| {
            candidates
                .iter()
                .find(|candidate| wanted(&candidate.investment))
                .map(|candidate| candidate.score)
                .expect("the investment is a candidate")
        };
        (
            score(|investment| *investment == Investment::Extractor(TilePos::new(14, 7))),
            score(|investment| matches!(investment, Investment::Upgrade { tier: 1, .. })),
        )
    };
    let (frame, deep_bore) = bore(true);
    let (_, early) = bore(false);
    assert!(
        frame > deep_bore,
        "an Extractor returns more for its price than a Deep Bore"
    );
    assert!(
        deep_bore > early,
        "a Crucible still to buy is part of the first Deep Bore's price"
    );
}

/// A frames-only site of two frames in the lane's middle, with `foundries`
/// as owner and anchor, and a West Kestrel overhead so West sees them.
fn claimed_with(foundries: &[(u8, i32, i32)], allied: bool) -> bool {
    let mut scenario = lane(&[(17, 10), (21, 10)]);
    if allied {
        scenario.map[11].replace_range(2..3, "3");
        let ally = scenario.players[1].clone();
        scenario.players.push(ally);
        for (player, team) in scenario.players.iter_mut().zip([0, 1, 0]) {
            player.team = Some(team);
        }
    }
    scenario.units.push(unit(0, UnitKind::Kestrel, 20, 8));
    for (owner, x, y) in foundries {
        scenario
            .buildings
            .push(building(*owner, BuildingKind::Foundry, *x, *y));
    }
    let model = map(&scenario);
    let site = model
        .sites()
        .iter()
        .find(|site| {
            site.nodes.is_empty()
                && site.frames.contains(&TilePos::new(17, 10))
                && site.frames.contains(&TilePos::new(21, 10))
        })
        .expect("the two frames make one site");
    let state = scenario.build().unwrap();
    let observation = mapped(&state, PlayerId(0));
    expansion::value(&observation, &model, &Memory::default(), site, 50).is_none()
}

#[test]
fn a_frames_only_site_is_claimed_by_an_ally_or_once_own_foundries_cover_it() {
    // (9, 10) supports only the frame at (17, 10); (25, 10) only the one at
    // (21, 10); (19, 11) both.
    assert!(!claimed_with(&[], false), "premise");
    assert!(
        !claimed_with(&[(0, 9, 10)], false),
        "an own Foundry covering one frame leaves the other to support"
    );
    assert!(claimed_with(&[(0, 9, 10), (0, 25, 10)], false));
    assert!(
        !claimed_with(&[(1, 19, 11)], false),
        "an enemy Foundry supports only its own Extractors"
    );
    assert!(
        claimed_with(&[(2, 9, 10)], true),
        "teammates do not crowd one expansion"
    );
}

#[test]
fn only_guns_that_reach_a_frame_count_as_its_danger() {
    let score = |guns: &[(BuildingKind, i32, i32)]| {
        let mut scenario = lane(&[(14, 7)]);
        scenario.units.push(unit(0, UnitKind::Kestrel, 16, 6));
        for (kind, x, y) in guns {
            scenario.buildings.push(building(1, *kind, *x, *y));
        }
        score_of(&frame_scores(&scenario), 14, 7)
    };
    let quiet = score(&[]);
    assert_eq!(
        score(&[
            (BuildingKind::Turret, 22, 7),
            (BuildingKind::FlakTurret, 13, 9)
        ]),
        quiet,
        "a Turret out of range and a Flak Turret, which fires only at the air"
    );
    assert!(score(&[(BuildingKind::Turret, 19, 7)]) < quiet);
}

#[test]
fn an_enemy_that_cannot_hurt_a_building_leaves_a_frame_wanted() {
    let mut scenario = arena(0);
    scenario.map[1].replace_range(6..7, "E");
    let wanted = |kind| {
        let mut scenario = scenario.clone();
        scenario.units.push(unit(1, kind, 8, 2));
        let state = scenario.build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        expansion::candidates(&observation, &map(&scenario), &Memory::default(), 50, 0)
            .iter()
            .any(|(investment, _)| *investment == Investment::Extractor(TilePos::new(6, 1)))
    };
    assert!(!wanted(UnitKind::Sentinel), "premise");
    assert!(wanted(UnitKind::Flakhound));
}
