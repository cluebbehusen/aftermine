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
pub(super) enum Output {
    Catalog(Vec<crate::saves::ReplayEntry>),
    Loaded(Box<crate::game::checkpoint::RestoredGame>),
    /// A saved LAN match, for Match Setup to resume with its players.
    LanSave(Box<crate::game::checkpoint::LanSave>),
    /// A save finished; an unnamed one names the slot and content it wrote.
    Saved(Option<autosave::SaveMark>),
    Retired,
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
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (work.task)(&work.cancel)
                    }))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("persistence worker failed")));
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
        anyhow::ensure!(self.active.is_none(), "persistence worker is busy");
        self.next += 1;
        let cancel = Cancellation::default();
        self.sender
            .try_send(Work {
                id: self.next,
                cancel: cancel.clone(),
                task,
            })
            .map_err(|_| anyhow::anyhow!("persistence worker is unavailable"))?;
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

    /// Lets a replaced match's world, bots and any speculative bot decision
    /// drop here rather than on the frame thread. A worker busy with other
    /// work leaves the game to drop on the caller's thread.
    pub(super) fn retire(&mut self, old: Game) {
        if self.busy() {
            drop(old);
            return;
        }
        let retired = old.retire();
        if let Err(error) = self.start(Box::new(move |_| {
            retired();
            Ok(Output::Retired)
        })) {
            eprintln!("Retiring the previous match failed: {error:#}");
        }
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
    Named(String),
    Leave(screens::pause::LeaveVerb, bool),
    Rematch,
}
impl Intent {
    fn loading(&self) -> bool {
        matches!(self, Self::Load(_) | Self::Continue)
    }
}
pub(super) struct Busy {
    intent: Intent,
    back: Box<Screen>,
    id: Option<u64>,
    presented: bool,
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
        Screen::Busy(Box::new(Busy {
            intent,
            back: Box::new(back),
            id: None,
            presented: false,
            quit_after: false,
            leave_after: false,
            menu: crate::menu::Menu::list(
                if loading {
                    "LOADING GAME"
                } else {
                    "SAVING GAME"
                },
                if loading {
                    vec!["Cancel".into()]
                } else {
                    Vec::new()
                },
            ),
        }))
    }

    pub(super) fn poll_persistence(&mut self, screen: &mut Screen) {
        autosave::discard_when_idle(
            &mut self.discarded,
            self.persistence.busy(),
            crate::paths::autosave_dir().as_deref(),
        );
        if self.catalog_delete.is_some() && !self.persistence.busy() {
            self.request_catalog();
        }
        let Some((id, result)) = self.persistence.poll() else {
            return;
        };
        if self.autosave_id == Some(id) {
            self.autosave_id = None;
            match result {
                Ok(Output::Saved(mark)) => {
                    if let Some(mark) = &mark {
                        self.game.mark_saved(mark);
                    }
                    self.background.succeeded();
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("Autosave failed: {error:#}");
                    let now = self.clock.elapsed();
                    if self.background.failed(now) {
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
                self.notify(format!("Could not refresh saves: {error:#}"), true);
            }
            if let Ok(Output::Catalog(entries)) = result {
                match screen {
                    Screen::Home(home) => {
                        home.set_catalog(
                            entries.iter().any(|e| e.compatible && e.kind.continues()),
                        );
                    }
                    Screen::Replays(shelf) => shelf.set_catalog(entries),
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
            Ok(Output::Saved(mark))
        });
        match self.persistence.start(task) {
            Ok(id) => self.autosave_id = Some(id),
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
        let resumable = local && undecided && !exempt;
        self.background.observe(super::background_save::Frame {
            played: if resumable {
                crate::numeric::to_f32(
                    f64::from(u32::try_from(ticks).unwrap_or(u32::MAX))
                        / f64::from(oxide_sim::TICKS_PER_SECOND)
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
            && let Some(link) = self.net.as_mut().filter(|link| link.hosting())
        {
            link.rearm_autosave();
            self.background.started();
        }
        if self.persistence.busy() || matches!(screen, Screen::Busy(_)) {
            return;
        }
        let decided = !undecided && self.game.end_stats.is_some() && unsaved;
        if self
            .background
            .due(decided, local && undecided && unsaved, self.clock.elapsed())
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
            Ok(Output::Saved(mark))
        });
        match self.persistence.start(task) {
            Ok(id) => {
                self.autosave_id = Some(id);
                self.background.started();
            }
            Err(error) => self.notify(error.to_string(), true),
        }
    }

    pub(super) fn request_catalog(&mut self) {
        if self.persistence.busy() || self.catalog_id.is_some() {
            return;
        }
        let delete = self.catalog_delete.take();
        let task: Task = Box::new(move |cancel| {
            if let Some(path) = delete {
                std::fs::remove_file(path)?;
            }
            let entries = crate::saves::discover(|| cancel.cancelled());
            anyhow::ensure!(!cancel.cancelled(), "catalog cancelled");
            Ok(Output::Catalog(entries))
        });
        match self.persistence.start(task) {
            Ok(id) => self.catalog_id = Some(id),
            Err(error) => self.notify(error.to_string(), true),
        }
    }
}

fn load_candidates(paths: Vec<PathBuf>, cancel: &Cancellation) -> Result<Output> {
    let mut last_error = anyhow::anyhow!("no loadable save is available");
    for path in paths {
        anyhow::ensure!(!cancel.cancelled(), "load cancelled");
        match crate::saved_game::prepare(&path) {
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

pub(super) fn frame(app: &mut App, mut busy: Box<Busy>, events: &[RawEvent]) -> Result<Screen> {
    render::draw(
        &app.game.view(),
        &app.sprites,
        &app.input,
        &app.config.bindings,
    );
    veil();
    let loading = busy.intent.loading();
    let dots = match numeric::to_u32(numeric::to_f32(get_time()) * 3.0) % 3 {
        0 => ".",
        1 => "..",
        _ => "...",
    };
    match &*busy.back {
        Screen::Pause(pause) if !loading => pause.draw_saving(&format!("Saving{dots}")),
        _ => busy.menu.draw(&format!(
            "{}{}",
            if loading {
                "Loading game"
            } else {
                "Saving game"
            },
            dots
        )),
    }
    if loading
        && app.persistence_result.is_none()
        && (busy.menu.handle(events, &mut app.input.mouse).is_some()
            || events.iter().any(|event| {
                matches!(
                    event,
                    RawEvent::KeyDown {
                        key: oxide_protocol::Key::Escape
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
                        Intent::Leave(screens::pause::LeaveVerb::Quit, false),
                        Screen::Playing,
                    ));
                }
                return Ok(Screen::Playing);
            }
            Ok(Output::LanSave(save)) => {
                // Nothing was installed, so quitting leaves as Home would.
                if busy.quit_after {
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::Quit, true),
                        *busy.back,
                    ));
                }
                // The lobby opens at once, under the player's saved name.
                app.draft.player_name = app
                    .config
                    .player_name
                    .as_deref()
                    .and_then(|name| oxide_net::PlayerName::new(name).ok());
                return Ok(Screen::Wizard(screens::wizard::Wizard::resume(
                    *save,
                    &mut app.draft,
                )));
            }
            Ok(Output::Saved(mark)) => {
                if let Some(mark) = mark {
                    app.game.mark_saved(&mark);
                }
                if busy.quit_after {
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::Quit, false),
                        *busy.back,
                    ));
                }
                if busy.leaving_after() {
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::MainMenu, false),
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
                    Intent::Leave(screens::pause::LeaveVerb::Quit, _) => std::process::exit(0),
                    Intent::Leave(_, _) => Screen::Home(HomeScreen::open()),
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
                if busy.quit_after {
                    return Ok(Screen::Pause(pause_menu(&app.game).with_save_failed(
                        error.to_string(),
                        screens::pause::LeaveVerb::Quit,
                        false,
                    )));
                }
                if busy.leaving_after() {
                    app.notify(error.to_string(), true);
                    return Ok(app.persistence_screen(
                        Intent::Leave(screens::pause::LeaveVerb::MainMenu, false),
                        *busy.back,
                    ));
                }
                if let Intent::Leave(verb, home) = busy.intent {
                    return Ok(Screen::Pause(pause_menu(&app.game).with_save_failed(
                        error.to_string(),
                        verb,
                        home,
                    )));
                }
                app.notify(
                    if loading {
                        format!("Load failed: {error}")
                    } else {
                        error.to_string()
                    },
                    true,
                );
                if let Screen::Pause(ref mut pause) = *busy.back {
                    pause.end_naming(error.to_string());
                }
                return Ok(*busy.back);
            }
            Ok(Output::Catalog(_) | Output::Retired) => unreachable!(),
        }
    }
    if !busy.presented {
        busy.presented = true;
        return Ok(Screen::Busy(busy));
    }
    if busy.id.is_none() && !app.persistence.busy() {
        let task: Task = match &busy.intent {
            Intent::Named(_) | Intent::Leave(_, _) | Intent::Rematch => {
                let name = if let Intent::Named(name) = &busy.intent {
                    Some(name.as_str())
                } else {
                    None
                };
                // A LAN host keeps an undecided match resumable, and a
                // match it names is always a LAN save, decided or not;
                // everyone else keeps what a local or decided match keeps.
                let lan = app
                    .net
                    .as_ref()
                    .filter(|_| name.is_some() || app.game.state.result().is_none())
                    .and_then(|link| link.capture_lan_save(&app.game));
                let job = match lan {
                    Some(capture) => autosave::SaveJob::lan(&app.game, capture, name),
                    None => autosave::SaveJob::capture(&app.game, name),
                };
                let mark = job.mark();
                Box::new(move |_| {
                    job.run().map_err(|error| {
                        let line = error.player_line();
                        anyhow::Error::new(error).context(line)
                    })?;
                    Ok(Output::Saved(mark))
                })
            }
            Intent::Load(_) | Intent::Continue => {
                let paths = match &busy.intent {
                    Intent::Load(path) => Some(vec![path.clone()]),
                    _ => None,
                };
                Box::new(move |cancel| {
                    load_candidates(paths.unwrap_or_else(autosave::candidates), cancel)
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

#[cfg(test)]
mod tests;
