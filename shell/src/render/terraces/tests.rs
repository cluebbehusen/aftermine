use super::*;

/// Authored rows: `#` edge ring, `^` peak, `?` unexplored ground.
fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_string()).collect()
}

fn terrain_at(rows: &[String], pos: TilePos) -> Option<(Terrain, bool)> {
    let row = rows.get(usize::try_from(pos.y).ok()?)?;
    let cell = *row.as_bytes().get(usize::try_from(pos.x).ok()?)?;
    Some(match cell {
        b'#' => (Terrain::Rock, true),
        b'^' => (Terrain::Peak, true),
        b'?' => (Terrain::Ground, false),
        _ => (Terrain::Ground, true),
    })
}

/// A field over the rows plus a margin, as the shell builds one.
fn terraced(rows: &[String], rule: fn(bool, Option<Terrain>, bool) -> bool) -> (Frame, CellField) {
    let frame = Frame::from_rows(rows);
    let width = rows[0].len().fit::<i32>();
    let height = rows.len().fit::<i32>();
    let mut field = CellField::seeded(
        TilePos::new(-MARGIN, -MARGIN),
        width + 2 * MARGIN,
        height + 2 * MARGIN,
        CELLS,
        |pos| {
            let tile = terrain_at(rows, pos);
            rule(
                frame.contains(pos),
                tile.map(|(terrain, _)| terrain),
                tile.is_none_or(|(_, explored)| explored),
            )
        },
    );
    terrace(&mut field, &frame);
    (frame, field)
}

fn material_of(frame: &Frame, field: &CellField, cx: i32, cy: i32) -> Option<Material> {
    let pos = TilePos::new(cx.div_euclid(CELLS), cy.div_euclid(CELLS));
    material(
        field.at(pos, cx.rem_euclid(CELLS), cy.rem_euclid(CELLS)),
        frame.widths(cx, cy),
        !frame.contains(pos),
    )
}

fn levels(frame: &Frame, field: &CellField, pos: TilePos) -> std::collections::BTreeSet<usize> {
    (0..CELLS)
        .flat_map(|sy| (0..CELLS).map(move |sx| (sx, sy)))
        .filter_map(|(sx, sy)| {
            match material_of(frame, field, pos.x * CELLS + sx, pos.y * CELLS + sy) {
                Some(Material::Riser(level) | Material::Bench(level)) => Some(level),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn the_playfield_excludes_only_fully_blocked_edges() {
    let closed = Frame::from_rows(&rows(&["#####", "#...#", "#...#", "#####"]));
    assert_eq!(
        (closed.x0, closed.y0, closed.width, closed.height),
        (1, 1, 3, 2)
    );
    let open = Frame::from_rows(&rows(&["##.##", "#...#", "....#", "##.##"]));
    assert_eq!((open.x0, open.y0, open.width, open.height), (0, 0, 4, 4));
}

#[test]
fn off_the_playfield_depth_walks_three_terraces_into_the_dark() {
    let widths = [4, 5, 3];
    let mut seen = Vec::new();
    for depth in 0..=30 {
        let m = material(depth, widths, true);
        if seen.last() != Some(&m) {
            seen.push(m);
        }
    }
    assert_eq!(
        seen,
        [
            None,
            Some(Material::Riser(0)),
            Some(Material::Bench(0)),
            Some(Material::Riser(1)),
            Some(Material::Bench(1)),
            Some(Material::Riser(2)),
            Some(Material::Bench(2)),
            Some(Material::Vignette(1)),
            Some(Material::Vignette(2)),
            Some(Material::Vignette(3)),
            Some(Material::Vignette(4)),
            Some(Material::Void),
        ]
    );
    // On the playfield the last bench is a plateau, never a hole.
    assert_eq!(material(30, widths, false), Some(Material::Bench(2)));
}

#[test]
fn each_level_rises_lighter() {
    for pair in LAYERS.windows(2) {
        assert!(pair[1].top.r > pair[0].top.r);
        assert!(pair[1].lip.r > pair[0].lip.r);
    }
    for layer in LAYERS {
        assert!(layer.bench_cells >= 3);
        assert!(layer.riser.r < layer.top.r && layer.top.r < layer.lip.r);
    }
}

#[test]
fn benches_jog_by_side_and_playfield_cells_take_their_nearest_side() {
    let frame = Frame::from_rows(&rows(&[
        "##########",
        "#........#",
        "#........#",
        "#........#",
        "##########",
    ]));
    for cx in -40..80 {
        for cy in -40..60 {
            assert!(frame.widths(cx, cy).iter().all(|w| (3..=5).contains(w)));
        }
    }
    // A cell just inside the top edge jogs with the top side above it.
    assert_eq!(
        frame.segment(2 * CELLS, CELLS),
        frame.segment(2 * CELLS, CELLS - 1)
    );
    let distinct: std::collections::BTreeSet<_> = frame.jogs.iter().collect();
    assert!(distinct.len() > 1, "the boundary never jogs");
}

#[test]
fn the_wall_ignores_fog_and_a_peak_continues_into_it() {
    assert!(wall(false, None, false));
    assert!(wall(true, Some(Terrain::Peak), true));
    assert!(
        !wall(true, Some(Terrain::Peak), false),
        "an unseen spur stays floor"
    );
    assert!(!wall(true, Some(Terrain::Ground), true));
    assert!(standing(false, None, true));
    assert!(
        standing(true, Some(Terrain::Ground), false),
        "fog never shows an edge"
    );
    assert!(standing(true, Some(Terrain::Peak), true));
    assert!(!standing(true, Some(Terrain::Rock), true));
}

#[test]
fn floor_stays_bare() {
    let map = rows(&["######", "#....#", "#....#", "######"]);
    let (frame, field) = terraced(&map, wall);
    for y in 1..3 {
        for x in 1..5 {
            assert!(levels(&frame, &field, TilePos::new(x, y)).is_empty());
        }
    }
}

#[test]
fn a_spur_joins_every_level_of_the_wall() {
    // A wide spur from the top edge: walking down its middle from the ring,
    // the levels change only where its own terraces step, never at the
    // edge line between wall and spur.
    let map = rows(&[
        "####################",
        "#......^^^^^^......#",
        "#......^^^^^^......#",
        "#......^^^^^^......#",
        "#......^^^^^^......#",
        "#......^^^^^^......#",
        "#..................#",
        "#..................#",
        "####################",
    ]);
    let (frame, field) = terraced(&map, wall);
    let column = 9 * CELLS + 2;
    let ring_edge = material_of(&frame, &field, column, CELLS - 1);
    let spur_edge = material_of(&frame, &field, column, CELLS);
    assert_eq!(ring_edge, spur_edge, "a seam at the map edge");
    assert!(
        matches!(spur_edge, Some(Material::Bench(level) | Material::Riser(level)) if level >= 1),
        "the spur's top is not one of the wall's upper levels: {spur_edge:?}"
    );
    // Its foot steps down to the floor through the first terrace.
    assert_eq!(
        material_of(&frame, &field, column, 6 * CELLS - 1),
        Some(Material::Riser(0))
    );
}

#[test]
fn freestanding_peaks_step_up_from_about_five_tiles_thick() {
    let four = rows(&[
        "..........",
        "...^^^^...",
        "...^^^^...",
        "...^^^^...",
        "...^^^^...",
        "..........",
    ]);
    let (frame, field) = terraced(&four, standing);
    assert_eq!(levels(&frame, &field, TilePos::new(4, 2)), [0].into());
    let five = rows(&[
        "...........",
        "...^^^^^...",
        "...^^^^^...",
        "...^^^^^...",
        "...^^^^^...",
        "...^^^^^...",
        "...........",
    ]);
    let (frame, field) = terraced(&five, standing);
    assert!(levels(&frame, &field, TilePos::new(5, 3)).contains(&1));
}

#[test]
fn upper_levels_never_trace_a_thin_arm() {
    let map = rows(&[
        "............",
        ".^^^^^^^^^^.",
        ".^^^^^^^^^^.",
        ".^^^^^^^^^^.",
        ".^^^^^^^^^^.",
        ".^^^^^^^^^^.",
        ".^^^^^^^^^^.",
        ".^^^^.......",
        ".^^^^.......",
        ".^^^^.......",
        ".^^^^.......",
        "............",
    ]);
    let (frame, field) = terraced(&map, standing);
    assert!(levels(&frame, &field, TilePos::new(5, 3)).contains(&1));
    assert_eq!(levels(&frame, &field, TilePos::new(2, 9)), [0].into());
}

#[test]
fn a_peak_running_into_fog_shows_no_edge_there() {
    let map = rows(&["........", ".^^^??..", ".^^^??..", ".^^^??..", "........"]);
    let (frame, field) = terraced(&map, standing);
    for sy in 0..CELLS {
        assert_ne!(
            material_of(&frame, &field, 3 * CELLS + CELLS - 1, 2 * CELLS + sy),
            Some(Material::Riser(0))
        );
    }
}

#[test]
fn peaks_are_eight_connected_playfield_regions() {
    let map_rows = rows(&["#######", "#^^...#", "#..^..#", "#....^#", "#######"]);
    let mut scenario = oxide_sim::Scenario::skirmish();
    scenario.map = map_rows.clone();
    let (map, _) = Map::parse(&scenario.map).expect("rows parse");
    let (found, owner) = peaks(&map, &Frame::from_rows(&map_rows));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].tiles.len(), 3);
    assert_eq!(owner[as_index(7 + 1)], Some(0));
    assert_eq!(owner[as_index(3 * 7 + 5)], Some(1));
}

#[test]
fn the_cache_key_tells_exploration_apart_not_just_its_size() {
    let a = explored_print([true, false, false, true].into_iter());
    let b = explored_print([false, true, true, false].into_iter());
    assert_ne!(a, b, "two masks with the same count must not share a field");
    assert_eq!(a, explored_print([true, false, false, true].into_iter()));
}
