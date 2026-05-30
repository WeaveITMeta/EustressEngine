//! Per-ClassName ClassSpawner implementations (Wave 3 fan-out).
//!
//! Each subdirectory is one logical group of related classes and
//! exposes its own self-contained Bevy plugin that registers its
//! spawners with the `ClassRegistry` resource at plugin-build time.
//!
//! ## Group layout
//!
//! - `lights/`         — PointLight / SpotLight / SurfaceLight / DirectionalLight (Wave 3.A)
//! - `gui_containers/` — ScreenGui / BillboardGui / SurfaceGui / Frame / ScrollingFrame (Wave 3.B)
//! - `gui_leaves/`     — TextLabel / TextButton / TextBox / ImageLabel / ImageButton / ViewportFrame (Wave 3.C)
//! - `constraints/`    — Attachment + 8 Avian joint constraints (Wave 3.D)
//! - `containers/`     — Folder / Model (Wave 3.E)
//! - `audio_vfx/`      — Sound / ParticleEmitter / Beam (Wave 3.F)
//!
//! ## How registration works
//!
//! Each group exposes a `<Group>SpawnerPlugin` struct. The orchestrator
//! (Wave 3.G) wires them into `SlintUiPlugin::build` so they register
//! their spawners against the shared `ClassRegistry` resource that
//! `ClassRegistryPlugin` (Wave 2.3) already mounts.
//!
//! ## Why per-group sub-plugins (not one monolithic plugin)
//!
//! Parallel agent dispatch in Wave 3 needed disjoint file ownership —
//! 6 worktrees, 6 independent groups, zero merge conflicts. Each group
//! keeps its own registration logic. The trade-off (one extra
//! `add_plugins` line per group in `SlintUiPlugin::build`) is trivial
//! for a one-time wire.
//!
//! Spec: `docs/architecture/CLASS_REGISTRY.md` §6 (plugin pattern) + §8
//! (per-group module checklist).

pub mod animation;       // Wave 5.D — Animator, KeyframeSequence
pub mod audio_vfx;
pub mod constraints;
pub mod containers;
pub mod gui_containers;
pub mod gui_leaves;
pub mod lights;
pub mod networking;      // Wave 5.B — RemoteEvent/Function, BindableEvent/Function
pub mod scripting;       // Wave 5.C — SoulScript, Luau*, WorkshopConversation
