//! The app's hold on the device while it plays or saves: keeping the screen
//! on, and asking for time to finish work if the player leaves the app. Only
//! iPad and Apple TV have either; elsewhere both do nothing.

/// Keeps the screen on while `awake`, or leaves dimming and sleep to the
/// system. A game read through controllers must do this itself, since the
/// system counts only touches as activity. Call it on the main thread.
pub fn keep_screen_awake(awake: bool) {
    #[cfg(any(target_os = "ios", target_os = "tvos"))]
    {
        bridge::keep_screen_awake(awake);
    }
    #[cfg(not(any(target_os = "ios", target_os = "tvos")))]
    {
        let _ = awake;
    }
}

/// Time asked of the system to finish work if the app leaves the foreground
/// meanwhile, held until dropped. The system ends it on its own once that
/// time runs out, rather than stopping the app.
#[derive(Debug)]
#[must_use = "dropping the task gives its time back at once"]
pub struct BackgroundTask {
    #[cfg(any(target_os = "ios", target_os = "tvos"))]
    id: usize,
}

impl BackgroundTask {
    /// Asks for the time; any thread may.
    pub fn begin() -> Self {
        #[cfg(any(target_os = "ios", target_os = "tvos"))]
        {
            Self {
                id: bridge::begin_background_task(),
            }
        }
        #[cfg(not(any(target_os = "ios", target_os = "tvos")))]
        {
            Self {}
        }
    }
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        #[cfg(any(target_os = "ios", target_os = "tvos"))]
        bridge::end_background_task(self.id);
    }
}

#[cfg(any(target_os = "ios", target_os = "tvos"))]
#[allow(unsafe_code)]
mod bridge {
    unsafe extern "C" {
        fn aftermine_apple_bridge_keep_screen_awake(awake: bool);
        fn aftermine_apple_bridge_begin_background_task() -> usize;
        fn aftermine_apple_bridge_end_background_task(task: usize);
    }

    pub(super) fn keep_screen_awake(awake: bool) {
        // SAFETY: takes a plain value and returns nothing.
        unsafe { aftermine_apple_bridge_keep_screen_awake(awake) }
    }

    pub(super) fn begin_background_task() -> usize {
        // SAFETY: takes nothing and returns a plain value.
        unsafe { aftermine_apple_bridge_begin_background_task() }
    }

    pub(super) fn end_background_task(task: usize) {
        // SAFETY: takes a plain value; the bridge ignores one already
        // ended or never begun.
        unsafe { aftermine_apple_bridge_end_background_task(task) }
    }
}
