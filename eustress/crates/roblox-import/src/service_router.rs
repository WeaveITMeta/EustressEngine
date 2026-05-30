//! Roblox service → Eustress space-relative folder routing.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §5.
//!
//! The DataModel's children are services (`Workspace`, `Lighting`,
//! `Players`, …). Each gets a destination folder under `<space_root>/`;
//! everything inside the Roblox service tree lands inside the
//! corresponding Eustress service folder.
//!
//! Three categories:
//! 1. **Direct cognate** — same name, same concept (`Workspace`,
//!    `Lighting`, `Players`, every storage container).
//! 2. **Runtime-only** — no on-disk children in source files
//!    (`Debris`, `RunService`, `HttpService`, …). The walker skips
//!    their subtrees silently.
//! 3. **`_imported/<ServiceName>/`** — Roblox services with no
//!    Eustress cognate (`MarketplaceService`, `TeleportService`, …)
//!    plus everything unrecognised. Their contents land under
//!    `_imported/` for the user to triage.
//!
//! A separate **deny-list** protects Eustress-only folders
//! (`SoulService/`, `AdornmentService/`, `_retired_layers/`); the
//! materializer must never produce paths inside them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::ImportError;

/// Resolution outcome for a Roblox service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOutcome {
    /// The service has a destination folder; recurse into it. Path is
    /// relative to `space_root` (joined by the materializer).
    Routed {
        /// Path relative to `space_root` (e.g. `Workspace`,
        /// `_imported/MarketplaceService`).
        dest: PathBuf,
        /// True when the destination is a Roblox-cognate folder; false
        /// for `_imported/...` placeholders. The materializer uses this
        /// to decide whether to warn.
        cognate: bool,
    },
    /// The service is a runtime-only container with no on-disk children
    /// in `.rbxl` saves — skip its subtree silently (no warning).
    SkipSilent,
}

/// The Eustress-only folder names that the importer must never touch.
/// Hardcoded — these are the project's invariant.
const DENY_LIST: &[&str] = &[
    "SoulService",
    "AdornmentService",
    "_retired_layers",
];

/// Roblox service → Eustress destination folder router.
///
/// Built once per import call with [`ServiceRouter::new`]. The router
/// is `Send + Sync` so it's safe to share across parallel materializers.
#[derive(Debug, Clone)]
pub struct ServiceRouter {
    space_root: PathBuf,
    deny: HashSet<&'static str>,
}

impl Default for ServiceRouter {
    /// A default-constructed router has an empty space root — useful for
    /// unit tests that only exercise the lookup logic. Production
    /// callers should use [`ServiceRouter::new`].
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

impl ServiceRouter {
    /// Construct a router rooted at `space_root` (the Eustress Space
    /// folder, e.g. `Universe1/Spaces/MyPlace/`).
    pub fn new(space_root: PathBuf) -> Self {
        Self {
            space_root,
            deny: DENY_LIST.iter().copied().collect(),
        }
    }

    /// Where does this Roblox service's content land on disk?
    ///
    /// Returns a [`RouteOutcome`] describing the routing decision. Hard
    /// errors only fire for the deny-listed names (`SoulService`, …) —
    /// every other input yields a `Routed` or `SkipSilent` outcome.
    pub fn route(&self, roblox_service: &str) -> Result<RouteOutcome, ImportError> {
        // Deny-list check first — these are the Eustress-only folders.
        if self.deny.contains(roblox_service) {
            return Err(ImportError::ServiceRouter {
                service: roblox_service.to_string(),
                reason: format!(
                    "{} is a Eustress-only folder; the importer is not allowed to write here",
                    roblox_service
                ),
            });
        }

        let outcome = match roblox_service {
            // ─── Direct cognates ────────────────────────────────────
            "Workspace" => routed("Workspace"),
            "Lighting" => routed("Lighting"),
            "Players" => routed("Players"),
            "StarterGui" => routed("StarterGui"),
            "StarterPack" => routed("StarterPack"),
            "StarterPlayerScripts" => routed("StarterPlayerScripts"),
            "StarterCharacterScripts" => routed("StarterCharacterScripts"),
            "ReplicatedStorage" => routed("ReplicatedStorage"),
            "ServerScriptService" => routed("ServerScriptService"),
            "ServerStorage" => routed("ServerStorage"),
            "SoundService" => routed("SoundService"),
            "Chat" => routed("Chat"),
            "Teams" => routed("Teams"),
            "MaterialService" => routed("MaterialService"),

            // ─── StarterPlayer subtree ─────────────────────────────
            //
            // The router routes the parent itself to
            // `StarterPlayerScripts/` (since that's the default storage
            // for properties on `StarterPlayer`); the walker treats
            // children named `StarterPlayerScripts` /
            // `StarterCharacterScripts` specially — see materializer.
            "StarterPlayer" => routed("StarterPlayerScripts"),

            // ─── ReplicatedFirst is collapsed ──────────────────────
            "ReplicatedFirst" => routed("ReplicatedStorage/_replicated_first"),

            // ─── Runtime-only services (no children in source files) ─
            "Debris"
            | "RunService"
            | "UserInputService"
            | "InputService"
            | "TweenService"
            | "HttpService"
            | "DataStoreService"
            | "PathfindingService"
            | "CollectionService"
            | "TextService"
            | "LocalizationService"
            | "GuiService"
            | "PhysicsService" => RouteOutcome::SkipSilent,

            // ─── No Eustress cognate → _imported/<ServiceName>/ ────
            //
            // The user can decide what to do with the children of
            // these services after the import lands.
            "MarketplaceService"
            | "TeleportService"
            | "BadgeService"
            | "GroupService"
            | "NotificationService" => routed_imported(roblox_service),

            // ─── Anything else → _imported/<ServiceName>/ ───────────
            other => routed_imported(other),
        };

        Ok(outcome)
    }

    /// Does `path` (absolute) point inside any of the Eustress-only
    /// folders? Used by the materializer as a defence-in-depth check
    /// before any disk write.
    pub fn is_off_limits(&self, path: &Path) -> bool {
        for component in path.components() {
            if let Some(s) = component.as_os_str().to_str() {
                if self.deny.contains(s) {
                    return true;
                }
            }
        }
        false
    }

    /// The space root the router was built with.
    pub fn space_root(&self) -> &Path {
        &self.space_root
    }

    /// Join the router's space root with a relative destination from
    /// a [`RouteOutcome::Routed`]. Returns the absolute on-disk path.
    pub fn absolute(&self, dest: &Path) -> PathBuf {
        self.space_root.join(dest)
    }
}

fn routed(rel: &str) -> RouteOutcome {
    RouteOutcome::Routed {
        dest: PathBuf::from(rel),
        cognate: true,
    }
}

fn routed_imported(name: &str) -> RouteOutcome {
    let mut p = PathBuf::from("_imported");
    p.push(name);
    RouteOutcome::Routed {
        dest: p,
        cognate: false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> ServiceRouter {
        ServiceRouter::new(PathBuf::from("/space"))
    }

    #[test]
    fn workspace_routes_directly() {
        let out = r().route("Workspace").unwrap();
        match out {
            RouteOutcome::Routed { dest, cognate } => {
                assert_eq!(dest, PathBuf::from("Workspace"));
                assert!(cognate);
            }
            _ => panic!("expected Routed"),
        }
    }

    #[test]
    fn lighting_routes_directly() {
        let out = r().route("Lighting").unwrap();
        assert!(matches!(out, RouteOutcome::Routed { ref dest, cognate: true } if dest == &PathBuf::from("Lighting")));
    }

    #[test]
    fn all_direct_cognates_route() {
        let services = [
            "Workspace",
            "Lighting",
            "Players",
            "StarterGui",
            "StarterPack",
            "StarterPlayerScripts",
            "StarterCharacterScripts",
            "ReplicatedStorage",
            "ServerScriptService",
            "ServerStorage",
            "SoundService",
            "Chat",
            "Teams",
            "MaterialService",
        ];
        let router = r();
        for s in services {
            let out = router.route(s).unwrap();
            match out {
                RouteOutcome::Routed { cognate, .. } => {
                    assert!(cognate, "{} should be cognate", s);
                }
                _ => panic!("{} did not route", s),
            }
        }
    }

    #[test]
    fn starter_player_routes_to_scripts() {
        let out = r().route("StarterPlayer").unwrap();
        match out {
            RouteOutcome::Routed { dest, cognate: true } => {
                assert_eq!(dest, PathBuf::from("StarterPlayerScripts"));
            }
            _ => panic!("expected Routed cognate"),
        }
    }

    #[test]
    fn replicated_first_collapses() {
        let out = r().route("ReplicatedFirst").unwrap();
        match out {
            RouteOutcome::Routed { dest, cognate: true } => {
                assert_eq!(
                    dest,
                    PathBuf::from("ReplicatedStorage/_replicated_first")
                );
            }
            _ => panic!("expected Routed cognate"),
        }
    }

    #[test]
    fn runtime_only_services_skip_silently() {
        let services = [
            "Debris",
            "RunService",
            "UserInputService",
            "TweenService",
            "HttpService",
            "PathfindingService",
            "CollectionService",
            "TextService",
            "LocalizationService",
            "GuiService",
            "PhysicsService",
        ];
        let router = r();
        for s in services {
            assert_eq!(router.route(s).unwrap(), RouteOutcome::SkipSilent, "{}", s);
        }
    }

    #[test]
    fn no_cognate_services_route_to_imported() {
        let services = [
            "MarketplaceService",
            "TeleportService",
            "BadgeService",
            "GroupService",
            "NotificationService",
        ];
        let router = r();
        for s in services {
            let out = router.route(s).unwrap();
            match out {
                RouteOutcome::Routed { dest, cognate: false } => {
                    assert_eq!(dest, PathBuf::from(format!("_imported/{}", s)));
                }
                _ => panic!("{} should route to _imported", s),
            }
        }
    }

    #[test]
    fn unknown_service_routes_to_imported() {
        let out = r().route("MadeUpService").unwrap();
        match out {
            RouteOutcome::Routed { dest, cognate: false } => {
                assert_eq!(dest, PathBuf::from("_imported/MadeUpService"));
            }
            _ => panic!("expected Routed (non-cognate)"),
        }
    }

    #[test]
    fn deny_list_returns_error() {
        for name in DENY_LIST {
            let err = r().route(name).unwrap_err();
            match err {
                ImportError::ServiceRouter { service, .. } => {
                    assert_eq!(&service, name);
                }
                _ => panic!("{} should produce ServiceRouter error", name),
            }
        }
    }

    #[test]
    fn is_off_limits_catches_eustress_only_paths() {
        let router = r();
        assert!(router.is_off_limits(Path::new("/space/SoulService/foo/_instance.toml")));
        assert!(router.is_off_limits(Path::new("/space/AdornmentService/Handles")));
        assert!(router.is_off_limits(Path::new("/space/_retired_layers/old")));
        assert!(!router.is_off_limits(Path::new("/space/Workspace/Part/_instance.toml")));
    }

    #[test]
    fn absolute_joins_with_space_root() {
        let router = r();
        let abs = router.absolute(Path::new("Workspace/Tower"));
        assert_eq!(abs, PathBuf::from("/space/Workspace/Tower"));
    }
}
