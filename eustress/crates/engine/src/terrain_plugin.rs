//! # Terrain Plugin for Engine Studio
//!
//! Engine-side terrain: the streaming chain, materials, layers, scatter and
//! water from the shared terrain plugins, the disk and voxel loaders, and the
//! terrain tools (see `docs/design/TERRAIN_TOOLS_UX.md`).
//!
//! The terrain tools are a Studio tool like Move: `StudioState::current_tool`
//! is `Tool::Terrain` while they are on, and [`sync_terrain_mode_to_tool`]
//! keeps the shared `TerrainMode` in step, so the brush systems run exactly
//! then. The brush itself (hover, stroke, dabs) lives in
//! `eustress_common::terrain::editor`; this plugin feeds it the Studio's
//! chrome veto, answers the terrain hotkeys ([`handle_terrain_actions`]), the
//! `B` size gesture ([`terrain_brush_gesture`]) and `Esc`, and pushes each
//! finished stroke onto the undo stack. The cursor is drawn by
//! `terrain_cursor`.

use bevy::ecs::schedule::common_conditions::resource_equals;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use eustress_common::classes::Terrain;
use eustress_common::terrain::{
    spawn_terrain, surface_data, Chunk, TerrainBaked, TerrainBrush, TerrainBrushHover, TerrainConfig,
    TerrainData, TerrainDirtyChunks, TerrainEditRecorder, TerrainMode, TerrainPaintGate, TerrainRoot,
    TerrainStroke, TerrainTool, TerrainVolume, PaintMode,
};

use crate::keybindings::Action;
use crate::ui::{SetTerrainBrushEvent, ToggleTerrainEditEvent};

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
            .init_resource::<TerrainBrushHover>()
            .init_resource::<TerrainStroke>()
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
            // Brush strokes record the raster tiles they touch here;
            // `commit_terrain_stroke` pushes each finished stroke onto the
            // unified `UndoStack`, so Ctrl+Z undoes terrain like any other
            // edit.
            .init_resource::<TerrainEditRecorder>()
            // The chrome veto the hover and the stroke read.
            .init_resource::<TerrainPaintGate>()
            // The messages the terrain hotkeys read and write. The UI's
            // `SpawnEventsPlugin` and the keybindings register them too;
            // registering is idempotent, and a system whose message is not
            // registered fails validation and never runs.
            .add_message::<crate::ui::MenuActionEvent>()
            .add_message::<SetTerrainBrushEvent>()
            .add_message::<ToggleTerrainEditEvent>()
            // Ungated: a Terrain instance added outside the terrain editor
            // must be seen then, not on the editor's first frame, where its
            // `Added` filter would take every Terrain the Space loaded with
            // for a new one. After the streaming chain, which the voxel
            // loader runs ahead of, so a root it spawned this frame is
            // already there and is never doubled.
            .add_systems(
                Update,
                sync_terrain_class_to_system.after(eustress_common::terrain::process_terrain_generation_queue),
            )
            // Ungated: the tool follows `current_tool` in and out, and the
            // hotkeys include `T`, which enters the tools from anywhere.
            .add_systems(Update, (
                sync_terrain_mode_to_tool,
                handle_terrain_actions,
                leave_terrain_tools_on_escape,
            ).chain().before(sync_terrain_paint_gate))
            .add_systems(Update, (
                terrain_brush_gesture,
                // Chained so the veto and the hover are fresh for the stroke
                // this frame: the brush dabs where the cursor shows it.
                (
                    sync_terrain_paint_gate,
                    eustress_common::terrain::update_brush_hover,
                    eustress_common::terrain::terrain_paint_system,
                )
                    .chain(),
            ).run_if(resource_equals(TerrainMode::Editor)))
            // Ungated: leaving the terrain tools mid-stroke has to close the
            // stroke too, and the brush above no longer runs then.
            .add_systems(Update, commit_terrain_stroke
                .after(eustress_common::terrain::terrain_paint_system))
            // Save writes only what changed since the disk last matched
            // memory; a terrain read from disk matches it as it arrives.
            .init_resource::<crate::ui::file_event_handler::TerrainSaveBaseline>()
            .add_systems(Update, crate::ui::file_event_handler::baseline_disk_terrain);

        // The brush cursor, grid, contours, plane and readout, the tool
        // bar, ribbon and readout surfaces in Slint, and the Sea Level tool.
        app.add_plugins((
            crate::terrain_cursor::TerrainCursorPlugin,
            crate::terrain_tools_ui::TerrainToolsUiPlugin,
            crate::terrain_sea_level::TerrainSeaLevelPlugin,
            crate::terrain_region::TerrainRegionPlugin,
        ));

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
// Systems
// ============================================================================

/// Give a Terrain class instance added to a loaded Space some ground.
///
/// A Terrain instance that arrives with its Space (while the Space loads, or
/// before the disk and voxel terrain loaders have decided for it) is the
/// Space's own: those loaders give it its ground, so it changes nothing here.
/// One added later, to a Space without terrain, spawns the Space's on-disk
/// terrain when `Workspace/Terrain/_terrain.toml` exists, else procedural
/// ground from the instance's settings. Existing ground is never replaced,
/// and a converted Space is left alone while the voxel loader is still
/// building its imported terrain. The system runs every frame, in and out of
/// the terrain editor, so its `Added` filter only ever sees instances added
/// since it last ran.
#[allow(clippy::too_many_arguments)]
fn sync_terrain_class_to_system(
    mut commands: Commands,
    query: Query<&Terrain, Added<Terrain>>,
    existing_terrain: Query<(), With<TerrainRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    load_in_progress: Option<Res<crate::space::file_loader::LoadInProgress>>,
    disk_latch: Option<Res<crate::terrain_disk_load::TerrainDiskLoadLatch>>,
    #[cfg(feature = "world-db")] voxel_latch: Option<Res<crate::terrain_voxel_load::VoxelTerrainLoadLatch>>,
    #[cfg(feature = "world-db")] voxel_build: Option<Res<crate::terrain_voxel_load::VoxelTerrainBuild>>,
) {
    let Some(terrain_class) = query.iter().next() else {
        return;
    };
    let Some(space_root) = space_root else {
        return;
    };
    let loading = load_in_progress.as_deref().is_some_and(|load| load.active);
    let disk_decided = disk_latch
        .as_deref()
        .is_some_and(|latch| latch.0.as_deref() == Some(space_root.0.as_path()));
    if loading || !disk_decided || !existing_terrain.is_empty() {
        return;
    }
    let migrated = crate::space::space_ops::space_is_migrated(&space_root.0);
    #[cfg(feature = "world-db")]
    {
        let voxel_decided = voxel_latch
            .as_deref()
            .is_some_and(|latch| latch.0.as_deref() == Some(space_root.0.as_path()));
        let building = voxel_build
            .as_deref()
            .is_some_and(crate::terrain_voxel_load::VoxelTerrainBuild::is_running);
        // The Spaces whose imported terrain the voxel loader builds: a
        // converted one, or one holding the importer's chunk files and no
        // disk terrain (a re-import), as the Player decides.
        let terrain_dir = space_root.0.join("Workspace").join("Terrain");
        let voxel_terrain = migrated
            || (!terrain_dir.join("_terrain.toml").exists()
                && eustress_common::terrain::voxel_import::has_voxel_chunk_files(&terrain_dir));
        if voxel_terrain && (!voxel_decided || building) {
            return;
        }
    }

    // Prefer the Space's on-disk terrain over procedural ground. A converted
    // Space's disk terrain is the voxel loader's to read.
    let terrain_dir = space_root.0.join("Workspace").join("Terrain");
    let disk = (!migrated && terrain_dir.join("_terrain.toml").exists())
        .then(|| crate::terrain_disk_load::hydrate_terrain_from_disk(&terrain_dir).ok())
        .flatten();

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

/// Keep the shared `TerrainMode` in step with the Studio's current tool: the
/// brush systems run exactly while `Tool::Terrain` is current. The tools are
/// left for Select when a Play session starts (the mouse belongs to the game)
/// or the terrain goes away (nothing to sculpt).
fn sync_terrain_mode_to_tool(
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    terrain: Query<(), With<TerrainRoot>>,
    mut mode: ResMut<TerrainMode>,
) {
    let Some(mut studio_state) = studio_state else { return };
    let editing = crate::play_mode::editor_input_enabled(play_state);
    if studio_state.current_tool == crate::ui::Tool::Terrain && (!editing || terrain.is_empty()) {
        studio_state.current_tool = crate::ui::Tool::Select;
    }
    let want =
        if studio_state.current_tool == crate::ui::Tool::Terrain { TerrainMode::Editor } else { TerrainMode::Render };
    if *mode != want {
        *mode = want;
    }
}

/// Grid steps the locked plane moves per `PageUp`/`PageDown`, and per the
/// `Shift` versions.
const PLANE_STEPS_FAST: f32 = 10.0;

/// Answer the terrain hotkeys (design section 6), which the keybinding table
/// sends as menu actions: `T` enters or leaves the tools from anywhere; the
/// rest arrive only while the tools are current (their context). Tool
/// choices go through [`SetTerrainBrushEvent`], so a key and a button take
/// the same path.
#[allow(clippy::too_many_arguments)]
fn handle_terrain_actions(
    mut menu: MessageReader<crate::ui::MenuActionEvent>,
    mut brush: ResMut<TerrainBrush>,
    hover: Res<TerrainBrushHover>,
    terrain: Query<(&TerrainConfig, &TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>,
    mut tool_events: MessageWriter<SetTerrainBrushEvent>,
    mut toggle_events: MessageWriter<ToggleTerrainEditEvent>,
    mut last_plane: Local<Option<f32>>,
    (mut sea_level, mut region): (
        Option<ResMut<crate::terrain_sea_level::SeaLevelTool>>,
        Option<ResMut<crate::terrain_region::RegionTool>>,
    ),
) {
    for event in menu.read() {
        let tool = match event.action {
            Action::TerrainDraw => Some(TerrainTool::Draw),
            Action::TerrainSculpt => Some(TerrainTool::Sculpt),
            Action::TerrainSmooth => Some(TerrainTool::Smooth),
            Action::TerrainFlatten => Some(TerrainTool::Flatten),
            Action::TerrainPaint => Some(TerrainTool::Paint),
            Action::TerrainSeaLevel => Some(TerrainTool::SeaLevel),
            Action::TerrainRegion => Some(TerrainTool::Region),
            _ => None,
        };
        if let Some(tool) = tool {
            tool_events.write(SetTerrainBrushEvent { tool, mode: None });
            continue;
        }
        let surface_y = hover.surface.map(|p| p.y);
        match event.action {
            Action::TerrainTools => {
                toggle_events.write(ToggleTerrainEditEvent);
            }
            Action::TerrainSizeDown => brush.step_size(false),
            Action::TerrainSizeUp => brush.step_size(true),
            Action::TerrainStrengthDown => brush.step_strength(false),
            Action::TerrainStrengthUp => brush.step_strength(true),
            Action::TerrainPivotPrev => brush.pivot = brush.pivot.prev(),
            Action::TerrainPivotNext => brush.pivot = brush.pivot.next(),
            Action::TerrainPlaneLock => {
                if let Some(y) = brush.plane_lock.take() {
                    *last_plane = Some(y);
                } else {
                    // At the ground under the cursor, never at the world
                    // origin: off the terrain, the last plane, else the
                    // height the brush last stood at.
                    let y = surface_y.or(*last_plane).or(hover.target.map(|p| p.y)).unwrap_or(0.0);
                    brush.plane_lock = Some(y);
                }
            }
            Action::TerrainPlanePick => {
                if let Some(y) = surface_y {
                    brush.plane_lock = Some(y);
                }
            }
            Action::TerrainPlaneUp
            | Action::TerrainPlaneDown
            | Action::TerrainPlaneUpFast
            | Action::TerrainPlaneDownFast => {
                let steps = match event.action {
                    Action::TerrainPlaneUp => 1.0,
                    Action::TerrainPlaneDown => -1.0,
                    Action::TerrainPlaneUpFast => PLANE_STEPS_FAST,
                    _ => -PLANE_STEPS_FAST,
                };
                let step = brush.snap_step.max(0.01);
                // Sea Level moves its water level, Region a Transform's
                // target; the brushes, their plane.
                if brush.tool == TerrainTool::SeaLevel {
                    if let Some(sea_level) = sea_level.as_deref_mut() {
                        sea_level.nudge_level(steps * step);
                    }
                } else if brush.tool == TerrainTool::Region {
                    if let Some(region) = region.as_deref_mut() {
                        region.nudge_target(steps * step);
                    }
                } else if let Some(y) = brush.plane_lock {
                    brush.plane_lock = Some(y + steps * step);
                }
            }
            Action::TerrainSnap => brush.snap = !brush.snap,
            Action::TerrainSnapStep => brush.next_snap_step(),
            Action::TerrainContours => brush.contours = !brush.contours,
            Action::TerrainMirror | Action::TerrainMirrorAxis => {
                let was_off = brush.mirror == eustress_common::terrain::MirrorAxes::Off;
                if event.action == Action::TerrainMirror {
                    brush.toggle_mirror();
                } else {
                    brush.next_mirror_axes();
                }
                // Turning mirror on puts its planes through the point under
                // the cursor.
                if was_off {
                    if let Some(at) = hover.surface.or(hover.target) {
                        brush.mirror_origin = at.xz();
                    }
                }
            }
            Action::TerrainSampleMaterial => {
                let (Some(hit), Ok((config, data, baked))) = (hover.surface, terrain.single()) else { continue };
                let Some(sample) =
                    eustress_common::terrain::material_at_world(config, surface_data(data, baked), hit.x, hit.z)
                else {
                    continue;
                };
                if brush.tool == TerrainTool::Paint && brush.paint_mode == PaintMode::Replace {
                    brush.source_material = sample.primary;
                } else {
                    brush.paint_material = sample.primary;
                }
            }
            _ => {}
        }
    }
}

/// `Esc` leaves the terrain tools for Select (design section 4.1), unless a
/// text field has the keyboard. With Sea Level's rectangle drawn, it clears
/// the rectangle first; with Region's box, it cancels a paste or a transform,
/// then drops the box.
fn leave_terrain_tools_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    brush: Res<TerrainBrush>,
    sea_level: Option<ResMut<crate::terrain_sea_level::SeaLevelTool>>,
    region: Option<ResMut<crate::terrain_region::RegionTool>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if ui_focus.as_deref().is_some_and(|focus| focus.text_input_focused)
        || crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    let Some(mut studio_state) = studio_state else { return };
    if studio_state.current_tool != crate::ui::Tool::Terrain {
        return;
    }
    if let Some(mut sea_level) = sea_level {
        if brush.tool == TerrainTool::SeaLevel && (sea_level.rect.is_some() || sea_level.dragging()) {
            sea_level.clear();
            return;
        }
    }
    if let Some(mut region) = region {
        if brush.tool == TerrainTool::Region && region.has_open_state() {
            region.escape();
            return;
        }
    }
    studio_state.current_tool = crate::ui::Tool::Select;
}

/// Size change per wheel notch with `B` held (a factor).
const GESTURE_SIZE_PER_NOTCH: f32 = 1.1;
/// Size change per pixel of horizontal drag with `B` and the left button held.
const GESTURE_SIZE_PER_PIXEL: f32 = 1.005;
/// Strength change per wheel notch with `Shift+B` held.
const GESTURE_STRENGTH_PER_NOTCH: f32 = 0.02;
/// Strength change per pixel of drag with `Shift+B` held.
const GESTURE_STRENGTH_PER_PIXEL: f32 = 0.002;

/// Roblox's brush gestures (design section 6): holding `B`, the wheel or a
/// left-button drag sizes the brush; with `Ctrl` it sets Draw's height; with
/// `Shift`, the strength. The camera ignores the wheel while `B` is held
/// (`camera_controller`), and the paint gate keeps a `B` drag from stroking.
fn terrain_brush_gesture(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    mut brush: ResMut<TerrainBrush>,
) {
    let notches: f32 = wheel
        .read()
        .map(|event| if event.unit == MouseScrollUnit::Line { event.y } else { event.y / 100.0 })
        .sum();
    let dx: f32 = motion.read().map(|event| event.delta.x).sum();
    if !keys.pressed(KeyCode::KeyB) {
        return;
    }
    let drag = if buttons.pressed(MouseButton::Left) { dx } else { 0.0 };
    if notches == 0.0 && drag == 0.0 {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if shift {
        let strength = brush.strength() + notches * GESTURE_STRENGTH_PER_NOTCH + drag * GESTURE_STRENGTH_PER_PIXEL;
        brush.set_strength(strength);
    } else {
        let factor = GESTURE_SIZE_PER_NOTCH.powf(notches) * GESTURE_SIZE_PER_PIXEL.powf(drag);
        if ctrl {
            let height = brush.draw_height() * factor;
            brush.set_draw_height(height);
        } else {
            let size = brush.size() * factor;
            brush.set_size(size);
        }
    }
}

/// Feed the shared [`TerrainPaintGate`] from the Studio's editor chrome.
///
/// The brush reads the raw cursor, so without this a drag that starts on a
/// ribbon button or a docked panel carves the ground underneath it. Same
/// two conditions every other engine tool checks (see `decal_place_tool`):
/// the pointer must be inside the viewport rectangle, and no Slint panel or
/// text field may own it. `B` held is the size gesture's, not a stroke's.
fn sync_terrain_paint_gate(
    mut gate: ResMut<TerrainPaintGate>,
    keys: Res<ButtonInput<KeyCode>>,
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

    let allowed = !over_chrome && in_viewport && !keys.pressed(KeyCode::KeyB);
    if gate.allowed != allowed {
        gate.allowed = allowed;
    }
}

/// Close the brush stroke [`TerrainEditRecorder`] holds and push it onto the
/// unified undo stack as one entry, once the left button is up or the
/// terrain tools were left mid-stroke. The recorder drops tiles and volume
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
                water: edit.water,
            },
        );
    }
}
