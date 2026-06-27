//! # Light-class sync — resolve Eustress light components to real Bevy lights
//!
//! A single source of truth that keeps a real `bevy_pbr` light in lockstep with
//! each Eustress light **authoring** component. For every entity carrying an
//! `EustressPointLight` / `EustressSpotLight` / `EustressDirectionalLight` /
//! `SurfaceLight`, this:
//!
//! 1. **Hydrates** — inserts the matching Bevy light if the entity doesn't have
//!    one yet, so EVERY spawn path lights up uniformly (TOML file_loader, UI
//!    insert, binary-ECS, Luau, clipboard paste, scene deserialize). Previously
//!    only some paths attached the Bevy component.
//! 2. **Syncs** — on `Changed<…>` (which also fires on spawn/`Added`), rewrites
//!    the Bevy light from the authoring component, so editing brightness / color
//!    / range / angle / shadows in the Properties panel takes effect live. There
//!    was no such watcher before (only a `// not yet built` comment in
//!    `spawners/lights/point_light.rs`).
//!
//! ## Brightness units (why placed lights used to be invisible)
//!
//! Authoring `brightness` is a Roblox-style `0..N` dial — every class template
//! ships `brightness = 1.0`. Bevy point/spot **intensity** is in lumens and
//! directional **illuminance** is in lux, both physically large. The legacy
//! `spawn.rs` fed `brightness` straight in as lumens, so a freshly-placed
//! PointLight emitted `1` lumen — effectively black. We scale here instead, so a
//! template light (`brightness = 1.0`) is clearly lit, and the dial stays
//! intuitive (≈5 ≈ bright indoor). The two constants below are the tunables.

use bevy::prelude::*;

use eustress_common::classes::{
    EustressDirectionalLight, EustressPointLight, EustressSpotLight, SurfaceLight,
};

/// Lumens per unit `brightness` for point & spot (and surface) lights.
/// `brightness = 1.0` → 50 000 lm (clearly lit); `2.0` → 100 000 lm, which is
/// the old "bright indoor light" calibration. Tune here if lights read too
/// hot/cold across a Space.
pub const LUMENS_PER_BRIGHTNESS: f32 = 50_000.0;

/// Lux per unit `brightness` for directional lights. `brightness = 1.0` →
/// 10 000 lux (overcast daylight); `2.0` → bright clear day. Matches the legacy
/// `illuminance = brightness * 10_000` factor so existing suns are unchanged.
pub const LUX_PER_BRIGHTNESS: f32 = 10_000.0;

/// Registers the four light-class sync systems on `Update`.
pub struct LightClassPlugin;

impl Plugin for LightClassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                sync_point_lights,
                sync_spot_lights,
                sync_directional_lights,
                sync_surface_lights,
            ),
        );
    }
}

/// `EustressPointLight` → `bevy_pbr::PointLight`.
fn sync_point_lights(
    mut commands: Commands,
    mut query: Query<
        (Entity, &EustressPointLight, Option<&mut PointLight>),
        Changed<EustressPointLight>,
    >,
) {
    for (entity, light, existing) in &mut query {
        let intensity = light.brightness * LUMENS_PER_BRIGHTNESS;
        if let Some(mut pl) = existing {
            pl.color = light.color;
            pl.intensity = intensity;
            pl.range = light.range;
            pl.radius = light.radius;
            pl.shadows_enabled = light.shadows;
        } else {
            commands.entity(entity).insert(PointLight {
                color: light.color,
                intensity,
                range: light.range,
                radius: light.radius,
                shadows_enabled: light.shadows,
                ..default()
            });
        }
    }
}

/// `EustressSpotLight` → `bevy_pbr::SpotLight`. `angle` is the full cone in
/// degrees; the inner cone is 85% of it (matches the legacy `spawn_spot_light`).
fn sync_spot_lights(
    mut commands: Commands,
    mut query: Query<
        (Entity, &EustressSpotLight, Option<&mut SpotLight>),
        Changed<EustressSpotLight>,
    >,
) {
    for (entity, light, existing) in &mut query {
        let intensity = light.brightness * LUMENS_PER_BRIGHTNESS;
        let outer_angle = light.angle.to_radians();
        let inner_angle = (light.angle * 0.85).to_radians();
        if let Some(mut sl) = existing {
            sl.color = light.color;
            sl.intensity = intensity;
            sl.range = light.range;
            sl.inner_angle = inner_angle;
            sl.outer_angle = outer_angle;
            sl.shadows_enabled = light.shadows;
        } else {
            commands.entity(entity).insert(SpotLight {
                color: light.color,
                intensity,
                range: light.range,
                inner_angle,
                outer_angle,
                shadows_enabled: light.shadows,
                ..default()
            });
        }
    }
}

/// `EustressDirectionalLight` → `bevy_pbr::DirectionalLight` (uses lux).
fn sync_directional_lights(
    mut commands: Commands,
    mut query: Query<
        (Entity, &EustressDirectionalLight, Option<&mut DirectionalLight>),
        Changed<EustressDirectionalLight>,
    >,
) {
    for (entity, light, existing) in &mut query {
        let illuminance = light.brightness * LUX_PER_BRIGHTNESS;
        if let Some(mut dl) = existing {
            dl.color = light.color;
            dl.illuminance = illuminance;
            dl.shadows_enabled = light.shadows;
            dl.shadow_depth_bias = light.shadow_depth_bias;
            dl.shadow_normal_bias = light.shadow_normal_bias;
        } else {
            commands.entity(entity).insert(DirectionalLight {
                color: light.color,
                illuminance,
                shadows_enabled: light.shadows,
                shadow_depth_bias: light.shadow_depth_bias,
                shadow_normal_bias: light.shadow_normal_bias,
                ..default()
            });
        }
    }
}

/// `SurfaceLight` → a co-located `bevy_pbr::PointLight`. The light rides the
/// SurfaceLight entity's own transform (so it lights from where it was placed —
/// the legacy `spawn_surface_light` pinned it to the origin). A face-aligned
/// emissive quad + directional cone is a later refinement; a point emitter is
/// what makes the surface actually illuminate the scene today.
fn sync_surface_lights(
    mut commands: Commands,
    mut query: Query<(Entity, &SurfaceLight, Option<&mut PointLight>), Changed<SurfaceLight>>,
) {
    for (entity, light, existing) in &mut query {
        let intensity = light.brightness * LUMENS_PER_BRIGHTNESS;
        if let Some(mut pl) = existing {
            pl.color = light.color;
            pl.intensity = intensity;
            pl.range = light.range;
            pl.shadows_enabled = light.shadows;
        } else {
            commands.entity(entity).insert(PointLight {
                color: light.color,
                intensity,
                range: light.range,
                shadows_enabled: light.shadows,
                ..default()
            });
        }
    }
}
