use super::*;
use crate::render::stretches::STRETCH;

/// Authored rows: `#` rock, `^` peak.
fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_string()).collect()
}

/// The shell's terraced field over these rows.
fn terraced(rows: &[String]) -> Terraces {
    let (map, _) = Map::parse(rows).expect("rows parse");
    Terraces::build(&map, rows)
}

fn levels(terraces: &Terraces, pos: TilePos) -> std::collections::BTreeSet<usize> {
    (0..CELLS)
        .flat_map(|sy| (0..CELLS).map(move |sx| (sx, sy)))
        .filter_map(
            |(sx, sy)| match terraces.material(pos.x * CELLS + sx, pos.y * CELLS + sy) {
                Some(Material::Riser(level) | Material::Bench(level)) => Some(level),
                _ => None,
            },
        )
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
fn floor_stays_bare() {
    let map = rows(&["######", "#....#", "#....#", "######"]);
    let terraces = terraced(&map);
    for y in 1..3 {
        for x in 1..5 {
            assert!(levels(&terraces, TilePos::new(x, y)).is_empty());
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
    let terraces = terraced(&map);
    let column = 9 * CELLS + 2;
    let ring_edge = terraces.material(column, CELLS - 1);
    let spur_edge = terraces.material(column, CELLS);
    assert_eq!(ring_edge, spur_edge, "a seam at the map edge");
    assert!(
        matches!(spur_edge, Some(Material::Bench(level) | Material::Riser(level)) if level >= 1),
        "the spur's top is not one of the wall's upper levels: {spur_edge:?}"
    );
    // Its foot steps down to the floor through the first terrace.
    assert_eq!(
        terraces.material(column, 6 * CELLS - 1),
        Some(Material::Riser(0))
    );
}

#[test]
fn thin_peaks_stand_at_two_heights_and_broad_ones_step_up() {
    let thin = rows(&[
        "..............................",
        ".^^^^^^^^^^^^^^^^^^^^^^^^.....",
        ".^^^^^^^^^^^^^^^^^^^^^^^^.....",
        "..............................",
    ]);
    let terraces = terraced(&thin);
    let heights: std::collections::BTreeSet<usize> = (1..25)
        .flat_map(|x| levels(&terraces, TilePos::new(x, 1)))
        .collect();
    assert_eq!(
        heights,
        [0, 1].into(),
        "a thin peak's stretches stand at two heights"
    );
    // Each stretch stands at one height across the whole thickness.
    for cx in CELLS..25 * CELLS {
        let height = |cy: i32| terraces.peaks.steps(cx, cy).map(|steps| steps.lift);
        assert_eq!(height(CELLS), height(3 * CELLS - 1), "split across at {cx}");
    }
    let six = rows(&[
        "............",
        "...^^^^^^...",
        "...^^^^^^...",
        "...^^^^^^...",
        "...^^^^^^...",
        "...^^^^^^...",
        "...^^^^^^...",
        "............",
    ]);
    let terraces = terraced(&six);
    assert!(
        levels(&terraces, TilePos::new(5, 4))
            .iter()
            .any(|level| *level >= 1)
    );
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
        ".^^.........",
        ".^^.........",
        ".^^.........",
        ".^^.........",
        "............",
    ]);
    let terraces = terraced(&map);
    assert!(
        levels(&terraces, TilePos::new(5, 3))
            .iter()
            .any(|level| *level >= 1)
    );
    assert_eq!(levels(&terraces, TilePos::new(1, 9)), [0].into());
}

#[test]
fn mirrored_peaks_draw_as_mirror_images() {
    let map = rows(&[
        "..................",
        "..^^^^^^..........",
        "..^^^^^^^^........",
        "..^^^^^^^^........",
        "..^^^^^^..........",
        "..................",
        "..........^^^^^^..",
        "........^^^^^^^^..",
        "........^^^^^^^^..",
        "..........^^^^^^..",
        "..................",
    ]);
    // The second peak is the first turned half a turn.
    let (width, height) = (18 * CELLS, 11 * CELLS);
    let terraces = terraced(&map);
    for cy in 0..height {
        for cx in 0..width {
            assert_eq!(
                terraces.material(cx, cy),
                terraces.material(width - 1 - cx, height - 1 - cy),
                "cell {cx},{cy} differs from its mirror"
            );
        }
    }
}

#[test]
fn a_corner_steps_alike_on_both_its_sides() {
    let map = rows(&[
        "............",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "..^^^^^^^^..",
        "............",
    ]);
    let terraces = terraced(&map);
    let (x0, y0) = (2 * CELLS, CELLS);
    let (x1, y1) = (10 * CELLS - 1, 9 * CELLS - 1);
    // Within a corner's stretch, a cell and its reflection across the
    // corner's diagonal share the first two benches, so each meets itself
    // square.
    for a in 0..STRETCH {
        for b in 0..a {
            for (here, there) in [
                ((x0 + a, y0 + b), (x0 + b, y0 + a)),
                ((x1 - a, y0 + b), (x1 - b, y0 + a)),
                ((x1 - a, y1 - b), (x1 - b, y1 - a)),
                ((x0 + a, y1 - b), (x0 + b, y1 - a)),
            ] {
                let benches = |(cx, cy): (i32, i32)| {
                    terraces
                        .peaks
                        .steps(cx, cy)
                        .map(|steps| (steps.widths[0], steps.widths[1]))
                };
                assert_eq!(
                    benches(here),
                    benches(there),
                    "a seam on the diagonal near {here:?}"
                );
            }
        }
    }
}

#[test]
fn a_long_side_varies_its_steps_and_spurs_keep_the_walls() {
    let map = rows(&[
        "##########################",
        "#........................#",
        "#..^^^^^^^^^^^^^^^^^^^^..#",
        "#..^^^^^^^^^^^^^^^^^^^^..#",
        "#..^^^^^^^^^^^^^^^^^^^^..#",
        "#..^^^^^^^^^^^^^^^^^^^^..#",
        "#..^^^^^^^^^^^^^^^^^^^^..#",
        "#........................#",
        "#.....^^^^...............#",
        "######^^^^################",
    ]);
    let terraces = terraced(&map);
    let firsts: std::collections::BTreeSet<i32> = (3 * CELLS..23 * CELLS)
        .filter_map(|cx| terraces.peaks.steps(cx, 2 * CELLS))
        .map(|steps| steps.widths[0])
        .collect();
    assert!(firsts.len() > 1, "the top side never varies: {firsts:?}");
    assert!(
        terraces.peaks.steps(7 * CELLS, 8 * CELLS).is_none(),
        "a spur took its own steps"
    );
}

#[test]
fn a_shadow_lies_one_cell_along_the_lower_and_right_sides() {
    // A wall over cells (0, 0) to (3, 3).
    let material = |cx: i32, cy: i32| {
        ((0..4).contains(&cx) && (0..4).contains(&cy)).then_some(Material::Bench(0))
    };
    assert!(
        in_shadow(material, 4, 0),
        "flush with the upper right corner"
    );
    assert!(
        in_shadow(material, 0, 4),
        "flush with the lower left corner"
    );
    assert!(in_shadow(material, 4, 4), "around the corner it turns");
    assert!(!in_shadow(material, 5, 2), "a single cell wide");
    assert!(!in_shadow(material, -1, 2), "nothing on the lit side");
    assert!(!in_shadow(material, 2, -1), "nothing on the lit side");
    assert!(!in_shadow(material, 2, 2), "nothing on the wall itself");
}

#[test]
fn a_lit_face_stays_darker_than_its_bench() {
    for layer in LAYERS {
        let (lit, shaded) = (riser_tone(layer, true), riser_tone(layer, false));
        assert!(shaded.r < layer.riser.r && layer.riser.r < lit.r && lit.r < layer.top.r);
    }
}

#[test]
fn height_rank_climbs_each_level_from_riser_to_bench() {
    let ladder = [
        None,
        Some(Material::Riser(0)),
        Some(Material::Bench(0)),
        Some(Material::Riser(1)),
        Some(Material::Bench(1)),
        Some(Material::Riser(2)),
        Some(Material::Bench(2)),
        Some(Material::Void),
    ];
    assert!(
        ladder
            .windows(2)
            .all(|pair| height_rank(pair[0]) < height_rank(pair[1]))
    );
    assert_eq!(
        height_rank(Some(Material::Vignette(1))),
        height_rank(Some(Material::Void))
    );
}
