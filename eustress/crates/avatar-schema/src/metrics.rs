//! # Derived body metrics — the ONE dimension source
//!
//! Every metric dimension in the avatar runtime comes out of
//! [`BodyMorphs::metrics`]. This retires four mutually inconsistent hardcoded
//! sets that shipped simultaneously:
//!
//! | source | height | radius | half-height | spawn Y |
//! |---|---|---|---|---|
//! | `skinned_character.rs:193-195` | 1.83 | 0.33 | 0.585 | +1.015 |
//! | `client/player_plugin.rs:265` (dead branch) | 1.75 | 0.24 | 0.635 | +1.375 |
//! | `humanoid.rs:135` | — | 0.3 | 1.2 (mesh) | +1.0 |
//!
//! `metrics()` is pure and takes the measured bind height as a parameter, so
//! wasm and the engine produce bit-identical results — which is what lets the
//! parity test assert equality rather than approximate equality.

use crate::descriptor::{AvatarDescriptor, BodyMorphs};

#[cfg(feature = "bevy")]
use bevy::reflect::Reflect;

/// Website Height slider endpoints.
pub const MIN_HEIGHT_M: f32 = 1.45;
pub const MAX_HEIGHT_M: f32 = 2.05;
/// Standard gravity, m/s²: the value of `eustress_common::units::STANDARD_GRAVITY`,
/// which this crate sits below and cannot name. A test in `common` pins the two.
///
/// [`ResolvedMotion::jump_velocity`] is a jump's take-off at this gravity; the
/// controller uses the Space's live gravity instead
/// ([`ResolvedMotion::jump_velocity_under`]).
pub const GRAVITY_MPS2: f32 = 9.80665;
/// Leg length of the reference body the shipped Mixamo clips were authored
/// against. `stride_scale` is measured relative to this.
pub const REFERENCE_LEG_LENGTH_M: f32 = 0.93;
/// Nominal used before the rig has been measured. Replaced by the real
/// measurement as soon as `AvatarRig` binds.
pub const NOMINAL_BIND_HEIGHT_M: f32 = 1.83;
/// The fastest a person has run, m/s: Usain Bolt's top speed in the 2009
/// Berlin 100 m final. Running and sprinting add speed up to here and no
/// further ([`ResolvedMotion::capped_speeds`]). Walking is never capped.
pub const HUMAN_SPRINT_CAP_MPS: f32 = 12.42;

/// Derived, never authored. Every field has exactly one producer.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct BodyMetrics {
    pub height_m: f32,
    pub capsule_radius: f32,
    /// Avian's `Collider::capsule(radius, length)` takes the CYLINDER LENGTH,
    /// not a half-height — verified against
    /// `avian3d-0.7.0/src/collision/collider/parry/mod.rs:790`.
    ///
    /// Both the commented-out insert at `skinned_character.rs:208` and the
    /// Client's live capsule at `player_plugin.rs:307` passed a half-height,
    /// which yields a character roughly half the intended height with the mesh
    /// floating above it. This field is named so that mistake is
    /// unrepresentable at the call site.
    pub capsule_cylinder_len: f32,
    /// Root Y above the ground contact point at spawn.
    pub spawn_center_offset: f32,
    /// Local Y of the mesh child relative to the root.
    pub mesh_offset: f32,
    pub eye_height: f32,
    pub hip_height: f32,
    pub leg_length: f32,
    pub shoulder_half_width: f32,
    /// Lateral distance from the root axis to each foot. Replaces the `±0.15`
    /// guess at `animation_plugin.rs:696`; overwritten with the measured
    /// bind-pose value once the rig binds.
    pub foot_half_separation: f32,
    pub mass_kg: f32,
    /// `leg_length / REFERENCE_LEG_LENGTH_M`. Scales blend-space knots,
    /// playback rate, and the Hips translation curves during retarget.
    pub stride_scale: f32,
    /// Uniform rig scale = `height_m / measured bind height`.
    pub rig_scale: f32,
}

impl BodyMetrics {
    /// Sanity predicate used by the parity test and by debug asserts at spawn.
    pub fn is_sane(&self) -> bool {
        let fields = [
            self.height_m,
            self.capsule_radius,
            self.capsule_cylinder_len,
            self.spawn_center_offset,
            self.eye_height,
            self.hip_height,
            self.leg_length,
            self.shoulder_half_width,
            self.foot_half_separation,
            self.mass_kg,
            self.stride_scale,
            self.rig_scale,
        ];
        fields.iter().all(|v| v.is_finite() && *v > 0.0)
            && self.mesh_offset.is_finite()
            && self.capsule_radius < self.height_m
    }

    /// Distance from the capsule centre to its lowest point. The spawn offset
    /// must equal this for the capsule bottom to rest exactly on the surface.
    pub fn capsule_half_extent(&self) -> f32 {
        self.capsule_cylinder_len * 0.5 + self.capsule_radius
    }
}

impl BodyMorphs {
    /// `bind_height_m` is what `AvatarRig` MEASURED on the loaded skeleton.
    /// Passing it in keeps this pure — no globals, no asset lookups — so the
    /// web preview and the engine agree bit-for-bit.
    pub fn metrics(&self, bind_height_m: f32) -> BodyMetrics {
        let h = self.height.remap(MIN_HEIGHT_M, MAX_HEIGHT_M);
        // Build maps 0..1 → -1..+1 so the midpoint is the unmodified body.
        let build = self.build.get() * 2.0 - 1.0;

        let r = 0.155 * h * (1.0 + 0.22 * build);
        let leg = h * self.leg_ratio.remap(0.46, 0.56);

        // Guard the cylinder length: at extreme build the radius could
        // otherwise exceed half the height and produce a negative length,
        // which Avian rejects at collider construction.
        let cylinder = (h - 2.0 * r).max(0.05);
        let half_extent = cylinder * 0.5 + r;

        BodyMetrics {
            height_m: h,
            capsule_radius: r,
            capsule_cylinder_len: cylinder,
            // Capsule bottom sits exactly on the surface, plus a small skin
            // so the first physics step is not already in penetration.
            spawn_center_offset: half_extent + 0.02,
            // Mixamo bodies have their origin at the feet, so the mesh child
            // drops by the capsule half-extent to align feet with the capsule
            // bottom.
            mesh_offset: -half_extent,
            eye_height: h * 0.935,
            hip_height: leg,
            leg_length: leg,
            shoulder_half_width: 0.115 * h * (1.0 + 0.25 * build),
            foot_half_separation: 0.055 * h,
            // Roughly BMI-shaped: mass scales with height² and with build.
            mass_kg: 22.0 * h * h * (1.0 + 0.30 * build),
            stride_scale: leg / REFERENCE_LEG_LENGTH_M,
            rig_scale: h / bind_height_m.max(0.5),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Resolved motion
// ─────────────────────────────────────────────────────────────────────────────

/// Motion numbers after overrides are folded onto body-derived defaults.
///
/// Replaces the two irreconcilable speed universes that shipped together:
/// `Character` at 1.8 m/s (`services/player.rs`) and `Humanoid` at
/// 16 studs/s = 0.798 m/s under `units.rs:127` — a 2.25× gap depending on
/// which component a system happened to read.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct ResolvedMotion {
    /// Walking pace, m/s. Never capped: a script or Space may set any
    /// WalkSpeed, as in Roblox, where speed boosts rely on it.
    pub walk_speed: f32,
    /// Running pace as authored or derived, m/s. A body runs at the capped
    /// pace ([`Self::capped_run_and_sprint`]).
    pub run_speed: f32,
    /// Sprint speed over the running pace. A body sprints at the capped
    /// speed ([`Self::capped_run_and_sprint`]).
    pub sprint_multiplier: f32,
    /// Metres above the take-off point that a jump peaks at, under whatever
    /// gravity the Space has: Roblox's JumpHeight rule, so an imported game
    /// jumps as it was tuned. Converted to an impulse via `v = sqrt(2·g·apex)`
    /// ([`Self::jump_velocity_under`]) so "jump 0.95 m" is a descriptor
    /// number rather than the magic 50.0 (`Humanoid` schema) or 5.5 that
    /// shipped before. Unused while [`Self::jump_speed_mps`] is set.
    pub jump_apex_m: f32,
    /// Take-off speed of a jump, m/s, when the jump is authored as a launch
    /// speed: Roblox's JumpPower under UseJumpPower. The jump then peaks
    /// wherever the Space's gravity takes that speed. `None` jumps by
    /// [`Self::jump_apex_m`].
    pub jump_speed_mps: Option<f32>,
}

impl ResolvedMotion {
    /// Take-off speed of a jump, m/s, under a downward gravity of `gravity`
    /// m/s²: `launch_speed` when one is set (Roblox's JumpPower under
    /// UseJumpPower), otherwise the speed that peaks `apex_m` metres up
    /// (Roblox's JumpHeight). The player and every NPC jump by this one rule.
    ///
    /// Weightless, a height needs no speed, so only a launch speed leaves the
    /// ground. A negative or non-finite input counts as zero: `math.huge`
    /// would otherwise launch the body at infinite speed.
    pub fn take_off_speed(launch_speed: Option<f32>, apex_m: f32, gravity: f32) -> f32 {
        let usable = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        match launch_speed {
            Some(speed) => usable(speed),
            None => (2.0 * usable(gravity) * usable(apex_m)).sqrt(),
        }
    }

    /// This body's take-off speed, m/s, under a downward gravity of `gravity`
    /// m/s² ([`Self::take_off_speed`]).
    pub fn jump_velocity_under(&self, gravity: f32) -> f32 {
        Self::take_off_speed(self.jump_speed_mps, self.jump_apex_m, gravity)
    }

    /// This body's take-off speed, m/s, under standard gravity
    /// ([`GRAVITY_MPS2`]).
    pub fn jump_velocity(&self) -> f32 {
        self.jump_velocity_under(GRAVITY_MPS2)
    }

    /// Running and sprinting speeds, m/s, for a walking pace, a running pace
    /// and a sprint multiplier: `run = max(walk, min(run, cap))` and
    /// `sprint = max(walk, min(run * multiplier, cap))`, where the cap is
    /// [`HUMAN_SPRINT_CAP_MPS`]. Running and sprinting never add speed past a
    /// person's top speed, and never slow a body below its walk, which is
    /// uncapped. Everything that moves a body at a run or a sprint reads its
    /// speed here, so a Space's paces and a script's are capped alike. A
    /// NaN pace or multiplier moves at the walk.
    pub fn capped_speeds(walk: f32, run: f32, sprint_multiplier: f32) -> (f32, f32) {
        let cap = |v: f32| if v.is_nan() { walk } else { v.min(HUMAN_SPRINT_CAP_MPS).max(walk) };
        (cap(run), cap(run * sprint_multiplier))
    }

    /// This body's running and sprinting speeds, m/s
    /// ([`Self::capped_speeds`]).
    pub fn capped_run_and_sprint(&self) -> (f32, f32) {
        Self::capped_speeds(self.walk_speed, self.run_speed, self.sprint_multiplier)
    }
}

/// Fold authored overrides onto body-derived defaults. Pure.
pub fn resolve_motion(desc: &AvatarDescriptor, m: &BodyMetrics) -> ResolvedMotion {
    // Gait speeds scale with leg length: a taller avatar genuinely walks
    // faster. This is what makes the Height slider visible in MOTION and not
    // only in silhouette.
    let walk = 1.45 * m.stride_scale;
    let run = 3.9 * m.stride_scale;
    ResolvedMotion {
        walk_speed: desc.motion.walk_speed_mps.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(walk),
        run_speed: desc.motion.run_speed_mps.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(run),
        sprint_multiplier: desc
            .motion
            .sprint_multiplier
            .filter(|v| v.is_finite() && *v >= 1.0)
            .unwrap_or(1.45),
        jump_apex_m: desc
            .motion
            .jump_apex_m
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(0.55 + 0.35 * m.stride_scale),
        // Zero is a real launch speed: Roblox's JumpPower 0 keeps a
        // character on the ground.
        jump_speed_mps: desc.motion.jump_speed_mps.filter(|v| v.is_finite() && *v >= 0.0),
    }
}

/// Convenience: metrics + motion in one call, the pair every consumer wants.
pub fn resolve(desc: &AvatarDescriptor, bind_height_m: f32) -> (BodyMetrics, ResolvedMotion) {
    let m = desc.morphs.metrics(bind_height_m);
    let motion = resolve_motion(desc, &m);
    (m, motion)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — these are the P1 gate's schema half
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::*;

    fn morphs(h: f32, b: f32) -> BodyMorphs {
        BodyMorphs { height: Norm01::new(h), build: Norm01::new(b), ..Default::default() }
    }

    #[test]
    fn capsule_bottom_rests_exactly_on_the_spawn_surface() {
        // The invariant the old code got wrong three different ways.
        let mut h = 0.0;
        while h <= 1.0 {
            let mut b = 0.0;
            while b <= 1.0 {
                let m = morphs(h, b).metrics(NOMINAL_BIND_HEIGHT_M);
                let bottom = m.spawn_center_offset - m.capsule_half_extent();
                assert!(
                    (bottom - 0.02).abs() < 1e-5,
                    "h={h} b={b}: capsule bottom {bottom} not at the 0.02 skin"
                );
                b += 0.125;
            }
            h += 0.125;
        }
    }

    #[test]
    fn no_metric_is_nan_or_negative_across_the_whole_slider_range() {
        let mut h = 0.0;
        while h <= 1.0 {
            let mut b = 0.0;
            while b <= 1.0 {
                let m = morphs(h, b).metrics(NOMINAL_BIND_HEIGHT_M);
                assert!(m.is_sane(), "h={h} b={b} produced {m:?}");
                b += 0.0625;
            }
            h += 0.0625;
        }
    }

    #[test]
    fn non_finite_authoring_input_cannot_reach_metrics() {
        let m = morphs(f32::NAN, f32::INFINITY).metrics(NOMINAL_BIND_HEIGHT_M);
        assert!(m.is_sane(), "NaN/Inf slider input leaked into metrics: {m:?}");
    }

    #[test]
    fn height_slider_endpoints_hit_the_documented_range() {
        assert!((morphs(0.0, 0.5).metrics(NOMINAL_BIND_HEIGHT_M).height_m - MIN_HEIGHT_M).abs() < 1e-6);
        assert!((morphs(1.0, 0.5).metrics(NOMINAL_BIND_HEIGHT_M).height_m - MAX_HEIGHT_M).abs() < 1e-6);
    }

    #[test]
    fn taller_avatars_walk_faster_and_jump_higher() {
        let d = AvatarDescriptor::default();
        let short = morphs(0.0, 0.5).metrics(NOMINAL_BIND_HEIGHT_M);
        let tall = morphs(1.0, 0.5).metrics(NOMINAL_BIND_HEIGHT_M);
        let (ms, mt) = (resolve_motion(&d, &short), resolve_motion(&d, &tall));
        assert!(mt.walk_speed > ms.walk_speed);
        assert!(mt.jump_apex_m > ms.jump_apex_m);
    }

    #[test]
    fn overrides_win_but_nonsense_overrides_do_not() {
        let mut d = AvatarDescriptor::default();
        d.motion.walk_speed_mps = Some(3.0);
        let m = morphs(0.5, 0.5).metrics(NOMINAL_BIND_HEIGHT_M);
        assert_eq!(resolve_motion(&d, &m).walk_speed, 3.0);

        d.motion.walk_speed_mps = Some(f32::NAN);
        assert!(resolve_motion(&d, &m).walk_speed.is_finite());
        d.motion.walk_speed_mps = Some(-5.0);
        assert!(resolve_motion(&d, &m).walk_speed > 0.0);
    }

    #[test]
    fn jump_velocity_reaches_the_requested_apex() {
        let mut d = AvatarDescriptor::default();
        d.motion.jump_apex_m = Some(0.95);
        let m = morphs(0.5, 0.5).metrics(NOMINAL_BIND_HEIGHT_M);
        let v = resolve_motion(&d, &m).jump_velocity();
        // v²/2g back to apex.
        let apex = v * v / (2.0 * GRAVITY_MPS2);
        assert!((apex - 0.95).abs() < 1e-4, "apex {apex}");
    }

    /// Under any gravity the jump peaks at its authored height; weightless,
    /// it cannot take off.
    #[test]
    fn a_jump_peaks_at_its_apex_under_any_gravity() {
        let mut d = AvatarDescriptor::default();
        d.motion.jump_apex_m = Some(0.95);
        let motion = resolve_motion(&d, &morphs(0.5, 0.5).metrics(NOMINAL_BIND_HEIGHT_M));
        for g in [1.62_f32, GRAVITY_MPS2, 54.936] {
            let v = motion.jump_velocity_under(g);
            assert!((v * v / (2.0 * g) - 0.95).abs() < 1e-4, "gravity {g}");
        }
        assert_eq!(motion.jump_velocity_under(0.0), 0.0);
        assert_eq!(motion.jump_velocity_under(-9.8), 0.0);
        assert_eq!(motion.jump_velocity(), motion.jump_velocity_under(GRAVITY_MPS2));
    }

    /// A launch speed (Roblox's JumpPower under UseJumpPower) takes off at
    /// exactly that speed under any gravity, weightless included, and the
    /// height is then unused.
    #[test]
    fn a_launch_speed_takes_off_at_that_speed_under_any_gravity() {
        let mut d = AvatarDescriptor::default();
        d.motion.jump_apex_m = Some(0.95);
        d.motion.jump_speed_mps = Some(12.5);
        let motion = resolve_motion(&d, &morphs(0.5, 0.5).metrics(NOMINAL_BIND_HEIGHT_M));
        assert_eq!(motion.jump_speed_mps, Some(12.5));
        for g in [0.0_f32, 1.62, GRAVITY_MPS2, 54.936] {
            assert_eq!(motion.jump_velocity_under(g), 12.5, "gravity {g}");
        }

        d.motion.jump_speed_mps = Some(0.0);
        let grounded = resolve_motion(&d, &morphs(0.5, 0.5).metrics(NOMINAL_BIND_HEIGHT_M));
        assert_eq!(grounded.jump_velocity(), 0.0, "JumpPower 0 does not jump");

        for bad in [f32::NAN, f32::INFINITY, -4.0] {
            d.motion.jump_speed_mps = Some(bad);
            let m = resolve_motion(&d, &morphs(0.5, 0.5).metrics(NOMINAL_BIND_HEIGHT_M));
            assert_eq!(m.jump_speed_mps, None, "{bad} falls back to the height");
        }
    }

    /// Written straight onto a body (a script's JumpPower or JumpHeight), a
    /// bad number still does not launch it.
    #[test]
    fn a_bad_jump_number_does_not_launch_the_body() {
        for bad in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, -3.0] {
            assert_eq!(ResolvedMotion::take_off_speed(Some(bad), 1.0, GRAVITY_MPS2), 0.0, "speed {bad}");
            assert_eq!(ResolvedMotion::take_off_speed(None, bad, GRAVITY_MPS2), 0.0, "height {bad}");
            assert_eq!(ResolvedMotion::take_off_speed(None, 1.0, bad), 0.0, "gravity {bad}");
        }
    }

    /// Box Head walks at 8 m/s with a run at the gait ratio (21.5) and a 1.45
    /// sprint (31.2): both stop at a person's top speed.
    #[test]
    fn a_fast_walk_runs_and_sprints_at_human_top_speed() {
        let (run, sprint) = ResolvedMotion::capped_speeds(8.0, 8.0 * 3.9 / 1.45, 1.45);
        assert_eq!(run, HUMAN_SPRINT_CAP_MPS);
        assert_eq!(sprint, HUMAN_SPRINT_CAP_MPS);
    }

    /// A walk above the cap is never slowed: a speed boost to 20 m/s moves at
    /// 20 in every gait.
    #[test]
    fn a_walk_above_the_cap_moves_at_its_walk_in_every_gait() {
        assert_eq!(ResolvedMotion::capped_speeds(20.0, 20.0 * 3.9 / 1.45, 1.45), (20.0, 20.0));
        // A run authored below the walk still never slows the body under it.
        assert_eq!(ResolvedMotion::capped_speeds(6.0, 3.0, 1.2), (6.0, 6.0));
    }

    /// Body-derived paces sit far below the cap, so every body's own run and
    /// sprint are exactly as resolved, the tallest included.
    #[test]
    fn body_paces_are_under_the_cap() {
        for (h, leg) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)] {
            let mut d = AvatarDescriptor::default();
            d.morphs.height = Norm01::new(h);
            d.morphs.leg_ratio = Norm01::new(leg);
            let m = d.morphs.metrics(NOMINAL_BIND_HEIGHT_M);
            let motion = resolve_motion(&d, &m);
            let (run, sprint) = motion.capped_run_and_sprint();
            assert_eq!(run, motion.run_speed, "height {h}, leg {leg}");
            assert_eq!(sprint, motion.run_speed * motion.sprint_multiplier, "height {h}, leg {leg}");
            assert!(sprint < HUMAN_SPRINT_CAP_MPS);
        }
    }

    /// A NaN pace moves at the walk rather than at the cap.
    #[test]
    fn a_nan_pace_moves_at_the_walk() {
        assert_eq!(ResolvedMotion::capped_speeds(2.0, f32::NAN, 1.45), (2.0, 2.0));
        assert_eq!(ResolvedMotion::capped_speeds(2.0, 4.0, f32::NAN), (4.0, 2.0));
    }

    #[test]
    fn descriptor_round_trips_through_json_and_toml() {
        let mut d = AvatarDescriptor::default();
        d.base_body = BaseBody::Masculine;
        d.morphs.height = Norm01::from_percent(72);
        d.palette.hair = Srgb8::from_hex("#c4892a").unwrap();
        d.set_slot(SlotKind::Hat, Some(ItemId("hat_cap_01".into())));
        d.motion.jump_apex_m = Some(0.8);

        let j: AvatarDescriptor = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(j, d);
        let t: AvatarDescriptor = toml::from_str(&toml::to_string(&d).unwrap()).unwrap();
        assert_eq!(t, d);
    }

    /// Unset, a launch speed leaves the JSON (the avatar API rejects motion
    /// keys it does not know); set, it round-trips and moves the hash.
    #[test]
    fn a_launch_speed_is_written_and_hashed_only_when_set() {
        let mut d = AvatarDescriptor::default();
        let unset = d.content_hash();
        assert!(!serde_json::to_string(&d).unwrap().contains("jump_speed_mps"));

        d.motion.jump_speed_mps = Some(15.0);
        assert!(serde_json::to_string(&d).unwrap().contains("jump_speed_mps"));
        assert_ne!(d.content_hash(), unset, "a launch speed did not move the hash");
        let j: AvatarDescriptor = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(j, d);
        let t: AvatarDescriptor = toml::from_str(&toml::to_string(&d).unwrap()).unwrap();
        assert_eq!(t, d);
    }

    /// The avatar API accepts only these motion keys (the Worker's allowlist
    /// in `infrastructure/cloudflare/api/src/avatar.mjs`). A launch speed is
    /// Space policy, never part of a saved avatar, so a descriptor the website
    /// saves writes no other motion key.
    #[test]
    fn a_saved_avatar_writes_only_the_motion_keys_the_api_accepts() {
        const API_MOTION_KEYS: [&str; 4] = ["walk_speed_mps", "run_speed_mps", "sprint_multiplier", "jump_apex_m"];
        let mut d = AvatarDescriptor::default();
        d.motion.walk_speed_mps = Some(2.0);
        d.motion.run_speed_mps = Some(4.0);
        d.motion.sprint_multiplier = Some(1.5);
        d.motion.jump_apex_m = Some(0.9);
        let json = serde_json::to_value(&d).unwrap();
        let motion = json["motion"].as_object().expect("motion is an object");
        assert_eq!(motion.len(), API_MOTION_KEYS.len());
        for key in motion.keys() {
            assert!(API_MOTION_KEYS.contains(&key.as_str()), "the avatar API rejects the motion key {key}");
        }
    }

    #[test]
    fn hex_round_trips_byte_exact_for_every_website_swatch() {
        // The 21 swatches rendered by web/src/pages/profile.rs.
        for hex in [
            "#f5d0a9", "#d4a574", "#a0754a", "#6b4226", "#3d2314", "#f0c8c8", // skin
            "#1a1a1a", "#3b2a1a", "#5a3825", "#8b6914", "#c4892a", "#d4a04a", "#c4500f", "#a0a0a0",
            "#e0e0e0", // hair
        ] {
            let c = Srgb8::from_hex(hex).unwrap();
            assert_eq!(c.to_hex(), hex);
            assert!(c.to_linear().iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0));
        }
        assert!(Srgb8::from_hex("nope").is_err());
        assert!(Srgb8::from_hex("#12345").is_err());
    }

    #[test]
    fn percent_round_trips_so_slider_and_runtime_cannot_disagree() {
        for p in 0..=100 {
            assert_eq!(Norm01::from_percent(p).percent(), p);
        }
    }

    #[test]
    fn content_hash_is_order_independent_and_change_sensitive() {
        let mut a = AvatarDescriptor::default();
        a.set_slot(SlotKind::Hat, Some(ItemId("h1".into())));
        a.set_slot(SlotKind::Hair, Some(ItemId("r1".into())));

        let mut b = AvatarDescriptor::default();
        b.set_slot(SlotKind::Hair, Some(ItemId("r1".into())));
        b.set_slot(SlotKind::Hat, Some(ItemId("h1".into())));

        assert_eq!(a.content_hash(), b.content_hash(), "slot insertion order leaked into the hash");

        b.palette.skin = Srgb8(1, 2, 3);
        assert_ne!(a.content_hash(), b.content_hash(), "palette change did not move the hash");
    }

    #[test]
    fn face_shape_one_hot_round_trips() {
        for s in FaceShape::all() {
            let mut m = BodyMorphs::default();
            s.apply(&mut m);
            assert_eq!(FaceShape::dominant(&m), s);
        }
    }

    #[test]
    fn base_body_pairing_is_uncrossable() {
        // The bug this type exists to kill: mesh and clip prefix must always
        // agree, because they come out of the same match.
        assert!(BaseBody::Feminine.body_asset().contains("x_bot"));
        assert_eq!(BaseBody::Feminine.clip_prefix(), "female");
        assert!(BaseBody::Masculine.body_asset().contains("y_bot"));
        assert_eq!(BaseBody::Masculine.clip_prefix(), "male");
    }
}
