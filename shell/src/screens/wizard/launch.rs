//! Turning a filled-in draft into a match: the scenario it describes, the
//! bot identities it gets, and whether it starts locally or as a hosted lobby.

use super::NewMatchDraft;
use crate::game::Game;
use aftermine_net::PlayerName;
use aftermine_sim::Scenario;
use anyhow::{Context, Result};
use chassis::rng::Pcg32;
use std::time::{SystemTime, UNIX_EPOCH};

const BOT_PERSONALITY_STREAM: u64 = 0x0B07_5EED;
pub(crate) const BOT_PERSONALITY_WINDOW: u64 = 16;
const AUTOMATION_PERSONALITY_SEED: u64 = 0xA117_0A7E_0B07_5EED;

#[derive(Debug, Clone)]
pub(crate) struct PersonalitySeedSource {
    next_base: u64,
}

impl PersonalitySeedSource {
    pub(crate) fn for_session(automation: bool) -> Self {
        Self::for_session_with_entropy(automation, Self::from_entropy)
    }

    pub(crate) fn for_session_with_entropy(
        automation: bool,
        entropy: impl FnOnce() -> PersonalitySeedSource,
    ) -> Self {
        if automation {
            Self::from_seed(AUTOMATION_PERSONALITY_SEED)
        } else {
            entropy()
        }
    }

    pub(crate) fn from_entropy() -> Self {
        let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos(),
            Err(error) => error.duration().as_nanos(),
        };
        let folded_time = (nanos & 0xFFFF_FFFF_FFFF_FFFF) as u64 ^ (nanos >> 64) as u64;
        let process = u64::from(std::process::id()).rotate_left(32);
        Self::from_seed(folded_time ^ process)
    }

    pub(crate) fn from_seed(seed: u64) -> Self {
        let mut rng = Pcg32::new(seed, BOT_PERSONALITY_STREAM);
        Self {
            // Scenario validation caps a roster at 16. Alignment makes
            // `base + seat` a complete, non-wrapping 16-seed window.
            next_base: rng.next_u64() & !(BOT_PERSONALITY_WINDOW - 1),
        }
    }

    pub(crate) fn match_base(&self) -> u64 {
        self.next_base
    }

    pub(crate) fn commit_launch(&mut self) {
        self.next_base = self.next_base.wrapping_add(BOT_PERSONALITY_WINDOW);
    }
}

/// Builds the game a filled-in draft describes.
pub(crate) fn launch(draft: &NewMatchDraft, personality_seed_base: u64) -> Result<Game> {
    Game::new(draft_scenario(draft, personality_seed_base)?)
}

/// The scenario a filled-in local draft describes.
pub(crate) fn draft_scenario(
    draft: &NewMatchDraft,
    personality_seed_base: u64,
) -> Result<Scenario> {
    hosted_scenario(draft, personality_seed_base, &[])
}

/// The scenario a filled-in draft describes with `names` (by seat) for its
/// LAN players. The player's own name, when saved, names their seat.
pub(crate) fn hosted_scenario(
    draft: &NewMatchDraft,
    personality_seed_base: u64,
    names: &[Option<PlayerName>],
) -> Result<Scenario> {
    let mut scenario = (**draft.scenario.as_ref().context("draft has a map")?).clone();
    // Seats come from the per-seat plans the setup screen filled.
    // Every opponent runs with its chosen difficulty and stance. This launch base gives each chair a distinct
    // hidden identity; after this point it is ordinary scenario/replay data.
    anyhow::ensure!(
        draft.seats.len() == scenario.players.len(),
        "draft seats out of step with the map"
    );
    // Discovery lists every parseable JSON without building it, so a
    // zero-seat file can reach here — refuse it as a launch error
    // instead of underflowing the seat clamp below.
    anyhow::ensure!(!scenario.players.is_empty(), "the map has no player seats");
    let seat_choice = draft.seat_choice.min(scenario.players.len() - 1);
    for (i, player) in scenario.players.iter_mut().enumerate() {
        let plan = draft.seats[i];
        player.bot = i != seat_choice && !plan.open;
        player.bot_config = player.bot.then(|| {
            aftermine_sim::scenario::BotConfig::new(
                plan.difficulty,
                plan.stance,
                personality_seed_base.wrapping_add(i as u64),
            )
        });
    }
    // Per-seat team chips regroup seats: an FFA chip drops the seat onto its own team, and the sim densifies chosen
    // ids by first appearance at build. The scenario carries the choice,
    // so saves and replays reproduce the grouping. An all-one-team draft
    // fails the build (OneTeam) like any other launch error; the wizard
    // refuses it earlier with the reason inline.
    for (i, plan) in draft.seats.iter().enumerate() {
        scenario.players[i].team = super::team_override(plan.team_choice);
    }
    // People play under their own names; a repeated name takes an
    // ordinal, since the victory banner, the panel, and the stats screen
    // address seats by name.
    if let Some(name) = &draft.player_name {
        scenario.players[seat_choice].name = name.to_string();
    }
    for (seat, name) in names.iter().enumerate() {
        if seat != seat_choice
            && let Some(name) = name
        {
            scenario.players[seat].name = name.to_string();
        }
    }
    crate::seating::unique_names(&mut scenario);
    let mut seen: Vec<&str> = scenario.players.iter().map(|p| p.name.as_str()).collect();
    seen.sort_unstable();
    seen.dedup();
    anyhow::ensure!(
        seen.len() == scenario.players.len(),
        "seat names collide after setup"
    );
    Ok(scenario)
}

#[cfg(test)]
mod tests;
