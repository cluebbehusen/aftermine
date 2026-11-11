//! Player-requested local exports; all serialization and copying happens off-frame.
use anyhow::{Context, Result, ensure};
use std::sync::mpsc::{self, Receiver};

/// A diagnostic operation that failed: the line the player reads, and the
/// cause, for the log.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) line: &'static str,
    pub(crate) cause: String,
}

#[derive(Default)]
pub(crate) struct ReportJob {
    /// The running operation's result, and what the player reads should it
    /// fail.
    receiver: Option<(Receiver<Result<String, String>>, &'static str)>,
}
impl ReportJob {
    fn begin(
        &mut self,
        name: &str,
        failure: &'static str,
        work: impl FnOnce() -> Result<String> + Send + 'static,
    ) -> Result<()> {
        ensure!(
            self.receiver.is_none(),
            "a diagnostic operation is already in progress"
        );
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _ = sender.send(work().map_err(|error| format!("{error:#}")));
            })?;
        self.receiver = Some((receiver, failure));
        Ok(())
    }
    /// Exports the live match's command log, when it has played any ticks,
    /// with the incident log and the newest crash dump under `root`.
    pub(crate) fn start(
        &mut self,
        game: &crate::game::Game,
        root: Option<std::path::PathBuf>,
    ) -> Result<()> {
        let root = root.context("diagnostics folder unavailable")?;
        let replay = (game.state.current_tick() > game.recorder.start_tick()).then(|| {
            let mut replay = game.recorder.clone();
            replay.meta.ticks = Some(game.state.current_tick());
            replay
        });
        self.begin("aftermine-report-export", EXPORT_FAILED, move || {
            let incidents = root.join(aftermine_kit::diagnostics::INCIDENTS);
            let incidents = incidents.is_file().then_some(incidents);
            let crash = aftermine_kit::diagnostics::newest_crash_dump(&root);
            if replay.is_none() && incidents.is_none() && crash.is_none() {
                return Ok(NOTHING_TO_EXPORT.into());
            }
            let reports = root.join("reports");
            std::fs::create_dir_all(&reports)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let report = reports.join(format!("report-{stamp}"));
            std::fs::create_dir(&report)?;
            let crash = crash.iter().flat_map(|dump| {
                [
                    (dump.join("replay.json"), "crash-replay.json"),
                    (dump.join("crash.json"), "crash.json"),
                ]
            });
            let copies: Vec<_> = incidents
                .map(|incidents| (incidents, aftermine_kit::diagnostics::INCIDENTS))
                .into_iter()
                .chain(crash)
                .collect();
            let written = (|| -> Result<()> {
                let mut contents = Vec::new();
                if let Some(replay) = &replay {
                    replay.save(report.join("replay.json"))?;
                    contents.push("replay.json");
                }
                // Copies stream from disk, so a replay of any loadable size fits.
                for (source, name) in copies {
                    std::fs::copy(&source, report.join(name))
                        .with_context(|| format!("could not copy {name}"))?;
                    contents.push(name);
                }
                let manifest = serde_json::json!({
                    "format": 1,
                    "running_build": crate::build_identity(),
                    "sim_version": aftermine_sim::SIM_VERSION,
                    "ticks": replay.as_ref().and_then(|replay| replay.meta.ticks),
                    "contents": contents,
                });
                chassis::fsx::write_atomic(report.join("manifest.json"), |writer| {
                    serde_json::to_writer_pretty(writer, &manifest).map_err(std::io::Error::other)
                })?;
                Ok(())
            })();
            if let Err(error) = written {
                // The folder was created by this export alone.
                let _ = std::fs::remove_dir_all(&report);
                return Err(error);
            }
            Ok(exported_notice(crate::platform::PLATFORM).into())
        })
    }
    /// The finished operation's notice, or why it failed.
    pub(crate) fn poll(&mut self) -> Option<Result<String, Failure>> {
        let (receiver, line) = self.receiver.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Disconnected) => Err("diagnostic worker stopped".into()),
            Err(mpsc::TryRecvError::Empty) => return None,
        };
        let line = *line;
        self.receiver = None;
        Some(result.map_err(|cause| Failure { line, cause }))
    }
    pub(crate) fn open_folder(&mut self) -> Result<()> {
        let root = crate::paths::diagnostics_dir().context("diagnostics folder unavailable")?;
        self.begin("aftermine-open-diagnostics", OPEN_FAILED, move || {
            std::fs::create_dir_all(&root)?;
            #[cfg(target_os = "macos")]
            let mut command = std::process::Command::new("open");
            #[cfg(target_os = "windows")]
            let mut command = std::process::Command::new("explorer");
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let mut command = std::process::Command::new("xdg-open");
            ensure!(
                command.arg(root).status()?.success(),
                "file manager could not open the diagnostics folder"
            );
            Ok(String::new())
        })
    }
}

/// What an export with nothing to put in a report tells the player.
const NOTHING_TO_EXPORT: &str = "Nothing to export yet.";

/// What the player reads when an export fails.
const EXPORT_FAILED: &str = "Could not export the diagnostic report";

/// What the player reads when the diagnostics folder does not open.
const OPEN_FAILED: &str = "Could not open the diagnostics folder";

/// What an export tells the player: where to find the report, wherever
/// `platform` has a way to reach it.
fn exported_notice(platform: crate::platform::Platform) -> &'static str {
    use crate::platform::Platform;
    match platform {
        Platform::Desktop => "Diagnostic report exported. Open diagnostics folder to find it.",
        Platform::Touch => {
            "Diagnostic report exported to Files: On My iPad > Aftermine > diagnostics."
        }
        Platform::Tv => "Diagnostic report exported.",
    }
}

#[cfg(test)]
mod tests;
