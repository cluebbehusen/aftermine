//! The decided-match report: team-grouped scoreboard, match curve, and
//! touchable next steps. It is its own screen with its own input, not an
//! overlay on the pause menu.

use crate::bot_label::{BotLabelStyle, bot_label};
use crate::game::{Game, SoundKind};
use crate::nav::{Axis, Nav, step_line};
use crate::numeric;
use crate::numeric::Fit;
use crate::press::{Fed, Press};
use crate::theme::Type;
use crate::typography::{clip_chars, elide_to_width};
use crate::{render, theme};
use aftermine_protocol::{Key, RawEvent};
use aftermine_sim::{GameResult, PlayerId};
use macroquad::prelude::*;

mod built;
mod chart;

/// The wide table's column headers, under their group's unit.
const WIDE_STAT_HEADERS: [&str; 6] = [
    "EARNED",
    "PEAK WEALTH",
    "PEAK ARMY",
    "LOST",
    "UNITS",
    "STRUCTURES",
];

/// The wide table's groups: what its columns are counted in, and the columns
/// each covers.
const WIDE_STAT_GROUPS: [(&str, std::ops::Range<usize>); 2] = [("SCRAP", 0..4), ("BUILT", 4..6)];

/// What a result frame decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Stay on the report.
    Stay,
    /// Start the same authored match again.
    Rematch,
    /// Watch the completed command record.
    Watch,
    /// Watch the completed command record from this tick.
    WatchFrom(u64),
    /// Inspect the already-final battlefield without replaying it.
    ViewFinalMap,
    /// Return to the front door.
    Home,
}

/// One of the report's next steps, in button order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultAction {
    Rematch,
    Watch,
    ViewFinalMap,
    Home,
}

impl ResultAction {
    /// Every action, left to right.
    pub const ALL: [Self; 4] = [Self::Rematch, Self::Watch, Self::ViewFinalMap, Self::Home];

    /// A LAN match's actions: a rematch takes a new lobby, so there is none.
    pub const LAN: [Self; 3] = [Self::Watch, Self::ViewFinalMap, Self::Home];

    fn label(self) -> &'static str {
        match self {
            Self::Rematch => "Rematch",
            Self::Watch => "Watch replay",
            Self::ViewFinalMap => "View final map",
            Self::Home => "Home",
        }
    }

    fn out(self) -> Out {
        match self {
            Self::Rematch => Out::Rematch,
            Self::Watch => Out::Watch,
            Self::ViewFinalMap => Out::ViewFinalMap,
            Self::Home => Out::Home,
        }
    }
}

/// Touchable action geometry, injected for headless tests.
pub(crate) fn action_rects(count: usize, viewport: Vec2, scale: f32) -> Vec<Rect> {
    let gap = 10.0 * scale;
    let margin = 24.0 * scale;
    let action_count = count as f32;
    let available = (viewport.x - margin * 2.0 - gap * (action_count - 1.0)).max(action_count);
    let width = (available / action_count).min(210.0 * scale);
    let total = width * action_count + gap * (action_count - 1.0);
    let x = (viewport.x - total) * 0.5;
    let y = viewport.y - 60.0 * scale;
    (0..count)
        .map(|index| {
            Rect::new(
                x + (width + gap) * index as f32,
                y,
                width,
                crate::theme::MIN_TOUCH_TARGET * scale,
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct ResultsLayout {
    compact_roster: bool,
    wide_table: bool,
    title_y: f32,
    meta_y: f32,
    meta_size: f32,
    header_y: f32,
    header_size: f32,
    rule_offset: f32,
    row_height: f32,
    row_size: f32,
    row_baseline: f32,
    marker_radius: f32,
    graph_label_size: f32,
    /// The chart's tab strip height: a fingertip, except in a window too
    /// short for anything but a mouse.
    chart_strip: f32,
    graph_top: f32,
    graph_bottom: f32,
}

/// The window height in logical px where the table can take its full spacing.
/// The smallest iPad is well above this.
const ROOMY_HEIGHT: f32 = 640.0;

/// The report's vertical rhythm in logical px, before any fitting.
#[derive(Debug, Clone, Copy)]
struct ResultsMetrics {
    title_y: f32,
    meta_y: f32,
    meta_size: f32,
    header_y: f32,
    header_size: f32,
    rule_offset: f32,
    row_height: f32,
    row_size: f32,
    /// Where in its row a player's text sits, as a fraction of the row.
    row_baseline: f32,
    marker_radius: f32,
    /// Rows of space between the table and the graph.
    graph_row_padding: f32,
    graph_padding: f32,
}

impl ResultsMetrics {
    /// Six or more players in a short window.
    const COMPACT: Self = Self {
        title_y: 43.0,
        meta_y: 62.0,
        meta_size: Type::Small.px(),
        header_y: 79.0,
        header_size: Type::Caption.px(),
        rule_offset: 13.0,
        row_height: 16.0,
        row_size: Type::Small.px(),
        row_baseline: 0.78,
        marker_radius: 3.4,
        graph_row_padding: 0.45,
        graph_padding: 3.0,
    };

    /// A short window, where the table keeps tight to leave room below it.
    fn short() -> Self {
        Self {
            title_y: 48.0,
            meta_y: 68.0,
            meta_size: Type::Body.px(),
            header_y: 91.0,
            header_size: Type::Body.px(),
            rule_offset: 16.0,
            row_height: 22.0,
            row_size: Type::Label.px(),
            row_baseline: 0.8,
            marker_radius: 4.0,
            graph_row_padding: 1.15,
            graph_padding: 5.0,
        }
    }

    /// Every other report; a crowded roster takes shorter rows to leave the
    /// chart its room.
    fn standard(player_count: usize) -> Self {
        Self {
            title_y: 48.0,
            meta_y: 74.0,
            meta_size: Type::Body.px(),
            header_y: 112.0,
            header_size: Type::Body.px(),
            rule_offset: 22.0,
            row_height: if player_count >= 6 { 30.0 } else { 38.0 },
            row_size: Type::Label.px(),
            row_baseline: 0.68,
            marker_radius: 4.0,
            graph_row_padding: 0.6,
            graph_padding: 6.0,
        }
    }
}

fn results_layout(viewport: Vec2, scale: f32, player_count: usize) -> ResultsLayout {
    let logical_width = viewport.x / scale.max(f32::EPSILON);
    let logical_height = viewport.y / scale.max(f32::EPSILON);
    let compact_roster = logical_height <= 480.0 && player_count >= 6;
    let wide_table = !compact_roster && logical_width >= 1_100.0;
    let ResultsMetrics {
        title_y,
        meta_y,
        meta_size,
        header_y,
        header_size,
        rule_offset,
        row_height,
        row_size,
        row_baseline,
        marker_radius,
        graph_row_padding,
        graph_padding,
    } = if compact_roster {
        ResultsMetrics::COMPACT
    } else if logical_height < ROOMY_HEIGHT {
        ResultsMetrics::short()
    } else {
        ResultsMetrics::standard(player_count)
    };
    let rule_offset = rule_offset + if wide_table { header_size } else { 0.0 };
    let actions_y = action_rects(1, viewport, scale)[0].y / scale;
    let row_height = f32::min(
        row_height,
        (actions_y - header_y - rule_offset - 8.0) / player_count.max(1) as f32,
    );
    let graph_top = header_y
        + rule_offset
        + (player_count as f32 + graph_row_padding) * row_height
        + graph_padding;
    let graph_bottom = actions_y - 25.0;

    ResultsLayout {
        compact_roster,
        wide_table,
        title_y: title_y * scale,
        meta_y: meta_y * scale,
        meta_size: meta_size * scale,
        header_y: header_y * scale,
        header_size: header_size * scale,
        rule_offset: rule_offset * scale,
        row_height: row_height * scale,
        row_size: row_size * scale,
        row_baseline,
        marker_radius: marker_radius * scale,
        graph_label_size: if compact_roster {
            Type::Small
        } else {
            Type::Body
        }
        .at(scale),
        chart_strip: if logical_height <= 480.0 {
            24.0
        } else {
            crate::theme::MIN_TOUCH_TARGET
        } * scale,
        graph_top: graph_top * scale,
        graph_bottom: graph_bottom * scale,
    }
}

fn action_at(
    actions: &[ResultAction],
    point: Vec2,
    viewport: Vec2,
    scale: f32,
) -> Option<ResultAction> {
    actions
        .iter()
        .copied()
        .zip(action_rects(actions.len(), viewport, scale))
        .find(|(_, rect)| rect.contains(point))
        .map(|(action, _)| action)
}

/// Where the scoreboard's columns sit: the player name's left edge, then
/// the six statistics (scrap earned; peak wealth; peak army; value lost; units
/// and structures built). A wide table centers each statistic on its x.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TableColumns {
    player: f32,
    stats: [f32; 6],
}

fn table_columns(left: f32, right: f32, wide: bool) -> TableColumns {
    let width = right - left;
    let stats = if wide {
        let stats_left = player_column_right(left, right, true);
        let stat_width = (right - stats_left) / 6.0;
        std::array::from_fn(|index| stats_left + (index as f32 + 0.5) * stat_width)
    } else {
        [0.39, 0.50, 0.59, 0.68, 0.77, 0.87].map(|fraction| left + width * fraction)
    };
    TableColumns {
        player: left,
        stats,
    }
}

fn player_column_right(left: f32, right: f32, wide: bool) -> f32 {
    left + (right - left) * if wide { 0.34 } else { 0.39 }
}

fn draw_centered_text(text: &str, x: f32, y: f32, size: f32, color: Color) {
    let width = measure_text(text, None, numeric::font_size(size), 1.0).width;
    draw_text(text, x - width * 0.5, y, size, color);
}

fn player_name_with_controller(
    game: &Game,
    seat: usize,
    max_name_chars: usize,
    compact: bool,
) -> String {
    let player = &game.state.players()[seat];
    let name = clip_chars(&player.name, max_name_chars);
    let Some(spec) = game.scenario.players.get(seat) else {
        return name;
    };
    if !spec.bot {
        return name;
    }
    match (spec.bot_config, compact) {
        (Some(config), true) => format!(
            "{name}  {}",
            bot_label(config.difficulty, config.stance, BotLabelStyle::Compact,)
        ),
        (Some(config), false) => format!(
            "{name}  {}",
            bot_label(config.difficulty, config.stance, BotLabelStyle::Controller)
        ),
        (None, _) => format!("{name}  bot"),
    }
}

fn text_fits(text: &str, max_width: f32, measure: &impl Fn(&str) -> f32) -> bool {
    measure(text) <= max_width
}

fn fitted_player_name_with_controller(
    game: &Game,
    seat: usize,
    max_name_chars: usize,
    prefer_compact: bool,
    max_width: f32,
    measure: &impl Fn(&str) -> f32,
) -> String {
    if !prefer_compact {
        let full = player_name_with_controller(game, seat, max_name_chars, false);
        if text_fits(&full, max_width, measure) {
            return full;
        }
    }

    let compact = player_name_with_controller(game, seat, max_name_chars, true);
    if text_fits(&compact, max_width, measure) {
        return compact;
    }
    for cap in (4..max_name_chars).rev() {
        let candidate = player_name_with_controller(game, seat, cap, true);
        if text_fits(&candidate, max_width, measure) {
            return candidate;
        }
    }

    let spec = game.scenario.players.get(seat);
    let fallback = match spec {
        Some(spec) if spec.bot => spec.bot_config.map_or_else(
            || "bot".to_string(),
            |config| bot_label(config.difficulty, config.stance, BotLabelStyle::Compact),
        ),
        _ => game.state.players()[seat].name.clone(),
    };
    elide_to_width(&fallback, max_width, measure)
}

/// Stateful pointer/keyboard ownership for the report.
pub struct ResultsScreen {
    actions: &'static [ResultAction],
    selected: ResultAction,
    hover: Option<ResultAction>,
    press: Press<ResultAction>,
    chart: chart::Chart,
}

impl ResultsScreen {
    /// Opens with the first action selected. A LAN match has no rematch.
    pub fn open(lan: bool) -> Self {
        let actions: &'static [ResultAction] = if lan {
            &ResultAction::LAN
        } else {
            &ResultAction::ALL
        };
        Self {
            actions,
            selected: actions[0],
            hover: None,
            press: Press::default(),
            chart: chart::Chart::default(),
        }
    }

    fn index_of(&self, action: ResultAction) -> usize {
        self.actions
            .iter()
            .position(|candidate| *candidate == action)
            .unwrap_or(0)
    }

    /// Keyboard cursor for the debug UI surface.
    pub fn selected(&self) -> usize {
        self.index_of(self.selected)
    }

    /// Pointer hover for the debug UI surface.
    pub fn hover(&self) -> Option<usize> {
        self.hover.map(|action| self.index_of(action))
    }

    /// Stable labels for automation and accessibility.
    pub fn items(&self) -> Vec<String> {
        self.actions
            .iter()
            .map(|action| action.label().to_string())
            .collect()
    }

    /// The chart panel's rect, when the window has room for one.
    fn chart_panel(layout: &ResultsLayout, viewport: Vec2, s: f32) -> Option<Rect> {
        let panel = results_panel(viewport, s);
        let left = panel.x + 20.0 * s;
        let right = panel.x + panel.w - 20.0 * s;
        (layout.graph_bottom - layout.graph_top >= 60.0 * s).then(|| {
            Rect::new(
                left,
                layout.graph_top,
                right - left,
                layout.graph_bottom - layout.graph_top,
            )
        })
    }

    /// Applies one frame to the whole report: the chart's share first, then
    /// the action row with what it leaves.
    pub fn frame(
        &mut self,
        game: &Game,
        events: &[RawEvent],
        mouse: &mut Vec2,
        viewport: Vec2,
        scale: f32,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let layout = results_layout(viewport, scale, game.state.players().len());
        let panel = Self::chart_panel(&layout, viewport, scale);
        let (Some(report), Some(panel)) = (game.end_stats.as_ref(), panel) else {
            return self.update(events, mouse, viewport, scale, sounds);
        };
        let chart_layout =
            chart::chart_layout(panel, layout.chart_strip, layout.graph_label_size, scale);
        let (watch, rest) = self
            .chart
            .route(events, report, &chart_layout, scale, sounds);
        if let Some(tick) = watch {
            return Out::WatchFrom(tick);
        }
        self.update(&rest, mouse, viewport, scale, sounds)
    }

    /// Applies one frame through the same raw-event funnel as every menu.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        viewport: Vec2,
        scale: f32,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        for event in events {
            let actions = self.actions;
            if let Fed::Activated(action) = self
                .press
                .feed(event, |p, _| action_at(actions, p, viewport, scale))
            {
                if let Some(p) = crate::press::position(event) {
                    *mouse = p;
                }
                self.selected = action;
                sounds.push((SoundKind::Click, None));
                return action.out();
            }
            match *event {
                RawEvent::MouseMove { x, y } => {
                    *mouse = vec2(x, y);
                    self.hover = action_at(actions, *mouse, viewport, scale);
                }
                RawEvent::TouchDown { id, x, y } | RawEvent::TouchMove { id, x, y }
                    if self.press.owns(id) =>
                {
                    *mouse = vec2(x, y);
                    self.hover = action_at(actions, *mouse, viewport, scale);
                }
                RawEvent::KeyDown { key: Key::Enter } => {
                    sounds.push((SoundKind::Click, None));
                    return self.selected.out();
                }
                RawEvent::KeyDown { key: Key::Escape } => {
                    sounds.push((SoundKind::Click, None));
                    return Out::Home;
                }
                RawEvent::KeyDown { .. } => {
                    let selected = self.index_of(self.selected);
                    if let Some(next) = Nav::decode(event).and_then(|nav| {
                        let count = actions.len();
                        step_line(count, selected, nav, Axis::Both, count, |_| true)
                    }) {
                        self.hover = None;
                        self.selected = actions[next];
                    }
                }
                _ => {}
            }
        }
        Out::Stay
    }

    /// Draws the report over the rendered battlefield.
    pub fn draw(&self, game: &Game, sprites: &crate::assets::Sprites) {
        let viewport = render::viewport();
        let s = render::ui_scale();
        draw_rectangle(0.0, 0.0, viewport.x, viewport.y, theme::VEIL);
        let panel = results_panel(viewport, s);
        draw_rectangle(panel.x, panel.y, panel.w, panel.h, theme::SURFACE_MENU);
        draw_rectangle_lines(
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            theme::Stroke::Edge.at(s),
            theme::EDGE_WARM,
        );

        let player_count = game.state.players().len();
        let layout = results_layout(viewport, s, player_count);
        let (title, title_color, subtitle) = verdict(game);
        crate::menu::draw_title(title, layout.title_y, title_color);
        let stats = game.end_stats.as_ref();
        let duration = stats.map_or(0, |report| report.final_tick);
        let meta = format!("{subtitle}  |  {}", crate::clock_label::ticks(duration));
        let meta_size = crate::typography::fit(
            &meta,
            layout.meta_size,
            panel.w - 24.0 * s,
            crate::theme::Type::Caption,
        );
        let meta_dims = measure_text(&meta, None, numeric::font_size(meta_size), 1.0);
        draw_text(
            &meta,
            (viewport.x - meta_dims.width) * 0.5,
            layout.meta_y,
            meta_size,
            theme::TEXT_BODY,
        );

        let header_y = layout.header_y;
        let row_h = layout.row_height;
        let left = panel.x + 20.0 * s;
        let right = panel.x + panel.w - 20.0 * s;
        let columns = table_columns(left, right, layout.wide_table);
        // The bottom header line, level with the column names, just above the
        // table's rule.
        let column_y = header_y + layout.rule_offset - 12.0 * s;
        draw_text(
            "PLAYER",
            columns.player,
            if layout.wide_table {
                column_y
            } else {
                header_y
            },
            layout.header_size,
            theme::TEXT_SECONDARY,
        );
        if layout.wide_table {
            for (group, span) in WIDE_STAT_GROUPS {
                let first = columns.stats[span.start];
                let last = columns.stats[span.end - 1];
                let reach = (columns.stats[1] - columns.stats[0]) * 0.46;
                draw_centered_text(
                    group,
                    f32::midpoint(first, last),
                    header_y,
                    layout.header_size,
                    theme::TEXT_BODY,
                );
                let line_y = header_y + 6.0 * s;
                draw_line(
                    first - reach,
                    line_y,
                    last + reach,
                    line_y,
                    s,
                    theme::TEXT_DISABLED,
                );
            }
            for (text, x) in WIDE_STAT_HEADERS.into_iter().zip(columns.stats) {
                draw_centered_text(text, x, column_y, layout.header_size, theme::TEXT_SECONDARY);
            }
        } else {
            for (text, x) in [
                ("SCRAP", columns.stats[0]),
                ("WEALTH", columns.stats[1]),
                ("ARMY", columns.stats[2]),
                ("LOST", columns.stats[3]),
                (
                    "BUILT",
                    f32::midpoint(columns.stats[4], columns.stats[5]) - 17.0 * s,
                ),
            ] {
                draw_text(text, x, header_y, layout.header_size, theme::TEXT_SECONDARY);
            }
            for (x, kind) in [
                (columns.stats[4], StatIcon::Unit),
                (columns.stats[5], StatIcon::Building),
            ] {
                draw_stat_icon(
                    vec2(x + 5.0 * s, header_y + 9.0 * s),
                    4.5 * s,
                    kind,
                    theme::TEXT_BODY,
                );
            }
        }
        let rule_y = header_y + layout.rule_offset;
        draw_line(left, rule_y, right, rule_y, 1.0 * s, theme::TEXT_DISABLED);

        if let Some(report) = stats {
            let mut seats: Vec<usize> = (0..game.state.players().len()).collect();
            seats.sort_by_key(|seat| (game.state.players()[*seat].team, *seat));
            let teamed = seats_share_a_team(game);
            for (row, seat) in seats.into_iter().enumerate() {
                let player = &game.state.players()[seat];
                let numbers = report
                    .players
                    .iter()
                    .find(|entry| usize::from(entry.seat) == seat);
                let y = rule_y + (row as f32 + layout.row_baseline) * row_h;
                let color = render::seat_identity_color(&game.view(), PlayerId(seat.fit::<u8>()));
                let winner = game.state.winners().contains(&PlayerId(seat.fit::<u8>()));
                if winner {
                    let band = winner_band(left, right, rule_y, row, row_h, s);
                    draw_rectangle(band.x, band.y, band.w, band.h, WINNER_TINT);
                    draw_rectangle(band.x, band.y, 3.0 * s, band.h, theme::TEXT_ACCENT);
                }
                draw_marker(
                    columns.player,
                    y - layout.row_size * 0.36,
                    layout.marker_radius,
                    seat,
                    color,
                );
                let won_size = layout.row_size;
                let won_width = if winner {
                    measure_text("WON", None, numeric::font_size(won_size), 1.0).width + 10.0 * s
                } else {
                    0.0
                };
                let max_name_chars = if viewport.x < 800.0 {
                    if game.scenario.players.get(seat).is_some_and(|spec| spec.bot) {
                        12
                    } else {
                        15
                    }
                } else {
                    24
                };
                let prefix = if teamed {
                    format!("T{}  ", player.team + 1)
                } else {
                    String::new()
                };
                let player_text_x = columns.player + 11.0 * s;
                let player_text_right = player_column_right(left, right, layout.wide_table);
                let fixed_width =
                    measure_text(&prefix, None, numeric::font_size(layout.row_size), 1.0).width
                        + won_width;
                let name_width =
                    (player_text_right - player_text_x - 8.0 * s - fixed_width).max(0.0);
                let measure_name = |text: &str| {
                    measure_text(text, None, numeric::font_size(layout.row_size), 1.0).width
                };
                let name = fitted_player_name_with_controller(
                    game,
                    seat,
                    max_name_chars,
                    layout.compact_roster,
                    name_width,
                    &measure_name,
                );
                let label = format!("{prefix}{name}");
                draw_text(&label, player_text_x, y, layout.row_size, color);
                if winner {
                    draw_text(
                        "WON",
                        player_text_x + measure_name(&label) + 10.0 * s,
                        y,
                        won_size,
                        theme::TEXT_ACCENT,
                    );
                }
                if let Some(numbers) = numbers {
                    let peak = |series: &[u32]| series.iter().copied().max().unwrap_or(0);
                    let structures: u32 = numbers.structures_built.values().sum();
                    let number_color = if winner {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_BODY
                    };
                    for (value, x) in [
                        (numbers.scrap_earned_total, columns.stats[0]),
                        (peak(&numbers.wealth), columns.stats[1]),
                        (peak(&numbers.army_value), columns.stats[2]),
                        (numbers.value_lost_total, columns.stats[3]),
                        (numbers.units_trained, columns.stats[4]),
                        (structures, columns.stats[5]),
                    ] {
                        let text = numeric::with_commas(value);
                        if layout.wide_table {
                            draw_centered_text(&text, x, y, layout.row_size, number_color);
                        } else {
                            draw_text(&text, x, y, layout.row_size, number_color);
                        }
                    }
                }
            }

            if let Some(chart_panel) = Self::chart_panel(&layout, viewport, s) {
                let chart_layout = chart::chart_layout(
                    chart_panel,
                    layout.chart_strip,
                    layout.graph_label_size,
                    s,
                );
                self.chart.draw(
                    game,
                    sprites,
                    report,
                    &chart_layout,
                    s,
                    layout.graph_label_size,
                );
            }
        }

        for (action, rect) in
            self.actions
                .iter()
                .copied()
                .zip(action_rects(self.actions.len(), viewport, s))
        {
            let active = self.hover == Some(action) || self.selected == action;
            crate::button::draw(rect, action.label(), active, s);
        }
    }
}

/// The tint behind a winner's row.
const WINNER_TINT: Color = Color::new(0.88, 0.66, 0.25, 0.16);

/// The band behind a winner's row.
fn winner_band(left: f32, right: f32, rule_y: f32, row: usize, row_h: f32, s: f32) -> Rect {
    Rect::new(
        left - 14.0 * s,
        rule_y + row as f32 * row_h + 1.0 * s,
        right - left + 28.0 * s,
        row_h - 2.0 * s,
    )
}

/// The report's plate.
fn results_panel(viewport: Vec2, s: f32) -> Rect {
    Rect::new(
        12.0 * s,
        10.0 * s,
        viewport.x - 24.0 * s,
        viewport.y - 20.0 * s,
    )
}

/// Whether any team holds two or more seats, which is when a row names its
/// team.
fn seats_share_a_team(game: &Game) -> bool {
    let players = game.state.players();
    players.iter().enumerate().any(|(seat, player)| {
        players[..seat]
            .iter()
            .any(|other| other.team == player.team)
    })
}

fn verdict(game: &Game) -> (&'static str, Color, String) {
    let winners = game.state.winners();
    match game.state.result() {
        Some(GameResult::Victory { .. }) if winners.contains(&game.presentation.human) => (
            "VICTORY",
            theme::TEXT_ACCENT,
            winner_subtitle(game, &winners),
        ),
        Some(GameResult::Victory { .. }) if game.state.player(game.presentation.human).resigned => {
            (
                "SURRENDERED",
                theme::TEXT_DANGER,
                winner_subtitle(game, &winners),
            )
        }
        Some(GameResult::Victory { .. }) => (
            "DEFEAT",
            theme::TEXT_DANGER,
            winner_subtitle(game, &winners),
        ),
        Some(GameResult::Draw) | None => (
            "MUTUAL DESTRUCTION",
            theme::TEXT_BODY,
            "no Foundry survived".to_string(),
        ),
    }
}

fn winner_subtitle(game: &Game, winners: &[PlayerId]) -> String {
    if winners.len() > 2 {
        let team = game.state.player(winners[0]).team;
        if winners
            .iter()
            .all(|winner| game.state.player(*winner).team == team)
        {
            return format!("Team {} won", team + 1);
        }
    }
    format!(
        "{} won",
        winners
            .iter()
            .map(|seat| game.state.player(*seat).name.clone())
            .collect::<Vec<_>>()
            .join(" & ")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatIcon {
    Unit,
    Building,
}

fn draw_stat_icon(center: Vec2, size: f32, kind: StatIcon, color: Color) {
    match kind {
        StatIcon::Unit => {
            draw_rectangle(
                center.x - size,
                center.y - size * 0.55,
                size * 2.0,
                size * 1.1,
                color,
            );
            draw_circle(
                center.x - size * 0.62,
                center.y + size * 0.72,
                size * 0.32,
                color,
            );
            draw_circle(
                center.x + size * 0.62,
                center.y + size * 0.72,
                size * 0.32,
                color,
            );
        }
        StatIcon::Building => {
            draw_rectangle_lines(
                center.x - size,
                center.y - size * 0.65,
                size * 2.0,
                size * 1.65,
                1.5,
                color,
            );
            draw_line(
                center.x - size,
                center.y - size * 0.65,
                center.x,
                center.y - size * 1.15,
                1.5,
                color,
            );
            draw_line(
                center.x,
                center.y - size * 1.15,
                center.x + size,
                center.y - size * 0.65,
                1.5,
                color,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeatMarker {
    Circle,
    Square,
    Diamond,
    Plus,
    RingDot,
    BoxDot,
    Triangle,
    Cross,
}

fn seat_marker(seat: usize) -> SeatMarker {
    match seat {
        0 => SeatMarker::Circle,
        1 => SeatMarker::Square,
        2 => SeatMarker::Diamond,
        3 => SeatMarker::Plus,
        4 => SeatMarker::RingDot,
        5 => SeatMarker::BoxDot,
        6 => SeatMarker::Triangle,
        _ => SeatMarker::Cross,
    }
}

pub(super) fn draw_marker(x: f32, y: f32, r: f32, seat: usize, color: Color) {
    match seat_marker(seat) {
        SeatMarker::Circle => draw_circle(x, y, r, color),
        SeatMarker::Square => draw_rectangle(x - r, y - r, r * 2.0, r * 2.0, color),
        SeatMarker::Diamond => {
            for (a, b) in [
                (vec2(x, y - r), vec2(x + r, y)),
                (vec2(x + r, y), vec2(x, y + r)),
                (vec2(x, y + r), vec2(x - r, y)),
                (vec2(x - r, y), vec2(x, y - r)),
            ] {
                draw_line(a.x, a.y, b.x, b.y, 1.5, color);
            }
        }
        SeatMarker::Plus => {
            draw_line(x - r, y, x + r, y, 1.5, color);
            draw_line(x, y - r, x, y + r, 1.5, color);
        }
        SeatMarker::RingDot => {
            draw_circle_lines(x, y, r, 1.5, color);
            draw_circle(x, y, r * 0.32, color);
        }
        SeatMarker::BoxDot => {
            draw_rectangle_lines(x - r, y - r, r * 2.0, r * 2.0, 1.5, color);
            draw_circle(x, y, r * 0.32, color);
        }
        SeatMarker::Triangle => {
            let points = [vec2(x, y - r), vec2(x + r, y + r), vec2(x - r, y + r)];
            for index in 0..3 {
                let a = points[index];
                let b = points[(index + 1) % points.len()];
                draw_line(a.x, a.y, b.x, b.y, 1.5, color);
            }
        }
        SeatMarker::Cross => {
            draw_line(x - r, y - r, x + r, y + r, 1.5, color);
            draw_line(x - r, y + r, x + r, y - r, 1.5, color);
        }
    }
}

#[cfg(test)]
mod tests;
