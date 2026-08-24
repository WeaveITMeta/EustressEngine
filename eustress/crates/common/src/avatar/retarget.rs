//! # Clip retargeting — making the limbs actually move
//!
//! ## The defect
//!
//! Bevy keys animation curves by `AnimationTargetId`, a hash of the bone's
//! **full name path**. The shipped Mixamo exports carry per-file numeric
//! suffixes, and the body export and the clip export disagree:
//!
//! | | `LeftUpLeg` | `RightLeg` |
//! |---|---|---|
//! | `male_walking.glb` animates | `…LeftUpLeg_056` | `…RightLeg_062` |
//! | `x_bot.glb` has | `…LeftUpLeg_061` | `…RightLeg_057` |
//! | `y_bot.glb` has | `…LeftUpLeg_056` | `…RightLeg_00` |
//!
//! Different name → different hash → the curve targets nothing. Studio (which
//! resolved to `x_bot`) animated **only its 7-bone spine chain**; the Client
//! (`y_bot`) walked with a dead right leg. Both looked like "the animation
//! system is broken", which it separately also was.
//!
//! ## The fix
//!
//! Bevy's own docs state the mechanism: *"Any animation is playable on any
//! armature as long as the bone names match."* So both sides are rewritten
//! into one canonical ID space:
//!
//! * the clip's curve keys are rebuilt under `canonical_target_id(bone)`;
//! * each bound bone entity's `AnimationTargetId` component is overwritten
//!   with the same value.
//!
//! Exporter suffixes then cannot matter, and any future humanoid glTF works
//! without re-authoring.
//!
//! ## Recovering which bone a curve belongs to
//!
//! An `AnimationTargetId` is a hash and cannot be inverted, so the source
//! glTF is re-parsed with the `gltf` crate to recover node names and
//! hierarchy. Bevy hashes the path from the *animation root*, and which node
//! that is depends on the exporter — so rather than assume, every ancestor
//! suffix is hashed and the one that matches a real curve key wins. That is
//! self-correcting: if Bevy changes its root convention, this still resolves.

use bevy::animation::{AnimatedBy, AnimationClip, AnimationTargetId, VariableCurve};
use bevy::prelude::*;
// Logging macros come in explicitly, not through `bevy::prelude::*`.
// The prelude only re-exports them when Bevy's `bevy_log` feature is on, and
// feature unification across the test target can turn it off — which made
// `cargo test -p eustress-common --lib` fail to compile this module while the
// ordinary lib build succeeded. Importing from `tracing` (what `bevy_log`
// re-exports anyway) makes the module build under every feature combination.
// An explicit import also shadows the glob, so there is no ambiguity.
use tracing::{info, warn};
use std::collections::HashMap;

use super::rig::{canonical_bone_key, AvatarRig, HumanoidBone};

/// The canonical ID space. A single path segment, so it depends only on which
/// bone this is — never on export suffixes or hierarchy depth.
pub fn canonical_target_id(bone: HumanoidBone) -> AnimationTargetId {
    // First alias is the canonical Mixamo-style key.
    canonical_target_id_for_key(bone.aliases()[0])
}

/// The same ID space keyed by an arbitrary canonical bone name.
///
/// Retargeting uses this rather than [`canonical_target_id`] so that EVERY
/// node carries through, not only the 23 semantic bones. A Mixamo rig has 65
/// nodes; keying off the enum bound 69 of 195 curves and left the hands
/// frozen in bind pose.
pub fn canonical_target_id_for_key(key: &str) -> AnimationTargetId {
    AnimationTargetId::from_iter(["eustress_rig", key])
}

/// Outcome of retargeting one clip onto the canonical space.
#[derive(Debug, Clone)]
pub struct RetargetReport {
    pub clip_file: String,
    /// Curves successfully rekeyed onto a known bone.
    pub bound_curves: usize,
    /// Curves whose source node did not map to a `HumanoidBone`.
    pub dropped_curves: usize,
    /// Distinct bones that received at least one curve.
    pub bones_bound: usize,
}

impl RetargetReport {
    /// The P2 acceptance signal. A healthy Mixamo clip binds ~19-22 bones;
    /// the pre-fix baseline on `x_bot` was 7.
    pub fn is_healthy(&self) -> bool {
        self.bones_bound >= 15
    }
}

/// Rewrite `clip` so its curves are keyed by canonical bone.
///
/// `glb_bytes` must be the source file the clip was loaded from — the parsed
/// `AnimationClip` no longer carries node names.
pub fn retarget_clip_in_place(
    clip: &mut AnimationClip,
    glb_bytes: &[u8],
    clip_file: &str,
) -> Result<RetargetReport, String> {
    let old_to_key = map_target_ids_to_bones(clip, glb_bytes)?;

    // Rebuild rather than mutate in place: two source bones could in principle
    // canonicalise onto one target, and merging must be explicit.
    let mut rebuilt: HashMap<AnimationTargetId, Vec<VariableCurve>> = HashMap::new();
    let mut bound_curves = 0usize;
    let mut dropped_curves = 0usize;

    for (old_id, curves) in clip.curves().iter() {
        match old_to_key.get(old_id) {
            Some(key) => {
                let new_id = canonical_target_id_for_key(key);
                let slot = rebuilt.entry(new_id).or_default();
                for c in curves {
                    slot.push(c.clone());
                    bound_curves += 1;
                }
            }
            None => dropped_curves += curves.len(),
        }
    }

    let bones_bound = rebuilt.len();
    let duration = clip.duration();

    // Replace the curve table wholesale.
    let curves = clip.curves_mut();
    curves.clear();
    for (id, list) in rebuilt {
        curves.insert(id, list);
    }
    clip.set_duration(duration);

    let report =
        RetargetReport { clip_file: clip_file.to_string(), bound_curves, dropped_curves, bones_bound };

    if report.is_healthy() {
        info!(
            "avatar: retargeted {} — {} curves onto {} bones ({} dropped), duration {:.3}s",
            clip_file, report.bound_curves, report.bones_bound, report.dropped_curves, duration
        );
    } else {
        warn!(
            "avatar: retarget of {} bound only {} bones ({} curves, {} dropped) — \
             limbs will not animate correctly",
            clip_file, report.bones_bound, report.bound_curves, report.dropped_curves
        );
    }

    Ok(report)
}

/// Recover `AnimationTargetId -> HumanoidBone` by re-parsing the source glTF.
fn map_target_ids_to_bones(
    clip: &AnimationClip,
    glb_bytes: &[u8],
) -> Result<HashMap<AnimationTargetId, String>, String> {
    let gltf = gltf::Gltf::from_slice(glb_bytes).map_err(|e| format!("parse glTF: {e}"))?;
    let doc = &gltf.document;

    let names: Vec<String> =
        doc.nodes().map(|n| n.name().unwrap_or_default().to_string()).collect();

    // Parent map, so an ancestor chain can be walked upward.
    let mut parent = vec![usize::MAX; names.len()];
    for node in doc.nodes() {
        for child in node.children() {
            parent[child.index()] = node.index();
        }
    }

    let existing = clip.curves();
    let mut out = HashMap::new();

    for anim in doc.animations() {
        for channel in anim.channels() {
            let idx = channel.target().node().index();
            let key = canonical_bone_key(&names[idx]);
            if key.is_empty() {
                continue;
            }

            // Root→node chain.
            let mut chain = vec![idx];
            let mut cur = idx;
            let mut guard = 0;
            while parent[cur] != usize::MAX && guard < 64 {
                cur = parent[cur];
                chain.push(cur);
                guard += 1;
            }
            chain.reverse();

            // Bevy hashes the path from the animation root, and which ancestor
            // that is depends on the exporter. Try every suffix and keep the
            // one that names a curve the clip actually has.
            for start in 0..chain.len() {
                let path: Vec<&str> = chain[start..].iter().map(|i| names[*i].as_str()).collect();
                let id = AnimationTargetId::from_iter(path);
                if existing.contains_key(&id) {
                    out.insert(id, key.clone());
                    break;
                }
            }
        }
    }

    if out.is_empty() {
        return Err(format!(
            "no curve key matched any glTF node path ({} curves, {} nodes) — \
             the clip and its source file may not correspond",
            existing.len(),
            names.len()
        ));
    }

    Ok(out)
}

/// Stamp the canonical ID onto every bound bone so the rewritten clips apply.
///
/// Overwrites the `AnimationTargetId` Bevy's glTF loader inserted. Safe: only
/// the id changes, not the `AnimationTarget.player` link.
/// Stamp the canonical id AND the player link onto every bone.
///
/// `AnimatedBy` is the half that is easy to miss. `bevy_gltf` inserts both
/// `AnimationTargetId` and `AnimatedBy` only for nodes inside an animation
/// context, and that context exists only for nodes listed in the glTF's own
/// `animation_roots` (`bevy_gltf-0.19.0/src/loader/mod.rs:1545-1560`).
///
/// The shipped bodies (`x_bot.glb`, `y_bot.glb`) contain **no animations** —
/// the clips live in separate files. So their bones arrive with neither
/// component, and Bevy has no link from bone to player. Stamping only the id
/// leaves every retargeted curve resolving to nothing, which presents exactly
/// as "the character does not animate at all" even though retargeting reports
/// full success.
pub fn apply_canonical_ids_to_rig(
    commands: &mut Commands,
    rig: &AvatarRig,
    player: Entity,
) -> usize {
    let mut n = 0;
    // Every named node — fingers and toes included — so the retargeted curves
    // for those chains have something to address.
    for (key, e) in rig.by_key.iter() {
        commands
            .entity(*e)
            .insert((canonical_target_id_for_key(key), AnimatedBy(player)));
        n += 1;
    }
    n
}

/// The rotation a clip's own root node carries.
///
/// Mixamo exports its animation files in a **Z-up** frame and compensates with
/// a +90° X rotation on the `Armature` node, while the shipped body files are
/// baked Y-up with an identity root. Verified in the assets:
///
/// | | root rotation | Hips local translation |
/// |---|---|---|
/// | `y_bot.glb` | identity | `[0, +99.79, 0]` (Y-up) |
/// | `female_walking.glb` | `+90° X` | `[0.6, 1.3, -101.7]` (Z-up) |
///
/// Bevy writes the clip's Hips LOCAL transform onto the body's Hips. With the
/// body's root unrotated, that lays the character on its back along -Z — the
/// observed failure. Applying this rotation to the body's skeleton root makes
/// the two frames agree.
///
/// Returned rather than assumed, so a clip exported from a different tool
/// (or already Y-up) works without a special case.
pub fn clip_root_rotation(glb_bytes: &[u8]) -> Option<Quat> {
    let gltf = gltf::Gltf::from_slice(glb_bytes).ok()?;
    let doc = &gltf.document;
    let scene = doc.default_scene().or_else(|| doc.scenes().next())?;
    let root = scene.nodes().next()?;

    let (_, r, _) = root.transform().decomposed();
    let q = Quat::from_xyzw(r[0], r[1], r[2], r[3]);

    // Treat a numerically-identity rotation as "no correction needed".
    if q.is_near_identity() || !q.is_finite() {
        None
    } else {
        Some(q.normalize())
    }
}

/// Read a bundled clip's bytes for retargeting.
pub fn read_bundled_clip(relative: &str) -> Result<Vec<u8>, String> {
    let path = super::boot::bundled_root().join(relative);
    std::fs::read(&path).map_err(|e| format!("read {path:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical space must be stable and collision-free — two bones
    /// sharing an id would silently merge their curves.
    #[test]
    fn canonical_ids_are_unique_per_bone() {
        let mut seen = std::collections::HashSet::new();
        for b in HumanoidBone::ALL {
            assert!(seen.insert(canonical_target_id(b)), "duplicate canonical id for {b:?}");
        }
    }

    #[test]
    fn canonical_ids_are_deterministic() {
        for b in HumanoidBone::ALL {
            assert_eq!(canonical_target_id(b), canonical_target_id(b));
        }
    }

    /// End-to-end against the real shipped assets: parse each clip, recover
    /// its bone mapping, and assert it covers the limbs.
    ///
    /// This is the test that goes red on the shipped defect — `x_bot` bound
    /// 7 bones before the canonical rewrite.
    #[test]
    fn shipped_clips_map_onto_the_humanoid_rig() {
        let root = super::super::boot::bundled_root();
        let dir = root.join("characters/animations");
        if !dir.is_dir() {
            eprintln!("skipping: bundled animations not found at {dir:?}");
            return;
        }

        for sex in ["male", "female"] {
            for motion in ["idle", "walking", "running", "jump"] {
                let file = dir.join(format!("{sex}_{motion}.glb"));
                if !file.is_file() {
                    continue;
                }
                let bytes = std::fs::read(&file).expect("read clip");
                let gltf = gltf::Gltf::from_slice(&bytes).expect("parse clip");

                let mut bones = std::collections::HashSet::new();
                for anim in gltf.document.animations() {
                    for ch in anim.channels() {
                        let n = ch.target().node();
                        if let Some(b) =
                            HumanoidBone::from_canonical(&canonical_bone_key(n.name().unwrap_or("")))
                        {
                            bones.insert(b);
                        }
                    }
                }

                assert!(
                    bones.len() >= 15,
                    "{sex}_{motion}.glb only mapped {} bones onto the humanoid rig: {:?}",
                    bones.len(),
                    bones
                );
                // The limbs specifically — the chains that were dead.
                for required in [
                    HumanoidBone::LeftUpLeg,
                    HumanoidBone::RightUpLeg,
                    HumanoidBone::LeftFoot,
                    HumanoidBone::RightFoot,
                    HumanoidBone::LeftArm,
                    HumanoidBone::RightArm,
                ] {
                    assert!(
                        bones.contains(&required),
                        "{sex}_{motion}.glb does not animate {required:?}"
                    );
                }
            }
        }
    }

    /// The pre-fix failure, asserted directly: raw Mixamo names from the body
    /// and the clip hash differently, which is why curves bound nothing.
    #[test]
    fn raw_export_names_hash_apart_but_canonical_names_agree() {
        let clip_side = AnimationTargetId::from_iter(["mixamorig:LeftUpLeg_056"]);
        let body_side = AnimationTargetId::from_iter(["mixamorig:LeftUpLeg_061"]);
        assert_ne!(clip_side, body_side, "raw names unexpectedly agree");

        // Both canonicalise onto the same bone, hence the same id.
        let a = HumanoidBone::from_canonical(&canonical_bone_key("mixamorig:LeftUpLeg_056")).unwrap();
        let b = HumanoidBone::from_canonical(&canonical_bone_key("mixamorig:LeftUpLeg_061")).unwrap();
        assert_eq!(canonical_target_id(a), canonical_target_id(b));
    }
}
