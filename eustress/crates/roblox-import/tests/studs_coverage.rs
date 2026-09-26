//! The stud boundary's unit table covers the reflection database.
//!
//! A script written for Roblox reads and writes lengths in studs, and the
//! Play VM converts each property by the unit its table gives
//! (`eustress_common::luau::play::studs::UNITS`). A property the table leaves
//! out would cross the boundary unconverted, so this walks every class in
//! scope and its ancestors, lists each scriptable property whose type could
//! carry a length, and fails on any the table does not classify.

use std::collections::HashSet;

use eustress_common::luau::play::studs::unit_of;
use rbx_dom_weak::types::VariantType;
use rbx_reflection::Scriptability;

/// The classes whose properties convert.
const IN_SCOPE: &[&str] = &[
    "BasePart",
    "MeshPart",
    "TriangleMeshPart",
    "Model",
    "Attachment",
    "Camera",
    "Workspace",
    "Mouse",
    "Humanoid",
    "StarterPlayer",
];

/// Types a length could hide in.
fn could_carry_a_length(ty: VariantType) -> bool {
    matches!(
        ty,
        VariantType::Float32
            | VariantType::Float64
            | VariantType::Int32
            | VariantType::Int64
            | VariantType::Vector3
            | VariantType::CFrame
            | VariantType::Ray
            | VariantType::Region3
            | VariantType::Vector3int16
            | VariantType::Region3int16
            | VariantType::NumberRange
    )
}

#[test]
fn every_length_candidate_in_scope_is_classified() {
    let db = rbx_reflection_database::get().expect("the reflection database loads");
    let mut seen: HashSet<&str> = HashSet::new();
    let mut candidates = Vec::new();
    let mut unclassified = Vec::new();
    for class in IN_SCOPE {
        let mut at = Some(*class);
        while let Some(name) = at {
            let descriptor = db.classes.get(name).unwrap_or_else(|| panic!("{name} is in the reflection database"));
            if seen.insert(name) {
                for (prop, p) in &descriptor.properties {
                    if matches!(p.scriptability, Scriptability::None) || !could_carry_a_length(p.data_type.ty()) {
                        continue;
                    }
                    candidates.push(format!("{name}.{prop}"));
                    // Asked of the class that declares it, which is how the
                    // table is keyed.
                    if unit_of(name, prop).is_none() {
                        unclassified.push(format!("{name}.{prop} ({:?})", p.data_type.ty()));
                    }
                }
            }
            at = descriptor.superclass.as_deref();
        }
    }
    // The walk has to reach what it is meant to check, or a pass means
    // nothing.
    for known in ["BasePart.Size", "BasePart.CFrame", "Humanoid.WalkSpeed", "Workspace.Gravity", "Mouse.Hit"] {
        assert!(candidates.iter().any(|c| c == known), "{known} is among the candidates: {candidates:?}");
    }
    unclassified.sort();
    assert!(
        unclassified.is_empty(),
        "{} of {} length candidates are missing from the stud table:\n{}",
        unclassified.len(),
        candidates.len(),
        unclassified.join("\n")
    );
}
