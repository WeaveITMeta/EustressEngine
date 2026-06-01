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
pub mod value_objects;   // Wave 6.A — 11 ValueObject classes (String/Int/Number/Bool/Object/Color3/Vector3/CFrame/BrickColor/Ray/BinaryString)
pub mod interaction;     // Wave 6.D — Tool/Accessory/ClickDetector/ProximityPrompt/Dialog/DialogChoice/BodyColors/CharacterMesh/Shirt/Pants/ShirtGraphic
pub mod ui_layout;       // Wave 7.B — UICorner/UIGradient/UIStroke/UI*Layout/UI*Constraint/CanvasGroup/UIDragDetector
pub mod meshes;          // Wave 7.C — BlockMesh/FileMesh/Texture/SurfaceAppearance/MaterialVariant/Highlight/Bone/Wrap*
pub mod legacy_joints;   // Wave 7.A — Weld/Motor/VelocityMotor/NoCollisionConstraint/RigidConstraint/LineForce/AnimationConstraint
pub mod audio_dsp;       // Wave 7.E — Audio* DSP effects + legacy *SoundEffect
pub mod character7;      // Wave 7.D — Animation/*Controller/HumanoidDescription/Backpack/Accessory*/IKControl/Pose/...
pub mod data7;           // Wave 7.F — DataStore*Options/*Curve/Path2D/LocalizationTable/Configuration/Noise/Wire/...
pub mod editable7;       // Wave 7.G — EditableImage/RobloxEditableImage/BuoyancySensor/DragDetector/TextCh*/HapticEffect
