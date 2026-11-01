//! Quarry boundary vocabulary: the terrace palette, decals, and edge
//! geometry the boundary, peak, and pit passes share.

use crate::game::Scene;
use crate::numeric::Fit;
use crate::render::prim::line_between;
use chassis::grid::TilePos;
use macroquad::prelude::*;

/// Salt for the boundary's bench jogs and decals.
pub(super) const SALT: u32 = 347;
pub(super) const WAVE_LIFTS: [i16; 6] = [-3, -1, 0, 1, 1, 3];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "one flag per map edge")]
pub(super) struct BoundaryInsets {
    pub(super) top: bool,
    pub(super) right: bool,
    pub(super) bottom: bool,
    pub(super) left: bool,
}

impl BoundaryInsets {
    pub(super) fn from_rows(rows: &[String]) -> Self {
        let width = rows.first().map_or(0, String::len);
        let blocked = |cell: u8| matches!(cell, b'#' | b'^');
        Self {
            top: rows
                .first()
                .is_some_and(|row| !row.is_empty() && row.bytes().all(blocked)),
            right: width > 0
                && rows
                    .iter()
                    .all(|row| row.as_bytes().get(width - 1).copied().is_some_and(blocked)),
            bottom: rows
                .last()
                .is_some_and(|row| !row.is_empty() && row.bytes().all(blocked)),
            left: width > 0
                && rows
                    .iter()
                    .all(|row| row.as_bytes().first().copied().is_some_and(blocked)),
        }
    }
}

/// Whether a tile sits in a fully blocked edge row or column, which the
/// boundary terraces draw over instead of the tile pass.
pub(super) fn boundary_ring(rows: &[String]) -> impl Fn(TilePos) -> bool + use<> {
    let insets = BoundaryInsets::from_rows(rows);
    let width = rows.first().map_or(0, String::len).fit::<i32>();
    let height = rows.len().fit::<i32>();
    move |pos: TilePos| {
        (insets.top && pos.y == 0)
            || (insets.bottom && pos.y == height - 1)
            || (insets.left && pos.x == 0)
            || (insets.right && pos.x == width - 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Material {
    Riser(usize),
    Bench(usize),
    Vignette(u8),
    Void,
}

#[derive(Clone, Copy)]
pub(super) struct Layer {
    pub(super) bench_cells: i32,
    pub(super) riser: Color,
    pub(super) top: Color,
    pub(super) lip: Color,
}

pub(super) const fn rgba(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r, g, b, 255)
}

/// The boundary's rising terraces, nearest the floor first; peaks share
/// them. Each level lifts its bench and lip by about twelve tones so height
/// reads in color as well as in the risers.
pub(super) const LAYERS: [Layer; 3] = [
    Layer {
        bench_cells: 4,
        riser: rgba(24, 25, 30),
        top: rgba(52, 52, 56),
        lip: rgba(69, 67, 73),
    },
    Layer {
        bench_cells: 5,
        riser: rgba(28, 28, 34),
        top: rgba(64, 63, 68),
        lip: rgba(84, 80, 86),
    },
    Layer {
        bench_cells: 3,
        riser: rgba(32, 31, 37),
        top: rgba(76, 74, 80),
        lip: rgba(99, 94, 100),
    },
];

pub(super) fn shifted(color: Color, amount: i16) -> Color {
    let amount = f32::from(amount) / 255.0;
    Color::new(
        (color.r + amount).clamp(0.0, 1.0),
        (color.g + amount).clamp(0.0, 1.0),
        (color.b + amount).clamp(0.0, 1.0),
        color.a,
    )
}

pub(super) fn mixed(left: Color, right: Color, amount: f32) -> Color {
    Color::new(
        left.r + (right.r - left.r) * amount,
        left.g + (right.g - left.g) * amount,
        left.b + (right.b - left.b) * amount,
        left.a + (right.a - left.a) * amount,
    )
}

pub(super) fn quarry_color(color: Color, material: Material, fractured: bool) -> Color {
    if fractured && matches!(material, Material::Riser(_) | Material::Bench(_)) {
        Color::new(color.r + 0.008, color.g + 0.004, color.b, color.a)
    } else {
        color
    }
}

pub(super) fn draw_strata(rect: Rect, edge: (i32, i32), layer: Layer, token: u32) {
    let (origin, tangent, inward) = match edge {
        (-1, 0) => (vec2(rect.x, rect.y), vec2(0.0, rect.h), vec2(rect.w, 0.0)),
        (1, 0) => (
            vec2(rect.x + rect.w, rect.y),
            vec2(0.0, rect.h),
            vec2(-rect.w, 0.0),
        ),
        (0, -1) => (vec2(rect.x, rect.y), vec2(rect.w, 0.0), vec2(0.0, rect.h)),
        _ => (
            vec2(rect.x, rect.y + rect.h),
            vec2(rect.w, 0.0),
            vec2(0.0, -rect.h),
        ),
    };
    if token.is_multiple_of(3) {
        let start = origin + inward * 0.42;
        let middle = start + tangent * 0.46 + inward * 0.08;
        let end = origin + tangent * 0.94 + inward * 0.43;
        let tone = mixed(layer.riser, layer.lip, 0.30);
        let width = (rect.w * 0.08).max(1.0);
        line_between(start, middle, width, tone);
        line_between(middle, end, width, tone);
    }
    if token.is_multiple_of(7) {
        let start = origin + tangent * 0.32;
        let tip = start + tangent * 0.23 + inward * 0.38;
        draw_triangle(start, start + tangent * 0.22, tip, layer.riser);
    }
}

pub(super) fn draw_lip(rect: Rect, direction: (i32, i32), thickness: f32, color: Color) {
    match direction {
        (-1, 0) => draw_rectangle(rect.x, rect.y, thickness, rect.h, color),
        (1, 0) => draw_rectangle(
            rect.x + rect.w - thickness,
            rect.y,
            thickness,
            rect.h,
            color,
        ),
        (0, -1) => draw_rectangle(rect.x, rect.y, rect.w, thickness, color),
        (0, 1) => draw_rectangle(
            rect.x,
            rect.y + rect.h - thickness,
            rect.w,
            thickness,
            color,
        ),
        _ => unreachable!("terrace lips are cardinal"),
    }
}

pub(super) fn draw_missing_slab(rect: Rect, cell: f32, top: Color, value: u32) {
    let (x, y) = if value & 1 == 0 {
        (0.18, 0.42)
    } else {
        (0.52, 0.22)
    };
    let void = rgba(17, 18, 22);
    draw_rectangle(
        rect.x + cell * x,
        rect.y + cell * y,
        cell * 1.2,
        cell * 0.68,
        void,
    );
    draw_rectangle(
        rect.x + cell * (x + if value & 1 == 0 { 0.22 } else { -0.14 }),
        rect.y + cell * (y + 0.68),
        cell * 0.82,
        cell * 0.24,
        void,
    );
    draw_rectangle(
        rect.x + cell * (x + 0.18),
        rect.y + cell * (y + 0.94),
        cell * 0.66,
        cell * 0.12,
        shifted(top, -14),
    );
}

pub(super) fn draw_fracture(rect: Rect, cell: f32, tile: f32, top: Color, value: u32) {
    let points = if value & 1 == 0 {
        [(0.12, 0.72), (0.7, 0.56), (1.02, 1.1), (1.84, 1.38)]
    } else {
        [(0.42, 0.12), (0.58, 0.74), (1.34, 0.96), (1.62, 1.82)]
    };
    for pair in points.windows(2) {
        draw_line(
            rect.x + cell * pair[0].0,
            rect.y + cell * pair[0].1,
            rect.x + cell * pair[1].0,
            rect.y + cell * pair[1].1,
            (tile * 0.035).max(1.0),
            shifted(top, -22),
        );
    }
}

pub(super) fn draw_rebar(rect: Rect, cell: f32, tile: f32) {
    for offset in [0.0, 0.22] {
        for (width, color) in [
            ((tile * 0.04).max(1.0) + 1.0, rgba(48, 38, 39)),
            ((tile * 0.04).max(1.0), rgba(116, 67, 52)),
        ] {
            draw_line(
                rect.x + cell * (0.52 + offset),
                rect.y + cell * 0.36,
                rect.x + cell * (1.42 + offset),
                rect.y + cell * 1.54,
                width,
                color,
            );
        }
    }
}

pub(super) fn hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut value = 2_166_136_261u32;
    for word in [x.cast_unsigned(), y.cast_unsigned(), salt] {
        for byte in word.to_le_bytes() {
            value ^= u32::from(byte);
            value = value.wrapping_mul(16_777_619);
        }
    }
    value
}

pub(super) fn draw_backdrop(_game: &Scene<'_>) {
    // Boundary sight extends off-map, so the void must match opaque fog.
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        super::FOG_UNEXPLORED,
    );
}

#[cfg(test)]
mod tests;
