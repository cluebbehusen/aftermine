use super::*;
use aftermine_protocol::{PassCostView, PassTimingView, SlowFrameView};

fn timing(mean_ms: f64) -> TimingSummary {
    TimingSummary {
        mean_ms,
        p50_ms: mean_ms,
        p95_ms: mean_ms + 1.0,
        p99_ms: mean_ms + 2.0,
        max_ms: mean_ms + 3.0,
    }
}

fn trial(units_mean: f64, fog_mean: f64, over_budget: usize) -> FrameProfileView {
    FrameProfileView {
        renderer: "gpu".to_string(),
        frames: 240,
        tick_start: 0,
        tick_end: 0,
        ticks_presented: 0,
        work: timing(units_mean + fog_mean),
        submit: timing(1.0),
        cpu: timing(units_mean + fog_mean + 1.0),
        passes: vec![
            PassTimingView {
                name: "fog".to_string(),
                timing: timing(fog_mean),
            },
            PassTimingView {
                name: "units".to_string(),
                timing: timing(units_mean),
            },
            PassTimingView {
                name: "menu".to_string(),
                timing: TimingSummary {
                    mean_ms: 0.0,
                    p50_ms: 0.0,
                    p95_ms: 0.0,
                    p99_ms: 0.0,
                    max_ms: 0.0,
                },
            },
        ],
        interval: timing(16.7),
        cpu_over_8_3_ms: over_budget,
        work_over_16_7_ms: 0,
        work_over_33_3_ms: 0,
        slowest: Some(SlowFrameView {
            mode: "playing".to_string(),
            tick_start: 0,
            tick_end: 0,
            work_ms: units_mean + fog_mean + 3.0,
            submit_ms: 1.0,
            passes: vec![PassCostView {
                name: "units".to_string(),
                ms: units_mean + 3.0,
            }],
            units: 1,
            buildings: 1,
        }),
        window: None,
        script_complete: Some(true),
    }
}

fn result(name: &str, trials: Vec<FrameProfileView>) -> WorkloadResult {
    WorkloadResult {
        name: name.to_string(),
        units: 300,
        structures: 240,
        final_camera: [1.0, 2.0, 8.0],
        trials,
    }
}

fn report(workloads: Vec<WorkloadResult>) -> GraphicsReport {
    GraphicsReport {
        revision: "abc".to_string(),
        window: WINDOW,
        workloads,
    }
}

#[test]
fn every_workload_has_a_unique_name_and_shares_a_shell_with_its_source() {
    let all = workloads();
    assert_eq!(all.len(), 1 + 4 * Shot::MATCH.len() + 1);
    let names: std::collections::BTreeSet<_> = all.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names.len(), all.len());
    let mut seen = Vec::new();
    for workload in &all {
        if seen.last() != Some(&workload.source) {
            assert!(!seen.contains(&workload.source), "{} split", workload.name);
            seen.push(workload.source);
        }
    }
    let running: Vec<_> = all.iter().filter(|w| w.running).collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].name, "large-fogged-running");
    assert_eq!(running[0].source, Source::Match(Size::Large, View::Fogged));
}

#[test]
fn only_keeps_matching_workloads_and_refuses_an_empty_selection() {
    let zooms = select(Some("zoom-in")).expect("matches");
    assert_eq!(zooms.len(), 4);
    assert_eq!(select(None).expect("all").len(), workloads().len());
    let refusal = select(Some("nothing")).unwrap_err();
    assert!(refusal.to_string().contains("nothing"));
}

#[test]
fn shots_become_the_camera_motions_they_name() {
    let center = [5.0, 6.0];
    let motion = |shot| {
        let workload = Workload {
            name: String::new(),
            source: Source::Home,
            shot,
            running: false,
        };
        script(&workload, center).motion
    };
    assert_eq!(
        motion(Shot::Own),
        CameraMotion::None {
            frames: HOLD_FRAMES
        }
    );
    for (shot, zoom) in [(Shot::Near, NEAR_ZOOM), (Shot::Far, FAR_ZOOM)] {
        assert_eq!(
            motion(shot),
            CameraMotion::Hold {
                center,
                zoom,
                frames: HOLD_FRAMES
            }
        );
    }
    assert!(matches!(
        motion(Shot::ZoomOut),
        CameraMotion::Zoom {
            from: NEAR_ZOOM,
            to: FAR_ZOOM,
            ..
        }
    ));
    assert!(matches!(
        motion(Shot::ZoomIn),
        CameraMotion::Zoom {
            from: FAR_ZOOM,
            to: NEAR_ZOOM,
            ..
        }
    ));
    assert!(matches!(
        motion(Shot::Pan),
        CameraMotion::Pan {
            frames: PAN_FRAMES,
            ..
        }
    ));
}

#[test]
fn only_the_running_workload_plays_the_match() {
    for workload in workloads() {
        assert_eq!(
            script(&workload, [0.0, 0.0]).plays,
            workload.name.ends_with("-running"),
            "{}",
            workload.name
        );
    }
}

#[test]
fn the_camera_starts_on_seat_zeros_own_forces() {
    let state = aftermine_sim::Scenario::skirmish().build().expect("builds");
    let view = StateView::capture(&state, StateFilter::default());
    let focus = seat_focus(&view).expect("seat zero has forces");

    let mut without_rivals = view.clone();
    without_rivals.units.retain(|unit| unit.player == 0);
    without_rivals
        .buildings
        .retain(|building| building.player == 0);
    assert_eq!(
        seat_focus(&without_rivals),
        Some(focus),
        "rivals don't count"
    );

    let mut without_seat = view;
    without_seat.units.retain(|unit| unit.player != 0);
    without_seat
        .buildings
        .retain(|building| building.player != 0);
    assert_eq!(seat_focus(&without_seat), None);
}

#[test]
fn a_digest_takes_each_statistics_median_across_trials() {
    let digest = digest(&result(
        "large",
        vec![trial(2.0, 1.0, 5), trial(9.0, 1.0, 50), trial(3.0, 1.0, 7)],
    ));
    assert_eq!(digest.frames, 240.0);
    assert_eq!(digest.over_budget, 7.0);
    assert_eq!(digest.passes[0].name, "units", "costliest first");
    assert_eq!(digest.passes[0].timing.mean_ms, 3.0);
    assert_eq!(digest.cpu.mean_ms, 5.0);
    assert_eq!(
        digest.passes[0].worst_frame_ms, 12.0,
        "worst frame comes from the slowest trial"
    );
    assert_eq!(digest.passes[1].worst_frame_ms, 0.0, "fog had no entry");
}

#[test]
fn the_report_lists_workloads_and_hides_passes_that_cost_nothing() {
    let text = render(
        &report(vec![result("large-fogged-near", vec![trial(2.0, 1.0, 5)])]),
        None,
    );
    assert!(text.contains("revision abc, window 1210x834"));
    assert!(text.contains("large-fogged-near: 300 units, 240 structures, 240 frames, 5 over 8.33"));
    assert!(text.contains("units"));
    assert!(!text.contains("menu"), "zero-cost passes stay out:\n{text}");
    assert!(
        !text.contains('('),
        "no changes without a baseline:\n{text}"
    );
}

#[test]
fn a_baseline_adds_each_means_change_for_workloads_it_ran() {
    let before = report(vec![result("near", vec![trial(4.0, 1.0, 5)])]);
    let after = report(vec![
        result("near", vec![trial(2.5, 1.0, 5)]),
        result("new-one", vec![trial(1.0, 1.0, 0)]),
    ]);
    let text = render(&after, Some(&before));
    assert!(text.contains("(-1.50)"), "means improved:\n{text}");
    let new_section = text.split("new-one:").nth(1).expect("detail section");
    assert!(!new_section.contains('('), "nothing to compare:\n{text}");
}

#[test]
fn a_report_survives_being_written_and_compared_later() {
    let before = report(vec![result("near", vec![trial(4.0, 1.0, 5)])]);
    let text = serde_json::to_string(&before).expect("serializes");
    let back: GraphicsReport = serde_json::from_str(&text).expect("parses");
    assert_eq!(back, before);
}

#[test]
fn trials_of_zero_are_refused_before_any_shell_starts() {
    let refusal = run(&GraphicsOptions {
        only: None,
        trials: 0,
        port: 0,
        dev: true,
    })
    .unwrap_err();
    assert!(refusal.to_string().contains("--trials"));
}
