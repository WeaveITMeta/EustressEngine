//! Roblox asset reference resolution.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §11 / §19.3.
//!
//! Roblox property URIs come in three flavours:
//! - `rbxassetid://NNNNNNNNN` — by numeric ID, on Roblox's CDN.
//! - `rbxasset://path/to/file` — packaged with the Studio install.
//! - `http(s)://...` — a direct URL (deprecated but appears in old
//!   `.rbxl`s).
//!
//! ## Two resolution modes
//!
//! - **Placeholder (no network)** — [`resolve`] with no fetcher emits a
//!   placeholder local path `assets/_unresolved/<scheme>/<id-or-path>`
//!   plus an [`crate::import_report::AssetWarning`] per occurrence. This
//!   is the behaviour for every non-mesh asset (textures / sounds) in
//!   Wave F2 — those are deferred to a later wave.
//! - **Mesh fetch (Wave F2)** — when an [`AssetFetcher`] is supplied AND
//!   the reference is a *mesh* property, [`resolve`] fetches the asset
//!   bytes, and if they are a Roblox `.mesh` blob, decodes them through
//!   [`crate::roblox_mesh`] into a `.glb` written under
//!   `<space_root>/assets/meshes/rbx-<id>.glb`. The returned
//!   [`ResolvedAsset::asset_path`] is then relative to the instance
//!   folder (the same `../meshes/...` convention V-Cell parts use; the
//!   engine resolves it via the `space://` asset source).
//!
//! The fetcher itself (network + local mirror) lives in the separate
//! `eustress-roblox-assets` crate so this importer stays engine-free and
//! network-free.

use std::path::{Path, PathBuf};

/// A pluggable Roblox asset byte source.
///
/// Declared here (in the engine-free importer) so [`crate::ImportOptions`]
/// can carry one; the concrete network / local-folder / chained
/// implementations live in the `eustress-roblox-assets` crate (spec
/// §19.3) which depends on this crate for the trait. The importer never
/// performs I/O itself — it only calls [`AssetFetcher::fetch`] when a
/// fetcher is present.
///
/// Implementations MUST be `Send + Sync` (the importer may run on a
/// worker thread and shares the fetcher behind an `Arc`).
pub trait AssetFetcher: Send + Sync {
    /// Fetch the raw bytes of the asset with the given numeric Roblox
    /// asset id. Returns the bytes on success, or a human-readable error
    /// string on failure (surfaced into the import report's asset
    /// warnings; the caller keeps the placeholder).
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, String>;
}

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

    /// The numeric asset id, when this reference carries one. `http(s)://`
    /// URLs of the form `.../asset/?id=NNN` also yield an id so a fetcher
    /// can route them through the same id-keyed path.
    fn asset_id(&self) -> Option<u64> {
        match self {
            AssetReference::AssetId(n) => Some(*n),
            AssetReference::HttpUrl(u) | AssetReference::RbxHttp(u) => extract_id_from_url(u),
            _ => None,
        }
    }
}

/// Pull a numeric asset id out of a URL like
/// `https://assetdelivery.roblox.com/v1/asset/?id=1234` or a trailing
/// `/asset/1234`. Returns `None` when no id is present.
fn extract_id_from_url(url: &str) -> Option<u64> {
    // `?id=` / `&id=` query form.
    if let Some(idx) = url.find("id=") {
        let tail = &url[idx + 3..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>() {
            return Some(n);
        }
    }
    // Trailing numeric path segment.
    let last = url.rsplit('/').find(|s| !s.is_empty())?;
    let digits: String = last.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u64>().ok()
}

/// Outcome of an asset-resolution call.
#[derive(Debug, Clone)]
pub struct ResolvedAsset {
    /// Path the writer should drop into the TOML (e.g. `[asset].mesh` or
    /// `[asset].path`). When `resolved == true` this is RELATIVE TO THE
    /// INSTANCE FOLDER (so the engine's `space://` source resolves it via
    /// `toml_dir.join(..)`); when `resolved == false` it is the
    /// space-relative placeholder under `assets/_unresolved/`.
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

/// Resolve a Roblox asset URI to a path for the TOML.
///
/// - `raw_uri` — the Roblox property value (`rbxassetid://…`, `http…`, …).
/// - `fetcher` — optional byte source. When `None`, always returns a
///   placeholder (the no-network default).
/// - `space_root` — the destination Space root. Fetched meshes are written
///   under `<space_root>/assets/meshes/` so the engine's `space://` source
///   (registered at the Space root) can serve them.
/// - `is_mesh` — true when this reference is a mesh property
///   (`MeshId` / `SpecialMesh.Content` / …). Only mesh refs are fetched +
///   decoded in Wave F2; everything else takes the placeholder path so
///   textures / sounds defer to a later wave.
/// - `instance_dir` — the instance's on-disk folder. Used to compute the
///   relative `[asset].mesh` path from the instance to the written `.glb`.
///
/// On any failure (no fetcher, non-mesh ref, fetch error, non-`.mesh`
/// bytes, decode error, write error) the result is the unchanged
/// placeholder behaviour — the caller keeps rendering the placeholder
/// block and logs the warning.
pub fn resolve(
    raw_uri: &str,
    fetcher: Option<&dyn AssetFetcher>,
    space_root: &Path,
    is_mesh: bool,
    instance_dir: &Path,
) -> ResolvedAsset {
    let parsed = AssetReference::parse(raw_uri);

    // Mesh fetch path (Wave F2). Only attempted for mesh properties with a
    // fetcher present and a resolvable numeric asset id.
    if is_mesh {
        if let (Some(f), Some(id)) = (fetcher, parsed.asset_id()) {
            match fetch_and_decode_mesh(f, id, space_root, instance_dir) {
                Ok(rel) => {
                    return ResolvedAsset {
                        asset_path: rel,
                        resolved: true,
                        reason: None,
                        original_uri: raw_uri.to_string(),
                    };
                }
                Err(reason) => {
                    // Fall through to the placeholder, but carry the
                    // fetch/decode reason so the report explains it.
                    return placeholder(&parsed, raw_uri, Some(reason));
                }
            }
        }
    }

    placeholder(&parsed, raw_uri, None)
}

/// Fetch asset `id`, decode it as a Roblox `.mesh`, write a `.glb` under
/// `<space_root>/assets/meshes/rbx-<id>.glb`, and return the path RELATIVE
/// to `instance_dir` (the convention the engine's mesh loader resolves via
/// `toml_dir.join(..)` + the `space://` source). Returns `Err(reason)` on
/// any failure so the caller keeps the placeholder.
fn fetch_and_decode_mesh(
    fetcher: &dyn AssetFetcher,
    id: u64,
    space_root: &Path,
    instance_dir: &Path,
) -> Result<PathBuf, String> {
    let bytes = fetcher.fetch(id)?;

    // Only the Roblox `.mesh` magic is handled in Wave F2. A `.glb`/binary
    // model / texture flowing through a mesh property is out of scope —
    // bail so the placeholder survives.
    if !crate::roblox_mesh::looks_like_roblox_mesh(&bytes) {
        return Err(format!(
            "rbxassetid://{id} fetched ({} bytes) but is not a Roblox .mesh blob \
             (only .mesh decode is supported this wave)",
            bytes.len()
        ));
    }

    let mesh = crate::roblox_mesh::decode_mesh(&bytes)
        .map_err(|e| format!("rbxassetid://{id} .mesh decode failed: {e}"))?;
    if mesh.is_empty() {
        return Err(format!("rbxassetid://{id} .mesh decoded to an empty mesh"));
    }

    let meshes_dir = space_root.join("assets").join("meshes");
    std::fs::create_dir_all(&meshes_dir)
        .map_err(|e| format!("create {}: {e}", meshes_dir.display()))?;
    let glb_abs = meshes_dir.join(format!("rbx-{id}.glb"));
    crate::csg::write_glb(&glb_abs, &mesh)
        .map_err(|e| format!("write {}: {e}", glb_abs.display()))?;

    Ok(relative_path(instance_dir, &glb_abs))
}

/// Build the no-network placeholder result. `extra_reason` (when set)
/// replaces the generic reason with a fetch/decode-specific one.
fn placeholder(parsed: &AssetReference, raw_uri: &str, extra_reason: Option<String>) -> ResolvedAsset {
    let scheme = parsed.scheme_tag();
    let leaf = match parsed {
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

    let reason = extra_reason.or_else(|| match parsed {
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
        AssetReference::Plain(p) => Some(format!(
            "plain path '{}' kept as placeholder — no AssetFetcher integration",
            p
        )),
    });

    ResolvedAsset {
        asset_path,
        resolved: false,
        reason,
        original_uri: raw_uri.to_string(),
    }
}

/// Compute a path from `from_dir` to `to_file` using `..` segments — the
/// `../meshes/...` shape the engine's mesh loader resolves. Both inputs
/// should be absolute (or share a common root); when they have no common
/// prefix this returns `to_file` unchanged (still absolute, which the
/// loader's `strip_prefix(space_root)` fallback handles).
fn relative_path(from_dir: &Path, to_file: &Path) -> PathBuf {
    use std::path::Component;
    let from: Vec<Component> = from_dir.components().collect();
    let to: Vec<Component> = to_file.components().collect();

    // Longest common prefix.
    let mut i = 0;
    while i < from.len() && i < to.len() && from[i] == to[i] {
        i += 1;
    }
    // No shared root at all — can't build a sane relative path.
    if i == 0 {
        return to_file.to_path_buf();
    }

    let mut out = PathBuf::new();
    for _ in i..from.len() {
        out.push("..");
    }
    for comp in &to[i..] {
        out.push(comp.as_os_str());
    }
    out
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

    fn no_fetch(uri: &str) -> ResolvedAsset {
        resolve(uri, None, Path::new("/space"), false, Path::new("/space/Workspace/E"))
    }

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
    fn extracts_id_from_query_url() {
        assert_eq!(
            extract_id_from_url("https://assetdelivery.roblox.com/v1/asset/?id=987654"),
            Some(987654)
        );
    }

    #[test]
    fn extracts_id_from_path_url() {
        assert_eq!(extract_id_from_url("https://x.com/asset/12345"), Some(12345));
    }

    #[test]
    fn rbxassetid_resolves_to_placeholder_when_no_fetcher() {
        let out = no_fetch("rbxassetid://42");
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
        let out = no_fetch("rbxasset://textures/grass.png");
        assert!(!out.resolved);
        // sanitiser replaces the slashes with underscores.
        assert!(out
            .asset_path
            .to_string_lossy()
            .ends_with("textures_grass.png"));
    }

    #[test]
    fn http_url_routes_to_placeholder() {
        let out = no_fetch("http://example.com/foo.png");
        assert!(!out.resolved);
        assert!(out.reason.unwrap().contains("AssetFetcher"));
    }

    #[test]
    fn unresolved_paths_live_under_assets_unresolved() {
        let out = no_fetch("rbxassetid://123");
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

    #[test]
    fn relative_path_climbs_to_assets_meshes() {
        // Instance at <space>/Workspace/Group/Part, mesh at
        // <space>/assets/meshes/rbx-1.glb → ../../../assets/meshes/rbx-1.glb
        let rel = relative_path(
            Path::new("/space/Workspace/Group/Part"),
            Path::new("/space/assets/meshes/rbx-1.glb"),
        );
        assert_eq!(rel, PathBuf::from("../../../assets/meshes/rbx-1.glb"));
    }

    /// A mesh fetch with a fetcher returning a valid `.mesh` writes the glb
    /// and yields a resolved relative path. Uses a fake fetcher + the
    /// roblox_mesh v2 fixture so no network is touched.
    #[test]
    fn mesh_fetch_writes_glb_and_returns_relative_path() {
        struct Fake(Vec<u8>);
        impl AssetFetcher for Fake {
            fn fetch(&self, _id: u64) -> Result<Vec<u8>, String> {
                Ok(self.0.clone())
            }
        }
        let blob = crate::roblox_mesh::make_v2_triangle_fixture();
        let fake = Fake(blob);

        let space = std::env::temp_dir().join(format!(
            "rbx_resolve_mesh_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let inst_dir = space.join("Workspace").join("Group").join("Part");
        std::fs::create_dir_all(&inst_dir).unwrap();

        let out = resolve("rbxassetid://7", Some(&fake), &space, true, &inst_dir);
        assert!(out.resolved, "mesh should resolve: {:?}", out.reason);
        assert!(space.join("assets/meshes/rbx-7.glb").is_file());
        // Relative path climbs out of Workspace/Group/Part into assets/meshes.
        let rel = out.asset_path.to_string_lossy().replace('\\', "/");
        assert!(
            rel.ends_with("assets/meshes/rbx-7.glb") && rel.starts_with("../"),
            "got {rel}"
        );
        let _ = std::fs::remove_dir_all(&space);
    }

    /// A non-mesh property never fetches even with a fetcher present.
    #[test]
    fn non_mesh_property_keeps_placeholder() {
        struct Boom;
        impl AssetFetcher for Boom {
            fn fetch(&self, _id: u64) -> Result<Vec<u8>, String> {
                panic!("must not fetch for a non-mesh property");
            }
        }
        let out = resolve(
            "rbxassetid://42",
            Some(&Boom),
            Path::new("/space"),
            false,
            Path::new("/space/Workspace/E"),
        );
        assert!(!out.resolved);
    }
}
