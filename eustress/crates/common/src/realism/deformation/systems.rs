//! # Deformation Systems
//!
//! ECS systems for updating mesh deformation.

use bevy::prelude::*;
use tracing::info;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};

use super::components::*;
use crate::classes::BasePart;
use crate::realism::materials::stress_strain::StressTensor;
use crate::realism::materials::properties::MaterialProperties;
use crate::realism::particles::components::ThermodynamicState;

// ============================================================================
// Initialization
// ============================================================================

/// Initialize deformable mesh components for entities with deformation enabled
///
/// PERF (P2 two-tier — Vehicle Simulator, ~120K residency-streamed COLD parts):
/// `Without<ColdStreamed>` excludes the streamed cold parts from this
/// `Changed<BasePart>` driver. Cold parts carry `BasePart` + `Mesh3d` and lack
/// `DeformableMesh`, so without the filter Bevy O(N)-visits all ~120K of them
/// every frame just to read change-ticks. A cold streamed part is static
/// scenery with `deformation = false` (the system early-`continue`s on it
/// anyway), and `deformation` can only be toggled on via the Properties panel
/// after SELECTING the part — which promotes it by removing `ColdStreamed` (see
/// `selection_sync::sync_selection_components`). So a cold part never needs a
/// deformable mesh, and a promoted part's `deformation = true` edit is still
/// caught. Skipping cold parts is therefore safe and removes the per-frame O(N)
/// archetype visit.
pub fn init_deformable_meshes(
    mut commands: Commands,
    query: Query<
        (Entity, &BasePart, &Mesh3d),
        (
            Or<(Changed<BasePart>, With<DeformInitPending>)>,
            Without<DeformableMesh>,
            Without<crate::classes::ColdStreamed>,
        ),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (entity, base_part, mesh3d) in query.iter() {
        if !base_part.destructible {
            // Covers the disable-before-init case: drop any pending retry.
            commands.entity(entity).remove::<DeformInitPending>();
            continue;
        }

        let source_handle = &mesh3d.0;

        // The mesh asset may still be streaming in (GLB parts load async).
        // Mark for retry rather than dropping the part on the floor — the
        // `Changed<BasePart>` tick that got us here does not come back.
        let Some(source_mesh) = meshes.get(source_handle) else {
            commands.entity(entity).insert(DeformInitPending);
            continue;
        };

        // NOTE: the mesh is taken at its AUTHORED resolution, deliberately.
        //
        // An authored cube has 24 vertices, all at corners, so it cannot show a
        // dent in the middle of a face — and this used to be fixed here by
        // subdividing the whole mesh at init. That worked but charged every
        // deformable part a five-level subdivision (12,288 triangles on a 3 m
        // plate) at LOAD, whether or not anything ever hit it, and still
        // resolved the crater coarsely because the budget was spread evenly
        // over a surface that is mostly never touched.
        //
        // Resolution is now added on impact instead, only where the crater
        // lands — see `refine_for_impact`. A part that is never hit stays at 24
        // vertices forever.

        // Capture the undeformed reference pose. Every later vertex write is
        // absolute against this snapshot, which is what makes drift
        // structurally impossible rather than merely unlikely.
        let Some(VertexAttributeValues::Float32x3(src_positions)) =
            source_mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            // No float3 position attribute — nothing this pipeline can deform.
            commands.entity(entity).remove::<DeformInitPending>();
            continue;
        };
        let original_positions: Vec<Vec3> = src_positions
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect();
        // Keep the undeformed normals as a fallback for degenerate recomputes.
        let original_normals: Vec<Vec3> = match source_mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(n)) => {
                n.iter().map(|v| Vec3::new(v[0], v[1], v[2])).collect()
            }
            _ => Vec::new(),
        };
        let vertex_count = original_positions.len();
        if vertex_count == 0 {
            // A real, loaded, but empty mesh can never deform — stop retrying.
            commands.entity(entity).remove::<DeformInitPending>();
            continue;
        }

        // Give this entity its OWN mesh asset to deform into. Primitive parts
        // share one cached source mesh (`PrimitiveMeshCache`), so writing
        // deformed vertices into that asset would deform every part sharing
        // it. Bind the clone to a local first so the read borrow of `meshes`
        // ends before the insert.
        let mesh_clone = source_mesh.clone();
        let deformed_handle = meshes.add(mesh_clone);

        let mut displacements = VertexDisplacements::default();
        displacements.init(vertex_count);

        commands
            .entity(entity)
            .insert((
                DeformableMesh {
                    original_mesh: source_handle.clone(),
                    deformed_mesh: deformed_handle.clone(),
                    original_positions,
                    original_normals,
                    vertex_count,
                    dirty: false,
                    quality: DeformationQuality::Medium,
                },
                displacements,
                // Empty until the first impact. It takes its seed topology from
                // the authored mesh then, and persists across impacts so
                // successive craters keep refining the SAME structure.
                MeshRefinement::default(),
                // Render the deformable copy, not the shared source — without
                // this the entity keeps drawing the pristine cached mesh and
                // deformation is computed but never visible.
                Mesh3d(deformed_handle),
            ))
            .remove::<DeformInitPending>();

        info!("Initialized deformable mesh with {} vertices", vertex_count);
    }
}

/// Remove deformation components when deformation is disabled.
///
/// Also restores `Mesh3d` to the undeformed source asset. Dropping the
/// components alone would leave the entity rendering its deformed per-entity
/// copy forever — the part would keep its dents after deformation was switched
/// off, with nothing left in the world able to undo them.
///
/// NOTE: this system was written but never registered in `DeformationPlugin`,
/// so toggling `deformation = false` previously did nothing at all.
pub fn cleanup_deformable_meshes(
    mut commands: Commands,
    query: Query<(Entity, &BasePart, &DeformableMesh), Changed<BasePart>>,
) {
    for (entity, base_part, deform_mesh) in query.iter() {
        if !base_part.destructible {
            commands
                .entity(entity)
                .insert(Mesh3d(deform_mesh.original_mesh.clone()))
                .remove::<DeformableMesh>()
                .remove::<VertexDisplacements>()
                .remove::<MeshRefinement>();
        }
    }
}

/// Tear down every deformable back to its undeformed source mesh.
///
/// The engine calls this on play-stop so runtime damage never leaks into Edit
/// mode or into a save: "stop always restores" is an engine invariant, and a
/// dented mesh that survived Stop would be indistinguishable from authored
/// geometry. Re-entering Play re-initialises from the pristine source.
pub fn restore_all_deformables(
    mut commands: Commands,
    query: Query<(Entity, &DeformableMesh)>,
) {
    let mut restored = 0usize;
    for (entity, deform_mesh) in query.iter() {
        commands
            .entity(entity)
            .insert(Mesh3d(deform_mesh.original_mesh.clone()))
            .remove::<DeformableMesh>()
            .remove::<VertexDisplacements>()
            .remove::<MeshRefinement>()
            .remove::<DeformInitPending>();
        restored += 1;
    }
    if restored > 0 {
        info!("Restored {restored} deformable mesh(es) to undeformed source");
    }
}

// ============================================================================
// Stress-Based Deformation
// ============================================================================

/// Update vertex displacement from stress tensor
pub fn update_stress_deformation(
    mut query: Query<(
        &BasePart,
        &StressTensor,
        &MaterialProperties,
        &mut VertexDisplacements,
        &mut DeformableMesh,
    )>,
    config: Res<DeformationConfig>,
) {
    for (base_part, stress, material, mut deform_state, mut deform_mesh) in query.iter_mut() {
        if !base_part.destructible {
            continue;
        }
        
        let vertex_count = deform_state.elastic_displacement.len();
        if vertex_count == 0 {
            continue;
        }
        
        // Calculate strain from stress using Hooke's law: ε = σ/E
        let young_modulus = material.young_modulus;
        let poisson = material.poisson_ratio;
        
        // Principal strains from principal stresses
        let strain_x = (stress.principal[0] - poisson * (stress.principal[1] + stress.principal[2])) / young_modulus;
        let strain_y = (stress.principal[1] - poisson * (stress.principal[0] + stress.principal[2])) / young_modulus;
        let strain_z = (stress.principal[2] - poisson * (stress.principal[0] + stress.principal[1])) / young_modulus;
        
        let strain_vec = Vec3::new(strain_x, strain_y, strain_z) * config.scale;
        
        // An unstressed part with nothing already displaced has no work to do.
        // Falling through would re-mark `dirty` every frame and rewrite +
        // renormal the entire vertex buffer forever for a zero-magnitude
        // strain field.
        if strain_vec.length_squared() < 1e-24 && deform_state.max_displacement <= f32::EPSILON {
            continue;
        }

        // Apply strain to vertices (uniform strain field; a fuller model would
        // interpolate the stress field across the mesh).
        //
        // Positions come from the reference-pose snapshot. The previous code
        // FABRICATED each vertex position from its INDEX (`i as f32 / count`,
        // `i % 100`, `i % 10`) — an index has no relationship to where the
        // vertex actually sits, so `displacement = strain × position` produced
        // geometric noise instead of expansion along the strain axes.
        let max_disp = base_part.size.min_element() * config.max_displacement_ratio;
        let count = vertex_count.min(deform_mesh.original_positions.len());

        for i in 0..count {
            let local_pos = deform_mesh.original_positions[i];

            // Displacement = strain × position
            let displacement = strain_vec * local_pos;
            let clamped = displacement.clamp_length_max(max_disp);

            deform_state.elastic_displacement[i] = clamped;

            // Check for plastic yield
            deform_state.check_yield(i, base_part.size);
        }

        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Thermal Deformation
// ============================================================================

/// Update vertex displacement from temperature
pub fn update_thermal_deformation(
    mut query: Query<(
        &BasePart,
        &ThermodynamicState,
        &mut VertexDisplacements,
        &mut DeformableMesh,
    )>,
    config: Res<DeformationConfig>,
) {
    for (base_part, thermo, mut deform_state, mut deform_mesh) in query.iter_mut() {
        if !base_part.destructible || !deform_state.allow_thermal {
            continue;
        }
        
        let vertex_count = deform_state.thermal_displacement.len();
        if vertex_count == 0 {
            continue;
        }
        
        let temperature = thermo.temperature;
        let delta_t = temperature - deform_state.reference_temperature;
        let thermal_strain = deform_state.thermal_expansion_coeff * delta_t;
        
        // A part at reference temperature with nothing displaced has no work.
        // Without this the system re-marks `dirty` every frame and rewrites the
        // whole vertex buffer for a zero-magnitude expansion.
        if thermal_strain.abs() < 1e-12 && deform_state.max_displacement <= f32::EPSILON {
            continue;
        }

        // Apply thermal expansion (isotropic, radial about the mesh origin).
        //
        // Positions come from the reference-pose snapshot; the previous code
        // fabricated them from the vertex INDEX, so "radial" expansion pushed
        // vertices along axes unrelated to where they actually were.
        let count = vertex_count.min(deform_mesh.original_positions.len());

        for i in 0..count {
            let local_pos = deform_mesh.original_positions[i];
            deform_state.thermal_displacement[i] = local_pos * thermal_strain * config.scale;
        }

        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Impact Deformation
// ============================================================================

/// Smallest edge length (metres) refinement will chase.
///
/// A very small crater would otherwise drive the target edge toward zero and
/// spend the whole triangle budget on a dimple nobody can see.
const MIN_REFINE_EDGE_M: f32 = 0.015;

/// How many triangles should span the crater RADIUS.
///
/// Refinement splits while the LONGEST edge exceeds the target, so the result
/// overshoots: a right triangle whose hypotenuse just fits has legs about 40%
/// shorter still. Four here lands nearer seven or eight samples across the
/// radius in practice, which is enough for the falloff to read as a smooth bowl
/// rather than a faceted pit.
const CRATER_SAMPLES: f32 = 4.0;

/// Width (metres) of the band along a part's silhouette where a dent is faded
/// out. See the seam-guard note in [`apply_impact_deformation`].
const SEAM_FADE_M: f32 = 0.05;

/// The outward surface normal at an impact point, taken from the authored
/// normal of the nearest reference-pose vertex.
///
/// Used to aim the dent perpendicular to the surface. The nearest vertex is
/// measured in WORLD metres (local offsets scaled by the part's size) so a
/// flat, non-uniformly scaled plate does not pick a vertex on its rim just
/// because that axis is compressed in local space.
///
/// Reads the authored normals rather than recomputing from triangles: they are
/// already the reference-pose surface direction, and they stay correct while
/// the surface around them is being displaced.
fn nearest_surface_normal(deform_mesh: &DeformableMesh, point: Vec3, size: Vec3) -> Option<Vec3> {
    if deform_mesh.original_normals.len() != deform_mesh.original_positions.len() {
        return None;
    }
    let mut best: Option<(f32, Vec3)> = None;
    for (p, n) in deform_mesh
        .original_positions
        .iter()
        .zip(deform_mesh.original_normals.iter())
    {
        let d = ((*p - point) * size).length_squared();
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, *n));
        }
    }
    best.map(|(_, n)| n.normalize_or_zero())
        .filter(|n| n.is_finite() && *n != Vec3::ZERO)
}

/// Add mesh resolution where an impact is about to land.
///
/// Returns the triangle count if the topology changed. Refinement APPENDS
/// vertices and never renumbers existing ones, so every index already held by
/// `VertexDisplacements`, the reference pose, and the mesh's own index buffer
/// stays valid — that is what makes it safe to do mid-simulation, between one
/// impact and the next.
fn refine_for_impact(
    deform_mesh: &mut DeformableMesh,
    deform_state: &mut VertexDisplacements,
    refinement: &mut MeshRefinement,
    meshes: &mut Assets<Mesh>,
    center: Vec3,
    radius: f32,
    size: Vec3,
    config: &DeformationConfig,
) -> Option<usize> {
    // Resolve the crater, not the part: a fixed world-space edge target spends
    // the same triangles on a 30 cm dent and a 3 m one.
    let target_edge = (radius / CRATER_SAMPLES).clamp(MIN_REFINE_EDGE_M, config.target_edge_m);

    let mesh = meshes.get(&deform_mesh.deformed_mesh)?;
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    // Only used to SEED the forest on the very first impact. Afterwards the
    // forest is the source of truth — feeding the rendered index buffer back in
    // is precisely the mistake that let green transition triangles be refined
    // into slivers and shredded the surface on the second impact.
    let seed_indices: Vec<u32> = match mesh.indices() {
        Some(Indices::U32(v)) => v.clone(),
        Some(Indices::U16(v)) => v.iter().map(|i| *i as u32).collect(),
        None => return None,
    };
    let uvs: Vec<Vec2> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(v)) => {
            v.iter().map(|t| Vec2::new(t[0], t[1])).collect()
        }
        _ => Vec::new(),
    };

    let base = deform_mesh.original_positions.len();
    let refined = refinement.0.refine(
        &mut deform_mesh.original_positions,
        &seed_indices,
        size,
        center,
        radius,
        target_edge,
        config.max_subdivision_levels,
        config.max_triangles,
    )?;

    // Normals and UVs come from the parent edge. Both loops read entries they
    // may have just appended, which is well-defined because a midpoint's
    // parents are always older than the midpoint itself.
    let had_normals = deform_mesh.original_normals.len() == base;
    let had_uvs = uvs.len() == base;
    let mut new_uvs = uvs;
    for &(a, b) in &refined.added_parents {
        let (ia, ib) = (a as usize, b as usize);
        if had_normals {
            let n = (deform_mesh.original_normals[ia] + deform_mesh.original_normals[ib])
                .normalize_or_zero();
            deform_mesh.original_normals.push(n);
        }
        if had_uvs {
            let uv = (new_uvs[ia] + new_uvs[ib]) * 0.5;
            new_uvs.push(uv);
        }
    }

    deform_state.grow_interpolated(&refined.added_parents);
    deform_state.update_total();
    deform_mesh.vertex_count = deform_mesh.original_positions.len();

    let n = deform_mesh.original_positions.len();
    let mut mesh = meshes.get_mut(&deform_mesh.deformed_mesh)?;

    // Positions and normals here are placeholders — the chained
    // `update_mesh_vertices` rewrites both this same frame — but every
    // attribute has to reach the new length NOW, because the index buffer
    // written below already references the appended vertices.
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        deform_mesh
            .original_positions
            .iter()
            .map(|p| [p.x, p.y, p.z])
            .collect::<Vec<_>>(),
    );
    if deform_mesh.original_normals.len() == n {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            deform_mesh
                .original_normals
                .iter()
                .map(|v| [v.x, v.y, v.z])
                .collect::<Vec<_>>(),
        );
    }
    if new_uvs.len() == n {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            new_uvs.iter().map(|t| [t.x, t.y]).collect::<Vec<_>>(),
        );
    }

    // Anything we did not extend (tangents on a GLB-authored part, vertex
    // colours) is now the wrong length, and a mesh whose attribute buffers
    // disagree is rejected outright by the render pipeline. Dropping them is
    // the safe resolution: tangents are only needed for normal mapping, and
    // Bevy regenerates or defaults them.
    let stale: Vec<_> = mesh
        .attributes()
        .filter(|(_, values)| values.len() != n)
        .map(|(attr, _)| attr.id)
        .collect();
    for id in stale {
        mesh.remove_attribute(id);
    }

    mesh.insert_indices(Indices::U32(refined.indices));
    Some(refined.triangle_count)
}

/// Apply deformation from impact events
pub fn apply_impact_deformation(
    mut events: MessageReader<ImpactDeformEvent>,
    mut query: Query<(
        &BasePart,
        &mut VertexDisplacements,
        &mut DeformableMesh,
        &mut MeshRefinement,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    config: Res<DeformationConfig>,
) {
    for event in events.read() {
        let Ok((base_part, mut deform_state, mut deform_mesh, mut refinement)) =
            query.get_mut(event.entity)
        else {
            continue;
        };

        // `Vec3::normalize` returns NaN for a zero vector, and a single NaN
        // vertex poisons the whole mesh (NaN bounds → the part vanishes or the
        // renderer chokes). A zero-force impact is simply a no-op.
        let direction = event.force.normalize_or_zero();
        if direction == Vec3::ZERO || event.radius <= 0.0 {
            continue;
        }

        // `event.force` carries the dent DEPTH IN METRES in its length, and
        // `event.radius` is likewise metres. Mesh vertices, however, live in
        // the primitive's unit-cube local space, so every comparison below has
        // to be converted with the part's size — which IS the local→world
        // scale for these parts.
        //
        // Doing this in local units instead was subtly wrong on any
        // non-uniformly scaled part: a 6 x 0.6 x 4 plate would spread the
        // falloff 10x further along X than along Y for the same local radius,
        // so the "dent" was a smeared ellipsoid rather than a crater.
        let size = base_part.size;

        // DENT ALONG THE SURFACE NORMAL, NOT ALONG THE FORCE.
        //
        // An indentation is perpendicular to the surface it is made in.
        // Displacing along the raw force vector instead means an oblique hit
        // shoves vertices SIDEWAYS across the face, and as soon as that
        // tangential motion exceeds the spacing between neighbouring vertices
        // they slide past one another and the triangles fold inside out — which
        // renders as a shredded surface with normals pointing into the part.
        //
        // The magnitude clamp below cannot prevent that. It bounds how far a
        // vertex moves; folding is about how far a vertex moves RELATIVE TO ITS
        // NEIGHBOURS. Projecting onto the normal removes the tangential
        // component entirely, so folding is structurally impossible rather than
        // merely bounded.
        //
        // It is also the better physics: the component along the inward normal
        // is what actually indents, so a glancing blow now dents less than a
        // square one instead of dragging the surface along with it.
        let surface_normal = nearest_surface_normal(&deform_mesh, event.point, size);
        let dent_dir = match surface_normal {
            // Into the surface, i.e. opposite the outward normal.
            Some(n) => -n,
            // No authored normals to project onto — fall back to the force
            // direction and accept the old behaviour rather than skipping the
            // impact entirely.
            None => direction,
        };
        let obliquity = if surface_normal.is_some() {
            direction.dot(dent_dir).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if obliquity <= 1.0e-4 {
            // A blow travelling along the surface scuffs it; it does not dent.
            continue;
        }
        let direction = dent_dir;

        let peak_m = event.force.length() * config.scale * obliquity;

        // Metres travelled per unit of local displacement along the dent
        // direction, used to convert the depth back into local units.
        let size_along = (direction * size).length().max(1.0e-6);

        // Never let one impact (or an accumulation of them) turn the part
        // inside out. Clamp in METRES, then convert.
        let max_depth_m = size.min_element() * config.max_displacement_ratio;
        let peak_m = peak_m.min(max_depth_m);
        let peak_local = peak_m / size_along;
        let max_disp_local = max_depth_m / size_along;

        // Give the crater somewhere to land BEFORE displacing anything. The
        // part is carrying its authored resolution until something hits it, so
        // on a 24-vertex cube there is otherwise nothing inside the impact
        // radius and the dent silently does nothing.
        if let Some(tris) = refine_for_impact(
            &mut deform_mesh,
            &mut deform_state,
            &mut refinement,
            &mut meshes,
            event.point,
            event.radius,
            size,
            &config,
        ) {
            info!(
                "deform-refine: entity={:?} now {} tris / {} verts (crater r={:.3}m)",
                event.entity,
                tris,
                deform_mesh.original_positions.len(),
                event.radius
            );
        }

        // SEAM GUARD — the local extent of the part, used to fade the dent out
        // along the silhouette.
        //
        // Refinement is per connected patch, and an authored cube's six faces
        // do NOT share vertices: each face carries its own copy of the four
        // corners so it can have its own normals. So the impacted face can gain
        // a vertex halfway along a boundary edge that the adjoining side face
        // has no counterpart for, and displacing it would peel the two faces
        // apart into a visible crack. (Uniform subdivision never hit this
        // because it refined every face identically, so the coincident
        // duplicates moved together.)
        //
        // Fading the dent to zero as it approaches the boundary makes the tear
        // structurally impossible instead of merely unlikely, and it is the
        // more physical answer anyway: the rim is where an edge-supported plate
        // is held. The band is a few centimetres, so a dent anywhere but right
        // on the edge is untouched.
        let (local_min, local_max) = {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for p in deform_mesh.original_positions.iter() {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            (lo, hi)
        };

        // Apply radial deformation from the impact point, using the
        // reference-pose snapshot so repeated impacts all measure distance
        // from the SAME undeformed geometry rather than from the running
        // deformed result.
        for (i, original) in deform_mesh.original_positions.iter().enumerate() {
            // Local offset → world metres, so the falloff sphere is a real
            // sphere in world space regardless of the part's aspect ratio.
            let dist = ((*original - event.point) * size).length();
            if dist >= event.radius {
                continue;
            }

            let mut edge_fade = 1.0f32;
            for axis in 0..3 {
                // Skip the axis the dent travels ALONG: that is the impacted
                // face itself, which must be free to move, not the rim.
                if direction[axis].abs() > 0.5 {
                    continue;
                }
                let s = size[axis].abs();
                if s <= 0.0 {
                    continue;
                }
                let to_lo = (original[axis] - local_min[axis]) * s;
                let to_hi = (local_max[axis] - original[axis]) * s;
                let margin = to_lo.min(to_hi).max(0.0);
                edge_fade = edge_fade.min((margin / SEAM_FADE_M).clamp(0.0, 1.0));
            }

            let falloff = 1.0 - (dist / event.radius);
            let displacement = direction * (peak_local * falloff * edge_fade);

            if event.permanent {
                // Directly add to plastic displacement
                if i < deform_state.plastic_displacement.len() {
                    let acc = (deform_state.plastic_displacement[i] + displacement)
                        .clamp_length_max(max_disp_local);
                    deform_state.plastic_displacement[i] = acc;
                }
            } else {
                deform_state.apply_elastic(i, displacement);
                if i < deform_state.elastic_displacement.len() {
                    deform_state.elastic_displacement[i] =
                        deform_state.elastic_displacement[i].clamp_length_max(max_disp_local);
                }
            }
        }

        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Elastic Recovery
// ============================================================================

/// Relax elastic (recoverable) displacement back toward the reference pose.
///
/// Without this, an "elastic" dent is indistinguishable from a plastic one:
/// [`VertexDisplacements::reset_elastic`] existed but nothing ever called it,
/// and `DeformationConfig::damping` was dead config. Plastic displacement is
/// deliberately untouched — permanent means permanent.
///
/// Only touches entities that actually carry elastic displacement, and marks
/// the mesh dirty only while it is still relaxing, so a settled part costs one
/// comparison per frame.
pub fn relax_elastic_deformation(
    mut query: Query<(&mut VertexDisplacements, &mut DeformableMesh)>,
    config: Res<DeformationConfig>,
    time: Res<Time>,
) {
    // Exponential decay at `damping` per second, so recovery looks identical
    // at 30 and 144 fps. `reset_elastic` multiplies by `1 - shed`, so pass it
    // the fraction removed this frame.
    let dt = time.delta_secs();
    if config.damping <= 0.0 || dt <= 0.0 {
        return;
    }
    let shed = (1.0 - (-config.damping * dt).exp()).clamp(0.0, 1.0);
    if shed <= 0.0 {
        return;
    }

    for (mut deform_state, mut deform_mesh) in query.iter_mut() {
        // O(1) reject for a settled part — `max_elastic_sq` is maintained by
        // `update_total`, so this costs one comparison rather than a full
        // per-vertex scan every frame.
        if deform_state.max_elastic_sq <= 1e-12 {
            continue;
        }

        deform_state.reset_elastic(shed);

        // Snap residuals to zero so the check above eventually rejects and the
        // part stops re-uploading its vertex buffer.
        for d in deform_state.elastic_displacement.iter_mut() {
            if d.length_squared() <= 1e-12 {
                *d = Vec3::ZERO;
            }
        }

        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Mesh Update
// ============================================================================

/// Apply total displacement to mesh vertices
///
/// Writes `original + total_displacement` into the entity's own deformed mesh
/// asset, recomputes normals so lighting follows the new surface, and clears
/// `dirty`. Clearing is why the query takes `&mut DeformableMesh`: with the
/// previous immutable borrow the flag could never be lowered, so every
/// deformable re-uploaded its whole vertex buffer every frame forever.
pub fn update_mesh_vertices(
    mut query: Query<(&mut DeformableMesh, &VertexDisplacements)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut warned_aliased: Local<bool>,
) {
    for (mut deform_mesh, deform_state) in query.iter_mut() {
        if !deform_mesh.dirty {
            continue;
        }

        // Defensive: `init_deformable_meshes` always allocates a distinct
        // per-entity asset. A hand-built `DeformableMesh` that aliased the two
        // handles would write deformed vertices straight into the SHARED
        // source mesh and visibly deform every other part using it. (Aliasing
        // can no longer cause drift now that writes are absolute against
        // `original_positions`, but corrupting the shared asset is reason
        // enough to refuse.)
        if deform_mesh.original_mesh.id() == deform_mesh.deformed_mesh.id() {
            if !*warned_aliased {
                *warned_aliased = true;
                tracing::warn!(
                    "DeformableMesh has original_mesh == deformed_mesh; skipping vertex \
                     write to avoid corrupting the shared source mesh. Build it via \
                     init_deformable_meshes."
                );
            }
            deform_mesh.dirty = false;
            continue;
        }

        // Absolute write against the captured reference pose — never a
        // read-back of this system's own previous output.
        let mut new_positions: Vec<[f32; 3]> =
            Vec::with_capacity(deform_mesh.original_positions.len());

        for (i, original) in deform_mesh.original_positions.iter().enumerate() {
            let p = *original + deform_state.get_displacement(i);
            new_positions.push([p.x, p.y, p.z]);
        }


        // Now get mutable reference to deformed mesh and update it
        let Some(mut mesh) = meshes.get_mut(&deform_mesh.deformed_mesh) else {
            // Asset not resolvable this frame — leave `dirty` set so the write
            // is retried rather than silently dropped.
            continue;
        };

        // Lighting has to follow the deformed surface — without this a dented
        // panel keeps shading as though it were still flat.
        //
        // This does NOT use `Mesh::compute_normals`. On a subdivided mesh that
        // produced visibly shattered shading: a single-coloured plate rendered
        // as a patchwork of sky-blue / sun-white / black shards. Any vertex
        // whose accumulated face normals cancel toward zero normalizes to NaN,
        // and the shader then lights those triangles from arbitrary
        // directions. Positions stayed finite, so the part never vanished —
        // it just looked shattered, which reads as "deformation is broken".
        //
        // Accumulate face normals manually and fall back to the vertex's
        // ORIGINAL normal whenever the result is degenerate or non-finite, so
        // a bad vertex degrades to flat shading instead of garbage.
        if mesh.primitive_topology() == PrimitiveTopology::TriangleList {
            let indices: Vec<u32> = match mesh.indices() {
                Some(Indices::U32(v)) => v.clone(),
                Some(Indices::U16(v)) => v.iter().map(|i| *i as u32).collect(),
                None => Vec::new(),
            };

            if !indices.is_empty() {
                let n_verts = deform_mesh.original_positions.len();
                let mut acc = vec![Vec3::ZERO; n_verts];

                // Which vertices actually MOVED, and which share a triangle
                // with one that did.
                //
                // Recomputing normals for the whole mesh — as this used to —
                // replaced the authored normals across the entire part even
                // though only the crater's geometry changed. Tiny differences
                // between authored and recomputed normals then showed up as
                // pale streaks over the WHOLE surface, far outside the dent.
                // Adopting recomputed normals only in the affected region
                // leaves the other ~99% byte-identical to how it rendered
                // before the impact, so an artifact cannot appear where no
                // geometry moved.
                let moved: Vec<bool> = (0..n_verts)
                    .map(|i| deform_state.get_displacement(i).length_squared() > 1e-14)
                    .collect();
                let mut near_moved = vec![false; n_verts];

                for tri in indices.chunks_exact(3) {
                    let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                    if a >= n_verts || b >= n_verts || c >= n_verts {
                        continue;
                    }
                    if moved[a] || moved[b] || moved[c] {
                        near_moved[a] = true;
                        near_moved[b] = true;
                        near_moved[c] = true;
                    }
                    let pa = Vec3::from(new_positions[a]);
                    let pb = Vec3::from(new_positions[b]);
                    let pc = Vec3::from(new_positions[c]);
                    // UNnormalized cross product: its magnitude is twice the
                    // triangle area, which correctly weights large faces more
                    // and makes degenerate triangles contribute nothing
                    // instead of a NaN.
                    let face = (pb - pa).cross(pc - pa);
                    if face.is_finite() {
                        acc[a] += face;
                        acc[b] += face;
                        acc[c] += face;
                    }
                }

                let mut flipped = 0usize;
                let mut fellback = 0usize;

                let out: Vec<[f32; 3]> = acc
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let orig = deform_mesh
                            .original_normals
                            .get(i)
                            .copied()
                            .unwrap_or(Vec3::Y);

                        // Outside the dented region, keep the authored normal
                        // exactly. Accumulation still ran over every triangle
                        // (so the region's own normals are complete and
                        // correct); this only decides where the result is
                        // ADOPTED.
                        if !near_moved[i] {
                            return [orig.x, orig.y, orig.z];
                        }

                        let mut v = n.normalize_or_zero();

                        if v == Vec3::ZERO || !v.is_finite() {
                            // Degenerate — keep the undeformed normal.
                            fellback += 1;
                            v = orig;
                        } else if orig != Vec3::ZERO && v.dot(orig) < 0.0 {
                            // ORIENTATION GUARD. A recomputed normal can come
                            // out pointing INTO the surface if the mesh winds
                            // clockwise rather than counter-clockwise — the
                            // cross-product order below assumes CCW. An
                            // inverted normal is finite and non-zero, so the
                            // degenerate check above waves it through, and the
                            // surface then faces away from every light and
                            // self-shadows: a fine speckle of sky-blue /
                            // sun-white / black across the whole part, which
                            // reads as shattered geometry rather than a
                            // lighting bug. Anchor to the undeformed normal's
                            // hemisphere so orientation is winding-independent.
                            flipped += 1;
                            v = -v;
                        }

                        [v.x, v.y, v.z]
                    })
                    .collect();

                // One line per deformation, not per frame — `dirty` gates this
                // whole system. Tells us which branch actually fired instead of
                // leaving it to inference.
                let recomputed = near_moved.iter().filter(|b| **b).count();
                if recomputed > 0 {
                    info!(
                        "deform-normals: {} of {} vertices in dented region ({} flipped, {} degenerate); \
                         rest keep authored normals",
                        recomputed,
                        acc.len(),
                        flipped,
                        fellback
                    );
                }

                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, out);
            }
        }

        // Positions are written LAST, after the normal pass above has read them.
        // Writing them first moves the buffer into the mesh, so the normal pass
        // is left borrowing a moved value. Cloning would also compile, but this
        // is a per-deformation vertex buffer and the reorder costs nothing: the
        // normal pass only reads `indices` and `primitive_topology`, neither of
        // which depends on the new positions being in the mesh yet.
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            VertexAttributeValues::Float32x3(new_positions),
        );

        deform_mesh.dirty = false;
    }
}

// ============================================================================
// Fracture Mesh
// ============================================================================

/// Diagnostic trace for fracture events.
///
/// The actual split — cutting the mesh and spawning the two halves as
/// independent dynamic bodies — is performed engine-side by
/// `eustress_engine::physics::fracture_bridge`, because it needs Avian
/// (colliders, rigid bodies, velocities) and `eustress-common` only carries
/// avian3d as an optional dependency. The geometry math itself lives in
/// [`super::fracture_mesh::split_mesh_by_plane`] and is pure, so a host
/// without physics can still cut meshes; it just has nothing to spawn them
/// into.
pub fn handle_fracture_mesh(mut events: MessageReader<FractureMeshEvent>) {
    for event in events.read() {
        tracing::debug!(
            "Fracture event on entity {:?} at {:?} along {:?} (energy {:.3})",
            event.entity,
            event.origin,
            event.direction,
            event.energy
        );
    }
}

// NOTE: a private `split_mesh_by_plane` stub used to live here returning
// `(None, None)`, shadowing the REAL implementation in
// `super::fracture_mesh`. It had no callers and guaranteed that anything
// wired to it could never fracture. Removed — use
// `fracture_mesh::split_mesh_by_plane`.

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_with(positions: Vec<Vec3>, normals: Vec<Vec3>) -> DeformableMesh {
        DeformableMesh {
            original_positions: positions,
            original_normals: normals,
            ..Default::default()
        }
    }

    /// The dent has to aim along the SURFACE normal, and finding that normal
    /// has to measure distance in world metres.
    ///
    /// On a flat, non-uniformly scaled plate the local-space nearest vertex is
    /// not the world-space nearest one: local Y is compressed 6x relative to X
    /// on a 3.0 x 0.5 x 2.5 part, so a naive local-space search picks a vertex
    /// on the far rim of the struck face — or worse, one on the opposite face —
    /// and aims the dent sideways, which is exactly the tangential displacement
    /// that folds the surface.
    #[test]
    fn surface_normal_is_taken_in_world_space() {
        let size = Vec3::new(3.0, 0.5, 2.5);
        // Top-face vertex directly under the impact, and a bottom-face vertex
        // that is CLOSER in raw local units but far away in metres.
        let m = mesh_with(
            vec![
                Vec3::new(0.02, 0.5, 0.0),  // top, 0.06 m away in world
                Vec3::new(0.0, -0.5, 0.0),  // bottom, 1.0 local but 0.5 m away
            ],
            vec![Vec3::Y, Vec3::NEG_Y],
        );

        let n = nearest_surface_normal(&m, Vec3::new(0.0, 0.5, 0.0), size)
            .expect("authored normals present");
        assert_eq!(n, Vec3::Y, "picked the wrong face's normal");
    }

    #[test]
    fn surface_normal_declines_without_authored_normals() {
        let m = mesh_with(vec![Vec3::Y * 0.5], Vec::new());
        assert!(
            nearest_surface_normal(&m, Vec3::ZERO, Vec3::ONE).is_none(),
            "must fall back rather than invent a normal"
        );
    }

    /// A blow travelling ALONG the surface must not dent it, and a square blow
    /// must dent at full depth. This is the projection that removes the
    /// tangential component responsible for folding.
    #[test]
    fn obliquity_scales_from_square_to_grazing() {
        let into = Vec3::NEG_Y; // dent direction for a top-face hit
        let square = Vec3::NEG_Y.dot(into).clamp(0.0, 1.0);
        let angled = Vec3::new(0.6, -0.8, 0.0).normalize().dot(into).clamp(0.0, 1.0);
        let grazing = Vec3::X.dot(into).clamp(0.0, 1.0);

        assert!((square - 1.0).abs() < 1e-6, "square hit must dent fully");
        assert!((angled - 0.8).abs() < 1e-3, "angled hit dents by cos, got {angled}");
        assert_eq!(grazing, 0.0, "a blow along the surface must not dent");
    }
}
