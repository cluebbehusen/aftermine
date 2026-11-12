use super::*;
use aftermine_protocol::{PassCostView, PassDrawCallsView, PassTimingView, SlowFrameView};

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
        gpu_wait: None,
        draw_calls: Vec::new(),
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
        gpu: false,
        skipped: Vec::new(),
        natural_clocks: false,
        workloads,
    }
}

#[test]
fn every_workload_has_a_unique_name_and_shares_a_shell_with_its_source() {
    let all = workloads();
    assert_eq!(all.len(), 1 + 4 * Shot::MATCH.len() + 1 + 2);
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
    assert_eq!(running[0].source, Source::Match(Scene::Large, View::Fogged));
}

#[test]
fn match_views_hold_near_far_and_the_zooms_between() {
    let names: Vec<String> = Shot::MATCH
        .iter()
        .filter(|shot| matches!(shot, Shot::Hold(_)))
        .map(|shot| shot.name())
        .collect();
    assert_eq!(names, ["near", "hold-20", "hold-13.5", "far"]);
    let between = |zoom: f64| FAR_ZOOM < zoom && zoom < NEAR_ZOOM;
    assert!(between(BLEND_ZOOM) && between(FEATHER_ZOOM));
    const {
        assert!(
            13.0 < FEATHER_ZOOM && FEATHER_ZOOM < 14.0,
            "tiles must alternate between 13 and 14 pixels"
        );
    }
    let holds = select(Some("small-fogged-hold-")).expect("matches");
    assert_eq!(holds.len(), 2);
}

#[test]
fn the_mass_battle_fits_in_view_with_every_unit_standing() {
    let mass = Scene::Mass.scenario(Path::new(".")).expect("generated");
    assert_eq!(mass.units.len(), 2 * MASS_PER_SIDE as usize);
    assert!(
        mass.players.iter().all(|seat| !seat.bot),
        "no seat may order its army away"
    );
    let state = mass.build().expect("builds");
    let width = f64::from(WINDOW[0]) / MASS_ZOOM;
    let height = f64::from(WINDOW[1]) / MASS_ZOOM;
    assert!(
        f64::from(state.map().width()) <= width && f64::from(state.map().height()) <= height,
        "the field must fit a {width:.0}x{height:.0}-tile view"
    );
    let workload = select(Some("mass-fogged-hold-12")).expect("matches");
    assert_eq!(workload[0].name, "mass-fogged-hold-12");
    assert_eq!(
        script(&workload[0], [0.0, 0.0]).motion,
        CameraMotion::Hold {
            center: [0.0, 0.0],
            zoom: MASS_ZOOM,
            frames: HOLD_FRAMES,
        }
    );
}

#[test]
fn a_large_selection_is_set_up_against_an_unselected_control() {
    let mass = select(Some("mass-")).expect("matches");
    let shape: Vec<_> = mass
        .iter()
        .map(|workload| (workload.name.as_str(), workload.setup))
        .collect();
    assert_eq!(
        shape,
        [
            ("mass-fogged-hold-12", Setup::None),
            ("mass-fogged-hold-12-selected", Setup::SelectArmy),
        ]
    );
    assert_eq!(mass[0].shot, mass[1].shot);
    assert!(!mass[0].changes_its_shell());
    assert!(
        mass[1].changes_its_shell(),
        "a selection would leak into the next workload"
    );
    let set_up: Vec<_> = workloads()
        .into_iter()
        .filter(|workload| workload.setup != Setup::None)
        .map(|workload| workload.name)
        .collect();
    assert_eq!(set_up, ["mass-fogged-hold-12-selected"]);
}

#[test]
fn selecting_the_army_presses_the_chord_the_profiling_config_binds() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../quiet_config.json")).expect("parses");
    let bound: Vec<_> = config["bindings"]["bindings"]
        .as_array()
        .expect("bindings")
        .iter()
        .filter(|binding| binding["action"] == "SelectArmy")
        .map(|binding| binding["chord"].clone())
        .collect();
    assert_eq!(
        bound,
        [serde_json::json!({"key": "a", "ctrl": true, "shift": false})]
    );
    let events: Vec<_> = Setup::SelectArmy
        .requests()
        .into_iter()
        .map(|request| match request {
            Request::InjectEvent { event } => event,
            other => panic!("a chord only injects input, not {other:?}"),
        })
        .collect();
    assert_eq!(
        events,
        [
            RawEvent::KeyDown { key: Key::Ctrl },
            RawEvent::KeyDown { key: Key::A },
            RawEvent::KeyUp { key: Key::A },
            RawEvent::KeyUp { key: Key::Ctrl },
        ]
    );
    assert!(Setup::None.requests().is_empty());
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
            setup: Setup::None,
        };
        script(&workload, center).motion
    };
    assert_eq!(
        motion(Shot::Own),
        CameraMotion::None {
            frames: HOLD_FRAMES
        }
    );
    for zoom in [NEAR_ZOOM, BLEND_ZOOM, FEATHER_ZOOM, FAR_ZOOM] {
        assert_eq!(
            motion(Shot::Hold(zoom)),
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
        gpu: false,
        skip: &[],
        natural_clocks: false,
    })
    .unwrap_err();
    assert!(refusal.to_string().contains("--trials"));
}

#[test]
fn draw_calls_come_from_one_plain_trial_only() {
    let skip = ["units".to_string()];
    let options = |gpu, skip| GraphicsOptions {
        only: None,
        trials: 3,
        port: 0,
        dev: true,
        gpu,
        skip,
        natural_clocks: false,
    };
    let plain = options(false, &[]);
    assert!(probes_for_trial(0, &plain).draw_calls);
    assert!(!probes_for_trial(1, &plain).draw_calls);

    let gpu = probes_for_trial(0, &options(true, &[]));
    assert!(
        gpu.gpu_wait && !gpu.draw_calls,
        "a capture would disturb it"
    );

    let skipping = probes_for_trial(0, &options(false, &skip));
    assert_eq!(skipping.skip_passes, skip);
    assert!(!skipping.draw_calls, "counts would miss the skipped pass");
}

#[test]
fn the_report_shows_gpu_wait_and_draw_calls_when_a_run_collected_them() {
    let mut measured = trial(2.0, 1.0, 0);
    measured.gpu_wait = Some(timing(4.5));
    measured.draw_calls = vec![
        PassDrawCallsView {
            name: "units".to_string(),
            draw_calls: 17,
            indices: 1_200,
        },
        PassDrawCallsView {
            name: "menu".to_string(),
            draw_calls: 3,
            indices: 6,
        },
    ];
    let mut run = report(vec![result("near", vec![measured])]);
    run.gpu = true;
    run.skipped = vec!["fog".to_string()];
    let text = render(&run, None);
    assert!(text.contains("waiting for the GPU each frame, without fog"));
    assert!(text.contains("gpu wait"));
    assert!(text.contains("  4.50"), "the wait's mean:\n{text}");
    let units = text
        .lines()
        .find(|line| line.contains("units  "))
        .expect("row");
    assert!(units.contains("17") && units.contains("1200"), "{units}");
    assert!(
        text.contains("menu"),
        "a pass that draws stays listed even when it takes no time:\n{text}"
    );
}

#[test]
fn a_digest_keeps_the_gpu_wait_median_and_finds_draw_calls_in_any_trial() {
    let mut first = trial(2.0, 1.0, 0);
    first.gpu_wait = Some(timing(1.0));
    first.draw_calls = vec![PassDrawCallsView {
        name: "units".to_string(),
        draw_calls: 5,
        indices: 60,
    }];
    let mut second = trial(2.0, 1.0, 0);
    second.gpu_wait = Some(timing(9.0));
    let mut third = trial(2.0, 1.0, 0);
    third.gpu_wait = Some(timing(4.0));
    let digest = digest(&result("near", vec![first, second, third]));
    assert_eq!(digest.gpu_wait.expect("wait").mean_ms, 4.0);
    let units = digest
        .passes
        .iter()
        .find(|row| row.name == "units")
        .expect("row");
    assert_eq!(units.draws, Some((5, 60)));
}

#[test]
fn a_run_nothing_paced_says_so() {
    let paced = report(vec![result("near", vec![trial(2.0, 1.0, 0)])]);
    assert!(!render(&paced, None).contains("WARNING"));

    let mut unpaced = trial(2.0, 1.0, 0);
    unpaced.interval = timing(1.5);
    let text = render(&report(vec![result("near", vec![unpaced])]), None);
    assert!(text.contains("WARNING"), "{text}");
    assert!(text.contains("asleep"), "{text}");
}

#[test]
fn a_trial_missing_what_it_asked_for_is_refused() {
    let wants_draws = FrameProbes {
        draw_calls: true,
        ..FrameProbes::default()
    };
    let mut profile = trial(2.0, 1.0, 0);
    assert!(check_trial(&profile, &FrameProbes::default()).is_ok());
    let refusal = check_trial(&profile, &wants_draws).unwrap_err();
    assert!(refusal.to_string().contains("draw call capture"));

    profile.draw_calls = vec![PassDrawCallsView {
        name: "units".to_string(),
        draw_calls: 1,
        indices: 6,
    }];
    assert!(check_trial(&profile, &wants_draws).is_ok());

    profile.frames = 0;
    let refusal = check_trial(&profile, &FrameProbes::default()).unwrap_err();
    assert!(refusal.to_string().contains("no frames"));
}

#[test]
fn held_clocks_stop_when_dropped() {
    let clocks = HeldClocks::start();
    assert_eq!(clocks.holders.len(), CLOCK_HOLDERS);
    let stop = Arc::clone(&clocks.stop);
    drop(clocks);
    assert!(stop.load(Ordering::Relaxed));
}

#[test]
fn the_report_names_its_clocks_and_warns_on_a_mismatched_comparison() {
    let held = report(vec![result("near", vec![trial(2.0, 1.0, 0)])]);
    assert!(render(&held, None).contains("held clocks"));
    assert!(!render(&held, Some(&held)).contains("WARNING"));

    let mut natural = held.clone();
    natural.natural_clocks = true;
    assert!(render(&natural, None).contains("natural clocks"));
    assert!(render(&natural, Some(&held)).contains("different clocks"));

    let mut waiting = held.clone();
    waiting.gpu = true;
    assert!(render(&waiting, Some(&held)).contains("different clocks or GPU"));
}
