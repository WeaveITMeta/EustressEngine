//! # Render Cascade — the "see-for-miles at 60fps" LOD tier ladder
//!
//! Implements Wave 3 CORE (W3.1–W3.4) of
//! [`docs/architecture/RENDER_CASCADE.md`](../../../../../docs/architecture/RENDER_CASCADE.md).
//!
//! ## What this module does
//!
//! Each streamed entity is assigned exactly one [`RenderTier`]
//! (Hero / Active / Streamed) per **16-frame cadence** based on
//! camera distance with **hysteresis**, then a reactor toggles its
//! [`Visibility`] (and a [`MeshLodTier`] marker) on tier change.
//!
//! ```text
//!   distance:   0m ─── 100m ──── 600m ──── 5500m ──── ∞
//!   tier:       HERO    ACTIVE   STREAMED  (despawned by streamer)
//!   visible:    yes     yes      no(W3)    n/a
//!   cap:        2k      20k      200k
//! ```
//!
//! ## Table of contents
//! - [`RenderTier`]          — Bevy `Component`: Hero / Active / Streamed / Horizon
//! - [`MeshLodTier`]         — Bevy `Component`: visual mesh-LOD marker the reactor sets
//! - [`TierCaps`]            — LRU caps per tier (2000 / 20000 / 200000)
//! - [`RenderCascadeConfig`] — Bevy `Resource`: distance bands + cadence + caps (§3 tunables)
//! - [`RenderCascadeFrame`]  — Bevy `Resource`: monotonic frame counter for the cadence gate
//! - [`sys_render_cascade`]  — the distance-band tier switcher (16-frame cadence, hysteresis, LRU caps)
//! - [`sys_apply_tier_change`] — the `Changed<RenderTier>` reactor (Visibility only — see LOOP 3)
//!
//! ## Scope of THIS wave (W3.1–W3.4)
//!
//! - `RenderTier` component + `RenderCascadeConfig` / `RenderCascadeFrame` resources.
//! - `sys_render_cascade`: per-entity tier from camera distance, hysteresis, LRU caps.
//! - `sys_apply_tier_change`: **visual components only** — `Visibility` + the
//!   `MeshLodTier` marker. **Never** `RigidBody` / `Collider` (see [LOOP 3](#loop-3)).
//!
//! Deferred to later waves: impostor / panorama swap (W3.9+), the
//! `ClassName::lod_components(tier)` bundle wiring (W3.4 trait integration),
//! `MeshLodCache` (W3.5), shadow-caster cap (W3.6), telemetry (W3.7),
//! and the Horizon skybox layer (W3.14+).
//!
//! ## LOOP 3 — physics-LOD desync (THE risk for this module) <a id="loop-3"></a>
//!
//! Per [`docs/process/AGENT_DISPATCH.md`](../../../../../docs/process/AGENT_DISPATCH.md)
//! LOOP 3: an LOD demotion that removes a `Collider` while leaving a
//! `RigidBody::Dynamic` makes the Avian solver run a body with no
//! collider next frame → it falls through the floor → scene corruption.
//!
//! **Breaker (enforced here):** [`sys_apply_tier_change`] toggles
//! `Visibility` and the [`MeshLodTier`] marker **only**. It adds/removes
//! **no** physics components. Physics LOD is a separate future wave.
//!
//! ## FPS guard — the cadence gate
//!
//! The 2-FPS MindMap regression was caused by unguarded per-frame
//! all-entity iteration. [`sys_render_cascade`] short-circuits cheaply
//! on the 15-of-16 non-cadence frames (a single `u64 % u32` test and an
//! early `return` before any query is iterated). Only on the 16th frame
//! does it scan entities — and even then it inserts `RenderTier` *only
//! on an actual change*, so `Changed<RenderTier>` (which drives the
//! reactor) fires for transitioning entities only, never the whole set.

use bevy::prelude::*;

use crate::streaming::plugin::StreamingInstanceRef;

// ─────────────────────────────────────────────────────────────────────────────
// RenderTier — the four-tier marker component
// ─────────────────────────────────────────────────────────────────────────────

/// Which render tier an entity currently occupies, assigned by
/// [`sys_render_cascade`] on the 16-frame cadence.
///
/// This is a marker the downstream reactor ([`sys_apply_tier_change`])
/// gates behaviour off. `Horizon` is **never** placed on an entity —
/// it denotes the camera-anchored skybox layer (a future wave) and is
/// listed only for completeness with the spec's four-tier model.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
#[reflect(Component)]
pub enum RenderTier {
    /// 0–100 m — full LOD0 mesh, shadow caster, full physics (the body-language zone).
    Hero,
    /// 100–600 m — LOD1/2 mesh, no shadow cast, static colliders (the encounter zone).
    Active,
    /// 600 m–5.5 km — LOD3 or impostor, no physics (the landscape zone).
    Streamed,
    /// 5 km+ — the composited skybox layer. **Never on an entity.**
    Horizon,
}

impl RenderTier {
    /// Whether an entity in this tier should be visible **in this wave**.
    ///
    /// W3 ships Hero + Active visible, Streamed/Horizon hidden (their
    /// impostor / panorama representations land in W3.9+). When the
    /// impostor swap arrives this returns `true` for `Streamed`.
    #[inline]
    pub fn is_visible_this_wave(self) -> bool {
        matches!(self, RenderTier::Hero | RenderTier::Active)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MeshLodTier — the visual mesh-LOD marker the reactor maintains
// ─────────────────────────────────────────────────────────────────────────────

/// Visual mesh level-of-detail marker mirrored from [`RenderTier`] by the
/// reactor. A separate component (rather than reusing `RenderTier`) so a
/// future mesh-swap system can query `Changed<MeshLodTier>` to load LOD
/// meshes without re-running the distance/cap logic.
///
/// This is a **visual-only** marker — it carries no physics meaning,
/// consistent with [LOOP 3](#loop-3).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
#[reflect(Component)]
pub enum MeshLodTier {
    /// Original GLB, meshopt-cache-optimised. Hero tier.
    Lod0,
    /// `simplify_sloppy(0.5 → 0.25)`. Active tier.
    Lod1,
    /// `simplify_sloppy(0.10)` or impostor billboard. Streamed tier.
    Lod3,
}

impl From<RenderTier> for MeshLodTier {
    #[inline]
    fn from(tier: RenderTier) -> Self {
        match tier {
            RenderTier::Hero => MeshLodTier::Lod0,
            RenderTier::Active => MeshLodTier::Lod1,
            // Streamed and the (never-on-entity) Horizon both map to the
            // coarsest mesh tier for this wave.
            RenderTier::Streamed | RenderTier::Horizon => MeshLodTier::Lod3,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TierCaps — LRU entity caps per tier
// ─────────────────────────────────────────────────────────────────────────────

/// Per-tier entity caps. When more than `cap` entities qualify for a
/// tier, the closest-N (by camera distance) win; the rest are demoted
/// one band outward. See [`enforce_caps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub struct TierCaps {
    /// Hero cap. Default 2 000.
    pub hero: usize,
    /// Active cap. Default 20 000.
    pub active: usize,
    /// Streamed cap. Default 200 000.
    pub streamed: usize,
}

impl Default for TierCaps {
    fn default() -> Self {
        Self {
            hero: 2_000,
            active: 20_000,
            streamed: 200_000,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RenderCascadeConfig — distance bands + cadence + caps (§3 tunables)
// ─────────────────────────────────────────────────────────────────────────────

/// Tunables for the render cascade, per `RENDER_CASCADE.md` §3.
///
/// Distances are world units (metres). The `*_in_m` / `*_out_m` pairs
/// form the hysteresis dead-zones that prevent a stationary player at a
/// band edge from oscillating (Risk R4).
///
/// Loadable from `<Space>/render_cascade.toml` in a future wave; for now
/// it is `Default`-initialised by [`StreamingPlugin`].
#[derive(Resource, Debug, Clone, PartialEq, Reflect)]
#[reflect(Resource)]
pub struct RenderCascadeConfig {
    /// Promote into Hero at `d ≤ hero_in_m`. Default 100.0.
    pub hero_in_m: f32,
    /// Demote out of Hero at `d > hero_out_m` (20 m dead-zone). Default 120.0.
    pub hero_out_m: f32,
    /// Re-enter Active from Streamed at `d < active_in_m`. Default 80.0.
    pub active_in_m: f32,
    /// Demote out of Active (→ Streamed) at `d > active_out_m`.
    /// Equals the streamer's `evict_radius` (600 m) — REUSE. Default 600.0.
    pub active_out_m: f32,
    /// Re-enter Streamed-band hysteresis at `d < streamed_in_m`. Default 480.0.
    pub streamed_in_m: f32,
    /// Beyond `streamed_out_m` the streamer (not this system) despawns.
    /// Default 5500.0.
    pub streamed_out_m: f32,
    /// Run the switcher once every `cadence_frames` frames. Default 16
    /// (~267 ms worst-case lag at 60 fps — invisible on the frame graph).
    pub cadence_frames: u32,
    /// Per-tier LRU caps (2000 / 20000 / 200000).
    pub caps: TierCaps,
}

impl Default for RenderCascadeConfig {
    fn default() -> Self {
        Self {
            hero_in_m: 100.0,
            hero_out_m: 120.0,
            active_in_m: 80.0,
            active_out_m: 600.0,
            streamed_in_m: 480.0,
            streamed_out_m: 5_500.0,
            cadence_frames: 16,
            caps: TierCaps::default(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RenderCascadeFrame — monotonic frame counter for the cadence gate
// ─────────────────────────────────────────────────────────────────────────────

/// Monotonic frame counter incremented once per `Update` by
/// [`sys_render_cascade`]. The cadence gate fires the expensive scan
/// only when `count % cadence_frames == 0`.
///
/// `u64` wraps after ~9.7 billion years at 60 fps; the modulo arithmetic
/// uses `wrapping_add` for total safety regardless.
#[derive(Resource, Debug, Default, Clone, Copy, Reflect)]
#[reflect(Resource)]
pub struct RenderCascadeFrame {
    /// Frames elapsed since startup (wrapping).
    pub count: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// compute_tier_with_hysteresis — the band switcher (pure fn, unit-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// Decide the candidate tier for one entity given its camera distance and
/// its **current** tier, applying hysteresis dead-zones (§3 pseudocode).
///
/// Hysteresis rule: an entity only leaves a tier at the *out* threshold
/// and only re-enters at the *in* threshold. A fresh entity (`None`) or
/// one currently `Active` is classified by the forward bands.
///
/// Pure and deterministic — the unit tests pin every boundary.
#[inline]
pub fn compute_tier_with_hysteresis(
    d: f32,
    current: Option<RenderTier>,
    cfg: &RenderCascadeConfig,
) -> RenderTier {
    match current {
        // Fresh entity or currently Active: classify by the forward bands.
        None | Some(RenderTier::Active) => {
            if d <= cfg.hero_in_m {
                RenderTier::Hero
            } else if d <= cfg.active_out_m {
                RenderTier::Active
            } else {
                // Beyond active_out_m → Streamed. The streamer keeps it
                // alive in the [active_out_m, streamed_out_m] overlap and
                // despawns past streamed_out_m, so we never observe an
                // entity farther than that.
                RenderTier::Streamed
            }
        }
        // Already Hero: only demote once past the wider out threshold.
        Some(RenderTier::Hero) => {
            if d > cfg.hero_out_m {
                RenderTier::Active
            } else {
                RenderTier::Hero
            }
        }
        // Already Streamed: only re-enter Active once well inside the band.
        Some(RenderTier::Streamed) => {
            if d < cfg.active_in_m {
                RenderTier::Active
            } else {
                // Stay Streamed; the streamer Hot-demotes past
                // evict_radius, not this system.
                RenderTier::Streamed
            }
        }
        // Horizon is never placed on an entity; treat defensively as a
        // fresh classification rather than panicking in a hot system.
        Some(RenderTier::Horizon) => {
            if d <= cfg.hero_in_m {
                RenderTier::Hero
            } else if d <= cfg.active_out_m {
                RenderTier::Active
            } else {
                RenderTier::Streamed
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// enforce_caps — LRU-by-distance cap enforcement (pure fn, unit-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// Enforce per-tier LRU caps over a candidate list in place.
///
/// When more than `cap` entities qualify for a tier, the closest-N (by
/// camera distance, ascending) keep that tier; the overflow is demoted
/// one band outward (Hero→Active, Active→Streamed). Streamed has no
/// outward demotion target here — overflow past its cap simply stays
/// `Streamed` (the streamer's own radius gate is the back-stop).
///
/// `candidates` is `(Entity, tier, distance)`. The order of the slice is
/// not relied upon by callers, so we sort it internally.
pub fn enforce_caps(candidates: &mut [(Entity, RenderTier, f32)], caps: &TierCaps) {
    // Sort once by distance ascending so "closest-N wins" is a prefix
    // count as we sweep. Ties broken by entity index for determinism.
    candidates.sort_by(|a, b| {
        a.2.partial_cmp(&b.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut hero_kept = 0usize;
    let mut active_kept = 0usize;

    for (_e, tier, _d) in candidates.iter_mut() {
        match *tier {
            RenderTier::Hero => {
                if hero_kept < caps.hero {
                    hero_kept += 1;
                } else {
                    // Hero overflow → Active. It then competes for the
                    // Active cap in the same sweep (Active is farther in
                    // the sort, but this entity is among the closest, so
                    // count it against Active immediately).
                    *tier = RenderTier::Active;
                    if active_kept < caps.active {
                        active_kept += 1;
                    } else {
                        *tier = RenderTier::Streamed;
                    }
                }
            }
            RenderTier::Active => {
                if active_kept < caps.active {
                    active_kept += 1;
                } else {
                    *tier = RenderTier::Streamed;
                }
            }
            // Streamed overflow has no nearer-tier promotion and no
            // farther render tier; the streamer caps the absolute count.
            RenderTier::Streamed | RenderTier::Horizon => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// sys_render_cascade — the distance-band tier switcher (W3.2)
// ─────────────────────────────────────────────────────────────────────────────

/// Distance-band tier switcher. Runs every `cadence_frames` frames
/// (default 16 ≈ 4 Hz at 60 fps); assigns [`RenderTier`] per streamed
/// entity from camera distance with hysteresis, then enforces LRU caps.
///
/// Chained **after** `streaming::sys_radius_gate` in `StreamingPlugin`
/// so we only ever tier an entity the streamer has already spawned
/// (constraint C1 / Risk R10 in `RENDER_CASCADE.md`).
///
/// ## FPS guard
/// The cadence gate is the first thing in the body: on 15 of every 16
/// frames it does a single `u64 % u32` test and returns **before any
/// query is iterated**. The entity scan, sort, and cap pass run only on
/// the cadence frame.
///
/// ## Change-minimal writes
/// `RenderTier` is inserted **only when the computed tier differs from
/// the current one**. This keeps `Changed<RenderTier>` (the reactor's
/// filter) firing for genuine transitions only — never the whole entity
/// set every cadence frame.
pub fn sys_render_cascade(
    mut frame: ResMut<RenderCascadeFrame>,
    cfg: Res<RenderCascadeConfig>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    entities: Query<
        (Entity, &GlobalTransform, Option<&RenderTier>),
        With<StreamingInstanceRef>,
    >,
    mut commands: Commands,
) {
    // ── Cadence gate (FPS guard) ───────────────────────────────────────────
    // Increment every frame so the cadence is wall-clock-stable, but bail
    // cheaply on non-cadence frames before touching any query.
    frame.count = frame.count.wrapping_add(1);
    let cadence = cfg.cadence_frames.max(1) as u64;
    if frame.count % cadence != 0 {
        return;
    }

    // Camera anchor (first Camera3d). No camera → nothing to tier.
    let Some(cam_tf) = cameras.iter().next() else {
        return;
    };
    let cam = cam_tf.translation();

    // ── Pass 1: compute candidate tier per entity (cheap distance + hysteresis)
    let mut candidates: Vec<(Entity, RenderTier, f32)> =
        Vec::with_capacity(entities.iter().len());
    for (e, tf, current) in entities.iter() {
        let d = cam.distance(tf.translation());
        let candidate = compute_tier_with_hysteresis(d, current.copied(), &cfg);
        candidates.push((e, candidate, d));
    }

    // ── Pass 2: enforce LRU caps (closest-N wins per tier) ──────────────────
    enforce_caps(&mut candidates, &cfg.caps);

    // ── Pass 3: apply changes (insert ONLY on an actual tier change) ────────
    // Re-fetch each entity's current tier to compare; `candidates` lost the
    // original tier in pass 1, so we look it up via the query's component.
    for (e, new_tier, _d) in candidates {
        let current = entities.get(e).ok().and_then(|(_, _, t)| t.copied());
        if current != Some(new_tier) {
            commands.entity(e).insert(new_tier);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// sys_apply_tier_change — the Changed<RenderTier> reactor (W3.3)
// ─────────────────────────────────────────────────────────────────────────────

/// Per-tier component reactor. Fires only on `Changed<RenderTier>`
/// (i.e. genuine transitions) and updates **visual components only**:
///
/// 1. [`Visibility`] — Hero/Active visible, Streamed/Horizon hidden for
///    this wave (the impostor / panorama swap is W3.9+).
/// 2. [`MeshLodTier`] — the visual mesh-LOD marker, mirrored from the tier
///    so a future mesh-swap system can react to `Changed<MeshLodTier>`.
///
/// ## LOOP 3 — physics-LOD desync breaker (MANDATORY) <a id="reactor-loop-3"></a>
///
/// This reactor **must not** add or remove `RigidBody` or `Collider`.
/// Dropping a `Collider` while a `RigidBody::Dynamic` remains makes the
/// Avian solver step a body with no collider → it falls through the floor
/// → scene corruption (AGENT_DISPATCH.md LOOP 3). Physics LOD is a
/// separate future wave. **Visual components ONLY here.**
///
/// The query deliberately carries no physics component handles, so the
/// constraint is enforced structurally, not just by convention.
pub fn sys_apply_tier_change(
    mut changed: Query<
        (&RenderTier, &mut Visibility, Option<&mut MeshLodTier>),
        Changed<RenderTier>,
    >,
    mut commands: Commands,
    // Entities that gained `RenderTier` but have no `MeshLodTier` yet need
    // one inserted; we resolve those via a second narrow query.
    needs_marker: Query<(Entity, &RenderTier), (Changed<RenderTier>, Without<MeshLodTier>)>,
) {
    for (tier, mut vis, mesh_lod) in changed.iter_mut() {
        // 1. Visibility — visual only.
        let want = if tier.is_visible_this_wave() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        // Avoid spuriously dirtying Visibility when it already matches.
        if *vis != want {
            *vis = want;
        }

        // 2. Mesh-LOD marker — visual only. Update in place if present.
        if let Some(mut lod) = mesh_lod {
            let want_lod = MeshLodTier::from(*tier);
            if *lod != want_lod {
                *lod = want_lod;
            }
        }
        // NOTE: NO RigidBody / Collider mutation here. See LOOP 3 above.
    }

    // Insert MeshLodTier for entities that just received a RenderTier and
    // had none. (A `mut Option<&mut MeshLodTier>` cannot insert; commands can.)
    for (e, tier) in needs_marker.iter() {
        commands.entity(e).insert(MeshLodTier::from(*tier));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RenderCascadeConfig {
        RenderCascadeConfig::default()
    }

    // ── Tier computation with hysteresis at band boundaries ─────────────────

    #[test]
    fn fresh_entity_classified_by_forward_bands() {
        let c = cfg();
        // 0–100 m → Hero.
        assert_eq!(compute_tier_with_hysteresis(0.0, None, &c), RenderTier::Hero);
        assert_eq!(compute_tier_with_hysteresis(100.0, None, &c), RenderTier::Hero);
        // 100–600 m → Active.
        assert_eq!(compute_tier_with_hysteresis(100.01, None, &c), RenderTier::Active);
        assert_eq!(compute_tier_with_hysteresis(600.0, None, &c), RenderTier::Active);
        // > 600 m → Streamed.
        assert_eq!(compute_tier_with_hysteresis(600.01, None, &c), RenderTier::Streamed);
        assert_eq!(compute_tier_with_hysteresis(5_000.0, None, &c), RenderTier::Streamed);
    }

    #[test]
    fn hero_demotes_only_past_the_out_threshold() {
        let c = cfg();
        // Already Hero, inside the 100–120 m dead-zone → STAYS Hero.
        assert_eq!(
            compute_tier_with_hysteresis(110.0, Some(RenderTier::Hero), &c),
            RenderTier::Hero,
            "Hero must not demote inside the hysteresis dead-zone"
        );
        assert_eq!(
            compute_tier_with_hysteresis(120.0, Some(RenderTier::Hero), &c),
            RenderTier::Hero
        );
        // Past 120 m → demote to Active.
        assert_eq!(
            compute_tier_with_hysteresis(120.01, Some(RenderTier::Hero), &c),
            RenderTier::Active
        );
    }

    #[test]
    fn streamed_reenters_active_only_well_inside_the_band() {
        let c = cfg();
        // Already Streamed at 90 m: still ≥ active_in_m (80) → STAYS Streamed
        // (the dead-zone between 80 and 600 prevents re-entry chatter).
        assert_eq!(
            compute_tier_with_hysteresis(90.0, Some(RenderTier::Streamed), &c),
            RenderTier::Streamed,
            "Streamed must not re-enter Active inside the hysteresis dead-zone"
        );
        assert_eq!(
            compute_tier_with_hysteresis(80.0, Some(RenderTier::Streamed), &c),
            RenderTier::Streamed
        );
        // Inside active_in_m → re-enter Active.
        assert_eq!(
            compute_tier_with_hysteresis(79.99, Some(RenderTier::Streamed), &c),
            RenderTier::Active
        );
        // Far away → stays Streamed (streamer handles the despawn).
        assert_eq!(
            compute_tier_with_hysteresis(5_400.0, Some(RenderTier::Streamed), &c),
            RenderTier::Streamed
        );
    }

    #[test]
    fn no_oscillation_for_stationary_entity_at_hero_band_edge() {
        let c = cfg();
        // A player parked at exactly 100.0 m: a fresh classify → Hero.
        let mut tier = compute_tier_with_hysteresis(100.0, None, &c);
        assert_eq!(tier, RenderTier::Hero);
        // Repeatedly re-evaluating at the same distance must be a fixed
        // point — no Hero↔Active flicker (Risk R4).
        for _ in 0..10 {
            tier = compute_tier_with_hysteresis(100.0, Some(tier), &c);
            assert_eq!(tier, RenderTier::Hero);
        }
        // And just over the in-threshold but under the out-threshold, a
        // Hero entity is still a fixed point.
        let mut tier = RenderTier::Hero;
        for _ in 0..10 {
            tier = compute_tier_with_hysteresis(115.0, Some(tier), &c);
            assert_eq!(tier, RenderTier::Hero);
        }
    }

    #[test]
    fn active_band_is_stable_in_its_interior() {
        let c = cfg();
        let mut tier = RenderTier::Active;
        for _ in 0..10 {
            tier = compute_tier_with_hysteresis(300.0, Some(tier), &c);
            assert_eq!(tier, RenderTier::Active);
        }
    }

    // ── LRU cap enforcement ─────────────────────────────────────────────────

    fn ent(i: u32) -> Entity {
        // bevy 0.18 renamed the infallible `Entity::from_raw(u32)` to the
        // validating `from_raw_u32(u32) -> Option<Entity>`; unwrap is fine
        // in this test helper (indices are small and always valid).
        Entity::from_raw_u32(i).expect("valid test entity index")
    }

    #[test]
    fn hero_cap_keeps_closest_n_demotes_rest_to_active() {
        let caps = TierCaps { hero: 2, active: 100, streamed: 1000 };
        // Three Hero candidates at increasing distance; cap is 2.
        let mut cands = vec![
            (ent(0), RenderTier::Hero, 30.0),
            (ent(1), RenderTier::Hero, 10.0),
            (ent(2), RenderTier::Hero, 20.0),
        ];
        enforce_caps(&mut cands, &caps);
        // After sort by distance: ent1(10), ent2(20), ent0(30).
        // Closest two stay Hero, farthest demoted to Active.
        let tier_of = |id: u32| cands.iter().find(|(e, _, _)| *e == ent(id)).unwrap().1;
        assert_eq!(tier_of(1), RenderTier::Hero, "closest stays Hero");
        assert_eq!(tier_of(2), RenderTier::Hero, "second-closest stays Hero");
        assert_eq!(tier_of(0), RenderTier::Active, "farthest demoted out of Hero");
    }

    #[test]
    fn active_cap_overflow_demotes_to_streamed() {
        let caps = TierCaps { hero: 100, active: 1, streamed: 1000 };
        let mut cands = vec![
            (ent(0), RenderTier::Active, 200.0),
            (ent(1), RenderTier::Active, 150.0),
        ];
        enforce_caps(&mut cands, &caps);
        let tier_of = |id: u32| cands.iter().find(|(e, _, _)| *e == ent(id)).unwrap().1;
        assert_eq!(tier_of(1), RenderTier::Active, "closest stays Active");
        assert_eq!(tier_of(0), RenderTier::Streamed, "overflow demoted to Streamed");
    }

    #[test]
    fn hero_overflow_cascades_through_active_into_streamed() {
        // Hero cap 1, Active cap 1: 3 Hero candidates.
        // Closest → Hero. Next → Hero-overflow → Active (fills active cap).
        // Third → Hero-overflow → Active-full → Streamed.
        let caps = TierCaps { hero: 1, active: 1, streamed: 1000 };
        let mut cands = vec![
            (ent(0), RenderTier::Hero, 5.0),
            (ent(1), RenderTier::Hero, 10.0),
            (ent(2), RenderTier::Hero, 15.0),
        ];
        enforce_caps(&mut cands, &caps);
        let tier_of = |id: u32| cands.iter().find(|(e, _, _)| *e == ent(id)).unwrap().1;
        assert_eq!(tier_of(0), RenderTier::Hero);
        assert_eq!(tier_of(1), RenderTier::Active);
        assert_eq!(tier_of(2), RenderTier::Streamed);
    }

    #[test]
    fn under_cap_leaves_every_tier_untouched() {
        let caps = TierCaps::default();
        let mut cands = vec![
            (ent(0), RenderTier::Hero, 5.0),
            (ent(1), RenderTier::Active, 300.0),
            (ent(2), RenderTier::Streamed, 2_000.0),
        ];
        let before = cands.clone();
        enforce_caps(&mut cands, &caps);
        // Same membership, tiers unchanged (order may differ post-sort).
        for (e, t, _) in before {
            let now = cands.iter().find(|(ee, _, _)| *ee == e).unwrap().1;
            assert_eq!(now, t, "under-cap entity {e:?} must keep its tier");
        }
    }

    #[test]
    fn streamed_overflow_stays_streamed() {
        // Streamed cap of 1, two Streamed candidates: the streamer (not
        // this fn) is the absolute back-stop, so both stay Streamed.
        let caps = TierCaps { hero: 100, active: 100, streamed: 1 };
        let mut cands = vec![
            (ent(0), RenderTier::Streamed, 1_000.0),
            (ent(1), RenderTier::Streamed, 2_000.0),
        ];
        enforce_caps(&mut cands, &caps);
        assert!(cands.iter().all(|(_, t, _)| *t == RenderTier::Streamed));
    }

    // ── 16-frame cadence gate ───────────────────────────────────────────────

    #[test]
    fn cadence_gate_fires_once_every_n_frames() {
        // Mirror the gate arithmetic exactly: increment-then-test, fire when
        // count % cadence == 0.
        let cadence: u64 = cfg().cadence_frames.max(1) as u64;
        assert_eq!(cadence, 16);
        let mut count: u64 = 0;
        let mut fires = 0usize;
        for _ in 0..160 {
            count = count.wrapping_add(1);
            if count % cadence == 0 {
                fires += 1;
            }
        }
        // 160 frames / 16 = exactly 10 cadence fires.
        assert_eq!(fires, 10, "cadence gate must fire exactly once per 16 frames");
    }

    #[test]
    fn cadence_gate_short_circuits_on_non_cadence_frames() {
        let cadence: u64 = 16;
        // Frames 1..=15 must NOT fire; frame 16 must.
        for count in 1u64..16 {
            assert_ne!(count % cadence, 0, "frame {count} must short-circuit");
        }
        assert_eq!(16u64 % cadence, 0, "frame 16 is the cadence frame");
    }

    #[test]
    fn cadence_of_one_runs_every_frame() {
        // Defensive: cadence_frames clamps to >= 1, so cadence==1 fires
        // every frame (count % 1 == 0 always).
        let cadence: u64 = 1u32.max(1) as u64;
        for count in 1u64..=5 {
            assert_eq!(count % cadence, 0);
        }
    }

    // ── Tier → visual mappings ──────────────────────────────────────────────

    #[test]
    fn visibility_policy_matches_wave_scope() {
        assert!(RenderTier::Hero.is_visible_this_wave());
        assert!(RenderTier::Active.is_visible_this_wave());
        assert!(!RenderTier::Streamed.is_visible_this_wave());
        assert!(!RenderTier::Horizon.is_visible_this_wave());
    }

    #[test]
    fn mesh_lod_mirrors_render_tier() {
        assert_eq!(MeshLodTier::from(RenderTier::Hero), MeshLodTier::Lod0);
        assert_eq!(MeshLodTier::from(RenderTier::Active), MeshLodTier::Lod1);
        assert_eq!(MeshLodTier::from(RenderTier::Streamed), MeshLodTier::Lod3);
    }
}
