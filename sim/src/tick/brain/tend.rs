//! Tending: workers welding wounded own ground machines and damaged own
//! buildings they come across, on a patrol leg or on a trip out from an idle
//! post.

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
use crate::state::{Building, Goal, Order, State, Unit};
use crate::stats::UnitKind;
use crate::stats::{Domain, ESCORT_ENGAGE_RADIUS, next_paid_unit_repair_debit};
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
}

impl Structure {
    /// The point of the footprint nearest `from`, which range and rank
    /// measure to, as they do for every footprint.
    fn closest(&self, from: Vec2Fx) -> Vec2Fx {
        footprint_contact(from, self.anchor, self.size)
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
        for unit in state.units.iter().filter(|u| weldable(u)) {
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
                });
            }
        }
        Self {
            machines,
            structures,
            banks: state.players.iter().map(|player| player.scrap).collect(),
        }
    }
}

/// Whether a field weld may take this machine on: alive, wounded,
/// repairable, on the ground, and not being dismantled.
fn weldable(unit: &Unit) -> bool {
    unit.hp > 0
        && unit.hp < unit.kind.stats().max_hp
        && unit.kind.stats().repairable()
        && unit.domain() == Domain::Ground
        && !unit.retiring()
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
pub(super) fn start(state: &mut State, reach: &mut Reach, wounded: &Wounded, id: UnitId) -> bool {
    let unit = state.unit(id).expect("caller checked");
    // A worker still easing off a dock is not at its post yet.
    if !unit.idle_weld() || !unit.work_stopped() || !unit.queue.is_empty() {
        return false;
    }
    let post = unit.tile();
    let Some(patient) = pick(state, reach, wounded, id, post.center()) else {
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
/// post, and gives up its patient once the owner switches idle welding off.
/// A machine heals through the field weld commit, a building through the
/// building-work resolver.
#[allow(clippy::too_many_arguments)]
pub(super) fn tend(
    state: &mut State,
    index: &UnitIndex,
    reach: &mut Reach,
    wounded: &Wounded,
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
        weld.filter(|&patient| keeps(state, wounded, id, patient, center, meter))
            .or_else(|| pick(state, reach, wounded, id, center))
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
            if approach_patient(state, id, patient) {
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
            if contact::approach_worker(state, id, building) {
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
/// and, for a machine, standing still or on its way to weld the welder back.
/// Two wounded workers that took each other on keep their patients while both
/// walk over, rather than each letting go of the other for moving.
fn keeps(
    state: &State,
    wounded: &Wounded,
    welder: UnitId,
    patient: Target,
    center: Vec2Fx,
    meter: u32,
) -> bool {
    match patient {
        Target::Unit(patient) => machines_in_range(state, wounded, welder, center, meter)
            .any(|p| p.id == patient && (p.still || p.torch == Some(welder))),
        Target::Building(patient) => {
            structures_in_range(state, wounded, welder, center, meter).any(|s| s.id == patient)
        }
    }
}

/// The patient `welder` takes on next: of the machines standing still and the
/// buildings in range, the first by [`patient_rank`] that a route can bring it
/// beside. A patient no route reaches is never picked, so no welder plans
/// toward it tick after tick.
fn pick(
    state: &State,
    reach: &mut Reach,
    wounded: &Wounded,
    welder: UnitId,
    center: Vec2Fx,
) -> Option<Target> {
    let unit = state.unit(welder).expect("caller checked");
    let mut candidates: Vec<_> = machines_in_range(state, wounded, welder, center, 0)
        .filter(|p| p.still)
        .map(|p| patient_rank(unit, p.pos, Target::Unit(p.id)))
        .chain(
            structures_in_range(state, wounded, welder, center, 0)
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
                reach.reaches(state, welder, rect_adjacent_tiles(b.anchor, b.kind.size()))
            }
        })
}
