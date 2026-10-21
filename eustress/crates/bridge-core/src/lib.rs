//! Traits the mobile Player uses for platform APIs.
//!
//! `bridge-android` implements these with JNI. `bridge-ios` implements them
//! with objc2. Desktop and tests use [`MemoryStore`] and the no-op haptics.

/// Logical pixels. The top inset is the status bar or notch.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeInsets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

pub trait SafeArea {
    fn insets(&self) -> SafeInsets;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HapticKind {
    Selection,
    ImpactLight,
    ImpactMedium,
    ImpactHeavy,
    Success,
    Warning,
    Error,
    Custom { intensity: f32, ms: u32 },
}

pub trait Haptics {
    fn play(&self, kind: HapticKind);
    fn supported(&self) -> bool;
}

/// Auth tokens and other secrets. The key is the desktop login file name
/// (`auth_token` or `auth_token.<scope>`), so a token stays with the API
/// that issued it.
pub trait SecureStore {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn remove(&self, key: &str) -> Result<(), String>;
}

#[derive(Default)]
pub struct NoopPlatform;

impl SafeArea for NoopPlatform {
    fn insets(&self) -> SafeInsets {
        SafeInsets::default()
    }
}

impl Haptics for NoopPlatform {
    fn play(&self, _kind: HapticKind) {}
    fn supported(&self) -> bool {
        false
    }
}

/// Process-local store for tests and a desktop `cargo check` of the shell.
#[derive(Default)]
pub struct MemoryStore {
    values: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecureStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.values.lock().map_err(|e| e.to_string())?.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.values.lock().map_err(|e| e.to_string())?.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), String> {
        self.values.lock().map_err(|e| e.to_string())?.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        assert_eq!(store.get("auth_token").unwrap(), None);
        store.set("auth_token", "abc").unwrap();
        assert_eq!(store.get("auth_token").unwrap().as_deref(), Some("abc"));
        store.remove("auth_token").unwrap();
        assert_eq!(store.get("auth_token").unwrap(), None);
    }
}
