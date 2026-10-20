//! Settings: four tabs of rows, the Controls tab holding the binding
//! editor. Config edits and binding capture are pure state updated here;
//! persistence and drawing stay with the caller, so binding capture runs
//! under headless tests.

use crate::action::{Action, BindingMap, Chord, Context};
use crate::config::{Config, MarkerPrefs, PerformanceDisplay};
use crate::game::SoundKind;
use crate::menu::{Heading, Label, Line, Menu};
use crate::nav::{Axis, Nav, step_line};
use crate::numeric;
use crate::numeric::Fit;
use crate::render;
use crate::tabs::TabStrip;
use crate::text_field::{Edit, Field, Form, Keyboard};
use macroquad::prelude::{Vec2, draw_text, measure_text};
use oxide_net::{NameError, PlayerName};
use oxide_protocol::{Key, RawEvent};

/// A Settings tab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tab {
    General,
    Audio,
    Display,
    /// Camera input, binding presets, and the binding editor.
    Controls,
}

impl Tab {
    /// Every tab, in strip order.
    const ALL: [Tab; 4] = [Tab::General, Tab::Audio, Tab::Display, Tab::Controls];

    fn label(self) -> &'static str {
        match self {
            Tab::General => "GENERAL",
            Tab::Audio => "AUDIO",
            Tab::Display => "DISPLAY",
            Tab::Controls => "CONTROLS",
        }
    }

    /// The debug protocol's stable mode name while the tab is up.
    fn mode_name(self) -> &'static str {
        match self {
            Tab::General => "settings",
            Tab::Audio => "settings_audio",
            Tab::Display => "settings_display",
            Tab::Controls => "controls",
        }
    }

    /// The tab `delta` steps away along the strip, wrapping at the ends.
    fn stepped(self, delta: isize) -> Tab {
        let at = Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .unwrap_or_default();
        Self::ALL[(at.fit::<isize>() + delta)
            .rem_euclid(Self::ALL.len().fit::<isize>())
            .fit::<usize>()]
    }
}

/// What the screen shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Face {
    /// A tab's rows.
    Tab(Tab),
    /// The Controls tab, listening for the chord to bind to the action.
    Capturing(Action),
    /// The Controls tab asking before a profile replaces every binding.
    Confirming(Profile),
}

impl Face {
    fn tab(self) -> Tab {
        match self {
            Face::Tab(tab) => tab,
            Face::Capturing(_) | Face::Confirming(_) => Tab::Controls,
        }
    }
}

/// A whole binding profile a Controls row puts in place of every
/// binding, the player's rebinds included, once the player confirms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Profile {
    /// The classic defaults.
    Classic,
    /// The left-handed preset.
    LeftHanded,
}

impl Profile {
    /// The Controls row that asks for it.
    fn item(self) -> Item {
        match self {
            Profile::Classic => Item::ResetBindings,
            Profile::LeftHanded => Item::Setting(Row::LeftHandedPreset),
        }
    }

    fn bindings(self) -> BindingMap {
        match self {
            Profile::Classic => BindingMap::classic(),
            Profile::LeftHanded => BindingMap::left_handed(),
        }
    }

    /// The dialog's title.
    fn question(self) -> &'static str {
        match self {
            Profile::Classic => "RESET BINDINGS?",
            Profile::LeftHanded => "LEFT-HANDED BINDINGS?",
        }
    }

    /// The dialog's subtitle: what confirming costs.
    fn consequence(self) -> &'static str {
        match self {
            Profile::Classic => "your rebinds are replaced by the defaults",
            Profile::LeftHanded => "your rebinds are replaced by the left-handed layout",
        }
    }

    /// The dialog row that carries it out.
    fn verb(self) -> &'static str {
        match self {
            Profile::Classic => "Reset bindings",
            Profile::LeftHanded => "Apply left-handed bindings",
        }
    }

    /// The notice once it is in place.
    fn applied(self) -> &'static str {
        match self {
            Profile::Classic => "bindings reset to defaults",
            Profile::LeftHanded => "left-handed profile applied",
        }
    }
}

/// What a settings frame decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Still tuning.
    Stay,
    /// Reveal local diagnostic records in the file manager.
    OpenDiagnostics,
    /// Export a consistent local report in the background.
    ExportDiagnostics,
    /// Back to wherever the screen was opened from — the coordinator
    /// holds the displaced screen and restores it wholesale.
    Leave,
}

/// A screen-owned status line, drawn by [`SettingsScreen::draw`] above
/// the caller's veil, where a HUD toast would be hidden. It persists
/// until the next action, so it needs no wall clock.
pub struct Notice {
    /// The message.
    pub text: String,
    /// A complaint (danger red) rather than a confirmation.
    pub danger: bool,
}

/// A frame's full result: the transition and whether the config
/// changed (the caller persists — screens never touch the disk, which
/// keeps their tests hermetic).
pub struct Update {
    /// Where to go.
    pub out: Out,
    /// The config changed; persist it.
    pub dirty: bool,
}

/// Every keyboard action a player can use is exposed, including contextual
/// cards and navigation; a packaged copy leaves out the debug overlay.
fn control_sections() -> Vec<(&'static str, Vec<Action>)> {
    use Action::{
        Accompany, AssignGroup, Back, Build, BuildCategory, CancelRound, ClearRally, Confirm,
        CycleIdleWorker, DeleteSave, HomeCamera, Hunt, JumpToLastAlert, Launch, MenuDown, MenuEnd,
        MenuHome, MenuLeft, MenuPageDown, MenuPageUp, MenuRight, MenuUp, NextTab, OrderRound,
        PanDown, PanLeft, PanRight, PanUp, Patrol, PrevTab, RecallBookmark, RepairUnit, ReplayBack,
        ReplayEnd, ReplayForward, ReplayPause, ReplaySpeed, ReplayStart, ReplayStats, Retire,
        ReturnCargo, Run, Salvage, Scout, SetBookmark, SetRally, Slot, StopOrScrap, ToggleAutofill,
        ToggleBuildPalette, ToggleIdleWeld, ToggleOverlay, TogglePause, TrainSlot, Unload, Upgrade,
    };
    let mut sections = vec![
        (
            "Camera",
            vec![
                PanUp,
                PanLeft,
                PanDown,
                PanRight,
                HomeCamera,
                CycleIdleWorker,
                JumpToLastAlert,
            ],
        ),
        (
            "Orders",
            vec![
                StopOrScrap,
                Run,
                Hunt,
                Scout,
                Patrol,
                Accompany,
                Salvage,
                RepairUnit,
                ToggleIdleWeld,
                ReturnCargo,
                Unload,
                Retire,
            ],
        ),
        (
            "Buildings",
            vec![ToggleBuildPalette, Upgrade, SetRally, ClearRally],
        ),
        (
            "Strategic",
            vec![Launch, OrderRound, CancelRound, ToggleAutofill],
        ),
        (
            "Production",
            (0..crate::action::TRAIN_SLOTS).map(TrainSlot).collect(),
        ),
        (
            "Construction categories",
            (0..crate::action::BUILD_CATEGORIES.len())
                .map(|n| BuildCategory(n.fit::<u8>()))
                .collect(),
        ),
    ];
    for (name, kinds) in crate::action::BUILD_CATEGORIES {
        sections.push((name, kinds.iter().copied().map(Build).collect()));
    }
    sections.extend([
        (
            "Control groups",
            (1..=5).flat_map(|n| [Slot(n), AssignGroup(n)]).collect(),
        ),
        (
            "Camera bookmarks",
            (0..4)
                .flat_map(|n| [RecallBookmark(n), SetBookmark(n)])
                .collect(),
        ),
        (
            "Match",
            if crate::paths::bundled() {
                vec![TogglePause]
            } else {
                vec![TogglePause, ToggleOverlay]
            },
        ),
        (
            "Replay",
            vec![
                ReplayPause,
                ReplayBack,
                ReplayForward,
                ReplayStart,
                ReplayEnd,
                ReplayStats,
            ],
        ),
        ("Replay speeds", (0..8).map(ReplaySpeed).collect()),
        (
            "Menus",
            vec![
                Back,
                Confirm,
                MenuUp,
                MenuDown,
                MenuLeft,
                MenuRight,
                MenuPageUp,
                MenuPageDown,
                MenuHome,
                MenuEnd,
                PrevTab,
                NextTab,
                DeleteSave,
            ],
        ),
    ]);
    sections
}
/// What a row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Item {
    /// A settings row.
    Setting(Row),
    /// A binding row: its action's two chords.
    Binding(Action),
    /// Every binding back to the classic map.
    ResetBindings,
    /// A dialog's way out without acting.
    Cancel,
    /// A dialog's way through.
    Confirm,
}

/// One settings row. Rows are values, not indices: the label, the step,
/// and the activation route all key off the row itself, so inserting a
/// row cannot leave a stale index pointing at its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    PlayerName,
    MatchTips,
    PerformanceDisplay,
    OpenDiagnostics,
    ExportDiagnostics,
    MasterVolume,
    EffectsVolume,
    UiVolume,
    MusicVolume,
    UiScale,
    ReducedMotion,
    Colorblind,
    ControlGroups,
    MarkerTiming,
    MarkerSize,
    EdgePan,
    InvertZoom,
    LeftHandedPreset,
}

/// The capture prompt, shown while a binding row listens.
const CAPTURE_PROMPT: &str = "press the new chord (modifiers held count) - Escape cancels";

/// Volume steps, low to high.
const VOLUMES: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// Interface and marker scale steps, small to large.
const SCALES: [f32; 4] = [render::MIN_USER_SCALE, 1.0, 1.25, render::MAX_USER_SCALE];

impl Row {
    /// Every row, in the order its tab shows them.
    const ALL: [Row; 18] = [
        Row::PlayerName,
        Row::MatchTips,
        Row::PerformanceDisplay,
        Row::OpenDiagnostics,
        Row::ExportDiagnostics,
        Row::MasterVolume,
        Row::EffectsVolume,
        Row::UiVolume,
        Row::MusicVolume,
        Row::UiScale,
        Row::ReducedMotion,
        Row::Colorblind,
        Row::ControlGroups,
        Row::MarkerTiming,
        Row::MarkerSize,
        Row::EdgePan,
        Row::InvertZoom,
        Row::LeftHandedPreset,
    ];

    /// The tab the row lives on.
    fn tab(self) -> Tab {
        match self {
            Row::PlayerName
            | Row::MatchTips
            | Row::PerformanceDisplay
            | Row::OpenDiagnostics
            | Row::ExportDiagnostics => Tab::General,
            Row::MasterVolume | Row::EffectsVolume | Row::UiVolume | Row::MusicVolume => Tab::Audio,
            Row::UiScale
            | Row::ReducedMotion
            | Row::Colorblind
            | Row::ControlGroups
            | Row::MarkerTiming
            | Row::MarkerSize => Tab::Display,
            Row::EdgePan | Row::InvertZoom | Row::LeftHandedPreset => Tab::Controls,
        }
    }

    /// Rows a touch-only build cannot use: it has no file manager to
    /// open a folder in. A keyboard or trackpad can join a touch device
    /// at any time, so their rows stay.
    fn needs_desktop(self) -> bool {
        matches!(self, Row::OpenDiagnostics)
    }

    fn label(self, config: &Config) -> Label {
        let pct = |v: f32| format!("{}%", (v * 100.0).round());
        let on = |v: bool| if v { "On" } else { "Off" }.to_string();
        let setting = |name: &str, value: String| Label::Setting {
            name: name.to_string(),
            value,
        };
        match self {
            Row::PlayerName => setting(
                "Name",
                config
                    .player_name
                    .clone()
                    .unwrap_or_else(|| "not set".to_string()),
            ),
            Row::MatchTips => setting("Match tips", on(config.match_tips)),
            Row::PerformanceDisplay => setting(
                "Performance readout",
                config.performance_display.label().to_string(),
            ),
            Row::MasterVolume => setting("Master volume", pct(config.volumes.master)),
            Row::EffectsVolume => setting("Effects volume", pct(config.volumes.effects)),
            Row::UiVolume => setting("UI volume", pct(config.volumes.ui)),
            Row::MusicVolume => setting("Music volume", pct(config.volumes.music)),
            Row::UiScale => setting("UI scale", pct(config.ui_scale)),
            Row::ReducedMotion => setting("Reduced motion", on(config.reduced_motion)),
            Row::Colorblind => setting("Colorblind accents", on(config.colorblind)),
            Row::ControlGroups => setting(
                "Control group bar",
                if config.control_groups {
                    "Shown"
                } else {
                    "Hidden"
                }
                .to_string(),
            ),
            Row::MarkerTiming => setting(
                "Strategic marker timing",
                config.markers.timing_label().to_string(),
            ),
            Row::MarkerSize => setting("Strategic marker size", pct(config.markers.scale)),
            Row::EdgePan => setting("Edge pan", on(config.camera.edge_pan)),
            Row::InvertZoom => setting("Invert zoom", on(config.camera.zoom_inverted)),
            Row::LeftHandedPreset => Label::from("Apply left-handed bindings"),
            Row::OpenDiagnostics => Label::from("Open diagnostics folder"),
            Row::ExportDiagnostics => Label::from("Export diagnostic report"),
        }
    }
}

/// The settings tabs' coaching line. `tab_keys` says a key steps the
/// tabs each way; a tab action left unbound has none once another menu
/// action takes its classic key.
fn settings_hint(hands: crate::platform::Hands, tab_keys: bool) -> &'static str {
    match (hands.touch(), hands.keys, tab_keys) {
        (false, true, true) => {
            "{left}/{right} or {confirm} change a value | {prev_tab}/{next_tab} switch tabs"
        }
        (false, true, false) => "{left}/{right} or {confirm} change a value",
        (false, false, _) => "click a row to change it, or a tab to switch",
        (true, true, true) => {
            "tap a row or press {confirm} to change it | {prev_tab}/{next_tab} switch tabs"
        }
        (true, true, false) => "tap a row or press {confirm} to change it",
        (true, false, _) => "tap a row to change it, or a tab to switch",
    }
}

/// The rows `tab` offers on this build, in order.
fn rows(tab: Tab, touch_only: bool) -> Vec<Row> {
    Row::ALL
        .into_iter()
        .filter(|row| row.tab() == tab && !(touch_only && row.needs_desktop()))
        .collect()
}

/// A tab's lines: its rows, and on Controls the reset row and the
/// binding sections, the chosen chord column bracketed on every
/// binding row.
fn tab_lines(tab: Tab, config: &Config, selected_slot: usize) -> Vec<Line<Item>> {
    let mut lines: Vec<Line<Item>> = rows(tab, crate::platform::TOUCH_ONLY)
        .into_iter()
        .map(|row| Line::Row(row.label(config), Item::Setting(row)))
        .collect();
    if tab == Tab::Controls {
        lines.push(Line::Row(
            Label::from("Reset bindings to defaults"),
            Item::ResetBindings,
        ));
        lines.extend(binding_lines(config, selected_slot));
    }
    lines
}

fn binding_lines(config: &Config, selected_slot: usize) -> Vec<Line<Item>> {
    let mut lines = Vec::new();
    for (section, actions) in control_sections() {
        lines.push(Line::Header(section.to_uppercase()));
        for action in actions {
            let key = |slot| {
                config
                    .bindings
                    .chord_at(action, slot)
                    .map_or_else(|| "unbound".into(), BindingMap::chord_label)
            };
            let label = Label::Binding {
                name: action.label(),
                keys: [key(0), key(1)],
                marked: Some(selected_slot),
            };
            lines.push(Line::Row(label, Item::Binding(action)));
        }
    }
    lines
}

fn tab_menu(tab: Tab, config: &Config, selected_slot: usize) -> Menu<Item> {
    let mut menu = Menu::new("SETTINGS", tab_lines(tab, config, selected_slot));
    menu.wide = true;
    menu.heading = Heading::Page {
        band: crate::tabs::BAND,
    };
    menu
}

/// The dialog asking before `profile` replaces every binding. Cancel
/// sits first and preselected, as in the pause menu's dialogs: a second
/// Enter or tap never costs the player their rebinds. On a touch screen
/// a row lit by an earlier tap looks just like one pressed, so the
/// dialog is what tells the player a press asked for the change.
fn confirm_menu(profile: Profile) -> Menu<Item> {
    let mut menu = Menu::rows(
        profile.question(),
        [
            (Label::from("Cancel"), Item::Cancel),
            (Label::from(profile.verb()), Item::Confirm),
        ],
    );
    menu.heading = Heading::Page { band: 0.0 };
    menu
}

/// Which way a press moves a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Enter, a click or a tap: on to the next value, wrapping.
    Cycle,
    /// Left: one value down, stopping at the first.
    Back,
    /// Right: one value up, stopping at the last.
    Forward,
}

/// Where `step` moves position `at` among `len` ordered values.
fn stepped(len: usize, at: usize, step: Step) -> usize {
    let last = len.saturating_sub(1);
    match step {
        Step::Cycle if at >= last => 0,
        Step::Cycle | Step::Forward => (at + 1).min(last),
        Step::Back => at.saturating_sub(1).min(last),
    }
}

/// The level `step` moves `current` to among ascending `values`: the
/// nearest one strictly below or above it, so a stored value between
/// two levels moves to its neighbour rather than past it. Back and
/// Forward stop at the ends; Cycle wraps past the last.
fn level(values: &[f32], current: f32, step: Step) -> f32 {
    let (first, last) = (values[0], values[values.len() - 1]);
    let below = values.iter().rev().find(|value| **value < current);
    let above = values.iter().find(|value| **value > current);
    match step {
        Step::Back => below.copied().unwrap_or(first),
        Step::Forward => above.copied().unwrap_or(last),
        Step::Cycle => above.copied().unwrap_or(first),
    }
}

/// Moves one settings row's value and applies it live. False on rows
/// that act instead of holding a value.
fn step_setting(config: &mut Config, row: Row, step: Step) -> bool {
    let toggle = |on: bool| stepped(2, usize::from(on), step) == 1;
    match row {
        Row::MasterVolume => config.volumes.master = level(&VOLUMES, config.volumes.master, step),
        Row::EffectsVolume => {
            config.volumes.effects = level(&VOLUMES, config.volumes.effects, step);
        }
        Row::UiVolume => config.volumes.ui = level(&VOLUMES, config.volumes.ui, step),
        Row::MusicVolume => config.volumes.music = level(&VOLUMES, config.volumes.music, step),
        Row::UiScale => {
            config.ui_scale = level(&SCALES, config.ui_scale, step);
            render::set_user_scale(config.ui_scale);
        }
        Row::EdgePan => config.camera.edge_pan = toggle(config.camera.edge_pan),
        Row::MatchTips => config.match_tips = toggle(config.match_tips),
        Row::InvertZoom => config.camera.zoom_inverted = toggle(config.camera.zoom_inverted),
        Row::ReducedMotion => {
            config.reduced_motion = toggle(config.reduced_motion);
            render::set_reduced_motion(config.reduced_motion);
        }
        Row::Colorblind => {
            config.colorblind = toggle(config.colorblind);
            render::set_colorblind(config.colorblind);
        }
        Row::PerformanceDisplay => {
            let modes = PerformanceDisplay::ALL;
            let at = modes
                .iter()
                .position(|mode| *mode == config.performance_display)
                .unwrap_or_default();
            config.performance_display = modes[stepped(modes.len(), at, step)];
        }
        Row::ControlGroups => {
            config.control_groups = toggle(config.control_groups);
            render::set_control_groups(config.control_groups);
        }
        Row::MarkerTiming => {
            // A hand-edited custom timing steps back onto Standard.
            let preset = config.markers.timing().map_or_else(
                || MarkerPrefs::default().timing().unwrap_or_default(),
                |at| stepped(MarkerPrefs::TIMINGS.len(), at, step),
            );
            config.markers.set_timing(preset);
            crate::strategic_markers::set_prefs(config.markers);
        }
        Row::MarkerSize => {
            config.markers.scale = level(&SCALES, config.markers.scale, step);
            crate::strategic_markers::set_prefs(config.markers);
        }
        Row::PlayerName | Row::LeftHandedPreset | Row::OpenDiagnostics | Row::ExportDiagnostics => {
            return false;
        }
    }
    true
}

/// The settings screen.
pub struct SettingsScreen {
    face: Face,
    /// The tab's live menu.
    pub menu: Menu<Item>,
    /// The screen's status line, if one is up.
    pub notice: Option<Notice>,
    binding_slot: usize,
    back: crate::button::BackButton,
    tabs: TabStrip,
    /// The name form, while it is open over the General tab.
    naming: Option<Form>,
}

fn name_check(name: &str) -> Result<(), &'static str> {
    PlayerName::new(name).map(drop).map_err(NameError::reason)
}

impl SettingsScreen {
    /// Opens on the General tab. Where leaving lands is the
    /// coordinator's business — it keeps the displaced screen.
    pub fn open(config: &Config) -> Self {
        Self {
            face: Face::Tab(Tab::General),
            menu: tab_menu(Tab::General, config, 0),
            notice: None,
            binding_slot: 0,
            back: crate::button::BackButton::default(),
            tabs: TabStrip::default(),
            naming: None,
        }
    }

    /// The keyboard the name form asks for while it is open.
    pub fn keyboard(&self) -> Option<Keyboard> {
        self.naming.as_ref().map(Form::keyboard)
    }

    /// Consumes the name form's request to raise the on-screen keyboard.
    pub fn take_keyboard_request(&mut self) -> bool {
        self.naming
            .as_mut()
            .is_some_and(Form::take_keyboard_request)
    }

    /// Whether the binding editor is up: it reads raw keys under the
    /// classic map, so a botched rebind can never lock the player out.
    pub fn edits_bindings(&self) -> bool {
        self.naming.is_none() && self.face.tab() == Tab::Controls
    }

    /// Opens the name form, prefilled with the current name.
    fn begin_naming(&mut self, config: &Config) {
        let current = config.player_name.as_deref().unwrap_or_default();
        let field =
            Field::new(current, PlayerName::MAX_CHARS, PlayerName::accepts).validated(name_check);
        let form = Form::new("YOUR NAME", "SAVE", vec![field], 0);
        self.menu = form.menu(|_| Item::Setting(Row::PlayerName));
        self.naming = Some(form);
    }

    /// Closes the name form, back on the Name row.
    fn end_naming(&mut self, config: &Config) {
        self.naming = None;
        self.menu = tab_menu(self.face.tab(), config, self.binding_slot);
        self.menu
            .select_where(|item| *item == Item::Setting(Row::PlayerName));
    }

    /// Runs the name form for a frame, saving a committed name.
    fn update_naming(
        &mut self,
        events: &[RawEvent],
        back: bool,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
        config: &mut Config,
    ) -> bool {
        let Some(form) = &mut self.naming else {
            return false;
        };
        if back {
            self.end_naming(config);
            return false;
        }
        match form.update(events, sounds) {
            Edit::Commit => {
                let name = PlayerName::new(form.value(0)).ok().map(String::from);
                let dirty = name.is_some();
                if dirty {
                    config.player_name = name;
                }
                self.end_naming(config);
                dirty
            }
            Edit::Cancel => {
                self.end_naming(config);
                false
            }
            Edit::Stay => {
                self.menu = form.menu(|_| Item::Setting(Row::PlayerName));
                false
            }
        }
    }

    /// The debug protocol's stable mode name for what is up.
    pub fn mode_name(&self) -> &'static str {
        if self.naming.is_some() {
            return "player_name";
        }
        match self.face {
            Face::Confirming(_) => "confirm_bindings",
            Face::Tab(_) | Face::Capturing(_) => self.face.tab().mode_name(),
        }
    }

    /// The line above the footer: the capture prompt while a binding
    /// row listens, since it is the only sign the next key is taken,
    /// else the standing notice. The flag marks a complaint.
    fn status(&self) -> Option<(&str, bool)> {
        match self.face {
            Face::Capturing(_) => Some((CAPTURE_PROMPT, false)),
            Face::Tab(_) | Face::Confirming(_) => self
                .notice
                .as_ref()
                .map(|notice| (notice.text.as_str(), notice.danger)),
        }
    }

    /// The tab's coaching line, in the footer.
    fn coaching(&self) -> Option<&'static str> {
        match self.face {
            Face::Tab(Tab::Controls) => Some(if self.binding_slot == 0 {
                "PRIMARY selected | Left/Right chooses column | Enter remaps | X clears | Q/E switch tabs"
            } else {
                "SECONDARY selected | Left/Right chooses column | Enter remaps | X clears | Q/E switch tabs"
            }),
            Face::Tab(_) => Some(settings_hint(
                crate::platform::hands(),
                crate::tabs::keys().is_some(),
            )),
            // The dialog keeps the standard menu footer.
            Face::Capturing(_) | Face::Confirming(_) => None,
        }
    }

    /// Shows `tab` from its first row, retiring the notice with the
    /// context it spoke about.
    fn show(&mut self, tab: Tab, config: &Config) {
        self.face = Face::Tab(tab);
        self.menu = tab_menu(tab, config, self.binding_slot);
        self.notice = None;
    }

    /// Relabels the tab's rows in place, keeping the cursor and scroll.
    fn relabel(&mut self, config: &Config) {
        self.menu
            .set_lines(tab_lines(self.face.tab(), config, self.binding_slot));
    }

    /// The tab step a frame's keys ask for. Menus receive the tab
    /// actions as Q and E; the binding editor reads raw keys, where the
    /// player's own tab chords count too.
    fn tab_key(
        &self,
        events: &[RawEvent],
        bindings: &BindingMap,
        ctrl0: bool,
        shift0: bool,
    ) -> Option<isize> {
        let (mut ctrl, mut shift) = (ctrl0, shift0);
        for event in events {
            match *event {
                RawEvent::KeyDown { key: Key::Ctrl } => ctrl = true,
                RawEvent::KeyUp { key: Key::Ctrl } => ctrl = false,
                RawEvent::KeyDown { key: Key::Shift } => shift = true,
                RawEvent::KeyUp { key: Key::Shift } => shift = false,
                RawEvent::KeyDown { key: Key::Q } => return Some(-1),
                RawEvent::KeyDown { key: Key::E } => return Some(1),
                RawEvent::KeyDown { key } if self.edits_bindings() => {
                    match bindings.resolve_in(key, ctrl, shift, Context::Menu) {
                        Some(Action::PrevTab) => return Some(-1),
                        Some(Action::NextTab) => return Some(1),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Draws the tab's menu, the strip, and the status line — the caller
    /// draws the veil first, so all land above it.
    pub fn draw(&self, mouse: Vec2) {
        if let Some(form) = &self.naming {
            form.draw("", crate::theme::TEXT_SECONDARY, mouse);
            return;
        }
        if let Face::Confirming(profile) = self.face {
            // A dialog: no tabs to wander off to while it asks.
            self.menu.draw(profile.consequence());
            return;
        }
        self.menu.draw_with_coaching("", self.coaching());
        let keys = crate::tabs::keys().filter(|_| crate::platform::hands().keys);
        let labels = Tab::ALL.map(Tab::label);
        let active = Tab::ALL
            .iter()
            .position(|tab| *tab == self.face.tab())
            .unwrap_or_default();
        crate::tabs::draw(
            &labels,
            active,
            mouse,
            keys.as_ref()
                .map(|(prev, next)| (prev.as_str(), next.as_str())),
        );
        if let Some((text, danger)) = self.status() {
            let s = render::ui_scale();
            let size = crate::theme::Type::Body.at(s);
            let width = measure_text(text, None, numeric::font_size(size), 1.0).width;
            draw_text(
                text,
                (render::viewport().x - width) * 0.5,
                render::viewport().y - 48.0 * s,
                size,
                if danger {
                    crate::theme::TEXT_DANGER
                } else {
                    crate::theme::TEXT_BODY
                },
            );
        }
    }

    /// Applies a frame's events, editing the config's binding map, the
    /// only one; `ctrl0`/`shift0` are the frame-start modifier truth a
    /// chord capture replays from.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
        config: &mut Config,
        ctrl0: bool,
        shift0: bool,
    ) -> Update {
        let mut update = Update {
            out: Out::Stay,
            dirty: false,
        };
        // The BACK button steps out exactly as Escape does, capture
        // included: it cancels a rebind without binding anything.
        let (back, routed) = self.back.route(events);
        let events = routed.as_slice();
        // The strip hears the pointer only over a tab's rows. A press it
        // armed must not outlive them: lifted unseen, its finger would
        // keep every later finger off the tabs.
        if self.naming.is_some() || !matches!(self.face, Face::Tab(_)) {
            self.tabs.cancel();
        }
        if self.naming.is_some() {
            if back {
                sounds.push((SoundKind::Click, None));
            }
            update.dirty = self.update_naming(events, back, sounds, config);
            return update;
        }
        if back {
            sounds.push((SoundKind::Click, None));
            match self.face {
                Face::Capturing(_) => {
                    self.face = Face::Tab(Tab::Controls);
                    self.relabel(config);
                }
                Face::Confirming(profile) => self.end_confirming(profile, config),
                Face::Tab(_) => update.out = Out::Leave,
            }
            return update;
        }
        if let Face::Capturing(target) = self.face {
            self.capture(target, events, sounds, config, ctrl0, shift0, &mut update);
            return update;
        }
        if let Face::Confirming(profile) = self.face {
            self.confirm(profile, events, mouse, sounds, config, &mut update);
            return update;
        }
        let tab = self.face.tab();
        let (picked, routed) = self.tabs.route(events, Tab::ALL.len());
        let events = routed.as_slice();
        let next = picked.map(|index| Tab::ALL[index]).or_else(|| {
            self.tab_key(events, &config.bindings, ctrl0, shift0)
                .map(|delta| tab.stepped(delta))
        });
        if let Some(next) = next {
            sounds.push((SoundKind::Click, None));
            if next != tab {
                self.show(next, config);
            }
            return update;
        }
        if events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }))
        {
            update.out = Out::Leave;
            return update;
        }
        // Left and Right move a focused value; on binding rows they
        // choose the chord column, as Tab does.
        let sideways = events.iter().find_map(|event| match event {
            RawEvent::KeyDown { key: Key::Tab } if tab == Tab::Controls => Some((Nav::Right, true)),
            RawEvent::KeyDown {
                key: Key::Left | Key::Right,
            } => Nav::decode(event).map(|nav| (nav, false)),
            _ => None,
        });
        if let Some((nav, column_only)) = sideways {
            let step = if nav == Nav::Left {
                Step::Back
            } else {
                Step::Forward
            };
            match self.menu.value().copied() {
                Some(Item::Setting(row)) if !column_only => {
                    let before = config.clone();
                    if step_setting(config, row, step) && *config != before {
                        sounds.push((SoundKind::Click, None));
                        self.notice = None;
                        update.dirty = true;
                        self.relabel(config);
                    }
                }
                _ if tab == Tab::Controls => {
                    if let Some(slot) =
                        step_line(2, self.binding_slot, nav, Axis::Horizontal, 1, |_| true)
                    {
                        self.binding_slot = slot;
                        self.relabel(config);
                    }
                }
                _ => {}
            }
            return update;
        }
        if tab == Tab::Controls
            && events
                .iter()
                .any(|e| matches!(e, RawEvent::KeyDown { key: Key::X }))
            && let Some(&Item::Binding(target)) = self.menu.value()
        {
            // X on a binding row unbinds it (outside capture, so the key
            // is free to mean this). Like any action, it retires the
            // standing notice.
            config.bindings.unbind_slot(target, self.binding_slot);
            self.notice = None;
            update.dirty = true;
            self.relabel(config);
            return update;
        }
        if let Some(picked) = self.menu.handle(events, mouse) {
            sounds.push((SoundKind::Click, None));
            // Any activation clears the standing notice.
            self.notice = None;
            // A click on a chord chooses its column.
            if let Some(slot) = picked.column {
                self.binding_slot = slot;
            }
            match picked.value {
                Item::Setting(row) => self.activate(row, config, &mut update),
                Item::Binding(action) => {
                    self.face = Face::Capturing(action);
                    // The brackets mark the chord being captured.
                    self.relabel(config);
                }
                Item::ResetBindings => self.ask(Profile::Classic),
                // Only the dialog offers these, and it is not up.
                Item::Cancel | Item::Confirm => {}
            }
        }
        update
    }

    /// Asks before `profile` replaces every binding.
    fn ask(&mut self, profile: Profile) {
        self.face = Face::Confirming(profile);
        self.menu = confirm_menu(profile);
    }

    /// Runs the dialog asking about `profile` for a frame. Cancel, the
    /// BACK button, and Escape all leave the bindings be; as in the pause
    /// menu's dialogs, Escape outranks a row picked in the same frame.
    fn confirm(
        &mut self,
        profile: Profile,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
        config: &mut Config,
        update: &mut Update,
    ) {
        let escaped = events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }));
        match self.menu.handle(events, mouse).map(|picked| picked.value) {
            _ if escaped => self.end_confirming(profile, config),
            Some(Item::Confirm) => {
                sounds.push((SoundKind::Click, None));
                config.bindings = profile.bindings();
                update.dirty = true;
                self.end_confirming(profile, config);
                self.notice = Some(Notice {
                    text: profile.applied().to_string(),
                    danger: false,
                });
            }
            Some(_) => {
                sounds.push((SoundKind::Click, None));
                self.end_confirming(profile, config);
            }
            None => {}
        }
    }

    /// Closes the dialog, back on the Controls row that opened it.
    fn end_confirming(&mut self, profile: Profile, config: &Config) {
        self.face = Face::Tab(Tab::Controls);
        self.menu = tab_menu(Tab::Controls, config, self.binding_slot);
        self.menu.select_where(|item| *item == profile.item());
    }

    /// Activates a settings row: cycles its value, or does what it says.
    fn activate(&mut self, row: Row, config: &mut Config, update: &mut Update) {
        if step_setting(config, row, Step::Cycle) {
            // Apply live, persist, keep the cursor on the row being tuned.
            update.dirty = true;
            self.relabel(config);
            return;
        }
        match row {
            // The left-handed preset replaces the whole profile, custom
            // rebinds included; the reset row walks back to Classic.
            Row::LeftHandedPreset => self.ask(Profile::LeftHanded),
            Row::OpenDiagnostics => update.out = Out::OpenDiagnostics,
            Row::ExportDiagnostics => update.out = Out::ExportDiagnostics,
            Row::PlayerName => self.begin_naming(config),
            _ => {}
        }
    }

    /// Listens for the chord to bind to `target`. The next key is the
    /// answer, read raw before any binding resolution so its current
    /// meaning does not fire. The frame's edges replay in order from the
    /// frame-start baseline, and the modifiers are read at the main key's
    /// press: batch-final flags would miss a chord whose Ctrl came back up
    /// later the same frame.
    #[allow(clippy::too_many_arguments)]
    fn capture(
        &mut self,
        target: Action,
        events: &[RawEvent],
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
        config: &mut Config,
        ctrl0: bool,
        shift0: bool,
        update: &mut Update,
    ) {
        let mut walk_ctrl = ctrl0;
        let mut walk_shift = shift0;
        let mut pressed: Option<(Key, bool, bool)> = None;
        for e in events {
            match e {
                RawEvent::KeyDown { key: Key::Ctrl } => walk_ctrl = true,
                RawEvent::KeyUp { key: Key::Ctrl } => walk_ctrl = false,
                RawEvent::KeyDown { key: Key::Shift } => walk_shift = true,
                RawEvent::KeyUp { key: Key::Shift } => walk_shift = false,
                RawEvent::KeyDown { key } if pressed.is_none() => {
                    pressed = Some((*key, walk_ctrl, walk_shift));
                }
                _ => {}
            }
        }
        match pressed {
            Some((Key::Escape, false, false)) if target != Action::Back => {
                self.face = Face::Tab(Tab::Controls);
                self.relabel(config);
            }
            Some((key, ctrl, shift)) => {
                let chord = Chord { key, ctrl, shift };
                self.face = Face::Tab(Tab::Controls);
                if config
                    .bindings
                    .rebind_slot(target, self.binding_slot, chord)
                {
                    update.dirty = true;
                } else {
                    // Refused: name the holder, so the player knows which
                    // row to unbind first.
                    let text = match config.bindings.conflict(target, self.binding_slot, chord) {
                        Some(holder) => format!(
                            "{} is already bound to {}",
                            BindingMap::chord_label(chord),
                            holder.label()
                        ),
                        None => "that key already means something".to_string(),
                    };
                    self.notice = Some(Notice { text, danger: true });
                    sounds.push((SoundKind::Denied, None));
                }
                self.relabel(config);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
