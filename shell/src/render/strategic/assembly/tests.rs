use super::*;

/// The shipped round times for both rigs.
fn rigs() -> [(RigLayout, usize); 2] {
    let round = |kind: aftermine_sim::BuildingKind| {
        kind.magazine().expect("magazine kind").round_ticks as usize
    };
    [
        (SPIRE_GANTRY, round(aftermine_sim::BuildingKind::Spire)),
        (LATTICE_ARM, round(aftermine_sim::BuildingKind::Lattice)),
    ]
}

/// Gantry or arm frames in the art.
fn frames(layout: RigLayout) -> usize {
    layout.last_carry() + layout.tail
}

/// How many numbered frames the atlas ships under `stem`.
fn shipped(atlas: &serde_json::Map<String, serde_json::Value>, stem: &str) -> usize {
    (1..=256)
        .take_while(|frame| atlas.contains_key(&format!("{stem}_{frame}")))
        .count()
}

#[test]
fn layouts_match_the_shipped_rig_frames() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/sprites/atlas.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    let atlas: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&text).expect("atlas.json is an object");
    for (layout, rig, stacks, noses) in [
        (
            SPIRE_GANTRY,
            "rig_spire_gantry",
            "rig_spire_stack",
            "rig_spire_nose",
        ),
        (
            LATTICE_ARM,
            "rig_lattice_arm_left",
            "rig_lattice_stack",
            "rig_lattice_nose_left",
        ),
        (
            LATTICE_ARM,
            "rig_lattice_arm_right",
            "rig_lattice_stack",
            "rig_lattice_nose_right",
        ),
    ] {
        assert_eq!(shipped(&atlas, rig), frames(layout), "{rig}");
        assert_eq!(shipped(&atlas, stacks), layout.parts - 1, "{stacks}");
        assert_eq!(
            shipped(&atlas, noses),
            layout.steps - layout.lead,
            "{noses} covers the nose's carry"
        );
    }
}

#[test]
fn every_move_plays_at_the_drawn_pace_and_ends_with_the_round() {
    for (layout, round_ticks) in rigs() {
        let poses: Vec<Assembly> = (0..round_ticks)
            .map(|progress| layout.at(progress, round_ticks))
            .collect();
        // Each frame shown holds exactly its four ticks, in order.
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for pose in &poses {
            if let Some(frame) = pose.frame {
                match runs.last_mut() {
                    Some((last, ticks)) if *last == frame => *ticks += 1,
                    _ => runs.push((frame, 1)),
                }
            }
        }
        assert!(
            runs.iter().all(|&(_, ticks)| ticks == FRAME_TICKS),
            "{runs:?}"
        );
        assert!(
            runs.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "{runs:?}"
        );
        let first_carry = layout.lead + 1;
        assert_eq!(
            runs.iter().map(|&(frame, _)| frame).collect::<Vec<_>>(),
            (first_carry..=frames(layout)).collect::<Vec<_>>(),
            "every frame but the first part's swing back plays once"
        );
        // Parts only ever join the stack, and all stand by the end.
        assert!(
            poses
                .windows(2)
                .all(|pair| pair[0].seated <= pair[1].seated)
        );
        assert_eq!(poses[0].seated, 0);
        assert_eq!(poses[round_ticks - 1].seated, layout.parts);
        assert_eq!(poses[round_ticks - 1].frame, None, "parked by the end");
    }
}

#[test]
fn the_box_makes_each_part_for_an_equal_share_of_the_round() {
    for (layout, round_ticks) in rigs() {
        let wait = layout.wait_ticks(round_ticks);
        assert!(
            wait * layout.parts * 4 > round_ticks * 3,
            "making fills most of the round"
        );
        let mut spells = Vec::new();
        let mut current = 0;
        for progress in 0..round_ticks {
            let pose = layout.at(progress, round_ticks);
            if pose.making {
                assert_eq!(pose.frame, None, "the hook or arm waits at the box");
                current += 1;
            } else if current > 0 {
                spells.push(current);
                current = 0;
            }
        }
        assert_eq!(spells, vec![wait; layout.parts]);
    }
}

#[test]
fn locks_and_clamps_shut_as_their_part_is_seated_and_before_the_round_ends() {
    for (layout, round_ticks) in rigs() {
        for part in 0..layout.parts {
            let at = layout.seated_at(part, round_ticks);
            assert_eq!(layout.at(at - 1, round_ticks).seated, part);
            assert_eq!(layout.at(at, round_ticks).seated, part + 1);
            assert_eq!(layout.closing(part, at - 1, round_ticks), 0);
            assert_eq!(layout.closing(part, at, round_ticks), 1);
            assert_eq!(
                layout.closing(part, round_ticks - 1, round_ticks),
                CLOSE_STEPS
            );
        }
    }
}

#[test]
fn a_round_shorter_than_its_moves_still_assembles_in_order() {
    for (layout, _) in rigs() {
        let round_ticks = frames(layout) * FRAME_TICKS / 2;
        assert_eq!(layout.wait_ticks(round_ticks), 0);
        for progress in 0..round_ticks * 3 {
            let pose = layout.at(progress, round_ticks);
            assert!(!pose.making);
            assert!(
                pose.frame
                    .is_none_or(|frame| (1..=frames(layout)).contains(&frame))
            );
            assert!(pose.seated <= layout.parts);
        }
    }
}
