use super::*;

#[test]
fn parses_legend_and_anchors() {
    let (map, anchors) = Map::parse(&["#.s", ".1.", "..2"]).unwrap();
    assert_eq!(map.width(), 3);
    assert_eq!(map.height(), 3);
    assert!(!map.terrain_passable(TilePos::new(0, 0)));
    assert!(map.terrain_passable(TilePos::new(1, 0)));
    assert!(!map.terrain_passable(TilePos::new(2, 0)), "scrap blocks");
    assert_eq!(map.scrap_at(TilePos::new(2, 0)), SCRAP_NODE_AMOUNT);
    // Anchor tiles are plain ground.
    assert!(map.terrain_passable(TilePos::new(1, 1)));
    assert_eq!(
        anchors,
        vec![
            (PlayerId(0), TilePos::new(1, 1)),
            (PlayerId(1), TilePos::new(2, 2)),
        ]
    );
}

#[test]
fn depleted_node_becomes_passable() {
    let (mut map, _) = Map::parse(&["s"]).unwrap();
    let pos = TilePos::new(0, 0);
    for left in (0..SCRAP_NODE_AMOUNT).rev() {
        assert_eq!(map.extract_scrap(pos), Some(left));
    }
    assert_eq!(map.extract_scrap(pos), None);
    assert!(map.terrain_passable(pos));
}

#[test]
fn rejects_ragged_and_unknown() {
    assert!(matches!(
        Map::parse(&["..", "..."]),
        Err(MapError::Ragged { row: 1, .. })
    ));
    assert!(matches!(
        Map::parse(&["..", ".x"]),
        Err(MapError::UnknownChar { c: 'x', x: 1, y: 1 })
    ));
    assert!(matches!(
        Map::parse(&["11"]),
        Err(MapError::DuplicateAnchor(PlayerId(0)))
    ));
}

#[test]
fn ascii_roundtrip() {
    let rows = ["#.s", ",S.", "s#."];
    let (map, _) = Map::parse(&rows).unwrap();
    assert_eq!(map.ascii_rows(), rows);
}

/// A plateau at level 2 over a floor at level 1: a cliff between them and a
/// ramp through it.
const TIERED: [&str; 4] = ["....", "=/==", "....", "...."];
const TIERED_LEVELS: [&str; 4] = ["2222", "1111", "1111", "1111"];

#[test]
fn tiered_rows_parse_and_round_trip() {
    let (map, _) = Map::parse_tiered(&TIERED, &TIERED_LEVELS).unwrap();
    assert!(map.tiered());
    assert_eq!(map.elevation(TilePos::new(0, 0)), Some(Elevation::Level(2)));
    assert_eq!(
        map.elevation(TilePos::new(1, 1)),
        Some(Elevation::Ramp { low: 1 })
    );
    assert_eq!(map.elevation(TilePos::new(0, 3)), Some(Elevation::Level(1)));
    assert!(
        !map.terrain_passable(TilePos::new(0, 1)),
        "cliffs block ground"
    );
    assert!(!map.tile(TilePos::new(0, 1)).unwrap().terrain.blocks_air());
    assert!(map.terrain_passable(TilePos::new(1, 1)), "ramps are ground");
    assert!(map.is_ramp(TilePos::new(1, 1)));
    assert_eq!(map.ascii_rows(), TIERED);
    assert_eq!(map.level_rows(), TIERED_LEVELS);
}

#[test]
fn levels_past_nine_use_letters() {
    let levels = ["bbbb", "aaaa", "aaaa", "aaaa"];
    let (map, _) = Map::parse_tiered(&["....", "=/=/", "....", "...."], &levels).unwrap();
    assert_eq!(
        map.elevation(TilePos::new(0, 0)),
        Some(Elevation::Level(11))
    );
    assert_eq!(
        map.elevation(TilePos::new(3, 1)),
        Some(Elevation::Ramp { low: 10 })
    );
    assert_eq!(map.level_rows(), levels);
}

#[test]
fn a_single_level_layer_is_a_flat_map() {
    let rows = ["#.s", "...", ".~."];
    let (flat, _) = Map::parse(&rows).unwrap();
    let (layered, _) = Map::parse_tiered(&rows, &["444", "444", "444"]).unwrap();
    assert!(!layered.tiered());
    assert_eq!(layered, flat);
}

#[test]
fn cliffs_and_ramps_need_a_levels_layer() {
    assert_eq!(
        Map::parse(&["..", ".="]).unwrap_err(),
        MapError::Untiered(TilePos::new(1, 1))
    );
    assert_eq!(
        Map::parse(&["/."]).unwrap_err(),
        MapError::Untiered(TilePos::new(0, 0))
    );
    assert_eq!(
        Map::parse_tiered(&["=."], &["11"]).unwrap_err(),
        MapError::Untiered(TilePos::new(0, 0)),
        "a single level is flat, and a flat map has no cliffs"
    );
}

#[test]
fn the_levels_layer_matches_the_map_in_known_glyphs() {
    assert_eq!(
        Map::parse_tiered(&["..", ".."], &["12"]).unwrap_err(),
        MapError::LevelRows {
            width: 2,
            height: 2
        }
    );
    assert_eq!(
        Map::parse_tiered(&["..", ".."], &["12", "1"]).unwrap_err(),
        MapError::LevelRows {
            width: 2,
            height: 2
        }
    );
    assert_eq!(
        Map::parse_tiered(&["..", ".."], &["1A", "11"]).unwrap_err(),
        MapError::UnknownLevel { c: 'A', x: 1, y: 0 }
    );
}

#[test]
fn levels_meet_only_through_cliffs_and_ramps() {
    // Ground at two levels touching, diagonals included.
    for levels in [["12", "11"], ["11", "21"]] {
        assert!(matches!(
            Map::parse_tiered(&["..", ".."], &levels),
            Err(MapError::LevelBreak(_))
        ));
    }
    // A ramp joins only the level above its own.
    assert!(matches!(
        Map::parse_tiered(&["...", "./.", "..."], &["333", "313", "111"]),
        Err(MapError::LevelBreak(_))
    ));
    // A cliff stands no higher than the ground beside it.
    assert!(matches!(
        Map::parse_tiered(&["...", "===", "..."], &["222", "222", "111"]),
        Err(MapError::LevelBreak(_))
    ));
    // Two levels apart behind one cliff is a taller cliff, which is fine.
    assert!(Map::parse_tiered(&["...", "===", "..."], &["333", "111", "111"]).is_ok());
}

#[test]
fn a_forged_levels_layer_is_inconsistent() {
    let (map, _) = Map::parse_tiered(&TIERED, &TIERED_LEVELS).unwrap();
    assert!(map.is_consistent());
    let forge = |change: &dyn Fn(&mut Map)| {
        let mut forged = map.clone();
        change(&mut forged);
        forged.is_consistent()
    };
    assert!(!forge(
        &|m| m.elevation = Some(Grid::new(4, 4, Elevation::Level(1)))
    ));
    assert!(!forge(
        &|m| m.elevation = Some(Grid::new(3, 4, Elevation::Level(1)))
    ));
    assert!(!forge(&|m| {
        let grid = m.elevation.as_mut().unwrap();
        *grid.get_mut(TilePos::new(0, 1)).unwrap() = Elevation::Ramp { low: 1 };
    }));
    assert!(!forge(&|m| m.elevation = None));
    assert!(!forge(&|m| {
        let grid = m.elevation.as_mut().unwrap();
        *grid.get_mut(TilePos::new(1, 1)).unwrap() = Elevation::Ramp { low: u8::MAX };
    }));
}

#[test]
fn an_extractor_frame_stays_off_ramps() {
    assert_eq!(
        Map::parse_tiered(&[".E..", "=//=", "....", "...."], &TIERED_LEVELS).unwrap_err(),
        MapError::BadFrame(TilePos::new(1, 0))
    );
    let (map, _) = Map::parse_tiered(&["....", "=/==", "E...", "...."], &TIERED_LEVELS).unwrap();
    assert!(map.is_consistent());
    let mut forged = map;
    *forged
        .elevation
        .as_mut()
        .unwrap()
        .get_mut(TilePos::new(0, 3))
        .unwrap() = Elevation::Ramp { low: 1 };
    assert!(
        !forged.is_consistent(),
        "a ramp under a frame loads no more than it parses"
    );
}

#[test]
fn wrecks_that_fall_on_a_cliff_are_gone() {
    let (mut map, _) = Map::parse_tiered(&TIERED, &TIERED_LEVELS).unwrap();
    let (cliff, ramp) = (TilePos::new(0, 1), TilePos::new(1, 1));
    map.add_wreck(cliff, 5);
    map.add_wreck(ramp, 5);
    assert_eq!(map.wreck_at(cliff), 0);
    assert_eq!(map.wreck_at(ramp), 5, "a ramp is ground and keeps salvage");
}
