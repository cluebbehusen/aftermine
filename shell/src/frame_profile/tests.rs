use super::*;

#[test]
fn exact_window_retains_high_rate_frames_between_simulation_ticks() {
    let mut profiler = FrameProfiler::new(true);
    profiler.arm(0, 480).unwrap();
    assert!(profiler.take_start_barrier());
    for tick in 0..480 {
        for frame in 0..64 {
            profiler.record(FrameObservation {
                mode: "playing",
                active_playing: true,
                tick_start: tick,
                tick_end: tick + u64::from(frame == 63),
                work_ms: 0.2,
                submit_ms: 0.0,
                units: 1,
                buildings: 1,
            });
        }
    }
    let view = profiler.snapshot(false);
    assert_eq!(view.frames, 480 * 64);
    assert_eq!(view.tick_start, 0);
    assert_eq!(view.tick_end, 480);
    assert_eq!(view.ticks_presented, 480);
    let window = view.window.unwrap();
    assert!(window.complete);
    assert!(!window.truncated);
}

#[test]
fn snapshot_reports_native_work_and_can_reset_the_window() {
    let mut profiler = FrameProfiler::new(true);
    for (index, work_ms) in [1.0, 2.0, 40.0].into_iter().enumerate() {
        profiler.record(FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 100 + index as u64,
            tick_end: 101 + index as u64,
            work_ms,
            submit_ms: 0.0,
            units: 10 + index,
            buildings: 4,
        });
    }

    let view = profiler.snapshot(true);
    assert_eq!(view.renderer, "gpu");
    assert_eq!(view.frames, 3);
    assert_eq!(view.tick_start, 100);
    assert_eq!(view.tick_end, 103);
    assert_eq!(view.ticks_presented, 3);
    assert_eq!(view.work.p50_ms, 2.0);
    assert_eq!(view.work.p95_ms, 40.0);
    assert_eq!(view.work_over_16_7_ms, 1);
    assert_eq!(view.work_over_33_3_ms, 1);
    assert_eq!(view.slowest.expect("slow frame").tick_end, 103);
    assert_eq!(profiler.snapshot(false).frames, 0);
}

#[test]
fn disabled_profiling_has_no_samples() {
    let mut profiler = FrameProfiler::new(false);
    profiler.record(FrameObservation {
        mode: "playing",
        active_playing: true,
        tick_start: 1,
        tick_end: 2,
        work_ms: 99.0,
        submit_ms: 0.0,
        units: 1,
        buildings: 1,
    });
    assert_eq!(profiler.snapshot(false).frames, 0);
}

#[test]
fn profiling_windows_require_an_enabled_forward_tick_range() {
    let mut disabled = FrameProfiler::new(false);
    assert!(disabled.arm(10, 20).unwrap_err().contains("disabled"));

    let mut profiler = FrameProfiler::new(true);
    assert!(profiler.arm(10, 10).unwrap_err().contains("greater"));
    assert!(profiler.arm(20, 10).unwrap_err().contains("greater"));
    profiler.arm(10, 20).expect("valid window");
    assert_eq!(profiler.stop_tick(), Some(20));
}

#[test]
fn exact_window_keeps_contiguous_active_playing_frames_including_tick_waits() {
    let mut profiler = FrameProfiler::new(true);
    profiler.arm(10, 14).unwrap();
    assert!(profiler.take_start_barrier());
    assert!(!profiler.take_start_barrier());
    for observation in [
        FrameObservation {
            mode: "pause_menu",
            active_playing: false,
            tick_start: 10,
            tick_end: 10,
            work_ms: 50.0,
            submit_ms: 0.0,
            units: 1,
            buildings: 1,
        },
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 10,
            tick_end: 10,
            work_ms: 1.0,
            submit_ms: 0.0,
            units: 10,
            buildings: 4,
        },
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 10,
            tick_end: 12,
            work_ms: 2.0,
            submit_ms: 0.0,
            units: 10,
            buildings: 4,
        },
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 12,
            tick_end: 14,
            work_ms: 3.0,
            submit_ms: 0.0,
            units: 11,
            buildings: 4,
        },
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 14,
            tick_end: 15,
            work_ms: 99.0,
            submit_ms: 0.0,
            units: 11,
            buildings: 4,
        },
    ] {
        profiler.record(observation);
    }
    let view = profiler.snapshot(false);
    assert_eq!(view.frames, 3);
    assert_eq!(view.tick_start, 10);
    assert_eq!(view.tick_end, 14);
    assert_eq!(view.ticks_presented, 4);
    assert!(view.slowest.iter().all(|frame| frame.mode == "playing"));
    let window = view.window.unwrap();
    assert!(window.complete);
    assert!(window.elapsed_ms > 0.0);
    assert!(!window.truncated);
    assert_eq!(profiler.stop_tick(), None);
}

#[test]
fn retained_samples_are_bounded() {
    let mut profiler = FrameProfiler::new(true);
    profiler
        .arm(0, MAX_SAMPLES as u64 + 10)
        .expect("long window");
    assert!(profiler.take_start_barrier());
    for tick in 0..(MAX_SAMPLES as u64 + 5) {
        profiler.record(FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: tick,
            tick_end: tick + 1,
            work_ms: 1.0,
            submit_ms: 0.0,
            units: 1,
            buildings: 1,
        });
    }
    let view = profiler.snapshot(false);
    assert_eq!(view.frames, MAX_SAMPLES);
    assert_eq!(view.tick_start, 5);
    assert_eq!(view.tick_end, MAX_SAMPLES as u64 + 5);
    let window = view.window.expect("armed window");
    assert!(!window.complete);
    assert!(window.truncated, "dropping an in-window sample is reported");
}

#[test]
fn an_exact_window_ignores_discontinuous_or_overshooting_frames() {
    let mut profiler = FrameProfiler::new(true);
    profiler.arm(10, 12).expect("window");
    assert!(profiler.take_start_barrier());
    for observation in [
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 9,
            tick_end: 10,
            work_ms: 90.0,
            submit_ms: 0.0,
            units: 99,
            buildings: 99,
        },
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 10,
            tick_end: 13,
            work_ms: 80.0,
            submit_ms: 0.0,
            units: 88,
            buildings: 88,
        },
        FrameObservation {
            mode: "playing",
            active_playing: true,
            tick_start: 10,
            tick_end: 12,
            work_ms: 2.0,
            submit_ms: 0.0,
            units: 8,
            buildings: 4,
        },
    ] {
        profiler.record(observation);
    }
    let view = profiler.snapshot(false);
    assert_eq!(view.frames, 1);
    assert_eq!(view.tick_start, 10);
    assert_eq!(view.tick_end, 12);
    assert_eq!(view.slowest.expect("one sample").units, 8);
    assert!(view.window.expect("window").complete);
}

fn frame(work_ms: f64, submit_ms: f64) -> FrameObservation<'static> {
    FrameObservation {
        mode: "playback",
        active_playing: false,
        tick_start: 7,
        tick_end: 7,
        work_ms,
        submit_ms,
        units: 1,
        buildings: 1,
    }
}

fn pass_timing<'a>(view: &'a FrameProfileView, name: &str) -> &'a TimingSummary {
    &view
        .passes
        .iter()
        .find(|pass| pass.name == name)
        .expect("named pass")
        .timing
}

#[test]
fn every_pass_has_a_distinct_name_and_a_slot_in_its_own_order() {
    let names: std::collections::BTreeSet<_> =
        FramePass::ALL.iter().map(|pass| pass.name()).collect();
    assert_eq!(names.len(), PASSES);
    assert!(!names.contains(OTHER_PASS));
    for (slot, pass) in FramePass::ALL.iter().enumerate() {
        assert_eq!(*pass as usize, slot);
    }
}

#[test]
fn a_pass_charges_only_the_frame_it_ran_in() {
    let mut profiler = FrameProfiler::new(true);
    pass(FramePass::Units, || {
        std::thread::sleep(Duration::from_millis(2));
    });
    let answer = measure(FramePass::Units, || 42);
    assert_eq!(answer, 42);
    profiler.record(frame(50.0, 0.0));
    profiler.record(frame(50.0, 0.0));

    let view = profiler.snapshot(false);
    let units = pass_timing(&view, "units");
    assert!(units.max_ms >= 2.0, "the slept frame: {units:?}");
    assert_eq!(units.p50_ms, 0.0, "the second frame ran no pass");
    assert_eq!(pass_timing(&view, "fog").max_ms, 0.0);
}

#[test]
fn other_is_the_work_no_pass_claimed_and_never_negative() {
    let mut profiler = FrameProfiler::new(true);
    pass(FramePass::Hud, || {
        std::thread::sleep(Duration::from_millis(2));
    });
    profiler.record(frame(0.5, 0.0));
    profiler.record(frame(9.0, 0.0));

    let view = profiler.snapshot(false);
    let other = pass_timing(&view, OTHER_PASS);
    assert_eq!(other.p50_ms, 0.0, "a pass longer than work leaves nothing");
    assert_eq!(other.max_ms, 9.0);
}

#[test]
fn the_120_hz_count_and_the_slowest_frame_include_submission() {
    let mut profiler = FrameProfiler::new(true);
    profiler.record(frame(8.0, 0.0));
    profiler.record(frame(6.0, 3.0));
    profiler.record(frame(1.0, 1.0));

    let view = profiler.snapshot(false);
    assert_eq!(view.cpu_over_8_3_ms, 1, "only 6 + 3 misses the budget");
    assert_eq!(view.cpu.max_ms, 9.0);
    assert_eq!(view.work.max_ms, 8.0);
    assert_eq!(view.submit.max_ms, 3.0);
    let slowest = view.slowest.expect("slowest");
    assert_eq!(slowest.work_ms, 6.0);
    assert_eq!(slowest.submit_ms, 3.0);
    assert_eq!(slowest.passes.len(), PASSES + 1);
    assert_eq!(slowest.passes.last().expect("other").name, OTHER_PASS);
    assert_eq!(slowest.passes.last().expect("other").ms, 6.0);
}

#[test]
fn a_disabled_profiler_collects_no_pass_time() {
    let mut profiler = FrameProfiler::new(false);
    assert_eq!(measure(FramePass::Fog, || 7), 7);
    let mut ran = false;
    pass(FramePass::Fog, || ran = true);
    assert!(ran, "off still draws");
    assert_eq!(PASS_SPENT.get(), [0.0; PASSES]);
    profiler.record(frame(1.0, 0.0));
    assert_eq!(profiler.snapshot(false).frames, 0);
}

fn camera() -> Camera {
    use macroquad::prelude::vec2;
    Camera::new(vec2(20.0, 20.0), 64, 40, vec2(1210.0, 834.0))
}

fn script(settle_frames: u32, frames: u32) -> FrameScript {
    FrameScript {
        settle_frames,
        motion: aftermine_protocol::CameraMotion::None { frames },
    }
}

#[test]
fn a_script_keeps_only_its_measured_frames_and_reports_finishing() {
    let mut profiler = FrameProfiler::new(true);
    let mut camera = camera();
    profiler.record(frame(50.0, 0.0));
    assert_eq!(profiler.snapshot(false).script_complete, None);
    profiler.begin_script(&script(2, 3)).expect("enabled");
    assert_eq!(profiler.snapshot(false).frames, 0, "old samples are gone");
    assert_eq!(profiler.snapshot(false).script_complete, Some(false));

    let mut drawn = 0;
    while profiler.scripted() {
        profiler.step_script(&mut camera);
        drawn += 1;
        profiler.record(frame(f64::from(drawn), 0.0));
    }
    assert_eq!(drawn, 6, "two settling, three measured, then the finish");
    let view = profiler.snapshot(false);
    assert_eq!(view.frames, 3);
    assert_eq!(view.work.max_ms, 5.0, "the finishing frame is not a sample");
    assert_eq!(view.work.p50_ms, 4.0, "settling frames are not samples");
    assert_eq!(view.script_complete, Some(true));

    profiler.record(frame(99.0, 0.0));
    assert_eq!(
        profiler.snapshot(true).frames,
        3,
        "nothing after the finish"
    );
    assert_eq!(profiler.snapshot(false).script_complete, None);
}

#[test]
fn time_spent_settling_never_counts_as_a_frame_interval() {
    let mut profiler = FrameProfiler::new(true);
    let mut camera = camera();
    let zoom = FrameScript {
        settle_frames: 1,
        motion: aftermine_protocol::CameraMotion::Zoom {
            center: [20.0, 20.0],
            from: 32.0,
            to: 32.0,
            repeats: 2,
            rest_frames: 1,
        },
    };
    profiler.begin_script(&zoom).expect("enabled");
    while profiler.scripted() {
        profiler.step_script(&mut camera);
        if profiler
            .script
            .as_ref()
            .is_some_and(|script| !script.measuring)
        {
            std::thread::sleep(Duration::from_millis(30));
        }
        profiler.record(frame(1.0, 0.0));
    }
    let view = profiler.snapshot(false);
    assert_eq!(view.frames, 2);
    assert_eq!(
        view.interval.max_ms, 0.0,
        "one sample per repeat, no intervals"
    );
}

#[test]
fn a_disabled_profiler_refuses_scripts() {
    let mut profiler = FrameProfiler::new(false);
    let refusal = profiler.begin_script(&script(0, 1)).unwrap_err();
    assert!(refusal.contains("disabled"));
    assert!(!profiler.scripted());
}

#[test]
fn arming_a_window_ends_a_script() {
    let mut profiler = FrameProfiler::new(true);
    profiler.begin_script(&script(0, 1)).expect("enabled");
    profiler.arm(0, 10).expect("window");
    assert!(!profiler.scripted());
    assert_eq!(profiler.snapshot(false).script_complete, None);
}
