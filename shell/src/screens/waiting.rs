//! A LAN match waiting for a lost player: who, for how long, and on the
//! host the choice to drop them or hand their seat to a bot.

use crate::bot_label::{difficulty_name, stance_name};
use crate::game::SoundKind;
use crate::menu::{Label, Menu, MenuView};
use macroquad::prelude::Vec2;
use oxide_protocol::{Key, RawEvent};
use oxide_sim::PlayerId;
use oxide_sim::scenario::{BotDifficulty, BotStance};
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
    /// Hand the seat to a bot, and play resumes.
    Replace(PlayerId, BotDifficulty, BotStance),
    /// Open the match menu.
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Drop(PlayerId),
    Replace(PlayerId),
    Menu,
    Cancel,
    Confirm,
    Difficulty(BotDifficulty),
    Stance(BotStance),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    Rows,
    /// Asking before dropping the seat.
    Confirm(PlayerId),
    /// Choosing the replacing bot's difficulty.
    Difficulty(PlayerId),
    /// Choosing its stance.
    Stance(PlayerId, BotDifficulty),
}

impl Face {
    fn seat(self) -> Option<PlayerId> {
        match self {
            Face::Rows => None,
            Face::Confirm(seat) | Face::Difficulty(seat) | Face::Stance(seat, _) => Some(seat),
        }
    }
}

/// The waiting screen over a LAN match.
pub struct WaitingScreen {
    menu: Menu<Choice>,
    face: Face,
    /// Only the host may drop or replace a seat.
    host: bool,
    absent: Vec<Absent>,
}

impl WaitingScreen {
    /// Opens on the seats the match waits for. `host` offers dropping or
    /// replacing them.
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
    /// when the seats are unchanged. A choice about a seat no longer waited
    /// on closes.
    pub fn refresh(&mut self, absent: Vec<Absent>) {
        let seats = |absent: &[Absent]| -> Vec<PlayerId> {
            absent.iter().map(|absent| absent.seat).collect()
        };
        let changed = seats(&absent) != seats(&self.absent);
        self.absent = absent;
        let gone = self
            .face
            .seat()
            .is_some_and(|seat| !self.absent.iter().any(|absent| absent.seat == seat));
        if gone || (self.face == Face::Rows && changed) {
            self.back_to_rows();
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
        if self.face == Face::Rows {
            return match picked {
                Some(Choice::Drop(seat)) => {
                    self.menu = confirm_menu(&self.name(seat));
                    self.face = Face::Confirm(seat);
                    Out::Stay
                }
                Some(Choice::Replace(seat)) => {
                    self.menu = difficulty_menu(&self.name(seat));
                    self.face = Face::Difficulty(seat);
                    Out::Stay
                }
                Some(Choice::Menu) => Out::Menu,
                None if escaped => Out::Menu,
                _ => Out::Stay,
            };
        }
        let out = match (self.face, picked) {
            (Face::Confirm(seat), Some(Choice::Confirm)) => Out::Drop(seat),
            (Face::Difficulty(seat), Some(Choice::Difficulty(difficulty))) => {
                self.menu = stance_menu(&self.name(seat));
                self.face = Face::Stance(seat, difficulty);
                return Out::Stay;
            }
            (Face::Stance(seat, difficulty), Some(Choice::Stance(stance))) => {
                Out::Replace(seat, difficulty, stance)
            }
            (_, None) if !escaped => return Out::Stay,
            _ => Out::Stay,
        };
        // A choice made, cancelled, or escaped returns to the rows.
        self.back_to_rows();
        out
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
            Face::Difficulty(_) => "replace_difficulty",
            Face::Stance(..) => "replace_stance",
        }
    }

    /// Who the match waits for and the longest wait, or what the open
    /// choice decides.
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
                    crate::clock_label::seconds(waited.as_secs())
                )
            }
            Face::Confirm(seat) => format!("{}'s seat surrenders", self.name(seat)),
            Face::Difficulty(_) => "Pick the bot's difficulty".to_owned(),
            Face::Stance(..) => "Pick the bot's stance".to_owned(),
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
    let choices = absent.iter().filter(|_| host).flat_map(|absent| {
        [
            (
                Label::from(format!("Drop {}", absent.name)),
                Choice::Drop(absent.seat),
            ),
            (
                Label::from(format!("Replace {} with a bot", absent.name)),
                Choice::Replace(absent.seat),
            ),
        ]
    });
    Menu::rows(
        "WAITING",
        choices.chain([(Label::from("Menu"), Choice::Menu)]),
    )
}

fn difficulty_menu(name: &str) -> Menu<Choice> {
    let mut menu = Menu::rows(
        format!("REPLACE {}", name.to_uppercase()),
        BotDifficulty::ALL
            .iter()
            .map(|&difficulty| {
                (
                    Label::from(difficulty_name(difficulty)),
                    Choice::Difficulty(difficulty),
                )
            })
            .chain([(Label::from("Cancel"), Choice::Cancel)]),
    );
    let default = BotDifficulty::ALL
        .iter()
        .position(|&difficulty| difficulty == BotDifficulty::default());
    menu.select(default.unwrap_or_default());
    menu
}

fn stance_menu(name: &str) -> Menu<Choice> {
    let mut menu = Menu::rows(
        format!("REPLACE {}", name.to_uppercase()),
        BotStance::ALL
            .iter()
            .map(|&stance| (Label::from(stance_name(stance)), Choice::Stance(stance)))
            .chain([(Label::from("Cancel"), Choice::Cancel)]),
    );
    let default = BotStance::ALL
        .iter()
        .position(|&stance| stance == BotStance::default());
    menu.select(default.unwrap_or_default());
    menu
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

#[cfg(test)]
mod tests;
