//! Secrets (AI provider keys, mail and print service credentials) in the
//! operating system's credential store: Windows Credential Manager, the macOS
//! Keychain, or the Linux Secret Service.
//!
//! Every secret has a namespaced name, `eustress/<provider>/<purpose>`, for
//! example `eustress/anthropic/api_key`. [`set`], [`get`] and [`delete`] act
//! on the store; each returns an error the caller can show when the store
//! cannot be reached.
//!
//! There is no plain-file path in this module, by design. A caller whose
//! store is unavailable keeps the secret for the session and, if it must
//! persist it, writes it through the engine's `private_file::write_private`
//! with a visible warning. It never writes plain JSON.
//!
//! The backend is chosen once per platform and must keep what it stores until
//! it is deleted: `keyring` falls back to an in-memory mock on a platform
//! whose backend feature is off, which would lose every key at exit without an
//! error, so a backend whose persistence is anything else is refused as
//! unavailable.
//!
//! No secret ever appears in an error, a log line or a `Debug` output.
//!
//! What the store protects: a secret kept here is out of every file, so it
//! is never synced, backed up, committed or published with a Space, and other
//! accounts on the machine cannot read it. What it does not protect against:
//! any program running as the same user can read it (on Windows a generic
//! credential is readable by every process of that user), so it is no defence
//! against malware running as the person. On Windows, `keyring` writes with
//! enterprise persistence (`CRED_PERSIST_ENTERPRISE`), so on a machine with a
//! domain roaming profile the credential roams with that profile; on a home
//! machine it stays local.
//!
//! A caller moving a secret out of a plain file uses [`set_verified`] (store,
//! read back, compare) and removes the plain copy only after it succeeds.

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use keyring::credential::{CredentialBuilder, CredentialPersistence};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The account half of every entry. The name lives in the service half, so
/// one constant account keeps every entry findable by name alone.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
const ACCOUNT: &str = "eustress";

/// Longest provider or purpose segment.
const MAX_SEGMENT: usize = 64;

/// Why a credential call failed. `Display` is a sentence a caller can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// The store cannot be reached or refused access: none on this platform,
    /// none running (a Linux machine without a Secret Service), or locked.
    Unavailable(String),
    /// A provider or purpose that is not a plain name.
    InvalidName(String),
    /// The store answered with an error of its own.
    Platform(String),
    /// The caller stopped waiting (see [`submit`]). The operation is still
    /// queued, or still running, and completes in order; its result can
    /// still arrive on the ticket. `queued_behind`: an earlier operation on
    /// the same name had not finished when the wait ended.
    TimedOut { waited: Duration, queued_behind: bool },
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::Unavailable(why) => write!(f, "the system credential store is unavailable: {why}"),
            CredentialError::InvalidName(why) => write!(f, "not a credential name: {why}"),
            CredentialError::Platform(why) => write!(f, "the system credential store failed: {why}"),
            CredentialError::TimedOut { waited, queued_behind: true } => write!(
                f,
                "still waiting for the system credential store: an earlier change to this key has not \
                 finished after {} s, and this one runs after it",
                waited.as_secs()
            ),
            CredentialError::TimedOut { waited, queued_behind: false } => write!(
                f,
                "still waiting for the system credential store after {} s; the change completes when it answers",
                waited.as_secs()
            ),
        }
    }
}

impl std::error::Error for CredentialError {}

/// A secret's name: `eustress/<provider>/<purpose>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialName {
    service: String,
}

impl CredentialName {
    /// `provider` and `purpose` are each 1 to 64 characters of `a-z`, `0-9`,
    /// `_` and `-`, such as `anthropic` and `api_key`.
    pub fn new(provider: &str, purpose: &str) -> Result<Self, CredentialError> {
        for (what, segment) in [("provider", provider), ("purpose", purpose)] {
            let plain = !segment.is_empty()
                && segment.len() <= MAX_SEGMENT
                && segment.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
            if !plain {
                return Err(CredentialError::InvalidName(format!(
                    "{what} '{segment}' must be 1 to {MAX_SEGMENT} of a-z, 0-9, _ and -"
                )));
            }
        }
        Ok(Self { service: format!("eustress/{provider}/{purpose}") })
    }

    /// `eustress/<provider>/<purpose>`.
    pub fn as_str(&self) -> &str {
        &self.service
    }
}

impl std::fmt::Display for CredentialName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.service)
    }
}

/// Store `secret` under `name`, replacing what was there.
pub fn set(name: &CredentialName, secret: &str) -> Result<(), CredentialError> {
    Store::native()?.set(name, secret)
}

/// Store `secret` under `name`, read it back and compare. `Ok` only when the
/// store holds exactly `secret`: a caller moving a secret out of a plain file
/// removes that copy only after this succeeds.
pub fn set_verified(name: &CredentialName, secret: &str) -> Result<(), CredentialError> {
    Store::native()?.set_verified(name, secret)
}

/// The secret stored under `name`, or `None` when there is none.
pub fn get(name: &CredentialName) -> Result<Option<String>, CredentialError> {
    Store::native()?.get(name)
}

/// Remove the secret under `name`. `false` when there was none.
pub fn delete(name: &CredentialName) -> Result<bool, CredentialError> {
    Store::native()?.delete(name)
}

// ── Ordered, bounded calls ──────────────────────────────────────────────────
//
// A store call can hang: a Keychain access prompt or a Secret Service unlock
// prompt waits on the person. [`submit`] puts the call on the queue for its name and
// returns at once. Each name has one worker running its calls strictly in
// submission order, so a slow write can never land after a newer save or
// remove and put an old key back. A caller's timeout only ends its wait; the
// call keeps its place and completes.

/// How long a caller waits for a call before [`CredentialError::TimedOut`]: 5
/// seconds.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// How long a caller waits for any call on macOS or Linux, where the store
/// may be showing the person a prompt, 30 seconds: the Keychain asks again
/// whenever a rebuilt (unsigned) Studio reads a key it saved, and a locked
/// Secret Service collection asks on a read as well as a write.
pub const SLOW_TIMEOUT: Duration = Duration::from_secs(30);

/// A call to queue with [`submit`].
#[derive(Clone)]
pub enum Op {
    Get,
    Set(String),
    SetVerified(String),
    Delete,
}

impl std::fmt::Debug for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Op::Get => "Get",
            Op::Set(_) => "Set(<secret>)",
            Op::SetVerified(_) => "SetVerified(<secret>)",
            Op::Delete => "Delete",
        })
    }
}

/// The platform a timeout is chosen for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Other,
}

impl Platform {
    /// The platform this build runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
}

/// How long a caller waits for `op` on `platform` by default: [`TIMEOUT`] on
/// Windows (Credential Manager never prompts), [`SLOW_TIMEOUT`] for every call
/// on macOS and Linux (their stores can prompt on any call). A pure function,
/// so every platform's policy is tested on any machine.
pub fn timeout_for(op: &Op, platform: Platform) -> Duration {
    let _ = op;
    match platform {
        Platform::MacOs | Platform::Linux => SLOW_TIMEOUT,
        Platform::Windows | Platform::Other => TIMEOUT,
    }
}

impl Op {
    /// [`timeout_for`] this platform.
    pub fn timeout(&self) -> Duration {
        timeout_for(self, Platform::current())
    }
}

/// What a completed call did.
#[derive(Clone, PartialEq, Eq)]
pub enum Done {
    /// A get: the secret, or `None` when nothing is stored.
    Got(Option<String>),
    /// A set or a verified set succeeded.
    Stored,
    /// A delete: `false` when there was nothing to delete.
    Deleted(bool),
}

impl std::fmt::Debug for Done {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Done::Got(Some(_)) => f.write_str("Got(Some(<secret>))"),
            Done::Got(None) => f.write_str("Got(None)"),
            Done::Stored => f.write_str("Stored"),
            Done::Deleted(had) => write!(f, "Deleted({had})"),
        }
    }
}

type Outcome = Result<Done, CredentialError>;

/// One queued call. Poll it from a frame system with [`Ticket::try_result`],
/// or block a worker thread on it with [`Ticket::wait`].
pub struct Ticket {
    seq: u64,
    started: Arc<AtomicU64>,
    submitted: Instant,
    timeout: Duration,
    rx: Mutex<mpsc::Receiver<Outcome>>,
    result: Mutex<Option<Outcome>>,
}

impl Ticket {
    /// The result, once the call has completed; `None` while it is queued or
    /// running. A call that outlived its timeout still delivers here. Never
    /// blocks: while another thread is in [`Ticket::wait`] on this ticket it
    /// reports no result yet.
    pub fn try_result(&self) -> Option<Outcome> {
        let mut result = self.result.lock().ok()?;
        if result.is_none() {
            let Ok(rx) = self.rx.try_lock() else { return None };
            match rx.try_recv() {
                Ok(outcome) => *result = Some(outcome),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    *result = Some(Err(CredentialError::Unavailable("the credential queue stopped".to_string())))
                }
            }
        }
        result.clone()
    }

    /// Block until the result, or until this call's timeout has passed since
    /// it was submitted: then [`CredentialError::TimedOut`], and the call
    /// carries on. Never call this on a UI or frame thread.
    pub fn wait(&self) -> Outcome {
        self.wait_up_to(self.timeout)
    }

    /// [`Ticket::wait`], waiting at most `limit` and never longer than this
    /// call's own timeout. Both count from when the call was submitted. For a
    /// caller that must not block long, such as a migration at startup (pass
    /// [`TIMEOUT`]). Only this wait is shorter: the call keeps its place and
    /// completes. A [`CredentialError::TimedOut`] carries the limit used.
    pub fn wait_up_to(&self, limit: Duration) -> Outcome {
        let limit = limit.min(self.timeout);
        if let Some(done) = self.try_result() {
            return done;
        }
        let left = limit.saturating_sub(self.submitted.elapsed());
        // `result` is not held across the wait, so `try_result` from another
        // thread never waits on this one. The receiver is held until the
        // result is cached: the worker drops the call's only sender right
        // after replying, and a `try_result` in that gap would otherwise see
        // the channel closed and cache "stopped" for a call that succeeded.
        let Ok(rx) = self.rx.lock() else {
            return Err(CredentialError::Unavailable("the credential queue stopped".to_string()));
        };
        let got = rx.recv_timeout(left);
        #[cfg(test)]
        tests::pause_after_receive();
        if let Ok(outcome) = &got {
            if let Ok(mut result) = self.result.lock() {
                *result = Some(outcome.clone());
            }
        }
        drop(rx);
        match got {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(CredentialError::TimedOut { waited: limit, queued_behind: self.queued_behind() })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => self.try_result().unwrap_or_else(|| {
                Err(CredentialError::Unavailable("the credential queue stopped".to_string()))
            }),
        }
    }

    /// An earlier call on the same name has not finished, so this one has
    /// not started: "still waiting for the system credential store".
    pub fn queued_behind(&self) -> bool {
        self.started.load(Ordering::SeqCst) < self.seq
    }

    /// This call's timeout has passed without a result.
    pub fn overdue(&self) -> bool {
        self.submitted.elapsed() >= self.timeout && self.try_result().is_none()
    }

    /// The wait [`Ticket::wait`] allows: [`Op::timeout`].
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

struct Job {
    seq: u64,
    op: Op,
    reply: mpsc::Sender<Outcome>,
}

struct Lane {
    tx: mpsc::Sender<Job>,
    next: u64,
    started: Arc<AtomicU64>,
}

type StoreFactory = Arc<dyn Fn() -> Result<Store, CredentialError> + Send + Sync>;

/// The queues, one per name, over one store.
struct Queues {
    store: StoreFactory,
    timeout: fn(&Op) -> Duration,
    lanes: Mutex<HashMap<String, Lane>>,
    /// Names a get, set or verified set has succeeded on in this process:
    /// see [`read_this_session`].
    answered: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl Queues {
    fn new(store: StoreFactory, timeout: fn(&Op) -> Duration) -> Self {
        Self { store, timeout, lanes: Mutex::new(HashMap::new()), answered: Arc::default() }
    }

    fn submit(&self, name: &CredentialName, op: Op) -> Ticket {
        let (reply, rx) = mpsc::channel();
        let timeout = (self.timeout)(&op);
        let mut lanes = self.lanes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let lane = lanes.entry(name.as_str().to_string()).or_insert_with(|| self.spawn(name));
        lane.next += 1;
        let seq = lane.next;
        let started = lane.started.clone();
        if lane.tx.send(Job { seq, op, reply: reply.clone() }).is_err() {
            let _ = reply.send(Err(CredentialError::Unavailable("the credential queue stopped".to_string())));
        }
        Ticket { seq, started, submitted: Instant::now(), timeout, rx: Mutex::new(rx), result: Mutex::new(None) }
    }

    /// The worker for one name: its calls, one at a time, in order.
    fn spawn(&self, name: &CredentialName) -> Lane {
        let (tx, rx) = mpsc::channel::<Job>();
        let started = Arc::new(AtomicU64::new(0));
        let (store, worker_started, key) = (self.store.clone(), started.clone(), name.clone());
        let answered = self.answered.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("credentials {}", name.as_str()))
            .spawn(move || {
                for job in rx {
                    worker_started.store(job.seq, Ordering::SeqCst);
                    // A backend that panics fails this call only; the lane
                    // keeps serving the calls after it.
                    let run = std::panic::AssertUnwindSafe(|| {
                        store().and_then(|s| match &job.op {
                            Op::Get => s.get(&key).map(Done::Got),
                            Op::Set(secret) => s.set(&key, secret).map(|()| Done::Stored),
                            Op::SetVerified(secret) => s.set_verified(&key, secret).map(|()| Done::Stored),
                            Op::Delete => s.delete(&key).map(Done::Deleted),
                        })
                    });
                    let outcome = std::panic::catch_unwind(run)
                        .unwrap_or_else(|_| Err(CredentialError::Platform("the credential store failed".to_string())));
                    // The store answered for this name without the person
                    // (or after they allowed it once): later reads will not
                    // prompt.
                    if outcome.is_ok() && !matches!(job.op, Op::Delete) {
                        if let Ok(mut names) = answered.lock() {
                            names.insert(key.as_str().to_string());
                        }
                    }
                    // A caller that has gone away no longer wants the result.
                    let _ = job.reply.send(outcome);
                }
            });
        // If the thread could not start, its receiver went with the closure,
        // so every send fails and `submit` tells the caller the queue stopped.
        drop(spawned);
        Lane { tx, next: 0, started }
    }
}

fn queues() -> &'static Queues {
    static QUEUES: OnceLock<Queues> = OnceLock::new();
    QUEUES.get_or_init(|| Queues::new(Arc::new(Store::native), Op::timeout))
}

/// Queue `op` on `name` and return at once. Calls on one name run strictly in
/// submission order; see the section above.
pub fn submit(name: &CredentialName, op: Op) -> Ticket {
    queues().submit(name, op)
}

/// Whether this process has already read or written `name` successfully, so
/// reading it again cannot raise a store prompt (the macOS Keychain asks a
/// rebuilt Studio before its first read of each key). A check the person did
/// not ask to touch the store for (the publish check) reads only these names
/// and reports any other as "could not be checked": no prompt may appear from
/// pressing Publish.
pub fn read_this_session(name: &CredentialName) -> bool {
    queues().answered(name)
}

impl Queues {
    fn answered(&self, name: &CredentialName) -> bool {
        self.answered.lock().is_ok_and(|names| names.contains(name.as_str()))
    }
}

/// One credential store, over a `keyring` backend.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
struct Store {
    builder: Box<CredentialBuilder>,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl Store {
    /// This platform's store.
    fn native() -> Result<Self, CredentialError> {
        Self::over(native_builder()?)
    }

    /// A store over `builder`, refused unless what it stores lasts until it
    /// is deleted.
    fn over(builder: Box<CredentialBuilder>) -> Result<Self, CredentialError> {
        if !matches!(builder.persistence(), CredentialPersistence::UntilDelete) {
            return Err(CredentialError::Unavailable(
                "this build has no lasting credential store for this platform".to_string(),
            ));
        }
        Ok(Self { builder })
    }

    fn entry(&self, name: &CredentialName) -> Result<keyring::Entry, CredentialError> {
        let credential = self.builder.build(None, name.as_str(), ACCOUNT).map_err(|e| from_keyring(e, name))?;
        Ok(keyring::Entry::new_with_credential(credential))
    }

    fn set(&self, name: &CredentialName, secret: &str) -> Result<(), CredentialError> {
        self.entry(name)?.set_password(secret).map_err(|e| from_keyring(e, name))
    }

    fn set_verified(&self, name: &CredentialName, secret: &str) -> Result<(), CredentialError> {
        self.set(name, secret)?;
        match self.get(name)? {
            Some(stored) if stored == secret => Ok(()),
            Some(_) => {
                // The store holds something other than what was meant: remove
                // it, so no later read can return it in place of the caller's
                // original copy.
                let _ = self.delete(name);
                Err(CredentialError::Platform(format!(
                    "{name} read back different from what was stored; keep the original copy"
                )))
            }
            None => Err(CredentialError::Platform(format!(
                "{name} was not found after it was stored; keep the original copy"
            ))),
        }
    }

    fn get(&self, name: &CredentialName) -> Result<Option<String>, CredentialError> {
        match self.entry(name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(from_keyring(e, name)),
        }
    }

    fn delete(&self, name: &CredentialName) -> Result<bool, CredentialError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(from_keyring(e, name)),
        }
    }
}

/// This platform's backend.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn native_builder() -> Result<Box<CredentialBuilder>, CredentialError> {
    #[cfg(target_os = "windows")]
    return Ok(keyring::windows::default_credential_builder());
    #[cfg(target_os = "macos")]
    return Ok(keyring::macos::default_credential_builder());
    #[cfg(target_os = "linux")]
    return Ok(keyring::secret_service::default_credential_builder());
}

/// Without a supported store (the web build, Android) there is no backend
/// and no `keyring`: every call is [`CredentialError::Unavailable`]. The type
/// has no values, so nothing past [`Store::native`] can run.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
enum Store {}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
impl Store {
    fn native() -> Result<Self, CredentialError> {
        Err(CredentialError::Unavailable("this platform has no supported credential store".to_string()))
    }
    fn set(&self, _name: &CredentialName, _secret: &str) -> Result<(), CredentialError> {
        match *self {}
    }
    fn set_verified(&self, _name: &CredentialName, _secret: &str) -> Result<(), CredentialError> {
        match *self {}
    }
    fn get(&self, _name: &CredentialName) -> Result<Option<String>, CredentialError> {
        match *self {}
    }
    fn delete(&self, _name: &CredentialName) -> Result<bool, CredentialError> {
        match *self {}
    }
}

/// A `keyring` error as a [`CredentialError`], without the secret: a
/// `BadEncoding` error carries the stored bytes, so its text is written here.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn from_keyring(e: keyring::Error, name: &CredentialName) -> CredentialError {
    match e {
        keyring::Error::NoStorageAccess(why) => CredentialError::Unavailable(why.to_string()),
        keyring::Error::PlatformFailure(why) => CredentialError::Platform(why.to_string()),
        keyring::Error::BadEncoding(_) => CredentialError::Platform(format!("the value stored for {name} is not text")),
        keyring::Error::TooLong(field, limit) => {
            CredentialError::Platform(format!("{field} is longer than this store allows ({limit})"))
        }
        keyring::Error::Invalid(field, why) => CredentialError::Platform(format!("{field}: {why}")),
        keyring::Error::Ambiguous(_) => CredentialError::Platform(format!("more than one entry matches {name}")),
        keyring::Error::NoEntry => CredentialError::Platform(format!("no entry for {name}")),
        other => CredentialError::Platform(format!("{other}")),
    }
}

#[cfg(all(test, any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// A store kept in a map shared by every entry it builds, so a value set
    /// through one entry is read back through another, as a real store does.
    #[derive(Default, Clone)]
    struct Shared(Arc<Mutex<HashMap<(String, String), Vec<u8>>>>);

    struct SharedCredential {
        key: (String, String),
        map: Shared,
    }

    impl CredentialApi for SharedCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            self.map.0.lock().unwrap().insert(self.key.clone(), secret.to_vec());
            Ok(())
        }
        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            self.map.0.lock().unwrap().get(&self.key).cloned().ok_or(keyring::Error::NoEntry)
        }
        fn delete_credential(&self) -> keyring::Result<()> {
            self.map.0.lock().unwrap().remove(&self.key).map(|_| ()).ok_or(keyring::Error::NoEntry)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl CredentialBuilderApi for Shared {
        fn build(&self, _target: Option<&str>, service: &str, user: &str) -> keyring::Result<Box<Credential>> {
            Ok(Box::new(SharedCredential { key: (service.to_string(), user.to_string()), map: self.clone() }))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// A store that cannot be reached, as a locked keychain or a Linux
    /// machine with no Secret Service running answers.
    struct Locked;

    impl CredentialBuilderApi for Locked {
        fn build(&self, _target: Option<&str>, _service: &str, _user: &str) -> keyring::Result<Box<Credential>> {
            Err(keyring::Error::NoStorageAccess("the keyring is locked".into()))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn name(provider: &str, purpose: &str) -> CredentialName {
        CredentialName::new(provider, purpose).unwrap()
    }

    #[test]
    fn a_secret_round_trips_through_separate_entries() {
        let map = Shared::default();
        let store = Store::over(Box::new(map.clone())).unwrap();
        let key = name("anthropic", "api_key");
        assert_eq!(store.get(&key).unwrap(), None, "nothing stored yet");
        store.set(&key, "sk-test-1").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("sk-test-1"));
        store.set(&key, "sk-test-2").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("sk-test-2"), "set replaces");
        assert!(store.delete(&key).unwrap());
        assert_eq!(store.get(&key).unwrap(), None, "gone after delete");
        assert!(!store.delete(&key).unwrap(), "a second delete finds nothing");
        assert!(map.0.lock().unwrap().is_empty());
    }

    #[test]
    fn names_are_namespaced_and_kept_apart() {
        let map = Shared::default();
        let store = Store::over(Box::new(map.clone())).unwrap();
        store.set(&name("anthropic", "api_key"), "a").unwrap();
        store.set(&name("lob", "api_key"), "b").unwrap();
        store.set(&name("anthropic", "org_id"), "c").unwrap();
        assert_eq!(store.get(&name("anthropic", "api_key")).unwrap().as_deref(), Some("a"));
        assert_eq!(store.get(&name("lob", "api_key")).unwrap().as_deref(), Some("b"));
        assert_eq!(store.get(&name("anthropic", "org_id")).unwrap().as_deref(), Some("c"));
        let mut services: Vec<(String, String)> = map.0.lock().unwrap().keys().cloned().collect();
        services.sort();
        assert_eq!(
            services,
            vec![
                ("eustress/anthropic/api_key".to_string(), ACCOUNT.to_string()),
                ("eustress/anthropic/org_id".to_string(), ACCOUNT.to_string()),
                ("eustress/lob/api_key".to_string(), ACCOUNT.to_string()),
            ]
        );
    }

    #[test]
    fn only_plain_names_are_accepted() {
        for (provider, purpose) in [
            ("", "api_key"),
            ("anthropic", ""),
            ("Anthropic", "api_key"),
            ("anthropic", "api key"),
            ("anthropic/x", "api_key"),
            ("anthropic", "../api_key"),
            ("anthropic", "api.key"),
        ] {
            let err = CredentialName::new(provider, purpose).unwrap_err();
            assert!(matches!(err, CredentialError::InvalidName(_)), "{provider}/{purpose}: {err}");
        }
        assert!(CredentialName::new(&"a".repeat(65), "x").is_err());
        assert_eq!(name("ses", "credentials").as_str(), "eustress/ses/credentials");
        assert_eq!(name(&"a".repeat(64), "x-1_2").as_str(), format!("eustress/{}/x-1_2", "a".repeat(64)));
    }

    #[test]
    fn an_unreachable_store_is_an_error_a_caller_can_show() {
        let store = Store::over(Box::new(Locked)).unwrap();
        let key = name("anthropic", "api_key");
        for err in [
            store.set(&key, "sk-test").unwrap_err(),
            store.get(&key).unwrap_err(),
            store.delete(&key).unwrap_err(),
        ] {
            assert!(matches!(err, CredentialError::Unavailable(_)), "{err:?}");
            let shown = err.to_string();
            assert!(shown.contains("unavailable") && shown.contains("locked"), "{shown}");
            assert!(!shown.contains("sk-test"), "never the secret: {shown}");
        }
    }

    /// keyring's in-memory mock, which it uses on a platform whose backend
    /// feature is off, loses everything at exit: refused, never used.
    #[test]
    fn a_store_that_forgets_is_refused() {
        let err = Store::over(keyring::mock::default_credential_builder()).err().expect("the mock must be refused");
        assert!(matches!(err, CredentialError::Unavailable(_)), "{err:?}");
    }

    /// The backend this platform uses is its real store, never keyring's
    /// mock, whatever features the build was given.
    #[test]
    fn the_native_store_is_never_the_mock() {
        let builder = native_builder().unwrap();
        assert!(matches!(builder.persistence(), CredentialPersistence::UntilDelete));
        assert!(builder.as_any().downcast_ref::<keyring::mock::MockCredentialBuilder>().is_none(), "the mock");
        #[cfg(target_os = "windows")]
        assert!(
            builder.as_any().downcast_ref::<keyring::windows::WinCredentialBuilder>().is_some(),
            "Windows uses Credential Manager"
        );
        assert!(Store::native().is_ok());
    }

    /// A store that keeps something other than what it was given.
    struct Altering(Shared);

    impl CredentialBuilderApi for Altering {
        fn build(&self, target: Option<&str>, service: &str, user: &str) -> keyring::Result<Box<Credential>> {
            let inner = self.0.build(target, service, user)?;
            Ok(Box::new(AlteringCredential(inner)))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct AlteringCredential(Box<Credential>);

    impl CredentialApi for AlteringCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            self.0.set_secret(&secret[..secret.len().saturating_sub(1)])
        }
        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            self.0.get_secret()
        }
        fn delete_credential(&self) -> keyring::Result<()> {
            self.0.delete_credential()
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn set_verified_succeeds_only_when_the_store_holds_exactly_the_secret() {
        let key = name("forge", "api_key");
        let good = Store::over(Box::new(Shared::default())).unwrap();
        good.set_verified(&key, "fk-test").unwrap();
        assert_eq!(good.get(&key).unwrap().as_deref(), Some("fk-test"));

        let altering = Store::over(Box::new(Altering(Shared::default()))).unwrap();
        let err = altering.set_verified(&key, "fk-test").unwrap_err();
        assert!(err.to_string().contains("keep the original copy"), "{err}");
        assert!(!err.to_string().contains("fk-test"), "never the secret: {err}");
        assert_eq!(altering.get(&key).unwrap(), None, "the wrong value is not left to be read");

        let locked = Store::over(Box::new(Locked)).unwrap();
        assert!(matches!(locked.set_verified(&key, "fk-test").unwrap_err(), CredentialError::Unavailable(_)));
    }

    /// A store over a shared map in which writing `held` waits until the
    /// test opens the gate: a slow or hung write.
    #[derive(Clone)]
    struct Gated {
        map: Shared,
        held: String,
        open: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }

    impl Gated {
        fn new(held: &str) -> Self {
            Self { map: Shared::default(), held: held.to_string(), open: Arc::new((Mutex::new(false), Default::default())) }
        }
        fn release(&self) {
            *self.open.0.lock().unwrap() = true;
            self.open.1.notify_all();
        }
        fn queues(&self) -> Queues {
            let me = self.clone();
            Queues::new(Arc::new(move || Store::over(Box::new(me.clone()))), |_| Duration::from_millis(200))
        }
        fn stored(&self, name: &CredentialName) -> Option<String> {
            let map = self.map.0.lock().unwrap();
            map.get(&(name.as_str().to_string(), ACCOUNT.to_string())).map(|b| String::from_utf8(b.clone()).unwrap())
        }
    }

    struct GatedCredential {
        inner: Box<Credential>,
        gate: Gated,
    }

    impl CredentialApi for GatedCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            if secret == self.gate.held.as_bytes() {
                let (lock, cvar) = &*self.gate.open;
                let _open = cvar.wait_while(lock.lock().unwrap(), |open| !*open).unwrap();
            }
            self.inner.set_secret(secret)
        }
        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            self.inner.get_secret()
        }
        fn delete_credential(&self) -> keyring::Result<()> {
            self.inner.delete_credential()
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl CredentialBuilderApi for Gated {
        fn build(&self, target: Option<&str>, service: &str, user: &str) -> keyring::Result<Box<Credential>> {
            Ok(Box::new(GatedCredential { inner: self.map.build(target, service, user)?, gate: self.clone() }))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Poll a ticket until its (possibly late) result arrives.
    fn eventually(ticket: &Ticket) -> Outcome {
        for _ in 0..500 {
            if let Some(done) = ticket.try_result() {
                return done;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no result arrived");
    }

    #[test]
    fn a_slow_write_never_lands_after_a_newer_save() {
        let gate = Gated::new("old-key");
        let queues = gate.queues();
        let key = name("anthropic", "api_key");
        let first = queues.submit(&key, Op::SetVerified("old-key".into()));
        let second = queues.submit(&key, Op::SetVerified("new-key".into()));
        assert!(second.queued_behind(), "the newer save waits for the older one");
        assert!(
            matches!(second.wait(), Err(CredentialError::TimedOut { queued_behind: true, .. })),
            "it cannot finish while the older one is held"
        );
        gate.release();
        assert_eq!(eventually(&first), Ok(Done::Stored));
        assert_eq!(eventually(&second), Ok(Done::Stored));
        assert_eq!(gate.stored(&key).as_deref(), Some("new-key"), "the newer save is what remains");
    }

    #[test]
    fn a_slow_write_never_lands_after_a_remove() {
        let gate = Gated::new("old-key");
        let queues = gate.queues();
        let key = name("lob", "api_key");
        let write = queues.submit(&key, Op::Set("old-key".into()));
        let remove = queues.submit(&key, Op::Delete);
        assert!(
            matches!(remove.wait(), Err(CredentialError::TimedOut { queued_behind: true, .. })),
            "the remove cannot run while the write is held"
        );
        gate.release();
        assert_eq!(eventually(&write), Ok(Done::Stored));
        assert_eq!(eventually(&remove), Ok(Done::Deleted(true)));
        assert_eq!(gate.stored(&key), None, "removed, and nothing put back");
    }

    /// A store that does not answer: the caller is told after the limit, the
    /// call keeps its place, later calls stay queued behind it in order, and
    /// every result still arrives once the store answers.
    #[test]
    fn a_hung_store_times_out_the_wait_but_keeps_the_order() {
        let gate = Gated::new("hung-key");
        let queues = gate.queues();
        let key = name("ses", "credentials");
        let hung = queues.submit(&key, Op::Set("hung-key".into()));
        match hung.wait() {
            Err(CredentialError::TimedOut { queued_behind: false, .. }) => {}
            other => panic!("expected a timeout, got {other:?}"),
        }
        assert!(hung.overdue());
        let read = queues.submit(&key, Op::Get);
        assert!(read.queued_behind());
        match read.wait() {
            Err(e @ CredentialError::TimedOut { queued_behind: true, .. }) => {
                assert!(e.to_string().contains("still waiting for the system credential store"), "{e}")
            }
            other => panic!("expected a queued timeout, got {other:?}"),
        }
        gate.release();
        assert_eq!(eventually(&hung), Ok(Done::Stored), "the timed-out write still completed");
        assert_eq!(eventually(&read), Ok(Done::Got(Some("hung-key".into()))), "and the read after it saw it");
        assert!(!read.queued_behind());
    }

    #[test]
    fn calls_on_different_names_do_not_wait_for_each_other() {
        let gate = Gated::new("hung-key");
        let queues = gate.queues();
        let hung = queues.submit(&name("anthropic", "api_key"), Op::Set("hung-key".into()));
        let other = queues.submit(&name("lob", "api_key"), Op::Set("lob-key".into()));
        assert_eq!(other.wait(), Ok(Done::Stored));
        gate.release();
        assert_eq!(eventually(&hung), Ok(Done::Stored));
    }

    /// A backend that panics once: that call fails, and the lane keeps
    /// serving the calls after it.
    #[derive(Clone, Default)]
    struct PanicsOnce {
        map: Shared,
        panicked: Arc<std::sync::atomic::AtomicBool>,
    }

    impl CredentialBuilderApi for PanicsOnce {
        fn build(&self, target: Option<&str>, service: &str, user: &str) -> keyring::Result<Box<Credential>> {
            if !self.panicked.swap(true, Ordering::SeqCst) {
                panic!("a backend bug");
            }
            self.map.build(target, service, user)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn a_backend_that_panics_fails_one_call_and_the_queue_lives_on() {
        let backend = PanicsOnce::default();
        let factory = backend.clone();
        let queues = Queues::new(Arc::new(move || Store::over(Box::new(factory.clone()))), |_| Duration::from_secs(5));
        let key = name("forge", "api_key");
        let first = queues.submit(&key, Op::Set("fk-1".into()));
        assert_eq!(first.wait(), Err(CredentialError::Platform("the credential store failed".to_string())));
        let second = queues.submit(&key, Op::SetVerified("fk-2".into()));
        assert_eq!(second.wait(), Ok(Done::Stored), "the same name still works");
        assert_eq!(queues.submit(&key, Op::Get).wait(), Ok(Done::Got(Some("fk-2".into()))));
    }

    #[test]
    fn a_caller_can_only_shorten_its_wait_and_the_timeout_names_the_limit_used() {
        let gate = Gated::new("hung-key");
        let queues = gate.queues(); // 200 ms per call here
        let key = name("anthropic", "api_key");
        let hung = queues.submit(&key, Op::Set("hung-key".into()));
        let short = Duration::from_millis(50);
        assert_eq!(
            hung.wait_up_to(short),
            Err(CredentialError::TimedOut { waited: short, queued_behind: false }),
            "the caller's shorter limit is the one used"
        );
        let read = queues.submit(&key, Op::Get);
        match read.wait_up_to(Duration::from_secs(60)) {
            Err(CredentialError::TimedOut { waited, .. }) => {
                assert_eq!(waited, Duration::from_millis(200), "never longer than the call's own limit")
            }
            other => panic!("expected a timeout, got {other:?}"),
        }
        gate.release();
        assert_eq!(eventually(&hung), Ok(Done::Stored), "the shortened call still completed");
        assert_eq!(eventually(&read), Ok(Done::Got(Some("hung-key".into()))));
    }

    /// Windows waits 5 s for any call; macOS and Linux wait 30 s for any
    /// call, a read included, since either store can show a prompt.
    #[test]
    fn the_timeout_policy_is_five_seconds_on_windows_and_thirty_on_macos_and_linux() {
        for op in [Op::Get, Op::Set(String::new()), Op::SetVerified(String::new()), Op::Delete] {
            for platform in [Platform::Windows, Platform::Other] {
                assert_eq!(timeout_for(&op, platform), TIMEOUT, "{op:?} {platform:?}");
            }
            for platform in [Platform::MacOs, Platform::Linux] {
                assert_eq!(timeout_for(&op, platform), SLOW_TIMEOUT, "{op:?} {platform:?}");
            }
        }
        assert_eq!((TIMEOUT, SLOW_TIMEOUT), (Duration::from_secs(5), Duration::from_secs(30)));
    }

    /// Startup on macOS or Linux: a verified write may take 30 s, and the
    /// startup migration waits 5, under each policy, run here on any machine.
    #[test]
    fn a_startup_migration_waits_five_seconds_not_thirty() {
        startup_migration_waits_five_seconds(|op| timeout_for(op, Platform::MacOs));
        startup_migration_waits_five_seconds(|op| timeout_for(op, Platform::Linux));
    }

    fn startup_migration_waits_five_seconds(policy: fn(&Op) -> Duration) {
        let gate = Gated::new("startup-key");
        let me = gate.clone();
        let queues = Queues::new(Arc::new(move || Store::over(Box::new(me.clone()))), policy);
        let ticket = queues.submit(&name("anthropic", "api_key"), Op::SetVerified("startup-key".into()));
        assert_eq!(ticket.timeout(), SLOW_TIMEOUT, "a person's save may wait 30 s");
        assert_eq!(
            ticket.wait_up_to(TIMEOUT),
            Err(CredentialError::TimedOut { waited: TIMEOUT, queued_behind: false }),
            "startup waits 5"
        );
        gate.release();
        assert_eq!(eventually(&ticket), Ok(Done::Stored));
    }

    /// A name counts as read this session only after a get or set on it
    /// succeeded; a failed call or another name does not count.
    #[test]
    fn only_a_name_read_successfully_counts_as_read_this_session() {
        let map = Shared::default();
        let me = map.clone();
        let queues = Queues::new(Arc::new(move || Store::over(Box::new(me.clone()))), |_| Duration::from_secs(5));
        let soul = name("anthropic", "api_key");
        let other = name("openai", "api_key");
        assert!(!queues.answered(&soul), "nothing read yet");
        assert_eq!(eventually(&queues.submit(&soul, Op::Get)), Ok(Done::Got(None)));
        assert!(queues.answered(&soul), "a successful read, even of nothing");
        assert!(!queues.answered(&other), "another name");
        let locked = Queues::new(Arc::new(|| Store::over(Box::new(Locked))), |_| Duration::from_secs(5));
        assert!(eventually(&locked.submit(&other, Op::Get)).is_err());
        assert!(!locked.answered(&other), "a failed read does not count");
    }

    /// A frame thread polling a ticket that a worker thread is waiting on
    /// returns at once.
    #[test]
    fn polling_never_blocks_on_another_threads_wait() {
        let gate = Gated::new("hung-key");
        let me = gate.clone();
        let queues = Queues::new(Arc::new(move || Store::over(Box::new(me.clone()))), |_| Duration::from_secs(30));
        let ticket = queues.submit(&name("lob", "api_key"), Op::Set("hung-key".into()));
        std::thread::scope(|s| {
            s.spawn(|| ticket.wait());
            std::thread::sleep(Duration::from_millis(100)); // the waiter is in recv
            let (done_tx, done_rx) = mpsc::channel();
            let polled_ticket = &ticket;
            s.spawn(move || {
                let polled = polled_ticket.try_result();
                let _ = done_tx.send(polled.is_none());
            });
            let polled_none = done_rx.recv_timeout(Duration::from_secs(5));
            gate.release();
            assert_eq!(polled_none, Ok(true), "try_result returned, with no result yet, while the wait was on");
        });
    }

    /// How long a waiter pauses between receiving a result and caching it,
    /// so a test can hold that gap open. Zero except in the test below.
    static PAUSE_AFTER_RECEIVE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    pub(super) fn pause_after_receive() {
        let ms = PAUSE_AFTER_RECEIVE_MS.load(Ordering::SeqCst);
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms));
        }
    }

    /// A waiter and a poller on one ticket agree: the poller never reads
    /// "stopped" for a call that succeeded, even with the gap between the
    /// waiter's receive and its cache held open.
    #[test]
    fn a_waiter_and_a_poller_always_see_the_same_result() {
        let key = name("lob", "api_key");
        PAUSE_AFTER_RECEIVE_MS.store(30, Ordering::SeqCst);
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                PAUSE_AFTER_RECEIVE_MS.store(0, Ordering::SeqCst);
            }
        }
        let _reset = Reset;
        for round in 0..20 {
            // The write is held until the waiter is inside its receive, so the
            // result reaches the waiter, not an early poll.
            let gate = Gated::new("slow");
            let me = gate.clone();
            let queues = Queues::new(Arc::new(move || Store::over(Box::new(me.clone()))), |_| Duration::from_secs(5));
            let ticket = queues.submit(&key, Op::Set("slow".into()));
            let (waited, polled) = std::thread::scope(|s| {
                let waiter = s.spawn(|| ticket.wait());
                std::thread::sleep(Duration::from_millis(20));
                let poller = s.spawn(|| loop {
                    if let Some(done) = ticket.try_result() {
                        return done;
                    }
                    std::hint::spin_loop();
                });
                gate.release();
                (waiter.join().unwrap(), poller.join().unwrap())
            });
            assert_eq!(waited, Ok(Done::Stored), "round {round}");
            assert_eq!(polled, Ok(Done::Stored), "round {round}: the poller saw {polled:?}");
        }
    }

    #[test]
    fn nothing_queued_shows_a_secret_in_debug_output() {
        let shown = format!("{:?} {:?}", Op::SetVerified("sk-secret".into()), Done::Got(Some("sk-secret".into())));
        assert!(!shown.contains("sk-secret"), "{shown}");
    }

    #[test]
    fn a_stored_value_that_is_not_text_is_reported_without_its_bytes() {
        let map = Shared::default();
        let store = Store::over(Box::new(map.clone())).unwrap();
        let key = name("anthropic", "api_key");
        map.0.lock().unwrap().insert((key.as_str().to_string(), ACCOUNT.to_string()), vec![0xff, 0xfe, b's', b'k']);
        let shown = store.get(&key).unwrap_err().to_string();
        assert!(shown.contains("not text"), "{shown}");
        assert!(!shown.contains("255") && !shown.contains("sk"), "never the stored bytes: {shown}");
    }

    /// The real store on this machine, set and read and deleted under a name
    /// no one uses. It writes to the person's own credential store, so it runs
    /// only when asked: `--ignored`.
    #[test]
    #[ignore]
    fn the_native_store_round_trips() {
        let key = CredentialName::new("test", &format!("roundtrip-{}", std::process::id())).unwrap();
        let _ = delete(&key);
        set(&key, "not-a-real-key").unwrap();
        assert_eq!(get(&key).unwrap().as_deref(), Some("not-a-real-key"));
        assert!(delete(&key).unwrap());
        assert_eq!(get(&key).unwrap(), None);
    }
}
