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
            let ticked = state.tick(&commands);
            events.record(PlayerId(0), &state, &ticked.events);
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
        events.record(PlayerId(0), &state, &report.events);
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
fn a_supported_extractor_becomes_a_deep_bore_before_harvesting_saturates_at_every_rung() {
    // Two Harvesters on untouched home nodes leave the crews far short, and
    // nothing is mined until they deliver.
    let scenario = bore_arena(12);
    for difficulty in BotDifficulty::ALL {
        let mut state = scenario.build().unwrap();
        let extractor = own_extractor(&state);
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
            assert!(
                upgraded
                    || !report.events.iter().any(|event| matches!(
                        event,
                        Event::ScrapDeposited {
                            player: PlayerId(0),
                            ..
                        }
                    )),
                "{difficulty:?} deepens before its first delivery, tick {}",
                report.tick
            );
            events.record(PlayerId(0), &state, &report.events);
        }
        assert!(upgraded, "{difficulty:?}");
        assert_eq!(state.building(extractor).unwrap().tier, 1, "{difficulty:?}");
    }
}

#[test]
fn a_deep_bore_with_neither_tech_building_starts_at_the_fabricator() {
    let mut scenario = bore_arena(12);
    scenario
        .buildings
        .retain(|building| building.kind != BuildingKind::Crucible);
    let state = scenario.build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let deep_bore = Investment::Upgrade {
        building: own_extractor(&state),
        tier: 1,
    };
    let frame = home_frame(&observation, &map(&scenario));
    assert_eq!(
        crate::investments::step(&observation, frame, deep_bore),
        Some((
            crate::Step::Build(BuildingKind::Fabricator),
            expansion::construction_cost(BuildingKind::Fabricator)
        ))
    );
}

#[test]
fn a_deep_bore_wanted_before_its_tech_stands_builds_that_tech_first() {
    let mut scenario = bore_arena(12);
    scenario
        .buildings
        .retain(|building| building.kind != BuildingKind::Crucible);
    for difficulty in BotDifficulty::ALL {
        let plans = crate::plans_deep_bores(difficulty);
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
            let own = |kind| {
                state
                    .buildings()
                    .iter()
                    .filter(move |building| building.player == PlayerId(0) && building.kind == kind)
            };
            let fabricator = own(BuildingKind::Fabricator).next().is_some();
            assert!(
                plans
                    || own(BuildingKind::Crucible).any(aftermine_sim::Building::built)
                    || !trace.as_ref().is_some_and(|trace| trace
                        .target
                        .is_some_and(|target| target.investment == deep_bore)),
                "{difficulty:?} saves toward a Deep Bore before a Crucible stands, tick {}",
                state.current_tick()
            );
            assert!(
                fabricator
                    || !trace.as_ref().is_some_and(|trace| trace.target.is_some_and(
                        |target| target.investment == deep_bore && target.next.is_none()
                    )),
                "{difficulty:?} saves toward the Deep Bore with nothing to buy, tick {}",
                state.current_tick()
            );
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
            events.record(PlayerId(0), &state, &report.events);
        }
        if plans {
            assert_eq!(
                pulled,
                Some(Some(deep_bore)),
                "{difficulty:?} builds its Crucible toward the Deep Bore"
            );
        } else {
            assert_ne!(pulled, Some(Some(deep_bore)), "{difficulty:?}");
        }
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
            plans_bores: crate::plans_deep_bores(difficulty),
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
    expansion::candidates(
        &observation,
        &map(scenario),
        &Memory::default(),
        (50, true, 0),
        0,
    )
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
    let (supported, own_side, enemy_side) = (
        score_of(&scores, 6, 4),
        score_of(&scores, 14, 7),
        score_of(&scores, 24, 5),
    );
    assert!(supported > own_side, "{scores:?}");
    assert!(
        enemy_side < own_side,
        "a frame East reaches first is worth less: {scores:?}"
    );
    assert!(
        !scores
            .iter()
            .any(|(frame, _)| *frame == TilePos::new(32, 5)),
        "and one at East's door is East's to hold: {scores:?}"
    );
    let bore = |crucible: bool, plans_bores: bool| {
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
            plans_bores,
            ..calm(&observation, &model, &memory)
        });
        let score = |wanted: fn(&Investment) -> bool| {
            candidates
                .iter()
                .find(|candidate| wanted(&candidate.investment))
                .map(|candidate| candidate.score)
        };
        (
            score(|investment| *investment == Investment::Extractor(TilePos::new(14, 7)))
                .expect("the frame is a candidate"),
            score(|investment| matches!(investment, Investment::Upgrade { tier: 1, .. })),
        )
    };
    let (frame, deep_bore) = bore(true, true);
    let deep_bore = deep_bore.expect("the Deep Bore is a candidate");
    let early = bore(false, true)
        .1
        .expect("an early Deep Bore is a candidate");
    assert!(
        frame > deep_bore,
        "an Extractor returns more for its price than a Deep Bore"
    );
    assert!(
        deep_bore > early,
        "a Crucible still to buy is part of the first Deep Bore's price"
    );
    assert!(
        bore(false, false).1.is_none(),
        "a seat that does not plan for a Deep Bore waits for a Crucible"
    );
}

#[test]
fn a_seat_refits_its_economy_s_share_of_its_extractors() {
    // Two Extractors West's Foundry supports and a Crucible: a seat that
    // builds half its economy, or most of it, deepens one of them, the full
    // economy both.
    // Deep Bores out of every Foundry's support count toward neither.
    let wanted = |deepened: bool, orphans: bool, difficulty: BotDifficulty| {
        let mut scenario = lane(&[(6, 3), (6, 9), (24, 3), (24, 9)]);
        let mut buildings = vec![
            (BuildingKind::Extractor, 6, 3),
            (BuildingKind::Extractor, 6, 9),
            (BuildingKind::Crucible, 9, 5),
        ];
        if orphans {
            buildings.extend([
                (BuildingKind::Extractor, 24, 3),
                (BuildingKind::Extractor, 24, 9),
            ]);
        }
        for (kind, x, y) in buildings {
            scenario.buildings.push(building(0, kind, x, y));
        }
        let mut value = serde_json::to_value(scenario.build().unwrap()).unwrap();
        for entry in value["buildings"].as_array_mut().unwrap() {
            let anchor = (entry["anchor"]["x"].clone(), entry["anchor"]["y"].clone());
            let refitted =
                (deepened && anchor == (6.into(), 3.into())) || entry["anchor"]["x"] == 24;
            if entry["kind"] == "extractor" && refitted {
                entry["tier"] = 1.into();
            }
        }
        let state: State = serde_json::from_value(value).unwrap();
        let observation = mapped(&state, PlayerId(0));
        let model = map(&scenario);
        let memory = Memory::default();
        crate::investments::candidates(&crate::investments::Situation {
            economy: crate::economy(difficulty),
            ..calm(&observation, &model, &memory)
        })
        .iter()
        .any(|candidate| matches!(candidate.investment, Investment::Upgrade { tier: 1, .. }))
    };
    assert!(
        wanted(false, false, BotDifficulty::Scrapheap),
        "the first of two"
    );
    assert!(
        !wanted(true, false, BotDifficulty::Scrapheap),
        "half of two is one"
    );
    assert!(
        wanted(true, false, BotDifficulty::Prime),
        "Prime deepens both"
    );
    assert!(
        !wanted(true, false, BotDifficulty::Veteran),
        "most of two, rounded down, is one"
    );
    assert!(
        wanted(false, true, BotDifficulty::Scrapheap),
        "Deep Bores no Foundry supports leave the supported two their share"
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

/// The frontier with a frame six tiles from West's safe field, an ally of
/// West's starting below West, and a West Kestrel overhead so West sees what
/// stands beside the field.
fn guarded_field() -> Scenario {
    let mut scenario = frontier(1_000);
    scenario.map[7].replace_range(20..21, "E");
    scenario.map[8].replace_range(2..3, "3");
    let ally = scenario.players[1].clone();
    scenario.players.push(ally);
    for (player, team) in scenario.players.iter_mut().zip([0, 1, 0]) {
        player.team = Some(team);
    }
    scenario.units.push(unit(0, UnitKind::Kestrel, 16, 9));
    scenario
}

/// Whether West's safe field reads as worked with `beside`, as owner, kind
/// and anchor, standing near it.
fn field_claimed_with(beside: Option<(u8, BuildingKind, i32, i32)>) -> bool {
    let mut scenario = guarded_field();
    if let Some((owner, kind, x, y)) = beside {
        scenario.buildings.push(building(owner, kind, x, y));
    }
    let model = map(&scenario);
    let (_, site) = site_with(&model, TilePos::new(12, 10));
    let state = scenario.build().unwrap();
    let observation = mapped(&state, PlayerId(0));
    expansion::value(&observation, &model, &Memory::default(), site, 50).is_none()
}

#[test]
fn a_field_is_claimed_by_a_foundry_or_an_ally_but_not_by_other_enemy_buildings() {
    assert!(!field_claimed_with(None), "premise");
    assert!(
        !field_claimed_with(Some((1, BuildingKind::Extractor, 20, 7))),
        "an enemy Extractor on another site's frame"
    );
    assert!(
        !field_claimed_with(Some((1, BuildingKind::Turret, 15, 12))),
        "an enemy Turret is danger, not a claim"
    );
    assert!(
        field_claimed_with(Some((1, BuildingKind::Foundry, 9, 8))),
        "an enemy Foundry works the field"
    );
    assert!(field_claimed_with(Some((0, BuildingKind::Foundry, 9, 8))));
    assert!(
        field_claimed_with(Some((2, BuildingKind::Turret, 15, 12))),
        "teammates do not crowd one expansion"
    );
}

#[test]
fn an_enemy_extractor_beside_a_field_leaves_it_to_expand_into() {
    let mut scenario = guarded_field();
    scenario.buildings.extend([
        building(1, BuildingKind::Extractor, 20, 7),
        building(0, BuildingKind::Fabricator, 6, 8),
    ]);
    let model = map(&scenario);
    let (index, site) = site_with(&model, TilePos::new(12, 10));
    // The rich field above failed at every anchor, so the guarded one is the
    // only expansion left.
    let rich: Vec<serde_json::Value> = site_with(&model, TilePos::new(12, 2))
        .1
        .anchors
        .iter()
        .map(|anchor| serde_json::json!({"kind": "foundry", "anchor": anchor, "at": 0}))
        .collect();
    let state = scenario.build().unwrap();
    let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
        json["memory"]["failures"] = serde_json::Value::Array(rich);
        json["saving"] = serde_json::json!({
            "protected": 0,
            "target": {"investment": {"expansion": index}, "attempt": null},
        });
    });
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        builds(&commands)
            .iter()
            .any(|(kind, anchor)| *kind == BuildingKind::Foundry && site.anchors.contains(anchor)),
        "{commands:?}"
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
        expansion::candidates(
            &observation,
            &map(&scenario),
            &Memory::default(),
            (50, true, 0),
            0,
        )
        .iter()
        .any(|(investment, _)| *investment == Investment::Extractor(TilePos::new(6, 1)))
    };
    assert!(!wanted(UnitKind::Sentinel), "premise");
    assert!(wanted(UnitKind::Flakhound));
}

#[test]
fn a_secure_seat_scores_a_site_by_the_support_its_remote_extractor_would_get() {
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
    let (index, _) = site_with(&model, TilePos::new(12, 10));
    let score = |scenario: &Scenario, secure: bool| {
        let state = scenario.build().unwrap();
        let observation = ObservationData::fog_honest(&state, PlayerId(0));
        expansion::candidates(&observation, &model, &Memory::default(), (50, secure, 0), 0)
            .into_iter()
            .find(|(investment, _)| {
                *investment == Investment::Expansion(u16::try_from(index).unwrap())
            })
            .map(|(_, score)| score)
    };
    let insecure = score(&scenario, false).expect("the site is a candidate");
    let secure = score(&scenario, true).expect("the site is a candidate");
    assert!(
        secure > insecure,
        "support for the stranded Extractor counts once the army holds: {secure} vs {insecure}"
    );
}

#[test]
fn a_stranded_deep_bore_draws_its_foundry_back_harder_than_an_extractor() {
    // Support lifts a Deep Bore's yield by more than an Extractor's, so a
    // Foundry that would support one stranded there scores higher.
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
    let (index, _) = site_with(&model, TilePos::new(12, 10));
    let score = |state: &State| {
        let observation = ObservationData::fog_honest(state, PlayerId(0));
        expansion::candidates(&observation, &model, &Memory::default(), (50, true, 0), 0)
            .into_iter()
            .find(|(investment, _)| {
                *investment == Investment::Expansion(u16::try_from(index).unwrap())
            })
            .map(|(_, score)| score)
            .expect("the site is a candidate")
    };
    let extractor = score(&scenario.build().unwrap());
    let bore = score(&bored(&scenario));
    assert!(bore > extractor, "{bore} vs {extractor}");
}

#[test]
fn a_frame_no_enemy_walks_to_is_still_theirs_when_their_guns_reach_it() {
    // Two frames on pit-ringed plateaus no ground unit walks onto, one
    // beside each seat. Each seat wants the one beside it; the one beside
    // the enemy is in reach of the enemy's ground and wanted by neither.
    let mut scenario = lane(&[(31, 3), (7, 9)]);
    for (row, columns) in [(2, 30..34), (5, 30..34), (8, 6..10), (11, 6..10)] {
        scenario.map[row].replace_range(columns.clone(), &"~".repeat(columns.len()));
    }
    for (rows, columns) in [(3..5, [30, 33]), (9..11, [6, 9])] {
        for row in rows {
            for column in columns {
                scenario.map[row].replace_range(column..=column, "~");
            }
        }
    }
    let state = scenario.build().unwrap();
    let model = map(&scenario);
    let wanted = |player: u8| -> Vec<TilePos> {
        expansion::candidates(
            &mapped(&state, PlayerId(player)),
            &model,
            &Memory::default(),
            (50, true, 0),
            0,
        )
        .into_iter()
        .filter_map(|(investment, _)| match investment {
            Investment::Extractor(frame) => Some(frame),
            _ => None,
        })
        .collect()
    };
    assert_eq!(wanted(0), vec![TilePos::new(7, 9)]);
    assert_eq!(wanted(1), vec![TilePos::new(31, 3)]);
}

#[test]
fn ground_shoots_a_frame_where_its_near_edge_is_in_view() {
    // A frame alone among pits, East's lane coming up from the south: a rock
    // below the frame hides its centre from the lane's end, but not its
    // near corner, and the simulation aims at that.
    let frame = TilePos::new(14, 4);
    let mut map: Vec<String> = (0..22)
        .map(|row| {
            if row == 0 || row == 21 {
                "#".repeat(40)
            } else {
                format!("#{}#", "~".repeat(38))
            }
        })
        .collect();
    let mut paint = |x: usize, y: usize, tiles: &str| {
        map[y].replace_range(x..x + tiles.len(), tiles);
    };
    for y in 1..6 {
        paint(1, y, ".....");
    }
    paint(2, 2, "1");
    for y in 15..21 {
        paint(30, y, ".........");
    }
    paint(36, 18, "2");
    paint(14, 4, "E.");
    paint(14, 5, "..");
    paint(15, 7, "#");
    for y in 10..21 {
        paint(17, y, ".");
    }
    paint(17, 20, &".".repeat(13));
    let mut scenario = lane(&[]);
    scenario.map = map;
    let model = self::map(&scenario);
    assert!(
        model.distance(PlayerId(1), TilePos::new(17, 10)) != crate::map::UNREACHABLE
            && model.distance(PlayerId(1), frame) == crate::map::UNREACHABLE,
        "premise: East walks the lane but not onto the frame"
    );
    assert_ne!(
        model.firing_distance(PlayerId(1), frame),
        crate::map::UNREACHABLE
    );
}

#[test]
fn a_plateau_the_seat_holds_with_a_foundry_keeps_its_frames() {
    // A pit-ringed plateau beside East's start, in reach of East's ground,
    // with West's Foundry standing on it: its frame is West's to build.
    let mut scenario = lane(&[(31, 3)]);
    for row in 1..=6 {
        let pits = if row == 1 || row == 6 { 26..34 } else { 26..27 };
        scenario.map[row].replace_range(pits.clone(), &"~".repeat(pits.len()));
        if row != 1 && row != 6 {
            scenario.map[row].replace_range(33..34, "~");
        }
    }
    let wanted = |scenario: &Scenario| {
        let state = scenario.build().unwrap();
        expansion::candidates(
            &mapped(&state, PlayerId(0)),
            &map(scenario),
            &Memory::default(),
            (50, true, 0),
            0,
        )
        .iter()
        .any(|(investment, _)| *investment == Investment::Extractor(TilePos::new(31, 3)))
    };
    assert!(!wanted(&scenario), "an enemy's guns reach the plateau");
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Foundry,
        x: 27,
        y: 2,
    });
    assert!(wanted(&scenario), "West holds the plateau");
}

#[test]
fn a_deep_bore_waits_while_a_known_enemy_army_outweighs_the_seat_s() {
    // A supported Extractor and a Crucible. With no enemy army known the
    // Deep Bore competes; three Lancers in sight outweigh an empty army and
    // hold it back. Four Sentinels, worth more but beaten by them, do not
    // hold them; two Wardens do, and it competes again.
    let bores = |lancers: i32, guards: &[(UnitKind, i32, i32)]| {
        let mut scenario = bore_arena(12);
        scenario
            .units
            .extend((0..lancers).map(|index| unit(1, UnitKind::Lancer, 14, 2 + index)));
        scenario
            .units
            .extend(guards.iter().map(|(kind, x, y)| unit(0, *kind, *x, *y)));
        let state = scenario.build().unwrap();
        let observation = mapped(&state, PlayerId(0));
        let mut memory = Memory::default();
        memory.observe_stranded(&observation, |_| false);
        let model = map(&scenario);
        crate::investments::candidates(&calm(&observation, &model, &memory))
            .iter()
            .any(|candidate| matches!(candidate.investment, Investment::Upgrade { tier: 1, .. }))
    };
    let sentinels = [(4, 8), (5, 8), (4, 9), (5, 9)].map(|(x, y)| (UnitKind::Sentinel, x, y));
    let wardens = [(4, 8), (5, 8)].map(|(x, y)| (UnitKind::Warden, x, y));
    assert!(bores(0, &[]), "nothing known");
    assert!(!bores(3, &[]), "three Lancers outweigh no army");
    assert!(!bores(3, &sentinels), "four Sentinels do not hold them");
    assert!(bores(3, &wardens), "two Wardens hold them");
}

#[test]
fn a_known_army_holds_a_deep_bore_back_until_it_is_seen_to_die() {
    // West has held scrap for a Deep Bore. Three East Lancers it saw at its
    // Extractor have gone from there since. Unseen, they still outweigh its
    // empty army, so it lets the bore go; seen to die, they no longer do, and
    // it saves on, as a seat that never saw them does.
    let saving = |seen: bool, died: bool| {
        let mut sighted = bore_arena(12);
        if seen {
            sighted
                .units
                .extend((0..3).map(|index| unit(1, UnitKind::Lancer, 14, 2 + index)));
        }
        let first = sighted.build().unwrap();
        let mut looked = seat(&sighted, 0);
        looked.act(&first, &mut OwnEvents::default());
        let memory = serde_json::to_value(looked.checkpoint()).unwrap()["memory"].clone();
        let lancers: Vec<(UnitId, TilePos)> = first
            .units()
            .iter()
            .filter(|unit| unit.kind == UnitKind::Lancer)
            .map(|unit| (unit.id, unit.tile()))
            .collect();

        let mut scenario = bore_arena(12);
        // Short of the bore's price, so it is still saved for.
        scenario.players[0].scrap = 300;
        let mut state = scenario.build().unwrap();
        advance_to(&mut state, crate::memory::UNIT_TICKS + 12, &[]);
        let deep_bore = Investment::Upgrade {
            building: own_extractor(&state),
            tier: 1,
        };
        let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
            json["memory"] = memory;
            json["saving"] = serde_json::json!({
                "protected": 300,
                "target": {"investment": deep_bore, "attempt": null},
            });
        });
        let mut events = OwnEvents::default();
        if died {
            let deaths: Vec<aftermine_sim::Event> = lancers
                .iter()
                .map(|&(unit, tile)| {
                    assert!(state.vision(PlayerId(0)).visible(tile), "premise: in sight");
                    aftermine_sim::Event::UnitDied {
                        unit,
                        kind: UnitKind::Lancer,
                        player: PlayerId(1),
                        pos: tile.center(),
                        grounded: true,
                    }
                })
                .collect();
            events.record(PlayerId(0), &state, &deaths);
        }
        let (_, trace) = opponent.act_traced(&state, &mut events);
        trace.unwrap().target.map(|target| target.investment) == Some(deep_bore)
    };
    assert!(
        saving(false, false),
        "premise: with nothing known it saves on"
    );
    assert!(!saving(true, false), "gone from sight, they still count");
    assert!(saving(true, true), "seen to die, they no longer do");
}

#[test]
fn a_seat_saving_for_a_deep_bore_lets_it_go_once_a_known_army_outweighs_its_own() {
    // West has held scrap for a Deep Bore. Three East Lancers come into
    // sight with no West army to meet them: it stops saving for the bore,
    // stops holding that scrap back, and spends it on meeting them.
    let saved = |lancers: i32| {
        let mut scenario = bore_arena(12);
        // Short of the bore's price, so it is still saved for.
        scenario.players[0].scrap = 300;
        scenario
            .units
            .extend((0..lancers).map(|index| unit(1, UnitKind::Lancer, 14, 2 + index)));
        let state = scenario.build().unwrap();
        let deep_bore = Investment::Upgrade {
            building: own_extractor(&state),
            tier: 1,
        };
        let mut opponent = restored_with(&seat(&scenario, 0), &scenario, &state, |json| {
            json["saving"] = serde_json::json!({
                "protected": 300,
                "target": {"investment": deep_bore, "attempt": null},
            });
        });
        let (commands, trace) = opponent.act_traced(&state, &mut OwnEvents::default());
        let trace = trace.unwrap();
        let spent = !trains(&commands).is_empty() || !builds(&commands).is_empty();
        (
            trace.target.map(|target| target.investment) == Some(deep_bore),
            trace.protected,
            spent,
        )
    };
    let (kept, held, _) = saved(0);
    assert!(kept && held > 0, "premise: with nothing known it saves on");
    let (kept, held_after, spent) = saved(3);
    assert!(!kept, "the Lancers outweigh an empty army");
    assert!(held_after < held, "{held_after} < {held}");
    assert!(spent, "the scrap let go buys an answer");
}

/// A memory that counts `lancers` East Lancers seen far off at tick 0, with
/// no place remembered for them now.
fn knows_lancers(lancers: u32) -> Memory {
    let census: Vec<serde_json::Value> = (0..lancers)
        .map(|id| {
            serde_json::json!({"id": 100 + id, "kind": UnitKind::Lancer, "player": 1, "tile": {"x": 36, "y": 2}, "seen": 0})
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "units": [], "census": census, "failures": [], "abandoned": [],
        "raided": [], "scouted": [], "stuck": [], "extractors": [],
    }))
    .unwrap()
}

#[test]
fn a_seat_on_one_foundry_an_army_outweighs_insures_it_with_an_expansion_on_its_side() {
    // A free frame beside a West field and its mirror beside an East one.
    // West stands on its start Foundry with two Sentinels. Three known East
    // Lancers outweigh them: what losing that Foundry would cost raises the
    // fields with frames above tech, the one on West's side by more than its
    // mirror near East. Two Wardens hold the Lancers, and a second Foundry,
    // built or going up, already insures the seat: then nothing changes.
    let mut scenario = frontier(0);
    scenario.map[9].replace_range(16..18, "E.");
    scenario.map[9].replace_range(22..24, "E.");
    let model = map(&scenario);
    let (west, _) = site_with(&model, TilePos::new(12, 2));
    let (east, _) = site_with(&model, rotate(TilePos::new(12, 2)));
    let scores = |guards: UnitKind, second: Option<bool>, lancers: u32| {
        let mut scenario = scenario.clone();
        scenario
            .units
            .extend([(8, 4), (8, 5)].map(|(x, y)| unit(0, guards, x, y)));
        if second == Some(true) {
            scenario.buildings.push(BuildingSpec {
                player: 0,
                kind: BuildingKind::Foundry,
                x: 2,
                y: 9,
            });
        }
        let state = scenario.build().unwrap();
        let mut observation = mapped(&state, PlayerId(0));
        if second == Some(false) {
            let mut site = crate::investments::built_foundries(&observation.my_buildings)
                .next()
                .unwrap()
                .clone();
            site.anchor = TilePos::new(2, 9);
            site.built = false;
            observation.my_buildings.push(site);
        }
        let memory = knows_lancers(lancers);
        let list = crate::investments::candidates(&calm(&observation, &model, &memory));
        let score = |investment: Investment| {
            list.iter()
                .find(|candidate| candidate.investment == investment)
                .map_or(0, |candidate| candidate.score)
        };
        let site = |index: usize| Investment::Expansion(u16::try_from(index).unwrap());
        (
            score(site(west)),
            score(site(east)),
            score(Investment::Tech(BuildingKind::Fabricator)),
        )
    };
    let (west_calm, east_calm, tech) = scores(UnitKind::Sentinel, None, 0);
    assert!(
        west_calm < tech,
        "premise: tech leads while nothing is known"
    );
    let (west_risk, east_risk, _) = scores(UnitKind::Sentinel, None, 3);
    assert!(west_risk > tech, "{west_risk} vs tech {tech}");
    assert!(
        west_risk - west_calm > east_risk - east_calm,
        "West's side gains more: {west_calm}->{west_risk} vs {east_calm}->{east_risk}"
    );
    assert_eq!(
        scores(UnitKind::Warden, None, 3).0,
        scores(UnitKind::Warden, None, 0).0
    );
    for second in [true, false] {
        assert_eq!(
            scores(UnitKind::Sentinel, Some(second), 3).0,
            scores(UnitKind::Sentinel, Some(second), 0).0,
            "a second Foundry, built: {second}"
        );
    }
}

/// The frontier with a Fabricator beside West's start, a West Harvester by
/// the north field, `lancers` East Lancers at West's door and `scrap` in
/// each bank.
fn besieged(scrap: u32, lancers: usize) -> Scenario {
    let mut scenario = frontier(scrap);
    scenario.units.push(harvester(0, 16, 3));
    scenario
        .buildings
        .push(building(0, BuildingKind::Fabricator, 2, 10));
    for (x, y) in [(6, 5), (6, 6), (6, 7), (7, 5), (7, 6), (7, 7)]
        .into_iter()
        .take(lancers)
    {
        scenario.units.push(unit(1, UnitKind::Lancer, x, y));
    }
    scenario
}

fn foundries_placed(commands: &[PlayerCommand]) -> Vec<TilePos> {
    builds(commands)
        .into_iter()
        .filter(|(kind, _)| *kind == BuildingKind::Foundry)
        .map(|(_, anchor)| anchor)
        .collect()
}

#[test]
fn a_seat_whose_last_foundry_is_overrun_places_another_out_of_known_fire() {
    // Six East Lancers at West's start outweigh everything its 600 could
    // buy: it places a Foundry at a field out of their reach. A bank that
    // could answer them trains instead, two Lancers are no such siege, nor
    // six raiding a far building, a second built Foundry needs no backup,
    // and once one is going up no other follows it.
    let placed = |scenario: &Scenario| {
        let state = scenario.build().unwrap();
        foundries_placed(
            &seat_with(scenario, 0, rung(BotDifficulty::Prime))
                .act(&state, &mut OwnEvents::default()),
        )
    };
    let scenario = besieged(600, 6);
    let model = map(&scenario);
    let backup = placed(&scenario);
    assert_eq!(backup.len(), 1, "{backup:?}");
    assert!(
        model
            .sites()
            .iter()
            .any(|site| site.anchors.contains(&backup[0]))
    );
    let reach = crate::defenses::reach(UnitKind::Lancer)
        .ceil()
        .to_num::<i32>();
    assert!(
        [(6, 5), (6, 6), (6, 7), (7, 5), (7, 6), (7, 7)]
            .into_iter()
            .all(|(x, y)| crate::frame::gap(
                backup[0],
                BuildingKind::Foundry.size(),
                TilePos::new(x, y),
                (1, 1)
            ) > reach),
        "{backup:?}"
    );
    assert!(placed(&besieged(1_000, 6)).is_empty(), "the bank answers");
    assert!(placed(&besieged(600, 2)).is_empty(), "two Lancers");
    let mut far = besieged(600, 0);
    far.buildings.push(building(0, BuildingKind::Array, 22, 9));
    for (x, y) in [(24, 8), (24, 9), (24, 10), (25, 8), (25, 9), (25, 10)] {
        far.units.push(unit(1, UnitKind::Lancer, x, y));
    }
    assert!(placed(&far).is_empty(), "a raid on a far building");
    let mut second = besieged(600, 6);
    second
        .buildings
        .push(building(0, BuildingKind::Foundry, 6, 10));
    assert!(placed(&second).is_empty(), "a second Foundry stands");

    let mut state = scenario.build().unwrap();
    let mut opponent = seat_with(&scenario, 0, rung(BotDifficulty::Prime));
    let mut events = OwnEvents::default();
    let commands = opponent.act(&state, &mut events);
    assert_eq!(foundries_placed(&commands).len(), 1);
    state.tick(&commands);
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    assert!(
        state
            .buildings()
            .iter()
            .any(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry && !b.built()),
        "premise: the site is going up"
    );
    assert!(foundries_placed(&opponent.act(&state, &mut events)).is_empty());
}

#[test]
fn a_seat_left_with_only_a_foundry_going_up_keeps_deciding() {
    // West's start falls while its north Foundry is going up, and the
    // builder has walked off: the site alone keeps West in the match, and
    // West sends a worker back to finish it.
    let mut scenario = besieged(600, 0);
    scenario.units.extend([
        unit(1, UnitKind::Sentinel, 5, 5),
        unit(1, UnitKind::Sentinel, 5, 7),
    ]);
    let state = scenario.build().unwrap();
    let start = foundries(&state, PlayerId(0))[0];
    let mut state = damaged(&state, start, 1);
    let builder = at(&state, 16, 3);
    let anchor = TilePos::new(15, 2);
    let command = |player: u8, command: Command| PlayerCommand {
        player: PlayerId(player),
        command,
    };
    state.tick(&[
        command(
            0,
            Command::Build {
                units: vec![builder],
                kind: BuildingKind::Foundry,
                anchor,
                queue: false,
                defer: false,
            },
        ),
        command(
            1,
            Command::Attack {
                units: vec![at(&state, 5, 5), at(&state, 5, 7)],
                target: AttackTarget::Building(start),
                queue: false,
            },
        ),
    ]);
    let site = |state: &State| {
        state
            .buildings()
            .iter()
            .find(|b| b.player == PlayerId(0) && b.anchor == anchor)
            .cloned()
    };
    let built = |state: &State| {
        state
            .buildings()
            .iter()
            .any(|b| b.player == PlayerId(0) && b.kind == BuildingKind::Foundry && b.built())
    };
    while site(&state).is_some_and(|site| site.unstarted()) && state.current_tick() < 600 {
        state.tick(&[]);
    }
    state.tick(&[command(
        0,
        Command::Run {
            units: vec![builder],
            goal: TilePos::new(22, 4),
            queue: false,
        },
    )]);
    while built(&state) && state.current_tick() < 1_200 {
        state.tick(&[]);
    }
    let mut opponent = seat_with(&scenario, 0, rung(BotDifficulty::Prime));
    while !opponent.decision_due(&state) {
        state.tick(&[]);
    }
    assert!(
        !built(&state) && site(&state).is_some_and(|site| !site.built() && !site.unstarted()),
        "premise: only a started site stands"
    );
    let commands = opponent.act(&state, &mut OwnEvents::default());
    assert!(
        builds(&commands)
            .iter()
            .any(|(kind, at)| *kind == BuildingKind::Foundry && *at == anchor),
        "{commands:?}"
    );
}
