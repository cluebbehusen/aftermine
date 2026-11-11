use super::*;
use crate::numeric;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc,
};

#[derive(Clone, Default)]
pub(super) struct Cancellation(Arc<AtomicU8>);
impl Cancellation {
    fn cancel(&self) -> bool {
        self.0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) == 1
    }
    fn commit(&self) -> Result<()> {
        self.0
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| anyhow::anyhow!("load cancelled"))
    }
}

type Task = Box<dyn FnOnce(&Cancellation) -> Result<Output> + Send>;
struct Work {
    id: u64,
    cancel: Cancellation,
    task: Task,
}
/// A save the app started on its own, while the worker runs it.
pub(super) struct BackgroundJob {
    id: u64,
    /// The match it saves.
    slot: autosave::SaveSlot,
    /// Whether it captured the match decided, so it writes the finished
    /// record any later save of that match would only repeat.
    decided: bool,
}

impl BackgroundJob {
    fn of(id: u64, game: &Game) -> Self {
        Self {
            id,
            slot: game.slot.clone(),
            decided: game.state.result().is_some(),
        }
    }
}

/// Whether `intent` can leave at once: Main menu while the worker is still
/// writing `slot`'s finished record, which leaving would otherwise wait
/// behind only to write again. That save finishes on the worker. Should it
/// fail, Home retries it while the match stays the app's, a Quit from there
/// saves it first, and a match started meanwhile still hears of the failure.
/// Quit waits, since it ends the process, and Rematch replaces the match the
/// save reports against.
fn leaves_at_once(
    intent: &Intent,
    running: Option<&BackgroundJob>,
    slot: &autosave::SaveSlot,
) -> bool {
    matches!(intent, Intent::Leave(screens::pause::LeaveVerb::MainMenu))
        && running.is_some_and(|job| job.decided && job.slot == *slot)
}

/// Whether a failed background save must reach the player even though the
/// app has moved on to another match: a finished record has no later save
/// to stand in for it once its match is gone.
fn failure_outlives_its_match(job: &BackgroundJob, slot: &autosave::SaveSlot) -> bool {
    job.decided && job.slot != *slot
}

/// How long the shelf waits before looking again for records still
/// downloading from iCloud.
const RESCAN: std::time::Duration = std::time::Duration::from_secs(2);

pub(super) enum Output {
    Catalog(crate::saves::Catalog),
    Loaded(Box<crate::game::checkpoint::RestoredGame>),
    /// A saved LAN match, for Match Setup to resume with its players.
    LanSave(Box<crate::game::checkpoint::LanSave>),
    /// A replay from Saves & Replays, for the viewer to open.
    Recording(Box<aftermine_kit::Recording>),
    /// A save finished, with the slot and content the match counts as saved.
    Saved(Option<autosave::SaveMark>),
    Discarded,
}

pub(super) struct Worker {
    sender: mpsc::SyncSender<Work>,
    receiver: mpsc::Receiver<(u64, Result<Output>)>,
    active: Option<(u64, Cancellation)>,
    next: u64,
}
impl Worker {
    pub(super) fn new() -> Result<Self> {
        let (sender, jobs) = mpsc::sync_channel::<Work>(1);
        let (results, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("save-loader".into())
            .spawn(move || {
                while let Ok(work) = jobs.recv() {
                    // Leaving the app mid-save must not cut the file short.
                    let background = aftermine_apple_bridge::application::BackgroundTask::begin();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (work.task)(&work.cancel)
                    }))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Could not finish saving")));
                    drop(background);
                    if results.send((work.id, result)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            receiver,
            active: None,
            next: 0,
        })
    }
    fn start(&mut self, task: Task) -> Result<u64> {
        anyhow::ensure!(self.active.is_none(), "Another save is still running");
        self.next += 1;
        let cancel = Cancellation::default();
        self.sender
            .try_send(Work {
                id: self.next,
                cancel: cancel.clone(),
                task,
            })
            .map_err(|_| anyhow::anyhow!("Could not start saving"))?;
        self.active = Some((self.next, cancel));
        Ok(self.next)
    }
    fn cancel(&self) -> bool {
        self.active
            .as_ref()
            .is_none_or(|(_, cancel)| cancel.cancel())
    }
    pub(super) fn busy(&self) -> bool {
        self.active.is_some()
    }

    fn poll(&mut self) -> Option<(u64, Result<Output>)> {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.active = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Disconnected) => self
                .active
                .take()
                .map(|(id, _)| (id, Err(anyhow::anyhow!("persistence worker stopped")))),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}

pub(super) enum Intent {
    Load(PathBuf),
    Continue,
    /// Opens the replay at this path in the viewer.
    Watch(PathBuf),
    Named(String),
    Leave(screens::pause::LeaveVerb),
    Rematch,
    /// Quits once the worker is idle, saving nothing: no live match is
    /// held, so the one behind the screen was saved on the way out or left
    /// unsaved on purpose. A LAN match is left first, and the quit waits a
    /// little for what is still going out (see [`departure`]).
    Exit,
}
impl Intent {
    fn loading(&self) -> bool {
        matches!(self, Self::Load(_) | Self::Continue | Self::Watch(_))
    }

    /// Whether this resumes a match, which another device may have played
    /// on since: a replay is a finished record and has no newer copy.
    fn resumes(&self) -> bool {
        matches!(self, Self::Load(_) | Self::Continue)
    }

    /// What a quit asks for: a live match is saved first; anything else
    /// has nothing to save.
    pub(super) fn quit(holds_live_match: bool) -> Self {
        if holds_live_match {
            Self::Leave(screens::pause::LeaveVerb::Quit)
        } else {
            Self::Exit
        }
    }
}
pub(super) struct Busy {
    intent: Intent,
    back: Box<Screen>,
    id: Option<u64>,
    presented: bool,
    /// When this began waiting on another thread: a load for a sync under
    /// way, or an exit for what is still going out.
    waiting_since: Option<std::time::Duration>,
    pub(super) menu: crate::menu::Menu<usize>,
    quit_after: bool,
    /// The LAN match ended while this ran: leave it once done.
    leave_after: bool,
}
impl Busy {
    pub(super) fn mode(&self) -> &'static str {
        if self.intent.loading() {
            "loading"
        } else {
            "saving"
        }
    }
    pub(super) fn request_quit(&mut self) {
        self.quit_after = true;
    }
    pub(super) fn request_leave(&mut self) {
        self.leave_after = true;
    }
    /// Whether the match must be left once this finishes: it ended
    /// meanwhile, and this was not already leaving.
    fn leaving_after(&self) -> bool {
        self.leave_after && !matches!(self.intent, Intent::Leave(..))
    }
}

impl App {
    pub(super) fn persistence_screen(&mut self, intent: Intent, back: Screen) -> Screen {
        // A LAN host's pause would stop every machine, and its save is
        // captured within one frame, so a LAN match plays on.
        if self.net.is_none() {
            self.game.clock.paused = true;
        }
        if self.catalog_id.take().is_some() {
            self.persistence.cancel();
        }
        let loading = intent.loading();
        // Another device may have just stopped playing this match.
        if intent.resumes() {
            crate::cloud_mirror::request(crate::cloud_mirror::Mode::Full);
        }
        let title = if matches!(intent, Intent::Watch(_)) {
            "LOADING REPLAY"
        } else if loading {
            "LOADING MATCH"
        } else if matches!(intent, Intent::Exit) {
            "QUITTING"
        } else {
            "SAVING MATCH"
        };
        Screen::Busy(Box::new(Busy {
            intent,
            back: Box::new(back),
            id: None,
            presented: false,
            waiting_since: None,
            quit_after: false,
            leave_after: false,
            menu: crate::menu::Menu::list(
                title,
                if loading {
                    vec!["Cancel".into()]
                } else {
                    Vec::new()
                },
            ),
        }))
    }

    pub(super) fn poll_persistence(&mut self, screen: &mut Screen) {
        crate::cloud_mirror::allow_arrivals(match screen {
            Screen::Home(_) | Screen::Replays(_) => true,
            Screen::Busy(busy) => busy.intent.loading() && busy.id.is_none(),
            _ => false,
        });
        if !self.discarded.is_empty() && !self.persistence.busy() {
            let slots = std::mem::take(&mut self.discarded);
            if let Err(error) = self.persistence.start(Box::new(move |_| {
                autosave::discard(&slots);
                Ok(Output::Discarded)
            })) {
                eprintln!("Discarding abandoned autosaves failed: {error:#}");
            }
        }
        if self.catalog_delete.is_some() && !self.persistence.busy() {
            self.request_catalog();
        }
        let Some((id, result)) = self.persistence.poll() else {
            return;
        };
        if let Some(job) = self.autosave_id.take_if(|job| job.id == id) {
            // A replaced match's save no longer counts toward this one.
            let current = job.slot == self.game.slot;
            match result {
                Ok(Output::Saved(mark)) => {
                    if let Some(mark) = &mark {
                        self.game.mark_saved(mark);
                    }
                    if current {
                        self.background.succeeded();
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("Autosave failed: {error:#}");
                    let now = self.clock.elapsed();
                    let tell = if current {
                        self.background.failed(now)
                    } else {
                        failure_outlives_its_match(&job, &self.game.slot)
                    };
                    if tell {
                        self.notify(error.to_string(), true);
                    }
                    // A wait freezes the tick the timed LAN save counts, so
                    // try again after a while instead.
                    self.autosave_retry = Some(now + super::background_save::RETRY);
                }
            }
        } else if self.catalog_id == Some(id) {
            self.catalog_id = None;
            if let Err(error) = &result {
                self.notify_failure("Could not read saves", format_args!("{error:#}"));
            }
            if let Ok(Output::Catalog(catalog)) = result {
                self.catalog_rescan =
                    (catalog.downloading > 0).then(|| self.clock.elapsed() + RESCAN);
                match screen {
                    Screen::Home(home) => {
                        // A rescan that changes nothing leaves the menu,
                        // and any press on it, alone.
                        let resumable = catalog.continuable().next().is_some();
                        if !home.catalog_ready || home.resumable() != resumable {
                            home.set_catalog(resumable);
                        }
                    }
                    Screen::Replays(shelf) => {
                        shelf.set_catalog(catalog.entries, catalog.downloading);
                    }
                    _ => {}
                }
            }
        } else if matches!(screen, Screen::Busy(busy) if busy.id == Some(id)) {
            self.persistence_result = Some(result);
        }
    }

    /// Starts the host's own save of its running LAN match when one is due
    /// and the worker is free; a busy worker leaves it due for a later
    /// frame.
    pub(super) fn lan_autosave(&mut self) {
        if self.persistence.busy() {
            return;
        }
        if let Some(at) = self.autosave_retry
            && self.clock.elapsed() >= at
            && let Some(link) = &mut self.net
        {
            self.autosave_retry = None;
            link.rearm_autosave();
        }
        let Some(capture) = self
            .net
            .as_mut()
            .and_then(|link| link.lan_autosave(&self.game))
        else {
            return;
        };
        let job = autosave::SaveJob::lan(&self.game, capture, None);
        let mark = job.mark();
        let task: Task = Box::new(move |_| {
            job.run().map_err(|error| {
                let line = error.player_line();
                anyhow::Error::new(error).context(line)
            })?;
            crate::cloud_mirror::request(crate::cloud_mirror::Mode::Push);
            Ok(Output::Saved(mark))
        });
        match self.persistence.start(task) {
            Ok(id) => self.autosave_id = Some(BackgroundJob::of(id, &self.game)),
            Err(error) => self.notify(error.to_string(), true),
        }
    }

    /// Starts a save the app owes on its own: a decided match's record, or
    /// a resumable match's autosave after a minute of live play or as the
    /// window goes inactive. `ticks` is how far the live match advanced
    /// during the frame. A LAN host's autosaves go through its link, which
    /// also saves on its own schedule.
    pub(super) fn background_save(&mut self, screen: &Screen, ticks: u64) {
        // Automated runs keep their timing and data folder to themselves.
        let exempt = self.args.automation;
        let undecided = self.game.state.result().is_none();
        let unsaved = !self.game.is_saved();
        let local = self.game.net_role().is_none();
        // A lesson is never saved; leaving one keeps nothing.
        let kept = self.game.tutorial.is_none();
        let resumable = local && undecided && kept && !exempt;
        self.background.observe(super::background_save::Frame {
            played: if resumable {
                crate::numeric::to_f32(
                    f64::from(u32::try_from(ticks).unwrap_or(u32::MAX))
                        / f64::from(aftermine_sim::TICKS_PER_SECOND)
                        / self.game.clock.speed.max(f64::EPSILON),
                )
            } else {
                0.0
            },
            minimized: (!exempt && screen.holds_live_match())
                .then(crate::input::reported_minimized)
                .flatten(),
            unsaved,
        });
        if self.background.inactive()
            && undecided
            && self.net.as_ref().is_some_and(crate::netplay::Link::hosting)
        {
            if let Some(link) = &mut self.net {
                link.rearm_autosave();
            }
            self.background.started();
            // The frame that reports going inactive may be the last before
            // the platform suspends the app, so the save starts now.
            self.lan_autosave();
        }
        if self.persistence.busy() || matches!(screen, Screen::Busy(_)) {
            return;
        }
        let decided = self.owes_finished_record();
        if self
            .background
            .due(
                decided,
                local && undecided && kept && unsaved,
                self.clock.elapsed(),
            )
            .is_none()
        {
            return;
        }
        let job = autosave::SaveJob::capture(&self.game, None);
        let mark = job.mark();
        let task: Task = Box::new(move |_| {
            job.run().map_err(|error| {
                let line = error.player_line();
                anyhow::Error::new(error).context(line)
            })?;
            // Sent at once, so a session left on one device can continue
            // on another.
            crate::cloud_mirror::request(crate::cloud_mirror::Mode::Push);
            Ok(Output::Saved(mark))
        });
        match self.persistence.start(task) {
            Ok(id) => {
                self.autosave_id = Some(BackgroundJob::of(id, &self.game));
                self.background.started();
            }
            Err(error) => self.notify(error.to_string(), true),
        }
    }

    /// Whether the app's match is decided and its finished record not yet
    /// written, so even a front-door screen's Quit saves it first.
    pub(super) fn owes_finished_record(&self) -> bool {
        self.game.state.result().is_some() && self.game.end_stats.is_some() && !self.game.is_saved()
    }

    pub(super) fn request_catalog(&mut self) {
        if self.persistence.busy() || self.catalog_id.is_some() {
            return;
        }
        let delete = self.catalog_delete.take();
        let now = self.clock.elapsed();
        if self.mirror_at.is_some_and(|at| now >= at) {
            crate::cloud_mirror::request(crate::cloud_mirror::Mode::Full);
            self.mirror_at = Some(now + crate::cloud_mirror::EVERY);
        }
        let task: Task = Box::new(move |cancel| {
            if let Some(path) = delete {
                crate::cloud_mirror::deleting(&path);
                aftermine_apple_bridge::coordinate(
                    &path,
                    aftermine_apple_bridge::Access::Delete,
                    |granted| std::fs::remove_file(granted),
                )??;
                crate::cloud_mirror::request(crate::cloud_mirror::Mode::Push);
            }
            let mut catalog = crate::saves::discover(|| cancel.cancelled());
            anyhow::ensure!(!cancel.cancelled(), "catalog cancelled");
            catalog.downloading += crate::cloud_mirror::pending();
            Ok(Output::Catalog(catalog))
        });
        match self.persistence.start(task) {
            Ok(id) => {
                self.catalog_id = Some(id);
                self.catalog_rescan = None;
                self.catalog_cloud = record_changes();
            }
            Err(error) => self.notify(error.to_string(), true),
        }
    }

    /// Whether the last catalog may be out of date: records were still
    /// downloading from iCloud long enough ago to look again, the iCloud
    /// folder or a mirror pass changed records since its scan began, or it
    /// is time to ask the mirror for records other devices changed.
    pub(super) fn catalog_stale(&self) -> bool {
        let now = self.clock.elapsed();
        self.catalog_rescan.is_some_and(|at| now >= at)
            || record_changes() != self.catalog_cloud
            || self.mirror_at.is_some_and(|at| now >= at)
    }
}

/// The longest a load waits for a sync under way.
const SYNC_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Changes that can leave a catalog out of date: a different iCloud Drive
/// folder, or a mirror pass that changed local records. Both counts only
/// grow, so their sum changes whenever either does.
fn record_changes() -> u64 {
    crate::paths::cloud_changes().wrapping_add(crate::cloud_mirror::changes())
}

/// Reads the replay at `path` under a coordinated read, which waits for a
/// file still downloading from iCloud.
fn open_recording(path: &std::path::Path, cancel: &Cancellation) -> Result<Output> {
    let recording =
        aftermine_apple_bridge::coordinate(path, aftermine_apple_bridge::Access::Read, |granted| {
            aftermine_kit::load_recording(granted)
        })
        .map_err(anyhow::Error::from)
        .and_then(|loaded| loaded.map_err(anyhow::Error::from))
        .with_context(|| format!("loading replay {}", path.display()))?;
    cancel.commit()?;
    Ok(Output::Recording(Box::new(recording)))
}

/// What the notice strip says when `intent` fails with `error`.
fn failure_line(intent: &Intent, error: &anyhow::Error) -> String {
    match intent {
        Intent::Watch(_) => "Could not open the replay".to_string(),
        Intent::Load(_) | Intent::Continue => "Could not load that save".to_string(),
        _ => error.to_string(),
    }
}

fn load_candidates(paths: Vec<PathBuf>, cancel: &Cancellation) -> Result<Output> {
    let mut last_error = anyhow::anyhow!("no loadable save is available");
    for path in paths {
        anyhow::ensure!(!cancel.cancelled(), "load cancelled");
        // A coordinated read waits for a file still downloading, or for a
        // newer copy another device is replacing it with.
        let prepared = aftermine_apple_bridge::coordinate(
            &path,
            aftermine_apple_bridge::Access::Read,
            crate::saved_game::prepare,
        )
        .map_err(anyhow::Error::from)
        .and_then(|prepared| prepared);
        match prepared {
            Ok(crate::saved_game::Loaded::Local(loaded)) => {
                cancel.commit()?;
                return Ok(Output::Loaded(loaded));
            }
            Ok(crate::saved_game::Loaded::Lan(save)) => {
                cancel.commit()?;
                return Ok(Output::LanSave(save));
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

pub(super) fn frame(
    app: &mut App,
    mut busy: Box<Busy>,
    events: &[RawEvent],
    rerun: &mut bool,
) -> Result<Screen> {
    if busy.id.is_none() && leaves_at_once(&busy.intent, app.autosave_id.as_ref(), &app.game.slot) {
        // Home draws this frame, so none goes by blank.
        *rerun = true;
        return Ok(Screen::Home(HomeScreen::open()));
    }
    let loading = busy.intent.loading();
    if loading {
        page_backdrop();
    } else {
        match_backdrop(app);
    }
    let dots = match numeric::to_u32(numeric::to_f32(get_time()) * 3.0) % 3 {
        0 => ".",
        1 => "..",
        _ => "...",
    };
    let exiting = matches!(busy.intent, Intent::Exit);
    match &*busy.back {
        Screen::Pause(pause) if !loading => {
            let doing = if exiting { "Quitting" } else { "Saving" };
            pause.draw_saving(&app.sprites, &format!("{doing}{dots}"));
        }
        _ => busy.menu.draw(
            &app.sprites,
            &format!(
                "{}{}",
                if matches!(busy.intent, Intent::Watch(_)) {
                    "Loading replay"
                } else if loading {
                    "Loading game"
                } else if exiting {
                    "Quitting"
                } else {
                    "Saving game"
                },
                dots
            ),
        ),
    }
    if loading
        && app.persistence_result.is_none()
        && (busy.menu.handle(events, &mut app.input.mouse).is_some()
            || events.iter().any(|event| {
                matches!(
                    event,
                    RawEvent::KeyDown {
                        key: aftermine_protocol::Key::Escape
                    }
                )
            }))
        && (busy.id.is_none() || app.persistence.cancel())
    {
        app.persistence_result = None;
        return Ok(*busy.back);
    }
    if let Some(result) = app.persistence_result.take() {
        match result {
            Ok(Output::Loaded(loaded)) => {
                app.install(loaded.install(), Install::local(true));
                if busy.quit_after {
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::Quit),
                        Screen::Playing,
                    ));
                }
                return Ok(Screen::Playing);
            }
            Ok(Output::LanSave(save)) => {
                // Nothing was installed, so quitting saves nothing.
                if busy.quit_after {
                    return Ok(app.persistence_screen(Intent::Exit, *busy.back));
                }
                // The lobby opens at once, under the player's name.
                app.draft.player_name = Some(app.config.name());
                return Ok(Screen::Wizard(screens::wizard::Wizard::resume(
                    *save,
                    &mut app.draft,
                )));
            }
            Ok(Output::Recording(recording)) => {
                // Nothing was installed, so quitting saves nothing.
                if busy.quit_after {
                    return Ok(app.persistence_screen(Intent::Exit, *busy.back));
                }
                return Ok(watch(*recording, *busy.back, |error| {
                    app.notify_failure("Could not open the replay", format_args!("{error:#}"));
                }));
            }
            Ok(Output::Saved(mark)) => {
                if let Some(mark) = mark {
                    app.game.mark_saved(&mark);
                }
                if busy.quit_after {
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::Quit),
                        *busy.back,
                    ));
                }
                if busy.leaving_after() {
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::MainMenu),
                        *busy.back,
                    ));
                }
                return Ok(match busy.intent {
                    Intent::Named(name) => {
                        if let Screen::Pause(ref mut pause) = *busy.back {
                            pause.end_naming(format!("saved: {name}"));
                        }
                        *busy.back
                    }
                    Intent::Leave(screens::pause::LeaveVerb::Quit) => {
                        app.persistence_screen(Intent::Exit, *busy.back)
                    }
                    Intent::Leave(screens::pause::LeaveVerb::MainMenu) => {
                        Screen::Home(HomeScreen::open())
                    }
                    Intent::Rematch => {
                        let fresh = super::screen_flow::rebuild_match(&app.game)?;
                        app.install(fresh, Install::local(app.args.paused));
                        Screen::Playing
                    }
                    _ => unreachable!(),
                });
            }
            Err(error) => {
                eprintln!(
                    "{} failed: {error:#}",
                    if loading { "Load" } else { "Save" }
                );
                return Ok(
                    match after_failure(&busy.intent, busy.quit_after, busy.leaving_after()) {
                        AfterFailure::Exit => app.persistence_screen(Intent::Exit, *busy.back),
                        AfterFailure::SaveFailed(verb) => Screen::Pause(
                            pause_menu(&app.game).with_save_failed(error.to_string(), verb),
                        ),
                        AfterFailure::Leave => {
                            app.notify(error.to_string(), true);
                            app.persistence_screen(
                                Intent::Leave(screens::pause::LeaveVerb::MainMenu),
                                *busy.back,
                            )
                        }
                        AfterFailure::Back => {
                            app.notify(failure_line(&busy.intent, &error), true);
                            if let Screen::Pause(ref mut pause) = *busy.back {
                                pause.end_naming(error.to_string());
                            }
                            *busy.back
                        }
                    },
                );
            }
            Ok(Output::Catalog(_) | Output::Discarded) => unreachable!(),
        }
    }
    // An exit with nothing left to go out quits without drawing a frame.
    if !busy.presented && !exiting {
        busy.presented = true;
        return Ok(Screen::Busy(busy));
    }
    // Another device's newer copy of a match should be here before it resumes,
    // since saving over it from an older one would lose that progress.
    if busy.id.is_none() && busy.intent.resumes() && crate::cloud_mirror::syncing() {
        let now = app.clock.elapsed();
        let since = *busy.waiting_since.get_or_insert(now);
        if now.saturating_sub(since) < SYNC_WAIT {
            return Ok(Screen::Busy(busy));
        }
    }
    if exiting && busy.id.is_none() && !app.persistence.busy() {
        let now = app.clock.elapsed();
        let waited = now.saturating_sub(*busy.waiting_since.get_or_insert(now));
        let flushing =
            app.lingering.is_some() || !app.ticket.settled() || !crate::cloud_mirror::sent();
        match departure(app.net.is_some(), flushing, waited) {
            Departure::Leave => super::screen_flow::leave_network(app),
            Departure::Wait => {}
            Departure::Exit => std::process::exit(0),
        }
        return Ok(Screen::Busy(busy));
    }
    if busy.id.is_none() && !app.persistence.busy() {
        let task: Task = match &busy.intent {
            Intent::Load(_) | Intent::Continue => {
                let paths = match &busy.intent {
                    Intent::Load(path) => Some(vec![path.clone()]),
                    _ => None,
                };
                Box::new(move |cancel| {
                    load_candidates(paths.unwrap_or_else(autosave::candidates), cancel)
                })
            }
            Intent::Watch(path) => {
                let path = path.clone();
                Box::new(move |cancel| open_recording(&path, cancel))
            }
            intent => {
                let Some(job) = save_job(intent, &app.game, app.net.as_ref()) else {
                    unreachable!("only an exit saves nothing, and it starts no task")
                };
                let mark = job.mark();
                Box::new(move |_| {
                    job.run().map_err(|error| {
                        let line = error.player_line();
                        anyhow::Error::new(error).context(line)
                    })?;
                    crate::cloud_mirror::request(crate::cloud_mirror::Mode::Push);
                    Ok(Output::Saved(mark))
                })
            }
        };
        match app.persistence.start(task) {
            Ok(id) => busy.id = Some(id),
            Err(error) => app.persistence_result = Some(Err(error)),
        }
    }
    Ok(Screen::Busy(busy))
}

/// The viewer over a recording the worker opened. Leaving it lands back on
/// `back`, the shelf that asked, its tab and cursor where they were; a
/// recording that will not play stays there and says so through `failed`.
fn watch(
    recording: aftermine_kit::Recording,
    back: Screen,
    failed: impl FnOnce(anyhow::Error),
) -> Screen {
    match screens::playback::PlaybackSession::watch(recording) {
        Ok(session) => super::screen_flow::open_playback(session, back),
        Err(error) => {
            failed(error);
            back
        }
    }
}

/// How long a quit waits for what is still going out: a LAN goodbye, the
/// rejoin ticket's removal, and the iCloud push of saves made so far, the
/// quit's own included.
const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The next step of a quit with nothing left to save.
#[derive(Debug, PartialEq, Eq)]
enum Departure {
    /// Leave the LAN match still joined, saying goodbye.
    Leave,
    /// Let what is going out finish.
    Wait,
    Exit,
}

/// Where a quit stands: `joined` while a LAN match is still joined,
/// `flushing` while something is still going out, `waited` since the quit
/// began. Other devices learn of the quit only if the process outlives it,
/// but never for longer than [`EXIT_WAIT`].
fn departure(joined: bool, flushing: bool, waited: std::time::Duration) -> Departure {
    if joined {
        Departure::Leave
    } else if flushing && waited < EXIT_WAIT {
        Departure::Wait
    } else {
        Departure::Exit
    }
}

/// Where a failed save or load goes.
#[derive(Debug, PartialEq, Eq)]
enum AfterFailure {
    /// Quit anyway: a failed load installed nothing, so there is nothing to
    /// save.
    Exit,
    /// The save-failure dialog, holding open what the player was leaving
    /// toward.
    SaveFailed(screens::pause::LeaveVerb),
    /// The LAN match ended meanwhile: say what failed and leave it.
    Leave,
    /// Say what failed, back where the player was.
    Back,
}

/// Where a failed `intent` goes, given a quit asked for meanwhile
/// (`quit_after`) or a LAN match that ended meanwhile (`leaving_after`).
fn after_failure(intent: &Intent, quit_after: bool, leaving_after: bool) -> AfterFailure {
    if quit_after && intent.loading() {
        AfterFailure::Exit
    } else if quit_after {
        AfterFailure::SaveFailed(screens::pause::LeaveVerb::Quit)
    } else if leaving_after {
        AfterFailure::Leave
    } else if let Intent::Leave(verb) = intent {
        AfterFailure::SaveFailed(*verb)
    } else {
        AfterFailure::Back
    }
}

/// The save `intent` owes before it completes, if any. A LAN host keeps
/// an undecided match resumable, and a match it names is always a LAN
/// save, decided or not; everyone else keeps what a local or decided match
/// keeps.
fn save_job(
    intent: &Intent,
    game: &crate::game::Game,
    net: Option<&crate::netplay::Link>,
) -> Option<autosave::SaveJob> {
    let name = match intent {
        Intent::Named(name) => Some(name.as_str()),
        Intent::Leave(_) | Intent::Rematch => None,
        Intent::Load(_) | Intent::Continue | Intent::Watch(_) | Intent::Exit => return None,
    };
    let lan = net
        .filter(|_| name.is_some() || game.state.result().is_none())
        .and_then(|link| link.capture_lan_save(game));
    Some(match lan {
        Some(capture) => autosave::SaveJob::lan(game, capture, name),
        None => autosave::SaveJob::capture(game, name),
    })
}

#[cfg(test)]
mod tests;
