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

/// Drags the ghost's row handle until the row ends at `tile`: the handle
/// rides a fingertip past the row's far edge, so the finger leads by that.
pub(super) fn pull_handle_to(game: &mut Game, input: &mut InputState, tile: TilePos) {
    let handle = lay_out_ghost_controls(game, input).handle;
    assert!(handle.w > 0.0, "premise: the ghost offers a handle");
    let ghost = input.ghost_anchor().expect("a ghost is down");
    let (dx, dy) = ((tile.x - ghost.x).signum(), (tile.y - ghost.y).signum());
    let reach = crate::theme::MIN_TOUCH_TARGET * input.ui * 0.5 / game.presentation.camera.zoom;
    let edge =
        vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5) + vec2(dx as f32, dy as f32) * (0.5 + reach);
    let to = game.presentation.camera.to_screen(edge);
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
            TilePos::new(3, 2),
            TilePos::new(4, 2),
            TilePos::new(5, 2),
            TilePos::new(6, 2)
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
    let (mut game, mut input) = ghost_range(TilePos::new(6, 1));
    pull_handle_to(&mut game, &mut input, TilePos::new(6, 4));
    let rows = input.planned_stamps().to_vec();
    assert_eq!(
        rows,
        [
            TilePos::new(6, 1),
            TilePos::new(6, 2),
            TilePos::new(6, 3),
            TilePos::new(6, 4)
        ],
        "south, one footprint a step"
    );
    assert!(row_runs_down(&input), "so the controls hang above");
    let controls = lay_out_ghost_controls(&game, &input);
    let ghost = ghost_footprint(&game.view(), &input).expect("a ghost is down");
    assert!(controls.confirm.bottom() < ghost.y, "clear of the row");
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
    pull_handle_to(&mut game, &mut input, TilePos::new(3, 3));
    assert!(row_runs_down(&input), "premise: a row pulled south");
    dispatch_action(&mut game, &mut input, Action::Back);
    assert!(input.planned_stamps().is_empty(), "Back folds a row too");
    assert!(!row_runs_down(&input), "leaving no direction behind");
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
    let reach = crate::theme::MIN_TOUCH_TARGET * input.ui * 0.5 / game.presentation.camera.zoom;
    let to = game.presentation.camera.to_screen(vec2(7.0 + reach, 2.5));
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
    let (mut game, mut input) = ghost_range(TilePos::new(8, 3));
    input.queue_toggle = QueueToggle::Locked;
    tap_check(&mut game, &mut input);
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(9, 3)),
        "one footprint east after the first"
    );
    tap_world(&mut game, &mut input, vec2(7.5, 2.5));
    tap_check(&mut game, &mut input);
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(6, 1)),
        "then the step from the last site"
    );

    pull_handle_to(&mut game, &mut input, TilePos::new(8, 1));
    tap_check(&mut game, &mut input);
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(9, 1)),
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

#[test]
fn a_row_builds_with_queue_as_it_stands_at_the_check() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    pull_handle_to(&mut game, &mut input, TilePos::new(5, 2));
    // Queue lights after the row was pulled out, before the check.
    input.queue_toggle = QueueToggle::Locked;
    tap_check(&mut game, &mut input);
    let row = builds(&game);
    assert_eq!(row.len(), 3);
    assert!(
        row.iter().all(|(_, queue)| *queue),
        "the opener queues too: {row:?}"
    );
}

#[test]
fn a_chain_after_a_gapped_row_keeps_the_row_step() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    input.queue_toggle = QueueToggle::Locked;
    pull_handle_to(&mut game, &mut input, TilePos::new(6, 2));
    // Another crew's claim makes the row skip its third cell.
    let other = game.state.units()[0].id;
    game.issue(Command::Build {
        units: vec![other],
        kind: oxide_sim::BuildingKind::Barricade,
        anchor: TilePos::new(5, 2),
        queue: true,
        defer: false,
    });
    tap_check(&mut game, &mut input);
    let row: Vec<_> = builds(&game).iter().map(|(a, _)| *a).collect();
    assert_eq!(
        row,
        [
            TilePos::new(5, 2),
            TilePos::new(3, 2),
            TilePos::new(4, 2),
            TilePos::new(6, 2)
        ],
        "premise: the other claim, then the row with its third cell skipped"
    );
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(7, 2)),
        "one row step past the last site, not the gap's double step"
    );
}

#[test]
fn a_finger_that_lands_on_the_cross_never_presses_the_check() {
    let (mut game, mut input) = ghost_range(TilePos::new(6, 2));
    let controls = lay_out_ghost_controls(&game, &input);
    let landing = vec2(controls.cancel.right() - 1.0, controls.cancel.center().y);
    let lift = vec2(controls.confirm.x + 1.0, controls.confirm.center().y);
    input.now += 1.0;
    apply_events(&mut game, &mut input, &[touch_down(1, landing)]);
    input.now += 0.05;
    apply_events(&mut game, &mut input, &[touch_up(1, lift)]);
    assert!(game.pending.is_empty(), "no build");
    assert!(input.placing.is_some(), "and no cancel either");
}

#[test]
fn a_finger_that_slips_off_a_ghost_button_moves_nothing() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    pull_handle_to(&mut game, &mut input, TilePos::new(5, 2));
    let controls = lay_out_ghost_controls(&game, &input);
    let landing = vec2(controls.confirm.right() - 1.0, controls.confirm.center().y);
    let lift = landing + vec2(4.0, 0.0);
    assert!(
        ghost_control_at(&game.presentation.layout.get(), lift).is_none(),
        "premise: the finger lifts just off the buttons"
    );
    input.now += 1.0;
    apply_events(&mut game, &mut input, &[touch_down(1, landing)]);
    input.now += 0.05;
    apply_events(&mut game, &mut input, &[touch_up(1, lift)]);
    assert!(game.pending.is_empty(), "no build");
    assert_eq!(
        input.ghost_anchor(),
        Some(TilePos::new(3, 2)),
        "the ghost stays put"
    );
    assert_eq!(input.planned_stamps().len(), 3, "and keeps its row");
}

#[test]
fn the_check_mark_wins_where_the_handle_reaches_over_it() {
    // Zoomed out, the handle's fingertip target reaches past its disc
    // onto the check mark below the ghost.
    let zoomed_out = || {
        let (mut game, input) = ghost_range(TilePos::new(6, 2));
        game.presentation.camera.zoom = 12.0;
        let controls = lay_out_ghost_controls(&game, &input);
        let shared = controls
            .confirm
            .intersect(controls.handle)
            .expect("premise: the targets overlap");
        (game, input, shared.center())
    };
    let (mut game, mut input, p) = zoomed_out();
    tap(&mut game, &mut input, p);
    assert_eq!(builds(&game).len(), 1, "a finger builds");

    let (mut game, mut input, p) = zoomed_out();
    apply_events(&mut game, &mut input, &[left_down(p), left_up(p)]);
    assert_eq!(builds(&game).len(), 1, "and so does a click");
}

#[test]
fn a_click_moves_a_ghost_a_finger_left() {
    let (mut game, mut input) = ghost_range(TilePos::new(6, 2));
    let p = game.presentation.camera.to_screen(vec2(9.5, 3.5));
    apply_events(
        &mut game,
        &mut input,
        &[mouse_move(p), left_down(p), left_up(p)],
    );
    assert!(game.pending.is_empty(), "the click builds nothing");
    assert_eq!(input.ghost_anchor(), Some(TilePos::new(9, 3)));
    assert_eq!(
        placement_preview_anchor(&game.view(), &input).map(|(_, anchor)| anchor),
        Some(TilePos::new(9, 3)),
        "so the preview shows where the check mark will build"
    );
}

#[test]
fn the_ghost_controls_answer_before_the_minimap_and_queue() {
    let (mut game, mut input) = ghost_range(TilePos::new(6, 2));
    let controls = lay_out_ghost_controls(&game, &input);
    let mut layout = game.presentation.layout.get();
    layout.minimap = controls.confirm;
    layout.queue_toggle = controls.cancel;
    game.presentation.layout.set(layout);
    let camera = game.presentation.camera.center;
    tap(&mut game, &mut input, controls.confirm.center());
    assert_eq!(builds(&game).len(), 1, "the check under the minimap builds");
    assert_eq!(
        game.presentation.camera.center, camera,
        "and steers nothing"
    );
}

#[test]
fn a_row_grows_with_the_handle_travel_alone() {
    // At the default zoom, a nudge just past the slop pulls no row.
    let (mut game, mut input) = ghost_range(TilePos::new(6, 2));
    let handle = lay_out_ghost_controls(&game, &input).handle;
    input.now += 1.0;
    apply_events(
        &mut game,
        &mut input,
        &[
            touch_down(7, handle.center()),
            touch_move(7, handle.center() + vec2(13.0, 0.0)),
        ],
    );
    assert!(input.planned_stamps().is_empty(), "a nudge is not a row");

    // Zoomed out, the handle's fingertip offset spans several tiles; only
    // the travel past it counts: two footprints, two more cells.
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    game.presentation.camera.zoom = 8.0;
    let handle = lay_out_ghost_controls(&game, &input).handle;
    input.now += 1.0;
    apply_events(
        &mut game,
        &mut input,
        &[
            touch_down(7, handle.center()),
            touch_move(7, handle.center() + vec2(16.0, 0.0)),
        ],
    );
    assert_eq!(input.planned_stamps().len(), 3);
}

#[test]
fn a_held_row_drops_cells_the_world_has_since_taken() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    // Queued, so the row keeps the builder's other claims.
    input.queue_toggle = QueueToggle::Locked;
    pull_handle_to(&mut game, &mut input, TilePos::new(6, 2));
    assert_eq!(input.planned_stamps().len(), 4);
    let other = game.state.units()[0].id;
    game.issue(Command::Build {
        units: vec![other],
        kind: oxide_sim::BuildingKind::Barricade,
        anchor: TilePos::new(5, 2),
        queue: true,
        defer: false,
    });
    update_touch(&mut game, &mut input);
    assert!(
        !input.planned_stamps().contains(&TilePos::new(5, 2)),
        "the row is judged again once the batch moves"
    );
}

#[test]
fn a_trackpad_drags_the_row_handle_too() {
    let (mut game, mut input) = ghost_range(TilePos::new(3, 2));
    let handle = lay_out_ghost_controls(&game, &input).handle;
    let reach = crate::theme::MIN_TOUCH_TARGET * input.ui * 0.5 / game.presentation.camera.zoom;
    let to = game.presentation.camera.to_screen(vec2(6.0 + reach, 2.5));
    apply_events(
        &mut game,
        &mut input,
        &[left_down(handle.center()), mouse_move(to), left_up(to)],
    );
    assert_eq!(input.planned_stamps().len(), 3, "the cursor pulled a row");
    assert!(game.pending.is_empty(), "and built nothing");
    assert!(input.placing.is_some());
}
