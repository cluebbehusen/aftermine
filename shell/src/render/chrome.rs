//! Standing chrome and overlays: the top bar with its controls hint,
//! toasts, the salvage hover tooltip, the omniscient debug overlay,
//! and the endgame verdict. Their geometry comes from the HUD layout
//! pass (`super::hud`), the same one input hit-tests.

use super::*;
use crate::numeric;
use crate::numeric::Fit;
use crate::render::prim::{fill_circle, fill_rect, line_between, stroke_circle, stroke_rect};
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
        crate::game::AlertKind::Assembly(machine) => format!(
            "enemy {} assembling",
            crate::typography::entity_name(machine.name())
        ),
    };
    if keyless || key.is_empty() {
        what
    } else {
        format!("{what} [{key}]")
    }
}

/// The idle badge names its key where one exists, and an unbound key reads
/// bare.
fn idle_badge_text(idle: usize, key: &str, keyless: bool) -> String {
    if keyless || key.is_empty() {
        format!("{idle} idle")
    } else {
        format!("{idle} idle [{key}]")
    }
}

/// The concede banner's line: that the team fights on, when a teammate
/// still plays, then the way to the menu.
fn concede_hint(hands: crate::platform::Hands, team_fights_on: bool) -> String {
    // A controller opens the menu with its own Menu button; pad mode
    // hides the top bar's.
    let way = if hands.pad.is_some() {
        "{menu} menu"
    } else {
        match (hands.touch(), hands.keys) {
            (false, true) => "{back} menu",
            (false, false) => "click the menu button",
            (true, true) => "{back} or tap the menu button",
            (true, false) => "tap the menu button",
        }
    };
    if team_fights_on {
        format!("your team fights on | {way}")
    } else {
        way.to_owned()
    }
}

/// A control-group slot: its number in the corner and, for a saved
/// group, the machines it holds; the "+" offers to save the selection,
/// and an empty group is a faint outline. The group the selection
/// already is wears an accent rim.
fn draw_group_slot(
    sprites: &Sprites,
    rect: Rect,
    slot: crate::layout::GroupSlot,
    count: usize,
    lead: Option<&[(Rect, Color)]>,
    current: bool,
    s: f32,
) {
    let number = slot.number();
    match slot {
        crate::layout::GroupSlot::Recall(_) => {
            fill_rect(rect, crate::theme::BADGE);
            let text = count.to_string();
            if let Some(layers) = lead {
                // The lead kind fills the slot under its number, and the
                // count moves to the bottom-right corner.
                let side = rect.h * 0.62;
                sprites.draw_portrait(
                    Rect::new(
                        rect.x + (rect.w - side) * 0.5,
                        rect.y + (rect.h - side) * 0.5,
                        side,
                        side,
                    ),
                    layers,
                );
                let size = crate::theme::Type::Small.at(s);
                let dims = measure_text(&text, None, numeric::font_size(size), 1.0);
                draw_text(
                    &text,
                    rect.x + rect.w - dims.width - 3.0 * s,
                    rect.y + rect.h - 3.0 * s,
                    size,
                    SCRAP_COLOR,
                );
            } else {
                let mut size = crate::theme::Type::Body.at(s);
                while measure_text(&text, None, numeric::font_size(size), 1.0).width
                    > rect.w - 14.0 * s
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
    /// Each group's lead kind, drawn in its slot.
    leads: [Option<crate::panel::CardIcon>; crate::action::CONTROL_GROUPS],
    /// The viewer's color, which the lead art wears.
    owner: Color,
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
    let column = crate::layout::group_column(s, crate::platform::PLATFORM.touch_only(), minimap)
        .filter(|_| shown)?;
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
        leads: input.group_leads(game),
        owner: crate::render::seat_identity_color(game, game.presentation.human),
        current: input.selected_group(game),
    })
}

fn draw_group_column(sprites: &Sprites, column: &GroupColumnLayout, s: f32) {
    fill_rect(column.plate, crate::theme::SURFACE_PLATE);
    stroke_rect(
        column.plate,
        crate::theme::Stroke::Edge.at(s),
        crate::theme::BORDER_STRONG,
    );
    for ((rect, slot, count), lead) in column.slots.into_iter().zip(column.leads) {
        let lead = lead
            .filter(|_| matches!(slot, crate::layout::GroupSlot::Recall(_)))
            .and_then(|icon| match icon {
                crate::panel::CardIcon::Unit(kind) => Some(super::panel_draw::unit_portrait(
                    sprites,
                    kind,
                    WHITE,
                    column.owner,
                )),
                crate::panel::CardIcon::Building(kind, tier) => {
                    Some(super::panel_draw::building_portrait(
                        sprites,
                        kind,
                        tier,
                        true,
                        WHITE,
                        column.owner,
                    ))
                }
                _ => None,
            });
        draw_group_slot(
            sprites,
            rect,
            slot,
            count,
            lead.as_deref(),
            column.current == Some(slot.number()),
            s,
        );
    }
}

/// The army strip as laid out this frame, with how many units each button
/// would select.
pub(crate) struct ArmyStripLayout {
    pub(super) strip: crate::layout::ArmyStrip,
    all: usize,
    in_view: usize,
}

/// The army strip on the minimap. It shows from the first frame, empty
/// buttons dimmed, so it never shifts the group column mid-match.
pub(super) fn army_strip_layout(
    game: &crate::game::Scene<'_>,
    minimap: Rect,
    s: f32,
) -> Option<ArmyStripLayout> {
    let strip = crate::layout::army_strip(s, crate::platform::PLATFORM.touch_only(), minimap)?;
    Some(ArmyStripLayout {
        strip,
        all: crate::input::army_units(game, false).len(),
        in_view: crate::input::army_units(game, true).len(),
    })
}

/// What pressing an army button would do, for its tooltip, naming its key
/// where one exists.
fn army_tooltip(reach: crate::layout::ArmyReach, count: usize, key: &str, keyless: bool) -> String {
    use crate::layout::ArmyReach;
    let what = match (reach, count) {
        (ArmyReach::All, 0) => "No army units".to_string(),
        (ArmyReach::All, 1) => "Select your 1 army unit".to_string(),
        (ArmyReach::All, n) => format!("Select all {n} army units"),
        (ArmyReach::InView, 0) => "No army units in view".to_string(),
        (ArmyReach::InView, 1) => "Select the 1 army unit in view".to_string(),
        (ArmyReach::InView, n) => format!("Select the {n} army units in view"),
    };
    if keyless || key.is_empty() {
        what
    } else {
        format!("{what}   [{key}]")
    }
}

/// The strip's buttons in the command cards' look: a quiet fill and thin
/// edge, lit under the cursor, dimmed while they would select nothing.
fn draw_army_strip(layout: &ArmyStripLayout, input: &InputState, s: f32) {
    let strip = layout.strip;
    fill_rect(strip.plate, crate::theme::SURFACE_PLATE);
    stroke_rect(
        strip.plate,
        crate::theme::Stroke::Edge.at(s),
        crate::theme::BORDER_STRONG,
    );
    let hover = input.chrome_hover_point();
    for (rect, count) in [(strip.all, layout.all), (strip.in_view, layout.in_view)] {
        let live = count > 0;
        let hovered = live && hover.is_some_and(|p| rect.contains(p));
        let (fill, edge) = match (live, hovered) {
            (false, _) => (crate::theme::SURFACE_PLATE, crate::theme::BORDER_FAINT),
            (true, true) => (crate::theme::CARD_HOVER, crate::theme::EDGE_FOCUS),
            (true, false) => (crate::theme::CARD_IDLE, crate::theme::EDGE_CARD),
        };
        fill_rect(rect, fill);
        stroke_rect(rect, crate::theme::Stroke::Edge.at(s), edge);
    }
    draw_army_count(strip.all, layout.all, s);
    draw_viewfinder(
        strip.in_view,
        if layout.in_view > 0 {
            TEXT_PRIMARY
        } else {
            TEXT_DISABLED
        },
        s,
    );
}

/// The wide button's face: the ARMY label and the count side by side,
/// centered, or the count alone where the pair does not fit.
fn draw_army_count(rect: Rect, count: usize, s: f32) {
    let live = count > 0;
    let small = crate::theme::Type::Small.at(s);
    let big = crate::theme::Type::Label.at(s);
    let number = count.to_string();
    let label_w = crate::typography::measure("ARMY", small).width;
    let number_w = crate::typography::measure(&number, big).width;
    let gap = 10.0 * s;
    let labelled = label_w + gap + number_w <= rect.w - 16.0 * s;
    let face_w = if labelled {
        label_w + gap + number_w
    } else {
        number_w
    };
    let x = rect.x + (rect.w - face_w) * 0.5;
    let middle = rect.y + rect.h * 0.5;
    if labelled {
        crate::typography::draw(
            "ARMY",
            x,
            middle + 4.5 * s,
            small,
            if live { TEXT_SECONDARY } else { TEXT_DISABLED },
        );
    }
    crate::typography::draw(
        &number,
        x + face_w - number_w,
        middle + 6.5 * s,
        big,
        if live { SCRAP_COLOR } else { TEXT_DISABLED },
    );
}

/// The in-view button's glyph: a viewfinder's four corner brackets around
/// a dot, drawn as whole-rect fills so it stays crisp at any scale.
fn draw_viewfinder(rect: Rect, color: Color, s: f32) {
    let c = rect.center();
    let (half, arm, t) = (9.0 * s, 6.0 * s, (2.0 * s).max(1.0));
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        // Each bracket runs inward from its corner of the frame.
        let x = if sx < 0.0 {
            c.x - half
        } else {
            c.x + half - arm
        };
        let y = if sy < 0.0 {
            c.y - half
        } else {
            c.y + half - arm
        };
        let edge_x = if sx < 0.0 { c.x - half } else { c.x + half - t };
        let edge_y = if sy < 0.0 { c.y - half } else { c.y + half - t };
        fill_rect(Rect::new(x, edge_y, arm, t), color);
        fill_rect(Rect::new(edge_x, y, t, arm), color);
    }
    let dot = 3.0 * s;
    fill_rect(Rect::new(c.x - dot * 0.5, c.y - dot * 0.5, dot, dot), color);
}

/// The army button under the cursor, or under a resting finger, says what
/// a press selects and names its key where one exists.
pub(crate) fn draw_army_tooltip(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    bindings: &crate::action::BindingMap,
) {
    let s = ui_scale();
    let layout = game.presentation.layout.get();
    let pointer = match input.touch_preview() {
        Some(p) => Some((p, Some(s))),
        None => input.chrome_hover_point().map(|p| (p, None)),
    };
    let Some(reach) =
        pointer.and_then(|(p, touch_ui)| crate::layout::army_button_under(&layout, p, touch_ui))
    else {
        return;
    };
    let Some((anchor, _)) = layout
        .army_buttons
        .iter()
        .flatten()
        .find(|(_, button)| *button == reach)
    else {
        return;
    };
    let count = crate::input::army_units(game, reach == crate::layout::ArmyReach::InView).len();
    // As the top bar's hints do, the key shows once a keyboard is in use
    // and only while one is bound.
    let key = bindings
        .chord_for(reach.action())
        .map(crate::action::BindingMap::chord_label)
        .unwrap_or_default();
    let text = army_tooltip(reach, count, &key, !crate::platform::hands().keys);
    let size = crate::theme::Type::Body.at(s);
    let pad = 12.0 * s;
    let width = measure_text(&text, None, numeric::font_size(size), 1.0).width + pad * 2.0;
    let height = 22.0 * s + pad * 1.5;
    let origin = crate::layout::tooltip_origin(
        *anchor,
        vec2(width, height),
        crate::layout::TooltipSide::Above,
        vec2(screen_width(), anchor.y),
        layout.top_bar_h,
        6.0 * s,
    );
    let rect = Rect::new(origin.x, origin.y, width, height);
    fill_rect(rect, crate::theme::SURFACE_CARD);
    stroke_rect(
        rect,
        crate::theme::Stroke::Edge.at(s),
        crate::theme::EDGE_CARD,
    );
    draw_text(
        &text,
        rect.x + pad,
        rect.y + pad + 0.6 * 22.0 * s,
        size,
        TEXT_PRIMARY,
    );
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

/// The pause button shows what a press does: the pause sign while the
/// match runs, and a lit play sign while it is paused, so a paused match
/// shows its way back at a glance.
fn draw_pause_button(rect: Rect, paused: bool, s: f32) {
    let middle = vec2(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    let sign_h = 14.0 * s;
    if paused {
        fill_rect(rect, SCRAP_COLOR);
        let sign_w = 12.0 * s;
        // The triangle's centroid sits a third of its width from the
        // flat side; centering that reads as centered.
        let left = middle.x - sign_w / 3.0;
        draw_triangle(
            vec2(left, middle.y - sign_h * 0.5),
            vec2(left, middle.y + sign_h * 0.5),
            vec2(left + sign_w, middle.y),
            crate::theme::SURFACE_PLATE,
        );
    } else {
        fill_rect(rect, crate::theme::BADGE);
        let bar_w = 4.0 * s;
        let gap = 4.0 * s;
        for x in [middle.x - gap * 0.5 - bar_w, middle.x + gap * 0.5] {
            fill_rect(
                Rect::new(x, middle.y - sign_h * 0.5, bar_w, sign_h),
                SCRAP_COLOR,
            );
        }
    }
}

/// The queue strip while a controller has it open: its lit entry outlined
/// as the menus' cursor is, and the buttons that drive it on a plate above
/// the dock.
pub(crate) fn draw_queue_strip(
    game: &crate::game::Scene<'_>,
    sprites: &crate::assets::Sprites,
    input: &InputState,
) {
    use aftermine_protocol::PadButton;
    let Some(focus) = input.dock.as_ref() else {
        return;
    };
    let s = ui_scale();
    let lit = focus.lit();
    let grow = 3.0 * s;
    let outline = Rect::new(
        lit.rect.x - grow,
        lit.rect.y - grow,
        lit.rect.w + 2.0 * grow,
        lit.rect.h + 2.0 * grow,
    );
    let weight = crate::platform::cursor_stroke().at(s);
    stroke_rect(outline, weight + 2.0 * s, Color::new(0.05, 0.05, 0.07, 0.7));
    stroke_rect(outline, weight, crate::theme::TEXT_TITLE);
    let family = crate::platform::hands()
        .pad
        .unwrap_or(aftermine_protocol::PadFamily::Xbox);
    let icon = |button| crate::pad::button_icon(family, button);
    let mut legend = crate::hint_line::HintLine::default();
    if let Some(verb) = strip_verb(lit.action) {
        legend = legend
            .pad(icon(PadButton::South))
            .text(format!(" {verb} - "));
    }
    let legend = legend.pad(icon(PadButton::East)).text(" close");
    let layout = game.presentation.layout.get();
    let columns = [layout.orders, layout.drones];
    let mut columns = columns.iter().filter(|rect| rect.w > 0.0);
    let Some(first) = columns.next() else {
        return;
    };
    let top = columns.fold(first.y, |top, rect| top.min(rect.y));
    let size = crate::theme::Type::Body.at(s);
    let pad = 6.0 * s;
    let plate = Rect::new(
        first.x,
        top - size * 1.6 - 2.0 * pad,
        legend.width(size) + 2.0 * pad,
        size * 1.6 + pad,
    );
    fill_rect(plate, crate::theme::SURFACE_PLATE);
    legend.draw_at(
        sprites,
        plate.x + pad,
        plate.y + plate.h * 0.5 + size * 0.35,
        size,
        crate::theme::TEXT_SECONDARY,
        crate::theme::TEXT_BODY,
    );
}

/// What A does to the queue strip's lit entry, as its legend names it.
fn strip_verb(action: crate::panel::CardAction) -> Option<&'static str> {
    use crate::panel::CardAction;
    match action {
        CardAction::Dispatch(crate::action::Action::StopOrScrap) => Some("stop"),
        CardAction::ClearQueues => Some("clear queues"),
        action if action.discards_work() => Some("cancel"),
        _ => None,
    }
}

/// The controller's reticle, while a controller is the pointer: a ring with
/// four ticks, which closes into brackets around the unit or structure it
/// has snapped onto. While A paints, the brush around it is the reticle
/// grown, and while a structure is armed its ghost is, tinted with what a
/// press would do, so only its center dot remains. A dark under-stroke
/// keeps it legible on pale ground.
/// Only the live match draws it, over everything as a cursor is: the same
/// input state reaches the backdrops behind menus and the replay viewer.
pub(crate) fn draw_reticle(game: &crate::game::Scene<'_>, input: &InputState) {
    // An open ring's hub stands where the reticle would.
    if input.last_pointer != crate::input::Pointer::Pad || input.ring.is_some() {
        return;
    }
    let alpha = game.clock.render_alpha();
    let s = ui_scale();
    let camera = &game.presentation.camera;
    let shade = Color::new(0.05, 0.05, 0.07, 0.6);
    if crate::input::painting(input) || input.placing.is_some() {
        for (thickness, color) in [(3.5 * s, shade), (1.5 * s, BONE)] {
            fill_circle(input.reticle.point(), thickness, color);
        }
        return;
    }
    // The snap was chosen before this frame's ticks, so check it again.
    let snap = input.reticle.snap().filter(|snap| snap.seen(game));
    let frame = snap.and_then(|snap| match snap {
        crate::input::reticle::Picked::Unit(id, _) => {
            let unit = game.state.unit(id)?;
            let at = camera.to_screen(game.presentation.draw_pos(id, unit.pos, alpha));
            let radius = crate::input::unit_pick_radius(unit.kind) * camera.zoom;
            Some((at, Vec2::splat(radius)))
        }
        crate::input::reticle::Picked::Building(id, _) => {
            let building = game.state.building(id)?;
            let (w, h) = building.kind.size();
            let min = vec2(building.anchor.x as f32, building.anchor.y as f32);
            let a = camera.to_screen(min);
            let b = camera.to_screen(min + vec2(w as f32, h as f32));
            Some(((a + b) * 0.5, (b - a) * 0.5))
        }
    });
    let point = input.reticle.point();
    for (thickness, color) in [(3.5 * s, shade), (1.5 * s, BONE)] {
        if let Some((center, half)) = frame {
            // Brackets a little outside the target, each arm a third of
            // the shorter side.
            let half = half + Vec2::splat(4.0 * s);
            let arm = (half.min_element() * 0.66).max(5.0 * s);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let corner = center + vec2(sx * half.x, sy * half.y);
                line_between(corner, corner - vec2(sx * arm, 0.0), thickness, color);
                line_between(corner, corner - vec2(0.0, sy * arm), thickness, color);
            }
            fill_circle(point, thickness, color);
        } else {
            let radius = 11.0 * s;
            stroke_circle(point, radius, thickness, color);
            for dir in [
                vec2(1.0, 0.0),
                vec2(-1.0, 0.0),
                vec2(0.0, 1.0),
                vec2(0.0, -1.0),
            ] {
                line_between(
                    point + dir * (radius - 4.0 * s),
                    point + dir * (radius + 5.0 * s),
                    thickness,
                    color,
                );
            }
        }
    }
}

/// Hovered salvage says what it holds, on the HUD's layer, so it never
/// shows over a match seen behind a menu.
pub(crate) fn draw_salvage_chip(chip: &super::hud::SalvageChip) {
    let s = ui_scale();
    let (x, y) = (chip.at.x, chip.at.y);
    draw_rectangle(
        x - 4.0 * s,
        y - 14.0 * s,
        chip.text_w + 8.0 * s,
        20.0 * s,
        PANEL,
    );
    draw_text(
        &chip.text,
        x,
        y,
        crate::theme::Type::Body.at(s),
        SCRAP_COLOR,
    );
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
            crate::theme::Type::Body.px(),
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
    touch_only
        && !game.presentation.spectating()
        && (commands_units(game) || input.queue_toggle != crate::input::QueueToggle::Off)
}

/// Whether the selection holds a unit of the human's, the only kind of
/// selection whose orders queue.
fn commands_units(game: &crate::game::Scene<'_>) -> bool {
    let human = game.presentation.human;
    game.presentation
        .selection
        .units
        .iter()
        .any(|id| game.state.unit(*id).is_some_and(|u| u.player == human))
}

/// How long the queue flag says orders keep queuing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueSpan {
    /// A latched QUEUE: the action at hand.
    Once,
    /// A locked QUEUE, or a held Shift or left trigger.
    On,
}

impl QueueSpan {
    fn label(self) -> &'static str {
        match self {
            Self::Once => "Queue once",
            Self::On => "Queue on",
        }
    }
}

/// The queue flag: an amber rule under the top bar and a tab hanging from
/// it that names the span. It shows wherever the player looks, since a
/// hand often covers the QUEUE toggle.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QueueFlagLayout {
    rule: Rect,
    tab: Rect,
    span: QueueSpan,
    label_w: f32,
}

/// The LAN notice plate's height under the top bar, in logical px.
const NET_NOTICE_H: f32 = 30.0;

/// The queue flag's tab height, in logical px.
const QUEUE_TAB_H: f32 = 28.0;
/// The chain glyph's width and its gap to the label, in logical px.
const QUEUE_TAB_GLYPH: f32 = 22.0;
const QUEUE_TAB_GAP: f32 = 8.0;
const QUEUE_TAB_PAD: f32 = 10.0;

/// The queue flag while orders queue: while the QUEUE toggle is lit, as
/// long as the toggle shows, and while a held key queues orders for a
/// selected own unit. A held Shift alone, with nothing to queue, also
/// drives hotkeys and adds to a selection; it raises no flag.
pub(super) fn queue_flag_layout(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    env: super::hud::HudEnv,
    performance: Rect,
    measure: super::hud::Measure<'_>,
) -> Option<QueueFlagLayout> {
    use crate::input::QueueToggle;
    let human = game.presentation.human;
    if game.presentation.spectating() || !game.state.accepts_commands(human) {
        return None;
    }
    let keys = input.queue_keys_held() && commands_units(game);
    let span = match input.queue_toggle {
        QueueToggle::Latched { .. } if !keys => QueueSpan::Once,
        QueueToggle::Off if !keys => return None,
        _ => QueueSpan::On,
    };
    let (viewport, s) = (env.viewport, env.ui);
    let top = crate::layout::TOP_BAR_H * s;
    let rule = Rect::new(0.0, top, viewport.x, crate::theme::Stroke::Heavy.at(s));
    // The tab is as wide as its widest label, so it holds still when a
    // double-tap locks QUEUE on.
    let size = crate::theme::Type::Body.at(s);
    let label_w = measure(super::hud::Face::Body, span.label(), size);
    let widest = [QueueSpan::Once, QueueSpan::On]
        .into_iter()
        .map(|span| measure(super::hud::Face::Body, span.label(), size))
        .fold(0.0, f32::max);
    let width = ((2.0 * QUEUE_TAB_PAD + QUEUE_TAB_GLYPH + QUEUE_TAB_GAP) * s + widest).round();
    // A LAN notice takes the spot under the bar; the tab hangs below it.
    let y = if game.presentation.net_notice.is_some() {
        top + (NET_NOTICE_H + 4.0) * s
    } else {
        rule.y + rule.h
    };
    // A lesson or tip card holds the corner under the bar and draws over
    // the HUD, and the performance readout may hold the other corner; the
    // tab keeps between them, or hangs under the readout when no room is
    // left between.
    let h = QUEUE_TAB_H * s;
    let left = game
        .corner_card
        .map_or(0.0, |card| (card.right(env) + 8.0 * s).round());
    let readout = (performance.w > 0.0 && performance.y < y + h && performance.bottom() > y)
        .then_some(performance);
    let right = readout.map_or(viewport.x - 12.0 * s, |panel| panel.x - 8.0 * s);
    let x = ((viewport.x - width) * 0.5)
        .round()
        .max(left)
        .min((right - width).round());
    let tab = match readout {
        Some(panel) if x < left => Rect::new(
            (panel.right() - width).round(),
            panel.bottom() + 4.0 * s,
            width,
            h,
        ),
        _ => Rect::new(x, y, width, h),
    };
    Some(QueueFlagLayout {
        rule,
        tab,
        span,
        label_w,
    })
}

impl QueueFlagLayout {
    /// The rule and the tab, which own their pixels against presses.
    pub(crate) fn regions(&self) -> [Rect; 2] {
        [self.rule, self.tab]
    }
}

/// Draws the queue flag: solid amber while orders keep queuing, the rule
/// dashed and the tab outlined while QUEUE covers only the action at hand,
/// as the toggle itself shows the two.
fn draw_queue_flag(flag: &QueueFlagLayout, s: f32) {
    let QueueFlagLayout {
        rule,
        tab,
        span,
        label_w,
    } = flag;
    let (rule, tab) = (*rule, *tab);
    let ink = match span {
        QueueSpan::On => {
            fill_rect(rule, TEXT_ACCENT);
            fill_rect(tab, TEXT_ACCENT);
            crate::theme::SURFACE_CARD
        }
        QueueSpan::Once => {
            let (dash, gap) = (16.0 * s, 10.0 * s);
            let mut x = rule.x;
            while x < rule.right() {
                let w = dash.min(rule.right() - x);
                fill_rect(Rect::new(x, rule.y, w, rule.h), TEXT_ACCENT);
                x += dash + gap;
            }
            fill_rect(tab, crate::theme::SURFACE_PLATE);
            stroke_rect(tab, crate::theme::Stroke::Heavy.at(s), TEXT_ACCENT);
            TEXT_ACCENT
        }
    };
    let content = (QUEUE_TAB_GLYPH + QUEUE_TAB_GAP) * s + label_w;
    let left = tab.x + (tab.w - content) * 0.5;
    draw_queue_chain(
        vec2(left + QUEUE_TAB_GLYPH * 0.5 * s, tab.center().y),
        0.75 * s,
        ink,
    );
    let size = crate::theme::Type::Body.at(s);
    draw_text(
        span.label(),
        left + (QUEUE_TAB_GLYPH + QUEUE_TAB_GAP) * s,
        tab.center().y + size * 0.32,
        size,
        ink,
    );
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
    draw_queue_chain(rect.center(), s, glyph);
}

/// QUEUE's mark: a chain of three queued waypoints, centered on `c` and
/// drawn at scale `s`.
fn draw_queue_chain(c: Vec2, s: f32, ink: Color) {
    let points = [
        c + vec2(-11.0, 6.0) * s,
        c + vec2(0.0, -6.0) * s,
        c + vec2(11.0, 6.0) * s,
    ];
    line_between(points[0], points[1], 2.0 * s, ink);
    line_between(points[1], points[2], 2.0 * s, ink);
    for point in points {
        fill_circle(point, 3.5 * s, ink);
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
    building: Option<aftermine_sim::BuildingKind>,
    label_w: f32,
    icon_w: f32,
    pub(super) status: Option<RibbonStatus>,
    pub(super) salvo: Option<SalvoSteppers>,
    pub(super) patrol: Option<PatrolButtons>,
}

/// A patrol ribbon's buttons: Start, lit once the route has a point, and
/// the cross that drops the route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatrolButtons {
    pub(crate) start: Rect,
    pub(crate) cancel: Rect,
    pub(crate) ready: bool,
}

/// The Start button's word.
const PATROL_START: &str = "Start";

/// A strike ribbon's buttons that shrink and grow the salvo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SalvoSteppers {
    pub(crate) less: Rect,
    pub(crate) more: Rect,
    less_enabled: bool,
    more_enabled: bool,
}

/// A salvo stepper's side, in logical px: the ribbon's touch row less a
/// margin, so a fingertip's padded target still reaches a whole row.
const STEPPER: f32 = crate::theme::MIN_TOUCH_TARGET - 8.0;

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
    let salvo = (mode == crate::input::ArmedMode::Strike)
        .then(|| crate::input::salvo_readout(game, input))
        .flatten();
    // A stroke's planned row counts itself and its total.
    let planned = input.planned_stamps().len();
    let label = if let Some((fires, ready)) = salvo {
        salvo_label(fires, ready)
    } else if planned > 1 {
        format!("{} \u{d7}{planned}", mode.label())
    } else {
        mode.label()
    };
    let cost = building.map(|kind| {
        (kind.base_stats().construction.cost * planned.max(1).fit::<u32>()).to_string()
    });
    let icon_w = if building.is_some() { 40.0 * s } else { 0.0 };
    let label_size = crate::theme::Type::Label.at(s);
    let label_w = measure(Face::Body, &label, label_size);
    let patrol = match mode {
        crate::input::ArmedMode::Patrol(points) => Some(points > 0),
        _ => None,
    };
    // A patrol ribbon holds still as points are picked: its words are
    // measured at their widest.
    let words_w = if patrol.is_some() {
        [0, aftermine_sim::stats::ORDER_QUEUE_CAP]
            .map(|points| measure(Face::Body, &crate::input::patrol_label(points), label_size))
            .into_iter()
            .fold(label_w, f32::max)
    } else {
        label_w
    };
    let start_w = measure(Face::Body, PATROL_START, label_size) + 36.0 * s;
    let patrol_w = patrol.map_or(0.0, |_| 16.0 * s + start_w + 6.0 * s + STEPPER * s);
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
    // The steppers hold still as the salvo changes: the label is measured
    // at its widest digits for the warheads ready.
    let steppers_w = salvo.map_or(0.0, |(_, ready)| {
        let widest = "8".repeat(ready.max(1).to_string().len());
        let widest = measure(
            Face::Body,
            &format!("Fire {widest} of {widest}"),
            crate::theme::Type::Label.at(s),
        );
        (widest - label_w).max(0.0) + 8.0 * s + 2.0 * STEPPER * s + 4.0 * s
    });
    let content = icon_w + (words_w + cost_w).max(status_w) + steppers_w + patrol_w;
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
    let rect = ribbon_geometry(viewport, s, panel_top, size, minimap);
    Some(RibbonLayout {
        rect,
        label,
        cost,
        building,
        label_w,
        icon_w,
        status,
        salvo: salvo.map(|(fires, ready)| {
            let side = STEPPER * s;
            let y = rect.y + (crate::theme::MIN_TOUCH_TARGET * s - side) * 0.5;
            let more = Rect::new(rect.x + rect.w - 4.0 * s - side, y, side, side);
            SalvoSteppers {
                less: Rect::new(more.x - 4.0 * s - side, y, side, side),
                more,
                less_enabled: fires > 1,
                more_enabled: fires < ready,
            }
        }),
        patrol: patrol.map(|ready| {
            let side = STEPPER * s;
            let y = rect.y + (crate::theme::MIN_TOUCH_TARGET * s - side) * 0.5;
            let cancel = Rect::new(rect.x + rect.w - 4.0 * s - side, y, side, side);
            PatrolButtons {
                start: Rect::new(cancel.x - 6.0 * s - start_w, y, start_w, side),
                cancel,
                ready,
            }
        }),
    })
}

/// The strike ribbon's label: how many warheads the next pick fires, of
/// those ready.
pub(crate) fn salvo_label(fires: usize, ready: usize) -> String {
    if ready == 0 {
        "No warhead loaded".to_string()
    } else {
        format!("Fire {fires} of {ready}")
    }
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
fn widest_placement_status(
    kind: aftermine_sim::BuildingKind,
    measure: &dyn Fn(&str) -> f32,
) -> f32 {
    use aftermine_sim::PlaceRefusal;
    let cost = kind.base_stats().construction.cost;
    // A builder holds at most the cap's worth of queue behind its order.
    let most = aftermine_sim::stats::ORDER_QUEUE_CAP + 1;
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
        PlaceRefusal::OutOfPlay,
        PlaceRefusal::Prerequisite,
        PlaceRefusal::FrameRequired,
        PlaceRefusal::FrameBlocked,
    ]
    .into_iter()
    .filter(|refusal| {
        *refusal != PlaceRefusal::FrameRequired || kind == aftermine_sim::BuildingKind::Extractor
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

fn draw_mode_ribbon(sprites: &Sprites, layout: &RibbonLayout, input: &InputState, s: f32) {
    let RibbonLayout {
        rect: ribbon,
        label,
        cost,
        building,
        label_w,
        icon_w,
        status,
        salvo,
        patrol,
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
    if let Some(steppers) = salvo {
        draw_salvo_steppers(sprites, steppers, input, s);
    }
    if let Some(buttons) = patrol {
        draw_patrol_buttons(sprites, buttons, input, s);
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

/// The patrol ribbon's Start, lit amber once there is a point to loop,
/// and its cross. In pad mode the buttons that do the same stand in for
/// the word and the glyph: the command ring's button, since picking
/// Patrol there again starts the loop, and Back, which drops the route.
fn draw_patrol_buttons(sprites: &Sprites, buttons: &PatrolButtons, input: &InputState, s: f32) {
    use crate::theme::{BORDER_FAINT, CARD_ARMED, CARD_HOVER, CARD_IDLE, EDGE_CARD, EDGE_FOCUS};
    let hover = input.chrome_hover_point();
    let (start, ready) = (buttons.start, buttons.ready);
    let start_hovered = ready && hover.is_some_and(|p| start.contains(p));
    fill_rect(start, if ready { CARD_ARMED } else { CARD_IDLE });
    let (stroke, edge) = match (ready, start_hovered) {
        (false, _) => (crate::theme::Stroke::Edge, BORDER_FAINT),
        (true, false) => (crate::theme::Stroke::Heavy, SCRAP_COLOR),
        (true, true) => (crate::theme::Stroke::Heavy, EDGE_FOCUS),
    };
    stroke_rect(start, stroke.at(s), edge);
    let pad = input.hud_hands().names();
    let start_ink = if ready {
        TEXT_ACCENT
    } else {
        crate::theme::TEXT_DISABLED
    };
    let size = crate::theme::Type::Label.at(s);
    if let Some(family) = pad {
        draw_centered_button(
            sprites,
            family,
            aftermine_protocol::PadButton::North,
            start,
            s,
            start_ink,
        );
    } else {
        let word_w = measure_text(PATROL_START, None, numeric::font_size(size), 1.0).width;
        draw_text(
            PATROL_START,
            start.x + (start.w - word_w) * 0.5,
            start.y + start.h * 0.66,
            size,
            start_ink,
        );
    }
    let cancel = buttons.cancel;
    let cancel_hovered = hover.is_some_and(|p| cancel.contains(p));
    fill_rect(
        cancel,
        if cancel_hovered {
            CARD_HOVER
        } else {
            CARD_IDLE
        },
    );
    stroke_rect(
        cancel,
        crate::theme::Stroke::Edge.at(s),
        if cancel_hovered {
            EDGE_FOCUS
        } else {
            EDGE_CARD
        },
    );
    if let Some(family) = pad {
        draw_centered_button(sprites, family, crate::pad::BACK, cancel, s, TEXT_BODY);
        return;
    }
    let (c, arm) = (cancel.center(), 7.0 * s);
    line_between(c - vec2(arm, arm), c + vec2(arm, arm), 2.5 * s, TEXT_BODY);
    line_between(c + vec2(-arm, arm), c + vec2(arm, -arm), 2.5 * s, TEXT_BODY);
}

/// A controller button's icon at body size, centered in `rect`.
fn draw_centered_button(
    sprites: &Sprites,
    family: aftermine_protocol::PadFamily,
    button: aftermine_protocol::PadButton,
    rect: Rect,
    s: f32,
    ink: Color,
) {
    let size = crate::theme::Type::Body.at(s);
    let icon = crate::hint_line::icon_width(size);
    let c = rect.center();
    let at = crate::pad::button_icon(family, button);
    crate::hint_line::draw_icon(sprites, at, c.x - icon * 0.5, c.y + size * 0.35, size, ink);
}

/// The salvo's minus and plus, faint where the size can go no further. In
/// pad mode the bumpers that step it stand in for the glyphs.
fn draw_salvo_steppers(sprites: &Sprites, steppers: &SalvoSteppers, input: &InputState, s: f32) {
    let hover = input.chrome_hover_point();
    let pad = input.hud_hands().names();
    for (rect, enabled, plus) in [
        (steppers.less, steppers.less_enabled, false),
        (steppers.more, steppers.more_enabled, true),
    ] {
        let hovered = enabled && hover.is_some_and(|p| rect.contains(p));
        fill_rect(
            rect,
            if hovered {
                crate::theme::CARD_HOVER
            } else {
                crate::theme::CARD_IDLE
            },
        );
        stroke_rect(
            rect,
            crate::theme::Stroke::Edge.at(s),
            if !enabled {
                crate::theme::BORDER_FAINT
            } else if hovered {
                crate::theme::EDGE_FOCUS
            } else {
                crate::theme::EDGE_CARD
            },
        );
        let ink = if enabled {
            TEXT_PRIMARY
        } else {
            crate::theme::TEXT_DISABLED
        };
        let (c, arm) = (rect.center(), 7.0 * s);
        if let Some(family) = pad {
            use aftermine_protocol::PadButton;
            let button = if plus {
                PadButton::RightShoulder
            } else {
                PadButton::LeftShoulder
            };
            draw_centered_button(sprites, family, button, rect, s, ink);
            continue;
        }
        line_between(c - vec2(arm, 0.0), c + vec2(arm, 0.0), 2.5 * s, ink);
        if plus {
            line_between(c - vec2(0.0, arm), c + vec2(0.0, arm), 2.5 * s, ink);
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

/// A toast's text size and lines within `width`: shrunk from the label size
/// down to body size, then wrapped rather than run past `width`.
fn toast_lines(
    text: &str,
    width: f32,
    scale: f32,
    measure: impl Fn(&str, f32) -> f32,
) -> (f32, Vec<String>) {
    let floor = crate::theme::Type::Body.at(scale);
    let mut size = crate::theme::Type::Label.at(scale);
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
    paused: bool,
    /// How the HUD stands: in pad mode the menu button stays undrawn, Menu
    /// standing in for it, and the badges and the status name the buttons
    /// of the controller named, if any.
    hands: crate::input::HudHands,
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
/// badges, and the clock with the pause and menu buttons. In pad mode
/// the badges and the status name no keys; while `hands` names a
/// controller they name its buttons, each icon taking room beside its
/// words.
pub(super) fn top_bar_layout(
    game: &crate::game::Scene<'_>,
    bindings: &crate::action::BindingMap,
    env: super::hud::HudEnv,
    measure: super::hud::Measure<'_>,
    hands: crate::input::HudHands,
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
    let passive = format!("+{}/min income", crate::panel::passive_income(game));
    let my_units = game
        .state
        .units()
        .iter()
        .filter(|unit| unit.player == game.presentation.human)
        .count();
    let scrap = scrap.to_string();
    let units = my_units.to_string();
    let keyless = hands.pad_mode() || !crate::platform::hands().keys;
    // A seat out of the match has no harvester left to order.
    let idle = if game.state.accepts_commands(game.presentation.human) {
        crate::input::idle_harvesters(game).len()
    } else {
        0
    };
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
    // A named controller's icon follows a badge's words after a space.
    let with_icon = |face, text: &str, size: f32| match hands.names() {
        Some(_) => measure(face, &format!("{text} "), size) + crate::hint_line::icon_width(size),
        None => measure(face, text, size),
    };
    let body = crate::theme::Type::Body.at(s);
    let small = crate::theme::Type::Small.at(s);
    let bar = crate::layout::top_bar(
        env.viewport.x,
        s,
        crate::platform::PLATFORM,
        crate::layout::TopBarText {
            scrap: measure(Face::Display, &scrap, crate::theme::Type::Heading.at(s)),
            passive: measure(Face::Body, &passive, body),
            units_label: measure(Face::Display, "UNITS", small),
            units: measure(Face::Display, &units, crate::theme::Type::Heading.at(s)),
            idle: idle
                .as_deref()
                .map(|text| with_icon(Face::Body, text, body)),
            alert: alert
                .as_deref()
                .map(|text| with_icon(Face::Body, text, body)),
            status: with_icon(Face::Display, &status, small),
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
        paused: game.clock.paused,
        hands,
    }
}

fn draw_top_bar(top: &TopBarLayout, sprites: &Sprites, s: f32) {
    use aftermine_protocol::PadButton;
    let TopBarLayout {
        bar,
        width,
        scrap,
        passive,
        units,
        idle,
        alert,
        status,
        paused,
        hands,
    } = top;
    // In pad mode, the button that does what a badge does follows its
    // words.
    let icon_after = |x: f32, size: f32, measured: f32, button, tint| {
        if let Some(family) = hands.names() {
            let x = x + measured + measure_text(" ", None, numeric::font_size(size), 1.0).width;
            crate::hint_line::draw_icon(
                sprites,
                crate::pad::button_icon(family, button),
                x,
                26.0 * s,
                size,
                tint,
            );
        }
    };
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
    let body = crate::theme::Type::Body.at(s);
    let body_width = |text: &str| measure_text(text, None, numeric::font_size(body), 1.0).width;
    if let Some(text) = idle {
        fill_rect(bar.idle_badge, crate::theme::BADGE);
        let x = bar.idle_badge.x + 9.0 * s;
        draw_text(text, x, 26.0 * s, body, SCRAP_COLOR);
        icon_after(x, body, body_width(text), PadButton::DpadDown, SCRAP_COLOR);
    }
    if let Some(text) = alert {
        fill_rect(bar.alert_badge, crate::theme::BADGE);
        let x = bar.alert_badge.x + 9.0 * s;
        draw_text(text, x, 26.0 * s, body, crate::theme::TEXT_DANGER);
        let tint = crate::theme::TEXT_DANGER;
        icon_after(x, body, body_width(text), PadButton::DpadLeft, tint);
    }
    // An Apple TV lays out no menu button, and its HUD can draw a frame
    // before pad mode settles, so the bars mustn't hang off an empty rect.
    if !hands.pad_mode() && bar.menu_button.w > 0.0 {
        draw_menu_button(bar.menu_button, s);
    }
    draw_pause_button(bar.pause_button, *paused, s);
    let small = crate::theme::Type::Small.at(s);
    crate::typography::draw(status, bar.status_x, 26.0 * s, small, TEXT_PRIMARY);
    // View pauses and resumes, so it follows the clock as well as PAUSED.
    let measured = crate::typography::measure(status, small).width;
    icon_after(
        bar.status_x,
        small,
        measured,
        PadButton::Select,
        TEXT_PRIMARY,
    );
}

/// Draws the HUD the layout pass placed: the top bar, the queue flag, the
/// command panel, the control groups, the armed-mode ribbon, the QUEUE
/// toggle, the performance readout, toasts, and the spectator strip.
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
        draw_top_bar(top_bar, sprites, s);
    }
    if let Some(flag) = &hud.queue_flag {
        draw_queue_flag(flag, s);
    }
    if let (Some(panel), Some(layout)) = (
        game.presentation.panel_model.borrow().as_ref(),
        hud.panel.as_ref(),
    ) {
        draw_panel(sprites, input, bindings, panel, layout);
    }
    if let Some(column) = &hud.group_column {
        draw_group_column(sprites, column, s);
    }
    if let Some(strip) = &hud.army_strip {
        draw_army_strip(strip, input, s);
    }
    if let Some(ribbon) = &hud.ribbon {
        draw_mode_ribbon(sprites, ribbon, input, s);
    }
    if let Some(rect) = hud.queue_toggle {
        draw_queue_toggle(rect, input.queue_toggle, s);
    }
    if let (Some(view), Some(layout)) = (performance, &hud.performance) {
        super::performance::draw(view, layout);
    }
    // Toasts keep their places whatever pad mode leaves out.
    let layout = hud.laid_out;
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
                    .max(queue_toggle.x + queue_toggle.w)
                    .max(layout.drones.right()),
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
        &[layout.minimap, layout.army_strip, layout.group_column],
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
            ..match toast.tone {
                crate::game::ToastTone::Notice => crate::theme::TEXT_PRIMARY,
                crate::game::ToastTone::Danger => crate::theme::TEXT_ALERT,
            }
        };
        let plate = Color {
            a: crate::theme::SURFACE_CAPTION.a * fade,
            ..crate::theme::SURFACE_CAPTION
        };
        for (index, line) in lines.iter().enumerate() {
            let at = origin(lines_below + lines.len() - 1 - index);
            let width = measure_text(line, None, numeric::font_size(size), 1.0).width;
            draw_rectangle(
                at.x - 6.0 * s,
                at.y - size,
                width + 12.0 * s,
                size + 6.0 * s,
                plate,
            );
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
        draw_rectangle(
            x - 12.0 * s,
            top,
            dims.width + 24.0 * s,
            NET_NOTICE_H * s,
            PANEL,
        );
        draw_text(text, x, top + 20.0 * s, size, crate::theme::TEXT_ALERT);
    }
}

/// A team-game concession can leave the match undecided while the ally
/// keeps fighting. This compact exit offer is the only result layer
/// gameplay draws; a decided match moves to the Results screen.
pub(crate) fn draw_result_overlay(game: &crate::game::Scene<'_>, sprites: &Sprites) {
    if !game.presentation.conceded_banner || game.state.result().is_some() {
        return;
    }
    let s = ui_scale();
    let text = "SURRENDERED";
    let width = crate::menu::title_width(text);
    let x = (screen_width() - width) * 0.5;
    let y = screen_height() * 0.38;
    draw_rectangle(
        x - 24.0 * s,
        y - 48.0 * s,
        width + 48.0 * s,
        112.0 * s,
        PANEL,
    );
    crate::menu::draw_title(text, y, DANGER);
    let team_fights_on = crate::game::team_fights_on(game.state, game.presentation.human);
    crate::menu::hint_line(&concede_hint(crate::platform::hands(), team_fights_on)).draw(
        sprites,
        screen_width() * 0.5,
        y + 28.0 * s,
        screen_width(),
        (crate::theme::Type::Label, crate::theme::Type::Label),
        TEXT_BODY,
        WHITE,
    );
}

/// What the controller notice says under its title: whether the match
/// waits for a controller or keeps running without one.
pub(crate) fn controller_notice_text(lan: bool) -> &'static str {
    if lan {
        "The match keeps running for the other players."
    } else {
        "The match waits until a controller connects."
    }
}

/// The notice over a match no controller can play: it asks for one, says
/// whether the match waits, and names the remote's Back, its one press
/// in a match.
pub(crate) fn draw_controller_notice(sprites: &Sprites, lan: bool) {
    use crate::hint_line::HintLine;
    use crate::theme::Type;
    let s = ui_scale();
    let title = "CONNECT A CONTROLLER";
    let body = HintLine::default().text(controller_notice_text(lan));
    let back = HintLine::default()
        .button(aftermine_protocol::PadFamily::Remote, crate::pad::BACK)
        .text(" for the menu");
    let size = Type::Label.at(s);
    let width = crate::menu::title_width(title)
        .max(body.width(size))
        .max(back.width(size));
    let center = screen_width() * 0.5;
    let y = screen_height() * 0.38;
    draw_rectangle(
        center - width * 0.5 - 24.0 * s,
        y - 48.0 * s,
        width + 48.0 * s,
        140.0 * s,
        PANEL,
    );
    crate::menu::draw_title(title, y, TEXT_PRIMARY);
    for (line, baseline) in [(body, y + 30.0 * s), (back, y + 64.0 * s)] {
        line.draw(
            sprites,
            center,
            baseline,
            screen_width(),
            (Type::Label, Type::Label),
            TEXT_BODY,
            WHITE,
        );
    }
}

#[cfg(test)]
mod tests;
