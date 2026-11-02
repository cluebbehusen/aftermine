use super::*;

const MAP: (i32, i32) = (128, 112);

fn camera() -> Camera {
    Camera::new(vec2(64.0, 56.0), MAP.0, MAP.1, vec2(1210.0, 834.0))
}

/// Runs a script the way the frame loop does, returning each frame's step.
fn run(script: &FrameScript, camera: &mut Camera) -> Vec<Step> {
    let mut run = FrameScriptRun::new(script);
    let mut steps = Vec::new();
    loop {
        let step = run.step(camera);
        steps.push(step);
        if step == Step::Done {
            return steps;
        }
        camera.update(FRAME_SECONDS);
        assert!(steps.len() < 100_000, "the script never finished");
    }
}

fn count(steps: &[Step], wanted: Step) -> usize {
    steps.iter().filter(|step| **step == wanted).count()
}

#[test]
fn a_motionless_script_settles_then_measures_its_frames_and_leaves_the_camera_alone() {
    let mut camera = camera();
    let center = camera.center;
    let zoom = camera.zoom;
    let steps = run(
        &FrameScript {
            settle_frames: 3,
            plays: false,
            motion: CameraMotion::None { frames: 5 },
        },
        &mut camera,
    );
    assert_eq!(&steps[..3], [Step::Settle; 3]);
    assert_eq!(&steps[3..8], [Step::Measure; 5]);
    assert_eq!(steps[8..], [Step::Done]);
    assert_eq!((camera.center, camera.zoom), (center, zoom));
}

#[test]
fn a_hold_frames_its_view_before_the_first_settling_frame() {
    let mut camera = camera();
    let mut script = FrameScriptRun::new(&FrameScript {
        settle_frames: 1,
        plays: false,
        motion: CameraMotion::Hold {
            center: [20.0, 30.0],
            zoom: 12.0,
            frames: 1,
        },
    });
    assert_eq!(script.step(&mut camera), Step::Settle);
    assert_eq!(camera.zoom, 12.0);
    let framed = camera.center;
    assert_eq!(script.step(&mut camera), Step::Measure);
    assert_eq!(camera.center, framed, "a hold stays put");
    assert_eq!(script.step(&mut camera), Step::Done);
    assert_eq!(script.step(&mut camera), Step::Done, "done stays done");
}

#[test]
fn a_pan_keeps_moving_for_all_its_frames_by_turning_back_at_the_map_edge() {
    let mut camera = camera();
    let mut script = FrameScriptRun::new(&FrameScript {
        settle_frames: 0,
        plays: false,
        motion: CameraMotion::Pan {
            center: [120.0, 56.0],
            zoom: 32.0,
            tiles_per_second: 30.0,
            frames: 2_000,
        },
    });
    let mut stalled = 0;
    let mut seen_left = false;
    let mut seen_right = false;
    let mut previous = None;
    loop {
        match script.step(&mut camera) {
            Step::Done => break,
            Step::Settle => unreachable!("no settling was asked for"),
            Step::Measure => {}
        }
        if let Some(previous) = previous {
            let moved: f32 = camera.center.x - previous;
            stalled += i32::from(moved == 0.0);
            seen_left |= moved < 0.0;
            seen_right |= moved > 0.0;
        }
        previous = Some(camera.center.x);
    }
    assert_eq!(stalled, 0, "an edge must not freeze the view");
    assert!(seen_left && seen_right, "the pan turns around");
}

#[test]
fn a_zoom_measures_each_glide_and_its_rest_but_never_the_settling_between() {
    let mut camera = camera();
    let steps = run(
        &FrameScript {
            settle_frames: 4,
            plays: false,
            motion: CameraMotion::Zoom {
                center: [64.0, 56.0],
                from: 32.0,
                to: 8.0,
                repeats: 3,
                rest_frames: 6,
            },
        },
        &mut camera,
    );
    assert_eq!(count(&steps, Step::Settle), 3 * 4);
    assert_eq!(camera.zoom, 8.0, "the last glide landed");
    assert!(!camera.zooming());
    let glides: Vec<&[Step]> = steps[..steps.len() - 1]
        .split(|step| *step == Step::Settle)
        .filter(|measured| !measured.is_empty())
        .collect();
    assert_eq!(glides.len(), 3);
    assert_eq!(glides[0].len(), glides[1].len(), "repeats are identical");
    assert_eq!(glides[1].len(), glides[2].len(), "repeats are identical");
    assert!(glides[0].len() > 6, "the glide itself is measured");
}

#[test]
fn a_zoom_in_lands_on_its_target_too() {
    let mut camera = camera();
    run(
        &FrameScript {
            settle_frames: 0,
            plays: false,
            motion: CameraMotion::Zoom {
                center: [64.0, 56.0],
                from: 8.0,
                to: 32.0,
                repeats: 1,
                rest_frames: 0,
            },
        },
        &mut camera,
    );
    assert_eq!(camera.zoom, 32.0);
}

#[test]
fn the_same_script_ends_at_the_same_camera_every_time() {
    let script = FrameScript {
        settle_frames: 2,
        plays: false,
        motion: CameraMotion::Zoom {
            center: [30.0, 40.0],
            from: 32.0,
            to: 11.0,
            repeats: 2,
            rest_frames: 3,
        },
    };
    let (mut first, mut second) = (camera(), camera());
    let first_steps = run(&script, &mut first);
    let second_steps = run(&script, &mut second);
    assert_eq!(first_steps, second_steps);
    assert_eq!(first.center, second.center);
    assert_eq!(first.zoom, second.zoom);
}

#[test]
fn a_zoom_with_no_repeats_is_done_at_once() {
    let mut camera = camera();
    let steps = run(
        &FrameScript {
            settle_frames: 10,
            plays: false,
            motion: CameraMotion::Zoom {
                center: [64.0, 56.0],
                from: 32.0,
                to: 8.0,
                repeats: 0,
                rest_frames: 10,
            },
        },
        &mut camera,
    );
    assert_eq!(steps, [Step::Done]);
}

#[test]
fn zoom_repeats_with_nothing_to_draw_finish_without_recursing() {
    let mut camera = camera();
    let steps = run(
        &FrameScript {
            settle_frames: 0,
            plays: false,
            motion: CameraMotion::Zoom {
                center: [64.0, 56.0],
                from: 32.0,
                to: 32.0,
                repeats: 1_000_000,
                rest_frames: 0,
            },
        },
        &mut camera,
    );
    assert_eq!(steps, [Step::Done]);
}
