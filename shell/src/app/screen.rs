//! The screens and what each one is: every per-screen fact the frame loop,
//! the debug socket and the audio frame ask about lives here, answered by
//! one exhaustive match apiece so a new screen cannot fall into a default.

use super::persistence;
use crate::game::{Game, Presentation};
use crate::screens::codex::CodexScreen;
use crate::screens::cold_open::ColdOpenScreen;
use crate::screens::final_map::FinalMapScreen;
use crate::screens::home::HomeScreen;
use crate::screens::lessons::LessonsScreen;
use crate::screens::lobby::LobbyScreen;
use crate::screens::pause::PauseScreen;
use crate::screens::playback::PlaybackSession;
use crate::screens::results::ResultsScreen;
use crate::screens::settings::SettingsScreen;
use crate::screens::shelf::Shelf;
use crate::screens::waiting::WaitingScreen;
use crate::screens::wizard::Wizard;
use crate::text_field::Keyboard;

/// Which screen owns input this frame, holding that screen's state.
/// Match choices live in the [`crate::screens::wizard::NewMatchDraft`] on
/// [`super::App`], not in a screen.
pub(super) enum Screen {
    /// The report shown once, before the first Home.
    ColdOpen(ColdOpenScreen),
    /// The front door: play, settings, quit.
    Home(HomeScreen),
    /// Settings and the Controls remap screen. `back` is where leaving
    /// returns to — Home, or the untouched pause menu whose payload
    /// waits here intact so the cursor comes back to the row that
    /// opened this screen.
    Settings {
        /// The screen itself.
        screen: SettingsScreen,
        /// The displaced screen, restored wholesale on leave.
        back: Box<Screen>,
    },
    /// The codex of machines and works, opened over Home or the paused
    /// match, which waits in `back` as it does under Settings.
    Codex {
        /// The screen itself.
        screen: CodexScreen,
        /// The displaced screen, restored wholesale on leave.
        back: Box<Screen>,
    },
    /// Basic Training's lesson list, opened from Home or over a paused
    /// lesson, which waits in `back` as it does under Settings.
    Lessons {
        /// The screen itself.
        screen: LessonsScreen,
        /// The displaced screen, restored wholesale on leave.
        back: Box<Screen>,
    },
    /// The New Match wizard (map grid, then match setup).
    Wizard(Wizard),
    /// A LAN match gathering its machines. Leaving drops every
    /// connection the lobby holds.
    Lobby {
        /// The screen itself.
        screen: Box<LobbyScreen>,
        /// Where leaving returns to: Home, or the match setup that hosted.
        back: Box<Screen>,
    },
    /// The game proper. The session lives in [`super::App::game`], which
    /// every screen needs as its backdrop.
    Playing,
    /// Read-only replay playback: the log is the match, seek included.
    Playback {
        /// Boxed: the session carries a whole presentation, and the enum
        /// should not make every other screen pay its size.
        session: Box<PlaybackSession>,
        /// The screen that opened the viewer, restored wholesale on leave.
        back: Box<Screen>,
    },
    /// The saves and replays shelf.
    Replays(Shelf),
    /// Decided-match report and next steps.
    Results(ResultsScreen),
    /// Camera-only inspection of the already-final live state.
    FinalMap(FinalMapScreen),
    /// Game visible but veiled; the pause screen owns input,
    /// confirmation and save-naming state included.
    Pause(PauseScreen),
    /// A LAN match veiled while it waits for a lost player.
    Waiting(WaitingScreen),
    /// A save or load in flight over the screen it displaced.
    Busy(Box<persistence::Busy>),
}

/// A screen's variant without its state, for comparing screens across a
/// frame and for the facts that depend on the variant alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScreenKind {
    ColdOpen,
    Home,
    Settings,
    Codex,
    Lessons,
    Wizard,
    Lobby,
    Playing,
    Playback,
    Replays,
    Results,
    FinalMap,
    Pause,
    Waiting,
    Busy,
}

impl ScreenKind {
    /// Whether the world is the screen rather than a backdrop: gameplay input
    /// reaches it, and menu notices wait for a menu.
    pub(super) const fn gameplay(self) -> bool {
        match self {
            Self::Playing | Self::Playback | Self::FinalMap => true,
            Self::ColdOpen
            | Self::Home
            | Self::Settings
            | Self::Codex
            | Self::Lessons
            | Self::Wizard
            | Self::Lobby
            | Self::Replays
            | Self::Results
            | Self::Pause
            | Self::Waiting
            | Self::Busy => false,
        }
    }

    /// Whether the screen veils the live match it holds, which diagnostics
    /// then report as paused.
    pub(super) const fn veils_match(self) -> bool {
        match self {
            Self::Pause | Self::Waiting => true,
            Self::ColdOpen
            | Self::Home
            | Self::Settings
            | Self::Codex
            | Self::Lessons
            | Self::Wizard
            | Self::Lobby
            | Self::Playing
            | Self::Playback
            | Self::Replays
            | Self::Results
            | Self::FinalMap
            | Self::Busy => false,
        }
    }

    /// Whether the system may act on a controller's presses: only on Home,
    /// the top screen.
    pub(super) const fn top_screen(self) -> bool {
        match self {
            Self::Home => true,
            Self::ColdOpen
            | Self::Settings
            | Self::Codex
            | Self::Lessons
            | Self::Wizard
            | Self::Lobby
            | Self::Playing
            | Self::Playback
            | Self::Replays
            | Self::Results
            | Self::FinalMap
            | Self::Pause
            | Self::Waiting
            | Self::Busy => false,
        }
    }

    /// Whether the soundtrack plays: the report before the first Home is
    /// silent, and the score starts at the cut.
    pub(super) const fn scored(self) -> bool {
        match self {
            Self::ColdOpen => false,
            Self::Home
            | Self::Settings
            | Self::Codex
            | Self::Lessons
            | Self::Wizard
            | Self::Lobby
            | Self::Playing
            | Self::Playback
            | Self::Replays
            | Self::Results
            | Self::FinalMap
            | Self::Pause
            | Self::Waiting
            | Self::Busy => true,
        }
    }

    /// Whether the menu notice line sits over the screen's action buttons
    /// rather than at the foot of the window: on the report, whose buttons
    /// hold the foot.
    pub(super) const fn notice_over_actions(self) -> bool {
        match self {
            Self::Results => true,
            Self::ColdOpen
            | Self::Home
            | Self::Settings
            | Self::Codex
            | Self::Lessons
            | Self::Wizard
            | Self::Lobby
            | Self::Playing
            | Self::Playback
            | Self::Replays
            | Self::FinalMap
            | Self::Pause
            | Self::Waiting
            | Self::Busy => false,
        }
    }

    /// The performance display's context; zero turns it off.
    pub(super) const fn performance_context(self) -> u8 {
        match self {
            Self::Playing => 1,
            Self::Playback => 2,
            Self::FinalMap => 3,
            Self::ColdOpen
            | Self::Home
            | Self::Settings
            | Self::Codex
            | Self::Lessons
            | Self::Wizard
            | Self::Lobby
            | Self::Replays
            | Self::Results
            | Self::Pause
            | Self::Waiting
            | Self::Busy => 0,
        }
    }
}

/// How the live match's effects age under a screen while the frame loop
/// runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EffectAging {
    /// The screen ages them through its own clock, or they hold still.
    Held,
    /// With presentation time: the decided match the report draws behind it.
    Always,
    /// While the match's clock runs: a LAN match ticking under a menu.
    WallClock,
}

/// How a frame script starts on the screen that admits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptStart {
    /// Home: from its backdrop's checkpoint, so every script draws the same
    /// frames however long Home has been open.
    Backdrop,
    /// The live match or the viewer, as they stand.
    Session,
}

/// The session a screen shows: the live match, or a replay viewer's own.
#[derive(Clone, Copy)]
pub(super) enum Visible<'a> {
    Live(&'a Game),
    Replay(&'a PlaybackSession),
}

impl<'a> Visible<'a> {
    pub(super) fn state(self) -> &'a aftermine_sim::State {
        match self {
            Self::Live(game) => &game.state,
            Self::Replay(session) => &session.engine.state,
        }
    }

    pub(super) fn presentation(self) -> &'a Presentation {
        match self {
            Self::Live(game) => &game.presentation,
            Self::Replay(session) => &session.presentation,
        }
    }

    /// The tick on screen.
    pub(super) fn tick(self) -> u64 {
        match self {
            Self::Live(game) => game.state.current_tick(),
            Self::Replay(session) => session.engine.position(),
        }
    }

    pub(super) fn speed(self) -> f64 {
        match self {
            Self::Live(game) => game.clock.speed,
            Self::Replay(session) => session.clock.speed,
        }
    }

    pub(super) fn paused(self) -> bool {
        match self {
            Self::Live(game) => game.clock.paused,
            Self::Replay(session) => session.clock.paused,
        }
    }

    /// The scene on screen.
    pub(super) fn view(self) -> crate::game::Scene<'a> {
        match self {
            Self::Live(game) => game.view(),
            Self::Replay(session) => session.view(),
        }
    }
}

impl Screen {
    pub(super) fn kind(&self) -> ScreenKind {
        match self {
            Self::ColdOpen(_) => ScreenKind::ColdOpen,
            Self::Home(_) => ScreenKind::Home,
            Self::Settings { .. } => ScreenKind::Settings,
            Self::Codex { .. } => ScreenKind::Codex,
            Self::Lessons { .. } => ScreenKind::Lessons,
            Self::Wizard(_) => ScreenKind::Wizard,
            Self::Lobby { .. } => ScreenKind::Lobby,
            Self::Playing => ScreenKind::Playing,
            Self::Playback { .. } => ScreenKind::Playback,
            Self::Replays(_) => ScreenKind::Replays,
            Self::Results(_) => ScreenKind::Results,
            Self::FinalMap(_) => ScreenKind::FinalMap,
            Self::Pause(_) => ScreenKind::Pause,
            Self::Waiting(_) => ScreenKind::Waiting,
            Self::Busy(_) => ScreenKind::Busy,
        }
    }

    /// The session on screen: a replay viewer shows its own, every other
    /// screen shows the live match, as play or as backdrop.
    pub(super) fn visible<'a>(&'a self, live: &'a Game) -> Visible<'a> {
        match self {
            Self::Playback { session, .. } => Visible::Replay(session),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => Visible::Live(live),
        }
    }

    /// The presentation on screen, for requests that steer what the
    /// window shows.
    pub(super) fn visible_presentation<'a>(
        &'a mut self,
        live: &'a mut Game,
    ) -> &'a mut Presentation {
        match self {
            Self::Playback { session, .. } => &mut session.presentation,
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => &mut live.presentation,
        }
    }

    /// The session the debug protocol's shared verbs act on.
    pub(super) fn visible_session<'a>(
        &'a mut self,
        live: &'a mut Game,
    ) -> &'a mut dyn aftermine_protocol::DebugSession {
        match self {
            Self::Playback { session, .. } => &mut **session,
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => live,
        }
    }

    /// Whether this is the pause menu, or a menu it opened that returns to it.
    pub(super) fn over_pause(&self) -> bool {
        match self {
            Self::Pause(_) => true,
            Self::Settings { back, .. } | Self::Codex { back, .. } | Self::Lessons { back, .. } => {
                back.over_pause()
            }
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// How the live match's effects age under this screen. Playing and the
    /// viewer advance their own clocks, and the pause menu freezes the
    /// battlefield behind it; a `lan` match keeps ticking under every menu,
    /// so its effects age there too.
    pub(super) fn effect_aging(&self, lan: bool) -> EffectAging {
        match self {
            Self::Results(_) => EffectAging::Always,
            Self::Playing => EffectAging::Held,
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => {
                if lan {
                    EffectAging::WallClock
                } else {
                    EffectAging::Held
                }
            }
        }
    }

    /// Whether a frame script may run on this screen, and how it starts.
    /// Only the live match may play during one.
    pub(super) fn frame_script(&self, plays: bool) -> Result<ScriptStart, &'static str> {
        const ONLY_LIVE_PLAYS: &str = "only the live match can play during a frame script";
        match self {
            Self::Playing => Ok(ScriptStart::Session),
            Self::Playback { session, .. } if session.engine.filling() => {
                Err("the viewer is still filling in its record; retry shortly")
            }
            Self::Playback { .. } | Self::Home(_) if plays => Err(ONLY_LIVE_PLAYS),
            Self::Playback { .. } => Ok(ScriptStart::Session),
            Self::Home(_) => Ok(ScriptStart::Backdrop),
            Self::ColdOpen(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => Err(if plays {
                ONLY_LIVE_PLAYS
            } else {
                "frame scripts run on Home, the live match or the viewer"
            }),
        }
    }

    /// Whether the binding editor is up: it reads raw keys under the classic
    /// map, so a botched rebind can never lock the player out.
    pub(super) fn edits_bindings(&self) -> bool {
        match self {
            Self::Settings { screen, .. } => screen.edits_bindings(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// Whether the Controls tab is listening for a chord to bind, which only
    /// the keyboard can answer.
    pub(super) fn captures_chord(&self) -> bool {
        match self {
            Self::Settings { screen, .. } => screen.capturing(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// Whether this is a menu built for a match still running, which the
    /// report replaces once a LAN match ends under it.
    pub(super) fn built_for_running_match(&self) -> bool {
        match self {
            Self::Pause(pause) => !pause.decided() && !pause.saving_failed(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// What the soundtrack scores under this screen: the match on screen,
    /// held under a menu or a paused viewer, or the menus' own music.
    pub(super) fn soundtrack_scene(&self, live: &Game) -> crate::soundtrack::Scene {
        use crate::soundtrack::match_scene;
        match self {
            Self::Playing | Self::Results(_) => match_scene(&live.view(), false),
            Self::Playback { session, .. } => match_scene(
                &session.view(),
                session.clock.paused || session.seeking.is_some(),
            ),
            Self::FinalMap(_) | Self::Busy(_) | Self::Pause(_) | Self::Waiting(_) => {
                match_scene(&live.view(), true)
            }
            Self::Settings { .. } | Self::Codex { .. } | Self::Lessons { .. }
                if self.over_pause() =>
            {
                match_scene(&live.view(), true)
            }
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Lobby { .. }
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Replays(_) => crate::soundtrack::Scene::Menu,
        }
    }

    /// Draws a save's `status` on this screen, under the save that covers
    /// it, where the screen has a line for one: the pause menu's own.
    /// Returns whether it drew.
    pub(super) fn draw_saving(&self, sprites: &crate::assets::Sprites, status: &str) -> bool {
        match self {
            Self::Pause(pause) => {
                pause.draw_saving(sprites, status);
                true
            }
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// Whether rocket, warhead and Maw motors run in the session on screen:
    /// a replay playing, or live play the clock runs and no lost controller
    /// holds.
    pub(super) fn motors_run(&self, live: &Game, held: bool) -> bool {
        match self {
            Self::Playback { session, .. } => !session.clock.paused && session.seeking.is_none(),
            Self::Playing => !live.clock.paused && !held,
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// Whether saves the cloud mirror brings in may land now: on the screens
    /// that list them, and under a load still waiting for the list.
    pub(super) fn takes_mirror_arrivals(&self) -> bool {
        match self {
            Self::Home(_) | Self::Replays(_) => true,
            Self::Busy(busy) => busy.awaits_catalog(),
            Self::ColdOpen(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_) => false,
        }
    }

    /// Hands a fresh catalog of saves to the screen that lists them.
    pub(super) fn take_catalog(&mut self, catalog: crate::saves::Catalog) {
        match self {
            Self::Home(home) => {
                // A rescan that changes nothing leaves the menu, and any
                // press on it, alone.
                let resumable = catalog.continuable().next().is_some();
                if !home.catalog_ready || home.resumable() != resumable {
                    home.set_catalog(resumable);
                }
            }
            Self::Replays(shelf) => shelf.set_catalog(catalog.entries, catalog.downloading),
            Self::ColdOpen(_)
            | Self::Settings { .. }
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => {}
        }
    }

    /// Delivers a notice to the screen that is up: the HUD's toast strip in
    /// live play or a replay, the Settings and pause menus' own notice
    /// lines, and the shared menu line everywhere else, including the
    /// waiting screen, which covers the HUD. The final map has no running
    /// clock to age a toast, so its notices wait on the menu line for the
    /// report it returns to.
    pub(super) fn deliver_notice(
        &mut self,
        live: &mut Game,
        menu_notice: &mut Option<super::screen_flow::MenuNotice>,
        super::Notice { text, danger }: super::Notice,
    ) {
        match self {
            Self::Playing => live.presentation.toast(text),
            Self::Playback { session, .. } => session.presentation.toast(text),
            Self::Settings { screen, .. } => {
                screen.notice = Some(crate::screens::settings::Notice { text, danger });
            }
            Self::Pause(pause) => pause.show_notice(text),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Wizard(_)
            | Self::Lobby { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => {
                *menu_notice = Some(super::screen_flow::MenuNotice::new(text));
            }
        }
    }

    /// Whether closing the window would abandon a live match behind this
    /// screen. Only then does a quit save the match before exiting (see
    /// `persistence::Intent::quit`), and a LAN match keeps its link. A save
    /// or load in flight keeps the match.
    pub(super) fn holds_live_match(&self) -> bool {
        match self {
            Self::Playing
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Pause(_)
            | Self::Waiting(_)
            | Self::Busy(_) => true,
            Self::Settings { .. } | Self::Codex { .. } | Self::Lessons { .. } => self.over_pause(),
            Self::Playback { back, .. } => back.holds_live_match(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Wizard(_)
            | Self::Replays(_)
            | Self::Lobby { .. } => false,
        }
    }

    /// Whether the screen should stay on: a local match or a replay
    /// playing, or an undecided LAN match, which runs under every screen
    /// that holds it and whose other seats would wait on a sleeping device.
    /// A local match `held` for want of a controller runs nothing. Menus over
    /// a local match, a paused one and the front door leave dimming and
    /// sleep to the system.
    pub(super) fn keeps_screen_awake(&self, live: &Game, held: bool) -> bool {
        let lan = live.net_role().is_some() && live.state.result().is_none();
        match self {
            Self::Playing => lan || (!live.clock.paused && !held),
            Self::Playback { session, back } => {
                (!session.clock.paused && !session.engine.at_end())
                    || (lan && back.holds_live_match())
            }
            Self::Pause(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => lan,
            Self::Settings { .. } | Self::Codex { .. } | Self::Lessons { .. } => {
                lan && self.over_pause()
            }
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Wizard(_)
            | Self::Replays(_)
            | Self::Lobby { .. } => false,
        }
    }

    /// The keyboard a text field on screen asks for: the save-name field,
    /// the player-name field, or the Join form.
    pub(super) fn keyboard(&self) -> Option<Keyboard> {
        match self {
            Self::Pause(pause) => pause.keyboard(),
            Self::Lobby { screen, .. } => screen.keyboard(),
            Self::Settings { screen, .. } => screen.keyboard(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Wizard(_)
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => None,
        }
    }

    /// The text the on-screen keyboard should show for the field on screen.
    pub(super) fn keyboard_text(&self) -> Option<&str> {
        match self {
            Self::Pause(pause) => pause.keyboard_text(),
            Self::Lobby { screen, .. } => screen.keyboard_text(),
            Self::Settings { screen, .. } => screen.keyboard_text(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Wizard(_)
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => None,
        }
    }

    /// Whether a text field owns input.
    pub(super) fn text_entry(&self) -> bool {
        self.keyboard().is_some()
    }

    /// Whether the text field on screen asked to bring back the on-screen
    /// keyboard this frame.
    pub(super) fn take_keyboard_request(&mut self) -> bool {
        match self {
            Self::Pause(pause) => pause.take_keyboard_request(),
            Self::Lobby { screen, .. } => screen.take_keyboard_request(),
            Self::Settings { screen, .. } => screen.take_keyboard_request(),
            Self::ColdOpen(_)
            | Self::Home(_)
            | Self::Wizard(_)
            | Self::Codex { .. }
            | Self::Lessons { .. }
            | Self::Playing
            | Self::Playback { .. }
            | Self::Replays(_)
            | Self::Results(_)
            | Self::FinalMap(_)
            | Self::Waiting(_)
            | Self::Busy(_) => false,
        }
    }

    /// The diagnostics' coarse name for the screen.
    pub(super) fn profile_mode(&self) -> &'static str {
        match self {
            Self::ColdOpen(_) => "cold_open",
            Self::Home(_) => "home",
            Self::Settings { .. } => "settings",
            Self::Codex { .. } => "codex",
            Self::Lessons { .. } => "lessons",
            Self::Wizard(_) => "wizard",
            Self::Lobby { .. } => "lobby",
            Self::Playing => "playing",
            Self::Playback { .. } => "playback",
            Self::FinalMap(_) => "final_map",
            Self::Results(_) => "results",
            Self::Replays(_) => "replays",
            Self::Pause(_) => "pause",
            Self::Waiting(_) => "waiting",
            Self::Busy(busy) => busy.mode(),
        }
    }

    /// The debug protocol's stable name for what the player is looking at,
    /// which also keys the coaching clock.
    pub(super) fn mode(&self) -> &'static str {
        match self {
            Self::ColdOpen(_) => "cold_open",
            Self::Home(home) => home.mode_name(),
            Self::Settings { screen, .. } => screen.mode_name(),
            Self::Codex { .. } => "codex",
            Self::Lessons { .. } => "lessons",
            Self::Wizard(wizard) => wizard.mode_name(),
            Self::Playing => "playing",
            Self::Playback { .. } => "playback",
            Self::FinalMap(_) => "final_map",
            Self::Results(_) => "results",
            Self::Replays(shelf) => shelf.mode_name(),
            Self::Lobby { screen, .. } => screen.mode_name(),
            Self::Busy(busy) => busy.mode(),
            Self::Pause(pause) => pause.mode_name(),
            Self::Waiting(waiting) => waiting.mode_name(),
        }
    }
}

#[cfg(test)]
mod tests;
