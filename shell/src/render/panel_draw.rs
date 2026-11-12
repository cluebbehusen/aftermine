//! The command band, the orders dock, and the hover tooltip: the
//! selection panel's drawn form. Geometry it publishes goes into the
//! `LayoutModel`; the pure card model lives in `crate::panel`.

use super::*;
use crate::layout::DOCK_SLOTS;
use crate::numeric;
use crate::numeric::Fit;
use crate::render::prim::{fill_rect, stroke_rect};

use super::panel_layout::{InfoLayout, PanelGeometry, measure_info};

const CARD_RIGHT_INSET: f32 = 12.0;
const CARD_LEFT_INSET: f32 = 8.0;
const RALLY_ICON_SIZE: f32 = 24.0;
const RALLY_CONTEXT_WIDTH: f32 = RALLY_ICON_SIZE + 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PanelPacking {
    right: f32,
    available: f32,
    per_row: usize,
    rally_count: usize,
    band_h: f32,
    top: f32,
    hides_minimap: bool,
}

/// The info column's width, then a command tile's width, height, and gap.
/// Tiles are sized so a Harvester carrying scrap (seven verbs), or rally
/// controls beside five products, fill one row at the 1133×744 layout
/// floor.
fn card_metrics(viewport: Vec2, scale: f32) -> (f32, f32, f32, f32) {
    let small = viewport.x / scale < 800.0 || viewport.y / scale < 500.0;
    let left = if small { 204.0 } else { 228.0 };
    let (width, height) = if small { (66.0, 76.0) } else { (84.0, 72.0) };
    (left * scale, width * scale, height * scale, 6.0 * scale)
}

fn rally_group_width(card_width: f32, scale: f32) -> f32 {
    RALLY_CONTEXT_WIDTH * scale
        + card_width.max((2.0 * crate::theme::MIN_TOUCH_TARGET + 6.0) * scale)
}

fn panel_packing_at_right(
    viewport: Vec2,
    scale: f32,
    right: f32,
    cards_shown: usize,
    rally_count: usize,
    hides_minimap: bool,
) -> PanelPacking {
    let (cards_x, card_w, card_h, gap) = card_metrics(viewport, scale);
    let first_width = if rally_count > 0 {
        rally_group_width(card_w, scale)
    } else {
        card_w
    };
    let available =
        (right - cards_x - (CARD_LEFT_INSET + CARD_RIGHT_INSET) * scale).max(first_width);
    let per_row = 1 + numeric::to_usize(((available - first_width) / (card_w + gap)).floor());
    let cards_h = if cards_shown == 0 {
        0.0
    } else {
        grouped_card_rows(cards_shown, rally_count, per_row) as f32 * (card_h + 4.0 * scale)
    };
    let band_h = (16.0 * scale + cards_h).max(72.0 * scale);
    PanelPacking {
        right,
        available,
        per_row,
        rally_count,
        band_h,
        top: viewport.y - band_h,
        hides_minimap,
    }
}

fn panel_packing(
    viewport: Vec2,
    minimap: Rect,
    scale: f32,
    cards_len: usize,
    rally_count: usize,
) -> PanelPacking {
    let cards_shown = cards_len.min(16);
    let reserved_right = if minimap.w > 0.0 {
        (minimap.x - 8.0 * scale).max(300.0 * scale).min(viewport.x)
    } else {
        viewport.x
    };
    let max_band_h = (viewport.y - crate::layout::TOP_BAR_H * scale).max(0.0);
    let packing = panel_packing_at_right(
        viewport,
        scale,
        reserved_right,
        cards_shown,
        rally_count,
        false,
    );
    if packing.band_h > max_band_h && reserved_right < viewport.x {
        panel_packing_at_right(viewport, scale, viewport.x, cards_shown, rally_count, true)
    } else {
        packing
    }
}

fn action_panel_rect(packing: PanelPacking, left: f32, right: f32, has_cards: bool) -> Rect {
    if has_cards {
        Rect::new(left, packing.top, right - left, packing.band_h)
    } else {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    }
}

fn grouped_card_slot(index: usize, rally_count: usize, per_row: usize) -> (usize, usize, bool) {
    let per_row = per_row.max(1);
    if rally_count == 0 {
        return (index / per_row, index % per_row, false);
    }
    if index < rally_count {
        return (0, 0, false);
    }
    let production_index = index - rally_count;
    if per_row == 1 {
        return (1 + production_index, 0, false);
    }
    let production_columns = per_row - 1;
    (
        production_index / production_columns,
        1 + production_index % production_columns,
        true,
    )
}

fn grouped_card_rows(shown: usize, rally_count: usize, per_row: usize) -> usize {
    if rally_count == 0 {
        shown.div_ceil(per_row.max(1)).max(1)
    } else if per_row <= 1 {
        1 + shown - rally_count
    } else {
        (shown - rally_count).div_ceil(per_row - 1).max(1)
    }
}

fn rally_card_count(cards: &[crate::panel::Card]) -> usize {
    use crate::panel::CardAction;
    cards
        .iter()
        .take(16)
        .take_while(|card| matches!(card.action, CardAction::ArmRally | CardAction::ClearRally))
        .count()
}

fn grouped_card_gap(
    shown: usize,
    rally_count: usize,
    per_row: usize,
    available: f32,
    card_width: f32,
    ordinary_gap: f32,
) -> f32 {
    let per_row = per_row.max(1);
    if rally_count == 0 || rally_count >= shown || per_row == 1 {
        return 0.0;
    }
    let occupied_columns = 1 + (shown - rally_count).min(per_row - 1);
    let ordinary_width = occupied_columns as f32 * card_width
        + occupied_columns.saturating_sub(1) as f32 * ordinary_gap;
    (available - ordinary_width).clamp(0.0, 10.0 * card_width / 66.0)
}

fn command_card_geometry(
    viewport: Vec2,
    scale: f32,
    packing: PanelPacking,
    cards: &[crate::panel::Card],
) -> (Vec<Rect>, f32) {
    let (left, width, height, gap) = card_metrics(viewport, scale);
    let shown = cards.len().min(16);
    let rally_count = packing.rally_count;
    let section_gap = grouped_card_gap(
        shown,
        rally_count,
        packing.per_row,
        packing.available - (rally_group_width(width, scale) - width),
        width,
        gap,
    );
    let slots: Vec<Rect> = (0..shown)
        .map(|index| {
            let (row, column, after_rally) = grouped_card_slot(index, rally_count, packing.per_row);
            let group_width = rally_group_width(width, scale);
            let rally_width = ((group_width
                - RALLY_CONTEXT_WIDTH * scale
                - gap * rally_count.saturating_sub(1) as f32)
                / rally_count.max(1) as f32)
                .max(crate::theme::MIN_TOUCH_TARGET * scale);
            let rally = index < rally_count;
            Rect::new(
                left + CARD_LEFT_INSET * scale
                    + column as f32 * (width + gap)
                    + if after_rally {
                        section_gap + group_width - width
                    } else {
                        0.0
                    }
                    + if rally {
                        RALLY_CONTEXT_WIDTH * scale + index as f32 * (rally_width + gap)
                    } else {
                        0.0
                    },
                packing.top + 10.0 * scale + row as f32 * (height + 4.0 * scale),
                if rally { rally_width } else { width },
                height,
            )
        })
        .collect();
    let band_width = if shown == 0 {
        left
    } else {
        slots
            .iter()
            .map(|rect| rect.x + rect.w)
            .fold(left, f32::max)
            + CARD_RIGHT_INSET * scale
    };
    (slots, band_width.min(packing.right))
}

fn rally_context_rect(first_button: Rect, scale: f32) -> Rect {
    let size = RALLY_ICON_SIZE * scale;
    Rect::new(
        first_button.x - RALLY_CONTEXT_WIDTH * scale,
        first_button.y + (first_button.h - size) * 0.5,
        size,
        size,
    )
}

fn draw_rally_control(
    card: &crate::panel::Card,
    verb: Option<crate::panel::RallyVerb>,
    rect: Rect,
    scale: f32,
) {
    let label = match card.action {
        crate::panel::CardAction::ClearRally => "Clear",
        _ => verb.unwrap_or(crate::panel::RallyVerb::Set).label(),
    };
    let color = if card.enabled {
        TEXT_PRIMARY
    } else {
        TEXT_DISABLED
    };
    for (text, size, baseline, tint) in [
        (label, 14.0, rect.y + rect.h * 0.5 - scale, color),
        (
            card.hotkey.as_str(),
            11.0,
            rect.y + rect.h * 0.5 + 15.0 * scale,
            if card.enabled {
                TEXT_SECONDARY
            } else {
                TEXT_DISABLED
            },
        ),
    ] {
        let available = rect.w - 6.0 * scale;
        let mut font_size = size * scale;
        let mut measured = measure_text(text, None, numeric::font_size(font_size), 1.0);
        while measured.width > available && font_size > crate::theme::Type::Caption.at(scale) {
            font_size -= 0.5 * scale;
            measured = measure_text(text, None, numeric::font_size(font_size), 1.0);
        }
        if measured.width <= available {
            draw_text(
                text,
                rect.x + (rect.w - measured.width) * 0.5,
                baseline,
                font_size,
                tint,
            );
        }
    }
}

/// The words on a strip chip's face: its count alone when chips are
/// small.
fn roster_face(chip: &crate::panel::RosterChip, small: bool) -> std::borrow::Cow<'_, str> {
    if small {
        chip.count.to_string().into()
    } else {
        chip.card.title.as_str().into()
    }
}

fn card_title_lines(title: &str, measure: impl Fn(&str) -> f32, width: f32) -> Vec<String> {
    if measure(title) <= width || !title.contains('-') {
        return wrap_words(title, measure, width);
    }
    // A hyphenated word may also break after its hyphen; halves that stay
    // on one line keep no space between them.
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in title.split_whitespace() {
        for (index, part) in word.split_inclusive('-').enumerate() {
            let glue = if line.is_empty() || index > 0 {
                ""
            } else {
                " "
            };
            let candidate = format!("{line}{glue}{part}");
            if !line.is_empty() && measure(&candidate) > width {
                lines.push(std::mem::take(&mut line));
                line = part.to_string();
            } else {
                line = candidate;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The title row's select-all button, framed like a strip chip. While pad
/// mode names a controller (`pad`), its RT leads the words, so they read
/// beside the button that presses it, inside the same rect.
fn draw_select_all(
    sprites: &Sprites,
    all: &crate::panel::SelectAll,
    rect: Rect,
    hovered: bool,
    pad: Option<aftermine_protocol::PadFamily>,
    s: f32,
) {
    let (fill, stroke, edge) = if hovered {
        (
            crate::theme::CARD_HOVER,
            crate::theme::Stroke::Focus,
            crate::theme::EDGE_FOCUS,
        )
    } else {
        (
            crate::theme::CHIP,
            crate::theme::Stroke::Edge,
            crate::theme::EDGE_CHIP,
        )
    };
    fill_rect(rect, fill);
    stroke_rect(rect, stroke.at(s), edge);
    let size = crate::theme::Type::Small.at(s);
    let baseline = rect.y + rect.h * 0.5 + size * 0.35;
    let measure = |text: &str| measure_text(text, None, numeric::font_size(size), 1.0).width;
    let Some(family) = pad else {
        let label = all.label();
        let x = rect.x + (rect.w - measure(&label)) * 0.5;
        draw_text(&label, x, baseline, size, TEXT_PRIMARY);
        return;
    };
    let label = format!(" {}", all.label());
    let width = crate::hint_line::icon_width(size) + measure(&label);
    let x = rect.x + (rect.w - width) * 0.5;
    let icon = crate::pad::button_icon(family, aftermine_protocol::PadButton::RightTrigger);
    let x = x + crate::hint_line::draw_icon(sprites, icon, x, baseline, size, TEXT_PRIMARY);
    draw_text(&label, x, baseline, size, TEXT_PRIMARY);
}

/// Packs every visible queue chip between the command band and
/// `ceiling`: a single column when it fits, otherwise a grid (a full
/// eight-slot production queue becomes 2×4 in a 640×400 window), so no
/// paid, cancelable job hides behind an overflow label.
fn queue_grid(
    queue_len: usize,
    panel_top: f32,
    ceiling: f32,
    scale: f32,
    header: f32,
) -> (Rect, [Rect; DOCK_SLOTS], usize) {
    queue_grid_with_width(queue_len, panel_top, ceiling, scale, 44.0, header)
}

/// The highest a dock stacked at the window's left may reach: under the
/// top bar, and under a lesson or tip card holding that corner, which
/// draws over the HUD and takes the presses that land on it.
fn dock_ceiling(game: &crate::game::Scene<'_>, scale: f32) -> f32 {
    let bar = crate::layout::TOP_BAR_H * scale;
    game.corner_card
        .map_or(bar, |card| bar.max(card.bottom() + 6.0 * scale))
}

/// The Stop button's own plate above the dock: the button and its inset.
const STOP_PLATE: f32 = 60.0;
/// The row under a full dock's chips that counts the entries past them.
const MORE_ROW: f32 = 16.0;
/// The gap between the Stop plate and the dock beneath it.
const STOP_GAP: f32 = 6.0;

/// `header` reserves logical px at the dock's top, above its label.
fn queue_grid_with_width(
    queue_len: usize,
    panel_top: f32,
    ceiling: f32,
    scale: f32,
    width: f32,
    header: f32,
) -> (Rect, [Rect; DOCK_SLOTS], usize) {
    let zero = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut slots = [zero; DOCK_SLOTS];
    let count = queue_len.min(slots.len());
    if count == 0 {
        return (zero, slots, 0);
    }
    let (size, gap) = (44.0 * scale, 4.0 * scale);
    let label_h = 24.0 * scale;
    let header = header * scale;
    let more = if queue_len > count {
        MORE_ROW * scale
    } else {
        0.0
    };
    let available = (panel_top - ceiling - header - label_h - more - 2.0 * scale).max(size);
    let max_rows = numeric::to_usize(((available + gap) / (size + gap)).floor()).max(1);
    let columns = count.div_ceil(max_rows).max(1);
    let rows = count.div_ceil(columns);
    let slot_width = width * scale;
    let width = 16.0 * scale + columns as f32 * slot_width + columns.saturating_sub(1) as f32 * gap;
    let height =
        header + label_h + rows as f32 * size + rows.saturating_sub(1) as f32 * gap + 2.0 * scale;
    let dock = Rect::new(0.0, panel_top - height, width, height);
    for (index, slot) in slots.iter_mut().take(count).enumerate() {
        let row = index / columns;
        let column = index % columns;
        *slot = Rect::new(
            8.0 * scale + column as f32 * (slot_width + gap),
            dock.y + header + label_h + row as f32 * (size + gap),
            slot_width,
            size,
        );
    }
    (dock, slots, count)
}

fn collective_queue_grid(
    count: usize,
    top: f32,
    ceiling: f32,
    scale: f32,
    viewport_width: f32,
    header: f32,
) -> (Rect, [Rect; DOCK_SLOTS], usize) {
    if viewport_width / scale < 800.0 {
        return queue_grid_with_width(count, top, ceiling, scale, 64.0, header);
    }
    let wide = queue_grid_with_width(count, top, ceiling, scale, 170.0, header);
    if wide.0.right() <= viewport_width {
        wide
    } else {
        queue_grid_with_width(count, top, ceiling, scale, 64.0, header)
    }
}

fn queue_label_width(panel: &crate::panel::Panel, measure: impl Fn(&str) -> f32) -> f32 {
    use crate::panel::{CardAction, CardIcon};

    if !panel.queue_groups.is_empty() {
        return measure(&panel.queue_label);
    }
    let ticks = panel
        .queue
        .iter()
        .filter_map(|card| match (card.action, card.icon) {
            (CardAction::CancelQueue(..), CardIcon::Unit(kind)) => Some(kind.stats().train_ticks),
            (CardAction::CancelRound(..), CardIcon::Round(kind)) => {
                kind.magazine().map(|stats| stats.round_ticks)
            }
            _ => None,
        });
    if ticks.clone().next().is_none() {
        return measure(&panel.queue_label);
    }

    // Reserve from complete jobs, not their changing progress. Digit widths
    // can differ, and a completed head may stay queued while its exit is blocked.
    let digit_width = (0..=9)
        .map(|digit| measure(&digit.to_string()))
        .fold(0.0_f32, f32::max);
    let time_width = |ticks: u32| {
        let seconds = ticks.div_ceil(aftermine_sim::TICKS_PER_SECOND).max(1);
        let digits = seconds.ilog10() + 1;
        (digits + 1) as f32 * digit_width + measure(".s")
    };
    time_width(ticks.sum())
        .max(measure("Ready"))
        .max(measure(&panel.queue_label))
}

/// The grouped palette's Back column, which spans both rows.
const PALETTE_BACK_WIDTH: f32 = 60.0;
/// A grouped palette cell's height.
const PALETTE_CELL_H: f32 = 76.0;
/// The narrowest grouped cell. Below it, only under the layout floor,
/// the palette falls back to a flat grid.
const PALETTE_MIN_CELL: f32 = 72.0;

/// Palette columns a category spans: its buildings fill two rows across.
fn category_columns(buildings: usize) -> usize {
    buildings.div_ceil(2)
}

/// The construction palette for `count` cards: every building in its
/// category's columns with Back last. Grouped, Back stands in its own
/// column at the band's left edge and each category fills two rows under
/// its header. A touch-only build holds the bottom-left card clear of the
/// rounded screen corner.
fn catalog_geometry(
    viewport: Vec2,
    scale: f32,
    minimap: Rect,
    count: usize,
    touch_only: bool,
) -> (Rect, Vec<Rect>, bool) {
    let left = 10.0 * scale;
    let corner = if touch_only {
        crate::layout::TOUCH_CORNER_INSET * scale
    } else {
        left
    };
    let right = if minimap.w > 0.0 {
        minimap.x - 8.0 * scale
    } else {
        viewport.x
    };
    let available = (right - left - 10.0 * scale).max(136.0 * scale);
    let gap = 4.0 * scale;
    let header = 28.0 * scale;
    let categories = &crate::action::BUILD_CATEGORIES;
    let buildings: usize = categories.iter().map(|(_, kinds)| kinds.len()).sum();
    let columns: usize = categories
        .iter()
        .map(|(_, kinds)| category_columns(kinds.len()))
        .sum();
    let back_w = PALETTE_BACK_WIDTH * scale;
    let cell_w = ((available - back_w - columns as f32 * gap) / columns as f32).min(140.0 * scale);
    if count == buildings + 1 && cell_w >= PALETTE_MIN_CELL * scale {
        let cell_h = PALETTE_CELL_H * scale;
        let height = header + 2.0 * cell_h + gap + 8.0 * scale;
        let top = viewport.y - height;
        let first_x = left + back_w + gap;
        let mut slots = Vec::with_capacity(count);
        let mut column = 0;
        for (_, kinds) in categories {
            let span = category_columns(kinds.len());
            for i in 0..kinds.len() {
                slots.push(Rect::new(
                    first_x + (column + i % span) as f32 * (cell_w + gap),
                    top + header + (i / span) as f32 * (cell_h + gap),
                    cell_w,
                    cell_h,
                ));
            }
            column += span;
        }
        // Back gives up its corner side, so every other cell keeps its place.
        slots.push(Rect::new(
            corner,
            top + header,
            left + back_w - corner,
            2.0 * cell_h + gap,
        ));
        let band = Rect::new(
            0.0,
            top,
            first_x + columns as f32 * (cell_w + gap) - gap + 10.0 * scale,
            height,
        );
        return (band, slots, true);
    }
    let left = corner;
    let available = (right - left - 10.0 * scale).max(136.0 * scale);
    let max_columns = numeric::to_usize(
        ((available + 4.0 * scale) / (116.0 * scale))
            .floor()
            .max(1.0),
    );
    let rows = count.max(1).div_ceil(max_columns);
    let columns = count.max(1).div_ceil(rows);
    let width = ((available - columns.saturating_sub(1) as f32 * 4.0 * scale) / columns as f32)
        .min(140.0 * scale);
    let row_h = 58.0 * scale;
    let height = header + count.div_ceil(columns) as f32 * row_h + 8.0 * scale;
    let band = Rect::new(
        0.0,
        viewport.y - height,
        left + columns as f32 * (width + 4.0 * scale) + 6.0 * scale,
        height,
    );
    let slots = (0..count)
        .map(|i| {
            Rect::new(
                left + (i % columns) as f32 * (width + 4.0 * scale),
                band.y + header + (i / columns) as f32 * row_h,
                width,
                row_h - 4.0 * scale,
            )
        })
        .collect();
    (band, slots, false)
}

/// A construction category's header: its name, a marker while open,
/// and on desktop the keys that reach it.
fn category_label(
    label: &str,
    open: bool,
    key: &str,
    palette_key: Option<&str>,
    keyless: bool,
) -> String {
    if open {
        format!("{label} *")
    } else if keyless {
        label.to_string()
    } else if let Some(palette_key) = palette_key {
        format!("{label} [{palette_key} > {key}]")
    } else {
        format!("{label} [{key}]")
    }
}

/// A category's header as `category_label` words it, without its keys
/// where `fits` refuses them: Caption is the smallest type, and the cells
/// and their tooltips name the keys too.
fn category_heading(
    label: &str,
    open: bool,
    key: &str,
    palette_key: Option<&str>,
    keyless: bool,
    fits: impl Fn(&str) -> bool,
) -> String {
    let heading = category_label(label, open, key, palette_key, keyless);
    if fits(&heading) {
        heading
    } else {
        category_label(label, open, key, palette_key, true)
    }
}

/// What pressing a drawn card does: its own action while enabled, an
/// explanation while disabled.
fn published_action(card: &crate::panel::Card) -> crate::panel::CardAction {
    if card.enabled {
        card.action
    } else {
        crate::panel::CardAction::Refused
    }
}

fn draw_catalog(
    panel: &crate::panel::Panel,
    input: &InputState,
    bindings: &crate::action::BindingMap,
    (band, slots, grouped): (Rect, &[Rect], bool),
    draw_icon: &impl Fn(Rect, &crate::panel::CardIcon, Color),
) {
    use crate::panel::CardAction;
    let s = ui_scale();
    fill_rect(band, crate::theme::SURFACE_BAND);
    draw_rectangle(band.x, band.y, band.w, s, crate::theme::EDGE_WARM);
    if !grouped {
        crate::typography::draw(
            "BUILD",
            slots.first().map_or(band.x, |slot| slot.x) + 2.0 * s,
            band.y + 20.0 * s,
            crate::theme::Type::Body.at(s),
            TEXT_PRIMARY,
        );
    }
    if grouped {
        let mut first = 0;
        for (category, (name, kinds)) in crate::action::BUILD_CATEGORIES.iter().enumerate() {
            let cells = &slots[first..first + kinds.len()];
            first += kinds.len();
            let key = bindings.label(crate::action::Action::BuildCategory(category.fit::<u8>()));
            let palette_key = bindings.label(crate::action::Action::ToggleBuildPalette);
            let x = cells[0].x + 5.0 * s;
            let right = cells.iter().map(Rect::right).fold(x, f32::max);
            let caption = crate::theme::Type::Caption.at(s);
            let label = category_heading(
                &name.to_uppercase(),
                input.build_category == Some(category.fit::<u8>()),
                &key,
                input
                    .build_category
                    .is_some()
                    .then_some(palette_key.as_str()),
                !crate::platform::hands().keys,
                |label| crate::typography::measure(label, caption).width <= right - x - 2.0 * s,
            );
            crate::typography::draw(&label, x, band.y + 20.0 * s, caption, TEXT_SECONDARY);
        }
    }
    let hovered = slots.iter().position(|rect| hovers(input, *rect));
    for (i, (card, &rect)) in panel.cards.iter().zip(slots).enumerate() {
        let armed =
            matches!(card.action, CardAction::ArmBuild(kind) if input.placing_kind() == Some(kind));
        let hot = hovered == Some(i);
        fill_rect(
            rect,
            if armed {
                crate::theme::CARD_ARMED
            } else if hot {
                crate::theme::CARD_HOVER
            } else {
                crate::theme::CARD_IDLE
            },
        );
        if armed || hot {
            draw_rectangle(
                rect.x,
                rect.y,
                2.0 * s,
                rect.h,
                if armed { SCRAP_COLOR } else { TEXT_PRIMARY },
            );
        }
        if grouped {
            draw_palette_cell(card, rect, s, draw_icon);
            continue;
        }
        draw_icon(
            Rect::new(rect.x + 5.0 * s, rect.y + 9.0 * s, 34.0 * s, 34.0 * s),
            &card.icon,
            if card.enabled {
                WHITE
            } else {
                Color::new(1.0, 1.0, 1.0, 0.5)
            },
        );
        let names = card_title_lines(
            &card.title,
            |text| {
                measure_text(
                    text,
                    None,
                    numeric::font_size(crate::theme::Type::Body.at(s)),
                    1.0,
                )
                .width
            },
            rect.w - 49.0 * s,
        );
        for (line, name) in names.iter().enumerate() {
            draw_text(
                name,
                rect.x + 44.0 * s,
                rect.y + (if names.len() > 1 { 16.0 } else { 22.0 } + line as f32 * 14.0) * s,
                crate::theme::Type::Body.at(s),
                if card.enabled {
                    TEXT_PRIMARY
                } else {
                    TEXT_SECONDARY
                },
            );
        }
        if let Some(cost) = card.cost {
            crate::typography::draw(
                &cost.to_string(),
                rect.x + 44.0 * s,
                rect.y + 42.0 * s,
                crate::theme::Type::Small.at(s),
                if card.enabled {
                    SCRAP_COLOR
                } else {
                    TEXT_DISABLED
                },
            );
        }
        let key_width = measure_text(
            &card.hotkey,
            None,
            numeric::font_size(crate::theme::Type::Caption.at(s)),
            1.0,
        )
        .width;
        draw_text(
            &card.hotkey,
            rect.x + rect.w - key_width - 5.0 * s,
            rect.y + 42.0 * s,
            crate::theme::Type::Caption.at(s),
            TEXT_SECONDARY,
        );
    }
}

/// One grouped palette cell: the building over its name, with the cost and
/// hotkey in a footer. A card without a cost, like Back, centers its icon
/// and name instead.
fn draw_palette_cell(
    card: &crate::panel::Card,
    rect: Rect,
    s: f32,
    draw_icon: &impl Fn(Rect, &crate::panel::CardIcon, Color),
) {
    let icon = 30.0 * s;
    let width = rect.w - 8.0 * s;
    let size = crate::typography::fit(
        &card.title,
        crate::theme::Type::Body.at(s),
        width,
        crate::theme::Type::Caption,
    );
    let measure = |text: &str| measure_text(text, None, numeric::font_size(size), 1.0);
    let names = card_title_lines(&card.title, |text| measure(text).width, width);
    let ascent = names
        .iter()
        .map(|name| measure(name).offset_y)
        .fold(0.0, f32::max);
    let line_h = size + 2.0 * s;
    let stack = icon + 4.0 * s + names.len() as f32 * line_h;
    let top = if card.cost.is_some() {
        rect.y + 6.0 * s
    } else {
        rect.y + ((rect.h - stack) * 0.5).max(4.0 * s)
    };
    draw_icon(
        Rect::new(rect.x + (rect.w - icon) * 0.5, top, icon, icon),
        &card.icon,
        if card.enabled {
            WHITE
        } else {
            Color::new(1.0, 1.0, 1.0, 0.5)
        },
    );
    for (line, name) in names.iter().enumerate() {
        draw_text(
            name,
            rect.x + (rect.w - measure(name).width) * 0.5,
            top + icon + 4.0 * s + ascent + line as f32 * line_h,
            size,
            if card.enabled {
                TEXT_PRIMARY
            } else {
                TEXT_SECONDARY
            },
        );
    }
    let footer = rect.y + rect.h - 6.0 * s;
    if let Some(cost) = card.cost {
        crate::typography::draw(
            &cost.to_string(),
            rect.x + 5.0 * s,
            footer,
            crate::theme::Type::Small.at(s),
            if card.enabled {
                SCRAP_COLOR
            } else {
                TEXT_DISABLED
            },
        );
    }
    let caption = crate::theme::Type::Caption.at(s);
    let key_width = measure_text(&card.hotkey, None, numeric::font_size(caption), 1.0).width;
    draw_text(
        &card.hotkey,
        rect.x + rect.w - key_width - 5.0 * s,
        footer,
        caption,
        TEXT_SECONDARY,
    );
}

/// The AUTO tag in a command tile's top-right corner, level with its hotkey:
/// lit while every selected unit runs the card's automatic mode, dim while
/// some do, faint while none do, so the mode reads before the tooltip.
fn draw_autocast_tag(tile: Rect, state: crate::panel::AutocastState, scale: f32) {
    use crate::panel::AutocastState;
    let size = crate::theme::Type::Caption.at(scale);
    let width = measure_text("AUTO", None, numeric::font_size(size), 1.0).width;
    let color = match state {
        AutocastState::On => crate::theme::TEXT_ACCENT,
        AutocastState::Mixed => crate::theme::TEXT_SECONDARY,
        AutocastState::Off => crate::theme::TEXT_DISABLED,
    };
    draw_text(
        "AUTO",
        tile.x + tile.w - width - 4.0 * scale,
        tile.y + TILE_HOTKEY_BASELINE * scale,
        size,
        color,
    );
}

/// The baseline of a command tile's hotkey, below the tile's top edge.
const TILE_HOTKEY_BASELINE: f32 = 13.0;

/// The highest a command tile's icon may sit: clear of the hotkey label
/// when it reaches into the icon's column, as a modifier chord or a second
/// binding does, else just inside the tile's top edge.
fn tile_icon_floor(tile: Rect, icon_size: f32, hotkey: Option<Rect>, scale: f32) -> f32 {
    let icon_x = tile.x + (tile.w - icon_size) * 0.5;
    match hotkey {
        Some(label) if label.right() + 2.0 * scale > icon_x => label.bottom() + 6.0 * scale,
        _ => tile.y + 4.0 * scale,
    }
}

/// How much taller than the band an info column may be before the two
/// keep separate top edges.
const FLUSH_SLACK: f32 = 12.0;

/// Grows the band to an info column that overtops it by less than
/// [`FLUSH_SLACK`], so they share one top edge instead of a sliver of a
/// step. A real difference keeps its open notch for the world.
fn flush_band(
    packing: PanelPacking,
    info_height: f32,
    viewport_h: f32,
    scale: f32,
) -> PanelPacking {
    let excess = info_height - packing.band_h;
    if excess > 0.0 && excess < FLUSH_SLACK * scale {
        PanelPacking {
            band_h: info_height,
            top: viewport_h - info_height,
            ..packing
        }
    } else {
        packing
    }
}

fn selection_info_rect(viewport: Vec2, width: f32, content_height: f32, actions: Rect) -> Rect {
    let height = content_height.max(actions.h);
    Rect::new(0.0, viewport.y - height, width, height)
}

/// Where the command panel's pieces sit this frame, laid out without
/// drawing so input hit-tests exactly what the next draw shows.
pub(crate) enum PanelLayout {
    /// The construction palette: its band, one slot per card, whether
    /// the slots stand in their category columns beside Back, and the
    /// builder's orders dock above the band.
    Catalog {
        band: Rect,
        slots: Vec<Rect>,
        grouped: bool,
        dock: Option<DockLayout>,
    },
    /// A selection's information region, command cards, roster and
    /// orders dock.
    Selection(Box<SelectionLayout>),
}

pub(crate) struct SelectionLayout {
    small: bool,
    cards_x: f32,
    measured: InfoLayout,
    info_rect: Rect,
    /// The selection's picture beside its title.
    portrait: Rect,
    action_rect: Rect,
    card_rects: Vec<Rect>,
    rally_count: usize,
    select_all: Option<Rect>,
    roster: Vec<Rect>,
    dock: Option<DockLayout>,
    drones: Option<DroneColumnLayout>,
    hides_minimap: bool,
}

/// The orders dock: its plate, the list beneath the Stop plate, the Stop
/// plate and button, the visible chips, and how many more are hidden.
pub(crate) struct DockLayout {
    dock: Rect,
    list: Rect,
    stop: Option<(Rect, Rect)>,
    chips: Vec<Rect>,
    hidden: usize,
    /// Where the count of hidden entries sits: the foot of the chips' own
    /// rows, above the QUEUE toggle a touch build stretches the plate over.
    more_baseline: f32,
}

/// The drone column: its plate and the chips it shows.
pub(crate) struct DroneColumnLayout {
    plate: Rect,
    chips: Vec<Rect>,
}

/// Whether the panel is the construction palette rather than a selection.
fn is_catalog(panel: &crate::panel::Panel) -> bool {
    use crate::panel::CardAction;
    panel
        .cards
        .iter()
        .any(|card| matches!(card.action, CardAction::ArmBuild(_)))
        && panel.cards.iter().all(|card| {
            matches!(
                card.action,
                CardAction::ArmBuild(_) | CardAction::ClosePalette
            )
        })
}

/// Lays out the command panel for `panel` in `env`'s window.
pub(crate) fn layout_panel(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    panel: &crate::panel::Panel,
    env: super::hud::HudEnv,
    measure: super::hud::Measure<'_>,
) -> PanelLayout {
    let (viewport, s) = (env.viewport, env.ui);
    let mini = minimap_rect_scaled(
        game.state.map().width(),
        game.state.map().height(),
        viewport,
        s,
    );
    if is_catalog(panel) {
        let (band, slots, grouped) = catalog_geometry(
            viewport,
            s,
            mini,
            panel.cards.len(),
            crate::platform::PLATFORM.touch_only(),
        );
        let dock = (!panel.queue.is_empty() || panel.stop.is_some()).then(|| {
            layout_dock(
                super::chrome::queue_toggle_shown(
                    game,
                    input,
                    crate::platform::PLATFORM.touch_only(),
                ),
                dock_ceiling(game, s),
                panel,
                viewport,
                s,
                band.y,
                measure,
            )
        });
        return PanelLayout::Catalog {
            band,
            slots,
            grouped,
            dock,
        };
    }
    let small = viewport.x / s < 800.0 || viewport.y / s < 500.0;
    let packing = panel_packing(
        viewport,
        mini,
        s,
        panel.cards.len(),
        rally_card_count(&panel.cards),
    );
    let (cards_x, _, _, _) = card_metrics(viewport, s);
    let measured = measure_info(
        panel,
        cards_x,
        s,
        small,
        crate::platform::PLATFORM.touch_only(),
        measure,
    );
    let packing = flush_band(packing, measured.height, viewport.y, s);
    let (card_rects, cards_w) = command_card_geometry(viewport, s, packing, &panel.cards);
    let action_rect = action_panel_rect(packing, cards_x, cards_w, !panel.cards.is_empty());
    let info_rect = selection_info_rect(viewport, cards_x, measured.height, action_rect);
    let top = info_rect.y;
    let portrait = Rect::new(
        10.0 * s,
        top + measured.header_pad + 6.0 * s,
        32.0 * s,
        32.0 * s,
    );
    // The select-all button's right edge lines up with the stat values,
    // its middle with the title's first line.
    let select_all = measured.select_all.map(|size| {
        Rect::new(
            cards_x - 12.0 * s - size.x,
            top + measured.header_pad + 21.0 * s - size.y * 0.5,
            size.x,
            size.y,
        )
    });
    let (size, gap) = (measured.roster_size, 4.0 * s);
    let roster = (0..panel.roster.len())
        .map(|index| {
            let row = index / measured.roster_columns;
            let column = index % measured.roster_columns;
            Rect::new(
                8.0 * s + column as f32 * (size + gap),
                top + measured.roster_y + row as f32 * (size + gap),
                size,
                size,
            )
        })
        .collect();
    let dock = (!panel.queue.is_empty() || panel.stop.is_some()).then(|| {
        layout_dock(
            super::chrome::queue_toggle_shown(game, input, crate::platform::PLATFORM.touch_only()),
            dock_ceiling(game, s),
            panel,
            viewport,
            s,
            top,
            measure,
        )
    });
    let drones = panel
        .drones
        .as_ref()
        .map(|column| layout_drone_column(game, input, column, dock.as_ref(), s, top, measure));
    PanelLayout::Selection(Box::new(SelectionLayout {
        small,
        cards_x,
        measured,
        info_rect,
        portrait,
        action_rect,
        card_rects,
        rally_count: packing.rally_count,
        select_all,
        roster,
        dock,
        drones,
        hides_minimap: packing.hides_minimap,
    }))
}

/// The drone column stands on the band just right of the orders column,
/// which the orders dock fills or, on touch, the QUEUE toggle keeps, so a
/// host's drones never cover its program.
fn layout_drone_column(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    column: &crate::panel::DroneColumn,
    dock: Option<&DockLayout>,
    s: f32,
    top: f32,
    measure: super::hud::Measure<'_>,
) -> DroneColumnLayout {
    let toggle =
        super::chrome::queue_toggle_shown(game, input, crate::platform::PLATFORM.touch_only());
    let orders_right = dock.map_or(0.0, |dock| dock.dock.right()).max(if toggle {
        (16.0 + crate::theme::MIN_TOUCH_TARGET) * s
    } else {
        0.0
    });
    let x = if orders_right > 0.0 {
        orders_right + 4.0 * s
    } else {
        0.0
    };
    let (mut plate, slots, n) = queue_grid(column.chips.len(), top, dock_ceiling(game, s), s, 0.0);
    let label_width = measure(
        super::hud::Face::Body,
        &column.label,
        crate::theme::Type::Small.at(s),
    ) + 16.0 * s;
    plate.x = x;
    plate.w = plate.w.max(label_width);
    let chips = slots
        .iter()
        .take(n)
        .map(|slot| Rect::new(slot.x + x, slot.y, slot.w, slot.h))
        .collect();
    DroneColumnLayout { plate, chips }
}

/// Production ghosts or order chips, stacked above the band's left
/// corner so the band itself stays short.
/// `toggle_below` lifts it clear of the QUEUE toggle, and its top stays
/// under `ceiling`.
fn layout_dock(
    toggle_below: bool,
    ceiling: f32,
    panel: &crate::panel::Panel,
    viewport: Vec2,
    s: f32,
    top: f32,
    measure: super::hud::Measure<'_>,
) -> DockLayout {
    let zero = Rect::new(0.0, 0.0, 0.0, 0.0);
    let floor = if toggle_below {
        top - super::chrome::queue_dock_lift(s)
    } else {
        top
    };
    let header = if panel.stop.is_some() {
        STOP_PLATE + STOP_GAP
    } else {
        0.0
    };
    let (mut grid_dock, grid_slots, n) = if panel.queue.is_empty() {
        // A lone Stop plate: some other selected unit is busy.
        let side = STOP_PLATE * s;
        (
            Rect::new(0.0, floor - side, side, side),
            [zero; DOCK_SLOTS],
            0,
        )
    } else if panel.queue_groups.is_empty() {
        queue_grid(panel.queue.len(), floor, ceiling, s, header)
    } else {
        collective_queue_grid(panel.queue.len(), floor, ceiling, s, viewport.x, header)
    };
    if !panel.queue.is_empty() {
        let queue_label_width = queue_label_width(panel, |text| {
            measure(
                super::hud::Face::Body,
                text,
                crate::theme::Type::Small.at(s),
            )
        }) + 16.0 * s;
        grid_dock.w = grid_dock.w.max(queue_label_width);
    }
    let mut dock = grid_dock;
    let hidden = panel.queue.len().saturating_sub(n);
    let more_h = if hidden > 0 { MORE_ROW * s } else { 0.0 };
    if more_h > 0.0 {
        dock.y -= more_h;
        dock.h += more_h;
    }
    let more_baseline = dock.bottom() - 8.0 * s;
    // The dock borders only its open top and right, resting on the
    // band. With the QUEUE toggle beneath it, the plate runs down to
    // the band so the toggle sits inside one closed column.
    if toggle_below {
        dock.h = top - dock.y;
    }
    // The list rests on the band beneath the Stop plate and its gap.
    let list = Rect::new(dock.x, dock.y + header * s, dock.w, dock.h - header * s);
    // The Stop button is as wide as the chips beneath it.
    let stop = panel.stop.as_ref().map(|_| {
        let width = if n > 0 { grid_slots[0].w } else { 44.0 * s };
        (
            Rect::new(0.0, dock.y, width + 16.0 * s, STOP_PLATE * s),
            Rect::new(8.0 * s, dock.y + 8.0 * s, width, 44.0 * s),
        )
    });
    let chips = grid_slots
        .iter()
        .take(n)
        .map(|slot| Rect::new(slot.x, slot.y - more_h, slot.w, slot.h))
        .collect();
    DockLayout {
        dock,
        list,
        stop,
        chips,
        hidden,
        more_baseline,
    }
}

impl DockLayout {
    /// Publishes the dock's plate, chips, and Stop button for `panel`.
    fn publish(&self, panel: &crate::panel::Panel, geometry: &mut PanelGeometry) {
        geometry.orders = self.dock;
        for (card, rect) in panel.queue.iter().zip(&self.chips) {
            geometry.queue_slots[geometry.queue_count] = (*rect, card.action);
            geometry.queue_count += 1;
        }
        if let (Some(card), Some((_, button))) = (&panel.stop, self.stop) {
            geometry.queue_stop = (button, card.action);
        }
    }
}

impl PanelLayout {
    /// The clickable regions this layout publishes for `panel`.
    pub(super) fn geometry(&self, panel: &crate::panel::Panel) -> PanelGeometry {
        let mut geometry = PanelGeometry::empty();
        match self {
            PanelLayout::Catalog {
                band, slots, dock, ..
            } => {
                geometry.actions = *band;
                for (i, (card, rect)) in panel.cards.iter().zip(slots).enumerate() {
                    geometry.cards[i] = (*rect, published_action(card));
                }
                geometry.card_count = panel.cards.len();
                if let Some(dock) = dock {
                    dock.publish(panel, &mut geometry);
                }
            }
            PanelLayout::Selection(selection) => {
                geometry.info = selection.info_rect;
                geometry.actions = selection.action_rect;
                geometry.hides_minimap = selection.hides_minimap;
                if let (Some(all), Some(rect)) = (&panel.select_all, selection.select_all) {
                    geometry.select_all = (rect, all.card.action);
                }
                for (chip, rect) in panel.roster.iter().zip(&selection.roster) {
                    geometry.roster_slots[geometry.roster_count] = (*rect, chip.card.action);
                    geometry.roster_count += 1;
                }
                for (card, rect) in panel.cards.iter().zip(&selection.card_rects) {
                    geometry.cards[geometry.card_count] = (*rect, published_action(card));
                    geometry.card_count += 1;
                }
                if let Some(dock) = &selection.dock {
                    dock.publish(panel, &mut geometry);
                }
                if let (Some(column), Some(layout)) = (&panel.drones, &selection.drones) {
                    geometry.drones = layout.plate;
                    for (card, rect) in column.chips.iter().zip(&layout.chips) {
                        geometry.drone_slots[geometry.drone_count] = (*rect, card.action);
                        geometry.drone_count += 1;
                    }
                }
            }
        }
        geometry
    }
}

/// Card art wears its owner's color on the same accent masks the world
/// tints, dimmed with the card.
fn portrait_shade(owner: Color, tint: Color) -> Color {
    Color::new(owner.r * tint.r, owner.g * tint.g, owner.b * tint.b, tint.a)
}

/// A building's card art. Defense art is authored as a base plus a
/// north-facing live mount; with `mount`, static cards compose the same
/// silhouette without inventing aim.
pub(crate) fn building_portrait(
    sprites: &Sprites,
    kind: aftermine_sim::BuildingKind,
    tier: u8,
    mount: bool,
    tint: Color,
    owner: Color,
) -> Vec<(Rect, Color)> {
    let mut layers = vec![
        (sprites.building_tiered(kind, tier), tint),
        (
            sprites.building_tiered_accent(kind, tier),
            portrait_shade(owner, tint),
        ),
    ];
    if mount && let Some(mount) = sprites.defense_mount(kind, tier) {
        layers.push((mount, tint));
        if let Some(accent) = sprites.defense_mount_accent(kind, tier) {
            layers.push((accent, portrait_shade(owner, tint)));
        }
    }
    layers
}

/// A unit's card art.
pub(crate) fn unit_portrait(
    sprites: &Sprites,
    kind: aftermine_sim::UnitKind,
    tint: Color,
    owner: Color,
) -> Vec<(Rect, Color)> {
    vec![
        (sprites.unit(kind), tint),
        (sprites.unit_accent(kind), portrait_shade(owner, tint)),
    ]
}

/// A card's face drawn in `dest`, tinted by `tint` and wearing `owner`'s
/// color: its sprite, or an order chip's subject under its verb badge.
pub(crate) fn draw_card_icon(
    sprites: &Sprites,
    dest: Rect,
    icon: &crate::panel::CardIcon,
    tint: Color,
    owner: Color,
) {
    use crate::panel::CardIcon;
    let blit = |dest: Rect, source: Rect, tint: Color| {
        sprites.draw_canvas(dest, &[(source, tint)]);
    };
    let shade = portrait_shade;
    let building_layers =
        |kind: aftermine_sim::BuildingKind, tier: u8, mount: bool, tint: Color, owner: Color| {
            building_portrait(sprites, kind, tier, mount, tint, owner)
        };
    let unit_layers = |kind: aftermine_sim::UnitKind, tint: Color, owner: Color| {
        unit_portrait(sprites, kind, tint, owner)
    };
    // An order chip is two composed draws: the subject's own silhouette
    // (translucent under a scaffold while its site is still rising) and
    // the verb as a corner badge on a dark plate, so the pictogram never
    // dissolves into the hull beneath it. Every other icon is one sprite.
    let (subject, verb, ghost) = match icon {
        CardIcon::Order {
            subject,
            verb,
            ghost,
        } => (subject, verb, ghost),
        CardIcon::Unit(kind) => {
            sprites.draw_portrait(dest, &unit_layers(*kind, tint, owner));
            return;
        }
        CardIcon::Building(kind, tier) => {
            sprites.draw_portrait(dest, &building_layers(*kind, *tier, true, tint, owner));
            return;
        }
        CardIcon::Round(kind) => {
            let layers = sprites.round(*kind).map_or_else(
                || building_layers(*kind, 0, true, tint, owner),
                |[body, accent]| vec![(body, tint), (accent, shade(owner, tint))],
            );
            sprites.draw_portrait(dest, &layers);
            return;
        }
        CardIcon::Verb(v) => {
            blit(dest, sprites.verb_icon(*v), tint);
            return;
        }
        CardIcon::Salvage { wreck } => {
            let pile = if *wreck {
                sprites.wreck_pile()
            } else {
                sprites.scrap(
                    aftermine_sim::stats::SCRAP_NODE_AMOUNT,
                    aftermine_sim::stats::SCRAP_NODE_AMOUNT,
                )
            };
            blit(dest, pile, tint);
            return;
        }
    };
    let hull = if *ghost {
        Color::new(tint.r, tint.g, tint.b, tint.a * 0.7)
    } else {
        tint
    };
    let mut layers = match subject {
        crate::panel::OrderSubject::Unit(kind, owner) => unit_layers(*kind, hull, *owner),
        crate::panel::OrderSubject::Building(kind, tier, owner) => {
            building_layers(*kind, *tier, !*ghost, hull, *owner)
        }
    };
    if *ghost {
        // Scaffold and subject share their authored canvas, including its margins.
        layers.push((
            sprites.scaffold(),
            Color::new(tint.r, tint.g, tint.b, tint.a * 0.45),
        ));
        sprites.draw_canvas(dest, &layers);
    } else {
        sprites.draw_portrait(dest, &layers);
    }
    let badge = dest.w * 0.44;
    let plate = Rect::new(
        dest.x + dest.w - badge,
        dest.y + dest.h - badge,
        badge,
        badge,
    );
    let base = crate::theme::ICON_PLATE;
    fill_rect(
        plate,
        Color {
            a: base.a * tint.a,
            ..base
        },
    );
    blit(plate, sprites.verb_icon(*verb), tint);
}

/// Draws the command panel band laid out by [`layout_panel`].
#[expect(clippy::too_many_lines, reason = "draws the whole selection panel")]
pub(crate) fn draw_panel(
    sprites: &Sprites,
    input: &InputState,
    bindings: &crate::action::BindingMap,
    panel: &crate::panel::Panel,
    layout: &PanelLayout,
) {
    use crate::panel::{CardAction, CardIcon};
    let s = ui_scale();
    let draw_icon = |dest: Rect, icon: &CardIcon, tint: Color| {
        draw_card_icon(sprites, dest, icon, tint, panel.owner);
    };
    let hands = input.hud_hands();
    let pad = hands.names();

    let selection = match layout {
        PanelLayout::Catalog {
            band,
            slots,
            grouped,
            dock,
        } => {
            // Pad mode builds through the ring's build rings instead.
            if !hands.pad_mode() {
                draw_catalog(panel, input, bindings, (*band, slots, *grouped), &draw_icon);
            }
            if let Some(dock) = dock {
                draw_dock(panel, input, dock, s, &draw_icon);
            }
            return;
        }
        PanelLayout::Selection(selection) => selection,
    };
    let SelectionLayout {
        small,
        cards_x,
        measured,
        info_rect,
        portrait,
        action_rect,
        card_rects,
        rally_count,
        select_all,
        roster,
        dock,
        drones,
        hides_minimap: _,
    } = &**selection;
    let (small, cards_x, info_rect, action_rect) = (*small, *cards_x, *info_rect, *action_rect);
    let top = info_rect.y;
    // Pad mode leaves the command cards to the ring: their plate, the
    // rally mark over them and the cards themselves stay undrawn, and
    // nothing else moves. Where they stood, a legend names the buttons
    // that open the rings.
    let legend = pad_legend(panel, pad)
        .map(|legend| (legend, pad_legend_band(action_rect, cards_x, info_rect)));
    let (action_rect, card_rects): (Rect, &[Rect]) = if hands.pad_mode() {
        (Rect::new(0.0, 0.0, 0.0, 0.0), &[])
    } else {
        (action_rect, card_rects)
    };

    for rect in [info_rect, action_rect] {
        if rect.w == 0.0 {
            continue;
        }
        fill_rect(rect, crate::theme::SURFACE_BAND);
        draw_rectangle(rect.x, rect.y, rect.w, s, crate::theme::EDGE_WARM);
    }
    if let Some((legend, vacated)) = legend {
        let size = crate::theme::Type::Body.at(s);
        let x = pad_legend_x(vacated, legend.width(size), s);
        let baseline = vacated.y + vacated.h * 0.5 + size * 0.35;
        legend.draw_at(sprites, x, baseline, size, TEXT_SECONDARY, TEXT_PRIMARY);
    }
    let header = top + measured.header_pad;
    draw_icon(*portrait, &panel.portrait, WHITE);
    for (index, title) in measured.title.iter().enumerate() {
        crate::typography::draw(
            title,
            52.0 * s,
            header + (24.0 + index as f32 * 17.0) * s,
            crate::theme::Type::Body.at(s),
            TEXT_PRIMARY,
        );
    }
    if let (Some(all), Some(rect)) = (&panel.select_all, *select_all) {
        draw_select_all(sprites, all, rect, hovers(input, rect), pad, s);
    }
    for (index, status) in measured.status.iter().enumerate() {
        draw_text(
            status,
            12.0 * s,
            top + measured.health_y
                - (measured.status.len() - index - 1) as f32 * measured.line_h
                - 4.0 * s,
            measured.font,
            TEXT_SECONDARY,
        );
    }
    if let Some(crate::panel::info::Health {
        label,
        hp,
        max_hp,
        color,
    }) = panel.info.health
    {
        let y = top + measured.health_y;
        let value = format!("{hp}/{max_hp}");
        draw_text(label, 12.0 * s, y + 13.0 * s, measured.font, TEXT_SECONDARY);
        draw_text(
            &value,
            cards_x
                - 12.0 * s
                - measure_text(&value, None, numeric::font_size(measured.font), 1.0).width,
            y + 13.0 * s,
            measured.font,
            TEXT_PRIMARY,
        );
        draw_rectangle(
            12.0 * s,
            y + 19.0 * s,
            cards_x - 24.0 * s,
            3.0 * s,
            crate::theme::HEALTH_TRACK,
        );
        draw_rectangle(
            12.0 * s,
            y + 19.0 * s,
            (cards_x - 24.0 * s) * (hp as f32 / max_hp.max(1) as f32).clamp(0.0, 1.0),
            3.0 * s,
            color,
        );
    }
    for line in &measured.lines {
        let y = top + line.offset + measured.font;
        if let Some(icon) = line.icon {
            match icon {
                crate::panel::info::StatIcon::Capability(icon) => draw_capability_icon(
                    vec2(18.0 * s, y - 5.0 * s),
                    6.0 * s,
                    icon,
                    capability_icon_color(icon),
                    false,
                ),
                crate::panel::info::StatIcon::Verb(icon) => draw_icon(
                    Rect::new(11.0 * s, y - 12.0 * s, 14.0 * s, 14.0 * s),
                    &CardIcon::Verb(icon),
                    TEXT_SECONDARY,
                ),
            }
        }
        draw_text(
            &line.label,
            32.0 * s,
            y,
            measured.font,
            if line.section {
                TEXT_PRIMARY
            } else {
                TEXT_SECONDARY
            },
        );
        let value_width =
            measure_text(&line.value, None, numeric::font_size(measured.font), 1.0).width;
        draw_text(
            &line.value,
            cards_x - 12.0 * s - value_width,
            y,
            measured.font,
            TEXT_PRIMARY,
        );
    }

    // Mixed-selection roster, visually and geometrically separate from
    // verbs. It reads as "what is in my hand" before "what can it do".
    if !roster.is_empty() {
        let label_y = top + measured.roster_y - 4.0 * s;
        let heading = match panel.roster.first().map(|chip| chip.card.action) {
            Some(CardAction::FilterBuilding(_)) => "SELECTED STRUCTURES",
            _ => "SELECTED UNITS",
        };
        draw_text(
            heading,
            12.0 * s,
            label_y,
            crate::theme::Type::Small.at(s),
            TEXT_SECONDARY,
        );
        for (chip, &rect) in panel.roster.iter().zip(roster) {
            let card = &chip.card;
            let hovered = hovers(input, rect);
            fill_rect(
                rect,
                if hovered {
                    crate::theme::CARD_HOVER
                } else {
                    crate::theme::CHIP
                },
            );
            stroke_rect(
                rect,
                if hovered {
                    crate::theme::Stroke::Focus
                } else {
                    crate::theme::Stroke::Edge
                }
                .at(s),
                if hovered {
                    crate::theme::EDGE_FOCUS
                } else {
                    crate::theme::EDGE_CHIP
                },
            );
            let icon_size = (if small { 28.0 } else { 42.0 }) * s;
            draw_icon(
                Rect::new(
                    rect.x + (rect.w - icon_size) * 0.5,
                    rect.y + 3.0 * s,
                    icon_size,
                    icon_size,
                ),
                &card.icon,
                WHITE,
            );
            let face = roster_face(chip, small);
            let label = face.as_ref();
            let mut size = crate::theme::Type::Caption.at(s);
            let mut dims = measure_text(label, None, numeric::font_size(size), 1.0);
            while dims.width > rect.w - 6.0 * s && size > 8.0 * s {
                size -= 0.5 * s;
                dims = measure_text(label, None, numeric::font_size(size), 1.0);
            }
            draw_text(
                label,
                rect.x + (rect.w - dims.width) * 0.5,
                rect.y + rect.h - 6.0 * s,
                size,
                TEXT_PRIMARY,
            );
        }
    }

    if *rally_count > 0 && !card_rects.is_empty() {
        draw_icon(
            rally_context_rect(card_rects[0], s),
            &CardIcon::Verb(crate::panel::VerbIcon::Rally),
            WHITE,
        );
    }

    // Command cards, wrapping into as many rows as the width demands.
    for (card, &rect) in panel.cards.iter().zip(card_rects) {
        let hovered = hovers(input, rect);
        // An armed placement or rally lights its card, and so does a
        // running patrol; a route still being picked is not one yet.
        let selected = matches!(card.action, CardAction::ArmBuild(kind) if input.placing_kind() == Some(kind))
            || (card.action == CardAction::ArmRally && !input.rallying().is_empty())
            || (card.action == CardAction::Dispatch(crate::action::Action::Patrol)
                && panel.patrolling
                && input.patrol_route().is_none());
        let bg = if selected {
            crate::theme::CARD_ARMED
        } else if hovered && card.enabled {
            crate::theme::CARD_HOVER
        } else {
            crate::theme::CARD_IDLE
        };
        fill_rect(rect, bg);
        let autocast_on = card
            .autocast
            .is_some_and(|autocast| autocast.state == crate::panel::AutocastState::On);
        let border = if selected {
            SCRAP_COLOR
        } else if !card.enabled {
            crate::theme::BORDER_FAINT
        } else if hovered {
            crate::theme::EDGE_FOCUS
        } else if autocast_on {
            crate::theme::TEXT_ACCENT
        } else {
            crate::theme::EDGE_CARD
        };
        stroke_rect(rect, crate::theme::Stroke::Edge.at(s), border);
        let tint = if card.enabled {
            WHITE
        } else {
            Color::new(1.0, 1.0, 1.0, 0.35)
        };
        if matches!(card.action, CardAction::ArmRally | CardAction::ClearRally) {
            draw_rally_control(card, panel.rally, rect, s);
            continue;
        }
        // A tile stacks its icon over a centered title. A card with a cost
        // keeps a footer for the train time and cost and hangs the stack
        // from the top; any other card centers the stack.
        let icon_size = crate::theme::Type::Heading.at(s);
        let icon_gap = 6.0 * s;
        let footer_top = card.cost.map(|cost| {
            let dims = measure_text(
                cost.to_string(),
                None,
                numeric::font_size(crate::theme::Type::Body.at(s)),
                1.0,
            );
            rect.y + rect.h - 5.0 * s - dims.offset_y - 3.0 * s
        });
        let hotkey = (!card.hotkey.is_empty()).then(|| {
            let dims = measure_text(
                &card.hotkey,
                None,
                numeric::font_size(crate::theme::Type::Small.at(s)),
                1.0,
            );
            Rect::new(
                rect.x + 4.0 * s,
                rect.y + TILE_HOTKEY_BASELINE * s - dims.offset_y,
                dims.width,
                dims.height,
            )
        });
        let icon_floor = tile_icon_floor(rect, icon_size, hotkey, s);
        let room =
            footer_top.unwrap_or(rect.y + rect.h - 4.0 * s) - (icon_floor + icon_size + icon_gap);
        let title_width = rect.w - 8.0 * s;
        let mut title_size = crate::theme::Type::Small.at(s);
        let (names, ascent, title_height) = loop {
            let names = card_title_lines(
                &card.title,
                |text| measure_text(text, None, numeric::font_size(title_size), 1.0).width,
                title_width,
            );
            let mut ascent = 0.0_f32;
            let mut descent = 0.0_f32;
            let mut width = 0.0_f32;
            for name in &names {
                let dims = measure_text(name, None, numeric::font_size(title_size), 1.0);
                ascent = ascent.max(dims.offset_y);
                descent = descent.max(dims.height - dims.offset_y);
                width = width.max(dims.width);
            }
            let height = ascent + descent + names.len().saturating_sub(1) as f32 * title_size;
            if (height <= room && width <= title_width) || title_size <= 8.0 * s {
                break (names, ascent, height);
            }
            title_size -= 0.5 * s;
        };
        let icon_y = if footer_top.is_some() {
            icon_floor
        } else {
            (rect.y + (rect.h - icon_size - icon_gap - title_height) * 0.5).max(icon_floor)
        };
        draw_icon(
            Rect::new(
                rect.x + (rect.w - icon_size) * 0.5,
                icon_y,
                icon_size,
                icon_size,
            ),
            &card.icon,
            tint,
        );
        let title_top = icon_y + icon_size + icon_gap;
        for (line_index, name) in names.iter().enumerate() {
            let width = measure_text(name, None, numeric::font_size(title_size), 1.0).width;
            draw_text(
                name,
                rect.x + (rect.w - width) * 0.5,
                title_top + ascent + line_index as f32 * title_size,
                title_size,
                TEXT_PRIMARY,
            );
        }
        if let Some(cost) = card.cost {
            let label = format!("{cost}");
            let dims = measure_text(
                &label,
                None,
                numeric::font_size(crate::theme::Type::Body.at(s)),
                1.0,
            );
            draw_text(
                &label,
                rect.x + rect.w - dims.width - 5.0 * s,
                rect.y + rect.h - 5.0 * s,
                crate::theme::Type::Body.at(s),
                if card.enabled {
                    SCRAP_COLOR
                } else {
                    TEXT_DISABLED
                },
            );
        }
        if !card.hotkey.is_empty() {
            draw_text(
                &card.hotkey,
                rect.x + 4.0 * s,
                rect.y + TILE_HOTKEY_BASELINE * s,
                crate::theme::Type::Small.at(s),
                TEXT_SECONDARY,
            );
        }
        if let Some(autocast) = card.autocast {
            draw_autocast_tag(rect, autocast.state, s);
        }
        if let (CardAction::Dispatch(crate::action::Action::TrainSlot(_)), CardIcon::Unit(kind)) =
            (card.action, card.icon)
        {
            let label = crate::panel::unit_train_time_label(kind);
            draw_text(
                &label,
                rect.x + 4.0 * s,
                rect.y + rect.h - 5.0 * s,
                crate::theme::Type::Caption.at(s),
                if card.enabled {
                    TEXT_SECONDARY
                } else {
                    TEXT_DISABLED
                },
            );
        }
    }

    // Orders dock on the left edge: production ghosts or order chips.
    if let Some(dock) = dock {
        draw_dock(panel, input, dock, s, &draw_icon);
    }
    if let (Some(column), Some(layout)) = (&panel.drones, drones) {
        draw_drone_column(column, layout, input, s, &draw_icon);
    }
}

/// The drone column: its plate, the time left over it, and a chip per
/// drone on order, the one building bright and wearing its meter.
fn draw_drone_column(
    column: &crate::panel::DroneColumn,
    layout: &DroneColumnLayout,
    input: &InputState,
    s: f32,
    draw_icon: &impl Fn(Rect, &crate::panel::CardIcon, Color),
) {
    draw_dock_plate(layout.plate, false, s);
    draw_text(
        &column.label,
        layout.plate.x + 8.0 * s,
        layout.plate.y + 17.0 * s,
        crate::theme::Type::Small.at(s),
        TEXT_PRIMARY,
    );
    for (i, (card, &rect)) in column.chips.iter().zip(&layout.chips).enumerate() {
        draw_chip_plate(rect, i == 0, hovers(input, rect), s);
        let isz = 34.0 * s;
        draw_icon(
            Rect::new(rect.x + (rect.w - isz) * 0.5, rect.y + 5.0 * s, isz, isz),
            &card.icon,
            WHITE,
        );
        draw_chip_meter(rect, card.progress, s);
    }
}

/// A dock plate borders its open sides; the screen edge or the band
/// closes the rest.
fn draw_dock_plate(rect: Rect, bottom: bool, s: f32) {
    let edge = crate::theme::BORDER_STRONG;
    fill_rect(rect, crate::theme::SURFACE_PLATE);
    draw_rectangle(rect.x, rect.y, rect.w, 1.5 * s, edge);
    draw_rectangle(rect.right() - 1.5 * s, rect.y, 1.5 * s, rect.h, edge);
    if bottom {
        draw_rectangle(rect.x, rect.bottom() - 1.5 * s, rect.w, 1.5 * s, edge);
    }
}

/// A dock chip's face. The active order or job wears the bright border.
fn draw_chip_plate(rect: Rect, active: bool, hovered: bool, s: f32) {
    fill_rect(rect, crate::theme::CHIP);
    stroke_rect(
        rect,
        if active {
            crate::theme::Stroke::Focus
        } else {
            crate::theme::Stroke::Edge
        }
        .at(s),
        if hovered || active {
            crate::theme::EDGE_FOCUS
        } else {
            crate::theme::EDGE_CHIP
        },
    );
}

/// A chip with a measurable job wears its meter: the production head's
/// build, a site's rise, a patient's hp. Read from the model, never
/// peeked back out of the state.
fn draw_chip_meter(rect: Rect, progress: Option<f32>, s: f32) {
    if let Some(frac) = progress {
        draw_rectangle(
            rect.x,
            rect.y + rect.h - 3.0 * s,
            rect.w * frac.clamp(0.0, 1.0),
            3.0 * s,
            SCRAP_COLOR,
        );
    }
}

fn draw_dock(
    panel: &crate::panel::Panel,
    input: &InputState,
    layout: &DockLayout,
    s: f32,
    draw_icon: &impl Fn(Rect, &crate::panel::CardIcon, Color),
) {
    use crate::panel::CardIcon;
    let DockLayout {
        dock: _,
        list,
        stop,
        chips,
        hidden,
        more_baseline,
    } = layout;
    if list.h > 0.0 {
        draw_dock_plate(*list, false, s);
    }
    if !panel.queue.is_empty() {
        draw_text(
            &panel.queue_label,
            8.0 * s,
            list.y + 17.0 * s,
            crate::theme::Type::Small.at(s),
            TEXT_PRIMARY,
        );
    }
    if let (Some(card), Some((stop_plate, rect))) = (&panel.stop, *stop) {
        // Named where the chips beneath it are.
        draw_dock_plate(stop_plate, true, s);
        let named = rect.w >= 150.0 * s;
        fill_rect(rect, crate::theme::CHIP);
        stroke_rect(
            rect,
            crate::theme::Stroke::Edge.at(s),
            if hovers(input, rect) {
                crate::theme::EDGE_FOCUS
            } else {
                crate::theme::EDGE_CHIP
            },
        );
        let isz = 34.0 * s;
        draw_icon(
            Rect::new(
                rect.x + if named { 4.0 * s } else { (rect.w - isz) * 0.5 },
                rect.y + (rect.h - isz) * 0.5,
                isz,
                isz,
            ),
            &card.icon,
            WHITE,
        );
        if named {
            draw_text(
                &card.title,
                rect.x + 42.0 * s,
                rect.y + 27.0 * s,
                crate::theme::Type::Small.at(s),
                TEXT_PRIMARY,
            );
        }
    }
    let orders_dock = panel.dock == crate::panel::DockKind::Orders;
    for (i, (card, &rect)) in panel.queue.iter().zip(chips).enumerate() {
        // The active order or production head wears the bright border;
        // a ready-but-blocked head remains the queue's current job.
        let group = panel.queue_groups.get(i);
        let active = group.map_or(i == 0, |g| g.active > 0);
        let wide_group = group.is_some() && rect.w >= 150.0 * s;
        draw_chip_plate(rect, active, hovers(input, rect), s);
        // Order chips carry the same numbers as the world
        // breadcrumbs: chip 2 is waypoint 2.
        if orders_dock && panel.queue.len() > 1 {
            draw_text(
                format!("{}", i + 1),
                rect.x + 3.0 * s,
                rect.y + 13.0 * s,
                crate::theme::Type::Caption.at(s),
                TEXT_SECONDARY,
            );
        }
        {
            let isz = 34.0 * s;
            draw_icon(
                Rect::new(
                    rect.x
                        + if wide_group {
                            4.0 * s
                        } else {
                            (rect.w - isz) * 0.5
                        },
                    rect.y + 5.0 * s,
                    isz,
                    isz,
                ),
                &card.icon,
                WHITE,
            );
        }
        if let Some(group) = group.filter(|_| !wide_group) {
            let label = format!("x{}", group.count);
            let width = measure_text(
                &label,
                None,
                numeric::font_size(crate::theme::Type::Small.at(s)),
                1.0,
            )
            .width
                + 4.0 * s;
            draw_rectangle(
                rect.right() - width - 2.0 * s,
                rect.y + 2.0 * s,
                width,
                14.0 * s,
                crate::theme::SURFACE_CARD,
            );
            draw_text(
                label,
                rect.right() - width,
                rect.y + 13.0 * s,
                crate::theme::Type::Small.at(s),
                TEXT_PRIMARY,
            );
        }
        if let Some(group) = group.filter(|_| wide_group) {
            let name = match card.icon {
                CardIcon::Unit(kind) => crate::typography::entity_name(kind.name()),
                _ => card.title.clone(),
            };
            draw_text(
                format!("{name} x {}", group.count),
                rect.x + 42.0 * s,
                rect.y + 18.0 * s,
                crate::theme::Type::Small.at(s),
                TEXT_PRIMARY,
            );
            draw_text(
                match group.next_ticks {
                    Some(0) => format!("{} building | ready", group.active),
                    Some(ticks) => format!(
                        "{} building | {}",
                        group.active,
                        crate::panel::tick_time_label(ticks)
                    ),
                    None => "waiting".into(),
                },
                rect.x + 42.0 * s,
                rect.y + 34.0 * s,
                crate::theme::Type::Caption.at(s),
                TEXT_SECONDARY,
            );
        }
        draw_chip_meter(rect, card.progress, s);
    }
    if *hidden > 0 {
        draw_text(
            format!("+{hidden}"),
            12.0 * s,
            *more_baseline,
            crate::theme::Type::Small.at(s),
            TEXT_SECONDARY,
        );
    }
}

/// Whether the mouse hovers `rect`, only while it is the pointer in use.
fn hovers(input: &InputState, rect: Rect) -> bool {
    input.chrome_hover_point().is_some_and(|p| rect.contains(p))
}

/// What pad mode shows where a selection's command cards were: the
/// buttons that open the command ring and the group ring. A selection
/// whose ring would be empty, such as an enemy's or an ally's, shows none,
/// and so does pad mode with no controller to name (`pad`).
pub(crate) fn pad_legend(
    panel: &crate::panel::Panel,
    pad: Option<aftermine_protocol::PadFamily>,
) -> Option<crate::hint_line::HintLine> {
    use aftermine_protocol::PadButton;
    let family = pad?;
    crate::radial::offers_commands(panel).then(|| {
        crate::hint_line::HintLine::default()
            .button(family, PadButton::North)
            .text(" Commands    ")
            .button(family, PadButton::DpadRight)
            .text(" Groups")
    })
}

/// The band pad mode's legend stands in: the card plate it vacated or, for
/// a selection with no cards to lay one out, such as a structure whose
/// only command is Stop, the stretch just right of the info plate.
fn pad_legend_band(plate: Rect, cards_x: f32, info: Rect) -> Rect {
    if plate.w > 0.0 {
        plate
    } else {
        Rect::new(cards_x, info.y, 0.0, info.h)
    }
}

/// Where pad mode's legend starts: centered on the vacated card plate, but
/// never left of its inset, where the info plate would cover the first
/// button. A plate too narrow for the line lets it run on to the right,
/// where nothing draws in pad mode.
fn pad_legend_x(vacated: Rect, width: f32, s: f32) -> f32 {
    (vacated.x + (vacated.w - width) * 0.5).max(vacated.x + 12.0 * s)
}

/// The hover tooltip for panel cards, drawn over everything: name,
/// hotkey, cost, description, weapon lines, and why a disabled card
/// refuses. Rebuilt from the same panel model the frame drew.
pub(crate) fn draw_panel_tooltip(game: &crate::game::Scene<'_>, input: &InputState) {
    use crate::layout::TooltipSide;
    let panel = game.presentation.panel_model.borrow();
    let Some(panel) = panel.as_ref() else {
        return;
    };
    let layout = game.presentation.layout.get();
    if !layout.panel_top.is_finite() {
        return;
    }
    let s = ui_scale();
    // A resting finger previews the card it covers; the mouse previews
    // what it hovers only while it is the pointer in use.
    let pointer = match input.touch_preview() {
        Some(p) => Some((p, Some(s))),
        None => input.chrome_hover_point().map(|p| (p, None)),
    };
    let Some(hit) =
        pointer.and_then(|(p, touch_ui)| crate::layout::card_under(&layout, p, touch_ui))
    else {
        return;
    };
    // The hovered rect is the anchor, not just the index: the orders
    // dock stacks upward from the band, so a tooltip pinned to the
    // band's top edge would sit far from a high chip.
    let Some(card) = panel.card(hit.row, hit.index) else {
        return;
    };
    let r = hit.rect;
    let column = match hit.row {
        crate::layout::CardRow::Queue | crate::layout::CardRow::Stop => Some(layout.orders),
        crate::layout::CardRow::Drones => Some(layout.drones),
        _ => None,
    };
    let (anchor, side) = match column {
        // Anchored across the column's full width so the box clears it
        // cleanly at any chip inset.
        Some(column) => (
            Rect::new(column.x, r.y, column.w.max(r.w), r.h),
            TooltipSide::RightOf,
        ),
        None => (r, TooltipSide::Above),
    };
    let comparison = matches!(card.action, crate::panel::CardAction::Upgrade)
        .then_some(panel.info.upgrade.as_ref())
        .flatten();
    // The box fits between the top bar and the band so it never covers
    // the command cards.
    let bounds = vec2(
        screen_width(),
        if matches!(side, TooltipSide::Above) {
            anchor.y
        } else if layout.panel_regions[1].w > 0.0 {
            layout.panel_regions[1].y
        } else {
            screen_height()
        },
    );
    draw_card_tooltip(
        card,
        comparison,
        (400.0 * s).min(screen_width() - 40.0 * s),
        |size| crate::layout::tooltip_origin(anchor, size, side, bounds, layout.top_bar_h, 6.0 * s),
    );
}

/// A card's tooltip: name and hotkey, cost, description, an upgrade's
/// before and after, and why a disabled card refuses. Its lines wrap at
/// `wrap_w`, and `place` turns the box's size into its top-left corner.
pub(crate) fn draw_card_tooltip(
    card: &crate::panel::Card,
    comparison: Option<&crate::panel::UpgradeComparison>,
    wrap_w: f32,
    place: impl FnOnce(Vec2) -> Vec2,
) {
    let s = ui_scale();
    let mut lines: Vec<(String, Color)> = Vec::new();
    let header = if card.hotkey.is_empty() {
        card.title.clone()
    } else {
        format!("{}   [{}]", card.title, card.hotkey)
    };
    lines.push((header, TEXT_PRIMARY));
    if let Some(cost) = card.cost {
        lines.push((format!("{cost} scrap"), SCRAP_COLOR));
    }
    let intro_lines = lines.len();
    let size = crate::theme::Type::Body.at(s);
    let pad = 12.0 * s;
    for d in &card.desc {
        for line in crate::render::wrap_words(
            d,
            |t| measure_text(t, None, numeric::font_size(size), 1.0).width,
            wrap_w,
        ) {
            lines.push((line, TEXT_BODY));
        }
    }
    if let Some(why) = &card.why {
        lines.push((crate::typography::sentence_case(why), DANGER));
    }
    let text_width = lines
        .iter()
        .map(|(l, _)| measure_text(l, None, numeric::font_size(size), 1.0).width)
        .fold(0.0f32, f32::max);
    let mut columns = [0.0_f32; 2];
    if let Some(comparison) = comparison {
        for (column, header) in columns.iter_mut().zip(["Changes", "After upgrade"]) {
            *column = measure_text(header, None, numeric::font_size(size), 1.0).width;
        }
        for row in &comparison.rows {
            for (column, text) in columns.iter_mut().zip([&row.label, &row.upgraded]) {
                *column = column.max(measure_text(text, None, numeric::font_size(size), 1.0).width);
            }
        }
    }
    let table_width = if comparison.is_some() {
        columns.iter().sum::<f32>() + 20.0 * s
    } else {
        0.0
    };
    let width = text_width.max(table_width) + pad * 2.0;
    let line_h = 22.0 * s;
    let table_height = comparison.map_or(0.0, |c| (c.rows.len() + 2) as f32 * line_h);
    let height = lines.len() as f32 * line_h + table_height + pad * 1.5;
    let origin = place(vec2(width, height));
    let (x, y) = (origin.x, origin.y);
    draw_rectangle(x, y, width, height, crate::theme::SURFACE_CARD);
    stroke_rect(
        Rect::new(x, y, width, height),
        crate::theme::Stroke::Edge.at(s),
        crate::theme::EDGE_CARD,
    );
    for (i, (line, color)) in lines.iter().enumerate() {
        draw_text(
            line,
            x + pad,
            y + pad + (i as f32 + 0.6) * line_h + if i >= intro_lines { table_height } else { 0.0 },
            size,
            *color,
        );
    }
    if let Some(comparison) = comparison {
        let table_y = y + pad + intro_lines as f32 * line_h;
        let upgraded_x = x + pad + columns[0] + 20.0 * s;
        for (label, left) in [("Changes", x + pad), ("After upgrade", upgraded_x)] {
            draw_text(label, left, table_y + 0.6 * line_h, size, TEXT_SECONDARY);
        }
        draw_rectangle(
            x + pad,
            table_y + line_h,
            table_width,
            s,
            crate::theme::EDGE_CARD,
        );
        for (i, row) in comparison.rows.iter().enumerate() {
            let baseline = table_y + (i as f32 + 1.8) * line_h;
            draw_text(&row.label, x + pad, baseline, size, TEXT_BODY);
            draw_text(&row.upgraded, upgraded_x, baseline, size, SCRAP_COLOR);
        }
    }
}

#[cfg(test)]
mod tests;
