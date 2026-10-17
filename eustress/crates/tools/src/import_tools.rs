//! Bring a mesh made elsewhere (headless Blender, say) into a Space, and place
//! it, over MCP: `import_asset` and `place_instances`.
//!
//! GLB only. STL, STEP, OBJ, PLY and FBX are not supported yet (the engine's
//! converters in `mesh_import.rs` are scaffolds); export GLB, which Blender
//! does natively. The source is a path inside the Universe
//! (`sandbox::resolve`), so whatever made the file writes it there first, for
//! example under `Imports/`; nothing is read from elsewhere on the machine.
//! The copy lands in the Space (`Assets/Meshes/` by default), or with
//! `shared` in the Universe's shared mesh folder
//! (`space_path::shared_mesh_dir`, `<Universe>/assets/meshes`), both folders
//! the `space://` reader serves. A placement is an ordinary Part written by
//! `eustress_common::instance_create::create_instance`, the same writer the
//! editor uses, so the file watcher spawns it live. A Part names a Space mesh
//! by its path from the Part's folder, and a shared mesh as
//! `assets/meshes/<file>`, which the loader looks for beside the Part, then in
//! the Space, then in the Universe.
//!
//! What Studio draws for a mesh Part decides what is checked here. The loader
//! (`instance_loader::spawn_instance`) loads `#Mesh0/Primitive0` and
//! `#Material0/std` from the file: the first primitive of the first mesh,
//! with the first material, and no node transform. So the bounds reported are
//! that primitive's, and a GLB Studio would draw only part of, or in another
//! place or size than Blender showed, is imported but not placed, with the
//! reason and the Blender step that fixes it.

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// Largest GLB imported. A city building is a few MB; past this a mistake is
/// likelier than a mesh.
const MAX_GLB_BYTES: u64 = 256 * 1024 * 1024;

/// Largest glTF JSON chunk read to find a mesh's bounds.
const MAX_JSON_BYTES: u32 = 64 * 1024 * 1024;

/// Most placements in one `place_instances` call.
const MAX_PLACEMENTS: usize = 500;

/// Most node visits while walking a GLB's node tree. A tree is visited once
/// per node; a file that lists nodes as children of many parents, or in a
/// cycle, stops here instead of walking forever.
const MAX_NODE_VISITS: usize = 100_000;

/// Mesh formats people try that are not imported yet.
const NOT_YET: &[&str] = &["stl", "step", "stp", "obj", "ply", "fbx", "gltf", "dae", "3ds", "blend"];

fn result(tool: &str, success: bool, content: String, data: Option<Value>) -> ToolResult {
    ToolResult {
        tool_name: tool.to_string(),
        tool_use_id: String::new(),
        success,
        content,
        structured_data: data,
        stream_topic: None,
    }
}

// ── GLB ─────────────────────────────────────────────────────────────────────

/// What a GLB holds, as far as Studio's drawing of it goes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlbInfo {
    bytes: u64,
    meshes: usize,
    /// Primitives across every mesh. Each is drawn with one material, so a
    /// Blender object with three materials exports as three.
    primitives: usize,
    /// The bounds Studio draws: the first mesh's first primitive's POSITION
    /// box, in metres, with no node transform.
    drawn: Option<([f64; 3], [f64; 3])>,
    /// A node that draws the first mesh carries a transform (Blender's object
    /// location, rotation or scale) that Studio does not apply.
    moved: bool,
    /// The material the first primitive names (`None` when it names none).
    material: Option<u64>,
}

impl GlbInfo {
    /// Why Studio would not draw this GLB as Blender showed it. Empty when it
    /// would.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.meshes == 0 || self.primitives == 0 {
            out.push("it has no mesh".to_string());
            return out;
        }
        if self.drawn.is_none() {
            out.push("its first mesh declares no bounds (POSITION min and max)".to_string());
        }
        if self.meshes > 1 || self.primitives > 1 {
            out.push(format!(
                "it has {} mesh(es) with {} part(s) in all, and Studio draws only the first part of the first mesh: in \
                 Blender join the objects (Ctrl+J) and give them one material before exporting",
                self.meshes, self.primitives
            ));
        }
        if self.moved {
            out.push(
                "its object carries a location, rotation or scale that Studio does not apply: apply it in Blender \
                 (Ctrl+A, All Transforms) before exporting"
                    .to_string(),
            );
        }
        if let Some(m) = self.material.filter(|m| *m != 0) {
            out.push(format!("its mesh uses material {m}, and Studio draws material 0"));
        }
        out
    }
}

/// Check that `path` is a GLB (glTF 2.0 binary: magic, version, total length,
/// a JSON first chunk) and read what Studio would draw. Only the header and
/// the JSON chunk are read.
pub(crate) fn read_glb(path: &Path) -> Result<GlbInfo, String> {
    use std::io::Read as _;
    let bytes = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if bytes > MAX_GLB_BYTES {
        return Err(format!("{} is {bytes} bytes; the limit is {MAX_GLB_BYTES}", path.display()));
    }
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut head = [0u8; 20];
    file.read_exact(&mut head).map_err(|_| format!("{} is too short to be a GLB", path.display()))?;
    let word = |i: usize| u32::from_le_bytes([head[i], head[i + 1], head[i + 2], head[i + 3]]);
    if &head[0..4] != b"glTF" {
        return Err(format!("{} is not a GLB (no glTF header)", path.display()));
    }
    if word(4) != 2 {
        return Err(format!("{} is glTF version {}, not 2", path.display(), word(4)));
    }
    if u64::from(word(8)) != bytes {
        return Err(format!("{} says it is {} bytes but is {bytes}: truncated or padded", path.display(), word(8)));
    }
    let json_len = word(12);
    if &head[16..20] != b"JSON" || json_len > MAX_JSON_BYTES || u64::from(json_len) + 20 > bytes {
        return Err(format!("{} has no readable JSON chunk", path.display()));
    }
    let mut json = vec![0u8; json_len as usize];
    file.read_exact(&mut json).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: Value = serde_json::from_slice(&json).map_err(|e| format!("{}: its glTF JSON does not parse ({e})", path.display()))?;
    analyse(&doc, bytes).map_err(|why| format!("{}: {why}", path.display()))
}

type Mat = [f64; 16];
const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Column-major 4x4 product `a * b`.
fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut out = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }
    out
}

fn numbers<const N: usize>(v: Option<&Value>) -> Option<[f64; N]> {
    let a = v?.as_array()?;
    if a.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64().filter(|x| x.is_finite())?;
    }
    Some(out)
}

/// A node's local matrix: `matrix`, or translation * rotation * scale.
fn local(node: &Value) -> Mat {
    if let Some(m) = numbers::<16>(node.get("matrix")) {
        return m;
    }
    let [tx, ty, tz] = numbers::<3>(node.get("translation")).unwrap_or([0.0; 3]);
    let [x, y, z, w] = numbers::<4>(node.get("rotation")).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let [sx, sy, sz] = numbers::<3>(node.get("scale")).unwrap_or([1.0; 3]);
    let r = [
        1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w), 0.0,
        2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w), 0.0,
        2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y), 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];
    let s = [sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, sz, 0.0, 0.0, 0.0, 0.0, 1.0];
    let t = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, tx, ty, tz, 1.0];
    mul(&t, &mul(&r, &s))
}

fn is_identity(m: &Mat) -> bool {
    m.iter().zip(IDENTITY.iter()).all(|(a, b)| (a - b).abs() < 1e-5)
}

/// Read a parsed glTF document as Studio draws it (see [`GlbInfo`]).
fn analyse(doc: &Value, bytes: u64) -> Result<GlbInfo, String> {
    let meshes = doc.get("meshes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let primitives = meshes
        .iter()
        .map(|m| m.get("primitives").and_then(Value::as_array).map_or(0, Vec::len))
        .sum();
    let first = meshes.first().and_then(|m| m.pointer("/primitives/0"));
    let drawn = first.and_then(|p| p.pointer("/attributes/POSITION")).and_then(Value::as_u64).and_then(|acc| {
        let accessor = doc.get("accessors")?.get(acc as usize)?;
        Some((numbers::<3>(accessor.get("min"))?, numbers::<3>(accessor.get("max"))?))
    });
    let material = first.and_then(|p| p.get("material")).and_then(Value::as_u64);

    // Whether any node in the scene that draws mesh 0 sits under a transform.
    let nodes = doc.get("nodes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let roots: Vec<usize> = match doc.get("scenes").and_then(Value::as_array) {
        Some(scenes) if !scenes.is_empty() => {
            let scene = doc.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
            let listed = scenes.get(scene).and_then(|s| s.get("nodes")).and_then(Value::as_array);
            listed.into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as usize)).collect()
        }
        _ => {
            let children: std::collections::HashSet<u64> = nodes
                .iter()
                .filter_map(|n| n.get("children").and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_u64)
                .collect();
            (0..nodes.len()).filter(|i| !children.contains(&(*i as u64))).collect()
        }
    };
    let mut moved = false;
    let mut visits = 0usize;
    let mut stack: Vec<(usize, Mat)> = roots.into_iter().map(|n| (n, IDENTITY)).collect();
    while let Some((index, parent)) = stack.pop() {
        visits += 1;
        if visits > MAX_NODE_VISITS {
            return Err("its node tree repeats or is too large to read".to_string());
        }
        let Some(node) = nodes.get(index) else {
            return Err(format!("it names node {index}, which it does not have"));
        };
        let world = mul(&parent, &local(node));
        if node.get("mesh").and_then(Value::as_u64) == Some(0) && !is_identity(&world) {
            moved = true;
        }
        for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
            if let Some(c) = child.as_u64() {
                stack.push((c as usize, world));
            }
        }
    }
    Ok(GlbInfo { bytes, meshes: meshes.len(), primitives, drawn, moved, material })
}

fn bounds_json(info: &GlbInfo) -> Value {
    match info.drawn {
        Some((min, max)) => json!({
            "min": min, "max": max,
            "size": [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            "ground_height": min[1],
        }),
        None => Value::Null,
    }
}

// ── Placement ───────────────────────────────────────────────────────────────

/// A rotation as `[x, y, z, w]`: a quaternion (normalized), or Euler angles
/// in degrees `[x, y, z]` composed like Roblox's `CFrame.Angles` (about X,
/// then the turned Y, then the turned Z), the convention `terrain_fill` uses.
fn parse_rotation(v: &Value) -> Result<[f32; 4], String> {
    let bad = || "`rotation` must be a quaternion [x, y, z, w] or Euler angles in degrees [x, y, z]".to_string();
    let parts: Vec<f64> = v
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .map(|n| n.as_f64().filter(|n| n.is_finite()))
        .collect::<Option<Vec<f64>>>()
        .ok_or_else(bad)?;
    let [x, y, z, w] = match parts.as_slice() {
        &[ax, ay, az] => {
            let half = |d: f64| (d.to_radians() / 2.0).sin_cos();
            let ((sx, cx), (sy, cy), (sz, cz)) = (half(ax), half(ay), half(az));
            // qx * qy, then * qz.
            let (x, y, z, w) = (sx * cy, cx * sy, sx * sy, cx * cy);
            [x * cz + y * sz, y * cz - x * sz, w * sz + z * cz, w * cz - z * sz]
        }
        &[x, y, z, w] => {
            let len = (x * x + y * y + z * z + w * w).sqrt();
            if !(len > 1e-6) {
                return Err("`rotation` quaternion has zero length".to_string());
            }
            [x / len, y / len, z / len, w / len]
        }
        _ => return Err(bad()),
    };
    Ok([x as f32, y as f32, z as f32, w as f32])
}

/// Three finite numbers, or `None` when the key is absent.
fn vec3(spec: &Value, key: &str) -> Result<Option<[f32; 3]>, String> {
    match spec.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => numbers::<3>(Some(v))
            .map(|a| Some(a.map(|x| x as f32)))
            .ok_or_else(|| format!("`{key}` must be three numbers [x, y, z], got {v}")),
    }
}

/// The relative path from a new instance's folder (one level under `base`)
/// to `target`, with forward slashes: what `[asset] mesh` holds, since the
/// loader joins it to the instance file's folder.
fn mesh_reference(base: &Path, target: &Path) -> Option<String> {
    let base: Vec<Component> = base.components().collect();
    let target: Vec<Component> = target.components().collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); base.len() - common + 1];
    parts.extend(target[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    Some(parts.join("/"))
}

/// The shared mesh folder relative to the Universe (`assets/meshes`), from
/// `space_path::shared_mesh_dir`, so this tool, the reader and a publish
/// agree on it.
fn shared_mesh_rel(universe_root: &Path) -> Result<String, String> {
    eustress_common::space_path::shared_mesh_dir(universe_root)
        .strip_prefix(universe_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "the shared mesh folder is not inside the Universe".to_string())
}

/// A GLB ready to place: where it is, what it holds, and, for a shared mesh,
/// the `assets/meshes/...` name a Part gives it.
struct Placeable {
    path: PathBuf,
    info: GlbInfo,
    shared_ref: Option<String>,
}

/// Place the GLB as a Part, from one placement object.
fn place_one(ctx: &ToolContext, asset: &Placeable, spec: &Value) -> Result<Value, String> {
    if !spec.is_object() {
        return Err("a placement must be an object".to_string());
    }
    let workspace = ctx.space_root.join("Workspace");
    let parent = spec.get("parent").and_then(Value::as_str).unwrap_or("").trim();
    let base = if parent.is_empty() {
        workspace.clone()
    } else {
        crate::sandbox::resolve(&workspace, parent).map_err(|why| format!("parent: {why}"))?
    };
    let stem = asset.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Mesh".into());
    let name = spec.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&stem);
    let rotation = match spec.get("rotation") {
        None | Some(Value::Null) => None,
        Some(v) => Some(parse_rotation(v)?),
    };
    // `scale` multiplies the mesh's own size. It is written explicitly: the
    // Part template's scale is a block's size, which would stretch the mesh.
    let scale = vec3(spec, "scale")?.unwrap_or([1.0; 3]);
    if scale.iter().any(|s| !(*s > 0.0)) {
        return Err(format!("`scale` must be positive on every axis, got {scale:?}"));
    }
    // Position in the caller's unit, the person's display unit, the Space's
    // default, else metres, as create_entity reads it; written in metres. No
    // unit is stamped on the file, because the loader converts a stamped
    // file's `scale` too, which for a mesh is a multiplier, not a length.
    let unit = spec
        .get("unit")
        .and_then(Value::as_str)
        .and_then(eustress_common::units::Unit::from_any)
        .or_else(|| ctx.display_unit.as_deref().and_then(eustress_common::units::Unit::from_any))
        .or_else(|| {
            eustress_common::project_manifest::read_space_default_unit(&ctx.space_root)
                .as_deref()
                .and_then(eustress_common::units::Unit::from_symbol)
        })
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
    let given = vec3(spec, "position")?.unwrap_or([0.0; 3]);
    let position = eustress_common::units::convert_vec3_f32(given, unit, eustress_common::units::ENGINE_NATIVE_UNIT);
    let mesh = match &asset.shared_ref {
        Some(shared) => shared.clone(),
        None => mesh_reference(&base, &asset.path).ok_or("the mesh and the Workspace share no folder")?,
    };
    // Studio reads a reference to `<shape>.glb` in a `parts` folder as its own
    // unit shape, whatever the file holds; the loader, a save and the Player
    // all decide by `primitive_shape`, so the reference is checked by it.
    if let Some(shape) = eustress_common::space_read::primitive_shape(&mesh) {
        return Err(format!(
            "Studio reads the mesh reference '{mesh}' as its built-in {shape:?} shape, not as this file; import it \
             under another name or folder (import_asset's dest), then place that"
        ));
    }
    let overrides = eustress_common::instance_create::InstanceOverrides {
        display_name: Some(name.to_string()),
        position: Some(position),
        rotation,
        scale: Some(scale),
        anchored: Some(spec.get("anchored").and_then(Value::as_bool).unwrap_or(true)),
        can_collide: Some(spec.get("can_collide").and_then(Value::as_bool).unwrap_or(true)),
        asset_mesh: Some(mesh.clone()),
        ..Default::default()
    };
    let created = eustress_common::instance_create::create_instance(&base, "Part", Some(name), overrides)
        .map_err(|e| format!("could not place {name}: {e}"))?;
    Ok(json!({
        "name": created.folder_name,
        "file": created.toml_path.to_string_lossy(),
        "mesh": mesh,
        "position_m": position,
    }))
}

/// The GLB a placement uses, that exists and that Studio will draw whole, as
/// itself: a Space-relative path inside the Space, or with `shared` a path in
/// the Universe's shared mesh folder (`assets/meshes/...`, as import_asset
/// returned it).
fn placeable_asset(ctx: &ToolContext, raw: &str, shared: bool) -> Result<Placeable, String> {
    let (path, shared_ref) = if shared {
        let folder = shared_mesh_rel(&ctx.universe_root)?;
        let name = raw.trim().replace('\\', "/");
        if !name.to_ascii_lowercase().starts_with(&format!("{}/", folder.to_ascii_lowercase())) {
            return Err(format!("a shared asset is a path in {folder}/, as import_asset with shared returned it; got '{raw}'"));
        }
        let path = crate::sandbox::resolve(&ctx.universe_root, &name).map_err(|why| format!("asset: {why}"))?;
        (path, Some(name))
    } else {
        (crate::sandbox::resolve(&ctx.space_root, raw).map_err(|why| format!("asset: {why}"))?, None)
    };
    if !path.is_file() {
        let place = if shared { "the Universe's shared mesh folder" } else { "the Space" };
        return Err(format!("asset '{raw}' is not a file in {place}; import it first (import_asset)"));
    }
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")) {
        return Err(format!("asset '{raw}' is not a .glb file"));
    }
    let info = read_glb(&path)?;
    let problems = info.problems();
    if !problems.is_empty() {
        return Err(format!("Studio would not draw '{raw}' as authored: {}", problems.join("; ")));
    }
    Ok(Placeable { path, info, shared_ref })
}

// ── import_asset ────────────────────────────────────────────────────────────

pub struct ImportAssetTool;

impl ToolHandler for ImportAssetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "import_asset",
            description: "Import a GLB mesh into the Space, and optionally place it. GLB only: STL, STEP, OBJ, PLY and FBX \
                are not supported yet, so export GLB (Blender does it natively). `source` is a path INSIDE the Universe \
                (write the file there first, for example under Imports/); it is checked as a glTF 2.0 binary (at most \
                256 MB) and copied to `dest` in the Space (default Assets/Meshes/<name>.glb), or with shared=true \
                into the Universe's shared mesh folder assets/meshes/, which every Space in the Universe can use (dest \
                is then a name inside that folder). Importing the same file again changes nothing; a different file at \
                `dest` needs overwrite=true. Studio draws a mesh Part from \
                the GLB's first mesh, its first primitive and its first material, without node transforms, so before \
                exporting from Blender join the objects (Ctrl+J), use one material and apply all transforms (Ctrl+A); \
                the reply lists any `problems`, and a GLB with problems is imported but not placed. The reply gives the \
                drawn bounds in metres: ground_height is the lowest point relative to the mesh's origin, so to stand it \
                on ground at height h use position y = h - ground_height x scale y. With `place`, it also writes a Part \
                using the mesh (the editor's own instance writer), which the engine spawns live; for many copies use \
                place_instances. Collision today is a box the size \
                of `scale` in metres at the Part's origin, not the mesh's shape; pass can_collide=false where that \
                box would get in the way.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "The GLB, a path relative to the Universe folder, e.g. Imports/citykit/house_a.glb." },
                    "dest": { "type": "string", "description": "Where it goes, ending .glb: Space-relative (default Assets/Meshes/<source name>), or with shared a name inside assets/meshes/ (default the source name)." },
                    "shared": { "type": "boolean", "description": "Put it in the Universe's shared mesh folder, assets/meshes/, for every Space in the Universe, instead of in this Space (default false). The reply's asset is then assets/meshes/<name>.glb." },
                    "overwrite": { "type": "boolean", "description": "Replace a different file already at dest (default false)." },
                    "place": {
                        "type": "object",
                        "description": "Also place one Part using the mesh.",
                        "properties": {
                            "parent": { "type": "string", "description": "Folder under Workspace, e.g. City/Block3 (made if missing). Default Workspace." },
                            "name": { "type": "string", "description": "Part name (made unique). Default the mesh name." },
                            "position": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] of the mesh's origin, in `unit`." },
                            "rotation": { "type": "array", "items": { "type": "number" }, "description": "A quaternion [x, y, z, w], or Euler angles in degrees [x, y, z] applied like Roblox CFrame.Angles (about X, then the turned Y, then the turned Z)." },
                            "scale": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] multiplier on the mesh's own size (default [1, 1, 1])." },
                            "anchored": { "type": "boolean", "description": "Physics does not move it (default true)." },
                            "can_collide": { "type": "boolean", "description": "It collides (default true)." },
                            "unit": { "type": "string", "description": "Unit of position (m, cm, ft, studs, ...). Default the person's display unit, else the Space's, else metres. The file is written in metres." }
                        }
                    }
                },
                "required": ["source"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.import_asset"],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "import_asset";
        let fail = |msg: String| result(TOOL, false, msg, None);
        let raw = input.get("source").and_then(Value::as_str).unwrap_or("").trim();
        if raw.is_empty() {
            return fail("`source` is required: a GLB inside the Universe, e.g. Imports/house.glb".into());
        }
        let ext = Path::new(raw).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext != "glb" {
            return fail(if NOT_YET.contains(&ext.as_str()) {
                format!(".{ext} is not supported yet: only GLB is (STL, STEP, OBJ, PLY and FBX are not supported yet). Export GLB and import that.")
            } else {
                format!("'{raw}' is not a .glb file")
            });
        }
        let source = match crate::sandbox::resolve(&ctx.universe_root, raw) {
            Ok(p) => p,
            Err(why) => return fail(format!("source: {why}. Put the file inside the Universe folder first.")),
        };
        if !source.is_file() {
            return fail(format!("source '{raw}' is not a file in the Universe"));
        }
        let info = match read_glb(&source) {
            Ok(info) => info,
            Err(e) => return fail(e),
        };

        let file_name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let shared = input.get("shared").and_then(Value::as_bool).unwrap_or(false);
        let given = input.get("dest").and_then(Value::as_str).map(str::trim).filter(|d| !d.is_empty());
        // A shared mesh's dest is a name inside the shared mesh folder, and
        // the whole path is checked against the Universe like any other.
        let (dest_root, dest_raw) = if shared {
            let folder = match shared_mesh_rel(&ctx.universe_root) {
                Ok(folder) => folder,
                Err(e) => return fail(e),
            };
            (&ctx.universe_root, format!("{folder}/{}", given.unwrap_or(&file_name)))
        } else {
            (&ctx.space_root, given.map(str::to_string).unwrap_or_else(|| format!("Assets/Meshes/{file_name}")))
        };
        if !Path::new(&dest_raw).extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")) {
            return fail(format!("dest '{dest_raw}' must end in .glb"));
        }
        let dest = match crate::sandbox::resolve(dest_root, &dest_raw) {
            Ok(p) => p,
            Err(why) => return fail(format!("dest: {why}")),
        };
        let overwrite = input.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
        let mut copied = true;
        if dest.exists() {
            if !dest.is_file() {
                return fail(format!("'{dest_raw}' is a folder"));
            }
            let same = matches!((std::fs::read(&dest), std::fs::read(&source)), (Ok(a), Ok(b)) if a == b);
            if same {
                copied = false;
            } else if !overwrite {
                return fail(format!("a different file is already at '{dest_raw}'; pass overwrite=true to replace it, or choose another dest"));
            }
        }
        if copied {
            if let Some(dir) = dest.parent() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    return fail(format!("could not make {}: {e}", dir.display()));
                }
            }
            // Through a temporary file, so a watcher never loads half a mesh.
            let tmp = dest.with_extension("glb.importing");
            let copy = std::fs::copy(&source, &tmp).and_then(|_| std::fs::rename(&tmp, &dest));
            if let Err(e) = copy {
                let _ = std::fs::remove_file(&tmp);
                return fail(format!("could not copy into the Space: {e}"));
            }
        }
        let asset = dest_raw.replace('\\', "/");
        let problems = info.problems();
        let mut data = json!({
            "asset": asset,
            "shared": shared,
            "file": dest.to_string_lossy(),
            "bytes": info.bytes,
            "bounds": bounds_json(&info),
            "copied": copied,
            "problems": problems,
        });
        let mut summary = format!(
            "{} {asset} ({} bytes){}.",
            if copied { "Imported" } else { "Already imported:" },
            info.bytes,
            info.drawn.map(|(min, _)| format!(", ground_height {:.3} m", min[1])).unwrap_or_default()
        );
        if !problems.is_empty() {
            summary.push_str(&format!(" Studio will not draw it as authored, so it cannot be placed yet: {}.", problems.join("; ")));
        }
        if let Some(spec) = input.get("place") {
            match placeable_asset(ctx, &asset, shared).and_then(|found| place_one(ctx, &found, spec)) {
                Ok(instance) => {
                    summary.push_str(&format!(" Placed {}.", instance["name"].as_str().unwrap_or("it")));
                    data["instance"] = instance;
                }
                Err(e) => return result(TOOL, false, format!("{summary} Not placed: {e}"), Some(data)),
            }
        }
        result(TOOL, true, summary, Some(data))
    }
}

// ── place_instances ─────────────────────────────────────────────────────────

pub struct PlaceInstancesTool;

impl ToolHandler for PlaceInstancesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "place_instances",
            description: "Place many Parts that use one imported GLB (import_asset it first), up to 500 per call, each \
                through the editor's own instance writer, which the engine spawns live. Each placement takes parent, \
                name, position, rotation, scale, anchored, can_collide and unit, as import_asset's `place` does: \
                rotation is a quaternion [x, y, z, w] or Euler degrees [x, y, z] like Roblox CFrame.Angles, scale \
                multiplies the mesh's size (default [1, 1, 1]), anchored and can_collide default true. The reply lists \
                every placed Part and the mesh's drawn bounds (ground_height is its lowest point relative to its \
                origin, in metres). A placement that fails is reported in `failed` and the others still go ahead. A \
                GLB Studio would not draw as authored is refused, with the reason.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "asset": { "type": "string", "description": "The GLB as import_asset returned it: Space-relative, e.g. Assets/Meshes/house_a.glb, or with shared assets/meshes/house_a.glb." },
                    "shared": { "type": "boolean", "description": "`asset` is in the Universe's shared mesh folder (import_asset with shared=true). Default false." },
                    "instances": {
                        "type": "array",
                        "maxItems": MAX_PLACEMENTS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "parent": { "type": "string" },
                                "name": { "type": "string" },
                                "position": { "type": "array", "items": { "type": "number" } },
                                "rotation": { "type": "array", "items": { "type": "number" } },
                                "scale": { "type": "array", "items": { "type": "number" } },
                                "anchored": { "type": "boolean" },
                                "can_collide": { "type": "boolean" },
                                "unit": { "type": "string" }
                            }
                        }
                    }
                },
                "required": ["asset", "instances"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.tool.place_instances"],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        const TOOL: &str = "place_instances";
        let raw = input.get("asset").and_then(Value::as_str).unwrap_or("").trim();
        let Some(specs) = input.get("instances").and_then(Value::as_array) else {
            return result(TOOL, false, "`instances` is required: an array of placements".into(), None);
        };
        if specs.is_empty() || specs.len() > MAX_PLACEMENTS {
            return result(TOOL, false, format!("give between 1 and {MAX_PLACEMENTS} placements (got {})", specs.len()), None);
        }
        let shared = input.get("shared").and_then(Value::as_bool).unwrap_or(false);
        let asset = match placeable_asset(ctx, raw, shared) {
            Ok(found) => found,
            Err(e) => return result(TOOL, false, e, None),
        };
        let info = &asset.info;
        let mut placed = Vec::new();
        let mut failed = Vec::new();
        for (i, spec) in specs.iter().enumerate() {
            if ctx.is_cancelled() {
                failed.push(json!({ "index": i, "error": "cancelled before this placement" }));
                continue;
            }
            match place_one(ctx, &asset, spec) {
                Ok(instance) => placed.push(instance),
                Err(e) => failed.push(json!({ "index": i, "error": e })),
            }
        }
        let summary = format!(
            "Placed {} of {} using {raw}{}{}",
            placed.len(),
            specs.len(),
            info.drawn.map(|(min, _)| format!(" (ground_height {:.3} m)", min[1])).unwrap_or_default(),
            if failed.is_empty() { ".".to_string() } else { format!("; {} failed (see `failed`).", failed.len()) }
        );
        result(
            TOOL,
            !placed.is_empty(),
            summary,
            Some(json!({ "asset": raw, "bounds": bounds_json(&info), "placed": placed, "failed": failed })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(root: &Path) -> ToolContext {
        ToolContext {
            space_root: root.join("Spaces").join("S"),
            universe_root: root.to_path_buf(),
            user_id: None,
            username: None,
            mlua_executor: None,
            display_unit: None,
            cancelled: None,
            permissions: crate::Permissions::standard(),
        }
    }

    fn universe(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eus_import_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Spaces").join("S").join("Workspace")).unwrap();
        std::fs::create_dir_all(root.join("Imports")).unwrap();
        root
    }

    /// A GLB with no binary chunk (only bounds are read): one node, its mesh's
    /// box from (-1, -0.5, -2) to (1, 1, 2), `extra` more primitives, and
    /// the node lifted `lift` metres.
    fn glb_with(lift: f64, extra: usize) -> Vec<u8> {
        let mut prims = vec![json!({ "attributes": { "POSITION": 0 }, "material": 0 })];
        prims.extend((0..extra).map(|_| json!({ "attributes": { "POSITION": 0 }, "material": 0 })));
        let doc = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "translation": [0.0, lift, 0.0] }],
            "meshes": [{ "primitives": prims }],
            "materials": [{}],
            "accessors": [{ "componentType": 5126, "count": 3, "type": "VEC3", "min": [-1.0, -0.5, -2.0], "max": [1.0, 1.0, 2.0] }]
        });
        let mut json = serde_json::to_vec(&doc).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let total = 12 + 8 + json.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out
    }

    fn glb() -> Vec<u8> {
        glb_with(0.0, 0)
    }

    #[test]
    fn a_glb_is_checked_and_read_as_studio_draws_it() {
        let root = universe("glb");
        let path = root.join("Imports").join("house.glb");
        std::fs::write(&path, glb()).unwrap();
        let info = read_glb(&path).unwrap();
        assert_eq!(info.drawn, Some(([-1.0, -0.5, -2.0], [1.0, 1.0, 2.0])));
        assert!(info.problems().is_empty(), "{:?}", info.problems());

        // Studio ignores the node's transform, so the drawn bounds do too,
        // and the lift is a problem to fix in Blender.
        std::fs::write(&path, glb_with(2.0, 0)).unwrap();
        let lifted = read_glb(&path).unwrap();
        assert_eq!(lifted.drawn, info.drawn);
        assert!(lifted.problems().iter().any(|p| p.contains("Ctrl+A")), "{:?}", lifted.problems());

        std::fs::write(&path, glb_with(0.0, 2)).unwrap();
        let parts = read_glb(&path).unwrap();
        assert!(parts.problems().iter().any(|p| p.contains("3 part(s)")), "{:?}", parts.problems());

        let mut short = glb();
        short.truncate(short.len() - 4);
        std::fs::write(&path, &short).unwrap();
        assert!(read_glb(&path).unwrap_err().contains("truncated"));
        std::fs::write(&path, b"solid stl ascii").unwrap();
        assert!(read_glb(&path).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_node_tree_that_repeats_is_refused_not_walked_forever() {
        let doc = json!({
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "children": [1, 1] }, { "children": [0] }],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }]
        });
        assert!(analyse(&doc, 0).unwrap_err().contains("repeats"));
        let missing = json!({ "scenes": [{ "nodes": [7] }], "nodes": [] });
        assert!(analyse(&missing, 0).is_err());
    }

    #[test]
    fn importing_copies_once_and_refuses_other_formats_and_outside_paths() {
        let root = universe("import");
        let c = ctx(&root);
        std::fs::write(root.join("Imports").join("house.glb"), glb()).unwrap();
        let r = ImportAssetTool.execute(json!({ "source": "Imports/house.glb" }), &c);
        assert!(r.success, "{}", r.content);
        assert_eq!(std::fs::read(c.space_root.join("Assets/Meshes/house.glb")).unwrap(), glb());
        let data = r.structured_data.unwrap();
        assert_eq!(data["asset"], "Assets/Meshes/house.glb");
        assert_eq!(data["bounds"]["ground_height"], -0.5);
        assert_eq!(data["problems"], json!([]));

        let again = ImportAssetTool.execute(json!({ "source": "Imports/house.glb" }), &c);
        assert!(again.success && again.content.starts_with("Already imported"), "{}", again.content);

        std::fs::write(root.join("Imports").join("house.glb"), glb_with(1.0, 0)).unwrap();
        let differs = ImportAssetTool.execute(json!({ "source": "Imports/house.glb" }), &c);
        assert!(!differs.success && differs.content.contains("overwrite"), "{}", differs.content);
        let replaced = ImportAssetTool.execute(json!({ "source": "Imports/house.glb", "overwrite": true }), &c);
        assert!(replaced.success && replaced.content.contains("cannot be placed yet"), "{}", replaced.content);
        assert!(!c.space_root.join("Assets/Meshes/house.glb.importing").exists());

        std::fs::write(root.join("Imports").join("part.stl"), b"solid x").unwrap();
        let stl = ImportAssetTool.execute(json!({ "source": "Imports/part.stl" }), &c);
        assert!(!stl.success && stl.content.contains("not supported yet"), "{}", stl.content);

        for outside in ["../house.glb", "C:/Windows/x.glb", "//host/share/x.glb", "Imports/../../x.glb"] {
            let r = ImportAssetTool.execute(json!({ "source": outside }), &c);
            assert!(!r.success, "{outside}: {}", r.content);
        }
        for dest in ["../../escape.glb", "C:/escape.glb", "Assets/Meshes/house.txt"] {
            let r = ImportAssetTool.execute(json!({ "source": "Imports/house.glb", "dest": dest, "overwrite": true }), &c);
            assert!(!r.success, "{dest}: {}", r.content);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn placing_writes_a_part_that_draws_the_mesh_at_its_own_size() {
        let root = universe("place");
        let c = ctx(&root);
        std::fs::write(root.join("Imports").join("house.glb"), glb()).unwrap();
        let r = ImportAssetTool.execute(
            json!({ "source": "Imports/house.glb", "place": {
                "parent": "City", "name": "House 1", "position": [10, 0, 5], "unit": "ft",
                "rotation": [0, 90, 0], "can_collide": false
            } }),
            &c,
        );
        assert!(r.success, "{}", r.content);
        let file = PathBuf::from(r.structured_data.unwrap()["instance"]["file"].as_str().unwrap());
        let doc: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
        assert_eq!(doc["asset"]["mesh"].as_str(), Some("../../../Assets/Meshes/house.glb"));
        let resolved = file.parent().unwrap().join(doc["asset"]["mesh"].as_str().unwrap());
        assert_eq!(std::fs::read(resolved).unwrap(), glb(), "the reference reaches the imported file");
        assert_eq!(doc["properties"]["anchored"].as_bool(), Some(true));
        assert_eq!(doc["properties"]["can_collide"].as_bool(), Some(false));
        let floats = |key: &str| -> Vec<f64> {
            doc["transform"][key].as_array().unwrap().iter().map(|v| v.as_float().unwrap()).collect()
        };
        assert_eq!(floats("scale"), vec![1.0, 1.0, 1.0], "not the Part template's block size");
        let pos = floats("position");
        assert!((pos[0] - 3.048).abs() < 1e-4 && (pos[2] - 1.524).abs() < 1e-4, "feet written as metres: {pos:?}");
        assert!(doc.get("metadata").and_then(|m| m.get("unit")).is_none(), "no unit stamped");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn place_instances_places_many_and_bounds_the_batch() {
        let root = universe("batch");
        let c = ctx(&root);
        std::fs::write(root.join("Imports").join("lamp.glb"), glb()).unwrap();
        assert!(ImportAssetTool.execute(json!({ "source": "Imports/lamp.glb" }), &c).success);
        let mut specs: Vec<Value> = (0..3).map(|i| json!({ "parent": "Street", "name": "Lamp", "position": [i * 4, 0, 0] })).collect();
        specs.push(json!({ "name": "Bad", "scale": [1, 0, 1] }));
        let r = PlaceInstancesTool.execute(json!({ "asset": "Assets/Meshes/lamp.glb", "instances": specs }), &c);
        assert!(r.success, "{}", r.content);
        let data = r.structured_data.unwrap();
        let names: Vec<String> = data["placed"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names.len(), 3);
        assert_eq!(names.iter().collect::<std::collections::HashSet<_>>().len(), 3, "unique names: {names:?}");
        assert_eq!(data["failed"][0]["index"], 3);

        let too_many: Vec<Value> = (0..=MAX_PLACEMENTS).map(|_| json!({})).collect();
        assert!(!PlaceInstancesTool.execute(json!({ "asset": "Assets/Meshes/lamp.glb", "instances": too_many }), &c).success);
        assert!(!PlaceInstancesTool.execute(json!({ "asset": "Assets/Meshes/missing.glb", "instances": [{}] }), &c).success);
        assert!(!PlaceInstancesTool.execute(json!({ "asset": "../lamp.glb", "instances": [{}] }), &c).success);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_mesh_studio_would_misdraw_is_imported_but_not_placed() {
        let root = universe("misdraw");
        let c = ctx(&root);
        // A shape word in a name is the author's own mesh; only `<shape>.glb`
        // in a `parts` folder reads as a built-in shape.
        std::fs::write(root.join("Imports").join("city_block_a.glb"), glb()).unwrap();
        let named = ImportAssetTool.execute(json!({ "source": "Imports/city_block_a.glb", "place": {} }), &c);
        assert!(named.success, "{}", named.content);
        let shaped = ImportAssetTool.execute(
            json!({ "source": "Imports/city_block_a.glb", "dest": "Assets/parts/block.glb", "place": {} }),
            &c,
        );
        assert!(!shaped.success && shaped.content.contains("built-in Block shape"), "{}", shaped.content);
        assert!(c.space_root.join("Assets/parts/block.glb").is_file(), "imported even so");

        std::fs::write(root.join("Imports").join("tower.glb"), glb_with(0.0, 1)).unwrap();
        let parts = ImportAssetTool.execute(json!({ "source": "Imports/tower.glb", "place": {} }), &c);
        assert!(!parts.success && parts.content.contains("Ctrl+J"), "{}", parts.content);
        assert!(c.space_root.join("Assets/Meshes/tower.glb").is_file(), "imported even so");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shared_mesh_goes_to_the_universe_and_parts_name_it_from_there() {
        let root = universe("shared");
        let c = ctx(&root);
        std::fs::write(root.join("Imports").join("tree.glb"), glb()).unwrap();
        let r = ImportAssetTool.execute(json!({ "source": "Imports/tree.glb", "shared": true, "place": { "name": "Oak" } }), &c);
        assert!(r.success, "{}", r.content);
        let data = r.structured_data.unwrap();
        assert_eq!(data["asset"], "assets/meshes/tree.glb");
        assert_eq!(std::fs::read(eustress_common::space_path::shared_mesh_dir(&root).join("tree.glb")).unwrap(), glb());
        assert!(!c.space_root.join("Assets").exists(), "nothing was copied into the Space");
        let file = PathBuf::from(data["instance"]["file"].as_str().unwrap());
        let doc: toml::Table = std::fs::read_to_string(&file).unwrap().parse().unwrap();
        assert_eq!(doc["asset"]["mesh"].as_str(), Some("assets/meshes/tree.glb"), "the loader's Universe lookup");

        let more = PlaceInstancesTool.execute(json!({ "asset": "assets/meshes/tree.glb", "shared": true, "instances": [{ "name": "Pine" }] }), &c);
        assert!(more.success, "{}", more.content);
        // A shared asset is a path in the shared folder, and nothing else.
        for asset in ["Imports/tree.glb", "assets/meshes/../../Imports/tree.glb", "assets/meshes/missing.glb"] {
            let r = PlaceInstancesTool.execute(json!({ "asset": asset, "shared": true, "instances": [{}] }), &c);
            assert!(!r.success, "{asset}: {}", r.content);
        }
        assert!(!PlaceInstancesTool.execute(json!({ "asset": "assets/meshes/tree.glb", "instances": [{}] }), &c).success, "not in the Space");
        for dest in ["../escape.glb", "C:/escape.glb", "sub/../../escape.glb"] {
            let r = ImportAssetTool.execute(json!({ "source": "Imports/tree.glb", "shared": true, "dest": dest }), &c);
            assert!(!r.success, "{dest}: {}", r.content);
        }
        assert!(!root.join("assets").join("escape.glb").exists() && !root.join("escape.glb").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rotation_reads_quaternions_and_cframe_angles() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(near(parse_rotation(&json!([0, 90, 0])).unwrap(), [0.0, s, 0.0, s]));
        assert!(near(parse_rotation(&json!([90, 0, 0])).unwrap(), [s, 0.0, 0.0, s]));
        // X first, then the turned Y: qx * qy, as CFrame.Angles composes.
        assert!(near(parse_rotation(&json!([90, 90, 0])).unwrap(), [0.5, 0.5, 0.5, 0.5]));
        assert!(near(parse_rotation(&json!([0, 0, 0, 2])).unwrap(), [0.0, 0.0, 0.0, 1.0]));
        assert!(parse_rotation(&json!([0, 0, 0, 0])).is_err());
        assert!(parse_rotation(&json!([1, 2])).is_err());
    }

    #[test]
    fn both_tools_are_write_class_so_a_standard_caller_may_run_them() {
        for tool in ["import_asset", "place_instances"] {
            assert_eq!(crate::capability_of(tool), Some(crate::Capability::Write), "{tool}");
            assert!(crate::authorize(tool, &crate::Permissions::standard()).is_ok(), "{tool}");
        }
    }

    #[test]
    fn the_mesh_reference_climbs_from_the_new_folder() {
        let base = Path::new("U/Spaces/S/Workspace/City");
        let target = Path::new("U/Spaces/S/Assets/Meshes/a.glb");
        assert_eq!(mesh_reference(base, target).as_deref(), Some("../../../Assets/Meshes/a.glb"));
    }
}
