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
    AdvancedView, DebugSession, PlayedView, PresentedView, Reply, Request, ResponseEnvelope,
    SavedView, ScreenshotView, SeenEvent, StatusView, dispatch_shared, hash_hex,
};
use aftermine_sim::command::RejectReason;
use aftermine_sim::{
    BuildingKind, Command, Event, GameResult, PlaceRefusal, PlayerCommand, PlayerId, SIM_VERSION,
    Scenario, State,
};
use anyhow::{Context, Result};
use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;
use chassis::replay::Replay;
use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One seat's terms for a playtest session.
#[derive(Clone, Debug)]
pub struct Playtest {
    /// The seats clients play, one per port; a client may see and command
    /// only its own, and the clock moves once every one has asked it to.
    pub seats: Vec<PlayerId>,
    /// The tick the match stops at, if any.
    pub tick_limit: Option<u64>,
    /// Where the session rewrites its replay after every advance. The
    /// client cannot save or read a replay through the protocol, since a
    /// replay holds every seat's commands.
    pub replay_out: PathBuf,
    /// The fewest ticks one advance may run, short of the tick limit: how
    /// often a player at this pace may look at the game.
    pub min_step: Option<u64>,
    /// The most orders the player may send in any minute of game time.
    pub orders_per_minute: Option<std::num::NonZeroU32>,
}

/// Game ticks in a minute, the window an order rate is counted over.
const MINUTE: u64 = 60 * aftermine_sim::TICKS_PER_SECOND as u64;

/// Trains of one kind at one structure a paced player queues as one order.
const TRAIN_BURST: u32 = 5;

/// What a seat hears when it asks the clock to move before every other
/// player has.
const WAITING: &str = "waiting for the other player to advance; the clock moves once both \
     have asked, by the shorter step. Advance again to keep waiting: you get this step's report";

/// How long a seat's advance waits for the other players before it is told
/// to ask again, well inside the client's read timeout.
const HOLD: Duration = Duration::from_secs(25);

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
    /// The seats playtest clients play, when this is a playtest.
    playtest: Option<Playtest>,
    /// How far each client seat has asked the clock to move since it last
    /// moved.
    asked: BTreeMap<PlayerId, u64>,
    /// Each seat's report of a step that ran while its client was not
    /// waiting on it.
    unread: BTreeMap<PlayerId, PlayedView>,
    /// The seat whose request is being answered, for answers that differ by
    /// seat.
    caller: Option<PlayerId>,
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
            asked: BTreeMap::new(),
            unread: BTreeMap::new(),
            caller: None,
        })
    }

    /// Opens a playtest: `playtest.seat` is the client's, and every
    /// request beyond what that seat could know or do is refused.
    pub fn playtest(scenario: Scenario, playtest: Playtest) -> Result<Self> {
        anyhow::ensure!(!playtest.seats.is_empty(), "a playtest has a seat to play");
        for seat in &playtest.seats {
            anyhow::ensure!(
                !scenario
                    .players
                    .get(usize::from(seat.0))
                    .with_context(|| format!("{} has no seat {}", scenario.name, seat.0))?
                    .bot,
                "seat {} is a bot's; a playtest client plays a seat no bot does",
                seat.0
            );
        }
        // Players who share a match fill it, so none of them is paired with a
        // bot as an ally or left a bot to fight beside.
        anyhow::ensure!(
            playtest.seats.len() == 1 || playtest.seats.len() == scenario.players.len(),
            "{} has {} seats; players who share a match fill every seat",
            scenario.name,
            scenario.players.len()
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
            asked: BTreeMap::new(),
            unread: BTreeMap::new(),
            caller: None,
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
        self.handle_from(None, request)
    }

    /// Answers one request from the client of `seat`, or of the playtest's
    /// first seat when `None`.
    pub fn handle_from(
        &mut self,
        seat: Option<PlayerId>,
        request: Request,
    ) -> Result<Reply, String> {
        self.caller = None;
        if let Some(playtest) = &self.playtest {
            let seat = seat.unwrap_or(playtest.seats[0]);
            if let Some(refusal) = playtest_refusal(seat, &request) {
                return Err(refusal);
            }
            self.caller = Some(seat);
            match request {
                Request::AdvanceTicks { ticks } => {
                    return match self.ask(seat, ticks)? {
                        Some(mut views) => {
                            let at = views.iter().position(|(owner, _)| *owner == seat);
                            let view = at.map(|at| views.swap_remove(at).1);
                            for (owner, view) in views {
                                self.unread.insert(owner, view);
                            }
                            Ok(Reply::Played(view.expect("the asking seat plays")))
                        }
                        None => Err(WAITING.to_string()),
                    };
                }
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

    /// Asks the clock, for `seat`, to move `ticks`: up to the tick limit or
    /// the tick the match ends. Once every client seat has asked, it moves
    /// the shortest of their steps and answers each seat with what it saw;
    /// until then, `None`. Then the replay is rewritten whole, through a
    /// temporary file, so a session killed at any moment leaves its last
    /// complete replay.
    pub fn ask(
        &mut self,
        seat: PlayerId,
        ticks: u64,
    ) -> Result<Option<Vec<(PlayerId, PlayedView)>>, String> {
        let Some(playtest) = self.playtest.clone() else {
            unreachable!("only a playtest asks the clock")
        };
        let ticks = self.step_for(&playtest, seat, ticks)?;
        self.asked.insert(seat, ticks);
        if !playtest
            .seats
            .iter()
            .all(|seat| self.asked.contains_key(seat))
        {
            return Ok(None);
        }
        let ticks = self.asked.values().copied().min().unwrap_or(0);
        self.asked.clear();
        let mut seen: Vec<Seen> = playtest.seats.iter().map(|seat| Seen::new(*seat)).collect();
        let mut ran = 0;
        while ran < ticks && self.state.result().is_none() {
            let at = self.state.current_tick();
            for seen in &mut seen {
                seen.look(&self.state);
            }
            let events = self.step();
            for seen in &mut seen {
                seen.note(at, &self.state, &events);
            }
            ran += 1;
        }
        let partial = playtest.replay_out.with_extension("partial");
        self.save_replay(&partial)
            .and_then(|()| {
                std::fs::rename(&partial, &playtest.replay_out)
                    .with_context(|| format!("replacing {}", playtest.replay_out.display()))
            })
            .map_err(|err| format!("writing the playtest replay: {err:#}"))?;
        Ok(Some(
            seen.into_iter()
                .map(|seen| {
                    let seat = seen.seat;
                    let view = PlayedView {
                        player: seat,
                        team: self.state.player(seat).team,
                        ticks: ran,
                        tick: self.state.current_tick(),
                        income: seen.income,
                        log: seen.log,
                        result: self.state.result(),
                        tick_limit: playtest.tick_limit,
                    };
                    (seat, view)
                })
                .collect(),
        ))
    }

    /// The report of a step that ran while `seat`'s client was not waiting
    /// on it, once.
    pub fn take_unread(&mut self, seat: PlayerId) -> Option<PlayedView> {
        self.unread.remove(&seat)
    }

    /// Keeps `view` for `seat`'s client to collect with its next advance.
    pub fn keep_unread(&mut self, seat: PlayerId, view: PlayedView) {
        self.unread.insert(seat, view);
    }

    /// How far `seat` may move the clock when it asks for `ticks`: refused
    /// once the match is over or at the tick limit, or below the pace's
    /// least step short of the limit.
    fn step_for(&self, playtest: &Playtest, seat: PlayerId, ticks: u64) -> Result<u64, String> {
        let tick = self.state.current_tick();
        if let Some(result) = self.state.result() {
            return Err(format!(
                "the match is over: {}; it ended by tick {tick}",
                self.outcome(seat, result)
            ));
        }
        let mut ticks = ticks.min(aftermine_protocol::MAX_ADVANCE_TICKS);
        let mut left = None;
        if let Some(limit) = playtest.tick_limit {
            if tick >= limit {
                return Err(format!(
                    "the playtest ended at its tick limit, {limit}; the match stops there"
                ));
            }
            left = Some(limit - tick);
            ticks = ticks.min(limit - tick);
        }
        if let Some(least) = playtest.min_step
            && ticks < least
            && left.is_none_or(|left| ticks < left)
        {
            return Err(format!(
                "this playtest advances at least {least} ticks ({}) at a time",
                clock(least)
            ));
        }
        Ok(ticks)
    }

    /// `result` as the playing seat would read it.
    fn outcome(&self, seat: PlayerId, result: GameResult) -> String {
        match result {
            GameResult::Draw => "a draw".to_string(),
            GameResult::Victory { team } if self.state.winners().contains(&seat) => {
                format!("your team, {team}, won")
            }
            GameResult::Victory { team } => format!("team {team} won and you lost"),
        }
    }

    /// A playtest's order: staged when the game will take it and the pace
    /// allows it, and otherwise refused with the reason, as the shell tells
    /// its player. Commands apply in order before anything else in a tick,
    /// and a rejected one changes nothing, so every order already staged
    /// was taken and a trial tick on a copy of the world reaches the next
    /// tick's verdict on this one.
    fn playtest_send(&mut self, order: PlayerCommand) -> Result<Reply, String> {
        if let Some(refusal) = self.over_pace(order.player, &order.command) {
            return Err(refusal);
        }
        let mut trial = self.pending.clone();
        trial.push(order.clone());
        let report = self.state.clone().tick(&trial);
        let rejection = report.events.iter().find_map(|event| match event {
            Event::CommandRejected { player, reason } if *player == order.player => Some(*reason),
            _ => None,
        });
        if let Some(reason) = rejection {
            let name = serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(|name| name.replace('_', " ")))
                .unwrap_or_else(|| format!("{reason:?}"));
            return Err(format!(
                "the game rejects this order ({name}){}; nothing was sent",
                self.why(&order, reason)
            ));
        }
        self.pending.push(order);
        Ok(Reply::Ok)
    }

    /// Why the seat may not send `next` yet at its pace, if so. Up to
    /// [`TRAIN_BURST`] trains of one kind at one structure in one tick are one
    /// order, as a player clicks a unit's button a few times at once.
    fn over_pace(&self, seat: PlayerId, next: &Command) -> Option<String> {
        let playtest = self.playtest.as_ref()?;
        let cap = usize::try_from(playtest.orders_per_minute?.get()).ok()?;
        let now = self.state.current_tick();
        let sent = self
            .recorder
            .commands
            .iter()
            .filter(|timed| timed.command.player == seat && timed.tick + MINUTE > now)
            .map(|timed| (timed.tick, &timed.command.command))
            .chain(
                self.pending
                    .iter()
                    .filter(|order| order.player == seat)
                    .map(|order| (now, &order.command)),
            );
        let mut recent: Vec<u64> = Vec::new();
        let mut burst = None;
        for (tick, command) in sent {
            match (command, burst) {
                (Command::Train { building, kind }, Some((at, at_building, at_kind, count)))
                    if (at, at_building, at_kind) == (tick, *building, *kind)
                        && count < TRAIN_BURST =>
                {
                    burst = Some((at, at_building, at_kind, count + 1));
                }
                (Command::Train { building, kind }, _) => {
                    burst = Some((tick, *building, *kind, 1));
                    recent.push(tick);
                }
                _ => {
                    burst = None;
                    recent.push(tick);
                }
            }
        }
        if let (Command::Train { building, kind }, Some((at, at_building, at_kind, count))) =
            (next, burst)
            && (at, at_building, at_kind) == (now, *building, *kind)
            && count < TRAIN_BURST
        {
            return None;
        }
        if recent.len() < cap {
            return None;
        }
        recent.sort_unstable();
        let frees = recent[recent.len() - cap] + MINUTE;
        Some(format!(
            "this playtest allows {cap} orders in any minute of game time; \
             your next order may go from tick {frees}"
        ))
    }

    /// What the seat can learn of why the game rejected `order`, as a
    /// clause after the reason, or nothing.
    fn why(&self, order: &PlayerCommand, reason: RejectReason) -> String {
        let seat = order.player;
        match (reason, &order.command) {
            (RejectReason::NotEnoughScrap, _) if !self.pending.is_empty() => {
                let mut after = self.state.clone();
                after.tick(&self.pending);
                format!(
                    ": the orders you already sent this step leave about {} of your {} scrap",
                    after.player(seat).scrap,
                    self.state.player(seat).scrap
                )
            }
            (
                RejectReason::BadSite,
                Command::Build {
                    units,
                    kind,
                    anchor,
                    queue,
                    defer,
                },
            ) => {
                let refusal = if *defer {
                    let replaced = if *queue { &[][..] } else { units.as_slice() };
                    self.state
                        .place_intent_refusal(seat, *kind, *anchor, replaced)
                } else {
                    self.state.place_refusal(seat, *kind, *anchor)
                };
                refusal.map_or_else(String::new, |refusal| {
                    site_refusal(&self.state, seat, *kind, *anchor, refusal)
                })
            }
            _ => String::new(),
        }
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

/// What a playtest seat sees happen, gathered tick by tick across one
/// advance: the same events the seat's own screen would show it.
struct Seen {
    seat: PlayerId,
    income: u32,
    log: Vec<SeenEvent>,
    /// Coarse map cells already logged as taking fire this advance.
    fire: BTreeSet<(i32, i32)>,
    /// Units and reasons already logged as stalled this advance.
    stalls: BTreeSet<(aftermine_sim::UnitId, String)>,
    /// Other seats' structures in sight as the coming tick begins, by id,
    /// so a site that vanishes in it can be named.
    in_sight: BTreeMap<aftermine_sim::BuildingId, String>,
}

/// The side of a coarse cell, in tiles: fire in one cell logs once.
const FIRE_CELL: i32 = 4;

impl Seen {
    fn new(seat: PlayerId) -> Self {
        Self {
            seat,
            income: 0,
            log: Vec::new(),
            fire: BTreeSet::new(),
            stalls: BTreeSet::new(),
            in_sight: BTreeMap::new(),
        }
    }

    /// Notes other seats' structures the seat sees before a tick runs.
    fn look(&mut self, state: &State) {
        let vision = state.vision(self.seat);
        self.in_sight = state
            .buildings()
            .iter()
            .filter(|building| {
                let (w, h) = building.kind.size();
                building.player != self.seat
                    && (0..h)
                        .any(|dy| (0..w).any(|dx| vision.visible(building.anchor.offset(dx, dy))))
            })
            .map(|building| {
                (
                    building.id,
                    format!(
                        "{} at ({},{})",
                        building.kind.tier_name(building.tier),
                        building.anchor.x,
                        building.anchor.y
                    ),
                )
            })
            .collect();
    }

    /// The own unit or structure at `pos` after a tick, as the seat names it.
    fn own_at(&self, state: &State, pos: Vec2Fx) -> Option<String> {
        if let Some(unit) = state
            .units()
            .iter()
            .find(|unit| unit.player == self.seat && unit.pos == pos)
        {
            return Some(format!("{} {}", unit.kind.name(), unit.id.0));
        }
        let tile = TilePos::containing(pos);
        state
            .buildings()
            .iter()
            .find(|building| building.player == self.seat && building.contains(tile))
            .map(|building| {
                format!(
                    "{} {}",
                    building.kind.tier_name(building.tier),
                    building.id.0
                )
            })
    }

    /// Keeps what the seat saw of tick `tick`'s `events`, judged against
    /// `state`, the world after that tick: its own losses, completions,
    /// stalls and the places it took fire, and other seats' losses,
    /// warheads and charges where its vision reaches.
    fn note(&mut self, tick: u64, state: &State, events: &[Event]) {
        let seat = self.seat;
        let sees = |pos: Vec2Fx| state.vision(seat).visible(TilePos::containing(pos));
        for event in events {
            let mut note = None;
            let keep = match event {
                Event::ScrapCredited { player, amount }
                | Event::ScrapDeposited { player, amount, .. } => {
                    if *player == seat {
                        self.income += amount;
                    }
                    false
                }
                Event::UnitDied { player, pos, .. }
                | Event::BuildingDestroyed { player, pos, .. } => *player == seat || sees(*pos),
                Event::WarheadLanded { player, at, .. }
                | Event::ChargeDetonated { player, at, .. } => *player == seat || sees(*at),
                Event::BuildCancelled {
                    building, player, ..
                } if *player != seat => {
                    note = self.in_sight.get(building).cloned();
                    note.is_some()
                }
                Event::UnitTrained { player, .. }
                | Event::BuildingCompleted { player, .. }
                | Event::BuildCancelled { player, .. }
                | Event::UnitAssembled { player, .. }
                | Event::CommandRejected { player, .. } => *player == seat,
                Event::OrderStalled {
                    unit,
                    player,
                    reason,
                    ..
                } => *player == seat && self.stalls.insert((*unit, format!("{reason:?}"))),
                Event::DamageTaken { player, pos } => {
                    let tile = TilePos::containing(*pos);
                    let fresh = *player == seat
                        && self
                            .fire
                            .insert((tile.x.div_euclid(FIRE_CELL), tile.y.div_euclid(FIRE_CELL)));
                    if fresh {
                        note = self.own_at(state, *pos);
                    }
                    fresh
                }
                Event::PlayerResigned { .. } | Event::GameOver { .. } => true,
                _ => false,
            };
            if keep {
                self.log.push(SeenEvent {
                    tick,
                    event: event.clone(),
                    note,
                });
            }
        }
    }
}

/// `ticks` as clock time.
fn clock(ticks: u64) -> String {
    let seconds = ticks / u64::from(aftermine_sim::TICKS_PER_SECOND);
    format!("{seconds} s")
}

/// Why a site was refused, as a clause after the reason: what the seat can
/// tell about its own footprint from the map it holds and what it sees or
/// remembers, never what fog hides.
fn site_refusal(
    state: &State,
    seat: PlayerId,
    kind: BuildingKind,
    anchor: TilePos,
    refusal: PlaceRefusal,
) -> String {
    let (w, h) = kind.size();
    let footprint = || (0..h).flat_map(move |dy| (0..w).map(move |dx| anchor.offset(dx, dy)));
    let (map, vision) = (state.map(), state.vision(seat));
    match refusal {
        PlaceRefusal::Fog => {
            ": part of the footprint is not in sight now; bring a unit to see it, \
             or add --defer to have the site placed once it is"
                .to_string()
        }
        PlaceRefusal::Terrain => footprint()
            .find_map(|at| {
                use aftermine_sim::map::Terrain;
                let tile = map.tile(at);
                let scrap = if vision.visible(at) {
                    tile.map_or(0, |tile| tile.scrap)
                } else {
                    vision.remembered_scrap(at)
                };
                let ground = match tile.map(|tile| tile.terrain) {
                    Some(Terrain::Rock) => "rock",
                    Some(Terrain::Peak) => "a peak",
                    Some(Terrain::Pit) => "a pit",
                    Some(Terrain::Cliff) => "a cliff",
                    Some(Terrain::Ground) if map.is_ramp(at) && !kind.fits_ramp() => {
                        "a ramp, where this structure cannot stand"
                    }
                    Some(Terrain::Ground) if scrap > 0 => "a scrap pile",
                    Some(Terrain::Ground) => return None,
                    None => "off the map",
                };
                Some(format!(": tile ({},{}) is {ground}", at.x, at.y))
            })
            .unwrap_or_else(|| ": the footprint is not open ground".to_string()),
        PlaceRefusal::Building => ": a structure you know of stands on the footprint".to_string(),
        PlaceRefusal::Unit => ": an enemy ground unit stands on the footprint".to_string(),
        PlaceRefusal::FrameRequired => {
            ": an Extractor goes only on an Extractor frame, anchored where map.json has an E"
                .to_string()
        }
        PlaceRefusal::FrameBlocked => {
            ": the footprint covers part of an Extractor frame, which only an Extractor may use"
                .to_string()
        }
        PlaceRefusal::Prerequisite => ": the structures it requires are not built".to_string(),
        PlaceRefusal::OutOfPlay => ": your seat is out of the match".to_string(),
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
            recorded_commands: match (&self.playtest, self.caller) {
                (Some(_), Some(seat)) => self
                    .recorder
                    .commands
                    .iter()
                    .filter(|timed| timed.command.player == seat)
                    .count(),
                _ => self.recorder.commands.len(),
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
                &setup.playtest.seats,
                setup.difficulty,
                setup.stance,
                setup.personality_seed_base,
            )?;
            Session::playtest(scenario, setup.playtest)?
        }
        None => Session::new(scenario)?,
    };
    let limits = Limits {
        idle_timeout,
        ..Limits::default()
    };
    let seats = session
        .playtest
        .as_ref()
        .map_or_else(Vec::new, |playtest| playtest.seats.clone());
    if seats.len() > 1 {
        let mut listeners = Vec::new();
        for (offset, seat) in (0_u16..).zip(&seats) {
            let at = port + offset;
            let listener = TcpListener::bind(("127.0.0.1", at))
                .with_context(|| format!("binding seat {} to 127.0.0.1:{at}", seat.0))?;
            eprintln!("seat {} listens on 127.0.0.1:{at}", seat.0);
            listeners.push((*seat, listener));
        }
        serve_seats(listeners, limits, session);
        return Ok(());
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("binding session server to 127.0.0.1:{port}"))?;
    eprintln!(
        "session server listening on 127.0.0.1:{port} — windowless, \
         sim time moves only on request"
    );
    serve_listener(listener, limits, session);
    Ok(())
}

/// The answering loop for a playtest of several client seats, each on its
/// own listener: a client may see and command only its seat. An advance
/// waits, up to 25 seconds, for every seat to ask; then the clock moves and
/// each waiting seat gets its report, and a seat no longer waiting gets it
/// with its next advance.
pub fn serve_seats(listeners: Vec<(PlayerId, TcpListener)>, limits: Limits, mut session: Session) {
    let (tx, rx) = std::sync::mpsc::channel();
    for (seat, listener) in listeners {
        let requests = incoming(listener, limits);
        let tx = tx.clone();
        std::thread::spawn(move || {
            for request in requests {
                if tx.send((seat, request)).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let mut held: BTreeMap<PlayerId, (u64, std::sync::mpsc::Sender<ResponseEnvelope>, Instant)> =
        BTreeMap::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok((seat, IncomingRequest { id, request, reply })) => {
                let Request::AdvanceTicks { ticks } = request else {
                    let response = match session.handle_from(Some(seat), request) {
                        Ok(ok) => ResponseEnvelope::ok(id, ok),
                        Err(err) => ResponseEnvelope::err(id, err),
                    };
                    reply.send(response).ok();
                    continue;
                };
                if let Some(view) = session.take_unread(seat) {
                    reply
                        .send(ResponseEnvelope::ok(id, Reply::Played(view)))
                        .ok();
                    continue;
                }
                match session.ask(seat, ticks) {
                    Err(err) => {
                        reply.send(ResponseEnvelope::err(id, err)).ok();
                    }
                    Ok(None) => {
                        held.insert(seat, (id, reply, Instant::now() + HOLD));
                    }
                    Ok(Some(views)) => {
                        for (owner, view) in views {
                            let waiting = if owner == seat {
                                Some((id, reply.clone()))
                            } else {
                                held.remove(&owner).map(|(id, reply, _)| (id, reply))
                            };
                            // A client that gave up waiting collects the report
                            // with its next advance.
                            let delivered = waiting.is_some_and(|(id, reply)| {
                                reply
                                    .send(ResponseEnvelope::ok(id, Reply::Played(view.clone())))
                                    .is_ok()
                            });
                            if !delivered {
                                session.keep_unread(owner, view);
                            }
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        let expired: Vec<PlayerId> = held
            .iter()
            .filter(|(_, (_, _, deadline))| *deadline <= now)
            .map(|(seat, _)| *seat)
            .collect();
        for seat in expired {
            if let Some((id, reply, _)) = held.remove(&seat) {
                reply.send(ResponseEnvelope::err(id, WAITING)).ok();
            }
        }
    }
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
