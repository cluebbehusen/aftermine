//! Software rendering of sim state for previews and golden images.
//!
//! tiny-skia rasterizes on the CPU with fixed inputs, so the same state
//! produces the same PNG bytes on every machine and golden tests compare
//! byte-for-byte with no GPU or window. The style is a plain diagram (flat
//! tiles, circles, hp bars); the shell owns presentation.

use anyhow::{Context, Result};
use chassis::grid::TilePos;
use oxide_sim::map::{Elevation, Map, Terrain};
use oxide_sim::{BuildingKind, PlayerId, State, UnitKind};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Shader, Transform};

/// Whole pixels per tile.
const TILE_PIXELS: u32 = 12;

/// Pixels per tile.
pub const TILE_PX: f32 = TILE_PIXELS as f32;

fn rgb(hex: u32) -> Color {
    Color::from_rgba8(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
        0xff,
    )
}

const GROUND: u32 = 0x23_23_29;
const ROCK: u32 = 0x52_52_5E;
const PEAK: u32 = 0x22_21_2A;
const PEAK_FACE: u32 = 0x34_33_3D;
const PEAK_LIP: u32 = 0x57_54_5F;
const RUBBLE: u32 = 0x2C_2C_34;
const CLIFF_FACE: u32 = 0x15_15_1A;
const PIT: u32 = 0x0B_0B_10;
const PIT_RIM: u32 = 0x1B_1B_22;
const SCRAP_FULL: u32 = 0xD9_A4_41;
const SCRAP_LOW: u32 = 0x8C_6A_2F;
const FRAME: u32 = 0x3E_3E_48;
const HP_BACK: u32 = 0x14_14_18;
const HP_FRONT: u32 = 0xE8_E4_D8;

/// One distinct fill per seat, so every seat of a diagram reads apart.
/// Seats past the palette reuse it.
const SEAT_COLORS: [u32; 8] = [
    0xC4_57_3B, 0x3F_94_82, 0x64_8C_E6, 0xC8_A0_3C, 0xA0_5A_C8, 0x78_B4_50, 0xD2_78_96, 0x5A_B4_D2,
];

/// Brightness step between neighbouring levels of a tiered map that fits the
/// band at this step.
const LEVEL_STEP: f32 = 0.15;
/// The darkest a level is drawn.
const LEVEL_DARKEST: f32 = 0.7;
/// The brightest a level is drawn.
const LEVEL_BRIGHTEST: f32 = 1.45;

/// The level of a tiered map drawn at flat ground's brightness: the level
/// most of `starts` stand on, else the level most ground stands on, ties
/// going to the lower level.
pub fn floor_level(map: &Map, starts: impl IntoIterator<Item = TilePos>) -> u8 {
    let mut counts = [0u32; 256];
    for pos in starts {
        if let Some(Elevation::Level(level)) = map.elevation(pos) {
            counts[usize::from(level)] += 1;
        }
    }
    if counts.iter().all(|&count| count == 0) {
        for (pos, tile) in map.iter() {
            if tile.terrain == Terrain::Ground
                && let Some(Elevation::Level(level)) = map.elevation(pos)
            {
                counts[usize::from(level)] += 1;
            }
        }
    }
    (0..=u8::MAX)
        .rev()
        .max_by_key(|&level| counts[usize::from(level)])
        .unwrap_or(0)
}

/// How a tiered map shades its levels: the floor at flat ground's brightness
/// and each level away from it a step brighter or darker. The step is
/// [`LEVEL_STEP`] unless the map reaches deeper or higher than the band holds
/// at that step; then the levels on that side share the band evenly, so every
/// level keeps its own tone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    floor: u8,
    below: f32,
    above: f32,
}

impl Shading {
    /// The shading of `map` with `floor` at flat ground's brightness.
    pub fn new(map: &Map, floor: u8) -> Self {
        let (mut lowest, mut highest) = (floor, floor);
        for (pos, _) in map.iter() {
            if let Some(elevation) = map.elevation(pos) {
                let (low, high) = elevation.span();
                lowest = lowest.min(low);
                highest = highest.max(high);
            }
        }
        let step = |room: f32, levels: u8| {
            if levels == 0 {
                LEVEL_STEP
            } else {
                LEVEL_STEP.min(room / f32::from(levels))
            }
        };
        Self {
            floor,
            below: step(1.0 - LEVEL_DARKEST, floor - lowest),
            above: step(LEVEL_BRIGHTEST - 1.0, highest - floor),
        }
    }

    /// How bright ground at `level` is drawn against flat ground.
    pub fn tone(&self, level: f32) -> f32 {
        let from_floor = level - f32::from(self.floor);
        let step = if from_floor < 0.0 {
            self.below
        } else {
            self.above
        };
        (1.0 + step * from_floor).clamp(LEVEL_DARKEST, LEVEL_BRIGHTEST)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "each channel is clamped to a byte first"
)]
fn shade(hex: u32, tone: f32) -> u32 {
    let channel = |shift: u32| {
        let value = ((hex >> shift) & 0xff) as f32 * tone;
        (value.round().clamp(0.0, 255.0) as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// The tone of a tile's ground: a ramp halfway between its levels, a cliff
/// the level at its foot.
fn tile_tone(map: &Map, pos: TilePos, shading: &Shading) -> f32 {
    match map.elevation(pos) {
        Some(Elevation::Level(level)) => shading.tone(f32::from(level)),
        Some(Elevation::Ramp { low }) => shading.tone(f32::from(low) + 0.5),
        None => 1.0,
    }
}

/// A cliff seen from above: the lower ground at its foot, with the drop's
/// dark face and lit lip along each side that meets higher ground. A cliff
/// with no foot of its own, inside a taller band, is all face.
fn draw_cliff(pixmap: &mut Pixmap, map: &Map, pos: TilePos, (x, y): (f32, f32), shading: &Shading) {
    let Some(Elevation::Level(level)) = map.elevation(pos) else {
        return;
    };
    let ground_at = |at: TilePos, higher: bool| {
        map.tile(at).is_some_and(|t| t.terrain == Terrain::Ground)
            && map.elevation(at).is_some_and(|e| {
                if higher {
                    e.span().0 > level
                } else {
                    e.span().0 == level
                }
            })
    };
    let footed = (-1..=1).any(|dy| (-1..=1).any(|dx| ground_at(pos.offset(dx, dy), false)));
    if !footed {
        fill_rect(pixmap, x, y, TILE_PX, TILE_PX, CLIFF_FACE);
        return;
    }
    fill_rect(
        pixmap,
        x,
        y,
        TILE_PX,
        TILE_PX,
        shade(GROUND, shading.tone(f32::from(level))),
    );
    let face = TILE_PX / 4.0;
    for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        if !ground_at(pos.offset(dx, dy), true) {
            continue;
        }
        let (fx, fy, fw, fh, lx, ly, lw, lh) = match (dx, dy) {
            (0, -1) => (x, y, TILE_PX, face, x, y, TILE_PX, 1.0),
            (0, _) => (
                x,
                y + TILE_PX - face,
                TILE_PX,
                face,
                x,
                y + TILE_PX - 1.0,
                TILE_PX,
                1.0,
            ),
            (-1, _) => (x, y, face, TILE_PX, x, y, 1.0, TILE_PX),
            _ => (
                x + TILE_PX - face,
                y,
                face,
                TILE_PX,
                x + TILE_PX - 1.0,
                y,
                1.0,
                TILE_PX,
            ),
        };
        fill_rect(pixmap, fx, fy, fw, fh, CLIFF_FACE);
        fill_rect(pixmap, lx, ly, lw, lh, PEAK_LIP);
    }
}

/// A ramp's walls: a dark edge along each side it shares with a cliff.
fn draw_ramp_walls(pixmap: &mut Pixmap, map: &Map, pos: TilePos, (x, y): (f32, f32)) {
    let wall = 2.0;
    for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        if !map
            .tile(pos.offset(dx, dy))
            .is_some_and(|t| t.terrain == Terrain::Cliff)
        {
            continue;
        }
        let (wx, wy, ww, wh) = match (dx, dy) {
            (0, -1) => (x, y, TILE_PX, wall),
            (0, _) => (x, y + TILE_PX - wall, TILE_PX, wall),
            (-1, _) => (x, y, wall, TILE_PX),
            _ => (x + TILE_PX - wall, y, wall, TILE_PX),
        };
        fill_rect(pixmap, wx, wy, ww, wh, CLIFF_FACE);
    }
}

fn seat_color(player: PlayerId) -> u32 {
    SEAT_COLORS[usize::from(player.0) % SEAT_COLORS.len()]
}

fn darken(hex: u32) -> u32 {
    let (r, g, b) = ((hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff);
    ((r * 3 / 5) << 16) | ((g * 3 / 5) << 8) | (b * 3 / 5)
}

/// Solid fill. Anti-aliasing stays off for axis-aligned rects — it adds
/// nothing there and tiny-skia's AA hairline path asserts on sub-pixel
/// spans (which hp bars produce constantly).
fn solid(color: Color) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(color),
        anti_alias: false,
        ..Paint::default()
    }
}

fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: u32) {
    if let Some(rect) = Rect::from_xywh(x, y, w, h) {
        pixmap.fill_rect(rect, &solid(rgb(color)), Transform::identity(), None);
    }
}

fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: u32) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    if let Some(path) = pb.finish() {
        let mut paint = solid(rgb(color));
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Draws `state` to a fresh pixmap at [`TILE_PX`] resolution, each seat in
/// its own fill.
pub fn render_state(state: &State) -> Pixmap {
    render_state_colored(state, seat_color)
}

/// Draws `state` like [`render_state`], with `seat_fill` choosing each
/// seat's fill as `0xRRGGBB`.
pub fn render_state_colored(state: &State, seat_fill: impl Fn(PlayerId) -> u32) -> Pixmap {
    let width = u32::try_from(state.map().width()).expect("map extents are positive") * TILE_PIXELS;
    let height =
        u32::try_from(state.map().height()).expect("map extents are positive") * TILE_PIXELS;
    let mut pixmap = Pixmap::new(width, height).expect("nonzero map dimensions");
    pixmap.fill(rgb(GROUND));
    let map = state.map();
    // A tiered map tints each level's ground, brighter above the floor its
    // Foundries stand on and darker below.
    let shading = map.tiered().then(|| {
        let floor = floor_level(
            map,
            state
                .buildings()
                .iter()
                .filter(|b| b.kind == BuildingKind::Foundry)
                .map(|b| b.anchor),
        );
        Shading::new(map, floor)
    });

    for (pos, tile) in map.iter() {
        let (x, y) = (pos.x as f32 * TILE_PX, pos.y as f32 * TILE_PX);
        let tone = shading.map_or(1.0, |shading| tile_tone(map, pos, &shading));
        if tile.terrain == Terrain::Ground && tone != 1.0 {
            fill_rect(&mut pixmap, x, y, TILE_PX, TILE_PX, shade(GROUND, tone));
        }
        if map.is_ramp(pos) {
            draw_ramp_walls(&mut pixmap, map, pos, (x, y));
        }
        match (tile.terrain, tile.scrap) {
            (Terrain::Rock, _) => {
                fill_rect(&mut pixmap, x, y, TILE_PX, TILE_PX, ROCK);
            }
            (Terrain::Cliff, _) => {
                if let Some(shading) = &shading {
                    draw_cliff(&mut pixmap, map, pos, (x, y), shading);
                }
            }
            // Peaks are uncut quarry mesas: a dark full-tile mass with a
            // south-facing cut and cap, distinct from the loose rock fill.
            (Terrain::Peak, _) => {
                fill_rect(&mut pixmap, x, y, TILE_PX, TILE_PX, PEAK);
                fill_rect(&mut pixmap, x, y + 8.0, TILE_PX, 4.0, PEAK_FACE);
                fill_rect(&mut pixmap, x, y + 7.0, TILE_PX, 1.0, PEAK_LIP);
                fill_rect(&mut pixmap, x + 2.0, y + 3.0, 5.0, 1.0, PEAK_FACE);
            }
            // Bottomless pits fall away to near-black, with a thin lit rim
            // along the top edge so the void reads as depth, not paint.
            (Terrain::Pit, _) => {
                fill_rect(&mut pixmap, x, y, TILE_PX, TILE_PX, PIT);
                fill_rect(&mut pixmap, x, y, TILE_PX, 1.0, PIT_RIM);
            }
            // Wreck salvage: a small dim-amber square, unmistakably not a
            // node (nodes render bigger and brighter below).
            (Terrain::Ground, 0) if tile.wreck > 0 => {
                let inset = TILE_PX * 0.35;
                fill_rect(
                    &mut pixmap,
                    x + inset,
                    y + inset,
                    TILE_PX - 2.0 * inset,
                    TILE_PX - 2.0 * inset,
                    SCRAP_LOW,
                );
            }
            // Rubble: a faint lightening so the goldens register it.
            (Terrain::Ground, 0) if tile.cosmetic == 1 => {
                fill_rect(&mut pixmap, x, y, TILE_PX, TILE_PX, shade(RUBBLE, tone));
            }
            (Terrain::Ground, 0) => {}
            (Terrain::Ground, scrap) => {
                // Nodes shrink and dim as they deplete; rich nodes render
                // saturated at full size.
                let fraction = (scrap as f32 / oxide_sim::stats::SCRAP_NODE_AMOUNT as f32).min(1.0);
                let color = if fraction > 0.5 {
                    SCRAP_FULL
                } else {
                    SCRAP_LOW
                };
                let inset = TILE_PX * (0.15 + 0.25 * (1.0 - fraction));
                fill_rect(
                    &mut pixmap,
                    x + inset,
                    y + inset,
                    TILE_PX - 2.0 * inset,
                    TILE_PX - 2.0 * inset,
                    color,
                );
            }
        }
    }

    // Derelict Extractor frames: a gray block rimmed in scrap gold, so a
    // rebuildable site never reads as rock. A rebuilt Extractor covers it.
    for frame in state.map().extractor_frames() {
        let (x, y) = (frame.x as f32 * TILE_PX, frame.y as f32 * TILE_PX);
        let size = 2.0 * TILE_PX;
        fill_rect(
            &mut pixmap,
            x + 1.0,
            y + 1.0,
            size - 2.0,
            size - 2.0,
            SCRAP_FULL,
        );
        fill_rect(&mut pixmap, x + 3.0, y + 3.0, size - 6.0, size - 6.0, FRAME);
    }

    for building in state.buildings() {
        let color = seat_fill(building.player);
        let (w, h) = building.kind.size();
        let (x, y) = (
            building.anchor.x as f32 * TILE_PX,
            building.anchor.y as f32 * TILE_PX,
        );
        let (pw, ph) = (w as f32 * TILE_PX, h as f32 * TILE_PX);
        fill_rect(&mut pixmap, x, y, pw, ph, darken(color));
        // Unfinished sites show only their dark frame — scaffolding.
        if building.built() {
            fill_rect(&mut pixmap, x + 2.0, y + 2.0, pw - 4.0, ph - 4.0, color);
        }
        draw_hp_bar(
            &mut pixmap,
            x,
            y - 4.0,
            pw,
            building.hp,
            building.stats().max_hp,
        );
    }

    for unit in state.units() {
        let color = seat_fill(unit.player);
        let cx = unit.pos.x.to_num::<f32>() * TILE_PX;
        let cy = unit.pos.y.to_num::<f32>() * TILE_PX;
        let r = unit.kind.stats().radius.to_num::<f32>() * TILE_PX;
        fill_circle(&mut pixmap, cx, cy, r + 1.0, darken(color));
        fill_circle(&mut pixmap, cx, cy, r, color);
        if unit.kind == UnitKind::Harvester && unit.carrying() > 0 {
            fill_circle(&mut pixmap, cx, cy, r * 0.4, SCRAP_FULL);
        }
        let max_hp = unit.kind.stats().max_hp;
        if unit.hp < max_hp {
            draw_hp_bar(&mut pixmap, cx - r, cy - r - 4.0, 2.0 * r, unit.hp, max_hp);
        }
    }
    pixmap
}

fn draw_hp_bar(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, hp: u32, max_hp: u32) {
    fill_rect(pixmap, x, y, w, 2.0, HP_BACK);
    let fraction = hp as f32 / max_hp as f32;
    fill_rect(pixmap, x, y, w * fraction, 2.0, HP_FRONT);
}

/// Renders straight to PNG bytes (what golden tests compare).
pub fn png_bytes(state: &State) -> Result<Vec<u8>> {
    render_state(state).encode_png().context("encoding png")
}

/// Renders to a PNG file.
pub fn save_png(state: &State, path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    render_state(state)
        .save_png(path)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests;
