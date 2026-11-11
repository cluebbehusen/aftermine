//! Ground layers: terrain tiles, the fog veil over them, and battle scars.

use super::ground::{Ground, GroundView};
use super::*;
use crate::numeric;
use crate::numeric::Fit;
use chassis::grid::as_index;

/// Share of a tile's width over which the veil fades into lighter ground.
const FOG_FEATHER: f32 = 0.45;

/// Fog of war from the local player's perspective: unexplored is void,
/// explored-but-unseen is dimmed.
pub(super) fn draw_fog(game: &crate::game::Scene<'_>, view: &GroundView) {
    game.presentation.fog_veil.borrow_mut().draw(game, view);
}

/// Most distinct feathers kept before the memo starts over. A frame's fog
/// edge holds a few dozen; this bound only stops a long zoom session from
/// collecting every step count of every pattern.
const FEATHER_MEMO: usize = 4096;

/// Most a veil mesh holds: well inside one macroquad draw call, whose
/// `geometry` clamps anything larger.
const MESH_VERTICES: usize = 4096;
const MESH_INDICES: usize = 6144;

/// The most grid steps a feather band takes.
const MAX_STEPS: usize = 8;

/// What a scan of the fog read: fog changes only with a tick, and only the
/// viewer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scanned {
    tick: aftermine_sim::Tick,
    viewer: aftermine_sim::PlayerId,
    /// The tiles scanned, `[lo, hi)`.
    lo: TilePos,
    hi: TilePos,
}

/// A row's run of equal fog that covers its ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    y: i32,
    /// The run's tiles, `[from, to)`.
    from: i32,
    to: i32,
    fog: Fog,
}

/// A known tile beside more fogged ground, which feathers into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edge {
    tile: TilePos,
    neighborhood: [[Fog; 3]; 3],
}

/// The fog veil's working memory, owned by a presentation. Its runs and
/// feathered tiles are kept in tile space until the tick, viewer or tiles in
/// view change, and its meshes in screen space until the view moves. Only
/// the map and the boundary fog's margin can hold anything but unexplored
/// ground, so the scan stops there and the rest of the view is covered by a
/// few opaque rects. A tile's feather depends only on its fog neighborhood
/// and its grid size, and the same few neighborhoods recur all along a fog
/// edge, so their alphas are kept too.
pub(crate) struct FogVeil {
    scanned: Option<Scanned>,
    /// Counts scans, so a projection knows which one it drew.
    scans: u64,
    states: Vec<Fog>,
    runs: Vec<Run>,
    edges: Vec<Edge>,
    projected: Option<(u64, GroundView)>,
    feathers: std::collections::HashMap<u32, Box<[f32]>>,
    /// Each step count's triangle list over its `side`×`side` vertex grid.
    indices: [Vec<u16>; MAX_STEPS],
    meshes: Vec<Mesh>,
    /// The meshes the projection fills, in drawing order.
    filled: usize,
}

impl Default for FogVeil {
    fn default() -> Self {
        Self {
            scanned: None,
            scans: 0,
            states: Vec::new(),
            runs: Vec::new(),
            edges: Vec::new(),
            projected: None,
            feathers: std::collections::HashMap::new(),
            indices: std::array::from_fn(|_| Vec::new()),
            meshes: Vec::new(),
            filled: 0,
        }
    }
}

impl FogVeil {
    /// Drops the scan, for a world replaced at a tick it may have scanned.
    pub(crate) fn forget(&mut self) {
        self.scanned = None;
    }

    fn draw(&mut self, game: &crate::game::Scene<'_>, view: &GroundView) {
        let (min, max) = view_tiles(view);
        let map = game.state.map();
        let reach = crate::boundary_fog::PAD;
        let scanned = Scanned {
            tick: game.state.current_tick(),
            viewer: game.presentation.human,
            lo: TilePos::new(min.x.max(-reach), min.y.max(-reach)),
            hi: TilePos::new(
                max.x.min(map.width() + reach),
                max.y.min(map.height() + reach),
            ),
        };
        self.scan(scanned, |tile| tile_fog(game, tile));
        self.project(view);
        for mesh in &self.meshes[..self.filled] {
            draw_mesh(mesh);
        }
    }

    /// Reads the fog over `scanned`'s tiles unless they are what the last
    /// scan read.
    fn scan(&mut self, scanned: Scanned, fog: impl Fn(TilePos) -> Fog) {
        if self.scanned == Some(scanned) {
            return;
        }
        self.scanned = Some(scanned);
        self.scans += 1;
        self.runs.clear();
        self.edges.clear();
        let Scanned { lo, hi, .. } = scanned;
        if lo.x >= hi.x || lo.y >= hi.y {
            return;
        }
        // One extra ring so every scanned tile sees all eight neighbors.
        let stride = as_index(hi.x - lo.x + 2);
        self.states.clear();
        let fog = &fog;
        self.states.extend(
            (lo.y - 1..=hi.y).flat_map(|y| (lo.x - 1..=hi.x).map(move |x| fog(TilePos::new(x, y)))),
        );
        let states = &self.states;
        let at = |x: i32, y: i32| states[as_index(y - lo.y + 1) * stride + as_index(x - lo.x + 1)];
        // One run per stretch of equal fog along a row: zoomed out, the view
        // holds thousands of tiles.
        for y in lo.y..hi.y {
            let mut x = lo.x;
            while x < hi.x {
                let fog = at(x, y);
                let from = x;
                while x < hi.x && at(x, y) == fog {
                    x += 1;
                }
                if fog.alpha() != 0.0 {
                    self.runs.push(Run {
                        y,
                        from,
                        to: x,
                        fog,
                    });
                }
            }
        }
        // Feather inward into known ground. Unknown tiles retain their opaque veil.
        for y in lo.y..hi.y {
            for x in lo.x..hi.x {
                let current = at(x, y);
                if current == Fog::Unexplored {
                    continue;
                }
                let neighborhood: [[Fog; 3]; 3] = std::array::from_fn(|row| {
                    std::array::from_fn(|col| {
                        at(x + col.fit::<i32>() - 1, y + row.fit::<i32>() - 1)
                    })
                });
                if neighborhood.iter().flatten().all(|&state| state <= current) {
                    continue;
                }
                self.edges.push(Edge {
                    tile: TilePos::new(x, y),
                    neighborhood,
                });
            }
        }
    }

    /// Lays the scan out on screen unless `view` and the scan are what the
    /// meshes already show.
    fn project(&mut self, view: &GroundView) {
        if self
            .projected
            .as_ref()
            .is_some_and(|(scans, shown)| *scans == self.scans && shown == view)
        {
            return;
        }
        self.projected = Some((self.scans, view.clone()));
        self.filled = 0;
        let (min, max) = view_tiles(view);
        let unexplored = Fog::Unexplored.cover();
        // Exact shared edges: translucent rects that overlap draw double-dark
        // seams, so each rect ends where the next begins.
        let screen = |x: i32, y: i32| view.to_screen(vec2(x as f32, y as f32)).floor();
        let rect = |veil: &mut Self, from: TilePos, to: TilePos, color: Color| {
            if from.x < to.x && from.y < to.y {
                veil.quad(screen(from.x, from.y), screen(to.x, to.y), color);
            }
        };
        for (from, to) in beyond(min, max, self.scanned) {
            rect(self, from, to, unexplored);
        }
        for index in 0..self.runs.len() {
            let Run { y, from, to, fog } = self.runs[index];
            rect(
                self,
                TilePos::new(from, y),
                TilePos::new(to, y + 1),
                fog.cover(),
            );
        }
        for index in 0..self.edges.len() {
            let Edge { tile, neighborhood } = self.edges[index];
            let a = screen(tile.x, tile.y);
            let b = screen(tile.x + 1, tile.y + 1);
            self.feather(a, b, &neighborhood);
        }
    }

    /// A mesh with room for `vertices` and `indices` more, after those
    /// already filled.
    fn room(&mut self, vertices: usize, indices: usize) -> &mut Mesh {
        let fits = self.filled > 0 && {
            let mesh = &self.meshes[self.filled - 1];
            mesh.vertices.len() + vertices <= MESH_VERTICES
                && mesh.indices.len() + indices <= MESH_INDICES
        };
        if !fits {
            if self.filled == self.meshes.len() {
                self.meshes.push(Mesh {
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    texture: None,
                });
            }
            let mesh = &mut self.meshes[self.filled];
            mesh.vertices.clear();
            mesh.indices.clear();
            self.filled += 1;
        }
        &mut self.meshes[self.filled - 1]
    }

    /// The quad `draw_rectangle` queues over the screen rect from `a` to `b`.
    fn quad(&mut self, a: Vec2, b: Vec2, color: Color) {
        let (x, y, w, h) = (a.x, a.y, b.x - a.x, b.y - a.y);
        let mesh = self.room(4, 6);
        let base = mesh.vertices.len().fit::<u16>();
        mesh.vertices.extend([
            Vertex::new(x, y, 0., 0.0, 0.0, color),
            Vertex::new(x + w, y, 0., 1.0, 0.0, color),
            Vertex::new(x + w, y + h, 0., 1.0, 1.0, color),
            Vertex::new(x, y + h, 0., 0.0, 1.0, color),
        ]);
        mesh.indices
            .extend([0, 1, 2, 0, 2, 3].map(|index| base + index));
    }

    /// Samples [`fog_feather_alpha`] on a grid over one tile's screen rect.
    fn feather(&mut self, a: Vec2, b: Vec2, neighborhood: &[[Fog; 3]; 3]) {
        let band = (b - a).max_element() * FOG_FEATHER;
        let steps = numeric::to_usize((band / 6.0).ceil().clamp(1.0, MAX_STEPS as f32));
        let side = 2 * (steps + 1);
        if self.indices[steps - 1].is_empty() {
            self.indices[steps - 1] = feather_indices(side);
        }
        if self.feathers.len() >= FEATHER_MEMO {
            self.feathers.clear();
        }
        let key = feather_key(neighborhood, steps);
        self.feathers
            .entry(key)
            .or_insert_with(|| feather_alphas(neighborhood, steps));
        let count = self.indices[steps - 1].len();
        self.room(side * side, count);
        let alphas = &self.feathers[&key];
        let indices = &self.indices[steps - 1];
        let mesh = &mut self.meshes[self.filled - 1];
        let base = mesh.vertices.len().fit::<u16>();
        mesh.vertices.extend((0..side).flat_map(|row| {
            (0..side).map(move |col| {
                let p = a + (b - a) * vec2(feather_coord(col, steps), feather_coord(row, steps));
                let alpha = alphas[row * side + col];
                let color = Color::new(FOG_UNEXPLORED.r, FOG_UNEXPLORED.g, FOG_UNEXPLORED.b, alpha);
                Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, color)
            })
        }));
        mesh.indices
            .extend(indices.iter().map(|index| base + index));
    }
}

/// The parts of the view's tiles `[min, max)` that `scanned` left out, each
/// `[from, to)`: unexplored ground, every one of them.
fn beyond(min: TilePos, max: TilePos, scanned: Option<Scanned>) -> [(TilePos, TilePos); 4] {
    let none = (min, min);
    match scanned {
        Some(Scanned { lo, hi, .. }) if lo.x < hi.x && lo.y < hi.y => [
            (min, TilePos::new(max.x, lo.y)),
            (TilePos::new(min.x, hi.y), max),
            (TilePos::new(min.x, lo.y), TilePos::new(lo.x, hi.y)),
            (TilePos::new(hi.x, lo.y), TilePos::new(max.x, hi.y)),
        ],
        _ => [(min, max), none, none, none],
    }
}

/// The tiles `view` covers, `[min, max)`, on the map or off it.
fn view_tiles(view: &GroundView) -> (TilePos, TilePos) {
    let (lo, hi) = view.world_rect();
    (
        numeric::tile_at(lo),
        TilePos::new(numeric::to_i32(hi.x.ceil()), numeric::to_i32(hi.y.ceil())),
    )
}

/// Where grid line `index` of a feather with `steps` steps per band sits
/// across a tile. Grid lines fall on the feather band boundaries so
/// straight fades interpolate exactly; only the rounded corners are
/// approximated.
fn feather_coord(index: usize, steps: usize) -> f32 {
    if index <= steps {
        FOG_FEATHER * index as f32 / steps as f32
    } else {
        1.0 - FOG_FEATHER + FOG_FEATHER * (index - steps - 1) as f32 / steps as f32
    }
}

/// A distinct key for each neighborhood and step count.
fn feather_key(neighborhood: &[[Fog; 3]; 3], steps: usize) -> u32 {
    let pattern = neighborhood
        .iter()
        .flatten()
        .fold(0, |code, &state| code * 3 + state as u32);
    pattern * 9 + steps.fit::<u32>()
}

/// A feather's alphas at its grid's vertices, row by row.
fn feather_alphas(neighborhood: &[[Fog; 3]; 3], steps: usize) -> Box<[f32]> {
    let side = 2 * (steps + 1);
    (0..side)
        .flat_map(|row| {
            (0..side).map(move |col| {
                fog_feather_alpha(
                    neighborhood,
                    feather_coord(col, steps),
                    feather_coord(row, steps),
                )
            })
        })
        .collect()
}

fn feather_indices(side: usize) -> Vec<u16> {
    (0..side - 1)
        .flat_map(|row| {
            (0..side - 1).flat_map(move |col| {
                let top = (row * side + col).fit::<u16>();
                let bottom = top + side.fit::<u16>();
                [top, top + 1, bottom + 1, top, bottom + 1, bottom]
            })
        })
        .collect()
}

/// Veil alpha layered over a known tile's own cover at tile-local
/// `(u, v)`. `neighborhood[row][col]` holds fog states with this tile at
/// the center.
///
/// The veil blends a fade toward hidden ground with a fade toward
/// unexplored ground. Neither depends on the drawing tile's own state, so
/// neighbors agree along every shared edge, including where visible,
/// explored, and unexplored ground meet.
fn fog_feather_alpha(neighborhood: &[[Fog; 3]; 3], u: f32, v: f32) -> f32 {
    let current = neighborhood[1][1].alpha();
    if current >= 1.0 {
        return 0.0;
    }
    let explored = Fog::Explored.alpha();
    let occlusion = explored * fog_reach(neighborhood, u, v, Fog::Explored)
        + (1.0 - explored) * fog_reach(neighborhood, u, v, Fog::Unexplored);
    ((occlusion - current) / (1.0 - current)).max(0.0)
}

/// How strongly tiles at least as fogged as `floor` reach tile-local
/// `(u, v)`: fully on such a tile, fading with distance from one, and
/// rounded off where two such sides meet so the corner does not jut.
fn fog_reach(neighborhood: &[[Fog; 3]; 3], u: f32, v: f32, floor: Fog) -> f32 {
    let fogged = |row: usize, col: usize| neighborhood[row][col] >= floor;
    let gap = |index: usize, offset: f32| match index {
        0 => offset,
        1 => 0.0,
        _ => 1.0 - offset,
    };
    let mut reach = 0.0_f32;
    for row in 0..3 {
        for col in 0..3 {
            if fogged(row, col) {
                let distance = gap(col, u).hypot(gap(row, v));
                reach = reach.max((1.0 - distance / FOG_FEATHER).clamp(0.0, 1.0));
            }
        }
    }
    for (row, col) in [(0, 0), (0, 2), (2, 0), (2, 2)] {
        if fogged(1, col) && fogged(row, 1) {
            let inset = (FOG_FEATHER - gap(col, u))
                .max(0.0)
                .hypot((FOG_FEATHER - gap(row, v)).max(0.0));
            reach = reach.max((inset / FOG_FEATHER).min(1.0));
        }
    }
    reach
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fog {
    Visible,
    Explored,
    Unexplored,
}

impl Fog {
    fn alpha(self) -> f32 {
        match self {
            Fog::Visible => 0.0,
            Fog::Explored => FOG_EXPLORED.a,
            Fog::Unexplored => 1.0,
        }
    }

    /// The veil over a tile of this fog.
    fn cover(self) -> Color {
        Color::new(
            FOG_UNEXPLORED.r,
            FOG_UNEXPLORED.g,
            FOG_UNEXPLORED.b,
            self.alpha(),
        )
    }
}

fn tile_fog(game: &crate::game::Scene<'_>, tile: TilePos) -> Fog {
    let (explored, visible) = if game.state.map().tile(tile).is_some() {
        (
            game.my_vision().explored(tile),
            game.my_vision().visible(tile),
        )
    } else {
        (
            game.presentation.boundary_fog.explored(tile),
            game.presentation.boundary_fog.visible(tile),
        )
    };
    if !explored {
        Fog::Unexplored
    } else if !visible {
        Fog::Explored
    } else {
        Fog::Visible
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThemePropPlacement {
    variant: usize,
    quarter_turns: u8,
}

impl ThemePropPlacement {
    /// Theme marks are flat surface marks, so they turn freely.
    fn rotation(self) -> f32 {
        f32::from(self.quarter_turns) * std::f32::consts::FRAC_PI_2
    }
}

const GROUND_BLOCKER_FOOTPRINTS: [(i32, i32); 9] = [
    (2, 2),
    (2, 1),
    (3, 2),
    (2, 2),
    (3, 2),
    (3, 2),
    (3, 1),
    (2, 2),
    (2, 2),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObstaclePlacement {
    anchor: TilePos,
    footprint: (i32, i32),
    /// Index into the ground-blocker library.
    variant: usize,
}

fn coordinate_hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut hash = 2_166_136_261u32;
    for word in [x.cast_unsigned(), y.cast_unsigned(), salt] {
        for byte in word.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

fn placement_for_group(
    group: TilePos,
    mut rock: impl FnMut(TilePos) -> bool,
) -> Option<ObstaclePlacement> {
    let hash = coordinate_hash(group.x, group.y, 0x5155_4152);
    // Rock is slabs; about one 3x2 cell in six resolves into an abandoned
    // machine footprint instead.
    if !hash.is_multiple_of(3) || !(hash / 3).is_multiple_of(2) {
        return None;
    }
    let start = hash as usize / 11 % GROUND_BLOCKER_FOOTPRINTS.len();
    for step in 0..GROUND_BLOCKER_FOOTPRINTS.len() {
        let variant = (start + step) % GROUND_BLOCKER_FOOTPRINTS.len();
        let footprint = GROUND_BLOCKER_FOOTPRINTS[variant];
        let x_slack = 3 - footprint.0;
        let y_slack = 2 - footprint.1;
        let anchor = TilePos::new(
            group.x
                + ((hash / 37 + step.fit::<u32>()) % (x_slack.cast_unsigned() + 1)).cast_signed(),
            group.y
                + ((hash / 71 + step.fit::<u32>()) % (y_slack.cast_unsigned() + 1)).cast_signed(),
        );
        let fits =
            (0..footprint.1).all(|dy| (0..footprint.0).all(|dx| rock(anchor.offset(dx, dy))));
        if fits {
            return Some(ObstaclePlacement {
                anchor,
                footprint,
                variant,
            });
        }
    }
    None
}

/// The map's industrial blockers. They stand only on rock inside the
/// boundary's edge ring, which the boundary draws over.
pub(super) struct Blockers {
    placements: Vec<ObstaclePlacement>,
    width: i32,
    /// Map tiles a blocker stands on, row-major, so no slab does.
    covered: Vec<bool>,
}

impl Blockers {
    pub(super) fn build(map: &aftermine_sim::map::Map, rows: &[String]) -> Self {
        let ring = super::environment::boundary_ring(rows);
        let (width, height) = (map.width(), map.height());
        let mut placements = Vec::new();
        let mut covered = vec![false; as_index((width * height).max(0))];
        for y in (0..height).step_by(2) {
            for x in (0..width).step_by(3) {
                let Some(placement) = placement_for_group(TilePos::new(x, y), |pos| {
                    !ring(pos)
                        && map
                            .tile(pos)
                            .is_some_and(|tile| tile.terrain == aftermine_sim::map::Terrain::Rock)
                }) else {
                    continue;
                };
                for dy in 0..placement.footprint.1 {
                    for dx in 0..placement.footprint.0 {
                        let pos = placement.anchor.offset(dx, dy);
                        covered[as_index(pos.y * width + pos.x)] = true;
                    }
                }
                placements.push(placement);
            }
        }
        Self {
            placements,
            width,
            covered,
        }
    }

    /// Whether a blocker stands on this tile.
    pub(super) fn covers(&self, pos: TilePos) -> bool {
        (0..self.width).contains(&pos.x)
            && pos.y >= 0
            && self
                .covered
                .get(as_index(pos.y * self.width + pos.x))
                .copied()
                .unwrap_or(false)
    }
}

/// The blockers in view, over the tiles and under the slabs.
pub(super) fn draw_blockers(
    view: &GroundView,
    sprites: &Sprites,
    blockers: &Blockers,
    grade: super::grade::Grade,
) {
    let zoom = view.zoom();
    let (lo, hi) = view.world_rect();
    let min = numeric::tile_at(lo);
    let max = TilePos::new(numeric::to_i32(hi.x.ceil()), numeric::to_i32(hi.y.ceil()));
    for placement in &blockers.placements {
        let (anchor, (width, height)) = (placement.anchor, placement.footprint);
        if anchor.x + width <= min.x
            || anchor.y + height <= min.y
            || anchor.x >= max.x
            || anchor.y >= max.y
        {
            continue;
        }
        let source = sprites.ground_blocker(placement.variant);
        let screen = view.to_screen(vec2(anchor.x as f32, anchor.y as f32));
        sprites.draw(
            screen.x.floor(),
            screen.y.floor(),
            grade.rock(),
            DrawTextureParams {
                dest_size: Some(vec2(width as f32 * zoom + 1.0, height as f32 * zoom + 1.0)),
                source: Some(source),
                ..Default::default()
            },
        );
    }
}

fn theme_code(theme: &str) -> Option<u32> {
    // Stable layout salts, chosen so each shipped theme exercises its complete
    // prop row without changing the shared density rule.
    match theme {
        "rusted-yard" => Some(1),
        "cold-circuitry" => Some(2),
        "quarry-dust" => Some(25),
        "basalt" => Some(4),
        "slag" => Some(5),
        "verdigris" => Some(6),
        _ => None,
    }
}

/// Sparse dressing picked from a coordinate and its 180-degree partner.
/// Both halves therefore choose the same art; the far half records a half-turn
/// so rotatable surface marks preserve the shipped maps' visual symmetry.
fn symmetric_theme_prop(
    theme: &str,
    pos: TilePos,
    width: i32,
    height: i32,
) -> Option<ThemePropPlacement> {
    let theme = theme_code(theme)?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let mirror = TilePos::new(width - 1 - pos.x, height - 1 - pos.y);
    // A directional one-tile mark cannot be its own 180-degree partner.
    if pos == mirror {
        return None;
    }
    let (canonical, mirrored) = if (pos.y, pos.x) <= (mirror.y, mirror.x) {
        (pos, false)
    } else {
        (mirror, true)
    };
    // FNV-1a over fixed-width words: stable across platforms and Rust
    // versions, unlike DefaultHasher. Dimensions keep two differently sized
    // maps from laying down the same visible stamp.
    let mut hash = 2_166_136_261u32;
    for word in [
        theme,
        canonical.x.cast_unsigned(),
        canonical.y.cast_unsigned(),
        width.cast_unsigned(),
        height.cast_unsigned(),
    ] {
        for byte in word.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    // One residue in eleven gives open ground enough history to establish a
    // map identity without turning every tile into visual noise. Residue five
    // keeps even the smallest shipped arenas above the density floor.
    if hash % 11 != 5 {
        return None;
    }
    let variant = (hash / 11 % 13) as usize;
    let base_turns = (hash / (11 * 13) % 4) as u8;
    Some(ThemePropPlacement {
        variant,
        quarter_turns: if mirrored {
            (base_turns + 2) % 4
        } else {
            base_turns
        },
    })
}

/// The theme mark a placement draws, if any. Only a theme's first three marks
/// are flat, low-contrast ground; the rest would pull the eye from units.
fn quiet_mark(placement: ThemePropPlacement) -> Option<usize> {
    (placement.variant < QUIET_MARKS).then_some(placement.variant)
}

const QUIET_MARKS: usize = 3;

fn authored_tile(rows: &[String], pos: TilePos) -> Option<u8> {
    if pos.x < 0 || pos.y < 0 {
        return None;
    }
    rows.get(as_index(pos.y))?
        .as_bytes()
        .get(as_index(pos.x))
        .copied()
}

/// Props only paint authored plain-ground tiles. A 3x3 terrain patch keeps
/// rock edges and one-tile passes visually clean; salvage counts as eventual
/// open ground so an unseen node cannot change the dressing on its explored
/// neighbor. A wider digit scan leaves the starting base apron quiet.
/// Consulting authored marks rather than live scrap/wreck state also keeps
/// the dressing stable for the entire match.
fn safe_theme_prop_tile(rows: &[String], pos: TilePos) -> bool {
    if authored_tile(rows, pos) != Some(b'.') {
        return false;
    }
    for dy in -1..=1 {
        for dx in -1..=1 {
            if !matches!(
                authored_tile(rows, pos.offset(dx, dy)),
                Some(b'.' | b's' | b'S')
            ) {
                return false;
            }
        }
    }
    for dy in -4..=4 {
        for dx in -4..=4 {
            if authored_tile(rows, pos.offset(dx, dy)).is_some_and(|c| c.is_ascii_digit()) {
                return false;
            }
        }
    }
    true
}

fn symmetric_safe_theme_prop_tile(rows: &[String], pos: TilePos) -> bool {
    let height = rows.len().fit::<i32>();
    let width = rows
        .first()
        .map_or(0, std::string::String::len)
        .fit::<i32>();
    let mirror = TilePos::new(width - 1 - pos.x, height - 1 - pos.y);
    safe_theme_prop_tile(rows, pos) && safe_theme_prop_tile(rows, mirror)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct QuarryDressing {
    variant: usize,
    rotation: f32,
    offset: Vec2,
    scale: f32,
}

fn quarry_dressing(pos: TilePos, width: i32, height: i32, seed: u64) -> Option<QuarryDressing> {
    let mirror = TilePos::new(width - 1 - pos.x, height - 1 - pos.y);
    if width <= 0 || height <= 0 || pos == mirror {
        return None;
    }
    let mirrored = (pos.y, pos.x) > (mirror.y, mirror.x);
    let canonical = if mirrored { mirror } else { pos };
    let salt = (seed & 0xFFFF_FFFF) as u32
        ^ (seed >> 32) as u32
        ^ width.cast_unsigned().rotate_left(9)
        ^ height.cast_unsigned().rotate_left(19);
    let mut token = super::environment::hash(canonical.x, canonical.y, salt);
    token ^= token >> 16;
    token = token.wrapping_mul(0x7feb_352d);
    token ^= token >> 15;
    token = token.wrapping_mul(0x846c_a68b);
    token ^= token >> 16;
    if !token.is_multiple_of(19) {
        return None;
    }
    let direction = if mirrored { -1.0 } else { 1.0 };
    Some(QuarryDressing {
        variant: (token / 19 % 12) as usize,
        rotation: (((token >> 12) % 4) as f32 + if mirrored { 2.0 } else { 0.0 })
            * std::f32::consts::FRAC_PI_2,
        offset: vec2(
            ((token >> 17) % 17) as f32 - 8.0,
            ((token >> 22) % 17) as f32 - 8.0,
        ) * (direction / 64.0),
        scale: 0.72 + ((token >> 27) % 8) as f32 * 0.035,
    })
}

/// A tile in view and the square it covers.
struct TileSpot<'a> {
    pos: TilePos,
    tile: &'a aftermine_sim::map::Tile,
    /// The tile's top-left corner, unsnapped.
    screen: Vec2,
    /// The tile's size between snapped corners.
    size: Vec2,
    /// The linear hash that steps ground shades in diagonal waves.
    wave: usize,
}

fn for_each_tile<'a>(
    map: &'a aftermine_sim::map::Map,
    view: &GroundView,
    mut draw: impl FnMut(TileSpot<'a>),
) {
    let (min, max) = view.tiles(map);
    for y in min.y..max.y {
        for x in min.x..max.x {
            let pos = TilePos::new(x, y);
            let Some(tile) = map.tile(pos) else {
                continue;
            };
            let screen = view.to_screen(vec2(x as f32, y as f32));
            let next = view.to_screen(vec2((x + 1) as f32, (y + 1) as f32));
            draw(TileSpot {
                pos,
                tile,
                screen,
                size: next.floor() - screen.floor(),
                wave: x
                    .wrapping_mul(31)
                    .wrapping_add(y.wrapping_mul(17))
                    .cast_unsigned() as usize,
            });
        }
    }
}

/// How a tile's ground sprite is tinted, and the white wash that brightens
/// it past what a tint can: a tiered map shades each tile by its level.
fn ground_paint<'a>(ground: &Ground<'a>) -> impl Fn(&TileSpot<'_>) -> (Color, f32) + use<'a> {
    let base = ground.grade.tint();
    let levels = ground.layout.levels.as_ref();
    move |spot| match levels.and_then(|levels| levels.tile_paint(spot.pos, spot.wave % 6)) {
        Some((f, wash)) => (Color::new(base.r * f, base.g * f, base.b * f, base.a), wash),
        None => (base, 0.0),
    }
}

/// Draws the floor one layer at a time: ground, the level washes, grit and
/// dressing, then the level steps. Each layer crosses the whole view before
/// the next starts, because a fill between two atlas sprites splits
/// macroquad's batch, and macroquad keeps a buffer for every draw a frame has
/// ever needed: drawn tile by tile, a zoomed-out tiered map took fourteen
/// thousand draws and exhausted an iPad's memory.
pub(super) fn draw_floor(ground: &Ground<'_>, view: &GroundView, sprites: &Sprites) {
    let zoom = view.zoom();
    let size = zoom.ceil() + 1.0; // slight overlap kills seam hairlines
    let theme = ground
        .scenario
        .meta
        .as_ref()
        .map_or("", |m| m.theme.as_str());
    let map = ground.map;
    let (map_width, map_height) = (map.width(), map.height());
    // Quarry props vary by map, not by anything in play.
    let quarry_salt = chassis::hash::fnv1a(ground.scenario.name.as_bytes());
    let levels = ground.layout.levels.as_ref();
    let paint = ground_paint(ground);
    for_each_tile(map, view, |spot| {
        sprites.draw(
            spot.screen.x.floor(),
            spot.screen.y.floor(),
            paint(&spot).0,
            DrawTextureParams {
                dest_size: Some(spot.size),
                source: Some(sprites.ground(spot.wave % 6)),
                ..Default::default()
            },
        );
    });
    if let Some(wash) = ground.grade.floor_wash() {
        let corner = view.to_screen(Vec2::ZERO).floor();
        let far = view
            .to_screen(vec2(map_width as f32, map_height as f32))
            .floor();
        draw_rectangle(corner.x, corner.y, far.x - corner.x, far.y - corner.y, wash);
    }
    if levels.is_some() {
        for_each_tile(map, view, |spot| {
            let wash = paint(&spot).1;
            if wash > 0.0 {
                draw_rectangle(
                    spot.screen.x.floor(),
                    spot.screen.y.floor(),
                    spot.size.x,
                    spot.size.y,
                    Color::new(1.0, 1.0, 1.0, wash),
                );
            }
        });
    }
    for_each_tile(map, view, |spot| {
        let (x, y) = (spot.pos.x, spot.pos.y);
        let tile = spot.tile;
        let tint = paint(&spot).0;
        // Grit and cracks use a mixed hash so no shade repeats one texture;
        // FNV's low bits track coordinate parity, so read its high bits.
        let detail = (coordinate_hash(x, y, 0x4752_4e44) >> 16) as usize;
        let orientation = detail / sprites.ground_detail_count();
        sprites.draw(
            spot.screen.x.floor(),
            spot.screen.y.floor(),
            tint,
            DrawTextureParams {
                dest_size: Some(spot.size),
                source: Some(sprites.ground_detail(detail)),
                flip_x: orientation & 1 == 1,
                flip_y: orientation & 2 == 2,
                ..Default::default()
            },
        );
        // Ground dressing stays under resources, entities, and the fog
        // veil. Static sprites use no wall clock, so reduced-motion mode
        // needs no alternate path.
        let pos = spot.pos;
        // A ramp's ruts and walls are its dressing.
        let dressed = tile.terrain == aftermine_sim::map::Terrain::Ground && !map.is_ramp(pos);
        let prop_candidate = if tile.cosmetic == 1 || !dressed {
            None
        } else {
            symmetric_theme_prop(theme, pos, map_width, map_height)
        };
        let placement = quarry_dressing(pos, map_width, map_height, quarry_salt).filter(|_| {
            tile.cosmetic != 1
                && dressed
                && symmetric_safe_theme_prop_tile(&ground.scenario.map, pos)
        });
        let dressing = if let Some(placement) = placement {
            Some((
                sprites.quarry_dressing(placement.variant),
                placement.rotation,
                tint,
            ))
        } else if tile.cosmetic == 1 {
            Some((sprites.wreck_decal(), 0.0, tint))
        } else if let Some(placement) = prop_candidate {
            quiet_mark(placement)
                .filter(|_| symmetric_safe_theme_prop_tile(&ground.scenario.map, pos))
                .and_then(|variant| sprites.theme_prop(theme, variant))
                .map(|source| (source, placement.rotation(), WHITE))
        } else {
            None
        };
        if let Some((source, rotation, dressing_tint)) = dressing {
            let (offset, scale) = placement.map_or((Vec2::ZERO, 1.0), |p| (p.offset, p.scale));
            let origin = spot.screen + (Vec2::splat((1.0 - scale) * 0.5) + offset) * zoom;
            sprites.draw(
                origin.x.floor(),
                origin.y.floor(),
                dressing_tint,
                DrawTextureParams {
                    dest_size: Some(vec2(size * scale, size * scale)),
                    source: Some(source),
                    rotation,
                    ..Default::default()
                },
            );
        }
        // Rocks cast a soft skirt onto neighboring ground. Each skirt fills
        // its tile exactly: neighbouring skirts drawn in the same layer would
        // otherwise overlap and leave a brighter seam. A quarter turn spins
        // the quad about its centre, so it is drawn with its sides swapped.
        if tile.terrain == aftermine_sim::map::Terrain::Ground {
            for (dx, dy, quarter_turns) in [(0, -1, 0u8), (1, 0, 1), (0, 1, 2), (-1, 0, 3)] {
                let rocky = map
                    .tile(pos.offset(dx, dy))
                    .is_some_and(|t| t.terrain == aftermine_sim::map::Terrain::Rock);
                if !rocky {
                    continue;
                }
                let corner = spot.screen.floor();
                let dest = if quarter_turns % 2 == 1 {
                    vec2(spot.size.y, spot.size.x)
                } else {
                    spot.size
                };
                let origin = corner + (spot.size - dest) * 0.5;
                sprites.draw(
                    origin.x,
                    origin.y,
                    tint,
                    DrawTextureParams {
                        dest_size: Some(dest),
                        source: Some(sprites.rock_skirt()),
                        rotation: f32::from(quarter_turns) * std::f32::consts::FRAC_PI_2,
                        ..Default::default()
                    },
                );
            }
        }
    });
    // A tiered map's steps and ramps lie over the ground, under salvage and
    // frames.
    if let Some(levels) = levels {
        for_each_tile(map, view, |spot| {
            levels.draw_tile(view, spot.pos, ground.grade);
        });
    }
}

/// Scrap nodes and wrecks on the floor in view. Scrap draws at its live
/// amount only in sight; unseen ground shows what the player remembers
/// (frozen, like ghosts).
pub(super) fn draw_salvage(
    game: &crate::game::Scene<'_>,
    ground: &Ground<'_>,
    view: &GroundView,
    sprites: &Sprites,
) {
    // Exact and smooth, unlike the baked ground: salvage draws live, so rounding
    // would make each piece step against the rest as the zoom moves.
    let size = view.zoom() + 1.0;
    let paint = ground_paint(ground);
    for_each_tile(ground.map, view, |spot| {
        let (pos, tile) = (spot.pos, spot.tile);
        let seen_now = game.presentation.all_seeing() || game.my_vision().visible(pos);
        let scrap = if seen_now {
            tile.scrap
        } else {
            game.my_vision().remembered_scrap(pos)
        };
        // Wrecks follow the same sight rule; a live node or rock outranks the
        // junk visually.
        let wreck = if seen_now {
            tile.wreck
        } else {
            game.my_vision().remembered_wreck(pos)
        };
        let (overlay, flip) = match (tile.terrain, scrap) {
            // Rock slabs, peak terraces, and pit drops are drawn by their own
            // passes after the tiles. A cliff tile shows the ground at its
            // foot, and the levels pass draws its face.
            (
                aftermine_sim::map::Terrain::Rock
                | aftermine_sim::map::Terrain::Peak
                | aftermine_sim::map::Terrain::Pit
                | aftermine_sim::map::Terrain::Cliff,
                _,
            ) => (None, false),
            (aftermine_sim::map::Terrain::Ground, 0) if wreck > 0 => (
                Some(sprites.wreck_pile_holding(wreck, spot.wave / 5)),
                spot.wave % 5 < 2,
            ),
            (aftermine_sim::map::Terrain::Ground, 0) => (None, false),
            (aftermine_sim::map::Terrain::Ground, s) => {
                (Some(sprites.scrap(s, SCRAP_NODE_AMOUNT)), false)
            }
        };
        let Some(source) = overlay else {
            return;
        };
        // Stamp what is on show; fade what is only remembered. The map stays
        // bounded: only tiles carrying salvage enter it.
        let fade = super::resource_memory_opacity(game, pos);
        let tint = paint(&spot).0;
        let tint = if fade < 1.0 {
            Color::new(tint.r, tint.g, tint.b, tint.a * fade)
        } else {
            tint
        };
        // A wreck pile's canvas is wider than its tile, and stays centered on it.
        let drawn = size * source.w / 64.0;
        let inset = (size - drawn) * 0.5;
        sprites.draw(
            spot.screen.x + inset,
            spot.screen.y + inset,
            tint,
            DrawTextureParams {
                dest_size: Some(vec2(drawn, drawn)),
                source: Some(source),
                flip_x: flip,
                ..Default::default()
            },
        );
    });
}

/// Unclaimed derelict Extractor frames, drawn as part of the map: a
/// collapsed 2x2 machine bed that says "rebuild here". A standing
/// building on the anchor covers its frame; unexplored ground hides it
/// like any other terrain fact.
pub(super) fn draw_extractor_frames(
    game: &crate::game::Scene<'_>,
    view: &GroundView,
    sprites: &Sprites,
) {
    let zoom = view.zoom();
    for &frame in game.state.map().extractor_frames() {
        if !crate::strategic_markers::extractor_frame_visible(game, frame) {
            continue;
        }
        let screen = view.to_screen(vec2(frame.x as f32, frame.y as f32));
        sprites.draw(
            screen.x,
            screen.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(zoom * 2.0 + 1.0, zoom * 2.0 + 1.0)),
                source: Some(sprites.extractor_frame()),
                ..Default::default()
            },
        );
    }
}

/// Battle scars: scorch decals where buildings died, fading over ~20s.
pub(crate) fn draw_scorches(game: &crate::game::Scene<'_>, sprites: &Sprites) {
    let zoom = game.presentation.camera.zoom;
    for (at, age, width) in &game.presentation.scorches {
        let alpha = (1.0 - age / 20.0).clamp(0.0, 1.0) * 0.85;
        let size = zoom * width;
        let screen = game.presentation.camera.to_screen(*at);
        sprites.draw(
            screen.x - size * 0.5,
            screen.y - size * 0.5,
            Color::new(1.0, 1.0, 1.0, alpha),
            DrawTextureParams {
                dest_size: Some(vec2(size, size)),
                source: Some(sprites.scorch()),
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests;
