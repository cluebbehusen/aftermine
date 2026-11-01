use super::*;

fn camera() -> Camera {
    Camera::new(vec2(20.0, 12.0), 40, 24, vec2(1280.0, 800.0))
}

#[test]
fn screen_world_roundtrip() {
    let cam = camera();
    for point in [vec2(0.0, 0.0), vec2(640.0, 400.0), vec2(1279.0, 799.0)] {
        let world = cam.to_world(point);
        let back = cam.to_screen(world);
        assert!((back - point).length() < 1e-3, "{point:?} -> {back:?}");
    }
}

/// How far a zoom may move the anchor, in logical pixels on either axis: the
/// view eases the world origin onto a whole pixel as it comes to rest.
const WHOLE_PIXEL_EASE: f32 = 0.501;

fn anchor_drift(cam: &Camera, anchored: Vec2, cursor: Vec2) -> f32 {
    (cam.to_screen(anchored) - cursor).abs().max_element()
}

#[test]
fn zoom_keeps_the_cursor_anchor_fixed() {
    let mut cam = camera();
    let cursor = vec2(300.0, 250.0);
    let before = cam.to_world(cursor);
    cam.zoom_at(cursor, 2.0);
    cam.update(1.0); // saturated step: glide finishes instantly
    assert!(
        anchor_drift(&cam, before, cursor) <= WHOLE_PIXEL_EASE,
        "anchor drifted to {:?}",
        cam.to_screen(before)
    );
    assert!(cam.zoom > 32.0);
}

#[test]
fn zoom_clamps_to_bounds() {
    let mut cam = camera();
    cam.zoom_at(vec2(0.0, 0.0), 100.0);
    cam.update(1.0);
    assert_eq!(cam.zoom, ZOOM_MAX);
    cam.zoom_at(vec2(0.0, 0.0), -100.0);
    cam.update(1.0);
    assert_eq!(cam.zoom, ZOOM_MIN);
}

#[test]
fn pan_clamps_to_map_with_slack() {
    let mut cam = camera();
    cam.pan(vec2(-1000.0, -1000.0));
    let lo = cam.center;
    cam.pan(vec2(2000.0, 2000.0));
    let hi = cam.center;
    assert!(lo.x < hi.x && lo.y < hi.y);
    // Slack allows at most two tiles beyond the edges.
    let (min, _) = {
        cam.center = lo;
        cam.world_rect()
    };
    assert!(min.x >= -3.0 && min.y >= -3.0, "runaway clamp: {min:?}");
}

#[test]
fn resize_reclamps() {
    let mut cam = camera();
    cam.pan(vec2(1000.0, 1000.0)); // pinned at the bottom-right clamp
    let before = cam.center;
    cam.set_viewport(vec2(640.0, 400.0)); // smaller window: clamp loosens
    cam.pan(vec2(1000.0, 1000.0));
    assert!(cam.center.x >= before.x && cam.center.y >= before.y);
}

#[test]
fn a_map_smaller_than_the_viewport_pins_the_view_to_center() {
    // A 4x4 map is dwarfed by the 1280x800 viewport at default zoom, so
    // there is no room to pan: the clamp holds the center on the map's
    // midpoint however hard it is shoved.
    let mut cam = Camera::new(vec2(2.0, 2.0), 4, 4, vec2(1280.0, 800.0));
    let mid = vec2(2.0, 2.0);
    assert!((cam.center - mid).length() < 1e-4);
    for shove in [
        vec2(-500.0, -500.0),
        vec2(500.0, 500.0),
        vec2(500.0, -500.0),
    ] {
        cam.pan(shove);
        assert!(
            (cam.center - mid).length() < 1e-4,
            "pinned center drifted to {:?}",
            cam.center
        );
    }
}

#[test]
fn the_cursor_anchor_holds_through_a_multi_step_zoom_glide() {
    // The existing anchor test saturates the glide in one step; this
    // pins that every *intermediate* frame keeps the world point under
    // the cursor stationary, not just the final one.
    let mut cam = camera();
    let cursor = vec2(800.0, 500.0);
    let anchored = cam.to_world(cursor);
    cam.zoom_at(cursor, 1.0);
    for _ in 0..30 {
        cam.update(1.0 / 60.0); // small dt: the glide is mid-flight here
        assert!(
            anchor_drift(&cam, anchored, cursor) <= WHOLE_PIXEL_EASE,
            "anchor drifted mid-glide to {:?}",
            cam.to_screen(anchored)
        );
    }
    assert!(
        (cam.zoom - ZOOM_DEFAULT * 1.15).abs() < 0.01,
        "glide never reached its one-notch target"
    );
}

#[test]
fn a_zoom_glide_reports_itself_until_it_lands() {
    let mut cam = camera();
    assert!(!cam.zooming());
    cam.zoom_at(vec2(640.0, 400.0), 1.0);
    cam.update(1.0 / 60.0);
    assert!(cam.zooming());
    cam.update(1.0);
    assert!(!cam.zooming());
}

#[test]
fn panning_past_the_far_corner_stops_within_slack() {
    // `pan_clamps_to_map_with_slack` pins the lo corner; this pins the
    // hi corner. Shoved past the bottom-right, the visible rect reaches
    // the map edge but overshoots it by at most the two-tile slack.
    let mut cam = camera();
    cam.pan(vec2(1000.0, 1000.0));
    let (_, max) = cam.world_rect();
    assert!(
        max.x >= 40.0 && max.y >= 24.0,
        "clamp fell short of the map edge: {max:?}"
    );
    assert!(
        max.x <= 40.0 + 3.0 && max.y <= 24.0 + 3.0,
        "far edge ran away: {max:?}"
    );
}

#[test]
fn a_zoom_glide_comes_to_rest_with_the_world_origin_on_a_whole_pixel() {
    // A roomy map, then one so small the view stays pinned to its center.
    for (width, height) in [(128, 112), (24, 20)] {
        let mut camera = Camera::new(vec2(61.3, 47.9), width, height, vec2(1210.0, 834.0));
        for (anchor, notches) in [(vec2(400.0, 300.0), 1.0), (vec2(900.0, 120.0), -0.7)] {
            camera.zoom_at(anchor, notches);
            while camera.zooming() {
                camera.update(1.0 / 120.0);
            }
            let origin = camera.to_screen(Vec2::ZERO);
            assert!(
                (origin - origin.round()).abs().max_element() < 0.001,
                "{width} by {height}: origin {origin}"
            );
        }
    }
}

#[test]
fn a_zoom_glide_lands_without_a_visible_last_step() {
    let viewport = vec2(1210.0, 834.0);
    for notches in [0.5, 1.0, 3.0, -2.0] {
        let mut camera = Camera::new(vec2(61.3, 47.9), 128, 112, viewport);
        let far_corner = camera.to_world(viewport);
        camera.zoom_at(Vec2::ZERO, notches);
        let mut last_step = f32::MAX;
        while camera.zooming() {
            let before = camera.to_screen(far_corner);
            camera.update(1.0 / 120.0);
            last_step = (camera.to_screen(far_corner) - before).length();
        }
        assert!(
            last_step < 0.25,
            "{notches} notches ended on a {last_step} px step"
        );
    }
}
