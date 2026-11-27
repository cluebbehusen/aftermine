//! Local guards: mobile defenders kept at each base but the start, as many
//! as the known threats near it call for beyond the completed guns covering
//! it. The start keeps its own reserve, which offense already respects.

use super::attack::{force_of, priced};
use super::defense::{at_home, lets_go};
use super::reserve::{self, LOCAL_REACH};
use super::{
    DefendPhase, MISSION_CAP, Mission, Missions, Shortfall, Task, UNIT_CAP, fill, hits, hunt,
    meeting, mine, value,
};
use crate::counters::weigh;
use crate::decision::Ledger;
use crate::frame::{HomeFrame, doubled, footprint_centre, gap, ring};
use crate::map::MapModel;
use crate::memory::Memory;
use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::scenario::BotStance;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, UnitId};

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
            .filter(|b| !crate::expansion::supported(observation, b.anchor)),
    );
    bases
}

/// Whether `unit` could defend a building on `ground`: aircraft from
/// anywhere, ground units only from that ground. A carrier stays free for
/// lifts, however armed.
fn reaches(unit: &UnitObs, map: &MapModel, ground: Option<u32>) -> bool {
    unit.kind.stats().can_fight()
        && !super::lift::carrier(unit.kind)
        && (unit.kind.stats().domain == Domain::Air || map.component(unit.tile) == ground)
}

impl Missions {
    /// Units kept in a quiet local guard, for hosts' reports.
    pub(crate) fn guarded_units(&self) -> Vec<UnitId> {
        self.list
            .iter()
            .filter(|m| m.guarding())
            .flat_map(|m| m.units.iter().copied())
            .collect()
    }

    /// Keeps each guarded building's local guard at what its need calls for:
    /// its current guards and free units nearest it first, never what home
    /// keeps in reserve, and disbands a guard nothing threatens. A base away
    /// from home whose known enemies nearby the units it could take would not
    /// match, at a Foundry as [`lets_go`] weighs it, is given up: its guard
    /// falls back home rather than wait there to be overrun. A
    /// guard is a defense mission in its Guard phase, so a defense that answers an
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
            if index.is_some_and(|index| !self.list[index].guarding()) {
                continue;
            }
            let keep =
                reserve::Keep::of(observation, map, memory, stance, asset.kind, asset.anchor);
            let ground = map.component(asset.anchor);
            let goal = ring(asset.anchor, asset.kind.size())
                .filter(|tile| map.component(*tile) == ground)
                .min_by_key(|tile| {
                    frame.rank(footprint_centre(asset.kind, asset.anchor), doubled(*tile))
                });
            let (Some(goal), true) = (goal, keep.need(&[]) != [0; 2]) else {
                if let Some(index) = index {
                    self.list.remove(index);
                }
                continue;
            };
            let members = index.map_or(&[][..], |i| self.list[i].units.as_slice());
            // One a defense standing down just sent home keeps that order this
            // decision rather than being turned straight back out.
            let free: Vec<_> = self
                .free(observation, ledger)
                .into_iter()
                .filter(|id| !ledger.running(*id))
                .filter_map(|id| mine(observation, id))
                .filter(|u| reaches(u, map, ground))
                .collect();
            let mut pool = self
                .spare(observation, map, &scratch.reserve)
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
            let could = force_of(
                pool.iter().copied().filter(|u| hits(u, Domain::Ground)),
                value,
            );
            // The guard is sized and filled by what each side trades against
            // the other: the known enemies near the building against the
            // units it could take.
            let need = keep.need(&force_of(pool.iter().copied(), value));
            let mut have = [0_u64; 2];
            let mut kept = fill(pool, need, &mut have, UNIT_CAP, |unit| {
                keep.force(unit.kind, unit.hp)
            });
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
                        let added = keep.force(*kind, kind.stats().max_hp);
                        have = [have[0] + added[0], have[1] + added[1]];
                    }
                }
            }
            let gap = [0, 1].map(|i| need[i].saturating_sub(have[i]));
            if gap != [0, 0] {
                // The known enemies the need comes from, so a seat that reads
                // threats answers them with what beats them rather than the
                // cheapest unit, which only stands there to be killed.
                let attackers = memory
                    .units()
                    .iter()
                    .filter(|unit| unit.kind.stats().can_fight())
                    .filter(|unit| {
                        unit.kind.stats().domain == Domain::Air
                            || map.component(unit.tile) == ground
                    })
                    .filter(|unit| {
                        crate::frame::gap(asset.anchor, asset.kind.size(), unit.tile, (1, 1))
                            <= LOCAL_REACH
                    })
                    .map(|unit| (unit.kind, unit.value(observation.tick)))
                    .collect();
                gaps.push(Shortfall {
                    ground,
                    gap,
                    attackers,
                    read: true,
                });
            }
            let met = meeting::opposed(observation, memory, goal);
            let (held, against) = (weigh(&could, &met), weigh(&met, &could));
            let outweighed = priced(&met) > 0
                && if asset.kind == BuildingKind::Foundry && asset.built {
                    lets_go(
                        held,
                        against,
                        crate::investments::built_foundries(&observation.my_buildings).count(),
                    )
                } else {
                    held < against
                };
            if !at_home(observation, map, goal) && outweighed {
                self.give_up(index, observation, map, frame, goal, ledger);
                continue;
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
