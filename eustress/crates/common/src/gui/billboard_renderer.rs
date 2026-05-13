//! # Billboard GUI Types
//!
//! Shared component types for 3D GUI surfaces — `BillboardGuiMarker`,
//! `SurfaceGuiMarker`, and `GuiElementDisplay`. The actual rendering lives
//! in the engine crate (`engine::billboard_gui`), which software-renders
//! each billboard's GUI subtree directly into a GPU texture (using the
//! `tiny-skia` rasterizer + `cosmic-text` text shaper — the same stack
//! Slint's own `SoftwareRenderer` uses internally) and maps that texture
//! onto a world-space quad through the custom render pipeline in
//! `engine::billboard_pipeline`.
//!
//! Historical note: this file used to host a CPU-atlas + fontdue renderer.
//! A subsequent attempt to reuse the StudioWindow's Slint component for
//! per-billboard rendering failed because Slint's `SharedGlobals.window_adapter`
//! is a `OnceCell` shared across every component instantiation — every
//! `BillboardCard::new()` returned a card bound to the StudioWindow's adapter
//! rather than its own. The current direct-rasterizer path bypasses Slint
//! components entirely while still using the same software-render stack.
//! The `BillboardRendererPlugin` is now a no-op shim kept so upstream crates
//! registering it don't need coordinated edits; prefer
//! `engine::billboard_gui::BillboardGuiPlugin` in new code.

use bevy::prelude::*;

/// GUI element display data — attached to each Frame / TextLabel / Button /
/// ... entity. Walked recursively by the engine's billboard renderer
/// (`engine::billboard_gui::collect_subtree`) into a flat list with
/// canvas-absolute positions, and read by the screen-space UI pipeline
/// for in-editor preview.
#[derive(Component, Debug, Clone)]
pub struct GuiElementDisplay {
    /// Resolved pixel-space rect — final computed values used by the
    /// renderer. Populated by `collect_subtree` each frame from the
    /// `*_udim2` fields below and the parent's resolved size, so
    /// `UDim2 (Scale, Offset)` propagates correctly through nested
    /// layouts (e.g. a `Size = (1, 0, 1, 0)` TextLabel fills its
    /// parent BillboardGui's canvas).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// `UDim2` source position — `[scale_x, offset_x, scale_y, offset_y]`.
    /// Resolved against parent at layout time; treated as pure offset
    /// (Scale=0) for elements without UDim2 layout (legacy paths).
    pub position_udim2: [f32; 4],
    /// `UDim2` source size — `[scale_x, offset_x, scale_y, offset_y]`.
    pub size_udim2: [f32; 4],
    /// Roblox-parity `AnchorPoint` — `(ax, ay)` in `[0, 1]`. Shifts the
    /// element by `-anchor * resolved_size` from its Position so the
    /// anchor lands on the Position point. Default `(0, 0)` = top-left
    /// anchored (existing behaviour); `(0.5, 0.5)` centres the element
    /// on its Position; `(1, 1)` bottom-right-anchored.
    pub anchor_point: [f32; 2],
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
    /// CSS-style font weight (400 = normal, 700 = bold). Drives
    /// `Text.font-weight` in the Slint billboard card; irrelevant
    /// for non-text elements.
    pub font_weight: i32,
    pub text_align: String,
    /// Vertical text alignment within the element's rect — "Top",
    /// "Center" (default), "Bottom". Case-insensitive at render time.
    pub text_y_align: String,
    /// Roblox `TextStrokeColor3` — RGBA. Alpha = `1 - TextStrokeTransparency`.
    /// Drawn as an 8-direction halo at 1-px offsets so the body text
    /// stays readable against busy 3D backgrounds. Zero alpha = no stroke.
    pub text_stroke_color: [f32; 4],
    /// Roblox `TextScaled`. When `true`, the renderer ignores `font_size`
    /// and instead picks the largest font size that fits the text inside
    /// the element's resolved rect (binary search, capped at 72 px).
    /// Useful for fixed-size billboard labels that should always read at
    /// the canvas extent regardless of how long the string is.
    pub text_scaled: bool,
    pub image_path: String,
    pub class_type: String,
    /// Mouse filter mode (Godot-style):
    /// - "stop" (default): consumes mouse events, blocks 3D selection behind
    /// - "pass": receives events but passes them through
    /// - "ignore": transparent to mouse, events go straight through
    pub mouse_filter: String,
}

/// Marker for BillboardGui entities — rendered by the engine crate as 3D
/// quads facing the camera. `size` is in billboard-local pixels; the
/// software-rendered tile in the shared atlas matches those dimensions.
///
/// **Roblox parity**: this marker mirrors the renderable subset of Roblox's
/// `BillboardGui` instance properties. Behaviour fields not handled by the
/// renderer (e.g. `Active`, `ResetOnSpawn`) live on the [`BillboardGui`]
/// class struct only and are read by other engine systems (input routing,
/// teleport reset, etc.). Anything that affects how the quad is drawn,
/// positioned, or culled lives here so the engine systems can act on it
/// without going back to the class component.
///
/// All fields are live-edited: the engine's `sync_billboard_properties`
/// system watches `Changed<BillboardGuiMarker>` and pushes updates into
/// the quad's visibility, depth mode, and texture size each frame.
#[derive(Component, Debug, Clone)]
pub struct BillboardGuiMarker {
    // ── Geometry ──────────────────────────────────────────────────────────
    /// Pixel canvas dimensions ([width, height]). The atlas tile is sized
    /// to fit; oversized billboards are clamped to TILE_W/TILE_H.
    pub size: [f32; 2],
    /// Roblox `SizeOffset` — pixel offset from the anchor point applied to
    /// the rendered card. Currently informational; the renderer treats the
    /// card as anchored at its centre.
    pub size_offset: [f32; 2],
    /// Roblox `ExtentsOffset` — offset (studs) relative to the adornee's
    /// bounding-box extents. Added to the entity's local transform. Useful
    /// for "1 stud above the part's top" placement that survives part resize.
    pub extents_offset: [f32; 3],
    /// Roblox `ExtentsOffsetWorldSpace` — offset (studs) added in world
    /// space (camera-roll independent). Distinct from `extents_offset`
    /// which follows the adornee's orientation.
    pub extents_offset_world_space: [f32; 3],
    /// Roblox `StudsOffsetWorldSpace` — world-space offset (camera-roll
    /// independent). Mirrors `BillboardGui::units_offset_world_space`.
    pub units_offset_world_space: [f32; 3],

    // ── Distance / culling ────────────────────────────────────────────────
    /// Roblox `MaxDistance` / `DistanceUpperLimit` (we use the smaller
    /// of the two when both are set). Distance in studs beyond which the
    /// quad is hidden. 0 disables culling.
    pub max_distance: f32,
    /// Roblox `DistanceLowerLimit` — distance below which the quad is
    /// hidden (so the player's own head label doesn't fill the screen).
    /// 0 disables.
    pub distance_lower_limit: f32,
    /// Roblox `DistanceStep` — quantises the apparent distance for size
    /// snapping. 0 disables (smooth scaling).
    pub distance_step: f32,

    // ── Layering / depth ──────────────────────────────────────────────────
    /// Roblox `AlwaysOnTop`. Render in front of all 3D geometry; flips the
    /// pipeline depth-compare to `Always`.
    pub always_on_top: bool,
    /// Roblox `ClipsDescendants`. When true, child UI elements are clipped
    /// to the billboard's bounds during rasterisation. (Currently the
    /// rasteriser always clips at tile boundaries — this flag is reserved
    /// for finer per-billboard clipping behaviour parity.)
    pub clips_descendants: bool,

    // ── Appearance ────────────────────────────────────────────────────────
    /// Roblox `Brightness`. Multiplier applied to the rendered tile in the
    /// fragment shader (1.0 = unchanged). Currently informational; the
    /// shader does not yet apply it.
    pub brightness: f32,
    /// Roblox `LightInfluence` (0..1). 0 = ignore scene lighting, 1 = fully
    /// affected. Currently informational; we render unlit.
    pub light_influence: f32,

    // ── Camera behaviour ──────────────────────────────────────────────────
    /// When true the quad faces the active camera each frame (handled by
    /// the WGSL vertex shader). False = use entity transform rotation
    /// literally; combined with `BillboardLockAxis::rotation` in the
    /// pipeline.
    pub face_camera: bool,

    // ── Visibility ────────────────────────────────────────────────────────
    /// Combined `Enabled` / `Visible` toggle from the class's `enabled`
    /// flag (Roblox `Enabled`). Distinct from `max_distance`/distance
    /// culling so the two don't fight each other.
    pub visible: bool,

    // ── Depth-bias ────────────────────────────────────────────────────────
    /// Integer ZIndex driving the shader's depth-bias-toward-camera. See
    /// [`crate::classes::BillboardGui::z_index`] for semantics. The
    /// vertex shader converts this to `bias_metres = z_index * 0.05` and
    /// shifts the quad along the camera-toward direction so a label can
    /// sit in front of its own anchor part without bypassing depth
    /// against closer geometry.
    pub z_index: i32,
}

impl Default for BillboardGuiMarker {
    fn default() -> Self {
        Self {
            size: [200.0, 100.0],
            size_offset: [0.0, 0.0],
            extents_offset: [0.0, 0.0, 0.0],
            extents_offset_world_space: [0.0, 0.0, 0.0],
            units_offset_world_space: [0.0, 0.0, 0.0],
            max_distance: 100.0,
            distance_lower_limit: 0.0,
            distance_step: 0.0,
            always_on_top: false,
            clips_descendants: false,
            brightness: 1.0,
            light_influence: 0.0,
            face_camera: true,
            visible: true,
            z_index: 0,
        }
    }
}

/// Marker for SurfaceGui entities — rendered as textures mapped to a part face.
#[derive(Component, Debug)]
pub struct SurfaceGuiMarker {
    pub face: String,
    pub target_part: String,
    /// Texture resolution per world meter on the target surface.
    pub pixels_per_meter: f32,
}

/// Legacy plugin name preserved for registration sites. The real billboard
/// systems live in `engine::billboard_gui::BillboardGuiPlugin` now.
pub struct BillboardRendererPlugin;

impl Plugin for BillboardRendererPlugin {
    fn build(&self, _app: &mut App) {
        // Intentionally empty. See module docs.
    }
}
