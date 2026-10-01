//! # Reading a Eustress Space's geometry — the shared subset
//!
//! ## Scope, stated up front
//!
//! This is **not** the full Space loader. Studio's `instance_loader` is ~3,900
//! lines covering CAD, GUI, terrain, decals, splats, nuclear state, Draco
//! meshes, streaming and editor write-back. None of that can move into
//! `eustress-common` without dragging Slint, `worlddb` and `radiance` with it.
//!
//! What this reads is the subset a *movement* Space is made of: BaseParts
//! (`Part`, `SpawnLocation`, `Seat`, `WedgePart`, ...) with a transform, a
//! size, a shape, a colour, a material preset, transparency and collision.
//! That is enough for the Client to open a real
//! Space from the real on-disk format and play it, which is the thing that was
//! impossible before — `space_fetch` unpacked an archive and nothing consumed
//! the result.
//!
//! Anything outside the subset is **skipped and counted**, never guessed at.
//! [`SpaceGeometry::skipped`] reports what was ignored so a Space that quietly
//! half-loads is distinguishable from one that loaded.
//!
//! ## The format, as it is on disk
//!
//! * A Space root holds service directories: `Workspace/`, `Lighting/`, …
//! * Geometry lives under `Workspace/`.
//! * **Hierarchy is directory nesting.** A directory containing
//!   `_instance.toml` *is* that instance; a directory without one is a plain
//!   folder whose children are still nested under it. There is no `parent`
//!   field anywhere.
//! * A flat `<Name>.instance.toml` beside a directory is the same thing in
//!   one file. When both forms exist for one name the folder form wins, which
//!   is what the engine loader does — otherwise the entity spawns twice.
//! * `[transform] scale` is **the part's size**, not a multiplier, in the unit
//!   `[metadata] unit` names (metres when it names none). The reader converts
//!   position and size to metres, as Studio's loader does.
//! * A Part's shape is **the name of its `[asset] mesh`**, one of the engine's
//!   primitives in `engine/assets/parts/` (`parts/ball.glb`, `parts/wedge.glb`,
//!   ...). There is no separate shape property. A Part with no `[asset]` is a
//!   block, as it is in the engine loader.
//!
//! ## Drawn as Studio draws it
//!
//! [`primitive_mesh`] builds each primitive at unit size with the extents,
//! orientation and tessellation of the engine's GLB, [`primitive_collider`]
//! gives it the collider Studio gives it, and [`part_material`] is Studio's
//! material for a part with no `.mat.toml` of its own. Custom meshes (drawn
//! as blocks at their size), the `.mat.toml` material library and every class
//! that is not a BasePart are outside the subset.
//!
//! ## The colour trap
//!
//! `[properties] color` is an array that may be 3 or 4 long and may be either
//! 0-255 integers or 0-1 floats. The rule — taken from the engine loader, not
//! invented here — is that an **all-integer** array means 0-255 and anything
//! containing a float means 0-1. A single `1.0` in an otherwise integer array
//! therefore reinterprets the whole thing, which is silent and total.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use serde::Deserialize;

use crate::classes::{Material as MaterialPreset, PartType};
use crate::datamodel::{is_base_part, DataModel, DmValue, InstanceId};

/// One `Part` read out of a Space.
#[derive(Debug, Clone)]
pub struct SpacePart {
    pub name: String,
    pub class_name: String,
    /// World transform, composed through directory nesting. `scale` carries
    /// the part's size in metres.
    pub transform: Transform,
    pub color: Color,
    pub material: String,
    /// The primitive the part is drawn as, named by its `[asset] mesh`.
    pub shape: PartType,
    /// The `[asset] mesh` when it names a custom mesh rather than one of the
    /// engine's primitives. Such a part is drawn as a block at its size, the
    /// engine's own fallback for a mesh it cannot load.
    pub custom_mesh: Option<String>,
    pub anchored: bool,
    pub can_collide: bool,
    pub transparency: f32,
    pub reflectance: f32,
    /// The nearest `Climbable` attribute on the part or on a Model or Folder
    /// above it, if any.
    pub climbable: Option<bool>,
    /// Inside a Model that holds a `Humanoid` or an `AnimationController`: a
    /// character's or an NPC's part, which characters do not climb unless an
    /// attribute says so. The tree reader leaves it to the tree's apply step,
    /// which knows every rig in the tree at once.
    pub in_character: bool,
}

/// Everything the shared reader understood, plus an honest account of what it
/// did not.
#[derive(Debug, Clone, Default)]
pub struct SpaceGeometry {
    pub parts: Vec<SpacePart>,
    /// Instance files skipped, keyed by `class_name`, with counts: everything
    /// that is not a BasePart. A movement Space is all parts; a class here is
    /// content the Client is not showing.
    pub skipped: BTreeMap<String, usize>,
    /// Files that failed to parse, with the reason. Never silently dropped:
    /// a Space that half-loads must be distinguishable from one that loaded.
    pub errors: Vec<String>,
}

impl SpaceGeometry {
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }

    /// Parts drawn as blocks because their mesh is a custom one.
    pub fn custom_mesh_parts(&self) -> usize {
        self.parts.iter().filter(|p| p.custom_mesh.is_some()).count()
    }

    /// Where a character's feet go on the Space's first SpawnLocation: on top
    /// of the pad, a little above it, by the rule Studio's Play uses.
    pub fn spawn_point(&self) -> Option<Vec3> {
        let pad = self.parts.iter().find(|p| p.class_name == "SpawnLocation")?;
        Some(crate::services::player::spawn_feet_on(&pad.transform))
    }
}

/// Where a character's feet go on the first SpawnLocation under the tree's
/// Workspace, by the rule `SpaceGeometry::spawn_point` uses for files.
pub fn tree_spawn_point(dm: &DataModel) -> Option<Vec3> {
    let ws = dm.find_service("Workspace")?;
    dm.descendants(ws)
        .into_iter()
        .filter(|&id| dm.class_of(id) == Some("SpawnLocation"))
        .find_map(|id| SpacePart::from_tree(dm, id))
        .map(|pad| crate::services::player::spawn_feet_on(&pad.transform))
}

// ── The same part in the DataModel tree ────────────────────────────────────

impl SpacePart {
    /// The part an instance in the tree describes, read as Studio's apply
    /// step reads it. A property the instance lacks takes a Part's default. A
    /// WedgePart or CornerWedgePart is always its own shape, a `MeshId` naming
    /// one of the engine's primitives sets the shape, and any other `MeshId`
    /// is a custom mesh drawn as a block. The pose is cleaned as the reader
    /// cleans it. `None` for anything that is not a BasePart, and for the
    /// Terrain, which is not drawn as a part.
    pub fn from_tree(dm: &DataModel, id: InstanceId) -> Option<SpacePart> {
        let inst = dm.get(id)?;
        let class = inst.class_name.as_str();
        if !is_base_part(class) || class == "Terrain" {
            return None;
        }
        let get = |k: &str| inst.props.get(k);
        let number = |k: &str, default: f32| get(k).and_then(DmValue::as_number).map_or(default, |n| n as f32);
        let flag = |k: &str, default: bool| get(k).and_then(DmValue::as_bool).unwrap_or(default);

        let named = match class {
            "WedgePart" => PartType::Wedge,
            "CornerWedgePart" => PartType::CornerWedge,
            _ => get("Shape")
                .and_then(DmValue::as_enum_name)
                .and_then(PartType::from_str)
                .unwrap_or(PartType::Block),
        };
        let mesh_id = get("MeshId").and_then(DmValue::as_str).map(str::trim).filter(|m| !m.is_empty());
        let (shape, custom_mesh) = match mesh_id {
            Some(mesh) => match primitive_shape(mesh) {
                Some(shape) => (shape, None),
                None => (PartType::Block, Some(mesh.to_owned())),
            },
            None => (named, None),
        };

        let size = get("Size").and_then(DmValue::as_vector3).map_or(Vec3::new(4.0, 1.0, 2.0), |v| v.to_vec3());
        let pose = get("CFrame").and_then(DmValue::as_cframe).unwrap_or_default().to_transform();
        let r = pose.rotation;
        Some(SpacePart {
            name: inst.name.clone(),
            class_name: inst.class_name.clone(),
            transform: clean_pose(pose.translation.to_array(), [r.x, r.y, r.z, r.w], size.to_array()),
            color: get("Color")
                .and_then(DmValue::as_color3)
                .map_or(Color::srgb_u8(163, 162, 165), |c| Color::srgb(c.r as f32, c.g as f32, c.b as f32)),
            material: get("Material").and_then(DmValue::as_enum_name).unwrap_or("Plastic").to_owned(),
            shape,
            custom_mesh,
            anchored: flag("Anchored", false),
            can_collide: flag("CanCollide", true),
            transparency: number("Transparency", 0.0).clamp(0.0, 1.0),
            reflectance: number("Reflectance", 0.0),
            climbable: nearest_climbable(dm, id),
            in_character: false,
        })
    }
}

/// The nearest `Climbable` attribute on an instance or above it.
fn nearest_climbable(dm: &DataModel, id: InstanceId) -> Option<bool> {
    let mut at = Some(id);
    // Bounded, as the tree's own ancestry walks are.
    for _ in 0..4096 {
        let here = at?;
        let set = dm.get_attribute(here, crate::attributes::CLIMBABLE_ATTRIBUTE).and_then(|v| v.as_bool());
        if set.is_some() {
            return set;
        }
        at = dm.parent(here);
    }
    None
}

/// A shape's name in the tree's `PartType` enum.
pub fn shape_name(shape: PartType) -> &'static str {
    match shape {
        PartType::Block => "Block",
        PartType::Ball => "Ball",
        PartType::Cylinder => "Cylinder",
        PartType::Wedge => "Wedge",
        PartType::CornerWedge => "CornerWedge",
        PartType::Cone => "Cone",
    }
}

// ── Wire types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct InstanceFile {
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    asset: Option<AssetBlock>,
    #[serde(default)]
    transform: TransformBlock,
    #[serde(default)]
    properties: Properties,
    #[serde(default)]
    attributes: toml::Table,
}

impl InstanceFile {
    fn class(&self) -> &str {
        self.metadata.class_name.as_deref().unwrap_or("Part")
    }

    /// Its own `Climbable` attribute, if it sets one.
    fn climbable(&self) -> Option<bool> {
        self.attributes.get(crate::attributes::CLIMBABLE_ATTRIBUTE).and_then(toml::Value::as_bool)
    }

    /// A `Humanoid` or an `AnimationController`: what makes its parent a
    /// character.
    fn is_rig(&self) -> bool {
        matches!(self.class(), "Humanoid" | "AnimationController")
    }
}

/// What a folder hands down to everything inside it, and whether the folder
/// is itself a Model, the only thing a rig makes a character.
#[derive(Debug, Clone, Copy, Default)]
struct Inherited {
    climbable: Option<bool>,
    in_character: bool,
    is_model: bool,
}

/// `[asset]`: the mesh a part draws, and so, for a primitive, its shape.
#[derive(Debug, Deserialize, Default)]
struct AssetBlock {
    #[serde(default)]
    mesh: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Metadata {
    #[serde(default)]
    class_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// The unit the file's lengths are in (`"m"`, `"ft"`, `"studs"`, ...).
    #[serde(default)]
    unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransformBlock {
    #[serde(default = "zero3")]
    position: [f32; 3],
    #[serde(default = "ident_quat")]
    rotation: [f32; 4],
    #[serde(default = "one3")]
    scale: [f32; 3],
}

impl Default for TransformBlock {
    fn default() -> Self {
        Self { position: zero3(), rotation: ident_quat(), scale: one3() }
    }
}

fn zero3() -> [f32; 3] {
    [0.0; 3]
}
fn one3() -> [f32; 3] {
    [1.0; 3]
}
fn ident_quat() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

#[derive(Debug, Deserialize, Default)]
struct Properties {
    #[serde(default)]
    color: Option<Vec<toml::Value>>,
    #[serde(default = "yes")]
    anchored: bool,
    #[serde(default = "yes")]
    can_collide: bool,
    #[serde(default)]
    transparency: f32,
    #[serde(default)]
    reflectance: f32,
    #[serde(default)]
    material: Option<String>,
}

fn yes() -> bool {
    true
}

/// Decode the colour array under the engine's own rule.
pub fn decode_color(raw: &[toml::Value]) -> Color {
    if raw.len() < 3 {
        return Color::srgb(0.6, 0.6, 0.6);
    }
    // All-integer means 0-255; ANY float means 0-1. Mixing the two silently
    // reinterprets every channel, so the test is over the whole array.
    let all_int = raw.iter().all(|v| v.as_integer().is_some());
    let f = |i: usize| -> f32 {
        raw.get(i)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
            .unwrap_or(0.0) as f32
    };
    let div = if all_int { 255.0 } else { 1.0 };
    let a = if raw.len() >= 4 { f(3) / div } else { 1.0 };
    Color::srgba(f(0) / div, f(1) / div, f(2) / div, a)
}

/// A part's transform from the numbers in its file, which are in the unit its
/// `[metadata] unit` names: position and size converted to metres, then
/// cleaned by [`clean_pose`]. Studio's loader converts exactly this way with
/// `units_v1` on (the engine's default), and reads every file in metres
/// without it, so this does too. A missing or unknown unit is metres.
pub fn authored_pose(position: [f32; 3], rotation: [f32; 4], size: [f32; 3], unit: Option<&str>) -> Transform {
    #[cfg(feature = "units_v1")]
    let (position, size) = match unit.and_then(crate::units::Unit::from_symbol) {
        Some(u) => (
            crate::units::authored_to_engine_vec3_f32(position, u),
            crate::units::authored_to_engine_vec3_f32(size, u),
        ),
        None => (position, size),
    };
    #[cfg(not(feature = "units_v1"))]
    let _ = unit;
    clean_pose(position, rotation, size)
}

/// A pose and size as a part's transform (`scale` is the size), cleaned the
/// way the engine loader cleans them, since Avian panics on a non-finite
/// position or rotation and on a collider whose extent is zero or negative.
/// A non-finite position component becomes 0, a zero-length or non-finite
/// rotation becomes the identity, and each size becomes its absolute value,
/// at least one micrometre.
pub fn clean_pose(position: [f32; 3], rotation: [f32; 4], size: [f32; 3]) -> Transform {
    let finite_or = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let [x, y, z, w] = rotation;
    let q = Quat::from_xyzw(x, y, z, w);
    let rotation = if rotation.iter().all(|c| c.is_finite()) && q.length_squared() >= 1e-8 {
        q.normalize()
    } else {
        Quat::IDENTITY
    };
    const MIN_SIZE: f32 = 1.0e-6;
    Transform {
        translation: Vec3::from_array(position.map(|v| finite_or(v, 0.0))),
        rotation,
        scale: Vec3::from_array(size.map(|v| finite_or(v, MIN_SIZE).abs().max(MIN_SIZE))),
    }
}

// ── The walk ───────────────────────────────────────────────────────────────

/// Read every `Part` under `<space_root>/Workspace`.
///
/// Returns `Err` only when the Space itself is unusable; per-file problems are
/// collected into [`SpaceGeometry::errors`] so one malformed part cannot cost
/// you the whole course.
pub fn read_space_parts(space_root: &Path) -> Result<SpaceGeometry, String> {
    let workspace = space_root.join("Workspace");
    if !workspace.is_dir() {
        return Err(format!(
            "not a Space: {} has no Workspace/ directory",
            space_root.display()
        ));
    }
    let mut out = SpaceGeometry::default();
    walk(&workspace, Transform::IDENTITY, Inherited::default(), &mut out);
    Ok(out)
}

/// Compose a child transform onto a parent.
///
/// Deliberately NOT `Transform::mul_transform`: `scale` here is a *size in
/// metres*, not a multiplier, so scale must not propagate to children. Only
/// position and rotation compose.
fn compose(parent: &Transform, child: &Transform) -> Transform {
    Transform {
        translation: parent.translation + parent.rotation * child.translation,
        rotation: parent.rotation * child.rotation,
        scale: child.scale,
    }
}

fn walk(dir: &Path, parent: Transform, inherited: Inherited, out: &mut SpaceGeometry) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        out.errors.push(format!("unreadable directory: {}", dir.display()));
        return;
    };

    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut flat: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            dirs.push(p);
        } else if name.ends_with(".instance.toml") {
            flat.push(p);
        }
    }

    // Folder form beats flat form for the same name, matching the engine —
    // otherwise a Space carrying both spawns the entity twice.
    let dir_names: Vec<String> = dirs
        .iter()
        .filter_map(|d| d.file_name()?.to_str().map(str::to_owned))
        .collect();

    // Every direct child is parsed before any of it is read: a Humanoid or an
    // AnimationController among them makes this a character, and that is a
    // fact about all of its parts.
    let flat: Vec<(String, PathBuf, Option<InstanceFile>)> = flat
        .into_iter()
        .filter_map(|f| {
            let stem = f
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.trim_end_matches(".instance.toml").to_owned())
                .unwrap_or_default();
            if dir_names.iter().any(|d| *d == stem) {
                return None;
            }
            let file = parse(&f, out);
            Some((stem, f, file))
        })
        .collect();
    let dirs: Vec<(String, PathBuf, Option<InstanceFile>)> = dirs
        .into_iter()
        .map(|d| {
            let name = d.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_owned();
            let inst = d.join("_instance.toml");
            let file = inst.is_file().then(|| parse(&inst, out)).flatten();
            (name, d, file)
        })
        .collect();
    let rigged = inherited.is_model
        && flat.iter().chain(&dirs).any(|(_, _, f)| f.as_ref().is_some_and(InstanceFile::is_rig));
    let here = Inherited {
        climbable: inherited.climbable,
        in_character: inherited.in_character || rigged,
        is_model: false,
    };

    for (stem, path, file) in &flat {
        if let Some(file) = file {
            ingest(file, path, stem, &parent, here, out);
        }
    }

    for (name, d, file) in dirs {
        let (world, inner) = match &file {
            Some(file) => (
                ingest(file, &d.join("_instance.toml"), &name, &parent, here, out),
                Inherited {
                    climbable: file.climbable().or(here.climbable),
                    is_model: file.class() == "Model",
                    ..here
                },
            ),
            // A plain directory is a folder: no transform of its own, but its
            // children still nest under whatever contains it.
            None => (parent, here),
        };
        walk(&d, world, inner, out);
    }
}

/// Read one instance file, recording why when it cannot be read.
fn parse(path: &Path, out: &mut SpaceGeometry) -> Option<InstanceFile> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            out.errors.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    match toml::from_str(&text) {
        Ok(f) => Some(f),
        Err(e) => {
            out.errors.push(format!("{}: {e}", path.display()));
            None
        }
    }
}

/// Take in one parsed instance file. Returns the composed world transform so
/// children can nest under it.
fn ingest(
    file: &InstanceFile,
    path: &Path,
    fallback_name: &str,
    parent: &Transform,
    inherited: Inherited,
    out: &mut SpaceGeometry,
) -> Transform {
    let unit = file.metadata.unit.as_deref();
    if let Some(symbol) = unit.filter(|s| crate::units::Unit::from_symbol(s).is_none()) {
        out.errors.push(format!("{}: unknown unit {symbol:?}, read as metres", path.display()));
    }
    let t = &file.transform;
    let world = compose(parent, &authored_pose(t.position, t.rotation, t.scale, unit));
    let class = file.class().to_owned();

    // Every BasePart draws as a part (a SpawnLocation, a Seat, a WedgePart,
    // ...), as in the engine loader; the Terrain is drawn by the terrain.
    if !is_base_part(&class) || class == "Terrain" {
        *out.skipped.entry(class).or_insert(0) += 1;
        return world;
    }

    let (shape, custom_mesh) = match file.asset.as_ref().and_then(|a| a.mesh.as_deref()) {
        None => (PartType::Block, None),
        Some(mesh) => match primitive_shape(mesh) {
            Some(shape) => (shape, None),
            None => (PartType::Block, Some(mesh.to_owned())),
        },
    };
    let p = &file.properties;
    out.parts.push(SpacePart {
        name: file.metadata.name.clone().unwrap_or_else(|| fallback_name.to_owned()),
        class_name: class,
        transform: world,
        color: p.color.as_deref().map(decode_color).unwrap_or(Color::srgb(0.6, 0.6, 0.6)),
        material: p.material.clone().unwrap_or_else(|| "Plastic".into()),
        shape,
        custom_mesh,
        anchored: p.anchored,
        can_collide: p.can_collide,
        transparency: p.transparency,
        reflectance: p.reflectance,
        climbable: file.climbable().or(inherited.climbable),
        in_character: inherited.in_character,
    });
    world
}

// ── Shapes, meshes and materials ───────────────────────────────────────────

/// The engine's primitive meshes by the shape each one is. ORDER MATTERS, as
/// in the engine loader's `PRIMITIVE_MESHES`: the first hint found in the
/// file name wins, and `corner_wedge` contains `wedge`.
const PRIMITIVE_HINTS: &[(&str, PartType)] = &[
    ("corner_wedge", PartType::CornerWedge),
    ("block", PartType::Block),
    ("ball", PartType::Ball),
    ("cylinder", PartType::Cylinder),
    ("wedge", PartType::Wedge),
    ("cone", PartType::Cone),
];

/// The primitive an `[asset] mesh` names, read as the engine loader reads it:
/// by the file name alone, ignoring case. `None` is a custom mesh.
pub fn primitive_shape(mesh: &str) -> Option<PartType> {
    let lower = mesh.to_lowercase();
    let file = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    PRIMITIVE_HINTS
        .iter()
        .find(|(hint, _)| file.contains(hint))
        .map(|&(_, shape)| shape)
}

/// A unit mesh of `shape`, filling the cube from -0.5 to 0.5 exactly as the
/// engine's `engine/assets/parts/<shape>.glb` does, with the same orientation
/// and tessellation. A part's size scales it through `Transform.scale`.
///
/// Cylinders and cones stand on the Y axis, with a cone's tip at +Y. The wedge
/// keeps its full face on the bottom and the back (-Z) and slopes down toward
/// +Z. The corner wedge is the tetrahedron whose right-angled corner sits at
/// (-0.5, -0.5, -0.5).
pub fn primitive_mesh(shape: PartType) -> Mesh {
    match shape {
        PartType::Block => Mesh::from(Cuboid::new(1.0, 1.0, 1.0)),
        PartType::Ball => Sphere::new(0.5).mesh().uv(32, 16),
        PartType::Cylinder => Cylinder::new(0.5, 1.0).mesh().resolution(32).build(),
        PartType::Cone => Cone::new(0.5, 1.0).mesh().resolution(32).build(),
        PartType::Wedge => faceted_mesh(&WEDGE_CORNERS, WEDGE_FACES),
        PartType::CornerWedge => faceted_mesh(&CORNER_WEDGE_CORNERS, CORNER_WEDGE_FACES),
    }
}

/// Bottom back left, bottom back right, bottom front right, bottom front
/// left, top back left, top back right.
const WEDGE_CORNERS: [Vec3; 6] = [
    Vec3::new(-0.5, -0.5, -0.5),
    Vec3::new(0.5, -0.5, -0.5),
    Vec3::new(0.5, -0.5, 0.5),
    Vec3::new(-0.5, -0.5, 0.5),
    Vec3::new(-0.5, 0.5, -0.5),
    Vec3::new(0.5, 0.5, -0.5),
];
/// Bottom, back, left, right, slope; each counter-clockwise from outside.
const WEDGE_FACES: &[&[usize]] = &[
    &[0, 1, 2, 3],
    &[0, 4, 5, 1],
    &[0, 3, 4],
    &[1, 5, 2],
    &[4, 3, 2, 5],
];

/// The right-angled corner, then the corners along +X, +Z and +Y from it.
const CORNER_WEDGE_CORNERS: [Vec3; 4] = [
    Vec3::new(-0.5, -0.5, -0.5),
    Vec3::new(0.5, -0.5, -0.5),
    Vec3::new(-0.5, -0.5, 0.5),
    Vec3::new(-0.5, 0.5, -0.5),
];
/// Bottom, back, left, slope; each counter-clockwise from outside.
const CORNER_WEDGE_FACES: &[&[usize]] = &[&[0, 1, 2], &[0, 3, 1], &[0, 2, 3], &[1, 3, 2]];

/// A flat-shaded mesh from convex faces. Each face lists its corners
/// counter-clockwise seen from outside and gets its own copy of them, so its
/// normal stays sharp at every edge.
fn faceted_mesh(corners: &[Vec3], faces: &[&[usize]]) -> Mesh {
    const UV: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let (mut positions, mut normals, mut uvs, mut indices) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for face in faces {
        let p: Vec<Vec3> = face.iter().map(|&i| corners[i]).collect();
        let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize();
        let base = positions.len() as u32;
        for (k, corner) in p.iter().enumerate() {
            positions.push(corner.to_array());
            normals.push(normal.to_array());
            uvs.push(UV[k % 4]);
        }
        for k in 1..p.len() as u32 - 1 {
            indices.extend([base, base + k, base + k + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// The unit collider Studio gives each shape (`safe_collider_from` in the
/// engine loader), which Avian scales by the part's `Transform.scale`. Studio
/// collides a cone as a cylinder and both wedges as boxes, and so does this,
/// so a body meets the same surfaces in Studio and the Player.
#[cfg(feature = "physics")]
pub fn primitive_collider(shape: PartType) -> avian3d::prelude::Collider {
    use avian3d::prelude::Collider;
    match shape {
        PartType::Ball => Collider::sphere(0.5),
        PartType::Cylinder | PartType::Cone => Collider::cylinder(0.5, 1.0),
        PartType::Block | PartType::Wedge | PartType::CornerWedge => {
            Collider::cuboid(1.0, 1.0, 1.0)
        }
    }
}

/// Studio's material for a part with no `.mat.toml` of its own
/// (`resolve_material` in the engine's material loader): the preset's
/// roughness, metallic and reflectance, alpha of 1 minus the transparency,
/// transmission for glass and a glow for neon.
pub fn part_material(part: &SpacePart) -> StandardMaterial {
    let preset = MaterialPreset::from_string(&part.material);
    let (roughness, metallic, preset_reflectance) = preset.pbr_params();
    let alpha = 1.0 - part.transparency;
    let mut material = StandardMaterial {
        base_color: part.color.with_alpha(alpha),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: roughness,
        metallic,
        reflectance: if part.reflectance > 0.0 { part.reflectance } else { preset_reflectance },
        ..default()
    };
    match preset {
        MaterialPreset::Glass => {
            material.specular_transmission = 0.9;
            material.diffuse_transmission = 0.3;
            material.thickness = 0.5;
            material.ior = 1.5;
        }
        MaterialPreset::Neon => material.emissive = LinearRgba::from(part.color) * 2.0,
        _ => {}
    }
    material
}

/// What makes two parts' materials identical, so they can share one: the
/// preset, the colour, the transparency and the reflectance.
pub type LookKey = (&'static str, [u32; 4], u32, u32);

/// A part's [`LookKey`].
pub fn look_key(part: &SpacePart) -> LookKey {
    let c = part.color.to_srgba();
    (
        MaterialPreset::from_string(&part.material).as_str(),
        [c.red, c.green, c.blue, c.alpha].map(f32::to_bits),
        part.transparency.to_bits(),
        part.reflectance.to_bits(),
    )
}

// ── Spawning ───────────────────────────────────────────────────────────────

/// Marks an entity spawned from a Space by [`spawn_space_parts`], so a shell
/// can clear the world without tracking handles itself.
#[cfg(feature = "physics")]
#[derive(Component, Debug)]
pub struct SpawnedFromSpace;

/// Spawn every part in `geo` as its shape, at its size, with Studio's
/// collider and material for it.
///
/// Lives in `common` rather than in a shell so that "the Client sees what
/// Studio sees" is a property of one function instead of two implementations
/// agreeing by inspection. Parts share one mesh per shape and one material
/// per distinct look.
///
/// ## The collider is UNIT-sized, and that is not a mistake
///
/// Avian applies the entity's `Transform.scale` to its collider — see
/// `update_collider_scale` in avian3d-0.7.0 `collision/collider/backend.rs:460`,
/// gated on `transform_to_collider_scale`, which defaults to `true`
/// (`physics_transform/mod.rs:162`).
///
/// A part's size is carried in `Transform.scale`, so passing that size to
/// `Collider::cuboid` as well multiplies it in twice: the collider ends up
/// **size²**. The course's 160 m ground plate got a 25 600 m collider, which
/// swallowed the whole level — the character stood on an invisible surface far
/// above the visible one and was shoved around by boxes metres from anything
/// on screen.
///
/// `Collider::cuboid` itself takes FULL lengths and halves them internally
/// (`parry/mod.rs:747`), so a unit cube scaled by `Transform.scale` is exactly
/// the visible box.
#[cfg(feature = "physics")]
pub fn spawn_space_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    geo: &SpaceGeometry,
) -> usize {
    use avian3d::prelude::RigidBody;

    // One mesh per shape, indexed by the `PartType` discriminant.
    let mut shape_meshes: [Option<Handle<Mesh>>; 6] = Default::default();
    // One material per distinct look.
    let mut looks: std::collections::HashMap<LookKey, Handle<StandardMaterial>> =
        std::collections::HashMap::new();
    let mut n = 0;

    for p in &geo.parts {
        // A zero or negative extent produces a degenerate collider that Avian
        // reports as NaN contacts rather than rejecting. `read_space_parts`
        // never yields one; this guards geometry built any other way.
        let s = p.transform.scale;
        if !s.is_finite() || s.min_element() <= 0.0 {
            continue;
        }
        let mesh = shape_meshes[p.shape as usize]
            .get_or_insert_with(|| meshes.add(primitive_mesh(p.shape)))
            .clone();
        let material = looks.entry(look_key(p)).or_insert_with(|| materials.add(part_material(p))).clone();

        let mut e = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            p.transform,
            Name::new(p.name.clone()),
            SpawnedFromSpace,
        ));

        if p.can_collide {
            e.insert((primitive_collider(p.shape), RigidBody::Static));
            // Every part here is Static, so what the climb rules would read
            // off an unanchored part's body, or off a part's attributes and
            // class in Studio, is recorded on the entity instead.
            if let Some(mark) = climb_mark(p) {
                e.insert(mark);
            }
        }
        n += 1;
    }
    n
}

/// The Models some Player's `Character` points at: the avatar runtime draws
/// and moves those.
pub fn character_models(g: &DataModel) -> std::collections::HashSet<InstanceId> {
    let Some(players) = g.find_service("Players") else { return Default::default() };
    g.children(players)
        .iter()
        .filter_map(|&p| g.get_prop(p, "Character").and_then(|v| v.as_instance()))
        .collect()
}

/// The Models holding a `Humanoid` or an `AnimationController`: NPCs and other
/// characters, whose parts characters do not climb by default. Only a Model is
/// a character, so a rig left loose in Workspace or a Folder marks nothing.
pub fn rigged_models(g: &DataModel) -> std::collections::HashSet<InstanceId> {
    let Some(workspace) = g.find_service("Workspace") else { return Default::default() };
    g.descendants(workspace)
        .into_iter()
        .filter(|&d| matches!(g.class_of(d), Some("Humanoid" | "AnimationController")))
        .filter_map(|d| g.parent(d))
        .filter(|&m| g.class_of(m) == Some("Model"))
        .collect()
}

/// Whether an instance is inside one of `models`.
pub fn inside_any(g: &DataModel, id: InstanceId, models: &std::collections::HashSet<InstanceId>) -> bool {
    let mut at = g.parent(id);
    // Bounded, as the tree's own ancestry walks are.
    for _ in 0..4096 {
        let Some(here) = at else { return false };
        if models.contains(&here) {
            return true;
        }
        at = g.parent(here);
    }
    false
}

/// The climb mark for a part, from what the Space says about it. See
/// [`crate::avatar::climbable`].
#[cfg(feature = "physics")]
pub fn climb_mark(p: &SpacePart) -> Option<crate::avatar::climbable::Climbable> {
    use crate::avatar::climbable::{loader_mark, INVISIBLE_TRANSPARENCY};
    loader_mark(p.climbable, p.in_character, !p.anchored, p.transparency >= INVISIBLE_TRANSPARENCY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_colour_arrays_are_0_255_and_float_arrays_are_0_1() {
        let ints: Vec<toml::Value> = vec![255.into(), 128.into(), 0.into()];
        let c = decode_color(&ints).to_srgba();
        assert!((c.red - 1.0).abs() < 1e-3, "255 should be full red, got {}", c.red);
        assert!((c.green - 0.502).abs() < 1e-2);

        let floats: Vec<toml::Value> = vec![
            toml::Value::Float(0.5),
            toml::Value::Float(0.25),
            toml::Value::Float(1.0),
        ];
        let c = decode_color(&floats).to_srgba();
        assert!((c.red - 0.5).abs() < 1e-3, "float array must not be divided by 255");
    }

    /// The trap: one float in an otherwise-integer array reinterprets EVERY
    /// channel. Pinned so the rule cannot be softened into per-element guessing.
    #[test]
    fn one_float_reinterprets_the_whole_array() {
        let mixed: Vec<toml::Value> = vec![200.into(), 120.into(), toml::Value::Float(1.0)];
        let c = decode_color(&mixed).to_srgba();
        assert!(
            c.red > 1.0,
            "mixed array must be read as 0-1 (and so overflow), not silently as 0-255"
        );
    }

    #[test]
    fn alpha_defaults_to_opaque_when_absent() {
        let rgb: Vec<toml::Value> = vec![
            toml::Value::Float(0.2),
            toml::Value::Float(0.2),
            toml::Value::Float(0.2),
        ];
        assert!((decode_color(&rgb).to_srgba().alpha - 1.0).abs() < 1e-6);
    }

    /// `scale` is a SIZE, so it must not propagate: a 4 m box inside a folder
    /// nested in another folder is still 4 m.
    #[test]
    fn nesting_composes_position_and_rotation_but_never_size() {
        let parent = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(3.0),
        };
        let child = Transform {
            translation: Vec3::new(0.0, 0.0, -2.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(4.0, 0.5, 1.5),
        };
        let w = compose(&parent, &child);

        assert_eq!(w.scale, child.scale, "parent scale leaked into child size");
        // +90° about Y maps -Z to -X.
        assert!((w.translation.x - 8.0).abs() < 1e-4, "got {:?}", w.translation);
        assert!(w.translation.z.abs() < 1e-4);
    }

    #[test]
    fn mesh_names_map_to_shapes_as_in_the_engine() {
        for (mesh, shape) in [
            ("parts/block.glb", PartType::Block),
            ("parts/ball.glb", PartType::Ball),
            ("parts/cylinder.glb", PartType::Cylinder),
            ("parts/wedge.glb", PartType::Wedge),
            ("parts/corner_wedge.glb", PartType::CornerWedge),
            ("parts/cone.glb", PartType::Cone),
            // `corner_wedge` contains `wedge`, and case and separators vary.
            ("PARTS\\Corner_Wedge.GLB", PartType::CornerWedge),
        ] {
            assert_eq!(primitive_shape(mesh), Some(shape), "{mesh}");
        }
        assert_eq!(primitive_shape("meshes/Paddle.glb"), None);
    }

    /// The two hand-built meshes against the engine's GLBs, whose vertex data
    /// gives these corners, face normals and triangle counts. Matching the
    /// outward normals also proves every face is wound to face out.
    #[test]
    fn wedges_match_the_engine_glbs() {
        use bevy::mesh::VertexAttributeValues;
        let (h, t) = (std::f32::consts::FRAC_1_SQRT_2, 1.0 / 3f32.sqrt());
        let wedge_corners = [
            [-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, -0.5],
            [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [0.5, 0.5, -0.5],
        ];
        let wedge_normals = [
            [0.0, -1.0, 0.0],
            [0.0, 0.0, -1.0],
            [-1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, h, h],
        ];
        let corner_corners = [
            [-0.5, -0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, -0.5],
            [0.5, -0.5, -0.5],
        ];
        let corner_normals = [[0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [t, t, t]];
        let cases: [(PartType, &[[f32; 3]], &[[f32; 3]], usize); 2] = [
            (PartType::Wedge, &wedge_corners, &wedge_normals, 8),
            (PartType::CornerWedge, &corner_corners, &corner_normals, 4),
        ];
        for (shape, corners, face_normals, triangles) in cases {
            let mesh = primitive_mesh(shape);
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("{shape:?}: no positions");
            };
            let Some(VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("{shape:?}: no normals");
            };
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("{shape:?}: no indices")
            };

            let key = |p: &[f32; 3]| p.map(|c| (c * 2.0).round() as i32);
            let mut got: Vec<_> = positions.iter().map(key).collect();
            got.sort();
            got.dedup();
            let mut want: Vec<_> = corners.iter().map(key).collect();
            want.sort();
            assert_eq!(got, want, "{shape:?}: corners");
            assert_eq!(indices.len() / 3, triangles, "{shape:?}: triangle count");

            for tri in indices.chunks(3) {
                let [a, b, c] =
                    [tri[0], tri[1], tri[2]].map(|i| Vec3::from_array(positions[i as usize]));
                let n = Vec3::from_array(normals[tri[0] as usize]);
                assert!(
                    (b - a).cross(c - a).normalize().dot(n) > 0.999,
                    "{shape:?}: winding disagrees with normal"
                );
                assert!(
                    face_normals.iter().any(|f| Vec3::from_array(*f).dot(n) > 0.999),
                    "{shape:?}: normal {n} is not one of the engine mesh's"
                );
            }
            for f in face_normals {
                let f = Vec3::from_array(*f);
                assert!(
                    normals.iter().any(|n| Vec3::from_array(*n).dot(f) > 0.999),
                    "{shape:?}: face {f} missing"
                );
            }
        }
    }

    /// What decides whether characters climb a part, read from a Space: an
    /// NPC's parts are a character's, a `Climbable` attribute on a Model or a
    /// Folder reaches everything inside it, and the nearest one wins.
    #[test]
    fn a_spaces_characters_and_climbable_attributes_reach_its_parts() {
        let root =
            std::env::temp_dir().join(format!("eustress_space_read_climb_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("Workspace");
        for dir in ["Npc", "Statue", "Map/Tower"] {
            std::fs::create_dir_all(workspace.join(dir)).unwrap();
        }
        let file = |class: &str, name: &str, attributes: &str| {
            format!(
                "[metadata]\nclass_name = \"{class}\"\nname = \"{name}\"\n\n\
                 [transform]\nposition = [0.0, 1.0, 0.0]\nscale = [1.0, 2.0, 1.0]\n\n{attributes}"
            )
        };
        let write = |path: &str, text: String| std::fs::write(workspace.join(path), text).unwrap();
        // A rig left loose in Workspace makes nothing a character.
        write("Stray.instance.toml", file("Humanoid", "Stray", ""));
        write("Npc/_instance.toml", file("Model", "Npc", ""));
        write("Npc/Humanoid.instance.toml", file("Humanoid", "Humanoid", ""));
        write("Npc/Torso.instance.toml", file("Part", "Torso", ""));
        write("Statue/_instance.toml", file("Model", "Statue", "[attributes]\nClimbable = true\n"));
        write("Statue/Humanoid.instance.toml", file("Humanoid", "Humanoid", ""));
        write("Statue/Plinth.instance.toml", file("Part", "Plinth", ""));
        write("Map/_instance.toml", file("Folder", "Map", "[attributes]\nClimbable = false\n"));
        write("Map/Trim.instance.toml", file("Part", "Trim", ""));
        write("Map/Tower/_instance.toml", file("Model", "Tower", "[attributes]\nClimbable = true\n"));
        write("Map/Tower/Wall.instance.toml", file("Part", "Wall", ""));
        write("Block.instance.toml", file("Part", "Block", "[properties]\nanchored = true\n"));
        write("Crate.instance.toml", file("Part", "Crate", ""));

        let geo = read_space_parts(&root).expect("space reads");
        let _ = std::fs::remove_dir_all(&root);
        assert!(geo.errors.is_empty(), "{:?}", geo.errors);
        let get = |name: &str| geo.parts.iter().find(|p| p.name == name).expect(name);

        assert!(get("Torso").in_character, "an NPC's part is a character's");
        assert_eq!(get("Torso").climbable, None);
        assert!(get("Plinth").in_character);
        assert_eq!(get("Plinth").climbable, Some(true), "the statue's own attribute reaches its parts");
        assert_eq!(get("Trim").climbable, Some(false), "a folder's attribute reaches its parts");
        assert!(!get("Trim").in_character);
        assert_eq!(get("Wall").climbable, Some(true), "the nearest attribute wins");
        assert_eq!(get("Block").climbable, None);
        assert!(!get("Block").in_character);

        #[cfg(feature = "physics")]
        {
            use crate::avatar::climbable::Climbable;
            assert_eq!(climb_mark(get("Torso")), Some(Climbable(false)));
            assert_eq!(climb_mark(get("Plinth")), Some(Climbable(true)));
            assert_eq!(climb_mark(get("Trim")), Some(Climbable(false)));
            assert_eq!(climb_mark(get("Block")), None, "an ordinary anchored part needs no mark");
            assert_eq!(climb_mark(get("Crate")), Some(Climbable(false)), "a loose part is never a climbing surface");
        }
    }

    /// Shape, custom meshes and the new properties, read from files in the
    /// on-disk format.
    #[test]
    fn a_parts_shape_comes_from_its_asset_mesh() {
        let root =
            std::env::temp_dir().join(format!("eustress_space_read_shapes_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("Workspace");
        std::fs::create_dir_all(workspace.join("Ramp")).unwrap();
        let part = |name: &str, asset: &str| {
            format!(
                "[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\n\n{asset}\
                 [transform]\nposition = [0.0, 1.0, 0.0]\n\
                 rotation = [0.0, 0.0, 0.0, 1.0]\nscale = [4.0, 2.0, 6.0]\n\n\
                 [properties]\ncolor = [0.2, 0.4, 0.8, 1.0]\ntransparency = 0.25\n\
                 reflectance = 0.1\nmaterial = \"Neon\"\n"
            )
        };
        std::fs::write(
            workspace.join("Ramp/_instance.toml"),
            part("Ramp", "[asset]\nmesh = \"parts/wedge.glb\"\n\n"),
        )
        .unwrap();
        std::fs::write(workspace.join("Plate.instance.toml"), part("Plate", "")).unwrap();
        std::fs::write(
            workspace.join("Car.instance.toml"),
            part("Car", "[asset]\nmesh = \"meshes/Car.glb\"\n\n"),
        )
        .unwrap();
        std::fs::write(
            workspace.join("Spawn.instance.toml"),
            "[metadata]\nclass_name = \"SpawnLocation\"\nname = \"Spawn\"\n\n\
             [transform]\nposition = [3.0, 1.0, -2.0]\nscale = [4.0, 1.0, 4.0]\n",
        )
        .unwrap();

        let geo = read_space_parts(&root).expect("space reads");
        let _ = std::fs::remove_dir_all(&root);
        assert!(geo.errors.is_empty(), "{:?}", geo.errors);
        let get = |name: &str| geo.parts.iter().find(|p| p.name == name).expect(name);

        let ramp = get("Ramp");
        assert_eq!(ramp.shape, PartType::Wedge);
        assert_eq!(ramp.custom_mesh, None);
        assert_eq!(ramp.transform.scale, Vec3::new(4.0, 2.0, 6.0));
        assert!((ramp.transparency - 0.25).abs() < 1e-6 && (ramp.reflectance - 0.1).abs() < 1e-6);
        assert_eq!(get("Plate").shape, PartType::Block, "a Part with no [asset] is a block");
        assert_eq!(get("Car").shape, PartType::Block);
        assert_eq!(get("Car").custom_mesh.as_deref(), Some("meshes/Car.glb"));
        assert_eq!(geo.custom_mesh_parts(), 1);
        assert_eq!(get("Spawn").class_name, "SpawnLocation", "every BasePart draws");
        let feet = geo.spawn_point().expect("the SpawnLocation is the spawn");
        assert!((feet - Vec3::new(3.0, 1.6, -2.0)).length() < 1e-5, "on top of the pad, got {feet}");
    }

    #[test]
    fn materials_follow_studios_presets() {
        let part = |material: &str, transparency: f32, reflectance: f32| SpacePart {
            name: "p".into(),
            class_name: "Part".into(),
            transform: Transform::IDENTITY,
            color: Color::srgb(1.0, 0.5, 0.0),
            material: material.into(),
            shape: PartType::Block,
            custom_mesh: None,
            anchored: true,
            can_collide: true,
            transparency,
            reflectance,
            climbable: None,
            in_character: false,
        };
        let see_through = part_material(&part("Plastic", 0.25, 0.0));
        assert!((see_through.base_color.alpha() - 0.75).abs() < 1e-6);
        assert!(matches!(see_through.alpha_mode, AlphaMode::Blend));
        assert!(matches!(part_material(&part("Plastic", 0.0, 0.0)).alpha_mode, AlphaMode::Opaque));
        assert_eq!(part_material(&part("Metal", 0.0, 0.0)).metallic, 1.0);
        assert!(part_material(&part("Neon", 0.0, 0.0)).emissive.red > 1.0, "neon glows");
        assert!((part_material(&part("Plastic", 0.0, 0.3)).reflectance - 0.3).abs() < 1e-6);
        assert!(part_material(&part("Glass", 0.0, 0.0)).specular_transmission > 0.0);
    }

    #[test]
    fn bad_transforms_are_cleaned_as_the_engine_cleans_them() {
        let t = clean_pose([f32::NAN, 2.0, f32::INFINITY], [0.0, 0.0, 0.0, 0.0], [-3.0, 0.0, f32::NAN]);
        assert_eq!(t.translation, Vec3::new(0.0, 2.0, 0.0));
        assert_eq!(t.rotation, Quat::IDENTITY);
        assert_eq!(t.scale, Vec3::new(3.0, 1.0e-6, 1.0e-6));
    }

    /// An imported file's numbers are in the unit it declares; the Player
    /// converts them to metres as Studio does, so the two draw the same size.
    #[cfg(feature = "units_v1")]
    #[test]
    fn declared_units_convert_to_metres() {
        let t = authored_pose([10.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [2.0, 4.0, 1.0], Some("ft"));
        assert!((t.translation.x - 3.048).abs() < 1e-5, "10 ft is 3.048 m, got {}", t.translation.x);
        assert!((t.scale - Vec3::new(0.6096, 1.2192, 0.3048)).length() < 1e-5, "got {}", t.scale);
        let studs = authored_pose([0.0; 3], [0.0, 0.0, 0.0, 1.0], [196.8, 1.0, 1.0], Some("studs"));
        assert!((studs.scale.x - 9.815).abs() < 1e-3, "196.8 studs is 9.815 m, got {}", studs.scale.x);
        let metres = authored_pose([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], None);
        assert_eq!(metres.translation, Vec3::new(1.0, 2.0, 3.0), "no unit is metres");
    }

    #[test]
    fn parts_read_from_the_tree_as_studio_reads_them() {
        use crate::datamodel::EnumItem;
        use crate::scripting::{CFrame, Color3, Vector3};

        let mut dm = DataModel::new();
        let ball = dm.create("Part");
        dm.set_prop(ball, "Shape", DmValue::Enum(EnumItem::new("PartType", "Ball"))).unwrap();
        dm.set_prop(ball, "CFrame", DmValue::CFrame(CFrame::new(1.0, 2.0, 3.0))).unwrap();
        dm.set_prop(ball, "Size", DmValue::Vector3(Vector3::new(2.0, 2.0, 2.0))).unwrap();
        dm.set_prop(ball, "Color", DmValue::Color3(Color3::new(1.0, 0.5, 0.0))).unwrap();
        dm.set_prop(ball, "Transparency", DmValue::Number(0.25)).unwrap();
        dm.set_prop(ball, "Material", DmValue::Enum(EnumItem::new("Material", "Neon"))).unwrap();
        dm.set_prop(ball, "Anchored", DmValue::Bool(true)).unwrap();
        let wedge = dm.create("WedgePart");
        let meshed = dm.create("MeshPart");
        dm.set_prop(meshed, "MeshId", DmValue::String("parts/cylinder.glb".into())).unwrap();
        let custom = dm.create("MeshPart");
        dm.set_prop(custom, "MeshId", DmValue::String("meshes/Car.glb".into())).unwrap();
        let folder = dm.create("Folder");

        let b = SpacePart::from_tree(&dm, ball).expect("a Part reads");
        assert_eq!(b.shape, PartType::Ball);
        assert_eq!(b.transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(b.transform.scale, Vec3::splat(2.0));
        assert_eq!(b.material, "Neon");
        assert!((b.transparency - 0.25).abs() < 1e-6 && b.anchored && b.can_collide);
        let c = b.color.to_srgba();
        assert!((c.red - 1.0).abs() < 1e-6 && (c.green - 0.5).abs() < 1e-6);
        assert_eq!(SpacePart::from_tree(&dm, wedge).unwrap().shape, PartType::Wedge, "the class is the shape");
        assert_eq!(
            SpacePart::from_tree(&dm, meshed).unwrap().shape,
            PartType::Cylinder,
            "a MeshId naming a primitive sets the shape"
        );
        let car = SpacePart::from_tree(&dm, custom).unwrap();
        assert_eq!((car.shape, car.custom_mesh.as_deref()), (PartType::Block, Some("meshes/Car.glb")));
        assert!(SpacePart::from_tree(&dm, folder).is_none(), "only BaseParts are parts");
    }

    #[test]
    fn the_tree_spawns_on_top_of_its_workspace_pad() {
        use crate::scripting::{CFrame, Vector3};

        let mut dm = DataModel::new();
        let pad = |dm: &mut DataModel, x: f64| {
            let id = dm.create("SpawnLocation");
            dm.set_prop(id, "CFrame", DmValue::CFrame(CFrame::new(x, 1.0, -2.0))).unwrap();
            dm.set_prop(id, "Size", DmValue::Vector3(Vector3::new(6.0, 1.0, 6.0))).unwrap();
            id
        };
        assert_eq!(tree_spawn_point(&dm), None, "no Workspace, no spawn");
        let ws = dm.get_service("Workspace").expect("Workspace is a service");
        let stored = pad(&mut dm, 50.0);
        let storage = dm.get_service("ReplicatedStorage").expect("ReplicatedStorage is a service");
        dm.set_parent(stored, Some(storage)).unwrap();
        assert_eq!(tree_spawn_point(&dm), None, "a pad outside the Workspace is not a spawn");
        let placed = pad(&mut dm, 3.0);
        dm.set_parent(placed, Some(ws)).unwrap();
        let feet = tree_spawn_point(&dm).expect("the Workspace pad is the spawn");
        assert!((feet - Vec3::new(3.0, 1.6, -2.0)).length() < 1e-5, "on top of the pad, got {feet}");
    }

    #[test]
    fn a_directory_without_workspace_is_not_a_space() {
        let tmp = std::env::temp_dir().join("eustress_space_read_neg");
        let _ = std::fs::create_dir_all(&tmp);
        assert!(read_space_parts(&tmp).is_err());
    }

    /// End-to-end against the real authored Space when it is present. Skipped
    /// rather than failed on a machine that has not generated it — but when it
    /// IS there, this is the only test that proves the reader agrees with the
    /// format actually on disk.
    #[test]
    fn the_climbing_course_reads_if_it_is_installed() {
        let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
        else {
            return;
        };
        let root = PathBuf::from(home)
            .join("Documents/Eustress/Movement/Spaces/Climbing");
        if !root.join("Workspace").is_dir() {
            return;
        }

        let geo = read_space_parts(&root).expect("Climbing Space failed to read");
        assert!(geo.errors.is_empty(), "parse errors: {:?}", geo.errors);
        assert!(
            geo.parts.len() >= 40,
            "expected the full course, got {} parts",
            geo.parts.len()
        );
        // The course is deliberately built so specific heights exist; if the
        // nesting maths is wrong these land somewhere else entirely.
        let ground = geo.parts.iter().find(|p| p.name == "Ground").expect("no Ground");
        assert!((ground.transform.translation.y + 0.5).abs() < 1e-3);
        assert!(ground.transform.scale.x > 100.0, "Ground lost its size");
    }
}
