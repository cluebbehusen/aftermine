//! The Maw drawn from its runtime layers, and its cradle's crane.

use crate::assets::{CradleRig, MAW_SCRAP_HEAT_STEPS, MawRig, Sprites};
use macroquad::prelude::*;

/// The unit canvas every Maw layer shares, in pixels.
const CANVAS: f32 = 256.0;
/// A belt scrap clump's own canvas, in unit-canvas pixels.
const CLUMP: f32 = 24.0;
/// How far back the drum and hood sit when stowed, in canvas pixels.
const DRUM_STOW: f32 = 8.0;
/// The belt's scrap runs between these canvas columns.
const BELT_COLUMNS: (f32, f32) = (120.0, 136.0);
/// The canvas row where scrap leaves the hood onto the belt.
const HOOD_ROW: f32 = 89.0;
/// How far along the belt scrap starts to heat; it is hottest at the kiln.
const HEAT_FROM: f32 = 29.0;

/// The distance between the Maw's left and right bogie columns, in world
/// tiles: its belts roll at this gauge when it turns in place.
pub(crate) const GAUGE: f32 = (205.5 - 50.5) / CANVAS * 4.0;

/// The state one Maw is drawn in.
pub(super) struct MawPose<'a> {
    /// Each side's belt travel in world tiles.
    pub treads: [f32; 2],
    pub motion: Option<&'a crate::maw_motion::MawMotion>,
    pub alpha: f32,
    pub still: bool,
}

/// Draws the Maw centred on `center`, its canvas `size` pixels across and
/// turned by `rotation`.
pub(super) fn draw(
    sprites: &Sprites,
    rig: &MawRig,
    (center, rotation, size): (Vec2, f32, f32),
    tint: Color,
    pose: &MawPose<'_>,
    zoom: f32,
) {
    let scale = size / CANVAS;
    let (sin, cos) = rotation.sin_cos();
    let layer = |source: Rect, color: Color, offset: Vec2, dest: f32| {
        let at = offset * scale;
        let at = center + vec2(at.x * cos - at.y * sin, at.x * sin + at.y * cos);
        sprites.draw_unit(
            at.x - dest * 0.5,
            at.y - dest * 0.5,
            color,
            DrawTextureParams {
                dest_size: Some(vec2(dest, dest)),
                source: Some(source),
                rotation,
                ..Default::default()
            },
            zoom,
        );
    };
    let motion = pose.motion.filter(|_| !pose.still);
    let alpha = pose.alpha;
    for (side, frames) in rig.treads.iter().enumerate() {
        let travel = if pose.still { 0.0 } else { pose.treads[side] };
        layer(
            frames[tread_phase(travel, frames.len())],
            WHITE,
            Vec2::ZERO,
            size,
        );
    }
    layer(rig.hull[0], WHITE, Vec2::ZERO, size);
    layer(rig.hull[1], tint, Vec2::ZERO, size);
    let belt = motion.map_or(0.0, |motion| motion.belt(alpha));
    layer(
        rig.slats[cycle(belt, rig.slats.len())],
        WHITE,
        Vec2::ZERO,
        size,
    );
    if let Some(motion) = motion {
        let variants = rig.scrap.len() / MAW_SCRAP_HEAT_STEPS;
        for (along, variant) in motion.loads(alpha) {
            let heat =
                ((along - HEAT_FROM) / (crate::maw_motion::BELT_RUN - HEAT_FROM)).clamp(0.0, 1.0);
            let row = HOOD_ROW + along;
            let step = crate::numeric::to_usize((heat * (MAW_SCRAP_HEAT_STEPS - 1) as f32).round());
            let column = BELT_COLUMNS.0 + (BELT_COLUMNS.1 - BELT_COLUMNS.0) * scatter(variant);
            layer(
                rig.scrap[(variant % variants) * MAW_SCRAP_HEAT_STEPS + step],
                WHITE,
                vec2(column - CANVAS * 0.5, row - CANVAS * 0.5),
                CLUMP * scale,
            );
        }
    }
    layer(rig.kiln, WHITE, Vec2::ZERO, size);
    let heat = pose.motion.map_or(0.0, |motion| motion.heat(alpha));
    if heat > 0.0 {
        layer(rig.glow, Color { a: heat, ..WHITE }, Vec2::ZERO, size);
    }
    let deploy = pose.motion.map_or(0.0, |motion| motion.deploy(alpha));
    let arm = crate::numeric::to_usize((deploy * (rig.arms.len() - 1) as f32).round());
    layer(rig.arms[arm], WHITE, Vec2::ZERO, size);
    let spin = motion.map_or(0.0, |motion| motion.spin(alpha));
    let drum = rig.drum[cycle(
        spin / std::f32::consts::PI * rig.drum.len() as f32,
        rig.drum.len(),
    )];
    let stow = vec2(0.0, DRUM_STOW * (1.0 - deploy));
    layer(drum[0], WHITE, stow, size);
    layer(drum[1], tint, stow, size);
}

/// The cleat frame for a side that has rolled `travel` world tiles, its
/// frames one canvas pixel apart over the cleats' pitch. These are the
/// shared belts' mechanics: the belt surface keeps pace with the ground, so
/// a tile of travel moves the cleats a tile of canvas, and travel forward
/// runs the upper run toward the nose.
pub(super) fn tread_phase(travel: f32, frames: usize) -> usize {
    let pixels_per_tile = CANVAS / super::unit_draw_scale(oxide_sim::UnitKind::Maw);
    cycle(-travel * pixels_per_tile, frames)
}

/// The frame of a looping row at `position`, in frames.
fn cycle(position: f32, frames: usize) -> usize {
    crate::numeric::to_usize(position.rem_euclid(frames as f32).floor()) % frames
}

/// A load's place across the belt, 0 to 1, fixed for its variant.
fn scatter(variant: usize) -> f32 {
    ((variant * 7 + 3) % 11) as f32 / 10.0
}

/// How many times the bridge runs the length of the cradle and back over a
/// whole assembly, so it moves only while a crew raises the Maw.
const CRADLE_SWEEPS: f32 = 15.0;
/// The bridge's travel either side of its sweep's centre and that centre's
/// offset from the layer's middle row, and the trolley's travel along the
/// bridge, in footprint-canvas pixels.
const BRIDGE_TRAVEL: f32 = 78.0;
const BRIDGE_CENTRE: f32 = 4.0;
const TROLLEY_TRAVEL: f32 = 46.0;
/// The footprint canvas the crane layers share, in pixels.
const CRADLE_CANVAS: f32 = 192.0;

/// Where assembly `progress` puts the bridge and the trolley along it, as
/// offsets from the hull's nose end and from the footprint's middle row, in
/// footprint-canvas pixels. The cradle faces east, so the bridge travels
/// across columns.
pub(super) fn crane_offsets(progress: f32) -> (f32, f32) {
    let sweep = progress.clamp(0.0, 1.0) * CRADLE_SWEEPS;
    let bridge = BRIDGE_TRAVEL * (sweep * std::f32::consts::TAU).cos() - BRIDGE_CENTRE;
    let trolley = TROLLEY_TRAVEL * (sweep * std::f32::consts::PI).sin();
    (bridge, trolley)
}

/// Draws the crane over a cradle site whose footprint starts at `origin`
/// and spans `dest`.
pub(super) fn draw_crane(
    sprites: &Sprites,
    rig: &CradleRig,
    (origin, dest): (Vec2, Vec2),
    tint: Color,
    progress: f32,
    welding: bool,
    zoom: f32,
) {
    let scale = dest.x / CRADLE_CANVAS;
    let (bridge, along) = crane_offsets(progress);
    let trolley = rig.trolley[usize::from(welding).min(rig.trolley.len() - 1)];
    for (row_source, offset) in [
        (rig.bridge, vec2(bridge, 0.0)),
        (trolley, vec2(bridge, along)),
    ] {
        let at = origin + offset * scale;
        for (source, color) in [(row_source[0], WHITE), (row_source[1], tint)] {
            sprites.draw_building(
                at.x,
                at.y,
                color,
                DrawTextureParams {
                    dest_size: Some(dest),
                    source: Some(source),
                    ..Default::default()
                },
                zoom,
            );
        }
    }
}

/// How far inside the footprint the crew lamps sit, in footprint-canvas
/// pixels: clear of the runway beams, where a working crew's tools reach.
const LAMP_INSET: f32 = 20.0;

/// Each crew lamp's centre on the footprint, in footprint-canvas pixels,
/// beside the pad of the same index in `BuildingKind::crew_pads`.
pub(super) fn lamp_centre(pad: (i32, i32), size: (i32, i32)) -> Vec2 {
    let tile = CRADLE_CANVAS / size.0 as f32;
    let column = (pad.0 as f32 + 0.5) * tile;
    let row = if pad.1 < 0 {
        LAMP_INSET
    } else {
        CRADLE_CANVAS - LAMP_INSET
    };
    vec2(column, row)
}

/// Draws a cradle's crew pads under its crew and their lamps on its
/// runways; `lit` says which places are worked.
pub(super) fn draw_crew_places(
    sprites: &Sprites,
    rig: &CradleRig,
    (origin, dest, size): (Vec2, Vec2, (i32, i32)),
    pads: [(i32, i32); oxide_sim::stats::ASSEMBLY_CREW_CAP],
    lit: [bool; oxide_sim::stats::ASSEMBLY_CREW_CAP],
    zoom: f32,
) {
    let tile = dest.x / size.0 as f32;
    let scale = dest.x / CRADLE_CANVAS;
    let lamp_size = LAMP * scale;
    for (&(dx, dy), on) in pads.iter().zip(lit) {
        sprites.draw_building(
            origin.x + dx as f32 * tile,
            origin.y + dy as f32 * tile,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(tile, tile)),
                source: Some(rig.pad),
                ..Default::default()
            },
            zoom,
        );
        let centre = origin + lamp_centre((dx, dy), size) * scale;
        sprites.draw_building(
            centre.x - lamp_size * 0.5,
            centre.y - lamp_size * 0.5,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(lamp_size, lamp_size)),
                source: Some(rig.lamp[usize::from(on).min(rig.lamp.len() - 1)]),
                ..Default::default()
            },
            zoom,
        );
    }
}

/// A crew lamp's canvas, in footprint-canvas pixels.
const LAMP: f32 = 16.0;

#[cfg(test)]
mod tests;
