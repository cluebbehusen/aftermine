//! The pause menu and its confirmation dialogs — one screen object.
//! Windowless update; the main loop performs the session verbs
//! (resume, watch, settings, restart, main menu, quit) and draws.

use crate::game::SoundKind;
use crate::game::network::NetRole;
use crate::menu::{Answer, Label, Menu};
use crate::text_field::{Edit, Field, Form};
use aftermine_protocol::RawEvent;
use macroquad::prelude::Vec2;

/// The name field's coaching line, for [`crate::menu::binding_hint`]. A
/// text field keeps the classic keys, so those are what it names; a
/// player with neither keys nor a controller has nothing to name.
fn naming_hint(hands: crate::platform::Hands) -> &'static str {
    if hands.pad.is_some() {
        "{confirm} saves | {back} cancels"
    } else if hands.keys {
        "Enter saves | Esc cancels"
    } else {
        ""
    }
}

/// One pause row. The row set is conditional (Watch Replay only once
/// the match is decided), so rows are values, not indices: the confirm
/// step and the cursor return key off the row itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Back to the match.
    Resume,
    /// Write a named save while the match is still running
    /// (non-destructive; never confirms).
    SaveGame,
    /// Watch the session so far (decided matches only).
    WatchReplay,
    /// Tune settings over the paused match.
    Settings,
    /// Read the codex over the paused match.
    Roster,
    /// Concede the human's seat (confirms; mid-match only — a decided
    /// match has nothing left to give up).
    Surrender,
    /// Rebuild the match (confirms).
    Restart,
    /// Open Basic Training's lesson list over the paused lesson.
    Lessons,
    /// Abandon to the front door (confirms).
    MainMenu,
    /// Leave the process (confirms).
    Quit,
}

impl Row {
    /// The confirmation this row asks before acting, if it asks one.
    fn confirmable(self) -> Option<Confirmable> {
        match self {
            Row::Surrender => Some(Confirmable::Surrender),
            Row::Restart => Some(Confirmable::Restart),
            Row::MainMenu => Some(Confirmable::MainMenu),
            Row::Quit => Some(Confirmable::Quit),
            Row::Resume
            | Row::SaveGame
            | Row::WatchReplay
            | Row::Settings
            | Row::Roster
            | Row::Lessons => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Row::Resume => "Resume",
            Row::SaveGame => "Save match",
            Row::WatchReplay => "Watch replay",
            Row::Settings => "Settings",
            Row::Roster => "Roster",
            Row::Surrender => "Surrender",
            Row::Restart => "Restart",
            Row::Lessons => "Lessons",
            Row::MainMenu => "Main menu",
            Row::Quit => "Quit",
        }
    }
}

/// A pause verb that asks before carrying out its consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmable {
    Surrender,
    Restart,
    MainMenu,
    Quit,
}

impl Confirmable {
    /// The pause row that asked.
    fn row(self) -> Row {
        match self {
            Confirmable::Surrender => Row::Surrender,
            Confirmable::Restart => Row::Restart,
            Confirmable::MainMenu => Row::MainMenu,
            Confirmable::Quit => Row::Quit,
        }
    }

    /// What confirming does.
    fn out(self) -> Out {
        match self {
            Confirmable::Surrender => Out::Surrender,
            Confirmable::Restart => Out::Restart,
            Confirmable::MainMenu => Out::MainMenu,
            Confirmable::Quit => Out::Quit,
        }
    }
}

/// Where the local seat stands in the match under the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The match runs and the seat still gives orders.
    Playing,
    /// The match runs on without the seat: it surrendered or lost its
    /// last Foundry.
    Out,
    /// The match has a result.
    Decided,
}

/// What a row of any pause face stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A pause row.
    Row(Row),
    /// Back out of a dialog without acting.
    Cancel,
    /// Carry out the verb a confirmation dialog asks about.
    Confirm,
    /// Try a failed save again.
    Retry,
    /// Leave without the save that failed.
    LeaveUnsaved,
    /// The save-name field's one row.
    Field,
}

/// The rows the current match state offers, in display order. Watch
/// Replay belongs to decided matches (mid-match playback would be a
/// fog-free scout of the enemy); Save Game and Surrender to
/// running ones, with Surrender further limited to a seat that still
/// has a voice — a resigned or eliminated spectator is shown no verb
/// the sim would only reject. Quit is left out where the platform, not
/// the app, closes apps.
fn rows(finished: bool, can_surrender: bool, quit: bool) -> Vec<Row> {
    let mut rows = vec![Row::Resume];
    if finished {
        rows.push(Row::WatchReplay);
    } else {
        rows.push(Row::SaveGame);
    }
    rows.push(Row::Settings);
    rows.push(Row::Roster);
    if !finished && can_surrender {
        rows.push(Row::Surrender);
    }
    rows.extend([Row::Restart, Row::MainMenu]);
    if quit {
        rows.push(Row::Quit);
    }
    rows
}

/// The verb a save-failure dialog is holding open: what the player
/// was leaving toward when the autosave refused to land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveVerb {
    /// Abandon to the front door.
    MainMenu,
    /// Leave the process.
    Quit,
}

/// What a pause frame decided. Confirmed verbs only ever emerge after
/// the confirmation step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Still paused.
    Stay,
    /// Back to the match.
    Resume,
    /// Save Game was picked; the caller supplies the suggested name via
    /// [`PauseScreen::begin_naming`] (only the session knows its map
    /// and tick).
    SaveGame,
    /// The name field committed: write the save under this name. The
    /// caller reports the verdict through [`PauseScreen::end_naming`].
    Save(String),
    /// Watch the session so far.
    WatchReplay,
    /// Open Settings over the paused match; this screen waits intact.
    Settings,
    /// Open the codex over the paused match; this screen waits intact.
    Roster,
    /// Open the lesson list over the paused lesson; this screen waits
    /// intact.
    Lessons,
    /// Confirmed: concede the human's seat. The caller issues the sim
    /// command and returns to the match — the result (or the team
    /// game's concede overlay) arrives with the next tick.
    Surrender,
    /// Confirmed: rebuild the match.
    Restart,
    /// Confirmed: abandon to the front door.
    MainMenu,
    /// Confirmed: leave the process.
    Quit,
    /// Try the failed save again, then perform the verb if it lands.
    RetrySave(LeaveVerb),
    /// Perform the verb without a save — the row that guarantees a
    /// player on a permanently full disk can always leave.
    LeaveUnsaved(LeaveVerb),
}

/// Which face of the pause screen is up.
enum Face {
    /// The pause rows.
    Rows,
    /// A verb asking for confirmation.
    Confirm(Confirmable),
    /// A leave verb's autosave refused.
    SaveFailed(SaveFailed),
    /// The save-name field has focus. Only here do Text events mean
    /// anything; letters stay semantic everywhere else.
    Naming(Form),
}

/// The pause screen: its rows and whichever face is up.
pub struct PauseScreen {
    /// The live menu (pause rows, the two-row confirm dialog, the
    /// save-failure dialog, or the name field's row).
    pub menu: Menu<Choice>,
    /// The displayed rows, in menu order.
    rows: Vec<Row>,
    face: Face,
    /// A one-line verdict from the last explicit save (success or
    /// failure), shown as the subtitle until the next activation.
    notice: Option<String>,
    /// This machine's part in the LAN match under the menu, if one is (see
    /// [`PauseScreen::for_lan_match`]).
    lan: Option<NetRole>,
    /// Where the local seat stands now. A LAN match keeps running under its
    /// menu, so this can move on after the rows were built.
    standing: Standing,
    /// Whether this is the menu over a lesson (see
    /// [`PauseScreen::for_lesson`]).
    lesson: bool,
}

/// The state a save-failure dialog holds open.
struct SaveFailed {
    /// The verb waiting on the save.
    verb: LeaveVerb,
    /// The player-facing failure sentence (the dialog's subtitle).
    line: String,
}

fn pause_menu(rows: &[Row], lan: bool) -> Menu<Choice> {
    Menu::rows(
        if lan { "MENU" } else { "PAUSED" },
        rows.iter()
            .map(|row| (Label::from(row.label()), Choice::Row(*row))),
    )
}

fn confirm_menu(verb: Confirmable) -> Menu<Choice> {
    let label = verb.row().label();
    // Cancel sits first and preselected: a consequential choice takes a
    // deliberate second motion, never a double-tap.
    Menu::rows(
        format!("{}?", label.to_uppercase()),
        [
            (Label::from("Cancel"), Choice::Cancel),
            (Label::from(label), Choice::Confirm),
        ],
    )
}

impl PauseScreen {
    /// Opens on the pause rows.
    pub fn open(finished: bool, can_surrender: bool) -> Self {
        let rows = rows(finished, can_surrender, crate::platform::PLATFORM.desktop());
        Self {
            menu: pause_menu(&rows, false),
            rows,
            face: Face::Rows,
            notice: None,
            lan: None,
            standing: if finished {
                Standing::Decided
            } else if can_surrender {
                Standing::Playing
            } else {
                Standing::Out
            },
            lesson: false,
        }
    }

    /// Draws the menu while a save this screen asked for runs: the same
    /// dialog, dimmed, saying so, so leaving never flashes another screen.
    pub fn draw_saving(&self, sprites: &crate::assets::Sprites, line: &str) {
        self.menu.draw_busy(sprites, line);
    }

    /// Draws the current face: the menu, or the name field with its
    /// Save and Cancel buttons.
    pub fn draw(&self, sprites: &crate::assets::Sprites, scenario_name: &str, mouse: Vec2) {
        match &self.face {
            Face::Naming(field) => field.draw(
                sprites,
                &crate::menu::hint_line(
                    field
                        .tv_pad_hint()
                        .as_deref()
                        .unwrap_or_else(|| naming_hint(crate::platform::hands())),
                ),
                (
                    crate::hints::fade(crate::theme::TEXT_SECONDARY),
                    crate::hints::fade(macroquad::prelude::WHITE),
                ),
                mouse,
            ),
            Face::Rows | Face::Confirm(_) | Face::SaveFailed(_) => {
                self.menu.draw(sprites, self.subtitle(scenario_name));
            }
        }
    }

    /// The menu over a LAN match, which keeps running underneath: no
    /// restart, and only the host saves.
    pub fn for_lan_match(mut self, role: NetRole) -> Self {
        self.rows.retain(|row| match row {
            Row::Restart => false,
            Row::SaveGame => role == NetRole::Host,
            _ => true,
        });
        self.lan = Some(role);
        self.menu = pause_menu(&self.rows, true);
        self
    }

    /// The menu over a Basic Training lesson, which is never saved and has
    /// no opponent to concede to: no save or surrender, and a way back to
    /// the lesson list.
    pub fn for_lesson(mut self) -> Self {
        self.rows
            .retain(|row| !matches!(row, Row::SaveGame | Row::Surrender));
        let at = self
            .rows
            .iter()
            .position(|row| *row == Row::Restart)
            .map_or(self.rows.len(), |at| at + 1);
        self.rows.insert(at, Row::Lessons);
        self.lesson = true;
        self.menu = pause_menu(&self.rows, false);
        self
    }

    /// Follows the local seat as the match under the menu plays on.
    pub fn observe(&mut self, standing: Standing) {
        self.standing = standing;
    }

    /// Shows `notice` as the subtitle until the next activation.
    pub fn show_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    /// Longest save name the field accepts — what the shelf row can
    /// show without eliding.
    pub const NAME_MAX: usize = 26;

    /// Opens the name field over the pause menu, prefilled with the
    /// caller's suggestion so Enter-Enter saves without typing.
    pub fn begin_naming(&mut self, suggested: &str) {
        // Typing only ever adds printable ASCII; a prefill holds to the
        // same alphabet (plus the middle dot saves use) so a map name
        // cannot smuggle glyphs past the ingest filter.
        let name = Field::new(suggested, Self::NAME_MAX, |ch| {
            ch.is_ascii() || ch == '\u{b7}'
        });
        let form = Form::new("SAVE MATCH", "Save", vec![name], 0).starting_on_confirm();
        self.menu = form.menu(|_| Choice::Field);
        self.face = Face::Naming(form);
    }

    /// Reports the save verdict and returns to the pause rows, cursor
    /// back on Save Game, the verdict as the subtitle.
    pub fn end_naming(&mut self, notice: String) {
        self.back_to_rows(Row::SaveGame);
        self.notice = Some(notice);
    }

    /// Returns to the pause rows with the cursor on `row`.
    fn back_to_rows(&mut self, row: Row) {
        self.face = Face::Rows;
        self.menu = pause_menu(&self.rows, self.lan.is_some());
        self.menu.select_where(|choice| *choice == Choice::Row(row));
    }

    /// Opens straight onto the save-failure dialog: a leave verb's
    /// autosave refused, and exiting silently would be data loss. The
    /// safe Cancel row sits preselected; Leave without saving is always
    /// reachable, so a full disk can never trap the player in the game.
    pub fn with_save_failed(mut self, line: String, verb: LeaveVerb) -> Self {
        let mut menu = Menu::rows(
            "COULD NOT SAVE",
            [
                (Label::from("Retry"), Choice::Retry),
                (Label::from("Cancel"), Choice::Cancel),
                (Label::from("Leave without saving"), Choice::LeaveUnsaved),
            ],
        );
        menu.select(1);
        self.menu = menu;
        self.face = Face::SaveFailed(SaveFailed { verb, line });
        self
    }

    /// The confirmation dialog's subtitle: what confirming `verb` costs. A
    /// lesson is never saved, so leaving one only loses its progress. While a
    /// LAN match is still being played, a host leaving ends it for everyone
    /// and a client leaving surrenders its seat, if the seat is still in.
    fn consequence(&self, verb: Confirmable) -> &'static str {
        match verb {
            Confirmable::Surrender => "",
            Confirmable::Restart if self.lesson => "",
            Confirmable::Restart => "Progress is lost",
            Confirmable::MainMenu | Confirmable::Quit => match (self.lan, self.standing) {
                (Some(NetRole::Host), Standing::Playing | Standing::Out) => {
                    "The match ends for every player"
                }
                (Some(NetRole::Client), Standing::Playing) => "You surrender your seat",
                (Some(_), _) => "You leave the match",
                (None, _) if self.lesson => "your progress in this lesson is lost",
                (None, _) => "The match is saved first",
            },
        }
    }

    /// Whether the rows were built for a decided match.
    pub fn decided(&self) -> bool {
        self.rows.contains(&Row::WatchReplay)
    }

    /// Whether the save-failure dialog is up.
    pub fn saving_failed(&self) -> bool {
        matches!(self.face, Face::SaveFailed(_))
    }

    /// The save-name form, while it is up.
    pub fn form(&mut self) -> Option<&mut Form> {
        match &mut self.face {
            Face::Naming(form) => Some(form),
            Face::Rows | Face::Confirm(_) | Face::SaveFailed(_) => None,
        }
    }

    /// The debug protocol's stable mode name for the current face.
    pub fn mode_name(&self) -> &'static str {
        match self.face {
            Face::Rows => "pause_menu",
            Face::Confirm(_) => "confirm_pause",
            Face::SaveFailed(_) => "save_failed",
            Face::Naming(_) => "save_name",
        }
    }

    /// The subtitle for the current face of the screen.
    pub fn subtitle<'a>(&'a self, scenario_name: &'a str) -> &'a str {
        match &self.face {
            Face::SaveFailed(dialog) => &dialog.line,
            Face::Naming(_) => naming_hint(crate::platform::hands()),
            Face::Confirm(verb) => self.consequence(*verb),
            Face::Rows => self.notice.as_deref().unwrap_or(scenario_name),
        }
    }

    /// Applies a frame's events.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let escaped = crate::nav::escaped(events);
        if let Face::Naming(form) = &mut self.face {
            match form.update(events, sounds) {
                Edit::Commit => return Out::Save(form.value(0).to_owned()),
                Edit::Stay => self.menu = form.menu(|_| Choice::Field),
                Edit::Cancel => self.back_to_rows(Row::SaveGame),
            }
            return Out::Stay;
        }
        match &self.face {
            Face::SaveFailed(dialog) => {
                let verb = dialog.verb;
                match self.menu.answer(events, mouse, escaped, sounds) {
                    Some(Answer::Picked(Choice::Retry)) => return Out::RetrySave(verb),
                    Some(Answer::Picked(Choice::LeaveUnsaved)) => return Out::LeaveUnsaved(verb),
                    Some(_) => {}
                    None => return Out::Stay,
                }
                // Cancel (or Escape — never the leave verb): back to the
                // rows, cursor on the verb that raised the dialog.
                self.back_to_rows(match verb {
                    LeaveVerb::MainMenu => Row::MainMenu,
                    LeaveVerb::Quit => Row::Quit,
                });
                Out::Stay
            }
            Face::Confirm(verb) => {
                let verb = *verb;
                match self.menu.answer(events, mouse, escaped, sounds) {
                    Some(Answer::Picked(Choice::Confirm)) => verb.out(),
                    Some(_) => {
                        // The cursor returns to the armed row.
                        self.back_to_rows(verb.row());
                        Out::Stay
                    }
                    None => Out::Stay,
                }
            }
            Face::Rows | Face::Naming(_) => {
                let picked = self.menu.handle(events, mouse).map(|picked| picked.value);
                let Some(Choice::Row(row)) = picked else {
                    return if escaped { Out::Resume } else { Out::Stay };
                };
                sounds.push((SoundKind::Click, None));
                self.notice = None;
                if let Some(verb) = row.confirmable() {
                    // Surrender, Restart, Main Menu, and Quit each ask
                    // before carrying out their distinct consequence.
                    self.face = Face::Confirm(verb);
                    self.menu = confirm_menu(verb);
                    return Out::Stay;
                }
                match row {
                    Row::Resume => Out::Resume,
                    Row::SaveGame => Out::SaveGame,
                    Row::WatchReplay => Out::WatchReplay,
                    Row::Settings => Out::Settings,
                    Row::Roster => Out::Roster,
                    Row::Lessons => Out::Lessons,
                    Row::Surrender | Row::Restart | Row::MainMenu | Row::Quit => Out::Stay,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
