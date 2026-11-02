use super::*;

/// A plateau at level 2 over a floor at level 1, a cliff along the floor's
/// edge, and a ramp cut two tiles into the plateau before it passes the
/// cliff.
const ROWS: [&str; 8] = [
    "..........",
    "...///....",
    "...///....",
    "===///====",
    "..........",
    "..........",
    "..........",
    "..........",
];
const LEVELS: [&str; 8] = [
    "2222222222",
    "2221112222",
    "2221112222",
    "1111111111",
    "1111111111",
    "1111111111",
    "1111111111",
    "1111111111",
];

fn map(rows: &[&str], levels: &[&str]) -> Map {
    Map::parse_tiered(rows, levels)
        .expect("the test map is a valid tiered map")
        .0
}

fn ledge() -> Levels {
    Levels::build(&map(&ROWS, &LEVELS), 1).expect("a tiered map has levels")
}

fn ops_at(levels: &Levels, pos: TilePos) -> &[Op] {
    let tile = as_index(pos.y * levels.width + pos.x);
    let (start, end) = (levels.starts[tile], levels.starts[tile + 1]);
    &levels.ops[start.fit::<usize>()..end.fit::<usize>()]
}

fn has_riser(ops: &[Op], (sx, sy): (u8, u8)) -> bool {
    ops.iter()
        .any(|op| (op.sx, op.sy) == (sx, sy) && op.color == LAYERS[0].riser)
}

fn has_face(ops: &[Op], (sx, sy): (u8, u8)) -> bool {
    ops.iter().any(|op| {
        (op.sx, op.sy) == (sx, sy)
            && (op.color == riser_tone(true) || op.color == riser_tone(false))
    })
}

#[test]
fn a_flat_map_has_no_levels() {
    let (flat, _) = Map::parse(&["...", "..."]).unwrap();
    assert!(Levels::build(&flat, 0).is_none());
}

#[test]
fn the_step_stands_on_the_upper_ground_and_the_cliff_shows_its_foot() {
    let levels = ledge();
    assert_eq!(
        levels.class_at(0, 2),
        Class::Band { upper: 2, lower: 1 },
        "the upper ground's last tile carries the step"
    );
    assert_eq!(levels.class_at(0, 1), Class::Open(2));
    assert_eq!(
        levels.class_at(0, 3),
        Class::Open(1),
        "the cliff tile reads as the floor"
    );
    assert_eq!(levels.tile_tone(TilePos::new(0, 3)), Some(1.0));
    assert!(levels.tile_tone(TilePos::new(0, 2)) > Some(1.0));
}

#[test]
fn the_face_reaches_one_cell_into_the_cliff_tile() {
    let levels = ledge();
    let cliff = ops_at(&levels, TilePos::new(0, 3));
    assert!(has_face(cliff, (0, 0)), "the face's first cell is dark");
    assert!(
        cliff
            .iter()
            .all(|op| op.sy == 0 || op.color == CLIFF_SHADOW),
        "below its first row of cells the cliff tile is bare floor under a shadow"
    );
    assert!(
        ops_at(&levels, TilePos::new(0, 4)).is_empty(),
        "the floor beyond the cliff is untouched"
    );
}

#[test]
fn the_ramp_mouth_keeps_its_u() {
    let levels = ledge();
    // The ramp runs south through columns 3 to 5 and passes the cliff near
    // its foot. Its outermost cells take the cliff's face and floor, and the
    // wall beside it runs a cell further down below the face on each side.
    let (west, east) = (TilePos::new(3, 3), TilePos::new(5, 3));
    assert!(has_riser(ops_at(&levels, west), (0, 1)));
    assert!(has_riser(ops_at(&levels, east), (3, 1)));
    assert!(
        !has_riser(ops_at(&levels, TilePos::new(4, 3)), (1, 1)),
        "the middle of the ramp stays open road"
    );
}

#[test]
fn a_cliff_face_takes_the_light_by_the_way_it_turns() {
    let away = ledge();
    assert!(
        ops_at(&away, TilePos::new(0, 3))
            .iter()
            .any(|op| op.color == riser_tone(false)),
        "a drop to the south turns from the light"
    );
    let rows = ["..........", "==========", "..........", ".........."];
    let heights = ["1111111111", "1111111111", "2222222222", "2222222222"];
    let toward = Levels::build(&map(&rows, &heights), 1).expect("a tiered map has levels");
    assert!(
        (0..4).any(|y| ops_at(&toward, TilePos::new(0, y))
            .iter()
            .any(|op| op.color == riser_tone(true))),
        "a drop to the north turns toward it"
    );
}

#[test]
fn the_shadow_lies_below_a_cliff_and_stops_short_of_the_ramp() {
    let levels = ledge();
    let shaded = |x: i32| {
        ops_at(&levels, TilePos::new(x, 3))
            .iter()
            .any(|op| op.color == CLIFF_SHADOW)
    };
    assert!(shaded(0), "the shadow lies along the open cliff");
    assert!(
        !shaded(3) && !shaded(4) && !shaded(5),
        "the ramp mouth stays clear"
    );
    assert!(
        ops_at(&levels, TilePos::new(2, 3))
            .iter()
            .filter(|op| op.color == CLIFF_SHADOW)
            .all(|op| op.sx < 2),
        "it stops two cells short of the ramp"
    );
}

#[test]
fn a_tall_cliff_takes_the_tallest_drop_beside_it() {
    // Level 3 over level 0 behind a cliff two tiles thick.
    let rows = ["....", "====", "====", "...."];
    let levels = Levels::build(&map(&rows, &["3333", "0000", "0000", "0000"]), 0).unwrap();
    assert_eq!(levels.class_at(1, 1), Class::Band { upper: 3, lower: 0 });
    assert_eq!(levels.class_at(1, 2), Class::Open(0));
}

#[test]
fn every_valid_shape_lays_out() {
    let shapes: [(&[&str], &[&str]); 4] = [
        // A mesa with a ramp down each side.
        (
            &[
                "...........",
                "...........",
                "..==///==..",
                "..=.....=..",
                "../...../..",
                "../...../..",
                "../...../..",
                "..=.....=..",
                "..==///==..",
                "...........",
                "...........",
            ],
            &[
                "00000000000",
                "00000000000",
                "00000000000",
                "00011111000",
                "00011111000",
                "00011111000",
                "00011111000",
                "00011111000",
                "00000000000",
                "00000000000",
                "00000000000",
            ],
        ),
        // A cliff along the map's edge with nothing below it.
        (&["=====", ".....", "....."], &["00000", "11111", "11111"]),
        // Six levels stacked behind one-tile cliffs.
        (
            &[
                "....", "====", "....", "====", "....", "====", "....", "====", "....", "====",
                "....",
            ],
            &[
                "5555", "4444", "4444", "3333", "3333", "2222", "2222", "1111", "1111", "0000",
                "0000",
            ],
        ),
        // A single raised tile.
        (
            &[".....", ".===.", ".=.=.", ".===.", "....."],
            &["00000", "00000", "00100", "00000", "00000"],
        ),
    ];
    for (rows, levels) in shapes {
        let laid = Levels::build(&map(rows, levels), 0).expect("a tiered map has levels");
        assert_eq!(laid.starts.len(), as_index(laid.width * laid.height) + 1);
        assert_eq!(
            laid.starts.last().copied(),
            Some(laid.ops.len().fit::<u32>())
        );
        assert!(laid.starts.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}

#[test]
fn bright_levels_wash_their_sprites_and_dark_ones_dim_them() {
    let levels = Levels::build(&map(&ROWS, &LEVELS), 0).unwrap();
    let (f, wash) = levels.tile_paint(TilePos::new(0, 0), 0).unwrap();
    assert!(
        f == 1.0 && wash > 0.0,
        "level 2 over a floor at 0 is lifted"
    );
    let levels = Levels::build(&map(&ROWS, &LEVELS), 3).unwrap();
    let (f, wash) = levels.tile_paint(TilePos::new(0, 6), 5).unwrap();
    assert!(
        f < 1.0 && wash == 0.0,
        "level 1 under a floor at 3 is dimmed"
    );
}

#[test]
fn a_lip_draws_inside_its_cell_on_every_side_at_every_size() {
    for size in 2..=24 {
        let size = size as f32;
        for (start, span) in [(0.0, LIP), (1.0 - LIP, LIP), (0.0, 1.0)] {
            let (offset, length) = span_in_cell(size, start, span);
            assert!(length >= 1.0, "size {size}, start {start}");
            assert!(
                offset >= 0.0 && offset + length <= size,
                "size {size}, start {start}: {offset} + {length}"
            );
        }
        // Opposite lips match, so no side of a cliff reads heavier.
        let near = span_in_cell(size, 0.0, LIP);
        let far = span_in_cell(size, 1.0 - LIP, LIP);
        assert_eq!(near.1, far.1, "size {size}");
        assert_eq!(
            far.0 + far.1,
            size,
            "size {size}: the far lip meets the edge"
        );
        // A fill covers its whole cell.
        assert_eq!(span_in_cell(size, 0.0, 1.0), (0.0, size));
    }
}
