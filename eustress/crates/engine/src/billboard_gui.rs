//! 3D billboard rendering — atlas-based software renderer.
//!
//! All billboards share a single GPU texture (the **atlas**) divided into
//! fixed-size tiles, with one tile per billboard entity. Per-frame work is:
//!
//! 1. For each billboard whose content changed (hash mismatch), render its
//!    GUI subtree into a temporary CPU pixmap using `tiny-skia` + `cosmic-text`
//!    (the same software-renderer stack Slint uses internally).
//! 2. Blit that pixmap into the shared atlas CPU buffer at the entity's
//!    assigned tile offset.
//! 3. After all dirty tiles are rendered, copy the entire CPU buffer into
//!    the atlas `Image` asset once — Bevy's render-asset extraction then
//!    uploads the texture in a single `write_texture` call.
//!
//! Each billboard quad samples the same atlas, with per-entity `BillboardUv`
//! uniforms holding `uv_min`/`uv_max` so the shader maps the quad's
//! `[0,1]×[0,1]` UV range onto the entity's tile region.
//!
//! ## Why an atlas?
//!
//! Per-billboard `Image` assets cost one GPU upload + one bind group + one
//! draw per dirty billboard per frame. For hundreds of billboards on screen
//! that becomes the bottleneck. The atlas collapses N uploads into 1 (only
//! the atlas asset is mutated), keeps every billboard's bind group identical
//! (sharing the same texture), and lets static billboards cost zero CPU
//! after their first render — their tile sits in the atlas forever.
//!
//! ## Why not Slint components?
//!
//! Earlier attempts tried `BillboardCard::new()` (a Slint component) per
//! billboard. That fails because Slint's `SharedGlobals.window_adapter` is
//! a `OnceCell` shared across every component compiled from the same
//! `include_modules!()` invocation — every component reuses the
//! StudioWindow's adapter, so per-billboard render targets are impossible
//! through the public API. Direct `tiny-skia` + `cosmic-text` use IS the
//! same stack Slint's `SoftwareRenderer` uses underneath, just one layer
//! lower.

use bevy::prelude::*;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use eustress_common::classes::BillboardGui;
use eustress_common::gui::billboard_renderer::{BillboardGuiMarker, GuiElementDisplay};
use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer as TextBuffer, Color as CosmicColor, Family, FontSystem, Metrics,
    Shaping, SwashCache,
};
// Alias `tiny_skia::Transform` so it doesn't shadow Bevy's `Transform`
// component when both are imported into systems via `use bevy::prelude::*`.
use tiny_skia::{
    Paint, PathBuilder, Pixmap, Rect, Stroke, Transform as TsTransform,
};

// ── Constants ──────────────────────────────────────────────────────────────

const PIXELS_PER_METER: f32 = 50.0;

/// World-space metres of camera-toward shift per `BillboardGui.ZIndex` unit.
///
/// ZIndex semantics: positive values pull the quad toward the camera so the
/// label appears in front of the part it's pinned to. The shift is in
/// world-space, so depth-occlusion against OTHER geometry still works —
/// unlike `AlwaysOnTop` which bypasses depth entirely.
///
/// Sized to clear a 1-stud (1 m diameter) part at `ZIndex = 1`: shifting by
/// ~50 cm = half a stud + small margin, so a label sitting at the part's
/// centre rides past the part's surface from any camera angle. Bigger parts
/// need bigger ZIndex (e.g. a 2-stud sphere needs `ZIndex = 2` for full
/// clearance). Negative ZIndex pushes the quad AWAY from camera so the
/// part occludes the label — useful for "label visible only when nothing
/// blocks line-of-sight" UX.
const Z_INDEX_METRES_PER_UNIT: f32 = 0.5;

/// Atlas tile pixel width. Sized to fit billboards up to ~5 m × 5 m
/// world quads without aspect distortion (5 m × 50 px/m = 250 logical
/// pixels, fits comfortably in 256). Oversized billboards are clamped
/// at render time and a one-shot warn is emitted.
const TILE_W: u32 = 256;
/// Tile pixel height. Equal to `TILE_W` (square tiles) so vertical
/// canvas dimensions up to ~5 m fit without clamping. Asymmetric tiles
/// (e.g. `TILE_H = 128`) double slot count but stretch any billboard
/// whose canvas exceeds the tile height — vertical text gets aspect-
/// distorted by `canvas_h / TILE_H`, which makes `Size.Y.Scale > 2.56`
/// (= 128 / 50) visibly ugly. Square tiles trade slot density for
/// distortion-free rendering up to 5 studs per axis.
const TILE_H: u32 = 256;
/// Atlas columns. Fixed — `try_grow` only adds rows, so column count
/// stays constant and every existing tile's `umin/umax` survives a grow
/// untouched (only `vmin/vmax` need a refresh).
const ATLAS_COLS: u32 = 16;
/// Initial row count. 16 cols × 16 rows = 256 slots, 4096×4096 RGBA =
/// 64 MiB CPU/GPU. Same memory footprint as a 16×32 × 256×128 atlas,
/// just rebalanced toward taller tiles. `try_grow` doubles rows on
/// demand (up to `MAX_ATLAS_DIM`), so hitting 256 slots isn't a hard
/// ceiling.
const INITIAL_ATLAS_ROWS: u32 = 16;
/// Hard cap on atlas dimension in pixels. wgpu's per-device
/// `max_texture_dimension_2d` is typically 8192 on integrated GPUs and
/// 16384 on discrete. We pick the conservative limit so grow can't
/// produce a texture the GPU rejects. Beyond this we refuse to spawn.
const MAX_ATLAS_DIM: u32 = 8192;

// ── NonSend resources ──────────────────────────────────────────────────────

/// FontSystem + SwashCache are `!Send`; live in a NonSend resource.
pub struct BillboardTextState {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl Default for BillboardTextState {
    fn default() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }
}

/// Shared atlas backing every billboard. The CPU buffer is staged here and
/// copied wholesale into the atlas `Image` asset at end of frame so Bevy's
/// render-asset extraction picks up the change.
///
/// `!Send` because `Pixmap` (used during rendering) isn't `Send`; treating
/// the atlas itself as NonSend keeps every billboard system on the main
/// thread, which is fine — the renderer is CPU-bound and serial anyway.
pub struct BillboardAtlas {
    /// Atlas Image asset shared by every billboard's bind group.
    pub texture: Handle<Image>,
    /// Current row count. Grows on demand (rows doubled) when
    /// `free_slots` is exhausted, up to `MAX_ATLAS_DIM / TILE_H`.
    pub rows: u32,
    /// CPU-side staging buffer (RGBA8, premultiplied).
    /// Length = `atlas_w_px() * atlas_h_px() * 4`.
    pub cpu_buf: Vec<u8>,
    /// Free-slot stack. `pop()` allocates a tile; despawn pushes it back.
    pub free_slots: Vec<u32>,
    /// True when at least one tile changed this frame; tells the upload
    /// system to copy the CPU buffer into the GPU image.
    pub dirty: bool,
}

impl BillboardAtlas {
    fn new(texture: Handle<Image>, rows: u32) -> Self {
        let total_pixels = (Self::atlas_w_px_static() * Self::atlas_h_px_for_rows(rows)) as usize;
        let mut free_slots: Vec<u32> = (0..(ATLAS_COLS * rows)).collect();
        // Reverse so `pop()` hands out slot 0 first (debugging convenience —
        // first billboard occupies the top-left tile, easy to find on a
        // texture dump).
        free_slots.reverse();
        Self {
            texture,
            rows,
            cpu_buf: vec![0u8; total_pixels * 4],
            free_slots,
            dirty: true,
        }
    }

    #[inline] pub fn atlas_w_px_static() -> u32 { ATLAS_COLS * TILE_W }
    #[inline] pub fn atlas_h_px_for_rows(rows: u32) -> u32 { rows * TILE_H }
    #[inline] pub fn atlas_w_px(&self) -> u32 { Self::atlas_w_px_static() }
    #[inline] pub fn atlas_h_px(&self) -> u32 { Self::atlas_h_px_for_rows(self.rows) }
    #[inline] pub fn total_slots(&self) -> u32 { ATLAS_COLS * self.rows }

    /// Compute UV bounds in atlas-relative `[0,1]` for a tile slot.
    /// Non-static because `vmin/vmax` depend on the current row count.
    fn slot_uv(&self, slot: u32) -> (Vec2, Vec2) {
        let col = slot % ATLAS_COLS;
        let row = slot / ATLAS_COLS;
        let umin = col as f32 / ATLAS_COLS as f32;
        let vmin = row as f32 / self.rows as f32;
        let umax = (col + 1) as f32 / ATLAS_COLS as f32;
        let vmax = (row + 1) as f32 / self.rows as f32;
        (Vec2::new(umin, vmin), Vec2::new(umax, vmax))
    }

    /// Attempt to double the row count. Returns `Some(new_rows)` if the
    /// grow happened (atlas dimensions can now hold more tiles), or
    /// `None` if we hit `MAX_ATLAS_DIM`. The caller is responsible for:
    ///
    /// 1. Resizing the `Image` asset on disk (we do that here via the
    ///    `images` mutable handle).
    /// 2. Refreshing every live billboard's `BillboardUv` since
    ///    `vmin/vmax` now scale against a larger `rows` count.
    /// 3. Pushing the new slot indices onto `free_slots`.
    fn try_grow(&mut self, images: &mut Assets<Image>) -> Option<u32> {
        let new_rows = self.rows.saturating_mul(2);
        let new_height_px = Self::atlas_h_px_for_rows(new_rows);
        if new_height_px > MAX_ATLAS_DIM {
            return None;
        }

        // Resize the GPU-bound asset. Image::resize extends the data Vec
        // at the END (row-major linear layout grows downward), so
        // existing tile pixel positions are preserved automatically.
        let Some(image) = images.get_mut(&self.texture) else {
            warn!("🪧 atlas: Image handle not in Assets<Image>, cannot grow");
            return None;
        };
        image.resize(bevy::render::render_resource::Extent3d {
            width: Self::atlas_w_px_static(),
            height: new_height_px,
            depth_or_array_layers: 1,
        });

        // Grow our CPU staging buffer in lockstep. Same layout: linear
        // RGBA, atlas_w_px stride. New rows zero-init at the tail.
        let new_size_bytes =
            (Self::atlas_w_px_static() as usize) * (new_height_px as usize) * 4;
        self.cpu_buf.resize(new_size_bytes, 0);

        // Push the new slot indices. The old rows still hold their
        // slots (0..ATLAS_COLS*old_rows); new range is
        // (ATLAS_COLS*old_rows .. ATLAS_COLS*new_rows).
        let old_slot_max = ATLAS_COLS * self.rows;
        let new_slot_max = ATLAS_COLS * new_rows;
        // Reverse so lower-index new slots pop first.
        for slot in (old_slot_max..new_slot_max).rev() {
            self.free_slots.push(slot);
        }

        let old_rows = self.rows;
        self.rows = new_rows;
        self.dirty = true;
        info!(
            "🪧 atlas grew: {} rows → {} rows ({} → {} slots, {}×{} → {}×{} px)",
            old_rows, new_rows,
            old_rows * ATLAS_COLS, new_slot_max,
            Self::atlas_w_px_static(), Self::atlas_h_px_for_rows(old_rows),
            Self::atlas_w_px_static(), new_height_px,
        );
        Some(new_rows)
    }
}

// ── Components ─────────────────────────────────────────────────────────────

/// Per-billboard tile assignment. Stable for the entity's lifetime.
#[derive(Component, Clone, Copy)]
pub struct BillboardAtlasTile {
    pub slot: u32,
}

/// Per-billboard render state: hash of last-rendered content + logical
/// pixel size (≤ tile size). The atlas slot lives in `BillboardAtlasTile`.
#[derive(Component)]
pub struct BillboardRenderHandle {
    pub width: u32,
    pub height: u32,
    pub last_label_hash: u64,
}

// ── Systems ────────────────────────────────────────────────────────────────

/// Backstop: ensure every `BillboardGui` entity has a `BillboardGuiMarker`
/// attached. Every documented spawn path (file_loader, spawn_billboard_gui,
/// file_watcher hot-create) attaches the marker explicitly, but a script,
/// plugin, or future code path that builds the class component alone would
/// otherwise produce an invisible billboard — `sync_billboard_class_to_marker`
/// queries `&mut BillboardGuiMarker` and silently skips entities without it.
/// Inserting a default marker here unblocks them.
fn ensure_billboard_marker(
    mut commands: Commands,
    missing: Query<
        Entity,
        (
            With<BillboardGui>,
            Without<BillboardGuiMarker>,
        ),
    >,
) {
    for entity in &missing {
        commands.entity(entity).insert(BillboardGuiMarker::default());
        debug!("🪧 attached default BillboardGuiMarker to {:?}", entity);
    }
}

/// Mirror every Roblox-parity property from the `BillboardGui` class onto
/// the `BillboardGuiMarker` that the renderer reads. Runs whenever the
/// class changes (Properties panel edit, script assignment, TOML reload).
///
/// Two world-space placement fields drive `Transform.translation`:
///
/// - `units_offset` — Roblox `StudsOffset`. Local-axis offset relative
///   to the parent's frame; **rotated** by the parent's orientation when
///   composed into world space. Bevy gives this for free because
///   `Transform` is local to `ChildOf(parent)`.
///
/// - `units_offset_world_space` — Roblox `StudsOffsetWorldSpace`.
///   World-axis offset; **not** rotated by parent. Implemented by
///   inverse-rotating with the parent's world rotation before adding
///   to `Transform.translation` so the parent transform chain
///   re-applies the rotation and the net offset stays world-axis.
///
/// `extents_offset` / `extents_offset_world_space` need adornee
/// bounding-box info and are still a follow-up.
fn sync_billboard_class_to_marker(
    mut q: Query<
        (Entity, &BillboardGui, &mut BillboardGuiMarker, &mut Transform, Option<&ChildOf>),
        Changed<BillboardGui>,
    >,
    parent_globals: Query<&GlobalTransform>,
    mut last_offsets: Local<HashMap<Entity, [f32; 6]>>,
) {
    for (entity, class, mut marker, mut transform, child_of) in &mut q {
        // Geometry. `class.size` is `UDim2` for Roblox parity (Scale =
        // studs, Offset = pixels). The renderer wants pure pixels, so
        // resolve via `to_pixels` using PIXELS_PER_METER as the
        // studs→pixels reference. `Offset` comes through as-is; pure-
        // pixel sizes (`UDim2::from_pixels(200, 50)`) flow with
        // Scale = 0 and don't depend on the reference at all.
        //
        // Disappearance guard: a fully-zero `UDim2` (`0, 0, 0, 0`)
        // collapses to 0×0 pixels, which renders as an effectively
        // invisible 2 cm world quad and reads as "the billboard
        // disappeared after I cleared a field". When BOTH axes resolve
        // to zero we treat that as an unintentional clear and fall
        // back to a small but findable 8 cm × 8 cm quad (4 px on each
        // axis). Any non-zero per-axis Offset is honoured EXACTLY —
        // `Size = UDim2(0, 2, 0, 2)` for a deliberate 2-px dot still
        // produces a 4 cm world quad as requested. Only the truly
        // empty case gets the rescue floor.
        let [w_raw, h_raw] = class.size.to_pixels(PIXELS_PER_METER, PIXELS_PER_METER);
        // `size_offset` is Roblox-parity `Vector2` — already in pixels.
        let [sox, soy] = class.size_offset;
        let (w_px, h_px) = if w_raw <= 0.0 && h_raw <= 0.0 {
            (4.0, 4.0)
        } else {
            // Per-axis: keep what the user asked for. If only ONE axis
            // is zero (e.g. Y-Offset = 0 but X has 200 px), preserve
            // the zero-then-clamp behaviour at 1 px so we never have a
            // negative-area quad, but don't promote it to the 4 px floor.
            (w_raw.max(1.0), h_raw.max(1.0))
        };
        marker.size = [w_px, h_px];
        marker.size_offset = [sox, soy];
        marker.extents_offset = class.extents_offset;
        marker.extents_offset_world_space = class.extents_offset_world_space;
        marker.units_offset_world_space = class.units_offset_world_space;

        // Distance — Roblox uses both `MaxDistance` and `DistanceUpperLimit`.
        // We honour the more restrictive of the two when both are set
        // (treating 0 as "unset / no limit" for both).
        marker.max_distance = match (class.max_distance, class.distance_upper_limit) {
            (m, u) if m > 0.0 && u > 0.0 => m.min(u),
            (m, u) if m > 0.0 => m,
            (_, u) if u > 0.0 => u,
            _ => 0.0,
        };
        marker.distance_lower_limit = class.distance_lower_limit.max(0.0);
        marker.distance_step = class.distance_step.max(0.0);

        // Layering / depth
        marker.always_on_top = class.always_on_top;
        marker.clips_descendants = class.clips_descendants;

        // Appearance
        marker.brightness = class.brightness.clamp(0.0, 8.0);
        marker.light_influence = class.light_influence.clamp(0.0, 1.0);

        // Visibility — Roblox `Enabled` is the user-facing toggle.
        marker.visible = class.enabled;

        // FaceCamera — Roblox-parity behaviour toggle. When false, the
        // pipeline uses the entity's Transform rotation literally
        // instead of camera-aligning the quad (see `BillboardLockAxis`
        // mapping below — `face_camera = false` → `lock_axis.rotation = true`).
        marker.face_camera = class.face_camera;

        // ZIndex depth-bias. Drives the WGSL vertex shader to shift the
        // quad along the camera-toward direction so it can win the
        // depth test against geometry it's pinned to (e.g. a sphere it
        // sits on) without bypassing depth for closer geometry.
        marker.z_index = class.z_index;

        // Combine `units_offset` (local-axis, Roblox `StudsOffset`) with
        // `units_offset_world_space` (world-axis, Roblox
        // `StudsOffsetWorldSpace`). The latter has to be expressed in
        // the parent's LOCAL frame before storing in `Transform.translation`,
        // otherwise the parent's GlobalTransform composition rotates it
        // a second time and the offset stops being world-axis. Inverse-
        // rotating with the parent's world rotation does exactly that —
        // for an unrotated parent (or no parent) the inverse is identity
        // and the two offsets simply add componentwise.
        let parent_rot = child_of
            .and_then(|c| parent_globals.get(c.parent()).ok())
            .map(|gt| gt.rotation())
            .unwrap_or(Quat::IDENTITY);
        let local_uo = Vec3::from(class.units_offset);
        let world_uo = Vec3::from(class.units_offset_world_space);
        let combined = local_uo + parent_rot.inverse() * world_uo;

        // Cache against the 6-tuple (local + world) so a Properties-panel
        // edit to EITHER offset rewrites Transform on the next sync, but
        // unchanged frames don't fight other transform writers.
        let key: [f32; 6] = [
            class.units_offset[0], class.units_offset[1], class.units_offset[2],
            class.units_offset_world_space[0],
            class.units_offset_world_space[1],
            class.units_offset_world_space[2],
        ];
        let cached = last_offsets.get(&entity).copied();
        let offsets_changed = cached.map_or(true, |c| c != key);
        if offsets_changed {
            transform.translation = combined;
            last_offsets.insert(entity, key);
        }
    }
}

fn spawn_billboard_render_state(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut atlas: NonSendMut<BillboardAtlas>,
    mut billboards: Query<
        (Entity, &BillboardGuiMarker, Option<&GlobalTransform>, &mut Transform),
        Without<BillboardRenderHandle>,
    >,
    // Existing billboards (already have a tile) — needed to refresh their
    // UVs after the atlas grows, because `vmin/vmax` are scaled against
    // `atlas.rows` and that doubled on grow.
    mut existing: Query<
        (
            &BillboardAtlasTile,
            &mut crate::billboard_pipeline::BillboardUv,
            &BillboardRenderHandle,
        ),
        With<BillboardRenderHandle>,
    >,
) {
    for (entity, marker, global_tf, mut transform) in &mut billboards {
        let raw_w = marker.size[0].max(1.0) as u32;
        let raw_h = marker.size[1].max(1.0) as u32;
        // Clamp to tile size — oversized content gets cropped, but the
        // billboard still spawns. Wide billboards beyond TILE_W get
        // cropped on the right; tall billboards beyond TILE_H get
        // cropped on the bottom.
        let w = raw_w.min(TILE_W);
        let h = raw_h.min(TILE_H);
        if raw_w > TILE_W || raw_h > TILE_H {
            warn!(
                "🪧 billboard {:?}: size {}×{} exceeds tile {}×{} — clamped",
                entity, raw_w, raw_h, TILE_W, TILE_H,
            );
        }

        // Slot allocation with grow-on-demand. Try `pop`, then `try_grow`,
        // then `pop` again. If grow fails (would exceed `MAX_ATLAS_DIM`),
        // log an error and skip — the entity stays in the main world but
        // has no render state, so it stays invisible until a slot frees.
        let slot = match atlas.free_slots.pop() {
            Some(s) => s,
            None => {
                match atlas.try_grow(&mut images) {
                    Some(_) => {
                        // Refresh every existing billboard's UV — their
                        // `vmin/vmax` are scaled against `atlas.rows`,
                        // which just doubled. Without this every tile
                        // before the grow would sample double-tall after
                        // the grow. Apply the same half-texel inset as
                        // the initial spawn so no bilinear bleed at
                        // tile boundaries.
                        let atlas_w_px = BillboardAtlas::atlas_w_px_static() as f32;
                        let atlas_h_px = atlas.atlas_h_px() as f32;
                        let half_u = 0.5 / atlas_w_px;
                        let half_v = 0.5 / atlas_h_px;
                        for (tile, mut uv, handle) in &mut existing {
                            let (uv_min, uv_max) = atlas.slot_uv(tile.slot);
                            let effective_umax = uv_min.x
                                + (uv_max.x - uv_min.x) * (handle.width  as f32 / TILE_W as f32);
                            let effective_vmax = uv_min.y
                                + (uv_max.y - uv_min.y) * (handle.height as f32 / TILE_H as f32);
                            uv.uv_min = Vec2::new(uv_min.x + half_u, uv_min.y + half_v);
                            uv.uv_max = Vec2::new(effective_umax - half_u, effective_vmax - half_v);
                        }
                        atlas.free_slots.pop()
                            .expect("free_slots populated by try_grow")
                    }
                    None => {
                        error!(
                            "🪧 billboard {:?}: atlas at MAX_ATLAS_DIM ({} px), {} slots used — not rendering",
                            entity, MAX_ATLAS_DIM, atlas.total_slots(),
                        );
                        continue;
                    }
                }
            }
        };
        let (uv_min, uv_max) = atlas.slot_uv(slot);

        let world_pos = global_tf.map(|g: &GlobalTransform| g.translation()).unwrap_or(Vec3::ZERO);
        let (size_x, size_y) = meters_from_pixels([w as f32, h as f32]);
        let mesh_handle = meshes.add(crate::billboard_pipeline::build_billboard_quad_mesh());

        // The quad's [0,1]×[0,1] UV range needs to map to the tile region
        // in the atlas, but we ALSO want oversized atlas tiles to show only
        // the rendered portion (the top-left `w × h` pixels of the tile).
        // Compute the effective UV box accordingly.
        let effective_umax = uv_min.x + (uv_max.x - uv_min.x) * (w as f32 / TILE_W as f32);
        let effective_vmax = uv_min.y + (uv_max.y - uv_min.y) * (h as f32 / TILE_H as f32);
        // Half-texel inset: with bilinear filtering enabled, a sample
        // taken at the EXACT edge of a tile's UV range blends 50/50
        // with the neighboring tile's edge pixel (still inside the
        // atlas, so ClampToEdge addressing doesn't catch it). That's
        // how the user's "red O / white smudge floating in the world"
        // artifacts appeared — fragments of other billboards' atlas
        // content bleeding through. Shrinking the sampled rectangle
        // inward by half a texel on each side guarantees every
        // bilinear sample's 4 neighbors are inside the canvas region.
        let (atlas_w_px, atlas_h_px) = (
            BillboardAtlas::atlas_w_px_static() as f32,
            atlas.atlas_h_px() as f32,
        );
        let half_texel_u = 0.5 / atlas_w_px;
        let half_texel_v = 0.5 / atlas_h_px;
        let uv_min_inset = Vec2::new(uv_min.x + half_texel_u, uv_min.y + half_texel_v);
        let uv_max_inset = Vec2::new(effective_umax - half_texel_u, effective_vmax - half_texel_v);

        // `face_camera = false` → emit a `BillboardLockAxis::rotation`
        // marker so the WGSL pipeline picks the LOCK_ROTATION variant,
        // which uses the entity's Transform.rotation literally instead
        // of camera-aligning the quad. The component's absence
        // (face_camera = true) leaves the standard camera-facing path.
        let mut ec = commands.entity(entity);
        ec.insert((
            crate::billboard_pipeline::Billboard,
            crate::billboard_pipeline::BillboardMesh(mesh_handle),
            crate::billboard_pipeline::BillboardAtlasTexture(atlas.texture.clone()),
            crate::billboard_pipeline::BillboardUv {
                uv_min: uv_min_inset,
                uv_max: uv_max_inset,
                z_bias: marker.z_index as f32 * Z_INDEX_METRES_PER_UNIT,
                _padding: 0.0,
            },
            crate::billboard_pipeline::BillboardDepth(!marker.always_on_top),
            BillboardAtlasTile { slot },
            BillboardRenderHandle {
                width: w,
                height: h,
                last_label_hash: 0,
            },
        ));
        if !marker.face_camera {
            ec.insert(crate::billboard_pipeline::BillboardLockAxis {
                y_axis: false,
                rotation: true,
            });
        }

        transform.scale = Vec3::new(size_x, size_y, 1.0);
        let _ = world_pos;
    }
}

/// Reclaim atlas slots from despawned billboards. We watch
/// `RemovedComponents<BillboardAtlasTile>` so the slot returns to the free
/// stack regardless of whether the entire entity went away or just the
/// tile component was removed.
fn release_atlas_slots(
    mut atlas: NonSendMut<BillboardAtlas>,
    mut removed: RemovedComponents<BillboardAtlasTile>,
    // We can't read components on already-despawned entities, so the slot
    // index needs to be cached BEFORE removal. We piggyback on the entity
    // → slot map maintained as systems run.
    mut slot_map: Local<HashMap<Entity, u32>>,
    live: Query<(Entity, &BillboardAtlasTile)>,
) {
    // Refresh the cache from the live query so newly-spawned tiles are
    // tracked. Entity → slot is stable, so we never overwrite once set.
    for (e, tile) in live.iter() {
        slot_map.entry(e).or_insert(tile.slot);
    }
    for entity in removed.read() {
        if let Some(slot) = slot_map.remove(&entity) {
            atlas.free_slots.push(slot);
            // Zero the tile so a future occupant doesn't see ghost pixels
            // before its first render.
            let atlas_w_px = BillboardAtlas::atlas_w_px_static();
            zero_tile_in_atlas(&mut atlas.cpu_buf, slot, atlas_w_px);
            atlas.dirty = true;
        }
    }
}

fn zero_tile_in_atlas(cpu_buf: &mut [u8], slot: u32, atlas_w_px: u32) {
    let col = slot % ATLAS_COLS;
    let row = slot / ATLAS_COLS;
    let ox = (col * TILE_W) as usize;
    let oy = (row * TILE_H) as usize;
    let stride = (atlas_w_px as usize) * 4;
    let row_bytes = (TILE_W as usize) * 4;
    for r in 0..TILE_H as usize {
        let dst = (oy + r) * stride + ox * 4;
        for b in &mut cpu_buf[dst..dst + row_bytes] {
            *b = 0;
        }
    }
}

fn sync_billboard_properties(
    mut commands: Commands,
    atlas: NonSend<BillboardAtlas>,
    mut billboards: Query<
        (
            Entity,
            &BillboardGuiMarker,
            &mut BillboardRenderHandle,
            &mut Transform,
            &mut crate::billboard_pipeline::BillboardDepth,
            &mut Visibility,
            &BillboardAtlasTile,
            &mut crate::billboard_pipeline::BillboardUv,
            Option<&crate::billboard_pipeline::BillboardLockAxis>,
        ),
        Changed<BillboardGuiMarker>,
    >,
) {
    for (
        entity,
        marker,
        mut handle,
        mut transform,
        mut depth,
        mut vis,
        tile,
        mut uv,
        lock_axis,
    ) in &mut billboards
    {
        // Two separate concepts:
        //
        //   raw_w_px  = the canvas pixel size the user asked for via UDim2.
        //               This drives the WORLD-SPACE quad dimensions
        //               directly (1 stud = PIXELS_PER_METER pixels).
        //               Uncapped — a UDim2 with Scale=10 yields a 10-stud
        //               billboard regardless of atlas tile dimensions.
        //
        //   canvas_w / canvas_h = the actual pixel region inside the
        //               atlas tile. Capped at TILE_W/H because the
        //               atlas slot is fixed-size; oversized canvases
        //               render their content scaled down or cropped.
        //
        // Previously the world scale ALSO used the clamped value, which
        // made `Scale = 10` produce a 5.12-stud billboard (TILE_W / 50)
        // instead of 10 — visibly indistinguishable from `Scale = 5`.
        let raw_w_px = marker.size[0].max(1.0);
        let raw_h_px = marker.size[1].max(1.0);
        let canvas_w = (raw_w_px as u32).min(TILE_W);
        let canvas_h = (raw_h_px as u32).min(TILE_H);

        // World scale derived from UNCLAMPED size every frame — without
        // the comparison guard we always reflect the latest UDim2 edit
        // to Transform.scale even when only the world dimensions change
        // (e.g. UDim2 Scale went from 0 to 10 with offsets that keep
        // the canvas pixel size identical).
        let target_world = Vec3::new(
            raw_w_px / PIXELS_PER_METER,
            raw_h_px / PIXELS_PER_METER,
            1.0,
        );
        if transform.scale != target_world {
            transform.scale = target_world;
        }

        // ZIndex depth-bias lives on the same uniform as the UV bounds,
        // so we update it every time the marker changes (cheap — one
        // float write) rather than waiting on a canvas-dim change.
        let want_z_bias = marker.z_index as f32 * Z_INDEX_METRES_PER_UNIT;
        if (uv.z_bias - want_z_bias).abs() > f32::EPSILON {
            uv.z_bias = want_z_bias;
        }

        if canvas_w != handle.width || canvas_h != handle.height {
            handle.width = canvas_w;
            handle.height = canvas_h;
            handle.last_label_hash = 0; // force re-render

            let (uv_min, uv_max) = atlas.slot_uv(tile.slot);
            let effective_umax = uv_min.x + (uv_max.x - uv_min.x) * (canvas_w as f32 / TILE_W as f32);
            let effective_vmax = uv_min.y + (uv_max.y - uv_min.y) * (canvas_h as f32 / TILE_H as f32);
            // Half-texel inset — see spawn_billboard_render_state for
            // the rationale. Without this, bilinear filtering at the
            // tile boundary pulls in the neighboring tile's content
            // and leaks fragments of unrelated billboards onto the
            // quad.
            let atlas_w_px = BillboardAtlas::atlas_w_px_static() as f32;
            let atlas_h_px = atlas.atlas_h_px() as f32;
            let half_u = 0.5 / atlas_w_px;
            let half_v = 0.5 / atlas_h_px;
            uv.uv_min = Vec2::new(uv_min.x + half_u, uv_min.y + half_v);
            uv.uv_max = Vec2::new(effective_umax - half_u, effective_vmax - half_v);

            if (raw_w_px as u32) > TILE_W || (raw_h_px as u32) > TILE_H {
                warn!(
                    "🪧 billboard {:?}: canvas {}×{} px capped at tile {}×{}; world quad still {:.2}×{:.2} m",
                    entity, raw_w_px as u32, raw_h_px as u32, TILE_W, TILE_H,
                    target_world.x, target_world.y,
                );
            }
        }

        let want_depth = !marker.always_on_top;
        if depth.0 != want_depth {
            depth.0 = want_depth;
        }

        *vis = if marker.visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        // FaceCamera live toggle: presence of `BillboardLockAxis` with
        // `rotation: true` switches the WGSL pipeline to the
        // LOCK_ROTATION variant (entity's literal rotation). Add/remove
        // the component to flip behaviour without respawning.
        let want_locked = !marker.face_camera;
        let currently_locked = lock_axis.map(|l| l.rotation).unwrap_or(false);
        if want_locked && !currently_locked {
            commands.entity(entity).insert(crate::billboard_pipeline::BillboardLockAxis {
                y_axis: false,
                rotation: true,
            });
        } else if !want_locked && lock_axis.is_some() {
            commands.entity(entity).remove::<crate::billboard_pipeline::BillboardLockAxis>();
        }
    }
}

// NOTE — Roblox parity work outstanding:
//
// `ExtentsOffset` (local-space, relative to adornee bounding box) and the
// world-space offset variants (`ExtentsOffsetWorldSpace`,
// `StudsOffsetWorldSpace` aka `units_offset_world_space`) are all carried
// on `BillboardGui` and `BillboardGuiMarker` and round-trip through TOML.
// Applying them to the runtime transform requires adornee tracking — i.e.
// looking up the part the billboard is attached to, reading its
// `GlobalTransform` and bounding box, then composing the offsets in the
// right space. That tracking is its own subsystem (Roblox `Adornee` is a
// referent, we currently store the name as a string in the TOML) and
// will be added in a follow-up. Until then, `units_offset` (local-space
// relative to the entity's parent) is the single placement field that
// actually moves the quad — and it's the one users edit 99% of the time.
//
// `Brightness` and `LightInfluence` are stored on the marker but not yet
// consumed by the WGSL fragment shader. Adding them is a single uniform
// + a multiply in the shader; deferred until we have a use-case driving
// the visual change.

/// Roblox-parity distance culling. Hides the billboard when:
/// - `distance_lower_limit > 0` and camera is closer than that limit, OR
/// - `max_distance > 0` and camera is farther than that limit.
///
/// The inverted-band behaviour mirrors Roblox where `DistanceLowerLimit`
/// suppresses the player's own head label without needing a separate
/// per-player hide flag.
fn cull_billboards_by_distance(
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut billboards: Query<
        (&BillboardGuiMarker, &GlobalTransform, &mut Visibility),
        With<crate::billboard_pipeline::Billboard>,
    >,
) {
    let Some(cam) = cameras.iter().next() else { return };
    let cam_pos = cam.translation();

    for (marker, global_tf, mut vis) in &mut billboards {
        if !marker.visible {
            continue;
        }
        let dist = global_tf.translation().distance(cam_pos);

        let too_close = marker.distance_lower_limit > 0.0 && dist < marker.distance_lower_limit;
        let too_far = marker.max_distance > 0.0 && dist > marker.max_distance;

        *vis = if too_close || too_far {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
}

/// Flat record produced by the recursive subtree walk: an element along with
/// its canvas-absolute position (parent offsets accumulated) and the clip
/// rect inherited from the nearest `clip_children = true` ancestor.
#[derive(Clone)]
struct FlatElem {
    elem: GuiElementDisplay,
    abs_x: f32,
    abs_y: f32,
    /// `[x, y, w, h]` in canvas-absolute coords, or `None` for unclipped.
    clip_rect: Option<[f32; 4]>,
}

/// Walk the `ChildOf` hierarchy starting at `parent_entity`, accumulating
/// parent offsets so nested Frame > TextLabel layouts position correctly on
/// the billboard canvas. The clip rect propagates through descendants until
/// a deeper `clip_children = true` overrides it.
fn collect_subtree(
    parent_entity: Entity,
    parent_abs_x: f32,
    parent_abs_y: f32,
    parent_w: f32,
    parent_h: f32,
    clip_rect: Option<[f32; 4]>,
    all_elements: &Query<(Entity, &GuiElementDisplay, &ChildOf)>,
    out: &mut Vec<FlatElem>,
) {
    for (child_entity, disp, child_of) in all_elements.iter() {
        if child_of.parent() != parent_entity {
            continue;
        }
        // Resolve UDim2 → pixels against the parent's resolved rect.
        // Roblox semantics: `pixel = scale * parent_extent + offset`.
        // A child with `Size = UDim2(1, 0, 1, 0)` therefore fills the
        // parent exactly; `Position = UDim2(0.5, 0, 0.5, 0)` lands at
        // the parent's centre.
        let rel_x  = disp.position_udim2[0] * parent_w + disp.position_udim2[1];
        let rel_y  = disp.position_udim2[2] * parent_h + disp.position_udim2[3];
        let resolved_w = (disp.size_udim2[0] * parent_w + disp.size_udim2[1]).max(1.0);
        let resolved_h = (disp.size_udim2[2] * parent_h + disp.size_udim2[3]).max(1.0);

        // `AnchorPoint` shifts the element so that the anchor point on
        // its OWN rect lands on the resolved Position. `(0.5, 0.5)`
        // centres on Position, `(1, 1)` puts the bottom-right corner
        // on Position. Without this, every child is top-left-anchored
        // and the typical "Position = (0.5, 0, 0.5, 0)" centring
        // pattern doesn't actually centre.
        let anchor_x = disp.anchor_point[0] * resolved_w;
        let anchor_y = disp.anchor_point[1] * resolved_h;

        let abs_x = parent_abs_x + rel_x - anchor_x;
        let abs_y = parent_abs_y + rel_y - anchor_y;
        let my_clip = if disp.clip_children {
            Some([abs_x, abs_y, resolved_w, resolved_h])
        } else {
            clip_rect
        };
        // Write the resolved dimensions back into the flattened
        // element so the renderer (`render_element`, `render_text`)
        // sees the parent-aware values. The source `GuiElementDisplay`
        // on the entity keeps its Offset-only fallback for any
        // consumer that reads it without the layout pass.
        let mut elem = disp.clone();
        elem.x = rel_x;
        elem.y = rel_y;
        elem.width = resolved_w;
        elem.height = resolved_h;
        out.push(FlatElem {
            elem: elem.clone(),
            abs_x,
            abs_y,
            clip_rect: my_clip,
        });
        collect_subtree(
            child_entity, abs_x, abs_y, resolved_w, resolved_h,
            my_clip, all_elements, out,
        );
    }
}

fn update_and_render_billboards(
    mut text_state: NonSendMut<BillboardTextState>,
    mut atlas: NonSendMut<BillboardAtlas>,
    mut billboards: Query<(Entity, &mut BillboardRenderHandle, &BillboardAtlasTile)>,
    gui_elements: Query<(Entity, &GuiElementDisplay, &ChildOf)>,
    // Billboards typically host children (TextLabel, Frame, …) that
    // carry the visible content. Some callers attach a
    // `GuiElementDisplay` directly to the billboard entity (placeholder
    // background, debug label, etc.) — we include that as the root of
    // the subtree so a billboard with no children still paints.
    billboard_self_display: Query<&GuiElementDisplay>,
) {
    for (entity, mut handle, tile) in &mut billboards {
        let mut flat: Vec<FlatElem> = Vec::new();
        // Root parent rect = the billboard's resolved canvas, so a
        // child with `Size = UDim2(1, 0, 1, 0)` fills the billboard's
        // pixel canvas exactly. Without this, root children would
        // resolve `Scale` against 0 width/height and collapse to their
        // Offset alone — which is why a `(1, 0, 1, 0)` TextLabel
        // disappeared.
        let parent_w = handle.width as f32;
        let parent_h = handle.height as f32;
        // Root: the billboard's own GuiElementDisplay if it has one.
        // Walking from `entity` covers descendants; this handles the
        // childless-but-self-displaying case.
        if let Ok(self_disp) = billboard_self_display.get(entity) {
            let clip = if self_disp.clip_children {
                Some([0.0, 0.0, parent_w, parent_h])
            } else {
                None
            };
            let mut root = self_disp.clone();
            root.width = parent_w;
            root.height = parent_h;
            flat.push(FlatElem {
                elem: root,
                abs_x: 0.0,
                abs_y: 0.0,
                clip_rect: clip,
            });
            collect_subtree(entity, 0.0, 0.0, parent_w, parent_h, clip, &gui_elements, &mut flat);
        } else {
            collect_subtree(entity, 0.0, 0.0, parent_w, parent_h, None, &gui_elements, &mut flat);
        }
        flat.sort_by_key(|f| f.elem.z_order);

        let hash = label_hash(&flat);
        if hash == handle.last_label_hash {
            continue;
        }
        handle.last_label_hash = hash;

        // Render this billboard into a temporary tile-sized pixmap, then
        // blit into the shared atlas at the entity's slot offset.
        let Some(mut pixmap) = Pixmap::new(TILE_W, TILE_H) else { continue };
        // Pixmap starts zeroed so previous tile contents don't leak through.

        for f in &flat {
            if !f.elem.visible {
                continue;
            }
            render_element(&mut pixmap, &f.elem, f.abs_x, f.abs_y, f.clip_rect, &mut text_state);
        }

        let atlas_w_px = BillboardAtlas::atlas_w_px_static();
        blit_tile_into_atlas(&mut atlas.cpu_buf, &pixmap, tile.slot, atlas_w_px);
        atlas.dirty = true;
        let _ = entity;
    }
}

/// Copy the rendered tile pixmap into the atlas CPU buffer at the slot's
/// pixel offset. Both buffers are RGBA8 row-major.
fn blit_tile_into_atlas(cpu_buf: &mut [u8], tile_pixmap: &Pixmap, slot: u32, atlas_w_px: u32) {
    let col = slot % ATLAS_COLS;
    let row = slot / ATLAS_COLS;
    let ox = (col * TILE_W) as usize;
    let oy = (row * TILE_H) as usize;
    let atlas_stride = (atlas_w_px as usize) * 4;
    let tile_stride = (TILE_W as usize) * 4;
    let src = tile_pixmap.data();

    for r in 0..TILE_H as usize {
        let src_start = r * tile_stride;
        let dst_start = (oy + r) * atlas_stride + ox * 4;
        cpu_buf[dst_start..dst_start + tile_stride]
            .copy_from_slice(&src[src_start..src_start + tile_stride]);
    }
}

/// After all dirty tiles are blitted, copy the atlas CPU buffer into the
/// atlas `Image` asset once. `images.get_mut` returns `Mut<Image>` which
/// triggers Bevy's render-asset extraction → one GPU upload covering every
/// dirty tile.
fn upload_atlas_to_gpu(
    mut atlas: NonSendMut<BillboardAtlas>,
    mut images: ResMut<Assets<Image>>,
) {
    if !atlas.dirty {
        return;
    }
    let Some(image) = images.get_mut(&atlas.texture) else { return };
    let Some(data) = image.data.as_mut() else { return };
    if data.len() != atlas.cpu_buf.len() {
        data.resize(atlas.cpu_buf.len(), 0);
    }
    data.copy_from_slice(&atlas.cpu_buf);
    atlas.dirty = false;
}

// ── Renderer ───────────────────────────────────────────────────────────────

/// Convert a clip rect (canvas-absolute `[x, y, w, h]`) into a tiny-skia
/// `Mask` matching the pixmap dimensions. tiny-skia's `fill_path` /
/// `stroke_path` accept an optional `&Mask` to clip drawing — we build one
/// rectangle mask per element when clipping is needed.
fn build_clip_mask(pixmap_w: u32, pixmap_h: u32, clip: [f32; 4]) -> Option<tiny_skia::Mask> {
    let mut mask = tiny_skia::Mask::new(pixmap_w, pixmap_h)?;
    let rect = Rect::from_xywh(clip[0], clip[1], clip[2].max(0.0), clip[3].max(0.0))?;
    let path = PathBuilder::from_rect(rect);
    mask.fill_path(&path, tiny_skia::FillRule::Winding, true, TsTransform::identity());
    Some(mask)
}

fn render_element(
    pixmap: &mut Pixmap,
    elem: &GuiElementDisplay,
    abs_x: f32,
    abs_y: f32,
    clip_rect: Option<[f32; 4]>,
    text_state: &mut BillboardTextState,
) {
    let x = abs_x;
    let y = abs_y;
    let w = elem.width.max(1.0);
    let h = elem.height.max(1.0);
    let r = elem.corner_radius;

    let mask = clip_rect.and_then(|c| build_clip_mask(pixmap.width(), pixmap.height(), c));
    let mask_ref = mask.as_ref();

    // Background fill
    if elem.bg_color[3] > 0.0 {
        let mut paint = Paint::default();
        paint.set_color_rgba8(
            (elem.bg_color[0] * 255.0) as u8,
            (elem.bg_color[1] * 255.0) as u8,
            (elem.bg_color[2] * 255.0) as u8,
            (elem.bg_color[3] * 255.0) as u8,
        );
        paint.anti_alias = true;

        let path = rounded_rect_path(x, y, w, h, r);
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, TsTransform::identity(), mask_ref);
    }

    // Border stroke
    if elem.border_size > 0.0 && elem.border_color[3] > 0.0 {
        let mut paint = Paint::default();
        paint.set_color_rgba8(
            (elem.border_color[0] * 255.0) as u8,
            (elem.border_color[1] * 255.0) as u8,
            (elem.border_color[2] * 255.0) as u8,
            (elem.border_color[3] * 255.0) as u8,
        );
        paint.anti_alias = true;
        let mut stroke = Stroke::default();
        stroke.width = elem.border_size;

        let path = rounded_rect_path(x, y, w, h, r);
        pixmap.stroke_path(&path, &paint, &stroke, TsTransform::identity(), mask_ref);
    }

    // ImageLabel / ImageButton — placeholder rendering. Bevy `Image` assets
    // referenced by `image_path` aren't readily accessible from CPU without
    // a copy-back path; for Phase A we draw a tinted placeholder rect so
    // the slot is visible. Wiring real image loading is a follow-up.
    if (elem.class_type.eq_ignore_ascii_case("imagelabel")
        || elem.class_type.eq_ignore_ascii_case("imagebutton"))
        && !elem.image_path.is_empty() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(80, 80, 100, 200);
        paint.anti_alias = true;
        let path = rounded_rect_path(x, y, w, h, r.max(2.0));
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, TsTransform::identity(), mask_ref);

        let mut stroke_paint = Paint::default();
        stroke_paint.set_color_rgba8(140, 140, 180, 255);
        stroke_paint.anti_alias = true;
        let mut stroke = Stroke::default();
        stroke.width = 1.5;
        pixmap.stroke_path(&path, &stroke_paint, &stroke, TsTransform::identity(), mask_ref);
    }

    // Text
    if !elem.text.is_empty() && elem.text_color[3] > 0.0 {
        render_text(pixmap, elem, abs_x, abs_y, clip_rect, text_state);
    }
}

fn render_text(
    pixmap: &mut Pixmap,
    elem: &GuiElementDisplay,
    abs_x: f32,
    abs_y: f32,
    clip_rect: Option<[f32; 4]>,
    text_state: &mut BillboardTextState,
) {
    // Resolve font size — either the user-specified `font_size` or, when
    // `TextScaled` is on, the largest size that fits inside the
    // element's rect via binary-search. The search shape-tests at each
    // candidate size; ~6 iterations get to ±1 px in the [8, 72] band.
    let font_size = if elem.text_scaled && !elem.text.is_empty()
        && elem.width > 0.0 && elem.height > 0.0
    {
        let weight = cosmic_text::Weight(elem.font_weight.clamp(100, 900) as u16);
        let attrs = Attrs::new().family(Family::SansSerif).weight(weight);
        // Shape the text at `candidate_size` and return whether the
        // resulting layout fits inside `(elem.width, elem.height)`.
        // Uses 1.4× line-height matching the body renderer so the test
        // matches what actually ships to the pixmap.
        let mut fits_at = |candidate_size: f32| -> bool {
            let metrics = Metrics::new(candidate_size, candidate_size * 1.4);
            let mut buf = TextBuffer::new(&mut text_state.font_system, metrics);
            // Bound WIDTH (so wrapping happens) but leave HEIGHT UNBOUNDED so
            // every wrapped line is counted. Passing Some(height) made
            // cosmic-text limit `layout_runs` to the lines that fit inside the
            // band, so a multi-line label (e.g. "AI Workshop" wrapping to two
            // lines) UNDER-reported its height → `fits_at` returned true for an
            // oversized font → the text rendered too big and the overflow line
            // was clipped ("AI Workshop" showed as "AI"). With `None` we sum
            // the TRUE total height of all wrapped lines and shrink the font
            // until the whole label genuinely fits the band height.
            buf.set_size(
                &mut text_state.font_system,
                Some(elem.width),
                None,
            );
            buf.set_wrap(&mut text_state.font_system, cosmic_text::Wrap::Word);
            buf.set_text(
                &mut text_state.font_system,
                &elem.text,
                attrs.clone(),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(&mut text_state.font_system, false);
            // Sum total run height; reject if any line exceeds element
            // width or stack overflows element height.
            let mut total_h = 0.0_f32;
            let mut max_w = 0.0_f32;
            for run in buf.layout_runs() {
                total_h += run.line_height;
                max_w = max_w.max(run.line_w);
            }
            total_h <= elem.height && max_w <= elem.width
        };

        // Binary search in [1, 72]. 1 px is the lower readability floor;
        // 72 is the canonical FontSize cap from the Properties panel —
        // TextScaled honours it so users can't get a label that ignores
        // their own clamp.
        let mut lo: f32 = 1.0;
        let mut hi: f32 = 72.0;
        let max_iters = 8;
        for _ in 0..max_iters {
            let mid = (lo + hi) * 0.5;
            if fits_at(mid) { lo = mid; } else { hi = mid; }
            if hi - lo < 0.5 { break; }
        }
        lo.max(1.0)
    } else {
        elem.font_size.max(8.0)
    };

    // 1.4x line-height: tighter than typical 1.5 but more readable than the
    // 1.2 cosmic-text default at small sizes.
    let metrics = Metrics::new(font_size, font_size * 1.4);

    let mut buffer = TextBuffer::new(&mut text_state.font_system, metrics);
    buffer.set_size(
        &mut text_state.font_system,
        Some(elem.width),
        Some(elem.height),
    );
    // Explicit word-wrap. Default depends on cosmic-text version — pin
    // it so long text on a narrow canvas (e.g. a mindmap node label)
    // breaks onto the next line instead of overflowing horizontally.
    buffer.set_wrap(&mut text_state.font_system, cosmic_text::Wrap::Word);

    let weight = cosmic_text::Weight(elem.font_weight.clamp(100, 900) as u16);
    let attrs = Attrs::new().family(Family::SansSerif).weight(weight);
    buffer.set_text(
        &mut text_state.font_system,
        &elem.text,
        attrs,
        Shaping::Advanced,
    );

    // Apply text alignment per line. cosmic-text's per-line `set_align`
    // governs where shaped runs are positioned within the buffer width
    // (`set_size` above bounds it to `elem.width`).
    // PascalCase canonical (`Left` / `Center` / `Right`), case-insensitive
    // match so legacy lowercase TOMLs still pick the right alignment.
    let alignment = match elem.text_align.to_ascii_lowercase().as_str() {
        "left" => Some(cosmic_text::Align::Left),
        "right" => Some(cosmic_text::Align::Right),
        _ => Some(cosmic_text::Align::Center),
    };
    for line in buffer.lines.iter_mut() {
        line.set_align(alignment);
    }

    buffer.shape_until_scroll(&mut text_state.font_system, false);

    let text_color = CosmicColor::rgba(
        (elem.text_color[0] * 255.0) as u8,
        (elem.text_color[1] * 255.0) as u8,
        (elem.text_color[2] * 255.0) as u8,
        (elem.text_color[3] * 255.0) as u8,
    );

    // Vertical text alignment. cosmic-text's own buffer.set_size bounds
    // horizontal wrap but doesn't center the rendered text block within
    // the bounds — we measure the total run height post-shape and shift
    // `origin_y` so Top / Center / Bottom land where the user expects.
    // Empty text or zero-height canvases pass through unchanged.
    let total_text_h: f32 = buffer
        .layout_runs()
        .map(|run| run.line_height)
        .sum();
    let y_align_lower = elem.text_y_align.to_ascii_lowercase();
    let y_align_offset = if total_text_h <= 0.0 || elem.height <= 0.0 {
        0.0
    } else {
        match y_align_lower.as_str() {
            "top" => 0.0,
            "bottom" => (elem.height - total_text_h).max(0.0),
            // Default + "center": centred vertically.
            _ => ((elem.height - total_text_h) * 0.5).max(0.0),
        }
    };

    let origin_x = abs_x as i32;
    let origin_y = (abs_y + y_align_offset) as i32;

    // Text-stroke halo. Roblox `TextStrokeColor3` + `TextStrokeTransparency`
    // draws a 1-px outline around glyphs so labels stay legible against
    // any 3D background. Implemented as an 8-direction offset pass —
    // glyph bitmaps redrawn at (±1, ±1) + cardinal offsets with the
    // stroke colour, before the body text overlays on top. Skipped when
    // stroke alpha is 0 to keep the typical no-stroke path fast.
    let stroke_a = (elem.text_stroke_color[3] * 255.0) as u8;
    if stroke_a > 0 {
        let stroke_color = CosmicColor::rgba(
            (elem.text_stroke_color[0] * 255.0) as u8,
            (elem.text_stroke_color[1] * 255.0) as u8,
            (elem.text_stroke_color[2] * 255.0) as u8,
            stroke_a,
        );
        const HALO_OFFSETS: [(i32, i32); 8] = [
            (-1, -1), (0, -1), (1, -1),
            (-1,  0),          (1,  0),
            (-1,  1), (0,  1), (1,  1),
        ];
        for (ox, oy) in HALO_OFFSETS {
            let ox_origin = origin_x + ox;
            let oy_origin = origin_y + oy;
            buffer.draw(
                &mut text_state.font_system,
                &mut text_state.swash_cache,
                stroke_color,
                |px, py, w, h, color| {
                    let px = ox_origin + px;
                    let py = oy_origin + py;
                    for dy in 0..h as i32 {
                        for dx in 0..w as i32 {
                            let sx = px + dx;
                            let sy = py + dy;
                            if sx < 0 || sy < 0 || sx >= pixmap.width() as i32 || sy >= pixmap.height() as i32 {
                                continue;
                            }
                            if let Some((cx0, cy0, cx1, cy1)) = clip_rect.map(|c| {
                                (c[0].floor() as i32, c[1].floor() as i32,
                                 (c[0] + c[2]).ceil() as i32, (c[1] + c[3]).ceil() as i32)
                            }) {
                                if sx < cx0 || sy < cy0 || sx >= cx1 || sy >= cy1 {
                                    continue;
                                }
                            }
                            let a = color.a();
                            if a == 0 { continue; }
                            let idx = (sy as usize * pixmap.width() as usize + sx as usize) * 4;
                            let buf = pixmap.data_mut();
                            let src_a = a as u32;
                            let inv = 255 - src_a;
                            buf[idx]     = ((color.r() as u32 * src_a + buf[idx]     as u32 * inv) / 255) as u8;
                            buf[idx + 1] = ((color.g() as u32 * src_a + buf[idx + 1] as u32 * inv) / 255) as u8;
                            buf[idx + 2] = ((color.b() as u32 * src_a + buf[idx + 2] as u32 * inv) / 255) as u8;
                            buf[idx + 3] = (src_a + buf[idx + 3] as u32 * inv / 255) as u8;
                        }
                    }
                },
            );
        }
    }
    let clip_px = clip_rect.map(|c| {
        let x0 = c[0].floor() as i32;
        let y0 = c[1].floor() as i32;
        let x1 = (c[0] + c[2]).ceil() as i32;
        let y1 = (c[1] + c[3]).ceil() as i32;
        (x0, y0, x1, y1)
    });

    buffer.draw(
        &mut text_state.font_system,
        &mut text_state.swash_cache,
        text_color,
        |px, py, w, h, color| {
            let px = origin_x + px;
            let py = origin_y + py;
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    let sx = px + dx;
                    let sy = py + dy;
                    if sx < 0 || sy < 0 || sx >= pixmap.width() as i32 || sy >= pixmap.height() as i32 {
                        continue;
                    }
                    if let Some((cx0, cy0, cx1, cy1)) = clip_px {
                        if sx < cx0 || sy < cy0 || sx >= cx1 || sy >= cy1 {
                            continue;
                        }
                    }
                    let a = color.a();
                    if a == 0 { continue; }
                    let idx = (sy as usize * pixmap.width() as usize + sx as usize) * 4;
                    let buf = pixmap.data_mut();
                    let src_a = a as u32;
                    let inv = 255 - src_a;
                    buf[idx]     = ((color.r() as u32 * src_a + buf[idx]     as u32 * inv) / 255) as u8;
                    buf[idx + 1] = ((color.g() as u32 * src_a + buf[idx + 1] as u32 * inv) / 255) as u8;
                    buf[idx + 2] = ((color.b() as u32 * src_a + buf[idx + 2] as u32 * inv) / 255) as u8;
                    buf[idx + 3] = (src_a + buf[idx + 3] as u32 * inv / 255) as u8;
                }
            }
        },
    );
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    if r < 0.5 {
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
    } else {
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
    }
    pb.finish().unwrap_or_else(|| PathBuilder::from_rect(
        Rect::from_xywh(x, y, w, h).unwrap()
    ))
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn meters_from_pixels(size_px: [f32; 2]) -> (f32, f32) {
    (size_px[0] / PIXELS_PER_METER, size_px[1] / PIXELS_PER_METER)
}

fn create_atlas_image(images: &mut Assets<Image>, rows: u32) -> Handle<Image> {
    let width = BillboardAtlas::atlas_w_px_static();
    let height = BillboardAtlas::atlas_h_px_for_rows(rows);
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let total_bytes = (width as usize) * (height as usize) * 4;
    let mut image = Image {
        // Explicit data buffer so `image.data.as_mut()` returns Some(_) on
        // first upload — `Image::resize` doesn't always allocate the data
        // vec depending on the construction path.
        data: Some(vec![0u8; total_bytes]),
        texture_descriptor: TextureDescriptor {
            label: Some("BillboardAtlas"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        // Atlas sampler MUST use ClampToEdge addressing, NOT Bevy's
        // default Repeat. Repeat wraps UVs > 1.0 around the whole
        // texture, which means a bilinear-filter sample near the right
        // edge of one tile picks up content from the FAR LEFT tile of
        // the atlas — the "stuck billboard artifacts" the user reported
        // 2026-05-12 (small bits of one billboard's text appearing
        // floating near unrelated parts). Combined with the half-texel
        // UV inset in `spawn_billboard_render_state`, this fully
        // contains each tile's samples to its own atlas region.
        //
        // Filtering: start from `linear()` so text edges stay smooth,
        // then override the address modes. `linear()` returns the
        // descriptor with mag/min set to Linear and mipmap Nearest —
        // the exact filter set the WGSL fragment shader wants.
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            label: Some("BillboardAtlasSampler".into()),
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            address_mode_w: ImageAddressMode::ClampToEdge,
            ..ImageSamplerDescriptor::linear()
        }),
        ..default()
    };
    image.resize(size);
    images.add(image)
}

fn label_hash(flat: &[FlatElem]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    flat.len().hash(&mut h);
    for f in flat {
        let e = &f.elem;
        e.text.hash(&mut h);
        f.abs_x.to_bits().hash(&mut h);
        f.abs_y.to_bits().hash(&mut h);
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
        e.text_y_align.hash(&mut h);
        for s in &e.text_stroke_color { s.to_bits().hash(&mut h); }
        for a in &e.anchor_point { a.to_bits().hash(&mut h); }
        e.text_scaled.hash(&mut h);
        e.corner_radius.to_bits().hash(&mut h);
        e.border_size.to_bits().hash(&mut h);
        e.image_path.hash(&mut h);
        e.class_type.hash(&mut h);
        if let Some(c) = f.clip_rect {
            for v in &c {
                v.to_bits().hash(&mut h);
            }
        } else {
            u64::MAX.hash(&mut h);
        }
    }
    h.finish()
}

// ── Plugin ─────────────────────────────────────────────────────────────────

// ── Class-component → GuiElementDisplay sync ──────────────────────────────
//
// The renderer reads `GuiElementDisplay` (the cached "what to paint" view).
// Properties-panel edits land on the source class component (TextLabel /
// Frame / TextButton / …) — we need to copy those changes into the
// display cache or the atlas tile would never re-render with the new
// content. Each system below watches `Changed<TheClass>` and mirrors the
// visible-state fields. `update_and_render_billboards` hash-checks the
// display fields, so any actual change automatically triggers a tile
// re-render on the next frame.

fn sync_textlabel_to_display(
    mut q: Query<(&eustress_common::classes::TextLabel, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::TextLabel>>,
) {
    use eustress_common::classes::{Font, TextXAlignment, TextYAlignment};
    for (tl, mut gui) in &mut q {
        gui.text = tl.text.clone();
        gui.text_color = [tl.text_color3[0], tl.text_color3[1], tl.text_color3[2],
                          (1.0 - tl.text_transparency).clamp(0.0, 1.0)];
        gui.font_size = tl.font_size.max(1.0);
        gui.font_weight = match tl.font {
            Font::GothamBold => 700,
            Font::GothamLight => 300,
            _ => 400,
        };
        gui.text_align = match tl.text_x_alignment {
            TextXAlignment::Left => "Left".to_string(),
            TextXAlignment::Center => "Center".to_string(),
            TextXAlignment::Right => "Right".to_string(),
        };
        gui.text_y_align = match tl.text_y_alignment {
            TextYAlignment::Top => "Top".to_string(),
            TextYAlignment::Center => "Center".to_string(),
            TextYAlignment::Bottom => "Bottom".to_string(),
        };
        // Text stroke (halo) — alpha derived from transparency. Zero
        // alpha = renderer skips the stroke pass entirely.
        gui.text_stroke_color = [
            tl.text_stroke_color3[0],
            tl.text_stroke_color3[1],
            tl.text_stroke_color3[2],
            (1.0 - tl.text_stroke_transparency).clamp(0.0, 1.0),
        ];
        gui.text_scaled = tl.text_scaled;
        gui.bg_color = [tl.background_color3[0], tl.background_color3[1], tl.background_color3[2],
                        (1.0 - tl.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [tl.border_color3[0], tl.border_color3[1], tl.border_color3[2], 1.0];
        gui.border_size = tl.border_size_pixel as f32;
        gui.visible = tl.visible;
        gui.z_order = tl.z_index;
        gui.anchor_point = tl.anchor_point;
        // Store BOTH the source UDim2 AND a best-effort Offset-only
        // resolved rect. `collect_subtree` re-resolves Scale at render
        // time using the parent billboard's canvas size — that's where
        // `Size = (1, 0, 1, 0)` becomes "fill the parent". The Offset
        // here keeps non-parented previews looking sane.
        gui.position_udim2 = [
            tl.position.x.scale, tl.position.x.offset,
            tl.position.y.scale, tl.position.y.offset,
        ];
        gui.size_udim2 = [
            tl.size.x.scale, tl.size.x.offset,
            tl.size.y.scale, tl.size.y.offset,
        ];
        gui.x = tl.position.x.offset;
        gui.y = tl.position.y.offset;
        gui.width = tl.size.x.offset.max(1.0);
        gui.height = tl.size.y.offset.max(1.0);
    }
}

fn sync_frame_to_display(
    mut q: Query<(&eustress_common::classes::Frame, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::Frame>>,
) {
    for (f, mut gui) in &mut q {
        gui.bg_color = [f.background_color3[0], f.background_color3[1], f.background_color3[2],
                        (1.0 - f.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [f.border_color3[0], f.border_color3[1], f.border_color3[2], 1.0];
        gui.border_size = f.border_size_pixel as f32;
        gui.visible = f.visible;
        gui.z_order = f.z_index;
        gui.clip_children = f.clips_descendants;
        gui.anchor_point = f.anchor_point;
        gui.position_udim2 = [
            f.position.x.scale, f.position.x.offset,
            f.position.y.scale, f.position.y.offset,
        ];
        gui.size_udim2 = [
            f.size.x.scale, f.size.x.offset,
            f.size.y.scale, f.size.y.offset,
        ];
        gui.x = f.position.x.offset;
        gui.y = f.position.y.offset;
        gui.width = f.size.x.offset.max(1.0);
        gui.height = f.size.y.offset.max(1.0);
    }
}

fn sync_textbutton_to_display(
    mut q: Query<(&eustress_common::classes::TextButton, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::TextButton>>,
) {
    use eustress_common::classes::TextXAlignment;
    for (b, mut gui) in &mut q {
        gui.text = b.text.clone();
        gui.text_color = [b.text_color3[0], b.text_color3[1], b.text_color3[2],
                          (1.0 - b.text_transparency).clamp(0.0, 1.0)];
        gui.font_size = b.font_size.max(1.0);
        // TextButton has no `font` family field — default to regular
        // weight (400). Users get weighted variants through TextLabel
        // siblings or via font_family overrides at the TOML layer.
        gui.font_weight = 400;
        gui.text_align = match b.text_x_alignment {
            TextXAlignment::Left => "Left".to_string(),
            TextXAlignment::Center => "Center".to_string(),
            TextXAlignment::Right => "Right".to_string(),
        };
        gui.text_y_align = "Center".to_string();
        gui.bg_color = [b.background_color3[0], b.background_color3[1], b.background_color3[2],
                        (1.0 - b.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [b.border_color3[0], b.border_color3[1], b.border_color3[2], 1.0];
        gui.border_size = b.border_size_pixel as f32;
        gui.visible = b.visible;
        gui.z_order = b.z_index;
        gui.anchor_point = b.anchor_point;
        gui.position_udim2 = [
            b.position.x.scale, b.position.x.offset,
            b.position.y.scale, b.position.y.offset,
        ];
        gui.size_udim2 = [
            b.size.x.scale, b.size.x.offset,
            b.size.y.scale, b.size.y.offset,
        ];
        gui.x = b.position.x.offset;
        gui.y = b.position.y.offset;
        gui.width = b.size.x.offset.max(1.0);
        gui.height = b.size.y.offset.max(1.0);
    }
}

// ── TOML persistence (one save-on-change system per UI class) ─────────────
//
// Each UI class component is the authoritative state — `Changed<T>` fires
// whenever the Properties panel, a script, MCP, or hot-reload mutates it.
// These systems write the corresponding GuiTomlFile back to disk so the
// next session sees the change. Skips:
//
// - `Added<T>` — initial spawn fires Changed for every freshly inserted
//   component. Without this skip every loaded scene would queue thousands
//   of write-amplification round-trips on the first frame.
// - `Without<BeingDragged>` — defers writes during gizmo manipulation.
//   The gizmo's own release branch writes the final transform; we don't
//   need a duplicate write here. (UI elements aren't gizmo-dragged today,
//   but the marker is harmless to filter for.)
// - `recently_written` — if this process just touched the TOML, skip
//   another write to prevent the file-watcher reload loop.

/// Helper: pour a `BillboardGui` class component into an existing
/// `GuiTomlFile`. Preserves the existing `text` / `asset` / `transform`
/// / `properties` / `tags` sections (those aren't BillboardGui state).
fn apply_billboard_gui_to_toml(
    class: &BillboardGui,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    use eustress_common::classes::ZIndexBehavior;
    // Stage-4 disk normalisation: read the file's authored unit (from
    // `[metadata].unit`) and convert engine-native length-typed fields
    // back to that unit before writing. `extents_offset*` is a part-
    // size multiplier (ratio, not length) and so passes through
    // unconverted, matching the load-side rule.
    let authored = toml.metadata.unit.as_deref()
        .and_then(eustress_common::units::Unit::from_symbol)
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
    let to_authored_vec3 = |v: [f32; 3]| eustress_common::units::engine_to_authored_vec3_f32(v, authored);
    let to_authored_f32  = |v: f32|       eustress_common::units::engine_to_authored_f32(v, authored);

    toml.gui.size = class.size;
    toml.gui.size_offset = Some(class.size_offset);
    toml.gui.active = Some(class.active);
    toml.gui.enabled = Some(class.enabled);
    toml.gui.always_on_top = Some(class.always_on_top);
    toml.gui.clips_descendants = Some(class.clips_descendants);
    toml.gui.reset_on_spawn = Some(class.reset_on_spawn);
    toml.gui.stiffness_by_distance = Some(class.stiffness_by_distance);
    toml.gui.max_distance = Some(to_authored_f32(class.max_distance));
    toml.gui.distance_lower_limit = Some(to_authored_f32(class.distance_lower_limit));
    toml.gui.distance_upper_limit = Some(to_authored_f32(class.distance_upper_limit));
    toml.gui.distance_step = Some(to_authored_f32(class.distance_step));
    toml.gui.brightness = Some(class.brightness);
    toml.gui.light_influence = Some(class.light_influence);
    toml.gui.extents_offset = Some(class.extents_offset);
    toml.gui.extents_offset_world_space = Some(class.extents_offset_world_space);
    toml.gui.units_offset = Some(to_authored_vec3(class.units_offset));
    toml.gui.units_offset_world_space = Some(to_authored_vec3(class.units_offset_world_space));
    // ZIndex on BillboardGui is a depth bias (per-billboard, integer),
    // not the GuiObject sort-order ZIndex used by Frame/TextLabel. Map
    // it into the same TOML `z_index` slot — there's no ambiguity per
    // file since each `_instance.toml` belongs to exactly one class.
    toml.gui.z_index = class.z_index;
    toml.gui.z_index_behavior = Some(match class.z_index_behavior {
        ZIndexBehavior::Global => "Global".to_string(),
        ZIndexBehavior::Sibling => "Sibling".to_string(),
    });
}

fn save_billboard_gui_changes(
    q: Query<
        (Entity, &BillboardGui, &crate::space::instance_loader::InstanceFile),
        (
            Changed<BillboardGui>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<BillboardGui>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    // Synchronous TOML I/O. We tried background-thread writes; they
    // introduce a copy-paste race — copy_dir_recursive reads disk, so
    // if the user edits a property then immediately Ctrl+C the source
    // TOML is still mid-flight, and the duplicate inherits the
    // pre-edit content. Inline writes keep the on-disk state in lock-
    // step with the ECS at the cost of a few ms per discrete user
    // commit, which is below the perception threshold.
    //
    // The `was_recently_written` SKIP-on-save check stays removed —
    // it dropped rapid edits because Bevy's `Changed<T>` resets the
    // moment this system iterates. We only `mark_written` AFTER the
    // write to break the save → file-watcher → save loop.
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t,
            Err(e) => {
                debug!("🪧 save_billboard_gui: skip {} ({})", inst_file.toml_path.display(), e);
                continue;
            }
        };
        apply_billboard_gui_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_billboard_gui: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

/// Mirror Roblox-parity TextLabel state into both the `gui` and `text`
/// sections of `GuiTomlFile`. `text` holds the text-specific subset
/// (string, color, font, alignment); `gui` holds the layout subset
/// (size/position UDim2, anchor, visibility, z_index).
fn apply_text_label_to_toml(
    class: &eustress_common::classes::TextLabel,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    use eustress_common::classes::{Font, TextXAlignment, TextYAlignment};
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;

    let font_name = match class.font {
        Font::GothamBold => "GothamBold",
        Font::GothamLight => "GothamLight",
        Font::RobotoMono => "RobotoMono",
        Font::Bangers => "Bangers",
        Font::Fantasy => "Fantasy",
        Font::Merriweather => "Merriweather",
        Font::Nunito => "Nunito",
        Font::Ubuntu => "Ubuntu",
        _ => "SourceSans",
    };
    let x_align = match class.text_x_alignment {
        TextXAlignment::Left => "Left",
        TextXAlignment::Center => "Center",
        TextXAlignment::Right => "Right",
    };
    let y_align = match class.text_y_alignment {
        TextYAlignment::Top => "Top",
        TextYAlignment::Center => "Center",
        TextYAlignment::Bottom => "Bottom",
    };
    toml.text = Some(crate::space::gui_loader::GuiTomlText {
        text: class.text.clone(),
        text_color: [
            class.text_color3[0],
            class.text_color3[1],
            class.text_color3[2],
            (1.0 - class.text_transparency).clamp(0.0, 1.0),
        ],
        font_size: class.font_size,
        font_family: String::new(),
        font: font_name.to_string(),
        text_x_alignment: x_align.to_string(),
        text_y_alignment: y_align.to_string(),
        text_scaled: class.text_scaled,
    });
}

fn save_text_label_changes(
    q: Query<
        (Entity, &eustress_common::classes::TextLabel, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::TextLabel>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::TextLabel>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    // Synchronous I/O — see save_billboard_gui_changes for the rationale.
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t,
            Err(e) => {
                debug!("🪧 save_text_label: skip {} ({})", inst_file.toml_path.display(), e);
                continue;
            }
        };
        apply_text_label_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_text_label: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        info!("💾 save_text_label: text={:?} font_size={} z_index={} → {}",
            class.text, class.font_size, class.z_index, inst_file.toml_path.display());
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn apply_frame_to_toml(
    class: &eustress_common::classes::Frame,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;
    toml.gui.clips_descendants = Some(class.clips_descendants);
}

fn save_frame_changes(
    q: Query<
        (Entity, &eustress_common::classes::Frame, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::Frame>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::Frame>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t, Err(_) => continue,
        };
        apply_frame_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_frame: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn apply_text_button_to_toml(
    class: &eustress_common::classes::TextButton,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    use eustress_common::classes::TextXAlignment;
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;
    let x_align = match class.text_x_alignment {
        TextXAlignment::Left => "Left",
        TextXAlignment::Center => "Center",
        TextXAlignment::Right => "Right",
    };
    toml.text = Some(crate::space::gui_loader::GuiTomlText {
        text: class.text.clone(),
        text_color: [
            class.text_color3[0],
            class.text_color3[1],
            class.text_color3[2],
            (1.0 - class.text_transparency).clamp(0.0, 1.0),
        ],
        font_size: class.font_size,
        font_family: String::new(),
        font: String::new(),
        text_x_alignment: x_align.to_string(),
        text_y_alignment: "Center".to_string(),
        text_scaled: false,
    });
}

fn save_text_button_changes(
    q: Query<
        (Entity, &eustress_common::classes::TextButton, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::TextButton>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::TextButton>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t, Err(_) => continue,
        };
        apply_text_button_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_text_button: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn apply_text_box_to_toml(
    class: &eustress_common::classes::TextBox,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;
    // TextBox uses placeholder_text when empty — round-trip the
    // user-facing text either way.
    toml.text = Some(crate::space::gui_loader::GuiTomlText {
        text: if class.text.is_empty() { class.placeholder_text.clone() } else { class.text.clone() },
        text_color: [
            class.text_color3[0],
            class.text_color3[1],
            class.text_color3[2],
            (1.0 - class.text_transparency).clamp(0.0, 1.0),
        ],
        font_size: class.font_size,
        font_family: String::new(),
        font: String::new(),
        text_x_alignment: "Left".to_string(),
        text_y_alignment: "Center".to_string(),
        text_scaled: false,
    });
}

fn save_text_box_changes(
    q: Query<
        (Entity, &eustress_common::classes::TextBox, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::TextBox>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::TextBox>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t, Err(_) => continue,
        };
        apply_text_box_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_text_box: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn sync_textbox_to_display(
    mut q: Query<(&eustress_common::classes::TextBox, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::TextBox>>,
) {
    for (tb, mut gui) in &mut q {
        // Show placeholder when text is empty (Roblox behaviour).
        // Use the placeholder colour when the placeholder is showing so
        // empty TextBoxes read as a hint, not a real value.
        let showing_placeholder = tb.text.is_empty();
        gui.text = if showing_placeholder { tb.placeholder_text.clone() } else { tb.text.clone() };
        let text_rgb = if showing_placeholder {
            tb.placeholder_color3
        } else {
            tb.text_color3
        };
        gui.text_color = [text_rgb[0], text_rgb[1], text_rgb[2],
                          (1.0 - tb.text_transparency).clamp(0.0, 1.0)];
        gui.font_size = tb.font_size.max(1.0);
        // TextBox has no `font` or `text_x_alignment` field — use sane
        // defaults (regular weight, left-aligned text).
        gui.font_weight = 400;
        gui.text_align = "Left".to_string();
        gui.text_y_align = "Center".to_string();
        gui.bg_color = [tb.background_color3[0], tb.background_color3[1], tb.background_color3[2],
                        (1.0 - tb.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [tb.border_color3[0], tb.border_color3[1], tb.border_color3[2], 1.0];
        gui.border_size = tb.border_size_pixel as f32;
        gui.visible = tb.visible;
        gui.z_order = tb.z_index;
        gui.anchor_point = tb.anchor_point;
        gui.position_udim2 = [
            tb.position.x.scale, tb.position.x.offset,
            tb.position.y.scale, tb.position.y.offset,
        ];
        gui.size_udim2 = [
            tb.size.x.scale, tb.size.x.offset,
            tb.size.y.scale, tb.size.y.offset,
        ];
        gui.x = tb.position.x.offset;
        gui.y = tb.position.y.offset;
        gui.width = tb.size.x.offset.max(1.0);
        gui.height = tb.size.y.offset.max(1.0);
    }
}

pub struct BillboardGuiPlugin;

impl Plugin for BillboardGuiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(BillboardTextState::default())
            .init_resource::<BillboardEditState>()
            .add_systems(Startup, init_billboard_atlas)
            .add_systems(
                Update,
                (
                    // Defensive: any BillboardGui without a marker gets
                    // one before downstream systems run.
                    ensure_billboard_marker,
                    sync_billboard_class_to_marker.after(ensure_billboard_marker),
                    spawn_billboard_render_state.after(sync_billboard_class_to_marker),
                    sync_billboard_properties.after(spawn_billboard_render_state),
                    // Mirror UI-class property edits into the renderer's
                    // GuiElementDisplay cache so changes show up live.
                    sync_textlabel_to_display.after(sync_billboard_properties),
                    sync_frame_to_display.after(sync_billboard_properties),
                    sync_textbutton_to_display.after(sync_billboard_properties),
                    sync_textbox_to_display.after(sync_billboard_properties),
                    cull_billboards_by_distance.after(sync_textlabel_to_display),
                    release_atlas_slots.after(cull_billboards_by_distance),
                    update_and_render_billboards.after(release_atlas_slots),
                    upload_atlas_to_gpu.after(update_and_render_billboards),
                ),
            )
            // ── In-viewport TextLabel editing ─────────────────────────────
            // Double-click on a part with a BillboardGui descendant enters
            // edit mode on the first TextLabel found. While editing,
            // keyboard input mutates `TextLabel.text` directly so the
            // billboard atlas re-renders live; Enter commits, Escape
            // reverts.
            .add_systems(
                Update,
                (
                    enter_billboard_edit_on_double_click,
                    process_billboard_edit_keyboard
                        .after(enter_billboard_edit_on_double_click),
                ),
            );

        // ── UI-class TOML write-back ──────────────────────────────────
        // GuiTomlFile-to-disk persistence for Property-panel / script /
        // MCP edits. Gated behind the `toml` feature for the same reason
        // `write_instance_changes_system` is (2026-05-15 ECS+DB pivot):
        // in the default build persistence is the WorldDb, not
        // `_instance.toml`, so these systems must NOT run — they also
        // require `RecentlyWrittenFiles`, which is part of the legacy
        // TOML write path. Re-enabled with `--features toml`.
        #[cfg(feature = "toml")]
        {
            app.add_systems(
                Update,
                (
                    save_billboard_gui_changes,
                    save_text_label_changes,
                    save_frame_changes,
                    save_text_button_changes,
                    save_text_box_changes,
                ),
            );
        }
    }
}

// ============================================================================
// BillboardEditState — in-viewport text editing
// ============================================================================

/// Tracks which TextLabel (if any) the user is currently editing
/// in-viewport. Populated by [`enter_billboard_edit_on_double_click`]
/// when the user double-clicks a part with a BillboardGui descendant;
/// consumed by [`process_billboard_edit_keyboard`] which routes typed
/// characters into `TextLabel.text` on the editing entity.
///
/// `original` is the text that was on the label when edit mode entered,
/// captured so Escape can revert. `replace_on_first_type` mirrors the
/// "select-all + type-to-replace" behaviour every text input on every
/// OS implements — the first printable character clears the existing
/// text, subsequent ones append.
#[derive(Resource, Default, Debug)]
pub struct BillboardEditState {
    pub editing: Option<Entity>,
    pub original: String,
    pub replace_on_first_type: bool,
    /// The same mouse-down that fired the second-click of a double-click
    /// is still `just_pressed` for the rest of this frame. Without this
    /// guard `process_billboard_edit_keyboard`'s click-to-exit branch
    /// would fire on the exact click that entered edit mode and
    /// instantly cancel it. Set true on entry, cleared the next frame.
    pub skip_next_click: bool,
}

/// Walk the ChildOf descendants of `root` and return the first entity
/// that has a [`TextLabel`] component. DFS, sibling order = whatever
/// Bevy hands us — for the typical Part → BillboardGui → TextLabel
/// chain there's only one candidate anyway.
fn find_first_textlabel_descendant(
    root: Entity,
    children_q: &Query<&Children>,
    label_q: &Query<(), With<eustress_common::classes::TextLabel>>,
) -> Option<Entity> {
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(e) = stack.pop() {
        visited += 1;
        if label_q.get(e).is_ok() {
            info!("✏️ found TextLabel descendant {:?} (visited {} entities)", e, visited);
            return Some(e);
        }
        match children_q.get(e) {
            Ok(children) => {
                info!("✏️ entity {:?} has {} children", e, children.len());
                for child in children.iter() {
                    stack.push(child);
                }
            }
            Err(_) => {
                info!("✏️ entity {:?} has NO Children component", e);
            }
        }
    }
    info!("✏️ no TextLabel descendant of {:?} (visited {} entities)", root, visited);
    None
}

/// Entry point: react to `DoubleClickedPart` messages by finding a
/// TextLabel descendant of the clicked entity and entering edit mode
/// on it. If nothing editable is found we just ignore the message —
/// double-clicking a plain part with no label is a no-op.
fn enter_billboard_edit_on_double_click(
    mut events: MessageReader<crate::part_selection::DoubleClickedPart>,
    children_q: Query<&Children>,
    label_q: Query<(), With<eustress_common::classes::TextLabel>>,
    text_q: Query<&eustress_common::classes::TextLabel>,
    // Reverse-direction lookup: every TextLabel entity + its ChildOf
    // chain. Used to walk UP from the clicked entity instead of DOWN.
    // We need both directions because the clicked Part doesn't always
    // own a `Children` component (some spawn paths set `ChildOf` on
    // the child without Bevy ever attaching the reciprocal `Children`
    // to the parent — depends on whether the parent was spawned
    // before the child's ChildOf insert was flushed). The Children
    // descent is the fast path; the ChildOf ascent is the fallback.
    all_textlabels: Query<Entity, With<eustress_common::classes::TextLabel>>,
    child_of_q: Query<&ChildOf>,
    mut edit_state: ResMut<BillboardEditState>,
) {
    for ev in events.read() {
        info!("✏️ DoubleClickedPart received for entity {:?}", ev.entity);

        // Try descent via Children first (fast path).
        let mut label_entity = find_first_textlabel_descendant(ev.entity, &children_q, &label_q);

        // Fallback: scan every TextLabel and walk its ChildOf chain
        // upward. If any ancestor is `ev.entity`, that's our match.
        // O(N_text_labels × tree_depth) per double-click — N is tiny
        // in practice and double-clicks are user-initiated.
        if label_entity.is_none() {
            info!("✏️ descent failed; trying ChildOf ascent over {} TextLabel(s)",
                all_textlabels.iter().count());
            for tl in all_textlabels.iter() {
                let mut cur = tl;
                // Cap the walk at 32 hops — way deeper than any real
                // hierarchy, but stops a malformed cycle from looping.
                for _ in 0..32 {
                    if cur == ev.entity {
                        label_entity = Some(tl);
                        info!("✏️ found via ChildOf ascent: TextLabel {:?} → ... → {:?}", tl, ev.entity);
                        break;
                    }
                    match child_of_q.get(cur) {
                        Ok(parent) => cur = parent.parent(),
                        Err(_) => break,
                    }
                }
                if label_entity.is_some() { break; }
            }
        }

        if let Some(label_entity) = label_entity {
            let original = text_q.get(label_entity)
                .map(|t| t.text.clone())
                .unwrap_or_default();
            edit_state.editing = Some(label_entity);
            edit_state.original = original;
            edit_state.replace_on_first_type = true;
            edit_state.skip_next_click = true;
            info!(
                "✏️ Entered billboard text edit mode on {:?} (original={:?})",
                label_entity, edit_state.original,
            );
        } else {
            info!("✏️ no TextLabel anywhere under {:?} — double-click is a no-op", ev.entity);
        }
    }
}

/// Read keyboard input + commit/abort triggers while in edit mode.
/// Mutates `TextLabel.text` directly so the existing
/// `sync_textlabel_to_display` + atlas re-render machinery shows the
/// changes live in the viewport. `save_text_label_changes` then
/// persists to disk through the normal `Changed<TextLabel>` path.
fn process_billboard_edit_keyboard(
    mut edit_state: ResMut<BillboardEditState>,
    mut text_q: Query<&mut eustress_common::classes::TextLabel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut key_events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut dbl_events: MessageReader<crate::part_selection::DoubleClickedPart>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    // Drain any double-click messages produced THIS frame; they were
    // consumed by `enter_billboard_edit_on_double_click` already but
    // un-read messages persist and we don't want them double-counted
    // against the "click while editing → exit" guard below.
    let _ = dbl_events.read().count();

    let Some(label_entity) = edit_state.editing else {
        // Even if nothing is editing, drain keyboard events so this
        // system doesn't accumulate a queue while inactive.
        let _ = key_events.read().count();
        return;
    };

    // Block edit-mode input when the user has clicked into a Slint
    // panel (Properties, Workshop, …). Without this, typing into the
    // Properties panel while a billboard happens to be in edit mode
    // would double-write the keystroke.
    if let Some(focus) = ui_focus.as_ref() {
        if focus.has_focus {
            let _ = key_events.read().count();
            return;
        }
    }

    // Escape — revert original text, exit edit mode.
    if keys.just_pressed(KeyCode::Escape) {
        if let Ok(mut label) = text_q.get_mut(label_entity) {
            label.text = std::mem::take(&mut edit_state.original);
        }
        edit_state.editing = None;
        edit_state.replace_on_first_type = false;
        info!("✏️ Billboard edit cancelled (Escape)");
        let _ = key_events.read().count();
        return;
    }

    // Enter — commit current text, exit edit mode. The text already
    // sits in TextLabel.text from the live updates below, so we just
    // drop edit state; `save_text_label_changes` writes the TOML.
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        edit_state.editing = None;
        edit_state.replace_on_first_type = false;
        edit_state.original.clear();
        info!("✏️ Billboard edit committed (Enter)");
        let _ = key_events.read().count();
        return;
    }

    // Click outside the label commits and exits. The double-click
    // that triggered edit-mode entry is still `just_pressed` for the
    // rest of THIS frame, so we swallow it once via `skip_next_click`
    // and exit only on the NEXT distinct mouse-down.
    if mouse.just_pressed(MouseButton::Left) {
        if edit_state.skip_next_click {
            edit_state.skip_next_click = false;
        } else {
            edit_state.editing = None;
            edit_state.replace_on_first_type = false;
            edit_state.original.clear();
            info!("✏️ Billboard edit committed (click)");
            let _ = key_events.read().count();
            return;
        }
    } else if !mouse.pressed(MouseButton::Left) {
        // Mouse button released — clear the guard so the next press
        // (which will be `just_pressed` again) commits properly.
        edit_state.skip_next_click = false;
    }

    // Live character input. `KeyboardInput` carries the platform-
    // resolved character via `text: Option<SmolStr>` — that gives us
    // proper layout handling (shift, AltGr, dead keys) for free, far
    // better than mapping `KeyCode` to chars ourselves.
    let mut label_mut = match text_q.get_mut(label_entity) {
        Ok(t) => t,
        Err(_) => return,
    };
    use bevy::input::ButtonState;
    for ev in key_events.read() {
        if ev.state != ButtonState::Pressed { continue; }
        // Backspace — delete one grapheme cluster off the end. Skip
        // the "replace" arming flag; a Backspace with nothing typed
        // yet should clear the original text (matching select-all UX).
        if ev.key_code == KeyCode::Backspace {
            if edit_state.replace_on_first_type {
                label_mut.text.clear();
                edit_state.replace_on_first_type = false;
            } else {
                label_mut.text.pop();
            }
            continue;
        }
        // Typed characters arrive in `event.text`. Filter out the
        // control characters that would otherwise sneak in (Enter,
        // Tab, etc. carry `text` payloads on some platforms).
        if let Some(text) = ev.text.as_ref() {
            for ch in text.chars() {
                if ch.is_control() { continue; }
                if edit_state.replace_on_first_type {
                    label_mut.text.clear();
                    edit_state.replace_on_first_type = false;
                }
                label_mut.text.push(ch);
            }
        }
    }
}

fn init_billboard_atlas(world: &mut World) {
    let texture = {
        let mut images = world.resource_mut::<Assets<Image>>();
        create_atlas_image(&mut images, INITIAL_ATLAS_ROWS)
    };
    world.insert_non_send_resource(BillboardAtlas::new(texture, INITIAL_ATLAS_ROWS));
}
