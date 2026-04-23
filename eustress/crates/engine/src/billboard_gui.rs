//! Slint-rendered 3D billboards.
//!
//! Each `BillboardGuiMarker` entity owns a `BillboardCard` Slint component,
//! instantiated through the engine's `SlintBevyPlatform`. The component is
//! software-rendered into a per-billboard RGBA image that backs a
//! `StandardMaterial` mapped onto a unit quad parented to the entity.
//!
//! This replaces the old CPU-atlas + fontdue approach in
//! `eustress-common::gui::billboard_renderer` — that path was pixel-level
//! custom drawing and couldn't share the engine's Slint theming.
//!
//! Pipeline per billboard:
//!
//! 1. `sync_billboard_class_to_marker` — mirrors the `BillboardGui`
//!    class component (what the Properties panel edits) into the runtime
//!    `BillboardGuiMarker`, so panel edits propagate each frame.
//! 2. `spawn_billboard_render_state` — sees a new marker, instantiates
//!    `BillboardCard::new()` (platform creates a fresh `BevyWindowAdapter`),
//!    allocates an RGBA image + unlit transparent material, spawns a quad.
//! 3. `sync_billboard_properties` — on `Changed<BillboardGuiMarker>`,
//!    resizes texture / quad / adapter for size edits, pushes
//!    `always_on_top` onto the material (depth-bias), toggles the quad's
//!    `Visibility` from the `visible` flag.
//! 4. `cull_billboards_by_distance` — each frame, hides quads past
//!    `max_distance` (0 disables the cull).
//! 5. `update_and_render_billboards` — walks `GuiElementDisplay`
//!    children, builds a `VecModel<BillboardLabelData>`, pushes it to
//!    the card, drives the software renderer into the image buffer.
//! 6. `billboard_face_camera` — yaws each quad to face the active camera.
//!    Gated per-entity on the parent marker's `face_camera` flag.
//!
//! Depth occlusion with 3D geometry is inherent — quads are real meshes in
//! the scene graph, not screen-space overlays.
//!
//! Slint component handles (`BillboardCard`) and window adapters are `!Send`
//! because they use `Rc` internally, so per-entity state lives in a NonSend
//! resource (`BillboardSlintStates`) keyed by `Entity` rather than on a
//! Bevy `Component`. The `BillboardRenderHandle` component is a send-safe
//! tag holding image/material/dimension data.

use bevy::prelude::*;
use eustress_common::classes::BillboardGui;
use eustress_common::gui::billboard_renderer::{BillboardGuiMarker, GuiElementDisplay};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

// Pulls in BillboardCard + BillboardLabelData generated from the engine's
// `main.slint` (which imports `billboard_card.slint`).
slint::include_modules!();

// ── Constants ──────────────────────────────────────────────────────────────

/// Pixel → meter conversion for billboard quad scale. The old renderer used
/// 50 px/stud; meters are Bevy's native unit, so we reuse the same ratio:
/// a 200×100 px billboard becomes a 4×2 m quad.
const PIXELS_PER_METER: f32 = 50.0;

/// Depth-bias applied to `always_on_top` materials. Large positive values
/// shift the rendered depth toward the near plane so the quad draws in
/// front of surrounding geometry. Not a true "disable depth test" — for
/// that, use a separate overlay camera — but it's enough for typical UI
/// labels that shouldn't get eaten by walls.
const ALWAYS_ON_TOP_DEPTH_BIAS: f32 = 10_000.0;

// ── Per-entity Slint state ─────────────────────────────────────────────────

/// Off-thread-hostile state for one billboard: the Slint component and the
/// window adapter backing it. Stored in a NonSend resource because
/// `BillboardCard` wraps `Rc` internally.
struct BillboardSlint {
    card: BillboardCard,
    adapter_weak: Weak<crate::ui::slint_ui::BevyWindowAdapter>,
}

/// NonSend map of per-billboard Slint state. Keyed by the entity that carries
/// the `BillboardGuiMarker`.
#[derive(Default)]
pub struct BillboardSlintStates {
    map: HashMap<Entity, BillboardSlint>,
}

// ── Components (all Send + Sync) ───────────────────────────────────────────

/// Send-safe per-entity data: the quad's image/material handles and the
/// current canvas dimensions. Size changes on `BillboardGuiMarker` trigger
/// texture reallocation via `sync_billboard_properties`.
#[derive(Component)]
pub struct BillboardRenderHandle {
    pub image: Handle<Image>,
    pub material: Handle<StandardMaterial>,
    pub width: u32,
    pub height: u32,
    pub last_label_hash: u64,
    pub quad_entity: Entity,
}

/// Marker on the child quad mesh so the face-camera system can find it.
#[derive(Component)]
pub struct BillboardQuad {
    /// Back-pointer to the parent BillboardGuiMarker entity so the
    /// face-camera and culling systems can read its properties without
    /// a second relational query.
    pub parent: Entity,
}

// ── Systems ────────────────────────────────────────────────────────────────

/// Mirror `BillboardGui` class fields into `BillboardGuiMarker` so Properties
/// panel edits are live. Runs only when the class component is changed.
///
/// Also writes `units_offset` onto the entity's local `Transform` so the
/// quad physically moves when the user drags the UnitsOffset value. Prior
/// versions only applied the offset once at spawn time (in
/// `spawn::spawn_billboard_gui`), so Properties-panel edits updated the
/// component but the quad stayed at its initial position — the exact
/// "UnitsOffset doesn't update in real time" bug the user reported.
fn sync_billboard_class_to_marker(
    mut q: Query<
        (&BillboardGui, &mut BillboardGuiMarker, &mut Transform),
        Changed<BillboardGui>,
    >,
) {
    for (class, mut marker, mut transform) in &mut q {
        marker.size = class.size;
        marker.max_distance = class.max_distance;
        marker.always_on_top = class.always_on_top;
        marker.visible = class.enabled;

        // Local offset from the parent (the adornee, attached via
        // ChildOf). Writing to `Transform.translation` — not
        // `GlobalTransform` — keeps parent-child propagation working
        // so the billboard tracks the part as it moves.
        let new_translation = Vec3::new(
            class.units_offset[0],
            class.units_offset[1],
            class.units_offset[2],
        );
        if (transform.translation - new_translation).length_squared() > f32::EPSILON {
            transform.translation = new_translation;
        }
    }
}

fn spawn_billboard_render_state(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut states: NonSendMut<BillboardSlintStates>,
    billboards: Query<(Entity, &BillboardGuiMarker, Option<&GlobalTransform>), Without<BillboardRenderHandle>>,
) {
    for (entity, marker, global_tf) in &billboards {
        let w = (marker.size[0].max(1.0)) as u32;
        let h = (marker.size[1].max(1.0)) as u32;
        let world_pos = global_tf
            .map(|g| g.translation())
            .unwrap_or(Vec3::ZERO);

        // Instantiate the Slint card. The engine's platform pushes a new
        // BevyWindowAdapter into a thread-local; we grab it right after.
        let card = match BillboardCard::new() {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to create BillboardCard for {:?}: {}", entity, e);
                continue;
            }
        };
        let adapter_weak = crate::ui::slint_ui::take_latest_window_adapter()
            .unwrap_or_else(Weak::new);

        card.set_canvas_width(w as i32);
        card.set_canvas_height(h as i32);

        if let Some(adapter) = adapter_weak.upgrade() {
            adapter.resize(slint::PhysicalSize::new(w, h), 1.0);
        }

        if let Err(e) = card.show() {
            warn!("Failed to show BillboardCard for {:?}: {}", entity, e);
            continue;
        }

        let (image, material) =
            create_billboard_texture(w, h, marker.always_on_top, &mut images, &mut materials);
        let (size_x, size_y) = meters_from_pixels(marker.size);

        let mesh_handle = meshes.add(build_billboard_quad_mesh());

        let quad_entity = commands
            .spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::new(size_x, size_y, 1.0)),
                Visibility::default(),
                BillboardQuad { parent: entity },
                ChildOf(entity),
                bevy::light::NotShadowCaster,
                Name::new("BillboardQuad"),
            ))
            .id();

        states
            .map
            .insert(entity, BillboardSlint { card, adapter_weak });
        commands.entity(entity).insert(BillboardRenderHandle {
            image,
            material,
            width: w,
            height: h,
            last_label_hash: 0,
            quad_entity,
        });

        info!(
            "🪧 Spawned Slint-rendered billboard for {:?} at world {:.2},{:.2},{:.2} ({}×{} px → {:.2}×{:.2} m)",
            entity, world_pos.x, world_pos.y, world_pos.z, w, h, size_x, size_y
        );
    }
}

/// React to `Changed<BillboardGuiMarker>` — push size/visibility/depth edits
/// to the quad, material, and (for size) the texture + adapter + card canvas.
#[allow(clippy::too_many_arguments)]
fn sync_billboard_properties(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut states: NonSendMut<BillboardSlintStates>,
    mut billboards: Query<
        (Entity, &BillboardGuiMarker, &mut BillboardRenderHandle),
        Changed<BillboardGuiMarker>,
    >,
    mut quads: Query<(&mut Transform, &mut Visibility), With<BillboardQuad>>,
) {
    for (entity, marker, mut handle) in &mut billboards {
        let Some(slint_state) = states.map.get(&entity) else { continue };

        let new_w = (marker.size[0].max(1.0)) as u32;
        let new_h = (marker.size[1].max(1.0)) as u32;
        let size_changed = new_w != handle.width || new_h != handle.height;

        if size_changed {
            // Reallocate the GPU image at the new dimensions. Can't resize
            // in-place because Bevy's Image owns the texture descriptor.
            let (new_image, new_material) = create_billboard_texture(
                new_w,
                new_h,
                marker.always_on_top,
                &mut images,
                &mut materials,
            );
            handle.image = new_image;
            handle.material = new_material.clone();
            handle.width = new_w;
            handle.height = new_h;
            handle.last_label_hash = 0; // force re-push on next render

            slint_state.card.set_canvas_width(new_w as i32);
            slint_state.card.set_canvas_height(new_h as i32);
            if let Some(adapter) = slint_state.adapter_weak.upgrade() {
                adapter.resize(slint::PhysicalSize::new(new_w, new_h), 1.0);
            }

            // Update the quad's material handle + scale to match.
            if let Ok((mut quad_tf, _)) = quads.get_mut(handle.quad_entity) {
                let (mx, my) = meters_from_pixels(marker.size);
                let current_scale = quad_tf.scale;
                quad_tf.scale = Vec3::new(mx, my, current_scale.z);
            }
        } else if let Some(mat) = materials.get_mut(&handle.material) {
            // Size unchanged — just sync always-on-top toggle onto the
            // existing material so we don't thrash allocations.
            mat.depth_bias = if marker.always_on_top {
                ALWAYS_ON_TOP_DEPTH_BIAS
            } else {
                0.0
            };
        }

        // Visibility from the explicit flag. `cull_billboards_by_distance`
        // may override this per frame when the camera is too far.
        if let Ok((_, mut vis)) = quads.get_mut(handle.quad_entity) {
            *vis = if marker.visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Each frame, hide quads whose parent billboard is beyond `max_distance`.
/// `max_distance <= 0` disables culling (always visible).
///
/// Runs after `sync_billboard_properties` so the per-edit visibility set
/// there is the baseline we gate against — a billboard marked `visible=false`
/// stays hidden even if the camera is in range.
fn cull_billboards_by_distance(
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<BillboardQuad>)>,
    billboards: Query<(&BillboardGuiMarker, &GlobalTransform, &BillboardRenderHandle)>,
    mut quads: Query<&mut Visibility, With<BillboardQuad>>,
) {
    let Some(cam) = cameras.iter().next() else { return };
    let cam_pos = cam.translation();

    for (marker, global_tf, handle) in &billboards {
        // Don't fight the explicit visibility flag — if the user / script
        // set visible=false, keep it hidden regardless of distance.
        if !marker.visible {
            continue;
        }
        let Ok(mut vis) = quads.get_mut(handle.quad_entity) else { continue };

        if marker.max_distance <= 0.0 {
            *vis = Visibility::Visible;
            continue;
        }

        let dist = global_tf.translation().distance(cam_pos);
        *vis = if dist <= marker.max_distance {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn update_and_render_billboards(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    states: NonSend<BillboardSlintStates>,
    mut billboards: Query<(Entity, &mut BillboardRenderHandle)>,
    children_q: Query<&Children>,
    gui_elements: Query<&GuiElementDisplay>,
) {
    slint::platform::update_timers_and_animations();

    for (entity, mut handle) in &mut billboards {
        let Some(slint_state) = states.map.get(&entity) else { continue };
        let Ok(children) = children_q.get(entity) else { continue };

        let mut elems: Vec<GuiElementDisplay> = children
            .iter()
            .filter_map(|c| gui_elements.get(c).ok().cloned())
            .collect();
        elems.sort_by_key(|e| e.z_order);

        let hash = label_hash(&elems);
        if hash != handle.last_label_hash {
            handle.last_label_hash = hash;
            // One-shot log per label-set change so it's visible when labels
            // appear/mutate without flooding when nothing's moving.
            info!(
                "🪧 billboard {:?}: pushing {} labels to card (hash={})",
                entity,
                elems.len(),
                hash
            );
            for e in &elems {
                info!(
                    "   label '{}' @ ({:.0},{:.0}) {}×{} font={}w{} color=rgba({:.2},{:.2},{:.2},{:.2})",
                    e.text, e.x, e.y, e.width, e.height,
                    e.font_size, e.font_weight,
                    e.text_color[0], e.text_color[1], e.text_color[2], e.text_color[3],
                );
            }
            push_labels_to_card(&slint_state.card, &elems);
        }

        let Some(adapter) = slint_state.adapter_weak.upgrade() else { continue };

        let Some(image) = images.get_mut(&handle.image) else { continue };
        let Some(data) = image.data.as_mut() else { continue };

        let pixels: &mut [slint::platform::software_renderer::PremultipliedRgbaColor] =
            bytemuck::cast_slice_mut(data);
        adapter.render_to_buffer(pixels, handle.width as usize);

        let _ = materials.get_mut(&handle.material);
    }
}

/// Yaw each billboard quad toward the active camera each frame — gated on
/// the parent marker's `face_camera` flag so sign-style billboards with
/// fixed rotation stay put.
fn billboard_face_camera(
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<BillboardQuad>, Without<BillboardGuiMarker>)>,
    markers: Query<&BillboardGuiMarker>,
    mut quads: Query<(&mut Transform, &GlobalTransform, &BillboardQuad)>,
) {
    let Some(cam) = cameras.iter().next() else { return };
    let cam_pos = cam.translation();

    for (mut local_tf, global_tf, quad) in &mut quads {
        let Ok(marker) = markers.get(quad.parent) else { continue };
        if !marker.face_camera {
            continue;
        }

        let quad_pos = global_tf.translation();
        let dir = cam_pos - quad_pos;
        if dir.length_squared() > 0.001 {
            let yaw = dir.x.atan2(dir.z);
            let scale = local_tf.scale;
            local_tf.rotation = Quat::from_rotation_y(yaw);
            local_tf.scale = scale;
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn meters_from_pixels(size_px: [f32; 2]) -> (f32, f32) {
    (
        size_px[0] / PIXELS_PER_METER,
        size_px[1] / PIXELS_PER_METER,
    )
}

fn create_billboard_texture(
    width: u32,
    height: u32,
    always_on_top: bool,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (Handle<Image>, Handle<StandardMaterial>) {
    use bevy::render::render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    };
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("BillboardCard"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let image_handle = images.add(image);

    let material_handle = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        depth_bias: if always_on_top {
            ALWAYS_ON_TOP_DEPTH_BIAS
        } else {
            0.0
        },
        ..default()
    });

    (image_handle, material_handle)
}

/// Unit quad centred at origin in the XY plane. Scaled by the billboard's
/// meter-size via `Transform::from_scale`, so the mesh is resolution-agnostic.
fn build_billboard_quad_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    // UV origin is top-left in Bevy/wgpu. Our pixel buffer has row 0 = top,
    // so v=0 goes with the top verts (y=+0.5).
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![
            [0.0, 1.0], // bottom-left
            [1.0, 1.0], // bottom-right
            [1.0, 0.0], // top-right
            [0.0, 0.0], // top-left
        ],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

fn color_from_arr(c: [f32; 4]) -> slint::Color {
    slint::Color::from_argb_f32(c[3], c[0], c[1], c[2])
}

fn push_labels_to_card(card: &BillboardCard, elems: &[GuiElementDisplay]) {
    let labels: Vec<BillboardLabelData> = elems
        .iter()
        .map(|e| BillboardLabelData {
            x: e.x,
            y: e.y,
            width: e.width,
            height: e.height,
            text: SharedString::from(e.text.as_str()),
            text_color: color_from_arr(e.text_color),
            bg_color: color_from_arr(e.bg_color),
            border_size: e.border_size,
            border_color: color_from_arr(e.border_color),
            corner_radius: e.corner_radius,
            font_size: e.font_size,
            font_weight: e.font_weight,
            text_align: SharedString::from(e.text_align.as_str()),
            z_order: e.z_order,
            visible: e.visible,
        })
        .collect();
    let model = Rc::new(VecModel::from(labels));
    card.set_labels(ModelRc::from(model));
}

/// Cheap change-detection key for the label set.
fn label_hash(elems: &[GuiElementDisplay]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    elems.len().hash(&mut h);
    for e in elems {
        e.text.hash(&mut h);
        e.x.to_bits().hash(&mut h);
        e.y.to_bits().hash(&mut h);
        e.width.to_bits().hash(&mut h);
        e.height.to_bits().hash(&mut h);
        e.z_order.hash(&mut h);
        e.visible.hash(&mut h);
        for c in e.text_color.iter().chain(e.bg_color.iter()).chain(e.border_color.iter()) {
            c.to_bits().hash(&mut h);
        }
        e.font_size.to_bits().hash(&mut h);
        e.font_weight.hash(&mut h);
        e.text_align.hash(&mut h);
    }
    h.finish()
}

// ── Plugin ─────────────────────────────────────────────────────────────────

pub struct BillboardGuiPlugin;

impl Plugin for BillboardGuiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(BillboardSlintStates::default())
            .add_systems(
                Update,
                (
                    sync_billboard_class_to_marker,
                    spawn_billboard_render_state.after(sync_billboard_class_to_marker),
                    sync_billboard_properties.after(spawn_billboard_render_state),
                    cull_billboards_by_distance.after(sync_billboard_properties),
                    update_and_render_billboards.after(cull_billboards_by_distance),
                    billboard_face_camera.after(update_and_render_billboards),
                ),
            );
    }
}
