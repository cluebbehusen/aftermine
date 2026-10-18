//! Self-contained shell continuation; historical commands belong to recordings.

use super::*;
use oxide_kit::checkpoint::SessionCheckpoint;
use oxide_sim::scenario::{BotDifficulty, BotStance};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GameCheckpoint {
    session: SessionCheckpoint,
    /// The local seat; for a LAN match, the host's.
    human: PlayerId,
    demo: crate::tutorial::Demo,
    concede_stats: Option<oxide_kit::stats::MatchStats>,
    boundary_fog: crate::boundary_fog::BoundaryFog,
    /// Who played the other seats of a saved LAN match.
    lan: Option<LanManifest>,
}

/// The joined players of a saved LAN match: each human seat other than the
/// host's, with the installation that played it, so a resume can seat
/// each player again. Seats handed to bots are no longer listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LanManifest {
    /// In seat order.
    pub(crate) seats: Vec<(PlayerId, oxide_net::InstallId)>,
}

impl LanManifest {
    /// Lists `installs` for the human seats of `scenario` other than
    /// `host`'s. A seat that never sent one cannot be in a running match.
    pub(crate) fn new(
        scenario: &Scenario,
        host: PlayerId,
        installs: &[(PlayerId, oxide_net::InstallId)],
    ) -> Self {
        let seats = joined_seats(scenario, host)
            .filter_map(|seat| {
                installs
                    .iter()
                    .find(|(installed, _)| *installed == seat)
                    .copied()
            })
            .collect();
        Self { seats }
    }
}

/// The human seats of `scenario` other than `host`'s, in seat order.
fn joined_seats(scenario: &Scenario, host: PlayerId) -> impl Iterator<Item = PlayerId> + '_ {
    scenario
        .players
        .iter()
        .enumerate()
        .filter(|(_, player)| !player.bot)
        .map(|(seat, _)| PlayerId::from_index(seat))
        .filter(move |&seat| seat != host)
}

/// Everything a save needs, detached from the live session so the match can
/// keep ticking while a worker encodes it.
pub(crate) struct SaveCapture {
    scenario: Scenario,
    state: State,
    bots: Vec<SeatController>,
    pending: Vec<oxide_sim::PlayerCommand>,
    stats: oxide_kit::stats::LiveMatchStats,
    human: PlayerId,
    demo: crate::tutorial::Demo,
    concede_stats: Option<oxide_kit::stats::MatchStats>,
    boundary_fog: crate::boundary_fog::BoundaryFog,
    lan: Option<LanManifest>,
}

impl Game {
    pub(crate) fn capture_save(&self) -> SaveCapture {
        SaveCapture {
            scenario: self.scenario.clone(),
            state: State::clone(&self.state.0),
            bots: self.bots.clone(),
            pending: self.pending.to_vec(),
            stats: self.live_stats.clone(),
            human: self.presentation.human,
            demo: self.demo,
            concede_stats: self.concede_stats.clone(),
            boundary_fog: self.presentation.boundary_fog.clone(),
            lan: None,
        }
    }
}

impl SaveCapture {
    /// The same capture as a saved LAN match: `pending` holds every seat's
    /// unsealed orders in arrival order, and `lan` who played each joined
    /// seat.
    pub(crate) fn for_lan(
        mut self,
        pending: Vec<oxide_sim::PlayerCommand>,
        lan: LanManifest,
    ) -> Self {
        self.pending = pending;
        self.lan = Some(lan);
        self
    }

    /// Whether this is a saved LAN match.
    pub(crate) fn is_lan(&self) -> bool {
        self.lan.is_some()
    }

    pub(crate) fn checkpoint(self) -> Result<GameCheckpoint> {
        Ok(GameCheckpoint {
            session: SessionCheckpoint::capture(
                &self.scenario,
                &self.state,
                &self.bots,
                &self.pending,
                Some(&self.stats),
            )?,
            human: self.human,
            demo: self.demo,
            concede_stats: self.concede_stats,
            boundary_fog: self.boundary_fog,
            lan: self.lan,
        })
    }
}

impl Serialize for Game {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.capture_save()
            .checkpoint()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Game {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let checkpoint = GameCheckpoint::deserialize(deserializer)?;
        checkpoint
            .restore()
            .map(RestoredGame::install)
            .map_err(serde::de::Error::custom)
    }
}

/// A saved LAN match, checked, waiting in Match Setup for its players.
pub(crate) struct LanSave {
    checkpoint: GameCheckpoint,
    /// The autosave slot the save came from, which the resumed match keeps.
    slot: Option<crate::autosave::SaveSlot>,
    scenario: Scenario,
    tick: u64,
    /// The joined seats still in the match, with their installations.
    players: Vec<(PlayerId, oxide_net::InstallId)>,
}

impl LanSave {
    /// The saved match's seats.
    pub(crate) fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Keeps writing to the autosave slot the save came from.
    pub(crate) fn with_slot(mut self, slot: Option<crate::autosave::SaveSlot>) -> Self {
        self.slot = slot;
        self
    }

    /// The host's seat.
    pub(crate) fn host(&self) -> PlayerId {
        self.checkpoint.human
    }

    /// The tick the saved world is at.
    pub(crate) fn tick(&self) -> u64 {
        self.tick
    }

    /// Each joined seat whose player was still in the match, in seat
    /// order, with the installation that played it. A seat whose player
    /// was out waits for nobody.
    pub(crate) fn players(&self) -> &[(PlayerId, oxide_net::InstallId)] {
        &self.players
    }

    /// Every joined seat, the players out of the match included, with the
    /// installation that played it: what the next save must list again.
    pub(crate) fn joined(&self) -> &[(PlayerId, oxide_net::InstallId)] {
        self.checkpoint
            .lan
            .as_ref()
            .map_or(&[], |lan| lan.seats.as_slice())
    }

    /// The match as the host resumes it, with `replaced` seats handed to
    /// bots, and the unsealed orders of every seat taking part for the
    /// session to seal first. The game projects only the host's own orders,
    /// as every networked game does.
    pub(crate) fn host_game(
        &self,
        replaced: &[(PlayerId, BotDifficulty, BotStance)],
    ) -> Result<(Game, Vec<oxide_sim::PlayerCommand>)> {
        let mut game = self.with_bots(replaced)?;
        game.net = Some(network::NetRole::Host);
        game.clock.paused = false;
        let host = game.presentation.human;
        // A player out of the match takes no part, so neither do its orders.
        let taking_part = |seat: PlayerId| {
            seat == host
                || (self.players.iter().any(|&(player, _)| player == seat)
                    && !replaced.iter().any(|&(bot, ..)| bot == seat))
        };
        let pending: Vec<oxide_sim::PlayerCommand> = std::mem::take(&mut game.pending.0)
            .into_iter()
            .filter(|order| taking_part(order.player))
            .collect();
        game.pending.0 = pending
            .iter()
            .filter(|order| order.player == host)
            .cloned()
            .collect();
        Ok((game, pending))
    }

    /// The match as the host resumes it alone, once every other player's
    /// seat was handed to a bot. Players out of the match get bots too, so
    /// the host holds the only human seat, as a local match's checkpoint
    /// requires.
    pub(crate) fn local_game(
        &self,
        replaced: &[(PlayerId, BotDifficulty, BotStance)],
    ) -> Result<Game> {
        let out = self
            .joined()
            .iter()
            .map(|&(seat, _)| seat)
            .filter(|&seat| !self.players.iter().any(|&(player, _)| player == seat))
            .map(|seat| (seat, BotDifficulty::default(), BotStance::default()));
        let bots: Vec<_> = replaced.iter().copied().chain(out).collect();
        self.with_bots(&bots)
    }

    /// The saved match with `bots` seats handed to bots and their orders
    /// dropped.
    fn with_bots(&self, bots: &[(PlayerId, BotDifficulty, BotStance)]) -> Result<Game> {
        let mut game = self
            .checkpoint
            .clone()
            .restore()?
            .with_slot(self.slot.clone())
            .install();
        for &(seat, difficulty, stance) in bots {
            anyhow::ensure!(
                self.joined().iter().any(|(joined, _)| *joined == seat),
                "{seat:?} was not a joined player's seat"
            );
            game.take_over_seat(seat, difficulty, stance)?;
        }
        game.pending
            .0
            .retain(|order| !bots.iter().any(|(seat, ..)| *seat == order.player));
        Ok(game)
    }
}

pub(crate) struct RestoredGame {
    core: oxide_kit::checkpoint::RestoredSession,
    recorder: GameReplay,
    human: PlayerId,
    demo: crate::tutorial::Demo,
    concede_stats: Option<oxide_kit::stats::MatchStats>,
    boundary_fog: crate::boundary_fog::BoundaryFog,
    /// The autosave slot the save came from, which the match keeps.
    slot: Option<crate::autosave::SaveSlot>,
}

impl GameCheckpoint {
    pub(crate) fn metadata(&self) -> (&str, usize, u64) {
        self.session.metadata()
    }

    /// Each seat's team tag, in seat order.
    pub(crate) fn team_tags(&self) -> impl Iterator<Item = Option<u8>> + '_ {
        self.session.team_tags()
    }

    /// Whether this is a saved LAN match.
    pub(crate) fn is_lan(&self) -> bool {
        self.lan.is_some()
    }

    /// The saved LAN match, checked as a restore would check it.
    pub(crate) fn into_lan_save(self) -> Result<LanSave> {
        let restored = self.clone().restore()?;
        let state = &restored.core.state;
        let players = self
            .lan
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("not a LAN match"))?
            .seats
            .iter()
            .copied()
            .filter(|&(seat, _)| state.accepts_commands(seat))
            .collect();
        Ok(LanSave {
            slot: None,
            scenario: restored.scenario().clone(),
            tick: restored.tick(),
            players,
            checkpoint: self,
        })
    }

    pub(crate) fn restore(self) -> Result<RestoredGame> {
        let checkpoint = self;
        let recorder = checkpoint.session.recording(&crate::build_identity())?;
        let core = checkpoint.session.restore()?;
        match &checkpoint.lan {
            None => anyhow::ensure!(
                checkpoint.human == Game::local_seat(&core.scenario),
                "invalid local seat"
            ),
            Some(lan) => {
                anyhow::ensure!(
                    core.scenario
                        .players
                        .get(usize::from(checkpoint.human.0))
                        .is_some_and(|player| !player.bot),
                    "invalid host seat"
                );
                anyhow::ensure!(
                    lan.seats
                        .iter()
                        .map(|(seat, _)| *seat)
                        .eq(joined_seats(&core.scenario, checkpoint.human)),
                    "LAN players do not match the human seats"
                );
            }
        }
        anyhow::ensure!(
            checkpoint.boundary_fog.valid_checkpoint(&core.state),
            "invalid boundary fog"
        );
        if let Some(stats) = &checkpoint.concede_stats {
            stats.validate_checkpoint(&core.state)?;
        }
        anyhow::ensure!(
            core.stats.is_some(),
            "shell checkpoint requires live statistics"
        );
        Ok(RestoredGame {
            core,
            recorder,
            human: checkpoint.human,
            demo: checkpoint.demo,
            concede_stats: checkpoint.concede_stats,
            boundary_fog: checkpoint.boundary_fog,
            slot: None,
        })
    }
}

impl RestoredGame {
    /// Keeps writing to the autosave slot the save came from.
    pub(crate) fn with_slot(mut self, slot: Option<crate::autosave::SaveSlot>) -> Self {
        self.slot = slot;
        self
    }

    pub(crate) fn scenario(&self) -> &Scenario {
        &self.core.scenario
    }
    pub(crate) fn tick(&self) -> u64 {
        self.core.state.current_tick()
    }
    pub(crate) fn install(self) -> Game {
        let Self {
            core,
            recorder,
            human,
            demo,
            concede_stats,
            boundary_fog,
            slot,
        } = self;
        let live_stats = core.stats.expect("restoration validated statistics");
        let mut presentation = Presentation::new(&core.state, human, crate::render::viewport());
        presentation.reset_after_jump(&core.state);
        presentation.boundary_fog = boundary_fog;
        presentation.conceded_banner = concede_stats.is_some();
        let end_stats = core
            .state
            .result()
            .map(|_| live_stats.snapshot(&core.state));
        let adopted = slot.is_some();
        let mut game = Game {
            scenario: core.scenario,
            state: ReadOnlyState(Arc::new(core.state)),
            bots: core.bots,
            bot_decision: None,
            recorder,
            pending: PendingCommands(core.pending),
            live_stats,
            end_stats,
            concede_stats,
            demo,
            presentation,
            // A restored match opens paused.
            clock: super::Clock {
                paused: true,
                ..super::Clock::default()
            },
            slot: slot.unwrap_or_else(crate::autosave::SaveSlot::fresh),
            saved: None,
            crash_dumps: None,
            tick_phase: super::TickPhase::Collecting,
            suppress_presentation: false,
            net: None,
            outbox: Vec::new(),
        };
        // A continued autosave already holds this content, and a decided
        // match was recorded when it ended.
        if adopted || game.state.result().is_some() {
            game.saved = Some(game.save_key());
        }
        game.refresh_fog_lift();
        game
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod background_tests;
