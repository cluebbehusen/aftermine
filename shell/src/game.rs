//! Live simulation, bot execution, command recording, and session bookkeeping.
//! Shared presentation borrows the active world and never advances it.

use crate::numeric;
use crate::numeric::Fit;
use anyhow::Result;
use chassis::replay::Replay;
use macroquad::prelude::{Vec2, vec2};
use oxide_kit::controller::{SeatController, seat_controllers};
use oxide_kit::diagnostics::{Stage, StageGuard};
use oxide_protocol::hash_hex;
use oxide_sim::{
    Building, BuildingId, Command, Event, PlayerCommand, PlayerId, SIM_VERSION, Scenario, State,
    TICKS_PER_SECOND, UnitId, UnitKind,
};
use std::{ops::Deref, sync::Arc};

/// Seconds per sim tick.
pub const TICK_DT: f32 = 1.0 / TICKS_PER_SECOND as f32;
/// Catch-up limit before returning to presentation. Supports the 64x speed cap
/// at 60 fps; expensive ticks can still exceed the frame budget.
const MAX_TICKS_PER_FRAME: u32 = 24;

pub use oxide_kit::GameReplay;

/// Immutable access to the authoritative simulation outside this module.
pub(crate) struct ReadOnlyState(Arc<State>);

impl Deref for ReadOnlyState {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
impl std::ops::DerefMut for ReadOnlyState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::get_mut(&mut self.0).expect("bot decision must finish before world mutation")
    }
}

/// How far a tick got before a panic: whether its batch was recorded and
/// whether the simulation finished it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickPhase {
    Collecting,
    Simulating,
    Simulated,
}

/// Commands waiting for the next recorded tick. A networked session keeps
/// its bound seat's orders here from sending until their batch executes.
#[derive(Debug, Default)]
pub(crate) struct PendingCommands(Vec<PlayerCommand>);

impl Deref for PendingCommands {
    type Target = Vec<PlayerCommand>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
impl std::ops::DerefMut for PendingCommands {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// What the player currently has selected.
#[derive(Default)]
pub struct Selection {
    /// Selected units, single-allegiance by construction (own for
    /// command, ally or enemy for read-only inspection).
    pub units: Vec<UnitId>,
    /// Selected buildings of one owner (mutually exclusive with units
    /// in practice), kept in id order. Commands validate ownership at
    /// their own gates.
    pub buildings: Vec<BuildingId>,
    /// An inspected salvage tile: held only while nothing else is
    /// selected and the viewer knows the tile still holds salvage.
    pub pile: Option<chassis::grid::TilePos>,
}

/// A transient visual effect (never sim-relevant).
mod clock;
mod fx;
mod presentation;
pub(crate) use clock::Clock;
pub(crate) use presentation::{AlertKind, Presentation, Salvage, Scene, Stance};
pub(crate) mod checkpoint;
pub(crate) mod network;
mod projectiles;
pub(crate) mod projection;
pub(crate) use projectiles::LaunchPose;

pub(crate) use fx::{
    BuildingHit, HitSurface, INTERCEPT_OVERLAP_TICKS, InterceptFlight, UnitBody, UnitHit,
    WARHEAD_BURST,
};
pub use fx::{Effect, EffectKind, FlakYokeDelay, PingKind, ShotStyle, SoundKind};

/// A transient HUD message (rejected orders, stalled units).
pub struct Toast {
    /// What to say.
    pub text: String,
    /// Seconds since raised.
    pub age: f32,
}

/// One running session.
pub struct Game {
    /// The scenario this session started from.
    pub scenario: Scenario,
    /// The sim. Mutable access stays inside `Game` outside test fixtures.
    pub(crate) state: ReadOnlyState,
    /// Command sources for bot-flagged players.
    bots: Vec<SeatController>,
    bot_decision: Option<oxide_kit::bot_execution::PendingDecision>,
    /// Every command of the session, tick-stamped.
    pub recorder: GameReplay,
    /// Worlds kept beside the recording so its replay seeks quickly.
    pub(crate) keyframes: oxide_kit::keyframes::KeyframeRecorder,
    /// A resumed match's saved history, until it is checked against the
    /// saved world.
    pub(crate) history: Option<oxide_kit::history::PendingHistory>,
    /// Commands staged for the next tick (human + debug socket).
    pub(crate) pending: PendingCommands,
    /// Where this match's autosave and finished record go.
    pub(crate) slot: crate::autosave::SaveSlot,
    /// What the slot last received. Guards double-writes when Main Menu
    /// saves and the same game then quits as the Home backdrop.
    pub(crate) saved: Option<crate::autosave::SaveKey>,
    /// Where a tick that panics leaves its command log; none keeps nothing.
    pub(crate) crash_dumps: Option<std::path::PathBuf>,
    /// How far the tick in progress got, for its crash dump.
    tick_phase: TickPhase,
    /// End-of-match statistics, computed once from the recorder when
    /// the result lands from the bounded live tracker.
    pub end_stats: Option<oxide_kit::stats::MatchStats>,
    /// Tick-event totals and adaptively thinned graph samples. This is
    /// presentation bookkeeping and never feeds back into the sim.
    live_stats: oxide_kit::stats::LiveMatchStats,
    /// Match statistics at the moment the human conceded an undecided team
    /// match, shown with the exit offer. Decided matches (a 1v1 surrender
    /// included) use `end_stats` instead.
    pub concede_stats: Option<oxide_kit::stats::MatchStats>,
    /// The Basic Training lesson this session stages, if any. A session
    /// carrying one is never saved.
    pub(crate) tutorial: Option<crate::tutorial::Tutorial>,
    /// The match tips this session has shown and is watching for.
    pub(crate) tips: crate::tips::Tips,
    /// True during bulk fast-forwards: presentation (fx, sounds, facing)
    /// is skipped instead of accumulated and discarded, so a long advance
    /// does not buffer every effect it passes.
    suppress_presentation: bool,
    /// This machine's side of a lockstep session, whose ticks come from
    /// supplied batches rather than `do_tick`.
    net: Option<network::NetRole>,
    /// Staged orders not yet handed to the session host.
    outbox: Vec<Command>,
    pub(crate) presentation: Presentation,
    /// Whether this match's time runs, how fast, and its tick debt.
    pub(crate) clock: Clock,
}

pub(crate) fn world_vec(pos: chassis::fx::Vec2Fx) -> Vec2 {
    vec2(pos.x.to_num::<f32>(), pos.y.to_num::<f32>())
}

impl Game {
    pub(crate) fn retire(self) -> impl FnOnce() + Send {
        let Self {
            state,
            bots,
            bot_decision,
            recorder,
            history,
            live_stats,
            presentation,
            ..
        } = self;
        drop(presentation);
        move || drop((state, bots, bot_decision, recorder, history, live_stats))
    }

    pub(crate) fn view(&self) -> Scene<'_> {
        Scene::new(
            &self.state,
            &self.scenario,
            &self.pending,
            &self.presentation,
            &self.clock,
        )
    }
    pub fn selection_commandable(&self) -> bool {
        self.view().selection_commandable()
    }
    pub fn home_foundry(&self) -> Option<&Building> {
        self.view().home_foundry()
    }
    pub fn my_vision(&self) -> &oxide_sim::Vision {
        self.state.vision(self.presentation.human)
    }
    pub(crate) fn update_fx(&mut self, dt: f32) {
        self.presentation.update_fx(&self.state, dt);
    }
    /// Ages effects by frame time while the match's clock runs; driven
    /// presentation steps call [`Self::update_fx`] directly because they
    /// stand for sim time even while the wall clock is paused.
    pub(crate) fn update_wall_clock_fx(&mut self, dt: f32) {
        if !self.clock.paused {
            self.update_fx(dt);
        }
    }
    pub(crate) fn drop_presentation(&mut self) {
        self.presentation.drop_presentation(&self.state);
    }
    pub(crate) fn replace_state_after_jump(&mut self, state: &State) {
        self.bot_decision = None;
        self.state.0 = Arc::new(state.clone());
        self.presentation.reset_after_jump(&self.state);
        self.refresh_fog_lift();
    }

    /// Lifts fog for the local seat once it is out of an undecided match
    /// and no LAN teammate plays on whom it could tip off. A local
    /// session's other human seats are passive, a bot ally reads only its
    /// own fog-honest observation, and a free-for-all seat leaves nobody
    /// fighting for its side. A spectator stays one.
    fn refresh_fog_lift(&mut self) {
        if self.presentation.spectating() {
            return;
        }
        let (state, seat) = (&self.state, self.presentation.human);
        let lan_teammate_plays = || {
            self.net.is_some()
                && self
                    .scenario
                    .players
                    .iter()
                    .enumerate()
                    .any(|(index, player)| {
                        let mate = PlayerId::from_index(index);
                        !player.bot && !state.hostile(seat, mate) && state.accepts_commands(mate)
                    })
        };
        let out =
            state.result().is_none() && !state.accepts_commands(seat) && !lan_teammate_plays();
        self.presentation.stance = if out { Stance::Out } else { Stance::Seat };
    }

    /// Starts a session from a scenario. Local input uses the first non-bot
    /// seat, or seat zero for an all-bot scene; configured bots keep running.
    pub fn new(scenario: Scenario) -> Result<Self> {
        Self::with_viewport(scenario, crate::render::viewport())
    }

    /// `new` with an explicit viewport instead of the injected window size.
    pub fn with_viewport(scenario: Scenario, viewport: Vec2) -> Result<Self> {
        let human = Self::local_seat(&scenario);
        Self::assemble(scenario, viewport, human, true)
    }

    fn local_seat(scenario: &Scenario) -> PlayerId {
        PlayerId(
            (scenario
                .players
                .iter()
                .position(|player| !player.bot)
                .unwrap_or(0))
            .fit::<u8>(),
        )
    }

    fn assemble(
        scenario: Scenario,
        viewport: Vec2,
        human: PlayerId,
        run_bots: bool,
    ) -> Result<Self> {
        let state = scenario.build()?;
        let live_stats = oxide_kit::stats::LiveMatchStats::new(&state);
        let recorder = Replay::new(
            SIM_VERSION,
            crate::build_identity().label(),
            scenario.clone(),
        );
        Self::assemble_at(
            scenario, state, live_stats, recorder, viewport, human, run_bots,
        )
    }

    /// A session over `state`, which `recorder` records from.
    fn assemble_at(
        scenario: Scenario,
        state: State,
        live_stats: oxide_kit::stats::LiveMatchStats,
        recorder: GameReplay,
        viewport: Vec2,
        human: PlayerId,
        run_bots: bool,
    ) -> Result<Self> {
        let bots = if run_bots {
            seat_controllers(&scenario)?
        } else {
            Vec::new()
        };
        let presentation = Presentation::new(&state, human, viewport);
        let mut game = Self {
            scenario,
            state: ReadOnlyState(Arc::new(state)),
            bots,
            bot_decision: None,
            recorder,
            keyframes: oxide_kit::keyframes::KeyframeRecorder::default(),
            history: None,
            pending: PendingCommands(Vec::new()),
            slot: crate::autosave::SaveSlot::fresh(),
            saved: None,
            crash_dumps: None,
            tick_phase: TickPhase::Collecting,
            end_stats: None,
            live_stats,
            concede_stats: None,
            tutorial: None,
            tips: crate::tips::Tips::default(),
            suppress_presentation: false,
            net: None,
            outbox: Vec::new(),
            presentation,
            clock: Clock::default(),
        };
        game.refresh_fog_lift();
        Ok(game)
    }

    /// Resumes a session from a recorded replay: rebuilds its scenario,
    /// re-executes every recorded tick headlessly, and keeps recording
    /// onto the same log.
    pub fn from_replay(replay: GameReplay) -> Result<Self> {
        // Replays load synchronously on the frame loop, and a structurally
        // valid file can still claim a duration long enough to freeze the
        // UI for minutes.
        const MAX_LOAD_TICKS: u64 = oxide_kit::MAX_REPLAY_TICKS;
        let _load = oxide_kit::diagnostics::stage(Stage::ReplayLoad, 0);
        // Untrusted file: enforce the invariants recording guarantees, and
        // refuse cross-version saves outright, since resuming one would
        // keep recording onto a log that can no longer reproduce.
        replay
            .validate(Some(SIM_VERSION))
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        anyhow::ensure!(
            replay.origin.is_none(),
            "live continuation requires a session checkpoint for this replay origin"
        );
        let scenario = replay.setup.clone();
        let mut state = scenario.build()?;
        let total = replay.meta.ticks.unwrap_or_else(|| {
            replay
                .commands
                .last()
                .map_or(0, |c| c.tick.saturating_add(1))
        });
        anyhow::ensure!(
            total <= MAX_LOAD_TICKS,
            "replay spans {total} ticks, beyond the {MAX_LOAD_TICKS}-tick interactive load limit \
             (the headless driver replays without one)"
        );
        // The fast-forward lets bots observe every tick to rebuild controller
        // memory. Their generated commands are discarded because the record
        // remains authoritative.
        let mut bots = seat_controllers(&scenario)?;
        let mut cursor = replay.cursor();
        let mut live_stats = oxide_kit::stats::LiveMatchStats::new(&state);
        let mut projectile_releases = projectiles::ProjectileReleases::default();
        let mut game = Self::new(scenario)?;
        let mut boundary_fog = game.presentation.boundary_fog.clone();
        for _ in 0..total {
            oxide_kit::diagnostics::progress(state.current_tick());
            let _ = oxide_kit::bot_execution::commands(&state, &mut bots);
            let commands: Vec<PlayerCommand> = cursor
                .take_tick(state.current_tick())
                .iter()
                .map(|t| t.command.clone())
                .collect();
            let report = state.tick(&commands);
            oxide_kit::controller::record_events(&mut bots, &report);
            game.keyframes.observe(&state);
            boundary_fog.observe(&state, game.presentation.human);
            projectile_releases.observe(&state, &report.events);
            live_stats.observe(&state, &report.events);
        }
        anyhow::ensure!(
            cursor.is_finished(),
            "replay duration metadata does not cover its own commands"
        );
        game.replace_state_after_jump(&state);
        game.presentation.boundary_fog = boundary_fog;
        game.presentation.projectile_releases = projectile_releases;
        game.bots = bots;
        game.recorder = replay;
        game.live_stats = live_stats;
        if game.state.result().is_some() {
            game.end_stats = Some(game.live_stats.snapshot(&game.state));
            // The replay already is this match's finished record.
            game.saved = Some(game.save_key());
        }
        if let Some(focus) = game
            .state
            .buildings()
            .iter()
            .find(|b| b.player == game.presentation.human)
            .map(|b| world_vec(b.center()))
        {
            game.presentation.camera.center = focus;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        }
        Ok(game)
    }

    /// Enter a main-thread diagnostic stage at this session's tick.
    pub(crate) fn diagnostic_stage(&self, stage: Stage) -> Option<StageGuard<'static>> {
        oxide_kit::diagnostics::stage(stage, self.state.current_tick())
    }

    /// Runs exactly one tick: bots think, staged commands drain, everything
    /// is recorded, and presentation caches update. Outside networked
    /// sessions, which execute supplied batches, this owns the live world's
    /// only authoritative state transition.
    pub fn do_tick(&mut self) -> oxide_sim::TickReport {
        assert!(
            self.net.is_none(),
            "networked sessions execute supplied batches"
        );
        self.guard_tick(|game| {
            let mut commands = std::mem::take(&mut game.pending.0);
            commands.extend(game.bot_commands());
            game.execute(&commands)
        })
    }

    /// Runs one tick's work. A panic leaves the command log behind for its
    /// repro before it continues to unwind, since the world it interrupted
    /// is not trusted.
    pub(crate) fn guard_tick<R>(&mut self, work: impl FnOnce(&mut Self) -> R) -> R {
        let tick = self.state.current_tick();
        let recorded = self.recorder.commands.len();
        self.tick_phase = TickPhase::Collecting;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(self))) {
            Ok(result) => result,
            Err(payload) => {
                self.dump_crash(tick, recorded, payload.as_ref());
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// Writes the commands through the last completed tick, with the
    /// panicking tick's batch kept apart when the simulation never finished
    /// it. Bots decide from the world alone, so a panic while collecting
    /// their orders replays from the prefix.
    fn dump_crash(&self, tick: u64, recorded: usize, payload: &(dyn std::any::Any + Send)) {
        let Some(root) = &self.crash_dumps else {
            return;
        };
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-text panic payload".into());
        let mut replay = self.recorder.clone();
        let (phase, unfinished, ticks) = match self.tick_phase {
            TickPhase::Collecting => ("collection", Vec::new(), tick),
            TickPhase::Simulating => (
                "simulation",
                replay
                    .commands
                    .split_off(recorded)
                    .into_iter()
                    .map(|timed| timed.command)
                    .collect(),
                tick,
            ),
            TickPhase::Simulated => ("after_simulation", Vec::new(), tick + 1),
        };
        replay.meta.ticks = Some(ticks);
        let build = crate::build_identity();
        // Failing to write the dump must not replace the original panic.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            oxide_kit::diagnostics::write_crash_dump(
                root,
                &oxide_kit::diagnostics::CrashDump {
                    replay: &replay,
                    phase,
                    unfinished: &unfinished,
                    message: &message,
                    build: &build,
                },
            )
        }));
    }

    /// The session content a save taken now would hold.
    pub(crate) fn save_key(&self) -> crate::autosave::SaveKey {
        crate::autosave::SaveKey::new(self.state.current_tick(), self.pending.len())
    }

    /// Whether the slot already holds this session's content.
    pub(crate) fn is_saved(&self) -> bool {
        self.saved == Some(self.save_key())
    }

    /// Records a finished save, unless it wrote another match's slot.
    pub(crate) fn mark_saved(&mut self, mark: &crate::autosave::SaveMark) {
        if mark.slot == self.slot {
            self.saved = Some(mark.key);
        }
    }

    /// This tick's bot commands, joining any background decision.
    fn bot_commands(&mut self) -> Vec<PlayerCommand> {
        let _bot_stage = self.diagnostic_stage(Stage::Bots);
        if let Some(decision) = self.bot_decision.take() {
            decision.finish(&self.state.0, &mut self.bots)
        } else {
            oxide_kit::bot_execution::commands(&self.state, &mut self.bots)
        }
    }

    /// Records and executes one complete batch, then updates statistics,
    /// lesson evidence, and presentation.
    fn execute(&mut self, commands: &[PlayerCommand]) -> oxide_sim::TickReport {
        // Interpolation cache; pointless during suppressed bulk advances
        // (advance_ticks rebuilds it once at the end).
        for command in commands {
            self.recorder
                .record(self.state.current_tick(), command.clone());
        }
        self.tick_phase = TickPhase::Simulating;
        if !self.suppress_presentation {
            self.presentation.remember_previous_tick(&self.state);
        }
        let sim_stage = self.diagnostic_stage(Stage::Simulation);
        let report = Arc::get_mut(&mut self.state.0)
            .expect("bot decision retained the world after collection")
            .tick(commands);
        drop(sim_stage);
        self.tick_phase = TickPhase::Simulated;
        self.keyframes.observe(&self.state);
        if let Some(history) = self.history.take() {
            self.history = history.settle(&mut self.recorder, &mut self.keyframes);
        }
        oxide_kit::controller::record_events(&mut self.bots, &report);
        let _presentation_stage = self.diagnostic_stage(Stage::Presentation);
        self.refresh_fog_lift();
        self.presentation
            .boundary_fog
            .observe(&self.state, self.presentation.human);
        self.live_stats.observe(&self.state, &report.events);
        if self.state.result().is_some() && self.end_stats.is_none() {
            self.end_stats = Some(self.live_stats.snapshot(&self.state));
        }

        if let Some(tutorial) = &mut self.tutorial {
            tutorial.observe(self.presentation.human, &report.events);
        }

        // A concession that did not decide the match (a team game with the
        // ally fighting on) raises the surrender overlay with the human's
        // statistics so far and Esc-to-menu as the exit. A decisive
        // surrender goes through the normal result flow, and a bulk
        // fast-forward (replay load) keeps only the resigned fact: the
        // banner marks a fresh concession, not standing state.
        if !self.suppress_presentation
            && self.state.result().is_none()
            && report
                .events
                .iter()
                .any(|e| matches!(e, Event::PlayerResigned { player } if *player == self.presentation.human))
        {
            self.concede_stats = Some(self.live_stats.snapshot(&self.state));
            self.presentation.conceded_banner = true;
        }

        if self.suppress_presentation {
            self.presentation
                .projectile_releases
                .observe(&self.state, &report.events);
        } else {
            self.presentation
                .observe_tick(&self.state, &report.events, &report.movement);
        }
        // Dead units leave the selection, and so do hostiles whose ground
        // fog has returned: the panel reads live hp from the selection, so
        // an inspection must not track a unit into fog. Allies stay
        // because team sight is shared.
        let human = self.presentation.human;
        let all_seeing = self.presentation.all_seeing();
        {
            let state = &self.state;
            self.presentation.selection.units.retain(|id| {
                state.unit(*id).is_some_and(|u| {
                    !state.hostile(human, u.player) || all_seeing || {
                        state.vision(human).sees_unit(u)
                    }
                })
            });
        }
        self.presentation.selection.buildings.retain(|id| {
            self.state.building(*id).is_some_and(|building| {
                !self.state.hostile(human, building.player)
                    || all_seeing
                    || (building
                        .tiles()
                        .any(|tile| self.state.vision(human).visible(tile))
                        && self.state.building_apparent(human, building))
            })
        });
        if let Some(tile) = self.presentation.selection.pile
            && presentation::known_salvage(&self.state, human, all_seeing, tile).is_none()
        {
            self.presentation.selection.pile = None;
        }
        report
    }

    /// Advances the ordinary live path while optionally stopping on one exact
    /// tick. Native profiling supplies the bound so a multi-tick frame cannot
    /// overshoot its requested sample window; ordinary play passes `None`.
    pub fn advance_wall_clock(&mut self, dt: f32, stop_tick: Option<u64>) -> bool {
        if self.clock.paused {
            return false;
        }
        let stopped = |game: &Self| stop_tick.is_some_and(|tick| game.state.current_tick() >= tick);
        self.pace(dt * numeric::to_f32(self.clock.speed), |game| {
            if stopped(game) {
                return false;
            }
            game.do_tick();
            true
        });
        if stopped(self) {
            self.settle_clock(0.0);
            return true;
        }
        if self.clock.accum < TICK_DT {
            self.prepare_bot_decision();
        }
        false
    }

    /// Runs `tick` once for each whole tick of accumulated time, up to a
    /// frame's cap. A declined tick or the cap drops the remaining debt
    /// rather than spiraling, leaving the render fraction at one full tick.
    fn pace(&mut self, dt: f32, mut tick: impl FnMut(&mut Self) -> bool) {
        self.clock.accum += dt;
        let mut ran = 0;
        while self.clock.accum >= TICK_DT && ran < MAX_TICKS_PER_FRAME {
            self.settle_clock(self.clock.accum - TICK_DT);
            if !tick(self) {
                self.clock.accum += TICK_DT;
                break;
            }
            ran += 1;
        }
        self.settle_clock(self.clock.accum.min(TICK_DT));
    }

    /// Sets the clock's tick debt and draws the picture at that point.
    fn settle_clock(&mut self, accum: f32) {
        self.clock.accum = accum;
        self.presentation
            .set_tick_fraction(self.clock.tick_fraction());
    }

    fn prepare_bot_decision(&mut self) {
        if self.bot_decision.is_none() {
            self.bot_decision = oxide_kit::bot_execution::prepare(&self.state.0, &self.bots);
        }
    }

    /// Fast-forwards `n` ticks immediately, pause state notwithstanding
    /// (the debug socket's driven-clock mode).
    pub fn advance_ticks(&mut self, n: u64) {
        self.suppress_presentation = true;
        for _ in 0..n {
            self.do_tick();
        }
        self.suppress_presentation = false;
        // No interpolation across a bulk advance, and presentation queued
        // before it does not survive the jump.
        self.settle_clock(0.0);
        self.drop_presentation();
        self.presentation.remember_previous_tick(&self.state);
        self.presentation.facing.clear();
        self.presentation.refresh_facing(&self.state, &[]);
    }

    /// Advances a small number of ticks while retaining presentation
    /// effects and returning the sim events. This is the agent-facing
    /// counterpart to [`Game::advance_ticks`]: slower, but suitable for
    /// judging command acceptance, transient feedback, and combat frames.
    pub fn present_ticks(&mut self, n: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..n {
            // A presented step stands in for one normal sim interval.
            // Age what the previous tick put on screen before spawning
            // this tick's effects, leaving the newest effects at age zero.
            self.update_fx(TICK_DT);
            events.extend(self.do_tick().events);
        }
        self.settle_clock(0.0);
        events
    }

    /// Stages a command from the local player for the next tick.
    pub fn issue(&mut self, command: Command) {
        self.stage(PlayerCommand {
            player: self.presentation.human,
            command,
        });
    }

    /// Stages an already attributed command from the debug input surface.
    /// A networked session also queues it for the session host; such a
    /// session stages only its bound seat's orders.
    pub(crate) fn stage(&mut self, command: PlayerCommand) {
        if self.net.is_some() {
            assert_eq!(
                command.player, self.presentation.human,
                "a networked session stages only its bound seat's orders"
            );
            self.outbox.push(command.command.clone());
        }
        self.pending.0.push(command);
    }

    /// Current state fingerprint, protocol-formatted.
    pub fn hash_hex(&self) -> String {
        hash_hex(self.state.hash())
    }

    /// The transport's view of this session, which is also the live half
    /// of the debug protocol's shared surface.
    pub fn status_view(&self) -> oxide_protocol::StatusView {
        oxide_protocol::StatusView {
            tick: self.state.current_tick(),
            paused: self.clock.paused,
            speed: self.clock.speed,
            scenario: self.scenario.name.clone(),
            sim_version: SIM_VERSION,
            result: self.state.result(),
            recorded_commands: self.recorder.commands.len(),
        }
    }
}

pub(crate) fn rotor_hull_turn_rate(kind: UnitKind) -> Option<f32> {
    match crate::look::unit(kind).gait {
        crate::look::Gait::Rotor { hull_turn } => Some(hull_turn),
        crate::look::Gait::Treads | crate::look::Gait::Legs | crate::look::Gait::Plain => None,
    }
}

fn angle_delta(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// The live session's half of the debug protocol's shared surface: real
/// clock, real recorder, sim time driven through the same `do_tick`
/// funnel every other command source uses.
impl oxide_protocol::DebugSession for Game {
    fn status(&self) -> oxide_protocol::StatusView {
        self.status_view()
    }

    fn state(&self) -> &State {
        &self.state
    }

    fn advance(&mut self, ticks: u64) -> oxide_protocol::AdvancedView {
        self.advance_ticks(ticks);
        oxide_protocol::AdvancedView {
            ticks,
            tick: self.state.current_tick(),
            hash: self.hash_hex(),
        }
    }

    fn present(&mut self, ticks: u64) -> oxide_protocol::PresentedView {
        let events = self.present_ticks(ticks);
        oxide_protocol::PresentedView {
            ticks,
            tick: self.state.current_tick(),
            hash: self.hash_hex(),
            events,
        }
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), String> {
        self.clock.paused = paused;
        Ok(())
    }

    fn set_speed(&mut self, multiplier: f64) -> Result<(), String> {
        oxide_protocol::check_speed(multiplier)?;
        self.clock.speed = multiplier;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
