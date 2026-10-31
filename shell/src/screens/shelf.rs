//! The record shelf: two tabs over one menu, SAVES (resumable sessions,
//! Enter loads) and REPLAYS (finished matches, Enter watches).
//! Windowless update; the main loop opens sessions and calls
//! [`Shelf::draw`].

use crate::game::SoundKind;
use crate::menu::{Heading, Label, Line, Menu};
use crate::saves::ReplayEntry;
use crate::tabs::TabStrip;
use aftermine_protocol::{Key, RawEvent};
use macroquad::prelude::{Vec2, draw_text, measure_text};

/// What a shelf frame decided.
#[derive(Debug, PartialEq)]
pub enum Out {
    /// Still browsing.
    Stay,
    /// Back to the front door.
    Home,
    /// Resume this record as a live session (the caller loads it and
    /// answers for a file that no longer loads).
    Load(std::path::PathBuf),
    /// Watch this record (the caller opens the playback session and
    /// answers for a file that no longer loads).
    Watch(std::path::PathBuf),
    /// A record was deleted; the caller rebuilds the shelf and the
    /// Home menu (the deleted file may have been Continue's save).
    Delete(std::path::PathBuf),
}

/// A shelf tab: which records it lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Resumable sessions: autosaves and saves, local or LAN.
    Saves,
    /// Finished matches, kept to watch.
    Replays,
}

impl Tab {
    /// Every tab, in strip order.
    const ALL: [Tab; 2] = [Tab::Saves, Tab::Replays];

    fn label(self) -> &'static str {
        match self {
            Tab::Saves => "SAVES",
            Tab::Replays => "REPLAYS",
        }
    }

    /// The debug protocol's stable mode name while the tab is up.
    pub fn mode_name(self) -> &'static str {
        match self {
            Tab::Saves => "saves",
            Tab::Replays => "replays",
        }
    }

    /// The tab a step either way lands on: with two, the other one.
    fn other(self) -> Tab {
        match self {
            Tab::Saves => Tab::Replays,
            Tab::Replays => Tab::Saves,
        }
    }

    /// Whether this tab lists `entry`.
    fn holds(self, entry: &ReplayEntry) -> bool {
        entry.kind.resumable() == (self == Tab::Saves)
    }

    /// What the tab says when it lists nothing.
    fn empty(self) -> &'static str {
        match self {
            Tab::Saves => "no saves yet: save from the pause menu or quit a match mid-way",
            Tab::Replays => "no replays yet: finish a match to keep its replay",
        }
    }

    /// Where a shelf opens over `entries`: on SAVES, unless only
    /// replays are waiting.
    fn opening(entries: &[ReplayEntry]) -> Tab {
        let holds_any = |tab: Tab| entries.iter().any(|entry| tab.holds(entry));
        if !holds_any(Tab::Saves) && holds_any(Tab::Replays) {
            Tab::Replays
        } else {
            Tab::Saves
        }
    }
}

/// The shelf screen: discovered records, the active tab's menu, and the
/// two-press delete arming state.
pub struct Shelf {
    /// Everything loadable, watchable, or deletable, newest first.
    pub entries: Vec<ReplayEntry>,
    pub(crate) catalog_ready: bool,
    /// The tab on show; `None` until the first catalog lands or the
    /// player picks one, so the shelf opens where the records are.
    tab: Option<Tab>,
    /// When the rows were dated, in unix seconds.
    now: u64,
    /// The active tab's rows, each standing for its index into
    /// `entries`. The exit is the BACK button outside the rows, so no
    /// delete-refresh can drop it and strand a mouse-only player in an
    /// exitless menu.
    pub menu: Menu<usize>,
    /// Entry armed for deletion; X on the same entry confirms.
    pub arming: Option<usize>,
    back: crate::button::BackButton,
    tabs: TabStrip,
}

impl Shelf {
    /// Scans the save and replay directories like the front door does.
    pub fn open() -> Self {
        let mut shelf = Self::from_entries(Vec::new());
        shelf.catalog_ready = false;
        shelf.tab = None;
        shelf
    }

    pub(crate) fn set_catalog(&mut self, entries: Vec<ReplayEntry>) {
        let selected = self.menu.value().map(|i| self.entries[*i].path.clone());
        let mut fresh = Self::at(entries, unix_now(), self.tab);
        if let Some(path) = selected {
            let paths: Vec<_> = fresh.entries.iter().map(|e| e.path.clone()).collect();
            fresh.menu.select_where(|i| paths[*i] == path);
        }
        fresh.catalog_ready = true;
        // A refresh landing mid-press must not drop the BACK or tab
        // gesture.
        fresh.back = std::mem::take(&mut self.back);
        fresh.tabs = std::mem::take(&mut self.tabs);
        *self = fresh;
    }

    /// Builds the shelf over the given records (tests inject their own),
    /// open on the tab [`Tab::opening`] picks.
    pub fn from_entries(entries: Vec<ReplayEntry>) -> Self {
        Self::at(entries, unix_now(), None)
    }

    /// The shelf as of `now` (unix seconds), which dates every row, on
    /// `tab`, or where the records are when no tab is chosen yet.
    fn at(entries: Vec<ReplayEntry>, now: u64, tab: Option<Tab>) -> Self {
        let tab = tab.unwrap_or_else(|| Tab::opening(&entries));
        let menu = tab_menu(&entries, tab, now);
        Self {
            entries,
            catalog_ready: true,
            tab: Some(tab),
            now,
            menu,
            arming: None,
            back: crate::button::BackButton::default(),
            tabs: TabStrip::default(),
        }
    }

    /// The tab on show.
    pub fn tab(&self) -> Tab {
        self.tab.unwrap_or(Tab::Saves)
    }

    /// The debug protocol's stable mode name for the tab on show.
    pub fn mode_name(&self) -> &'static str {
        self.tab().mode_name()
    }

    /// Shows `tab` from its first row, disarming any delete.
    fn show(&mut self, tab: Tab) {
        self.tab = Some(tab);
        self.menu = tab_menu(&self.entries, tab, self.now);
        self.arming = None;
    }

    /// Applies a frame's events. Deletion happens here (two X presses
    /// on the same row); everything session-shaped is returned to the
    /// caller as an [`Out`].
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let escaped = events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }));
        let x_pressed = events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::X }));
        let (back, events) = self.back.route(events);
        if back {
            sounds.push((SoundKind::Click, None));
            return Out::Home;
        }
        let (picked, events) = self.tabs.route(&events, Tab::ALL.len());
        let next = picked
            .map(|index| Tab::ALL[index])
            .or_else(|| steps_tab(&events).then(|| self.tab().other()));
        if let Some(next) = next {
            sounds.push((SoundKind::Click, None));
            if next == self.tab() {
                self.tab = Some(next);
            } else {
                self.show(next);
            }
            return Out::Stay;
        }
        let picked = self.menu.handle(&events, mouse);
        if escaped {
            return Out::Home;
        }
        if let Some(row) = picked {
            sounds.push((SoundKind::Click, None));
            return match self.entries.get(row.value) {
                Some(entry) if entry.available() && entry.kind.resumable() => {
                    Out::Load(entry.path.clone())
                }
                Some(entry) if entry.available() => Out::Watch(entry.path.clone()),
                Some(_) => {
                    // The status line already says why.
                    sounds.push((SoundKind::Denied, None));
                    Out::Stay
                }
                None => Out::Stay,
            };
        }
        if x_pressed
            && self.catalog_ready
            && let Some(&i) = self.menu.value()
        {
            if self.arming == Some(i) {
                self.arming = None;
                return Out::Delete(self.entries[i].path.clone());
            }
            self.arming = Some(i);
        }
        Out::Stay
    }

    /// How to act on the focused row: coaching for the footer. None while
    /// a delete is armed, since the status line already says what to
    /// press.
    pub fn coaching(&self) -> Option<String> {
        let i = *self.menu.value()?;
        if self.arming == Some(i) {
            return None;
        }
        let hands = crate::platform::hands();
        let hint = self
            .entries
            .get(i)
            .map(|entry| entry.hint(hands))
            .filter(|hint| !hint.is_empty())?;
        Some(if hands.keys && crate::tabs::keys().is_some() {
            format!("{hint} | {{prev_tab}}/{{next_tab}} switch tabs")
        } else {
            hint
        })
    }

    /// The line above the footer: what the shelf is doing, or what is
    /// wrong with the focused record. Rows say everything else.
    pub fn status(&self) -> String {
        if !self.catalog_ready {
            "looking for saves and replays...".to_string()
        } else if self.menu.lines().is_empty() {
            self.tab().empty().to_string()
        } else if self.arming.is_some() && self.arming == self.menu.value().copied() {
            "press {delete} again to delete this record".to_string()
        } else {
            self.menu
                .value()
                .and_then(|i| self.entries.get(*i)?.problem)
                .map_or_else(String::new, |problem| problem.reason().to_string())
        }
    }

    /// Draws the tab's rows, the strip, and the status line over the
    /// veil the caller drew. `notice_shown` says the app's menu notice
    /// holds the status line's place this frame.
    pub fn draw(&self, mouse: Vec2, notice_shown: bool) {
        self.menu.draw_with_coaching("", self.coaching().as_deref());
        let keys = crate::tabs::keys().filter(|_| crate::platform::hands().keys);
        let active = Tab::ALL
            .iter()
            .position(|tab| *tab == self.tab())
            .unwrap_or_default();
        crate::tabs::draw(
            &Tab::ALL.map(Tab::label),
            active,
            mouse,
            keys.as_ref()
                .map(|(prev, next)| (prev.as_str(), next.as_str())),
        );
        let status = crate::menu::binding_hint(&self.status());
        if notice_shown || status.is_empty() {
            return;
        }
        let s = crate::render::ui_scale();
        let size = crate::theme::Type::Body.at(s);
        let width = measure_text(&status, None, crate::numeric::font_size(size), 1.0).width;
        draw_text(
            &status,
            (crate::render::viewport().x - width) * 0.5,
            crate::render::viewport().y - 48.0 * s,
            size,
            crate::theme::TEXT_SECONDARY,
        );
    }
}

/// `tab`'s rows over `entries`, dated as of `now`, under the shelf's
/// page title with room for the strip.
fn tab_menu(entries: &[ReplayEntry], tab: Tab, now: u64) -> Menu<usize> {
    let lines = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| tab.holds(entry))
        .map(|(i, entry)| {
            let label = Label::Record {
                title: entry.title.clone(),
                map: entry.map.clone(),
                tag: entry.kind.tag(),
                format: entry.format.clone(),
                clock: entry.clock.clone(),
                when: crate::saves::saved_ago(entry.saved_secs, now),
                available: entry.available(),
            };
            Line::Row(label, i)
        })
        .collect();
    let mut menu = Menu::new("SAVES & REPLAYS", lines);
    menu.wide = true;
    menu.heading = Heading::Page {
        band: crate::tabs::BAND,
    };
    menu
}

/// Whether a frame's keys step the tabs. Menus receive the tab actions
/// as Q and E, and with two tabs either way lands on the other.
fn steps_tab(events: &[RawEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            RawEvent::KeyDown {
                key: Key::Q | Key::E
            }
        )
    })
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests;
