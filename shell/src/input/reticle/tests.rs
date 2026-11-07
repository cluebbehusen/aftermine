//! The reticle through the real funnel: pad events recorded as the frame
//! loop records them, then a match's frame of held input.

use super::super::select::pick;
use super::super::{InputState, Pointer, apply_events, track_hands, update_held};
use super::*;
use crate::action::BindingMap;
use aftermine_protocol::{Key, RawEvent, Stick};
use aftermine_sim::{BuildingKind, UnitKind};
use macroquad::prelude::vec2;

const ZOOM: f32 = 64.0;

/// The armed skirmish, zoomed in so the camera has room to pan, with a
/// second Foundry of your own to cycle to.
fn game() -> Game {
    let mut scenario = super::super::tests::armed_skirmish();
    scenario
        .buildings
        .push(aftermine_sim::scenario::BuildingSpec {
            player: 0,
            kind: BuildingKind::Foundry,
            x: 30,
            y: 3,
        });
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("the scenario builds");
    game.presentation.camera.show(vec2(20.0, 12.0), ZOOM);
    game
}

/// Feeds events as the frame loop does on the Playing screen: every screen
/// notes the hands, then the match applies them.
fn feed(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    for event in events {
        track_hands(input, event, false);
    }
    apply_events(game, input, &BindingMap::classic(), events, |_, _| {});
}

fn stick(stick: Stick, x: f32, y: f32) -> RawEvent {
    RawEvent::PadStick {
        pad: 1,
        stick,
        x,
        y,
    }
}

fn press(button: PadButton) -> RawEvent {
    RawEvent::PadDown { pad: 1, button }
}

fn own(game: &Game, kind: UnitKind) -> &aftermine_sim::Unit {
    game.state
        .units()
        .iter()
        .find(|u| u.kind == kind && u.player == game.presentation.human)
        .expect("the opening fields one")
}

fn at(unit: &aftermine_sim::Unit) -> Vec2 {
    vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>())
}

/// The world point under the reticle.
fn under_reticle(game: &Game, input: &InputState) -> Vec2 {
    game.presentation.camera.to_world(input.reticle.point())
}

#[test]
fn a_held_stick_pans_every_frame_faster_the_further_it_is_pushed() {
    let mut game = game();
    let mut input = InputState::new();
    feed(&mut game, &mut input, &[stick(Stick::Left, 0.5, 0.0)]);
    assert_eq!(input.last_pointer, Pointer::Pad);
    let start = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.1);
    let half = game.presentation.camera.center - start;
    update_held(&mut game, &mut input, 0.1);
    let again = game.presentation.camera.center - start - half;
    assert!((half - again).length() < 1e-4, "the stick sent once");
    let speed = crate::input::PAN_PX_PER_SEC * input.camera_prefs.pan_speed;
    assert!((half.x - speed * 0.25 * 0.1 / ZOOM).abs() < 1e-4, "{half}");
    assert_eq!(half.y, 0.0);

    let before = game.presentation.camera.center;
    feed(&mut game, &mut input, &[stick(Stick::Left, 0.0, 1.0)]);
    update_held(&mut game, &mut input, 0.1);
    let full = game.presentation.camera.center - before;
    assert!(full.x.abs() < 1e-4);
    assert!(
        (full.y - 4.0 * half.x).abs() < 1e-4,
        "a full push is 4x half"
    );

    feed(&mut game, &mut input, &[stick(Stick::Left, 0.0, 0.0)]);
    let rest = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(game.presentation.camera.center, rest);
}

#[test]
fn a_stick_let_go_under_a_menu_stops_the_pan() {
    let mut game = game();
    let mut input = InputState::new();
    feed(&mut game, &mut input, &[stick(Stick::Left, 1.0, 0.0)]);
    // The pause menu sees the release; the match never does.
    track_hands(&mut input, &stick(Stick::Left, 0.0, 0.0), false);
    let rest = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(game.presentation.camera.center, rest);
    // A pad that leaves takes its sticks with it.
    feed(&mut game, &mut input, &[stick(Stick::Left, 1.0, 0.0)]);
    track_hands(&mut input, &RawEvent::PadDisconnected { pad: 1 }, false);
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(game.presentation.camera.center, rest);
}

#[test]
fn at_the_map_edge_the_reticle_drifts_and_comes_back_first() {
    let mut game = game();
    game.presentation.camera.show(vec2(40.0, 12.0), ZOOM);
    let mut input = InputState::new();
    let edge = game.presentation.camera.center;
    feed(&mut game, &mut input, &[stick(Stick::Left, 1.0, 0.0)]);
    let full = crate::input::PAN_PX_PER_SEC * input.camera_prefs.pan_speed;
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(game.presentation.camera.center, edge);
    let center = game.presentation.camera.viewport() * 0.5;
    assert!((input.reticle.point() - center - vec2(full * 0.1, 0.0)).length() < 1e-3);

    // Back toward center spends the drift before the camera moves.
    feed(&mut game, &mut input, &[stick(Stick::Left, -1.0, 0.0)]);
    update_held(&mut game, &mut input, 0.05);
    assert_eq!(game.presentation.camera.center, edge);
    assert!((input.reticle.point().x - center.x - full * 0.05).abs() < 1e-3);
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(input.reticle.point(), center);
    let moved = (edge.x - game.presentation.camera.center.x) * ZOOM;
    assert!((moved - full * 0.05).abs() < 1e-2, "{moved}");

    // However long it is held, the reticle stays inside the window.
    feed(&mut game, &mut input, &[stick(Stick::Left, 1.0, 1.0)]);
    for _ in 0..100 {
        update_held(&mut game, &mut input, 0.1);
    }
    let viewport = game.presentation.camera.viewport();
    let point = input.reticle.point();
    assert!(point.x < viewport.x && point.y < viewport.y, "{point}");
    assert!(point.x > center.x && point.y > center.y, "{point}");
}

#[test]
fn the_right_stick_zooms_about_the_reticle() {
    for inverted in [false, true] {
        let mut game = game();
        game.presentation.camera.show(vec2(20.0, 12.0), 48.0);
        let mut input = InputState::new();
        input.camera_prefs.zoom_inverted = inverted;
        let world = game.presentation.camera.center;
        feed(&mut game, &mut input, &[stick(Stick::Right, 0.0, -1.0)]);
        for _ in 0..10 {
            update_held(&mut game, &mut input, 0.05);
            game.presentation.camera.update(0.05);
        }
        let zoom = game.presentation.camera.zoom;
        assert_eq!(zoom > 48.0, !inverted, "stick up zooms in unless inverted");
        assert!((under_reticle(&game, &input) - world).length() < 1e-2);
    }
}

#[test]
fn the_reticle_snaps_onto_a_unit_in_reach_and_slows_over_it() {
    let mut game = game();
    let mut input = InputState::new();
    let sentinel = own(&game, UnitKind::Sentinel);
    let (id, owner, spot) = (sentinel.id, sentinel.player, at(sentinel));
    game.presentation.camera.show(spot + vec2(0.3, 0.0), ZOOM);
    feed(&mut game, &mut input, &[press(PadButton::South)]);
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.reticle.snap(), Some(Picked::Unit(id, owner)));

    // Friction: the same push moves the camera half as far while snapped.
    feed(&mut game, &mut input, &[stick(Stick::Left, 0.0, 1.0)]);
    let before = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.01);
    let slow = game.presentation.camera.center.y - before.y;
    game.presentation.camera.show(spot + vec2(3.0, 0.0), ZOOM);
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.reticle.snap(), None, "nothing in reach");
    let before = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.01);
    let fast = game.presentation.camera.center.y - before.y;
    assert!((fast - 2.0 * slow).abs() < 1e-5, "{slow} {fast}");

    // A mouse takes the pointer back, and the reticle rests on nothing.
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseMove { x: 5.0, y: 6.0 }],
    );
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.hover_point(), Some(vec2(5.0, 6.0)));
    assert_eq!(input.reticle.snap(), None);
}

#[test]
fn the_reticle_snaps_onto_a_structure_near_its_footprint_but_not_through_fog() {
    let mut game = game();
    let mut input = InputState::new();
    let zoom = 96.0;
    let foundry = game.view().own_foundries().next().expect("a home Foundry");
    let (id, owner) = (foundry.id, foundry.player);
    let (w, h) = BuildingKind::Foundry.size();
    let edge = vec2(
        foundry.anchor.x as f32 + w as f32,
        foundry.anchor.y as f32 + h as f32 * 0.5,
    );
    // A fifth of a tile past the footprint's right edge, inside the pad's
    // reach but past the mouse's.
    let near = edge + vec2(0.2, 0.0);
    game.presentation.camera.show(near, zoom);
    let point = game.presentation.camera.viewport() * 0.5;
    assert_eq!(pick(&game, point, 1.0, Pointer::Mouse), None);
    feed(&mut game, &mut input, &[press(PadButton::South)]);
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.reticle.snap(), Some(Picked::Building(id, owner)));

    let enemy = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player != owner && b.kind == BuildingKind::Foundry)
        .expect("the enemy's Foundry");
    let center = enemy.center();
    let center = vec2(center.x.to_num::<f32>(), center.y.to_num::<f32>());
    game.presentation.camera.show(center, ZOOM);
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.reticle.snap(), None, "the enemy base is fogged");
}

#[test]
fn the_dpad_jumps_like_its_keys_and_the_reticle_lands_on_what_it_framed() {
    let mut game = game();
    let mut input = InputState::new();

    // Down: each idle Harvester in turn. They stand near the map's corner,
    // so the camera stops short and the reticle drifts onto each, even
    // when the camera has nowhere left to go.
    let mut cameras = Vec::new();
    for _ in 0..3 {
        feed(&mut game, &mut input, &[press(PadButton::DpadDown)]);
        let picked = game.presentation.selection.units.clone();
        assert_eq!(picked.len(), 1);
        let harvester = at(game.state.unit(picked[0]).expect("selected"));
        assert!((under_reticle(&game, &input) - harvester).length() < 1e-3);
        cameras.push(game.presentation.camera.center);
    }
    assert_eq!(cameras[1], cameras[2], "the camera stood still for one");

    // Left: the last alert, or nothing.
    let rest = game.presentation.camera.center;
    feed(&mut game, &mut input, &[press(PadButton::DpadLeft)]);
    assert_eq!(game.presentation.camera.center, rest);
    game.presentation.last_alert = Some(vec2(20.0, 12.0));
    feed(&mut game, &mut input, &[press(PadButton::DpadLeft)]);
    assert_eq!(game.presentation.camera.center, vec2(20.0, 12.0));
    assert_eq!(
        input.reticle.point(),
        game.presentation.camera.viewport() * 0.5
    );

    // The right-stick click: the selection's center.
    let sentinel = own(&game, UnitKind::Sentinel);
    let (id, spot) = (sentinel.id, at(sentinel));
    game.presentation.selection.units = vec![id];
    feed(&mut game, &mut input, &[press(PadButton::RightStick)]);
    assert!((under_reticle(&game, &input) - spot).length() < 1e-3);

    // Buttons a later control owns leave the camera be.
    let rest = game.presentation.camera.center;
    feed(&mut game, &mut input, &[press(PadButton::North)]);
    assert_eq!(game.presentation.camera.center, rest);
}

#[test]
fn center_on_foundry_cycles_through_them_by_pad_and_key() {
    let mut game = game();
    let mut input = InputState::new();
    let foundries = super::super::dispatch::own_foundries(&game.view());
    assert_eq!(foundries.len(), 2, "the scenario fields a second Foundry");
    let mut framed = Vec::new();
    for events in [
        vec![press(PadButton::DpadUp)],
        vec![
            RawEvent::KeyDown { key: Key::Space },
            RawEvent::KeyUp { key: Key::Space },
        ],
        vec![press(PadButton::DpadUp)],
    ] {
        feed(&mut game, &mut input, &events);
        let looking = under_reticle(&game, &input);
        framed.push(
            foundries
                .iter()
                .position(|at| (*at - looking).length() < 1e-3),
        );
    }
    assert_eq!(framed, [Some(0), Some(1), Some(0)], "it wraps");
}
