//! Fog of war: per-player visibility.
//!
//! Each player owns two boolean grids. `visible` is recomputed from scratch
//! every tick — the union of vision discs around that player's units and
//! buildings. `explored` only ever accumulates. Vision is radius-based;
//! rocks do not block line of sight (a deliberate simplification, cheap and
//! predictable).
//!
//! Fog is both a presentation surface and the knowledge boundary for built-in
//! opponents. Fog-honest views expose current sight, explored terrain, ghosts,
//! remembered salvage, radar contacts that tell air from ground but name nothing
//! else, and the public buildings every
//! seat knows of. A targeted attack needs current team sight, a remembered or
//! public building, or a live contact track.

use crate::ids::PlayerId;
use crate::state::State;
use crate::stats::{BuildingKind, Domain};
use chassis::fx::{Fx, HALF, Vec2Fx};
use chassis::grid::{CARDINALS, Grid, TilePos, as_index};
use chassis::path::AstarScratch;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, OnceCell, RefCell};

mod tracking;
pub use tracking::{ContactSample, ContactTrack};

/// A radar blip: a tile holding an unseen hostile machine or one tile of an
/// unseen hostile building, and whether the return is airborne. Radar names
/// no kind, owner or condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadarContact {
    /// The reported tile.
    pub tile: TilePos,
    /// Air for a flying machine; ground for a landed one or a building.
    pub domain: Domain,
}

impl RadarContact {
    /// The canonical blip order: by tile, then ground before air.
    pub fn sort_key(&self) -> (i32, i32, Domain) {
        (self.tile.y, self.tile.x, self.domain)
    }
}

/// A building every seat knows of regardless of fog: where it stands, what it
/// is and whose it is. Its condition stays hidden, except an assembly
/// cradle's progress, which every seat watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicBuilding {
    /// Its owner.
    pub owner: PlayerId,
    /// Its kind.
    pub kind: BuildingKind,
    /// Its tier.
    pub tier: u8,
    /// Its footprint's top-left tile.
    pub anchor: TilePos,
    /// For an assembly cradle, the construction work done toward its kind's
    /// build time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u32>,
}

impl PublicBuilding {
    /// The attack target that names it.
    pub fn target(self) -> crate::ids::RememberedBuilding {
        crate::ids::RememberedBuilding {
            owner: self.owner,
            building_kind: self.kind,
            anchor: self.anchor,
        }
    }

    /// Whether its footprint covers `tile`.
    pub fn covers(self, tile: TilePos) -> bool {
        let (width, height) = self.kind.size();
        tile.x >= self.anchor.x
            && tile.x < self.anchor.x + width
            && tile.y >= self.anchor.y
            && tile.y < self.anchor.y + height
    }
}

/// A remembered enemy building: what its ground looked like the last time
/// this player saw it. Ghosts are beliefs, not facts — the building may be
/// long gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhostBuilding {
    /// Building type as last seen.
    pub kind: BuildingKind,
    /// Whose building it was.
    pub owner: PlayerId,
    /// Footprint anchor.
    pub anchor: TilePos,
    /// Hit points at last sighting.
    pub hp: u32,
    /// Whether construction had finished at last sighting — a scouted
    /// scaffold stays a scaffold in memory until seen complete.
    #[serde(
        default = "ghost_built_default",
        skip_serializing_if = "core::clone::Clone::clone"
    )]
    pub built: bool,
    /// Upgrade tier as last seen.
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub tier: u8,
}

fn ghost_built_default() -> bool {
    true
}

impl GhostBuilding {
    pub(crate) fn footprint(&self) -> impl Iterator<Item = TilePos> + use<> {
        let (w, h) = self.kind.size();
        let anchor = self.anchor;
        (0..h).flat_map(move |dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
    }
}

/// A recent hostile hit remembered by the team that suffered it.
///
/// Only the allied victim's tile is retained. The record deliberately does
/// not identify or locate the attacker, so artillery landing from fog adds
/// caution without turning damage into reconnaissance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SalvageIncident {
    /// Where the allied asset stood when damage landed.
    pub(crate) tile: TilePos,
    /// First tick on which this caution zone no longer applies.
    pub(crate) expires_at: crate::Tick,
}

/// One player's view of the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vision {
    visible: Grid<bool>,
    explored: Grid<bool>,
    /// Remembered enemy buildings, sorted by (anchor.y, anchor.x, owner).
    /// The owner is part of the key because two hostile seats can leave
    /// memories recorded under the same corner.
    ghosts: Vec<GhostBuilding>,
    /// Scrap per tile as this player last saw it. Only meaningful where
    /// `explored`; frozen wherever sight is lost, exactly like ghosts.
    remembered_scrap: Grid<u32>,
    /// Wreck salvage per tile as last seen — same freeze-frame rule. Kept
    /// apart from scrap memory because renderers draw them differently
    /// and the harvest brain approaches them differently.
    remembered_wreck: Grid<u32>,
    /// Radar blips: hostile units and one tile of each hostile building
    /// inside an allied radar ring but outside true sight. A contact carries
    /// its domain but no kind, owner or memory (rebuilt every tick).
    contacts: Vec<RadarContact>,
    tracking: tracking::Tracking,
    /// Recent tiles where this team saw one of its own assets take damage.
    /// Sorted and deduplicated by (y, x).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    salvage_incidents: Vec<SalvageIncident>,
    /// Hostile warheads in flight this team has had in sight or radar,
    /// sorted by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warheads: Vec<WarheadTrack>,
}

/// Exposure at which a team's batteries have locked on to a warhead.
const LOCKED: u8 = crate::stats::WARHEAD_TRACK_TICKS + crate::stats::WARHEAD_LOCK_TICKS;

/// A team's hold on one hostile warhead in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarheadTrack {
    /// The warhead.
    pub id: crate::WarheadId,
    /// Ticks the team has held it in sight or radar, capped once its
    /// batteries have locked on: [`WARHEAD_TRACK_TICKS`] to track it, then
    /// [`WARHEAD_LOCK_TICKS`] more.
    ///
    /// [`WARHEAD_TRACK_TICKS`]: crate::stats::WARHEAD_TRACK_TICKS
    /// [`WARHEAD_LOCK_TICKS`]: crate::stats::WARHEAD_LOCK_TICKS
    pub exposure: u8,
    /// Whether it was in sight or radar at the last refresh.
    pub in_view: bool,
}

impl WarheadTrack {
    /// Whether the team has held it long enough to know its line.
    pub fn tracked(&self) -> bool {
        self.exposure >= crate::stats::WARHEAD_TRACK_TICKS
    }

    /// Whether the team's batteries have locked on and may stop it.
    pub fn locked(&self) -> bool {
        self.exposure >= LOCKED
    }

    /// Ticks in view still needed before the team's batteries lock on.
    pub(crate) fn lock_remaining(self) -> u8 {
        LOCKED.saturating_sub(self.exposure)
    }
}

impl Vision {
    pub(crate) fn forget_building(
        &mut self,
        owner: PlayerId,
        kind: crate::stats::BuildingKind,
        anchor: TilePos,
    ) {
        self.ghosts
            .retain(|g| g.owner != owner || g.kind != kind || g.anchor != anchor);
    }

    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self {
            visible: Grid::new(width, height, false),
            explored: Grid::new(width, height, false),
            ghosts: Vec::new(),
            remembered_scrap: Grid::new(width, height, 0),
            remembered_wreck: Grid::new(width, height, 0),
            contacts: Vec::new(),
            tracking: tracking::Tracking::default(),
            salvage_incidents: Vec::new(),
            warheads: Vec::new(),
        }
    }

    /// Enemy buildings as last observed. Records freeze when sight is lost
    /// or a completed mine becomes concealed, even on visible ground.
    /// Draw live state only for currently observable buildings; otherwise
    /// retain this last-seen marker without exposing current condition.
    pub fn ghosts(&self) -> &[GhostBuilding] {
        &self.ghosts
    }

    /// Whether the deserialized view holds together against the map it
    /// claims to describe — see [`crate::State::validate_invariants`].
    pub fn is_consistent(&self, width: i32, height: i32) -> bool {
        let dims = |w: i32, h: i32, ok: bool| ok && w == width && h == height;
        dims(
            self.visible.width(),
            self.visible.height(),
            self.visible.is_consistent(),
        ) && dims(
            self.explored.width(),
            self.explored.height(),
            self.explored.is_consistent(),
        ) && dims(
            self.remembered_scrap.width(),
            self.remembered_scrap.height(),
            self.remembered_scrap.is_consistent(),
        ) && dims(
            self.remembered_wreck.width(),
            self.remembered_wreck.height(),
            self.remembered_wreck.is_consistent(),
        )
    }

    /// Scrap at `pos` as last seen (zero where never seen or out of
    /// bounds). Renderers should use live amounts on visible ground and
    /// this everywhere else.
    pub fn remembered_scrap(&self, pos: TilePos) -> u32 {
        self.remembered_scrap.get(pos).copied().unwrap_or(0)
    }

    /// Wreck salvage at `pos` as last seen (zero where never seen or out
    /// of bounds). Decay keeps running in the fog — this is a belief.
    pub fn remembered_wreck(&self, pos: TilePos) -> u32 {
        self.remembered_wreck.get(pos).copied().unwrap_or(0)
    }

    /// Radar blips in [`RadarContact::sort_key`] order, deduplicated,
    /// rebuilt every tick. One tile can hold an air and a ground blip.
    pub fn contacts(&self) -> &[RadarContact] {
        &self.contacts
    }

    /// Continuous mobile contacts, including identified units in true sight.
    pub fn tracks(&self) -> &[ContactTrack] {
        &self.tracking.tracks
    }

    /// A live observation identity; lost identities never resolve again.
    pub fn track(&self, id: crate::ContactId) -> Option<&ContactTrack> {
        self.tracks()
            .binary_search_by_key(&id, |track| track.id)
            .ok()
            .map(|index| &self.tracks()[index])
    }

    pub(crate) fn tracking_valid(&self, state: &State, player: PlayerId) -> bool {
        self.tracking.valid(self, state, player)
    }

    pub(crate) fn shares_tracking(&self, other: &Self) -> bool {
        self.tracking == other.tracking
    }

    pub(crate) fn minted_contact(&self, id: crate::ContactId) -> bool {
        id.0 < self.tracking.next_id
    }

    pub(crate) fn salvage_incidents(&self) -> &[SalvageIncident] {
        &self.salvage_incidents
    }

    pub(crate) fn remember_salvage_incident(&mut self, tile: TilePos, expires_at: crate::Tick) {
        let key = (tile.y, tile.x);
        match self
            .salvage_incidents
            .binary_search_by_key(&key, |incident| (incident.tile.y, incident.tile.x))
        {
            Ok(index) => {
                self.salvage_incidents[index].expires_at =
                    self.salvage_incidents[index].expires_at.max(expires_at);
            }
            Err(mut index) => {
                if self.salvage_incidents.len() == crate::stats::HARVEST_INCIDENT_CAP {
                    let evict = self
                        .salvage_incidents
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, incident)| {
                            (incident.expires_at, incident.tile.y, incident.tile.x)
                        })
                        .map(|(index, _)| index)
                        .expect("a full incident table is nonempty");
                    self.salvage_incidents.remove(evict);
                    index = self
                        .salvage_incidents
                        .binary_search_by_key(&key, |incident| (incident.tile.y, incident.tile.x))
                        .unwrap_or_else(|index| index);
                }
                self.salvage_incidents
                    .insert(index, SalvageIncident { tile, expires_at });
            }
        }
    }

    fn prune_salvage_incidents(&mut self, tick: crate::Tick) {
        self.salvage_incidents
            .retain(|incident| incident.expires_at > tick);
    }

    /// Whether the player currently sees `pos`.
    pub fn visible(&self, pos: TilePos) -> bool {
        self.visible.get(pos).copied().unwrap_or(false)
    }

    /// Whether any tile `unit` claims is in sight: its centre tile for a
    /// small body, any tile of a wider hull, so a machine half out of the
    /// fog is seen.
    pub fn sees_unit(&self, unit: &crate::state::Unit) -> bool {
        if unit.kind.stats().radius <= crate::stats::SMALL_BODY_RADIUS {
            return self.visible(unit.tile());
        }
        unit.body_tiles().any(|t| self.visible(t))
    }

    /// Drops one tile from current sight until the next vision refresh.
    #[cfg(test)]
    pub(crate) fn conceal(&mut self, pos: TilePos) {
        if let Some(visible) = self.visible.get_mut(pos) {
            *visible = false;
        }
    }

    /// Whether the player has ever seen `pos`.
    pub fn explored(&self, pos: TilePos) -> bool {
        self.explored.get(pos).copied().unwrap_or(false)
    }

    /// Copies `src` into `self` byte-for-byte while reusing this view's
    /// grid and vector allocations: the team-sight shortcut clones a whole
    /// view per teammate per tick, and a plain clone-assign would reallocate
    /// four map-sized grids each time. The exhaustive destructure makes a
    /// future field a compile error here instead of silently stale team
    /// sight.
    pub(crate) fn copy_from(&mut self, src: &Vision) {
        let Vision {
            visible,
            explored,
            ghosts,
            remembered_scrap,
            remembered_wreck,
            contacts,
            tracking,
            salvage_incidents,
            warheads,
        } = self;
        visible.copy_from(&src.visible);
        explored.copy_from(&src.explored);
        ghosts.clone_from(&src.ghosts);
        remembered_scrap.copy_from(&src.remembered_scrap);
        remembered_wreck.copy_from(&src.remembered_wreck);
        contacts.clone_from(&src.contacts);
        tracking.clone_from(&src.tracking);
        salvage_incidents.clone_from(&src.salvage_incidents);
        warheads.clone_from(&src.warheads);
    }

    /// This team's holds on hostile warheads in flight, sorted by id.
    pub fn warheads(&self) -> &[WarheadTrack] {
        &self.warheads
    }

    /// This team's hold on the warhead `id`, if it has ever seen it.
    pub fn warhead(&self, id: crate::WarheadId) -> Option<&WarheadTrack> {
        self.warheads
            .binary_search_by_key(&id, |track| track.id)
            .ok()
            .map(|index| &self.warheads[index])
    }

    /// Forgets every warhead, as a decided match does.
    pub(crate) fn forget_warheads(&mut self) {
        self.warheads.clear();
    }

    /// Rebuilds the holds over the warheads now in flight: one more tick
    /// of exposure for each hostile warhead whose tile is in sight or
    /// `on_radar`, and a hold dropped once its warhead is gone.
    ///
    /// A completed track gives the silo away: the line starts at its
    /// centre, so the team remembers a Spire there, intact for all it
    /// knows, until it sees that ground again.
    fn track_warheads(
        &mut self,
        state: &State,
        hostile: impl Fn(PlayerId) -> bool,
        on_radar: impl Fn(TilePos) -> bool,
    ) {
        let old = std::mem::take(&mut self.warheads);
        let mut revealed = false;
        for warhead in state.warheads.iter().filter(|w| hostile(w.player)) {
            let tile = TilePos::containing(warhead.position(state.tick));
            let in_view = self.visible(tile) || on_radar(tile);
            let held = old
                .binary_search_by_key(&warhead.id, |track| track.id)
                .ok()
                .map_or(0, |index| old[index].exposure);
            if held == 0 && !in_view {
                continue;
            }
            let track = WarheadTrack {
                id: warhead.id,
                exposure: (held + u8::from(in_view)).min(LOCKED),
                in_view,
            };
            if track.tracked() && held < crate::stats::WARHEAD_TRACK_TICKS {
                revealed |= self.remember_silo(state, warhead);
            }
            self.warheads.push(track);
        }
        if revealed {
            self.ghosts
                .sort_unstable_by_key(|g| (g.anchor.y, g.anchor.x, g.owner));
        }
    }

    /// Records the Spire `warhead` launched from, unless the team already
    /// remembers it or has its ground in sight, where memory reconciliation
    /// has just recorded what stands there. Returns whether it added a
    /// memory.
    ///
    /// The launch proves the Spire stood there, so every remembered
    /// building its footprint overlaps is gone, except a concealed charge
    /// of a seat hostile to the silo's owner: a hostile building may stand
    /// over one.
    fn remember_silo(&mut self, state: &State, warhead: &crate::state::Warhead) -> bool {
        let kind = BuildingKind::Spire;
        let (width, height) = kind.size();
        let silo = GhostBuilding {
            kind,
            owner: warhead.player,
            anchor: TilePos::containing(
                warhead.launch
                    - chassis::fx::Vec2Fx::new(
                        chassis::fx::Fx::from_num(width) / 2,
                        chassis::fx::Fx::from_num(height) / 2,
                    ),
            ),
            hp: kind.base_stats().max_hp,
            built: true,
            tier: 0,
        };
        if silo.footprint().any(|t| self.visible(t))
            || self
                .ghosts
                .iter()
                .any(|g| g.kind == kind && g.owner == silo.owner && g.anchor == silo.anchor)
        {
            return false;
        }
        let covered = |t: TilePos| {
            (silo.anchor.x..silo.anchor.x + width).contains(&t.x)
                && (silo.anchor.y..silo.anchor.y + height).contains(&t.y)
        };
        self.ghosts.retain(|g| {
            (g.kind.is_stealthy() && state.hostile(g.owner, silo.owner))
                || !g.footprint().any(covered)
        });
        self.ghosts.push(silo);
        true
    }

    /// Row slices for the observation builder's full-map walk, instead of
    /// four bounds-checked point lookups per tile.
    pub(crate) fn rows(&self, y: i32) -> Option<VisionRows<'_>> {
        Some((
            self.visible.row(y)?,
            self.explored.row(y)?,
            self.remembered_scrap.row(y)?,
            self.remembered_wreck.row(y)?,
        ))
    }

    fn stamp_disc(&mut self, center: TilePos, radius: i32, coverage: &mut RowCoverage) {
        let spans = disc_spans(radius);
        for dy in -radius..=radius {
            let span = spans[dy.unsigned_abs() as usize];
            let y = center.y + dy;
            self.visible
                .fill_row_span(y, center.x - span, center.x + span, true);
            coverage.cover(y, center.x - span, center.x + span);
        }
    }

    /// Stamps the union of discs centered on every tile of a `w`x`h`
    /// footprint — the rectangle's Minkowski sum with the sight disc,
    /// written row by row. Cell-identical to stamping each footprint
    /// tile separately, without visiting the overlap four times.
    fn stamp_rect(
        &mut self,
        anchor: TilePos,
        w: i32,
        h: i32,
        radius: i32,
        coverage: &mut RowCoverage,
    ) {
        let spans = disc_spans(radius);
        for dy in -radius..(h + radius) {
            let vdist = (-dy).max(dy - (h - 1)).max(0);
            let span = spans[as_index(vdist)];
            let y = anchor.y + dy;
            self.visible
                .fill_row_span(y, anchor.x - span, anchor.x + (w - 1) + span, true);
            coverage.cover(y, anchor.x - span, anchor.x + (w - 1) + span);
        }
    }
}

/// Per-refresh record of which cells this team's sight stamps could have
/// touched: one bounding x-span per row. The memory-reconciliation walk
/// visits only these spans instead of the whole map. A bounding span may
/// include cells between two disjoint discs that are not actually visible —
/// the walk re-checks `visible` per cell, so coverage only ever bounds the
/// scan, never widens what counts as seen.
struct RowCoverage {
    /// `(min_x, max_x)` per row, clamped to the grid; `min > max` = untouched.
    bounds: Vec<(i32, i32)>,
    width: i32,
}

impl RowCoverage {
    fn new(width: i32, height: i32) -> Self {
        Self {
            bounds: vec![(i32::MAX, i32::MIN); as_index(height.max(0))],
            width,
        }
    }

    fn reset(&mut self) {
        self.bounds.fill((i32::MAX, i32::MIN));
    }

    fn cover(&mut self, y: i32, x0: i32, x1: i32) {
        let Some(entry) = usize::try_from(y).ok().and_then(|y| self.bounds.get_mut(y)) else {
            return;
        };
        let x0 = x0.max(0);
        let x1 = x1.min(self.width - 1);
        if x0 > x1 {
            return;
        }
        entry.0 = entry.0.min(x0);
        entry.1 = entry.1.max(x1);
    }
}

/// Whether the shared fog-honest view for `viewer` justifies treating a
/// ground salvage tile as dangerous.
///
/// This is the one threat-knowledge funnel used by autonomous Harvest
/// work. Live mobile enemies are consulted only when the viewer's
/// current team vision contains their tile, and nearby friendly ground
/// firepower can screen equal or weaker pressure. Static weapons come
/// from the vision's own building memories, and unidentified radar
/// contacts matter only in a tight local ring. Recent allied impact sites
/// remain as anonymous caution zones for a short cooldown; nothing here
/// remembers a mobile enemy's identity or position after sight is lost, or
/// reads an unseen live building.
#[derive(Debug, Clone, Copy)]
struct MobileGroundPressure {
    pos: Vec2Fx,
    reach_sq: Fx,
    strength: u64,
    hostile: bool,
}

#[derive(Debug, Clone, Copy)]
struct StaticGroundPressure {
    anchor: TilePos,
    size: (i32, i32),
    reach_sq: Fx,
}

/// One row of a view's four per-tile grids, in (visible, explored,
/// remembered scrap, remembered wreck) order — the observation
/// builder's bulk-read unit.
pub(crate) type VisionRows<'a> = (&'a [bool], &'a [bool], &'a [u32], &'a [u32]);

/// Memo-lane bits for [`GroundSalvageDanger::lanes`].
mod lane {
    /// Tile sits inside some incident danger ring (stamped at capture).
    pub const INCIDENT_NEAR: u16 = 1 << 0;
    /// The contains verdict has been computed.
    pub const CONTAINS_SET: u16 = 1 << 1;
    /// The memoized contains verdict.
    pub const CONTAINS: u16 = 1 << 2;
    /// The observed-contains verdict has been computed.
    pub const OBSERVED_SET: u16 = 1 << 3;
    /// The memoized observed-contains verdict.
    pub const OBSERVED: u16 = 1 << 4;
    /// The known-ground verdict has been computed.
    pub const GROUND_SET: u16 = 1 << 5;
    /// The memoized known-ground verdict.
    pub const GROUND: u16 = 1 << 6;
    /// The scout-danger verdict has been computed.
    pub const SCOUT_SET: u16 = 1 << 7;
    /// The memoized scout-danger verdict.
    pub const SCOUT: u16 = 1 << 8;
}

/// One player's immutable, fog-honest salvage-danger snapshot for the
/// brain phase. Capturing once makes every A* predicate a walk over compact
/// threat records instead of repeatedly rescanning the full game state.
pub(crate) struct GroundSalvageDanger {
    width: i32,
    height: i32,
    contacts: Vec<TilePos>,
    incidents: Vec<TilePos>,
    /// Remembered hostile Scuttle Charge anchors, which only Scout routing
    /// steers around.
    charges: Vec<TilePos>,
    mobile: Vec<MobileGroundPressure>,
    statics: Vec<StaticGroundPressure>,
    building_blocks: Vec<Vec<(i32, i32)>>,
    /// Per coarse cell, the threat records whose reach could cover a tile
    /// in it; built from the captured lists on the first probe.
    threat_cells: OnceCell<ThreatCells>,
    /// Memo lanes per tile; the A* predicates probe these once
    /// per neighbor. The incident-near stamp is set at capture (the
    /// incident rule depends on the mover's origin, so only the
    /// outside-every-ring case caches); the contains and observed verdicts
    /// memoize on first probe; known-ground memoizes except its volatile
    /// arm (a visible tile with live scrap), which is served uncached.
    lanes: Vec<Cell<u16>>,
    path_scratch: RefCell<AstarScratch>,
    /// What failed safe searches from outside every envelope reached this
    /// phase; see [`Self::safe_route_impossible`].
    safe_proofs: RefCell<Vec<SafeProof>>,
    #[cfg(test)]
    route_searches: Cell<usize>,
}

impl GroundSalvageDanger {
    /// Captures the threat knowledge that stays fixed throughout one brain
    /// phase. Unit damage is buffered and positions move afterward, so the
    /// snapshot is exact for every Harvester decision in that phase.
    pub(crate) fn capture(state: &State, viewer: PlayerId) -> Self {
        let vision = state.vision(viewer);
        let mobile = state
            .units
            .iter()
            .filter_map(|unit| {
                let hostile = state.hostile(viewer, unit.player);
                if hostile && !vision.sees_unit(unit) {
                    return None;
                }
                let range = unit
                    .kind
                    .stats()
                    .max_range_vs(crate::stats::Domain::Ground)?
                    + crate::stats::HARVEST_MOBILE_DANGER_MARGIN;
                let stats = unit.kind.stats();
                Some(MobileGroundPressure {
                    pos: unit.pos,
                    reach_sq: range * range,
                    strength: u64::from(stats.cost).saturating_mul(u64::from(unit.hp))
                        / u64::from(stats.max_hp),
                    hostile,
                })
            })
            .collect();
        let statics = vision
            .ghosts()
            .iter()
            .filter(|ghost| ghost.built)
            .filter_map(|ghost| {
                let range = ground_weapon_reach(ghost.kind.tier_stats(ghost.tier).weapons)?
                    + crate::stats::HARVEST_STATIC_DANGER_MARGIN;
                Some(StaticGroundPressure {
                    anchor: ghost.anchor,
                    size: ghost.kind.size(),
                    reach_sq: range * range,
                })
            })
            .collect();
        let mut building_blocks = vec![Vec::new(); as_index(state.map.height())];
        let viewer_team = state.player(viewer).team;
        for building in state
            .buildings
            .iter()
            .filter(|building| !building.kind.is_stealthy() && !building.provisional())
        {
            if state.player(building.player).team == viewer_team {
                stamp_blocked_rect(
                    &mut building_blocks,
                    state.map.width(),
                    building.anchor,
                    building.kind.size(),
                );
            } else {
                // A hostile structure placed during this tick's command
                // phase is not in the previous tick's ghost table yet.
                // Its currently visible tiles are still live truth; its
                // unseen footprint must remain unknown until vision
                // refresh records it.
                for tile in building.tiles().filter(|tile| vision.visible(*tile)) {
                    stamp_blocked_span(
                        &mut building_blocks,
                        state.map.width(),
                        tile.y,
                        tile.x,
                        tile.x,
                    );
                }
            }
        }
        for ghost in vision
            .ghosts()
            .iter()
            .filter(|ghost| !ghost.kind.is_stealthy())
        {
            stamp_blocked_rect(
                &mut building_blocks,
                state.map.width(),
                ghost.anchor,
                ghost.kind.size(),
            );
        }
        for row in &mut building_blocks {
            merge_spans(row);
        }
        let cell_count = chassis::grid::cell_count(state.map.width(), state.map.height());
        let incidents: Vec<TilePos> = vision
            .salvage_incidents()
            .iter()
            .filter(|incident| incident.expires_at > state.tick)
            .map(|incident| incident.tile)
            .collect();
        let (width, height) = (state.map.width(), state.map.height());
        let lanes: Vec<Cell<u16>> = std::iter::repeat_with(|| Cell::new(0))
            .take(cell_count)
            .collect();
        let radius = crate::stats::HARVEST_INCIDENT_DANGER_RADIUS;
        for incident in &incidents {
            for y in (incident.y - radius).max(0)..=(incident.y + radius).min(height - 1) {
                for x in (incident.x - radius).max(0)..=(incident.x + radius).min(width - 1) {
                    let cell = &lanes[TilePos::new(x, y).row_major(width)];
                    cell.set(cell.get() | lane::INCIDENT_NEAR);
                }
            }
        }
        let charges = vision
            .ghosts()
            .iter()
            .filter(|ghost| ghost.kind.is_stealthy())
            .map(|ghost| ghost.anchor)
            .collect();
        Self {
            width,
            height,
            contacts: {
                let mut tiles: Vec<TilePos> = vision
                    .contacts()
                    .iter()
                    .map(|contact| contact.tile)
                    .collect();
                tiles.dedup();
                tiles
            },
            incidents,
            charges,
            mobile,
            statics,
            building_blocks,
            threat_cells: OnceCell::new(),
            lanes,
            path_scratch: RefCell::new(AstarScratch::default()),
            safe_proofs: RefCell::new(Vec::new()),
            #[cfg(test)]
            route_searches: Cell::new(0),
        }
    }

    /// The threat cells, built on first use: many snapshots are never
    /// probed at all.
    fn threat_cells(&self) -> &ThreatCells {
        self.threat_cells.get_or_init(|| self.index_threats())
    }

    /// Files every threat record under each coarse cell its reach could
    /// cover. Each box is a conservative superset of the tiles whose centers
    /// the record's exact test can accept, so a probe that walks only its
    /// cell's records reaches the same verdict as a walk over all of them.
    fn index_threats(&self) -> ThreatCells {
        let (columns, rows) = (danger_cells(self.width), danger_cells(self.height));
        let radar = crate::stats::HARVEST_RADAR_DANGER_RADIUS;
        let contacts = CellLists::build(
            columns,
            rows,
            self.contacts
                .iter()
                .map(|&contact| (contact.offset(-radar, -radar), contact.offset(radar, radar))),
        );
        let mobile = CellLists::build(
            columns,
            rows,
            self.mobile.iter().map(|pressure| {
                let home = TilePos::containing(pressure.pos);
                let reach = tile_reach(pressure.reach_sq) + 1;
                (home.offset(-reach, -reach), home.offset(reach, reach))
            }),
        );
        let statics = CellLists::build(
            columns,
            rows,
            self.statics.iter().map(|pressure| {
                let reach = tile_reach(pressure.reach_sq);
                (
                    pressure.anchor.offset(-reach - 1, -reach - 1),
                    pressure
                        .anchor
                        .offset(pressure.size.0 + reach, pressure.size.1 + reach),
                )
            }),
        );
        let near = crate::stats::SCOUT_CHARGE_AVOID_RADIUS;
        let charges = CellLists::build(
            columns,
            rows,
            self.charges
                .iter()
                .map(|&charge| (charge.offset(-near, -near), charge.offset(near, near))),
        );
        ThreatCells {
            contacts,
            mobile,
            statics,
            charges,
        }
    }

    /// The coarse cell holding `tile`; off-map tiles share the nearest
    /// border cell, as off-map record boxes do.
    fn cell_of(&self, tile: TilePos) -> usize {
        let columns = danger_cells(self.width);
        let column = tile.x.div_euclid(DANGER_CELL).clamp(0, columns - 1);
        let row = tile
            .y
            .div_euclid(DANGER_CELL)
            .clamp(0, danger_cells(self.height) - 1);
        as_index(row * columns + column)
    }

    /// Whether this snapshot marks one tile as too dangerous for
    /// autonomous salvage work.
    pub(crate) fn contains(&self, source: TilePos) -> bool {
        self.lane_memo(source, lane::CONTAINS_SET, lane::CONTAINS, || {
            self.compute_contains(source)
        })
    }

    /// Whether an autonomous route may traverse `tile` from its current
    /// planning origin. Live threats and radar remain hard barriers. A worker
    /// inside remembered static fire or an incident ring may move laterally
    /// or outward; a worker outside cannot enter either envelope.
    pub(crate) fn route_safe_from(&self, from: TilePos, tile: TilePos) -> bool {
        // One bounds test and one byte load serve both the observed
        // memo and the incident-near stamp.
        let index = self.lane_index(tile);
        let bits = index.map_or(0, |i| self.lanes[i].get());
        let observed = self.observed_at(tile, index, bits);
        if observed && self.mobile_or_radar_contains(tile) {
            return false;
        }
        if observed {
            let from_point = from.center();
            let next_point = tile.center();
            let statics = self.threat_cells().statics.at(self.cell_of(tile));
            if statics
                .iter()
                .map(|&i| &self.statics[i as usize])
                .any(|pressure| {
                    let next_distance =
                        rect_closest_point(pressure.anchor, pressure.size, next_point)
                            .dist_sq(next_point);
                    next_distance <= pressure.reach_sq
                        && next_distance
                            < rect_closest_point(pressure.anchor, pressure.size, from_point)
                                .dist_sq(from_point)
                })
            {
                return false;
            }
        }
        if index.is_some() && bits & lane::INCIDENT_NEAR == 0 {
            return true;
        }
        !self.incidents.iter().any(|incident| {
            let next_distance = incident.chebyshev(tile);
            next_distance <= crate::stats::HARVEST_INCIDENT_DANGER_RADIUS
                && next_distance < incident.chebyshev(from)
        })
    }

    /// Memoized observed-danger test for one tile, given its lane index
    /// and current lane bits.
    fn observed_at(&self, tile: TilePos, index: Option<usize>, bits: u16) -> bool {
        if let Some(i) = index {
            if bits & lane::OBSERVED_SET != 0 {
                bits & lane::OBSERVED != 0
            } else {
                let value = self.compute_observed_contains(tile);
                let cell = &self.lanes[i];
                cell.set(cell.get() | lane::OBSERVED_SET | if value { lane::OBSERVED } else { 0 });
                value
            }
        } else {
            self.compute_observed_contains(tile)
        }
    }

    /// Whether a building occupies this tile in the viewer's shared
    /// knowledge. The row spans are captured once so every A* expansion
    /// avoids a full building and ghost scan.
    pub(crate) fn known_building_blocked(&self, tile: TilePos) -> bool {
        let Some(row) = usize::try_from(tile.y)
            .ok()
            .and_then(|row| self.building_blocks.get(row))
        else {
            return false;
        };
        let index = row.partition_point(|&(_, end)| end < tile.x);
        row.get(index)
            .is_some_and(|&(start, end)| tile.x >= start && tile.x <= end)
    }

    /// Runs one A* query while reusing this team phase's allocation storage.
    /// Brain phases are sequential, so one scratch arena serves every
    /// Harvester without entering deterministic state.
    pub(crate) fn find_route(
        &self,
        start: TilePos,
        goal: TilePos,
        passable: impl FnMut(TilePos) -> bool,
    ) -> Option<Vec<TilePos>> {
        #[cfg(test)]
        self.route_searches.set(self.route_searches.get() + 1);
        chassis::path::astar_with_scratch(
            self.width,
            self.height,
            start,
            goal,
            passable,
            crate::stats::PATH_EXPANSION_CAP,
            &mut self.path_scratch.borrow_mut(),
        )
    }

    /// Whether `from` lies outside every remembered static envelope and
    /// incident ring. From such an origin [`Self::route_safe_from`] refuses
    /// exactly the tiles [`Self::contains`] marks, so every safe search from
    /// one shares a single passability rule.
    pub(crate) fn outside_every_envelope(&self, from: TilePos) -> bool {
        let point = from.center();
        let in_static = self
            .threat_cells()
            .statics
            .at(self.cell_of(from))
            .iter()
            .map(|&i| &self.statics[i as usize])
            .any(|pressure| {
                rect_closest_point(pressure.anchor, pressure.size, point).dist_sq(point)
                    <= pressure.reach_sq
            });
        let in_ring = self.incidents.iter().any(|incident| {
            incident.chebyshev(from) <= crate::stats::HARVEST_INCIDENT_DANGER_RADIUS
        });
        !in_static && !in_ring
    }

    /// Keeps what the search that just failed reached, when it explored
    /// its whole component: every later safe search starting inside that
    /// component reaches exactly it. `may_drain` marks closed ground that
    /// could open later in this phase (a visible node with live scrap);
    /// such tiles beside the component are watched, since one opening could
    /// join more ground to it.
    ///
    /// Only a component covering at least an eighth of the map is kept: a
    /// proof costs a walk over the whole map, which a small component's
    /// repeat searches would not repay. That bounds the walk by eight times
    /// the search it follows; and since a search from inside a kept
    /// component never fails into a new proof, at most eight are kept at
    /// once.
    pub(crate) fn record_safe_failure(&self, may_drain: impl Fn(TilePos) -> bool) {
        let scratch = self.path_scratch.borrow();
        let area = chassis::grid::cell_count(self.width, self.height);
        if !scratch.last_search_exhausted() || (scratch.last_expansions() as usize) * 8 < area {
            return;
        }
        let mut reached = vec![0u64; area.div_ceil(64)];
        let mut watched = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let tile = TilePos::new(x, y);
                if !scratch.last_search_reached(tile) {
                    continue;
                }
                let i = self.lane_index(tile).expect("in bounds");
                reached[i / 64] |= 1 << (i % 64);
                for (dx, dy) in CARDINALS {
                    let next = tile.offset(dx, dy);
                    if !scratch.last_search_reached(next) && may_drain(next) {
                        watched.push(next);
                    }
                }
            }
        }
        self.safe_proofs
            .borrow_mut()
            .push(SafeProof { reached, watched });
    }

    /// Whether a recorded failure proves that a safe search from `from`, an
    /// origin outside every envelope, cannot reach `goal` (or, with
    /// `allow_goal_only`, any cardinal neighbor of it). A proof applies when
    /// it reached `from`, and lapses once `drained` reports a watched tile
    /// open. Adjacent goals are left to the search, which can step onto
    /// them from a closed origin.
    pub(crate) fn safe_route_impossible(
        &self,
        from: TilePos,
        goal: TilePos,
        allow_goal_only: bool,
        drained: impl Fn(TilePos) -> bool,
    ) -> bool {
        if from.chebyshev(goal) <= 1 {
            return false;
        }
        let mut proofs = self.safe_proofs.borrow_mut();
        proofs.retain(|proof| !proof.watched.iter().any(|&tile| drained(tile)));
        let Some(proof) = proofs
            .iter()
            .find(|proof| self.lane_index(from).is_some_and(|i| proof.reaches(i)))
        else {
            return false;
        };
        let reached = |tile: TilePos| self.lane_index(tile).is_some_and(|i| proof.reaches(i));
        !(reached(goal)
            || (allow_goal_only
                && CARDINALS
                    .into_iter()
                    .any(|(dx, dy)| reached(goal.offset(dx, dy)))))
    }

    #[cfg(test)]
    pub(crate) fn route_search_count(&self) -> usize {
        self.route_searches.get()
    }

    /// Whether the most recent route search explored its start's whole
    /// component before failing, so [`Self::last_route_reachability`] can
    /// answer for other goals.
    pub(crate) fn last_route_exhausted(&self) -> bool {
        self.path_scratch.borrow().last_search_exhausted()
    }

    /// Reachability of alternate goals proved by the most recent exhausted
    /// route search. `allow_goal_only` handles a goal-specific exception to
    /// the common passability predicate: an alternate goal can be entered iff
    /// the explored component reaches one of its cardinal neighbors. (A legal
    /// diagonal entry also makes a cardinal companion reachable.) `None`
    /// means the search did not explore the complete component.
    pub(crate) fn last_route_reachability(
        &self,
        goals: &[TilePos],
        allow_goal_only: bool,
    ) -> Option<Vec<bool>> {
        let scratch = self.path_scratch.borrow();
        scratch.last_search_exhausted().then(|| {
            goals
                .iter()
                .map(|goal| {
                    scratch.last_search_reached(*goal)
                        || (allow_goal_only
                            && CARDINALS
                                .into_iter()
                                .any(|(dx, dy)| scratch.last_search_reached(goal.offset(dx, dy))))
                })
                .collect()
        })
    }

    fn compute_contains(&self, source: TilePos) -> bool {
        if self.incidents.iter().any(|incident| {
            incident.chebyshev(source) <= crate::stats::HARVEST_INCIDENT_DANGER_RADIUS
        }) {
            return true;
        }
        self.compute_observed_contains(source)
    }

    fn compute_observed_contains(&self, source: TilePos) -> bool {
        if self.mobile_or_radar_contains(source) {
            return true;
        }
        let source_point = source.center();
        let statics = self.threat_cells().statics.at(self.cell_of(source));
        statics
            .iter()
            .map(|&i| &self.statics[i as usize])
            .any(|pressure| {
                rect_closest_point(pressure.anchor, pressure.size, source_point)
                    .dist_sq(source_point)
                    <= pressure.reach_sq
            })
    }

    fn mobile_or_radar_contains(&self, source: TilePos) -> bool {
        let cell = self.cell_of(source);
        let cells = self.threat_cells();
        if cells.contacts.at(cell).iter().any(|&i| {
            self.contacts[i as usize].chebyshev(source) <= crate::stats::HARVEST_RADAR_DANGER_RADIUS
        }) {
            return true;
        }
        // Strengths are non-negative, so a saturating sum over any subset
        // holding every in-reach pressure equals the sum over all of them.
        let source_point = source.center();
        let (mut hostile_strength, mut screen_strength) = (0u64, 0u64);
        for pressure in cells
            .mobile
            .at(cell)
            .iter()
            .map(|&i| &self.mobile[i as usize])
        {
            if pressure.pos.dist_sq(source_point) > pressure.reach_sq {
                continue;
            }
            if pressure.hostile {
                hostile_strength = hostile_strength.saturating_add(pressure.strength);
            } else {
                screen_strength = screen_strength.saturating_add(pressure.strength);
            }
        }
        hostile_strength > screen_strength
    }

    /// Whether a scout keeps off `tile`: inside the known reach of a visible
    /// armed enemy or a remembered defense, near a radar contact or a recent
    /// incident, or beside a known charge. Friendly screens do not count,
    /// so a lone scout never walks into fire because an army stands nearby.
    pub(crate) fn scout_contains(&self, tile: TilePos) -> bool {
        self.lane_memo(tile, lane::SCOUT_SET, lane::SCOUT, || {
            self.scout_threatened(tile, None)
        })
    }

    /// Whether a scout route planned from `from` may cross `tile`. Safe
    /// tiles always pass. A dangerous tile passes only when it lies no
    /// closer than `from` to every threat covering it, so a scout already in
    /// danger can slip sideways or away but never toward a gun, and a scout
    /// on safe ground never enters danger. A known charge is stricter: each
    /// step beside one must open the distance, because the follower cuts
    /// corners closer than the trigger radius.
    pub(crate) fn scout_route_safe_from(&self, from: TilePos, tile: TilePos) -> bool {
        !self.scout_contains(tile) || !self.scout_threatened(tile, Some(from))
    }

    /// Whether some threat covers `tile`; with `from`, only threats that
    /// `tile` is closer to than `from` is count.
    fn scout_threatened(&self, tile: TilePos, from: Option<TilePos>) -> bool {
        let approaches = |next: i64, origin: Option<i64>| origin.is_none_or(|was| next < was);
        let point = tile.center();
        let from_point = from.map(TilePos::center);
        let cell = self.cell_of(tile);
        let cells = self.threat_cells();
        let charge_radius = crate::stats::SCOUT_CHARGE_AVOID_RADIUS;
        if cells.charges.at(cell).iter().any(|&i| {
            let charge = self.charges[i as usize];
            let next = charge.chebyshev(tile);
            next <= charge_radius && from.is_none_or(|from| next <= charge.chebyshev(from))
        }) {
            return true;
        }
        let incident_radius = crate::stats::HARVEST_INCIDENT_DANGER_RADIUS;
        if self.incidents.iter().any(|incident| {
            let next = incident.chebyshev(tile);
            next <= incident_radius
                && approaches(
                    i64::from(next),
                    from.map(|from| i64::from(incident.chebyshev(from))),
                )
        }) {
            return true;
        }
        let radar = crate::stats::HARVEST_RADAR_DANGER_RADIUS;
        if cells.contacts.at(cell).iter().any(|&i| {
            let contact = self.contacts[i as usize];
            let next = contact.chebyshev(tile);
            next <= radar
                && approaches(
                    i64::from(next),
                    from.map(|from| i64::from(contact.chebyshev(from))),
                )
        }) {
            return true;
        }
        let closer = |next: Fx, was: Option<Fx>| was.is_none_or(|was| next < was);
        if cells.mobile.at(cell).iter().any(|&i| {
            let pressure = &self.mobile[i as usize];
            let next = pressure.pos.dist_sq(point);
            pressure.hostile
                && next <= pressure.reach_sq
                && closer(next, from_point.map(|from| pressure.pos.dist_sq(from)))
        }) {
            return true;
        }
        cells.statics.at(cell).iter().any(|&i| {
            let pressure = &self.statics[i as usize];
            let closest =
                |at: Vec2Fx| rect_closest_point(pressure.anchor, pressure.size, at).dist_sq(at);
            let next = closest(point);
            next <= pressure.reach_sq && closer(next, from_point.map(closest))
        })
    }
}

impl GroundSalvageDanger {
    /// Serves the known-ground probe through the per-tile memo. The
    /// closure returns `(open, volatile)`; volatile verdicts are
    /// handed back but never stored, so a visible scrap tile whose
    /// node can deplete mid-phase re-evaluates on every probe while
    /// everything else pays the full walk exactly once.
    pub(crate) fn known_ground_cached(
        &self,
        tile: TilePos,
        compute: impl FnOnce() -> (bool, bool),
    ) -> bool {
        let Some(index) = self.lane_index(tile) else {
            return compute().0;
        };
        let cell = &self.lanes[index];
        let bits = cell.get();
        if bits & lane::GROUND_SET != 0 {
            return bits & lane::GROUND != 0;
        }
        let (open, volatile) = compute();
        if !volatile {
            cell.set(bits | lane::GROUND_SET | if open { lane::GROUND } else { 0 });
        }
        open
    }

    /// The packed index of an in-bounds tile.
    fn lane_index(&self, tile: TilePos) -> Option<usize> {
        ((0..self.width).contains(&tile.x) && (0..self.height).contains(&tile.y))
            .then(|| tile.row_major(self.width))
    }

    /// Serves one boolean verdict through its memo lane pair; out of
    /// bounds computes uncached.
    fn lane_memo(
        &self,
        tile: TilePos,
        set: u16,
        value: u16,
        compute: impl FnOnce() -> bool,
    ) -> bool {
        let Some(index) = self.lane_index(tile) else {
            return compute();
        };
        let cell = &self.lanes[index];
        let bits = cell.get();
        if bits & set != 0 {
            return bits & value != 0;
        }
        let verdict = compute();
        cell.set(bits | set | if verdict { value } else { 0 });
        verdict
    }
}

/// Edge of a [`GroundSalvageDanger`] threat cell, in tiles.
const DANGER_CELL: i32 = 8;

fn danger_cells(tiles: i32) -> i32 {
    tiles.div_euclid(DANGER_CELL).max(0) + 1
}

/// The smallest whole tile count at least as long as `sqrt(reach_sq)`.
fn tile_reach(reach_sq: Fx) -> i32 {
    let mut reach = 0;
    while Fx::from_num(reach * reach) < reach_sq {
        reach += 1;
    }
    reach
}

/// One failed safe search's reach: a whole cardinal component, plus the
/// drainable tiles bordering it.
struct SafeProof {
    reached: Vec<u64>,
    watched: Vec<TilePos>,
}

impl SafeProof {
    fn reaches(&self, index: usize) -> bool {
        self.reached[index / 64] >> (index % 64) & 1 == 1
    }
}

/// [`CellLists`] for each kind of threat record.
struct ThreatCells {
    contacts: CellLists,
    mobile: CellLists,
    statics: CellLists,
    charges: CellLists,
}

/// Record indices filed per coarse cell, ascending within each cell.
struct CellLists {
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl CellLists {
    /// Files record `i` under every cell its inclusive tile box overlaps,
    /// clamping boxes that run past the map onto the border cells.
    fn build(
        columns: i32,
        rows: i32,
        boxes: impl Iterator<Item = (TilePos, TilePos)> + Clone,
    ) -> Self {
        let cells = |(low, high): (TilePos, TilePos)| {
            let clamp_column = |x: i32| x.div_euclid(DANGER_CELL).clamp(0, columns - 1);
            let clamp_row = |y: i32| y.div_euclid(DANGER_CELL).clamp(0, rows - 1);
            (clamp_row(low.y)..=clamp_row(high.y)).flat_map(move |row| {
                (clamp_column(low.x)..=clamp_column(high.x))
                    .map(move |column| as_index(row * columns + column))
            })
        };
        let mut starts = vec![0u32; as_index(columns * rows) + 1];
        for cell in boxes.clone().flat_map(cells) {
            starts[cell + 1] += 1;
        }
        for cell in 1..starts.len() {
            starts[cell] += starts[cell - 1];
        }
        let mut cursors = starts.clone();
        let mut items = vec![0u32; starts[starts.len() - 1] as usize];
        for (record, bounds) in boxes.enumerate() {
            for cell in cells(bounds) {
                items[cursors[cell] as usize] =
                    u32::try_from(record).expect("record counts fit in u32");
                cursors[cell] += 1;
            }
        }
        Self { starts, items }
    }

    fn at(&self, cell: usize) -> &[u32] {
        &self.items[self.starts[cell] as usize..self.starts[cell + 1] as usize]
    }
}

fn stamp_blocked_rect(
    rows: &mut [Vec<(i32, i32)>],
    map_width: i32,
    anchor: TilePos,
    size: (i32, i32),
) {
    for y in anchor.y..anchor.y + size.1 {
        stamp_blocked_span(rows, map_width, y, anchor.x, anchor.x + size.0 - 1);
    }
}

fn stamp_blocked_span(rows: &mut [Vec<(i32, i32)>], map_width: i32, y: i32, start: i32, end: i32) {
    let Some(row) = usize::try_from(y).ok().and_then(|y| rows.get_mut(y)) else {
        return;
    };
    let start = start.max(0);
    let end = end.min(map_width - 1);
    if start <= end {
        row.push((start, end));
    }
}

fn merge_spans(row: &mut Vec<(i32, i32)>) {
    row.sort_unstable();
    let mut write = 0;
    for read in 0..row.len() {
        let (start, end) = row[read];
        if write > 0 && start <= row[write - 1].1.saturating_add(1) {
            row[write - 1].1 = row[write - 1].1.max(end);
        } else {
            row[write] = (start, end);
            write += 1;
        }
    }
    row.truncate(write);
}

fn ground_weapon_reach(weapons: &[crate::stats::WeaponStats]) -> Option<Fx> {
    weapons
        .iter()
        .filter(|weapon| weapon.targets.covers(Domain::Ground))
        .map(|weapon| weapon.range)
        .max()
}

fn rect_closest_point(anchor: TilePos, size: (i32, i32), from: Vec2Fx) -> Vec2Fx {
    let min = anchor.center() - Vec2Fx::new(HALF, HALF);
    let max = min + Vec2Fx::new(Fx::from_num(size.0), Fx::from_num(size.1));
    Vec2Fx::new(from.x.clamp(min.x, max.x), from.y.clamp(min.y, max.y))
}

/// Horizontal half-spans of a sight disc, per |dy|: `spans[d]` is the
/// widest `dx` with `dx*dx + d*d <= r*r`. Built once per process for
/// every radius the stats can name — integer math, no libm.
fn disc_spans(radius: i32) -> &'static [i32] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<Vec<i32>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=32i32)
            .map(|r| {
                (0..=r)
                    .map(|dy| {
                        let mut span = r;
                        while span * span + dy * dy > r * r {
                            span -= 1;
                        }
                        span
                    })
                    .collect()
            })
            .collect()
    });
    &table[as_index(radius)]
}

/// Every seat learns at once that a public building fell, so every seat,
/// teammates alike, forgets its ghost with it.
pub(crate) fn forget_fallen_public_buildings(state: &mut State) {
    let fallen: Vec<_> = state
        .buildings
        .iter()
        .filter(|b| b.hp == 0 && b.is_public())
        .map(|b| (b.player, b.kind, b.anchor))
        .collect();
    for (owner, kind, anchor) in fallen {
        forget_public_building(state, owner, kind, anchor);
    }
}

/// Every seat forgets a public building that has left the world: it knew of
/// it without sight, so it learns of its going the same way.
pub(crate) fn forget_public_building(
    state: &mut State,
    owner: PlayerId,
    kind: BuildingKind,
    anchor: TilePos,
) {
    for vision in &mut state.vision {
        vision.forget_building(owner, kind, anchor);
    }
}

/// Forget a building only when its removal itself was observable.
pub(crate) fn forget_observed_building(state: &mut State, id: crate::BuildingId, detonated: bool) {
    let Some(b) = state.building(id) else {
        return;
    };
    let (owner, kind, anchor) = (b.player, b.kind, b.anchor);
    let viewers: Vec<_> = (0..state.players.len())
        .filter(|&index| {
            let viewer = PlayerId::from_index(index);
            b.tiles().any(|t| state.vision(viewer).visible(t))
                && (detonated || state.building_apparent(viewer, b))
        })
        .collect();
    for index in viewers {
        state.vision[index].forget_building(owner, kind, anchor);
    }
}

/// Rebuilds every player's `visible` set from their live entities, then
/// reconciles their building memory against what is now in sight.
pub(crate) fn refresh(state: &mut State) {
    let mut vision = std::mem::take(&mut state.vision);
    let mut coverage = RowCoverage::new(state.map.width(), state.map.height());
    let mut eyes: Vec<(TilePos, i32)> = Vec::new();
    let sightings = Sighting::gather(state);
    for index in 0..vision.len() {
        // Team sight is seat-symmetric by construction: every teammate
        // stamps the same discs, reconciles the same memories, and hears
        // the same radar, so a later seat on an already-computed team is a
        // byte-for-byte clone.
        if let Some(src) = (0..index).find(|&j| state.players[j].team == state.players[index].team)
        {
            let (head, tail) = vision.split_at_mut(index);
            tail[0].copy_from(&head[src]);
            continue;
        }
        let view = &mut vision[index];
        view.prune_salvage_incidents(state.tick);
        let my_team = state.players[index].team;
        let allied = |p: PlayerId| state.players[p.0 as usize].team == my_team;
        let viewer = PlayerId::from_index(index);
        let stealth = crate::state::ChargeLens::new(state, viewer);
        view.visible.fill(false);
        coverage.reset();
        // Team sight: every teammate's eyes stamp into this view. A disc
        // lies inside any wider one around the same tile, and crowds share
        // tiles, so only the widest disc per tile is stamped.
        eyes.clear();
        eyes.extend(
            sightings
                .iter()
                .filter(|unit| unit.team == my_team)
                .map(|unit| (unit.tile, unit.vision)),
        );
        eyes.sort_unstable_by_key(|&(tile, radius)| (tile.y, tile.x, std::cmp::Reverse(radius)));
        eyes.dedup_by_key(|&mut (tile, _)| tile);
        for &(tile, radius) in &eyes {
            view.stamp_disc(tile, radius, &mut coverage);
        }
        // Unbuilt sites provide no vision.
        for building in state
            .buildings
            .iter()
            .filter(|b| allied(b.player) && b.built())
        {
            let (w, h) = building.kind.size();
            view.stamp_rect(
                building.anchor,
                w,
                h,
                building.stats().vision,
                &mut coverage,
            );
        }

        // Memory reconciliation. Wherever we have sight, live state is the
        // truth: drop every record on visible ground, then re-record every
        // enemy building actually seen there (fresh hp). A building seen
        // *gone* thus loses its record, and a record on unseen ground
        // freezes at its last sighting.
        let mut ghosts = std::mem::take(&mut view.ghosts);
        ghosts.retain(|ghost| {
            if !ghost.footprint().any(|t| view.visible(t)) {
                return true;
            }
            // Ordinary sight cannot prove that a buried mine has vanished.
            // A visible replacement from the mine's team proves its removal:
            // that team cannot place over its own charge. Hostile scaffolds
            // can overlap a concealed mine and do not disprove the memory.
            ghost.kind.is_stealthy()
                && !stealth.detected_at(ghost.anchor)
                && !state.buildings.iter().any(|b| {
                    b.contains(ghost.anchor)
                        && !state.hostile(ghost.owner, b.player)
                        && stealth.building_apparent(b)
                })
        });
        for building in state.buildings.iter().filter(|b| !allied(b.player)) {
            // An undetected buried charge never enters memory: sight of
            // its tile alone is not knowledge of it (the one stealth
            // rule; see `State::building_apparent`).
            if !stealth.building_apparent(building) {
                continue;
            }
            if building.tiles().any(|t| view.visible(t)) {
                ghosts.push(GhostBuilding {
                    kind: building.kind,
                    owner: building.player,
                    anchor: building.anchor,
                    hp: building.hp,
                    built: building.built(),
                    tier: building.tier,
                });
            }
        }
        ghosts.sort_unstable_by_key(|g| (g.anchor.y, g.anchor.x, g.owner));
        view.ghosts = ghosts;

        // Freeze-frame the economy the same way: wherever there is sight,
        // remember the salvage; everywhere else the remembered numbers stand.
        // Row slices, not per-cell lookups, both memories in one walk —
        // and only inside the x-spans this team's stamps could have
        // touched, since nothing outside them became visible this tick.
        // The per-cell `seen` check still decides; the coverage bounds
        // only shrink the walk.
        for y in 0..state.map.height() {
            let (x0, x1) = coverage.bounds[as_index(y)];
            if x0 > x1 {
                continue;
            }
            let (x0, x1) = (as_index(x0), as_index(x1));
            let visible = &view.visible.row(y).expect("row in range")[x0..=x1];
            let tiles = &state.map.grid().row(y).expect("row in range")[x0..=x1];
            let scrap = &mut view.remembered_scrap.row_mut(y).expect("row in range")[x0..=x1];
            let wreck = &mut view.remembered_wreck.row_mut(y).expect("row in range")[x0..=x1];
            let explored = &mut view.explored.row_mut(y).expect("row in range")[x0..=x1];
            for (x, (&seen, tile)) in visible.iter().zip(tiles).enumerate() {
                if seen {
                    // Explored accumulates here instead of in the sight
                    // stamps, since this walk already touches every newly
                    // visible cell exactly once.
                    explored[x] = true;
                    scrap[x] = tile.scrap;
                    wreck[x] = tile.wreck;
                }
            }
        }

        // Radar blips: hostile units and buildings inside any allied radar
        // ring (a built mast's, or a scout's with its radar up), on ground
        // this player cannot actually see. A tile and a domain, with no
        // identity and no memory: a contact that leaves every ring is gone.
        view.contacts.clear();
        let mut masts: Vec<(TilePos, i32)> = state
            .buildings
            .iter()
            .filter(|b| allied(b.player) && b.built())
            .filter_map(|b| Some((b.anchor, b.stats().radar?)))
            .chain(
                sightings
                    .iter()
                    .filter(|unit| unit.team == my_team)
                    .filter_map(|unit| Some((unit.tile, unit.radar?))),
            )
            .collect();
        // A wider ring on the same tile contains a narrower one.
        masts.sort_unstable_by_key(|&(t, r)| (t.y, t.x, std::cmp::Reverse(r)));
        masts.dedup_by_key(|&mut (t, _)| t);
        let distance = |t: TilePos, m: TilePos| {
            let (dx, dy) = (t.x - m.x, t.y - m.y);
            dx * dx + dy * dy
        };
        // The squared distance to the nearest mast whose ring covers `t`.
        let ring_distance = |t: TilePos| {
            masts
                .iter()
                .filter_map(|&(m, r)| {
                    let d = distance(t, m);
                    (d <= r * r).then_some(d)
                })
                .min()
        };
        if !masts.is_empty() {
            for unit in sightings.iter().filter(|unit| unit.team != my_team) {
                let t = unit.tile;
                if !unit.seen_by(view) && masts.iter().any(|&(m, r)| distance(t, m) <= r * r) {
                    view.contacts.push(RadarContact {
                        tile: t,
                        domain: unit.domain,
                    });
                }
            }
            // A building returns one blip, like a unit of any size. An
            // undetected charge or hostile provisional site is not apparent
            // and returns nothing.
            for b in state
                .buildings
                .iter()
                .filter(|b| !allied(b.player) && stealth.building_apparent(b))
            {
                if b.tiles().any(|t| view.visible(t)) {
                    continue;
                }
                if let Some(tile) = radar_return(state, b, ring_distance) {
                    view.contacts.push(RadarContact {
                        tile,
                        domain: Domain::Ground,
                    });
                }
            }
            view.contacts.sort_unstable_by_key(RadarContact::sort_key);
            view.contacts.dedup();
        }
        let mut tracking = std::mem::take(&mut view.tracking);
        tracking.refresh(view, state, viewer, &sightings);
        view.tracking = tracking;
        view.track_warheads(state, |p| !allied(p), |t| ring_distance(t).is_some());
    }
    state.vision = vision;
}

/// One unit as each view's refresh reads it: gathered once per refresh
/// instead of once per view.
pub(super) struct Sighting {
    pub(super) id: crate::UnitId,
    pub(super) tile: TilePos,
    pos: chassis::fx::Vec2Fx,
    radius: chassis::fx::Fx,
    pub(super) team: u8,
    /// Where the unit is now: a landed flyer is on the ground.
    pub(super) domain: Domain,
    vision: i32,
    /// The unit's radar radius while its ring is up.
    radar: Option<i32>,
}

impl Sighting {
    pub(super) fn gather(state: &State) -> Vec<Self> {
        state
            .units
            .iter()
            .map(|unit| Self {
                id: unit.id,
                tile: unit.tile(),
                pos: unit.pos,
                radius: unit.kind.stats().radius,
                team: state.players[unit.player.0 as usize].team,
                domain: unit.domain(),
                vision: unit.kind.stats().vision,
                radar: unit.radar(),
            })
            .collect()
    }

    /// Whether `view` sees any tile this body claims.
    pub(super) fn seen_by(&self, view: &Vision) -> bool {
        if self.radius <= crate::stats::SMALL_BODY_RADIUS {
            return view.visible(self.tile);
        }
        crate::geometry::body_tiles(self.pos, self.radius).any(|t| view.visible(t))
    }
}

/// The footprint tile a building's radar return reports: the one nearest a
/// mast whose ring covers it, with distance ties ranked in the footprint's radial map frame. A
/// footprint centered on the map has no radial frame, so its remaining ties
/// rank toward the owner's first Foundry and then by seat parity, the fallback
/// order group spreads use. Mirrored seats therefore report mirrored tiles.
fn radar_return(
    state: &State,
    building: &crate::state::Building,
    ring_distance: impl Fn(TilePos) -> Option<i32>,
) -> Option<TilePos> {
    let map_size = (state.map.width(), state.map.height());
    let (anchor, size) = (building.anchor, building.kind.size());
    let ranked: Vec<_> = building
        .tiles()
        .filter_map(|t| {
            let radial = crate::geometry::spawn_doorstep_key(map_size, anchor, size, t);
            ring_distance(t).map(|d| ((d, radial), t))
        })
        .collect();
    let best = ranked.iter().map(|&(key, _)| key).min()?;
    let tied: Vec<TilePos> = ranked
        .into_iter()
        .filter(|&(key, _)| key == best)
        .map(|(_, t)| t)
        .collect();
    if let [only] = tied[..] {
        return Some(only);
    }
    // Doubled coordinates keep even footprint centers exact.
    let center = (
        i64::from(anchor.x) * 2 + i64::from(size.0),
        i64::from(anchor.y) * 2 + i64::from(size.1),
    );
    let toward_home = state
        .buildings
        .iter()
        .filter(|b| {
            b.player == building.player && !b.provisional() && b.kind == BuildingKind::Foundry
        })
        .min_by_key(|b| b.id)
        .map(|foundry| {
            let (w, h) = foundry.kind.size();
            (
                i64::from(foundry.anchor.x) * 2 + i64::from(w) - center.0,
                i64::from(foundry.anchor.y) * 2 + i64::from(h) - center.1,
            )
        })
        .filter(|&ray| ray != (0, 0));
    match toward_home {
        Some((rx, ry)) => tied.into_iter().max_by_key(|t| {
            let (cx, cy) = (
                i64::from(t.x) * 2 + 1 - center.0,
                i64::from(t.y) * 2 + 1 - center.1,
            );
            (rx * cx + ry * cy, rx * cy - ry * cx)
        }),
        None if building.player.0 % 2 == 1 => tied.last().copied(),
        None => tied.first().copied(),
    }
}

#[cfg(test)]
mod sight_tests;

#[cfg(test)]
mod danger_tests;
