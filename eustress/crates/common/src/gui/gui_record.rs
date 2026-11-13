//! # A GUI instance's file, read one way in every shell
//!
//! Studio's loader and a Player's reader turn a GUI instance file (a
//! ScreenGui, a BillboardGui, a SurfaceGui, a Frame, a TextLabel, an
//! ImageLabel, ...) into what it draws and into the tree's properties with
//! the functions here, so no file is read two ways.
//!
//! * [`GuiTomlFile`] is the file: `[gui]` (layout, colours, corner radius,
//!   and a BillboardGui's or SurfaceGui's own settings), `[text]`, `[image]`,
//!   `[scrolling]`, `[viewport]`, and `[asset] path`, an image's picture.
//!   Keys are read in either spelling ([`crate::class_schema::normalise_keys`]).
//! * [`gui_display_for_class`] builds the [`GuiElementDisplay`] of a GUI
//!   object or a ScreenGui, [`billboard_class_from_def`] a BillboardGui's
//!   class and [`surface_class_from_props`] a SurfaceGui's.
//! * [`gui_object_props`], [`billboard_props`] and [`surface_props`] turn
//!   those components into the tree's properties. Studio's seed calls them on
//!   the live components, so its tree holds an edit made after loading. A
//!   Player's reader calls them on the components it builds from the file
//!   ([`record_gui_props`]).
//! * An `Image` in the tree is a path in the Space ([`gui_image_id`]), never a
//!   host's absolute path, which a Player could not open.
//!
//! The property names and value types are the ones the Play VM, replication
//! and the Play runtime's `apply` use: `Text`, `TextColor3`, `Position` (a
//! `UDim2`), `StudsOffset`, `Face` (a `NormalId` item), and so on.
//! `CornerRadius` (pixels) is the Eustress form of a Roblox `UICorner`.

use serde::{Deserialize, Serialize};

use crate::classes::{BillboardGui, SurfaceGui, ZIndexBehavior};
use crate::datamodel::{is_gui_object, DmValue, EnumItem};
use crate::gui::billboard_renderer::GuiElementDisplay;
use crate::scripting::{Color3, UDim2 as TreeUDim2, Vector2, Vector3};

// ============================================================================
// 1. TOML deserialization structs
// ============================================================================

/// Top-level GUI TOML file structure
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlFile {
    #[serde(default)]
    pub instance: GuiTomlInstance,
    #[serde(default)]
    pub metadata: GuiTomlMetadata,
    #[serde(default)]
    pub gui: GuiTomlProperties,
    #[serde(default)]
    pub text: Option<GuiTomlText>,
    /// `[image]` — ImageLabel / ImageButton tint + scale (image SOURCE is in
    /// `[asset].path`, not here).
    #[serde(default)]
    pub image: Option<GuiTomlImage>,
    /// `[scrolling]` — ScrollingFrame canvas / scrollbar config.
    #[serde(default)]
    pub scrolling: Option<GuiTomlScrolling>,
    /// `[viewport]` — ViewportFrame ambient/light config (data round-trip only;
    /// no offscreen render yet).
    #[serde(default)]
    pub viewport: Option<GuiTomlViewport>,
    // ScreenGui files may use instance_loader format with [asset], [transform], [properties]
    #[serde(default)]
    pub asset: Option<toml::Value>,
    #[serde(default)]
    pub transform: Option<toml::Value>,
    #[serde(default)]
    pub properties: Option<toml::Value>,
    /// `CollectionService` tags for this GUI instance — written from
    /// `MluaCreatedInstance.tags` and hydrated into the ECS
    /// [`Tags`](crate::attributes::Tags) component on space load.
    /// Same shape and semantics as `instance_loader::InstanceDefinition.tags`,
    /// so MCP `add_tag` / `get_tagged_entities` see GUI tags identically to
    /// Part tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// [instance] section
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlInstance {
    #[serde(default)]
    pub name: String,
}

/// [metadata] section
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlMetadata {
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub archivable: bool,
    /// Authored unit symbol (`"m"`, `"cm"`, `"mm"`, `"ft"`, `"in"`,
    /// `"studs"`). `None` means the file was authored without a unit
    /// declaration → treat as engine-native meters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// [gui] section — shared visual properties
///
/// **Roblox parity, no legacy paths.** `position` and `size` are
/// strictly `UDim2` (4-float `[scale_x, offset_x, scale_y, offset_y]`).
/// The previous `Vec<f32>` accept-anything form has been removed —
/// older TOML files using `[x, y]` / `[w, h]` will fail to load and
/// must be migrated to the 4-tuple form.
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct GuiTomlProperties {
    #[serde(default)]
    pub position: crate::ui_types::UDim2,
    #[serde(default = "default_size_udim2")]
    pub size: crate::ui_types::UDim2,
    #[serde(default)]
    pub anchor_point: [f32; 2],
    #[serde(default = "default_bg_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub background_color: [f32; 4],
    /// `None` when the TOML has no explicit `background_transparency` key.
    /// Distinguishing that from an authored `0.0` matters: some content
    /// (hand-authored or AI-generated, not Roblox-imported) puts the
    /// intended transparency in `background_color`'s 4th channel instead
    /// of this dedicated field. Use `resolved_background_transparency()`
    /// to fall back to that alpha channel; reading this field directly
    /// silently treats such content as fully opaque.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_transparency: Option<f32>,
    #[serde(default)]
    pub border_size: f32,
    #[serde(default = "default_border_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub border_color: [f32; 4],
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub z_index: i32,
    /// A ScreenGui's `DisplayOrder`: where it draws among the other
    /// ScreenGuis. `None` when the file has no `display_order` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_order: Option<i32>,

    // ── BillboardGui-specific properties ──────────────────────────────────
    // Behaviour flags
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_on_top: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clips_descendants: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_on_spawn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stiffness_by_distance: Option<bool>,

    // Distance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_distance: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_lower_limit: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_upper_limit: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_step: Option<f32>,

    // Appearance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_influence: Option<f32>,

    // Offsets — skip-if-none so files don't bloat with zeroes.
    /// Roblox `SizeOffset` — `Vector2` `[offset_x, offset_y]` in pixels.
    /// Lenient deserialize accepts BOTH the new 2-tuple and the older
    /// 4-tuple UDim2 shape `[scale_x, offset_x, scale_y, offset_y]` —
    /// when migrating from a previous version the existing offset
    /// components survive even though Scale is discarded (Roblox parity
    /// makes Scale meaningless for `SizeOffset`).
    #[serde(default, deserialize_with = "deserialize_size_offset_lenient", skip_serializing_if = "Option::is_none")]
    pub size_offset: Option<[f32; 2]>,
    /// The `Scale` component of `Size` (a UDim2). The importer writes
    /// `size_scale` + `size_offset` as a pair instead of a combined `size`
    /// key, so `size` on its own always fell back to its 100x30px default.
    /// Use `resolved_size()` to get the real, combined value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_scale: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extents_offset: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extents_offset_world_space: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units_offset: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units_offset_world_space: Option<[f32; 3]>,

    // Sorting
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_index_behavior: Option<String>,  // "Sibling" | "Global"

    // Adornee — instance name reference; resolved to entity at load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adornee: Option<String>,

    // ── SurfaceGui-specific properties ────────────────────────────────────
    // Face of the parent part the GUI draws on, a `NormalId` name ("Front").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    // Virtual canvas `[width, height]` in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas_size: Option<[f32; 2]>,
    // Canvas pixels per unit of the face (the importer writes per metre).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixels_per_unit: Option<f32>,
}

impl GuiTomlProperties {
    /// Resolves the real size across the two conventions this data set
    /// mixes: the Roblox importer's split `size_scale` + `size_offset`
    /// pair (never a combined `size` key), and content that computes and
    /// writes the combined `size` 4-tuple directly, leaving `size_scale`/
    /// `size_offset` as stale zero/placeholder leftovers from an earlier
    /// generation pass. Priority:
    ///   1. `size_scale` is non-zero — a real scale signal, the importer's
    ///      convention. Pair it with `size_offset` (defaulting to zero).
    ///   2. `size` differs from the hard default — explicitly authored,
    ///      even though `size_scale` had nothing to say.
    ///   3. Only `size_offset` was ever recorded — pure pixel-offset
    ///      content (Roblox parity: Scale is meaningless for SizeOffset).
    ///   4. Nothing — the hard default.
    pub fn resolved_size(&self) -> crate::ui_types::UDim2 {
        if let Some(scale) = self.size_scale {
            if scale != [0.0, 0.0] {
                let [ox, oy] = self.size_offset.unwrap_or([0.0, 0.0]);
                return crate::ui_types::UDim2::new(scale[0], ox, scale[1], oy);
            }
        }
        if self.size != default_size_udim2() {
            return self.size;
        }
        if let Some([ox, oy]) = self.size_offset {
            return crate::ui_types::UDim2::new(0.0, ox, 0.0, oy);
        }
        self.size
    }

    /// The real transparency: the explicit field if set, else derived from
    /// `background_color`'s alpha channel. Roblox-imported content always
    /// has alpha=1.0 here (3-element RGB padded opaque by `de_color_rgb_or_rgba`),
    /// so this is a no-op for the common case — it only changes behavior
    /// for content that authored a 4-element RGBA color without also
    /// setting `background_transparency`.
    pub fn resolved_background_transparency(&self) -> f32 {
        self.background_transparency
            .unwrap_or_else(|| (1.0 - self.background_color[3]).clamp(0.0, 1.0))
    }
}

/// [text] section — text-specific properties for TextLabel, TextButton, TextBox
#[derive(Debug, Deserialize, Serialize)]
pub struct GuiTomlText {
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_text_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub text_color: [f32; 4],
    #[serde(default)]
    pub text_transparency: f32,
    #[serde(default = "default_text_stroke_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub text_stroke_color: [f32; 4],
    #[serde(default = "default_text_stroke_transparency")]
    pub text_stroke_transparency: f32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub font_family: String,
    /// Font family variant — e.g. "GothamBold", "SourceSans", "RobotoMono".
    /// Feeds font_weight derivation: names containing "Bold" → 700, else 400.
    #[serde(default)]
    pub font: String,
    #[serde(default = "default_left")]
    pub text_x_alignment: String,
    #[serde(default = "default_center")]
    pub text_y_alignment: String,
    /// Roblox `TextScaled`. When `true` the renderer ignores `font_size`
    /// and auto-fits the text to the element's rect (binary search).
    /// `skip_serializing_if = false` keeps TOMLs that haven't opted in
    /// from carrying a redundant `text_scaled = false` line.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub text_scaled: bool,
    /// Roblox `TextWrapped`: the text breaks onto new lines at the element's
    /// width instead of running past it. A file without the key wraps, as
    /// every label drew before the key was read; Roblox's own files and the
    /// class templates write it. Always written, so a save keeps it.
    #[serde(default = "default_true")]
    pub text_wrapped: bool,
}

/// Empty text with the zero values a derived `Default` gives, and wrapping
/// on as for a file without the key.
impl Default for GuiTomlText {
    fn default() -> Self {
        Self {
            text: String::new(),
            text_color: [0.0; 4],
            text_transparency: 0.0,
            text_stroke_color: [0.0; 4],
            text_stroke_transparency: 0.0,
            font_size: 0.0,
            font_family: String::new(),
            font: String::new(),
            text_x_alignment: String::new(),
            text_y_alignment: String::new(),
            text_scaled: false,
            text_wrapped: true,
        }
    }
}

/// `[image]` section — ImageLabel / ImageButton appearance. The image
/// SOURCE path lives in `[asset].path` (the importer routes the `Image`
/// asset_ref through the asset resolver); `image` here is only a fallback.
/// Every color field MUST use `de_color_rgb_or_rgba` so a 3-element 0-255
/// integer RGB array doesn't fail the whole gui parse.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlImage {
    /// Fallback image path (importer normally writes the source to [asset].path).
    #[serde(default)]
    pub image: String,
    #[serde(default = "default_image_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub image_color: [f32; 4],
    #[serde(default)]
    pub image_transparency: f32,
    #[serde(default)]
    pub scale_type: String,
    #[serde(default)]
    pub slice_center: Vec<f32>,
    #[serde(default)]
    pub slice_scale: f32,
    #[serde(default)]
    pub tile_size: Vec<f32>,
    /// ImageButton-only: hover/pressed swap images (stored, not yet wired).
    #[serde(default)]
    pub hover_image: String,
    #[serde(default)]
    pub pressed_image: String,
}

/// `[scrolling]` section — ScrollingFrame canvas + scrollbar config.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlScrolling {
    #[serde(default)]
    pub scrolling_enabled: bool,
    #[serde(default)]
    pub scroll_bar_thickness: i32,
    #[serde(default)]
    pub canvas_size: Vec<f32>,
    #[serde(default)]
    pub canvas_position: Vec<f32>,
    #[serde(default)]
    pub scrolling_direction: String,
    #[serde(default = "default_scrollbar_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub scroll_bar_image_color: [f32; 4],
    #[serde(default)]
    pub scroll_bar_image_transparency: f32,
    #[serde(default)]
    pub elastic_behavior: String,
    #[serde(default)]
    pub top_image: String,
    #[serde(default)]
    pub mid_image: String,
    #[serde(default)]
    pub bottom_image: String,
}

/// `[viewport]` section — ViewportFrame ambient/light config. Data
/// round-trip only; the offscreen-render subsystem is a deferred FEATURE.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlViewport {
    #[serde(default = "default_viewport_ambient", deserialize_with = "de_color_rgb_or_rgba")]
    pub ambient: [f32; 4],
    #[serde(default = "default_viewport_light_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub light_color: [f32; 4],
    #[serde(default)]
    pub light_direction: Vec<f32>,
    #[serde(default = "default_image_color", deserialize_with = "de_color_rgb_or_rgba")]
    pub image_color: [f32; 4],
    #[serde(default)]
    pub image_transparency: f32,
}

fn default_image_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }
fn default_scrollbar_color() -> [f32; 4] { [0.3, 0.3, 0.3, 1.0] }
fn default_viewport_ambient() -> [f32; 4] { [0.78, 0.78, 0.78, 1.0] }
fn default_viewport_light_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }

/// Map a font name to a CSS-style weight. Conservative: anything with "Bold"
/// becomes 700, "Light" becomes 300, otherwise 400. Slint's software renderer
/// uses the weight to pick the right variant when multiple are registered.
pub fn font_weight_from_name(name: &str) -> i32 {
    let lower = name.to_lowercase();
    // Black / Heavy faces, and the single-weight display faces (Fredoka One,
    // Luckiest Guy, Bangers) are heavy type: bold is the nearest the
    // overlay's default font can draw.
    let heavy = ["bold", "black", "heavy", "fredoka", "luckiest", "bangers"];
    if heavy.iter().any(|w| lower.contains(w)) { 700 }
    else if lower.contains("light") || lower.contains("thin") { 300 }
    else { 400 }
}

pub fn default_size_udim2() -> crate::ui_types::UDim2 {
    crate::ui_types::UDim2::from_pixels(100.0, 30.0)
}
pub fn default_bg_color() -> [f32; 4] { [0.2, 0.2, 0.2, 0.8] }
pub fn default_border_color() -> [f32; 4] { [0.5, 0.5, 0.5, 1.0] }
pub fn default_text_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }
fn default_text_stroke_color() -> [f32; 4] { [0.0, 0.0, 0.0, 1.0] }
fn default_text_stroke_transparency() -> f32 { 1.0 }
pub fn default_font_size() -> f32 { 14.0 }
fn default_left() -> String { "Left".to_string() }
fn default_center() -> String { "Center".to_string() }
fn default_true() -> bool { true }

/// Accept the new 2-element `[offset_x, offset_y]` form for `size_offset`.
/// Legacy 4-element UDim2 shapes are treated as missing (default `[0, 0]`):
/// an earlier code path wrote `size_offset` as a UDim2 carrying the same
/// values as `size`, so naively extracting `[arr[1], arr[3]]` would
/// migrate stale duplicate data into the Vector2 field. Roblox's
/// `SizeOffset` default is `(0, 0)`; resetting on migration matches that
/// and the save-on-change pass writes the cleaned value back to disk on
/// the first edit. Users with legitimately-set non-zero values will need
/// to re-enter them once after upgrade — acceptable since the field is
/// rarely used and the cleanup eliminates a confusing redundancy.
fn deserialize_size_offset_lenient<'de, D>(de: D) -> Result<Option<[f32; 2]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v: Option<Vec<f32>> = Option::deserialize(de)?;
    Ok(v.and_then(|arr| match arr.len() {
        2 => Some([arr[0], arr[1]]),
        // Legacy 4-tuple → reset to default. See doc above.
        _ => None,
    }))
}

/// Lenient RGBA color deserialize for the `[gui]` / `[text]` color
/// fields. Accepts BOTH a 3-element `[r, g, b]` (Roblox `Color3`, alpha
/// padded to opaque) AND a 4-element `[r, g, b, a]`, in EITHER the
/// 0-255 integer encoding the GUI schema templates author or the
/// 0.0-1.0 float encoding the runtime panel writes. Always yields a
/// normalised 0.0-1.0 `[f32; 4]` RGBA.
///
/// **Why this exists.** Roblox GUI color properties (`BackgroundColor3`,
/// `BorderColor3`, `TextColor3`) are 3-float `Color3` values with no
/// alpha channel, and the class-schema GUI templates
/// (`class_schema/{Frame,TextLabel,…}/_instance.toml`) likewise author
/// `background_color` / `border_color` / `text_color` as 3-element
/// 0-255 integer arrays (e.g. `[255, 255, 255]`). The strict `[f32; 4]`
/// field previously rejected every one of these with *"invalid length
/// 3, expected an array of length 4"* — ~8038 such failures when
/// loading an imported Vehicle Simulator place (StarterGui / SurfaceGui
/// leaves: ColorOption-N, Hitbox, ColorText, race-checkpoint arrows…).
///
/// This is the same logic the Part `color` field uses
/// (`instance_loader::deserialize_color_flexible`) and matches the
/// runtime edit path (`slint_ui::route_property` → `color_255_to_01` /
/// `color_01`), so GUI colors round-trip consistently no matter which
/// surface authored them. Existing 0.0-1.0 *float* RGBA data (the only
/// 4-element form the loader's own 0-1 defaults and runtime path ever
/// produced) deserialises identically — floats are passed through
/// unchanged; only integer arrays are divided by 255.
fn de_color_rgb_or_rgba<'de, D>(de: D) -> Result<[f32; 4], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values: Vec<toml::Value> = serde::Deserialize::deserialize(de)?;
    if values.len() != 3 && values.len() != 4 {
        return Err(serde::de::Error::invalid_length(
            values.len(),
            &"a color array of length 3 ([r, g, b]) or 4 ([r, g, b, a])",
        ));
    }

    // 0-255 integer encoding (schema templates) vs 0.0-1.0 float
    // encoding (runtime panel / loader defaults). All-integer ⇒ divide
    // by 255; otherwise read as floats (coercing any stray integer).
    let all_integers = values.iter().all(|v| v.is_integer());
    let channel = |v: &toml::Value, fallback: f32| -> f32 {
        if all_integers {
            v.as_integer().map(|i| i as f32 / 255.0).unwrap_or(fallback)
        } else {
            v.as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .map(|f| f as f32)
                .unwrap_or(fallback)
        }
    };

    let r = channel(&values[0], 0.0);
    let g = channel(&values[1], 0.0);
    let b = channel(&values[2], 0.0);
    let a = if values.len() == 4 {
        channel(&values[3], 1.0)
    } else {
        1.0
    };
    Ok([r, g, b, a])
}

// ============================================================================
// 2. Parsing
// ============================================================================

/// A GUI file's document as a [`GuiTomlFile`]: keys normalised to snake_case
/// first, so `[Gui]` and `[gui]` read alike.
pub fn gui_definition_from_value(mut value: toml::Value) -> Result<GuiTomlFile, String> {
    crate::class_schema::normalise_keys(&mut value);
    value
        .try_into::<GuiTomlFile>()
        .map_err(|e: toml::de::Error| format!("Failed to deserialize GUI TOML: {}", e))
}

/// [`gui_definition_from_value`] for a file's text.
pub fn gui_definition_from_str(content: &str) -> Result<GuiTomlFile, String> {
    let value: toml::Value = content
        .parse()
        .map_err(|e: toml::de::Error| format!("Failed to parse GUI TOML: {}", e))?;
    gui_definition_from_value(value)
}

/// Convert a BillboardGui's 3D length fields from the file's authored unit to
/// engine-native metres: the stud offsets and the distance limits
/// (`max_distance`, `distance_lower_limit`, `distance_upper_limit`,
/// `distance_step`). Pixel-space UDim2 fields (`position`, `size`,
/// `anchor_point`, `size_offset`) are 2D canvas coordinates and are not
/// converted; `extents_offset` and `extents_offset_world_space` are
/// part-size multipliers (a ratio, not a length) and are not converted either.
///
/// Every path that turns a BillboardGui definition into its class calls this
/// ([`billboard_class_from_def`]). A Space imported from Roblox is authored in
/// feet, so a path that skipped it revealed each label 3.28 times farther
/// than authored.
pub fn billboard_lengths_to_native(g: &mut GuiTomlProperties, authored_unit: Option<&str>) {
    #[cfg(feature = "units_v1")]
    {
        let from = authored_unit
            .and_then(crate::units::Unit::from_symbol)
            .unwrap_or(crate::units::ENGINE_NATIVE_UNIT);
        let to = crate::units::ENGINE_NATIVE_UNIT;
        if from == to {
            return;
        }
        if let Some(v) = g.units_offset {
            g.units_offset = Some(crate::units::convert_vec3_f32(v, from, to));
        }
        if let Some(v) = g.units_offset_world_space {
            g.units_offset_world_space = Some(crate::units::convert_vec3_f32(v, from, to));
        }
        for f in [&mut g.max_distance, &mut g.distance_lower_limit, &mut g.distance_upper_limit, &mut g.distance_step] {
            if let Some(v) = *f {
                *f = Some(crate::units::convert_f32(v, from, to));
            }
        }
    }
    #[cfg(not(feature = "units_v1"))]
    {
        let _ = (g, authored_unit);
    }
}

// ============================================================================
// 3. What a GUI instance draws as
// ============================================================================

/// A GUI object's display from its `[gui]` section and, for a text class,
/// its `[text]` section. `class_type` names the class. Position and size are
/// stored as their UDim2 4-tuples and as a best-effort pixel rect (offsets
/// only, since the parent's size is not known here); the renderer resolves
/// Scale against the parent when it lays the tree out.
pub fn gui_display_from_props(gui: &GuiTomlProperties, text_props: Option<&GuiTomlText>, class_type: &str) -> GuiElementDisplay {
    let (text, text_color, font, text_stroke_color, font_size, text_align, text_y_align, font_weight, text_scaled, text_wrapped) =
        if let Some(tp) = text_props {
            let weight_source = if !tp.font.is_empty() { &tp.font } else { &tp.font_family };
            (
                tp.text.clone(),
                [tp.text_color[0], tp.text_color[1], tp.text_color[2], (1.0 - tp.text_transparency).clamp(0.0, 1.0)],
                if !tp.font.is_empty() { tp.font.clone() } else { tp.font_family.clone() },
                [
                    tp.text_stroke_color[0],
                    tp.text_stroke_color[1],
                    tp.text_stroke_color[2],
                    (1.0 - tp.text_stroke_transparency).clamp(0.0, 1.0),
                ],
                tp.font_size,
                tp.text_x_alignment.clone(),
                tp.text_y_alignment.clone(),
                font_weight_from_name(weight_source),
                tp.text_scaled,
                tp.text_wrapped,
            )
        } else {
            (
                String::new(),
                [1.0, 1.0, 1.0, 1.0],
                String::new(),
                [0.0, 0.0, 0.0, 0.0],
                14.0,
                "Center".to_string(),
                "Center".to_string(),
                400,
                false,
                true,
            )
        };
    let is_button = class_type.eq_ignore_ascii_case("textbutton") || class_type.eq_ignore_ascii_case("imagebutton");
    GuiElementDisplay {
        x: gui.position.x.offset,
        y: gui.position.y.offset,
        width: gui.size.x.offset.max(1.0),
        height: gui.size.y.offset.max(1.0),
        position_udim2: [gui.position.x.scale, gui.position.x.offset, gui.position.y.scale, gui.position.y.offset],
        size_udim2: [gui.size.x.scale, gui.size.x.offset, gui.size.y.scale, gui.size.y.offset],
        anchor_point: gui.anchor_point,
        z_order: gui.z_index,
        visible: gui.visible,
        // A ScrollingFrame clips unless its file says otherwise; anything
        // else clips only when its file says `clips_descendants = true`.
        clip_children: gui.clips_descendants.unwrap_or(class_type.eq_ignore_ascii_case("scrollingframe")),
        scroll_x: 0.0,
        scroll_y: 0.0,
        bg_color: [
            gui.background_color[0],
            gui.background_color[1],
            gui.background_color[2],
            (1.0 - gui.resolved_background_transparency()).clamp(0.0, 1.0),
        ],
        border_size: gui.border_size,
        border_color: gui.border_color,
        corner_radius: gui.corner_radius,
        text,
        text_color,
        font,
        font_size,
        font_weight,
        text_align,
        text_y_align,
        text_stroke_color,
        text_scaled,
        text_wrapped,
        image_path: String::new(),
        class_type: class_type.to_string(),
        mouse_filter: "stop".to_string(),
        // Roblox: a button is Active unless its file says otherwise; any
        // other object only when its file says so.
        active: gui.active.unwrap_or(is_button),
    }
}

/// A ScreenGui's display: the root its GUI objects draw under. It draws
/// nothing itself; `visible` hides its whole subtree and `display_order`
/// orders it among the other ScreenGuis.
pub fn screen_gui_display(visible: bool, display_order: i32) -> GuiElementDisplay {
    GuiElementDisplay {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
        position_udim2: [0.0; 4],
        size_udim2: [0.0; 4],
        anchor_point: [0.0, 0.0],
        z_order: display_order,
        visible,
        clip_children: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        bg_color: [0.0; 4],
        border_size: 0.0,
        border_color: [0.0; 4],
        corner_radius: 0.0,
        text: String::new(),
        text_color: [1.0; 4],
        font: String::new(),
        font_size: 14.0,
        font_weight: 400,
        text_align: "Center".to_string(),
        text_y_align: "Center".to_string(),
        text_stroke_color: [0.0, 0.0, 0.0, 0.0],
        text_scaled: false,
        text_wrapped: true,
        image_path: String::new(),
        class_type: "screengui".to_string(),
        mouse_filter: "ignore".to_string(),
        active: false,
    }
}

/// What a GUI object or a ScreenGui of class `class` draws as, from its file:
/// the one builder Studio's loader (every path: a folder, a flat file, a hot
/// reload) and a Player's reader use. The size is [`GuiTomlProperties::resolved_size`].
/// A ScreenGui shows when its file leaves both `enabled` and `visible` on. A
/// text class reads `[text]`; a ViewportFrame or another media frame draws
/// its class name until it renders; a ScrollingFrame starts scrolled to its
/// `[scrolling] canvas_position`. An image class's picture is the caller's to
/// set in `image_path` ([`gui_image_source`]), since Studio and a Player
/// find files in different places.
pub fn gui_display_for_class(class: &str, def: &GuiTomlFile) -> GuiElementDisplay {
    let mut gui = def.gui.clone();
    gui.size = gui.resolved_size();
    match class {
        "ScreenGui" => screen_gui_display(gui.enabled.unwrap_or(true) && gui.visible, gui.display_order.unwrap_or(0)),
        "TextLabel" | "TextButton" | "TextBox" => gui_display_from_props(&gui, def.text.as_ref(), class),
        "ViewportFrame" | "VideoFrame" | "DocumentFrame" | "WebFrame" => {
            let placeholder = GuiTomlText {
                text: format!("[{class}]"),
                text_color: [0.5, 0.5, 0.5, 0.8],
                font_size: 12.0,
                text_x_alignment: "Center".to_string(),
                text_y_alignment: "Center".to_string(),
                ..Default::default()
            };
            gui_display_from_props(&gui, Some(&placeholder), class)
        }
        "ScrollingFrame" => {
            let mut d = gui_display_from_props(&gui, None, class);
            let [x, y] = scrolling_canvas_position(def);
            d.scroll_x = x;
            d.scroll_y = y;
            d
        }
        _ => gui_display_from_props(&gui, None, class),
    }
}

/// A ScrollingFrame's starting scroll, `[scrolling] canvas_position`, else
/// the class default.
pub fn scrolling_canvas_position(def: &GuiTomlFile) -> [f32; 2] {
    def.scrolling
        .as_ref()
        .filter(|s| s.canvas_position.len() == 2)
        .map(|s| [s.canvas_position[0], s.canvas_position[1]])
        .unwrap_or_else(|| crate::classes::ScrollingFrame::default().canvas_position)
}

/// An ImageLabel's or ImageButton's picture as its file names it, relative to
/// the file's folder: `[asset] path` (the importer's), else `[image] image`.
/// `None` when it names none.
pub fn gui_image_source(def: &GuiTomlFile) -> Option<String> {
    def.asset
        .as_ref()
        .and_then(|a| a.get("path"))
        .and_then(|p| p.as_str())
        .map(str::to_string)
        .or_else(|| def.image.as_ref().map(|i| i.image.clone()).filter(|s| !s.is_empty()))
        .filter(|s| !s.is_empty())
}

/// An image's tint and transparency, `[image] image_color` and
/// `image_transparency`: opaque white without them.
pub fn gui_image_tint(def: &GuiTomlFile) -> ([f32; 3], f32) {
    let img = def.image.as_ref();
    (
        img.map(|i| [i.image_color[0], i.image_color[1], i.image_color[2]]).unwrap_or([1.0, 1.0, 1.0]),
        img.map(|i| i.image_transparency).unwrap_or(0.0),
    )
}

/// The tree's `Image` for a picture `source` named by the file at
/// `instance_key` (Space-relative): its path in the Space, `..` and `.`
/// resolved, as a part's `MeshId` is ([`crate::datamodel::record::mesh_id_of`]).
/// A content id (`rbxassetid://...`, `space://...`) is kept as written.
pub fn gui_image_id(instance_key: &str, source: &str) -> String {
    if source.is_empty() || source.contains("://") {
        return source.to_string();
    }
    crate::datamodel::record::mesh_id_of(instance_key, source)
}

/// A BillboardGui's class from its file: lengths in metres
/// ([`billboard_lengths_to_native`]), the size resolved
/// ([`GuiTomlProperties::resolved_size`]), then [`billboard_class_from_props`].
pub fn billboard_class_from_def(def: &GuiTomlFile) -> BillboardGui {
    let mut g = def.gui.clone();
    billboard_lengths_to_native(&mut g, def.metadata.unit.as_deref());
    g.size = g.resolved_size();
    billboard_class_from_props(&g)
}

/// Build a `BillboardGui` class component from parsed `[gui]` properties:
/// the single mapping from the on-disk schema to the class. A field the file
/// lacks keeps the class default, so an older file loads cleanly. The caller
/// resolves the size and the lengths first ([`billboard_class_from_def`]).
pub fn billboard_class_from_props(g: &GuiTomlProperties) -> BillboardGui {
    let mut bb = BillboardGui::default();

    // Geometry
    bb.size = g.size;
    if let Some(v) = g.size_offset { bb.size_offset = v; }
    if let Some(v) = g.extents_offset { bb.extents_offset = v; }
    if let Some(v) = g.extents_offset_world_space { bb.extents_offset_world_space = v; }
    if let Some(v) = g.units_offset { bb.units_offset = v; }
    if let Some(v) = g.units_offset_world_space { bb.units_offset_world_space = v; }

    // Distance
    if let Some(v) = g.max_distance { bb.max_distance = v; }
    if let Some(v) = g.distance_lower_limit { bb.distance_lower_limit = v; }
    if let Some(v) = g.distance_upper_limit { bb.distance_upper_limit = v; }
    if let Some(v) = g.distance_step { bb.distance_step = v; }

    // Behaviour flags
    if let Some(v) = g.active { bb.active = v; }
    if let Some(v) = g.enabled { bb.enabled = v; }
    if let Some(v) = g.always_on_top { bb.always_on_top = v; }
    if let Some(v) = g.clips_descendants { bb.clips_descendants = v; }
    if let Some(v) = g.reset_on_spawn { bb.reset_on_spawn = v; }
    if let Some(v) = g.stiffness_by_distance { bb.stiffness_by_distance = v; }

    // Appearance
    if let Some(v) = g.brightness { bb.brightness = v; }
    if let Some(v) = g.light_influence { bb.light_influence = v; }

    // Sorting
    if let Some(ref s) = g.z_index_behavior {
        bb.z_index_behavior = match s.as_str() {
            "Global" => ZIndexBehavior::Global,
            _ => ZIndexBehavior::Sibling,
        };
    }
    bb.z_index = g.z_index;

    bb
}

/// Build a `SurfaceGui` class component from parsed `[gui]` properties, as
/// [`billboard_class_from_props`] does for a BillboardGui. It reads the face
/// (any case, an `Enum.NormalId.` prefix or Roblox's number; an unknown name
/// is Front), the canvas, the distance, the behaviour flags and the
/// appearance. `size` is not read: a SurfaceGui file has none, and the
/// `[gui]` default is a 100x30 pixel box where the class default covers the
/// whole face. Nothing is unit-converted; the canvas is in pixels.
pub fn surface_class_from_props(g: &GuiTomlProperties) -> SurfaceGui {
    let mut sg = SurfaceGui::default();

    // Face and canvas
    if let Some(ref s) = g.face {
        sg.face = crate::classes::NormalId::parse(s).unwrap_or_default();
    }
    if let Some(v) = g.canvas_size { sg.canvas_size = v; }
    if let Some(v) = g.pixels_per_unit { sg.pixels_per_unit = v; }

    // Distance
    if let Some(v) = g.max_distance { sg.max_distance = v; }

    // Behaviour flags
    if let Some(v) = g.active { sg.active = v; }
    if let Some(v) = g.enabled { sg.enabled = v; }
    if let Some(v) = g.always_on_top { sg.always_on_top = v; }
    if let Some(v) = g.clips_descendants { sg.clips_descendants = v; }

    // Appearance
    if let Some(v) = g.brightness { sg.brightness = v; }
    if let Some(v) = g.light_influence { sg.light_influence = v; }

    sg
}

// ============================================================================
// 4. The tree's properties
// ============================================================================

/// An ImageLabel's or ImageButton's own properties: its `Image` (a path in
/// the Space, [`gui_image_id`]), `ImageColor3` and `ImageTransparency`.
#[derive(Debug, Clone, PartialEq)]
pub struct GuiImageProps {
    pub image: String,
    pub color: [f32; 3],
    pub transparency: f32,
}

/// [`GuiImageProps`] from an image class's file at `instance_key`.
pub fn gui_image_props(def: &GuiTomlFile, instance_key: &str) -> GuiImageProps {
    let (color, transparency) = gui_image_tint(def);
    let image = gui_image_source(def).map(|s| gui_image_id(instance_key, &s)).unwrap_or_default();
    GuiImageProps { image, color, transparency }
}

/// Whether a class draws text from `[text]`.
pub fn is_text_class(class: &str) -> bool {
    matches!(class, "TextLabel" | "TextButton" | "TextBox")
}

/// Whether a class draws a picture.
pub fn is_image_class(class: &str) -> bool {
    matches!(class, "ImageLabel" | "ImageButton")
}

/// An alignment or font name as its enum item spells it: "center" reads
/// "Center". Empty reads `fallback`.
fn enum_name(s: &str, fallback: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => fallback.to_string(),
    }
}

fn color3(c: &[f32]) -> DmValue {
    DmValue::Color3(Color3::new(c[0] as f64, c[1] as f64, c[2] as f64))
}

fn udim2(a: [f32; 4]) -> DmValue {
    DmValue::UDim2(TreeUDim2::new(a[0] as f64, a[1] as f64, a[2] as f64, a[3] as f64))
}

fn vector3(a: [f32; 3]) -> DmValue {
    DmValue::Vector3(Vector3::new(a[0] as f64, a[1] as f64, a[2] as f64))
}

/// A ScreenGui's or a GUI object's properties in the tree, from its display.
/// A ScreenGui has `Enabled` and `DisplayOrder`. A GUI object has its layout
/// (`Position`, `Size`, `AnchorPoint`, `ZIndex`, `Visible`), its box
/// (`BackgroundColor3`, `BackgroundTransparency`, `BorderSizePixel`,
/// `BorderColor3`, `CornerRadius`), `ClipsDescendants` and `Active`; a text
/// class its text, font, alignment, scaling, wrapping and stroke; an image
/// class `image`'s properties; a ScrollingFrame its `CanvasPosition`.
pub fn gui_object_props(class: &str, d: &GuiElementDisplay, image: Option<&GuiImageProps>) -> Vec<(String, DmValue)> {
    let mut p: Vec<(String, DmValue)> = Vec::new();
    let mut put = |k: &str, v: DmValue| p.push((k.to_string(), v));
    if class == "ScreenGui" {
        put("Enabled", DmValue::Bool(d.visible));
        put("DisplayOrder", DmValue::Number(d.z_order as f64));
        return p;
    }
    put("Position", udim2(d.position_udim2));
    put("Size", udim2(d.size_udim2));
    put("AnchorPoint", DmValue::Vector2(Vector2::new(d.anchor_point[0] as f64, d.anchor_point[1] as f64)));
    put("Visible", DmValue::Bool(d.visible));
    put("ZIndex", DmValue::Number(d.z_order as f64));
    put("BackgroundColor3", color3(&d.bg_color));
    put("BackgroundTransparency", DmValue::Number(1.0 - d.bg_color[3] as f64));
    put("BorderSizePixel", DmValue::Number(d.border_size as f64));
    put("BorderColor3", color3(&d.border_color));
    put("CornerRadius", DmValue::Number(d.corner_radius as f64));
    put("ClipsDescendants", DmValue::Bool(d.clip_children));
    put("Active", DmValue::Bool(d.active));
    if is_text_class(class) {
        put("Text", DmValue::String(d.text.clone()));
        put("TextColor3", color3(&d.text_color));
        put("TextTransparency", DmValue::Number(1.0 - d.text_color[3] as f64));
        put("TextSize", DmValue::Number(d.font_size as f64));
        put("TextScaled", DmValue::Bool(d.text_scaled));
        put("TextWrapped", DmValue::Bool(d.text_wrapped));
        let font = if d.font.is_empty() { "SourceSans".to_string() } else { d.font.clone() };
        put("Font", DmValue::Enum(EnumItem::new("Font", font)));
        put("TextXAlignment", DmValue::Enum(EnumItem::new("TextXAlignment", enum_name(&d.text_align, "Center"))));
        put("TextYAlignment", DmValue::Enum(EnumItem::new("TextYAlignment", enum_name(&d.text_y_align, "Center"))));
        put("TextStrokeColor3", color3(&d.text_stroke_color));
        put("TextStrokeTransparency", DmValue::Number(1.0 - d.text_stroke_color[3] as f64));
    }
    if is_image_class(class) {
        let blank = GuiImageProps { image: String::new(), color: [1.0, 1.0, 1.0], transparency: 0.0 };
        let img = image.unwrap_or(&blank);
        put("Image", DmValue::String(img.image.clone()));
        put("ImageColor3", color3(&img.color));
        put("ImageTransparency", DmValue::Number(img.transparency as f64));
    }
    if class == "ScrollingFrame" {
        put("CanvasPosition", DmValue::Vector2(Vector2::new(d.scroll_x as f64, d.scroll_y as f64)));
    }
    p
}

/// A BillboardGui's properties in the tree, from its class: Roblox's names
/// (`StudsOffset`, `MaxDistance`, ...), lengths in metres, `Size` a UDim2
/// whose Scale is studs and whose Offset is pixels. `ZIndex` is its depth
/// bias.
pub fn billboard_props(bb: &BillboardGui) -> Vec<(String, DmValue)> {
    let s = bb.size;
    let z_index_behavior = match bb.z_index_behavior {
        ZIndexBehavior::Global => "Global",
        ZIndexBehavior::Sibling => "Sibling",
    };
    vec![
        ("Enabled".into(), DmValue::Bool(bb.enabled)),
        ("Active".into(), DmValue::Bool(bb.active)),
        ("AlwaysOnTop".into(), DmValue::Bool(bb.always_on_top)),
        ("ClipsDescendants".into(), DmValue::Bool(bb.clips_descendants)),
        ("ResetOnSpawn".into(), DmValue::Bool(bb.reset_on_spawn)),
        ("StiffnessByDistance".into(), DmValue::Bool(bb.stiffness_by_distance)),
        ("Size".into(), udim2([s.x.scale, s.x.offset, s.y.scale, s.y.offset])),
        ("SizeOffset".into(), DmValue::Vector2(Vector2::new(bb.size_offset[0] as f64, bb.size_offset[1] as f64))),
        ("StudsOffset".into(), vector3(bb.units_offset)),
        ("StudsOffsetWorldSpace".into(), vector3(bb.units_offset_world_space)),
        ("ExtentsOffset".into(), vector3(bb.extents_offset)),
        ("ExtentsOffsetWorldSpace".into(), vector3(bb.extents_offset_world_space)),
        ("MaxDistance".into(), DmValue::Number(bb.max_distance as f64)),
        ("DistanceLowerLimit".into(), DmValue::Number(bb.distance_lower_limit as f64)),
        ("DistanceUpperLimit".into(), DmValue::Number(bb.distance_upper_limit as f64)),
        ("DistanceStep".into(), DmValue::Number(bb.distance_step as f64)),
        ("Brightness".into(), DmValue::Number(bb.brightness as f64)),
        ("LightInfluence".into(), DmValue::Number(bb.light_influence as f64)),
        ("ZIndexBehavior".into(), DmValue::Enum(EnumItem::new("ZIndexBehavior", z_index_behavior))),
        ("ZIndex".into(), DmValue::Number(bb.z_index as f64)),
    ]
}

/// A SurfaceGui's properties in the tree, from its class: `Face` (a
/// `NormalId` item), `CanvasSize` (pixels), `PixelsPerUnit` (canvas pixels
/// per metre of the face), and its distance, flags and appearance.
pub fn surface_props(sg: &SurfaceGui) -> Vec<(String, DmValue)> {
    let z_index_behavior = match sg.z_index_behavior {
        ZIndexBehavior::Global => "Global",
        ZIndexBehavior::Sibling => "Sibling",
    };
    vec![
        ("Enabled".into(), DmValue::Bool(sg.enabled)),
        ("Active".into(), DmValue::Bool(sg.active)),
        ("AlwaysOnTop".into(), DmValue::Bool(sg.always_on_top)),
        ("ClipsDescendants".into(), DmValue::Bool(sg.clips_descendants)),
        ("Face".into(), DmValue::Enum(EnumItem::new("NormalId", sg.face.as_str()))),
        ("CanvasSize".into(), DmValue::Vector2(Vector2::new(sg.canvas_size[0] as f64, sg.canvas_size[1] as f64))),
        ("PixelsPerUnit".into(), DmValue::Number(sg.pixels_per_unit as f64)),
        ("MaxDistance".into(), DmValue::Number(sg.max_distance as f64)),
        ("Brightness".into(), DmValue::Number(sg.brightness as f64)),
        ("LightInfluence".into(), DmValue::Number(sg.light_influence as f64)),
        ("ZIndexBehavior".into(), DmValue::Enum(EnumItem::new("ZIndexBehavior", z_index_behavior))),
    ]
}

/// A GUI instance's properties in a Player's tree, read from its file
/// `doc` at the Space-relative `instance_key` through the builders Studio's
/// loader uses, then turned into properties as Studio's seed turns its live
/// components. `class` is the tree class. A class that is no GUI, or a file
/// that does not parse (Studio then loads it with its defaults), reads none.
pub fn record_gui_props(class: &str, doc: &toml::Value, instance_key: &str) -> Vec<(String, DmValue)> {
    if !reads_gui_file(class) {
        return Vec::new();
    }
    let Ok(def) = gui_definition_from_value(doc.clone()) else {
        return Vec::new();
    };
    match class {
        "BillboardGui" => billboard_props(&billboard_class_from_def(&def)),
        "SurfaceGui" => surface_props(&surface_class_from_props(&def.gui)),
        _ => {
            let display = gui_display_for_class(class, &def);
            let image = is_image_class(class).then(|| gui_image_props(&def, instance_key));
            gui_object_props(class, &display, image.as_ref())
        }
    }
}

/// Whether a tree class is read from a GUI file: a ScreenGui, a
/// BillboardGui, a SurfaceGui or a GUI object.
pub fn reads_gui_file(class: &str) -> bool {
    matches!(class, "ScreenGui" | "BillboardGui" | "SurfaceGui") || is_gui_object(class)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(class: &str, text: &str, key: &str) -> std::collections::HashMap<String, DmValue> {
        let doc: toml::Value = text.parse().expect("test file parses");
        record_gui_props(class, &doc, key).into_iter().collect()
    }

    fn num(p: &std::collections::HashMap<String, DmValue>, k: &str) -> f64 {
        match p.get(k) {
            Some(DmValue::Number(n)) => *n,
            other => panic!("{k}: {other:?}"),
        }
    }

    /// A TextLabel's file reads into the tree: its text and font, its stroke
    /// (alpha from its transparency), its wrapping, its corner radius, and a
    /// size from the importer's split `size_scale` + `size_offset`.
    #[test]
    fn gui_record_reads_a_text_labels_file() {
        let p = props(
            "TextLabel",
            "[metadata]\nclass_name = \"TextLabel\"\n\n\
             [gui]\nsize_scale = [1.0, 0.5]\nsize_offset = [4.0, 0.0]\nanchor_point = [0.5, 0.0]\n\
             corner_radius = 8.0\nclips_descendants = true\nbackground_transparency = 1.0\nz_index = 3\n\n\
             [text]\ntext = \"Formulate your Question\"\nfont = \"Oswald\"\nfont_size = 48.0\n\
             text_color = [255, 128, 0]\ntext_stroke_color = [0.0, 0.0, 0.0]\ntext_stroke_transparency = 0.5\n\
             text_scaled = true\ntext_wrapped = false\ntext_x_alignment = \"left\"\n",
            "Workspace/Sign/Label/Text/_instance.toml",
        );
        assert_eq!(p.get("Text"), Some(&DmValue::String("Formulate your Question".into())));
        assert_eq!(p.get("Font"), Some(&DmValue::Enum(EnumItem::new("Font", "Oswald"))));
        assert_eq!(p.get("TextXAlignment"), Some(&DmValue::Enum(EnumItem::new("TextXAlignment", "Left"))));
        assert_eq!(p.get("TextScaled"), Some(&DmValue::Bool(true)));
        assert_eq!(p.get("TextWrapped"), Some(&DmValue::Bool(false)));
        assert!((num(&p, "TextStrokeTransparency") - 0.5).abs() < 1e-6);
        assert_eq!(num(&p, "TextSize"), 48.0);
        assert_eq!(num(&p, "CornerRadius"), 8.0);
        assert_eq!(num(&p, "ZIndex"), 3.0);
        assert_eq!(p.get("ClipsDescendants"), Some(&DmValue::Bool(true)));
        assert_eq!(p.get("Size"), Some(&DmValue::UDim2(TreeUDim2::new(1.0, 4.0, 0.5, 0.0))));
        match p.get("TextColor3") {
            Some(DmValue::Color3(c)) => assert!((c.r - 1.0).abs() < 1e-6 && (c.g - 128.0 / 255.0).abs() < 1e-6),
            other => panic!("TextColor3: {other:?}"),
        }
        // A Frame has no text of its own.
        let frame = props("Frame", "[gui]\ncorner_radius = 4.0\n", "StarterGui/Hud/Panel/_instance.toml");
        assert!(frame.get("Text").is_none());
        assert_eq!(num(&frame, "CornerRadius"), 4.0);
    }

    /// A file that leaves `text_wrapped` out wraps, as every label drew
    /// before the key was read.
    #[test]
    fn gui_record_wraps_when_the_file_does_not_say() {
        let p = props("TextLabel", "[text]\ntext = \"Hi\"\n", "StarterGui/A/_instance.toml");
        assert_eq!(p.get("TextWrapped"), Some(&DmValue::Bool(true)));
    }

    /// A BillboardGui's file reads its class: the importer's split size,
    /// its distance and flags, and its Global sorting.
    #[test]
    fn gui_record_reads_a_billboards_file() {
        let p = props(
            "BillboardGui",
            "[metadata]\nclass_name = \"BillboardGui\"\nunit = \"m\"\n\n\
             [gui]\nalways_on_top = true\nmax_distance = 160.0\nsize_scale = [3.4, 0.55]\nsize_offset = [0.0, 0.0]\n\
             units_offset = [0.0, 1.5, 0.0]\nz_index_behavior = \"Global\"\nlight_influence = 0.0\n",
            "Workspace/Sign/Label/_instance.toml",
        );
        assert_eq!(p.get("Size"), Some(&DmValue::UDim2(TreeUDim2::new(3.4f32 as f64, 0.0, 0.55f32 as f64, 0.0))));
        assert_eq!(p.get("AlwaysOnTop"), Some(&DmValue::Bool(true)));
        assert_eq!(num(&p, "MaxDistance"), 160.0);
        assert_eq!(p.get("StudsOffset"), Some(&DmValue::Vector3(Vector3::new(0.0, 1.5, 0.0))));
        assert_eq!(p.get("ZIndexBehavior"), Some(&DmValue::Enum(EnumItem::new("ZIndexBehavior", "Global"))));
    }

    /// A SurfaceGui's file reads its face and canvas; an image's picture is
    /// a path in the Space, resolved against its file's folder.
    #[test]
    fn gui_record_reads_a_surface_and_an_image() {
        let p = props(
            "SurfaceGui",
            "[gui]\nface = \"Enum.NormalId.Top\"\ncanvas_size = [400.0, 200.0]\n",
            "Workspace/Screen/Gui/_instance.toml",
        );
        assert_eq!(p.get("Face"), Some(&DmValue::Enum(EnumItem::new("NormalId", "Top"))));
        assert_eq!(p.get("CanvasSize"), Some(&DmValue::Vector2(Vector2::new(400.0, 200.0))));
        let img = props(
            "ImageLabel",
            "[asset]\npath = \"../Images/logo.png\"\n\n[image]\nimage_transparency = 0.25\n",
            "StarterGui/Hud/Logo/_instance.toml",
        );
        assert_eq!(img.get("Image"), Some(&DmValue::String("StarterGui/Hud/Images/logo.png".into())));
        assert_eq!(num(&img, "ImageTransparency"), 0.25);
        assert_eq!(gui_image_id("A/B/_instance.toml", "rbxassetid://12"), "rbxassetid://12");
    }

    /// A ScreenGui shows only when its file leaves `enabled` and `visible`
    /// both on, and orders by `display_order`.
    #[test]
    fn gui_record_reads_a_screen_guis_enabled_and_order() {
        let p = props("ScreenGui", "[gui]\nenabled = false\ndisplay_order = 4\n", "StarterGui/Hud/_instance.toml");
        assert_eq!(p.get("Enabled"), Some(&DmValue::Bool(false)));
        assert_eq!(num(&p, "DisplayOrder"), 4.0);
        assert!(record_gui_props("Part", &"[gui]\n".parse().unwrap(), "x").is_empty());
    }
}
