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
//! - **Shadows:** a light casts only if it was AUTHORED to (`shadows` on its
//!   Eustress light class), and of those only the nearest
//!   [`SHADOW_LIGHT_BUDGET`] keep `shadow_maps_enabled = true`. This is the
//!   single biggest lever — it collapses thousands of shadow maps to a few
//!   dozen — and it never adds a shadow the author switched off.
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
//! Visual-only and fully reversible: it mutates only `shadow_maps_enabled` and
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

/// Nearest-N lights that keep `shadow_maps_enabled = true`. Every other light's
/// shadows are turned off. Conservative: the closest set the player is most
/// likely looking at keeps real shadows.
const SHADOW_LIGHT_BUDGET: usize = 32;

/// Nearest-N lights that keep their authored intensity. Beyond this rank (or
/// beyond the hysteresis radius) a light is dimmed to 0.
///
/// PERF: cut from 256 → 64. Every active (intensity>0) clusterable light feeds
/// `assign_objects_to_clusters`, `prepare/extract_clusters`, `queue_shadows`
/// and `specialize_shadows`. 64 nearest-by-camera lights keep local lighting
/// looking authored (the camera's immediate surroundings are fully lit) while
/// roughly quartering the clustered-forward + shadow-queue cost. Distant lights
/// are dimmed, not despawned, and snap back to authored intensity on approach
/// via the hysteresis band below.
const ACTIVE_LIGHT_BUDGET: usize = 64;

/// A light beyond this distance from the camera is dimmed (turned off). Used
/// as the ON edge of the hysteresis band: a dimmed light only relights once
/// it is back inside this radius AND within the nearest-`ACTIVE_LIGHT_BUDGET`.
const ACTIVE_ON_RADIUS_M: f32 = 250.0;

/// The OFF edge of the hysteresis band: a lit light is only dimmed once it
/// passes beyond this radius (or drops out of the nearest set). `>` the ON
/// radius so there is a dead-zone and lights do not flicker at the boundary.
const ACTIVE_OFF_RADIUS_M: f32 = 350.0;

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


/// A light's AUTHORED shadow flag, stashed the first time the culler (or the
/// load-time budget) meets a light that has no Eustress light class to read
/// the flag from (a raw Bevy light spawned by some other path). Class-backed
/// lights read the flag straight from `EustressPointLight` /
/// `EustressSpotLight` / `SurfaceLight`, so they never get one.
#[derive(Component, Debug, Clone, Copy)]
pub struct AuthoredLightShadows(pub bool);

/// The authored shadow flag of a point-emitting light, if it can be known
/// without guessing: from its class component, else from an earlier stash.
/// A light switched off (`enabled = false`) never casts.
fn authored_point_shadows(
    class: Option<&eustress_common::classes::EustressPointLight>,
    surface: Option<&eustress_common::classes::SurfaceLight>,
    stash: Option<&AuthoredLightShadows>,
) -> Option<bool> {
    class
        .map(|c| c.shadows && c.enabled)
        .or_else(|| surface.map(|s| s.shadows && s.enabled))
        .or_else(|| stash.map(|s| s.0))
}

/// The authored shadow flag of a spot light (class, else stash).
fn authored_spot_shadows(
    class: Option<&eustress_common::classes::EustressSpotLight>,
    stash: Option<&AuthoredLightShadows>,
) -> Option<bool> {
    class.map(|c| c.shadows && c.enabled).or_else(|| stash.map(|s| s.0))
}

/// Whether a light is switched on. Only class-backed lights can be off; the
/// light sync keeps an off light at zero intensity with no shadow map, so
/// the culler leaves it alone (dimming one would stash its zero intensity
/// and "restore" darkness after the user switches it back on).
fn light_enabled(
    point: Option<&eustress_common::classes::EustressPointLight>,
    spot: Option<&eustress_common::classes::EustressSpotLight>,
    surface: Option<&eustress_common::classes::SurfaceLight>,
) -> bool {
    point.map_or(true, |c| c.enabled)
        && spot.map_or(true, |c| c.enabled)
        && surface.map_or(true, |s| s.enabled)
}

/// Rank lights by distance to the order-0 camera and apply the shadow +
/// intensity budgets. See the module docs for the policy.
///
/// Shadows follow the AUTHORED flag. A light authored with `shadows = false`
/// never casts, and [`SHADOW_LIGHT_BUDGET`] ranks only the lights authored
/// with shadows. This used to hand shadows to the nearest 32 lights whatever
/// they were authored with: Super Station's 49 lights are all authored
/// `shadows = false`, and that rendered ~150 cube-face shadow maps every
/// frame (158 render views, a 490 ms render thread) for shadows the scene
/// had switched off.
///
/// `Local<CullGate>` keeps the cadence/movement state without a registered
/// resource. A light is only written when its state actually changes, so an
/// evaluation that changes nothing marks nothing `Changed`.
#[allow(clippy::type_complexity)]
pub fn cull_lights_to_nearest(
    mut gate: Local<CullGate>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut point_lights: Query<(
        Entity,
        &GlobalTransform,
        &mut PointLight,
        Option<&OriginalLightIntensity>,
        Option<&eustress_common::classes::EustressPointLight>,
        Option<&eustress_common::classes::SurfaceLight>,
        Option<&AuthoredLightShadows>,
    )>,
    mut spot_lights: Query<(
        Entity,
        &GlobalTransform,
        &mut SpotLight,
        Option<&OriginalLightIntensity>,
        Option<&eustress_common::classes::EustressSpotLight>,
        Option<&AuthoredLightShadows>,
    )>,
    mut commands: Commands,
) {
    // Order-0 camera = the window/editor camera (the AI camera is order 1,
    // the Slint overlay is order 100). Cull relative to what the user sees.
    let Some(cam_pos) = cameras
        .iter()
        .find(|(c, _)| c.order == 0)
        .map(|(_, gt)| gt.translation())
    else {
        return; // no main camera yet (early boot) — try again next frame
    };

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

    #[derive(Clone, Copy)]
    enum Kind {
        Point,
        Spot,
    }

    // Gather (distance², entity, kind, authored shadows). A light with no
    // class component and no stash is met for the first time: nothing has
    // changed its shadow flag yet (the load-time budget stashes before it
    // turns one off), so its live flag IS the authored one; stash it.
    let mut ranked: Vec<(f32, Entity, Kind, bool)> =
        Vec::with_capacity(point_lights.iter().len() + spot_lights.iter().len());
    for (e, gt, light, _, class, surface, stash) in point_lights.iter() {
        if !light_enabled(class, None, surface) {
            continue;
        }
        let authored = match authored_point_shadows(class, surface, stash) {
            Some(a) => a,
            None => {
                commands
                    .entity(e)
                    .insert(AuthoredLightShadows(light.shadow_maps_enabled));
                light.shadow_maps_enabled
            }
        };
        ranked.push((gt.translation().distance_squared(cam_pos), e, Kind::Point, authored));
    }
    for (e, gt, light, _, class, stash) in spot_lights.iter() {
        if !light_enabled(None, class, None) {
            continue;
        }
        let authored = match authored_spot_shadows(class, stash) {
            Some(a) => a,
            None => {
                commands
                    .entity(e)
                    .insert(AuthoredLightShadows(light.shadow_maps_enabled));
                light.shadow_maps_enabled
            }
        };
        ranked.push((gt.translation().distance_squared(cam_pos), e, Kind::Spot, authored));
    }
    // Nearest first. `total_cmp` handles any NaN deterministically.
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));

    let on_radius_sq = ACTIVE_ON_RADIUS_M * ACTIVE_ON_RADIUS_M;
    let off_radius_sq = ACTIVE_OFF_RADIUS_M * ACTIVE_OFF_RADIUS_M;

    // The shadow budget ranks only lights authored to cast shadows.
    let mut shadow_rank = 0usize;
    for (rank, (dist_sq, entity, kind, authored_shadows)) in ranked.iter().enumerate() {
        let want_shadows = *authored_shadows && shadow_rank < SHADOW_LIGHT_BUDGET;
        if *authored_shadows {
            shadow_rank += 1;
        }
        let within_active_rank = rank < ACTIVE_LIGHT_BUDGET;
        match kind {
            Kind::Point => {
                if let Ok((_, _, mut light, original, _, _, _)) = point_lights.get_mut(*entity) {
                    let d = decide_light_policy(
                        light.shadow_maps_enabled,
                        light.intensity,
                        original.copied(),
                        want_shadows,
                        within_active_rank,
                        *dist_sq,
                        on_radius_sq,
                        off_radius_sq,
                    );
                    if light.shadow_maps_enabled != d.shadows {
                        light.shadow_maps_enabled = d.shadows;
                    }
                    if light.intensity != d.intensity {
                        light.intensity = d.intensity;
                    }
                    if let Some(v) = d.stash_intensity {
                        commands.entity(*entity).insert(OriginalLightIntensity(v));
                    }
                }
            }
            Kind::Spot => {
                if let Ok((_, _, mut light, original, _, _)) = spot_lights.get_mut(*entity) {
                    let d = decide_light_policy(
                        light.shadow_maps_enabled,
                        light.intensity,
                        original.copied(),
                        want_shadows,
                        within_active_rank,
                        *dist_sq,
                        on_radius_sq,
                        off_radius_sq,
                    );
                    if light.shadow_maps_enabled != d.shadows {
                        light.shadow_maps_enabled = d.shadows;
                    }
                    if light.intensity != d.intensity {
                        light.intensity = d.intensity;
                    }
                    if let Some(v) = d.stash_intensity {
                        commands.entity(*entity).insert(OriginalLightIntensity(v));
                    }
                }
            }
        }
    }
}

/// Hard backstop: cap shadow-casting point + spot lights to
/// [`SHADOW_LIGHT_BUDGET`] BEFORE the render world prepares shadow maps.
///
/// A large import (Vehicle Simulator has thousands of shadow-casting
/// spotlights — light poles, headlights) otherwise makes `prepare_lights`
/// allocate one shadow map per caster and OOMs the GPU **during load**, before
/// the gated [`cull_lights_to_nearest`] cadence can react (it panicked
/// `wgpu error: Out of Memory` in `bevy_pbr::render::light::prepare_lights`).
///
/// Runs in `PostUpdate` (after every spawn, before render extract) and ONLY
/// while a Space is still streaming in (`LoadInProgress.active`); once loaded,
/// `cull_lights_to_nearest` maintains the nearest-N set, so this is a no-op with
/// zero steady-state cost. It does not distance-rank (it keeps the first
/// `budget` casters it visits) — the proper nearest-N ranking is applied by
/// `cull_lights_to_nearest` on its cadence; this only guarantees the COUNT can
/// never exceed the budget on any single frame. Before it turns off a light
/// that has no class component to read the authored flag from, it stashes
/// the flag, so the culler never mistakes a budget cut for an authored `false`.
#[allow(clippy::type_complexity)]
pub fn enforce_shadow_budget(
    load: Option<Res<crate::space::file_loader::LoadInProgress>>,
    mut point_lights: Query<(
        Entity,
        &mut PointLight,
        Option<&eustress_common::classes::EustressPointLight>,
        Option<&eustress_common::classes::SurfaceLight>,
        Option<&AuthoredLightShadows>,
    )>,
    mut spot_lights: Query<(
        Entity,
        &mut SpotLight,
        Option<&eustress_common::classes::EustressSpotLight>,
        Option<&AuthoredLightShadows>,
    )>,
    mut commands: Commands,
) {
    // Only needed while lights are still streaming in.
    if !load.map_or(false, |l| l.active) {
        return;
    }
    let mut kept = 0usize;
    for (e, mut light, class, surface, stash) in point_lights.iter_mut() {
        if light.shadow_maps_enabled {
            if kept < SHADOW_LIGHT_BUDGET {
                kept += 1;
            } else {
                if authored_point_shadows(class, surface, stash).is_none() {
                    commands.entity(e).insert(AuthoredLightShadows(true));
                }
                light.shadow_maps_enabled = false;
            }
        }
    }
    for (e, mut light, class, stash) in spot_lights.iter_mut() {
        if light.shadow_maps_enabled {
            if kept < SHADOW_LIGHT_BUDGET {
                kept += 1;
            } else {
                if authored_spot_shadows(class, stash).is_none() {
                    commands.entity(e).insert(AuthoredLightShadows(true));
                }
                light.shadow_maps_enabled = false;
            }
        }
    }
}

/// What the policy wants a light to be.
struct LightDecision {
    shadows: bool,
    intensity: f32,
    /// The authored intensity to stash the first time a light is dimmed.
    stash_intensity: Option<f32>,
}

/// Shared per-light decision used by both the point and spot branches.
///
/// `shadows` / `intensity` are the light's live values. `original` is the
/// stashed authored intensity (if this light was ever dimmed before). The
/// caller writes back only the fields that differ, and inserts
/// [`OriginalLightIntensity`] when `stash_intensity` is set, so the authored
/// value can be restored later.
#[allow(clippy::too_many_arguments)]
fn decide_light_policy(
    shadows: bool,
    intensity: f32,
    original: Option<OriginalLightIntensity>,
    want_shadows: bool,
    within_active_rank: bool,
    dist_sq: f32,
    on_radius_sq: f32,
    off_radius_sq: f32,
) -> LightDecision {
    let _ = shadows; // shadows need no hysteresis: the budget decides directly
    // Authored intensity: the value we restore TO. If we already stashed one,
    // that is the source of truth; otherwise the live value is still authored.
    let authored = original.map(|o| o.0).unwrap_or(intensity);

    // Current on/off state inferred from the live intensity: a light we
    // previously dimmed reads ~0 (and carries an OriginalLightIntensity).
    let currently_off = original.is_some() && intensity <= f32::EPSILON;

    // Desired state with hysteresis:
    //   - turn ON  only when within the nearest set AND inside the ON radius
    //   - turn OFF only when outside the nearest set OR beyond the OFF radius
    //   - otherwise hold (dead-zone) to avoid flicker.
    let mut out = LightDecision { shadows: want_shadows, intensity, stash_intensity: None };
    if currently_off {
        if within_active_rank && dist_sq <= on_radius_sq {
            out.intensity = authored;
        }
    } else if !within_active_rank || dist_sq > off_radius_sq {
        // Stash the authored intensity once so we can restore it exactly.
        if original.is_none() {
            out.stash_intensity = Some(intensity);
        }
        out.intensity = 0.0;
    }
    out
}
