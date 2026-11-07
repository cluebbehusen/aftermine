//! A staged sandbox for controller tests: open ground with the camera at a
//! known pose, and pad events fed as the frame loop feeds them.

use super::super::{InputState, Pointer, apply_events, reticle, track_hands, update_held};
use crate::action::BindingMap;
use crate::game::Game;
use aftermine_protocol::{PadButton, RawEvent, Stick};
use aftermine_sim::{BuildingId, BuildingKind, UnitId, UnitKind};
use macroquad::prelude::{Vec2, vec2};

/// The scripted controller's id.
pub(crate) const PAD: u64 = 1;
/// Pixels per tile: a tile is 32 logical pixels, so the reticle's 24-pixel
/// reach is three quarters of a tile.
pub(crate) const ZOOM: f32 = 32.0;
/// Where the camera looks: the middle of the map, clear of every edge.
pub(crate) const CENTER: Vec2 = vec2(32.0, 20.0);
/// Seconds per simulated frame.
pub(crate) const FRAME: f32 = 1.0 / 60.0;

/// A 64 by 40 sandbox with scrap at (40, 20), seat 0 local and seats 1
/// and 2 passive, holding `units` and `buildings` as (seat, kind, x, y).
pub(crate) fn scene(
    units: &[(u8, UnitKind, i32, i32)],
    buildings: &[(u8, BuildingKind, i32, i32)],
) -> Game {
    let mut map = vec![".".repeat(64); 40];
    map[20].replace_range(40..41, "s");
    let scenario = aftermine_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Pad drill",
            "mode": "sandbox",
            "players": [
                {"name": "You", "scrap": 1000, "bot": false},
                {"name": "Red", "scrap": 0, "bot": false},
                {"name": "Blue", "scrap": 0, "bot": false}
            ],
            "map": map,
            "units": units
                .iter()
                .map(|(player, kind, x, y)| serde_json::json!({"player": player, "kind": kind, "x": x, "y": y}))
                .collect::<Vec<_>>(),
            "buildings": buildings
                .iter()
                .map(|(player, kind, x, y)| serde_json::json!({"player": player, "kind": kind, "x": x, "y": y}))
                .collect::<Vec<_>>(),
        })
        .to_string(),
    )
    .expect("the drill parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("the drill builds");
    game.presentation.camera.show(CENTER, ZOOM);
    game
}

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

/// The ids of `player`'s units of `kind`, in id order.
pub(crate) fn units_of(game: &Game, player: u8, kind: UnitKind) -> Vec<UnitId> {
    let mut ids: Vec<UnitId> = game
        .state
        .units()
        .iter()
        .filter(|u| u.player.0 == player && u.kind == kind)
        .map(|u| u.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// The unit standing on tile `(x, y)`.
pub(crate) fn unit_at(game: &Game, x: i32, y: i32) -> UnitId {
    game.state
        .units()
        .iter()
        .find(|u| u.tile() == chassis::grid::TilePos::new(x, y))
        .map(|u| u.id)
        .expect("a unit stands there")
}

/// Where a unit stands.
pub(crate) fn pos(game: &Game, id: UnitId) -> Vec2 {
    let unit = game.state.unit(id).expect("the unit lives");
    vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>())
}

/// The building anchored at `(x, y)`.
pub(crate) fn building_at(game: &Game, x: i32, y: i32) -> BuildingId {
    game.state
        .buildings()
        .iter()
        .find(|b| b.anchor == chassis::grid::TilePos::new(x, y))
        .map(|b| b.id)
        .expect("a building stands there")
}

/// Whether this frame played the refusal cue.
pub(crate) fn denied(game: &Game) -> bool {
    game.presentation
        .sounds_pending
        .contains(&(crate::game::SoundKind::Denied, None))
}

/// Takes `hp` off building `id` through the state's own serialization.
pub(crate) fn wound_building(game: &mut Game, id: BuildingId, hp: u64) {
    let mut data = serde_json::to_value(&*game.state).expect("state serializes");
    let building = data["buildings"]
        .as_array_mut()
        .expect("buildings")
        .iter_mut()
        .find(|b| b["id"] == serde_json::json!(id))
        .expect("the building is listed");
    building["hp"] = serde_json::json!(building["hp"].as_u64().expect("hp") - hp);
    *game.state = serde_json::from_value(data).expect("the wounded state loads");
}
