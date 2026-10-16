//! The bridge's per-launch token, `~/.eustress_engine/bridge/<port>.token`.
//!
//! Every request must carry it in `auth` (`server::handle_connection`), so
//! reaching the port is not enough to drive the engine: a web page cannot
//! read the file, and neither can another account. It lives in the user's
//! own profile, outside every Universe and the workspace, where no
//! Universe-confined tool (the file tools, `sandbox::resolve`) reaches, and
//! its folder and file are private ([`crate::private_file`]): their access
//! lists name only the user and SYSTEM, set explicitly and checked, never
//! inherited, because a profile folder can grant other accounts (an AI
//! agent's sandbox group, on one machine) read access. A process running as
//! the same user can read it, as it can read everything else that user
//! owns; the token does not claim more than that.
//!
//! There is no way to run the bridge without it: no environment variable, no
//! setting, no legacy mode. If the file cannot be written privately, no
//! client can authenticate and the log says why.
//!
//! Clients find it by the port they connected to
//! (`eustress_bridge_client::read_token`), so every way of finding the
//! port (the Universe's `engine.port`, the workspace's, the instance
//! registry) finds the token too.

use std::path::{Path, PathBuf};

/// A fresh token: 32 bytes from the operating system's generator, in hex.
pub(crate) fn generate() -> String {
    use rand::RngCore as _;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Owns `<dir>/<port>.token` and removes it on drop while it still holds
/// this launch's token.
pub(crate) struct TokenFile {
    path: PathBuf,
    token: String,
}

impl TokenFile {
    /// Write `token` for `port` under the user's bridge folder.
    pub(crate) fn write(port: u16, token: &str) -> std::io::Result<Self> {
        let dir = eustress_bridge_client::token_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no home folder to keep the bridge token in")
        })?;
        Self::write_in(&dir, port, token)
    }

    /// [`TokenFile::write`] into `dir`, privately
    /// ([`crate::private_file::write_private`]: the folder and the file are
    /// made private and checked before the token is written, and it is
    /// renamed into place, so a client never reads half a token).
    pub(crate) fn write_in(dir: &Path, port: u16, token: &str) -> std::io::Result<Self> {
        let path = eustress_bridge_client::token_path_in(dir, port);
        crate::private_file::write_private(&path, token.as_bytes())?;
        Ok(Self { path, token: token.to_string() })
    }

    pub(crate) fn display_path(&self) -> String {
        self.path.display().to_string()
    }
}

impl Drop for TokenFile {
    fn drop(&mut self) {
        // Another engine may have bound this port since and written its own.
        if std::fs::read_to_string(&self.path).is_ok_and(|t| t.trim() == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// The token a request carries matches this launch's, compared in time that
/// does not depend on where they differ.
pub(crate) fn matches(sent: Option<&str>, token: &str) -> bool {
    let Some(sent) = sent else { return false };
    let (a, b) = (sent.as_bytes(), token.as_bytes());
    if a.len() != b.len() || b.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_fresh() {
        let (a, b) = (generate(), generate());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn only_the_exact_token_matches() {
        let t = generate();
        assert!(matches(Some(&t), &t));
        assert!(!matches(None, &t));
        assert!(!matches(Some(""), &t));
        assert!(!matches(Some(&t[..63]), &t));
        let mut wrong = t.clone().into_bytes();
        wrong[63] = if wrong[63] == b'0' { b'1' } else { b'0' };
        assert!(!matches(Some(std::str::from_utf8(&wrong).unwrap()), &t));
        assert!(!matches(Some(""), ""), "an empty token never matches");
    }

    /// Written whole where clients look, private (the folder and the file),
    /// readable through the client's own reader, and removed on drop; a
    /// file another launch rewrote is kept.
    #[test]
    fn the_file_is_written_privately_read_and_removed() {
        let dir = std::env::temp_dir().join(format!("eus_bridge_token_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let token = generate();
        let file = TokenFile::write_in(&dir, 50123, &token).unwrap();
        assert_eq!(eustress_bridge_client::read_token_in(&dir, 50123).as_deref(), Some(token.as_str()));
        assert!(!dir.join("50123.token.tmp").exists());
        crate::private_file::verify(&dir).unwrap();
        crate::private_file::verify(&dir.join("50123.token")).unwrap();
        drop(file);
        assert!(eustress_bridge_client::read_token_in(&dir, 50123).is_none());

        let file = TokenFile::write_in(&dir, 50124, &token).unwrap();
        std::fs::write(dir.join("50124.token"), "another launch").unwrap();
        drop(file);
        assert!(dir.join("50124.token").exists(), "a file another launch rewrote is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
