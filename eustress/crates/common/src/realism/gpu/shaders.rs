//! # WGSL Compute Shaders
//!
//! SPH compute shaders for GPU-accelerated fluid simulation.

/// SPH density estimation compute shader
pub const SPH_DENSITY_SHADER: &str = r#"
// SPH Density Estimation Compute Shader
// Calculates density at each particle using Poly6 kernel

struct Particle {
    position: vec3<f32>,
    velocity: vec3<f32>,
    force: vec3<f32>,
    density: f32,
    pressure: f32,
    mass: f32,
    _padding: vec2<f32>,
}

struct SphParams {
    smoothing_length: f32,
    rest_density: f32,
    gas_constant: f32,
    viscosity: f32,
    dt: f32,
    particle_count: u32,
    gravity: vec3<f32>,
}

@group(0) @binding(0)
var<storage, read_write> particles: array<Particle>;

@group(0) @binding(1)
var<uniform> params: SphParams;

// Poly6 kernel: W(r, h) = (315 / 64πh⁹) * (h² - r²)³
fn poly6_kernel(r: f32, h: f32) -> f32 {
    if (r > h) {
        return 0.0;
    }
    let h2 = h * h;
    let r2 = r * r;
    let diff = h2 - r2;
    let coeff = 315.0 / (64.0 * 3.14159265359 * pow(h, 9.0));
    return coeff * pow(diff, 3.0);
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    if (idx >= params.particle_count) {
        return;
    }
    
    var p = particles[idx];
    let h = params.smoothing_length;
    
    // Self-contribution
    var density = p.mass * poly6_kernel(0.0, h);
    
    // Neighbor contributions
    for (var j = 0u; j < params.particle_count; j++) {
        if (j == idx) {
            continue;
        }
        
        let neighbor = particles[j];
        let r_vec = p.position - neighbor.position;
        let r = length(r_vec);
        
        if (r < h * 2.0) {
            density += neighbor.mass * poly6_kernel(r, h);
        }
    }
    
    particles[idx].density = max(density, 1.0);
    
    // Calculate pressure using equation of state
    particles[idx].pressure = params.gas_constant * (particles[idx].density - params.rest_density);
    if (particles[idx].pressure < 0.0) {
        particles[idx].pressure = 0.0;
    }
}
"#;

/// SPH force calculation compute shader
pub const SPH_FORCES_SHADER: &str = r#"
// SPH Force Calculation Compute Shader
// Calculates pressure and viscosity forces using Spiky and Viscosity kernels

struct Particle {
    position: vec3<f32>,
    velocity: vec3<f32>,
    force: vec3<f32>,
    density: f32,
    pressure: f32,
    mass: f32,
    _padding: vec2<f32>,
}

struct SphParams {
    smoothing_length: f32,
    rest_density: f32,
    gas_constant: f32,
    viscosity: f32,
    dt: f32,
    particle_count: u32,
    gravity: vec3<f32>,
}

@group(0) @binding(0)
var<storage, read_write> particles: array<Particle>;

@group(0) @binding(1)
var<uniform> params: SphParams;

// Spiky kernel gradient: ∇W(r, h) = -(45 / πh⁶) * (h - r)² * r̂
fn spiky_gradient(r_vec: vec3<f32>, h: f32) -> vec3<f32> {
    let r = length(r_vec);
    if (r > h || r < 0.0001) {
        return vec3<f32>(0.0);
    }
    let diff = h - r;
    let coeff = -45.0 / (3.14159265359 * pow(h, 6.0));
    return coeff * pow(diff, 2.0) / r * r_vec;
}

// Viscosity kernel Laplacian: ∇²W(r, h) = (45 / πh⁶) * (h - r)
fn viscosity_laplacian(r: f32, h: f32) -> f32 {
    if (r > h) {
        return 0.0;
    }
    let coeff = 45.0 / (3.14159265359 * pow(h, 6.0));
    return coeff * (h - r);
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    if (idx >= params.particle_count) {
        return;
    }
    
    var p = particles[idx];
    let h = params.smoothing_length;
    
    var f_pressure = vec3<f32>(0.0);
    var f_viscosity = vec3<f32>(0.0);
    
    if (p.density < 0.0001) {
        return;
    }
    
    // Calculate forces from neighbors
    for (var j = 0u; j < params.particle_count; j++) {
        if (j == idx) {
            continue;
        }
        
        let neighbor = particles[j];
        if (neighbor.density < 0.0001) {
            continue;
        }
        
        let r_vec = p.position - neighbor.position;
        let r = length(r_vec);
        
        if (r < h * 2.0 && r > 0.0001) {
            // Pressure force (symmetric formulation)
            let pressure_term = (p.pressure / (p.density * p.density)) 
                              + (neighbor.pressure / (neighbor.density * neighbor.density));
            let grad_w = spiky_gradient(r_vec, h);
            f_pressure -= neighbor.mass * pressure_term * grad_w;
            
            // Viscosity force
            let lap_w = viscosity_laplacian(r, h);
            let vel_diff = neighbor.velocity - p.velocity;
            f_viscosity += (neighbor.mass / neighbor.density) * vel_diff * lap_w;
        }
    }
    
    f_pressure *= p.density;
    f_viscosity *= params.viscosity;
    
    // Gravity
    let f_gravity = p.mass * params.gravity;
    
    // Total force
    particles[idx].force = f_pressure + f_viscosity + f_gravity;
}
"#;

/// SPH integration compute shader
pub const SPH_INTEGRATE_SHADER: &str = r#"
// SPH Integration Compute Shader
// Updates velocity and position using semi-implicit Euler

struct Particle {
    position: vec3<f32>,
    velocity: vec3<f32>,
    force: vec3<f32>,
    density: f32,
    pressure: f32,
    mass: f32,
    _padding: vec2<f32>,
}

struct SphParams {
    smoothing_length: f32,
    rest_density: f32,
    gas_constant: f32,
    viscosity: f32,
    dt: f32,
    particle_count: u32,
    gravity: vec3<f32>,
}

struct Bounds {
    min: vec3<f32>,
    max: vec3<f32>,
    damping: f32,
}

@group(0) @binding(0)
var<storage, read_write> particles: array<Particle>;

@group(0) @binding(1)
var<uniform> params: SphParams;

@group(0) @binding(2)
var<uniform> bounds: Bounds;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    if (idx >= params.particle_count) {
        return;
    }
    
    var p = particles[idx];
    
    // Calculate acceleration
    var acceleration = vec3<f32>(0.0);
    if (p.mass > 0.0001) {
        acceleration = p.force / p.mass;
    }
    
    // Semi-implicit Euler integration
    p.velocity += acceleration * params.dt;
    p.position += p.velocity * params.dt;
    
    // Boundary conditions (reflect with damping)
    if (p.position.x < bounds.min.x) {
        p.position.x = bounds.min.x;
        p.velocity.x *= -bounds.damping;
    }
    if (p.position.x > bounds.max.x) {
        p.position.x = bounds.max.x;
        p.velocity.x *= -bounds.damping;
    }
    if (p.position.y < bounds.min.y) {
        p.position.y = bounds.min.y;
        p.velocity.y *= -bounds.damping;
    }
    if (p.position.y > bounds.max.y) {
        p.position.y = bounds.max.y;
        p.velocity.y *= -bounds.damping;
    }
    if (p.position.z < bounds.min.z) {
        p.position.z = bounds.min.z;
        p.velocity.z *= -bounds.damping;
    }
    if (p.position.z > bounds.max.z) {
        p.position.z = bounds.max.z;
        p.velocity.z *= -bounds.damping;
    }
    
    // Clear force for next frame
    p.force = vec3<f32>(0.0);
    
    particles[idx] = p;
}
"#;

/// Neighbor grid construction shader (spatial hashing)
pub const NEIGHBOR_GRID_SHADER: &str = r#"
// Spatial Hash Grid Construction
// Assigns particles to grid cells for O(1) neighbor lookup

struct Particle {
    position: vec3<f32>,
    velocity: vec3<f32>,
    force: vec3<f32>,
    density: f32,
    pressure: f32,
    mass: f32,
    _padding: vec2<f32>,
}

struct GridParams {
    cell_size: f32,
    grid_dim: vec3<u32>,
    particle_count: u32,
}

@group(0) @binding(0)
var<storage, read> particles: array<Particle>;

@group(0) @binding(1)
var<uniform> grid_params: GridParams;

@group(0) @binding(2)
var<storage, read_write> cell_counts: array<atomic<u32>>;

@group(0) @binding(3)
var<storage, read_write> particle_cells: array<u32>;

fn position_to_cell(pos: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(
        u32(max(0.0, pos.x / grid_params.cell_size)),
        u32(max(0.0, pos.y / grid_params.cell_size)),
        u32(max(0.0, pos.z / grid_params.cell_size))
    );
}

fn cell_to_index(cell: vec3<u32>) -> u32 {
    let clamped = min(cell, grid_params.grid_dim - vec3<u32>(1u));
    return clamped.x + clamped.y * grid_params.grid_dim.x 
         + clamped.z * grid_params.grid_dim.x * grid_params.grid_dim.y;
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    if (idx >= grid_params.particle_count) {
        return;
    }
    
    let p = particles[idx];
    let cell = position_to_cell(p.position);
    let cell_idx = cell_to_index(cell);
    
    // Increment cell count atomically
    atomicAdd(&cell_counts[cell_idx], 1u);
    
    // Store particle's cell index
    particle_cells[idx] = cell_idx;
}
"#;
