//! The report's chart panel: a tab per view of the match, the series for
//! each, its turning points, and a press that opens the replay at that moment.

use super::draw_marker;
use crate::game::{Game, SoundKind};
use crate::numeric;
use crate::press::{Fed, Press};
use crate::tabs::TabStrip;
use crate::typography::entity_name;
use crate::{render, theme};
use aftermine_kit::stats::{MatchStats, Moment, MomentKind, PlayerStats};
use aftermine_protocol::{Key, RawEvent};
use aftermine_sim::{PlayerId, TICKS_PER_SECOND};
use macroquad::prelude::*;

/// What the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChartView {
    Wealth,
    Army,
    Income,
    Losses,
    Built,
}

impl ChartView {
    const ALL: [Self; 5] = [
        Self::Wealth,
        Self::Army,
        Self::Income,
        Self::Losses,
        Self::Built,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Wealth => "Total wealth",
            Self::Army => "Army",
            Self::Income => "Income",
            Self::Losses => "Losses",
            Self::Built => "Built",
        }
    }

    /// What the numbers on this view are counted in, over the scale.
    const fn axis_label(self) -> &'static str {
        match self {
            Self::Income => "SCRAP PER MINUTE",
            _ => "SCRAP",
        }
    }

    /// The unit a readout's numbers carry.
    const fn unit(self) -> &'static str {
        match self {
            Self::Income => "scrap/min",
            _ => "scrap",
        }
    }

    /// The seat's points on this view, one per sample column. The Built tab
    /// has no series.
    fn series(self, ticks: &[u64], player: &PlayerStats) -> Vec<u32> {
        match self {
            Self::Wealth => player.wealth.clone(),
            Self::Army => player.army_value.clone(),
            Self::Income => income_per_minute(ticks, &player.scrap_earned),
            Self::Losses => player.value_lost.clone(),
            Self::Built => Vec::new(),
        }
    }
}

/// Scrap a column must lose to count as heavy fighting, whatever its length.
const FIGHTING_FLOOR: u32 = 200;

/// Scrap a minute the match must lose to count as heavy fighting, so a floor's
/// worth spread across one long column of a long match doesn't.
const FIGHTING_PER_MINUTE: u64 = 300;

/// The share of the match's worst loss rate a column must reach, in percent.
const FIGHTING_SHARE_OF_PEAK: u64 = 50;

/// How far before a turning point its replay starts, so the lead-in shows.
const LEAD_IN_TICKS: u64 = 10 * TICKS_PER_SECOND as u64;

/// How near a press must land on a turning point to snap to it, in logical px.
const SNAP: f32 = 12.0;

/// The panel height below the strip that can spare a row for the scale's unit,
/// in logical px.
const AXIS_LABEL_ROOM: f32 = 120.0;

/// The widest scale label, "100,000", in ems, with room to clear the panel edge.
const GUTTER_EMS: f32 = 4.8;

/// The vertical step between turning points that would overlap, in logical px.
const MARK_ROW: f32 = 11.0;

const TICKS_PER_MINUTE: u64 = 60 * TICKS_PER_SECOND as u64;

/// Scrap per minute at each sample, over the minute behind it, from a running
/// total. A final column can be only ticks long, which would make its own rate
/// read zero or spike. The first sample reads zero.
fn income_per_minute(ticks: &[u64], earned: &[u32]) -> Vec<u32> {
    (0..earned.len().min(ticks.len()))
        .map(|sample| {
            let Some(mut start) = sample.checked_sub(1) else {
                return 0;
            };
            while start > 0 && ticks[sample] - ticks[start - 1] <= TICKS_PER_MINUTE {
                start -= 1;
            }
            let scrap = u64::from(earned[sample].saturating_sub(earned[start]));
            let elapsed = ticks[sample].saturating_sub(ticks[start]).max(1);
            u32::try_from(scrap * TICKS_PER_MINUTE / elapsed).unwrap_or(u32::MAX)
        })
        .collect()
}

/// The spans of heavy fighting: runs of sample columns where the whole match
/// lost close to its worst rate. Derived from the loss series, so a saved report
/// stores nothing extra.
fn fighting_spans(report: &MatchStats) -> Vec<(u64, u64)> {
    let columns: Vec<(u64, u64, u32, u64)> = (1..report.sample_ticks.len())
        .map(|column| {
            let lost: u32 = report
                .players
                .iter()
                .map(|player| {
                    player.value_lost[column].saturating_sub(player.value_lost[column - 1])
                })
                .fold(0, u32::saturating_add);
            let (from, to) = (report.sample_ticks[column - 1], report.sample_ticks[column]);
            let per_minute = u64::from(lost) * TICKS_PER_MINUTE / to.saturating_sub(from).max(1);
            (from, to, lost, per_minute)
        })
        .collect();
    let peak = columns.iter().map(|column| column.3).max().unwrap_or(0);
    let bar = FIGHTING_PER_MINUTE.max(peak * FIGHTING_SHARE_OF_PEAK / 100);
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for (from, to, lost, per_minute) in columns {
        if lost < FIGHTING_FLOOR || per_minute < bar {
            continue;
        }
        match spans.last_mut() {
            Some((_, end)) if *end == from => *end = to,
            _ => spans.push((from, to)),
        }
    }
    spans
}

/// The rounded-up top of a chart's scale.
pub(super) fn graph_ceiling(max_value: u32) -> u32 {
    let max_value = max_value.max(1);
    let magnitude = 10u32.pow(max_value.ilog10());
    [1, 2, 3, 4, 5, 6, 8, 10]
        .into_iter()
        .map(|multiple| magnitude.saturating_mul(multiple))
        .find(|candidate| *candidate >= max_value)
        .unwrap_or(u32::MAX)
}

fn x_of(tick: u64, final_tick: u64, plot: Rect) -> f32 {
    let final_tick = final_tick.max(1);
    plot.x + plot.w * tick.min(final_tick) as f32 / final_tick as f32
}

pub(super) fn graph_points(
    sample_ticks: &[u64],
    values: &[u32],
    final_tick: u64,
    ceiling: u32,
    plot: Rect,
) -> Vec<Vec2> {
    let ceiling = ceiling.max(1);
    sample_ticks
        .iter()
        .copied()
        .zip(values.iter().copied())
        .map(|(tick, value)| {
            vec2(
                x_of(tick, final_tick, plot),
                plot.y + plot.h - plot.h * value.min(ceiling) as f32 / ceiling as f32,
            )
        })
        .collect()
}

/// Where each turning point draws along the plot's top edge. Neighbours too
/// close to read step down a row, up to three rows.
fn mark_positions(moments: &[Moment], final_tick: u64, plot: Rect, s: f32) -> Vec<Vec2> {
    let mut placed: Vec<Vec2> = Vec::with_capacity(moments.len());
    for moment in moments {
        let x = x_of(moment.tick, final_tick, plot);
        let taken = |row: usize| {
            placed.iter().any(|mark| {
                (mark.x - x).abs() < MARK_ROW * s
                    && (mark.y - (plot.y + (row as f32 + 0.6) * MARK_ROW * s)).abs() < 1.0
            })
        };
        let row = (0..3).find(|row| !taken(*row)).unwrap_or(2);
        placed.push(vec2(x, plot.y + (row as f32 + 0.6) * MARK_ROW * s));
    }
    placed
}

/// The tick a press at `x` opens the replay on: just before a turning point or
/// a stretch of heavy fighting when it lands near one, else that moment.
fn press_tick(report: &MatchStats, plot: Rect, x: f32, s: f32) -> u64 {
    let final_tick = report.final_tick;
    let starts = report
        .moments
        .iter()
        .map(|moment| moment.tick)
        .chain(fighting_spans(report).into_iter().map(|(from, _)| from));
    let snapped = starts
        .map(|tick| (tick, (x_of(tick, final_tick, plot) - x).abs()))
        .filter(|(_, distance)| *distance <= SNAP * s)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((tick, _)) = snapped {
        return tick.saturating_sub(LEAD_IN_TICKS);
    }
    tick_at(x, final_tick, plot)
}

/// The tick under `x` on the plot's time axis.
fn tick_at(x: f32, final_tick: u64, plot: Rect) -> u64 {
    let along = ((x - plot.x) / plot.w.max(1.0)).clamp(0.0, 1.0);
    numeric::to_u64((along * final_tick as f32).round()).min(final_tick)
}

/// The sample column nearest `tick`.
fn nearest_column(ticks: &[u64], tick: u64) -> usize {
    (0..ticks.len())
        .min_by_key(|column| ticks[*column].abs_diff(tick))
        .unwrap_or(0)
}

/// A turning point in words.
fn moment_label(moment: &Moment, name: &str) -> String {
    match moment.kind {
        MomentKind::FirstLoss => format!("First loss: {name}"),
        MomentKind::Expansion => format!("{name} expanded"),
        MomentKind::TechStructure(kind) => {
            format!("First {}: {name}", entity_name(kind.name()))
        }
        MomentKind::TechUnit(kind) => format!("First {}: {name}", entity_name(kind.name())),
        MomentKind::FoundryLost => format!("{name} lost a Foundry"),
    }
}

/// Where the panel's parts sit.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ChartLayout {
    pub(super) panel: Rect,
    pub(super) strip: Vec<Rect>,
    pub(super) plot: Rect,
    pub(super) body: Rect,
    /// Where the scale's unit starts, if it fits.
    pub(super) axis_label: Option<Vec2>,
}

/// Lays out the panel. `strip_height` is a fingertip wherever fingers can
/// reach; only a short desktop window gets less. The plot's left gutter holds
/// the widest scale label at `label_size`.
pub(super) fn chart_layout(panel: Rect, strip_height: f32, label_size: f32, s: f32) -> ChartLayout {
    let inset = 6.0 * s;
    let strip_area = Rect::new(
        panel.x + inset,
        panel.y + inset,
        panel.w - 2.0 * inset,
        strip_height,
    );
    let strip = crate::tabs::rects_in(ChartView::ALL.len(), strip_area, 130.0 * s);
    let top = strip_area.y + strip_height + 6.0 * s;
    let body = Rect::new(
        panel.x + inset,
        top,
        panel.w - 2.0 * inset,
        (panel.y + panel.h - top - inset).max(1.0),
    );
    let room = panel.y + panel.h - top;
    let spare = room >= AXIS_LABEL_ROOM * s;
    let plot_top = top + if spare { 20.0 } else { 2.0 } * s;
    let left = (label_size * GUTTER_EMS).min(panel.w * 0.2);
    let axis_label = spare.then(|| vec2(panel.x + left, top + 12.0 * s));
    let plot = Rect::new(
        panel.x + left,
        plot_top,
        (panel.w - left - 12.0 * s).max(1.0),
        (panel.y + panel.h - plot_top - 22.0 * s).max(1.0),
    );
    ChartLayout {
        panel,
        strip,
        plot,
        body,
        axis_label,
    }
}

/// The panel's view, tab strip and pointer.
#[derive(Default)]
pub(super) struct Chart {
    view: usize,
    tabs: TabStrip,
    press: Press<()>,
    /// Where the readout follows the mouse or a resting finger.
    pointer: Option<Vec2>,
}

impl Chart {
    pub(super) fn view(&self) -> ChartView {
        ChartView::ALL[self.view]
    }

    fn step(&mut self, delta: isize, sounds: &mut Vec<(SoundKind, Option<Vec2>)>) {
        let count = ChartView::ALL.len().cast_signed();
        self.view = (self.view.cast_signed() + delta)
            .rem_euclid(count)
            .cast_unsigned();
        self.press.cancel();
        self.pointer = None;
        sounds.push((SoundKind::Click, None));
    }

    /// Takes the chart's share of a frame's events first, as the tab strip
    /// and Back button do, and returns the tick a press opened the replay on
    /// and the events left for the screen.
    pub(super) fn route(
        &mut self,
        events: &[RawEvent],
        report: &MatchStats,
        layout: &ChartLayout,
        s: f32,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> (Option<u64>, Vec<RawEvent>) {
        let (tab, events) = self.tabs.route_in(events, &layout.strip);
        if let Some(tab) = tab {
            self.view = tab;
            self.press.cancel();
            self.pointer = None;
            sounds.push((SoundKind::Click, None));
        }
        let charted = self.view() != ChartView::Built;
        let plot = layout.plot;
        let mut rest = Vec::with_capacity(events.len());
        for event in &events {
            match *event {
                RawEvent::KeyDown { key: Key::Q } => {
                    self.step(-1, sounds);
                    continue;
                }
                RawEvent::KeyDown { key: Key::E } => {
                    self.step(1, sounds);
                    continue;
                }
                RawEvent::MouseMove { x, y } => {
                    let at = vec2(x, y);
                    self.pointer = (charted && plot.contains(at)).then_some(at);
                }
                _ => {}
            }
            if !charted {
                rest.push(*event);
                continue;
            }
            match self
                .press
                .feed(event, |at, _| plot.contains(at).then_some(()))
            {
                Fed::Activated(()) => {
                    self.pointer = None;
                    let x = crate::press::position(event).map_or(plot.x, |at| at.x);
                    sounds.push((SoundKind::Click, None));
                    return (Some(press_tick(report, plot, x, s)), Vec::new());
                }
                Fed::Held => match *event {
                    RawEvent::TouchDown { x, y, .. } | RawEvent::TouchMove { x, y, .. } => {
                        let at = vec2(x, y);
                        self.pointer = plot.contains(at).then_some(at);
                    }
                    RawEvent::TouchUp { .. } | RawEvent::TouchCancel { .. } => self.pointer = None,
                    _ => {}
                },
                Fed::Ignored => rest.push(*event),
            }
        }
        (None, rest)
    }

    /// Draws the panel.
    pub(super) fn draw(
        &self,
        game: &Game,
        sprites: &crate::assets::Sprites,
        report: &MatchStats,
        layout: &ChartLayout,
        s: f32,
        label_size: f32,
    ) {
        let panel = layout.panel;
        draw_rectangle(panel.x, panel.y, panel.w, panel.h, theme::SURFACE_PANEL);
        draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, s, theme::TEXT_DISABLED);
        let labels = ChartView::ALL.map(ChartView::label);
        let keys = crate::tabs::keys();
        let mouse: Vec2 = mouse_position().into();
        crate::tabs::draw_in(
            &layout.strip,
            &labels,
            self.view,
            mouse,
            keys.as_ref()
                .map(|(prev, next)| (prev.as_str(), next.as_str())),
        );
        if self.view() == ChartView::Built {
            super::built::draw(game, sprites, report, layout.body, s);
        } else {
            if let Some(at) = layout.axis_label {
                draw_text(
                    self.view().axis_label(),
                    at.x,
                    at.y,
                    theme::Type::Small.at(s),
                    theme::TEXT_SECONDARY,
                );
            }
            draw_plot(game, report, self.view(), layout.plot, s, label_size);
            if let Some(pointer) = self.pointer {
                draw_readout(game, report, self.view(), layout.plot, pointer, s);
            }
        }
    }
}

fn draw_plot(
    game: &Game,
    report: &MatchStats,
    view: ChartView,
    plot: Rect,
    s: f32,
    label_size: f32,
) {
    let series: Vec<Vec<u32>> = report
        .players
        .iter()
        .map(|player| view.series(&report.sample_ticks, player))
        .collect();
    let max_value = series.iter().flatten().copied().max().unwrap_or(1).max(1);
    let ceiling = graph_ceiling(max_value);
    for value in [ceiling, ceiling / 2, 0] {
        let y = plot.y + plot.h - plot.h * value as f32 / ceiling as f32;
        draw_line(
            plot.x,
            y,
            plot.x + plot.w,
            y,
            s,
            Color::new(0.55, 0.55, 0.62, 0.18),
        );
        let label = numeric::with_commas(value);
        let dims = measure_text(&label, None, numeric::font_size(label_size), 1.0);
        draw_text(
            &label,
            plot.x - dims.width - 5.0 * s,
            y + 4.0 * s,
            label_size,
            theme::TEXT_BODY,
        );
    }
    let mut x_ticks = vec![0, report.final_tick / 2, report.final_tick];
    x_ticks.dedup();
    for tick in x_ticks {
        let x = x_of(tick, report.final_tick, plot);
        draw_line(
            x,
            plot.y,
            x,
            plot.y + plot.h,
            s,
            Color::new(0.55, 0.55, 0.62, 0.13),
        );
        let label = crate::clock_label::ticks(tick);
        let dims = measure_text(&label, None, numeric::font_size(label_size), 1.0);
        let label_x = (x - dims.width * 0.5).clamp(plot.x, plot.x + plot.w - dims.width);
        draw_text(
            &label,
            label_x,
            plot.y + plot.h + 15.0 * s,
            label_size,
            theme::TEXT_BODY,
        );
    }
    for (from, to) in fighting_spans(report) {
        let left = x_of(from, report.final_tick, plot);
        let right = x_of(to, report.final_tick, plot);
        draw_rectangle(
            left,
            plot.y,
            (right - left).max(s),
            plot.h,
            Color::new(1.0, 1.0, 1.0, 0.07),
        );
    }
    for (player, values) in report.players.iter().zip(&series) {
        let color = render::seat_identity_color(&game.view(), PlayerId(player.seat));
        let points = graph_points(
            &report.sample_ticks,
            values,
            report.final_tick,
            ceiling,
            plot,
        );
        for pair in points.windows(2) {
            draw_line(pair[0].x, pair[0].y, pair[1].x, pair[1].y, 1.8 * s, color);
        }
        let marker_every = (points.len() / 5).max(1);
        for (sample, point) in points.iter().enumerate() {
            if sample.is_multiple_of(marker_every) || sample + 1 == points.len() {
                draw_marker(point.x, point.y, 2.7 * s, usize::from(player.seat), color);
            }
        }
    }
    let marks = mark_positions(&report.moments, report.final_tick, plot, s);
    for (moment, mark) in report.moments.iter().zip(marks) {
        let color = render::seat_identity_color(&game.view(), PlayerId(moment.seat));
        draw_line(mark.x, plot.y, mark.x, mark.y, s, color);
        draw_marker(mark.x, mark.y, 3.6 * s, usize::from(moment.seat), color);
    }
}

/// The crosshair and the box of what the match looked like under it.
fn draw_readout(
    game: &Game,
    report: &MatchStats,
    view: ChartView,
    plot: Rect,
    pointer: Vec2,
    s: f32,
) {
    let tick = tick_at(pointer.x, report.final_tick, plot);
    let column = nearest_column(&report.sample_ticks, tick);
    draw_line(
        pointer.x,
        plot.y,
        pointer.x,
        plot.y + plot.h,
        s,
        theme::TEXT_SECONDARY,
    );

    let name = |seat: u8| game.state.players()[usize::from(seat)].name.as_str();
    let mut head = crate::clock_label::ticks(tick);
    let near = report
        .moments
        .iter()
        .map(|moment| {
            (
                moment_label(moment, name(moment.seat)),
                (x_of(moment.tick, report.final_tick, plot) - pointer.x).abs(),
            )
        })
        .filter(|(_, distance)| *distance <= SNAP * s)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(label, _)| label)
        .or_else(|| {
            fighting_spans(report)
                .into_iter()
                .any(|(from, to)| (from..=to).contains(&tick))
                .then(|| "Heavy fighting".to_string())
        });
    if let Some(near) = near {
        head = format!("{head}  {near}");
    }

    let size = theme::Type::Small.at(s);
    let line = size + 4.0 * s;
    let rows: Vec<(u8, String)> = report
        .players
        .iter()
        .map(|player| {
            let value = view
                .series(&report.sample_ticks, player)
                .get(column)
                .copied()
                .unwrap_or(0);
            (
                player.seat,
                format!(
                    "{}  {} {}",
                    name(player.seat),
                    numeric::with_commas(value),
                    view.unit()
                ),
            )
        })
        .collect();
    let widest = rows
        .iter()
        .map(|(_, text)| text.as_str())
        .chain([head.as_str()])
        .map(|text| measure_text(text, None, numeric::font_size(size), 1.0).width)
        .fold(0.0, f32::max);
    let width = widest + 26.0 * s;
    let height = line * (rows.len() + 1) as f32 + 8.0 * s;
    let x = if pointer.x + width + 10.0 * s <= plot.x + plot.w {
        pointer.x + 10.0 * s
    } else {
        (pointer.x - width - 10.0 * s).max(plot.x)
    };
    let y = (pointer.y - height * 0.5).clamp(plot.y, (plot.y + plot.h - height).max(plot.y));
    draw_rectangle(x, y, width, height, theme::SURFACE_CAPTION);
    draw_text(&head, x + 6.0 * s, y + line, size, theme::TEXT_PRIMARY);
    for (index, (seat, text)) in rows.iter().enumerate() {
        let baseline = y + line * (index + 2) as f32;
        let color = render::seat_identity_color(&game.view(), PlayerId(*seat));
        draw_marker(
            x + 11.0 * s,
            baseline - size * 0.36,
            3.0 * s,
            usize::from(*seat),
            color,
        );
        draw_text(text, x + 20.0 * s, baseline, size, theme::TEXT_BODY);
    }
}

#[cfg(test)]
mod tests;
