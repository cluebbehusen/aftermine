//! The list menu the row-based screens share, plus scenario discovery and
//! the map preview cache.
//!
//! A menu is a titled list navigated by arrow keys, Enter, the mouse, or a
//! finger. Menu input arrives through the same [`RawEvent`] funnel as
//! gameplay, so injected events drive menus exactly like hardware.

use crate::numeric;
use crate::numeric::Fit;
use aftermine_protocol::{MouseButton, RawEvent};
use aftermine_sim::Scenario;
use aftermine_sim::scenario::{MapSize, MapWealth, ScenarioMeta};
use macroquad::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::press::{ScrollPress, Swipe};
use crate::theme::{
    CHIP, EDGE_CHIP, SURFACE_MENU, Stroke, TEXT_ACCENT, TEXT_BODY, TEXT_DISABLED, TEXT_PRIMARY,
    TEXT_SECONDARY, TEXT_TITLE,
};

const ITEM_HEIGHT: f32 = 44.0;
const ITEM_WIDTH: f32 = 420.0;
/// The space between rows, left out of each row's target.
const ROW_GAP: f32 = 6.0;
/// The tightest a desktop list packs its rows before it scrolls.
const MIN_ROW: f32 = 30.0;

thread_local! {
    static MENU_BINDINGS: std::cell::RefCell<crate::action::BindingMap> = std::cell::RefCell::new(crate::action::BindingMap::classic());
}

/// Points menu hints at `bindings`, copying them only when they changed.
pub(crate) fn set_bindings(bindings: &crate::action::BindingMap) {
    MENU_BINDINGS.with(|current| {
        if *current.borrow() != *bindings {
            current.borrow_mut().clone_from(bindings);
        }
    });
}

pub(crate) fn binding_hint(template: &str) -> String {
    use crate::action::Action;
    MENU_BINDINGS.with(|bindings| {
        let bindings = bindings.borrow();
        let mut text = template.to_string();
        for (token, action) in [
            ("{confirm}", Action::Confirm),
            ("{back}", Action::Back),
            ("{up}", Action::MenuUp),
            ("{down}", Action::MenuDown),
            ("{left}", Action::MenuLeft),
            ("{right}", Action::MenuRight),
            ("{delete}", Action::DeleteSave),
            ("{prev_tab}", Action::PrevTab),
            ("{next_tab}", Action::NextTab),
        ] {
            let label = match action {
                Action::PrevTab | Action::NextTab => bindings
                    .tab_chord(action)
                    .map_or_else(|| "unbound".into(), crate::action::BindingMap::chord_label),
                _ => bindings.label(action),
            };
            text = text.replace(token, &label);
        }
        text
    })
}

/// The key that steps tabs as `action` in menus right now, if any.
pub(crate) fn tab_key(action: crate::action::Action) -> Option<String> {
    MENU_BINDINGS.with(|bindings| {
        bindings
            .borrow()
            .tab_chord(action)
            .map(crate::action::BindingMap::chord_label)
    })
}

fn ui() -> f32 {
    crate::render::ui_scale()
}

/// Where a Controls row's columns sit: the action's name, then its
/// primary and secondary chords. Drawing and clicking share it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BindingColumns {
    /// The action's name.
    pub name: Rect,
    /// The primary chord.
    pub primary: Rect,
    /// The secondary chord.
    pub secondary: Rect,
}

impl BindingColumns {
    /// The columns across `row` at UI scale `s`.
    pub fn of(row: Rect, s: f32) -> Self {
        let column = |x: f32, w: f32| Rect::new(x, row.y, w, row.h);
        Self {
            name: column(row.x + 18.0 * s, row.w * 0.56),
            primary: column(row.x + row.w * 0.61, row.w * 0.18),
            secondary: column(row.x + row.w * 0.81, row.w * 0.18),
        }
    }

    /// The chord slot a pointer at `x` picks: each chord owns the space
    /// from its column to the next. `None` over the name.
    pub fn slot_at(&self, x: f32) -> Option<usize> {
        if x >= self.secondary.x {
            Some(1)
        } else if x >= self.primary.x {
            Some(0)
        } else {
            None
        }
    }
}

/// Where a shelf record's columns sit: its title, the match's format,
/// how long the match ran, and when it was saved. Drawing and tests
/// share it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordColumns {
    /// The record's name, or its map, with its map aside and kind chip.
    pub title: Rect,
    /// The match's format.
    pub format: Rect,
    /// Match time, right-aligned.
    pub clock: Rect,
    /// When it was saved, right-aligned.
    pub when: Rect,
}

impl RecordColumns {
    /// The least share of a row the title column keeps.
    const TITLE_SHARE: f32 = 0.4;

    /// The columns across `row` at UI scale `s`, for a list whose widest
    /// format, clock, and when values measure `widest`. Those columns
    /// take the room their widest value needs, scaled down together when
    /// that would leave the title less than [`Self::TITLE_SHARE`] of the
    /// row; the title takes the rest.
    pub fn fit(row: Rect, s: f32, widest: [f32; 3]) -> Self {
        let pad = 18.0 * s;
        let gap = 20.0 * s;
        let inner = row.w - 2.0 * pad;
        let room = (inner * (1.0 - Self::TITLE_SHARE) - 3.0 * gap).max(0.0);
        let need: f32 = widest.iter().sum();
        let scale = if need > room { room / need } else { 1.0 };
        let [format, clock, when] = widest.map(|width| width * scale);
        let when = Rect::new(row.x + row.w - pad - when, row.y, when, row.h);
        let clock = Rect::new(when.x - gap - clock, row.y, clock, row.h);
        let format = Rect::new(clock.x - gap - format, row.y, format, row.h);
        let title_x = row.x + pad;
        Self {
            title: Rect::new(title_x, row.y, (format.x - gap - title_x).max(0.0), row.h),
            format,
            clock,
            when,
        }
    }
}

/// The widest format, clock, and when among `lines`' records, as
/// `measure` sizes them, so every record row shares one set of columns.
fn widest_record_values<R>(lines: &[Line<R>], measure: impl Fn(&str) -> f32) -> [f32; 3] {
    lines.iter().fold([0.0; 3], |widest, line| match line {
        Line::Row(
            Label::Record {
                format,
                clock,
                when,
                ..
            },
            _,
        ) => [
            widest[0].max(measure(format)),
            widest[1].max(measure(clock)),
            widest[2].max(measure(when)),
        ],
        _ => widest,
    })
}

/// Draws a shelf record in `columns` at UI scale `s`: the title in
/// `color`, its map aside and kind chip after it, then the format, clock,
/// and when. The chip always shows; the title outranks the map aside for
/// the room left beside it.
fn draw_record(
    label: &Label,
    columns: RecordColumns,
    s: f32,
    text_size: f32,
    color: Color,
    busy: bool,
) {
    let Label::Record {
        title,
        map,
        tag,
        format,
        clock,
        when,
        available,
    } = label
    else {
        return;
    };
    let dim = if busy { TEXT_DISABLED } else { TEXT_SECONDARY };
    let cell = columns.title;
    let baseline = cell.y + cell.h * 0.68;
    let gap = 10.0 * s;
    let chip_pad = 8.0 * s;
    let row_measure =
        |text: &str| measure_text(text, None, numeric::font_size(text_size), 1.0).width;
    let aside_size = crate::theme::Type::Body.at(s).min(text_size);
    let aside_measure =
        |text: &str| measure_text(text, None, numeric::font_size(aside_size), 1.0).width;
    let chip_size = crate::theme::Type::Small.at(s);
    let chip = tag.map(|tag| {
        let text = measure_text(tag, None, numeric::font_size(chip_size), 1.0).width;
        (tag, text, text + 2.0 * chip_pad)
    });

    let text_room = (cell.w - chip.map_or(0.0, |(_, _, width)| width + gap)).max(0.0);
    let aside_need = map.as_deref().map_or(0.0, |map| aside_measure(map) + gap);
    let shown = crate::typography::elide_to_width(
        title,
        (text_room - aside_need).max(text_room * 0.5),
        &row_measure,
    );
    draw_text(
        &shown,
        cell.x,
        baseline,
        text_size,
        if *available { color } else { TEXT_DISABLED },
    );
    let mut end = cell.x + row_measure(&shown);
    if let Some(map) = map {
        let aside =
            crate::typography::elide_to_width(map, cell.x + text_room - end - gap, &aside_measure);
        if !aside.is_empty() {
            draw_text(&aside, end + gap, baseline, aside_size, dim);
            end += gap + aside_measure(&aside);
        }
    }
    if let Some((tag, text, width)) = chip {
        let height = (chip_size + 10.0 * s).min(cell.h - 4.0 * s);
        let x = if shown.is_empty() { cell.x } else { end + gap };
        let y = cell.y + (cell.h - height) * 0.5;
        draw_rectangle(x, y, width, height, CHIP);
        draw_rectangle_lines(x, y, width, height, Stroke::Hairline.at(s), EDGE_CHIP);
        draw_text(
            tag,
            x + (width - text) * 0.5,
            y + height * 0.5 + chip_size * 0.35,
            chip_size,
            if busy { TEXT_DISABLED } else { TEXT_BODY },
        );
    }

    for (text, column, right) in [
        (format, columns.format, false),
        (clock, columns.clock, true),
        (when, columns.when, true),
    ] {
        let size = crate::typography::fit(text, text_size, column.w, 1.0);
        let x = if right {
            column.x + column.w - measure_text(text, None, numeric::font_size(size), 1.0).width
        } else {
            column.x
        };
        draw_text(text, x, baseline, size, dim);
    }
}

/// What a menu row says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// One line of text.
    Text(String),
    /// A Controls row: the action's name and its primary and secondary
    /// chords, drawn in [`BindingColumns`]; `marked` brackets the chord
    /// the keyboard has chosen.
    Binding {
        name: String,
        keys: [String; 2],
        marked: Option<usize>,
    },
    /// A Settings row: what the setting is, and its value right-aligned.
    Setting { name: String, value: String },
    /// A saved game or replay on the shelf, drawn in [`RecordColumns`]:
    /// its title, then its map when the title is a given name, then its
    /// kind's chip. An unavailable record's title draws disabled.
    Record {
        title: String,
        map: Option<String>,
        tag: Option<&'static str>,
        format: String,
        clock: String,
        when: String,
        available: bool,
    },
}

impl std::fmt::Display for Label {
    /// The row as one line, as automation reads it: `Pan up: W | [I]`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Label::Text(text) => f.write_str(text),
            Label::Binding { name, keys, marked } => {
                let key = |slot: usize| {
                    if *marked == Some(slot) {
                        format!("[{}]", keys[slot])
                    } else {
                        keys[slot].clone()
                    }
                };
                write!(f, "{name}: {} | {}", key(0), key(1))
            }
            Label::Setting { name, value } => write!(f, "{name}: {value}"),
            Label::Record {
                title,
                map,
                tag,
                format,
                clock,
                when,
                ..
            } => {
                f.write_str(title)?;
                if let Some(map) = map {
                    write!(f, " \u{b7} {map}")?;
                }
                if let Some(tag) = tag {
                    write!(f, " [{tag}]")?;
                }
                write!(f, " | {format} | {clock} | {when}")
            }
        }
    }
}

impl From<&str> for Label {
    fn from(text: &str) -> Self {
        Label::Text(text.to_string())
    }
}

impl From<String> for Label {
    fn from(text: String) -> Self {
        Label::Text(text)
    }
}

/// One line of a menu: a section heading, or a row standing for `R`.
#[derive(Debug, Clone, PartialEq)]
pub enum Line<R> {
    /// A section label: drawn dimmer, skipped by the cursor, never
    /// activated.
    Header(String),
    /// A selectable row.
    Row(Label, R),
}

/// A row the player activated: what it stands for and, for a Controls
/// row clicked or tapped on a chord, which chord.
#[derive(Debug, Clone, PartialEq)]
pub struct Activation<R> {
    pub value: R,
    pub column: Option<usize>,
}

/// How a menu heads its list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Heading {
    /// The game's wordmark above a list centred in the space beneath. Home
    /// alone uses it.
    Wordmark,
    /// A screen-title-size title along the top edge, as the setup and
    /// browser pages draw theirs, leaving a long list the room to show
    /// more rows. `band` logical px under the title are reserved for a
    /// strip the screen draws there; the list hangs just below it, so
    /// changing what the strip shows never moves the list.
    Page { band: f32 },
    /// A question or short menu over a match: a screen-title-size title
    /// directly above a centred list.
    Dialog,
}

/// The baseline of a [`Heading::Page`] title, level with the Back button,
/// at UI scale `s`.
pub(crate) fn page_title_baseline(s: f32) -> f32 {
    56.0 * s
}

/// The distance from a title's baseline to its subtitle's.
const SUBTITLE_DROP: f32 = 34.0;

/// Draws a screen title at `baseline`, shrinking only as far as it must to
/// stay between the Back button and its mirror. Every title in the game
/// comes through here, so no screen picks its own size.
pub(crate) fn draw_title(title: &str, baseline: f32, color: Color) {
    let s = ui();
    let room = view_w() - 2.0 * (crate::button::corner_slot(0, s).right() + 16.0 * s);
    let size = crate::typography::fit(
        title,
        crate::theme::Type::Title.at(s),
        room,
        crate::theme::Type::Heading.at(s),
    );
    let dims = measure_text(title, None, numeric::font_size(size), 1.0);
    draw_text(title, (view_w() - dims.width) * 0.5, baseline, size, color);
}

/// How wide a title draws at full size, for a plate sized around it.
pub(crate) fn title_width(title: &str) -> f32 {
    let size = crate::theme::Type::Title.at(ui());
    measure_text(title, None, numeric::font_size(size), 1.0).width
}

/// Draws a page's title along the top edge and its subtitle beneath.
pub(crate) fn draw_page_heading(title: &str, subtitle: &str) {
    let s = ui();
    let baseline = page_title_baseline(s);
    draw_title(title, baseline, TEXT_TITLE);
    draw_subtitle(subtitle, baseline + SUBTITLE_DROP * s);
}

/// Draws a subtitle centered at `baseline`, shrinking to fit the window.
pub(crate) fn draw_subtitle(subtitle: &str, baseline: f32) {
    let s = ui();
    let subtitle = crate::typography::sentence_case(subtitle);
    let size = crate::typography::fit(
        &subtitle,
        crate::theme::Type::Label.at(s),
        view_w() - 2.0 * (crate::button::corner_slot(0, s).right() + 16.0 * s),
        crate::theme::Type::Caption.at(s),
    );
    let dims = measure_text(&subtitle, None, numeric::font_size(size), 1.0);
    draw_text(
        &subtitle,
        (view_w() - dims.width) * 0.5,
        baseline,
        size,
        TEXT_SECONDARY,
    );
}

/// What automation reads off a menu.
pub struct MenuView {
    pub title: String,
    pub selected: usize,
    pub items: Vec<String>,
    pub visible_range: [usize; 2],
    pub hover: Option<usize>,
}

/// Where the list lives this frame.
#[derive(Debug, Clone, Copy)]
struct ListWindow {
    /// The first visible row's top edge.
    top: f32,
    /// The distance from one row to the next.
    pitch: f32,
    /// The first visible row.
    first: usize,
    /// How many rows show.
    visible: usize,
}

/// A titled, selectable list whose rows stand for values of `R`.
///
/// Three independent pieces of state, deliberately: `selected` is the
/// keyboard cursor and activation target, `scroll` is which window of
/// rows is shown, and `hover` is only a highlight. If hover moves the
/// selection or the window follows it, a stationary pointer can walk the
/// whole list by itself.
pub struct Menu<R> {
    /// Heading above the list.
    pub title: String,
    lines: Vec<Line<R>>,
    /// Keyboard cursor; what Enter activates.
    pub selected: usize,
    /// First visible row — moved by the wheel, paging keys, and
    /// `ensure_visible`; never by the pointer.
    scroll: usize,
    /// Row under the pointer, highlight only.
    hover: Option<usize>,
    /// Fractional wheel accumulation: trackpads deliver hundredths per
    /// frame, so only whole accumulated rows scroll.
    wheel_accum: f32,
    /// Row armed by a mouse press or the owning finger. The first finger
    /// down scrolls the list once it drags: a touch-only player has no
    /// wheel or paging keys, so this is the only way to reach rows below
    /// the window.
    press: ScrollPress<usize>,
    /// Drag travel not yet spent on a whole row of scrolling.
    carry: f32,
    /// Horizontal offset of the list as a fraction of the viewport
    /// width, zero for a centered list: the codex shifts its list left
    /// to make room for the page beside it.
    pub shift: f32,
    /// Draws the list at the Controls width, for rows whose labels outgrow
    /// the usual one.
    pub wide: bool,
    /// How the title heads the list.
    pub heading: Heading,
}

fn view_w() -> f32 {
    crate::render::viewport().x
}

fn view_h() -> f32 {
    crate::render::viewport().y
}

impl Menu<usize> {
    /// A plain list whose rows stand for their own indices.
    pub fn list(title: impl Into<String>, items: Vec<String>) -> Self {
        Self::new(
            title,
            items
                .into_iter()
                .enumerate()
                .map(|(index, item)| Line::Row(Label::Text(item), index))
                .collect(),
        )
    }
}

impl<R: Clone> Menu<R> {
    /// A menu over `rows` with no headings, the first row highlighted.
    pub fn rows(title: impl Into<String>, rows: impl IntoIterator<Item = (Label, R)>) -> Self {
        Self::new(
            title,
            rows.into_iter()
                .map(|(label, value)| Line::Row(label, value))
                .collect(),
        )
    }

    /// A menu over `lines`, the first row highlighted.
    pub fn new(title: impl Into<String>, lines: Vec<Line<R>>) -> Self {
        let mut menu = Self {
            title: title.into(),
            lines,
            selected: 0,
            scroll: 0,
            hover: None,
            wheel_accum: 0.0,
            press: ScrollPress::default(),
            carry: 0.0,
            shift: 0.0,
            wide: false,
            heading: Heading::Dialog,
        };
        menu.selected = menu.settle(0, false);
        menu
    }

    /// Whether a line is a section label.
    pub fn is_header(&self, index: usize) -> bool {
        matches!(self.lines.get(index), Some(Line::Header(_)))
    }

    /// The menu's lines.
    pub fn lines(&self) -> &[Line<R>] {
        &self.lines
    }

    /// What the selected row stands for; `None` on an empty or
    /// all-heading list.
    pub fn value(&self) -> Option<&R> {
        match self.lines.get(self.selected)? {
            Line::Row(_, value) => Some(value),
            Line::Header(_) => None,
        }
    }

    /// Selects the first row whose value `pick` accepts; false when none
    /// does.
    pub fn select_where(&mut self, pick: impl Fn(&R) -> bool) -> bool {
        let found = self
            .lines
            .iter()
            .position(|line| matches!(line, Line::Row(_, value) if pick(value)));
        if let Some(index) = found {
            self.select(index);
        }
        found.is_some()
    }

    /// Replaces the lines in place, keeping the cursor, the scroll window
    /// and the pointer's press: a row relabelled after it cycles a value
    /// stays under the cursor.
    pub fn set_lines(&mut self, lines: Vec<Line<R>>) {
        self.lines = lines;
        self.selected = self.settle(self.selected.min(self.lines.len().saturating_sub(1)), false);
    }

    /// The rows as automation reads them.
    pub fn view(&self) -> MenuView {
        MenuView {
            title: self.title.clone(),
            selected: self.selected,
            items: self
                .lines
                .iter()
                .map(|line| match line {
                    Line::Header(text) => text.clone(),
                    Line::Row(label, _) => label.to_string(),
                })
                .collect(),
            visible_range: self.visible_range(),
            hover: self.hover,
        }
    }

    /// Whether the list draws wide: asked to, or because a row draws in
    /// columns.
    fn wide(&self) -> bool {
        self.wide
            || self.lines.iter().any(|line| {
                matches!(
                    line,
                    Line::Row(Label::Binding { .. } | Label::Record { .. }, _)
                )
            })
    }

    /// The nearest non-header row from `index`, toward the start when
    /// `back`, without wrapping; `index` itself on an all-header list.
    fn settle(&self, index: usize, back: bool) -> usize {
        crate::nav::nearest(self.lines.len(), index, back, |row| !self.is_header(row))
            .unwrap_or(index)
    }

    /// Moves the keyboard cursor and scrolls just enough to show it —
    /// the only coupling between selection and the scroll window.
    pub fn select(&mut self, index: usize) {
        self.selected = self.settle(index.min(self.lines.len().saturating_sub(1)), false);
        self.ensure_visible();
    }

    fn ensure_visible(&mut self) {
        let visible = self.window().visible;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible {
            self.scroll = self.selected + 1 - visible;
        }
    }

    fn scroll_by(&mut self, delta: i64) {
        let visible = self.window().visible;
        let max = self.lines.len().saturating_sub(visible);
        self.scroll = (self.scroll.fit::<i64>() + delta)
            .clamp(0, max.fit::<i64>())
            .fit::<usize>();
        // The selection rides inside the window, so Enter never activates
        // a row the wheel has scrolled out of sight, and it never lands on
        // a header.
        let clamped = self
            .selected
            .clamp(self.scroll, self.scroll + visible.saturating_sub(1));
        self.selected = self.settle(clamped, clamped < self.selected);
    }

    /// Scrolls whole rows as a dragging finger moves the list `dy` px.
    /// The content follows the finger.
    fn drag_by(&mut self, dy: f32, began: bool) {
        if began {
            self.carry = 0.0;
            self.hover = None;
        }
        self.carry += dy;
        let row = self.window().pitch;
        while self.carry >= row {
            self.scroll_by(-1);
            self.carry -= row;
        }
        while self.carry <= -row {
            self.scroll_by(1);
            self.carry += row;
        }
    }

    fn row_at(&self, point: Vec2) -> Option<usize> {
        (0..self.lines.len()).find(|i| self.item_rect(*i).is_some_and(|r| r.contains(point)))
    }

    /// Where the list lives this frame: top edge, row height, and the
    /// window of visible rows. The list fits itself between the title
    /// block and the hint line — rows pack as tight as `row_pitch`
    /// allows when the window is short, and past that the list scrolls
    /// around the selection instead of running off the screen.
    fn window(&self) -> ListWindow {
        let s = ui();
        let (top_bound, centring) = match self.heading {
            Heading::Wordmark => ((view_h() * 0.36).max(view_h() * 0.28 + 64.0 * s), 0.5),
            Heading::Page { band } => (page_title_baseline(s) + (64.0 + band) * s, 0.0),
            Heading::Dialog => (page_title_baseline(s) + 64.0 * s, 0.5),
        };
        let bottom_bound = view_h() - 64.0 * s;
        let avail = (bottom_bound - top_bound).max(ITEM_HEIGHT * s);
        let n = self.lines.len().max(1);
        let pitch = row_pitch(avail, n, s, crate::platform::TOUCH_ONLY);
        let visible = numeric::to_usize((avail / pitch).floor()).clamp(1, n);
        ListWindow {
            top: top_bound + (avail - visible as f32 * pitch) * centring,
            pitch,
            // The window is scroll state, clamped — never a function of
            // the selection, or hovering near an edge walks the list.
            first: self.scroll.min(n.saturating_sub(visible)),
            visible,
        }
    }

    /// Touchable bounds for one visible row.
    pub(crate) fn item_rect(&self, index: usize) -> Option<Rect> {
        let s = ui();
        let ListWindow {
            top,
            pitch,
            first,
            visible,
        } = self.window();
        if index < first || index >= first + visible {
            return None;
        }
        let width = if self.wide() {
            (760.0 * s).min(view_w() - 32.0 * s)
        } else {
            ITEM_WIDTH * s
        };
        Some(Rect::new(
            (view_w() - width) * 0.5 + view_w() * self.shift,
            top + (index - first) as f32 * pitch,
            width,
            pitch - ROW_GAP * s,
        ))
    }

    /// Half-open range of rows currently drawn by the scroll window.
    pub fn visible_range(&self) -> [usize; 2] {
        let window = self.window();
        [window.first, window.first + window.visible]
    }

    /// Feeds a frame of events through the menu; returns the activated
    /// row, if any. Mouse position updates come along in the same events.
    pub fn handle(&mut self, events: &[RawEvent], mouse: &mut Vec2) -> Option<Activation<R>> {
        // An empty list has nothing to select, scroll, or activate —
        // and its wrap-around arithmetic divides by zero. The shelf can
        // legitimately be empty on a fresh profile.
        if self.lines.is_empty() {
            return None;
        }
        for event in events {
            let mut press = self.press;
            let swipe = press.feed(event, ui(), |p, _| {
                self.row_at(p).filter(|row| !self.is_header(*row))
            });
            self.press = press;
            match swipe {
                Swipe::Scrolled { dy, began } => {
                    self.drag_by(dy, began);
                    continue;
                }
                Swipe::Activated(row) => {
                    let point = crate::press::position(event);
                    if let Some(p) = point {
                        *mouse = p;
                    }
                    self.selected = row;
                    let column = point.zip(self.item_rect(row)).and_then(|(p, rect)| {
                        matches!(self.lines[row], Line::Row(Label::Binding { .. }, _))
                            .then(|| BindingColumns::of(rect, ui()).slot_at(p.x))
                            .flatten()
                    });
                    return self.activation(row, column);
                }
                Swipe::Held | Swipe::Ignored => {}
            }
            match *event {
                RawEvent::MouseMove { x, y } => {
                    *mouse = vec2(x, y);
                    self.hover = self.row_at(*mouse).filter(|r| !self.is_header(*r));
                }
                RawEvent::MouseDown {
                    button: MouseButton::Left,
                    x,
                    y,
                }
                | RawEvent::MouseUp {
                    button: MouseButton::Left,
                    x,
                    y,
                } => *mouse = vec2(x, y),
                RawEvent::TouchDown { id, x, y } | RawEvent::TouchMove { id, x, y }
                    if self.press.owns(id) =>
                {
                    *mouse = vec2(x, y);
                    self.hover = self.row_at(*mouse).filter(|r| !self.is_header(*r));
                }
                RawEvent::Wheel { delta } => {
                    // Wheel up shows earlier rows; the pointer stays put
                    // and the hover follows whatever slid beneath it.
                    // Whole notches only — fractions accumulate.
                    self.wheel_accum += delta;
                    let steps = self.wheel_accum.trunc();
                    if steps == 0.0 {
                        continue;
                    }
                    self.wheel_accum -= steps;
                    self.scroll_by(if steps > 0.0 { -1 } else { 1 });
                    self.hover = self.row_at(*mouse).filter(|r| !self.is_header(*r));
                }
                RawEvent::KeyDown { .. } => match crate::nav::Nav::decode(event) {
                    Some(crate::nav::Nav::Confirm) if !self.is_header(self.selected) => {
                        return self.activation(self.selected, None);
                    }
                    Some(nav) => {
                        let visible = self.window().visible;
                        if let Some(row) = crate::nav::step_line(
                            self.lines.len(),
                            self.selected,
                            nav,
                            crate::nav::Axis::Vertical,
                            visible,
                            |row| !self.is_header(row),
                        ) {
                            self.hover = None;
                            self.selected = row;
                            self.ensure_visible();
                        }
                    }
                    None => {}
                },
                _ => {}
            }
        }
        None
    }

    fn activation(&self, row: usize, column: Option<usize>) -> Option<Activation<R>> {
        match &self.lines[row] {
            Line::Row(_, value) => Some(Activation {
                value: value.clone(),
                column,
            }),
            Line::Header(_) => None,
        }
    }

    /// Draws the menu (over whatever the caller already drew).
    pub fn draw(&self, subtitle: &str) {
        self.draw_with_coaching(subtitle, None);
    }

    /// Draws the menu with `coaching` in the footer line in place of the
    /// standard key help. The footer is coaching either way: it fades in
    /// only when the player seems stuck.
    pub fn draw_with_coaching(&self, subtitle: &str, coaching: Option<&str>) {
        self.draw_face(subtitle, coaching, false);
    }

    /// Draws the menu while work it started runs: every row dimmed and
    /// inert-looking, `subtitle` saying what is happening, no footer.
    pub fn draw_busy(&self, subtitle: &str) {
        self.draw_face(subtitle, None, true);
    }

    /// Draws the plate a dialog's title and rows sit on, which holds them
    /// apart from the match behind.
    fn draw_dialog_plate(&self, title_baseline: f32) {
        let s = ui();
        let pad = 24.0 * s;
        let list = self.window();
        let last = list.first + list.visible.saturating_sub(1);
        let (Some(first_row), Some(last_row)) = (self.item_rect(list.first), self.item_rect(last))
        else {
            return;
        };
        let top = title_baseline - crate::theme::Type::Title.at(s) - pad * 0.5;
        let rect = Rect::new(
            first_row.x - pad,
            top,
            first_row.w + 2.0 * pad,
            last_row.bottom() + pad - top,
        );
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, crate::theme::SURFACE_PLATE);
        draw_rectangle_lines(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            crate::theme::Stroke::Hairline.at(s),
            crate::theme::TEXT_DISABLED,
        );
    }

    fn draw_face(&self, subtitle: &str, coaching: Option<&str>, busy: bool) {
        let subtitle = binding_hint(subtitle);
        let s = ui();
        let list = self.window();
        match self.heading {
            Heading::Wordmark => {
                let size = crate::theme::Type::Display.at(s);
                let baseline = view_h() * 0.28;
                let dims = measure_text(&self.title, None, numeric::font_size(size), 1.0);
                draw_text(
                    &self.title,
                    (view_w() - dims.width) * 0.5,
                    baseline,
                    size,
                    TEXT_TITLE,
                );
                draw_subtitle(&subtitle, baseline + SUBTITLE_DROP * s);
            }
            Heading::Page { .. } => draw_page_heading(&self.title, &subtitle),
            Heading::Dialog => {
                let baseline = list.top - 64.0 * s;
                self.draw_dialog_plate(baseline);
                draw_title(&self.title, baseline, TEXT_TITLE);
                draw_subtitle(&subtitle, baseline + SUBTITLE_DROP * s);
            }
        }

        let ListWindow {
            pitch: row,
            first,
            visible,
            ..
        } = list;
        let text_size = (crate::theme::Type::Row.at(s) * (row / (ITEM_HEIGHT * s))).clamp(
            crate::theme::Type::Label.at(s),
            crate::theme::Type::Row.at(s),
        );
        // Measured over every record, not just the visible window, so the
        // columns hold still while the list scrolls.
        let record_widest = widest_record_values(&self.lines, |text| {
            measure_text(text, None, numeric::font_size(text_size), 1.0).width
        });
        for (index, line) in self.lines.iter().enumerate() {
            let Some(rect) = self.item_rect(index) else {
                continue;
            };
            let label = match line {
                Line::Header(text) => {
                    let size = crate::theme::Type::Heading.at(s).min(text_size);
                    let dims = measure_text(text, None, numeric::font_size(size), 1.0);
                    draw_text(
                        text,
                        rect.x + (rect.w - dims.width) * 0.5,
                        rect.y + rect.h * 0.68,
                        size,
                        TEXT_SECONDARY,
                    );
                    continue;
                }
                Line::Row(label, _) => label,
            };
            let selected = index == self.selected && !busy;
            let hovered = self.hover == Some(index) && !busy;
            if selected {
                draw_rectangle(rect.x, rect.y, rect.w, rect.h, SURFACE_MENU);
                draw_rectangle_lines(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Stroke::Focus.at(s),
                    TEXT_TITLE,
                );
            } else if hovered {
                draw_rectangle_lines(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Stroke::Hairline.at(s),
                    TEXT_SECONDARY,
                );
            }
            let color = match (busy, selected) {
                (true, _) => TEXT_DISABLED,
                (false, true) => TEXT_PRIMARY,
                (false, false) => TEXT_BODY,
            };
            match label {
                Label::Binding { name, keys, marked } => {
                    let columns = BindingColumns::of(rect, s);
                    let key = |slot: usize| {
                        if *marked == Some(slot) {
                            format!("[{}]", keys[slot])
                        } else {
                            keys[slot].clone()
                        }
                    };
                    for (text, column) in [
                        (name.clone(), columns.name),
                        (key(0), columns.primary),
                        (key(1), columns.secondary),
                    ] {
                        let size = crate::typography::fit(&text, text_size, column.w, 1.0);
                        draw_text(&text, column.x, rect.y + rect.h * 0.68, size, color);
                    }
                }
                Label::Setting { name, value } => {
                    let pad = 18.0 * s;
                    let value_size = crate::typography::fit(value, text_size, rect.w * 0.4, 1.0);
                    let value_width =
                        measure_text(value, None, numeric::font_size(value_size), 1.0).width;
                    let value_x = rect.x + rect.w - pad - value_width;
                    let shown = crate::typography::elide_to_width(
                        name,
                        value_x - rect.x - 2.0 * pad,
                        &|text: &str| {
                            measure_text(text, None, numeric::font_size(text_size), 1.0).width
                        },
                    );
                    let baseline = rect.y + rect.h * 0.68;
                    draw_text(&shown, rect.x + pad, baseline, text_size, color);
                    draw_text(
                        value,
                        value_x,
                        baseline,
                        value_size,
                        if busy { TEXT_DISABLED } else { TEXT_ACCENT },
                    );
                }
                Label::Record { .. } => {
                    let columns = RecordColumns::fit(rect, s, record_widest);
                    draw_record(label, columns, s, text_size, color, busy);
                }
                Label::Text(text) => {
                    let room = rect.w - 36.0 * s;
                    let size = crate::typography::fit(
                        text,
                        text_size,
                        room,
                        crate::theme::Type::Label.at(s),
                    );
                    let shown = crate::typography::elide_to_width(text, room, &|text: &str| {
                        measure_text(text, None, numeric::font_size(size), 1.0).width
                    });
                    draw_text(
                        &shown,
                        rect.x + 18.0 * s,
                        rect.y + rect.h * 0.68,
                        size,
                        color,
                    );
                }
            }
        }
        // Scroll cues when the list is windowed.
        if first > 0 {
            let r = self.item_rect(first).unwrap();
            draw_text(
                "^",
                r.x + r.w * 0.5,
                r.y - 6.0 * s,
                crate::theme::Type::Heading.at(s),
                TEXT_SECONDARY,
            );
        }
        if first + visible < self.lines.len() {
            let r = self.item_rect(first + visible - 1).unwrap();
            draw_text(
                "v",
                r.x + r.w * 0.5,
                r.y + row + 14.0 * s,
                crate::theme::Type::Heading.at(s),
                TEXT_SECONDARY,
            );
        }

        if self.lines.is_empty() || busy {
            return;
        }

        let scrolls = first > 0 || first + visible < self.lines.len();
        let hint = coaching.map_or_else(
            || menu_footer(crate::platform::hands(), scrolls),
            binding_hint,
        );
        let hint = crate::typography::sentence_case(&hint);
        let hint_size = crate::typography::fit(
            &hint,
            crate::theme::Type::Label.at(s),
            view_w() - 32.0 * s,
            crate::theme::Type::Caption.at(s),
        );
        let hint_dims = measure_text(&hint, None, numeric::font_size(hint_size), 1.0);
        draw_text(
            &hint,
            (view_w() - hint_dims.width) * 0.5,
            view_h() - 24.0 * s,
            hint_size,
            crate::hints::fade(TEXT_SECONDARY),
        );
    }
}

/// The distance from one row to the next. A desktop list packs its rows
/// down to a readable minimum before it scrolls; a touch-only list keeps
/// every row a full fingertip target and scrolls sooner.
fn row_pitch(avail: f32, rows: usize, s: f32, touch_only: bool) -> f32 {
    let (min, max) = if touch_only {
        let pitch = crate::theme::MIN_TOUCH_TARGET + ROW_GAP;
        (pitch, pitch)
    } else {
        (MIN_ROW, ITEM_HEIGHT)
    };
    (avail / rows.max(1) as f32).clamp(min * s, max * s)
}

/// The line under every menu: its keys once the player has a keyboard,
/// the pointer's verb, and without keys the drag or scroll only when the
/// list actually scrolls. ASCII on purpose: the default font has no
/// glyphs for arrows.
fn menu_footer(hands: crate::platform::Hands, scrolls: bool) -> String {
    let verb = if hands.touch() { "tap" } else { "click" };
    if !hands.keys {
        let scroll = match (scrolls, hands.touch()) {
            (false, _) => "",
            (true, true) => " - drag to scroll",
            (true, false) => " - scroll for more",
        };
        return format!("{verb} to choose{scroll}");
    }
    MENU_BINDINGS.with(|bindings| {
        let bindings = bindings.borrow();
        format!(
            "{}/{} select - {} confirm - or {verb}",
            bindings.label(crate::action::Action::MenuUp),
            bindings.label(crate::action::Action::MenuDown),
            bindings.label(crate::action::Action::Confirm)
        )
    })
}

/// Fog-free map previews, drawn for the box each screen shows them in. The
/// driver's software rasterizer draws each one at the size it is shown, so
/// nothing is shrunk on the GPU, where thin lines like a frame's rim would
/// drop out. Loading, building, and rasterizing a map takes long enough to
/// hitch a frame, so that runs on a worker; only the texture upload, which
/// must stay on the window thread, happens here. Entries are keyed by
/// scenario path because the discovery list can be rebuilt while the cache
/// lives on; a map that arrived over the network is keyed by its content.
pub struct PreviewCache {
    slots: HashMap<PreviewKey, PreviewSlot<Texture2D>>,
    worker: PreviewWorker,
}

/// Which preview a slot holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PreviewKey {
    /// A discovered map's path; `None` is the embedded skirmish.
    Path(Option<PathBuf>),
    /// A map held in memory, by a hash of its contents.
    Map(u64),
}

/// What the worker draws.
enum PreviewSource {
    Path(Option<PathBuf>),
    Map(Box<Scenario>),
}

/// The physical pixels a preview is drawn to fit, width by height.
type PixelBox = (u32, u32);

/// Boxes a map keeps previews for: the map list's and the setup screen's.
const BOXES_KEPT: usize = 2;

/// One map's previews.
#[derive(Debug)]
struct PreviewSlot<T> {
    /// Drawn previews with the box each fits, most recently wanted first.
    drawn: Vec<(PixelBox, T)>,
    /// The box the worker is drawing for. One at a time, so a window being
    /// resized queues one redraw per map, not one per frame.
    drawing: Option<PixelBox>,
    /// The map failed to load or build; it is not asked for again.
    failed: bool,
}

impl<T> Default for PreviewSlot<T> {
    fn default() -> Self {
        Self {
            drawn: Vec::new(),
            drawing: None,
            failed: false,
        }
    }
}

impl<T> PreviewSlot<T> {
    /// Marks `size` as the box to show next, returning whether the worker
    /// must draw it.
    fn want(&mut self, size: PixelBox) -> bool {
        if let Some(at) = self.drawn.iter().position(|(drawn, _)| *drawn == size) {
            self.drawn[..=at].rotate_right(1);
            return false;
        }
        if self.failed || self.drawing.is_some() {
            return false;
        }
        self.drawing = Some(size);
        true
    }

    /// Takes the worker's preview for `size`, or its failure.
    fn finish(&mut self, size: PixelBox, drawn: Option<T>) {
        if self.drawing == Some(size) {
            self.drawing = None;
        }
        match drawn {
            Some(preview) => {
                self.drawn.insert(0, (size, preview));
                self.drawn.truncate(BOXES_KEPT);
            }
            None => self.failed = true,
        }
    }

    /// The preview to show: the one drawn for the box last wanted, else,
    /// while that one is drawn, the most recently wanted.
    fn shown(&self) -> Option<&(PixelBox, T)> {
        self.drawn.first()
    }
}

/// A preview ready to draw.
pub struct Preview<'a> {
    texture: &'a Texture2D,
    /// The box it was drawn to fit.
    fitted: PixelBox,
    /// Physical pixels per logical one when it was fetched.
    dpi: f32,
}

impl Preview<'_> {
    /// Where the preview lands inside `area`.
    pub fn rect(&self, area: Rect) -> Rect {
        preview_rect(
            (self.texture.width(), self.texture.height()),
            self.fitted,
            area,
            self.dpi,
        )
    }

    /// Draws the preview inside `area` with `tint`, returning where it
    /// landed.
    pub fn draw(&self, area: Rect, tint: Color) -> Rect {
        let rect = self.rect(area);
        draw_texture_ex(
            self.texture,
            rect.x,
            rect.y,
            tint,
            DrawTextureParams {
                dest_size: Some(vec2(rect.w, rect.h)),
                ..Default::default()
            },
        );
        rect
    }
}

/// The whole physical pixels inside `area`, or `None` when it has none.
fn pixel_box(area: Rect, dpi: f32) -> Option<PixelBox> {
    let size = (
        numeric::to_u32((area.w * dpi).floor()),
        numeric::to_u32((area.h * dpi).floor()),
    );
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// Where a `texture`-pixel preview drawn to fit `fitted` lands in `area`,
/// in logical units at `dpi`. Drawn for this box, it maps texel for pixel
/// from a whole-pixel corner, so nothing is resampled. A preview from
/// another box (one still showing while a resized window's is drawn) or one
/// that stopped at full scale short of the box is scaled to fit instead.
fn preview_rect(texture: (f32, f32), fitted: PixelBox, area: Rect, dpi: f32) -> Rect {
    let (tw, th) = texture;
    let (bw, bh) = (area.w * dpi, area.h * dpi);
    let exact =
        pixel_box(area, dpi) == Some(fitted) && (tw == fitted.0 as f32 || th == fitted.1 as f32);
    let scale = if exact { 1.0 } else { (bw / tw).min(bh / th) };
    let (pw, ph) = (tw * scale, th * scale);
    Rect::new(
        (area.x * dpi + (bw - pw) * 0.5).round() / dpi,
        (area.y * dpi + (bh - ph) * 0.5).round() / dpi,
        pw / dpi,
        ph / dpi,
    )
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self {
            slots: HashMap::default(),
            worker: PreviewWorker::spawn(),
        }
    }
}

impl PreviewCache {
    /// The preview of a scenario to show in `area`, requesting it on first
    /// sight. `None` until one is drawn, and for good when it cannot be:
    /// the screen just shows no panel for it.
    pub fn get(&mut self, entry: &ScenarioEntry, area: Rect) -> Option<Preview<'_>> {
        self.slot(&PreviewKey::Path(entry.path.clone()), area, || {
            PreviewSource::Path(entry.path.clone())
        })
    }

    /// The preview of a map held in memory, such as one a LAN host sent,
    /// requested on first sight like [`PreviewCache::get`].
    pub fn get_map(&mut self, scenario: &Scenario, area: Rect) -> Option<Preview<'_>> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(scenario)
            .unwrap_or_default()
            .hash(&mut hasher);
        self.slot(&PreviewKey::Map(hasher.finish()), area, || {
            PreviewSource::Map(Box::new(scenario.clone()))
        })
    }

    fn slot(
        &mut self,
        key: &PreviewKey,
        area: Rect,
        source: impl FnOnce() -> PreviewSource,
    ) -> Option<Preview<'_>> {
        for (finished, size, pixels) in self.worker.finished.try_iter() {
            let texture = pixels.map(|pixels| {
                let texture = Texture2D::from_rgba8(pixels.width, pixels.height, &pixels.rgba);
                texture.set_filter(FilterMode::Nearest);
                texture
            });
            self.slots
                .entry(finished)
                .or_default()
                .finish(size, texture);
        }
        let dpi = screen_dpi_scale();
        let size = pixel_box(area, dpi)?;
        let slot = self.slots.entry(key.clone()).or_default();
        // A dead worker leaves the slot empty, which draws as no panel.
        if slot.want(size) {
            self.worker
                .requests
                .send((key.clone(), size, source()))
                .ok();
        }
        slot.shown().map(|(fitted, texture)| Preview {
            texture,
            fitted: *fitted,
            dpi,
        })
    }
}

/// Rasterized preview bytes, ready for the window thread to upload.
struct PreviewPixels {
    width: u16,
    height: u16,
    rgba: Vec<u8>,
}

/// The thread that draws previews. It exits when the cache drops its sender.
struct PreviewWorker {
    requests: std::sync::mpsc::Sender<(PreviewKey, PixelBox, PreviewSource)>,
    finished: std::sync::mpsc::Receiver<(PreviewKey, PixelBox, Option<PreviewPixels>)>,
}

impl PreviewWorker {
    fn spawn() -> Self {
        let (requests, queue) = std::sync::mpsc::channel::<(PreviewKey, PixelBox, PreviewSource)>();
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (key, size, source) in queue {
                let scenario = match source {
                    PreviewSource::Path(Some(path)) => Scenario::load(path).ok(),
                    PreviewSource::Path(None) => Some(Scenario::skirmish()),
                    PreviewSource::Map(scenario) => Some(*scenario),
                };
                let pixels = scenario.and_then(|scenario| render_preview(&scenario, size));
                if done.send((key, size, pixels)).is_err() {
                    break;
                }
            }
        });
        Self { requests, finished }
    }
}

/// Draws one scenario's preview to fit `size`.
fn render_preview(scenario: &Scenario, (width, height): PixelBox) -> Option<PreviewPixels> {
    let state = scenario.build().ok()?;
    // Seats draw in one neutral tone: the setup screen's seat markers carry
    // ownership, and a preview has no viewer to be relative to.
    let pixmap = aftermine_kit::render::render_thumbnail(&state, width, height, |_| 0x96_94_8C);
    Some(PreviewPixels {
        width: pixmap.width().fit::<u16>(),
        height: pixmap.height().fit::<u16>(),
        rgba: pixmap.data().to_vec(),
    })
}

/// A startable entry on the main menu.
pub struct ScenarioEntry {
    /// Display name (from the file's own `name` field).
    pub label: String,
    /// Seats on the map — with `teams`, the browser's section key (1v1
    /// first).
    pub seats: usize,
    /// Teams the seats form, as [`team_count`] counts them: a
    /// free-for-all has one per seat.
    pub teams: usize,
    /// One-line browser blurb from the authored metadata, when present:
    /// hook plus size/mode/wealth badges.
    pub blurb: Option<String>,
    /// File path; `None` means the embedded skirmish.
    pub path: Option<PathBuf>,
    /// Theme key from the authored metadata — the preview panel grades
    /// its thumbnail with the same tint the match will wear.
    pub theme: String,
}

/// Lists playable scenarios: everything parseable under `scenarios/`, or
/// the embedded skirmish if the directory is missing (e.g. running the
/// binary outside the repo).
pub fn discover_scenarios() -> Vec<ScenarioEntry> {
    let mut entries: Vec<ScenarioEntry> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(crate::assets::resource_root().join("scenarios")) {
        let mut paths: Vec<PathBuf> = dir
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(scenario) = Scenario::load(&path) {
                let blurb = scenario.meta.as_ref().map(browser_blurb);
                let theme = scenario
                    .meta
                    .as_ref()
                    .map(|m| m.theme.clone())
                    .unwrap_or_default();
                entries.push(ScenarioEntry {
                    seats: scenario.players.len(),
                    teams: team_count(scenario.players.iter().map(|p| p.team)),
                    label: scenario.name,
                    blurb,
                    path: Some(path),
                    theme,
                });
            }
        }
    }
    if entries.is_empty() {
        entries.push(ScenarioEntry {
            seats: 2,
            teams: 2,
            label: Scenario::skirmish().name,
            blurb: None,
            path: None,
            theme: String::new(),
        });
    }
    // Sections: 1v1 first (a first Play+Enter must never launch a team
    // match), bigger formats after with team play ahead of a free-for-all
    // of the same size, alphabetical within each. Callers key the
    // remembered pick by path, so re-sorting can't move it.
    entries.sort_by_key(|e| (e.seats, e.teams, e.label.to_lowercase()));
    entries
}

/// A match's format as players name it: "1v1", "2v2", "4-way FFA".
pub(crate) fn match_format(seats: usize, teams: usize) -> String {
    match (seats, teams) {
        (0 | 1, _) => "Solo".to_string(),
        (2, _) => "1v1".to_string(),
        (n, t) if t == n => format!("{n}-way FFA"),
        (n, 2) if n % 2 == 0 => format!("{}v{}", n / 2, n / 2),
        (n, _) => format!("{n} seats"),
    }
}

/// The browser's line for a map: its hook, then its size, mode and wealth
/// badges.
pub(crate) fn browser_blurb(meta: &ScenarioMeta) -> String {
    // The duration band shares one badge with the size it qualifies.
    let size = meta.size.map_or("", MapSize::label);
    let size = match (size.is_empty(), meta.duration.is_empty()) {
        (false, false) => format!("{size} | {}", meta.duration),
        (false, true) => size.to_owned(),
        (true, _) => meta.duration.clone(),
    };
    // Only badges that exist: a missing field must not render as a
    // dangling separator.
    let badges: Vec<&str> = [
        size.as_str(),
        meta.mode.as_str(),
        meta.wealth.map_or("", MapWealth::label),
    ]
    .into_iter()
    .filter(|badge| !badge.is_empty())
    .collect();
    format!("{}  [{}]", meta.hook, badges.join(" - "))
}

/// Teams formed by seats with these team indices: seats sharing an index
/// count once, and a seat with none is its own team.
pub(crate) fn team_count(teams: impl IntoIterator<Item = Option<u8>>) -> usize {
    let mut shared = std::collections::BTreeSet::new();
    let mut alone = 0;
    for team in teams {
        match team {
            Some(team) => {
                shared.insert(team);
            }
            None => alone += 1,
        }
    }
    shared.len() + alone
}

#[cfg(test)]
mod footer_tests;

#[cfg(test)]
mod preview_tests;

#[cfg(test)]
mod empty_tests;

#[cfg(test)]
mod header_tests;

#[cfg(test)]
mod entry_tests;
