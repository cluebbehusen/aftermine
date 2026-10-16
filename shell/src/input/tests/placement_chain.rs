use super::*;

/// One harvester with scrap for a row of turrets on open, visible ground.
fn funded_builder() -> (Game, InputState) {
    let scenario = oxide_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Chain Range",
            "players": [
                {"name": "Mason", "scrap": 1000, "bot": false},
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
    .expect("chain range parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let mut input = InputState::new();
    game.presentation.selection.units = vec![game.state.units()[0].id];
    input.arm_placement(oxide_sim::BuildingKind::Turret);
    input.build_menu = true;
    (game, input)
}

fn at(game: &Game, x: i32, y: i32) -> Vec2 {
    game.presentation
        .camera
        .to_screen(vec2(x as f32 + 0.5, y as f32 + 0.5))
}

fn builds(game: &Game) -> Vec<(TilePos, bool)> {
    game.pending
        .iter()
        .filter_map(|pc| match pc.command {
            Command::Build { anchor, queue, .. } => Some((anchor, queue)),
            _ => None,
        })
        .collect()
}

#[test]
fn letting_go_of_shift_after_a_queued_placement_ends_the_chain() {
    let (mut game, mut input) = funded_builder();
    let (a, b, c) = (at(&game, 7, 3), at(&game, 9, 3), at(&game, 11, 3));
    apply_events(
        &mut game,
        &mut input,
        &[
            key_down(Key::Shift),
            left_down(a),
            left_up(a),
            left_down(b),
            left_up(b),
        ],
    );
    assert_eq!(builds(&game).len(), 2, "premise: Shift chains");
    assert!(input.placing.is_some(), "and keeps placing while held");

    apply_events(&mut game, &mut input, &[key_up(Key::Shift)]);
    assert!(input.placing.is_none(), "letting go ends the chain");

    // The next click selects; it must not build a plain site that would
    // replace the two just queued.
    apply_events(&mut game, &mut input, &[left_down(c), left_up(c)]);
    assert_eq!(
        builds(&game),
        [(TilePos::new(7, 3), true), (TilePos::new(9, 3), true)]
    );
}

#[test]
fn letting_go_of_shift_before_anything_is_placed_keeps_placement_armed() {
    let (mut game, mut input) = funded_builder();
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Shift), key_up(Key::Shift)],
    );
    assert!(input.placing.is_some(), "a bare Shift tap changes nothing");

    // A refused Shift-click places nothing either.
    let wall = at(&game, 0, 3);
    apply_events(
        &mut game,
        &mut input,
        &[
            key_down(Key::Shift),
            left_down(wall),
            left_up(wall),
            key_up(Key::Shift),
        ],
    );
    assert!(builds(&game).is_empty());
    assert!(input.placing.is_some(), "a refusal leaves the mode armed");
}

#[test]
fn letting_go_of_shift_mid_stroke_leaves_the_decision_to_the_release() {
    let (mut game, mut input) = funded_builder();
    let (a, b, c) = (at(&game, 7, 3), at(&game, 8, 3), at(&game, 9, 3));
    apply_events(
        &mut game,
        &mut input,
        &[
            key_down(Key::Shift),
            left_down(a),
            mouse_move(b),
            key_up(Key::Shift),
        ],
    );
    assert!(
        !input.planned_stamps().is_empty(),
        "the stroke outlives the Shift release"
    );
    apply_events(&mut game, &mut input, &[mouse_move(c), left_up(c)]);
    assert_eq!(builds(&game).len(), 3, "the stroke kept stamping");
    assert!(
        input.placing.is_none(),
        "and a release without Shift ends placement"
    );
}

#[test]
fn turning_queue_off_after_a_queued_confirm_ends_touch_placement() {
    let (mut game, mut input) = funded_builder();
    let chip = macroquad::math::Rect::new(160.0, 620.0, 96.0, 44.0);
    let mut layout = bare_layout(f32::INFINITY, 0.0);
    layout.queue_toggle = chip;
    game.presentation.layout.set(layout);
    let ground = vec2(7.5, 3.5);

    // QUEUE off with a ghost down but nothing placed keeps placing.
    input.queue_toggle = QueueToggle::Locked;
    tap_world(&mut game, &mut input, ground);
    tap(&mut game, &mut input, chip.center());
    assert!(input.placing.is_some(), "nothing was placed yet");
    assert!(input.ghost_anchor().is_some(), "and the ghost stays down");

    tap(&mut game, &mut input, chip.center());
    tap_world(&mut game, &mut input, ground);
    assert_eq!(builds(&game).len(), 1, "premise: the queued confirm built");
    assert!(input.placing.is_some(), "QUEUE keeps placing");

    tap(&mut game, &mut input, chip.center());
    assert_eq!(input.queue_toggle, QueueToggle::Off);
    assert!(input.placing.is_none(), "turning QUEUE off ends the chain");
}
