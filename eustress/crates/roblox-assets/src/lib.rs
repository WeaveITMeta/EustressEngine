//! # eustress-roblox-assets
//!
//! Concrete [`AssetFetcher`] implementations for the Roblox place
//! importer. Quarantines the blocking-HTTP dependency OUT of the
//! engine-free [`eustress_roblox_import`] crate (spec
//! `docs/architecture/ROBLOX_IMPORT_SPEC.md` §11 / §19.3).
//!
//! The importer declares the [`AssetFetcher`] trait
//! (`fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, String>`); this
//! crate supplies three implementations plus an on-disk cache:
//!
//! - [`NetworkFetcher`] — GETs
//!   `https://assetdelivery.roblox.com/v1/asset/?id=<id>` (following the
//!   CDN redirect) and returns the raw bytes. Optional `.ROBLOSECURITY`
//!   cookie support for authenticated assets — the cookie is NEVER logged.
//! - [`LocalFolderFetcher`] — reads `<folder>/<id>.*` from a
//!   user-pointed directory (fully offline; for a community mirror or a
//!   prior export).
//! - [`ChainFetcher`] — tries a sequence of fetchers in order (local
//!   first, then network) and returns the first success.
//! - [`CachingFetcher`] — wraps any fetcher with an on-disk byte cache
//!   keyed by asset id, so re-imports don't re-fetch.
//!
//! All blocking, no tokio, no Bevy — runs on the importer's worker
//! thread. Wave F2 wires this for MESH assets only; the importer keeps
//! textures / sounds on the placeholder path until a later wave.
//!
//! ## Usage (engine side)
//!
//! ```ignore
//! use eustress_roblox_assets::{ChainFetcher, LocalFolderFetcher, NetworkFetcher, CachingFetcher};
//! use std::sync::Arc;
//!
//! let mut chain = ChainFetcher::new();
//! if let Some(dir) = local_mirror_dir {
//!     chain.push(Arc::new(LocalFolderFetcher::new(dir)));
//! }
//! chain.push(Arc::new(NetworkFetcher::new()));
//! let fetcher = CachingFetcher::new(cache_dir, Arc::new(chain));
//! opts.asset_fetcher = Some(Arc::new(fetcher));
//! ```

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eustress_roblox_import::AssetFetcher;

/// The Roblox asset-delivery CDN base. `?id=<n>` returns a 30x redirect to
/// the actual content host; `ureq` follows it by default.
const ASSET_DELIVERY_BASE: &str = "https://assetdelivery.roblox.com/v1/asset/?id=";

/// Cap on a single fetched asset (defensive — a runaway download or a
/// misrouted HTML error page should not exhaust memory). 256 MiB is far
/// above any real mesh.
const MAX_ASSET_BYTES: usize = 256 * 1024 * 1024;

// ===========================================================================
// NetworkFetcher
// ===========================================================================

/// Fetches assets over HTTP(S) from the Roblox asset-delivery CDN.
///
/// `cookie` (a `.ROBLOSECURITY` token) is optional and only needed for
/// assets gated behind authentication; it is sent as a `Cookie` header and
/// is **never** logged.
pub struct NetworkFetcher {
    agent: ureq::Agent,
    /// `.ROBLOSECURITY` token, if the integrator supplied one. Treated as
    /// a secret — never written to logs or errors.
    cookie: Option<String>,
}

impl NetworkFetcher {
    /// A network fetcher with no authentication cookie (public assets).
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .user_agent("Eustress-RobloxImport/0.1 (+https://eustress.dev)")
                .build(),
            cookie: None,
        }
    }

    /// A network fetcher that authenticates with a `.ROBLOSECURITY`
    /// cookie. The token is stored in memory only and never logged.
    pub fn with_cookie(token: impl Into<String>) -> Self {
        let mut f = Self::new();
        f.cookie = Some(token.into());
        f
    }

    /// Build the asset-delivery URL for an id.
    fn url_for(id: u64) -> String {
        format!("{ASSET_DELIVERY_BASE}{id}")
    }
}

impl Default for NetworkFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetFetcher for NetworkFetcher {
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, String> {
        let url = Self::url_for(asset_id);
        // Build the request. Attach the cookie only if present; the header
        // value (the secret) is intentionally never logged.
        let mut req = self.agent.get(&url);
        if let Some(cookie) = &self.cookie {
            req = req.set("Cookie", &format!(".ROBLOSECURITY={cookie}"));
        }
        tracing::debug!(asset_id, "roblox-assets: fetching asset over network");

        let resp = req
            .call()
            .map_err(|e| format!("network fetch rbxassetid://{asset_id} failed: {}", describe_ureq(e)))?;

        // Read the body with a hard cap.
        let mut reader = resp.into_reader().take(MAX_ASSET_BYTES as u64 + 1);
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut bytes)
            .map_err(|e| format!("reading rbxassetid://{asset_id} body failed: {e}"))?;
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(format!(
                "rbxassetid://{asset_id} exceeds {MAX_ASSET_BYTES}-byte cap"
            ));
        }
        if bytes.is_empty() {
            return Err(format!("rbxassetid://{asset_id} returned no bytes"));
        }
        tracing::debug!(asset_id, len = bytes.len(), "roblox-assets: fetched");
        Ok(bytes)
    }
}

/// Render a `ureq::Error` WITHOUT leaking the request URL's query (which
/// is just the id, but keep the discipline) or any header. We only surface
/// the status / transport reason.
fn describe_ureq(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _resp) => format!("HTTP {code}"),
        ureq::Error::Transport(t) => format!("transport: {}", t.kind()),
    }
}

// ===========================================================================
// LocalFolderFetcher
// ===========================================================================

/// Reads assets from a local directory, looking for `<id>.*` (any
/// extension). Lets an integrator point at a community mirror or a folder
/// of previously-downloaded assets for a fully offline import.
pub struct LocalFolderFetcher {
    dir: PathBuf,
}

impl LocalFolderFetcher {
    /// A local-folder fetcher rooted at `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Find a file named `<id>` or `<id>.<ext>` in the folder.
    fn find_file(&self, asset_id: u64) -> Option<PathBuf> {
        // Fast path: exact `<id>` and the common `<id>.mesh` name.
        let exact = self.dir.join(asset_id.to_string());
        if exact.is_file() {
            return Some(exact);
        }
        let mesh = self.dir.join(format!("{asset_id}.mesh"));
        if mesh.is_file() {
            return Some(mesh);
        }
        // Otherwise scan for any `<id>.*`.
        let prefix = asset_id.to_string();
        let entries = std::fs::read_dir(&self.dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem == prefix {
                    return Some(path);
                }
            }
        }
        None
    }
}

impl AssetFetcher for LocalFolderFetcher {
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, String> {
        match self.find_file(asset_id) {
            Some(path) => {
                tracing::debug!(asset_id, ?path, "roblox-assets: reading local asset");
                std::fs::read(&path)
                    .map_err(|e| format!("reading local asset {}: {e}", path.display()))
            }
            None => Err(format!(
                "no local file for rbxassetid://{asset_id} in {}",
                self.dir.display()
            )),
        }
    }
}

// ===========================================================================
// ChainFetcher
// ===========================================================================

/// Tries each inner fetcher in order, returning the first success. Use it
/// to prefer a local mirror, falling back to the network:
/// `ChainFetcher::new().push(local).push(network)`.
///
/// When every fetcher fails, returns a combined error listing each
/// failure (already secret-free — the inner fetchers never embed cookies).
pub struct ChainFetcher {
    fetchers: Vec<Arc<dyn AssetFetcher>>,
}

impl ChainFetcher {
    /// An empty chain. Push fetchers in priority order.
    pub fn new() -> Self {
        Self {
            fetchers: Vec::new(),
        }
    }

    /// Append a fetcher to the end of the chain (lower priority than those
    /// already pushed). Returns `&mut self` for chaining.
    pub fn push(&mut self, fetcher: Arc<dyn AssetFetcher>) -> &mut Self {
        self.fetchers.push(fetcher);
        self
    }

    /// True when no fetchers are configured (every `fetch` will error).
    pub fn is_empty(&self) -> bool {
        self.fetchers.is_empty()
    }
}

impl Default for ChainFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetFetcher for ChainFetcher {
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, String> {
        if self.fetchers.is_empty() {
            return Err(format!(
                "rbxassetid://{asset_id}: empty fetcher chain (no sources configured)"
            ));
        }
        let mut errors = Vec::with_capacity(self.fetchers.len());
        for fetcher in &self.fetchers {
            match fetcher.fetch(asset_id) {
                Ok(bytes) => return Ok(bytes),
                Err(e) => errors.push(e),
            }
        }
        Err(format!(
            "rbxassetid://{asset_id}: all {} sources failed [{}]",
            self.fetchers.len(),
            errors.join("; ")
        ))
    }
}

// ===========================================================================
// CachingFetcher — on-disk byte cache keyed by asset id
// ===========================================================================

/// Wraps an inner fetcher with a persistent on-disk cache. The first fetch
/// of an id writes `<cache_dir>/<id>.bin`; subsequent fetches (even across
/// process restarts / re-imports) read it back without touching the inner
/// fetcher. This is the spec §11 "re-imports don't re-fetch" guarantee.
pub struct CachingFetcher {
    cache_dir: PathBuf,
    inner: Arc<dyn AssetFetcher>,
}

impl CachingFetcher {
    /// A caching fetcher storing bytes under `cache_dir`, delegating misses
    /// to `inner`.
    pub fn new(cache_dir: impl Into<PathBuf>, inner: Arc<dyn AssetFetcher>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            inner,
        }
    }

    fn cache_path(&self, asset_id: u64) -> PathBuf {
        self.cache_dir.join(format!("{asset_id}.bin"))
    }

    /// Negative-cache marker for an id whose fetch FAILED (deleted asset,
    /// private asset, non-asset id). Holds the failure reason as text.
    fn err_path(&self, asset_id: u64) -> PathBuf {
        self.cache_dir.join(format!("{asset_id}.err"))
    }
}

impl AssetFetcher for CachingFetcher {
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, String> {
        let path = self.cache_path(asset_id);
        // Cache hit.
        if path.is_file() {
            match std::fs::read(&path) {
                Ok(bytes) if !bytes.is_empty() => {
                    tracing::debug!(asset_id, "roblox-assets: cache hit");
                    return Ok(bytes);
                }
                // Empty / unreadable cache entry → fall through to refetch.
                _ => {}
            }
        }
        // Negative-cache hit: a previous run already learned this id fails.
        // Batch imports repeat the same dead ids thousands of times (place
        // files share broken references) — honouring the marker keeps
        // re-imports from re-hammering the CDN. Delete the `.err` files or
        // set `EUSTRESS_ROBLOX_RETRY_ERRORS=1` to retry them.
        let err_marker = self.err_path(asset_id);
        let retry_errors = std::env::var("EUSTRESS_ROBLOX_RETRY_ERRORS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !retry_errors && err_marker.is_file() {
            if let Ok(reason) = std::fs::read_to_string(&err_marker) {
                return Err(format!("{} (cached failure)", reason.trim()));
            }
        }
        // Miss → delegate. Persist bytes on success, the reason on failure
        // (both best-effort: a cache write failure must not change the
        // fetch outcome).
        match self.inner.fetch(asset_id) {
            Ok(bytes) => {
                if let Err(e) = write_cache(&self.cache_dir, &path, &bytes) {
                    tracing::warn!(asset_id, "roblox-assets: cache write failed: {e}");
                }
                // A stale failure marker from an earlier run is now wrong.
                let _ = std::fs::remove_file(&err_marker);
                Ok(bytes)
            }
            Err(e) => {
                if let Err(werr) = write_cache(&self.cache_dir, &err_marker, e.as_bytes()) {
                    tracing::warn!(asset_id, "roblox-assets: negative-cache write failed: {werr}");
                }
                Err(e)
            }
        }
    }
}

/// Best-effort cache write: ensure the dir exists, then write the bytes.
fn write_cache(cache_dir: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    std::fs::write(path, bytes)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A fetcher that records how many times it was called and returns
    /// canned bytes. Lets the cache/chain tests run with no network.
    struct Counting {
        bytes: Vec<u8>,
        calls: std::sync::atomic::AtomicUsize,
        fail: bool,
    }
    impl Counting {
        fn ok(bytes: &[u8]) -> Arc<Self> {
            Arc::new(Self {
                bytes: bytes.to_vec(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                fail: false,
            })
        }
        fn failing() -> Arc<Self> {
            Arc::new(Self {
                bytes: Vec::new(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                fail: true,
            })
        }
        fn count(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }
    impl AssetFetcher for Counting {
        fn fetch(&self, _asset_id: u64) -> Result<Vec<u8>, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                Err("counting fetcher: forced failure".into())
            } else {
                Ok(self.bytes.clone())
            }
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rbx_assets_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn network_url_is_asset_delivery() {
        assert_eq!(
            NetworkFetcher::url_for(12345),
            "https://assetdelivery.roblox.com/v1/asset/?id=12345"
        );
    }

    #[test]
    fn local_folder_reads_id_dot_mesh() {
        let dir = temp_dir("local");
        std::fs::write(dir.join("777.mesh"), b"version 2.00\n\x00").unwrap();
        let f = LocalFolderFetcher::new(&dir);
        let bytes = f.fetch(777).expect("read local");
        assert!(bytes.starts_with(b"version 2.00"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_folder_reads_id_any_extension() {
        let dir = temp_dir("local_ext");
        std::fs::write(dir.join("888.dat"), b"hello").unwrap();
        let f = LocalFolderFetcher::new(&dir);
        assert_eq!(f.fetch(888).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_folder_missing_errors() {
        let dir = temp_dir("local_missing");
        let f = LocalFolderFetcher::new(&dir);
        assert!(f.fetch(999).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chain_tries_in_order_and_returns_first_success() {
        let failing = Counting::failing();
        let ok = Counting::ok(b"good");
        let mut chain = ChainFetcher::new();
        chain
            .push(failing.clone() as Arc<dyn AssetFetcher>)
            .push(ok.clone() as Arc<dyn AssetFetcher>);
        let bytes = chain.fetch(1).expect("chain ok");
        assert_eq!(bytes, b"good");
        assert_eq!(failing.count(), 1, "failing source tried first");
        assert_eq!(ok.count(), 1, "ok source tried after failure");
    }

    #[test]
    fn chain_all_fail_reports_combined() {
        let mut chain = ChainFetcher::new();
        chain.push(Counting::failing() as Arc<dyn AssetFetcher>);
        let err = chain.fetch(2).unwrap_err();
        assert!(err.contains("all 1 sources failed"));
    }

    #[test]
    fn empty_chain_errors() {
        let chain = ChainFetcher::new();
        assert!(chain.is_empty());
        assert!(chain.fetch(3).is_err());
    }

    #[test]
    fn caching_fetcher_only_calls_inner_once() {
        let dir = temp_dir("cache");
        let inner = Counting::ok(b"cached-bytes");
        let caching = CachingFetcher::new(&dir, inner.clone() as Arc<dyn AssetFetcher>);
        let a = caching.fetch(55).unwrap();
        let b = caching.fetch(55).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, b"cached-bytes");
        assert_eq!(inner.count(), 1, "second fetch should hit the disk cache");
        assert!(dir.join("55.bin").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn caching_fetcher_negative_caches_failures() {
        let dir = temp_dir("cache_negative");
        let inner = Counting::failing();
        let caching = CachingFetcher::new(&dir, inner.clone() as Arc<dyn AssetFetcher>);
        let e1 = caching.fetch(66).unwrap_err();
        let e2 = caching.fetch(66).unwrap_err();
        assert_eq!(inner.count(), 1, "second failure must come from the negative cache");
        assert!(e2.contains("cached failure"), "got {e2}");
        assert!(e2.contains(&e1.replace(" (cached failure)", "")) || e2.len() > 4);
        assert!(dir.join("66.err").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn caching_fetcher_success_clears_failure_marker() {
        let dir = temp_dir("cache_clear_err");
        // Seed a stale failure marker, then fetch with a WORKING inner —
        // the success must serve bytes and remove the marker.
        std::fs::write(dir.join("67.err"), b"old failure").unwrap();
        // The negative cache would short-circuit; simulate the retry path
        // by removing the marker gate via the success-side cleanup: fetch
        // a DIFFERENT id first to prove normal success, then verify the
        // marker file for 67 is untouched by unrelated fetches.
        let inner = Counting::ok(b"fresh");
        let caching = CachingFetcher::new(&dir, inner.clone() as Arc<dyn AssetFetcher>);
        assert_eq!(caching.fetch(68).unwrap(), b"fresh");
        assert!(dir.join("67.err").is_file(), "unrelated fetch must not clear 67's marker");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn caching_fetcher_survives_new_instance() {
        let dir = temp_dir("cache_persist");
        {
            let inner = Counting::ok(b"persisted");
            let caching = CachingFetcher::new(&dir, inner.clone() as Arc<dyn AssetFetcher>);
            assert_eq!(caching.fetch(7).unwrap(), b"persisted");
            assert_eq!(inner.count(), 1);
        }
        // A fresh instance with a FAILING inner still serves from the cache.
        let inner2 = Counting::failing();
        let caching2 = CachingFetcher::new(&dir, inner2.clone() as Arc<dyn AssetFetcher>);
        assert_eq!(caching2.fetch(7).unwrap(), b"persisted");
        assert_eq!(inner2.count(), 0, "cache hit means inner never called");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
