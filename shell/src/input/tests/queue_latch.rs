use super::*;

/// A fighter selected on open ground, with the QUEUE chip laid out.
fn latch_range() -> (Game, InputState, Vec2, Vec2) {
    let mut game = headless_game();
    let (fighter, at) = own_fighter(&game);
    let chip = macroquad::math::Rect::new(160.0, 620.0, 96.0, 44.0);
    let mut layout = bare_layout(f32::INFINITY, 0.0);
    layout.queue_toggle = chip;
    game.presentation.layout.set(layout);
    game.presentation.selection.units = vec![fighter];
    (game, InputState::new(), chip.center(), at)
}

/// A finger lifting where it landed, `after` seconds from the last.
fn tap_after(game: &mut Game, input: &mut InputState, p: Vec2, after: f64) {
    input.now += after;
    apply_events(game, input, &[touch_down(1, p)]);
    input.now += 0.02;
    apply_events(game, input, &[touch_up(1, p)]);
}

#[test]
fn queue_taps_like_the_ipad_shift_key() {
    let (mut game, mut input, chip, _) = latch_range();
    tap_after(&mut game, &mut input, chip, 1.0);
    assert!(
        matches!(input.queue_toggle, QueueToggle::Latched { .. }),
        "one tap latches it"
    );
    tap_after(&mut game, &mut input, chip, 1.0);
    assert_eq!(input.queue_toggle, QueueToggle::Off, "a later tap lets go");

    tap_after(&mut game, &mut input, chip, 1.0);
    tap_after(&mut game, &mut input, chip, 0.1);
    assert_eq!(
        input.queue_toggle,
        QueueToggle::Locked,
        "a double-tap locks it"
    );
    tap_after(&mut game, &mut input, chip, 0.1);
    assert_eq!(
        input.queue_toggle,
        QueueToggle::Off,
        "and any tap unlocks it"
    );
}

#[test]
fn a_latched_queue_covers_one_order_outside_a_mode() {
    let (mut game, mut input, chip, at) = latch_range();
    tap_after(&mut game, &mut input, chip, 1.0);
    // Panning the camera is not an action.
    apply_events(
        &mut game,
        &mut input,
        &[
            touch_down(2, vec2(600.0, 300.0)),
            touch_move(2, vec2(500.0, 300.0)),
        ],
    );
    apply_events(&mut game, &mut input, &[touch_up(2, vec2(500.0, 300.0))]);
    assert!(matches!(input.queue_toggle, QueueToggle::Latched { .. }));

    long_press_world(&mut game, &mut input, at + vec2(4.0, 2.0));
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::Advance { queue: true, .. })),
        "the order queued: {:?}",
        game.pending
    );
    assert_eq!(input.queue_toggle, QueueToggle::Off, "and QUEUE let go");

    game.pending.clear();
    long_press_world(&mut game, &mut input, at + vec2(5.0, 2.0));
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::Advance { queue: false, .. })),
        "the next order replaces"
    );
}

#[test]
fn a_latched_queue_covers_one_selection_tap() {
    let (mut game, mut input, chip, _) = latch_range();
    let other = game
        .state
        .units()
        .iter()
        .find(|u| u.player == game.presentation.human && u.kind == UnitKind::Harvester)
        .map(|u| vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>()))
        .expect("a harvester");
    tap_after(&mut game, &mut input, chip, 1.0);
    tap_world(&mut game, &mut input, other);
    assert_eq!(game.presentation.selection.units.len(), 2, "the tap added");
    assert_eq!(input.queue_toggle, QueueToggle::Off);
}

#[test]
fn a_latched_queue_lasts_a_whole_placement_chain() {
    let kind = oxide_sim::BuildingKind::Barricade;
    let (mut game, mut input, _) = armed_placement(kind);
    let chip = macroquad::math::Rect::new(160.0, 620.0, 96.0, 44.0);
    let ribbon = macroquad::math::Rect::new(500.0, 620.0, 240.0, 44.0);
    let mut layout = bare_layout(f32::INFINITY, 0.0);
    layout.queue_toggle = chip;
    layout.mode_ribbon = ribbon;
    game.presentation.layout.set(layout);
    let foundry = game.state.buildings()[0].anchor;

    tap_after(&mut game, &mut input, chip.center(), 1.0);
    for dx in [3.5, 5.5] {
        let ground = vec2(foundry.x as f32 + dx, foundry.y as f32 + 3.5);
        tap_world(&mut game, &mut input, ground);
        tap_world(&mut game, &mut input, ground);
    }
    assert_eq!(staged_anchors(&game).len(), 2, "both ghosts built");
    assert!(
        game.pending
            .iter()
            .all(|c| matches!(c.command, Command::Build { queue: true, .. })),
        "and both queued"
    );
    assert!(matches!(input.queue_toggle, QueueToggle::Latched { .. }));
    assert!(input.placing.is_some(), "still placing");

    tap_after(&mut game, &mut input, ribbon.center(), 1.0);
    assert!(input.placing.is_none(), "the ribbon ends placement");
    assert_eq!(input.queue_toggle, QueueToggle::Off, "and QUEUE with it");
}

#[test]
fn a_locked_queue_outlasts_orders() {
    let (mut game, mut input, chip, at) = latch_range();
    tap_after(&mut game, &mut input, chip, 1.0);
    tap_after(&mut game, &mut input, chip, 0.1);
    long_press_world(&mut game, &mut input, at + vec2(4.0, 2.0));
    long_press_world(&mut game, &mut input, at + vec2(5.0, 2.0));
    let queued = game
        .pending
        .iter()
        .filter(|c| matches!(c.command, Command::Advance { queue: true, .. }))
        .count();
    assert_eq!(queued, 2, "both orders queued");
    assert_eq!(input.queue_toggle, QueueToggle::Locked);
}

#[test]
fn only_a_latched_queue_takes_a_snapshot() {
    let (game, mut input, chip, _) = latch_range();
    assert!(
        input.latch_probe(&game).is_none(),
        "QUEUE off copies nothing"
    );
    input.queue_toggle = QueueToggle::Locked;
    assert!(
        input.latch_probe(&game).is_none(),
        "nor does a locked QUEUE"
    );
    input.queue_toggle = QueueToggle::Latched { at: 0.0 };
    assert!(input.latch_probe(&game).is_some());
}
