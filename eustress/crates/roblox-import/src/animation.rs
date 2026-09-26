//! Roblox animation data as Eustress clip records.
//!
//! A `KeyframeSequence` is one record, in the shape
//! `docs/design/ANIMATION_SYSTEM.md` ("The clip record") gives it and
//! `eustress_common::animation::clip` reads: `[keyframe_sequence]` holds its
//! `Loop`, `Priority` and `AuthoredHipHeight`, and each `Keyframe` is one
//! `[[keyframes]]` entry holding its `Pose`s (flattened, each naming the pose
//! above it), its `NumberPose`s and its `KeyframeMarker`s. A pose drives the
//! joint between its parent pose's part and its own, so a top-level pose
//! drives nothing and is left out; its children still name it as their
//! parent. A sequence in the
//! place is its folder's `_instance.toml`. A clip fetched by Roblox id is
//! `assets/animations/rbx-<id>.anim.toml`, and the Space's id map
//! (`assets/roblox_ids.toml`) names that file for the id, which is how an
//! `AnimationId` such as `rbxassetid://507770239` finds its clip.
//!
//! Values are Roblox's own. A pose's `cframe` is its `CFrame` in Roblox's
//! order (position, then the rotation matrix by rows) and in studs, which the
//! record's `[metadata] unit = "ft"` declares. Enums are written by name, as
//! the reflection database lists them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use eustress_common::animation::clip::{MAX_KEYFRAMES, MAX_POSES_PER_KEYFRAME, MAX_RECORD_BYTES};
use eustress_common::animation::content::{roblox_asset_id, ROBLOX_ID_MAP};
use rbx_dom_weak::types::{CFrame, Variant};
use rbx_dom_weak::{Instance, WeakDom};

/// Where the Space keeps clips fetched by Roblox id, relative to its root.
pub const CLIP_DIR: &str = "assets/animations";

/// A `KeyframeSequence` as the tables of its record.
#[derive(Debug, Default)]
pub struct SequenceRecord {
    /// `[keyframe_sequence]`.
    pub sequence: toml::value::Table,
    /// `keyframes`: one table per `Keyframe`, in time order.
    pub keyframes: toml::value::Array,
    /// Poses written, over every keyframe.
    pub poses: usize,
    /// What the record leaves out or cannot play, one sentence each.
    pub notes: Vec<String>,
}

/// The record for `sequence`, a `KeyframeSequence` in `dom`.
pub fn sequence_record(dom: &WeakDom, sequence: &Instance) -> SequenceRecord {
    let mut record = SequenceRecord::default();

    let looped = match prop(sequence, "Loop") {
        Some(Variant::Bool(b)) => *b,
        _ => true,
    };
    record.sequence.insert("loop".to_string(), toml::Value::Boolean(looped));
    let priority = enum_value(prop(sequence, "Priority")).unwrap_or(2);
    record.sequence.insert("priority".to_string(), enum_name(priority, priority_name));
    if let Some(Variant::Float32(height)) = prop(sequence, "AuthoredHipHeight") {
        record.sequence.insert("authored_hip_height".to_string(), float(*height));
    }

    let mut frames: Vec<(f32, toml::value::Table)> = Vec::new();
    for keyframe in children(dom, sequence) {
        if keyframe.class.as_str() != "Keyframe" {
            record.notes.push(format!(
                "{} '{}' is not a Keyframe, so the clip leaves it out",
                keyframe.class, keyframe.name
            ));
            continue;
        }
        let time = match prop(keyframe, "Time") {
            Some(Variant::Float32(t)) => *t,
            Some(Variant::Float64(t)) => *t as f32,
            _ => 0.0,
        };
        if !time.is_finite() || time < 0.0 {
            record.notes.push(format!("keyframe '{}' has the time {time}, so the clip leaves it out", keyframe.name));
            continue;
        }
        let mut frame = toml::value::Table::new();
        frame.insert("time".to_string(), float(time));
        if !keyframe.name.is_empty() && keyframe.name != "Keyframe" {
            frame.insert("name".to_string(), toml::Value::String(keyframe.name.clone()));
        }
        let mut contents = FrameContents::default();
        visit(dom, keyframe, None, time, &mut contents, &mut record.notes);
        if contents.poses.len() > MAX_POSES_PER_KEYFRAME {
            record.notes.push(format!(
                "the keyframe at {time} s has {} poses; the clip reader takes at most {MAX_POSES_PER_KEYFRAME}, \
                 so the clip will not play",
                contents.poses.len()
            ));
        }
        record.poses += contents.poses.len();
        if !contents.poses.is_empty() {
            frame.insert("poses".to_string(), toml::Value::Table(contents.poses));
        }
        if !contents.numbers.is_empty() {
            frame.insert("numbers".to_string(), toml::Value::Table(contents.numbers));
        }
        if !contents.markers.is_empty() {
            frame.insert("markers".to_string(), toml::Value::Array(contents.markers));
        }
        frames.push((time, frame));
    }
    if frames.len() > MAX_KEYFRAMES {
        record.notes.push(format!(
            "{} keyframes; the clip reader takes at most {MAX_KEYFRAMES}, so the clip will not play",
            frames.len()
        ));
    }
    // Stable: keyframes at the same time keep their order in the place.
    frames.sort_by(|a, b| a.0.total_cmp(&b.0));
    record.keyframes = frames.into_iter().map(|(_, frame)| toml::Value::Table(frame)).collect();
    record
}

#[derive(Default)]
struct FrameContents {
    poses: toml::value::Table,
    numbers: toml::value::Table,
    markers: toml::value::Array,
}

/// Collect the poses, numbers and markers under `node` (a `Keyframe`, or a
/// `Pose` named `parent`).
fn visit(
    dom: &WeakDom,
    node: &Instance,
    parent: Option<&str>,
    time: f32,
    out: &mut FrameContents,
    notes: &mut Vec<String>,
) {
    for child in children(dom, node) {
        match child.class.as_str() {
            "Pose" => {
                // A top-level pose drives no joint (it has no parent pose's
                // part to turn against), and written without `parent` the
                // clip reader would bind it by name alone: it is left out.
                if parent.is_some() {
                    let pose = pose_table(child, parent);
                    let keyed = |t: &toml::Value| t.get("weight").and_then(|w| w.as_float()).map_or(true, |w| w != 0.0);
                    match out.poses.get(&child.name) {
                        None => {
                            out.poses.insert(child.name.clone(), toml::Value::Table(pose));
                        }
                        Some(existing) => {
                            // A record holds one pose per joint name in a
                            // keyframe: keep the one that keys the joint.
                            let new = toml::Value::Table(pose);
                            let replace = keyed(&new) && !keyed(existing);
                            let kept = if replace { &new } else { existing };
                            let under = kept.get("parent").and_then(|p| p.as_str()).unwrap_or("the keyframe");
                            notes.push(format!(
                                "the keyframe at {time} s has two poses named '{}'; the clip keeps the one under '{under}'",
                                child.name
                            ));
                            if replace {
                                out.poses.insert(child.name.clone(), new);
                            }
                        }
                    }
                }
                visit(dom, child, Some(&child.name), time, out, notes);
            }
            "NumberPose" => {
                let value = match prop(child, "Value") {
                    Some(Variant::Float64(v)) => *v,
                    Some(Variant::Float32(v)) => *v as f64,
                    _ => 0.0,
                };
                let style = enum_value(prop(child, "EasingStyle")).unwrap_or(0);
                let direction = enum_value(prop(child, "EasingDirection")).unwrap_or(0);
                let entry = if style == 0 && direction == 0 {
                    toml::Value::Float(value)
                } else {
                    let mut t = toml::value::Table::new();
                    t.insert("value".to_string(), toml::Value::Float(value));
                    if style != 0 {
                        t.insert("easing".to_string(), enum_name(style, easing_style_name));
                    }
                    if direction != 0 {
                        t.insert("direction".to_string(), enum_name(direction, easing_direction_name));
                    }
                    toml::Value::Table(t)
                };
                if out.numbers.contains_key(&child.name) {
                    notes.push(format!(
                        "the keyframe at {time} s has two number poses named '{}'; the clip keeps the first",
                        child.name
                    ));
                } else {
                    out.numbers.insert(child.name.clone(), entry);
                }
                visit(dom, child, parent, time, out, notes);
            }
            "KeyframeMarker" => {
                let value = match prop(child, "Value") {
                    Some(Variant::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let mut marker = toml::value::Table::new();
                marker.insert("name".to_string(), toml::Value::String(child.name.clone()));
                marker.insert("value".to_string(), toml::Value::String(value));
                out.markers.push(toml::Value::Table(marker));
            }
            other => notes.push(format!(
                "{other} '{}' in the keyframe at {time} s is not part of a clip, so the clip leaves it out",
                child.name
            )),
        }
    }
}

/// One `Pose` as its record table: only what differs from Roblox's defaults
/// (an identity `CFrame`, `Linear`, `In`, a weight of 1).
fn pose_table(pose: &Instance, parent: Option<&str>) -> toml::value::Table {
    let mut t = toml::value::Table::new();
    if let Some(parent) = parent {
        t.insert("parent".to_string(), toml::Value::String(parent.to_string()));
    }
    if let Some(Variant::CFrame(cf)) = prop(pose, "CFrame") {
        let numbers = cframe_numbers(cf);
        if numbers != IDENTITY_CFRAME {
            t.insert("cframe".to_string(), toml::Value::Array(numbers.iter().map(|v| float(*v)).collect()));
        }
    }
    let style = enum_value(prop(pose, "EasingStyle")).unwrap_or(0);
    if style != 0 {
        t.insert("easing".to_string(), enum_name(style, easing_style_name));
    }
    let direction = enum_value(prop(pose, "EasingDirection")).unwrap_or(0);
    if direction != 0 {
        t.insert("direction".to_string(), enum_name(direction, easing_direction_name));
    }
    let weight = match prop(pose, "Weight") {
        Some(Variant::Float32(w)) => *w,
        Some(Variant::Float64(w)) => *w as f32,
        _ => 1.0,
    };
    if weight != 1.0 {
        t.insert("weight".to_string(), float(weight));
    }
    t
}

const IDENTITY_CFRAME: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// A `CFrame` in Roblox's order: position, then the rotation matrix by rows
/// (`rbx_types::Matrix3`'s `x`, `y` and `z` are its rows).
fn cframe_numbers(cf: &CFrame) -> [f32; 12] {
    let (p, m) = (cf.position, cf.orientation);
    [p.x, p.y, p.z, m.x.x, m.x.y, m.x.z, m.y.x, m.y.y, m.y.z, m.z.x, m.z.y, m.z.z]
}

/// `Enum.AnimationPriority`, as the reflection database lists it.
fn priority_name(value: u32) -> Option<&'static str> {
    Some(match value {
        0 => "Idle",
        1 => "Movement",
        2 => "Action",
        3 => "Action2",
        4 => "Action3",
        5 => "Action4",
        1000 => "Core",
        _ => return None,
    })
}

/// `Enum.PoseEasingStyle`, as the reflection database lists it.
fn easing_style_name(value: u32) -> Option<&'static str> {
    Some(match value {
        0 => "Linear",
        1 => "Constant",
        2 => "Elastic",
        3 => "Cubic",
        4 => "Bounce",
        5 => "CubicV2",
        _ => return None,
    })
}

/// `Enum.PoseEasingDirection`, as the reflection database lists it.
fn easing_direction_name(value: u32) -> Option<&'static str> {
    Some(match value {
        0 => "In",
        1 => "Out",
        2 => "InOut",
        _ => return None,
    })
}

/// An enum by name, or its number when the name is not known (the clip
/// reader takes both).
fn enum_name(value: u32, name: fn(u32) -> Option<&'static str>) -> toml::Value {
    match name(value) {
        Some(n) => toml::Value::String(n.to_string()),
        None => toml::Value::Integer(value as i64),
    }
}

fn enum_value(variant: Option<&Variant>) -> Option<u32> {
    match variant? {
        Variant::Enum(e) => Some(e.to_u32()),
        Variant::EnumItem(item) => Some(item.value),
        Variant::Int32(i) => u32::try_from(*i).ok(),
        _ => None,
    }
}

/// An `f32` as the TOML float with the same shortest decimal form, so
/// `0.70710677` is written as that and not as `0.7071067690849304`.
fn float(x: f32) -> toml::Value {
    toml::Value::Float(format!("{x}").parse::<f64>().unwrap_or(x as f64))
}

fn prop<'a>(inst: &'a Instance, name: &str) -> Option<&'a Variant> {
    inst.properties.get(&rbx_dom_weak::ustr(name))
}

fn children<'a>(dom: &'a WeakDom, inst: &Instance) -> Vec<&'a Instance> {
    inst.children().iter().filter_map(|r| dom.get_by_ref(*r)).collect()
}

// ============================================================================
// Clips fetched by id
// ============================================================================

/// A clip file made from a fetched Roblox animation asset.
#[derive(Debug)]
pub struct ClipFile {
    /// The file's text: a whole record.
    pub text: String,
    /// Poses written, over every keyframe.
    pub poses: usize,
    /// What the clip leaves out or cannot play, one sentence each.
    pub notes: Vec<String>,
}

/// The clip file for the Roblox asset `id`, whose fetched bytes are
/// `bytes`. `Ok(None)` when the asset is not a model holding a
/// `KeyframeSequence` (an image or a sound a script also names, say).
pub fn clip_from_model(bytes: &[u8], id: u64) -> Result<Option<ClipFile>, String> {
    let dom = if bytes.starts_with(b"<roblox!") {
        rbx_binary::from_reader(bytes).map_err(|e| format!("animation model: {e}"))?
    } else if bytes.starts_with(b"<roblox") {
        rbx_xml::from_reader_default(bytes).map_err(|e| format!("animation model: {e}"))?
    } else {
        return Ok(None);
    };
    let Some(sequence) = dom.descendants().find(|i| i.class.as_str() == "KeyframeSequence") else {
        return Ok(None);
    };
    let record = sequence_record(&dom, sequence);
    let mut notes = record.notes.clone();
    let text = record_text(&sequence.name, &record, Some(&format!("rbxassetid://{id}")))?;
    if text.len() > MAX_RECORD_BYTES {
        notes.push(format!(
            "the clip is {} bytes; the clip reader takes at most {MAX_RECORD_BYTES}, so it will not play",
            text.len()
        ));
    }
    Ok(Some(ClipFile { text, poses: record.poses, notes }))
}

/// A whole record: `[metadata]` (class, name, and the stud unit its
/// positions are in), `[keyframe_sequence]` and the keyframes.
pub fn record_text(name: &str, record: &SequenceRecord, source: Option<&str>) -> Result<String, String> {
    let mut metadata = toml::value::Table::new();
    metadata.insert("class_name".to_string(), toml::Value::String("KeyframeSequence".to_string()));
    metadata.insert("name".to_string(), toml::Value::String(name.to_string()));
    metadata.insert("unit".to_string(), toml::Value::String(eustress_common::units::Unit::Stud.symbol().to_string()));
    let mut sequence = record.sequence.clone();
    if let Some(source) = source {
        sequence.insert("source".to_string(), toml::Value::String(source.to_string()));
    }
    let mut root = toml::value::Table::new();
    root.insert("metadata".to_string(), toml::Value::Table(metadata));
    root.insert("keyframe_sequence".to_string(), toml::Value::Table(sequence));
    root.insert("keyframes".to_string(), toml::Value::Array(record.keyframes.clone()));
    toml::to_string_pretty(&toml::Value::Table(root)).map_err(|e| format!("clip record: {e}"))
}

/// The Space-relative clip file for Roblox id `id`.
pub fn clip_path(id: u64) -> String {
    format!("{CLIP_DIR}/rbx-{id}.anim.toml")
}

/// Name each clip file in the Space's id map (`assets/roblox_ids.toml`,
/// `[assets] "<id>" = "<file>"`), keeping the entries already there.
pub fn merge_id_map(space_root: &Path, clips: &BTreeMap<u64, String>) -> Result<(), String> {
    let path = space_root.join(ROBLOX_ID_MAP);
    let mut doc: toml::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| toml::Value::Table(toml::value::Table::new()));
    let root = doc.as_table_mut().ok_or("the id map is not a table")?;
    let assets = root
        .entry("assets".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let assets = assets.as_table_mut().ok_or("the id map's [assets] is not a table")?;
    for (id, file) in clips {
        assets.insert(id.to_string(), toml::Value::String(file.clone()));
    }
    let text = toml::to_string_pretty(&doc).map_err(|e| format!("id map: {e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

// ============================================================================
// Ids a script plays
// ============================================================================

/// The Roblox asset ids in a script's string literals, when the script works
/// with animations: Roblox's `Animate` script keeps its default clips that
/// way, and scripts make Animations at run time from literal ids. Each is a
/// candidate the clip fetch sorts out: an id that turns out to be an image
/// or a sound is left alone.
pub fn script_animation_ids(source: &str) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    if !source.contains("Animation") {
        return ids;
    }
    for literal in string_literals(source) {
        if let Some(id) = roblox_asset_id(literal) {
            ids.insert(id);
        }
    }
    ids
}

/// The contents of every `"..."` and `'...'` literal in Luau source, escapes
/// left as written.
fn string_literals(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != quote && bytes[j] != b'\n' {
            j += if bytes[j] == b'\\' { 2 } else { 1 };
        }
        let end = j.min(bytes.len());
        if let Some(literal) = source.get(start..end) {
            out.push(literal);
        }
        i = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::animation::clip::parse_sequence;
    use eustress_common::datamodel::animation::Priority;
    use rbx_dom_weak::types::{Enum, Matrix3, Vector3};
    use rbx_dom_weak::InstanceBuilder;

    /// +90 degrees about Y, as `rbx_types` stores it (its rows).
    fn turned() -> Matrix3 {
        Matrix3::new(Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0), Vector3::new(-1.0, 0.0, 0.0))
    }

    fn wave() -> WeakDom {
        let pose = |name: &str| InstanceBuilder::new("Pose").with_name(name);
        WeakDom::new(
            InstanceBuilder::new("KeyframeSequence")
                .with_name("Wave")
                .with_property("Loop", Variant::Bool(false))
                .with_property("Priority", Variant::Enum(Enum::from_u32(3)))
                .with_property("AuthoredHipHeight", Variant::Float32(2.5))
                .with_child(
                    InstanceBuilder::new("Keyframe")
                        .with_property("Time", Variant::Float32(0.3))
                        .with_name("Raised")
                        .with_child(
                            pose("HumanoidRootPart").with_property("Weight", Variant::Float32(0.0)).with_child(
                                pose("LowerTorso")
                                    .with_property(
                                        "CFrame",
                                        Variant::CFrame(CFrame::new(Vector3::new(0.0, 1.0, 0.0), turned())),
                                    )
                                    .with_property("EasingStyle", Variant::Enum(Enum::from_u32(5)))
                                    .with_property("EasingDirection", Variant::Enum(Enum::from_u32(1)))
                                    .with_child(pose("UpperTorso").with_property("Weight", Variant::Float32(0.0))),
                            ),
                        )
                        .with_child(InstanceBuilder::new("KeyframeMarker").with_name("Wave").with_property(
                            "Value",
                            Variant::String("start".to_string()),
                        ))
                        .with_child(
                            InstanceBuilder::new("NumberPose").with_name("JawDrop").with_property("Value", Variant::Float64(0.5)),
                        ),
                )
                .with_child(
                    InstanceBuilder::new("Keyframe")
                        .with_property("Time", Variant::Float32(0.0))
                        .with_child(pose("HumanoidRootPart").with_property("Weight", Variant::Float32(0.0))),
                ),
        )
    }

    /// The record the importer writes is the record the clip reader reads:
    /// values, units, rotation direction, easing, markers and number poses.
    #[test]
    fn a_sequence_becomes_the_record_the_clip_reader_reads() {
        let dom = wave();
        let sequence = dom.get_by_ref(dom.root_ref()).unwrap();
        let record = sequence_record(&dom, sequence);
        assert!(record.notes.is_empty(), "{:?}", record.notes);
        assert_eq!(record.poses, 2, "the top-level HumanoidRootPart poses drive no joint");
        let text = record_text("Wave", &record, None).unwrap();
        let (parsed, problems) = parse_sequence(&text, "fallback").unwrap();
        assert!(problems.is_empty(), "{problems:?}\n{text}");
        assert_eq!(parsed.name, "Wave");
        assert!(!parsed.looped);
        assert_eq!(parsed.priority, Priority::Action2);
        // 2.5 studs, read in metres through the record's unit.
        let stud = eustress_common::units::Unit::Stud.to_meters();
        assert!((parsed.authored_hip_height as f64 - 2.5 * stud).abs() < 1e-6, "{}", parsed.authored_hip_height);
        let times: Vec<f32> = parsed.keyframes.iter().map(|k| k.time).collect();
        assert_eq!(times, vec![0.0, 0.3], "keyframes in time order");
        let raised = &parsed.keyframes[1];
        assert_eq!(raised.name, "Raised");
        let lower = raised.poses.iter().find(|p| p.joint == "LowerTorso").unwrap();
        assert_eq!(lower.parent.as_deref(), Some("HumanoidRootPart"));
        // One stud up, in metres; +90 degrees about Y is qy = +0.7071.
        let p = lower.position;
        let stud = eustress_common::units::Unit::Stud.to_meters() as f32;
        assert!(p.x.abs() < 1e-5 && (p.y - stud).abs() < 1e-5 && p.z.abs() < 1e-5, "{p:?}");
        let q = lower.rotation;
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((q.y - h).abs() < 1e-5 && (q.w - h).abs() < 1e-5 && q.x.abs() < 1e-5 && q.z.abs() < 1e-5, "{q:?}");
        assert_eq!(lower.style, eustress_common::animation::easing::PoseEasingStyle::CubicV2);
        assert_eq!(lower.direction, eustress_common::animation::easing::PoseEasingDirection::Out);
        assert!(raised.poses.iter().all(|p| p.joint != "HumanoidRootPart"), "a top-level pose is left out");
        assert!(parsed.keyframes[0].poses.is_empty(), "a keyframe of only a top-level pose keys nothing");
        let upper = raised.poses.iter().find(|p| p.joint == "UpperTorso").unwrap();
        assert_eq!(upper.parent.as_deref(), Some("LowerTorso"));
        assert_eq!(upper.weight, 0.0, "a structural pose keys nothing");
        assert_eq!(raised.markers, vec![("Wave".to_string(), "start".to_string())]);
        assert_eq!(raised.numbers.len(), 1);
        assert_eq!(raised.numbers[0].name, "JawDrop");
        assert_eq!(raised.numbers[0].value, 0.5);
    }

    /// A keyframe holds one pose per joint name: the one that keys wins.
    #[test]
    fn two_poses_with_one_name_keep_the_keyed_one() {
        let pose = |name: &str| InstanceBuilder::new("Pose").with_name(name);
        let dom = WeakDom::new(
            InstanceBuilder::new("KeyframeSequence").with_child(
                InstanceBuilder::new("Keyframe")
                    .with_property("Time", Variant::Float32(0.0))
                    .with_child(
                        pose("Torso")
                            .with_child(
                                pose("Left Arm").with_child(pose("Handle").with_property("Weight", Variant::Float32(0.0))),
                            )
                            .with_child(pose("Right Arm").with_child(pose("Handle"))),
                    ),
            ),
        );
        let record = sequence_record(&dom, dom.get_by_ref(dom.root_ref()).unwrap());
        let poses = record.keyframes[0].get("poses").unwrap();
        assert_eq!(poses["Handle"]["parent"].as_str(), Some("Right Arm"));
        assert!(poses["Handle"].get("weight").is_none());
        assert_eq!(record.notes.len(), 1, "{:?}", record.notes);
    }

    /// A fetched model becomes a whole clip file that names its source.
    #[test]
    fn a_fetched_model_becomes_a_clip_file() {
        let dom = wave();
        let mut bytes = Vec::new();
        rbx_binary::to_writer(&mut bytes, &dom, &[dom.root_ref()]).unwrap();
        let clip = clip_from_model(&bytes, 507770239).unwrap().expect("a clip");
        let doc: toml::Value = clip.text.parse().unwrap();
        assert_eq!(doc["keyframe_sequence"]["source"].as_str(), Some("rbxassetid://507770239"));
        assert_eq!(doc["metadata"]["unit"].as_str(), Some(eustress_common::units::Unit::Stud.symbol()));
        let (parsed, problems) = parse_sequence(&clip.text, "x").unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(parsed.keyframes.len(), 2);
        assert!(clip_from_model(b"\x89PNG\r\n", 1).unwrap().is_none(), "an image is not a clip");
    }

    #[test]
    fn scripts_name_clips_in_string_literals() {
        let animate = r#"
local animNames = {
    idle = {
        { id = "http://www.roblox.com/asset/?id=507766666", weight = 1 },
    },
}
local wave = Instance.new("Animation")
wave.AnimationId = 'rbxassetid://507770239'
local other = "http://example.com/?id=5"
"#;
        let ids: Vec<u64> = script_animation_ids(animate).into_iter().collect();
        assert_eq!(ids, vec![507766666, 507770239]);
        assert!(script_animation_ids(r#"local img = "rbxassetid://99""#).is_empty(), "not an animation script");
    }

    /// The id map is the one the runtime reads, and it keeps what it held.
    #[test]
    fn the_id_map_names_each_clip_for_the_runtime() {
        let root = std::env::temp_dir().join(format!("eustress_anim_map_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join(ROBLOX_ID_MAP), "[assets]\n\"11\" = \"assets/animations/mine.anim.toml\"\n").unwrap();
        let mut clips = BTreeMap::new();
        clips.insert(507770239u64, clip_path(507770239));
        merge_id_map(&root, &clips).unwrap();
        assert_eq!(
            eustress_common::animation::content::roblox_id_file(&root, 507770239).unwrap(),
            "assets/animations/rbx-507770239.anim.toml"
        );
        assert_eq!(
            eustress_common::animation::content::roblox_id_file(&root, 11).unwrap(),
            "assets/animations/mine.anim.toml"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
