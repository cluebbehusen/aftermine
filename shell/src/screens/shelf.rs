//! The record shelf: two tabs over one menu, SAVES (resumable sessions,
//! Enter loads) and REPLAYS (finished matches, Enter watches).
//! Windowless update; the main loop opens sessions and calls
//! [`Shelf::draw`].

use crate::game::SoundKind;
use crate::menu::{Answer, Heading, Label, Line, Menu};
use crate::platform::{PLATFORM, Platform};
use crate::saves::ReplayEntry;
use crate::tabs::TabStrip;
use aftermine_apple_bridge::cloudkit::Trouble;
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
            Tab::Saves => "Saves",
            Tab::Replays => "Replays",
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
            Tab::Saves => "No saves yet",
            Tab::Replays => "No replays yet",
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

/// The question a delete asks first.
struct Ask {
    /// The record it would delete.
    path: std::path::PathBuf,
    /// The record's name, and its map beside a name the player gave.
    subtitle: String,
    /// Cancel, preselected, then Delete.
    menu: Menu<bool>,
}

/// The shelf screen: discovered records, the active tab's menu, and the
/// question a delete asks first.
pub struct Shelf {
    /// Everything loadable, watchable, or deletable, newest first.
    pub entries: Vec<ReplayEntry>,
    pub(crate) catalog_ready: bool,
    /// Records still downloading from iCloud at the last scan.
    downloading: usize,
    /// Why records are not backing up to iCloud, if they are not.
    backup: Option<Trouble>,
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
    /// The delete waiting on the player's answer, drawn in place of the
    /// tab.
    asking: Option<Ask>,
    back: crate::button::BackButton,
    tabs: TabStrip,
}

impl Shelf {
    /// An empty shelf awaiting the worker's scan of saves and replays.
    pub fn open() -> Self {
        let mut shelf = Self::from_entries(Vec::new());
        shelf.catalog_ready = false;
        shelf.tab = None;
        shelf
    }

    pub(crate) fn set_catalog(&mut self, entries: Vec<ReplayEntry>, downloading: usize) {
        // A rescan while files download usually finds the same records;
        // keeping the menu then keeps any press on it.
        let unchanged = self.catalog_ready
            && self.entries.len() == entries.len()
            && self
                .entries
                .iter()
                .zip(&entries)
                .all(|(old, new)| old.path == new.path && old.saved_secs == new.saved_secs);
        if unchanged {
            self.downloading = downloading;
            return;
        }
        let selected = self.menu.value().map(|i| self.entries[*i].path.clone());
        let line = self.menu.selected;
        // A question about a record that has gone, deleted on another
        // device, has nothing left to ask about.
        let asking = self
            .asking
            .take()
            .filter(|ask| entries.iter().any(|entry| entry.path == ask.path));
        let mut fresh = Self::at(entries, unix_now(), self.tab);
        if let Some(path) = selected {
            let paths: Vec<_> = fresh.entries.iter().map(|e| e.path.clone()).collect();
            if !fresh.menu.select_where(|i| paths[*i] == path) {
                // The focused record is gone: the cursor stays in place,
                // on the record that moved up into its row.
                fresh.menu.select(line);
            }
        }
        fresh.asking = asking;
        fresh.catalog_ready = true;
        fresh.downloading = downloading;
        fresh.backup = self.backup;
        // A refresh landing mid-press must not drop the BACK or tab
        // gesture.
        fresh.back = std::mem::take(&mut self.back);
        fresh.tabs = std::mem::take(&mut self.tabs);
        fresh.menu.cursor_hidden = fresh.tabs.focused();
        *self = fresh;
    }

    /// Takes why records are not backing up to iCloud, or `None` while they
    /// are.
    pub(crate) fn set_backup(&mut self, trouble: Option<Trouble>) {
        self.backup = trouble;
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
            downloading: 0,
            backup: None,
            tab: Some(tab),
            now,
            menu,
            asking: None,
            back: crate::button::BackButton::default(),
            tabs: TabStrip::default(),
        }
    }

    /// The tab on show.
    pub fn tab(&self) -> Tab {
        self.tab.unwrap_or(Tab::Saves)
    }

    /// The debug protocol's stable mode name: the delete question while
    /// it is up, else the tab on show.
    pub fn mode_name(&self) -> &'static str {
        if self.asking.is_some() {
            "confirm_delete"
        } else {
            self.tab().mode_name()
        }
    }

    /// What automation reads: the delete question while it is up, else
    /// the tab's rows.
    pub fn view(&self) -> crate::menu::MenuView {
        self.asking
            .as_ref()
            .map_or_else(|| self.menu.view(), |ask| ask.menu.view())
    }

    /// Shows `tab` from its first row.
    fn show(&mut self, tab: Tab) {
        self.tab = Some(tab);
        self.menu = tab_menu(&self.entries, tab, self.now);
        self.menu.cursor_hidden = self.tabs.focused();
    }

    /// Asks before deleting `entries[index]`. Cancel sits first and
    /// preselected, as in the other dialogs, so a second press of the key
    /// or button that asked never deletes.
    fn ask(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let title = if entry.kind.resumable() {
            "DELETE THIS SAVE?"
        } else {
            "DELETE THIS REPLAY?"
        };
        let subtitle = match &entry.map {
            Some(map) => format!("{} \u{b7} {map}", entry.title),
            None => entry.title.clone(),
        };
        self.asking = Some(Ask {
            path: entry.path.clone(),
            subtitle,
            menu: Menu::rows(
                title,
                [
                    (Label::from("Cancel"), false),
                    (Label::from("Delete"), true),
                ],
            ),
        });
    }

    /// Applies a frame's events. A delete asks first, here; the deletion
    /// itself and everything session-shaped are returned to the caller
    /// as an [`Out`].
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let escaped = crate::nav::escaped(events);
        let delete_pressed = events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::X }));
        let (back, events) = self.back.route(events);
        if let Some(ask) = &mut self.asking {
            match ask.menu.answer(&events, mouse, back || escaped, sounds) {
                Some(Answer::Picked(true)) => {
                    let ask = self.asking.take();
                    return ask.map_or(Out::Stay, |ask| Out::Delete(ask.path));
                }
                Some(_) => self.asking = None,
                None => {}
            }
            return Out::Stay;
        }
        if back {
            sounds.push((SoundKind::Click, None));
            return Out::Home;
        }
        let (picked, events) = self.tabs.route(&events, Tab::ALL.len());
        let (stepped, events) = self.tabs.route_remote_above(&events, &mut self.menu);
        let next = picked
            .map(|index| Tab::ALL[index])
            .or_else(|| (stepped.is_some() || steps_tab(&events)).then(|| self.tab().other()));
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
            if row.column == Some(crate::menu::ROW_ACTION) {
                // A list still rescanning after a delete may hold the
                // record already gone.
                if self.catalog_ready {
                    self.ask(row.value);
                }
                return Out::Stay;
            }
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
        if delete_pressed
            && self.catalog_ready
            && let Some(&i) = self.menu.value()
        {
            sounds.push((SoundKind::Click, None));
            self.ask(i);
        }
        Out::Stay
    }

    /// How to act on the focused row: coaching for the footer.
    pub fn coaching(&self) -> Option<String> {
        if self.tabs.focused() {
            return Some(crate::tabs::ON_STRIP_HINT.to_string());
        }
        let hands = crate::platform::hands();
        let hint = self
            .menu
            .value()
            .and_then(|i| self.entries.get(*i))
            .map(|entry| entry.hint(hands, self.menu.on_action()))
            .filter(|hint| !hint.is_empty());
        if hands.pad == Some(aftermine_protocol::PadFamily::Remote) {
            // An empty tab or a damaged record still leaves the tabs to reach.
            return Some(hint.map_or_else(
                || crate::tabs::REMOTE_HINT.to_string(),
                |hint| format!("{hint} | {}", crate::tabs::REMOTE_HINT),
            ));
        }
        let hint = hint?;
        Some(if crate::tabs::shown_keys().is_some() {
            format!("{hint} | {{prev_tab}}/{{next_tab}} switch tabs")
        } else {
            hint
        })
    }

    /// What the tab says in place of rows, once no record is on its way.
    fn empty_message(&self) -> Option<&'static str> {
        (self.catalog_ready && self.downloading == 0 && self.menu.lines().is_empty())
            .then(|| self.tab().empty())
    }

    /// The line above the footer: what the shelf is doing, or what is
    /// wrong with the focused record, then on Apple TV why records are not
    /// backing up, and otherwise there that records may not last. Rows and
    /// an empty tab's message say everything else.
    pub fn status(&self) -> String {
        self.status_on(PLATFORM)
    }

    /// [`Self::status`] as `platform` words it.
    pub(crate) fn status_on(&self, platform: Platform) -> String {
        let problem = self
            .menu
            .value()
            .and_then(|i| self.entries.get(*i)?.problem);
        let backup = self
            .backup
            .and_then(|trouble| backup_line(platform, trouble));
        if !self.catalog_ready {
            "looking for saves and replays...".to_string()
        } else if let Some(problem) = problem {
            problem.reason().to_string()
        } else if self.downloading > 0 {
            downloading_line(self.downloading)
        } else if let Some(backup) = backup {
            backup.to_string()
        } else if self.empty_message().is_some() {
            String::new()
        } else {
            standing_line(platform).to_string()
        }
    }

    /// Draws the delete question, or else the tab's rows, the strip, and
    /// the status line, over the veil the caller drew. `notice_shown`
    /// says the app's menu notice holds the status line's place this
    /// frame.
    pub fn draw(&self, sprites: &crate::assets::Sprites, mouse: Vec2, notice_shown: bool) {
        if let Some(ask) = &self.asking {
            ask.menu.draw_named(sprites, &ask.subtitle);
            return;
        }
        self.menu
            .draw_with_coaching(sprites, "", self.coaching().as_deref());
        let keys = crate::tabs::shown_keys();
        let active = Tab::ALL
            .iter()
            .position(|tab| *tab == self.tab())
            .unwrap_or_default();
        crate::tabs::draw(
            sprites,
            &Tab::ALL.map(Tab::label),
            active,
            mouse,
            keys.as_ref(),
            self.tabs.focused(),
        );
        let s = crate::render::ui_scale();
        let view_h = crate::render::viewport().y;
        if let Some(message) = self.empty_message() {
            draw_centered(message, view_h * 0.5, crate::theme::Type::Row.at(s));
        }
        let status = crate::menu::binding_hint(&self.status());
        if notice_shown || status.is_empty() {
            return;
        }
        draw_centered(&status, view_h - 48.0 * s, crate::theme::Type::Body.at(s));
    }
}

/// Draws secondary text centered across the screen, on `baseline`.
fn draw_centered(text: &str, baseline: f32, size: f32) {
    let width = measure_text(text, None, crate::numeric::font_size(size), 1.0).width;
    draw_text(
        text,
        (crate::render::viewport().x - width) * 0.5,
        baseline,
        size,
        crate::theme::TEXT_SECONDARY,
    );
}

/// What the status line says when nothing more pressing does: on Apple TV,
/// which may clear the cache that holds every record, that records may not
/// last.
fn standing_line(platform: Platform) -> &'static str {
    match platform {
        Platform::Tv => "Apple TV may delete saves and replays when it needs space",
        Platform::Desktop | Platform::Touch => "",
    }
}

/// What the status line says while records are not backing up to iCloud:
/// only on Apple TV, where iCloud holds the only lasting copy of each
/// record. iPad and Mac keep theirs in iCloud Drive or on the device.
fn backup_line(platform: Platform, trouble: Trouble) -> Option<&'static str> {
    if platform != Platform::Tv {
        return None;
    }
    Some(match trouble {
        Trouble::StorageFull => "iCloud storage is full, so saves and replays are not backed up",
        Trouble::NoAccount => "Sign in to iCloud to back up saves and replays",
        Trouble::Failing => "Could not back up saves and replays to iCloud",
    })
}

/// What the status line says while records download from iCloud.
fn downloading_line(files: usize) -> String {
    if files == 1 {
        "Downloading 1 file from iCloud".to_string()
    } else {
        format!("Downloading {files} files from iCloud")
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
    menu.row_action = Some("Delete");
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
