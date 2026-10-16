//! Moving a Space's native wedges onto the Roblox-correct meshes (Space
//! format 2, [`space_format::WEDGE_MESHES`]), keeping everything where it was
//! drawn.
//!
//! The wedge meshes are Roblox's: a wedge's tall face is at its back, and a
//! corner wedge rises at its right front corner. A wedge built natively
//! against the meshes Eustress drew before them is turned to fit
//! ([`crate::part_meshes`] has each turn). [`plan`] reads a Space's records and
//! gives each file that changes, whole:
//!
//! 1. A native wedge or corner wedge takes its turn about its own up axis
//!    (`part_meshes::migrate_native_wedge`), a corner wedge's width and depth
//!    swapping. A part qualifies when `part_meshes::native_wedge_shape` names
//!    it AND it draws that shape (a part whose DataMesh child draws something
//!    else does not). A Roblox import (`part_meshes::is_roblox_import`) is
//!    left as it is, and so is everything placed on it.
//! 2. Every pose given relative to a turned wedge takes the inverse turn, so it
//!    stays where it was in the world: the `[transform]` of each instance
//!    placed on the wedge's frame (under either pose rule, looking through the
//!    folders and scripts that pass a frame through; an Attachment's or a
//!    Bone's under the legacy rule too, whose `[transform]` is local to the
//!    part), an Attachment's stored axes, and a joint's `c0` or `c1` when its
//!    `Part0` or `Part1` is the wedge.
//! 3. Every face named on a turned wedge is remapped (`part_meshes::remap_face`):
//!    a Decal's, a Texture's, a SurfaceGui's, a SpotLight's or SurfaceLight's
//!    face, and a ParticleEmitter's emission direction.
//!
//! Values a file leaves out are its class template's, as every reader heals
//! them (a corner wedge with no `scale` has the template's size), and a turned
//! value is written whole.
//!
//! Studio writes a plan to the Space's files and database when it opens the
//! Space, then the format number. A reader that never writes (the Player)
//! reads a Space whose format is older through [`migrated_records`], the same
//! plan in memory, so both see one world. The walk is the Player's reader's
//! own ([`posed_records`]), under the Space's own pose rule.

use std::collections::{BTreeMap, HashMap};

use bevy::math::{Quat, Vec3};

use crate::class_schema::{heal_instance_value, migrate_legacy_light_extras, ClassSchemaRegistry};
use crate::classes::{ClassName, NormalId, PartType};
use crate::datamodel::record::{is_data_mesh_class, raw_class_name, record_mesh, TransformRule};
use crate::part_meshes::{
    is_roblox_import, migrate_native_wedge, migrate_pose_relative_to_wedge, native_wedge_shape, native_wedge_turn,
    remap_face,
};
use crate::space_format;
use crate::space_read::{clean_pose, primitive_shape};
use crate::tree_read::{normalize_key, posed_records, PosedRecord};

/// One file's new text, and what changed in it.
#[derive(Debug, Clone, PartialEq)]
pub struct WedgeRewrite {
    /// The record's key, exactly as the caller gave it.
    pub key: String,
    /// The whole new file, every table it held kept.
    pub text: String,
    /// What changed, one line each, for a migration log.
    pub changes: Vec<String>,
}

/// What [`plan`] found.
#[derive(Debug, Default)]
pub struct WedgePlan {
    /// The files to rewrite, in key order, each once.
    pub rewrites: Vec<WedgeRewrite>,
    /// Every turned wedge's `[metadata] uuid` ([`normalise_uuid`]d) and its
    /// shape, sorted, so a baked copy of it takes the same turn. A wedge
    /// without a uuid is turned and not listed.
    pub turned: Vec<(String, PartType)>,
    /// Native wedges turned.
    pub wedges: usize,
    /// Native corner wedges turned.
    pub corner_wedges: usize,
    /// Instances placed on a turned wedge whose pose was turned back.
    pub children: usize,
    /// Joint frames on a turned wedge, turned back.
    pub joints: usize,
    /// Faces on a turned wedge, remapped.
    pub faces: usize,
    /// Files of Roblox-imported wedges (by `part_meshes::native_wedge_shape`
    /// and the import mark), left as they are.
    pub roblox_left: usize,
    /// Records left as they are, one line each, and why.
    pub problems: Vec<String>,
}

/// The rewrites that move `records` (a Space's files keyed by their path from
/// the Space's folder, with `space.toml` among them for the pose rule) onto
/// the Roblox-correct wedge meshes. It reads whatever format the Space names:
/// the caller asks [`space_format::needs_wedge_migration`] first.
pub fn plan(records: &[(String, Vec<u8>)]) -> WedgePlan {
    let mut out = WedgePlan::default();
    // Only a native wedge needs the walk: a Space whose wedges are all Roblox
    // imports, or that has none, has nothing to plan and is not walked.
    let mut native = false;
    for (key, bytes) in records {
        if !key.ends_with(".toml") || !names_a_wedge(bytes) {
            continue;
        }
        let Some(doc) = std::str::from_utf8(bytes).ok().and_then(|text| text.parse::<toml::Value>().ok()) else { continue };
        if native_wedge_shape(raw_class_name(&doc).unwrap_or_default(), asset_mesh(&doc)).is_none() {
            continue;
        }
        if is_roblox_import(|key| metadata_has(&doc, key)) {
            out.roblox_left += 1;
        } else {
            native = true;
        }
    }
    if !native {
        return out;
    }
    let rule = TransformRule::of_space(space_toml(records).as_ref());
    let walk = posed_records(records, rule);
    let originals: HashMap<String, (&str, &[u8])> =
        records.iter().map(|(key, bytes)| (normalize_key(key), (key.as_str(), bytes.as_slice()))).collect();
    let at: HashMap<&str, usize> = walk.iter().enumerate().map(|(i, r)| (r.key.as_str(), i)).collect();
    let registry = ClassSchemaRegistry::builtin();

    // Each walked record as written, and as its readers read it.
    let docs: Vec<Option<(toml::Value, toml::Value)>> = walk
        .iter()
        .map(|r| {
            let (_, bytes) = originals.get(&r.key)?;
            let doc = std::str::from_utf8(bytes).ok()?.parse::<toml::Value>().ok()?;
            let healed = heal_instance_value(doc.clone(), registry).map(|h| h.value).unwrap_or_else(|_| doc.clone());
            Some((doc, healed))
        })
        .collect();

    // A part whose DataMesh child draws instead of its own mesh.
    let mut redrawn = vec![false; walk.len()];
    for r in &walk {
        if is_data_mesh_class(r.class) {
            if let Some(&p) = tree_parent(r, &at, &walk) {
                redrawn[p] = true;
            }
        }
    }

    // 1. The native wedges, and the turn each takes.
    let mut shapes: Vec<Option<(PartType, Quat)>> = vec![None; walk.len()];
    let mut turn_by_uuid: HashMap<String, Quat> = HashMap::new();
    for (i, r) in walk.iter().enumerate() {
        if !r.from_file || redrawn[i] {
            continue;
        }
        let Some((doc, healed)) = &docs[i] else { continue };
        let class = raw_class_name(doc).unwrap_or_default();
        let Some(shape) = native_wedge_shape(class, asset_mesh(healed)) else { continue };
        let drawn = record_mesh(r.class, healed).map(|mesh| primitive_shape(&mesh).unwrap_or(PartType::Block));
        if drawn != Some(shape) || is_roblox_import(|key| metadata_has(doc, key)) {
            continue;
        }
        let Some(turn) = native_wedge_turn(shape) else { continue };
        shapes[i] = Some((shape, turn));
        if let Some(uuid) = record_uuid(doc) {
            turn_by_uuid.insert(uuid.clone(), turn);
            out.turned.push((uuid, shape));
        }
        match shape {
            PartType::CornerWedge => out.corner_wedges += 1,
            _ => out.wedges += 1,
        }
    }
    if shapes.iter().all(Option::is_none) {
        return out;
    }

    // 2 and 3. Every record, parents first: its own turn, the turn of the
    // wedge it is placed on, its joint frames and its face.
    let mut rewrites: BTreeMap<&str, WedgeRewrite> = BTreeMap::new();
    for (i, r) in walk.iter().enumerate() {
        let frame_turn = frame_parent(r, &at, &walk).and_then(|p| shapes[p]).map(|(_, turn)| turn);
        let face_turn = tree_parent(r, &at, &walk).and_then(|&p| shapes[p]).map(|(_, turn)| turn);
        let own = shapes[i];
        let Some((doc, healed)) = &docs[i] else {
            if frame_turn.is_some() || face_turn.is_some() {
                out.problems.push(format!("{}: placed on a turned wedge but not readable, left as it is", r.key));
            }
            continue;
        };
        let joint_turns = joint_turns(r.class, doc, &turn_by_uuid);
        if own.is_none() && frame_turn.is_none() && face_turn.is_none() && joint_turns.iter().all(|(_, t)| t.is_none()) {
            continue;
        }

        let mut new = doc.clone();
        let mut changes = Vec::new();
        let posed_here = r.posed || matches!(r.class, ClassName::Attachment | ClassName::Bone);
        let pose_turn = frame_turn.filter(|_| posed_here && !(places_by_face(r.class) && section(doc, "transform").is_none()));
        if pose_turn.is_some() || own.is_some() {
            turn_pose(&mut new, doc, healed, pose_turn, own.map(|(shape, _)| shape), &mut changes);
            if pose_turn.is_some() {
                out.children += 1;
            }
        }
        if let (Some(turn), ClassName::Attachment) = (pose_turn, r.class) {
            turn_axes(&mut new, doc, healed, turn, &mut changes);
        }
        for (base, turn) in joint_turns {
            if let Some(turn) = turn {
                turn_joint_frame(&mut new, healed, base, turn, &mut changes);
                out.joints += 1;
            }
        }
        if let Some(turn) = face_turn {
            match remap(&mut new, healed, r.class, turn, &mut changes) {
                Ok(true) => out.faces += 1,
                Ok(false) => {}
                Err(problem) => out.problems.push(format!("{}: {problem}", r.key)),
            }
        }
        if changes.is_empty() {
            continue;
        }
        let Some(&(key, _)) = originals.get(&r.key) else { continue };
        match toml::to_string_pretty(&new) {
            Ok(text) => {
                rewrites.insert(key, WedgeRewrite { key: key.to_string(), text, changes });
            }
            Err(e) => out.problems.push(format!("{}: {e}, left as it is", r.key)),
        }
    }
    out.rewrites = rewrites.into_values().collect();
    out.turned.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `records` as a Space reads once its wedges are moved: a Space whose
/// `space.toml` names a format before [`space_format::WEDGE_MESHES`], with
/// [`plan`]'s rewrites and that format number, in memory. `None` when nothing
/// changes, so the caller reads `records` as they are; on its own output it is
/// always `None`. The second value is the plan's problems. A reader runs it
/// through `space_format::migrated_records`, which takes a Space through
/// every later step too.
pub fn migrated_records(records: &[(String, Vec<u8>)]) -> Option<(Vec<(String, Vec<u8>)>, Vec<String>)> {
    let format = space_format::of_space(space_toml(records).as_ref());
    if !space_format::needs_wedge_migration(format) {
        return None;
    }
    let plan = plan(records);
    if plan.rewrites.is_empty() {
        return None;
    }
    let by_key: HashMap<&str, &str> = plan.rewrites.iter().map(|r| (r.key.as_str(), r.text.as_str())).collect();
    let mut named = false;
    let mut out: Vec<(String, Vec<u8>)> = records
        .iter()
        .map(|(key, bytes)| {
            let text = if normalize_key(key) == "space.toml" {
                named = true;
                Some(space_format::with_format(std::str::from_utf8(bytes).ok(), space_format::WEDGE_MESHES))
            } else {
                by_key.get(key.as_str()).map(|text| text.to_string())
            };
            (key.clone(), text.map_or_else(|| bytes.clone(), String::into_bytes))
        })
        .collect();
    if !named {
        out.push(("space.toml".to_string(), space_format::with_format(None, space_format::WEDGE_MESHES).into_bytes()));
    }
    Some((out, plan.problems))
}

/// A uuid as references compare it: lowercase, without hyphens.
pub fn normalise_uuid(uuid: &str) -> String {
    uuid.trim().chars().filter(|c| *c != '-').flat_map(char::to_lowercase).collect()
}

// ── The walk ───────────────────────────────────────────────────────────────

/// Whether a file's bytes name a wedge anywhere (a class or a mesh), in any
/// case: the first look that spares a Space without one its walk.
fn names_a_wedge(bytes: &[u8]) -> bool {
    bytes.windows(5).any(|w| w.eq_ignore_ascii_case(b"wedge"))
}

fn space_toml(records: &[(String, Vec<u8>)]) -> Option<toml::Value> {
    let (_, bytes) = records.iter().find(|(key, _)| normalize_key(key) == "space.toml")?;
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// The record whose frame `r` composes onto: its nearest ancestor read from a
/// file that places itself. An ancestor that does not (a script, a light, a
/// legacy Attachment) passes its parent's frame through, as a folder without a
/// file does.
fn frame_parent(r: &PosedRecord, at: &HashMap<&str, usize>, walk: &[PosedRecord]) -> Option<usize> {
    let mut up = r.parent.as_deref();
    while let Some(key) = up {
        let p = *at.get(key)?;
        if walk[p].posed {
            return Some(p);
        }
        up = walk[p].parent.as_deref();
    }
    None
}

/// The record `r` hangs directly under in the tree: its nearest ancestor read
/// from a file, when no folder stands between them.
fn tree_parent<'a>(r: &PosedRecord, at: &'a HashMap<&str, usize>, walk: &[PosedRecord]) -> Option<&'a usize> {
    let parent = r.parent.as_deref()?;
    let p = at.get(parent)?;
    let own_folder = r.key.strip_suffix("/_instance.toml").unwrap_or(&r.key);
    let enclosing = own_folder.rsplit_once('/').map(|(dir, _)| dir)?;
    (walk[*p].key.strip_suffix("/_instance.toml") == Some(enclosing)).then_some(p)
}

/// Classes placed by a face of their part rather than a pose of their own: a
/// file of one that names no `[transform]` is given none.
fn places_by_face(class: ClassName) -> bool {
    matches!(
        class,
        ClassName::Decal
            | ClassName::Texture
            | ClassName::SurfaceGui
            | ClassName::SpotLight
            | ClassName::SurfaceLight
            | ClassName::ParticleEmitter
    )
}

/// Where a class names its face: the section and the key.
fn face_key(class: ClassName) -> Option<(&'static str, &'static str)> {
    match class {
        ClassName::Decal => Some(("decal", "face")),
        ClassName::Texture => Some(("texture", "face")),
        ClassName::SurfaceGui => Some(("gui", "face")),
        ClassName::SpotLight | ClassName::SurfaceLight => Some(("light", "face")),
        ClassName::ParticleEmitter => Some(("particle", "emission_direction")),
        _ => None,
    }
}

// ── The edits ──────────────────────────────────────────────────────────────

/// A record's pose: turned back onto the wedge it is placed on
/// (`frame_turn`), then turned as a wedge itself (`own`), from the values its
/// readers read. The rotation is written whole; the position when it was
/// written or is no longer the origin; a corner wedge's size always, since
/// its width and depth swap.
fn turn_pose(
    new: &mut toml::Value,
    doc: &toml::Value,
    healed: &toml::Value,
    frame_turn: Option<Quat>,
    own: Option<PartType>,
    changes: &mut Vec<String>,
) {
    let read = section(healed, "transform");
    let pose = clean_pose(
        floats(read, "position").unwrap_or([0.0; 3]),
        floats(read, "rotation").unwrap_or([0.0, 0.0, 0.0, 1.0]),
        floats(read, "scale").unwrap_or([1.0; 3]),
    );
    let (mut rotation, mut position, mut size) = (pose.rotation, pose.translation, pose.scale);
    if let Some(turn) = frame_turn {
        (rotation, position) = migrate_pose_relative_to_wedge(turn, rotation, position);
        changes.push("pose turned back onto the wedge it is placed on".to_string());
    }
    if let Some(shape) = own {
        (rotation, size) = migrate_native_wedge(shape, rotation, size);
        changes.push(match shape {
            PartType::CornerWedge => "corner wedge turned a quarter turn, its width and depth swapped".to_string(),
            _ => "wedge turned a half turn".to_string(),
        });
    }
    let had_position = section(doc, "transform").and_then(|t| get_ci(t, "position")).is_some();
    let Some(transform) = section_mut(new, "transform") else { return };
    set_floats(transform, "rotation", &rotation.to_array());
    if frame_turn.is_some() && (had_position || position != Vec3::ZERO) {
        set_floats(transform, "position", &position.to_array());
    }
    if own == Some(PartType::CornerWedge) {
        set_floats(transform, "scale", &size.to_array());
    }
}

/// An Attachment's stored axes, directions in its part's frame, turned back
/// with its pose. An axis the file does not store is left out.
fn turn_axes(new: &mut toml::Value, doc: &toml::Value, healed: &toml::Value, turn: Quat, changes: &mut Vec<String>) {
    let (written, read) = (section(doc, "attachment"), section(healed, "attachment"));
    let back = turn.inverse();
    for key in ["axis", "secondary_axis"] {
        if written.and_then(|s| get_ci(s, key)).is_none() {
            continue;
        }
        let Some(axis) = floats::<3>(read, key) else { continue };
        let Some(attachment) = section_mut(new, "attachment") else { return };
        set_floats(attachment, key, &(back * Vec3::from_array(axis)).to_array());
        changes.push(format!("{key} turned back"));
    }
}

/// Which of a joint's frames sit on a turned wedge: `c0` when its `Part0` is
/// one, `c1` when its `Part1` is. A WeldConstraint keeps its parts as they
/// are placed and has no frames, unless its file writes them.
fn joint_turns(class: ClassName, doc: &toml::Value, turn_by_uuid: &HashMap<String, Quat>) -> [(&'static str, Option<Quat>); 2] {
    let refs = section(doc, "references").filter(|_| !turn_by_uuid.is_empty());
    let turn = |name: &str, base: &str| {
        let turn = refs
            .and_then(|r| get_ci(r, name))
            .and_then(|v| v.as_str())
            .and_then(|uuid| turn_by_uuid.get(&normalise_uuid(uuid)).copied())?;
        let framed = class != ClassName::WeldConstraint
            || section(doc, "constraint").is_some_and(|c| get_ci(c, base).is_some() || get_ci(c, &format!("{base}_rotation")).is_some());
        framed.then_some(turn)
    };
    [("c0", turn("part0", "c0")), ("c1", turn("part1", "c1"))]
}

/// A joint's frame in a turned wedge (`c0` and `c0_rotation`, or `c1` and
/// `c1_rotation`), turned back. A frame the file leaves out is the identity,
/// Roblox's default, and is written.
fn turn_joint_frame(new: &mut toml::Value, healed: &toml::Value, base: &str, turn: Quat, changes: &mut Vec<String>) {
    let read = section(healed, "constraint");
    let rotation_key = format!("{base}_rotation");
    let pose = clean_pose(
        floats(read, base).unwrap_or([0.0; 3]),
        floats(read, &rotation_key).unwrap_or([0.0, 0.0, 0.0, 1.0]),
        [1.0; 3],
    );
    let (rotation, position) = migrate_pose_relative_to_wedge(turn, pose.rotation, pose.translation);
    let Some(constraint) = section_mut(new, "constraint") else { return };
    set_floats(constraint, base, &position.to_array());
    set_floats(constraint, &rotation_key, &rotation.to_array());
    changes.push(format!("{base} turned back onto the wedge"));
}

/// A face named on a turned wedge, remapped so it stays on the same side.
/// `Ok(true)` when it moved. A light's legacy extras are first moved where
/// every reader moves them, so they cannot bring the old face back.
fn remap(new: &mut toml::Value, healed: &toml::Value, class: ClassName, turn: Quat, changes: &mut Vec<String>) -> Result<bool, String> {
    let Some((section_name, key)) = face_key(class) else { return Ok(false) };
    let Some(value) = section(healed, section_name).and_then(|s| get_ci(s, key)) else { return Ok(false) };
    let face = face_name(value).ok_or_else(|| format!("{section_name}.{key} {value} is not a face, left as it is"))?;
    let Some(moved) = remap_face(turn, face).filter(|moved| *moved != face) else { return Ok(false) };
    if matches!(class, ClassName::SpotLight | ClassName::SurfaceLight) {
        migrate_legacy_light_extras(new);
    }
    let Some(table) = section_mut(new, section_name) else { return Ok(false) };
    let spelled = table.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned().unwrap_or_else(|| key.to_string());
    table.insert(spelled, toml::Value::String(moved.to_string()));
    changes.push(format!("{section_name}.{key} {face} is now {moved}"));
    Ok(true)
}

/// A face as Roblox names it, read as every face reader reads one
/// ([`NormalId::parse`]): a name in any case, with or without an
/// `Enum.NormalId.` prefix, or its NormalId number.
fn face_name(value: &toml::Value) -> Option<&'static str> {
    let text = match value {
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::String(s) => s.clone(),
        _ => return None,
    };
    NormalId::parse(&text).map(|face| face.as_str())
}

// ── Reading and writing values ─────────────────────────────────────────────

fn get_ci<'v>(v: &'v toml::Value, key: &str) -> Option<&'v toml::Value> {
    v.as_table()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

fn section<'v>(doc: &'v toml::Value, name: &str) -> Option<&'v toml::Value> {
    get_ci(doc, name).filter(|s| s.is_table())
}

/// The section in the file's own spelling, made when the file has none.
fn section_mut<'v>(doc: &'v mut toml::Value, name: &str) -> Option<&'v mut toml::value::Table> {
    let root = doc.as_table_mut()?;
    let key = root.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.to_string());
    root.entry(key).or_insert_with(|| toml::Value::Table(toml::value::Table::new())).as_table_mut()
}

/// `N` numbers under `key`, or `None` when it holds anything else.
fn floats<const N: usize>(table: Option<&toml::Value>, key: &str) -> Option<[f32; N]> {
    let array = get_ci(table?, key)?.as_array()?;
    if array.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, v) in out.iter_mut().zip(array) {
        *slot = v.as_float().or_else(|| v.as_integer().map(|i| i as f64))? as f32;
    }
    Some(out)
}

/// `values` under `key`, in the spelling the table already uses, each the
/// TOML float with the same shortest decimal form.
fn set_floats(table: &mut toml::value::Table, key: &str, values: &[f32]) {
    let spelled = table.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned().unwrap_or_else(|| key.to_string());
    let float = |x: f32| toml::Value::Float(format!("{x}").parse::<f64>().unwrap_or(f64::from(x)));
    table.insert(spelled, toml::Value::Array(values.iter().map(|v| float(*v)).collect()));
}

fn asset_mesh(doc: &toml::Value) -> Option<&str> {
    section(doc, "asset").and_then(|a| get_ci(a, "mesh")).and_then(|m| m.as_str())
}

fn metadata_has(doc: &toml::Value, key: &str) -> bool {
    section(doc, "metadata").and_then(|m| m.as_table()).is_some_and(|m| m.contains_key(key))
}

fn record_uuid(doc: &toml::Value) -> Option<String> {
    section(doc, "metadata")
        .and_then(|m| get_ci(m, "uuid"))
        .and_then(|u| u.as_str())
        .map(normalise_uuid)
        .filter(|u| !u.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::DmValue;
    use crate::scripting::CFrame;
    use crate::tree_read::{read_space_records, SceneTree};

    fn rec(key: &str, text: &str) -> (String, Vec<u8>) {
        (key.to_string(), text.as_bytes().to_vec())
    }

    /// A Space under `rule` (`""` for the legacy rule) holding `records`.
    fn space(rule: &str, records: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        let rule = if rule.is_empty() { String::new() } else { format!("transform_rule = \"{rule}\"\n") };
        let mut out = vec![
            rec("space.toml", &format!("[space]\nname = \"Garage\"\n{rule}")),
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
        ];
        out.extend(records.iter().map(|(k, v)| rec(k, v)));
        out
    }

    /// The same records, read as a Space already at the current format: the
    /// world as it was drawn, with nothing turned.
    fn as_drawn(records: &[(String, Vec<u8>)]) -> SceneTree {
        let current: Vec<_> = records
            .iter()
            .map(|(k, v)| {
                if k == "space.toml" {
                    let text = space_format::with_format(std::str::from_utf8(v).ok(), space_format::CURRENT);
                    (k.clone(), text.into_bytes())
                } else {
                    (k.clone(), v.clone())
                }
            })
            .collect();
        read_space_records(&current)
    }

    fn wedge(name: &str, extra: &str) -> String {
        format!(
            "[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\nuuid = \"AAAA-0001\"\n{extra}\n\
             [asset]\nmesh = \"parts/wedge.glb\"\n\n\
             [transform]\nposition = [3.0, 1.0, -2.0]\nrotation = [0.0, 0.38268343, 0.0, 0.9238795]\nscale = [2.0, 1.0, 4.0]\n"
        )
    }

    fn id(tree: &SceneTree, key: &str) -> crate::datamodel::InstanceId {
        tree.keys.iter().find(|(k, _)| k == key).map(|(_, id)| *id).unwrap_or_else(|| panic!("{key} not read"))
    }

    fn cframe(tree: &SceneTree, key: &str, prop: &str) -> CFrame {
        match tree.dm.get_prop(id(tree, key), prop) {
            Some(DmValue::CFrame(cf)) => cf,
            other => panic!("{key}.{prop}: {other:?}"),
        }
    }

    fn prop(tree: &SceneTree, key: &str, prop: &str) -> DmValue {
        tree.dm.get_prop(id(tree, key), prop).unwrap_or_else(|| panic!("{key}.{prop} missing"))
    }

    fn same_cframe(a: &CFrame, b: &CFrame) -> bool {
        let (ma, mb) = (a.rotation_matrix(), b.rotation_matrix());
        (a.position - b.position).magnitude() < 1e-4
            && (0..3).all(|i| (0..3).all(|j| (ma[i][j] - mb[i][j]).abs() < 1e-4))
    }

    fn turned(cf: &CFrame, turn: Quat) -> CFrame {
        *cf * CFrame::from_quaternion(turn.to_array().map(f64::from))
    }

    /// A native wedge turns half a turn, and everything placed on it stays
    /// where it was drawn, under either pose rule: a part, an attachment, a
    /// part inside a folder with no file, and a weld's frame.
    #[test]
    fn a_wedge_turns_and_what_it_holds_stays_put() {
        for rule in ["", "parent_pose"] {
            let records = space(
                rule,
                &[
                    ("Workspace/Roof/_instance.toml", &wedge("Roof", "")),
                    (
                        "Workspace/Roof/Vent/_instance.toml",
                        "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [0.5, 0.25, 0.0]\nrotation = [0.0, 0.0, 0.258819, 0.9659258]\nscale = [0.2, 0.2, 0.2]\n",
                    ),
                    (
                        "Workspace/Roof/Hook/_instance.toml",
                        "[metadata]\nclass_name = \"Attachment\"\n\n[transform]\nposition = [0.3, 0.0, 0.4]\n\n[attachment]\naxis = [1.0, 0.0, 0.0]\n",
                    ),
                    (
                        "Workspace/Roof/trim/Strip/_instance.toml",
                        "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [0.0, 0.5, 0.4]\nscale = [0.1, 0.1, 0.1]\n",
                    ),
                    (
                        "Workspace/Joint/_instance.toml",
                        "[metadata]\nclass_name = \"Weld\"\n\n[references]\nPart0 = \"aaaa0001\"\n\n[constraint]\nc0 = [0.0, 0.0, 1.0]\n",
                    ),
                ],
            );
            let plan = plan(&records);
            assert_eq!((plan.wedges, plan.corner_wedges, plan.joints), (1, 0, 1), "{rule:?}");
            assert_eq!(plan.children, 3, "{rule:?}: the vent, the hook and the strip");
            assert_eq!(plan.turned, vec![("aaaa0001".to_string(), PartType::Wedge)]);

            let (before, after) = (as_drawn(&records), read_space_records(&records));
            let turn = native_wedge_turn(PartType::Wedge).unwrap();
            let roof = "Workspace/Roof/_instance.toml";
            assert!(same_cframe(&cframe(&after, roof, "CFrame"), &turned(&cframe(&before, roof, "CFrame"), turn)), "{rule:?}");
            for key in ["Workspace/Roof/Vent/_instance.toml", "Workspace/Roof/trim/Strip/_instance.toml"] {
                assert!(same_cframe(&cframe(&after, key, "CFrame"), &cframe(&before, key, "CFrame")), "{rule:?}: {key} moved");
            }
            // An attachment's CFrame is local to its part under either rule.
            // The tree keys a folder built by its class by the folder; the
            // plan keys every rewrite by its file.
            let (hook, hook_file) = ("Workspace/Roof/Hook", "Workspace/Roof/Hook/_instance.toml");
            assert!(
                same_cframe(&cframe(&after, hook, "WorldCFrame"), &cframe(&before, hook, "WorldCFrame")),
                "{rule:?}: the hook moved"
            );
            let hook_text = &plan.rewrites.iter().find(|r| r.key == hook_file).unwrap().text;
            let axis: toml::Value = hook_text.parse().unwrap();
            let x = axis["attachment"]["axis"][0].as_float().unwrap();
            assert!((x + 1.0).abs() < 1e-6, "the stored axis turns back with the pose: {hook_text}");

            // The weld's frame on the roof, in the world, is where it was.
            let joint = plan.rewrites.iter().find(|r| r.key == "Workspace/Joint/_instance.toml").unwrap();
            let doc: toml::Value = joint.text.parse().unwrap();
            let c0 = |k: &str, n: usize| -> Vec<f32> {
                doc["constraint"][k].as_array().unwrap().iter().take(n).map(|v| v.as_float().unwrap() as f32).collect()
            };
            let (p, q) = (c0("c0", 3), c0("c0_rotation", 4));
            assert!((turn * Vec3::new(p[0], p[1], p[2])).abs_diff_eq(Vec3::new(0.0, 0.0, 1.0), 1e-5));
            assert!((turn * Quat::from_xyzw(q[0], q[1], q[2], q[3])).abs_diff_eq(Quat::IDENTITY, 1e-5));

            assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        }
    }

    /// A corner wedge turns a quarter and swaps its width and depth, its
    /// template's size when its file names none.
    #[test]
    fn a_corner_wedge_swaps_its_width_and_depth() {
        let records = space(
            "parent_pose",
            &[
                (
                    "Workspace/Corner/_instance.toml",
                    "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"Parts\\\\Corner_Wedge.GLB\"\n\n[transform]\nscale = [2.0, 3.0, 5.0]\n",
                ),
                ("Workspace/Bare/_instance.toml", "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"parts/corner_wedge.glb\"\n"),
            ],
        );
        let plan = plan(&records);
        assert_eq!(plan.corner_wedges, 2);
        let after = read_space_records(&records);
        let size = |key: &str| match prop(&after, key, "Size") {
            DmValue::Vector3(v) => [v.x, v.y, v.z],
            other => panic!("{other:?}"),
        };
        assert_eq!(size("Workspace/Corner/_instance.toml"), [5.0, 3.0, 2.0]);
        let before = as_drawn(&records);
        let template = match prop(&before, "Workspace/Bare/_instance.toml", "Size") {
            DmValue::Vector3(v) => [v.x, v.y, v.z],
            other => panic!("{other:?}"),
        };
        assert_ne!(template[0], template[2], "the template's footprint is not square");
        let bare = size("Workspace/Bare/_instance.toml");
        assert!((bare[0] - template[2]).abs() < 1e-6 && (bare[2] - template[0]).abs() < 1e-6, "{bare:?} from {template:?}");
    }

    /// A Roblox import, a custom mesh named like a wedge, a part whose DataMesh
    /// draws instead, and a WeldConstraint with no frames are left as written.
    #[test]
    fn imports_and_lookalikes_are_left_alone() {
        let imported = wedge("Imported", "roblox_color_srgb = [120, 120, 120]\n");
        let records = space(
            "parent_pose",
            &[
                ("Workspace/Imported/_instance.toml", &imported),
                ("Workspace/Imported/Child/_instance.toml", "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [1.0, 0.0, 0.0]\n"),
                ("Workspace/Roof/_instance.toml", "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"meshes/wedge_roof.glb\"\n"),
                (
                    "Workspace/Skinned/_instance.toml",
                    "[metadata]\nclass_name = \"Part\"\nuuid = \"bbbb\"\n\n[asset]\nmesh = \"parts/wedge.glb\"\n",
                ),
                ("Workspace/Skinned/Mesh/_instance.toml", "[metadata]\nclass_name = \"SpecialMesh\"\n\n[mesh]\nmesh_type = \"Sphere\"\n"),
                (
                    "Workspace/Native/_instance.toml",
                    "[metadata]\nclass_name = \"Part\"\nuuid = \"cccc\"\n\n[asset]\nmesh = \"parts/wedge.glb\"\n",
                ),
                (
                    "Workspace/Glue/_instance.toml",
                    "[metadata]\nclass_name = \"WeldConstraint\"\n\n[references]\nPart0 = \"CCCC\"\n",
                ),
            ],
        );
        let plan = plan(&records);
        assert_eq!(plan.roblox_left, 1);
        assert_eq!(plan.wedges, 1, "only Native");
        let keys: Vec<&str> = plan.rewrites.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["Workspace/Native/_instance.toml"]);
        assert_eq!(plan.joints, 0, "a WeldConstraint has no frames to turn");
    }

    /// A wedge on a wedge takes both turns in one rewrite, and its own child
    /// stays put.
    #[test]
    fn a_wedge_on_a_wedge_takes_both_turns() {
        let records = space(
            "parent_pose",
            &[
                ("Workspace/Roof/_instance.toml", &wedge("Roof", "")),
                (
                    "Workspace/Roof/Ridge/_instance.toml",
                    "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"parts/corner_wedge.glb\"\n\n[transform]\nposition = [0.0, 1.0, 0.5]\nscale = [1.0, 1.0, 2.0]\n",
                ),
                (
                    "Workspace/Roof/Ridge/Flag/_instance.toml",
                    "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [0.2, 0.7, -0.3]\nscale = [0.1, 0.5, 0.1]\n",
                ),
            ],
        );
        let plan = plan(&records);
        assert_eq!(plan.rewrites.iter().filter(|r| r.key == "Workspace/Roof/Ridge/_instance.toml").count(), 1);
        let (before, after) = (as_drawn(&records), read_space_records(&records));
        let ridge = "Workspace/Roof/Ridge/_instance.toml";
        let quarter = native_wedge_turn(PartType::CornerWedge).unwrap();
        assert!(same_cframe(&cframe(&after, ridge, "CFrame"), &turned(&cframe(&before, ridge, "CFrame"), quarter)));
        let flag = "Workspace/Roof/Ridge/Flag/_instance.toml";
        assert!(same_cframe(&cframe(&after, flag, "CFrame"), &cframe(&before, flag, "CFrame")));
    }

    /// Faces on a turned wedge keep their side: a decal naming none (the
    /// template's Front), a numbered texture face, a light's face, and an
    /// emitter's direction. A decal gets no pose it did not have.
    #[test]
    fn faces_on_a_wedge_keep_their_side() {
        let records = space(
            "parent_pose",
            &[
                ("Workspace/Roof/_instance.toml", &wedge("Roof", "")),
                ("Workspace/Roof/Sign/_instance.toml", "[metadata]\nclass_name = \"Decal\"\n\n[decal]\ntexture = \"sign.png\"\n"),
                ("Workspace/Roof/Tiles/_instance.toml", "[metadata]\nclass_name = \"Texture\"\n\n[texture]\nface = 3\n"),
                ("Workspace/Roof/Lamp/_instance.toml", "[metadata]\nclass_name = \"SpotLight\"\n\n[light]\nface = \"Enum.NormalId.Right\"\n"),
                ("Workspace/Roof/Steam/_instance.toml", "[metadata]\nclass_name = \"ParticleEmitter\"\n\n[particle]\nemission_direction = \"Front\"\n"),
            ],
        );
        let plan = plan(&records);
        assert_eq!(plan.faces, 4);
        let doc = |key: &str| -> toml::Value { plan.rewrites.iter().find(|r| r.key == key).unwrap().text.parse().unwrap() };
        let sign = doc("Workspace/Roof/Sign/_instance.toml");
        assert_eq!(sign["decal"]["face"].as_str(), Some("Back"));
        assert!(sign.get("transform").is_none(), "a decal sits on its face, not a pose");
        assert_eq!(doc("Workspace/Roof/Tiles/_instance.toml")["texture"]["face"].as_str(), Some("Right"), "Left (3) moves to Right");
        assert_eq!(doc("Workspace/Roof/Lamp/_instance.toml")["light"]["face"].as_str(), Some("Left"));
        assert_eq!(doc("Workspace/Roof/Steam/_instance.toml")["particle"]["emission_direction"].as_str(), Some("Back"));
    }

    /// The Player reads an older Space migrated, the number set in memory, so
    /// a second pass changes nothing; a Space at the current format, or with
    /// no wedge, is read as it is.
    #[test]
    fn a_reader_migrates_once() {
        let records = space("parent_pose", &[("Workspace/Roof/_instance.toml", &wedge("Roof", ""))]);
        let (migrated, problems) = migrated_records(&records).expect("a format-1 Space with a native wedge");
        assert!(problems.is_empty());
        assert_eq!(space_format::of_space(space_toml(&migrated).as_ref()), space_format::WEDGE_MESHES, "its own step's format");
        assert!(migrated_records(&migrated).is_none(), "applied twice, it turns nothing twice");
        let no_wedge = space("", &[("Workspace/Box/_instance.toml", "[metadata]\nclass_name = \"Part\"\n")]);
        assert!(migrated_records(&no_wedge).is_none());
        assert!(plan(&no_wedge).rewrites.is_empty());
        // No space.toml at all: format 1, and the number is added.
        let bare: Vec<_> = records.iter().filter(|(k, _)| k != "space.toml").cloned().collect();
        let (migrated, _) = migrated_records(&bare).unwrap();
        assert!(migrated_records(&migrated).is_none());
    }

    #[test]
    fn faces_read_in_every_spelling() {
        let v = |s: &str| toml::Value::String(s.to_string());
        assert_eq!(face_name(&v("front")), Some("Front"));
        assert_eq!(face_name(&v("Enum.NormalId.Top")), Some("Top"));
        assert_eq!(face_name(&v("4")), Some("Bottom"));
        assert_eq!(face_name(&toml::Value::Integer(0)), Some("Right"));
        assert_eq!(face_name(&v("Sideways")), None);
        assert_eq!(normalise_uuid(" AAAA-0001 "), "aaaa0001");
    }
}
