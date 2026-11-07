//! `cargo mac`: build the Mac app through the Xcode project in `apple/`
//! and open it. The app runs as a signed bundle, so unlike a workspace
//! `cargo run` it keeps saves in iCloud Drive.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// How long a running copy gets to write its autosave and exit.
const QUIT_WAIT: Duration = Duration::from_secs(10);

/// Builds the Release app, quits a copy of it that is still running, and
/// opens the fresh one.
pub fn run() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("cargo mac needs macOS with Xcode");
    }
    let root = crate::ios::workspace_root();
    let status = Command::new("xcodebuild")
        .current_dir(&root)
        .args([
            "-project",
            "apple/Aftermine.xcodeproj",
            "-scheme",
            "Aftermine Mac",
            "-configuration",
            "Release",
            "-derivedDataPath",
            "target/mac-xcode",
            "-allowProvisioningUpdates",
            "-allowProvisioningDeviceRegistration",
            "-quiet",
            "-destination",
            "platform=macOS,arch=arm64",
            "build",
        ])
        .status()
        .context("running xcodebuild")?;
    if !status.success() {
        bail!("xcodebuild failed");
    }
    let app = root
        .join("target/mac-xcode/Build/Products/Release/Aftermine.app")
        .canonicalize()
        .context("finding the built app")?;
    quit_running(&app)?;
    let status = Command::new("open")
        .arg(&app)
        .status()
        .context("running open")?;
    if !status.success() {
        bail!("could not open {}", app.display());
    }
    println!("running {}", app.display());
    Ok(())
}

/// Quits a copy still running from `app`, since `open` would bring it
/// forward instead of starting the new build. The quit goes through the app
/// as a player's would, so a match in progress is autosaved first; a signal
/// would end the process without that save.
fn quit_running(app: &Path) -> Result<()> {
    let executable = app.join("Contents/MacOS/Aftermine");
    if !running(&executable) {
        return Ok(());
    }
    println!("quitting the running copy");
    let quit = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "quit app {}",
            applescript_string(&app.display().to_string())
        ))
        .output()
        .context("running osascript")?;
    if !quit.status.success() {
        bail!(
            "could not quit the running copy:\n{}",
            crate::ios::tool_output(&quit)
        );
    }
    let started = Instant::now();
    while running(&executable) {
        if started.elapsed() > QUIT_WAIT {
            bail!("Aftermine is still running; quit it, then run cargo mac again");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn running(executable: &Path) -> bool {
    Command::new("pgrep")
        .arg("-f")
        .arg(executable)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// `text` as an AppleScript string literal.
fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests;
