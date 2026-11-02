use super::*;

const BASE: [i32; 3] = [2, 2, 2];

#[test]
fn depth_walks_lip_bench_vignette_void_in_order() {
    let mut seen = Vec::new();
    for depth in 1..=(MAX_DEPTH.fit::<u8>() + 1) {
        let material = material(depth, BASE).expect("pit cell");
        if seen.last() != Some(&material) {
            seen.push(material);
        }
    }
    assert_eq!(
        seen,
        [
            Material::Riser(0),
            Material::Bench(0),
            Material::Riser(1),
            Material::Bench(1),
            Material::Riser(2),
            Material::Bench(2),
            Material::Vignette(1),
            Material::Vignette(2),
            Material::Vignette(3),
            Material::Vignette(4),
            Material::Void,
        ]
    );
    assert_eq!(material(0, BASE), None);
}

#[test]
fn each_level_descends_toward_the_void() {
    let mut previous: Option<Layer> = None;
    for layer in DROP {
        assert!(layer.riser.r < layer.top.r);
        assert!(layer.top.r < layer.lip.r);
        if let Some(above) = previous {
            assert!(layer.top.r < above.top.r);
            assert!(layer.lip.r < above.lip.r);
        }
        previous = Some(layer);
    }
    assert!(VOID.r < DROP[DROP.len() - 1].top.r);
}

#[test]
fn bench_widths_jog_within_bounds_and_never_exceed_the_padding() {
    let mut distinct = std::collections::BTreeSet::new();
    for by in -8..8 {
        for bx in -8..8 {
            let widths = bench_widths(bx, by);
            assert!(widths.iter().all(|w| (1..=MAX_BENCH_CELLS).contains(w)));
            let depth: i32 = widths.iter().map(|w| 1 + w).sum::<i32>() + VIGNETTE_CELLS;
            assert!(depth <= MAX_DEPTH);
            distinct.insert(widths);
        }
    }
    assert!(distinct.len() > 1, "the rim never jogs");
}

fn flat_field(cells: i32) -> PitField {
    PitField {
        field: CellField {
            origin: TilePos::new(0, 0),
            width: cells / CELLS,
            height: cells / CELLS,
            cells: CELLS,
            depth: vec![u8::MAX; as_index(cells * cells)],
        },
        pits: Shapes::find(0, 0, |_| false, |_| true, SALT),
    }
}

/// The pit field over authored rows: `~` pit, `.` ground.
fn pits(rows: &[&str]) -> PitField {
    let rows: Vec<String> = rows.iter().map(|row| (*row).to_string()).collect();
    let (map, _) = Map::parse(&rows).expect("rows parse");
    PitField::build(&map)
}

#[test]
fn chessboard_distance_is_exact_from_a_flat_field() {
    let mut field = flat_field(12);
    // A single standing cell in the middle; everything else is pit.
    field.field.depth[6 * 12 + 6] = 0;
    field.field.relax();
    for y in 0..12i32 {
        for x in 0..12i32 {
            let expected = (x - 6).abs().max((y - 6).abs()).fit::<u8>();
            assert_eq!(field.field.cell(x, y), expected, "cell {x},{y}");
        }
    }
}

#[test]
fn a_pit_clear_of_the_edge_steps_by_stretch_within_the_padding() {
    let field = pits(&[
        "..............................",
        "..~~~~~~~~~~~~~~~~~~~~~~~~~~..",
        "..~~~~~~~~~~~~~~~~~~~~~~~~~~..",
        "..~~~~~~~~~~~~~~~~~~~~~~~~~~..",
        "..~~~~~~~~~~~~~~~~~~~~~~~~~~..",
        "..~~~~~~~~~~~~~~~~~~~~~~~~~~..",
        "..............................",
    ]);
    let mut firsts = std::collections::BTreeSet::new();
    for cy in CELLS..6 * CELLS {
        for cx in 2 * CELLS..28 * CELLS {
            let widths = field.widths(cx, cy);
            assert!(widths.iter().all(|w| (1..=MAX_BENCH_CELLS).contains(w)));
            let depth: i32 = widths.iter().map(|w| 1 + w).sum::<i32>() + VIGNETTE_CELLS;
            assert!(depth <= MAX_DEPTH);
            firsts.insert(widths[0]);
        }
    }
    assert!(firsts.len() > 1, "the rim never varies");
}

#[test]
fn a_pit_running_off_the_map_keeps_its_block_jogs() {
    let field = pits(&["~~~.......", "~~~.......", "~~~.......", ".........."]);
    for cy in 0..3 * CELLS {
        for cx in 0..3 * CELLS {
            assert_eq!(
                field.widths(cx, cy),
                bench_widths(cx / CELLS / BLOCK_TILES, cy / CELLS / BLOCK_TILES)
            );
        }
    }
}

#[test]
fn mirrored_pits_drop_as_mirror_images() {
    let rows = [
        "..................",
        "..~~~~~~..........",
        "..~~~~~~~~........",
        "..~~~~~~~~........",
        "..~~~~~~..........",
        "..................",
        "..........~~~~~~..",
        "........~~~~~~~~..",
        "........~~~~~~~~..",
        "..........~~~~~~..",
        "..................",
    ];
    let field = pits(&rows);
    let (width, height) = (18, 11);
    for ty in 0..height {
        for tx in 0..width {
            for sy in 0..CELLS {
                for sx in 0..CELLS {
                    let here = field.material_at(TilePos::new(tx, ty), sx, sy);
                    let there = field.material_at(
                        TilePos::new(width - 1 - tx, height - 1 - ty),
                        CELLS - 1 - sx,
                        CELLS - 1 - sy,
                    );
                    assert_eq!(here, there, "tile {tx},{ty} cell {sx},{sy}");
                }
            }
        }
    }
}

#[test]
fn a_pit_face_takes_the_light_by_the_way_it_turns_and_the_rim_shades_inside() {
    let field = pits(&[
        ".........",
        ".~~~~~~~.",
        ".~~~~~~~.",
        ".~~~~~~~.",
        ".~~~~~~~.",
        ".~~~~~~~.",
        ".........",
    ]);
    let grey = Color::new(0.3, 0.3, 0.3, 1.0);
    let (mut toward, mut away, mut rim) = (0, 0, 0);
    for y in 1..6 {
        for x in 1..8 {
            let pos = TilePos::new(x, y);
            for sy in 0..CELLS {
                for sx in 0..CELLS {
                    let material = field.material_at(pos, sx, sy).unwrap_or(Material::Void);
                    let tone = lit(&field, pos, (sx, sy), material, grey, &DROP);
                    let depth = field.at(pos, sx, sy);
                    let shaded = (1..=RIM_SHADE_CELLS).any(|k| field.at(pos, sx - k, sy - k) == 0);
                    match material {
                        Material::Riser(level) if !shaded => {
                            let deeper_above = field.at(pos, sx, sy - 1) > depth;
                            let deeper_below = field.at(pos, sx, sy + 1) > depth;
                            if deeper_above {
                                assert_eq!(tone, mixed(grey, DROP[level].top, LIT_FACE));
                                toward += 1;
                            } else if deeper_below {
                                assert!(tone.r < grey.r, "a face from the light darkens");
                                away += 1;
                            }
                        }
                        Material::Bench(_) if shaded && depth > 0 => {
                            assert!(tone.r < grey.r, "the rim shades the step inside it");
                            rim += 1;
                        }
                        Material::Bench(_) if !shaded => assert_eq!(tone, grey),
                        _ => {}
                    }
                }
            }
        }
    }
    assert!(toward > 0 && away > 0 && rim > 0, "{toward} {away} {rim}");
}
