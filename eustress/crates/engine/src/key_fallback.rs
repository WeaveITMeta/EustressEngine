//! A key in the system credential store, with a private file as the fallback
//! while the store cannot take it. Every caller that keeps a key (the Soul
//! provider keys, the capture and Forge keys) goes through here, so the order
//! that keeps the newest key is written once.
//!
//! The fallback lives in its own folder, `<local data>\Eustress\keys\`
//! (`%LOCALAPPDATA%` on Windows, `~/Library/Application Support` on macOS,
//! `$XDG_DATA_HOME` or `~/.local/share` on Linux)
//! (the platform data-local directory elsewhere), one file per credential:
//! `keys\<provider>.<purpose>.key`, holding the key as UTF-8 and nothing else.
//! A name's segments are `[a-z0-9_-]`, so the mapping is one to one and cannot
//! leave the folder. One file per name means no read-modify-write that could
//! drop another name's key, across queue threads or across Studio builds run
//! side by side, and one damaged file affects one key. Every write goes
//! through `private_file::write_private`, which makes the `keys` folder (only
//! that folder) and the file readable by the current user alone.
//!
//! The rule: the fallback file exists only while it holds the newest key.
//! - [`KeyFallback::save`] clears the file first (if that fails, the save is
//!   refused and nothing changes), then stores the key, read back and
//!   compared. If the store fails or does not answer in time, the key goes to
//!   the file; if that fails too, it is kept for this session only.
//! - [`KeyFallback::load`] takes a key from this session or the file first,
//!   because it is the newest, and moves it into the store (waiting at most
//!   [`TIMEOUT`]); the file is cleared only once the store holds it. Only with
//!   neither does it read the store. A file that cannot be read is an error:
//!   no key, no move, and never the store's older key.
//! - [`KeyFallback::remove`] removes the file first, then the store's key.
//!
//! Each operation runs under [`KeyFallback::lock`] for its name, held for the
//! whole operation, including a caller's own legacy step: without it, a save
//! that ran between a load's read of the file and its move into the store
//! would be overwritten by that older key. A migration at startup uses
//! [`KeyFallback::save_within`] with [`TIMEOUT`], so it waits 5 s on every
//! platform; a person's Save uses [`KeyFallback::save`].
//!
//! These calls wait on the credential queue: call them from a worker thread or
//! task, never from a frame system. No key appears in any outcome's text or
//! `Debug` output.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use eustress_common::credentials::{submit, CredentialError, CredentialName, Done, Op, TIMEOUT};

/// Where a loaded key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The system credential store.
    Store,
    /// The private fallback file: the store could not take it yet.
    File,
    /// Memory, for this session only: neither the store nor the file could.
    Session,
}

/// What a save did. Each text is showable and never holds the key.
#[derive(Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// In the system credential store, read back and compared.
    Stored,
    /// The store could not take it; it is in the private fallback file.
    KeptInFile(String),
    /// Neither could; it is kept in memory until Studio closes.
    SessionOnly(String),
    /// The old fallback copy could not be cleared, so nothing was changed.
    Aborted(String),
}

/// What a load found.
#[derive(Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// A key, where it is now, and a warning to show when it is not in the
    /// store.
    Key { value: String, place: Place, warning: Option<String> },
    /// No key anywhere.
    NoKey,
    /// The fallback file exists and could not be read: no key is used.
    FileError(String),
    /// The store could not be asked, and there is no fallback copy.
    StoreError(String),
}

/// What a remove did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Removed from the file and the store; `false` when there was none.
    Removed(bool),
    /// The fallback file could not be removed, so nothing was changed.
    Aborted(String),
    /// The fallback copy is gone, but the store could not be asked.
    StoreError(String),
}

impl std::fmt::Debug for SaveOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveOutcome::Stored => f.write_str("Stored"),
            SaveOutcome::KeptInFile(why) => write!(f, "KeptInFile({why:?})"),
            SaveOutcome::SessionOnly(why) => write!(f, "SessionOnly({why:?})"),
            SaveOutcome::Aborted(why) => write!(f, "Aborted({why:?})"),
        }
    }
}

impl std::fmt::Debug for LoadOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadOutcome::Key { place, warning, .. } => write!(f, "Key {{ value: <key>, place: {place:?}, warning: {warning:?} }}"),
            LoadOutcome::NoKey => f.write_str("NoKey"),
            LoadOutcome::FileError(why) => write!(f, "FileError({why:?})"),
            LoadOutcome::StoreError(why) => write!(f, "StoreError({why:?})"),
        }
    }
}

/// The store side, so tests can stand in for the system store.
pub(crate) trait KeyStore: Send + Sync {
    /// Store, read back and compare, waiting at most `limit` when given.
    fn set_verified(&self, name: &CredentialName, key: &str, limit: Option<Duration>) -> Result<(), CredentialError>;
    fn get(&self, name: &CredentialName) -> Result<Option<String>, CredentialError>;
    fn delete(&self, name: &CredentialName) -> Result<bool, CredentialError>;
    /// [`Self::get`], waiting at most `limit`.
    fn get_within(&self, name: &CredentialName, limit: Duration) -> Result<Option<String>, CredentialError> {
        let _ = limit;
        self.get(name)
    }
}

/// The system credential store, through its per-name queue.
struct SystemKeys;

fn unexpected(name: &CredentialName) -> CredentialError {
    CredentialError::Platform(format!("the credential queue answered {name} with another call's result"))
}

impl KeyStore for SystemKeys {
    fn set_verified(&self, name: &CredentialName, key: &str, limit: Option<Duration>) -> Result<(), CredentialError> {
        let ticket = submit(name, Op::SetVerified(key.to_string()));
        match limit.map_or_else(|| ticket.wait(), |l| ticket.wait_up_to(l))? {
            Done::Stored => Ok(()),
            _ => Err(unexpected(name)),
        }
    }
    fn get(&self, name: &CredentialName) -> Result<Option<String>, CredentialError> {
        match submit(name, Op::Get).wait()? {
            Done::Got(value) => Ok(value),
            _ => Err(unexpected(name)),
        }
    }
    fn delete(&self, name: &CredentialName) -> Result<bool, CredentialError> {
        match submit(name, Op::Delete).wait()? {
            Done::Deleted(had) => Ok(had),
            _ => Err(unexpected(name)),
        }
    }
    fn get_within(&self, name: &CredentialName, limit: Duration) -> Result<Option<String>, CredentialError> {
        match submit(name, Op::Get).wait_up_to(limit)? {
            Done::Got(value) => Ok(value),
            _ => Err(unexpected(name)),
        }
    }
}

/// Keys kept for this session only, by fallback folder and name.
fn session() -> &'static Mutex<HashMap<(PathBuf, String), String>> {
    static SESSION: OnceLock<Mutex<HashMap<(PathBuf, String), String>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(HashMap::new()))
}

/// No file is at the path: it is missing, or a folder on the way to it is a
/// file. Windows reports both as not found; macOS and Linux report the second
/// as "not a directory" (ENOTDIR), and it is just as surely no key file.
fn absent(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory)
}

/// The names locked by a [`KeyGuard`], by fallback folder and name.
fn locks() -> &'static (Mutex<HashSet<(PathBuf, String)>>, Condvar) {
    static LOCKS: OnceLock<(Mutex<HashSet<(PathBuf, String)>>, Condvar)> = OnceLock::new();
    LOCKS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

/// One credential name, held for a whole operation: a load's read of the
/// file and its move into the store, or a caller's legacy step plus a save,
/// cannot interleave with another save, load or remove of the same name in
/// this process. Taken with [`KeyFallback::lock`]; released when dropped. It is
/// held at most as long as its operation's store calls, which are bounded by
/// their timeouts.
pub struct KeyGuard {
    slot: (PathBuf, String),
    name: CredentialName,
}

impl KeyGuard {
    /// The name this guard holds.
    pub fn name(&self) -> &CredentialName {
        &self.name
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        let (held, released) = locks();
        if let Ok(mut held) = held.lock() {
            held.remove(&self.slot);
        }
        released.notify_all();
    }
}

/// The store with its private fallback folder.
pub struct KeyFallback {
    dir: PathBuf,
    store: Box<dyn KeyStore>,
}

impl KeyFallback {
    /// The system store, with `<local data>/Eustress/keys/` (the platform
    /// data-local directory elsewhere). `None` only when that directory cannot
    /// be resolved; a file that cannot be read is reported by [`Self::load`].
    pub fn standard() -> Option<Self> {
        Some(Self::with(dirs::data_local_dir()?.join("Eustress").join("keys"), Box::new(SystemKeys)))
    }

    pub(crate) fn with(dir: PathBuf, store: Box<dyn KeyStore>) -> Self {
        Self { dir, store }
    }

    /// `keys\<provider>.<purpose>.key`.
    fn file(&self, name: &CredentialName) -> PathBuf {
        let rest = name.as_str().strip_prefix("eustress/").unwrap_or(name.as_str());
        self.dir.join(format!("{}.key", rest.replace('/', ".")))
    }

    fn session_key(&self, name: &CredentialName) -> (PathBuf, String) {
        (self.dir.clone(), name.as_str().to_string())
    }

    /// The fallback file's key: `Ok(None)` when there is no file; an error
    /// for a file that cannot be read, is not UTF-8, or is empty.
    fn read_key(&self, name: &CredentialName) -> io::Result<Option<String>> {
        let path = self.file(name);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if absent(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "it is not text"))?;
        let key = text.trim();
        if key.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "it is empty"));
        }
        Ok(Some(key.to_string()))
    }

    /// Hold `name` for one operation; see [`KeyGuard`]. Waits while another
    /// thread holds it.
    pub fn lock(&self, name: &CredentialName) -> KeyGuard {
        let slot = self.session_key(name);
        let (held, released) = locks();
        let mut held = held.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while held.contains(&slot) {
            held = released.wait(held).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        held.insert(slot.clone());
        KeyGuard { slot, name: name.clone() }
    }

    /// Remove the fallback file, and the temporary file a write that failed
    /// at its rename can leave holding the key; `Ok(true)` when there was a
    /// fallback file.
    fn clear(&self, name: &CredentialName) -> io::Result<bool> {
        let path = self.file(name);
        let mut tmp = path.clone().into_os_string();
        tmp.push(".tmp");
        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(e) if absent(&e) => {}
            Err(e) => return Err(e),
        }
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if absent(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Save `key` under the guarded name: see the module documentation for
    /// the order. Waits for the store as long as the write's own timeout (30 s
    /// on macOS and Linux): for a person's Save.
    pub fn save(&self, guard: &KeyGuard, key: &str) -> SaveOutcome {
        self.save_inner(guard, key, None)
    }

    /// [`Self::save`], waiting for the store at most `limit` (never longer
    /// than the write's own timeout): for a migration at startup, which passes
    /// [`TIMEOUT`]. A save that times out keeps the key in the fallback file,
    /// as any failed save does.
    pub fn save_within(&self, guard: &KeyGuard, key: &str, limit: Duration) -> SaveOutcome {
        self.save_inner(guard, key, Some(limit))
    }

    fn save_inner(&self, guard: &KeyGuard, key: &str, limit: Option<Duration>) -> SaveOutcome {
        let name = guard.name();
        let path = self.file(name);
        if let Err(e) = self.clear(name) {
            return SaveOutcome::Aborted(format!(
                "the old copy at {} could not be cleared ({e}), so nothing was changed",
                path.display()
            ));
        }
        // A key kept for the session is older than this one.
        if let Ok(mut s) = session().lock() {
            s.remove(&self.session_key(name));
        }
        match self.store.set_verified(name, key, limit) {
            Ok(()) => SaveOutcome::Stored,
            Err(store_error) => match crate::private_file::write_private(&path, key.as_bytes()) {
                Ok(()) => SaveOutcome::KeptInFile(format!(
                    "{store_error}. The key is in {}, readable only by you but not encrypted, and moves \
                     into the store once it is available",
                    path.display()
                )),
                Err(file_error) => {
                    if let Ok(mut s) = session().lock() {
                        s.insert(self.session_key(name), key.to_string());
                    }
                    SaveOutcome::SessionOnly(format!(
                        "{store_error}, and {} could not be written ({file_error}). The key is kept only \
                         until Studio closes",
                        path.display()
                    ))
                }
            },
        }
    }

    /// The key under the guarded name: see the module documentation for the
    /// order.
    pub fn load(&self, guard: &KeyGuard) -> LoadOutcome {
        let name = guard.name();
        let path = self.file(name);
        let from_session = session().lock().ok().and_then(|s| s.get(&self.session_key(name)).cloned());
        if let Some(value) = from_session {
            return match self.store.set_verified(name, &value, Some(TIMEOUT)) {
                Ok(()) => {
                    if let Ok(mut s) = session().lock() {
                        s.remove(&self.session_key(name));
                    }
                    LoadOutcome::Key { value, place: Place::Store, warning: None }
                }
                Err(e) => LoadOutcome::Key {
                    value,
                    place: Place::Session,
                    warning: Some(format!("{e}. The key is kept only until Studio closes")),
                },
            };
        }
        let value = match self.read_key(name) {
            Err(e) => return LoadOutcome::FileError(format!("{} could not be read: {e}", path.display())),
            Ok(Some(value)) => value,
            Ok(None) => {
                return match self.store.get(name) {
                    Ok(Some(value)) => LoadOutcome::Key { value, place: Place::Store, warning: None },
                    Ok(None) => LoadOutcome::NoKey,
                    Err(e) => LoadOutcome::StoreError(e.to_string()),
                };
            }
        };
        match self.store.set_verified(name, &value, Some(TIMEOUT)) {
            Ok(()) => {
                let warning = self.clear(name).err().map(|e| {
                    format!("the key is in the store, but its old copy at {} stays: {e}", path.display())
                });
                LoadOutcome::Key { value, place: Place::Store, warning }
            }
            Err(e) => LoadOutcome::Key {
                value,
                place: Place::File,
                warning: Some(format!(
                    "{e}. The key is in {}, readable only by you but not encrypted",
                    path.display()
                )),
            },
        }
    }

    /// Remove the key under the guarded name: the fallback file first, then
    /// the store.
    pub fn remove(&self, guard: &KeyGuard) -> RemoveOutcome {
        let name = guard.name();
        let had_file = match self.clear(name) {
            Ok(had) => had,
            Err(e) => {
                return RemoveOutcome::Aborted(format!(
                    "{} could not be removed ({e}), so nothing was changed",
                    self.file(name).display()
                ))
            }
        };
        let had_session = session().lock().map(|mut s| s.remove(&self.session_key(name)).is_some()).unwrap_or(false);
        match self.store.delete(name) {
            Ok(had) => RemoveOutcome::Removed(had || had_file || had_session),
            Err(e) => RemoveOutcome::StoreError(e.to_string()),
        }
    }

    /// The person's current key under `name`, read only: this session's, then
    /// the fallback file's, then the store's, the same order [`Self::load`]
    /// uses, but nothing is moved or written and no guard is needed. For a
    /// check such as a publish preflight. `Ok(None)` when nothing is saved;
    /// an error (showable, never a key) when the file cannot be read or the
    /// store fails or does not answer within `limit`.
    pub fn peek(&self, name: &CredentialName, limit: Duration) -> Result<Option<String>, String> {
        if let Some(value) = session().lock().ok().and_then(|s| s.get(&self.session_key(name)).cloned()) {
            return Ok(Some(value));
        }
        match self.read_key(name) {
            Err(e) => return Err(format!("{} could not be read: {e}", self.file(name).display())),
            Ok(Some(value)) => return Ok(Some(value)),
            Ok(None) => {}
        }
        self.store.get_within(name, limit).map_err(|e| e.to_string())
    }

    /// The fallback folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::credentials::{timeout_for, Platform};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A system store in memory that can be down, slow (times out) or
    /// answer normally.
    /// Runs once, inside the store write of a given key, before it lands.
    type Hook = Box<dyn FnOnce() + Send>;

    #[derive(Default, Clone)]
    struct TestKeys {
        map: Arc<Mutex<HashMap<String, String>>>,
        down: Arc<AtomicBool>,
        /// Every write times out, as the queue reports it under the Linux
        /// policy (`timeout_for`): the limit given, capped at the write's own.
        slow: Arc<AtomicBool>,
        hook: Arc<Mutex<Option<(String, Hook)>>>,
    }

    impl TestKeys {
        fn holds(&self, name: &CredentialName) -> Option<String> {
            self.map.lock().unwrap().get(name.as_str()).cloned()
        }
        fn check(&self) -> Result<(), CredentialError> {
            if self.down.load(Ordering::SeqCst) {
                return Err(CredentialError::Unavailable("the test store is down".into()));
            }
            Ok(())
        }
    }

    impl KeyStore for TestKeys {
        fn set_verified(&self, name: &CredentialName, key: &str, limit: Option<Duration>) -> Result<(), CredentialError> {
            self.check()?;
            if self.slow.load(Ordering::SeqCst) {
                let own = timeout_for(&Op::SetVerified(String::new()), Platform::Linux);
                let waited = limit.map_or(own, |l| l.min(own));
                return Err(CredentialError::TimedOut { waited, queued_behind: false });
            }
            let fire = {
                let mut hook = self.hook.lock().unwrap();
                match hook.as_ref() {
                    Some((when, _)) if when == key => hook.take().map(|(_, f)| f),
                    _ => None,
                }
            };
            if let Some(f) = fire {
                f();
            }
            self.map.lock().unwrap().insert(name.as_str().to_string(), key.to_string());
            Ok(())
        }
        fn get(&self, name: &CredentialName) -> Result<Option<String>, CredentialError> {
            self.check()?;
            Ok(self.holds(name))
        }
        fn delete(&self, name: &CredentialName) -> Result<bool, CredentialError> {
            self.check()?;
            Ok(self.map.lock().unwrap().remove(name.as_str()).is_some())
        }
    }

    fn folder(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eustress-keyfallback-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn setup(tag: &str) -> (PathBuf, TestKeys, KeyFallback) {
        let parent = folder(tag);
        let keys = TestKeys::default();
        let kf = KeyFallback::with(parent.join("keys"), Box::new(keys.clone()));
        (parent, keys, kf)
    }

    fn save(kf: &KeyFallback, name: &CredentialName, key: &str) -> SaveOutcome {
        kf.save(&kf.lock(name), key)
    }

    fn load(kf: &KeyFallback, name: &CredentialName) -> LoadOutcome {
        kf.load(&kf.lock(name))
    }

    fn remove(kf: &KeyFallback, name: &CredentialName) -> RemoveOutcome {
        kf.remove(&kf.lock(name))
    }

    fn soul() -> CredentialName {
        CredentialName::new("anthropic", "api_key").unwrap()
    }

    fn forge() -> CredentialName {
        CredentialName::new("forge", "api_key").unwrap()
    }

    fn key_of(outcome: &LoadOutcome) -> Option<(&str, Place)> {
        match outcome {
            LoadOutcome::Key { value, place, .. } => Some((value.as_str(), *place)),
            _ => None,
        }
    }

    #[test]
    fn a_save_goes_into_the_store_and_leaves_no_file() {
        let (parent, keys, kf) = setup("store");
        assert_eq!(save(&kf, &soul(), "sk-a"), SaveOutcome::Stored);
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-a"));
        assert!(!kf.file(&soul()).exists());
        assert_eq!(key_of(&load(&kf, &soul())), Some(("sk-a", Place::Store)));
        let _ = std::fs::remove_dir_all(parent);
    }

    /// Key A in the store, the store goes down, the person saves B, the store
    /// comes back: B is used, B is in the store, and no file copy remains.
    #[test]
    fn the_newest_key_wins_after_the_store_comes_back() {
        let (parent, keys, kf) = setup("a_then_b");
        assert_eq!(save(&kf, &soul(), "sk-a"), SaveOutcome::Stored);
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-b"), SaveOutcome::KeptInFile(_)));
        assert_eq!(std::fs::read_to_string(kf.file(&soul())).unwrap(), "sk-b");
        keys.down.store(false, Ordering::SeqCst);
        assert_eq!(key_of(&load(&kf, &soul())), Some(("sk-b", Place::Store)), "B, not the store's older A");
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-b"));
        assert!(!kf.file(&soul()).exists(), "the fallback copy is gone");
        let _ = std::fs::remove_dir_all(parent);
    }

    /// A clear that fails refuses the save and changes nothing: the old file
    /// and the store's key both stay.
    #[test]
    fn a_failed_clear_aborts_the_save_with_both_copies_unchanged() {
        let (parent, keys, kf) = setup("clear_fails");
        assert_eq!(save(&kf, &soul(), "sk-a"), SaveOutcome::Stored);
        // A folder where the file goes cannot be removed as a file.
        std::fs::create_dir_all(kf.file(&soul()).join("blocker")).unwrap();
        let outcome = save(&kf, &soul(), "sk-b");
        assert!(matches!(outcome, SaveOutcome::Aborted(_)), "{outcome:?}");
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-a"), "the store is unchanged");
        assert!(kf.file(&soul()).join("blocker").is_dir(), "the file side is unchanged");
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn a_failed_store_write_after_the_clear_leaves_the_new_key_in_the_file() {
        let (parent, keys, kf) = setup("store_fails");
        keys.down.store(true, Ordering::SeqCst);
        let outcome = save(&kf, &soul(), "sk-new");
        assert!(matches!(outcome, SaveOutcome::KeptInFile(ref why) if !why.contains("sk-new")), "{outcome:?}");
        assert_eq!(std::fs::read_to_string(kf.file(&soul())).unwrap(), "sk-new");
        assert_eq!(key_of(&load(&kf, &soul())), Some(("sk-new", Place::File)));
        assert!(kf.file(&soul()).exists(), "a failed move leaves the file: it is the only copy");
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn a_timed_out_store_write_leaves_the_new_key_in_the_file() {
        let (parent, keys, kf) = setup("store_slow");
        keys.slow.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-new"), SaveOutcome::KeptInFile(_)));
        assert_eq!(std::fs::read_to_string(kf.file(&soul())).unwrap(), "sk-new");
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn remove_deletes_both_copies() {
        let (parent, keys, kf) = setup("remove");
        assert_eq!(save(&kf, &soul(), "sk-a"), SaveOutcome::Stored);
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-b"), SaveOutcome::KeptInFile(_)));
        keys.down.store(false, Ordering::SeqCst);
        // The store still holds A here; the file holds B.
        assert!(kf.file(&soul()).exists());
        assert_eq!(remove(&kf, &soul()), RemoveOutcome::Removed(true));
        assert!(!kf.file(&soul()).exists());
        assert_eq!(keys.holds(&soul()), None);
        assert_eq!(load(&kf, &soul()), LoadOutcome::NoKey);
        let _ = std::fs::remove_dir_all(parent);
    }

    /// A fallback file that cannot be read is an error: no key, and never the
    /// store's older key.
    #[test]
    fn an_unreadable_file_is_an_error_and_the_older_store_key_is_not_used() {
        let (parent, keys, kf) = setup("unreadable");
        assert_eq!(save(&kf, &soul(), "sk-old"), SaveOutcome::Stored);
        std::fs::create_dir_all(kf.dir()).unwrap();
        for bad in [&[0xffu8, 0xfe][..], b"   \n"] {
            std::fs::write(kf.file(&soul()), bad).unwrap();
            match load(&kf, &soul()) {
                LoadOutcome::FileError(why) => {
                    assert!(why.contains(&kf.file(&soul()).display().to_string()), "names the path: {why}");
                    assert!(!why.contains("sk-old"), "{why}");
                }
                other => panic!("expected a file error, got {other:?}"),
            }
        }
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-old"), "nothing was moved");
        let _ = std::fs::remove_dir_all(parent);
    }

    /// One file per name: writing one key never touches another's, and a
    /// damaged file affects its own key only.
    #[test]
    fn each_key_has_its_own_file() {
        let (parent, keys, kf) = setup("per_name");
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-soul"), SaveOutcome::KeptInFile(_)));
        assert!(matches!(save(&kf, &forge(), "fk-forge"), SaveOutcome::KeptInFile(_)));
        assert_eq!(std::fs::read_to_string(kf.dir().join("anthropic.api_key.key")).unwrap(), "sk-soul");
        assert_eq!(std::fs::read_to_string(kf.dir().join("forge.api_key.key")).unwrap(), "fk-forge");
        std::fs::write(kf.file(&soul()), [0xffu8]).unwrap();
        assert!(matches!(load(&kf, &soul()), LoadOutcome::FileError(_)));
        assert_eq!(key_of(&load(&kf, &forge())), Some(("fk-forge", Place::File)), "Forge still loads");
        let _ = std::fs::remove_dir_all(parent);
    }

    /// Only the keys folder is made private; the folder it sits in (shared
    /// with Modes, Plugins, the Player's files) keeps its own access.
    #[cfg(windows)]
    #[test]
    fn only_the_keys_folder_is_made_private() {
        let (parent, keys, kf) = setup("private");
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-a"), SaveOutcome::KeptInFile(_)));
        crate::private_file::verify(kf.dir()).expect("the keys folder is private");
        crate::private_file::verify(&kf.file(&soul())).expect("the key file is private");
        assert!(crate::private_file::verify(&parent).is_err(), "the parent folder was not made private");
        let _ = std::fs::remove_dir_all(parent);
    }

    /// A path through a file holds no key file, on this platform's own
    /// errors: Windows says not found, macOS and Linux say ENOTDIR.
    #[test]
    fn a_path_through_a_file_is_no_key_file() {
        let parent = folder("through");
        std::fs::write(parent.join("file"), b"x").unwrap();
        let inside = parent.join("file").join("k.key");
        let read = std::fs::read(&inside).unwrap_err();
        assert!(absent(&read), "{read:?}");
        let removed = std::fs::remove_file(&inside).unwrap_err();
        assert!(absent(&removed), "{removed:?}");
        assert!(!absent(&io::Error::from(io::ErrorKind::PermissionDenied)), "a refusal is not an absence");
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn with_store_and_file_both_failing_the_key_lives_for_the_session() {
        let (parent, keys, kf) = setup("session");
        keys.down.store(true, Ordering::SeqCst);
        // A file where the keys folder should be: nothing can be written there.
        std::fs::write(kf.dir(), b"not a folder").unwrap();
        let outcome = save(&kf, &soul(), "sk-a");
        assert!(matches!(outcome, SaveOutcome::SessionOnly(_)), "{outcome:?}");
        assert_eq!(key_of(&load(&kf, &soul())), Some(("sk-a", Place::Session)));
        keys.down.store(false, Ordering::SeqCst);
        assert_eq!(key_of(&load(&kf, &soul())), Some(("sk-a", Place::Store)), "it moves in once the store is back");
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-a"));
        let _ = std::fs::remove_dir_all(parent);
    }

    /// A save that arrives while a load is moving the file's key into the
    /// store waits for it, so the newer save is what the store keeps.
    #[test]
    fn a_save_during_a_loads_move_is_not_overwritten_by_it() {
        let parent = folder("lock");
        let keys = TestKeys::default();
        let kf = Arc::new(KeyFallback::with(parent.join("keys"), Box::new(keys.clone())));
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-b"), SaveOutcome::KeptInFile(_)));
        keys.down.store(false, Ordering::SeqCst);
        // At the load's move of B into the store, a save of C starts on
        // another thread and is given 200 ms to run before B lands.
        let saver: Arc<Mutex<Option<std::thread::JoinHandle<SaveOutcome>>>> = Arc::default();
        let (kf2, saver2) = (kf.clone(), saver.clone());
        *keys.hook.lock().unwrap() = Some((
            "sk-b".to_string(),
            Box::new(move || {
                let handle = std::thread::spawn(move || save(&kf2, &soul(), "sk-c"));
                *saver2.lock().unwrap() = Some(handle);
                std::thread::sleep(Duration::from_millis(200));
            }),
        ));
        assert_eq!(key_of(&load(&kf, &soul())), Some(("sk-b", Place::Store)));
        let handle = saver.lock().unwrap().take().expect("the hook ran");
        assert_eq!(handle.join().unwrap(), SaveOutcome::Stored);
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-c"), "the newer save is kept, not the load's B");
        assert!(!kf.file(&soul()).exists());
        let _ = std::fs::remove_dir_all(parent);
    }

    /// The startup migration waits 5 s on every platform; a person's Save
    /// waits the write's own limit (30 s on macOS and Linux). Both keep the key in the
    /// file when the store does not answer.
    #[test]
    fn save_within_waits_its_limit_under_the_linux_policy() {
        let (parent, keys, kf) = setup("within");
        keys.slow.store(true, Ordering::SeqCst);
        match kf.save_within(&kf.lock(&soul()), "sk-a", TIMEOUT) {
            SaveOutcome::KeptInFile(why) => assert!(why.contains("after 5 s"), "{why}"),
            other => panic!("expected the file, got {other:?}"),
        }
        match save(&kf, &forge(), "fk-a") {
            SaveOutcome::KeptInFile(why) => assert!(why.contains("after 30 s"), "{why}"),
            other => panic!("expected the file, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(kf.file(&soul())).unwrap(), "sk-a");
        let _ = std::fs::remove_dir_all(parent);
    }

    /// A write whose rename failed can leave `<file>.tmp` holding the key: a
    /// remove leaves nothing behind.
    #[test]
    fn remove_also_deletes_a_left_over_temporary_file() {
        let (parent, keys, kf) = setup("tmp");
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-a"), SaveOutcome::KeptInFile(_)));
        let mut tmp = kf.file(&soul()).into_os_string();
        tmp.push(".tmp");
        std::fs::write(&tmp, "sk-a").unwrap();
        keys.down.store(false, Ordering::SeqCst);
        assert_eq!(remove(&kf, &soul()), RemoveOutcome::Removed(true));
        assert!(!kf.file(&soul()).exists());
        assert!(!std::path::Path::new(&tmp).exists(), "no copy of the key is left");
        let _ = std::fs::remove_dir_all(parent);
    }

    /// A peek finds the newest key (the file's, while the store still holds
    /// an older one) and moves nothing.
    #[test]
    fn peek_reads_the_newest_key_and_changes_nothing() {
        let (parent, keys, kf) = setup("peek");
        assert_eq!(kf.peek(&soul(), TIMEOUT), Ok(None), "nothing saved");
        assert_eq!(save(&kf, &soul(), "sk-a"), SaveOutcome::Stored);
        assert_eq!(kf.peek(&soul(), TIMEOUT), Ok(Some("sk-a".into())), "from the store");
        keys.down.store(true, Ordering::SeqCst);
        assert!(matches!(save(&kf, &soul(), "sk-b"), SaveOutcome::KeptInFile(_)));
        keys.down.store(false, Ordering::SeqCst);
        assert_eq!(kf.peek(&soul(), TIMEOUT), Ok(Some("sk-b".into())), "the file's newer key, not the store's A");
        assert_eq!(keys.holds(&soul()).as_deref(), Some("sk-a"), "nothing moved into the store");
        assert!(kf.file(&soul()).exists(), "and the file stays");
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn peek_reports_an_unreadable_file_or_a_store_it_cannot_ask() {
        let (parent, keys, kf) = setup("peek_errors");
        keys.down.store(true, Ordering::SeqCst);
        assert!(kf.peek(&soul(), TIMEOUT).is_err(), "the store is down and no file holds it");
        std::fs::create_dir_all(kf.dir()).unwrap();
        std::fs::write(kf.file(&soul()), [0xffu8]).unwrap();
        let err = kf.peek(&soul(), TIMEOUT).unwrap_err();
        assert!(err.contains(&kf.file(&soul()).display().to_string()), "{err}");
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn no_outcome_shows_the_key() {
        let shown = format!(
            "{:?} {:?}",
            LoadOutcome::Key { value: "sk-secret".into(), place: Place::Store, warning: None },
            SaveOutcome::KeptInFile("why".into())
        );
        assert!(!shown.contains("sk-secret"), "{shown}");
    }
}
