//! Nearest-N light culling (perf quick-win QW1 + QW2).
//!
//! A freshly-imported large Roblox place can carry thousands of
//! `PointLight` / `SpotLight` entities (the diagnosed case: ~4,430). Each
//! shadow-casting light is a full shadow-map render pass, and every light
//! within a camera frustum cluster costs in the clustered-forward lighting
//! pass — so 4,430 live lights crater the frame rate (~0.1 FPS observed).
//!
//! This system keeps the *visual* result close to authored while collapsing
//! that cost, by ranking lights by distance to the **order-0** (window)
//! camera and:
//!
//! - **Shadows:** only the nearest [`SHADOW_LIGHT_BUDGET`] keep
//!   `shadows_enabled = true`; all others have shadows turned off. This is
//!   the single biggest lever — it collapses thousands of shadow maps to a
//!   few dozen.
//! - **Active intensity:** only the nearest [`ACTIVE_LIGHT_BUDGET`] *and*
//!   within a hysteresis radius keep their authored intensity. Lights that
//!   fall outside are dimmed to `intensity = 0.0` so they drop out of the
//!   clustered-forward cost. Their authored intensity is stashed once in
//!   [`OriginalLightIntensity`] so re-entering the active set restores it
//!   exactly.
//!
//! ## Hysteresis
//!
//! A light toggles ON only inside [`ACTIVE_ON_RADIUS_M`] and toggles OFF
//! only beyond [`ACTIVE_OFF_RADIUS_M`] (a dead-zone band, mirroring
//! `space::residency`'s load/evict radii). This stops a light from
//! flickering on and off as the camera hovers near the boundary.
//!
//! ## Cost / cadence
//!
//! The work is gated: it runs only when the order-0 camera has moved past a
//! small dead-zone, or at most once every [`FORCE_INTERVAL_FRAMES`] frames.
//! A frame where neither triggers does nothing. The per-run cost is one
//! gather + one partial-style sort of the light set — negligible next to a
//! single shadow-map pass.
//!
//! ## Safety
//!
//! Visual-only and fully reversible: it mutates only `shadows_enabled` and
//! `intensity` on lights, never despawns, and restores authored intensity
//! from the stored component. The DirectionalLight sun/moon are untouched
//! (this only queries `PointLight` / `SpotLight`).

use bevy::prelude::*;

/// Authored light intensity, stashed the first time a light is culled so it
/// can be restored exactly when the light re-enters the active set. Inserted
/// lazily (on first dim) — a light that never leaves the active set never
/// gets one, so this is free for small scenes.
#[derive(Component, Debug, Clone, Copy)]
pub struct OriginalLightIntensity(pub f32);

/// Nearest-N lights that keep `shadows_enabled = true`. Every other light's
/// shadows are turned off. Conservative: the closest set the player is most
/// likely looking at keeps real shadows.
const SHADOW_LIGHT_BUDGET: usize = 32;

/// Nearest-N lights that keep their authored intensity. Beyond this rank (or
/// beyond the hysteresis radius) a light is dimmed to 0.
const ACTIVE_LIGHT_BUDGET: usize = 256;

/// A light beyond this distance from the camera is dimmed (turned off). Used
/// as the ON edge of the hysteresis band: a dimmed light only relights once
/// it is back inside this radius AND within the nearest-`ACTIVE_LIGHT_BUDGET`.
const ACTIVE_ON_RADIUS_M: f32 = 350.0;

/// The OFF edge of the hysteresis band: a lit light is only dimmed once it
/// passes beyond this radius (or drops out of the nearest set). `>` the ON
/// radius so there is a dead-zone and lights do not flicker at the boundary.
const ACTIVE_OFF_RADIUS_M: f32 = 450.0;

/// The camera must move at least this far (squared, m²) from the last
/// evaluated position to force a re-cull on movement. Avoids re-running every
/// frame for a near-stationary camera. ~5 m of travel.
const CAMERA_MOVE_DEADZONE_SQ: f32 = 25.0;

/// Hard cadence cap: even a perfectly still camera re-evaluates at least this
/// often, so lights settle after a scene streams in around a parked camera.
const FORCE_INTERVAL_FRAMES: u32 = 30;

/// Per-run state for the cull system. Tracks the last camera position used
/// for an evaluation and a frame counter for the cadence gate.
#[derive(Default)]
pub struct CullGate {
    last_camera_pos: Option<Vec3>,
    frames_since_run: u32,
}

/// Rank lights by distance to the order-0 camera and apply the shadow +
/// intensity budgets. See the module docs for the policy.
///
/// `Local<CullGate>` keeps the cadence/movement state without a registered
/// resource. Both light queries are `Option`-free `&GlobalTransform` reads
/// plus a `&mut` on the light component.
pub fn cull_lights_to_nearest(
    mut gate: Local<CullGate>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut point_lights: Query<
        (Entity, &GlobalTransform, &mut PointLight, Option<&OriginalLightIntensity>),
    >,
    mut spot_lights: Query<
        (Entity, &GlobalTransform, &mut SpotLight, Option<&OriginalLightIntensity>),
    >,
    mut commands: Commands,
) {
    // ── Find the order-0 (window) camera. The AI camera is order -1 and
    //    the Slint overlay / other cameras are not order 0, so this only
    //    ever tracks the editor viewport camera. ────────────────────────
    let Some(cam_pos) = cameras
        .iter()
        .find(|(c, _)| c.order == 0)
        .map(|(_, gt)| gt.translation())
    else {
        return; // no main camera yet (early boot) — try again next frame
    };

    // ── Cadence gate: run on meaningful camera movement OR every
    //    FORCE_INTERVAL_FRAMES, whichever first. ─────────────────────────
    gate.frames_since_run = gate.frames_since_run.saturating_add(1);
    let moved_enough = match gate.last_camera_pos {
        Some(prev) => prev.distance_squared(cam_pos) >= CAMERA_MOVE_DEADZONE_SQ,
        None => true, // first run
    };
    if !moved_enough && gate.frames_since_run < FORCE_INTERVAL_FRAMES {
        return;
    }
    gate.frames_since_run = 0;
    gate.last_camera_pos = Some(cam_pos);

    // ── Gather (entity, distance²) for every light, tagged by kind, then
    //    rank by distance. A `Vec` sort of a few thousand entries is cheap
    //    versus one shadow-map pass; we only do it on the gated cadence. ──
    #[derive(Clone, Copy)]
    enum Kind {
        Point,
        Spot,
    }
    let mut ranked: Vec<(f32, Entity, Kind)> =
        Vec::with_capacity(point_lights.iter().len() + spot_lights.iter().len());
    for (e, gt, _, _) in point_lights.iter() {
        ranked.push((gt.translation().distance_squared(cam_pos), e, Kind::Point));
    }
    for (e, gt, _, _) in spot_lights.iter() {
        ranked.push((gt.translation().distance_squared(cam_pos), e, Kind::Spot));
    }
    // Ascending by distance². `total_cmp` is finite-safe (NaN positions sort
    // last rather than panicking the comparator).
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));

    let on_radius_sq = ACTIVE_ON_RADIUS_M * ACTIVE_ON_RADIUS_M;
    let off_radius_sq = ACTIVE_OFF_RADIUS_M * ACTIVE_OFF_RADIUS_M;

    // Apply per-rank policy. `rank` is the distance order across BOTH light
    // kinds, so the global nearest-N budgets are honoured.
    for (rank, (dist_sq, entity, kind)) in ranked.iter().enumerate() {
        let want_shadows = rank < SHADOW_LIGHT_BUDGET;
        let within_active_rank = rank < ACTIVE_LIGHT_BUDGET;

        match kind {
            Kind::Point => {
                if let Ok((_, _, light, original)) = point_lights.get_mut(*entity) {
                    // Deref the change-detection `Mut` to a plain `&mut` once so
                    // the two field args below are disjoint borrows, not two
                    // full `&mut light` borrows through `DerefMut`.
                    let light = light.into_inner();
                    apply_light_policy(
                        &mut commands,
                        *entity,
                        &mut light.shadows_enabled,
                        &mut light.intensity,
                        original.copied(),
                        want_shadows,
                        within_active_rank,
                        *dist_sq,
                        on_radius_sq,
                        off_radius_sq,
                    );
                }
            }
            Kind::Spot => {
                if let Ok((_, _, light, original)) = spot_lights.get_mut(*entity) {
                    // Deref the change-detection `Mut` to a plain `&mut` once so
                    // the two field args below are disjoint borrows, not two
                    // full `&mut light` borrows through `DerefMut`.
                    let light = light.into_inner();
                    apply_light_policy(
                        &mut commands,
                        *entity,
                        &mut light.shadows_enabled,
                        &mut light.intensity,
                        original.copied(),
                        want_shadows,
                        within_active_rank,
                        *dist_sq,
                        on_radius_sq,
                        off_radius_sq,
                    );
                }
            }
        }
    }
}

/// Shared per-light decision used by both the point and spot branches.
///
/// `shadows` / `intensity` are `&mut` into the live light component.
/// `original` is the stashed authored intensity (if this light was ever
/// dimmed before). The function decides the new shadow + intensity state and,
/// when it first needs to dim a light, inserts [`OriginalLightIntensity`] via
/// `commands` so the authored value can be restored later.
#[allow(clippy::too_many_arguments)]
fn apply_light_policy(
    commands: &mut Commands,
    entity: Entity,
    shadows: &mut bool,
    intensity: &mut f32,
    original: Option<OriginalLightIntensity>,
    want_shadows: bool,
    within_active_rank: bool,
    dist_sq: f32,
    on_radius_sq: f32,
    off_radius_sq: f32,
) {
    // Shadows: cheap boolean, no hysteresis needed (toggling a shadow map on
    // costs a frame's render but not a flicker artifact). Only mutate on an
    // actual change so we don't spuriously mark the component Changed.
    if *shadows != want_shadows {
        *shadows = want_shadows;
    }

    // Authored intensity: the value we restore TO. If we already stashed one,
    // that is the source of truth; otherwise the live value is still authored.
    let authored = original.map(|o| o.0).unwrap_or(*intensity);

    // Current on/off state inferred from the live intensity: a light we
    // previously dimmed reads ~0 (and carries an OriginalLightIntensity).
    let currently_off = original.is_some() && *intensity <= f32::EPSILON;

    // Desired state with hysteresis:
    //   - turn ON  only when within the nearest set AND inside the ON radius
    //   - turn OFF only when outside the nearest set OR beyond the OFF radius
    //   - otherwise hold (dead-zone) to avoid flicker.
    if currently_off {
        let should_turn_on = within_active_rank && dist_sq <= on_radius_sq;
        if should_turn_on {
            *intensity = authored;
        }
        // else: stay off (still out of range / out of rank).
    } else {
        let should_turn_off = !within_active_rank || dist_sq > off_radius_sq;
        if should_turn_off {
            // Stash the authored intensity once so we can restore it exactly.
            if original.is_none() {
                commands
                    .entity(entity)
                    .insert(OriginalLightIntensity(*intensity));
            }
            *intensity = 0.0;
        }
        // else: stay on at authored intensity (no write needed).
    }
}
