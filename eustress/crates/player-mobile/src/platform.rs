//! The platform behind the shell: insets, haptics, the auth token, files.
//!
//! Android calls [`eustress_bridge_android`]. iOS calls [`eustress_bridge_ios`].
//! A desktop `cargo check` uses an in-memory token and no haptics, so the
//! shell still builds where there is no phone.

use std::path::PathBuf;
use std::sync::OnceLock;

use eustress_bridge_core::{HapticKind, Haptics, MemoryStore, NoopPlatform, SafeArea, SecureStore};

pub use eustress_bridge_core::SafeInsets;

pub fn insets() -> eustress_common::input_actions::SafeInsets {
    let insets = match current() {
        Which::Android => android_insets(),
        Which::Ios => ios_insets(),
        Which::Host => NoopPlatform.insets(),
    };
    eustress_common::input_actions::SafeInsets {
        top: insets.top,
        bottom: insets.bottom,
        left: insets.left,
        right: insets.right,
    }
}

pub fn haptic(kind: HapticKind) {
    match current() {
        Which::Android => android_haptic(kind),
        Which::Ios => ios_haptic(kind),
        Which::Host => NoopPlatform.play(kind),
    }
}

pub fn token_get(key: &str) -> Result<Option<String>, String> {
    match current() {
        Which::Android => android_store_get(key),
        Which::Ios => ios_store_get(key),
        Which::Host => host_store().get(key),
    }
}

pub fn token_set(key: &str, value: &str) -> Result<(), String> {
    match current() {
        Which::Android => android_store_set(key, value),
        Which::Ios => ios_store_set(key, value),
        Which::Host => host_store().set(key, value),
    }
}

pub fn token_remove(key: &str) -> Result<(), String> {
    match current() {
        Which::Android => android_store_remove(key),
        Which::Ios => ios_store_remove(key),
        Which::Host => host_store().remove(key),
    }
}

/// Unpack the character GLBs shipped in the APK. `None` on a host build,
/// which resolves `bundled://` from the checkout instead.
pub fn install_bundled_assets() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let path = match eustress_bridge_android::install_bundled_assets() {
            Some(path) => path,
            None => {
                eprintln!("eustress player: bundled assets were not installed; the character stays a capsule");
                return None;
            }
        };
        let path = PathBuf::from(path);
        if path.join("characters/y_bot.glb").is_file() {
            return Some(path);
        }
        eprintln!("eustress player: bundled copy at {} has no characters/y_bot.glb", path.display());
        return None;
    }
    #[cfg(not(target_os = "android"))]
    None
}

/// Where downloaded worlds are cached. Android uses the activity's files
/// directory. A host build uses a directory under the temp folder.
pub fn files_dir() -> PathBuf {
    if let Some(dir) = android_files() {
        return PathBuf::from(dir);
    }
    if let Some(dir) = ios_files() {
        return PathBuf::from(dir);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            eprintln!("eustress player: files dir unavailable; downloaded worlds go under the temp directory");
        });
    }
    std::env::temp_dir().join("eustress-player")
}

/// An `eustress-player://` URI the OS handed the activity, if any.
pub fn view_intent_uri() -> Option<String> {
    android_intent().or_else(ios_intent)
}

enum Which {
    Android,
    Ios,
    Host,
}

fn current() -> Which {
    #[cfg(target_os = "android")]
    {
        return Which::Android;
    }
    #[cfg(target_os = "ios")]
    {
        return Which::Ios;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Which::Host
}

fn host_store() -> &'static MemoryStore {
    static STORE: OnceLock<MemoryStore> = OnceLock::new();
    STORE.get_or_init(MemoryStore::default)
}

#[cfg(target_os = "android")]
fn android_insets() -> SafeInsets {
    eustress_bridge_android::AndroidPlatform.insets()
}
#[cfg(not(target_os = "android"))]
fn android_insets() -> SafeInsets {
    SafeInsets::default()
}

#[cfg(target_os = "ios")]
fn ios_insets() -> SafeInsets {
    eustress_bridge_ios::IosPlatform.insets()
}
#[cfg(not(target_os = "ios"))]
fn ios_insets() -> SafeInsets {
    SafeInsets::default()
}

#[cfg(target_os = "android")]
fn android_haptic(kind: HapticKind) {
    eustress_bridge_android::AndroidPlatform.play(kind);
}
#[cfg(not(target_os = "android"))]
fn android_haptic(kind: HapticKind) {
    let _ = kind;
}

#[cfg(target_os = "ios")]
fn ios_haptic(kind: HapticKind) {
    eustress_bridge_ios::IosPlatform.play(kind);
}
#[cfg(not(target_os = "ios"))]
fn ios_haptic(kind: HapticKind) {
    let _ = kind;
}

#[cfg(target_os = "android")]
fn android_store_get(key: &str) -> Result<Option<String>, String> {
    eustress_bridge_android::AndroidPlatform.get(key)
}
#[cfg(not(target_os = "android"))]
fn android_store_get(key: &str) -> Result<Option<String>, String> {
    let _ = key;
    Ok(None)
}

#[cfg(target_os = "android")]
fn android_store_set(key: &str, value: &str) -> Result<(), String> {
    eustress_bridge_android::AndroidPlatform.set(key, value)
}
#[cfg(not(target_os = "android"))]
fn android_store_set(key: &str, value: &str) -> Result<(), String> {
    let _ = (key, value);
    Ok(())
}

#[cfg(target_os = "android")]
fn android_store_remove(key: &str) -> Result<(), String> {
    eustress_bridge_android::AndroidPlatform.remove(key)
}
#[cfg(not(target_os = "android"))]
fn android_store_remove(key: &str) -> Result<(), String> {
    let _ = key;
    Ok(())
}

#[cfg(target_os = "ios")]
fn ios_store_get(key: &str) -> Result<Option<String>, String> {
    eustress_bridge_ios::IosPlatform.get(key)
}
#[cfg(not(target_os = "ios"))]
fn ios_store_get(key: &str) -> Result<Option<String>, String> {
    let _ = key;
    Ok(None)
}

#[cfg(target_os = "ios")]
fn ios_store_set(key: &str, value: &str) -> Result<(), String> {
    eustress_bridge_ios::IosPlatform.set(key, value)
}
#[cfg(not(target_os = "ios"))]
fn ios_store_set(key: &str, value: &str) -> Result<(), String> {
    let _ = (key, value);
    Ok(())
}

#[cfg(target_os = "ios")]
fn ios_store_remove(key: &str) -> Result<(), String> {
    eustress_bridge_ios::IosPlatform.remove(key)
}
#[cfg(not(target_os = "ios"))]
fn ios_store_remove(key: &str) -> Result<(), String> {
    let _ = key;
    Ok(())
}

#[cfg(target_os = "android")]
fn android_files() -> Option<String> {
    eustress_bridge_android::files_dir()
}
#[cfg(not(target_os = "android"))]
fn android_files() -> Option<String> {
    None
}

#[cfg(target_os = "android")]
fn android_intent() -> Option<String> {
    eustress_bridge_android::view_intent_uri()
}
#[cfg(not(target_os = "android"))]
fn android_intent() -> Option<String> {
    None
}

#[cfg(target_os = "ios")]
fn ios_files() -> Option<String> {
    eustress_bridge_ios::files_dir()
}
#[cfg(not(target_os = "ios"))]
fn ios_files() -> Option<String> {
    None
}

#[cfg(target_os = "ios")]
fn ios_intent() -> Option<String> {
    eustress_bridge_ios::open_url()
}
#[cfg(not(target_os = "ios"))]
fn ios_intent() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn player_mobile_does_not_call_jni() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect(&root, &mut files);
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let impl_src = text.split("# [cfg(test)]").next().unwrap_or(&text);
            let impl_src = impl_src.split("#[cfg(test)]").next().unwrap();
            assert!(!impl_src.contains("jni::"), "{path:?} calls JNI; bridge-android owns that");
            assert!(!impl_src.contains("JavaVM"), "{path:?} calls JNI; bridge-android owns that");
            assert!(!impl_src.contains("ndk_context"), "{path:?} reads ndk_context; bridge-android owns that");
        }
    }

    fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
}
