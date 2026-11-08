//! The reticle through the real funnel: pad events recorded as the frame
//! loop records them, then a match's frame of held input.

use super::super::select::{Reach, pick};
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
    let speed = crate::input::PAN_PX_PER_SEC * input.pad_prefs.stick_speed;
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
    let full = crate::input::PAN_PX_PER_SEC * input.pad_prefs.stick_speed;
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
fn the_stick_pans_at_its_own_speed_and_the_keys_at_theirs() {
    let mut game = game();
    let mut input = InputState::new();
    input.pad_prefs.stick_speed = 2.0;
    input.camera_prefs.pan_speed = 0.5;
    feed(&mut game, &mut input, &[stick(Stick::Left, 1.0, 0.0)]);
    let before = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.01);
    let by_stick = (game.presentation.camera.center.x - before.x) * ZOOM;
    assert!((by_stick - 900.0 * 2.0 * 0.01).abs() < 1e-2, "{by_stick}");

    feed(
        &mut game,
        &mut input,
        &[
            stick(Stick::Left, 0.0, 0.0),
            RawEvent::KeyDown { key: Key::Left },
        ],
    );
    let before = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.01);
    let by_keys = (before.x - game.presentation.camera.center.x) * ZOOM;
    assert!((by_keys - 900.0 * 0.5 * 0.01).abs() < 1e-2, "{by_keys}");
}

#[test]
fn the_right_stick_zooms_about_the_reticle() {
    for inverted in [false, true] {
        let mut game = game();
        game.presentation.camera.show(vec2(20.0, 12.0), 48.0);
        let mut input = InputState::new();
        input.pad_prefs.zoom_inverted = inverted;
        // The wheel's own setting leaves the stick alone.
        input.camera_prefs.zoom_inverted = !inverted;
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

    // While the stick still moves the camera, the pad keeps the pointer.
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseMove { x: 5.0, y: 6.0 }],
    );
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.last_pointer, Pointer::Pad);
    // Once it is let go, a mouse that only moves leaves the pad the
    // pointer; a click takes it back, and the reticle rests on nothing.
    feed(
        &mut game,
        &mut input,
        &[
            stick(Stick::Left, 0.0, 0.0),
            RawEvent::MouseMove { x: 5.0, y: 6.0 },
        ],
    );
    update_held(&mut game, &mut input, 0.0);
    assert_eq!(input.last_pointer, Pointer::Pad);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseDown {
            button: aftermine_protocol::MouseButton::Left,
            x: 5.0,
            y: 6.0,
        }],
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
    assert_eq!(pick(&game, point, 1.0, Reach::MOUSE), None);
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
fn each_snap_strength_reaches_and_slows_as_far_as_it_says() {
    // Reach in logical pixels around the reticle, and the share of stick
    // speed left while it rests on something. Off picks as the mouse does.
    let strengths = [
        (ReticleSnap::Off, 10.0, 1.0, false),
        (ReticleSnap::Light, 16.0, 0.75, false),
        (ReticleSnap::Standard, 24.0, 0.5, true),
        (ReticleSnap::Strong, 32.0, 0.35, true),
    ];
    assert_eq!(snap_feel(ReticleSnap::Off).0, Reach::MOUSE);
    let mut game = game();
    let sentinel = own(&game, UnitKind::Sentinel);
    let (id, owner, spot) = (sentinel.id, sentinel.player, at(sentinel));
    let foundry = game.view().own_foundries().next().expect("a home Foundry");
    let home = Picked::Building(foundry.id, foundry.player);
    let (w, h) = BuildingKind::Foundry.size();
    // A fifth of a tile, 19 px at this zoom, past the footprint's right
    // edge.
    let near_home = vec2(
        foundry.anchor.x as f32 + w as f32 + 0.2,
        foundry.anchor.y as f32 + h as f32 * 0.5,
    );
    for (snap, px, friction, reaches_home) in strengths {
        let reach = snap_feel(snap).0;
        // Zoomed out, a unit's own body is smaller than every reach.
        game.presentation.camera.show(spot, 10.0);
        let camera = &game.presentation.camera;
        assert!(crate::input::unit_pick_radius(UnitKind::Sentinel) * camera.zoom < 9.0);
        let from = camera.to_screen(spot);
        let inside = pick(&game, from + vec2(0.0, px - 1.0), 1.0, reach);
        assert_eq!(inside, Some(Picked::Unit(id, owner)), "{snap:?}");
        let outside = pick(&game, from + vec2(0.0, px + 1.0), 1.0, reach);
        assert_ne!(outside, Some(Picked::Unit(id, owner)), "{snap:?}");
        game.presentation.camera.show(near_home, 96.0);
        let point = game.presentation.camera.viewport() * 0.5;
        let near = pick(&game, point, 1.0, reach);
        assert_eq!(near == Some(home), reaches_home, "{snap:?}");

        // The same push, snapped and then in the open.
        let mut input = InputState::new();
        input.pad_prefs.snap = snap;
        game.presentation.camera.show(spot + vec2(0.3, 0.0), ZOOM);
        feed(&mut game, &mut input, &[stick(Stick::Left, 0.0, 1.0)]);
        update_held(&mut game, &mut input, 0.0);
        assert_eq!(input.reticle.snap(), Some(Picked::Unit(id, owner)));
        let before = game.presentation.camera.center;
        update_held(&mut game, &mut input, 0.01);
        let slow = game.presentation.camera.center.y - before.y;
        game.presentation.camera.show(spot + vec2(3.0, 0.0), ZOOM);
        update_held(&mut game, &mut input, 0.0);
        assert_eq!(input.reticle.snap(), None);
        let before = game.presentation.camera.center;
        update_held(&mut game, &mut input, 0.01);
        let fast = game.presentation.camera.center.y - before.y;
        assert!(
            (slow - friction * fast).abs() < 1e-5,
            "{snap:?}: {slow} {fast}"
        );
    }
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

/// Drives the reticle off center at the map's right edge.
fn drift_at_edge(game: &mut Game, input: &mut InputState) {
    game.presentation.camera.show(vec2(40.0, 12.0), ZOOM);
    feed(game, input, &[stick(Stick::Left, 1.0, 0.0)]);
    update_held(game, input, 0.3);
    feed(game, input, &[stick(Stick::Left, 0.0, 0.0)]);
    assert_ne!(
        input.reticle.point(),
        game.presentation.camera.viewport() * 0.5
    );
}

#[test]
fn every_jump_takes_the_drift_with_it() {
    let classic = BindingMap::classic();
    let mut game = game();
    let mut input = InputState::new();

    // A bookmark in the open: the reticle comes back to center on it.
    drift_at_edge(&mut game, &mut input);
    input.bookmarks[0] = Some(vec2(20.0, 12.0));
    dispatch_action(&mut game, &mut input, &classic, Action::RecallBookmark(0));
    assert_eq!(game.presentation.camera.center, vec2(20.0, 12.0));
    assert_eq!(
        input.reticle.point(),
        game.presentation.camera.viewport() * 0.5
    );

    // A control group recalled twice quickly: the reticle rests on it.
    let sentinel = own(&game, UnitKind::Sentinel);
    let (id, spot) = (sentinel.id, at(sentinel));
    game.presentation.selection.units = vec![id];
    dispatch_action(&mut game, &mut input, &classic, Action::AssignGroup(1));
    game.presentation.selection.units.clear();
    drift_at_edge(&mut game, &mut input);
    dispatch_action(&mut game, &mut input, &classic, Action::Slot(1));
    dispatch_action(&mut game, &mut input, &classic, Action::Slot(1));
    assert!((under_reticle(&game, &input) - spot).length() < 1e-3);
}

#[test]
fn a_snap_is_seen_only_while_its_target_is() {
    let game = game();
    let view = game.view();
    let sentinel = own(&game, UnitKind::Sentinel);
    assert!(Picked::Unit(sentinel.id, sentinel.player).seen(&view));
    let enemy = game
        .state
        .units()
        .iter()
        .find(|u| u.player != game.presentation.human)
        .expect("the enemy fields units");
    assert!(!Picked::Unit(enemy.id, enemy.player).seen(&view), "fogged");
    let gone = aftermine_sim::UnitId(u32::MAX);
    assert!(!Picked::Unit(gone, sentinel.player).seen(&view), "dead");
}

#[test]
fn a_new_match_starts_with_a_fresh_reticle() {
    let mut game = game();
    let mut input = InputState::new();
    drift_at_edge(&mut game, &mut input);
    input.reticle.snap = Some(Picked::Unit(
        own(&game, UnitKind::Sentinel).id,
        game.presentation.human,
    ));
    input.reset_session();
    assert_eq!(input.reticle, Reticle::default());
}

#[test]
fn the_last_pad_leaving_hands_the_pointer_back() {
    let mut game = game();
    let mut input = InputState::new();
    let connect = |pad| RawEvent::PadConnected {
        pad,
        family: aftermine_protocol::PadFamily::Xbox,
    };
    feed(
        &mut game,
        &mut input,
        &[connect(1), connect(2), press(PadButton::South)],
    );
    assert_eq!(input.last_pointer, Pointer::Pad);
    track_hands(&mut input, &RawEvent::PadDisconnected { pad: 2 }, false);
    assert_eq!(input.last_pointer, Pointer::Pad, "another pad remains");
    track_hands(&mut input, &RawEvent::PadDisconnected { pad: 1 }, false);
    assert_eq!(input.last_pointer, crate::platform::Hands::BUILD.pointer);
}

#[test]
fn zooming_out_at_the_map_edge_keeps_the_reticle_on_its_ground() {
    let mut game = game();
    let mut input = InputState::new();
    drift_at_edge(&mut game, &mut input);
    update_held(&mut game, &mut input, 0.0);
    let aim = under_reticle(&game, &input);
    let start = game.presentation.camera.center;
    feed(&mut game, &mut input, &[stick(Stick::Right, 0.0, 1.0)]);
    // The frame loop advances the glide before it reads input.
    for _ in 0..10 {
        game.presentation.camera.update(0.05);
        update_held(&mut game, &mut input, 0.05);
    }
    assert!(game.presentation.camera.zoom < ZOOM, "the stick zoomed out");
    assert!(
        game.presentation.camera.center.x < start.x,
        "the edge pushed the camera in"
    );
    assert!(
        (under_reticle(&game, &input) - aim).length() < 1e-2,
        "{aim} moved to {}",
        under_reticle(&game, &input)
    );

    // Let go and open a menu at once: the glide settles under it, where the
    // match reads no input, and the reticle catches up when play resumes.
    track_hands(&mut input, &stick(Stick::Right, 0.0, 0.0), false);
    let before = game.presentation.camera.center;
    for _ in 0..10 {
        game.presentation.camera.update(0.05);
    }
    assert_ne!(game.presentation.camera.center, before, "the glide went on");
    update_held(&mut game, &mut input, 0.05);
    assert!(
        (under_reticle(&game, &input) - aim).length() < 1e-2,
        "{aim} moved to {} behind the menu",
        under_reticle(&game, &input)
    );
}

#[test]
fn a_stick_held_since_a_menu_makes_the_pad_the_pointer() {
    let mut game = game();
    let mut input = InputState::new();
    input.last_pointer = Pointer::Mouse;
    // Pushed while a menu was up: only the hands saw it.
    track_hands(&mut input, &stick(Stick::Left, 0.6, 0.0), false);
    assert_eq!(input.last_pointer, Pointer::Mouse);
    let before = game.presentation.camera.center;
    update_held(&mut game, &mut input, 0.1);
    assert_ne!(
        game.presentation.camera.center, before,
        "the held stick pans"
    );
    assert_eq!(input.last_pointer, Pointer::Pad, "so the reticle shows");
    assert_eq!(input.hover_point(), Some(input.reticle.point()));
}

#[test]
fn a_jump_while_an_edge_zoom_settles_lands_on_its_target() {
    let mut game = game();
    let mut input = InputState::new();
    drift_at_edge(&mut game, &mut input);
    feed(&mut game, &mut input, &[stick(Stick::Right, 0.0, 1.0)]);
    update_held(&mut game, &mut input, 0.05);
    feed(&mut game, &mut input, &[stick(Stick::Right, 0.0, 0.0)]);
    // The glide the edge holds back moves the camera at the frame's start,
    // then the same frame's jump lands.
    game.presentation.camera.update(0.05);
    feed(&mut game, &mut input, &[press(PadButton::DpadUp)]);
    update_held(&mut game, &mut input, 0.05);
    let foundry = super::super::dispatch::own_foundries(&game.view())[0];
    assert!((under_reticle(&game, &input) - foundry).length() < 1e-2);
}

#[test]
fn a_growing_window_keeps_the_reticle_on_its_ground() {
    let mut game = game();
    let mut input = InputState::new();
    drift_at_edge(&mut game, &mut input);
    update_held(&mut game, &mut input, 0.0);
    let aim = under_reticle(&game, &input);
    let edge = game.presentation.camera.center;
    game.presentation.camera.set_viewport(vec2(1600.0, 900.0));
    assert_ne!(
        game.presentation.camera.center, edge,
        "the edge pulled it in"
    );
    update_held(&mut game, &mut input, 0.0);
    assert!((under_reticle(&game, &input) - aim).length() < 1e-3);
}

#[test]
fn pan_keys_move_the_reticle_while_the_pad_points() {
    let mut game = game();
    let mut input = InputState::new();
    let speed = crate::input::PAN_PX_PER_SEC * input.camera_prefs.pan_speed;
    // In the open, the keys pan the camera as they always do.
    feed(&mut game, &mut input, &[press(PadButton::South)]);
    let before = game.presentation.camera.center;
    feed(
        &mut game,
        &mut input,
        &[RawEvent::KeyDown { key: Key::Left }],
    );
    update_held(&mut game, &mut input, 0.1);
    let moved = (before.x - game.presentation.camera.center.x) * ZOOM;
    assert!((moved - speed * 0.1).abs() < 1e-2, "{moved}");
    feed(&mut game, &mut input, &[RawEvent::KeyUp { key: Key::Left }]);

    // At the edge they bring an off-center reticle home before the camera.
    drift_at_edge(&mut game, &mut input);
    update_held(&mut game, &mut input, 0.0);
    let edge = game.presentation.camera.center;
    let center = game.presentation.camera.viewport() * 0.5;
    let drift = input.reticle.point().x - center.x;
    feed(
        &mut game,
        &mut input,
        &[RawEvent::KeyDown { key: Key::Left }],
    );
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(game.presentation.camera.center, edge);
    assert!((input.reticle.point().x - center.x - (drift - speed * 0.1)).abs() < 1e-2);
}

#[test]
fn center_on_foundry_cycles_under_the_mouse_too() {
    let mut game = game();
    let mut input = InputState::new();
    input.last_pointer = Pointer::Mouse;
    let foundries = super::super::dispatch::own_foundries(&game.view());
    let space = [
        RawEvent::KeyDown { key: Key::Space },
        RawEvent::KeyUp { key: Key::Space },
    ];
    let mut framed = Vec::new();
    for _ in 0..3 {
        feed(&mut game, &mut input, &space);
        // A quiet frame between presses: the camera stays, so the ground
        // the reticle marks does too, even where the edge held it back.
        update_held(&mut game, &mut input, 0.1);
        let looking = input.reticle.ground(&game.presentation.camera);
        framed.push(
            foundries
                .iter()
                .position(|at| (*at - looking).length() < 1e-3),
        );
    }
    assert_eq!(framed, [Some(0), Some(1), Some(0)]);
    // Once the mouse's own hands move the camera, the reticle follows it.
    feed(
        &mut game,
        &mut input,
        &[RawEvent::KeyDown { key: Key::Right }],
    );
    update_held(&mut game, &mut input, 0.1);
    assert_eq!(
        input.reticle.ground(&game.presentation.camera),
        game.presentation.camera.center
    );
}
