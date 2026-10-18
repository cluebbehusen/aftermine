//! Terrain: a tile grid parsed from ASCII art.
//!
//! Maps are authored as text so an agent or a human can read a scenario
//! file and see the level. Legend:
//!
//! ```text
//! . ,  ground, plain or cosmetically rubble-strewn
//! #    rock (blocks ground and direct ground fire)
//! ^    peak (blocks ground, air, and all fire)
//! ~    bottomless pit (blocks ground; air and fire cross)
//! =    cliff between two levels (blocks ground; air and fire cross)
//! /    ramp joining two levels (ground)
//! s S  normal or rich scrap node (ground after depletion)
//! E    derelict Extractor frame's top-left tile
//! 1-8  Foundry anchors for seats 1-8
//! a-h  Foundry anchors for seats 9-16
//! ```
//!
//! A tiered map adds a levels layer: one row per map row, one glyph per
//! tile, `0`-`9` then `a`-`z` for levels 0 to 35. A cliff tile and a ramp
//! both carry the lower level they join. A map with no levels
//! layer, or one at a single level throughout, is flat.

use crate::ids::PlayerId;
use crate::stats::{RICH_SCRAP_NODE_AMOUNT, SCRAP_NODE_AMOUNT};
use chassis::grid::{Grid, TilePos, as_index};
use serde::{Deserialize, Serialize};

/// Largest supported map edge, in tiles.
pub const MAX_MAP_EDGE: u16 = 256;

/// A position or extent of a parsed map, already checked against
/// [`MAX_MAP_EDGE`].
fn edge_coordinate(value: usize) -> i32 {
    i32::try_from(value).expect("map edges are checked against MAX_MAP_EDGE")
}

/// Base terrain of a tile.
///
/// Variant order is hash-load-bearing: postcard serializes the
/// declaration index, so new variants are appended, never inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terrain {
    /// Walkable.
    Ground,
    /// Never walkable; aircraft fly over it.
    Rock,
    /// A mountain: blocks ground, air, and every shot, artillery arcs
    /// included.
    Peak,
    /// A bottomless excavation: blocks ground, while aircraft and fire of
    /// every kind cross it. Wrecks that fall here are gone.
    Pit,
    /// A sheer face between two levels of a tiered map: blocks ground,
    /// while aircraft cross it. Wrecks that fall here are gone.
    Cliff,
}

/// The height of a tile on a tiered map.
///
/// Variant order is hash-load-bearing: postcard serializes the declaration
/// index, so new variants are appended, never inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Elevation {
    /// A tile at one level. A cliff carries the lower level it bounds, so it
    /// stands as high as the ground at its foot.
    Level(u8),
    /// A ramp joining `low` and the level above it.
    Ramp {
        /// The lower of the two levels the ramp joins.
        low: u8,
    },
}

impl Elevation {
    /// The lowest and highest level the tile stands at.
    pub fn span(self) -> (u8, u8) {
        match self {
            Self::Level(level) => (level, level),
            Self::Ramp { low } => (low, low.saturating_add(1)),
        }
    }

    /// Whether the two share a level: equal levels, or a ramp and either
    /// level it joins.
    pub fn overlaps(self, other: Self) -> bool {
        let ((a_lo, a_hi), (b_lo, b_hi)) = (self.span(), other.span());
        a_lo <= b_hi && b_lo <= a_hi
    }
}

impl Terrain {
    /// Whether ground machines can never enter this terrain.
    pub fn blocks_ground(self) -> bool {
        self != Terrain::Ground
    }

    /// Whether aircraft cannot overfly this terrain.
    pub fn blocks_air(self) -> bool {
        self == Terrain::Peak
    }

    /// Whether this terrain gives cover against direct ground-vs-ground
    /// fire. Air-involved and indirect fire ignore this; see
    /// [`Terrain::blocks_all_fire`] for the absolute wall.
    pub fn blocks_direct_fire(self) -> bool {
        matches!(self, Terrain::Rock | Terrain::Peak)
    }

    /// Whether this terrain blocks every shot in every pairing,
    /// artillery arcs included.
    pub fn blocks_all_fire(self) -> bool {
        self == Terrain::Peak
    }
}

/// One tile of the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    /// Base terrain.
    pub terrain: Terrain,
    /// Remaining scrap. A ground tile with scrap is a node: impassable and
    /// harvestable until it hits zero.
    pub scrap: u32,
    /// Battlefield salvage left by destroyed machines. Unlike a node it
    /// never blocks movement — harvesters stand *on* the tile to strip it
    /// — and it decays slowly back into the dirt.
    pub wreck: u32,
    /// Purely visual ground dressing (0 plain, 1 rubble). No gameplay
    /// effect, but part of the map and therefore of the state hash.
    pub cosmetic: u8,
}

/// Errors from [`Map::parse`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MapError {
    /// The map had no rows or empty rows.
    #[error("map is empty")]
    Empty,
    /// The map exceeds the supported envelope. The cap keeps fixed-point
    /// squared distances and neighborhood scans comfortably inside their
    /// numeric ranges.
    #[error("map is {width}x{height}; the supported maximum is {MAX_MAP_EDGE} per side")]
    TooLarge {
        /// Parsed width.
        width: usize,
        /// Parsed height.
        height: usize,
    },
    /// Not all rows share one length.
    #[error("row {row} is {len} tiles wide, expected {expected}")]
    Ragged {
        /// Offending row index.
        row: usize,
        /// Its length.
        len: usize,
        /// Length of row zero.
        expected: usize,
    },
    /// A character outside the legend.
    #[error("unknown map character {c:?} at ({x}, {y})")]
    UnknownChar {
        /// The character.
        c: char,
        /// Column.
        x: i32,
        /// Row.
        y: i32,
    },
    /// The same player digit appeared twice.
    #[error("duplicate foundry anchor for player {0}")]
    DuplicateAnchor(PlayerId),
    /// An extractor frame's 2x2 footprint runs off the map or overlaps
    /// non-ground terrain or a ramp.
    #[error("extractor frame at {0:?} needs a clear 2x2 ground footprint off any ramp")]
    BadFrame(TilePos),
    /// A cliff or ramp on a map with no levels layer.
    #[error("the cliff or ramp at {0:?} needs a levels layer")]
    Untiered(TilePos),
    /// The levels layer is not the map's size.
    #[error("the levels layer must be {width}x{height}, one glyph per tile")]
    LevelRows {
        /// The map's width.
        width: i32,
        /// The map's height.
        height: i32,
    },
    /// A character outside the level glyphs.
    #[error("unknown level {c:?} at ({x}, {y})")]
    UnknownLevel {
        /// The character.
        c: char,
        /// Column.
        x: i32,
        /// Row.
        y: i32,
    },
    /// A stored levels layer holds a single level, which is a flat map.
    #[error("a levels layer must hold more than one level")]
    FlatLevels,
    /// Touching ground shares no level, a ramp is off the ground, or a
    /// cliff stands above the ground beside it.
    #[error(
        "levels break at {0:?}: touching ground must share a level or meet at a ramp, and a cliff takes the lower level it bounds"
    )]
    LevelBreak(TilePos),
}

/// The playfield. Owns terrain and scrap amounts; buildings and units live
/// in [`crate::State`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    grid: Grid<Tile>,
    /// Anchors of derelict Extractor frames ('E'): the only 2x2 footprints
    /// where an Extractor can be rebuilt. The frame is part of the map and
    /// outlives every machine restored on it; the tiles beneath are plain
    /// ground and block nothing while unclaimed.
    extractor_frames: Vec<TilePos>,
    /// Every tile's level on a tiered map; `None` on a flat one.
    elevation: Option<Grid<Elevation>>,
}

impl Map {
    /// Parses ASCII rows into a flat map plus the Foundry anchors found,
    /// sorted by player.
    pub fn parse(rows: &[impl AsRef<str>]) -> Result<(Self, Vec<(PlayerId, TilePos)>), MapError> {
        Self::parse_tiered(rows, &[] as &[&str])
    }

    /// Parses ASCII rows and an optional levels layer (empty for a flat
    /// map) into a map plus the Foundry anchors found, sorted by player.
    pub fn parse_tiered(
        rows: &[impl AsRef<str>],
        levels: &[impl AsRef<str>],
    ) -> Result<(Self, Vec<(PlayerId, TilePos)>), MapError> {
        if rows.is_empty() || rows[0].as_ref().is_empty() {
            return Err(MapError::Empty);
        }
        let expected = rows[0].as_ref().chars().count();
        if rows.len() > usize::from(MAX_MAP_EDGE) || expected > usize::from(MAX_MAP_EDGE) {
            return Err(MapError::TooLarge {
                width: expected,
                height: rows.len(),
            });
        }
        let mut cells = Vec::with_capacity(rows.len() * expected);
        let mut anchors: Vec<(PlayerId, TilePos)> = Vec::new();
        let mut extractor_frames: Vec<TilePos> = Vec::new();
        let mut ramps: Vec<TilePos> = Vec::new();

        for (y, row) in rows.iter().enumerate() {
            let row = row.as_ref();
            let len = row.chars().count();
            if len != expected {
                return Err(MapError::Ragged {
                    row: y,
                    len,
                    expected,
                });
            }
            for (x, c) in row.chars().enumerate() {
                let pos = TilePos::new(edge_coordinate(x), edge_coordinate(y));
                let tile = match c {
                    '.' => Tile {
                        terrain: Terrain::Ground,
                        scrap: 0,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    ',' => Tile {
                        terrain: Terrain::Ground,
                        scrap: 0,
                        wreck: 0,
                        cosmetic: 1,
                    },
                    '#' => Tile {
                        terrain: Terrain::Rock,
                        scrap: 0,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    '^' => Tile {
                        terrain: Terrain::Peak,
                        scrap: 0,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    '~' => Tile {
                        terrain: Terrain::Pit,
                        scrap: 0,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    '=' => Tile {
                        terrain: Terrain::Cliff,
                        scrap: 0,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    '/' => {
                        ramps.push(pos);
                        Tile {
                            terrain: Terrain::Ground,
                            scrap: 0,
                            wreck: 0,
                            cosmetic: 0,
                        }
                    }
                    'E' => {
                        extractor_frames.push(pos);
                        Tile {
                            terrain: Terrain::Ground,
                            scrap: 0,
                            wreck: 0,
                            cosmetic: 0,
                        }
                    }
                    's' => Tile {
                        terrain: Terrain::Ground,
                        scrap: SCRAP_NODE_AMOUNT,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    'S' => Tile {
                        terrain: Terrain::Ground,
                        scrap: RICH_SCRAP_NODE_AMOUNT,
                        wreck: 0,
                        cosmetic: 0,
                    },
                    '1'..='8' | 'a'..='h' => {
                        // Seats 1-8 wear digits; 9-16 wear 'a'-'h'
                        // (chosen to dodge the terrain letters).
                        let player = if c.is_ascii_digit() {
                            PlayerId(c as u8 - b'1')
                        } else {
                            PlayerId(8 + (c as u8 - b'a'))
                        };
                        if anchors.iter().any(|(p, _)| *p == player) {
                            return Err(MapError::DuplicateAnchor(player));
                        }
                        anchors.push((player, pos));
                        Tile {
                            terrain: Terrain::Ground,
                            scrap: 0,
                            wreck: 0,
                            cosmetic: 0,
                        }
                    }
                    other => {
                        return Err(MapError::UnknownChar {
                            c: other,
                            x: pos.x,
                            y: pos.y,
                        });
                    }
                };
                cells.push(tile);
            }
        }
        anchors.sort_by_key(|(p, _)| *p);
        let (width, height) = (edge_coordinate(expected), edge_coordinate(rows.len()));
        let elevation = Self::parse_levels(levels, width, height, &ramps)?;
        if elevation.is_none()
            && let Some(&ramp) = ramps.first()
        {
            return Err(MapError::Untiered(ramp));
        }
        let map = Self {
            grid: Grid::from_cells(width, height, cells),
            extractor_frames,
            elevation,
        };

        if let Some(&frame) = map
            .extractor_frames
            .iter()
            .find(|&&frame| !map.frame_footprint_bare(frame))
        {
            return Err(MapError::BadFrame(frame));
        }
        if let Some(fault) = map.elevation_fault() {
            return Err(fault);
        }
        Ok((map, anchors))
    }

    /// The levels layer as a grid, or `None` when it is absent or holds a
    /// single level with no ramps: a flat map has only one form.
    fn parse_levels(
        levels: &[impl AsRef<str>],
        width: i32,
        height: i32,
        ramps: &[TilePos],
    ) -> Result<Option<Grid<Elevation>>, MapError> {
        if levels.is_empty() {
            return Ok(None);
        }
        let sized = levels.len() == as_index(height)
            && levels
                .iter()
                .all(|row| row.as_ref().chars().count() == as_index(width));
        if !sized {
            return Err(MapError::LevelRows { width, height });
        }
        let mut cells = Vec::with_capacity(as_index(width) * as_index(height));
        for (y, row) in levels.iter().enumerate() {
            for (x, c) in row.as_ref().chars().enumerate() {
                let level = (c.is_ascii_digit() || c.is_ascii_lowercase())
                    .then(|| c.to_digit(36))
                    .flatten()
                    .ok_or(MapError::UnknownLevel {
                        c,
                        x: edge_coordinate(x),
                        y: edge_coordinate(y),
                    })?;
                cells.push(Elevation::Level(
                    u8::try_from(level).expect("base-36 digits fit a byte"),
                ));
            }
        }
        for ramp in ramps {
            let cell = &mut cells[ramp.row_major(width)];
            let (low, _) = cell.span();
            *cell = Elevation::Ramp { low };
        }
        let uniform = cells.windows(2).all(|pair| pair[0] == pair[1]);
        if uniform && matches!(cells.first(), Some(Elevation::Level(_))) {
            return Ok(None);
        }
        Ok(Some(Grid::from_cells(width, height, cells)))
    }

    /// The first way the levels fail to hold together, if any. A flat map
    /// has no cliffs; a tiered map's layer matches the map, holds more than
    /// one level, keeps ramps on ground, joins touching ground only where
    /// their levels overlap, and never puts a cliff below the ground beside
    /// it.
    fn elevation_fault(&self) -> Option<MapError> {
        let Some(levels) = &self.elevation else {
            return self
                .grid
                .iter()
                .find(|(_, tile)| tile.terrain == Terrain::Cliff)
                .map(|(pos, _)| MapError::Untiered(pos));
        };
        if !levels.is_consistent()
            || levels.width() != self.width()
            || levels.height() != self.height()
        {
            return Some(MapError::LevelRows {
                width: self.width(),
                height: self.height(),
            });
        }
        let first = levels.get(TilePos::new(0, 0)).copied();
        if matches!(first, Some(Elevation::Level(_)))
            && levels.iter().all(|(_, level)| Some(*level) == first)
        {
            return Some(MapError::FlatLevels);
        }
        for (pos, &here) in levels.iter() {
            let terrain = self.tile(pos)?.terrain;
            if let Elevation::Ramp { low } = here
                && (low == u8::MAX || terrain != Terrain::Ground)
            {
                return Some(MapError::LevelBreak(pos));
            }
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let near = pos.offset(dx, dy);
                    let (Some(tile), Some(&there)) = (self.tile(near), levels.get(near)) else {
                        continue;
                    };
                    let both_ground = terrain == Terrain::Ground && tile.terrain == Terrain::Ground;
                    let below_cliff = terrain == Terrain::Cliff
                        && tile.terrain == Terrain::Ground
                        && there.span().0 < here.span().0;
                    if (both_ground && !here.overlaps(there)) || below_cliff {
                        return Some(MapError::LevelBreak(pos));
                    }
                }
            }
        }
        None
    }

    /// Whether the deserialized grid holds together (see
    /// [`chassis::grid::Grid::is_consistent`]), levels included.
    pub fn is_consistent(&self) -> bool {
        self.grid.is_consistent()
            && self.elevation_fault().is_none()
            // Frames are a canonical row-major list. Fog views copy it
            // in order, so a reordered or duplicated list would give one
            // semantic map two hashes.
            && self
                .extractor_frames
                .windows(2)
                .all(|pair| (pair[0].y, pair[0].x) < (pair[1].y, pair[1].x))
            // The full parse-side contract. A deserialized frame on a scrap
            // node would let restoration and harvesting stack on one tile, a
            // shape parse refuses and loading must refuse too.
            && self
                .extractor_frames
                .iter()
                .all(|&frame| self.frame_footprint_bare(frame))
    }

    /// Whether a frame at `frame` stands on bare ground: no scrap under it
    /// and no ramp, where an Extractor could never be rebuilt.
    fn frame_footprint_bare(&self, frame: TilePos) -> bool {
        (0..2).all(|dy| {
            (0..2).all(|dx| {
                let tile = frame.offset(dx, dy);
                self.tile(tile)
                    .is_some_and(|t| t.terrain == Terrain::Ground && t.scrap == 0)
                    && !self.is_ramp(tile)
            })
        })
    }

    /// Anchors of derelict Extractor frames, in authored (row-major)
    /// order.
    pub fn extractor_frames(&self) -> &[TilePos] {
        &self.extractor_frames
    }

    /// Whether `anchor` is exactly a derelict frame's anchor.
    pub fn is_extractor_frame(&self, anchor: TilePos) -> bool {
        self.extractor_frames.contains(&anchor)
    }

    /// Whether `tile` lies under any frame's 2x2 footprint.
    pub fn tile_in_extractor_frame(&self, tile: TilePos) -> bool {
        self.extractor_frames.iter().any(|frame| {
            tile.x >= frame.x && tile.x < frame.x + 2 && tile.y >= frame.y && tile.y < frame.y + 2
        })
    }

    /// The elevation at `pos` on a tiered map; `None` on a flat map or off
    /// the map.
    pub fn elevation(&self, pos: TilePos) -> Option<Elevation> {
        self.elevation.as_ref()?.get(pos).copied()
    }

    /// Whether `pos` is a ramp.
    pub fn is_ramp(&self, pos: TilePos) -> bool {
        matches!(self.elevation(pos), Some(Elevation::Ramp { .. }))
    }

    /// Whether bodies standing on `a` and `b` share a level, as a blast
    /// needs to reach one from the other. Always true on a flat map.
    pub fn shares_level(&self, a: TilePos, b: TilePos) -> bool {
        match (self.elevation(a), self.elevation(b)) {
            (Some(a), Some(b)) => a.overlaps(b),
            _ => true,
        }
    }

    /// Whether the map has levels.
    pub fn tiered(&self) -> bool {
        self.elevation.is_some()
    }

    /// The levels layer in its authored glyphs; empty for a flat map. A
    /// level past the last glyph, which no scenario can author, prints as
    /// `?`.
    pub fn level_rows(&self) -> Vec<String> {
        let Some(levels) = &self.elevation else {
            return Vec::new();
        };
        let mut rows = vec![String::with_capacity(as_index(self.width())); as_index(self.height())];
        for (pos, elevation) in levels.iter() {
            let (level, _) = elevation.span();
            let glyph = char::from_digit(u32::from(level), 36).unwrap_or('?');
            rows[as_index(pos.y)].push(glyph);
        }
        rows
    }

    /// Map width in tiles.
    pub fn width(&self) -> i32 {
        self.grid.width()
    }

    /// Map height in tiles.
    pub fn height(&self) -> i32 {
        self.grid.height()
    }

    /// Clamps a world position into the flight envelope: at least half a
    /// tile inside every edge, so the containing tile is always in
    /// bounds. Aircraft slide along it and shells land inside it; it is
    /// the world's outer wall.
    pub fn clamp_to_envelope(&self, p: chassis::fx::Vec2Fx) -> chassis::fx::Vec2Fx {
        let half = chassis::fx::HALF;
        let max_x = chassis::fx::Fx::from_num(self.width()) - half;
        let max_y = chassis::fx::Fx::from_num(self.height()) - half;
        chassis::fx::Vec2Fx::new(p.x.clamp(half, max_x), p.y.clamp(half, max_y))
    }

    /// The tile at `pos`, if in bounds.
    pub fn tile(&self, pos: TilePos) -> Option<&Tile> {
        self.grid.get(pos)
    }

    /// Whether terrain alone allows standing on `pos` (buildings are
    /// [`crate::State`]'s concern).
    pub fn terrain_passable(&self, pos: TilePos) -> bool {
        self.grid
            .get(pos)
            .is_some_and(|t| !t.terrain.blocks_ground() && t.scrap == 0)
    }

    /// The raw tile grid — row-slice access for hot scans (vision's
    /// memory reconciliation); everything else should prefer the
    /// per-tile accessors.
    pub(crate) fn grid(&self) -> &Grid<Tile> {
        &self.grid
    }

    /// Remaining scrap at `pos` (zero when out of bounds).
    pub fn scrap_at(&self, pos: TilePos) -> u32 {
        self.grid.get(pos).map_or(0, |t| t.scrap)
    }

    /// Removes one scrap from the node at `pos`. Returns the amount left,
    /// or `None` if there was nothing to extract.
    pub fn extract_scrap(&mut self, pos: TilePos) -> Option<u32> {
        let tile = self.grid.get_mut(pos)?;
        if tile.scrap == 0 {
            return None;
        }
        tile.scrap -= 1;
        Some(tile.scrap)
    }

    /// Battlefield salvage at `pos` (zero when out of bounds).
    pub fn wreck_at(&self, pos: TilePos) -> u32 {
        self.grid.get(pos).map_or(0, |t| t.wreck)
    }

    /// Deposits salvage on ground terrain, including live scrap node tiles,
    /// which become standable once the node exhausts. Rock, peaks, and pits
    /// swallow it.
    pub(crate) fn add_wreck(&mut self, pos: TilePos, amount: u32) {
        if let Some(tile) = self.grid.get_mut(pos)
            && tile.terrain == Terrain::Ground
        {
            tile.wreck = tile.wreck.saturating_add(amount);
        }
    }

    /// Removes one salvage from the wreck at `pos`. Returns the amount
    /// left, or `None` if there was nothing to strip.
    pub fn extract_wreck(&mut self, pos: TilePos) -> Option<u32> {
        let tile = self.grid.get_mut(pos)?;
        if tile.wreck == 0 {
            return None;
        }
        tile.wreck -= 1;
        Some(tile.wreck)
    }

    /// One decay step: every wreck tile loses one salvage. Runs on a
    /// global cadence so battlefield scrap eventually returns to the
    /// dirt without per-tile timers in the hash.
    pub(crate) fn decay_wrecks(&mut self) {
        for (_, tile) in self.grid.iter_mut() {
            tile.wreck = tile.wreck.saturating_sub(1);
        }
    }

    /// Erases wreck salvage when construction starts on the tile.
    pub(crate) fn clear_wreck(&mut self, pos: TilePos) {
        if let Some(tile) = self.grid.get_mut(pos) {
            tile.wreck = 0;
        }
    }

    /// Iterates all tiles row-major.
    pub fn iter(&self) -> impl Iterator<Item = (TilePos, &Tile)> {
        self.grid.iter()
    }

    /// Renders terrain back to the ASCII legend (buildings not included —
    /// callers overlay entities as needed).
    pub fn ascii_rows(&self) -> Vec<String> {
        let mut rows = vec![String::with_capacity(as_index(self.width())); as_index(self.height())];
        for (pos, tile) in self.grid.iter() {
            let c = if self.extractor_frames.contains(&pos) {
                'E'
            } else {
                match (tile.terrain, tile.scrap) {
                    (Terrain::Rock, _) => '#',
                    (Terrain::Peak, _) => '^',
                    (Terrain::Pit, _) => '~',
                    (Terrain::Cliff, _) => '=',
                    // Render-only: wrecks are never authored, so `w` stays
                    // out of the parse legend.
                    (Terrain::Ground, 0) if tile.wreck > 0 => 'w',
                    (Terrain::Ground, 0) if self.is_ramp(pos) => '/',
                    (Terrain::Ground, 0) if tile.cosmetic == 1 => ',',
                    (Terrain::Ground, 0) => '.',
                    (Terrain::Ground, s) if s > SCRAP_NODE_AMOUNT => 'S',
                    (Terrain::Ground, _) => 's',
                }
            };
            rows[as_index(pos.y)].push(c);
        }
        rows
    }
}

#[cfg(test)]
mod tests;
