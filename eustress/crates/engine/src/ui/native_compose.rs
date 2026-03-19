// ============================================================================
// Native Window Composition — Win32 APIs for Dual-Window Overlay
// ============================================================================
//
// Architecture:
//   Bevy runs natively on the main thread with its own GPU window.
//   Slint runs in a background thread with a borderless overlay window.
//   This module uses Win32 APIs to:
//     1. Find the Slint overlay window (by title match)
//     2. Make it an "owned window" of the Bevy window (move/minimize/alt-tab together)
//     3. Position the overlay to exactly cover the Bevy window
//     4. Set WS_EX_LAYERED + WS_EX_TRANSPARENT on the viewport region so
//        mouse clicks pass through to Bevy's native window underneath
//     5. Sync position/size when Bevy window moves or resizes
//
// Table of Contents:
//   - NativeComposePlugin: Bevy plugin that registers the composition systems
//   - NativeComposeState: Resource tracking the linked window handles
//   - find_and_link_overlay: Startup system to find Slint window and link it
//   - sync_overlay_position: Update system to keep overlay positioned over Bevy
// ============================================================================

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Bevy plugin that links the Slint overlay window to the Bevy native window.
pub struct NativeComposePlugin;

impl Plugin for NativeComposePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<NativeComposeState>()
            .add_systems(Update, find_and_link_overlay)
            .add_systems(Update, sync_overlay_position.after(find_and_link_overlay));
    }
}

/// Tracks the linked window handles and last known Bevy window geometry.
/// HWND handles are stored as raw isize for Send+Sync compatibility with Bevy resources.
#[derive(Resource, Default)]
pub struct NativeComposeState {
    /// Whether we have successfully linked the Slint overlay to the Bevy window
    pub linked: bool,
    /// Retry timer — don't spam FindWindow every frame
    pub next_retry: f64,
    /// Last known Bevy window position (screen coordinates)
    pub last_position: (i32, i32),
    /// Last known Bevy window size (pixels)
    pub last_size: (u32, u32),
    /// Platform-specific handles (Win32 HWNDs stored as isize for Send+Sync)
    #[cfg(target_os = "windows")]
    pub bevy_hwnd: Option<isize>,
    #[cfg(target_os = "windows")]
    pub slint_hwnd: Option<isize>,
}

/// Finds the Slint overlay window and links it as an owned window of Bevy.
/// Retries every 500ms until found (Slint thread may take a moment to create its window).
fn find_and_link_overlay(
    mut state: ResMut<NativeComposeState>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if state.linked {
        return;
    }

    let elapsed = time.elapsed_secs_f64();
    if elapsed < state.next_retry {
        return;
    }
    state.next_retry = elapsed + 0.5;

    // Need the Bevy window to exist first
    let Ok(_bevy_window) = windows.single() else {
        return;
    };

    #[cfg(target_os = "windows")]
    {
        link_windows_win32(&mut state);
    }

    #[cfg(not(target_os = "windows"))]
    {
        // On non-Windows platforms, just mark as linked (no composition needed yet)
        // Future: implement for macOS (NSWindow) and Linux (X11/Wayland)
        warn!("NativeComposePlugin: window composition not yet implemented for this platform");
        state.linked = true;
    }
}

/// Keeps the Slint overlay positioned and sized to match the Bevy window.
fn sync_overlay_position(
    state: Res<NativeComposeState>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if !state.linked {
        return;
    }

    let Ok(_bevy_window) = windows.single() else {
        return;
    };

    #[cfg(target_os = "windows")]
    {
        sync_position_win32(&state);
    }
}

// ============================================================================
// Win32 Implementation
// ============================================================================
// HWND in windows-sys 0.59 is *mut c_void. We store handles as isize in
// NativeComposeState for Send+Sync safety, and cast at the API boundary.

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE, FALSE};

/// Convert isize to HWND (*mut c_void)
#[cfg(target_os = "windows")]
#[inline]
fn to_hwnd(val: isize) -> HWND { val as HWND }

/// Convert HWND (*mut c_void) to isize
#[cfg(target_os = "windows")]
#[inline]
fn from_hwnd(hwnd: HWND) -> isize { hwnd as isize }

#[cfg(target_os = "windows")]
fn link_windows_win32(state: &mut NativeComposeState) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    // Find the Bevy window by title
    let bevy_title: Vec<u16> = "Eustress Engine\0".encode_utf16().collect();
    let bevy_hwnd = unsafe { FindWindowW(std::ptr::null(), bevy_title.as_ptr()) };
    if bevy_hwnd.is_null() {
        info!("NativeCompose: Bevy window not found yet, retrying...");
        return;
    }

    // Get the Slint overlay HWND directly from our custom Win32 platform.
    // No more fragile EnumWindows — the platform stores the HWND atomically.
    let slint_hwnd_raw = super::win32_platform::get_slint_hwnd();
    if slint_hwnd_raw == 0 {
        info!("NativeCompose: Slint overlay window not created yet, retrying...");
        return;
    }
    let slint_hwnd = to_hwnd(slint_hwnd_raw);

    info!("NativeCompose: Found Bevy HWND={:?}, Slint HWND={:?}", bevy_hwnd, slint_hwnd);

    // Store the Bevy HWND so set_overlay_geometry can use it for z-ordering
    super::win32_platform::set_bevy_hwnd(from_hwnd(bevy_hwnd));

    // Make the Slint window an owned popup of the Bevy window.
    // This makes them move/minimize/alt-tab together.
    unsafe {
        // Set the Bevy window as the owner of the Slint overlay
        SetWindowLongPtrW(slint_hwnd, GWLP_HWNDPARENT, from_hwnd(bevy_hwnd));

        // Position the Slint overlay over the Bevy window's CLIENT area only.
        // The client area excludes the title bar and borders, so OS window
        // buttons (minimize/maximize/close) remain visible and clickable.
        let mut win_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut client_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(bevy_hwnd, &mut win_rect);
        GetClientRect(bevy_hwnd, &mut client_rect);

        // Compute the border/title bar offsets:
        // window width vs client width gives us left+right borders
        // window height vs client height gives us title bar + bottom border
        let win_width = win_rect.right - win_rect.left;
        let win_height = win_rect.bottom - win_rect.top;
        let client_width = client_rect.right - client_rect.left;
        let client_height = client_rect.bottom - client_rect.top;
        let border_x = (win_width - client_width) / 2;
        let title_bar_height = win_height - client_height - border_x; // top offset

        let client_screen_x = win_rect.left + border_x;
        let client_screen_y = win_rect.top + title_bar_height;

        info!(
            "NativeCompose: win_rect=({},{},{},{}), client_rect=({},{},{},{}), border_x={}, title_bar_height={}, overlay_pos=({},{}) size={}x{}",
            win_rect.left, win_rect.top, win_rect.right, win_rect.bottom,
            client_rect.left, client_rect.top, client_rect.right, client_rect.bottom,
            border_x, title_bar_height,
            client_screen_x, client_screen_y,
            client_width, client_height,
        );

        SetWindowPos(
            slint_hwnd,
            HWND_TOP, // above Bevy but not above all apps
            client_screen_x,
            client_screen_y,
            client_width,
            client_height,
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
    }

    state.bevy_hwnd = Some(from_hwnd(bevy_hwnd));
    state.slint_hwnd = Some(slint_hwnd_raw);
    state.linked = true;
    info!("NativeCompose: Slint overlay linked to Bevy window");
}

// find_slint_window removed — the custom Win32 platform in win32_platform.rs
// stores the HWND atomically, so we read it directly via get_slint_hwnd().

#[cfg(target_os = "windows")]
fn sync_position_win32(state: &NativeComposeState) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    let Some(bevy_raw) = state.bevy_hwnd else { return };
    let Some(_slint_raw) = state.slint_hwnd else { return };
    let bevy_hwnd = to_hwnd(bevy_raw);

    unsafe {
        // Use CLIENT rect (excludes title bar + borders) so the Slint overlay
        // sits below the Bevy window's title bar, leaving OS buttons visible.
        let mut win_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut client_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(bevy_hwnd, &mut win_rect) == 0 {
            return;
        }
        if GetClientRect(bevy_hwnd, &mut client_rect) == 0 {
            return;
        }

        // Compute border/title bar offsets from window vs client rects
        let win_width = win_rect.right - win_rect.left;
        let win_height = win_rect.bottom - win_rect.top;
        let client_width = client_rect.right - client_rect.left;
        let client_height = client_rect.bottom - client_rect.top;
        let border_x = (win_width - client_width) / 2;
        let title_bar_height = win_height - client_height - border_x;

        let client_screen_x = win_rect.left + border_x;
        let client_screen_y = win_rect.top + title_bar_height;

        // Move the Slint overlay to match the Bevy window's client area.
        // set_overlay_geometry also updates the MinimalSoftwareWindow size
        // so the renderer buffer stays in sync.
        super::win32_platform::set_overlay_geometry(client_screen_x, client_screen_y, client_width, client_height);
    }
}
