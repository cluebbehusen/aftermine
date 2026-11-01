//! The fixed ground: the terrain layers that read the map alone. Terrain never
//! changes during a match, so these layers are baked into textures a chunk at a
//! time and the frame draws the textures.
//!
//! A zoom never redraws the ground. It stretches the chunks it started from, over
//! a low-resolution image of the whole map for ground they do not hold, while
//! fresh chunks bake for where it will land, and those go on show as soon as they
//! are ready, with the view still moving to hide the change. Images cannot move
//! against themselves, so nothing in the ground steps or shakes.
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
/// Most fresh chunks baked in one frame while stretched ones stay on show. The
/// frame is not waiting on them, so the work can spread out.
const BACKGROUND_BAKES_PER_FRAME: usize = 4;
/// Tiles past a chunk's edge whose art can still reach into it: dressing,
/// pit relief and wear marks overhang their tile by less than one. Slabs reach
/// further and pad their own window.
const REACH: f32 = 1.0;

/// Where the ground passes draw, in logical pixels: the screen, or one chunk
/// of the cache. While the zoom holds still, world `(0, 0)` lands on a whole
/// pixel, so ground drawn straight to the screen floors to the same pixels as a
/// chunk baked at the same zoom and moved into place.
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
    /// The camera's view of the ground once its zoom glide comes to rest.
    fn landing(camera: &crate::camera::Camera) -> Self {
        let (zoom, origin) = camera.landing();
        Self {
            zoom,
            origin,
            lo: -origin / zoom,
            hi: (camera.viewport() - origin) / zoom,
        }
    }

    /// The camera's view of the ground this frame.
    pub(super) fn live(camera: &crate::camera::Camera) -> Self {
        let (lo, hi) = camera.world_rect();
        let origin = camera.to_screen(Vec2::ZERO);
        Self {
            zoom: camera.zoom,
            // A gliding view stays exact. Rounded, the ground hops a whole pixel
            // at a time against a slower true motion, so sharp art like rocks
            // jumps together and drifts back as the zoom creeps to rest.
            origin: if camera.zooming() {
                origin
            } else {
                origin.round()
            },
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
    pub(super) grade: super::grade::Grade,
}

impl<'a> Ground<'a> {
    pub(super) fn of(game: &crate::game::Scene<'a>) -> Self {
        Self {
            map: game.state.map(),
            scenario: game.scenario,
            layout: game.ground(),
            grade: super::grade::Grade::of(
                game.scenario.meta.as_ref().map_or("", |m| m.theme.as_str()),
            ),
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
                let grade = ground.grade;
                world::draw_blockers(view, sprites, &layout.blockers, grade);
                pits::draw_pits(ground.map, view, &layout.pits, fractured, grade);
                boulders::draw_boulders(view, sprites, &layout.slabs, grade);
                terraces::draw_terraces(ground.map, view, &layout.terraces, fractured, grade);
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
/// logical pixels shows through `view`, with a chunk `side` logical pixels
/// across.
fn chunks_in_view(
    view: &GroundView,
    viewport: Vec2,
    extent: (Vec2, Vec2),
    side: f32,
) -> impl Iterator<Item = (i32, i32)> {
    // In logical pixels from world (0, 0).
    let lo = (-view.origin).max(extent.0 * view.zoom);
    let hi = (viewport - view.origin).min(extent.1 * view.zoom);
    let first = (lo / side).floor();
    let last = (hi / side).ceil();
    let (x0, y0) = (numeric::to_i32(first.x), numeric::to_i32(first.y));
    let (x1, y1) = (numeric::to_i32(last.x), numeric::to_i32(last.y));
    (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
}

/// The chunks of both layers that `view` shows.
fn chunks_of_view(map: &Map, view: &GroundView, viewport: Vec2, side: f32) -> Vec<ChunkKey> {
    [Layer::Floor, Layer::Raised]
        .into_iter()
        .flat_map(|layer| {
            chunks_in_view(view, viewport, layer.extent(map), side).map(move |(x, y)| (layer, x, y))
        })
        .collect()
}

/// The screen rectangle `[lo, hi)` of chunk `(x, y)`. Neighbors compute their
/// shared edge identically, so stretched chunks leave no seams, and it stays
/// exact so no chunk steps against the next.
fn chunk_on_screen(view: &GroundView, (x, y): (i32, i32), side: f32) -> (Vec2, Vec2) {
    let corner = |x: i32, y: i32| view.origin + vec2(x as f32, y as f32) * side;
    (corner(x, y), corner(x + 1, y + 1))
}

/// The parts of the rectangle `whole` outside `covered`.
fn parts_outside(whole: (Vec2, Vec2), covered: (Vec2, Vec2)) -> Vec<(Vec2, Vec2)> {
    let (lo, hi) = whole;
    let (c_lo, c_hi) = covered;
    if c_hi.x <= lo.x || c_hi.y <= lo.y || c_lo.x >= hi.x || c_lo.y >= hi.y {
        return vec![whole];
    }
    let (top, bottom) = (c_lo.y.max(lo.y), c_hi.y.min(hi.y));
    [
        (lo, vec2(hi.x, top)),
        (vec2(lo.x, bottom), hi),
        (vec2(lo.x, top), vec2(c_lo.x.min(hi.x), bottom)),
        (vec2(c_hi.x.max(lo.x), top), vec2(hi.x, bottom)),
    ]
    .into_iter()
    .filter(|(a, b)| a.x < b.x && a.y < b.y)
    .collect()
}

/// The parts of the rectangle `whole` that none of `covered` reach. Images on
/// show never overlap, so their translucent edges blend once, as they baked.
fn uncovered_parts(whole: (Vec2, Vec2), covered: &[(Vec2, Vec2)]) -> Vec<(Vec2, Vec2)> {
    covered.iter().fold(vec![whole], |parts, &rect| {
        parts
            .into_iter()
            .flat_map(|part| parts_outside(part, rect))
            .collect()
    })
}

/// Whether fresh chunks of on-screen size `fresh` look sharper than chunks of
/// size `chunks`: whichever is resampled least.
fn fresh_goes_on_top(chunks: f32, fresh: f32) -> bool {
    (fresh / CHUNK).ln().abs() <= (chunks / CHUNK).ln().abs()
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

/// A set of chunks on screen.
struct ChunkGrid {
    keys: Vec<ChunkKey>,
    /// Logical pixels a chunk covers on screen.
    side: f32,
}

/// The chunks a frame draws in place of its ground.
struct Showing {
    chunks: ChunkGrid,
    /// Fresh chunks for where the zoom will land, once they are all ready.
    fresh: Option<ChunkGrid>,
    fresh_on_top: bool,
    /// Whether the whole-map image fills in what the chunks do not cover.
    overview: bool,
}

impl Showing {
    fn settled(keys: Vec<ChunkKey>) -> Self {
        Self {
            chunks: ChunkGrid { keys, side: CHUNK },
            fresh: None,
            fresh_on_top: false,
            overview: false,
        }
    }
}

/// The whole map baked small, one image per layer, for ground a zoom out reveals
/// before fresh chunks are ready.
struct Overview {
    dpi: f32,
    floor: RenderTarget,
    raised: RenderTarget,
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
    showing: Option<Showing>,
    /// Fresh chunks baking behind the ones on show, for the zoom in `next_zoom`.
    next: BTreeMap<ChunkKey, Chunk>,
    next_zoom: Option<f32>,
    /// Whether `next` holds all it needs.
    next_ready: bool,
    /// The zoom a glide was headed for last frame. While it keeps changing,
    /// fresh chunks would be stale before they finish.
    last_target: Option<f32>,
    /// Whether `chunks` sample smoothly, for stretching. Held still at their
    /// own zoom they sample exactly, which stays sharp at fractional DPI.
    smooth: bool,
    overview: Option<Box<Overview>>,
}

impl GroundCache {
    /// Readies the chunks this frame shows, baking what is missing within the
    /// frame's allowance. While the zoom moves, or has landed past the chunks,
    /// those stretch to fit while fresh ones bake. Before any chunks exist, the
    /// frame draws its ground straight to the screen.
    pub(super) fn prepare(
        &mut self,
        ground: &Ground<'_>,
        view: &GroundView,
        camera: &crate::camera::Camera,
        sprites: &Sprites,
    ) {
        let dpi = screen_dpi_scale();
        let viewport = camera.viewport();
        self.showing = None;
        if self.overview.as_ref().is_none_or(|map| map.dpi != dpi) {
            self.overview = Some(Box::new(bake_overview(ground, sprites, dpi)));
        }
        let moving = camera.zooming();
        let same_dpi = self.scale.is_some_and(|(_, baked)| baked == dpi);
        if moving && !same_dpi {
            return;
        }
        if same_dpi && (moving || self.scale != Some((view.zoom, dpi))) {
            self.showing = Some(self.show_between(ground, view, camera, sprites, dpi));
            return;
        }
        self.last_target = None;
        self.recycle_next();
        self.set_smooth(false);
        if self.scale != Some((view.zoom, dpi)) {
            self.spare.clear();
            self.chunks.clear();
            self.scale = Some((view.zoom, dpi));
        }
        self.frame += 1;
        let wanted = chunks_of_view(ground.map, view, viewport, CHUNK);
        let missing: Vec<ChunkKey> = wanted
            .iter()
            .copied()
            .filter(|key| !self.chunks.contains_key(key))
            .collect();
        let allowance = missing.len().min(BAKES_PER_FRAME);
        let baked = self.bake(
            ground,
            sprites,
            dpi,
            view.zoom,
            FilterMode::Nearest,
            &missing[..allowance],
        );
        self.chunks.extend(baked);
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
        // Enough to bake a whole fresh view behind the one on show.
        self.spare.truncate(wanted.len());
        if missing.len() <= BAKES_PER_FRAME {
            self.showing = Some(Showing::settled(wanted));
        }
    }

    /// What a frame shows while the zoom moves, or has landed past the chunks,
    /// baking ahead for where it will land.
    fn show_between(
        &mut self,
        ground: &Ground<'_>,
        view: &GroundView,
        camera: &crate::camera::Camera,
        sprites: &Sprites,
        dpi: f32,
    ) -> Showing {
        let viewport = camera.viewport();
        let moving = camera.zooming();
        let landing = GroundView::landing(camera);
        let held = !moving || self.last_target == Some(landing.zoom);
        self.last_target = moving.then_some(landing.zoom);
        // A target on the move leaves ready chunks behind, yet they are still
        // closer to the view than the ones on show.
        if !held
            && self.next_ready
            && let Some(zoom) = self.next_zoom
        {
            self.promote_next(zoom, dpi);
        }
        self.set_smooth(true);
        let mut fresh = None;
        if self.scale.is_some_and(|(zoom, _)| zoom == landing.zoom) {
            self.recycle_next();
        } else if held && let Some(keys) = self.bake_next(ground, &landing, viewport, sprites, dpi)
        {
            if !moving {
                self.promote_next(view.zoom, dpi);
                return Showing::settled(keys);
            }
            fresh = Some(ChunkGrid {
                keys,
                side: CHUNK * view.zoom / landing.zoom,
            });
        }
        let chunks = self.on_hand(ground.map, view, viewport);
        let fresh_on_top = fresh
            .as_ref()
            .is_some_and(|grid| fresh_goes_on_top(chunks.side, grid.side));
        Showing {
            chunks,
            fresh,
            fresh_on_top,
            overview: true,
        }
    }

    /// The chunks on hand that `view` shows, stretched to fit it.
    fn on_hand(&self, map: &Map, view: &GroundView, viewport: Vec2) -> ChunkGrid {
        let baked = self.scale.map_or(view.zoom, |(zoom, _)| zoom);
        let side = CHUNK * view.zoom / baked;
        let keys = chunks_of_view(map, view, viewport, side)
            .into_iter()
            .filter(|key| self.chunks.contains_key(key))
            .collect();
        ChunkGrid { keys, side }
    }

    /// Bakes this frame's share of the fresh chunks for `view`, and returns the
    /// ones it shows once they are all ready.
    fn bake_next(
        &mut self,
        ground: &Ground<'_>,
        view: &GroundView,
        viewport: Vec2,
        sprites: &Sprites,
        dpi: f32,
    ) -> Option<Vec<ChunkKey>> {
        if self.next_zoom != Some(view.zoom) {
            self.recycle_next();
            self.next_zoom = Some(view.zoom);
        }
        self.frame += 1;
        let wanted = chunks_of_view(ground.map, view, viewport, CHUNK);
        let missing: Vec<ChunkKey> = wanted
            .iter()
            .copied()
            .filter(|key| !self.next.contains_key(key))
            .collect();
        let allowance = missing.len().min(BACKGROUND_BAKES_PER_FRAME);
        let baked = self.bake(
            ground,
            sprites,
            dpi,
            view.zoom,
            FilterMode::Linear,
            &missing[..allowance],
        );
        self.next.extend(baked);
        self.next_ready = missing.len() <= allowance;
        self.next_ready.then_some(wanted)
    }

    /// Puts the fresh chunks on show and frees the ones they replace.
    fn promote_next(&mut self, zoom: f32, dpi: f32) {
        let retired = std::mem::replace(&mut self.chunks, std::mem::take(&mut self.next));
        self.spare
            .extend(retired.into_values().filter_map(|chunk| chunk.target));
        self.scale = Some((zoom, dpi));
        self.next_zoom = None;
        self.next_ready = false;
        // Baked to stretch; they now hold still at their own zoom.
        self.smooth = true;
        self.set_smooth(false);
    }

    /// Abandons any fresh chunks, keeping their textures for later bakes.
    fn recycle_next(&mut self) {
        self.next_zoom = None;
        self.next_ready = false;
        self.spare.extend(
            std::mem::take(&mut self.next)
                .into_values()
                .filter_map(|chunk| chunk.target),
        );
    }

    fn set_smooth(&mut self, smooth: bool) {
        if self.smooth == smooth {
            return;
        }
        self.smooth = smooth;
        let filter = if smooth {
            FilterMode::Linear
        } else {
            FilterMode::Nearest
        };
        for target in self
            .chunks
            .values()
            .filter_map(|chunk| chunk.target.as_ref())
        {
            target.texture.set_filter(filter);
        }
    }

    /// Draws `layer` for this frame: from the images [`Self::prepare`] readied,
    /// otherwise straight to the screen.
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
        // Top of the stack first, each image clipped to what is still bare.
        let mut covered = Vec::new();
        let chunks = Some((&showing.chunks, &self.chunks));
        let fresh = showing.fresh.as_ref().map(|grid| (grid, &self.next));
        let stack = if showing.fresh_on_top {
            [fresh, chunks]
        } else {
            [chunks, fresh]
        };
        for (grid, held) in stack.into_iter().flatten() {
            let mut rects = Vec::with_capacity(grid.keys.len());
            for key in grid.keys.iter().filter(|key| key.0 == layer) {
                let whole = chunk_on_screen(view, (key.1, key.2), grid.side);
                if let Some(target) = held.get(key).and_then(|chunk| chunk.target.as_ref()) {
                    for part in uncovered_parts(whole, &covered) {
                        draw_chunk_part(target, whole, part);
                    }
                }
                rects.push(whole);
            }
            covered.extend(rects);
        }
        if showing.overview
            && let Some(overview) = &self.overview
        {
            let (lo, hi) = layer.extent(ground.map);
            let whole = (view.to_screen(lo), view.to_screen(hi));
            let target = match layer {
                Layer::Floor => &overview.floor,
                Layer::Raised => &overview.raised,
            };
            for part in uncovered_parts(whole, &covered) {
                draw_chunk_part(target, whole, part);
            }
        }
        gl_use_default_material();
    }

    /// Bakes `keys` at `zoom` into textures of the display scale `dpi`.
    fn bake(
        &mut self,
        ground: &Ground<'_>,
        sprites: &Sprites,
        dpi: f32,
        zoom: f32,
        filter: FilterMode,
        keys: &[ChunkKey],
    ) -> Vec<(ChunkKey, Chunk)> {
        if keys.is_empty() {
            return Vec::new();
        }
        let side = numeric::to_u32((CHUNK * dpi).ceil());
        let mut baked = Vec::with_capacity(keys.len());
        push_camera_state();
        gl_use_material(&sprites.ground_blend().bake);
        for &(layer, x, y) in keys {
            let view = GroundView::chunk(zoom, (x, y));
            let target = if layer == Layer::Raised && raised_is_empty(ground.map, &view) {
                None
            } else {
                let target = self
                    .spare
                    .pop()
                    .unwrap_or_else(|| render_target(side, side));
                target.texture.set_filter(filter);
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
            baked.push((
                (layer, x, y),
                Chunk {
                    target,
                    used: self.frame,
                },
            ));
        }
        gl_use_default_material();
        pop_camera_state();
        baked
    }
}

/// Bakes the whole map once, at the widest zoom a view can reach.
fn bake_overview(ground: &Ground<'_>, sprites: &Sprites, dpi: f32) -> Overview {
    push_camera_state();
    gl_use_material(&sprites.ground_blend().bake);
    let [floor, raised] = [Layer::Floor, Layer::Raised].map(|layer| {
        let (lo, hi) = layer.extent(ground.map);
        let zoom = crate::camera::ZOOM_MIN;
        let size = (hi - lo) * zoom;
        let target = render_target(
            numeric::to_u32((size.x * dpi).ceil()),
            numeric::to_u32((size.y * dpi).ceil()),
        );
        target.texture.set_filter(FilterMode::Linear);
        set_camera(&Camera2D {
            target: size * 0.5,
            zoom: vec2(2.0 / size.x, 2.0 / size.y),
            render_target: Some(target.clone()),
            ..Default::default()
        });
        clear_background(BLANK);
        let view = GroundView {
            zoom,
            origin: -lo * zoom,
            lo,
            hi,
        };
        layer.draw(ground, &view, sprites);
        target
    });
    gl_use_default_material();
    pop_camera_state();
    Overview { dpi, floor, raised }
}

/// Draws the `part` of a chunk that lands on `whole`. A mesh, not a sized draw:
/// its corners are exactly the shared edges, where a position plus a size can
/// miss by a rounding error.
fn draw_chunk_part(target: &RenderTarget, whole: (Vec2, Vec2), part: (Vec2, Vec2)) {
    let span = whole.1 - whole.0;
    let vertex = |x: f32, y: f32| {
        let uv = (vec2(x, y) - whole.0) / span;
        Vertex::new(x, y, 0.0, uv.x, uv.y, WHITE)
    };
    let (lo, hi) = part;
    draw_mesh(&Mesh {
        vertices: vec![
            vertex(lo.x, lo.y),
            vertex(hi.x, lo.y),
            vertex(hi.x, hi.y),
            vertex(lo.x, hi.y),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: Some(target.texture.clone()),
    });
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
