//! CloudKit's private database as a mirror for record files: one zone, one
//! record per file, each carrying the file and when it was saved. Every call
//! waits for CloudKit, which can take seconds, so only a worker thread may
//! make one. Which files go where is the shell's decision.
//!
//! A call CloudKit refuses fails with an [`io::Error`] carrying a
//! [`Failure`], which says what the player could do about it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A record another device changed: a file, copied to this device, or a
/// delete record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Changed {
    /// The record's name: the file's path under the records root.
    pub name: String,
    /// How far the save was played, or the delete record's tick.
    pub ticks: u64,
    /// When the file was saved, in Unix seconds; nothing for a delete record.
    pub saved_at: u64,
    /// The copy of the file, in the staging folder the fetch was given;
    /// `None` for a delete record.
    #[serde(default)]
    pub staged: Option<PathBuf>,
}

/// What changed in the zone since a token.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Changes {
    /// Records added or replaced.
    pub changed: Vec<Changed>,
    /// Where the next fetch resumes, once these changes are applied.
    pub token: Option<String>,
    /// Whether more changes wait beyond this batch.
    pub more: bool,
    /// Changed records that could not be copied to this device. While any
    /// failed, the token must not advance, or they are never fetched again.
    #[serde(default)]
    pub failed: usize,
    /// Whether these changes start from the beginning because CloudKit no
    /// longer knew the token or lost the zone: what the caller last agreed
    /// with it no longer holds.
    #[serde(default)]
    pub restarted: bool,
}

/// What the player could do about a call CloudKit refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trouble {
    /// The player's iCloud storage is full.
    StorageFull,
    /// No iCloud account CloudKit can act for: the device is signed out or
    /// restricted, or the build has no CloudKit entitlement.
    NoAccount,
    /// Anything else, such as no network or CloudKit being busy.
    Failing,
}

/// A call CloudKit refused, as the [`io::Error`]s of this module carry it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// What the player could do about it.
    pub trouble: Trouble,
    /// How long CloudKit asked callers to wait before trying again.
    pub retry_after: Option<Duration>,
    /// CloudKit's description of what went wrong.
    pub message: String,
}

impl Failure {
    /// The CloudKit refusal behind `error`, if CloudKit refused anything.
    pub fn of(error: &io::Error) -> Option<&Self> {
        error.get_ref()?.downcast_ref()
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// The iCloud account CloudKit would act for: its user record name. Fails
/// with [`Trouble::NoAccount`] while the device is signed out or restricted,
/// when the build has no CloudKit entitlement, and always off Apple
/// platforms; with another [`Trouble`] when CloudKit could not tell, as
/// when offline.
pub fn account() -> io::Result<String> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::account()
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        Err(unsupported())
    }
}

/// The zone's changes since `token`, or since the start without one. Each
/// changed record's file is copied into `staging`, which must exist; the
/// caller moves or removes the copies.
pub fn fetch(token: Option<&str>, staging: &Path) -> io::Result<Changes> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::fetch(token, staging)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        let _ = (token, staging);
        Err(unsupported())
    }
}

/// Saves the file at `path` as the record `name`. Fails if CloudKit holds a
/// version further along, which the next fetch reports.
pub fn upload(name: &str, path: &Path, ticks: u64, saved_at: u64) -> io::Result<()> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::upload(name, path, ticks, saved_at)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        let _ = (name, path, ticks, saved_at);
        Err(unsupported())
    }
}

/// Replaces the record `name` with a delete record of tick `ticks`, which
/// other devices weigh against their copies as they would any save.
/// Fails with [`io::ErrorKind::AlreadyExists`] if CloudKit holds a version
/// further along.
pub fn delete(name: &str, ticks: u64) -> io::Result<()> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::delete(name, ticks)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        let _ = (name, ticks);
        Err(unsupported())
    }
}

#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        Failure {
            trouble: Trouble::NoAccount,
            retry_after: None,
            message: "CloudKit needs an Apple platform".to_owned(),
        },
    )
}

#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[allow(unsafe_code)]
mod bridge {
    use super::{Changes, Failure, Trouble};
    use std::ffi::{CString, c_char, c_longlong};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::time::Duration;

    unsafe extern "C" {
        fn aftermine_icloud_cloudkit_account() -> *mut c_char;
        fn aftermine_icloud_cloudkit_fetch(
            token: *const c_char,
            staging: *const c_char,
        ) -> *mut c_char;
        fn aftermine_icloud_cloudkit_upload(
            name: *const c_char,
            path: *const c_char,
            ticks: c_longlong,
            saved_at: c_longlong,
        ) -> *mut c_char;
        fn aftermine_icloud_cloudkit_delete(name: *const c_char, ticks: c_longlong) -> *mut c_char;
    }

    #[derive(serde::Deserialize)]
    struct Account {
        account: String,
    }

    /// A reply naming what went wrong.
    #[derive(serde::Deserialize)]
    struct Refusal {
        error: Option<String>,
        #[serde(default)]
        behind: bool,
        trouble: Option<Trouble>,
        /// In seconds.
        retry_after: Option<f64>,
    }

    fn failing(message: String) -> io::Error {
        io::Error::other(Failure {
            trouble: Trouble::Failing,
            retry_after: None,
            message,
        })
    }

    /// The bridge's JSON reply as `T`, or the error it names: a version
    /// further along as [`io::ErrorKind::AlreadyExists`], and anything else
    /// with its [`Failure`].
    pub(super) fn answer<T: serde::de::DeserializeOwned>(text: Option<Vec<u8>>) -> io::Result<T> {
        let text = text.ok_or_else(|| failing("CloudKit gave no answer".to_owned()))?;
        if let Ok(Refusal {
            error: Some(error),
            behind,
            trouble,
            retry_after,
        }) = serde_json::from_slice::<Refusal>(&text)
        {
            if behind {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, error));
            }
            return Err(io::Error::other(Failure {
                trouble: trouble.unwrap_or(Trouble::Failing),
                retry_after: retry_after
                    .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
                    .filter(|wait| !wait.is_zero()),
                message: error,
            }));
        }
        serde_json::from_slice(&text).map_err(|error| failing(error.to_string()))
    }

    fn text(value: &str) -> io::Result<CString> {
        CString::new(value).map_err(io::Error::other)
    }

    fn path(value: &Path) -> io::Result<CString> {
        CString::new(value.as_os_str().as_bytes()).map_err(io::Error::other)
    }

    pub(super) fn account() -> io::Result<String> {
        // SAFETY: takes no arguments and returns a string `take` owns.
        let reply = crate::objc::take(unsafe { aftermine_icloud_cloudkit_account() });
        answer::<Account>(reply).map(|reply| reply.account)
    }

    pub(super) fn fetch(token: Option<&str>, staging: &Path) -> io::Result<Changes> {
        let token = token.map(text).transpose()?;
        let staging = path(staging)?;
        // SAFETY: both strings are NUL-terminated and outlive the call; a
        // missing token passes NULL, which the bridge reads as none.
        let reply = crate::objc::take(unsafe {
            aftermine_icloud_cloudkit_fetch(
                token
                    .as_ref()
                    .map_or(std::ptr::null(), |token| token.as_ptr()),
                staging.as_ptr(),
            )
        });
        answer(reply)
    }

    pub(super) fn upload(name: &str, file: &Path, ticks: u64, saved_at: u64) -> io::Result<()> {
        let name = text(name)?;
        let file = path(file)?;
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let reply = crate::objc::take(unsafe {
            aftermine_icloud_cloudkit_upload(
                name.as_ptr(),
                file.as_ptr(),
                c_longlong::try_from(ticks).unwrap_or(c_longlong::MAX),
                c_longlong::try_from(saved_at).unwrap_or(c_longlong::MAX),
            )
        });
        answer::<serde_json::Value>(reply).map(|_| ())
    }

    pub(super) fn delete(name: &str, ticks: u64) -> io::Result<()> {
        let name = text(name)?;
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let reply = crate::objc::take(unsafe {
            aftermine_icloud_cloudkit_delete(
                name.as_ptr(),
                c_longlong::try_from(ticks).unwrap_or(c_longlong::MAX),
            )
        });
        answer::<serde_json::Value>(reply).map(|_| ())
    }
}

#[cfg(all(test, any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
mod tests;
