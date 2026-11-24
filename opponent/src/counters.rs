//! The counter chart: how each combat kind trades against each other at
//! even prices, measured in the simulation by the driver's fight lab and
//! checked in as generated data. Veteran and Prime weigh units by it instead
//! of by price: a Warden is worth more against Lancers than its scrap says,
//! a Sentinel far less.
//!
//! Support and unarmed kinds (artillery, the Mantle, spotters, workers and
//! carriers) and the cradle machines are not measured: a pure army of them
//! sells them short, so they count at their price on both sides.

use aftermine_sim::stats::UnitKind;

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

/// A fingerprint of every unit kind's stats that decide a fight. The chart
/// is stale once this differs from the fingerprint it was measured with.
#[must_use]
pub fn fingerprint() -> u64 {
    let stats: Vec<_> = UnitKind::ALL
        .iter()
        .map(|kind| {
            let stats = kind.stats();
            let weapons: Vec<_> = stats
                .weapons
                .iter()
                .map(|weapon| {
                    (
                        weapon.damage,
                        weapon.range,
                        weapon.minimum_range,
                        weapon.cooldown_ticks,
                        (weapon.targets.ground, weapon.targets.air),
                        weapon.splash,
                        weapon.indirect,
                        weapon.building_damage,
                        weapon.projectile.is_some(),
                    )
                })
                .collect();
            let defense = stats
                .point_defense
                .map(|defense| (defense.charges, defense.radius, defense.recharge_ticks));
            (
                *kind,
                (stats.cost, stats.max_hp, stats.speed, stats.radius),
                stats.domain,
                weapons,
                defense,
            )
        })
        .collect();
    chassis::hash::state_hash(&stats)
}

/// The chart as checked in: the fingerprint it was measured with, and
/// `trades[a][b]`, per mille of its own value lost, the value `MEASURED[a]`
/// destroys of `MEASURED[b]`.
#[must_use]
pub fn chart() -> (u64, [[u16; MEASURED.len()]; MEASURED.len()]) {
    (chart::STATS, chart::TRADES)
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
