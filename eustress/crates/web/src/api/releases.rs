// =============================================================================
// Eustress Web - Release Manifests
// =============================================================================
// Reads the latest.json a release pipeline publishes to downloads.eustress.dev,
// so download buttons follow each release instead of naming files by hand.
// The Player's manifest lives at /player/latest.json and has the shape
// .github/workflows/release.yml writes for Eustress Engine:
//
//     { "version", "date", "channel", "changelog_url",
//       "platforms": { "<key>": { "file", "url", "sha256", "size_bytes", "installer" } } }
//
// Table of Contents:
// 1. Platforms
// 2. Manifest
// 3. Fetching
// =============================================================================

use std::collections::BTreeMap;

use serde::Deserialize;

/// Where the Player's release pipeline publishes its manifest.
pub const PLAYER_MANIFEST_URL: &str = "https://downloads.eustress.dev/player/latest.json";

/// The Player's permanent download links: `<base>/<manifest key>` redirects to
/// that platform's file in the current release. Buttons use these rather than
/// the versioned URLs in the manifest, so a page left open across a release
/// still downloads the build that is current when it is clicked.
const PLAYER_LATEST_BASE: &str = "https://downloads.eustress.dev/player/latest";

// -----------------------------------------------------------------------------
// 1. Platforms
// -----------------------------------------------------------------------------

/// The desktop platforms a release ships for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPlatform {
    Windows,
    MacOS,
    Linux,
}

impl DesktopPlatform {
    pub const ALL: [DesktopPlatform; 3] = [Self::Windows, Self::MacOS, Self::Linux];

    /// Manifest keys to offer, best first. A visitor on Windows wants the
    /// installer; `windows-x64` is the zip the in-app updater unpacks over an
    /// existing install, offered only when a release has no installer.
    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Windows => &["windows-x64-installer", "windows-x64"],
            Self::MacOS => &["macos-arm64"],
            Self::Linux => &["linux-x64"],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOS => "macOS (Apple Silicon)",
            Self::Linux => "Linux",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Windows => "/assets/icons/windows.svg",
            Self::MacOS => "/assets/icons/macos.svg",
            Self::Linux => "/assets/icons/linux.svg",
        }
    }

    /// The `platform-btn` modifier class the download pages style.
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::Linux => "linux",
        }
    }
}

// -----------------------------------------------------------------------------
// 2. Manifest
// -----------------------------------------------------------------------------

/// One downloadable file of a release.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseAsset {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: u64,
}

impl ReleaseAsset {
    /// Download size for a button, in megabytes: "85 MB". Empty when the
    /// manifest gives none, so a button never claims a size it does not know.
    pub fn size_label(&self) -> String {
        match self.size_bytes {
            0 => String::new(),
            bytes => format!("{} MB", ((bytes + 500_000) / 1_000_000).max(1)),
        }
    }
}

/// A product's latest release.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseManifest {
    pub version: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub changelog_url: String,
    #[serde(default)]
    pub platforms: BTreeMap<String, ReleaseAsset>,
}

/// What to offer a visitor for one platform of the Player's current release.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDownload {
    /// The permanent link, which follows each new release.
    pub url: String,
    pub asset: ReleaseAsset,
}

impl ReleaseManifest {
    /// The Player file to offer for a platform, if this release has one. An
    /// entry without an https URL is skipped, so a half-written manifest shows
    /// the platform as coming soon instead of linking nowhere.
    pub fn player_download(&self, platform: DesktopPlatform) -> Option<PlayerDownload> {
        platform.keys().iter().find_map(|key| {
            let asset = self.platforms.get(*key)?;
            asset.url.starts_with("https://").then(|| PlayerDownload {
                url: format!("{PLAYER_LATEST_BASE}/{key}"),
                asset: asset.clone(),
            })
        })
    }
}

/// Where a product's release stands, for the page offering it.
#[derive(Debug, Clone, PartialEq)]
pub enum ReleaseState {
    /// Not looked up yet. The prerendered page shows this state.
    Checking,
    /// The host answered and there is no manifest: no release yet.
    Unreleased,
    /// The host could not be reached, or sent something that is not a manifest.
    Unreachable,
    Released(ReleaseManifest),
}

// -----------------------------------------------------------------------------
// 3. Fetching
// -----------------------------------------------------------------------------

/// The Player's latest release.
pub async fn fetch_player_release() -> ReleaseState {
    fetch_release(PLAYER_MANIFEST_URL).await
}

async fn fetch_release(url: &str) -> ReleaseState {
    let Ok(resp) = gloo_net::http::Request::get(url).send().await else {
        return ReleaseState::Unreachable;
    };
    if resp.status() == 404 {
        return ReleaseState::Unreleased;
    }
    if !resp.ok() {
        return ReleaseState::Unreachable;
    }
    match resp.json::<ReleaseManifest>().await {
        Ok(manifest) if !manifest.version.trim().is_empty() => ReleaseState::Released(manifest),
        _ => ReleaseState::Unreachable,
    }
}
