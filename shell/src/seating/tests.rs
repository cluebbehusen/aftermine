use super::*;
use aftermine_net::Gathering;
use aftermine_sim::scenario::BotConfig;
use std::time::Duration;

const HOST: PlayerId = PlayerId(0);

/// Broad Front with the host on seat 0, seat 3 a Veteran bot, and seats 1
/// and 2 open.
fn map() -> Scenario {
    let mut scenario = Scenario::from_json(include_str!("../../../scenarios/broad-front.json"))
        .expect("the shipped map parses");
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat == 3;
        player.bot_config =
            (seat == 3).then(|| BotConfig::new(BotDifficulty::Veteran, BotStance::Aggressive, 77));
    }
    scenario
}

/// Fresh member ids, which only a lobby hands out.
fn members(count: usize) -> Vec<MemberId> {
    let mut lobby = Gathering::new("abc", &map());
    (0..count)
        .map(|_| lobby.connected(Duration::ZERO))
        .collect()
}

fn name(name: &str) -> PlayerName {
    PlayerName::new(name).unwrap()
}

#[test]
fn a_seating_starts_from_the_scenario_with_the_host_and_its_bots() {
    let seating = Seating::from_scenario(&map(), HOST);
    assert_eq!(
        seating.seats(),
        &[
            Occupant::Host,
            Occupant::Open,
            Occupant::Open,
            Occupant::Bot {
                difficulty: BotDifficulty::Veteran,
                stance: BotStance::Aggressive,
            },
        ]
    );
    assert_eq!(seating.host_seat(), HOST);
    assert!(!seating.ready());
    let elsewhere = Seating::from_scenario(&map(), PlayerId(2));
    assert_eq!(elsewhere.host_seat(), PlayerId(2));
    assert_eq!(elsewhere.seats()[0], Occupant::Open);
}

#[test]
fn joiners_fill_open_seats_in_order_then_replace_bots_then_are_refused() {
    let ids = members(4);
    let mut seating = Seating::from_scenario(&map(), HOST);
    let order = [0, 2, 1, 3];
    assert_eq!(seating.place(ids[0], &order), Some(PlayerId(2)));
    assert_eq!(seating.place(ids[1], &order), Some(PlayerId(1)));
    assert!(seating.ready());
    assert_eq!(
        seating.place(ids[2], &order),
        Some(PlayerId(3)),
        "the bot's seat"
    );
    assert_eq!(
        seating.place(ids[3], &order),
        None,
        "every seat holds a person"
    );
    assert_eq!(
        seating.members(),
        vec![
            (ids[1], PlayerId(1)),
            (ids[0], PlayerId(2)),
            (ids[2], PlayerId(3))
        ]
    );

    assert_eq!(seating.vacate(ids[0]), Some(PlayerId(2)));
    assert_eq!(
        seating.seats()[2],
        Occupant::Open,
        "a leaver's seat stays open"
    );
    assert_eq!(seating.vacate(ids[0]), None);
    assert_eq!(seating.seat_of(ids[1]), Some(PlayerId(1)));
    assert_eq!(seating.seat_of(ids[0]), None);
}

#[test]
fn the_room_names_every_seat_and_carries_its_team() {
    let ids = members(2);
    let mut seating = Seating::from_scenario(&map(), HOST);
    seating.place(ids[0], &[1]);
    seating.place(ids[1], &[2]);
    let room = seating.room(
        &name("Connor"),
        |member| (member == ids[0]).then(|| name("Alex")),
        |seat| u8::try_from(seat % 2).ok(),
    );
    let holders: Vec<&SeatHolder> = room.iter().map(|seat| &seat.holder).collect();
    assert_eq!(
        holders,
        [
            &SeatHolder::Host {
                name: name("Connor")
            },
            &SeatHolder::Player { name: name("Alex") },
            &SeatHolder::Open,
            &SeatHolder::Bot {
                difficulty: BotDifficulty::Veteran,
                stance: BotStance::Aggressive,
            },
        ],
        "a member the lobby cannot name shows as open"
    );
    let teams: Vec<Option<u8>> = room.iter().map(|seat| seat.team).collect();
    assert_eq!(teams, [Some(0), Some(1), Some(0), Some(1)]);
}

#[test]
fn applying_a_seating_writes_people_names_and_bots_into_the_scenario() {
    let ids = members(2);
    let mut seating = Seating::from_scenario(&map(), HOST);
    seating.place(ids[0], &[1, 2]);
    seating.place(ids[1], &[1, 2]);
    let mut scenario = map();
    let names = |member| {
        Some(if member == ids[0] {
            name("Alex")
        } else {
            name("Connor")
        })
    };
    seating.apply(&mut scenario, Some(&name("Connor")), names, 1000);
    let seats: Vec<(&str, bool)> = scenario
        .players
        .iter()
        .map(|player| (player.name.as_str(), player.bot))
        .collect();
    assert_eq!(
        seats,
        [
            ("Connor", false),
            ("Alex", false),
            ("Connor 2", false),
            ("South East", true)
        ],
        "a repeated name takes an ordinal"
    );
    assert_eq!(
        scenario.players[3].bot_config.unwrap().personality_seed,
        77,
        "an unchanged bot keeps its seed"
    );

    // A joiner in the bot's seat plays it; no name keeps the seat's.
    let mut seating = Seating::from_scenario(&map(), HOST);
    let order = [3];
    seating.place(ids[0], &order);
    let mut scenario = map();
    seating.apply(&mut scenario, None, |_| Some(name("Alex")), 1000);
    assert_eq!(scenario.players[0].name, "North West");
    assert!(!scenario.players[3].bot);
    assert_eq!(scenario.players[3].bot_config, None);
    assert_eq!(scenario.players[3].name, "Alex");
}

#[test]
fn a_seat_that_becomes_a_bot_takes_a_seed_from_the_base() {
    let seating = Seating::from_scenario(&map(), HOST);
    let mut scenario = map();
    scenario.players[3].bot = false;
    seating.apply(&mut scenario, None, |_| None, 1000);
    assert!(scenario.players[3].bot);
    assert_eq!(
        scenario.players[3].bot_config.unwrap().personality_seed,
        1003
    );
}

#[test]
fn unique_names_number_every_repeat() {
    let mut scenario = map();
    for (player, name) in scenario
        .players
        .iter_mut()
        .zip(["Alex", "Alex 2", "Alex", "Alex"])
    {
        player.name = name.to_owned();
    }
    unique_names(&mut scenario);
    let names: Vec<&str> = scenario
        .players
        .iter()
        .map(|player| player.name.as_str())
        .collect();
    assert_eq!(names, ["Alex", "Alex 2", "Alex 3", "Alex 4"]);
}

/// Seating the scenario host the LAN tests drive uses, which the wizard
/// does not.
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
