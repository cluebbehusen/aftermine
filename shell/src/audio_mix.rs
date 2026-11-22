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
/// The least share of its level a notice plays at, however far off.
const NOTICE_FLOOR: f32 = 0.4;
/// How far a blast the viewer did not witness sits behind one it did.
const FOGGED_BLAST_GAIN: f32 = 0.5;
/// How far toward one speaker a sound at the view's edge sits. Hard pans
/// tire headphones and leave one tablet speaker carrying a fight alone.
const PAN_WIDTH: f32 = 0.5;
/// The zoom at which a sound under the view's center is right beside the
/// listener; zooming out to `WIDE_ZOOM` lifts the camera to the far end.
const NEAR_ZOOM: f32 = 96.0;
/// How much farther a sound at the view's corner sits than one at its center.
const EDGE_REMOTENESS: f32 = 0.3;
/// How much quieter a sound at the view's corner plays than one at its center.
const EDGE_FALLOFF: f32 = 0.35;
/// How much farther an unseen blast at the end of its reach sits than one at
/// the view's edge.
const BEYOND_REMOTENESS: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameSound {
    pub kind: SoundKind,
    pub gain: f32,
    /// From -1, left, to 1, right: where the loudest emitter sits across
    /// the view.
    pub pan: f32,
    /// From 0, beside the listener, to 1, as far off as the camera can hear;
    /// `None` for sounds with no place on the map.
    pub remoteness: Option<f32>,
    /// Whether it competes for the frame's positional voices.
    competes: bool,
}

fn zoom_detail(zoom: f32) -> f32 {
    ((zoom - WIDE_ZOOM) / (DETAIL_ZOOM - WIDE_ZOOM)).clamp(0.0, 1.0)
}

fn zoom_gain(kind: SoundKind, zoom: f32) -> f32 {
    let detail = zoom_detail(zoom);
    match crate::mixer::spec(kind).weight {
        Weight::Detail => 0.25 + 0.75 * detail,
        Weight::Standard | Weight::Notice => 0.45 + 0.55 * detail,
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

/// How far `world` sits from the view's center, as a share of the distance to
/// its corner.
fn view_offset(world: Vec2, center: Vec2, half_extents: Vec2) -> f32 {
    let half_extents = Vec2::new(half_extents.x.max(1.0), half_extents.y.max(1.0));
    (world - center).length() / half_extents.length()
}

/// Turns down sounds toward the view's edges, so the action under the
/// center leads the mix.
fn edge_gain(world: Vec2, center: Vec2, half_extents: Vec2) -> f32 {
    1.0 - EDGE_FALLOFF * view_offset(world, center, half_extents).min(1.0).powf(1.5)
}

/// How far off a sound sits: the camera's height, then the sound's distance
/// across the view and, for a blast, beyond it.
fn remoteness(world: Vec2, center: Vec2, half_extents: Vec2, zoom: f32, reach: f32) -> f32 {
    let height = ((NEAR_ZOOM / zoom.max(1.0)).ln() / (NEAR_ZOOM / WIDE_ZOOM).ln()).clamp(0.0, 1.0);
    let half_extents = Vec2::new(half_extents.x.max(1.0), half_extents.y.max(1.0));
    let delta = (world - center).abs() - half_extents;
    let beyond = Vec2::new(delta.x.max(0.0), delta.y.max(0.0)).length() / reach.max(1.0);
    let across = view_offset(world, center, half_extents).min(1.0);
    (height + EDGE_REMOTENESS * across + BEYOND_REMOTENESS * beyond).clamp(0.0, 1.0)
}

fn pan(world: Vec2, center: Vec2, half_extents: Vec2) -> f32 {
    ((world.x - center.x) / half_extents.x.max(1.0)).clamp(-1.0, 1.0) * PAN_WIDTH
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
/// Unpositioned UI and alert sounds, and the player's notices, always survive. Positional duplicates keep
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
        let notice = spec.weight == Weight::Notice;
        let placed = world.filter(|_| spec.weight != Weight::Protected);
        let (gain, pan, remoteness) = match placed {
            Some(world) => {
                let set_back = if fogged { FOGGED_BLAST_GAIN } else { 1.0 };
                let gain = distance_gain(kind, world, center, half_extents)
                    * zoom_gain(kind, zoom)
                    * edge_gain(world, center, half_extents)
                    * set_back;
                let reach = if spec.explosion {
                    EXPLOSION_FALLOFF_TILES
                } else {
                    half_extents.length()
                };
                let remoteness = if fogged {
                    1.0
                } else {
                    remoteness(world, center, half_extents, zoom, reach)
                };
                let gain = if notice {
                    let floor = NOTICE_FLOOR / crate::audio_engine::distance_level(remoteness);
                    gain.max(floor.min(1.0))
                } else {
                    gain
                };
                (gain, pan(world, center, half_extents), Some(remoteness))
            }
            None => (1.0, 0.0, None),
        };

        if gain <= 0.0 {
            continue;
        }

        let competes = placed.is_some() && !notice;
        if let Some(existing) = mixed.iter_mut().find(|event| event.kind == kind) {
            if gain > existing.gain {
                existing.gain = gain;
                existing.pan = pan;
                existing.remoteness = remoteness;
                existing.competes = competes;
            }
        } else {
            mixed.push(FrameSound {
                kind,
                gain,
                pan,
                remoteness,
                competes,
            });
        }
    }

    let limit = positional_voice_limit(zoom);
    let mut ranked: Vec<usize> = mixed
        .iter()
        .enumerate()
        .filter_map(|(index, event)| event.competes.then_some(index))
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
