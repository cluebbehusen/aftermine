//! The app's user defaults: small strings kept on this device alone. On
//! Apple TV they are the only local storage the system never clears.

/// The string stored under `key`, if any. `None` off Apple platforms.
pub fn get(key: &str) -> Option<String> {
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

/// Stores `value` under `key`; the system writes it out in the background.
/// A key or value with a NUL byte is dropped, as is everything off Apple
/// platforms.
pub fn set(key: &str, value: &str) {
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
    {
        bridge::set(key, value);
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "tvos")))]
    {
        let _ = (key, value);
    }
}

#[cfg(any(target_os = "ios", target_os = "macos", target_os = "tvos"))]
#[allow(unsafe_code)]
mod bridge {
    use std::ffi::{CString, c_char};

    unsafe extern "C" {
        fn aftermine_apple_bridge_defaults_get(key: *const c_char) -> *mut c_char;
        fn aftermine_apple_bridge_defaults_set(key: *const c_char, value: *const c_char);
    }

    pub(super) fn get(key: &str) -> Option<String> {
        let key = CString::new(key).ok()?;
        // SAFETY: `key` is NUL-terminated and outlives the call, which
        // returns a string `take` owns.
        let bytes =
            crate::objc::take(unsafe { aftermine_apple_bridge_defaults_get(key.as_ptr()) })?;
        String::from_utf8(bytes).ok()
    }

    pub(super) fn set(key: &str, value: &str) {
        let (Ok(key), Ok(value)) = (CString::new(key), CString::new(value)) else {
            return;
        };
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe { aftermine_apple_bridge_defaults_set(key.as_ptr(), value.as_ptr()) };
    }
}
