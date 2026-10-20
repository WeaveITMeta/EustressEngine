//! # A rig's graph: coverage classes
//!
//! Roblox blends per joint: tracks rank from the highest priority down, each
//! priority takes what is left of a weight of 1, and the rest pose takes the
//! remainder. Bevy's graph weights are per node, not per joint, so the joints
//! are grouped by their *coverage*, the set of playing tracks that animate
//! them. Joints with the same coverage form a class, each class gets its own
//! masked branch, and within a class every joint sees the same tracks, so one
//! set of weights per class gives Roblox's per-joint result exactly:
//!
//! ```text
//! root (Blend)
//! ├── class 0 (Blend; masked to its joints: the right arm)
//! │   ├── rest pose
//! │   ├── walk
//! │   └── wave
//! └── class 1 (Blend; masked to its joints: everything else)
//!     ├── rest pose
//!     └── walk
//! ```
//!
//! Each class has its own clip node per track, so every node has one parent
//! and one mask (Bevy computes one mask per node). Node weights stay 1; the
//! per-frame weights go on each clip node's `ActiveAnimation`, where a weight
//! of exactly 0 is skipped before it can contribute.

use std::collections::BTreeMap;

use bevy::animation::graph::{AnimationGraph, AnimationMask, AnimationNodeIndex};
use bevy::animation::{AnimationClip, AnimationTargetId};
use bevy::prelude::*;

/// Bevy's mask width: the most classes one rig's graph can hold.
pub const MAX_CLASSES: usize = 64;

/// Joints grouped by the tracks that animate them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageClass {
    /// Indices into the rig's tracks this frame, ascending.
    pub tracks: Vec<usize>,
    /// Indices into the rig's joints, ascending.
    pub joints: Vec<usize>,
}

/// Group `joint_count` joints by the tracks covering each. `coverages[t]`
/// lists the joints track `t` animates. Joints no track animates form a
/// class of their own, which holds only the rest pose.
///
/// Returns the classes and whether some had to merge to fit Bevy's mask
/// width (the blend of a merged class is approximate).
pub fn coverage_classes(joint_count: usize, coverages: &[Vec<usize>]) -> (Vec<CoverageClass>, bool) {
    let mut signature: Vec<Vec<usize>> = vec![Vec::new(); joint_count];
    for (t, coverage) in coverages.iter().enumerate() {
        for &j in coverage {
            if j < joint_count && signature[j].last() != Some(&t) {
                signature[j].push(t);
            }
        }
    }
    let mut grouped: BTreeMap<Vec<usize>, Vec<usize>> = BTreeMap::new();
    for (j, sig) in signature.into_iter().enumerate() {
        grouped.entry(sig).or_default().push(j);
    }
    let mut classes: Vec<CoverageClass> =
        grouped.into_iter().map(|(tracks, joints)| CoverageClass { tracks, joints }).collect();
    if classes.len() <= MAX_CLASSES {
        return (classes, false);
    }
    // Keep the largest classes; fold the rest into one with every track any
    // of them had.
    classes.sort_by(|a, b| b.joints.len().cmp(&a.joints.len()).then(a.tracks.cmp(&b.tracks)));
    let tail = classes.split_off(MAX_CLASSES - 1);
    let mut merged = CoverageClass { tracks: Vec::new(), joints: Vec::new() };
    for c in tail {
        merged.tracks.extend(c.tracks);
        merged.joints.extend(c.joints);
    }
    merged.tracks.sort_unstable();
    merged.tracks.dedup();
    merged.joints.sort_unstable();
    classes.push(merged);
    (classes, true)
}

/// One class's nodes in a built graph.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassNodes {
    pub rest: AnimationNodeIndex,
    /// (index into the rig's tracks, that track's clip node in this class).
    pub tracks: Vec<(usize, AnimationNodeIndex)>,
}

/// Build the graph for `classes`: a masked blend per class (no mask when
/// there is only one), each holding the rest pose and one clip node per
/// track in the class.
pub fn build_graph(
    rest: &Handle<AnimationClip>,
    clips: &[Handle<AnimationClip>],
    classes: &[CoverageClass],
    targets: &[AnimationTargetId],
) -> (AnimationGraph, Vec<ClassNodes>) {
    let mut graph = AnimationGraph::new();
    let root = graph.root;
    let masked = classes.len() > 1;
    let mut nodes = Vec::with_capacity(classes.len());
    for (c, class) in classes.iter().enumerate() {
        let parent = if masked {
            for &j in &class.joints {
                if let Some(target) = targets.get(j) {
                    graph.add_target_to_mask_group(*target, c as u32);
                }
            }
            let mask: AnimationMask = !(1u64 << c);
            graph.add_blend_with_mask(mask, 1.0, root)
        } else {
            root
        };
        let rest_node = graph.add_clip(rest.clone(), 1.0, parent);
        let tracks = class
            .tracks
            .iter()
            .filter_map(|&t| clips.get(t).map(|clip| (t, graph.add_clip(clip.clone(), 1.0, parent))))
            .collect();
        nodes.push(ClassNodes { rest: rest_node, tracks });
    }
    (graph, nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arm_wave_over_a_walk_makes_two_classes() {
        // Joints 0..4; the walk covers all, the wave covers joints 2 and 3.
        let walk = vec![0, 1, 2, 3];
        let wave = vec![2, 3];
        let (classes, merged) = coverage_classes(4, &[walk, wave]);
        assert!(!merged);
        assert_eq!(classes.len(), 2);
        let arm = classes.iter().find(|c| c.tracks == vec![0, 1]).expect("the arm class");
        assert_eq!(arm.joints, vec![2, 3]);
        let body = classes.iter().find(|c| c.tracks == vec![0]).expect("the body class");
        assert_eq!(body.joints, vec![0, 1]);
    }

    #[test]
    fn joints_nothing_animates_form_a_rest_only_class() {
        let (classes, _) = coverage_classes(3, &[vec![0]]);
        let rest_only = classes.iter().find(|c| c.tracks.is_empty()).expect("a rest-only class");
        assert_eq!(rest_only.joints, vec![1, 2]);
    }

    #[test]
    fn more_classes_than_mask_bits_merge_the_smallest() {
        // 70 tracks, each covering one joint of its own: 70 classes plus nothing else.
        let coverages: Vec<Vec<usize>> = (0..70).map(|j| vec![j]).collect();
        let (classes, merged) = coverage_classes(70, &coverages);
        assert!(merged);
        assert_eq!(classes.len(), MAX_CLASSES);
        let joints: usize = classes.iter().map(|c| c.joints.len()).sum();
        assert_eq!(joints, 70, "no joint is lost in the merge");
    }

    #[test]
    fn one_class_needs_no_mask_and_several_do() {
        let rest = Handle::<AnimationClip>::default();
        let clips = vec![Handle::<AnimationClip>::default(), Handle::<AnimationClip>::default()];
        let targets: Vec<AnimationTargetId> =
            (0..4).map(|i| AnimationTargetId::from_iter([String::from("t"), i.to_string()])).collect();

        let (one, _) = coverage_classes(4, &[vec![0, 1, 2, 3]]);
        let (graph, nodes) = build_graph(&rest, &clips, &one, &targets);
        assert!(graph.mask_groups.is_empty());
        assert_eq!(nodes[0].tracks.len(), 1);

        let (two, _) = coverage_classes(4, &[vec![0, 1, 2, 3], vec![2, 3]]);
        let (graph, nodes) = build_graph(&rest, &clips, &two, &targets);
        assert_eq!(graph.mask_groups.len(), 4, "every joint belongs to its class's group");
        assert_eq!(nodes.len(), 2);
        let total: usize = nodes.iter().map(|n| n.tracks.len()).sum();
        assert_eq!(total, 3, "the walk appears in both classes, the wave in one");
    }
}
