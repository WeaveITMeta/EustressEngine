//! Where an API key lives: outside every Space, in the operating system's
//! credential store, and out of the UI's way.
//!
//! The constraint that shapes this: a Space folder is the thing we ask auditors
//! to read, and it gets synced, committed, and shared. A key typed into
//! Properties would land in `_instance.toml` and from there into a git history.
//! So the key never goes in a Space. The Connector stores only the NAME of the
//! credential (`secret_ref`), and the value is kept by
//! [`crate::key_fallback::KeyFallback`]: in the system credential store
//! (Windows Credential Manager, the macOS Keychain, the Linux Secret Service),
//! or, while that cannot take it, in a private fallback file. One key per name
//! for the machine; there is no per-Space slot.
//!
//! `%LOCALAPPDATA%\Eustress\credentials.toml` (the platform data-local
//! directory elsewhere) now holds only when each key was saved, which is not
//! secret. Earlier builds kept the keys themselves there in plain text: the
//! first read of a name moves its key into the store (the newest-key order in
//! `key_fallback`) and then takes it out of this file, rewriting the file
//! without it, or deleting the file when nothing else is left in it.
//!
//! Read at fetch time, so saving a key takes effect on the next sync with no
//! restart. Every function has a `_with` form over an explicit key store and
//! file, so the tests never touch the real store or profile. These calls wait
//! on the credential queue (at most 5 s on Windows; 30 s on macOS and Linux).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eustress_common::credentials::{CredentialError, CredentialName, TIMEOUT};
use serde::{Deserialize, Serialize};

use super::CaptureError;
use crate::key_fallback::{KeyFallback, KeyGuard, LoadOutcome, Place, RemoveOutcome, SaveOutcome};

/// File name inside the Eustress data directory.
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Default credential name for SAM.gov, when a Connector names none.
pub const DEFAULT_SAM_KEY: &str = "SAM_API_KEY";

/// Whether `name` may be used as a SAM.gov credential name.
///
/// The name comes from a Connector's `secret_ref`, and a Connector is a file
/// any MCP agent can write. Without this, an agent-written Connector could name
/// `ANTHROPIC_API_KEY` or `GITHUB_TOKEN`, and the environment fallback in
/// [`get`] would hand that value to the fetch. Only `SAM_API_KEY` and
/// `SAM_API_KEY_<SUFFIX>` (uppercase letters, digits, underscore) qualify,
/// which still lets one machine keep a key per client.
pub fn is_sam_credential_name(name: &str) -> bool {
    if name == DEFAULT_SAM_KEY {
        return true;
    }
    match name.strip_prefix("SAM_API_KEY_") {
        Some(rest) => {
            !rest.is_empty()
                && rest.len() <= 48
                && rest.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        }
        None => false,
    }
}

/// The store's name for a credential: `eustress/sam/api_key[_<suffix>]` for a
/// SAM key, `eustress/forge/url` and `eustress/forge/api_key` for Forge, and
/// `eustress/capture/<name in lower case>` for any other plain name.
fn store_name(name: &str) -> Result<CredentialName, CaptureError> {
    let bad = |e: CredentialError| CaptureError::BadQuery(format!("credential {name:?}: {e}"));
    if is_sam_credential_name(name) {
        let purpose = name["SAM_".len()..].to_ascii_lowercase();
        return CredentialName::new("sam", &purpose).map_err(bad);
    }
    match name {
        "EUSTRESS_FORGE_URL" => CredentialName::new("forge", "url").map_err(bad),
        "EUSTRESS_FORGE_API_KEY" => CredentialName::new("forge", "api_key").map_err(bad),
        _ => CredentialName::new("capture", &name.to_ascii_lowercase()).map_err(bad),
    }
}

/// The store names capture keeps a secret under: the Forge key and the
/// default SAM.gov key. (The remembered Forge URL is not a secret; a SAM key a
/// Connector names with a suffix is named by that Connector.) For the check
/// that every secret is in `eustress_common::credentials::SECRET_NAMES`.
pub fn capture_secret_names() -> Vec<CredentialName> {
    [DEFAULT_SAM_KEY, "EUSTRESS_FORGE_API_KEY"].into_iter().filter_map(|name| store_name(name).ok()).collect()
}

/// `credentials.toml`: when each key was saved, and, from earlier builds,
/// plain key values waiting to be moved.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Legacy {
    /// Plain key values an earlier build saved. Moved out on first read.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    keys: BTreeMap<String, String>,
    /// When each key was saved, so the dialog can tell one key from the next
    /// without showing any part of either.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    saved_at: BTreeMap<String, String>,
}

/// The names of the plain keys, never their values.
impl std::fmt::Debug for Legacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Legacy")
            .field("keys", &self.keys.keys().collect::<Vec<_>>())
            .field("saved_at", &self.saved_at)
            .finish()
    }
}

/// Whether a credential is set, and where it came from. Never any part of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStatus {
    pub name: String,
    pub present: bool,
    /// When the saved key was saved, e.g. `2026-10-01 15:40 UTC`. `None` for a
    /// key from the environment, or one saved before times were recorded.
    pub saved_at: Option<String>,
    /// True when the value came from the environment rather than a save, so
    /// the UI can explain why a Remove button will not clear it.
    pub from_environment: bool,
    /// True when the saved key is not in the system credential store (it is
    /// in the private fallback file, or kept for this session), because the
    /// store could not take it.
    pub outside_store: bool,
    /// True when the saved key is kept only in memory: neither the store nor
    /// the private file could take it, so it is gone when Studio closes.
    pub session_only: bool,
    /// Why the key could not be read, when it could not: its fallback file is
    /// damaged, or the store could not be asked. Never any part of a key.
    pub problem: Option<String>,
}

impl KeyStatus {
    /// One line for the dialog and the ribbon report.
    pub fn summary(&self) -> String {
        if let Some(problem) = &self.problem {
            return format!("{} could not be read: {problem}", self.name);
        }
        let base = match (self.present, self.from_environment, &self.saved_at) {
            (false, _, _) => return format!("{} is not set", self.name),
            (true, true, _) => return format!("{} is set from the environment", self.name),
            (true, false, Some(when)) => format!("{} is saved ({when})", self.name),
            (true, false, None) => format!("{} is saved", self.name),
        };
        if self.session_only {
            format!(
                "{base}, but only until Studio closes: neither the system credential store nor a private \
                 file could keep it, so save it again after a restart"
            )
        } else if self.outside_store {
            format!(
                "{base}, but not in the system credential store, which was unavailable: it is in a private \
                 file and not encrypted, and moves into the store once the store is available"
            )
        } else {
            base
        }
    }
}

/// `%LOCALAPPDATA%\Eustress` on Windows, the platform equivalent elsewhere.
pub fn credentials_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Eustress"))
}

/// Full path of the credential file.
pub fn credentials_path() -> Option<PathBuf> {
    credentials_dir().map(|d| d.join(CREDENTIALS_FILE))
}

fn no_data_dir() -> CaptureError {
    CaptureError::Io("no local data directory on this platform".into())
}

/// Read one credential.
///
/// A saved key wins over the environment. A user who just pasted a key into
/// the dialog expects that key to be used, and silently preferring a stale
/// environment variable they set months ago would be the worst kind of
/// surprise: it looks saved, and the wrong one goes out.
pub fn get(name: &str) -> Option<String> {
    let saved = match (KeyFallback::standard(), credentials_path()) {
        (Some(kf), Some(legacy)) => get_with(&kf, &legacy, name),
        _ => None,
    };
    // The environment is consulted only for SAM credential names. Reading an
    // arbitrary variable by a name an agent chose is the exfiltration
    // primitive; see `is_sam_credential_name`.
    saved.or_else(|| env_value(name))
}

fn env_value(name: &str) -> Option<String> {
    if !is_sam_credential_name(name) {
        return None;
    }
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The saved key for `name`, after moving any plain copy an earlier build left
/// in `legacy`. A warning or error is logged; no key is.
pub(crate) fn get_with(kf: &KeyFallback, legacy: &Path, name: &str) -> Option<String> {
    let key = store_name(name).ok()?;
    match load_saved(kf, legacy, name, &key) {
        Ok(value) => value,
        Err(e) => {
            tracing::warn!("capture: {name} could not be read: {e}");
            None
        }
    }
}

/// Why [`read_saved`] has no answer. Never any part of a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// A key is saved, in the private fallback file, and that file could not
    /// be read: it is damaged, or not readable.
    Unreadable(String),
    /// The system credential store could not be asked, or did not answer in
    /// time (on macOS or Linux, maybe a prompt nobody answered). A key may be
    /// saved there, or none may be.
    NotAsked(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Unreadable(why) | ReadError::NotAsked(why) => f.write_str(why),
        }
    }
}

/// The saved key for `name`, telling "none is saved" (`Ok(None)`) from "one
/// is saved but could not be read" and "the store could not be asked" (see
/// [`ReadError`]). Saved keys only; the environment is not read. For a caller
/// that must not treat an unread key as no key, such as the Forge dialog,
/// which would otherwise forget it.
pub fn read_saved(name: &str) -> Result<Option<String>, ReadError> {
    let not_asked = || ReadError::NotAsked(no_data_dir().to_string());
    let kf = KeyFallback::standard().ok_or_else(not_asked)?;
    let legacy = credentials_path().ok_or_else(not_asked)?;
    read_saved_with(&kf, &legacy, name)
}

/// [`read_saved`] over an explicit key store and file. Nothing can be saved
/// under a name that is not a credential name, so it reads as none.
pub(crate) fn read_saved_with(kf: &KeyFallback, legacy: &Path, name: &str) -> Result<Option<String>, ReadError> {
    let Ok(key) = store_name(name) else { return Ok(None) };
    load_saved(kf, legacy, name, &key)
}

fn load_saved(kf: &KeyFallback, legacy: &Path, name: &str, key: &CredentialName) -> Result<Option<String>, ReadError> {
    let guard = kf.lock(key);
    import_legacy(kf, &guard, legacy, name);
    match kf.load(&guard) {
        LoadOutcome::Key { value, warning, .. } => {
            if let Some(w) = warning {
                tracing::warn!("capture: {name}: {w}");
            }
            Ok(Some(value))
        }
        LoadOutcome::NoKey => Ok(None),
        LoadOutcome::FileError(e) => Err(ReadError::Unreadable(e)),
        LoadOutcome::StoreError(e) => Err(ReadError::NotAsked(e)),
    }
}

/// Move a plain key an earlier build saved in `legacy` into the key store, and
/// only once it is kept there (in the store or its private fallback) take it
/// out of `legacy`. Best effort: a key it cannot move stays for the next read.
fn import_legacy(kf: &KeyFallback, guard: &KeyGuard, legacy: &Path, name: &str) {
    let plain = read_legacy(legacy).keys.get(name).map(|v| v.trim().to_string());
    let Some(value) = plain.filter(|v| !v.is_empty()) else {
        return;
    };
    // The file is not held while the store is asked (up to `TIMEOUT`), and
    // is read again for the removal: other names may have changed it since,
    // and an earlier build running beside this one may have saved a newer
    // plain key under this name, which stays for the next read.
    match kf.save_within(guard, &value, TIMEOUT) {
        SaveOutcome::Stored | SaveOutcome::KeptInFile(_) => {
            let moved = |file: &mut Legacy| {
                let still = file.keys.get(name).is_some_and(|v| v.trim() == value);
                still && file.keys.remove(name).is_some()
            };
            if let Err(e) = update_legacy(legacy, moved) {
                tracing::warn!("capture: {name} moved, but its plain copy in {} stays: {e}", legacy.display());
            }
        }
        SaveOutcome::SessionOnly(why) | SaveOutcome::Aborted(why) => {
            tracing::warn!("capture: {name} could not be moved out of {}: {why}", legacy.display());
        }
    }
}

/// Save one credential.
pub fn set(name: &str, value: &str) -> Result<(), CaptureError> {
    let kf = KeyFallback::standard().ok_or_else(no_data_dir)?;
    let legacy = credentials_path().ok_or_else(no_data_dir)?;
    set_with(&kf, &legacy, name, value)
}

/// [`set`] over an explicit key store and file. A plain copy an earlier build
/// left is taken out first (it is older, and would otherwise be moved over
/// this key on the next read); then the key is saved by `key_fallback`.
pub(crate) fn set_with(kf: &KeyFallback, legacy: &Path, name: &str, value: &str) -> Result<(), CaptureError> {
    let value = value.trim();
    let name = name.trim();
    if value.is_empty() {
        return Err(CaptureError::BadQuery("an API key cannot be empty".into()));
    }
    if name.is_empty() {
        return Err(CaptureError::BadQuery("a credential needs a name".into()));
    }
    let key = store_name(name)?;
    let guard = kf.lock(&key);
    update_legacy(legacy, |file| file.keys.remove(name).is_some()).map_err(|e| {
        CaptureError::Io(format!("an old copy in {} could not be cleared, so nothing was saved: {e}", legacy.display()))
    })?;
    match kf.save(&guard, value) {
        SaveOutcome::Stored => {}
        SaveOutcome::KeptInFile(why) | SaveOutcome::SessionOnly(why) => {
            tracing::warn!("capture: {name} is not in the system credential store: {why}");
        }
        SaveOutcome::Aborted(why) => return Err(CaptureError::Io(why)),
    }
    // Read again: the store write can take 30 s, and other names may have
    // changed the file meanwhile.
    let when = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    if let Err(e) = update_legacy(legacy, |file| {
        file.saved_at.insert(name.to_string(), when);
        true
    }) {
        tracing::warn!("capture: {name} is saved, but when it was saved could not be recorded: {e}");
    }
    Ok(())
}

/// Forget one credential. Returns whether there was one to forget.
pub fn remove(name: &str) -> Result<bool, CaptureError> {
    let kf = KeyFallback::standard().ok_or_else(no_data_dir)?;
    let legacy = credentials_path().ok_or_else(no_data_dir)?;
    remove_with(&kf, &legacy, name)
}

/// [`remove`] over an explicit key store and file: the plain copy, the
/// fallback file and the store all forget it.
pub(crate) fn remove_with(kf: &KeyFallback, legacy: &Path, name: &str) -> Result<bool, CaptureError> {
    let key = store_name(name)?;
    let guard = kf.lock(&key);
    let mut had_plain = false;
    update_legacy(legacy, |file| {
        had_plain = file.keys.remove(name).is_some();
        let had_time = file.saved_at.remove(name).is_some();
        had_plain || had_time
    })
    .map_err(|e| CaptureError::Io(format!("{}: {e}", legacy.display())))?;
    match kf.remove(&guard) {
        RemoveOutcome::Removed(had) => Ok(had || had_plain),
        RemoveOutcome::Aborted(why) | RemoveOutcome::StoreError(why) => Err(CaptureError::Io(why)),
    }
}

/// Report a credential's state without revealing any part of it.
pub fn status(name: &str) -> KeyStatus {
    match (KeyFallback::standard(), credentials_path()) {
        (Some(kf), Some(legacy)) => status_with(&kf, &legacy, name),
        _ => status_from_env(name),
    }
}

/// [`status`] over an explicit key store and file, then the environment.
pub(crate) fn status_with(kf: &KeyFallback, legacy: &Path, name: &str) -> KeyStatus {
    let Ok(key) = store_name(name) else { return status_from_env(name) };
    let guard = kf.lock(&key);
    import_legacy(kf, &guard, legacy, name);
    let saved_at = read_legacy(legacy).saved_at.get(name).cloned();
    match kf.load(&guard) {
        LoadOutcome::Key { place, .. } => KeyStatus {
            name: name.to_string(),
            present: true,
            saved_at,
            from_environment: false,
            outside_store: place != Place::Store,
            session_only: place == Place::Session,
            problem: None,
        },
        LoadOutcome::NoKey => status_from_env(name),
        LoadOutcome::FileError(e) | LoadOutcome::StoreError(e) => KeyStatus {
            name: name.to_string(),
            present: false,
            saved_at,
            from_environment: false,
            outside_store: false,
            session_only: false,
            problem: Some(e),
        },
    }
}

/// Same rule as `get`: never read an environment variable that is not a SAM
/// credential, not even to report that it exists.
fn status_from_env(name: &str) -> KeyStatus {
    let present = env_value(name).is_some();
    KeyStatus {
        name: name.to_string(),
        present,
        saved_at: None,
        from_environment: present,
        outside_store: false,
        session_only: false,
        problem: None,
    }
}

/// One change to `credentials.toml` at a time in this process. The file is
/// shared by every name, and a key's own guard covers only that name: two
/// names changing it at once would each write back a copy without the
/// other's change, bringing back a plain key already moved, or losing one.
fn legacy_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `credentials.toml` as it is now.
fn read_legacy(path: &Path) -> Legacy {
    let _held = legacy_lock();
    load_legacy(path)
}

/// Read `credentials.toml` fresh, let `change` edit it, and write it back
/// when `change` says it changed something; all under [`legacy_lock`].
/// Never held across a store call.
fn update_legacy(path: &Path, change: impl FnOnce(&mut Legacy) -> bool) -> Result<(), String> {
    let _held = legacy_lock();
    let mut file = load_legacy(path);
    if change(&mut file) {
        save_legacy(path, &file)
    } else {
        Ok(())
    }
}

fn load_legacy(path: &Path) -> Legacy {
    // A missing or unreadable file is empty: it only holds when keys were
    // saved and old plain copies, and neither may block a save.
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Legacy::default(),
    }
}

/// Deleted when nothing is left in it. Its folder (`%LOCALAPPDATA%\Eustress`,
/// shared with Modes, Plugins and the Player) is never made private by a
/// write here. The file itself is written owner-only on macOS and Linux
/// ([`write_owner_only`]): until every name has been read once, it still holds
/// earlier builds' plain keys. Call under [`legacy_lock`].
fn save_legacy(path: &Path, file: &Legacy) -> Result<(), String> {
    sweep_stale_tmps(path);
    if file.keys.is_empty() && file.saved_at.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    let text = toml::to_string_pretty(file).map_err(|e| format!("serializing credentials: {e}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = legacy_tmp(path);
    let written = write_owner_only(&tmp, text.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map_err(|e| e.to_string())
}

/// How old a temporary file beside `credentials.toml` must be before it is
/// taken for one a stopped write left, not another Studio's write in progress.
const STALE_TMP: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Delete the temporary files stopped writes left beside `credentials.toml`
/// (this build's `credentials.toml.<process id>.tmp`, an earlier build's
/// `credentials.toml.tmp`) once older than [`STALE_TMP`]: they may hold
/// earlier builds' plain keys. Best effort. Call under [`legacy_lock`].
fn sweep_stale_tmps(path: &Path) {
    let (Some(dir), Some(base)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let prefix = format!("{base}.");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(&prefix) || !name.ends_with(".tmp") {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age >= STALE_TMP);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// `credentials.toml.<process id>.tmp`: another process (a second Studio)
/// never writes the same temporary file, and in this one [`legacy_lock`]
/// keeps writes one at a time.
fn legacy_tmp(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// Create `path` new and write `bytes` to it: on macOS and Linux readable by
/// its owner only (0600), as earlier builds kept `credentials.toml`. On
/// Windows it takes its folder's access, which in `%LOCALAPPDATA%` is the
/// person's own. A file left by a process that stopped mid-write is replaced.
fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let open = || {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)
    };
    let mut out = match open() {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(path)?;
            open()?
        }
        other => other?,
    };
    out.write_all(bytes)?;
    out.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_fallback::KeyStore;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default, Clone)]
    struct TestKeys {
        map: Arc<Mutex<HashMap<String, String>>>,
        down: Arc<AtomicBool>,
        /// A store write for this name waits until it is set back to `None`.
        held: Arc<Mutex<Option<String>>>,
        /// A held write has started waiting.
        waiting: Arc<AtomicBool>,
    }

    impl TestKeys {
        fn holds(&self, service: &str) -> Option<String> {
            self.map.lock().unwrap().get(service).cloned()
        }
        fn is_held(&self, name: &CredentialName) -> bool {
            self.held.lock().unwrap().as_deref() == Some(name.as_str())
        }
        /// Wait, at most 10 s so a broken test cannot hang, while `name` is held.
        fn wait_while_held(&self, name: &CredentialName) {
            if !self.is_held(name) {
                return;
            }
            self.waiting.store(true, Ordering::SeqCst);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while self.is_held(name) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fn check(&self) -> Result<(), CredentialError> {
            if self.down.load(Ordering::SeqCst) {
                return Err(CredentialError::Unavailable("the test store is down".into()));
            }
            Ok(())
        }
    }

    impl KeyStore for TestKeys {
        fn set_verified(&self, name: &CredentialName, key: &str, _limit: Option<Duration>) -> Result<(), CredentialError> {
            self.check()?;
            self.wait_while_held(name);
            self.map.lock().unwrap().insert(name.as_str().to_string(), key.to_string());
            Ok(())
        }
        fn get(&self, name: &CredentialName) -> Result<Option<String>, CredentialError> {
            self.check()?;
            Ok(self.holds(name.as_str()))
        }
        fn delete(&self, name: &CredentialName) -> Result<bool, CredentialError> {
            self.check()?;
            Ok(self.map.lock().unwrap().remove(name.as_str()).is_some())
        }
    }

    struct Setup {
        dir: PathBuf,
        keys: TestKeys,
        kf: KeyFallback,
        legacy: PathBuf,
    }

    fn setup(tag: &str) -> Setup {
        let dir = std::env::temp_dir().join(format!("eustress-creds-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let keys = TestKeys::default();
        let kf = KeyFallback::with(dir.join("keys"), Box::new(keys.clone()));
        Setup { legacy: dir.join(CREDENTIALS_FILE), dir, keys, kf }
    }

    fn text(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn a_saved_key_goes_into_the_store_and_the_file_holds_only_when() {
        let s = setup("store");
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert_eq!(s.keys.holds("eustress/sam/api_key").as_deref(), Some("abc123def456"));
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).as_deref(), Some("abc123def456"));
        assert!(!text(&s.legacy).contains("abc123def456"), "{}", text(&s.legacy));
        assert!(text(&s.legacy).contains("saved_at"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn names_map_to_one_global_store_entry_each() {
        let s = setup("names");
        set_with(&s.kf, &s.legacy, "SAM_API_KEY", "first-key-value").unwrap();
        set_with(&s.kf, &s.legacy, "SAM_API_KEY_CLIENT_B", "second-key-value").unwrap();
        set_with(&s.kf, &s.legacy, "EUSTRESS_FORGE_URL", "https://forge.example").unwrap();
        set_with(&s.kf, &s.legacy, "EUSTRESS_FORGE_API_KEY", "forge-key-value").unwrap();
        assert_eq!(s.keys.holds("eustress/sam/api_key").as_deref(), Some("first-key-value"));
        assert_eq!(s.keys.holds("eustress/sam/api_key_client_b").as_deref(), Some("second-key-value"));
        assert_eq!(s.keys.holds("eustress/forge/url").as_deref(), Some("https://forge.example"));
        assert_eq!(s.keys.holds("eustress/forge/api_key").as_deref(), Some("forge-key-value"));
        assert!(set_with(&s.kf, &s.legacy, "NOT A NAME", "v").is_err());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// An earlier build's plain copy moves into the store on first read and
    /// leaves the file; when it was saved stays.
    #[test]
    fn a_plain_key_from_an_earlier_build_moves_out_of_the_file() {
        let s = setup("legacy");
        std::fs::write(
            &s.legacy,
            "[keys]\nSAM_API_KEY = \"legacy-key-value\"\n\n[saved_at]\nSAM_API_KEY = \"2026-10-01 15:40 UTC\"\n",
        )
        .unwrap();
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).as_deref(), Some("legacy-key-value"));
        assert_eq!(s.keys.holds("eustress/sam/api_key").as_deref(), Some("legacy-key-value"));
        assert!(!text(&s.legacy).contains("legacy-key-value"), "{}", text(&s.legacy));
        assert!(text(&s.legacy).contains("2026-10-01 15:40 UTC"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn a_legacy_file_that_held_only_secrets_is_deleted() {
        let s = setup("legacy_only");
        std::fs::write(&s.legacy, "[keys]\nEUSTRESS_FORGE_API_KEY = \"forge-key-value\"\n").unwrap();
        assert_eq!(get_with(&s.kf, &s.legacy, "EUSTRESS_FORGE_API_KEY").as_deref(), Some("forge-key-value"));
        assert!(!s.legacy.exists(), "nothing else was in it");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// A newer save takes an older plain copy out first, so it can never be
    /// moved over the newer key later.
    #[test]
    fn a_new_save_takes_the_old_plain_copy_out_first() {
        let s = setup("legacy_then_save");
        std::fs::write(&s.legacy, "[keys]\nSAM_API_KEY = \"old-plain-key\"\n").unwrap();
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "new-key-value").unwrap();
        assert!(!text(&s.legacy).contains("old-plain-key"));
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).as_deref(), Some("new-key-value"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn with_the_store_down_the_status_says_the_key_is_outside_it() {
        let s = setup("down");
        s.keys.down.store(true, Ordering::SeqCst);
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).as_deref(), Some("abc123def456"));
        let st = status_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        assert!(st.present && st.outside_store, "{st:?}");
        assert!(st.summary().contains("not encrypted"), "{}", st.summary());
        assert!(!st.summary().contains("abc1"), "{}", st.summary());
        s.keys.down.store(false, Ordering::SeqCst);
        assert!(!status_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).outside_store, "moved in once it is back");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// With neither the store nor the fallback folder able to keep it, the
    /// key works for this session and its status says it ends with Studio,
    /// not that it is in a file.
    #[test]
    fn a_session_only_key_says_it_lasts_until_studio_closes() {
        let s = setup("session_only");
        s.keys.down.store(true, Ordering::SeqCst);
        // A file where the keys folder should be: no fallback file can be made.
        std::fs::write(s.dir.join("keys"), b"not a folder").unwrap();
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        let st = status_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        assert!(st.present && st.outside_store && st.session_only, "{st:?}");
        let line = st.summary();
        assert!(line.contains("until Studio closes"), "{line}");
        assert!(!line.contains("private file and not encrypted"), "it is in no file: {line}");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn the_capture_secret_names_are_the_sam_and_forge_keys() {
        let names: Vec<String> = capture_secret_names().iter().map(|n| n.as_str().to_string()).collect();
        assert_eq!(names, ["eustress/sam/api_key", "eustress/forge/api_key"]);
    }

    #[test]
    fn a_damaged_fallback_file_is_reported_not_hidden() {
        let s = setup("damaged");
        s.keys.down.store(true, Ordering::SeqCst);
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        std::fs::write(s.dir.join("keys").join("sam.api_key.key"), [0xffu8]).unwrap();
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY), None);
        let st = status_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        assert!(!st.present && st.problem.is_some(), "{st:?}");
        assert!(st.summary().contains("could not be read"), "{}", st.summary());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn removing_forgets_it_everywhere() {
        let s = setup("remove");
        assert!(!remove_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).unwrap(), "nothing saved yet");
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert!(remove_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).unwrap());
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY), None);
        assert_eq!(s.keys.holds("eustress/sam/api_key"), None);
        assert_eq!(status_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).saved_at, None);
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// A save waits on the store (up to 30 s on macOS and Linux) while another
    /// name's plain copy moves out of the shared file. The save's last write
    /// reads the file again, so the moved key does not come back.
    #[test]
    fn a_slow_save_does_not_bring_back_a_plain_key_moved_meanwhile() {
        let s = setup("concurrent");
        std::fs::write(&s.legacy, "[keys]\nSAM_API_KEY_B = \"b-plain-key\"\n").unwrap();
        *s.keys.held.lock().unwrap() = Some("eustress/sam/api_key".into());
        std::thread::scope(|scope| {
            let saving = scope.spawn(|| set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "a-new-key"));
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !s.keys.waiting.load(Ordering::SeqCst) {
                assert!(std::time::Instant::now() < deadline, "the save never reached the store");
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(get_with(&s.kf, &s.legacy, "SAM_API_KEY_B").as_deref(), Some("b-plain-key"));
            assert!(!text(&s.legacy).contains("b-plain-key"), "moved out: {}", text(&s.legacy));
            *s.keys.held.lock().unwrap() = None;
            saving.join().unwrap().unwrap();
        });
        let left = text(&s.legacy);
        assert!(!left.contains("b-plain-key"), "the save brought the moved key back: {left}");
        assert!(left.contains("SAM_API_KEY = "), "and recorded when it was saved: {left}");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// Many names moving out of the shared file at once: every plain key
    /// leaves it and is in the store.
    #[test]
    fn names_moving_out_at_once_all_leave_the_file() {
        let s = setup("parallel");
        let suffixes: Vec<String> = (0..12).map(|i| format!("N{i}")).collect();
        let mut plain = String::from("[keys]\n");
        for x in &suffixes {
            plain += &format!("SAM_API_KEY_{x} = \"plain-{x}\"\n");
        }
        std::fs::write(&s.legacy, plain).unwrap();
        std::thread::scope(|scope| {
            for x in &suffixes {
                let s = &s;
                scope.spawn(move || get_with(&s.kf, &s.legacy, &format!("SAM_API_KEY_{x}")));
            }
        });
        let left = text(&s.legacy);
        assert!(!left.contains("plain-"), "a plain key stayed or came back: {left}");
        for x in &suffixes {
            let stored = s.keys.holds(&format!("eustress/sam/api_key_{}", x.to_ascii_lowercase()));
            assert_eq!(stored, Some(format!("plain-{x}")), "{x}");
        }
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// One change to the shared file at a time: a second change waits until
    /// the first is written, rather than reading the file mid-change and
    /// writing back over it.
    #[test]
    fn changes_to_the_old_file_take_turns() {
        let s = setup("turns");
        let (first_inside, second_ran, second_ran_inside_first) =
            (AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false));
        std::thread::scope(|scope| {
            scope.spawn(|| {
                update_legacy(&s.legacy, |file| {
                    first_inside.store(true, Ordering::SeqCst);
                    // Room for the second change to run, which it may do
                    // only after this one is written.
                    let until = std::time::Instant::now() + Duration::from_millis(300);
                    while std::time::Instant::now() < until && !second_ran.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    second_ran_inside_first.store(second_ran.load(Ordering::SeqCst), Ordering::SeqCst);
                    file.saved_at.insert("FIRST".into(), "1".into());
                    true
                })
                .unwrap();
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !first_inside.load(Ordering::SeqCst) {
                assert!(std::time::Instant::now() < deadline, "the first change never started");
                std::thread::sleep(Duration::from_millis(1));
            }
            update_legacy(&s.legacy, |file| {
                second_ran.store(true, Ordering::SeqCst);
                file.saved_at.insert("SECOND".into(), "2".into());
                true
            })
            .unwrap();
        });
        assert!(!second_ran_inside_first.load(Ordering::SeqCst), "the second change ran inside the first");
        let left = text(&s.legacy);
        assert!(left.contains("FIRST = ") && left.contains("SECOND = "), "a change was lost: {left}");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// Until every name is read once, the file still holds earlier builds'
    /// plain keys: a rewrite leaves it readable by its owner only.
    #[cfg(unix)]
    #[test]
    fn the_rewritten_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let s = setup("mode");
        std::fs::write(&s.legacy, "[keys]\nSAM_API_KEY = \"a-plain-key\"\nSAM_API_KEY_B = \"b-plain-key\"\n").unwrap();
        get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        assert!(text(&s.legacy).contains("b-plain-key"), "B is not read yet, so it is still there");
        let mode = std::fs::metadata(&s.legacy).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "credentials.toml is {mode:o}");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// A temporary file a stopped process left under this process id is
    /// replaced, not a reason to fail.
    #[test]
    fn a_left_over_temporary_file_does_not_block_a_save() {
        let s = setup("leftover");
        std::fs::write(legacy_tmp(&s.legacy), "half writ").unwrap();
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert!(text(&s.legacy).contains("saved_at"), "{}", text(&s.legacy));
        assert!(!legacy_tmp(&s.legacy).exists());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// Temporary files a stopped write left (they may hold earlier builds'
    /// plain keys) are deleted by the next change once they are old; a young
    /// one may be another Studio's write in progress, and stays.
    #[test]
    fn stale_temporary_files_are_swept_and_young_ones_kept() {
        let s = setup("sweep");
        let stale_tmps = [s.dir.join("credentials.toml.tmp"), s.dir.join("credentials.toml.999999.tmp")];
        let young = s.dir.join("credentials.toml.888888.tmp");
        let other = s.dir.join("other.toml.tmp");
        for p in stale_tmps.iter().chain([&young, &other]) {
            std::fs::write(p, "[keys]\nSAM_API_KEY = \"plain-key\"\n").unwrap();
        }
        let an_hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
        for p in stale_tmps.iter().chain([&other]) {
            std::fs::File::options().write(true).open(p).unwrap().set_modified(an_hour_ago).unwrap();
        }
        set_with(&s.kf, &s.legacy, "SAM_API_KEY_B", "b-key-value").unwrap();
        for p in &stale_tmps {
            assert!(!p.exists(), "{} was left", p.display());
        }
        assert!(young.exists(), "a write in progress was deleted");
        assert!(other.exists(), "a file that is not ours was deleted");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// While a plain key is moved into the store, an earlier build running
    /// beside this one saves a newer plain key under the same name: the move
    /// does not take the newer one out of the file (it moves on the next read).
    #[test]
    fn a_newer_plain_key_saved_during_the_move_stays() {
        let s = setup("newer_plain");
        std::fs::write(&s.legacy, "[keys]\nSAM_API_KEY = \"older-plain\"\n").unwrap();
        *s.keys.held.lock().unwrap() = Some("eustress/sam/api_key".into());
        std::thread::scope(|scope| {
            let reading = scope.spawn(|| get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY));
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !s.keys.waiting.load(Ordering::SeqCst) {
                assert!(std::time::Instant::now() < deadline, "the move never reached the store");
                std::thread::sleep(Duration::from_millis(2));
            }
            std::fs::write(&s.legacy, "[keys]\nSAM_API_KEY = \"newer-plain\"\n").unwrap();
            *s.keys.held.lock().unwrap() = None;
            assert_eq!(reading.join().unwrap().as_deref(), Some("older-plain"));
        });
        assert!(text(&s.legacy).contains("newer-plain"), "the newer key was taken out unmoved: {}", text(&s.legacy));
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).as_deref(), Some("newer-plain"), "it moves on the next read");
        assert!(!text(&s.legacy).contains("newer-plain"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn the_old_files_debug_shows_names_never_keys() {
        let mut file = Legacy::default();
        file.keys.insert(DEFAULT_SAM_KEY.into(), "plain-secret-key".into());
        let shown = format!("{file:?}");
        assert!(shown.contains(DEFAULT_SAM_KEY) && !shown.contains("plain-secret"), "{shown}");
    }

    /// No key saved, a key saved, and a key that cannot be read are three
    /// different answers: the Forge dialog must not take the last for none.
    #[test]
    fn read_saved_tells_no_key_from_an_unreadable_one() {
        let s = setup("read_saved");
        assert_eq!(read_saved_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY), Ok(None));
        assert_eq!(read_saved_with(&s.kf, &s.legacy, "NOT A NAME"), Ok(None), "nothing is saved under a non-name");
        s.keys.down.store(true, Ordering::SeqCst);
        // No file and a store that cannot be asked: a key may be there, or none.
        let not_asked = read_saved_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        assert!(matches!(not_asked, Err(ReadError::NotAsked(_))), "{not_asked:?}");
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert_eq!(read_saved_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY), Ok(Some("abc123def456".into())));
        // A fallback file that cannot be read: a key is surely saved.
        std::fs::write(s.dir.join("keys").join("sam.api_key.key"), [0xffu8]).unwrap();
        match read_saved_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY) {
            Err(ReadError::Unreadable(why)) => assert!(!why.contains("abc1"), "{why}"),
            other => panic!("a damaged file is an unreadable key: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    /// The folder credentials.toml sits in is shared: nothing here makes it
    /// private.
    #[cfg(windows)]
    #[test]
    fn the_shared_folder_is_never_made_private() {
        let s = setup("shared");
        std::fs::write(&s.legacy, "[keys]\nSAM_API_KEY = \"legacy-key-value\"\n\n[saved_at]\nX = \"t\"\n").unwrap();
        s.keys.down.store(true, Ordering::SeqCst);
        get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        set_with(&s.kf, &s.legacy, "SAM_API_KEY_B", "b-key-value").unwrap();
        assert!(crate::private_file::verify(&s.dir).is_err(), "the shared folder keeps its own access");
        crate::private_file::verify(&s.dir.join("keys")).expect("only the keys folder is private");
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn an_empty_key_is_refused_rather_than_saved_as_blank() {
        let s = setup("empty");
        assert!(matches!(set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "   "), Err(CaptureError::BadQuery(_))));
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY), None);
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn whitespace_around_a_pasted_key_is_trimmed() {
        // Copying from a web page routinely picks up a trailing newline, and a
        // key with one appended fails as a 403 that looks like a wrong key.
        let s = setup("trim");
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, "  abc123def456\n").unwrap();
        assert_eq!(get_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY).as_deref(), Some("abc123def456"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn the_status_never_carries_any_part_of_the_key() {
        let s = setup("status");
        let key = "QZXJ8W4KVPR7MN2T";
        set_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY, key).unwrap();
        let st = status_with(&s.kf, &s.legacy, DEFAULT_SAM_KEY);
        assert!(st.present && !st.from_environment && !st.outside_store, "{st:?}");
        assert!(st.saved_at.is_some(), "a save records when it happened");
        let line = st.summary();
        assert!(line.contains("is saved"), "{line}");
        let chars: Vec<char> = key.chars().collect();
        for window in chars.windows(4) {
            let piece: String = window.iter().collect();
            assert!(!line.contains(&piece), "the status line shows {piece:?} of the key: {line}");
        }
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn only_sam_credential_names_qualify() {
        for ok in ["SAM_API_KEY", "SAM_API_KEY_CLIENT_B", "SAM_API_KEY_2", "SAM_API_KEY_A_B_C"] {
            assert!(is_sam_credential_name(ok), "{ok} should qualify");
        }
        for bad in [
            "ANTHROPIC_API_KEY",
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "PATH",
            "SAM_API_KEY_",
            "SAM_API_KEYX",
            "sam_api_key",
            "SAM_API_KEY_client",
            "SAM_API_KEY_A B",
            "SAM_API_KEY_A-B",
            "",
        ] {
            assert!(!is_sam_credential_name(bad), "{bad:?} must not qualify");
        }
    }

    #[test]
    fn an_arbitrary_environment_variable_is_never_read() {
        // PATH is set on every machine, so before this rule `get("PATH")`
        // returned it. A Connector naming PATH, or ANTHROPIC_API_KEY, must get
        // nothing, and the key dialog must not show four characters of it.
        assert!(std::env::var("PATH").is_ok(), "the test relies on PATH being set");
        assert_eq!(env_value("PATH"), None);
        let st = status_from_env("PATH");
        assert!(!st.present, "{st:?}");
        assert_eq!(st.saved_at, None, "{st:?}");
    }

    #[test]
    fn the_file_never_lands_inside_a_space() {
        // The whole point. If this path is ever under a Space, the key ends up
        // in an audit folder and from there in a commit.
        let Some(p) = credentials_path() else { return };
        let s = p.to_string_lossy().to_lowercase();
        assert!(s.ends_with("credentials.toml"), "{s}");
        assert!(
            !s.contains("documents") && !s.contains("spaces") && !s.contains("workspace"),
            "the credential file must live outside every Space: {s}"
        );
    }
}
