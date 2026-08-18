//! # Deformation Components
//!
//! ECS components for mesh deformation state.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// DeformableMesh Component
// ============================================================================

/// Marks an entity as having deformable mesh vertices
/// Added automatically when BasePart.destructible = true
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct DeformableMesh {
    /// Handle of the shared, undeformed SOURCE mesh this part was built from.
    ///
    /// Read-only provenance: it is what `Mesh3d` is restored to when
    /// deformation is torn down (play-stop / `deformation = false`). The
    /// per-frame reference pose is [`original_positions`](Self::original_positions),
    /// NOT this asset — several parts share one cached source mesh.
    pub original_mesh: Handle<Mesh>,
    /// Per-entity writable mesh asset. `Mesh3d` points here while deformable.
    pub deformed_mesh: Handle<Mesh>,
    /// Undeformed vertex positions, captured once at init.
    ///
    /// Holding the reference pose in the component (rather than reading it
    /// back out of a mesh asset) is what makes every vertex write ABSOLUTE —
    /// `original[i] + displacement[i]` — so displacement can never compound
    /// frame over frame into unbounded drift, no matter what the dirty flag
    /// does. It also removes a full position-buffer clone from every write.
    pub original_positions: Vec<Vec3>,
    /// Undeformed vertex normals, captured alongside the positions.
    ///
    /// Used as the FALLBACK when a recomputed normal comes out degenerate.
    /// Relying on `Mesh::compute_normals` alone produced visibly shattered
    /// shading — a single-coloured part rendering as a patchwork of sky-blue,
    /// sun-white and black shards, because any vertex whose accumulated face
    /// normal cancels to ~zero normalizes to NaN and the shader then lights
    /// that triangle from an arbitrary direction.
    pub original_normals: Vec<Vec3>,
    /// Number of vertices
    pub vertex_count: usize,
    /// Whether mesh needs GPU sync
    pub dirty: bool,
    /// Deformation quality level (affects vertex update frequency)
    pub quality: DeformationQuality,
}

impl Default for DeformableMesh {
    fn default() -> Self {
        Self {
            original_mesh: Handle::default(),
            deformed_mesh: Handle::default(),
            original_positions: Vec::new(),
            original_normals: Vec::new(),
            vertex_count: 0,
            dirty: false,
            quality: DeformationQuality::Medium,
        }
    }
}

/// The persistent adaptive-refinement structure for one deformable mesh.
///
/// Impacts are REPEATED — a bouncing impactor refines the same part again and
/// again — and this is what makes that stable. Rendering needs a green closure
/// (thin transition triangles stitching a refined region to its coarser
/// neighbours), and those must never be refined further: they are slivers by
/// construction. Keeping the tree between impacts means the closure is
/// re-derived from the leaves each time and never becomes structure, so what
/// gets split is always the well-shaped red subdivision.
///
/// Deliberately NOT `Reflect`: this is derived runtime state rebuilt from the
/// authored mesh on the next Play, never authored or saved.
#[derive(Component, Default)]
pub struct MeshRefinement(pub super::vertex::RefineForest);

/// Marks a part that wants deformation (`BasePart.destructible = true`) but
/// whose mesh asset had not finished loading when
/// [`init_deformable_meshes`](super::systems::init_deformable_meshes) first
/// saw it.
///
/// The init system is driven by `Changed<BasePart>` (a deliberate perf choice
/// — see that system's docs), which fires ONCE. A GLB-backed part whose mesh
/// is still streaming in at that moment would therefore never become
/// deformable: the change tick is spent and nothing re-triggers it. This
/// marker adds the entity to the init query's second `Or` arm so it is
/// retried each frame until its mesh resolves, then removed. Cold streamed
/// scenery never receives it (those early-out on `deformation == false`), so
/// the O(N) cold-part scan the `Changed` filter avoids stays avoided.
#[derive(Component, Reflect, Clone, Copy, Debug, Default)]
#[reflect(Component)]
pub struct DeformInitPending;

/// Deformation quality settings
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum DeformationQuality {
    /// Update every frame, full precision
    High,
    /// Update every 2 frames
    #[default]
    Medium,
    /// Update every 4 frames
    Low,
    /// GPU compute shader (best for large meshes)
    Gpu,
}

// ============================================================================
// VertexDisplacements Component
// ============================================================================

/// Per-vertex displacement accumulators for a [`DeformableMesh`].
///
/// NOTE ON THE NAME: this was called `DeformationState`, which collided with
/// the *other* `DeformationState` component in
/// [`crate::realism::materials::deformation`] — a `StrainTensor`-based
/// physical state. Both were glob-re-exported from `realism::prelude`, so a
/// `use realism::prelude::*` silently resolved `DeformationState` to the
/// materials type and any code trying to reach this one got a component the
/// vertex pipeline never writes. The two are pipeline STAGES, not duplicates:
/// the materials type is the physical stress/strain state, this is the
/// render-side displacement cache a full pipeline would derive from it.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct VertexDisplacements {
    /// Elastic displacement (recoverable)
    pub elastic_displacement: Vec<Vec3>,
    /// Plastic displacement (permanent)
    pub plastic_displacement: Vec<Vec3>,
    /// Thermal displacement
    pub thermal_displacement: Vec<Vec3>,
    /// Total displacement (sum of all)
    pub total_displacement: Vec<Vec3>,
    /// Maximum elastic strain before plastic deformation
    pub yield_strain: f32,
    /// Current maximum displacement magnitude
    pub max_displacement: f32,
    /// Largest SQUARED elastic displacement magnitude, refreshed by
    /// [`update_total`](Self::update_total).
    ///
    /// Lets the elastic-relaxation system reject a settled part in O(1)
    /// instead of scanning every vertex every frame just to discover there is
    /// nothing to relax. Squared to keep the hot loop free of `sqrt`.
    pub max_elastic_sq: f32,
    /// Reference temperature for thermal expansion (K)
    pub reference_temperature: f32,
    /// Thermal expansion coefficient (1/K)
    pub thermal_expansion_coeff: f32,
    /// Enable plastic (permanent) deformation
    pub allow_plastic: bool,
    /// Enable thermal deformation
    pub allow_thermal: bool,
}

impl Default for VertexDisplacements {
    fn default() -> Self {
        Self {
            elastic_displacement: Vec::new(),
            plastic_displacement: Vec::new(),
            thermal_displacement: Vec::new(),
            total_displacement: Vec::new(),
            yield_strain: 0.002, // 0.2% typical for steel
            max_displacement: 0.0,
            max_elastic_sq: 0.0,
            reference_temperature: 293.15, // 20°C
            thermal_expansion_coeff: 12e-6, // Steel
            allow_plastic: true,
            allow_thermal: true,
        }
    }
}

impl VertexDisplacements {
    /// Initialize for given vertex count
    pub fn init(&mut self, vertex_count: usize) {
        self.elastic_displacement = vec![Vec3::ZERO; vertex_count];
        self.plastic_displacement = vec![Vec3::ZERO; vertex_count];
        self.thermal_displacement = vec![Vec3::ZERO; vertex_count];
        self.total_displacement = vec![Vec3::ZERO; vertex_count];
    }
    
    /// Append entries for vertices created by adaptive refinement.
    ///
    /// `parents` is [`Refinement::added_parents`](super::vertex::Refinement),
    /// so each new vertex is the midpoint of an edge between two vertices that
    /// already existed. It inherits the AVERAGE of their displacement rather
    /// than zero: refinement can land inside a crater a previous impact already
    /// dented, and a zeroed midpoint there would sit on the undeformed surface
    /// and punch a spike straight through the dent.
    ///
    /// Parents are always older than the vertex they produce, so a midpoint of
    /// a midpoint reads values this loop has already appended.
    pub fn grow_interpolated(&mut self, parents: &[(u32, u32)]) {
        self.elastic_displacement.reserve(parents.len());
        self.plastic_displacement.reserve(parents.len());
        self.thermal_displacement.reserve(parents.len());
        self.total_displacement.reserve(parents.len());

        for &(a, b) in parents {
            let (ia, ib) = (a as usize, b as usize);
            let push_mid = |v: &mut Vec<Vec3>| {
                let x = v.get(ia).copied().unwrap_or(Vec3::ZERO);
                let y = v.get(ib).copied().unwrap_or(Vec3::ZERO);
                v.push((x + y) * 0.5);
            };
            push_mid(&mut self.elastic_displacement);
            push_mid(&mut self.plastic_displacement);
            push_mid(&mut self.thermal_displacement);
            push_mid(&mut self.total_displacement);
        }
    }

    /// Update total displacement from components
    pub fn update_total(&mut self) {
        self.max_displacement = 0.0;
        self.max_elastic_sq = 0.0;

        for i in 0..self.total_displacement.len() {
            self.total_displacement[i] = self.elastic_displacement[i]
                + self.plastic_displacement[i]
                + self.thermal_displacement[i];

            let mag = self.total_displacement[i].length();
            if mag > self.max_displacement {
                self.max_displacement = mag;
            }

            // Tracked in the loop that already runs, so the relaxation system
            // can skip settled parts without a second full scan.
            let elastic_sq = self.elastic_displacement[i].length_squared();
            if elastic_sq > self.max_elastic_sq {
                self.max_elastic_sq = elastic_sq;
            }
        }
    }
    
    /// Apply elastic displacement at vertex
    pub fn apply_elastic(&mut self, vertex_idx: usize, displacement: Vec3) {
        if vertex_idx < self.elastic_displacement.len() {
            self.elastic_displacement[vertex_idx] += displacement;
        }
    }
    
    /// Convert elastic to plastic if exceeds yield
    pub fn check_yield(&mut self, vertex_idx: usize, size: Vec3) {
        if !self.allow_plastic || vertex_idx >= self.elastic_displacement.len() {
            return;
        }
        
        let elastic = self.elastic_displacement[vertex_idx];
        let strain = elastic.length() / size.min_element().max(0.001);
        
        if strain > self.yield_strain {
            // Transfer excess to plastic
            let excess_ratio = (strain - self.yield_strain) / strain;
            let plastic_part = elastic * excess_ratio;
            
            self.plastic_displacement[vertex_idx] += plastic_part;
            self.elastic_displacement[vertex_idx] -= plastic_part;
        }
    }
    
    /// Apply thermal displacement based on temperature delta
    pub fn apply_thermal(&mut self, vertex_idx: usize, position: Vec3, temperature: f32) {
        if !self.allow_thermal || vertex_idx >= self.thermal_displacement.len() {
            return;
        }
        
        let delta_t = temperature - self.reference_temperature;
        let strain = self.thermal_expansion_coeff * delta_t;
        
        // Thermal expansion is radial from center
        self.thermal_displacement[vertex_idx] = position * strain;
    }
    
    /// Reset elastic deformation (spring back)
    pub fn reset_elastic(&mut self, damping: f32) {
        for disp in &mut self.elastic_displacement {
            *disp *= 1.0 - damping;
        }
    }
    
    /// Get total displacement at vertex
    pub fn get_displacement(&self, vertex_idx: usize) -> Vec3 {
        self.total_displacement.get(vertex_idx).copied().unwrap_or(Vec3::ZERO)
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event triggered when mesh should fracture
#[derive(Message, Clone, Debug)]
pub struct FractureMeshEvent {
    /// Entity to fracture
    pub entity: Entity,
    /// Fracture plane origin
    pub origin: Vec3,
    /// Fracture plane normal
    pub normal: Vec3,
    /// Crack propagation direction
    pub direction: Vec3,
    /// Fracture energy
    pub energy: f32,
}

/// Event triggered on impact deformation
#[derive(Message, Clone, Debug)]
pub struct ImpactDeformEvent {
    /// Entity that was impacted
    pub entity: Entity,
    /// Impact point in local space
    pub point: Vec3,
    /// Impact force vector
    pub force: Vec3,
    /// Impact radius
    pub radius: f32,
    /// Is this a permanent (plastic) deformation
    pub permanent: bool,
}

// ============================================================================
// Vertex Influence
// ============================================================================

/// Per-vertex influence data for deformation
#[derive(Clone, Debug, Default)]
pub struct VertexInfluence {
    /// Stress contribution to this vertex
    pub stress: Vec3,
    /// Temperature at this vertex
    pub temperature: f32,
    /// Distance from impact point (if any)
    pub impact_distance: f32,
    /// Bone/joint weights (for skeletal deformation)
    pub bone_weights: [f32; 4],
    /// Bone/joint indices
    pub bone_indices: [u32; 4],
}

/// Deformation configuration resource
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct DeformationConfig {
    /// Master switch for the whole vertex-deformation pipeline.
    ///
    /// The engine drives this from play state: deformation is a RUNTIME
    /// effect, so it stays off while editing (physics is paused there anyway)
    /// and is switched on when Play starts. Keeping the gate as plain resource
    /// state rather than a `PlayModeState` run-condition is deliberate —
    /// `eustress-common` must not depend on the engine's state enum, and
    /// headless/other hosts can drive the same switch.
    pub enabled: bool,
    /// Global deformation scale
    pub scale: f32,
    /// Maximum displacement as fraction of mesh size
    pub max_displacement_ratio: f32,
    /// Elastic spring constant (stiffness)
    pub stiffness: f32,
    /// Elastic recovery RATE, in units of 1/second.
    ///
    /// Elastic displacement decays as `exp(-damping · dt)`, so this is
    /// frame-rate independent: ~5.0 relaxes a dent by 95% in about 0.6 s.
    /// (The field previously went unread — nothing ever relaxed elastic
    /// deformation — and its old `0.1` default was written for a per-FRAME
    /// `disp *= 1 - damping` interpretation, which as a per-second rate would
    /// take minutes to visibly recover.)
    pub damping: f32,
    /// Enable GPU compute for large meshes
    pub use_gpu: bool,
    /// Vertex count threshold for GPU compute
    pub gpu_threshold: usize,
    /// Update frequency (frames between updates)
    pub update_interval: u32,
    /// COARSEST edge length (metres) refinement will settle for.
    ///
    /// Deformation moves existing vertices and cannot create new ones, so an
    /// authored 24-vertex cube — every vertex at a corner — has nothing to
    /// displace under a localized impact and looks totally unreactive. The mesh
    /// is refined on impact until the crater is spanned by triangles roughly
    /// this long or shorter.
    ///
    /// The actual target is normally FINER than this: it is derived from the
    /// crater radius (see `MIN_REFINE_EDGE_M` and the derivation in
    /// [`apply_impact_deformation`](super::systems::apply_impact_deformation)),
    /// and this value only caps how coarse a very large crater is allowed to
    /// get.
    pub target_edge_m: f32,
    /// Hard cap on refinement depth. Each level quarters the edge length of the
    /// triangles it touches, so this bounds a small crater on a huge part from
    /// recursing indefinitely.
    ///
    /// Higher than it would have to be for uniform subdivision, because the
    /// cost here is LOCAL: only the triangles the crater actually covers reach
    /// the deepest levels.
    pub max_subdivision_levels: u32,
    /// Triangle budget for one deformable part.
    ///
    /// Refinement stops adding detail once it would exceed this, and refuses
    /// outright if it cannot finish enforcing conformity inside it — a
    /// half-balanced mesh renders with cracks, which is worse than a slightly
    /// coarse dent.
    pub max_triangles: usize,
}

impl Default for DeformationConfig {
    fn default() -> Self {
        Self {
            // On by default so a common-only / headless host gets working
            // deformation without extra wiring; the engine explicitly clears
            // it for Edit mode at startup.
            enabled: true,
            scale: 1.0,
            max_displacement_ratio: 0.1, // 10% of mesh size
            stiffness: 1000.0,
            damping: 5.0,
            use_gpu: true,
            gpu_threshold: 10000,
            update_interval: 1,
            target_edge_m: 0.12,
            max_subdivision_levels: 8,
            max_triangles: 8000,
        }
    }
}
