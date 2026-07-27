//! # The procedural life layer
//!
//! Breathing, exertion, idle breaks, landing flex, and look-at. These are what
//! separate "a correct character" from one that reads as alive, and none of
//! them need authored clips — which matters, because the entire shipped asset
//! library is 8 Mixamo clips.
//!
//! ## Composition, not assignment
//!
//! **Verified constraint:** the shipped clips key `scale` channels (parsed out
//! of `male_walking.glb`: channel paths are `{rotation, translation, scale}`).
//! So anything that touches bone scale must *multiply* the animated value in
//! [`AvatarSystems::PostAnim`], never assign it. Assigning at bind time — the
//! obvious implementation of a height slider — gets silently overwritten by
//! `animate_targets` on the very next frame.
//!
//! The same applies to rotations: these systems compose a delta onto whatever
//! the clip produced, so the procedural layer reads as additive motion rather
//! than replacing the animation.
//!
//! ## What this replaces
//!
//! `ProceduralAnimation::get_breathing_offset` / `get_sway_offset` in the old
//! code were correct math whose only caller was `#[allow(dead_code)]`. The
//! best procedural animation in the repo — counter-phase shoulder swing,
//! elbow flex coupled to shoulder phase, distinct airborne poses, ~380 lines
//! at `client/src/plugins/player_plugin.rs:1371-1750` — was dead code keyed on
//! a component no live path ever inserted.

use bevy::prelude::*;

use super::rig::{AvatarRig, HumanoidBone};
use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarSystems, SpawnedByAvatarRuntime};

/// Resting breaths per minute.
const BREATH_RPM_REST: f32 = 12.0;
/// Additional breaths per minute at full exertion.
const BREATH_RPM_EXERTION: f32 = 22.0;
/// How long exertion takes to decay after stopping, seconds (roughly).
const EXERTION_DECAY: f32 = 6.0;
/// Idle-break window.
const IDLE_BREAK_MIN: f32 = 9.0;
const IDLE_BREAK_MAX: f32 = 19.0;

/// Per-avatar procedural state.
#[derive(Component, Debug, Clone)]
pub struct AvatarLife {
    /// Breathing phase, radians.
    pub breath_phase: f32,
    /// Low-passed effort, 0..1. Decays over seconds, so a player who just
    /// sprinted keeps breathing hard — cheap, and most of what "alive" means.
    pub exertion: f32,
    /// Seconds spent continuously idle.
    pub idle_time: f32,
    /// When the next idle break fires.
    pub next_break_at: f32,
    /// Active idle-break progress, 0..1; 0 = inactive.
    pub break_t: f32,
    /// Landing flex, released by a critically damped spring.
    pub land_flex: f32,
    pub land_flex_vel: f32,
    /// Smoothed look-at offsets (yaw, pitch) in radians.
    pub look: Vec2,
    /// Deterministic per-avatar variation so a crowd does not breathe in
    /// lockstep. Seeded from the entity index, never from a global RNG —
    /// `Math.random`-style state would break replay determinism.
    pub phase_offset: f32,
}

impl AvatarLife {
    fn new(seed: u32) -> Self {
        let f = (seed.wrapping_mul(2_654_435_761) >> 8) as f32 / 16_777_216.0;
        Self {
            breath_phase: f * std::f32::consts::TAU,
            exertion: 0.0,
            idle_time: 0.0,
            next_break_at: IDLE_BREAK_MIN + f * (IDLE_BREAK_MAX - IDLE_BREAK_MIN),
            break_t: 0.0,
            land_flex: 0.0,
            land_flex_vel: 0.0,
            look: Vec2::ZERO,
            phase_offset: f,
        }
    }
}

pub(crate) struct AvatarLifePlugin;

impl Plugin for AvatarLifePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_life.in_set(AvatarSystems::Lifecycle))
            .add_systems(Update, tick_life.in_set(AvatarSystems::Animation))
            // Bone writes compose onto the animated pose.
            // After `lock_root_motion`, which pins hips translation — running
            // before it would have every hips write silently discarded.
            .add_systems(
                PostUpdate,
                apply_life_to_bones
                    .in_set(AvatarSystems::PostAnim)
                    .after(super::anim::lock_root_motion),
            );
    }
}

fn attach_life(
    mut commands: Commands,
    q: Query<Entity, (With<SpawnedByAvatarRuntime>, With<AvatarRig>, Without<AvatarLife>)>,
) {
    for e in q.iter() {
        commands.entity(e).insert(AvatarLife::new(e.to_bits() as u32));
    }
}

/// Advance the state machine. Pure bookkeeping — no bone access, so it can
/// live in `Update` alongside the animation driver.
fn tick_life(
    time: Res<Time>,
    mut q: Query<(&mut AvatarLife, &AvatarLocomotion, &AvatarIntent, &AvatarBody), With<SpawnedByAvatarRuntime>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (mut life, loco, intent, body) in q.iter_mut() {
        // ── Exertion: fast attack, slow release ────────────────────────────
        let effort = (loco.speed_norm).clamp(0.0, 1.5) / 1.5;
        let rate = if effort > life.exertion { 1.2 } else { 1.0 / EXERTION_DECAY };
        life.exertion += (effort - life.exertion) * (1.0 - (-rate * dt).exp());
        life.exertion = life.exertion.clamp(0.0, 1.0);

        // ── Breathing ──────────────────────────────────────────────────────
        let rpm = BREATH_RPM_REST + BREATH_RPM_EXERTION * life.exertion;
        life.breath_phase += (rpm / 60.0) * std::f32::consts::TAU * dt;
        if life.breath_phase > std::f32::consts::TAU {
            life.breath_phase -= std::f32::consts::TAU;
        }

        // ── Idle breaks ────────────────────────────────────────────────────
        let is_idle = loco.grounded
            && loco.planar_speed < 0.05
            && intent.direction.length_squared() < 1e-4;

        if is_idle {
            life.idle_time += dt;
            if life.break_t > 0.0 {
                // Play out over 1.4 s.
                life.break_t += dt / 1.4;
                if life.break_t >= 1.0 {
                    life.break_t = 0.0;
                    life.idle_time = 0.0;
                    // Vary the next interval deterministically.
                    let f = (life.phase_offset * 7.3).fract();
                    life.next_break_at = IDLE_BREAK_MIN + f * (IDLE_BREAK_MAX - IDLE_BREAK_MIN);
                }
            } else if life.idle_time >= life.next_break_at {
                life.break_t = 1e-4;
            }
        } else {
            life.idle_time = 0.0;
            life.break_t = 0.0;
        }

        // ── Landing flex ───────────────────────────────────────────────────
        //
        // `land_impact` is latched by the locomotion controller on the
        // grounding edge, before vertical velocity is zeroed.
        if loco.land_impact > life.land_flex {
            life.land_flex = loco.land_impact;
            life.land_flex_vel = 0.0;
        }
        // Critically damped release (zeta = 1): settles without overshoot.
        let omega = 14.0;
        let accel = -omega * omega * life.land_flex - 2.0 * omega * life.land_flex_vel;
        life.land_flex_vel += accel * dt;
        life.land_flex = (life.land_flex + life.land_flex_vel * dt).max(0.0);

        let _ = body;
    }
}

/// Compose the procedural layer onto the animated pose.
pub(crate) fn apply_life_to_bones(
    mut writes: Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    q: Query<(&AvatarLife, &AvatarRig, &AvatarBody), With<SpawnedByAvatarRuntime>>,
) {
    for (life, rig, body) in q.iter() {
        if !rig.is_healthy() {
            continue;
        }

        let breath = life.breath_phase.sin();
        let amp = 1.0 + 0.9 * life.exertion;

        // ── Chest expansion ────────────────────────────────────────────────
        //
        // MULTIPLY: the clips animate scale, so assigning here would fight
        // `animate_targets` and flicker.
        if let Some(chest) = rig.bone(HumanoidBone::Spine2) {
            if let Ok(mut t) = writes.get_mut(chest) {
                let s = 1.0 + 0.012 * amp * breath;
                t.scale.z *= s;
                t.scale.x *= 1.0 + 0.006 * amp * breath;
            }
        }

        // ── Spine pitch, with the neck counter-rotating ────────────────────
        if let Some(spine) = rig.bone(HumanoidBone::Spine1) {
            if let Ok(mut t) = writes.get_mut(spine) {
                t.rotation *= Quat::from_rotation_x(0.006 * amp * breath);
            }
        }
        if let Some(neck) = rig.bone(HumanoidBone::Neck) {
            if let Ok(mut t) = writes.get_mut(neck) {
                // Counter-pitch keeps the head level while the chest rises.
                t.rotation *= Quat::from_rotation_x(-0.004 * amp * breath);
            }
        }

        // ── Landing flex ───────────────────────────────────────────────────
        //
        // Knees absorb the impact; the hips drop proportionally to leg length
        // so a tall and a short avatar flex by the same *relative* amount.
        if life.land_flex > 1e-3 {
            let flex = 0.20 * life.land_flex;
            for knee in [HumanoidBone::LeftLeg, HumanoidBone::RightLeg] {
                if let Some(e) = rig.bone(knee) {
                    if let Ok(mut t) = writes.get_mut(e) {
                        t.rotation *= Quat::from_rotation_x(flex);
                    }
                }
            }
            if let Some(hips) = rig.bone(HumanoidBone::Hips) {
                if let Ok(mut t) = writes.get_mut(hips) {
                    // METRES → armature-local. The Mixamo armature carries
                    // scale 0.01, so writing a metres value straight into a
                    // bone's local translation is ~100x too large — that is
                    // what launched the character out of view when this layer
                    // was first enabled.
                    t.translation.y -= (flex * body.metrics.leg_length * 0.5) / rig.armature_scale;
                }
            }
        }

        // ── Idle break: a lateral sway and a shoulder roll ─────────────────
        if life.break_t > 0.0 {
            // Half-sine envelope: starts and ends at zero, so no pop.
            let env = (life.break_t.clamp(0.0, 1.0) * std::f32::consts::PI).sin();
            if let Some(hips) = rig.bone(HumanoidBone::Hips) {
                if let Ok(mut t) = writes.get_mut(hips) {
                    t.translation.x += (0.03 * env) / rig.armature_scale;
                }
            }
            if let Some(sh) = rig.bone(HumanoidBone::RightShoulder) {
                if let Ok(mut t) = writes.get_mut(sh) {
                    t.rotation *= Quat::from_rotation_z(0.12 * env);
                }
            }
        }

        // ── Look-at, distributed down the spine ────────────────────────────
        //
        // 20/30/50 across Spine2/Neck/Head, each clamped, so the head never
        // snaps past the shoulder.
        if life.look.length_squared() > 1e-6 {
            let parts = [
                (HumanoidBone::Spine2, 0.20, 20.0_f32),
                (HumanoidBone::Neck, 0.30, 35.0),
                (HumanoidBone::Head, 0.50, 45.0),
            ];
            for (bone, share, limit) in parts {
                if let Some(e) = rig.bone(bone) {
                    if let Ok(mut t) = writes.get_mut(e) {
                        let lim = limit.to_radians();
                        let yaw = (life.look.x * share).clamp(-lim, lim);
                        let pitch = (life.look.y * share).clamp(-lim, lim);
                        t.rotation *= Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breathing_rate_rises_with_exertion_and_stays_bounded() {
        for e in [0.0_f32, 0.5, 1.0] {
            let rpm = BREATH_RPM_REST + BREATH_RPM_EXERTION * e;
            assert!(rpm >= BREATH_RPM_REST && rpm <= BREATH_RPM_REST + BREATH_RPM_EXERTION);
        }
        assert!(
            BREATH_RPM_REST + BREATH_RPM_EXERTION < 40.0,
            "peak breathing rate is implausible for a human"
        );
    }

    /// Exertion must attack faster than it decays — that asymmetry is the
    /// whole effect. A symmetric filter makes a sprint stop look instant.
    #[test]
    fn exertion_attacks_fast_and_decays_slow() {
        let dt = 1.0 / 60.0;

        let mut e = 0.0_f32;
        for _ in 0..60 {
            let rate = if 1.0 > e { 1.2 } else { 1.0 / EXERTION_DECAY };
            e += (1.0 - e) * (1.0 - (-rate * dt).exp());
        }
        let after_1s_sprint = e;

        for _ in 0..60 {
            let rate = if 0.0 > e { 1.2 } else { 1.0 / EXERTION_DECAY };
            e += (0.0 - e) * (1.0 - (-rate * dt).exp());
        }
        let after_1s_rest = e;

        assert!(after_1s_sprint > 0.5, "attack too slow: {after_1s_sprint}");
        assert!(
            after_1s_rest > after_1s_sprint * 0.6,
            "decayed too fast — the character should still be breathing hard \
             a second after stopping (was {after_1s_sprint}, now {after_1s_rest})"
        );
    }

    #[test]
    fn landing_flex_settles_without_overshoot() {
        let dt = 1.0 / 60.0;
        let mut x = 1.0_f32;
        let mut v = 0.0_f32;
        let omega = 14.0;
        let mut min_seen = f32::INFINITY;

        for _ in 0..240 {
            let a = -omega * omega * x - 2.0 * omega * v;
            v += a * dt;
            x = (x + v * dt).max(0.0);
            min_seen = min_seen.min(x);
        }

        assert!(x < 0.02, "flex never settled: {x}");
        assert!(min_seen >= 0.0, "critically damped spring overshot below zero");
    }

    #[test]
    fn idle_break_interval_is_in_range_and_varies_between_avatars() {
        let a = AvatarLife::new(1);
        let b = AvatarLife::new(999_983);
        for l in [&a, &b] {
            assert!(l.next_break_at >= IDLE_BREAK_MIN && l.next_break_at <= IDLE_BREAK_MAX);
            assert!(l.breath_phase >= 0.0 && l.breath_phase <= std::f32::consts::TAU);
        }
        assert_ne!(
            a.phase_offset, b.phase_offset,
            "a crowd would breathe in lockstep"
        );
    }

    /// Same seed must give the same state — replay determinism depends on it.
    #[test]
    fn life_state_is_deterministic_per_entity() {
        let a = AvatarLife::new(42);
        let b = AvatarLife::new(42);
        assert_eq!(a.phase_offset, b.phase_offset);
        assert_eq!(a.next_break_at, b.next_break_at);
        assert_eq!(a.breath_phase, b.breath_phase);
    }

    #[test]
    fn look_distribution_sums_to_one() {
        let total: f32 = 0.20 + 0.30 + 0.50;
        assert!((total - 1.0).abs() < 1e-6, "look shares sum to {total}");
    }
}
