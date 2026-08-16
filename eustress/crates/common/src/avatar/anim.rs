//! # The motion graph
//!
//! Builds a real Bevy `AnimationGraph` per avatar and drives its weights from
//! the locomotion signal.
//!
//! ## Two structural decisions that erase whole bug classes
//!
//! **1. Every node is `play()`ed exactly once, at graph build.** Steady state
//! only moves weights. The old code called `play()` on the *target* node when
//! starting a crossfade, so the source node was frequently not playing at all
//! — and `update_locomotion_blend` then drove the walk node to `1.0 - blend`
//! with no run node playing, so the character visibly **faded out of
//! animation as it sped up**. Playing everything once makes that
//! unrepresentable.
//!
//! **2. `ground` is a `Blend` node.** Bevy normalises a blend node's children,
//! so idle/walk/run weights cannot sum to less than full influence no matter
//! what the driver writes. The previous design hand-managed absolute weights
//! on a flat graph and had to get every transition arithmetically right.
//!
//! ```text
//! root
//! ├── ground   (Blend, weight 1 when grounded)
//! │   ├── idle · walk · run
//! └── air      (Blend, weight 1 when airborne)
//!     └── jump
//! ```
//!
//! ## Speed-matched playback
//!
//! Playback rate is `planar_speed / authored_speed`, clamped. Without it a
//! taller avatar (longer stride, faster walk) skates, because the clip plays
//! at a fixed rate regardless of how fast the body is actually moving. This is
//! the mechanism by which the website's Height slider is visible **in motion**
//! and not only in silhouette.

// Graph types live in the `graph` submodule; only the prelude re-exports them.
use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::animation::AnimationPlayer;
use bevy::prelude::*;

use super::rig::AvatarRig;
use super::spawn::{AvatarBody, AvatarLocomotion};
use super::{AvatarSystems, SpawnedByAvatarRuntime};
use eustress_avatar_schema::BaseBody;

/// Authored ground speed of the shipped Mixamo clips, m/s at rate 1.0.
///
/// Measured rather than guessed would be better (sample foot planar speed
/// across the cycle); these are close enough that the clamp absorbs the error,
/// and P4's foot lock removes residual slide entirely.
const WALK_AUTHORED_MPS: f32 = 1.45;
const RUN_AUTHORED_MPS: f32 = 3.9;
/// Beyond this the rate change reads worse than the mismatch it fixes.
const RATE_CLAMP: (f32, f32) = (0.60, 1.60);

/// Handles + node indices for one avatar's graph.
#[derive(Component, Debug)]
pub struct AvatarMotionGraph {
    pub player: Entity,
    /// Blend-node weights live in the graph ASSET, not on the player —
    /// `AnimationPlayer::animation_mut` only reaches nodes that are playing
    /// clips, and returns `None` for a `Blend`. Driving ground/air through the
    /// player was silently a no-op.
    pub handle: Handle<AnimationGraph>,
    pub ground: AnimationNodeIndex,
    pub air: AnimationNodeIndex,
    pub idle: AnimationNodeIndex,
    pub walk: AnimationNodeIndex,
    pub run: AnimationNodeIndex,
    pub jump: AnimationNodeIndex,
}

/// Marks an avatar whose clips are still loading.
#[derive(Component, Debug)]
pub struct AvatarClipsLoading {
    pub body: BaseBody,
    pub idle: Handle<AnimationClip>,
    pub walk: Handle<AnimationClip>,
    pub run: Handle<AnimationClip>,
    pub jump: Handle<AnimationClip>,
    /// Retarget is applied once, after the assets resolve.
    pub retargeted: bool,
}

/// Proof-of-life probe for the animation pipeline.
///
/// Every other signal in this module reports on the half of the pipeline this
/// code owns — curves rekeyed, bones stamped, graph built. All of those were
/// green while nothing moved, because whether Bevy *applies* a curve depends
/// on `AnimatedBy`, which none of them measured.
///
/// This samples a real bone's rotation over time and reports whether it
/// actually changed. It is the one animation signal here that can fail.
#[derive(Component, Debug)]
pub struct AnimationLivenessProbe {
    /// Per-bone rotation captured after the bind-pose settle.
    snapshot: Vec<(super::rig::HumanoidBone, Quat)>,
    frames: u32,
    reported: bool,
    /// Running maximum deviation from the snapshot, over the whole window.
    ///
    /// Endpoint-to-endpoint sampling is phase-locked against a looping clip:
    /// 4 s of a 1.033 s walk is 3.87 cycles, so both samples land at nearly the
    /// same phase and the range of motion is understated. Tracking the running
    /// max measures the actual amplitude.
    peak_deg: f32,
    peak_bone: Option<super::rig::HumanoidBone>,
}

impl Default for AnimationLivenessProbe {
    fn default() -> Self {
        Self { snapshot: Vec::new(), frames: 0, reported: false, peak_deg: 0.0, peak_bone: None }
    }
}

pub(crate) struct AvatarAnimPlugin;

impl Plugin for AvatarAnimPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (request_clips_on_bind, retarget_and_build_graph, drive_motion_weights)
                .chain()
                .in_set(AvatarSystems::Animation),
        )
        .add_systems(
            PostUpdate,
            (lock_root_motion, probe_animation_liveness)
                .chain()
                .after(bevy::app::AnimationSystems),
        );
    }
}

/// Holds the hips translation this avatar is pinned to.
#[derive(Component, Debug, Default)]
pub struct RootMotionLock {
    pinned: Option<Vec3>,
    settle: u8,
}

/// Pin the hips translation, discarding the clip's root motion.
///
/// Runs after Bevy's animation systems have written the animated pose and
/// before transform propagation, so the clip's rotations survive and only its
/// translation is neutralised. Without this the jump clip walks the mesh off
/// the capsule and snaps it back on loop.
pub(crate) fn lock_root_motion(
    mut bones: Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    mut q: Query<
        (&AvatarRig, &mut RootMotionLock, Option<&AvatarMotionGraph>),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    use super::rig::HumanoidBone;
    for (rig, mut lock, graph) in q.iter_mut() {
        // Do not capture the pin until clips are actually driving the skeleton.
        //
        // Capturing on the first frame grabbed the BIND-pose hips translation
        // (local Y = 0.998). The skeleton root carries the clip's +90° X
        // correction, which maps local +Y to world FORWARD — so the body was
        // pinned a metre ahead of the capsule and visibly pivoted around a
        // point behind itself when turning. A few frames of settling makes the
        // captured value a real animated one.
        if graph.is_none() {
            continue;
        }
        if lock.settle < 8 {
            lock.settle += 1;
            continue;
        }

        let Some(hips) = rig.bone(HumanoidBone::Hips) else { continue };
        let Ok(mut t) = bones.get_mut(hips) else { continue };

        // Pin to the CLIP's hips translation, captured on the first animated
        // frame — not to the body's bind pose.
        //
        // The body's bind hips is (0, +99.79, 0) in armature-local units, but
        // the skeleton root carries the clip's +90° X correction, which maps
        // that to 1 m FORWARD rather than 1 m UP. Pinning to it dropped the
        // body a metre and left the foot calibration silently compensating
        // with a +1.03 m shove. The clip's own value is already expressed in
        // the rotated frame, so capturing it is correct by construction and
        // works for any body/clip pair.
        let pinned = *lock.pinned.get_or_insert(t.translation);
        t.translation = pinned;
    }
}

/// Is the pose *continuously* changing, and is the player actually advancing?
///
/// The first version of this compared frame 0 against frame 60 and reported
/// 1.94° as "LIVE". That was wrong: settling from the bind pose into the first
/// animated pose is a ONE-TIME change and produces a non-zero delta even when
/// playback is frozen. A liveness check has to sample a *steady-state* window,
/// and it has to report the player's own state — elapsed time is what proves
/// the clip is advancing rather than holding a single frame.
fn probe_animation_liveness(
    bones: Query<&Transform, Without<SpawnedByAvatarRuntime>>,
    players: Query<&AnimationPlayer>,
    graphs: Res<Assets<AnimationGraph>>,
    mut q: Query<
        (
            &AvatarRig,
            &AvatarMotionGraph,
            &AvatarLocomotion,
            &AvatarBody,
            &mut AnimationLivenessProbe,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    use super::rig::HumanoidBone;

    for (rig, graph, loco, body, mut probe) in q.iter_mut() {
        if probe.reported {
            continue;
        }
        let Some(bone) = rig.bone(HumanoidBone::Spine1).or_else(|| rig.bone(HumanoidBone::LeftUpLeg))
        else {
            continue;
        };
        let Ok(t) = bones.get(bone) else { continue };

        probe.frames += 1;
        let _ = t;

        // Skip the first 30 frames: that window contains the bind-pose settle,
        // which is not evidence of playback.
        //
        // Snapshot EVERY named bone, not one. The shipped idle is an 8.35 s,
        // 501-key clip whose spine motion is genuinely tiny — watching a single
        // bone for one second cannot distinguish "subtle" from "frozen".
        if probe.frames == 30 {
            probe.snapshot = rig
                .bones
                .iter()
                .filter_map(|(b, e)| bones.get(*e).ok().map(|t| (*b, t.rotation)))
                .collect();
            return;
        }
        // Accumulate the peak every frame rather than sampling the endpoints.
        let mut frame_peak = (0.0_f32, None);
        for (b, start) in probe.snapshot.iter() {
            let Some(e) = rig.bone(*b) else { continue };
            let Ok(now) = bones.get(e) else { continue };
            let d = start.angle_between(now.rotation).to_degrees();
            if d > frame_peak.0 {
                frame_peak = (d, Some(*b));
            }
        }
        if frame_peak.0 > probe.peak_deg {
            probe.peak_deg = frame_peak.0;
            probe.peak_bone = frame_peak.1;
        }

        // ~4 s: covers a full cycle of every shipped clip.
        if probe.frames < 270 {
            return;
        }

        let delta = probe.peak_deg;
        let worst = probe.peak_bone.unwrap_or(HumanoidBone::Hips);
        let _ = bone;

        // What the blend driver decided, and what it decided it from.
        let loco_detail = format!(
            "grounded={} planar_speed={:.3} speed_norm={:.3} walk_speed={:.3}",
            loco.grounded, loco.planar_speed, loco.speed_norm, body.motion.walk_speed
        );

        // The player's own view of what it is doing.
        let (active, detail) = match players.get(graph.player) {
            Ok(p) => {
                let n = [graph.idle, graph.walk, graph.run, graph.jump]
                    .iter()
                    .filter(|i| p.is_playing_animation(**i))
                    .count();
                let gw = |n| {
                    graphs
                        .get(&graph.handle)
                        .and_then(|g| g.get(n))
                        .map(|node| node.weight)
                        .unwrap_or(-1.0)
                };
                let w = |n| p.animation(n).map(|a| a.weight()).unwrap_or(-1.0);
                let d = format!(
                    "weights: ground={:.3} air={:.3} idle={:.3} walk={:.3} run={:.3}; idle elapsed={:.3}s",
                    gw(graph.ground),
                    gw(graph.air),
                    w(graph.idle),
                    w(graph.walk),
                    w(graph.run),
                    p.animation(graph.idle).map(|a| a.elapsed()).unwrap_or(-1.0),
                );
                (n, d)
            }
            Err(_) => (0, "AnimationPlayer not found at graph.player".to_string()),
        };

        if delta > 0.05 {
            info!(
                "avatar: animation LIVE — spine moved {:.2}° in steady state; \
                 {active}/4 nodes playing; {detail}",
                delta
            );
        } else {
            error!(
                "avatar: animation FROZEN — spine moved {:.4}° over 60 steady-state frames. \
                 {active}/4 nodes playing; {detail}",
                delta
            );
        }
        probe.reported = true;
    }
}

/// Start loading the four clips for this avatar's body once the rig binds.
fn request_clips_on_bind(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    q: Query<
        (Entity, &super::AvatarDescriptor),
        (With<SpawnedByAvatarRuntime>, Added<AvatarRig>, Without<AvatarClipsLoading>),
    >,
) {
    for (e, desc) in q.iter() {
        let p = desc.base_body.clip_prefix();
        // `#Animation0` is the first clip in each single-clip Mixamo export.
        let load = |m: &str| -> Handle<AnimationClip> {
            asset_server.load(format!("bundled://characters/animations/{p}_{m}.glb#Animation0"))
        };
        commands.entity(e).insert(AvatarClipsLoading {
            body: desc.base_body,
            idle: load("idle"),
            walk: load("walking"),
            run: load("running"),
            jump: load("jump"),
            retargeted: false,
        });
    }
}

/// Retarget the loaded clips onto the canonical bone space, then build and
/// attach the graph.
#[allow(clippy::too_many_arguments)]
fn retarget_and_build_graph(
    mut commands: Commands,
    mut clips: ResMut<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    transforms: Query<&Transform, Without<SpawnedByAvatarRuntime>>,
    mut q: Query<
        (Entity, &mut AvatarClipsLoading, &AvatarRig),
        (With<SpawnedByAvatarRuntime>, Without<AvatarMotionGraph>),
    >,
) {
    for (root, mut loading, rig) in q.iter_mut() {
        // The Armature carries the export's unit scale (0.01 for Mixamo);
        // replacing its Transform must preserve that or the body collapses.
        let rig_root_scale = rig
            .skeleton_root
            .and_then(|e| transforms.get(e).ok())
            .map(|t| t.scale)
            .unwrap_or(Vec3::ONE);
        if loading.retargeted {
            continue;
        }
        // All four must be resolved before the graph is meaningful.
        let all = [
            loading.idle.clone(),
            loading.walk.clone(),
            loading.run.clone(),
            loading.jump.clone(),
        ];
        if all.iter().any(|h| clips.get(h).is_none()) {
            continue;
        }

        // ── Retarget ───────────────────────────────────────────────────────
        //
        // Rewrites curve keys from the export's suffixed names onto the
        // canonical space. Without this the clips address bones that do not
        // exist on this body: `x_bot` bound 7 of 65 curves.
        let prefix = loading.body.clip_prefix();
        let mut clip_root_fix: Option<Quat> = None;
        for (motion, handle) in [
            ("idle", &loading.idle),
            ("walking", &loading.walk),
            ("running", &loading.run),
            ("jump", &loading.jump),
        ] {
            let rel = format!("characters/animations/{prefix}_{motion}.glb");
            let Ok(bytes) = super::retarget::read_bundled_clip(&rel) else {
                warn!("avatar: cannot read {rel} for retargeting; limbs will not animate");
                continue;
            };
            // Clips authored Z-up carry their correction on their own root
            // node; the body is baked Y-up with an identity root. Without
            // re-applying it the clip lays the character on its back.
            if clip_root_fix.is_none() {
                clip_root_fix = super::retarget::clip_root_rotation(&bytes);
            }
            if let Some(mut clip) = clips.get_mut(handle) {
                // `Assets::get_mut` yields an `AssetMut` guard, not a bare
                // `&mut` — deref through it.
                if let Err(e) = super::retarget::retarget_clip_in_place(&mut clip, &bytes, &rel) {
                    warn!("avatar: retarget {rel} failed: {e}");
                }
            }
        }

        // Align the body's skeleton root with the frame the clips were
        // authored in.
        if let (Some(fix), Some(skel)) = (clip_root_fix, rig.skeleton_root) {
            commands.entity(skel).insert(Transform {
                rotation: fix,
                scale: rig_root_scale,
                ..default()
            });
            let (axis, angle) = fix.to_axis_angle();
            info!(
                "avatar: skeleton root aligned to clip frame — {:.0}° about {:?}",
                angle.to_degrees(),
                axis
            );
        }

        // ── The AnimationPlayer ────────────────────────────────────────────
        //
        // The glTF loader only creates one for a file that HAS animations. The
        // shipped bodies have none (clips live in separate files), so usually
        // there is nothing to find and the runtime must supply the player
        // itself. It goes on the skeleton root so the player is an ancestor of
        // every bone it drives.
        let player = match find_animation_player(root, &children, &players) {
            Some(p) => p,
            None => match rig.skeleton_root {
                Some(skel) => {
                    commands.entity(skel).insert(AnimationPlayer::default());
                    info!("avatar: no AnimationPlayer in the body glTF — created one on the skeleton root");
                    skel
                }
                // Rig not bound yet; retry next frame.
                None => continue,
            },
        };

        // Stamp the canonical id AND the `AnimatedBy` player link onto every
        // bone. Both are required: without the link Bevy has no path from a
        // bone to the player, and every retargeted curve resolves to nothing —
        // which looks exactly like "no animation at all".
        let stamped = super::retarget::apply_canonical_ids_to_rig(&mut commands, rig, player);

        // ── Build the graph ───────────────────────────────────────────────
        let mut graph = AnimationGraph::new();
        let ground = graph.add_blend(1.0, graph.root);
        let air = graph.add_blend(0.0, graph.root);

        // Every clip node starts at weight 1.0.
        //
        // Bevy evaluates a clip as `active_animation.weight * graph_node.weight`
        // (bevy_animation-0.19.0/src/lib.rs:1234). Building walk/run at 0.0 and
        // then driving only the PLAYER's weight multiplied 0.989 by 0.0 — they
        // could never contribute no matter what the player reported. Blending is
        // driven entirely through the graph asset below; the player's weight
        // stays at 1.0 and acts as a pure on/off.
        let idle = graph.add_clip(loading.idle.clone(), 1.0, ground);
        let walk = graph.add_clip(loading.walk.clone(), 1.0, ground);
        let run = graph.add_clip(loading.run.clone(), 1.0, ground);
        let jump = graph.add_clip(loading.jump.clone(), 1.0, air);

        let handle = graphs.add(graph);

        commands.entity(player).insert(AnimationGraphHandle(handle.clone()));
        commands.entity(root).insert(AnimationLivenessProbe::default());
        commands.entity(root).insert(RootMotionLock::default());
        commands.entity(root).insert(AvatarMotionGraph {
            player,
            handle,
            ground,
            air,
            idle,
            walk,
            run,
            jump,
        });

        loading.retargeted = true;
        info!(
            "avatar: motion graph built — {stamped} bones stamped, player {:?}",
            player
        );
    }
}

/// The glTF loader puts `AnimationPlayer` somewhere inside the spawned graph.
fn find_animation_player(
    root: Entity,
    children: &Query<&Children>,
    players: &Query<(), With<AnimationPlayer>>,
) -> Option<Entity> {
    let mut stack = vec![root];
    let mut guard = 0;
    while let Some(e) = stack.pop() {
        guard += 1;
        if guard > 512 {
            return None;
        }
        if players.contains(e) {
            return Some(e);
        }
        if let Ok(kids) = children.get(e) {
            stack.extend(kids.iter());
        }
    }
    None
}

/// Start every node once, then only move weights.
fn drive_motion_weights(
    time: Res<Time>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<&mut AnimationPlayer>,
    q: Query<
        (
            &AvatarMotionGraph,
            &AvatarLocomotion,
            &AvatarBody,
            Option<&super::climb::AvatarClimb>,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();

    for (g, loco, body, climb) in q.iter() {
        let Ok(mut player) = players.get_mut(g.player) else { continue };

        // One-time start. `play` is idempotent per node, and every node stays
        // playing for the avatar's whole life.
        for n in [g.idle, g.walk, g.run, g.jump] {
            if !player.is_playing_animation(n) {
                player.play(n).repeat();
                if let Some(a) = player.animation_mut(n) {
                    a.set_weight(1.0);
                }
            }
        }

        // ── Ground vs air ──────────────────────────────────────────────────
        // Exponential smoothing, framerate-independent. The old blend used a
        // hardcoded 0.016 and changed behaviour with framerate.
        let alpha = 1.0 - (-14.0 * dt).exp();
        let want_ground = if loco.grounded { 1.0 } else { 0.0 };

        // ── Climbing holds the GROUND branch as a base pose ────────────────
        //
        // `locomotion` reports `grounded = false` while attached to a wall, so
        // the air blend saturates — and jump is air's only child. The player
        // watched a LOOPING JUMP CLIP through every hang, shimmy and mantle
        // while `solve_climb_limbs` dragged the arms back onto the ledge each
        // frame. The two fought at frame rate, and that is what read as the
        // character wiggling on the ledge.
        //
        // Blending to *nothing* fixed the fight and introduced a worse
        // problem: with no clip driving them, every bone the IK does not touch
        // froze, and the character became a mannequin with animated arms.
        //
        // The right shape is a base pose plus an IK layer. Pinning the ground
        // branch gives idle — `planar_speed` is forced to zero while attached,
        // so the gait blend inside that branch resolves to idle on its own —
        // which keeps the spine, neck and head breathing while the limb solve
        // overrides the arms and legs it actually owns. Idle is the only clip
        // in the library that composes sanely underneath a limb solve.
        let climbing = climb.map(|c| c.is_climbing()).unwrap_or(false);
        let want_ground = if climbing { 1.0 } else { want_ground };

        // Ground/air are Blend nodes: their weights live in the graph asset.
        if let Some(mut graph) = graphs.get_mut(&g.handle) {
            if let Some(node) = graph.get_mut(g.ground) {
                let next = node.weight + (want_ground - node.weight) * alpha;
                node.weight = next;
            }
            if let Some(node) = graph.get_mut(g.air) {
                let next = node.weight + ((1.0 - want_ground) - node.weight) * alpha;
                node.weight = next;
            }
        }

        // Diagnostic override: pin one clip at full weight so a probe can tell
        // "the clips cannot drive bones" from "the idle is simply subtle".
        // EUSTRESS_ANIM_PROBE=walk|run|idle
        if let Ok(force) = std::env::var("EUSTRESS_ANIM_PROBE") {
            let target = match force.as_str() {
                "walk" => g.walk,
                "run" => g.run,
                _ => g.idle,
            };
            for n in [g.idle, g.walk, g.run] {
                if let Some(a) = player.animation_mut(n) {
                    a.set_speed(1.0);
                }
            }
            if let Some(mut graph) = graphs.get_mut(&g.handle) {
                for n in [g.idle, g.walk, g.run] {
                    if let Some(node) = graph.get_mut(n) {
                        node.weight = if n == target { 1.0 } else { 0.0 };
                    }
                }
                if let Some(node) = graph.get_mut(g.ground) {
                    node.weight = 1.0;
                }
                if let Some(node) = graph.get_mut(g.air) {
                    node.weight = 0.0;
                }
            }
            continue;
        }

        // ── idle → walk → run, by measured speed ──────────────────────────
        //
        // Absolute weights are safe here because `ground` is a Blend node:
        // Bevy normalises the children, so these are ratios and cannot sum to
        // a partial pose the way the old flat graph could.
        let speed = loco.planar_speed;
        let walk_s = body.motion.walk_speed.max(0.05);
        let run_s = body.motion.run_speed.max(walk_s + 0.05);

        let (w_idle, w_walk, w_run) = if speed < 0.08 {
            (1.0, 0.0, 0.0)
        } else if speed <= walk_s {
            let t = (speed / walk_s).clamp(0.0, 1.0);
            (1.0 - t, t, 0.0)
        } else {
            let t = ((speed - walk_s) / (run_s - walk_s)).clamp(0.0, 1.0);
            (0.0, 1.0 - t, t)
        };

        // Clip blending also lives in the graph asset, for the same reason the
        // ground/air blend does — the player's weight is only a multiplier.
        if let Some(mut graph) = graphs.get_mut(&g.handle) {
            for (node, target) in [(g.idle, w_idle), (g.walk, w_walk), (g.run, w_run)] {
                if let Some(n) = graph.get_mut(node) {
                    n.weight += (target - n.weight) * alpha;
                }
            }
        }

        // ── Speed-matched playback ─────────────────────────────────────────
        //
        // Per node. NEVER via a global retime — the old code called
        // `playing_animations_mut()` and retimed the jump clip to walk speed
        // mid-crossfade.
        let stride = body.metrics.stride_scale.max(0.25);
        let walk_rate = (speed / (WALK_AUTHORED_MPS * stride)).clamp(RATE_CLAMP.0, RATE_CLAMP.1);
        let run_rate = (speed / (RUN_AUTHORED_MPS * stride)).clamp(RATE_CLAMP.0, RATE_CLAMP.1);

        if let Some(a) = player.animation_mut(g.walk) {
            a.set_speed(walk_rate);
        }
        if let Some(a) = player.animation_mut(g.run) {
            a.set_speed(run_rate);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blend must always sum to full influence. The old implementation
    /// drove walk to `1.0 - blend` with no run node playing, so the character
    /// faded out of animation as it accelerated — this asserts the shape of
    /// the replacement across the whole speed range.
    #[test]
    fn ground_blend_weights_always_sum_to_one() {
        let walk_s = 1.45_f32;
        let run_s = 3.9_f32;
        let mut speed = 0.0_f32;
        while speed <= 6.0 {
            let (i, w, r) = if speed < 0.08 {
                (1.0, 0.0, 0.0)
            } else if speed <= walk_s {
                let t = (speed / walk_s).clamp(0.0, 1.0);
                (1.0 - t, t, 0.0)
            } else {
                let t = ((speed - walk_s) / (run_s - walk_s)).clamp(0.0, 1.0);
                (0.0, 1.0 - t, t)
            };
            let sum = i + w + r;
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "at {speed} m/s weights sum to {sum} (idle {i}, walk {w}, run {r})"
            );
            assert!(i >= 0.0 && w >= 0.0 && r >= 0.0, "negative weight at {speed}");
            speed += 0.05;
        }
    }

    #[test]
    fn playback_rate_stays_inside_the_clamp() {
        for stride in [0.6_f32, 1.0, 1.6] {
            for speed in [0.0_f32, 1.0, 3.0, 12.0] {
                let r = (speed / (WALK_AUTHORED_MPS * stride)).clamp(RATE_CLAMP.0, RATE_CLAMP.1);
                assert!(r >= RATE_CLAMP.0 && r <= RATE_CLAMP.1, "rate {r}");
                assert!(r.is_finite());
            }
        }
    }

    /// A taller avatar has a longer stride, so at the same ground speed its
    /// clip must play SLOWER — that is what keeps the feet from skating.
    #[test]
    fn longer_stride_lowers_playback_rate() {
        let speed = 1.4;
        let short = (speed / (WALK_AUTHORED_MPS * 0.85_f32)).clamp(RATE_CLAMP.0, RATE_CLAMP.1);
        let tall = (speed / (WALK_AUTHORED_MPS * 1.15_f32)).clamp(RATE_CLAMP.0, RATE_CLAMP.1);
        assert!(tall < short, "tall {tall} should play slower than short {short}");
    }
}
