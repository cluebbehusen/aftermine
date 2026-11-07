//! `cargo ipad` and `cargo tv`: build the iPad or Apple TV app through the
//! Xcode project in `apple/`, install it, and launch it on a paired device
//! or a simulator. With several targets and no `--device`, it lists them and
//! asks, remembering each family's choice for next time.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fmt::Write as _;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which app to build, and so which devices can run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// The iPad app, `cargo ipad`.
    Ipad,
    /// The Apple TV app, `cargo tv`.
    Tv,
}

impl Family {
    fn command(self) -> &'static str {
        match self {
            Family::Ipad => "cargo ipad",
            Family::Tv => "cargo tv",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Family::Ipad => "iPad",
            Family::Tv => "Apple TV",
        }
    }

    fn scheme(self) -> &'static str {
        match self {
            Family::Ipad => "Aftermine iPad",
            Family::Tv => "Aftermine Apple TV",
        }
    }

    /// Names this family's folders under `target/`.
    fn slug(self) -> &'static str {
        match self {
            Family::Ipad => "ipad",
            Family::Tv => "tv",
        }
    }

    /// Whether a devicectl device runs this family's app. An iPhone also
    /// reports the iOS platform, so iPads go by device type.
    fn runs_on(self, hardware: &Value) -> bool {
        match self {
            Family::Ipad => hardware["deviceType"] == "iPad",
            Family::Tv => hardware["platform"] == "tvOS",
        }
    }

    /// The part of a simulator's device type identifier that marks it.
    fn simulator_type(self) -> &'static str {
        match self {
            Family::Ipad => ".iPad",
            Family::Tv => ".Apple-TV",
        }
    }

    /// The OS name in a simulator runtime identifier and in its label.
    fn runtime_os(self) -> &'static str {
        match self {
            Family::Ipad => "iOS",
            Family::Tv => "tvOS",
        }
    }

    /// Xcode's build products folder for a device or a simulator.
    fn products(self, kind: DeviceKind) -> &'static str {
        match (self, kind) {
            (Family::Ipad, DeviceKind::Physical) => "Release-iphoneos",
            (Family::Ipad, DeviceKind::Simulator { .. }) => "Release-iphonesimulator",
            (Family::Tv, DeviceKind::Physical) => "Release-appletvos",
            (Family::Tv, DeviceKind::Simulator { .. }) => "Release-appletvsimulator",
        }
    }
}

/// Somewhere the app can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What the device calls itself.
    pub name: String,
    /// The identifier Xcode and the install tools accept.
    pub id: String,
    /// Real hardware or a simulator.
    pub kind: DeviceKind,
    /// A short description for the picker.
    pub detail: String,
}

/// How the app reaches a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// A paired device, over a cable or the local network.
    Physical,
    /// A simulator on this Mac.
    Simulator {
        /// Whether it is running already.
        booted: bool,
    },
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.detail)
    }
}

/// The family's paired physical devices from
/// `xcrun devicectl list devices --json-output`.
pub fn physical_devices(family: Family, devicectl: &Value) -> Vec<Device> {
    let devices = devicectl["result"]["devices"].as_array();
    devices
        .into_iter()
        .flatten()
        .filter(|device| {
            let hardware = &device["hardwareProperties"];
            hardware["reality"] == "physical"
                && family.runs_on(hardware)
                && device["connectionProperties"]["pairingState"] == "paired"
        })
        .filter_map(|device| {
            let hardware = &device["hardwareProperties"];
            let id = hardware["udid"].as_str()?;
            let name = device["deviceProperties"]["name"].as_str().unwrap_or(id);
            let model = hardware["marketingName"].as_str().unwrap_or(family.noun());
            let link = match device["connectionProperties"]["transportType"].as_str() {
                Some("localNetwork") => "Wi-Fi",
                Some("wired") => "USB",
                _ => "paired",
            };
            Some(Device {
                name: name.to_string(),
                id: id.to_string(),
                kind: DeviceKind::Physical,
                detail: format!("{model}, {link}"),
            })
        })
        .collect()
}

/// The family's available simulators from
/// `xcrun simctl list devices available --json`.
pub fn simulators(family: Family, simctl: &Value) -> Vec<Device> {
    let Some(runtimes) = simctl["devices"].as_object() else {
        return Vec::new();
    };
    let mut simulators: Vec<Device> = runtimes
        .iter()
        .filter_map(|(runtime, devices)| {
            Some((runtime_label(family, runtime)?, devices.as_array()?))
        })
        .flat_map(|(runtime, devices)| {
            devices.iter().filter_map(move |device| {
                let device_type = device["deviceTypeIdentifier"].as_str()?;
                if !device_type.contains(family.simulator_type()) || device["isAvailable"] == false
                {
                    return None;
                }
                let booted = device["state"] == "Booted";
                Some(Device {
                    name: device["name"].as_str()?.to_string(),
                    id: device["udid"].as_str()?.to_string(),
                    kind: DeviceKind::Simulator { booted },
                    detail: format!(
                        "simulator, {runtime}{}",
                        if booted { ", booted" } else { "" }
                    ),
                })
            })
        })
        .collect();
    simulators.sort_by(|a, b| {
        let booted = |d: &Device| matches!(d.kind, DeviceKind::Simulator { booted: true });
        booted(b)
            .cmp(&booted(a))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.detail.cmp(&b.detail))
    });
    simulators
}

/// "iOS 27.0" from "com.apple.CoreSimulator.SimRuntime.iOS-27-0"; `None`
/// for runtimes that cannot run the family's app.
fn runtime_label(family: Family, runtime: &str) -> Option<String> {
    let os = family.runtime_os();
    let version = runtime
        .rsplit('.')
        .next()?
        .strip_prefix(os)?
        .strip_prefix('-')?;
    Some(format!("{os} {}", version.replace('-', ".")))
}

/// The device a `--device` query names: an exact identifier, an exact
/// name (ignoring case), or the one device whose name contains it.
pub fn find<'a>(devices: &'a [Device], query: &str) -> Result<&'a Device> {
    if let Some(device) = devices.iter().find(|d| d.id.eq_ignore_ascii_case(query)) {
        return Ok(device);
    }
    // Two runtimes can each carry a simulator of the same model, so an
    // exact name is only as good as it is unique.
    let wanted = query.to_lowercase();
    let exact: Vec<&Device> = devices
        .iter()
        .filter(|d| d.name.to_lowercase() == wanted)
        .collect();
    let matches = if exact.is_empty() {
        devices
            .iter()
            .filter(|d| d.name.to_lowercase().contains(&wanted))
            .collect()
    } else {
        exact
    };
    match matches.as_slice() {
        [device] => Ok(device),
        [] => bail!("no device matches {query:?}\n{}", listing(devices)),
        _ => bail!(
            "{query:?} matches more than one device; pick one by id or number\n{}",
            listing(devices)
        ),
    }
}

/// The picker's default: the device chosen last time, else the first.
pub fn default_choice(devices: &[Device], remembered: Option<&str>) -> usize {
    remembered
        .and_then(|id| devices.iter().position(|d| d.id == id))
        .unwrap_or(0)
}

/// Reads one picker answer: blank takes the default, a number picks from
/// the list, anything else is a `--device` style query.
pub fn choose(devices: &[Device], answer: &str, default: usize) -> Result<usize> {
    let answer = answer.trim();
    if answer.is_empty() {
        return Ok(default);
    }
    if let Ok(number) = answer.parse::<usize>() {
        if (1..=devices.len()).contains(&number) {
            return Ok(number - 1);
        }
        bail!("pick a number from 1 to {}", devices.len());
    }
    let device = find(devices, answer)?;
    Ok(devices
        .iter()
        .position(|d| d == device)
        .expect("find returns one of these devices"))
}

/// The numbered device list the picker shows.
pub fn listing(devices: &[Device]) -> String {
    let mut out = String::new();
    for (i, device) in devices.iter().enumerate() {
        let _ = writeln!(out, "  {}) {device}", i + 1);
    }
    out
}

/// Builds, installs, and launches the family's app. `device` skips the
/// picker; `list` prints the targets and stops.
pub fn run(family: Family, device: Option<&str>, list: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("{} needs macOS with Xcode", family.command());
    }
    let root = workspace_root();
    let devices = discover(family)?;
    if list {
        print!("{}", listing(&devices));
        return Ok(());
    }
    let remembered_path = root
        .join("target")
        .join(format!("{}-device", family.slug()));
    let target = match device {
        Some(query) => find(&devices, query)?.clone(),
        None => pick(
            &devices,
            std::fs::read_to_string(&remembered_path).ok().as_deref(),
        )?,
    };
    // Best effort: forgetting the last device only costs a keypress.
    let _ = std::fs::create_dir_all(root.join("target"));
    let _ = std::fs::write(&remembered_path, &target.id);

    println!("building for {target}");
    let app = build(family, &root, &target)?;
    let bundle = bundle_id(&app.join("Info.plist"))?;
    match target.kind {
        DeviceKind::Physical => install_physical(&target, &app, &bundle),
        DeviceKind::Simulator { booted } => install_simulator(&target, booted, &app, &bundle),
    }
}

pub(crate) fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Paired devices first, then simulators.
fn discover(family: Family) -> Result<Vec<Device>> {
    combine(family, list_physical(family), list_simulators(family))
}

/// Joins both discoveries. A tool that fails is reported rather than read
/// as "no devices": its error prints as a warning when the other tool
/// found something to run on, and becomes the error when nothing did.
pub fn combine(
    family: Family,
    physical: std::result::Result<Vec<Device>, String>,
    simulators: std::result::Result<Vec<Device>, String>,
) -> Result<Vec<Device>> {
    let mut devices = Vec::new();
    let mut problems = Vec::new();
    for found in [physical, simulators] {
        match found {
            Ok(found) => devices.extend(found),
            Err(problem) => problems.push(problem),
        }
    }
    if devices.is_empty() {
        if problems.is_empty() {
            let noun = family.noun();
            bail!(
                "no paired {noun} or {noun} simulator found; pair an {noun} in Xcode or \
                 add an {noun} simulator"
            );
        }
        bail!("no devices found:\n{}", problems.join("\n"));
    }
    for problem in &problems {
        eprintln!("warning: {problem}");
    }
    Ok(devices)
}

fn list_physical(family: Family) -> std::result::Result<Vec<Device>, String> {
    let json = std::env::temp_dir().join(format!("aftermine-devices-{}.json", std::process::id()));
    let output = Command::new("xcrun")
        .args(["devicectl", "list", "devices", "--quiet", "--json-output"])
        .arg(&json)
        .output()
        .map_err(|error| {
            format!("could not run xcrun devicectl (is full Xcode installed?): {error}")
        })?;
    let text = std::fs::read_to_string(&json).unwrap_or_default();
    let _ = std::fs::remove_file(&json);
    if !output.status.success() {
        return Err(format!("xcrun devicectl failed:\n{}", tool_output(&output)));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("xcrun devicectl wrote unreadable JSON: {error}"))?;
    Ok(physical_devices(family, &value))
}

fn list_simulators(family: Family) -> std::result::Result<Vec<Device>, String> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "--json"])
        .output()
        .map_err(|error| format!("could not run xcrun simctl: {error}"))?;
    if !output.status.success() {
        return Err(format!("xcrun simctl failed:\n{}", tool_output(&output)));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("xcrun simctl printed unreadable JSON: {error}"))?;
    Ok(simulators(family, &value))
}

fn pick(devices: &[Device], remembered: Option<&str>) -> Result<Device> {
    if let [only] = devices {
        return Ok(only.clone());
    }
    if !std::io::stdin().is_terminal() {
        bail!(
            "several devices are available; name one with --device\n{}",
            listing(devices)
        );
    }
    let default = default_choice(devices, remembered.map(str::trim));
    println!("Devices:\n{}", listing(devices));
    loop {
        print!("Choose a device [{}]: ", default + 1);
        std::io::stdout().flush()?;
        let mut answer = String::new();
        if std::io::stdin().lock().read_line(&mut answer)? == 0 {
            bail!("no device chosen");
        }
        match choose(devices, &answer, default) {
            Ok(index) => return Ok(devices[index].clone()),
            Err(error) => eprintln!("{error}"),
        }
    }
}

fn build(family: Family, root: &Path, target: &Device) -> Result<PathBuf> {
    let derived = format!("target/{}-xcode", family.slug());
    // Registering lets the first build for a newly paired device sign for it.
    let status = Command::new("xcodebuild")
        .current_dir(root)
        .args(["-project", "apple/Aftermine.xcodeproj", "-scheme"])
        .arg(family.scheme())
        .args(["-configuration", "Release", "-derivedDataPath"])
        .arg(&derived)
        .args([
            "-allowProvisioningUpdates",
            "-allowProvisioningDeviceRegistration",
            "-quiet",
            "-destination",
        ])
        .arg(format!("id={}", target.id))
        .arg("build")
        .status()
        .context("running xcodebuild")?;
    if !status.success() {
        bail!("xcodebuild failed");
    }
    Ok(root
        .join(derived)
        .join("Build/Products")
        .join(family.products(target.kind))
        .join("Aftermine.app"))
}

/// The bundle ID the build signed, read from the app's `Info.plist`, which
/// honors a contributor's own ID from `apple/Local.xcconfig`.
pub(crate) fn bundle_id(info_plist: &Path) -> Result<String> {
    let output = Command::new("plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(info_plist)
        .output()
        .context("reading the app's bundle ID")?;
    if !output.status.success() {
        bail!("could not read the bundle ID from {}", info_plist.display());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn install_physical(target: &Device, app: &Path, bundle: &str) -> Result<()> {
    println!("installing on {}", target.name);
    let install = Command::new("xcrun")
        .args([
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            &target.id,
        ])
        .arg(app)
        .output()
        .context("running xcrun devicectl")?;
    if !install.status.success() {
        bail!("install failed:\n{}", tool_output(&install));
    }
    println!("launching");
    let launch = Command::new("xcrun")
        .args([
            "devicectl",
            "device",
            "process",
            "launch",
            "--device",
            &target.id,
        ])
        .args(["--terminate-existing", bundle])
        .output()
        .context("running xcrun devicectl")?;
    if launch.status.success() {
        println!("running on {}", target.name);
        return Ok(());
    }
    let report = tool_output(&launch);
    if report.contains("Locked") || report.contains("unlocked") {
        println!(
            "installed; {} is locked, so unlock it and open Aftermine yourself",
            target.name
        );
        return Ok(());
    }
    bail!("launch failed:\n{report}")
}

fn install_simulator(target: &Device, booted: bool, app: &Path, bundle: &str) -> Result<()> {
    if !booted {
        println!("booting {}", target.name);
        let boot = Command::new("xcrun")
            .args(["simctl", "boot", &target.id])
            .output()
            .context("running xcrun simctl")?;
        if !boot.status.success() {
            bail!("boot failed:\n{}", tool_output(&boot));
        }
    }
    // Brings the Simulator window forward; the app runs either way, and
    // some Xcode installs ship no standalone Simulator app to open.
    let shown = Command::new("open")
        .args(["-a", "Simulator"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !shown {
        println!("no Simulator app to open; use Xcode > Open Developer Tool > Simulator to watch");
    }
    println!("installing on {}", target.name);
    let install = Command::new("xcrun")
        .args(["simctl", "install", &target.id])
        .arg(app)
        .output()
        .context("running xcrun simctl")?;
    if !install.status.success() {
        bail!("install failed:\n{}", tool_output(&install));
    }
    let launch = Command::new("xcrun")
        .args([
            "simctl",
            "launch",
            "--terminate-running-process",
            &target.id,
            bundle,
        ])
        .output()
        .context("running xcrun simctl")?;
    if !launch.status.success() {
        bail!("launch failed:\n{}", tool_output(&launch));
    }
    println!("running on {}", target.name);
    Ok(())
}

pub(crate) fn tool_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(test)]
mod tests;
