use super::*;

/// One harvester with plenty of scrap on open, visible ground, a Barricade
/// armed and its ghost down at `ghost`.
fn ghost_range(ghost: TilePos) -> (Game, InputState) {
    let scenario = oxide_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Ghost Range",
            "players": [
                {"name": "Mason", "scrap": 5000, "bot": false},
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
                {"player": 0, "kind": "harvester", "x": 5, "y": 4}
            ]
        })
        .to_string(),
    )
    .expect("ghost range parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let mut input = InputState::new();
    game.presentation.selection.units = vec![game.state.units()[0].id];
    input.arm_placement(oxide_sim::BuildingKind::Barricade);
    input.build_menu = true;
    tap_world(
        &mut game,
        &mut input,
        vec2(ghost.x as f32 + 0.5, ghost.y as f32 + 0.5),
    );
    assert_eq!(
        input.ghost_anchor(),
        Some(ghost),
        "premise: the ghost is down"
    );
    (game, input)
}

/// Drags the ghost's row handle out to the centre of `tile`.
fn pull_handle_to(game: &mut Game, input: &mut InputState, tile: TilePos) {
    let handle = lay_out_ghost_controls(game, input).handle;
    assert!(handle.w > 0.0, "premise: the ghost offers a handle");
    let to = game
        .presentation
        .camera
        .to_screen(vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5));
    input.now += 1.0;
    apply_events(game, input, &[touch_down(7, handle.center())]);
    apply_events(game, input, &[touch_move(7, to)]);
    apply_events(game, input, &[touch_up(7, to)]);
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
fn arming_by_touch_drops_the_ghost_mid_view_and_by_mouse_does_not() {
    let (mut game, mut input, _) = armed_placement(oxide_sim::BuildingKind::Turret);
    input.stop_placing();
    tap_panel_card(&mut game, &mut input, |card| {
        card.action == crate::panel::CardAction::ArmBuild(oxide_sim::BuildingKind::Turret)
    });
    let center = game.presentation.camera.center;
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(
            numeric::to_i32((center.x - 0.5).round()),
            numeric::to_i32((center.y - 0.5).round()),
        )),
        "a finger's pick shows its ghost at once"
    );

    input.stop_placing();
    input.last_pointer = Pointer::Mouse;
    activate_card(
        &mut game,
        &mut input,
        crate::panel::CardAction::ArmBuild(oxide_sim::BuildingKind::Turret),
    );
    assert!(input.placing.is_some());
    assert_eq!(
        input.ghost_anchor(),
        None,
        "the cursor is the mouse's ghost"
    );
}

#[test]
fn a_pulled_row_previews_until_the_check_builds_it_whole() {
    let ghost = TilePos::new(3, 2);
    let (mut game, mut input) = ghost_range(ghost);
    pull_handle_to(&mut game, &mut input, TilePos::new(6, 2));
    assert_eq!(
        input.planned_stamps(),
        [
            (TilePos::new(3, 2), false),
            (TilePos::new(4, 2), false),
            (TilePos::new(5, 2), false),
            (TilePos::new(6, 2), false)
        ],
        "the row runs out to the finger"
    );
    assert!(game.pending.is_empty(), "lifting builds nothing");

    tap_check(&mut game, &mut input);
    assert_eq!(
        builds(&game),
        [
            (TilePos::new(3, 2), false),
            (TilePos::new(4, 2), true),
            (TilePos::new(5, 2), true),
            (TilePos::new(6, 2), true)
        ],
        "the check builds the row, the first replacing as a plain tap does"
    );
    assert!(input.placing.is_none(), "and a plain check ends placement");
}

#[test]
fn a_row_runs_along_the_axis_the_finger_leads() {
    let (mut game, mut input) = ghost_range(TilePos::new(6, 4));
    pull_handle_to(&mut game, &mut input, TilePos::new(6, 1));
    let rows: Vec<_> = input.planned_stamps().iter().map(|(a, _)| *a).collect();
    assert_eq!(
        rows,
        [
            TilePos::new(6, 4),
            TilePos::new(6, 3),
            TilePos::new(6, 2),
            TilePos::new(6, 1)
        ],
        "north, one footprint a step"
    );
    assert!(row_runs_up(&input), "so the controls hang below");
    let controls = lay_out_ghost_controls(&game, &input);
    let ghost = ghost_footprint(&game.view(), &input).expect("a ghost is down");
    assert!(controls.confirm.y > ghost.bottom(), "clear of the row");
}

#[test]
fn the_cross_folds_a_row_back_then_returns_the_building() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    pull_handle_to(&mut game, &mut input, TilePos::new(5, 2));
    assert_eq!(input.planned_stamps().len(), 3);
    tap_cross(&mut game, &mut input);
    assert!(input.planned_stamps().is_empty(), "the row folds away");
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(3, 2)),
        "the ghost stays"
    );
    pull_handle_to(&mut game, &mut input, TilePos::new(3, 1));
    assert!(row_runs_up(&input), "premise: a row pulled north");
    dispatch_action(&mut game, &mut input, Action::Back);
    assert!(input.planned_stamps().is_empty(), "Back folds a row too");
    assert!(!row_runs_up(&input), "leaving no direction behind");
    assert_eq!(input.ghost_anchor(), Some(TilePos::new(3, 2)));
    tap_cross(&mut game, &mut input);
    assert!(
        input.placing.is_none(),
        "a lone ghost gives the building back"
    );
    assert!(input.build_menu, "to the palette");
    assert!(game.pending.is_empty());
}

#[test]
fn a_cancelled_handle_drops_its_row_and_moving_the_ghost_leaves_it() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    let handle = lay_out_ghost_controls(&game, &input).handle;
    let to = game.presentation.camera.to_screen(vec2(6.5, 2.5));
    input.now += 1.0;
    apply_events(
        &mut game,
        &mut input,
        &[touch_down(7, handle.center()), touch_move(7, to)],
    );
    assert_eq!(input.planned_stamps().len(), 4, "premise: a row");
    apply_events(&mut game, &mut input, &[touch_cancel(7)]);
    assert!(
        input.planned_stamps().is_empty(),
        "a taken finger completes nothing"
    );

    pull_handle_to(&mut game, &mut input, TilePos::new(5, 2));
    assert_eq!(input.planned_stamps().len(), 3);
    tap_world(&mut game, &mut input, vec2(8.5, 4.5));
    assert_eq!(input.ghost_anchor(), Some(TilePos::new(8, 4)));
    assert!(input.planned_stamps().is_empty(), "the row stays behind");
}

#[test]
fn a_queued_chain_offers_its_next_spot_along_the_last_step() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    input.queue_toggle = QueueToggle::Locked;
    tap_check(&mut game, &mut input);
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(4, 2)),
        "one footprint east after the first"
    );
    tap_world(&mut game, &mut input, vec2(2.5, 3.5));
    tap_check(&mut game, &mut input);
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(1, 4)),
        "then the step from the last site"
    );

    pull_handle_to(&mut game, &mut input, TilePos::new(3, 4));
    tap_check(&mut game, &mut input);
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(4, 4)),
        "a row hands on its own step"
    );
    assert_eq!(builds(&game).len(), 5);
}

#[test]
fn a_ghost_smaller_than_a_fingertip_rides_above_the_finger() {
    let (mut game, mut input) = ghost_range(TilePos::new(6, 2));
    let grab = game.presentation.camera.to_screen(vec2(6.5, 2.5));
    let finger = game.presentation.camera.to_screen(vec2(9.5, 5.5));
    input.now += 1.0;
    apply_events(
        &mut game,
        &mut input,
        &[touch_down(1, grab), touch_move(1, finger)],
    );
    let ghost = ghost_footprint(&game.view(), &input).expect("a ghost is down");
    assert!(
        ghost.bottom() <= finger.y,
        "the ghost clears the fingertip: {ghost:?} over {finger:?}"
    );
    assert!(
        (ghost.center().x - finger.x).abs() <= game.presentation.camera.zoom * 0.5,
        "straight above it"
    );
}
