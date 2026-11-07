//! Draws the command ring over the match: its slices around a hub, each
//! wearing its card's face, cost, queued count and progress; the ring's
//! heading and the pointed slice's name in the hub; that card's own
//! tooltip beside the ring; and the buttons that drive it underneath.

use super::prim::{fill_circle, stroke_arc, stroke_circle};
use super::*;
use crate::radial::{Geometry, Pick, Ring, Slice};
use crate::theme::{Stroke, Type};

/// How long a ring takes to scale in after it opens, in seconds.
const OPEN_SECS: f64 = 0.1;
/// How much larger the pointed slice draws.
const POINTED_GROWTH: f32 = 1.12;

/// An open ring as one frame lays it out.
pub(crate) struct RingLayout {
    ring: Ring,
    geometry: Geometry,
    pointed: Option<usize>,
    /// How far its scale-in has run, from 0 to 1.
    opened: f32,
}

/// Lays the open ring out from the panel this frame built, between the
/// top bar and the command panel. None while no ring is open.
pub(crate) fn layout(
    game: &crate::game::Scene<'_>,
    input: &InputState,
    panel: Option<&crate::panel::Panel>,
    env: hud::HudEnv,
    bottom: f32,
) -> Option<RingLayout> {
    let open = input.ring.as_ref()?;
    let ring = crate::input::pad_ring::shown(game, panel?, open)?;
    let geometry = Geometry::fit(
        input.reticle.point(),
        env.viewport,
        env.ui,
        crate::layout::TOP_BAR_H * env.ui,
        bottom,
    );
    let pointed = open.pointed(&ring);
    let opened = numeric::to_f32(((input.now - open.opened_at()) / OPEN_SECS).clamp(0.0, 1.0));
    Some(RingLayout {
        ring,
        geometry,
        pointed,
        opened,
    })
}

/// Draws the ring laid out this frame.
pub(crate) fn draw(sprites: &Sprites, shown: &RingLayout) {
    let ease = 1.0 - (1.0 - shown.opened).powi(3);
    let geometry = Geometry {
        center: shown.geometry.center,
        scale: shown.geometry.scale * (0.92 + 0.08 * ease),
    };
    let k = geometry.scale;
    let fade = |color: Color| Color {
        a: color.a * ease,
        ..color
    };
    fill_circle(
        geometry.center,
        geometry.reach(),
        fade(Color::new(0.05, 0.05, 0.07, 0.62)),
    );
    stroke_circle(
        geometry.center,
        geometry.reach(),
        Stroke::Hairline.at(k),
        fade(crate::theme::EDGE_CARD),
    );
    // The hub's own plate keeps its words legible over whatever stands
    // under the ring.
    fill_circle(
        geometry.center,
        geometry.radius() - geometry.slice() - 8.0 * k,
        fade(Color::new(0.03, 0.03, 0.04, 0.82)),
    );
    let count = shown.ring.slices.len();
    for (index, slice) in shown.ring.slices.iter().enumerate() {
        draw_slice(
            sprites,
            &geometry,
            (index, count),
            slice,
            shown.pointed == Some(index),
            shown.ring.owner,
            &fade,
        );
    }
    let pointed = shown.pointed.and_then(|index| shown.ring.slices.get(index));
    draw_hub(&geometry, &shown.ring.heading, pointed, &fade);
    if ease >= 1.0
        && let Some(slice) = pointed
    {
        let upgrade = (slice.card.action == crate::panel::CardAction::Upgrade)
            .then_some(shown.ring.upgrade.as_ref())
            .flatten();
        draw_tooltip(&geometry, &slice.card, upgrade);
    }
    legend(&shown.ring, pointed).draw(
        sprites,
        geometry.center.x,
        geometry.legend_baseline(),
        geometry.reach() * 2.0 + 120.0 * k,
        (Type::Body, Type::Caption),
        fade(TEXT_SECONDARY),
        fade(TEXT_BODY),
    );
}

/// One slice: its disc wearing its card's face and cost, the progress of
/// what it builds swept around it, its queued count, and a notch on the
/// rim when it opens a ring of its own.
fn draw_slice(
    sprites: &Sprites,
    geometry: &Geometry,
    (index, count): (usize, usize),
    slice: &Slice,
    pointed: bool,
    owner: Color,
    fade: &impl Fn(Color) -> Color,
) {
    let k = geometry.scale;
    let card = &slice.card;
    let center = geometry.slice_center(index, count);
    let direction = crate::radial::slice_direction(index, count);
    let radius = geometry.slice() * if pointed { POINTED_GROWTH } else { 1.0 };
    let (fill, edge, weight) = if pointed {
        (
            crate::theme::CARD_HOVER,
            crate::theme::EDGE_FOCUS,
            Stroke::Focus,
        )
    } else if card.enabled {
        (
            crate::theme::CARD_IDLE,
            crate::theme::EDGE_CARD,
            Stroke::Edge,
        )
    } else {
        (
            crate::theme::CARD_IDLE,
            crate::theme::BORDER_FAINT,
            Stroke::Edge,
        )
    };
    fill_circle(center, radius, fade(fill));
    stroke_circle(center, radius, weight.at(k), fade(edge));
    if let Some(progress) = card.progress {
        stroke_arc(
            center,
            radius + 3.0 * k,
            3.0 * k,
            progress.clamp(0.0, 1.0) * 360.0,
            fade(SCRAP_COLOR),
        );
    }
    if matches!(slice.pick, Pick::Open(_)) {
        // A notch on the rim, pointing outward: this slice opens a ring.
        let across = vec2(-direction.y, direction.x) * 7.0 * k;
        let base = center + direction * (radius + 3.0 * k);
        let tip = center + direction * (radius + 12.0 * k);
        draw_triangle(base + across, base - across, tip, fade(TEXT_BODY));
    }
    let tint = if card.enabled {
        WHITE
    } else {
        Color::new(1.0, 1.0, 1.0, 0.35)
    };
    let side = radius * 1.15;
    let lift = if card.cost.is_some() {
        radius * 0.14
    } else {
        0.0
    };
    panel_draw::draw_card_icon(
        sprites,
        Rect::new(
            center.x - side * 0.5,
            center.y - side * 0.5 - lift,
            side,
            side,
        ),
        &card.icon,
        fade(tint),
        owner,
    );
    if let Some(cost) = card.cost {
        let text = cost.to_string();
        let size = Type::Caption.at(k);
        let width = measure_text(&text, None, numeric::font_size(size), 1.0).width;
        draw_text(
            &text,
            center.x - width * 0.5,
            center.y + radius * 0.78,
            size,
            fade(if card.enabled {
                SCRAP_COLOR
            } else {
                TEXT_DISABLED
            }),
        );
    }
    if slice.count > 0 {
        let text = slice.count.to_string();
        let size = Type::Small.at(k);
        let width = measure_text(&text, None, numeric::font_size(size), 1.0).width;
        let badge = (size * 0.75).max(width * 0.5 + 4.0 * k);
        let at = center + vec2(radius * 0.72, -radius * 0.72);
        fill_circle(at, badge, fade(SCRAP_COLOR));
        draw_text(
            &text,
            at.x - width * 0.5,
            at.y + size * 0.35,
            size,
            fade(crate::theme::SURFACE_CARD),
        );
    }
}

/// The hub: the ring's heading over the pointed slice's name and cost.
fn draw_hub(
    geometry: &Geometry,
    heading: &str,
    pointed: Option<&Slice>,
    fade: &impl Fn(Color) -> Color,
) {
    let k = geometry.scale;
    let room = (geometry.radius() - geometry.slice()) * 2.0 - 16.0 * k;
    let centered = |text: &str, size: f32, baseline: f32, color: Color| {
        let width = measure_text(text, None, numeric::font_size(size), 1.0).width;
        draw_text(
            text,
            geometry.center.x - width * 0.5,
            baseline,
            size,
            fade(color),
        );
    };
    let caption = crate::typography::fit(heading, Type::Caption.at(k), room, Type::Caption);
    centered(
        heading,
        caption,
        geometry.center.y - 18.0 * k,
        TEXT_SECONDARY,
    );
    let Some(card) = pointed.map(|slice| &slice.card) else {
        return;
    };
    let size = crate::typography::fit(&card.title, Type::Body.at(k), room, Type::Caption);
    centered(
        &card.title,
        size,
        geometry.center.y + 6.0 * k,
        if card.enabled {
            TEXT_PRIMARY
        } else {
            TEXT_DISABLED
        },
    );
    if let Some(cost) = card.cost {
        centered(
            &format!("{cost} scrap"),
            Type::Caption.at(k),
            geometry.center.y + 24.0 * k,
            if card.enabled {
                SCRAP_COLOR
            } else {
                TEXT_DISABLED
            },
        );
    }
}

/// The pointed card's own tooltip, beside the ring on whichever side has
/// room.
fn draw_tooltip(
    geometry: &Geometry,
    card: &crate::panel::Card,
    upgrade: Option<&crate::panel::UpgradeComparison>,
) {
    let k = geometry.scale;
    let gap = 8.0 * k;
    let viewport = vec2(screen_width(), screen_height());
    let reach = geometry.reach();
    panel_draw::draw_card_tooltip(card, upgrade, 300.0 * k, |size| {
        let right = geometry.center.x + reach + gap;
        let x = if right + size.x <= viewport.x - gap {
            right
        } else {
            geometry.center.x - reach - gap - size.x
        };
        vec2(
            x.clamp(gap, (viewport.x - size.x - gap).max(gap)),
            (geometry.center.y - size.y * 0.5).clamp(gap, (viewport.y - size.y - gap).max(gap)),
        )
    });
}

/// The buttons that drive the ring, as the controller in use names them:
/// X only while the pointed slice has one to cancel, and Y beside B only
/// once B no longer closes the ring itself.
fn legend(ring: &Ring, pointed: Option<&Slice>) -> crate::hint_line::HintLine {
    use aftermine_protocol::PadButton;
    let family = crate::platform::hands()
        .pad
        .unwrap_or(aftermine_protocol::PadFamily::Xbox);
    let icon = |button| crate::pad::button_icon(family, button);
    let mut line = crate::hint_line::HintLine::default()
        .pad(icon(PadButton::South))
        .text(" pick - ");
    if pointed.is_some_and(|slice| slice.cancel.is_some()) {
        line = line.pad(icon(PadButton::West)).text(" cancel one - ");
    }
    if ring.path.is_empty() {
        line.pad(icon(PadButton::East)).text(" close")
    } else {
        line.pad(icon(PadButton::East))
            .text(" back - ")
            .pad(icon(PadButton::North))
            .text(" close")
    }
}
