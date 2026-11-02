use super::*;

fn tiered(rows: &[&str], levels: &[&str]) -> Map {
    Map::parse_tiered(rows, levels)
        .expect("the test map is a valid tiered map")
        .0
}

fn faces(map: &Map, terrain: Terrain) -> TallFaces {
    let field = CellField::seeded(
        TilePos::new(0, 0),
        map.width(),
        map.height(),
        CELLS,
        |pos| map.tile(pos).is_some_and(|tile| tile.terrain == terrain),
    );
    TallFaces::build(map, terrain, &field)
}

const RIM_LEVELS: [&str; 5] = [
    "000000001111",
    "000000001111",
    "000000001111",
    "000000001111",
    "000000001111",
];
const PIT_ROWS: [&str; 5] = [
    "..~~~~~~~~..",
    "..~~~~~~~~..",
    "..~~~~~~~~..",
    "..~~~~~~~~..",
    "..~~~~~~~~..",
];
const PEAK_ROWS: [&str; 5] = [
    "..^^^^^^^^..",
    "..^^^^^^^^..",
    "..^^^^^^^^..",
    "..^^^^^^^^..",
    "..^^^^^^^^..",
];

#[test]
fn a_flat_map_has_none() {
    let map = Map::parse(&["~~", "~~"]).unwrap().0;
    assert!(faces(&map, Terrain::Pit).cells.is_empty());
}

#[test]
fn a_shape_on_one_level_has_none() {
    let rows = ["..~~~~..", "..~~~~..", "........", "========", "........"];
    let levels = ["11111111", "11111111", "11111111", "00000000", "00000000"];
    assert!(
        faces(&tiered(&rows, &levels), Terrain::Pit)
            .cells
            .is_empty()
    );
}

#[test]
fn a_pits_tall_face_is_along_its_higher_ground() {
    let faces = faces(&tiered(&PIT_ROWS, &RIM_LEVELS), Terrain::Pit);
    let y = 2 * CELLS + 1;
    assert!(faces.in_face(10 * CELLS - 1, y, 1));
    assert!(faces.in_face(10 * CELLS - 2, y, 2));
    assert!(
        !faces.in_face(10 * CELLS - 3, y, 3),
        "the bench starts here"
    );
    assert!(!faces.in_face(2 * CELLS, y, 1), "the lower side");
}

#[test]
fn a_peaks_tall_face_is_along_its_lower_ground() {
    let faces = faces(&tiered(&PEAK_ROWS, &RIM_LEVELS), Terrain::Peak);
    let y = 2 * CELLS + 1;
    assert!(faces.in_face(2 * CELLS, y, 1));
    assert!(!faces.in_face(10 * CELLS - 1, y, 1), "the higher side");
}

#[test]
fn a_tall_side_ends_square_where_a_cliff_meets_the_shape() {
    // Level 1 north of the cliff and level 0 south of it, along the pit's east
    // side.
    let rows = [
        "............",
        "~~~~~~......",
        "~~~~~~......",
        "~~~~~~======",
        "~~~~~~......",
        "~~~~~~......",
        "............",
    ];
    let levels = [
        "111111111111",
        "111111111111",
        "111111111111",
        "000000000000",
        "000000000000",
        "000000000000",
        "000000000000",
    ];
    let faces = faces(&tiered(&rows, &levels), Terrain::Pit);
    for inward in 1..=3 {
        let x = 6 * CELLS - inward;
        assert!(faces.on_tall_side(x, 3 * CELLS - 1), "above, {inward} in");
        assert!(
            !faces.on_tall_side(x, 3 * CELLS),
            "at the cliff, {inward} in"
        );
    }
}

#[test]
fn a_shape_with_a_tall_face_keeps_its_first_bench_wide() {
    let faces = faces(&tiered(&PIT_ROWS, &RIM_LEVELS), Terrain::Pit);
    let y = 2 * CELLS + 1;
    assert_eq!(
        faces.widths(4 * CELLS, y, [1, 2, 3]),
        [MIN_FIRST_BENCH, 2, 3]
    );
    assert_eq!(faces.widths(0, y, [1, 2, 3]), [1, 2, 3], "off the shape");
}

/// Whether a half turn maps the map's terrain and levels onto themselves.
fn half_turned(map: &Map) -> bool {
    (0..map.height()).all(|y| {
        (0..map.width()).all(|x| {
            let (pos, turned) = (
                TilePos::new(x, y),
                TilePos::new(map.width() - 1 - x, map.height() - 1 - y),
            );
            map.tile(pos).map(|tile| tile.terrain) == map.tile(turned).map(|tile| tile.terrain)
                && map.elevation(pos) == map.elevation(turned)
        })
    })
}

#[test]
fn tall_sides_on_shipped_maps_mirror_as_the_maps_do() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).expect("the shipped maps") {
        let path = entry.expect("a map").path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let scenario = aftermine_sim::Scenario::load(&path).expect("a shipped map loads");
        let (map, _) = scenario
            .parse_map_and_anchors()
            .expect("shipped rows parse");
        if !map.tiered() || !half_turned(&map) {
            continue;
        }
        for terrain in [Terrain::Pit, Terrain::Peak] {
            let faces = faces(&map, terrain);
            checked += usize::from(!faces.cells.is_empty());
            if let Some(last) = faces.cells.len().checked_sub(1) {
                for (index, kind) in faces.cells.iter().enumerate() {
                    assert_eq!(
                        *kind,
                        faces.cells[last - index],
                        "{} at cell {index}",
                        path.display()
                    );
                }
            }
        }
    }
    assert!(checked > 0, "a half-turned shipped map has a tall face");
}
