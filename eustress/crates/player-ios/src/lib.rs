//! iOS player shell.
//!
//! UNVERIFIED. This machine has no Xcode and no `xcrun`, so nothing here is
//! compiled against an Apple SDK. `cargo check` on the host only builds the
//! non-iOS arms of [`eustress_bridge_ios`].
//!
//! The URL scheme lives in `Info.plist` next to this file
//! (`CFBundleURLTypes` / `eustress-player`). A real iOS app target still has
//! to be generated on a Mac and must call [`eustress_bridge_ios::IosPlatform`]
//! for insets and haptics. Keychain reads and writes return errors on purpose:
//! the Security.framework query is a stub.

pub use eustress_bridge_ios::IosPlatform;

/// The scheme the plist registers. Parsing of the URI is the mobile player's
/// `links` module, which calls `split_link_extras`. This crate does not link
/// that library: it is the Android cdylib, and an iOS target would link it
/// once a Mac build exists.
pub const URL_SCHEME: &str = "eustress-player";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scheme_matches_the_desktop_player() {
        assert_eq!(URL_SCHEME, "eustress-player");
        let _ = IosPlatform;
    }
}
