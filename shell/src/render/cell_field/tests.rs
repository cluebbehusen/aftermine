use super::*;

fn grid(rows: &[&str]) -> CellField {
    let height = i32::try_from(rows.len()).expect("small grid");
    let width = i32::try_from(rows[0].len()).expect("small grid");
    CellField::seeded(TilePos::new(0, 0), width, height, 1, |pos| {
        rows[usize::try_from(pos.y).expect("in grid")].as_bytes()
            [usize::try_from(pos.x).expect("in grid")]
            == b'#'
    })
}

#[test]
fn depth_is_the_chessboard_distance_to_the_nearest_outside_tile() {
    let field = grid(&[".......", ".#####.", ".#####.", ".#####.", "......."]);
    assert_eq!(field.at(TilePos::new(0, 0), 0, 0), 0);
    assert_eq!(field.at(TilePos::new(1, 1), 0, 0), 1);
    assert_eq!(field.at(TilePos::new(3, 2), 0, 0), 2);
    assert_eq!(field.at(TilePos::new(5, 3), 0, 0), 1);
}

#[test]
fn cells_beyond_the_window_count_as_deep_inside() {
    let field = grid(&["###", "###", "###"]);
    // No outside cell in the window: the edge never reads as an edge.
    assert!(field.depth.iter().all(|&depth| depth == u8::MAX));
    assert_eq!(field.cell(-1, 0), u8::MAX);
}

#[test]
fn sub_tile_cells_measure_in_cells() {
    let field = CellField::seeded(TilePos::new(0, 0), 3, 1, CELLS, |pos| pos.x == 1);
    let depths: Vec<u8> = (0..CELLS)
        .map(|sx| field.at(TilePos::new(1, 0), sx, 0))
        .collect();
    // Above and below the strip lies the window's unknown edge, so depth
    // grows only from the open tiles beside it.
    assert_eq!(depths, [1, 2, 2, 1]);
}

#[test]
fn a_mask_field_shares_the_window() {
    let field = grid(&["#####", "#####"]);
    let mask = field.masked(|index| index % 5 != 0);
    assert_eq!((mask.width, mask.height, mask.cells), (5, 2, 1));
    assert_eq!(mask.at(TilePos::new(0, 0), 0, 0), 0);
    assert_eq!(mask.at(TilePos::new(1, 0), 0, 0), 1);
    assert_eq!(mask.at(TilePos::new(3, 1), 0, 0), 3);
}
