//! Who plays each seat of a match before it starts: the host, a joined
//! LAN member, a bot, or nobody yet. Pure; the screens decide when it
//! changes, and launch writes it into the scenario.

use aftermine_net::{MemberId, PlayerName, RoomSeat, SeatHolder};
use aftermine_sim::scenario::{BotDifficulty, BotStance};
use aftermine_sim::{PlayerId, Scenario};

use crate::numeric::Fit;

/// Who plays one seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Occupant {
    /// The player at this machine.
    Host,
    /// A joined LAN member.
    Member(MemberId),
    /// A bot.
    Bot {
        difficulty: BotDifficulty,
        stance: BotStance,
    },
    /// Waiting for a player.
    Open,
}

/// One occupant per seat of a map, exactly one of them the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seating {
    seats: Vec<Occupant>,
}

impl Seating {
    /// A seating of exactly these occupants, one of them the host.
    pub(crate) fn from_occupants(seats: Vec<Occupant>) -> Self {
        debug_assert_eq!(
            seats.iter().filter(|seat| **seat == Occupant::Host).count(),
            1,
            "one seat holds the host"
        );
        Self { seats }
    }

    /// Where `member` sits.
    pub(crate) fn seat_of(&self, member: MemberId) -> Option<PlayerId> {
        self.seats
            .iter()
            .position(|occupant| *occupant == Occupant::Member(member))
            .map(|seat| PlayerId(seat.fit::<u8>()))
    }

    /// The room as the lobby shows it: each seat's holder, named, and its
    /// team.
    pub(crate) fn room(
        &self,
        host: &PlayerName,
        name_of: impl Fn(MemberId) -> Option<PlayerName>,
        team_of: impl Fn(usize) -> Option<u8>,
    ) -> Vec<RoomSeat> {
        self.seats
            .iter()
            .enumerate()
            .map(|(seat, occupant)| {
                let holder = match *occupant {
                    Occupant::Host => SeatHolder::Host { name: host.clone() },
                    Occupant::Member(member) => match name_of(member) {
                        Some(name) => SeatHolder::Player { name },
                        None => SeatHolder::Open,
                    },
                    Occupant::Bot { difficulty, stance } => SeatHolder::Bot { difficulty, stance },
                    Occupant::Open => SeatHolder::Open,
                };
                RoomSeat {
                    holder,
                    team: team_of(seat),
                }
            })
            .collect()
    }
}

/// Gives each repeated seat name an ordinal (`Alex`, `Alex 2`), since the
/// victory banner, the panel, and the stats screen address seats by name.
pub(crate) fn unique_names(scenario: &mut Scenario) {
    let mut seen: Vec<String> = Vec::new();
    for player in &mut scenario.players {
        if seen.contains(&player.name) {
            let mut n = 2;
            while seen.contains(&format!("{} {n}", player.name)) {
                n += 1;
            }
            player.name = format!("{} {n}", player.name);
        }
        seen.push(player.name.clone());
    }
}

#[cfg(test)]
mod tests;
