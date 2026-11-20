//! Scouting: a scout looks at each place the seat has not seen for a while,
//! first the one whose time unseen weighted by its value is greatest, then
//! the next. For stale places no scout out or queued would see in time it
//! asks production for more.

use super::Scratch;
use super::air::{self, Hazard};
use super::{MISSION_CAP, Mission, Missions, Task, approach, defense, mine, run};
use crate::decision::Ledger;
use crate::events::OwnEvent;
use crate::frame::{HomeFrame, doubled, footprint_centre, octile_tenths, tiles_of};
use crate::map::MapModel;
use crate::memory::Memory;
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, Command, PlayerId, StallReason, UnitKind};
use chassis::fx::Fx;
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

/// Whether the seat saw `owner`'s start lately enough that its scouting
/// point is not yet worth another look.
pub(crate) fn seen_lately(
    map: &MapModel,
    memory: &Memory,
    me: PlayerId,
    owner: PlayerId,
    now: u64,
) -> bool {
    // Hostile starts lead `points` in this order.
    let Some(point) = map
        .hostiles(me)
        .filter(|hostile| map.start(*hostile).is_some())
        .position(|hostile| hostile == owner)
    else {
        return false;
    };
    let seen = memory.seen(point);
    seen > 0 && now - seen < STALE_TICKS
}

impl Missions {
    /// Keeps a scout looking at each stale place, no two at one place.
    /// Returns how many stale places no scout, out or queued, would see
    /// within [`TRAVEL_TICKS`] that the scout production would train could
    /// reach, which production trains scouts for: an aircraft once an
    /// Airworks stands, else a Surveyor. Zero while no mission could take
    /// another.
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
                    // Standing where the tour ended would keep its last
                    // places fresh in its own sight, so it would never be
                    // sent on, and it would wait there for whatever finds it.
                    if !defense::at_home(observation, map, scout.tile)
                        && let Some(post) = defense::home_post(observation, map, frame, scout.tile)
                    {
                        if flies(scout) {
                            send(observation, frame, hazards, scout, post, ledger);
                        } else {
                            ledger.order(run(vec![scout.id], post));
                        }
                    }
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
        let mut taken = held(self, None);
        let board = Board {
            now,
            frame,
            points: &points,
            scouted,
        };
        // Scouts out take the points they would see after their own, ground
        // scouts first as above; then scouts queued at producers do.
        let mut out: Vec<(&UnitObs, TilePos)> = self
            .list
            .iter()
            .filter(|mission| scouting(&mission.task))
            .filter_map(|mission| Some((mine(observation, mission.units[0])?, mission.goal)))
            .collect();
        out.sort_by_key(|(unit, _)| {
            (
                flies(unit),
                frame.rank(frame.home, doubled(unit.tile)),
                unit.id,
            )
        });
        for (unit, goal) in out {
            let tourer = Tourer {
                from: doubled(goal),
                spent: travel(doubled(unit.tile), goal, unit.kind),
                kind: unit.kind,
            };
            board.tour(tourer, goals.of(flies(unit)), &mut taken);
        }
        for (building, queue) in observation.my_buildings.iter().zip(&observation.my_queues) {
            for kind in queue.iter().copied().filter(|kind| is_scout(*kind)) {
                let tourer = Tourer {
                    from: footprint_centre(building.kind, building.anchor),
                    spent: 0,
                    kind,
                };
                board.tour(
                    tourer,
                    goals.of(kind.stats().domain == Domain::Air),
                    &mut taken,
                );
            }
        }
        let air = crate::investments::built(observation, BuildingKind::Airworks);
        let reach = goals.of(air);
        scouted
            .iter()
            .enumerate()
            .filter(|(index, seen)| {
                // Scouts already standing set out at once; another is
                // trained only for a place unseen that long in play, so the
                // opening's first purchases stay the economy's and army's.
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

/// Whether `kind` is trained to scout.
fn is_scout(kind: UnitKind) -> bool {
    kind == UnitKind::Kestrel || kind.stats().scouts
}

/// Ticks a `kind` takes from `from` to see `goal`, in a straight line.
fn travel(from: (i64, i64), goal: TilePos, kind: UnitKind) -> u64 {
    let tenths = octile_tenths(from, doubled(goal));
    (Fx::from_num(tenths) / (Fx::from_num(10) * kind.stats().speed))
        .ceil()
        .to_num::<u64>()
}

/// What every scout's tour reads in one decision.
struct Board<'a> {
    now: u64,
    frame: HomeFrame,
    points: &'a [Point],
    scouted: &'a [u64],
}

/// A scout as a tour starts it: where it will be, after how many ticks.
#[derive(Clone, Copy)]
struct Tourer {
    from: (i64, i64),
    spent: u64,
    kind: UnitKind,
}

impl Board<'_> {
    /// Adds to `taken` each stale point `tourer` would see within
    /// [`TRAVEL_TICKS`], one after another in the order [`best`] would hand
    /// them to it, so no other scout is trained for them.
    fn tour(&self, tourer: Tourer, goals: &[Option<TilePos>], taken: &mut Vec<u16>) {
        let Tourer {
            mut from,
            mut spent,
            kind,
        } = tourer;
        while let Some((point, goal)) = best(
            self.now,
            self.frame,
            self.points,
            self.scouted,
            taken,
            goals,
        ) {
            spent = spent.saturating_add(travel(from, goal, kind));
            if spent > TRAVEL_TICKS {
                break;
            }
            taken.push(point);
            from = doubled(goal);
        }
    }
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
    /// cannot go or is in sight now: the point's footprint tile nearest home, since a Scout
    /// order that stops short guarantees a view of its goal tile alone.
    fn of(&self, air: bool) -> &[Option<TilePos>] {
        let frame = self.frame;
        let observation = self.observation;
        // A point in sight now needs no scout; at the first tick it reads as
        // never seen.
        let nearest = move |point: &Point| {
            let tiles = tiles_of(point.anchor, BuildingKind::Foundry.size());
            if tiles.clone().any(|tile| observation.visible(tile)) {
                return None;
            }
            tiles.min_by_key(|tile| frame.rank(frame.home, doubled(*tile)))
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

/// Ticks a point last in sight at `seen` has gone unseen by `now`. A point
/// never seen, recorded at zero, counts as unseen for as long as the ranking
/// counts, so the seat's scouts set out from the first decision.
fn age(now: u64, seen: u64) -> u64 {
    if seen == 0 { AGE_CAP } else { now - seen }
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
        .filter(|(_, ((_, seen), _))| age(now, **seen) >= STALE_TICKS)
        .filter(|(index, _)| u16::try_from(*index).is_ok_and(|point| !held.contains(&point)))
        .filter_map(|(index, ((point, seen), goal))| {
            let score = age(now, *seen).min(AGE_CAP) * point.value;
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

#[cfg(test)]
mod tests;
