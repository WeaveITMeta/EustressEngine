//! The wedge and corner-wedge shapes, as Roblox builds them, in a unit box
//! centred on the origin (a part of Size 1, 1, 1; the part's size scales it).
//! The part's front is -Z (its LookVector), its right +X and its top +Y.
//!
//! - A wedge's bottom and back (+Z) are full squares; its slope runs from the
//!   top back edge down to the bottom front edge, facing up and to the front.
//! - A corner wedge is a square base with one apex over its right front corner:
//!   a third of the box.
//!
//! `assets/parts/generate_part_meshes.py` writes `parts/wedge.glb` and
//! `parts/corner_wedge.glb` from the same table, and the test below reads the
//! files back against it. Colliders build their hulls from these corners
//! (`part_draw::shape_collider`), so what a part draws and what it collides as
//! are one shape.

use bevy::math::{Quat, Vec3};

use crate::classes::PartType;

/// A NATIVE part's pose for these meshes, given its pose against the meshes
/// Eustress drew before them: a wedge then had its tall face at the front, and
/// a corner wedge was a tetrahedron rising at its left front corner. A wedge
/// turns half a turn about its own up axis and looks exactly as it did; a
/// corner wedge turns a quarter turn so its tall corner stays where it was, its
/// width and depth swapping so its footprint does too. Roblox imports never
/// pass through here: their poses were always Roblox's, and these meshes are.
pub fn migrate_native_wedge(shape: PartType, rotation: Quat, size: Vec3) -> (Quat, Vec3) {
    match native_wedge_turn(shape) {
        Some(turn) if shape == PartType::CornerWedge => (rotation * turn, Vec3::new(size.z, size.y, size.x)),
        Some(turn) => (rotation * turn, size),
        None => (rotation, size),
    }
}

/// The `[metadata]` keys a Roblox import writes on every part it brings in
/// (its Roblox colour); Studio's re-saves keep them, a copy of an imported
/// part carries them, and a part a user adds never has them.
pub const ROBLOX_IMPORT_KEYS: [&str; 2] = ["roblox_color_srgb", "roblox_brick_color"];

/// Whether a part came from a Roblox import, asked of its `[metadata]` by key:
/// the one test the migration runner and every reader use. An import's poses
/// were always Roblox's, so it is never migrated.
pub fn is_roblox_import(has_metadata_key: impl Fn(&str) -> bool) -> bool {
    ROBLOX_IMPORT_KEYS.iter().any(|key| has_metadata_key(key))
}

/// Which wedge a part is, for the migration, from its class and its `[asset]`
/// mesh: a WedgePart or a CornerWedgePart by class, whatever its mesh; any
/// other part only when its mesh IS the primitive (`parts/corner_wedge.glb` or
/// `parts/wedge.glb`, any case, either slash). A custom mesh merely named like
/// a wedge is not one. Whether the part came from a Roblox import is a
/// separate question; an import is never migrated.
pub fn native_wedge_shape(class_name: &str, mesh: Option<&str>) -> Option<PartType> {
    match class_name {
        "WedgePart" => return Some(PartType::Wedge),
        "CornerWedgePart" => return Some(PartType::CornerWedge),
        _ => {}
    }
    let mesh = mesh?.to_lowercase().replace('\\', "/");
    if mesh.ends_with("parts/corner_wedge.glb") {
        Some(PartType::CornerWedge)
    } else if mesh.ends_with("parts/wedge.glb") {
        Some(PartType::Wedge)
    } else {
        None
    }
}

/// The turn about its own up axis a native part of `shape` takes when it is
/// migrated, or `None` for the shapes that do not change.
pub fn native_wedge_turn(shape: PartType) -> Option<Quat> {
    match shape {
        PartType::Wedge => Some(Quat::from_rotation_y(std::f32::consts::PI)),
        PartType::CornerWedge => Some(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        _ => None,
    }
}

/// A pose given relative to a migrated wedge (a child's local CFrame, or a
/// Weld's C0 or C1 on the wedge's side), so it stays where it is in the world
/// once the wedge has taken `turn`. Relative poses carry no size, so a corner
/// wedge's swapped width and depth do not enter.
pub fn migrate_pose_relative_to_wedge(turn: Quat, rotation: Quat, translation: Vec3) -> (Quat, Vec3) {
    let back = turn.inverse();
    ((back * rotation).normalize(), back * translation)
}

/// A face named on a migrated wedge (a Decal's, Texture's or SurfaceGui's
/// `Face`), so it stays on the same side in the world once the wedge has taken
/// `turn`. Names are Roblox's NormalId: Front is -Z, Right +X, Top +Y. `None`
/// for a name that is not a face.
pub fn remap_face(turn: Quat, face: &str) -> Option<&'static str> {
    use crate::classes::NormalId;
    // Read as every face is read (names, `Enum.NormalId.` prefixes, numbers).
    let moved = turn.inverse() * NormalId::parse(face)?.to_normal();
    NormalId::ALL
        .iter()
        .max_by(|a, b| a.to_normal().dot(moved).total_cmp(&b.to_normal().dot(moved)))
        .map(|face| face.as_str())
}

/// A wedge's corners: the bottom four, then the top back edge.
pub const WEDGE_CORNERS: [[f32; 3]; 6] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, -0.5, 0.5],
    [-0.5, -0.5, 0.5],
    [-0.5, 0.5, 0.5],
    [0.5, 0.5, 0.5],
];

/// A corner wedge's corners: the bottom four, then the apex over the right
/// front corner.
pub const CORNER_WEDGE_CORNERS: [[f32; 3]; 5] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, -0.5, 0.5],
    [-0.5, -0.5, 0.5],
    [0.5, 0.5, -0.5],
];

/// A wedge's faces as loops of [`WEDGE_CORNERS`], each counter-clockwise seen
/// from outside: bottom, back, slope, left, right.
pub const WEDGE_FACES: &[&[usize]] = &[&[0, 1, 2, 3], &[3, 2, 5, 4], &[1, 0, 4, 5], &[0, 3, 4], &[1, 5, 2]];

/// A corner wedge's faces as loops of [`CORNER_WEDGE_CORNERS`], each
/// counter-clockwise seen from outside: bottom, front, right, back slope, left
/// slope.
pub const CORNER_WEDGE_FACES: &[&[usize]] = &[&[0, 1, 2, 3], &[0, 4, 1], &[1, 4, 2], &[2, 4, 3], &[3, 4, 0]];

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a part's corner lands in its parent's frame.
    fn placed(rotation: Quat, size: Vec3, corner: [f32; 3]) -> Vec3 {
        rotation * (Vec3::from_array(corner) * size)
    }

    fn same_points(mut a: Vec<Vec3>, mut b: Vec<Vec3>) {
        let key = |v: &Vec3| v.to_array().map(|x| (x * 1000.0).round() as i32);
        a.sort_by_key(key);
        b.sort_by_key(key);
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(&b) {
            assert!(p.distance(*q) < 1e-4, "{p} vs {q}");
        }
    }

    #[test]
    fn a_migrated_native_wedge_looks_as_it_did() {
        // The mesh Eustress drew before: the top edge at the front (-Z).
        let old: Vec<[f32; 3]> = WEDGE_CORNERS.iter().map(|c| [c[0], c[1], if c[1] > 0.0 { -c[2] } else { c[2] }]).collect();
        let (rotation, size) = (Quat::from_rotation_x(0.3) * Quat::from_rotation_y(1.1), Vec3::new(2.0, 3.0, 5.0));
        let (r, s) = migrate_native_wedge(PartType::Wedge, rotation, size);
        assert_eq!(s, size);
        same_points(
            WEDGE_CORNERS.iter().map(|c| placed(r, s, *c)).collect(),
            old.iter().map(|c| placed(rotation, size, *c)).collect(),
        );
    }

    #[test]
    fn a_migrated_native_corner_wedge_keeps_its_tall_corner_and_footprint() {
        let (rotation, size) = (Quat::from_rotation_y(0.7), Vec3::new(2.0, 3.0, 5.0));
        let (r, s) = migrate_native_wedge(PartType::CornerWedge, rotation, size);
        assert_eq!(s, Vec3::new(5.0, 3.0, 2.0), "width and depth swap with the quarter turn");
        // The old tetrahedron rose at its left front corner.
        let old_apex = placed(rotation, size, [-0.5, 0.5, -0.5]);
        assert!(placed(r, s, CORNER_WEDGE_CORNERS[4]).distance(old_apex) < 1e-4);
        let base = |rot: Quat, sz: Vec3| {
            [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]].map(|c| placed(rot, sz, c)).to_vec()
        };
        same_points(base(r, s), base(rotation, size));
    }

    #[test]
    fn a_child_of_a_migrated_wedge_stays_where_it_is() {
        let wedge = (Quat::from_rotation_x(0.2) * Quat::from_rotation_y(0.9), Vec3::new(4.0, 1.0, -2.0));
        let child = (Quat::from_rotation_z(0.5), Vec3::new(0.3, 0.6, -0.4));
        let world = |parent: (Quat, Vec3), local: (Quat, Vec3)| (parent.0 * local.0, parent.0 * local.1 + parent.1);
        for shape in [PartType::Wedge, PartType::CornerWedge] {
            let turn = native_wedge_turn(shape).unwrap();
            let (rotation, _) = migrate_native_wedge(shape, wedge.0, Vec3::ONE);
            let moved = migrate_pose_relative_to_wedge(turn, child.0, child.1);
            let (before, after) = (world(wedge, child), world((rotation, wedge.1), moved));
            assert!(before.0.angle_between(after.0) < 1e-4 && before.1.distance(after.1) < 1e-4, "{shape:?}");
        }
    }

    /// The Legacy pose rule composes a child onto its parent's WHOLE
    /// transform, size included. The same inverse turn keeps the child's world
    /// pose, for a corner wedge too: its swapped size is the old size seen
    /// through the quarter turn.
    #[test]
    fn a_child_under_the_legacy_rule_stays_where_it_is() {
        use bevy::math::Mat4;
        let (rotation, position, size) = (Quat::from_rotation_x(0.3) * Quat::from_rotation_y(0.8), Vec3::new(1.0, 2.0, 3.0), Vec3::new(2.0, 3.0, 5.0));
        let child = (Quat::from_rotation_z(0.6), Vec3::new(0.2, -0.3, 0.4));
        for shape in [PartType::Wedge, PartType::CornerWedge] {
            let turn = native_wedge_turn(shape).unwrap();
            let (r, s) = migrate_native_wedge(shape, rotation, size);
            let moved = migrate_pose_relative_to_wedge(turn, child.0, child.1);
            let before = Mat4::from_scale_rotation_translation(size, rotation, position) * Mat4::from_rotation_translation(child.0, child.1);
            let after = Mat4::from_scale_rotation_translation(s, r, position) * Mat4::from_rotation_translation(moved.0, moved.1);
            assert!(before.abs_diff_eq(after, 1e-4), "{shape:?}: {before} vs {after}");
        }
    }

    #[test]
    fn an_import_is_known_by_its_roblox_colour() {
        let meta: toml::Table = toml::from_str("name = \"Roof\"\nroblox_brick_color = 194\n").unwrap();
        assert!(is_roblox_import(|key| meta.contains_key(key)));
        let native: toml::Table = toml::from_str("name = \"Roof\"\n").unwrap();
        assert!(!is_roblox_import(|key| native.contains_key(key)));
    }

    #[test]
    fn which_parts_count_as_native_wedges() {
        assert_eq!(native_wedge_shape("WedgePart", None), Some(PartType::Wedge));
        assert_eq!(native_wedge_shape("CornerWedgePart", Some("meshes/anything.glb")), Some(PartType::CornerWedge));
        assert_eq!(native_wedge_shape("Part", Some("parts/wedge.glb")), Some(PartType::Wedge));
        assert_eq!(native_wedge_shape("Part", Some("PARTS\\Corner_Wedge.GLB")), Some(PartType::CornerWedge));
        assert_eq!(native_wedge_shape("Part", Some("meshes/wedge_roof.glb")), None, "a custom mesh named like a wedge");
        assert_eq!(native_wedge_shape("Part", Some("parts/block.glb")), None);
        assert_eq!(native_wedge_shape("Part", None), None);
    }

    #[test]
    fn faces_on_a_migrated_wedge_keep_their_side() {
        let half = native_wedge_turn(PartType::Wedge).unwrap();
        assert_eq!(remap_face(half, "Front"), Some("Back"));
        assert_eq!(remap_face(half, "Left"), Some("Right"));
        assert_eq!(remap_face(half, "Top"), Some("Top"));
        let quarter = native_wedge_turn(PartType::CornerWedge).unwrap();
        // The four sides cycle; the face turns back by what the part turned.
        assert_eq!(remap_face(quarter, "Right"), Some("Back"));
        assert_eq!(remap_face(quarter, "Back"), Some("Left"));
        assert_eq!(remap_face(quarter, "Left"), Some("Front"));
        assert_eq!(remap_face(quarter, "Front"), Some("Right"));
        assert_eq!(remap_face(quarter, "Bottom"), Some("Bottom"));
        assert_eq!(remap_face(quarter, "Sideways"), None);
        // Faces are read as NormalId reads them everywhere.
        assert_eq!(remap_face(half, "Enum.NormalId.Front"), Some("Back"));
        assert_eq!(remap_face(half, "front"), Some("Back"));
    }

    /// Each face loop turns counter-clockwise seen from outside, and together
    /// they close a solid of `volume`.
    fn check_faces(corners: &[[f32; 3]], faces: &[&[usize]], volume: f32) {
        let c: Vec<Vec3> = corners.iter().map(|p| Vec3::from_array(*p)).collect();
        let centroid = c.iter().copied().sum::<Vec3>() / c.len() as f32;
        let mut enclosed = 0.0;
        for face in faces {
            let p: Vec<Vec3> = face.iter().map(|&i| c[i]).collect();
            let normal = (p[1] - p[0]).cross(p[2] - p[0]);
            let center = p.iter().copied().sum::<Vec3>() / p.len() as f32;
            assert!(normal.dot(center - centroid) > 0.0, "face {face:?} winds inward");
            for k in 1..p.len() - 1 {
                enclosed += p[0].dot(p[k].cross(p[k + 1])) / 6.0;
            }
        }
        assert!((enclosed - volume).abs() < 1e-5, "volume {enclosed}, want {volume}");
    }

    #[test]
    fn the_face_tables_close_outward_solids() {
        check_faces(&WEDGE_CORNERS, WEDGE_FACES, 0.5);
        check_faces(&CORNER_WEDGE_CORNERS, CORNER_WEDGE_FACES, 1.0 / 3.0);
    }

    #[test]
    fn other_shapes_and_imports_are_left_alone() {
        let (rotation, size) = (Quat::from_rotation_z(0.4), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(migrate_native_wedge(PartType::Block, rotation, size), (rotation, size));
        assert_eq!(migrate_native_wedge(PartType::Cylinder, rotation, size), (rotation, size));
    }

    /// A GLB's positions, normals and triangle indices (the one primitive the
    /// part meshes hold).
    fn read_glb(bytes: &[u8]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<usize>) {
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(u32_at(0), 0x4654_6C67, "a GLB");
        let json_len = u32_at(12);
        let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let bin = 20 + json_len + 8;
        let accessor = |index: &serde_json::Value| {
            let a = &doc["accessors"][index.as_u64().unwrap() as usize];
            let view = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let start = bin + view["byteOffset"].as_u64().unwrap_or(0) as usize;
            (start, a["count"].as_u64().unwrap() as usize)
        };
        let f32_at = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let prim = &doc["meshes"][0]["primitives"][0];
        let vec3s = |index: &serde_json::Value| {
            let (start, count) = accessor(index);
            (0..count).map(|k| [0, 1, 2].map(|i| f32_at(start + 12 * k + 4 * i))).collect::<Vec<_>>()
        };
        let positions = vec3s(&prim["attributes"]["POSITION"]);
        let normals = vec3s(&prim["attributes"]["NORMAL"]);
        let (start, count) = accessor(&prim["indices"]);
        let indices = (0..count)
            .map(|k| u16::from_le_bytes(bytes[start + 2 * k..start + 2 * k + 2].try_into().unwrap()) as usize)
            .collect();
        (positions, normals, indices)
    }

    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// The file holds exactly the table's corners, every triangle faces out
    /// along its stored normal, and the solid it closes has `volume`.
    fn check(bytes: &[u8], corners: &[[f32; 3]], volume: f32) {
        let (positions, normals, indices) = read_glb(bytes);
        let key = |p: &[f32; 3]| p.map(|x| (x * 1000.0).round() as i32);
        let mut in_file: Vec<_> = positions.iter().map(key).collect();
        in_file.sort();
        in_file.dedup();
        let mut want: Vec<_> = corners.iter().map(key).collect();
        want.sort();
        assert_eq!(in_file, want, "the mesh's corners are the table's");
        let mut enclosed = 0.0;
        for t in indices.chunks(3) {
            let (a, b, c) = (positions[t[0]], positions[t[1]], positions[t[2]]);
            let face = cross(sub(b, a), sub(c, a));
            assert!(dot(face, normals[t[0]]) > 0.0, "a triangle wound against its normal: {t:?}");
            enclosed += dot(a, cross(b, c)) / 6.0;
        }
        assert!((enclosed - volume).abs() < 1e-5, "volume {enclosed}, want {volume}: the mesh is not a closed, outward solid");
    }

    #[test]
    fn the_wedge_mesh_is_robloxs_wedge() {
        check(include_bytes!("../assets/parts/wedge.glb"), &WEDGE_CORNERS, 0.5);
        // The tall face is the back: both top corners sit at +Z.
        assert!(WEDGE_CORNERS.iter().filter(|c| c[1] > 0.0).all(|c| c[2] > 0.0));
    }

    #[test]
    fn the_corner_wedge_mesh_is_robloxs_corner_wedge() {
        check(include_bytes!("../assets/parts/corner_wedge.glb"), &CORNER_WEDGE_CORNERS, 1.0 / 3.0);
        // One apex, over the right front corner.
        let tops: Vec<_> = CORNER_WEDGE_CORNERS.iter().filter(|c| c[1] > 0.0).collect();
        assert_eq!(tops, vec![&[0.5, 0.5, -0.5]]);
    }
}
