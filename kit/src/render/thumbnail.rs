//! Map thumbnails drawn at the pixel size they are shown.
//!
//! Shrinking the full-size preview on the GPU samples one source pixel in
//! several and skips the rest, so a frame's two-pixel rim or a one-pixel
//! cliff lip survives or vanishes depending on where the samples land. A
//! thumbnail instead lays every tile on whole pixels and draws frames,
//! nodes, buildings and units as marks of one size each, never smaller
//! than a pixel, so nothing drops out however small the map is shown.

use super::*;

/// From this many pixels per tile up, tiles keep the one-pixel details of
/// the full-size preview: half its scale, where they are still lines.
const FINE_SCALE: f32 = 6.0;

/// The smallest frame mark: a pixel of rim around a pixel of core, so a
/// frame stays hollow and never reads as a scrap node.
const MIN_FRAME: f32 = 3.0;

/// From this unit radius in pixels up, units are discs like the full-size
/// preview's; below it, a square of their width.
const DISC_RADIUS: f32 = 2.0;

/// Draws `state` as a thumbnail that fits inside `width` x `height`
/// pixels, each seat in `seat_fill`. Tiles land on whole pixels, and every
/// frame, node, building and unit is one mark of its size at least a pixel
/// wide. Health bars, which carry nothing in a preview, are left out. The
/// scale never passes the full-size preview's [`TILE_PX`], where frames and
/// full nodes match it exactly.
pub fn render_thumbnail(
    state: &State,
    width: u32,
    height: u32,
    seat_fill: impl Fn(PlayerId) -> u32,
) -> Pixmap {
    let map = state.map();
    let scale = (width as f32 / map.width() as f32)
        .min(height as f32 / map.height() as f32)
        .min(TILE_PX);
    let mut pixmap = Pixmap::new(
        whole_pixels(edge(map.width(), scale)),
        whole_pixels(edge(map.height(), scale)),
    )
    .expect("a thumbnail is at least a pixel");
    pixmap.fill(rgb(GROUND));
    let shading = level_shading(state);
    let fine = scale >= FINE_SCALE;
    for (pos, tile) in map.iter() {
        let cell = cell_at(pos, scale);
        draw_terrain(&mut pixmap, map, (pos, tile), cell, shading.as_ref(), fine);
        match (tile.terrain, tile.scrap) {
            (Terrain::Ground, 0) if tile.wreck > 0 => {
                draw_inset_mark(&mut pixmap, pos, scale, WRECK_INSET, SCRAP_LOW);
            }
            (Terrain::Ground, scrap) if scrap > 0 => {
                let (color, inset) = node_look(scrap);
                draw_inset_mark(&mut pixmap, pos, scale, inset, color);
            }
            _ => {}
        }
    }

    let frame = (2.0 * scale).round().max(MIN_FRAME);
    for &pos in map.extractor_frames() {
        let (x, y) = mark_origin(centre(pos, 2, scale), frame);
        draw_frame(&mut pixmap, x, y, frame);
    }

    for building in state.buildings() {
        draw_thumbnail_building(&mut pixmap, building, scale, &seat_fill);
    }

    for unit in state.units() {
        let color = seat_fill(unit.player);
        let (cx, cy) = (
            unit.pos.x.to_num::<f32>() * scale,
            unit.pos.y.to_num::<f32>() * scale,
        );
        let radius = unit.kind.stats().radius.to_num::<f32>() * scale;
        if radius >= DISC_RADIUS {
            fill_circle(&mut pixmap, cx, cy, radius + 1.0, darken(color));
            fill_circle(&mut pixmap, cx, cy, radius, color);
        } else {
            draw_mark(
                &mut pixmap,
                (cx, cy),
                (2.0 * radius).round().max(1.0),
                color,
            );
        }
    }
    pixmap
}

/// A building over the whole pixels of its tiles: a dark frame around the
/// seat's fill once built, only the frame while it is a site. The frame
/// keeps a pixel wherever the building is at least four pixels wide.
fn draw_thumbnail_building(
    pixmap: &mut Pixmap,
    building: &aftermine_sim::Building,
    scale: f32,
    seat_fill: impl Fn(PlayerId) -> u32,
) {
    let color = seat_fill(building.player);
    let (w, h) = building.kind.size();
    let (x, pw) = span(building.anchor.x, building.anchor.x + w, scale);
    let (y, ph) = span(building.anchor.y, building.anchor.y + h, scale);
    fill_rect(pixmap, x, y, pw, ph, darken(color));
    if building.built() {
        let rim = (scale / 6.0)
            .round()
            .max(if pw.min(ph) >= 4.0 { 1.0 } else { 0.0 });
        fill_rect(
            pixmap,
            x + rim,
            y + rim,
            pw - 2.0 * rim,
            ph - 2.0 * rim,
            color,
        );
    }
}

/// Where tile edge `index` falls at `scale` pixels per tile.
fn edge(index: i32, scale: f32) -> f32 {
    (index as f32 * scale).round()
}

/// The whole-pixel start and length of tiles `from..to` along one axis.
fn span(from: i32, to: i32, scale: f32) -> (f32, f32) {
    let start = edge(from, scale);
    (start, edge(to, scale) - start)
}

/// The whole pixels tile `pos` covers.
fn cell_at(pos: TilePos, scale: f32) -> Cell {
    let (x, w) = span(pos.x, pos.x + 1, scale);
    let (y, h) = span(pos.y, pos.y + 1, scale);
    Cell { x, y, w, h }
}

/// The pixel centre of the `tiles`-wide square anchored at `pos`.
fn centre(pos: TilePos, tiles: i32, scale: f32) -> (f32, f32) {
    let half = tiles as f32 * 0.5;
    ((pos.x as f32 + half) * scale, (pos.y as f32 + half) * scale)
}

/// The whole-pixel corner of a `size`-pixel mark centred on `(cx, cy)`.
fn mark_origin((cx, cy): (f32, f32), size: f32) -> (f32, f32) {
    ((cx - size * 0.5).round(), (cy - size * 0.5).round())
}

/// A square set `inset` (a share of the tile) inside tile `pos`, at least a
/// pixel wide. Its corner snaps down as tiny-skia's unaliased fill snaps the
/// full-size preview's, so at full scale a full node lands on the same
/// pixels.
fn draw_inset_mark(pixmap: &mut Pixmap, pos: TilePos, scale: f32, inset: f32, color: u32) {
    let size = (scale * (1.0 - 2.0 * inset)).round().max(1.0);
    let x = ((pos.x as f32 + inset) * scale).floor();
    let y = ((pos.y as f32 + inset) * scale).floor();
    fill_rect(pixmap, x, y, size, size, color);
}

/// A solid `size`-pixel square centred on `centre`.
fn draw_mark(pixmap: &mut Pixmap, centre: (f32, f32), size: f32, color: u32) {
    let (x, y) = mark_origin(centre, size);
    fill_rect(pixmap, x, y, size, size, color);
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a thumbnail side is a rounded pixel count no larger than the full-size preview's"
)]
fn whole_pixels(length: f32) -> u32 {
    length.max(1.0) as u32
}

#[cfg(test)]
mod tests;
