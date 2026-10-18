use super::*;
use crate::numeric::Fit;

/// Shapes of `#` on these rows, clear of nothing but the map itself.
fn shapes(rows: &[&str]) -> Shapes {
    let (width, height) = (rows[0].len().fit::<i32>(), rows.len().fit::<i32>());
    let at = |pos: TilePos| {
        pos.x >= 0
            && pos.y >= 0
            && pos.x < width
            && pos.y < height
            && rows[as_index(pos.y)].as_bytes()[as_index(pos.x)] == b'#'
    };
    let inside = |pos: TilePos| pos.x >= 0 && pos.y >= 0 && pos.x < width && pos.y < height;
    Shapes::find(width, height, at, inside, 7)
}

#[test]
fn a_shape_touching_the_edge_is_left_out() {
    let found = shapes(&["#.......", "........", "...##...", "...##...", "........"]);
    assert!(!found.owns(TilePos::new(0, 0)), "an edge shape was kept");
    assert!(found.owns(TilePos::new(3, 2)));
    assert!(found.owns(TilePos::new(4, 3)));
    assert!(!found.owns(TilePos::new(5, 2)));
}

#[test]
fn half_turn_mirrors_share_their_places() {
    let rows = [
        "............",
        ".####.......",
        ".######.....",
        "............",
        ".....######.",
        ".......####.",
        "............",
    ];
    let found = shapes(&rows);
    let (width, height) = (12 * CELLS, 7 * CELLS);
    for cy in 0..height {
        for cx in 0..width {
            assert_eq!(
                found.place(cx, cy),
                found.place(width - 1 - cx, height - 1 - cy),
                "cell {cx},{cy} differs from its mirror"
            );
        }
    }
}

#[test]
fn a_shape_that_is_its_own_mirror_matches_itself() {
    let rows = ["..........", "..######..", "..######..", ".........."];
    let found = shapes(&rows);
    let (width, height) = (10 * CELLS, 4 * CELLS);
    for cy in 0..height {
        for cx in 0..width {
            assert_eq!(
                found.place(cx, cy),
                found.place(width - 1 - cx, height - 1 - cy)
            );
        }
    }
}

#[test]
fn a_corner_stretch_wraps_both_its_sides() {
    let rows = [
        "............",
        "..########..",
        "..########..",
        "..########..",
        "..########..",
        "..########..",
        "..########..",
        "..########..",
        "..########..",
        "............",
    ];
    let found = shapes(&rows);
    let (x0, y0) = (2 * CELLS, CELLS);
    let (x1, y1) = (10 * CELLS - 1, 9 * CELLS - 1);
    let stretch = |cx: i32, cy: i32| found.place(cx, cy).map(|place| place.stretch);
    for a in 0..STRETCH {
        for b in 0..a {
            assert_eq!(stretch(x0 + a, y0 + b), stretch(x0 + b, y0 + a));
            assert_eq!(stretch(x1 - a, y0 + b), stretch(x1 - b, y0 + a));
            assert_eq!(stretch(x1 - a, y1 - b), stretch(x1 - b, y1 - a));
            assert_eq!(stretch(x0 + a, y1 - b), stretch(x0 + b, y1 - a));
        }
    }
}

#[test]
fn a_thin_shape_varies_only_along_its_length() {
    let rows = [
        "..............................",
        ".########################.....",
        ".########################.....",
        "..............................",
    ];
    let found = shapes(&rows);
    let mut lengths = std::collections::BTreeSet::new();
    for cx in CELLS..25 * CELLS {
        let top = found.place(cx, CELLS).expect("thin shape cell");
        let bottom = found.place(cx, 3 * CELLS - 1).expect("thin shape cell");
        assert!(top.thin);
        assert_eq!(top.length, bottom.length, "split across at {cx}");
        lengths.insert(top.length);
    }
    assert!(lengths.len() > 1, "a long thin shape never changes stretch");
}
