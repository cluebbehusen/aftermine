//! One owner for every directory Aftermine writes: the per-OS config and
//! data roots, the iCloud Drive folder, and the derived subdirectories the
//! persistence sites share. Path policy lives here so no feature needs its
//! own `#[cfg]` block.

use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
use crate::platform::{PLATFORM, Platform};

/// Platform config directory for Aftermine, created on save, never on load.
/// Apple TV has none: it keeps settings in user defaults.
pub fn config_dir() -> Option<PathBuf> {
    // An iOS or tvOS app's HOME is its sandbox container, which keeps the
    // same Library layout as a Mac home.
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
    {
        std::env::var_os("HOME").and_then(|h| apple_config_dir(Path::new(&h), PLATFORM))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Aftermine"))
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "windows"
    )))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("aftermine"))
    }
}

/// Platform data root (autosaves, the LAN rejoin ticket), if resolvable.
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
    {
        std::env::var_os("HOME").map(|h| apple_data_dir(Path::new(&h), PLATFORM))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Aftermine"))
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "windows"
    )))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("aftermine"))
    }
}

/// The config directory under an Apple `home`, which is the data root,
/// except on Apple TV, which keeps settings in user defaults instead.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
fn apple_config_dir(home: &Path, platform: Platform) -> Option<PathBuf> {
    (platform != Platform::Tv).then(|| apple_data_dir(home, platform))
}

/// The data root under an Apple `home`. Apple TV keeps files only in
/// Caches, which it may clear while the app is closed.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
fn apple_data_dir(home: &Path, platform: Platform) -> PathBuf {
    let library = home.join("Library");
    match platform {
        Platform::Tv => library.join("Caches/Aftermine"),
        Platform::Desktop | Platform::Touch => library.join("Application Support/Aftermine"),
    }
}

/// Root of the files a player keeps on this device: diagnostics, and the
/// records iCloud does not keep. On iOS that is the app's Documents
/// folder, which the Files app shows under On My iPad; elsewhere it is the
/// data root.
fn player_files_dir() -> Option<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents"))
    }
    #[cfg(not(target_os = "ios"))]
    {
        data_dir()
    }
}

/// The ticket of a LAN seat this player may still take back.
pub fn rejoin_ticket() -> Option<PathBuf> {
    data_dir().map(|d| d.join("rejoin.json"))
}

/// Crash and freeze incident logs and exported diagnostic reports, which
/// stay on the device that wrote them.
pub fn diagnostics_dir() -> Option<PathBuf> {
    player_files_dir().map(|directory| directory.join("diagnostics"))
}

/// Where each kind of record lives under one root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folders {
    autosaves: Option<PathBuf>,
    matches: Option<PathBuf>,
    saves: Option<PathBuf>,
    replays: PathBuf,
}

impl Folders {
    /// Every record folder under `root`, as iCloud keeps them.
    fn under(root: &Path) -> Self {
        Self {
            autosaves: Some(root.join("autosaves")),
            matches: Some(root.join("matches")),
            saves: Some(root.join("saves")),
            replays: root.join("replays"),
        }
    }

    /// This device's folders. Autosaves stay out of the player's sight;
    /// a packaged bundle's cwd is not a usable root (typically `/`), so it
    /// keeps replays with the player's files, while a workspace run keeps
    /// the documented cwd-relative `replays/`.
    fn local() -> Self {
        let player = player_files_dir();
        Self {
            autosaves: data_dir().map(|d| d.join("autosaves")),
            matches: player.as_ref().map(|d| d.join("matches")),
            saves: player.as_ref().map(|d| d.join("saves")),
            replays: player
                .filter(|_| bundled())
                .map_or_else(|| PathBuf::from("replays"), |d| d.join("replays")),
        }
    }

    fn all(&self) -> impl Iterator<Item = PathBuf> {
        [
            self.autosaves.clone(),
            self.matches.clone(),
            self.saves.clone(),
            Some(self.replays.clone()),
        ]
        .into_iter()
        .flatten()
    }
}

/// Where records go and where the shelf finds them: the iCloud Drive
/// folder when iCloud keeps them, this device's folders otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Records {
    cloud: Option<Folders>,
    local: Folders,
}

impl Records {
    /// Records in iCloud Drive under `documents`, if it is available.
    fn with_cloud(documents: Option<&Path>, local: Folders) -> Self {
        Self {
            cloud: documents.map(Folders::under),
            local,
        }
    }

    fn writing(&self) -> &Folders {
        self.cloud.as_ref().unwrap_or(&self.local)
    }

    /// Where the autosave rotation lives; Continue resumes from here.
    pub fn autosaves(&self) -> Option<PathBuf> {
        self.writing().autosaves.clone()
    }

    /// Every folder that may hold a match's autosave: the rotation's, then,
    /// with iCloud on, this device's, where a session left before iCloud
    /// was on still waits.
    pub fn autosave_folders(&self) -> Vec<PathBuf> {
        self.cloud
            .iter()
            .chain([&self.local])
            .filter_map(|folders| folders.autosaves.clone())
            .collect()
    }

    /// Finished-match recordings, kept apart from the autosaves they
    /// replace so a player can find and share them.
    pub fn matches(&self) -> Option<PathBuf> {
        self.writing().matches.clone()
    }

    /// Explicit, player-initiated saves. Autosave rotation never touches
    /// this folder: a save the player asked for is deleted only by the
    /// player.
    pub fn saves(&self) -> Option<PathBuf> {
        self.writing().saves.clone()
    }

    /// Every folder the shelf lists. With iCloud on, this device's folders
    /// follow iCloud's, so records kept before iCloud was on stay in reach.
    pub fn listed(&self) -> Vec<PathBuf> {
        self.cloud
            .iter()
            .chain([&self.local])
            .flat_map(Folders::all)
            .collect()
    }

    /// The folders whose finished matches withdraw older sessions from
    /// Continue.
    pub fn match_folders(&self) -> Vec<PathBuf> {
        self.cloud
            .iter()
            .chain([&self.local])
            .filter_map(|folders| folders.matches.clone())
            .collect()
    }

    /// The folders iCloud keeps, which may hold files not on this device
    /// yet.
    pub fn cloud_folders(&self) -> Vec<PathBuf> {
        self.cloud.iter().flat_map(Folders::all).collect()
    }

    /// The folders new records go to, each with the name it takes in the
    /// CloudKit mirror, so a record keeps one name on every device.
    pub fn mirrored(&self) -> Vec<(&'static str, PathBuf)> {
        let folders = self.writing();
        [
            ("autosaves", folders.autosaves.clone()),
            ("matches", folders.matches.clone()),
            ("saves", folders.saves.clone()),
            ("replays", Some(folders.replays.clone())),
        ]
        .into_iter()
        .filter_map(|(name, dir)| dir.map(|dir| (name, dir)))
        .collect()
    }
}

/// What this device last agreed with the CloudKit mirror. It sits beside
/// the device's own records, so on Apple TV a cleared cache takes it along
/// and the TV downloads its records again instead of reading their loss as
/// deletes.
pub fn mirror_manifest() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("cloud-mirror.json"))
}

/// Whether the system may remove record files on its own: Apple TV keeps
/// them in its cache.
pub fn records_purgeable() -> bool {
    PLATFORM == Platform::Tv
}

/// Where downloaded records wait before they move into place.
pub fn mirror_staging() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("cloud-staging"))
}

/// Where records live now. The first iCloud lookup may still be running,
/// and this waits for it, so only worker threads call it.
pub fn records() -> Records {
    Records::with_cloud(CLOUD.answer().as_deref(), Folders::local())
}

/// Looks up the iCloud Drive folder on a background thread, since iCloud
/// can take seconds to answer. The app calls this at launch and whenever
/// it returns to the foreground, where the player may have signed out or
/// switched iCloud Drive off meanwhile. Only a packaged copy looks, so a
/// workspace run never touches iCloud.
pub fn look_up_cloud() {
    if bundled() {
        CLOUD.start(aftermine_icloud::documents);
    }
}

/// How many times a later lookup found a different iCloud Drive folder, or
/// none where there was one. A catalog scanned before a change lists
/// records the player can no longer reach, or misses new ones.
pub fn cloud_changes() -> u64 {
    CLOUD.changes.load(std::sync::atomic::Ordering::Acquire)
}

static CLOUD: Lookup = Lookup::new();

/// The iCloud Drive folder as last looked up.
struct Lookup {
    state: std::sync::Mutex<LookupState>,
    answered: std::sync::Condvar,
    changes: std::sync::atomic::AtomicU64,
}

struct LookupState {
    /// Whether a lookup has finished yet.
    answered: bool,
    /// The newest answer.
    folder: Option<PathBuf>,
    looking: bool,
}

impl Lookup {
    const fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(LookupState {
                answered: false,
                folder: None,
                looking: false,
            }),
            answered: std::sync::Condvar::new(),
            changes: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LookupState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts `find` on its own thread unless a lookup is already running.
    /// Earlier answers stand until it finishes.
    fn start(&'static self, find: fn() -> Option<PathBuf>) {
        {
            let mut state = self.lock();
            if state.looking {
                return;
            }
            state.looking = true;
        }
        let spawned = std::thread::Builder::new()
            .name("icloud-lookup".into())
            .spawn(move || match std::panic::catch_unwind(find) {
                Ok(found) => self.finish(found),
                Err(_) => self.give_up(),
            });
        if spawned.is_err() {
            self.give_up();
        }
    }

    /// Ends a lookup with the folder it `found`.
    fn finish(&self, found: Option<PathBuf>) {
        let mut state = self.lock();
        if state.answered && state.folder != found {
            self.changes
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        state.folder = found;
        state.answered = true;
        state.looking = false;
        self.answered.notify_all();
    }

    /// Ends a lookup that could not answer, leaving the earlier answer
    /// standing, or no folder at all.
    fn give_up(&self) {
        let mut state = self.lock();
        state.answered = true;
        state.looking = false;
        self.answered.notify_all();
    }

    /// The iCloud Drive folder, waiting out a first lookup still running.
    /// Without a lookup, there is none.
    fn answer(&self) -> Option<PathBuf> {
        let mut state = self.lock();
        while !state.answered && state.looking {
            state = self
                .answered
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.folder.clone()
    }
}

/// Whether this executable runs from a packaged bundle: the copy
/// players run, as opposed to a workspace build. Probed once.
pub fn bundled() -> bool {
    static BUNDLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *BUNDLED.get_or_init(|| bundle_resources().is_some())
}

/// Where a packaged bundle keeps its read-only resources, if this
/// executable runs from one: `Contents/Resources` beside
/// `Contents/MacOS/<exe>` in a macOS .app, or the directory that holds
/// the executable itself in an iOS app or a desktop release folder.
/// Found by probing for the atlas, the one file no build ships without.
pub fn bundle_resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundle_resources_beside(exe.parent()?)
}

fn bundle_resources_beside(exe_dir: &Path) -> Option<PathBuf> {
    [exe_dir.join("../Resources"), exe_dir.to_path_buf()]
        .into_iter()
        .find(|root| root.join("assets/sprites/atlas.png").exists())
}

#[cfg(test)]
mod tests;
