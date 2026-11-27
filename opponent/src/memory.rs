//! What the seat remembers between decisions: enemy units it has seen or
//! learned stood up from a public cradle, where they were with confidence
//! that fades until they are seen again, and that they exist until the seat
//! sees them die; building footprints it
//! failed to claim, so it tries somewhere else for a while; enemy buildings
//! it gave up attacking, so it attacks something else for a while; enemy
//! buildings it raided, so the next raid goes elsewhere; when it last saw
//! each of its scouting points; and its own units whose orders stalled for
//! want of a route or a firing position, so they sit out orders for a while.
//! Enemy buildings need no other memory here: the observation keeps their
//! ghosts.

use crate::events::OwnEvent;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::{BuildingKind, PlayerId, StallReason, UnitId, UnitKind};
use chassis::grid::TilePos;
use serde::{Deserialize, Serialize};

/// Ticks a failed footprint stays skipped.
const FAILURE_TICKS: u64 = 3_600;

/// Failures, abandoned targets and raided targets each remembered at once,
/// the oldest forgotten first: a computation bound that normal play stays
/// under, since each is forgotten after `FAILURE_TICKS` anyway.
const FAILURE_CAP: usize = 256;

/// Ticks an enemy unit is remembered after it was last seen.
pub(crate) const UNIT_TICKS: u64 = 600;

/// Ticks an enemy ground unit seen on ground the seat's own does not reach is
/// remembered: it cannot leave there without carriers, so it is likely still
/// there long after a mobile unit would have moved on.
const STRANDED_TICKS: u64 = 3_600;

/// Ticks an enemy unit counts toward what the seat knows the enemy fields
/// after it was last seen, unless the seat sees it die first: a bound on how
/// long a death out of the seat's sight keeps counting.
const CENSUS_TICKS: u64 = 3_600;

/// Enemy units remembered at once, where they stood and that they exist each,
/// the stalest forgotten first: a computation bound that normal play stays
/// under, since units seen more than `CENSUS_TICKS` ago are forgotten anyway.
const UNIT_CAP: usize = 4_096;

/// Ticks an own unit whose order stalled for want of a route or a firing
/// position sits out orders, unless it moves off where it stopped first.
const STUCK_TICKS: u64 = 600;

/// Stuck own units remembered at once, the oldest forgotten first: a
/// computation bound that normal play stays under.
const STUCK_CAP: usize = 1_024;

/// The seat's memory: enemy units by id, failures oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Memory {
    units: Vec<SeenUnit>,
    /// Enemy units the seat has seen and not seen die, by id, whether or not
    /// it still knows where they are.
    census: Vec<Censused>,
    failures: Vec<Failure>,
    /// Attack targets given up on, oldest first.
    abandoned: Vec<Failure>,
    /// Raid targets raided, oldest first. Kept apart from abandoned targets
    /// so a raid leaves the target to larger missions.
    raided: Vec<Failure>,
    /// Tick each scouting point was last in sight, by point; empty before
    /// the first decision.
    scouted: Vec<u64>,
    /// Own units whose orders stalled for want of a route or a firing
    /// position, by id.
    stuck: Vec<Stuck>,
    /// Anchors of the seat's own Extractors, sites included, at the last
    /// decision, in order.
    extractors: Vec<TilePos>,
}

/// An own unit whose order stalled for want of a route or a firing position:
/// where it stopped, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stuck {
    unit: UnitId,
    tile: TilePos,
    at: u64,
}

/// An enemy unit as last seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeenUnit {
    pub(crate) id: UnitId,
    pub(crate) kind: UnitKind,
    pub(crate) tile: TilePos,
    pub(crate) seen: u64,
}

/// An enemy unit the seat knows exists, and when it last saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Censused {
    id: UnitId,
    kind: UnitKind,
    seen: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    kind: BuildingKind,
    anchor: TilePos,
    at: u64,
}

impl Memory {
    /// Refreshes every enemy unit in sight, and forgets where those gone
    /// stale or missing from where they were last seen stand, remembering for
    /// longer the ground units `stranded` says stand on ground the seat's own
    /// does not reach. That they exist is forgotten only once they are seen to
    /// die or `CENSUS_TICKS` pass unseen.
    pub(crate) fn observe_stranded(
        &mut self,
        observation: &ObservationData,
        stranded: impl Fn(&SeenUnit) -> bool,
    ) {
        let now = observation.tick;
        for unit in &observation.enemy_units {
            let seen = SeenUnit {
                id: unit.id,
                kind: unit.kind,
                tile: unit.tile,
                seen: now,
            };
            match self.units.binary_search_by_key(&unit.id, |known| known.id) {
                Ok(index) => self.units[index] = seen,
                Err(index) => self.units.insert(index, seen),
            }
            self.count(unit.id, unit.kind, now);
        }
        self.census.retain(|unit| now < unit.seen + CENSUS_TICKS);
        self.units.retain(|unit| {
            let ticks = if stranded(unit) {
                STRANDED_TICKS
            } else {
                UNIT_TICKS
            };
            now < unit.seen + ticks && (unit.seen == now || !observation.visible(unit.tile))
        });
        while self.units.len() > UNIT_CAP {
            let stalest = self
                .units
                .iter()
                .enumerate()
                .min_by_key(|(_, unit)| (unit.seen, unit.id))
                .map(|(index, _)| index)
                .expect("over the cap");
            self.units.remove(stalest);
        }
        while self.census.len() > UNIT_CAP {
            let stalest = self
                .census
                .iter()
                .enumerate()
                .min_by_key(|(_, unit)| (unit.seen, unit.id))
                .map(|(index, _)| index)
                .expect("over the cap");
            self.census.remove(stalest);
        }
    }

    /// Counts `id` as an enemy unit of `kind` seen at `now`. Drones are left
    /// out: they dock in their host without dying and launch again under new
    /// ids.
    fn count(&mut self, id: UnitId, kind: UnitKind, now: u64) {
        if kind.stats().docks() {
            return;
        }
        let counted = Censused {
            id,
            kind,
            seen: now,
        };
        match self.census.binary_search_by_key(&id, |known| known.id) {
            Ok(index) => self.census[index] = counted,
            Err(index) => self.census.insert(index, counted),
        }
    }

    /// Forgets the enemy units `events` report the seat saw die: it stops
    /// counting them, and stops remembering where they stood, which a
    /// sighting elsewhere would not clear while their old place is fogged.
    pub(crate) fn died(&mut self, events: &[OwnEvent]) {
        for event in events {
            if let OwnEvent::EnemyDied { unit } = *event {
                if let Ok(index) = self.census.binary_search_by_key(&unit, |known| known.id) {
                    self.census.remove(index);
                }
                self.units.retain(|seen| seen.id != unit);
            }
        }
    }

    /// Remembers each machine of a seat `hostile` names that `events` report
    /// standing up from its cradle, as seen where it stood up at this
    /// decision. A later observation refreshes or forgets it like any unit
    /// seen.
    pub(crate) fn assembled(
        &mut self,
        observation: &ObservationData,
        events: &[OwnEvent],
        hostile: impl Fn(PlayerId) -> bool,
    ) {
        for event in events {
            if let OwnEvent::MachineAssembled {
                unit,
                kind,
                player,
                pos,
            } = *event
                && hostile(player)
            {
                let seen = SeenUnit {
                    id: unit,
                    kind,
                    tile: TilePos::containing(pos),
                    seen: observation.tick,
                };
                match self.units.binary_search_by_key(&unit, |known| known.id) {
                    Ok(index) => self.units[index] = seen,
                    Err(index) => self.units.insert(index, seen),
                }
                self.count(unit, kind, observation.tick);
            }
        }
    }

    /// Remembered enemy units where they were last seen, by id: what tactics
    /// read.
    pub(crate) fn units(&self) -> &[SeenUnit] {
        &self.units
    }

    /// The kind of each enemy unit the seat has seen and not seen die,
    /// wherever it now is: what the army's needs and the growth it holds back
    /// count of the enemy, so an army out of sight is unknown rather than
    /// gone.
    pub(crate) fn census(&self) -> impl Iterator<Item = UnitKind> + '_ {
        self.census.iter().map(|unit| unit.kind)
    }

    /// Enemy units in sight on the tile they were last seen on, by id. Read
    /// before this look refreshes the memory, they are those standing still
    /// since the last.
    pub(crate) fn standing(&self, observation: &ObservationData) -> Vec<UnitId> {
        let mut still: Vec<UnitId> = observation
            .enemy_units
            .iter()
            .filter(|unit| {
                self.units
                    .binary_search_by_key(&unit.id, |known| known.id)
                    .is_ok_and(|index| self.units[index].tile == unit.tile)
            })
            .map(|unit| unit.id)
            .collect();
        still.sort_unstable();
        still
    }

    /// Remembers that `kind` could not be claimed at `anchor`.
    pub(crate) fn fail(&mut self, kind: BuildingKind, anchor: TilePos, now: u64) {
        record(&mut self.failures, kind, anchor, now);
    }

    /// Whether `kind` recently failed at `anchor`.
    pub(crate) fn failed(&self, kind: BuildingKind, anchor: TilePos, now: u64) -> bool {
        recent(&self.failures, kind, anchor, now)
    }

    /// Remembers giving up attacking, lifting to or striking `kind` at
    /// `anchor`.
    pub(crate) fn abandon(&mut self, kind: BuildingKind, anchor: TilePos, now: u64) {
        record(&mut self.abandoned, kind, anchor, now);
    }

    /// Whether attacking, lifting to or striking `kind` at `anchor` was recently
    /// given up.
    pub(crate) fn abandoned(&self, kind: BuildingKind, anchor: TilePos, now: u64) -> bool {
        recent(&self.abandoned, kind, anchor, now)
    }

    /// Every target an attack, lift or strike recently gave up.
    pub(crate) fn given_up(&self, now: u64) -> impl Iterator<Item = (BuildingKind, TilePos)> + '_ {
        self.abandoned
            .iter()
            .filter(move |failure| now < failure.at + FAILURE_TICKS)
            .map(|failure| (failure.kind, failure.anchor))
    }

    /// Remembers a raid on `kind` at `anchor`.
    pub(crate) fn raid(&mut self, kind: BuildingKind, anchor: TilePos, now: u64) {
        record(&mut self.raided, kind, anchor, now);
    }

    /// Whether `kind` at `anchor` was recently raided.
    pub(crate) fn raided(&self, kind: BuildingKind, anchor: TilePos, now: u64) -> bool {
        recent(&self.raided, kind, anchor, now)
    }

    /// When each scouting point was last in sight, sized to `points` on first
    /// use.
    pub(crate) fn scouted(&mut self, points: usize) -> &mut [u64] {
        if self.scouted.is_empty() {
            self.scouted = vec![0; points];
        }
        &mut self.scouted
    }

    /// Whether scouting point `point` was in sight recently enough that the
    /// enemy units seen around it are still remembered.
    pub(crate) fn looked(&self, point: usize, now: u64) -> bool {
        self.scouted
            .get(point)
            .is_some_and(|seen| now < seen + UNIT_TICKS)
    }

    /// The tick scouting point `point` was last in sight, zero if never.
    pub(crate) fn seen(&self, point: usize) -> u64 {
        self.scouted.get(point).copied().unwrap_or_default()
    }

    /// Counts scouting point `point` as seen at `now`.
    pub(crate) fn saw(&mut self, point: usize, now: u64) {
        if let Some(seen) = self.scouted.get_mut(point) {
            *seen = now;
        }
    }

    /// Remembers the own units whose orders stalled for want of a route or a
    /// firing position in `events`, and forgets those that left where they stopped, are gone, or
    /// sat out [`STUCK_TICKS`].
    pub(crate) fn stalls(&mut self, observation: &ObservationData, events: &[OwnEvent]) {
        let now = observation.tick;
        for event in events {
            if let OwnEvent::OrderStalled {
                unit,
                pos,
                reason: StallReason::NoRoute | StallReason::NoFiringPosition,
            } = *event
            {
                let stuck = Stuck {
                    unit,
                    tile: TilePos::containing(pos),
                    at: now,
                };
                match self.stuck.binary_search_by_key(&unit, |held| held.unit) {
                    Ok(index) => self.stuck[index] = stuck,
                    Err(index) => self.stuck.insert(index, stuck),
                }
            }
        }
        if self.stuck.is_empty() {
            return;
        }
        let tiles: std::collections::BTreeMap<UnitId, TilePos> = observation
            .my_units
            .iter()
            .map(|unit| (unit.id, unit.tile))
            .collect();
        self.stuck.retain(|stuck| {
            now < stuck.at + STUCK_TICKS && tiles.get(&stuck.unit) == Some(&stuck.tile)
        });
        while self.stuck.len() > STUCK_CAP {
            let oldest = self
                .stuck
                .iter()
                .enumerate()
                .min_by_key(|(_, stuck)| (stuck.at, stuck.unit))
                .map_or(0, |(index, _)| index);
            self.stuck.remove(oldest);
        }
    }

    /// Own units sitting out orders, by id.
    pub(crate) fn stuck(&self) -> Vec<UnitId> {
        self.stuck.iter().map(|stuck| stuck.unit).collect()
    }

    /// Remembers as failed each frame whose own Extractor, standing or begun
    /// at the last decision, is gone: raiders waiting out of sight are known
    /// only by what they kill.
    pub(crate) fn lost_extractors(&mut self, observation: &ObservationData) {
        let mut standing: Vec<TilePos> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::Extractor)
            .map(|building| building.anchor)
            .collect();
        standing.sort_unstable();
        standing.dedup();
        for anchor in std::mem::take(&mut self.extractors) {
            if standing.binary_search(&anchor).is_err() {
                record(
                    &mut self.failures,
                    BuildingKind::Extractor,
                    anchor,
                    observation.tick,
                );
            }
        }
        self.extractors = standing;
    }

    /// Forgets failures old enough to try again.
    pub(crate) fn forget(&mut self, now: u64) {
        self.failures
            .retain(|failure| now < failure.at + FAILURE_TICKS);
        self.abandoned
            .retain(|failure| now < failure.at + FAILURE_TICKS);
        self.raided
            .retain(|failure| now < failure.at + FAILURE_TICKS);
    }

    /// Rejects a restored memory that could not have been recorded by `now`
    /// on a map of the given size with `points` scouting points.
    pub(crate) fn validate(
        &self,
        now: u64,
        width: i32,
        height: i32,
        points: usize,
    ) -> Result<(), String> {
        let on_map = |tile: TilePos| (0..width).contains(&tile.x) && (0..height).contains(&tile.y);
        if !self.units.iter().all(|unit| on_map(unit.tile)) {
            return Err("checkpoint enemy units are off the map".into());
        }
        let sized = self.scouted.is_empty() || self.scouted.len() == points;
        if !sized || self.scouted.iter().any(|tick| *tick > now) {
            return Err("checkpoint scouting memory does not fit the map".into());
        }
        let full = |list: &[Failure]| list.len() > FAILURE_CAP;
        if full(&self.failures)
            || full(&self.abandoned)
            || full(&self.raided)
            || self.units.len() > UNIT_CAP
            || self.census.len() > UNIT_CAP
            || self.stuck.len() > STUCK_CAP
        {
            return Err("checkpoint remembers too much".into());
        }
        let stuck = self
            .stuck
            .windows(2)
            .all(|pair| pair[0].unit < pair[1].unit)
            && self
                .stuck
                .iter()
                .all(|stuck| stuck.at <= now && on_map(stuck.tile));
        if !stuck {
            return Err("checkpoint stuck units are malformed".into());
        }
        // Distinct anchors on the map: bounded by the map, as the seat's own
        // Extractors are.
        let extractors = self.extractors.windows(2).all(|pair| pair[0] < pair[1])
            && self.extractors.iter().all(|anchor| on_map(*anchor));
        if !extractors {
            return Err("checkpoint own Extractors are malformed".into());
        }
        let by_id = self.units.windows(2).all(|pair| pair[0].id < pair[1].id)
            && self.census.windows(2).all(|pair| pair[0].id < pair[1].id);
        let seen = self.units.iter().map(|unit| unit.seen);
        if !by_id
            || seen
                .chain(self.census.iter().map(|unit| unit.seen))
                .any(|tick| tick > now)
        {
            return Err("checkpoint enemy units are out of order".into());
        }
        let ordered = |list: &[Failure]| {
            list.windows(2).all(|pair| pair[0].at <= pair[1].at)
                && list.iter().all(|failure| failure.at <= now)
        };
        if !ordered(&self.failures) || !ordered(&self.abandoned) || !ordered(&self.raided) {
            return Err("checkpoint failures are out of order".into());
        }
        Ok(())
    }
}

/// Remembers `kind` at `anchor` in `list`, oldest forgotten first once full.
fn record(list: &mut Vec<Failure>, kind: BuildingKind, anchor: TilePos, now: u64) {
    list.retain(|failure| (failure.kind, failure.anchor) != (kind, anchor));
    if list.len() == FAILURE_CAP {
        list.remove(0);
    }
    list.push(Failure {
        kind,
        anchor,
        at: now,
    });
}

/// Whether `list` recorded `kind` at `anchor` recently.
fn recent(list: &[Failure], kind: BuildingKind, anchor: TilePos, now: u64) -> bool {
    list.iter().any(|failure| {
        (failure.kind, failure.anchor) == (kind, anchor) && now < failure.at + FAILURE_TICKS
    })
}

impl SeenUnit {
    /// The unit's price weighted by how sure the seat is it is still there.
    pub(crate) fn value(&self, now: u64) -> u64 {
        u64::from(self.kind.stats().cost) * u64::from(self.confidence(now)) / 1_000
    }

    /// Per-mille confidence that the unit is still roughly where and what it
    /// was.
    pub(crate) fn confidence(&self, now: u64) -> u32 {
        let age = now.saturating_sub(self.seen).min(UNIT_TICKS);
        u32::try_from((UNIT_TICKS - age) * 1_000 / UNIT_TICKS)
            .expect("a per-mille share fits in u32")
    }
}

#[cfg(test)]
mod tests;
