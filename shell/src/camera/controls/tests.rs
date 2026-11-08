use super::*;
use aftermine_protocol::Key;

fn camera() -> Camera {
    Camera::new(vec2(100.0, 100.0), 200, 200, vec2(1280.0, 800.0))
}

#[test]
fn held_keys_pan_at_the_preferred_speed_in_any_view() {
    let mut resolver = ActionResolver::default();
    let bindings = crate::action::BindingMap::classic();
    resolver.key_edge_in(
        &bindings,
        Key::Right,
        true,
        crate::action::Context::FinalMap,
    );
    assert_eq!(held_pan(&resolver), vec2(1.0, 0.0));

    let mut slow = camera();
    let mut fast = camera();
    let start = slow.center;
    pan_toward(&mut slow, held_pan(&resolver), CameraPrefs::default(), 0.1);
    let quick = CameraPrefs {
        pan_speed: 2.0,
        ..CameraPrefs::default()
    };
    pan_toward(&mut fast, held_pan(&resolver), quick, 0.1);
    assert!(slow.center.x > start.x);
    assert!((fast.center.x - start.x) > (slow.center.x - start.x) * 1.9);

    let mut still = camera();
    pan_toward(&mut still, Vec2::ZERO, CameraPrefs::default(), 1.0);
    assert_eq!(still.center, start);
}

#[test]
fn the_wheel_honors_the_inverted_zoom_preference() {
    let at = vec2(640.0, 400.0);
    let mut plain = camera();
    let mut inverted = camera();
    wheel_zoom(&mut plain, at, 1.0, CameraPrefs::default());
    wheel_zoom(
        &mut inverted,
        at,
        1.0,
        CameraPrefs {
            zoom_inverted: true,
            ..CameraPrefs::default()
        },
    );
    plain.update(10.0);
    inverted.update(10.0);
    assert!(plain.zoom > camera().zoom);
    assert!(inverted.zoom < camera().zoom);
}

#[test]
fn a_viewer_steers_from_the_minimap_until_the_press_lets_go() {
    let mut hands = ViewerHands::default();
    let mut camera = camera();
    let mut mouse = Vec2::ZERO;
    let prefs = CameraPrefs::default();
    let on_map = |world: Vec2| MinimapPoint {
        under: Some(world),
        clamped: Some(world),
    };
    let press = RawEvent::MouseDown {
        button: MouseButton::Left,
        x: 1200.0,
        y: 700.0,
    };
    hands.pointer(
        &press,
        on_map(vec2(10.0, 10.0)),
        &mut camera,
        &mut mouse,
        prefs,
        1.0,
    );
    let pressed = camera.center;
    assert_eq!(mouse, vec2(1200.0, 700.0));

    let drag = RawEvent::MouseMove {
        x: 1210.0,
        y: 710.0,
    };
    hands.pointer(
        &drag,
        on_map(vec2(30.0, 20.0)),
        &mut camera,
        &mut mouse,
        prefs,
        1.0,
    );
    assert_ne!(camera.center, pressed, "a held press keeps steering");
    let steered = camera.center;

    let release = RawEvent::MouseUp {
        button: MouseButton::Left,
        x: 1210.0,
        y: 710.0,
    };
    hands.pointer(
        &release,
        MinimapPoint::default(),
        &mut camera,
        &mut mouse,
        prefs,
        1.0,
    );
    hands.pointer(
        &drag,
        on_map(vec2(5.0, 5.0)),
        &mut camera,
        &mut mouse,
        prefs,
        1.0,
    );
    assert_eq!(camera.center, steered, "a released press stops steering");
}

#[test]
fn a_cancelled_minimap_finger_stops_steering() {
    let mut hands = ViewerHands::default();
    let mut camera = camera();
    let mut mouse = Vec2::ZERO;
    let prefs = CameraPrefs::default();
    let on_map = |world: Vec2| MinimapPoint {
        under: Some(world),
        clamped: Some(world),
    };
    let mut feed = |event: RawEvent, minimap: MinimapPoint, camera: &mut Camera| {
        hands.pointer(&event, minimap, camera, &mut mouse, prefs, 1.0);
    };
    let (x, y) = (1200.0, 700.0);
    feed(
        RawEvent::TouchDown { id: 1, x, y },
        on_map(vec2(10.0, 10.0)),
        &mut camera,
    );
    feed(
        RawEvent::TouchMove { id: 1, x, y },
        on_map(vec2(30.0, 20.0)),
        &mut camera,
    );
    let steered = camera.center;
    feed(
        RawEvent::TouchCancel { id: 1 },
        MinimapPoint::default(),
        &mut camera,
    );
    feed(
        RawEvent::TouchMove { id: 1, x, y },
        on_map(vec2(5.0, 5.0)),
        &mut camera,
    );
    assert_eq!(camera.center, steered, "a cancelled finger stops steering");
}

#[test]
fn a_viewer_middle_drag_pans_against_the_pointer() {
    let mut hands = ViewerHands::default();
    let mut camera = camera();
    let mut mouse = Vec2::ZERO;
    let prefs = CameraPrefs::default();
    let start = camera.center;
    for event in [
        RawEvent::MouseDown {
            button: MouseButton::Middle,
            x: 600.0,
            y: 400.0,
        },
        RawEvent::MouseMove { x: 500.0, y: 400.0 },
        RawEvent::MouseUp {
            button: MouseButton::Middle,
            x: 500.0,
            y: 400.0,
        },
        RawEvent::MouseMove { x: 300.0, y: 400.0 },
    ] {
        hands.pointer(
            &event,
            MinimapPoint::default(),
            &mut camera,
            &mut mouse,
            prefs,
            1.0,
        );
    }
    let moved = camera.center.x - start.x;
    assert!(
        (moved - 100.0 / camera.zoom).abs() < 1e-4,
        "dragging left by 100px pans right by 100px of world, then release stops it"
    );
}

fn remote_pads() -> ViewerPads {
    ViewerPads {
        remotes: vec![9],
        ..ViewerPads::default()
    }
}

fn press(pad: u64, button: PadButton) -> RawEvent {
    RawEvent::PadDown { pad, button }
}

#[test]
fn a_remote_click_cycles_its_modes_and_goes_no_further() {
    let mut camera = camera();
    let mut remote = RemoteCamera::with_control();
    let click = [press(9, crate::pad::CONFIRM)];
    for mode in [RemoteMode::Pan, RemoteMode::Zoom, RemoteMode::Control] {
        assert!(
            remote
                .route(&click, &mut camera, &remote_pads(), 0.0)
                .is_empty()
        );
        assert_eq!(remote.mode(), mode);
    }
    let mut no_controls = RemoteCamera::default();
    assert_eq!(no_controls.mode(), RemoteMode::Pan);
    assert_eq!(no_controls.modes(), [RemoteMode::Pan, RemoteMode::Zoom]);
    assert_eq!(no_controls.mode_index(), 0);
    no_controls.route(&click, &mut camera, &remote_pads(), 0.0);
    assert_eq!(no_controls.mode(), RemoteMode::Zoom);
    assert_eq!(no_controls.mode_index(), 1);
}

fn release(pad: u64, button: PadButton) -> RawEvent {
    RawEvent::PadUp { pad, button }
}

#[test]
fn a_held_remote_ring_pans_in_pan_until_let_go() {
    let mut camera = camera();
    let start = camera.center;
    let mut pads = remote_pads();
    pads.prefs.stick_speed = 1.0;
    let mut remote = RemoteCamera::default();
    remote.route(&[press(9, PadButton::DpadRight)], &mut camera, &pads, 0.5);
    let expected = crate::input::PAN_PX_PER_SEC * 0.5 / camera.zoom;
    assert!((camera.center.x - start.x - expected).abs() < 1e-3);
    assert_eq!(camera.center.y, start.y);

    remote.route(&[], &mut camera, &pads, 0.5);
    assert!(
        (camera.center.x - start.x - 2.0 * expected).abs() < 1e-3,
        "it keeps panning while held"
    );
    let held = camera.center;
    remote.route(&[release(9, PadButton::DpadRight)], &mut camera, &pads, 0.5);
    assert_eq!(camera.center, held, "and stops once let go");
}

#[test]
fn a_swipes_tap_in_one_batch_is_no_hold() {
    let mut camera = camera();
    let start = camera.center;
    let mut remote = RemoteCamera::default();
    let tap = [
        press(9, PadButton::DpadRight),
        release(9, PadButton::DpadRight),
    ];
    remote.route(&tap, &mut camera, &remote_pads(), 0.5);
    assert_eq!(camera.center, start);
}

#[test]
fn a_held_remote_ring_keeps_zooming_in_zoom() {
    let settle = |camera: &mut Camera| {
        for _ in 0..240 {
            camera.update(1.0 / 60.0);
        }
    };
    let click = [press(9, crate::pad::CONFIRM)];
    let zoomed = |seconds: f32| {
        let mut camera = camera();
        let mut remote = RemoteCamera::default();
        remote.route(&click, &mut camera, &remote_pads(), 0.0);
        remote.route(
            &[press(9, PadButton::DpadUp)],
            &mut camera,
            &remote_pads(),
            0.0,
        );
        remote.route(&[], &mut camera, &remote_pads(), seconds);
        settle(&mut camera);
        camera.zoom
    };
    assert!(zoomed(0.0) > camera().zoom, "a press zooms a notch");
    assert!(zoomed(0.5) > zoomed(0.0), "and holding keeps going");
}

#[test]
fn changing_mode_lets_go_of_a_held_ring() {
    let mut camera = camera();
    let mut remote = RemoteCamera::default();
    remote.route(
        &[press(9, PadButton::DpadUp)],
        &mut camera,
        &remote_pads(),
        0.0,
    );
    remote.route(
        &[press(9, crate::pad::CONFIRM)],
        &mut camera,
        &remote_pads(),
        0.0,
    );
    remote.route(&[], &mut camera, &remote_pads(), 0.5);
    for _ in 0..240 {
        camera.update(1.0 / 60.0);
    }
    assert_eq!(camera.zoom, self::camera().zoom, "zoom never saw the press");
}

#[test]
fn a_disconnected_remote_stops_panning() {
    let mut camera = camera();
    let mut remote = RemoteCamera::default();
    remote.route(
        &[press(9, PadButton::DpadRight)],
        &mut camera,
        &remote_pads(),
        0.0,
    );
    let held = camera.center;
    remote.route(
        &[RawEvent::PadDisconnected { pad: 9 }],
        &mut camera,
        &remote_pads(),
        0.5,
    );
    assert_eq!(camera.center, held);
}

#[test]
fn the_ring_belongs_to_the_viewer_in_its_own_controls() {
    let mut camera = camera();
    let start = camera.center;
    let mut remote = RemoteCamera::with_control();
    let events = [press(9, PadButton::DpadRight)];
    assert_eq!(
        remote.route(&events, &mut camera, &remote_pads(), 0.5),
        events
    );
    assert_eq!(camera.center, start);
}

#[test]
fn a_remote_swipe_drags_the_map_only_in_pan() {
    let mut camera = camera();
    let before = camera.center;
    let swipe = [RawEvent::PadSwipe {
        pad: 9,
        x: 0.5,
        y: -0.25,
    }];
    let mut remote = RemoteCamera::with_control();
    assert!(
        remote
            .route(&swipe, &mut camera, &remote_pads(), 0.0)
            .is_empty()
    );
    assert_eq!(camera.center, before, "control mode leaves the camera");

    let mut remote = RemoteCamera::default();
    remote.route(&swipe, &mut camera, &remote_pads(), 0.0);
    let moved = camera.center - before;
    assert!(moved.x < 0.0, "a swipe right drags the map right");
    assert!(moved.y > 0.0, "a swipe up drags the map up");
    let zoom = camera.zoom;
    assert!((moved.x + 0.5 * SWIPE_PAN_PX / zoom).abs() < 1e-3);
}

#[test]
fn a_remote_swipe_zooms_only_up_and_down_in_zoom_and_honors_the_flip() {
    let settle = |camera: &mut Camera| {
        for _ in 0..240 {
            camera.update(1.0 / 60.0);
        }
    };
    let mut pads = remote_pads();
    let click = [press(9, crate::pad::CONFIRM)];
    for (flip, button, zooms_in) in [
        (false, PadButton::DpadUp, true),
        (false, PadButton::DpadDown, false),
        (true, PadButton::DpadUp, false),
    ] {
        pads.prefs.zoom_inverted = flip;
        let mut camera = camera();
        let mut remote = RemoteCamera::default();
        remote.route(&click, &mut camera, &pads, 0.0);
        assert_eq!(remote.mode(), RemoteMode::Zoom);
        let before = camera.zoom;
        assert!(
            remote
                .route(&[press(9, button)], &mut camera, &pads, 0.0)
                .is_empty()
        );
        settle(&mut camera);
        assert_eq!(camera.zoom > before, zooms_in, "{flip} {button:?}");
        let held = camera.zoom;
        let center = camera.center;
        remote.route(&[press(9, PadButton::DpadRight)], &mut camera, &pads, 0.0);
        settle(&mut camera);
        assert_eq!((camera.zoom, camera.center), (held, center));
    }
}

#[test]
fn a_remote_leaves_the_viewer_its_own_presses_and_other_controllers_all_of_theirs() {
    let mut camera = camera();
    let mut remote = RemoteCamera::with_control();
    let events = [
        press(9, PadButton::DpadRight),
        press(9, PadButton::PlayPause),
        press(9, crate::pad::BACK),
        press(1, crate::pad::CONFIRM),
        RawEvent::PadSwipe {
            pad: 1,
            x: 1.0,
            y: 0.0,
        },
    ];
    assert_eq!(
        remote.route(&events, &mut camera, &remote_pads(), 0.0),
        events
    );
    assert_eq!(remote.mode(), RemoteMode::Control, "another pad's A");
}

#[test]
fn sticks_pan_and_zoom_any_viewer_at_the_players_speed() {
    let mut camera = camera();
    let before = (camera.center, camera.zoom);
    let mut pads = ViewerPads {
        left: vec2(1.0, 0.0),
        right: vec2(0.0, -1.0),
        ..ViewerPads::default()
    };
    pads.prefs.stick_speed = 1.0;
    RemoteCamera::default().route(&[], &mut camera, &pads, 0.5);
    let expected = crate::input::PAN_PX_PER_SEC * 0.5 / before.1;
    assert!((camera.center.x - before.0.x - expected).abs() < 1e-3);
    for _ in 0..240 {
        camera.update(1.0 / 60.0);
    }
    assert!(camera.zoom > before.1, "a right stick pushed up zooms in");
}
