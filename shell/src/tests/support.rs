//! Staging the shell's unit tests share: open sandboxes, lookups by seat
//! and kind, pointer and key events, and the press gesture every screen
//! button keeps.

use crate::game::Game;
use aftermine_protocol::{Key, MouseButton, RawEvent};
use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::{BuildingId, BuildingKind, Command, Scenario, UnitId, UnitKind};
use macroquad::prelude::{Vec2, vec2};
use std::fmt::Debug;

/// Pixels per tile in [`scene`]: a tile is 32 logical pixels, so the
/// reticle's 24-pixel reach is three quarters of a tile.
pub(crate) const ZOOM: f32 = 32.0;
/// Where [`scene`]'s camera looks: the middle of the map, clear of every
/// edge.
pub(crate) const CENTER: Vec2 = vec2(32.0, 20.0);

/// Open ground `width` by `height` tiles in sandbox mode for `seats` seats,
/// none a bot and none holding scrap, with `units` and `buildings` as
/// (seat, kind, x, y).
pub(crate) fn sandbox(
    width: usize,
    height: usize,
    seats: u8,
    units: &[(u8, UnitKind, i32, i32)],
    buildings: &[(u8, BuildingKind, i32, i32)],
) -> Scenario {
    Scenario {
        mode: ScenarioMode::Sandbox,
        name: "Sandbox".to_owned(),
        map: vec![".".repeat(width); height],
        levels: Vec::new(),
        players: (1..=seats)
            .map(|seat| PlayerSpec {
                name: format!("Seat {seat}"),
                team: None,
                scrap: 0,
                bot: false,
                bot_config: None,
            })
            .collect(),
        units: units
            .iter()
            .map(|&(player, kind, x, y)| UnitSpec { player, kind, x, y })
            .collect(),
        buildings: buildings
            .iter()
            .map(|&(player, kind, x, y)| BuildingSpec { player, kind, x, y })
            .collect(),
        meta: None,
    }
}

/// A 64 by 40 [`sandbox`] with scrap at (40, 20), seat 0 local with 1,000
/// scrap and seats 1 and 2 passive, and the camera on [`CENTER`] at
/// [`ZOOM`].
pub(crate) fn scene(
    units: &[(u8, UnitKind, i32, i32)],
    buildings: &[(u8, BuildingKind, i32, i32)],
) -> Game {
    let mut scenario = sandbox(64, 40, 3, units, buildings);
    scenario.players[0].scrap = 1000;
    scenario.map[20].replace_range(40..41, "s");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("the scene builds");
    game.presentation.camera.show(CENTER, ZOOM);
    game
}

/// Issues `command` for the local seat and runs one tick.
pub(crate) fn issue_tick(game: &mut Game, command: Command) {
    game.issue(command);
    game.do_tick();
}

/// The local seat's first unit of `kind`.
#[track_caller]
pub(crate) fn own_unit(game: &Game, kind: UnitKind) -> UnitId {
    game.state
        .units()
        .iter()
        .find(|unit| unit.player == game.presentation.human && unit.kind == kind)
        .expect("the local seat fields one")
        .id
}

/// The local seat's first structure of `kind`.
#[track_caller]
pub(crate) fn own_building(game: &Game, kind: BuildingKind) -> BuildingId {
    game.state
        .buildings()
        .iter()
        .find(|building| building.player == game.presentation.human && building.kind == kind)
        .expect("the local seat holds one")
        .id
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
#[track_caller]
pub(crate) fn unit_at(game: &Game, x: i32, y: i32) -> UnitId {
    game.state
        .units()
        .iter()
        .find(|u| u.tile() == chassis::grid::TilePos::new(x, y))
        .map(|u| u.id)
        .expect("a unit stands there")
}

/// Where a unit stands.
#[track_caller]
pub(crate) fn pos(game: &Game, id: UnitId) -> Vec2 {
    let unit = game.state.unit(id).expect("the unit lives");
    vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>())
}

/// The building anchored at `(x, y)`.
#[track_caller]
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

pub(crate) fn mouse_move(at: Vec2) -> RawEvent {
    RawEvent::MouseMove { x: at.x, y: at.y }
}

pub(crate) fn left_down(at: Vec2) -> RawEvent {
    RawEvent::MouseDown {
        button: MouseButton::Left,
        x: at.x,
        y: at.y,
    }
}

pub(crate) fn left_up(at: Vec2) -> RawEvent {
    RawEvent::MouseUp {
        button: MouseButton::Left,
        x: at.x,
        y: at.y,
    }
}

pub(crate) fn right_down(at: Vec2) -> RawEvent {
    RawEvent::MouseDown {
        button: MouseButton::Right,
        x: at.x,
        y: at.y,
    }
}

pub(crate) fn right_up(at: Vec2) -> RawEvent {
    RawEvent::MouseUp {
        button: MouseButton::Right,
        x: at.x,
        y: at.y,
    }
}

/// A left click in place.
pub(crate) fn click(at: Vec2) -> [RawEvent; 2] {
    [left_down(at), left_up(at)]
}

pub(crate) fn touch_down(id: u64, at: Vec2) -> RawEvent {
    RawEvent::TouchDown {
        id,
        x: at.x,
        y: at.y,
    }
}

pub(crate) fn touch_move(id: u64, at: Vec2) -> RawEvent {
    RawEvent::TouchMove {
        id,
        x: at.x,
        y: at.y,
    }
}

pub(crate) fn touch_up(id: u64, at: Vec2) -> RawEvent {
    RawEvent::TouchUp {
        id,
        x: at.x,
        y: at.y,
    }
}

pub(crate) fn touch_cancel(id: u64) -> RawEvent {
    RawEvent::TouchCancel { id }
}

/// Finger `id` tapping in place.
pub(crate) fn tap(id: u64, at: Vec2) -> [RawEvent; 2] {
    [touch_down(id, at), touch_up(id, at)]
}

/// `text` typed one character at a time.
pub(crate) fn typed(text: &str) -> Vec<RawEvent> {
    text.chars().map(|ch| RawEvent::Text { ch }).collect()
}

pub(crate) fn key_down(key: Key) -> RawEvent {
    RawEvent::KeyDown { key }
}

pub(crate) fn key_up(key: Key) -> RawEvent {
    RawEvent::KeyUp { key }
}

/// A key pressed and released.
pub(crate) fn keystroke(key: Key) -> [RawEvent; 2] {
    [key_down(key), key_up(key)]
}

/// Walks the press gesture every screen button keeps, pressing `armed` and
/// letting go over `other` where a beat calls for the wrong place:
///
/// - a click dragged to `other` cancels, and a click in place commits;
/// - finger 7 arms `armed`, finger 8 pressing, moving and lifting on
///   `other` neither steals nor drops its press, and finger 7 lifting in
///   place commits;
/// - finger 9 arming `armed`, sliding to `other` and lifting there cancels;
/// - finger 10 tapping `armed` commits.
///
/// `drive` feeds one frame to the screen and returns what the test watches
/// and where the screen's pointer rests. A frame that commits nothing must
/// leave the watch at `idle`, a commit gives `committed`, and a second
/// finger never moves the pointer. The screen's own assertions follow the
/// walk.
#[track_caller]
pub(crate) fn assert_press_contract<O: PartialEq + Debug>(
    mut drive: impl FnMut(&[RawEvent]) -> (O, Vec2),
    armed: Vec2,
    other: Vec2,
    idle: &O,
    committed: &O,
) {
    let beats: [(&[RawEvent], &O, Option<Vec2>, &str); 7] = [
        (
            &[left_down(armed), left_up(other)],
            idle,
            None,
            "a click dragged away cancels",
        ),
        (&click(armed), committed, None, "a click in place commits"),
        (&[touch_down(7, armed)], idle, Some(armed), "a finger arms"),
        (
            &[
                touch_down(8, other),
                touch_move(8, other),
                touch_up(8, other),
            ],
            idle,
            Some(armed),
            "a second finger neither steals nor resolves the press",
        ),
        (
            &[touch_up(7, armed)],
            committed,
            Some(armed),
            "the owner still holds its press",
        ),
        (
            &[
                touch_down(9, armed),
                touch_move(9, other),
                touch_up(9, other),
            ],
            idle,
            None,
            "the owner lifting elsewhere cancels",
        ),
        (
            &tap(10, armed),
            committed,
            Some(armed),
            "the next tap commits",
        ),
    ];
    for (events, want, pointer, beat) in beats {
        let (got, at) = drive(events);
        assert_eq!(&got, want, "{beat}");
        if let Some(pointer) = pointer {
            assert_eq!(at, pointer, "{beat}: where the pointer rests");
        }
    }
}
