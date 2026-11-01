//! The report's Built tab: what each seat made, drawn in its own colors.

use super::draw_marker;
use crate::assets::Sprites;
use crate::game::Game;
use crate::render::{building_portrait, unit_portrait};
use crate::{numeric, render, theme};
use aftermine_kit::stats::{MatchStats, PlayerStats};
use aftermine_sim::{BuildingKind, PlayerId, UnitKind};
use macroquad::prelude::*;

/// One thing a seat built, and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltEntry {
    Unit(UnitKind, u32),
    Structure(BuildingKind, u32),
}

/// What a seat built: units, then structures, each by count and then in a
/// stable kind order.
pub(super) fn entries(player: &PlayerStats) -> Vec<BuiltEntry> {
    let mut units: Vec<(UnitKind, u32)> = player
        .units_built
        .iter()
        .map(|(kind, count)| (*kind, *count))
        .collect();
    units.sort_by_key(|(kind, count)| (std::cmp::Reverse(*count), *kind));
    let mut structures: Vec<(BuildingKind, u32)> = player
        .structures_built
        .iter()
        .map(|(kind, count)| (*kind, *count))
        .collect();
    structures.sort_by_key(|(kind, count)| (std::cmp::Reverse(*count), *kind));
    units
        .into_iter()
        .map(|(kind, count)| BuiltEntry::Unit(kind, count))
        .chain(
            structures
                .into_iter()
                .map(|(kind, count)| BuiltEntry::Structure(kind, count)),
        )
        .collect()
}

/// The shortest row a sprite and its count stay legible on, in logical px.
const SHORTEST_ROW: f32 = 18.0;

/// The largest a sprite draws here, in logical px.
const LARGEST_ICON: f32 = 44.0;

/// The space between a seat's units and its structures, in logical px.
const GROUP_GAP: f32 = 18.0;

/// How one seat's row is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RowShape {
    pub(super) icon: f32,
    pub(super) lines: usize,
    pub(super) height: f32,
}

/// Fits `rows` seats in `height`: a roomy report wraps each seat onto two
/// lines and stacks from the top, and a tight one shrinks to a single line.
pub(super) fn row_shape(rows: usize, height: f32, s: f32) -> Option<RowShape> {
    let room = height / rows.max(1) as f32;
    if room < SHORTEST_ROW * s {
        return None;
    }
    let pad = 8.0 * s;
    let full = LARGEST_ICON * s;
    if room >= 2.0 * full + pad {
        return Some(RowShape {
            icon: full,
            lines: 2,
            height: 2.0 * full + pad,
        });
    }
    let icon = (room - pad.min(room * 0.2)).min(full);
    Some(RowShape {
        icon,
        lines: 1,
        height: room.min(full + pad),
    })
}

/// Draws one row per seat. What doesn't fit is what the seat built least of.
pub(super) fn draw(game: &Game, sprites: &Sprites, report: &MatchStats, body: Rect, s: f32) {
    let size = theme::Type::Body.at(s);
    let Some(shape) = row_shape(report.players.len(), body.h, s) else {
        draw_text(
            "Make the window taller to see this",
            body.x + 8.0 * s,
            body.y + 18.0 * s,
            theme::Type::Small.at(s),
            theme::TEXT_SECONDARY,
        );
        return;
    };
    let names_right = body.x + (body.w * 0.18).min(170.0 * s);
    let line_h = shape.height / shape.lines as f32;
    for (row, player) in report.players.iter().enumerate() {
        let top = body.y + row as f32 * shape.height;
        let first_middle = top + line_h * 0.5;
        let owner = render::seat_identity_color(&game.view(), PlayerId(player.seat));
        draw_marker(
            body.x + 6.0 * s,
            first_middle,
            3.6 * s,
            usize::from(player.seat),
            owner,
        );
        let name = &game.state.players()[usize::from(player.seat)].name;
        draw_text(
            crate::typography::elide_to_width(name, names_right - body.x - 24.0 * s, &|text| {
                measure_text(text, None, numeric::font_size(size), 1.0).width
            }),
            body.x + 16.0 * s,
            first_middle + size * 0.35,
            size,
            theme::TEXT_BODY,
        );
        let mut x = names_right;
        let mut line = 0;
        let mut in_units = true;
        for entry in entries(player) {
            let (layers, count, unit) = match entry {
                BuiltEntry::Unit(kind, count) => {
                    (unit_portrait(sprites, kind, WHITE, owner), count, true)
                }
                BuiltEntry::Structure(kind, count) => (
                    building_portrait(sprites, kind, 0, true, WHITE, owner),
                    count,
                    false,
                ),
            };
            if in_units && !unit {
                in_units = false;
                x += GROUP_GAP * s;
            }
            let label = count.to_string();
            let width = measure_text(&label, None, numeric::font_size(size), 1.0).width;
            if x + shape.icon + width + 6.0 * s > body.x + body.w {
                line += 1;
                x = names_right;
                if line >= shape.lines {
                    break;
                }
            }
            let middle = first_middle + line as f32 * line_h;
            sprites.draw_portrait(
                Rect::new(x, middle - shape.icon * 0.5, shape.icon, shape.icon),
                &layers,
            );
            draw_text(
                &label,
                x + shape.icon + 2.0 * s,
                middle + size * 0.35,
                size,
                theme::TEXT_PRIMARY,
            );
            x += shape.icon + width + 12.0 * s;
        }
    }
}

#[cfg(test)]
mod tests;
