use super::*;

fn circle(center: Vec2, radius: f32, paint: f32) -> Frame {
    Frame {
        center,
        half: Vec2::splat(radius),
        round: radius,
        focus: 0.0,
        paint,
    }
}

#[test]
fn the_first_cursor_frame_starts_at_its_target_even_at_zero() {
    let mut motion = Motion::default();
    let target = circle(Vec2::ZERO, 11.0, 0.0);
    assert_eq!(motion.sample(target, 1.0, false), target);
    let moved = circle(vec2(10.0, 20.0), 11.0, 0.0);
    let frame = motion.sample(moved, 1.01, false);
    assert!(frame.center.x > 0.0 && frame.center.x < moved.center.x);
}

#[test]
fn repeated_draws_at_one_instant_do_not_advance_the_cursor() {
    let mut motion = Motion::default();
    motion.sample(circle(Vec2::ZERO, 11.0, 0.0), 0.0, false);
    let target = circle(vec2(30.0, 0.0), 20.0, 0.0);
    let first = motion.sample(target, 0.05, false);
    assert_eq!(motion.sample(target, 0.05, false), first);
}

#[test]
fn a_live_brush_draws_the_exact_center_and_radius_it_catches_with() {
    let mut motion = Motion::default();
    motion.sample(circle(Vec2::ZERO, 11.0, 0.0), 0.0, false);
    let brush = circle(vec2(200.0, 100.0), 60.0, 1.0);
    let frame = motion.sample(brush, 0.01, false);
    assert_eq!(frame.center, brush.center);
    assert_eq!(frame.half, brush.half);
    assert_eq!(frame.round, brush.round);
    assert!(frame.paint > 0.0 && frame.paint < 1.0);
}

#[test]
fn release_returns_to_idle_without_a_radius_jump() {
    let mut motion = Motion::default();
    let brush = circle(vec2(200.0, 100.0), 96.0, 1.0);
    motion.sample(brush, 0.0, false);
    let idle = circle(brush.center, 11.0, 0.0);
    let frame = motion.sample(idle, 0.01, false);
    assert!(frame.half.x > idle.half.x && frame.half.x < brush.half.x);
    assert!(frame.paint > 0.0 && frame.paint < 1.0);
    let settled = motion.sample(idle, 0.02, true);
    assert_eq!(settled, idle);
}

#[test]
fn cursor_easing_depends_on_elapsed_time_rather_than_frame_count() {
    let initial = circle(Vec2::ZERO, 11.0, 0.0);
    let target = circle(vec2(50.0, 25.0), 30.0, 0.0);
    let mut single = Motion::default();
    single.sample(initial, 0.0, false);
    let one = single.sample(target, 0.08, false);
    let mut divided = Motion::default();
    divided.sample(initial, 0.0, false);
    let mut many = initial;
    for step in 1..=8 {
        many = divided.sample(target, f64::from(step) * 0.01, false);
    }
    assert!((one.center - many.center).length() < 1e-4);
    assert!((one.half - many.half).length() < 1e-4);
    assert!((one.round - many.round).abs() < 1e-4);
}

#[test]
fn a_hidden_cursor_or_an_interrupted_clock_starts_fresh() {
    let mut motion = Motion::default();
    motion.sample(circle(Vec2::ZERO, 96.0, 1.0), 2.0, false);
    let idle = circle(vec2(300.0, 200.0), 11.0, 0.0);
    motion.clear();
    assert_eq!(motion.sample(idle, 2.01, false), idle);
    assert_eq!(motion.sample(idle, 3.0, false), idle);
    let elsewhere = circle(Vec2::ZERO, 11.0, 0.0);
    assert_eq!(motion.sample(elsewhere, 1.0, false), elsewhere);
}

#[test]
fn focus_follows_only_what_the_controller_can_pick() {
    use crate::tests::support::scene;
    use aftermine_sim::UnitKind;
    let mut game = scene(&[(0, UnitKind::Sentinel, 32, 20)], &[]);
    let mut input = InputState::new();
    input.last_pointer = Pointer::Pad;
    game.presentation.camera.show(vec2(32.5, 20.5), 32.0);
    crate::input::update_held(&mut game, &mut input, 0.0);
    let focused = target(&game.view(), &input, input.ui);
    assert_eq!(focused.focus, 1.0);
    assert_eq!(focused.center, input.reticle.point());
    game.presentation.camera.show(vec2(38.5, 20.5), 32.0);
    crate::input::update_held(&mut game, &mut input, 0.0);
    let idle = target(&game.view(), &input, input.ui);
    assert_eq!(idle.focus, 0.0);
    assert_eq!(idle.half, Vec2::splat(11.0 * input.ui));
}
