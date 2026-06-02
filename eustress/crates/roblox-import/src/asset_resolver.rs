//! Roblox asset reference resolution.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §11.
//!
//! Roblox property URIs come in three flavours:
//! - `rbxassetid://NNNNNNNNN` — by numeric ID, on Roblox's CDN.
//! - `rbxasset://path/to/file` — packaged with the Studio install.
//! - `http(s)://...` — a direct URL (deprecated but appears in old
//!   `.rbxl`s).
//!
//! Default behaviour: emit a placeholder local path
//! `assets/_unresolved/<scheme>/<id-or-path>` plus an
//! [`crate::import_report::AssetWarning`] per occurrence. No network.
//!
//! A future `AssetFetcher` integration (separate
//! `eustress-roblox-assets` crate per spec §19) can plug in a community
//! mirror; Wave 4.A.1 keeps the surface no-network.

use std::path::PathBuf;

/// Discriminated form of a Roblox asset URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetReference {
    /// `rbxassetid://NNNNNNNNN` — the modern asset-ID form.
    AssetId(u64),
    /// `rbxasset://path/to/file` — Studio-bundled.
    BundledPath(String),
    /// `http(s)://...` — direct URL.
    HttpUrl(String),
    /// `rbxhttp://...` — Roblox-side CDN routing.
    RbxHttp(String),
    /// Some other URI scheme we don't recognise (`rbxgameasset://`,
    /// `rbxthumb://`, …). Preserved verbatim for the report.
    Other(String),
    /// Plain string that didn't match any known scheme — likely an
    /// authored local path or empty.
    Plain(String),
}

impl AssetReference {
    /// Classify a raw asset URI.
    pub fn parse(raw: &str) -> Self {
        if let Some(rest) = raw.strip_prefix("rbxassetid://") {
            if let Ok(n) = rest.parse::<u64>() {
                return AssetReference::AssetId(n);
            }
            return AssetReference::Other(raw.to_string());
        }
        if let Some(rest) = raw.strip_prefix("rbxasset://") {
            return AssetReference::BundledPath(rest.to_string());
        }
        if let Some(rest) = raw.strip_prefix("rbxhttp://") {
            return AssetReference::RbxHttp(rest.to_string());
        }
        if raw.starts_with("https://") || raw.starts_with("http://") {
            return AssetReference::HttpUrl(raw.to_string());
        }
        // Other URI schemes (rbxgameasset://, rbxthumb://) — preserve.
        if raw.contains("://") {
            return AssetReference::Other(raw.to_string());
        }
        AssetReference::Plain(raw.to_string())
    }

    /// Scheme tag used by the placeholder path + the asset warning.
    fn scheme_tag(&self) -> &'static str {
        match self {
            AssetReference::AssetId(_) => "rbxassetid",
            AssetReference::BundledPath(_) => "rbxasset",
            AssetReference::HttpUrl(_) => "http",
            AssetReference::RbxHttp(_) => "rbxhttp",
            AssetReference::Other(_) => "other",
            AssetReference::Plain(_) => "plain",
        }
    }
}

/// Outcome of an asset-resolution call.
#[derive(Debug, Clone)]
pub struct ResolvedAsset {
    /// Universe-relative path the writer should drop into the TOML
    /// (e.g. `[asset].mesh` or `[asset].path`). Always a relative path.
    pub asset_path: PathBuf,
    /// Whether the underlying bytes are present on disk (true), or this
    /// is a placeholder (false).
    pub resolved: bool,
    /// When `resolved == false`, the human-readable reason — surfaced
    /// into `ImportReport::asset_warnings.reason`.
    pub reason: Option<String>,
    /// The original URI for cross-reference.
    pub original_uri: String,
}

/// Resolve a Roblox asset URI to a local path.
///
/// Wave 4.A.1 default: no network. Every URI lands at
/// `assets/_unresolved/<scheme>/<id-or-path>` and the caller emits an
/// [`crate::import_report::AssetWarning`] holding the `reason`.
///
/// A future integration can replace this with a network-aware
/// implementation; the signature accepts a placeholder for that
/// future-state without committing to a fetcher trait in this
/// task's surface.
pub fn resolve(raw_uri: &str) -> ResolvedAsset {
    let parsed = AssetReference::parse(raw_uri);
    let scheme = parsed.scheme_tag();
    let leaf = match &parsed {
        AssetReference::AssetId(n) => format!("{}.bin", n),
        AssetReference::BundledPath(p) => sanitise_uri_component(p),
        AssetReference::HttpUrl(u) => sanitise_uri_component(u),
        AssetReference::RbxHttp(u) => sanitise_uri_component(u),
        AssetReference::Other(u) => sanitise_uri_component(u),
        AssetReference::Plain(p) => sanitise_uri_component(p),
    };

    let asset_path = PathBuf::from("assets")
        .join("_unresolved")
        .join(scheme)
        .join(leaf);

    let reason = match &parsed {
        AssetReference::AssetId(n) => Some(format!(
            "rbxassetid://{} not fetched — no AssetFetcher configured",
            n
        )),
        AssetReference::BundledPath(p) => Some(format!(
            "rbxasset://{} not resolved — Studio install lookup deferred",
            p
        )),
        AssetReference::HttpUrl(u) => {
            Some(format!("{} not fetched — no AssetFetcher configured", u))
        }
        AssetReference::RbxHttp(u) => Some(format!(
            "rbxhttp://{} not fetched — no AssetFetcher configured",
            u
        )),
        AssetReference::Other(u) => Some(format!("unknown asset URI scheme: {}", u)),
        AssetReference::Plain(p) => {
            // Plain strings might be authored local paths. We still
            // route them to the placeholder so the TOML has something
            // pointable to; the warning is informational.
            Some(format!(
                "plain path '{}' kept as placeholder — no AssetFetcher integration",
                p
            ))
        }
    };

    ResolvedAsset {
        asset_path,
        resolved: false,
        reason,
        original_uri: raw_uri.to_string(),
    }
}

/// Replace path-unsafe characters in a URI component so it can live
/// inside `assets/_unresolved/<scheme>/<leaf>` without traversal issues.
fn sanitise_uri_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            ':' | '/' | '\\' | '?' | '#' | '*' | '|' | '<' | '>' | '"' => out.push('_'),
            _ => out.push(c),
        }
    }
    if out.is_empty() {
        "empty".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rbxassetid_uri() {
        let r = AssetReference::parse("rbxassetid://1234567890");
        assert_eq!(r, AssetReference::AssetId(1234567890));
    }

    #[test]
    fn parses_rbxasset_uri() {
        let r = AssetReference::parse("rbxasset://textures/grass.png");
        assert_eq!(
            r,
            AssetReference::BundledPath("textures/grass.png".to_string())
        );
    }

    #[test]
    fn parses_http_uri() {
        let r = AssetReference::parse("https://example.com/grass.png");
        assert_eq!(
            r,
            AssetReference::HttpUrl("https://example.com/grass.png".to_string())
        );
    }

    #[test]
    fn parses_other_uri_scheme() {
        let r = AssetReference::parse("rbxgameasset://Avatar1.rbxm");
        assert!(matches!(r, AssetReference::Other(_)));
    }

    #[test]
    fn parses_plain_string() {
        let r = AssetReference::parse("foo/bar.png");
        assert_eq!(r, AssetReference::Plain("foo/bar.png".to_string()));
    }

    #[test]
    fn rbxassetid_resolves_to_placeholder() {
        let out = resolve("rbxassetid://42");
        assert!(!out.resolved);
        assert!(out.reason.is_some());
        assert_eq!(
            out.asset_path,
            PathBuf::from("assets/_unresolved/rbxassetid/42.bin")
        );
        assert_eq!(out.original_uri, "rbxassetid://42");
    }

    #[test]
    fn rbxasset_resolves_to_placeholder() {
        let out = resolve("rbxasset://textures/grass.png");
        assert!(!out.resolved);
        // sanitiser replaces the slashes with underscores.
        assert!(out
            .asset_path
            .to_string_lossy()
            .ends_with("textures_grass.png"));
    }

    #[test]
    fn http_url_routes_to_placeholder() {
        let out = resolve("http://example.com/foo.png");
        assert!(!out.resolved);
        assert!(out.reason.unwrap().contains("AssetFetcher"));
    }

    #[test]
    fn unresolved_paths_live_under_assets_unresolved() {
        let out = resolve("rbxassetid://123");
        let s = out.asset_path.to_string_lossy().to_string();
        assert!(
            s.starts_with("assets") && s.contains("_unresolved"),
            "got {:?}",
            s
        );
    }

    #[test]
    fn malformed_rbxassetid_becomes_other() {
        let out = AssetReference::parse("rbxassetid://not-numeric");
        assert!(matches!(out, AssetReference::Other(_)));
    }
}
