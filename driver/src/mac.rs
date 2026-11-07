//! `cargo mac`: build the Mac app through the Xcode project in `apple/`
//! and open it. The app runs as a signed bundle, so unlike a workspace
//! `cargo run` it keeps saves in iCloud Drive.

use anyhow::{Context, Result, bail};
use std::process::Command;
use std::time::{Duration, Instant};

/// How long a running copy gets to write its autosave and exit.
const QUIT_WAIT: Duration = Duration::from_secs(10);

/// Quits a copy of the app that is still running, builds the Release app,
/// and opens the fresh one.
pub fn run() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("cargo mac needs macOS with Xcode");
    }
    let root = crate::ios::workspace_root();
    let app = root.join("target/mac-xcode/Build/Products/Release/Aftermine.app");
    let info_plist = app.join("Contents/Info.plist");
    // The build rewrites the last build's executable in place, and macOS
    // kills a running copy whose signed code changes, before it can save.
    if info_plist.is_file() {
        quit_running(&crate::ios::bundle_id(&info_plist)?)?;
    }
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
    // A first build, or one with a new bundle ID, still has to quit a copy
    // opened from elsewhere.
    quit_running(&crate::ios::bundle_id(&info_plist)?)?;
    let app = app.canonicalize().context("finding the built app")?;
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

/// Quits any running copy of the app, wherever it was opened from: macOS
/// treats copies with one bundle ID as the same app, so `open` would bring
/// that copy forward instead of starting the new build. The quit goes
/// through the app as a player's would, so a match in progress is autosaved
/// first; a signal would end the process without that save.
fn quit_running(bundle: &str) -> Result<()> {
    if !running(bundle) {
        return Ok(());
    }
    println!("quitting the running copy");
    // The app always answers "User canceled": it turns the system quit down
    // so its own quit can save first, then exits. Whether it exits is the
    // answer.
    let quit = Command::new("osascript")
        .arg("-e")
        .arg(format!("quit app id {}", applescript_string(bundle)))
        .output()
        .context("running osascript")?;
    let started = Instant::now();
    while running(bundle) {
        if started.elapsed() > QUIT_WAIT {
            bail!(
                "Aftermine is still running; quit it, then run cargo mac again\n{}",
                crate::ios::tool_output(&quit)
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Whether any copy with this bundle ID is running. `lsappinfo` answers for
/// an ID macOS has never seen, where `osascript` would fail.
fn running(bundle: &str) -> bool {
    Command::new("lsappinfo")
        .arg("find")
        .arg(format!("bundleid={bundle}"))
        .output()
        .is_ok_and(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// `text` as a string literal in an `osascript` script.
fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests;
