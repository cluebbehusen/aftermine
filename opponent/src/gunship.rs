//! Gunships: a cradle to assemble a carrier that soaks anti-air, flies over
//! guns and fights where it lands, wanted by the known anti-air its lifts go
//! after, by known guns its army cannot attack, or by the share of its army
//! its personality would have strike from the air. Nothing here persists
//! between decisions.

use crate::breach::{cradles, fielded, guns_shortfall};
use crate::investments::{Investment, weighed};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use crate::missions::breacher;
use crate::profile::PersonalityTraits;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, UnitKind};

/// Whether `kind` is a gunship: an assembled flyer that carries riders.
pub(crate) fn gunship(kind: UnitKind) -> bool {
    let stats = kind.stats();
    kind.assembled() && stats.domain == Domain::Air && stats.transport_capacity > 0
}

/// What a seat's situation asks of its gunships.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wants {
    /// The known anti-air around the next lift's target, by the attack margin:
    /// nothing while the seat's ground reaches an enemy.
    pub(crate) flak: u64,
    /// Per mille of the known defense an attack must bring.
    pub(crate) margin: u64,
    /// What of its army the seat's personality would have strike from the air,
    /// at its value.
    pub(crate) air_share: u64,
    /// Whether the seat knows of targets and its ground reaches none of them.
    pub(crate) severed: bool,
}

/// A cradle for each gunship once a Crucible stands and a Deep Bore stands or
/// could. It is scored by the most any of three asks: the known anti-air around
/// the next lift's target, known guns its army cannot attack, each beyond the
/// machines that answer it, and the personality's air share beyond the gunships
/// fielded. The share is a personality limit: never worth more than one
/// quarter of a gunship's price, so it waits behind what the seat needs. While
/// no machine that answers it is fielded or assembled, it is also wanted, as
/// [`cradles`] scores a mission given up, once a lift or strike at ground the
/// seat cannot reach, or an attack at ground it can, was lately given up: that
/// mission went without one. Air weighs it and income scales it.
pub(crate) fn investments(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    traits: PersonalityTraits,
    income: u32,
    wants: Wants,
) -> Vec<(Investment, u32)> {
    let crucible = observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Crucible && building.built);
    if !crucible || !bore_within_reach(observation) {
        return Vec::new();
    }
    let me = observation.me;
    let fielded_here = fielded(observation, gunship);
    let share = wants
        .air_share
        .saturating_sub(fielded_here)
        .min(u64::from(UnitKind::Roc.stats().cost) / 4);
    let shortfall = wants
        .flak
        .saturating_sub(fielded_here)
        .max(guns_shortfall(observation, map, wants.margin))
        .max(share);
    let lost = memory.given_up(observation.tick).any(|(_, anchor)| {
        let reached = map.distance(me, anchor) != UNREACHABLE;
        if wants.severed {
            !reached
        } else {
            reached && fielded(observation, breacher) == 0
        }
    });
    let base = u64::from(weighed(traits.air, income));
    cradles(observation, gunship, shortfall, lost, base)
}

/// Whether the seat holds a Deep Bore or an Extractor it could refit into one.
/// With neither, a cradle's prerequisites have no next step and saving for one
/// would hold the seat's scrap for nothing.
fn bore_within_reach(observation: &ObservationData) -> bool {
    observation.my_buildings.iter().any(|building| {
        building.kind == BuildingKind::Extractor
            && building.built
            && (building.tier > 0 || crate::expansion::supported(observation, building.anchor))
    })
}
