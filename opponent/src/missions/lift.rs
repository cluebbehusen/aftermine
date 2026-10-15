//! The lift mission: when ground reaches no enemy target, carriers that exist
//! take a payload that exists to a landing on the target's island, set it
//! down, and the landed units fight. The mission owns only its actual
//! carriers and passengers. Production keeps a stock of carriers, and nothing
//! waits on a carrier that is not built.

use super::Scratch;
use super::air::{self, Hazard};

use super::attack::{FIT, healthy, margin, minimum, opposed, striking};
use super::{
    LiftPhase, MISSION_CAP, Mission, Missions, Objective, Task, UNIT_CAP, approach, hunt, mine,
    run, standing,
};
use crate::composition::{self, Role};
use crate::decision::Ledger;
use crate::frame::{HomeFrame, centre_distance, doubled, footprint_centre, gap, ring};
use crate::map::MapModel;
use crate::memory::Memory;
use crate::profile::ResolvedProfile;
use chassis::grid::TilePos;
use oxide_sim::observation::{CarriedUnitObs, ObservationData, UnitObs};
use oxide_sim::scenario::BotDifficulty;
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingKind, Command, UnitId, UnitKind};

const LOAD_TICKS: u64 = 1_200;
const FLY_TICKS: u64 = 2_400;
const FIGHT_TICKS: u64 = 3_600;

/// Tiles around a target's footprint searched for a landing: a computation
/// bound wide enough to find ground out of a defended base's reach.
const LANDING_REACH: i32 = 16;

/// Empty tiles a landing would best leave between itself and the target.
const LANDING_GAP: i32 = 4;

/// Tiles around the landing over which unloaded units spread.
const SPREAD: i32 = 4;

/// Tiles a carrier looks around for open ground to wait over.
const CLEARING: i32 = 4;

/// Tiles around a clearing kept free of other aircraft: a flier that meets
/// one idling beside its goal stops short of it.
const AIRSPACE: i32 = 2;

/// Ticks a lift's `phase` should end within.
pub(super) fn timeout(phase: LiftPhase) -> u64 {
    match phase {
        LiftPhase::Load => LOAD_TICKS,
        LiftPhase::Fly => FLY_TICKS,
        LiftPhase::Fight { .. } => FIGHT_TICKS,
    }
}

/// A delivery mission's members, as the observation shows them.
pub(super) struct Manifest<'a> {
    /// Its carriers, empty or loaded.
    pub(super) carriers: Vec<&'a UnitObs>,
    /// Its ground units not aboard a carrier.
    pub(super) riders: Vec<&'a UnitObs>,
    /// Its members other than carriers and workers.
    pub(super) escorts: Vec<&'a UnitObs>,
    /// Its workers not aboard a carrier.
    pub(super) workers: Vec<&'a UnitObs>,
    /// Its members aboard a carrier, or riding one of its carriers.
    pub(super) aboard: Vec<&'a CarriedUnitObs>,
}

impl<'a> Manifest<'a> {
    pub(super) fn new(observation: &'a ObservationData, members: &[UnitId]) -> Self {
        let live: Vec<_> = members
            .iter()
            .filter_map(|id| mine(observation, *id))
            .collect();
        Self {
            carriers: live.iter().copied().filter(|u| carrier(u.kind)).collect(),
            riders: live
                .iter()
                .copied()
                .filter(|u| u.kind.stats().domain == Domain::Ground)
                .collect(),
            escorts: live
                .iter()
                .copied()
                .filter(|u| !carrier(u.kind) && !crate::workers::worker(u.kind))
                .collect(),
            workers: live
                .iter()
                .copied()
                .filter(|u| crate::workers::worker(u.kind))
                .collect(),
            aboard: observation
                .my_carried_units
                .iter()
                .filter(|u| {
                    members.binary_search(&u.id).is_ok()
                        || members.binary_search(&u.carrier).is_ok()
                })
                .collect(),
        }
    }
}

/// Where carriers load, land and wait, shared by the combat lift and
/// settlements.
pub(super) struct Delivery<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    scratch: &'a Scratch,
}

impl<'a> Delivery<'a> {
    pub(super) fn new(
        observation: &'a ObservationData,
        map: &'a MapModel,
        scratch: &'a Scratch,
    ) -> Self {
        Self {
            observation,
            map,
            scratch,
        }
    }

    /// Whether a ground unit could stand on `tile` of `component`, by
    /// [`open`].
    pub(super) fn open(&self, tile: TilePos, component: Option<u32>) -> bool {
        component.is_some_and(|island| open(self.observation, self.map, tile, island))
    }

    /// Where riders bound for the building of `size` at `anchor` set down:
    /// open ground on its island least exposed to known fire, nearest
    /// `preferred_gap` empty tiles from it, with nothing but that island
    /// within a carrier's spread, so none land across a chasm. Ranking every
    /// tile in reach first leaves the costlier checks of open ground for the
    /// few tiles tried.
    pub(super) fn landing(
        &self,
        anchor: TilePos,
        size: (i32, i32),
        preferred_gap: i32,
    ) -> Option<TilePos> {
        let component = self.map.component(anchor)?;
        let mut ranked: Vec<_> = (-LANDING_REACH..size.1 + LANDING_REACH)
            .flat_map(|dy| {
                (-LANDING_REACH..size.0 + LANDING_REACH).map(move |dx| anchor.offset(dx, dy))
            })
            .filter(|tile| self.map.component(*tile) == Some(component))
            .filter_map(|tile| {
                let gap = gap(anchor, size, tile, (1, 1));
                (gap >= 1).then(|| {
                    let point = doubled(tile);
                    let exposure: u64 = self
                        .scratch
                        .air
                        .iter()
                        .chain(&self.scratch.ground)
                        .filter(|h| h.covers(point))
                        .map(|h| h.value)
                        .sum();
                    (
                        (
                            exposure,
                            (gap - preferred_gap).abs(),
                            self.scratch.frame.rank(self.scratch.frame.home, point),
                        ),
                        tile,
                    )
                })
            })
            .collect();
        ranked.sort_unstable_by_key(|(key, _)| *key);
        ranked.into_iter().map(|(_, tile)| tile).find(|tile| {
            self.open(*tile, Some(component))
                && (-SPREAD..=SPREAD).all(|dy| {
                    (-SPREAD..=SPREAD).all(|dx| {
                        self.map
                            .component(tile.offset(dx, dy))
                            .is_none_or(|c| c == component)
                    })
                })
        })
    }

    /// The open tile of `source`'s ground nearest it, away from other
    /// aircraft, where riders can reach `carrier` when it hovers over a
    /// building or other ground they cannot stand on.
    pub(super) fn clearing(&self, carrier: &UnitObs, source: TilePos) -> Option<TilePos> {
        (-CLEARING..=CLEARING)
            .flat_map(|dy| (-CLEARING..=CLEARING).map(move |dx| source.offset(dx, dy)))
            .filter(|tile| self.open(*tile, self.map.component(source)))
            .filter(|tile| {
                self.observation.my_units.iter().all(|u| {
                    u.id == carrier.id
                        || u.kind.stats().domain != Domain::Air
                        || u.tile.chebyshev(*tile) > AIRSPACE
                })
            })
            .min_by_key(|tile| {
                (
                    tile.chebyshev(source),
                    self.scratch
                        .frame
                        .rank(self.scratch.frame.home, doubled(*tile)),
                )
            })
    }

    /// The open tile beside a built own Foundry nearest `from`, clear of
    /// known anti-air if any is: where carriers coming home wait or set
    /// down. Carriers with nowhere safer still come down under fire rather
    /// than hold their passengers aloft.
    pub(super) fn pad(&self, from: TilePos) -> Option<TilePos> {
        self.observation
            .my_buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::Foundry && b.built)
            .flat_map(|b| ring(b.anchor, b.kind.size()))
            .filter(|tile| self.open(*tile, self.map.component(*tile)))
            .min_by_key(|tile| {
                (
                    self.scratch.air.iter().any(|h| h.covers(doubled(*tile))),
                    self.scratch.frame.rank(doubled(from), doubled(*tile)),
                )
            })
    }

    /// Loads `riders` at `source` into the idle empty `transports` on open
    /// ground there, first fit, while the decision has orders and `room`
    /// mission members, and calls idle carriers elsewhere to a clearing at
    /// `source` while those under way fall short. Returns the carriers and
    /// riders sent.
    pub(super) fn load(
        &self,
        source: TilePos,
        transports: &[&UnitObs],
        riders: &[&UnitObs],
        room: usize,
        ledger: &mut Ledger,
    ) -> Vec<UnitId> {
        let component = self.map.component(source);
        let required: usize = riders
            .iter()
            .map(|u| usize::from(u.kind.stats().transport_size))
            .sum();
        let mut capacity: usize = transports
            .iter()
            .filter(|u| u.cargo == 0 && !u.idle && self.map.component(u.tile) != component)
            .map(|u| usize::from(u.kind.stats().transport_capacity))
            .sum();
        let mut ready = Vec::new();
        let mut members = Vec::new();
        for transport in transports.iter().filter(|u| u.idle && u.cargo == 0) {
            if self.map.component(transport.tile) != component
                || !self.open(transport.tile, component)
            {
                if capacity >= required {
                    continue;
                }
                let from = if self.map.component(transport.tile) == component {
                    transport.tile
                } else {
                    source
                };
                if let Some(pad) = self.clearing(transport, from)
                    && ledger.order(run(vec![transport.id], pad))
                    && self.map.component(transport.tile) != component
                {
                    members.push(transport.id);
                    capacity += usize::from(transport.kind.stats().transport_capacity);
                }
                continue;
            }
            ready.push((transport.id, transport.kind.stats().transport_capacity));
            capacity += usize::from(transport.kind.stats().transport_capacity);
        }
        for (transport, riders) in pack(&ready, riders) {
            if members.len() + 1 + riders.len() > room {
                break;
            }
            let mut ids = ids(&riders);
            ids.sort_unstable();
            if !ledger.order(Command::Load {
                units: ids.clone(),
                transport,
                queue: false,
            }) {
                break;
            }
            members.push(transport);
            members.extend(ids);
        }
        members
    }

    /// Lets the empty `carriers` go: those already beside a home pad as they
    /// are, the rest once flown to the pad nearest them, around known
    /// anti-air. Returns those let go.
    pub(super) fn return_empty(&self, carriers: &[&UnitObs], ledger: &mut Ledger) -> Vec<UnitId> {
        let (home, away): (Vec<&UnitObs>, Vec<&UnitObs>) = carriers
            .iter()
            .copied()
            .filter(|u| u.cargo == 0)
            .partition(|u| {
                self.pad(u.tile)
                    .is_some_and(|pad| u.tile.chebyshev(pad) <= SPREAD)
            });
        let mut released = ids(&home);
        if let Some(first) = away.first()
            && let Some(pad) = self.pad(first.tile)
        {
            let via = air::route(
                self.observation,
                self.scratch.frame,
                &self.scratch.air,
                first.tile,
                pad,
            );
            if ledger.room() >= route_orders(via) {
                let sent = ids(&away);
                fly_to(sent.clone(), via, pad, false, ledger);
                released.extend(sent);
            }
        }
        released
    }
}

/// Sets down the idle loaded `carriers` at `at`, or sends those not yet
/// within a carrier's spread of it there.
pub(super) fn land(carriers: &[&UnitObs], at: TilePos, ledger: &mut Ledger) {
    for carrier in carriers.iter().filter(|u| u.cargo > 0 && u.idle) {
        ledger.order(if carrier.tile.chebyshev(at) <= SPREAD {
            unload(carrier.id, at, false)
        } else {
            run(vec![carrier.id], at)
        });
    }
}

/// A place to fly a payload to.
#[derive(Clone, Copy)]
struct Drop {
    target: Objective,
    landing: TilePos,
}

/// A carrier and the riders packed into it, with their value against
/// ground.
type Load = (UnitId, Vec<UnitId>, u64);

/// The own ground a lift loads on, and the tile its carriers gather beside.
#[derive(Clone, Copy)]
struct Source {
    ground: u32,
    tile: TilePos,
}

/// A lift under way, as this decision sees it.
struct Flight<'a> {
    index: usize,
    target: Objective,
    landing: TilePos,
    age: u64,
    carriers: Vec<&'a UnitObs>,
    /// Members on the ground: riders waiting to board, or set down.
    grounded: Vec<&'a UnitObs>,
    /// Members aboard the carriers.
    aboard: usize,
    /// Their value against ground.
    loaded: u64,
}

/// What one decision knows about lifting.
struct Lifting<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    frame: HomeFrame,
    delivery: Delivery<'a>,
    memory: &'a Memory,
    profile: &'a ResolvedProfile,
    home: u32,
    /// Known fire against aircraft, which carriers must avoid.
    air: &'a [Hazard],
    objectives: &'a [Objective],
    severed: bool,
    /// What offense leaves at home.
    reserve: [u64; 2],
    /// Targets another lift holds or this decision gave up on.
    held: Vec<Objective>,
}

impl Missions {
    /// Advances every lift under way, then forms another while ground
    /// reaches no enemy target and the free carriers and payload at home can
    /// carry enough to a landing no lift holds. Returns the targets given up
    /// on.
    pub(crate) fn lift(
        &mut self,
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
        ledger: &mut Ledger,
    ) -> Vec<(BuildingKind, TilePos)> {
        let Some(mut lifting) = Lifting::new(observation, map, profile, memory, scratch) else {
            return Vec::new();
        };
        let lifting_kind: fn(&Task) -> bool = |task| matches!(task, Task::Lift { .. });
        let mut given_up: Vec<Objective> = Vec::new();
        for id in self.ids(lifting_kind) {
            let Some(index) = self.index_of(id) else {
                continue;
            };
            lifting.held = self.held(lifting_kind, Some(id), &given_up);
            if let Some(target) = self.advance_lift(index, &lifting, ledger) {
                given_up.push(target);
            }
        }
        let loading = self.list.iter().any(|mission| {
            matches!(
                mission.task,
                Task::Lift {
                    phase: LiftPhase::Load,
                    ..
                }
            )
        });
        if lifting.severed || loading {
            self.clear(&lifting, ledger);
        }
        loop {
            lifting.held = self.held(lifting_kind, None, &given_up);
            if !self.form(&lifting, ledger) {
                break;
            }
        }
        given_up
            .into_iter()
            .map(|target| (target.building, target.anchor))
            .collect()
    }

    /// Moves free carriers hovering where no rider could reach them, such as
    /// over the Airworks that trained them, to open ground, at home or on the
    /// ground a lift would load at. Lifts under way spend the decision's
    /// orders first.
    fn clear(&self, lifting: &Lifting<'_>, ledger: &mut Ledger) {
        let observation = lifting.observation;
        let free: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .collect();
        let source = self
            .source(lifting, &free, None)
            .map(|(source, _)| source.ground);
        for unit in free
            .iter()
            .copied()
            .filter(|unit| carrier(unit.kind) && unit.idle && unit.cargo == 0)
            .filter(|unit| {
                lifting.map.component(unit.tile).is_some_and(|ground| {
                    (ground == lifting.home || Some(ground) == source)
                        && !lifting.open(unit.tile, ground)
                })
            })
        {
            if let Some(tile) = lifting.clearing(unit) {
                ledger.order(run(vec![unit.id], tile));
            }
        }
    }

    /// Forms a lift when none is under way and the free carriers and the
    /// riders on the seat's ground that bring the most, with any bombers out
    /// clearing the way, can meet the need of the best landing, sending every
    /// load this decision's orders allow; the rest board on later decisions.
    /// One landing of everything beats several that each meet the island's
    /// defenders alone. Returns whether one formed.
    fn form(&mut self, lifting: &Lifting<'_>, ledger: &mut Ledger) -> bool {
        let observation = lifting.observation;
        let lifting_now = self
            .list
            .iter()
            .any(|mission| matches!(mission.task, Task::Lift { .. }));
        if lifting_now || self.list.len() >= MISSION_CAP || !lifting.severed {
            return false;
        }
        let Some((source, loads)) = self.loads(lifting, None, &[], ledger) else {
            return false;
        };
        let value = loads.iter().map(|(_, _, value)| value).sum::<u64>();
        if loads.is_empty() || value < minimum(lifting.profile.stance) {
            return false;
        }
        let Some(drop) = lifting.best_drop() else {
            return false;
        };
        let need = lifting.need(drop.target, drop.landing);
        let cover = lifting.cover(self.cover(observation, drop.target.anchor));
        if value + cover < need || !lifting.looked(drop.target) {
            return false;
        }
        let mut units = lifting.load(source, &loads, UNIT_CAP, ledger);
        if units.is_empty() {
            return false;
        }
        units.sort_unstable();
        self.list.push(Mission {
            id: self.next,
            since: observation.tick,
            units,
            goal: drop.landing,
            task: Task::Lift {
                target: drop.target,
                phase: LiftPhase::Load,
            },
        });
        self.next += 1;
        self.waiting = None;
        true
    }

    /// The loads a lift could take on at `ground`, or else on the source
    /// [`source`](Self::source) picks: its free riders packed strongest value
    /// per slot first into the lift's `own` empty carriers, then into free
    /// idle empty ones nearest the source, with each load's value. A free
    /// carrier on the source over ground no rider could reach waits to be
    /// moved off it.
    fn loads(
        &self,
        lifting: &Lifting<'_>,
        ground: Option<u32>,
        own: &[&UnitObs],
        ledger: &Ledger,
    ) -> Option<(Source, Vec<Load>)> {
        let observation = lifting.observation;
        let frame = lifting.frame;
        let free: Vec<&UnitObs> = self
            .free(observation, ledger)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .collect();
        let (source, riders) = self.source(lifting, &free, ground)?;
        let gather = if source.ground == lifting.home {
            frame.home
        } else {
            doubled(source.tile)
        };
        let mut carriers: Vec<&UnitObs> = free
            .iter()
            .copied()
            .filter(|unit| carrier(unit.kind) && unit.idle && unit.cargo == 0)
            .filter(|unit| {
                lifting.map.component(unit.tile) != Some(source.ground)
                    || lifting.open(unit.tile, source.ground)
            })
            .collect();
        carriers.sort_by_key(|unit| (frame.rank(gather, doubled(unit.tile)), unit.id));
        let rooms: Vec<(UnitId, u8)> = own
            .iter()
            .copied()
            .filter(|unit| unit.cargo == 0)
            .chain(carriers)
            .map(|unit| (unit.id, unit.kind.stats().transport_capacity))
            .collect();
        let loads = pack(&rooms, &riders)
            .into_iter()
            .map(|(carrier, riders)| {
                let value = riders.iter().map(|unit| striking(unit)).sum();
                let mut riders = ids(&riders);
                riders.sort_unstable();
                (carrier, riders, value)
            })
            .collect();
        Some((source, loads))
    }

    /// The own ground a lift loads riders on, with the riders among `units`
    /// it would take there: `ground` when given, else the ground whose riders
    /// bring the most, home first on a tie. An army trained or set down
    /// where no ground route leaves goes on by carrier like the one at home;
    /// one on a target's ground walks.
    fn source<'a>(
        &self,
        lifting: &Lifting<'_>,
        units: &[&'a UnitObs],
        ground: Option<u32>,
    ) -> Option<(Source, Vec<&'a UnitObs>)> {
        let (map, frame) = (lifting.map, lifting.frame);
        let mut grounds: Vec<u32> = units
            .iter()
            .filter(|unit| rides(unit.kind))
            .filter_map(|unit| map.component(unit.tile))
            .filter(|island| ground.is_none_or(|ground| ground == *island))
            .filter(|island| !targeted(lifting.objectives, map, *island))
            .collect();
        grounds.sort_unstable();
        grounds.dedup();
        grounds
            .into_iter()
            .filter_map(|island| {
                let on: Vec<&UnitObs> = units
                    .iter()
                    .copied()
                    .filter(|unit| map.component(unit.tile) == Some(island))
                    .collect();
                let riders = self.riders(lifting, &on);
                let tile = if island == lifting.home {
                    map.start(lifting.observation.me)?
                } else {
                    riders
                        .iter()
                        .min_by_key(|unit| (frame.rank(frame.home, doubled(unit.tile)), unit.id))?
                        .tile
                };
                let value: u64 = riders.iter().map(|unit| striking(unit)).sum();
                let key = (
                    std::cmp::Reverse(value),
                    island != lifting.home,
                    frame.rank(frame.home, doubled(tile)),
                );
                Some((
                    key,
                    Source {
                        ground: island,
                        tile,
                    },
                    riders,
                ))
            })
            .min_by_key(|(key, _, _)| *key)
            .map(|(_, source, riders)| (source, riders))
    }

    /// The riders among `free` a lift takes, most value per transport slot
    /// first so a few strong units are not crowded out by many weak ones
    /// nearer home, less the weakest ones the reserve keeps.
    fn riders<'a>(&self, lifting: &Lifting<'_>, free: &[&'a UnitObs]) -> Vec<&'a UnitObs> {
        let frame = lifting.frame;
        let mut riders: Vec<&UnitObs> = free
            .iter()
            .copied()
            .filter(|unit| rides(unit.kind) && healthy(unit, FIT))
            .collect();
        riders.sort_by_key(|unit| {
            let slots = u64::from(unit.kind.stats().transport_size.max(1));
            (
                std::cmp::Reverse(striking(unit) * 1_000 / slots),
                frame.rank(frame.home, doubled(unit.tile)),
                unit.id,
            )
        });
        self.spare(lifting.observation, lifting.map, lifting.reserve)
            .trim(lifting.map, riders)
    }

    /// Whether the seat's `have` carriers, alive and queued, fall short of
    /// lifting every free rider on the ground a lift would load at once,
    /// packed as a lift packs them: `None` when they do not, and otherwise
    /// whether some of those riders already have no room. A seat wants at
    /// least one carrier.
    pub(crate) fn carriers_short(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        profile: &ResolvedProfile,
        memory: &Memory,
        scratch: &Scratch,
        have: u64,
    ) -> Option<bool> {
        let mut lifting = Lifting::new(observation, map, profile, memory, scratch)?;
        lifting.held = self.held(|task| matches!(task, Task::Lift { .. }), None, &[]);
        let units: Vec<&UnitObs> = self
            .available(observation, false)
            .into_iter()
            .filter_map(|id| mine(observation, id))
            .collect();
        let riders = self
            .source(&lifting, &units, None)
            .map(|(_, riders)| riders)
            .unwrap_or_default();
        // Carriers count in Skyhook rooms, and first fit opens a room only
        // when no earlier one fits, so the first k rooms are what k Skyhooks
        // load.
        let capacity = UnitKind::Skyhook.stats().transport_capacity;
        let rooms: Vec<u64> = pack(&vec![(UnitId(u32::MAX), capacity); riders.len()], &riders)
            .into_iter()
            .map(|(_, riders)| riders.iter().map(|unit| striking(unit)).sum())
            .collect();
        let carry = |need: u64| {
            let mut value = 0;
            let reached = rooms.iter().position(|room| {
                value += room;
                value >= need
            });
            reached.map_or(rooms.len(), |index| index + 1).max(1) as u64
        };
        let full = have >= rooms.len() as u64;
        (have < carry(u64::MAX)).then_some(!full)
    }

    fn advance_lift(
        &mut self,
        index: usize,
        lifting: &Lifting<'_>,
        ledger: &mut Ledger,
    ) -> Option<Objective> {
        let observation = lifting.observation;
        let mission = &self.list[index];
        let Task::Lift { target, phase } = mission.task else {
            unreachable!("the lift mission's task");
        };
        let manifest = Manifest::new(observation, &mission.units);
        let flight = Flight {
            index,
            target,
            landing: mission.goal,
            age: observation.tick - mission.since,
            carriers: manifest.carriers,
            grounded: manifest.riders,
            aboard: manifest.aboard.len(),
            loaded: manifest
                .aboard
                .iter()
                .map(|u| super::force(u.kind, u.hp)[0])
                .sum(),
        };
        match phase {
            LiftPhase::Load => {
                self.board(&flight, lifting, ledger);
                None
            }
            LiftPhase::Fly => {
                self.fly(&flight, lifting, ledger);
                None
            }
            LiftPhase::Fight { .. } => self.fight(&flight, lifting, ledger),
        }
    }

    /// Sends every free carrier and rider while boarding lasts, and waits
    /// while riders walk to their carriers. Once none is walking and none was
    /// sent, or time runs out, flies when what is aboard, with any bombers
    /// out clearing the way, meets the landing's need as now known, and
    /// otherwise sets everyone down and lets them go. A rider that stopped
    /// short of its carrier could not board it and is not sent again.
    fn board(&mut self, flight: &Flight<'_>, lifting: &Lifting<'_>, ledger: &mut Ledger) {
        let waiting = &flight.grounded;
        let need = lifting.need(flight.target, flight.landing);
        if flight.age < LOAD_TICKS {
            // Riders wait to board, and loaded carriers hover, on the ground
            // the lift loads at.
            let ground = waiting
                .first()
                .or_else(|| flight.carriers.iter().find(|unit| unit.cargo > 0))
                .and_then(|unit| lifting.map.component(unit.tile));
            // An empty carrier of the lift may still wait for riders walking
            // to it, so it takes others only once none is walking.
            let own: Vec<&UnitObs> = if waiting.iter().any(|unit| !unit.idle) {
                Vec::new()
            } else {
                flight
                    .carriers
                    .iter()
                    .copied()
                    .filter(|unit| unit.cargo == 0)
                    .collect()
            };
            if let Some((source, loads)) = self.loads(lifting, ground, &own, ledger) {
                let room = UNIT_CAP.saturating_sub(self.list[flight.index].units.len());
                let sent = lifting.load(source, &loads, room, ledger);
                if !sent.is_empty() {
                    let units = &mut self.list[flight.index].units;
                    units.extend(sent);
                    units.sort_unstable();
                    units.dedup();
                    return;
                }
            }
        }
        if waiting.iter().any(|unit| !unit.idle) && flight.age < LOAD_TICKS {
            return;
        }
        let cover = lifting.cover(self.cover(lifting.observation, flight.target.anchor));
        if flight.aboard > 0 && flight.loaded + cover >= need {
            self.take_off(flight, lifting, ledger);
            return;
        }
        let loaded: Vec<&&UnitObs> = flight
            .carriers
            .iter()
            .filter(|unit| unit.cargo > 0)
            .collect();
        let orders = loaded.len() + usize::from(!waiting.is_empty());
        if (ledger.room() as usize) < orders {
            return;
        }
        for carrier in loaded {
            ledger.order(unload(carrier.id, carrier.tile, false));
        }
        if !waiting.is_empty() {
            ledger.order(Command::Stop {
                units: ids(waiting),
            });
        }
        self.list.remove(flight.index);
    }

    /// Sends every loaded carrier to the landing together in one order, around
    /// known anti-air when the straight line crosses it; each sets its riders
    /// down once there. Riders still walking to a carrier stop, orders
    /// permitting, and are let go.
    fn take_off(&mut self, flight: &Flight<'_>, lifting: &Lifting<'_>, ledger: &mut Ledger) {
        let landing = flight.landing;
        let (loaded, empty): (Vec<&UnitObs>, Vec<&UnitObs>) =
            flight.carriers.iter().partition(|unit| unit.cargo > 0);
        let Some(pad) = lifting.pad(landing) else {
            return;
        };
        let from = loaded
            .first()
            .filter(|unit| lifting.map.component(unit.tile) != Some(lifting.home))
            .map_or(pad, |unit| unit.tile);
        let loaded = ids(&loaded);
        let via = lifting.route(from, landing);
        if ledger.room() < route_orders(via) {
            return;
        }
        fly_to(loaded, via, landing, false, ledger);
        if !flight.grounded.is_empty() && ledger.room() > 0 {
            ledger.order(Command::Stop {
                units: ids(&flight.grounded),
            });
        }
        let mission = &mut self.list[flight.index];
        if let Task::Lift { phase, .. } = &mut mission.task {
            *phase = LiftPhase::Fly;
        }
        mission.since = lifting.observation.tick;
        let released: Vec<UnitId> = ids(&empty)
            .into_iter()
            .chain(ids(&flight.grounded))
            .collect();
        self.release_units(flight.index, &released);
    }

    /// Sets riders down as each carrier reaches the landing, sends emptied
    /// carriers home together and frees them, sends landed riders at the
    /// target, brings back carriers still loaded when time runs out, and
    /// fights once no one is aboard and every emptied carrier is on its way
    /// home.
    fn fly(&mut self, flight: &Flight<'_>, lifting: &Lifting<'_>, ledger: &mut Ledger) {
        let map = lifting.map;
        let island = map.component(flight.target.anchor);
        let hunt_at = lifting.hunt_tile(flight.target, flight.landing);
        let pad = lifting.pad(flight.landing);
        let (loaded, empty): (Vec<&UnitObs>, Vec<&UnitObs>) =
            flight.carriers.iter().partition(|unit| unit.cargo > 0);
        let late = flight.age >= FLY_TICKS;
        let (home, away): (Vec<&UnitObs>, Vec<&UnitObs>) = loaded
            .iter()
            .partition(|u| map.component(u.tile) == Some(lifting.home));
        for carrier in home.iter().filter(|u| u.idle) {
            ledger.order(unload(carrier.id, carrier.tile, false));
        }
        if !late {
            land(&away, flight.landing, ledger);
        }
        let away: Vec<UnitId> = loaded
            .iter()
            .filter(|unit| map.component(unit.tile) != Some(lifting.home))
            .map(|unit| unit.id)
            .collect();
        if late
            && let (false, Some(pad)) = (away.is_empty(), pad)
            && ledger.order(run(away, pad))
        {
            self.list[flight.index].since = lifting.observation.tick;
        }
        let (resting, leaving): (Vec<&UnitObs>, Vec<&UnitObs>) =
            empty.iter().partition(|unit| unit.idle);
        let (parked, away): (Vec<&UnitObs>, Vec<&UnitObs>) = resting
            .iter()
            .partition(|unit| map.component(unit.tile) == Some(lifting.home));
        let mut going: Vec<UnitId> = ids(&leaving).into_iter().chain(ids(&parked)).collect();
        // A carrier freed off home ground would be stranded there, so the lift
        // flies on until every emptied one has been sent back.
        let homebound = match (away.is_empty(), pad) {
            (true, _) | (false, None) => true,
            (false, Some(pad)) => {
                let via = lifting.route(flight.landing, pad);
                let sent = ledger.room() >= route_orders(via);
                if sent {
                    fly_to(ids(&away), via, pad, false, ledger);
                    going.extend(ids(&away));
                }
                sent
            }
        };
        let (landed, astray): (Vec<&UnitObs>, Vec<&UnitObs>) = flight
            .grounded
            .iter()
            .partition(|unit| island.is_some() && map.component(unit.tile) == island);
        let delivered = loaded.is_empty() && flight.aboard == 0 && homebound;
        let idle: Vec<&UnitObs> = landed.iter().copied().filter(|unit| unit.idle).collect();
        if let (false, false, Some(hunt_at)) = (delivered, idle.is_empty(), hunt_at) {
            ledger.order(hunt(ids(&idle), hunt_at));
        }
        let released: Vec<UnitId> = going.into_iter().chain(ids(&astray)).collect();
        if !self.release_units(flight.index, &released) || !delivered {
            return;
        }
        if landed.is_empty() {
            self.list.remove(flight.index);
            return;
        }
        let goal = hunt_at.unwrap_or(flight.landing);
        let mut riders = ids(&landed);
        riders.sort_unstable();
        if ledger.order(hunt(riders, goal)) {
            let mission = &mut self.list[flight.index];
            mission.task = Task::Lift {
                target: flight.target,
                phase: LiftPhase::Fight { focus: None },
            };
            mission.since = lifting.observation.tick;
            mission.goal = goal;
        }
    }

    /// Landed riders fight the target, then the next target on the same
    /// ground, and are let go when there is none or time runs out. Returns a
    /// target they stood beside without taking.
    fn fight(
        &mut self,
        flight: &Flight<'_>,
        lifting: &Lifting<'_>,
        ledger: &mut Ledger,
    ) -> Option<Objective> {
        let target = flight.target;
        if flight.age >= FIGHT_TICKS {
            self.list.remove(flight.index);
            return None;
        }
        if !flight.grounded.iter().all(|unit| unit.idle) {
            return None;
        }
        if standing(lifting.observation, target) {
            self.list.remove(flight.index);
            return Some(target);
        }
        let (map, observation, frame) = (lifting.map, lifting.observation, lifting.frame);
        let island = map.component(target.anchor);
        let from = flight.landing;
        let next = lifting
            .objectives
            .iter()
            .copied()
            .filter(|objective| *objective != target && !lifting.held.contains(objective))
            .filter(|objective| island.is_some() && map.component(objective.anchor) == island)
            .filter(|objective| {
                !lifting
                    .memory
                    .abandoned(objective.building, objective.anchor, observation.tick)
            })
            .filter_map(|objective| Some((objective, lifting.hunt_tile(objective, from)?)))
            .min_by_key(|(_, tile)| (tile.chebyshev(from), frame.rank(frame.home, doubled(*tile))));
        let Some((next, tile)) = next else {
            self.list.remove(flight.index);
            return None;
        };
        let mut riders = ids(&flight.grounded);
        riders.sort_unstable();
        if ledger.order(hunt(riders, tile)) {
            let mission = &mut self.list[flight.index];
            mission.task = Task::Lift {
                target: next,
                phase: LiftPhase::Fight { focus: None },
            };
            mission.since = observation.tick;
            mission.goal = tile;
        }
        None
    }

    /// Takes `units` out of the mission at `index`, dropping it if empty.
    /// Returns whether it remains.
    fn release_units(&mut self, index: usize, units: &[UnitId]) -> bool {
        self.list[index].units.retain(|id| !units.contains(id));
        if self.list[index].units.is_empty() {
            self.list.remove(index);
            return false;
        }
        true
    }
}

impl Missions {
    /// The enemy building a lift goes after, or else the one the next lift
    /// would: where bombers clear the way for the carriers.
    pub(super) fn lift_focus(
        &self,
        observation: &ObservationData,
        map: &MapModel,
        memory: &Memory,
        scratch: &Scratch,
    ) -> Option<Objective> {
        self.list
            .iter()
            .find_map(|mission| match mission.task {
                Task::Lift { target, .. } => Some(target),
                _ => None,
            })
            .or_else(|| {
                targets(&scratch.objectives, observation, map, scratch.frame, memory)
                    .into_iter()
                    .next()
            })
    }
}

impl<'a> Lifting<'a> {
    fn new(
        observation: &'a ObservationData,
        map: &'a MapModel,
        profile: &'a ResolvedProfile,
        memory: &'a Memory,
        scratch: &'a Scratch,
    ) -> Option<Self> {
        let home = map
            .start(observation.me)
            .and_then(|start| map.component(start))?;
        Some(Self {
            observation,
            map,
            frame: scratch.frame,
            delivery: Delivery::new(observation, map, scratch),
            memory,
            profile,
            home,
            air: &scratch.air,
            objectives: &scratch.objectives,
            severed: scratch.severed,
            reserve: scratch.reserve,
            held: Vec::new(),
        })
    }

    /// The best target no ground route reaches and no other lift holds that
    /// has a landing, by value for its distance from home.
    fn best_drop(&self) -> Option<Drop> {
        targets(
            self.objectives,
            self.observation,
            self.map,
            self.frame,
            self.memory,
        )
        .into_iter()
        .filter(|target| !self.held.contains(target))
        .find_map(|target| {
            Some(Drop {
                target,
                landing: self.landing(target)?,
            })
        })
    }

    /// The bombers' `cover` a lift counts on: only the upper rungs time a
    /// lift to them. A difficulty limit.
    fn cover(&self, cover: u64) -> u64 {
        match self.profile.difficulty {
            BotDifficulty::Veteran | BotDifficulty::Prime => cover,
            BotDifficulty::Scrapheap | BotDifficulty::Standard => 0,
        }
    }

    /// Whether the scouting point nearest `target` on its island was in
    /// sight recently enough to know the army a landing would meet. An
    /// island no point lies on cannot be looked at, so it counts as known.
    fn looked(&self, target: Objective) -> bool {
        // The lowest rung lifts without looking first: a difficulty limit.
        if self.profile.difficulty == BotDifficulty::Scrapheap {
            return true;
        }
        let island = self.map.component(target.anchor);
        super::points(self.map, self.observation.me)
            .iter()
            .enumerate()
            .filter(|(_, point)| self.map.component(point.anchor) == island)
            .min_by_key(|(_, point)| {
                (
                    point.anchor.chebyshev(target.anchor),
                    self.frame.rank(self.frame.home, doubled(point.anchor)),
                )
            })
            .is_none_or(|(index, _)| self.memory.looked(index, self.observation.tick))
    }

    /// Army value a lift to `target` set down at `landing` needs: the known
    /// army it would meet, with the guns at the target the riders walk into
    /// as well as those at the landing, times the margin, and never under the
    /// stance minimum.
    fn need(&self, target: Objective, landing: TilePos) -> u64 {
        (opposed(
            self.observation,
            self.map,
            self.memory,
            &[landing, target.anchor],
        ) * margin(self.profile.difficulty)
            / 1_000)
            .max(minimum(self.profile.stance))
    }

    /// Where to set a payload down near `target`: known open ground on the
    /// target's island, out of reach of known fire where any is, then as near
    /// `LANDING_GAP` empty tiles from the target as it can be, so riders set
    /// down out of range walk in. Every ground tile riders could be set down
    /// on lies on the target's island, so none land across a chasm.
    fn landing(&self, target: Objective) -> Option<TilePos> {
        self.delivery
            .landing(target.anchor, target.building.size(), LANDING_GAP)
    }

    /// Whether a ground unit could stand on `tile` of `island`, by [`open`].
    fn open(&self, tile: TilePos, island: u32) -> bool {
        open(self.observation, self.map, tile, island)
    }

    fn clearing(&self, carrier: &UnitObs) -> Option<TilePos> {
        self.delivery.clearing(carrier, carrier.tile)
    }

    fn load(
        &self,
        source: Source,
        loads: &[Load],
        room: usize,
        ledger: &mut Ledger,
    ) -> Vec<UnitId> {
        let transports: Vec<_> = loads
            .iter()
            .filter_map(|(id, _, _)| mine(self.observation, *id))
            .collect();
        let riders: Vec<_> = loads
            .iter()
            .flat_map(|(_, riders, _)| riders)
            .filter_map(|id| mine(self.observation, *id))
            .collect();
        self.delivery
            .load(source.tile, &transports, &riders, room, ledger)
    }

    fn pad(&self, landing: TilePos) -> Option<TilePos> {
        air::pad(self.observation, self.map, self.frame, doubled(landing))
    }

    fn route(&self, from: TilePos, to: TilePos) -> Option<TilePos> {
        air::route(self.observation, self.frame, self.air, from, to)
    }

    /// The tile beside `target` on its island nearest `from`.
    fn hunt_tile(&self, target: Objective, from: TilePos) -> Option<TilePos> {
        let island = self.map.component(target.anchor)?;
        ring(target.anchor, target.building.size())
            .filter(|tile| self.map.component(*tile) == Some(island))
            .min_by_key(|tile| {
                (
                    tile.chebyshev(from),
                    self.frame.rank(self.frame.home, doubled(*tile)),
                )
            })
    }
}

/// Orders [`fly_to`] issues.
pub(super) fn route_orders(via: Option<TilePos>) -> u32 {
    1 + u32::from(via.is_some())
}

/// Whether a ground unit could stand on `tile` of `island` as far as the seat
/// knows: no known building, its own, an ally's or an enemy's, and no rock or
/// scrap covers it.
pub(super) fn open(
    observation: &ObservationData,
    map: &MapModel,
    tile: TilePos,
    island: u32,
) -> bool {
    map.component(tile) == Some(island)
        && !observation.known_scrap_at(tile)
        && !observation.known_rock_at(tile)
        && !observation
            .enemy_buildings
            .iter()
            .chain(&observation.my_buildings)
            .chain(&observation.ally_buildings)
            .any(|building| gap(building.anchor, building.kind.size(), tile, (1, 1)) < 0)
}

/// Sends `carriers` to `goal` in one group order, or around known anti-air by
/// `via` first in two.
pub(super) fn fly_to(
    carriers: Vec<UnitId>,
    via: Option<TilePos>,
    goal: TilePos,
    engage: bool,
    ledger: &mut Ledger,
) {
    let movement = |units, goal, queue| {
        if engage {
            Command::Hunt { units, goal, queue }
        } else {
            Command::Run { units, goal, queue }
        }
    };
    match via {
        Some(via) => {
            ledger.order(movement(carriers.clone(), via, false));
            ledger.order(movement(carriers, goal, true));
        }
        None => {
            ledger.order(movement(carriers, goal, false));
        }
    }
}

/// The targets no ground route reaches and not given up on, most valuable for
/// their distance from home first.
fn targets(
    objectives: &[Objective],
    observation: &ObservationData,
    map: &MapModel,
    frame: HomeFrame,
    memory: &Memory,
) -> Vec<Objective> {
    let Some(start) = map.start(observation.me) else {
        return Vec::new();
    };
    let mut ranked = objectives
        .iter()
        .copied()
        .filter(|objective| {
            approach(
                map,
                observation.me,
                frame,
                objective.building,
                objective.anchor,
            )
            .is_none()
        })
        .filter(|objective| {
            !memory.abandoned(objective.building, objective.anchor, observation.tick)
        })
        .map(|target| {
            let cost = target
                .building
                .base_stats()
                .construction
                .as_ref()
                .map_or(0, |construction| construction.cost);
            let distance = centre_distance(
                footprint_centre(BuildingKind::Foundry, start),
                footprint_centre(target.building, target.anchor),
            );
            let score = u64::from(cost) * 1_000 / (100 + distance);
            let rank = frame.rank(frame.home, footprint_centre(target.building, target.anchor));
            ((std::cmp::Reverse(score), rank), target)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(order, _)| *order);
    ranked.into_iter().map(|(_, target)| target).collect()
}

/// Assigns `riders` in order to the first carrier with room, each carrier
/// given as its id and remaining room. Carriers left empty are omitted.
pub(super) fn pack<'a>(
    carriers: &[(UnitId, u8)],
    riders: &[&'a UnitObs],
) -> Vec<(UnitId, Vec<&'a UnitObs>)> {
    let mut rooms: Vec<(UnitId, u8, Vec<&UnitObs>)> = carriers
        .iter()
        .map(|(id, room)| (*id, *room, Vec::new()))
        .collect();
    for rider in riders {
        let size = rider.kind.stats().transport_size;
        if let Some(slot) = rooms.iter_mut().find(|(_, room, _)| *room >= size) {
            slot.1 -= size;
            slot.2.push(rider);
        }
    }
    rooms
        .into_iter()
        .filter(|(_, _, riders)| !riders.is_empty())
        .map(|(id, _, riders)| (id, riders))
        .collect()
}

/// Whether `kind` carries others.
pub(crate) fn carrier(kind: UnitKind) -> bool {
    kind.stats().transport_capacity > 0
}

/// The value against ground of the units a lift could take now: those at
/// home, and those `free` of missions on the other own ground, none of it
/// `targeted`, where they bring the most. `free` is in id order.
pub(crate) fn payload(
    observation: &ObservationData,
    map: &MapModel,
    targeted: &[u32],
    free: &[UnitId],
) -> (u64, u64) {
    let home = map
        .start(observation.me)
        .and_then(|start| map.component(start));
    let mut grounds: Vec<(u32, u64)> = observation
        .my_units
        .iter()
        .filter(|unit| rides(unit.kind) && healthy(unit, FIT))
        .filter_map(|unit| Some((map.component(unit.tile)?, unit)))
        .filter(|(ground, unit)| {
            Some(*ground) == home
                || (!targeted.contains(ground) && free.binary_search(&unit.id).is_ok())
        })
        .map(|(ground, unit)| (ground, striking(unit)))
        .collect();
    grounds.sort_unstable_by_key(|(ground, _)| *ground);
    let mut at_home = 0;
    let mut elsewhere = 0;
    for chunk in grounds.chunk_by(|a, b| a.0 == b.0) {
        let value: u64 = chunk.iter().map(|(_, value)| value).sum();
        if Some(chunk[0].0) == home {
            at_home = value;
        } else {
            elsewhere = elsewhere.max(value);
        }
    }
    (at_home, elsewhere)
}

/// Whether a target stands on `ground`, which units there walk to.
fn targeted(objectives: &[Objective], map: &MapModel, ground: u32) -> bool {
    objectives
        .iter()
        .any(|objective| map.component(objective.anchor) == Some(ground))
}

/// Whether `kind` is a line, siege or anti-heavy unit a carrier can take.
fn rides(kind: UnitKind) -> bool {
    matches!(
        composition::role(kind),
        Some(Role::Line | Role::Siege | Role::AntiHeavy)
    ) && kind.stats().transport_size > 0
}

fn ids(units: &[&UnitObs]) -> Vec<UnitId> {
    units.iter().map(|unit| unit.id).collect()
}

fn unload(transport: UnitId, at: TilePos, queue: bool) -> Command {
    Command::Unload {
        transport,
        at,
        queue,
    }
}
