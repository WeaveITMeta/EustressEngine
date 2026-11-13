//! # Keeping the Player current
//!
//! At startup, on a worker thread, the Player reads the release manifest of
//! the channel it was built for (`latest.json` on downloads.eustress.dev, the
//! one `.github/workflows/player-release.yml` publishes) and compares that
//! release with its own version:
//!
//! - a newer release: a notice offers the update, and Enter dismisses it;
//! - this Player is below the manifest's `min_version`: a notice says it is
//!   too old to play, with the update as the way forward;
//! - a link's `min=X.Y.Z` above this Player (`main` checks it before anything
//!   is fetched or joined): a notice says the simulation needs a newer
//!   Player, with the same update.
//!
//! The update is the U key while one of these notices is on screen (the
//! screen notice has no buttons). It reads the manifest again, then:
//!
//! - Windows: downloads `windows-x64-installer` to a temp file, checks its
//!   size and SHA-256 against the manifest, starts it with Inno Setup's
//!   silent-upgrade flags ([`INSTALLER_FLAGS`]) and closes this Player. A
//!   file that does not match is deleted and never run.
//! - macOS and Linux: opens the release's download in the browser. The Player
//!   never replaces itself there.
//!
//! Every URL taken from a manifest must be `https://downloads.eustress.dev/`
//! under this channel's folder ([`check_download_url`]), redirects included.
//! A network failure or a bad manifest is logged and shows nothing; nothing
//! here holds up startup. `EUSTRESS_PLAYER_UPDATE_CHECK=0` turns the startup
//! check off, and a debug build checks only with `=1` ([`check_enabled`]).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use eustress_networking::join_link::{parse_hex32, parse_version, version_text};
use eustress_play_runtime::screen_notice::{Notice, ScreenNotice};
use sha2::{Digest, Sha256};

use super::join_notice::PlainNotice;

/// Set to `0` (or `false`, `no`, `off`) to skip the startup check; `1` (or
/// `true`, `yes`, `on`) turns it on in a debug build.
pub const UPDATE_CHECK_VAR: &str = "EUSTRESS_PLAYER_UPDATE_CHECK";
/// The one host a manifest, an installer or a download may come from.
pub const DOWNLOAD_HOST: &str = "downloads.eustress.dev";
/// Where a person gets the Player by hand, named when the Player cannot
/// update itself.
pub const DOWNLOAD_PAGE: &str = "eustress.dev/downloads/player";
/// Inno Setup's flags for an upgrade nobody has to click through
/// (`installer/windows/eustress-player.iss`): `/SILENT` shows only the
/// progress window, `/SUPPRESSMSGBOXES` answers any message box with its
/// default, `/NORESTART` never restarts Windows, `/CLOSEAPPLICATIONS` closes a
/// Player still holding a file the upgrade replaces, and `/SP-` skips the
/// "This will install" prompt. The install keeps the folder and mode (this
/// user or all users) of the one before it. `/RELAUNCH=1` starts the new
/// Player when the silent install finishes (the `.iss` `RelaunchAfterUpdate`
/// check); a person's own silent install, without it, starts nothing.
pub const INSTALLER_FLAGS: &[&str] =
    &["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/SP-", "/RELAUNCH=1"];

/// The largest manifest read.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// The largest installer accepted, whatever a manifest says.
const MAX_INSTALLER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Redirects followed, each one held to [`check_download_url`].
const MAX_REDIRECTS: usize = 5;

/// The release channel a Player was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Staging,
}

impl Channel {
    /// A build compiled for the staging API (`EUSTRESS_DEFAULT_API_URL`, as
    /// the release workflow's staging run sets it) is a staging Player; any
    /// other build is stable. A link's `api=` or `--env` never changes it:
    /// they choose a server for one run, not the Player installed.
    pub fn of_build(compiled_api: Option<&str>) -> Self {
        match compiled_api.map(|api| api.trim().trim_end_matches('/')) {
            Some(api) if api == eustress_common::api_base::STAGING_API => Channel::Staging,
            _ => Channel::Stable,
        }
    }

    /// This build's channel.
    pub fn this_build() -> Self {
        Self::of_build(option_env!("EUSTRESS_DEFAULT_API_URL"))
    }

    /// The channel's release manifest.
    pub fn manifest_url(self) -> &'static str {
        match self {
            Channel::Stable => "https://downloads.eustress.dev/player/latest.json",
            Channel::Staging => "https://downloads.eustress.dev/staging/player/latest.json",
        }
    }

    /// The folder on [`DOWNLOAD_HOST`] every file of this channel lives in.
    fn folder(self) -> &'static str {
        match self {
            Channel::Stable => "/player/",
            Channel::Staging => "/staging/player/",
        }
    }

    /// The manifest's `channel` value for this channel.
    fn manifest_name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Staging => "staging",
        }
    }
}

/// This Player's version, `None` if `CARGO_PKG_VERSION` is not `X.Y.Z`.
pub fn this_version() -> Option<[u32; 3]> {
    parse_version(env!("CARGO_PKG_VERSION"))
}

/// Whether the startup check runs: `EUSTRESS_PLAYER_UPDATE_CHECK` off-words
/// turn it off, on-words turn it on, and unset (or anything else) leaves it on
/// in a release build and off in a debug build, which is not installed and
/// should not offer to install over the Player that is.
pub fn check_enabled(var: Option<&str>, debug_build: bool) -> bool {
    match var.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("0" | "false" | "no" | "off") => false,
        Some("1" | "true" | "yes" | "on") => true,
        _ => !debug_build,
    }
}

/// The manifest's platform key for a Player on `os` and `arch`
/// (`std::env::consts`). Windows takes the installer, never the zip.
pub fn platform_key(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("windows-x64-installer"),
        ("macos", "aarch64") => Some("macos-arm64"),
        ("linux", "x86_64") => Some("linux-x64"),
        _ => None,
    }
}

/// The download for this Player's platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Passed [`check_download_url`].
    pub url: String,
    pub sha256: [u8; 32],
    pub size_bytes: u64,
}

/// What a manifest says, for this Player's channel and platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: [u32; 3],
    /// The oldest Player still allowed to play. Absent: no minimum.
    pub min_version: Option<[u32; 3]>,
    /// `None` when the manifest lists nothing for this platform.
    pub download: Option<Download>,
}

#[derive(serde::Deserialize)]
struct RawManifest {
    version: String,
    #[serde(default)]
    min_version: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    platforms: HashMap<String, RawPlatform>,
}

#[derive(serde::Deserialize)]
struct RawPlatform {
    url: String,
    sha256: String,
    size_bytes: u64,
}

/// A manifest's bytes as a [`Release`] for `channel` and the platform `key`
/// ([`platform_key`]). Refused whole when its version or minimum is not
/// `X.Y.Z`, the minimum is above the version, it names another channel, or
/// this platform's entry has a URL off [`DOWNLOAD_HOST`] or this channel's
/// folder, a SHA-256 that is not 64 hex characters, or a size of 0 or over
/// the limit. Another platform's entry is not read.
pub fn parse_manifest(json: &[u8], channel: Channel, key: Option<&str>) -> Result<Release, String> {
    let raw: RawManifest = serde_json::from_slice(json).map_err(|e| format!("the manifest is not valid JSON: {e}"))?;
    let version = parse_version(raw.version.trim()).ok_or("the manifest's version is not X.Y.Z")?;
    let min_version = match raw.min_version.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(min) => Some(parse_version(min).ok_or("the manifest's min_version is not X.Y.Z")?),
    };
    if min_version.is_some_and(|min| min > version) {
        return Err("the manifest's min_version is above its version".into());
    }
    if let Some(named) = raw.channel.as_deref() {
        if named != channel.manifest_name() {
            return Err(format!("the manifest is for the {named:?} channel, not {:?}", channel.manifest_name()));
        }
    }
    let download = match key.and_then(|k| raw.platforms.get(k)) {
        None => None,
        Some(p) => {
            check_download_url(&p.url, channel)?;
            let sha = p.sha256.trim();
            let sha256 = (sha.len() == 64).then(|| parse_hex32(sha)).flatten().ok_or("the download's sha256 is not 64 hex characters")?;
            if p.size_bytes == 0 || p.size_bytes > MAX_INSTALLER_BYTES {
                return Err(format!("the download's size_bytes ({}) is out of range", p.size_bytes));
            }
            Some(Download { url: p.url.clone(), sha256, size_bytes: p.size_bytes })
        }
    };
    Ok(Release { version, min_version, download })
}

/// `url`, when it is `https://downloads.eustress.dev/` with no port, user,
/// query or fragment, and its path is in this channel's folder (after `..`
/// and its percent-encoded forms are resolved).
pub fn check_download_url(url: &str, channel: Channel) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "a download URL in the manifest is not a URL".to_string())?;
    let ok = parsed.scheme() == "https"
        && parsed.host_str() == Some(DOWNLOAD_HOST)
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path().starts_with(channel.folder());
    if !ok {
        return Err(format!("refused a download URL that is not https://{DOWNLOAD_HOST}{}...", channel.folder()));
    }
    Ok(parsed)
}

/// What this Player should do about a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to offer: up to date, or no download for this platform.
    Current,
    /// A newer release can be installed.
    Newer([u32; 3]),
    /// This Player is below the manifest's minimum.
    TooOld { min: [u32; 3] },
}

pub fn verdict(this: [u32; 3], release: &Release) -> Verdict {
    if let Some(min) = release.min_version.filter(|min| this < *min) {
        return Verdict::TooOld { min };
    }
    if release.version > this && release.download.is_some() {
        Verdict::Newer(release.version)
    } else {
        Verdict::Current
    }
}

/// Read `file` to its end and accept it only when it is exactly `size` bytes
/// whose SHA-256 is `sha256`. Stops reading as soon as it is too long.
pub fn verify_download(mut file: impl Read, size: u64, sha256: &[u8; 32]) -> Result<(), String> {
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("could not read the download: {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > size {
            return Err(format!("the download is larger than the {size} bytes the manifest lists"));
        }
        hasher.update(&buf[..n]);
    }
    if total != size {
        return Err(format!("the download is {total} bytes, and the manifest lists {size}"));
    }
    if hasher.finalize().as_slice() != sha256.as_slice() {
        return Err("the download does not match the SHA-256 the manifest lists".into());
    }
    Ok(())
}

/// FUTURE SIGNATURE CHECK (minisign), not built yet. The release workflow
/// publishes `checksums.txt` beside each release. Once it also signs that
/// file, this fetches `checksums.txt` and its `.minisig` from the release's
/// folder, verifies the signature with a public key compiled in here, and
/// requires the installer's line to match `download.sha256`; until then it
/// accepts. No key exists yet: do not add one that is not the release key.
fn verify_release_signature(_download: &Download) -> Result<(), String> {
    Ok(())
}

/// The installer's arguments: [`INSTALLER_FLAGS`], then `/LOG=<log>`.
pub fn installer_args(log: &Path) -> Vec<OsString> {
    let mut log_arg = OsString::from("/LOG=");
    log_arg.push(log.as_os_str());
    INSTALLER_FLAGS.iter().map(OsString::from).chain(std::iter::once(log_arg)).collect()
}

// ---------------------------------------------------------------------------
// What the Player says
// ---------------------------------------------------------------------------

/// The update key's hint, for this platform.
fn update_hint(windows: bool) -> &'static str {
    if windows {
        "Press U to update. The Player downloads the update, checks it, installs it and closes."
    } else {
        "Press U to open the download in your browser."
    }
}

fn manual_hint() -> String {
    format!("Download the newest Eustress Player from {DOWNLOAD_PAGE}.")
}

fn plain(title: impl Into<String>, detail: impl Into<String>, hint: Option<String>) -> PlainNotice {
    PlainNotice { title: title.into(), detail: detail.into(), hint }
}

/// A newer release is ready. Enter keeps playing.
pub fn newer_notice(latest: [u32; 3], this: [u32; 3], windows: bool) -> PlainNotice {
    let how = if windows {
        "Press U to update now: the Player downloads it, checks it, installs it and closes."
    } else {
        "Press U to open the download in your browser."
    };
    plain(
        format!("A new Eustress Player is ready (v{})", version_text(latest)),
        format!("This Player is v{}.", version_text(this)),
        Some(format!("{how} Press Enter to keep playing.")),
    )
}

/// This Player is below the manifest's minimum.
pub fn too_old_notice(min: [u32; 3], this: [u32; 3], can_update: bool, windows: bool) -> PlainNotice {
    plain(
        "This Player is too old to play",
        format!("Eustress needs Player v{} or later, and this one is v{}.", version_text(min), version_text(this)),
        Some(if can_update { update_hint(windows).to_string() } else { manual_hint() }),
    )
}

/// A link names a minimum this Player is below.
pub fn link_needs_newer_notice(min: [u32; 3], this: [u32; 3], windows: bool) -> PlainNotice {
    plain(
        "Update the Eustress Player",
        format!(
            "This simulation needs a newer Eustress Player ({} or later). This one is {}.",
            version_text(min),
            version_text(this)
        ),
        Some(format!("{} Then open the link again.", update_hint(windows))),
    )
}

fn downloading_notice() -> PlainNotice {
    plain(
        "Updating the Eustress Player",
        "Downloading the new Player and checking it. This can take a few minutes.",
        None,
    )
}

fn browser_notice() -> PlainNotice {
    plain(
        "The download is open in your browser",
        "Install the new Player from it, then open Eustress Player again.",
        None,
    )
}

/// An update that did not happen, in a sentence, with a retry.
pub fn failed_notice(reason: &str) -> PlainNotice {
    let mut detail = reason.trim().to_string();
    if let Some(first) = detail.get(..1) {
        detail = format!("{}{}", first.to_uppercase(), &detail[1..]);
    }
    if !detail.ends_with(['.', '!', '?']) {
        detail.push('.');
    }
    plain(
        "The update did not install",
        detail,
        Some(format!("Press U to try again, or download the newest Eustress Player from {DOWNLOAD_PAGE}.")),
    )
}

// ---------------------------------------------------------------------------
// Network and process work, on worker threads
// ---------------------------------------------------------------------------

/// An HTTPS-only client whose every redirect must pass
/// [`check_download_url`].
fn http_client(channel: Channel, timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if check_download_url(attempt.url().as_str(), channel).is_ok() {
            attempt.follow()
        } else {
            attempt.error(format!("a redirect left https://{DOWNLOAD_HOST}{}", channel.folder()))
        }
    });
    reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(policy)
        .connect_timeout(Duration::from_secs(20))
        .timeout(timeout)
        .user_agent(concat!("eustress-client/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

/// The channel's manifest, for this platform.
fn fetch_release(channel: Channel) -> Result<Release, String> {
    let client = http_client(channel, Duration::from_secs(30))?;
    let url = channel.manifest_url();
    let resp = client.get(url).send().map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("{status} from {url}"));
    }
    let mut body = Vec::new();
    resp.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut body).map_err(|e| format!("read {url}: {e}"))?;
    if body.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!("{url} is larger than {MAX_MANIFEST_BYTES} bytes"));
    }
    parse_manifest(&body, channel, platform_key(std::env::consts::OS, std::env::consts::ARCH))
}

/// Open `path` to read, and on Windows let nobody else write, rename or
/// delete it while the handle is held: the file that was checked is the file
/// that runs. Reading and running it stay allowed.
#[cfg(windows)]
fn open_held(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)
}

#[cfg(not(windows))]
fn open_held(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Download the installer to `path`: no more than `size_bytes` are written.
fn download_to(client: &reqwest::blocking::Client, download: &Download, path: &Path) -> Result<(), String> {
    let url = download.url.as_str();
    let resp = client.get(url).send().map_err(|e| format!("could not download the update: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("the update download answered {status}"));
    }
    if let Some(len) = resp.content_length().filter(|len| *len != download.size_bytes) {
        return Err(format!("the download is {len} bytes, and the manifest lists {}", download.size_bytes));
    }
    let mut file = std::fs::File::create(path).map_err(|e| format!("could not save the update: {e}"))?;
    let written = std::io::copy(&mut resp.take(download.size_bytes + 1), &mut file)
        .map_err(|e| format!("could not save the update: {e}"))?;
    if written != download.size_bytes {
        return Err(format!("the download is {written} bytes, and the manifest lists {}", download.size_bytes));
    }
    file.sync_all().map_err(|e| format!("could not save the update: {e}"))
}

/// Download, check and start the Windows installer. `Ok` once it runs.
fn install_on_windows(download: &Download, channel: Channel) -> Result<(), String> {
    let client = http_client(channel, Duration::from_secs(60 * 60))?;
    let dir = std::env::temp_dir().join("eustress-player-update");
    // Earlier updates' files; one an installer still runs from stays.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make a folder for the update: {e}"))?;
    let pid = std::process::id();
    let path = dir.join(format!("EustressPlayer-Setup-{pid}.exe"));
    let log = dir.join(format!("install-{pid}.log"));

    let checked = download_to(&client, download, &path).and_then(|()| {
        let held = open_held(&path).map_err(|e| format!("could not open the update: {e}"))?;
        verify_download(&held, download.size_bytes, &download.sha256)?;
        verify_release_signature(download)?;
        Ok(held)
    });
    let held = match checked {
        Ok(held) => held,
        Err(e) => {
            // Never left where it could be run by hand.
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };
    info!("update: {} matches the manifest ({} bytes); starting it", path.display(), download.size_bytes);
    let started = std::process::Command::new(&path).args(installer_args(&log)).spawn();
    drop(held);
    match started {
        Ok(child) => {
            info!("update: the installer is running (process {}, log {})", child.id(), log.display());
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(format!("could not start the installer: {e}"))
        }
    }
}

/// Open a checked download URL in the browser. Passed as one argument, never
/// through a shell.
fn open_in_browser(url: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open the browser ({program}): {e}"))
}

/// What a worker thread reports.
enum Outcome {
    Checked(Result<Release, String>),
    /// The installer is running; the Player closes.
    Installing,
    /// The download is open in the browser.
    Opened,
    Failed(String),
}

/// The update itself: the manifest again, then the platform's way.
/// `needed`: the version a minimum asks for, if any.
fn run_update(channel: Channel, this: [u32; 3], needed: Option<[u32; 3]>) -> Outcome {
    let release = match fetch_release(channel) {
        Ok(release) => release,
        Err(e) => {
            warn!("update: {e}");
            return Outcome::Failed("Eustress's release list could not be read. Check the internet connection".into());
        }
    };
    if let Some(needed) = needed.filter(|n| release.version < *n) {
        return Outcome::Failed(format!(
            "Player {} is not published yet; the newest is {}",
            version_text(needed),
            version_text(release.version)
        ));
    }
    if release.version <= this {
        return Outcome::Failed(format!("No Player newer than {} is published yet", version_text(this)));
    }
    let Some(download) = release.download else {
        return Outcome::Failed("No update is published for this computer's system".into());
    };
    if cfg!(windows) {
        match install_on_windows(&download, channel) {
            Ok(()) => Outcome::Installing,
            Err(e) => Outcome::Failed(e),
        }
    } else {
        match open_in_browser(&download.url) {
            Ok(()) => Outcome::Opened,
            Err(e) => Outcome::Failed(e),
        }
    }
}

// ---------------------------------------------------------------------------
// The Bevy side
// ---------------------------------------------------------------------------

/// Set by `main` when the launching link's `min` is above this Player, in
/// place of the join or fetch it would have started.
#[derive(Resource, Debug, Clone, Copy)]
pub struct LinkNeedsNewer(pub [u32; 3]);

/// The update notice this module put on screen, if it is still there.
#[derive(Resource, Default)]
struct UpdateOffer {
    /// What was shown. The keys act only while the screen still shows it.
    shown: Option<Notice>,
    /// Enter dismisses it (a newer release, not a minimum).
    optional: bool,
    /// U starts the update.
    can_update: bool,
    /// The version a minimum asks for.
    needed: Option<[u32; 3]>,
    /// An update is under way.
    busy: bool,
}

#[derive(Resource)]
struct UpdateChannel {
    tx: Sender<Outcome>,
    rx: Mutex<Receiver<Outcome>>,
}

pub struct PlayerUpdatePlugin;

impl Plugin for PlayerUpdatePlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = channel();
        app.insert_resource(UpdateChannel { tx, rx: Mutex::new(rx) })
            .init_resource::<UpdateOffer>()
            .add_systems(Startup, (show_link_needs_newer.run_if(resource_exists::<LinkNeedsNewer>), start_check))
            .add_systems(Update, (receive_outcomes, drive_update_keys).chain());
    }
}

fn offer(screen: &mut ScreenNotice, state: &mut UpdateOffer, n: PlainNotice) {
    let notice = Notice { title: n.title, detail: n.detail, hint: n.hint };
    screen.show(notice.clone());
    state.shown = Some(notice);
}

fn show_link_needs_newer(need: Res<LinkNeedsNewer>, mut screen: ResMut<ScreenNotice>, mut state: ResMut<UpdateOffer>) {
    let this = this_version().unwrap_or([0, 0, 0]);
    warn!("update: the link needs Player {} or later; this is {}", version_text(need.0), env!("CARGO_PKG_VERSION"));
    state.optional = false;
    state.can_update = true;
    state.needed = Some(need.0);
    offer(&mut screen, &mut state, link_needs_newer_notice(need.0, this, cfg!(windows)));
}

fn start_check(chan: Res<UpdateChannel>) {
    if !check_enabled(std::env::var(UPDATE_CHECK_VAR).ok().as_deref(), cfg!(debug_assertions)) {
        info!("update: startup check off ({UPDATE_CHECK_VAR})");
        return;
    }
    if this_version().is_none() {
        warn!("update: this Player's version {} is not X.Y.Z; no check", env!("CARGO_PKG_VERSION"));
        return;
    }
    let channel = Channel::this_build();
    let tx = chan.tx.clone();
    let spawned = std::thread::Builder::new().name("player-update-check".into()).spawn(move || {
        let _ = tx.send(Outcome::Checked(fetch_release(channel)));
    });
    if let Err(e) = spawned {
        warn!("update: could not start the check: {e}");
    }
}

fn receive_outcomes(
    chan: Res<UpdateChannel>,
    mut screen: ResMut<ScreenNotice>,
    mut state: ResMut<UpdateOffer>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(rx) = chan.rx.lock() else { return };
    let windows = cfg!(windows);
    while let Ok(outcome) = rx.try_recv() {
        match outcome {
            Outcome::Checked(Err(e)) => warn!("update: no check this time: {e}"),
            Outcome::Checked(Ok(release)) => {
                let Some(this) = this_version() else { continue };
                match verdict(this, &release) {
                    Verdict::Current => info!("update: v{} is current", version_text(this)),
                    Verdict::Newer(latest) => {
                        info!("update: v{} is published; this is v{}", version_text(latest), version_text(this));
                        // Only over an empty screen: a failure on screen
                        // says more.
                        if screen.0.is_none() && state.shown.is_none() {
                            state.optional = true;
                            state.can_update = true;
                            state.needed = None;
                            offer(&mut screen, &mut state, newer_notice(latest, this, windows));
                        }
                    }
                    Verdict::TooOld { min } => {
                        warn!("update: this Player v{} is below the minimum v{}", version_text(this), version_text(min));
                        // A link's own minimum is already on screen, and says
                        // the same with its own words.
                        if state.needed.is_none() {
                            state.optional = false;
                            let can_update = release.download.is_some();
                            state.can_update = can_update;
                            state.needed = Some(min);
                            offer(&mut screen, &mut state, too_old_notice(min, this, can_update, windows));
                        }
                    }
                }
            }
            Outcome::Installing => {
                info!("update: the installer is running; closing the Player");
                exit.write(AppExit::Success);
            }
            Outcome::Opened => {
                state.busy = false;
                state.can_update = false;
                state.optional = state.needed.is_none();
                offer(&mut screen, &mut state, browser_notice());
            }
            Outcome::Failed(e) => {
                error!("update: {e}");
                state.busy = false;
                state.can_update = true;
                state.optional = state.needed.is_none();
                offer(&mut screen, &mut state, failed_notice(&e));
            }
        }
    }
}

/// U updates and Enter dismisses (an optional offer only), while the update
/// notice is the one on screen, the pause menu is closed and no TextBox has
/// the keyboard.
fn drive_update_keys(
    keys: Res<ButtonInput<KeyCode>>,
    hud: Option<Res<eustress_common::play_session::HudPointer>>,
    pause: Option<Res<crate::plugins::pause_menu::PauseMenuState>>,
    chan: Res<UpdateChannel>,
    mut screen: ResMut<ScreenNotice>,
    mut state: ResMut<UpdateOffer>,
) {
    if state.shown.is_none() || state.busy {
        return;
    }
    if screen.0.as_ref() != state.shown.as_ref() {
        // Something else took the screen; the offer went with it.
        state.shown = None;
        return;
    }
    if hud.is_some_and(|h| h.keys_taken) || pause.is_some_and(|p| p.is_open) {
        return;
    }
    if state.can_update && keys.just_pressed(KeyCode::KeyU) {
        let Some(this) = this_version() else { return };
        let (channel, needed, tx) = (Channel::this_build(), state.needed, chan.tx.clone());
        let spawned = std::thread::Builder::new()
            .name("player-update".into())
            .spawn(move || {
                let _ = tx.send(run_update(channel, this, needed));
            });
        match spawned {
            Ok(_) => {
                info!("update: started ({:?} channel)", channel);
                state.busy = true;
                offer(&mut screen, &mut state, downloading_notice());
            }
            Err(e) => {
                state.optional = state.needed.is_none();
                offer(&mut screen, &mut state, failed_notice(&format!("could not start the update: {e}")));
            }
        }
    } else if state.optional && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)) {
        screen.clear();
        state.shown = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The staging manifest as downloads.eustress.dev served it on
    /// 2026-11-13, with a version and minimum put in by each test.
    fn manifest(version: &str, min: Option<&str>, channel: &str) -> String {
        let min = min.map(|m| format!("\"min_version\": \"{m}\",")).unwrap_or_default();
        format!(
            r#"{{
  "version": "{version}",
  {min}
  "date": "2026-10-20",
  "channel": "{channel}",
  "changelog_url": "https://github.com/WeaveITMeta/EustressEngine/actions/runs/39470312440",
  "platforms": {{
    "windows-x64":           {{ "file": "eustress-player-v0.1.0-windows-x64.zip",   "url": "https://downloads.eustress.dev/staging/player/v0.1.0/eustress-player-v0.1.0-windows-x64.zip",   "sha256": "ecd926aee0e593a576071af90472aa72393a303dbe5a52cfd9db22b6f9959b83",   "size_bytes": 199353361,   "installer": false }},
    "windows-x64-installer": {{ "file": "EustressPlayer-Setup.exe", "url": "https://downloads.eustress.dev/staging/player/v0.1.0/EustressPlayer-Setup.exe", "sha256": "66abd8efb48c83d7c1bb290a32fb4c19387f67df9e2f1843e67b36c5d61a015a", "size_bytes": 181249148, "installer": true }},
    "linux-x64":             {{ "file": "eustress-player-v0.1.0-linux-x64.tar.gz",   "url": "https://downloads.eustress.dev/staging/player/v0.1.0/eustress-player-v0.1.0-linux-x64.tar.gz",   "sha256": "84c1eb0d0ec44fe1d00408f70b8450198fb1a5f6303a8906fb31d173c0d9e34f",   "size_bytes": 204372767,   "installer": false }}
  }}
}}"#
        )
    }

    const WIN: Option<&str> = Some("windows-x64-installer");

    /// The served manifest reads as the installer for Windows, with its hash
    /// and size, and a platform it does not list has no download.
    #[test]
    fn the_served_manifest_reads() {
        let r = parse_manifest(manifest("0.3.0", None, "staging").as_bytes(), Channel::Staging, WIN).unwrap();
        assert_eq!((r.version, r.min_version), ([0, 3, 0], None));
        let d = r.download.expect("Windows has an installer");
        assert_eq!(d.url, "https://downloads.eustress.dev/staging/player/v0.1.0/EustressPlayer-Setup.exe");
        assert_eq!(d.size_bytes, 181_249_148);
        assert_eq!(d.sha256[0], 0x66);
        assert_eq!(d.sha256[31], 0x5a);
        let mac = parse_manifest(manifest("0.3.0", None, "staging").as_bytes(), Channel::Staging, Some("macos-arm64")).unwrap();
        assert_eq!(mac.download, None, "staging 0.1.0 listed no macOS build here");
        let min = parse_manifest(manifest("0.3.0", Some("0.2.1"), "staging").as_bytes(), Channel::Staging, WIN).unwrap();
        assert_eq!(min.min_version, Some([0, 2, 1]));
    }

    /// A manifest that is malformed, for the other channel, or with a minimum
    /// above its version is refused whole.
    #[test]
    fn a_bad_manifest_is_refused() {
        let staging = |m: String| parse_manifest(m.as_bytes(), Channel::Staging, WIN);
        assert!(staging(manifest("0.3", None, "staging")).is_err());
        assert!(staging(manifest("v0.3.0", None, "staging")).is_err());
        assert!(staging(manifest("0.3.0", Some("latest"), "staging")).is_err());
        assert!(staging(manifest("0.3.0", Some("0.4.0"), "staging")).unwrap_err().contains("above"));
        assert!(staging(manifest("0.3.0", None, "stable")).unwrap_err().contains("channel"));
        assert!(parse_manifest(b"not json", Channel::Staging, WIN).is_err());
        assert!(parse_manifest(b"{}", Channel::Staging, WIN).is_err(), "no version");
        // The staging manifest's URLs are not in the stable folder.
        let stable = manifest("0.3.0", None, "stable");
        assert!(parse_manifest(stable.as_bytes(), Channel::Stable, WIN).is_err());
        // A manifest that names no channel is read as the one fetched.
        let unnamed = manifest("0.3.0", None, "staging").replace("\"channel\": \"staging\",", "");
        assert!(staging(unnamed).is_ok());
    }

    /// This platform's entry must hold a good hash and size; another
    /// platform's entry is not read.
    #[test]
    fn this_platforms_entry_is_checked() {
        let good = manifest("0.3.0", None, "staging");
        let short_sha = good.replace("66abd8efb48c83d7c1bb290a32fb4c19387f67df9e2f1843e67b36c5d61a015a", "66abd8ef");
        assert!(parse_manifest(short_sha.as_bytes(), Channel::Staging, WIN).unwrap_err().contains("sha256"));
        assert!(parse_manifest(short_sha.as_bytes(), Channel::Staging, Some("linux-x64")).is_ok());
        let zero = good.replace("\"size_bytes\": 181249148", "\"size_bytes\": 0");
        assert!(parse_manifest(zero.as_bytes(), Channel::Staging, WIN).unwrap_err().contains("size_bytes"));
        let huge = good.replace("\"size_bytes\": 181249148", "\"size_bytes\": 99999999999");
        assert!(parse_manifest(huge.as_bytes(), Channel::Staging, WIN).is_err());
        let elsewhere = good.replace(
            "https://downloads.eustress.dev/staging/player/v0.1.0/EustressPlayer-Setup.exe",
            "https://evil.example/staging/player/v0.1.0/EustressPlayer-Setup.exe",
        );
        assert!(parse_manifest(elsewhere.as_bytes(), Channel::Staging, WIN).is_err());
    }

    /// Only `https://downloads.eustress.dev/` in this channel's folder, with
    /// no tricks in the host, port, user, path, query or fragment.
    #[test]
    fn only_the_download_host_is_trusted() {
        let ok = "https://downloads.eustress.dev/staging/player/v0.3.0/EustressPlayer-Setup.exe";
        assert!(check_download_url(ok, Channel::Staging).is_ok());
        assert!(check_download_url("https://downloads.eustress.dev:443/player/v1.0.0/x.exe", Channel::Stable).is_ok(), "443 is https's own port");
        for bad in [
            "http://downloads.eustress.dev/staging/player/v0.3.0/x.exe",
            "https://downloads.eustress.dev.evil.example/staging/player/x.exe",
            "https://evil.example/staging/player/x.exe",
            "https://evil.example@downloads.eustress.dev/staging/player/x.exe",
            "https://downloads.eustress.dev@evil.example/staging/player/x.exe",
            "https://downloads.eustress.dev:8443/staging/player/x.exe",
            "https://downloads.eustress.dev./staging/player/x.exe",
            "https://xdownloads.eustress.dev/staging/player/x.exe",
            "https://downloads.eustress.dev/player/v0.3.0/x.exe",
            "https://downloads.eustress.dev/staging/player/../../other/x.exe",
            "https://downloads.eustress.dev/staging/player/%2e%2e/%2e%2e/other/x.exe",
            "https://downloads.eustress.dev/staging/player/x.exe?y=1",
            "https://downloads.eustress.dev/staging/player/x.exe#y",
            "file:///C:/staging/player/x.exe",
            "downloads.eustress.dev/staging/player/x.exe",
            "",
        ] {
            assert!(check_download_url(bad, Channel::Staging).is_err(), "{bad}");
        }
        assert!(check_download_url(ok, Channel::Stable).is_err(), "a staging file is not a stable one");
    }

    #[test]
    fn newer_too_old_or_current() {
        let release = |v: [u32; 3], min: Option<[u32; 3]>, has: bool| Release {
            version: v,
            min_version: min,
            download: has.then(|| Download { url: String::new(), sha256: [0; 32], size_bytes: 1 }),
        };
        assert_eq!(verdict([0, 2, 0], &release([0, 3, 0], None, true)), Verdict::Newer([0, 3, 0]));
        assert_eq!(verdict([0, 2, 0], &release([0, 10, 0], None, true)), Verdict::Newer([0, 10, 0]), "numeric order");
        assert_eq!(verdict([0, 2, 0], &release([0, 2, 0], None, true)), Verdict::Current);
        assert_eq!(verdict([0, 3, 0], &release([0, 2, 0], None, true)), Verdict::Current, "a newer dev build");
        assert_eq!(verdict([0, 2, 0], &release([0, 3, 0], None, false)), Verdict::Current, "nothing to install here");
        assert_eq!(verdict([0, 2, 0], &release([0, 3, 0], Some([0, 2, 1]), true)), Verdict::TooOld { min: [0, 2, 1] });
        assert_eq!(verdict([0, 2, 0], &release([0, 3, 0], Some([0, 2, 1]), false)), Verdict::TooOld { min: [0, 2, 1] });
        assert_eq!(verdict([0, 2, 1], &release([0, 3, 0], Some([0, 2, 1]), true)), Verdict::Newer([0, 3, 0]), "at the minimum is enough");
    }

    /// Exactly the listed bytes with the listed hash; anything else is refused.
    #[test]
    fn a_download_must_match_its_size_and_hash() {
        let body = b"an installer".to_vec();
        let sha: [u8; 32] = Sha256::digest(&body).into();
        assert_eq!(verify_download(&body[..], body.len() as u64, &sha), Ok(()));
        let mut tampered = body.clone();
        tampered[0] ^= 1;
        assert!(verify_download(&tampered[..], body.len() as u64, &sha).unwrap_err().contains("SHA-256"));
        assert!(verify_download(&body[..body.len() - 1], body.len() as u64, &sha).unwrap_err().contains("bytes"));
        let mut longer = body.clone();
        longer.push(b'!');
        assert!(verify_download(&longer[..], body.len() as u64, &sha).unwrap_err().contains("larger"));
        assert!(verify_download(&body[..], body.len() as u64, &[0; 32]).is_err());
    }

    #[test]
    fn the_installer_upgrades_silently_and_logs() {
        let args = installer_args(Path::new(r"C:\Temp\eustress-player-update\install-7.log"));
        let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            [
                "/SILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
                "/SP-",
                "/RELAUNCH=1",
                r"/LOG=C:\Temp\eustress-player-update\install-7.log"
            ]
        );
    }

    #[test]
    fn the_channel_is_the_one_this_build_was_compiled_for() {
        assert_eq!(Channel::of_build(None), Channel::Stable);
        assert_eq!(Channel::of_build(Some("https://api-staging.eustress.dev")), Channel::Staging);
        assert_eq!(Channel::of_build(Some(" https://api-staging.eustress.dev/ ")), Channel::Staging);
        assert_eq!(Channel::of_build(Some("https://api.eustress.dev")), Channel::Stable);
        assert_eq!(Channel::of_build(Some("http://localhost:8787")), Channel::Stable);
        assert_eq!(Channel::Stable.manifest_url(), "https://downloads.eustress.dev/player/latest.json");
        assert_eq!(Channel::Staging.manifest_url(), "https://downloads.eustress.dev/staging/player/latest.json");
        for c in [Channel::Stable, Channel::Staging] {
            assert!(check_download_url(c.manifest_url(), c).is_ok(), "the manifest passes its own channel's rule");
        }
    }

    #[test]
    fn the_check_can_be_turned_off_and_a_dev_build_opts_in() {
        assert!(check_enabled(None, false));
        assert!(!check_enabled(None, true));
        for off in ["0", "false", " No ", "off"] {
            assert!(!check_enabled(Some(off), false), "{off}");
            assert!(!check_enabled(Some(off), true), "{off}");
        }
        for on in ["1", "true", "YES", "on"] {
            assert!(check_enabled(Some(on), true), "{on}");
        }
        assert!(!check_enabled(Some(""), true));
    }

    #[test]
    fn each_platform_takes_its_own_download() {
        assert_eq!(platform_key("windows", "x86_64"), Some("windows-x64-installer"));
        assert_eq!(platform_key("macos", "aarch64"), Some("macos-arm64"));
        assert_eq!(platform_key("linux", "x86_64"), Some("linux-x64"));
        assert_eq!(platform_key("macos", "x86_64"), None, "no Intel Mac build is published");
        assert_eq!(platform_key("windows", "aarch64"), None);
    }

    /// The release workflow checks `--version` against its tag, and every
    /// comparison here needs this Player's own version.
    #[test]
    fn this_players_version_is_x_y_z() {
        assert!(this_version().is_some(), "{} is not X.Y.Z", env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_words_name_the_versions() {
        let n = newer_notice([0, 3, 0], [0, 2, 0], true);
        assert_eq!(n.title, "A new Eustress Player is ready (v0.3.0)");
        assert!(n.hint.as_deref().unwrap().contains("Press U") && n.hint.as_deref().unwrap().contains("Enter"));
        let link = link_needs_newer_notice([0, 3, 0], [0, 2, 0], true);
        assert!(link.detail.starts_with("This simulation needs a newer Eustress Player (0.3.0 or later)."), "{}", link.detail);
        assert!(link.hint.unwrap().contains("Press U"));
        let old = too_old_notice([0, 2, 1], [0, 2, 0], false, true);
        assert!(old.detail.contains("v0.2.1") && old.hint.unwrap().contains(DOWNLOAD_PAGE));
        assert!(newer_notice([0, 3, 0], [0, 2, 0], false).hint.unwrap().contains("browser"));
        assert_eq!(failed_notice("could not download the update: timed out").detail, "Could not download the update: timed out.");
        for n in [n, browser_notice(), downloading_notice(), failed_notice("x")] {
            for text in [&n.title, &n.detail].into_iter().chain(n.hint.as_ref()) {
                assert!(!text.contains('\u{2014}'), "no em dash: {text}");
            }
        }
    }

    /// The checked file is the file that runs: while it is held, nothing can
    /// write it or delete it, and it still starts. Runs a copy of cmd.exe.
    #[cfg(windows)]
    #[test]
    fn a_held_installer_cannot_be_swapped_and_still_runs() {
        let dir = std::env::temp_dir().join(format!("eustress-update-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("held.exe");
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::fs::copy(Path::new(&system).join(r"System32\cmd.exe"), &exe).unwrap();
        let result = (|| {
            let held = open_held(&exe).map_err(|e| format!("open: {e}"))?;
            if std::fs::OpenOptions::new().write(true).open(&exe).is_ok() {
                return Err("the held file opened for writing".to_string());
            }
            if std::fs::remove_file(&exe).is_ok() {
                return Err("the held file was deleted".to_string());
            }
            let status = std::process::Command::new(&exe).args(["/C", "exit 7"]).status().map_err(|e| format!("run: {e}"))?;
            drop(held);
            if status.code() != Some(7) {
                return Err(format!("the held file ran with {status:?}"));
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result.unwrap();
    }
}
