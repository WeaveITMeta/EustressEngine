//! Whether the browser still holds the mouse the Player asked to lock.
//!
//! While a page holds the pointer lock, the browser keeps Esc for itself: it
//! releases the mouse and the page never receives the key. A Player that
//! waits for Esc keeps believing the mouse is locked, hides the cursor over
//! the canvas, and turns the camera on every move. The browser can also
//! refuse a request outright (no click came before it). Each frame this
//! compares what the Player asked for with what the document reports and
//! says when the Player must let go. Pure, so it is tested without a browser.

/// How long a lock request may wait for the browser before it counts as
/// refused. A granted request lands within a frame or two.
pub const REFUSED_AFTER_S: f32 = 1.0;

/// The most one frame counts toward [`REFUSED_AFTER_S`], so a single long
/// frame (a tab brought back, a shader compile) is not read as a refusal.
const MAX_FRAME_S: f32 = 0.1;

/// What the Player does with the mouse this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStep {
    /// Nothing changes.
    Keep,
    /// The browser held the lock and let it go: the person pressed Esc, or
    /// the page lost focus. The Player frees the cursor.
    LeftByPerson,
    /// The browser never took the lock that was asked for. The Player frees
    /// the cursor; the next click asks again.
    Refused,
}

/// The browser's side of the lock, as last seen.
#[derive(Debug, Default)]
pub struct LockWatch {
    held: bool,
    waited_s: f32,
}

impl LockWatch {
    /// `asked`: the Player's cursor is set to locked. `browser_holds`: the
    /// document has a pointer-lock element. `dt_s`: this frame's length.
    pub fn step(&mut self, asked: bool, browser_holds: bool, dt_s: f32) -> LockStep {
        if browser_holds {
            self.held = true;
            self.waited_s = 0.0;
            return LockStep::Keep;
        }
        if !asked {
            self.held = false;
            self.waited_s = 0.0;
            return LockStep::Keep;
        }
        if self.held {
            self.held = false;
            self.waited_s = 0.0;
            return LockStep::LeftByPerson;
        }
        self.waited_s += dt_s.clamp(0.0, MAX_FRAME_S);
        if self.waited_s >= REFUSED_AFTER_S {
            self.waited_s = 0.0;
            return LockStep::Refused;
        }
        LockStep::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    #[test]
    fn esc_in_the_browser_frees_the_mouse_once() {
        let mut w = LockWatch::default();
        assert_eq!(w.step(true, false, FRAME), LockStep::Keep, "request pending");
        assert_eq!(w.step(true, true, FRAME), LockStep::Keep, "granted");
        assert_eq!(w.step(true, true, FRAME), LockStep::Keep, "held");
        // Esc: the browser lets go, the Player still asks.
        assert_eq!(w.step(true, false, FRAME), LockStep::LeftByPerson);
        // The Player has freed the cursor; nothing more happens.
        assert_eq!(w.step(false, false, FRAME), LockStep::Keep);
        assert_eq!(w.step(false, false, FRAME), LockStep::Keep);
    }

    #[test]
    fn a_request_the_browser_never_grants_is_given_up_after_a_second() {
        let mut w = LockWatch::default();
        // Sixty frames of 1/60 s sum to a hair under one second in f32, so
        // the refusal may land one frame after the sixtieth.
        let frames = (REFUSED_AFTER_S / FRAME).round() as usize;
        for i in 0..frames - 1 {
            assert_eq!(w.step(true, false, FRAME), LockStep::Keep, "frame {i}");
        }
        let refused_by = (0..2).find(|_| w.step(true, false, FRAME) == LockStep::Refused);
        assert!(refused_by.is_some(), "no refusal by frame {}", frames + 1);
    }

    #[test]
    fn a_slow_grant_is_not_a_refusal() {
        let mut w = LockWatch::default();
        for _ in 0..30 {
            assert_eq!(w.step(true, false, FRAME), LockStep::Keep);
        }
        assert_eq!(w.step(true, true, FRAME), LockStep::Keep);
        for _ in 0..120 {
            assert_eq!(w.step(true, true, FRAME), LockStep::Keep);
        }
    }

    #[test]
    fn one_long_frame_is_not_a_refusal() {
        let mut w = LockWatch::default();
        assert_eq!(w.step(true, false, 5.0), LockStep::Keep);
        assert_eq!(w.step(true, true, FRAME), LockStep::Keep);
    }

    #[test]
    fn nothing_happens_while_the_player_asks_for_no_lock() {
        let mut w = LockWatch::default();
        for _ in 0..200 {
            assert_eq!(w.step(false, false, FRAME), LockStep::Keep);
        }
        // A lock the Player released, still held for a frame by the browser.
        assert_eq!(w.step(false, true, FRAME), LockStep::Keep);
        assert_eq!(w.step(false, false, FRAME), LockStep::Keep);
    }
}
