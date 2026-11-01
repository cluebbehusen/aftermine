//! The front door: Continue, Play, Tutorial, Saves & Replays, Settings,
//! Quit. Windowless update; every row's session verb executes in the
//! caller.

use crate::game::SoundKind;
use crate::menu::{Label, Menu};
use aftermine_protocol::RawEvent;
use macroquad::prelude::{Vec2, draw_text};

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
}

impl HomeScreen {
    /// Builds the door, checking for a resumable autosave.
    pub fn open() -> Self {
        Self::with_resumable(false)
    }

    pub(crate) fn set_catalog(&mut self, resumable: bool) {
        let selected = self.menu.value().copied();
        let mut fresh = Self::with_resumable(resumable).with_rejoin(self.rejoin.take());
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

    fn resumable(&self) -> bool {
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
        Self::build(resumable, !crate::platform::TOUCH_ONLY)
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
        }
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
        let Some(choice) = self.menu.handle(events, mouse) else {
            return Out::Stay;
        };
        sounds.push((SoundKind::Click, None));
        choice.value
    }

    /// Draws the door, with the build in the bottom left corner where a
    /// bug report can quote it, clear of the minimap behind the veil and
    /// below the footer's key help.
    pub fn draw(&self) {
        self.menu.draw(self.subtitle());
        let s = crate::render::ui_scale();
        draw_text(
            version_label(),
            12.0 * s,
            crate::render::viewport().y - 8.0 * s,
            crate::theme::Type::Small.at(s),
            crate::theme::TEXT_SECONDARY,
        );
    }
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
