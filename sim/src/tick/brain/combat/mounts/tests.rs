use super::*;
use crate::stats::UnitKind;

fn guns() -> MountedGuns {
    UnitKind::Roc
        .stats()
        .mounts
        .expect("the Roc has mounted guns")
}

#[test]
fn each_gun_bears_ahead_of_the_beam_and_on_its_own_rear_quarter_only() {
    let [port, starboard] = guns().mounts else {
        panic!("two guns");
    };
    for relative in 0..=u8::MAX {
        // Starboard positive: the port gun is blind behind the starboard
        // wing, the starboard gun behind the port wing, and dead astern sits
        // on both arcs' edges.
        let signed = relative.cast_signed();
        assert_eq!(port.covers(relative), signed <= 64, "port at {signed}");
        assert_eq!(
            starboard.covers(relative),
            signed >= -64 || signed == i8::MIN,
            "starboard at {signed}"
        );
        assert_eq!(
            port.covers(relative) && starboard.covers(relative),
            (-64..=64).contains(&signed) || signed == i8::MIN,
            "both bear ahead of the beam: {signed}"
        );
    }
}

#[test]
fn a_gun_traverses_through_its_arc_never_its_blind_quarter() {
    let [port, _] = guns().mounts else {
        panic!("two guns");
    };
    // From its forward edge (+64, starboard abeam) to its aft edge (-128,
    // astern), the port gun swings back through the nose, not across its
    // blind starboard quarter.
    let from = port.arc_offset(64);
    let to = port.arc_offset(128);
    assert_eq!(from, 96);
    assert_eq!(to, -96);
    let mut offset = from;
    let mut visited = Vec::new();
    while offset != to {
        offset += (to - offset).clamp(-6, 6);
        visited.push(offset);
    }
    assert!(visited.iter().all(|step| step.unsigned_abs() <= 96));
    assert!(visited.contains(&0), "it passes its arc's centre");
}

#[test]
fn pivots_turn_with_the_hull_exactly() {
    let pos = Vec2Fx::new(Fx::from_num(10), Fx::from_num(7));
    for mount in guns().mounts {
        for heading in 0..=u8::MAX {
            let pivot = mount.pivot(pos, heading);
            let half = mount.pivot(pos, heading.wrapping_add(128));
            assert_eq!(pivot - pos, pos - half, "a half turn mirrors the pivot");
            let quarter = mount.pivot(pos, heading.wrapping_add(64)) - pos;
            let offset = pivot - pos;
            // A quarter turn maps (x, y) to (-y, x) on the compass table.
            assert_eq!(quarter, Vec2Fx::new(-offset.y, offset.x), "at {heading}");
        }
    }
}

#[test]
fn the_reach_margin_covers_every_pivot() {
    let guns = guns();
    for mount in guns.mounts {
        let offset =
            chassis::fx::sqrt(mount.forward * mount.forward + mount.starboard * mount.starboard);
        assert!(offset <= guns.reach, "{offset} past {}", guns.reach);
    }
}

#[test]
fn only_a_target_under_the_rear_hull_is_beyond_every_turn() {
    let guns = guns();
    let pos = Vec2Fx::new(Fx::from_num(10), Fx::from_num(10));
    let at = |x: &str, y: &str| pos + Vec2Fx::new(Fx::lit(x), Fx::lit(y));
    assert!(
        !feedable(guns, pos, pos),
        "nothing aims at the hull's own centre"
    );
    assert!(
        !feedable(guns, pos, at("0.4", "0")),
        "inside the pivots' line"
    );
    assert!(feedable(guns, pos, at("0.8", "0")));
    assert!(
        feedable(guns, pos, at("-3", "0")),
        "far astern turns into an arc"
    );
    assert!(feedable(guns, pos, at("0", "4")));
}
