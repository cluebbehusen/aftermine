//! The minimap: its corner geometry (one source for drawing, clicks,
//! and the debug protocol's chrome report) and its drawing, which shows
//! live state on visible ground and memories elsewhere, like the world
//! renderer.

use super::*;
use crate::numeric::Fit;
use crate::render::prim::stroke_circle;

const MINIMAP_MAX: Vec2 = vec2(220.0, 150.0);
const MINI_VOID: Color = color_u8!(10, 10, 13, 255);
const MINI_GROUND: Color = color_u8!(44, 44, 52, 255);
const MINI_ROCK: Color = color_u8!(84, 84, 96, 255);
const MINI_PEAK: Color = color_u8!(48, 47, 57, 255);
const MINI_PIT: Color = color_u8!(6, 6, 9, 255);
/// How much darker a cliff is than the ground at its foot.
const MINI_CLIFF: f32 = 0.55;

fn toned(color: Color, tone: f32) -> Color {
    Color::new(
        (color.r * tone).min(1.0),
        (color.g * tone).min(1.0),
        (color.b * tone).min(1.0),
        color.a,
    )
}

/// Minimap identity color: the same seat accents the world renderer uses
/// (own faction, cool allies, warm hostiles).
fn mini_entity_color(game: &crate::game::Scene<'_>, owner: oxide_sim::PlayerId) -> Color {
    super::seat_identity_color(game, owner)
}

fn dim(color: Color) -> Color {
    Color::new(color.r * 0.55, color.g * 0.55, color.b * 0.55, color.a)
}

/// The minimap's staleness fade: memories slide toward the dim ground
/// color as they age, matching the world view's alpha fade.
fn stale_toward(color: Color, floor: Color, age: f32) -> Color {
    let t = super::staleness_fade(age);
    Color::new(
        color.r + (floor.r - color.r) * t,
        color.g + (floor.g - color.g) * t,
        color.b + (floor.b - color.b) * t,
        color.a,
    )
}

/// How long since the player last saw this key. Unstamped memories (from
/// loaded saves) start aging now.
fn memory_age(game: &crate::game::Scene<'_>, key: (i32, i32)) -> f32 {
    let mut seen = game.presentation.last_seen.borrow_mut();
    let stamp = *seen
        .entry(key)
        .or_insert_with(|| game.presentation.fx_time());
    game.presentation.fx_time() - stamp
}

/// Where the minimap sits (flush bottom-right, matching the command
/// band's bottom edge) for a map of `map_w`×`map_h` tiles in a
/// `viewport`-pixel window. Shared with input hit-testing and unit tests.
pub fn minimap_rect_for(map_w: i32, map_h: i32, viewport: Vec2) -> Rect {
    minimap_rect_scaled(map_w, map_h, viewport, ui_scale())
}

/// [`minimap_rect_for`] with an explicit UI scale.
pub fn minimap_rect_scaled(map_w: i32, map_h: i32, viewport: Vec2, s: f32) -> Rect {
    let mw = map_w as f32;
    let mh = map_h as f32;
    let scale = (MINIMAP_MAX.x * s / mw).min(MINIMAP_MAX.y * s / mh);
    let (w, h) = (mw * scale, mh * scale);
    Rect::new(viewport.x - w - 12.0 * s, viewport.y - h - 12.0 * s, w, h)
}

/// Where the minimap sits this frame.
pub fn minimap_rect(game: &crate::game::Scene<'_>) -> Rect {
    minimap_rect_for(
        game.state.map().width(),
        game.state.map().height(),
        game.presentation.camera.viewport(),
    )
}

/// The world point under a screen position, if it lies on the minimap.
/// Clicks use it to jump the camera and to aim armed ground orders.
pub fn minimap_world_at(game: &crate::game::Scene<'_>, screen: Vec2) -> Option<Vec2> {
    // The published rect, not a recomputation: hit-testing reads the
    // LayoutModel like all chrome and never touches the window, which
    // keeps the click path headless-testable.
    minimap_world_in(
        game.presentation.layout.get().minimap,
        game.state.map().width(),
        screen,
    )
}

/// The world point a pointer dragging from the minimap steers to:
/// clamped into the published rect, so sliding off an edge holds the
/// view at that edge instead of stalling the pan. `None` while the
/// minimap is hidden.
pub fn minimap_world_clamped(game: &crate::game::Scene<'_>, screen: Vec2) -> Option<Vec2> {
    let rect = game.presentation.layout.get().minimap;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let clamped = vec2(
        screen.x.clamp(rect.x, rect.x + rect.w - 1.0),
        screen.y.clamp(rect.y, rect.y + rect.h - 1.0),
    );
    minimap_world_in(rect, game.state.map().width(), clamped)
}

/// Testable core of [`minimap_world_at`], given the minimap rect.
pub fn minimap_world_in(rect: Rect, map_w: i32, screen: Vec2) -> Option<Vec2> {
    if !rect.contains(screen) {
        return None;
    }
    let scale = rect.w / map_w as f32;
    Some(vec2(
        (screen.x - rect.x) / scale,
        (screen.y - rect.y) / scale,
    ))
}

/// The minimap's terrain-and-fog layer: one pixel per tile, uploaded to
/// a texture and drawn as a single scaled quad instead of one quad per
/// tile. Created lazily inside the draw so headless sessions never touch
/// the GPU.
pub(crate) struct MinimapLayer {
    paint: MinimapPaint,
    texture: Texture2D,
    filter: FilterMode,
}

impl MinimapLayer {
    fn ensure<'a>(slot: &'a mut Option<Self>, game: &crate::game::Scene<'_>) -> &'a mut Self {
        let map = game.state.map();
        let (w, h) = (
            map.width().max(1).fit::<u16>(),
            map.height().max(1).fit::<u16>(),
        );
        if slot
            .as_ref()
            .is_none_or(|layer| layer.paint.image.width != w || layer.paint.image.height != h)
        {
            let paint = MinimapPaint::new(game);
            let texture = Texture2D::from_image(&paint.image);
            *slot = Some(Self {
                paint,
                texture,
                filter: FilterMode::Linear,
            });
        }
        slot.as_mut().expect("just ensured")
    }

    /// Repaints on the next draw, for a world that changed without a tick.
    pub(crate) fn forget(&mut self) {
        self.paint.painted = None;
    }
}

/// How often remembered salvage steps along its fade while nothing else
/// changes. The fade takes 90 seconds, far below a color step per tick.
const FADE_STEPS_PER_SECOND: f32 = 4.0;

/// What the minimap image last showed. Tiles change only when a tick lands,
/// the viewer changes, or a memory fades a step.
#[derive(Clone, Copy, PartialEq)]
struct Painted {
    tick: u64,
    viewer: oxide_sim::PlayerId,
    omniscient: bool,
    fade: u32,
}

/// The minimap's pixels, kept apart from the texture so tests can paint
/// them headless.
struct MinimapPaint {
    image: Image,
    /// Each tile's color when explored and bare. Terrain and level tones
    /// never change during a match.
    terrain: Vec<Color>,
    painted: Option<Painted>,
}

impl MinimapPaint {
    fn new(game: &crate::game::Scene<'_>) -> Self {
        let map = game.state.map();
        let levels = game.ground().levels.as_ref();
        let terrain = map
            .iter()
            .map(|(pos, tile)| {
                // Ground on a tiered map takes its level's tone, and a cliff
                // reads as a dark drop at the ground of its foot.
                let tone = levels.and_then(|levels| levels.minimap_tone(map, pos));
                let ground = tone.map_or(MINI_GROUND, |tone| toned(MINI_GROUND, tone));
                match tile.terrain {
                    oxide_sim::map::Terrain::Rock => MINI_ROCK,
                    oxide_sim::map::Terrain::Cliff => toned(ground, MINI_CLIFF),
                    oxide_sim::map::Terrain::Peak => MINI_PEAK,
                    oxide_sim::map::Terrain::Pit => MINI_PIT,
                    oxide_sim::map::Terrain::Ground => ground,
                }
            })
            .collect();
        let (w, h) = (
            map.width().max(1).fit::<u16>(),
            map.height().max(1).fit::<u16>(),
        );
        Self {
            image: Image::gen_image_color(w, h, MINI_VOID),
            terrain,
            painted: None,
        }
    }

    /// Brings the pixels up to date, reporting whether any changed.
    fn refresh(&mut self, game: &crate::game::Scene<'_>) -> bool {
        let omniscient = game.presentation.all_seeing();
        let now = Painted {
            tick: game.state.current_tick(),
            viewer: game.presentation.human,
            omniscient,
            fade: crate::numeric::to_u32(game.presentation.fx_time() * FADE_STEPS_PER_SECOND),
        };
        if self.painted == Some(now) {
            return false;
        }
        self.painted = Some(now);
        let vision = game.my_vision();
        let mut changed = false;
        for ((pos, tile), &bare) in game.state.map().iter().zip(&self.terrain) {
            let (explored, visible) = if omniscient {
                (true, true)
            } else {
                (vision.explored(pos), vision.visible(pos))
            };
            let color = if explored {
                // Same memory rule as the world view: live scrap in sight,
                // last-seen scrap under the dim.
                let scrap = if visible {
                    tile.scrap
                } else {
                    vision.remembered_scrap(pos)
                };
                let base = if tile.terrain == oxide_sim::map::Terrain::Ground && scrap > 0 {
                    SCRAP_COLOR
                } else {
                    bare
                };
                if visible {
                    // Stamp what is on show. The world renderer stamps only
                    // camera-visible tiles, so without this a node re-scouted
                    // off-camera would keep fading from its old timestamp.
                    if tile.scrap > 0 || tile.wreck > 0 {
                        game.presentation
                            .last_seen
                            .borrow_mut()
                            .insert((pos.x, pos.y), game.presentation.fx_time());
                    }
                    base
                } else if scrap > 0 {
                    // Remembered salvage fades like the world view's but
                    // never disappears.
                    stale_toward(
                        dim(base),
                        dim(MINI_GROUND),
                        memory_age(game, (pos.x, pos.y)),
                    )
                } else {
                    dim(base)
                }
            } else {
                MINI_VOID
            };
            changed |= repaint(&mut self.image, pos.x.fit(), pos.y.fit(), color);
        }
        changed
    }
}

/// Sets one pixel, reporting whether its color changed.
fn repaint(image: &mut Image, x: u32, y: u32, color: Color) -> bool {
    let index = (y * u32::from(image.width) + x).fit::<usize>();
    let pixel: [u8; 4] = color.into();
    let slot = &mut image.get_image_data_mut()[index];
    let changed = *slot != pixel;
    *slot = pixel;
    changed
}

/// Draws the minimap under the same fog rules as the world view
/// (omniscient while the F1 overlay is up).
pub(crate) fn draw_minimap(game: &crate::game::Scene<'_>) {
    let rect = game.presentation.layout.get().minimap;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let scale = rect.w / game.state.map().width() as f32;
    let omniscient = game.presentation.all_seeing();
    draw_rectangle(
        rect.x - 3.0,
        rect.y - 3.0,
        rect.w + 6.0,
        rect.h + 6.0,
        PANEL,
    );

    let mut layer_slot = game.presentation.minimap_layer.borrow_mut();
    let layer = MinimapLayer::ensure(&mut layer_slot, game);
    // Replacing a texture the GPU may still be drawing from can stall the
    // frame, so upload only after a pixel changed.
    if layer.paint.refresh(game) {
        layer.texture.update(&layer.paint.image);
    }
    // Downscaled tiles (sub-pixel on grand maps) blend; upscaled tiles
    // stay crisp blocks.
    let filter = if scale < 1.0 {
        FilterMode::Linear
    } else {
        FilterMode::Nearest
    };
    if layer.filter != filter {
        layer.texture.set_filter(filter);
        layer.filter = filter;
    }
    draw_texture_ex(
        &layer.texture,
        rect.x,
        rect.y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(rect.w, rect.h)),
            ..Default::default()
        },
    );
    drop(layer_slot);

    let vision = game.my_vision();
    if !omniscient {
        for ghost in vision.ghosts() {
            let (w, h) = ghost.kind.size();
            let age = memory_age(game, (ghost.anchor.x, ghost.anchor.y));
            // Ghosts keep their dimmed seat color as it fades, so
            // allegiance stays readable when the player plans from memory.
            let color = stale_toward(
                dim(mini_entity_color(game, ghost.owner)),
                dim(MINI_GROUND),
                age,
            );
            draw_rectangle(
                rect.x + ghost.anchor.x as f32 * scale,
                rect.y + ghost.anchor.y as f32 * scale,
                w as f32 * scale,
                h as f32 * scale,
                color,
            );
        }
        // Public buildings are known facts, not fading memories: a full
        // seat-colored outline, set apart from the filled ghosts.
        for public in
            oxide_sim::observation::hostile_public_buildings(game.state, game.presentation.human)
        {
            let (w, h) = public.kind.size();
            let color = mini_entity_color(game, public.owner);
            draw_rectangle_lines(
                rect.x + public.anchor.x as f32 * scale - 1.0,
                rect.y + public.anchor.y as f32 * scale - 1.0,
                w as f32 * scale + 2.0,
                h as f32 * scale + 2.0,
                2.0,
                color,
            );
            // A cradle fills from the bottom as its crew raises it.
            if let Some(progress) = public.progress {
                let build = public
                    .kind
                    .base_stats()
                    .construction
                    .map_or(1, |c| c.build_ticks.max(1));
                let filled = h as f32 * scale * progress.min(build) as f32 / build as f32;
                draw_rectangle(
                    rect.x + public.anchor.x as f32 * scale,
                    rect.y + (public.anchor.y + h) as f32 * scale - filled,
                    w as f32 * scale,
                    filled,
                    color,
                );
            }
        }
    }
    for building in game.state.buildings() {
        let seen = omniscient
            || building.player == game.presentation.human
            || (building.tiles().any(|t| vision.visible(t))
                && game
                    .state
                    .building_apparent(game.presentation.human, building));
        if !seen {
            continue;
        }
        let (w, h) = building.kind.size();
        draw_rectangle(
            rect.x + building.anchor.x as f32 * scale,
            rect.y + building.anchor.y as f32 * scale,
            w as f32 * scale,
            h as f32 * scale,
            mini_entity_color(game, building.player),
        );
    }
    for unit in game.state.units() {
        let seen = omniscient || unit.player == game.presentation.human || vision.sees_unit(unit);
        if !seen {
            continue;
        }
        // A hull wider than a tile shows at its size; small bodies share one dot.
        let excess = oxide_sim::stats::hull_excess(unit.kind.stats().radius).to_num::<f32>();
        let dot = (scale * (0.7 + 2.0 * excess)).max(2.0);
        draw_rectangle(
            rect.x + unit.pos.x.to_num::<f32>() * scale - dot * 0.5,
            rect.y + unit.pos.y.to_num::<f32>() * scale - dot * 0.5,
            dot,
            dot,
            mini_entity_color(game, unit.player),
        );
    }

    // Camera frame.
    let (lo, hi) = game.presentation.camera.world_rect();
    let x = rect.x + lo.x.max(0.0) * scale;
    let y = rect.y + lo.y.max(0.0) * scale;
    let x2 = rect.x + hi.x.min(game.state.map().width() as f32) * scale;
    let y2 = rect.y + hi.y.min(game.state.map().height() as f32) * scale;
    draw_rectangle_lines(x, y, (x2 - x).max(4.0), (y2 - y).max(4.0), 1.5, BONE);

    // Under-attack pulses: an expanding, fading ring at each alert, or
    // under reduced motion a steady marker that fades without expanding.
    // An assembling experimental pulses as a square in bone, so it reads
    // apart from an attack by shape as well as color.
    for (world, age, kind) in &game.presentation.alerts {
        let center = vec2(rect.x + world.x * scale, rect.y + world.y * scale);
        let (radius, alpha) = if reduced_motion() {
            (5.0, (1.0 - (age / 6.0)).max(0.0))
        } else {
            let pulse = (age * 1.5).fract();
            (
                2.0 + pulse * 10.0,
                (1.0 - pulse) * (1.0 - (age / 6.0)).max(0.0),
            )
        };
        match kind {
            crate::game::AlertKind::Attack => {
                stroke_circle(center, radius, 1.5, Color::new(0.85, 0.32, 0.29, alpha));
            }
            crate::game::AlertKind::Assembly(_) => draw_rectangle_lines(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
                1.5,
                Color { a: alpha, ..BONE },
            ),
        }
    }
}

#[cfg(test)]
mod tests;
