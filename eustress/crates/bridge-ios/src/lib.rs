//! iOS implementations of [`eustress_bridge_core`].
//!
//! Safe area and haptics are UIKit (objc2). The keychain is the Security
//! framework, which is a C API. None of this has been compiled or run: the
//! build machine has no Apple SDK (`xcrun` is absent). Treat every body
//! under `target_os = "ios"` as UNVERIFIED.
//!
//! Host `cargo check` compiles only [`IosPlatform`]'s non-iOS arms, which
//! report that the platform is unavailable.

use eustress_bridge_core::{HapticKind, Haptics, SafeArea, SafeInsets, SecureStore};

pub struct IosPlatform;

impl SafeArea for IosPlatform {
    fn insets(&self) -> SafeInsets {
        #[cfg(target_os = "ios")]
        {
            return ios::insets();
        }
        #[cfg(not(target_os = "ios"))]
        SafeInsets::default()
    }
}

impl Haptics for IosPlatform {
    fn play(&self, kind: HapticKind) {
        #[cfg(target_os = "ios")]
        ios::haptic(kind);
        #[cfg(not(target_os = "ios"))]
        {
            let _ = kind;
        }
    }

    fn supported(&self) -> bool {
        cfg!(target_os = "ios")
    }
}

/// Documents directory. UNVERIFIED: the lookup is only compiled for iOS.
pub fn files_dir() -> Option<String> {
    #[cfg(target_os = "ios")]
    {
        return ios::documents_dir();
    }
    #[cfg(not(target_os = "ios"))]
    None
}

/// The URL iOS asked the process to open. The app delegate has to publish it;
/// this crate has no UIApplication delegate, so the value stays empty until
/// that glue exists. UNVERIFIED.
pub fn open_url() -> Option<String> {
    None
}

impl SecureStore for IosPlatform {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        #[cfg(target_os = "ios")]
        {
            return ios::keychain_get(key);
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = key;
            Err("iOS keychain is only available on iOS (UNVERIFIED, no Apple SDK here)".into())
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        {
            return ios::keychain_set(key, value);
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = (key, value);
            Err("iOS keychain is only available on iOS (UNVERIFIED, no Apple SDK here)".into())
        }
    }

    fn remove(&self, key: &str) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        {
            return ios::keychain_remove(key);
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = key;
            Err("iOS keychain is only available on iOS (UNVERIFIED, no Apple SDK here)".into())
        }
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use super::*;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, ClassType};
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};

    // Security.framework. These signatures match the public C API.
    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecItemCopyMatching(query: *mut AnyObject, result: *mut *mut c_void) -> i32;
        fn SecItemAdd(attributes: *mut AnyObject, result: *mut *mut c_void) -> i32;
        fn SecItemDelete(query: *mut AnyObject) -> i32;
    }

    const ERR_SEC_SUCCESS: i32 = 0;
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    pub fn documents_dir() -> Option<String> {
        // NSSearchPathForDirectoriesInDomains(NSDocumentDirectory, NSUserDomainMask, YES).
        // Left as a stub: the array layout is not something to guess without a device.
        None
    }

    pub fn insets() -> SafeInsets {
        unsafe {
            let ui_app = AnyClass::get(c"UIApplication").expect("UIApplication");
            let app: *mut AnyObject = msg_send![ui_app, sharedApplication];
            let window: *mut AnyObject = msg_send![app, keyWindow];
            if window.is_null() {
                return SafeInsets::default();
            }
            // UIEdgeInsets { top, left, bottom, right } as four f64 on arm64.
            #[repr(C)]
            struct Insets {
                top: f64,
                left: f64,
                bottom: f64,
                right: f64,
            }
            let insets: Insets = msg_send![window, safeAreaInsets];
            let scale: f64 = msg_send![window, contentScaleFactor];
            let scale = if scale > 0.0 { scale as f32 } else { 1.0 };
            // UIKit insets are already in points (logical pixels).
            let _ = scale;
            SafeInsets {
                top: insets.top as f32,
                bottom: insets.bottom as f32,
                left: insets.left as f32,
                right: insets.right as f32,
            }
        }
    }

    pub fn haptic(kind: HapticKind) {
        unsafe {
            match kind {
                HapticKind::Selection => {
                    let class = AnyClass::get(c"UISelectionFeedbackGenerator").expect("selection");
                    let gen: *mut AnyObject = msg_send![class, new];
                    let _: () = msg_send![gen, prepare];
                    let _: () = msg_send![gen, selectionChanged];
                }
                HapticKind::ImpactLight | HapticKind::ImpactMedium | HapticKind::ImpactHeavy => {
                    let style: i64 = match kind {
                        HapticKind::ImpactHeavy => 2,
                        HapticKind::ImpactMedium => 1,
                        _ => 0,
                    };
                    let class = AnyClass::get(c"UIImpactFeedbackGenerator").expect("impact");
                    let gen: *mut AnyObject = msg_send![class, alloc];
                    let gen: *mut AnyObject = msg_send![gen, initWithStyle: style];
                    let _: () = msg_send![gen, impactOccurred];
                }
                HapticKind::Success | HapticKind::Warning | HapticKind::Error => {
                    let note: i64 = match kind {
                        HapticKind::Warning => 1,
                        HapticKind::Error => 2,
                        _ => 0,
                    };
                    let class = AnyClass::get(c"UINotificationFeedbackGenerator").expect("note");
                    let gen: *mut AnyObject = msg_send![class, new];
                    let _: () = msg_send![gen, notificationOccurred: note];
                }
                HapticKind::Custom { .. } => {
                    // Core Haptics is not wired. The preset generators above
                    // are the path that has a written call. UNVERIFIED.
                }
            }
        }
    }

    fn service() -> *mut AnyObject {
        // kSecClassGenericPassword as a CFString constant is not linked by
        // name here; the query is built in keychain_query if a future Mac
        // build fills the dictionary. This stub returns null and the callers
        // fail closed rather than invent a successful read.
        std::ptr::null_mut()
    }

    pub fn keychain_get(key: &str) -> Result<Option<String>, String> {
        let _ = (key, service, ERR_SEC_SUCCESS, ERR_SEC_ITEM_NOT_FOUND);
        // The Security calls are linked above so a Mac build sees them.
        // A complete CFDictionary query is not filled in: without a device
        // this would only be a guess at the toll-free NSDictionary layout.
        Err("iOS keychain read is a stub (UNVERIFIED, no Apple SDK on the build machine)".into())
    }

    pub fn keychain_set(key: &str, value: &str) -> Result<(), String> {
        let _ = (key, value, SecItemAdd as *const (), SecItemCopyMatching as *const (), SecItemDelete as *const ());
        let _ = CString::new(key);
        Err("iOS keychain write is a stub (UNVERIFIED, no Apple SDK on the build machine)".into())
    }

    pub fn keychain_remove(key: &str) -> Result<(), String> {
        let _ = key;
        Err("iOS keychain delete is a stub (UNVERIFIED, no Apple SDK on the build machine)".into())
    }

    #[allow(dead_code)]
    fn cstr(ptr: *const c_char) -> String {
        if ptr.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
    }
}
