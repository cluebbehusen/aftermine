//! The counter chart: how each combat kind trades against each other, and
//! against the structures a seat raises to defend, at even prices, measured
//! in the simulation by the driver's fight lab and checked in as generated
//! data. Veteran and Prime weigh units by it instead of by price: a Warden is
//! worth more against Lancers than its scrap says, a Sentinel far less.
//!
//! Support and unarmed kinds (artillery, the Mantle, spotters, workers and
//! carriers) and the cradle machines are not measured: a pure army of them
//! sells them short, so they count at their price on both sides.

use aftermine_sim::stats::{BuildingKind, UnitKind};

mod chart;

/// The kinds the chart measures, in its row and column order.
pub const MEASURED: [UnitKind; 10] = [
    UnitKind::Scuttler,
    UnitKind::Sentinel,
    UnitKind::Lancer,
    UnitKind::Warden,
    UnitKind::Piledriver,
    UnitKind::Breaker,
    UnitKind::Flakhound,
    UnitKind::Buzzard,
    UnitKind::Talon,
    UnitKind::Condor,
];

/// The structures the chart measures the kinds against, at their first
/// rung, in its column order: those a short defense may raise.
pub const GUNS: [BuildingKind; 3] = [
    BuildingKind::Turret,
    BuildingKind::Bastion,
    BuildingKind::FlakTurret,
];

/// A fingerprint of every unit kind's stats and the measured structures',
/// all of them, with the structures' footprints, which set how they stand in
/// a block and how far a shot reaches them, so that no change that could
/// decide a fight slips past: weapons and their projectiles, movement,
/// turning, aiming and bracing alike. The chart is stale once this differs
/// from the fingerprint it was measured with.
#[must_use]
pub fn fingerprint() -> u64 {
    let stats: Vec<String> = UnitKind::ALL
        .iter()
        .map(|kind| format!("{kind:?} {:?}", kind.stats()))
        .chain(
            GUNS.iter()
                .map(|gun| format!("{gun:?} {:?} {:?}", gun.base_stats(), gun.size())),
        )
        .collect();
    chassis::hash::state_hash(&stats)
}

/// The chart as checked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chart {
    /// The fingerprint it was measured with.
    pub stats: u64,
    /// `trades[a][b]`, per mille of its own value lost, the value
    /// `MEASURED[a]` destroys of `MEASURED[b]`.
    pub trades: [[u16; MEASURED.len()]; MEASURED.len()],
    /// `against[a][g]`, per mille of its own value lost, the value
    /// `MEASURED[a]` destroys of `GUNS[g]`.
    pub against: [[u16; GUNS.len()]; MEASURED.len()],
}

/// The chart as checked in.
#[must_use]
pub fn chart() -> Chart {
    Chart {
        stats: chart::STATS,
        trades: chart::TRADES,
        against: chart::AGAINST,
    }
}

/// The position of `kind` in the chart, if measured.
fn row(kind: UnitKind) -> Option<usize> {
    MEASURED.iter().position(|measured| *measured == kind)
}

/// Per mille of its value lost, the value a `kind` destroys of `enemy`'s at
/// even prices, from the chart; a thousand when either is not measured.
pub(crate) fn pair(kind: UnitKind, enemy: UnitKind) -> u64 {
    match (row(kind), row(enemy)) {
        (Some(own), Some(other)) => u64::from(chart::TRADES[own][other]),
        _ => 1_000,
    }
}

/// Per mille of its value lost, the value a `gun` destroys of an
/// `attacker`'s at even prices: the inverse of what the attacker destroys of
/// it, from the chart. A gun that cannot fire on the attacker's domain
/// destroys none of it. Against a kind the chart does not measure it is
/// even, unless the kind is built to wreck structures, hitting them harder
/// than anything else, as a Maw is; a structure the chart does not measure
/// is even.
pub(crate) fn against(gun: BuildingKind, attacker: UnitKind) -> u64 {
    let Some(column) = GUNS.iter().position(|measured| *measured == gun) else {
        return 1_000;
    };
    let domain = attacker.stats().domain;
    let fires = gun
        .base_stats()
        .weapons
        .iter()
        .any(|weapon| weapon.targets.covers(domain));
    if !fires {
        return 125;
    }
    if let Some(own) = row(attacker) {
        let taken = u64::from(chart::AGAINST[own][column]).max(1);
        return ((1_000_000 + taken / 2) / taken).clamp(125, 8_000);
    }
    let wrecks = attacker.stats().weapons.iter().any(|weapon| {
        weapon
            .building_damage
            .is_some_and(|damage| damage > weapon.damage)
    });
    if wrecks { 125 } else { 1_000 }
}

/// Per mille of its value lost, the value `kind` destroys of `enemies`, each
/// enemy kind weighed by its value; a thousand against nothing.
pub(crate) fn trade(kind: UnitKind, enemies: &[(UnitKind, u64)]) -> u64 {
    let total: u64 = enemies.iter().map(|(_, value)| value).sum();
    if total == 0 {
        return 1_000;
    }
    enemies
        .iter()
        .map(|(enemy, value)| pair(kind, *enemy) * value)
        .sum::<u64>()
        / total
}

/// Per mille of its price, what a `kind` is worth against `enemies`: the
/// square root of its trade, so that two sides weighed this way stand in the
/// ratio their trade measured at even prices.
pub(crate) fn weight(kind: UnitKind, enemies: &[(UnitKind, u64)]) -> u64 {
    (trade(kind, enemies) * 1_000).isqrt()
}

#[cfg(test)]
mod tests;
