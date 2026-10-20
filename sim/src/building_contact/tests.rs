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
        for ((a, b), (stance, outward)) in surface.faces().zip(surface.square_stances(clearance)) {
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

#[test]
fn only_a_bevel_across_a_corner_is_no_face() {
    let faces = |kind: BuildingKind, outline: Outline| {
        let surface = Surface {
            outline,
            anchor: TilePos::new(10, 20),
            size: kind.size(),
        };
        (surface.edges().count(), surface.faces().count())
    };
    // Corner bevels cut between long sides: two on the Reclaimer and the
    // Turret, all four on the Citadel.
    assert_eq!(faces(BuildingKind::Reclaimer, RECLAIMER), (7, 5));
    assert_eq!(faces(BuildingKind::Turret, TURRET), (7, 5));
    assert_eq!(faces(BuildingKind::Bastion, BASTION_T1), (8, 4));
    // The Flak's legs are a run of short faces, each still a face.
    assert_eq!(faces(BuildingKind::FlakTurret, FLAK_TURRET), (17, 17));
}

/// `low` plus `step` times `(dx, dy)`, pushed a random part of a step
/// further along each axis so queries land between grid lines.
fn jittered(rng: &mut chassis::rng::Pcg32, low: Vec2Fx, step: Fx, dx: i32, dy: i32) -> Vec2Fx {
    let mut jitter = || Fx::from_bits(i64::from(rng.next_u32())) * step;
    low + Vec2Fx::new(
        Fx::from_num(dx) * step + jitter(),
        Fx::from_num(dy) * step + jitter(),
    )
}

/// [`State::in_building_work_reach`] rejects a body by the footprint's
/// closest point before consulting the surface, which is exact only while
/// every point the surface answers lies on the footprint.
#[test]
fn every_closest_surface_point_lies_on_the_footprint() {
    let mut rng = chassis::rng::Pcg32::new(43, 9);
    let anchor = TilePos::new(10, 20);
    let low = Vec2Fx::new(Fx::from_num(anchor.x), Fx::from_num(anchor.y));
    let eighth = Fx::lit("0.125");
    for (kind, outline) in every_outline() {
        let size = kind.size();
        let surface = Surface {
            outline,
            anchor,
            size,
        };
        let high = low + Vec2Fx::new(Fx::from_num(size.0), Fx::from_num(size.1));
        for dy in -24..size.1 * 8 + 24 {
            for dx in -24..size.0 * 8 + 24 {
                let from = jittered(&mut rng, low, eighth, dx, dy);
                let point = surface.closest(from);
                assert!(
                    (low.x..=high.x).contains(&point.x) && (low.y..=high.y).contains(&point.y),
                    "{kind:?} {from:?} meets {point:?} off its footprint"
                );
                let footprint =
                    Vec2Fx::new(from.x.clamp(low.x, high.x), from.y.clamp(low.y, high.y));
                assert!(
                    from.dist_sq(footprint) <= from.dist_sq(point),
                    "{kind:?} {from:?}"
                );
            }
        }
    }
}

fn work_reach(unit: &crate::Unit) -> Fx {
    unit.kind.stats().radius + crate::stats::WORK_FOOTPRINT_GAP + crate::stats::WORK_REACH
        - crate::stats::WORK_APPROACH_GAP
        + Fx::lit("0.04")
}

/// The reach test before its footprint reject: the surface alone.
fn surface_work_reach(state: &State, unit: &crate::Unit, building: crate::BuildingId) -> bool {
    let Some(b) = state.building(building) else {
        return false;
    };
    let reach = work_reach(unit);
    let surface = state.contact_surface(b);
    unit.pos.dist_sq(surface.closest(unit.pos)) <= reach * reach
        && surface.clear(unit.pos, unit.pos, unit.kind.stats().radius)
        && state
            .crew_pads_for(unit, b)
            .is_none_or(|pads| pads.contains(&unit.tile()))
}

#[test]
fn work_reach_matches_the_surface_alone() {
    use crate::state::BuildingPhase;
    let (map, _) = crate::map::Map::parse(&vec![".".repeat(24); 24]).unwrap();
    let seat = crate::state::Player {
        name: "p".into(),
        team: 0,
        scrap: 0,
        recovery: crate::state::Recovery::Ready,
        resigned: false,
        eliminated_at: None,
    };
    let mut state = State::assemble(map, vec![seat]);
    let anchor = TilePos::new(10, 10);
    let low = Vec2Fx::new(Fx::from_num(anchor.x), Fx::from_num(anchor.y));
    let site = state.place_building(crate::PlayerId(0), BuildingKind::Foundry, anchor);
    let mut hulls: Vec<crate::stats::UnitKind> = crate::stats::UnitKind::ALL.to_vec();
    hulls.sort_by_key(|kind| kind.stats().radius);
    hulls.dedup_by_key(|kind| kind.stats().radius);
    let workers: Vec<_> = hulls
        .iter()
        .map(|&kind| state.spawn_unit(crate::PlayerId(0), kind, anchor.center()))
        .collect();
    let mut rng = chassis::rng::Pcg32::new(47, 3);
    let quarter = Fx::lit("0.25");
    let (mut reached, mut missed, mut skipped) = (0, 0, 0);
    for kind in BuildingKind::ALL {
        for tier in 0..u8::try_from(kind.tiers().len()).unwrap() {
            let b = state.building_mut(site).unwrap();
            (b.kind, b.tier) = (kind, tier);
            let total = b.stats().construction.map_or(1, |c| c.build_ticks);
            let stages = (0..CONSTRUCTION_STAGES)
                .map(|s| u32::from(s) * total / u32::from(CONSTRUCTION_STAGES));
            let phases: Vec<BuildingPhase> = std::iter::once(BuildingPhase::Built { training: 0 })
                .chain(
                    stages
                        .clone()
                        .map(|progress| BuildingPhase::Site { progress }),
                )
                .chain(stages.map(|progress| BuildingPhase::Upgrading { progress }))
                .collect();
            let size = kind.size();
            for phase in phases {
                state.building_mut(site).unwrap().phase = phase;
                for &worker in &workers {
                    for dy in -8..size.1 * 4 + 8 {
                        for dx in -8..size.0 * 4 + 8 {
                            let pos = jittered(&mut rng, low, quarter, dx, dy);
                            let unit = state.unit_mut(worker).unwrap();
                            unit.pos = pos;
                            unit.order = if (dx + dy) % 3 == 0 {
                                crate::Order::Build { site }
                            } else {
                                crate::Order::Idle
                            };
                            let unit = state.unit(worker).unwrap();
                            let fast = state.in_building_work_reach(unit, site);
                            assert_eq!(
                                fast,
                                surface_work_reach(&state, unit, site),
                                "{kind:?} {phase:?} {pos:?}"
                            );
                            let b = state.building(site).unwrap();
                            let reach = work_reach(unit);
                            skipped +=
                                usize::from(pos.dist_sq(b.closest_point_to(pos)) > reach * reach);
                            reached += usize::from(fast);
                            missed += usize::from(!fast);
                        }
                    }
                }
            }
        }
    }
    assert!(
        reached > 0 && missed > 0 && skipped > 0,
        "{reached} reached, {missed} missed, {skipped} beyond the footprint reject"
    );
}
