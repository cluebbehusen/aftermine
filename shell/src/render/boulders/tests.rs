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
fn a_slab_stands_by_its_neighbors_alone() {
    // Clipping the candidate set to any window that keeps a slab's reach
    // leaves that slab's fate unchanged, so panning never reshuffles rock.
    let slabs = field_of(14);
    let kept = standing(&slabs);
    let window: Vec<Slab> = slabs
        .iter()
        .copied()
        .filter(|slab| slab.x < 9.0 && slab.y < 9.0)
        .collect();
    let windowed = standing(&window);
    for slab in &window {
        if slab.x < 9.0 - REACH as f32 && slab.y < 9.0 - REACH as f32 {
            assert_eq!(kept.contains(slab), windowed.contains(slab));
        }
    }
}
