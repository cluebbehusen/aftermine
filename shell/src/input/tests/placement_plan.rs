use super::*;

/// The drag bank's builder selected, a Turret armed.
fn planning(scrap: u32) -> (Game, InputState) {
    let mut game = drag_arena(scrap);
    let mut input = InputState::new();
    game.presentation.selection.units = vec![game.state.units()[0].id];
    input.arm_placement(aftermine_sim::BuildingKind::Turret);
    input.build_menu = true;
    (game, input)
}

fn at(game: &Game, x: i32) -> Vec2 {
    game.presentation
        .camera
        .to_screen(vec2(x as f32 + 0.5, 2.5))
}

fn turret_cost() -> u32 {
    aftermine_sim::BuildingKind::Turret
        .base_stats()
        .construction
        .expect("turrets are built")
        .cost
}

#[test]
fn a_drag_stages_nothing_until_the_release() {
    let (mut game, mut input) = planning(10_000);
    let (a, b, c) = (at(&game, 4), at(&game, 6), at(&game, 8));
    apply_events(
        &mut game,
        &mut input,
        &[left_down(a), mouse_move(b), mouse_move(c)],
    );
    assert_eq!(
        input.planned_stamps().len(),
        5,
        "the row is planned, the cells between moves included"
    );
    assert!(game.pending.is_empty(), "and nothing is spent yet");
    assert!(game.presentation.fx.is_empty(), "nor acknowledged");

    apply_events(&mut game, &mut input, &[left_up(c)]);
    assert_eq!(staged_builds(&game), 5, "the release stages the row");
    assert!(input.placing.is_none(), "and a plain stroke ends placement");
}

#[test]
fn escape_or_a_right_click_mid_drag_drops_the_row_and_keeps_placing() {
    for cancel in [
        [key_down(Key::Escape), key_up(Key::Escape)],
        [
            right_down(vec2(600.0, 300.0)),
            RawEvent::MouseUp {
                button: MouseButton::Right,
                x: 600.0,
                y: 300.0,
            },
        ],
    ] {
        let (mut game, mut input) = planning(10_000);
        let (a, b) = (at(&game, 4), at(&game, 6));
        apply_events(&mut game, &mut input, &[left_down(a), mouse_move(b)]);
        apply_events(&mut game, &mut input, &cancel);
        assert!(
            input.planned_stamps().is_empty(),
            "{cancel:?} drops the row"
        );
        assert!(input.placing.is_some(), "{cancel:?} keeps placing");
        let c = at(&game, 8);
        apply_events(&mut game, &mut input, &[mouse_move(c), left_up(c)]);
        assert!(game.pending.is_empty(), "{cancel:?}: nothing staged");
        assert!(input.placing.is_some(), "{cancel:?}: still placing");
    }
}

#[test]
fn a_row_stops_where_the_bank_runs_out() {
    let (mut game, mut input) = planning(2 * turret_cost());
    let tiles: Vec<_> = [4, 6, 8, 10].into_iter().map(|x| at(&game, x)).collect();
    apply_events(&mut game, &mut input, &[left_down(tiles[0])]);
    for p in &tiles[1..] {
        apply_events(&mut game, &mut input, &[mouse_move(*p)]);
    }
    assert_eq!(input.planned_stamps().len(), 2, "two Turrets' worth");
    apply_events(&mut game, &mut input, &[left_up(tiles[3])]);
    assert_eq!(staged_builds(&game), 2);
}

#[test]
fn a_release_skips_stamps_the_ground_no_longer_allows() {
    let (mut game, mut input) = planning(10_000);
    let builder = game.state.units()[0].id;
    let (a, b) = (at(&game, 4), at(&game, 6));
    // Shift, so the opening stamp keeps the builder's other claims.
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Shift), left_down(a), mouse_move(b)],
    );
    assert_eq!(input.planned_stamps().len(), 3);
    // Another command claims the last stamp's ground mid-drag.
    game.issue(Command::Build {
        units: vec![builder],
        kind: aftermine_sim::BuildingKind::Barricade,
        anchor: TilePos::new(6, 2),
        queue: true,
        defer: false,
    });
    apply_events(&mut game, &mut input, &[left_up(b)]);
    let turrets = game
        .pending
        .iter()
        .filter(|pc| {
            matches!(
                pc.command,
                Command::Build {
                    kind: aftermine_sim::BuildingKind::Turret,
                    ..
                }
            )
        })
        .count();
    assert_eq!(turrets, 2, "the stamps before it still stage");
    assert!(
        game.presentation
            .toasts
            .iter()
            .any(|toast| toast.text == "1 of 3 sites skipped"),
        "and the release says what it skipped"
    );
}

#[test]
fn one_long_move_or_a_bare_release_fills_every_cell_between() {
    let (mut game, mut input) = planning(10_000);
    let (a, b) = (at(&game, 4), at(&game, 10));
    apply_events(&mut game, &mut input, &[left_down(a), mouse_move(b)]);
    let row: Vec<_> = input.planned_stamps().iter().map(|t| t.x).collect();
    assert_eq!(row, [4, 5, 6, 7, 8, 9, 10], "no gaps for a fast cursor");
    apply_events(&mut game, &mut input, &[left_up(b)]);

    let (mut game, mut input) = planning(10_000);
    let (a, b) = (at(&game, 4), at(&game, 7));
    apply_events(&mut game, &mut input, &[left_down(a), left_up(b)]);
    assert_eq!(staged_builds(&game), 4, "the release point counts too");
}

/// Applies `batches` as the frame loop does: its pointer pre-pass moves
/// `input.mouse` to each frame's last pointer before the frame's events
/// apply.
fn frames(game: &mut Game, input: &mut InputState, batches: &[Vec<RawEvent>]) {
    for batch in batches {
        if let Some(last) = batch.iter().rev().find_map(|event| match *event {
            RawEvent::MouseMove { x, y }
            | RawEvent::MouseDown { x, y, .. }
            | RawEvent::MouseUp { x, y, .. } => Some(vec2(x, y)),
            _ => None,
        }) {
            input.mouse = last;
        }
        apply_events(game, input, batch);
    }
}

#[test]
fn a_fast_drag_through_the_frame_loop_lays_a_gapless_row_in_order() {
    for moves_a_frame in [1, 3] {
        let (mut game, mut input) = planning(10_000);
        input.arm_placement(aftermine_sim::BuildingKind::Barricade);
        // Two tiles a move: fast enough to skip a cell each move.
        let points: Vec<Vec2> = (4..=12).step_by(2).map(|x| at(&game, x)).collect();
        let mut batches = vec![vec![left_down(points[0])]];
        for frame in points[1..].chunks(moves_a_frame) {
            batches.push(frame.iter().map(|p| mouse_move(*p)).collect());
        }
        batches.push(vec![left_up(points[points.len() - 1])]);
        frames(&mut game, &mut input, &batches);
        let row: Vec<i32> = game
            .pending
            .iter()
            .filter_map(|pc| match pc.command {
                Command::Build { anchor, .. } => Some(anchor.x),
                _ => None,
            })
            .collect();
        assert_eq!(
            row,
            (4..=12).collect::<Vec<_>>(),
            "{moves_a_frame} moves a frame: every cell crossed, in order"
        );
    }
}

#[test]
fn a_camera_jump_mid_drag_lays_nothing_between_the_two_views() {
    let (mut game, mut input) = planning(10_000);
    input.arm_placement(aftermine_sim::BuildingKind::Barricade);
    // Close enough that the map outgrows the window, so a jump moves it.
    let camera = &mut game.presentation.camera;
    camera.glide_to(camera.to_screen(vec2(5.5, 2.5)), 128.0);
    for _ in 0..100 {
        camera.update(1.0);
    }
    assert!(!camera.zooming());
    let (a, b) = (at(&game, 4), at(&game, 6));
    frames(
        &mut game,
        &mut input,
        &[vec![left_down(a)], vec![mouse_move(b)]],
    );
    assert_eq!(
        input.planned_stamps(),
        [4, 5, 6].map(|x| TilePos::new(x, 2))
    );
    super::super::reticle::look_at(&mut game, &mut input, vec2(16.5, 7.5));
    assert!(
        game.presentation
            .camera
            .to_world(b)
            .distance(vec2(6.5, 2.5))
            > 4.0,
        "premise: the jump moved the view"
    );
    let c = b + vec2(1.0, 0.0);
    frames(&mut game, &mut input, &[vec![mouse_move(c)]]);
    let world = game.presentation.camera.to_world(c);
    let under = TilePos::new(
        crate::numeric::to_i32(world.x.floor()),
        crate::numeric::to_i32(world.y.floor()),
    );
    for stamp in input.planned_stamps() {
        assert!(
            (stamp.y == 2 && (4..=6).contains(&stamp.x)) || *stamp == under,
            "{stamp:?} lies between the views"
        );
    }
}

#[test]
fn the_first_stamp_that_stages_opens_the_row() {
    let scenario = aftermine_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Two Crews",
            "players": [
                {"name": "Mason", "scrap": 10_000, "bot": false},
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
                {"player": 0, "kind": "harvester", "x": 5, "y": 4},
                {"player": 0, "kind": "harvester", "x": 8, "y": 4}
            ]
        })
        .to_string(),
    )
    .expect("two crews parse");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let mut input = InputState::new();
    let (builder, other) = (game.state.units()[0].id, game.state.units()[1].id);
    game.presentation.selection.units = vec![builder];
    input.arm_placement(aftermine_sim::BuildingKind::Turret);
    let (a, b) = (at(&game, 4), at(&game, 5));
    apply_events(&mut game, &mut input, &[left_down(a), mouse_move(b)]);
    assert_eq!(input.planned_stamps().len(), 2);
    // Another crew claims the opener's ground mid-drag.
    game.issue(Command::Build {
        units: vec![other],
        kind: aftermine_sim::BuildingKind::Barricade,
        anchor: TilePos::new(4, 2),
        queue: false,
        defer: false,
    });
    apply_events(&mut game, &mut input, &[left_up(b)]);
    let turrets: Vec<_> = game
        .pending
        .iter()
        .filter_map(|pc| match pc.command {
            Command::Build {
                kind: aftermine_sim::BuildingKind::Turret,
                anchor,
                queue,
                ..
            } => Some((anchor, queue)),
            _ => None,
        })
        .collect();
    assert_eq!(
        turrets,
        [(TilePos::new(5, 2), false)],
        "the survivor replaces as the plain stroke meant to"
    );
}

#[test]
fn a_stamp_whose_ground_falls_into_fog_is_judged_as_fog() {
    // A long bank: the human's Foundry far west, the stamp out of its
    // sight, seen only by the harvester that then walks east.
    let width = 64;
    let mut map = vec!["#".repeat(width)];
    for y in 1..6 {
        let mut line = format!("#{}#", ".".repeat(width - 2));
        if y == 1 {
            line.replace_range(1..2, "1");
        }
        if y == 4 {
            line.replace_range(width - 3..width - 2, "2");
        }
        map.push(line);
    }
    map.push("#".repeat(width));
    let scenario = aftermine_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Long Bank",
            "players": [
                {"name": "Mason", "scrap": 10_000, "bot": false},
                {"name": "Idle", "scrap": 0, "bot": true}
            ],
            "map": map,
            "units": [{"player": 0, "kind": "harvester", "x": 28, "y": 3}]
        })
        .to_string(),
    )
    .expect("long bank parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let builder = game.state.units()[0].id;
    game.presentation.selection.units = vec![builder];
    let kind = aftermine_sim::BuildingKind::Turret;
    let stamp = TilePos::new(30, 3);
    assert!(
        !build_defer_needed(&game.view(), kind, stamp),
        "premise: in sight"
    );
    let stroke = PlacingStroke {
        queue: true,
        stamps: vec![stamp],
    };
    let human = game.presentation.human;
    game.state.tick(&[PlayerCommand {
        player: human,
        command: Command::Run {
            units: vec![builder],
            goal: TilePos::new(55, 3),
            queue: false,
        },
    }]);
    for _ in 0..2000 {
        if build_defer_needed(&game.view(), kind, stamp) {
            break;
        }
        game.state.tick(&[]);
    }
    assert!(
        build_defer_needed(&game.view(), kind, stamp),
        "premise: the stamp's ground has fallen into fog"
    );
    let batch = planned_batch(&game.view(), kind, &stroke);
    assert!(
        matches!(
            batch.last(),
            Some(PlayerCommand {
                command: Command::Build { defer: true, .. },
                ..
            })
        ),
        "the projection judges it from memory, never against what fog hides"
    );
}
