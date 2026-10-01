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
use eustress_common::classes::{
    ClassName, Instance, EustressPointLight, EustressSpotLight, SurfaceLight, Terrain, Atmosphere,
    Sun as SunClass, Moon as MoonClass, Sky, BloomEffect, Clouds, SunRaysEffect, ColorCorrectionEffect,
    ColorGradingEffect,
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
            .register_type::<ColorCorrectionEffect>()
            .register_type::<ColorGradingEffect>()
            .add_systems(Update, reload_lighting_fx)
            // The Sun's latitude and the Atmosphere object's values reach
            // the sky through SharedLightingPlugin (`sync_sun_latitude`,
            // `sync_atmosphere_to_rendering`), in Studio and the Player alike.
            // NOTE: no second sun driver here. `update_directional_light_from_sun_class`
            // used to run over the same `With<SunMarker>` entity as
            // SharedLightingPlugin's `update_sun_position`, with a different
            // intensity curve, and the two were unordered. `update_sun_position`
            // absorbed the `SunClass::current_intensity()` model and is now the
            // single owner of the sun light.
            // Sync Lighting ServiceComponent property edits → LightingService resource
            .add_systems(Update, (seed_lighting_service_properties, sync_service_properties_to_lighting).chain())
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

        // A script's Play changes to the sky stay in Play: Stop puts back
        // the Lighting service and every sky object as Play found them, as
        // `light_sync` does for the light classes (its helper, shared).
        add_lighting_play_snapshot(app);
        crate::light_sync::add_light_play_snapshot::<SunClass>(app);
        crate::light_sync::add_light_play_snapshot::<MoonClass>(app);
        crate::light_sync::add_light_play_snapshot::<Sky>(app);
        crate::light_sync::add_light_play_snapshot::<EustressAtmosphere>(app);
        crate::light_sync::add_light_play_snapshot::<Atmosphere>(app);
        crate::light_sync::add_light_play_snapshot::<Clouds>(app);
        crate::light_sync::add_light_play_snapshot::<BloomEffect>(app);
        crate::light_sync::add_light_play_snapshot::<SunRaysEffect>(app);
        crate::light_sync::add_light_play_snapshot::<ColorCorrectionEffect>(app);
        crate::light_sync::add_light_play_snapshot::<ColorGradingEffect>(app);
    }
}

/// The Lighting service as it was when Play started.
#[derive(Resource, Default)]
struct LightingPlaySnapshot(Option<LightingService>);

/// Snapshot the Lighting service as Play starts from Edit (resuming from
/// Pause also enters Playing, and keeps the first snapshot), and put it back
/// when Edit mode returns, after the scripts' `on_exit`, so an exit handler's
/// change does not survive Stop either.
fn add_lighting_play_snapshot(app: &mut App) {
    use crate::play_mode::PlayModeState;
    app.init_resource::<LightingPlaySnapshot>()
        .add_systems(
            OnTransition { exited: PlayModeState::Editing, entered: PlayModeState::Playing },
            snapshot_lighting_on_play,
        )
        .add_systems(
            OnEnter(PlayModeState::Editing),
            restore_lighting_on_stop.after(crate::soul::rune_api::cleanup_scripts_on_stop),
        );
}

fn snapshot_lighting_on_play(lighting: Res<LightingService>, mut snapshot: ResMut<LightingPlaySnapshot>) {
    snapshot.0 = Some(lighting.clone());
}

fn restore_lighting_on_stop(mut snapshot: ResMut<LightingPlaySnapshot>, mut lighting: ResMut<LightingService>) {
    let Some(authored) = snapshot.0.take() else { return };
    // Written only when it differs, so an untouched service does not wake
    // every system that watches it.
    let same = matches!(
        (toml::Value::try_from(&*lighting), toml::Value::try_from(&authored)),
        (Ok(live), Ok(saved)) if live == saved
    );
    if !same {
        *lighting = authored;
    }
}

/// Hydrate file-loaded Lighting/ entities with real ECS components.
///
/// The file loader spawns `Instance` entities from `Lighting/*.instance.toml`
/// but only attaches generic components (Instance, Transform, Visibility,
/// Attributes, Name). This system detects entities with lighting class names
/// that lack their real Bevy components and attaches:
///
/// - **Star → DirectionalLight + SunMarker + SunClass + cascade shadows + SunDisk**,
///   from the shared `lighting_plugin::sun_light` the Player uses too
/// - **Moon → DirectionalLight + MoonMarker + MoonClass**, from `moon_light`
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
/// the cloud layer and the post effects the renderer honours (bloom, sun
/// rays and the two colour grades).
fn is_lighting_fx(class: ClassName) -> bool {
    matches!(
        class,
        ClassName::Clouds
            | ClassName::BloomEffect
            | ClassName::SunRaysEffect
            | ClassName::ColorCorrectionEffect
            | ClassName::ColorGradingEffect
    )
}

/// An effect object's component from its document by the one reader,
/// `celestial_sections::CelestialBody::from_document` (its own section, and
/// under it a Roblox import's `[properties.extras]`), which the Player uses
/// too; the class default when there is no document.
fn fx_body(class: ClassName, doc: Option<&toml::Value>) -> Option<celestial_sections::CelestialBody> {
    match doc {
        Some(d) => celestial_sections::CelestialBody::from_document(class, d),
        None => celestial_sections::CelestialBody::from_section(class, None),
    }
}

/// A `BloomEffect` from its `[bloom]` section or imported extras.
pub fn bloom_from_toml(doc: Option<&toml::Value>) -> BloomEffect {
    match fx_body(ClassName::BloomEffect, doc) {
        Some(celestial_sections::CelestialBody::Bloom(b)) => b,
        _ => BloomEffect::default(),
    }
}

/// A `SunRaysEffect` from its `[sun_rays]` section or imported extras.
pub fn sun_rays_from_toml(doc: Option<&toml::Value>) -> SunRaysEffect {
    match fx_body(ClassName::SunRaysEffect, doc) {
        Some(celestial_sections::CelestialBody::SunRays(r)) => r,
        _ => SunRaysEffect::default(),
    }
}

/// A cloud layer from its `[clouds]` section over the fair-weather default.
pub fn clouds_from_toml(doc: Option<&toml::Value>) -> Clouds {
    match fx_body(ClassName::Clouds, doc) {
        Some(celestial_sections::CelestialBody::Clouds(c)) => c,
        _ => Clouds::default(),
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

/// Build the component for a Clouds, Bloom, SunRays or colour-grade object
/// from its TOML file (defaults when the file cannot be read).
fn hydrate_lighting_fx(commands: &mut Commands, entity: Entity, class: ClassName, path: Option<&std::path::Path>) {
    use celestial_sections::CelestialBody as Body;
    let doc = read_instance_doc(path);
    let mut target = commands.entity(entity);
    match fx_body(class, doc.as_ref()) {
        Some(Body::Clouds(c)) => {
            target.try_insert(c);
        }
        Some(Body::Bloom(b)) => {
            target.try_insert(b);
        }
        Some(Body::SunRays(r)) => {
            target.try_insert(r);
        }
        Some(Body::ColorCorrection(g)) => {
            target.try_insert(g);
        }
        Some(Body::ColorGrading(g)) => {
            target.try_insert(g);
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
        Has<ColorCorrectionEffect>,
        Has<ColorGradingEffect>,
    )>,
) {
    if pending.0.is_empty() {
        return;
    }
    let queued = std::mem::take(&mut pending.0);
    for &entity in &queued {
        let (Ok((instance, ..)), Ok((file, clouds, bloom, sun_rays, correction, grading))) =
            (candidates.get(entity), fx_candidates.get(entity))
        else {
            continue;
        };
        let missing = match instance.class_name {
            ClassName::Clouds => !clouds,
            ClassName::BloomEffect => !bloom,
            ClassName::SunRaysEffect => !sun_rays,
            ClassName::ColorCorrectionEffect => !correction,
            ClassName::ColorGradingEffect => !grading,
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
        // the Lighting service's: the clock and the latitude. The shared
        // `sun_light` builds the rest, exactly as the Player builds it.
        let sun_class = authored.cloned().unwrap_or_else(celestial_sections::default_sun);
        commands.entity(entity).insert((
            eustress_common::plugins::lighting_plugin::sun_light(
                &sun_class,
                &lighting,
                crate::photoreal::contact_shadows_on(),
            ),
            LightingServiceOwner,
        ));
    }

    // ── Moon → DirectionalLight + MoonMarker + MoonClass ──────────────
    for (entity, authored) in unhydrated_moon {
        info!("🌙 Hydrating Moon entity {:?} from Lighting/ TOML", entity);
        // The Moon's values came from its `[moon]` section when it spawned.
        // The shared `moon_light` leaves the light dark until
        // `update_moon_position` places it on the next frame.
        commands.entity(entity).insert((
            eustress_common::plugins::lighting_plugin::moon_light(&authored.cloned().unwrap_or_default()),
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

/// Carry the Lighting service's properties, as the Properties panel and the
/// Space's `_service.toml` spell them, into the live `LightingService`.
///
/// The panel edits the `ServiceComponent` first; this applies it. See
/// [`apply_lighting_properties`] for the keys.
fn sync_service_properties_to_lighting(
    mut lighting: ResMut<LightingService>,
    service_query: Query<&crate::space::service_loader::ServiceComponent, Changed<crate::space::service_loader::ServiceComponent>>,
) {
    for service in service_query.iter() {
        if service.class_name != "Lighting" {
            continue;
        }
        apply_lighting_properties(&service.properties, &mut lighting);
    }
}

/// Give a Lighting service loaded without some of its properties those
/// properties, at the values the live `LightingService` already has for
/// them, so the panel's edit of that row is saved instead of dropped (the
/// panel's write path only updates keys the component holds) and nothing
/// the Space renders changes.
///
/// A file written before a key was renamed keeps its value: `GlobalShadows`
/// was saved as `shadows_enabled` and `DayCycle` as `cycle_enabled`.
fn seed_lighting_service_properties(
    mut services: Query<&mut crate::space::service_loader::ServiceComponent, Added<crate::space::service_loader::ServiceComponent>>,
) {
    let defaults = LightingService::default();
    for mut service in services.iter_mut() {
        if service.class_name != "Lighting" {
            continue;
        }
        let missing: Vec<(&'static str, crate::space::service_loader::PropertyValue)> =
            lighting_property_defaults(&defaults)
                .into_iter()
                .filter(|(key, _)| !service.properties.contains_key(*key))
                .map(|(key, value)| {
                    let legacy = match key {
                        "global_shadows" => service.properties.get("shadows_enabled").cloned(),
                        "day_cycle" => service.properties.get("cycle_enabled").cloned(),
                        _ => None,
                    };
                    (key, legacy.unwrap_or(value))
                })
                .collect();
        if missing.is_empty() {
            continue;
        }
        let count = missing.len();
        for (key, value) in missing {
            service.properties.insert(key.to_string(), value);
        }
        info!("Lighting: seeded {count} missing properties");
    }
}

/// Studio's Lighting properties (the service component's), as the shared
/// parser reads them.
struct ServiceLightingProperties<'a>(&'a LightingProps);

impl eustress_common::services::lighting_properties::LightingProperties for ServiceLightingProperties<'_> {
    fn value(&self, key: &str) -> Option<eustress_common::services::lighting_properties::LightingValue<'_>> {
        use crate::space::service_loader::PropertyValue as P;
        use eustress_common::services::lighting_properties::LightingValue as V;
        Some(match self.0.get(key)? {
            P::Float(v) => V::Number(*v as f32),
            P::Int(v) => V::Number(*v as f32),
            P::Bool(v) => V::Bool(*v),
            P::String(s) => V::Text(s.as_str()),
            P::Vec3(v) => V::Color([v[0] as f32, v[1] as f32, v[2] as f32, 1.0]),
            P::Vec4(v) => V::Color([v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32]),
        })
    }
}

type LightingProps = std::collections::HashMap<String, crate::space::service_loader::PropertyValue>;

/// The Lighting service's properties applied to `lighting` by the shared
/// parser, `lighting_properties::apply_lighting_properties`, which the
/// Player uses on the same file. [`lighting_property_defaults`] lists the
/// panel's keys, and a test walks the panel's schema and fails on any row
/// whose key reaches nothing.
pub fn apply_lighting_properties(props: &LightingProps, lighting: &mut LightingService) {
    eustress_common::services::lighting_properties::apply_lighting_properties(
        &ServiceLightingProperties(props),
        lighting,
    );
}

/// The keys the Properties panel writes for the Lighting service, at
/// `defaults`' values: what `seed_lighting_service_properties` gives a
/// service that lacks one.
pub fn lighting_property_defaults(
    defaults: &LightingService,
) -> Vec<(&'static str, crate::space::service_loader::PropertyValue)> {
    use crate::space::service_loader::PropertyValue as P;
    let color = |c: [f32; 4]| P::Vec4([c[0] as f64, c[1] as f64, c[2] as f64, c[3] as f64]);
    let d = defaults;
    vec![
        ("ambient", color(d.ambient)),
        ("brightness", P::Float(d.brightness as f64)),
        ("color_shift_bottom", color(d.color_shift_bottom)),
        ("color_shift_top", color(d.color_shift_top)),
        ("environment_diffuse_scale", P::Float(d.environment_diffuse_scale as f64)),
        ("environment_specular_scale", P::Float(d.environment_specular_scale as f64)),
        ("global_shadows", P::Bool(d.shadows_enabled)),
        ("outdoor_ambient", color(d.outdoor_ambient)),
        ("exposure_compensation", P::Float(d.exposure_compensation as f64)),
        ("clock_time", P::Float((d.time_of_day * 24.0) as f64)),
        ("geographic_latitude", P::Float(d.geographic_latitude as f64)),
        ("day_cycle", P::Bool(d.cycle_enabled)),
        ("day_length_minutes", P::Float(d.day_length_minutes as f64)),
        ("fog_color", color(d.fog_color)),
        ("fog_end", P::Float(d.fog_end as f64)),
        ("fog_start", P::Float(d.fog_start as f64)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> toml::Value {
        text.parse::<toml::Value>().expect("test TOML parses")
    }

    #[test]
    fn stop_puts_back_the_lighting_and_sky_play_changed() {
        use crate::play_mode::PlayModeState;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin));
        app.init_state::<PlayModeState>().init_resource::<LightingService>();
        add_lighting_play_snapshot(&mut app);
        crate::light_sync::add_light_play_snapshot::<Clouds>(&mut app);
        app.update();
        let enter = |app: &mut App, state: PlayModeState| {
            app.world_mut().resource_mut::<NextState<PlayModeState>>().set(state);
            app.update();
        };
        let layer = app.world_mut().spawn(Clouds::default()).id();
        enter(&mut app, PlayModeState::Playing);
        app.world_mut().resource_mut::<LightingService>().brightness = 9.0;
        app.world_mut().get_mut::<Clouds>(layer).unwrap().coverage = 0.95;
        // Pausing and resuming keeps the snapshot Play started with.
        enter(&mut app, PlayModeState::Paused);
        enter(&mut app, PlayModeState::Playing);
        app.world_mut().resource_mut::<LightingService>().clock_time = "03:00:00".to_string();
        enter(&mut app, PlayModeState::Editing);
        let lighting = app.world().resource::<LightingService>();
        assert_eq!(lighting.brightness, LightingService::default().brightness);
        assert_eq!(lighting.clock_time, LightingService::default().clock_time);
        assert_eq!(app.world().get::<Clouds>(layer).unwrap().coverage, Clouds::default().coverage);
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
        use eustress_common::classes::CloudLayerType;
        let d = doc("[clouds]\nlayer_type = \"Cirrus\"\n");
        assert_eq!(clouds_from_toml(Some(&d)).layer_type, CloudLayerType::Cirrus);
        let d = doc("[clouds]\nlayer_type = \"nonsense\"\n");
        assert_eq!(clouds_from_toml(Some(&d)).layer_type, CloudLayerType::Cumulus);
    }

    const LIGHTING_SCHEMA: &str = include_str!("../../../common/assets/service_properties/Lighting.toml");

    /// A value unlike `v`, of the same kind.
    fn changed(v: &crate::space::service_loader::PropertyValue) -> crate::space::service_loader::PropertyValue {
        use crate::space::service_loader::PropertyValue as P;
        match v {
            P::Bool(b) => P::Bool(!b),
            P::Int(i) => P::Int(i + 3),
            P::Float(f) => P::Float(f + 1.5),
            P::String(s) => P::String(format!("{s}x")),
            P::Vec3(_) => P::Vec3([0.25, 0.5, 0.75]),
            P::Vec4(_) => P::Vec4([0.25, 0.5, 0.75, 1.0]),
        }
    }

    /// `lighting` after applying `props` to the default service, as text.
    fn applied(props: LightingProps) -> String {
        let mut lighting = LightingService::default();
        apply_lighting_properties(&props, &mut lighting);
        format!("{lighting:?}")
    }

    #[test]
    fn every_lighting_row_the_panel_saves_reaches_the_renderer() {
        // Each editable row saves under its name in snake_case (or its
        // `source`); that key must be seeded and must change the service.
        // GlobalShadows saved as `global_shadows` and was read as
        // `shadows_enabled`, so it reset on every reload.
        let schema: toml::Value = LIGHTING_SCHEMA.parse().expect("the Lighting schema parses");
        let defaults = lighting_property_defaults(&LightingService::default());
        let untouched = format!("{:?}", LightingService::default());
        let mut rows = 0;
        for (section, entries) in schema.as_table().expect("a table") {
            let Some(entries) = entries.as_table() else { continue };
            for (name, row) in entries {
                if row.get("readonly").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                let key = row
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| crate::ui::slint_ui::to_snake_case(name));
                let default = defaults
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v)
                    .unwrap_or_else(|| panic!("[{section}] {name} saves as `{key}`, which is not seeded"));
                let props: LightingProps = [(key.clone(), changed(default))].into_iter().collect();
                assert_ne!(applied(props), untouched, "[{section}] {name} (`{key}`) reaches nothing");
                rows += 1;
            }
        }
        assert!(rows >= 15, "only {rows} editable rows");
    }

    #[test]
    fn a_panel_edit_survives_save_and_load() {
        // What the panel stores for a row, written to `_service.toml` and
        // read back the way a Space loads, still reaches the service.
        use crate::space::service_loader::{property_value_to_toml, toml_to_property_value};
        let untouched = format!("{:?}", LightingService::default());
        for (key, default) in lighting_property_defaults(&LightingService::default()) {
            let saved = property_value_to_toml(&changed(&default));
            let loaded = toml_to_property_value(&saved).unwrap_or_else(|| panic!("`{key}` does not read back"));
            let props: LightingProps = [(key.to_string(), loaded)].into_iter().collect();
            assert_ne!(applied(props), untouched, "`{key}` is lost between save and load");
        }
    }

    #[test]
    fn global_shadows_and_the_day_cycle_read_either_spelling() {
        use crate::space::service_loader::PropertyValue as P;
        for key in ["global_shadows", "shadows_enabled"] {
            let props: LightingProps = [(key.to_string(), P::Bool(false))].into_iter().collect();
            let mut lighting = LightingService::default();
            apply_lighting_properties(&props, &mut lighting);
            assert!(!lighting.shadows_enabled, "{key}");
        }
        for key in ["day_cycle", "cycle_enabled"] {
            let props: LightingProps = [(key.to_string(), P::Bool(true))].into_iter().collect();
            let mut lighting = LightingService::default();
            apply_lighting_properties(&props, &mut lighting);
            assert!(lighting.cycle_enabled, "{key}");
        }
    }

    #[test]
    fn a_whole_number_is_read_as_a_number() {
        use crate::space::service_loader::PropertyValue as P;
        let props: LightingProps = [("brightness".to_string(), P::Int(4))].into_iter().collect();
        let mut lighting = LightingService::default();
        apply_lighting_properties(&props, &mut lighting);
        assert_eq!(lighting.brightness, 4.0);
    }
}
