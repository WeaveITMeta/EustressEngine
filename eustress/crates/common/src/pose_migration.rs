//! Moving a Space from the legacy pose rule to `ParentPose`, keeping every
//! instance where it belongs.
//!
//! Under the legacy rule a child's `[transform]` composes onto its parent's
//! whole transform, a part's size included; under `ParentPose` onto the
//! parent's pose alone (`datamodel::record::TransformRule`). [`plan`] reads a
//! Space's records and gives, for each record whose placement would change,
//! the `[transform]` that keeps it in place under `ParentPose`:
//!
//! 1. A record the legacy rule does not place by its own file (an
//!    `Attachment`, a `Bone`, a part class on the general branch) keeps its
//!    file. `ParentPose` reads it as a pose relative to its parent, unscaled,
//!    which is what it means; the legacy rule never read it.
//! 2. A part the importer wrote that nothing has saved since holds a Roblox
//!    world pose, and goes where that pose says. The caller decides which
//!    records those are (`pristine`).
//! 3. Every other placed record stays where the legacy rule drew it, at the
//!    size it was drawn. A rotated child of a part that is not a cube was
//!    drawn sheared, which no pose and size reproduce: it takes the nearest
//!    pose and size, and its rewrite says so.
//!
//! The walk is the Player's reader's own ([`posed_records`]), run once under
//! each rule. Positions and sizes are written back in each file's own unit.

use std::collections::HashMap;

use bevy::math::{Affine3A, Isometry3d};
use bevy::prelude::*;

use crate::datamodel::record::{pose_relative_to, record_pose, TransformRule};
use crate::tree_read::posed_records;

/// Which rule placed a rewritten record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Rule 2: a Roblox world pose, from a file nothing has saved since the
    /// import.
    RobloxWorld,
    /// Rule 3: where the legacy rule drew it.
    AsDrawn,
}

/// One file's new `[transform]`, in the file's own unit, with the one it
/// replaces.
#[derive(Debug, Clone, PartialEq)]
pub struct Rewrite {
    /// The record's key: its path from the Space's folder.
    pub key: String,
    /// Which rule placed it.
    pub placement: Placement,
    /// The new `position`.
    pub position: [f32; 3],
    /// The new `rotation`, `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// The new `scale`, for a record whose file scale is part of its world (a
    /// part's size). `None` keeps the file's.
    pub scale: Option<[f32; 3]>,
    /// The file's `position`, `rotation` and `scale` before, as written.
    pub before: ([f32; 3], [f32; 4], [f32; 3]),
    /// The drawn transform was sheared: the new one is the nearest pose and
    /// size.
    pub sheared: bool,
}

/// What [`plan`] found.
#[derive(Debug, Default)]
pub struct MigrationPlan {
    /// The files to rewrite, parents before children.
    pub rewrites: Vec<Rewrite>,
    /// Records left as they are because they could not be read (neither rule
    /// can load them), one line each.
    pub problems: Vec<String>,
}

/// The rewrites that move `records` (a Space's files, keyed by their path
/// from the Space's folder, as `tree_read::space_records` reads them) from
/// the legacy rule to `ParentPose`. `pristine` says whether a record holds a
/// Roblox world pose (rule 2); it is asked only about records built from
/// their file under a parent. An `Err` when the Space cannot be planned: the
/// caller must then leave it on the legacy rule.
pub fn plan(records: &[(String, Vec<u8>)], pristine: &dyn Fn(&str, &toml::Value) -> bool) -> Result<MigrationPlan, String> {
    let mut out = MigrationPlan::default();
    let legacy = posed_records(records, TransformRule::Legacy);
    let parent_pose = posed_records(records, TransformRule::ParentPose);
    let same_walk = legacy.len() == parent_pose.len()
        && legacy.iter().zip(&parent_pose).all(|(a, b)| a.key == b.key && a.parent == b.parent);
    if !same_walk {
        return Err("the walk differs between the two rules, so no file can be planned".to_string());
    }
    let bytes: HashMap<&str, &[u8]> = records.iter().map(|(k, b)| (k.as_str(), b.as_slice())).collect();

    // Each record's frame under ParentPose: what its children compose onto.
    // Every one is an isometry, from the services' identity down.
    let mut frames: HashMap<&str, Isometry3d> = HashMap::new();
    for (old, new) in legacy.iter().zip(&parent_pose) {
        let parent_frame = old
            .parent
            .as_deref()
            .and_then(|p| frames.get(p))
            .copied()
            .unwrap_or(Isometry3d::IDENTITY);
        if !new.posed {
            // Placed by its parent under both rules: a pass-through.
            frames.insert(old.key.as_str(), parent_frame);
            continue;
        }
        let Some(doc) = bytes
            .get(old.key.as_str())
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|text| text.parse::<toml::Value>().ok())
        else {
            out.problems.push(format!("{}: not readable, left as it is", old.key));
            frames.insert(old.key.as_str(), parent_frame);
            continue;
        };
        if !old.posed {
            // Rule 1: kept as written, and placed by it under ParentPose.
            let pose = TransformRule::ParentPose.folder_pose(old.class, &doc);
            frames.insert(old.key.as_str(), parent_frame * Isometry3d::new(pose.translation, pose.rotation));
            continue;
        }
        let file = record_pose(&doc);
        let (target, placement, sheared) = if old.parent.is_some() && old.from_file && pristine(&old.key, &doc) {
            (file, Placement::RobloxWorld, false)
        } else {
            let (drawn, sheared) = decompose(&old.world);
            (drawn, Placement::AsDrawn, sheared)
        };
        frames.insert(old.key.as_str(), Isometry3d::new(target.translation, target.rotation));

        let local = pose_relative_to(parent_frame, &target);
        let moved = !same_point(local.translation, file.translation) || !same_rotation(local.rotation, file.rotation);
        let resized = old.from_file && !same_size(local.scale, file.scale);
        if !moved && !resized {
            continue;
        }
        let unit = file_unit(&doc);
        let before = written_transform(&doc);
        let (position, scale) = to_file_unit(local.translation, local.scale, unit.as_deref());
        let q = local.rotation.normalize();
        out.rewrites.push(Rewrite {
            key: old.key.clone(),
            placement,
            position,
            rotation: [q.x, q.y, q.z, q.w],
            scale: old.from_file.then_some(scale),
            before,
            sheared,
        });
    }
    Ok(out)
}

/// `text`, a record, with its `[transform]` set to `rewrite`'s numbers and
/// every other table kept.
pub fn rewrite_text(text: &str, rewrite: &Rewrite) -> Result<String, String> {
    let mut doc: toml::Value = text.parse().map_err(|e| format!("{}: {e}", rewrite.key))?;
    let root = doc.as_table_mut().ok_or_else(|| format!("{}: not a table", rewrite.key))?;
    let key = root
        .keys()
        .find(|k| k.eq_ignore_ascii_case("transform"))
        .cloned()
        .unwrap_or_else(|| "transform".to_string());
    let transform = root
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("{}: [transform] is not a table", rewrite.key))?;
    let mut set = |name: &str, values: &[f32]| {
        transform.retain(|k, _| !k.eq_ignore_ascii_case(name));
        transform.insert(name.to_string(), toml::Value::Array(values.iter().map(|v| float(*v)).collect()));
    };
    set("position", &rewrite.position);
    set("rotation", &rewrite.rotation);
    if let Some(scale) = rewrite.scale {
        set("scale", &scale);
    }
    toml::to_string_pretty(&doc).map_err(|e| format!("{}: {e}", rewrite.key))
}

/// A Space's `space.toml` text naming the `ParentPose` rule, everything else
/// in it kept. `None` (no file, or one that does not parse) gives a file
/// holding only the rule.
pub fn with_parent_pose_rule(space_toml: Option<&str>) -> String {
    let mut doc: toml::Value = space_toml
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| toml::Value::Table(toml::value::Table::new()));
    if let Some(root) = doc.as_table_mut() {
        let key = root
            .keys()
            .find(|k| k.eq_ignore_ascii_case("space"))
            .cloned()
            .unwrap_or_else(|| "space".to_string());
        let space = root.entry(key).or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(space) = space.as_table_mut() {
            space.retain(|k, _| !k.eq_ignore_ascii_case("transform_rule"));
            space.insert("transform_rule".to_string(), toml::Value::String("parent_pose".to_string()));
        }
    }
    toml::to_string_pretty(&doc).unwrap_or_else(|_| "[space]\ntransform_rule = \"parent_pose\"\n".to_string())
}

/// A drawn transform as a pose and a size, and whether it was sheared.
fn decompose(world: &GlobalTransform) -> (Transform, bool) {
    let (scale, rotation, translation) = world.to_scale_rotation_translation();
    let affine = world.affine();
    let nearest = Affine3A::from_scale_rotation_translation(scale, rotation, translation);
    let d = affine.matrix3 - nearest.matrix3;
    let error = d.x_axis.abs().max_element().max(d.y_axis.abs().max_element()).max(d.z_axis.abs().max_element());
    let sheared = error > 1e-4 * scale.abs().max_element().max(1.0);
    (Transform { translation, rotation, scale }, sheared)
}

fn same_point(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() <= 1e-5 * a.abs().max_element().max(1.0)
}

fn same_rotation(a: Quat, b: Quat) -> bool {
    let (a, b) = (a.normalize(), b.normalize());
    let d = (Vec4::from(a) - Vec4::from(b)).abs().max_element();
    let e = (Vec4::from(a) + Vec4::from(b)).abs().max_element();
    d.min(e) <= 1e-5
}

fn same_size(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() <= 1e-5 * a.abs().max_element().max(1.0)
}

/// The unit a file's numbers are in (`[metadata] unit`, any case).
fn file_unit(doc: &toml::Value) -> Option<String> {
    let meta = get_ci(doc, "metadata")?;
    get_ci(meta, "unit").and_then(|u| u.as_str()).map(str::to_owned)
}

/// A file's `[transform]` numbers as written, missing ones at their
/// defaults.
fn written_transform(doc: &toml::Value) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let t = get_ci(doc, "transform");
    let get = |name: &str| t.and_then(|t| get_ci(t, name)).and_then(|v| v.as_array());
    let nums = |a: Option<&Vec<toml::Value>>, n: usize| -> Option<Vec<f32>> {
        let a = a?;
        (a.len() == n).then(|| a.iter().map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).unwrap_or(0.0) as f32).collect())
    };
    let p = nums(get("position"), 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
    let r = nums(get("rotation"), 4).map_or([0.0, 0.0, 0.0, 1.0], |v| [v[0], v[1], v[2], v[3]]);
    let s = nums(get("scale"), 3).map_or([1.0; 3], |v| [v[0], v[1], v[2]]);
    (p, r, s)
}

/// Metres back into the file's unit: the inverse of the loader's
/// conversion (`space_read::authored_pose`), under the same gate.
fn to_file_unit(translation: Vec3, scale: Vec3, unit: Option<&str>) -> ([f32; 3], [f32; 3]) {
    #[cfg(feature = "units_v1")]
    if let Some(u) = unit.and_then(crate::units::Unit::from_symbol) {
        return (
            crate::units::engine_to_authored_vec3_f32(translation.to_array(), u),
            crate::units::engine_to_authored_vec3_f32(scale.to_array(), u),
        );
    }
    #[cfg(not(feature = "units_v1"))]
    let _ = unit;
    (translation.to_array(), scale.to_array())
}

fn get_ci<'v>(v: &'v toml::Value, key: &str) -> Option<&'v toml::Value> {
    v.as_table()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

/// An `f32` as the TOML float with the same shortest decimal form.
fn float(x: f32) -> toml::Value {
    toml::Value::Float(format!("{x}").parse::<f64>().unwrap_or(x as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::DmValue;
    use crate::tree_read::{read_space_records, SceneTree};

    fn part(unit: Option<&str>, position: [f32; 3], rotation: [f32; 4], size: [f32; 3], extra: &str) -> Vec<u8> {
        let unit = unit.map(|u| format!("unit = \"{u}\"\n")).unwrap_or_default();
        format!(
            "[metadata]\nclass_name = \"Part\"\n{unit}{extra}\n[transform]\nposition = {position:?}\nrotation = {rotation:?}\nscale = {size:?}\n"
        )
        .into_bytes()
    }

    fn model() -> Vec<u8> {
        b"[metadata]\nclass_name = \"Model\"\n".to_vec()
    }

    fn attachment(position: [f32; 3]) -> Vec<u8> {
        format!("[metadata]\nclass_name = \"Attachment\"\n\n[transform]\nposition = {position:?}\nrotation = [0.0, 0.0, 0.0, 1.0]\n").into_bytes()
    }

    fn space(records: &[(&str, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = vec![
            ("space.toml".to_string(), b"[space]\nname = \"Test\"\n".to_vec()),
            ("Workspace/_service.toml".to_string(), b"[service]\nclass_name = \"Workspace\"\n".to_vec()),
        ];
        out.extend(records.iter().map(|(k, v)| (k.to_string(), v.clone())));
        out
    }

    /// The records after `plan`'s rewrites and the rule key.
    fn migrated(records: &[(String, Vec<u8>)], plan: &MigrationPlan) -> Vec<(String, Vec<u8>)> {
        records
            .iter()
            .map(|(k, v)| {
                let text = std::str::from_utf8(v).unwrap();
                let new = if k == "space.toml" {
                    with_parent_pose_rule(Some(text))
                } else if let Some(r) = plan.rewrites.iter().find(|r| &r.key == k) {
                    rewrite_text(text, r).unwrap()
                } else {
                    text.to_string()
                };
                (k.clone(), new.into_bytes())
            })
            .collect()
    }

    /// A part's world CFrame, as the Player's reader places it: position,
    /// then the rotation's columns.
    fn placed(tree: &SceneTree, key: &str) -> [f64; 12] {
        let id = tree.keys.iter().find(|(k, _)| k == key).map(|(_, id)| *id).expect(key);
        let Some(DmValue::CFrame(cf)) = tree.dm.get_prop(id, "CFrame") else { panic!("{key} has no CFrame") };
        let m = cf.rotation_matrix();
        [
            cf.position.x, cf.position.y, cf.position.z,
            m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
        ]
    }

    /// A part's `Size`: its file's own scale. The legacy rule drew a nested
    /// part at that times its parent's size.
    fn size(tree: &SceneTree, key: &str) -> [f64; 3] {
        let id = tree.keys.iter().find(|(k, _)| k == key).map(|(_, id)| *id).expect(key);
        let Some(DmValue::Vector3(s)) = tree.dm.get_prop(id, "Size") else { panic!("{key} has no Size") };
        [s.x, s.y, s.z]
    }

    fn assert_close<const N: usize>(a: [f64; N], b: [f64; N], what: &str) {
        for i in 0..N {
            assert!((a[i] - b[i]).abs() < 1e-4, "{what}: component {i} {} != {}\n{a:?}\n{b:?}", a[i], b[i]);
        }
    }

    const QUARTER_Y: [f32; 4] = [0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2];

    /// A child of a sized, turned part lands where the legacy rule drew it,
    /// at the size it was drawn, read by the Player's reader before and
    /// after.
    #[test]
    fn a_child_of_a_sized_part_stays_where_it_was_drawn() {
        let records = space(&[
            ("Workspace/Base/_instance.toml", part(None, [3.0, 1.0, -2.0], QUARTER_Y, [2.0, 1.0, 4.0], "")),
            ("Workspace/Base/Flag/_instance.toml", part(None, [0.25, 1.5, 0.1], [0.0, 0.0, 0.0, 1.0], [0.1, 2.0, 0.1], "")),
        ]);
        let plan = plan(&records, &|_, _| false).unwrap();
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        assert_eq!(plan.rewrites.len(), 1, "{:?}", plan.rewrites);
        assert_eq!(plan.rewrites[0].placement, Placement::AsDrawn);
        let before = read_space_records(&records);
        let after = read_space_records(&migrated(&records, &plan));
        for key in ["Workspace/Base/_instance.toml", "Workspace/Base/Flag/_instance.toml"] {
            assert_close(placed(&before, key), placed(&after, key), key);
        }
        // Drawn at the base's size times its own, (2, 1, 4) times
        // (0.1, 2, 0.1): that is its Size now.
        let flag = "Workspace/Base/Flag/_instance.toml";
        let (was, now) = (size(&before, flag), size(&after, flag));
        assert_close([now[0] / was[0], now[1] / was[1], now[2] / was[2]], [2.0, 1.0, 4.0], "drawn size");
    }

    /// A Model with no pose passes its part's size down under the legacy
    /// rule and only its pose under ParentPose; the part below it is
    /// rewritten, the Model is not.
    #[test]
    fn an_identity_model_between_parts() {
        let records = space(&[
            ("Workspace/Base/_instance.toml", part(None, [0.0, 2.0, 0.0], QUARTER_Y, [4.0, 1.0, 2.0], "")),
            ("Workspace/Base/Holder/_instance.toml", model()),
            ("Workspace/Base/Holder/Flag/_instance.toml", part(None, [0.5, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.1, 0.5, 0.1], "")),
        ]);
        let plan = plan(&records, &|_, _| false).unwrap();
        let keys: Vec<&str> = plan.rewrites.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec!["Workspace/Base/Holder/Flag/_instance.toml"]);
        let before = read_space_records(&records);
        let after = read_space_records(&migrated(&records, &plan));
        let key = "Workspace/Base/Holder/Flag/_instance.toml";
        assert_close(placed(&before, key), placed(&after, key), key);
        let s = plan.rewrites[0].scale.expect("a part's size");
        assert_close([s[0] as f64, s[1] as f64, s[2] as f64], [0.4, 0.5, 0.2], "drawn through the Model at the base's size");
    }

    /// A file in feet gets its new numbers in feet: read back through the
    /// loader's conversion, it lands where it was drawn.
    #[test]
    fn a_file_in_feet_stays_in_feet() {
        let records = space(&[
            ("Workspace/Base/_instance.toml", part(Some("ft"), [10.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0], [8.0, 2.0, 8.0], "")),
            ("Workspace/Base/Post/_instance.toml", part(Some("ft"), [0.25, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.125, 1.0, 0.125], "")),
        ]);
        let plan = plan(&records, &|_, _| false).unwrap();
        assert_eq!(plan.rewrites.len(), 1);
        let before = read_space_records(&records);
        let after_records = migrated(&records, &plan);
        let after = read_space_records(&after_records);
        let key = "Workspace/Base/Post/_instance.toml";
        assert_close(placed(&before, key), placed(&after, key), key);
        // Drawn at the base's size times its own, each read the way the
        // loader reads it (with `units_v1` feet become metres first, so
        // 8 ft x 0.125 ft was drawn 2.4384 m x 0.0381 m = 0.0929 m, and
        // 8 x 0.125 = 1 without). That is its Size now, read back from the
        // file in feet: numbers written in metres would read 0.3048 times
        // too small.
        let base = "Workspace/Base/_instance.toml";
        let (b, p) = (size(&before, base), size(&before, key));
        assert_close(size(&after, key), [b[0] * p[0], b[1] * p[1], b[2] * p[2]], "drawn size");
        let doc: toml::Value = std::str::from_utf8(&after_records.iter().find(|(k, _)| k == key).unwrap().1)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(doc["metadata"]["unit"].as_str(), Some("ft"));
    }

    /// A part the importer wrote and nothing saved since holds a Roblox world
    /// pose: after the move it is there, not where the legacy rule drew it.
    #[test]
    fn an_untouched_import_goes_where_roblox_had_it() {
        let provenance = "roblox_brick_color = 194\n";
        let records = space(&[
            ("Workspace/Pad/_instance.toml", part(Some("ft"), [10.0, 0.0, 0.0], QUARTER_Y, [4.0, 1.0, 8.0], provenance)),
            ("Workspace/Pad/Ignore/_instance.toml", part(Some("ft"), [10.0, 0.0, 5.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0], provenance)),
        ]);
        let plan = plan(&records, &|key, _| key.ends_with("Ignore/_instance.toml")).unwrap();
        assert_eq!(plan.rewrites.len(), 1);
        assert_eq!(plan.rewrites[0].placement, Placement::RobloxWorld);
        // Under ParentPose, 5 ft along world +Z from a pad turned +90 degrees
        // about Y is 5 ft along the pad's -X, at its own size.
        let r = &plan.rewrites[0];
        assert!((r.position[0] + 5.0).abs() < 1e-4 && r.position[1].abs() < 1e-4 && r.position[2].abs() < 1e-4, "{:?}", r.position);
        let s = r.scale.expect("a part's size");
        assert_close([s[0] as f64, s[1] as f64, s[2] as f64], [1.0, 1.0, 1.0], "its own size, in feet");
    }

    /// An Attachment keeps its file; the part holding it is not moved.
    #[test]
    fn an_attachment_keeps_its_file() {
        let records = space(&[
            ("Workspace/Wheel/_instance.toml", part(None, [0.0, 1.0, 0.0], QUARTER_Y, [2.0, 2.0, 0.5], "")),
            ("Workspace/Wheel/Axle/_instance.toml", attachment([0.0, 0.0, 0.25])),
        ]);
        let plan = plan(&records, &|_, _| false).unwrap();
        assert!(plan.rewrites.is_empty(), "{:?}", plan.rewrites);
    }

    /// A turned child of a part that is not a cube was drawn sheared; its
    /// rewrite says so.
    #[test]
    fn a_sheared_child_is_marked() {
        let tilt = [0.0, 0.0, (std::f32::consts::FRAC_PI_8).sin(), (std::f32::consts::FRAC_PI_8).cos()];
        let records = space(&[
            ("Workspace/Beam/_instance.toml", part(None, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [8.0, 1.0, 1.0], "")),
            ("Workspace/Beam/Brace/_instance.toml", part(None, [0.1, 0.0, 0.0], tilt, [0.5, 0.5, 0.5], "")),
        ]);
        let plan = plan(&records, &|_, _| false).unwrap();
        assert_eq!(plan.rewrites.len(), 1);
        assert!(plan.rewrites[0].sheared);
    }

    /// Records directly under a service, and a Space already flat, need
    /// nothing.
    #[test]
    fn records_under_a_service_are_never_rewritten() {
        let records = space(&[
            ("Workspace/A/_instance.toml", part(Some("ft"), [1.0, 2.0, 3.0], QUARTER_Y, [4.0, 5.0, 6.0], "")),
            ("Workspace/Group/_instance.toml", model()),
            ("Workspace/Group/B/_instance.toml", part(None, [7.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0], "")),
        ]);
        let plan = plan(&records, &|_, _| true).unwrap();
        assert!(plan.rewrites.is_empty(), "{:?}", plan.rewrites);
    }

    #[test]
    fn the_rule_key_keeps_the_rest_of_space_toml() {
        let text = with_parent_pose_rule(Some("[space]\nname = \"Garage\"\nTransform_Rule = \"legacy\"\n\n[metadata]\nauthor = \"x\"\n"));
        let doc: toml::Value = text.parse().unwrap();
        assert_eq!(doc["space"]["transform_rule"].as_str(), Some("parent_pose"));
        assert!(doc["space"].get("Transform_Rule").is_none());
        assert_eq!(doc["space"]["name"].as_str(), Some("Garage"));
        assert_eq!(doc["metadata"]["author"].as_str(), Some("x"));
        assert_eq!(
            TransformRule::of_space(Some(&doc)),
            TransformRule::ParentPose
        );
    }
}
