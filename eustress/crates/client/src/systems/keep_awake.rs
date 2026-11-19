//! The display stays on while the Player's window is in front.
//!
//! Windows turns the display off after its idle timer, and on some drivers
//! that takes the GPU device with it (`systems::gpu_recovery`). A game on
//! screen is not idle, so while the window has focus the Player resets the
//! display and sleep timers every [`NUDGE_EVERY`], as video players do. It
//! never holds a standing request (no `ES_CONTINUOUS`): with the window in the
//! background, or the Player closed, Windows' own timers run as usual.
//!
//! Windows only. Elsewhere this does nothing.

use std::time::Duration;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// How often the idle timers are reset while the window has focus. Far
/// shorter than any idle timer Windows offers (one minute at least).
pub const NUDGE_EVERY: Duration = Duration::from_secs(30);

pub struct KeepAwakePlugin;

impl Plugin for KeepAwakePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, keep_display_awake);
    }
}

/// Whether the timers are due a reset: the window has focus, and none was
/// made in the last [`NUDGE_EVERY`]. `last` and `now` are times since start.
pub fn nudge_due(focused: bool, last: Option<Duration>, now: Duration) -> bool {
    focused && last.is_none_or(|t| now.saturating_sub(t) >= NUDGE_EVERY)
}

fn keep_display_awake(
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut last: Local<Option<Duration>>,
) {
    let focused = windows.single().is_ok_and(|w| w.focused);
    let now = time.elapsed();
    if !nudge_due(focused, *last, now) {
        return;
    }
    *last = Some(now);
    reset_idle_timers();
}

/// One reset of the display and sleep idle timers. Without `ES_CONTINUOUS`
/// nothing stays set, so it does not matter which thread calls it.
#[cfg(windows)]
fn reset_idle_timers() {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
    // SAFETY: takes plain flags and only resets this session's idle timers.
    let previous = unsafe { SetThreadExecutionState(ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) };
    if previous == 0 {
        warn_once!("could not keep the display on while playing (SetThreadExecutionState failed)");
    }
}

#[cfg(not(windows))]
fn reset_idle_timers() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_focused_window_resets_the_timers_and_at_most_every_nudge_interval() {
        let t = Duration::from_secs(100);
        assert!(nudge_due(true, None, t), "the first focused frame resets them");
        assert!(!nudge_due(false, None, t), "a window in the background never does");
        assert!(!nudge_due(true, Some(t), t + NUDGE_EVERY - Duration::from_millis(1)));
        assert!(nudge_due(true, Some(t), t + NUDGE_EVERY));
        assert!(!nudge_due(false, Some(t), t + NUDGE_EVERY * 10));
    }
}
