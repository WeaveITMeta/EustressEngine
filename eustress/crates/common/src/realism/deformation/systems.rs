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
    config: Res<DeformationConfig>,
) {
    for (entity, base_part, mesh3d) in query.iter() {
        if !base_part.deformation {
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

        // Subdivide first. An authored cube has 24 vertices, ALL at corners —
        // a dent in the middle of a face would have nothing within its radius
        // to displace, so the part would look completely unreactive no matter
        // how correct the contact model is. Subdividing is what gives the
        // deformation somewhere to push; it is planar, so the shape is
        // unchanged.
        let levels = super::vertex::subdivision_levels_for(
            base_part.size,
            config.target_edge_m,
            config.max_subdivision_levels,
        );
        let subdivided = super::vertex::subdivide_mesh(source_mesh, levels);
        let source_mesh: &Mesh = subdivided.as_ref().unwrap_or(source_mesh);

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
        if !base_part.deformation {
            commands
                .entity(entity)
                .insert(Mesh3d(deform_mesh.original_mesh.clone()))
                .remove::<DeformableMesh>()
                .remove::<VertexDisplacements>();
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
        if !base_part.deformation {
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
        if !base_part.deformation || !deform_state.allow_thermal {
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

/// Apply deformation from impact events
pub fn apply_impact_deformation(
    mut events: MessageReader<ImpactDeformEvent>,
    mut query: Query<(&BasePart, &mut VertexDisplacements, &mut DeformableMesh)>,
    config: Res<DeformationConfig>,
) {
    for event in events.read() {
        let Ok((base_part, mut deform_state, mut deform_mesh)) = query.get_mut(event.entity) else {
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
        let peak_m = event.force.length() * config.scale;

        // Metres travelled per unit of local displacement along the dent
        // direction, used to convert the depth back into local units.
        let size_along = (direction * size).length().max(1.0e-6);

        // Never let one impact (or an accumulation of them) turn the part
        // inside out. Clamp in METRES, then convert.
        let max_depth_m = size.min_element() * config.max_displacement_ratio;
        let peak_m = peak_m.min(max_depth_m);
        let peak_local = peak_m / size_along;
        let max_disp_local = max_depth_m / size_along;

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

            let falloff = 1.0 - (dist / event.radius);
            let displacement = direction * (peak_local * falloff);

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

                for tri in indices.chunks_exact(3) {
                    let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                    if a >= n_verts || b >= n_verts || c >= n_verts {
                        continue;
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
                if flipped > 0 || fellback > 0 {
                    info!(
                        "deform-normals: {} of {} flipped (winding), {} fell back (degenerate)",
                        flipped,
                        acc.len(),
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
