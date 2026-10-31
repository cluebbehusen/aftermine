#![doc = include_str!("../README.md")]
// Floats and hash-ordered collections would break bit-identical replays.
#![deny(clippy::float_arithmetic, clippy::disallowed_types)]

mod breach;
mod checkpoint;
mod composition;
mod decision;
mod defenses;
mod events;
mod expansion;
mod frame;
mod gunship;
mod income;
mod investments;
mod map;
mod memory;
mod missions;
mod placement;
mod profile;
mod public;
mod saving;
mod trace;
mod warheads;
mod workers;

pub use checkpoint::Checkpoint;
pub use events::{OwnEvent, OwnEvents};
pub use investments::{Investment, Step};
pub use map::MapModel;
pub use missions::{Launch, MissionKind, MissionStatus, Phase};
pub use profile::{PersonalityTraits, ResolvedProfile, Specialty};
pub use trace::{NextPurchase, Purchase, SavingTarget, Trace};

use aftermine_sim::observation::ObservationData;
use aftermine_sim::scenario::{BotConfig, BotDifficulty};
use aftermine_sim::{BuildingKind, PlayerCommand, PlayerId, State};
use std::sync::Arc;

/// One seat driven by `aftermine-opponent`.
#[derive(Debug, Clone)]
pub struct Opponent {
    player: PlayerId,
    profile: ResolvedProfile,
    map: Arc<MapModel>,
    persistent: Box<decision::Persistent>,
    /// Attacks the last decision launched; output only, never saved.
    launches: Vec<Launch>,
}

impl Opponent {
    /// Creates the controller for `player` from its scenario configuration and
    /// the match's shared map model.
    pub fn new(player: PlayerId, config: BotConfig, map: Arc<MapModel>) -> Self {
        Self {
            player,
            profile: ResolvedProfile::resolve(config),
            map,
            persistent: Box::default(),
            launches: Vec::new(),
        }
    }

    /// Scrap the seat is holding back for its saving target.
    pub fn protected_scrap(&self) -> u32 {
        self.persistent.saving.protected()
    }

    /// The seat's missions as its last decision left them.
    pub fn missions(&self) -> Vec<MissionStatus> {
        self.persistent.missions.statuses()
    }

    /// Actual units retained for ordinary local defense during quiet periods.
    pub fn guarded_units(&self) -> Vec<aftermine_sim::UnitId> {
        self.persistent.missions.guarded_units()
    }

    /// The attacks the seat's last decision launched, with what it believed
    /// of each when it did. Diagnostics for hosts: never saved, and no
    /// decision reads them.
    pub fn launches(&self) -> &[Launch] {
        &self.launches
    }

    /// The seat this controller drives.
    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// The resolved difficulty, stance and personality.
    pub fn profile(&self) -> &ResolvedProfile {
        &self.profile
    }

    /// Whether this seat decides at the state's tick. A due seat can still
    /// produce no commands.
    pub fn decision_due(&self, state: &State) -> bool {
        state.result().is_none()
            && state
                .current_tick()
                .is_multiple_of(decision_interval(self.profile.difficulty))
    }

    /// Commands for this tick. A decision consumes the seat's buffered own
    /// events; a tick without one leaves them for the next.
    pub fn act(&mut self, state: &State, events: &mut OwnEvents) -> Vec<PlayerCommand> {
        self.decide(state, events)
            .map_or_else(Vec::new, |(_, _, decision)| decision.commands)
    }

    /// Commands for this tick plus a trace of the decision that produced them.
    /// Ticks without a decision return no trace and leave `events` untouched.
    pub fn act_traced(
        &mut self,
        state: &State,
        events: &mut OwnEvents,
    ) -> (Vec<PlayerCommand>, Option<Trace>) {
        let Some((observation, events, decision)) = self.decide(state, events) else {
            return (Vec::new(), None);
        };
        let trace = Trace {
            tick: observation.tick,
            player: self.player,
            bank: observation.scrap,
            events,
            spent: decision.spent,
            purchases: decision.purchases,
            unit_orders: decision.unit_orders,
            allowance: decision.allowance,
            target: decision.target,
            protected: decision.protected,
            missions: self.missions(),
        };
        (decision.commands, Some(trace))
    }

    fn decide(
        &mut self,
        state: &State,
        events: &mut OwnEvents,
    ) -> Option<(ObservationData, Vec<OwnEvent>, decision::Decision)> {
        if !self.decision_due(state)
            || state.player(self.player).resigned
            || !state.buildings().iter().any(|building| {
                building.player == self.player
                    && building.kind == BuildingKind::Foundry
                    && building.built()
            })
        {
            return None;
        }
        let mut observation = ObservationData::fog_honest(state, self.player);
        public::merge(&mut observation);
        let events = events.take();
        let mut decision = decision::decide(
            &observation,
            &events,
            &self.map,
            &self.profile,
            &mut self.persistent,
        );
        self.launches = std::mem::take(&mut decision.launches);
        Some((observation, events, decision))
    }
}

/// Ticks between decisions: Scrapheap decides half as often as Standard and
/// Veteran, Prime twice as often. A difficulty limit on reaction time.
pub(crate) fn decision_interval(difficulty: BotDifficulty) -> u64 {
    match difficulty {
        BotDifficulty::Scrapheap => 24,
        BotDifficulty::Standard | BotDifficulty::Veteran => 12,
        BotDifficulty::Prime => 6,
    }
}

/// Per mille of its full economy a seat builds: Scrapheap half of it,
/// Standard seven tenths, Veteran most and Prime all of it. It thins the crew
/// on every scrap node and lowers how much each kind of growth is wanted,
/// never whether it is. A difficulty limit on macro competence.
pub(crate) fn economy(difficulty: BotDifficulty) -> u64 {
    match difficulty {
        BotDifficulty::Scrapheap => 500,
        BotDifficulty::Standard => 700,
        BotDifficulty::Veteran => 850,
        BotDifficulty::Prime => 1_000,
    }
}

/// Whether a seat takes the safe Extractor frames on its own half before it
/// saves for tech. Scrapheap saves for tech first. A difficulty limit on macro
/// competence.
pub(crate) fn extractors_first(difficulty: BotDifficulty) -> bool {
    difficulty != BotDifficulty::Scrapheap
}

#[cfg(test)]
mod tests;
