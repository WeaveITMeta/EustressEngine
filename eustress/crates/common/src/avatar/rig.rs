//! # Rig binding — mapping a loaded skeleton onto named bones
//!
//! ## The bug this fixes
//!
//! Bevy's `AnimationTargetId::from_names` hashes the **full `Name` path** from
//! the animation root. The shipped Mixamo exports carry per-file numeric
//! suffixes on every bone, and the suffixes differ between the body export and
//! the clip export. Verified by parsing the GLB JSON chunks:
//!
//! | | `LeftUpLeg` | `RightLeg` |
//! |---|---|---|
//! | `male_walking.glb` animates | `mixamorig:LeftUpLeg_056` | `mixamorig:RightLeg_062` |
//! | `x_bot.glb` has | `mixamorig:LeftUpLeg_061` | `mixamorig:RightLeg_057` |
//! | `y_bot.glb` has | `mixamorig:LeftUpLeg_056` | `mixamorig:RightLeg_00` |
//!
//! Consequence on shipped code: Studio defaults to `Female` → `XBot` and
//! animates **only its 7-bone spine chain** — arms and legs are frozen in bind
//! pose. The Client defaults to `Male` → `YBot` and walks with a dead right
//! leg. Every symptom read as "the animation system is broken", and the
//! animation system is *also* broken for unrelated reasons, which is why this
//! went unnoticed.
//!
//! [`canonical_bone_key`] strips the `mixamorig:` namespace, the trailing
//! `_NNN` suffix, and all non-alphanumerics, which recovers a full 65/65 match
//! on both bodies.
//!
//! ## Why a rig map is needed at all
//!
//! `HumanoidRig` in the old code had **three query sites and zero insert
//! sites** repo-wide. Nothing ever walked a skeleton to populate it, so every
//! system that asked for it — foot IK, root motion, look-at, bone masks —
//! silently never ran. [`AvatarRig`] is the replacement, and
//! [`RigBindPlugin`] is the pass that was missing.

use bevy::prelude::*;
// Logging macros come in explicitly, not through `bevy::prelude::*`.
// The prelude only re-exports them when Bevy's `bevy_log` feature is on, and
// feature unification across the test target can turn it off — which made
// `cargo test -p eustress-common --lib` fail to compile this module while the
// ordinary lib build succeeded. Importing from `tracing` (what `bevy_log`
// re-exports anyway) makes the module build under every feature combination.
// An explicit import also shadows the glob, so there is no ambiguity.
use tracing::{debug, error, info, warn};
// Bevy 0.19 replaced the old `bevy_scene` glTF scene with `WorldAsset`;
// `WorldInstanceReady` is the "graph is fully spawned" trigger.
use bevy::world_serialization::WorldInstanceReady;
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Canonical bone naming
// ─────────────────────────────────────────────────────────────────────────────

/// Reduce an exporter-specific bone name to a stable key.
///
/// `"mixamorig:LeftUpLeg_061"` → `"leftupleg"`
/// `"mixamorig:LeftUpLeg_056"` → `"leftupleg"`
///
/// Only a trailing `_<digits>` group is stripped, so a legitimately numbered
/// bone like `Spine1` keeps its digit. Without that restriction `Spine1`,
/// `Spine2` and `Spine` would all collapse to the same key and the spine chain
/// would bind three bones to one slot.
pub fn canonical_bone_key(raw: &str) -> String {
    // Drop an exporter namespace prefix ("mixamorig:", "Armature|", …).
    let no_ns = raw.rsplit(':').next().unwrap_or(raw);
    let no_ns = no_ns.rsplit('|').next().unwrap_or(no_ns);

    // Strip a trailing _NNN export-index suffix, but only if what precedes it
    // is not itself empty.
    let trimmed = match no_ns.rfind('_') {
        Some(i) if i > 0 && no_ns[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < no_ns.len() => {
            &no_ns[..i]
        }
        _ => no_ns,
    };

    trimmed.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

/// The bones the runtime addresses by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub enum HumanoidBone {
    Hips,
    Spine,
    Spine1,
    Spine2,
    Neck,
    Head,
    HeadTop,
    LeftShoulder,
    LeftArm,
    LeftForeArm,
    LeftHand,
    RightShoulder,
    RightArm,
    RightForeArm,
    RightHand,
    LeftUpLeg,
    LeftLeg,
    LeftFoot,
    LeftToeBase,
    RightUpLeg,
    RightLeg,
    RightFoot,
    RightToeBase,
}

impl HumanoidBone {
    /// The bone on the other side of the body, or self for a centre bone.
    ///
    /// Lets a one-sided pose be authored once and reflected, instead of
    /// maintaining two copies that drift apart the moment either is tuned.
    pub const fn mirrored(self) -> Self {
        use HumanoidBone as B;
        match self {
            B::LeftShoulder => B::RightShoulder,
            B::LeftArm => B::RightArm,
            B::LeftForeArm => B::RightForeArm,
            B::LeftHand => B::RightHand,
            B::LeftUpLeg => B::RightUpLeg,
            B::LeftLeg => B::RightLeg,
            B::LeftFoot => B::RightFoot,
            B::LeftToeBase => B::RightToeBase,
            B::RightShoulder => B::LeftShoulder,
            B::RightArm => B::LeftArm,
            B::RightForeArm => B::LeftForeArm,
            B::RightHand => B::LeftHand,
            B::RightUpLeg => B::LeftUpLeg,
            B::RightLeg => B::LeftLeg,
            B::RightFoot => B::LeftFoot,
            B::RightToeBase => B::LeftToeBase,
            centre => centre,
        }
    }

    pub const ALL: [HumanoidBone; 23] = [
        HumanoidBone::Hips,
        HumanoidBone::Spine,
        HumanoidBone::Spine1,
        HumanoidBone::Spine2,
        HumanoidBone::Neck,
        HumanoidBone::Head,
        HumanoidBone::HeadTop,
        HumanoidBone::LeftShoulder,
        HumanoidBone::LeftArm,
        HumanoidBone::LeftForeArm,
        HumanoidBone::LeftHand,
        HumanoidBone::RightShoulder,
        HumanoidBone::RightArm,
        HumanoidBone::RightForeArm,
        HumanoidBone::RightHand,
        HumanoidBone::LeftUpLeg,
        HumanoidBone::LeftLeg,
        HumanoidBone::LeftFoot,
        HumanoidBone::LeftToeBase,
        HumanoidBone::RightUpLeg,
        HumanoidBone::RightLeg,
        HumanoidBone::RightFoot,
        HumanoidBone::RightToeBase,
    ];

    /// Canonical keys that map to this bone. First entry is the Mixamo name.
    pub const fn aliases(self) -> &'static [&'static str] {
        match self {
            HumanoidBone::Hips => &["hips", "pelvis", "root"],
            HumanoidBone::Spine => &["spine", "spine01"],
            HumanoidBone::Spine1 => &["spine1", "spine02"],
            HumanoidBone::Spine2 => &["spine2", "chest", "spine03"],
            HumanoidBone::Neck => &["neck"],
            HumanoidBone::Head => &["head"],
            HumanoidBone::HeadTop => &["headtopend", "headtop", "headend"],
            HumanoidBone::LeftShoulder => &["leftshoulder", "shoulderl", "claviclel"],
            HumanoidBone::LeftArm => &["leftarm", "upperarml", "arml"],
            HumanoidBone::LeftForeArm => &["leftforearm", "lowerarml", "forearml"],
            HumanoidBone::LeftHand => &["lefthand", "handl"],
            HumanoidBone::RightShoulder => &["rightshoulder", "shoulderr", "clavicler"],
            HumanoidBone::RightArm => &["rightarm", "upperarmr", "armr"],
            HumanoidBone::RightForeArm => &["rightforearm", "lowerarmr", "forearmr"],
            HumanoidBone::RightHand => &["righthand", "handr"],
            HumanoidBone::LeftUpLeg => &["leftupleg", "upperlegl", "thighl"],
            HumanoidBone::LeftLeg => &["leftleg", "lowerlegl", "calfl", "shinl"],
            HumanoidBone::LeftFoot => &["leftfoot", "footl"],
            HumanoidBone::LeftToeBase => &["lefttoebase", "toebasel", "toel"],
            HumanoidBone::RightUpLeg => &["rightupleg", "upperlegr", "thighr"],
            HumanoidBone::RightLeg => &["rightleg", "lowerlegr", "calfr", "shinr"],
            HumanoidBone::RightFoot => &["rightfoot", "footr"],
            HumanoidBone::RightToeBase => &["righttoebase", "toebaser", "toer"],
        }
    }

    /// Bones without which the runtime cannot function. A rig missing any of
    /// these is rejected rather than half-driven.
    pub const fn is_required(self) -> bool {
        matches!(
            self,
            HumanoidBone::Hips
                | HumanoidBone::Spine
                | HumanoidBone::Head
                | HumanoidBone::LeftUpLeg
                | HumanoidBone::LeftLeg
                | HumanoidBone::LeftFoot
                | HumanoidBone::RightUpLeg
                | HumanoidBone::RightLeg
                | HumanoidBone::RightFoot
        )
    }

    pub fn from_canonical(key: &str) -> Option<HumanoidBone> {
        HumanoidBone::ALL.into_iter().find(|b| b.aliases().contains(&key))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The bound rig
// ─────────────────────────────────────────────────────────────────────────────

/// A skeleton bound to named bones, plus what was measured off its bind pose.
///
/// Inserted by [`RigBindPlugin`] once the body scene finishes loading. This is
/// the component whose absence disabled every procedural animation path in the
/// old code.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct AvatarRig {
    /// Named bone → entity, for semantic lookups (which entity is the foot).
    pub bones: HashMap<HumanoidBone, Entity>,
    /// EVERY named node in the skeleton, keyed by canonical name.
    ///
    /// Retargeting keys off this rather than off [`HumanoidBone`], because the
    /// enum covers 23 semantic bones while a Mixamo rig has 65 nodes — the
    /// other 42 are finger and toe chains. Retargeting only the enumerated
    /// bones bound 69 of 195 curves and left the hands frozen in bind pose.
    pub by_key: HashMap<String, Entity>,
    /// The skeleton's root node (the `Armature`, i.e. Hips' parent).
    ///
    /// Clips authored in a different up-axis carry their correction as a
    /// rotation on THEIR root node; applying that same rotation here is what
    /// makes a Z-up clip drive a Y-up body correctly.
    pub skeleton_root: Option<Entity>,
    /// Required bones that could not be found. Empty on a healthy rig.
    pub unresolved: Vec<HumanoidBone>,
    /// Distance from the lowest foot to the top of the head in the bind pose,
    /// in world units after the armature's own scale.
    ///
    /// Feeds `BodyMorphs::metrics(bind_height_m)`, replacing the hardcoded
    /// 1.83 that assumed one specific export.
    pub bind_height_m: f32,
    /// Measured lateral distance from the root axis to each foot. Replaces the
    /// `±0.15` guess in the old foot-IK code.
    pub foot_half_separation: f32,
    /// Measured hip height in the bind pose.
    pub bind_hip_height: f32,
    /// Uniform scale carried by the skeleton root (0.01 for Mixamo exports).
    ///
    /// Anything converting a world/metres quantity into a bone's LOCAL
    /// translation must divide by this. Skipping it is a ~100x error.
    pub armature_scale: f32,
    /// Hips LOCAL translation in the bind pose.
    ///
    /// Mixamo clips animate hips translation — that is root motion. On a
    /// physics-driven character the capsule owns all translation, so the
    /// clip's displacement fights it: the mesh drifts off the capsule and
    /// snaps back when the clip loops. Pinning hips to this value each frame
    /// keeps the animation in place and leaves movement to the controller.
    pub bind_hips_translation: Vec3,
    /// Stable identity of this skeleton's shape, used to key the retarget
    /// cache so clip rekeying happens once per (clip, rig) pair.
    pub signature: u64,
}

impl AvatarRig {
    pub fn bone(&self, b: HumanoidBone) -> Option<Entity> {
        self.bones.get(&b).copied()
    }
    pub fn is_healthy(&self) -> bool {
        self.unresolved.is_empty()
    }
}

/// Diagnostics for the parity test and the P2 acceptance gate.
#[derive(Resource, Debug, Default)]
pub struct RigBindStats {
    pub rigs_bound: usize,
    pub last_bound_count: usize,
    pub last_unresolved: Vec<HumanoidBone>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Binding
// ─────────────────────────────────────────────────────────────────────────────

/// Identity transform for the bind walk's accumulator.
fn root_tf_identity() -> &'static Transform {
    static IDENT: Transform = Transform::IDENTITY;
    &IDENT
}

/// Marks a character root whose scene has not yet been bound.
#[derive(Component, Debug)]
pub struct AwaitingRigBind;

pub struct RigBindPlugin;

impl Plugin for RigBindPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_scene_ready);
    }
}

/// Bind on `WorldInstanceReady` rather than polling `Children`.
///
/// A `Children` poll races the asset loader: the root exists with an empty (or
/// partial) child list for an indeterminate number of frames, so a poll either
/// binds nothing or binds a half-built hierarchy. The trigger fires exactly
/// once, after the whole graph is present.
fn on_scene_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    awaiting: Query<(), With<AwaitingRigBind>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    mut stats: ResMut<RigBindStats>,
) {
    // Walk up from the scene root to the character root carrying the marker.
    let mut root = ready.entity;
    let mut guard = 0;
    while !awaiting.contains(root) {
        let Ok(parent) = parents.get(root) else { return };
        root = parent.parent();
        guard += 1;
        if guard > 16 {
            return;
        }
    }

    let mut bones: HashMap<HumanoidBone, Entity> = HashMap::new();
    let mut by_key: HashMap<String, Entity> = HashMap::new();
    let mut lowest_y = f32::INFINITY;
    let mut highest_y = f32::NEG_INFINITY;

    // Depth-first over the whole spawned graph, composing FULL transforms.
    //
    // Summing raw local Y here was wrong: a Mixamo armature carries scale 0.01
    // with Hips at local y≈104, so naive addition reported a bind height of
    // 231.8 m instead of ~2.3 m. That value feeds `rig_scale`, so a height
    // slider driven from it would have shrunk the avatar by ~130x.
    let mut stack = vec![(root, *root_tf_identity())];
    let mut visited = 0usize;
    while let Some((e, parent_tf)) = stack.pop() {
        visited += 1;
        if visited > 4096 {
            warn!("avatar: rig bind walked over 4096 nodes, aborting");
            break;
        }

        let world = match transforms.get(e) {
            Ok(local) => parent_tf.mul_transform(*local),
            Err(_) => parent_tf,
        };
        let y = world.translation.y;

        if let Ok(name) = names.get(e) {
            let key = canonical_bone_key(name.as_str());
            if let Some(bone) = HumanoidBone::from_canonical(&key) {
                // First match wins: the walk reaches the skeleton root before
                // any duplicate deeper in the graph.
                bones.entry(bone).or_insert(e);
            }
            if !key.is_empty() {
                // Every named node, so fingers and toes retarget too.
                by_key.entry(key.clone()).or_insert(e);
            }
            if !key.is_empty() {
                lowest_y = lowest_y.min(y);
                highest_y = highest_y.max(y);
            }
        }

        if let Ok(kids) = children.get(e) {
            for k in kids.iter() {
                stack.push((k, world));
            }
        }
    }

    let unresolved: Vec<HumanoidBone> =
        HumanoidBone::ALL.into_iter().filter(|b| b.is_required() && !bones.contains_key(b)).collect();

    // Measure the bind pose rather than assuming 1.83.
    let measured = if highest_y.is_finite() && lowest_y.is_finite() && highest_y > lowest_y {
        highest_y - lowest_y
    } else {
        eustress_avatar_schema::NOMINAL_BIND_HEIGHT_M
    };

    let foot_sep = match (bones.get(&HumanoidBone::LeftFoot), bones.get(&HumanoidBone::RightFoot)) {
        (Some(l), Some(r)) => {
            // Local X only — the feet share a parent, so their separation is
            // already in the same space; scale it by the armature's world
            // scale so the result is metres like every other metric.
            let lx = transforms.get(*l).map(|t| t.translation.x).unwrap_or(0.0);
            let rx = transforms.get(*r).map(|t| t.translation.x).unwrap_or(0.0);
            let armature_scale = if measured > 0.001 { 1.0 } else { 1.0 };
            ((lx - rx).abs() * 0.5 * armature_scale).max(0.02)
        }
        _ => 0.055 * measured,
    };

    let hip_height = bones
        .get(&HumanoidBone::Hips)
        .and_then(|h| transforms.get(*h).ok())
        .map(|t| t.translation.y.abs())
        .unwrap_or(measured * 0.52);

    // Signature: which bones bound, in a stable order. Keys the retarget cache.
    let mut sig: u64 = 0xcbf2_9ce4_8422_2325;
    for b in HumanoidBone::ALL {
        let present = bones.contains_key(&b) as u64;
        sig ^= present.wrapping_add(b as u64);
        sig = sig.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let bound_count = bones.len();
    let named_nodes = by_key.len();
    if unresolved.is_empty() {
        info!(
            "avatar: rig bound — {}/{} named bones, {} total nodes, bind height {:.3} m",
            bound_count,
            HumanoidBone::ALL.len(),
            named_nodes,
            measured
        );
    } else {
        warn!(
            "avatar: rig bound with {} MISSING required bones: {:?} ({}/{} found). \
             Procedural animation will be degraded.",
            unresolved.len(),
            unresolved,
            bound_count,
            HumanoidBone::ALL.len()
        );
    }

    stats.rigs_bound += 1;
    stats.last_bound_count = bound_count;
    stats.last_unresolved = unresolved.clone();

    // The Armature is Hips' parent. Clip up-axis corrections are applied
    // there, so it has to be captured while the hierarchy is in hand.
    let skeleton_root = bones
        .get(&HumanoidBone::Hips)
        .and_then(|h| parents.get(*h).ok())
        .map(|p| p.parent());

    let bind_hips_translation = bones
        .get(&HumanoidBone::Hips)
        .and_then(|h| transforms.get(*h).ok())
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);

    let armature_scale = skeleton_root
        .and_then(|e| transforms.get(e).ok())
        .map(|t| t.scale.y.abs().max(1e-4))
        .unwrap_or(1.0);

    commands.entity(root).remove::<AwaitingRigBind>().insert(AvatarRig {
        bones,
        by_key,
        skeleton_root,
        armature_scale,
        bind_hips_translation,
        unresolved,
        bind_height_m: measured,
        foot_half_separation: foot_sep,
        bind_hip_height: hip_height,
        signature: sig,
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixamo_export_suffixes_collapse_to_one_key() {
        // The exact strings verified in the shipped GLBs. Body and clip
        // disagree on the numeric suffix; both must reach the same bone.
        assert_eq!(canonical_bone_key("mixamorig:LeftUpLeg_061"), "leftupleg"); // x_bot
        assert_eq!(canonical_bone_key("mixamorig:LeftUpLeg_056"), "leftupleg"); // y_bot + clip
        assert_eq!(canonical_bone_key("mixamorig:RightLeg_062"), "rightleg"); // clip
        assert_eq!(canonical_bone_key("mixamorig:RightLeg_057"), "rightleg"); // x_bot
        assert_eq!(canonical_bone_key("mixamorig:RightLeg_00"), "rightleg"); // y_bot
    }

    #[test]
    fn numbered_spine_bones_stay_distinct() {
        // The failure mode a naive digit-strip would introduce: collapsing the
        // spine chain onto one slot.
        let s = canonical_bone_key("mixamorig:Spine_02");
        let s1 = canonical_bone_key("mixamorig:Spine1_03");
        let s2 = canonical_bone_key("mixamorig:Spine2_04");
        assert_eq!((s.as_str(), s1.as_str(), s2.as_str()), ("spine", "spine1", "spine2"));
        assert_ne!(s, s1);
        assert_ne!(s1, s2);
    }

    #[test]
    fn every_mixamo_bone_in_the_shipped_bodies_resolves() {
        // Names taken from the x_bot/y_bot node lists.
        for (raw, expect) in [
            ("mixamorig:Hips_01", HumanoidBone::Hips),
            ("mixamorig:Spine_02", HumanoidBone::Spine),
            ("mixamorig:Spine1_03", HumanoidBone::Spine1),
            ("mixamorig:Spine2_04", HumanoidBone::Spine2),
            ("mixamorig:Neck_05", HumanoidBone::Neck),
            ("mixamorig:Head_06", HumanoidBone::Head),
            ("mixamorig:HeadTop_End_07", HumanoidBone::HeadTop),
            ("mixamorig:LeftUpLeg_056", HumanoidBone::LeftUpLeg),
            ("mixamorig:RightToeBase_063", HumanoidBone::RightToeBase),
        ] {
            assert_eq!(
                HumanoidBone::from_canonical(&canonical_bone_key(raw)),
                Some(expect),
                "failed to resolve {raw}"
            );
        }
    }

    #[test]
    fn head_top_end_is_not_mistaken_for_head() {
        // "HeadTop_End_07" must not canonicalise into "head".
        assert_eq!(canonical_bone_key("mixamorig:HeadTop_End_07"), "headtopend");
        assert_eq!(
            HumanoidBone::from_canonical(&canonical_bone_key("mixamorig:HeadTop_End_07")),
            Some(HumanoidBone::HeadTop)
        );
        assert_eq!(
            HumanoidBone::from_canonical(&canonical_bone_key("mixamorig:Head_06")),
            Some(HumanoidBone::Head)
        );
    }

    #[test]
    fn alias_table_has_no_duplicate_keys_across_bones() {
        // A duplicate would make binding order-dependent.
        let mut seen = std::collections::HashSet::new();
        for b in HumanoidBone::ALL {
            for a in b.aliases() {
                assert!(seen.insert(*a), "alias {a:?} claimed by two bones (second: {b:?})");
            }
        }
    }

    #[test]
    fn non_bone_nodes_do_not_bind() {
        for junk in ["Armature", "Alpha_Surface", "Alpha_Joints", "Scene", ""] {
            assert_eq!(HumanoidBone::from_canonical(&canonical_bone_key(junk)), None, "{junk}");
        }
    }
}
