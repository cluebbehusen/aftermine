use super::*;

#[test]
fn heading_of_rounds_to_the_nearest_compass_step() {
    assert_eq!(heading_of(Vec2Fx::new(Fx::lit("3"), Fx::ZERO)), 0);
    assert_eq!(heading_of(Vec2Fx::new(Fx::ZERO, Fx::lit("-2"))), 192);
    assert_eq!(heading_of(Vec2Fx::new(Fx::lit("1"), Fx::lit("1"))), 32);
}

fn exhaustive_heading(v: Vec2Fx) -> u8 {
    let mut best = 0;
    let mut best_dot = v.x;
    for k in 1..=255 {
        let d = dir(k);
        let dot = d.x * v.x + d.y * v.y;
        if dot > best_dot {
            best = k;
            best_dot = dot;
        }
    }
    best
}

#[test]
fn quadrant_search_preserves_fixed_point_rounding_and_first_ties() {
    for x in -64..=64 {
        for y in -64..=64 {
            let v = Vec2Fx::new(Fx::from_bits(x), Fx::from_bits(y));
            assert_eq!(heading_of(v), exhaustive_heading(v), "{v:?}");
        }
    }
    for step in 0..=255u8 {
        let v = dir(step);
        let reflected_x = dir(128u8.wrapping_sub(step));
        let reflected_y = dir(step.wrapping_neg());
        assert_eq!(reflected_x, Vec2Fx::new(-v.x, v.y));
        assert_eq!(reflected_y, Vec2Fx::new(v.x, -v.y));
        for scale in [Fx::from_bits(1), Fx::lit("0.01"), Fx::ONE, Fx::lit("8192")] {
            let boundary = v + dir(step.wrapping_add(1));
            for dx in [-1, 0, 1] {
                for dy in [-1, 0, 1] {
                    let v = Vec2Fx::new(
                        boundary.x * scale + Fx::from_bits(dx),
                        boundary.y * scale + Fx::from_bits(dy),
                    );
                    assert_eq!(heading_of(v), exhaustive_heading(v), "{v:?}");
                }
            }
        }
    }
    let mut rng = crate::rng::Pcg32::new(0x1234, 7);
    for _ in 0..20_000 {
        let v = Vec2Fx::new(
            Fx::from_bits(i64::from(rng.next_u32().cast_signed()) << 13),
            Fx::from_bits(i64::from(rng.next_u32().cast_signed()) << 13),
        );
        assert_eq!(heading_of(v), exhaustive_heading(v), "{v:?}");
    }
}

/// The quadrant scan [`heading_of`] narrows to the two steps around `v`.
fn scanned_heading(v: Vec2Fx) -> u8 {
    let first: u8 = if v.y >= Fx::ZERO {
        if v.x >= Fx::ZERO { 0 } else { 64 }
    } else if v.x > Fx::ZERO {
        192
    } else {
        128
    };
    let mut best = 0;
    let mut best_dot = v.x;
    for k in first..=first.saturating_add(64) {
        let d = dir(k);
        let dot = d.x * v.x + d.y * v.y;
        if dot > best_dot {
            best = k;
            best_dot = dot;
        }
    }
    best
}

fn raw(v: Vec2Fx) -> (i128, i128) {
    (i128::from(v.x.to_bits()), i128::from(v.y.to_bits()))
}

fn exact_dot(a: (i128, i128), b: (i128, i128)) -> i128 {
    a.0 * b.0 + a.1 * b.1
}

/// The bracket search needs every step's direction turned strictly
/// toward +y from the one before it, exactly.
#[test]
fn each_step_turns_toward_positive_y_from_the_last() {
    for k in 0..=u8::MAX {
        let (a, b) = (raw(dir(k)), raw(dir(k.wrapping_add(1))));
        assert!(a.0 * b.1 - a.1 * b.0 > 0, "step {k}");
    }
}

/// The margin [`BRACKETED_FROM`] rests on. A vector between steps `a` and
/// `a + 1` is `p·d(a) + q·d(a + 1)` with `p, q ≥ 0`, so for any other step
/// `k` the mean of the pair's exact dots beats `d(k)·v` by `p·G1 + q·G2`,
/// at least `|v|·min G / max |d|`. A computed dot is two products, each
/// within an ulp of exact, so a step trailing by 2^34 (raw squared) can
/// neither win nor tie.
#[test]
fn outside_its_bracket_no_step_comes_within_rounding_of_the_best() {
    let one = 1_i128 << 32;
    let mut rng = crate::rng::Pcg32::new(0x0b5e, 3);
    for _ in 0..10_000 {
        let a = Fx::from_bits(i64::from(rng.next_u32()) - (1 << 31));
        let b = Fx::from_bits(i64::from(rng.next_u32().cast_signed()) << 24);
        let error =
            i128::from((a * b).to_bits()) * one - i128::from(a.to_bits()) * i128::from(b.to_bits());
        assert!(error.abs() < one, "{a:?} * {b:?}");
    }
    let longest = (0..=u8::MAX)
        .map(|k| exact_dot(raw(dir(k)), raw(dir(k))))
        .max()
        .unwrap();
    assert!(longest <= (one + 1) * (one + 1));
    let mut least = i128::MAX;
    for a in 0..=u8::MAX {
        let (low, high) = (raw(dir(a)), raw(dir(a.wrapping_add(1))));
        for k in (0..=u8::MAX).filter(|&k| k != a && k != a.wrapping_add(1)) {
            let d = raw(dir(k));
            let twice_g1 = exact_dot(low, low) + exact_dot(low, high) - 2 * exact_dot(d, low);
            let twice_g2 = exact_dot(low, high) + exact_dot(high, high) - 2 * exact_dot(d, high);
            least = least.min(twice_g1).min(twice_g2);
        }
    }
    assert!(
        least * i128::from(BRACKETED_FROM) >= (1_i128 << 35) * (one + 1),
        "{least}"
    );
}

#[test]
fn bracketed_heading_matches_the_quadrant_scan() {
    let check = |x: i64, y: i64| {
        let v = Vec2Fx::new(Fx::from_bits(x), Fx::from_bits(y));
        assert_eq!(heading_of(v), scanned_heading(v), "{v:?}");
    };
    // Every direction at the shortest bracketed span, and just below it.
    let near = i64::try_from(BRACKETED_FROM).unwrap();
    for span in [near - 1, near] {
        for t in -span..span {
            check(t, span);
            check(span, -t);
            check(-t, -span);
            check(-span, t);
        }
    }
    // Along each step and midway to the next, at every scale through the
    // longest bracketed span, an ulp either way.
    for k in 0..=u8::MAX {
        for along in [dir(k), dir(k) + dir(k.wrapping_add(1))] {
            let (x, y) = (along.x.to_bits(), along.y.to_bits());
            let scaled = (0..18)
                .map(|s| (x >> s, y >> s))
                .chain((1..28).map(|s| (x << s, y << s)));
            for (x, y) in scaled {
                for (dx, dy) in [(-1, -1), (-1, 0), (0, 0), (0, 1), (1, 1), (1, -1)] {
                    check(x + dx, y + dy);
                }
            }
        }
    }
    // Both edges of the overflow guard.
    let far = i64::try_from(BRACKETED_BELOW).unwrap();
    for (x, y) in [
        (far - 1, 7),
        (far, 7),
        (-3, far - 1),
        (-3, -far),
        (far - 1, far - 1),
    ] {
        check(x, y);
    }
    // Every magnitude, each axis drawn on its own.
    let mut rng = crate::rng::Pcg32::new(0x5eed, 11);
    let mut draw = || {
        let bits = (u64::from(rng.next_u32()) << 32 | u64::from(rng.next_u32())).cast_signed();
        bits >> (3 + rng.next_u32() % 61)
    };
    for _ in 0..1_000_000 {
        let (x, y) = (draw(), draw());
        check(x, y);
    }
}

#[test]
fn every_step_is_a_unit_vector() {
    for (k, v) in COMPASS_256.iter().enumerate() {
        let len_sq = v.x * v.x + v.y * v.y;
        let err = (len_sq - Fx::ONE).abs();
        assert!(err < Fx::lit("0.0001"), "step {k} has |v|^2 = {len_sq:?}");
    }
}

#[test]
fn quarter_turns_land_on_the_axes() {
    assert_eq!(dir(0), Vec2Fx::new(Fx::ONE, Fx::ZERO));
    assert_eq!(dir(64), Vec2Fx::new(Fx::ZERO, Fx::ONE));
    assert_eq!(dir(128), Vec2Fx::new(-Fx::ONE, Fx::ZERO));
    assert_eq!(dir(192), Vec2Fx::new(Fx::ZERO, -Fx::ONE));
}

#[test]
fn adjacent_steps_stay_adjacent() {
    // One step is about 1.4 degrees; consecutive directions must
    // never jump. Catches a shuffled or truncated table.
    for k in 0..=u8::MAX {
        let a = dir(k);
        let b = dir(k.wrapping_add(1));
        let dot = a.x * b.x + a.y * b.y;
        assert!(dot > Fx::lit("0.999"), "step {k} breaks continuity");
    }
}
