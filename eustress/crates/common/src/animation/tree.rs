//! # Sequences in the tree
//!
//! On disk a `KeyframeSequence` is one record; in the tree it has Roblox's
//! shape, `Keyframe` children holding `Pose`s nested the way the joints nest,
//! plus `NumberPose`s and `KeyframeMarker`s, so scripts read and edit it with
//! Roblox's API. [`materialize_sequence`] builds that shape from a record,
//! for a world's reader and for `KeyframeSequenceProvider`;
//! [`sequence_from_tree`] reads it back for the clip builder.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bevy::prelude::*;

use super::clip::{
    parse_sequence, record_fallback_name, KeyframeData, NumberData, PoseData, Sequence, DEFAULT_AUTHORED_HIP_HEIGHT,
    MAX_RECORD_BYTES,
};
use super::content::{resolve, ClipSource, Roots};
use super::easing::{PoseEasingDirection, PoseEasingStyle};
use crate::datamodel::animation::Priority;
use crate::datamodel::{DataModel, DmValue, EnumItem, InstanceId, OutputLevel};
use crate::scripting::CFrame;

/// `KeyframeSequenceProvider:GetKeyframeSequenceAsync(id)`: a new
/// `KeyframeSequence`, parented to nothing, holding the clip `content` names:
/// a copy of a sequence in the tree, or one read from its record.
pub fn fetch_sequence(dm: &mut DataModel, content: &str) -> Result<InstanceId, String> {
    let bundled = crate::avatar::boot::bundled_root();
    let space = dm.animation.space_root().map(Path::to_path_buf);
    let roots = Roots { space: space.as_deref(), bundled: &bundled };
    match resolve(content, dm, None, &roots)? {
        ClipSource::Live { instance } => {
            dm.clone_instance(instance).ok_or_else(|| format!("{content}: the KeyframeSequence is not Archivable"))
        }
        ClipSource::Record { file } => {
            let bytes = std::fs::read(&file).map_err(|e| format!("{content}: {e}"))?;
            if bytes.len() > MAX_RECORD_BYTES {
                return Err(format!("{content}: {} bytes is over the clip limit", bytes.len()));
            }
            let (sequence, problems) =
                parse_sequence(&String::from_utf8_lossy(&bytes), &record_fallback_name(&file))
                    .map_err(|e| format!("{content}: {e}"))?;
            for p in problems {
                dm.print(OutputLevel::Warn, "KeyframeSequenceProvider", format!("{content}: {p}"));
            }
            Ok(materialize_sequence(dm, &sequence, None))
        }
        ClipSource::Gltf { .. } => Err(format!("{content} is a glTF clip, which has no KeyframeSequence")),
    }
}

/// Make `sequence` as instances under `parent`: a `KeyframeSequence`, and
/// under it its keyframes, their poses nested by joint, their number poses
/// and markers. A pose whose parent joint is not keyed in that keyframe gets
/// that parent as a weight-0 placeholder, as Roblox's editor writes it.
/// Engine writes: nothing is marked for the engine to spawn or write back.
pub fn materialize_sequence(dm: &mut DataModel, sequence: &Sequence, parent: Option<InstanceId>) -> InstanceId {
    let ks = dm.create_virtual("KeyframeSequence", &sequence.name, parent);
    dm.set_prop_from_engine(ks, "Loop", DmValue::Bool(sequence.looped));
    dm.set_prop_from_engine(ks, "Priority", DmValue::Enum(sequence.priority.enum_item()));
    dm.set_prop_from_engine(ks, "AuthoredHipHeight", DmValue::Number(sequence.authored_hip_height as f64));
    for keyframe in &sequence.keyframes {
        let k = dm.create_virtual("Keyframe", &keyframe.name, Some(ks));
        dm.set_prop_from_engine(k, "Time", DmValue::Number(keyframe.time as f64));

        let mut placed: HashMap<String, InstanceId> = HashMap::new();
        let mut pending: Vec<&PoseData> = keyframe.poses.iter().collect();
        while !pending.is_empty() {
            let before = pending.len();
            let mut rest = Vec::new();
            for pose in pending {
                let under = match pose.parent.as_deref() {
                    None => Some(k),
                    Some(p) => placed.get(p).copied(),
                };
                match under {
                    Some(under) => {
                        let id = pose_instance(dm, pose, under);
                        placed.insert(pose.joint.clone(), id);
                    }
                    None => rest.push(pose),
                }
            }
            if rest.len() == before {
                // Parents this keyframe does not pose: structural poses at the
                // top, which key nothing.
                let waiting: HashSet<&str> = rest.iter().map(|p| p.joint.as_str()).collect();
                let mut made = false;
                for pose in &rest {
                    if let Some(p) = pose.parent.as_deref() {
                        if !placed.contains_key(p) && !waiting.contains(p) {
                            let holder = dm.create_virtual("Pose", p, Some(k));
                            dm.set_prop_from_engine(holder, "CFrame", DmValue::CFrame(CFrame::IDENTITY));
                            dm.set_prop_from_engine(holder, "Weight", DmValue::Number(0.0));
                            placed.insert(p.to_string(), holder);
                            made = true;
                        }
                    }
                }
                if !made {
                    // Parents that name each other: the first goes at the top.
                    let first = rest.remove(0);
                    let id = pose_instance(dm, first, k);
                    placed.insert(first.joint.clone(), id);
                }
            }
            pending = rest;
        }
        for number in &keyframe.numbers {
            let n = dm.create_virtual("NumberPose", &number.name, Some(k));
            dm.set_prop_from_engine(n, "Value", DmValue::Number(number.value as f64));
            dm.set_prop_from_engine(n, "EasingStyle", DmValue::Enum(EnumItem::new("PoseEasingStyle", number.style.name())));
            dm.set_prop_from_engine(
                n,
                "EasingDirection",
                DmValue::Enum(EnumItem::new("PoseEasingDirection", number.direction.name())),
            );
        }
        for (name, value) in &keyframe.markers {
            let m = dm.create_virtual("KeyframeMarker", name, Some(k));
            dm.set_prop_from_engine(m, "Value", DmValue::String(value.clone()));
        }
    }
    ks
}

fn pose_instance(dm: &mut DataModel, pose: &PoseData, under: InstanceId) -> InstanceId {
    let id = dm.create_virtual("Pose", &pose.joint, Some(under));
    let transform = Transform::from_translation(pose.position).with_rotation(pose.rotation);
    dm.set_prop_from_engine(id, "CFrame", DmValue::CFrame(CFrame::from_transform(&transform)));
    dm.set_prop_from_engine(id, "EasingStyle", DmValue::Enum(EnumItem::new("PoseEasingStyle", pose.style.name())));
    dm.set_prop_from_engine(
        id,
        "EasingDirection",
        DmValue::Enum(EnumItem::new("PoseEasingDirection", pose.direction.name())),
    );
    dm.set_prop_from_engine(id, "Weight", DmValue::Number(pose.weight as f64));
    id
}

fn enum_name(v: Option<DmValue>) -> Option<String> {
    match v {
        Some(DmValue::Enum(e)) => Some(e.name),
        Some(DmValue::String(s)) => Some(s),
        _ => None,
    }
}

/// A `KeyframeSequence` in the tree as a [`Sequence`]. `None` when it holds
/// no keyframes.
pub fn sequence_from_tree(g: &DataModel, ks: InstanceId) -> Option<Sequence> {
    let mut keyframes = Vec::new();
    for &k in g.children(ks) {
        if g.class_of(k) != Some("Keyframe") {
            continue;
        }
        let time = g.get_prop(k, "Time").and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
        let mut poses = Vec::new();
        let mut markers = Vec::new();
        let mut numbers = Vec::new();
        // Poses nest the way the joints do: walk them with their parent pose.
        let mut stack: Vec<(InstanceId, Option<String>)> = g.children(k).iter().map(|c| (*c, None)).collect();
        while let Some((id, parent)) = stack.pop() {
            match g.class_of(id) {
                Some("Pose") => {
                    let name = g.name_of(id).unwrap_or_default().to_string();
                    let transform = match g.get_prop(id, "CFrame") {
                        Some(DmValue::CFrame(cf)) => cf.to_transform(),
                        _ => Transform::IDENTITY,
                    };
                    poses.push(PoseData {
                        joint: name.clone(),
                        parent: parent.clone(),
                        position: transform.translation,
                        rotation: transform.rotation,
                        style: enum_name(g.get_prop(id, "EasingStyle"))
                            .and_then(|s| PoseEasingStyle::from_name(&s))
                            .unwrap_or_default(),
                        direction: enum_name(g.get_prop(id, "EasingDirection"))
                            .and_then(|s| PoseEasingDirection::from_name(&s))
                            .unwrap_or_default(),
                        weight: g.get_prop(id, "Weight").and_then(|v| v.as_number()).unwrap_or(1.0) as f32,
                    });
                    for c in g.children(id) {
                        stack.push((*c, Some(name.clone())));
                    }
                }
                Some("NumberPose") => {
                    let value = g.get_prop(id, "Value").and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
                    numbers.push(NumberData {
                        name: g.name_of(id).unwrap_or_default().to_string(),
                        value,
                        style: enum_name(g.get_prop(id, "EasingStyle"))
                            .and_then(|s| PoseEasingStyle::from_name(&s))
                            .unwrap_or_default(),
                        direction: enum_name(g.get_prop(id, "EasingDirection"))
                            .and_then(|s| PoseEasingDirection::from_name(&s))
                            .unwrap_or_default(),
                    });
                }
                Some("KeyframeMarker") => {
                    let value =
                        g.get_prop(id, "Value").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    markers.push((g.name_of(id).unwrap_or_default().to_string(), value));
                }
                _ => {}
            }
        }
        keyframes.push(KeyframeData {
            time,
            name: g.name_of(k).unwrap_or("Keyframe").to_string(),
            poses,
            markers,
            numbers,
        });
    }
    if keyframes.is_empty() {
        return None;
    }
    keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
    Some(Sequence {
        name: g.name_of(ks).unwrap_or("KeyframeSequence").to_string(),
        looped: g.get_prop(ks, "Loop").and_then(|v| v.as_bool()).unwrap_or(true),
        priority: g.get_prop(ks, "Priority").and_then(|v| Priority::from_value(&v)).unwrap_or(Priority::Action),
        authored_hip_height: g
            .get_prop(ks, "AuthoredHipHeight")
            .and_then(|v| v.as_number())
            .map_or(DEFAULT_AUTHORED_HIP_HEIGHT, |h| h as f32),
        rig: None,
        keyframes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::clip::parse_sequence;

    const WAVE: &str = r#"
[metadata]
name = "Wave"

[keyframe_sequence]
loop = false
priority = "Action2"

[[keyframes]]
time = 0.0

[keyframes.poses.RightUpperArm]
parent = "UpperTorso"
rotation = [0.0, 0.0, 30.0]

[[keyframes]]
time = 0.4
name = "Raised"

[keyframes.poses.RightUpperArm]
parent = "UpperTorso"
rotation = [0.0, 0.0, 120.0]
easing = "Bounce"

[keyframes.numbers]
JawDrop = 0.5

[[keyframes.markers]]
name = "Wave"
value = "start"
"#;

    #[test]
    fn a_record_round_trips_through_the_tree() {
        let (seq, problems) = parse_sequence(WAVE, "w").unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let mut dm = DataModel::new();
        let rs = dm.get_service("ReplicatedStorage").unwrap();
        let ks = materialize_sequence(&mut dm, &seq, Some(rs));

        let keyframes: Vec<_> = dm.children(ks).to_vec();
        assert_eq!(keyframes.len(), 2);
        let raised = keyframes[1];
        assert_eq!(dm.name_of(raised), Some("Raised"));
        // The unposed parent is a weight-0 placeholder above the keyed arm.
        let holder = dm.find_first_child(raised, "UpperTorso", false).expect("placeholder");
        assert_eq!(dm.get_prop(holder, "Weight"), Some(DmValue::Number(0.0)));
        assert!(dm.find_first_child(holder, "RightUpperArm", false).is_some());

        let back = sequence_from_tree(&dm, ks).expect("sequence");
        assert!(!back.looped);
        assert_eq!(back.priority, Priority::Action2);
        assert_eq!(back.keyframes[1].markers, vec![("Wave".to_string(), "start".to_string())]);
        assert_eq!(back.keyframes[1].numbers[0].name, "JawDrop");
        let arm = back.keyframes[1].poses.iter().find(|p| p.joint == "RightUpperArm").unwrap();
        assert_eq!(arm.parent.as_deref(), Some("UpperTorso"));
        assert_eq!(arm.style, PoseEasingStyle::Bounce);
        let original = seq.keyframes[1].poses[0].rotation;
        assert!(crate::animation::clip::rotation_gap(arm.rotation, original) < 1e-5);
        // Placeholders key nothing: one channel, the arm's.
        let channels = back.channels();
        assert_eq!(channels.len(), 1, "{channels:?}");
    }

    #[test]
    fn a_tree_sequence_reads_its_nested_poses() {
        let mut g = DataModel::new();
        let rs = g.get_service("ReplicatedStorage").unwrap();
        let ks = g.create_virtual("KeyframeSequence", "Wave", Some(rs));
        let k = g.create_virtual("Keyframe", "Raised", Some(ks));
        g.set_prop_from_engine(k, "Time", DmValue::Number(0.5));
        let root = g.create_virtual("Pose", "HumanoidRootPart", Some(k));
        let lower = g.create_virtual("Pose", "LowerTorso", Some(root));
        g.set_prop_from_engine(lower, "EasingStyle", DmValue::String("CubicV2".into()));
        let marker = g.create_virtual("KeyframeMarker", "Step", Some(k));
        g.set_prop_from_engine(marker, "Value", DmValue::String("Left".into()));
        let seq = sequence_from_tree(&g, ks).expect("a sequence");
        assert_eq!(seq.keyframes.len(), 1);
        let pose = seq.keyframes[0].poses.iter().find(|p| p.joint == "LowerTorso").unwrap();
        assert_eq!(pose.parent.as_deref(), Some("HumanoidRootPart"));
        assert_eq!(pose.style, PoseEasingStyle::CubicV2);
        assert_eq!(seq.keyframes[0].markers, vec![("Step".to_string(), "Left".to_string())]);
        let empty = g.create_virtual("KeyframeSequence", "Empty", Some(rs));
        assert!(sequence_from_tree(&g, empty).is_none());
    }

    #[test]
    fn a_record_or_a_registered_sequence_fetches_as_a_new_copy() {
        let dir = std::env::temp_dir().join(format!("eustress-anim-fetch-{}", std::process::id()));
        let folder = dir.join("ReplicatedStorage").join("Waving");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("_instance.toml"), WAVE.replace("name = \"Wave\"\n", "")).unwrap();

        let mut dm = DataModel::new();
        dm.animation.set_space_root(Some(dir.clone()));
        let ks = fetch_sequence(&mut dm, "space://ReplicatedStorage/Waving");
        let _ = std::fs::remove_dir_all(&dir);
        let ks = ks.unwrap();
        assert_eq!(dm.name_of(ks), Some("Waving"), "a record with no name takes its folder's");
        assert_eq!(dm.parent(ks), None);
        assert_eq!(dm.children(ks).len(), 2);

        let id = dm.register_keyframe_sequence(ks).unwrap();
        let copy = fetch_sequence(&mut dm, &id).unwrap();
        assert_ne!(copy, ks, "a fetch is a copy");
        assert_eq!(sequence_from_tree(&dm, copy), sequence_from_tree(&dm, ks));
        assert!(fetch_sequence(&mut dm, "space://Characters/Wave.glb").is_err());
    }

    #[test]
    fn parents_that_name_each_other_still_place_every_pose() {
        let (mut seq, _) = parse_sequence(WAVE, "w").unwrap();
        let k = &mut seq.keyframes[0];
        k.poses[0].parent = Some("B".into());
        let mut b = k.poses[0].clone();
        b.joint = "B".into();
        b.parent = Some("RightUpperArm".into());
        k.poses.push(b);
        let mut dm = DataModel::new();
        let ks = materialize_sequence(&mut dm, &seq, None);
        let back = sequence_from_tree(&dm, ks).unwrap();
        let mut joints: Vec<_> = back.keyframes[0].poses.iter().map(|p| p.joint.as_str()).collect();
        joints.sort();
        assert_eq!(joints, ["B", "RightUpperArm"]);
    }
}
