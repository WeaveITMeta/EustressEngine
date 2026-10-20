//! Properties panel for the light classes: PointLight, SpotLight,
//! SurfaceLight and DirectionalLight.
//!
//! Rows come from the live authoring component, so the panel shows what the
//! light is doing, not a Part's Appearance and Physics (which a light has no
//! use for, and which is what it showed before: every light field was
//! unreachable and its Color row was a grey `[properties]` default).
//!
//! An edit is parsed, written to the component (the Bevy light follows it
//! through `light_classes`), saved into the file's `[light]` section and
//! pushed on the undo stack as a `ChangeClassField`, whose replay comes back
//! through [`set_field_text`]. Range and Radius are shown in the display unit
//! and stored in metres.
//!
//! This module also plans where a new light goes (see [`plan_insert`]).

use std::path::{Path, PathBuf};

use bevy::ecs::component::Mutable;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use eustress_common::classes::{
    ClassName, EustressDirectionalLight, EustressPointLight, EustressSpotLight, SurfaceLight,
};
use eustress_common::plugins::light_classes::{
    light_section_table, normalize_face, LightSectionRef, FACES,
};
use eustress_common::units::{convert_f32, Unit, ENGINE_NATIVE_UNIT};

use crate::space::instance_create::InstanceOverrides;

/// Category the light rows appear under.
pub const LIGHT_CATEGORY: &str = "Light";

/// The choices the Face row offers.
pub fn face_options() -> Vec<slint::SharedString> {
    FACES.iter().map(|f| slint::SharedString::from(*f)).collect()
}

/// True for the four light classes.
pub fn is_light_class(class: ClassName) -> bool {
    eustress_common::plugins::light_classes::is_light_class(class)
}

// ============================================================================
// Field access shared by the four classes
// ============================================================================

/// Text in, text out: every field in the form the undo stack stores it
/// (lengths in metres, colours as `r, g, b` in 0-255).
trait LightFields {
    /// The panel's rows, in order: `(name, kind)`.
    const ROWS: &'static [(&'static str, &'static str)];
    fn get_text(&self, key: &str) -> Option<String>;
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String>;
    fn section(&self) -> toml::value::Table;
}

fn fmt_f32(v: f32) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".to_string() } else { s.to_string() }
}

fn color_text(c: Color) -> String {
    let s = c.to_srgba();
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{}, {}, {}", ch(s.red), ch(s.green), ch(s.blue))
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(format!("expected true or false, got {other:?}")),
    }
}

fn parse_non_negative(text: &str) -> Result<f32, String> {
    let v: f32 = text.trim().parse().map_err(|_| format!("expected a number, got {:?}", text.trim()))?;
    if !v.is_finite() {
        return Err("expected a finite number".into());
    }
    Ok(v.max(0.0))
}

/// `#RRGGBB`, `RRGGBB`, or `r, g, b` in 0-255 (or 0-1 when every channel is
/// a fraction written with a decimal point).
fn parse_color(text: &str) -> Result<Color, String> {
    let t = text.trim();
    let hex = t.trim_start_matches('#');
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
        return Ok(Color::srgb_u8(b(0), b(2), b(4)));
    }
    let parts: Vec<&str> = t.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.len() < 3 {
        return Err(format!("expected r, g, b, got {t:?}"));
    }
    let mut ch = [0f32; 3];
    for (i, p) in parts.iter().take(3).enumerate() {
        ch[i] = p.parse::<f32>().map_err(|_| format!("bad colour channel {p:?}"))?;
        if !ch[i].is_finite() {
            return Err("bad colour channel".into());
        }
    }
    let fractions = parts.iter().take(3).all(|p| p.contains('.')) && ch.iter().all(|c| *c <= 1.0);
    let scale = if fractions { 1.0 } else { 1.0 / 255.0 };
    Ok(Color::srgb(
        (ch[0] * scale).clamp(0.0, 1.0),
        (ch[1] * scale).clamp(0.0, 1.0),
        (ch[2] * scale).clamp(0.0, 1.0),
    ))
}

/// A cone angle in degrees: 1 to 180 (a SurfaceLight's saved 0 reads as
/// "unset", so the panel never writes one).
fn parse_angle(text: &str) -> Result<f32, String> {
    Ok(parse_non_negative(text)?.clamp(1.0, 180.0))
}

impl LightFields for EustressPointLight {
    const ROWS: &'static [(&'static str, &'static str)] = &[
        ("Enabled", "bool"),
        ("Brightness", "float"),
        ("Color", "color"),
        ("Range", "float"),
        ("Shadows", "bool"),
        ("Radius", "float"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Brightness" => fmt_f32(self.brightness),
            "Color" => color_text(self.color),
            "Range" => fmt_f32(self.range),
            "Shadows" => self.shadows.to_string(),
            "Radius" => fmt_f32(self.radius),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Brightness" => self.brightness = parse_non_negative(text)?,
            "Color" => self.color = parse_color(text)?,
            "Range" => self.range = parse_non_negative(text)?,
            "Shadows" => self.shadows = parse_bool(text)?,
            "Radius" => self.radius = parse_non_negative(text)?,
            _ => return Err(format!("PointLight has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        light_section_table(&LightSectionRef::Point(self))
    }
}

impl LightFields for EustressSpotLight {
    const ROWS: &'static [(&'static str, &'static str)] = &[
        ("Enabled", "bool"),
        ("Brightness", "float"),
        ("Color", "color"),
        ("Range", "float"),
        ("Angle", "float"),
        ("Face", "choice"),
        ("Shadows", "bool"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Brightness" => fmt_f32(self.brightness),
            "Color" => color_text(self.color),
            "Range" => fmt_f32(self.range),
            "Angle" => fmt_f32(self.angle),
            "Face" => normalize_face(&self.face).to_string(),
            "Shadows" => self.shadows.to_string(),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Brightness" => self.brightness = parse_non_negative(text)?,
            "Color" => self.color = parse_color(text)?,
            "Range" => self.range = parse_non_negative(text)?,
            "Angle" => self.angle = parse_angle(text)?,
            "Face" => self.face = normalize_face(text).to_string(),
            "Shadows" => self.shadows = parse_bool(text)?,
            _ => return Err(format!("SpotLight has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        light_section_table(&LightSectionRef::Spot(self))
    }
}

impl LightFields for SurfaceLight {
    const ROWS: &'static [(&'static str, &'static str)] = &[
        ("Enabled", "bool"),
        ("Brightness", "float"),
        ("Color", "color"),
        ("Range", "float"),
        ("Angle", "float"),
        ("Face", "choice"),
        ("Shadows", "bool"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Brightness" => fmt_f32(self.brightness),
            "Color" => color_text(self.color),
            "Range" => fmt_f32(self.range),
            "Angle" => fmt_f32(self.angle),
            "Face" => normalize_face(&self.face).to_string(),
            "Shadows" => self.shadows.to_string(),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Brightness" => self.brightness = parse_non_negative(text)?,
            "Color" => self.color = parse_color(text)?,
            "Range" => self.range = parse_non_negative(text)?,
            "Angle" => self.angle = parse_angle(text)?,
            "Face" => self.face = normalize_face(text).to_string(),
            "Shadows" => self.shadows = parse_bool(text)?,
            _ => return Err(format!("SurfaceLight has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        light_section_table(&LightSectionRef::Surface(self))
    }
}

impl LightFields for EustressDirectionalLight {
    const ROWS: &'static [(&'static str, &'static str)] = &[
        ("Enabled", "bool"),
        ("Brightness", "float"),
        ("Color", "color"),
        ("Shadows", "bool"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Brightness" => fmt_f32(self.brightness),
            "Color" => color_text(self.color),
            "Shadows" => self.shadows.to_string(),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Brightness" => self.brightness = parse_non_negative(text)?,
            "Color" => self.color = parse_color(text)?,
            "Shadows" => self.shadows = parse_bool(text)?,
            _ => return Err(format!("DirectionalLight has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        light_section_table(&LightSectionRef::Directional(self))
    }
}

/// Lengths are shown in the display unit and stored in metres.
fn is_length(key: &str) -> bool {
    matches!(key, "Range" | "Radius")
}

// ============================================================================
// Rows
// ============================================================================

/// Read access for building rows. Like [`EditQueries`] it holds no
/// resources; the caller passes the display unit.
#[derive(SystemParam)]
pub struct PanelQueries<'w, 's> {
    lights: Query<
        'w,
        's,
        (
            Option<&'static EustressPointLight>,
            Option<&'static EustressSpotLight>,
            Option<&'static SurfaceLight>,
            Option<&'static EustressDirectionalLight>,
        ),
    >,
}

fn rows_of<T: LightFields>(light: &T, unit: Unit) -> Vec<(&'static str, &'static str, String, &'static str)> {
    T::ROWS
        .iter()
        .filter_map(|(name, kind)| {
            let mut text = light.get_text(name)?;
            if is_length(name) && unit != ENGINE_NATIVE_UNIT {
                if let Ok(m) = text.parse::<f32>() {
                    text = fmt_f32(convert_f32(m, ENGINE_NATIVE_UNIT, unit));
                }
            }
            Some((LIGHT_CATEGORY, *name, text, *kind))
        })
        .collect()
}

/// Property rows `(category, name, value, kind)` for a light, from its live
/// component, with lengths in the display `unit`. `None` for any other
/// class, or a light whose component has not been attached yet.
pub fn light_rows(
    class: ClassName,
    entity: Entity,
    unit: Unit,
    queries: &PanelQueries,
) -> Option<Vec<(&'static str, &'static str, String, &'static str)>> {
    if !is_light_class(class) {
        return None;
    }
    let (point, spot, surface, directional) = queries.lights.get(entity).ok()?;
    if let Some(l) = point {
        return Some(rows_of(l, unit));
    }
    if let Some(l) = spot {
        return Some(rows_of(l, unit));
    }
    if let Some(l) = surface {
        return Some(rows_of(l, unit));
    }
    directional.map(|l| rows_of(l, unit))
}

// ============================================================================
// Edits
// ============================================================================

/// Write access for panel edits. It holds no resources: the drain system
/// that owns it already holds `DisplayUnit` mutably, and a second access to
/// it in one system is a startup panic (B0002). The caller passes the unit.
#[derive(SystemParam)]
pub struct EditQueries<'w, 's> {
    lights: Query<
        'w,
        's,
        (
            Option<&'static mut EustressPointLight>,
            Option<&'static mut EustressSpotLight>,
            Option<&'static mut SurfaceLight>,
            Option<&'static mut EustressDirectionalLight>,
        ),
    >,
}

/// What a panel edit did, for the Output panel and the undo stack.
pub struct EditOutcome {
    pub message: String,
    pub undo: Option<crate::undo::Action>,
}

/// Apply a Properties edit if `key` is a field of the selected light. `None`
/// means "not ours" (Name, Position, Rotation, ...): the generic handler
/// takes it. `unit` is the display unit lengths are typed in.
pub fn handle_edit(
    entity: Entity,
    key: &str,
    raw: &str,
    unit: Unit,
    toml_path: Option<&Path>,
    queries: &mut EditQueries,
) -> Option<Result<EditOutcome, String>> {
    let (point, spot, surface, directional) = queries.lights.get_mut(entity).ok()?;
    if let Some(mut c) = point {
        return apply(&mut *c, key, raw, unit, toml_path);
    }
    if let Some(mut c) = spot {
        return apply(&mut *c, key, raw, unit, toml_path);
    }
    if let Some(mut c) = surface {
        return apply(&mut *c, key, raw, unit, toml_path);
    }
    if let Some(mut c) = directional {
        return apply(&mut *c, key, raw, unit, toml_path);
    }
    None
}

fn apply<T: LightFields>(
    light: &mut T,
    key: &str,
    raw: &str,
    unit: Unit,
    toml_path: Option<&Path>,
) -> Option<Result<EditOutcome, String>> {
    if !T::ROWS.iter().any(|(name, _)| *name == key) {
        return None;
    }
    // Lengths arrive in the display unit; the component and file hold metres.
    let text = if is_length(key) && unit != ENGINE_NATIVE_UNIT {
        match raw.trim().parse::<f32>() {
            Ok(v) => fmt_f32(convert_f32(v, unit, ENGINE_NATIVE_UNIT)),
            Err(_) => return Some(Err(format!("{key}: expected a number, got {:?}", raw.trim()))),
        }
    } else {
        raw.to_string()
    };
    let old_text = light.get_text(key).unwrap_or_default();
    if let Err(e) = light.set_text(key, &text) {
        return Some(Err(format!("{key}: {e}")));
    }
    let new_text = light.get_text(key).unwrap_or_default();
    if new_text == old_text {
        return Some(Ok(EditOutcome { message: String::new(), undo: None }));
    }
    let mut undo = None;
    if let Some(path) = toml_path {
        if let Err(e) = save_light_section(path, light.section()) {
            return Some(Err(format!("{key} changed but was not saved: {e}")));
        }
        undo = Some(crate::undo::Action::ChangeClassField {
            toml_path: path.to_path_buf(),
            property: key.to_string(),
            old_text,
            new_text: new_text.clone(),
        });
    }
    Some(Ok(EditOutcome { message: format!("{key} = {new_text}"), undo }))
}

/// Write a light's `[light]` section into its instance file: the WorldDb
/// copy when a DB is active (authoritative on migrated Spaces) and the disk
/// mirror. Every other section is preserved; the importer's legacy
/// `light_*` extras are removed so `[light]` is the only source.
pub fn save_light_section(toml_path: &Path, section: toml::value::Table) -> Result<(), String> {
    let text = match crate::space::active_db::get_instance_text(toml_path) {
        Some(t) => t,
        None => std::fs::read_to_string(toml_path).map_err(|e| format!("read {}: {e}", toml_path.display()))?,
    };
    let mut doc: toml::Value = text.parse().map_err(|e| format!("parse {}: {e}", toml_path.display()))?;
    if !doc.is_table() {
        return Err(format!("{} is not a TOML table", toml_path.display()));
    }
    eustress_common::plugins::light_classes::store_light_section(&mut doc, section);
    let out = toml::to_string_pretty(&doc).map_err(|e| format!("serialize {}: {e}", toml_path.display()))?;
    let db_ok = crate::space::active_db::put_instance_text(toml_path, &out);
    if let Err(e) = crate::space::gui_loader::write_atomic(toml_path, out.as_bytes()) {
        if !db_ok {
            return Err(format!("write {}: {e}", toml_path.display()));
        }
    }
    Ok(())
}

/// Set one field from its stored text form on the light whose file is
/// `toml_path`, and save the section: how the undo stack's
/// `ChangeClassField` replays a light edit. `None` when the entity is not a
/// light; otherwise the parse result, and inside it the save result.
pub fn set_field_text(
    world: &mut World,
    entity: Entity,
    toml_path: &Path,
    property: &str,
    text: &str,
) -> Option<Result<Option<Result<(), String>>, String>> {
    fn set<T: LightFields + Component<Mutability = Mutable>>(
        world: &mut World,
        entity: Entity,
        toml_path: &Path,
        property: &str,
        text: &str,
    ) -> Option<Result<Option<Result<(), String>>, String>> {
        let mut component = world.get_mut::<T>(entity)?;
        Some(
            component
                .set_text(property, text)
                .map(|_| Some(save_light_section(toml_path, component.section()))),
        )
    }
    set::<EustressPointLight>(world, entity, toml_path, property, text)
        .or_else(|| set::<EustressSpotLight>(world, entity, toml_path, property, text))
        .or_else(|| set::<SurfaceLight>(world, entity, toml_path, property, text))
        .or_else(|| set::<EustressDirectionalLight>(world, entity, toml_path, property, text))
}

// ============================================================================
// Insert placement
// ============================================================================

/// What the insert knows about the selection it lands next to.
pub struct InsertContext {
    /// The Space root.
    pub space_root: PathBuf,
    /// The selected instance's own folder, when it is folder-form on disk.
    pub selected_folder: Option<PathBuf>,
    /// The selected instance is a part.
    pub selected_is_part: bool,
    /// The selected instance is a Folder or Model.
    pub selected_is_container: bool,
    /// The selected instance's world position.
    pub selected_position: Option<Vec3>,
    /// The editor camera.
    pub camera: Option<GlobalTransform>,
}

/// Where a new light goes: its folder and its placement.
pub struct InsertPlan {
    pub dir: PathBuf,
    pub overrides: InstanceOverrides,
}

/// The light class an Insert action names: the ribbon's lowercase ids and
/// the Insert dialog's class names alike.
pub fn insert_action_class(action: &str) -> Option<&'static str> {
    let class = action.strip_prefix("insert:")?;
    match class.to_ascii_lowercase().as_str() {
        "pointlight" => Some("PointLight"),
        "spotlight" => Some("SpotLight"),
        "surfacelight" => Some("SurfaceLight"),
        "directionallight" => Some("DirectionalLight"),
        _ => None,
    }
}

/// A light's default aim when placed on its own. Spots look down;
/// a DirectionalLight comes in 60 degrees below the horizon.
fn default_aim(class: ClassName) -> Option<[f32; 4]> {
    match class {
        // -90 degrees about X: -Z onto -Y.
        ClassName::SpotLight | ClassName::SurfaceLight => {
            Some([-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2])
        }
        // -60 degrees about X.
        ClassName::DirectionalLight => Some([-0.5, 0.0, 0.0, 0.866_025_4]),
        _ => None,
    }
}

/// Plan a light insert the way Roblox places lights:
///
/// - **A part is selected** and has a folder: the light goes inside it and
///   shines from it (a SpotLight or SurfaceLight out of its Front face).
/// - **A part with no folder** (a binary-ECS part) is selected: the light
///   goes in `Workspace/` at the part's centre, aimed down.
/// - **A Folder or Model is selected**: inside it, in front of the camera.
/// - **Otherwise**: `Workspace/`, in front of the camera, at least 2 m up.
///
/// A light used to land at the world origin, usually inside the baseplate,
/// and in `Lighting/` from the Toolbox, so inserting one seemed to do nothing.
pub fn plan_insert(class: ClassName, ctx: &InsertContext) -> InsertPlan {
    let workspace = ctx.space_root.join("Workspace");
    let camera_spot = || -> Vec3 {
        match ctx.camera {
            Some(cam) => {
                let p = cam.translation() + cam.forward() * 8.0;
                Vec3::new(p.x, p.y.max(2.0), p.z)
            }
            None => Vec3::new(0.0, 4.0, 0.0),
        }
    };
    if ctx.selected_is_part {
        if let Some(folder) = ctx.selected_folder.clone() {
            return InsertPlan { dir: folder, overrides: InstanceOverrides::default() };
        }
        let at = ctx.selected_position.unwrap_or_else(camera_spot);
        return InsertPlan {
            dir: workspace,
            overrides: InstanceOverrides {
                position: Some(at),
                rotation: default_aim(class),
                ..Default::default()
            },
        };
    }
    let dir = match (&ctx.selected_folder, ctx.selected_is_container) {
        (Some(folder), true) => folder.clone(),
        _ => workspace,
    };
    InsertPlan {
        dir,
        overrides: InstanceOverrides {
            position: Some(camera_spot()),
            rotation: default_aim(class),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_every_form_the_panel_sends() {
        let c = parse_color("255, 128, 0").unwrap().to_srgba();
        assert!((c.green - 128.0 / 255.0).abs() < 1e-6);
        let c = parse_color("#00FF80").unwrap().to_srgba();
        assert!((c.green - 1.0).abs() < 1e-6);
        let c = parse_color("0.5, 0.25, 1.0").unwrap().to_srgba();
        assert!((c.red - 0.5).abs() < 1e-6);
        assert!(parse_color("red").is_err());
    }

    #[test]
    fn a_field_round_trips_through_its_text() {
        let mut spot = EustressSpotLight::default();
        spot.set_text("Face", "Enum.NormalId.Bottom").unwrap();
        assert_eq!(spot.get_text("Face").as_deref(), Some("Bottom"));
        spot.set_text("Angle", "400").unwrap();
        assert_eq!(spot.angle, 180.0);
        spot.set_text("Brightness", "-3").unwrap();
        assert_eq!(spot.brightness, 0.0);
        assert!(spot.set_text("Radius", "1").is_err());
    }

    #[test]
    fn a_light_inside_a_part_goes_into_its_folder() {
        let ctx = InsertContext {
            space_root: PathBuf::from("S"),
            selected_folder: Some(PathBuf::from("S/Workspace/Lamp")),
            selected_is_part: true,
            selected_is_container: false,
            selected_position: Some(Vec3::new(1.0, 2.0, 3.0)),
            camera: None,
        };
        let plan = plan_insert(ClassName::SpotLight, &ctx);
        assert_eq!(plan.dir, PathBuf::from("S/Workspace/Lamp"));
        assert!(plan.overrides.position.is_none());
    }

    #[test]
    fn a_free_light_goes_in_front_of_the_camera_off_the_ground() {
        let ctx = InsertContext {
            space_root: PathBuf::from("S"),
            selected_folder: None,
            selected_is_part: false,
            selected_is_container: false,
            selected_position: None,
            camera: Some(GlobalTransform::from(
                Transform::from_xyz(0.0, 1.0, 0.0).looking_to(Vec3::NEG_Z, Vec3::Y),
            )),
        };
        let plan = plan_insert(ClassName::PointLight, &ctx);
        assert_eq!(plan.dir, PathBuf::from("S").join("Workspace"));
        let p = plan.overrides.position.unwrap();
        assert!((p[2] + 8.0).abs() < 1e-4);
        assert!(p[1] >= 2.0);
    }
}
