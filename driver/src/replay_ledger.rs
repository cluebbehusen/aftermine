//! `replay-ledger`: the impact ledger over recorded games, for balancing and
//! review. Each replay or match recording is re-executed through
//! [`crate::ledger::ImpactLedger`]; the text report pools seats by
//! controller.

use crate::ledger::{ImpactLedger, LedgerPool, SeatLedger, WORTH_PERIOD};
use aftermine_kit::GameReplay;
use aftermine_sim::SIM_VERSION;
use aftermine_sim::scenario::BotConfig;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Ticks per minute.
const MINUTE: u64 = aftermine_sim::TICKS_PER_SECOND as u64 * 60;

/// One seat of a recorded game.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerSeat {
    /// Player seat.
    pub seat: u8,
    /// Seat name.
    pub name: String,
    /// Team.
    pub team: u8,
    /// Who played the seat.
    pub controller: SeatPlayer,
    /// What its units and buildings did.
    pub ledger: SeatLedger,
}

/// Who played a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "player", rename_all = "snake_case")]
pub enum SeatPlayer {
    /// A person.
    Human,
    /// A bot, with its configuration.
    Bot {
        /// The configuration.
        config: BotConfig,
    },
}

/// One recorded game.
#[derive(Debug, Clone, Serialize)]
pub struct GameLedger {
    /// Where it was read from.
    pub path: PathBuf,
    /// Tick the recording begins at: zero, or later for one that starts from
    /// a saved world.
    pub start: u64,
    /// Ticks re-executed.
    pub ticks: u64,
    /// Seats, by seat.
    pub seats: Vec<LedgerSeat>,
}

/// Re-executes `replay` through the ledger.
pub fn ledger(path: &Path, replay: &GameReplay) -> Result<GameLedger> {
    replay.validate(Some(SIM_VERSION))?;
    let total = aftermine_kit::bounded_replay_duration(replay)?;
    let mut state = aftermine_kit::recording::initial_state(replay)?;
    let start = state.current_tick();
    let mut ledger = ImpactLedger::new(&state);
    let mut playback = aftermine_kit::ReplayPlayback::new(replay);
    while state.current_tick() < total {
        let report = playback.step(&mut state);
        ledger.observe(&state, &report);
    }
    let ledgers = ledger.finish(&state);
    let seats = replay
        .setup
        .players
        .iter()
        .zip(seat_players(replay))
        .zip(ledgers)
        .enumerate()
        .map(|(seat, ((spec, controller), ledger))| LedgerSeat {
            seat: u8::try_from(seat).expect("seat indices fit in u8"),
            name: spec.name.clone(),
            team: state.players()[seat].team,
            controller,
            ledger,
        })
        .collect();
    Ok(GameLedger {
        path: path.to_owned(),
        start,
        ticks: state.current_tick() - start,
        seats,
    })
}

/// Who played each seat of `replay`, by seat.
pub fn seat_players(replay: &GameReplay) -> Vec<SeatPlayer> {
    replay
        .setup
        .players
        .iter()
        .map(|spec| {
            if spec.bot {
                SeatPlayer::Bot {
                    config: spec.bot_config.unwrap_or_default(),
                }
            } else {
                SeatPlayer::Human
            }
        })
        .collect()
}

impl SeatPlayer {
    /// The label a seat pools under: its controller and, for a bot, its
    /// difficulty.
    pub fn label(self) -> String {
        match self {
            SeatPlayer::Human => "human".to_owned(),
            SeatPlayer::Bot { config } => format!("bot {}", config.difficulty),
        }
    }
}

/// The text report over `games`.
pub fn render(games: &[GameLedger]) -> String {
    let mut pools: BTreeMap<String, LedgerPool> = BTreeMap::new();
    for game in games {
        for seat in &game.seats {
            pools
                .entry(seat.controller.label())
                .or_default()
                .add(&seat.ledger);
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} game{}; value dealt is scrap of damage done, credited to whoever hit",
        games.len(),
        if games.len() == 1 { "" } else { "s" }
    );
    for (label, pool) in &pools {
        let _ = writeln!(out, "\n== {label} ({} seat-games)", pool.seats);
        pool.render(&mut out, "  ", None);
    }
    if let [game] = games {
        let _ = writeln!(out, "\n== net worth by minute (army, buildings and bank)");
        for seat in &game.seats {
            let samples: Vec<String> = seat
                .ledger
                .worth
                .iter()
                .enumerate()
                .map(|(index, worth)| (game.start + (index as u64 + 1) * WORTH_PERIOD, worth))
                .filter(|(tick, _)| tick % (5 * MINUTE) < WORTH_PERIOD)
                .map(|(tick, worth)| format!("{}m {worth}", tick / MINUTE))
                .collect();
            let _ = writeln!(
                out,
                "  seat {} {}: {}",
                seat.seat,
                seat.name,
                samples.join(", ")
            );
        }
    }
    out
}

#[cfg(test)]
mod tests;
