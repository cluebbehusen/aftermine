//! A mission's fight, shared by attacks, lifts, strikes and clearances: it
//! goes on while it makes progress and ends once its window passes without
//! any.

use super::attack::CONTACT_TILES;
use aftermine_sim::observation::{ObservationData, UnitObs};
use aftermine_sim::{PlayerId, UnitId};
use serde::{Deserialize, Serialize};

/// A fight under way: the enemy it focuses, what it last knew of the enemy
/// around its members, and when it last made progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fight {
    pub(super) focus: Option<UnitId>,
    /// `None` until the fight's first decision.
    tally: Option<Tally>,
    /// The tick the fight began or last made progress.
    progressed: u64,
}

impl Fight {
    /// A fight beginning at `now`.
    pub(super) fn new(now: u64) -> Self {
        Self {
            focus: None,
            tally: None,
            progressed: now,
        }
    }

    /// The tick the fight began or last made progress.
    pub(super) fn progressed(self) -> u64 {
        self.progressed
    }

    /// Reads the enemy around `members` now, in a fight against `owner`, and
    /// returns whether the fight goes on: whether it made progress within
    /// the last `window` ticks. Without an owner it learns nothing new and
    /// only its time runs.
    pub(super) fn advance(
        &mut self,
        observation: &ObservationData,
        members: &[&UnitObs],
        owner: Option<PlayerId>,
        window: u64,
    ) -> bool {
        if let Some(owner) = owner {
            let now = Tally::of(observation, members, owner);
            if self.tally.is_some_and(|last| now.progresses(last)) {
                self.progressed = observation.tick;
            }
            self.tally = Some(self.tally.map_or(now, |last| now.since(last)));
        }
        observation.tick - self.progressed < window
    }

    /// Starts the fight's time again at `now`, for a fight still being won.
    pub(super) fn press(&mut self, now: u64) {
        self.progressed = now;
    }
}

/// What a fight knows of the enemy it fights: the health of the target
/// owner's known buildings at its last decision, and the least health of
/// the armed enemy units in sight around its members at any of its
/// decisions. Buildings stay where they stand, so any loss of their health
/// is progress; units step in and out of sight, so only a new low is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tally {
    buildings: u64,
    units: u64,
}

impl Tally {
    /// The enemy around `members` now, in a fight against `owner`.
    fn of(observation: &ObservationData, members: &[&UnitObs], owner: PlayerId) -> Self {
        let buildings = observation
            .enemy_buildings
            .iter()
            .filter(|building| building.player == owner)
            .map(|building| u64::from(building.hp))
            .sum();
        let units = observation
            .enemy_units
            .iter()
            .filter(|enemy| {
                enemy.kind.stats().can_fight()
                    && members
                        .iter()
                        .any(|unit| unit.tile.chebyshev(enemy.tile) <= CONTACT_TILES)
            })
            .map(|enemy| u64::from(enemy.hp))
            .sum();
        Self { buildings, units }
    }

    /// Whether the fight made progress since `last`.
    fn progresses(self, last: Self) -> bool {
        self.buildings < last.buildings || self.units < last.units
    }

    /// What the fight knows after this decision, having known `last`.
    fn since(self, last: Self) -> Self {
        Self {
            buildings: self.buildings,
            units: self.units.min(last.units),
        }
    }
}
