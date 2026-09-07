//! # Manifest keys
//!
//! Key material for the published website manifest: minting, storage,
//! rotation, revocation and verification.
//!
//! ## What a manifest key is, and what it is not
//!
//! Read this before writing any copy about it.
//!
//! A key **names the caller**. Traffic can be attributed per key, rate limited
//! per key, and cut off per key. An author can see which sites are live on
//! their numbers and stop one that misbehaves. That is real and it is the
//! entire value of the mechanism.
//!
//! A key **does not make the manifest secret**. A browser key ships inside the
//! page's JavaScript, so it is visible in the page source and in the network
//! tab to anyone who opens devtools. It is published the moment it deploys.
//! Rotating it does not un-publish the values it fetched. Nothing in this file
//! provides confidentiality, and nothing built on it should claim to.
//!
//! If confidentiality is the actual requirement, the answer is a server-side
//! proxy holding a key the browser never sees, or short-lived signed tokens
//! minted per session. Both are real designs. Neither is this one, and a
//! longer key string is not a substitute for either.
//!
//! ## Two kinds, told apart by prefix
//!
//! | prefix | kind | where it lives |
//! |---|---|---|
//! | `eus_pk_` | browser key, public by design | the site's JavaScript |
//! | `eus_bk_` | build key, held as a CI secret | the deploy pipeline |
//!
//! Same manifest, different exposure. Separately revocable, so killing a
//! leaked browser key leaves the build pipeline running. The prefix exists so
//! a leaked key is greppable and so the two can never be confused.
//!
//! ## Only the hash is stored
//!
//! [`KeySet`] holds a BLAKE3 hash of each key and never the key itself. The
//! plaintext is returned exactly once, by [`KeySet::generate`] or
//! [`KeySet::rotate`], and is gone the moment the caller drops it.
//!
//! Two consequences, both deliberate:
//!
//! 1. A dump of a Space's `_service.toml` hands nobody a working key.
//! 2. The Properties panel can genuinely show a key in full exactly once. A
//!    design that stores plaintext cannot make that promise, because the value
//!    is still sitting there for the next reader.
//!
//! This differs from the illustrative `_service.toml` in WEBSITE_SERVICE.md
//! 2.1, which shows `current = "eus_pk_..."` in the clear. The spec's own
//! section 6.2 requires the Worker to look a key up by hash and 6.3 requires
//! the panel to show it once, and both of those are only true if the plaintext
//! is never written down. The hash form is what this module implements.
//!
//! ## Table of Contents
//!
//! 1. Constants and [`KeyKind`]
//! 2. [`KeyRecord`] and [`MintedKey`] - the stored form and the once-only form
//! 3. [`KeySet`] - mint, rotate, revoke, verify, mask
//! 4. Flat `[properties]` serialization for `_service.toml`
//! 5. Tests

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};

// ============================================================================
// 1. Constants and KeyKind
// ============================================================================

/// Browser key prefix. Public by design.
pub const BROWSER_PREFIX: &str = "eus_pk_";
/// Build key prefix. Held as a CI secret, never shipped to a page.
pub const BUILD_PREFIX: &str = "eus_bk_";

/// Days a rotated-out key keeps working.
///
/// The overlap is not politeness. A consumer is a deployed website, and
/// updating one means a person editing a config and running a deploy. With no
/// overlap, rotation breaks every live site the instant it happens, so nobody
/// rotates, so a leaked key stays live forever. Thirty days survives a holiday.
pub const DEFAULT_OVERLAP_DAYS: i64 = 30;

/// Requests per minute per key before the Worker returns 429.
pub const DEFAULT_RATE_LIMIT_PER_MINUTE: u32 = 60;

/// Short-window allowance above the sustained rate.
pub const DEFAULT_BURST: u32 = 120;

/// Bytes of OS entropy behind every key. 256 bits, hex encoded to 64
/// characters after the prefix.
const SECRET_BYTES: usize = 32;

/// Hex characters of the hash used as the human-facing key id. Six is enough
/// to tell two of an author's keys apart in a log line and carries no useful
/// information about the key itself.
const ID_HEX_LEN: usize = 6;

/// Which exposure a key is issued for.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyKind {
    /// Ships in a page. Readable by anyone who opens devtools.
    Browser,
    /// Held by CI. Never reaches a browser.
    Build,
}

impl KeyKind {
    /// Literal prefix every key of this kind carries.
    pub fn prefix(self) -> &'static str {
        match self {
            KeyKind::Browser => BROWSER_PREFIX,
            KeyKind::Build => BUILD_PREFIX,
        }
    }

    /// Prefix on the key id (`wk_3f8a12`, `bk_a41e77`).
    pub fn id_prefix(self) -> &'static str {
        match self {
            KeyKind::Browser => "wk_",
            KeyKind::Build => "bk_",
        }
    }

    /// Stem for the flat `[properties]` keys in `_service.toml`.
    ///
    /// Flat, not nested, because `service_loader::toml_to_property_value`
    /// returns `None` for `toml::Value::Table`: a `[website.key]` section
    /// parses and is then silently dropped from `ServiceComponent.properties`,
    /// so the Properties panel would render nothing and persist nothing.
    pub fn property_stem(self) -> &'static str {
        match self {
            KeyKind::Browser => "WebsiteKey",
            KeyKind::Build => "BuildKey",
        }
    }

    /// Human label for panel copy and errors.
    pub fn label(self) -> &'static str {
        match self {
            KeyKind::Browser => "browser key",
            KeyKind::Build => "build key",
        }
    }
}

// ============================================================================
// 2. KeyRecord and MintedKey
// ============================================================================

/// What is kept about a key once it exists: an id, a hash, and when it was
/// issued. Never the key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyRecord {
    /// Stable public identifier, derived from the hash so it leaks nothing.
    pub id: String,
    /// Lowercase hex BLAKE3 of the full key string, prefix included.
    pub hash: String,
    /// RFC 3339 UTC, seconds precision.
    pub created_at: String,
}

/// A freshly minted key. `secret` is the only time the plaintext exists;
/// show it to the author once and drop it.
#[derive(Clone, Debug)]
pub struct MintedKey {
    /// The full key, prefix included. Show once, store never.
    pub secret: String,
    /// What actually gets written to `_service.toml`.
    pub record: KeyRecord,
}

/// Result of checking a candidate key against a [`KeySet`].
///
/// The Worker maps these onto status codes: [`KeyVerdict::Current`] and
/// [`KeyVerdict::Overlap`] to 200, [`KeyVerdict::Revoked`] to 403, everything
/// else to 401. A consumer treats 401 and 403 identically, because from the
/// page's side both mean the key stopped working and the right response to
/// both is to keep the values already on screen. The distinction is for the
/// operator reading logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyVerdict {
    /// Matches the live key.
    Current,
    /// Matches the rotated-out key and is still inside its overlap window.
    Overlap {
        /// RFC 3339 UTC instant the old key stops working.
        expires_at: String,
    },
    /// Matches the rotated-out key but the overlap window has closed.
    Expired {
        /// RFC 3339 UTC instant the old key stopped working.
        expired_at: String,
    },
    /// Every key for this kind was revoked. Revocation has no overlap.
    Revoked {
        /// RFC 3339 UTC instant of the revocation.
        at: String,
    },
    /// No key has ever been issued for this kind.
    NoKey,
    /// Well-formed request, unrecognised key.
    Unknown,
}

/// Lowercase hex SHA-256 of a key string. The stored form, and the only form
/// a lookup ever compares against.
///
/// SHA-256 specifically, and not BLAKE3, because the worker names the algorithm
/// on the wire: it stores and validates `sha256:<64 hex>`. BLAKE3 also emits 64
/// hex characters, so the worker's format guard ACCEPTS a BLAKE3 digest and
/// then fails every comparison against it. The two sides have to agree on the
/// function, not merely on the digest width.
pub fn fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The wire form the worker stores and compares: `sha256:<64 lowercase hex>`.
pub fn fingerprint_wire(secret: &str) -> String {
    format!("sha256:{}", fingerprint(secret))
}

/// Collapse a property name to letters and digits, lowercased.
///
/// `WebsiteKeyId`, `website_key_id` and `websiteKeyId` all squash to
/// `websitekeyid`, so a service file reads the same whether or not it went
/// through the loader's key normaliser.
pub fn squash(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Compare two hex digests without an early exit.
///
/// The inputs are hashes rather than secrets, so the exposure here is small,
/// but the caller-supplied side is attacker controlled and the cost of not
/// leaking a match prefix is four lines.
fn digests_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn now_rfc3339(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// 256 bits from the OS CSPRNG.
///
/// `OsRng` rather than `thread_rng` so the entropy path for key material is
/// the operating system's and not a userspace PRNG that happens to be seeded
/// from it.
fn random_secret(kind: KeyKind) -> String {
    use rand::RngCore;
    let mut buf = [0u8; SECRET_BYTES];
    let mut rng = rand::rngs::OsRng;
    rng.fill_bytes(&mut buf);
    format!("{}{}", kind.prefix(), hex::encode(buf))
}

// ============================================================================
// 3. KeySet
// ============================================================================

/// Every key of one kind for one Website service, plus its rotation and rate
/// limit policy.
#[derive(Clone, Debug)]
pub struct KeySet {
    /// Browser or build. Fixes the prefixes and the property stem.
    pub kind: KeyKind,
    /// The live key, if one has been issued.
    pub current: Option<KeyRecord>,
    /// The rotated-out key, valid until `rotated_at` plus `overlap_days`.
    pub previous: Option<KeyRecord>,
    /// RFC 3339 UTC of the last rotation. Anchors the overlap window.
    pub rotated_at: Option<String>,
    /// RFC 3339 UTC of the last revocation, if any.
    pub revoked_at: Option<String>,
    /// Days `previous` keeps working after `rotated_at`.
    pub overlap_days: i64,
    /// Sustained per-key request budget.
    pub rate_limit_per_minute: u32,
    /// Short-window allowance above the sustained rate.
    pub burst: u32,
}

impl KeySet {
    /// An unissued key set with the default policy.
    pub fn new(kind: KeyKind) -> Self {
        Self {
            kind,
            current: None,
            previous: None,
            rotated_at: None,
            revoked_at: None,
            overlap_days: DEFAULT_OVERLAP_DAYS,
            rate_limit_per_minute: DEFAULT_RATE_LIMIT_PER_MINUTE,
            burst: DEFAULT_BURST,
        }
    }

    /// True when at least one key has been issued and not revoked.
    pub fn is_issued(&self) -> bool {
        self.current.is_some()
    }

    /// Issue the first key, discarding any rotation history.
    ///
    /// Use this once per site. Afterwards use [`KeySet::rotate`], which keeps
    /// the outgoing key working through its overlap window instead of cutting
    /// every live consumer off at once.
    pub fn generate(&mut self, now: DateTime<Utc>) -> MintedKey {
        let minted = self.mint(now);
        self.current = Some(minted.record.clone());
        self.previous = None;
        self.rotated_at = None;
        self.revoked_at = None;
        minted
    }

    /// Move `current` to `previous`, mint a new `current`, stamp `rotated_at`.
    ///
    /// The outgoing key keeps working until `rotated_at` plus `overlap_days`;
    /// [`KeySet::overlap_expires_at`] is the date to put in the confirmation,
    /// because "rotate" without that date reads as free and it is not.
    ///
    /// Rotating a set that has never been issued simply mints the first key
    /// and leaves the history empty, since there is nothing to overlap with.
    pub fn rotate(&mut self, now: DateTime<Utc>) -> MintedKey {
        let minted = self.mint(now);
        if let Some(outgoing) = self.current.take() {
            self.previous = Some(outgoing);
            self.rotated_at = Some(now_rfc3339(now));
        }
        self.current = Some(minted.record.clone());
        self.revoked_at = None;
        minted
    }

    /// Cut every key of this kind off immediately.
    ///
    /// No overlap. Revocation is the control for a key that is being abused,
    /// and a grace period on that control defeats its only purpose.
    ///
    /// Two honest caveats. A response served from a browser or edge cache
    /// never reaches the Worker, so revocation takes effect within one
    /// `max-age` window rather than instantly. And a pinned `?v=` response,
    /// immutable for a year, is unattributed after its first fetch.
    pub fn revoke(&mut self, now: DateTime<Utc>) {
        self.current = None;
        self.previous = None;
        self.rotated_at = None;
        self.revoked_at = Some(now_rfc3339(now));
    }

    /// When the rotated-out key stops working, if a rotation has happened.
    pub fn overlap_expires_at(&self) -> Option<DateTime<Utc>> {
        let rotated = self.rotated_at.as_deref()?;
        let parsed = DateTime::parse_from_rfc3339(rotated).ok()?;
        Some(parsed.with_timezone(&Utc) + chrono::Duration::days(self.overlap_days))
    }

    /// Check a caller-supplied key.
    ///
    /// Lookup is by hash, never by value, so the stored form cannot be
    /// replayed even by whoever holds it.
    pub fn verify(&self, candidate: &str, now: DateTime<Utc>) -> KeyVerdict {
        if !candidate.starts_with(self.kind.prefix()) {
            return KeyVerdict::Unknown;
        }
        let digest = fingerprint(candidate);

        if let Some(cur) = &self.current {
            if digests_match(&cur.hash, &digest) {
                return KeyVerdict::Current;
            }
        }

        if let Some(prev) = &self.previous {
            if digests_match(&prev.hash, &digest) {
                return match self.overlap_expires_at() {
                    Some(expiry) if now < expiry => KeyVerdict::Overlap {
                        expires_at: now_rfc3339(expiry),
                    },
                    Some(expiry) => KeyVerdict::Expired {
                        expired_at: now_rfc3339(expiry),
                    },
                    // Rotated with no readable timestamp: treat the old key as
                    // dead rather than as live forever.
                    None => KeyVerdict::Expired {
                        expired_at: String::new(),
                    },
                };
            }
        }

        if self.current.is_none() && self.previous.is_none() {
            return match &self.revoked_at {
                Some(at) => KeyVerdict::Revoked { at: at.clone() },
                None => KeyVerdict::NoKey,
            };
        }

        KeyVerdict::Unknown
    }

    /// What Properties shows after the one-time reveal.
    ///
    /// The stars are not decoration hiding a value we hold: the plaintext is
    /// genuinely gone. The id is what identifies this key in logs and in the
    /// rotate and revoke controls.
    pub fn masked(&self) -> String {
        match &self.current {
            Some(rec) => format!("{}******** ({})", self.kind.prefix(), rec.id),
            None => "not issued".to_string(),
        }
    }

    fn mint(&self, now: DateTime<Utc>) -> MintedKey {
        let secret = random_secret(self.kind);
        let hash = fingerprint(&secret);
        let id = format!("{}{}", self.kind.id_prefix(), &hash[..ID_HEX_LEN]);
        MintedKey {
            record: KeyRecord {
                id,
                hash,
                created_at: now_rfc3339(now),
            },
            secret,
        }
    }

    // ========================================================================
    // 4. Flat [properties] serialization
    // ========================================================================

    /// Every `[properties]` key this kind occupies in `_service.toml`.
    ///
    /// The shipped `service_templates/Website/_service.toml` must contain all
    /// of these. A key absent from the template renders in the Properties
    /// panel, accepts typing, and persists nothing, because the dynamic write
    /// back path only writes keys that already exist in the service file.
    pub fn property_keys(kind: KeyKind) -> Vec<String> {
        let s = kind.property_stem();
        [
            "Id",
            "Hash",
            "CreatedAt",
            "PreviousId",
            "PreviousHash",
            "PreviousCreatedAt",
            "RotatedAt",
            "RevokedAt",
            "OverlapDays",
            "RateLimitPerMinute",
            "Burst",
        ]
        .iter()
        .map(|suffix| format!("{s}{suffix}"))
        .collect()
    }

    /// Flat `[properties]` entries for `_service.toml`.
    ///
    /// Every key in [`KeySet::property_keys`] is emitted, empty strings
    /// included, so the panel always has a row to render and the write back
    /// path always has an existing key to overwrite.
    pub fn to_flat_properties(&self) -> Vec<(String, toml::Value)> {
        let s = self.kind.property_stem();
        let text = |v: Option<&str>| toml::Value::String(v.unwrap_or_default().to_string());
        vec![
            (
                format!("{s}Id"),
                text(self.current.as_ref().map(|r| r.id.as_str())),
            ),
            (
                format!("{s}Hash"),
                text(self.current.as_ref().map(|r| r.hash.as_str())),
            ),
            (
                format!("{s}CreatedAt"),
                text(self.current.as_ref().map(|r| r.created_at.as_str())),
            ),
            (
                format!("{s}PreviousId"),
                text(self.previous.as_ref().map(|r| r.id.as_str())),
            ),
            (
                format!("{s}PreviousHash"),
                text(self.previous.as_ref().map(|r| r.hash.as_str())),
            ),
            (
                format!("{s}PreviousCreatedAt"),
                text(self.previous.as_ref().map(|r| r.created_at.as_str())),
            ),
            (format!("{s}RotatedAt"), text(self.rotated_at.as_deref())),
            (format!("{s}RevokedAt"), text(self.revoked_at.as_deref())),
            (
                format!("{s}OverlapDays"),
                toml::Value::Integer(self.overlap_days),
            ),
            (
                format!("{s}RateLimitPerMinute"),
                toml::Value::Integer(self.rate_limit_per_minute as i64),
            ),
            (format!("{s}Burst"), toml::Value::Integer(self.burst as i64)),
        ]
    }

    /// Read a key set back out of a flat `[properties]` table.
    ///
    /// Missing or blank entries mean "not issued", which is the state every
    /// Space starts in, so this never fails.
    ///
    /// Lookup goes through [`squash`], so `WebsiteKeyId`, `websiteKeyId` and
    /// `website_key_id` are one key. A Space that went through the key
    /// normaliser and one that did not both read the same, which matters
    /// because the loader normalises and a hand-edited file does not.
    pub fn from_flat_properties(kind: KeyKind, props: &BTreeMap<String, toml::Value>) -> Self {
        let s = kind.property_stem();
        let lookup: BTreeMap<String, &toml::Value> =
            props.iter().map(|(k, v)| (squash(k), v)).collect();

        let text = |suffix: &str| -> Option<String> {
            lookup
                .get(&squash(&format!("{s}{suffix}")))
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let int = |suffix: &str, fallback: i64| -> i64 {
            lookup
                .get(&squash(&format!("{s}{suffix}")))
                .and_then(|v| v.as_integer())
                .unwrap_or(fallback)
        };

        let record = |id: Option<String>, hash: Option<String>, created: Option<String>| {
            // A record without a hash cannot be verified against, so it is not
            // a record. Dropping it here keeps `verify` from matching on an
            // empty digest.
            hash.map(|hash| KeyRecord {
                id: id.unwrap_or_default(),
                hash,
                created_at: created.unwrap_or_default(),
            })
        };

        Self {
            kind,
            current: record(text("Id"), text("Hash"), text("CreatedAt")),
            previous: record(
                text("PreviousId"),
                text("PreviousHash"),
                text("PreviousCreatedAt"),
            ),
            rotated_at: text("RotatedAt"),
            revoked_at: text("RevokedAt"),
            overlap_days: int("OverlapDays", DEFAULT_OVERLAP_DAYS).max(0),
            rate_limit_per_minute: int("RateLimitPerMinute", DEFAULT_RATE_LIMIT_PER_MINUTE as i64)
                .clamp(0, u32::MAX as i64) as u32,
            burst: int("Burst", DEFAULT_BURST as i64).clamp(0, u32::MAX as i64) as u32,
        }
    }
}

// ============================================================================
// 5. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn at(iso: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(iso)
            .expect("test timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn minted_key_carries_its_kind_prefix_and_full_entropy() {
        let mut browser = KeySet::new(KeyKind::Browser);
        let mut build = KeySet::new(KeyKind::Build);
        let b = browser.generate(at("2026-07-02T09:15:00Z"));
        let c = build.generate(at("2026-07-02T09:15:00Z"));

        assert!(b.secret.starts_with(BROWSER_PREFIX));
        assert!(c.secret.starts_with(BUILD_PREFIX));
        assert_eq!(b.secret.len(), BROWSER_PREFIX.len() + SECRET_BYTES * 2);
        assert!(b.record.id.starts_with("wk_"));
        assert!(c.record.id.starts_with("bk_"));
        assert_ne!(b.secret, c.secret);
    }

    #[test]
    fn two_mints_never_collide() {
        let mut set = KeySet::new(KeyKind::Browser);
        let a = set.generate(at("2026-07-02T09:15:00Z"));
        let b = set.generate(at("2026-07-02T09:15:00Z"));
        assert_ne!(a.secret, b.secret);
        assert_ne!(a.record.hash, b.record.hash);
    }

    #[test]
    fn only_the_hash_is_stored() {
        let mut set = KeySet::new(KeyKind::Browser);
        let minted = set.generate(at("2026-07-02T09:15:00Z"));

        // The secret must not survive anywhere in the persisted form.
        let flat = set.to_flat_properties();
        let rendered = format!("{flat:?}");
        assert!(
            !rendered.contains(&minted.secret),
            "plaintext key leaked into the stored properties"
        );
        assert!(rendered.contains(&minted.record.hash));
        assert!(!set.masked().contains(&minted.secret));
    }

    #[test]
    fn rotation_keeps_the_old_key_working_through_the_overlap() {
        let mut set = KeySet::new(KeyKind::Browser);
        let first = set.generate(at("2026-07-02T09:15:00Z"));
        let rotated_at = at("2026-08-26T07:41:00Z");
        let second = set.rotate(rotated_at);

        assert_ne!(first.secret, second.secret);
        assert_eq!(
            set.previous.as_ref().map(|r| r.hash.clone()),
            Some(first.record.hash.clone())
        );
        assert_eq!(
            set.current.as_ref().map(|r| r.hash.clone()),
            Some(second.record.hash.clone())
        );

        // New key live immediately.
        assert_eq!(set.verify(&second.secret, rotated_at), KeyVerdict::Current);

        // Old key still live one day in.
        let day_after = at("2026-08-27T07:41:00Z");
        assert!(matches!(
            set.verify(&first.secret, day_after),
            KeyVerdict::Overlap { .. }
        ));

        // Old key dead one day past the window.
        let past_window = rotated_at + chrono::Duration::days(DEFAULT_OVERLAP_DAYS + 1);
        assert!(matches!(
            set.verify(&first.secret, past_window),
            KeyVerdict::Expired { .. }
        ));
    }

    #[test]
    fn rotation_confirmation_can_name_the_expiry_date() {
        let mut set = KeySet::new(KeyKind::Browser);
        set.generate(at("2026-07-02T09:15:00Z"));
        set.rotate(at("2026-08-26T07:41:00Z"));
        let expiry = set.overlap_expires_at().expect("rotated set has an expiry");
        assert_eq!(now_rfc3339(expiry), "2026-09-25T07:41:00Z");
    }

    #[test]
    fn rotating_an_unissued_set_just_mints_the_first_key() {
        let mut set = KeySet::new(KeyKind::Build);
        let minted = set.rotate(at("2026-07-02T09:15:00Z"));
        assert!(set.previous.is_none());
        assert!(set.rotated_at.is_none());
        assert_eq!(
            set.verify(&minted.secret, at("2026-07-02T09:15:00Z")),
            KeyVerdict::Current
        );
    }

    #[test]
    fn revoke_has_no_overlap() {
        let mut set = KeySet::new(KeyKind::Browser);
        let first = set.generate(at("2026-07-02T09:15:00Z"));
        let second = set.rotate(at("2026-08-26T07:41:00Z"));
        let revoked_at = at("2026-08-26T08:00:00Z");
        set.revoke(revoked_at);

        assert!(matches!(
            set.verify(&second.secret, revoked_at),
            KeyVerdict::Revoked { .. }
        ));
        assert!(matches!(
            set.verify(&first.secret, revoked_at),
            KeyVerdict::Revoked { .. }
        ));
        assert!(!set.is_issued());
    }

    #[test]
    fn a_key_of_the_wrong_kind_is_never_recognised() {
        let mut browser = KeySet::new(KeyKind::Browser);
        let mut build = KeySet::new(KeyKind::Build);
        let b = build.generate(at("2026-07-02T09:15:00Z"));
        browser.generate(at("2026-07-02T09:15:00Z"));
        assert_eq!(
            browser.verify(&b.secret, at("2026-07-02T09:15:00Z")),
            KeyVerdict::Unknown
        );
    }

    #[test]
    fn an_unissued_set_reports_no_key_rather_than_unknown() {
        let set = KeySet::new(KeyKind::Browser);
        assert_eq!(
            set.verify("eus_pk_deadbeef", at("2026-07-02T09:15:00Z")),
            KeyVerdict::NoKey
        );
        assert_eq!(set.masked(), "not issued");
    }

    #[test]
    fn flat_properties_round_trip() {
        let mut set = KeySet::new(KeyKind::Browser);
        set.generate(at("2026-07-02T09:15:00Z"));
        let minted = set.rotate(at("2026-08-26T07:41:00Z"));
        set.overlap_days = 14;
        set.rate_limit_per_minute = 120;
        set.burst = 240;

        let flat: BTreeMap<String, toml::Value> = set.to_flat_properties().into_iter().collect();
        let back = KeySet::from_flat_properties(KeyKind::Browser, &flat);

        assert_eq!(back.current, set.current);
        assert_eq!(back.previous, set.previous);
        assert_eq!(back.rotated_at, set.rotated_at);
        assert_eq!(back.overlap_days, 14);
        assert_eq!(back.rate_limit_per_minute, 120);
        assert_eq!(back.burst, 240);
        assert_eq!(
            back.verify(&minted.secret, at("2026-08-26T07:41:00Z")),
            KeyVerdict::Current
        );
    }

    #[test]
    fn every_property_key_is_emitted_so_the_panel_can_persist_it() {
        for kind in [KeyKind::Browser, KeyKind::Build] {
            let set = KeySet::new(kind);
            let emitted: Vec<String> = set
                .to_flat_properties()
                .into_iter()
                .map(|(k, _)| k)
                .collect();
            for expected in KeySet::property_keys(kind) {
                assert!(
                    emitted.contains(&expected),
                    "{expected} missing from to_flat_properties"
                );
            }
            assert_eq!(emitted.len(), KeySet::property_keys(kind).len());
        }
    }

    #[test]
    fn a_normalised_service_file_reads_the_same_as_a_hand_written_one() {
        let mut set = KeySet::new(KeyKind::Browser);
        let minted = set.generate(at("2026-07-02T09:15:00Z"));

        // What the loader's key normaliser would leave behind.
        let snake: BTreeMap<String, toml::Value> = set
            .to_flat_properties()
            .into_iter()
            .map(|(k, v)| (squash(&k), v))
            .collect();
        let back = KeySet::from_flat_properties(KeyKind::Browser, &snake);
        assert_eq!(
            back.verify(&minted.secret, at("2026-07-02T09:15:00Z")),
            KeyVerdict::Current
        );
    }

    #[test]
    fn a_blank_stored_hash_is_not_a_record() {
        let mut flat = BTreeMap::new();
        flat.insert(
            "WebsiteKeyId".to_string(),
            toml::Value::String("wk_000000".to_string()),
        );
        flat.insert(
            "WebsiteKeyHash".to_string(),
            toml::Value::String("   ".to_string()),
        );
        let set = KeySet::from_flat_properties(KeyKind::Browser, &flat);
        assert!(set.current.is_none());
        assert_eq!(
            set.verify("eus_pk_whatever", at("2026-07-02T09:15:00Z")),
            KeyVerdict::NoKey
        );
    }
}
