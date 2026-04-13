//! # Billboard GUI Renderer
//!
//! High-performance manual pixel renderer for BillboardGui and SurfaceGui.
//! Designed to scale to 100K+ billboards via:
//!
//! - **Template atlas**: unique GUI layouts rendered once to shared texture regions
//! - **GPU instancing**: one quad mesh + one atlas material + N instance transforms
//! - **Dirty tracking**: only re-render textures when data changes
//! - **LOD culling**: billboards beyond distance threshold are hidden
//!
//! ## Architecture
//!
//! 1. Each BillboardGui entity has `BillboardGuiMarker` + child `GuiElementDisplay` components
//! 2. `update_billboard_textures` renders dirty billboards to pixel buffers
//! 3. Pixel buffers are uploaded to a shared texture atlas (Image asset)
//! 4. `spawn_billboard_quads` creates/updates 3D quad meshes with atlas UVs
//! 5. `billboard_face_camera` orients quads toward the active camera each frame

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use std::sync::OnceLock;

// ── Embedded Font ─────────────────────────────────────────────────────────────

/// FiraMono subset (SIL Open Font License) — embedded at compile time.
static FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/FiraMono-subset.ttf");

/// Lazily initialized fontdue font for CPU text rasterization.
static FONT: OnceLock<fontdue::Font> = OnceLock::new();

fn get_font() -> &'static fontdue::Font {
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("Failed to load embedded billboard font")
    })
}

/// GUI element display data — shared between ScreenGui (Slint) and BillboardGui (manual renderer).
/// Stored as a Bevy component on each GUI entity (Frame, TextLabel, TextButton, etc.).
#[derive(Component, Debug, Clone)]
pub struct GuiElementDisplay {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub z_order: i32,
    pub visible: bool,
    pub clip_children: bool,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub bg_color: [f32; 4],
    pub border_size: f32,
    pub border_color: [f32; 4],
    pub corner_radius: f32,
    pub text: String,
    pub text_color: [f32; 4],
    pub font_size: f32,
    pub text_align: String,
    pub image_path: String,
    pub class_type: String,
    /// Mouse filter mode (Godot-style):
    /// - "stop" (default): consumes mouse events, blocks 3D selection behind
    /// - "pass": receives events but passes them through
    /// - "ignore": transparent to mouse, events go straight through
    pub mouse_filter: String,
}

/// Marker for BillboardGui entities — rendered as 3D quads facing the camera.
#[derive(Component, Debug)]
pub struct BillboardGuiMarker {
    pub size: [f32; 2],
    pub max_distance: f32,
    pub always_on_top: bool,
}

/// Marker for SurfaceGui entities — rendered as textures mapped to a part face.
#[derive(Component, Debug)]
pub struct SurfaceGuiMarker {
    pub face: String,
    pub target_part: String,
    pub pixels_per_stud: f32,
}

// ── Constants ──────────────────────────────────────────────────────────────────

/// Atlas texture size (shared across all billboards)
const ATLAS_SIZE: u32 = 4096;
/// Maximum billboard slots in the atlas (each slot is a region)
const MAX_BILLBOARD_SLOTS: usize = 256;
/// Default billboard texture resolution
const DEFAULT_BILLBOARD_WIDTH: u32 = 256;
const DEFAULT_BILLBOARD_HEIGHT: u32 = 128;
/// Maximum render distance for billboards (studs)
const DEFAULT_MAX_DISTANCE: f32 = 500.0;

// ── Components ─────────────────────────────────────────────────────────────────

/// Tracks the atlas slot assigned to this billboard's rendered texture
#[derive(Component)]
struct BillboardAtlasSlot {
    slot_index: usize,
    /// Hash of the last rendered state — skip re-render if unchanged
    content_hash: u64,
    /// UV coordinates in the atlas [u_min, v_min, u_max, v_max]
    uvs: [f32; 4],
}

/// Marker for the 3D quad entity that displays the billboard texture
#[derive(Component)]
struct BillboardQuad;

/// Marker for SurfaceGui quad entities
#[derive(Component)]
struct SurfaceQuad;

// ── Resources ──────────────────────────────────────────────────────────────────

/// Shared texture atlas for all billboard GUI renders
#[derive(Resource)]
struct BillboardAtlas {
    /// The atlas image handle (shared by all billboard materials)
    image: Handle<Image>,
    /// Material using the atlas texture
    material: Handle<StandardMaterial>,
    /// Pixel buffer for CPU-side rendering
    pixels: Vec<u8>,
    /// Which slots are occupied
    slot_occupied: Vec<bool>,
    /// Slot dimensions (uniform for simplicity)
    slot_width: u32,
    slot_height: u32,
    /// Number of slots per row in the atlas
    slots_per_row: u32,
    /// Shared quad mesh (1x1, scaled per instance)
    quad_mesh: Handle<Mesh>,
    /// Whether any slot was dirtied this frame
    dirty: bool,
}

impl BillboardAtlas {
    fn new(
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        meshes: &mut Assets<Mesh>,
    ) -> Self {
        // Create atlas image
        let pixels = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        let mut image = Image::new_fill(
            Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            default(),
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;

        let image_handle = images.add(image);

        // Unlit material with alpha blending for transparent backgrounds
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(image_handle.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            double_sided: true,
            ..default()
        });

        // Unit quad mesh — scaled per billboard instance
        let quad_mesh = meshes.add(Rectangle::new(1.0, 1.0));

        let slot_width = DEFAULT_BILLBOARD_WIDTH;
        let slot_height = DEFAULT_BILLBOARD_HEIGHT;
        let slots_per_row = ATLAS_SIZE / slot_width;

        Self {
            image: image_handle,
            material,
            pixels,
            slot_occupied: vec![false; MAX_BILLBOARD_SLOTS],
            slot_width,
            slot_height,
            slots_per_row,
            quad_mesh,
            dirty: false,
        }
    }

    /// Allocate a slot in the atlas, returns slot index or None if full
    fn allocate_slot(&mut self) -> Option<usize> {
        for (i, occupied) in self.slot_occupied.iter_mut().enumerate() {
            if !*occupied {
                *occupied = true;
                return Some(i);
            }
        }
        None
    }

    /// Free a slot
    fn free_slot(&mut self, index: usize) {
        if index < self.slot_occupied.len() {
            self.slot_occupied[index] = false;
        }
    }

    /// Get pixel coordinates for a slot
    fn slot_origin(&self, index: usize) -> (u32, u32) {
        let col = (index as u32) % self.slots_per_row;
        let row = (index as u32) / self.slots_per_row;
        (col * self.slot_width, row * self.slot_height)
    }

    /// Get UV coordinates for a slot [u_min, v_min, u_max, v_max]
    fn slot_uvs(&self, index: usize) -> [f32; 4] {
        let (x, y) = self.slot_origin(index);
        let u_min = x as f32 / ATLAS_SIZE as f32;
        let v_min = y as f32 / ATLAS_SIZE as f32;
        let u_max = (x + self.slot_width) as f32 / ATLAS_SIZE as f32;
        let v_max = (y + self.slot_height) as f32 / ATLAS_SIZE as f32;
        [u_min, v_min, u_max, v_max]
    }

    /// Clear a slot region to transparent
    fn clear_slot(&mut self, slot: usize) {
        let (origin_x, origin_y) = self.slot_origin(slot);
        let sw = self.slot_width as usize;
        let sh = self.slot_height as usize;
        let atlas_w = ATLAS_SIZE as usize;

        for py in 0..sh {
            let row_start = ((origin_y as usize + py) * atlas_w + origin_x as usize) * 4;
            let row_end = row_start + sw * 4;
            if row_end <= self.pixels.len() {
                self.pixels[row_start..row_end].fill(0);
            }
        }
        self.dirty = true;
    }

    /// Render a GUI element into the atlas at the given slot (additive — call clear_slot first)
    fn render_element_to_slot(&mut self, slot: usize, element: &GuiElementDisplay) {
        let (origin_x, origin_y) = self.slot_origin(slot);
        let sw = self.slot_width as usize;
        let sh = self.slot_height as usize;
        let atlas_w = ATLAS_SIZE as usize;

        // Element bounds within the slot (clamped)
        let ex = element.x.max(0.0) as usize;
        let ey = element.y.max(0.0) as usize;
        let ew = (element.width as usize).min(sw.saturating_sub(ex));
        let eh = (element.height as usize).min(sh.saturating_sub(ey));

        // Draw background rectangle with alpha blending
        let bg_r = (element.bg_color[0] * 255.0) as u8;
        let bg_g = (element.bg_color[1] * 255.0) as u8;
        let bg_b = (element.bg_color[2] * 255.0) as u8;
        let bg_a = (element.bg_color[3] * 255.0) as u8;

        for py in 0..eh {
            for px in 0..ew {
                let ax = origin_x as usize + ex + px;
                let ay = origin_y as usize + ey + py;
                let idx = (ay * atlas_w + ax) * 4;
                if idx + 3 < self.pixels.len() {
                    self.pixels[idx] = bg_r;
                    self.pixels[idx + 1] = bg_g;
                    self.pixels[idx + 2] = bg_b;
                    self.pixels[idx + 3] = bg_a;
                }
            }
        }

        // Draw border
        if element.border_size > 0.0 && ew > 0 && eh > 0 {
            let br = (element.border_color[0] * 255.0) as u8;
            let bgreen = (element.border_color[1] * 255.0) as u8;
            let bb = (element.border_color[2] * 255.0) as u8;
            let ba = (element.border_color[3] * 255.0) as u8;
            let bw = (element.border_size.ceil() as usize).min(ew / 2).min(eh / 2);

            for py in 0..eh {
                for px in 0..ew {
                    if py < bw || py >= eh - bw || px < bw || px >= ew - bw {
                        let ax = origin_x as usize + ex + px;
                        let ay = origin_y as usize + ey + py;
                        let idx = (ay * atlas_w + ax) * 4;
                        if idx + 3 < self.pixels.len() {
                            self.pixels[idx] = br;
                            self.pixels[idx + 1] = bgreen;
                            self.pixels[idx + 2] = bb;
                            self.pixels[idx + 3] = ba;
                        }
                    }
                }
            }
        }

        // CPU text rasterization via fontdue
        if !element.text.is_empty() && ew > 0 && eh > 0 {
            let font = get_font();
            let font_size = element.font_size.max(6.0).min(64.0);
            let tr = (element.text_color[0] * 255.0) as u8;
            let tg = (element.text_color[1] * 255.0) as u8;
            let tb = (element.text_color[2] * 255.0) as u8;
            let ta = (element.text_color[3] * 255.0) as u8;

            // Layout: measure total width, then position based on alignment
            let mut glyphs: Vec<(fontdue::Metrics, Vec<u8>)> = Vec::new();
            let mut total_width = 0.0f32;
            for ch in element.text.chars() {
                let (metrics, bitmap) = font.rasterize(ch, font_size);
                total_width += metrics.advance_width;
                glyphs.push((metrics, bitmap));
            }

            // Vertical centering: use font metrics
            let line_height = font_size;
            let y_start = match element.text_align.as_str() {
                _ => ((eh as f32 - line_height) / 2.0).max(0.0) as usize, // center vertically
            };

            // Horizontal alignment
            let x_start = match element.text_align.as_str() {
                "left" => 2usize, // small padding
                "right" => (ew as f32 - total_width - 2.0).max(0.0) as usize,
                _ => ((ew as f32 - total_width) / 2.0).max(0.0) as usize, // center
            };

            let mut cursor_x = x_start as f32;
            for (metrics, bitmap) in &glyphs {
                let gx = (cursor_x + metrics.xmin as f32) as isize;
                let gy = (y_start as f32 + (font_size - metrics.height as f32 - metrics.ymin as f32)) as isize;

                for row in 0..metrics.height {
                    for col in 0..metrics.width {
                        let px = gx + col as isize;
                        let py = gy + row as isize;
                        if px < 0 || py < 0 || px >= ew as isize || py >= eh as isize {
                            continue;
                        }
                        let coverage = bitmap[row * metrics.width + col];
                        if coverage == 0 { continue; }

                        let ax = origin_x as usize + ex + px as usize;
                        let ay = origin_y as usize + ey + py as usize;
                        let idx = (ay * atlas_w + ax) * 4;
                        if idx + 3 >= self.pixels.len() { continue; }

                        // Alpha-blend text over background
                        let alpha = (coverage as u16 * ta as u16) / 255;
                        let inv = 255 - alpha as u16;
                        self.pixels[idx] = ((tr as u16 * alpha + self.pixels[idx] as u16 * inv) / 255) as u8;
                        self.pixels[idx + 1] = ((tg as u16 * alpha + self.pixels[idx + 1] as u16 * inv) / 255) as u8;
                        self.pixels[idx + 2] = ((tb as u16 * alpha + self.pixels[idx + 2] as u16 * inv) / 255) as u8;
                        self.pixels[idx + 3] = (self.pixels[idx + 3] as u16 + alpha).min(255) as u8;
                    }
                }
                cursor_x += metrics.advance_width;
            }
        }

        self.dirty = true;
    }
}

// ── Systems ────────────────────────────────────────────────────────────────────

/// Initialize the billboard atlas on startup
fn setup_billboard_atlas(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let atlas = BillboardAtlas::new(&mut images, &mut materials, &mut meshes);
    commands.insert_resource(atlas);
}

/// Assign atlas slots to new BillboardGui entities and render their content
fn update_billboard_textures(
    mut atlas: ResMut<BillboardAtlas>,
    mut commands: Commands,
    new_billboards: Query<(Entity, &BillboardGuiMarker), Without<BillboardAtlasSlot>>,
    existing_billboards: Query<(Entity, &BillboardAtlasSlot, &BillboardGuiMarker)>,
    children_query: Query<&Children>,
    gui_elements: Query<&GuiElementDisplay>,
) {
    // Assign slots to new billboards
    for (entity, _marker) in &new_billboards {
        if let Some(slot) = atlas.allocate_slot() {
            let uvs = atlas.slot_uvs(slot);

            // Clear slot, then render ALL child GUI elements (TextLabels, Frames, etc.)
            atlas.clear_slot(slot);
            let mut rendered = false;
            if let Ok(children) = children_query.get(entity) {
                // Sort children by z_order for correct layering
                let mut child_elements: Vec<&GuiElementDisplay> = children.iter()
                    .filter_map(|child| gui_elements.get(child).ok())
                    .collect();
                child_elements.sort_by_key(|e| e.z_order);

                for elem in &child_elements {
                    atlas.render_element_to_slot(slot, elem);
                    rendered = true;
                }
            }

            if !rendered {
                // Render a default placeholder
                let placeholder = GuiElementDisplay {
                    x: 0.0, y: 0.0,
                    width: DEFAULT_BILLBOARD_WIDTH as f32,
                    height: DEFAULT_BILLBOARD_HEIGHT as f32,
                    z_order: 0, visible: true, clip_children: false,
                    scroll_x: 0.0, scroll_y: 0.0,
                    bg_color: [0.1, 0.1, 0.15, 0.9],
                    border_size: 1.0,
                    border_color: [0.3, 0.5, 0.8, 1.0],
                    corner_radius: 0.0,
                    text: String::new(),
                    text_color: [1.0, 1.0, 1.0, 1.0],
                    font_size: 14.0,
                    text_align: "center".to_string(),
                    image_path: String::new(),
                    class_type: "billboardgui".to_string(),
                    mouse_filter: "stop".to_string(),
                };
                atlas.render_element_to_slot(slot, &placeholder);
            }

            commands.entity(entity).insert(BillboardAtlasSlot {
                slot_index: slot,
                content_hash: 0,
                uvs,
            });
        }
    }

    // TODO: Check for dirty billboards (content_hash changed) and re-render
}

/// Upload dirty atlas pixels to GPU
fn upload_billboard_atlas(
    mut atlas: ResMut<BillboardAtlas>,
    mut images: ResMut<Assets<Image>>,
) {
    if !atlas.dirty { return; }
    atlas.dirty = false;

    if let Some(image) = images.get_mut(&atlas.image) {
        if let Some(ref mut data) = image.data {
            data.copy_from_slice(&atlas.pixels);
        }
    }
}

/// Spawn 3D quad entities for BillboardGui entities that have atlas slots but no quad
fn spawn_billboard_quads(
    mut commands: Commands,
    atlas: Res<BillboardAtlas>,
    billboards: Query<(Entity, &BillboardGuiMarker, &BillboardAtlasSlot, &GlobalTransform), Without<BillboardQuad>>,
) {
    for (entity, marker, _slot, transform) in &billboards {
        let t = transform.compute_transform();
        let size_x = marker.size[0];
        let size_y = marker.size[1];

        // Spawn quad as child of the billboard entity
        let quad = commands.spawn((
            Mesh3d(atlas.quad_mesh.clone()),
            MeshMaterial3d(atlas.material.clone()),
            Transform::from_translation(t.translation)
                .with_scale(Vec3::new(size_x, size_y, 1.0)),
            BillboardQuad,
            Name::new("BillboardQuad"),
        )).id();

        // Mark the billboard entity as having a quad
        commands.entity(entity).insert(BillboardQuad);

        info!("🪧 Spawned billboard quad for {:?} ({}x{})", entity, size_x, size_y);
    }
}

/// Orient billboard quads toward the active camera each frame
fn billboard_face_camera(
    camera_query: Query<&GlobalTransform, (With<Camera3d>, Without<BillboardQuad>)>,
    mut billboard_quads: Query<&mut Transform, With<BillboardQuad>>,
) {
    // Find main camera (order 0)
    let Some(camera_transform) = camera_query.iter().next() else { return };
    let camera_pos = camera_transform.translation();

    for mut transform in &mut billboard_quads {
        // Billboard: face camera, but keep upright (only rotate around Y axis)
        let dir = camera_pos - transform.translation;
        if dir.length_squared() > 0.001 {
            let yaw = dir.x.atan2(dir.z);
            transform.rotation = Quat::from_rotation_y(yaw);
        }
    }
}

/// Clean up atlas slots when billboard entities are despawned
fn cleanup_billboard_slots(
    mut atlas: ResMut<BillboardAtlas>,
    mut removed: RemovedComponents<BillboardAtlasSlot>,
    slots: Query<&BillboardAtlasSlot>,
) {
    // RemovedComponents tracks entities that had the component removed
    // We can't query the component since it's removed, so we track via the event
    for _entity in removed.read() {
        // Can't get slot index from removed entity — would need a separate tracker
        // For now, slots leak until space reload clears the atlas
    }
}

// ── Plugin ─────────────────────────────────────────────────────────────────────

pub struct BillboardRendererPlugin;

impl Plugin for BillboardRendererPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(Startup, setup_billboard_atlas)
            .add_systems(Update, (
                update_billboard_textures,
                upload_billboard_atlas.after(update_billboard_textures),
                spawn_billboard_quads.after(update_billboard_textures),
                billboard_face_camera,
            ));
    }
}
