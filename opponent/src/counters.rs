//! The counter chart: how each combat kind trades against each other, and
//! against the structures a seat raises to defend, at even prices, measured
//! in the simulation by the driver's fight lab and checked in as generated
//! data. Veteran and Prime weigh units by it instead of by price: a Warden is
//! worth more against Lancers than its scrap says, a Sentinel far less.
//!
//! Support and unarmed kinds (artillery, the Mantle, spotters, workers and
//! carriers) and the cradle machines are not measured: a pure army of them
//! sells them short, so they count at their price on both sides.

use aftermine_sim::stats::{BuildingKind, Domain, UnitKind};

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

/// The structures the chart measures the kinds against, each at a rung, in
/// its column order: every armed one, upgraded or not.
pub const GUNS: [(BuildingKind, u8); 7] = [
    (BuildingKind::Turret, 0),
    (BuildingKind::Turret, 1),
    (BuildingKind::Turret, 2),
    (BuildingKind::FlakTurret, 0),
    (BuildingKind::FlakTurret, 1),
    (BuildingKind::Bastion, 0),
    (BuildingKind::Bastion, 1),
];

/// What fights: a unit of a kind, or a structure of a kind at a rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece {
    Unit(UnitKind),
    Gun(BuildingKind, u8),
}

impl From<UnitKind> for Piece {
    fn from(kind: UnitKind) -> Self {
        Piece::Unit(kind)
    }
}

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
        .chain(GUNS.iter().map(|(gun, tier)| {
            format!(
                "{gun:?} {tier} {:?} {:?}",
                gun.tier_stats(*tier),
                gun.size()
            )
        }))
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

/// The column of a `gun` at `tier` in the chart, if measured.
fn column(gun: BuildingKind, tier: u8) -> Option<usize> {
    GUNS.iter().position(|measured| *measured == (gun, tier))
}

/// Per mille of its value lost, the value `piece` destroys of `enemy`'s at
/// even prices, from the chart; a thousand when either is not measured, and
/// between two structures, which never fight. A kind the chart does not
/// measure against a structure is even, unless it can fire on the structure
/// and either the structure cannot fire on it or it is built to wreck
/// structures, hitting them harder than anything else, as a Maw is: then it
/// destroys the structure and loses nothing, as the chart reads measured
/// kinds. The structure against a kind is the inverse, and destroys none of
/// a kind it cannot fire on.
pub(crate) fn pair(piece: impl Into<Piece>, enemy: impl Into<Piece>) -> u64 {
    match (piece.into(), enemy.into()) {
        (Piece::Unit(own), Piece::Unit(other)) => match (row(own), row(other)) {
            (Some(own), Some(other)) => u64::from(chart::TRADES[own][other]),
            _ => 1_000,
        },
        (Piece::Gun(gun, tier), Piece::Unit(unit)) => {
            if column(gun, tier).is_none() {
                return 1_000;
            }
            let domain = unit.stats().domain;
            let fires = gun
                .tier_stats(tier)
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(domain));
            if fires {
                inverse(pair(unit, Piece::Gun(gun, tier)))
            } else {
                125
            }
        }
        (Piece::Unit(unit), Piece::Gun(gun, tier)) => {
            let Some(column) = column(gun, tier) else {
                return 1_000;
            };
            if let Some(own) = row(unit) {
                return u64::from(chart::AGAINST[own][column]);
            }
            let domain = unit.stats().domain;
            let fired_on = gun
                .tier_stats(tier)
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(domain));
            let fires = unit
                .stats()
                .weapons
                .iter()
                .any(|weapon| weapon.targets.covers(Domain::Ground));
            let wrecks = unit.stats().weapons.iter().any(|weapon| {
                weapon
                    .building_damage
                    .is_some_and(|damage| damage > weapon.damage)
            });
            if fires && (!fired_on || wrecks) {
                8_000
            } else {
                1_000
            }
        }
        (Piece::Gun(..), Piece::Gun(..)) => 1_000,
    }
}

/// The other side of a trade, held like the chart within an eighth and
/// eight times.
fn inverse(trade: u64) -> u64 {
    let trade = trade.max(1);
    ((1_000_000 + trade / 2) / trade).clamp(125, 8_000)
}

/// Per mille of its value lost, the value `piece` destroys of `enemies`,
/// each enemy weighed by its value; a thousand against nothing.
pub(crate) fn trade<P: Into<Piece> + Copy>(piece: impl Into<Piece>, enemies: &[(P, u64)]) -> u64 {
    let piece = piece.into();
    let total: u64 = enemies.iter().map(|(_, value)| value).sum();
    if total == 0 {
        return 1_000;
    }
    enemies
        .iter()
        .map(|(enemy, value)| pair(piece, *enemy) * value)
        .sum::<u64>()
        / total
}

/// Per mille of its price, what `piece` is worth against `enemies`: the
/// square root of its trade, so that two sides weighed this way stand in the
/// ratio their trade measured at even prices.
pub(crate) fn weight<P: Into<Piece> + Copy>(piece: impl Into<Piece>, enemies: &[(P, u64)]) -> u64 {
    (trade(piece, enemies) * 1_000).isqrt()
}

/// What `side` is worth against `against`: each of its pieces' value
/// weighed by what the piece trades against the other side.
pub(crate) fn weigh(side: &[(Piece, u64)], against: &[(Piece, u64)]) -> u64 {
    // A force lists each unit, but a trade is a mean weighed by value, so
    // pieces of a kind merge exactly: the work grows with the kinds on each
    // side, not the units.
    let against = merged(against);
    let mut weights: Vec<(Piece, u64)> = Vec::new();
    side.iter()
        .map(|(piece, value)| {
            let weight = if let Some((_, weight)) = weights.iter().find(|(seen, _)| seen == piece) {
                *weight
            } else {
                let weight = weight(*piece, &against);
                weights.push((*piece, weight));
                weight
            };
            value * weight / 1_000
        })
        .sum()
}

/// `force` with each piece once, at its total value.
fn merged(force: &[(Piece, u64)]) -> Vec<(Piece, u64)> {
    let mut merged: Vec<(Piece, u64)> = Vec::new();
    for (piece, value) in force {
        match merged.iter_mut().find(|(seen, _)| seen == piece) {
            Some((_, total)) => *total += value,
            None => merged.push((*piece, *value)),
        }
    }
    merged
}

#[cfg(test)]
mod tests;
