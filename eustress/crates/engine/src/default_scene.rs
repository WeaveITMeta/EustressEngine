use bevy::prelude::*;
use bevy::core_pipeline::tonemapping::Tonemapping;
// R2.2: DepthPrepass enables early-Z (cuts overdraw on dense grids) and is the
// prerequisite for GPU occlusion culling. In `bevy_core_pipeline` (already a
// dep) — needs none of R1's held bevy_anti_alias/bevy_post_process crates.
use bevy::core_pipeline::prepass::DepthPrepass;
// R1 (landed): the post stack. `bevy_anti_alias` / `bevy_post_process` are
// enabled features of the workspace `bevy` dep and resolve at 0.19.0 stable —
// the old "0.18.0-rc.1 only" blocker is gone.
use bevy::render::view::Msaa;
use eustress_common::classes::{Instance, ClassName};
use crate::startup::StartupArgs;

/// Plugin to set up the default scene with camera and ground
/// Lighting is handled by SharedLightingPlugin (same as client)
pub struct DefaultScenePlugin;

impl Plugin for DefaultScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_default_scene);
        app.add_systems(Update, diagnose_scene_once.run_if(bevy::time::common_conditions::once_after_real_delay(std::time::Duration::from_secs(6))));
    }
}

/// Canonical Studio 3D camera bundle — the **single source of truth** for how a
/// Studio camera is built. Both the editor camera and the off-screen AI camera
/// are created from this, so they are identical by construction.
///
/// Why this matters: `SharedLightingPlugin` auto-attaches Skybox / Atmosphere /
/// EnvironmentMapLight to *every* `Camera3d`. If two cameras drift in their
/// view features (tonemapping, MSAA, projection, …) they end up with different
/// `mesh_view_bind_group` shapes against a shared layout → a wgpu validation
/// panic ("N bindings != M bindings"). Building every Studio camera here keeps
/// them in lockstep. Callers add only what's genuinely camera-specific:
/// `EustressCamera` (editor controls) or `AiCamera` + a `RenderTarget::Image`
/// (the off-screen AI camera).
pub fn studio_camera_bundle(name: &str, transform: Transform) -> impl Bundle {
    (
        Camera3d::default(),
        // R1: filmic tonemap (was Reinhard) — neutral-hue ACES-ish curve, the
        // Bevy default; needs the `tonemapping_luts` feature (already enabled).
        Tonemapping::TonyMcMapface,
        transform,
        Projection::Perspective(PerspectiveProjection {
            fov: 70.0_f32.to_radians(),
            near: 0.1,
            far: 10000.0,
            ..default()
        }),
        Instance {
            name: name.to_string(),
            class_name: ClassName::Camera,
            archivable: true,
            id: 0,
            ..Default::default()
        },
        Name::new(name.to_string()),
        // R2.2: depth prepass — early-Z overdraw rejection on the dense binary-ECS
        // grid + prerequisite for GPU occlusion culling. Added to the SHARED bundle
        // so editor + AI camera stay in lockstep against SharedLightingPlugin's
        // view bind-group layout (both get the depth binding identically).
        DepthPrepass,
        // ── R1 post stack (in the SHARED bundle, so editor + AI camera keep
        // identical view-bind-group shapes — see the type docs above) ────────
        //
        // MSAA OFF. This is a perf fix as much as a quality one: nothing ever
        // set `Msaa`, so every Studio camera silently ran Bevy's `Sample4`
        // default — 4× raster bandwidth on the main pass AND on `DepthPrepass`.
        // On a Gaussian-splat scene that is close to pure waste: splats are
        // alpha-blended, so MSAA barely touches them while still charging full
        // price. SMAA below restores (and on splat edges improves) the AA that
        // 4×MSAA was buying, as a single cheap post pass.
        // NOTE: MSAA off is NOT unconditionally a win. Measured on Space1
        // without the splat cloud: `Msaa::Off` + SMAA + bloom moved
        // render+present 27.3 → 31.1 ms/frame — the post passes cost more than
        // the 4× raster saving on a geometrically light scene. The saving
        // scales with overdraw, so it pays off on heavy/alpha-blended content
        // and loses on sparse scenes. Hence every stage below (including this
        // one) is switchable at startup via `photoreal.rs` rather than baked
        // in — see `photoreal::apply_camera_stages`.
        Msaa::Off,
        // Marks this as a Studio camera. `photoreal.rs` attaches the AA / bloom
        // / GTAO / auto-exposure stages to every entity carrying this marker in
        // ONE system, which is what keeps their view-bind-group shapes matched.
        StudioCamera,
    )
}

/// Marker for a camera built by [`studio_camera_bundle`].
///
/// Exists so the opt-in photoreal stages can be applied to **every** Studio
/// camera in one place: any per-camera divergence in view features re-creates
/// the shared-layout wgpu panic described on [`studio_camera_bundle`].
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct StudioCamera;

/// One-shot diagnostic: dump all Camera3d and Mesh3d entities after 3 seconds
fn diagnose_scene_once(
    cameras: Query<(Entity, &Transform, &Camera), With<Camera3d>>,
    meshes: Query<(Entity, &Transform, Option<&Name>), With<Mesh3d>>,
    scene_roots: Query<(Entity, &WorldAssetRoot, Option<&Name>, &Transform)>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
    children_query: Query<&Children>,
    all_entities: Query<(Entity, Option<&Name>)>,
    asset_server: Res<AssetServer>,
) {
    info!("=== SCENE DIAGNOSTIC (3s after startup) ===");
    info!("Total entities: {}", all_entities.iter().count());
    info!("Instance entities: {}", instances.iter().count());
    info!("SceneRoot entities: {}", scene_roots.iter().count());
    for (entity, scene_root, name, transform) in scene_roots.iter().take(5) {
        let child_count = children_query.get(entity).map(|c| c.len()).unwrap_or(0);
        let load_state = asset_server.get_load_state(scene_root.0.id());
        let dep_load_state = asset_server.get_recursive_dependency_load_state(scene_root.0.id());
        let asset_path = asset_server.get_path(scene_root.0.id());
        info!("  SceneRoot {:?} '{}': pos={}, children={}, load_state={:?}, dep_state={:?}, path={:?}",
            entity, name.map(|n| n.as_str()).unwrap_or("unnamed"), transform.translation, child_count, load_state, dep_load_state, asset_path);
    }
    info!("Camera3d entities: {}", cameras.iter().count());
    for (entity, transform, camera) in cameras.iter() {
        info!("  Camera {:?}: pos={} order={} viewport={:?}",
            entity, transform.translation, camera.order, camera.viewport);
    }
    info!("Mesh3d entities: {}", meshes.iter().count());
    // Cap the per-mesh dump: on a 387K-mesh import this loop emitted one log
    // line PER MESH in a single frame — a multi-minute stall (225 s observed).
    // A sample of 20 is plenty for diagnostics; the count above is the real
    // signal.
    for (entity, transform, name) in meshes.iter().take(20) {
        info!("  Mesh {:?} '{}': pos={}",
            entity, name.map(|n| n.as_str()).unwrap_or("unnamed"), transform.translation);
    }
    info!("=== END SCENE DIAGNOSTIC ===");
}

pub fn setup_default_scene(
    mut commands: Commands,
    _meshes: ResMut<Assets<Mesh>>,
    _materials: ResMut<Assets<StandardMaterial>>,
    startup_args: Res<StartupArgs>,
    _asset_server: Res<AssetServer>,
) {
    // Check if we're loading a scene file - if so, skip default content
    let loading_scene_file = startup_args.scene_file.is_some();
    
    if loading_scene_file {
        println!("🎬 Scene file specified - skipping default scene content...");
    } else {
        println!("🎬 Setting up default scene (shared with Client)...");
    }
    
    // =========================================================================
    // CAMERA - Editor camera (dark background like egui era)
    // Always spawn the camera regardless of scene file
    // =========================================================================
    
    // Editor camera — built from the shared `studio_camera_bundle` so it is
    // identical to the AI camera (which uses the same method). Sky, atmosphere
    // and environment map are attached by `SkyAtmospherePlugin`.
    commands.spawn(studio_camera_bundle("Camera", editor_camera_start()));
    
    // =========================================================================
    // SPAWN DEFAULT SCENE - Only if NOT loading a scene file
    // =========================================================================
    
    // NOTE: Instance loading is handled by SpaceFileLoaderPlugin (file_loader.rs)
    // which properly creates folder hierarchy with parent-child relationships.
    // Do NOT load instances here to avoid duplicates.
    if !loading_scene_file {
        println!("✅ Default scene ready — instances loaded by SpaceFileLoaderPlugin");
    } else {
        println!("⏭️ Skipping default scene content (loading scene file)");
    }
    
    // =========================================================================
    // LIGHTING ENTITIES — spawned dynamically by the file loader from each
    // Space's Lighting/*.instance.toml files (Sun, Moon, Sky, Atmosphere).
    // The engine-side hydrate_lighting_entities system (LightingPlugin)
    // attaches real ECS components (DirectionalLight, SunMarker, etc.)
    // once the file loader creates the bare Instance entities.
    // =========================================================================
    println!("☀️ Lighting entities will be loaded from Space's Lighting/ folder");
}

/// The editor camera's spawn pose.
///
/// Only the first frame or two: `EustressCamera` owns `pivot`/`yaw`/`pitch`/
/// `distance` and re-derives this Transform every frame, so writing a pose here
/// does not aim the camera. To aim it, seed the controller instead — see
/// `EUSTRESS_CAMERA_ORBIT` on [`crate::camera_controller::EustressCamera`].
fn editor_camera_start() -> Transform {
    Transform::from_xyz(10.0, 8.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Grid rendering system - 9.80665m tessellation (SI standard gravity)
pub fn draw_grid(mut gizmos: Gizmos) {
    let grid_size = 20;
    let grid_spacing = 9.80665 / 10.0; // 9.80665 / 10 = 0.980665 per cell
    let color_major = Color::srgba(0.3, 0.3, 0.3, 0.8);
    let color_minor = Color::srgba(0.2, 0.2, 0.2, 0.5);
    
    for i in -grid_size..=grid_size {
        let pos = i as f32 * grid_spacing;
        let color = if i % 5 == 0 { color_major } else { color_minor };
        
        // Lines along X axis
        gizmos.line(
            Vec3::new(-grid_size as f32 * grid_spacing, 0.0, pos),
            Vec3::new(grid_size as f32 * grid_spacing, 0.0, pos),
            color,
        );
        
        // Lines along Z axis
        gizmos.line(
            Vec3::new(pos, 0.0, -grid_size as f32 * grid_spacing),
            Vec3::new(pos, 0.0, grid_size as f32 * grid_spacing),
            color,
        );
    }
    
    // Draw origin axes
    gizmos.line(Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0), Color::srgb(1.0, 0.0, 0.0)); // X - Red
    gizmos.line(Vec3::ZERO, Vec3::new(0.0, 3.0, 0.0), Color::srgb(0.0, 1.0, 0.0)); // Y - Green
    gizmos.line(Vec3::ZERO, Vec3::new(0.0, 0.0, 3.0), Color::srgb(0.0, 0.0, 1.0)); // Z - Blue
}

// Skybox is now created by SharedLightingPlugin from eustress_common
// Instance loading is handled by SpaceFileLoaderPlugin (file_loader.rs)
