//! A running match's world for a client that rejoins it.

use crate::recording::WorldOrigin;
use crate::stats::LiveMatchStats;
use aftermine_sim::{Scenario, State};
use anyhow::Result;
use chassis::replay::RecordingOrigin;
use serde::{Deserialize, Serialize};

/// What a rejoining client restores: the scenario every machine plays, the
/// world at a tick boundary, and the match statistics so far. Clients run no
/// bots, so no controller memory travels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSnapshot {
    scenario: Scenario,
    origin: WorldOrigin,
    stats: LiveMatchStats,
}

impl WorldSnapshot {
    /// Captures a host's match between ticks.
    pub fn capture(scenario: &Scenario, state: &State, stats: &LiveMatchStats) -> Result<Self> {
        stats.validate_checkpoint(state)?;
        Ok(Self {
            scenario: scenario.clone(),
            origin: WorldOrigin::capture(scenario, state)?,
            stats: stats.clone(),
        })
    }

    /// Checks a snapshot that arrived over the wire.
    pub fn validate(&self) -> Result<()> {
        self.origin.validate(&self.scenario)?;
        self.stats.validate_checkpoint(self.origin.state())
    }

    /// The world it holds.
    pub fn state(&self) -> &State {
        self.origin.state()
    }

    /// The scenario, the world as a recording origin, and the statistics.
    pub fn into_parts(self) -> (Scenario, WorldOrigin, LiveMatchStats) {
        (self.scenario, self.origin, self.stats)
    }
}

#[cfg(test)]
mod tests;
