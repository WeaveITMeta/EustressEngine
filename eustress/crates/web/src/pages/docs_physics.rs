// =============================================================================
// Eustress Web - Physics Documentation Page
// =============================================================================
// Comprehensive physics documentation with floating TOC
// Covers: Realism Physics System, Materials, Fluids, Deformation, GPU, Quantum
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Table of Contents Data
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct TocSection {
    id: &'static str,
    title: &'static str,
    subsections: Vec<TocSubsection>,
}

#[derive(Clone, Debug, PartialEq)]
struct TocSubsection {
    id: &'static str,
    title: &'static str,
}

fn get_toc() -> Vec<TocSection> {
    vec![
        TocSection {
            id: "overview",
            title: "Overview",
            subsections: vec![
                TocSubsection { id: "overview-intro", title: "Introduction" },
                TocSubsection { id: "overview-architecture", title: "Architecture" },
                TocSubsection { id: "overview-features", title: "Feature Flags" },
            ],
        },
        TocSection {
            id: "fundamentals",
            title: "Fundamental Laws",
            subsections: vec![
                TocSubsection { id: "fundamentals-thermo", title: "Thermodynamics" },
                TocSubsection { id: "fundamentals-mechanics", title: "Mechanics" },
                TocSubsection { id: "fundamentals-conservation", title: "Conservation Laws" },
            ],
        },
        TocSection {
            id: "particles",
            title: "Particle Systems",
            subsections: vec![
                TocSubsection { id: "particles-components", title: "ECS Components" },
                TocSubsection { id: "particles-spawning", title: "Spawning" },
                TocSubsection { id: "particles-spatial", title: "Spatial Queries" },
            ],
        },
        TocSection {
            id: "materials",
            title: "Materials Science",
            subsections: vec![
                TocSubsection { id: "materials-properties", title: "Material Properties" },
                TocSubsection { id: "materials-stress", title: "Stress & Strain" },
                TocSubsection { id: "materials-fracture", title: "Fracture Mechanics" },
            ],
        },
        TocSection {
            id: "fluids",
            title: "Fluid Dynamics",
            subsections: vec![
                TocSubsection { id: "fluids-sph", title: "SPH Simulation" },
                TocSubsection { id: "fluids-water", title: "Water Physics" },
                TocSubsection { id: "fluids-aero", title: "Aerodynamics" },
            ],
        },
        TocSection {
            id: "deformation",
            title: "Mesh Deformation",
            subsections: vec![
                TocSubsection { id: "deformation-enable", title: "Enabling Deformation" },
                TocSubsection { id: "deformation-stress", title: "Stress-Based" },
                TocSubsection { id: "deformation-thermal", title: "Thermal Warping" },
                TocSubsection { id: "deformation-impact", title: "Impact Deformation" },
                TocSubsection { id: "deformation-fracture", title: "Fracture Splitting" },
            ],
        },
        TocSection {
            id: "gpu",
            title: "GPU Acceleration",
            subsections: vec![
                TocSubsection { id: "gpu-sph", title: "GPU SPH" },
                TocSubsection { id: "gpu-shaders", title: "Compute Shaders" },
                TocSubsection { id: "gpu-performance", title: "Performance" },
            ],
        },
        TocSection {
            id: "quantum",
            title: "Quantum Effects",
            subsections: vec![
                TocSubsection { id: "quantum-statistics", title: "Quantum Statistics" },
                TocSubsection { id: "quantum-bec", title: "Bose-Einstein Condensates" },
            ],
        },
        TocSection {
            id: "api",
            title: "API Reference",
            subsections: vec![
                TocSubsection { id: "api-constants", title: "Physical Constants" },
                TocSubsection { id: "api-units", title: "Unit System" },
                TocSubsection { id: "api-components", title: "Components" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Physics documentation page with floating TOC.
#[component]
pub fn DocsPhysicsPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());
    
    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />
            
            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-physics"></div>
            </div>
            
            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/physics.svg" alt="Physics" class="toc-icon" />
                        <h2>"Physics"</h2>
                    </div>
                    <nav class="toc-nav">
                        {get_toc().into_iter().map(|section| {
                            let section_id = section.id.to_string();
                            let is_active = {
                                let section_id = section_id.clone();
                                move || active_section.get() == section_id
                            };
                            view! {
                                <div class="toc-section">
                                    <a 
                                        href=format!("#{}", section.id)
                                        class="toc-section-title"
                                        class:active=is_active
                                    >
                                        {section.title}
                                    </a>
                                    <div class="toc-subsections">
                                        {section.subsections.into_iter().map(|sub| {
                                            view! {
                                                <a href=format!("#{}", sub.id) class="toc-subsection">
                                                    {sub.title}
                                                </a>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </nav>
                    
                    <div class="toc-footer">
                        <a href="/learn" class="toc-back">
                            <img src="/assets/icons/arrow-left.svg" alt="Back" />
                            "Back to Learn"
                        </a>
                    </div>
                </aside>
                
                // Main Content
                <main class="docs-content">
                    // Hero
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Physics"</span>
                        </div>
                        <h1 class="docs-title">"Physics System"</h1>
                        <p class="docs-subtitle">
                            "Physically accurate simulations grounded in fundamental laws of physics. 
                            From thermodynamics to fluid dynamics, materials science to quantum effects."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "45 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/calendar.svg" alt="Updated" />
                                "Updated Jan 2026"
                            </span>
                        </div>
                    </header>
                    
                    // =========================================================
                    // OVERVIEW SECTION
                    // =========================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Overview"
                        </h2>
                        
                        <div id="overview-intro" class="subsection">
                            <h3>"Introduction"</h3>
                            <p>
                                "The Eustress Physics System provides a comprehensive, physically accurate 
                                simulation framework built on Bevy ECS and Avian3D. Unlike simple game physics, 
                                this system implements real physics equations for thermodynamics, materials science, 
                                fluid dynamics, and even quantum effects."
                            </p>
                            
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/thermometer.svg" alt="Thermodynamics" />
                                    </div>
                                    <h4>"Thermodynamics"</h4>
                                    <p>"PV=nRT, heat transfer, phase transitions"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/cube.svg" alt="Materials" />
                                    </div>
                                    <h4>"Materials Science"</h4>
                                    <p>"Stress tensors, fracture mechanics, plasticity"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/droplet.svg" alt="Fluids" />
                                    </div>
                                    <h4>"Fluid Dynamics"</h4>
                                    <p>"SPH simulation, Navier-Stokes, aerodynamics"</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/gpu.svg" alt="GPU" />
                                    </div>
                                    <h4>"GPU Acceleration"</h4>
                                    <p>"WGPU compute shaders for 1M+ particles"</p>
                                </div>
                            </div>
                        </div>
                        
                        <div id="overview-architecture" class="subsection">
                            <h3>"Architecture"</h3>
                            <p>
                                "The physics system is organized into modular subsystems that can be 
                                enabled independently via feature flags:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Architecture"</span>
                                </div>
                                <pre><code class="language-text">{r#"realism/
├── constants.rs      # Physical constants (R, G, k_B, σ)
├── units.rs          # SI unit system with conversions
├── laws/             # Thermodynamics, mechanics, conservation
├── particles/        # High-performance particle ECS
├── materials/        # Stress, strain, fracture mechanics
├── fluids/           # SPH, water, aerodynamics, buoyancy
├── deformation/      # Vertex-level mesh deformation
├── gpu/              # WGPU compute shaders
├── quantum/          # Bose-Einstein, Fermi-Dirac
├── symbolic/         # Symbolica equation solving
└── scripting/        # Rune dynamic physics scripts"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="overview-features" class="subsection">
                            <h3>"Feature Flags"</h3>
                            <p>
                                "Enable only the physics features you need to optimize compile times and binary size:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Cargo.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[features]
# Core physics (always included with realism)
realism = ["physics"]

# Symbolic math with Symbolica 1.0+
realism-symbolic = ["realism", "symbolica"]

# Rune scripting for dynamic physics
realism-scripting = ["realism", "rune", "rune-modules"]

# GPU compute for SPH (WGPU)
realism-gpu = ["realism", "wgpu", "encase"]

# Advanced spatial queries (Kiddo KD-trees)
realism-spatial = ["realism", "kiddo", "rkyv"]

# Quantum effects (Bose-Einstein, Fermi-Dirac)
realism-quantum = ["realism-symbolic"]

# Everything enabled
realism-full = ["realism-symbolic", "realism-scripting", 
                "realism-spatial", "realism-gpu"]"#}</code></pre>
                            </div>
                            
                            <div class="callout callout-info">
                                <img src="/assets/icons/info.svg" alt="Info" />
                                <div>
                                    <strong>"Quick Start"</strong>
                                    <p>"For most games, use "<code>"realism"</code>" for basic physics or "
                                    <code>"realism-full"</code>" for all features."</p>
                                </div>
                            </div>
                        </div>
                    </section>
                    
                    // =========================================================
                    // FUNDAMENTALS SECTION
                    // =========================================================
                    <section id="fundamentals" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Fundamental Laws"
                        </h2>
                        
                        <div id="fundamentals-thermo" class="subsection">
                            <h3>"Thermodynamics"</h3>
                            <p>
                                "The thermodynamics module implements the four laws of thermodynamics 
                                with real-time simulation capabilities:"
                            </p>
                            
                            <div class="equation-card">
                                <div class="equation">"PV = nRT"</div>
                                <div class="equation-label">"Ideal Gas Law"</div>
                            </div>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress_common::realism::prelude::*;

// Calculate pressure for an ideal gas
let pressure = ideal_gas_pressure(
    1.0,    // n: moles
    300.0,  // T: temperature (K)
    0.001,  // V: volume (m³)
);
// Result: ~2.49 MPa

// Heat transfer between objects
let heat_flow = heat_conduction(
    50.0,   // k: thermal conductivity (W/m·K)
    0.01,   // A: cross-sectional area (m²)
    100.0,  // ΔT: temperature difference (K)
    0.1,    // d: thickness (m)
);
// Result: 5000 W"#}</code></pre>
                            </div>
                            
                            <h4>"Available Functions"</h4>
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Function"</th>
                                            <th>"Description"</th>
                                            <th>"Formula"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"ideal_gas_pressure"</code></td>
                                            <td>"Pressure from ideal gas law"</td>
                                            <td>"P = nRT/V"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"heat_conduction"</code></td>
                                            <td>"Fourier's law heat transfer"</td>
                                            <td>"Q = kA(ΔT/d)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"stefan_boltzmann"</code></td>
                                            <td>"Radiative heat transfer"</td>
                                            <td>"P = εσAT⁴"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"entropy_change"</code></td>
                                            <td>"Entropy for reversible process"</td>
                                            <td>"ΔS = Q/T"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                        
                        <div id="fundamentals-mechanics" class="subsection">
                            <h3>"Mechanics"</h3>
                            <p>
                                "Classical mechanics functions for force, momentum, and energy calculations:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Kinetic energy
let ke = kinetic_energy(10.0, Vec3::new(5.0, 0.0, 0.0));
// Result: 125 J

// Gravitational force between two masses
let force = gravitational_force(5.97e24, 1000.0, 6.371e6);
// Result: ~9810 N (weight on Earth)

// Drag force on moving object
let drag = drag_force(1.225, Vec3::new(30.0, 0.0, 0.0), 0.47, 1.0);
// Result: ~259 N"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="fundamentals-conservation" class="subsection">
                            <h3>"Conservation Laws"</h3>
                            <p>
                                "Automatic conservation tracking for closed systems:"
                            </p>
                            
                            <ul class="feature-list">
                                <li>
                                    <strong>"Mass Conservation"</strong>
                                    " - Total mass remains constant in closed systems"
                                </li>
                                <li>
                                    <strong>"Energy Conservation"</strong>
                                    " - Tracks kinetic, potential, thermal, and internal energy"
                                </li>
                                <li>
                                    <strong>"Momentum Conservation"</strong>
                                    " - Linear and angular momentum preserved in collisions"
                                </li>
                            </ul>
                        </div>
                    </section>
                    
                    // =========================================================
                    // PARTICLES SECTION
                    // =========================================================
                    <section id="particles" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Particle Systems"
                        </h2>
                        
                        <div id="particles-components" class="subsection">
                            <h3>"ECS Components"</h3>
                            <p>
                                "Particles are represented as Bevy entities with specialized components:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Spawn a particle with full physics
commands.spawn((
    Particle {
        mass: 1.0,
        radius: 0.1,
        particle_type: ParticleType::Fluid,
    },
    ThermodynamicState {
        temperature: 300.0,  // Kelvin
        pressure: 101325.0,  // Pascals
        volume: 0.001,       // m³
        internal_energy: 0.0,
        entropy: 0.0,
    },
    KineticState {
        velocity: Vec3::new(1.0, 0.0, 0.0),
        acceleration: Vec3::ZERO,
        momentum: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
    },
    Transform::from_xyz(0.0, 5.0, 0.0),
));"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="particles-spawning" class="subsection">
                            <h3>"Spawning"</h3>
                            <p>
                                "Use the particle spawner for efficient batch creation:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Spawn 10,000 water particles in a cube
let spawner = ParticleSpawner::new()
    .with_count(10_000)
    .with_region(SpawnRegion::Cube {
        center: Vec3::new(0.0, 10.0, 0.0),
        size: Vec3::splat(2.0),
    })
    .with_velocity_range(-0.5..0.5)
    .with_particle_type(ParticleType::Fluid)
    .with_temperature(293.15);  // 20°C

spawner.spawn(&mut commands);"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="particles-spatial" class="subsection">
                            <h3>"Spatial Queries"</h3>
                            <p>
                                "Efficient neighbor queries using spatial hashing or KD-trees:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Using spatial hash grid (O(1) lookup)
let mut grid = SpatialHashGrid::new(cell_size);
grid.insert(entity, position);

let neighbors = grid.query_radius(position, radius);

// Using KD-tree (requires realism-spatial feature)
#[cfg(feature = "realism-spatial")]
{
    use kiddo::KdTree;
    let tree: KdTree<f32, 3> = KdTree::new();
    // ... build tree and query
}"#}</code></pre>
                            </div>
                        </div>
                    </section>
                    
                    // =========================================================
                    // MATERIALS SECTION
                    // =========================================================
                    <section id="materials" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Materials Science"
                        </h2>
                        
                        <div id="materials-properties" class="subsection">
                            <h3>"Material Properties"</h3>
                            <p>
                                "Define realistic material properties for accurate simulation:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Use preset materials
let steel = MaterialProperties::steel();
let aluminum = MaterialProperties::aluminum();
let glass = MaterialProperties::glass();

// Or define custom materials
let custom = MaterialProperties {
    young_modulus: 200e9,      // 200 GPa
    poisson_ratio: 0.3,
    yield_strength: 250e6,     // 250 MPa
    ultimate_strength: 400e6,
    fracture_toughness: 50.0,  // MPa·√m
    density: 7850.0,           // kg/m³
    thermal_conductivity: 50.0,
    specific_heat: 500.0,
    thermal_expansion: 12e-6,
    melting_point: 1800.0,
};"#}</code></pre>
                            </div>
                            
                            <div class="callout callout-tip">
                                <img src="/assets/icons/lightbulb.svg" alt="Tip" />
                                <div>
                                    <strong>"Material Presets"</strong>
                                    <p>"Available presets: "<code>"steel()"</code>", "<code>"aluminum()"</code>", "
                                    <code>"glass()"</code>", "<code>"concrete()"</code>", "<code>"wood()"</code>", "
                                    <code>"rubber()"</code>", "<code>"ice()"</code></p>
                                </div>
                            </div>
                        </div>
                        
                        <div id="materials-stress" class="subsection">
                            <h3>"Stress & Strain"</h3>
                            <p>
                                "Full 3D stress tensor calculations with principal stresses and von Mises yield:"
                            </p>
                            
                            <div class="equation-card">
                                <div class="equation">"σ_vm = √(½[(σ₁-σ₂)² + (σ₂-σ₃)² + (σ₃-σ₁)²])"</div>
                                <div class="equation-label">"von Mises Stress"</div>
                            </div>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Create stress tensor
let mut stress = StressTensor::default();
stress.set(0, 0, 100e6);  // σ_xx = 100 MPa
stress.set(1, 1, 50e6);   // σ_yy = 50 MPa
stress.update_invariants();

// Check for yielding
let material = MaterialProperties::steel();
if stress.von_mises > material.yield_strength {
    println!("Material has yielded!");
}

// Get principal stresses
let (σ1, σ2, σ3) = (
    stress.principal[0],
    stress.principal[1],
    stress.principal[2],
);"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="materials-fracture" class="subsection">
                            <h3>"Fracture Mechanics"</h3>
                            <p>
                                "Linear elastic fracture mechanics with Paris law fatigue:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Check if crack will propagate
let k = stress_intensity_factor(
    100e6,  // Applied stress (Pa)
    0.01,   // Crack length (m)
);

let material = MaterialProperties::steel();
if k > material.fracture_toughness * 1e6 {
    // Crack will propagate!
}

// Paris law fatigue crack growth
use paris_parameters::STEEL;
let (c, m) = STEEL;
let da_dn = paris_law(delta_k, c, m);  // m/cycle"#}</code></pre>
                            </div>
                        </div>
                    </section>
                    
                    // =========================================================
                    // FLUIDS SECTION
                    // =========================================================
                    <section id="fluids" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Fluid Dynamics"
                        </h2>
                        
                        <div id="fluids-sph" class="subsection">
                            <h3>"SPH Simulation"</h3>
                            <p>
                                "Smoothed Particle Hydrodynamics for realistic fluid simulation:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Configure SPH simulation
let sph_config = SphConfig {
    smoothing_length: 0.1,
    rest_density: 1000.0,      // Water: 1000 kg/m³
    gas_constant: 2000.0,
    viscosity: 0.001,
    surface_tension: 0.0728,
    particle_mass: 0.02,
};

// SPH kernels
let density = poly6_kernel(r, h);           // Density estimation
let pressure_grad = spiky_gradient(r, h);   // Pressure force
let viscosity = viscosity_laplacian(r, h);  // Viscosity force"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="fluids-water" class="subsection">
                            <h3>"Water Physics"</h3>
                            <p>
                                "Specialized water simulation with waves, buoyancy, and splash effects:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Add water body component
commands.spawn((
    WaterBody {
        surface_level: 0.0,
        density: 1000.0,
        viscosity: 0.001,
        wave_amplitude: 0.1,
        wave_frequency: 1.0,
    },
    Transform::default(),
));

// Calculate buoyancy force
let buoyancy = buoyancy_force(
    1000.0,  // Fluid density
    0.5,     // Submerged volume
    9.81,    // Gravity
);
// Result: 4905 N upward"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="fluids-aero" class="subsection">
                            <h3>"Aerodynamics"</h3>
                            <p>
                                "Lift, drag, and Reynolds number calculations:"
                            </p>
                            
                            <div class="equation-card">
                                <div class="equation">"F_D = ½ρv²C_DA"</div>
                                <div class="equation-label">"Drag Force"</div>
                            </div>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Add aerodynamic body
commands.spawn((
    AerodynamicBody {
        drag_coefficient: 0.47,  // Sphere
        lift_coefficient: 0.0,
        drag_area: 1.0,          // m²
        lift_area: 0.0,
    },
    KineticState::default(),
    Transform::default(),
));

// Calculate terminal velocity
let v_terminal = terminal_velocity(
    100.0,   // Mass (kg)
    0.47,    // Drag coefficient
    1.0,     // Area (m²)
    1.225,   // Air density
);
// Result: ~40.8 m/s"#}</code></pre>
                            </div>
                        </div>
                    </section>
                    
                    // =========================================================
                    // DEFORMATION SECTION
                    // =========================================================
                    <section id="deformation" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Mesh Deformation"
                        </h2>
                        
                        <div class="callout callout-new">
                            <img src="/assets/icons/sparkles.svg" alt="New" />
                            <div>
                                <strong>"New in 2026"</strong>
                                <p>"Vertex-level mesh deformation from stress, temperature, and impacts!"</p>
                            </div>
                        </div>
                        
                        <div id="deformation-enable" class="subsection">
                            <h3>"Enabling Deformation"</h3>
                            <p>
                                "Enable deformation on any BasePart or MeshPart with a single property:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Enable deformation on a part
base_part.deformation = true;

// Or via property system
base_part.set_property("Deformation", PropertyValue::Bool(true));

// The system automatically adds DeformableMesh and DeformationState
// components when deformation is enabled"#}</code></pre>
                            </div>
                            
                            <div class="comparison-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th><code>"deformation = false"</code></th>
                                            <th><code>"deformation = true"</code></th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"Behavior"</td>
                                            <td>"Rigid body (default)"</td>
                                            <td>"Soft body with vertex deformation"</td>
                                        </tr>
                                        <tr>
                                            <td>"Performance"</td>
                                            <td>"Fast"</td>
                                            <td>"Depends on vertex count"</td>
                                        </tr>
                                        <tr>
                                            <td>"Stress Response"</td>
                                            <td>"None"</td>
                                            <td>"Vertices displaced by strain"</td>
                                        </tr>
                                        <tr>
                                            <td>"Temperature"</td>
                                            <td>"No effect on mesh"</td>
                                            <td>"Thermal expansion/contraction"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                        
                        <div id="deformation-stress" class="subsection">
                            <h3>"Stress-Based Deformation"</h3>
                            <p>
                                "Vertices are displaced based on the stress tensor using Hooke's law:"
                            </p>
                            
                            <div class="equation-card">
                                <div class="equation">"ε = σ / E"</div>
                                <div class="equation-label">"Strain from Stress (Hooke's Law)"</div>
                            </div>
                            
                            <p>
                                "The system automatically handles elastic (recoverable) and plastic 
                                (permanent) deformation based on the material's yield strength."
                            </p>
                        </div>
                        
                        <div id="deformation-thermal" class="subsection">
                            <h3>"Thermal Warping"</h3>
                            <p>
                                "Temperature changes cause isotropic expansion or contraction:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Configure thermal deformation
deformation_state.reference_temperature = 293.15;  // 20°C
deformation_state.thermal_expansion_coeff = 12e-6; // Steel
deformation_state.allow_thermal = true;

// When temperature changes, vertices expand/contract:
// displacement = position × α × ΔT"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="deformation-impact" class="subsection">
                            <h3>"Impact Deformation"</h3>
                            <p>
                                "Trigger localized deformation from impacts:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Send impact event
commands.trigger(ImpactDeformEvent {
    entity,
    point: Vec3::new(0.0, 0.5, 0.0),   // Impact location
    force: Vec3::new(0.0, -1000.0, 0.0), // Impact force
    radius: 0.5,                        // Effect radius
    permanent: true,                    // Plastic deformation
});"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="deformation-fracture" class="subsection">
                            <h3>"Fracture Splitting"</h3>
                            <p>
                                "Split meshes along fracture planes for destruction effects:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Trigger mesh fracture
commands.trigger(FractureMeshEvent {
    entity,
    origin: crack_position,
    normal: crack_plane_normal,
    direction: propagation_direction,
    energy: fracture_energy,
});

// For explosion-style fracture, use Voronoi
let fragments = voronoi_fracture(
    &mesh,
    impact_point,
    num_fragments,
    random_seed,
);"#}</code></pre>
                            </div>
                        </div>
                    </section>
                    
                    // =========================================================
                    // GPU SECTION
                    // =========================================================
                    <section id="gpu" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "GPU Acceleration"
                        </h2>
                        
                        <div id="gpu-sph" class="subsection">
                            <h3>"GPU SPH"</h3>
                            <p>
                                "Enable the "<code>"realism-gpu"</code>" feature for GPU-accelerated SPH 
                                with 10x particle scaling:"
                            </p>
                            
                            <div class="stats-grid">
                                <div class="stat-card">
                                    <div class="stat-value">"100K"</div>
                                    <div class="stat-label">"particles @ 60fps"</div>
                                    <div class="stat-note">"RTX 3080"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"1M"</div>
                                    <div class="stat-label">"particles @ 30fps"</div>
                                    <div class="stat-note">"with grid optimization"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"O(n)"</div>
                                    <div class="stat-label">"neighbor lookup"</div>
                                    <div class="stat-note">"spatial hash grid"</div>
                                </div>
                            </div>
                        </div>
                        
                        <div id="gpu-shaders" class="subsection">
                            <h3>"Compute Shaders"</h3>
                            <p>
                                "Four-pass compute pipeline for SPH simulation:"
                            </p>
                            
                            <ol class="numbered-list">
                                <li>
                                    <strong>"Grid Construction"</strong>
                                    " - Spatial hash for O(1) neighbor lookup"
                                </li>
                                <li>
                                    <strong>"Density Pass"</strong>
                                    " - Poly6 kernel density estimation"
                                </li>
                                <li>
                                    <strong>"Force Pass"</strong>
                                    " - Pressure + viscosity + surface tension"
                                </li>
                                <li>
                                    <strong>"Integration Pass"</strong>
                                    " - Semi-implicit Euler with boundary handling"
                                </li>
                            </ol>
                        </div>
                        
                        <div id="gpu-performance" class="subsection">
                            <h3>"Performance Tips"</h3>
                            
                            <ul class="feature-list">
                                <li>
                                    <strong>"Workgroup size 256"</strong>
                                    " - Optimal for most GPUs"
                                </li>
                                <li>
                                    <strong>"Use grid acceleration"</strong>
                                    " - Reduces O(n²) to O(n)"
                                </li>
                                <li>
                                    <strong>"Batch buffer updates"</strong>
                                    " - Minimize CPU↔GPU transfers"
                                </li>
                                <li>
                                    <strong>"Profile with GPU timers"</strong>
                                    " - Use SphGpuMetrics for timing"
                                </li>
                            </ul>
                        </div>
                    </section>
                    
                    // =========================================================
                    // QUANTUM SECTION
                    // =========================================================
                    <section id="quantum" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"08"</span>
                            "Quantum Effects"
                        </h2>
                        
                        <div class="callout callout-advanced">
                            <img src="/assets/icons/atom.svg" alt="Advanced" />
                            <div>
                                <strong>"Advanced Feature"</strong>
                                <p>"Quantum effects are for specialized simulations. Most games don't need this!"</p>
                            </div>
                        </div>
                        
                        <div id="quantum-statistics" class="subsection">
                            <h3>"Quantum Statistics"</h3>
                            <p>
                                "Bose-Einstein and Fermi-Dirac distributions for quantum systems:"
                            </p>
                            
                            <div class="equation-card">
                                <div class="equation">"⟨n⟩ = 1 / (e^((ε-μ)/k_BT) ∓ 1)"</div>
                                <div class="equation-label">"Quantum Distribution (- for BE, + for FD)"</div>
                            </div>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Bose-Einstein distribution (bosons: photons, phonons)
let occupation = bose_einstein_distribution(
    energy,             // Energy level (J)
    chemical_potential, // μ (J)
    temperature,        // T (K)
);

// Fermi-Dirac distribution (fermions: electrons)
let occupation = fermi_dirac_distribution(
    energy,
    fermi_energy,
    temperature,
);"#}</code></pre>
                            </div>
                        </div>
                        
                        <div id="quantum-bec" class="subsection">
                            <h3>"Bose-Einstein Condensates"</h3>
                            <p>
                                "Simulate BEC formation and properties:"
                            </p>
                            
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rust"</span>
                                </div>
                                <pre><code class="language-rust">{r#"// Create Rubidium-87 BEC
let mut bec = BoseEinsteinCondensate::rubidium_87(
    1e6,                              // Particle count
    2.0 * PI * 100.0,                 // Trap frequency (Hz)
);
bec.temperature = 100e-9;             // 100 nK
bec.update();

println!("Critical temp: {} nK", bec.critical_temperature * 1e9);
println!("Condensate fraction: {:.1}%", bec.condensate_fraction * 100.0);
println!("Thomas-Fermi radius: {} μm", bec.thomas_fermi_radius() * 1e6);"#}</code></pre>
                            </div>
                        </div>
                    </section>
                    
                    // =========================================================
                    // API REFERENCE SECTION
                    // =========================================================
                    <section id="api" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"09"</span>
                            "API Reference"
                        </h2>
                        
                        <div id="api-constants" class="subsection">
                            <h3>"Physical Constants"</h3>
                            
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Constant"</th>
                                            <th>"Symbol"</th>
                                            <th>"Value"</th>
                                            <th>"Unit"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"GAS_CONSTANT"</code></td>
                                            <td>"R"</td>
                                            <td>"8.314"</td>
                                            <td>"J/(mol·K)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"BOLTZMANN"</code></td>
                                            <td>"k_B"</td>
                                            <td>"1.381×10⁻²³"</td>
                                            <td>"J/K"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"GRAVITATIONAL"</code></td>
                                            <td>"G"</td>
                                            <td>"6.674×10⁻¹¹"</td>
                                            <td>"N·m²/kg²"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"STEFAN_BOLTZMANN"</code></td>
                                            <td>"σ"</td>
                                            <td>"5.670×10⁻⁸"</td>
                                            <td>"W/(m²·K⁴)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"PLANCK"</code></td>
                                            <td>"h"</td>
                                            <td>"6.626×10⁻³⁴"</td>
                                            <td>"J·s"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AVOGADRO"</code></td>
                                            <td>"N_A"</td>
                                            <td>"6.022×10²³"</td>
                                            <td>"mol⁻¹"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                        
                        <div id="api-units" class="subsection">
                            <h3>"Unit System"</h3>
                            <p>
                                "All physics calculations use SI units:"
                            </p>
                            
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Quantity"</th>
                                            <th>"Unit"</th>
                                            <th>"Symbol"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr><td>"Length"</td><td>"meter"</td><td>"m"</td></tr>
                                        <tr><td>"Mass"</td><td>"kilogram"</td><td>"kg"</td></tr>
                                        <tr><td>"Time"</td><td>"second"</td><td>"s"</td></tr>
                                        <tr><td>"Temperature"</td><td>"kelvin"</td><td>"K"</td></tr>
                                        <tr><td>"Pressure"</td><td>"pascal"</td><td>"Pa"</td></tr>
                                        <tr><td>"Energy"</td><td>"joule"</td><td>"J"</td></tr>
                                        <tr><td>"Force"</td><td>"newton"</td><td>"N"</td></tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                        
                        <div id="api-components" class="subsection">
                            <h3>"Components"</h3>
                            
                            <div class="api-table">
                                <table>
                                    <thead>
                                        <tr>
                                            <th>"Component"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Particle"</code></td>
                                            <td>"Base particle with mass, radius, type"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"ThermodynamicState"</code></td>
                                            <td>"T, P, V, U, S state variables"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"KineticState"</code></td>
                                            <td>"Velocity, acceleration, momentum"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"MaterialProperties"</code></td>
                                            <td>"Young's modulus, yield strength, etc."</td>
                                        </tr>
                                        <tr>
                                            <td><code>"StressTensor"</code></td>
                                            <td>"3×3 stress tensor with invariants"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"DeformableMesh"</code></td>
                                            <td>"Vertex deformation state"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FluidParticle"</code></td>
                                            <td>"SPH particle with density, pressure"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"AerodynamicBody"</code></td>
                                            <td>"Drag/lift coefficients and areas"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </section>
                    
                    // Next/Prev Navigation
                    <nav class="docs-nav-footer">
                        <a href="/docs/building" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Building"</span>
                            </div>
                        </a>
                        <a href="/docs/networking" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Networking"</span>
                            </div>
                            <img src="/assets/icons/arrow-right.svg" alt="Next" />
                        </a>
                    </nav>
                </main>
            </div>
            
            <Footer />
        </div>
    }
}
