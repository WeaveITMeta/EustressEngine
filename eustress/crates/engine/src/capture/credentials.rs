//! Where an API key lives: outside every Space, and out of the UI's way.
//!
//! The constraint that shapes this: a Space folder is the thing we ask auditors
//! to read, and it gets synced, committed, and shared. A key typed into
//! Properties would land in `_instance.toml` and from there into a git history.
//! So the key never goes in a Space. The Connector stores only the NAME of the
//! credential (`secret_ref`), and the value lives here.
//!
//! `%LOCALAPPDATA%\Eustress\credentials.toml` on Windows, the platform
//! data-local directory elsewhere. Read at fetch time, so saving a key takes
//! effect on the next sync with no restart.
//!
//! ## What this is and is not
//!
//! It is a plaintext file in the user's own profile directory, the same posture
//! as `~/.aws/credentials` or `gh`'s `hosts.yml`. It is NOT encrypted, and it
//! does not pretend to be: the thing it holds is a read-only key for public
//! federal data whose worst-case loss is a burned daily quota. What it does
//! guarantee is the property that actually matters here, which is that the key
//! is not in the folder you are about to hand an auditor.
//!
//! Every function takes an explicit path in its `_at` form so the tests never
//! touch the real profile and never race each other through an env var.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::CaptureError;

/// File name inside the Eustress data directory.
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Default credential name for SAM.gov, when a Connector names none.
pub const DEFAULT_SAM_KEY: &str = "SAM_API_KEY";

/// The whole store: credential name to value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

/// Whether a credential is set, and enough of it to recognise which one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStatus {
    pub name: String,
    pub present: bool,
    /// `••••••••3f9a`, or empty when absent. Never the whole value.
    pub masked: String,
    /// True when the value came from the environment rather than the file, so
    /// the UI can explain why a Remove button will not clear it.
    pub from_environment: bool,
}

impl KeyStatus {
    /// One line for the dialog and the ribbon report.
    pub fn summary(&self) -> String {
        match (self.present, self.from_environment) {
            (false, _) => format!("{} is not set", self.name),
            (true, true) => {
                format!("{} is set from the environment ({})", self.name, self.masked)
            }
            (true, false) => format!("{} is saved ({})", self.name, self.masked),
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

/// Read one credential.
///
/// The saved file wins over the environment. A user who just pasted a key into
/// the dialog expects that key to be used, and silently preferring a stale
/// environment variable they set months ago would be the worst kind of
/// surprise: it looks saved, and the wrong one goes out.
pub fn get(name: &str) -> Option<String> {
    let from_file = credentials_path().and_then(|p| get_at(&p, name));
    from_file.or_else(|| {
        std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    })
}

/// Read one credential from a specific file, ignoring the environment.
pub fn get_at(path: &Path, name: &str) -> Option<String> {
    load_at(path)
        .keys
        .get(name)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Save one credential, creating the file and its directory as needed.
pub fn set(name: &str, value: &str) -> Result<(), CaptureError> {
    let path = credentials_path()
        .ok_or_else(|| CaptureError::Io("no local data directory on this platform".into()))?;
    set_at(&path, name, value)
}

/// Save one credential to a specific file.
pub fn set_at(path: &Path, name: &str, value: &str) -> Result<(), CaptureError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CaptureError::BadQuery("an API key cannot be empty".into()));
    }
    if name.trim().is_empty() {
        return Err(CaptureError::BadQuery("a credential needs a name".into()));
    }
    let mut store = load_at(path);
    store.keys.insert(name.trim().to_string(), value.to_string());
    save_at(path, &store)
}

/// Forget one credential. Returns whether there was one to forget.
pub fn remove(name: &str) -> Result<bool, CaptureError> {
    let path = credentials_path()
        .ok_or_else(|| CaptureError::Io("no local data directory on this platform".into()))?;
    remove_at(&path, name)
}

/// Forget one credential in a specific file.
pub fn remove_at(path: &Path, name: &str) -> Result<bool, CaptureError> {
    let mut store = load_at(path);
    let had = store.keys.remove(name).is_some();
    if had {
        save_at(path, &store)?;
    }
    Ok(had)
}

/// Report a credential's state without revealing it.
pub fn status(name: &str) -> KeyStatus {
    let path = credentials_path();
    let from_file = path.as_deref().and_then(|p| get_at(p, name));
    if let Some(v) = from_file {
        return KeyStatus {
            name: name.to_string(),
            present: true,
            masked: mask(&v),
            from_environment: false,
        };
    }
    match std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        Some(v) => KeyStatus {
            name: name.to_string(),
            present: true,
            masked: mask(v.trim()),
            from_environment: true,
        },
        None => KeyStatus {
            name: name.to_string(),
            present: false,
            masked: String::new(),
            from_environment: false,
        },
    }
}

/// Everything but the last four characters, replaced.
///
/// A short value is masked entirely rather than mostly shown: revealing three
/// of four characters of a four-character secret is not masking.
pub fn mask(value: &str) -> String {
    let n = value.chars().count();
    if n <= 8 {
        return "•".repeat(n.max(1));
    }
    let tail: String = value.chars().skip(n - 4).collect();
    format!("{}{tail}", "•".repeat(8))
}

fn load_at(path: &Path) -> Store {
    // A missing or unreadable file is an empty store, not an error: the first
    // save creates it, and a corrupted one must not block the user from
    // replacing the key that would fix it.
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Store::default(),
    }
}

fn save_at(path: &Path, store: &Store) -> Result<(), CaptureError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CaptureError::Io(format!("{}: {e}", parent.display())))?;
    }
    let text = toml::to_string_pretty(store)
        .map_err(|e| CaptureError::Io(format!("serializing credentials: {e}")))?;
    std::fs::write(path, text).map_err(|e| CaptureError::Io(format!("{}: {e}", path.display())))?;
    restrict_permissions(path);
    Ok(())
}

/// Owner-only permissions where the platform expresses them that way.
///
/// On Windows the file already sits under the user's own `LOCALAPPDATA`, which
/// is user-scoped by default; tightening the ACL further would need a
/// platform-specific dependency for no meaningful gain against this threat
/// model. Best effort, never fatal: failing to chmod must not lose the key the
/// user just pasted.
#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir()
            .join(format!("eustress-creds-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join(CREDENTIALS_FILE)
    }

    #[test]
    fn a_saved_key_reads_back() {
        let p = temp("roundtrip");
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY), None);
        set_at(&p, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY).as_deref(), Some("abc123def456"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn saving_a_second_credential_keeps_the_first() {
        let p = temp("two-keys");
        set_at(&p, "SAM_API_KEY", "first-key-value").unwrap();
        set_at(&p, "SAM_API_KEY_CLIENT_B", "second-key-value").unwrap();
        assert_eq!(get_at(&p, "SAM_API_KEY").as_deref(), Some("first-key-value"));
        assert_eq!(get_at(&p, "SAM_API_KEY_CLIENT_B").as_deref(), Some("second-key-value"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_key_is_replaced_not_appended() {
        let p = temp("replace");
        set_at(&p, DEFAULT_SAM_KEY, "old-key-value").unwrap();
        set_at(&p, DEFAULT_SAM_KEY, "new-key-value").unwrap();
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY).as_deref(), Some("new-key-value"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn removing_reports_whether_there_was_anything_to_remove() {
        let p = temp("remove");
        assert!(!remove_at(&p, DEFAULT_SAM_KEY).unwrap(), "nothing saved yet");
        set_at(&p, DEFAULT_SAM_KEY, "abc123def456").unwrap();
        assert!(remove_at(&p, DEFAULT_SAM_KEY).unwrap());
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY), None);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn an_empty_key_is_refused_rather_than_saved_as_blank() {
        let p = temp("empty");
        assert!(matches!(set_at(&p, DEFAULT_SAM_KEY, "   "), Err(CaptureError::BadQuery(_))));
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY), None);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn whitespace_around_a_pasted_key_is_trimmed() {
        // Copying from a web page routinely picks up a trailing newline, and a
        // key with one appended fails as a 403 that looks like a wrong key.
        let p = temp("trim");
        set_at(&p, DEFAULT_SAM_KEY, "  abc123def456\n").unwrap();
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY).as_deref(), Some("abc123def456"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_corrupted_file_does_not_block_replacing_the_key() {
        let p = temp("corrupt");
        std::fs::write(&p, "this is not toml {{{").unwrap();
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY), None, "unreadable reads as absent");
        set_at(&p, DEFAULT_SAM_KEY, "abc123def456").expect("saving must still work");
        assert_eq!(get_at(&p, DEFAULT_SAM_KEY).as_deref(), Some("abc123def456"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn masking_never_reveals_a_short_secret() {
        assert_eq!(mask("abcd"), "••••");
        assert_eq!(mask("12345678"), "••••••••");
        assert_eq!(mask("abcdefghij3f9a"), "••••••••3f9a");
        assert!(!mask("abcdefghij3f9a").contains("abcdef"), "the head must never show");
        assert_eq!(mask(""), "•", "an empty value still masks to something");
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
