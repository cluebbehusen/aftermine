//! Paint select through the real funnel: pad events as the frame loop
//! feeds them, then frames of a held stick.

use super::super::tests::pad_scene::{
    FRAME, PAD, building_at, down, feed, frame, left_stick, pad_input, press, push_until, rest_on,
    scene, unit_at, units_of, up,
};
use super::*;
use aftermine_protocol::{PadButton, RawEvent};
use aftermine_sim::{BuildingKind, PlayerId, UnitKind};
use macroquad::prelude::vec2;

const SENTINEL: UnitKind = UnitKind::Sentinel;

/// Four own Sentinels in a row along y 20 from x 28, and one more three
/// tiles below the row's middle.
fn row_scene() -> crate::game::Game {
    scene(
        &[
            (0, SENTINEL, 28, 20),
            (0, SENTINEL, 29, 20),
            (0, SENTINEL, 30, 20),
            (0, SENTINEL, 31, 20),
            (0, SENTINEL, 30, 23),
        ],
        &[],
    )
}

fn row(game: &crate::game::Game) -> Vec<UnitId> {
    (28..=31).map(|x| unit_at(game, x, 20)).collect()
}

/// Holds A at `from` and sweeps right along y 20.5 until the reticle has
/// passed `to_x`, frame by frame of `dt` seconds. A stays down.
fn sweep_right(
    game: &mut crate::game::Game,
    input: &mut InputState,
    from: f32,
    to_x: f32,
    dt: f32,
) {
    rest_on(game, input, vec2(from, 20.5));
    feed(game, input, &[down(PadButton::South)]);
    push_until(game, input, (1.0, 0.0), dt, |ground| ground.x >= to_x);
}

#[test]
fn a_sweep_selects_what_the_reticle_crossed_once_a_comes_up() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    sweep_right(&mut game, &mut input, 26.5, 31.5, FRAME);
    assert!(
        game.presentation.selection.units.is_empty(),
        "nothing selects while A is held"
    );
    let preview = preview(&game.view(), &input).expect("the sweep shows");
    assert_eq!(preview.kept, Some(Kept::Units(PlayerId(0), row(&game))));
    assert_eq!(preview.brush, input.reticle.point());
    assert_eq!(
        preview.radius,
        super::super::reticle::reach(&input).unit_px(input.ui)
    );
    assert_eq!(preview.trail.len(), 1);
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(game.presentation.selection.units, row(&game));
    assert!(preview_gone(&game, &input));
}

#[test]
fn a_fast_stick_still_catches_everything_it_crosses() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    sweep_right(&mut game, &mut input, 22.5, 33.0, 0.5);
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(game.presentation.selection.units, row(&game));
}

#[test]
fn a_press_that_never_leaves_its_spot_paints_nothing() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(26.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    // Less than the reticle's reach: still a tap.
    push_until(&mut game, &mut input, (0.3, 0.0), FRAME, |g| g.x >= 26.9);
    assert!(preview_gone(&game, &input));
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert!(game.presentation.selection.units.is_empty());
}

#[test]
fn own_units_outrank_own_structures_and_foreign_units() {
    let mut game = scene(
        &[(1, SENTINEL, 30, 20), (0, SENTINEL, 32, 20)],
        &[(0, BuildingKind::Array, 26, 20)],
    );
    let array = building_at(&game, 26, 20);
    let mut input = pad_input(&mut game);
    sweep_right(&mut game, &mut input, 24.5, 27.5, FRAME);
    assert_eq!(
        preview(&game.view(), &input).and_then(|p| p.kept),
        Some(Kept::Buildings(PlayerId(0), vec![array])),
        "a structure alone is kept"
    );
    push_until(&mut game, &mut input, (1.0, 0.0), FRAME, |g| g.x >= 30.5);
    assert_eq!(
        preview(&game.view(), &input).and_then(|p| p.kept),
        Some(Kept::Buildings(PlayerId(0), vec![array])),
        "an own structure outranks a foreign unit"
    );
    push_until(&mut game, &mut input, (1.0, 0.0), FRAME, |g| g.x >= 32.5);
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(
        game.presentation.selection.units,
        units_of(&game, 0, SENTINEL)
    );
    assert!(game.presentation.selection.buildings.is_empty());
}

#[test]
fn with_nothing_own_caught_one_foreign_owner_is_kept_in_id_order() {
    let mut game = scene(
        &[
            (1, SENTINEL, 31, 20),
            (2, SENTINEL, 28, 20),
            (1, SENTINEL, 29, 20),
            (2, SENTINEL, 30, 20),
            // Sight over the row, well clear of the sweep.
            (0, SENTINEL, 30, 25),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    sweep_right(&mut game, &mut input, 26.5, 31.5, FRAME);
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    let red = units_of(&game, 1, SENTINEL);
    assert_eq!(red.len(), 2);
    assert_eq!(game.presentation.selection.units, red);
}

#[test]
fn held_queue_adds_the_sweep_to_the_selection() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    let below = unit_at(&game, 30, 23);
    rest_on(&mut game, &mut input, vec2(30.5, 23.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(game.presentation.selection.units, vec![below]);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    sweep_right(&mut game, &mut input, 26.5, 31.5, FRAME);
    feed(
        &mut game,
        &mut input,
        &[up(PadButton::South), up(PadButton::LeftTrigger)],
    );
    let mut all = row(&game);
    all.push(below);
    all.sort_unstable();
    assert_eq!(game.presentation.selection.units, all);
}

#[test]
fn an_empty_sweep_clears_the_selection_unless_queue_is_held() {
    for queue in [false, true] {
        let mut game = row_scene();
        let mut input = pad_input(&mut game);
        let below = unit_at(&game, 30, 23);
        game.presentation.selection.units = vec![below];
        if queue {
            feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
        }
        rest_on(&mut game, &mut input, vec2(26.5, 30.5));
        feed(&mut game, &mut input, &[down(PadButton::South)]);
        push_until(&mut game, &mut input, (1.0, 0.0), FRAME, |g| g.x >= 31.5);
        feed(&mut game, &mut input, &[up(PadButton::South)]);
        let expected = if queue { vec![below] } else { Vec::new() };
        assert_eq!(game.presentation.selection.units, expected, "queue {queue}");
    }
}

#[test]
fn b_abandons_a_sweep_and_keeps_the_selection() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    let below = unit_at(&game, 30, 23);
    game.presentation.selection.units = vec![below];
    sweep_right(&mut game, &mut input, 26.5, 31.5, FRAME);
    assert!(!preview_gone(&game, &input));
    press(&mut game, &mut input, PadButton::East);
    assert!(preview_gone(&game, &input));
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(
        game.presentation.selection.units,
        vec![below],
        "B backed out of the sweep alone"
    );
}

#[test]
fn a_camera_jump_mid_sweep_paints_no_ground_between() {
    let mut game = scene(
        &[
            (0, SENTINEL, 28, 20),
            (0, SENTINEL, 29, 20),
            (0, SENTINEL, 36, 20),
            (0, SENTINEL, 45, 20),
            (0, SENTINEL, 46, 20),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    sweep_right(&mut game, &mut input, 26.5, 29.5, FRAME);
    rest_on(&mut game, &mut input, vec2(43.5, 20.5));
    push_until(&mut game, &mut input, (1.0, 0.0), FRAME, |g| g.x >= 46.5);
    assert_eq!(
        preview(&game.view(), &input).map(|p| p.trail.len()),
        Some(2)
    );
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    let caught: Vec<UnitId> = [(28, 20), (29, 20), (45, 20), (46, 20)]
        .iter()
        .map(|&(x, y)| unit_at(&game, x, y))
        .collect();
    assert_eq!(game.presentation.selection.units, caught);
}

#[test]
fn a_jump_before_the_sweep_begins_carries_the_press_along() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(20.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    rest_on(&mut game, &mut input, vec2(26.5, 20.5));
    assert!(preview_gone(&game, &input), "the jump swept nothing");
    push_until(&mut game, &mut input, (1.0, 0.0), FRAME, |g| g.x >= 31.5);
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(game.presentation.selection.units, row(&game));
}

#[test]
fn chrome_over_the_reticle_hides_the_ground_it_would_paint() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(26.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    let mut layout = game.presentation.layout.get();
    layout.performance = macroquad::prelude::Rect::new(0.0, 0.0, 1280.0, 800.0);
    game.presentation.layout.set(layout);
    push_until(&mut game, &mut input, (1.0, 0.0), FRAME, |g| g.x >= 31.5);
    assert_eq!(preview(&game.view(), &input).and_then(|p| p.kept), None);
}

#[test]
fn another_pointer_or_a_lost_pad_ends_the_sweep() {
    for ending in [
        RawEvent::MouseMove { x: 5.0, y: 5.0 },
        RawEvent::PadDisconnected { pad: PAD },
    ] {
        let mut game = row_scene();
        let mut input = pad_input(&mut game);
        sweep_right(&mut game, &mut input, 26.5, 31.5, FRAME);
        feed(&mut game, &mut input, &[ending]);
        assert!(input.pad_press.is_none(), "{ending:?}");
        feed(&mut game, &mut input, &[up(PadButton::South)]);
        assert!(game.presentation.selection.units.is_empty(), "{ending:?}");
    }
}

#[test]
fn only_the_pad_holding_a_ends_its_press() {
    let mut game = row_scene();
    let mut input = pad_input(&mut game);
    sweep_right(&mut game, &mut input, 26.5, 31.5, FRAME);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::PadUp {
            pad: PAD + 1,
            button: PadButton::South,
        }],
    );
    assert!(input.pad_press.is_some());
    assert!(game.presentation.selection.units.is_empty());
}

#[test]
fn the_trail_keeps_a_bounded_number_of_points() {
    let mut sweep = Sweep::at(vec2(0.0, 0.0), 0.0);
    for step in 1..=2 * TRAIL_CAP {
        sweep.extend_trail(vec2(step as f32, 0.0), 32.0, 1.0);
    }
    assert_eq!(sweep.trail.iter().map(Vec::len).sum::<usize>(), TRAIL_CAP);
    // Points closer than a step apart are not laid.
    let mut close = Sweep::at(vec2(0.0, 0.0), 0.0);
    close.extend_trail(vec2(0.05, 0.0), 32.0, 1.0);
    assert_eq!(close.trail, vec![vec![vec2(0.0, 0.0)]]);
}

/// Holds the stick still, frame by frame, until the wall clock reads
/// `until` seconds.
fn wait_until(game: &mut crate::game::Game, input: &mut InputState, until: f64) {
    while input.now < until {
        input.now += f64::from(FRAME);
        frame(game, input, FRAME);
    }
}

#[test]
fn holding_a_still_grows_the_brush_to_its_full_size() {
    // Own Sentinels 0, 1, 2 and 4 tiles right of the reticle.
    let mut game = scene(
        &[
            (0, SENTINEL, 32, 20),
            (0, SENTINEL, 33, 20),
            (0, SENTINEL, 34, 20),
            (0, SENTINEL, 36, 20),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    let near: Vec<UnitId> = (32..=34).map(|x| unit_at(&game, x, 20)).collect();
    let kept = |game: &crate::game::Game, input: &InputState| {
        preview(&game.view(), input).and_then(|p| p.kept)
    };
    rest_on(&mut game, &mut input, vec2(32.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    wait_until(&mut game, &mut input, TAP_SECS / 2.0);
    assert!(preview_gone(&game, &input), "still a tap");
    wait_until(&mut game, &mut input, TAP_SECS + 0.05);
    assert_eq!(
        kept(&game, &input),
        Some(Kept::Units(PlayerId(0), near[..1].to_vec())),
        "held past a tap, it paints from the reticle's reach"
    );
    wait_until(&mut game, &mut input, TAP_SECS + GROW_SECS / 2.0);
    assert_eq!(
        kept(&game, &input),
        Some(Kept::Units(PlayerId(0), near[..2].to_vec()))
    );
    wait_until(&mut game, &mut input, TAP_SECS + GROW_SECS * 2.0);
    let full = preview(&game.view(), &input).expect("the sweep shows");
    assert_eq!(full.radius, BRUSH * input.ui, "it stops at its full size");
    assert_eq!(full.kept, Some(Kept::Units(PlayerId(0), near.clone())));
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(game.presentation.selection.units, near);
}

#[test]
fn a_release_held_past_a_tap_paints_even_before_a_frame_draws_it() {
    // Two Sentinels, each half a tile from the reticle.
    let mut game = scene(&[(0, SENTINEL, 32, 20), (0, SENTINEL, 33, 20)], &[]);
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(33.0, 20.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        game.presentation.selection.units.len(),
        1,
        "a tap picks one"
    );
    input.now += 1.0;
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    input.now += TAP_SECS * 1.5;
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(
        game.presentation.selection.units,
        units_of(&game, 0, SENTINEL),
        "a hold paints both"
    );
}

#[test]
fn nothing_the_reticle_rests_on_slows_a_sweep() {
    let mut game = scene(&[(0, SENTINEL, 32, 20)], &[]);
    let mut input = pad_input(&mut game);
    // How far one frame of a half-pushed stick carries the reticle off the
    // Sentinel it rests on.
    let step = |game: &mut crate::game::Game, input: &mut InputState| {
        let from = input.reticle.ground(&game.presentation.camera);
        feed(game, input, &[left_stick(0.5, 0.0)]);
        frame(game, input, 0.1);
        feed(game, input, &[left_stick(0.0, 0.0)]);
        input.reticle.ground(&game.presentation.camera).x - from.x
    };
    rest_on(&mut game, &mut input, vec2(32.5, 20.5));
    assert!(input.reticle.snap().is_some());
    let resting = step(&mut game, &mut input);
    rest_on(&mut game, &mut input, vec2(32.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    input.now += TAP_SECS;
    frame(&mut game, &mut input, 0.0);
    assert!(painting(&input));
    assert!(input.reticle.snap().is_some());
    let painting_step = step(&mut game, &mut input);
    assert!(
        painting_step > resting * 1.9,
        "a sweep glides at full speed: {painting_step} against {resting} at rest"
    );
}

fn preview_gone(game: &crate::game::Game, input: &InputState) -> bool {
    preview(&game.view(), input).is_none()
}

#[test]
fn a_full_trail_starts_no_new_stroke() {
    let mut sweep = Sweep::at(vec2(0.0, 0.0), 0.0);
    for step in 1..TRAIL_CAP {
        sweep.extend_trail(vec2(step as f32, 0.0), 32.0, 1.0);
    }
    sweep.break_stroke(vec2(-5.0, 0.0));
    assert_eq!(sweep.trail.len(), 1, "a jump past the cap lays no stroke");
}
