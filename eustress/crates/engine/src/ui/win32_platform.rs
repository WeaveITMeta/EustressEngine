// ============================================================================
// Win32 Custom Platform for Slint — Raw Win32 Window + Software Renderer
// ============================================================================
//
// Architecture:
//   Slint runs in a background thread with NO winit dependency. This module
//   provides a custom slint::platform::Platform implementation that:
//
//   1. Creates a raw Win32 overlay window (borderless, layered, transparent)
//   2. Uses Slint's MinimalSoftwareWindow for rendering
//   3. Blits the software-rendered RGBA buffer to the Win32 window via GDI
//   4. Pumps Win32 messages and forwards input to Slint
//   5. Exposes the HWND for native_compose.rs to link with Bevy's window
//
// Table of Contents:
//   - Win32Platform: implements slint::platform::Platform
//   - run_win32_event_loop(): Win32 message pump + Slint rendering loop
//   - wnd_proc(): Win32 window procedure — forwards input to Slint
//   - blit_to_window(): software renderer → GDI DIB → screen
//   - create_overlay_window(): CreateWindowExW with WS_EX_LAYERED
// ============================================================================

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use slint::platform::software_renderer::MinimalSoftwareWindow;
use slint::platform::{Platform, WindowAdapter};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::LibraryLoader::*;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// Win32 constants not exported by our windows-sys feature set.
// Values from the Windows SDK (winuser.h / wingdi.h).
#[cfg(target_os = "windows")]
const WM_ERASEBKGND: u32 = 0x0014;
#[cfg(target_os = "windows")]
const WM_KEYDOWN: u32 = 0x0100;
#[cfg(target_os = "windows")]
const WM_KEYUP: u32 = 0x0101;
#[cfg(target_os = "windows")]
const WM_CHAR: u32 = 0x0102;
#[cfg(target_os = "windows")]
const WM_SYSKEYDOWN: u32 = 0x0104;
#[cfg(target_os = "windows")]
const WM_SYSKEYUP: u32 = 0x0105;
#[cfg(target_os = "windows")]
const WM_MOUSEWHEEL: u32 = 0x020A;
#[cfg(target_os = "windows")]
const WM_MOUSELEAVE: u32 = 0x02A3;

// UpdateLayeredWindow constants for per-pixel alpha compositing
#[cfg(target_os = "windows")]
const ULW_ALPHA: u32 = 0x00000002;
#[cfg(target_os = "windows")]
const AC_SRC_OVER: u8 = 0x00;
#[cfg(target_os = "windows")]
const AC_SRC_ALPHA: u8 = 0x01;

// BLENDFUNCTION struct (not in windows-sys under our features)
#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Copy, Clone)]
struct BLENDFUNCTION {
    BlendOp: u8,
    BlendFlags: u8,
    SourceConstantAlpha: u8,
    AlphaFormat: u8,
}

// UpdateLayeredWindow function pointer (from user32.dll)
#[cfg(target_os = "windows")]
extern "system" {
    fn UpdateLayeredWindow(
        hWnd: HWND,
        hdcDst: HDC,
        pptDst: *const POINT,
        psize: *const SIZE,
        hdcSrc: HDC,
        pptSrc: *const POINT,
        crKey: u32,
        pblend: *const BLENDFUNCTION,
        dwFlags: u32,
    ) -> BOOL;
}

// ============================================================================
// Shared State — Thread-safe HWND Access
// ============================================================================

/// Thread-safe storage for the Slint overlay HWND.
/// native_compose.rs reads this to link the overlay with Bevy's window.
static SLINT_HWND: AtomicIsize = AtomicIsize::new(0);

/// Thread-safe storage for the Bevy window HWND.
/// Used by set_overlay_geometry for z-order positioning (overlay sits just above Bevy).
static BEVY_HWND: AtomicIsize = AtomicIsize::new(0);

/// Get the Slint overlay HWND (returns 0 if not yet created).
pub fn get_slint_hwnd() -> isize {
    SLINT_HWND.load(Ordering::Relaxed)
}

/// Store the Bevy window HWND for z-order reference.
/// Called by native_compose.rs once the Bevy window is found.
pub fn set_bevy_hwnd(hwnd: isize) {
    BEVY_HWND.store(hwnd, Ordering::Relaxed);
}

/// Signal the Win32 event loop to quit.
static QUIT_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn request_quit() {
    QUIT_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

// ============================================================================
// Thread-local: MinimalSoftwareWindow shared between Platform + event loop
// ============================================================================

thread_local! {
    static SLINT_WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = RefCell::new(None);
}

// ============================================================================
// Win32Platform — implements slint::platform::Platform
// ============================================================================

/// Custom Slint platform that uses a raw Win32 window instead of winit.
/// This avoids the winit event loop conflict with Bevy.
pub struct Win32Platform {
    start_time: Instant,
    window: Rc<MinimalSoftwareWindow>,
}

impl Win32Platform {
    pub fn new() -> Self {
        let window = MinimalSoftwareWindow::new(
            slint::platform::software_renderer::RepaintBufferType::ReusedBuffer,
        );
        Self {
            start_time: Instant::now(),
            window,
        }
    }
}

impl Platform for Win32Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        self.start_time.elapsed()
    }

    fn run_event_loop(&self) -> Result<(), slint::PlatformError> {
        // Store the window in thread-local so the event loop can access it
        SLINT_WINDOW.with(|w| {
            *w.borrow_mut() = Some(self.window.clone());
        });

        #[cfg(target_os = "windows")]
        {
            run_win32_event_loop(self.window.clone())?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            return Err(slint::PlatformError::Other(
                "Win32Platform only supported on Windows".into(),
            ));
        }

        Ok(())
    }
}

// ============================================================================
// Win32 Event Loop — Message Pump + Slint Rendering
// ============================================================================

#[cfg(target_os = "windows")]
fn run_win32_event_loop(
    window: Rc<MinimalSoftwareWindow>,
) -> Result<(), slint::PlatformError> {
    // Create the overlay window
    let hwnd = create_overlay_window()?;
    let hwnd_isize = hwnd as isize;
    SLINT_HWND.store(hwnd_isize, Ordering::Relaxed);

    // Tell Slint the window size (start with a reasonable default; native_compose will resize)
    window.set_size(slint::PhysicalSize::new(1600, 900));

    // Pixel buffer for software rendering (RGB format for GDI blit).
    // Viewport pixels render as magenta (#FF00FF) which LWA_COLORKEY makes transparent.
    let mut pixel_buffer: Vec<slint::Rgb8Pixel> = Vec::new();

    // Force an initial render so the layered window has valid pixel data.
    // Without this, Windows marks layered windows as "not responding".
    {
        let size = window.size();
        let w = size.width as usize;
        let h = size.height as usize;
        if w > 0 && h > 0 {
            let needed = w * h;
            pixel_buffer.resize(needed, slint::Rgb8Pixel::default());
            // Request a full redraw then render
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut pixel_buffer, w);
                blit_to_window(hwnd, &pixel_buffer, w as i32, h as i32);
            });
        }
    }

    // Show the window (initially positioned at 0,0; native_compose will reposition)
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
    }

    // Main event loop
    loop {
        // Check quit
        if QUIT_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        // Pump all pending Win32 messages
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return Ok(());
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Let Slint process timers and animations
        slint::platform::update_timers_and_animations();

        // Render if needed
        window.draw_if_needed(|renderer| {
            let size = window.size();
            let width = size.width as usize;
            let height = size.height as usize;

            if width == 0 || height == 0 {
                return;
            }

            // Ensure buffer is correct size
            let needed = width * height;
            if pixel_buffer.len() != needed {
                pixel_buffer.resize(needed, slint::Rgb8Pixel::default());
            }

            // Render into the buffer
            renderer.render(&mut pixel_buffer, width);

            // Blit to the Win32 window
            blit_to_window(hwnd, &pixel_buffer, width as i32, height as i32);
        });

        // Sleep until next timer or a short interval to avoid busy-waiting
        let sleep_duration = slint::platform::duration_until_next_timer_update()
            .unwrap_or(std::time::Duration::from_millis(16))
            .min(std::time::Duration::from_millis(16));

        if !window.has_active_animations() {
            std::thread::sleep(sleep_duration);
        } else {
            // During animations, yield briefly to avoid 100% CPU
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    Ok(())
}

// ============================================================================
// Win32 Window Creation — Borderless Overlay
// ============================================================================

/// Window class name (wide string)
#[cfg(target_os = "windows")]
const CLASS_NAME: &[u16] = &[
    b'E' as u16, b'u' as u16, b's' as u16, b't' as u16, b'r' as u16,
    b'e' as u16, b's' as u16, b's' as u16, b'S' as u16, b'l' as u16,
    b'i' as u16, b'n' as u16, b't' as u16, b'O' as u16, b'v' as u16,
    b'e' as u16, b'r' as u16, b'l' as u16, b'a' as u16, b'y' as u16,
    0, // null terminator
];

#[cfg(target_os = "windows")]
fn create_overlay_window() -> Result<HWND, slint::PlatformError> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        if hinstance.is_null() {
            return Err(slint::PlatformError::Other(
                "GetModuleHandleW failed".into(),
            ));
        }

        // Register the window class
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: CLASS_NAME.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };

        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            // Class may already be registered — that's fine
            let err = GetLastError();
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(slint::PlatformError::Other(
                    format!("RegisterClassExW failed: error {}", err).into(),
                ));
            }
        }

        // Window title (wide string)
        let title: Vec<u16> = "Eustress Engine UI"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // Create a borderless popup window — no title bar, no border.
        // WS_EX_TOOLWINDOW: doesn't appear in taskbar (Bevy window handles that).
        // WS_EX_LAYERED: enables color-key transparency via LWA_COLORKEY.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP, // Start hidden — native_compose will ShowWindow after positioning
            0,
            0,
            1600,
            900,
            std::ptr::null_mut(), // no parent initially; native_compose sets this
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );

        if hwnd.is_null() {
            return Err(slint::PlatformError::Other(
                format!("CreateWindowExW failed: error {}", GetLastError()).into(),
            ));
        }

        // LWA_COLORKEY: all pixels matching magenta (0xFF00FF) become fully
        // transparent and click-through. Normal window input handling is preserved
        // for all non-magenta pixels. COLORREF is BGR: 0x00FF00FF = magenta.
        SetLayeredWindowAttributes(hwnd, 0x00FF00FF, 255, LWA_COLORKEY);

        Ok(hwnd)
    }
}

// ============================================================================
// Window Procedure — Handles Win32 Messages + Slint Input
// ============================================================================

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }

        WM_CLOSE => {
            // Let Slint handle this via the close-requested callback
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::CloseRequested,
                    );
                }
            });
            return 0;
        }

        WM_SIZE => {
            let width = (lparam & 0xFFFF) as u32;
            let height = ((lparam >> 16) & 0xFFFF) as u32;
            if width > 0 && height > 0 {
                SLINT_WINDOW.with(|w| {
                    if let Some(ref win) = *w.borrow() {
                        win.set_size(slint::PhysicalSize::new(width, height));
                    }
                });
            }
            return 0;
        }

        // Mouse move
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerMoved {
                            position: slint::LogicalPosition::new(x, y),
                        },
                    );
                }
            });
            return 0;
        }

        // Mouse buttons
        WM_LBUTTONDOWN => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerPressed {
                            position: slint::LogicalPosition::new(x, y),
                            button: slint::platform::PointerEventButton::Left,
                        },
                    );
                }
            });
            return 0;
        }
        WM_LBUTTONUP => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerReleased {
                            position: slint::LogicalPosition::new(x, y),
                            button: slint::platform::PointerEventButton::Left,
                        },
                    );
                }
            });
            return 0;
        }
        WM_RBUTTONDOWN => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerPressed {
                            position: slint::LogicalPosition::new(x, y),
                            button: slint::platform::PointerEventButton::Right,
                        },
                    );
                }
            });
            return 0;
        }
        WM_RBUTTONUP => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerReleased {
                            position: slint::LogicalPosition::new(x, y),
                            button: slint::platform::PointerEventButton::Right,
                        },
                    );
                }
            });
            return 0;
        }
        WM_MBUTTONDOWN => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerPressed {
                            position: slint::LogicalPosition::new(x, y),
                            button: slint::platform::PointerEventButton::Middle,
                        },
                    );
                }
            });
            return 0;
        }
        WM_MBUTTONUP => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerReleased {
                            position: slint::LogicalPosition::new(x, y),
                            button: slint::platform::PointerEventButton::Middle,
                        },
                    );
                }
            });
            return 0;
        }

        // Mouse wheel
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerScrolled {
                            position: slint::LogicalPosition::new(x, y),
                            delta_x: 0.0,
                            delta_y: delta * 30.0, // Convert to pixels
                        },
                    );
                }
            });
            return 0;
        }

        WM_MOUSELEAVE => {
            SLINT_WINDOW.with(|w| {
                if let Some(ref win) = *w.borrow() {
                    let _ = win.try_dispatch_event(
                        slint::platform::WindowEvent::PointerExited,
                    );
                }
            });
            return 0;
        }

        // Keyboard events
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if let Some(text) = vkey_to_slint_key(wparam as u32) {
                SLINT_WINDOW.with(|w| {
                    if let Some(ref win) = *w.borrow() {
                        let _ = win.try_dispatch_event(
                            slint::platform::WindowEvent::KeyPressed {
                                text: text.into(),
                            },
                        );
                    }
                });
            }
            return 0;
        }
        WM_KEYUP | WM_SYSKEYUP => {
            if let Some(text) = vkey_to_slint_key(wparam as u32) {
                SLINT_WINDOW.with(|w| {
                    if let Some(ref win) = *w.borrow() {
                        let _ = win.try_dispatch_event(
                            slint::platform::WindowEvent::KeyReleased {
                                text: text.into(),
                            },
                        );
                    }
                });
            }
            return 0;
        }

        // Character input (WM_CHAR for typed text)
        WM_CHAR => {
            if let Some(ch) = char::from_u32(wparam as u32) {
                if !ch.is_control() {
                    let text: String = ch.to_string();
                    SLINT_WINDOW.with(|w| {
                        if let Some(ref win) = *w.borrow() {
                            let _ = win.try_dispatch_event(
                                slint::platform::WindowEvent::KeyPressed {
                                    text: text.clone().into(),
                                },
                            );
                            let _ = win.try_dispatch_event(
                                slint::platform::WindowEvent::KeyReleased {
                                    text: text.into(),
                                },
                            );
                        }
                    });
                }
            }
            return 0;
        }

        // Erase background — do nothing (we paint the whole window)
        WM_ERASEBKGND => {
            return 1;
        }

        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ============================================================================
// GDI Blit — Software Renderer Buffer → Win32 Window
// ============================================================================

#[cfg(target_os = "windows")]
fn blit_to_window(hwnd: HWND, buffer: &[slint::Rgb8Pixel], width: i32, height: i32) {
    if width <= 0 || height <= 0 || buffer.is_empty() {
        return;
    }

    unsafe {
        let hdc = GetDC(hwnd);
        if hdc.is_null() {
            return;
        }

        // BITMAPINFO for a top-down 32-bit DIB
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB as u32,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };

        // Convert Rgb8Pixel (3-byte RGB) to BGRA (4-byte) for GDI.
        // Magenta pixels (#FF00FF) from the Slint viewport pass through as-is;
        // LWA_COLORKEY makes them transparent and click-through to Bevy.
        let pixel_count = (width * height) as usize;
        let mut bgra_buffer: Vec<u8> = Vec::with_capacity(pixel_count * 4);
        for i in 0..pixel_count.min(buffer.len()) {
            let pixel = &buffer[i];
            bgra_buffer.push(pixel.b);   // Blue
            bgra_buffer.push(pixel.g);   // Green
            bgra_buffer.push(pixel.r);   // Red
            bgra_buffer.push(255);       // Alpha (unused by StretchDIBits)
        }

        StretchDIBits(
            hdc,
            0,
            0,
            width,
            height,
            0,
            0,
            width,
            height,
            bgra_buffer.as_ptr() as *const _,
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );

        ReleaseDC(hwnd, hdc);
    }
}

// ============================================================================
// Key Mapping — Win32 Virtual Key Codes → Slint Key Strings
// ============================================================================

#[cfg(target_os = "windows")]
fn vkey_to_slint_key(vkey: u32) -> Option<slint::SharedString> {
    use slint::platform::Key;
    let key = match vkey {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Return,
        0x10 => Key::Shift,
        0x11 => Key::Control,
        0x12 => Key::Alt,
        0x13 => return None, // Pause
        0x14 => return None, // Caps Lock
        0x1B => Key::Escape,
        0x20 => return None, // Space handled by WM_CHAR
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::LeftArrow,
        0x26 => Key::UpArrow,
        0x27 => Key::RightArrow,
        0x28 => Key::DownArrow,
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        _ => return None, // Printable keys handled by WM_CHAR
    };
    Some(key.into())
}

// ============================================================================
// Public API — Resize Overlay Window
// ============================================================================

/// Resize and reposition the Slint overlay window.
/// Called by native_compose.rs when the Bevy window moves/resizes.
#[cfg(target_os = "windows")]
pub fn set_overlay_geometry(x: i32, y: i32, width: i32, height: i32) {
    let hwnd_isize = SLINT_HWND.load(Ordering::Relaxed);
    if hwnd_isize == 0 {
        return;
    }
    let hwnd = hwnd_isize as HWND;

    unsafe {
        // HWND_TOP puts overlay at top of z-order without forcing it above all
        // other applications (unlike HWND_TOPMOST). The ownership relationship
        // via GWLP_HWNDPARENT keeps it associated with Bevy for alt-tab/minimize.
        SetWindowPos(
            hwnd,
            HWND_TOP,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE,
        );
    }

    // Also tell Slint about the new size
    SLINT_WINDOW.with(|w| {
        if let Some(ref win) = *w.borrow() {
            if width > 0 && height > 0 {
                win.set_size(slint::PhysicalSize::new(width as u32, height as u32));
            }
        }
    });
}
