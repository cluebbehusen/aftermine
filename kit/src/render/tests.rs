use super::*;

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("oxide-kit-render-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn save_png_creates_parents_and_writes_the_canonical_render() {
    let dir = scratch("save");
    let path = dir.join("nested/state.png");
    let state = oxide_sim::Scenario::skirmish().build().unwrap();

    save_png(&state, &path).unwrap();

    let saved = std::fs::read(&path).unwrap();
    assert_eq!(saved, png_bytes(&state).unwrap());
    let decoded = Pixmap::decode_png(&saved).expect("saved bytes are a PNG");
    assert_eq!(
        decoded.width(),
        u32::try_from(state.map().width()).unwrap() * TILE_PIXELS
    );
    assert_eq!(
        decoded.height(),
        u32::try_from(state.map().height()).unwrap() * TILE_PIXELS
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_extractor_frame_draws_as_a_gold_rimmed_block_unlike_rock() {
    let state = oxide_sim::Scenario::skirmish().build().unwrap();
    let frame = *state
        .map()
        .extractor_frames()
        .iter()
        .find(|frame| {
            !state
                .buildings()
                .iter()
                .any(|building| building.anchor == **frame)
        })
        .expect("an open extractor frame");
    let pixmap = render_state(&state);
    let at = |dx: u32, dy: u32| {
        let x = u32::try_from(frame.x).unwrap() * TILE_PIXELS + dx;
        let y = u32::try_from(frame.y).unwrap() * TILE_PIXELS + dy;
        let p = pixmap.pixel(x, y).expect("inside the map");
        (u32::from(p.red()) << 16) | (u32::from(p.green()) << 8) | u32::from(p.blue())
    };
    assert_eq!(at(1, 1), SCRAP_FULL, "the rim");
    assert_eq!(at(TILE_PIXELS, TILE_PIXELS), FRAME, "the block");
    assert_ne!(FRAME, ROCK);
}

#[test]
fn save_png_reports_a_parent_that_cannot_be_created() {
    let dir = scratch("bad-parent");
    let blocked = dir.join("not-a-directory");
    std::fs::write(&blocked, b"existing file").unwrap();
    let state = oxide_sim::Scenario::skirmish().build().unwrap();

    assert!(save_png(&state, &blocked.join("state.png")).is_err());
    assert_eq!(std::fs::read(&blocked).unwrap(), b"existing file");
    std::fs::remove_dir_all(dir).ok();
}

/// Level 2 over level 1 over level 0, each parted by a cliff at its foot.
fn three_levels() -> Map {
    Map::parse_tiered(
        &["....", "====", "....", "....", "====", "....", "...."],
        &["2222", "1111", "1111", "1111", "0000", "0000", "0000"],
    )
    .unwrap()
    .0
}

#[test]
fn the_floor_is_where_most_starts_stand_else_where_most_ground_lies() {
    let map = three_levels();
    let (top, middle, bottom) = (TilePos::new(0, 0), TilePos::new(0, 2), TilePos::new(0, 6));
    assert_eq!(floor_level(&map, [top, bottom, bottom]), 0);
    assert_eq!(floor_level(&map, [top, middle]), 1, "a tie goes lower");
    assert_eq!(floor_level(&map, []), 0, "levels 0 and 1 tie on ground");
}

#[test]
fn each_level_steps_its_tone_from_the_floor() {
    let shading = Shading::new(&three_levels(), 1);
    assert_eq!(shading.tone(1.0), 1.0);
    assert!((shading.tone(2.0) - (1.0 + LEVEL_STEP)).abs() < 1e-6);
    assert!((shading.tone(0.0) - (1.0 - LEVEL_STEP)).abs() < 1e-6);
}

#[test]
fn a_map_deeper_than_the_band_shares_it_so_every_level_keeps_its_tone() {
    // Four levels under a floor at 4: a full step each would run past the
    // darkest tone, so they divide the band below the floor evenly.
    let (deep, _) = Map::parse_tiered(
        &["..", "==", "..", "==", "..", "==", "..", "==", ".."],
        &["44", "33", "33", "22", "22", "11", "11", "00", "00"],
    )
    .unwrap();
    let shading = Shading::new(&deep, 4);
    let tones: Vec<f32> = (0..=4u8)
        .map(|level| shading.tone(f32::from(level)))
        .collect();
    assert!((tones[0] - LEVEL_DARKEST).abs() < 1e-6);
    assert_eq!(tones[4], 1.0);
    let step = (1.0 - LEVEL_DARKEST) / 4.0;
    for pair in tones.windows(2) {
        assert!((pair[1] - pair[0] - step).abs() < 1e-6, "{tones:?}");
    }
    // A climb past the band shares it the same way, its top level the
    // brightest tone.
    let (tall, _) = Map::parse_tiered(&["..", "==", ".."], &["99", "44", "44"]).unwrap();
    assert!((Shading::new(&tall, 4).tone(9.0) - LEVEL_BRIGHTEST).abs() < 1e-6);
}

#[test]
fn a_tiered_map_draws_its_levels_cliffs_and_ramps() {
    let mut scenario = oxide_sim::Scenario::skirmish();
    scenario.mode = oxide_sim::scenario::ScenarioMode::Sandbox;
    scenario.map = vec!["....".into(), "=/==".into(), "....".into()];
    scenario.levels = vec!["1111".into(), "0000".into(), "0000".into()];
    scenario.players.truncate(1);
    scenario.units.clear();
    scenario.buildings.clear();
    let state = scenario.build().unwrap();
    let pixmap = render_state(&state);
    let at = |x: u32, y: u32| {
        let p = pixmap.pixel(x, y).unwrap();
        (p.red(), p.green(), p.blue())
    };
    let tile = TILE_PIXELS;
    let center = tile / 2;
    let floor = at(center, 2 * tile + center);
    let plateau = at(center, center);
    assert!(plateau.0 > floor.0, "the upper level is brighter");
    let cliff_face = at(center, tile + 2);
    let cliff_foot = at(center, tile + tile - 1);
    assert_eq!(cliff_foot, floor, "a cliff shows the ground at its foot");
    assert!(cliff_face.0 < floor.0, "with the drop's face along its top");
    let ramp = at(tile + center, tile + center);
    assert!(
        floor.0 < ramp.0 && ramp.0 < plateau.0,
        "a ramp sits between"
    );
}
