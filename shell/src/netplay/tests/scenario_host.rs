//! The fixed-scenario host the LAN tests drive: it seats joiners and starts
//! once every seat has someone to play it.

use super::*;
use crate::seating::{Occupant, Seating};
use anyhow::ensure;

/// A test host for a fixed scenario: its human seats open, joiners
/// take them (or a bot's, when none is open), leavers free theirs, and the
/// match starts once every seat has someone to play it.
pub(super) struct ScenarioHost {
    pub(super) lobby: HostLobby,
    pub(super) scenario: Scenario,
    pub(super) host_name: Option<PlayerName>,
    pub(super) seating: Seating,
    /// The seat order joiners fill: the setup screen's, grouped by team.
    pub(super) order: Vec<usize>,
    /// The host's match while a start runs.
    pub(super) starting: Option<Box<Game>>,
    /// A seating whose match could not be built, so it is not retried.
    pub(super) unbuildable: Option<Seating>,
    pub(super) notice: Option<String>,
}

impl ScenarioHost {
    /// Listens on `address` for the scenario's human seats other than
    /// `host`'s. `host_name`, when given, names the host's seat.
    pub(super) fn new(
        address: &str,
        scenario: Scenario,
        host: PlayerId,
        commit: &str,
        host_name: Option<PlayerName>,
    ) -> Result<Self> {
        let humans: Vec<PlayerId> = scenario
            .players
            .iter()
            .zip(0..)
            .filter(|(player, _)| !player.bot)
            .map(|(_, seat)| PlayerId(seat))
            .collect();
        ensure!(
            humans.len() >= 2 && humans.contains(&host),
            "a hosted scenario needs the host's seat and another human seat (seats with \"bot\": false)"
        );
        Ok(Self {
            lobby: HostLobby::open(address, commit, &scenario)?,
            seating: Seating::from_scenario(&scenario, host),
            order: crate::screens::wizard::seat_display_order(&scenario),
            scenario,
            host_name,
            starting: None,
            unbuildable: None,
            notice: None,
        })
    }

    pub(super) fn status(&self) -> String {
        let place = if self.starting.is_some() {
            "Starting the match...".to_owned()
        } else {
            let open = self
                .seating
                .seats()
                .iter()
                .filter(|occupant| **occupant == Occupant::Open)
                .count();
            let here = 1 + self.seating.members().len();
            format!(
                "Hosting on {}: {here} of {} players here",
                self.lobby.address(),
                here + open
            )
        };
        match &self.notice {
            Some(notice) => format!("{place}. {notice}"),
            None => place,
        }
    }

    /// One line per seat: its number and who plays it.
    pub(super) fn roster(&self) -> Vec<String> {
        roster_lines(&self.room(), None)
    }

    fn room(&self) -> Vec<RoomSeat> {
        let host = shown_host_name(
            self.host_name.as_ref(),
            &self.scenario,
            self.seating.host_seat(),
        );
        self.seating.room(
            &host,
            |member| self.lobby.name(member).cloned(),
            |seat| self.scenario.players[seat].team,
        )
    }

    pub(super) fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        for event in self.lobby.poll(now, self.starting.is_some()) {
            match event {
                LobbyEvent::Joined { member, .. } => {
                    if self.seating.place(member, &self.order).is_none() {
                        self.lobby.refuse(member);
                    }
                }
                LobbyEvent::Left { member, .. } => {
                    self.seating.vacate(member);
                }
                // The lobby closes these itself.
                LobbyEvent::Turned { .. } => {}
            }
        }
        if self.starting.is_some() {
            return self.poll_start(now);
        }
        let room = self.room();
        let seating = &self.seating;
        self.lobby
            .publish(&room, |member| seating.seat_of(member), now);
        // Joiners can take any seat but the host's, so the map's seats are
        // the lobby's room.
        let here = 1 + self.seating.members().len();
        let host = shown_host_name(
            self.host_name.as_ref(),
            &self.scenario,
            self.seating.host_seat(),
        );
        let seats = self.scenario.players.len();
        self.lobby
            .advertise(&host, &self.scenario.name, here, seats, now);
        if self.seating.ready() && self.unbuildable.as_ref() != Some(&self.seating) {
            self.start(now, viewport);
        }
        None
    }

    fn start(&mut self, now: Duration, viewport: Vec2) {
        let mut scenario = self.scenario.clone();
        let lobby = &self.lobby;
        // Every bot seat here came from the scenario and keeps its seed.
        self.seating.apply(
            &mut scenario,
            self.host_name.as_ref(),
            |member| lobby.name(member).cloned(),
            0,
        );
        let host = self.seating.host_seat();
        match Game::networked(scenario.clone(), host, NetRole::Host, viewport) {
            Ok(game) => {
                let seated = self.seating.members();
                self.lobby.start(
                    StartContent::Fresh(&scenario),
                    host,
                    &seated,
                    game.state.hash(),
                    now,
                );
                self.starting = Some(Box::new(game));
                self.notice = None;
            }
            Err(error) => {
                self.unbuildable = Some(self.seating.clone());
                eprintln!("Cannot start: {error}");
                self.notice = Some("Cannot start this match".to_owned());
            }
        }
    }

    fn poll_start(&mut self, now: Duration) -> Option<(Game, Link)> {
        match self.lobby.poll_start(now, &self.seating.members()) {
            StartPoll::Waiting => None,
            StartPoll::Started(started) => {
                let game = self.starting.take()?;
                let link = Link::host(*started, &game, now);
                Some((*game, link))
            }
            StartPoll::Failed(failed) => {
                self.starting = None;
                self.notice = Some(start_failed_notice(failed).to_owned());
                None
            }
        }
    }
}
