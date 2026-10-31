//! The fixed ground: the terrain layers that read the map alone. Terrain never
//! changes during a match, so once the zoom holds still these layers are baked
//! into textures a chunk at a time and the frame draws the textures. While the
//! zoom glides they draw straight to the screen.
//!
//! Two layers bracket the ground's live overlays. The floor lies under
//! salvage; the raised layer of blockers, pits, slabs and terraces lies over
//! it, so a slab's overhang still covers the scrap at its foot. Chunks hold
//! premultiplied color, which composites onto the frame as the passes
//! themselves would have blended there.

use super::{GroundLayout, boulders, pits, terraces, world};
use crate::assets::Sprites;
use crate::numeric;
use aftermine_sim::Scenario;
use aftermine_sim::map::{Map, Terrain};
use chassis::grid::TilePos;
use macroquad::prelude::*;
use std::collections::BTreeMap;

/// Logical pixels on a side of one chunk. Chunks are a fixed size on screen,
/// so the cache holds about a screen of them per layer at any zoom.
const CHUNK: f32 = 256.0;
/// Most chunks baked in one frame. A view missing more draws its ground
/// straight to the screen while the cache fills over the next frames.
const BAKES_PER_FRAME: usize = 12;
/// Tiles past a chunk's edge whose art can still reach into it: dressing,
/// pit relief and wear marks overhang their tile by less than one. Slabs reach
/// further and pad their own window.
const REACH: f32 = 1.0;

/// Where the ground passes draw, in logical pixels: the screen, or one chunk
/// of the cache. World `(0, 0)` lands on a whole pixel, so ground drawn
/// straight to the screen floors to the same pixels as a chunk baked at the
/// same zoom and moved into place.
#[derive(Debug, PartialEq)]
pub(super) struct GroundView {
    zoom: f32,
    /// Where world `(0, 0)` lands.
    origin: Vec2,
    /// The world rectangle to cover; the passes draw every tile that can
    /// reach into it.
    lo: Vec2,
    hi: Vec2,
}

impl GroundView {
    /// The camera's view of the ground this frame.
    pub(super) fn live(camera: &crate::camera::Camera) -> Self {
        let (lo, hi) = camera.world_rect();
        Self {
            zoom: camera.zoom,
            origin: camera.to_screen(Vec2::ZERO).floor(),
            lo,
            hi,
        }
    }

    /// The view baked into `chunk` at `zoom`.
    fn chunk(zoom: f32, chunk: (i32, i32)) -> Self {
        let corner = vec2(chunk.0 as f32, chunk.1 as f32) * CHUNK;
        Self {
            zoom,
            origin: -corner,
            lo: corner / zoom - Vec2::splat(REACH),
            hi: (corner + Vec2::splat(CHUNK)) / zoom + Vec2::splat(REACH),
        }
    }

    /// Logical pixels per tile.
    pub(super) fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Logical pixels for a world point.
    pub(super) fn to_screen(&self, world: Vec2) -> Vec2 {
        world * self.zoom + self.origin
    }

    /// The world rectangle `[lo, hi]` the view covers.
    pub(super) fn world_rect(&self) -> (Vec2, Vec2) {
        (self.lo, self.hi)
    }

    /// The map tiles the view covers, `[min, max)`.
    pub(super) fn tiles(&self, map: &Map) -> (TilePos, TilePos) {
        (
            TilePos::new(
                numeric::to_i32(self.lo.x.floor()).max(0),
                numeric::to_i32(self.lo.y.floor()).max(0),
            ),
            TilePos::new(
                numeric::to_i32(self.hi.x.ceil()).min(map.width()),
                numeric::to_i32(self.hi.y.ceil()).min(map.height()),
            ),
        )
    }
}

/// What the ground passes draw from: the map's terrain and the layouts built
/// from it, none of which changes during a match.
#[derive(Clone, Copy)]
pub(super) struct Ground<'a> {
    pub(super) map: &'a Map,
    pub(super) scenario: &'a Scenario,
    pub(super) layout: &'a GroundLayout,
}

impl<'a> Ground<'a> {
    pub(super) fn of(game: &crate::game::Scene<'a>) -> Self {
        Self {
            map: game.state.map(),
            scenario: game.scenario,
            layout: game.ground(),
        }
    }
}

/// The two baked layers, in draw order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Layer {
    /// Ground, washes, grit, dressing and level steps: under salvage.
    Floor,
    /// Blockers, pit drops, slabs and terraces: over salvage, under frames.
    Raised,
}

impl Layer {
    /// Draws this layer's passes into `view`.
    fn draw(self, ground: &Ground<'_>, view: &GroundView, sprites: &Sprites) {
        match self {
            Layer::Floor => world::draw_floor(ground, view, sprites),
            Layer::Raised => {
                let fractured = sprites.quarry_dressing(0).is_some();
                let layout = ground.layout;
                world::draw_blockers(view, sprites, &layout.blockers);
                pits::draw_pits(ground.map, view, &layout.pits, fractured);
                boulders::draw_boulders(view, sprites, &layout.slabs);
                terraces::draw_terraces(ground.map, view, &layout.terraces, fractured);
            }
        }
    }

    /// The world rectangle this layer can draw in.
    fn extent(self, map: &Map) -> (Vec2, Vec2) {
        let size = vec2(map.width() as f32, map.height() as f32);
        match self {
            Layer::Floor => (Vec2::ZERO, size),
            Layer::Raised => {
                let margin = Vec2::splat(terraces::MARGIN as f32);
                (-margin, size + margin)
            }
        }
    }
}

type ChunkKey = (Layer, i32, i32);

/// The chunks of a layer's `extent` (world) that a screen of `viewport`
/// logical pixels shows through `view`.
fn chunks_in_view(
    view: &GroundView,
    viewport: Vec2,
    extent: (Vec2, Vec2),
) -> impl Iterator<Item = (i32, i32)> {
    // In logical pixels from world (0, 0).
    let lo = (-view.origin).max(extent.0 * view.zoom);
    let hi = (viewport - view.origin).min(extent.1 * view.zoom);
    let first = (lo / CHUNK).floor();
    let last = (hi / CHUNK).ceil();
    let (x0, y0) = (numeric::to_i32(first.x), numeric::to_i32(first.y));
    let (x1, y1) = (numeric::to_i32(last.x), numeric::to_i32(last.y));
    (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
}

/// Whether nothing raised can reach into the chunk `view` bakes: every tile
/// that can is open floor inside the map.
fn raised_is_empty(map: &Map, view: &GroundView) -> bool {
    let (lo, hi) = view.world_rect();
    let reach = Vec2::splat(boulders::DRAW_PAD as f32);
    let (lo, hi) = (lo - reach, hi + reach);
    let (x0, y0) = (numeric::to_i32(lo.x.floor()), numeric::to_i32(lo.y.floor()));
    let (x1, y1) = (numeric::to_i32(hi.x.ceil()), numeric::to_i32(hi.y.ceil()));
    (y0..y1).all(|y| {
        (x0..x1).all(|x| {
            map.tile(TilePos::new(x, y))
                .is_some_and(|tile| matches!(tile.terrain, Terrain::Ground | Terrain::Cliff))
        })
    })
}

/// The `count` least recently used of `chunks`, oldest first.
fn stalest(chunks: &BTreeMap<ChunkKey, Chunk>, count: usize) -> Vec<ChunkKey> {
    let mut by_use: Vec<(u64, ChunkKey)> = chunks.iter().map(|(&key, c)| (c.used, key)).collect();
    by_use.sort_unstable();
    by_use.into_iter().take(count).map(|(_, key)| key).collect()
}

struct Chunk {
    /// `None` when nothing in the layer reaches the chunk.
    target: Option<RenderTarget>,
    /// The frame that last drew it.
    used: u64,
}

/// The shaders the cache bakes and composites with. They live as long as the
/// sprites: macroquad deletes a material's pipeline as soon as it drops, even
/// under draws still queued for the frame.
pub(crate) struct GroundBlend {
    /// Draws the passes into a chunk as premultiplied color.
    bake: Material,
    /// Draws a chunk onto the frame.
    composite: Material,
}

/// The baked ground, kept by the presentation for its match. Created empty;
/// the first settled frame makes its textures, so headless sessions never
/// touch the GPU.
#[derive(Default)]
pub(crate) struct GroundCache {
    /// The zoom the chunks hold, and the display scale their textures fit.
    scale: Option<(f32, f32)>,
    chunks: BTreeMap<ChunkKey, Chunk>,
    /// Textures of dropped chunks, reused by later bakes at the same display
    /// scale.
    spare: Vec<RenderTarget>,
    frame: u64,
    /// The chunks this frame draws, when the cache covers the view.
    showing: Option<Vec<ChunkKey>>,
}

impl GroundCache {
    /// Readies the chunks this frame shows, baking what is missing within the
    /// frame's allowance. Without them all, or while the zoom glides, the
    /// frame draws its ground straight to the screen.
    pub(super) fn prepare(
        &mut self,
        ground: &Ground<'_>,
        view: &GroundView,
        camera: &crate::camera::Camera,
        sprites: &Sprites,
    ) {
        self.showing = None;
        if camera.zooming() {
            return;
        }
        let dpi = screen_dpi_scale();
        if self.scale != Some((view.zoom, dpi)) {
            let same_size = self.scale.is_some_and(|(_, old)| old == dpi);
            let retired = std::mem::take(&mut self.chunks);
            if same_size {
                self.spare
                    .extend(retired.into_values().filter_map(|chunk| chunk.target));
            } else {
                self.spare.clear();
            }
            self.scale = Some((view.zoom, dpi));
        }
        self.frame += 1;
        let viewport = camera.viewport();
        let wanted: Vec<ChunkKey> = [Layer::Floor, Layer::Raised]
            .into_iter()
            .flat_map(|layer| {
                chunks_in_view(view, viewport, layer.extent(ground.map))
                    .map(move |(x, y)| (layer, x, y))
            })
            .collect();
        let missing: Vec<ChunkKey> = wanted
            .iter()
            .copied()
            .filter(|key| !self.chunks.contains_key(key))
            .collect();
        let allowance = missing.len().min(BAKES_PER_FRAME);
        self.bake(ground, sprites, dpi, &missing[..allowance]);
        for key in &wanted {
            if let Some(chunk) = self.chunks.get_mut(key) {
                chunk.used = self.frame;
            }
        }
        // Keep half again the chunks on show, so panning back finds them.
        let keep = wanted.len() + wanted.len() / 2;
        if self.chunks.len() > keep {
            for key in stalest(&self.chunks, self.chunks.len() - keep) {
                if let Some(target) = self.chunks.remove(&key).and_then(|chunk| chunk.target) {
                    self.spare.push(target);
                }
            }
        }
        let held = self.chunks.values().filter(|c| c.target.is_some()).count();
        self.spare.truncate(keep.saturating_sub(held));
        if missing.len() <= BAKES_PER_FRAME {
            self.showing = Some(wanted);
        }
    }

    /// Draws `layer` for this frame: from its chunks when [`Self::prepare`]
    /// readied them all, otherwise straight to the screen.
    pub(super) fn draw(
        &self,
        layer: Layer,
        ground: &Ground<'_>,
        view: &GroundView,
        sprites: &Sprites,
    ) {
        let Some(showing) = &self.showing else {
            layer.draw(ground, view, sprites);
            return;
        };
        gl_use_material(&sprites.ground_blend().composite);
        for key in showing.iter().filter(|key| key.0 == layer) {
            let Some(target) = self.chunks.get(key).and_then(|chunk| chunk.target.as_ref()) else {
                continue;
            };
            let corner = view.origin + vec2(key.1 as f32, key.2 as f32) * CHUNK;
            draw_texture_ex(
                &target.texture,
                corner.x,
                corner.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(Vec2::splat(CHUNK)),
                    ..Default::default()
                },
            );
        }
        gl_use_default_material();
    }

    fn bake(&mut self, ground: &Ground<'_>, sprites: &Sprites, dpi: f32, keys: &[ChunkKey]) {
        let Some((zoom, _)) = self.scale else {
            return;
        };
        if keys.is_empty() {
            return;
        }
        let side = numeric::to_u32((CHUNK * dpi).ceil());
        push_camera_state();
        gl_use_material(&sprites.ground_blend().bake);
        for &(layer, x, y) in keys {
            let view = GroundView::chunk(zoom, (x, y));
            let target = if layer == Layer::Raised && raised_is_empty(ground.map, &view) {
                None
            } else {
                let target = self.spare.pop().unwrap_or_else(|| {
                    let target = render_target(side, side);
                    target.texture.set_filter(FilterMode::Nearest);
                    target
                });
                set_camera(&Camera2D {
                    target: Vec2::splat(CHUNK * 0.5),
                    zoom: Vec2::splat(2.0 / CHUNK),
                    render_target: Some(target.clone()),
                    ..Default::default()
                });
                clear_background(BLANK);
                layer.draw(ground, &view, sprites);
                Some(target)
            };
            self.chunks.insert(
                (layer, x, y),
                Chunk {
                    target,
                    used: self.frame,
                },
            );
        }
        gl_use_default_material();
        pop_camera_state();
    }
}

impl GroundBlend {
    pub(crate) fn load() -> anyhow::Result<Self> {
        use anyhow::Context;
        use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
        // macroquad's own sprite shader, which every pass draws with on screen.
        const VERTEX: &str = r"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main(){ gl_Position=Projection*Model*vec4(position,1.0); color=color0/255.0; uv=texcoord; }
";
        let premultiplied = || MaterialParams {
            pipeline_params: PipelineParams {
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::One,
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            },
            ..Default::default()
        };
        let bake = load_material(
            ShaderSource::Glsl {
                vertex: VERTEX,
                fragment: r"#version 100
precision mediump float;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
void main(){ vec4 pixel=color*texture2D(Texture,uv); gl_FragColor=vec4(pixel.rgb*pixel.a,pixel.a); }
",
            },
            premultiplied(),
        )
        .context("loading the terrain bake shader")?;
        let composite = load_material(
            ShaderSource::Glsl {
                vertex: VERTEX,
                fragment: r"#version 100
precision mediump float;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
void main(){ gl_FragColor=color*texture2D(Texture,uv); }
",
            },
            premultiplied(),
        )
        .context("loading the terrain composite shader")?;
        Ok(Self { bake, composite })
    }
}

#[cfg(test)]
mod tests;
