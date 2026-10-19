use super::*;
use crate::camera::Camera;

fn camera(zoom: f32) -> Camera {
    let mut camera = Camera::new(vec2(20.3, 11.7), 40, 24, vec2(1280.0, 800.0));
    camera.zoom = zoom;
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
    for zoom in [8.0, 32.0, 96.0] {
        let camera = camera(zoom);
        let view = GroundView::live(&camera);
        let viewport = camera.viewport();
        let extent = (Vec2::ZERO, vec2(40.0, 24.0));
        let chunks: Vec<(i32, i32)> = chunks_in_view(&view, viewport, extent).collect();
        let screen_rect = |chunk: (i32, i32)| {
            let corner = view.origin + vec2(chunk.0 as f32, chunk.1 as f32) * CHUNK;
            Rect::new(corner.x, corner.y, CHUNK, CHUNK)
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
