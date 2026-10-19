//! Same-version session continuation without executing historical ticks.
//!
//! Player saves wrap this checkpoint; replay origins carry only the
//! world. Controller memory is part of continuation, not world state.

use crate::controller::{ControllerCheckpoint, SeatController};
use crate::{GameReplay, stats::LiveMatchStats};
use anyhow::{Context, Result, ensure};
use oxide_sim::{PlayerCommand, PlayerId, SIM_VERSION, Scenario, State};
use serde::{Deserialize, Serialize};

/// Session envelope revision, separate from simulation and controller revisions.
pub const VERSION: u32 = 1;
/// Encoded checkpoint load bound.
pub const MAX_BYTES: usize = 256 * 1024 * 1024;

/// World and command sources at the boundary before the next tick executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCheckpoint {
    version: u32,
    sim_version: u32,
    scenario: Scenario,
    state: State,
    snapshot_binding: u64,
    bots: Vec<ControllerCheckpoint>,
    pending: Vec<PlayerCommand>,
    stats: Option<LiveMatchStats>,
}

/// Validated parts ready for a host to install together.
pub struct RestoredSession {
    /// Authored match setup.
    pub scenario: Scenario,
    /// Exact world at the saved boundary.
    pub state: State,
    /// Controllers in canonical seat order.
    pub bots: Vec<SeatController>,
    /// Inputs that have not executed or entered the command record yet.
    pub pending: Vec<PlayerCommand>,
    /// Incremental statistics, when the host tracks them.
    pub stats: Option<LiveMatchStats>,
}

impl SessionCheckpoint {
    /// Cheap metadata for an already captured checkpoint.
    pub fn metadata(&self) -> (&str, usize, u64) {
        (
            &self.scenario.name,
            self.scenario.players.len(),
            self.state.current_tick(),
        )
    }

    /// Each seat's team tag, in seat order, so a host can count teams
    /// without restoring the session.
    pub fn team_tags(&self) -> impl Iterator<Item = Option<u8>> + '_ {
        self.scenario.players.iter().map(|player| player.team)
    }

    /// Starts a world-only recording by `build` at this session boundary.
    pub fn recording(&self, build: &crate::build::BuildIdentity) -> Result<GameReplay> {
        self.validate_world()?;
        Ok(GameReplay::with_origin(
            SIM_VERSION,
            build.label(),
            self.scenario.clone(),
            crate::recording::WorldOrigin::capture(&self.scenario, &self.state)?,
        )?)
    }

    /// Borrows a quiescent host after its tick and statistics update have finished.
    /// Does not run controllers, drain inputs, or advance the simulation.
    /// The host must supply the scenario that produced this world. The binding
    /// detects later mismatches; it does not prove the world's historical origin.
    pub fn capture(
        scenario: &Scenario,
        state: &State,
        bots: &[SeatController],
        pending: &[PlayerCommand],
        stats: Option<&LiveMatchStats>,
    ) -> Result<Self> {
        let checkpoint = Self {
            version: VERSION,
            sim_version: SIM_VERSION,
            scenario: scenario.clone(),
            state: state.clone(),
            snapshot_binding: snapshot_binding(scenario, state),
            bots: bots.iter().map(SeatController::checkpoint).collect(),
            pending: pending.to_vec(),
            stats: stats.cloned(),
        };
        checkpoint.validate_world()?;
        ensure!(
            roster(bots) == checkpoint.expected_seats(),
            "controller roster mismatch"
        );
        Ok(checkpoint)
    }

    fn expected_seats(&self) -> Vec<PlayerId> {
        self.scenario
            .players
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.bot && seat.bot_config.is_some())
            .map(|(seat, _)| PlayerId::from_index(seat))
            .collect()
    }

    fn validate_world(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported session checkpoint version {}",
            self.version
        );
        ensure!(
            self.sim_version == SIM_VERSION,
            "checkpoint simulation version mismatch"
        );
        self.state
            .validate_invariants()
            .context("checkpoint world")?;
        ensure!(
            self.snapshot_binding == snapshot_binding(&self.scenario, &self.state),
            "checkpoint scenario/world binding mismatch"
        );
        validate_setup(&self.scenario, &self.state)?;
        ensure!(
            self.pending.len() <= 65_536
                && self
                    .pending
                    .iter()
                    .all(|command| usize::from(command.player.0) < self.state.players().len()),
            "invalid pending command roster or count"
        );
        if let Some(stats) = &self.stats {
            stats.validate_checkpoint(&self.state)?;
        }
        Ok(())
    }

    /// Validates all parts before handing a usable session to the host.
    pub fn restore(self) -> Result<RestoredSession> {
        self.validate_world()?;
        ensure!(
            self.bots.len() == self.expected_seats().len(),
            "controller roster mismatch"
        );
        let opponent_map = crate::controller::OpponentMap::new(&self.scenario);
        let bots = self
            .bots
            .iter()
            .map(|checkpoint| {
                SeatController::restore(checkpoint, &self.scenario, &self.state, &opponent_map)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(anyhow::Error::msg)?;
        ensure!(
            roster(&bots) == self.expected_seats(),
            "controller seat order mismatch"
        );
        Ok(RestoredSession {
            scenario: self.scenario,
            state: self.state,
            bots,
            pending: self.pending,
            stats: self.stats,
        })
    }
}

fn roster(bots: &[SeatController]) -> Vec<PlayerId> {
    bots.iter().map(SeatController::player).collect()
}

pub(crate) fn validate_setup(scenario: &Scenario, state: &State) -> Result<()> {
    let initial = scenario.build().context("checkpoint scenario")?;
    ensure!(
        initial.mode() == state.mode(),
        "checkpoint scenario mode mismatch"
    );
    ensure!(
        initial.map().width() == state.map().width()
            && initial.map().height() == state.map().height(),
        "checkpoint map dimensions mismatch"
    );
    ensure!(
        initial.players().len() == state.players().len()
            && initial
                .players()
                .iter()
                .zip(state.players())
                .all(|(a, b)| a.team == b.team),
        "checkpoint seats mismatch"
    );
    Ok(())
}

pub(crate) fn snapshot_binding(scenario: &Scenario, state: &State) -> u64 {
    chassis::hash::state_hash(&(scenario, state.hash()))
}

#[cfg(test)]
mod tests;
