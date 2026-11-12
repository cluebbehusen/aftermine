use super::*;

/// A selected Harvester with scrap for a row of turrets on open, visible
/// ground, and a second one standing by.
fn funded_builder() -> (Game, InputState) {
    let scenario = aftermine_sim::Scenario::from_json(
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
                {"player": 0, "kind": "harvester", "x": 5, "y": 3},
                {"player": 0, "kind": "harvester", "x": 3, "y": 5}
            ]
        })
        .to_string(),
    )
    .expect("chain range parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("builds");
    let mut input = InputState::new();
    game.presentation.selection.units = vec![game.state.units()[0].id];
    input.arm_placement(aftermine_sim::BuildingKind::Turret);
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

/// Shift-clicks two turrets, then lets go of Shift.
fn shift_chain(game: &mut Game, input: &mut InputState) {
    let (a, b) = (at(game, 7, 3), at(game, 9, 3));
    apply_events(
        game,
        input,
        &[
            key_down(Key::Shift),
            left_down(a),
            left_up(a),
            left_down(b),
            left_up(b),
            key_up(Key::Shift),
        ],
    );
}

const CHAIN: [(TilePos, bool); 2] = [(TilePos::new(7, 3), true), (TilePos::new(9, 3), true)];

#[test]
fn letting_go_of_shift_mid_chain_keeps_placing_until_a_plain_click() {
    let (mut game, mut input) = funded_builder();
    shift_chain(&mut game, &mut input);
    assert_eq!(builds(&game), CHAIN, "premise: Shift chains");
    assert!(
        input.placing().is_some(),
        "letting go of Shift keeps placing"
    );

    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Shift), key_up(Key::Shift)],
    );
    assert!(input.placing().is_some(), "and so does a bare Shift tap");

    // Shift is up, so the click is a plain build: it replaces the
    // builder's orders, the chain included, and ends placement.
    let c = at(&game, 11, 3);
    apply_events(&mut game, &mut input, &[left_down(c), left_up(c)]);
    assert_eq!(builds(&game)[2..], [(TilePos::new(11, 3), false)]);
    assert!(input.placing().is_none(), "a plain placement ends the mode");
    assert!(input.build_menu, "and the palette stays for the next pick");
}

#[test]
fn letting_go_of_shift_before_anything_is_placed_keeps_placement_armed() {
    let (mut game, mut input) = funded_builder();
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Shift), key_up(Key::Shift)],
    );
    assert!(
        input.placing().is_some(),
        "a bare Shift tap changes nothing"
    );

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
    assert!(input.placing().is_some(), "a refusal leaves the mode armed");
}

#[test]
fn escape_after_a_chain_leaves_placement_and_keeps_the_chain() {
    let (mut game, mut input) = funded_builder();
    shift_chain(&mut game, &mut input);
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Escape), key_up(Key::Escape)],
    );
    assert!(input.placing().is_none(), "Esc leaves placement");
    assert!(input.build_menu, "back to the palette");
    assert_eq!(builds(&game), CHAIN, "without placing or undoing a site");
}

#[test]
fn the_ribbon_s_cross_after_a_chain_leaves_placement_and_keeps_the_chain() {
    let (mut game, mut input) = funded_builder();
    let cross = macroquad::math::Rect::new(460.0, 624.0, 36.0, 36.0);
    let mut layout = bare_layout(f32::INFINITY, 0.0);
    layout.mode_ribbon = macroquad::math::Rect::new(220.0, 620.0, 280.0, 44.0);
    layout.ribbon_cancel = cross;
    game.presentation.layout.set(layout);
    shift_chain(&mut game, &mut input);
    assert!(input.placing().is_some(), "premise: still placing");

    apply_events(
        &mut game,
        &mut input,
        &click(cross.center().x, cross.center().y),
    );
    assert!(input.placing().is_none(), "the cross ends placement");
    assert_eq!(builds(&game), CHAIN, "and places nothing");
}

#[test]
fn letting_go_of_shift_mid_stroke_keeps_placing() {
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
    let staged = builds(&game);
    assert_eq!(staged.len(), 3, "the stroke kept stamping");
    assert!(
        staged.iter().all(|(_, queue)| *queue),
        "queued, as the press was: {staged:?}"
    );
    assert!(
        input.placing().is_some(),
        "a stroke begun under Shift keeps placing"
    );
}

#[test]
fn turning_queue_off_after_a_queued_confirm_keeps_touch_placement() {
    let (mut game, mut input) = funded_builder();
    let chip = macroquad::math::Rect::new(160.0, 620.0, 96.0, 44.0);
    let mut layout = bare_layout(f32::INFINITY, 0.0);
    layout.queue_toggle = chip;
    game.presentation.layout.set(layout);
    let ground = vec2(7.5, 3.5);

    input.queue_toggle = QueueToggle::Locked;
    tap_world(&mut game, &mut input, ground);
    tap_check(&mut game, &mut input);
    assert_eq!(builds(&game).len(), 1, "premise: the queued confirm built");
    let next = input.ghost_anchor().expect("the ghost moves on");

    tap(&mut game, &mut input, chip.center());
    assert_eq!(input.queue_toggle, QueueToggle::Off);
    assert!(input.placing().is_some(), "turning QUEUE off keeps placing");
    assert_eq!(
        input.ghost_anchor(),
        Some(next),
        "with the ghost where it was"
    );

    // QUEUE is off, so the check mark is a plain build that ends placement.
    tap_check(&mut game, &mut input);
    assert_eq!(builds(&game)[1..], [(next, false)]);
    assert!(input.placing().is_none(), "a plain confirm ends the mode");
}

#[test]
fn dropping_a_stroke_after_letting_go_of_shift_keeps_placing() {
    let (mut game, mut input) = funded_builder();
    let (a, b, c) = (at(&game, 7, 3), at(&game, 9, 3), at(&game, 10, 3));
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Shift), left_down(a), left_up(a)],
    );
    assert_eq!(builds(&game).len(), 1, "premise: a chain has staged");
    apply_events(
        &mut game,
        &mut input,
        &[left_down(b), mouse_move(c), key_up(Key::Shift)],
    );
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Escape), key_up(Key::Escape)],
    );
    assert!(input.planned_stamps().is_empty(), "Esc drops the stroke");
    assert!(input.placing().is_some(), "and keeps placing");
    assert_eq!(builds(&game).len(), 1);
}

#[test]
fn a_fully_refused_plain_release_after_a_chain_keeps_placing() {
    let (mut game, mut input) = funded_builder();
    let (a, b) = (at(&game, 7, 3), at(&game, 9, 3));
    apply_events(
        &mut game,
        &mut input,
        &[
            key_down(Key::Shift),
            left_down(a),
            left_up(a),
            key_up(Key::Shift),
        ],
    );
    apply_events(&mut game, &mut input, &[left_down(b)]);
    // The other Harvester founds a site on the planned ground before the
    // release. The builder's own would go with the orders a plain build
    // replaces.
    let builder = game.presentation.selection.units[0];
    let other = game
        .state
        .units()
        .iter()
        .map(|unit| unit.id)
        .find(|id| *id != builder)
        .expect("a second Harvester");
    game.issue(Command::Build {
        units: vec![other],
        kind: aftermine_sim::BuildingKind::Barricade,
        anchor: TilePos::new(9, 3),
        queue: true,
        defer: false,
    });
    game.advance_ticks(1);
    apply_events(&mut game, &mut input, &[left_up(b)]);
    assert!(
        builds(&game).is_empty(),
        "premise: the release staged nothing"
    );
    assert!(
        input.placing().is_some(),
        "nothing staged, so the refusal keeps placing"
    );
}

#[test]
fn a_mouse_placement_leaves_the_palette_up() {
    let (mut game, mut input) = funded_builder();
    let a = at(&game, 7, 3);
    apply_events(&mut game, &mut input, &[left_down(a), left_up(a)]);
    assert_eq!(builds(&game), [(TilePos::new(7, 3), false)]);
    assert!(input.placing().is_none(), "one placement ends the mode");
    assert!(
        input.build_menu,
        "the drawn palette stays for the next pick"
    );
}

#[test]
fn a_mouse_placement_under_a_held_left_trigger_leaves_the_palette_up() {
    use aftermine_protocol::{PadButton, PadFamily};
    let (mut game, mut input) = funded_builder();
    let a = at(&game, 7, 3);
    let pad = 1;
    let feed = |game: &mut Game, input: &mut InputState, events: &[RawEvent]| {
        for event in events {
            track_hands(input, event, false);
        }
        apply_events(game, input, events);
        input.settle_hud();
    };
    feed(
        &mut game,
        &mut input,
        &[
            RawEvent::PadConnected {
                pad,
                family: PadFamily::Xbox,
            },
            RawEvent::PadDown {
                pad,
                button: PadButton::LeftTrigger,
            },
        ],
    );
    feed(&mut game, &mut input, &[left_down(a), left_up(a)]);
    assert_eq!(builds(&game), [(TilePos::new(7, 3), true)]);
    assert!(input.placing().is_some(), "the held trigger keeps placing");
    assert!(!input.hud_pad, "the click handed the HUD to the mouse");
    feed(
        &mut game,
        &mut input,
        &[RawEvent::PadUp {
            pad,
            button: PadButton::LeftTrigger,
        }],
    );
    assert!(input.placing().is_some(), "letting go keeps placing");
    let b = at(&game, 9, 3);
    feed(&mut game, &mut input, &[left_down(b), left_up(b)]);
    assert_eq!(builds(&game)[1..], [(TilePos::new(9, 3), false)]);
    assert!(input.placing().is_none(), "a plain click ends the mode");
    assert!(
        input.construction_open(),
        "the drawn palette stays for the next pick"
    );
}
