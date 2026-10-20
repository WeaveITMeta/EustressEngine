// =============================================================================
// Eustress Engine - Studio Authentication
// =============================================================================
// Studio signs in with the user's Eustress identity key: it signs the API's
// challenge together with the API's own origin (`eustress-login-v1`), and the
// API answers with a token for that account. Signed in means exactly that.
// See docs/architecture/STUDIO_IDENTITY_SIGNIN.md.
// =============================================================================

use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// The API sign-ins go to: production, or `EUSTRESS_API_URL` when it names an
// accepted test API (`eustress_common::api_base`).
use eustress_common::api_base::{
    api_base, api_label, api_origin, api_url, login_message, site_base, LOGIN_FORMAT, PRODUCTION_SITE,
};

/// Authentication state resource
#[derive(Resource)]
pub struct AuthState {
    /// The signed-in account, as the API describes it.
    pub user: Option<AuthUser>,
    /// The token the API issued for that account; never anything else.
    pub token: Option<String>,
    /// Sign-in status
    pub status: AuthStatus,
    /// Why the last sign-in failed, as the user is shown it.
    pub error: Option<String>,
    /// Whether `token` is a token the API issued: true exactly while signed
    /// in. Bliss reads it before asking the API to co-sign.
    pub jwt_valid: bool,
    /// The identity file this session signed in with, if it did.
    pub identity: Option<PathBuf>,
    /// Sign-ins that finished on their threads, applied by [`auth_poll_system`].
    finished: Arc<Mutex<Vec<SignInFinished>>>,
    /// The sign-in whose result counts. Starting another, or signing out,
    /// supersedes it, and its result is dropped when it lands.
    attempt: u64,
    /// A finished sign-in for the UI to show once ([`AuthState::take_notice`]).
    notice: Option<SignInNotice>,
    /// A sign-out still telling the API ([`AuthState::sign_out_everywhere`]);
    /// the next sign-in waits for it, so it never ends the new token.
    revoking: Option<std::thread::JoinHandle<()>>,
    /// Set by that sign-out's thread when the API could not end the account's
    /// tokens; shown once ([`AuthState::take_warning`]).
    revoke_warning: Arc<Mutex<Option<String>>>,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            user: None,
            token: None,
            status: AuthStatus::LoggedOut,
            error: None,
            jwt_valid: false,
            identity: None,
            finished: Arc::new(Mutex::new(Vec::new())),
            attempt: 0,
            notice: None,
            revoking: None,
            revoke_warning: Arc::new(Mutex::new(None)),
        }
    }
}

/// Bliss node state — tracks node mode and balance for the engine UI.
/// Light node runs by default. Full node is opt-in (+10% BLS bonus).
/// BLS has 2 decimal places (1 BLS = 100 minor units in the ledger).
#[derive(Resource, Clone, Debug)]
pub struct BlissNodeState {
    /// "Light" or "Full"
    pub mode: String,
    /// Current BLS balance — 2 decimals, full precision (for dropdown)
    pub balance: String,
    /// Current BLS balance — shortened 2 decimals (for badge)
    pub balance_short: String,
    /// Pending contribution score for today (display string, e.g. "+9.8 pts today")
    pub pending: String,
    /// Bonus multiplier display (e.g. "1.0x" or "1.1x")
    pub bonus: String,
    /// Whether Bliss integration is enabled
    pub enabled: bool,
}

impl Default for BlissNodeState {
    fn default() -> Self {
        Self {
            mode: "Light".to_string(),
            balance: "0.00".to_string(),
            balance_short: "0.00".to_string(),
            pending: "+0.0 pts today".to_string(),
            bonus: "1.0x".to_string(),
            enabled: true,
        }
    }
}

impl BlissNodeState {
    pub fn set_light(&mut self) {
        self.mode = "Light".to_string();
        self.bonus = "1.0x".to_string();
    }

    pub fn set_full(&mut self) {
        self.mode = "Full".to_string();
        self.bonus = "1.1x".to_string();
    }
}

/// Authenticated user info
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub steam_id: Option<String>,
    pub discord_id: Option<String>,
    /// Whole BLS, fractional — the ledger credits fractional amounts at each
    /// daily distribution, so this is f64 (an i64 truncated every balance
    /// below 1 BLS to zero).
    pub bliss_balance: f64,
    pub total_hours: f64,
}

/// Authentication status
#[derive(Default, Clone, PartialEq)]
pub enum AuthStatus {
    #[default]
    LoggedOut,
    /// Signing in with an identity file
    LoggingIn,
    /// Startup: checking the saved sign-in
    Validating,
    /// Signed in: the API issued this session's token
    LoggedIn,
    /// The last sign-in failed (`AuthState::error` says why)
    Failed,
    /// The API could not be reached; Studio works offline, signed out
    Offline,
}

/// What started a sign-in, so its result is shown in the right place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInSource {
    /// Startup: the saved sign-in, else the saved identity file.
    Startup,
    /// The sign-in dialog.
    Dialog,
    /// Switching to another saved identity.
    Switch,
}

/// A finished sign-in, for the UI to show once.
#[derive(Clone, Debug)]
pub struct SignInNotice {
    pub source: SignInSource,
    /// The identity file it signed in with, and that file's public key (64
    /// hex digits), when it signed in with one.
    pub identity: Option<(PathBuf, String)>,
    /// The account's name, or why Studio is not signed in.
    pub outcome: Result<String, String>,
}

/// A sign-in that failed: what the user is shown, and whether the API was
/// out of reach (Studio then works offline).
#[derive(Clone, Debug)]
struct SignInError {
    message: String,
    unreachable: bool,
}

impl SignInError {
    fn refused(message: impl Into<String>) -> Self {
        Self { message: message.into(), unreachable: false }
    }
}

/// A successful sign-in, before it is applied.
struct SignedIn {
    token: String,
    user: AuthUser,
    /// The identity file's public key, when it signed in with one.
    public_key: Option<String>,
}

/// A sign-in's result, left by its thread for [`auth_poll_system`].
struct SignInFinished {
    attempt: u64,
    source: SignInSource,
    identity: Option<PathBuf>,
    outcome: Result<SignedIn, SignInError>,
    /// The saved sign-in was refused (it expired): remove it.
    forget_saved: bool,
}

/// What applying a result does to this API's saved login file.
#[derive(Debug, Default, PartialEq)]
struct LoginFileChange {
    forget: bool,
    save: Option<String>,
}

impl AuthState {
    /// Signed in: the API issued this session's token.
    pub fn is_logged_in(&self) -> bool {
        self.status == AuthStatus::LoggedIn && self.token.is_some()
    }

    /// The API could not be reached at the last sign-in.
    pub fn is_offline(&self) -> bool {
        self.status == AuthStatus::Offline
    }

    /// Check if can publish (must be online and logged in)
    pub fn can_publish(&self) -> bool {
        self.is_logged_in() && !self.is_offline()
    }

    /// Get the auth token if logged in
    pub fn get_token(&self) -> Option<&str> {
        if self.is_logged_in() {
            self.token.as_deref()
        } else {
            None
        }
    }

    /// Sign in with the identity file at `path`, on a thread of its own. It
    /// supersedes any sign-in under way; the result lands through
    /// [`auth_poll_system`], and the UI shows it from [`Self::take_notice`].
    pub fn sign_in_with_identity(&mut self, path: PathBuf, source: SignInSource) {
        let attempt = self.begin(AuthStatus::LoggingIn);
        // A sign-out still telling the API goes first, so it can never end
        // the token this sign-in is about to get.
        let revoking = self.revoking.take();
        self.spawn(attempt, source, move || {
            if let Some(revoking) = revoking {
                let _ = revoking.join();
            }
            let outcome = sign_in(&path);
            (outcome, Some(path), false)
        });
    }

    /// Startup: the saved sign-in if the API still takes it, else the saved
    /// identity file (`identity`), on one thread. With neither, Studio stays
    /// signed out.
    pub fn restore(&mut self, identity: Option<PathBuf>) {
        reprotect_saved_logins();
        let saved = load_saved_token();
        if saved.is_none() && identity.is_none() {
            return;
        }
        let attempt = self.begin(AuthStatus::Validating);
        self.spawn(attempt, SignInSource::Startup, move || restore_session(saved, identity));
    }

    /// Sign out of every device: the API ends every token this account holds
    /// (in Studio, the Player and the website, everywhere), and Studio signs
    /// out here ([`Self::sign_out_here`]). The API is told on a thread of its
    /// own, which the next sign-in waits for; if the API cannot be reached,
    /// the token lapses when it expires.
    pub fn sign_out_everywhere(&mut self) {
        if let Some(token) = self.get_token().map(str::to_string) {
            let warning = self.revoke_warning.clone();
            self.revoking = std::thread::Builder::new()
                .name("eustress-sign-out".into())
                .spawn(move || {
                    let reply = agent()
                        .post(&api_url("/api/auth/logout"))
                        .set("Authorization", &format!("Bearer {token}"))
                        .call();
                    // A 401: the token was already ended.
                    let ended = matches!(reply, Ok(_) | Err(ureq::Error::Status(401, _)));
                    if !ended {
                        if let Ok(mut w) = warning.lock() {
                            *w = Some(REVOKE_FAILED.to_string());
                        }
                    }
                })
                .ok();
        }
        self.sign_out_here();
    }

    /// What a sign-out of every device could not do, to show once.
    pub fn take_warning(&self) -> Option<String> {
        self.revoke_warning.lock().ok()?.take()
    }

    /// Sign out on this machine: this API's saved login is removed (the
    /// Player here shares it) and a sign-in under way no longer counts; the
    /// account stays signed in on other devices and the website. Sign Out and
    /// switching identity use this.
    pub fn sign_out_here(&mut self) {
        self.attempt += 1;
        self.user = None;
        self.token = None;
        self.status = AuthStatus::LoggedOut;
        self.error = None;
        self.jwt_valid = false;
        self.identity = None;
        if let Some(path) = get_token_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Whether a finished sign-in waits to be shown.
    pub fn has_notice(&self) -> bool {
        self.notice.is_some()
    }

    /// The finished sign-in to show, once.
    pub fn take_notice(&mut self) -> Option<SignInNotice> {
        self.notice.take()
    }

    fn begin(&mut self, status: AuthStatus) -> u64 {
        self.attempt += 1;
        self.status = status;
        self.error = None;
        self.attempt
    }

    /// Run `work` on its own thread. Its result is applied when it lands, if
    /// `attempt` still counts then.
    fn spawn(
        &mut self,
        attempt: u64,
        source: SignInSource,
        work: impl FnOnce() -> (Result<SignedIn, SignInError>, Option<PathBuf>, bool) + Send + 'static,
    ) {
        let finished = self.finished.clone();
        let started = std::thread::Builder::new().name("eustress-sign-in".into()).spawn(move || {
            let (outcome, identity, forget_saved) = work();
            if let Ok(mut f) = finished.lock() {
                f.push(SignInFinished { attempt, source, identity, outcome, forget_saved });
            }
        });
        if let Err(e) = started {
            if let Ok(mut f) = self.finished.lock() {
                f.push(SignInFinished {
                    attempt,
                    source,
                    identity: None,
                    outcome: Err(SignInError::refused(format!("Could not start signing in: {e}"))),
                    forget_saved: false,
                });
            }
        }
    }

    /// Apply a finished sign-in, unless a later one superseded it. Returns
    /// what to do with this API's saved login file; the caller does it, so
    /// this stays off the disk.
    fn apply(&mut self, finished: SignInFinished) -> LoginFileChange {
        if finished.attempt != self.attempt {
            return LoginFileChange::default();
        }
        let mut change = LoginFileChange { forget: finished.forget_saved, save: None };
        match finished.outcome {
            Ok(signed_in) => {
                let name = signed_in.user.username.clone();
                // Saved for this API, so the Player on this machine shares it.
                change.save = Some(signed_in.token.clone());
                self.token = Some(signed_in.token);
                self.user = Some(signed_in.user);
                self.status = AuthStatus::LoggedIn;
                self.jwt_valid = true;
                self.error = None;
                self.identity = finished.identity.clone();
                self.notice = Some(SignInNotice {
                    source: finished.source,
                    identity: finished.identity.zip(signed_in.public_key),
                    outcome: Ok(name),
                });
            }
            Err(e) => {
                self.token = None;
                self.user = None;
                self.jwt_valid = false;
                self.identity = None;
                self.status = if e.unreachable { AuthStatus::Offline } else { AuthStatus::Failed };
                self.error = Some(e.message.clone());
                self.notice = Some(SignInNotice { source: finished.source, identity: None, outcome: Err(e.message) });
            }
        }
        change
    }
}

/// What the user is told when the API cannot be reached.
const UNREACHABLE: &str = "Could not reach Eustress. Studio works offline; sign in again when you are back online.";

/// What the user is told when a sign-out of every device could not reach the
/// API (the token lives 72 hours).
const REVOKE_FAILED: &str = "Signed out here. Eustress could not be reached to end the session on your other devices; it ends on its own within 72 hours.";

/// What the user is told when the API is reached but not answering.
const NOT_ANSWERING: &str = "Eustress is not answering right now. Studio works offline; sign in again later.";

/// Short timeouts, since the user waits on a sign-in even off the frame;
/// and no redirects, so a sign-in only ever reaches the API it names.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(4))
        .timeout(std::time::Duration::from_secs(8))
        .redirects(0)
        .build()
}

/// A challenge Studio signs: exactly 64 hex digits, the API's 32 random
/// bytes. Any other text an API sends is never signed.
fn is_challenge(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The keys of an identity file, checked against each other. Only the
/// public key and the signing key are kept. The signing key wipes itself
/// when dropped; the file's text and the parsed copies of the key are freed
/// without wiping, so in memory the key is as exposed as in the file itself.
struct IdentityKey {
    /// 64 lower-case hex digits.
    public_hex: String,
    signing: ed25519_dalek::SigningKey,
}

impl IdentityKey {
    fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read the identity file: {e}"))?;
        Self::parse(&text)
    }

    /// The `[identity]` table's `public_key` and `private_key`, or the same
    /// keys at the top level (an older file).
    fn parse(text: &str) -> Result<Self, String> {
        fn field<'a>(doc: &'a toml::Table, name: &str) -> Option<&'a str> {
            doc.get("identity")
                .and_then(|t| t.get(name))
                .or_else(|| doc.get(name))
                .and_then(|v| v.as_str())
                .map(str::trim)
        }
        let doc: toml::Table = toml::from_str(text).map_err(|_| "This is not an identity file.".to_string())?;
        let public_hex = field(&doc, "public_key").ok_or("This identity file has no public key.")?.to_ascii_lowercase();
        let private =
            field(&doc, "private_key").ok_or("This identity file has no private key, so it cannot sign in.")?;
        let public = hex32(&public_hex).ok_or("This identity file's public key is not valid.")?;
        let secret = hex32(private).ok_or("This identity file's private key is not valid.")?;
        let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
        if signing.verifying_key().to_bytes() != public {
            return Err("This identity file's keys do not match.".to_string());
        }
        Ok(Self { public_hex, signing })
    }

    /// The signature (128 hex digits) on the login message for the API at
    /// `origin` (`eustress_common::api_base::login_message`).
    fn sign_login(&self, origin: &str, challenge: &str) -> String {
        use ed25519_dalek::Signer;
        hex::encode(self.signing.sign(login_message(origin, challenge).as_bytes()).to_bytes())
    }
}

/// 32 bytes from exactly 64 hex digits.
fn hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    hex::decode(text).ok()?.try_into().ok()
}

/// Sign in with the identity file at `path`: the API's challenge, signed
/// together with the API's own origin, for the API's token and account.
fn sign_in(path: &Path) -> Result<SignedIn, SignInError> {
    let key = IdentityKey::read(path).map_err(|why| SignInError::refused(why))?;
    let origin = api_origin();
    // A base with no web origin (never one `api_base` accepts) names no API,
    // so nothing is sent for it.
    if origin == "null" {
        return Err(SignInError::refused("The API address is not valid."));
    }
    let agent = agent();
    let reply = agent
        .post(&api_url("/api/auth/challenge"))
        .send_json(serde_json::json!({ "public_key": key.public_hex.as_str() }))
        .map_err(api_error)?;
    let body: serde_json::Value =
        reply.into_json().map_err(|_| SignInError::refused("Eustress sent an unreadable challenge."))?;
    let challenge = body["challenge"]
        .as_str()
        .filter(|c| is_challenge(c))
        .ok_or_else(|| SignInError::refused("Eustress sent a challenge Studio does not sign."))?
        .to_string();
    let reply = agent
        .post(&api_url("/api/auth/verify-challenge"))
        .send_json(serde_json::json!({
            "public_key": key.public_hex.as_str(),
            "challenge": challenge.as_str(),
            "signature": key.sign_login(&origin, &challenge),
            "format": LOGIN_FORMAT,
        }))
        .map_err(api_error)?;
    let body: serde_json::Value =
        reply.into_json().map_err(|_| SignInError::refused("Eustress sent an unreadable answer."))?;
    let token = body["token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| SignInError::refused("Eustress sent no token."))?
        .to_string();
    let user = user_from_json(&body["user"]).ok_or_else(|| SignInError::refused("Eustress sent an incomplete account."))?;
    Ok(SignedIn { token, user, public_key: Some(key.public_hex.clone()) })
}

/// The words for an identity the API in use does not know, with where to get
/// one for it. `server` is the API as [`api_label`] names it (`None` for
/// production), and `site` the website that goes with it ([`site_base`]).
fn not_registered(server: Option<&(String, String)>, site: Option<&str>) -> String {
    match server {
        None => format!(
            "This identity isn't registered with Eustress. Sign up at {}/login to get one.",
            site_name(site.unwrap_or(PRODUCTION_SITE))
        ),
        Some((name, _)) if name == "staging" => "This identity isn't registered on staging. Staging has its own accounts; \
                                                use the identity file from your staging sign-up."
            .to_string(),
        Some((name, host)) => format!(
            "This identity isn't registered on {name} ({host}). It has its own accounts; use an identity file registered there."
        ),
    }
}

/// The sign-in dialog's two server lines: which server it signs in to, when
/// that is not production (empty for production), and where an identity file
/// for it comes from. The API is chosen as Studio starts, so they hold for the
/// session.
pub fn sign_in_texts() -> (String, String) {
    sign_in_texts_for(api_label().as_ref(), site_base().as_deref())
}

fn sign_in_texts_for(server: Option<&(String, String)>, site: Option<&str>) -> (String, String) {
    let note = server.map(|(name, host)| format!("Signing in to {name} ({host}).")).unwrap_or_default();
    let from = match (server, site) {
        (None, site) => format!("Get one at {}/login.", site_name(site.unwrap_or(PRODUCTION_SITE))),
        (Some(_), Some(site)) => format!("Get one at {}/login.", site_name(site)),
        (Some(_), None) => "Get one where you signed up for this server.".to_string(),
    };
    (note, format!("Your eustress-username.toml holds your Ed25519 keypair. {from} Never share your private key."))
}

/// A website as a person types it: no scheme, no trailing slash.
fn site_name(site: &str) -> &str {
    let site = site.trim().trim_end_matches('/');
    site.strip_prefix("https://").or_else(|| site.strip_prefix("http://")).unwrap_or(site)
}

/// What a failed request tells the user.
fn api_error(error: ureq::Error) -> SignInError {
    match error {
        ureq::Error::Status(404, _) => SignInError::refused(not_registered(api_label().as_ref(), site_base().as_deref())),
        ureq::Error::Status(401, _) => SignInError::refused("Eustress did not accept this identity's signature."),
        ureq::Error::Status(code, _) if code >= 500 => {
            SignInError { message: NOT_ANSWERING.to_string(), unreachable: true }
        }
        ureq::Error::Status(code, reply) => {
            let body: serde_json::Value = reply.into_json().unwrap_or_default();
            // The API's own words (a suspension and its reason, for one).
            let said = match (body["error"].as_str(), body["reason"].as_str()) {
                (Some(error), Some(reason)) => format!("{error}: {reason}"),
                (Some(text), None) | (None, Some(text)) => text.to_string(),
                (None, None) => String::new(),
            };
            let said: String = said.chars().filter(|c| !c.is_control()).take(200).collect();
            if said.trim().is_empty() {
                SignInError::refused(format!("Eustress could not sign you in (error {code})."))
            } else {
                SignInError::refused(said)
            }
        }
        ureq::Error::Transport(_) => SignInError { message: UNREACHABLE.to_string(), unreachable: true },
    }
}

/// The account as the API describes it (`/api/auth/me`, and the `user` of a
/// sign-in). `None` without an id and a name.
fn user_from_json(json: &serde_json::Value) -> Option<AuthUser> {
    let id = json["id"].as_str().filter(|s| !s.is_empty())?.to_string();
    let username = json["username"].as_str().filter(|s| !s.is_empty())?.to_string();
    Some(AuthUser {
        id,
        username,
        email: json["email"].as_str().map(str::to_string),
        avatar_url: json["avatar_url"].as_str().map(str::to_string),
        steam_id: json["steam_id"].as_str().map(str::to_string),
        discord_id: json["discord_id"].as_str().map(str::to_string),
        bliss_balance: json["bliss_balance"].as_f64().unwrap_or(0.0),
        total_hours: json["total_hours"].as_f64().unwrap_or(0.0),
    })
}

/// What the API says of a saved token.
enum TokenCheck {
    Valid(AuthUser),
    /// Refused: it expired, or its account signed out everywhere.
    Expired,
    Failed(SignInError),
}

fn check_token(token: &str) -> TokenCheck {
    let reply = match agent().get(&api_url("/api/auth/me")).set("Authorization", &format!("Bearer {token}")).call() {
        Ok(reply) => reply,
        Err(ureq::Error::Status(401, _)) => return TokenCheck::Expired,
        Err(e) => return TokenCheck::Failed(api_error(e)),
    };
    match reply.into_json::<serde_json::Value>().ok().as_ref().and_then(user_from_json) {
        Some(user) => TokenCheck::Valid(user),
        None => TokenCheck::Failed(SignInError::refused("Eustress sent an incomplete account.")),
    }
}

/// Startup, on the sign-in thread: the saved token, then the identity file.
/// Returns the outcome, the identity file it signed in with, and whether to
/// forget the saved token.
fn restore_session(
    saved: Option<String>,
    identity: Option<PathBuf>,
) -> (Result<SignedIn, SignInError>, Option<PathBuf>, bool) {
    let mut expired = false;
    if let Some(token) = saved {
        match check_token(&token) {
            TokenCheck::Valid(user) => return (Ok(SignedIn { token, user, public_key: None }), None, false),
            TokenCheck::Failed(e) => return (Err(e), None, false),
            TokenCheck::Expired => expired = true,
        }
    }
    match identity {
        Some(path) => (sign_in(&path), Some(path), expired),
        None => (Err(SignInError::refused("Your sign-in expired. Sign in again.")), None, expired),
    }
}

/// Where the saved login lives: one file per API, so a login is only ever
/// sent back to the API that issued it. A Studio pointed at a test API
/// (`EUSTRESS_API_URL`) never loads production's saved login.
fn get_token_path() -> Option<std::path::PathBuf> {
    Some(dirs::data_local_dir()?.join("EustressEngine").join(eustress_common::api_base::login_file()))
}

/// Save the login to disk where only this user can read it: the folder and
/// the file each get their own access list (this user and, on Windows,
/// SYSTEM; 0700 and 0600 on Unix), checked before the token is written, and
/// the file is renamed into place ([`crate::private_file::write_private`]).
/// An inherited list can grant another account read, as one sandbox group an
/// AI agent runs as had on this folder. This is the one write path for every
/// login file (production's and each other API's). A login that cannot be
/// saved so lasts for this session only.
fn save_token(token: &str) {
    let Some(path) = get_token_path() else { return };
    if let Err(e) = save_token_to(&path, token) {
        warn!("auth: the login was not saved ({e}); it lasts for this session only");
    }
}

/// [`save_token`] into `path`.
fn save_token_to(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    crate::private_file::write_private(path, token.as_bytes())
}

/// Make the login files saved before they were private private, once, at
/// startup ([`AuthState::restore`], before anything reads or saves one):
/// every login file in Studio's folder (`auth_token`, and `auth_token.<scope>`
/// for each other API) that another account could open is written back
/// through `write_private`, so a token an inherited list exposed is not left
/// readable while its owner stays signed in. One line each goes to the log.
/// A file that cannot be made private is kept as it was, never deleted.
fn reprotect_saved_logins() {
    let Some(dir) = get_token_path().and_then(|path| path.parent().map(std::path::Path::to_path_buf)) else { return };
    for (name, outcome) in reprotect_logins_in(&dir) {
        match outcome {
            Ok(LoginRepair::Private) => info!("auth: the saved login {name} is private"),
            Ok(LoginRepair::Adopted) => info!("auth: the saved login {name} was made private"),
            Ok(LoginRepair::Removed { owner }) => warn!(
                "auth: the saved login {name} belonged to another account ({owner}) and was deleted; sign in again"
            ),
            Err(e) => warn!("auth: the saved login {name} could not be made private ({e}); it is kept as it was"),
        }
    }
}

/// What startup does with a saved login file, from [`crate::private_file::verify`]
/// on its folder and then the file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LoginRepair {
    /// Private already: left untouched.
    Private,
    /// Not private, but this user's (or made by an elevated run of this
    /// user, owned by Administrators or SYSTEM): rewritten privately, its
    /// token kept.
    Adopted,
    /// Owned by another account: deleted, never adopted, so a login file
    /// someone else planted cannot become this user's session. They sign in
    /// again.
    Removed { owner: String },
}

/// See [`LoginRepair`]: what `checked` (verify's result) calls for.
fn login_repair(checked: &std::io::Result<()>) -> LoginRepair {
    match checked {
        Ok(()) => LoginRepair::Private,
        Err(e) => match crate::private_file::not_owner(e) {
            Some(owner) if !owner.trusted => LoginRepair::Removed { owner: owner.owner.clone() },
            _ => LoginRepair::Adopted,
        },
    }
}

/// [`reprotect_saved_logins`] in `dir`: each login file's name and what
/// became of it ([`LoginRepair`]). One already private is left untouched.
fn reprotect_logins_in(dir: &std::path::Path) -> Vec<(String, std::io::Result<LoginRepair>)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_login_file(name))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let path = dir.join(&name);
            let checked = crate::private_file::verify(dir).and_then(|()| crate::private_file::verify(&path));
            let outcome = match login_repair(&checked) {
                LoginRepair::Private => Ok(LoginRepair::Private),
                LoginRepair::Adopted => std::fs::read(&path)
                    .and_then(|bytes| crate::private_file::write_private(&path, &bytes))
                    .map(|()| LoginRepair::Adopted),
                LoginRepair::Removed { owner } => std::fs::remove_file(&path).map(|()| LoginRepair::Removed { owner }),
            };
            (name, outcome)
        })
        .collect()
}

/// A name [`eustress_common::api_base::login_file`] gives: `auth_token`, or
/// `auth_token.` and the 16 hex digits of another API's scope.
fn is_login_file(name: &str) -> bool {
    name == "auth_token"
        || name
            .strip_prefix("auth_token.")
            .is_some_and(|scope| scope.len() == 16 && scope.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// This API's saved token, if there is one.
fn load_saved_token() -> Option<String> {
    let path = get_token_path()?;
    let token = std::fs::read_to_string(path).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Apply the sign-ins that finished on their threads, and save or remove
/// this API's login file as they say.
pub fn auth_poll_system(mut auth_state: ResMut<AuthState>) {
    // Read through `Deref` first, so a frame with nothing finished leaves
    // the resource unchanged.
    let finished: Vec<SignInFinished> = match auth_state.finished.try_lock() {
        Ok(mut f) if !f.is_empty() => std::mem::take(&mut *f),
        _ => return,
    };
    for result in finished {
        let change = auth_state.apply(result);
        if change.forget {
            if let Some(path) = get_token_path() {
                let _ = std::fs::remove_file(path);
            }
        }
        if let Some(token) = change.save {
            save_token(&token);
        }
    }
}

// ============================================================================
// Bliss node lifecycle
// ============================================================================
//
// Earning is handled by `crate::bliss_tracker` (BlissTrackerPlugin): real
// work events (scene edits, script edits, active time) accumulate into
// contribution buckets, get co-signed by the witness at api.eustress.dev,
// and the authoritative ledger balance flows back into `BlissNodeState`
// via the node heartbeat. This module keeps only auth + node startup.

/// Bliss node handle — holds the running API server task.
#[derive(Resource)]
pub struct BlissNodeHandle {
    pub port: u16,
    pub running: bool,
}

impl Default for BlissNodeHandle {
    fn default() -> Self {
        Self {
            port: 0,
            running: false,
        }
    }
}

/// Plugin for the auth system + Bliss node.
pub struct StudioAuthPlugin;

impl Plugin for StudioAuthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AuthState>()
            .init_resource::<BlissNodeState>()
            .init_resource::<BlissNodeHandle>()
            // `.chain()` matters: the saved mode must be applied BEFORE the
            // node starts, otherwise a user who chose Full would boot a Light
            // node for the session and only see the label flip.
            .add_systems(Startup, (restore_bliss_settings, start_bliss_node).chain());
        // NOTE: `auth_poll_system` is registered by `SlintUiPlugin`
        // (ui/slint_ui.rs), which always coexists with this plugin in
        // the studio bin — adding it here too would run it twice per
        // frame. Earning systems live in `BlissTrackerPlugin`.
    }
}

/// Restore the persisted Bliss node mode + enabled flag from EditorSettings.
///
/// Without this the badge reset to Light on every launch, silently discarding
/// an opt-in to Full (and its +10% earning bonus). Runs before
/// `start_bliss_node` so the node actually boots in the chosen mode.
fn restore_bliss_settings(
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
    mut bliss: ResMut<BlissNodeState>,
) {
    let Some(settings) = settings else { return };
    if settings.bliss_node_mode == "Full" {
        bliss.set_full();
    } else {
        bliss.set_light();
    }
    bliss.enabled = settings.bliss_enabled;
    info!(
        "Bliss: restored node mode {} (enabled: {})",
        bliss.mode, bliss.enabled
    );
}

/// Start the Bliss node API server on engine startup.
fn start_bliss_node(
    mut handle: ResMut<BlissNodeHandle>,
    bliss_state: Res<BlissNodeState>,
    args: Option<Res<crate::startup::StartupArgs>>,
) {
    if !bliss_state.enabled {
        info!("Bliss node disabled — skipping startup");
        return;
    }

    let mode_str = bliss_state.mode.clone();
    let mode = if mode_str == "Full" {
        eustress_bliss::NodeMode::Full
    } else {
        eustress_bliss::NodeMode::Light
    };

    let cli = args.as_ref().and_then(|a| a.bliss_bind.clone());
    let from_env = std::env::var("EUSTRESS_BLISS_BIND").ok();
    let bind = match eustress_bliss::resolve_bind(cli.as_deref(), from_env.as_deref()) {
        Ok(ip) => ip,
        Err(e) => {
            eprintln!("🔴 {e}");
            return;
        }
    };

    let port = 7777u16;

    println!("🟣 Starting Bliss {} on {bind}:{port}...", mode);
    println!("🟣 Registration: {} (Cloudflare)", api_base());
    println!("🟣 This node: co-signing + identity verification");

    // Spawn the async node startup on a background thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime for Bliss node");

        rt.block_on(async {
            let mut node = eustress_bliss::BlissNode::new(eustress_bliss::NodeConfig {
                mode,
                api_port: port,
                bind_addr: bind.to_string(),
                ..eustress_bliss::NodeConfig::default()
            });

            match node.start().await {
                Ok(addr) => {
                    // `addr` is the socket's local address, so this line
                    // cannot name a different interface than the one bound.
                    println!(
                        "🟣 Bliss node API running on {}",
                        eustress_bliss::listener_log_line(addr)
                    );
                    // Keep the runtime alive so the server keeps running
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    }
                }
                Err(e) => {
                    eprintln!("🔴 Failed to start Bliss node: {}", e);
                }
            }
        });
    });

    handle.port = port;
    handle.running = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared login-v1 vectors, read by the Worker's tests too.
    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../../docs/architecture/login-v1-vectors.json")).unwrap()
    }

    fn identity_file(public: &str, private: &str) -> String {
        format!("[identity]\nversion = \"1.0\"\nusername = \"Vector\"\npublic_key = \"{public}\"\nprivate_key = \"{private}\"\n")
    }

    fn account() -> AuthUser {
        user_from_json(&serde_json::json!({ "id": "acct-1", "username": "Ada" })).unwrap()
    }

    fn finished(state: &AuthState, outcome: Result<SignedIn, SignInError>) -> SignInFinished {
        SignInFinished {
            attempt: state.attempt,
            source: SignInSource::Dialog,
            identity: Some(PathBuf::from("identity.toml")),
            outcome,
            forget_saved: false,
        }
    }

    /// Studio's signature on each vector's message is the vector's own (an
    /// Ed25519 signature is deterministic), and it signs in to no other API.
    /// The Worker's tests check the same file.
    #[test]
    fn login_signatures_match_the_shared_vectors() {
        use ed25519_dalek::{Signature, Verifier};
        let v = vectors();
        let key = IdentityKey::parse(&identity_file(v["public_key"].as_str().unwrap(), v["secret_key"].as_str().unwrap()))
            .unwrap();
        let signatures = v["signatures"].as_array().unwrap();
        for case in signatures {
            let (origin, challenge) = (case["origin"].as_str().unwrap(), case["challenge"].as_str().unwrap());
            assert!(is_challenge(challenge), "Studio signs the vector's challenge");
            let signed = key.sign_login(origin, challenge);
            assert_eq!(signed, case["signature"].as_str().unwrap(), "{origin}");
            let signature = Signature::from_slice(&hex::decode(&signed).unwrap()).unwrap();
            for other in signatures.iter().filter(|o| o["origin"] != case["origin"]) {
                let message = login_message(other["origin"].as_str().unwrap(), challenge);
                assert!(
                    key.signing.verifying_key().verify(message.as_bytes(), &signature).is_err(),
                    "a signature for {origin} signs in nowhere else"
                );
            }
        }
    }

    /// A bad identity file is turned away before anything is sent.
    #[test]
    fn identity_files_are_checked_before_anything_is_sent() {
        let v = vectors();
        let (public, secret) = (v["public_key"].as_str().unwrap(), v["secret_key"].as_str().unwrap());
        assert!(IdentityKey::parse(&identity_file(public, secret)).is_ok());
        assert!(
            IdentityKey::parse(&format!("public_key = \"{public}\"\nprivate_key = \"{secret}\"\n")).is_ok(),
            "an older file, its keys at the top level"
        );
        let other_public = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
        let cases = [
            (identity_file(other_public, secret), "do not match"),
            (format!("[identity]\npublic_key = \"{public}\"\n"), "no private key"),
            (identity_file(public, "zz"), "private key is not valid"),
            (identity_file(&public[..62], secret), "public key is not valid"),
            ("not = [toml".to_string(), "not an identity file"),
        ];
        for (text, why) in cases {
            let err = IdentityKey::parse(&text).err().expect(why);
            assert!(err.contains(why), "{err:?} should say {why:?}");
        }
    }

    /// Signed in means the API issued the token: a failed sign-in leaves
    /// Studio signed out with no token to offer, and a successful one is
    /// saved for this API and counts for Bliss.
    #[test]
    fn a_failed_sign_in_is_signed_out() {
        let mut state = AuthState::default();
        state.attempt = 1;
        let change = state.apply(finished(&state, Err(SignInError::refused("This identity is not registered with Eustress."))));
        assert_eq!(change, LoginFileChange::default(), "nothing saved");
        assert!(!state.is_logged_in() && state.get_token().is_none() && !state.jwt_valid);
        assert!(state.status == AuthStatus::Failed);
        assert!(matches!(state.take_notice(), Some(SignInNotice { outcome: Err(_), .. })));

        let change = state.apply(finished(&state, Err(SignInError { message: UNREACHABLE.into(), unreachable: true })));
        assert!(state.status == AuthStatus::Offline && state.is_offline());
        assert!(state.get_token().is_none() && change.save.is_none());

        let public_key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a".to_string();
        let change = state.apply(finished(
            &state,
            Ok(SignedIn { token: "jwt".into(), user: account(), public_key: Some(public_key.clone()) }),
        ));
        assert_eq!(change.save.as_deref(), Some("jwt"), "saved for the Player");
        assert!(state.is_logged_in() && state.jwt_valid);
        assert_eq!(state.get_token(), Some("jwt"));
        let notice = state.take_notice().unwrap();
        assert_eq!(notice.outcome, Ok("Ada".to_string()), "the account's name, from the API");
        assert_eq!(notice.identity, Some((PathBuf::from("identity.toml"), public_key)));
        assert!(state.take_notice().is_none(), "shown once");
    }

    /// A sign-in that was superseded (another began, or the user signed out)
    /// changes nothing when it lands.
    #[test]
    fn a_superseded_sign_in_changes_nothing() {
        let mut state = AuthState::default();
        state.attempt = 2;
        let mut late = finished(&state, Ok(SignedIn { token: "old".into(), user: account(), public_key: None }));
        late.attempt = 1;
        assert_eq!(state.apply(late), LoginFileChange::default());
        assert!(!state.is_logged_in() && state.token.is_none() && !state.has_notice());
    }

    /// A sign-out that could not reach the API is told once.
    #[test]
    fn a_failed_sign_out_everywhere_is_told_once() {
        let state = AuthState::default();
        assert!(state.take_warning().is_none());
        *state.revoke_warning.lock().unwrap() = Some(REVOKE_FAILED.to_string());
        assert_eq!(state.take_warning().as_deref(), Some(REVOKE_FAILED));
        assert!(state.take_warning().is_none(), "once");
    }

    /// Only a 64-hex challenge is signed: never shorter, longer or other text.
    #[test]
    fn only_a_challenge_is_signed() {
        assert!(is_challenge(&"0f".repeat(32)));
        for text in ["0f".repeat(31) + "0", "0f".repeat(33), "zz".repeat(32), "eustress-login-v1".to_string(), String::new()] {
            assert!(!is_challenge(&text), "{text:?}");
        }
    }

    /// The account comes from the API: without an id and a name there is none.
    #[test]
    fn an_account_needs_an_id_and_a_name() {
        assert!(user_from_json(&serde_json::json!({ "id": "acct-1" })).is_none());
        assert!(user_from_json(&serde_json::json!({ "username": "Ada" })).is_none());
        assert_eq!(account().username, "Ada");
    }

    /// The words for an unknown identity, and the dialog's server lines, name
    /// the server in use: production points at its sign-up, staging says it
    /// has its own accounts, and any other server names its host.
    #[test]
    fn sign_in_words_name_the_server() {
        let staging = ("staging".to_string(), "api-staging.eustress.dev".to_string());
        let local = ("the local server".to_string(), "localhost:8787".to_string());
        assert_eq!(
            not_registered(None, Some("https://eustress.dev")),
            "This identity isn't registered with Eustress. Sign up at eustress.dev/login to get one."
        );
        assert_eq!(
            not_registered(Some(&staging), Some("https://staging.eustress-cbb.pages.dev")),
            "This identity isn't registered on staging. Staging has its own accounts; \
             use the identity file from your staging sign-up."
        );
        assert!(not_registered(Some(&local), None).contains("the local server (localhost:8787)"));

        let (note, hint) = sign_in_texts_for(None, Some("https://eustress.dev"));
        assert_eq!(note, "", "production says nothing about the server");
        assert!(hint.contains("Get one at eustress.dev/login."), "{hint}");
        let (note, hint) = sign_in_texts_for(Some(&staging), Some("https://staging.eustress-cbb.pages.dev/"));
        assert_eq!(note, "Signing in to staging (api-staging.eustress.dev).");
        assert!(hint.contains("Get one at staging.eustress-cbb.pages.dev/login."), "{hint}");
        let (note, hint) = sign_in_texts_for(Some(&local), None);
        assert_eq!(note, "Signing in to the local server (localhost:8787).");
        assert!(hint.contains("where you signed up for this server") && !hint.contains("eustress.dev"), "{hint}");
    }

    /// A saved login is private: only this user (and, on Windows, SYSTEM)
    /// can open the folder or the file, and the file holds the token.
    #[test]
    fn a_saved_login_is_private() {
        let dir = std::env::temp_dir().join(format!("eustress-login-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("auth_token.test");
        save_token_to(&path, "jwt-one").unwrap();
        save_token_to(&path, "jwt-two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "jwt-two", "replaced, not appended");
        crate::private_file::verify(&dir).unwrap();
        crate::private_file::verify(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Login files saved before they were private are made private at
    /// startup, their tokens unchanged; anything else in the folder is left
    /// alone, and a second pass changes nothing.
    #[test]
    fn startup_makes_old_login_files_private() {
        let dir = std::env::temp_dir().join(format!("eustress-login-reprotect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Written the old way: whatever list the folder hands down.
        std::fs::write(dir.join("auth_token"), "jwt-production").unwrap();
        std::fs::write(dir.join("auth_token.0123456789abcdef"), "jwt-staging").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a login").unwrap();

        let done = reprotect_logins_in(&dir);
        let names: Vec<&str> = done.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["auth_token", "auth_token.0123456789abcdef"]);
        assert!(done.iter().all(|(_, outcome)| outcome.is_ok()), "{done:?}");
        crate::private_file::verify(&dir).unwrap();
        crate::private_file::verify(&dir.join("auth_token")).unwrap();
        crate::private_file::verify(&dir.join("auth_token.0123456789abcdef")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("auth_token")).unwrap(), "jwt-production");
        assert_eq!(std::fs::read_to_string(dir.join("auth_token.0123456789abcdef")).unwrap(), "jwt-staging");
        assert_eq!(std::fs::read_to_string(dir.join("notes.txt")).unwrap(), "not a login");
        assert!(reprotect_logins_in(&dir).iter().all(|(_, outcome)| outcome.is_ok()), "again: nothing to do");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A login file another account owns is deleted, never adopted; one an
    /// elevated run of this user made (Administrators or SYSTEM) is adopted,
    /// as is one with only a wrong access list.
    #[test]
    fn a_login_file_another_account_owns_is_deleted_not_adopted() {
        use crate::private_file::NotOwner;
        let owned_by = |owner: &str, trusted: bool| -> std::io::Result<()> {
            Err(NotOwner { path: "auth_token".into(), owner: owner.into(), trusted, advice: String::new() }.into())
        };
        assert_eq!(login_repair(&Ok(())), LoginRepair::Private);
        assert_eq!(
            login_repair(&owned_by("S-1-5-21-1-2-3-1001", false)),
            LoginRepair::Removed { owner: "S-1-5-21-1-2-3-1001".into() }
        );
        assert_eq!(login_repair(&owned_by("S-1-5-32-544", true)), LoginRepair::Adopted, "Administrators");
        assert_eq!(login_repair(&owned_by("S-1-5-18", true)), LoginRepair::Adopted, "SYSTEM");
        let wrong_list = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "it inherits access from its folder");
        assert_eq!(login_repair(&Err(wrong_list)), LoginRepair::Adopted, "a wrong list alone");
    }

    /// Only the names login_file gives are login files.
    #[test]
    fn only_login_files_are_reprotected() {
        assert!(is_login_file("auth_token"));
        assert!(is_login_file("auth_token.0123456789abcdef"));
        for other in ["auth_token.tmp", "auth_token.0123", "auth_token.0123456789abcdeg", "auth_tokens", "settings.toml"] {
            assert!(!is_login_file(other), "{other}");
        }
    }
}
