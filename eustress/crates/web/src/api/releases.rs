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

/// Where Studio's release pipeline publishes its manifest.
pub const STUDIO_MANIFEST_URL: &str = "https://downloads.eustress.dev/latest.json";

/// The Player's permanent download links: `<base>/<manifest key>` redirects to
/// that platform's file in the current release. Buttons use these rather than
/// the versioned URLs in the manifest, so a page left open across a release
/// still downloads the build that is current when it is clicked.
const PLAYER_LATEST_BASE: &str = "https://downloads.eustress.dev/player/latest";

/// Studio's permanent download links, the same way.
const STUDIO_LATEST_BASE: &str = "https://downloads.eustress.dev/latest";

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

    /// The platform's name as a sentence uses it: "Windows", "macOS", "Linux".
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOS => "macOS",
            Self::Linux => "Linux",
        }
    }

    /// The desktop platform a browser's user-agent text names, or `None` for a phone, a tablet, a Chromebook or anything unknown:
    /// there is no build for those.
    pub fn from_user_agent(user_agent: &str) -> Option<Self> {
        let ua = user_agent.to_ascii_lowercase();
        if ["android", "iphone", "ipad", "ipod", "cros"].iter().any(|m| ua.contains(m)) {
            return None;
        }
        if ua.contains("windows") {
            Some(Self::Windows)
        } else if ua.contains("mac os") || ua.contains("macintosh") {
            Some(Self::MacOS)
        } else if ua.contains("linux") || ua.contains("x11") {
            Some(Self::Linux)
        } else {
            None
        }
    }

    /// The visitor's computer. The prerender has no visitor, so it knows nothing.
    pub fn detect() -> Option<Self> {
        #[cfg(feature = "ssr")]
        {
            None
        }
        #[cfg(not(feature = "ssr"))]
        {
            web_sys::window().and_then(|w| w.navigator().user_agent().ok()).and_then(|ua| Self::from_user_agent(&ua))
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
        self.file_for(PLAYER_LATEST_BASE, platform)
    }

    /// The Studio file to offer for a platform, the same way.
    pub fn studio_download(&self, platform: DesktopPlatform) -> Option<PlayerDownload> {
        self.file_for(STUDIO_LATEST_BASE, platform)
    }

    /// The file of a product's release for a platform.
    pub fn download(&self, product: super::release_notify::Product, platform: DesktopPlatform) -> Option<PlayerDownload> {
        match product {
            super::release_notify::Product::Player => self.player_download(platform),
            super::release_notify::Product::Studio => self.studio_download(platform),
        }
    }

    fn file_for(&self, base: &str, platform: DesktopPlatform) -> Option<PlayerDownload> {
        platform.keys().iter().find_map(|key| {
            let asset = self.platforms.get(*key)?;
            asset.url.starts_with("https://").then(|| PlayerDownload { url: format!("{base}/{key}"), asset: asset.clone() })
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

/// Studio's latest release.
pub async fn fetch_studio_release() -> ReleaseState {
    fetch_release(STUDIO_MANIFEST_URL).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_computer_is_known_by_its_user_agent_and_a_phone_or_a_tablet_has_no_build() {
        let windows = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126.0 Safari/537.36";
        let mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/17.0 Safari/605.1.15";
        let linux = "Mozilla/5.0 (X11; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0";
        assert_eq!(DesktopPlatform::from_user_agent(windows), Some(DesktopPlatform::Windows));
        assert_eq!(DesktopPlatform::from_user_agent(mac), Some(DesktopPlatform::MacOS));
        assert_eq!(DesktopPlatform::from_user_agent(linux), Some(DesktopPlatform::Linux));
        for phone in [
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/126.0 Mobile Safari/537.36",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Mobile/15E148 Safari/604.1",
            "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Mobile/15E148 Safari/604.1",
            "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 Chrome/126.0 Safari/537.36",
        ] {
            assert_eq!(DesktopPlatform::from_user_agent(phone), None, "{phone}");
        }
        assert_eq!(DesktopPlatform::from_user_agent(""), None);
    }

    #[test]
    fn each_product_links_its_own_permanent_address_and_only_for_a_build_that_is_there() {
        use super::super::release_notify::Product;
        let mut m = ReleaseManifest { version: "0.1.0".to_string(), date: String::new(), changelog_url: String::new(), platforms: BTreeMap::new() };
        m.platforms.insert("windows-x64-installer".to_string(), ReleaseAsset { url: "https://downloads.eustress.dev/v0.1.0/Setup.exe".to_string(), sha256: String::new(), size_bytes: 1 });
        assert_eq!(m.download(Product::Player, DesktopPlatform::Windows).map(|d| d.url), Some("https://downloads.eustress.dev/player/latest/windows-x64-installer".to_string()));
        assert_eq!(m.download(Product::Studio, DesktopPlatform::Windows).map(|d| d.url), Some("https://downloads.eustress.dev/latest/windows-x64-installer".to_string()));
        assert!(m.download(Product::Studio, DesktopPlatform::MacOS).is_none(), "no build, no link");
        // A plain-http address is not a build.
        m.platforms.insert("macos-arm64".to_string(), ReleaseAsset { url: "http://insecure.example/x.dmg".to_string(), sha256: String::new(), size_bytes: 1 });
        assert!(m.download(Product::Player, DesktopPlatform::MacOS).is_none());
    }

    #[test]
    fn a_platform_is_named_the_way_a_sentence_names_it() {
        assert_eq!(DesktopPlatform::MacOS.short_label(), "macOS");
        assert_eq!(DesktopPlatform::Windows.short_label(), "Windows");
    }
}
