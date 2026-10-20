//! Continuous exposed belt runs inside the existing track casings.

use crate::numeric::Fit;
use macroquad::models::Vertex;
use macroquad::prelude::*;
use oxide_sim::UnitKind;

pub(crate) fn supported(kind: UnitKind) -> bool {
    !runs(kind).is_empty()
}

fn runs(kind: UnitKind) -> &'static [[f32; 4]] {
    crate::look::unit(kind).belts
}

pub(crate) fn gauge(kind: UnitKind, scale: f32) -> f32 {
    let belts = runs(kind);
    if belts.len() < 2 {
        return 0.0;
    }
    (belts[1][0] + belts[1][2] - belts[0][0] - belts[0][2]) * 0.5 / 128.0 * scale
}

pub(super) fn draw(
    kind: UnitKind,
    center: Vec2,
    size: f32,
    rotation: f32,
    travel: [f32; 2],
    scale: f32,
) {
    let factor = size / 128.0;
    let (sin, cos) = rotation.sin_cos();
    let point = |x: f32, y: f32| {
        let x = (x - 64.0) * factor;
        let y = (y - 64.0) * factor;
        center + vec2(x * cos - y * sin, x * sin + y * cos)
    };
    // Every link goes into one mesh: a draw call per triangle cost more
    // than the rest of the unit together.
    let mut mesh = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        texture: None,
    };
    let mut rect = |x0: f32, y0: f32, x1: f32, y1: f32, color: Color| {
        if y1 <= y0 {
            return;
        }
        let first = mesh.vertices.len().fit::<u16>();
        for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
            let p = point(x, y);
            mesh.vertices
                .push(Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, color));
        }
        mesh.indices
            .extend([0, 1, 2, 0, 2, 3].map(|corner| first + corner));
    };
    // Links keep one size against the visible body, however much of the
    // canvas that body spans.
    let link = crate::look::unit(kind).extent;
    for &[x0, y0, x1, y1] in runs(kind) {
        let side = usize::from(x0 > 64.0);
        let pitch = 13.0 * link;
        // The visible upper run travels toward the nose; the hidden contact
        // run travels backward. One world unit of belt travel stays one unit.
        let offset = (-travel[side] * 128.0 / scale).rem_euclid(pitch);
        rect(x0, y0, x1, y1, color_u8!(23, 24, 29, 255));
        let mut y = y0 - pitch + offset;
        while y < y1 {
            let top = y.max(y0);
            let bottom = (y + 6.0 * link).min(y1);
            rect(x0, top, x1, bottom, color_u8!(67, 68, 76, 255));
            rect(
                x0 + link,
                top,
                x1 - link,
                (y + 1.6 * link).min(y1),
                color_u8!(107, 105, 107, 255),
            );
            rect(
                x0 + 2.0 * link,
                (y + 4.5 * link).max(y0),
                x1 - 2.0 * link,
                bottom,
                color_u8!(44, 45, 52, 255),
            );
            y += pitch;
        }
    }
    draw_mesh(&mesh);
}

#[cfg(test)]
mod tests;
