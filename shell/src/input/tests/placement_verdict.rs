use super::*;

/// One harvester with `scrap` on open, visible ground.
fn builder_with(scrap: u32) -> (Game, InputState, UnitId) {
    let scenario = oxide_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Verdict Range",
            "players": [
                {"name": "Mason", "scrap": scrap, "bot": false},
                {"name": "Idle", "scrap": 0, "bot": true}
            ],
            "map": [
                "################",
                "#1.............#",
                "#..............#",
                "#..............#",
                "#............2.#",
                "#..............#",
                "################"
            ],
            "units": [
                {"player": 0, "kind": "harvester", "x": 5, "y": 3}
            ]
        })
        .to_string(),
    )
    .expect("verdict range parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let mut input = InputState::new();
    let builder = game.state.units()[0].id;
    game.presentation.selection.units = vec![builder];
    input.arm_placement(oxide_sim::BuildingKind::Turret);
    input.build_menu = true;
    (game, input, builder)
}

fn turret_cost() -> u32 {
    oxide_sim::BuildingKind::Turret
        .base_stats()
        .construction
        .expect("turrets are built")
        .cost
}

#[test]
fn a_staged_build_refreshes_the_cached_verdict() {
    let (mut game, _, builder) = builder_with(1000);
    let kind = oxide_sim::BuildingKind::Turret;
    let anchor = TilePos::new(8, 3);
    assert_eq!(
        placement_verdict(&game.view(), kind, anchor, true),
        PlacementVerdict::Builds
    );
    game.issue(Command::Build {
        units: vec![builder],
        kind,
        anchor,
        queue: true,
        defer: false,
    });
    assert_eq!(
        placement_verdict(&game.view(), kind, anchor, true),
        PlacementVerdict::Refused(oxide_sim::PlaceRefusal::Building),
        "the staged site now holds that ground"
    );
}

#[test]
fn an_unaffordable_spot_is_short_by_the_missing_scrap_and_refuses_the_click() {
    let (mut game, mut input, _) = builder_with(10);
    let anchor = TilePos::new(8, 3);
    assert_eq!(
        placement_verdict(&game.view(), oxide_sim::BuildingKind::Turret, anchor, false),
        PlacementVerdict::Short(turret_cost() - 10)
    );
    let p = game
        .presentation
        .camera
        .to_screen(vec2(anchor.x as f32 + 0.5, anchor.y as f32 + 0.5));
    apply_events(&mut game, &mut input, &[left_down(p), left_up(p)]);
    assert!(game.pending.is_empty(), "a short click stages nothing");
    assert!(input.placing.is_some(), "and keeps placing");
    assert!(
        game.presentation
            .toasts
            .iter()
            .any(|toast| toast.text == "Not enough scrap for a Turret"),
        "the toast names the shortfall"
    );
}

#[test]
fn a_queued_build_takes_the_number_its_ghost_wore() {
    let (mut game, mut input, builder) = builder_with(1000);
    let human = game.presentation.human;
    for (x, queue) in [(9, false), (11, true)] {
        game.state.tick(&[PlayerCommand {
            player: human,
            command: Command::Run {
                units: vec![builder],
                goal: TilePos::new(x, 5),
                queue,
            },
        }]);
    }
    let backlog = placement_backlog(&game.view()).expect("the builder has a program");
    assert_eq!(backlog, 2, "premise: two runs ahead of the build");

    let anchor = TilePos::new(8, 2);
    let p = game
        .presentation
        .camera
        .to_screen(vec2(anchor.x as f32 + 0.5, anchor.y as f32 + 0.5));
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Shift), left_down(p), left_up(p)],
    );
    let view = game.view();
    let projection = view.projection();
    let program = projection.program(builder).expect("decorated builder");
    assert!(
        matches!(
            program.orders[backlog],
            oxide_sim::Order::Build { .. } | oxide_sim::Order::Found { .. }
        ),
        "chip #{} is the build: {:?}",
        backlog + 1,
        program.orders
    );
}

#[test]
fn a_visible_pocket_the_founder_cannot_reach_is_refused() {
    let scenario = oxide_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Pocket",
            "players": [
                {"name": "Mason", "scrap": 1000, "bot": false},
                {"name": "Idle", "scrap": 0, "bot": true}
            ],
            "map": [
                "################",
                "#1.............#",
                "#.......###....#",
                "#.......#.#....#",
                "#.......###..2.#",
                "#..............#",
                "################"
            ],
            "units": [
                {"player": 0, "kind": "harvester", "x": 5, "y": 3}
            ]
        })
        .to_string(),
    )
    .expect("pocket parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let mut input = InputState::new();
    game.presentation.selection.units = vec![game.state.units()[0].id];
    input.arm_placement(oxide_sim::BuildingKind::Barricade);
    let pocket = TilePos::new(9, 3);
    assert!(
        game.state.vision(game.presentation.human).visible(pocket),
        "premise: the pocket is in sight"
    );
    assert_eq!(
        placement_verdict(
            &game.view(),
            oxide_sim::BuildingKind::Barricade,
            pocket,
            false
        ),
        PlacementVerdict::Unreachable
    );
    assert_eq!(
        placement_verdict(
            &game.view(),
            oxide_sim::BuildingKind::Barricade,
            TilePos::new(6, 3),
            false
        ),
        PlacementVerdict::Builds,
        "open ground beside it is fine"
    );
    let report = game.state.clone().tick(&[PlayerCommand {
        player: game.presentation.human,
        command: Command::Build {
            units: game.presentation.selection.units.clone(),
            kind: oxide_sim::BuildingKind::Barricade,
            anchor: pocket,
            queue: false,
            defer: false,
        },
    }]);
    assert!(
        report.events.iter().any(|event| matches!(
            event,
            oxide_sim::Event::CommandRejected {
                reason: oxide_sim::command::RejectReason::UnreachableGoal,
                ..
            }
        )),
        "the sim agrees"
    );
    let p = game.presentation.camera.to_screen(vec2(9.5, 3.5));
    apply_events(&mut game, &mut input, &[left_down(p), left_up(p)]);
    assert!(game.pending.is_empty(), "the click stages nothing");
    assert!(
        game.presentation
            .toasts
            .iter()
            .any(|toast| toast.text == "Can't build there: your builders can't reach it"),
        "and says why"
    );
}

#[test]
fn a_builder_with_a_full_queue_shows_no_queue_number() {
    let (mut game, _, builder) = builder_with(1000);
    let human = game.presentation.human;
    let mut fill = vec![PlayerCommand {
        player: human,
        command: Command::Run {
            units: vec![builder],
            goal: TilePos::new(9, 5),
            queue: false,
        },
    }];
    for _ in 0..oxide_sim::stats::ORDER_QUEUE_CAP {
        fill.push(PlayerCommand {
            player: human,
            command: Command::Run {
                units: vec![builder],
                goal: TilePos::new(11, 5),
                queue: true,
            },
        });
    }
    game.state.tick(&fill);
    assert_eq!(
        placement_backlog(&game.view()),
        Some(oxide_sim::stats::ORDER_QUEUE_CAP + 1),
        "premise: an active order and a full queue"
    );
    assert_eq!(queued_position(&game.view()), None, "it would be skipped");
}
