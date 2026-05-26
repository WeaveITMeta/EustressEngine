# Lighting Audit + Rust Backing Plan (Wave 1 SPEC)

**Status:** READ-ONLY audit. No code modified. Delivered as text for save to `E:/Workspace/EustressEngine/docs/architecture/LIGHTING_AUDIT.md`.

**Date:** 2026-05-26
**Scope:** Lighting subsystem audit + Rust backing plan for Waves 2–4.

> **Cross-reference notice:** This document references `CLASS_REGISTRY.md` and `RENDER_CASCADE.md` per the task brief. Neither file currently exists under `E:/Workspace/EustressEngine/docs/architecture/` (verified). The plan below specifies the contracts these documents are expected to define. Wave 2 must produce those prerequisites or this plan loses its anchors.

---

## 1. Executive Summary

The Eustress lighting stack is **partially scaffolded but not wired end-to-end**. Concretely:

- **Celestial path is live.** `Star` (alias `Sun`), `Moon`, `Sky`, `Atmosphere` round-trip through TOML, get hydrated into Bevy `DirectionalLight + SunDisk + cascade shadows`, and respond to property edits via the `LightingService` resource and `ServiceComponent` change detection. See `eustress/crates/engine/src/plugins/lighting_plugin.rs:89` (`hydrate_lighting_entities`) and `eustress/crates/common/src/plugins/lighting_plugin.rs:34` (`SharedLightingPlugin`).
- **Discrete-light classes are stranded.** `PointLight`, `SpotLight`, `SurfaceLight`, `DirectionalLight` have struct definitions, `ClassName` enum entries, in-memory spawn functions, `PropertyAccess` impls, and binary class IDs — but **no file-loader hydration path**. A `.instance.toml` for a `PointLight` placed under a Space lands as a propertyless non-visual `Instance` entity; the `bevy_pbr::PointLight` component never attaches.
- **No `Changed<T>`-reactive sync system.** When the Properties panel edits `EustressPointLight.brightness`, nothing copies that change into the sibling `bevy_pbr::PointLight.intensity`. The discrete-light spawn path only sets the Bevy component once, at spawn (`eustress/crates/engine/src/spawn.rs:409`).
- **No `ClassSpawner` trait.** The orchestration the brief assumes does not exist in this repo. Wave 2's `CLASS_REGISTRY.md` must define it.
- **No `RENDER_CASCADE.md` LOD policy.** All discrete lights currently spawn full-resolution with shadows enabled by default. No tier-based throttling.
- **Post-processing is unimplemented.** `Bloom`, `DepthOfField`, `ColorCorrection`, `Blur`, `SunRays` from FEATURE_PARITY §9 have zero call sites in the engine crate.

Coverage by FEATURE_PARITY §9 (23 items): **3 done, 5 partial, 15 not started**.

---

## 2. Current State Inventory

### 2.1 PointLight

| Aspect | Status | Evidence |
|---|---|---|
| Class struct | **Defined** as `EustressPointLight` | `eustress/crates/common/src/classes.rs:1487–1531` |
| Fields | `brightness: f32`, `color: Color`, `range: f32`, `radius: f32`, `shadows: bool`, `texture: Option<String>` | classes.rs:1491–1518 |
| `ClassName` enum | **Yes** — `ClassName::PointLight` | classes.rs:215, 329, 430 |
| Spawn path (in-memory) | `spawn_point_light` | `eustress/crates/engine/src/spawn.rs:409` |
| Spawn from JSON scene | `pointlight_from_properties` + dispatch | `eustress/crates/engine/src/serialization/scene.rs:911, 1212` |
| Spawn from `.instance.toml` | **Missing** — no match arm in `space/instance_loader.rs:1413` (`spawn_instance`); files would fall through to the no-mesh non-visual `Instance` path at line 1535 | gap |
| TOML schema | **Missing** — no template under `eustress/crates/engine/assets/lighting_templates/`; only Sun/Moon/Sky/Atmosphere ship there. The advertised filename pattern is `.pointlight.toml` (`class_conversion.rs:320`) but nothing reads it | gap |
| Property panel category | `"Light"` category, 4 descriptors: Brightness, Color, Range, Shadows | `eustress/crates/common/src/properties.rs:510–539` |
| Bevy component mapping | `bevy::prelude::PointLight` (intensity = brightness lumens, range, radius, shadows_enabled). `PointLightTexture` cookie planned via `texture` field but `TODO` left at `spawn.rs:416` | spawn.rs:418–425 |
| Hot-reload (Property panel → Bevy) | **No** — `Changed<EustressPointLight>` system does not exist (`grep` returned no results) | gap |
| Hot-reload (TOML file watcher → ECS) | Indirect — `file_watcher.rs` triggers reload of `.instance.toml`, but with no light-aware hydration the reload just respawns a propertyless Instance | gap |
| Binary-ECS round-trip via Fjall | **Partial** — `ClassId::PointLight` exists (`binary.rs:446`), `instance_to_arch` (`arch_instance.rs:108`) preserves `class_name` and the cold `extra` tail as `EusValue`, but light-specific fields (brightness/range/etc.) have no typed slot in `ArchInstanceCore` (`worlddb/src/rkyv_values.rs:228`). They'd survive the round-trip only because they land in the `[Light]` section of `extra` and serialize through `EusValue::Table`. Untested. | partial |
| Spawn-from-toolbox UI action | Yes — `UIAction::SpawnPointLight` (`ui/world_view.rs:890`) and right-click Insert-into-selection (`ui/world_view.rs:1035`) | OK |
| Icon | `pointlight.svg` ships at `eustress/crates/engine/assets/icons/` | OK |

### 2.2 SpotLight

| Aspect | Status | Evidence |
|---|---|---|
| Class struct | **Defined** as `EustressSpotLight` | `eustress/crates/common/src/classes.rs:1533–1564` |
| Fields | `brightness`, `color`, `range`, `angle` (degrees, cone), `shadows`, `texture: Option<String>` | classes.rs:1538–1551 |
| `ClassName` enum | **Yes** | classes.rs:216, 330 |
| Spawn path | `spawn_spot_light` (sets `inner_angle = angle * 0.85`, `outer_angle = angle`) | `eustress/crates/engine/src/spawn.rs:437–460` |
| Spawn from `.instance.toml` | **Missing** | gap |
| TOML schema | **Missing** | gap |
| Property panel | 5 properties listed in `get_property`, but only 3 emitted by `list_properties` — **Brightness/Color/Range/Shadows/Angle returned by get, but list_properties omits Range and Shadows** | `properties.rs:545–575` (bug) |
| Bevy mapping | `bevy::prelude::SpotLight` | spawn.rs:446–453 |
| Hot-reload | **No** | gap |
| Binary-ECS | Same status as PointLight: enum entry exists (`binary.rs:447`), no typed rkyv slot | partial |
| Spawn-from-toolbox UI | Yes (`UIAction::SpawnSpotLight`, world_view.rs:919) | OK |

### 2.3 SurfaceLight

| Aspect | Status | Evidence |
|---|---|---|
| Class struct | **Defined** as `SurfaceLight` (no `Eustress` prefix — naming inconsistency) | `eustress/crates/common/src/classes.rs:1566–1595` |
| Fields | `brightness`, `color`, `range`, `face: String` ("Top"/"Bottom"/…), `shadows`, `texture: Option<String>` | classes.rs:1570–1582 |
| `ClassName` enum | **Yes** | classes.rs:217, 331 |
| Spawn path | `spawn_surface_light` — currently spawns a plain `PointLight` with `intensity = brightness * 500.0` and **ignores the `face` field entirely** | `eustress/crates/engine/src/spawn.rs:462–483` (semantic bug) |
| Spawn from `.instance.toml` | **Missing** | gap |
| TOML schema | **Missing** | gap |
| Property panel | 5 properties (Brightness, Color, Range, Shadows, Face) — but `list_properties` only emits 3 (Brightness, Color, Face), dropping Range and Shadows | `properties.rs:988–1018` (bug) |
| Bevy mapping | None native — Bevy has no "face emitter on a part" primitive. Currently approximated as PointLight at part origin. Roblox renders it as an emissive surface offset from the face. | gap / design decision |
| Hot-reload | **No** | gap |
| Binary-ECS | Enum entry exists (`binary.rs:448`); no typed rkyv slot | partial |

### 2.4 DirectionalLight

| Aspect | Status | Evidence |
|---|---|---|
| Class struct | **Defined** as `EustressDirectionalLight` | `eustress/crates/common/src/classes.rs:1597–1643` |
| Fields | `brightness`, `color`, `shadows`, `shadow_depth_bias`, `shadow_normal_bias`, `texture: Option<String>` | classes.rs:1603–1627 |
| `ClassName` enum | **Yes** | classes.rs:218, 332 |
| Spawn path | `spawn_directional_light` (`spawn.rs:489–511`). Multiplies brightness by 10000.0 to get Bevy `illuminance` lux | spawn.rs:498–505 |
| Spawn from `.instance.toml` | **Missing** for the standalone DirectionalLight class. The Sun/Moon hydration path in `lighting_plugin.rs:101` attaches a `DirectionalLight` *for the Sun/Moon classes* — that's a different entity flow | gap |
| TOML schema | **Missing** as a discrete class. Sun.instance.toml exists for the Sun class (different class). | gap |
| Property panel | Only 2 properties emitted (Brightness, Shadows) — 4-line stub on a one-liner `impl PropertyAccess` | `properties.rs:1482–1490` (incomplete) |
| Bevy mapping | `bevy::prelude::DirectionalLight` (lux), optional `DirectionalLightTexture` cookie | OK at spawn |
| Hot-reload | **No** | gap |
| Binary-ECS | Enum entry exists (`binary.rs:449`); no typed rkyv slot | partial |
| Sun/Moon path (separate) | Functional. `Star` (alias `Sun`) and `Moon` classes have full hydration, latitude-based sun arc math (`Star::elevation` at classes.rs:5442), realistic moon orbital mechanics (`Moon::direction_realistic` referenced at `lighting_plugin.rs:250`) | OK |

### 2.5 Atmosphere subsystem (relevant for §9 audit)

| Aspect | Status | Evidence |
|---|---|---|
| `Atmosphere` class struct | **Defined** with density/offset/color/decay/glare/haze | `eustress/crates/common/src/classes.rs:5074–5093` |
| `EustressAtmosphere` component | **Defined** with Roblox-style props + Bevy 0.17 raymarched-atmosphere props (planet_radius, atmosphere_height, rayleigh_coefficient, mie_*, environment_map_*) | `eustress/crates/common/src/services/lighting.rs:191–276` |
| Hydration | `hydrate_lighting_entities` attaches both components on TOML load | `lighting_plugin.rs:200–209` |
| Bevy mapping | `bevy::pbr::Atmosphere::earthlike(medium)` applied to all `Camera3d` lacking `NoAtmosphere` | `common/src/plugins/lighting_plugin.rs:656–660` |
| Property panel | **2-field stub only** — `Density`, `Haze`. The other 4 Roblox props (offset/color/decay/glare) are unreachable from the UI | `properties.rs:1492–1500` (bug) |
| `SceneAtmosphere` resource | Pushed to all atmosphere'd cameras; updated by `sync_atmosphere_to_rendering` (`engine/lighting_plugin.rs:282`) | OK |

### 2.6 LightingService (the global Roblox-Lighting equivalent)

Fully fleshed out at `eustress/crates/common/src/services/lighting.rs:21–125`. Roblox-style props (`time_of_day`, `clock_time`, `geographic_latitude`, `ambient`, `outdoor_ambient`, `brightness`, `fog_*`, `sun_color`, `sun_intensity`, `sun_angular_radius`, `shadows_enabled`, `shadow_softness`, `sky_color`, `horizon_color`, `exposure_compensation`, `environment_diffuse_scale`, `environment_specular_scale`, `cycle_enabled`, `day_length_minutes`).

`sync_service_properties_to_lighting` (`engine/lighting_plugin.rs:313–393`) reads 17 service properties from `ServiceComponent` change events and pushes them into the `LightingService` resource. **This is the only working hot-reload loop for lighting.** Roblox `Technology`, `ColorShift_Top/Bottom`, color-correction effects, and post-processing are absent.

---

## 3. Gap Analysis — FEATURE_PARITY §9 (23 items)

Reference: `docs/FEATURE_PARITY.md:204–229`.

| # | Item | Status | Estimate | Notes |
|---|---|---|---|---|
| 1 | Atmosphere plugin (Bevy atmosphere) | **DONE** | — | `SharedLightingPlugin` + `apply_atmosphere_to_cameras` |
| 2 | Sky / HDR skybox | **DONE** | — | `create_procedural_skybox` + `SkyboxHandle` |
| 3 | Property sync (ambient, brightness, etc.) | **DONE** | — | `sync_service_properties_to_lighting` |
| 4 | `ClockTime` → sun angle via `GeographicLatitude` | **Partial** | trivial (1h) | Math implemented in `Star::elevation`/`azimuth` and `Star::direction`; needs verification that `LightingService.clock_time` ↔ `Star.time_of_day` two-way sync is bug-free (currently `sync_clock_time_to_sun` + `sync_sun_with_lighting_service` do it but at different cadences) |
| 5 | `TimeOfDay` string parse round-trip | **Partial** | trivial (1h) | `parse_clock_time` exists at lighting_plugin.rs:797; `update_clock_time` exists at services/lighting.rs:135. Need a round-trip test and ensure UI emits both fields. |
| 6 | `Lighting.Technology` enum | **Not started** | moderate (4h) | Bevy doesn't have a runtime-swappable lighting backend; map to enum + ignore or use to select shadow/PCSS quality |
| 7 | `Brightness` multiplier | **Partial** | trivial (1h) | Wired in `update_exposure_compensation` (`common/lighting_plugin.rs:273`) — but applies via ambient brightness; needs a separate global multiplier on sun_intensity + ambient |
| 8 | `Ambient` + `OutdoorAmbient` | **Partial** | trivial (1h) | `ambient` flows through `update_ambient_light`; `outdoor_ambient` is read from ServiceComponent but never used by any system. Wire it for outdoor surfaces only |
| 9 | `ColorShift_Top` / `ColorShift_Bottom` | **Not started** | moderate (4h) | Hemispheric tint added to sky gradient in `create_procedural_skybox`; currently uses hard-coded day/night palettes |
| 10 | `EnvironmentDiffuseScale` / `EnvironmentSpecularScale` | **Partial** | trivial (1h) | Service property exists; needs to drive `EnvironmentMapLight.intensity` in `attach_skybox_to_cameras` (currently hardcoded `400.0`) |
| 11 | `ExposureCompensation` | **Partial** | trivial (1h) | Applies to ambient brightness only (`update_exposure_compensation`); should also set `Camera.exposure_compensation` or use `bevy::core_pipeline::tonemapping` |
| 12 | `GlobalShadows` toggle | **Partial** | trivial (1h) | Service property exists (`shadows_enabled`); does propagate to sun shadows. Need to also affect per-light `shadows_enabled` for PointLight/SpotLight |
| 13 | `ShadowSoftness` PCSS | **Partial** | substantial (1 day) | `shadow_softness` is a property but Bevy doesn't expose PCSS penumbra-radius natively; need custom shadow shader or `CascadeShadowConfig` tuning |
| 14 | `FogColor` / `FogStart` / `FogEnd` | **DONE** | — | `update_fog_settings` (common/lighting_plugin.rs:287) with auto-swap of inverted range |
| 15 | `ColorCorrectionEffect` | **Not started** | moderate (4h) | Use `bevy::core_pipeline::tonemapping::Tonemapping` + `bevy::render::color::ColorGrading` |
| 16 | `BloomEffect` | **Not started** | moderate (4h) | Add `bevy::core_pipeline::bloom::Bloom` to camera, drive from a `BloomEffect` class entity |
| 17 | `BlurEffect` | **Not started** | moderate (4h) | No Bevy built-in full-screen Gaussian; needs custom post-process pass via `bevy::render::view::ViewTarget` |
| 18 | `SunRaysEffect` | **Not started** | substantial (1 day) | Volumetric god-rays; could reuse `bevy::light::VolumetricLight` (currently inserted on Sun at lighting_plugin.rs:159 but not exposed to user) |
| 19 | `DepthOfFieldEffect` | **Not started** | moderate (4h) | `bevy::core_pipeline::dof::DepthOfField` |
| 20 | `Atmosphere` full props exposed | **Partial** | trivial (1h) | Only Density/Haze in `PropertyAccess`; expand to all 6 + the Bevy 0.17 raymarching params |
| 21 | 6-face skybox custom textures | **Not started** | moderate (4h) | `Sky.instance.toml` exposes a `SkyMode` enum with "Skybox" option but no texture asset references for the six faces. Use Bevy's cubemap loader (KTX2 or 6 PNGs assembled) |
| 22 | `MoonTextureId` / `SunTextureId` / `*AngularSize` | **Partial** | moderate (4h) | `Star.texture` and `Moon.texture` fields exist as paths; `angular_size` exists. Spawn path doesn't load the texture or apply it to `SunDisk` — currently `SunDisk` is procedural |
| 23 | `StarCount` + `CelestialBodiesShown` | **Partial** | trivial (1h) | Stars are baked into the procedural skybox at fixed density (~0.8% bright + ~2% dim); `StarCount` from `Sky.instance.toml` is parsed into Attributes but no system reads it. `CelestialBodiesShown` ditto |

**Aggregated effort**: ~3 hours trivial cleanup × 8 items + ~12 hours moderate × 8 items + ~3 days substantial × 2 items ≈ **6–7 engineering days** to close §9 with all items "exists, exposed, hot-reloadable".

Plus the four discrete light classes (PointLight / SpotLight / SurfaceLight / DirectionalLight) which §9 doesn't list but are blockers for any user-authored lighting: **2–3 more days** to land hydration + reactive sync + TOML schemas.

**Total Wave-3 effort: ~9–10 engineering days.**

---

## 4. Rust Backing Plan — per Light Type

### 4.1 Shared Pattern (Wave 2 prerequisite)

`CLASS_REGISTRY.md` must define a trait approximately like this (no existing analogue in the repo — Wave 2 owns the design):

```rust
/// Registry contract: every spawnable class implements this so the
/// .instance.toml loader can dispatch by ClassName without a giant
/// match arm in instance_loader.rs.
pub trait ClassSpawner: Send + Sync + 'static {
    /// The class this spawner handles.
    fn class_name(&self) -> ClassName;

    /// Decode the class-specific component from the TOML `[properties]`
    /// + flattened section tables, then attach it (and any sibling
    /// Bevy components) to `entity`. Returns the new asset/mesh
    /// reference if the class introduces visible geometry, else None.
    fn hydrate(
        &self,
        commands: &mut Commands,
        asset_server: &AssetServer,
        entity: Entity,
        instance: &InstanceDefinition,
    ) -> Result<HydrationResult, ClassError>;

    /// Mirror current ECS state back into an InstanceDefinition for
    /// disk persistence (TOML write-back).
    fn dehydrate(
        &self,
        world: &World,
        entity: Entity,
    ) -> Result<InstanceDefinition, ClassError>;
}
```

Wave 3 then registers `PointLightSpawner`, `SpotLightSpawner`, etc. against this trait.

The `Changed<T>`-based reactivity uses a generic system:

```rust
fn sync_eustress_light_to_bevy<E, B>(
    mut q: Query<(&E, &mut B), Changed<E>>,
)
where
    E: EustressLight,        // adds method `apply_to(&self, b: &mut B)`
    B: Component,
{
    for (eustress, mut bevy) in q.iter_mut() {
        eustress.apply_to(&mut *bevy);
    }
}
```

…or four explicit copies of the same body, one per (Eustress, Bevy) pair. The explicit form is simpler given Bevy's component-coherent borrow model and the count is small.

### 4.2 PointLight

**`ClassSpawner` impl sketch:**

```rust
struct PointLightSpawner;

impl ClassSpawner for PointLightSpawner {
    fn class_name(&self) -> ClassName { ClassName::PointLight }

    fn hydrate(&self, commands, asset_server, entity, def)
        -> Result<HydrationResult, ClassError>
    {
        let light = pointlight_from_properties(&def.properties_as_json_map());
        let xf = Transform::from(def.transform.clone());

        commands.entity(entity).insert((
            bevy::prelude::PointLight {
                color: light.color,
                intensity: light.brightness,        // lumens
                range: light.range,
                radius: light.radius,
                shadows_enabled: light.shadows,
                ..default()
            },
            xf,
            light,                                  // EustressPointLight
        ));

        // Optional cookie texture
        if let Some(path) = &light.texture {
            let handle = asset_server.load::<Image>(path);
            commands.entity(entity).insert(bevy::pbr::PointLightTexture {
                image: handle,
                ..default()
            });
        }

        Ok(HydrationResult::NoMesh)
    }

    fn dehydrate(&self, world, entity) -> Result<InstanceDefinition, ClassError> {
        let light = world.get::<EustressPointLight>(entity)
            .ok_or(ClassError::MissingComponent)?;
        // Build InstanceDefinition with [Light] section
        Ok(make_instance_def(entity, world, ClassName::PointLight,
            &[("Light", light.to_toml_table())]))
    }
}
```

**Sync system signature:**

```rust
fn sync_point_light_to_bevy(
    mut q: Query<
        (&EustressPointLight, &mut bevy::prelude::PointLight),
        Changed<EustressPointLight>,
    >,
) {
    for (e, mut b) in q.iter_mut() {
        b.color = e.color;
        b.intensity = e.brightness;
        b.range = e.range;
        b.radius = e.radius;
        b.shadows_enabled = e.shadows;
    }
}

/// Cookie texture hot-swap: separate system because it requires
/// AssetServer (not allowed alongside `&mut B` in one system without
/// careful access).
fn sync_point_light_texture(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    q: Query<(Entity, &EustressPointLight), Changed<EustressPointLight>>,
    existing: Query<&bevy::pbr::PointLightTexture>,
) {
    for (entity, e) in q.iter() {
        match (&e.texture, existing.get(entity).is_ok()) {
            (Some(path), _) => {
                let handle = asset_server.load::<Image>(path);
                commands.entity(entity).insert(bevy::pbr::PointLightTexture {
                    image: handle, ..default()
                });
            }
            (None, true) => {
                commands.entity(entity).remove::<bevy::pbr::PointLightTexture>();
            }
            (None, false) => {}
        }
    }
}
```

**Property panel category** (additive to `properties.rs:531`):

```rust
fn list_properties(&self) -> Vec<PropertyDescriptor> {
    use PropertyDescriptor as PD;
    vec![
        // Light category
        PD::float("Brightness", "Light", 0.0, 1_000_000.0),
        PD::color("Color",      "Light"),
        PD::float("Range",      "Light", 0.0, 10_000.0),
        PD::float("Radius",     "Light", 0.0, 100.0),   // area light radius — currently in struct but not in list
        PD::bool ("Shadows",    "Light"),
        // Appearance category
        PD::asset("Texture",    "Appearance", "PointLightTexture (cubemap KTX2)"),
    ]
}
```

**LOD policy (per `RENDER_CASCADE.md` spec — to be written in Wave 2):**

| Tier | Range from camera | Behavior |
|---|---|---|
| Hero | 0–100 m | `bevy_pbr::PointLight` with shadows; full radius for area soft-shadow |
| Active | 100–500 m | `bevy_pbr::PointLight` with `shadows_enabled = false`; halved intensity falloff |
| Streamed | 500 m – 5 km | Skip discrete light; contribute the light's color × falloff into a baked light-probe atlas (one entry per ~10 m grid cell). Use Bevy's `IrradianceVolume` once stable, else custom probe baking |
| Horizon | 5 km+ | Drop entirely. If the light is part of a large-radius landmark (lighthouse, city block) it's baked into the panorama cubemap that wraps Earth-curvature horizon |

**Shadow-caster cap policy:** Hard limit `N` shadow-casting PointLights per frame (start with `N = 16`). System sorts by distance to camera and toggles `shadows_enabled` on the N nearest; rest are flipped off. Single Bevy resource `ShadowBudget { max_casters: u32 }` drives this. Sun shadows are independent and always on if `LightingService.shadows_enabled`.

**TOML schema example** (`PointLight.instance.toml` template under `eustress/crates/engine/assets/lighting_templates/PointLight.instance.toml`):

```toml
# PointLight — Omnidirectional point light source
# Authoring unit: meter (intensity in lumens, range in meters)

[metadata]
class_name = "PointLight"
archivable = true
unit = "m"

[transform]
position = [0.0, 2.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [1.0, 1.0, 1.0]

[Light]
Brightness = { type = "float", value = 100000.0, min = 0.0, max = 1000000.0, description = "Intensity in lumens (physically based)" }
Color      = { type = "Color3", value = [1.0, 1.0, 1.0], description = "Light hue" }
Range      = { type = "float", value = 60.0, min = 0.1, max = 10000.0, description = "Falloff distance in meters" }
Radius     = { type = "float", value = 0.0, min = 0.0, max = 100.0, description = "Spherical area-light radius — larger = softer shadows" }
Shadows    = { type = "bool",  value = true, description = "Cast dynamic shadows" }

[Appearance]
Texture = { type = "asset", value = "", description = "Optional cubemap cookie (KTX2) — for gobos / stained glass" }
```

**Hot-reload contract:**

| Property change | Action |
|---|---|
| `brightness`, `color`, `range`, `radius`, `shadows` | In-place mutation of `bevy_pbr::PointLight`. No respawn. |
| `texture` change | Add/remove `PointLightTexture` component (re-uses asset cache). No entity respawn. |
| `metadata.class_name` change | Class change → respawn via `ClassSpawner::dehydrate` then re-`hydrate` against the new class. |
| `transform` change | Already handled generically by `Changed<Transform>` watchers — no light-specific work. |

**Binary-ECS rkyv layout** — `ArchInstanceCore` (`worlddb/src/rkyv_values.rs:228`) currently has no typed light fields. Two options:

1. **Cold tail only** (zero rkyv changes). Light fields live in `extra` under key `__light`:
   ```rust
   ("__light", EusValue::Table(vec![
       ("brightness", EusValue::Float(100000.0)),
       ("color", EusValue::Array(/* rgba floats */)),
       ("range", EusValue::Float(60.0)),
       ("radius", EusValue::Float(0.0)),
       ("shadows", EusValue::Bool(true)),
       ("texture", EusValue::String(/* path */)),
   ]))
   ```
   Round-trip works today via `instance_to_arch`'s flatten of `extra`. Cost: ~80 bytes per light. **Recommended for Wave 3** — keeps rkyv schema stable.

2. **Typed hot core** (rkyv schema bump). Add `ArchLight` sibling struct; emit it when `class_name` is a light. Bumps `RKYV_VALUE_TAG`. Forces a baker migration of all extant Spaces. Reserve for Wave 5+ when lights are dense enough to matter for load time.

Core vs. cold field classification (Wave 3 ships option 1):
- **Core (hot in renderer):** `brightness`, `color`, `range`, `shadows`, `radius` → 5 fields, ~24 bytes packed
- **Cold (rare):** `texture` (only Some<5% of lights)

### 4.3 SpotLight

Differs from PointLight by adding `inner_angle` and `outer_angle`. Roblox-style `Angle` is the cone half-angle; current code interprets it as full outer angle with `inner = 0.85 * outer`. **Open question** below: keep this convention or expose both angles separately.

**`ClassSpawner` impl** is identical structure to PointLight with these substitutions:
- Bevy component: `bevy::prelude::SpotLight`
- Extra fields: `inner_angle.to_radians()`, `outer_angle.to_radians()`
- Cookie type: `bevy::pbr::SpotLightTexture` (2D, not cubemap)

**Sync system signature:**

```rust
fn sync_spot_light_to_bevy(
    mut q: Query<
        (&EustressSpotLight, &mut bevy::prelude::SpotLight),
        Changed<EustressSpotLight>,
    >,
) {
    for (e, mut b) in q.iter_mut() {
        b.color = e.color;
        b.intensity = e.brightness;
        b.range = e.range;
        b.inner_angle = (e.angle * 0.85).to_radians();
        b.outer_angle = e.angle.to_radians();
        b.shadows_enabled = e.shadows;
    }
}
```

**Property panel category** — fix the bug in `properties.rs:568–574` where `list_properties` omits Range and Shadows:

```rust
fn list_properties(&self) -> Vec<PropertyDescriptor> {
    vec![
        PD::float("Brightness", "Light", 0.0, 1_000_000.0),
        PD::color("Color",      "Light"),
        PD::float("Range",      "Light", 0.0, 10_000.0),  // currently missing
        PD::float("Angle",      "Light", 0.0, 180.0),
        PD::bool ("Shadows",    "Light"),                  // currently missing
        PD::asset("Texture",    "Appearance", "SpotLight cookie (PNG/KTX2)"),
    ]
}
```

**LOD policy:**

| Tier | Range | Behavior |
|---|---|---|
| Hero | 0–100 m | Full `SpotLight` with shadows |
| Active | 100–500 m | `shadows_enabled = false`; cone-cull contribution to nearby surfaces only |
| Streamed | 500 m – 5 km | Bake into directional light-probe contribution (axis = transform forward) |
| Horizon | 5 km+ | Drop |

**TOML schema:**

```toml
[metadata]
class_name = "SpotLight"
archivable = true
unit = "m"

[transform]
position = [0.0, 5.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]  # -Z is the cone axis (Bevy convention)
scale = [1.0, 1.0, 1.0]

[Light]
Brightness = { type = "float", value = 40000.0, description = "Intensity in lumens" }
Color      = { type = "Color3", value = [1.0, 1.0, 1.0] }
Range      = { type = "float", value = 60.0 }
Angle      = { type = "float", value = 45.0, min = 0.0, max = 180.0, description = "Outer cone angle (degrees)" }
Shadows    = { type = "bool",  value = true }

[Appearance]
Texture = { type = "asset", value = "", description = "2D cookie texture — gobo / stained glass / window frame" }
```

### 4.4 SurfaceLight

The most architecturally awkward: Roblox renders this as an emissive face on the part it's parented to. Bevy has no direct equivalent. Options (decided in Wave 2 via §7 open question):

**Option A — Emissive quad (recommended).**
- On hydrate, create a child entity with a unit-quad mesh sized to the parent face dimensions.
- Set the quad's material `emissive = color * brightness`.
- Add a `bevy_pbr::PointLight` offset 0.05 m along the face normal with `intensity = brightness * factor` to actually illuminate other geometry.
- The quad's normal aligns with the named face: `"Top"/"Bottom"/"Front"/"Back"/"Left"/"Right"` → ±X/±Y/±Z in the part's local frame.

**Option B — Face-emitting PointLight (current code, semantically wrong).**
- Just spawns a PointLight at the part's center. Ignores `face` entirely. The emissive surface is not visible. This is what `spawn.rs:471–477` does today.

**Option C — Area light via `bevy::light::Spotlight` with wide cone.**
- Use a SpotLight aimed along the face normal with `outer_angle = 90°` and `radius` matching part dimensions. No quad. Pragmatic but doesn't render the bright surface, just illuminates outward.

**Recommended decision:** **Option A.** It matches Roblox's visual semantics and the dual quad+light split is cheap.

**`ClassSpawner` impl sketch:**

```rust
fn hydrate(&self, commands, asset_server, entity, def) -> Result<...> {
    let sl = surfacelight_from_properties(&def.properties_as_json_map());
    // Resolve face → local normal + half-extents from parent BasePart.size
    let (normal_local, half_extents) = face_to_local(&sl.face, parent_size);
    // Spawn the emissive quad as a CHILD of `entity`
    commands.entity(entity).with_children(|p| {
        p.spawn((
            Mesh3d(quad_mesh_handle(half_extents)),
            MeshMaterial3d(emissive_material(sl.color, sl.brightness)),
            Transform::from_translation(normal_local * 0.001)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ));
        // The actual emitter
        p.spawn((
            bevy::prelude::PointLight {
                color: sl.color,
                intensity: sl.brightness * AREA_LIGHT_BRIGHTNESS_SCALE,
                range: sl.range,
                shadows_enabled: sl.shadows,
                ..default()
            },
            Transform::from_translation(normal_local * 0.05),
        ));
    });
    commands.entity(entity).insert(sl);
    Ok(HydrationResult::NoMesh)
}
```

**Sync system signature** is more involved because both the child quad's emissive material AND the child PointLight must be synced when `SurfaceLight` changes:

```rust
fn sync_surface_light_to_bevy(
    q: Query<(Entity, &SurfaceLight, &Children), Changed<SurfaceLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mat_q: Query<&MeshMaterial3d<StandardMaterial>>,
    mut light_q: Query<&mut bevy::prelude::PointLight>,
) {
    for (_, sl, children) in q.iter() {
        for &child in children.iter() {
            if let Ok(mat_h) = mat_q.get(child) {
                if let Some(mat) = materials.get_mut(&mat_h.0) {
                    mat.emissive = sl.color * sl.brightness;
                }
            }
            if let Ok(mut pl) = light_q.get_mut(child) {
                pl.color = sl.color;
                pl.intensity = sl.brightness * AREA_LIGHT_BRIGHTNESS_SCALE;
                pl.range = sl.range;
                pl.shadows_enabled = sl.shadows;
            }
        }
    }
}
```

**Face change** triggers a respawn (the child quad's transform must be rebuilt). Implement via a separate `Changed<SurfaceLight>` system that compares old vs. new face and triggers `commands.entity(entity).despawn_descendants()` + re-hydrate when face differs.

**Property panel** — fix `properties.rs:1011–1017` similarly to SpotLight:

```rust
fn list_properties(&self) -> Vec<PropertyDescriptor> {
    vec![
        PD::float("Brightness", "Light", 0.0, 1_000_000.0),
        PD::color("Color",      "Light"),
        PD::float("Range",      "Light", 0.0, 10_000.0),
        PD::enum_("Face",       "Light", &["Top","Bottom","Front","Back","Left","Right"]),
        PD::bool ("Shadows",    "Light"),
        PD::asset("Texture",    "Appearance", "Face emission cookie"),
    ]
}
```

**LOD policy** — same as SpotLight tiers. At Streamed tier, drop both child entities and bake the face emission into the parent part's emissive PBR factor.

**TOML schema:**

```toml
[metadata]
class_name = "SurfaceLight"
archivable = true
parent_required = true   # SurfaceLight needs a BasePart parent

[Light]
Brightness = { type = "float", value = 1.0 }
Color      = { type = "Color3", value = [1.0, 1.0, 1.0] }
Range      = { type = "float", value = 60.0 }
Face       = { type = "enum", value = "Front",
               options = ["Top","Bottom","Front","Back","Left","Right"] }
Shadows    = { type = "bool", value = true }

[Appearance]
Texture = { type = "asset", value = "" }
```

### 4.5 DirectionalLight

Note: the **Sun** uses the celestial path (Sun.instance.toml → hydrate as `DirectionalLight + SunMarker + SunClass`). The discrete `DirectionalLight` class is a *second*, simpler entry point for users who want a directional light **without** the time-of-day machinery (e.g. a stage spotlight in a Studio building).

**`ClassSpawner` impl:**

```rust
fn hydrate(&self, commands, asset_server, entity, def) -> Result<...> {
    let light = directionallight_from_properties(&def.properties_as_json_map());
    let xf = Transform::from(def.transform.clone());
    commands.entity(entity).insert((
        bevy::prelude::DirectionalLight {
            color: light.color,
            illuminance: light.brightness * 10_000.0,
            shadows_enabled: light.shadows,
            shadow_depth_bias: light.shadow_depth_bias,
            shadow_normal_bias: light.shadow_normal_bias,
            ..default()
        },
        xf,
        light,
    ));
    if let Some(path) = &light.texture {
        let handle = asset_server.load::<Image>(path);
        commands.entity(entity).insert(bevy::pbr::DirectionalLightTexture {
            image: handle, ..default()
        });
    }
    Ok(HydrationResult::NoMesh)
}
```

**Sync system signature:**

```rust
fn sync_directional_light_to_bevy(
    mut q: Query<
        (&EustressDirectionalLight, &mut bevy::prelude::DirectionalLight),
        Changed<EustressDirectionalLight>,
    >,
) {
    for (e, mut b) in q.iter_mut() {
        b.color = e.color;
        b.illuminance = e.brightness * 10_000.0;
        b.shadows_enabled = e.shadows;
        b.shadow_depth_bias = e.shadow_depth_bias;
        b.shadow_normal_bias = e.shadow_normal_bias;
    }
}
```

**Property panel** — fix `properties.rs:1482–1490` to expose the full set:

```rust
fn list_properties(&self) -> Vec<PropertyDescriptor> {
    vec![
        PD::float("Brightness",       "Light", 0.0, 100.0, "Illuminance scale (×10⁴ lux)"),
        PD::color("Color",            "Light"),
        PD::bool ("Shadows",          "Light"),
        PD::float("ShadowDepthBias",  "Shadows", 0.0, 1.0),
        PD::float("ShadowNormalBias", "Shadows", 0.0, 10.0),
        PD::asset("Texture",          "Appearance", "Directional cookie — cloud shadows, foliage"),
    ]
}
```

**LOD policy** — only one is the global Sun (it's a special class). Discrete DirectionalLights are rare; cap total count at 4 (Bevy's default `MAX_DIRECTIONAL_LIGHTS = 10` is the hard ceiling). No LOD tiering — directional lights have no falloff.

**TOML schema:**

```toml
[metadata]
class_name = "DirectionalLight"
archivable = true

[transform]
position = [0.0, 50.0, 0.0]   # position is ignored; rotation matters
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [1.0, 1.0, 1.0]

[Light]
Brightness        = { type = "float", value = 1.0, description = "Illuminance scale (multiplies by 10000 to get lux)" }
Color             = { type = "Color3", value = [1.0, 1.0, 1.0] }
Shadows           = { type = "bool",  value = true }
ShadowDepthBias   = { type = "float", value = 0.02, min = 0.0, max = 1.0 }
ShadowNormalBias  = { type = "float", value = 1.8, min = 0.0, max = 10.0 }

[Appearance]
Texture = { type = "asset", value = "", description = "Directional cookie — e.g. cloud_shadows.ktx2" }
```

---

## 5. Atmosphere Subsystem (Wave 1 spec)

Roblox's atmosphere + lighting feature surface maps onto Bevy 0.17's stack as follows:

### 5.1 Time / Sun arc

| Roblox property | Bevy mechanism | Status |
|---|---|---|
| `ClockTime` (float 0–24) | `Star.time_of_day` + `Star::direction()` solar math | wired (lighting_plugin.rs:225–233) |
| `TimeOfDay` (string "HH:MM:SS") | `parse_clock_time` ↔ `update_clock_time` round-trip | wired; needs round-trip test |
| `GeographicLatitude` | `Star.latitude` feeds `Star::elevation()` and `azimuth()` | wired (classes.rs:5442) |

### 5.2 Fog

| Roblox | Bevy | Status |
|---|---|---|
| `FogColor` | `DistanceFog::color` | wired (common/lighting_plugin.rs:301–306) |
| `FogStart` | `FogFalloff::Linear { start }` | wired |
| `FogEnd` | `FogFalloff::Linear { end }` | wired with reversed-range auto-correct |
| (Atmosphere fog beyond linear) | Use `FogFalloff::Exponential` or `ExponentialSquared` for distance haze | not surfaced; planned |

### 5.3 Exposure & Tone Mapping

| Roblox | Bevy | Status |
|---|---|---|
| `ExposureCompensation` | `bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface` + `Camera::exposure_compensation` (Bevy 0.15+) | partial (currently affects ambient only) |
| `Brightness` | Global multiplier on `GlobalAmbientLight.brightness` + `sun.illuminance` | partial |
| `Ambient` / `OutdoorAmbient` | `GlobalAmbientLight` (Bevy 0.15 `GlobalAmbientLight::color`/`brightness`) | partial |

### 5.4 Post-processing effects

All four below attach as **components on the main `Camera3d` entity**. Bevy 0.17 makes them composable; just `commands.entity(camera).insert(...)`.

| Roblox effect | Bevy component | Properties to map |
|---|---|---|
| `BloomEffect` | `bevy::core_pipeline::bloom::Bloom` | `intensity → Bloom.intensity`, `threshold → BloomCompositeMode::Additive` + `Bloom.prefilter.threshold`, `size → Bloom.scale` |
| `BlurEffect` | Custom — Bevy lacks a built-in full-screen Gaussian. Implement via `bevy::render::view::ViewNode` + a 2-pass blur post-process. Reserve for Wave 4. | `size → blur sigma` |
| `SunRaysEffect` | Reuse existing `bevy::light::VolumetricLight` (already inserted on Sun at lighting_plugin.rs:159). Promote to user-controllable via `SunRaysEffect` class. | `intensity → VolumetricLight.intensity`, `spread → density along ray` |
| `DepthOfFieldEffect` | `bevy::core_pipeline::dof::DepthOfField` | `focus_distance`, `near/far_intensity`, `aperture` |
| `ColorCorrectionEffect` | `bevy::render::view::ColorGrading` | `brightness → exposure`, `contrast → contrast`, `saturation → saturation`, `tint_color → mid_color` |

**Spawn pattern.** Each effect is a **class entity** parented under `Lighting/`. The `hydrate` step finds the active main camera and attaches the corresponding Bevy component. The `Changed<BloomEffectClass>` system syncs property edits. When the entity is despawned, the component is removed from the camera.

### 5.5 6-face skybox + Sun/Moon textures

Roblox supports six face textures (`SkyboxBk/Dn/Ft/Lf/Rt/Up`). Bevy uses a single cubemap `Handle<Image>` attached via `bevy::core_pipeline::Skybox`.

**Mapping:**
1. `Sky.SkyMode = "Skybox"` triggers a custom skybox loader that takes six face asset paths.
2. Compose the 6 PNG/KTX2 images at load time into a `TextureFormat::Rgba8UnormSrgb` cubemap via `Image::new(Extent3d { width, height, depth_or_array_layers: 6 }, ...)` — same pattern as `create_procedural_skybox` (common/lighting_plugin.rs:367) but reading from disk instead of synthesizing.
3. Replace `SkyboxHandle.handle` with the loaded cubemap.

**Sun/Moon textures** (`SunTextureId`, `MoonTextureId`):
- `Star.texture` and `Moon.texture` already exist as fields.
- Bevy's `SunDisk` shader doesn't accept a texture — it's procedural.
- Implementation: write a custom `Material` that samples the texture for the sun disc and is rendered as a screen-space billboard via `bevy_sprite::Sprite3d`-style approach, OR fork the `SunDisk` shader. **Defer to Wave 5.** For Wave 3, the texture field is parsed but ignored with a `tracing::warn!`.

### 5.6 Properties panel completeness

The current `PropertyAccess` impl for `Atmosphere` (properties.rs:1492–1500) only exposes 2 of 6 Roblox-compatible properties. Wave 3 should ship the full 6:

```rust
impl PropertyAccess for Atmosphere {
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PD::float("Density", "Appearance", 0.0, 1.0),
            PD::float("Offset",  "Scattering", -1.0, 1.0),
            PD::color("Color",   "Appearance"),
            PD::color("Decay",   "Appearance"),
            PD::float("Glare",   "Appearance", 0.0, 1.0),
            PD::float("Haze",    "Appearance", 0.0, 1.0),
        ]
    }
    // get_property / set_property: extend symmetrically
}
```

---

## 6. Multi-Light Streaming Policy

Tied to `RENDER_CASCADE.md` (Wave 2 spec). Reproduced here for context:

### 6.1 Tier definitions

```
┌─────────────────────────────────────────────────────────────────┐
│ Camera                                                          │
│                                                                 │
│  Hero (0–100 m): full shadow-casting Bevy lights                │
│       └─ ShadowBudget cap (default N = 16 point/spot casters)   │
│       └─ Hero sun: cascade shadows, 4 cascades, 2048 m far      │
│                                                                 │
│  Active (100–500 m): non-shadow Bevy lights                     │
│       └─ All discrete lights, shadows_enabled = false           │
│       └─ Drop SurfaceLight emissive quads here                  │
│                                                                 │
│  Streamed (500 m – 5 km): light-probe atlas                     │
│       └─ Bake each light's color × falloff into nearest          │
│         IrradianceVolume probe cell (~10 m grid)                │
│       └─ Probes resampled every N frames (start: 30)            │
│                                                                 │
│  Horizon (5 km+): panorama-baked                                │
│       └─ Light's contribution folded into the horizon panorama  │
│         cubemap regenerated on time-of-day change               │
└─────────────────────────────────────────────────────────────────┘
```

### 6.2 Tier transition system

```rust
#[derive(Component)]
struct LightLodTier(Tier);

enum Tier { Hero, Active, Streamed, Horizon }

fn assign_light_tiers(
    camera_q: Query<&GlobalTransform, With<Camera3d>>,
    mut light_q: Query<(Entity, &GlobalTransform, &mut LightLodTier),
                       Or<(With<EustressPointLight>,
                           With<EustressSpotLight>,
                           With<SurfaceLight>)>>,
) {
    let Ok(cam) = camera_q.single() else { return };
    let cam_pos = cam.translation();
    for (_, xf, mut tier) in light_q.iter_mut() {
        let d = (xf.translation() - cam_pos).length();
        let new_tier = match d {
            d if d < 100.0  => Tier::Hero,
            d if d < 500.0  => Tier::Active,
            d if d < 5000.0 => Tier::Streamed,
            _               => Tier::Horizon,
        };
        if new_tier != tier.0 {
            tier.0 = new_tier;
        }
    }
}

fn apply_light_tier_effects(
    mut commands: Commands,
    q: Query<(Entity, &LightLodTier, &EustressPointLight),
             Changed<LightLodTier>>,
) {
    for (entity, tier, e) in q.iter() {
        match tier.0 {
            Tier::Hero => {
                commands.entity(entity).insert(bevy::prelude::PointLight {
                    intensity: e.brightness,
                    shadows_enabled: e.shadows,
                    ..default()
                });
            }
            Tier::Active => {
                commands.entity(entity).insert(bevy::prelude::PointLight {
                    intensity: e.brightness * 0.5,
                    shadows_enabled: false,
                    ..default()
                });
            }
            Tier::Streamed => {
                commands.entity(entity).remove::<bevy::prelude::PointLight>();
                commands.entity(entity).insert(StreamedLightContribution {
                    color: e.color, intensity: e.brightness, range: e.range,
                });
            }
            Tier::Horizon => {
                commands.entity(entity).remove::<bevy::prelude::PointLight>();
                commands.entity(entity).remove::<StreamedLightContribution>();
                // Panorama baker handles this entity by entity-id lookup.
            }
        }
    }
}
```

### 6.3 Shadow-caster budget

```rust
#[derive(Resource)]
struct ShadowBudget { max_casters: u32 } // default 16

fn enforce_shadow_budget(
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    mut light_q: Query<(&GlobalTransform, &EustressPointLight,
                        &mut bevy::prelude::PointLight,
                        &LightLodTier)>,
    budget: Res<ShadowBudget>,
) {
    let Ok(cam) = cam_q.single() else { return };
    let cam_pos = cam.translation();
    let mut hero: Vec<_> = light_q.iter_mut()
        .filter(|(_, e, _, t)| t.0 == Tier::Hero && e.shadows)
        .collect();
    hero.sort_by_key(|(xf, _, _, _)|
        (xf.translation() - cam_pos).length() as u32);
    for (i, (_, _, mut pl, _)) in hero.iter_mut().enumerate() {
        pl.shadows_enabled = (i as u32) < budget.max_casters;
    }
}
```

Sun shadows are **not counted** against the discrete-light budget — they use `CascadeShadowConfig` (lighting_plugin.rs:129).

---

## 7. Implementation Order — Wave 3 Checklist

Numbered execution sequence. Each step is independent enough to be a single PR.

1. **Wave 2 prerequisites** (other agent, blocking)
   - Land `docs/architecture/CLASS_REGISTRY.md` defining the `ClassSpawner` trait + central registry.
   - Land `docs/architecture/RENDER_CASCADE.md` defining the 4 LOD tiers + budget rules.
2. **PointLight spawner** (simplest light, validates the pattern)
   - Implement `PointLightSpawner: ClassSpawner`.
   - Add a `PointLight.instance.toml` template under `eustress/crates/engine/assets/lighting_templates/`.
   - Extend `space/space_ops.rs:250` `lighting_children` array to optionally include `PointLight` example.
   - Add file-loader dispatch in `instance_loader.rs:1413` (`spawn_instance`) for `ClassName::PointLight`.
   - **Acceptance:** drop a `MyLight.instance.toml` with `class_name = "PointLight"` in any Space, see it appear and illuminate at runtime.
3. **Property panel: color picker reuse + sliders for PointLight**
   - Fix `properties.rs:531` to include `Radius` + `Texture`.
   - Verify the Slint property panel renders Color3 widget for `Color` (likely already does — `BasePart.Color` works).
4. **Hot-reload via `Changed<EustressPointLight>`**
   - Add `sync_point_light_to_bevy` system to `engine/plugins/lighting_plugin.rs:38`.
   - Add `sync_point_light_texture` system (separate due to AssetServer).
   - **Acceptance:** edit Brightness in Properties panel → light's actual intensity updates within one frame, with no respawn.
5. **SpotLight** (adds inner/outer angle handling)
   - Mirror PointLight steps. Fix `properties.rs:568–574` to include Range and Shadows.
   - Implement `SpotLightSpawner`. Sync system.
6. **SurfaceLight** (adds face + emissive quad)
   - Implement `SurfaceLightSpawner` per §4.4 Option A.
   - Sync system + face-change respawn handler.
   - Fix `properties.rs:1011–1017` (`Range`/`Shadows` missing from list).
   - **Acceptance:** create a Part, add a SurfaceLight child with Face="Top", see emissive surface + illumination.
7. **DirectionalLight** (standalone, distinct from Sun celestial path)
   - Implement `DirectionalLightSpawner`. Sync system.
   - Expand `properties.rs:1482–1490` to all 6 fields (Brightness, Color, Shadows, ShadowDepthBias, ShadowNormalBias, Texture).
8. **Atmosphere properties** (FEATURE_PARITY §9 items 20)
   - Expand `Atmosphere`'s `PropertyAccess` impl (`properties.rs:1492–1500`) to all 6 Roblox-style props.
   - Add a 7th field group for the Bevy 0.17 raymarched-atmosphere params (planet_radius, atmosphere_height, etc.) under an `[Advanced]` category — read-only / advanced toggle in UI.
9. **Post-processing effects, one at a time** (FEATURE_PARITY items 15–19)
   - BloomEffect class → `Bloom` component on camera.
   - ColorCorrectionEffect → `ColorGrading`.
   - DepthOfFieldEffect → `DepthOfField`.
   - SunRaysEffect → promote existing `VolumetricLight` to user-controllable.
   - BlurEffect → custom shader; lowest priority (Roblox uses it for menu blur, rare).
10. **Light streaming cascade** (Wave 4)
    - Implement `LightLodTier` + `assign_light_tiers` + `apply_light_tier_effects` per §6.2.
    - Implement `ShadowBudget` + `enforce_shadow_budget`.
    - Wire `StreamedLightContribution` into a future `IrradianceVolume` baker.
    - Panorama baking for Horizon tier — Wave 5.
11. **Cookie textures** (Bevy 0.17 PointLightTexture/SpotLightTexture/DirectionalLightTexture)
    - Wire `texture` field for all four light classes (currently TODOs at spawn.rs:416/444/496).
    - Asset server integration in each `ClassSpawner::hydrate`.
12. **6-face skybox + Sun/Moon textures** (FEATURE_PARITY items 21, 22)
    - Sky.SkyMode = "Skybox" branch in skybox loader.
    - Star.texture / Moon.texture wired into custom sun-disc material.
13. **Verification**: Bring FEATURE_PARITY §9 to 23/23 checked. Update §9 line items in `docs/FEATURE_PARITY.md`.

---

## 8. Open Questions (Human Decision Required)

1. **SurfaceLight implementation strategy.** Option A (emissive quad + child PointLight), Option B (PointLight only, current code, semantically incorrect), or Option C (wide-cone SpotLight aimed along face normal). Recommended: A. Decide before step 6.
2. **SpotLight inner/outer angle exposure.** Current code synthesizes `inner_angle = outer * 0.85`. Roblox exposes only `Angle` (= outer). Keep this convention or expose `InnerAngle` separately? Decision affects TOML schema stability.
3. **Light area-radius semantic.** `EustressPointLight.radius` is the spherical area-light radius (soft shadows). Roblox doesn't have this. Keep it (Eustress advantage), make it optional with hidden-by-default UI?
4. **`Lighting.Technology` enum mapping.** Roblox's `Legacy/Compatibility/ShadowMap/Voxel/Future` don't map to Bevy. Pick one: expose as enum but ignore the value (cosmetic for migration), or use it to gate shadow quality presets (e.g. "Voxel" → enable VXGI when ready). Recommended: ignore, log a warn when set.
5. **PointLight cookie format.** Bevy's `PointLightTexture` needs a cubemap. Roblox doesn't have point-light cookies. Asset pipeline: do we accept KTX2 cubemaps only, or also auto-build from 6 PNGs at import? Affects `EustressForge`.
6. **Sun.texture default.** Bevy's `SunDisk` is procedural. If a user sets `Star.texture = "sun.png"`, do we silently ignore (current behavior, log warn at spawn.rs:416-equivalent), or implement a custom sun-disc material? Recommended: defer to Wave 5.
7. **Shadow budget default.** `MAX_DIRECTIONAL_LIGHTS = 10` is the Bevy hard ceiling. For point/spot, what's `N`? Recommend 16 on desktop, 4 on integrated GPU; gate by `bevy::render::settings::WgpuSettings`.
8. **Atmosphere ↔ Lighting authority.** Both `Atmosphere` entity (in scene Explorer) and `LightingService` resource control overlapping parameters (fog color appears in both). Which is canonical for fog? Recommendation: `LightingService` is the source of truth; `Atmosphere` is a per-Space override layer.
9. **`outdoor_ambient` semantics.** Roblox distinguishes "indoor" vs. "outdoor" by raycasting against parts marked with `CastShadow`. Bevy has no such distinction. Drop the property silently, or implement a coarse heuristic (use `outdoor_ambient` when the camera is above world.surface_altitude)?

---

## 9. Risks + Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Bevy 0.17 light texture API churn (`PointLightTexture` etc.) is recent and may shift in 0.18 | Medium | Wave-5 rework of cookie systems | Pin Bevy version in Cargo.toml; gate cookie code behind a feature flag `light_textures_v017` |
| Shadow budget enforcement causes flicker as lights swap shadow state | High | Visible artifact, user reports "flickering" | Hysteresis: use 100 m / 110 m thresholds (not the same value for upgrade/downgrade); add 1-frame delay before swap |
| SurfaceLight emissive quad + PointLight pair doubles entity count for SurfaceLights | High | Memory + iteration cost in large scenes | Pool the child entities; only mark the parent `SurfaceLight` `Changed<>` to avoid hot-path scans |
| `Changed<EustressPointLight>` fires every frame if any system mutates the component each frame (e.g. animation) | Medium | Wasted work, possible quad respawn loop on SurfaceLight face change | Compare old/new before respawn (cache last-seen face); add `bypass_change_detection()` pattern used elsewhere (e.g. lighting_plugin.rs:169) |
| Cold-tail `extra` storage for light fields makes round-trip lossy if `EusValue` representation can't round-trip a specific type (e.g. NaN floats) | Low | Lost edits on save/reload | Add a `test_pointlight_rkyv_roundtrip` to `arch_instance.rs` mirroring the existing tests at 287 |
| `LightingService` and `Atmosphere` parameter overlap causes user confusion ("why doesn't my Atmosphere.Color do anything?") | Medium | Friction in property panel | Open question #8 above; document the authority order in `docs/services/Lighting.md` |
| Wave 2 `CLASS_REGISTRY.md` slips, blocking discrete-light hydration | Medium | Wave 3 starts late | Build a stopgap: add explicit match arms in `space/instance_loader.rs:1413` for `ClassName::PointLight/SpotLight/SurfaceLight/DirectionalLight` that call `spawn_point_light` etc. directly. Refactor to ClassSpawner once Wave 2 lands |
| Tier-3 (Streamed) probe baking has no implementation today; depends on Bevy `IrradianceVolume` stability | High | Streamed tier ships as "drop light entirely" instead of probe contribution | Document the partial implementation; gate Streamed-tier behaviour behind a `light_probes_v1` feature flag; Wave 5 fills in |
| `Lighting.Technology` user-set values get silently ignored, eroding trust | Low | Reported "engine ignores my settings" | Emit a `tracing::warn!` once per session per value seen; log to the in-engine console viewer |
| Auto-created lighting templates (`space_ops.rs:250`) re-overwrite user edits on space re-scaffolding | Medium | Lost work | Confirmed: `space_ops.rs:255–264` only writes if the target doesn't exist (`write_file`'s semantics); verify this is non-destructive. Add a test |
| The 60-frame skybox throttle in `regenerate_skybox_on_sun_change` (common/lighting_plugin.rs:729–733) means time-of-day changes lag visibly | Low | Visual stutter on rapid `clock_time` slider changes | Drop throttle to 10 frames during user interaction (detect via Slint event), keep 60 for cycle-driven changes |

---

## 10. Critical Files for Implementation

Five files Wave 3 will touch most heavily:

- `E:\Workspace\EustressEngine\eustress\crates\engine\src\space\instance_loader.rs` — file-loader dispatch must learn the four light classes (currently they fall into the no-mesh non-visual path at line 1535).
- `E:\Workspace\EustressEngine\eustress\crates\engine\src\plugins\lighting_plugin.rs` — register new `sync_*_light_to_bevy` systems alongside `hydrate_lighting_entities` (line 89).
- `E:\Workspace\EustressEngine\eustress\crates\engine\src\spawn.rs` — current spawn functions (lines 409–511) become the body of `ClassSpawner::hydrate` impls; the cookie-texture TODOs at lines 416/444/488/496 land here.
- `E:\Workspace\EustressEngine\eustress\crates\common\src\properties.rs` — `PropertyAccess` impls at lines 510 (PointLight), 545 (SpotLight), 988 (SurfaceLight), 1482 (DirectionalLight), 1492 (Atmosphere) all need `list_properties` audit + expansion.
- `E:\Workspace\EustressEngine\eustress\crates\engine\assets\lighting_templates\` — add `PointLight.instance.toml`, `SpotLight.instance.toml`, `SurfaceLight.instance.toml`, `DirectionalLight.instance.toml` mirroring the existing `Sun.instance.toml` / `Moon.instance.toml` / `Atmosphere.instance.toml` / `Sky.instance.toml` patterns.

Secondary files (smaller edits):

- `E:\Workspace\EustressEngine\eustress\crates\engine\src\space\space_ops.rs` line 250 — extend `lighting_children` array if light examples should auto-scaffold.
- `E:\Workspace\EustressEngine\eustress\crates\engine\src\serialization\scene.rs` lines 911–924, 1212–1282 — JSON dispatch already exists but needs to share the same property-decode path as the new ClassSpawner.
- `E:\Workspace\EustressEngine\eustress\crates\worlddb\src\rkyv_values.rs` line 228 — only touched if option 2 (typed rkyv slot for lights) is taken. Stay with option 1 for Wave 3.
- `E:\Workspace\EustressEngine\eustress\crates\engine\src\commands\property_command.rs` lines 98–117 — already routes light property writes correctly; no change required, but verify after `list_properties` expansion that all new properties are reachable.
