//! Assembly cradles: the crew a cradle admits, and the machine that stands
//! up when the last work lands.

use crate::Event;
use crate::ids::{BuildingId, PlayerId, UnitId};
use crate::state::{Building, Order, State, Unit};
use crate::stats::{ASSEMBLY_CREW_CAP, BuildingKind};
use chassis::grid::TilePos;
use std::cmp::Reverse;

/// How a build order on a cradle lands for the workers it names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Admission {
    /// Workers the order reaches: newcomers the crew admits, workers
    /// already holding the cradle, and queued joiners judged when their
    /// turn comes.
    pub admitted: Vec<UnitId>,
    /// Holders a faster newcomer displaces.
    pub bumped: Vec<UnitId>,
    /// Newcomers the crew has no room for; their programs stay as they are.
    pub refused: Vec<UnitId>,
}

/// Whether `unit`'s current order holds the cradle of `kind` at `anchor`.
fn holds(unit: &Unit, kind: BuildingKind, anchor: TilePos, site: Option<BuildingId>) -> bool {
    match unit.order {
        Order::Build { site: id } => Some(id) == site,
        Order::Found { kind: k, anchor: a } => k == kind && a == anchor,
        _ => false,
    }
}

/// A crew rank: the faster worker first, then one already at work, then
/// the lower id. Every candidate has one owner, so id order is that
/// owner's own rank and mirrored seats choose alike.
fn rank(unit: &Unit, newcomer: bool) -> (Reverse<u32>, bool, UnitId) {
    (Reverse(unit.kind.stats().build_rate), newcomer, unit.id)
}

impl State {
    /// The workers whose current order holds `cradle`, in id order: its
    /// crew, at most [`ASSEMBLY_CREW_CAP`] of them.
    pub fn cradle_crew(&self, cradle: BuildingId) -> Vec<UnitId> {
        self.building(cradle)
            .filter(|b| b.kind.assembles().is_some())
            .map_or_else(Vec::new, |b| crew(self, b))
    }

    /// How a build order from `units` on the cradle of `kind` at `anchor`
    /// lands, for a kind that assembles an experimental. At most
    /// [`ASSEMBLY_CREW_CAP`] workers hold a cradle; a newcomer that builds
    /// strictly faster than a holder takes its place, and otherwise the
    /// crew stands as it is. A queued joiner is admitted or refused when
    /// its turn comes. `None` for any other kind.
    pub fn assembly_admission(
        &self,
        player: PlayerId,
        kind: BuildingKind,
        anchor: TilePos,
        units: &[UnitId],
        queue: bool,
    ) -> Option<Admission> {
        kind.assembles()?;
        let site = self
            .buildings
            .iter()
            .find(|b| {
                b.player == player && b.kind == kind && b.anchor == anchor && b.under_construction()
            })
            .map(|b| b.id);
        let worker = |u: &&Unit| u.player == player && u.hp > 0 && u.kind.stats().harvest.is_some();
        let named: Vec<&Unit> = units
            .iter()
            .filter_map(|&id| self.unit(id))
            .filter(worker)
            .collect();
        let mut admission = Admission::default();
        let mut ranked: Vec<(_, &Unit)> = self
            .units
            .iter()
            .filter(worker)
            .filter(|u| holds(u, kind, anchor, site))
            .map(|u| (rank(u, false), u))
            .collect();
        let mut holding = Vec::new();
        for unit in &named {
            if holds(unit, kind, anchor, site) {
                holding.push(unit.id);
            } else if queue && unit.order != Order::Idle {
                admission.admitted.push(unit.id);
            } else {
                ranked.push((rank(unit, true), unit));
            }
        }
        ranked.sort_unstable_by_key(|(key, _)| *key);
        for (slot, (key, unit)) in ranked.into_iter().enumerate() {
            let (_, newcomer, _) = key;
            match (slot < ASSEMBLY_CREW_CAP, newcomer) {
                (true, true) => admission.admitted.push(unit.id),
                (true, false) => {}
                (false, true) => admission.refused.push(unit.id),
                (false, false) => admission.bumped.push(unit.id),
            }
        }
        // A named holder keeps its order unless a faster newcomer displaced it.
        admission.admitted.extend(
            holding
                .into_iter()
                .filter(|id| !admission.bumped.contains(id)),
        );
        admission.admitted.sort_unstable();
        Some(admission)
    }
}

/// The holders of every cradle at one moment, so a later phase can tell a
/// crew's incumbents from the workers whose orders came up since.
pub(super) struct CrewSnapshot(Vec<(BuildingId, Vec<UnitId>)>);

impl CrewSnapshot {
    pub(super) fn capture(state: &State) -> Self {
        Self(
            state
                .buildings
                .iter()
                .filter(|b| b.kind.assembles().is_some() && b.under_construction())
                .map(|b| (b.id, crew(state, b)))
                .collect(),
        )
    }
}

/// Whether `unit` is one of `cradle`'s crew: an own living worker whose
/// current order holds it.
pub(crate) fn assembly_holds(unit: &Unit, cradle: &Building) -> bool {
    unit.player == cradle.player
        && unit.hp > 0
        && unit.kind.stats().harvest.is_some()
        && holds(unit, cradle.kind, cradle.anchor, Some(cradle.id))
}

/// The workers whose current order holds `cradle`, in id order.
fn crew(state: &State, cradle: &Building) -> Vec<UnitId> {
    state
        .units
        .iter()
        .filter(|u| assembly_holds(u, cradle))
        .map(|u| u.id)
        .collect()
}

/// Holds every cradle to its crew cap after a phase in which queued build
/// orders may have come up. Incumbents keep their places by the crew rank;
/// a promoted worker that does not make the crew moves on to its next
/// order and reports why, and a slower incumbent it outranks gives way.
pub(super) fn settle_crews(state: &mut State, before: &CrewSnapshot, events: &mut Vec<Event>) {
    let cradles: Vec<BuildingId> = state
        .buildings
        .iter()
        .filter(|b| b.kind.assembles().is_some() && b.under_construction())
        .map(|b| b.id)
        .collect();
    for id in cradles {
        let cradle = state.building(id).expect("collected cradle");
        let holders = crew(state, cradle);
        if holders.len() <= ASSEMBLY_CREW_CAP {
            continue;
        }
        let earlier: &[UnitId] = before
            .0
            .iter()
            .find(|(c, _)| *c == id)
            .map_or(&[], |(_, crew)| crew.as_slice());
        let mut ranked: Vec<_> = holders
            .iter()
            .map(|&u| {
                let unit = state.unit(u).expect("holder lives");
                (rank(unit, !earlier.contains(&u)), u)
            })
            .collect();
        ranked.sort_unstable_by_key(|(key, _)| *key);
        let (player, kind, anchor) = (cradle.player, cradle.kind, cradle.anchor);
        for (key, unit) in ranked.into_iter().skip(ASSEMBLY_CREW_CAP) {
            let (_, newcomer, _) = key;
            let pos = state.unit(unit).expect("holder lives").pos;
            super::commands::release_site_orders(state, unit, id, kind, anchor);
            if newcomer {
                events.push(Event::OrderStalled {
                    unit,
                    player,
                    pos,
                    reason: crate::StallReason::CrewFull,
                });
            }
        }
    }
}

/// Turns every finished cradle into its machine: the cradle leaves the
/// world, every seat forgets it, its crew moves on, and the experimental
/// stands at its centre with the cradle's share of health.
pub(super) fn stand_up(state: &mut State, assembled: &[BuildingId], events: &mut Vec<Event>) {
    for &id in assembled {
        let Some(index) = state.buildings.iter().position(|b| b.id == id) else {
            continue;
        };
        let cradle = &state.buildings[index];
        let Some(kind) = cradle.kind.assembles().filter(|_| cradle.hp > 0) else {
            continue;
        };
        let (player, building_kind, anchor) = (cradle.player, cradle.kind, cradle.anchor);
        let max = u64::from(cradle.stats().max_hp);
        let share = u64::from(cradle.hp);
        let pos = crate::geometry::footprint_center(anchor, building_kind.size());
        state.stamp_building_occupancy(index, false);
        crate::vision::forget_public_building(state, player, building_kind, anchor);
        crate::vision::forget_observed_building(state, id, false);
        super::commands::clear_site_orders(state, player, id);
        state.buildings.retain(|b| b.id != id);
        let unit = state.spawn_unit(player, kind, pos);
        let hp = u64::from(kind.stats().max_hp) * share / max.max(1);
        state.unit_mut(unit).expect("just spawned").hp =
            u32::try_from(hp.max(1)).expect("machine hp fits u32");
        events.push(Event::UnitAssembled {
            cradle: id,
            unit,
            kind,
            player,
            pos,
        });
    }
}
