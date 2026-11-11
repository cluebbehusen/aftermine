//! Marker shapes gathered into one reused mesh. Each shape queues exactly the
//! vertices and triangles macroquad's own call for it would, reading its
//! trigonometry from tables computed by macroquad's own expressions, so the
//! picture stays the same while the per-shape allocation and trigonometry go.

use crate::render::prim::outline_bands;
use macroquad::prelude::*;

/// Most a mesh holds before it is drawn: well inside one macroquad draw
/// call, whose `geometry` clamps anything larger.
const MESH_VERTICES: usize = 4096;
const MESH_INDICES: usize = 6144;

/// The sides macroquad gives a filled and an outlined circle.
const CIRCLE_SIDES: u8 = 20;
const RING_SIDES: u8 = 30;

/// Unit directions, kept by sides and rotation bits.
type Table<T> = Vec<((u8, u32), Box<[T]>)>;

pub(super) struct Ink {
    mesh: Mesh,
    /// `draw_poly`'s rim directions by sides and rotation bits, rim start
    /// repeated at the end.
    polys: Table<Vec2>,
    /// `draw_arc`'s full-turn segment directions by sides and rotation
    /// bits: each segment's start and end.
    rings: Table<[Vec2; 2]>,
}

impl Default for Ink {
    fn default() -> Self {
        Self {
            mesh: Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                texture: None,
            },
            polys: Vec::new(),
            rings: Vec::new(),
        }
    }
}

/// `draw_poly`'s rim directions.
fn poly_rim(sides: u8, rotation: f32) -> Box<[Vec2]> {
    let rot = rotation.to_radians();
    (0..=sides)
        .map(|i| {
            let rx = (f32::from(i) / f32::from(sides) * std::f32::consts::PI * 2. + rot).cos();
            let ry = (f32::from(i) / f32::from(sides) * std::f32::consts::PI * 2. + rot).sin();
            vec2(rx, ry)
        })
        .collect()
}

/// `draw_arc`'s segment directions for a whole turn.
fn ring_segments(sides: u8, rotation: f32) -> Box<[[Vec2; 2]]> {
    let rot = rotation.to_radians();
    let part = 360.0_f32.to_radians();
    let sides = (f32::from(sides) * part / std::f32::consts::TAU)
        .ceil()
        .max(1.0);
    let span = part / sides;
    (0..crate::numeric::to_usize(sides))
        .map(|i| {
            let start = i as f32 * span + rot;
            let end = start + span;
            [Vec2::from_angle(start), Vec2::from_angle(end)]
        })
        .collect()
}

fn table<T>(
    tables: &mut Table<T>,
    sides: u8,
    rotation: f32,
    build: fn(u8, f32) -> Box<[T]>,
) -> usize {
    let key = (sides, rotation.to_bits());
    tables
        .iter()
        .position(|(kept, _)| *kept == key)
        .unwrap_or_else(|| {
            tables.push((key, build(sides, rotation)));
            tables.len() - 1
        })
}

impl Ink {
    /// Makes room for a shape of `vertices` and `indices`, drawing what the
    /// mesh holds first if it would not fit; returns the shape's first
    /// vertex index.
    fn reserve(&mut self, vertices: usize, indices: usize) -> u16 {
        if self.mesh.vertices.len() + vertices > MESH_VERTICES
            || self.mesh.indices.len() + indices > MESH_INDICES
        {
            self.flush();
        }
        u16::try_from(self.mesh.vertices.len()).expect("a mesh holds under 65536 vertices")
    }

    fn shape(&mut self, vertices: &[Vertex], indices: &[u16]) {
        let base = self.reserve(vertices.len(), indices.len());
        self.mesh.vertices.extend_from_slice(vertices);
        self.mesh
            .indices
            .extend(indices.iter().map(|index| base + index));
    }

    /// Draws what the mesh holds.
    pub(super) fn flush(&mut self) {
        if !self.mesh.indices.is_empty() {
            draw_mesh(&self.mesh);
        }
        self.mesh.vertices.clear();
        self.mesh.indices.clear();
    }

    /// `draw_line`.
    pub(super) fn line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        thickness: f32,
        color: Color,
    ) {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let nx = -dy;
        let ny = dx;
        let tlen = (nx * nx + ny * ny).sqrt() / (thickness * 0.5);
        if tlen < f32::EPSILON {
            return;
        }
        let tx = nx / tlen;
        let ty = ny / tlen;
        self.shape(
            &[
                Vertex::new(x1 + tx, y1 + ty, 0., 0., 0., color),
                Vertex::new(x1 - tx, y1 - ty, 0., 0., 0., color),
                Vertex::new(x2 + tx, y2 + ty, 0., 0., 0., color),
                Vertex::new(x2 - tx, y2 - ty, 0., 0., 0., color),
            ],
            &[0, 1, 2, 2, 1, 3],
        );
    }

    /// `draw_rectangle`.
    pub(super) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.shape(
            &[
                Vertex::new(x, y, 0., 0.0, 0.0, color),
                Vertex::new(x + w, y, 0., 1.0, 0.0, color),
                Vertex::new(x + w, y + h, 0., 1.0, 1.0, color),
                Vertex::new(x, y + h, 0., 0.0, 1.0, color),
            ],
            &[0, 1, 2, 0, 2, 3],
        );
    }

    /// `prim::stroke_rect`.
    pub(super) fn rect_lines(&mut self, rect: Rect, thickness: f32, color: Color) {
        for band in outline_bands(rect, thickness, screen_dpi_scale())
            .into_iter()
            .flatten()
        {
            self.rect(band.x, band.y, band.w, band.h, color);
        }
    }

    /// `draw_triangle`.
    pub(super) fn triangle(&mut self, a: Vec2, b: Vec2, c: Vec2, color: Color) {
        self.shape(
            &[
                Vertex::new(a.x, a.y, 0., 0., 0., color),
                Vertex::new(b.x, b.y, 0., 0., 0., color),
                Vertex::new(c.x, c.y, 0., 0., 0., color),
            ],
            &[0, 1, 2],
        );
    }

    /// `draw_poly`.
    pub(super) fn poly(
        &mut self,
        x: f32,
        y: f32,
        sides: u8,
        radius: f32,
        rotation: f32,
        color: Color,
    ) {
        let rim = table(&mut self.polys, sides, rotation, poly_rim);
        let count = usize::from(sides);
        let base = self.reserve(count + 2, count * 3);
        self.mesh
            .vertices
            .push(Vertex::new(x, y, 0., 0., 0., color));
        for (i, direction) in self.polys[rim].1.iter().enumerate() {
            let (rx, ry) = (direction.x, direction.y);
            self.mesh.vertices.push(Vertex::new(
                x + radius * rx,
                y + radius * ry,
                0.,
                rx,
                ry,
                color,
            ));
            if i != count {
                let i = u16::try_from(i).expect("a poly has under 256 sides");
                self.mesh
                    .indices
                    .extend_from_slice(&[base, base + i + 1, base + i + 2]);
            }
        }
    }

    /// `draw_poly_lines`, which macroquad draws as a whole-turn `draw_arc`.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors draw_poly_lines, whose output it reproduces"
    )]
    pub(super) fn poly_lines(
        &mut self,
        x: f32,
        y: f32,
        sides: u8,
        radius: f32,
        rotation: f32,
        thickness: f32,
        color: Color,
    ) {
        let ring = table(&mut self.rings, sides, rotation, ring_segments);
        let segments = self.rings[ring].1.len();
        self.reserve(segments * 4, segments * 6);
        let center = Vec2::new(x, y);
        for index in 0..segments {
            let [start, end] = self.rings[ring].1[index];
            let base = u16::try_from(self.mesh.vertices.len()).expect("reserved above");
            self.mesh
                .indices
                .extend([0, 1, 2, 2, 1, 3].map(|k| k + base));
            for (direction, radius) in [
                (start, radius),
                (start, radius + thickness),
                (end, radius),
                (end, radius + thickness),
            ] {
                let point = center + radius * direction;
                self.mesh
                    .vertices
                    .push(Vertex::new(point.x, point.y, 0., 0., 0., color));
            }
        }
    }

    /// `prim::fill_circle`.
    pub(super) fn circle(&mut self, center: Vec2, radius: f32, color: Color) {
        self.poly(center.x, center.y, CIRCLE_SIDES, radius, 0., color);
    }

    /// `prim::stroke_circle`.
    pub(super) fn circle_lines(&mut self, center: Vec2, radius: f32, thickness: f32, color: Color) {
        self.poly_lines(center.x, center.y, RING_SIDES, radius, 0., thickness, color);
    }
}

#[cfg(test)]
mod tests;
