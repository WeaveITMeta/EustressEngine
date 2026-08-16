//! # The sealed avatar runtime
//!
//! One runtime owns the play character. Both shells — the windowed Studio
//! editor and the standalone Client — compose it identically and differ only
//! through [`HostSeams`], an exhaustive `const fn` match.
//!
//! ## Why this exists
//!
//! The previous design (`SharedCharacterPlugin`) shared *systems* and left
//! four things free: entity construction, plugin composition, Bevy feature
//! selection, and environment. Its own module doc claimed it "ensures
//! identical gameplay behavior in both contexts." It produced 48 divergences
//! with 17 blockers, and four of its own systems were commented out of
//! registration while the doc comment still advertised them.
//!
//! Convention cannot fail loudly, so it failed silently. This module makes
//! divergence *unrepresentable* rather than discouraged:
//!
//! * **Sealed construction.** [`SpawnedByAvatarRuntime`] holds a private
//!   field, so no crate outside `eustress-common` can build one. Every avatar
//!   system filters on it. The only public way to create a character is the
//!   [`SpawnAvatar`] message, which carries a descriptor — never a mesh path
//!   and never a sex. That signature is what let Studio pass Female and the
//!   Client pass Male into the same function.
//! * **Private sub-plugins.** The character/animation sub-plugins are
//!   `pub(crate)`. A host that tries to add one gets `E0603`.
//! * **Exhaustive seams.** Adding a field to [`HostSeams`] is a compile error
//!   in *both* arms of the match, so a new host difference cannot be
//!   introduced on one side only.
//! * **Boot assertions.** [`AvatarRuntimePlugin`] panics at startup with the
//!   remedy in the message if the asset sources or physics are missing —
//!   in shipped builds, not only under test.
//!
//! ## Ordering contract
//!
//! [`boot::register_avatar_asset_sources`] MUST be called before the shell
//! adds `AssetPlugin` (Bevy freezes the asset-source table at `AssetPlugin`
//! build time). The boot assertion in `build` catches the mistake, but it
//! catches it one frame too late to fix automatically.

use bevy::prelude::*;

pub mod boot;
pub mod rig;

/// The motion graph — retargeted clips blended by real locomotion.
#[cfg(all(feature = "physics", feature = "model-import"))]
pub mod anim;
#[cfg(feature = "physics")]
pub mod control;
/// Ledge detection and mantling.
#[cfg(feature = "physics")]
pub mod climb;
/// Grip detection — the "what can I grab in direction d" primitive the whole
/// traversal set is built on. Avian-dependent, so gated with its siblings.
#[cfg(feature = "physics")]
pub mod grip;
/// Two-bone IK, foot planting, ground adaptation.
#[cfg(feature = "physics")]
pub mod ik;
/// Landing response — soft, hard, and the roll.
#[cfg(feature = "physics")]
pub mod landing;
/// Breathing, exertion, idle breaks, landing flex, look-at.
#[cfg(feature = "physics")]
pub mod procedural;
#[cfg(feature = "physics")]
pub mod locomotion;
/// Behavioural tests that actually move the character through real geometry.
#[cfg(feature = "physics")]
pub mod behaviour;
#[cfg(feature = "physics")]
pub mod parity;
/// Clip retargeting onto the canonical bone space. Requires the `gltf` parser
/// (the `model-import` feature, on by default) to recover node hierarchy.
#[cfg(feature = "model-import")]
pub mod retarget;
#[cfg(feature = "physics")]
pub mod spawn;

pub use eustress_avatar_schema::{
    resolve, AvatarDescriptor, BaseBody, BodyMetrics, BodyMorphs, FaceShape, ItemId,
    MotionOverrides, Norm01, Palette, ResolvedMotion, SlotKind, Srgb8, GRAVITY_MPS2,
    MAX_HEIGHT_M, MIN_HEIGHT_M, NOMINAL_BIND_HEIGHT_M,
};

// ─────────────────────────────────────────────────────────────────────────────
// The sealed token
// ─────────────────────────────────────────────────────────────────────────────

/// Proof that an entity was built by this runtime.
///
/// The private tuple field is the entire mechanism: `SpawnedByAvatarRuntime`
/// cannot be named-and-constructed from `eustress-engine` or
/// `eustress-client`, so no host can hand-roll a character that the avatar
/// systems would then partially drive. That was the shape of the old
/// Studio-ghost-vs-Client-capsule divergence, where each shell appended its
/// own `.insert()` tail to a shared spawn call.
#[derive(Component, Debug)]
pub struct SpawnedByAvatarRuntime(pub(crate) ());

/// Marks the avatar the local human is controlling. Networked/remote avatars
/// carry [`SpawnedByAvatarRuntime`] without this.
#[derive(Component, Debug, Default)]
pub struct LocalAvatar;

/// Who is driving this avatar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvatarControl {
    /// Local keyboard/mouse.
    #[default]
    LocalPlayer,
    /// Script, network replica, or AI. Input systems skip it.
    Remote,
}

/// The ONLY public way to create a character.
///
/// Carries a descriptor, never a model or a sex — so the crossed
/// `BiologicalSex::character_model` (Female → XBot) and
/// `SkinnedCharacter::new` (XBot → Male) pairing has nowhere to live.
#[derive(Message, Debug, Clone)]
pub struct SpawnAvatar {
    descriptor: AvatarDescriptor,
    at: Vec3,
    yaw: f32,
    control: AvatarControl,
}

impl SpawnAvatar {
    pub fn new(descriptor: AvatarDescriptor, at: Vec3) -> Self {
        Self { descriptor, at, yaw: 0.0, control: AvatarControl::LocalPlayer }
    }
    pub fn with_yaw(mut self, yaw: f32) -> Self {
        self.yaw = yaw;
        self
    }
    pub fn with_control(mut self, control: AvatarControl) -> Self {
        self.control = control;
        self
    }
    pub fn descriptor(&self) -> &AvatarDescriptor {
        &self.descriptor
    }
    pub fn at(&self) -> Vec3 {
        self.at
    }
    pub fn yaw(&self) -> f32 {
        self.yaw
    }
    pub fn control(&self) -> AvatarControl {
        self.control
    }
}

/// Request removal of every avatar this runtime owns. Studio's Stop path uses
/// this instead of hand-despawning by marker, so cleanup cannot drift.
#[derive(Message, Debug, Clone, Default)]
pub struct DespawnAllAvatars;

// ─────────────────────────────────────────────────────────────────────────────
// Host seams
// ─────────────────────────────────────────────────────────────────────────────

/// Which shell is hosting the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarHost {
    /// The windowed Studio editor. The avatar shares a process with the
    /// editor camera, editor keybindings, and a Slint overlay.
    Studio,
    /// The standalone Client. The avatar owns the window.
    Client,
}

/// Every legitimate difference between the two shells, in one exhaustive
/// match.
///
/// Studio genuinely needs viewport-focus gating and editor-camera
/// suppression; a design that only deletes host-specific code has nowhere to
/// put them, and they reappear as ungated host code. Adding a field here is a
/// compile error in both arms — which is the point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostSeams {
    /// Ignore gameplay input unless the 3D viewport has focus. Studio only —
    /// the Client's window *is* the viewport.
    pub gate_input_on_viewport_focus: bool,
    /// Deactivate the editor fly-camera while an avatar is live.
    pub suppress_editor_camera: bool,
    /// Suppress editor keyboard shortcuts (Delete, F, 1/2/3, Ctrl+Shift+S)
    /// while an avatar is live, so gameplay keys cannot mutate the scene.
    pub gate_editor_shortcuts: bool,
    /// Render order for the avatar camera. Studio must out-rank the editor
    /// camera without touching the Slint overlay at order 300.
    pub camera_order: isize,
    /// What Escape does. This key was previously bound in three places with
    /// opposite effects and no ordering constraint.
    pub escape_action: EscapeAction,
}

/// The single owner of the Escape key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeAction {
    /// Leave play mode entirely (Studio's Stop).
    StopPlay,
    /// Open the in-game pause menu (Client).
    PauseMenu,
}

impl AvatarHost {
    pub const fn seams(self) -> HostSeams {
        match self {
            AvatarHost::Studio => HostSeams {
                gate_input_on_viewport_focus: true,
                suppress_editor_camera: true,
                gate_editor_shortcuts: true,
                camera_order: 10,
                escape_action: EscapeAction::StopPlay,
            },
            AvatarHost::Client => HostSeams {
                gate_input_on_viewport_focus: false,
                suppress_editor_camera: false,
                gate_editor_shortcuts: false,
                camera_order: 0,
                escape_action: EscapeAction::PauseMenu,
            },
        }
    }
}

/// Resource form of the seams, inserted by the plugin.
#[derive(Resource, Debug, Clone, Copy)]
pub struct AvatarHostConfig {
    pub host: AvatarHost,
    pub seams: HostSeams,
}

// ─────────────────────────────────────────────────────────────────────────────
// System sets
// ─────────────────────────────────────────────────────────────────────────────

/// Ordered phases of one avatar frame.
///
/// `PostAnim` is the critical one: every procedural bone write must land
/// `.after(bevy::animation::AnimationSystems)` and
/// `.before(TransformSystems::Propagate)`. The old code put
/// `apply_foot_ik_to_bones` and `extract_root_motion` in bare `PostUpdate`
/// with no constraint, so their writes raced `animate_targets` and were
/// silently stomped. That failure mode looks like "IK does nothing" with no
/// error anywhere.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AvatarSystems {
    /// Consume `SpawnAvatar` / `DespawnAllAvatars`.
    Lifecycle,
    /// Sample input into `MovementIntent`.
    Input,
    /// Ground probe, move-and-slide, and the single `LocomotionController`
    /// producer.
    Locomotion,
    /// Bind skeletons, retarget clips, drive the animation graph.
    Animation,
    /// Procedural bone writes. Strictly after Bevy's animation sampling.
    PostAnim,
}

// ─────────────────────────────────────────────────────────────────────────────
// The plugin
// ─────────────────────────────────────────────────────────────────────────────

/// The one plugin a host adds. Everything else in this module is
/// `pub(crate)`.
pub struct AvatarRuntimePlugin {
    host: AvatarHost,
}

impl AvatarRuntimePlugin {
    pub fn new(host: AvatarHost) -> Self {
        Self { host }
    }
}

impl Plugin for AvatarRuntimePlugin {
    fn build(&self, app: &mut App) {
        // ── Gate 4: boot assertions, with the remedy in the message ────────
        //
        // These fire in shipped builds, not only under test. Findings 1/21/43
        // (the Client never registers `bundled://`, so every character GLB and
        // clip 404s and the player is invisible) were a silent asset failure
        // that looked like an animation bug for as long as they shipped.
        assert!(
            app.is_plugin_added::<bevy::asset::AssetPlugin>(),
            "AvatarRuntimePlugin requires AssetPlugin. Add DefaultPlugins (or \
             MinimalPlugins + AssetPlugin) before AvatarRuntimePlugin."
        );

        if !boot::avatar_asset_sources_registered() {
            panic!(
                "AvatarRuntimePlugin: avatar asset sources were never registered.\n\
                 \n\
                 Call `eustress_common::avatar::boot::register_avatar_asset_sources(&mut app)` \
                 BEFORE adding AssetPlugin/DefaultPlugins — Bevy freezes the asset-source table \
                 when AssetPlugin builds, so registering afterwards silently does nothing and \
                 every `bundled://` character asset 404s.\n"
            );
        }

        app.insert_resource(AvatarHostConfig { host: self.host, seams: self.host.seams() })
            .add_message::<SpawnAvatar>()
            .add_message::<DespawnAllAvatars>()
            .register_type::<rig::AvatarRig>()
            .register_type::<rig::HumanoidBone>()
            .init_resource::<rig::RigBindStats>();

        app.configure_sets(
            Update,
            (
                AvatarSystems::Lifecycle,
                AvatarSystems::Input,
                AvatarSystems::Locomotion,
                AvatarSystems::Animation,
            )
                .chain(),
        );

        // Procedural bone writes land in PostUpdate, strictly between Bevy's
        // animation sampling and transform propagation.
        // `AnimationSystems` is re-exported from `bevy_app`, not
        // `bevy_animation` (bevy_animation itself imports it from there).
        app.configure_sets(
            PostUpdate,
            AvatarSystems::PostAnim
                .after(bevy::app::AnimationSystems)
                .before(TransformSystems::Propagate),
        );

        app.add_plugins(rig::RigBindPlugin);

        #[cfg(feature = "physics")]
        {
            app.add_plugins((
                spawn::AvatarSpawnPlugin,
                locomotion::AvatarLocomotionPlugin,
                control::AvatarControlPlugin,
            ));
            // ── Procedural life layer: ENABLED ─────────────────────────────
            //
            // Its rotations were always correct — they compose local deltas
            // onto local rotations. Only two hips TRANSLATION writes were
            // wrong: metres written straight into armature-local space, which
            // on a 0.01-scale Mixamo armature is ~100x too large and launched
            // the character out of view. Both now divide by
            // `AvatarRig::armature_scale`.
            //
            // Limb IK is ON. It used to compose a WORLD-space rotation delta
            // straight onto a LOCAL bone rotation, which is only valid when
            // the parent's world rotation is identity — never true inside a
            // skeleton, so every solved bone was rotated about the wrong axis.
            // `ik::local_after_world_delta` now conjugates the delta through
            // the bone's own world rotation, which is what unblocked both foot
            // planting and the climb grip.
            app.add_plugins((
                procedural::AvatarLifePlugin,
                climb::AvatarClimbPlugin,
                landing::AvatarLandingPlugin,
                ik::AvatarIkPlugin,
            ));
            #[cfg(feature = "model-import")]
            app.add_plugins(anim::AvatarAnimPlugin);
            // Facing is integrated once, in PostAnim, so it composes with
            // animated bone rotations instead of racing them.
            app.add_systems(
                PostUpdate,
                control::face_movement_direction.in_set(AvatarSystems::PostAnim),
            );
        }

        #[cfg(not(feature = "physics"))]
        panic!(
            "AvatarRuntimePlugin was built without the `physics` feature.\n\
             An avatar with no collider cannot stand, walk, or jump — that is \
             blocker #7 and is not a supported configuration.\n\
             Add `features = [\"physics\"]` to the eustress-common dependency."
        );
    }
}
