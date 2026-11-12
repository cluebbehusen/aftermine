//! Pad events fed as the frame loop feeds them, for controller tests on
//! [`crate::tests::support::scene`].

use super::super::{InputState, Pointer, apply_events, reticle, track_hands, update_held};
use crate::action::BindingMap;
use crate::game::Game;
use aftermine_protocol::{PadButton, RawEvent, Stick};
use macroquad::prelude::Vec2;

/// The scripted controller's id.
pub(crate) const PAD: u64 = 1;
/// Seconds per simulated frame.
pub(crate) const FRAME: f32 = 1.0 / 60.0;

/// A fresh controller in use, its reticle resting on the camera's center.
pub(crate) fn pad_input(game: &mut Game) -> InputState {
    let mut input = InputState::new();
    feed(
        game,
        &mut input,
        &[RawEvent::PadConnected {
            pad: PAD,
            family: aftermine_protocol::PadFamily::Xbox,
        }],
    );
    input.last_pointer = Pointer::Pad;
    frame(game, &mut input, 0.0);
    input
}

/// Feeds events as the frame loop does on the Playing screen: every screen
/// notes the hands, then the match applies them.
pub(crate) fn feed(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    for event in events {
        track_hands(input, event, false);
    }
    apply_events(game, input, &BindingMap::classic(), events, |_, _| {});
}

/// One frame of held input.
pub(crate) fn frame(game: &mut Game, input: &mut InputState, dt: f32) {
    update_held(game, input, dt);
}

pub(crate) fn down(button: PadButton) -> RawEvent {
    RawEvent::PadDown { pad: PAD, button }
}

pub(crate) fn up(button: PadButton) -> RawEvent {
    RawEvent::PadUp { pad: PAD, button }
}

/// A full press: down, then up.
pub(crate) fn press(game: &mut Game, input: &mut InputState, button: PadButton) {
    feed(game, input, &[down(button), up(button)]);
}

pub(crate) fn left_stick(x: f32, y: f32) -> RawEvent {
    RawEvent::PadStick {
        pad: PAD,
        stick: Stick::Left,
        x,
        y,
    }
}

/// Rests the reticle on `world` as any camera jump does, and lets a frame
/// settle what it snaps onto.
pub(crate) fn rest_on(game: &mut Game, input: &mut InputState, world: Vec2) {
    reticle::look_at(game, input, world);
    frame(game, input, 0.0);
}

/// Pushes the left stick by `(x, y)` until the reticle's ground has passed
/// `until`, frame by frame of `dt` seconds, then lets the stick go.
pub(crate) fn push_until(
    game: &mut Game,
    input: &mut InputState,
    (x, y): (f32, f32),
    dt: f32,
    until: impl Fn(Vec2) -> bool,
) {
    feed(game, input, &[left_stick(x, y)]);
    for _ in 0..10_000 {
        if until(input.reticle.ground(&game.presentation.camera)) {
            break;
        }
        frame(game, input, dt);
    }
    feed(game, input, &[left_stick(0.0, 0.0)]);
    assert!(
        until(input.reticle.ground(&game.presentation.camera)),
        "the stick never brought the reticle there"
    );
}
