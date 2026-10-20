//! Where the Studio window opens.
//!
//! The first launch opens maximized, so the window fills the work area of the
//! primary monitor and stays clear of the taskbar at any resolution or display
//! scaling. After that the window opens where the user left it: its restored
//! rectangle and whether it was maximized are saved to
//! `~/.eustress_engine/window.toml` and applied at the next launch, clamped to
//! the work area of the nearest monitor, so a monitor that has since been
//! unplugged, moved or rescaled cannot leave the window off screen or under
//! the taskbar.
//!
//! Saving and restoring use the Win32 window placement calls, which report the
//! restored rectangle even while the window is maximized or minimized. On
//! other platforms the window opens maximized every time.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The window's size before its placement applies, in logical pixels, when
/// the monitor's work area cannot be read. It fits a 1280x720 logical work
/// area (1920x1080 at 150% scaling).
const STARTUP_SIZE: (u32, u32) = (1120, 630);

/// Share of the primary monitor's work area, per axis, the window covers
/// from the moment it is created. The maximize and the saved placement apply
/// only once frames run, which can be minutes into a large Space's load, so
/// the window must already be a working size. The remainder leaves room for
/// the title bar and borders, keeping the whole window inside the work area.
#[cfg_attr(not(windows), allow(dead_code))]
const STARTUP_SHARE: f32 = 0.9;

/// Share of the work area, per axis, a first launch's restored (not
/// maximized) rectangle covers, centered: restoring down from maximized
/// gives a large window, not the small creation-time fallback.
#[cfg_attr(not(windows), allow(dead_code))]
const RESTORED_SHARE: f32 = 0.8;

/// The smallest restored window, in physical pixels.
const MIN_SIZE: (i32, i32) = (640, 400);

/// A window rectangle, edges in physical pixels.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edges {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// What `window.toml` holds.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SavedPlacement {
    pub maximized: bool,
    /// The restored (not maximized) outer rectangle, in Win32 workspace
    /// coordinates: (0, 0) is the top-left of the primary monitor's work area.
    pub normal: Edges,
}

fn placement_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(".eustress_engine").join("window.toml"))
}

fn load() -> Option<SavedPlacement> {
    let text = std::fs::read_to_string(placement_path()?).ok()?;
    toml::from_str(&text).ok()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn save(placement: &SavedPlacement) {
    let Some(path) = placement_path() else { return };
    let Ok(text) = toml::to_string(placement) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Write beside it and rename, so a crash mid-write never leaves a torn file.
    let tmp = path.with_extension("toml.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Fit `rect` inside `work`: shrink it to the work area if it is larger, then
/// slide it in from whichever edge it crosses.
pub fn clamp_to_work_area(rect: Edges, work: Edges) -> Edges {
    let work_w = (work.right - work.left).max(1);
    let work_h = (work.bottom - work.top).max(1);
    let w = (rect.right - rect.left).max(MIN_SIZE.0).min(work_w);
    let h = (rect.bottom - rect.top).max(MIN_SIZE.1).min(work_h);
    let left = rect.left.clamp(work.left, work.right - w);
    let top = rect.top.clamp(work.top, work.bottom - h);
    Edges { left, top, right: left + w, bottom: top + h }
}

/// `share` of `work` on each axis, centered in it.
pub fn centered_share(work: Edges, share: f32) -> Edges {
    let work_w = (work.right - work.left).max(1);
    let work_h = (work.bottom - work.top).max(1);
    let w = ((work_w as f32 * share).round() as i32).clamp(MIN_SIZE.0.min(work_w), work_w);
    let h = ((work_h as f32 * share).round() as i32).clamp(MIN_SIZE.1.min(work_h), work_h);
    let left = work.left + (work_w - w) / 2;
    let top = work.top + (work_h - h) / 2;
    Edges { left, top, right: left + w, bottom: top + h }
}

/// The size the window is created at, in logical pixels: most of the primary
/// monitor's work area, or the fixed fallback when it cannot be read.
fn startup_size() -> (u32, u32) {
    #[cfg(windows)]
    {
        if let Some((w, h)) = win32::primary_work_area_logical() {
            let w = ((w * STARTUP_SHARE).round() as u32).max(MIN_SIZE.0 as u32);
            let h = ((h * STARTUP_SHARE).round() as u32).max(MIN_SIZE.1 as u32);
            return (w, h);
        }
    }
    STARTUP_SIZE
}

/// The primary window as it should be created: most of the work area,
/// centered, and maximized unless the saved placement says the user last had
/// it restored. `track_window_placement` then maximizes it, or applies the
/// saved rectangle, once the native window is showing.
pub fn startup_window(mut window: Window) -> Window {
    let (width, height) = startup_size();
    window.resolution = bevy::window::WindowResolution::new(width, height);
    window.position = bevy::window::WindowPosition::Centered(bevy::window::MonitorSelection::Primary);
    let restored = cfg!(windows) && load().is_some_and(|p| !p.maximized);
    if !restored {
        window.set_maximized(true);
    }
    window
}

pub struct WindowPlacementPlugin;

impl Plugin for WindowPlacementPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(windows)]
        app.add_systems(Update, win32::track_window_placement);
        #[cfg(not(windows))]
        let _ = app;
    }
}

#[cfg(windows)]
mod win32 {
    use super::{centered_share, clamp_to_work_area, load, save, Edges, SavedPlacement, RESTORED_SHARE};
    use bevy::ecs::system::NonSendMarker;
    use bevy::prelude::*;
    use bevy::window::{PrimaryWindow, RawHandleWrapper};
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetDC, GetDeviceCaps, GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, ReleaseDC, LOGPIXELSX,
        MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, IsWindowVisible, SetWindowPlacement, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED,
        SW_SHOWNORMAL, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
    };

    /// How often the placement is read back, in seconds. A change is saved
    /// once two reads in a row agree, so a drag in progress is not written.
    const POLL_SECS: f64 = 0.5;

    #[derive(Default)]
    pub(super) struct Tracker {
        restored: bool,
        last_poll: f64,
        saved: Option<SavedPlacement>,
        candidate: Option<SavedPlacement>,
    }

    fn edges(r: RECT) -> Edges {
        Edges { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }

    fn rect(e: Edges) -> RECT {
        RECT { left: e.left, top: e.top, right: e.right, bottom: e.bottom }
    }

    fn monitor_info(monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> Option<MONITORINFO> {
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: `info` is a valid MONITORINFO with cbSize set.
        (unsafe { GetMonitorInfoW(monitor, &mut info) } != 0).then_some(info)
    }

    fn primary_work_area() -> Option<Edges> {
        // SAFETY: plain value arguments.
        let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        monitor_info(primary).map(|i| edges(i.rcWork))
    }

    /// The primary monitor's work area in logical pixels, for sizing the
    /// window before it exists. This runs before winit makes the process DPI
    /// aware, when Windows reports coordinates already scaled to 96 DPI and
    /// the screen DC reports 96; in a process that is already aware both are
    /// physical and the DC reports the real DPI. Dividing by the DC's DPI
    /// gives logical pixels either way.
    pub(super) fn primary_work_area_logical() -> Option<(f32, f32)> {
        let work = primary_work_area()?;
        // SAFETY: a null window asks for the screen DC, released right after.
        let dpi = unsafe {
            let dc = GetDC(std::ptr::null_mut());
            if dc.is_null() {
                96
            } else {
                let dpi = GetDeviceCaps(dc, LOGPIXELSX as i32);
                ReleaseDC(std::ptr::null_mut(), dc);
                dpi
            }
        };
        let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
        let w = (work.right - work.left) as f32 / scale;
        let h = (work.bottom - work.top) as f32 / scale;
        (w > 0.0 && h > 0.0).then_some((w, h))
    }

    /// A first launch's placement: maximized, restoring down to most of the
    /// primary work area. In workspace coordinates, as `apply` takes them.
    fn first_launch_placement() -> Option<SavedPlacement> {
        let work = primary_work_area()?;
        let normal = centered_share(work, RESTORED_SHARE);
        Some(SavedPlacement {
            maximized: true,
            normal: Edges {
                left: normal.left - work.left,
                top: normal.top - work.top,
                right: normal.right - work.left,
                bottom: normal.bottom - work.top,
            },
        })
    }

    /// Workspace coordinates are screen coordinates shifted by the top-left of
    /// the primary monitor's work area (a taskbar on the top or left edge).
    fn workspace_offset() -> (i32, i32) {
        // SAFETY: plain value arguments.
        let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        monitor_info(primary).map_or((0, 0), |i| (i.rcWork.left, i.rcWork.top))
    }

    fn read(hwnd: HWND) -> Option<SavedPlacement> {
        let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        // SAFETY: `hwnd` is the live primary window; `wp` has its length set.
        if unsafe { GetWindowPlacement(hwnd, &mut wp) } == 0 {
            return None;
        }
        let maximized = if wp.showCmd == SW_SHOWMINIMIZED as u32 {
            wp.flags & WPF_RESTORETOMAXIMIZED != 0
        } else {
            wp.showCmd == SW_SHOWMAXIMIZED as u32
        };
        Some(SavedPlacement { maximized, normal: edges(wp.rcNormalPosition) })
    }

    /// Apply `saved`, first fitting its restored rectangle to the work area of
    /// the monitor nearest to where it was.
    fn apply(hwnd: HWND, saved: SavedPlacement) {
        let (dx, dy) = workspace_offset();
        let n = saved.normal;
        let on_screen = RECT { left: n.left + dx, top: n.top + dy, right: n.right + dx, bottom: n.bottom + dy };
        // SAFETY: `on_screen` is a valid RECT.
        let monitor = unsafe { MonitorFromRect(&on_screen, MONITOR_DEFAULTTONEAREST) };
        let fitted = match monitor_info(monitor) {
            Some(info) => clamp_to_work_area(edges(on_screen), edges(info.rcWork)),
            None => edges(on_screen),
        };
        let normal = Edges {
            left: fitted.left - dx,
            top: fitted.top - dy,
            right: fitted.right - dx,
            bottom: fitted.bottom - dy,
        };
        let wp = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: if saved.maximized { SW_SHOWMAXIMIZED as u32 } else { SW_SHOWNORMAL as u32 },
            rcNormalPosition: rect(normal),
            ..Default::default()
        };
        // SAFETY: `hwnd` is the live primary window; `wp` is fully initialised.
        unsafe { SetWindowPlacement(hwnd, &wp) };
    }

    /// Restores the saved placement once the window is showing, then saves the
    /// placement whenever it settles on a new one. `NonSendMarker` keeps this
    /// on the main thread, which owns the window: `SetWindowPlacement` from
    /// another thread waits on the main thread's message loop.
    pub(super) fn track_window_placement(
        _main_thread: NonSendMarker,
        windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
        time: Res<Time<Real>>,
        mut tracker: Local<Tracker>,
    ) {
        let Ok(handle) = windows.single() else { return };
        let RawWindowHandle::Win32(win32) = handle.get_window_handle() else { return };
        let hwnd = win32.hwnd.get() as HWND;

        if !tracker.restored {
            // The window is created hidden and shown once it is ready; a
            // placement applied before that would show it early.
            // SAFETY: `hwnd` is the live primary window.
            if unsafe { IsWindowVisible(hwnd) } == 0 {
                return;
            }
            tracker.restored = true;
            if let Some(saved) = load() {
                apply(hwnd, saved);
                tracker.saved = Some(saved);
            } else if let Some(first) = first_launch_placement() {
                // Maximize here rather than trust the request made when the
                // window was built: Bevy acts on that request only when it
                // next sees the window change, which may never happen.
                apply(hwnd, first);
            }
            tracker.last_poll = time.elapsed_secs_f64();
            return;
        }

        let now = time.elapsed_secs_f64();
        if now - tracker.last_poll < POLL_SECS {
            return;
        }
        tracker.last_poll = now;
        let Some(current) = read(hwnd) else { return };
        if tracker.saved == Some(current) {
            tracker.candidate = None;
        } else if tracker.candidate == Some(current) {
            save(&current);
            tracker.saved = Some(current);
            tracker.candidate = None;
        } else {
            tracker.candidate = Some(current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Edges = Edges { left: 0, top: 0, right: 1920, bottom: 1032 };

    #[test]
    fn a_window_inside_the_work_area_is_unchanged() {
        let r = Edges { left: 100, top: 80, right: 1300, bottom: 880 };
        assert_eq!(clamp_to_work_area(r, WORK), r);
    }

    #[test]
    fn a_window_over_the_taskbar_moves_up() {
        let r = Edges { left: 100, top: 300, right: 1300, bottom: 1119 };
        let c = clamp_to_work_area(r, WORK);
        assert_eq!((c.top, c.bottom), (213, 1032));
        assert_eq!(c.bottom - c.top, r.bottom - r.top);
    }

    #[test]
    fn a_window_taller_than_the_work_area_shrinks_to_it() {
        let r = Edges { left: 0, top: -20, right: 1600, bottom: 1119 };
        let c = clamp_to_work_area(r, WORK);
        assert_eq!((c.top, c.bottom), (0, 1032));
    }

    #[test]
    fn a_window_on_a_missing_monitor_comes_back() {
        let r = Edges { left: 4000, top: 200, right: 5200, bottom: 1000 };
        let c = clamp_to_work_area(r, WORK);
        assert_eq!((c.left, c.right), (720, 1920));
    }

    #[test]
    fn a_tiny_window_grows_to_the_minimum() {
        let r = Edges { left: 10, top: 10, right: 20, bottom: 20 };
        let c = clamp_to_work_area(r, WORK);
        assert_eq!((c.right - c.left, c.bottom - c.top), MIN_SIZE);
    }

    #[test]
    fn a_share_is_centered_in_the_work_area() {
        let c = centered_share(WORK, 0.8);
        assert_eq!((c.right - c.left, c.bottom - c.top), (1536, 826));
        assert_eq!((c.left, c.top), (192, 103));
    }

    #[test]
    fn a_share_never_drops_below_the_minimum_or_exceeds_the_work_area() {
        let small = Edges { left: 0, top: 0, right: 700, bottom: 450 };
        let c = centered_share(small, 0.5);
        assert_eq!((c.right - c.left, c.bottom - c.top), MIN_SIZE);
        let c = centered_share(WORK, 1.5);
        assert_eq!(c, WORK);
    }

    #[test]
    fn the_saved_file_round_trips() {
        let p = SavedPlacement { maximized: true, normal: Edges { left: -8, top: 0, right: 1200, bottom: 800 } };
        let text = toml::to_string(&p).unwrap();
        assert_eq!(toml::from_str::<SavedPlacement>(&text).unwrap(), p);
    }
}
