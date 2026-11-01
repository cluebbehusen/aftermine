//! Pure camera-listener weighting for one frame of queued sound events.

use crate::game::SoundKind;
use crate::mixer::Weight;
use crate::numeric;
use macroquad::prelude::Vec2;

const WIDE_ZOOM: f32 = 8.0;
const DETAIL_ZOOM: f32 = 32.0;
const WIDE_POSITIONAL_VOICES: usize = 5;
const CLOSE_POSITIONAL_VOICES: usize = 12;
const EXPLOSION_FALLOFF_TILES: f32 = 24.0;
/// How far a blast the viewer did not witness sits behind one it did.
const FOGGED_BLAST_GAIN: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameSound {
    pub kind: SoundKind,
    pub gain: f32,
    /// Whether hearing it raises the score's combat bed.
    pub combat: bool,
    positioned: bool,
}

fn zoom_detail(zoom: f32) -> f32 {
    ((zoom - WIDE_ZOOM) / (DETAIL_ZOOM - WIDE_ZOOM)).clamp(0.0, 1.0)
}

fn zoom_gain(kind: SoundKind, zoom: f32) -> f32 {
    let detail = zoom_detail(zoom);
    match crate::mixer::spec(kind).weight {
        Weight::Detail => 0.25 + 0.75 * detail,
        Weight::Standard => 0.45 + 0.55 * detail,
        Weight::Heavy => 0.72 + 0.28 * detail,
        Weight::Protected => 1.0,
    }
}

fn distance_gain(kind: SoundKind, world: Vec2, center: Vec2, half_extents: Vec2) -> f32 {
    let half_extents = Vec2::new(half_extents.x.max(1.0), half_extents.y.max(1.0));
    let delta = (world - center).abs() - half_extents;
    let outside = Vec2::new(delta.x.max(0.0), delta.y.max(0.0));
    let distance = outside.length();
    let reach = if crate::mixer::spec(kind).explosion {
        EXPLOSION_FALLOFF_TILES
    } else {
        half_extents.length()
    };
    (1.0 - distance / reach).clamp(0.0, 1.0)
}

fn positional_voice_limit(zoom: f32) -> usize {
    let detail = zoom_detail(zoom);
    WIDE_POSITIONAL_VOICES
        + numeric::to_usize(
            ((CLOSE_POSITIONAL_VOICES - WIDE_POSITIONAL_VOICES) as f32 * detail).round(),
        )
}

/// Coalesces one frame of emitters into the mix heard at the current camera.
///
/// Unpositioned UI and alert sounds always survive. Positional duplicates keep
/// the loudest emitter of their kind, and a wide camera admits fewer minor
/// voices while reserving room for heavy threats. A fogged blast plays set back
/// and leaves the score alone, so it cannot announce a fight the viewer cannot
/// see.
pub(crate) fn frame_mix(
    queued: impl IntoIterator<Item = (SoundKind, Option<Vec2>)>,
    fogged_blasts: impl IntoIterator<Item = (SoundKind, Vec2)>,
    center: Vec2,
    half_extents: Vec2,
    zoom: f32,
) -> Vec<FrameSound> {
    let queued = queued.into_iter().map(|(kind, world)| (kind, world, false));
    let fogged = fogged_blasts
        .into_iter()
        .map(|(kind, world)| (kind, Some(world), true));
    let mut mixed: Vec<FrameSound> = Vec::new();
    for (kind, world, fogged) in queued.chain(fogged) {
        let spec = crate::mixer::spec(kind);
        let protected = spec.weight == Weight::Protected;
        let positioned = world.is_some() && !protected;
        let gain = if protected {
            1.0
        } else if let Some(world) = world {
            let set_back = if fogged { FOGGED_BLAST_GAIN } else { 1.0 };
            distance_gain(kind, world, center, half_extents) * zoom_gain(kind, zoom) * set_back
        } else {
            1.0
        };

        if gain <= 0.0 {
            continue;
        }

        let combat = spec.combat && !fogged;
        if let Some(existing) = mixed.iter_mut().find(|event| event.kind == kind) {
            existing.combat |= combat;
            if gain > existing.gain {
                existing.gain = gain;
                existing.positioned = positioned;
            }
        } else {
            mixed.push(FrameSound {
                kind,
                gain,
                combat,
                positioned,
            });
        }
    }

    let limit = positional_voice_limit(zoom);
    let mut ranked: Vec<usize> = mixed
        .iter()
        .enumerate()
        .filter_map(|(index, event)| event.positioned.then_some(index))
        .collect();
    ranked.sort_by(|&left, &right| {
        let heavy = |event: FrameSound| crate::mixer::spec(event.kind).weight == Weight::Heavy;
        heavy(mixed[right])
            .cmp(&heavy(mixed[left]))
            .then_with(|| mixed[right].gain.total_cmp(&mixed[left].gain))
            .then_with(|| left.cmp(&right))
    });
    if ranked.len() > limit {
        let mut keep = vec![true; mixed.len()];
        for index in ranked.into_iter().skip(limit) {
            keep[index] = false;
        }
        let mut index = 0;
        mixed.retain(|_| {
            let retain = keep[index];
            index += 1;
            retain
        });
    }

    mixed
}

#[cfg(test)]
mod tests;
