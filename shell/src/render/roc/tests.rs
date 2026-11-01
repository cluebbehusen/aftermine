use super::*;

#[test]
fn the_hatch_swings_open_for_a_rider_then_closes() {
    assert_eq!(hatch_opening(None), 0.0);
    let at = |progress| hatch_opening(Some(TransportActionState::Boarding { progress }));
    assert_eq!(at(0.0), 0.0);
    assert_eq!(at(0.4), 1.0, "open while the rider passes");
    assert_eq!(at(1.0), 0.0, "shut once it has");
    assert_eq!(
        hatch_opening(Some(TransportActionState::Unloading { progress: 0.4 })),
        1.0,
        "leaving opens it too"
    );
}

#[test]
fn hull_offsets_turn_with_the_hull() {
    let east = turned(vec2(0.0, -10.0), std::f32::consts::FRAC_PI_2, 2.0);
    assert!((east - vec2(20.0, 0.0)).length() < 1e-4, "{east}");
}

/// How far the wall layer drops the hull's silhouette toward the tail, in
/// canvas pixels.
const WALL_DROP: f32 = 5.0;

#[test]
fn no_gun_fires_across_the_hull() {
    let wall = Image::from_file_with_format(
        include_bytes!("../../../../assets/sprites/rig_roc_wall.png"),
        Some(ImageFormat::Png),
    )
    .expect("the wall layer decodes");
    let hull = |x: f32, y: f32| {
        let y = y + WALL_DROP;
        (0.0..f32::from(wall.width)).contains(&x)
            && (0.0..f32::from(wall.height)).contains(&y)
            && wall.bytes[(crate::numeric::to_usize(y) * usize::from(wall.width)
                + crate::numeric::to_usize(x))
                * 4
                + 3]
                >= 128
    };
    let guns = UnitKind::Roc
        .stats()
        .mounts
        .expect("the Roc has mounted guns");
    for (slot, mount) in guns.mounts.iter().enumerate() {
        let pivot = vec2(
            128.0 + mount.starboard.to_num::<f32>() * CANVAS_TILE,
            128.0 - mount.forward.to_num::<f32>() * CANVAS_TILE,
        );
        assert!(
            !hull(pivot.x, pivot.y),
            "gun {slot}'s pivot lies outside the hull"
        );
        for relative in (0..=u8::MAX).filter(|relative| mount.covers(*relative)) {
            let angle = f32::from(relative) * std::f32::consts::TAU / 256.0;
            let aim = vec2(angle.sin(), -angle.cos());
            for reach in 1..256_u16 {
                let point = pivot + aim * f32::from(reach);
                assert!(
                    !hull(point.x, point.y),
                    "gun {slot} at bearing {relative} crosses the hull {reach} pixels out"
                );
            }
        }
    }
}
