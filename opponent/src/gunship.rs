//! Gunships: when the seat's ground reaches no enemy and known anti-air
//! guards what its lifts go after, a cradle to assemble a carrier that soaks
//! that fire and fights at the landing. Nothing here persists between
//! decisions.

use crate::breach::{cradles, fielded};
use crate::investments::{Investment, weighed};
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use crate::profile::PersonalityTraits;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, UnitKind};

/// Whether `kind` is a gunship: an assembled flyer that carries riders.
pub(crate) fn gunship(kind: UnitKind) -> bool {
    let stats = kind.stats();
    kind.assembled() && stats.domain == Domain::Air && stats.transport_capacity > 0
}

/// A cradle for each gunship once a Crucible stands, while the seat's ground
/// reaches no enemy. It is scored by how far the gunships the seat fields and
/// assembles, at their price, fall short of the known anti-air around the
/// next lift's target, which `flak` gives by the attack margin. While the
/// seat neither fields nor assembles one, it is also wanted, as
/// [`cradles`] scores a mission given up, once a lift or strike at ground the
/// seat cannot reach was lately given up: that lift went without one. Air
/// weighs it and income scales it.
pub(crate) fn investments(
    observation: &ObservationData,
    map: &MapModel,
    memory: &Memory,
    traits: PersonalityTraits,
    income: u32,
    flak: u64,
    severed: bool,
) -> Vec<(Investment, u32)> {
    let crucible = observation
        .my_buildings
        .iter()
        .any(|building| building.kind == BuildingKind::Crucible && building.built);
    if !severed || !crucible {
        return Vec::new();
    }
    let me = observation.me;
    let shortfall = flak.saturating_sub(fielded(observation, gunship));
    let lost = memory
        .given_up(observation.tick)
        .any(|(_, anchor)| map.distance(me, anchor) == UNREACHABLE);
    let base = u64::from(weighed(traits.air, income));
    cradles(observation, gunship, shortfall, lost, base)
}
