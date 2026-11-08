use super::*;
use macroquad::prelude::vec2;

fn taps(swipe: &mut Swipe, path: &[(f32, f32)]) -> Vec<PadButton> {
    path.iter()
        .filter_map(|&(x, y)| swipe.feed(vec2(x, y), false).0)
        .collect()
}

#[test]
fn touching_and_lifting_tap_nothing() {
    let mut swipe = Swipe::default();
    assert_eq!(taps(&mut swipe, &[(0.9, -0.9), (0.0, 0.0)]), []);
}

#[test]
fn a_touch_beginning_anywhere_measures_from_where_it_began() {
    let mut swipe = Swipe::default();
    assert_eq!(
        taps(&mut swipe, &[(-0.8, 0.5), (-0.6, 0.5), (-0.3, 0.5)]),
        [PadButton::DpadRight]
    );
}

#[test]
fn each_step_of_travel_taps_once_in_its_direction() {
    for (axis, forward, backward) in [
        (vec2(1.0, 0.0), PadButton::DpadRight, PadButton::DpadLeft),
        (vec2(0.0, 1.0), PadButton::DpadDown, PadButton::DpadUp),
    ] {
        let mut swipe = Swipe::default();
        let along = |at: f32| (axis.x * at + 0.05, axis.y * at + 0.05);
        let path = [along(0.0), along(0.45), along(0.9)];
        assert_eq!(taps(&mut swipe, &path), [forward, forward]);
        let back = [along(0.3)];
        assert_eq!(taps(&mut swipe, &back), [backward]);
    }
}

#[test]
fn a_long_jump_between_reads_taps_on_following_reads() {
    let mut swipe = Swipe::default();
    let path = [(-0.9, 0.2), (0.9, 0.2), (0.9, 0.2), (0.9, 0.2), (0.9, 0.2)];
    assert_eq!(taps(&mut swipe, &path), [PadButton::DpadRight; 4]);
}

#[test]
fn a_resting_thumb_taps_nothing_more() {
    let mut swipe = Swipe::default();
    assert_eq!(
        taps(&mut swipe, &[(0.1, 0.1), (0.1, 0.6)]),
        [PadButton::DpadDown]
    );
    for _ in 0..100 {
        assert_eq!(swipe.feed(vec2(0.1, 0.6), false), (None, Vec2::ZERO));
    }
}

#[test]
fn a_swipe_bent_off_line_never_taps_sideways() {
    let mut swipe = Swipe::default();
    let path = [(0.0, 0.1), (0.3, 0.5), (0.1, 0.9), (0.39, 1.0)];
    assert!(
        taps(&mut swipe, &path)
            .iter()
            .all(|button| *button == PadButton::DpadDown)
    );
}

#[test]
fn a_perfect_diagonal_reads_vertical() {
    let mut swipe = Swipe::default();
    assert_eq!(
        taps(&mut swipe, &[(0.1, 0.1), (0.6, 0.6)]),
        [PadButton::DpadDown]
    );
}

#[test]
fn a_click_moves_nothing_and_travel_counts_from_where_it_ended() {
    let mut swipe = Swipe::default();
    assert_eq!(swipe.feed(vec2(0.1, 0.1), false).0, None);
    assert_eq!(swipe.feed(vec2(0.1, 0.8), true), (None, Vec2::ZERO));
    assert_eq!(swipe.feed(vec2(0.1, 0.8), false).0, None);
    assert_eq!(swipe.feed(vec2(0.1, 1.0), false).0, None);
}

#[test]
fn lifting_forgets_the_touch() {
    let mut swipe = Swipe::default();
    assert_eq!(taps(&mut swipe, &[(0.1, 0.1), (0.0, 0.0), (0.1, 0.45)]), []);
}

#[test]
fn travel_is_the_movement_between_reads_of_one_touch() {
    let mut swipe = Swipe::default();
    assert_eq!(swipe.feed(vec2(0.5, 0.5), false).1, Vec2::ZERO, "touching");
    assert_eq!(swipe.feed(vec2(0.75, 0.25), false).1, vec2(0.25, -0.25));
    assert_eq!(swipe.feed(vec2(0.75, 0.25), false).1, Vec2::ZERO, "resting");
    assert_eq!(swipe.feed(Vec2::ZERO, false).1, Vec2::ZERO, "lifting");
    assert_eq!(
        swipe.feed(vec2(-0.5, 0.5), false).1,
        Vec2::ZERO,
        "a new touch"
    );
}
