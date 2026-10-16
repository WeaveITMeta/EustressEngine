//! Soul's AI provider keys: where they live, and what the screen shows.
//!
//! One key per provider (Anthropic, xAI, OpenAI), global, kept in the system
//! credential store under `eustress/<provider>/api_key`. A key is never kept
//! in a Space, and this build never writes one to `soul_settings.json`.
//! [`crate::key_fallback`] does the storing: it saves, loads and removes a
//! key, and keeps one in its own private file only while the store cannot
//! take it. Its calls wait on the store's queue and hold the name's guard,
//! so they run here on one Soul key thread, in the order the person acted,
//! and the frame system calls [`SoulKeyStore::poll`] to pick up the
//! results. The UI never waits on the store.
//!
//! An older Studio build keeps its keys in `soul_settings.json`. Every Save
//! and Remove in this build clears that provider's field first, so a key
//! still found there was written after this build last touched the file: it
//! is the newest key, and the start-up import moves it into the store
//! ([`reconcile`]). Until it has moved, an ordinary settings save writes it
//! back unchanged, so a key that exists nowhere else is never dropped.
//!
//! What the screen shows for a provider follows the last action submitted
//! for it; a failed Save or Remove always adds its failure line
//! ([`reconcile`]). No key reaches a log line, an Output line, a `Debug`
//! output or a Slint property: the screen gets [`mask`] only.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;

use bevy::prelude::*;

// The one place the credential store's path appears.
use eustress_common::credentials as secret_store;

use crate::key_fallback::{KeyFallback, KeyGuard, LoadOutcome, Place, RemoveOutcome, SaveOutcome};

use super::{GlobalSoulSettings, Provider};

/// What a key field shows while its key is being looked up.
pub const CHECKING: &str = "checking";

/// What a key field and Workshop show while the store has not answered.
pub const STILL_WAITING: &str = "still waiting for the system credential store";

/// A mask for a key of 16 characters or fewer: no character of it.
pub const MASK_DOTS: &str = "••••";

/// The Output notice for a key found in a Space's SoulService file that is
/// not the saved key (or cannot be compared with it).
pub const SPACE_KEY_NOTICE: &str =
    "This Space contained an API key. It was removed and not used. If it was yours, enter it again in Settings.";

/// The fields `soul_settings.json` kept keys in before this build, per
/// provider. `claude_api_key` is an older Anthropic spelling.
pub const LEGACY_FIELDS: [&[&str]; 3] = [
    &["global_api_key", "claude_api_key"],
    &["global_xai_api_key"],
    &["global_openai_api_key"],
];

/// The three global Soul key names, (provider, purpose), in provider
/// order: the one source of every name this code stores under. Public, so
/// a check can compare it with the store's own list of names.
pub const SOUL_SECRET_NAMES: &[(&str, &str)] = &[("anthropic", "api_key"), ("xai", "api_key"), ("openai", "api_key")];

/// The store's provider segment: `eustress/<this>/api_key`.
pub fn store_id(provider: Provider) -> &'static str {
    SOUL_SECRET_NAMES[slot(provider)].0
}

fn slot(provider: Provider) -> usize {
    match provider {
        Provider::Anthropic => 0,
        Provider::Xai => 1,
        Provider::OpenAi => 2,
    }
}

/// `eustress/<provider>/api_key`, built from [`SOUL_SECRET_NAMES`] only.
pub fn credential_name(provider: Provider) -> Result<secret_store::CredentialName, secret_store::CredentialError> {
    let (id, purpose) = SOUL_SECRET_NAMES[slot(provider)];
    secret_store::CredentialName::new(id, purpose)
}

/// What the screen may show of a key: the public prefix (`sk-ant-`,
/// `sk-proj-`, `sk-` or `xai-`) and the last 4 characters, or dots alone for
/// a key of 16 characters or fewer. Empty for no key.
pub fn mask(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return String::new();
    }
    let count = key.chars().count();
    if count <= 16 {
        return MASK_DOTS.to_string();
    }
    let prefix = ["sk-ant-", "sk-proj-", "xai-", "sk-"]
        .into_iter()
        .find(|p| key.starts_with(p))
        .unwrap_or("");
    let last4: String = key.chars().skip(count - 4).collect();
    format!("{prefix}…{last4}")
}

/// Whether a value typed into a key field (or sent back by the Properties
/// row) is a new key. Empty, the mask on screen, or a busy text is not: the
/// panel can send back what it shows, and only a newly typed key is stored.
pub fn is_new_key(settings: &GlobalSoulSettings, provider: Provider, value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value != settings.key_placeholder(provider)
        && value != mask(settings.session_key(provider))
        && value != CHECKING
        && value != STILL_WAITING
        && !value.contains('…')
        && !value.contains('•')
}

/// One provider's key as `soul_settings.json` held it at start-up, and
/// whether it has moved into the store (then the file stops carrying it).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LegacyKey {
    pub value: String,
    pub moved: bool,
}

impl std::fmt::Debug for LegacyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LegacyKey({:?}, moved: {})", mask(&self.value), self.moved)
    }
}

/// What a provider's key field is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Busy {
    #[default]
    Idle,
    /// Looking the key up: at start-up, or after a failed Save or Remove.
    Checking,
    Saving,
    Removing,
    /// A lookup passed its timeout and the store has not answered.
    Waiting,
}

/// A provider's key on screen, besides the key itself. Holds no key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeySlot {
    pub busy: Busy,
    /// The store or the fallback file needs attention (shown on the AI keys
    /// page); replaced by the next result for this provider.
    pub warning: Option<String>,
    /// A failed Save or Remove. Stays on screen, whatever runs after it,
    /// until a later Save or Remove of this provider succeeds.
    pub failure: Option<String>,
}

// ============================================================================
// The rules, in one place
// ============================================================================

/// A question for [`reconcile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Start-up found this provider's key in `soul_settings.json`.
    LegacyKeyFound,
    /// A result came back for the action numbered `of`; `current` is the
    /// number of the last action submitted for the provider.
    Result { of: u64, current: u64, failed_write: bool },
}

/// What [`reconcile`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Move the file's key into the store with the helper's save; the
    /// file's copy goes only after that returns Stored or KeptInFile.
    MoveFileKey,
    /// The result is the screen's: apply all of it.
    Apply,
    /// An earlier Save or Remove failed: add its failure line, change
    /// nothing else.
    FailureLineOnly,
    /// An earlier result: it never changes the screen.
    Discard,
}

/// The conflict rule and the ordering rule for one provider's key.
///
/// The invariant: a key still in `soul_settings.json` is the newest key.
/// Every Save and Remove clears its provider's field from that file before
/// it reaches the store, so a key found there at start-up was written later,
/// by an older Studio build running beside this one. It therefore wins over
/// the store's value and the fallback file's: with the name's guard held,
/// it is moved into the store with `save_within` (no load before it), and
/// the file's copy is cleared only after the move returns Stored or
/// KeptInFile: clearing first would lose the key whenever the move fails.
///
/// Every action on a provider (start-up load, Save, Remove, the lookup that
/// follows a failed write) takes the next number from
/// [`SoulKeyStore::submit_number`]. A result reaches the screen only when it
/// belongs to the last action submitted, so a start-up read that comes back
/// late is applied only if no Save or Remove was submitted since it started,
/// and an earlier result never changes the mask. The one exception: a failed
/// Save or Remove adds its failure line whatever was submitted after it.
pub fn reconcile(rule: Rule) -> Verdict {
    match rule {
        Rule::LegacyKeyFound => Verdict::MoveFileKey,
        Rule::Result { of, current, failed_write } => {
            if of == current {
                Verdict::Apply
            } else if failed_write {
                Verdict::FailureLineOnly
            } else {
                Verdict::Discard
            }
        }
    }
}

// ============================================================================
// The settings and their session keys
// ============================================================================

/// What `soul_settings.json` gave at start-up.
#[derive(Default)]
pub struct SettingsLoad {
    /// The file is there but is not JSON: its keys cannot be read.
    pub unreadable: Option<PathBuf>,
}

impl GlobalSoulSettings {
    /// This session's key for `provider` ("" for none).
    pub fn session_key(&self, provider: Provider) -> &str {
        match provider {
            Provider::Anthropic => &self.global_api_key,
            Provider::Xai => &self.global_xai_api_key,
            Provider::OpenAi => &self.global_openai_api_key,
        }
    }

    fn set_session_key(&mut self, provider: Provider, key: String) {
        if provider == Provider::Anthropic && self.global_api_key.trim() != key.trim() {
            self.api_key_valid = None;
        }
        match provider {
            Provider::Anthropic => self.global_api_key = key,
            Provider::Xai => self.global_xai_api_key = key,
            Provider::OpenAi => self.global_openai_api_key = key,
        }
    }

    pub fn key_slot(&self, provider: Provider) -> &KeySlot {
        &self.key_slots[slot(provider)]
    }

    fn key_slot_mut(&mut self, provider: Provider) -> &mut KeySlot {
        &mut self.key_slots[slot(provider)]
    }

    /// What an empty key field shows: "checking" while the key is looked
    /// up, the waiting text when the store has not answered, otherwise the
    /// saved key's mask ("" for none).
    pub fn key_placeholder(&self, provider: Provider) -> String {
        match self.key_slot(provider).busy {
            Busy::Checking => CHECKING.to_string(),
            Busy::Waiting if self.session_key(provider).trim().is_empty() => STILL_WAITING.to_string(),
            _ => mask(self.session_key(provider)),
        }
    }

    /// Remove is offered for a saved key only: the field shows its mask.
    pub fn can_remove_key(&self, provider: Provider) -> bool {
        matches!(self.key_slot(provider).busy, Busy::Idle | Busy::Waiting)
            && !self.session_key(provider).trim().is_empty()
    }

    /// No key yet, and the store has not answered: Workshop waits instead of
    /// reporting a missing key.
    pub fn key_pending(&self, provider: Provider) -> bool {
        self.session_key(provider).trim().is_empty()
            && matches!(self.key_slot(provider).busy, Busy::Checking | Busy::Waiting)
    }

    /// Workshop's status while [`Self::key_pending`].
    pub fn key_wait_text(&self, provider: Provider) -> Option<String> {
        if !self.key_pending(provider) {
            return None;
        }
        Some(match self.key_slot(provider).busy {
            Busy::Waiting => format!("{} key: {STILL_WAITING}.", provider.label()),
            _ => format!("Checking the saved {} key.", provider.label()),
        })
    }

    /// The AI keys page's status lines: failures, warnings and waits.
    pub fn key_status_text(&self) -> String {
        let mut lines = Vec::new();
        for provider in Provider::ALL {
            let slot = self.key_slot(provider);
            match slot.busy {
                Busy::Saving => lines.push(format!("{}: saving the key.", provider.label())),
                Busy::Removing => lines.push(format!("{}: removing the saved key.", provider.label())),
                Busy::Waiting => lines.push(format!("{}: {STILL_WAITING}.", provider.label())),
                Busy::Idle | Busy::Checking => {}
            }
            lines.extend(slot.failure.iter().cloned());
            lines.extend(slot.warning.iter().cloned());
        }
        lines.join("\n")
    }

    /// Read `soul_settings.json`. Never fails: a missing, empty or malformed
    /// file gives the defaults, as before. The key fields are read leniently
    /// from the raw JSON, so a bad value elsewhere does not lose them; they
    /// become this build's legacy keys, and no session key is set until the
    /// store answers.
    pub fn load_from(path: &Path) -> (Self, SettingsLoad) {
        let mut load = SettingsLoad::default();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!("No Soul settings file found. Using default settings.");
                return (Self::default(), load);
            }
            Err(e) => {
                warn!("Failed to read Soul settings file: {}. Using defaults.", e);
                load.unreadable = Some(path.to_path_buf());
                return (Self::default(), load);
            }
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return (Self::default(), load);
        }
        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(e) => {
                warn!("Soul settings file is not JSON: {}. Using defaults.", e);
                load.unreadable = Some(path.to_path_buf());
                return (Self::default(), load);
            }
        };
        let mut settings = match serde_json::from_value::<GlobalSoulSettings>(value.clone()) {
            Ok(settings) => settings,
            Err(e) => {
                warn!("Failed to parse Soul settings file: {}. Using defaults.", e);
                Self::default()
            }
        };
        for (i, fields) in LEGACY_FIELDS.iter().enumerate() {
            let found = fields
                .iter()
                .filter_map(|field| value.get(*field).and_then(serde_json::Value::as_str))
                .map(str::trim)
                .find(|key| !key.is_empty());
            settings.legacy_keys[i] = LegacyKey { value: found.unwrap_or_default().to_string(), moved: false };
        }
        info!("Loaded global Soul settings from {:?}", path);
        (settings, load)
    }

    /// The settings file's JSON: every setting; no session key; and each key
    /// field `on_disk` still holds unless this build moved that value or
    /// `clearing` names its provider. `on_disk` is the file as it is now, so
    /// a key an older build wrote since start-up survives an ordinary save.
    pub fn settings_file_json(&self, on_disk: Option<&serde_json::Value>, clearing: Option<Provider>) -> Result<String, String> {
        let mut out = serde_json::to_value(self).map_err(|e| format!("Failed to serialize Soul settings: {e}"))?;
        let object = out
            .as_object_mut()
            .ok_or_else(|| "Soul settings did not serialize to an object".to_string())?;
        for (i, fields) in LEGACY_FIELDS.iter().enumerate() {
            if clearing.is_some_and(|p| slot(p) == i) {
                continue;
            }
            for field in fields.iter() {
                let Some(kept) = on_disk.and_then(|d| d.get(*field)).and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let legacy = &self.legacy_keys[i];
                if kept.trim().is_empty() || (legacy.moved && kept.trim() == legacy.value) {
                    continue;
                }
                object.insert((*field).to_string(), serde_json::Value::String(kept.to_string()));
            }
        }
        serde_json::to_string_pretty(&out).map_err(|e| format!("Failed to serialize Soul settings: {e}"))
    }

    /// Write `soul_settings.json` as an ordinary file, through a temporary
    /// name renamed into place. It holds no key of this build's; see
    /// [`Self::settings_file_json`] for the older build's keys it keeps.
    pub fn save_to(&self, path: &Path, clearing: Option<Provider>) -> Result<(), String> {
        let _file = settings_file_lock();
        let on_disk = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        let json = self.settings_file_json(on_disk.as_ref(), clearing)?;
        let dir = path.parent().ok_or_else(|| "Soul settings path has no folder".to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create settings directory: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes())
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("Failed to write Soul settings file: {e}")
            })
    }

}

impl std::fmt::Debug for GlobalSoulSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalSoulSettings")
            .field("global_api_key", &mask(&self.global_api_key))
            .field("api_key_valid", &self.api_key_valid)
            .field("use_global_for_new_spaces", &self.use_global_for_new_spaces)
            .field("workshop_model", &self.workshop_model)
            .field("global_xai_api_key", &mask(&self.global_xai_api_key))
            .field("global_openai_api_key", &mask(&self.global_openai_api_key))
            .field("workshop_gauntlet", &self.workshop_gauntlet)
            .field("workshop_auto_approve", &self.workshop_auto_approve)
            .field("legacy_keys", &self.legacy_keys)
            .field("key_slots", &self.key_slots)
            .finish()
    }
}

// ============================================================================
// The vault: every store call Soul makes
// ============================================================================

/// One credential name, held: every call on it inside [`KeyVault::with_lock`].
pub trait LockedKey {
    /// [`KeyFallback::save`]: the fallback copy cleared first, then stored.
    fn save(&self, key: &str) -> SaveOutcome;
    /// [`KeyFallback::save_within`]: the start-up migration passes `TIMEOUT`.
    fn save_within(&self, key: &str, limit: std::time::Duration) -> SaveOutcome;
    /// [`KeyFallback::load`]: the session copy, then the file, then the store.
    fn load(&self) -> LoadOutcome;
    /// [`KeyFallback::remove`]: the fallback copy first, then the store.
    fn remove(&self) -> RemoveOutcome;
}

/// Every store call Soul makes, so tests can stand in for the system store.
///
/// `with_lock` holds the name for the whole of `step` (MCP's `KeyGuard`), so
/// a Save's clear of the settings field and its store write, or a start-up
/// migration's read, write and clear, cannot interleave with another call on
/// that name. It waits while another thread holds the name: it runs on the
/// Soul key thread only, never in a frame system.
pub trait KeyVault: Send + Sync {
    fn with_lock(&self, name: &secret_store::CredentialName, step: &mut dyn FnMut(&dyn LockedKey));
    /// [`KeyFallback::peek`]: the person's key now, read only (no guard,
    /// nothing moved), waiting at most `limit`; for the older-build notice.
    fn peek(&self, name: &secret_store::CredentialName, limit: std::time::Duration) -> Result<Option<String>, String>;
}

/// MCP's helper, holding one name's guard.
struct Held<'a> {
    helper: &'a KeyFallback,
    guard: &'a KeyGuard,
}

impl LockedKey for Held<'_> {
    fn save(&self, key: &str) -> SaveOutcome {
        self.helper.save(self.guard, key)
    }
    fn save_within(&self, key: &str, limit: std::time::Duration) -> SaveOutcome {
        self.helper.save_within(self.guard, key, limit)
    }
    fn load(&self) -> LoadOutcome {
        self.helper.load(self.guard)
    }
    fn remove(&self) -> RemoveOutcome {
        self.helper.remove(self.guard)
    }
}

impl KeyVault for KeyFallback {
    fn with_lock(&self, name: &secret_store::CredentialName, step: &mut dyn FnMut(&dyn LockedKey)) {
        let guard = self.lock(name);
        step(&Held { helper: self, guard: &guard });
    }
    fn peek(&self, name: &secret_store::CredentialName, limit: std::time::Duration) -> Result<Option<String>, String> {
        KeyFallback::peek(self, name, limit)
    }
}

/// The helper cannot find its data folder: nothing could be kept on disk.
const NO_DATA_FOLDER: &str = "the folder for the fallback key file could not be found";

/// No data folder: no fallback file, so no key can be kept or found there.
struct NoFolder;

impl LockedKey for NoFolder {
    fn save(&self, _key: &str) -> SaveOutcome {
        SaveOutcome::SessionOnly(NO_DATA_FOLDER.to_string())
    }
    fn save_within(&self, _key: &str, _limit: std::time::Duration) -> SaveOutcome {
        SaveOutcome::SessionOnly(NO_DATA_FOLDER.to_string())
    }
    fn load(&self) -> LoadOutcome {
        LoadOutcome::StoreError(NO_DATA_FOLDER.to_string())
    }
    fn remove(&self) -> RemoveOutcome {
        RemoveOutcome::Aborted(NO_DATA_FOLDER.to_string())
    }
}

/// The system store with its fallback folder ([`KeyFallback::standard`]),
/// resolved per call.
pub struct StandardVault;

impl KeyVault for StandardVault {
    fn with_lock(&self, name: &secret_store::CredentialName, step: &mut dyn FnMut(&dyn LockedKey)) {
        match KeyFallback::standard() {
            Some(helper) => helper.with_lock(name, step),
            None => step(&NoFolder),
        }
    }
    fn peek(&self, name: &secret_store::CredentialName, limit: std::time::Duration) -> Result<Option<String>, String> {
        match KeyFallback::standard() {
            Some(helper) => helper.peek(name, limit),
            None => Err(NO_DATA_FOLDER.to_string()),
        }
    }
}

// ============================================================================
// The settings file's key fields (on the Soul key thread, under the guard)
// ============================================================================

/// Serializes every write of `soul_settings.json` in this process: the
/// frame's ordinary saves and the Soul key thread's field clears.
static SETTINGS_FILE: Mutex<()> = Mutex::new(());

fn settings_file_lock() -> std::sync::MutexGuard<'static, ()> {
    SETTINGS_FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Write `value` over the settings file, through a temporary name renamed
/// into place. An ordinary file: it holds no key of this build's.
fn replace_settings_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes()).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The settings file as JSON: `None` when it is missing or is not JSON (it
/// then holds no key that can be read), `Err` when it cannot be read.
fn settings_file_value(path: &Path) -> Result<Option<serde_json::Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice::<serde_json::Value>(&bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{} could not be read ({e})", path.display())),
    }
}

/// `provider`'s key in the settings file as it is now.
fn file_field(path: &Path, provider: Provider) -> Result<Option<String>, String> {
    let value = settings_file_value(path)?;
    Ok(value.and_then(|v| {
        LEGACY_FIELDS[slot(provider)]
            .iter()
            .filter_map(|field| v.get(*field).and_then(serde_json::Value::as_str))
            .map(str::trim)
            .find(|key| !key.is_empty())
            .map(str::to_string)
    }))
}

/// Remove `provider`'s key fields from the settings file, keeping the rest
/// as it is: the key that was there. `Err`: the file was not changed.
fn clear_file_field(path: &Path, provider: Provider) -> Result<Option<String>, String> {
    let _file = settings_file_lock();
    let Some(mut value) = settings_file_value(path)? else { return Ok(None) };
    let Some(object) = value.as_object_mut() else { return Ok(None) };
    let mut removed = None;
    for field in LEGACY_FIELDS[slot(provider)] {
        if let Some(serde_json::Value::String(key)) = object.remove(*field) {
            if removed.is_none() && !key.trim().is_empty() {
                removed = Some(key.trim().to_string());
            }
        }
    }
    if removed.is_none() {
        return Ok(None);
    }
    replace_settings_file(path, &value).map_err(|e| format!("{} could not be updated ({e})", path.display()))?;
    Ok(removed)
}

/// Put `older` back as `provider`'s key field (a Save that cleared it ended
/// Aborted, so nothing reached the store).
fn restore_file_field(path: &Path, provider: Provider, older: &str) -> Result<(), String> {
    let _file = settings_file_lock();
    let mut value = settings_file_value(path)?.unwrap_or_else(|| serde_json::json!({}));
    let object = value.as_object_mut().ok_or_else(|| "it is not a JSON object".to_string())?;
    object.insert(LEGACY_FIELDS[slot(provider)][0].to_string(), serde_json::Value::String(older.to_string()));
    replace_settings_file(path, &value)
}

// ============================================================================
// The resource
// ============================================================================

/// A line for the Output panel. Never holds a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Info(String),
    Warn(String),
}

/// What a lookup of the key (the vault's load) is for.
enum After {
    /// A start-up load that failed.
    Startup,
    /// A Save that did not end Stored.
    Save { key: String },
    /// A Remove that did not end Removed.
    Remove,
}

enum Finished {
    /// Start-up: no key field in the settings file; the vault's load.
    Loaded(LoadOutcome),
    /// Start-up: the settings file's key, moved with save_within. `cleared`:
    /// the field's clear after Stored or KeptInFile (`None`: not cleared).
    /// `older_build`: peek found a different saved key before the move.
    Imported { outcome: SaveOutcome, key: String, cleared: Option<Result<(), String>>, older_build: bool },
    /// A Save. `cleared`: the settings field was cleared first; `restored`:
    /// it was put back after Aborted.
    Saved { outcome: SaveOutcome, key: String, cleared: bool, restored: Option<Result<(), String>> },
    /// The settings field could not be cleared: the Save changed nothing.
    SaveRefused(String),
    Removed(RemoveOutcome),
    /// The settings field could not be cleared: the Remove changed nothing.
    RemoveRefused(String),
    LookedUp { after: After, outcome: LoadOutcome },
    /// A Space's key compared with the saved one: not a screen action.
    SpaceChecked { same: bool },
}

struct Report {
    provider: Provider,
    number: u64,
    finished: Finished,
}

type Job = Box<dyn FnOnce() + Send>;

/// Soul's keys in flight: the Soul key thread and its results. See the
/// module docs.
#[derive(Resource)]
pub struct SoulKeyStore {
    vault: std::sync::Arc<dyn KeyVault>,
    settings_path: Option<PathBuf>,
    numbers: [u64; 3],
    worker: Option<mpsc::Sender<Job>>,
    reports_tx: mpsc::Sender<Report>,
    reports: Mutex<mpsc::Receiver<Report>>,
    imports_left: usize,
    imported: usize,
    pending_remove: Option<Provider>,
    lines: Vec<Line>,
}

impl SoulKeyStore {
    /// `settings_path`: `soul_settings.json`, or `None` when the home folder
    /// cannot be found. Stores through [`StandardVault`].
    pub fn new(settings_path: Option<PathBuf>) -> Self {
        Self::with_vault(settings_path, std::sync::Arc::new(StandardVault))
    }

    /// [`Self::new`] over another vault (tests).
    pub fn with_vault(settings_path: Option<PathBuf>, vault: std::sync::Arc<dyn KeyVault>) -> Self {
        let (reports_tx, reports) = mpsc::channel();
        Self {
            vault,
            settings_path,
            numbers: [0; 3],
            worker: None,
            reports_tx,
            reports: Mutex::new(reports),
            imports_left: 0,
            imported: 0,
            pending_remove: None,
            lines: Vec::new(),
        }
    }

    /// The next action number for `provider`: the screen follows the
    /// highest (see [`reconcile`]).
    pub fn submit_number(&mut self, provider: Provider) -> u64 {
        self.numbers[slot(provider)] += 1;
        self.numbers[slot(provider)]
    }

    /// The Output lines since the last call.
    pub fn take_lines(&mut self) -> Vec<Line> {
        std::mem::take(&mut self.lines)
    }

    /// A line for the Output panel from outside the key flows.
    pub fn warn(&mut self, line: String) {
        self.lines.push(Line::Warn(line));
    }

    /// Run `job` on the Soul key thread, after every job submitted before
    /// it: one call at a time, in order.
    fn run_on_worker(&mut self, provider: Provider, job: Job) {
        let job = match &self.worker {
            Some(worker) => match worker.send(job) {
                Ok(()) => return,
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new().name("soul keys".to_string()).spawn(move || {
            for job in rx {
                job();
            }
        });
        if spawned.is_err() {
            // No thread could start: say so rather than block the frame on
            // the store.
            let number = self.numbers[slot(provider)];
            let _ = self.reports_tx.send(Report {
                provider,
                number,
                finished: Finished::LookedUp {
                    after: After::Startup,
                    outcome: LoadOutcome::StoreError("no thread for the key store could start".to_string()),
                },
            });
            return;
        }
        let _ = tx.send(job);
        self.worker = Some(tx);
    }

    /// Run `work` on the Soul key thread with `name` held, and report its
    /// result under `number`.
    fn send_report(
        &mut self,
        provider: Provider,
        number: u64,
        name: secret_store::CredentialName,
        work: impl FnOnce(&dyn LockedKey) -> Finished + Send + 'static,
    ) {
        let tx = self.reports_tx.clone();
        let vault = self.vault.clone();
        self.run_on_worker(
            provider,
            Box::new(move || {
                let mut work = Some(work);
                let mut finished = None;
                vault.with_lock(&name, &mut |key| {
                    if let Some(work) = work.take() {
                        finished = Some(work(key));
                    }
                });
                let finished = finished.unwrap_or(Finished::LookedUp {
                    after: After::Startup,
                    outcome: LoadOutcome::StoreError("the key store did not run the call".to_string()),
                });
                let _ = tx.send(Report { provider, number, finished });
            }),
        );
    }

    /// [`Self::send_report`], with a read-only peek first (when
    /// `peek_first`), taken before the guard and waiting at most TIMEOUT.
    fn send_report_after_peek(
        &mut self,
        provider: Provider,
        number: u64,
        name: secret_store::CredentialName,
        peek_first: bool,
        work: impl FnOnce(&dyn LockedKey, Option<Result<Option<String>, String>>) -> Finished + Send + 'static,
    ) {
        let tx = self.reports_tx.clone();
        let vault = self.vault.clone();
        self.run_on_worker(
            provider,
            Box::new(move || {
                let mut peeked = peek_first.then(|| vault.peek(&name, secret_store::TIMEOUT));
                let mut work = Some(work);
                let mut finished = None;
                vault.with_lock(&name, &mut |key| {
                    if let Some(work) = work.take() {
                        finished = Some(work(key, peeked.take()));
                    }
                });
                let finished = finished.unwrap_or(Finished::LookedUp {
                    after: After::Startup,
                    outcome: LoadOutcome::StoreError("the key store did not run the call".to_string()),
                });
                let _ = tx.send(Report { provider, number, finished });
            }),
        );
    }

    /// Start-up: each provider's key, off the UI thread, with its name held.
    /// A key the settings file still holds is the newest: it moves into the
    /// store first ([`reconcile`]), with no load before it.
    pub fn start_load(&mut self, settings: &mut GlobalSoulSettings, load: &SettingsLoad) {
        if let Some(path) = &load.unreadable {
            self.lines.push(Line::Warn(format!(
                "Saved AI keys could not be read from {}; enter them again.",
                path.display()
            )));
        }
        for provider in Provider::ALL {
            let number = self.submit_number(provider);
            settings.key_slot_mut(provider).busy = Busy::Checking;
            let name = match credential_name(provider) {
                Ok(name) => name,
                Err(e) => {
                    settings.key_slot_mut(provider).busy = Busy::Idle;
                    settings.key_slot_mut(provider).warning = Some(e.to_string());
                    continue;
                }
            };
            if !settings.legacy_keys[slot(provider)].value.is_empty() {
                self.imports_left += 1;
            }
            let path = self.settings_path.clone();
            // Only a provider whose settings field held a key at start-up
            // peeks, read only and before the guard, for the notice.
            let peek_first = !settings.legacy_keys[slot(provider)].value.is_empty();
            self.send_report_after_peek(provider, number, name, peek_first, move |key, peeked| {
                let field = path.as_deref().map(|p| file_field(p, provider)).unwrap_or(Ok(None));
                match field {
                    Ok(Some(legacy)) if reconcile(Rule::LegacyKeyFound) == Verdict::MoveFileKey => {
                        // A start-up call: it waits at most TIMEOUT, whatever
                        // the platform; a timeout keeps the key in the file.
                        let outcome = key.save_within(&legacy, secret_store::TIMEOUT);
                        let cleared = match outcome {
                            SaveOutcome::Stored | SaveOutcome::KeptInFile(_) => {
                                path.as_deref().map(|p| clear_file_field(p, provider).map(|_| ()))
                            }
                            _ => None,
                        };
                        // Informational only: a read that races the move is fine.
                        let older_build =
                            matches!(peeked, Some(Ok(Some(ref saved))) if saved.trim() != legacy.trim());
                        Finished::Imported { outcome, key: legacy, cleared, older_build }
                    }
                    _ => Finished::Loaded(key.load()),
                }
            });
        }
    }

    /// The Settings Save and the Properties ApiKey row. Each value that is a
    /// new key goes to the store; an empty field, or one showing the mask,
    /// changes nothing and never clears a saved key. Returns how many saves
    /// started.
    pub fn save_typed(&mut self, settings: &mut GlobalSoulSettings, typed: &[(Provider, &str)]) -> usize {
        let mut started = 0;
        for &(provider, value) in typed {
            if !is_new_key(settings, provider, value) {
                continue;
            }
            if self.submit_save(settings, provider, value.trim().to_string()) {
                started += 1;
            }
        }
        started
    }

    fn submit_save(&mut self, settings: &mut GlobalSoulSettings, provider: Provider, new_key: String) -> bool {
        let name = match credential_name(provider) {
            Ok(name) => name,
            Err(e) => {
                settings.key_slot_mut(provider).failure = Some(e.to_string());
                return false;
            }
        };
        let number = self.submit_number(provider);
        settings.key_slot_mut(provider).busy = Busy::Saving;
        let path = self.settings_path.clone();
        self.send_report(provider, number, name, move |key| {
            // One guard for the whole Save: the settings field first.
            let removed = match path.as_deref().map(|p| clear_file_field(p, provider)) {
                Some(Err(why)) => return Finished::SaveRefused(why),
                Some(Ok(removed)) => removed,
                None => None,
            };
            let outcome = key.save(&new_key);
            // Back only after Aborted (nothing reached the store). After
            // SessionOnly a timed-out store write can still land, so a
            // restored copy would put the older key back at the next start.
            let restored = match (&outcome, &removed, path.as_deref()) {
                (SaveOutcome::Aborted(_), Some(older), Some(p)) => Some(restore_file_field(p, provider, older)),
                _ => None,
            };
            Finished::Saved { outcome, key: new_key, cleared: removed.is_some(), restored }
        });
        true
    }

    /// Remove asked for: the confirmation's text, or `None` when there is no
    /// saved key to remove.
    pub fn request_remove(&mut self, settings: &GlobalSoulSettings, provider: Provider) -> Option<String> {
        if !settings.can_remove_key(provider) {
            return None;
        }
        self.pending_remove = Some(provider);
        Some(format!(
            "Remove the saved {} key from this computer? Workshop will stop using it until you enter one again.",
            provider.label()
        ))
    }

    /// Cancel: nothing changes, and nothing is called.
    pub fn cancel_remove(&mut self) {
        self.pending_remove = None;
    }

    /// The provider a Remove confirmation is open for.
    pub fn pending_remove(&self) -> Option<Provider> {
        self.pending_remove
    }

    /// Confirmed: one guard; the settings field first, then the vault's
    /// remove.
    pub fn confirm_remove(&mut self, settings: &mut GlobalSoulSettings) -> bool {
        let Some(provider) = self.pending_remove.take() else { return false };
        let name = match credential_name(provider) {
            Ok(name) => name,
            Err(e) => {
                settings.key_slot_mut(provider).failure = Some(e.to_string());
                return false;
            }
        };
        let number = self.submit_number(provider);
        settings.key_slot_mut(provider).busy = Busy::Removing;
        let path = self.settings_path.clone();
        self.send_report(provider, number, name, move |key| {
            if let Some(Err(why)) = path.as_deref().map(|p| clear_file_field(p, provider)) {
                return Finished::RemoveRefused(why);
            }
            Finished::Removed(key.remove())
        });
        true
    }

    /// A Space's SoulService file held `removed`: it is never used or
    /// stored. One notice follows unless it equals the saved Anthropic key,
    /// as the vault's load finds it.
    pub fn check_space_key(&mut self, removed: String) {
        let Ok(name) = credential_name(Provider::Anthropic) else {
            self.lines.push(Line::Warn(SPACE_KEY_NOTICE.to_string()));
            return;
        };
        self.send_report(Provider::Anthropic, 0, name, move |key| {
            let same = matches!(key.load(), LoadOutcome::Key { ref value, .. } if value.trim() == removed.trim());
            Finished::SpaceChecked { same }
        });
    }

    /// The key as the vault's load finds it, after every earlier call on it.
    fn look_up(&mut self, settings: &mut GlobalSoulSettings, provider: Provider, after: After) {
        let Ok(name) = credential_name(provider) else { return };
        let number = self.submit_number(provider);
        let slot_now = settings.key_slot_mut(provider);
        let overdue = [&slot_now.warning, &slot_now.failure]
            .iter()
            .any(|text| text.as_deref().is_some_and(|t| t.contains(STILL_WAITING)));
        slot_now.busy = if overdue { Busy::Waiting } else { Busy::Checking };
        self.send_report(provider, number, name, move |key| Finished::LookedUp { after, outcome: key.load() });
    }

    /// Apply what came back. Returns whether anything changed.
    pub fn poll(&mut self, settings: &mut GlobalSoulSettings) -> bool {
        let reports: Vec<Report> = match self.reports.lock() {
            Ok(rx) => rx.try_iter().collect(),
            Err(_) => Vec::new(),
        };
        let changed = !reports.is_empty();
        for report in reports {
            self.apply(settings, report);
        }
        changed
    }

    fn apply(&mut self, settings: &mut GlobalSoulSettings, report: Report) {
        let provider = report.provider;
        let label = provider.label();
        if let Finished::SpaceChecked { same } = report.finished {
            if !same {
                self.lines.push(Line::Warn(SPACE_KEY_NOTICE.to_string()));
            }
            return;
        }
        let failed_write = match &report.finished {
            Finished::Saved { outcome, .. } => !matches!(outcome, SaveOutcome::Stored),
            Finished::Removed(outcome) => !matches!(outcome, RemoveOutcome::Removed(_)),
            Finished::SaveRefused(_) | Finished::RemoveRefused(_) => true,
            _ => false,
        };

        // The settings file's bookkeeping, whatever reaches the screen.
        match &report.finished {
            Finished::Imported { cleared, older_build, outcome, .. } => {
                self.imports_left = self.imports_left.saturating_sub(1);
                if *older_build && matches!(outcome, SaveOutcome::Stored | SaveOutcome::KeptInFile(_)) {
                    self.lines.push(Line::Info(format!(
                        "Moved a key saved by an older Studio build into the system credential store: {label}."
                    )));
                }
                match cleared {
                    Some(Ok(())) => {
                        settings.legacy_keys[slot(provider)].moved = true;
                        self.imported += 1;
                    }
                    Some(Err(e)) => self.lines.push(Line::Warn(format!(
                        "The {label} key moved to the system credential store, but soul_settings.json still holds it: {}. Moving it is tried again at the next start.",
                        clause(e)
                    ))),
                    None => {}
                }
                if self.imports_left == 0 && self.imported > 0 {
                    info!("Soul keys moved to the credential store ({} entries)", self.imported);
                    self.imported = 0;
                }
            }
            Finished::Saved { cleared, restored, .. } => {
                if *cleared {
                    settings.legacy_keys[slot(provider)].moved = true;
                }
                match restored {
                    Some(Ok(())) => settings.legacy_keys[slot(provider)].moved = false,
                    Some(Err(e)) => self.lines.push(Line::Warn(format!(
                        "The older {label} key could not be put back into soul_settings.json: {}. That older key is gone; enter a key again in Settings.",
                        clause(e)
                    ))),
                    None => {}
                }
            }
            Finished::Removed(_) => settings.legacy_keys[slot(provider)].moved = true,
            _ => {}
        }

        let current = self.numbers[slot(provider)];
        match reconcile(Rule::Result { of: report.number, current, failed_write }) {
            Verdict::Discard | Verdict::MoveFileKey => {}
            Verdict::FailureLineOnly => {
                if let Some(line) = failure_line(provider, &report.finished) {
                    settings.key_slot_mut(provider).failure = Some(line.clone());
                    self.lines.push(Line::Warn(line));
                }
            }
            Verdict::Apply => self.apply_current(settings, provider, report.finished),
        }
    }

    fn apply_current(&mut self, settings: &mut GlobalSoulSettings, provider: Provider, finished: Finished) {
        let label = provider.label();
        settings.key_slot_mut(provider).busy = Busy::Idle;
        if let Some(line) = failure_line(provider, &finished) {
            settings.key_slot_mut(provider).failure = Some(line.clone());
            self.lines.push(Line::Warn(line));
        }
        match finished {
            Finished::SpaceChecked { .. } | Finished::SaveRefused(_) | Finished::RemoveRefused(_) => {}
            Finished::Loaded(outcome) => {
                let store_error = matches!(outcome, LoadOutcome::StoreError(_));
                self.apply_load(settings, provider, outcome);
                if store_error {
                    // A read that did not answer in time can still answer; it
                    // reaches the screen only if nothing is submitted first.
                    self.look_up(settings, provider, After::Startup);
                }
            }
            Finished::Imported { outcome, key, .. } => {
                settings.set_session_key(provider, key);
                settings.key_slot_mut(provider).warning = match outcome {
                    SaveOutcome::Stored => None,
                    SaveOutcome::KeptInFile(why) => Some(kept_in_file(provider, &why)),
                    SaveOutcome::SessionOnly(why) | SaveOutcome::Aborted(why) => {
                        let line = format!(
                            "The {label} key in soul_settings.json could not be moved to the system credential store: {}. It is used for this session and stays in that file; moving it is tried again at the next start.",
                            clause(&why)
                        );
                        self.lines.push(Line::Warn(line.clone()));
                        Some(line)
                    }
                };
            }
            Finished::Saved { outcome, key, .. } => {
                if matches!(outcome, SaveOutcome::Stored) {
                    settings.set_session_key(provider, key);
                    let slot = settings.key_slot_mut(provider);
                    slot.warning = None;
                    slot.failure = None;
                    self.lines.push(Line::Info(format!("Saved the {label} key to the system credential store.")));
                } else {
                    // The screen follows the vault's load, as for every lookup.
                    self.look_up(settings, provider, After::Save { key });
                }
            }
            Finished::Removed(outcome) => match outcome {
                RemoveOutcome::Removed(_) => {
                    settings.set_session_key(provider, String::new());
                    if provider == Provider::Anthropic {
                        settings.api_key_valid = None;
                    }
                    let slot = settings.key_slot_mut(provider);
                    slot.warning = None;
                    slot.failure = None;
                    self.lines.push(Line::Info(format!(
                        "Removed the saved {label} key from this computer. This does not revoke it at {label}; rotate the key there to revoke it."
                    )));
                }
                RemoveOutcome::Aborted(_) | RemoveOutcome::StoreError(_) => self.look_up(settings, provider, After::Remove),
            },
            Finished::LookedUp { after, outcome } => self.apply_lookup(settings, provider, after, outcome),
        }
    }

    /// The screen takes a load's key, and its warning when it has one.
    fn apply_load(&mut self, settings: &mut GlobalSoulSettings, provider: Provider, outcome: LoadOutcome) {
        let label = provider.label();
        match outcome {
            LoadOutcome::Key { value, warning, .. } => {
                settings.set_session_key(provider, value);
                settings.key_slot_mut(provider).warning = warning;
            }
            LoadOutcome::NoKey => {
                settings.set_session_key(provider, String::new());
                settings.key_slot_mut(provider).warning = None;
            }
            LoadOutcome::FileError(why) => {
                // The fallback file may hold a newer key than the store: no
                // key this session, the file untouched, never another source.
                settings.set_session_key(provider, String::new());
                settings.key_slot_mut(provider).warning = Some(why.clone());
                self.lines.push(Line::Warn(why));
            }
            LoadOutcome::StoreError(why) => {
                settings.set_session_key(provider, String::new());
                let line = format!("The saved {label} key could not be read: {}.", clause(&why));
                settings.key_slot_mut(provider).warning = Some(line.clone());
                self.lines.push(Line::Warn(line));
            }
        }
    }

    /// A lookup after a failed write, or after a start-up read that failed:
    /// the screen takes the vault's load (its key, place and warning).
    fn apply_lookup(&mut self, settings: &mut GlobalSoulSettings, provider: Provider, after: After, outcome: LoadOutcome) {
        let label = provider.label();
        if let (After::Save { .. } | After::Remove, LoadOutcome::FileError(why) | LoadOutcome::StoreError(why)) =
            (&after, &outcome)
        {
            // The write failed and the store cannot say what it holds: the
            // screen keeps the key it had (it may still be saved), with the
            // load's text as the warning.
            settings.key_slot_mut(provider).warning = Some(format!("{label} key: {}.", clause(why)));
            return;
        }
        match after {
            After::Startup => {
                if matches!(outcome, LoadOutcome::Key { .. }) {
                    self.apply_load(settings, provider, outcome);
                }
            }
            After::Save { key } => {
                let late = matches!(&outcome, LoadOutcome::Key { value, place: Place::Store, .. } if value.trim() == key.trim());
                let none = matches!(outcome, LoadOutcome::NoKey);
                self.apply_load(settings, provider, outcome);
                if late {
                    let line = format!(
                        "The {label} key save did not finish in time, but the key was stored when the system answered."
                    );
                    settings.key_slot_mut(provider).failure = Some(line.clone());
                    self.lines.push(Line::Info(line));
                } else if none {
                    let line = format!("The {label} key was not saved. No {label} key is saved now; enter it again.");
                    settings.key_slot_mut(provider).failure = Some(line.clone());
                    self.lines.push(Line::Warn(line));
                }
            }
            After::Remove => {
                let gone = matches!(outcome, LoadOutcome::NoKey);
                self.apply_load(settings, provider, outcome);
                if gone {
                    if provider == Provider::Anthropic {
                        settings.api_key_valid = None;
                    }
                    let line = format!(
                        "The {label} key removal did not finish in time, but the key was removed when the system answered."
                    );
                    settings.key_slot_mut(provider).failure = Some(line.clone());
                    self.lines.push(Line::Info(line));
                }
            }
        }
    }
}

/// The failure line an action adds, if it failed.
fn failure_line(provider: Provider, finished: &Finished) -> Option<String> {
    let label = provider.label();
    match finished {
        Finished::Saved { outcome, .. } => match outcome {
            SaveOutcome::Stored => None,
            SaveOutcome::KeptInFile(why) => Some(kept_in_file(provider, why)),
            SaveOutcome::SessionOnly(why) => Some(format!("The {label} key is kept for this session only: {}.", clause(why))),
            SaveOutcome::Aborted(why) => Some(format!("The {label} key was not saved, and nothing changed: {}.", clause(why))),
        },
        Finished::SaveRefused(why) => Some(format!("The {label} key was not saved, and nothing changed: {}.", clause(why))),
        Finished::Removed(RemoveOutcome::Aborted(why)) | Finished::RemoveRefused(why) => Some(remove_failed(provider, why)),
        Finished::Removed(RemoveOutcome::StoreError(why)) => Some(format!(
            "{} It may come back the next time Studio loads its keys.",
            remove_failed(provider, why)
        )),
        _ => None,
    }
}

/// `why` without its closing period, to sit inside a sentence.
fn clause(why: &str) -> &str {
    why.trim().trim_end_matches('.')
}

fn kept_in_file(provider: Provider, why: &str) -> String {
    format!(
        "The {} key is kept in a private file on this computer until the system credential store works again: {}.",
        provider.label(),
        clause(why)
    )
}


fn remove_failed(provider: Provider, why: &str) -> String {
    format!(
        "Could not remove the saved {} key: {}. The key is still saved; try Remove again.",
        provider.label(),
        clause(why)
    )
}

#[cfg(test)]
mod tests {
    //! The rules that need no credential store. Synthetic keys only.
    use super::*;

    const ANTHROPIC: &str = concat!("sk-ant-", "TEST0000", "a1b2c3d4e5f6");
    const XAI: &str = concat!("xai-", "TEST0000", "q7r8s9t0u1v2");
    const OPENAI: &str = concat!("sk-proj-", "TEST0000", "w3x4y5z6a7b8");
    const LEGACY_OLD: &str = concat!("sk-ant-", "TEST0000", "legacyOLD1");
    const SESSION_KEY: &str = concat!("sk-ant-", "TEST0000", "session7777");
    const OLDER_KEY: &str = concat!("sk-ant-", "TEST0000", "older8888zz");

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eus_soul_keys_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("soul_settings.json")
    }

    fn with_keys() -> GlobalSoulSettings {
        let mut s = GlobalSoulSettings::default();
        s.global_api_key = ANTHROPIC.to_string();
        s.global_xai_api_key = XAI.to_string();
        s.global_openai_api_key = OPENAI.to_string();
        s
    }

    /// Leak 1: the settings' own JSON never carries a session key.
    #[test]
    fn t1_settings_json_holds_no_key() {
        let json = serde_json::to_string_pretty(&with_keys()).unwrap();
        for key in [ANTHROPIC, XAI, OPENAI] {
            assert!(!json.contains(key), "a key reached the settings JSON");
        }
        for field in ["global_api_key", "global_xai_api_key", "global_openai_api_key"] {
            assert!(!json.contains(field), "{field} is in the settings JSON");
        }
    }

    /// `{:?}` of the settings prints masks, never a key or a legacy key.
    #[test]
    fn debug_output_holds_no_key() {
        let mut s = with_keys();
        s.legacy_keys[0] = LegacyKey { value: LEGACY_OLD.to_string(), moved: false };
        let shown = format!("{s:?}");
        for key in [ANTHROPIC, XAI, OPENAI, LEGACY_OLD] {
            assert!(!shown.contains(key), "a key reached Debug output");
        }
        assert!(shown.contains(&mask(ANTHROPIC)), "Debug shows the mask");
    }

    /// T7: the prefix and the last 4 characters at most; dots alone for 16
    /// characters or fewer; nothing for no key.
    #[test]
    fn t7_mask_shows_prefix_and_last_four_only() {
        assert_eq!(mask(ANTHROPIC), "sk-ant-…e5f6");
        assert_eq!(mask(XAI), "xai-…u1v2");
        assert_eq!(mask(OPENAI), "sk-proj-…a7b8");
        assert_eq!(mask(concat!("sk-", "TEST0000", "abcdefghij")), "sk-…ghij");
        assert_eq!(mask("TEST0000abcdefghijkl"), "…ijkl");
        assert_eq!(mask(concat!("sk-ant-", "TEST0000", "1")), MASK_DOTS, "16 characters");
        assert_eq!(mask("short"), MASK_DOTS);
        assert_eq!(mask("   "), "");
        assert_eq!(mask(""), "");
        for key in [ANTHROPIC, XAI, OPENAI] {
            let shown = mask(key);
            let hidden: String = key.chars().skip(8).take(key.chars().count() - 12).collect();
            assert!(!shown.contains(&hidden), "the middle of a key is on screen");
        }
    }

    /// T6: an empty value, the mask on screen, or a busy text is not a key.
    #[test]
    fn t6_empty_and_mask_values_are_ignored() {
        let mut s = GlobalSoulSettings::default();
        s.global_api_key = ANTHROPIC.to_string();
        assert!(!is_new_key(&s, Provider::Anthropic, ""));
        assert!(!is_new_key(&s, Provider::Anthropic, "   "));
        assert!(!is_new_key(&s, Provider::Anthropic, &mask(ANTHROPIC)));
        assert!(!is_new_key(&s, Provider::Anthropic, &s.key_placeholder(Provider::Anthropic)));
        s.key_slots[0].busy = Busy::Checking;
        assert!(!is_new_key(&s, Provider::Anthropic, CHECKING));
        assert!(is_new_key(&s, Provider::Anthropic, concat!("sk-ant-", "TEST0000", "newkey99zz")));
        assert!(is_new_key(&s, Provider::Xai, XAI));
    }

    /// A malformed file loads the defaults and asks for the keys again; an
    /// empty one is no file.
    #[test]
    fn malformed_settings_load_defaults_with_the_notice() {
        let path = scratch("malformed");
        std::fs::write(&path, format!("{{ \"global_api_key\": \"{ANTHROPIC}\", oops")).unwrap();
        let (s, load) = GlobalSoulSettings::load_from(&path);
        assert_eq!(load.unreadable.as_deref(), Some(path.as_path()));
        assert!(s.legacy_keys.iter().all(|k| k.value.is_empty()));
        assert!(s.session_key(Provider::Anthropic).is_empty());
        std::fs::write(&path, b"  \n").unwrap();
        let (_, load) = GlobalSoulSettings::load_from(&path);
        assert!(load.unreadable.is_none(), "an empty file is not a failed import");
    }

    /// A bad non-key field loses the settings (as before) but not the keys.
    #[test]
    fn a_bad_non_key_field_does_not_block_the_keys() {
        let path = scratch("badfield");
        let text = format!(
            "{{\"global_api_key\":\"{ANTHROPIC}\",\"global_xai_api_key\":\"{XAI}\",\"global_openai_api_key\":\"{OPENAI}\",\"workshop_gauntlet\":\"not a bool\"}}"
        );
        std::fs::write(&path, text).unwrap();
        let (s, load) = GlobalSoulSettings::load_from(&path);
        assert!(load.unreadable.is_none());
        assert_eq!(s.legacy_keys[0].value, ANTHROPIC);
        assert_eq!(s.legacy_keys[1].value, XAI);
        assert_eq!(s.legacy_keys[2].value, OPENAI);
        assert!(Provider::ALL.iter().all(|p| s.session_key(*p).is_empty()), "no key before the store answers");
    }

    /// An older Anthropic spelling is read too.
    #[test]
    fn claude_api_key_is_read_as_the_anthropic_legacy_key() {
        let path = scratch("claude");
        std::fs::write(&path, format!("{{\"claude_api_key\":\"{ANTHROPIC}\"}}")).unwrap();
        let (s, _) = GlobalSoulSettings::load_from(&path);
        assert_eq!(s.legacy_keys[0].value, ANTHROPIC);
    }

    /// An unmoved key is written back as it was; a moved one, or the one a
    /// Save or Remove clears, is not; a key written since start-up survives.
    #[test]
    fn settings_file_keeps_unmoved_keys_and_drops_moved_ones() {
        let mut s = GlobalSoulSettings::default();
        s.global_api_key = SESSION_KEY.to_string();
        s.legacy_keys[0] = LegacyKey { value: ANTHROPIC.to_string(), moved: true };
        s.legacy_keys[1] = LegacyKey { value: XAI.to_string(), moved: false };
        let disk = serde_json::json!({ "global_api_key": ANTHROPIC, "global_xai_api_key": XAI });
        let out = s.settings_file_json(Some(&disk), None).unwrap();
        assert!(!out.contains(ANTHROPIC), "a moved key was written back");
        assert!(out.contains(XAI), "an unmoved key was dropped");
        assert!(!out.contains(SESSION_KEY), "a session key reached the file");
        // A Save of OpenAI keeps xAI's unmoved key.
        let out = s.settings_file_json(Some(&disk), Some(Provider::OpenAi)).unwrap();
        assert!(out.contains(XAI));
        // Clearing xAI drops it.
        let out = s.settings_file_json(Some(&disk), Some(Provider::Xai)).unwrap();
        assert!(!out.contains(XAI));
        // A different Anthropic key an older build wrote since start-up stays.
        let newer = serde_json::json!({ "global_api_key": OLDER_KEY });
        let out = s.settings_file_json(Some(&newer), None).unwrap();
        assert!(out.contains(OLDER_KEY));
    }

    /// save_to writes the settings and no key field of this build's, and
    /// reads back whole.
    #[test]
    fn a_saved_file_has_no_key_fields_and_reads_back() {
        let path = scratch("roundtrip");
        let mut s = with_keys();
        s.workshop_gauntlet = true;
        s.save_to(&path, None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for field in ["global_api_key", "global_xai_api_key", "global_openai_api_key", "claude_api_key"] {
            assert!(!text.contains(field), "{field} written");
        }
        let (back, load) = GlobalSoulSettings::load_from(&path);
        assert!(load.unreadable.is_none());
        assert!(back.workshop_gauntlet, "a file without key fields must read back whole");
        assert!(!path.with_extension("json.tmp").exists(), "temporary file left behind");
    }

    /// The screen follows the last action; a failed write still adds its line.
    #[test]
    fn reconcile_follows_the_last_action() {
        assert_eq!(reconcile(Rule::LegacyKeyFound), Verdict::MoveFileKey);
        assert_eq!(reconcile(Rule::Result { of: 3, current: 3, failed_write: false }), Verdict::Apply);
        assert_eq!(reconcile(Rule::Result { of: 3, current: 3, failed_write: true }), Verdict::Apply);
        assert_eq!(reconcile(Rule::Result { of: 2, current: 3, failed_write: false }), Verdict::Discard);
        assert_eq!(reconcile(Rule::Result { of: 2, current: 3, failed_write: true }), Verdict::FailureLineOnly);
    }

    /// An old SoulService record that still holds the removed per-Space
    /// fields loads, and the values are dropped.
    #[test]
    fn an_old_soul_service_record_loads_without_its_key_fields() {
        let old = format!("{{\"claude_api_key\":\"{ANTHROPIC}\",\"use_global_key\":false,\"telemetry_enabled\":true}}");
        let loaded: super::super::SoulServiceSettings = serde_json::from_str(&old).unwrap();
        assert!(loaded.telemetry_enabled);
        let back = serde_json::to_string(&loaded).unwrap();
        assert!(!back.contains(ANTHROPIC), "the old key came back");
        assert!(!back.contains("claude_api_key") && !back.contains("use_global_key"), "{back}");
    }

    /// The three names come from the one public list, in provider order.
    #[test]
    fn the_three_names_come_from_one_list() {
        assert_eq!(SOUL_SECRET_NAMES.len(), 3);
        for (provider, expected) in [
            (Provider::Anthropic, "eustress/anthropic/api_key"),
            (Provider::Xai, "eustress/xai/api_key"),
            (Provider::OpenAi, "eustress/openai/api_key"),
        ] {
            assert_eq!(credential_name(provider).unwrap().as_str(), expected);
            assert_eq!(store_id(provider), SOUL_SECRET_NAMES[slot(provider)].0);
        }
    }

    /// Workshop waits only while a key is still being looked up.
    #[test]
    fn workshop_waits_while_checking() {
        let mut s = GlobalSoulSettings::default();
        s.key_slots[0].busy = Busy::Checking;
        assert!(s.key_pending(Provider::Anthropic));
        assert_eq!(s.key_placeholder(Provider::Anthropic), CHECKING);
        assert!(s.key_wait_text(Provider::Anthropic).is_some());
        s.key_slots[0].busy = Busy::Idle;
        assert!(!s.key_pending(Provider::Anthropic), "no key and not checking: missing, not pending");
        s.key_slots[0].busy = Busy::Checking;
        s.global_api_key = ANTHROPIC.to_string();
        assert!(!s.key_pending(Provider::Anthropic), "a key in hand is used");
    }
}
