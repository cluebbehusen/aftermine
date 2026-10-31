//! Scouting: a scout looks at each place the seat has not seen for a while,
//! first the one whose time unseen weighted by its value is greatest. For
//! places no scout can take it asks production for more.

use super::Scratch;
use super::air::{self, Hazard};
use super::{MISSION_CAP, Mission, Missions, Task, approach, mine, run};
use crate::decision::Ledger;
use crate::events::OwnEvent;
use crate::frame::{HomeFrame, doubled, tiles_of};
use crate::map::MapModel;
use crate::memory::Memory;
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, Command, PlayerId, StallReason, UnitKind};
use chassis::grid::TilePos;
use std::cell::OnceCell;

/// Ticks a point may go unseen before it is worth a look.
pub(crate) const STALE_TICKS: u64 = 1_800;

/// Unseen ticks counted at most, so a point's value outweighs its age.
const AGE_CAP: u64 = 7_200;

/// Ticks a scout may take to see its point.
pub(super) const TRAVEL_TICKS: u64 = 1_800;

/// A place worth seeing, as a Foundry footprint.
pub(crate) struct Point {
    pub(crate) anchor: TilePos,
    value: u64,
}

/// The seat's scouting points: hostile starts, then expansion sites by
/// their best Foundry anchor. Starts are worth most, then sites nearer an
/// enemy than home.
pub(crate) fn points(map: &MapModel, me: PlayerId) -> Vec<Point> {
    let footprint = |anchor: TilePos| tiles_of(anchor, BuildingKind::Foundry.size());
    let starts = map
        .hostiles(me)
        .filter_map(|owner| map.start(owner))
        .map(|anchor| Point { anchor, value: 3 });
    let sites = map
        .sites()
        .iter()
        .filter_map(|site| site.anchors.first().copied())
        .map(|anchor| {
            let hostile = footprint(anchor)
                .map(|tile| map.hostile_distance(me, tile))
                .min();
            let own = footprint(anchor).map(|tile| map.distance(me, tile)).min();
            Point {
                anchor,
                value: if hostile < own { 2 } else { 1 },
            }
        });
    starts.chain(sites).collect()
}

impl Missions {
    /// Keeps a scout looking at each stale place, no two at one place.
    /// Returns how many stale places no scout holds that the scout
    /// production would train could reach, which production trains scouts
    /// for: an aircraft once an Airworks stands, else a Surveyor. Zero while
    /// no mission could take another.
    pub(crate) fn scout(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        frame: HomeFrame,
        memory: &mut Memory,
        scratch: &Scratch,
        ledger: &mut Ledger,
    ) -> usize {
        let now = observation.tick;
        let points = points(map, observation.me);
        let goals = Goals {
            observation,
            map,
            frame,
            points: &points,
            ground: OnceCell::new(),
            air: OnceCell::new(),
        };
        let hazards = &scratch.air;
        let scouted = memory.scouted(points.len());

        let scouting: fn(&Task) -> bool = |task| matches!(task, Task::Scout { .. });
        let held = |missions: &Self, except: Option<u64>| -> Vec<u16> {
            missions
                .list
                .iter()
                .filter(|mission| Some(mission.id) != except)
                .filter_map(|mission| match mission.task {
                    Task::Scout { point } => Some(point),
                    _ => None,
                })
                .collect()
        };
        for id in self.ids(scouting) {
            let Some(index) = self.index_of(id) else {
                continue;
            };
            let mission = &self.list[index];
            let Task::Scout { point } = mission.task else {
                unreachable!("the scout mission's kind");
            };
            let point = usize::from(point);
            let arrived = scouted[point] == now;
            let late = now >= mission.since + TRAVEL_TICKS;
            if !(arrived || late) {
                continue;
            }
            scouted[point] = now;
            let Some(scout) = mine(observation, mission.units[0]) else {
                continue;
            };
            let others = held(self, Some(id));
            match best(
                now,
                frame,
                &points,
                scouted,
                &others,
                goals.of(flies(scout)),
            ) {
                Some((next, goal)) => {
                    if send(observation, frame, hazards, scout, goal, ledger) {
                        let mission = &mut self.list[index];
                        mission.task = Task::Scout { point: next };
                        mission.since = now;
                        mission.goal = goal;
                    }
                }
                None => {
                    self.list.remove(index);
                }
            }
        }

        let home = map
            .start(observation.me)
            .and_then(|start| map.component(start));
        let mut scouts: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .filter(|unit| {
                unit.kind == UnitKind::Kestrel
                    || (unit.kind.stats().scouts && map.component(unit.tile) == home)
            })
            .collect();
        // Ground scouts take what they can reach first, leaving aircraft
        // the places only flight reaches.
        scouts.sort_by_key(|unit| {
            (
                flies(unit),
                frame.rank(frame.home, doubled(unit.tile)),
                unit.id,
            )
        });
        while self.list.len() < MISSION_CAP {
            let taken = held(self, None);
            let chosen = scouts.iter().enumerate().find_map(|(index, scout)| {
                best(now, frame, &points, scouted, &taken, goals.of(flies(scout)))
                    .map(|best| (index, best))
            });
            let Some((index, (point, goal))) = chosen else {
                break;
            };
            let scout = scouts.remove(index);
            if !send(observation, frame, hazards, scout, goal, ledger) {
                break;
            }
            self.list.push(Mission {
                id: self.next,
                since: now,
                units: vec![scout.id],
                goal,
                task: Task::Scout { point },
            });
            self.next += 1;
        }
        if self.list.len() >= MISSION_CAP {
            return 0;
        }
        let taken = held(self, None);
        let air = observation
            .my_buildings
            .iter()
            .any(|building| building.kind == BuildingKind::Airworks && building.built);
        let reach = goals.of(air);
        scouted
            .iter()
            .enumerate()
            .filter(|(index, seen)| {
                now - **seen >= STALE_TICKS
                    && reach[*index].is_some()
                    && u16::try_from(*index).is_ok_and(|point| !taken.contains(&point))
            })
            .count()
    }

    /// Counts the point of each scout that stopped short of it, or gave it
    /// up for want of a safe view, as seen: the scout moves on, and the next
    /// one waits until the point is stale again.
    pub(crate) fn scouts_stopped(&self, events: &[OwnEvent], memory: &mut Memory, now: u64) {
        for event in events {
            let OwnEvent::OrderStalled {
                unit,
                reason: StallReason::SafeVantage | StallReason::NoSafeVantage,
                ..
            } = *event
            else {
                continue;
            };
            if let Some(point) = self.list.iter().find_map(|mission| match mission.task {
                Task::Scout { point } if mission.units.contains(&unit) => Some(point),
                _ => None,
            }) {
                memory.saw(usize::from(point), now);
            }
        }
    }
}

/// Counts every scouting point in sight now as seen, before any mission
/// reads when each was last seen.
pub(crate) fn look(observation: &ObservationData, map: &MapModel, memory: &mut Memory) {
    let now = observation.tick;
    let points = points(map, observation.me);
    let scouted = memory.scouted(points.len());
    for (point, seen) in points.iter().zip(scouted.iter_mut()) {
        let visible = tiles_of(point.anchor, BuildingKind::Foundry.size())
            .any(|tile| observation.visible(tile));
        if visible {
            *seen = now;
        }
    }
}

/// Whether `scout` flies.
fn flies(scout: &UnitObs) -> bool {
    scout.kind.stats().domain == Domain::Air
}

/// Sends `scout` to `goal`: a ground scout scouts it, which keeps out of
/// known reach on its own, and an aircraft flies around known anti-air when
/// the straight line crosses it. Returns whether the orders fit this
/// decision.
fn send(
    observation: &ObservationData,
    frame: HomeFrame,
    hazards: &[Hazard],
    scout: &UnitObs,
    goal: TilePos,
    ledger: &mut Ledger,
) -> bool {
    if !flies(scout) {
        return ledger.order(Command::Scout {
            units: vec![scout.id],
            goal,
            queue: false,
        });
    }
    let Some(via) = air::route(observation, frame, hazards, scout.tile, goal) else {
        return ledger.order(run(vec![scout.id], goal));
    };
    if ledger.room() < 2 {
        return false;
    }
    ledger.order(run(vec![scout.id], via));
    ledger.order(Command::Run {
        units: vec![scout.id],
        goal,
        queue: true,
    })
}

/// Where a scout goes to see each point, on foot or in the air. A goal does
/// not depend on which scout goes, so each is worked out once per decision,
/// when first wanted.
struct Goals<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    frame: HomeFrame,
    points: &'a [Point],
    ground: OnceCell<Vec<Option<TilePos>>>,
    air: OnceCell<Vec<Option<TilePos>>>,
}

impl Goals<'_> {
    /// Each point's goal for a scout, an aircraft when `air`, `None` where it
    /// cannot go: the point's footprint tile nearest home, since a Scout
    /// order that stops short guarantees a view of its goal tile alone.
    fn of(&self, air: bool) -> &[Option<TilePos>] {
        let frame = self.frame;
        let nearest = move |point: &Point| {
            tiles_of(point.anchor, BuildingKind::Foundry.size())
                .min_by_key(|tile| frame.rank(frame.home, doubled(*tile)))
        };
        if air {
            self.air
                .get_or_init(|| self.points.iter().map(nearest).collect())
        } else {
            // A ground scout reaches a point when a tile beside it is open
            // to it on foot.
            self.ground.get_or_init(|| {
                self.points
                    .iter()
                    .map(|point| {
                        approach(
                            self.map,
                            self.observation.me,
                            frame,
                            BuildingKind::Foundry,
                            point.anchor,
                        )
                        .and_then(|_| nearest(point))
                    })
                    .collect()
            })
        }
    }
}

/// The stale point no other scout holds that a scout with `goals` should look
/// at next and where to send it, with the point's index.
fn best(
    now: u64,
    frame: HomeFrame,
    points: &[Point],
    scouted: &[u64],
    held: &[u16],
    goals: &[Option<TilePos>],
) -> Option<(u16, TilePos)> {
    points
        .iter()
        .zip(scouted)
        .zip(goals)
        .enumerate()
        .filter(|(_, ((_, seen), _))| now - **seen >= STALE_TICKS)
        .filter(|(index, _)| u16::try_from(*index).is_ok_and(|point| !held.contains(&point)))
        .filter_map(|(index, ((point, seen), goal))| {
            let score = (now - seen).min(AGE_CAP) * point.value;
            Some((index, (*goal)?, score))
        })
        .max_by_key(|(_, goal, score)| {
            (
                *score,
                std::cmp::Reverse(frame.rank(frame.home, doubled(*goal))),
            )
        })
        .and_then(|(index, goal, _)| Some((u16::try_from(index).ok()?, goal)))
}
