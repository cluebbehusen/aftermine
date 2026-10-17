use super::*;

/// Every distinct outline a kind shows at any tier, built or at any
/// construction stage, with its kind.
fn every_outline() -> Vec<(BuildingKind, Outline)> {
    let mut all: Vec<(BuildingKind, Outline)> = Vec::new();
    for kind in BuildingKind::ALL {
        for tier in 0..u8::try_from(kind.tiers().len()).unwrap() {
            for stage in std::iter::once(None).chain((0..CONSTRUCTION_STAGES).map(Some)) {
                let shown = outline(kind, tier, stage);
                if !all.iter().any(|&(k, o)| k == kind && o == shown) {
                    all.push((kind, shown));
                }
            }
        }
    }
    all
}

/// Above this many vertices an outline costs every contact query more than
/// its art's detail is worth.
const MAX_VERTICES: usize = 24;

#[test]
fn every_kind_outlines_each_tier_built_and_at_each_stage() {
    for kind in BuildingKind::ALL {
        let outlines = outlines(kind);
        let tiers = kind.tiers().len();
        let built = if kind.assembles().is_some() { 0 } else { tiers };
        assert_eq!(outlines.built.len(), built, "{kind:?}");
        assert_eq!(outlines.site.len(), tiers, "{kind:?}");
    }
}

/// Each outline is a simple polygon on its canvas, wound clockwise on
/// screen, whose edges are long enough and turn sharply enough that each
/// stays a face, with few enough vertices to keep contact queries cheap.
#[test]
fn every_outline_is_a_clockwise_simple_polygon_on_its_canvas() {
    let cross = |a: (i64, i64), b: (i64, i64)| a.0 * b.1 - a.1 * b.0;
    let sub = |a: (i32, i32), b: (i32, i32)| (i64::from(a.0 - b.0), i64::from(a.1 - b.1));
    let length_sq = |v: (i64, i64)| v.0 * v.0 + v.1 * v.1;
    for (kind, outline) in every_outline() {
        let n = outline.len();
        assert!((3..=MAX_VERTICES).contains(&n), "{kind:?} {outline:?}");
        assert!(
            outline
                .iter()
                .all(|&(x, y)| (0..=128).contains(&x) && (0..=128).contains(&y)),
            "{kind:?} leaves its canvas: {outline:?}"
        );
        let edge = |i: usize| (outline[i], outline[(i + 1) % n]);
        let area: i64 = (0..n)
            .map(|i| {
                let (a, b) = edge(i);
                i64::from(a.0) * i64::from(b.1) - i64::from(b.0) * i64::from(a.1)
            })
            .sum();
        assert!(area > 0, "{kind:?} winds counterclockwise: {outline:?}");
        for i in 0..n {
            let (a, b) = edge(i);
            let (_, c) = edge((i + 1) % n);
            let (ab, bc) = (sub(b, a), sub(c, b));
            assert!(length_sq(ab) >= 9, "{kind:?} edge {a:?}-{b:?} is too short");
            // sin² 5° is about 0.0076: a shallower turn splits one face.
            let turn = cross(ab, bc);
            assert!(
                turn * turn * 10_000 > 76 * length_sq(ab) * length_sq(bc),
                "{kind:?} barely turns at {b:?}"
            );
        }
        // No two edges that share no vertex may meet.
        let side =
            |p: (i32, i32), q: (i32, i32), r: (i32, i32)| cross(sub(q, p), sub(r, p)).signum();
        let on = |p: (i32, i32), q: (i32, i32), r: (i32, i32)| {
            side(p, q, r) == 0
                && r.0 >= p.0.min(q.0)
                && r.0 <= p.0.max(q.0)
                && r.1 >= p.1.min(q.1)
                && r.1 <= p.1.max(q.1)
        };
        for i in 0..n {
            for j in i + 1..n {
                if j == i + 1 || (i == 0 && j == n - 1) {
                    continue;
                }
                let ((a, b), (c, d)) = (edge(i), edge(j));
                let proper = side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0;
                let touching = on(a, b, c) || on(a, b, d) || on(c, d, a) || on(c, d, b);
                assert!(!proper && !touching, "{kind:?} crosses itself: {outline:?}");
            }
        }
    }
}

/// An outline within a unit of mirror symmetry is exactly symmetric, so a
/// mirrored map's contacts mirror too; only art that is itself lopsided may
/// leave its outline so.
#[test]
fn outlines_near_mirror_symmetry_are_exactly_symmetric() {
    for (kind, outline) in every_outline() {
        let mirror = |&(x, y): &(i32, i32)| (128 - x, y);
        let partners = outline.iter().map(|vertex| {
            let (mx, my) = mirror(vertex);
            outline
                .iter()
                .map(|&(x, y)| (x - mx).abs().max((y - my).abs()))
                .min()
                .expect("nonempty outline")
        });
        let worst = partners.max().expect("nonempty outline");
        assert!(
            worst == 0 || worst > 1,
            "{kind:?} misses mirror symmetry by a unit: {outline:?}"
        );
    }
}

/// The pruned queries against a plain walk over every edge, around every
/// outline, from points inside, beside, and well clear of it.
#[test]
fn pruned_surface_queries_match_a_walk_over_every_edge() {
    let mut rng = chassis::rng::Pcg32::new(41, 7);
    let mut near = |anchor: i32, span: i32| {
        Fx::from_num(anchor - 3)
            + Fx::from_bits(
                i64::from(rng.next_u32() % ((u32::try_from(span).unwrap() + 6) << 10)) << 22,
            )
    };
    let (mut blocked, mut open) = (0, 0);
    for (kind, outline) in every_outline() {
        let size = kind.size();
        let surface = Surface {
            outline,
            anchor: TilePos::new(10, 20),
            size,
        };
        let center = Vec2Fx::new(Fx::from_num(10), Fx::from_num(20))
            + Vec2Fx::new(Fx::from_num(size.0), Fx::from_num(size.1)) / Fx::from_num(2);
        for _ in 0..400 {
            let from = Vec2Fx::new(near(10, size.0), near(20, size.1));
            let to =
                from + (Vec2Fx::new(near(10, size.0), near(20, size.1)) - from) / Fx::from_num(4);
            let radius = near(0, 0) / 8 + Fx::lit("0.2");
            let closest = surface
                .edges()
                .map(|(a, b)| closest_on_segment(from, a, b))
                .min_by_key(|p| (from.dist_sq(*p), cross(from - center, *p - center)))
                .expect("nonempty outline");
            assert_eq!(surface.closest(from), closest, "{kind:?} {from:?}");
            let clear = !surface.contains(from)
                && !surface.contains(to)
                && surface.edges().all(|(a, b)| {
                    !segments_cross(from, to, a, b)
                        && [
                            from.dist_sq(closest_on_segment(from, a, b)),
                            to.dist_sq(closest_on_segment(to, a, b)),
                            a.dist_sq(closest_on_segment(a, from, to)),
                            b.dist_sq(closest_on_segment(b, from, to)),
                        ]
                        .into_iter()
                        .all(|d| d >= radius * radius)
                });
            assert_eq!(
                surface.clear(from, to, radius),
                clear,
                "{kind:?} {from:?} {to:?}"
            );
            blocked += usize::from(!clear);
            open += usize::from(clear);
        }
    }
    assert!(
        blocked > 200 && open > 200,
        "{blocked} blocked, {open} open"
    );
}

/// Every face's square stance stands outside its outline, out from that
/// face's midpoint: the outward turn agrees with each outline's winding.
#[test]
fn square_stances_stand_out_from_each_face_midpoint() {
    let clearance = Fx::lit("0.015625");
    for (kind, outline) in every_outline() {
        let surface = Surface {
            outline,
            anchor: TilePos::new(10, 20),
            size: kind.size(),
        };
        for ((a, b), (stance, outward)) in surface.edges().zip(surface.square_stances(clearance)) {
            let midpoint = (a + b) / Fx::from_num(2);
            assert!(!surface.contains(stance), "{kind:?} {a:?} {b:?}");
            assert!(
                surface.closest(stance).dist_sq(midpoint) < Fx::lit("0.000001"),
                "{kind:?} {a:?} {b:?}"
            );
            assert!(
                (stance - midpoint - outward * clearance).length() < Fx::lit("0.001"),
                "{kind:?} {a:?} {b:?}"
            );
        }
    }
}
