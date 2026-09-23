//! # Lighting Plugin
//! 
//! Uses the shared lighting plugin from eustress_common.
//! Adds engine-specific light class registrations.
//! Hydrates file-loaded Lighting/ entities with real ECS components.
//! 
//! ## Architecture
//! Each Space owns its lighting via `Lighting/*.instance.toml` files.
//! The file loader spawns them as bare `Instance` entities. This plugin's
//! `hydrate_lighting_entities` system detects freshly-loaded lighting
//! class entities (Star, Moon, Sky, Atmosphere) and attaches the real
//! Bevy components (DirectionalLight, SunMarker, SunClass, etc.).
//! On Space switch, all entities are despawned; the new Space's TOMLs
//! are re-loaded and re-hydrated, preserving per-Space lighting config.

use bevy::prelude::*;
use bevy::light::{light_consts::lux, CascadeShadowConfigBuilder, VolumetricLight, SunDisk};
use eustress_common::classes::{
    ClassName, Instance, EustressPointLight, EustressSpotLight, SurfaceLight, Terrain, Atmosphere,
    Sun as SunClass, Moon as MoonClass, Sky,
};
use eustress_common::services::lighting::{Sun as SunMarker, Moon as MoonMarker, EustressAtmosphere, LightingService};
// Aliased because `ClassName::ReflectionProbe` (the authored class) and the
// component that implements it share a name.
use eustress_common::plugins::reflections::ReflectionProbe as ReflectionProbeComponent;

// Re-export shared plugin. Sky rendering moved to `sky_atmosphere`, so
// `SkyboxHandle` / `create_procedural_skybox` / `regenerate_skybox` are gone:
// the star field is built once into `StarField` and the analytic gradient is
// only reachable as the `EUSTRESS_SKY=gradient` fallback.
pub use eustress_common::plugins::lighting_plugin::{SharedLightingPlugin, StarField};

/// Component to track which service an entity belongs to (for Explorer)
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct LightingServiceOwner;

/// P2 two-tier (Update-bound lag fix): exclude residency-streamed binary-ECS
/// parts from `hydrate_lighting_entities`' candidate query.
///
/// A binary streamed part is ALWAYS an authored Part — it can NEVER be a
/// Star/Moon/Sky/Atmosphere (those come from `Lighting/*.instance.toml` via
/// the file loader, never from the binary `entities` partition), so excluding
/// every `BinaryEcsInstance` (cold OR promoted) from these queries can never
/// drop a real lighting hydration. We use `BinaryEcsInstance` rather than
/// `ColdStreamed` deliberately: a promoted (selected) binary part is still not
/// a celestial class, so it never needs lighting hydration either — excluding
/// the whole binary population is both safe and maximal.
///
/// Resolves to `Without<BinaryEcsInstance>` when `world-db` is compiled, and
/// to the empty filter `()` otherwise (no binary path exists in that build),
/// so the queries are unchanged on a `--no-default-features` lighting build.
#[cfg(feature = "world-db")]
type NotBinaryStreamed = Without<crate::space::world_db_binary::BinaryEcsInstance>;
#[cfg(not(feature = "world-db"))]
type NotBinaryStreamed = ();

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        // Use the shared lighting plugin (sun, ambient, skybox)
        app.add_plugins(SharedLightingPlugin);
        
        // Engine-specific: register additional light classes for editor
        app
            // Light classes (for Properties panel)
            .register_type::<EustressPointLight>()
            .register_type::<EustressSpotLight>()
            .register_type::<SurfaceLight>()
            .register_type::<LightingServiceOwner>()
            
            // Celestial classes
            .register_type::<SunClass>()
            .register_type::<MoonClass>()
            
            // Environment classes
            .register_type::<Terrain>()
            .register_type::<Atmosphere>()
            
            // Hydrate file-loaded Lighting/ entities with real ECS components.
            // Runs every frame — detects Instance entities with lighting
            // class names that lack their real Bevy components and attaches
            // DirectionalLight, SunMarker, SunClass, MoonMarker, etc.
            // This is the authoritative path for per-Space lighting.
            // Candidates arrive through an `Add<Instance>` observer that queues
            // only lighting classes; the system drains that queue. The bulk-
            // load tick just batches the drain; nothing is missed.
            .init_resource::<PendingLightingHydration>()
            .add_observer(queue_lighting_hydration)
            .add_systems(Update, hydrate_lighting_entities.run_if(crate::space::file_loader::ui_sync_tick))
            // Sync Sun class properties with LightingService
            .add_systems(Update, sync_sun_with_lighting_service)
            // NOTE: no second sun driver here. `update_directional_light_from_sun_class`
            // used to run over the same `With<SunMarker>` entity as
            // SharedLightingPlugin's `update_sun_position`, with a different
            // intensity curve, and the two were unordered. `update_sun_position`
            // absorbed the `SunClass::current_intensity()` model and is now the
            // single owner of the sun light.
            // Sync Atmosphere entity with SceneAtmosphere resource for rendering
            .add_systems(Update, sync_atmosphere_to_rendering)
            // Sync Lighting ServiceComponent property edits → LightingService resource
            .add_systems(Update, sync_service_properties_to_lighting)
            // Perf QW1/QW2 — cap active + shadow-casting PointLight/SpotLight
            // to the nearest-N around the order-0 camera (collapses thousands
            // of shadow maps + clustered-forward cost on huge imports). Self-
            // gated to a movement/cadence trigger, so it is a cheap no-op for
            // a stationary camera and for small scenes. See `light_cull`.
            .add_systems(Update, crate::light_cull::cull_lights_to_nearest)
            // Hard shadow-caster cap in PostUpdate (after spawns, before render
            // extract) so a huge import can't OOM the GPU shadow atlas during
            // load before the gated cull above reacts. Gated to load-active →
            // zero steady-state cost. See `light_cull::enforce_shadow_budget`.
            .add_systems(PostUpdate, crate::light_cull::enforce_shadow_budget);
    }
}

/// Maximum sun cascade-shadow distance in meters.
///
/// Read once from `EUSTRESS_SHADOW_DISTANCE` (default 200.0). Street-level
/// shadows only — beyond this the HLOD whole-map proxies are
/// `NotShadowCaster` so longer cascades just re-walk near casters at
/// coarser resolution. Set the env var higher (e.g. 2048) to restore the
/// old long-range behavior for cinematic captures.
fn sun_shadow_distance() -> f32 {
    static D: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *D.get_or_init(|| {
        std::env::var("EUSTRESS_SHADOW_DISTANCE")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            // Terrain-scale default (was 200 m, street-scale): a generated
            // world spans kilometres, so 200 m left almost the whole surface
            // shadowless and flat-looking. 1000 m + 4 cascades keeps near
            // shadows crisp while distant relief still casts. Dense part-heavy
            // scenes can dial back via EUSTRESS_SHADOW_DISTANCE.
            .unwrap_or(1000.0)
    })
}

/// Hydrate file-loaded Lighting/ entities with real ECS components.
///
/// The file loader spawns `Instance` entities from `Lighting/*.instance.toml`
/// but only attaches generic components (Instance, Transform, Visibility,
/// Attributes, Name). This system detects entities with lighting class names
/// that lack their real Bevy components and attaches:
///
/// - **Star → DirectionalLight + SunMarker + SunClass + cascade shadows + SunDisk**
/// - **Moon → DirectionalLight + MoonMarker + MoonClass**
/// - **Sky → Sky component**
/// - **Atmosphere → Atmosphere + EustressAtmosphere**
///
/// Entities whose `Instance` was just added with a class this module
/// hydrates. Filled by [`queue_lighting_hydration`], drained by
/// [`hydrate_lighting_entities`].
#[derive(Resource, Default)]
pub struct PendingLightingHydration(Vec<Entity>);

/// `Add<Instance>` observer: queue the rare lighting-class entities. The
/// hydration used to find them with five `Added<Instance>` queries, and
/// `Added` is checked per entity, not per archetype, so every frame paid
/// ~5 × the live instance count in tick comparisons: 29 ms per frame at
/// steady state on Super Station's ~136K instances, for a Space whose
/// lighting entities were all hydrated in its first second.
fn queue_lighting_hydration(
    add: On<bevy::ecs::lifecycle::Add, Instance>,
    instances: Query<&Instance>,
    mut pending: ResMut<PendingLightingHydration>,
) {
    let entity = add.event().entity;
    let Ok(instance) = instances.get(entity) else { return };
    if matches!(
        instance.class_name,
        ClassName::Star
            | ClassName::Moon
            | ClassName::Sky
            | ClassName::Atmosphere
            | ClassName::ReflectionProbe
    ) {
        pending.0.push(entity);
    }
}

/// Drains the queue each run; a no-op when nothing lighting-classed was
/// spawned since the last run. The per-class `Has<marker>` checks keep it
/// idempotent: an entity is only hydrated while it still lacks its marker.
fn hydrate_lighting_entities(
    mut commands: Commands,
    lighting: Res<LightingService>,
    mut pending: ResMut<PendingLightingHydration>,
    // `NotBinaryStreamed` (P2) drops the streamed binary parts, which can
    // never be a lighting class — see the type alias above.
    candidates: Query<
        (
            &Instance,
            Has<SunMarker>,
            Has<MoonMarker>,
            Has<Sky>,
            Has<EustressAtmosphere>,
            Has<ReflectionProbeComponent>,
        ),
        NotBinaryStreamed,
    >,
) {
    if pending.0.is_empty() {
        return;
    }
    let queued = std::mem::take(&mut pending.0);
    let mut unhydrated_sun: Vec<(Entity, &Instance)> = Vec::new();
    let mut unhydrated_moon: Vec<(Entity, &Instance)> = Vec::new();
    let mut unhydrated_sky: Vec<(Entity, &Instance)> = Vec::new();
    let mut unhydrated_atmo: Vec<(Entity, &Instance)> = Vec::new();
    let mut unhydrated_probe: Vec<(Entity, &Instance)> = Vec::new();
    for entity in queued {
        let Ok((instance, sun, moon, sky, atmo, probe)) = candidates.get(entity) else {
            continue; // despawned, or a streamed binary part
        };
        match instance.class_name {
            ClassName::Star if !sun && !moon => unhydrated_sun.push((entity, instance)),
            ClassName::Moon if !moon && !sun => unhydrated_moon.push((entity, instance)),
            ClassName::Sky if !sky => unhydrated_sky.push((entity, instance)),
            ClassName::Atmosphere if !atmo => unhydrated_atmo.push((entity, instance)),
            ClassName::ReflectionProbe if !probe => unhydrated_probe.push((entity, instance)),
            _ => {}
        }
    }

    // ── Star → Sun (DirectionalLight + SunMarker + SunClass) ──────────
    for (entity, instance) in unhydrated_sun {
        if instance.class_name != ClassName::Star { continue; }

        info!("☀️ Hydrating Sun entity {:?} from Lighting/ TOML", entity);

        let sun_class = SunClass {
            enabled: true,
            time_of_day: lighting.time_of_day * 24.0,
            cycle_speed: 0.0,
            cycle_paused: true,
            latitude: lighting.geographic_latitude,
            day_of_year: 172,
            angular_size: lighting.sun_angular_radius * 2.0,
            noon_color: lighting.sun_color,
            horizon_color: [1.0, 0.5, 0.2, 1.0],
            noon_intensity: lighting.sun_intensity,
            horizon_intensity: 1000.0,
            cast_shadows: lighting.shadows_enabled,
            shadow_softness: lighting.shadow_softness,
            ambient_day_color: lighting.ambient,
            ambient_night_color: [0.02, 0.02, 0.05, 1.0],
            corona_intensity: 0.3,
            god_rays_intensity: 0.0,
            texture: String::new(),
        };

        let sun_dir = lighting.sun_direction();
        // PERF: bound the sun's shadow frusta to street-level range.
        // The old config (4 cascades to 2048 m) made bevy_pbr's
        // queue_shadows + specialize_shadows walk every caster in a
        // 2 km radius across 4 cascade views (~9 ms/frame on the 121K
        // Vehicle Simulator scene). Distant geometry is HLOD proxies
        // marked NotShadowCaster anyway, so cascades past ~200 m buy
        // nothing. 2 cascades to 200 m keeps crisp near shadows and
        // cuts the per-cascade caster set dramatically.
        // Env-tunable: EUSTRESS_SHADOW_DISTANCE (meters, default 200).
        let cascade_shadow_config = CascadeShadowConfigBuilder {
            num_cascades: 4,
            minimum_distance: 0.1,
            maximum_distance: sun_shadow_distance(),
            first_cascade_far_bound: 90.0,
            overlap_proportion: 0.25,
            ..default()
        }
        .build();

        commands.entity(entity).insert((
            DirectionalLight {
                color: Color::srgba(
                    lighting.sun_color[0],
                    lighting.sun_color[1],
                    lighting.sun_color[2],
                    lighting.sun_color[3],
                ),
                illuminance: lux::RAW_SUNLIGHT,
                shadow_maps_enabled: true,
                shadow_depth_bias: 0.02,
                shadow_normal_bias: 1.8,
                ..default()
            },
            SunDisk {
                angular_size: sun_class.angular_size.to_radians(),
                intensity: 1.0,
            },
            Transform::from_translation(sun_dir * 100.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            VolumetricLight,
            cascade_shadow_config,
            SunMarker,
            sun_class,
            LightingServiceOwner,
        ));
    }

    // ── Moon → DirectionalLight + MoonMarker + MoonClass ──────────────
    for (entity, instance) in unhydrated_moon {
        if instance.class_name != ClassName::Moon { continue; }

        info!("🌙 Hydrating Moon entity {:?} from Lighting/ TOML", entity);

        commands.entity(entity).insert((
            DirectionalLight {
                color: Color::srgb(0.7, 0.75, 0.9),
                illuminance: 500.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(50.0, 80.0, -30.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            MoonMarker,
            MoonClass::default(),
            LightingServiceOwner,
        ));
    }

    // ── Sky → Sky component ───────────────────────────────────────────
    for (entity, instance) in unhydrated_sky {
        if instance.class_name != ClassName::Sky { continue; }

        info!("�️ Hydrating Sky entity {:?} from Lighting/ TOML", entity);
        commands.entity(entity).insert((
            Sky::default(),
            LightingServiceOwner,
        ));
    }

    // ── Atmosphere → Atmosphere + EustressAtmosphere ──────────────────
    for (entity, instance) in unhydrated_atmo {
        if instance.class_name != ClassName::Atmosphere { continue; }

        info!("🌫️ Hydrating Atmosphere entity {:?} from Lighting/ TOML", entity);
        commands.entity(entity).insert((
            Atmosphere::clear_day(),
            EustressAtmosphere::default(),
            LightingServiceOwner,
        ));
    }

    // ── ReflectionProbe → ReflectionProbe component ───────────────────
    //
    // Unlike the classes above, a probe is a spatial object: its Transform is
    // the volume it covers (bevy treats a light probe as a 1x1x1 cube in local
    // space, so the scale is the extent). It lives in Workspace, not under
    // Lighting, and is deliberately selectable so it can be placed and sized.
    // `hydrate_reflection_probes` in the reflections plugin turns the component
    // into bevy's `LightProbe` plus a filtered environment map.
    for (entity, instance) in unhydrated_probe {
        if instance.class_name != ClassName::ReflectionProbe { continue; }

        info!("🪞 Hydrating ReflectionProbe entity {:?}", entity);
        commands.entity(entity).insert(ReflectionProbeComponent::default());
    }
}

/// Sync Sun class properties with LightingService for real-time updates
/// Geographic latitude from LightingService controls sun/moon arc paths
fn sync_sun_with_lighting_service(
    lighting: Res<LightingService>,
    mut sun_query: Query<&mut SunClass>,
) {
    if !lighting.is_changed() {
        return;
    }
    
    for mut sun in sun_query.iter_mut() {
        // Sync latitude from LightingService (controls sun arc path)
        sun.latitude = lighting.geographic_latitude;
        
        // Parse ClockTime string to time_of_day if it changed
        if let Some((hours, minutes)) = parse_clock_time(&lighting.clock_time) {
            let time = hours as f32 + (minutes as f32 / 60.0);
            if (sun.time_of_day - time).abs() > 0.01 {
                sun.time_of_day = time;
            }
        }
    }
}

/// Parse `"HH:MM:SS"`, `"HH:MM"`, or a bare decimal hour into hours.
///
/// Wraps into `[0, 24)` rather than clamping, so 25:00 reads as 01:00 instead of
/// pinning to midnight.
fn parse_hours(text: &str) -> Option<f32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if !text.contains(':') {
        return text.parse::<f32>().ok().map(|h| h.rem_euclid(24.0));
    }
    let mut parts = text.split(':');
    let h: f32 = parts.next()?.trim().parse().ok()?;
    let m: f32 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    let s: f32 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    (h.is_finite() && m.is_finite() && s.is_finite())
        .then(|| (h + m / 60.0 + s / 3600.0).rem_euclid(24.0))
}

/// Parse clock time string "HH:MM:SS" to (hours, minutes)
fn parse_clock_time(clock_time: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = clock_time.split(':').collect();
    if parts.len() >= 2 {
        let hours = parts[0].parse().ok()?;
        let minutes = parts[1].parse().ok()?;
        Some((hours, minutes))
    } else {
        None
    }
}

/// Sync the authored Atmosphere entity into the `SceneAtmosphere` resource that
/// drives rendering.
///
/// This system always worked; what changed is that something now reads the
/// result. `SceneAtmosphere` used to be applied to a camera exactly once and
/// then filtered out forever behind an `AtmosphereApplied` marker, so every edit
/// after the first frame updated a resource nothing consumed.
///
/// `Atmosphere` (the Explorer class) carries the six artistic properties;
/// `EustressAtmosphere` carries those plus the scattering model. Writing the six
/// individually rather than replacing the whole struct is deliberate: it keeps
/// the authored planet radius, scale heights and Rayleigh/Mie coefficients
/// intact when someone drags the Density slider.
fn sync_atmosphere_to_rendering(
    atmosphere_query: Query<&Atmosphere, Changed<Atmosphere>>,
    eustress_atmo_query: Query<&EustressAtmosphere, Changed<EustressAtmosphere>>,
    mut scene_atmosphere: ResMut<eustress_common::plugins::lighting_plugin::SceneAtmosphere>,
) {
    for atmosphere in atmosphere_query.iter() {
        scene_atmosphere.atmosphere.density = atmosphere.density;
        scene_atmosphere.atmosphere.offset = atmosphere.offset;
        scene_atmosphere.atmosphere.color = atmosphere.color;
        scene_atmosphere.atmosphere.decay = atmosphere.decay;
        scene_atmosphere.atmosphere.glare = atmosphere.glare;
        scene_atmosphere.atmosphere.haze = atmosphere.haze;

        info!("🌫️ Synced Atmosphere to rendering (density: {}, haze: {})",
              atmosphere.density, atmosphere.haze);
    }

    // A direct EustressAtmosphere edit carries the scattering model too, so it
    // replaces the whole thing.
    for eustress_atmo in eustress_atmo_query.iter() {
        scene_atmosphere.atmosphere = eustress_atmo.clone();
        info!("🌫️ Synced EustressAtmosphere to rendering");
    }
}

/// Sync Lighting ServiceComponent property edits → LightingService resource.
///
/// When the user edits ClockTime, Brightness, etc. in the Properties panel,
/// those changes go to ServiceComponent first. This system reads them and
/// writes to the live LightingService resource so Bevy systems react immediately.
fn sync_service_properties_to_lighting(
    mut lighting: ResMut<LightingService>,
    service_query: Query<&crate::space::service_loader::ServiceComponent, Changed<crate::space::service_loader::ServiceComponent>>,
) {
    use crate::space::service_loader::PropertyValue;

    for service in service_query.iter() {
        // Only sync the Lighting service
        if service.class_name != "Lighting" { continue; }

        let props = &service.properties;

        // `clock_time` is hours (0-24). Accept a string too: the panel used to
        // write it as `PropertyValue::String`, which this arm matched only as
        // Float and therefore dropped on the floor, so the edit reached the TOML
        // but never the renderer. Authored Spaces may still carry either shape.
        let clock_hours = match props.get("clock_time") {
            Some(PropertyValue::Float(v)) => Some(*v as f32),
            Some(PropertyValue::Int(v)) => Some(*v as f32),
            Some(PropertyValue::String(s)) => parse_hours(s),
            _ => None,
        };
        if let Some(hours) = clock_hours {
            let new_tod = (hours / 24.0).rem_euclid(1.0);
            if (lighting.time_of_day - new_tod).abs() > 0.001 {
                lighting.time_of_day = new_tod;
                let total = (new_tod * 24.0 * 3600.0).round() as u32 % 86_400;
                lighting.clock_time =
                    format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60);
            }
        }
        if let Some(PropertyValue::Float(v)) = props.get("brightness") {
            lighting.brightness = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("shadow_softness") {
            lighting.shadow_softness = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("exposure_compensation") {
            lighting.exposure_compensation = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("geographic_latitude") {
            lighting.geographic_latitude = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("fog_start") {
            lighting.fog_start = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("fog_end") {
            lighting.fog_end = *v as f32;
        }
        if let Some(PropertyValue::Bool(v)) = props.get("fog_enabled") {
            lighting.fog_enabled = *v;
        }
        if let Some(PropertyValue::Bool(v)) = props.get("shadows_enabled") {
            lighting.shadows_enabled = *v;
        }
        if let Some(PropertyValue::Float(v)) = props.get("sun_intensity") {
            lighting.sun_intensity = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("environment_diffuse_scale") {
            lighting.environment_diffuse_scale = *v as f32;
        }
        if let Some(PropertyValue::Float(v)) = props.get("environment_specular_scale") {
            lighting.environment_specular_scale = *v as f32;
        }
        if let Some(PropertyValue::Bool(v)) = props.get("cycle_enabled") {
            lighting.cycle_enabled = *v;
        }
        if let Some(PropertyValue::Float(v)) = props.get("day_length_minutes") {
            lighting.day_length_minutes = *v as f32;
        }
        // Color arrays (stored as Vec4 in ServiceComponent)
        if let Some(PropertyValue::Vec4(v)) = props.get("fog_color") {
            lighting.fog_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("ambient") {
            lighting.ambient = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("outdoor_ambient") {
            lighting.outdoor_ambient = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("sun_color") {
            lighting.sun_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("sky_color") {
            lighting.sky_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
        if let Some(PropertyValue::Vec4(v)) = props.get("horizon_color") {
            lighting.horizon_color = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
        }
    }
}
