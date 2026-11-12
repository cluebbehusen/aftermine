use super::*;

/// A grid from rows where `#` is closed.
fn grid(rows: &[&str]) -> Grid {
    let height = i32::try_from(rows.len()).unwrap();
    let width = i32::try_from(rows[0].len()).unwrap();
    Grid::new(width, height, |t| {
        rows[as_index(t.y)].as_bytes()[as_index(t.x)] != b'#'
    })
}

#[test]
fn a_diagonal_step_needs_both_corners_open() {
    let cut = grid(&["..", "#."]);
    let from = TilePos::new(0, 0);
    let steps: Vec<_> = cut.steps(from).collect();
    assert_eq!(steps, [(TilePos::new(1, 0), 10)]);
    let open = grid(&["..", ".."]);
    assert!(
        open.steps(from)
            .any(|step| step == (TilePos::new(1, 1), 14))
    );
}

#[test]
fn distances_count_tenths_and_leave_the_unreached_at_max() {
    let map = grid(&["...#.", "...#."]);
    let dist = map.distances(&[TilePos::new(0, 0)]);
    assert_eq!(dist[map.index(TilePos::new(2, 0))], 20);
    assert_eq!(dist[map.index(TilePos::new(2, 1))], 24);
    assert_eq!(dist[map.index(TilePos::new(4, 0))], u32::MAX);
}

#[test]
fn a_flood_marks_one_region_and_a_ring_skips_closed_ground() {
    let map = grid(&["..#..", "..#..", "....."]);
    let mut marks = vec![usize::MAX; map.cells()];
    let left = map.flood(&[TilePos::new(0, 0)], &mut marks, 3);
    assert_eq!(left.len(), 13, "the bottom row joins both sides");
    assert!(marks.iter().all(|&mark| mark == 3 || mark == usize::MAX));
    let ring = map.ring(TilePos::new(0, 0), (2, 2));
    assert_eq!(
        ring,
        [TilePos::new(0, 2), TilePos::new(1, 2), TilePos::new(2, 2)]
    );
}
