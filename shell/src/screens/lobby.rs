//! A LAN match before it starts: the Join form (your name and the host's
//! address), then the lobby's status and Cancel while machines gather.
//! The caller starts joins and installs the match once the lobby starts
//! it.

use crate::game::{Game, SoundKind};
use crate::menu::{Label, Line, Menu};
use crate::netplay::{Link, Lobby, address_char, parse_join_address};
use crate::text_field::{Edit, Field, Form, Keyboard};
use macroquad::prelude::Vec2;
use oxide_net::{NameError, PlayerName};
use oxide_protocol::{Key, RawEvent};
use std::time::Duration;

/// Longest host address the field accepts: the longest DNS name and a port.
const ADDRESS_MAX: usize = 253 + ":65535".len();

/// What a lobby frame decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Keep going.
    Stay,
    /// Join the host at `address`, which names a port, as `name`.
    Join { name: PlayerName, address: String },
    /// Leave for the screen underneath.
    Cancel,
}

enum Face {
    /// Asking who and where; `failure` says why the last join did not.
    Join { form: Form, failure: Option<String> },
    /// Machines gathering.
    Waiting(Lobby),
}

/// The lobby screen.
pub struct LobbyScreen {
    face: Face,
    /// The form's fields, or the roster and Cancel, for the UI report.
    pub menu: Menu<usize>,
}

fn name_check(name: &str) -> Result<(), &'static str> {
    PlayerName::new(name).map(drop).map_err(NameError::reason)
}

fn address_check(address: &str) -> Result<(), &'static str> {
    parse_join_address(address).map(drop)
}

impl LobbyScreen {
    /// The Join form, prefilled with the saved `name` and `address`;
    /// focus starts on whichever the player still has to type.
    pub fn join(name: Option<&str>, address: &str) -> Self {
        let name = name.unwrap_or_default();
        let name_field = Field::new(name, PlayerName::MAX_CHARS, PlayerName::accepts)
            .labeled("Your name", "")
            .validated(name_check);
        let address_field = Field::new(address, ADDRESS_MAX, address_char)
            .labeled("Host address", "e.g. 192.168.1.20")
            .validated(address_check)
            .keyboard(Keyboard::Address)
            .weight(2.0);
        let focus = usize::from(PlayerName::new(name).is_ok());
        let form = Form::new("JOIN MATCH", "JOIN", vec![name_field, address_field], focus);
        Self::face(Face::Join {
            form,
            failure: None,
        })
    }

    /// The Join form showing why the last join failed.
    pub fn failed(name: &str, address: &str, reason: String) -> Self {
        let mut screen = Self::join(Some(name), address);
        if let Face::Join { failure, .. } = &mut screen.face {
            *failure = Some(reason);
        }
        screen
    }

    /// Waits on `lobby`.
    pub fn waiting(lobby: Lobby) -> Self {
        Self::face(Face::Waiting(lobby))
    }

    fn face(face: Face) -> Self {
        let menu = match &face {
            Face::Join { form, .. } => form.menu(|index| index),
            Face::Waiting(lobby) => Menu::new("LAN MATCH", waiting_lines(lobby)),
        };
        Self { face, menu }
    }

    /// Polls the lobby; returns the match once it starts. A join that
    /// gives up comes back to the form, with the reason.
    pub fn poll(&mut self, now: Duration, viewport: Vec2) -> Option<(Game, Link)> {
        let Face::Waiting(lobby) = &mut self.face else {
            return None;
        };
        let started = lobby.poll(now, viewport);
        if let Some((name, address, reason)) = lobby.failure() {
            *self = Self::failed(name.as_str(), address, reason.to_owned());
        } else {
            self.menu.set_lines(waiting_lines(lobby));
        }
        started
    }

    /// The keyboard the focused field asks for, while the form is up.
    pub fn keyboard(&self) -> Option<Keyboard> {
        match &self.face {
            Face::Join { form, .. } => Some(form.keyboard()),
            Face::Waiting(_) => None,
        }
    }

    /// Consumes a request to raise the on-screen keyboard again.
    pub fn take_keyboard_request(&mut self) -> bool {
        match &mut self.face {
            Face::Join { form, .. } => form.take_keyboard_request(),
            Face::Waiting(_) => false,
        }
    }

    /// The debug protocol's stable mode name for the current face.
    pub fn mode_name(&self) -> &'static str {
        match self.face {
            Face::Join { .. } => "join_match",
            Face::Waiting(_) => "lobby",
        }
    }

    /// The line under the title: the lobby's status, or why the last join
    /// failed.
    pub fn subtitle(&self) -> String {
        match &self.face {
            Face::Join { failure, .. } => failure.clone().unwrap_or_default(),
            Face::Waiting(lobby) => lobby.status(),
        }
    }

    /// Applies a frame's events.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        match &mut self.face {
            Face::Join { form, .. } => {
                let edit = form.update(events, sounds);
                self.menu = form.menu(|index| index);
                match edit {
                    Edit::Commit => {
                        match (
                            PlayerName::new(form.value(0)),
                            parse_join_address(form.value(1)),
                        ) {
                            (Ok(name), Ok(address)) => Out::Join { name, address },
                            // The form refuses a commit either check fails.
                            _ => Out::Stay,
                        }
                    }
                    Edit::Cancel => Out::Cancel,
                    Edit::Stay => Out::Stay,
                }
            }
            Face::Waiting(_) => {
                let escaped = events
                    .iter()
                    .any(|event| matches!(event, RawEvent::KeyDown { key: Key::Escape }));
                if self.menu.handle(events, mouse).is_some() {
                    sounds.push((SoundKind::Click, None));
                    return Out::Cancel;
                }
                if escaped { Out::Cancel } else { Out::Stay }
            }
        }
    }

    /// Draws the form or the status and Cancel.
    pub fn draw(&self, mouse: Vec2) {
        match &self.face {
            Face::Join { form, failure } => {
                let color = if failure.is_some() {
                    crate::theme::TEXT_DANGER
                } else {
                    crate::theme::TEXT_SECONDARY
                };
                form.draw(&self.subtitle(), color, mouse);
            }
            Face::Waiting(_) => self.menu.draw(&self.subtitle()),
        }
    }
}

/// The waiting face's lines: the roster, which the cursor skips, then
/// Cancel.
fn waiting_lines(lobby: &Lobby) -> Vec<Line<usize>> {
    lobby
        .roster()
        .into_iter()
        .map(Line::Header)
        .chain([Line::Row(Label::from("Cancel"), 0)])
        .collect()
}

#[cfg(test)]
mod tests;
