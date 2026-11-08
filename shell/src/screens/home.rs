//! The front door: Continue, Play, Tutorial, Saves & Replays, Settings,
//! Quit. Windowless update; every row's session verb executes in the
//! caller.

use crate::game::SoundKind;
use crate::menu::{Label, Menu};
use aftermine_protocol::{Key, RawEvent};
use macroquad::prelude::{DrawTextureParams, Vec2, WHITE, draw_text, draw_texture_ex, vec2};

/// What a Home frame decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    /// Still at the door.
    Stay,
    /// Take back a seat in a running LAN match.
    Rejoin,
    /// Resume the match left since the last finished one.
    Continue,
    /// Open the New Match wizard.
    Play,
    /// Ask for a LAN host to join.
    Join,
    /// Open Basic Training's lesson list.
    Tutorial,
    /// Open the saves and replays shelf.
    Replays,
    /// Open the codex: every machine and works, with figures.
    Roster,
    /// Open settings.
    Settings,
    /// Leave the process.
    Quit,
    /// Fetch the files found missing from the Aftermine folder again.
    BringBackMissing,
    /// Delete the files found missing from the Aftermine folder everywhere.
    DeleteMissing,
}

/// The question about files missing from the Aftermine folder.
struct MissingQuestion {
    names: Vec<String>,
    subtitle: String,
    menu: Menu<bool>,
    /// How long it has been up, in seconds. It can appear at any moment, so
    /// presses aimed at Home's rows must not land on its answers.
    age: f32,
}

/// How long the question ignores input after it appears, in seconds.
const QUESTION_GRACE: f32 = 1.0;

impl MissingQuestion {
    /// Bringing them back sits first and preselected: a mistaken press
    /// deletes nothing.
    fn new(names: Vec<String>) -> Self {
        let (subtitle, back) = if names.len() == 1 {
            (
                "1 save is missing from the Aftermine folder".to_owned(),
                "Bring it back",
            )
        } else {
            (
                format!(
                    "{} saves are missing from the Aftermine folder",
                    names.len()
                ),
                "Bring them back",
            )
        };
        Self {
            names,
            subtitle,
            menu: Menu::rows(
                "DELETE EVERYWHERE?",
                [
                    (Label::from(back), false),
                    (Label::from("Delete everywhere"), true),
                ],
            ),
            age: 0.0,
        }
    }
}

/// The Home screen: its menu and the verb behind each row.
pub struct HomeScreen {
    /// The rows, each standing for its verb, so a conditional row
    /// (Rejoin, Continue) cannot shift its neighbours onto the wrong
    /// verb.
    pub menu: Menu<Out>,
    pub(crate) catalog_ready: bool,
    /// The row offering a LAN seat back, once its host confirmed it waits.
    rejoin: Option<String>,
    missing: Option<MissingQuestion>,
}

impl HomeScreen {
    /// Builds the door, checking for a match Continue can resume.
    pub fn open() -> Self {
        Self::with_resumable(false)
    }

    pub(crate) fn set_catalog(&mut self, resumable: bool) {
        let selected = self.menu.value().copied();
        let mut fresh = Self::with_resumable(resumable).with_rejoin(self.rejoin.take());
        fresh.missing = self.missing.take();
        if let Some(selected) = selected {
            fresh.menu.select_where(|row| *row == selected);
        }
        fresh.catalog_ready = true;
        *self = fresh;
    }

    /// Offers a LAN seat back under `label`, or stops offering one.
    pub(crate) fn set_rejoin(&mut self, label: Option<String>) {
        if self.rejoin == label {
            return;
        }
        let resumable = self.resumable();
        let ready = self.catalog_ready;
        self.rejoin = label;
        self.set_catalog(resumable);
        self.catalog_ready = ready;
    }

    pub(crate) fn resumable(&self) -> bool {
        self.menu
            .lines()
            .iter()
            .any(|line| matches!(line, crate::menu::Line::Row(_, Out::Continue)))
    }

    fn with_rejoin(mut self, label: Option<String>) -> Self {
        if let Some(label) = &label {
            let mut lines = vec![crate::menu::Line::Row(
                Label::Text(label.clone()),
                Out::Rejoin,
            )];
            lines.extend_from_slice(self.menu.lines());
            self.menu = Menu::new("AFTERMINE", lines);
            self.menu.heading = crate::menu::Heading::Wordmark;
            // A host's name and a map's together outgrow the usual row.
            self.menu.wide = true;
        }
        self.rejoin = label;
        self
    }

    /// Builds the door with resumability decided by the caller (tests).
    pub fn with_resumable(resumable: bool) -> Self {
        Self::build(resumable, crate::platform::PLATFORM.desktop())
    }

    /// Quit is left out where the platform, not the app, closes apps.
    fn build(resumable: bool, quit: bool) -> Self {
        let rows = resumable
            .then_some(("Continue", Out::Continue))
            .into_iter()
            .chain([
                ("Play", Out::Play),
                ("Join match", Out::Join),
                ("Tutorial", Out::Tutorial),
                ("Saves & replays", Out::Replays),
                ("Roster", Out::Roster),
                ("Settings", Out::Settings),
            ])
            .chain(quit.then_some(("Quit", Out::Quit)))
            .map(|(label, out)| (Label::from(label), out));
        let mut menu = Menu::rows("AFTERMINE", rows);
        menu.heading = crate::menu::Heading::Wordmark;
        Self {
            menu,
            catalog_ready: false,
            rejoin: None,
            missing: None,
        }
    }

    /// Asks about `names`, the files now missing from the Aftermine folder,
    /// or stops asking once there are none.
    pub(crate) fn set_missing(&mut self, names: Vec<String>) {
        if names.is_empty() {
            self.missing = None;
        } else if self
            .missing
            .as_ref()
            .is_none_or(|asked| asked.names != names)
        {
            self.missing = Some(MissingQuestion::new(names));
        }
    }

    /// Lets `seconds` pass for the question, if it is up.
    pub(crate) fn age_question(&mut self, seconds: f32) {
        if let Some(asked) = &mut self.missing {
            asked.age += seconds;
        }
    }

    /// The files the question just answered was about.
    pub(crate) fn take_missing(&mut self) -> Vec<String> {
        self.missing
            .take()
            .map_or_else(Vec::new, |asked| asked.names)
    }

    /// The debug protocol's stable mode name.
    pub(crate) fn mode_name(&self) -> &'static str {
        if self.missing.is_some() {
            "confirm_missing"
        } else {
            "home"
        }
    }

    /// What automation reads: the question while it is up, else the rows.
    pub(crate) fn view(&self) -> crate::menu::MenuView {
        self.missing
            .as_ref()
            .map_or_else(|| self.menu.view(), |asked| asked.menu.view())
    }

    /// The standing subtitle.
    #[expect(
        clippy::unused_self,
        reason = "other screens answer this from their state"
    )]
    pub fn subtitle(&self) -> &'static str {
        "Machines eating a dead world"
    }

    /// Applies a frame's events.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        if let Some(asked) = &mut self.missing {
            if asked.age < QUESTION_GRACE {
                return Out::Stay;
            }
            let escaped = events
                .iter()
                .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }));
            let picked = asked.menu.handle(events, mouse).map(|row| row.value);
            return if escaped || picked == Some(false) {
                sounds.push((SoundKind::Click, None));
                Out::BringBackMissing
            } else if picked == Some(true) {
                sounds.push((SoundKind::Click, None));
                Out::DeleteMissing
            } else {
                Out::Stay
            };
        }
        self.fit();
        let Some(choice) = self.menu.handle(events, mouse) else {
            return Out::Stay;
        };
        sounds.push((SoundKind::Click, None));
        choice.value
    }

    /// Sizes and places the list for the window: left of center, clear of
    /// the fighting behind it.
    fn fit(&mut self) {
        self.menu.growth = growth(crate::render::viewport());
        let s = crate::render::ui_scale() * self.menu.growth;
        self.menu.shift_left(LIST_SHIFT, 32.0 * s);
    }

    /// Draws the door, with the build in the bottom left corner where a
    /// bug report can quote it, clear of the minimap behind the veil and
    /// below the footer's key help.
    pub fn draw(&mut self, sprites: &crate::assets::Sprites) {
        if let Some(asked) = &self.missing {
            asked.menu.draw(sprites, &asked.subtitle);
            return;
        }
        self.fit();
        self.menu.draw(sprites, self.subtitle());
        let ui = crate::render::ui_scale();
        let s = ui * self.menu.growth;
        let view = crate::render::viewport();
        let left = self.menu.rows_left();
        let baseline = view.y * WORDMARK_BASELINE;
        let wordmark = sprites.wordmark();
        let width = (WORDMARK_WIDTH * s).min(view.x * WORDMARK_MAX_SHARE);
        let height = width * wordmark.height() / wordmark.width();
        draw_texture_ex(
            wordmark,
            left,
            baseline - height,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(width, height)),
                ..Default::default()
            },
        );
        draw_text(
            self.subtitle(),
            left,
            baseline + 34.0 * s,
            crate::theme::Type::Body.at(s),
            crate::theme::TEXT_SECONDARY,
        );
        draw_text(
            version_label(),
            12.0 * ui,
            view.y - 8.0 * ui,
            crate::theme::Type::Small.at(ui),
            crate::theme::TEXT_SECONDARY,
        );
    }
}

/// How far left of center the list sits, as a share of the window.
const LIST_SHIFT: f32 = 0.28;
/// Where the wordmark's foot sits, as a share of the window's height.
const WORDMARK_BASELINE: f32 = 0.28;
/// The wordmark's width in logical pixels at interface scale one.
const WORDMARK_WIDTH: f32 = 620.0;
/// The most of the window's width the wordmark may take.
const WORDMARK_MAX_SHARE: f32 = 0.62;

/// How much Home grows on a window larger than the 1440 by 900 it is laid out
/// for. It takes a little over half the extra room, so it stays in proportion
/// to the match behind it without filling a large monitor.
fn growth(view: Vec2) -> f32 {
    let room = (view.x / 1440.0).min(view.y / 900.0).clamp(1.0, 2.0);
    1.0 + (room - 1.0) * 0.6
}

/// The running build as Home shows it.
fn version_label() -> &'static str {
    static LABEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LABEL.get_or_init(|| describe_build(&crate::build_identity()))
}

/// The package version and the short commit it was built from, marked
/// when the build had local changes. A source archive has no commit.
fn describe_build(build: &aftermine_kit::build::BuildIdentity) -> String {
    if build.revision == "unknown" {
        return format!("v{}", build.version);
    }
    let commit: String = build.revision.chars().take(7).collect();
    let dirty = if build.dirty == "true" { "-dirty" } else { "" };
    format!("v{} ({commit}{dirty})", build.version)
}

#[cfg(test)]
mod tests;
