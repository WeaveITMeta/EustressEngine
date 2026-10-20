//! A published world, played alone: which API and which Gallery id, and the
//! chunks assembled into the world the rest of the Player already opens.
//!
//! The download itself is the browser's `fetch` (`fetch.rs`). Everything
//! here is pure, so it is tested without a browser.

use std::collections::HashMap;

use eustress_echk::{content_hash, is_content_hash, WorldManifest};
use eustress_networking::session::{assemble_world, DownloadedWorld};

/// Where a player stands when the world names no spawn. The desktop Player
/// uses the same point (`client` `space_fetch.rs`).
const DEFAULT_SPAWN: [f32; 3] = [0.0, 2.0, 8.0];

const APIS: [&str; 2] = [
    "https://api.eustress.dev",
    "https://api-staging.eustress.dev",
];

/// A Gallery id: the same shape `host.js` accepts, letters in either case.
pub fn is_sim_id(id: &str) -> bool {
    let n = id.len();
    (8..=64).contains(&n) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// The page's API, with no trailing slash. Only the two Eustress APIs.
pub fn api_origin(api_base: &str) -> Option<&str> {
    let api = api_base.trim().trim_end_matches('/');
    APIS.into_iter().find(|origin| *origin == api)
}

/// `GET {api}/api/simulations/{id}/world/manifest`
pub fn manifest_url(api_base: &str, sim_id: &str) -> Result<String, &'static str> {
    let api = api_origin(api_base).ok_or("bad-api")?;
    if !is_sim_id(sim_id) {
        return Err("bad-sim-id");
    }
    Ok(format!("{api}/api/simulations/{sim_id}/world/manifest"))
}

/// `GET {api}/api/simulations/{id}/world/chunks/{hash}`
pub fn chunk_url(api_base: &str, sim_id: &str, hash: &str) -> Result<String, &'static str> {
    let api = api_origin(api_base).ok_or("bad-api")?;
    if !is_sim_id(sim_id) || !is_content_hash(hash) {
        return Err("bad-chunk");
    }
    Ok(format!(
        "{api}/api/simulations/{sim_id}/world/chunks/{hash}"
    ))
}

/// What the page asked to open. A join link wins: a live session is the
/// world the host is running, not a second copy downloaded beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Join,
    Solo,
}

pub fn classify_launch(join: Option<&str>, sim_id: Option<&str>) -> Result<Launch, &'static str> {
    let join = join.map(str::trim).filter(|s| !s.is_empty());
    if join.is_some() {
        return Ok(Launch::Join);
    }
    match sim_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) if is_sim_id(id) => Ok(Launch::Solo),
        Some(_) => Err("bad-sim-id"),
        None => Err("no-world"),
    }
}

/// Decode a published world from its manifest and the chunk bytes, each
/// already checked against its content hash by the caller... no: this checks
/// the hashes itself, so a swapped chunk cannot open.
pub fn world_from_chunks(
    manifest_json: &[u8],
    chunks: &HashMap<String, Vec<u8>>,
) -> Result<DownloadedWorld, String> {
    let manifest: WorldManifest = serde_json::from_slice(manifest_json)
        .map_err(|_| "the world manifest could not be read".to_string())?;
    manifest
        .validate()
        .map_err(|e| format!("the world manifest was refused: {e}"))?;
    for hash in manifest.hashes() {
        let bytes = chunks
            .get(&hash)
            .ok_or_else(|| format!("chunk {hash} never arrived"))?;
        if content_hash(bytes) != hash {
            return Err(format!("chunk {hash} does not match its content hash"));
        }
    }
    assemble_world(
        &manifest,
        |hash| chunks.get(hash).map(|b| b.as_slice()),
        DEFAULT_SPAWN,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIM: &str = "07d084d8-9098-4192-bb48-84d2c428db36";
    const STAGING: &str = "https://api-staging.eustress.dev";

    #[test]
    fn a_gallery_id_is_hex_and_hyphens_only() {
        assert!(is_sim_id(SIM));
        assert!(is_sim_id("ABCDEF12"));
        for bad in [
            "",
            "short",
            "07d084d8/9098",
            "07d084d8 9098",
            "../etc/passwd",
            &"a".repeat(65),
        ] {
            assert!(!is_sim_id(bad), "{bad}");
        }
    }

    #[test]
    fn urls_stay_on_the_two_eustress_apis() {
        assert_eq!(
            manifest_url(STAGING, SIM).unwrap(),
            format!("{STAGING}/api/simulations/{SIM}/world/manifest")
        );
        let hash = "ab".repeat(32);
        assert_eq!(
            chunk_url(&format!("{STAGING}/"), SIM, &hash).unwrap(),
            format!("{STAGING}/api/simulations/{SIM}/world/chunks/{hash}")
        );
        assert!(manifest_url("https://evil.example", SIM).is_err());
        assert!(manifest_url("http://api-staging.eustress.dev", SIM).is_err());
        assert!(chunk_url(STAGING, SIM, "abcd").is_err());
        assert!(chunk_url(STAGING, "not an id", &hash).is_err());
    }

    #[test]
    fn a_join_link_wins_and_a_gallery_id_is_solo() {
        assert_eq!(
            classify_launch(Some("eustress-player://join/x"), Some(SIM)),
            Ok(Launch::Join)
        );
        assert_eq!(classify_launch(None, Some(SIM)), Ok(Launch::Solo));
        assert_eq!(classify_launch(Some("  "), Some(SIM)), Ok(Launch::Solo));
        assert_eq!(classify_launch(None, Some("not an id")), Err("bad-sim-id"));
        assert_eq!(classify_launch(None, None), Err("no-world"));
    }

    #[test]
    fn a_manifest_that_is_not_one_is_refused() {
        let err = match world_from_chunks(br#"{"format":"pak"}"#, &HashMap::new()) {
            Err(err) => err,
            Ok(_) => panic!("a non-manifest opened"),
        };
        assert!(
            err.contains("refused") || err.contains("could not be read"),
            "{err}"
        );
        assert!(world_from_chunks(b"not json", &HashMap::new()).is_err());
    }
}
