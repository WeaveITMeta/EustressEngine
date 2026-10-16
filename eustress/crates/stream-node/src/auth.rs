//! Login, enrollment and authorization shared by every network listener.
//!
//! The node authenticates each party by the Ed25519 public key enrolled for it
//! in `parties.toml`, and authorizes it per topic, default deny. This module
//! holds what every transport shares:
//!
//! - the login transcripts for TCP, QUIC and REST, each with its own label, and
//!   every variable-length field preceded by a two-byte little-endian length;
//! - the parties loader and the live party table, re-read every second;
//! - the pre-login limiter (128 connections, 16 per source IP) and the warn
//!   lines for failed logins, rate-limited per source IP;
//! - the registry of live sessions, the per-party subscription quota, the
//!   REST nonce table (challenges, looked up by nonce, single use) and the
//!   REST token table;
//! - [`NodeAuth`], which ties them together, runs the startup checks, and runs
//!   the once-a-second reload and revocation check.
//!
//! Every per-address limit counts under [`limit_key`]: an IPv6 address by its
//! /64, an IPv4-mapped address as IPv4. Every session, token and stream
//! records the fingerprint of its party's key, so a reload that finds another
//! key under the same id ends them all.
//!
//! Reserved topics: `scene_deltas` and `agent_commands` also require
//! `admin = true` to publish. The admin flag decides who may publish a scene
//! delta at all. The materializer that writes a Space to disk keeps its own
//! path checks (`space://` containment) on whatever a delta asks it to write.
//! Both checks apply, and neither replaces the other.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::Hash;
use std::io::Read as _;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use parking_lot::{Mutex, RwLock};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;
use tracing::{error, info, warn};

use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::transport::{sha256, NodeTls};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// The login protocol version this node speaks. `Hello` carries it.
pub const PROTOCOL_VERSION: u16 = 1;

/// Transcript label for TCP logins. The TLS exporter uses the same label.
pub const TCP_LABEL: &[u8] = b"eustress-stream-login-v1";
/// Transcript label for QUIC logins. The QUIC exporter uses the same label.
pub const QUIC_LABEL: &[u8] = b"eustress-stream-login-quic-v1";
/// Transcript label for REST logins.
pub const REST_LABEL: &[u8] = b"eustress-stream-login-rest-v1";

/// Length of a login nonce.
pub const NONCE_LEN: usize = 32;
/// Length of a channel binding: an exporter value or a certificate hash.
pub const BINDING_LEN: usize = 32;
/// Longest node id a transcript carries, in bytes.
pub const MAX_NODE_ID_BYTES: usize = 64;
/// Longest party id, in bytes. The shortest is one byte.
pub const MAX_PARTY_ID_BYTES: usize = 64;

/// Frames read before `Welcome` are capped at 64 KB.
pub const PRE_LOGIN_MAX_FRAME: usize = 64 * 1024;
/// A login must finish this long after accept. The TLS handshake counts.
pub const LOGIN_DEADLINE: Duration = Duration::from_secs(5);
/// Connections that may be pre-login at once.
pub const MAX_PRE_LOGIN_TOTAL: usize = 128;
/// Connections from one source IP that may be pre-login at once.
pub const MAX_PRE_LOGIN_PER_IP: usize = 16;

/// REST challenges that may be outstanding from one source IP.
pub const MAX_CHALLENGES_PER_IP: usize = 16;
/// REST challenges that may be outstanding in all.
pub const MAX_CHALLENGES_TOTAL: usize = 4096;
/// A REST challenge is valid this long, and for one login.
pub const CHALLENGE_LIFETIME: Duration = Duration::from_secs(5);

/// Bearer tokens the node holds at once. A login is refused when it is full.
pub const TOKEN_TABLE_MAX: usize = 4096;
/// Live bearer tokens one party may hold. A login past it is refused with
/// 429 and `Retry-After: 1`, for that party only.
pub const MAX_TOKENS_PER_PARTY: usize = 32;
/// Lifetime of a `session` token.
pub const SESSION_TOKEN_LIFETIME: Duration = Duration::from_secs(60 * 60);
/// Lifetime of a `browser` token.
pub const BROWSER_TOKEN_LIFETIME: Duration = Duration::from_secs(15 * 60);

/// How often the node re-reads `parties.toml` and applies revocations.
pub const RELOAD_INTERVAL: Duration = Duration::from_secs(1);
/// One failed-login warn line per source IP per window.
pub const WARN_WINDOW: Duration = Duration::from_secs(10);
/// Source IPs the warn limiter tracks before it stops admitting new ones.
const WARN_TABLE_MAX: usize = 4096;

/// Refused fire-and-forget publishes (`PublishNoAck`, `PublishBatchNoAck`) one
/// connection may get inside one window. The next one closes the connection.
pub const MAX_NO_ACK_REFUSALS: u32 = 100;
/// That window: fixed, 60 seconds, starting at the connection's first refused
/// fire-and-forget publish. The sender reads no reply to those frames, so this
/// is the only bound on a party that keeps sending refused ones.
pub const NO_ACK_REFUSAL_WINDOW: Duration = Duration::from_secs(60);

/// Every write to a peer must finish within this long, waiting for the
/// writer included. A write that does not closes the connection, so a peer
/// that stops reading cannot hold a slot, a subscription or a party's quota
/// open.
pub const WRITE_DEADLINE: Duration = Duration::from_secs(10);

/// `max_subscriptions` when a party entry does not set it.
pub const DEFAULT_MAX_SUBSCRIPTIONS: usize = 8;
/// Topics that also require `admin = true` to publish.
pub const RESERVED_TOPICS: [&str; 2] = ["scene_deltas", "agent_commands"];

/// The one refusal text for an unknown party, a malformed `party_id`, a
/// disabled party and a bad signature, so a reply never reveals which parties
/// exist.
pub const REASON_AUTH_FAILED: &str = "auth_failed";
/// The refusal text for a protocol version this node does not speak.
pub const REASON_VERSION_UNSUPPORTED: &str = "version_unsupported";

/// `ServerFrame::Error` code: a frame that makes no sense here.
pub const ERR_BAD_REQUEST: u32 = 400;
/// `ServerFrame::Error` code: the party has no grant for this topic.
pub const ERR_FORBIDDEN: u32 = 403;
/// `ServerFrame::Error` code: the party's subscription quota is used up.
pub const ERR_QUOTA: u32 = 429;
/// `ServerFrame::Error` code: the topic has no subscriber slot left.
pub const ERR_TOPIC_FULL: u32 = 503;

/// A valid Ed25519 public key (RFC 8032, test 1) that no party holds. A login
/// for an unknown or disabled party checks the signature against it, so the
/// refusal costs about what a wrong-key refusal costs.
const TIMING_KEY_B64: &str = "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";

/// A fresh 32-byte nonce from the operating system's generator.
pub fn fresh_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

// ─────────────────────────────────────────────────────────────────────────────
// Transcripts
// ─────────────────────────────────────────────────────────────────────────────

/// The channel a login runs on. Each has its own transcript label, so a
/// signature made for one channel is refused on every other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Tcp,
    Quic,
    Rest,
}

impl Channel {
    /// The transcript label. A `0x00` byte follows it in the transcript.
    pub fn label(self) -> &'static [u8] {
        match self {
            Channel::Tcp => TCP_LABEL,
            Channel::Quic => QUIC_LABEL,
            Channel::Rest => REST_LABEL,
        }
    }

    /// Lower-case name for log lines.
    pub fn name(self) -> &'static str {
        match self {
            Channel::Tcp => "tcp",
            Channel::Quic => "quic",
            Channel::Rest => "rest",
        }
    }
}

/// What ties a login signature to one connection, or to one node.
#[derive(Clone, Copy, Debug)]
pub enum Binding<'a> {
    /// TCP and QUIC: 32 bytes of this connection's TLS 1.3 exporter.
    Exporter(&'a [u8; BINDING_LEN]),
    /// REST: the SHA-256 of the node's DER certificate.
    CertSha256(&'a [u8; BINDING_LEN]),
}

/// The class of a REST bearer token. It is part of the signed REST
/// transcript, so it cannot be changed in transit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenClass {
    /// One hour. For native clients.
    Session,
    /// Fifteen minutes. Minted by a native login and handed to a browser.
    Browser,
}

impl TokenClass {
    /// The transcript byte: `0x01` session, `0x02` browser.
    pub fn byte(self) -> u8 {
        match self {
            TokenClass::Session => 0x01,
            TokenClass::Browser => 0x02,
        }
    }

    /// How long a token of this class is valid.
    pub fn lifetime(self) -> Duration {
        match self {
            TokenClass::Session => SESSION_TOKEN_LIFETIME,
            TokenClass::Browser => BROWSER_TOKEN_LIFETIME,
        }
    }

    /// The JSON spelling: `"session"` or `"browser"`.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenClass::Session => "session",
            TokenClass::Browser => "browser",
        }
    }

    /// Parse the JSON spelling. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "session" => Some(TokenClass::Session),
            "browser" => Some(TokenClass::Browser),
            _ => None,
        }
    }
}

/// `true` for 1 to 64 bytes of `[A-Za-z0-9_-]`.
pub fn is_valid_party_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PARTY_ID_BYTES
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `true` for the topics that also require `admin = true` to publish.
pub fn is_reserved_topic(topic: &str) -> bool {
    RESERVED_TOPICS.contains(&topic)
}

/// Build a login transcript. The signature is Ed25519 over these bytes:
///
/// | field | encoding |
/// |---|---|
/// | label | the channel's ASCII label, then `0x00` |
/// | node_id | u16 LE length, then UTF-8 (at most 64 bytes) |
/// | nonce | 32 bytes |
/// | binding | 32 bytes: the exporter (TCP, QUIC) or the certificate SHA-256 (REST) |
/// | party_id | u16 LE length, then UTF-8 (1 to 64 bytes) |
/// | protocol_version | 2 bytes, LE |
/// | token_class | 1 byte, REST only |
///
/// TCP and QUIC take an exporter and no token class; REST takes a
/// certificate hash and a token class. Any other combination is refused.
pub fn transcript(
    channel: Channel,
    node_id: &str,
    nonce: &[u8; NONCE_LEN],
    binding: Binding<'_>,
    party_id: &str,
    protocol_version: u16,
    token_class: Option<TokenClass>,
) -> Result<Vec<u8>, NodeError> {
    match (channel, binding, token_class) {
        (Channel::Tcp | Channel::Quic, Binding::Exporter(_), None) => {}
        (Channel::Rest, Binding::CertSha256(_), Some(_)) => {}
        _ => {
            return Err(NodeError::Protocol(
                "transcript fields do not match the channel".to_string(),
            ))
        }
    }
    if node_id.len() > MAX_NODE_ID_BYTES {
        return Err(NodeError::Protocol("node id is longer than 64 bytes".to_string()));
    }
    if !is_valid_party_id(party_id) {
        return Err(NodeError::Protocol("party id is malformed".to_string()));
    }

    let mut out = Vec::with_capacity(48 + node_id.len() + NONCE_LEN + BINDING_LEN + party_id.len());
    out.extend_from_slice(channel.label());
    out.push(0x00);
    push_prefixed(&mut out, node_id.as_bytes());
    out.extend_from_slice(nonce);
    match binding {
        // TCP and QUIC: the exporter. Without it a relay that terminates TLS
        // on both legs could pass a signature from one leg to the other.
        Binding::Exporter(exporter) => out.extend_from_slice(exporter),
        // REST: the node certificate's hash, as the client saw it.
        Binding::CertSha256(cert_hash) => out.extend_from_slice(cert_hash),
    }
    push_prefixed(&mut out, party_id.as_bytes());
    out.extend_from_slice(&protocol_version.to_le_bytes());
    if let Some(class) = token_class {
        out.push(class.byte());
    }
    Ok(out)
}

/// Append a u16 LE length, then the bytes. Callers bound every field to 64
/// bytes first, so the length always fits.
fn push_prefixed(out: &mut Vec<u8>, field: &[u8]) {
    let len = field.len().min(usize::from(u16::MAX));
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(&field[..len]);
}

/// The TCP transcript: TCP label and the connection's TLS exporter.
pub fn tcp_transcript(
    node_id: &str,
    nonce: &[u8; NONCE_LEN],
    exporter: &[u8; BINDING_LEN],
    party_id: &str,
    protocol_version: u16,
) -> Result<Vec<u8>, NodeError> {
    transcript(Channel::Tcp, node_id, nonce, Binding::Exporter(exporter), party_id, protocol_version, None)
}

/// The QUIC transcript: QUIC label and the QUIC connection's TLS exporter.
pub fn quic_transcript(
    node_id: &str,
    nonce: &[u8; NONCE_LEN],
    exporter: &[u8; BINDING_LEN],
    party_id: &str,
    protocol_version: u16,
) -> Result<Vec<u8>, NodeError> {
    transcript(Channel::Quic, node_id, nonce, Binding::Exporter(exporter), party_id, protocol_version, None)
}

/// The REST transcript: REST label, the SHA-256 of the node's DER
/// certificate, and the token class.
pub fn rest_transcript(
    node_id: &str,
    nonce: &[u8; NONCE_LEN],
    cert_sha256: &[u8; BINDING_LEN],
    party_id: &str,
    protocol_version: u16,
    token_class: TokenClass,
) -> Result<Vec<u8>, NodeError> {
    transcript(
        Channel::Rest,
        node_id,
        nonce,
        Binding::CertSha256(cert_sha256),
        party_id,
        protocol_version,
        Some(token_class),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// parties.toml
// ─────────────────────────────────────────────────────────────────────────────

fn default_max_subscriptions() -> usize {
    DEFAULT_MAX_SUBSCRIPTIONS
}

/// One `[[party]]` entry of `parties.toml`, as written.
///
/// A missing `enabled` reads as `false`: a party logs in only when its entry
/// says `enabled = true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyEntry {
    /// 1 to 64 characters of `[A-Za-z0-9_-]`.
    pub id: String,
    /// Base64 Ed25519 public key from the party's identity file.
    pub public_key: String,
    #[serde(default)]
    pub enabled: bool,
    /// Also required to publish to a reserved topic.
    #[serde(default)]
    pub admin: bool,
    /// Simultaneous subscriptions this party may hold on the node.
    #[serde(default = "default_max_subscriptions")]
    pub max_subscriptions: usize,
    /// Topics this party may subscribe to. Nothing else.
    #[serde(default)]
    pub subscribe: Vec<String>,
    /// Topics this party may publish to. Nothing else.
    #[serde(default)]
    pub publish: Vec<String>,
}

/// The whole of `parties.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartiesFile {
    #[serde(default, rename = "party")]
    pub parties: Vec<PartyEntry>,
}

impl PartiesFile {
    /// Parse the TOML text of a parties file.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The TOML text for this set of entries.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(self).map_err(|e| e.to_string())
    }

    /// The entry with this id, for editing.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut PartyEntry> {
        self.parties.iter_mut().find(|p| p.id == id)
    }
}

/// A checked party: its id is well formed and its key decodes.
pub struct Party {
    pub id: String,
    pub enabled: bool,
    pub admin: bool,
    pub max_subscriptions: usize,
    /// Canonical base64 of the 32-byte key, for `verify_signed_data`.
    public_key_b64: String,
    /// SHA-256 of the 32-byte key.
    key_fingerprint: [u8; 32],
    subscribe: HashSet<String>,
    publish: HashSet<String>,
}

impl fmt::Debug for Party {
    // The public key is left out on purpose: keys are never logged.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Party")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .field("admin", &self.admin)
            .field("max_subscriptions", &self.max_subscriptions)
            .finish_non_exhaustive()
    }
}

impl Party {
    fn from_entry(entry: &PartyEntry) -> Result<Self, String> {
        if !is_valid_party_id(&entry.id) {
            return Err(format!(
                "party id {:?} is not 1 to 64 characters of A-Z, a-z, 0-9, _ and -",
                truncate_for_log(&entry.id)
            ));
        }
        let key = eustress_identity::keypair::decode_public_key(entry.public_key.trim())
            .map_err(|_| format!("party {}: public_key is not a base64 Ed25519 public key", entry.id))?;
        Ok(Party {
            id: entry.id.clone(),
            enabled: entry.enabled,
            admin: entry.admin,
            max_subscriptions: entry.max_subscriptions,
            public_key_b64: STANDARD.encode(key.as_bytes()),
            key_fingerprint: sha256(key.as_bytes()),
            subscribe: entry.subscribe.iter().cloned().collect(),
            publish: entry.publish.iter().cloned().collect(),
        })
    }

    /// The subscribe grant: the topic is in this party's `subscribe` list.
    pub fn may_subscribe(&self, topic: &str) -> bool {
        self.subscribe.contains(topic)
    }

    /// The publish grant: the topic is in this party's `publish` list and,
    /// for a reserved topic, the party is an admin.
    pub fn may_publish(&self, topic: &str) -> bool {
        self.publish.contains(topic) && (self.admin || !is_reserved_topic(topic))
    }

    /// Whether a topic list shows this topic to this party: any grant on it.
    pub fn may_list(&self, topic: &str) -> bool {
        self.subscribe.contains(topic) || self.publish.contains(topic)
    }

    /// SHA-256 of this party's public key. Every session, token and stream
    /// records it when it is created; a reload that finds another key under
    /// the same id ends them all.
    pub fn key_fingerprint(&self) -> [u8; 32] {
        self.key_fingerprint
    }

    /// `true` when `signature` is this party's Ed25519 signature over
    /// `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        signature.len() == 64
            && eustress_identity::keypair::verify_signed_data(
                &self.public_key_b64,
                message,
                &STANDARD.encode(signature),
            )
            .is_ok()
    }
}

/// Spend the work of one signature check against a key no party holds, so an
/// unknown or disabled party, or an unknown, expired or used REST nonce, is
/// refused in about the time a wrong key is.
pub(crate) fn burn_signature_check(signature: &[u8]) {
    let checked =
        eustress_identity::keypair::verify_signed_data(TIMING_KEY_B64, b"eustress-stream-login-timing", &STANDARD.encode(signature));
    #[cfg(feature = "test-hooks")]
    burn_hook::record(signature, &checked);
    drop(checked);
}

/// At most 128 characters of `text`, for a log line.
pub(crate) fn truncate_for_log(text: &str) -> &str {
    match text.char_indices().nth(128) {
        Some((at, _)) => &text[..at],
        None => text,
    }
}

/// The checked parties of one version of `parties.toml`.
#[derive(Debug, Default)]
pub struct PartySet {
    parties: HashMap<String, Arc<Party>>,
}

impl PartySet {
    /// Check every entry. An entry with a malformed id or key, and every entry
    /// that shares its id with another, is left out and reported: it cannot
    /// log in.
    pub fn from_file(file: &PartiesFile) -> (PartySet, Vec<String>) {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for entry in &file.parties {
            *counts.entry(entry.id.as_str()).or_default() += 1;
        }
        let mut problems = Vec::new();
        let mut parties = HashMap::new();
        for entry in &file.parties {
            if counts.get(entry.id.as_str()).copied().unwrap_or(0) > 1 {
                continue;
            }
            match Party::from_entry(entry) {
                Ok(party) => {
                    parties.insert(party.id.clone(), Arc::new(party));
                }
                Err(why) => problems.push(why),
            }
        }
        for (id, n) in counts {
            if n > 1 {
                problems.push(format!(
                    "party {:?} is enrolled {n} times; none of those entries can log in",
                    truncate_for_log(id)
                ));
            }
        }
        (PartySet { parties }, problems)
    }

    /// The party with this id, enabled or not.
    pub fn get(&self, id: &str) -> Option<&Arc<Party>> {
        self.parties.get(id)
    }

    /// The party with this id when it is enabled.
    pub fn active(&self, id: &str) -> Option<&Arc<Party>> {
        self.parties.get(id).filter(|p| p.enabled)
    }

    /// `true` when `id` names an enabled party whose key is still the one
    /// with `fingerprint`. A session, token or stream recorded under another
    /// key, or for a party that is gone or disabled, fails this.
    pub fn holds_key(&self, id: &str, fingerprint: &[u8; 32]) -> bool {
        self.active(id).is_some_and(|party| party.key_fingerprint == *fingerprint)
    }

    /// How many parties are enabled.
    pub fn enabled_count(&self) -> usize {
        self.parties.values().filter(|p| p.enabled).count()
    }

    /// How many parties are enrolled, enabled or not.
    pub fn len(&self) -> usize {
        self.parties.len()
    }

    /// `true` when no party is enrolled.
    pub fn is_empty(&self) -> bool {
        self.parties.is_empty()
    }
}

/// What a reload did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// The file is unchanged since the last good read.
    Unchanged,
    /// The file changed and its new set is in force.
    Replaced,
    /// The file could not be used; the last good set stays in force.
    KeptLastGood(String),
}

/// The live set of parties, read from `parties.toml`.
pub struct PartyTable {
    path: PathBuf,
    current: RwLock<Arc<PartySet>>,
    last_bytes: Mutex<Vec<u8>>,
    last_failure: Mutex<Option<String>>,
    owner_only: OwnerOnly,
}

impl PartyTable {
    /// Read the file at startup: check the folder that holds it, open the
    /// file with the owner-only check and read it through that handle, then
    /// parse it. Any failure refuses startup: there is no last good set yet.
    pub fn load(path: &Path) -> Result<Self, NodeError> {
        Self::load_with(path, OWNER_ONLY)
    }

    fn load_with(path: &Path, owner_only: OwnerOnly) -> Result<Self, NodeError> {
        let (bytes, file) = read_parties(path, owner_only).map_err(NodeError::Startup)?;
        let (set, problems) = PartySet::from_file(&file);
        for problem in problems {
            error!("parties file {}: {problem}", path.display());
        }
        Ok(PartyTable {
            path: path.to_path_buf(),
            current: RwLock::new(Arc::new(set)),
            last_bytes: Mutex::new(bytes),
            last_failure: Mutex::new(None),
            owner_only,
        })
    }

    /// The set in force now.
    pub fn current(&self) -> Arc<PartySet> {
        Arc::clone(&self.current.read())
    }

    /// The file this table reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Re-read the file. A file that fails the owner-only check, cannot be
    /// read, or fails to parse keeps the last good set, and the failure is
    /// logged at error level (once until it changes or clears).
    pub fn reload(&self) -> ReloadOutcome {
        let (bytes, file) = match read_parties(&self.path, self.owner_only) {
            Ok(read) => read,
            Err(why) => return self.keep_last_good(why),
        };
        if *self.last_bytes.lock() == bytes {
            self.clear_failure();
            return ReloadOutcome::Unchanged;
        }
        let (set, problems) = PartySet::from_file(&file);
        for problem in problems {
            error!("parties file {}: {problem}", self.path.display());
        }
        *self.current.write() = Arc::new(set);
        *self.last_bytes.lock() = bytes;
        self.clear_failure();
        info!("parties file {} reloaded", self.path.display());
        ReloadOutcome::Replaced
    }

    fn keep_last_good(&self, why: String) -> ReloadOutcome {
        let mut last = self.last_failure.lock();
        if last.as_deref() != Some(why.as_str()) {
            error!("{why}; keeping the last good set of parties");
            *last = Some(why.clone());
        }
        ReloadOutcome::KeptLastGood(why)
    }

    fn clear_failure(&self) {
        if self.last_failure.lock().take().is_some() {
            info!("parties file {} is usable again", self.path.display());
        }
    }
}

/// Check, read and parse a parties file. The owner-only checks run first, at
/// startup and on every reload: `verify` on the folder that holds the file,
/// then `verify_open` on the file, whose bytes are read through the handle
/// that call returned. The file read is the file checked, so a file another
/// account can reach, or replace, is never parsed.
fn read_parties(path: &Path, owner_only: OwnerOnly) -> Result<(Vec<u8>, PartiesFile), String> {
    let folder = parent_folder(path);
    (owner_only.verify)(&folder)
        .map_err(|e| format!("the folder that holds the parties file failed the owner-only check: {}", owner_only_refusal(&folder, &e)))?;
    let mut checked = (owner_only.verify_open)(path)
        .map_err(|e| format!("parties file failed the owner-only check: {}", owner_only_refusal(path, &e)))?;
    let mut bytes = Vec::new();
    checked
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read parties file {}: {e}", path.display()))?;
    // On Windows the open handle keeps other writers out, including the
    // rename `party add` and `party revoke` make, so it is closed before the
    // parse.
    drop(checked);
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| format!("parties file {} is not UTF-8", path.display()))?;
    let file = PartiesFile::parse(text)
        .map_err(|e| format!("parties file {} does not parse: {e}", path.display()))?;
    Ok((bytes, file))
}

/// The folder that holds `path`, or `.` for a bare file name.
pub fn parent_folder(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// The owner-only helper startup and the reload call. Every build uses
/// `eustress_identity::private_file` itself ([`OWNER_ONLY`]); one unit test
/// hands startup a helper that refuses as `private_file` does for a file
/// another account owns, since a real refusal of that kind needs a second
/// account.
#[derive(Clone, Copy)]
struct OwnerOnly {
    /// The check for a folder.
    verify: fn(&Path) -> std::io::Result<()>,
    /// Open a file and check it through the open handle.
    verify_open: fn(&Path) -> std::io::Result<std::fs::File>,
}

/// The helper itself.
const OWNER_ONLY: OwnerOnly = OwnerOnly {
    verify: eustress_identity::private_file::verify,
    verify_open: eustress_identity::private_file::verify_open,
};

/// Why the owner-only check refused `path`, for the error and the log. When
/// another account owns the file or folder, the text names that owner, which
/// `private_file::not_owner` returns, says whether it is an administrator
/// account (a file made from an elevated prompt is owned by one), and gives
/// the helper's advice for taking ownership back. Any other refusal is the
/// helper's own text, which names the account that also has access.
pub fn owner_only_refusal(path: &Path, e: &std::io::Error) -> String {
    match eustress_identity::private_file::not_owner(e) {
        Some(found) => format!(
            "{} is owned by {}, {}, not by the current user; {}",
            path.display(),
            found.owner,
            if found.trusted { "an administrator account" } else { "another account" },
            found.advice
        ),
        None => e.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pre-login limits
// ─────────────────────────────────────────────────────────────────────────────

/// The key every per-address limit counts under: the address, with an
/// IPv4-mapped IPv6 address folded to IPv4 and an IPv6 address cut to its
/// /64 prefix. One host controls a whole /64, so counting each IPv6 address
/// on its own would leave the per-address limits empty for IPv6.
pub fn limit_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        v4 => v4,
    }
}

/// Caps the connections that are accepted but not yet logged in: 128 in all
/// and 16 per source IP. A permit is taken at accept and released when the
/// login finishes, fails, or misses its deadline. A source counts under
/// [`limit_key`].
pub struct PreLoginLimiter {
    total_max: usize,
    per_ip_max: usize,
    state: Mutex<LimiterState>,
}

#[derive(Default)]
struct LimiterState {
    total: usize,
    per_ip: HashMap<IpAddr, usize>,
}

/// One pre-login slot. Dropping it frees the slot.
pub struct PreLoginPermit {
    limiter: Arc<PreLoginLimiter>,
    ip: IpAddr,
}

impl fmt::Debug for PreLoginPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreLoginPermit").field("ip", &self.ip).finish()
    }
}

impl PreLoginLimiter {
    /// The node's limits: 128 in all, 16 per source IP.
    pub fn new() -> Arc<Self> {
        Self::with_limits(MAX_PRE_LOGIN_TOTAL, MAX_PRE_LOGIN_PER_IP)
    }

    /// Other limits, for a listener that needs its own.
    pub fn with_limits(total_max: usize, per_ip_max: usize) -> Arc<Self> {
        Arc::new(PreLoginLimiter { total_max, per_ip_max, state: Mutex::new(LimiterState::default()) })
    }

    /// A slot for a connection from `ip`, or `None` when either cap is reached.
    pub fn try_acquire(self: &Arc<Self>, ip: IpAddr) -> Option<PreLoginPermit> {
        let ip = limit_key(ip);
        let mut guard = self.state.lock();
        let state = &mut *guard;
        if state.total >= self.total_max {
            return None;
        }
        let used = state.per_ip.get(&ip).copied().unwrap_or(0);
        if used >= self.per_ip_max {
            return None;
        }
        state.per_ip.insert(ip, used + 1);
        state.total += 1;
        Some(PreLoginPermit { limiter: Arc::clone(self), ip })
    }

    /// Slots in use.
    pub fn in_use(&self) -> usize {
        self.state.lock().total
    }

    /// Slots in use by `ip`.
    pub fn in_use_for(&self, ip: IpAddr) -> usize {
        self.state.lock().per_ip.get(&limit_key(ip)).copied().unwrap_or(0)
    }
}

impl Drop for PreLoginPermit {
    fn drop(&mut self) {
        let mut guard = self.limiter.state.lock();
        let state = &mut *guard;
        state.total = state.total.saturating_sub(1);
        if let Some(used) = state.per_ip.get_mut(&self.ip) {
            *used = used.saturating_sub(1);
            if *used == 0 {
                state.per_ip.remove(&self.ip);
            }
        }
    }
}

/// Rate-limits warn lines to one per key per 10 seconds. The key is a source
/// address, counted under [`limit_key`] (failed logins, and refusals over TCP
/// and QUIC), or a party id (refusals over REST). The lines held back are
/// counted and reported: on the next line admitted for that key, or by
/// [`WarnLimiter::flush`] once the window has closed, so a burst that simply
/// stops is still summarized.
pub struct WarnLimiter<K = IpAddr> {
    windows: Mutex<HashMap<K, WarnWindow>>,
}

struct WarnWindow {
    started: Instant,
    suppressed: u64,
}

impl<K: Eq + Hash + Clone + Ord> Default for WarnLimiter<K> {
    fn default() -> Self {
        WarnLimiter { windows: Mutex::new(HashMap::new()) }
    }
}

impl<K: Eq + Hash + Clone + Ord> WarnLimiter<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Some(n)` when a warn line for `key` may be written now, where `n` is
    /// the number of lines held back in the window that just closed. `None`
    /// while the key's window is still open; the line is then counted.
    pub fn admit_key(&self, key: K, now: Instant) -> Option<u64> {
        let mut windows = self.windows.lock();
        if windows.len() >= WARN_TABLE_MAX && !windows.contains_key(&key) {
            windows.retain(|_, w| now.duration_since(w.started) < WARN_WINDOW);
            if windows.len() >= WARN_TABLE_MAX {
                return None;
            }
        }
        match windows.get_mut(&key) {
            Some(window) if now.duration_since(window.started) < WARN_WINDOW => {
                window.suppressed += 1;
                None
            }
            Some(window) => {
                let held_back = window.suppressed;
                window.started = now;
                window.suppressed = 0;
                Some(held_back)
            }
            None => {
                windows.insert(key, WarnWindow { started: now, suppressed: 0 });
                Some(0)
            }
        }
    }

    /// Close every window that has run its 10 seconds. Returns each key
    /// whose closed window held lines back, with the count, so the caller can
    /// write one summary line for it. A closed window that held nothing back
    /// is dropped quietly. Its key's next line is admitted at once.
    pub fn flush(&self, now: Instant) -> Vec<(K, u64)> {
        let mut summaries = Vec::new();
        self.windows.lock().retain(|key, window| {
            if now.duration_since(window.started) < WARN_WINDOW {
                return true;
            }
            if window.suppressed > 0 {
                summaries.push((key.clone(), window.suppressed));
            }
            false
        });
        summaries.sort();
        summaries
    }
}

impl WarnLimiter<IpAddr> {
    /// [`WarnLimiter::admit_key`] for a source address, counted under
    /// [`limit_key`].
    pub fn admit(&self, ip: IpAddr, now: Instant) -> Option<u64> {
        self.admit_key(limit_key(ip), now)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Live sessions and quotas
// ─────────────────────────────────────────────────────────────────────────────

/// The logged-in connections and SSE streams, by party, so a revocation can
/// end them.
pub struct SessionRegistry {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<u64, SessionEntry>>,
}

struct SessionEntry {
    party_id: String,
    /// The fingerprint of the key the party logged in with.
    key_fingerprint: [u8; 32],
    kill: watch::Sender<bool>,
}

/// One live session. Dropping it removes it from the registry.
pub struct SessionGuard {
    registry: Arc<SessionRegistry>,
    id: u64,
    kill: watch::Receiver<bool>,
}

impl SessionRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(SessionRegistry { next_id: AtomicU64::new(1), sessions: Mutex::new(HashMap::new()) })
    }

    /// Record a live session for `party`, under the fingerprint of the key it
    /// logged in with.
    pub fn register(self: &Arc<Self>, party: &Party) -> SessionGuard {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = watch::channel(false);
        let entry = SessionEntry { party_id: party.id.clone(), key_fingerprint: party.key_fingerprint, kill: tx };
        self.sessions.lock().insert(id, entry);
        SessionGuard { registry: Arc::clone(self), id, kill: rx }
    }

    /// End every session whose party is not enabled in `set`, or whose
    /// party's key is no longer the one it logged in with. Returns how many
    /// sessions each such party lost.
    pub fn drop_inactive(&self, set: &PartySet) -> HashMap<String, usize> {
        let mut dropped: HashMap<String, usize> = HashMap::new();
        self.sessions.lock().retain(|_, session| {
            if set.holds_key(&session.party_id, &session.key_fingerprint) {
                return true;
            }
            let _ = session.kill.send(true);
            *dropped.entry(session.party_id.clone()).or_default() += 1;
            false
        });
        dropped
    }

    /// Live sessions for `party_id`.
    pub fn live_for(&self, party_id: &str) -> usize {
        self.sessions.lock().values().filter(|s| s.party_id == party_id).count()
    }

    /// Live sessions in all.
    pub fn live(&self) -> usize {
        self.sessions.lock().len()
    }
}

impl SessionGuard {
    /// Completes when this session's party has been revoked.
    pub async fn revoked(&mut self) {
        loop {
            if *self.kill.borrow_and_update() {
                return;
            }
            if self.kill.changed().await.is_err() {
                // The registry dropped the entry without a revocation: the
                // session is ending on its own, so there is nothing to wait for.
                std::future::pending::<()>().await;
            }
        }
    }

    /// `true` once this session's party has been revoked.
    pub fn is_revoked(&self) -> bool {
        *self.kill.borrow()
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.registry.sessions.lock().remove(&self.id);
    }
}

/// Counts each party's simultaneous subscriptions across TCP, QUIC and SSE.
pub struct SubscriptionQuotas {
    counts: Mutex<HashMap<String, usize>>,
}

/// One subscription slot. Dropping it frees the slot.
pub struct QuotaSlot {
    quotas: Arc<SubscriptionQuotas>,
    party_id: String,
}

impl SubscriptionQuotas {
    pub fn new() -> Arc<Self> {
        Arc::new(SubscriptionQuotas { counts: Mutex::new(HashMap::new()) })
    }

    /// A slot for `party`, or `None` when it holds `max_subscriptions` already.
    pub fn try_acquire(self: &Arc<Self>, party: &Party) -> Option<QuotaSlot> {
        let mut counts = self.counts.lock();
        let used = counts.get(&party.id).copied().unwrap_or(0);
        if used >= party.max_subscriptions {
            return None;
        }
        counts.insert(party.id.clone(), used + 1);
        Some(QuotaSlot { quotas: Arc::clone(self), party_id: party.id.clone() })
    }

    /// Slots `party_id` holds now.
    pub fn in_use(&self, party_id: &str) -> usize {
        self.counts.lock().get(party_id).copied().unwrap_or(0)
    }
}

impl Drop for QuotaSlot {
    fn drop(&mut self) {
        let mut counts = self.quotas.counts.lock();
        if let Some(used) = counts.get_mut(&self.party_id) {
            *used = used.saturating_sub(1);
            if *used == 0 {
                counts.remove(&self.party_id);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// REST tokens and the nonce table
// ─────────────────────────────────────────────────────────────────────────────

/// Bearer tokens, stored as SHA-256 hashes and bound to their party and to
/// the fingerprint of the key it logged in with. At most 4096 in all and 32
/// per party; the per-party count is kept on issue, expiry and revocation.
pub struct TokenTable {
    state: Mutex<TokenState>,
}

#[derive(Default)]
struct TokenState {
    entries: HashMap<[u8; 32], TokenEntry>,
    /// Tokens held per party, live or not yet cleared.
    per_party: HashMap<String, usize>,
}

struct TokenEntry {
    party_id: String,
    class: TokenClass,
    expires_at: Instant,
    /// The fingerprint of the party's key when the token was issued.
    key_fingerprint: [u8; 32],
}

impl TokenState {
    /// One token fewer for `party_id`.
    fn release(&mut self, party_id: &str) {
        if let Some(count) = self.per_party.get_mut(party_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.per_party.remove(party_id);
            }
        }
    }

    /// Drop every token `keep` refuses, and keep the per-party counts.
    /// Returns how many tokens each party lost.
    fn retain(&mut self, mut keep: impl FnMut(&TokenEntry) -> bool) -> HashMap<String, usize> {
        let mut dropped: HashMap<String, usize> = HashMap::new();
        self.entries.retain(|_, entry| {
            if keep(entry) {
                return true;
            }
            *dropped.entry(entry.party_id.clone()).or_default() += 1;
            false
        });
        for (party_id, lost) in &dropped {
            if let Some(count) = self.per_party.get_mut(party_id) {
                *count = count.saturating_sub(*lost);
                if *count == 0 {
                    self.per_party.remove(party_id);
                }
            }
        }
        dropped
    }

    /// Tokens `party_id` holds, live or not yet cleared.
    fn held_by(&self, party_id: &str) -> usize {
        self.per_party.get(party_id).copied().unwrap_or(0)
    }
}

/// A token just issued. `token` is the only copy of the secret.
#[derive(Clone)]
pub struct IssuedToken {
    /// Base64 of 32 random bytes.
    pub token: String,
    pub class: TokenClass,
    pub expires_in: Duration,
}

impl fmt::Debug for IssuedToken {
    // The token itself is left out: tokens are never logged.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedToken")
            .field("class", &self.class)
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// What a live bearer token stands for.
#[derive(Debug, Clone)]
pub struct TokenGrant {
    /// The party the token is bound to.
    pub party_id: String,
    pub class: TokenClass,
    /// When the token stops being valid. A stream opened with it ends then.
    pub expires_at: Instant,
    /// The fingerprint of the party's key when the token was issued.
    pub key_fingerprint: [u8; 32],
}

/// Why a token was not issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRefusal {
    /// The table holds 4096 live tokens. REST answers 503.
    Full,
    /// The party holds 32 live tokens. REST answers 429, for that party only.
    PartyLimit,
}

impl TokenTable {
    pub fn new() -> Arc<Self> {
        Arc::new(TokenTable { state: Mutex::new(TokenState::default()) })
    }

    /// Issue a token of `class` for `party`, recorded under the fingerprint
    /// of its key. At either cap, expired tokens are cleared first; a table,
    /// or a party, still at its cap refuses.
    pub fn issue(&self, party: &Party, class: TokenClass, now: Instant) -> Result<IssuedToken, TokenRefusal> {
        let mut guard = self.state.lock();
        let state = &mut *guard;
        if state.entries.len() >= TOKEN_TABLE_MAX || state.held_by(&party.id) >= MAX_TOKENS_PER_PARTY {
            state.retain(|entry| entry.expires_at > now);
        }
        if state.entries.len() >= TOKEN_TABLE_MAX {
            return Err(TokenRefusal::Full);
        }
        if state.held_by(&party.id) >= MAX_TOKENS_PER_PARTY {
            return Err(TokenRefusal::PartyLimit);
        }
        let mut raw = [0u8; 32];
        OsRng.fill_bytes(&mut raw);
        let lifetime = class.lifetime();
        let entry = TokenEntry {
            party_id: party.id.clone(),
            class,
            expires_at: now + lifetime,
            key_fingerprint: party.key_fingerprint,
        };
        if let Some(replaced) = state.entries.insert(sha256(&raw), entry) {
            state.release(&replaced.party_id);
        }
        *state.per_party.entry(party.id.clone()).or_default() += 1;
        Ok(IssuedToken { token: STANDARD.encode(raw), class, expires_in: lifetime })
    }

    /// What a live token stands for, or `None` for an unknown or expired
    /// one. An expired token is removed.
    pub fn validate(&self, token: &str, now: Instant) -> Option<TokenGrant> {
        let raw = STANDARD.decode(token.trim()).ok()?;
        if raw.len() != 32 {
            return None;
        }
        let key = sha256(&raw);
        let mut guard = self.state.lock();
        let state = &mut *guard;
        let entry = state.entries.get(&key)?;
        if entry.expires_at <= now {
            if let Some(expired) = state.entries.remove(&key) {
                state.release(&expired.party_id);
            }
            return None;
        }
        Some(TokenGrant {
            party_id: entry.party_id.clone(),
            class: entry.class,
            expires_at: entry.expires_at,
            key_fingerprint: entry.key_fingerprint,
        })
    }

    /// Drop every expired token, and every token whose party is gone,
    /// disabled, or holds another key now. Returns how many tokens each such
    /// party lost; expired tokens are not counted there.
    pub fn retain_active(&self, set: &PartySet, now: Instant) -> HashMap<String, usize> {
        let mut state = self.state.lock();
        state.retain(|entry| entry.expires_at > now);
        state.retain(|entry| set.holds_key(&entry.party_id, &entry.key_fingerprint))
    }

    /// Tokens `party_id` holds, live or not yet cleared.
    pub fn live_for(&self, party_id: &str) -> usize {
        self.state.lock().held_by(party_id)
    }

    /// Tokens held, live or not yet cleared.
    pub fn len(&self) -> usize {
        self.state.lock().entries.len()
    }

    /// `true` when no token is held.
    pub fn is_empty(&self) -> bool {
        self.state.lock().entries.is_empty()
    }
}

/// The REST nonce table: the outstanding login challenges. Each lives 5
/// seconds and is good for one login attempt. At most 16 are outstanding from
/// one source IP and 4096 in all. A login finds its challenge by the nonce it
/// carries, whatever address it comes from.
pub struct ChallengeTable {
    state: Mutex<ChallengeState>,
}

#[derive(Default)]
struct ChallengeState {
    /// The outstanding challenges, by nonce.
    by_nonce: HashMap<[u8; NONCE_LEN], Outstanding>,
    /// Issue order, which is also expiry order: every challenge lives the
    /// same 5 seconds. A taken challenge stays here until it expires.
    order: VecDeque<([u8; NONCE_LEN], Instant)>,
    /// Outstanding challenges per source IP.
    per_ip: HashMap<IpAddr, usize>,
}

struct Outstanding {
    ip: IpAddr,
    expires_at: Instant,
}

/// Why a challenge was not issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeRefusal {
    /// The source IP has 16 challenges outstanding. REST answers 429.
    TooManyFromSource,
    /// 4096 challenges are outstanding in all. REST answers 503.
    Full,
}

impl ChallengeTable {
    pub fn new() -> Arc<Self> {
        Arc::new(ChallengeTable { state: Mutex::new(ChallengeState::default()) })
    }

    fn release(per_ip: &mut HashMap<IpAddr, usize>, ip: IpAddr) {
        if let Some(count) = per_ip.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                per_ip.remove(&ip);
            }
        }
    }

    /// Drop the challenges that have expired. Each entry is visited once, so
    /// this costs nothing when nothing has expired.
    fn purge(state: &mut ChallengeState, now: Instant) {
        while let Some(&(nonce, expires_at)) = state.order.front() {
            if expires_at > now {
                break;
            }
            state.order.pop_front();
            if let Some(challenge) = state.by_nonce.remove(&nonce) {
                Self::release(&mut state.per_ip, challenge.ip);
            }
        }
        if state.order.len() > 2 * MAX_CHALLENGES_TOTAL {
            // Taken challenges wait in `order` until they expire. A fast loop
            // of challenge and login could grow it, so drop them now.
            let by_nonce = &state.by_nonce;
            state.order.retain(|(nonce, _)| by_nonce.contains_key(nonce));
        }
    }

    /// Issue a challenge to a login from `ip`.
    pub fn issue(&self, ip: IpAddr, now: Instant) -> Result<[u8; NONCE_LEN], ChallengeRefusal> {
        let ip = limit_key(ip);
        let mut guard = self.state.lock();
        let state = &mut *guard;
        Self::purge(state, now);
        if state.per_ip.get(&ip).copied().unwrap_or(0) >= MAX_CHALLENGES_PER_IP {
            return Err(ChallengeRefusal::TooManyFromSource);
        }
        if state.by_nonce.len() >= MAX_CHALLENGES_TOTAL {
            return Err(ChallengeRefusal::Full);
        }
        let nonce = fresh_nonce();
        let expires_at = now + CHALLENGE_LIFETIME;
        state.by_nonce.insert(nonce, Outstanding { ip, expires_at });
        state.order.push_back((nonce, expires_at));
        *state.per_ip.entry(ip).or_default() += 1;
        Ok(nonce)
    }

    /// Use up the challenge for `nonce`. `true` when it was issued, has not
    /// expired, and was not used before. The lookup is by nonce alone, from
    /// any source address. Whatever the answer, the nonce is gone afterwards.
    pub fn take(&self, nonce: &[u8; NONCE_LEN], now: Instant) -> bool {
        let mut guard = self.state.lock();
        let state = &mut *guard;
        let Some(challenge) = state.by_nonce.remove(nonce) else {
            return false;
        };
        Self::release(&mut state.per_ip, challenge.ip);
        challenge.expires_at > now
    }

    /// Drop the expired challenges. The once-a-second check calls this.
    pub fn purge_expired(&self, now: Instant) {
        Self::purge(&mut self.state.lock(), now);
    }

    /// Challenges outstanding from `ip`, live or not yet cleared.
    pub fn outstanding_for(&self, ip: IpAddr) -> usize {
        self.state.lock().per_ip.get(&limit_key(ip)).copied().unwrap_or(0)
    }

    /// Challenges outstanding in all, live or not yet cleared.
    pub fn outstanding(&self) -> usize {
        self.state.lock().by_nonce.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Login checks
// ─────────────────────────────────────────────────────────────────────────────

/// Why a login was refused. `reason` is the only text the peer sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// `"auth_failed"` or `"version_unsupported"`.
    pub reason: &'static str,
    /// For the node's own log line.
    pub cause: &'static str,
    /// The party id, only when it names an enrolled party.
    pub enrolled_party: Option<String>,
}

/// The result of checking a `Hello`.
#[derive(Debug)]
pub enum HelloOutcome {
    Welcome(Arc<Party>),
    Refused(Refusal),
}

/// Check a TCP or QUIC `Hello` against `set`, in the order the specification
/// gives: the protocol version, the `party_id` format, that the party exists
/// and is enabled, then the signature over this channel's transcript with
/// this connection's nonce and exporter.
#[allow(clippy::too_many_arguments)]
pub fn check_hello(
    set: &PartySet,
    node_id: &str,
    channel: Channel,
    nonce: &[u8; NONCE_LEN],
    exporter: &[u8; BINDING_LEN],
    protocol_version: u16,
    party_id: &str,
    signature: &[u8],
) -> HelloOutcome {
    let enrolled = |id: &str| -> Option<String> {
        if is_valid_party_id(id) {
            set.get(id).map(|p| p.id.clone())
        } else {
            None
        }
    };
    if protocol_version != PROTOCOL_VERSION {
        return HelloOutcome::Refused(Refusal {
            reason: REASON_VERSION_UNSUPPORTED,
            cause: "unsupported protocol version",
            enrolled_party: enrolled(party_id),
        });
    }
    if !is_valid_party_id(party_id) {
        // Refused before any lookup.
        return HelloOutcome::Refused(Refusal {
            reason: REASON_AUTH_FAILED,
            cause: "malformed party id",
            enrolled_party: None,
        });
    }
    let Some(party) = set.active(party_id) else {
        burn_signature_check(signature);
        let enrolled_party = enrolled(party_id);
        let cause = if enrolled_party.is_some() { "party disabled" } else { "unknown party" };
        return HelloOutcome::Refused(Refusal { reason: REASON_AUTH_FAILED, cause, enrolled_party });
    };
    let message = match transcript(channel, node_id, nonce, Binding::Exporter(exporter), party_id, protocol_version, None) {
        Ok(message) => message,
        Err(_) => {
            return HelloOutcome::Refused(Refusal {
                reason: REASON_AUTH_FAILED,
                cause: "transcript could not be built",
                enrolled_party: Some(party.id.clone()),
            })
        }
    };
    if !party.verify(&message, signature) {
        return HelloOutcome::Refused(Refusal {
            reason: REASON_AUTH_FAILED,
            cause: "bad signature",
            enrolled_party: Some(party.id.clone()),
        });
    }
    HelloOutcome::Welcome(Arc::clone(party))
}

// ─────────────────────────────────────────────────────────────────────────────
// NodeAuth
// ─────────────────────────────────────────────────────────────────────────────

/// What one revocation pass did.
#[derive(Debug, Clone)]
pub struct RevocationReport {
    pub reload: ReloadOutcome,
    /// Live connections and SSE streams ended, by party.
    pub sessions_dropped: HashMap<String, usize>,
    /// Tokens dropped, by party.
    pub tokens_dropped: HashMap<String, usize>,
}

/// Everything a listener needs to authenticate and authorize: the node's
/// identity and TLS material, the live parties, sessions, quotas, tokens,
/// challenges and pre-login limits.
pub struct NodeAuth {
    node_id: String,
    tls: Arc<NodeTls>,
    parties: Arc<PartyTable>,
    /// Logged-in connections and SSE streams.
    pub sessions: Arc<SessionRegistry>,
    /// Simultaneous subscriptions per party.
    pub quotas: Arc<SubscriptionQuotas>,
    /// REST bearer tokens.
    pub tokens: Arc<TokenTable>,
    /// The REST nonce table: outstanding login challenges.
    pub challenges: Arc<ChallengeTable>,
    /// TCP and QUIC connections that are accepted but not logged in.
    pub pre_login: Arc<PreLoginLimiter>,
    /// REST connections that are accepted but have not finished TLS.
    pub rest_handshakes: Arc<PreLoginLimiter>,
    warn_lines: WarnLimiter,
    /// Rate-limits the warn lines for refused publishes and subscribes, per
    /// source IP, apart from the login warnings.
    refusal_lines: WarnLimiter,
    /// Rate-limits the warn lines for refused REST requests, per party.
    rest_refusal_lines: WarnLimiter<String>,
    /// Bumped each time a reload puts a new set of parties in force.
    parties_generation: watch::Sender<u64>,
    revocation_started: AtomicBool,
}

impl NodeAuth {
    /// Run the startup checks and build the node's authentication state.
    ///
    /// Refuses, so that no listener starts, when: the node id is longer than
    /// 64 bytes; `tls_cert`, `tls_key` or `parties_path` is not set; the TLS
    /// private key, `parties.toml`, or the folder holding `parties.toml` fails
    /// the owner-only check (each of the two files is opened with
    /// `eustress_identity::private_file::verify_open` and read through the
    /// handle that call returned, and the folder is checked with `verify`; the
    /// log line names the path and the account the check found, and the owner
    /// when another account owns the file); the certificate or key does not
    /// load; `parties.toml` does not parse; or the bind address is not
    /// loopback and no party is enabled.
    pub fn from_config(config: &NodeConfig) -> Result<Arc<Self>, NodeError> {
        Self::from_config_with(config, OWNER_ONLY)
    }

    fn from_config_with(config: &NodeConfig, owner_only: OwnerOnly) -> Result<Arc<Self>, NodeError> {
        if config.node_id.len() > MAX_NODE_ID_BYTES {
            return Err(startup_refused(format!(
                "node_id is {} bytes; the most a login transcript carries is 64",
                config.node_id.len()
            )));
        }
        let cert_path = config.tls_cert.as_deref().ok_or_else(|| {
            startup_refused("no TLS certificate is configured (tls_cert); every listener requires one".to_string())
        })?;
        let key_path = config.tls_key.as_deref().ok_or_else(|| {
            startup_refused("no TLS private key is configured (tls_key); every listener requires one".to_string())
        })?;
        let parties_path = config.parties_path.as_deref().ok_or_else(|| {
            startup_refused("no parties file is configured (parties_path); nobody could log in".to_string())
        })?;

        // The owner-only checks come before anything is parsed. The key is
        // read through the handle its check opened; `PartyTable::load_with`
        // checks the folder that holds `parties.toml`, then reads the file the
        // same way.
        let key_pem = read_private(owner_only, key_path, "TLS private key")?;
        let parties = PartyTable::load_with(parties_path, owner_only).map_err(|e| {
            error!("stream node not started: {e}");
            e
        })?;
        // The certificate is public, so it is read by path.
        let cert_pem = std::fs::read(cert_path)
            .map_err(|e| startup_refused(format!("cannot read the certificate {}: {e}", cert_path.display())))?;
        let tls = NodeTls::from_pem(&cert_pem, &key_pem).map_err(|e| startup_refused(e.to_string()))?;

        let enabled = parties.current().enabled_count();
        if config.is_externally_bound() && enabled == 0 {
            return Err(startup_refused(format!(
                "bind address {} is not loopback and no party in {} is enabled",
                config.bind_addr,
                parties_path.display()
            )));
        }

        info!(
            node_id = %config.node_id,
            spki_pin = %tls.spki_pin(),
            enabled_parties = enabled,
            "stream node authentication ready (TLS 1.3; clients pin this SPKI SHA-256)"
        );
        Ok(Arc::new(Self::with_parts(config.node_id.clone(), Arc::new(tls), Arc::new(parties))))
    }

    fn with_parts(node_id: String, tls: Arc<NodeTls>, parties: Arc<PartyTable>) -> Self {
        NodeAuth {
            node_id,
            tls,
            parties,
            sessions: SessionRegistry::new(),
            quotas: SubscriptionQuotas::new(),
            tokens: TokenTable::new(),
            challenges: ChallengeTable::new(),
            pre_login: PreLoginLimiter::new(),
            rest_handshakes: PreLoginLimiter::new(),
            warn_lines: WarnLimiter::new(),
            refusal_lines: WarnLimiter::new(),
            rest_refusal_lines: WarnLimiter::new(),
            parties_generation: watch::channel(0).0,
            revocation_started: AtomicBool::new(false),
        }
    }

    /// The node id every transcript carries.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// The node's certificate, key and TLS configurations.
    pub fn tls(&self) -> &Arc<NodeTls> {
        &self.tls
    }

    /// The live party table.
    pub fn parties(&self) -> &Arc<PartyTable> {
        &self.parties
    }

    /// The set of parties in force now.
    pub fn party_set(&self) -> Arc<PartySet> {
        self.parties.current()
    }

    /// Register the live session of a login just admitted (by `check_hello`,
    /// or by a bearer token for a stream), then check the party again against
    /// the set in force now. A revocation or a key change that landed between
    /// the check and the registration is seen here: the session is dropped
    /// and `None` returned. Once registered, a session is seen by every later
    /// revocation pass, so nothing can fall between the two.
    pub fn register_session(&self, party: &Party) -> Option<SessionGuard> {
        let session = self.sessions.register(party);
        if self.party_set().holds_key(&party.id, &party.key_fingerprint) {
            Some(session)
        } else {
            None
        }
    }

    /// A receiver that changes each time a reload puts a new set of parties
    /// in force. Each connection and each SSE stream then re-checks its
    /// grants and drops what its party is no longer granted.
    pub fn parties_changed(&self) -> watch::Receiver<u64> {
        self.parties_generation.subscribe()
    }

    /// Check a TCP or QUIC `Hello` against the parties in force now.
    pub fn check_hello(
        &self,
        channel: Channel,
        nonce: &[u8; NONCE_LEN],
        exporter: &[u8; BINDING_LEN],
        protocol_version: u16,
        party_id: &str,
        signature: &[u8],
    ) -> HelloOutcome {
        let set = self.party_set();
        check_hello(&set, &self.node_id, channel, nonce, exporter, protocol_version, party_id, signature)
    }

    /// Write the warn line for a failed login, at most one per source IP per
    /// 10 seconds. `party` is passed only when it names an enrolled party.
    pub fn log_login_failure(&self, source: IpAddr, channel: Channel, party: Option<&str>, reason: &str, cause: &str) {
        if let Some(suppressed) = self.warn_lines.admit(source, Instant::now()) {
            match party {
                Some(party) => warn!(
                    source = %source,
                    channel = channel.name(),
                    party = %party,
                    reason = reason,
                    cause = cause,
                    suppressed_in_last_window = suppressed,
                    "login refused"
                ),
                None => warn!(
                    source = %source,
                    channel = channel.name(),
                    reason = reason,
                    cause = cause,
                    suppressed_in_last_window = suppressed,
                    "login refused"
                ),
            }
        }
    }

    /// Write the warn line for a publish or subscribe that a logged-in
    /// party's grants or quota refused, rate-limited like the login warnings:
    /// at most one per source IP per 10 seconds, with the count held back.
    /// The party is authenticated, so its id is named.
    pub fn log_refusal(&self, source: IpAddr, party: &str, topic: &str, what: &str) {
        if let Some(suppressed) = self.refusal_lines.admit(source, Instant::now()) {
            warn!(
                source = %source,
                party = %party,
                topic = ?truncate_for_log(topic),
                refused = what,
                suppressed_in_last_window = suppressed,
                "refused by the party's grants"
            );
        }
    }

    /// Write the warn line for a REST request that a party's grants, quota or
    /// token cap refused, rate-limited per party: at most one per party per
    /// 10 seconds, with the count held back.
    pub fn log_rest_refusal(&self, party: &str, topic: Option<&str>, what: &str) {
        let Some(suppressed) = self.rest_refusal_lines.admit_key(party.to_string(), Instant::now()) else {
            return;
        };
        match topic {
            Some(topic) => warn!(
                party = %party,
                topic = ?truncate_for_log(topic),
                refused = what,
                suppressed_in_last_window = suppressed,
                "REST request refused"
            ),
            None => warn!(
                party = %party,
                refused = what,
                suppressed_in_last_window = suppressed,
                "REST request refused"
            ),
        }
    }

    /// Write the warn line for a connection refused at accept by the
    /// pre-login limits, rate-limited like a failed login.
    pub fn log_refused_at_accept(&self, source: IpAddr, channel: Channel) {
        if let Some(suppressed) = self.warn_lines.admit(source, Instant::now()) {
            warn!(
                source = %source,
                channel = channel.name(),
                suppressed_in_last_window = suppressed,
                "connection refused at accept: pre-login limit reached"
            );
        }
    }

    /// Start the once-a-second reload and revocation check. Only the first
    /// call starts it. It stops when `shutdown` fires or the node's
    /// authentication state is dropped.
    pub fn spawn_revocation_task(self: &Arc<Self>, shutdown: Option<broadcast::Receiver<()>>) {
        if self.revocation_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut shutdown = shutdown;
            let mut ticker = tokio::time::interval(RELOAD_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                match shutdown.as_mut() {
                    Some(rx) => {
                        tokio::select! {
                            _ = ticker.tick() => {}
                            _ = rx.recv() => break,
                        }
                    }
                    None => {
                        ticker.tick().await;
                    }
                }
                let Some(auth) = weak.upgrade() else { break };
                auth.revocation_tick().await;
            }
        });
    }

    /// One pass of the check: re-read `parties.toml`, then end the sessions
    /// and drop the tokens of every party that is no longer enabled, or that
    /// holds another key now, and, when a new set is in force, tell every
    /// connection and stream to re-check its grants. It also clears expired
    /// REST challenges and writes the summary line for every source, and
    /// every party on REST, whose warn window closed with lines held back.
    pub async fn revocation_tick(&self) -> RevocationReport {
        let parties = Arc::clone(&self.parties);
        let reload = match tokio::task::spawn_blocking(move || parties.reload()).await {
            Ok(outcome) => outcome,
            Err(e) => ReloadOutcome::KeptLastGood(format!("the reload task failed: {e}")),
        };
        let set = self.party_set();
        let sessions_dropped = self.sessions.drop_inactive(&set);
        let now = Instant::now();
        let tokens_dropped = self.tokens.retain_active(&set, now);
        // A new set in force: every connection and stream re-checks its
        // grants against it, and drops what it is no longer granted.
        if reload == ReloadOutcome::Replaced {
            self.parties_generation.send_modify(|generation| *generation = generation.wrapping_add(1));
        }
        self.challenges.purge_expired(now);
        for (source, suppressed) in self.warn_lines.flush(now) {
            warn!(
                source = %source,
                suppressed_in_last_window = suppressed,
                "failed logins from this source were held back from the log in the last 10 seconds"
            );
        }
        for (source, suppressed) in self.refusal_lines.flush(now) {
            warn!(
                source = %source,
                suppressed_in_last_window = suppressed,
                "refused publishes and subscribes from this source were held back from the log in the last 10 seconds"
            );
        }
        for (party, suppressed) in self.rest_refusal_lines.flush(now) {
            warn!(
                party = %party,
                suppressed_in_last_window = suppressed,
                "refused REST requests from this party were held back from the log in the last 10 seconds"
            );
        }

        let mut revoked: Vec<&String> = sessions_dropped.keys().chain(tokens_dropped.keys()).collect();
        revoked.sort();
        revoked.dedup();
        for party in revoked {
            info!(
                party = %party,
                live_connections_dropped = sessions_dropped.get(party).copied().unwrap_or(0),
                tokens_dropped = tokens_dropped.get(party).copied().unwrap_or(0),
                "revocation applied"
            );
        }
        RevocationReport { reload, sessions_dropped, tokens_dropped }
    }
}

fn startup_refused(message: String) -> NodeError {
    error!("stream node not started: {message}");
    NodeError::Startup(message)
}

/// Open a key file with the owner-only check and read it through the handle
/// that check returned, so the bytes read are the file checked. A refusal
/// stops startup; the log line names the path and the account the check
/// found, and the owner when another account owns the file.
fn read_private(owner_only: OwnerOnly, path: &Path, what: &str) -> Result<Vec<u8>, NodeError> {
    let mut checked = (owner_only.verify_open)(path)
        .map_err(|e| startup_refused(format!("the {what} failed the owner-only check: {}", owner_only_refusal(path, &e))))?;
    let mut bytes = Vec::new();
    checked
        .read_to_end(&mut bytes)
        .map_err(|e| startup_refused(format!("cannot read the {what} {}: {e}", path.display())))?;
    Ok(bytes)
}

// ─────────────────────────────────────────────────────────────────────────────
// Test hook (feature `test-hooks` only)
// ─────────────────────────────────────────────────────────────────────────────

/// Test hook, compiled only under the non-default feature `test-hooks`, which
/// no shipped build enables: a count, per thread, of the dummy signature
/// checks that reached the Ed25519 verify. `verify_signed_data` verifies
/// exactly when the key decodes and the signature is 64 bytes; a key that does
/// not decode returns `InvalidKey` before the verify. A test on a
/// current-thread runtime, where the node's tasks run on the test's thread,
/// reads its own count.
#[cfg(feature = "test-hooks")]
pub mod burn_hook {
    use std::cell::Cell;

    use eustress_identity::IdentityError;

    thread_local! {
        static VERIFIES: Cell<u64> = const { Cell::new(0) };
    }

    /// Count one dummy check, if it reached the verify.
    pub(crate) fn record(signature: &[u8], checked: &Result<(), IdentityError>) {
        if signature.len() == 64 && !matches!(checked, Err(IdentityError::InvalidKey)) {
            VERIFIES.with(|count| count.set(count.get() + 1));
        }
    }

    /// The dummy checks this thread ran through to the verify since the last
    /// call. The count starts again at zero.
    pub fn take() -> u64 {
        VERIFIES.with(|count| count.replace(0))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, last))
    }

    fn party(id: &str, enabled: bool, key_b64: &str) -> Party {
        Party {
            id: id.to_string(),
            enabled,
            admin: false,
            max_subscriptions: 2,
            public_key_b64: key_b64.to_string(),
            key_fingerprint: sha256(key_b64.as_bytes()),
            subscribe: ["log/output".to_string()].into_iter().collect(),
            publish: ["log/output".to_string(), "scene_deltas".to_string()].into_iter().collect(),
        }
    }

    #[test]
    fn transcripts_with_different_field_splits_differ() {
        let nonce = [7u8; NONCE_LEN];
        let binding = [9u8; BINDING_LEN];
        let a = tcp_transcript("ab", &nonce, &binding, "cd", 1).unwrap();
        let b = tcp_transcript("abc", &nonce, &binding, "d", 1).unwrap();
        assert_ne!(a, b, "length prefixes must keep the split between node id and party id");
    }

    #[test]
    fn transcript_layout_is_label_zero_then_prefixed_fields() {
        let nonce = [1u8; NONCE_LEN];
        let binding = [2u8; BINDING_LEN];
        let t = tcp_transcript("n1", &nonce, &binding, "p", 1).unwrap();
        let mut want = Vec::new();
        want.extend_from_slice(b"eustress-stream-login-v1");
        want.push(0);
        want.extend_from_slice(&[2, 0]);
        want.extend_from_slice(b"n1");
        want.extend_from_slice(&nonce);
        want.extend_from_slice(&binding);
        want.extend_from_slice(&[1, 0]);
        want.extend_from_slice(b"p");
        want.extend_from_slice(&[1, 0]);
        assert_eq!(t, want);
    }

    #[test]
    fn each_channel_has_its_own_label() {
        let nonce = [3u8; NONCE_LEN];
        let binding = [4u8; BINDING_LEN];
        let tcp = tcp_transcript("node", &nonce, &binding, "p", 1).unwrap();
        let quic = quic_transcript("node", &nonce, &binding, "p", 1).unwrap();
        let rest = rest_transcript("node", &nonce, &binding, "p", 1, TokenClass::Session).unwrap();
        assert_ne!(tcp, quic);
        assert_ne!(tcp[..], rest[..rest.len() - 1]);
        assert_ne!(quic[..], rest[..rest.len() - 1]);
    }

    #[test]
    fn rest_transcript_carries_the_token_class() {
        let nonce = [5u8; NONCE_LEN];
        let hash = [6u8; BINDING_LEN];
        let session = rest_transcript("node", &nonce, &hash, "p", 1, TokenClass::Session).unwrap();
        let browser = rest_transcript("node", &nonce, &hash, "p", 1, TokenClass::Browser).unwrap();
        assert_eq!(session.last(), Some(&0x01));
        assert_eq!(browser.last(), Some(&0x02));
    }

    #[test]
    fn transcript_refuses_mismatched_fields() {
        let nonce = [0u8; NONCE_LEN];
        let binding = [0u8; BINDING_LEN];
        assert!(transcript(Channel::Tcp, "n", &nonce, Binding::CertSha256(&binding), "p", 1, None).is_err());
        assert!(transcript(Channel::Rest, "n", &nonce, Binding::Exporter(&binding), "p", 1, Some(TokenClass::Session)).is_err());
        assert!(transcript(Channel::Rest, "n", &nonce, Binding::CertSha256(&binding), "p", 1, None).is_err());
        assert!(tcp_transcript(&"n".repeat(65), &nonce, &binding, "p", 1).is_err());
    }

    #[test]
    fn party_id_format() {
        assert!(is_valid_party_id("north-plant_2"));
        assert!(is_valid_party_id(&"a".repeat(64)));
        assert!(!is_valid_party_id(""));
        assert!(!is_valid_party_id(&"a".repeat(65)));
        assert!(!is_valid_party_id("north plant"));
        assert!(!is_valid_party_id("north/plant"));
        assert!(!is_valid_party_id("n\u{e9}"));
    }

    #[test]
    fn malformed_party_id_is_refused_before_lookup() {
        // A set that, against the loader's rules, holds an entry under a
        // malformed id. The format check must refuse before the lookup could
        // find it.
        let mut set = PartySet::default();
        set.parties.insert("bad id".to_string(), Arc::new(party("bad id", true, TIMING_KEY_B64)));
        let outcome = check_hello(&set, "node", Channel::Tcp, &[0; 32], &[0; 32], 1, "bad id", &[0; 64]);
        match outcome {
            HelloOutcome::Refused(r) => {
                assert_eq!(r.reason, REASON_AUTH_FAILED);
                assert_eq!(r.cause, "malformed party id");
                assert_eq!(r.enrolled_party, None);
            }
            HelloOutcome::Welcome(_) => panic!("a malformed id must never reach the lookup"),
        }
    }

    #[test]
    fn version_is_checked_first() {
        let set = PartySet::default();
        match check_hello(&set, "node", Channel::Tcp, &[0; 32], &[0; 32], 2, "", &[]) {
            HelloOutcome::Refused(r) => assert_eq!(r.reason, REASON_VERSION_UNSUPPORTED),
            HelloOutcome::Welcome(_) => panic!("version 2 must be refused"),
        }
    }

    #[test]
    fn grants_default_deny_and_reserved_topics_need_admin() {
        let p = party("p", true, TIMING_KEY_B64);
        assert!(p.may_subscribe("log/output"));
        assert!(!p.may_subscribe("scene_deltas"));
        assert!(p.may_publish("log/output"));
        assert!(!p.may_publish("other"));
        assert!(!p.may_publish("scene_deltas"), "a reserved topic needs admin even when listed");
        let mut admin = party("a", true, TIMING_KEY_B64);
        admin.admin = true;
        assert!(admin.may_publish("scene_deltas"));
        assert!(!admin.may_publish("agent_commands"), "admin does not grant an unlisted topic");
    }

    #[test]
    fn parties_file_defaults_and_duplicates() {
        let text = format!(
            "[[party]]\nid = \"a\"\npublic_key = \"{k}\"\n\n[[party]]\nid = \"b\"\npublic_key = \"{k}\"\nenabled = true\n\n[[party]]\nid = \"b\"\npublic_key = \"{k}\"\nenabled = true\n\n[[party]]\nid = \"bad id\"\npublic_key = \"{k}\"\nenabled = true\n",
            k = TIMING_KEY_B64
        );
        let file = PartiesFile::parse(&text).unwrap();
        let (set, problems) = PartySet::from_file(&file);
        let a = set.get("a").expect("a is enrolled");
        assert!(!a.enabled, "a missing `enabled` reads as false");
        assert_eq!(a.max_subscriptions, DEFAULT_MAX_SUBSCRIPTIONS);
        assert!(set.get("b").is_none(), "a duplicated id is left out entirely");
        assert!(set.get("bad id").is_none());
        assert_eq!(problems.len(), 2);
        assert_eq!(set.enabled_count(), 0);
    }

    #[test]
    fn per_ip_limiter_refuses_the_17th_from_one_ip_and_admits_another_ip() {
        let limiter = PreLoginLimiter::new();
        let held: Vec<_> = (0..MAX_PRE_LOGIN_PER_IP).map(|_| limiter.try_acquire(ip(1)).expect("within the cap")).collect();
        assert!(limiter.try_acquire(ip(1)).is_none(), "the 17th from one IP is refused");
        let other = limiter.try_acquire(ip(2));
        assert!(other.is_some(), "another IP is admitted");
        drop(held);
        assert_eq!(limiter.in_use_for(ip(1)), 0, "dropped permits free their slots");
        assert!(limiter.try_acquire(ip(1)).is_some());
    }

    #[test]
    fn limiter_caps_the_total() {
        let limiter = PreLoginLimiter::with_limits(3, 2);
        let _a = limiter.try_acquire(ip(1)).unwrap();
        let _b = limiter.try_acquire(ip(2)).unwrap();
        let _c = limiter.try_acquire(ip(3)).unwrap();
        assert!(limiter.try_acquire(ip(4)).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn warn_lines_are_one_per_source_per_window_with_a_count() {
        let limiter: WarnLimiter = WarnLimiter::new();
        let start = Instant::now();
        assert_eq!(limiter.admit(ip(1), start), Some(0));
        assert_eq!(limiter.admit(ip(1), start), None);
        assert_eq!(limiter.admit(ip(1), start + Duration::from_secs(9)), None);
        assert_eq!(limiter.admit(ip(2), start), Some(0), "another source has its own window");
        assert_eq!(limiter.admit(ip(1), start + WARN_WINDOW), Some(2));
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_that_simply_stops_is_still_summarized() {
        let limiter: WarnLimiter = WarnLimiter::new();
        let start = Instant::now();
        assert_eq!(limiter.admit(ip(1), start), Some(0));
        for _ in 0..3 {
            assert_eq!(limiter.admit(ip(1), start + Duration::from_secs(1)), None);
        }
        assert_eq!(limiter.admit(ip(2), start), Some(0), "a source with nothing held back");
        assert!(limiter.flush(start + Duration::from_secs(9)).is_empty(), "the window is still open");
        assert_eq!(limiter.flush(start + WARN_WINDOW), vec![(ip(1), 3)], "the burst stopped; its count is reported once");
        assert!(limiter.flush(start + WARN_WINDOW * 2).is_empty(), "and only once");
        assert_eq!(limiter.admit(ip(1), start + WARN_WINDOW), Some(0), "the next line starts a new window");
    }

    #[tokio::test(start_paused = true)]
    async fn tokens_expire_and_are_stored_hashed() {
        let table = TokenTable::new();
        let now = Instant::now();
        let p = party("p", true, TIMING_KEY_B64);
        let issued = table.issue(&p, TokenClass::Browser, now).unwrap();
        let raw = STANDARD.decode(&issued.token).unwrap();
        assert!(!table.state.lock().entries.contains_key(raw.as_slice()), "the raw token is never a key");
        let grant = table.validate(&issued.token, now + BROWSER_TOKEN_LIFETIME - Duration::from_secs(1));
        assert_eq!(grant.map(|g| (g.party_id, g.expires_at)), Some(("p".to_string(), now + BROWSER_TOKEN_LIFETIME)));
        assert!(table.validate(&issued.token, now + BROWSER_TOKEN_LIFETIME).is_none());
        assert!(table.is_empty(), "an expired token is removed when it is seen");
        assert_eq!(table.live_for("p"), 0, "and the party's count with it");
    }

    #[tokio::test(start_paused = true)]
    async fn token_table_is_bounded() {
        let table = TokenTable::new();
        let now = Instant::now();
        for n in 0..(TOKEN_TABLE_MAX / MAX_TOKENS_PER_PARTY) {
            let p = party(&format!("p{n}"), true, TIMING_KEY_B64);
            for _ in 0..MAX_TOKENS_PER_PARTY {
                table.issue(&p, TokenClass::Session, now).unwrap();
            }
        }
        let late = party("late", true, TIMING_KEY_B64);
        assert_eq!(table.issue(&late, TokenClass::Session, now).unwrap_err(), TokenRefusal::Full);
        // Once they expire, the space comes back.
        assert!(table.issue(&late, TokenClass::Session, now + SESSION_TOKEN_LIFETIME).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn tokens_per_party_are_capped_and_the_count_follows_expiry_and_revocation() {
        let table = TokenTable::new();
        let now = Instant::now();
        let p = party("p", true, TIMING_KEY_B64);
        let q = party("q", true, TIMING_KEY_B64);
        for _ in 0..MAX_TOKENS_PER_PARTY {
            table.issue(&p, TokenClass::Browser, now).unwrap();
        }
        assert_eq!(table.issue(&p, TokenClass::Browser, now).unwrap_err(), TokenRefusal::PartyLimit);
        assert!(table.issue(&q, TokenClass::Browser, now).is_ok(), "another party is unaffected");
        assert_eq!(table.live_for("p"), MAX_TOKENS_PER_PARTY);

        // Expiry: the count comes down and the party may log in again.
        let later = now + BROWSER_TOKEN_LIFETIME;
        assert!(table.issue(&p, TokenClass::Browser, later).is_ok());
        assert_eq!(table.live_for("p"), 1);

        // Revocation: a set without p drops its token and its count.
        let mut set = PartySet::default();
        set.parties.insert("q".to_string(), Arc::new(party("q", true, TIMING_KEY_B64)));
        let dropped = table.retain_active(&set, later);
        assert_eq!(dropped.get("p"), Some(&1));
        assert_eq!(table.live_for("p"), 0);
    }

    #[test]
    fn limit_key_folds_ipv4_mapped_addresses_and_keys_ipv6_by_its_64() {
        let mapped: IpAddr = "::ffff:10.0.0.7".parse().unwrap();
        assert_eq!(limit_key(mapped), ip(7), "an IPv4-mapped address counts as its IPv4 address");
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff:1:2:3".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(limit_key(a), limit_key(b), "one /64, one key");
        assert_ne!(limit_key(a), limit_key(other), "another /64, another key");
        assert_eq!(limit_key(ip(1)), ip(1), "IPv4 is counted per address");
    }

    #[tokio::test(start_paused = true)]
    async fn challenges_are_capped_per_source_found_by_nonce_and_single_use() {
        let table = ChallengeTable::new();
        let now = Instant::now();
        let nonces: Vec<_> = (0..MAX_CHALLENGES_PER_IP).map(|_| table.issue(ip(1), now).unwrap()).collect();
        assert_eq!(table.issue(ip(1), now).unwrap_err(), ChallengeRefusal::TooManyFromSource);
        assert!(table.issue(ip(2), now).is_ok(), "another source has its own cap");
        assert!(table.take(&nonces[0], now), "a live nonce is taken, whatever address the login comes from");
        assert!(!table.take(&nonces[0], now), "a nonce is used once");
        assert!(!table.take(&[9u8; NONCE_LEN], now), "an unknown nonce");
        assert_eq!(table.outstanding_for(ip(1)), MAX_CHALLENGES_PER_IP - 1, "a used nonce frees its slot");
        assert!(table.issue(ip(1), now).is_ok());
        assert!(!table.take(&nonces[1], now + CHALLENGE_LIFETIME), "challenges live 5 seconds");
        assert!(!table.take(&nonces[1], now), "an expired nonce is gone once seen");
    }

    #[tokio::test(start_paused = true)]
    async fn challenge_table_caps_the_total_and_expired_challenges_free_it() {
        let table = ChallengeTable::new();
        let now = Instant::now();
        for n in 0..(MAX_CHALLENGES_TOTAL / MAX_CHALLENGES_PER_IP) {
            let source = IpAddr::V4(Ipv4Addr::new(10, 1, (n / 256) as u8, (n % 256) as u8));
            for _ in 0..MAX_CHALLENGES_PER_IP {
                table.issue(source, now).expect("within both caps");
            }
        }
        assert_eq!(table.outstanding(), MAX_CHALLENGES_TOTAL);
        assert_eq!(table.issue(ip(1), now).unwrap_err(), ChallengeRefusal::Full);
        assert!(table.issue(ip(1), now + CHALLENGE_LIFETIME).is_ok(), "expired challenges free the table");
        assert_eq!(table.outstanding(), 1);
    }

    /// Log output, kept for the test that reads it.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// An owner refusal from the helper stops startup, and both the error and
    /// the startup log line name the owner. The refusal is the helper's own
    /// `NotOwner`, the error `verify_open` returns for a file another account
    /// owns. A real one needs a second account, so the test hands startup a
    /// helper that returns it; the helper's own tests cover the refusal.
    #[test]
    fn an_owner_refusal_stops_startup_and_the_log_names_the_owner() {
        const OWNER: &str = "S-1-5-21-1-2-3-1001";
        fn another_owner(path: &Path) -> std::io::Result<std::fs::File> {
            Err(eustress_identity::private_file::NotOwner {
                path: path.display().to_string(),
                owner: OWNER.to_string(),
                trusted: false,
                advice: "take ownership with: icacls <file> /setowner <user>".to_string(),
            }
            .into())
        }
        fn passes(_: &Path) -> std::io::Result<()> {
            Ok(())
        }
        let owner_only = OwnerOnly { verify: passes, verify_open: another_owner };
        let config = NodeConfig::default()
            .with_tls("node.crt", "private/node.key")
            .with_parties("private/parties.toml");

        let logs = Captured::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish();
        let started = tracing::subscriber::with_default(subscriber, || NodeAuth::from_config_with(&config, owner_only));

        let message = match started {
            Err(NodeError::Startup(message)) => message,
            Err(other) => panic!("expected a startup refusal, got {other}"),
            Ok(_) => panic!("an owner refusal must stop startup"),
        };
        assert!(message.contains("TLS private key failed the owner-only check"), "{message}");
        assert!(message.contains(&format!("is owned by {OWNER}, another account")), "{message}");
        let log = String::from_utf8_lossy(&logs.0.lock()).into_owned();
        let lines: Vec<&str> = log.lines().filter(|line| line.contains("stream node not started")).collect();
        assert_eq!(lines.len(), 1, "one startup refusal line: {log}");
        assert!(lines[0].contains("ERROR"), "{}", lines[0]);
        assert!(lines[0].contains(OWNER), "the log line names the owner: {}", lines[0]);
    }

    /// The timing key is the RFC 8032 section 7.1 test-1 public key, a valid
    /// Ed25519 point. A key that does not decode makes `burn_signature_check`
    /// return before it checks any signature, so an unknown party would be
    /// refused faster than a wrong key; a typo here fails this test. Under
    /// `test-hooks` it also checks that the dummy check reaches the verify.
    #[test]
    fn timing_key_is_the_rfc_8032_test_1_public_key() {
        let key = eustress_identity::keypair::decode_public_key(TIMING_KEY_B64)
            .expect("the timing key decodes as an Ed25519 public key");
        let hex: String = key.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        #[cfg(feature = "test-hooks")]
        {
            burn_hook::take();
            burn_signature_check(&[0u8; 64]);
            assert_eq!(burn_hook::take(), 1, "the dummy check reaches the signature verify");
        }
    }
}
