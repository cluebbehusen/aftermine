//! Tending: workers welding wounded own ground machines and damaged own
//! buildings they come across, on a patrol leg or on a trip out from an idle
//! post. A trip from an idle post takes on no building its team knows to be
//! in danger, judged as autonomous harvest work judges it, and at most
//! [`TEND_CREW_CAP`] such welders hold one building.

use super::super::reach::Reach;
use super::super::spatial::UnitIndex;
use super::economy::{
    BuildingWeld, approach_patient, metered, patient_approaches, patient_rank, weld_building,
};
use super::locomotion::walk;
use super::{PendingFieldWeld, PendingHpGain, contact};
use crate::event::Event;
use crate::geometry::{footprint_contact, rect_adjacent_tiles};
use crate::ids::{BuildingId, Target, UnitId};
use crate::state::{Building, Goal, Order, State};
use crate::stats::UnitKind;
use crate::stats::{ESCORT_ENGAGE_RADIUS, TEND_CREW_CAP, next_paid_unit_repair_debit};
use crate::vision::GroundSalvageDanger;
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;

/// A wounded machine a tending welder may take on, as the brain phase began.
struct Machine {
    id: UnitId,
    kind: UnitKind,
    pos: Vec2Fx,
    /// It held still for a weld.
    still: bool,
    /// The machine it was itself welding.
    torch: Option<UnitId>,
}

/// A damaged building a tending welder may take on, as the brain phase began.
struct Structure {
    id: BuildingId,
    weld: BuildingWeld,
    anchor: TilePos,
    size: (i32, i32),
    /// The welders a trip from an idle post holds on it, nearest first by
    /// [`patient_rank`] from each welder, then id: one owner's crew, so
    /// mirrored seats rank alike.
    crew: Vec<UnitId>,
}

impl Structure {
    /// The point of the footprint nearest `from`, which range and rank
    /// measure to, as they do for every footprint.
    fn closest(&self, from: Vec2Fx) -> Vec2Fx {
        footprint_contact(from, self.anchor, self.size)
    }

    /// Whether `welder`, on a trip from an idle post, may hold this building:
    /// among the first [`TEND_CREW_CAP`] of its crew, or a newcomer while
    /// the crew is short. Newcomers that join together past the cap settle
    /// at the next phase, when the last-ranked let go.
    fn admits(&self, welder: UnitId) -> bool {
        self.crew
            .iter()
            .position(|&held| held == welder)
            .map_or(self.crew.len() < TEND_CREW_CAP, |rank| rank < TEND_CREW_CAP)
    }
}

/// Every player's eligible patients and bank as the brain phase began.
///
/// Brain-phase scratch: positions hold still until movement, and reading
/// stillness, torches and banks here keeps each welder's choice independent
/// of whether a patient's brain or another welder's bill comes first in
/// this tick's parity order. Commands have already run, so a building a
/// salvage command claimed this tick is no patient. A player with nothing
/// damaged costs a tending welder nothing to check.
pub(super) struct Wounded {
    machines: Vec<Vec<Machine>>,
    structures: Vec<Vec<Structure>>,
    banks: Vec<u32>,
}

impl Wounded {
    pub(super) fn capture(state: &State) -> Self {
        let mut machines: Vec<Vec<Machine>> = state.players.iter().map(|_| Vec::new()).collect();
        for unit in state.units.iter().filter(|u| u.weldable()) {
            machines[usize::from(unit.player.0)].push(Machine {
                id: unit.id,
                kind: unit.kind,
                pos: unit.pos,
                still: unit.still_for_weld(),
                torch: unit.order.torch(),
            });
        }
        let mut structures: Vec<Vec<Structure>> =
            state.players.iter().map(|_| Vec::new()).collect();
        let mut damaged = state.buildings.iter().filter(|b| mendable(b)).peekable();
        if damaged.peek().is_some() {
            let salvaged = super::salvage_commitments(state);
            for b in damaged.filter(|b| !salvaged.contains(&b.id)) {
                structures[usize::from(b.player.0)].push(Structure {
                    id: b.id,
                    weld: BuildingWeld::of(b.kind, b.tier),
                    anchor: b.anchor,
                    size: b.kind.size(),
                    crew: Vec::new(),
                });
            }
            crews(state, &mut structures);
        }
        Self {
            machines,
            structures,
            banks: state.players.iter().map(|player| player.scrap).collect(),
        }
    }
}

/// Fills each structure's crew with the welders on trips from idle posts
/// that hold it, ranked nearest first.
fn crews(state: &State, structures: &mut [Vec<Structure>]) {
    let mut ranked: Vec<Vec<Vec<_>>> = structures
        .iter()
        .map(|owned| owned.iter().map(|_| Vec::new()).collect())
        .collect();
    for unit in state.units.iter().filter(|u| u.hp > 0 && !u.looping) {
        let Order::Tend {
            weld: Some(Target::Building(held)),
            ..
        } = unit.order
        else {
            continue;
        };
        let player = usize::from(unit.player.0);
        if let Some(index) = structures[player].iter().position(|s| s.id == held) {
            let point = structures[player][index].closest(unit.pos);
            ranked[player][index].push(patient_rank(unit, point, Target::Unit(unit.id)));
        }
    }
    for (owned, ranks) in structures.iter_mut().zip(ranked) {
        for (structure, mut crew) in owned.iter_mut().zip(ranks) {
            crew.sort_unstable();
            structure.crew = crew
                .into_iter()
                .map(|(_, _, welder)| match welder {
                    Target::Unit(id) => id,
                    Target::Building(_) => unreachable!("a crew holds welders"),
                })
                .collect();
        }
    }
}

/// Whether tending may repair this building: complete, standing, damaged,
/// and not hidden, since a welder at work would give a hidden charge away.
fn mendable(building: &Building) -> bool {
    building.built()
        && building.hp > 0
        && building.hp < building.stats().max_hp
        && !building.kind.is_stealthy()
}

/// Sends an idle worker on a trip to the first patient around its post,
/// returning whether it went. The trip's goal is the post itself, so the
/// worker walks home once nothing near it needs a weld.
pub(super) fn start(
    state: &mut State,
    reach: &mut Reach,
    wounded: &Wounded,
    dangers: &mut [Option<GroundSalvageDanger>],
    id: UnitId,
) -> bool {
    let unit = state.unit(id).expect("caller checked");
    // A worker still easing off a dock is not at its post yet.
    if !unit.idle_weld() || !unit.work_stopped() || !unit.queue.is_empty() {
        return false;
    }
    let post = unit.tile();
    let Some(patient) = pick(state, reach, wounded, id, post.center(), Some(dangers)) else {
        return false;
    };
    let unit = state.unit_mut(id).expect("caller checked");
    unit.order = Order::Tend {
        goal: Goal::at(post),
        weld: Some(patient),
    };
    unit.path = None;
    unit.progress = 0;
    unit.settled = 0;
    true
}

/// One tick of [`Order::Tend`] for `id`: weld the patient it holds or the
/// next one it finds, and otherwise walk on toward its goal. A patrol leg
/// searches around the welder; a trip from an idle post searches around the
/// post, and gives up its patient once the owner switches idle welding off;
/// only such a trip keeps to the crew cap and out of known danger.
/// A machine heals through the field weld commit, a building through the
/// building-work resolver.
#[allow(clippy::too_many_arguments)]
pub(super) fn tend(
    state: &mut State,
    index: &UnitIndex,
    reach: &mut Reach,
    wounded: &Wounded,
    dangers: &mut [Option<GroundSalvageDanger>],
    id: UnitId,
    welds: &mut Vec<PendingFieldWeld>,
    builds: &mut Vec<PendingHpGain>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    let Order::Tend { goal, weld } = unit.order else {
        unreachable!("the brain dispatches only a tending walk here");
    };
    let (looping, meter) = (unit.looping, metered(unit.progress));
    let center = if looping {
        unit.pos
    } else {
        goal.tile().center()
    };
    let patient = if looping || unit.idle_weld() {
        weld.filter(|&patient| keeps(state, wounded, id, patient, center, (meter, !looping)))
            .or_else(|| {
                pick(
                    state,
                    reach,
                    wounded,
                    id,
                    center,
                    (!looping).then_some(dangers),
                )
            })
    } else {
        None
    };
    if patient != weld {
        set_weld(state, id, patient);
    }
    match patient {
        Some(Target::Unit(patient)) => {
            let (welder, wounded) = (
                state.unit(id).expect("caller checked"),
                state.unit(patient).expect("a captured patient lives"),
            );
            if welder.work_stopped() && welder.in_repair_reach(wounded) {
                state.unit_mut(id).expect("caller checked").path = None;
                welds.push(PendingFieldWeld {
                    welder: id,
                    patient,
                });
                return;
            }
            if approach_patient(state, index, id, patient) {
                return;
            }
            set_weld(state, id, None);
        }
        Some(Target::Building(building)) => {
            let welder = state.unit(id).expect("caller checked");
            if welder.work_stopped() && state.in_building_work_reach(welder, building) {
                // A bank that cannot pay only skips the tick: the patient
                // keeps its meter, and the next phase lets it go unless the
                // bank recovers.
                weld_building(state, id, building, builds);
                return;
            }
            if contact::approach_worker(state, index, id, building) {
                return;
            }
            set_weld(state, id, None);
        }
        None => {}
    }
    walk(state, index, reach, id, events);
}

/// Writes the welder's patient, resetting its billing meter when the patient
/// changes: the meter ramps along one patient's clock.
fn set_weld(state: &mut State, id: UnitId, patient: Option<Target>) {
    let unit = state.unit_mut(id).expect("caller checked");
    if let Order::Tend { weld, .. } = &mut unit.order
        && *weld != patient
    {
        *weld = patient;
        unit.progress = 0;
    }
}

/// The wounded machines of `welder`'s owner within reach of `center` that its
/// bank can pay to weld from `meter` on.
fn machines_in_range<'a>(
    state: &State,
    wounded: &'a Wounded,
    welder: UnitId,
    center: Vec2Fx,
    meter: u32,
) -> impl Iterator<Item = &'a Machine> {
    let player = usize::from(state.unit(welder).expect("caller checked").player.0);
    let bank = wounded.banks[player];
    let reach_sq = ESCORT_ENGAGE_RADIUS * ESCORT_ENGAGE_RADIUS;
    wounded.machines[player].iter().filter(move |p| {
        p.id != welder
            && p.pos.dist_sq(center) <= reach_sq
            && next_paid_unit_repair_debit(p.kind, meter) <= bank
    })
}

/// The damaged buildings of `welder`'s owner whose footprint lies within
/// reach of `center` and that its bank can pay to weld from `meter` on.
fn structures_in_range<'a>(
    state: &State,
    wounded: &'a Wounded,
    welder: UnitId,
    center: Vec2Fx,
    meter: u32,
) -> impl Iterator<Item = &'a Structure> {
    let player = usize::from(state.unit(welder).expect("caller checked").player.0);
    let bank = u64::from(wounded.banks[player]);
    let reach_sq = ESCORT_ENGAGE_RADIUS * ESCORT_ENGAGE_RADIUS;
    wounded.structures[player].iter().filter(move |s| {
        s.closest(center).dist_sq(center) <= reach_sq && s.weld.next_paid(meter) <= bank
    })
}

/// Whether `welder` goes on welding `patient`: still in range and payable,
/// for a building on a trip from an idle post within the crew cap, and, for
/// a machine, standing still or on its way to weld the welder back. Danger
/// arising at a building does not send away a welder already on it.
/// Two wounded workers that took each other on keep their patients while both
/// walk over, rather than each letting go of the other for moving.
fn keeps(
    state: &State,
    wounded: &Wounded,
    welder: UnitId,
    patient: Target,
    center: Vec2Fx,
    (meter, idle): (u32, bool),
) -> bool {
    match patient {
        Target::Unit(patient) => machines_in_range(state, wounded, welder, center, meter)
            .any(|p| p.id == patient && (p.still || p.torch == Some(welder))),
        Target::Building(patient) => structures_in_range(state, wounded, welder, center, meter)
            .any(|s| s.id == patient && (!idle || s.admits(welder))),
    }
}

/// The patient `welder` takes on next: of the machines standing still and the
/// buildings in range, the first by [`patient_rank`] that a route can bring it
/// beside. A patient no route reaches is never picked, so no welder plans
/// toward it tick after tick. With the team's `dangers`, as on a trip from an
/// idle post, a building at its crew cap or with a footprint tile in known
/// danger is passed over; danger is a property of the building, so every
/// welder reads it alike.
fn pick(
    state: &State,
    reach: &mut Reach,
    wounded: &Wounded,
    welder: UnitId,
    center: Vec2Fx,
    mut dangers: Option<&mut [Option<GroundSalvageDanger>]>,
) -> Option<Target> {
    let unit = state.unit(welder).expect("caller checked");
    let idle = dangers.is_some();
    let mut candidates: Vec<_> = machines_in_range(state, wounded, welder, center, 0)
        .filter(|p| p.still)
        .map(|p| patient_rank(unit, p.pos, Target::Unit(p.id)))
        .chain(
            structures_in_range(state, wounded, welder, center, 0)
                .filter(|s| !idle || s.admits(welder))
                .map(|s| patient_rank(unit, s.closest(unit.pos), Target::Building(s.id))),
        )
        .collect();
    candidates.sort_unstable();
    candidates
        .into_iter()
        .map(|(_, _, patient)| patient)
        .find(|&patient| match patient {
            Target::Unit(id) => {
                let patient = state.unit(id).expect("a captured patient lives");
                let tiles = patient_approaches(unit, patient).map(|(tile, _)| tile);
                reach.reaches(state, welder, tiles)
            }
            Target::Building(id) => {
                let b = state.building(id).expect("a captured patient stands");
                if let Some(dangers) = dangers.as_deref_mut() {
                    let danger = super::team_danger(dangers, state, welder);
                    if b.tiles().any(|tile| danger.contains(tile)) {
                        return false;
                    }
                }
                reach.reaches(state, welder, rect_adjacent_tiles(b.anchor, b.kind.size()))
            }
        })
}
