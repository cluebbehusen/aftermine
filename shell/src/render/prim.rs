//! macroquad's shape calls take loose scalars, so every call site explodes a
//! `Rect` or `Vec2` back into its fields. These take the value whole.

use macroquad::prelude::{
    Color, Rect, Vec2, draw_arc, draw_circle, draw_circle_lines, draw_line, draw_rectangle,
    screen_dpi_scale,
};

/// A `size` rectangle centered on `center`, snapped to a `pixel` grid and
/// never thinner than one pixel.
pub(crate) fn pixel_rect(center: Vec2, size: Vec2, pixel: f32, color: Color) {
    let origin = (center - size * 0.5) / pixel;
    draw_rectangle(
        origin.x.round() * pixel,
        origin.y.round() * pixel,
        (size.x / pixel).round().max(1.0) * pixel,
        (size.y / pixel).round().max(1.0) * pixel,
        color,
    );
}

pub(crate) fn fill_rect(rect: Rect, color: Color) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, color);
}

/// An outline drawn inside `rect`, `thickness / 2` wide as macroquad's own
/// is. macroquad's falls between device pixels at fractional positions, and
/// an edge thinner than a pixel can then cover no pixel centre and vanish:
/// a top or left edge most often.
pub(crate) fn stroke_rect(rect: Rect, thickness: f32, color: Color) {
    for band in outline_bands(rect, thickness, screen_dpi_scale())
        .into_iter()
        .flatten()
    {
        draw_rectangle(band.x, band.y, band.w, band.h, color);
    }
}

/// The fills that outline `rect`: each edge on whole device pixels and at
/// least one wide, or the whole rect where the edges would meet.
pub(crate) fn outline_bands(rect: Rect, thickness: f32, dpi: f32) -> [Option<Rect>; 4] {
    let snap = |v: f32| (v * dpi).round() / dpi;
    let (left, top) = (snap(rect.x), snap(rect.y));
    let (right, bottom) = (snap(rect.x + rect.w), snap(rect.y + rect.h));
    let edge = (thickness * 0.5 * dpi).round().max(1.0) / dpi;
    let (w, h) = (right - left, bottom - top);
    if w <= 2.0 * edge || h <= 2.0 * edge {
        return [Some(Rect::new(left, top, w, h)), None, None, None];
    }
    [
        Some(Rect::new(left, top, w, edge)),
        Some(Rect::new(left, bottom - edge, w, edge)),
        Some(Rect::new(left, top + edge, edge, h - 2.0 * edge)),
        Some(Rect::new(right - edge, top + edge, edge, h - 2.0 * edge)),
    ]
}

pub(crate) fn line_between(from: Vec2, to: Vec2, thickness: f32, color: Color) {
    draw_line(from.x, from.y, to.x, to.y, thickness, color);
}

pub(crate) fn fill_circle(center: Vec2, radius: f32, color: Color) {
    draw_circle(center.x, center.y, radius, color);
}

pub(crate) fn stroke_circle(center: Vec2, radius: f32, thickness: f32, color: Color) {
    draw_circle_lines(center.x, center.y, radius, thickness, color);
}

/// A clockwise arc starting at twelve o'clock and sweeping `degrees`.
pub(crate) fn stroke_arc(center: Vec2, radius: f32, thickness: f32, degrees: f32, color: Color) {
    draw_arc(
        center.x, center.y, 48, radius, -90.0, thickness, degrees, color,
    );
}

#[cfg(test)]
mod tests;
