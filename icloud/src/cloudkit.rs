//! CloudKit's private database as a mirror for record files: one zone, one
//! record per file, each carrying the file and when it was saved. Every call
//! waits for CloudKit, which can take seconds, so only a worker thread may
//! make one. Which files go where is the shell's decision.

use std::io;
use std::path::{Path, PathBuf};

/// A record another device changed, its file copied to this device.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Changed {
    /// The record's name: the file's path under the records root.
    pub name: String,
    /// When the file was saved, in Unix seconds.
    pub saved_at: u64,
    /// The copy of the file, in the staging folder the fetch was given.
    pub staged: PathBuf,
}

/// What changed in the zone since a token.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Changes {
    /// Records added or replaced.
    pub changed: Vec<Changed>,
    /// Names of records deleted.
    pub deleted: Vec<String>,
    /// Where the next fetch resumes, once these changes are applied.
    pub token: Option<String>,
    /// Whether more changes wait beyond this batch.
    pub more: bool,
    /// Changed records that could not be copied to this device. While any
    /// failed, the token must not advance, or they are never fetched again.
    #[serde(default)]
    pub failed: usize,
}

/// The iCloud account CloudKit would act for: its user record name, or
/// `None` while the device is signed out, restricted, offline, or the build
/// has no CloudKit entitlement. Always `None` off Apple platforms.
pub fn account() -> Option<String> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::account()
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        None
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

/// Saves the file at `path` as the record `name`, replacing any record of
/// that name.
pub fn upload(name: &str, path: &Path, saved_at: u64, size: u64) -> io::Result<()> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::upload(name, path, saved_at, size)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        let _ = (name, path, saved_at, size);
        Err(unsupported())
    }
}

/// Deletes the record `name`. A record already gone counts as deleted.
pub fn delete(name: &str) -> io::Result<()> {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::delete(name)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        let _ = name;
        Err(unsupported())
    }
}

#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "CloudKit needs an Apple platform",
    )
}

#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[allow(unsafe_code)]
mod bridge {
    use super::Changes;
    use std::ffi::{CString, c_char, c_longlong};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    unsafe extern "C" {
        fn aftermine_icloud_cloudkit_account() -> *mut c_char;
        fn aftermine_icloud_cloudkit_fetch(
            token: *const c_char,
            staging: *const c_char,
        ) -> *mut c_char;
        fn aftermine_icloud_cloudkit_upload(
            name: *const c_char,
            path: *const c_char,
            saved_at: c_longlong,
            size: c_longlong,
        ) -> *mut c_char;
        fn aftermine_icloud_cloudkit_delete(name: *const c_char) -> *mut c_char;
    }

    #[derive(serde::Deserialize)]
    struct Account {
        account: Option<String>,
    }

    /// A reply naming what went wrong.
    #[derive(serde::Deserialize)]
    struct Failure {
        error: Option<String>,
    }

    /// The bridge's JSON reply as `T`, or the error it names.
    pub(super) fn answer<T: serde::de::DeserializeOwned>(text: Option<Vec<u8>>) -> io::Result<T> {
        let text = text.ok_or_else(|| io::Error::other("CloudKit gave no answer"))?;
        if let Ok(Failure { error: Some(error) }) = serde_json::from_slice::<Failure>(&text) {
            return Err(io::Error::other(error));
        }
        serde_json::from_slice(&text).map_err(io::Error::other)
    }

    fn text(value: &str) -> io::Result<CString> {
        CString::new(value).map_err(io::Error::other)
    }

    fn path(value: &Path) -> io::Result<CString> {
        CString::new(value.as_os_str().as_bytes()).map_err(io::Error::other)
    }

    pub(super) fn account() -> Option<String> {
        // SAFETY: takes no arguments and returns a string `take` owns.
        let reply = crate::objc::take(unsafe { aftermine_icloud_cloudkit_account() });
        answer::<Account>(reply).ok()?.account
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

    pub(super) fn upload(name: &str, file: &Path, saved_at: u64, size: u64) -> io::Result<()> {
        let name = text(name)?;
        let file = path(file)?;
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let reply = crate::objc::take(unsafe {
            aftermine_icloud_cloudkit_upload(
                name.as_ptr(),
                file.as_ptr(),
                c_longlong::try_from(saved_at).unwrap_or(c_longlong::MAX),
                c_longlong::try_from(size).unwrap_or(c_longlong::MAX),
            )
        });
        answer::<serde_json::Value>(reply).map(|_| ())
    }

    pub(super) fn delete(name: &str) -> io::Result<()> {
        let name = text(name)?;
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let reply = crate::objc::take(unsafe { aftermine_icloud_cloudkit_delete(name.as_ptr()) });
        answer::<serde_json::Value>(reply).map(|_| ())
    }
}
