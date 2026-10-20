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
    Sun as SunClass, Moon as MoonClass, Sky, BloomEffect, CloudLayerType, Clouds, SunRaysEffect,
};
use eustress_common::services::lighting::{Sun as SunMarker, Moon as MoonMarker, EustressAtmosphere, LightingService};
use eustress_common::plugins::celestial_sections;
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
            .register_type::<eustress_common::classes::EustressDirectionalLight>()
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
            // A Clouds, Bloom or SunRays object re-reads its TOML when the
            // file changes, so a Properties-panel edit (or an editor's) takes
            // effect live instead of on the next open.
            .register_type::<Clouds>()
            .register_type::<BloomEffect>()
            .register_type::<SunRaysEffect>()
            .add_systems(Update, reload_lighting_fx)
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
            // Before the light sync, so a budget it writes shapes the Bevy
            // light in the same frame.
            .add_systems(
                Update,
                crate::light_cull::cull_lights_to_nearest
                    .before(eustress_common::plugins::light_classes::LightSyncSet),
            )
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
    ) || is_lighting_fx(instance.class_name)
    {
        pending.0.push(entity);
    }
}

// ============================================================================
// Clouds and post effects: authored values from the instance TOML
// ============================================================================

/// The Lighting children whose component is built from their TOML here:
/// the cloud layer and the two post effects the renderer honours.
fn is_lighting_fx(class: ClassName) -> bool {
    matches!(class, ClassName::Clouds | ClassName::BloomEffect | ClassName::SunRaysEffect)
}

/// A number from a TOML value, integer or float.
fn toml_f32(value: &toml::Value) -> Option<f32> {
    value
        .as_float()
        .map(|f| f as f32)
        .or_else(|| value.as_integer().map(|i| i as f32))
}

/// An effect's authored value: its own `[section]`, which is what the
/// Properties panel writes, else the PascalCase `[properties.extras]` a
/// Roblox import carries.
fn effect_value<'a>(
    doc: Option<&'a toml::Value>,
    section: &str,
    key: &str,
    roblox_key: &str,
) -> Option<&'a toml::Value> {
    let doc = doc?;
    doc.get(section).and_then(|s| s.get(key)).or_else(|| {
        doc.get("properties")
            .and_then(|p| p.get("extras"))
            .and_then(|e| e.get(roblox_key))
    })
}

/// A `BloomEffect` from its `[bloom]` section or imported extras.
pub fn bloom_from_toml(doc: Option<&toml::Value>) -> BloomEffect {
    let d = BloomEffect::default();
    let num = |key: &str, roblox_key: &str, fallback: f32| {
        effect_value(doc, "bloom", key, roblox_key).and_then(toml_f32).unwrap_or(fallback)
    };
    BloomEffect {
        enabled: effect_value(doc, "bloom", "enabled", "Enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.enabled),
        intensity: num("intensity", "Intensity", d.intensity),
        size: num("size", "Size", d.size),
        threshold: num("threshold", "Threshold", d.threshold),
    }
}

/// A `SunRaysEffect` from its `[sun_rays]` section or imported extras.
pub fn sun_rays_from_toml(doc: Option<&toml::Value>) -> SunRaysEffect {
    let d = SunRaysEffect::default();
    let num = |key: &str, roblox_key: &str, fallback: f32| {
        effect_value(doc, "sun_rays", key, roblox_key).and_then(toml_f32).unwrap_or(fallback)
    };
    SunRaysEffect {
        enabled: effect_value(doc, "sun_rays", "enabled", "Enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.enabled),
        intensity: num("intensity", "Intensity", d.intensity),
        spread: num("spread", "Spread", d.spread),
    }
}

/// A cloud layer from its `[clouds]` section: the importer's `enabled`,
/// `cover`, `density` and `color` (0-255 channels), and the renderer's own
/// `altitude`, `thickness` (metres), `wind_speed`, `wind_direction` and
/// `layer_type`.
pub fn clouds_from_toml(doc: Option<&toml::Value>) -> Clouds {
    let sec = doc.and_then(|d| d.get("clouds"));
    let d = Clouds::default();
    let num = |key: &str| sec.and_then(|s| s.get(key)).and_then(toml_f32);
    let color = sec
        .and_then(|s| s.get("color"))
        .and_then(|v| v.as_array())
        .filter(|a| a.len() >= 3)
        .map(|a| {
            // 0-255 integers, or 0-1 floats.
            let channel = |i: usize, fallback: f32| {
                a.get(i)
                    .and_then(|v| {
                        v.as_integer()
                            .map(|n| n as f32 / 255.0)
                            .or_else(|| v.as_float().map(|f| f as f32))
                    })
                    .unwrap_or(fallback)
            };
            [channel(0, d.color[0]), channel(1, d.color[1]), channel(2, d.color[2]), 1.0]
        })
        .unwrap_or(d.color);
    let layer_type = sec
        .and_then(|s| s.get("layer_type"))
        .and_then(|v| v.as_str())
        .map(|s| match s.trim().to_ascii_lowercase().as_str() {
            "cirrus" => CloudLayerType::Cirrus,
            "stratus" => CloudLayerType::Stratus,
            "cumulonimbus" => CloudLayerType::Cumulonimbus,
            "altocumulus" => CloudLayerType::Altocumulus,
            _ => CloudLayerType::Cumulus,
        })
        .unwrap_or(d.layer_type);
    Clouds {
        enabled: sec
            .and_then(|s| s.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(d.enabled),
        density: num("density").unwrap_or(d.density),
        // The importer writes Roblox's `cover`.
        coverage: num("cover").or_else(|| num("coverage")).unwrap_or(d.coverage),
        color,
        altitude: num("altitude").unwrap_or(d.altitude),
        thickness: num("thickness").unwrap_or(d.thickness),
        wind_speed: num("wind_speed").unwrap_or(d.wind_speed),
        wind_direction: num("wind_direction").unwrap_or(d.wind_direction),
        layer_type,
        ..d
    }
}

/// An instance file's document: the WorldDb copy when a DB is active (it is
/// authoritative on migrated Spaces), else the file on disk.
fn read_instance_doc(path: Option<&std::path::Path>) -> Option<toml::Value> {
    let path = path?;
    let text = crate::space::active_db::get_instance_text(path)
        .or_else(|| std::fs::read_to_string(path).ok())?;
    text.parse::<toml::Value>().ok()
}

/// Build the component for a Clouds, Bloom or SunRays object from its TOML
/// file (defaults when the file cannot be read).
fn hydrate_lighting_fx(commands: &mut Commands, entity: Entity, class: ClassName, path: Option<&std::path::Path>) {
    let doc = read_instance_doc(path);
    let doc = doc.as_ref();
    let mut target = commands.entity(entity);
    match class {
        ClassName::Clouds => {
            target.try_insert(clouds_from_toml(doc));
        }
        ClassName::BloomEffect => {
            target.try_insert(bloom_from_toml(doc));
        }
        ClassName::SunRaysEffect => {
            target.try_insert(sun_rays_from_toml(doc));
        }
        _ => {}
    }
}

/// Re-read a Clouds, Bloom or SunRays object when its `_instance.toml`
/// changes on disk. The Properties panel edits these by writing the file, so
/// this is also what makes a panel edit take effect.
///
/// A Star, Moon, Sky or Atmosphere re-reads its section too, so an edit made
/// outside Studio (MCP, a text editor, a git checkout) lands live. Their
/// panel edits set the component directly and save the file, and the reload
/// that follows reads back the same values.
fn reload_lighting_fx(
    mut commands: Commands,
    mut changes: MessageReader<eustress_common::file_events::FileChanged>,
    objects: Query<(
        Entity,
        &Instance,
        &crate::space::instance_loader::InstanceFile,
        Option<&SunClass>,
        Option<&MoonClass>,
        Option<&Sky>,
        Option<&EustressAtmosphere>,
    )>,
) {
    if changes.is_empty() {
        return;
    }
    let changed: Vec<std::path::PathBuf> = changes
        .read()
        .filter(|c| c.kind != eustress_common::file_events::FileChangeKind::Removed)
        .map(|c| c.path.clone())
        .collect();
    if changed.is_empty() {
        return;
    }
    for (entity, instance, file, sun, moon, sky, atmosphere) in objects.iter() {
        let class = instance.class_name;
        if !changed.iter().any(|p| *p == file.toml_path) {
            continue;
        }
        if is_lighting_fx(class) {
            hydrate_lighting_fx(&mut commands, entity, class, Some(&file.toml_path));
            info!("🌥️ Reloaded {:?} from {:?}", class, file.toml_path);
        } else if celestial_sections::is_celestial_class(class) {
            let Some(doc) = read_instance_doc(Some(&file.toml_path)) else { continue };
            let section = celestial_sections::section_of(&doc, class);
            let mut target = commands.entity(entity);
            // Over the live component: what the section does not set stays,
            // and a Sun's clock and latitude are Lighting's.
            match class {
                ClassName::Star => {
                    if let Some(sun) = sun {
                        target.try_insert(celestial_sections::sun_from_section(section, sun.clone()));
                    }
                }
                ClassName::Moon => {
                    if let Some(moon) = moon {
                        target.try_insert(celestial_sections::moon_from_section(section, moon.clone()));
                    }
                }
                ClassName::Sky => {
                    target.try_insert(celestial_sections::sky_from_section(
                        section,
                        sky.cloned().unwrap_or_default(),
                    ));
                }
                _ => {
                    let base = atmosphere.cloned().unwrap_or_default();
                    target.try_insert(celestial_sections::atmosphere_from_section(section, &base));
                }
            }
            info!("🌌 Reloaded {:?} from {:?}", class, file.toml_path);
        }
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
            // The Sun's and Moon's components, built from their sections
            // when the instance spawned (`celestial_sections`).
            Option<&SunClass>,
            Option<&MoonClass>,
        ),
        NotBinaryStreamed,
    >,
    // Clouds and post-effect objects that arrived without their component
    // (anything but the file loader's environment path, which builds it in
    // the same spawn).
    fx_candidates: Query<(
        Option<&crate::space::instance_loader::InstanceFile>,
        Has<Clouds>,
        Has<BloomEffect>,
        Has<SunRaysEffect>,
    )>,
) {
    if pending.0.is_empty() {
        return;
    }
    let queued = std::mem::take(&mut pending.0);
    for &entity in &queued {
        let (Ok((instance, ..)), Ok((file, clouds, bloom, sun_rays))) =
            (candidates.get(entity), fx_candidates.get(entity))
        else {
            continue;
        };
        let missing = match instance.class_name {
            ClassName::Clouds => !clouds,
            ClassName::BloomEffect => !bloom,
            ClassName::SunRaysEffect => !sun_rays,
            _ => false,
        };
        if missing {
            hydrate_lighting_fx(
                &mut commands,
                entity,
                instance.class_name,
                file.map(|f| f.toml_path.as_path()),
            );
        }
    }
    let mut unhydrated_sun: Vec<(Entity, Option<&SunClass>)> = Vec::new();
    let mut unhydrated_moon: Vec<(Entity, Option<&MoonClass>)> = Vec::new();
    let mut unhydrated_sky: Vec<Entity> = Vec::new();
    let mut unhydrated_atmo: Vec<Entity> = Vec::new();
    let mut unhydrated_probe: Vec<Entity> = Vec::new();
    for entity in queued {
        let Ok((instance, sun, moon, sky, atmo, probe, sun_class, moon_class)) = candidates.get(entity) else {
            continue; // despawned, or a streamed binary part
        };
        match instance.class_name {
            ClassName::Star if !sun && !moon => unhydrated_sun.push((entity, sun_class)),
            ClassName::Moon if !moon && !sun => unhydrated_moon.push((entity, moon_class)),
            ClassName::Sky if !sky => unhydrated_sky.push(entity),
            ClassName::Atmosphere if !atmo => unhydrated_atmo.push(entity),
            ClassName::ReflectionProbe if !probe => unhydrated_probe.push(entity),
            _ => {}
        }
    }

    // ── Star → Sun (DirectionalLight + SunMarker + SunClass) ──────────
    for (entity, authored) in unhydrated_sun {
        info!("☀️ Hydrating Sun entity {:?} from Lighting/ TOML", entity);

        // The Sun's own values (light, colour, disc, shadows, shafts, date)
        // came from its `[star]` section when it spawned. Where it stands is
        // the Lighting service's: the clock and the latitude.
        let mut sun_class = authored.cloned().unwrap_or_else(celestial_sections::default_sun);
        sun_class.time_of_day = lighting.time_of_day * 24.0;
        sun_class.latitude = lighting.geographic_latitude;

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
                    sun_class.noon_color[0],
                    sun_class.noon_color[1],
                    sun_class.noon_color[2],
                    sun_class.noon_color[3],
                ),
                illuminance: lux::RAW_SUNLIGHT,
                shadow_maps_enabled: true,
                // Screen-space contact shadows only reach a camera that
                // carries `ContactShadows`, which `photoreal` attaches when
                // `EUSTRESS_CONTACT_SHADOWS` asks for it.
                contact_shadows_enabled: crate::photoreal::contact_shadows_on(),
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
    for (entity, authored) in unhydrated_moon {
        info!("🌙 Hydrating Moon entity {:?} from Lighting/ TOML", entity);

        // Dark until `update_moon_position` places it, on the next frame: the
        // old placeholder was a 500 lux moon hanging overhead, and it stayed
        // lit day and night until something edited the clock. The Moon's
        // values came from its `[moon]` section when it spawned.
        commands.entity(entity).insert((
            DirectionalLight {
                color: Color::srgb(0.7, 0.75, 0.9),
                illuminance: 0.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(50.0, 80.0, -30.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            MoonMarker,
            authored.cloned().unwrap_or_default(),
            LightingServiceOwner,
        ));
    }

    // A Sky or an Atmosphere normally arrives with its component, built
    // from its section by whichever loader spawned it. One that did not is
    // built from its file here, never from bare defaults.
    let section_doc = |entity: Entity| -> Option<toml::Value> {
        let (file, ..) = fx_candidates.get(entity).ok()?;
        read_instance_doc(file.map(|f| f.toml_path.as_path()))
    };

    // ── Sky → Sky component ───────────────────────────────────────────
    for entity in unhydrated_sky {
        info!("🌌 Hydrating Sky entity {:?} from Lighting/ TOML", entity);
        let doc = section_doc(entity);
        let section = doc.as_ref().and_then(|d| celestial_sections::section_of(d, ClassName::Sky));
        commands.entity(entity).insert((
            celestial_sections::sky_from_section(section, Sky::default()),
            LightingServiceOwner,
        ));
    }

    // ── Atmosphere → Atmosphere + EustressAtmosphere ──────────────────
    for entity in unhydrated_atmo {
        info!("🌫️ Hydrating Atmosphere entity {:?} from Lighting/ TOML", entity);
        let doc = section_doc(entity);
        let section = doc.as_ref().and_then(|d| celestial_sections::section_of(d, ClassName::Atmosphere));
        let (atmosphere, model) =
            celestial_sections::atmosphere_from_section(section, &EustressAtmosphere::default());
        commands.entity(entity).insert((atmosphere, model, LightingServiceOwner));
    }

    // ── ReflectionProbe → ReflectionProbe component ───────────────────
    //
    // Unlike the classes above, a probe is a spatial object: its Transform is
    // the volume it covers (bevy treats a light probe as a 1x1x1 cube in local
    // space, so the scale is the extent). It lives in Workspace, not under
    // Lighting, and is deliberately selectable so it can be placed and sized.
    // `hydrate_reflection_probes` in the reflections plugin turns the component
    // into bevy's `LightProbe` plus a filtered environment map.
    for entity in unhydrated_probe {
        info!("🪞 Hydrating ReflectionProbe entity {:?}", entity);
        commands.entity(entity).insert(ReflectionProbeComponent::default());
    }
}

/// Carry Lighting's GeographicLatitude to the Sun: where the sun stands is
/// the service's, like the clock (`sync_clock_time_to_sun` in the shared
/// plugin owns `time_of_day`).
///
/// Only that. The sun's light, colour, disc and shadows are the Sun's own
/// properties, in its `[star]` section. This used to copy the service's
/// `sun_intensity`, `sun_color`, `sun_angular_radius` and `shadows_enabled`
/// over them whenever the service changed, which during a day cycle is every
/// frame, so an edit to the Sun lasted a frame. `GlobalShadows` still
/// switches the sun's shadows off: `update_sun_position` reads both.
fn sync_sun_with_lighting_service(
    lighting: Res<LightingService>,
    mut sun_query: Query<&mut SunClass>,
) {
    if !lighting.is_changed() {
        return;
    }
    for mut sun in sun_query.iter_mut() {
        if sun.latitude != lighting.geographic_latitude {
            sun.latitude = lighting.geographic_latitude;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> toml::Value {
        text.parse::<toml::Value>().expect("test TOML parses")
    }

    #[test]
    fn a_roblox_bloom_reads_its_imported_extras() {
        // The shape the Roblox importer writes: PascalCase keys under
        // `[properties.extras]`, no section of our own.
        let d = doc(
            "[metadata]\nclass_name = \"BloomEffect\"\n\n[properties.extras]\nEnabled = true\nIntensity = 1.0\nSize = 24.0\nThreshold = 2.0\n",
        );
        let bloom = bloom_from_toml(Some(&d));
        assert!(bloom.enabled);
        assert_eq!((bloom.intensity, bloom.size, bloom.threshold), (1.0, 24.0, 2.0));
    }

    #[test]
    fn an_edited_value_wins_over_the_imported_one() {
        // The Properties panel writes `[sun_rays]`; the import's extras stay
        // in the file but no longer decide anything.
        let d = doc(
            "[sun_rays]\nenabled = true\nintensity = 0.5\n\n[properties.extras]\nEnabled = false\nIntensity = 0.01\nSpread = 0.1\n",
        );
        let rays = sun_rays_from_toml(Some(&d));
        assert!(rays.enabled);
        assert_eq!(rays.intensity, 0.5);
        assert!((rays.spread - 0.1).abs() < 1e-6, "an unedited field still comes from the import");
    }

    #[test]
    fn imported_clouds_keep_their_cover_colour_and_switch() {
        let d = doc("[clouds]\ncolor = [231, 231, 231]\ncover = 0.5\ndensity = 0.25\nenabled = false\n");
        let clouds = clouds_from_toml(Some(&d));
        assert!(!clouds.enabled);
        assert_eq!(clouds.coverage, 0.5);
        assert_eq!(clouds.density, 0.25);
        assert!((clouds.color[0] - 231.0 / 255.0).abs() < 1e-6, "0-255 channels");
        // Fields the importer never writes fall back to the class defaults.
        assert_eq!(clouds.altitude, Clouds::default().altitude);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let clouds = clouds_from_toml(None);
        assert_eq!(clouds.coverage, Clouds::default().coverage);
        assert!(bloom_from_toml(None).enabled);
    }

    #[test]
    fn the_layer_type_is_read_by_name() {
        let d = doc("[clouds]\nlayer_type = \"Cirrus\"\n");
        assert_eq!(clouds_from_toml(Some(&d)).layer_type, CloudLayerType::Cirrus);
        let d = doc("[clouds]\nlayer_type = \"nonsense\"\n");
        assert_eq!(clouds_from_toml(Some(&d)).layer_type, CloudLayerType::Cumulus);
    }
}
