use super::*;

#[test]
fn graph_ceiling_uses_readable_steps() {
    for (value, expected) in [
        (0, 1),
        (1, 1),
        (2, 2),
        (3, 3),
        (7, 8),
        (11, 20),
        (52, 60),
        (501, 600),
        (27_954, 30_000),
    ] {
        assert_eq!(graph_ceiling(value), expected);
    }
}

#[test]
fn graph_uses_sample_time_and_keeps_every_segment() {
    let plot = Rect::new(10.0, 20.0, 100.0, 50.0);
    let points = graph_points(&[0, 41, 82, 100], &[0, 10, 40, 100], 100, 100, plot);
    assert_eq!(points.len(), 4);
    assert_eq!(points.windows(2).count(), 3);
    assert!((points[1].x - 51.0).abs() < f32::EPSILON);
    assert!((points[2].x - 92.0).abs() < f32::EPSILON);
    assert_eq!(points[3], vec2(110.0, 20.0));
}

fn player(seat: u8, value_lost: Vec<u32>) -> PlayerStats {
    let samples = value_lost.len();
    PlayerStats {
        seat,
        scrap: vec![0; samples],
        army_value: vec![0; samples],
        wealth: vec![0; samples],
        scrap_earned: vec![0; samples],
        value_lost,
        kinds: vec![std::collections::BTreeMap::new(); samples],
        scrap_collected: 0,
        scrap_earned_total: 0,
        value_lost_total: 0,
        units_trained: 0,
        units_built: std::collections::BTreeMap::new(),
        buildings_completed: 0,
        structures_built: std::collections::BTreeMap::new(),
        units_lost: 0,
        buildings_lost: 0,
        buildings_salvaged: 0,
        units_retired: 0,
    }
}

fn report(ticks: Vec<u64>, players: Vec<PlayerStats>, moments: Vec<Moment>) -> MatchStats {
    let final_tick = ticks.last().copied().unwrap_or(0);
    MatchStats {
        sample_ticks: ticks,
        players,
        final_tick,
        moments,
    }
}

const PLOT: Rect = Rect {
    x: 100.0,
    y: 50.0,
    w: 1000.0,
    h: 200.0,
};

fn layout() -> ChartLayout {
    ChartLayout {
        panel: PLOT,
        strip: crate::tabs::rects_in(
            ChartView::ALL.len(),
            Rect::new(100.0, 0.0, 1000.0, 44.0),
            130.0,
        ),
        plot: PLOT,
        body: PLOT,
        axis_label: None,
    }
}

#[test]
fn income_reads_per_minute_across_uneven_columns() {
    // 60 scrap over one minute, then 60 over thirty seconds.
    let minute = TICKS_PER_MINUTE;
    assert_eq!(
        income_per_minute(&[0, minute, minute + minute / 2], &[0, 60, 120]),
        [0, 60, 120]
    );
    assert_eq!(income_per_minute(&[], &[]), Vec::<u32>::new());
}

#[test]
fn income_survives_a_match_ending_just_after_a_sample() {
    let half = TICKS_PER_MINUTE / 2;
    // A steady 60 a minute, and a final column three ticks long with no
    // delivery in it.
    let ticks = [0, half, half * 2, half * 2 + 3];
    let income = income_per_minute(&ticks, &[0, 30, 60, 60]);
    assert_eq!(income[2], 60);
    assert!((55..=60).contains(&income[3]), "{income:?}");
}

#[test]
fn income_is_the_only_rate() {
    for view in ChartView::ALL {
        let rate = view == ChartView::Income;
        assert_eq!(view.axis_label() == "SCRAP PER MINUTE", rate, "{view:?}");
        assert_eq!(view.unit() == "scrap/min", rate, "{view:?}");
    }
}

#[test]
fn the_scale_labels_keep_clear_of_the_panel_edge() {
    let label_size = crate::theme::Type::Body.px();
    let panel = Rect::new(32.0, 300.0, 1200.0, 400.0);
    let parts = chart_layout(panel, 44.0, label_size, 1.0);
    // "100,000" is seven characters, at most about 0.65 em each in this face.
    assert!(parts.plot.x - panel.x >= 7.0 * 0.65 * label_size);
    assert!(parts.plot.x + parts.plot.w < panel.x + panel.w);
    assert!(parts.axis_label.is_some(), "a tall panel names the unit");
    let short = chart_layout(Rect::new(32.0, 230.0, 576.0, 90.0), 24.0, 13.0, 1.0);
    assert_eq!(
        short.axis_label, None,
        "a short one keeps the room for the plot"
    );
    assert!(short.plot.h > 20.0);
}

#[test]
fn each_view_draws_its_own_series() {
    let mut seat = player(0, vec![1, 2]);
    seat.wealth = vec![10, 20];
    seat.army_value = vec![30, 40];
    seat.scrap_earned = vec![0, 60];
    let ticks = [0, TICKS_PER_MINUTE];
    assert_eq!(ChartView::Wealth.series(&ticks, &seat), [10, 20]);
    assert_eq!(ChartView::Army.series(&ticks, &seat), [30, 40]);
    assert_eq!(ChartView::Income.series(&ticks, &seat), [0, 60]);
    assert_eq!(ChartView::Losses.series(&ticks, &seat), [1, 2]);
    assert!(ChartView::Built.series(&ticks, &seat).is_empty());
}

#[test]
fn heavy_fighting_joins_adjacent_columns_and_skips_trickles() {
    let minute = TICKS_PER_MINUTE;
    let stats = report(
        vec![0, minute, minute * 2, minute * 3, minute * 4, minute * 14],
        vec![
            // A hard fight across two columns, a quiet one, a small loss,
            // then a floor's worth stretched over ten minutes.
            player(0, vec![0, 300, 600, 600, 650, 650]),
            player(1, vec![0, 100, 400, 400, 400, 650]),
        ],
        Vec::new(),
    );
    assert_eq!(fighting_spans(&stats), [(0, minute * 2)]);
}

#[test]
fn a_match_that_fights_throughout_still_marks_its_worst_stretches() {
    let minute = TICKS_PER_MINUTE;
    let stats = report(
        vec![0, minute, minute * 2, minute * 3, minute * 4],
        vec![player(0, vec![0, 400, 800, 3_000, 3_400])],
        Vec::new(),
    );
    assert_eq!(fighting_spans(&stats), [(minute * 2, minute * 3)]);
}

#[test]
fn a_press_opens_the_moment_under_it_or_just_before_a_turning_point() {
    let stats = report(
        vec![0, 10_000],
        vec![player(0, vec![0, 0])],
        vec![Moment {
            tick: 5_000,
            seat: 0,
            kind: MomentKind::Expansion,
        }],
    );
    assert_eq!(press_tick(&stats, PLOT, 100.0, 1.0), 0);
    assert_eq!(press_tick(&stats, PLOT, 1100.0, 1.0), 10_000);
    assert_eq!(press_tick(&stats, PLOT, 300.0, 1.0), 2_000);
    assert_eq!(
        press_tick(&stats, PLOT, 605.0, 1.0),
        5_000 - LEAD_IN_TICKS,
        "near a turning point, the replay starts on its lead-in"
    );
}

#[test]
fn a_lead_in_never_runs_before_the_match() {
    let stats = report(
        vec![0, 10_000],
        vec![player(0, vec![0, 0])],
        vec![Moment {
            tick: 5,
            seat: 0,
            kind: MomentKind::FirstLoss,
        }],
    );
    assert_eq!(press_tick(&stats, PLOT, 100.0, 1.0), 0);
}

#[test]
fn crowded_turning_points_step_down_rather_than_overlap() {
    let moments: Vec<Moment> = (0..4)
        .map(|index| Moment {
            tick: 1_000 + index,
            seat: 0,
            kind: MomentKind::Expansion,
        })
        .collect();
    let marks = mark_positions(&moments, 10_000, PLOT, 1.0);
    assert!(marks[0].y < marks[1].y && marks[1].y < marks[2].y);
    assert_eq!(
        marks[3].y, marks[2].y,
        "the rows run out, and the last share"
    );
    let apart = mark_positions(
        &[
            moments[0],
            Moment {
                tick: 9_000,
                ..moments[0]
            },
        ],
        10_000,
        PLOT,
        1.0,
    );
    assert_eq!(apart[0].y, apart[1].y, "room to spare stays on one row");
}

#[test]
fn turning_points_read_as_plain_sentences() {
    let moment = |kind| Moment {
        tick: 0,
        seat: 0,
        kind,
    };
    for (kind, text) in [
        (MomentKind::FirstLoss, "First loss: Ada"),
        (MomentKind::Expansion, "Ada expanded"),
        (
            MomentKind::TechStructure(aftermine_sim::BuildingKind::Airworks),
            "First Airworks: Ada",
        ),
        (
            MomentKind::TechUnit(aftermine_sim::UnitKind::Roc),
            "First Roc: Ada",
        ),
        (MomentKind::FoundryLost, "Ada lost a Foundry"),
    ] {
        assert_eq!(moment_label(&moment(kind), "Ada"), text);
    }
}

#[test]
fn tabs_cycle_with_keys_and_wrap_both_ways() {
    let stats = report(vec![0, 100], vec![player(0, vec![0, 0])], Vec::new());
    let mut chart = Chart::default();
    let mut sounds = Vec::new();
    assert_eq!(chart.view(), ChartView::Wealth);
    let key = |key| [RawEvent::KeyDown { key }];
    let (_, rest) = chart.route(&key(Key::E), &stats, &layout(), 1.0, &mut sounds);
    assert!(rest.is_empty(), "the chart keeps its keys");
    assert_eq!(chart.view(), ChartView::Army);
    chart.route(&key(Key::Q), &stats, &layout(), 1.0, &mut sounds);
    chart.route(&key(Key::Q), &stats, &layout(), 1.0, &mut sounds);
    assert_eq!(chart.view(), ChartView::Built, "Q wraps left");
    chart.route(&key(Key::E), &stats, &layout(), 1.0, &mut sounds);
    assert_eq!(chart.view(), ChartView::Wealth, "E wraps right");
}

#[test]
fn a_tab_press_picks_its_view() {
    let stats = report(vec![0, 100], vec![player(0, vec![0, 0])], Vec::new());
    let parts = layout();
    let at = parts.strip[2].center();
    let mut chart = Chart::default();
    let press = [
        RawEvent::TouchDown {
            id: 1,
            x: at.x,
            y: at.y,
        },
        RawEvent::TouchUp {
            id: 1,
            x: at.x,
            y: at.y,
        },
    ];
    chart.route(&press, &stats, &parts, 1.0, &mut Vec::new());
    assert_eq!(chart.view(), ChartView::Income);
}

#[test]
fn a_finger_reads_the_plot_lifts_to_open_and_slides_off_to_cancel() {
    let stats = report(vec![0, 10_000], vec![player(0, vec![0, 0])], Vec::new());
    let parts = layout();
    let mut chart = Chart::default();
    let mut sounds = Vec::new();
    let down = RawEvent::TouchDown {
        id: 3,
        x: 300.0,
        y: 100.0,
    };
    let (open, rest) = chart.route(&[down], &stats, &parts, 1.0, &mut sounds);
    assert_eq!(open, None);
    assert!(rest.is_empty(), "the plot owns the finger");
    assert_eq!(chart.pointer, Some(vec2(300.0, 100.0)), "resting reads");

    let up = RawEvent::TouchUp {
        id: 3,
        x: 300.0,
        y: 100.0,
    };
    let (open, _) = chart.route(&[up], &stats, &parts, 1.0, &mut sounds);
    assert_eq!(open, Some(2_000), "lifting opens that moment");
    assert_eq!(chart.pointer, None);

    chart.route(&[down], &stats, &parts, 1.0, &mut sounds);
    let off = RawEvent::TouchUp {
        id: 3,
        x: 300.0,
        y: 600.0,
    };
    let (open, _) = chart.route(&[off], &stats, &parts, 1.0, &mut sounds);
    assert_eq!(open, None, "sliding off the plot cancels");
    assert_eq!(chart.pointer, None);
}

#[test]
fn the_mouse_reads_on_hover_and_opens_on_a_click() {
    let stats = report(vec![0, 10_000], vec![player(0, vec![0, 0])], Vec::new());
    let parts = layout();
    let mut chart = Chart::default();
    let mut sounds = Vec::new();
    let moved = RawEvent::MouseMove { x: 300.0, y: 100.0 };
    let (_, rest) = chart.route(&[moved], &stats, &parts, 1.0, &mut sounds);
    assert_eq!(rest, [moved], "the screen still hears the mouse move");
    assert_eq!(chart.pointer, Some(vec2(300.0, 100.0)));
    let left = aftermine_protocol::MouseButton::Left;
    let press = [
        RawEvent::MouseDown {
            button: left,
            x: 300.0,
            y: 100.0,
        },
        RawEvent::MouseUp {
            button: left,
            x: 300.0,
            y: 100.0,
        },
    ];
    let (open, _) = chart.route(&press, &stats, &parts, 1.0, &mut sounds);
    assert_eq!(open, Some(2_000));
}

#[test]
fn the_built_tab_has_no_plot_to_press() {
    let stats = report(vec![0, 10_000], vec![player(0, vec![0, 0])], Vec::new());
    let parts = layout();
    let mut chart = Chart {
        view: 4,
        ..Chart::default()
    };
    let at = RawEvent::TouchDown {
        id: 1,
        x: 300.0,
        y: 100.0,
    };
    let (open, rest) = chart.route(&[at], &stats, &parts, 1.0, &mut Vec::new());
    assert_eq!(open, None);
    assert_eq!(rest, [at], "the screen hears it instead");
}

#[test]
fn nearest_column_snaps_to_the_closest_sample() {
    assert_eq!(nearest_column(&[0, 100, 200], 140), 1);
    assert_eq!(nearest_column(&[0, 100, 200], 160), 2);
    assert_eq!(nearest_column(&[], 160), 0);
}

#[test]
fn large_numbers_mark_their_thousands() {
    for (value, text) in [
        (0, "0"),
        (999, "999"),
        (1_000, "1,000"),
        (39_545, "39,545"),
        (1_234_567, "1,234,567"),
    ] {
        assert_eq!(numeric::with_commas(value), text);
    }
}
