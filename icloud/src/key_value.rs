//! iCloud's key-value store: a few small strings that follow the player's
//! Apple ID across their iPad, Mac and Apple TV.

/// The app's iCloud key-value store, which iCloud keeps in step across
/// every device signed in to the same Apple ID, newest write winning per
/// key. It holds values in memory and syncs in the background, so its calls
/// never wait on iCloud. While the device is offline or signed out it keeps
/// working on this device alone.
#[derive(Debug)]
pub struct KeyValueStore {
    _opened: (),
}

impl KeyValueStore {
    /// Starts counting outside changes and asks iCloud for the newest
    /// values. `None` when the store refuses this app, as it does for a
    /// build without the key-value store entitlement, and on platforms
    /// without iCloud.
    pub fn open() -> Option<Self> {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
        {
            bridge::open().then_some(Self { _opened: () })
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
        {
            None
        }
    }

    /// The string stored under `key`, if any.
    pub fn get(&self, key: &str) -> Option<String> {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
        {
            bridge::get(key)
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
        {
            let _ = key;
            None
        }
    }

    /// Stores `value` under `key`; iCloud uploads it in the background. A
    /// key or value with a NUL byte is dropped.
    pub fn set(&self, key: &str, value: &str) {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
        {
            bridge::set(key, value);
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
        {
            let _ = (key, value);
        }
    }

    /// How many times the store has changed from outside since the process
    /// started: another device's write, an account change, or a write of
    /// this device's that iCloud refused. Read it before reading values, so
    /// a change that lands meanwhile still moves the count.
    pub fn changes(&self) -> u64 {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
        {
            bridge::changes()
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
        {
            0
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[allow(unsafe_code)]
mod bridge {
    use std::ffi::{CString, c_char, c_long};

    unsafe extern "C" {
        fn aftermine_icloud_key_value_open() -> bool;
        fn aftermine_icloud_key_value_get(key: *const c_char) -> *mut c_char;
        fn aftermine_icloud_key_value_set(key: *const c_char, value: *const c_char);
        fn aftermine_icloud_key_value_changes() -> c_long;
    }

    pub(super) fn open() -> bool {
        // SAFETY: takes no arguments.
        unsafe { aftermine_icloud_key_value_open() }
    }

    pub(super) fn get(key: &str) -> Option<String> {
        let key = CString::new(key).ok()?;
        // SAFETY: `key` is NUL-terminated and outlives the call, which
        // returns a string `take` owns.
        let bytes = crate::objc::take(unsafe { aftermine_icloud_key_value_get(key.as_ptr()) })?;
        String::from_utf8(bytes).ok()
    }

    pub(super) fn set(key: &str, value: &str) {
        let (Ok(key), Ok(value)) = (CString::new(key), CString::new(value)) else {
            return;
        };
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe { aftermine_icloud_key_value_set(key.as_ptr(), value.as_ptr()) };
    }

    pub(super) fn changes() -> u64 {
        // SAFETY: takes no arguments.
        let changes = unsafe { aftermine_icloud_key_value_changes() };
        u64::try_from(changes).unwrap_or(0)
    }
}
