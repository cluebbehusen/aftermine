//! Wounded units back to a Repair Bay between fights. Nothing here persists:
//! each decision reads who is wounded, where the Bays stand and how many
//! patients each already serves.

use super::air::Hazard;
use super::lift::{fly_to, open, route_orders};
use super::{Missions, Scratch, mine, run};
use crate::decision::Ledger;
use crate::defenses::Span;
use crate::frame::{HomeFrame, doubled, footprint_centre, ring};
use crate::map::MapModel;
use crate::workers::{UNIT_WELD_DAMAGE, calm, worker};
use chassis::grid::TilePos;
use oxide_sim::observation::{BuildingObs, ObservationData, UnitObs};
use oxide_sim::stats::{Domain, REPAIR_BAY_PATIENTS};
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
            .filter(|unit| unit.kind.stats().can_fight() && !worker(unit.kind) && unit.cargo == 0)
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

/// Whether a Repair Bay in reach could heal `unit` where it stands: wounded,
/// and on the ground or unable to land.
fn treatable(unit: &UnitObs) -> bool {
    let stats = unit.kind.stats();
    unit.hp < stats.max_hp && (unit.grounded || !stats.can_land())
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
