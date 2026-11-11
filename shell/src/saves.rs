//! The shelf classifies resumable checkpoints and watchable recordings.
//! Unavailable revisions stay visible with a reason; malformed files are skipped.

use crate::numeric::Fit;
pub use crate::saved_game::Unavailable;
#[cfg(test)]
use aftermine_sim::SIM_VERSION;
use std::path::PathBuf;

/// What a record on disk is, read from its metadata `kind` tag; an
/// untagged record is a finished match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// A live session saved on its own as it plays.
    Autosave,
    /// A player-named explicit save.
    Save,
    /// A finished match, kept to watch.
    Match,
    /// A LAN match the host saved by name.
    LanSave,
    /// A LAN match the host saved on its own.
    LanAutosave,
}

impl RecordKind {
    /// Whether the shelf's verb for this record is Load. Autosaves and
    /// saves are live sessions: watching one fog-free mid-match would
    /// scout the enemy, so they resume instead.
    pub fn resumable(self) -> bool {
        !matches!(self, RecordKind::Match)
    }

    /// Whether Continue picks up this record: the autosaves, local or LAN,
    /// and named local saves. A LAN host's leave always writes an autosave,
    /// so its named saves are never a session's latest.
    pub fn continues(self) -> bool {
        matches!(
            self,
            RecordKind::Autosave | RecordKind::Save | RecordKind::LanAutosave
        )
    }

    /// The chip a shelf row wears beside its title, for the kinds its
    /// tab does not already imply.
    pub fn tag(self) -> Option<&'static str> {
        match self {
            RecordKind::Autosave => Some("AUTO"),
            RecordKind::LanSave => Some("LAN"),
            RecordKind::LanAutosave => Some("LAN AUTO"),
            RecordKind::Save | RecordKind::Match => None,
        }
    }
}

pub(crate) fn record_kind(meta: &chassis::replay::ReplayMeta) -> RecordKind {
    match meta.kind.as_deref() {
        Some("autosave") => RecordKind::Autosave,
        Some("save") => RecordKind::Save,
        Some("lan") => RecordKind::LanSave,
        Some("lan-autosave") => RecordKind::LanAutosave,
        _ => RecordKind::Match,
    }
}

/// One record on the shelf.
pub struct ReplayEntry {
    /// File on disk.
    pub path: PathBuf,
    /// The record's name, or its map when nobody named it.
    pub title: String,
    /// The map, when the title is a name the player gave instead.
    pub map: Option<String>,
    /// The match's format: `1v1`, `2v2`, `4-way FFA`.
    pub format: String,
    /// Match time when the record was written.
    pub clock: String,
    /// When the record was saved, in unix seconds.
    pub saved_secs: u64,
    /// Why this build cannot open the record, if it cannot.
    pub problem: Option<Unavailable>,
    /// What the record is; decides its shelf section and verb.
    pub kind: RecordKind,
}

impl ReplayEntry {
    /// Whether this build can load or watch the record.
    pub fn available(&self) -> bool {
        self.problem.is_none()
    }

    /// How to act on the row, for the hands the player is using.
    /// `on_delete` says the menu cursor stands on the row's Delete button.
    pub fn hint(&self, hands: crate::platform::Hands, on_delete: bool) -> String {
        let action = if on_delete {
            Some(("deletes", "delete"))
        } else {
            match (self.problem, self.kind.resumable()) {
                (Some(_), _) => None,
                (None, true) => Some(("loads paused", "load paused")),
                (None, false) => Some(("watches", "watch")),
            }
        };
        entry_hint(action, hands, on_delete)
    }
}

/// How to act on a shelf row: activate it, where it can be activated,
/// and delete it by key or button. A finger or a pointer alone deletes
/// with the row's own Delete button, which says so itself. The Siri Remote
/// has no delete button, so it swipes right onto the row's. Once the cursor
/// stands there, `action` is the delete itself.
fn entry_hint(
    action: Option<(&str, &str)>,
    hands: crate::platform::Hands,
    on_delete: bool,
) -> String {
    let remote = hands.pad == Some(aftermine_protocol::PadFamily::Remote);
    let by_key = hands.pad.is_some() || (hands.keys && !hands.touch());
    let verb = if hands.touch() { "tap" } else { "click" };
    let activate = action.map(|(key_action, action)| {
        if remote {
            format!("{{confirm}} to {action}")
        } else if by_key {
            format!("{{confirm}} {key_action}")
        } else {
            format!("{verb} to {action}")
        }
    });
    let delete = if on_delete {
        None
    } else if remote {
        Some("Swipe right to delete".to_string())
    } else {
        (hands.pad.is_some() || hands.keys).then(|| "{delete} deletes".to_string())
    };
    activate
        .into_iter()
        .chain(delete)
        .collect::<Vec<_>>()
        .join(" | ")
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// When a record was saved, as of `now` (both unix seconds): minutes,
/// hours, then days for a week, then its UTC calendar date, with the
/// year only when it is not this one. A save stamped in the future (a
/// clock moved back) reads as just now.
pub fn saved_ago(saved: u64, now: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    let elapsed = now.saturating_sub(saved);
    match elapsed {
        ..MINUTE => "just now".to_string(),
        MINUTE..HOUR => format!("{} min ago", elapsed / MINUTE),
        HOUR..DAY => format!("{} h ago", elapsed / HOUR),
        DAY..WEEK => match elapsed / DAY {
            1 => "1 day ago".to_string(),
            days => format!("{days} days ago"),
        },
        _ => {
            let (year, month, day) = civil(saved);
            let month = MONTHS[month as usize - 1];
            if year == civil(now).0 {
                format!("{month} {day}")
            } else {
                format!("{month} {day}, {year}")
            }
        }
    }
}

/// Days-since-epoch to a civil (year, month, day) in UTC (Howard
/// Hinnant's algorithm): enough calendar for a shelf row without pulling
/// a time crate.
fn civil(secs: u64) -> (i64, u32, u32) {
    let days = (secs / 86_400).fit::<i64>();
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m.fit::<u32>(), d.fit::<u32>())
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RecordTime {
    saved_at: std::time::SystemTime,
    modified: std::time::SystemTime,
}

#[cfg(test)]
fn scan(dir: &std::path::Path, out: &mut Vec<(RecordTime, ReplayEntry)>) {
    scan_cancellable(dir, out, &|_| false, &|| false);
}

/// Lists the records in `dir`, leaving out any file `dataless` says iCloud
/// has not brought down yet: reading one would wait on its download, and
/// offline the read fails. [`aftermine_icloud::download`] counts those as
/// on their way.
fn scan_cancellable(
    dir: &std::path::Path,
    out: &mut Vec<(RecordTime, ReplayEntry)>,
    dataless: &impl Fn(&std::path::Path) -> bool,
    cancelled: &impl Fn() -> bool,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
    {
        if cancelled() {
            break;
        }
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("json" | "amreplay" | "amsave")
        ) || dataless(&path)
        {
            continue;
        }
        let Ok(replay) = crate::saved_game::inspect(&path) else {
            continue;
        };
        let kind = record_kind(&replay.meta);
        // A record's own saved_at outranks mtime: a copied or synced
        // file reports the copy date, and only the metadata records when
        // the save was made.
        let modified = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let saved_at = replay
            .meta
            .saved_at
            // checked: a copied or hand-edited record can carry any
            // u64, and an out-of-range timestamp must fall back to
            // mtime, not panic the whole shelf scan.
            .and_then(|secs| {
                std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs))
            })
            .unwrap_or(modified);
        let saved_secs = saved_at
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let name = replay.meta.description.clone();
        let map = name
            .as_ref()
            .is_some_and(|name| *name != replay.map)
            .then(|| replay.map.clone());
        out.push((
            RecordTime { saved_at, modified },
            ReplayEntry {
                path,
                title: name.unwrap_or_else(|| replay.map.clone()),
                map,
                format: crate::menu::match_format(replay.seats, replay.teams),
                clock: crate::clock_label::ticks(replay.meta.ticks.unwrap_or(0)),
                saved_secs,
                problem: replay.problem,
                kind,
            },
        ));
    }
}

/// Every record the shelf can list, and what is still on its way.
pub struct Catalog {
    /// Every known record, newest first.
    pub entries: Vec<ReplayEntry>,
    /// Records iCloud holds that are still downloading to this device.
    pub downloading: usize,
    /// The folders whose finished matches end Continue's offers.
    matches: Vec<PathBuf>,
}

impl Catalog {
    /// Continue's material: the available records it picks up, left since
    /// the player's newest finished match. A finished match withdraws the
    /// sessions left before it from Continue; the shelf still offers them.
    pub(crate) fn continuable(&self) -> impl Iterator<Item = &ReplayEntry> {
        continuable_since(&self.entries, &self.matches)
    }
}

/// Every known record, newest first, after starting downloads of the ones
/// iCloud holds but this device does not, which it leaves out and counts as
/// downloading. Waits on the first iCloud lookup, so only the save worker
/// runs it.
pub fn discover(cancelled: impl Fn() -> bool) -> Catalog {
    discover_with(&crate::paths::dataless, cancelled)
}

/// [`discover`] for Continue, which also reads the records iCloud has not
/// brought down yet, waiting on their downloads, so a newer session on its
/// way is found before an older one resumes. A download that cannot finish,
/// as when the device is offline, leaves its record out.
pub(crate) fn discover_arrived(cancelled: impl Fn() -> bool) -> Catalog {
    discover_with(&|_| false, cancelled)
}

fn discover_with(
    dataless: &impl Fn(&std::path::Path) -> bool,
    cancelled: impl Fn() -> bool,
) -> Catalog {
    let records = crate::paths::records();
    let downloading = records
        .cloud_folders()
        .iter()
        .map(|dir| aftermine_icloud::download(dir))
        .sum();
    let mut found = Vec::new();
    for dir in records.listed() {
        scan_cancellable(&dir, &mut found, dataless, &cancelled);
    }
    Catalog {
        entries: newest_first(found),
        downloading,
        matches: records.match_folders(),
    }
}

/// Only the matches folders record what this player finished; a recording
/// added to the replays folder says nothing about their last match.
fn continuable_since<'a>(
    entries: &'a [ReplayEntry],
    matches: &'a [PathBuf],
) -> impl Iterator<Item = &'a ReplayEntry> {
    entries
        .iter()
        .take_while(move |entry| {
            entry.kind != RecordKind::Match
                || !entry
                    .path
                    .parent()
                    .is_some_and(|parent| matches.iter().any(|folder| folder == parent))
        })
        .filter(|entry| entry.available() && entry.kind.continues())
}

fn newest_first(mut found: Vec<(RecordTime, ReplayEntry)>) -> Vec<ReplayEntry> {
    found.sort_by(|(left_time, left), (right_time, right)| {
        (right_time, &right.path).cmp(&(left_time, &left.path))
    });
    found.into_iter().map(|(_, entry)| entry).collect()
}

#[cfg(test)]
mod tests;
