//! Player-requested local exports; all serialization and copying happens off-frame.
use anyhow::{Context, Result, ensure};
use std::sync::mpsc::{self, Receiver};

#[derive(Default)]
pub(crate) struct ReportJob {
    receiver: Option<Receiver<Result<String, String>>>,
}
impl ReportJob {
    fn begin(
        &mut self,
        name: &str,
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
        self.receiver = Some(receiver);
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
        self.begin("oxide-report-export", move || {
            let incidents = root.join(oxide_kit::diagnostics::INCIDENTS);
            let incidents = incidents.is_file().then_some(incidents);
            let crash = oxide_kit::diagnostics::newest_crash_dump(&root);
            ensure!(
                replay.is_some() || incidents.is_some() || crash.is_some(),
                "no match or incident is available to export"
            );
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
                .map(|incidents| (incidents, oxide_kit::diagnostics::INCIDENTS))
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
                    "sim_version": oxide_sim::SIM_VERSION,
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
            Ok(if cfg!(target_os = "ios") {
                "Diagnostic report exported to Files: On My iPad > Oxide > diagnostics."
            } else {
                "Diagnostic report exported. Open diagnostics folder to find it."
            }
            .into())
        })
    }
    pub(crate) fn poll(&mut self) -> Option<Result<String, String>> {
        match self.receiver.as_ref()?.try_recv() {
            Ok(result) => {
                self.receiver = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                Some(Err("diagnostic worker stopped".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
    pub(crate) fn open_folder(&mut self) -> Result<()> {
        let root = crate::paths::diagnostics_dir().context("diagnostics folder unavailable")?;
        self.begin("oxide-open-diagnostics", move || {
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
            Ok("Diagnostics folder opened.".into())
        })
    }
}

#[cfg(test)]
mod tests;
