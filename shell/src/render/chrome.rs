//! Standing chrome and overlays: the top bar with its controls hint,
//! toasts, the salvage hover tooltip, the omniscient debug overlay,
//! and the endgame verdict. Their geometry comes from the HUD layout
//! pass (`super::hud`), the same one input hit-tests.

use super::*;
use crate::numeric;
use crate::numeric::Fit;
use crate::render::prim::{fill_rect, line_between, stroke_rect};
use crate::theme::TEXT_ACCENT;

/// The paused status names the pause key where one exists. A
/// touch-only build has no key to name, and an unbound key reads bare.
fn paused_status(key: &str, keyless: bool) -> String {
    if keyless || key.is_empty() {
        "PAUSED".to_string()
    } else {
        format!("PAUSED [{key}]")
    }
}

/// The alert badge says what the alerts warn of, an attack before an
/// assembling experimental, and names the jump key where one exists.
fn alert_badge_text(kind: crate::game::AlertKind, key: &str, keyless: bool) -> String {
    let what = match kind {
        crate::game::AlertKind::Attack => "under attack".to_string(),
        crate::game::AlertKind::Assembly(machine) => format!("enemy {} assembling", machine.name()),
    };
    if keyless || key.is_empty() {
        what
    } else {
        format!("{what} [{key}]")
    }
}

/// The idle badge names its key where one exists.
fn idle_badge_text(idle: usize, key: &str, keyless: bool) -> String {
    if keyless {
        format!("{idle} idle")
    } else {
        format!("{idle} idle [{key}]")
    }
}

/// The concede banner's way to the menu.
fn concede_hint(hands: crate::platform::Hands) -> &'static str {
    match (hands.touch(), hands.keys) {
        (false, true) => "your team fights on | {back} for options",
        (false, false) => "your team fights on | click the menu button for options",
        (true, true) => "your team fights on | {back} or tap the menu button for options",
        (true, false) => "your team fights on | tap the menu button for options",
    }
}

/// A control-group slot: its number in the corner and, for a saved
/// group, the machines it holds; the "+" offers to save the selection,
/// and an empty group is a faint outline. The group the selection
/// already is wears an accent rim.
fn draw_group_slot(
    rect: Rect,
    slot: crate::layout::GroupSlot,
    count: usize,
    current: bool,
    s: f32,
) {
    let number = slot.number();
    match slot {
        crate::layout::GroupSlot::Recall(_) => {
            fill_rect(rect, crate::theme::BADGE);
            let text = count.to_string();
            let mut size = crate::theme::Type::Body.at(s);
            while measure_text(&text, None, numeric::font_size(size), 1.0).width > rect.w - 14.0 * s
                && size > 11.0 * s
            {
                size -= 1.0 * s;
            }
            // Centered on the glyphs' own box, not the baseline, so the
            // count sits in the middle of the slot at any size.
            let dims = measure_text(&text, None, numeric::font_size(size), 1.0);
            draw_text(
                &text,
                rect.x + (rect.w - dims.width) * 0.5,
                rect.y + (rect.h - dims.height) * 0.5 + dims.offset_y,
                size,
                SCRAP_COLOR,
            );
        }
        crate::layout::GroupSlot::Assign(_) => {
            fill_rect(rect, crate::theme::SURFACE_PLATE);
            stroke_rect(rect, crate::theme::Stroke::Hairline.at(s), TEXT_DISABLED);
            let c = rect.center();
            let arm = 6.0 * s;
            line_between(c - vec2(arm, 0.0), c + vec2(arm, 0.0), 2.0 * s, SCRAP_COLOR);
            line_between(c - vec2(0.0, arm), c + vec2(0.0, arm), 2.0 * s, SCRAP_COLOR);
        }
        crate::layout::GroupSlot::Empty(_) => {
            stroke_rect(
                rect,
                crate::theme::Stroke::Hairline.at(s),
                crate::theme::BORDER_FAINT,
            );
        }
    }
    draw_text(
        number.to_string(),
        rect.x + 4.0 * s,
        rect.y + 12.0 * s,
        crate::theme::Type::Small.at(s),
        if matches!(slot, crate::layout::GroupSlot::Empty(_)) {
            TEXT_DISABLED
        } else {
            TEXT_SECONDARY
        },
    );
    if current {
        stroke_rect(rect, crate::theme::Stroke::Edge.at(s), TEXT_ACCENT);
    }
}

/// The control-group column: its plate, and each slot with what it does
/// and how many machines its group holds.
pub(crate) struct GroupColumnLayout {
    pub(super) plate: Rect,
    pub(super) slots: [(Rect, crate::layout::GroupSlot, usize); crate::action::CONTROL_GROUPS],
    current: Option<u8>,
}

/// The control-group column above the minimap, once groups are in use
/// or there is a selection to save: every group keeps its place, empty
/// ones as faint outlines.
pub(super) fn group_column_layout(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    minimap: Rect,
    s: f32,
) -> Option<GroupColumnLayout> {
    let counts = input.group_counts(game);
    let offer = input.group_on_offer(game);
    let shown = crate::render::control_groups()
        && (counts.iter().any(|count| *count > 0) || offer.is_some());
    let column =
        crate::layout::group_column(s, crate::platform::TOUCH_ONLY, minimap).filter(|_| shown)?;
    let slots = std::array::from_fn(|slot| {
        let number = slot.fit::<u8>() + 1;
        let action = if counts[slot] > 0 {
            crate::layout::GroupSlot::Recall(number)
        } else if offer == Some(number) {
            crate::layout::GroupSlot::Assign(number)
        } else {
            crate::layout::GroupSlot::Empty(number)
        };
        (column.slots[slot], action, counts[slot])
    });
    Some(GroupColumnLayout {
        plate: column.plate,
        slots,
        current: input.selected_group(game),
    })
}

fn draw_group_column(column: &GroupColumnLayout, s: f32) {
    fill_rect(column.plate, crate::theme::SURFACE_PLATE);
    stroke_rect(
        column.plate,
        crate::theme::Stroke::Edge.at(s),
        crate::theme::BORDER_STRONG,
    );
    for (rect, slot, count) in column.slots {
        draw_group_slot(rect, slot, count, column.current == Some(slot.number()), s);
    }
}

/// Three bars on the badge fill: the glyph needs no font coverage or
/// atlas entry, and whole-rect fills stay crisp at any scale.
fn draw_menu_button(rect: Rect, s: f32) {
    fill_rect(rect, crate::theme::BADGE);
    let bar_w = 16.0 * s;
    let bar_h = (2.0 * s).max(1.0);
    let x = rect.x + (rect.w - bar_w) * 0.5;
    let middle = rect.y + rect.h * 0.5;
    for offset in [-5.0, 0.0, 5.0] {
        fill_rect(
            Rect::new(x, middle + offset * s - bar_h * 0.5, bar_w, bar_h),
            SCRAP_COLOR,
        );
    }
}

/// Where the cursor hovers: only while the mouse is the pointer in use,
/// never at a stale point on a touch device.
pub(super) fn hover_point(input: &InputState) -> Option<Vec2> {
    (input.touches.is_empty() && input.last_pointer == crate::input::Pointer::Mouse)
        .then_some(input.mouse)
}

/// Hovered salvage says what it holds, by the same fog rule as the
/// panel. Touch reads it by selecting the pile instead.
pub(crate) fn draw_salvage_tooltip(game: &crate::game::Scene<'_>, input: &InputState) {
    let Some(point) = hover_point(input) else {
        return;
    };
    if game.presentation.layout.get().chrome_owns(point) {
        return;
    }
    let world = game.presentation.camera.to_world(point);
    let tile = numeric::tile_at(world);
    let text = match game.known_salvage(tile) {
        Some(crate::game::Salvage::Scrap(amount)) => format!("scrap {amount}"),
        Some(crate::game::Salvage::Wreck(amount)) => format!("wreck {amount}"),
        None => return,
    };
    let s = ui_scale();
    let dims = measure_text(
        &text,
        None,
        numeric::font_size(crate::theme::Type::Body.at(s)),
        1.0,
    );
    let (x, y) = (point.x + 14.0 * s, point.y - 10.0 * s);
    draw_rectangle(
        x - 4.0 * s,
        y - 14.0 * s,
        dims.width + 8.0 * s,
        20.0 * s,
        PANEL,
    );
    draw_text(&text, x, y, crate::theme::Type::Body.at(s), SCRAP_COLOR);
}

pub(crate) fn draw_overlay(game: &crate::game::Scene<'_>, alpha: f32) {
    let (min, max) = visible_tiles(game);
    for x in min.x..=max.x {
        let a = game
            .presentation
            .camera
            .to_screen(vec2(x as f32, min.y as f32));
        let b = game
            .presentation
            .camera
            .to_screen(vec2(x as f32, max.y as f32));
        line_between(a, b, 1.0, BONE_FAINT);
    }
    for y in min.y..=max.y {
        let a = game
            .presentation
            .camera
            .to_screen(vec2(min.x as f32, y as f32));
        let b = game
            .presentation
            .camera
            .to_screen(vec2(max.x as f32, y as f32));
        line_between(a, b, 1.0, BONE_FAINT);
    }
    for unit in game.state.units() {
        let pos = game.presentation.draw_pos(unit.id, unit.pos, alpha);
        let screen = game.presentation.camera.to_screen(pos);
        draw_text(
            format!("u{} {}hp", unit.id.0, unit.hp),
            screen.x + 8.0,
            screen.y - 8.0,
            16.0,
            BONE,
        );
        if let Some(path) = &unit.path {
            let mut previous = screen;
            for waypoint in path.waypoints.iter().skip(path.next as usize) {
                let next = game
                    .presentation
                    .camera
                    .to_screen(vec2(waypoint.x as f32 + 0.5, waypoint.y as f32 + 0.5));
                line_between(previous, next, 1.0, BONE_FAINT);
                previous = next;
            }
        }
    }
}

pub(crate) fn draw_overlay_info(game: &crate::game::Scene<'_>) {
    let info = format!(
        "tick {}  fps {}  zoom {:.0}  center ({:.1},{:.1})",
        game.state.current_tick(),
        get_fps(),
        game.presentation.camera.zoom,
        game.presentation.camera.center.x,
        game.presentation.camera.center.y,
    );
    let s = ui_scale();
    let panel = game.presentation.layout.get().performance;
    let y = if panel.w > 0.0 {
        panel.y + panel.h + 20.0 * s
    } else {
        60.0 * s
    };
    let size = crate::theme::Type::Small.at(s);
    let width = measure_text(&info, None, numeric::font_size(size), 1.0).width;
    draw_text(
        &info,
        (screen_width() - width - 12.0 * s).max(0.0),
        y,
        size,
        BONE,
    );
}

/// The armed-mode ribbon's narrowest width, in logical px at 1x.
const RIBBON_MIN_W: f32 = 210.0;

/// The top of the ribbon: just above the panel band, or above the
/// window's bottom edge, and never under the top bar.
fn ribbon_row_y(viewport: Vec2, scale: f32, panel_top: f32, height: f32) -> f32 {
    let preferred_y = if panel_top.is_finite() {
        panel_top - height - 8.0 * scale
    } else {
        viewport.y - height - 12.0 * scale
    };
    let min_y = crate::layout::TOP_BAR_H * scale + 8.0 * scale;
    let max_y = (viewport.y - height - 8.0 * scale).max(min_y);
    preferred_y.clamp(min_y, max_y)
}

/// The armed-mode ribbon's width for a label of `label_width`.
fn ribbon_width(viewport: Vec2, scale: f32, label_width: f32) -> f32 {
    (label_width + 34.0 * scale)
        .max(RIBBON_MIN_W * scale)
        .min((viewport.x - 24.0 * scale).max(RIBBON_MIN_W * scale))
}

/// The status row a placement ribbon adds under its label, in logical px.
const RIBBON_STATUS_H: f32 = 14.0;

/// The armed-mode ribbon's height: one touch target, plus the status row
/// while placement has a spot to judge.
fn ribbon_height(scale: f32, status: bool) -> f32 {
    let status = if status { RIBBON_STATUS_H } else { 0.0 };
    (crate::theme::MIN_TOUCH_TARGET + status) * scale
}

/// Where the armed-mode ribbon sits: centered over the panel band,
/// sliding left only when it would run off the window or into the
/// minimap.
fn ribbon_geometry(
    viewport: Vec2,
    scale: f32,
    panel_top: f32,
    (width, height): (f32, f32),
    minimap: Rect,
) -> Rect {
    let y = ribbon_row_y(viewport, scale, panel_top, height);
    let blocks_row = minimap.w > 0.0 && minimap.y < y + height && minimap.y + minimap.h > y;
    let right = if blocks_row {
        minimap.x - 8.0 * scale
    } else {
        viewport.x - 12.0 * scale
    };
    let x = ((viewport.x - width) * 0.5)
        .min(right - width)
        .max(12.0 * scale);
    Rect::new(x, y, width, height)
}

/// Whether a touch-only build offers the QUEUE toggle: while the human
/// commands a selected unit, or while QUEUE is on so it can be turned
/// off. Buildings queue nothing, and a spectator commands nothing.
pub(super) fn queue_toggle_shown(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    touch_only: bool,
) -> bool {
    let human = game.presentation.human;
    let commands_units = game
        .presentation
        .selection
        .units
        .iter()
        .any(|id| game.state.unit(*id).is_some_and(|u| u.player == human));
    touch_only
        && !game.presentation.spectating()
        && (commands_units || input.queue_toggle != crate::input::QueueToggle::Off)
}

/// Where the QUEUE toggle sits: over the panel's left corner, in the
/// orders dock's column, so it never moves as orders queue up; the dock
/// stacks above it instead.
pub(super) fn queue_toggle_rect(viewport: Vec2, scale: f32, regions: &[Rect; 2]) -> Rect {
    let size = crate::theme::MIN_TOUCH_TARGET * scale;
    let x = 8.0 * scale;
    let floor = regions
        .iter()
        .filter(|r| r.w > 0.0 && r.x < x + size)
        .map(|r| r.y)
        .fold(viewport.y, f32::min);
    Rect::new(x, floor - size - 8.0 * scale, size, size)
}

/// How far the orders dock rises to clear the QUEUE toggle.
pub(super) fn queue_dock_lift(scale: f32) -> f32 {
    (crate::theme::MIN_TOUCH_TARGET + 16.0) * scale
}

/// The QUEUE toggle: a chain of queued waypoints, dark like the orders dock
/// while off, outlined in amber while it covers the next action, and solid
/// amber while locked on, so neither mode can hide.
fn draw_queue_toggle(rect: Rect, toggle: crate::input::QueueToggle, s: f32) {
    use crate::input::QueueToggle;
    use crate::theme::{BORDER_STRONG, SURFACE_PLATE, Stroke};
    let glyph = match toggle {
        QueueToggle::Off => {
            fill_rect(rect, SURFACE_PLATE);
            stroke_rect(rect, Stroke::Edge.at(s), BORDER_STRONG);
            TEXT_BODY
        }
        QueueToggle::Latched { .. } => {
            fill_rect(rect, SURFACE_PLATE);
            stroke_rect(rect, Stroke::Heavy.at(s), TEXT_ACCENT);
            TEXT_ACCENT
        }
        QueueToggle::Locked => {
            fill_rect(rect, TEXT_ACCENT);
            SURFACE_CARD
        }
    };
    let c = rect.center();
    let points = [
        c + vec2(-11.0, 6.0) * s,
        c + vec2(0.0, -6.0) * s,
        c + vec2(11.0, 6.0) * s,
    ];
    line_between(points[0], points[1], 2.0 * s, glyph);
    line_between(points[1], points[2], 2.0 * s, glyph);
    for point in points {
        crate::render::prim::fill_circle(point, 3.5 * s, glyph);
    }
}

/// The touch ghost's controls: the cross, the check mark, lit while a tap
/// would build or launch, and the row handle. They draw last of the chrome, over the
/// minimap too, since they answer presses first.
pub(crate) fn draw_ghost_controls(game: &crate::game::Scene<'_>, input: &InputState) {
    let s = ui_scale();
    let layout = game.presentation.layout.get();
    let plate = |rect: Rect| {
        fill_rect(rect, crate::theme::SURFACE_PLATE);
        stroke_rect(
            rect,
            crate::theme::Stroke::Edge.at(s),
            crate::theme::BORDER_STRONG,
        );
    };
    if layout.ghost_cancel.w > 0.0 {
        let rect = layout.ghost_cancel;
        plate(rect);
        let (c, arm) = (rect.center(), 8.0 * s);
        line_between(c - vec2(arm, arm), c + vec2(arm, arm), 2.5 * s, TEXT_BODY);
        line_between(c + vec2(-arm, arm), c + vec2(arm, -arm), 2.5 * s, TEXT_BODY);
    }
    if layout.ghost_confirm.w > 0.0 {
        let rect = layout.ghost_confirm;
        plate(rect);
        let builds = input.striking.is_some()
            || !input.planned_stamps().is_empty()
            || crate::input::preview_spot(game, input).is_some_and(|spot| spot.verdict.builds());
        let mark = if builds {
            TEXT_ACCENT
        } else {
            crate::theme::TEXT_DISABLED
        };
        let c = rect.center();
        let (start, knee, end) = (
            c + vec2(-9.0, 0.0) * s,
            c + vec2(-3.0, 7.0) * s,
            c + vec2(10.0, -8.0) * s,
        );
        line_between(start, knee, 3.0 * s, mark);
        line_between(knee, end, 3.0 * s, mark);
    }
    if layout.ghost_handle.w > 0.0 {
        let c = layout.ghost_handle.center();
        crate::render::prim::fill_circle(c, 11.0 * s, crate::theme::SURFACE_PLATE);
        crate::render::prim::stroke_circle(c, 11.0 * s, 1.5 * s, crate::theme::BORDER_STRONG);
        for dx in [-4.0, 0.0, 4.0] {
            let x = c.x + dx * s;
            line_between(
                vec2(x, c.y - 5.0 * s),
                vec2(x, c.y + 5.0 * s),
                1.5 * s,
                TEXT_BODY,
            );
        }
    }
}

/// The armed-mode ribbon: where it sits and what it names.
pub(crate) struct RibbonLayout {
    pub(super) rect: Rect,
    pub(super) label: String,
    pub(super) cost: Option<String>,
    building: Option<oxide_sim::BuildingKind>,
    label_w: f32,
    icon_w: f32,
    pub(super) status: Option<RibbonStatus>,
}

/// A placement ribbon's status row: what a click at the ghost or cursor
/// would do, then whether it joins the builder's queue or replaces it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RibbonStatus {
    pub(crate) verdict: String,
    pub(crate) color: Color,
    pub(crate) detail: Option<String>,
    verdict_w: f32,
}

/// The separator between a status row's verdict and its detail.
const STATUS_SEPARATOR: &str = " \u{b7} ";

/// The armed-mode ribbon, while a mode is armed.
pub(super) fn ribbon_layout(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    regions: &[Rect; 2],
    minimap: Rect,
    env: super::hud::HudEnv,
    measure: super::hud::Measure<'_>,
) -> Option<RibbonLayout> {
    use super::hud::Face;
    let mode = input.armed_mode()?;
    let (viewport, s) = (env.viewport, env.ui);
    let building = match mode {
        crate::input::ArmedMode::Build(kind) => Some(kind),
        _ => None,
    };
    // A stroke's planned row counts itself and its total.
    let planned = input.planned_stamps().len();
    let label = if planned > 1 {
        format!("{} \u{d7}{planned}", mode.label())
    } else {
        mode.label()
    };
    let cost = building
        .and_then(|kind| kind.base_stats().construction)
        .map(|construction| (construction.cost * planned.max(1).fit::<u32>()).to_string());
    let icon_w = if building.is_some() { 40.0 * s } else { 0.0 };
    let label_w = measure(Face::Body, &label, crate::theme::Type::Label.at(s));
    let cost_w = cost.as_deref().map_or(0.0, |cost| {
        12.0 * s + measure(Face::Body, cost, crate::theme::Type::Body.at(s))
    });
    let status_size = crate::theme::Type::Small.at(s);
    let status = building.and_then(|_| {
        let mut status = placement_status(game, input)?;
        status.verdict_w = measure(Face::Body, &status.verdict, status_size);
        Some(status)
    });
    // The row is as wide as its widest possible status, so the ribbon holds
    // still while the ghost moves between verdicts.
    let status_w = match (building, &status) {
        (Some(kind), Some(_)) => {
            widest_placement_status(kind, &|text| measure(Face::Body, text, status_size))
        }
        _ => 0.0,
    };
    let content = icon_w + (label_w + cost_w).max(status_w);
    let size = (
        ribbon_width(viewport, s, content),
        ribbon_height(s, status.is_some()),
    );
    // The ribbon sits above whichever panel region lies under it.
    let open = ribbon_geometry(viewport, s, f32::INFINITY, size, minimap);
    let panel_top = regions
        .iter()
        .filter(|r| r.w > 0.0 && r.x < open.x + open.w && r.x + r.w > open.x)
        .map(|r| r.y)
        .fold(f32::INFINITY, f32::min);
    Some(RibbonLayout {
        rect: ribbon_geometry(viewport, s, panel_top, size, minimap),
        label,
        cost,
        building,
        label_w,
        icon_w,
        status,
    })
}

/// What placing would do at the ghost or under the cursor, if either
/// names a spot.
pub(crate) fn placement_status(
    game: &crate::game::Scene<'_>,
    input: &InputState,
) -> Option<RibbonStatus> {
    use crate::input::PlacementVerdict;
    let spot = crate::input::preview_spot(game, input)?;
    // Over chrome a click does something else; the row stays, blank, so
    // the ribbon holds still as the cursor reaches it.
    if crate::input::cursor_on_chrome(game, input) {
        return Some(RibbonStatus {
            verdict: String::new(),
            color: TEXT_BODY,
            detail: None,
            verdict_w: 0.0,
        });
    }
    let (text, color) = match spot.verdict {
        PlacementVerdict::Builds => ("Builds now".to_string(), TEXT_BODY),
        PlacementVerdict::BuildsOnArrival => ("Builds on arrival".to_string(), TEXT_ACCENT),
        PlacementVerdict::Refused(refusal) => (
            crate::typography::sentence_case(crate::input::refusal_reason(refusal)),
            crate::theme::TEXT_DANGER,
        ),
        PlacementVerdict::Short(scrap) => (
            format!("Needs {scrap} more scrap"),
            crate::theme::TEXT_DANGER,
        ),
        PlacementVerdict::QueueFull => {
            ("Order queue is full".to_string(), crate::theme::TEXT_DANGER)
        }
        PlacementVerdict::Unreachable => (UNREACHABLE.to_string(), crate::theme::TEXT_DANGER),
    };
    Some(RibbonStatus {
        verdict: text,
        color,
        detail: spot.lands.map(lands_detail),
        verdict_w: 0.0,
    })
}

/// The status row's word for a site the founder cannot reach.
const UNREACHABLE: &str = "Your builders can't reach it";

/// Where a build lands, as the status row says it.
fn lands_detail(lands: crate::input::Lands) -> String {
    use crate::input::Lands;
    match lands {
        Lands::Queues(position) => format!("Queues as #{position}"),
        Lands::Replaces(1) => "Replaces 1 order".to_string(),
        Lands::Replaces(orders) => format!("Replaces {orders} orders"),
    }
}

/// The widest status row placing `kind` can show.
fn widest_placement_status(kind: oxide_sim::BuildingKind, measure: &dyn Fn(&str) -> f32) -> f32 {
    use oxide_sim::PlaceRefusal;
    let cost = kind.base_stats().construction.map_or(0, |c| c.cost);
    // A builder holds at most the cap's worth of queue behind its order.
    let most = oxide_sim::stats::ORDER_QUEUE_CAP + 1;
    let detail = [
        crate::input::Lands::Queues(most),
        crate::input::Lands::Replaces(most),
    ]
    .into_iter()
    .map(|lands| measure(&format!("{STATUS_SEPARATOR}{}", lands_detail(lands))))
    .fold(0.0, f32::max);
    let building = ["Builds now", "Builds on arrival"]
        .into_iter()
        .map(|text| measure(text) + detail)
        .fold(0.0, f32::max);
    let refusals = [
        PlaceRefusal::Fog,
        PlaceRefusal::Terrain,
        PlaceRefusal::Building,
        PlaceRefusal::Unit,
        PlaceRefusal::NotConstructible,
        PlaceRefusal::Prerequisite,
        PlaceRefusal::FrameRequired,
        PlaceRefusal::FrameBlocked,
    ]
    .into_iter()
    .filter(|refusal| {
        *refusal != PlaceRefusal::FrameRequired || kind == oxide_sim::BuildingKind::Extractor
    })
    .map(|refusal| crate::typography::sentence_case(crate::input::refusal_reason(refusal)))
    .chain([
        format!("Needs {cost} more scrap"),
        "Order queue is full".to_string(),
        UNREACHABLE.to_string(),
    ])
    .map(|text| measure(&text))
    .fold(0.0, f32::max);
    building.max(refusals)
}

fn draw_mode_ribbon(sprites: &Sprites, layout: &RibbonLayout, s: f32) {
    let RibbonLayout {
        rect: ribbon,
        label,
        cost,
        building,
        label_w,
        icon_w,
        status,
    } = layout;
    let (ribbon, building, label_w, icon_w) = (*ribbon, *building, *label_w, *icon_w);
    // The label keeps the touch-target row; a status row hangs under it.
    let row_h = crate::theme::MIN_TOUCH_TARGET * s;
    let (label_size, cost_size) = (
        crate::theme::Type::Label.at(s),
        crate::theme::Type::Body.at(s),
    );
    fill_rect(ribbon, crate::theme::SURFACE_PLATE);
    stroke_rect(ribbon, crate::theme::Stroke::Edge.at(s), SCRAP_COLOR);
    let mut x = ribbon.x + 10.0 * s;
    if let Some(kind) = building {
        // The works being placed is the viewer's own.
        let owner = crate::render::self_color();
        let mut layers = vec![
            (sprites.building_tiered(kind, 0), WHITE),
            (sprites.building_tiered_accent(kind, 0), owner),
        ];
        if let Some(mount) = sprites.defense_mount(kind, 0) {
            layers.push((mount, WHITE));
            if let Some(accent) = sprites.defense_mount_accent(kind, 0) {
                layers.push((accent, owner));
            }
        }
        let icon = Rect::new(x, ribbon.y + (row_h - 32.0 * s) * 0.5, 32.0 * s, 32.0 * s);
        sprites.draw_portrait(icon, &layers);
        x += icon_w;
    }
    let baseline = ribbon.y + row_h * 0.64;
    draw_text(label, x, baseline, label_size, TEXT_PRIMARY);
    if let Some(cost) = cost {
        draw_text(
            cost,
            x + label_w + 12.0 * s,
            baseline,
            cost_size,
            SCRAP_COLOR,
        );
    }
    if let Some(status) = status {
        let size = crate::theme::Type::Small.at(s);
        let baseline = ribbon.y + ribbon.h - 8.0 * s;
        draw_text(&status.verdict, x, baseline, size, status.color);
        if let Some(detail) = &status.detail {
            draw_text(
                format!("{STATUS_SEPARATOR}{detail}"),
                x + status.verdict_w,
                baseline,
                size,
                crate::theme::TEXT_SECONDARY,
            );
        }
    }
}

/// Where toast text line `line` (0 is the newest, lowest line) starts.
fn toast_origin(
    viewport: Vec2,
    scale: f32,
    panel_top: f32,
    orders: Rect,
    row: Rect,
    line: usize,
) -> Vec2 {
    let x = if orders.w > 0.0 {
        orders.x + orders.w + 12.0 * scale
    } else {
        12.0 * scale
    };
    // Toasts stack above the ribbon row when it shows, so neither hides
    // the other.
    let panel_top = if row.w > 0.0 {
        panel_top.min(row.y)
    } else {
        panel_top
    };
    let newest = if panel_top.is_finite() {
        panel_top - 12.0 * scale
    } else {
        viewport.y - 24.0 * scale
    };
    vec2(
        x,
        (newest - TOAST_LINE * line as f32 * scale).max((crate::layout::TOP_BAR_H + 18.0) * scale),
    )
}

/// The spacing between toast lines, in logical px.
const TOAST_LINE: f32 = 24.0;

/// How far right toast text may reach: short of any chrome in `obstacles`
/// that shares the rows from `top` to `bottom`, else the window's edge.
/// The minimap and control-group plate draw over toasts.
fn toast_right(viewport_w: f32, scale: f32, top: f32, bottom: f32, obstacles: &[Rect]) -> f32 {
    obstacles
        .iter()
        .filter(|rect| rect.w > 0.0 && rect.y < bottom && rect.y + rect.h > top)
        .map(|rect| rect.x)
        .fold(viewport_w, f32::min)
        - 12.0 * scale
}

/// A toast's text size and lines within `width`: shrunk from the heading
/// size down to body size, then wrapped rather than run past `width`.
fn toast_lines(
    text: &str,
    width: f32,
    scale: f32,
    measure: impl Fn(&str, f32) -> f32,
) -> (f32, Vec<String>) {
    let floor = crate::theme::Type::Body.at(scale);
    let mut size = crate::theme::Type::Heading.at(scale);
    while measure(text, size) > width && size > floor {
        size = (size - scale).max(floor);
    }
    if measure(text, size) <= width {
        return (size, vec![text.to_string()]);
    }
    (
        size,
        super::wrap_words(text, |line| measure(line, size), width),
    )
}

/// The top bar's texts and where they sit.
pub(crate) struct TopBarLayout {
    pub(super) bar: crate::layout::TopBar,
    width: f32,
    scrap: String,
    passive: String,
    units: String,
    idle: Option<String>,
    alert: Option<String>,
    status: String,
}

impl TopBarLayout {
    /// The idle badge; zero-sized while nobody idles.
    pub(super) fn idle_badge(&self) -> Rect {
        self.bar.idle_badge
    }

    /// The under-attack badge; zero-sized without a recent alert.
    pub(super) fn alert_badge(&self) -> Rect {
        self.bar.alert_badge
    }
}

/// Lays out the top bar: the bank, income, unit count, idle and alert
/// badges, and the clock with the menu button.
pub(super) fn top_bar_layout(
    game: &crate::game::Scene<'_>,
    bindings: &crate::action::BindingMap,
    env: super::hud::HudEnv,
    measure: super::hud::Measure<'_>,
) -> TopBarLayout {
    use super::hud::Face;
    use crate::action::{Action, BindingMap};
    let s = env.ui;
    let label = |action| {
        bindings
            .chord_for(action)
            .map(BindingMap::chord_label)
            .unwrap_or_default()
    };
    let scrap = game.state.player(game.presentation.human).scrap;
    let passive = crate::panel::passive_income(game);
    let my_units = game
        .state
        .units()
        .iter()
        .filter(|unit| unit.player == game.presentation.human)
        .count();
    let scrap = scrap.to_string();
    let passive = format!("+{passive}/min passive");
    let units = my_units.to_string();
    let keyless = !crate::platform::hands().keys;
    let idle = crate::input::idle_harvesters(game).len();
    let idle = (idle > 0).then(|| idle_badge_text(idle, &label(Action::CycleIdleWorker), keyless));
    // Alerts age out in a few seconds and hold while paused, so the
    // badge shows exactly while the minimap still pulses one.
    let alert = game
        .presentation
        .alerts
        .iter()
        .map(|&(_, _, kind)| kind)
        .min_by_key(|kind| *kind != crate::game::AlertKind::Attack)
        .map(|kind| alert_badge_text(kind, &label(Action::JumpToLastAlert), keyless));
    let status = if game.clock.paused {
        paused_status(&label(Action::TogglePause), keyless)
    } else if (game.clock.speed - 1.0).abs() > f64::EPSILON {
        format!("x{:.2}", game.clock.speed)
    } else {
        crate::clock_label::ticks(game.state.current_tick())
    };
    let bar = crate::layout::top_bar(
        env.viewport.x,
        s,
        crate::platform::TOUCH_ONLY,
        crate::layout::TopBarText {
            scrap: measure(Face::Display, &scrap, crate::theme::Type::Heading.at(s)),
            passive: measure(Face::Body, &passive, crate::theme::Type::Body.at(s)),
            units_label: measure(Face::Display, "UNITS", crate::theme::Type::Small.at(s)),
            units: measure(Face::Display, &units, crate::theme::Type::Heading.at(s)),
            idle: idle
                .as_deref()
                .map(|text| measure(Face::Body, text, crate::theme::Type::Body.at(s))),
            alert: alert
                .as_deref()
                .map(|text| measure(Face::Body, text, crate::theme::Type::Body.at(s))),
            status: measure(Face::Display, &status, crate::theme::Type::Small.at(s)),
        },
    );
    TopBarLayout {
        bar,
        width: env.viewport.x,
        scrap,
        passive,
        units,
        idle,
        alert,
        status,
    }
}

fn draw_top_bar(top: &TopBarLayout, s: f32) {
    let TopBarLayout {
        bar,
        width,
        scrap,
        passive,
        units,
        idle,
        alert,
        status,
    } = top;
    draw_rectangle(0.0, 0.0, *width, crate::layout::TOP_BAR_H * s, PANEL);
    crate::typography::draw(
        "SCRAP",
        bar.scrap_label_x,
        26.0 * s,
        crate::theme::Type::Small.at(s),
        TEXT_SECONDARY,
    );
    crate::typography::draw(
        scrap,
        bar.scrap_x,
        27.0 * s,
        crate::theme::Type::Heading.at(s),
        SCRAP_COLOR,
    );
    draw_text(
        passive,
        bar.passive_x,
        26.0 * s,
        crate::theme::Type::Body.at(s),
        TEXT_BODY,
    );
    crate::typography::draw(
        "UNITS",
        bar.units_x,
        26.0 * s,
        crate::theme::Type::Small.at(s),
        TEXT_SECONDARY,
    );
    crate::typography::draw(
        units,
        bar.count_x,
        27.0 * s,
        crate::theme::Type::Heading.at(s),
        TEXT_PRIMARY,
    );
    if let Some(text) = idle {
        fill_rect(bar.idle_badge, crate::theme::BADGE);
        draw_text(
            text,
            bar.idle_badge.x + 9.0 * s,
            26.0 * s,
            crate::theme::Type::Body.at(s),
            SCRAP_COLOR,
        );
    }
    if let Some(text) = alert {
        fill_rect(bar.alert_badge, crate::theme::BADGE);
        draw_text(
            text,
            bar.alert_badge.x + 9.0 * s,
            26.0 * s,
            crate::theme::Type::Body.at(s),
            crate::theme::TEXT_DANGER,
        );
    }
    draw_menu_button(bar.menu_button, s);
    crate::typography::draw(
        status,
        bar.status_x,
        26.0 * s,
        crate::theme::Type::Small.at(s),
        TEXT_PRIMARY,
    );
}

/// Draws the HUD the layout pass placed: the top bar, the command panel,
/// the control groups, the armed-mode ribbon, the QUEUE toggle, the
/// performance readout, toasts, and the spectator strip.
pub(crate) fn draw_hud(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    input: &InputState,
    bindings: &crate::action::BindingMap,
    hud: &super::hud::HudGeometry,
    performance: Option<&crate::performance::PerformanceView>,
) {
    let s = ui_scale();
    if let Some(top_bar) = &hud.top_bar {
        draw_top_bar(top_bar, s);
    }
    if let (Some(panel), Some(layout)) = (
        game.presentation.panel_model.borrow().as_ref(),
        hud.panel.as_ref(),
    ) {
        draw_panel(sprites, input, bindings, panel, layout);
    }
    if let Some(column) = &hud.group_column {
        draw_group_column(column, s);
    }
    if let Some(ribbon) = &hud.ribbon {
        draw_mode_ribbon(sprites, ribbon, s);
    }
    if let Some(rect) = hud.queue_toggle {
        draw_queue_toggle(rect, input.queue_toggle, s);
    }
    if let (Some(view), Some(layout)) = (performance, &hud.performance) {
        super::performance::draw(view, layout);
    }
    let layout = game.presentation.layout.get();
    let panel_regions = layout.panel_regions;
    let orders_dock = layout.orders;
    let queue_toggle = layout.queue_toggle;
    let mode_ribbon = layout.mode_ribbon;

    // Toasts: rejected orders and stalled units, newest at the bottom.
    let origin = |line: usize| {
        toast_origin(
            vec2(screen_width(), screen_height()),
            s,
            if panel_regions[1].w > 0.0 {
                panel_regions[1].y
            } else {
                screen_height()
            },
            Rect::new(
                0.0,
                0.0,
                orders_dock
                    .w
                    .max(panel_regions[0].w)
                    .max(queue_toggle.x + queue_toggle.w),
                0.0,
            ),
            mode_ribbon,
            line,
        )
    };
    let newest = origin(0);
    let right = toast_right(
        screen_width(),
        s,
        newest.y - (3.0 * TOAST_LINE + 22.0) * s,
        newest.y + 6.0 * s,
        &[layout.minimap, layout.group_column],
    );
    let mut lines_below = 0;
    for toast in game.presentation.toasts.iter().rev().take(3) {
        let fade = (1.0 - (toast.age - 1.5).max(0.0)).clamp(0.0, 1.0);
        let (size, lines) =
            toast_lines(&toast.text, (right - newest.x).max(1.0), s, |text, size| {
                measure_text(text, None, numeric::font_size(size), 1.0).width
            });
        let color = Color {
            a: fade,
            ..crate::theme::TEXT_ALERT
        };
        for (index, line) in lines.iter().enumerate() {
            let at = origin(lines_below + lines.len() - 1 - index);
            draw_text(line, at.x, at.y, size, color);
        }
        lines_below += lines.len();
    }

    // Spectator strip: a foundry-less or resigned seat on a living team
    // stays in the match (its machines finish their orders and the team
    // plays on), so tell the human the seat can no longer act. Commands
    // still route; the sim rejects them.
    let resigned = game.state.player(game.presentation.human).resigned;
    if game.state.result().is_none() && !game.state.accepts_commands(game.presentation.human) {
        let text = if resigned {
            "SURRENDERED - SPECTATING"
        } else {
            "ELIMINATED - SPECTATING"
        };
        let dims = measure_text(
            text,
            None,
            numeric::font_size(crate::theme::Type::Heading.at(s)),
            1.0,
        );
        let x = (screen_width() - dims.width) * 0.5;
        draw_rectangle(
            x - 12.0 * s,
            40.0 * s,
            dims.width + 24.0 * s,
            30.0 * s,
            PANEL,
        );
        draw_text(text, x, 60.0 * s, crate::theme::Type::Heading.at(s), DANGER);
    }

    // The network's word on a LAN match, pinned under the spectator strip.
    if let Some(text) = &game.presentation.net_notice {
        let spectating =
            game.state.result().is_none() && !game.state.accepts_commands(game.presentation.human);
        let top = if spectating { 76.0 } else { 40.0 } * s;
        let size = crate::theme::Type::Heading.at(s);
        let dims = measure_text(text, None, numeric::font_size(size), 1.0);
        let x = (screen_width() - dims.width) * 0.5;
        draw_rectangle(x - 12.0 * s, top, dims.width + 24.0 * s, 30.0 * s, PANEL);
        draw_text(text, x, top + 20.0 * s, size, crate::theme::TEXT_ALERT);
    }
}

/// A team-game concession can leave the match undecided while the ally
/// keeps fighting. This compact exit offer is the only result layer
/// gameplay draws; a decided match moves to the Results screen.
pub(crate) fn draw_result_overlay(game: &crate::game::Scene<'_>) {
    if !game.presentation.conceded_banner || game.state.result().is_some() {
        return;
    }
    let s = ui_scale();
    let text = "SURRENDERED";
    let size = crate::theme::Type::Title.at(s);
    let dims = measure_text(text, None, numeric::font_size(size), 1.0);
    let x = (screen_width() - dims.width) * 0.5;
    let y = screen_height() * 0.38;
    draw_rectangle(
        x - 24.0 * s,
        y - 48.0 * s,
        dims.width + 48.0 * s,
        112.0 * s,
        PANEL,
    );
    draw_text(text, x, y, size, DANGER);
    let sub = crate::menu::binding_hint(concede_hint(crate::platform::hands()));
    let sub_dims = measure_text(
        &sub,
        None,
        numeric::font_size(crate::theme::Type::Label.at(s)),
        1.0,
    );
    draw_text(
        &sub,
        (screen_width() - sub_dims.width) * 0.5,
        y + 28.0 * s,
        crate::theme::Type::Label.at(s),
        TEXT_BODY,
    );
}

#[cfg(test)]
mod tests;
