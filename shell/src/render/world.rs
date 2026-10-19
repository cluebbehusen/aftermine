//! Ground layers: terrain tiles (fog-ruled), the fog veil, and battle
//! scars.

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
    let (lo, hi) = view.world_rect();
    let min = numeric::tile_at(lo);
    let max = TilePos::new(numeric::to_i32(hi.x.ceil()), numeric::to_i32(hi.y.ceil()));
    // One extra ring so every drawn tile sees all eight neighbors.
    let stride = as_index(max.x - min.x + 2);
    let states: Vec<Fog> = (min.y - 1..=max.y)
        .flat_map(|y| (min.x - 1..=max.x).map(move |x| tile_fog(game, TilePos::new(x, y))))
        .collect();
    let fog = |x: i32, y: i32| states[as_index(y - min.y + 1) * stride + as_index(x - min.x + 1)];
    for y in min.y..max.y {
        for x in min.x..max.x {
            let alpha = fog(x, y).alpha();
            if alpha == 0.0 {
                continue;
            }
            let cover = Color::new(FOG_UNEXPLORED.r, FOG_UNEXPLORED.g, FOG_UNEXPLORED.b, alpha);
            // Exact shared edges: translucent rects that overlap draw
            // double-dark seams, so each tile ends where the next begins.
            let a = view.to_screen(vec2(x as f32, y as f32)).floor();
            let b = view.to_screen(vec2((x + 1) as f32, (y + 1) as f32)).floor();
            draw_rectangle(a.x, a.y, b.x - a.x, b.y - a.y, cover);
        }
    }
    // Feather inward into known ground. Unknown tiles retain their opaque veil.
    for y in min.y..max.y {
        for x in min.x..max.x {
            let neighborhood: [[Fog; 3]; 3] = std::array::from_fn(|row| {
                std::array::from_fn(|col| fog(x + col.fit::<i32>() - 1, y + row.fit::<i32>() - 1))
            });
            let current = neighborhood[1][1];
            if current == Fog::Unexplored
                || neighborhood.iter().flatten().all(|&state| state <= current)
            {
                continue;
            }
            let a = view.to_screen(vec2(x as f32, y as f32)).floor();
            let b = view.to_screen(vec2((x + 1) as f32, (y + 1) as f32)).floor();
            draw_fog_feather(a, b, &neighborhood);
        }
    }
}

/// Samples [`fog_feather_alpha`] on a grid over one tile's screen rect.
fn draw_fog_feather(a: Vec2, b: Vec2, neighborhood: &[[Fog; 3]; 3]) {
    let band = (b - a).max_element() * FOG_FEATHER;
    let steps = numeric::to_usize((band / 6.0).ceil().clamp(1.0, 8.0));
    // Grid lines fall on the feather band boundaries so straight fades
    // interpolate exactly; only the rounded corners are approximated.
    let coords: Vec<f32> = (0..=steps)
        .map(|i| FOG_FEATHER * i as f32 / steps as f32)
        .chain((0..=steps).map(|i| 1.0 - FOG_FEATHER + FOG_FEATHER * i as f32 / steps as f32))
        .collect();
    let side = coords.len();
    let vertices = coords
        .iter()
        .flat_map(|&v| {
            coords.iter().map(move |&u| {
                let p = a + (b - a) * vec2(u, v);
                let alpha = fog_feather_alpha(neighborhood, u, v);
                let color = Color::new(FOG_UNEXPLORED.r, FOG_UNEXPLORED.g, FOG_UNEXPLORED.b, alpha);
                Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, color)
            })
        })
        .collect();
    let indices = (0..side - 1)
        .flat_map(|row| {
            (0..side - 1).flat_map(move |col| {
                let top = (row * side + col).fit::<u16>();
                let bottom = top + side.fit::<u16>();
                [top, top + 1, bottom + 1, top, bottom + 1, bottom]
            })
        })
        .collect();
    draw_mesh(&Mesh {
        vertices,
        indices,
        texture: None,
    });
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
    fn rotation(self) -> f32 {
        // The first prop bank is made of flat surface marks. The second bank
        // contains raised objects with a baked world-space highlight and
        // shadow, so rotating those objects also rotates their lighting.
        let turns = if self.variant < 3 {
            self.quarter_turns
        } else {
            0
        };
        f32::from(turns) * std::f32::consts::FRAC_PI_2
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
    pub(super) fn build(map: &oxide_sim::map::Map, rows: &[String]) -> Self {
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
                            .is_some_and(|tile| tile.terrain == oxide_sim::map::Terrain::Rock)
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
pub(super) fn draw_blockers(view: &GroundView, sprites: &Sprites, blockers: &Blockers) {
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
            WHITE,
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
    tile: &'a oxide_sim::map::Tile,
    /// The tile's top-left corner, unsnapped.
    screen: Vec2,
    /// The tile's size between snapped corners.
    size: Vec2,
    /// The linear hash that steps ground shades in diagonal waves.
    wave: usize,
}

fn for_each_tile<'a>(
    map: &'a oxide_sim::map::Map,
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
    let theme = ground
        .scenario
        .meta
        .as_ref()
        .map_or("", |m| m.theme.as_str());
    let base = theme_tint(theme);
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
    let themed = theme_code(theme).is_some();
    let map = ground.map;
    let (map_width, map_height) = (map.width(), map.height());
    // Quarry props vary by map, not by anything in play.
    let quarry_salt = chassis::hash::fnv1a(ground.scenario.name.as_bytes());
    let quarry = sprites.quarry_dressing(0).is_some();
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
        let dressed = tile.terrain == oxide_sim::map::Terrain::Ground && !map.is_ramp(pos);
        let prop_candidate = if tile.cosmetic == 1 || !dressed {
            None
        } else {
            symmetric_theme_prop(theme, pos, map_width, map_height)
        };
        let placement = quarry
            .then(|| quarry_dressing(pos, map_width, map_height, quarry_salt))
            .flatten()
            .filter(|_| {
                tile.cosmetic != 1
                    && dressed
                    && symmetric_safe_theme_prop_tile(&ground.scenario.map, pos)
            });
        let dressing = if let Some(placement) = placement {
            sprites
                .quarry_dressing(placement.variant)
                .map(|source| (source, placement.rotation, tint))
        } else if tile.cosmetic == 1 {
            Some((sprites.decal(3), 0.0, tint))
        } else if quarry {
            None
        } else if let Some(placement) = prop_candidate {
            if symmetric_safe_theme_prop_tile(&ground.scenario.map, pos) {
                let source = if placement.variant < 3 {
                    sprites.theme_prop(theme, placement.variant)
                } else {
                    Some(sprites.field_debris(placement.variant - 3))
                };
                source.map(|source| (source, placement.rotation(), WHITE))
            } else {
                None
            }
        } else if !themed && dressed && spot.wave.is_multiple_of(23) {
            Some((sprites.decal(spot.wave / 23 % 3), 0.0, tint))
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
        if tile.terrain == oxide_sim::map::Terrain::Ground {
            for (dx, dy, quarter_turns) in [(0, -1, 0u8), (1, 0, 1), (0, 1, 2), (-1, 0, 3)] {
                let rocky = map
                    .tile(pos.offset(dx, dy))
                    .is_some_and(|t| t.terrain == oxide_sim::map::Terrain::Rock);
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
        for_each_tile(map, view, |spot| levels.draw_tile(view, spot.pos));
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
    let size = view.zoom().ceil() + 1.0;
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
                oxide_sim::map::Terrain::Rock
                | oxide_sim::map::Terrain::Peak
                | oxide_sim::map::Terrain::Pit
                | oxide_sim::map::Terrain::Cliff,
                _,
            ) => (None, false),
            (oxide_sim::map::Terrain::Ground, 0) if wreck > 0 => {
                (Some(sprites.wreck_pile()), spot.wave % 5 < 2)
            }
            (oxide_sim::map::Terrain::Ground, 0) => (None, false),
            (oxide_sim::map::Terrain::Ground, s) => {
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
        sprites.draw(
            spot.screen.x.floor(),
            spot.screen.y.floor(),
            tint,
            DrawTextureParams {
                dest_size: Some(vec2(size, size)),
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
            screen.x.floor(),
            screen.y.floor(),
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
