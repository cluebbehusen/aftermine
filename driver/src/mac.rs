//! `cargo mac`: build the Mac app through the Xcode project in `apple/`
//! and open it. The app runs as a signed bundle, so unlike a workspace
//! `cargo run` it keeps saves in iCloud Drive.

use anyhow::{Context, Result, bail};
use std::process::Command;

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
    // `open` would bring an older copy forward instead of starting the new
    // build. No match is fine: pkill then just reports that nothing ran.
    let executable = app.join("Contents/MacOS/Aftermine");
    let _ = Command::new("pkill").arg("-f").arg(&executable).status();
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
