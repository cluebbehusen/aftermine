//! Escort duty: holding a slot around an own unit, fighting threats near it,
//! and mending its party whenever it stops.

use super::super::goals::spread_goals_by;
use super::super::reach::Reach;
use super::super::spatial::UnitIndex;
use super::PendingFieldWeld;
use super::combat::acquire_escort_target;
use super::economy::{approach_patient, patient_rank};
use crate::ids::{PlayerId, UnitId};
use crate::state::{Order, PathFollow, Resume, State};
use crate::stats::{Domain, ESCORT_ENGAGE_RADIUS};
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;
use std::collections::BTreeMap;

/// One escorted unit's party as the brain phase began.
struct Party {
    /// The followed body: the escorted unit, or the transport carrying it.
    body: UnitId,
    /// Where the followed body stands.
    pos: Vec2Fx,
    /// Owner of the escorted unit.
    owner: PlayerId,
    /// The followed body stood still with no route, so welders may work.
    stopped: bool,
    /// Escorts flying, then driving, each in id order: the slot ranks.
    air: Vec<UnitId>,
    ground: Vec<UnitId>,
    /// The open tiles around the followed body, one per escort rank.
    air_slots: Vec<TilePos>,
    ground_slots: Vec<TilePos>,
}

/// Every escort party, keyed by escorted unit.
///
/// Brain-phase scratch captured from the world the phase started in.
/// Positions hold still until movement, and reading `stopped` here keeps a
/// welder's choice independent of whether the escorted unit's own brain runs
/// first in this tick's parity order. Escorts away fighting keep their rank,
/// so the others hold their slots.
pub(super) struct Parties {
    parties: BTreeMap<UnitId, Party>,
}

impl Parties {
    pub(super) fn capture(state: &State) -> Self {
        let mut members: BTreeMap<UnitId, (Vec<UnitId>, Vec<UnitId>)> = BTreeMap::new();
        for unit in &state.units {
            if let Some(escorted) = unit.order.escorted() {
                let (air, ground) = members.entry(escorted).or_default();
                match unit.kind.stats().domain {
                    Domain::Air => air.push(unit.id),
                    Domain::Ground => ground.push(unit.id),
                }
            }
        }
        let parties = members
            .into_iter()
            .filter_map(|(escorted, (air, ground))| {
                let body = state.escorted(escorted)?;
                let owner = body.player;
                let center = body.tile();
                let hull = crate::geometry::body_tiles(body.pos, body.kind.stats().radius);
                let reverse = slot_frame(state, owner);
                let slots = |domain: Domain, count: usize| {
                    if count == 0 {
                        return Vec::new();
                    }
                    spread_goals_by(center, count, reverse, |tile| {
                        !hull.clone().any(|t| t == tile) && state.passable_for(domain, tile)
                    })
                };
                Some((
                    escorted,
                    Party {
                        body: body.id,
                        pos: body.pos,
                        owner,
                        stopped: body.path.is_none() && body.drive_speed() == chassis::fx::Fx::ZERO,
                        air_slots: slots(Domain::Air, air.len()),
                        ground_slots: slots(Domain::Ground, ground.len()),
                        air,
                        ground,
                    },
                ))
            })
            .collect();
        Self { parties }
    }
}

/// The slot scan's frame for every party `owner` fields: its earliest
/// standing Foundry's side of the map, then seat parity. Fixed per owner
/// rather than per approach, so slots never reshuffle as escorts cross the
/// escorted unit's row, while mirrored seats still take mirrored slots.
fn slot_frame(state: &State, owner: PlayerId) -> bool {
    let map = (state.map.width(), state.map.height());
    state
        .buildings()
        .iter()
        .find(|b| {
            b.player == owner && !b.provisional() && b.kind == crate::stats::BuildingKind::Foundry
        })
        .map_or(owner.0 % 2 == 1, |home| {
            let (w, h) = home.kind.size();
            crate::geometry::group_spread_scan_reversed(
                home.anchor.offset(w / 2, h / 2),
                [],
                None,
                map,
                owner,
            )
        })
}

/// One tick of escort duty for `id`, escorting `escorted`.
#[allow(clippy::too_many_arguments)]
pub(super) fn accompany(
    state: &mut State,
    index: &UnitIndex,
    reach: &mut Reach,
    parties: &Parties,
    id: UnitId,
    escorted: UnitId,
    weld: Option<UnitId>,
    welds: &mut Vec<PendingFieldWeld>,
) {
    let unit = state.unit(id).expect("caller checked");
    let (player, stats) = (unit.player, unit.kind.stats());
    // Self-escort and foreign escorts are refused at dispatch and by the
    // validator; an order that names one anyway just ends.
    let Some(party) = parties
        .parties
        .get(&escorted)
        .filter(|party| escorted != id && party.owner == player)
    else {
        state
            .unit_mut(id)
            .expect("caller checked")
            .drop_active_order();
        return;
    };
    // Mounted guns pick their own fights on escort duty too.
    if stats.can_fight()
        && stats.mounts.is_none()
        && let Some(target) = acquire_escort_target(state, index, reach, id, party.pos)
    {
        let unit = state.unit_mut(id).expect("caller checked");
        unit.order = Order::Attack {
            pursue: false,
            target: target.into(),
            resume: Some(Resume::Accompany(escorted)),
        };
        unit.path = None;
        return;
    }
    if stats.harvest.is_some() && party.stopped && mend(state, party, id, escorted, weld, welds) {
        return;
    }
    if weld.is_some() {
        set_weld(state, id, None);
    }
    // A transport carrying the unit it escorts would chase a slot around
    // itself; it holds where it is until the rider is set down.
    if party.body != id {
        follow(state, reach, party, id);
    }
}

/// Writes the welder's patient, resetting its billing meter when the patient
/// changes: the meter ramps along one patient's training clock.
fn set_weld(state: &mut State, id: UnitId, patient: Option<UnitId>) {
    let unit = state.unit_mut(id).expect("caller checked");
    if let Order::Accompany { weld, .. } = &mut unit.order
        && *weld != patient
    {
        *weld = patient;
        unit.progress = 0;
    }
}

/// Mends the party's most urgent wounded ground member: the escorted unit
/// first, then the nearest other escort. Returns false when nobody needs a
/// weld or no route reaches the patient, leaving the welder to follow.
fn mend(
    state: &mut State,
    party: &Party,
    id: UnitId,
    escorted: UnitId,
    weld: Option<UnitId>,
    welds: &mut Vec<PendingFieldWeld>,
) -> bool {
    let Some(patient) = patient(state, party, id, escorted) else {
        return false;
    };
    if weld != Some(patient) {
        set_weld(state, id, Some(patient));
    }
    let (welder, wounded) = (
        state.unit(id).expect("caller checked"),
        state.unit(patient).expect("chosen patient exists"),
    );
    if welder.work_stopped() && welder.in_repair_reach(wounded) {
        state.unit_mut(id).expect("caller checked").path = None;
        welds.push(PendingFieldWeld {
            welder: id,
            patient,
        });
        return true;
    }
    approach_patient(state, id, patient)
}

/// The party member `id` should weld, if any is wounded, on the ground, not
/// retiring, and within [`ESCORT_ENGAGE_RADIUS`] of the followed body.
fn patient(state: &State, party: &Party, id: UnitId, escorted: UnitId) -> Option<UnitId> {
    let welder = state.unit(id).expect("caller checked");
    let reach_sq = ESCORT_ENGAGE_RADIUS * ESCORT_ENGAGE_RADIUS;
    let wounded = |candidate: UnitId| {
        state.unit(candidate).filter(|u| {
            u.id != id
                && u.player == welder.player
                && u.hp > 0
                && u.hp < u.kind.stats().max_hp
                && u.kind.stats().repairable()
                && u.domain() == Domain::Ground
                && !u.retiring()
                && u.pos.dist_sq(party.pos) <= reach_sq
        })
    };
    if wounded(escorted).is_some() {
        return Some(escorted);
    }
    party
        .air
        .iter()
        .chain(&party.ground)
        .filter_map(|&candidate| wounded(candidate))
        .min_by_key(|u| patient_rank(welder, u))
        .map(|u| u.id)
}

/// Heads for this escort's slot around the followed body, replanning only
/// once the slot has drifted more than a tile from the route's end.
/// Reachability is consulted only then: its labels are a whole-map pass that
/// an escort holding its slot must not pay every tick.
fn follow(state: &mut State, reach: &mut Reach, party: &Party, id: UnitId) {
    let unit = state.unit(id).expect("caller checked");
    let (ranks, slots) = match unit.kind.stats().domain {
        Domain::Air => (&party.air, &party.air_slots),
        Domain::Ground => (&party.ground, &party.ground_slots),
    };
    let rank = ranks.binary_search(&id).unwrap_or(0);
    let Some(&slot) = slots.get(rank).or(slots.last()) else {
        return;
    };
    let settled = |unit: &crate::state::Unit, tile: TilePos| match &unit.path {
        Some(path) => path.goal.chebyshev(tile) <= 1,
        None => unit.tile().chebyshev(tile) <= 1,
    };
    if settled(unit, slot) {
        return;
    }
    let Some(endpoint) = reach.endpoint(state, id, slot, None) else {
        return;
    };
    let unit = state.unit(id).expect("caller checked");
    if settled(unit, endpoint) {
        return;
    }
    let (pos, kind) = (unit.pos, unit.kind);
    let path =
        super::super::route_for_position(state, kind, pos, endpoint).map(|waypoints| PathFollow {
            final_point: None,
            goal: endpoint,
            waypoints,
            next: 0,
        });
    state.unit_mut(id).expect("caller checked").path = path;
}
