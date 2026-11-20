//! The windowless session server: the full debug protocol with no GPU,
//! no window, and no wall clock.
//!
//! One [`Session`] holds a match open the way the shell's `Game` does (sim
//! state, seat bots, an always-on recorder, and staged socket commands),
//! but its screenshots come from the CPU renderer. It serves the same
//! [`aftermine_protocol`] vocabulary over the same framed transport
//! ([`aftermine_protocol::framing`]), so every `driver live` verb works against
//! it unchanged; requests that need a window (camera, UI, input injection,
//! the overlay) or a wall clock (pause, resume, speed) are refused with a
//! reason rather than faked.
//!
//! `driver/tests/session_parity.rs` drives the same script through this
//! server and the live shell and asserts hash, event, and status identity;
//! its headless half runs in CI.
//!
//! A [`Playtest`] session is played by one seat against bots, as a person
//! plays a New Match. It answers only what that seat could know and do: its
//! own fog view and commands, and an advancing clock. It refuses everything
//! omniscient, every other seat, and every file request, refuses an order the
//! game would reject with the game's reason, and writes its own replay after
//! each advance.

use aftermine_kit::GameReplay;
use aftermine_kit::controller::{SeatController, record_events, seat_controllers};
use aftermine_kit::runner;
use aftermine_protocol::framing::{IncomingRequest, Limits, incoming};
use aftermine_protocol::{
    AdvancedView, DebugSession, PresentedView, Reply, Request, ResponseEnvelope, SavedView,
    ScreenshotView, StatusView, dispatch_shared, hash_hex,
};
use aftermine_sim::{Event, PlayerCommand, PlayerId, SIM_VERSION, Scenario, State};
use anyhow::{Context, Result};
use chassis::replay::Replay;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One seat's terms for a playtest session.
#[derive(Clone, Debug)]
pub struct Playtest {
    /// The seat the client plays; the session refuses every other seat.
    pub seat: PlayerId,
    /// The tick the match stops at, if any.
    pub tick_limit: Option<u64>,
    /// Where the session rewrites its replay after every advance. The
    /// client cannot save or read a replay through the protocol, since a
    /// replay holds every seat's commands.
    pub replay_out: PathBuf,
}

/// A headless match a debug client can drive: the same session shape as
/// the shell's `Game`, minus everything presentational.
pub struct Session {
    scenario: Scenario,
    state: State,
    bots: Vec<SeatController>,
    recorder: GameReplay,
    /// Socket commands staged for the next tick, as a paused shell stages
    /// them.
    pending: Vec<PlayerCommand>,
    /// The one seat a playtest client plays, when this is a playtest.
    playtest: Option<Playtest>,
}

impl Session {
    /// Opens a session on a scenario.
    pub fn new(scenario: Scenario) -> Result<Self> {
        let state = scenario.build().context("building scenario")?;
        let bots = seat_controllers(&scenario).context("building public bot map briefing")?;
        let recorder = Replay::new(
            SIM_VERSION,
            crate::build_identity().label(),
            scenario.clone(),
        );
        Ok(Self {
            scenario,
            state,
            bots,
            recorder,
            pending: Vec::new(),
            playtest: None,
        })
    }

    /// Opens a playtest: `playtest.seat` is the client's, and every
    /// request beyond what that seat could know or do is refused.
    pub fn playtest(scenario: Scenario, playtest: Playtest) -> Result<Self> {
        anyhow::ensure!(
            !scenario
                .players
                .get(usize::from(playtest.seat.0))
                .with_context(|| format!("{} has no seat {}", scenario.name, playtest.seat.0))?
                .bot,
            "seat {} is a bot's; a playtest client plays a seat no bot does",
            playtest.seat.0
        );
        Ok(Self {
            playtest: Some(playtest),
            ..Self::new(scenario)?
        })
    }

    /// Resumes a session from a scenario-origin replay as the shell does:
    /// validate it, rebuild the scenario, re-execute every recorded tick,
    /// and keep recording. A replay from another sim version is refused,
    /// since commands recorded onto it could not reproduce.
    pub fn resume(replay: GameReplay) -> Result<Self> {
        let aftermine_kit::Resumed { state, bots } = aftermine_kit::resume(&replay, |_, _| {})?;
        Ok(Self {
            scenario: replay.setup.clone(),
            state,
            bots,
            recorder: replay,
            pending: Vec::new(),
            playtest: None,
        })
    }

    /// The current world, read-only (tests compare hashes through this).
    pub fn state(&self) -> &State {
        &self.state
    }

    /// One tick in the shell's `Game::do_tick` order: staged commands, then
    /// bot commands, all recorded, then the sim steps. Any other order
    /// breaks parity with the shell.
    fn step(&mut self) -> Vec<Event> {
        let mut commands = std::mem::take(&mut self.pending);
        for bot in &mut self.bots {
            commands.extend(bot.act(&self.state));
        }
        for command in &commands {
            self.recorder
                .record(self.state.current_tick(), command.clone());
        }
        let report = self.state.tick(&commands);
        record_events(&mut self.bots, &report);
        report.events
    }

    /// Answers one request. Shared requests (state reads, the driven
    /// clock) go through the protocol dispatcher; every other method is
    /// implemented here or refused with a reason.
    pub fn handle(&mut self, request: Request) -> Result<Reply, String> {
        if let Some(playtest) = &self.playtest {
            if let Some(refusal) = playtest_refusal(playtest.seat, &request) {
                return Err(refusal);
            }
            match request {
                Request::AdvanceTicks { ticks } => return self.playtest_advance(ticks),
                Request::SendCommand { player, command } => {
                    return self.playtest_send(PlayerCommand { player, command });
                }
                _ => {}
            }
        }
        if let Some(outcome) = dispatch_shared(self, &request) {
            return outcome;
        }
        match request {
            Request::SendCommand { player, command } => {
                if (player.0 as usize) < self.state.players().len() {
                    self.pending.push(PlayerCommand { player, command });
                    Ok(Reply::Ok)
                } else {
                    Err(format!("no such player {player}"))
                }
            }
            Request::Screenshot { path } => {
                let path = path.unwrap_or_else(|| {
                    format!("screenshots/tick-{}.png", self.state.current_tick())
                });
                self.screenshot(&path)
                    .map_err(|err| format!("screenshot: {err:#}"))
            }
            Request::LoadScenario { path } => Scenario::load(&path)
                .map_err(|err| format!("loading {path}: {err}"))
                .and_then(|scenario| {
                    Session::new(scenario).map_err(|err| format!("building scenario: {err:#}"))
                })
                .map(|fresh| {
                    *self = fresh;
                    Reply::Ok
                }),
            Request::LoadReplay { path } => aftermine_kit::load_replay(&path)
                .map_err(|err| format!("loading replay {path}: {err}"))
                .and_then(|replay| {
                    Session::resume(replay).map_err(|err| format!("resuming replay: {err:#}"))
                })
                .map(|fresh| {
                    *self = fresh;
                    Reply::Status(self.status())
                }),
            Request::SaveReplay { path } => match self.save_replay(Path::new(&path)) {
                Ok(()) => Ok(Reply::Saved(SavedView {
                    path,
                    commands: self.recorder.commands.len(),
                })),
                Err(err) => Err(format!("saving replay: {err:#}")),
            },
            // Each refusal names what is missing and, where one exists, the
            // headless way to get the same information.
            Request::QueryCamera => Err(
                "the headless session has no window or camera; its screenshots \
                 render the whole map"
                    .to_string(),
            ),
            Request::QueryUi => {
                Err("the headless session has no window or menus to report".to_string())
            }
            Request::QueryPerformance { .. }
            | Request::BeginPerformanceWindow { .. }
            | Request::RunFrameScript { .. } => Err(
                "the headless session has no native window or GPU frame loop to profile"
                    .to_string(),
            ),
            Request::InjectEvent { .. } => Err(
                "the headless session has no input funnel; issue game commands \
                 with send_command"
                    .to_string(),
            ),
            Request::ToggleOverlay => Err(
                "the headless session has no renderer overlay; query_state is \
                 already omniscient"
                    .to_string(),
            ),
            // Shared requests were answered above. Listing them keeps this
            // match exhaustive, so a new protocol request forces a decision
            // about which side answers it.
            Request::Status
            | Request::QueryState { .. }
            | Request::QueryFogView { .. }
            | Request::StateHash
            | Request::AdvanceTicks { .. }
            | Request::PresentTicks { .. }
            | Request::Pause
            | Request::Resume
            | Request::SetSpeed { .. } => {
                unreachable!("shared requests are answered by dispatch_shared")
            }
        }
    }

    /// Writes the recording so far to `path`, creating its directory.
    fn save_replay(&mut self, path: &Path) -> Result<()> {
        self.recorder.meta.ticks = Some(self.state.current_tick());
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        self.recorder
            .save(path)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// A playtest's advance: up to the tick limit, then the replay is
    /// rewritten whole, through a temporary file, so a session killed at
    /// any moment leaves its last complete replay.
    fn playtest_advance(&mut self, ticks: u64) -> Result<Reply, String> {
        let Some(playtest) = self.playtest.clone() else {
            unreachable!("only a playtest advances through here")
        };
        let tick = self.state.current_tick();
        let mut ticks = ticks.min(aftermine_protocol::MAX_ADVANCE_TICKS);
        if let Some(limit) = playtest.tick_limit {
            if tick >= limit {
                return Err(format!(
                    "the playtest ended at its tick limit, {limit}; the match stops there"
                ));
            }
            ticks = ticks.min(limit - tick);
        }
        let view = DebugSession::advance(self, ticks);
        let partial = playtest.replay_out.with_extension("partial");
        self.save_replay(&partial)
            .and_then(|()| {
                std::fs::rename(&partial, &playtest.replay_out)
                    .with_context(|| format!("replacing {}", playtest.replay_out.display()))
            })
            .map_err(|err| format!("writing the playtest replay: {err:#}"))?;
        Ok(Reply::Advanced(view))
    }

    /// A playtest's order: staged when the game will take it, and otherwise
    /// refused with the game's reason, as the shell tells its player.
    /// Commands apply in order before anything else in a tick, and a
    /// rejected one changes nothing, so every order already staged was
    /// taken and a trial tick on a copy of the world reaches the next
    /// tick's verdict on this one.
    fn playtest_send(&mut self, order: PlayerCommand) -> Result<Reply, String> {
        let mut trial = self.pending.clone();
        trial.push(order.clone());
        let report = self.state.clone().tick(&trial);
        let rejection = report.events.iter().find_map(|event| match event {
            Event::CommandRejected { player, reason } if *player == order.player => Some(*reason),
            _ => None,
        });
        if let Some(reason) = rejection {
            let reason = serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(|name| name.replace('_', " ")))
                .unwrap_or_else(|| format!("{reason:?}"));
            return Err(format!(
                "the game rejects this order ({reason}); nothing was sent"
            ));
        }
        self.pending.push(order);
        Ok(Reply::Ok)
    }

    fn screenshot(&self, path: &str) -> Result<Reply> {
        let pixmap = aftermine_kit::render::render_state(&self.state);
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        pixmap
            .save_png(path)
            .with_context(|| format!("writing {path}"))?;
        Ok(Reply::Screenshot(ScreenshotView {
            path: path.to_string(),
            width: pixmap.width(),
            height: pixmap.height(),
            // The schematic CPU render, not what the shell draws; reported
            // so a client never judges visual polish from it.
            renderer: "cpu".to_string(),
        }))
    }
}

/// Why a playtest client may not make `request`, or `None` when it may.
/// A playtest answers only what `seat` could know and do in a match: its
/// own fog view and commands, the status, and an advancing clock.
fn playtest_refusal(seat: PlayerId, request: &Request) -> Option<String> {
    let own_seat_only = |player: PlayerId| {
        (player != seat).then(|| {
            format!(
                "a playtest client plays seat {} only; seat {} is not yours",
                seat.0, player.0
            )
        })
    };
    match request {
        Request::QueryFogView { player } | Request::SendCommand { player, .. } => {
            own_seat_only(*player)
        }
        Request::QueryState { .. } | Request::StateHash | Request::Screenshot { .. } => {
            Some(format!(
                "a playtest shows only what your seat sees; query_fog_view for seat {}",
                seat.0
            ))
        }
        Request::PresentTicks { .. } => Some(
            "a playtest's events cover every seat; advance_ticks and query_fog_view instead"
                .to_string(),
        ),
        Request::LoadScenario { .. } | Request::LoadReplay { .. } | Request::SaveReplay { .. } => {
            Some(
                "a playtest plays one match and writes its own replay; it reads and saves no files"
                    .to_string(),
            )
        }
        Request::Status
        | Request::AdvanceTicks { .. }
        | Request::Pause
        | Request::Resume
        | Request::SetSpeed { .. }
        | Request::QueryCamera
        | Request::QueryUi
        | Request::QueryPerformance { .. }
        | Request::BeginPerformanceWindow { .. }
        | Request::RunFrameScript { .. }
        | Request::InjectEvent { .. }
        | Request::ToggleOverlay => None,
    }
}

const NO_CLOCK: &str = "the headless session has no wall clock; sim time moves only through \
     advance_ticks and present_ticks";

/// The clockless debug session: always in driven mode, so pause and speed
/// are refused. With no transient effects to age, `present` advances and
/// returns the events, the same sim result as the shell's presented step.
impl DebugSession for Session {
    fn status(&self) -> StatusView {
        StatusView {
            tick: self.state.current_tick(),
            // Sim time moves only on request, which reports as paused, as a
            // driven-mode shell does.
            paused: true,
            speed: 1.0,
            scenario: self.scenario.name.clone(),
            sim_version: SIM_VERSION,
            result: self.state.result(),
            // A playtest client learns nothing of the bots' commands.
            recorded_commands: match &self.playtest {
                Some(playtest) => self
                    .recorder
                    .commands
                    .iter()
                    .filter(|timed| timed.command.player == playtest.seat)
                    .count(),
                None => self.recorder.commands.len(),
            },
        }
    }

    fn state(&self) -> &State {
        &self.state
    }

    fn advance(&mut self, ticks: u64) -> AdvancedView {
        for _ in 0..ticks {
            self.step();
        }
        AdvancedView {
            ticks,
            tick: self.state.current_tick(),
            hash: hash_hex(self.state.hash()),
        }
    }

    fn present(&mut self, ticks: u64) -> PresentedView {
        let mut events = Vec::new();
        for _ in 0..ticks {
            events.extend(self.step());
        }
        PresentedView {
            ticks,
            tick: self.state.current_tick(),
            hash: hash_hex(self.state.hash()),
            events,
        }
    }

    fn set_paused(&mut self, _paused: bool) -> Result<(), String> {
        Err(NO_CLOCK.to_string())
    }

    fn set_speed(&mut self, _multiplier: f64) -> Result<(), String> {
        Err(NO_CLOCK.to_string())
    }
}

/// How `aftermine-driver session --player` seats its match: the client's
/// seat and terms, and the bot every other seat gets.
#[derive(Clone, Debug)]
pub struct PlaytestSetup {
    /// The client's seat, tick limit and replay path.
    pub playtest: Playtest,
    /// Every other seat's skill rung.
    pub difficulty: aftermine_sim::scenario::BotDifficulty,
    /// Every other seat's posture.
    pub stance: aftermine_sim::scenario::BotStance,
    /// Seat `n`'s personality seed is this plus `n`.
    pub personality_seed_base: u64,
}

/// Binds, announces, and answers until the process ends: the
/// `aftermine-driver session` subcommand.
pub fn serve(
    port: u16,
    scenario: &str,
    idle_timeout: Duration,
    playtest: Option<PlaytestSetup>,
) -> Result<()> {
    let mut scenario = runner::load_scenario(scenario)?;
    let session = match playtest {
        Some(setup) => {
            aftermine_kit::controller::seat_bots_around(
                &mut scenario,
                setup.playtest.seat,
                setup.difficulty,
                setup.stance,
                setup.personality_seed_base,
            )?;
            Session::playtest(scenario, setup.playtest)?
        }
        None => Session::new(scenario)?,
    };
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("binding session server to 127.0.0.1:{port}"))?;
    eprintln!(
        "session server listening on 127.0.0.1:{port} — windowless, \
         sim time moves only on request"
    );
    let limits = Limits {
        idle_timeout,
        ..Limits::default()
    };
    serve_listener(listener, limits, session);
    Ok(())
}

/// The answering loop over an already-bound listener; tests bind port 0
/// and drive it on a thread. Requests from every connection funnel through
/// one channel, so each response reflects a settled world, as the shell
/// answers between frames.
pub fn serve_listener(listener: TcpListener, limits: Limits, mut session: Session) {
    let rx = incoming(listener, limits);
    for IncomingRequest { id, request, reply } in rx {
        let response = match session.handle(request) {
            Ok(ok) => ResponseEnvelope::ok(id, ok),
            Err(err) => ResponseEnvelope::err(id, err),
        };
        reply.send(response).ok();
    }
}

#[cfg(test)]
mod tests;
