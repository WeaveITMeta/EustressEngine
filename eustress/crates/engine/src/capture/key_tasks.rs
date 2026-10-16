//! Credential work for the UI, off the frame thread.
//!
//! A credential call waits on the system store (up to 5 s on Windows; 30 s on
//! macOS and Linux, where the store may be showing the person a prompt), so no
//! frame system makes one. A system hands a [`KeyRequest`] to [`KeyTasks`] and
//! returns at once; one worker thread runs the requests in the order they
//! came, so a Save followed by a Remove never runs the other way round, and
//! sends each [`KeyResult`] back on a channel that `apply_key_results` (in
//! `ui::slint_ui`) drains every frame and shows. This is the same shape as the
//! Workshop's Soul key work: a background thread, a channel, a polled system.
//!
//! The Forge dialog never holds the real key. When a key is remembered, the
//! dialog shows [`FORGE_KEY_MASK`] and the key stays here, in
//! [`ForgeRememberedKey`]; Connect uses it while the field still shows the mask
//! ([`connect_key`]), and only for the origin (scheme, host, port) it was
//! saved for: a key is never sent to a server it was not given for.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use bevy::prelude::Resource;

use eustress_forge_sdk::connect_policy::{origin_of, Origin};

use super::credentials::{self, KeyStatus, ReadError};

/// The remembered Forge connection's credential names (an environment
/// variable of the same name also works for neither: only SAM keys read the
/// environment).
pub const FORGE_URL_CREDENTIAL: &str = "EUSTRESS_FORGE_URL";
/// See [`FORGE_URL_CREDENTIAL`].
pub const FORGE_KEY_CREDENTIAL: &str = "EUSTRESS_FORGE_API_KEY";

/// The URL the Forge dialog's field starts with (`forge-server-url` in
/// main.slint, `server-url` in forge_connect.slint). A saved URL replaces it:
/// the field holding it is a field nobody has typed in.
pub const FORGE_DEFAULT_URL: &str = "https://forge.eustress.dev";

/// What the Forge dialog's key field shows while a remembered key is in use.
pub const FORGE_KEY_MASK: &str = "••••••••••••";

/// Credential work a UI system asks for.
pub enum KeyRequest {
    /// The remembered Forge URL and key, read once at startup. `generation`
    /// is [`KeyTasks`]'s Forge generation when it was asked: see
    /// [`KeyTasks::apply_forge_prefill`].
    ForgePrefill { generation: u64 },
    /// Connect's "Remember credentials": keep the URL and the key in hand
    /// together (no key: none is kept for this URL); or forget both.
    ForgeRemember { remember: bool, url: String, key: Option<String> },
    /// The capture key dialog's status line for `name`.
    Status { name: String },
    /// The capture key dialog's Save.
    Save { name: String, value: String },
    /// The capture key dialog's Remove.
    Remove { name: String },
}

impl std::fmt::Debug for KeyRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyRequest::ForgePrefill { generation } => write!(f, "ForgePrefill({generation})"),
            KeyRequest::ForgeRemember { remember, key, .. } => {
                write!(f, "ForgeRemember {{ remember: {remember}, key: {} }}", if key.is_some() { "<key>" } else { "none" })
            }
            KeyRequest::Status { name } => write!(f, "Status({name})"),
            KeyRequest::Save { name, .. } => write!(f, "Save({name}, <key>)"),
            KeyRequest::Remove { name } => write!(f, "Remove({name})"),
        }
    }
}

/// What a request found or did.
pub enum KeyResult {
    /// The remembered Forge URL and key. An error is a read that failed (a
    /// fallback file is damaged, or the store could not be asked), never any
    /// part of a key: a key may be saved, and must not be taken for none.
    ForgePrefill { url: Result<Option<String>, ReadError>, key: Result<Option<String>, ReadError>, generation: u64 },
    /// What a Remember could not do, one line each; empty when it all worked.
    ForgeRemembered { problems: Vec<String> },
    Status { name: String, status: KeyStatus },
    Saved { name: String, result: Result<KeyStatus, String> },
    Removed { name: String, result: Result<bool, String> },
}

impl std::fmt::Debug for KeyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyResult::ForgePrefill { url, key, generation } => write!(
                f,
                "ForgePrefill {{ url: {url:?}, key: {}, generation: {generation} }}",
                match key {
                    Ok(Some(_)) => "<key>".to_string(),
                    Ok(None) => "none".to_string(),
                    Err(e) => format!("unreadable ({e})"),
                }
            ),
            KeyResult::ForgeRemembered { problems } => write!(f, "ForgeRemembered({problems:?})"),
            KeyResult::Status { name, status } => write!(f, "Status({name}, {status:?})"),
            KeyResult::Saved { name, result } => write!(f, "Saved({name}, {result:?})"),
            KeyResult::Removed { name, result } => write!(f, "Removed({name}, {result:?})"),
        }
    }
}

/// Do one request, here, on the calling thread: only ever the worker's.
pub fn run(request: KeyRequest) -> KeyResult {
    match request {
        KeyRequest::ForgePrefill { generation } => KeyResult::ForgePrefill {
            url: credentials::read_saved(FORGE_URL_CREDENTIAL),
            key: credentials::read_saved(FORGE_KEY_CREDENTIAL),
            generation,
        },
        KeyRequest::ForgeRemember { remember, url, key } => {
            let mut problems = remember_with(
                remember,
                &url,
                key.as_deref(),
                &mut |name, value| credentials::set(name, value).map_err(|e| e.to_string()),
                &mut |name| credentials::remove(name).map(drop).map_err(|e| e.to_string()),
            );
            // Saved, but maybe not where it is protected: say so.
            if remember && key.is_some() && problems.is_empty() {
                let status = credentials::status(FORGE_KEY_CREDENTIAL);
                if status.outside_store {
                    problems.push(status.summary());
                }
            }
            KeyResult::ForgeRemembered { problems }
        }
        KeyRequest::Status { name } => {
            let status = credentials::status(&name);
            KeyResult::Status { name, status }
        }
        KeyRequest::Save { name, value } => {
            let result = credentials::set(&name, &value).map(|()| credentials::status(&name)).map_err(|e| e.to_string());
            KeyResult::Saved { name, result }
        }
        KeyRequest::Remove { name } => {
            let result = credentials::remove(&name).map_err(|e| e.to_string());
            KeyResult::Removed { name, result }
        }
    }
}

/// Remember (or forget) the Forge URL and key with `set` and `remove`, and
/// say what could not be done. The saved key is removed first, then the URL
/// written, then the key: a stop at any point leaves no key paired with a
/// URL it was not saved for (the pair is what binds a key to its origin).
fn remember_with(
    remember: bool,
    url: &str,
    key: Option<&str>,
    set: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    remove: &mut dyn FnMut(&str) -> Result<(), String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    if !remember {
        for name in [FORGE_KEY_CREDENTIAL, FORGE_URL_CREDENTIAL] {
            if let Err(e) = remove(name) {
                problems.push(format!("{name} could not be forgotten: {e}"));
            }
        }
        return problems;
    }
    // An empty URL field is not a URL to remember, and a key needs one.
    if url.trim().is_empty() {
        return problems;
    }
    if let Err(e) = remove(FORGE_KEY_CREDENTIAL) {
        problems.push(format!("the Forge URL and key were not remembered: the old key could not be cleared: {e}"));
        return problems;
    }
    if let Err(e) = set(FORGE_URL_CREDENTIAL, url) {
        problems.push(format!("the Forge URL and key were not remembered: {e}"));
        return problems;
    }
    if let Some(key) = key {
        if let Err(e) = set(FORGE_KEY_CREDENTIAL, key) {
            problems.push(format!("the Forge key was not remembered: {e}"));
        }
    }
    problems
}

type Runner = Arc<dyn Fn(KeyRequest) -> KeyResult + Send + Sync>;

/// The credential worker: requests in, results out.
#[derive(Resource)]
pub struct KeyTasks {
    requests: Mutex<mpsc::Sender<KeyRequest>>,
    results: Mutex<mpsc::Receiver<KeyResult>>,
    /// Bumped by every Forge forget, and every Remember that carries a key:
    /// a prefill asked before either is out of date by the time it arrives.
    forge_generation: AtomicU64,
    /// The startup prefill has been asked for and its result not yet
    /// applied: a Connect with no key must wait for it.
    prefill_pending: AtomicBool,
}

impl Default for KeyTasks {
    fn default() -> Self {
        Self::with_runner(Arc::new(run))
    }
}

impl KeyTasks {
    /// A worker running `runner` on its own thread, one request at a time, in
    /// the order they were submitted.
    pub(crate) fn with_runner(runner: Runner) -> Self {
        let (requests, inbox) = mpsc::channel::<KeyRequest>();
        let (outbox, results) = mpsc::channel::<KeyResult>();
        let spawned = spawn_worker(move || {
            for request in inbox {
                if outbox.send(runner(request)).is_err() {
                    break;
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("credential worker could not start ({e}); saved keys cannot be read or saved this session");
        }
        Self { requests: Mutex::new(requests), results: Mutex::new(results), forge_generation: AtomicU64::new(0), prefill_pending: AtomicBool::new(false) }
    }

    /// Queue `request` for the worker and return at once. `false` when the
    /// worker is not running and the request was dropped (logged).
    pub fn submit(&self, request: KeyRequest) -> bool {
        // A forget makes an earlier prefill stale (applying it would bring
        // back what the person asked to forget), and so does a Remember with
        // a key (applied into a field emptied since, it would bring back the
        // older key). A prefill fills only empty fields, so after any other
        // Connect it changes nothing typed.
        if matches!(
            request,
            KeyRequest::ForgeRemember { remember: false, .. } | KeyRequest::ForgeRemember { key: Some(_), .. }
        ) {
            self.forge_generation.fetch_add(1, Ordering::SeqCst);
        }
        let sent = match self.requests.lock() {
            Ok(tx) => tx.send(request).is_ok(),
            Err(_) => false,
        };
        if !sent {
            tracing::warn!("credential worker is not running; the request was dropped");
        }
        sent
    }

    /// Ask for the remembered Forge URL and key, as of now.
    pub fn submit_forge_prefill(&self) {
        // Set before the send, so a result applied at once cannot be
        // followed by a stale "pending"; cleared again if nothing was sent,
        // or Connect would wait for a read that never comes.
        self.prefill_pending.store(true, Ordering::SeqCst);
        let generation = self.forge_generation.load(Ordering::SeqCst);
        if !self.submit(KeyRequest::ForgePrefill { generation }) {
            self.prefill_pending.store(false, Ordering::SeqCst);
        }
    }

    /// Whether a prefill asked at `generation` is still current: no Forge
    /// forget, and no Remember with a key, has been submitted since.
    pub fn prefill_is_current(&self, generation: u64) -> bool {
        self.forge_generation.load(Ordering::SeqCst) == generation
    }

    /// What a prefill result fills in, given what the dialog's fields hold
    /// now. A prefill that is out of date fills nothing: the person has
    /// Connected since, and what they chose (forget, or a new key) stands. A
    /// current one fills only fields that are still empty (for the URL:
    /// empty or still [`FORGE_DEFAULT_URL`]), keeping what the person typed;
    /// the key goes into `kept`, bound to the saved URL's origin, and the
    /// field gets the mask. A key with no saved URL (or one that fails the
    /// connection policy) is not kept: it has no origin.
    ///
    /// A key that may be saved but is not kept (it could not be read, the
    /// store could not be asked, it has no origin, or the person typed over
    /// the field first) is marked in `kept`, so a Connect with the field
    /// empty does not remove it: see [`ForgeRememberedKey::connect_request`].
    /// When the field is empty the fill says why: `problem` when a key is
    /// surely saved, `note` when the store could not be asked and none may be.
    pub fn apply_forge_prefill(
        &self,
        kept: &mut ForgeRememberedKey,
        generation: u64,
        url: Result<Option<String>, ReadError>,
        key: Result<Option<String>, ReadError>,
        url_field: &str,
        key_field: &str,
    ) -> PrefillFill {
        self.prefill_pending.store(false, Ordering::SeqCst);
        if !self.prefill_is_current(generation) {
            return PrefillFill::default();
        }
        let (url, url_problem) = match url {
            Ok(url) => (url, None),
            Err(why) => (None, Some(why)),
        };
        let fill_url = url.clone().filter(|_| {
            let field = url_field.trim();
            field.is_empty() || field == FORGE_DEFAULT_URL
        });
        let key = match key {
            Ok(key) => key,
            Err(ReadError::Unreadable(why)) => {
                kept.2 = true;
                return PrefillFill {
                    url: fill_url,
                    key_field: None,
                    problem: Some(format!("The saved Forge key could not be read ({why}). It stays saved: {LEFT_SAVED}")),
                    note: None,
                };
            }
            // Most often nothing is saved: with the store down, a save goes to
            // the fallback file, which was read. So not a warning toast.
            Err(ReadError::NotAsked(why)) => {
                kept.2 = true;
                return PrefillFill {
                    url: fill_url,
                    key_field: None,
                    problem: None,
                    note: Some(format!(
                        "The system credential store could not be asked for a saved Forge key ({why}). If one is \
                         saved there, it stays saved: {LEFT_SAVED}"
                    )),
                };
            }
        };
        let saved_url = url.clone().unwrap_or_default();
        let shown = (key_field.is_empty() && key.is_some())
            .then(|| kept.remember(key.clone(), &saved_url))
            .filter(|shown| !shown.is_empty());
        // Read from the store, so it is there already.
        if shown.is_some() {
            kept.1 = true;
        }
        let mut problem = None;
        if key.is_some() && shown.is_none() {
            kept.2 = true;
            // A typed field keeps what was typed and says nothing; an empty
            // one says why the saved key is not in it.
            if key_field.is_empty() {
                let why = match (&url_problem, &url) {
                    (Some(e), _) => format!("The saved Forge URL could not be read ({e}), so the saved key is not used."),
                    (None, None) => "A Forge key is saved with no server URL, so it is not used.".to_string(),
                    (None, Some(_)) => {
                        "The saved Forge URL is not one a key is sent to, so the saved key is not used.".to_string()
                    }
                };
                problem = Some(format!("{why} It stays saved: {LEFT_SAVED}"));
            }
        }
        PrefillFill { url: fill_url, key_field: shown, problem, note: None }
    }

    /// The startup read of the saved Forge key has not come back yet. A
    /// Connect with an empty key waits for it: sent now, it would connect
    /// without authentication while the saved key is on its way.
    pub fn prefill_pending(&self) -> bool {
        self.prefill_pending.load(Ordering::SeqCst)
    }

    /// Every result that has arrived, in order. Never blocks.
    pub fn take_results(&self) -> Vec<KeyResult> {
        match self.results.lock() {
            Ok(rx) => rx.try_iter().collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// What a Forge prefill sets in the dialog: `None` leaves a field as it is.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PrefillFill {
    pub url: Option<String>,
    /// The mask for the key field (the key itself is in the
    /// [`ForgeRememberedKey`]).
    pub key_field: Option<&'static str>,
    /// A saved key could not be put in the field: why, and that it stays
    /// saved. Shown as a warning. Never any part of a key.
    pub problem: Option<String>,
    /// The store could not be asked, so a key may be saved, or none: said in
    /// the Output only, since most often none is.
    pub note: Option<String>,
}

/// The end of every prefill problem and note: what Connect does with a saved
/// key the dialog does not hold.
const LEFT_SAVED: &str = "a Connect with the key field empty leaves it, and the saved URL, as they are. Type \
                          the key to replace it, or untick Remember credentials and Connect to forget it.";

/// Run the credential worker on its own thread.
fn spawn_worker(work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new().name("credential-tasks".into()).spawn(work).map(drop)
}

/// The remembered Forge key and the origin it was saved for, kept on the Rust
/// side while the dialog shows [`FORGE_KEY_MASK`]; whether the store holds
/// that key for that origin already (so a re-Connect need not write it
/// again); and whether a key may be saved that is not kept here (it could not
/// be read or bound to an origin, or the field was typed in first), which a
/// Connect with the field empty must leave saved.
#[derive(Resource, Default)]
pub struct ForgeRememberedKey(Option<(String, Origin)>, bool, bool);

impl std::fmt::Debug for ForgeRememberedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "ForgeRememberedKey(<key>)" } else { "ForgeRememberedKey(none)" })
    }
}

impl ForgeRememberedKey {
    /// Keep `key`, bound to `url`'s origin, and say what the dialog's key
    /// field shows: the mask, or (no key, or a URL that fails the connection
    /// policy and so has no origin) an empty field with nothing kept.
    pub fn remember(&mut self, key: Option<String>, url: &str) -> &'static str {
        self.0 = key.zip(origin_of(url));
        self.1 = false;
        self.2 = false;
        if self.0.is_some() { FORGE_KEY_MASK } else { "" }
    }

    /// Forget the remembered key.
    pub fn forget(&mut self) {
        self.0 = None;
        self.1 = false;
        self.2 = false;
    }

    /// What a Connect asks the worker to remember, if anything. A re-Connect
    /// with Remember ticked and the kept key, already in the store for this
    /// origin, asks nothing: each write costs a store call (on Linux maybe an
    /// unlock prompt), and a remove that worked followed by a write that
    /// failed would lose a key that was fine. After a forget, a new key, or a
    /// Remember that reported a problem, it is written again. While a key may
    /// be saved that is not kept here, a ticked Connect with no key asks
    /// nothing either: remembering "no key" would remove the saved one, which
    /// the person never saw in the field.
    pub fn connect_request(&self, remember: bool, url: &str, key: &str, typed: bool) -> Option<KeyRequest> {
        if remember && self.2 && key.trim().is_empty() {
            return None;
        }
        let already_stored = remember
            && !typed
            && self.1
            && self.0.as_ref().is_some_and(|(kept, saved)| kept == key && origin_of(url).as_ref() == Some(saved));
        (!already_stored).then(|| remember_request(remember, url, key))
    }

    /// After a Connect with `key` in hand: with Remember ticked, the store has
    /// the kept key only when that is the key the Connect carried (unless a
    /// problem comes back: [`Self::not_stored`]). A Connect with the field
    /// emptied removed it from the store, so if the mask comes back (an undo
    /// in the field), the next Connect writes it again. Unticked, every saved
    /// key was forgotten.
    pub fn after_connect(&mut self, remember: bool, key: &str) {
        self.1 = remember && !key.trim().is_empty() && self.0.as_ref().is_some_and(|(kept, _)| kept == key);
        if !remember {
            self.2 = false;
        }
    }

    /// A Remember reported a problem: the store may not hold the key, so the
    /// next Connect writes it again.
    pub fn not_stored(&mut self) {
        self.1 = false;
    }
}

/// The bullet [`FORGE_KEY_MASK`] is made of. No Forge key contains it.
const MASK_CHAR: char = '\u{2022}';

/// What Connect has for a key, from what the dialog's field holds.
#[derive(PartialEq, Eq)]
pub enum ConnectKey {
    /// The field shows the whole mask: the remembered key (empty if none),
    /// for the origin it was saved for.
    Kept(String),
    /// The field shows the mask, but the URL is not the origin the key was
    /// saved for (`saved`). Connect is refused: nothing is sent or stored.
    OtherOrigin { saved: String },
    /// The person typed this key; it is worth remembering.
    Typed(String),
    /// The field still holds part of the mask (a Backspace in it, say). It
    /// is neither key: Connect is refused, so the bullets are never sent or
    /// saved over the remembered key.
    PartlyMasked,
}

impl std::fmt::Debug for ConnectKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shown = |key: &str| if key.is_empty() { "empty" } else { "<key>" };
        match self {
            ConnectKey::Kept(key) => write!(f, "Kept({})", shown(key)),
            ConnectKey::Typed(key) => write!(f, "Typed({})", shown(key)),
            ConnectKey::PartlyMasked => f.write_str("PartlyMasked"),
            ConnectKey::OtherOrigin { saved } => write!(f, "OtherOrigin({saved})"),
        }
    }
}

/// See [`ConnectKey`]. `url` is the dialog's URL field.
pub fn connect_key(field: &str, remembered: &ForgeRememberedKey, url: &str) -> ConnectKey {
    if field == FORGE_KEY_MASK {
        match &remembered.0 {
            None => ConnectKey::Kept(String::new()),
            Some((key, saved)) => match origin_of(url) {
                Some(now) if &now == saved => ConnectKey::Kept(key.clone()),
                _ => ConnectKey::OtherOrigin { saved: saved.to_string() },
            },
        }
    } else if field.contains(MASK_CHAR) {
        ConnectKey::PartlyMasked
    } else {
        ConnectKey::Typed(field.to_string())
    }
}

/// What a Connect asks the credential worker to do with "Remember
/// credentials". Ticked, the key in hand is remembered whether it was typed
/// or kept: after an untick forgot it, a re-tick with the mask still showing
/// stores it again (sending only typed keys lost it at the next restart).
/// Unticked, both are forgotten.
pub fn remember_request(remember: bool, url: &str, key: &str) -> KeyRequest {
    if remember {
        KeyRequest::ForgeRemember {
            remember: true,
            url: url.to_string(),
            key: (!key.trim().is_empty()).then(|| key.to_string()),
        }
    } else {
        KeyRequest::ForgeRemember { remember: false, url: url.to_string(), key: None }
    }
}

/// What one Connect does, decided from the Forge dialog by [`plan_connect`].
#[derive(Default)]
pub struct ConnectPlan {
    /// The key to connect with (empty: none). `None`: Connect is refused,
    /// and nothing is sent or remembered.
    pub key: Option<String>,
    /// What to ask the credential worker to remember or forget, if anything.
    pub request: Option<KeyRequest>,
    /// What the key field shows afterwards (the mask over a typed key that
    /// is now kept); `None` leaves it.
    pub key_field: Option<&'static str>,
    /// Why Connect was refused, for the dialog's error line.
    pub refusal: Option<String>,
}

impl std::fmt::Debug for ConnectPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self.key.as_deref() {
            None => "refused",
            Some(key) if key.is_empty() => "empty",
            Some(_) => "<key>",
        };
        write!(f, "ConnectPlan {{ key: {key}, request: {:?}, refusal: {:?} }}", self.request, self.refusal)
    }
}

/// Decide one Connect of the Forge dialog. `url` and `field` are the dialog's
/// URL and key fields; `remember`, its "Remember credentials" box; `pending`,
/// whether the startup read of the saved key is still out; `policy`, the URL
/// checked against the connection policy. Updates `kept`: a typed key is kept
/// (for this session only when Remember is unticked), and what the store
/// holds is noted. The UI applies the plan; nothing here waits.
pub fn plan_connect(
    kept: &mut ForgeRememberedKey,
    url: &str,
    field: &str,
    remember: bool,
    pending: bool,
    policy: Result<(), String>,
) -> ConnectPlan {
    let refused = |why: String| ConnectPlan { refusal: Some(why), ..ConnectPlan::default() };
    // A URL the connection policy refuses gets no key.
    if let Err(why) = policy {
        return refused(why);
    }
    let (key, typed) = match connect_key(field, kept, url) {
        // The saved key is for another server: not sent, not stored again
        // for this one.
        ConnectKey::OtherOrigin { saved } => return refused(other_origin_message(&saved, url)),
        // No key yet while the saved one is still being read: sent now, it
        // would connect without one.
        ConnectKey::Kept(key) | ConnectKey::Typed(key) if pending && key.trim().is_empty() => {
            return refused(STILL_READING.to_string())
        }
        ConnectKey::PartlyMasked => return refused(PARTLY_MASKED.to_string()),
        ConnectKey::Kept(key) => (key, false),
        ConnectKey::Typed(key) => (key, true),
    };
    let typed = typed && !key.trim().is_empty();
    // Ticked, the key in hand is remembered whether typed or kept (see
    // `remember_request`); a re-Connect with the saved key, already in the
    // store for this origin, writes nothing.
    let request = kept.connect_request(remember, url, &key, typed);
    // A typed key is kept on the Rust side and the field shows the mask
    // again, so the key never stays in Slint.
    let key_field = typed.then(|| kept.remember(Some(key.clone()), url));
    kept.after_connect(remember, &key);
    ConnectPlan { key: Some(key), request, key_field, refusal: None }
}

/// What the Forge dialog says when Connect, with no key typed, comes before
/// the saved key has been read.
pub const STILL_READING: &str = "Still reading the saved Forge key. Connect again in a moment.";

/// What the Forge dialog says when Connect is refused because the URL is
/// not the server the saved key was given for.
pub fn other_origin_message(saved: &str, url: &str) -> String {
    format!(
        "The saved Forge key is for {saved}, so it is not sent to {}. Type the key for this server, or change the URL back.",
        url.trim()
    )
}

/// What the Forge dialog says when Connect is refused for a partly masked
/// key.
pub const PARTLY_MASKED: &str =
    "The key field still holds part of the hidden saved key. Clear the field and type the key, or leave it as it was to use the saved key.";

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    /// The URL a test key is kept for, and the one `forge_runner` saved.
    const URL: &str = "https://forge.example.com";
    const OLD: &str = "https://forge.old";

    /// A worker, built on a helper thread with a deadline: a worker that ran
    /// on the constructing thread would never return, so a test fails here
    /// instead of hanging.
    fn tasks_with(runner: Runner) -> KeyTasks {
        let (built, made) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = built.send(KeyTasks::with_runner(runner));
        });
        made.recv_timeout(Duration::from_secs(5))
            .expect("KeyTasks did not come back: the worker ran on the constructing thread")
    }

    fn results_within(tasks: &KeyTasks, count: usize) -> Vec<KeyResult> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < count && Instant::now() < deadline {
            got.extend(tasks.take_results());
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    /// No store call runs on the thread that asks: the request returns at
    /// once and the work happens on the worker.
    #[test]
    fn requests_run_on_the_worker_not_the_calling_thread() {
        let seen: Arc<Mutex<Vec<ThreadId>>> = Arc::default();
        let record = seen.clone();
        let tasks = tasks_with(Arc::new(move |request| {
            record.lock().unwrap().push(std::thread::current().id());
            match request {
                KeyRequest::Remove { name } => KeyResult::Removed { name, result: Ok(true) },
                _ => KeyResult::ForgeRemembered { problems: Vec::new() },
            }
        }));
        tasks.submit(KeyRequest::Remove { name: "SAM_API_KEY".into() });
        assert_eq!(results_within(&tasks, 1).len(), 1);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_ne!(seen[0], std::thread::current().id(), "the store call ran on the calling thread");
    }

    /// One worker, in order: a Save then a Remove are done as Save then Remove.
    #[test]
    fn requests_run_in_the_order_they_were_made() {
        let tasks = tasks_with(Arc::new(|request| match request {
            KeyRequest::Save { name, .. } => {
                std::thread::sleep(Duration::from_millis(50));
                KeyResult::Saved { name, result: Err("saved".into()) }
            }
            KeyRequest::Remove { name } => KeyResult::Removed { name, result: Ok(true) },
            _ => KeyResult::ForgeRemembered { problems: Vec::new() },
        }));
        tasks.submit(KeyRequest::Save { name: "A".into(), value: "k".into() });
        tasks.submit(KeyRequest::Remove { name: "A".into() });
        let got = results_within(&tasks, 2);
        assert!(matches!(got.as_slice(), [KeyResult::Saved { .. }, KeyResult::Removed { .. }]), "{got:?}");
    }

    /// The dialog field never holds the key: a remembered key shows as the
    /// mask, and Connect gets the real key back only from the Rust side.
    #[test]
    fn the_forge_dialog_shows_a_mask_and_connect_uses_the_kept_key() {
        let mut kept = ForgeRememberedKey::default();
        let shown = kept.remember(Some("fk-real-secret".into()), URL);
        assert_eq!(shown, FORGE_KEY_MASK);
        assert!(!shown.contains("fk-real"), "the field never holds the key");
        assert_eq!(connect_key(shown, &kept, URL), ConnectKey::Kept("fk-real-secret".into()), "untouched: the kept key");
        assert_eq!(connect_key("fk-typed", &kept, URL), ConnectKey::Typed("fk-typed".into()), "typed: the new key");
        assert_eq!(kept.remember(None, URL), "", "no remembered key: an empty field");
        assert_eq!(connect_key(FORGE_KEY_MASK, &kept, OLD), ConnectKey::Kept(String::new()));
        let mut forget = ForgeRememberedKey::default();
        forget.remember(Some("k".into()), URL);
        forget.forget();
        assert_eq!(connect_key(FORGE_KEY_MASK, &forget, OLD), ConnectKey::Kept(String::new()));
    }

    /// A Backspace in the masked field leaves bullets that are neither key:
    /// Connect is refused, so they are never sent or saved over the key.
    #[test]
    fn a_partly_masked_field_is_refused() {
        let mut kept = ForgeRememberedKey::default();
        let shown = kept.remember(Some("fk-real-secret".into()), URL);
        let backspaced = &shown[..shown.len() - MASK_CHAR.len_utf8()];
        assert_eq!(connect_key(backspaced, &kept, URL), ConnectKey::PartlyMasked);
        assert_eq!(connect_key(&format!("{shown}x"), &kept, URL), ConnectKey::PartlyMasked);
        assert_eq!(connect_key("", &kept, URL), ConnectKey::Typed(String::new()), "an emptied field is not the mask");
    }

    fn forge_runner() -> Runner {
        Arc::new(|request| match request {
            KeyRequest::ForgePrefill { generation } => KeyResult::ForgePrefill {
                url: Ok(Some("https://forge.old".into())),
                key: Ok(Some("fk-old".into())),
                generation,
            },
            _ => KeyResult::ForgeRemembered { problems: Vec::new() },
        })
    }

    type Read = Result<Option<String>, ReadError>;

    fn prefill_of(results: Vec<KeyResult>) -> (Read, Read, u64) {
        results
            .into_iter()
            .find_map(|r| match r {
                KeyResult::ForgePrefill { url, key, generation } => Some((url, key, generation)),
                _ => None,
            })
            .expect("a prefill result")
    }

    /// The person Connects with Remember unticked before the startup read
    /// returns: the late prefill must not bring the forgotten key back, nor
    /// overwrite what they typed.
    #[test]
    fn a_prefill_that_arrives_after_a_connect_changes_nothing() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        tasks.submit(KeyRequest::ForgeRemember { remember: false, url: "https://typed".into(), key: None });
        let (url, key, generation) = prefill_of(results_within(&tasks, 2));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url, key, "https://typed", "");
        assert_eq!(fill, PrefillFill::default(), "nothing is filled");
        assert_eq!(connect_key(FORGE_KEY_MASK, &kept, OLD), ConnectKey::Kept(String::new()), "the forgotten key is not kept");
    }

    /// A Connect that keeps Remember ticked does not make the startup read
    /// stale: its URL and key still fill whatever is empty.
    #[test]
    fn a_connect_that_remembers_keeps_the_prefill_current() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        tasks.submit(KeyRequest::ForgeRemember { remember: true, url: "https://typed".into(), key: None });
        let (url, key, generation) = prefill_of(results_within(&tasks, 2));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url, key, "https://typed", "");
        assert_eq!(fill, PrefillFill { url: None, key_field: Some(FORGE_KEY_MASK), problem: None, note: None }, "the empty key field is filled");
        assert_eq!(connect_key(FORGE_KEY_MASK, &kept, OLD), ConnectKey::Kept("fk-old".into()));
    }

    /// A Connect with no key waits while the startup read is out, and not
    /// once its result has been applied.
    #[test]
    fn a_connect_with_no_key_waits_for_the_startup_read() {
        let tasks = tasks_with(forge_runner());
        assert!(!tasks.prefill_pending(), "nothing asked yet");
        tasks.submit_forge_prefill();
        assert!(tasks.prefill_pending(), "asked, not yet applied");
        let (url, key, generation) = prefill_of(results_within(&tasks, 1));
        assert!(tasks.prefill_pending(), "arrived but not applied is still pending");
        let mut kept = ForgeRememberedKey::default();
        tasks.apply_forge_prefill(&mut kept, generation, url, key, "", "");
        assert!(!tasks.prefill_pending(), "applied");
    }

    /// With the worker gone, the prefill is never read: Connect must not wait
    /// for it.
    #[test]
    fn a_prefill_that_cannot_be_sent_is_not_waited_for() {
        let tasks = tasks_with(Arc::new(|_| panic!("the worker stops here")));
        // The first request stops the worker; once a send fails, it is gone.
        let deadline = Instant::now() + Duration::from_secs(5);
        while tasks.submit(KeyRequest::Remove { name: "N".into() }) {
            assert!(Instant::now() < deadline, "the worker did not stop");
            std::thread::sleep(Duration::from_millis(5));
        }
        tasks.submit_forge_prefill();
        assert!(!tasks.prefill_pending(), "nothing was sent, so nothing is pending");
    }

    /// A current prefill fills only what is still empty.
    #[test]
    fn a_current_prefill_fills_only_empty_fields() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, key, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url.clone(), key.clone(), "", "");
        assert_eq!(fill, PrefillFill { url: Some("https://forge.old".into()), key_field: Some(FORGE_KEY_MASK), problem: None, note: None });
        assert_eq!(connect_key(FORGE_KEY_MASK, &kept, OLD), ConnectKey::Kept("fk-old".into()));
        let mut typed = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut typed, generation, url, key, "https://mine", "fk-mine");
        assert_eq!(fill, PrefillFill::default(), "typed fields are kept");
        assert_eq!(connect_key(FORGE_KEY_MASK, &typed, OLD), ConnectKey::Kept(String::new()), "and the stored key is not taken over them");
    }

    /// The dialog starts with the default URL in the field; a saved URL
    /// replaces it, and a URL the person typed is kept. The default here is
    /// the one the Slint files really start with.
    #[test]
    fn a_saved_url_replaces_the_default_but_not_a_typed_one() {
        let dir = env!("CARGO_MANIFEST_DIR");
        for (file, property) in [("ui/slint/main.slint", "forge-server-url"), ("ui/slint/forge_connect.slint", "server-url")] {
            let source = std::fs::read_to_string(format!("{dir}/{file}")).expect(file);
            let line = source
                .lines()
                .find(|l| l.contains(&format!("property <string> {property}:")))
                .unwrap_or_else(|| panic!("{file} has no {property}"));
            assert!(line.contains(&format!("\"{FORGE_DEFAULT_URL}\"")), "{file} starts {property} elsewhere: {line}");
        }
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, key, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url.clone(), key.clone(), FORGE_DEFAULT_URL, "");
        assert_eq!(fill.url.as_deref(), Some("https://forge.old"), "the saved URL replaces the default");
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url, key, "https://mine", "");
        assert_eq!(fill.url, None, "a typed URL is kept");
    }

    /// Untick Remember and Connect (the key is forgotten), then tick it and
    /// Connect with the mask still showing: the kept key is stored again.
    #[test]
    fn a_re_ticked_remember_stores_the_kept_key_again() {
        let mut kept = ForgeRememberedKey::default();
        let shown = kept.remember(Some("fk-real-secret".into()), URL);
        let ConnectKey::Kept(key) = connect_key(shown, &kept, URL) else { panic!("the mask is the kept key") };
        assert!(matches!(
            remember_request(false, "https://f", &key),
            KeyRequest::ForgeRemember { remember: false, key: None, .. }
        ));
        match remember_request(true, "https://f", &key) {
            KeyRequest::ForgeRemember { remember: true, key: Some(k), .. } => assert_eq!(k, "fk-real-secret"),
            other => panic!("the kept key was not sent to be remembered: {other:?}"),
        }
        assert!(
            matches!(remember_request(true, "https://f", "  "), KeyRequest::ForgeRemember { remember: true, key: None, .. }),
            "no key in hand, none to remember"
        );
    }

    /// A Remember that carries a key makes an earlier prefill stale: applied
    /// into a field emptied since, it would bring the older key back.
    #[test]
    fn a_remember_with_a_key_makes_the_prefill_stale() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        tasks.submit(remember_request(true, "https://typed", "fk-new"));
        let (url, key, generation) = prefill_of(results_within(&tasks, 2));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url, key, "", "");
        assert_eq!(fill, PrefillFill::default(), "nothing is filled");
        assert_eq!(connect_key(FORGE_KEY_MASK, &kept, OLD), ConnectKey::Kept(String::new()), "the older key is not kept");
    }

    /// A key saved for one server is refused for another: the mask with a
    /// changed URL is neither sent nor stored. The same server written with
    /// other letter case, an explicit default port or a path is the same.
    #[test]
    fn a_saved_key_is_refused_for_another_origin() {
        let mut kept = ForgeRememberedKey::default();
        let shown = kept.remember(Some("fk-real-secret".into()), "https://forge.example.com");
        for same in ["https://FORGE.example.com", "https://forge.example.com:443/", "https://forge.example.com/api"] {
            assert_eq!(connect_key(shown, &kept, same), ConnectKey::Kept("fk-real-secret".into()), "{same}");
        }
        for other in ["https://evil.example.com", "https://forge.example.com:8443", "http://localhost"] {
            let refused = connect_key(shown, &kept, other);
            assert_eq!(refused, ConnectKey::OtherOrigin { saved: "https://forge.example.com:443".into() }, "{other}");
            assert!(!format!("{refused:?}").contains("fk-real"), "{refused:?}");
        }
        assert!(other_origin_message("https://forge.example.com:443", " https://evil.example.com ").contains("https://evil.example.com."));
    }

    /// Security's case: Remember ticked, the mask in the field, a changed
    /// origin. Connect is refused before any request is made, so nothing is
    /// stored and nothing is sent.
    #[test]
    fn a_changed_origin_with_remember_ticked_stores_and_sends_nothing() {
        let mut kept = ForgeRememberedKey::default();
        let shown = kept.remember(Some("fk-real-secret".into()), URL);
        let plan = plan_connect(&mut kept, "https://new.example.com", shown, true, false, Ok(()));
        assert!(plan.key.is_none() && plan.request.is_none(), "{plan:?}");
        assert!(plan.refusal.as_deref().is_some_and(|r| r.contains("https://new.example.com")), "{plan:?}");
        // A URL the policy refuses: no key, typed or kept, and nothing stored.
        for field in [shown, "fk-typed"] {
            let plan = plan_connect(&mut kept, URL, field, true, false, Err("refused by the policy".into()));
            assert!(plan.key.is_none() && plan.request.is_none(), "{plan:?}");
            assert_eq!(plan.refusal.as_deref(), Some("refused by the policy"));
        }
    }

    /// A saved key with no saved URL has no origin: it is not kept, the
    /// field stays empty, and the dialog says why.
    #[test]
    fn a_saved_key_without_a_saved_url_is_not_kept() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (_, key, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, Ok(None), key, "", "");
        assert_eq!((fill.url, fill.key_field), (None, None));
        assert!(fill.problem.as_deref().is_some_and(|p| p.contains("no server URL")), "{:?}", fill.problem);
        assert_eq!(connect_key(FORGE_KEY_MASK, &kept, OLD), ConnectKey::Kept(String::new()));
    }

    /// Remember clears the old key, then writes the URL, then the key; a
    /// failure stops it there, so no key is left beside a URL it was not
    /// saved for. Forget removes both.
    #[test]
    fn remember_never_leaves_a_key_beside_another_url() {
        fn run(fail: Option<&str>, key: Option<&str>) -> (Vec<String>, Vec<String>) {
            let log = std::cell::RefCell::new(Vec::new());
            let problems = remember_with(
                true,
                "https://new.example.com",
                key,
                &mut |name, _| {
                    log.borrow_mut().push(format!("set {name}"));
                    if fail == Some(name) { Err("down".into()) } else { Ok(()) }
                },
                &mut |name| {
                    log.borrow_mut().push(format!("remove {name}"));
                    if fail == Some(name) { Err("down".into()) } else { Ok(()) }
                },
            );
            (log.into_inner(), problems)
        }
        let (log, problems) = run(None, Some("fk-new"));
        assert_eq!(log, [
            format!("remove {FORGE_KEY_CREDENTIAL}"),
            format!("set {FORGE_URL_CREDENTIAL}"),
            format!("set {FORGE_KEY_CREDENTIAL}"),
        ]);
        assert!(problems.is_empty());
        let (log, problems) = run(Some(FORGE_URL_CREDENTIAL), Some("fk-new"));
        assert!(!log.contains(&format!("set {FORGE_KEY_CREDENTIAL}")), "no key after a failed URL: {log:?}");
        assert_eq!(problems.len(), 1);
        let (log, _) = run(None, None);
        assert_eq!(log[0], format!("remove {FORGE_KEY_CREDENTIAL}"), "no key in hand: the old one is not left for the new URL");
        let mut removed = Vec::new();
        remember_with(false, "", None, &mut |_, _| Ok(()), &mut |name| { removed.push(name.to_string()); Ok(()) });
        assert_eq!(removed, [FORGE_KEY_CREDENTIAL, FORGE_URL_CREDENTIAL]);
    }

    /// A plain re-Connect with the saved key writes nothing. After a forget
    /// (Remember unticked), a new key, or a Remember that reported a problem,
    /// the next ticked Connect writes it again.
    #[test]
    fn a_plain_re_connect_does_not_store_the_key_again() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, key, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        tasks.apply_forge_prefill(&mut kept, generation, url, key, "", "");
        let connect = |kept: &mut ForgeRememberedKey, remember: bool, field: &str| {
            let plan = plan_connect(kept, OLD, field, remember, false, Ok(()));
            assert!(plan.key.is_some(), "{plan:?}");
            plan.request
        };
        assert!(connect(&mut kept, true, FORGE_KEY_MASK).is_none(), "the saved key, unchanged: nothing written");
        assert!(connect(&mut kept, true, FORGE_KEY_MASK).is_none(), "and again");
        assert!(
            matches!(connect(&mut kept, false, FORGE_KEY_MASK), Some(KeyRequest::ForgeRemember { remember: false, .. })),
            "unticked: forgotten"
        );
        assert!(
            matches!(connect(&mut kept, true, FORGE_KEY_MASK), Some(KeyRequest::ForgeRemember { key: Some(_), .. })),
            "re-ticked after a forget: written again"
        );
        assert!(connect(&mut kept, true, FORGE_KEY_MASK).is_none(), "then not again");
        kept.not_stored();
        assert!(connect(&mut kept, true, FORGE_KEY_MASK).is_some(), "after a reported problem: written again");
        assert!(
            matches!(connect(&mut kept, true, "fk-new"), Some(KeyRequest::ForgeRemember { key: Some(_), .. })),
            "a new key is written"
        );
        assert!(connect(&mut kept, true, FORGE_KEY_MASK).is_none(), "and kept after that");
    }

    /// The saved key could not be read at startup (on macOS or Linux, a store
    /// prompt nobody answered within 30 s). The dialog says so, and a ticked
    /// Connect with the field empty asks nothing, so the key stays saved. A
    /// typed key replaces it; an unticked Connect forgets it.
    #[test]
    fn an_unreadable_saved_key_is_said_and_not_removed_by_an_empty_connect() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, _, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        let unread = Err(ReadError::Unreadable("sam.api_key.key could not be read: it is not text".to_string()));
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url.clone(), unread.clone(), "", "");
        assert_eq!(fill.key_field, None, "no mask over a key that is not kept");
        let problem = fill.problem.expect("the dialog is told");
        assert!(problem.contains("could not be read") && problem.contains("stays saved"), "{problem}");
        let plan = plan_connect(&mut kept, OLD, "", true, false, Ok(()));
        assert_eq!(plan.key.as_deref(), Some(""), "it connects without a key");
        assert!(plan.request.is_none(), "and asks nothing, so the saved key is not removed: {plan:?}");
        assert!(plan_connect(&mut kept, OLD, "", true, false, Ok(())).request.is_none(), "nor on the next Connect");
        let typed = plan_connect(&mut kept, OLD, "fk-new", true, false, Ok(()));
        assert!(matches!(typed.request, Some(KeyRequest::ForgeRemember { remember: true, key: Some(_), .. })), "{typed:?}");
        let emptied = plan_connect(&mut kept, OLD, "", true, false, Ok(()));
        assert!(
            matches!(emptied.request, Some(KeyRequest::ForgeRemember { key: None, .. })),
            "the typed key is kept now, so emptying the field is the person's choice again: {emptied:?}"
        );
        let mut other = ForgeRememberedKey::default();
        tasks.apply_forge_prefill(&mut other, generation, url, unread, "", "");
        let unticked = plan_connect(&mut other, OLD, "", false, false, Ok(()));
        assert!(matches!(unticked.request, Some(KeyRequest::ForgeRemember { remember: false, .. })), "unticked: forgotten");
        let after = plan_connect(&mut other, OLD, "", true, false, Ok(()));
        assert!(after.request.is_some(), "forgotten, so nothing is left to protect: {after:?}");
    }

    /// A saved key whose saved URL cannot be read, or is one the connection
    /// policy refuses, has no origin: it is not used, the dialog says why, and
    /// an empty Connect leaves it saved.
    #[test]
    fn a_saved_key_with_no_usable_url_is_said_and_left_saved() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (_, key, generation) = prefill_of(results_within(&tasks, 1));
        for (url, says) in [
            (Ok(Some("http://forge.example.com".to_string())), "not one a key is sent to"),
            (Err(ReadError::NotAsked("the store did not answer in time".to_string())), "URL could not be read"),
        ] {
            let mut kept = ForgeRememberedKey::default();
            let fill = tasks.apply_forge_prefill(&mut kept, generation, url, key.clone(), "", "");
            assert_eq!(fill.key_field, None);
            assert!(fill.problem.as_deref().is_some_and(|p| p.contains(says) && p.contains("stays saved")), "{fill:?}");
            let plan = plan_connect(&mut kept, "https://forge.example.com", "", true, false, Ok(()));
            assert!(plan.request.is_none(), "{says}: {plan:?}");
        }
    }

    /// The store could not be asked at startup (no Secret Service on this
    /// Linux desktop, or a prompt nobody answered): most often nothing is
    /// saved, so it is a note for the Output, not a warning, and it says
    /// "if". A key that may be there is still not removed by an empty Connect.
    #[test]
    fn a_store_that_could_not_be_asked_is_a_note_and_still_protects() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, _, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        let not_asked = Err(ReadError::NotAsked("the system credential store is unavailable".to_string()));
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url, not_asked, "", "");
        assert_eq!(fill.problem, None, "no warning toast: most often nothing is saved");
        let note = fill.note.expect("the Output is told");
        assert!(note.contains("If one is saved") && !note.contains("could not be read"), "{note}");
        let plan = plan_connect(&mut kept, OLD, "", true, false, Ok(()));
        assert!(plan.request.is_none(), "a key that may be there is left: {plan:?}");
    }

    /// The person typed in the key field before the saved key was read: what
    /// they typed stays and nothing is said. If they then empty the field and
    /// Connect, the saved key they never saw is left saved.
    #[test]
    fn a_saved_key_behind_a_typed_field_is_left_saved() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, key, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        let fill = tasks.apply_forge_prefill(&mut kept, generation, url, key, "", "fk-typed");
        assert_eq!(fill, PrefillFill { url: Some(OLD.into()), key_field: None, problem: None, note: None });
        let plan = plan_connect(&mut kept, OLD, "", true, false, Ok(()));
        assert!(plan.request.is_none(), "{plan:?}");
    }

    /// Remember ticked and the field emptied: that Connect removes the key
    /// from the store. If the mask comes back (an undo in the field), the next
    /// Connect uses the kept key and writes it again, so it is still saved
    /// after a restart.
    #[test]
    fn after_an_emptied_connect_the_kept_key_is_written_again() {
        let tasks = tasks_with(forge_runner());
        tasks.submit_forge_prefill();
        let (url, key, generation) = prefill_of(results_within(&tasks, 1));
        let mut kept = ForgeRememberedKey::default();
        tasks.apply_forge_prefill(&mut kept, generation, url, key, "", "");
        assert!(plan_connect(&mut kept, OLD, FORGE_KEY_MASK, true, false, Ok(())).request.is_none(), "stored already");
        let emptied = plan_connect(&mut kept, OLD, "", true, false, Ok(()));
        assert!(matches!(emptied.request, Some(KeyRequest::ForgeRemember { key: None, .. })), "{emptied:?}");
        let undone = plan_connect(&mut kept, OLD, FORGE_KEY_MASK, true, false, Ok(()));
        assert_eq!(undone.key.as_deref(), Some("fk-old"));
        match undone.request {
            Some(KeyRequest::ForgeRemember { remember: true, key: Some(k), .. }) => assert_eq!(k, "fk-old"),
            other => panic!("the kept key was not written again: {other:?}"),
        }
    }

    #[test]
    fn no_request_or_result_shows_a_key() {
        let shown = format!(
            "{:?} {:?} {:?} {:?} {:?} {:?}",
            ConnectKey::Kept("sk-secret".into()),
            ConnectKey::Typed("sk-secret".into()),
            KeyRequest::Save { name: "N".into(), value: "sk-secret".into() },
            KeyRequest::ForgeRemember { remember: true, url: "u".into(), key: Some("sk-secret".into()) },
            KeyResult::ForgePrefill { url: Ok(None), key: Ok(Some("sk-secret".into())), generation: 0 },
            ForgeRememberedKey(Some(("sk-secret".into(), origin_of(URL).unwrap())), true, false),
        );
        let plan = plan_connect(&mut ForgeRememberedKey::default(), URL, "sk-secret", true, false, Ok(()));
        let shown = format!("{shown} {plan:?}");
        assert!(!shown.contains("sk-secret"), "{shown}");
    }
}
