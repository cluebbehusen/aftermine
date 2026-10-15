//! Gunships: when the seat's ground reaches no enemy and known anti-air
//! guards what its lifts go after, a cradle to assemble a carrier that soaks
//! that fire and fights at the landing. Nothing here persists between
//! decisions.

use crate::breach::{GIVEN_UP, WORTH_CAP};
use crate::investments::Investment;
use crate::map::{MapModel, UNREACHABLE};
use crate::memory::Memory;
use crate::profile::PersonalityTraits;
use oxide_sim::observation::ObservationData;
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingKind, UnitKind};

/// Whether `kind` is a gunship: an assembled flyer that carries riders.
pub(crate) fn gunship(kind: UnitKind) -> bool {
    let stats = kind.stats();
    kind.assembled() && stats.domain == Domain::Air && stats.transport_capacity > 0
}

/// A cradle for each gunship once a Crucible stands, while the seat's ground
/// reaches no enemy. It is scored by how far the gunships the seat fields and
/// assembles, at their price, fall short of the known anti-air around the
/// next lift's target, which `flak` gives by the attack margin. While the
/// seat neither fields nor assembles one, it is also wanted at [`GIVEN_UP`]
/// times the base once a lift or strike at ground the seat cannot reach was
/// lately given up: that lift went without one. Air weighs it and income
/// scales it.
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
    let held: u64 = observation
        .my_units
        .iter()
        .filter(|unit| gunship(unit.kind))
        .map(|unit| {
            let stats = unit.kind.stats();
            u64::from(stats.cost) * u64::from(unit.hp) / u64::from(stats.max_hp.max(1))
        })
        .chain(observation.my_buildings.iter().filter_map(|building| {
            building
                .kind
                .assembles()
                .filter(|machine| gunship(*machine))
                .map(|machine| u64::from(machine.stats().cost))
        }))
        .sum();
    let shortfall = flak.saturating_sub(held);
    let lost = memory
        .given_up(observation.tick)
        .any(|(_, anchor)| map.distance(me, anchor) == UNREACHABLE);
    let base = u64::from((150 + 3 * u32::from(traits.air)) * income.min(360) / 360);
    BuildingKind::ALL
        .into_iter()
        .filter_map(|cradle| cradle.assembles().map(|machine| (cradle, machine)))
        .filter(|(_, machine)| gunship(*machine))
        .filter_map(|(cradle, machine)| {
            let without = lost
                && !observation.my_units.iter().any(|unit| unit.kind == machine)
                && !observation
                    .my_buildings
                    .iter()
                    .any(|building| building.kind == cradle);
            let bar = (u64::from(machine.stats().cost) / 4).max(1);
            let guns = base * shortfall.min(WORTH_CAP * bar) / bar;
            let score = guns.max(if without { GIVEN_UP * base } else { 0 });
            (score > 0).then(|| {
                (
                    Investment::Tech(cradle),
                    u32::try_from(score).unwrap_or(u32::MAX),
                )
            })
        })
        .collect()
}
