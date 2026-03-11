// =============================================================================
// Eustress Web - Realism Platform Documentation Page
// =============================================================================
// Comprehensive documentation on the Realism simulation platform covering
// STEM applications, business/economics, rapid prototyping, and industry use cases.
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
            id: "vision",
            title: "Vision",
            subsections: vec![
                TocSubsection { id: "vision-future", title: "The Future of Prototyping" },
                TocSubsection { id: "vision-why", title: "Why Eustress" },
                TocSubsection { id: "vision-philosophy", title: "Philosophy" },
            ],
        },
        TocSection {
            id: "stem",
            title: "STEM Applications",
            subsections: vec![
                TocSubsection { id: "stem-science", title: "Science" },
                TocSubsection { id: "stem-technology", title: "Technology" },
                TocSubsection { id: "stem-engineering", title: "Engineering" },
                TocSubsection { id: "stem-math", title: "Mathematics" },
            ],
        },
        TocSection {
            id: "industry",
            title: "Industry Solutions",
            subsections: vec![
                TocSubsection { id: "industry-manufacturing", title: "Manufacturing" },
                TocSubsection { id: "industry-warehouse", title: "Warehousing" },
                TocSubsection { id: "industry-supply", title: "Supply Chain" },
                TocSubsection { id: "industry-energy", title: "Energy Systems" },
            ],
        },
        TocSection {
            id: "business",
            title: "Business & Economics",
            subsections: vec![
                TocSubsection { id: "business-modeling", title: "Business Modeling" },
                TocSubsection { id: "business-economics", title: "Economic Simulation" },
                TocSubsection { id: "business-optimization", title: "Optimization" },
            ],
        },
        TocSection {
            id: "prototyping",
            title: "Rapid Prototyping",
            subsections: vec![
                TocSubsection { id: "prototyping-workflow", title: "Workflow" },
                TocSubsection { id: "prototyping-validation", title: "Validation" },
                TocSubsection { id: "prototyping-iteration", title: "Iteration" },
            ],
        },
        TocSection {
            id: "physics",
            title: "Physics Laws",
            subsections: vec![
                TocSubsection { id: "physics-mechanics", title: "Mechanics" },
                TocSubsection { id: "physics-thermo", title: "Thermodynamics" },
                TocSubsection { id: "physics-electro", title: "Electromagnetism" },
                TocSubsection { id: "physics-fluids", title: "Fluid Dynamics" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Realism Platform documentation page.
#[component]
pub fn DocsRealismPage() -> impl IntoView {
    let active_section = RwSignal::new("vision".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-realism"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/physics.svg" alt="Realism" class="toc-icon" />
                        <h2>"Realism Platform"</h2>
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
                            <span class="current">"Realism Platform"</span>
                        </div>
                        <h1 class="docs-title">"The Realism Platform"</h1>
                        <p class="docs-subtitle">
                            "The future of rapid prototyping. Simulate anything from batteries to 
                            factories to entire economies. Compress years into seconds. Validate 
                            before you build."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "40 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "All Levels"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // Vision
                    // ─────────────────────────────────────────────────────
                    <section id="vision" class="docs-section">
                        <h2 class="section-anchor">"Vision"</h2>

                        <div id="vision-future" class="docs-block">
                            <h3>"The Future of Prototyping"</h3>
                            <p>
                                "Imagine a world where you can test a product's 10-year lifespan in 
                                10 seconds. Where you can simulate an entire factory before laying a 
                                single brick. Where you can validate a supply chain across continents 
                                before signing a single contract."
                            </p>
                            <p>
                                <strong>"That world is here. That world is Eustress."</strong>
                            </p>
                            <div class="vision-stats">
                                <div class="vision-stat">
                                    <span class="stat-number">"31M×"</span>
                                    <span class="stat-label">"Time Compression"</span>
                                    <span class="stat-detail">"1 year per second"</span>
                                </div>
                                <div class="vision-stat">
                                    <span class="stat-number">"∞"</span>
                                    <span class="stat-label">"Scale"</span>
                                    <span class="stat-detail">"Atoms to galaxies"</span>
                                </div>
                                <div class="vision-stat">
                                    <span class="stat-number">"0"</span>
                                    <span class="stat-label">"Physical Prototypes"</span>
                                    <span class="stat-detail">"Until validated"</span>
                                </div>
                            </div>
                        </div>

                        <div id="vision-why" class="docs-block">
                            <h3>"Why Eustress"</h3>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Aspect"</th>
                                            <th>"Traditional"</th>
                                            <th>"Eustress"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"Prototype Cost"</td>
                                            <td>"$10,000 - $1M+"</td>
                                            <td>"$0 (simulation)"</td>
                                        </tr>
                                        <tr>
                                            <td>"Test Duration"</td>
                                            <td>"Months to years"</td>
                                            <td>"Seconds to minutes"</td>
                                        </tr>
                                        <tr>
                                            <td>"Iteration Speed"</td>
                                            <td>"Weeks per change"</td>
                                            <td>"Instant hot-reload"</td>
                                        </tr>
                                        <tr>
                                            <td>"Failure Analysis"</td>
                                            <td>"Post-mortem only"</td>
                                            <td>"Pause at any moment"</td>
                                        </tr>
                                        <tr>
                                            <td>"Parameter Sweeps"</td>
                                            <td>"Expensive, slow"</td>
                                            <td>"Automated, parallel"</td>
                                        </tr>
                                        <tr>
                                            <td>"Reproducibility"</td>
                                            <td>"Variable"</td>
                                            <td>"100% deterministic"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="vision-philosophy" class="docs-block">
                            <h3>"Philosophy"</h3>
                            <div class="philosophy-cards">
                                <div class="philosophy-card">
                                    <h4>"🎯 Accuracy Over Speed"</h4>
                                    <p>
                                        "We use real physics laws, not approximations. Nernst equation for 
                                        electrochemistry. Navier-Stokes for fluids. Fourier's law for heat 
                                        transfer. The simulation is only as good as its physics."
                                    </p>
                                </div>
                                <div class="philosophy-card">
                                    <h4>"📂 File-System-First"</h4>
                                    <p>
                                        "Your data is your data. Plain TOML files, version-controlled with 
                                        Git, editable in any text editor. No proprietary formats, no vendor 
                                        lock-in, no database servers."
                                    </p>
                                </div>
                                <div class="philosophy-card">
                                    <h4>"🔧 Composable"</h4>
                                    <p>
                                        "Build complex systems from simple components. A battery is cells, 
                                        electrodes, and electrolyte. A factory is machines, conveyors, and 
                                        workers. Compose at any scale."
                                    </p>
                                </div>
                                <div class="philosophy-card">
                                    <h4>"🚀 Accessible"</h4>
                                    <p>
                                        "Simulation shouldn't require a PhD. Soul Language lets you describe 
                                        behavior in plain English. Rune provides full control when you need it. 
                                        Start simple, grow complex."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // STEM Applications
                    // ─────────────────────────────────────────────────────
                    <section id="stem" class="docs-section">
                        <h2 class="section-anchor">"STEM Applications"</h2>

                        <div id="stem-science" class="docs-block">
                            <h3>"Science"</h3>
                            <p>
                                "Eustress is a laboratory without walls. Test hypotheses, run experiments, 
                                and analyze results — all in simulation."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Chemistry"</strong>" — Reaction kinetics, equilibrium, electrochemistry"</li>
                                <li><strong>"Physics"</strong>" — Mechanics, thermodynamics, electromagnetism"</li>
                                <li><strong>"Biology"</strong>" — Population dynamics, ecosystem modeling"</li>
                                <li><strong>"Earth Science"</strong>" — Climate modeling, hydrology, geology"</li>
                                <li><strong>"Astronomy"</strong>" — Orbital mechanics, stellar evolution"</li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"Example:"</strong>
                                " Simulate 1 million years of stellar evolution in under a minute. 
                                Watch a star form, burn, and die — with accurate nuclear physics."
                            </div>
                        </div>

                        <div id="stem-technology" class="docs-block">
                            <h3>"Technology"</h3>
                            <p>
                                "From microchips to data centers, simulate the technology that powers 
                                our world."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Electronics"</strong>" — Circuit simulation, thermal management"</li>
                                <li><strong>"Batteries"</strong>" — Cycle life, degradation, thermal runaway"</li>
                                <li><strong>"Robotics"</strong>" — Motion planning, sensor fusion, control systems"</li>
                                <li><strong>"Networks"</strong>" — Traffic simulation, latency modeling"</li>
                                <li><strong>"IoT"</strong>" — Sensor networks, edge computing"</li>
                            </ul>
                        </div>

                        <div id="stem-engineering" class="docs-block">
                            <h3>"Engineering"</h3>
                            <p>
                                "Design, test, and validate engineering systems before building them."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Mechanical"</strong>" — Stress analysis, fatigue, vibration"</li>
                                <li><strong>"Electrical"</strong>" — Power systems, motor drives, transformers"</li>
                                <li><strong>"Civil"</strong>" — Structural analysis, traffic flow, water systems"</li>
                                <li><strong>"Chemical"</strong>" — Process simulation, reactor design"</li>
                                <li><strong>"Aerospace"</strong>" — Flight dynamics, propulsion, thermal protection"</li>
                            </ul>
                            <pre class="code-block"><code>{"// Example: Structural fatigue analysis
sim.add_watchpoint(\"stress\", \"Von Mises Stress\", \"MPa\");
sim.add_watchpoint(\"cycles\", \"Load Cycles\", \"\");
sim.add_breakpoint(\"failure\", \"stress\", \">\", yield_strength);

// Run 10 million load cycles in seconds
sim.battery_test();  // 7.2M× compression
sim.run_until_tick(10000000);"}</code></pre>
                        </div>

                        <div id="stem-math" class="docs-block">
                            <h3>"Mathematics"</h3>
                            <p>
                                "Visualize and explore mathematical concepts in 3D."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Differential Equations"</strong>" — Visualize solutions in real-time"</li>
                                <li><strong>"Optimization"</strong>" — Gradient descent, genetic algorithms"</li>
                                <li><strong>"Statistics"</strong>" — Monte Carlo simulation, Bayesian inference"</li>
                                <li><strong>"Geometry"</strong>" — Parametric surfaces, fractals, topology"</li>
                                <li><strong>"Game Theory"</strong>" — Multi-agent simulations, Nash equilibria"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Industry Solutions
                    // ─────────────────────────────────────────────────────
                    <section id="industry" class="docs-section">
                        <h2 class="section-anchor">"Industry Solutions"</h2>

                        <div id="industry-manufacturing" class="docs-block">
                            <h3>"Manufacturing"</h3>
                            <p>
                                "Design and optimize factories before breaking ground."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-item">
                                    <h4>"🏭 Factory Layout"</h4>
                                    <p>"Optimize machine placement, material flow, and worker paths"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"⚙️ Production Lines"</h4>
                                    <p>"Balance cycle times, identify bottlenecks, maximize throughput"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"🔧 Equipment Wear"</h4>
                                    <p>"Predict maintenance needs, optimize replacement schedules"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"📊 OEE Analysis"</h4>
                                    <p>"Track availability, performance, and quality metrics"</p>
                                </div>
                            </div>
                            <pre class="code-block"><code>{"// Example: Production line simulation
let machines = [\"CNC_1\", \"CNC_2\", \"Assembly\", \"QC\"];
for machine in machines {
    sim.add_watchpoint(machine + \"_utilization\", machine, \"%\");
    sim.add_watchpoint(machine + \"_queue\", machine + \" Queue\", \"parts\");
}

// Run 1 year of production in 1 second
sim.fast_year();
sim.run_years(1.0);

// Analyze results
let bottleneck = sim.find_max(\"*_queue\");
log(\"Bottleneck: \" + bottleneck);"}</code></pre>
                        </div>

                        <div id="industry-warehouse" class="docs-block">
                            <h3>"Warehousing"</h3>
                            <p>
                                "Optimize storage, picking, and fulfillment operations."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Layout Optimization"</strong>" — Minimize travel distance, maximize storage density"</li>
                                <li><strong>"Pick Path Planning"</strong>" — Optimize order batching and routing"</li>
                                <li><strong>"Inventory Placement"</strong>" — ABC analysis, slotting optimization"</li>
                                <li><strong>"Labor Planning"</strong>" — Staffing levels, shift scheduling"</li>
                                <li><strong>"Automation ROI"</strong>" — Compare manual vs. automated systems"</li>
                            </ul>
                        </div>

                        <div id="industry-supply" class="docs-block">
                            <h3>"Supply Chain"</h3>
                            <p>
                                "Model global supply chains with realistic disruptions and constraints."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Network Design"</strong>" — Facility location, transportation modes"</li>
                                <li><strong>"Inventory Optimization"</strong>" — Safety stock, reorder points"</li>
                                <li><strong>"Demand Forecasting"</strong>" — Seasonal patterns, trend analysis"</li>
                                <li><strong>"Risk Analysis"</strong>" — Supplier disruptions, natural disasters"</li>
                                <li><strong>"Cost Modeling"</strong>" — Total landed cost, make-vs-buy decisions"</li>
                            </ul>
                            <div class="docs-callout success">
                                <strong>"Real-World Impact:"</strong>
                                " One customer reduced inventory carrying costs by 23% after simulating 
                                their supply chain with Eustress and optimizing safety stock levels."
                            </div>
                        </div>

                        <div id="industry-energy" class="docs-block">
                            <h3>"Energy Systems"</h3>
                            <p>
                                "Design and optimize power generation, storage, and distribution."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Battery Systems"</strong>" — Pack design, BMS algorithms, thermal management"</li>
                                <li><strong>"Solar/Wind"</strong>" — Generation profiles, grid integration"</li>
                                <li><strong>"Microgrids"</strong>" — Load balancing, islanding, resilience"</li>
                                <li><strong>"HVAC"</strong>" — Building energy modeling, equipment sizing"</li>
                                <li><strong>"Grid Simulation"</strong>" — Power flow, stability analysis"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Business & Economics
                    // ─────────────────────────────────────────────────────
                    <section id="business" class="docs-section">
                        <h2 class="section-anchor">"Business & Economics"</h2>

                        <div id="business-modeling" class="docs-block">
                            <h3>"Business Modeling"</h3>
                            <p>
                                "Simulate business operations and test strategies before implementation."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Financial Modeling"</strong>" — Cash flow, P&L, balance sheet projections"</li>
                                <li><strong>"Pricing Strategy"</strong>" — Elasticity analysis, competitive response"</li>
                                <li><strong>"Capacity Planning"</strong>" — Growth scenarios, resource allocation"</li>
                                <li><strong>"M&A Analysis"</strong>" — Integration scenarios, synergy modeling"</li>
                                <li><strong>"Risk Assessment"</strong>" — Monte Carlo simulation, sensitivity analysis"</li>
                            </ul>
                        </div>

                        <div id="business-economics" class="docs-block">
                            <h3>"Economic Simulation"</h3>
                            <p>
                                "Model economic systems from individual agents to entire markets."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Agent-Based Modeling"</strong>" — Individual behavior, emergent phenomena"</li>
                                <li><strong>"Market Dynamics"</strong>" — Supply/demand, price discovery"</li>
                                <li><strong>"Policy Analysis"</strong>" — Tax impacts, regulatory effects"</li>
                                <li><strong>"Trade Flows"</strong>" — International commerce, tariff modeling"</li>
                                <li><strong>"Labor Markets"</strong>" — Employment dynamics, wage effects"</li>
                            </ul>
                            <pre class="code-block"><code>{"// Example: Market simulation
struct Agent {
    budget: f64,
    demand_curve: fn(f64) -> f64,
}

// Spawn 10,000 agents with varied preferences
for i in 0..10000 {
    let agent = Agent {
        budget: random_normal(50000.0, 15000.0),
        demand_curve: |price| 100.0 - price * 0.5,
    };
    spawn_agent(agent);
}

// Run market simulation
sim.fast_year();
sim.run_years(10.0);

// Analyze equilibrium
let eq_price = sim.get(\"market_price\");
let eq_quantity = sim.get(\"market_quantity\");"}</code></pre>
                        </div>

                        <div id="business-optimization" class="docs-block">
                            <h3>"Optimization"</h3>
                            <p>
                                "Find optimal solutions to complex business problems."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Linear Programming"</strong>" — Resource allocation, blending problems"</li>
                                <li><strong>"Integer Programming"</strong>" — Scheduling, assignment problems"</li>
                                <li><strong>"Genetic Algorithms"</strong>" — Complex, non-linear optimization"</li>
                                <li><strong>"Simulated Annealing"</strong>" — Global optimization, avoiding local minima"</li>
                                <li><strong>"Multi-Objective"</strong>" — Pareto frontiers, trade-off analysis"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Rapid Prototyping
                    // ─────────────────────────────────────────────────────
                    <section id="prototyping" class="docs-section">
                        <h2 class="section-anchor">"Rapid Prototyping"</h2>

                        <div id="prototyping-workflow" class="docs-block">
                            <h3>"Workflow"</h3>
                            <p>
                                "The Eustress rapid prototyping workflow:"
                            </p>
                            <ol class="docs-list numbered">
                                <li><strong>"Define"</strong>" — Describe your system in TOML files"</li>
                                <li><strong>"Simulate"</strong>" — Run time-compressed simulations"</li>
                                <li><strong>"Analyze"</strong>" — Review watchpoints, breakpoints, reports"</li>
                                <li><strong>"Iterate"</strong>" — Modify parameters, re-run instantly"</li>
                                <li><strong>"Validate"</strong>" — Compare against real-world data"</li>
                                <li><strong>"Build"</strong>" — Only build physical prototypes after validation"</li>
                            </ol>
                            <div class="workflow-diagram">
                                <div class="workflow-step">"Define"</div>
                                <div class="workflow-arrow">"→"</div>
                                <div class="workflow-step">"Simulate"</div>
                                <div class="workflow-arrow">"→"</div>
                                <div class="workflow-step">"Analyze"</div>
                                <div class="workflow-arrow">"→"</div>
                                <div class="workflow-step loop">"Iterate"</div>
                                <div class="workflow-arrow">"→"</div>
                                <div class="workflow-step">"Validate"</div>
                                <div class="workflow-arrow">"→"</div>
                                <div class="workflow-step final">"Build"</div>
                            </div>
                        </div>

                        <div id="prototyping-validation" class="docs-block">
                            <h3>"Validation"</h3>
                            <p>
                                "Validate simulation results against real-world data:"
                            </p>
                            <pre class="code-block"><code>{"// Load reference data
let reference = load_csv(\"test_data/battery_cycle_test.csv\");

// Run simulation with same conditions
sim.set_param(\"temperature\", 25.0);
sim.set_param(\"c_rate\", 1.0);
sim.run_cycles(1000);

// Compare results
let sim_capacity = sim.get(\"capacity\");
let ref_capacity = reference.get(\"capacity\", 1000);
let error = abs(sim_capacity - ref_capacity) / ref_capacity * 100.0;

log(\"Simulation error: \" + error.round(2) + \"%\");
assert(error < 5.0, \"Simulation accuracy within 5%\");"}</code></pre>
                        </div>

                        <div id="prototyping-iteration" class="docs-block">
                            <h3>"Iteration"</h3>
                            <p>
                                "Iterate at the speed of thought with hot-reload:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Parameter Changes"</strong>" — Edit TOML, see results instantly"</li>
                                <li><strong>"Script Changes"</strong>" — Modify Rune, hot-reload without restart"</li>
                                <li><strong>"Geometry Changes"</strong>" — Update glTF, auto-reload in scene"</li>
                                <li><strong>"A/B Testing"</strong>" — Run parallel simulations with different configs"</li>
                                <li><strong>"Parameter Sweeps"</strong>" — Automated exploration of design space"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Physics Laws
                    // ─────────────────────────────────────────────────────
                    <section id="physics" class="docs-section">
                        <h2 class="section-anchor">"Physics Laws"</h2>

                        <div id="physics-mechanics" class="docs-block">
                            <h3>"Mechanics"</h3>
                            <p>
                                "Rigid body dynamics, constraints, and collisions powered by Avian3D:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Newton's Laws"</strong>" — F = ma, momentum conservation"</li>
                                <li><strong>"Constraints"</strong>" — Joints, hinges, sliders, springs"</li>
                                <li><strong>"Collisions"</strong>" — Continuous detection, restitution, friction"</li>
                                <li><strong>"Soft Bodies"</strong>" — Deformation, stress, strain"</li>
                            </ul>
                        </div>

                        <div id="physics-thermo" class="docs-block">
                            <h3>"Thermodynamics"</h3>
                            <p>
                                "Heat transfer and thermal behavior:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Fourier's Law"</strong>" — Heat conduction through solids"</li>
                                <li><strong>"Newton's Cooling"</strong>" — Convective heat transfer"</li>
                                <li><strong>"Stefan-Boltzmann"</strong>" — Radiative heat transfer"</li>
                                <li><strong>"Phase Changes"</strong>" — Melting, boiling, sublimation"</li>
                                <li><strong>"Ideal Gas Law"</strong>" — PV = nRT relationships"</li>
                            </ul>
                        </div>

                        <div id="physics-electro" class="docs-block">
                            <h3>"Electromagnetism"</h3>
                            <p>
                                "Electrical and electrochemical systems:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Ohm's Law"</strong>" — V = IR, resistance networks"</li>
                                <li><strong>"Kirchhoff's Laws"</strong>" — Current and voltage loops"</li>
                                <li><strong>"Nernst Equation"</strong>" — Electrochemical equilibrium"</li>
                                <li><strong>"Butler-Volmer"</strong>" — Electrode kinetics"</li>
                                <li><strong>"Arrhenius"</strong>" — Temperature dependence"</li>
                            </ul>
                        </div>

                        <div id="physics-fluids" class="docs-block">
                            <h3>"Fluid Dynamics"</h3>
                            <p>
                                "Fluid flow and pressure systems (via Garbongus integration):"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Navier-Stokes"</strong>" — Viscous fluid motion"</li>
                                <li><strong>"Bernoulli"</strong>" — Energy conservation in flow"</li>
                                <li><strong>"Darcy-Weisbach"</strong>" — Pressure drop in pipes"</li>
                                <li><strong>"Pump Curves"</strong>" — Head vs. flow characteristics"</li>
                                <li><strong>"Cavitation"</strong>" — Vapor pressure effects"</li>
                            </ul>
                        </div>
                    </section>

                    // Footer
                    <footer class="docs-footer">
                        <div class="docs-nav-links">
                            <a href="/docs/simulation" class="nav-prev">
                                <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                                <span>"Simulation"</span>
                            </a>
                            <a href="/docs/philosophy" class="nav-next">
                                <span>"Philosophy"</span>
                                <img src="/assets/icons/arrow-right.svg" alt="Next" />
                            </a>
                        </div>
                    </footer>
                </main>
            </div>

            <Footer />
        </div>
    }
}
