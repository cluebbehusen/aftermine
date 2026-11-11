//! How a decided match ended for one seat. The Results title, the stinger
//! and the score all read it, so they never disagree.

use aftermine_sim::{GameResult, PlayerId, State};

/// How a decided match ended for one seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The seat's team won, as [`State::winners`] counts it: a seat that
    /// surrendered on the way still shares its team's win.
    Victory,
    /// The seat surrendered and another team won.
    Surrendered,
    /// Another team won.
    Defeat,
    /// Every team's last Foundry fell on the same tick.
    Draw,
}

impl Verdict {
    /// How the match in `state` ended for `seat`; `None` while it runs.
    pub(crate) fn of(state: &State, seat: PlayerId) -> Option<Self> {
        let player = state.player(seat);
        Some(match state.result()? {
            GameResult::Draw => Self::Draw,
            GameResult::Victory { team } if player.team == team => Self::Victory,
            GameResult::Victory { .. } if player.resigned => Self::Surrendered,
            GameResult::Victory { .. } => Self::Defeat,
        })
    }
}

#[cfg(test)]
mod tests;
