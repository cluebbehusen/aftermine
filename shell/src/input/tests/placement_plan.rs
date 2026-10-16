use super::*;

/// The drag bank's builder selected, a Turret armed.
fn planning(scrap: u32) -> (Game, InputState) {
    let mut game = drag_arena(scrap);
    let mut input = InputState::new();
    game.presentation.selection.units = vec![game.state.units()[0].id];
    input.arm_placement(oxide_sim::BuildingKind::Turret);
    input.build_menu = true;
    (game, input)
}

fn at(game: &Game, x: i32) -> Vec2 {
    game.presentation
        .camera
        .to_screen(vec2(x as f32 + 0.5, 2.5))
}

fn turret_cost() -> u32 {
    oxide_sim::BuildingKind::Turret
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
    assert_eq!(input.planned_stamps().len(), 3, "the row is planned");
    assert!(game.pending.is_empty(), "and nothing is spent yet");
    assert!(game.presentation.fx.is_empty(), "nor acknowledged");

    apply_events(&mut game, &mut input, &[left_up(c)]);
    assert_eq!(staged_builds(&game), 3, "the release stages the row");
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
    assert_eq!(input.planned_stamps().len(), 2);
    // Another command claims the second stamp's ground mid-drag.
    game.issue(Command::Build {
        units: vec![builder],
        kind: oxide_sim::BuildingKind::Barricade,
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
                    kind: oxide_sim::BuildingKind::Turret,
                    ..
                }
            )
        })
        .count();
    assert_eq!(turrets, 1, "the first stamp still stages");
    assert!(
        game.presentation
            .toasts
            .iter()
            .any(|toast| toast.text == "1 of 2 sites skipped"),
        "and the release says what it skipped"
    );
}
