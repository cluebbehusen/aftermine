//! Local guards: mobile defenders kept at each base but the start, as many
//! as the known threats near it call for beyond the completed guns covering
//! it. The start keeps its own reserve, which offense already respects.

use super::reserve::{self, LOCAL_REACH};
use super::{
    DefendPhase, MISSION_CAP, Mission, Missions, Shortfall, Task, UNIT_CAP, force, hits, hunt, mine,
};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled, footprint_centre, gap, ring};
use crate::map::MapModel;
use crate::memory::Memory;
use oxide_sim::geometry::foundry_supports;
use oxide_sim::observation::{BuildingObs, ObservationData, UnitObs};
use oxide_sim::scenario::BotStance;
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingKind, UnitId};

/// Empty tiles between two Foundries on one ground that make them one base
/// with one guard: a stance-free model of a base's extent.
const BASE_REACH: i32 = 8;

/// The buildings the seat guards: one Foundry for each base, the start's
/// first, and each Extractor no completed Foundry supports. Sites still
/// under construction count, so a guard covers their building too.
fn assets<'a>(
    observation: &'a ObservationData,
    map: &MapModel,
    frame: HomeFrame,
) -> Vec<&'a BuildingObs> {
    let mut foundries: Vec<_> = observation
        .my_buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::Foundry)
        .collect();
    foundries.sort_by_key(|b| {
        (
            map.start(observation.me) != Some(b.anchor),
            frame.rank(frame.home, footprint_centre(b.kind, b.anchor)),
            b.id,
        )
    });
    let mut bases: Vec<&BuildingObs> = Vec::new();
    for foundry in foundries {
        if !bases.iter().any(|base| {
            map.component(base.anchor) == map.component(foundry.anchor)
                && gap(
                    base.anchor,
                    base.kind.size(),
                    foundry.anchor,
                    foundry.kind.size(),
                ) <= BASE_REACH
        }) {
            bases.push(foundry);
        }
    }
    bases.extend(
        observation
            .my_buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::Extractor)
            .filter(|b| {
                !observation.my_buildings.iter().any(|f| {
                    f.kind == BuildingKind::Foundry
                        && f.built
                        && foundry_supports(f.anchor, b.anchor)
                })
            }),
    );
    bases
}

/// Whether `unit` could defend a building on `ground`: aircraft from
/// anywhere, ground units only from that ground.
fn reaches(unit: &UnitObs, map: &MapModel, ground: Option<u32>) -> bool {
    unit.kind.stats().can_fight()
        && (unit.kind.stats().domain == Domain::Air || map.component(unit.tile) == ground)
}

impl Missions {
    /// Units kept in a quiet local guard, for hosts' reports.
    pub(crate) fn guarded_units(&self) -> Vec<UnitId> {
        self.list
            .iter()
            .filter(|m| {
                matches!(
                    m.task,
                    Task::Defend {
                        phase: DefendPhase::Guard,
                        ..
                    }
                )
            })
            .flat_map(|m| m.units.iter().copied())
            .collect()
    }

    /// Keeps each guarded building's local guard at what its need calls for:
    /// its current guards and free units nearest it first, never what home
    /// keeps in reserve, and disbands a guard nothing threatens. A guard is a
    /// defense mission in its Guard phase, so a defense that answers an
    /// attack on the building takes its guards along, and the guard takes
    /// over again only once that defense has finished recovering. Returns
    /// what each guard still lacks beyond the units training nearby.
    pub(crate) fn guard(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        scratch: &super::Scratch,
        memory: &Memory,
        stance: BotStance,
        ledger: &mut Ledger,
    ) -> Vec<Shortfall> {
        let frame = scratch.frame;
        let mut gaps = Vec::new();
        let assets: Vec<_> = assets(observation, map, frame)
            .into_iter()
            .filter(|asset| map.start(observation.me) != Some(asset.anchor))
            .collect();
        // A base's guard follows the building that now stands for it, such
        // as a nearer Foundry founded beside it.
        self.list.retain(|m| match m.task {
            Task::Defend {
                asset,
                phase: DefendPhase::Guard,
            } => assets.iter().any(|a| a.id == asset),
            _ => true,
        });
        for asset in assets {
            let index = self
                .list
                .iter()
                .position(|m| matches!(m.task, Task::Defend { asset: id, .. } if id == asset.id));
            if index.is_some_and(|index| {
                !matches!(
                    self.list[index].task,
                    Task::Defend {
                        phase: DefendPhase::Guard,
                        ..
                    }
                )
            }) {
                continue;
            }
            let need = reserve::need(observation, map, memory, stance, asset.kind, asset.anchor);
            let ground = map.component(asset.anchor);
            let goal = ring(asset.anchor, asset.kind.size())
                .filter(|tile| map.component(*tile) == ground)
                .min_by_key(|tile| {
                    frame.rank(footprint_centre(asset.kind, asset.anchor), doubled(*tile))
                });
            let (Some(goal), true) = (goal, need != [0; 2]) else {
                if let Some(index) = index {
                    self.list.remove(index);
                }
                continue;
            };
            let members = index.map_or(&[][..], |i| self.list[i].units.as_slice());
            let free: Vec<_> = self
                .available(observation, false)
                .into_iter()
                .filter(|id| !ledger.stuck(*id))
                .filter_map(|id| mine(observation, id))
                .filter(|u| reaches(u, map, ground))
                .collect();
            let mut pool = self
                .spare(observation, map, scratch.reserve)
                .outermost(map, frame, free);
            pool.extend(
                members
                    .iter()
                    .filter(|id| !ledger.stuck(**id))
                    .filter_map(|id| mine(observation, *id))
                    .filter(|u| reaches(u, map, ground)),
            );
            pool.sort_by_key(|u| {
                (
                    frame.rank(footprint_centre(asset.kind, asset.anchor), doubled(u.tile)),
                    u.id,
                )
            });
            let mut have = [0_u64; 2];
            let mut kept = Vec::new();
            for unit in pool {
                if kept.len() >= UNIT_CAP {
                    break;
                }
                if [Domain::Ground, Domain::Air]
                    .into_iter()
                    .enumerate()
                    .any(|(i, domain)| have[i] < need[i] && hits(unit, domain))
                {
                    kept.push(unit.id);
                    let added = force(unit.kind, unit.hp);
                    have = [have[0] + added[0], have[1] + added[1]];
                }
            }
            for (producer, queue) in observation.my_buildings.iter().zip(&observation.my_queues) {
                if map.component(producer.anchor) == ground
                    && gap(
                        asset.anchor,
                        asset.kind.size(),
                        producer.anchor,
                        producer.kind.size(),
                    ) <= LOCAL_REACH
                {
                    for kind in queue {
                        let added = force(*kind, kind.stats().max_hp);
                        have = [have[0] + added[0], have[1] + added[1]];
                    }
                }
            }
            let gap = [0, 1].map(|i| need[i].saturating_sub(have[i]));
            if gap != [0, 0] {
                gaps.push(Shortfall { ground, gap });
            }
            kept.sort_unstable();
            let changed = members != kept;
            if kept.is_empty() {
                if let Some(index) = index {
                    self.list.remove(index);
                }
            } else if !changed || ledger.order(hunt(kept.clone(), goal)) {
                if let Some(index) = index {
                    let mission = &mut self.list[index];
                    mission.units = kept;
                    if changed {
                        mission.since = observation.tick;
                        mission.goal = goal;
                    }
                } else if self.list.len() < MISSION_CAP {
                    self.list.push(Mission {
                        id: self.next,
                        since: observation.tick,
                        units: kept,
                        goal,
                        task: Task::Defend {
                            asset: asset.id,
                            phase: DefendPhase::Guard,
                        },
                    });
                    self.next += 1;
                }
            }
        }
        gaps
    }
}
