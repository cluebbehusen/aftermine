//! Who plays each seat of a match before it starts: the host, a joined
//! LAN member, a bot, or nobody yet. Pure; the screens decide when it
//! changes, and launch writes it into the scenario.

use oxide_net::{MemberId, PlayerName, RoomSeat, SeatHolder};
use oxide_sim::scenario::{BotConfig, BotDifficulty, BotStance};
use oxide_sim::{PlayerId, Scenario};

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
    /// The map's seats as its scenario sets them: the host on `host`, bot
    /// seats keeping their authored bot, and every other human seat open.
    pub(crate) fn from_scenario(scenario: &Scenario, host: PlayerId) -> Self {
        let seats = scenario
            .players
            .iter()
            .enumerate()
            .map(|(seat, player)| {
                if seat == usize::from(host.0) {
                    Occupant::Host
                } else if player.bot {
                    let bot = player.bot_config.unwrap_or_default();
                    Occupant::Bot {
                        difficulty: bot.difficulty,
                        stance: bot.stance,
                    }
                } else {
                    Occupant::Open
                }
            })
            .collect();
        Self { seats }
    }

    /// Every seat's occupant, in seat order.
    pub(crate) fn seats(&self) -> &[Occupant] {
        &self.seats
    }

    /// The host's seat.
    pub(crate) fn host_seat(&self) -> PlayerId {
        let seat = self
            .seats
            .iter()
            .position(|occupant| *occupant == Occupant::Host)
            .expect("one seat always holds the host");
        PlayerId(seat.fit::<u8>())
    }

    /// Seats `member` on the first open seat in `order`, or else in place of
    /// the first bot; `None` when every seat holds a person.
    pub(crate) fn place(&mut self, member: MemberId, order: &[usize]) -> Option<PlayerId> {
        let seat = order
            .iter()
            .copied()
            .find(|&seat| self.seats[seat] == Occupant::Open)
            .or_else(|| {
                order
                    .iter()
                    .copied()
                    .find(|&seat| matches!(self.seats[seat], Occupant::Bot { .. }))
            })?;
        self.seats[seat] = Occupant::Member(member);
        Some(PlayerId(seat.fit::<u8>()))
    }

    /// Frees `member`'s seat, which stays open; `None` when it held none.
    pub(crate) fn vacate(&mut self, member: MemberId) -> Option<PlayerId> {
        let seat = self.seat_of(member)?;
        self.seats[usize::from(seat.0)] = Occupant::Open;
        Some(seat)
    }

    /// Whether every seat has someone to play it.
    pub(crate) fn ready(&self) -> bool {
        !self.seats.contains(&Occupant::Open)
    }

    /// Where `member` sits.
    pub(crate) fn seat_of(&self, member: MemberId) -> Option<PlayerId> {
        self.seats
            .iter()
            .position(|occupant| *occupant == Occupant::Member(member))
            .map(|seat| PlayerId(seat.fit::<u8>()))
    }

    /// Every seated member and its seat, in seat order.
    pub(crate) fn members(&self) -> Vec<(MemberId, PlayerId)> {
        self.seats
            .iter()
            .enumerate()
            .filter_map(|(seat, occupant)| match occupant {
                Occupant::Member(member) => Some((*member, PlayerId(seat.fit::<u8>()))),
                _ => None,
            })
            .collect()
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

    /// Writes the seating into `scenario`: people play their seats under
    /// their names (the host keeps the seat's name without one), and bots
    /// play theirs. A seat that becomes a bot takes the personality seed
    /// `seed_base` plus its index; one that already ran the same bot keeps
    /// its own. Names then stay unique.
    pub(crate) fn apply(
        &self,
        scenario: &mut Scenario,
        host: Option<&PlayerName>,
        name_of: impl Fn(MemberId) -> Option<PlayerName>,
        seed_base: u64,
    ) {
        debug_assert_eq!(self.seats.len(), scenario.players.len());
        for (seat, (occupant, player)) in self.seats.iter().zip(&mut scenario.players).enumerate() {
            match *occupant {
                Occupant::Host | Occupant::Member(_) | Occupant::Open => {
                    player.bot = false;
                    player.bot_config = None;
                    let name = match *occupant {
                        Occupant::Host => host.cloned(),
                        Occupant::Member(member) => name_of(member),
                        Occupant::Bot { .. } | Occupant::Open => None,
                    };
                    if let Some(name) = name {
                        player.name = name.to_string();
                    }
                }
                Occupant::Bot { difficulty, stance } => {
                    let same = player.bot
                        && player.bot_config.is_some_and(|bot| {
                            bot.difficulty == difficulty && bot.stance == stance
                        });
                    if !same {
                        player.bot = true;
                        player.bot_config = Some(BotConfig::new(
                            difficulty,
                            stance,
                            seed_base.wrapping_add(seat as u64),
                        ));
                    }
                }
            }
        }
        unique_names(scenario);
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
