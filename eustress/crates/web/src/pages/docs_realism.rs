// =============================================================================
// Eustress Web - Realism Documentation Page
// =============================================================================
// Realism: the physical-law libraries, which of them run every frame on parts,
// how a part opts in, the cell model, and the V-Cell case study.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

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
                TocSubsection { id: "overview-what", title: "What Realism Is" },
                TocSubsection { id: "overview-kinds", title: "Systems and Libraries" },
            ],
        },
        TocSection {
            id: "authoring",
            title: "Opting In",
            subsections: vec![
                TocSubsection { id: "authoring-sections", title: "Sections in a Part File" },
                TocSubsection { id: "authoring-properties", title: "In Properties" },
                TocSubsection { id: "authoring-units", title: "Units and Constants" },
            ],
        },
        TocSection {
            id: "materials",
            title: "Materials & Fracture",
            subsections: vec![
                TocSubsection { id: "materials-presets", title: "Material Presets" },
                TocSubsection { id: "materials-dents", title: "Dents" },
                TocSubsection { id: "materials-fracture", title: "Cracks and Fragments" },
                TocSubsection { id: "materials-stress", title: "The Stress Library" },
            ],
        },
        TocSection {
            id: "heat",
            title: "Heat",
            subsections: vec![
                TocSubsection { id: "heat-conduction", title: "Conduction Between Parts" },
                TocSubsection { id: "heat-laws", title: "Heat Laws and Cycles" },
            ],
        },
        TocSection {
            id: "cell",
            title: "The Cell Model",
            subsections: vec![
                TocSubsection { id: "cell-step", title: "One Step" },
                TocSubsection { id: "cell-fade", title: "Life and Fade" },
                TocSubsection { id: "cell-keys", title: "Keys and Readouts" },
            ],
        },
        TocSection {
            id: "scripts",
            title: "Scripts & Tools",
            subsections: vec![
                TocSubsection { id: "scripts-rune", title: "Rune Law Modules" },
                TocSubsection { id: "scripts-tools", title: "Agent Tools" },
            ],
        },
        TocSection {
            id: "vcell",
            title: "Case Study: V-Cell",
            subsections: vec![
                TocSubsection { id: "vcell-setup", title: "The Walkthrough" },
                TocSubsection { id: "vcell-loop", title: "Detect, Repair, Verify" },
                TocSubsection { id: "vcell-model", title: "What Models It" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-components", title: "Laws Without a File Section" },
                TocSubsection { id: "roadmap-time", title: "Simulated Time for Every Law" },
            ],
        },
    ]
}

/// The cell model's step, in the order the code computes it.
#[component]
fn CellStepDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 240" role="img"
                aria-label="Each step of the cell model computes the open-circuit voltage, subtracts the losses to get the terminal voltage, counts charge into the state of charge, turns the losses into heat and temperature, and adds damage to the fade channels.">
                <defs>
                    <marker id="cell-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>
                <text x="20" y="24" class="dg-title">"One cell-model step"</text>

                <rect x="20" y="44" width="180" height="60" rx="8" class="dg-box"></rect>
                <text x="110" y="70" class="dg-label" text-anchor="middle">"Open-circuit voltage"</text>
                <text x="110" y="90" class="dg-note" text-anchor="middle">"Nernst, from SOC and T"</text>
                <line x1="200" y1="74" x2="228" y2="74" class="dg-line" marker-end="url(#cell-arrow)"></line>

                <rect x="230" y="44" width="180" height="60" rx="8" class="dg-box"></rect>
                <text x="320" y="70" class="dg-label" text-anchor="middle">"Losses"</text>
                <text x="320" y="90" class="dg-note" text-anchor="middle">"IR, activation, transport"</text>
                <line x1="410" y1="74" x2="438" y2="74" class="dg-line" marker-end="url(#cell-arrow)"></line>

                <rect x="440" y="44" width="180" height="60" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="530" y="70" class="dg-label" text-anchor="middle">"Terminal voltage"</text>
                <text x="530" y="90" class="dg-note" text-anchor="middle">"OCV and losses"</text>
                <line x1="530" y1="104" x2="530" y2="138" class="dg-line" marker-end="url(#cell-arrow)"></line>

                <rect x="440" y="140" width="180" height="60" rx="8" class="dg-box"></rect>
                <text x="530" y="166" class="dg-label" text-anchor="middle">"State of charge"</text>
                <text x="530" y="186" class="dg-note" text-anchor="middle">"coulomb counting"</text>
                <line x1="440" y1="170" x2="412" y2="170" class="dg-line" marker-end="url(#cell-arrow)"></line>

                <rect x="230" y="140" width="180" height="60" rx="8" class="dg-box"></rect>
                <text x="320" y="166" class="dg-label" text-anchor="middle">"Heat and temperature"</text>
                <text x="320" y="186" class="dg-note" text-anchor="middle">"ohmic, reaction, entropic"</text>
                <line x1="230" y1="170" x2="202" y2="170" class="dg-line" marker-end="url(#cell-arrow)"></line>

                <rect x="20" y="140" width="180" height="60" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="110" y="166" class="dg-label" text-anchor="middle">"Life"</text>
                <text x="110" y="186" class="dg-note" text-anchor="middle">"dendrite risk, five fades"</text>

                <text x="320" y="228" class="dg-note" text-anchor="middle">"Repeated in clock timesteps until the frame's simulated time is used."</text>
            </svg>
            <figcaption>
                "The cell model's step, in the order the engine computes it. The temperature from one
                step feeds the voltage and the fade rates of the next."
            </figcaption>
        </figure>
    }
}

/// Realism documentation page.
#[component]
pub fn DocsRealismPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-realism"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/fire.svg" alt="Realism" class="toc-icon" />
                        <h2>"Realism"</h2>
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Realism"</span>
                        </div>
                        <h1 class="docs-title">"Realism"</h1>
                        <p class="docs-subtitle">
                            "Realism is Eustress's library of physical laws in SI units: materials and
                            fracture, heat, electrochemistry, structures and more. A few of the laws run every
                            frame on the parts that opt in through their files; the rest are functions you call
                            from Rune or Rust."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "20 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/cube.svg" alt="Level" />
                                "Advanced"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "Updated Sep 2026"
                            </span>
                        </div>
                    </header>

                    // =========================================================
                    // OVERVIEW
                    // =========================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Overview"
                        </h2>

                        <div id="overview-what" class="subsection">
                            <h3>"What Realism Is"</h3>
                            <p>
                                "Realism is the physical-law layer of Eustress. Each law is a plain function over
                                SI quantities, such as the Nernst potential or the Euler buckling load, kept in one
                                library that Studio, "<code>"eustress-headless"</code>" and scripts share. A few
                                laws are also wired into systems that update parts while a run plays, which is how
                                a battery drains, a concrete slab cracks and a hot block warms its neighbor."
                            </p>
                            <p>
                                "The plugin that registers those systems is part of the core simulation tier, so it
                                is always on, in Studio and in "<code>"eustress-headless"</code>". Rigid-body
                                motion belongs to Avian and is covered in "<a href="/docs/physics">"Physics"</a>
                                "; Realism adds the quantities rigid bodies do not carry, and "
                                <a href="/docs/simulation">"Simulation"</a>" covers the clock and the recordings
                                that capture them."
                            </p>
                            <div class="stats-grid">
                                <div class="stat-card">
                                    <div class="stat-value">"8"</div>
                                    <div class="stat-label">"Named material presets"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"24"</div>
                                    <div class="stat-label">"Cell readouts"</div>
                                    <div class="stat-note">"published every Play frame"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"5"</div>
                                    <div class="stat-label">"Fade channels"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"13"</div>
                                    <div class="stat-label">"Rune law modules"</div>
                                </div>
                            </div>
                        </div>

                        <div id="overview-kinds" class="subsection">
                            <h3>"Systems and Libraries"</h3>
                            <p>"A law reaches a Space in one of three ways, and the way decides what a file can do with it:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Laws"</th><th>"Runs"</th><th>"On"</th><th>"When"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Cell model"</td><td>"Every frame"</td><td>"Parts with "<code>"[electrochemical]"</code>" and a capacity above zero"</td><td>"Play"</td></tr>
                                    <tr><td>"Heat conduction"</td><td>"Every frame"</td><td>"Pairs of parts with "<code>"[material]"</code>" and "<code>"[thermodynamic]"</code>" within 0.5 m"</td><td>"Edit and Play"</td></tr>
                                    <tr><td>"Dents and fracture"</td><td>"On impact"</td><td>"Parts with Destructible on"</td><td>"Play, in Studio"</td></tr>
                                    <tr><td>"Stress tensors, reactors, circuits, fluids, particles"</td><td>"Every frame"</td><td>"Components attached from Rust; no file section creates them"</td><td>"Edit and Play"</td></tr>
                                    <tr><td>"Structures, cycles, propulsion, optics, acoustics, nuclear, plasma, control, numerics"</td><td>"When called"</td><td>"Rune scripts and Rust code"</td><td>"Any time"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Not in this build"</strong>
                                    <p>
                                        "The realism library also holds GPU compute for fluid particles, a symbolic
                                        solver and quantum statistics. They sit behind build features that Eustress
                                        Engine does not enable, so they are not part of the app."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // OPTING IN
                    // =========================================================
                    <section id="authoring" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Opting In"
                        </h2>

                        <div id="authoring-sections" class="subsection">
                            <h3>"Sections in a Part File"</h3>
                            <p>"A part joins a realism system through a section in its file. The loader reads five sections on any class:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Section"</th><th>"Holds"</th><th>"Used by"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"[material]"</code></td><td>"Mechanical and thermal constants"</td><td>"Heat conduction; dents and fracture"</td></tr>
                                    <tr><td><code>"[thermodynamic]"</code></td><td>"Temperature, pressure, volume, energy, entropy, moles"</td><td>"Heat conduction; the cell model's temperature"</td></tr>
                                    <tr><td><code>"[electrochemical]"</code></td><td>"A cell's design and starting state"</td><td>"The cell model"</td></tr>
                                    <tr><td><code>"[nuclear]"</code></td><td>"Starting state of an "<code>"ArcReactorCore"</code></td><td>"The ARC-1 reactor model ("<a href="#roadmap-components">"not yet active"</a>")"</td></tr>
                                    <tr><td><code>"[plasma]"</code></td><td>"Densities, temperatures, ionization, field"</td><td>"No per-frame system yet"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Destructible parts need no section at all. Set "<code>"destructible"</code>" in "
                                <code>"[properties]"</code>" and the part takes its mechanical constants from its "
                                <code>"material"</code>" name:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[properties]
material = "Concrete"
destructible = true    # dents and cracks during Play

[metadata]
class_name = "Part""#}</code></pre>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A [material] block replaces the preset"</strong>
                                    <p>
                                        "The loader reads a "<code>"[material]"</code>" block as written: a key it
                                        leaves out is 0, not the preset's value. The dent model then falls back to
                                        generic plastic constants for each missing mechanical value, and heat
                                        conduction ignores a part with no conductivity, density or specific heat.
                                        Name the material in "<code>"[properties]"</code>" instead, and write a "
                                        <code>"[material]"</code>" block only for a material the presets do not
                                        cover, with every constant filled in."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="authoring-properties" class="subsection">
                            <h3>"In Properties"</h3>
                            <p>
                                "Select a part and Properties shows "<strong>"Destructible"</strong>" under
                                Physics, and the realism sections its file declares under "
                                <strong>"Material"</strong>", "<strong>"Thermodynamic"</strong>" and "
                                <strong>"Electrochemical"</strong>". For a destructible part with no "
                                <code>"[material]"</code>" block, the Material category lists the constants its
                                material name implies, without writing them to the file."
                            </p>
                            <p>
                                "Those rows show the file's values. The live state of a running model, such as a
                                cell's charge or temperature, is published as sim values: read it with the
                                simulation tools or in the run's "<a href="/docs/simulation#observe-recordings">"recording"</a>"."
                            </p>
                        </div>

                        <div id="authoring-units" class="subsection">
                            <h3>"Units and Constants"</h3>
                            <p>"Every law works in SI units, and so do the file keys, with a few named exceptions:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Quantity"</th><th>"Unit"</th><th>"Exceptions"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Length"</td><td>"m"</td><td>"µm for "<code>"separator_thickness_um"</code>", "<code>"plated_thickness_um"</code></td></tr>
                                    <tr><td>"Temperature"</td><td>"K ("<code>"[thermodynamic]"</code>" defaults to 298.15)"</td><td>"°C in "<code>"battery.temperature_c"</code>" and "<code>"battery.ambient_c"</code></td></tr>
                                    <tr><td>"Pressure, stress, moduli"</td><td>"Pa (pressure defaults to 101,325)"</td><td>"MPa for "<code>"stack_pressure_mpa"</code>", "<code>"creep_threshold_mpa"</code></td></tr>
                                    <tr><td>"Fracture toughness"</td><td>"Pa·√m"</td><td>"None"</td></tr>
                                    <tr><td>"Current, capacity"</td><td>"A (positive discharges), Ah"</td><td>"None"</td></tr>
                                    <tr><td>"Activation energies"</td><td>"eV"</td><td>"None"</td></tr>
                                    <tr><td>"State of charge, retention, dendrite risk"</td><td>"Fractions from 0 to 1"</td><td>"Their watchpoints are labeled %"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Physical constants come from one module: R = 8.314462618 J/(mol·K), F =
                                96,485.33212 C/mol, k_B = 1.380649e-23 J/K, N_A = 6.02214076e23 /mol, G =
                                6.67430e-11 N·m²/kg² and the Stefan-Boltzmann constant 5.670374419e-8 W/(m²·K⁴)."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // MATERIALS & FRACTURE
                    // =========================================================
                    <section id="materials" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Materials & Fracture"
                        </h2>

                        <div id="materials-presets" class="subsection">
                            <h3>"Material Presets"</h3>
                            <p>
                                "A material's constants are 14 numbers: Young's modulus, Poisson's ratio, yield
                                and ultimate strength, fracture toughness K_IC, hardness, thermal conductivity,
                                specific heat, thermal expansion, melting point, density, static and kinetic
                                friction, and restitution, plus any custom numeric keys. Eight presets supply them
                                by name, and a part's appearance material maps onto one:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Preset"</th><th>"Part materials"</th><th>"E (GPa)"</th><th>"Yield (MPa)"</th><th>"K_IC (MPa·√m)"</th><th>"Density (kg/m³)"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Steel"</td><td>"Steel, Metal, CorrodedMetal, DiamondPlate"</td><td>"200"</td><td>"250"</td><td>"50"</td><td>"7,850"</td></tr>
                                    <tr><td>"Aluminum"</td><td>"Aluminum, Foil"</td><td>"70"</td><td>"270"</td><td>"30"</td><td>"2,700"</td></tr>
                                    <tr><td>"Concrete"</td><td>"Concrete, Brick, Granite, Marble, Slate, Cobblestone"</td><td>"30"</td><td>"30"</td><td>"1"</td><td>"2,400"</td></tr>
                                    <tr><td>"Glass"</td><td>"Glass"</td><td>"70"</td><td>"45"</td><td>"0.7"</td><td>"2,500"</td></tr>
                                    <tr><td>"Ice"</td><td>"Ice"</td><td>"9"</td><td>"1"</td><td>"0.1"</td><td>"917"</td></tr>
                                    <tr><td>"Wood (Oak)"</td><td>"Wood, WoodPlanks"</td><td>"12"</td><td>"60"</td><td>"10"</td><td>"700"</td></tr>
                                    <tr><td>"Rubber"</td><td>"Rubber, Fabric, Grass"</td><td>"0.01"</td><td>"15"</td><td>"5"</td><td>"1,100"</td></tr>
                                    <tr><td>"Plastic (ABS)"</td><td>"Plastic, SmoothPlastic, Neon, Sand, Pebble"</td><td>"2.3"</td><td>"40"</td><td>"3"</td><td>"1,050"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Names match without regard to case, spaces, underscores or hyphens. A name with no
                                preset, such as Gold, gives a destructible part generic plastic-like constants.
                                The library derives the rest from these: shear modulus G = E / (2(1 + ν)), bulk
                                modulus, the Lamé constants, thermal diffusivity k / (ρ c_p) and the speed of sound
                                √(E / ρ). It calls a material brittle when K_IC is below 8 MPa·√m or its yield
                                strength is at least 90 % of its ultimate strength."
                            </p>
                        </div>

                        <div id="materials-dents" class="subsection">
                            <h3>"Dents"</h3>
                            <p>
                                "During Play, a destructible part that is struck takes a dent computed from contact
                                mechanics. The impact energy comes from the closing speed, treating the collision
                                as fully inelastic. A static part counts as immovable, and the striking body is
                                treated as a sphere whose radius is half its smallest dimension."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"E = ½ m_eff v²,   1 / m_eff = 1 / m₁ + 1 / m₂"</div>
                                <div class="equation-label">"Impact energy from the closing speed v"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"1 / E* = (1 - ν₁²) / E₁ + (1 - ν₂²) / E₂"</div>
                                <div class="equation-label">"Reduced contact modulus of the pair"</div>
                            </div>
                            <p>
                                "Below Johnson's yield-onset energy the dent is elastic and springs back; above it
                                the dent is plastic, using Tabor hardness H = 3σ_y, and stays:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"δ = (E / ((8/15) E* √R))^(2/5)"</div>
                                <div class="equation-label">"Elastic depth (Hertz)"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"δ = √((E - E_y) / (π R H)),   E_y = 10 R³ σ_y (σ_y / E*)⁴"</div>
                                <div class="equation-label">"Plastic depth above the yield-onset energy E_y"</div>
                            </div>
                            <p>
                                "The dent spreads over a patch of radius √(2 R δ), and its depth is physical, in
                                meters, with no visual exaggeration."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Pausing undoes dents"</strong>
                                    <p>
                                        "Dents exist only while a run is playing. Pause or Stop restores every
                                        dented mesh, and resuming does not bring the dents back. Fracture fragments
                                        stay until Stop."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="materials-fracture" class="subsection">
                            <h3>"Cracks and Fragments"</h3>
                            <p>
                                "When the impact energy exceeds the part's Griffith threshold, the part cracks
                                instead of denting. The threshold is the material's critical energy release rate
                                times the part's smallest cross-section, the cheapest path for a crack:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"G_c = K_IC² / E,   E_crack = G_c × A_min"</div>
                                <div class="equation-label">"Griffith fracture threshold"</div>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Preset"</th><th>"G_c (J/m²)"</th><th>"Threshold, 2 m × 1 m × 0.2 m slab"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Ice"</td><td>"1.1"</td><td>"0.22 J"</td></tr>
                                    <tr><td>"Glass"</td><td>"7"</td><td>"1.4 J"</td></tr>
                                    <tr><td>"Concrete"</td><td>"33"</td><td>"6.7 J"</td></tr>
                                    <tr><td>"Plastic (ABS)"</td><td>"3,900"</td><td>"780 J"</td></tr>
                                    <tr><td>"Steel"</td><td>"12,500"</td><td>"2,500 J"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "So a 10 kg block that hits an anchored slab at 2 m/s brings 20 J: enough to crack
                                glass or concrete, and only a dent in plastic or steel."
                            </p>
                            <p>
                                "A crack splits the part into two dynamic bodies along a plane that contains the
                                impact direction, so a part struck from above splits down through itself. The
                                original part is hidden rather than deleted, and Stop brings it back and removes
                                the fragments. Four guards stop a chain reaction: a fragment can be fractured again
                                at most twice, a cut that would leave a piece under 0.05 m long is refused, a new
                                fragment cannot crack for 0.3 s, and at most 4 fractures happen per frame."
                            </p>
                        </div>

                        <div id="materials-stress" class="subsection">
                            <h3>"The Stress Library"</h3>
                            <p>
                                "The library's stress and strain tensors are 3 by 3 and symmetric. A stress tensor
                                caches its von Mises stress, principal stresses, hydrostatic stress and maximum
                                shear. Functions convert between the two with generalized Hooke's law, including
                                plane stress and plane strain, and test yield:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"σ_vm = √(½ [(σxx - σyy)² + (σyy - σzz)² + (σzz - σxx)² + 6 (τxy² + τyz² + τzx²)])"</div>
                                <div class="equation-label">"Von Mises equivalent stress"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"σ = λ tr(ε) I + 2μ ε,   λ = E ν / ((1 + ν)(1 - 2ν)),   μ = E / (2(1 + ν))"</div>
                                <div class="equation-label">"Generalized Hooke's law"</div>
                            </div>
                            <p>
                                "A material yields when σ_vm ≥ σ_y (von Mises) or τ_max ≥ σ_y / 2 (Tresca), and
                                its safety factor is σ_y / σ_vm. These are Rust functions today: a per-frame system
                                updates stress from strain on entities that carry both tensors, and no file section
                                attaches them yet."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // HEAT
                    // =========================================================
                    <section id="heat" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Heat"
                        </h2>

                        <div id="heat-conduction" class="subsection">
                            <h3>"Conduction Between Parts"</h3>
                            <p>
                                "Parts that carry both "<code>"[material]"</code>" and "
                                <code>"[thermodynamic]"</code>" exchange heat by conduction. Every 0.5 s the
                                engine pairs any two such parts whose centers are within 0.5 m, and every frame it
                                moves heat across each pair by Fourier's law, using the harmonic mean of the two
                                conductivities:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"Q̇ = k_eff A ΔT / L,   k_eff = 2 k_a k_b / (k_a + k_b)"</div>
                                <div class="equation-label">"Fourier conduction across a contact"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"ΔT_part = Q̇ Δt / (ρ V c_p)"</div>
                                <div class="equation-label">"Temperature change of each part in a frame"</div>
                            </div>
                            <p>
                                "The contact area A is the smaller X extent of the pair times the smaller Z extent,
                                L is the distance between the centers (at least 1 mm), and V is each part's volume
                                from its size. Each contact changes a part's temperature by at most 50 K per frame,
                                and pairs less than 0.01 K apart are skipped. A contact, once made, is kept even if
                                the parts move apart."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[material]
name = "Aluminum"
density = 2700.0              # kg/m3
specific_heat = 900.0         # J/(kg K)
thermal_conductivity = 237.0  # W/(m K)

[thermodynamic]
temperature = 350.0           # K"#}</code></pre>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Conduction runs while you edit"</strong>
                                    <p>
                                        "Conduction steps on real frame time whenever the Space is open, in Edit as
                                        well as Play, and does not follow the time scale. Temperatures therefore
                                        drift while you edit, a run starts from wherever they are when you press
                                        Play, and Stop returns them to that point."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="heat-laws" class="subsection">
                            <h3>"Heat Laws and Cycles"</h3>
                            <p>
                                "The thermodynamics library covers the rest of heat as functions: ideal and van der
                                Waals gases, work in isobaric, isothermal and adiabatic processes, heat capacities,
                                entropy changes, Carnot efficiency and heat-pump COP, conduction, convection and
                                radiation rates, phase change, enthalpy, and the Gibbs and Helmholtz free
                                energies. The thermocycles library adds steady-state Rankine, Brayton, Otto and
                                Diesel, refrigeration and heat-exchanger analysis (LMTD and effectiveness-NTU)."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"Q̇ = h A (T_surface - T_fluid)"</div>
                                <div class="equation-label">"Convection (Newton's law of cooling)"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"η = 1 - T_cold / T_hot"</div>
                                <div class="equation-label">"Carnot efficiency"</div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // THE CELL MODEL
                    // =========================================================
                    <section id="cell" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "The Cell Model"
                        </h2>

                        <div id="cell-step" class="subsection">
                            <h3>"One Step"</h3>
                            <p>
                                "The cell model turns a part's "<code>"[electrochemical]"</code>" section into a
                                live battery. Every Play frame it steps each such part whose capacity is above zero,
                                cutting the frame's simulated time into clock timesteps (1/60 s by default; see "
                                <a href="/docs/simulation#clock-limits">"limits of compression"</a>"). The cell with
                                the largest capacity is the one driven by sim values and published. With a "
                                <code>"[thermodynamic]"</code>" section the cell's temperature evolves; without one
                                it stays at 298.15 K."
                            </p>
                            <CellStepDiagram />
                            <p>
                                "Open-circuit voltage follows the Nernst equation, with the reaction quotient taken
                                from the state of charge and n = 2 electrons. The standard potential is the cell's
                                own "<code>"standard_potential_v"</code>", or the sodium-sulfur 2.23 V when unset:"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"E = E° - (RT / nF) ln Q,   Q = (1 - SOC) / SOC"</div>
                                <div class="equation-label">"Open-circuit voltage (Nernst)"</div>
                            </div>
                            <p>
                                "Three losses come off it. Resistance rises in the cold by an Arrhenius law (0.35 eV
                                unless set). Activation loss is the exact inverse of symmetric Butler-Volmer, with
                                an exchange current density of 50 A/m². Transport loss rises steeply as the current
                                density j approaches the limiting current σ(RT/F) / (1.5 t), set by the separator's
                                ionic conductivity σ and thickness t, and is zero when no separator is set."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"R(T) = R₂₅ exp[(E_a / k_B)(1/T - 1/298.15 K)]"</div>
                                <div class="equation-label">"Temperature-dependent resistance"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"η_ct = (2RT / F) asinh(j / 2j₀),   η_conc = -(RT / 2F) ln(1 - j / j_lim)"</div>
                                <div class="equation-label">"Activation and transport losses"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"V = E - (I R + η_ct + η_conc) discharging,   V = E + (I R + η_ct + η_conc) charging"</div>
                                <div class="equation-label">"Terminal voltage, with I positive on discharge"</div>
                            </div>
                            <p>"Charge counting, heat and temperature close the step:"</p>
                            <div class="equation-card">
                                <div class="equation">"SOC ← SOC - I Δt / (3600 Q_nom r)"</div>
                                <div class="equation-label">"State of charge (Q_nom in Ah, r the capacity retention)"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"Q̇ = I² R + |I| |η_ct| - T I (dE/dT)"</div>
                                <div class="equation-label">"Heat: ohmic, reaction and reversible entropic terms"</div>
                            </div>
                            <div class="equation-card">
                                <div class="equation">"T ← T_ss + (T - T_ss) e^(-Δt/τ),   T_ss = T_amb + Q̇ R_th,   τ = R_th C_th"</div>
                                <div class="equation-label">"Lumped thermal model, solved exactly for each step"</div>
                            </div>
                            <p>
                                "Dendrite risk compares the plating current density on charge with a critical
                                current density: the cell's "<code>"j_crit_a_per_m2"</code>", or a Monroe-Newman
                                estimate from sodium constants (G = 30 GPa, δ = 5 nm, V_m = 23.7 cm³/mol, about
                                131 A/m²) when unset. Discharge scores zero, and the risk is capped at 1."
                            </p>
                            <div class="equation-card">
                                <div class="equation">"risk = j_plating / j_crit,   j_crit = 2 G δ / (F V_m)"</div>
                                <div class="equation-label">"Dendrite risk and the Monroe-Newman critical current"</div>
                            </div>
                        </div>

                        <div id="cell-fade" class="subsection">
                            <h3>"Life and Fade"</h3>
                            <p>"Capacity retention is the product of five independent channels, so whichever bites first sets the life:"</p>
                            <div class="equation-card">
                                <div class="equation">"retention = (1 - f_Li)(1 - f_crack)(1 - f_cal)(1 - f_creep)(1 - f_short)"</div>
                                <div class="equation-label">"Capacity retention, kept between 0.01 and 1"</div>
                            </div>
                            <ul class="docs-list">
                                <li><strong>"Lithium inventory"</strong>": each unit of charge loses a little metal to interphase, more for deeper cycles (depth to the power 0.8) and at higher temperature. A reservoir ("<code>"li_reservoir_frac"</code>") buffers this channel only."</li>
                                <li><strong>"Cathode cracking"</strong>": fatigue damage per unit charge of "<code>"crack_k"</code>" times depth to the power 1.5, optionally scaled by stack pressure."</li>
                                <li><strong>"Calendar ageing"</strong>": grows with the square root of equivalent hours, which accrue on every step, faster at high charge and temperature; the default coefficient is 2.14e-4."</li>
                                <li><strong>"Creep"</strong>": above "<code>"creep_threshold_mpa"</code>" (1 MPa by default) the plated metal creeps at a rate with exponent 6.6 in the overpressure, and only strain beyond the accommodation void ("<code>"creep_accommodation_frac"</code>") counts."</li>
                                <li><strong>"Bridged layers"</strong>": creep that crosses the separator shorts whole layers of a "<code>"layer_count"</code>" stack, counted as layers lost rather than fade."</li>
                            </ul>
                            <p>
                                "With "<code>"sei_thickness_nm"</code>" set, the coulombic loss behind the lithium
                                channel is derived from deposit roughness, areal capacity q and stack pressure P
                                instead of a fitted efficiency (0.995 by default):"
                            </p>
                            <div class="equation-card">
                                <div class="equation">"1 - CE = R δ ρ_Li F / (M_Li q),   R = 1 + k (j / j_crit)^1.5 (2 / P)^1.5"</div>
                                <div class="equation-label">"Derived coulombic loss (k = 105.7 by default)"</div>
                            </div>
                            <p>
                                <code>"battery.cycle_count"</code>" counts equivalent full cycles: the charge moved
                                divided by twice the effective capacity."
                            </p>
                        </div>

                        <div id="cell-keys" class="subsection">
                            <h3>"Keys and Readouts"</h3>
                            <p>"The main "<code>"[electrochemical]"</code>" keys, and what the model does when a key is absent:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Unit"</th><th>"If absent"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"capacity_ah"</code></td><td>"Ah"</td><td>"0, and the part is not stepped"</td></tr>
                                    <tr><td><code>"soc"</code></td><td>"0 to 1"</td><td>"1, a full cell"</td></tr>
                                    <tr><td><code>"internal_resistance"</code></td><td>"Ω at 25 °C"</td><td>"0, no ohmic loss"</td></tr>
                                    <tr><td><code>"standard_potential_v"</code>", "<code>"entropy_coefficient_v_per_k"</code></td><td>"V, V/K"</td><td>"Sodium-sulfur values"</td></tr>
                                    <tr><td><code>"electrode_area_m2"</code></td><td>"m², all layers"</td><td>"0.03 m²"</td></tr>
                                    <tr><td><code>"ionic_conductivity"</code>", "<code>"separator_thickness_um"</code></td><td>"S/m, µm"</td><td>"No transport loss"</td></tr>
                                    <tr><td><code>"thermal_mass_j_per_k"</code>", "<code>"thermal_resistance_k_per_w"</code></td><td>"J/K, K/W"</td><td>"625.5 J/K and 2.0 K/W"</td></tr>
                                    <tr><td><code>"ambient_temperature_k"</code></td><td>"K"</td><td>"298.15 K"</td></tr>
                                    <tr><td><code>"j_crit_a_per_m2"</code></td><td>"A/m²"</td><td>"Monroe-Newman estimate"</td></tr>
                                    <tr><td><code>"stack_pressure_mpa"</code></td><td>"MPa"</td><td>"2.0 MPa"</td></tr>
                                    <tr><td><code>"layer_count"</code></td><td>"layers"</td><td>"0, series-only mechanisms off"</td></tr>
                                    <tr><td><code>"cell_mass_kg"</code></td><td>"kg"</td><td>"No specific energy"</td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[electrochemical]
capacity_ah = 40.0
soc = 1.0
internal_resistance = 0.002      # ohm at 25 C
electrode_area_m2 = 0.74         # all layers
thermal_mass_j_per_k = 3000.0
thermal_resistance_k_per_w = 0.6
stack_pressure_mpa = 2.0
cell_mass_kg = 3.5

[thermodynamic]
temperature = 298.15             # K

[metadata]
class_name = "Part""#}</code></pre>
                            </div>
                            <p>"Every Play frame the model publishes 24 readouts as sim values:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Group"</th><th>"Keys"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Terminal"</td><td><code>"battery.voltage"</code>" (V), "<code>"battery.current"</code>" (A), "<code>"battery.power"</code>" (W), "<code>"battery.c_rate"</code>", "<code>"battery.resistance_ohm"</code></td></tr>
                                    <tr><td>"State"</td><td><code>"battery.soc"</code>", "<code>"battery.temperature_c"</code>", "<code>"battery.ambient_c"</code>", "<code>"battery.heat_generation"</code>" (W), "<code>"battery.cycle_count"</code></td></tr>
                                    <tr><td>"Safety"</td><td><code>"battery.dendrite_risk"</code>", "<code>"battery.pressure_ceiling_active"</code>" (1 while creep reaches the separator)"</td></tr>
                                    <tr><td>"Life"</td><td><code>"battery.capacity_retention"</code>", "<code>"battery.fade_lithium"</code>", "<code>"battery.li_inventory_lost"</code>", "<code>"battery.fade_cathode"</code>", "<code>"battery.fade_calendar"</code>", "<code>"battery.calendar_hours"</code>", "<code>"battery.fade_creep"</code>", "<code>"battery.creep_strain_total"</code>", "<code>"battery.fade_short"</code>", "<code>"battery.shorted_layers"</code></td></tr>
                                    <tr><td>"Cost"</td><td><code>"battery.reservoir_mass_g"</code>" (g), "<code>"battery.specific_energy_wh_kg"</code>" (Wh/kg)"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Because they are sim values, every run records all 24, and nine of them carry
                                watchpoints; see "<a href="/docs/simulation#values-map">"sim values"</a>" for the
                                keys the model reads back, "<code>"battery.mode"</code>" and "
                                <code>"battery.target_current"</code>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SCRIPTS & TOOLS
                    // =========================================================
                    <section id="scripts" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Scripts & Tools"
                        </h2>

                        <div id="scripts-rune" class="subsection">
                            <h3>"Rune Law Modules"</h3>
                            <p>
                                "Rune scripts reach the law library through 13 modules under "
                                <code>"eustress::realism"</code>". They take and return f64 values and wrap the
                                same Rust functions the engine uses:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Module"</th><th>"Covers"</th><th>"For example"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"thermodynamics"</code></td><td>"Gases, work, entropy, heat transfer, free energy"</td><td><code>"heat_radiation_rate"</code></td></tr>
                                    <tr><td><code>"thermocycles"</code></td><td>"Rankine, Brayton, Otto, refrigeration, heat exchangers"</td><td><code>"brayton_efficiency"</code></td></tr>
                                    <tr><td><code>"mechanics"</code></td><td>"Energy, gravity, orbits, friction, springs, inertia"</td><td><code>"orbital_velocity"</code></td></tr>
                                    <tr><td><code>"structures"</code></td><td>"Beams, columns, fatigue, fracture, composites"</td><td><code>"euler_critical_load"</code></td></tr>
                                    <tr><td><code>"electrical"</code></td><td>"Ohm's law, RC, RL and RLC, reactance, AC power"</td><td><code>"rlc_natural_frequency"</code></td></tr>
                                    <tr><td><code>"chemistry"</code></td><td>"Arrhenius, rate laws, equilibrium, pH, enzymes, combustion"</td><td><code>"arrhenius_rate"</code></td></tr>
                                    <tr><td><code>"propulsion"</code></td><td>"Rockets, jets, propellers, electric thrusters"</td><td><code>"tsiolkovsky_delta_v"</code></td></tr>
                                    <tr><td><code>"optics"</code></td><td>"Lenses, mirrors, interference, diffraction, photons"</td><td><code>"bragg_angle"</code></td></tr>
                                    <tr><td><code>"acoustics"</code></td><td>"Sound speed and level, Doppler, room acoustics"</td><td><code>"sabine_reverberation_time"</code></td></tr>
                                    <tr><td><code>"nuclear"</code></td><td>"Decay, shielding, criticality"</td><td><code>"critical_radius_sphere"</code></td></tr>
                                    <tr><td><code>"plasma"</code></td><td>"Debye length, MHD, fusion"</td><td><code>"lawson_triple_product"</code></td></tr>
                                    <tr><td><code>"control"</code></td><td>"Step response, damping, decibels"</td><td><code>"settling_time_2pct"</code></td></tr>
                                    <tr><td><code>"numerics"</code></td><td>"Interpolation, error function, distributions"</td><td><code>"gaussian_cdf"</code></td></tr>
                                </tbody>
                            </table>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::log_info;
use eustress::realism::structures;

pub fn on_init() {
    // A 2 m steel column, 50 mm square, pinned at both ends (K = 1).
    let i = structures::moment_of_area_rectangle(0.05, 0.05);         // m^4
    let p_cr = structures::euler_critical_load(200.0e9, i, 2.0, 1.0); // about 257 kN
    log_info(`Euler buckling load: ${p_cr} N`);

    // A 1 mm edge crack (Y = 1.12) in glass under 20 MPa of tension.
    let k = structures::stress_intensity_factor(20.0e6, 0.001, 1.12);
    if structures::fracture_occurs(k, 0.7e6) {
        log_info("K exceeds glass's K_IC of 0.7 MPa m^0.5: the crack runs.");
    }
}"#}</code></pre>
                            </div>
                            <p>
                                "Euler buckling is P_cr = π² E I / (K L)², and the stress intensity is K = Y σ √(π a).
                                See "<a href="/docs/scripting">"Scripting"</a>" for how scripts are attached and
                                run."
                            </p>
                        </div>

                        <div id="scripts-tools" class="subsection">
                            <h3>"Agent Tools"</h3>
                            <p>"Two tools of the "<a href="/learn/mcp">"MCP server"</a>" expose the library to agents:"</p>
                            <ul class="docs-list">
                                <li><code>"calculate_physics"</code>" evaluates one of nine formulas by name: "<code>"ideal_gas_pressure"</code>", "<code>"kinetic_energy"</code>", "<code>"gravitational_force"</code>", "<code>"heat_transfer_conduction"</code>", "<code>"nernst_potential"</code>", "<code>"escape_velocity"</code>", "<code>"spring_force"</code>", "<code>"drag_force"</code>" and "<code>"buoyancy_force"</code>"."</li>
                                <li><code>"query_material"</code>" returns a part material's appearance values and the mechanical constants of the preset it maps to, or says that no preset matches."</li>
                            </ul>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "calculate_physics", "arguments": {
    "equation": "nernst_potential",
    "params": {
        "standard_potential": 2.23,
        "temperature_k": 298.15,
        "electron_count": 2,
        "reaction_quotient": 1.0
    }
} }"#}</code></pre>
                            </div>
                            <p>
                                "Running, stepping and comparing models uses the simulation tools listed in "
                                <a href="/docs/simulation#experiments-tools">"Simulation"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // CASE STUDY: V-CELL
                    // =========================================================
                    <section id="vcell" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Case Study: V-Cell"
                        </h2>

                        <div id="vcell-setup" class="subsection">
                            <h3>"The Walkthrough"</h3>
                            <p>
                                "The V-Cell is Voltec's battery cell, and its case study is the end-to-end test of
                                the cell model and the tools around it. The walkthrough, "
                                <code>"docs/architecture/VCELL_CASE_STUDY.md"</code>" in the repository, sets out
                                to show that Eustress can detect a simulation anomaly, diagnose it with the
                                Workshop agent, correct it through MCP tools and verify the fix, with no manual
                                step after the run starts. The cell is the subject because it produces continuous
                                telemetry with well-defined safety thresholds: voltage, temperature and dendrite
                                risk."
                            </p>
                            <p>"Its prerequisites:"</p>
                            <ul class="docs-list">
                                <li>"A Universe with a Space containing a V-Cell prototype entity."</li>
                                <li>"The cell model producing "<code>"battery.*"</code>" sim values."</li>
                                <li>"A Rune SoulScript attached to the V-Cell entity."</li>
                                <li>"The Workshop panel open with a valid BYOK API key."</li>
                                <li>"The Watchman enabled, which it is by default."</li>
                            </ul>
                        </div>

                        <div id="vcell-loop" class="subsection">
                            <h3>"Detect, Repair, Verify"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Phase"</th><th>"What happens"</th><th>"Tools"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"1. Baseline"</td><td>"Start the run at 10x, confirm Play and the first readings"</td><td><code>"run_simulation"</code>", "<code>"get_simulation_state"</code>", "<code>"tail_telemetry"</code></td></tr>
                                    <tr><td>"2. Detection"</td><td>"When "<code>"battery.temperature_c"</code>" passes 60 °C, the Watchman catches it on its next 5-second poll and posts a "<code>"[Watchman Alert]"</code>" message to the Workshop, which Claude receives with full tool access"</td><td>"None, engine side"</td></tr>
                                    <tr><td>"3. Repair"</td><td>"Claude reads the temperature trend and the current state, then lowers the charge rate"</td><td><code>"tail_telemetry"</code>", "<code>"get_simulation_state"</code>", "<code>"feedback_diff"</code>", "<code>"read_file"</code>", "<code>"set_sim_value"</code></td></tr>
                                    <tr><td>"4. Verification"</td><td>"Confirm the temperature turns down; a 30-second cooldown prevents alert storms; stop the run, which exports the recording"</td><td><code>"tail_telemetry"</code>", "<code>"get_simulation_state"</code>", "<code>"stop_simulation"</code>", "<code>"query_audit_log"</code></td></tr>
                                    <tr><td>"5. Git loop"</td><td>"Branch "<code>"fix/thermal-runaway"</code>", add a thermal limiter to the script, commit, compare with "<code>"main"</code></td><td><code>"git_branch"</code>", "<code>"write_file"</code>", "<code>"git_status"</code>", "<code>"git_commit"</code>", "<code>"feedback_diff"</code></td></tr>
                                </tbody>
                            </table>
                            <p>"The limiter the walkthrough adds halves the current whenever the cell passes 55 °C. As a complete script:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{get_sim_value, set_sim_value};

pub fn on_update(dt) {
    let temp = get_sim_value("battery.temperature_c");
    if temp > 55.0 {
        let reduced = get_sim_value("battery.current") * 0.5;
        set_sim_value("battery.current", reduced);
    }
}"#}</code></pre>
                            </div>
                            <p>"A complete cycle is judged against the walkthrough's checklist:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Metric"</th><th>"Expected"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Lines in "<code>"telemetry.jsonl"</code></td><td>"At least 30 (30 or more seconds at 1 Hz)"</td></tr>
                                    <tr><td>"Watchman alerts fired"</td><td>"1 to 3"</td></tr>
                                    <tr><td>"Claude API calls in "<code>"query_audit_log"</code></td><td>"3 to 8 (alert, diagnosis, verification)"</td></tr>
                                    <tr><td>"Simulation commands processed"</td><td>"At least 2 (run and set_sim_value)"</td></tr>
                                    <tr><td>"Recordings exported"</td><td>"1 JSON file"</td></tr>
                                    <tr><td>"Completing a key that starts with "<code>"bat"</code>" in "<code>"get_sim_value"</code></td><td>"Offers "<code>"battery.voltage"</code>" and the other keys"</td></tr>
                                    <tr><td>"Branch after the git loop"</td><td><code>"fix/thermal-runaway"</code></td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="vcell-model" class="subsection">
                            <h3>"What Models It"</h3>
                            <p>
                                "Everything the walkthrough exercises is the machinery on this page and on "
                                <a href="/docs/simulation">"Simulation"</a>": the V-Cell's "
                                <code>"[electrochemical]"</code>" section drives the cell model, its readouts are
                                the "<code>"battery.*"</code>" sim values, telemetry is written once a second, the
                                runtime snapshot four times a second, and Stop exports the recording. The Watchman
                                runs in Studio during Play and ships with these thresholds:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Value"</th><th>"Alerts when"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"battery.temperature_c"</code></td><td>"Above 60 °C"</td></tr>
                                    <tr><td><code>"battery.voltage"</code></td><td>"Outside 1.8 to 2.5 V"</td></tr>
                                    <tr><td><code>"battery.soc"</code></td><td>"Outside 0 to 1"</td></tr>
                                    <tr><td><code>"battery.dendrite_risk"</code></td><td>"Above 0.8"</td></tr>
                                    <tr><td><code>"battery.capacity_retention"</code></td><td>"Below 0.7"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "It polls every 5 seconds, waits 30 seconds before repeating an alert for the same
                                value, and stops after 10 alerts in a run. In the Workshop, a "
                                <code>"set_sim_value"</code>" call runs without an approval prompt."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Lower the charge rate with the target current"</strong>
                                    <p>
                                        "Phase 3 of the walkthrough writes "<code>"battery.current"</code>" = -1.5
                                        with "<code>"set_sim_value"</code>". A current written by a tool is replaced
                                        before the cell model reads it, so from a tool set "
                                        <code>"battery.mode"</code>" to 1 and "<code>"battery.target_current"</code>
                                        " to 1.5, which charges at 1.5 A. Inside a script, as in the limiter above, "
                                        <code>"battery.current"</code>" takes effect directly."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"08"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-components" class="subsection">
                            <h3>"Laws Without a File Section"</h3>
                            <p>
                                "Several laws already run as systems but have no way in from a Space file:
                                continuous and batch chemical reactors, circuit elements with motors and power
                                converters, particle fluids with buoyancy and aerodynamic drag, and stress tensors on
                                parts. They will gain file sections or classes, so a part can carry a circuit or a
                                reactor the way it carries a cell today."
                            </p>
                            <p>
                                "The ARC-1 reactor model already exists as systems: one-group point kinetics,
                                thermal-hydraulics, power conversion, a battery buffer, a three-loop PID controller
                                and a scram monitor, publishing "<code>"arc1.*"</code>" watchpoints. A "
                                <code>"[nuclear]"</code>" section already records an "<code>"ArcReactorCore"</code>
                                "'s starting state. The plugin that attaches the reactor's components to such
                                instances is not yet added to the app; once it is, the reactor will run in any
                                Space."
                            </p>
                        </div>

                        <div id="roadmap-time" class="subsection">
                            <h3>"Simulated Time for Every Law"</h3>
                            <p>
                                "Only the cell model integrates against the simulation clock today. Heat
                                conduction, the reactor and the reaction and circuit systems step on frame time,
                                several of them limited to 0.05 s per frame, so a time scale does not speed them up.
                                Moving them onto the clock will make one time scale apply to every model in a run.
                                The GPU fluid path and the symbolic solver will follow."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Name the material, write the section, and the laws do the rest."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/simulation" class="btn-secondary-steel">"Simulation Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/simulation" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Simulation"</span>
                            </div>
                        </a>
                        <a href="/docs/universes" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Universes"</span>
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
