//! One bot seat's `aftermine-opponent` controller and the own order failures its
//! host has recorded for it.

use aftermine_opponent::{MapModel, Opponent, OwnEvents};
use aftermine_sim::scenario::{BotConfig, BotDifficulty, BotStance, ScenarioError};
use aftermine_sim::{PlayerCommand, PlayerId, Scenario, State, TickReport};
use serde::{Deserialize, Serialize};
use std::cell::OnceCell;
use std::sync::Arc;

/// One decision's diagnostic.
pub type SeatTrace = aftermine_opponent::Trace;

/// The map model a roster's seats share. It is built from the scenario for
/// the first seat, so a roster without one never builds it.
pub struct OpponentMap<'a> {
    scenario: &'a Scenario,
    model: OnceCell<Arc<MapModel>>,
}

impl<'a> OpponentMap<'a> {
    /// A holder for `scenario`'s model. Nothing is built yet.
    pub fn new(scenario: &'a Scenario) -> Self {
        Self {
            scenario,
            model: OnceCell::new(),
        }
    }

    fn model(&self) -> Result<Arc<MapModel>, ScenarioError> {
        if let Some(model) = self.model.get() {
            return Ok(Arc::clone(model));
        }
        let model = Arc::new(MapModel::from_scenario(self.scenario)?);
        Ok(Arc::clone(self.model.get_or_init(|| model)))
    }
}

/// A configured bot seat as the shell and driver run it, beside the own order
/// failures its host has recorded since the seat's last decision.
#[derive(Debug, Clone)]
pub struct SeatController {
    controller: Opponent,
    events: OwnEvents,
}

/// One seat's continuation: its policy state and its undelivered own events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerCheckpoint {
    /// The seat's policy state.
    pub controller: aftermine_opponent::Checkpoint,
    /// Events recorded since the seat's last decision.
    pub events: OwnEvents,
}

impl SeatController {
    /// The one place a configured seat becomes a running controller.
    pub fn configured(
        player: PlayerId,
        config: BotConfig,
        opponent_map: &OpponentMap<'_>,
    ) -> Result<Self, ScenarioError> {
        Ok(Self {
            controller: Opponent::new(player, config, opponent_map.model()?),
            events: OwnEvents::default(),
        })
    }

    /// The seat this controller drives.
    pub fn player(&self) -> PlayerId {
        self.controller.player()
    }

    /// The seat's policy.
    pub fn opponent(&self) -> &Opponent {
        &self.controller
    }

    /// Scrap the seat is holding back for a saving target.
    pub fn protected_scrap(&self) -> u32 {
        self.controller.protected_scrap()
    }

    /// Whether this seat is scheduled to decide at this tick. A due seat can
    /// still produce no commands.
    pub fn decision_due(&self, state: &State) -> bool {
        self.controller.decision_due(state)
    }

    /// Commands for this tick.
    pub fn act(&mut self, state: &State) -> Vec<PlayerCommand> {
        self.controller.act(state, &mut self.events)
    }

    /// Commands plus a decision trace, when this tick decided.
    pub fn act_traced(&mut self, state: &State) -> (Vec<PlayerCommand>, Option<SeatTrace>) {
        self.controller.act_traced(state, &mut self.events)
    }

    /// Captures the seat without running a decision.
    pub fn checkpoint(&self) -> ControllerCheckpoint {
        ControllerCheckpoint {
            controller: self.controller.checkpoint(),
            events: self.events.clone(),
        }
    }

    /// Restores a validated controller at a completed simulation boundary.
    /// `opponent_map` holds the model for the same scenario.
    pub fn restore(
        checkpoint: &ControllerCheckpoint,
        scenario: &Scenario,
        state: &State,
        opponent_map: &OpponentMap<'_>,
    ) -> Result<Self, String> {
        let map = opponent_map.model().map_err(|error| error.to_string())?;
        Opponent::restore(&checkpoint.controller, scenario, state, map).map(|controller| Self {
            controller,
            events: checkpoint.events.clone(),
        })
    }
}

/// Hands each seat its own order failures from a completed tick. Hosts call
/// this after every tick that runs with these controllers, including
/// fast-forwards whose bot commands they discard.
pub fn record_events(bots: &mut [SeatController], report: &TickReport) {
    for bot in bots {
        bot.events.record(bot.controller.player(), &report.events);
    }
}

/// Leaves `humans` as the scenario's people and seats a bot at `difficulty`
/// and `stance` in every other chair, seat `n` taking personality seed
/// `seed_base + n`, the way a New Match seats its bots.
pub fn seat_bots_around(
    scenario: &mut Scenario,
    humans: &[PlayerId],
    difficulty: BotDifficulty,
    stance: BotStance,
    seed_base: u64,
) -> anyhow::Result<()> {
    for human in humans {
        anyhow::ensure!(
            usize::from(human.0) < scenario.players.len(),
            "{} has no seat {}",
            scenario.name,
            human.0
        );
    }
    for (index, seat) in scenario.players.iter_mut().enumerate() {
        seat.bot = !humans.iter().any(|human| usize::from(human.0) == index);
        seat.bot_config = seat
            .bot
            .then(|| BotConfig::new(difficulty, stance, seed_base.wrapping_add(index as u64)));
    }
    Ok(())
}

/// Every controller a scenario configures, in seat order. A `bot` seat
/// without a config remains an empty chair.
pub fn seat_controllers(scenario: &Scenario) -> Result<Vec<SeatController>, ScenarioError> {
    let opponent_map = OpponentMap::new(scenario);
    scenario
        .players
        .iter()
        .enumerate()
        .filter(|(_, seat)| seat.bot)
        .filter_map(|(index, seat)| {
            seat.bot_config
                .map(|config| (PlayerId::from_index(index), config))
        })
        .map(|(player, config)| SeatController::configured(player, config, &opponent_map))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
