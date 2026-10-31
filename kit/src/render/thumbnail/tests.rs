use super::*;

/// Every shipped map, built at its first tick.
fn shipped_states() -> Vec<(String, State)> {
    let mut paths: Vec<_> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../scenarios"))
        .expect("the shipped scenarios")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "premise: maps ship");
    paths
        .into_iter()
        .map(|path| {
            let state = aftermine_sim::Scenario::load(&path)
                .unwrap()
                .build()
                .unwrap();
            (path.display().to_string(), state)
        })
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "thumbnail pixel coordinates are small and positive"
)]
fn pixel(value: f32) -> u32 {
    value.floor() as u32
}

/// The pixels per tile a thumbnail of `state` takes inside `width` x
/// `height`.
fn scale_for(state: &State, width: u32, height: u32) -> f32 {
    let map = state.map();
    (width as f32 / map.width() as f32)
        .min(height as f32 / map.height() as f32)
        .min(TILE_PX)
}

fn color_at(pixmap: &Pixmap, x: u32, y: u32) -> u32 {
    let p = pixmap.pixel(x, y).expect("inside the pixmap");
    (u32::from(p.red()) << 16) | (u32::from(p.green()) << 8) | u32::from(p.blue())
}

/// Whether anything drawn after frames (a building, or a unit) lies within
/// a tile of the frame at `pos`.
fn frame_overlapped(state: &State, pos: TilePos) -> bool {
    let near = |t: TilePos| {
        (pos.x - 1..=pos.x + 2).contains(&t.x) && (pos.y - 1..=pos.y + 2).contains(&t.y)
    };
    state.buildings().iter().any(|b| b.tiles().any(near))
        || state
            .units()
            .iter()
            .any(|u| near(TilePos::containing(u.pos)))
}

/// The map list's thumbnail boxes on an iPad (2x) and a 1080p screen (1x),
/// a cramped window's, and a match setup preview's between half and full
/// scale.
const BOXES: [(u32, u32); 4] = [(523, 245), (261, 122), (90, 60), (900, 900)];

#[test]
fn every_open_frame_keeps_its_gold_rim_on_all_four_sides() {
    for (name, state) in shipped_states() {
        let map = state.map();
        for (width, height) in BOXES {
            let pixmap = render_thumbnail(&state, width, height, |_| 0x96_94_8C);
            let scale = scale_for(&state, width, height);
            for &frame in map.extractor_frames() {
                if frame_overlapped(&state, frame) {
                    continue;
                }
                let cx = pixel((frame.x + 1) as f32 * scale);
                let cy = pixel((frame.y + 1) as f32 * scale);
                assert_eq!(
                    color_at(&pixmap, cx, cy),
                    FRAME,
                    "{name} at {width}x{height}: the frame at {frame:?} shows its core"
                );
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (mut x, mut y) = (cx, cy);
                    while color_at(&pixmap, x, y) == FRAME {
                        x = x.checked_add_signed(dx).unwrap();
                        y = y.checked_add_signed(dy).unwrap();
                    }
                    assert_eq!(
                        color_at(&pixmap, x, y),
                        SCRAP_FULL,
                        "{name} at {width}x{height}: the frame at {frame:?} keeps its rim \
                         towards ({dx}, {dy})"
                    );
                }
            }
        }
    }
}

#[test]
fn a_thumbnail_fits_its_box_and_spans_one_side() {
    let state = aftermine_sim::Scenario::skirmish().build().unwrap();
    for (width, height) in BOXES.into_iter().chain([(400, 50), (1, 1), (0, 0)]) {
        let pixmap = render_thumbnail(&state, width, height, |_| 0x96_94_8C);
        let (w, h) = (pixmap.width(), pixmap.height());
        let (width, height) = (width.max(1), height.max(1));
        assert!(w <= width && h <= height, "{w}x{h} fits {width}x{height}");
        let full = (
            u32::try_from(state.map().width()).unwrap() * TILE_PIXELS,
            u32::try_from(state.map().height()).unwrap() * TILE_PIXELS,
        );
        assert!(
            w == width || h == height || (w, h) == full,
            "{w}x{h} spans one side of {width}x{height} unless it reached full size"
        );
    }
}

#[test]
fn the_scale_stops_at_the_full_size_preview() {
    let state = aftermine_sim::Scenario::skirmish().build().unwrap();
    let full = render_state(&state);
    let pixmap = render_thumbnail(&state, 16_000, 16_000, seat_color);
    assert_eq!(
        (pixmap.width(), pixmap.height()),
        (full.width(), full.height())
    );
}

#[test]
fn at_full_scale_ground_frames_and_full_nodes_match_the_full_size_preview() {
    for (name, state) in shipped_states() {
        let full = render_state(&state);
        let thumbnail = render_thumbnail(&state, full.width(), full.height(), seat_color);
        let map = state.map();
        // Health bars and units draw past their tiles in the full-size
        // preview, and a partly mined node rounds its square differently.
        let busy = |pos: TilePos| {
            let near = |t: TilePos| (t.x - pos.x).abs() <= 1 && (t.y - pos.y).abs() <= 1;
            state.buildings().iter().any(|b| b.tiles().any(near))
                || state
                    .units()
                    .iter()
                    .any(|u| near(TilePos::containing(u.pos)))
                || map.tile(pos).is_some_and(|tile| {
                    tile.scrap > 0 && tile.scrap < aftermine_sim::stats::SCRAP_NODE_AMOUNT
                })
        };
        let mut compared = 0;
        for (pos, _) in map.iter() {
            if busy(pos) {
                continue;
            }
            let x0 = u32::try_from(pos.x).unwrap() * TILE_PIXELS;
            let y0 = u32::try_from(pos.y).unwrap() * TILE_PIXELS;
            for y in y0..y0 + TILE_PIXELS {
                for x in x0..x0 + TILE_PIXELS {
                    assert_eq!(
                        color_at(&thumbnail, x, y),
                        color_at(&full, x, y),
                        "{name}: pixel ({x}, {y}) of tile {pos:?}"
                    );
                }
            }
            compared += 1;
        }
        assert!(compared > 0, "{name}: premise: open ground to compare");
    }
}

#[test]
fn a_small_peak_takes_the_mean_tone_of_a_full_size_one() {
    let mut pixmap = Pixmap::new(TILE_PIXELS, TILE_PIXELS).unwrap();
    draw_peak(&mut pixmap, Cell::full(TilePos::new(0, 0)));
    let count = TILE_PIXELS * TILE_PIXELS;
    let mean = |shift: u32| {
        let sum: u32 = (0..TILE_PIXELS)
            .flat_map(|y| (0..TILE_PIXELS).map(move |x| (x, y)))
            .map(|(x, y)| (color_at(&pixmap, x, y) >> shift) & 0xff)
            .sum();
        (sum + count / 2) / count
    };
    assert_eq!((mean(16) << 16) | (mean(8) << 8) | mean(0), PEAK_MEAN);
}
