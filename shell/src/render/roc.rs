//! The Roc's layers around its hull and guns: ground shadows, the wall and
//! yokes beneath its lower-flank guns, and the fans and drone docks above
//! its hull. Offsets are in the hull canvas's pixels from its centre, nose
//! up, where a tile spans `CANVAS_TILE` pixels.

use super::{
    air_presentation, draw_mounted_guns, mount_pivot, reduced_motion, seat_identity_color,
};
use crate::assets::{RocRig, Sprites, UnitRig};
use crate::dock_motion::{DockMotion, LAUNCH_TICKS, launch_progress};
use crate::presentation_animation::{PropulsionState, TransportActionState, UnitAnimationState};
use aftermine_sim::UnitKind;
use macroquad::prelude::*;

const CANVAS_TILE: f32 = 64.0;
/// Each fan's centre.
const FANS: [Vec2; 2] = [vec2(-52.0, -15.0), vec2(52.0, -15.0)];
/// Each drone dock's centre, in the art's dock order.
const DOCKS: [Vec2; 6] = [
    vec2(-26.0, 12.0),
    vec2(-26.0, 37.0),
    vec2(-26.0, 62.0),
    vec2(26.0, 62.0),
    vec2(26.0, 37.0),
    vec2(26.0, 12.0),
];
/// The cargo hatch's centre.
const HATCH: Vec2 = vec2(0.0, 30.0);
/// How opaque the Roc stands in its cradle at each assembly stage, as the
/// building fade draws sites.
const ASSEMBLY_OPACITY: [f32; 3] = [96.0 / 255.0, 160.0 / 255.0, 224.0 / 255.0];
/// How far the art's light throws the hull's and a gun's ground shadows,
/// in screen space: the shadow does not turn with the hull.
const HULL_SHADOW: Vec2 = vec2(5.0, 12.0);
const GUN_SHADOW: Vec2 = vec2(3.0, 7.0);
/// A drone's, on its own canvas at the same pixel density.
const DRONE_SHADOW: Vec2 = vec2(3.0, 7.0);

/// A canvas offset turned with the hull, in screen pixels at `scale`
/// screen pixels per canvas pixel.
fn turned(offset: Vec2, rotation: f32, scale: f32) -> Vec2 {
    let (sin, cos) = rotation.sin_cos();
    vec2(
        offset.x * cos - offset.y * sin,
        offset.x * sin + offset.y * cos,
    ) * scale
}

fn draw_layer(
    sprites: &Sprites,
    (source, tint): (Rect, Color),
    center: Vec2,
    rotation: f32,
    scale: f32,
    zoom: f32,
) {
    let size = vec2(source.w, source.h) * scale;
    sprites.draw_unit(
        center.x - size.x * 0.5,
        center.y - size.y * 0.5,
        tint,
        DrawTextureParams {
            dest_size: Some(size),
            source: Some(source),
            rotation,
            ..Default::default()
        },
        zoom,
    );
}

/// The presentation clock in ticks, between this tick and the next.
fn now(game: &crate::game::Scene<'_>) -> f32 {
    game.state.current_tick() as f32 + game.presentation.tick_fraction()
}

/// A rig's own ground shadows in place of the shared airframe shadow: the
/// hull's, and each mounted gun's at its bearing. `body` is where the hull
/// is drawn, lifted.
pub(super) fn draw_shadows(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &aftermine_sim::Unit,
    rig: &UnitRig,
    (body, body_rotation, body_size): (Vec2, f32, Vec2),
    alpha: f32,
) {
    let zoom = game.presentation.camera.zoom;
    let scale = body_size.x / rig.hull(0).0.w;
    let throw = if unit.kind == UnitKind::Roc {
        HULL_SHADOW
    } else {
        DRONE_SHADOW
    };
    if let Some(shadow) = rig.shadow() {
        draw_layer(
            sprites,
            (shadow, WHITE),
            body + throw * scale,
            body_rotation,
            scale,
            zoom,
        );
    }
    let (Some(shadow), Some(guns)) = (rig.mount_shadow(), unit.kind.stats().mounts) else {
        return;
    };
    for (slot, mount) in guns.mounts.iter().enumerate() {
        let Some(rotation) = game.presentation.draw_mount_heading(unit, slot, alpha) else {
            continue;
        };
        let pivot = mount_pivot(body, body_rotation, mount, zoom);
        draw_layer(
            sprites,
            (shadow, WHITE),
            pivot + GUN_SHADOW * scale,
            rotation,
            scale,
            zoom,
        );
    }
}

/// The wall and yokes, then the guns, which hang under the hull.
pub(super) fn draw_under(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &aftermine_sim::Unit,
    (rig, roc): (&UnitRig, &RocRig),
    (body, body_rotation, body_size): (Vec2, f32, Vec2),
    hull: Rect,
    alpha: f32,
) {
    let zoom = game.presentation.camera.zoom;
    let scale = body_size.x / hull.w;
    for source in [roc.wall, roc.yoke] {
        draw_layer(sprites, (source, WHITE), body, body_rotation, scale, zoom);
    }
    draw_mounted_guns(
        game,
        sprites,
        unit,
        rig,
        (body, body_rotation, body_size),
        hull,
        alpha,
    );
}

fn ramp(t: f32, a: f32, b: f32) -> f32 {
    let u = ((t - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// How far the cargo hatch stands open while a rider boards or leaves:
/// it swings open, holds, then closes.
fn hatch_opening(transport: Option<TransportActionState>) -> f32 {
    match transport {
        Some(
            TransportActionState::Boarding { progress }
            | TransportActionState::Unloading { progress },
        ) => ramp(progress, 0.0, 0.25) * (1.0 - ramp(progress, 0.6, 1.0)),
        None => 0.0,
    }
}

/// The fans' blades and the cargo hatch, then each dock's clamps and the
/// drone resting in it or gliding into it, then the aft thrusters' glow at
/// the engines' heat.
pub(super) fn draw_over(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &aftermine_sim::Unit,
    roc: &RocRig,
    (body, body_rotation, body_size): (Vec2, f32, Vec2),
    animation: UnitAnimationState,
    hull: Rect,
) {
    let zoom = game.presentation.camera.zoom;
    let scale = body_size.x / hull.w;
    let tint = seat_identity_color(game, unit.player);
    let cycle = match animation.propulsion {
        PropulsionState::LiftRotors { cycle } => cycle,
        PropulsionState::None => 0.0,
    };
    for (side, frames) in roc.fans.iter().enumerate() {
        let frame = frames[crate::numeric::to_usize(cycle * frames.len() as f32) % frames.len()];
        let center = body + turned(FANS[side], body_rotation, scale);
        for layer in [(frame[0], WHITE), (frame[1], tint)] {
            draw_layer(sprites, layer, center, body_rotation, scale, zoom);
        }
    }
    let last = roc.hatch.len() - 1;
    let step = crate::numeric::to_usize((hatch_opening(animation.transport) * last as f32).round())
        .min(last);
    let center = body + turned(HATCH, body_rotation, scale);
    for layer in [(roc.hatch[step][0], WHITE), (roc.hatch[step][1], tint)] {
        draw_layer(sprites, layer, center, body_rotation, scale, zoom);
    }
    let settled = DockMotion::settled(unit.bay.map_or(0, |bay| bay.docked));
    let docks = game
        .presentation
        .dock_motion
        .get(&unit.id.0)
        .unwrap_or(&settled);
    let now = now(game);
    for (dock, steps) in roc.docks.iter().enumerate().take(DOCKS.len()) {
        let center = body + turned(DOCKS[dock], body_rotation, scale);
        let step =
            crate::numeric::to_usize((docks.spread(dock, now) * (steps.len() - 1) as f32).round())
                .min(steps.len() - 1);
        for layer in [(steps[step][0], WHITE), (steps[step][1], tint)] {
            draw_layer(sprites, layer, center, body_rotation, scale, zoom);
        }
        let rotor = dock + usize::try_from(game.state.current_tick() / 2 % 3).unwrap_or(0);
        if docks.resting(dock, now) {
            draw_drone(
                game,
                sprites,
                unit.player,
                (center, body_rotation, scale),
                rotor,
            );
        } else if let Some(landing) = docks.landing(dock, now) {
            let mut from = game.presentation.camera.to_screen(landing.from);
            from.y -= air_presentation(UnitKind::Drone, zoom).2;
            draw_drone(
                game,
                sprites,
                unit.player,
                (from.lerp(center, landing.progress), body_rotation, scale),
                rotor,
            );
        }
    }
    let heat = game.presentation.engine_heat(unit.id);
    if heat > 0.0 {
        draw_layer(
            sprites,
            (roc.glow, Color::new(1.0, 1.0, 1.0, heat)),
            body,
            body_rotation,
            scale,
            zoom,
        );
    }
}

/// The Roc its cradle is assembling, facing east as a new unit does, at the
/// site's assembly `stage`; `centre` is the footprint's centre.
pub(super) fn draw_assembling(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    player: aftermine_sim::PlayerId,
    centre: Vec2,
    stage: usize,
) {
    let Some(roc) = sprites.roc_rig() else {
        return;
    };
    let zoom = game.presentation.camera.zoom;
    let opacity = ASSEMBLY_OPACITY[stage.min(ASSEMBLY_OPACITY.len() - 1)];
    let scale = zoom * super::unit_draw_scale(UnitKind::Roc) / roc.ghost[0].w;
    let tint = seat_identity_color(game, player);
    for layer in [
        (roc.ghost[0], Color::new(1.0, 1.0, 1.0, opacity)),
        (roc.ghost[1], Color { a: opacity, ..tint }),
    ] {
        draw_layer(
            sprites,
            layer,
            centre,
            std::f32::consts::FRAC_PI_2,
            scale,
            zoom,
        );
    }
}

/// A drone sitting on its host, at the host's pixel density.
fn draw_drone(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    player: aftermine_sim::PlayerId,
    (center, rotation, scale): (Vec2, f32, f32),
    rotor: usize,
) {
    let Some(rig) = sprites.unit_rig(UnitKind::Drone) else {
        return;
    };
    let zoom = game.presentation.camera.zoom;
    let tint = seat_identity_color(game, player);
    for (body, accent) in [rig.hull(rotor % 3), rig.mount(None)] {
        for layer in [(body, WHITE), (accent, tint)] {
            draw_layer(sprites, layer, center, rotation, scale, zoom);
        }
    }
}

/// Where a drone that launched moments ago is drawn, in world tiles. The
/// simulation launches it from its host's centre; it is drawn off that
/// path by its dock's offset, easing to nothing as it rises clear, so it
/// leaves from its dock and never crosses the hull.
pub(crate) fn drone_draw_pos(
    game: &crate::game::Scene<'_>,
    unit: &aftermine_sim::Unit,
    alpha: f32,
) -> Option<Vec2> {
    let &(host_id, dock, at) = game.presentation.drone_launches.get(&unit.id.0)?;
    let age = now(game) - at as f32;
    if age >= LAUNCH_TICKS {
        return None;
    }
    let host = game.state.unit(host_id)?;
    let rotation = game
        .presentation
        .draw_heading(host_id, host.weapon_heading(), alpha)
        + game
            .presentation
            .slide_yaw(host_id, alpha, reduced_motion());
    let lifts = air_presentation(UnitKind::Drone, 1.0).2 - air_presentation(host.kind, 1.0).2;
    let offset = turned(*DOCKS.get(dock)?, rotation, 1.0 / CANVAS_TILE) + vec2(0.0, lifts);
    let flying = game.presentation.draw_pos(unit.id, unit.pos, alpha);
    Some(flying + offset * (1.0 - launch_progress(age)))
}

#[cfg(test)]
mod tests;
