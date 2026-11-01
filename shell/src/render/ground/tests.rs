use super::*;
use crate::camera::Camera;

/// A camera holding still at about `zoom`.
fn camera(zoom: f32) -> Camera {
    let viewport = vec2(1280.0, 800.0);
    let mut camera = Camera::new(vec2(20.3, 11.7), 40, 24, viewport);
    camera.zoom_at(viewport * 0.5, (zoom / camera.zoom).log(1.15));
    camera.update(1.0);
    assert!(!camera.zooming());
    camera
}

/// A camera at about `zoom` with a glide just begun.
fn gliding(zoom: f32) -> Camera {
    let mut camera = camera(zoom);
    camera.zoom_at(vec2(333.0, 271.0), 1.0);
    camera.pan(vec2(0.137, 0.211));
    assert!(camera.zooming());
    camera
}

fn open_map(width: usize, height: usize, rock: &[(usize, usize)]) -> Map {
    let rows: Vec<String> = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| if rock.contains(&(x, y)) { '#' } else { '.' })
                .collect()
        })
        .collect();
    Map::parse(&rows).expect("rows parse").0
}

#[test]
fn a_baked_chunk_lands_on_the_pixels_drawn_straight_to_the_screen() {
    for zoom in [8.0, 32.0, 32.0 * 1.15 * 1.15 * 1.15, 96.0] {
        let live = GroundView::live(&camera(zoom));
        let zoom = live.zoom();
        assert_eq!(
            live.origin,
            live.origin.floor(),
            "world (0, 0) is pixel-aligned"
        );
        for chunk in [(0, 0), (1, 2), (-1, 0), (3, -1)] {
            let baked = GroundView::chunk(zoom, chunk);
            let corner = live.origin + vec2(chunk.0 as f32, chunk.1 as f32) * CHUNK;
            let (lo, hi) = baked.world_rect();
            // Quarter-tile corners, where every pass floors its edges.
            for qy in 0..=numeric::to_i32((hi.y - lo.y) * 4.0) {
                for qx in 0..=numeric::to_i32((hi.x - lo.x) * 4.0) {
                    let world = lo.floor() + vec2(qx as f32, qy as f32) * 0.25;
                    assert_eq!(
                        live.to_screen(world).floor(),
                        corner + baked.to_screen(world).floor(),
                        "zoom {zoom}, chunk {chunk:?}, world {world}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_chunk_covers_every_tile_that_can_reach_into_it() {
    let zoom = 32.0;
    let view = GroundView::chunk(zoom, (2, 1));
    let (lo, hi) = view.world_rect();
    let corner = vec2(2.0, 1.0) * CHUNK / zoom;
    assert_eq!(lo, corner - Vec2::splat(REACH));
    assert_eq!(hi, corner + Vec2::splat(CHUNK / zoom + REACH));
    assert_eq!(view.to_screen(corner), Vec2::ZERO);
}

#[test]
fn the_chunks_on_show_tile_the_screen_over_the_layer() {
    for (camera, stretch) in [
        (camera(8.0), 1.0),
        (camera(32.0), 1.0),
        (camera(96.0), 1.0),
        (gliding(11.0), 1.375),
        (gliding(16.0), 2.0),
    ] {
        let view = GroundView::live(&camera);
        let zoom = view.zoom();
        let viewport = camera.viewport();
        let extent = (Vec2::ZERO, vec2(40.0, 24.0));
        let side = CHUNK * stretch;
        let chunks: Vec<(i32, i32)> = chunks_in_view(&view, viewport, extent, side).collect();
        let screen_rect = |chunk: (i32, i32)| {
            let (lo, hi) = chunk_on_screen(&view, chunk, side);
            Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
        };
        let ground = Rect::new(
            view.origin.x,
            view.origin.y,
            extent.1.x * zoom,
            extent.1.y * zoom,
        );
        let screen = Rect::new(0.0, 0.0, viewport.x, viewport.y);
        for &chunk in &chunks {
            let rect = screen_rect(chunk);
            assert!(
                rect.overlaps(&screen),
                "zoom {zoom}: {chunk:?} is off screen"
            );
            assert!(
                rect.overlaps(&ground),
                "zoom {zoom}: {chunk:?} is off the layer"
            );
        }
        // Every pixel of the layer on screen lies in exactly one chunk.
        for y in (0..800).step_by(7) {
            for x in (0..1280).step_by(7) {
                let pixel = vec2(x as f32, y as f32);
                if !ground.contains(pixel) {
                    continue;
                }
                let holders = chunks
                    .iter()
                    .filter(|&&chunk| {
                        let rect = screen_rect(chunk);
                        pixel.x >= rect.x
                            && pixel.y >= rect.y
                            && pixel.x < rect.x + rect.w
                            && pixel.y < rect.y + rect.h
                    })
                    .count();
                assert_eq!(holders, 1, "zoom {zoom}: pixel {pixel}");
            }
        }
    }
}

#[test]
fn an_unstretched_chunk_keeps_its_baked_size() {
    let view = GroundView::live(&camera(32.0));
    let (lo, hi) = chunk_on_screen(&view, (2, -1), CHUNK);
    assert_eq!(lo, view.origin + vec2(2.0, -1.0) * CHUNK);
    assert_eq!(hi - lo, Vec2::splat(CHUNK));
}

/// A cache holding every chunk of a settled view of a 128 by 112 map.
fn settled_cache(map: &Map, center: Vec2, zoom: f32) -> GroundCache {
    let view = GroundView::live(&wide_camera(center, zoom));
    let chunks = chunks_of_view(map, &view, VIEWPORT, CHUNK)
        .into_iter()
        .map(|key| {
            (
                key,
                Chunk {
                    target: None,
                    used: 0,
                },
            )
        })
        .collect();
    GroundCache {
        scale: Some((zoom, 2.0)),
        chunks,
        ..GroundCache::default()
    }
}

const VIEWPORT: Vec2 = vec2(1210.0, 834.0);

fn wide_camera(center: Vec2, zoom: f32) -> Camera {
    let mut camera = Camera::new(center, 128, 112, VIEWPORT);
    camera.zoom = zoom;
    camera
}

#[test]
fn a_zoom_in_holds_every_chunk_it_shows_and_a_zoom_out_only_some() {
    let map = open_map(128, 112, &[]);
    let center = vec2(64.0, 56.0);
    let cache = settled_cache(&map, center, 16.0);
    for (zoom, stretch) in [(16.0, 1.0), (20.0, 1.25), (32.0, 2.0), (96.0, 6.0)] {
        let view = GroundView::live(&wide_camera(center, zoom));
        let on_hand = cache.on_hand(&map, &view, VIEWPORT);
        assert_eq!(on_hand.side, CHUNK * stretch);
        assert_eq!(
            on_hand.keys.len(),
            chunks_of_view(&map, &view, VIEWPORT, on_hand.side).len(),
            "zoom {zoom} reveals nothing new"
        );
    }
    let view = GroundView::live(&wide_camera(center, 8.0));
    let on_hand = cache.on_hand(&map, &view, VIEWPORT);
    assert_eq!(on_hand.side, CHUNK * 0.5);
    let all = chunks_of_view(&map, &view, VIEWPORT, on_hand.side).len();
    assert!(!on_hand.keys.is_empty() && on_hand.keys.len() < all);
}

#[test]
fn whichever_chunks_are_resampled_least_go_on_top() {
    assert!(fresh_goes_on_top(CHUNK * 4.0, CHUNK * 0.8));
    assert!(!fresh_goes_on_top(CHUNK * 1.1, CHUNK * 0.5));
    assert!(fresh_goes_on_top(CHUNK * 0.4, CHUNK * 1.5));
    assert!(!fresh_goes_on_top(CHUNK * 0.9, CHUNK * 3.0));
}

#[test]
fn fresh_chunks_replace_the_ones_on_show_all_at_once() {
    let map = open_map(128, 112, &[]);
    let center = vec2(64.0, 56.0);
    let mut cache = settled_cache(&map, center, 16.0);
    let fresh = (Layer::Floor, 40, 40);
    cache.next.insert(
        fresh,
        Chunk {
            target: None,
            used: 0,
        },
    );
    cache.next_zoom = Some(24.0);
    cache.smooth = true;
    cache.promote_next(24.0, 2.0);
    assert_eq!(cache.scale, Some((24.0, 2.0)));
    assert_eq!(cache.chunks.keys().copied().collect::<Vec<_>>(), [fresh]);
    assert!(cache.next.is_empty());
    assert_eq!(cache.next_zoom, None);
    assert!(!cache.smooth);
}

#[test]
fn a_gliding_view_stays_exact_and_a_still_one_lands_on_a_whole_pixel() {
    let moving = gliding(12.0);
    assert_eq!(
        GroundView::live(&moving).origin,
        moving.to_screen(Vec2::ZERO)
    );
    let still = GroundView::live(&camera(12.0));
    assert_eq!(still.origin, still.origin.round());
}

#[test]
fn images_on_show_never_overlap() {
    let whole = (vec2(0.0, 0.0), vec2(100.0, 100.0));
    assert_eq!(uncovered_parts(whole, &[]), [whole]);
    let apart = (vec2(200.0, 200.0), vec2(300.0, 300.0));
    assert_eq!(parts_outside(whole, apart), [whole]);
    let over = (vec2(-1.0, -1.0), vec2(101.0, 101.0));
    assert!(parts_outside(whole, over).is_empty());

    let area = |(lo, hi): (Vec2, Vec2)| (hi.x - lo.x) * (hi.y - lo.y);
    for covered in [
        (vec2(40.0, 30.0), vec2(60.0, 70.0)),
        (vec2(-10.0, 30.0), vec2(60.0, 70.0)),
        (vec2(40.0, -10.0), vec2(110.0, 70.0)),
    ] {
        let parts = parts_outside(whole, covered);
        let inside = (covered.0.max(whole.0), covered.1.min(whole.1));
        let total: f32 = parts.into_iter().map(area).sum();
        assert_eq!(total, area(whole) - area(inside));
    }

    // Two neighboring chunks over a larger image leave just the rest of it.
    let left = (vec2(10.0, 10.0), vec2(50.0, 50.0));
    let right = (vec2(50.0, 10.0), vec2(90.0, 50.0));
    let parts = uncovered_parts(whole, &[left, right]);
    let total: f32 = parts.iter().copied().map(area).sum();
    assert_eq!(total, area(whole) - area(left) - area(right));
    for &(lo, hi) in &parts {
        for rect in [left, right] {
            assert!(hi.x <= rect.0.x || lo.x >= rect.1.x || hi.y <= rect.0.y || lo.y >= rect.1.y);
        }
    }
}

#[test]
fn the_view_a_glide_bakes_ahead_for_is_the_one_it_lands_on() {
    let center = vec2(61.3, 47.9);
    for (from, notches, anchor) in [
        (10.0, 1.0, vec2(400.0, 300.0)),
        (8.5, 0.5, vec2(605.0, 417.0)),
        (24.0, 4.0, vec2(900.0, 200.0)),
    ] {
        let mut camera = Camera::new(center, 128, 112, VIEWPORT);
        camera.zoom_at(VIEWPORT * 0.5, (from / camera.zoom).log(1.15));
        camera.update(1.0);
        camera.zoom_at(anchor, notches);
        let ahead = GroundView::landing(&camera);
        while camera.zooming() {
            camera.update(1.0 / 120.0);
        }
        let landed = GroundView::live(&camera);
        assert_eq!(ahead.zoom, landed.zoom);
        assert!(
            (ahead.origin - landed.origin).abs().max_element() <= 1.0,
            "from {from}: {} against {}",
            ahead.origin,
            landed.origin
        );
    }
}

#[test]
fn a_raised_chunk_is_empty_only_over_open_floor_inside_the_map() {
    let map = open_map(40, 40, &[(3, 3)]);
    let zoom = 32.0;
    // Chunks of eight tiles: the first holds the rock, the next reaches it
    // through a slab's overhang, a middle one sees only floor, and one on
    // the edge reaches past the map.
    assert!(!raised_is_empty(&map, &GroundView::chunk(zoom, (0, 0))));
    assert!(!raised_is_empty(&map, &GroundView::chunk(zoom, (1, 0))));
    assert!(raised_is_empty(&map, &GroundView::chunk(zoom, (2, 2))));
    assert!(!raised_is_empty(&map, &GroundView::chunk(zoom, (4, 2))));
}

#[test]
fn the_least_recently_drawn_chunks_go_first() {
    let chunks: BTreeMap<ChunkKey, Chunk> = [
        ((Layer::Floor, 0, 0), 5),
        ((Layer::Raised, 0, 0), 2),
        ((Layer::Floor, 1, 0), 9),
        ((Layer::Floor, 2, 0), 1),
    ]
    .into_iter()
    .map(|(key, used)| (key, Chunk { target: None, used }))
    .collect();
    assert_eq!(
        stalest(&chunks, 2),
        vec![(Layer::Floor, 2, 0), (Layer::Raised, 0, 0)]
    );
    assert!(stalest(&chunks, 0).is_empty());
}
