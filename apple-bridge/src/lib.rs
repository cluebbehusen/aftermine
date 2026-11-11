#![doc = include_str!("../README.md")]

use std::path::{Path, PathBuf};

pub mod application;
pub mod cloudkit;
pub mod defaults;
mod key_value;

pub use key_value::KeyValueStore;

/// How a coordinated access touches its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Reads the file, after iCloud downloads it if it is not on this device.
    Read,
    /// Writes the file whole, creating or replacing it.
    Replace,
    /// Removes the file.
    Delete,
}

/// The app's iCloud Drive folder: its container's `Documents` directory,
/// which Files and Finder show under the app's name. `None` when iCloud
/// Drive is off or signed out, when the build lacks the iCloud entitlement,
/// and on platforms without iCloud Drive. Setting up iCloud can take
/// seconds, so never call this on the main thread.
pub fn documents() -> Option<PathBuf> {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        apple::documents()
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
    {
        None
    }
}

/// Runs `access` on `path` while iCloud holds off syncing it, so iCloud
/// never uploads a half-written file or replaces one mid-read. A read waits
/// for a file that is not on this device yet. `access` receives the path to
/// work on, which is where the file now lives if it moved meanwhile.
/// Without iCloud Drive it simply runs on `path`. Errs only when iCloud
/// refused the access, in which case `access` never ran.
pub fn coordinate<R>(
    path: &Path,
    kind: Access,
    access: impl FnOnce(&Path) -> R,
) -> std::io::Result<R> {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        apple::coordinate(path, kind, access)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
    {
        let _ = kind;
        Ok(access(path))
    }
}

/// Starts downloading every file in `dir` that iCloud holds but this
/// device does not, and keeps iCloud's choice where two devices changed a
/// file at once. Returns how many files are still on their way.
pub fn download(dir: &Path) -> usize {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        apple::download(dir)
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
    {
        let _ = dir;
        0
    }
}

/// Strings crossing from the Objective-C side, shared by both bridges.
#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[allow(unsafe_code)]
mod objc {
    use std::ffi::{CStr, c_char, c_void};

    unsafe extern "C" {
        fn free(pointer: *mut c_void);
    }

    /// Takes ownership of a `strdup`ed string from the Objective-C side.
    pub(crate) fn take(text: *mut c_char) -> Option<Vec<u8>> {
        if text.is_null() {
            return None;
        }
        // SAFETY: a non-null result is a NUL-terminated `strdup` copy that
        // nothing else holds; it is read once and freed here.
        let bytes = unsafe { CStr::from_ptr(text) }.to_bytes().to_vec();
        // SAFETY: `strdup` allocated it with `malloc`.
        unsafe { free(text.cast()) };
        Some(bytes)
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[allow(unsafe_code)]
mod apple {
    use super::Access;
    use super::objc::take;
    use std::ffi::{CStr, CString, c_char, c_int, c_long, c_void};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    unsafe extern "C" {
        fn aftermine_apple_bridge_documents() -> *mut c_char;
        fn aftermine_apple_bridge_coordinate(
            path: *const c_char,
            access: c_int,
            context: *mut c_void,
            accessor: extern "C" fn(*mut c_void, *const c_char),
            error_out: *mut *mut c_char,
        ) -> bool;
        fn aftermine_apple_bridge_download(dir: *const c_char) -> c_long;
    }

    fn c_path(path: &Path) -> std::io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
    }

    pub(super) fn documents() -> Option<PathBuf> {
        // SAFETY: takes no arguments and returns a string `take` owns.
        let path = take(unsafe { aftermine_apple_bridge_documents() })?;
        Some(PathBuf::from(std::ffi::OsString::from_vec(path)))
    }

    /// The closure and its outcome, shared with the accessor callback.
    struct Slot<F, R> {
        access: Option<F>,
        outcome: Option<std::thread::Result<R>>,
    }

    extern "C" fn run<F: FnOnce(&Path) -> R, R>(context: *mut c_void, granted: *const c_char) {
        // SAFETY: `coordinate` passes its own `Slot<F, R>`, which outlives
        // the synchronous call that invokes this.
        let slot = unsafe { &mut *context.cast::<Slot<F, R>>() };
        // SAFETY: the coordinator passes a NUL-terminated path that lives
        // for the duration of this call.
        let granted = unsafe { CStr::from_ptr(granted) };
        let granted = Path::new(std::ffi::OsStr::from_bytes(granted.to_bytes()));
        if let Some(access) = slot.access.take() {
            // A panic must not unwind through Objective-C frames, so it
            // crosses as a value and resumes on the Rust side.
            slot.outcome = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || access(granted),
            )));
        }
    }

    pub(super) fn coordinate<F: FnOnce(&Path) -> R, R>(
        path: &Path,
        kind: Access,
        access: F,
    ) -> std::io::Result<R> {
        let c_path = c_path(path)?;
        let mut slot = Slot {
            access: Some(access),
            outcome: None,
        };
        let mut error = std::ptr::null_mut();
        let kind = match kind {
            Access::Read => 0,
            Access::Replace => 1,
            Access::Delete => 2,
        };
        // SAFETY: the path and slot outlive the call, the accessor matches
        // the slot's type, and `error` receives at most one string `take`
        // owns.
        unsafe {
            aftermine_apple_bridge_coordinate(
                c_path.as_ptr(),
                kind,
                (&raw mut slot).cast(),
                run::<F, R>,
                &raw mut error,
            );
        }
        let message = take(error);
        match slot.outcome {
            Some(Ok(value)) => Ok(value),
            Some(Err(panic)) => std::panic::resume_unwind(panic),
            _ => Err(std::io::Error::other(format!(
                "iCloud refused access to {}: {}",
                path.display(),
                message.map_or_else(String::new, |bytes| String::from_utf8_lossy(&bytes).into())
            ))),
        }
    }

    pub(super) fn download(dir: &Path) -> usize {
        let Ok(dir) = c_path(dir) else {
            return 0;
        };
        // SAFETY: `dir` is a NUL-terminated path that outlives the call.
        let downloading = unsafe { aftermine_apple_bridge_download(dir.as_ptr()) };
        usize::try_from(downloading).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests;
