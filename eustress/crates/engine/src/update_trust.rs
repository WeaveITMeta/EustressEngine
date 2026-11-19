//! # Update trust
//!
//! Decides whether a release manifest may drive an update. Pure functions with
//! no I/O and no Bevy, so every rule here is unit tested without a network.
//!
//! A release publishes two files beside its artifacts:
//! `https://downloads.eustress.dev/v<version>/latest.json` (the manifest) and
//! `.../latest.json.sig` (Ed25519 signatures over the exact manifest bytes).
//! The unsigned pointer `https://downloads.eustress.dev/latest.json` only names
//! the newest version; the signed manifest is what says which file to install
//! and which SHA-256 and size it must have.
//!
//! Signature file format (text, at most [`MAX_SIGNATURE_FILE_BYTES`]): one
//! signature per line, `<16 hex key id> <128 hex Ed25519 signature>`. Blank
//! lines and lines starting with `#` are ignored; any other line is an error.
//! A key id is the lowercase hex of the first 8 bytes of SHA-256 over the 32
//! public key bytes ([`key_id`]), which is what the release workflow prints.

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// The one host and scheme an update artifact may come from.
pub const RELEASE_ORIGIN: &str = "https://downloads.eustress.dev/";
/// The largest signed manifest accepted.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
/// The largest signature file accepted.
pub const MAX_SIGNATURE_FILE_BYTES: usize = 4 * 1024;
/// The largest artifact a manifest may name.
pub const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The product a Studio update manifest must name.
pub const STUDIO_PRODUCT: &str = "studio";

/// A public key this build trusts to sign release manifests.
pub struct TrustedKey {
    /// The key id, as [`key_id`] computes it from `key`. Matching uses the id
    /// computed from `key`; this text is checked against it by a unit test.
    pub id: &'static str,
    /// The 32-byte Ed25519 public key.
    pub key: [u8; 32],
}

/// The keys whose signature makes a manifest trusted.
///
/// The list is empty until the release signing key exists. An empty list means
/// automatic install is off, and an update the user clicks keeps the hash-only
/// check against the unsigned pointer. Adding a key turns signature checking on
/// for every update, clicked or automatic.
pub const TRUSTED_KEYS: &[TrustedKey] = &[];

/// Why a manifest or its signature was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustError {
    /// This build trusts no key, so nothing can be verified.
    NoTrustedKeys,
    /// The signature file is too large, not text, or has a line that is not
    /// `<16 hex> <128 hex>`.
    MalformedSignatureFile(String),
    /// No line in the signature file names a key this build trusts.
    NoSignatureForTrustedKey,
    /// A line names a trusted key, but no such line verifies over the manifest.
    BadSignature,
    /// The manifest is larger than [`MAX_MANIFEST_BYTES`].
    ManifestTooLarge(usize),
    /// The manifest is not the JSON shape a release writes.
    MalformedManifest(String),
    /// The manifest is for another product.
    WrongProduct(String),
    /// The manifest's version is not the one the pointer named.
    VersionMismatch { manifest: String, expected: String },
    /// The manifest's version is not newer than the running build.
    NotNewer { manifest: String, running: String },
    /// The manifest has no entry for this platform.
    NoPlatform(String),
    /// The platform entry's URL is not a file in this version's release folder.
    BadUrl(String),
}

impl fmt::Display for TrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrustError::NoTrustedKeys => write!(f, "this build trusts no update signing key"),
            TrustError::MalformedSignatureFile(why) => write!(f, "the signature file is malformed: {why}"),
            TrustError::NoSignatureForTrustedKey => write!(f, "the manifest is not signed by a trusted key"),
            TrustError::BadSignature => write!(f, "the manifest signature does not verify"),
            TrustError::ManifestTooLarge(n) => {
                write!(f, "the manifest is {n} bytes, over the {MAX_MANIFEST_BYTES} byte limit")
            }
            TrustError::MalformedManifest(why) => write!(f, "the manifest is malformed: {why}"),
            TrustError::WrongProduct(p) => write!(f, "the manifest is for product {p:?}, not {STUDIO_PRODUCT:?}"),
            TrustError::VersionMismatch { manifest, expected } => {
                write!(f, "the signed manifest is for v{manifest}, the pointer named v{expected}")
            }
            TrustError::NotNewer { manifest, running } => {
                write!(f, "the signed manifest's v{manifest} is not newer than the running v{running}")
            }
            TrustError::NoPlatform(key) => write!(f, "the manifest has no entry for {key}"),
            TrustError::BadUrl(url) => write!(f, "the artifact URL {url:?} is not a file in this release's folder"),
        }
    }
}

impl std::error::Error for TrustError {}

/// The id of a public key: lowercase hex of the first 8 bytes of SHA-256 over
/// the 32 key bytes.
pub fn key_id(public_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_key);
    hex::encode(&digest[..8])
}

/// Whether `s` is exactly `len` ASCII hex digits.
fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Check that `manifest` carries a valid signature from a key in `keys`.
///
/// Succeeds when at least one line of `signature_file` names a trusted key id
/// and that key verifies the signature over the exact manifest bytes with
/// `verify_strict`, which also rejects small-order keys and non-canonical
/// signatures. Every line must be well formed, even lines for keys this build
/// does not know, so a damaged file is refused as a whole.
pub fn verify_manifest(manifest: &[u8], signature_file: &str, keys: &[TrustedKey]) -> Result<(), TrustError> {
    if keys.is_empty() {
        return Err(TrustError::NoTrustedKeys);
    }
    if signature_file.len() > MAX_SIGNATURE_FILE_BYTES {
        return Err(TrustError::MalformedSignatureFile(format!(
            "{} bytes, over the {MAX_SIGNATURE_FILE_BYTES} byte limit",
            signature_file.len()
        )));
    }

    let mut entries: Vec<(String, [u8; 64])> = Vec::new();
    for (index, raw) in signature_file.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_ascii_whitespace();
        let (Some(id), Some(sig_hex), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(TrustError::MalformedSignatureFile(format!("line {} is not `<key id> <signature>`", index + 1)));
        };
        if !is_hex(id, 16) || !is_hex(sig_hex, 128) {
            return Err(TrustError::MalformedSignatureFile(format!(
                "line {} needs a 16 hex digit key id and a 128 hex digit signature",
                index + 1
            )));
        }
        let mut sig = [0u8; 64];
        hex::decode_to_slice(sig_hex, &mut sig)
            .map_err(|e| TrustError::MalformedSignatureFile(format!("line {}: {e}", index + 1)))?;
        entries.push((id.to_ascii_lowercase(), sig));
    }

    let mut named_trusted_key = false;
    for key in keys {
        let id = key_id(&key.key);
        for (line_id, sig) in &entries {
            if *line_id != id {
                continue;
            }
            named_trusted_key = true;
            let Ok(verifying) = VerifyingKey::from_bytes(&key.key) else {
                continue;
            };
            let signature = Signature::from_bytes(sig);
            if verifying.verify_strict(manifest, &signature).is_ok() {
                return Ok(());
            }
        }
    }
    if named_trusted_key {
        Err(TrustError::BadSignature)
    } else {
        Err(TrustError::NoSignatureForTrustedKey)
    }
}

/// Parse `X.Y.Z`: exactly three dot-separated runs of ASCII digits, each a u32.
/// No `v` prefix, no pre-release or build suffix.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split('.');
    let mut next = || -> Option<u32> {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let version = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return None;
    }
    Some(version)
}

/// One platform's artifact in a signed manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformArtifact {
    pub url: String,
    /// Lowercase hex SHA-256 of the artifact.
    pub sha256: String,
    pub size_bytes: u64,
}

/// The fields of a signed manifest the updater relies on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedManifest {
    pub product: String,
    /// The version exactly as the manifest writes it.
    pub version_text: String,
    pub version: (u32, u32, u32),
    pub platforms: BTreeMap<String, PlatformArtifact>,
}

#[derive(serde::Deserialize)]
struct RawManifest {
    product: String,
    version: String,
    platforms: BTreeMap<String, RawPlatform>,
}

#[derive(serde::Deserialize)]
struct RawPlatform {
    url: String,
    sha256: String,
    size_bytes: u64,
}

/// Parse a signed manifest. Strict JSON: `product` (string), `version`
/// (`X.Y.Z`) and `platforms` (a map of key to `{ url, sha256, size_bytes }`)
/// are required, with `sha256` 64 hex digits and `size_bytes` from 1 byte to
/// 2 GiB. Other fields are ignored.
pub fn parse_signed_manifest(bytes: &[u8]) -> Result<SignedManifest, TrustError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(TrustError::ManifestTooLarge(bytes.len()));
    }
    let raw: RawManifest =
        serde_json::from_slice(bytes).map_err(|e| TrustError::MalformedManifest(e.to_string()))?;
    let version = parse_version(&raw.version)
        .ok_or_else(|| TrustError::MalformedManifest(format!("version {:?} is not X.Y.Z", raw.version)))?;
    let mut platforms = BTreeMap::new();
    for (key, platform) in raw.platforms {
        if !is_hex(&platform.sha256, 64) {
            return Err(TrustError::MalformedManifest(format!("{key}: sha256 is not 64 hex digits")));
        }
        if platform.size_bytes == 0 || platform.size_bytes > MAX_ARTIFACT_BYTES {
            return Err(TrustError::MalformedManifest(format!(
                "{key}: size_bytes {} is outside 1 to {MAX_ARTIFACT_BYTES}",
                platform.size_bytes
            )));
        }
        platforms.insert(
            key,
            PlatformArtifact {
                url: platform.url,
                sha256: platform.sha256.to_ascii_lowercase(),
                size_bytes: platform.size_bytes,
            },
        );
    }
    Ok(SignedManifest { product: raw.product, version_text: raw.version, version, platforms })
}

/// The update a verified manifest allows on this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateTarget {
    pub version_text: String,
    pub version: (u32, u32, u32),
    pub url: String,
    /// The artifact's file name, the last segment of `url`.
    pub file_name: String,
    /// Lowercase hex SHA-256 the download must match.
    pub sha256: String,
    /// The exact size the download must have.
    pub size_bytes: u64,
}

/// Whether `name` is a plain release file name: `[A-Za-z0-9._-]+`, and not
/// made of dots only.
fn is_release_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !name.bytes().all(|b| b == b'.')
}

/// Choose the update a verified manifest allows.
///
/// Requires the product to be Studio, the version to equal `expected_version`
/// (what the unsigned pointer named, so an old signed manifest cannot be
/// replayed under a newer pointer), the version to be newer than `running`
/// (no downgrade), an entry for `platform_key`, and that entry's URL to be
/// exactly `https://downloads.eustress.dev/v<version>/<file name>`: no query,
/// fragment, port, user info or other folder.
pub fn select_update(
    manifest: &SignedManifest,
    expected_version: (u32, u32, u32),
    running: (u32, u32, u32),
    platform_key: &str,
) -> Result<UpdateTarget, TrustError> {
    let text = |v: (u32, u32, u32)| format!("{}.{}.{}", v.0, v.1, v.2);
    if manifest.product != STUDIO_PRODUCT {
        return Err(TrustError::WrongProduct(manifest.product.clone()));
    }
    if manifest.version != expected_version {
        return Err(TrustError::VersionMismatch {
            manifest: manifest.version_text.clone(),
            expected: text(expected_version),
        });
    }
    if manifest.version <= running {
        return Err(TrustError::NotNewer { manifest: manifest.version_text.clone(), running: text(running) });
    }
    let artifact = manifest
        .platforms
        .get(platform_key)
        .ok_or_else(|| TrustError::NoPlatform(platform_key.to_string()))?;
    let folder = format!("{RELEASE_ORIGIN}v{}/", manifest.version_text);
    let file_name = artifact
        .url
        .strip_prefix(&folder)
        .filter(|name| is_release_file_name(name))
        .ok_or_else(|| TrustError::BadUrl(artifact.url.clone()))?;
    Ok(UpdateTarget {
        version_text: manifest.version_text.clone(),
        version: manifest.version,
        url: artifact.url.clone(),
        file_name: file_name.to_string(),
        sha256: artifact.sha256.clone(),
        size_bytes: artifact.size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway key made with OpenSSL; its private half was destroyed.
    const TEST_PUBLIC_KEY_HEX: &str = "26a5e40394ffbf350afd6cfc5f01bec915ed89e9276321cf8992724e1441f872";
    const TEST_KEY_ID: &str = "e9da590d9b1a4eea";
    const TEST_SIGNATURE_HEX: &str = "d8fd6b53f5e594fb746c64b8e2ed06bc60b6bf9cdedf9cade676a977e36de9985f806d10ed4d868ea7d98b97c5a440526261c8d589c749891e2f9a09fd315201";
    /// Exactly the 311 signed bytes, no trailing newline.
    const TEST_MANIFEST: &str = r#"{"product":"studio","version":"9.9.9","channel":"stable","platforms":{"windows-x64":{"file":"eustress-engine-v9.9.9-windows-x64.zip","url":"https://downloads.eustress.dev/v9.9.9/eustress-engine-v9.9.9-windows-x64.zip","sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","size_bytes":1}}}"#;
    const TEST_MANIFEST_SHA256: &str = "df0e4256eeeec9f6899ccba82ae3229f18ebfcd44ffc50a9ecc84223cb477bf0";

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        hex::decode_to_slice(TEST_PUBLIC_KEY_HEX, &mut key).unwrap();
        key
    }

    fn test_keys() -> Vec<TrustedKey> {
        vec![TrustedKey { id: TEST_KEY_ID, key: test_key() }]
    }

    fn test_signature_file() -> String {
        format!("{TEST_KEY_ID} {TEST_SIGNATURE_HEX}\n")
    }

    fn manifest_with(product: &str, version: &str, url: &str) -> SignedManifest {
        let json = format!(
            r#"{{"product":"{product}","version":"{version}","platforms":{{"windows-x64":{{"url":"{url}","sha256":"{}","size_bytes":10}}}}}}"#,
            "a".repeat(64)
        );
        parse_signed_manifest(json.as_bytes()).unwrap()
    }

    #[test]
    fn manifest_constant_is_the_signed_bytes() {
        assert_eq!(TEST_MANIFEST.len(), 311);
        assert_eq!(hex::encode(Sha256::digest(TEST_MANIFEST.as_bytes())), TEST_MANIFEST_SHA256);
    }

    #[test]
    fn key_id_of_the_test_key() {
        assert_eq!(key_id(&test_key()), TEST_KEY_ID);
    }

    #[test]
    fn trusted_key_ids_match_their_keys() {
        for key in TRUSTED_KEYS {
            assert_eq!(key.id, key_id(&key.key), "TRUSTED_KEYS entry {} has the wrong id", key.id);
        }
    }

    #[test]
    fn the_vector_verifies() {
        assert_eq!(verify_manifest(TEST_MANIFEST.as_bytes(), &test_signature_file(), &test_keys()), Ok(()));
    }

    #[test]
    fn comments_blank_lines_and_crlf_are_accepted() {
        let file = format!("# release signature\r\n\r\n{TEST_KEY_ID} {TEST_SIGNATURE_HEX}\r\n");
        assert_eq!(verify_manifest(TEST_MANIFEST.as_bytes(), &file, &test_keys()), Ok(()));
    }

    #[test]
    fn one_changed_manifest_byte_fails() {
        let mut manifest = TEST_MANIFEST.as_bytes().to_vec();
        let i = manifest.len() / 2;
        manifest[i] ^= 0x01;
        assert_eq!(
            verify_manifest(&manifest, &test_signature_file(), &test_keys()),
            Err(TrustError::BadSignature)
        );
    }

    #[test]
    fn one_changed_signature_byte_fails() {
        let mut sig = hex::decode(TEST_SIGNATURE_HEX).unwrap();
        sig[10] ^= 0x01;
        let file = format!("{TEST_KEY_ID} {}\n", hex::encode(&sig));
        assert_eq!(verify_manifest(TEST_MANIFEST.as_bytes(), &file, &test_keys()), Err(TrustError::BadSignature));
    }

    #[test]
    fn correct_signature_under_an_untrusted_key_id_fails() {
        let file = format!("0123456789abcdef {TEST_SIGNATURE_HEX}\n");
        assert_eq!(
            verify_manifest(TEST_MANIFEST.as_bytes(), &file, &test_keys()),
            Err(TrustError::NoSignatureForTrustedKey)
        );
    }

    #[test]
    fn empty_key_list_fails() {
        assert_eq!(
            verify_manifest(TEST_MANIFEST.as_bytes(), &test_signature_file(), &[]),
            Err(TrustError::NoTrustedKeys)
        );
    }

    #[test]
    fn malformed_signature_files_fail() {
        let keys = test_keys();
        let m = TEST_MANIFEST.as_bytes();
        for file in [
            format!("{TEST_KEY_ID}\n"),
            format!("{TEST_KEY_ID} {TEST_SIGNATURE_HEX} extra\n"),
            format!("{TEST_KEY_ID}0 {TEST_SIGNATURE_HEX}\n"),
            format!("{TEST_KEY_ID} {}\n", &TEST_SIGNATURE_HEX[..126]),
            format!("zzzzzzzzzzzzzzzz {TEST_SIGNATURE_HEX}\n"),
            format!("{TEST_KEY_ID} {TEST_SIGNATURE_HEX}\nnot a signature\n"),
            "#".repeat(MAX_SIGNATURE_FILE_BYTES + 1),
        ] {
            assert!(
                matches!(verify_manifest(m, &file, &keys), Err(TrustError::MalformedSignatureFile(_))),
                "accepted {file:?}"
            );
        }
    }

    #[test]
    fn the_vector_parses_and_selects() {
        let manifest = parse_signed_manifest(TEST_MANIFEST.as_bytes()).unwrap();
        assert_eq!(manifest.version, (9, 9, 9));
        let target = select_update(&manifest, (9, 9, 9), (0, 3, 8), "windows-x64").unwrap();
        assert_eq!(target.file_name, "eustress-engine-v9.9.9-windows-x64.zip");
        assert_eq!(target.size_bytes, 1);
        assert_eq!(target.sha256, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn malformed_manifests_are_refused() {
        let ok_platform = format!(r#""p":{{"url":"u","sha256":"{}","size_bytes":5}}"#, "a".repeat(64));
        for json in [
            String::from("not json"),
            format!(r#"{{"version":"1.2.3","platforms":{{{ok_platform}}}}}"#),
            format!(r#"{{"product":"studio","version":"1.2","platforms":{{{ok_platform}}}}}"#),
            format!(r#"{{"product":"studio","version":"v1.2.3","platforms":{{{ok_platform}}}}}"#),
            format!(r#"{{"product":"studio","version":"1.2.3-beta","platforms":{{{ok_platform}}}}}"#),
            String::from(r#"{"product":"studio","version":"1.2.3"}"#),
            format!(r#"{{"product":"studio","version":"1.2.3","platforms":{{"p":{{"url":"u","sha256":"abc","size_bytes":5}}}}}}"#),
            format!(
                r#"{{"product":"studio","version":"1.2.3","platforms":{{"p":{{"url":"u","sha256":"{}","size_bytes":0}}}}}}"#,
                "a".repeat(64)
            ),
            format!(
                r#"{{"product":"studio","version":"1.2.3","platforms":{{"p":{{"url":"u","sha256":"{}","size_bytes":{}}}}}}}"#,
                "a".repeat(64),
                MAX_ARTIFACT_BYTES + 1
            ),
            format!(r#"{{"product":"studio","version":"1.2.3","platforms":{{{ok_platform}}}}} trailing"#),
        ] {
            assert!(parse_signed_manifest(json.as_bytes()).is_err(), "accepted {json}");
        }
        let big = vec![b' '; MAX_MANIFEST_BYTES + 1];
        assert_eq!(parse_signed_manifest(&big), Err(TrustError::ManifestTooLarge(MAX_MANIFEST_BYTES + 1)));
    }

    #[test]
    fn product_other_than_studio_is_refused() {
        let m = manifest_with("player", "1.2.3", "https://downloads.eustress.dev/v1.2.3/a.zip");
        assert_eq!(select_update(&m, (1, 2, 3), (1, 0, 0), "windows-x64"), Err(TrustError::WrongProduct("player".into())));
    }

    #[test]
    fn version_not_above_running_is_refused() {
        let m = manifest_with("studio", "1.2.3", "https://downloads.eustress.dev/v1.2.3/a.zip");
        assert!(matches!(select_update(&m, (1, 2, 3), (1, 2, 3), "windows-x64"), Err(TrustError::NotNewer { .. })));
        assert!(matches!(select_update(&m, (1, 2, 3), (1, 3, 0), "windows-x64"), Err(TrustError::NotNewer { .. })));
        assert!(select_update(&m, (1, 2, 3), (1, 2, 2), "windows-x64").is_ok());
    }

    #[test]
    fn version_other_than_the_pointer_is_refused() {
        let m = manifest_with("studio", "1.2.3", "https://downloads.eustress.dev/v1.2.3/a.zip");
        assert!(matches!(
            select_update(&m, (1, 2, 4), (1, 0, 0), "windows-x64"),
            Err(TrustError::VersionMismatch { .. })
        ));
    }

    #[test]
    fn missing_platform_is_refused() {
        let m = manifest_with("studio", "1.2.3", "https://downloads.eustress.dev/v1.2.3/a.zip");
        assert_eq!(select_update(&m, (1, 2, 3), (1, 0, 0), "linux-x64"), Err(TrustError::NoPlatform("linux-x64".into())));
    }

    #[test]
    fn urls_outside_the_release_folder_are_refused() {
        for url in [
            "https://evil.example/v1.2.3/a.zip",
            "https://downloads.eustress.dev.evil.example/v1.2.3/a.zip",
            "http://downloads.eustress.dev/v1.2.3/a.zip",
            "https://downloads.eustress.dev:8443/v1.2.3/a.zip",
            "https://user@downloads.eustress.dev/v1.2.3/a.zip",
            "https://downloads.eustress.dev/v1.2.3/a.zip?x=1",
            "https://downloads.eustress.dev/v1.2.3/a.zip#x",
            "https://downloads.eustress.dev/v1.2.2/a.zip",
            "https://downloads.eustress.dev/v1.2.3/sub/a.zip",
            "https://downloads.eustress.dev/v1.2.3/",
            "https://downloads.eustress.dev/v1.2.3/..",
            "https://downloads.eustress.dev/a.zip",
        ] {
            let m = manifest_with("studio", "1.2.3", url);
            assert!(
                matches!(select_update(&m, (1, 2, 3), (1, 0, 0), "windows-x64"), Err(TrustError::BadUrl(_))),
                "accepted {url}"
            );
        }
    }

    #[test]
    fn parse_version_is_strict() {
        assert_eq!(parse_version("0.3.8"), Some((0, 3, 8)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
        for bad in ["", "1", "1.2", "1.2.3.4", "v1.2.3", "1.2.x", "1..3", "1.2.3-rc1", " 1.2.3", "+1.2.3", "99999999999.0.0"] {
            assert_eq!(parse_version(bad), None, "accepted {bad:?}");
        }
    }
}
