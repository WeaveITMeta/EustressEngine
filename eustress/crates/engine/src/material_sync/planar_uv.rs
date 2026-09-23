//! Mesh geometry that lets the flat primitives (block, wedge, corner wedge)
//! carry library materials: face-planar UVs, outward winding, and the
//! per-size copies textured parts are drawn from.
//!
//! Plain functions of a `Mesh`, with no ECS.

use bevy::math::{Vec2, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};

/// Where point `p` of a unit primitive lands on its face's own plane, in
/// metres, when the primitive is drawn at `size`. `n` is the face's normal in
/// the unit mesh.
///
/// Faces are read from outside. On side and sloped faces U runs right along
/// the horizontal and V down the face, so textures stand upright; the top
/// takes U along +X and V along +Z, the bottom U along +X and V along -Z.
/// U counts from the left of the part's box and V ends at its foot, so on
/// every side of a block the texture starts at the bottom edge and its rows
/// meet at the corners.
pub fn planar_uv(p: Vec3, n: Vec3, size: Vec3) -> Vec2 {
    let size = size.abs().max(Vec3::splat(1e-6));
    let q = p * size;
    // A normal scales by the inverse, so this is the drawn face's own normal
    // and the axes below lie in it even on a stretched slope.
    let n = (n / size).normalize_or_zero();
    let (right, down) = if n.y > 0.9999 {
        (Vec3::X, Vec3::Z)
    } else if n.y < -0.9999 {
        (Vec3::X, Vec3::NEG_Z)
    } else {
        let right = Vec3::Y.cross(n).normalize_or_zero();
        (right, right.cross(n))
    };
    // Half the part's extent along an axis.
    let half = |axis: Vec3| 0.5 * (size * axis.abs()).element_sum();
    Vec2::new(q.dot(right) + half(right), q.dot(down) - half(down))
}

/// [`planar_uv`] for every vertex of `mesh` drawn at `size`, in units of
/// `metres_per_uv`. The mesh must have flat faces that share no vertices, so
/// that each vertex normal is its face's normal, as the primitives do.
pub fn planar_face_uvs(mesh: &Mesh, size: Vec3, metres_per_uv: f32) -> Option<Vec<[f32; 2]>> {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else {
        return None;
    };
    if positions.len() != normals.len() {
        return None;
    }
    Some(
        positions
            .iter()
            .zip(normals)
            .map(|(p, n)| (planar_uv(Vec3::from(*p), Vec3::from(*n), size) / metres_per_uv).to_array())
            .collect(),
    )
}

/// Turn every triangle of an indexed triangle list to face the way its
/// vertex normals point, and return how many were turned.
///
/// Front faces are counter-clockwise, so a triangle wound the other way is
/// culled when seen from outside and leaves a hole in the part. The bundled
/// wedge and corner wedge were wound like that, and Spaces may hold copies.
pub fn fix_winding(mesh: &mut Mesh) -> usize {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return 0;
    }
    let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Float32x3(normals))) =
        (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL))
    else {
        return 0;
    };
    let inward = |t: [usize; 3]| {
        let (Some(a), Some(b), Some(c)) = (positions.get(t[0]), positions.get(t[1]), positions.get(t[2])) else {
            return false;
        };
        let normal: Vec3 = t.iter().filter_map(|&i| normals.get(i)).map(|v| Vec3::from(*v)).sum();
        let (a, b, c) = (Vec3::from(*a), Vec3::from(*b), Vec3::from(*c));
        (b - a).cross(c - a).dot(normal) < 0.0
    };
    let turned: Vec<usize> = match mesh.indices() {
        Some(Indices::U16(i)) => i
            .chunks_exact(3)
            .enumerate()
            .filter(|(_, t)| inward([t[0] as usize, t[1] as usize, t[2] as usize]))
            .map(|(k, _)| k)
            .collect(),
        Some(Indices::U32(i)) => i
            .chunks_exact(3)
            .enumerate()
            .filter(|(_, t)| inward([t[0] as usize, t[1] as usize, t[2] as usize]))
            .map(|(k, _)| k)
            .collect(),
        None => return 0,
    };
    match mesh.indices_mut() {
        Some(Indices::U16(i)) => turned.iter().for_each(|&k| i.swap(3 * k + 1, 3 * k + 2)),
        Some(Indices::U32(i)) => turned.iter().for_each(|&k| i.swap(3 * k + 1, 3 * k + 2)),
        None => {}
    }
    turned.len()
}

/// A copy of flat primitive `base` for a part of `size`: outward winding,
/// UVs in tiles of `metres_per_tile` from [`planar_face_uvs`], and tangents
/// for normal maps. Every face of the part then shows the texture at the same
/// world scale, whatever the part's proportions.
pub fn sized_copy(base: &Mesh, size: Vec3, metres_per_tile: f32) -> Option<Mesh> {
    let mut mesh = base.clone();
    fix_winding(&mut mesh);
    let uvs = planar_face_uvs(&mesh, size, metres_per_tile)?;
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.generate_tangents().ok()?;
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::primitives::Cuboid;

    const TILE: f32 = 4.0;

    fn vec3s(mesh: &Mesh, attribute: bevy::mesh::MeshVertexAttribute) -> Vec<Vec3> {
        match mesh.attribute(attribute) {
            Some(VertexAttributeValues::Float32x3(v)) => v.iter().map(|a| Vec3::from(*a)).collect(),
            _ => panic!("missing {}", attribute.name),
        }
    }

    fn uvs(mesh: &Mesh) -> Vec<Vec2> {
        match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(v)) => v.iter().map(|a| Vec2::from(*a)).collect(),
            _ => panic!("missing UV_0"),
        }
    }

    fn unit_block() -> Mesh {
        Mesh::from(Cuboid::new(1.0, 1.0, 1.0))
    }

    #[test]
    fn side_faces_read_upright() {
        // +Z face: right is +X, down is -Y, and V ends at the foot.
        let top_left = planar_uv(Vec3::new(-0.5, 0.5, 0.5), Vec3::Z, Vec3::ONE);
        let bottom_right = planar_uv(Vec3::new(0.5, -0.5, 0.5), Vec3::Z, Vec3::ONE);
        assert!(top_left.abs_diff_eq(Vec2::new(0.0, -1.0), 1e-6), "{top_left}");
        assert!(bottom_right.abs_diff_eq(Vec2::new(1.0, 0.0), 1e-6), "{bottom_right}");
        // +X face: right is -Z.
        let uv = planar_uv(Vec3::new(0.5, 0.5, 0.5), Vec3::X, Vec3::ONE);
        assert!(uv.abs_diff_eq(Vec2::new(0.0, -1.0), 1e-6), "{uv}");
    }

    #[test]
    fn unit_faces_span_one_uv_unit() {
        let mut mesh = unit_block();
        let uv = planar_face_uvs(&mesh, Vec3::ONE, 1.0).unwrap();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        let (normals, uv) = (vec3s(&mesh, Mesh::ATTRIBUTE_NORMAL), uvs(&mesh));
        for n in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
            let face: Vec<Vec2> = (0..uv.len()).filter(|&i| normals[i].abs_diff_eq(n, 1e-4)).map(|i| uv[i]).collect();
            let span = face.iter().fold(Vec2::NEG_INFINITY, |a, b| a.max(*b)) - face.iter().fold(Vec2::INFINITY, |a, b| a.min(*b));
            assert!(span.abs_diff_eq(Vec2::ONE, 1e-5), "{n}: {span}");
        }
    }

    #[test]
    fn every_face_of_a_sized_copy_shows_the_texture_at_world_scale() {
        // A small brick part, long along Z. One uv_transform per part gave
        // its end faces the long faces' scale.
        let size = Vec3::new(0.3, 0.2, 2.0);
        let copy = sized_copy(&unit_block(), size, TILE).unwrap();
        let (normals, uv) = (vec3s(&copy, Mesh::ATTRIBUTE_NORMAL), uvs(&copy));
        for (n, extent) in [
            (Vec3::X, Vec2::new(size.z, size.y)),
            (Vec3::NEG_X, Vec2::new(size.z, size.y)),
            (Vec3::Z, Vec2::new(size.x, size.y)),
            (Vec3::NEG_Z, Vec2::new(size.x, size.y)),
            (Vec3::Y, Vec2::new(size.x, size.z)),
            (Vec3::NEG_Y, Vec2::new(size.x, size.z)),
        ] {
            let face: Vec<Vec2> = (0..uv.len()).filter(|&i| normals[i].abs_diff_eq(n, 1e-4)).map(|i| uv[i]).collect();
            assert_eq!(face.len(), 4, "{n}");
            let span = face.iter().fold(Vec2::NEG_INFINITY, |a, b| a.max(*b)) - face.iter().fold(Vec2::INFINITY, |a, b| a.min(*b));
            assert!(span.abs_diff_eq(extent / TILE, 1e-5), "{n}: {span} vs {}", extent / TILE);
        }
        assert!(copy.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
    }

    #[test]
    fn side_rows_start_at_the_foot() {
        let size = Vec3::new(1.3, 0.7, 2.1);
        let copy = sized_copy(&unit_block(), size, TILE).unwrap();
        let (positions, normals, uv) = (
            vec3s(&copy, Mesh::ATTRIBUTE_POSITION),
            vec3s(&copy, Mesh::ATTRIBUTE_NORMAL),
            uvs(&copy),
        );
        for i in (0..uv.len()).filter(|&i| normals[i].y.abs() < 0.5) {
            let expected = if positions[i].y < 0.0 { 0.0 } else { -size.y / TILE };
            assert!((uv[i].y - expected).abs() < 1e-5, "vertex {i}: {} vs {expected}", uv[i].y);
        }
    }

    #[test]
    fn slopes_are_measured_along_the_drawn_face() {
        // A wedge slope from the back top edge to the front foot, drawn at
        // 2 x 1 x 3: sqrt(10) m down the fall line, 2 m across.
        let n = Vec3::new(0.0, 1.0, 1.0).normalize();
        let size = Vec3::new(2.0, 1.0, 3.0);
        let top = planar_uv(Vec3::new(-0.5, 0.5, -0.5), n, size);
        let foot = planar_uv(Vec3::new(-0.5, -0.5, 0.5), n, size);
        let across = planar_uv(Vec3::new(0.5, -0.5, 0.5), n, size);
        assert!((foot.y - top.y - 10f32.sqrt()).abs() < 1e-5, "{top} {foot}");
        assert!(foot.y.abs() < 1e-5 && (foot.x - top.x).abs() < 1e-5, "{top} {foot}");
        assert!((across.x - foot.x - 2.0).abs() < 1e-5, "{foot} {across}");
    }

    #[test]
    fn inward_triangles_are_turned_out() {
        let mut mesh = unit_block();
        assert_eq!(fix_winding(&mut mesh), 0, "the cuboid already faces out");
        match mesh.indices_mut() {
            Some(Indices::U16(i)) => i.chunks_exact_mut(3).for_each(|t| t.swap(1, 2)),
            Some(Indices::U32(i)) => i.chunks_exact_mut(3).for_each(|t| t.swap(1, 2)),
            None => panic!("the cuboid is indexed"),
        }
        assert_eq!(fix_winding(&mut mesh), 12);
        assert_eq!(fix_winding(&mut mesh), 0);
    }
}
