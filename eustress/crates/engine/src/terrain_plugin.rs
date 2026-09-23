//! # Terrain Plugin for Engine Studio
//!
//! Engine-side terrain editing with brush tools, heightmap import/export,
//! and integration with the Explorer/Properties panels.
//!
//! Note: UI is now handled by Slint - see ui/slint/terrain_editor.slint

use bevy::prelude::*;
use bevy::ecs::schedule::common_conditions::resource_equals;
use eustress_common::terrain::{
    TerrainConfig, TerrainData, TerrainMode, TerrainBrush, BrushMode,
    spawn_terrain, TerrainRoot, Chunk,
    AdvancedBrushState,
    TerrainPaintGate,
    TerrainDirtyChunks,
    TerrainEditRecorder,
    TerrainVolume,
    CsgShape,
};
use bevy::window::PrimaryWindow;
use eustress_common::classes::Terrain;
use std::path::PathBuf;

// ============================================================================
// Plugin
// ============================================================================

/// Engine terrain plugin - adds editor UI and tools
pub struct EngineTerrainPlugin;

impl Plugin for EngineTerrainPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<TerrainMode>()
            .init_resource::<TerrainBrush>()
            .init_resource::<eustress_common::terrain::TerrainGenerationQueue>()
            .register_type::<TerrainConfig>()
            .register_type::<TerrainData>()
            .register_type::<Chunk>()
            .init_resource::<eustress_common::terrain::LodUpdateState>()
            .init_resource::<eustress_common::terrain::ChunkSpawnThrottle>()
            .init_resource::<TerrainDirtyChunks>()
            .add_systems(Update, (
                eustress_common::terrain::process_terrain_generation_queue,
                eustress_common::terrain::update_lod_system,
                eustress_common::terrain::chunk_spawn_system,
                eustress_common::terrain::chunk_cull_system,
            ).chain())
            // Rebuilds meshes and colliders for chunks any terrain writer
            // marked (brush, Part to Terrain, undo, layer bakes). Not gated
            // on Editor mode: undo and layer edits happen outside it. The same
            // registration lives in the shared `TerrainPlugin` for the
            // Client; this engine never adds that plugin, so there is no
            // double registration.
            .add_systems(Update, eustress_common::terrain::apply_terrain_dirty_chunks
                .after(eustress_common::terrain::chunk_cull_system)
                .after(eustress_common::terrain::terrain_paint_system))
            .init_resource::<TerrainEditorState>()
            .init_resource::<TerrainSelection>()
            // Brush strokes record the raster tiles they touch here;
            // `commit_terrain_stroke` pushes each finished stroke onto the
            // unified `UndoStack`, so Ctrl+Z undoes terrain like any other
            // edit.
            .init_resource::<TerrainEditRecorder>()
            .init_resource::<AdvancedBrushState>()
            .init_resource::<BrushPreviewState>()
            // The brush veto + the brush itself. These used to be left to
            // `common::terrain::TerrainPlugin`, which the Studio engine has
            // never added (only the Client does) — so every Terrain-ribbon
            // brush set a mode that nothing consumed and the ground never
            // moved. Registered here, next to the systems that drive them.
            .init_resource::<TerrainPaintGate>()
            .add_systems(Update, (
                sync_terrain_class_to_system,
                handle_editor_shortcuts,
                update_selection_gizmos,
                // Chained so the veto is fresh for BOTH consumers this frame:
                // an unordered tuple would leave the preview circle drawing
                // (and the brush deciding) off last frame's cursor position.
                (
                    sync_terrain_paint_gate,
                    (
                        update_brush_preview,
                        eustress_common::terrain::terrain_paint_system,
                    ),
                )
                    .chain(),
            ).run_if(resource_equals(TerrainMode::Editor)))
            // Ungated: leaving the terrain editor mid-stroke has to close the
            // stroke too, and the brush above no longer runs then.
            .add_systems(Update, commit_terrain_stroke
                .after(eustress_common::terrain::terrain_paint_system));

        // Material slot table + texture arrays, and the textured terrain
        // material that draws them: the other half of the shared plugin this
        // engine does not add. `terrain_disk_load::register` below points the
        // table at the open Space's Workspace/Terrain.
        if !app.is_plugin_added::<eustress_common::terrain::TerrainMaterialSlotsPlugin>() {
            app.add_plugins(eustress_common::terrain::TerrainMaterialSlotsPlugin);
        }
        if !app.is_plugin_added::<eustress_common::terrain::TerrainSurfacePlugin>() {
            app.add_plugins(eustress_common::terrain::TerrainSurfacePlugin);
        }

        // Terrain layer instances (splines, stamps, flatten pads, noise,
        // material fills) baked over the base, the third part of the shared
        // plugin; and, Studio-side, keeping each spline point under the
        // spline whose folder holds it, and drawing a selected spline (see
        // `terrain_layers`). The drawing runs after the dirty-chunk pass so
        // it follows the corridor that pass just baked. Ungated, like the
        // pass: layers are selected and edited outside the terrain editor.
        if !app.is_plugin_added::<eustress_common::terrain::TerrainLayersPlugin>() {
            app.add_plugins(eustress_common::terrain::TerrainLayersPlugin);
        }
        app.add_systems(Update, crate::terrain_layers::adopt_points_by_folder);
        app.add_systems(
            Update,
            crate::terrain_layers::draw_spline_gizmos.after(eustress_common::terrain::apply_terrain_dirty_chunks),
        );
        // Scatter layers (grass, shrubs, rocks, trees) placed over the
        // finished ground and streamed around the view: the fourth part of
        // the shared plugin, same guard. Ungated, like the layers.
        if !app.is_plugin_added::<eustress_common::terrain::TerrainScatterPlugin>() {
            app.add_plugins(eustress_common::terrain::TerrainScatterPlugin);
        }
        // Water, the fifth part, same guard: the ocean plane the Terrain
        // ribbon's Water button toggles (`WaterConfig`), the lakes of
        // `TerrainWaterBody` instances and the water of River splines, all
        // on the shared water material. Ungated, like the layers.
        if !app.is_plugin_added::<eustress_common::terrain::TerrainWaterPlugin>() {
            app.add_plugins(eustress_common::terrain::TerrainWaterPlugin);
        }

        // Disk-terrain auto-loader — on Space open, when
        // `Workspace/Terrain/_terrain.toml` exists (worldgen export or
        // heightmap import), hydrate + spawn it once per Space. UNGATED:
        // disk terrain is a default engine capability; migrated Spaces
        // stand down at runtime (the voxel loader below owns those).
        crate::terrain_disk_load::register(app);

        // Wave 9.C — imported-terrain voxel loader (migrated Spaces read
        // Fjall voxels → runtime heightfield, gated on `space_is_migrated`).
        // Feature-gated: only present when the Fjall WorldDb is compiled in.
        // It spawns a `TerrainRoot` the `chunk_spawn_system` above already
        // meshes, so no extra render wiring is needed.
        #[cfg(feature = "world-db")]
        crate::terrain_voxel_load::register(app);
    }
}

// ============================================================================
// Resources
// ============================================================================

/// Editor state for terrain tools
#[derive(Resource)]
#[allow(dead_code)]
pub struct TerrainEditorState {
    pub pending_import: Option<PathBuf>,
    pub last_export_path: Option<PathBuf>,
    pub show_advanced: bool,
    pub show_advanced_brushes: bool,
    pub is_editing: bool,
    pub last_mesh_regen: std::time::Instant,
    pub pending_regen_chunks: Vec<Entity>,
    pub mesh_regen_interval: f32,
    pub last_brush_apply: std::time::Instant,
    pub brush_apply_interval: f32,
}

impl Default for TerrainEditorState {
    fn default() -> Self {
        Self {
            pending_import: None,
            last_export_path: None,
            show_advanced: false,
            show_advanced_brushes: false,
            is_editing: false,
            last_mesh_regen: std::time::Instant::now(),
            pending_regen_chunks: Vec::new(),
            mesh_regen_interval: 0.1,
            last_brush_apply: std::time::Instant::now(),
            brush_apply_interval: 0.016,
        }
    }
}

/// Selection state for Explorer integration
#[derive(Resource, Default)]
pub struct TerrainSelection {
    pub selected_chunk: Option<Entity>,
    pub hovered_chunk: Option<Entity>,
}

/// Brush preview state — tracks where the brush circle should render
#[derive(Resource, Default)]
pub struct BrushPreviewState {
    /// World-space position of the brush center (terrain hit point)
    pub position: Option<Vec3>,
    /// Whether the brush is actively painting (LMB held)
    pub is_painting: bool,
}

// ============================================================================
// Systems
// ============================================================================

/// Sync Terrain class component to terrain system
///
/// Do-not-fight guard: when the live Space has an on-disk terrain
/// (`Workspace/Terrain/_terrain.toml` — worldgen export or heightmap
/// import), an `Added<Terrain>` class instance re-spawns the DISK terrain
/// instead of clobbering it with procedural noise. Migrated Spaces are the
/// voxel loader's domain, so they keep the procedural fallback here.
fn sync_terrain_class_to_system(
    mut commands: Commands,
    query: Query<(Entity, &Terrain), Added<Terrain>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    for (_entity, terrain_class) in query.iter() {
        for existing in existing_terrain.iter() {
            commands.entity(existing).despawn();
        }

        // Prefer the Space's on-disk terrain over procedural regeneration.
        let disk = space_root.as_ref().and_then(|sr| {
            if crate::space::space_ops::space_is_migrated(&sr.0) {
                return None;
            }
            let terrain_dir = sr.0.join("Workspace").join("Terrain");
            if !terrain_dir.join("_terrain.toml").exists() {
                return None;
            }
            crate::terrain_disk_load::hydrate_terrain_from_disk(&terrain_dir).ok()
        });

        let from_disk = disk.is_some();
        let terrain_entity = match disk {
            Some(terrain) => terrain.spawn(&mut commands, &mut meshes, &mut materials),
            None => spawn_terrain(
                &mut commands,
                &mut meshes,
                &mut materials,
                terrain_class.to_config(),
                TerrainData::procedural(),
            ),
        };
        if from_disk {
            commands
                .entity(terrain_entity)
                .insert(crate::terrain_disk_load::DiskSourcedTerrain);
            info!("🏔️ Engine terrain spawned from Terrain class (hydrated from Workspace/Terrain)");
        } else {
            info!("🏔️ Engine terrain spawned from Terrain class");
        }
    }
}

/// Handle keyboard shortcuts for terrain editing
fn handle_editor_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<TerrainMode>,
    mut brush: ResMut<TerrainBrush>,
    terrain_query: Query<Entity, With<TerrainRoot>>,
) {
    if terrain_query.is_empty() {
        return;
    }

    // Brush shortcuts only apply while the terrain editor is active. Without
    // this guard, 1-5 would mutate brush state (and fight shortcuts like
    // camera-ortho on Digit5) even though the user never opened the editor.
    let editor_active = matches!(*mode, TerrainMode::Editor);
    if editor_active {
        if keys.just_pressed(KeyCode::Digit1) {
            brush.mode = BrushMode::Raise;
            info!("🖌️ Brush: Raise");
        }
        if keys.just_pressed(KeyCode::Digit2) {
            brush.mode = BrushMode::Lower;
            info!("🖌️ Brush: Lower");
        }
        if keys.just_pressed(KeyCode::Digit3) {
            brush.mode = BrushMode::Smooth;
            info!("🖌️ Brush: Smooth");
        }
        if keys.just_pressed(KeyCode::Digit4) {
            brush.mode = BrushMode::Flatten;
            info!("🖌️ Brush: Flatten");
        }
        if keys.just_pressed(KeyCode::Digit5) {
            brush.mode = BrushMode::PaintTexture;
            info!("🖌️ Brush: Paint Texture");
        }
    }
    
    if keys.just_pressed(KeyCode::KeyT) {
        *mode = match *mode {
            TerrainMode::Render => {
                info!("🎨 Terrain Editor: ENABLED");
                TerrainMode::Editor
            }
            TerrainMode::Editor => {
                info!("🎨 Terrain Editor: DISABLED");
                TerrainMode::Render
            }
        };
    }
    
    if keys.just_pressed(KeyCode::BracketLeft) {
        brush.radius = (brush.radius - 2.0).max(1.0);
        info!("🖌️ Brush size: {:.1}", brush.radius);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        brush.radius = (brush.radius + 2.0).min(50.0);
        info!("🖌️ Brush size: {:.1}", brush.radius);
    }
}

/// Update selection gizmos for terrain chunks
fn update_selection_gizmos(
    selection: Res<TerrainSelection>,
    mut gizmos: Gizmos,
    chunk_query: Query<(&Chunk, &GlobalTransform)>,
    config_query: Query<&TerrainConfig, With<TerrainRoot>>,
) {
    let Ok(config) = config_query.single() else { return };
    
    if let Some(selected) = selection.selected_chunk {
        if let Ok((_chunk, transform)) = chunk_query.get(selected) {
            let pos = transform.translation();
            let size = config.chunk_size;
            gizmos.cube(
                Transform::from_translation(pos + Vec3::Y * 0.5)
                    .with_scale(Vec3::new(size, 1.0, size)),
                bevy::color::Color::srgba(0.0, 1.0, 0.0, 0.5),
            );
        }
    }
}

/// Feed the shared [`TerrainPaintGate`] from the Studio's editor chrome.
///
/// The brush reads the raw cursor, so without this a drag that starts on a
/// ribbon button or a docked panel carves the ground underneath it. Same
/// two conditions every other engine tool checks (see `decal_place_tool`):
/// the pointer must be inside the viewport rectangle, and no Slint panel or
/// text field may own it.
fn sync_terrain_paint_gate(
    mut gate: ResMut<TerrainPaintGate>,
    windows: Query<&Window, With<PrimaryWindow>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    let over_chrome = ui_focus
        .as_deref()
        .map(|f| f.has_focus || f.text_input_focused)
        .unwrap_or(false);

    let in_viewport = windows
        .single()
        .ok()
        .and_then(|window| {
            let cursor = window.cursor_position()?;
            Some(match viewport_bounds.as_deref() {
                Some(bounds) => bounds.contains_logical(cursor, window.scale_factor() as f32),
                None => true,
            })
        })
        .unwrap_or(false);

    let allowed = !over_chrome && in_viewport;
    if gate.allowed != allowed {
        gate.allowed = allowed;
    }
}

/// Draw the brush preview gizmo at the cursor's terrain hit: a circle on the
/// surface for the heightfield brushes, or for the 3D brushes the sphere,
/// box or cylinder the next dab will add, carve or smooth.
fn update_brush_preview(
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<
        (&TerrainConfig, &TerrainData, Option<&TerrainVolume>, Option<&eustress_common::terrain::TerrainBaked>),
        With<TerrainRoot>,
    >,
    brush: Res<TerrainBrush>,
    buttons: Res<ButtonInput<MouseButton>>,
    gate: Res<TerrainPaintGate>,
    mut preview: ResMut<BrushPreviewState>,
    mut gizmos: Gizmos,
) {
    let Ok(window) = windows.single() else { return };
    // `order == 0` (the engine-wide "camera the user looks through"
    // convention), NOT `single()`: the Studio runs the scene camera, the
    // Slint chrome overlay and the AI camera at once, so `single()` always
    // errored here and the preview circle never drew.
    let Some((camera, camera_transform)) = camera_query.iter().find(|(c, _)| c.order == 0) else {
        return;
    };
    let Ok((config, data, volume, baked)) = terrain_query.single() else { return };
    // The ground the user sees, layer bake included.
    let data = eustress_common::terrain::surface_data(data, baked);

    // Nothing to preview while the pointer is over editor chrome — and the
    // brush would not paint there either.
    if !gate.allowed {
        preview.position = None;
        preview.is_painting = false;
        return;
    }

    let Some(cursor_pos) = window.cursor_position() else {
        preview.position = None;
        preview.is_painting = false;
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        preview.position = None;
        return;
    };

    // Raymarch the REAL terrain, the same call `terrain_paint_system` uses to
    // pick its hit point (the whole field, caves included, once the terrain
    // has volumetric edits). The old flat Y=0 plane test put the circle
    // somewhere the brush was not going to act on any sculpted ground.
    let Some(hit) = eustress_common::terrain::height_query::raycast_terrain_surface(
        config, data, volume, ray, 2000.0, 2.0,
    ) else {
        preview.position = None;
        return;
    };

    preview.position = Some(hit);
    preview.is_painting = buttons.pressed(MouseButton::Left);

    // Draw brush circle on terrain surface
    let radius = brush.radius;
    let color = if preview.is_painting {
        // Active painting: bright mode-specific color
        match brush.mode {
            BrushMode::Raise => bevy::color::Color::srgba(0.2, 1.0, 0.2, 0.9),
            BrushMode::Lower => bevy::color::Color::srgba(1.0, 0.2, 0.2, 0.9),
            BrushMode::Smooth => bevy::color::Color::srgba(0.2, 0.6, 1.0, 0.9),
            BrushMode::Flatten => bevy::color::Color::srgba(1.0, 1.0, 0.2, 0.9),
            BrushMode::PaintTexture => bevy::color::Color::srgba(1.0, 0.5, 0.0, 0.9),
            BrushMode::VoxelAdd => bevy::color::Color::srgba(0.3, 1.0, 0.6, 0.9),
            BrushMode::VoxelRemove => bevy::color::Color::srgba(1.0, 0.35, 0.35, 0.9),
            BrushMode::VoxelSmooth => bevy::color::Color::srgba(0.4, 0.8, 1.0, 0.9),
            _ => bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.9),
        }
    } else {
        // Hovering: semi-transparent white
        bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.5)
    };

    // A 3D brush acts on the volume around the hit, not on a patch of ground
    // under it, so it previews the solid it will change instead of a circle.
    // The shape comes from the same `voxel_shape` the paint system dabs with.
    if brush.mode.is_volumetric() {
        draw_voxel_brush_preview(&mut gizmos, brush.voxel_shape(hit), color);
        return;
    }

    // Outer brush circle
    gizmos.circle(
        Isometry3d::new(
            hit + Vec3::Y * 0.05, // Slight Y offset to avoid z-fighting
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ),
        radius,
        color,
    );

    // Inner falloff circle (shows where full-strength brush starts fading)
    if brush.falloff > 0.01 {
        let inner_radius = radius * (1.0 - brush.falloff);
        if inner_radius > 0.1 {
            let inner_color = bevy::color::Color::srgba(
                color.to_srgba().red,
                color.to_srgba().green,
                color.to_srgba().blue,
                0.25,
            );
            gizmos.circle(
                Isometry3d::new(
                    hit + Vec3::Y * 0.05,
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                inner_radius,
                inner_color,
            );
        }
    }

    // Crosshair at center
    let cross_size = radius * 0.1;
    let cross_color = bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.3);
    gizmos.line(
        hit + Vec3::new(-cross_size, 0.05, 0.0),
        hit + Vec3::new(cross_size, 0.05, 0.0),
        cross_color,
    );
    gizmos.line(
        hit + Vec3::new(0.0, 0.05, -cross_size),
        hit + Vec3::new(0.0, 0.05, cross_size),
        cross_color,
    );
}

/// Wireframe of the region one 3D brush dab covers, centred on the terrain
/// hit, with a small cross at the centre so the dab point reads even when the
/// shape is much larger than the view.
fn draw_voxel_brush_preview(gizmos: &mut Gizmos, shape: CsgShape, color: bevy::color::Color) {
    let center = match shape {
        CsgShape::Sphere { center, radius } => {
            gizmos.sphere(Isometry3d::new(center, Quat::IDENTITY), radius, color);
            center
        }
        CsgShape::AxisBox { center, half_extents } => {
            gizmos.cube(Transform::from_translation(center).with_scale(half_extents * 2.0), color);
            center
        }
        CsgShape::Cylinder { center, radius, half_height } => {
            gizmos.primitive_3d(
                &bevy::math::primitives::Cylinder { radius, half_height },
                Isometry3d::new(center, Quat::IDENTITY),
                color,
            );
            center
        }
    };
    let (lo, hi) = shape.bounds();
    let cross = ((hi - lo).max_element() * 0.05).max(0.05);
    let cross_color = bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.4);
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        gizmos.line(center - axis * cross, center + axis * cross, cross_color);
    }
}

/// Close the brush stroke [`TerrainEditRecorder`] holds and push it onto the
/// unified undo stack as one entry, once the left button is up or the
/// terrain editor was left mid-stroke. The recorder drops tiles and volume
/// bricks the stroke did not change, and a stroke that changed nothing
/// pushes nothing.
fn commit_terrain_stroke(
    buttons: Res<ButtonInput<MouseButton>>,
    mode: Res<TerrainMode>,
    mut recorder: ResMut<TerrainEditRecorder>,
    terrain_query: Query<(Entity, &TerrainData, Option<&TerrainVolume>), With<TerrainRoot>>,
    undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    // Read through `Deref` first so idle frames leave the resource unchanged.
    if !recorder.is_recording() {
        return;
    }
    if buttons.pressed(MouseButton::Left) && *mode == TerrainMode::Editor {
        return;
    }
    let Ok((root, data, volume)) = terrain_query.single() else {
        // The terrain went away mid-stroke: there is nothing to undo into.
        recorder.cancel();
        return;
    };
    let volume = volume.unwrap_or(TerrainVolume::empty());
    let Some(edit) = recorder.finish_with_volume(Some(root), data, volume) else {
        return;
    };
    if let Some(mut undo) = undo {
        undo.push_labeled(
            edit.label.clone(),
            crate::undo::Action::TerrainEdit {
                label: edit.label,
                root: root.to_bits(),
                tiles: edit.tiles,
                bricks: edit.bricks,
            },
        );
    }
}
