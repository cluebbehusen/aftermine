//! A LAN match waiting for a lost player: who, for how long, and on the
//! host the choice to drop them.

use crate::game::SoundKind;
use crate::menu::{Label, Menu, MenuView};
use macroquad::prelude::Vec2;
use oxide_protocol::{Key, RawEvent};
use oxide_sim::PlayerId;
use std::time::Duration;

/// A seat the match waits for: its player's name and how long it waited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Absent {
    pub seat: PlayerId,
    pub name: String,
    pub waited: Duration,
}

/// What the player chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    Stay,
    /// Drop the seat: it surrenders and play resumes.
    Drop(PlayerId),
    /// Open the match menu.
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Drop(PlayerId),
    Menu,
    Cancel,
    Confirm,
}

enum Face {
    Rows,
    /// Asking before dropping the seat.
    Confirm(PlayerId),
}

/// The waiting screen over a LAN match.
pub struct WaitingScreen {
    menu: Menu<Choice>,
    face: Face,
    /// Only the host may drop a seat.
    host: bool,
    absent: Vec<Absent>,
}

impl WaitingScreen {
    /// Opens on the seats the match waits for. `host` offers dropping them.
    pub fn open(absent: Vec<Absent>, host: bool) -> Self {
        let menu = rows_menu(&absent, host);
        Self {
            menu,
            face: Face::Rows,
            host,
            absent,
        }
    }

    /// Takes the latest seats the match waits for, keeping the cursor's row
    /// when the seats are unchanged. A confirmation for a seat no longer
    /// waited on closes.
    pub fn refresh(&mut self, absent: Vec<Absent>) {
        let seats = |absent: &[Absent]| -> Vec<PlayerId> {
            absent.iter().map(|absent| absent.seat).collect()
        };
        let changed = seats(&absent) != seats(&self.absent);
        self.absent = absent;
        match self.face {
            Face::Confirm(seat) if !self.absent.iter().any(|absent| absent.seat == seat) => {
                self.back_to_rows();
            }
            Face::Rows if changed => self.back_to_rows(),
            Face::Rows | Face::Confirm(_) => {}
        }
    }

    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let escaped = events
            .iter()
            .any(|event| matches!(event, RawEvent::KeyDown { key: Key::Escape }));
        let picked = self.menu.handle(events, mouse).map(|picked| picked.value);
        if picked.is_some() {
            sounds.push((SoundKind::Click, None));
        }
        match self.face {
            Face::Rows => match picked {
                Some(Choice::Drop(seat)) => {
                    self.menu = confirm_menu(&self.name(seat));
                    self.face = Face::Confirm(seat);
                    Out::Stay
                }
                Some(Choice::Menu) => Out::Menu,
                None if escaped => Out::Menu,
                _ => Out::Stay,
            },
            Face::Confirm(seat) => match picked {
                Some(Choice::Confirm) => {
                    self.back_to_rows();
                    Out::Drop(seat)
                }
                Some(_) => {
                    self.back_to_rows();
                    Out::Stay
                }
                None if escaped => {
                    self.back_to_rows();
                    Out::Stay
                }
                None => Out::Stay,
            },
        }
    }

    pub fn draw(&self) {
        self.menu.draw(&self.subtitle());
    }

    pub fn view(&self) -> MenuView {
        self.menu.view()
    }

    pub fn mode_name(&self) -> &'static str {
        match self.face {
            Face::Rows => "waiting",
            Face::Confirm(_) => "confirm_drop",
        }
    }

    /// Who the match waits for and the longest wait, or what dropping a
    /// seat does.
    fn subtitle(&self) -> String {
        match self.face {
            Face::Rows => {
                let names: Vec<&str> = self
                    .absent
                    .iter()
                    .map(|absent| absent.name.as_str())
                    .collect();
                let waited = self
                    .absent
                    .iter()
                    .map(|absent| absent.waited)
                    .max()
                    .unwrap_or_default();
                format!(
                    "Waiting for {} · {}",
                    crate::netplay::sentence(&names),
                    clock(waited)
                )
            }
            Face::Confirm(seat) => format!("{}'s seat surrenders", self.name(seat)),
        }
    }

    fn name(&self, seat: PlayerId) -> String {
        self.absent
            .iter()
            .find(|absent| absent.seat == seat)
            .map_or_else(String::new, |absent| absent.name.clone())
    }

    fn back_to_rows(&mut self) {
        self.menu = rows_menu(&self.absent, self.host);
        self.face = Face::Rows;
    }
}

fn rows_menu(absent: &[Absent], host: bool) -> Menu<Choice> {
    let drops = absent.iter().filter(|_| host).map(|absent| {
        (
            Label::from(format!("Drop {}", absent.name)),
            Choice::Drop(absent.seat),
        )
    });
    Menu::rows(
        "WAITING",
        drops.chain([(Label::from("Menu"), Choice::Menu)]),
    )
}

fn confirm_menu(name: &str) -> Menu<Choice> {
    // Cancel sits first and preselected, as every consequential choice does.
    Menu::rows(
        format!("DROP {}?", name.to_uppercase()),
        [
            (Label::from("Cancel"), Choice::Cancel),
            (Label::from(format!("Drop {name}")), Choice::Confirm),
        ],
    )
}

/// A wait as minutes and seconds.
fn clock(waited: Duration) -> String {
    let seconds = waited.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests;
