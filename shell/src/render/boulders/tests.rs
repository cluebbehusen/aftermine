use super::*;

#[test]
fn slab_radii_match_the_generated_library() {
    let source = include_str!("../../../../tools/production_sprite_sources/quarry_rocks.py");
    let line = source
        .lines()
        .find(|line| line.starts_with("SLAB_RADII = ("))
        .expect("the generator names its slab radii");
    let radii: Vec<f32> = line
        .trim_start_matches("SLAB_RADII = (")
        .trim_end_matches(')')
        .split(',')
        .map(|value| value.trim().parse().expect("a radius"))
        .collect();
    assert_eq!(radii, SLAB_RADII);
}

#[test]
fn size_classes_pick_the_nearest_radius() {
    assert_eq!(size_class(0.1), 0);
    assert_eq!(size_class(0.33), 0);
    assert_eq!(size_class(0.5), 1);
    assert_eq!(size_class(0.95), 4);
    assert_eq!(size_class(3.0), ROCK_SLAB_SIZES - 1);
}

#[test]
fn thick_rock_offers_one_larger_slab_and_edges_two_small_ones() {
    let edge: Vec<_> = candidates(TilePos::new(4, 4), 1).collect();
    let deep: Vec<_> = candidates(TilePos::new(4, 4), 4).collect();
    assert_eq!(edge.len(), 2);
    assert_eq!(deep.len(), 1);
    assert!(deep[0].radius > edge.iter().map(|slab| slab.radius).fold(0.0, f32::max));
    // Growth stops at the thickness cap.
    assert_eq!(
        candidates(TilePos::new(4, 4), 9).next(),
        candidates(TilePos::new(4, 4), 4).next()
    );
    for slab in edge.iter().chain(&deep) {
        assert!((4.0..5.0).contains(&slab.x) && (4.0..5.0).contains(&slab.y));
    }
}

#[test]
fn a_slab_must_stand_on_rock() {
    let slab = candidates(TilePos::new(2, 2), 3).next().expect("a slab");
    assert!(fits(&slab, |_| true));
    assert!(!fits(&slab, |pos| pos == TilePos::new(2, 2)));
}

fn field_of(size: i32) -> Vec<Slab> {
    (0..size)
        .flat_map(|y| (0..size).map(move |x| TilePos::new(x, y)))
        .flat_map(|pos| {
            let depth = (pos.x + 1)
                .min(pos.y + 1)
                .min(size - pos.x)
                .min(size - pos.y);
            candidates(pos, u8::try_from(depth).expect("small"))
        })
        .collect()
}

#[test]
fn standing_slabs_never_overlap_and_ignore_input_order() {
    let slabs = field_of(9);
    let kept = standing(&slabs);
    assert!(!kept.is_empty() && kept.len() < slabs.len());
    for (i, a) in kept.iter().enumerate() {
        for b in &kept[i + 1..] {
            assert!((a.x - b.x).hypot(a.y - b.y) >= SPACING * (a.radius + b.radius));
        }
    }
    let mut reversed = slabs.clone();
    reversed.reverse();
    let mut again = standing(&reversed);
    again.reverse();
    assert_eq!(kept, again);
}

#[test]
fn an_edge_slab_fits_on_its_tile_alone() {
    for y in -20..20 {
        for x in -20..20 {
            let pos = TilePos::new(x, y);
            for slab in candidates(pos, 1) {
                assert!(fits(&slab, |tile| tile == pos), "{pos:?} {slab:?}");
            }
        }
    }
}

#[test]
fn every_rock_tile_gets_a_slab_reaching_its_center() {
    let tiles: Vec<TilePos> = (0..12).map(|x| TilePos::new(x, 3)).collect();
    // One far-reaching slab and nothing else: every tile it misses gets a
    // fallback.
    let lone = Slab {
        x: 2.5,
        y: 3.5,
        radius: 1.0,
        rank: (4, 0),
    };
    let slabs = with_fallbacks(vec![lone], &tiles);
    for pos in &tiles {
        let (cx, cy) = (pos.x as f32 + 0.5, pos.y as f32 + 0.5);
        assert!(
            slabs
                .iter()
                .any(|slab| (slab.x - cx).hypot(slab.y - cy) <= slab.radius),
            "{pos:?} reads as open floor"
        );
    }
    assert_eq!(slabs.len(), 1 + tiles.len() - 3);
}

#[test]
fn no_rock_tile_on_a_shipped_map_reads_as_open_floor() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios");
    let mut paths: Vec<_> = std::fs::read_dir(&root)
        .expect("scenario directory")
        .map(|entry| entry.expect("scenario entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    for path in paths {
        let scenario = aftermine_sim::Scenario::load(&path).expect("shipped map loads");
        let rows = &scenario.map;
        let (map, _) = scenario
            .parse_map_and_anchors()
            .expect("shipped rows parse");
        let ring = boundary_ring(rows);
        let blockers = Blockers::build(&map, rows);
        let Slabs(slabs) = Slabs::build(&map, rows, &blockers);
        for y in 0..map.height() {
            for x in 0..map.width() {
                let pos = TilePos::new(x, y);
                if map.tile(pos).map(|tile| tile.terrain) != Some(Terrain::Rock) || ring(pos) {
                    continue;
                }
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                if blockers.covers(pos) {
                    assert!(
                        !slabs
                            .iter()
                            .any(|slab| (tile_of(slab.x), tile_of(slab.y)) == (x, y)),
                        "{}: a slab stands on the blocker at {pos:?}",
                        path.display()
                    );
                } else {
                    assert!(
                        slabs
                            .iter()
                            .any(|slab| (slab.x - cx).hypot(slab.y - cy) <= slab.radius),
                        "{}: rock at {pos:?} reads as open floor",
                        path.display()
                    );
                }
            }
        }
    }
}
