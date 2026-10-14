//! Wounded units back to a Repair Bay between fights, and wounded kiln
//! machines to safe known wreck. Nothing here persists: each decision reads
//! who is wounded, where the Bays stand, how many patients each already
//! serves, and what wreck the seat knows of.

use super::air::Hazard;
use super::lift::{fly_to, open, route_orders};
use super::{Missions, Scratch, mine, run};
use crate::decision::Ledger;
use crate::defenses::Span;
use crate::frame::{HomeFrame, doubled, footprint_centre, ring};
use crate::map::MapModel;
use crate::workers::{Routes, UNIT_WELD_DAMAGE, calm, worker};
use chassis::grid::TilePos;
use oxide_sim::observation::{BuildingObs, ObservationData, UnitObs};
use oxide_sim::stats::{Domain, Mend, REPAIR_BAY_PATIENTS};
use oxide_sim::{BuildingKind, UnitId, UnitKind};
use std::cmp::Reverse;

impl Missions {
    /// Keeps the free wounded already in a calm own Repair Bay's reach there,
    /// and sends the idle free wounded to the nearest calm Bay with room, the
    /// most missing value first: ground units drive beside it and aircraft
    /// fly over it, where an idle Condor sets down on its own. No other
    /// mission or welder
    /// takes them this decision. A Bay bills its heals from the bank as
    /// income arrives, so scrap spent elsewhere this decision holds no one
    /// back.
    pub(crate) fn repair(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        scratch: &Scratch,
        ledger: &mut Ledger,
    ) {
        let size = BuildingKind::RepairBay.size();
        let frame = scratch.frame;
        let bays: Vec<&BuildingObs> = observation
            .my_buildings
            .iter()
            .filter(|building| {
                building.kind == BuildingKind::RepairBay && building.built && building.hp > 0
            })
            .collect();
        if bays.is_empty() {
            return;
        }
        // A unit that heals on the ground must be safe from ground fire there,
        // and one that flies in from fire against aircraft; a Condor needs
        // both.
        let calm_for = |bay: &BuildingObs, kind: UnitKind| {
            let stats = kind.stats();
            let centre = footprint_centre(BuildingKind::RepairBay, bay.anchor);
            let clear = |hazards: &[Hazard]| calm(observation, hazards, bay.anchor, size, centre);
            let rests = stats.domain == Domain::Ground || stats.can_land();
            let flies = stats.domain == Domain::Air;
            (!rests || clear(&scratch.ground)) && (!flies || clear(&scratch.air))
        };
        // Every own body a Bay treats where it stands takes one of that Bay's
        // slots, whoever commands it.
        let anchors: Vec<TilePos> = bays.iter().map(|bay| bay.anchor).collect();
        let units: Vec<&UnitObs> = observation.my_units.iter().collect();
        let treated = treating(&anchors, &units);
        let mut room = vec![REPAIR_BAY_PATIENTS; bays.len()];
        for index in treated.iter().flatten() {
            room[*index] -= 1;
        }
        let treated_by = |unit: &UnitObs| {
            let at = units.binary_search_by_key(&unit.id, |unit| unit.id).ok()?;
            treated[at]
        };
        let mut wounded: Vec<(u64, &UnitObs)> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .filter(|unit| {
                let stats = unit.kind.stats();
                stats.can_fight() && stats.repairable() && !worker(unit.kind) && unit.cargo == 0
            })
            .filter_map(|unit| wound(unit).map(|missing| (missing, unit)))
            .collect();
        wounded.sort_by_key(|(missing, unit)| {
            (
                Reverse(*missing),
                frame.rank(frame.home, doubled(unit.tile)),
                unit.id,
            )
        });
        // Units share an order when they head for the same Bay the same way:
        // aircraft by the detour around known anti-air from where each is.
        let mut groups: Vec<(usize, Domain, Option<TilePos>, Vec<&UnitObs>)> = Vec::new();
        for (_, unit) in wounded {
            let domain = unit.kind.stats().domain;
            if treated_by(unit).is_some_and(|index| calm_for(bays[index], unit.kind)) {
                ledger.dock(unit.id);
                continue;
            }
            if !unit.idle {
                continue;
            }
            let ground = map.component(unit.tile);
            let Some(index) = (0..bays.len())
                .filter(|index| room[*index] > 0 && calm_for(bays[*index], unit.kind))
                .filter(|index| {
                    domain == Domain::Air || map.component(bays[*index].anchor) == ground
                })
                .min_by_key(|index| {
                    frame.rank(
                        doubled(unit.tile),
                        footprint_centre(BuildingKind::RepairBay, bays[*index].anchor),
                    )
                })
            else {
                continue;
            };
            room[index] -= 1;
            let via = if domain == Domain::Air {
                super::air::route(
                    observation,
                    frame,
                    &scratch.air,
                    unit.tile,
                    bays[index].anchor,
                )
            } else {
                None
            };
            match groups
                .iter_mut()
                .find(|(bay, layer, way, _)| *bay == index && *layer == domain && *way == via)
            {
                Some((_, _, _, units)) => units.push(unit),
                None => groups.push((index, domain, via, vec![unit])),
            }
        }
        for (index, domain, via, units) in groups {
            let bay = bays[index];
            let lead = units[0].tile;
            let mut ids: Vec<UnitId> = units.iter().map(|unit| unit.id).collect();
            ids.sort_unstable();
            if domain == Domain::Ground {
                let Some(goal) = beside(observation, map, frame, bay.anchor, lead) else {
                    continue;
                };
                if !ledger.order(run(ids.clone(), goal)) {
                    return;
                }
            } else {
                if ledger.room() < route_orders(via) {
                    return;
                }
                fly_to(ids.clone(), via, bay.anchor, false, ledger);
            }
            for id in ids {
                ledger.dock(id);
            }
        }
    }
}

/// Where a free wounded kiln machine feeds this decision: the tile it
/// stands on when its intake already reaches safe wreck there, else the tile
/// it drives to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Meal {
    pub(crate) unit: UnitId,
    kind: UnitKind,
    pub(crate) at: TilePos,
}

impl Meal {
    /// Whether the machine's intake reaches the wreck on `tile` from where it
    /// feeds, so workers leave that wreck to it.
    pub(crate) fn reserves(&self, tile: TilePos) -> bool {
        intake_reaches(self.kind, self.at, tile)
    }
}

impl Missions {
    /// Where each free wounded kiln machine feeds: where it stands if its
    /// intake already reaches safe known wreck, else the nearest tile its
    /// whole hull can drive to through known ground whose intake would,
    /// one holding enough to mend what it misses first. Safe wreck is wreck
    /// the seat's harvesters could salvage, by a route from home that avoids
    /// known danger and recent losses.
    pub(crate) fn meals(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        scratch: &Scratch,
        ledger: &Ledger,
    ) -> Vec<Meal> {
        let frame = scratch.frame;
        let hungry: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .filter(|unit| matches!(unit.kind.stats().mend, Mend::Kiln(_)))
            .filter(|unit| wound(unit).is_some())
            .collect();
        if hungry.is_empty() {
            return Vec::new();
        }
        let homes: Vec<&BuildingObs> = observation
            .my_buildings
            .iter()
            .filter(|building| building.kind == BuildingKind::Foundry && building.built)
            .collect();
        let routes = Routes::new(map, frame, scratch, &homes);
        let mut routes = routes.around_incidents(&observation.salvage_incidents);
        let fields: Vec<(TilePos, u32)> = observation
            .known_wrecks
            .iter()
            .filter(|(tile, amount)| *amount > 0 && !observation.known_scrap_at(*tile))
            .filter(|(tile, _)| routes.clear(*tile))
            .copied()
            .collect();
        if fields.is_empty() {
            return Vec::new();
        }
        hungry
            .into_iter()
            .filter_map(|unit| {
                let at = meal(observation, map, frame, &fields, unit)?;
                Some(Meal {
                    unit: unit.id,
                    kind: unit.kind,
                    at,
                })
            })
            .collect()
    }

    /// Keeps each of `meals` whose machine already feeds where it stands
    /// there while calm, and sends the idle others to where they would feed.
    /// No other mission takes them this decision; free units are out of
    /// every fight, so a machine at work in an attack feeds on what it cuts
    /// instead.
    pub(crate) fn feed(
        observation: &ObservationData,
        scratch: &Scratch,
        meals: &[Meal],
        ledger: &mut Ledger,
    ) {
        for meal in meals {
            let Some(unit) = mine(observation, meal.unit) else {
                continue;
            };
            if meal.at == unit.tile {
                if calm(
                    observation,
                    &scratch.ground,
                    unit.tile,
                    (1, 1),
                    doubled(unit.tile),
                ) {
                    ledger.dock(unit.id);
                }
                continue;
            }
            if !unit.idle {
                continue;
            }
            if !ledger.order(run(vec![unit.id], meal.at)) {
                return;
            }
            ledger.dock(unit.id);
        }
    }
}

/// Where wounded kiln machine `unit` feeds on `fields`: where it stands if
/// its intake already reaches one, else the nearest tile its hull reaches
/// from there whose intake would, preferring one holding enough to mend what
/// it misses.
fn meal(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    fields: &[(TilePos, u32)],
    unit: &UnitObs,
) -> Option<TilePos> {
    let stats = unit.kind.stats();
    let Mend::Kiln(kiln) = stats.mend else {
        return None;
    };
    if fields
        .iter()
        .any(|(tile, _)| intake_reaches(unit.kind, unit.tile, *tile))
    {
        return Some(unit.tile);
    }
    // Each tile a machine could feed from, with the wreck its intake would
    // reach there: no farther from a wreck than its hull's excess past one
    // tile plus the intake's reach.
    let span = oxide_sim::stats::hull_excess(stats.radius)
        .ceil()
        .to_num::<i32>()
        + kiln.reach;
    let mut stands: Vec<(TilePos, u64)> = Vec::new();
    for (wreck, amount) in fields {
        for dy in -span..=span {
            for dx in -span..=span {
                let stand = wreck.offset(dx, dy);
                if intake_reaches(unit.kind, stand, *wreck) {
                    stands.push((stand, u64::from(*amount)));
                }
            }
        }
    }
    stands.sort_unstable_by_key(|(tile, _)| (tile.y, tile.x));
    let mut food: Vec<(TilePos, u64)> = Vec::with_capacity(stands.len());
    for (tile, amount) in stands {
        match food.last_mut() {
            Some((last, held)) if *last == tile => *held += amount,
            _ => food.push((tile, amount)),
        }
    }
    let missing = u64::from(stats.max_hp.saturating_sub(unit.hp));
    let from = doubled(unit.tile);
    food.sort_unstable_by_key(|(tile, held)| {
        (
            held * u64::from(kiln.hp_per_wreck) < missing,
            frame.rank(from, doubled(*tile)),
        )
    });
    let mut flood = crate::breach::HullFlood::new(map, observation, stats.radius, unit.tile);
    food.into_iter()
        .map(|(tile, _)| tile)
        .find(|tile| flood.reaches(*tile))
}

/// Whether wounded kiln machine `unit` would bite the wreck on `tile` from
/// where it stands: the tile lies under its hull or within its kiln's reach
/// of it.
pub(crate) fn feeds(unit: &UnitObs, tile: TilePos) -> bool {
    unit.hp < unit.kind.stats().max_hp && intake_reaches(unit.kind, unit.tile, tile)
}

/// Whether the kiln of a `kind` machine standing on `stand` reaches the wreck
/// on `tile`: the tile lies under its hull or within its kiln's reach of it.
fn intake_reaches(kind: UnitKind, stand: TilePos, tile: TilePos) -> bool {
    let stats = kind.stats();
    let Mend::Kiln(kiln) = stats.mend else {
        return false;
    };
    oxide_sim::geometry::body_tiles(stand.center(), stats.radius)
        .any(|body| body.chebyshev(tile) <= kiln.reach)
}

/// Whether a Repair Bay in reach could heal `unit` where it stands: wounded,
/// and on the ground or unable to land.
fn treatable(unit: &UnitObs) -> bool {
    let stats = unit.kind.stats();
    unit.hp < stats.max_hp && stats.repairable() && (unit.grounded || !stats.can_land())
}

/// Which of the Repair Bays at `bays` treats each of `units`, as the
/// simulation chooses: Bays in building order, each taking up to
/// [`REPAIR_BAY_PATIENTS`] of the treatable units in its reach that no
/// earlier Bay took, the most wounded first. Reach is judged from tile
/// centres.
pub(crate) fn treating(bays: &[TilePos], units: &[&UnitObs]) -> Vec<Option<usize>> {
    let size = BuildingKind::RepairBay.size();
    let mut treated = vec![None; units.len()];
    for (index, anchor) in bays.iter().enumerate() {
        let span = Span::of(size, *anchor);
        let mut reached: Vec<usize> = (0..units.len())
            .filter(|unit| {
                treated[*unit].is_none()
                    && treatable(units[*unit])
                    && span.aura(Span::point(units[*unit].tile))
            })
            .collect();
        let missing = |unit: &UnitObs| {
            let max = u64::from(unit.kind.stats().max_hp);
            (max - u64::from(unit.hp), max)
        };
        reached.sort_by(|a, b| {
            let ((a_missing, a_max), (b_missing, b_max)) = (missing(units[*a]), missing(units[*b]));
            (b_missing * a_max)
                .cmp(&(a_missing * b_max))
                .then(units[*a].id.cmp(&units[*b].id))
        });
        for unit in reached.into_iter().take(REPAIR_BAY_PATIENTS) {
            treated[unit] = Some(index);
        }
    }
    treated
}

/// The scrap of health `unit` misses, when it misses enough to go to a Bay.
fn wound(unit: &UnitObs) -> Option<u64> {
    let stats = unit.kind.stats();
    let (hp, max) = (u64::from(unit.hp), u64::from(stats.max_hp.max(1)));
    (hp * 1_000 <= max * (1_000 - UNIT_WELD_DAMAGE))
        .then(|| u64::from(stats.cost) * (max - hp) / max)
}

/// The open tile bordering the Bay at `anchor` nearest `from`, on the Bay's
/// ground.
fn beside(
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    anchor: TilePos,
    from: TilePos,
) -> Option<TilePos> {
    let ground = map.component(anchor)?;
    ring(anchor, BuildingKind::RepairBay.size())
        .filter(|tile| open(observation, map, *tile, ground))
        .min_by_key(|tile| frame.rank(doubled(from), doubled(*tile)))
}
