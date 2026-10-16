//! Planning the pieces of a split Part's imported mesh: the pure part of the
//! editor's `MeshSplitParts` and `MeshSplitEdges` actions.
//!
//! The engine runs the action. It owns the trash, the undo stack and the file
//! watcher, so it writes the pieces and moves the original. This module reads
//! the Part's own mesh and record, refuses what would draw wrong, and works out
//! each piece's mesh and record. It writes nothing.
//!
//! The pieces draw exactly where the source draws its vertices. The loader draws
//! a custom mesh's vertex `v` at `position + rotation * (scale * v)`. A piece
//! with local box centre `c` and size `s` gets `position = P + R (S * c)`,
//! `rotation = R`, `scale = S * s`, and stores the vertices `(v - c) / s`, a unit
//! box centred on the origin. `[asset] scale_kind = "length"` says that unit box
//! is drawn at the part's size.
//!
//! Refused, as `import_asset` refuses them: a GLB with more than one mesh or
//! part, a node transform, or a material other than 0 (Studio draws only the
//! first part of the first mesh, with no node transform). Also refused: a folded
//! mesh (`mesh_scale` or `mesh_offset`, which the loader applies again on top of
//! the transform), a unit-stamped part (its scale converts by unit), a primitive
//! (it has no mesh), and a part that is not a Part.

use std::path::Path;

use eustress_common::instance_create::fresh_uuid_for_create;
use eustress_common::pointcloud::formats::import_mesh;
use eustress_common::space_read::primitive_shape;
use eustress_cad::EvalMesh;
use eustress_mesh_edit::segment;
use glam::{Quat, Vec3};
use toml::{Table, Value as TomlValue};

use crate::import_tools::read_glb;
use crate::sandbox;

/// More pieces than this is refused: hundreds of Parts from one mesh is almost
/// always debris, and it would flood the Explorer.
pub const MAX_PIECES: usize = 200;

/// `Split::Edges` splits where faces bend by more than this (in degrees).
pub const CREASE_DEGREES: f32 = 30.0;

/// How a mesh is cut into pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// One piece per connected part of the mesh: separate shells come apart.
    Parts,
    /// One piece per body between hard edges: a handle comes off a block.
    Edges,
}

/// A Part's mesh and record, read and checked. The vertices are the mesh's own
/// (the ones the loader draws), and the transform is the Part's.
pub struct Source {
    /// The Part's display name (its folder name when the record has none).
    pub name: String,
    pub record: Table,
    pub positions: Vec<Vec3>,
    /// Empty when the GLB has no normals.
    pub normals: Vec<Vec3>,
    /// Empty when the GLB has no UVs.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// One piece, ready for the engine to write: its folder holds `mesh.glb` (the
/// `mesh` here) and `_instance.toml` (the `record` here).
pub struct PiecePlan {
    pub name: String,
    pub triangles: usize,
    pub mesh: EvalMesh,
    pub record: Table,
    pub position: Vec3,
    pub scale: Vec3,
}

/// Read the Part whose folder is `folder`, and refuse anything a split would
/// draw wrong. `space_root` and `universe_root` resolve a mesh the record names
/// as the loader does: beside the Part, then in the Space, then in the Universe.
pub fn read_source(folder: &Path, space_root: &Path, universe_root: &Path) -> Result<Source, String> {
    let label = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Part".into());
    let text = std::fs::read_to_string(folder.join("_instance.toml")).map_err(|e| format!("{label}: {e}"))?;
    let record: Table = text.parse().map_err(|e| format!("{label}: its _instance.toml does not parse ({e})"))?;

    let metadata = record.get("metadata").and_then(TomlValue::as_table);
    let class = metadata.and_then(|m| m.get("class_name")).and_then(TomlValue::as_str).unwrap_or("");
    if class != "Part" {
        return Err(format!("{label} is a {class:?}; only a Part's mesh can be split"));
    }
    if record.contains_key("mesh_scale") || record.contains_key("mesh_offset") {
        return Err(format!("{label} carries a folded mesh; split it after the fold is removed"));
    }
    if metadata.is_some_and(|m| m.contains_key("unit")) {
        return Err(format!("{label} has a unit stamp; splitting a stamped part is not supported yet"));
    }
    let name = metadata
        .and_then(|m| m.get("name"))
        .and_then(TomlValue::as_str)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| label.clone());

    let mesh_rel = record
        .get("asset")
        .and_then(TomlValue::as_table)
        .and_then(|a| a.get("mesh"))
        .and_then(TomlValue::as_str)
        .ok_or_else(|| format!("{label} has no mesh to split: it is not a custom-mesh Part"))?;
    if let Some(shape) = primitive_shape(mesh_rel) {
        return Err(format!("{label} draws the built-in {shape:?} shape, which has no mesh to split"));
    }
    let mesh_path = [
        sandbox::resolve(folder, mesh_rel),
        sandbox::resolve(space_root, mesh_rel),
        sandbox::resolve(universe_root, mesh_rel),
    ]
    .into_iter()
    .filter_map(Result::ok)
    .find(|p| p.is_file())
    .ok_or_else(|| format!("the mesh {mesh_rel} of {label} is not on disk"))?;

    let info = read_glb(&mesh_path)?;
    let problems = info.problems();
    if !problems.is_empty() {
        return Err(format!("{label}'s mesh cannot be split as Studio draws it: {}", problems.join("; ")));
    }
    let imported = import_mesh(&mesh_path).map_err(|why| format!("{label}: {why}"))?;
    let [mesh] = imported.meshes.as_slice() else {
        return Err(format!("{label}'s mesh holds {} meshes; Studio draws one", imported.meshes.len()));
    };
    let positions: Vec<Vec3> = mesh.vertices.iter().map(|v| Vec3::from_array(v.to_array())).collect();
    let indices = mesh.indices.clone();
    if positions.is_empty() || indices.is_empty() || indices.len() % 3 != 0 {
        return Err(format!("{label}'s mesh has no triangles to split"));
    }
    if indices.iter().any(|&i| i as usize >= positions.len()) {
        return Err(format!("{label}'s mesh indexes past its vertices"));
    }
    let normals = if mesh.normals.len() == positions.len() {
        mesh.normals.iter().map(|n| Vec3::from_array(n.to_array())).collect()
    } else {
        Vec::new()
    };
    let uvs = if mesh.uvs.len() == positions.len() { mesh.uvs.iter().map(|t| t.to_array()).collect() } else { Vec::new() };

    let transform = record.get("transform").and_then(TomlValue::as_table);
    let position = Vec3::from_slice(&transform_array(transform, "position", &[0.0; 3]).map_err(|e| format!("{label}: {e}"))?);
    let rotation = {
        let q = transform_array(transform, "rotation", &[0.0, 0.0, 0.0, 1.0]).map_err(|e| format!("{label}: {e}"))?;
        Quat::from_xyzw(q[0], q[1], q[2], q[3])
    };
    let scale = Vec3::from_slice(&transform_array(transform, "scale", &[1.0; 3]).map_err(|e| format!("{label}: {e}"))?);
    if scale.min_element() <= 0.0 {
        return Err(format!("{label}: transform.scale must be positive on every axis"));
    }

    Ok(Source { name, record, positions, normals, uvs, indices, position, rotation, scale })
}

/// The pieces of `source` under `split`, each with its mesh and record. Nothing
/// is written.
pub fn plan_pieces(source: &Source, split: Split) -> Result<Vec<PiecePlan>, String> {
    let groups = match split {
        Split::Parts => segment::split_by_connectivity(&source.positions, &source.indices),
        Split::Edges => segment::split_by_crease(&source.positions, &source.indices, CREASE_DEGREES),
    };
    if groups.len() < 2 {
        return Err(match split {
            Split::Parts => format!("{} is one connected part, so there is nothing to split", source.name),
            Split::Edges => format!(
                "{} is one body at {CREASE_DEGREES} degrees; no edge is sharp enough to split it",
                source.name
            ),
        });
    }
    if groups.len() > MAX_PIECES {
        return Err(format!("{} would make {} pieces; the limit is {MAX_PIECES}", source.name, groups.len()));
    }

    let mut out = Vec::with_capacity(groups.len());
    for (k, group) in groups.iter().enumerate() {
        let piece = segment::extract(source.positions.len(), &source.indices, group);
        let local = segment::gather(&source.positions, &piece);
        let (center, size) = frame_of(local.iter().copied());
        let normals: Vec<[f32; 3]> = if source.normals.is_empty() {
            Vec::new()
        } else {
            segment::gather(&source.normals, &piece)
                .into_iter()
                .map(|n| normal_normalized(n, size).to_array())
                .collect()
        };
        let name = format!("{} {}", source.name, k + 1);
        let (position, scale) = placement(source.position, source.rotation, source.scale, center, size);
        let record = piece_record(&source.record, &name, &fresh_uuid_for_create(), position, source.rotation, scale)
            .map_err(|why| format!("{name}: {why}"))?;
        out.push(PiecePlan {
            name,
            triangles: group.len(),
            mesh: EvalMesh {
                positions: normalized(&local, center, size).iter().map(|p| p.to_array()).collect(),
                normals,
                uvs: if source.uvs.is_empty() { Vec::new() } else { segment::gather(&source.uvs, &piece) },
                indices: piece.indices.clone(),
                ..Default::default()
            },
            record,
            position,
            scale,
        });
    }
    Ok(out)
}

/// `[transform]` key `key` as `default.len()` numbers, or `default` when absent.
/// A key that is present but not that many numbers is an error, not a default.
fn transform_array(transform: Option<&Table>, key: &str, default: &[f32]) -> Result<Vec<f32>, String> {
    let Some(value) = transform.and_then(|t| t.get(key)) else {
        return Ok(default.to_vec());
    };
    let numbers: Option<Vec<f32>> = value.as_array().and_then(|items| {
        items
            .iter()
            .map(|x| x.as_float().map(|f| f as f32).or_else(|| x.as_integer().map(|i| i as f32)))
            .collect()
    });
    match numbers {
        Some(n) if n.len() == default.len() => Ok(n),
        _ => Err(format!("transform.{key} must be {} numbers", default.len())),
    }
}

// ---------------------------------------------------------------------------
// Placement: where each piece goes, and what its mesh holds.
// ---------------------------------------------------------------------------

/// The centre and size of the box round `points`, in the source mesh's own
/// coordinates. An axis with no extent (a flat piece) gets size 1, so the
/// normalised vertices stay finite and the piece keeps zero thickness there.
/// No points give the origin and a unit box.
fn frame_of(points: impl IntoIterator<Item = Vec3>) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for p in points {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if lo.x > hi.x || lo.y > hi.y || lo.z > hi.z {
        return (Vec3::ZERO, Vec3::ONE);
    }
    let extent = hi - lo;
    let size = Vec3::new(
        if extent.x > 0.0 { extent.x } else { 1.0 },
        if extent.y > 0.0 { extent.y } else { 1.0 },
        if extent.z > 0.0 { extent.z } else { 1.0 },
    );
    ((lo + hi) * 0.5, size)
}

/// Where a piece's Part sits and how big it is, from the source Part's position
/// `P`, rotation `R` and scale `S`, and the piece's box `(c, s)`.
fn placement(source_position: Vec3, source_rotation: Quat, source_scale: Vec3, center: Vec3, size: Vec3) -> (Vec3, Vec3) {
    (
        source_position + source_rotation * (source_scale * center),
        source_scale * size,
    )
}

/// The piece's own vertices, `(v - c) / s`: a unit box centred on the origin.
fn normalized(points: &[Vec3], center: Vec3, size: Vec3) -> Vec<Vec3> {
    points.iter().map(|&v| (v - center) / size).collect()
}

/// A normal after the vertices' `(v - c) / s` map. A normal moves by the inverse
/// transpose, which for a per-axis divide by `s` is a multiply by `s`, then a
/// renormalise. Multiplying the source normal alone would shade a non-uniformly
/// sized piece wrongly.
fn normal_normalized(normal: Vec3, size: Vec3) -> Vec3 {
    (size * normal).normalize_or_zero()
}

fn vec_value(v: &[f32]) -> TomlValue {
    TomlValue::Array(v.iter().map(|&x| TomlValue::Float(f64::from(x))).collect())
}

/// The record for one piece, from the source Part's record. Its properties,
/// attributes and tags carry over; its metadata, transform and mesh are the
/// piece's own.
fn piece_record(
    source: &Table,
    name: &str,
    uuid: &str,
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
) -> Result<Table, String> {
    for key in ["mesh_scale", "mesh_offset"] {
        if source.contains_key(key) {
            return Err(format!("the part carries a folded mesh ({key})"));
        }
    }
    let mut metadata = source.get("metadata").and_then(TomlValue::as_table).cloned().unwrap_or_default();
    if metadata.contains_key("unit") {
        return Err("the part has a unit stamp".to_string());
    }
    metadata.insert("class_name".into(), TomlValue::String("Part".into()));
    metadata.insert("name".into(), TomlValue::String(name.into()));
    metadata.insert("uuid".into(), TomlValue::String(uuid.into()));

    let mut transform = source.get("transform").and_then(TomlValue::as_table).cloned().unwrap_or_default();
    transform.insert("position".into(), vec_value(&position.to_array()));
    transform.insert("rotation".into(), vec_value(&rotation.to_array()));
    transform.insert("scale".into(), vec_value(&scale.to_array()));

    // The piece's mesh is a unit box, drawn at its size: a length.
    let mut asset = source.get("asset").and_then(TomlValue::as_table).cloned().unwrap_or_default();
    asset.insert("mesh".into(), TomlValue::String("mesh.glb".into()));
    asset.insert("scale_kind".into(), TomlValue::String("length".into()));

    let mut record = source.clone();
    record.insert("metadata".into(), TomlValue::Table(metadata));
    record.insert("transform".into(), TomlValue::Table(transform));
    record.insert("asset".into(), TomlValue::Table(asset));
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_cad::write_glb;

    /// A fresh folder `<temp>/eus_mesh_split_<tag>_<pid>/Thing`, and its root.
    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("eus_mesh_split_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Thing")).unwrap();
        root
    }

    /// A cube of half-width `half` on `c`, written as a glTF exporter writes one:
    /// each face has its own four vertices.
    fn cube(c: [f32; 3], half: f32) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
        let (mut pos, mut nor, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let faces = [
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        ];
        for (n, u, v) in faces {
            let base = pos.len() as u32;
            for (a, b) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let p = [0, 1, 2].map(|i| c[i] + n[i] * half + u[i] * a * half + v[i] * b * half);
                pos.push(p);
                nor.push(n);
                uv.push([(a + 1.0) * 0.5, (b + 1.0) * 0.5]);
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        (pos, nor, uv, idx)
    }

    fn write_mesh(path: &Path, parts: &[([f32; 3], f32)]) {
        let (mut positions, mut normals, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &(c, half) in parts {
            let base = positions.len() as u32;
            let (p, n, t, i) = cube(c, half);
            positions.extend(p);
            normals.extend(n);
            uvs.extend(t);
            indices.extend(i.into_iter().map(|x| x + base));
        }
        let mesh = EvalMesh { positions, normals, uvs, indices, ..Default::default() };
        write_glb(path, &mesh, None).unwrap();
    }

    /// A Part folder `<root>/Thing` with `mesh.glb` and `_instance.toml`. `extra`
    /// is written first, at the top of the record (for top-level keys).
    fn make_part(root: &Path, parts: &[([f32; 3], f32)], position: [f32; 3], scale: [f32; 3], extra: &str) -> std::path::PathBuf {
        let folder = root.join("Thing");
        write_mesh(&folder.join("mesh.glb"), parts);
        let record = format!(
            "{extra}[metadata]\nclass_name = \"Part\"\narchivable = true\nname = \"Thing\"\nuuid = \"0123abcd\"\n\n\
             [transform]\nposition = [{}, {}, {}]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [{}, {}, {}]\n\n\
             [properties]\ncolor = [0.5, 0.5, 0.5]\nanchored = true\n\n[asset]\nmesh = \"mesh.glb\"\nscale_kind = \"length\"\n",
            position[0], position[1], position[2], scale[0], scale[1], scale[2]
        );
        std::fs::write(folder.join("_instance.toml"), record).unwrap();
        folder
    }

    /// The world vertices a Part draws, from its own mesh: `P + S * v` (identity rotation).
    fn drawn(position: Vec3, scale: Vec3, vertices: &[Vec3]) -> Vec<[i64; 3]> {
        vertices
            .iter()
            .map(|v| {
                let w = position + scale * *v;
                [0, 1, 2].map(|i| (f64::from(w[i]) * 1000.0).round() as i64)
            })
            .collect()
    }

    #[test]
    fn two_loose_cubes_draw_exactly_the_source_vertices() {
        let root = temp_root("loose");
        let folder = make_part(&root, &[([0.0, 0.0, 0.0], 1.0), ([5.0, 0.0, 0.0], 1.0)], [1.0, 2.0, 3.0], [2.0, 2.0, 2.0], "");
        let source = read_source(&folder, &root, &root).expect("a plain Part is read");
        let plans = plan_pieces(&source, Split::Parts).expect("two shells split");
        assert_eq!(plans.len(), 2);

        let mut from_source = drawn(source.position, source.scale, &source.positions);
        let mut from_pieces = Vec::new();
        for plan in &plans {
            let vertices: Vec<Vec3> = plan.mesh.positions.iter().map(|p| Vec3::from_array(*p)).collect();
            from_pieces.extend(drawn(plan.position, plan.scale, &vertices));
        }
        from_source.sort_unstable();
        from_pieces.sort_unstable();
        assert_eq!(from_source, from_pieces, "the pieces draw the source's vertices, no more and no fewer");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cube_splits_into_its_six_faces_and_one_part_is_refused() {
        let root = temp_root("faces");
        let folder = make_part(&root, &[([0.0, 0.0, 0.0], 1.0)], [0.0; 3], [1.0; 3], "");
        let source = read_source(&folder, &root, &root).expect("read");

        let faces = plan_pieces(&source, Split::Edges).expect("six faces");
        assert_eq!(faces.len(), 6);
        assert!(faces.iter().all(|p| p.triangles == 2));

        let err = plan_pieces(&source, Split::Parts).err().expect("one connected part is refused");
        assert!(err.contains("one connected part"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_piece_keeps_its_source_properties_and_takes_its_own_geometry() {
        let root = temp_root("record");
        let folder = make_part(&root, &[([0.0, 0.0, 0.0], 1.0), ([5.0, 0.0, 0.0], 1.0)], [0.0; 3], [1.0; 3], "");
        let source = read_source(&folder, &root, &root).expect("read");
        let plans = plan_pieces(&source, Split::Parts).expect("split");
        let meta = plans[0].record["metadata"].as_table().expect("metadata");
        assert_eq!(meta["name"].as_str(), Some("Thing 1"));
        assert_eq!(meta["class_name"].as_str(), Some("Part"));
        assert_ne!(meta["uuid"].as_str(), Some("0123abcd"), "a new uuid");
        assert_eq!(plans[0].record["properties"]["anchored"].as_bool(), Some(true));
        assert_eq!(plans[0].record["asset"]["mesh"].as_str(), Some("mesh.glb"));
        assert_eq!(plans[0].record["asset"]["scale_kind"].as_str(), Some("length"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refusals_name_the_reason_and_write_nothing() {
        let root = temp_root("refuse");
        let folder = make_part(&root, &[([0.0, 0.0, 0.0], 1.0), ([5.0, 0.0, 0.0], 1.0)], [0.0; 3], [1.0; 3], "mesh_scale = [2.0, 2.0, 2.0]\n");
        let err = read_source(&folder, &root, &root).err().expect("a folded part is refused");
        assert!(err.contains("folded mesh"), "{err}");

        let folder = make_part(&root, &[([0.0, 0.0, 0.0], 1.0), ([5.0, 0.0, 0.0], 1.0)], [0.0; 3], [1.0; 3], "");
        let text = std::fs::read_to_string(folder.join("_instance.toml")).unwrap();
        std::fs::write(folder.join("_instance.toml"), text.replace("archivable = true\n", "archivable = true\nunit = \"ft\"\n")).unwrap();
        let err = read_source(&folder, &root, &root).err().expect("a stamped part is refused");
        assert!(err.contains("unit stamp"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
