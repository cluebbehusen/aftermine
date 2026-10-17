//! Tending: workers welding wounded own ground machines they come across,
//! on a patrol leg or on a trip out from an idle post.

use super::super::reach::Reach;
use super::super::spatial::UnitIndex;
use super::PendingFieldWeld;
use super::economy::{approach_patient, metered, patient_approaches, patient_rank};
use super::locomotion::walk;
use crate::event::Event;
use crate::ids::UnitId;
use crate::state::{Goal, Order, State, Unit};
use crate::stats::UnitKind;
use crate::stats::{Domain, ESCORT_ENGAGE_RADIUS, next_paid_unit_repair_debit};
use chassis::fx::Vec2Fx;

/// A wounded machine a tending welder may take on, as the brain phase began.
struct Patient {
    id: UnitId,
    kind: UnitKind,
    pos: Vec2Fx,
    /// It held still for a weld.
    still: bool,
    /// The machine it was itself welding.
    torch: Option<UnitId>,
}

/// Every player's eligible patients and bank as the brain phase began.
///
/// Brain-phase scratch: positions hold still until movement, and reading
/// stillness, torches and banks here keeps each welder's choice independent
/// of whether a patient's brain or another welder's bill comes first in
/// this tick's parity order. A player with no wounded machine costs a tending
/// welder nothing to check.
pub(super) struct Wounded {
    patients: Vec<Vec<Patient>>,
    banks: Vec<u32>,
}

impl Wounded {
    pub(super) fn capture(state: &State) -> Self {
        let mut patients: Vec<Vec<Patient>> = state.players.iter().map(|_| Vec::new()).collect();
        for unit in state.units.iter().filter(|u| weldable(u)) {
            patients[usize::from(unit.player.0)].push(Patient {
                id: unit.id,
                kind: unit.kind,
                pos: unit.pos,
                still: unit.still_for_weld(),
                torch: unit.order.torch(),
            });
        }
        Self {
            patients,
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

/// Sends an idle worker on a trip to the first patient around its post,
/// returning whether it went. The trip's goal is the post itself, so the
/// worker walks home once nobody near it needs a weld.
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
/// next one it finds, and otherwise walk on toward `goal`. A patrol leg
/// searches around the welder; a trip from an idle post searches around the
/// post, and gives up its patient once the owner switches idle welding off.
#[allow(clippy::too_many_arguments)]
pub(super) fn tend(
    state: &mut State,
    index: &UnitIndex,
    reach: &mut Reach,
    wounded: &Wounded,
    id: UnitId,
    goal: Goal,
    weld: Option<UnitId>,
    welds: &mut Vec<PendingFieldWeld>,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
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
    if let Some(patient) = patient {
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
    walk(state, index, reach, id, events);
}

/// Writes the welder's patient, resetting its billing meter when the patient
/// changes: the meter ramps along one patient's training clock.
fn set_weld(state: &mut State, id: UnitId, patient: Option<UnitId>) {
    let unit = state.unit_mut(id).expect("caller checked");
    if let Order::Tend { weld, .. } = &mut unit.order
        && *weld != patient
    {
        *weld = patient;
        unit.progress = 0;
    }
}

/// The patients of `welder`'s owner within reach of `center` that its bank
/// can pay to weld from `meter` on.
fn in_range<'a>(
    state: &State,
    wounded: &'a Wounded,
    welder: UnitId,
    center: Vec2Fx,
    meter: u32,
) -> impl Iterator<Item = &'a Patient> {
    let player = usize::from(state.unit(welder).expect("caller checked").player.0);
    let bank = wounded.banks[player];
    let reach_sq = ESCORT_ENGAGE_RADIUS * ESCORT_ENGAGE_RADIUS;
    wounded.patients[player].iter().filter(move |p| {
        p.id != welder
            && p.pos.dist_sq(center) <= reach_sq
            && next_paid_unit_repair_debit(p.kind, meter) <= bank
    })
}

/// Whether `welder` goes on welding `patient`: still in range and payable,
/// and standing still or on its way to weld the welder back. Two wounded
/// workers that took each other on keep their patients while both walk
/// over, rather than each letting go of the other for moving.
fn keeps(
    state: &State,
    wounded: &Wounded,
    welder: UnitId,
    patient: UnitId,
    center: Vec2Fx,
    meter: u32,
) -> bool {
    in_range(state, wounded, welder, center, meter)
        .any(|p| p.id == patient && (p.still || p.torch == Some(welder)))
}

/// The patient `welder` takes on next: of those standing still in range,
/// the first by [`patient_rank`] that a route can bring it beside. A
/// patient no route reaches is never picked, so no welder plans toward it
/// tick after tick.
fn pick(
    state: &State,
    reach: &mut Reach,
    wounded: &Wounded,
    welder: UnitId,
    center: Vec2Fx,
) -> Option<UnitId> {
    let unit = state.unit(welder).expect("caller checked");
    let mut candidates: Vec<&Unit> = in_range(state, wounded, welder, center, 0)
        .filter(|p| p.still)
        .filter_map(|p| state.unit(p.id))
        .collect();
    candidates.sort_unstable_by_key(|patient| patient_rank(unit, patient));
    candidates
        .into_iter()
        .find(|patient| {
            let tiles = patient_approaches(unit, patient).map(|(tile, _)| tile);
            reach.reaches(state, welder, tiles)
        })
        .map(|patient| patient.id)
}
