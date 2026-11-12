//! Launching and driving a real shell window. `launch` is the one way the
//! driver starts a shell it then talks to: profiling launches its own, and
//! [`ScratchShell`] runs the automation shell on a throwaway HOME for the
//! smoke check, the screenshot suite and the native test suites. The rest is
//! driving vocabulary for the automation-mode shell, which ignores hardware
//! input: read the shell's own UI report, and walk menus by row label — never
//! by index, because Home's rows shift with resumable state on the host
//! machine.

use crate::client::Client;
use aftermine_protocol::{Key, PadButton, PadFamily, RawEvent, Request, UiView};
use anyhow::{Context, Result, bail};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A shell child killed on drop, so a failing walk never strands a
/// window holding the port.
pub(crate) struct ShellGuard(std::process::Child);

impl ShellGuard {
    /// The shell's process id.
    pub(crate) fn id(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for ShellGuard {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Builds the shell with the caller's normal Rust environment and returns
/// Cargo's exact executable path. The separation matters: an isolated HOME
/// belongs on the game process, not the rustup shim that launches Cargo.
pub(crate) fn build_shell_executable() -> Result<PathBuf> {
    build_shell_executable_for(false)
}

/// Builds either the ordinary development shell or the optimized shell used
/// for native frame profiling. Callers choose explicitly because debug-build
/// timing is not representative of the shipped executable.
pub(crate) fn build_shell_executable_for(release: bool) -> Result<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut command = std::process::Command::new("cargo");
    command.args([
        "build",
        "-p",
        "aftermine-shell",
        "--locked",
        "--message-format=json-render-diagnostics",
    ]);
    if release {
        command.arg("--release");
    }
    let output = command
        .current_dir(&root)
        .output()
        .context("building aftermine-shell via cargo")?;
    if !output.status.success() {
        let rendered = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|message| message["message"]["rendered"].as_str().map(str::to_owned))
            .collect::<String>();
        bail!(
            "building aftermine-shell failed:\n{rendered}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    shell_executable_from_cargo_output(&output.stdout)
        .context("cargo did not report the Aftermine executable")
}

fn shell_executable_from_cargo_output(stdout: &[u8]) -> Option<PathBuf> {
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == "Aftermine"
            && let Some(executable) = message["executable"].as_str()
        {
            return Some(PathBuf::from(executable));
        }
    }
    None
}

/// Points every platform's writable roots at `home`. `HOME` alone is not
/// hermetic: Windows resolves config and autosaves through `APPDATA`, and
/// Linux prefers `XDG_CONFIG_HOME` and `XDG_DATA_HOME` over `HOME` when they
/// are set.
pub(crate) fn isolate_home(command: &mut std::process::Command, home: &Path) {
    command.env("HOME", home);
    command.env("XDG_CONFIG_HOME", home.join(".config"));
    command.env("XDG_DATA_HOME", home.join(".local/share"));
    command.env("APPDATA", home.join("AppData"));
}

/// Writes `text` as the settings file under an isolated `home`, wherever each
/// platform's shell looks for it.
pub fn write_home_config(home: &Path, text: &str) -> Result<()> {
    for dir in [
        home.join("Library/Application Support/Aftermine"),
        home.join(".config/aftermine"),
        home.join("AppData/Aftermine"),
    ] {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("config.json"), text)?;
    }
    Ok(())
}

/// A throwaway HOME and the automation shell spawned on it. Dropping it
/// stops the shell before removing the HOME, so a last autosave never lands
/// in a tree being deleted and a failing walk never strands a window holding
/// the port.
pub struct ScratchShell {
    shell: Option<ShellGuard>,
    home: PathBuf,
}

impl ScratchShell {
    /// Creates an empty HOME under the system temporary directory, named
    /// after `label`, with no shell on it yet. Anything the shell should find
    /// there, such as [`write_home_config`]'s settings, goes in before
    /// [`ScratchShell::spawn`].
    pub fn new(label: &str) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let home =
                std::env::temp_dir().join(format!("aftermine-{label}-{}-{id}", std::process::id()));
            match std::fs::create_dir(&home) {
                Ok(()) => return Ok(Self { shell: None, home }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// The HOME, which is also the shell's working directory.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Builds and spawns an automation-mode shell on this HOME, after
    /// stopping any shell already running on it, and connects once it
    /// answers on `port`. The window is pinned to 1280x800 because the walks
    /// driving it depend on stable geometry, such as a camera that can move
    /// on a small map, and a persisted config may carry another size.
    pub fn spawn(&mut self, port: u16, paused: bool) -> Result<Client> {
        self.shell = None;
        // Read-only resources stay rooted at the workspace while the
        // writable working directory is the HOME.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut command = std::process::Command::new(build_shell_executable()?);
        command
            .args([
                "--debug-server",
                "--automation",
                "--window",
                "1280x800",
                "--port",
                &port.to_string(),
            ])
            .env("AFTERMINE_RESOURCE_ROOT", &root)
            .current_dir(&self.home);
        if paused {
            command.arg("--paused");
        }
        isolate_home(&mut command, &self.home);
        let (shell, client) = launch(command, port, Duration::from_secs(120))?;
        self.shell = Some(shell);
        Ok(client)
    }
}

impl Drop for ScratchShell {
    fn drop(&mut self) {
        drop(self.shell.take());
        if let Err(error) = std::fs::remove_dir_all(&self.home)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "warning: could not remove scratch HOME {}: {error}",
                self.home.display()
            );
        }
    }
}

/// Spawns `command`, a shell told to serve the debug protocol on `port`, and
/// connects once it answers there, waiting up to `patience`.
///
/// A port that already answers is refused before anything spawns: the
/// client would reach that server, not the new shell. A shell that exits
/// before it answers is reported with its exit status as soon as it is
/// gone.
pub(crate) fn launch(
    mut command: std::process::Command,
    port: u16,
    patience: Duration,
) -> Result<(ShellGuard, Client)> {
    const POLL: Duration = Duration::from_millis(250);
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok() {
        bail!("a server already answers on {address}; stop it or choose another port");
    }
    let mut shell = ShellGuard(command.spawn().context("spawning the Aftermine shell")?);
    let deadline = Instant::now() + patience;
    loop {
        if let Some(status) = shell.0.try_wait().context("checking on the shell")? {
            bail!("the shell exited before it answered on {address} ({status})");
        }
        if let Ok(client) = Client::connect(&address.to_string()) {
            return Ok((shell, client));
        }
        if Instant::now() >= deadline {
            bail!(
                "the shell never answered on {address} within {}s",
                patience.as_secs()
            );
        }
        std::thread::sleep(POLL);
    }
}

/// The shell's own report of what screen it shows.
pub fn ui(client: &mut Client) -> Result<UiView> {
    client.ui()
}

/// Injects one raw event into the real input funnel.
pub fn inject(client: &mut Client, event: RawEvent) -> Result<()> {
    client.call(Request::InjectEvent { event })?;
    Ok(())
}

/// A full key press: down then up.
pub fn press_key(client: &mut Client, key: Key) -> Result<()> {
    inject(client, RawEvent::KeyDown { key })?;
    inject(client, RawEvent::KeyUp { key })
}

/// The pad id scripted controller input uses.
pub const INJECTED_PAD: u64 = 0;

/// Announces the scripted controller, as hardware does before any press.
pub fn connect_pad(client: &mut Client, family: PadFamily) -> Result<()> {
    inject(
        client,
        RawEvent::PadConnected {
            pad: INJECTED_PAD,
            family,
        },
    )
}

/// A full controller button press: down then up.
pub fn press_pad(client: &mut Client, button: PadButton) -> Result<()> {
    inject(
        client,
        RawEvent::PadDown {
            pad: INJECTED_PAD,
            button,
        },
    )?;
    inject(
        client,
        RawEvent::PadUp {
            pad: INJECTED_PAD,
            button,
        },
    )
}

/// A walk of the cursor to a labeled row, steered by where each key
/// press lands rather than by index arithmetic: menus wrap, skip section
/// headings, and grids move by rows, so only the shell knows where a press
/// goes. It presses Down until the cursor reaches the row or comes back to
/// a row it already visited, then Right, which reaches every cell of a
/// grid in reading order.
#[derive(Debug)]
struct Walk {
    target: usize,
    key: Key,
    seen: Vec<usize>,
    presses: usize,
    limit: usize,
}

/// What a walk does next.
#[derive(Debug, PartialEq, Eq)]
enum Stride {
    Press(Key),
    Arrived,
    Stuck,
}

impl Walk {
    /// A walk to the row whose label equals `needle`, or else contains it
    /// (case-insensitive).
    fn to_label(view: &UiView, needle: &str) -> Result<Self> {
        let lower = needle.to_lowercase();
        let target = view
            .items
            .iter()
            .position(|item| item.to_lowercase() == lower)
            .or_else(|| {
                view.items
                    .iter()
                    .position(|item| item.to_lowercase().contains(&lower))
            })
            .with_context(|| format!("no row containing '{needle}' in {:?}", view.items))?;
        Ok(Self {
            target,
            key: Key::Down,
            seen: Vec::new(),
            presses: 0,
            limit: 2 * view.items.len() + 2,
        })
    }

    /// The next key, given the row the cursor rests on.
    fn next(&mut self, selected: usize) -> Stride {
        if selected == self.target {
            return Stride::Arrived;
        }
        if self.seen.contains(&selected) {
            if self.key == Key::Right {
                return Stride::Stuck;
            }
            self.key = Key::Right;
            self.seen.clear();
        }
        if self.presses >= self.limit {
            return Stride::Stuck;
        }
        self.seen.push(selected);
        self.presses += 1;
        Stride::Press(self.key)
    }
}

/// Moves the cursor with keyboard navigation to the row whose label
/// contains `needle` (case-insensitive), without activating it.
pub fn select_labeled(client: &mut Client, needle: &str) -> Result<()> {
    walk_to_label(client, needle, press_key)
}

/// Selects the row whose label contains `needle` (case-insensitive)
/// with keyboard navigation, then activates it with Enter.
pub fn activate_labeled(client: &mut Client, needle: &str) -> Result<()> {
    select_labeled(client, needle)?;
    press_key(client, Key::Enter)
}

/// Like [`activate_labeled`], but on the scripted controller: the D-pad
/// walks and the bottom face button confirms.
pub fn activate_labeled_by_pad(client: &mut Client, needle: &str) -> Result<()> {
    walk_to_label(client, needle, |client, key| {
        let button = match key {
            Key::Down => PadButton::DpadDown,
            Key::Right => PadButton::DpadRight,
            other => bail!("a walk never presses {other:?}"),
        };
        press_pad(client, button)
    })?;
    press_pad(client, PadButton::South)
}

fn walk_to_label(
    client: &mut Client,
    needle: &str,
    mut press: impl FnMut(&mut Client, Key) -> Result<()>,
) -> Result<()> {
    let view = ui(client)?;
    let mut walk = Walk::to_label(&view, needle)?;
    let mut selected = view.selected.unwrap_or(0);
    loop {
        match walk.next(selected) {
            Stride::Press(key) => {
                press(client, key)?;
                selected = ui(client)?.selected.unwrap_or(0);
            }
            Stride::Arrived => return Ok(()),
            Stride::Stuck => bail!("the cursor never reached '{needle}' in {:?}", view.items),
        }
    }
}

/// Steps a tabbed screen (Settings, the saves and replays shelf) with
/// the classic Next tab key until the shell reports `mode`, the wanted
/// tab's mode name.
pub fn select_tab(client: &mut Client, mode: &str) -> Result<()> {
    // The most tabs any screen has.
    const TABS: usize = 4;
    for _ in 0..TABS {
        if ui(client)?.mode == mode {
            return Ok(());
        }
        press_key(client, Key::E)?;
    }
    assert_mode(client, mode, "stepping the tabs")
}

/// Fails loudly when the shell is not on the expected screen.
pub fn assert_mode(client: &mut Client, expected: &str, at: &str) -> Result<()> {
    let mode = ui(client)?.mode;
    if mode != expected {
        bail!("after {at}: expected mode '{expected}', shell reports '{mode}'");
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum ModeWait {
    Arrived,
    Transitional,
    Unexpected,
}

fn classify_mode(mode: &str, expected: &str, transitional: &[&str]) -> ModeWait {
    if mode == expected {
        ModeWait::Arrived
    } else if transitional.contains(&mode) {
        ModeWait::Transitional
    } else {
        ModeWait::Unexpected
    }
}

/// Like [`assert_mode`], but rides out the named `transitional` screens
/// a step legitimately passes through while background work finishes,
/// such as the saving screen that holds a leave until its autosave
/// lands. Any other screen fails at once; a transitional screen fails
/// only if it outlasts a bounded wait.
pub fn wait_for_mode(
    client: &mut Client,
    expected: &str,
    transitional: &[&str],
    at: &str,
) -> Result<()> {
    const TIMEOUT: Duration = Duration::from_secs(10);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let mode = ui(client)?.mode;
        match classify_mode(&mode, expected, transitional) {
            ModeWait::Arrived => return Ok(()),
            ModeWait::Unexpected => {
                bail!("after {at}: expected mode '{expected}', shell reports '{mode}'")
            }
            ModeWait::Transitional if Instant::now() >= deadline => bail!(
                "after {at}: expected mode '{expected}', shell still reports '{mode}' after {}s",
                TIMEOUT.as_secs()
            ),
            ModeWait::Transitional => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests;
