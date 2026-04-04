// =============================================================================
// Eustress Web - Simulation Documentation Page
// =============================================================================
// Comprehensive simulation documentation covering tick-based time compression,
// watchpoints, breakpoints, data recording, and the Rune simulation API.
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
                TocSubsection { id: "overview-benefits", title: "Benefits" },
                TocSubsection { id: "overview-usecases", title: "Use Cases" },
            ],
        },
        TocSection {
            id: "studio",
            title: "Studio Controls",
            subsections: vec![
                TocSubsection { id: "studio-play", title: "Play/Pause/Stop" },
                TocSubsection { id: "studio-timescale", title: "Time Scale" },
                TocSubsection { id: "studio-sandbox", title: "Sandboxed Testing" },
            ],
        },
        TocSection {
            id: "tick",
            title: "Tick System",
            subsections: vec![
                TocSubsection { id: "tick-clock", title: "Simulation Clock" },
                TocSubsection { id: "tick-compression", title: "Time Compression" },
                TocSubsection { id: "tick-presets", title: "Presets" },
            ],
        },
        TocSection {
            id: "observability",
            title: "Observability",
            subsections: vec![
                TocSubsection { id: "observability-watchpoints", title: "WatchPoints" },
                TocSubsection { id: "observability-breakpoints", title: "BreakPoints" },
                TocSubsection { id: "observability-recorder", title: "Data Recorder" },
                TocSubsection { id: "observability-reports", title: "Reports" },
            ],
        },
        TocSection {
            id: "rune",
            title: "Rune Simulation API",
            subsections: vec![
                TocSubsection { id: "rune-control", title: "Time Control" },
                TocSubsection { id: "rune-watchpoints", title: "WatchPoint API" },
                TocSubsection { id: "rune-breakpoints", title: "BreakPoint API" },
                TocSubsection { id: "rune-recording", title: "Recording API" },
            ],
        },
        TocSection {
            id: "config",
            title: "Configuration",
            subsections: vec![
                TocSubsection { id: "config-toml", title: "simulation.toml" },
                TocSubsection { id: "config-tests", title: "Test Suites" },
                TocSubsection { id: "config-parameters", title: "Parameters" },
            ],
        },
        TocSection {
            id: "realism",
            title: "Realism Physics",
            subsections: vec![
                TocSubsection { id: "realism-materials", title: "Material Properties" },
                TocSubsection { id: "realism-thermo", title: "Thermodynamics" },
                TocSubsection { id: "realism-electro", title: "Electrochemistry" },
                TocSubsection { id: "realism-fluids", title: "Fluid Dynamics" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Simulation documentation page with floating TOC.
#[component]
pub fn DocsSimulationPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-simulation"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/physics.svg" alt="Simulation" class="toc-icon" />
                        <h2>"Simulation"</h2>
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
                            <span class="current">"Simulation"</span>
                        </div>
                        <h1 class="docs-title">"Simulation System"</h1>
                        <p class="docs-subtitle">
                            "Compress years of testing into seconds. Run physics simulations at 
                            31 million times real-time. Validate prototypes before building them."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "35 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate to Advanced"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // Overview
                    // ─────────────────────────────────────────────────────
                    <section id="overview" class="docs-section">
                        <h2 class="section-anchor">"Overview"</h2>

                        <div id="overview-intro" class="docs-block">
                            <h3>"Introduction"</h3>
                            <p>
                                "Eustress Engine's simulation system is designed for one purpose: "
                                <strong>"rapid prototyping through time compression"</strong>". Instead of 
                                waiting months for real-world tests, compress years of simulated time 
                                into seconds of wall-clock time."
                            </p>
                            <div class="docs-callout info">
                                <strong>"The Power of Time Compression:"</strong>
                                " A battery that takes 2 years to reach end-of-life in the real world 
                                can be simulated in under 10 seconds. Run 10,000 charge/discharge cycles 
                                before lunch."
                            </div>
                        </div>

                        <div id="overview-benefits" class="docs-block">
                            <h3>"Benefits"</h3>
                            <div class="benefit-grid">
                                <div class="benefit-card">
                                    <div class="benefit-icon">"⚡"</div>
                                    <h4>"Speed"</h4>
                                    <p>"Up to 31 million times faster than real-time. Years become seconds."</p>
                                </div>
                                <div class="benefit-card">
                                    <div class="benefit-icon">"🔬"</div>
                                    <h4>"Accuracy"</h4>
                                    <p>"Physics-based simulation using real material properties and thermodynamics."</p>
                                </div>
                                <div class="benefit-card">
                                    <div class="benefit-icon">"📊"</div>
                                    <h4>"Observability"</h4>
                                    <p>"WatchPoints, BreakPoints, and detailed reports for every variable."</p>
                                </div>
                                <div class="benefit-card">
                                    <div class="benefit-icon">"🔄"</div>
                                    <h4>"Reproducibility"</h4>
                                    <p>"Deterministic simulation. Same inputs always produce same outputs."</p>
                                </div>
                                <div class="benefit-card">
                                    <div class="benefit-icon">"💰"</div>
                                    <h4>"Cost Savings"</h4>
                                    <p>"Validate designs before building physical prototypes. Catch issues early."</p>
                                </div>
                                <div class="benefit-card">
                                    <div class="benefit-icon">"🎯"</div>
                                    <h4>"Precision"</h4>
                                    <p>"Pause at any moment, inspect any variable, set breakpoints on conditions."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-usecases" class="docs-block">
                            <h3>"Use Cases"</h3>
                            <p>"Eustress simulation powers innovation across industries:"</p>
                            <ul class="docs-list">
                                <li><strong>"Battery Testing"</strong>" — Cycle life, degradation, thermal runaway prediction"</li>
                                <li><strong>"Robotics"</strong>" — Motion planning, wear analysis, OEE optimization"</li>
                                <li><strong>"Manufacturing"</strong>" — Factory layout, throughput simulation, bottleneck detection"</li>
                                <li><strong>"Supply Chain"</strong>" — Inventory optimization, demand forecasting, disruption modeling"</li>
                                <li><strong>"HVAC Systems"</strong>" — Thermal comfort, energy efficiency, equipment sizing"</li>
                                <li><strong>"Fluid Systems"</strong>" — Pump curves, cavitation analysis, pressure drop"</li>
                                <li><strong>"Scientific Research"</strong>" — Hypothesis testing, parameter sweeps, sensitivity analysis"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Studio Controls
                    // ─────────────────────────────────────────────────────
                    <section id="studio" class="docs-section">
                        <h2 class="section-anchor">"Studio Controls"</h2>

                        <div id="studio-play" class="docs-block">
                            <h3>"Play/Pause/Stop"</h3>
                            <p>
                                "The ribbon toolbar provides intuitive controls for simulation:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Button"</th>
                                        <th>"Shortcut"</th>
                                        <th>"Action"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"▶ Play"</td>
                                        <td><code>"F5"</code></td>
                                        <td>"Start simulation with character"</td>
                                    </tr>
                                    <tr>
                                        <td>"▶ Run"</td>
                                        <td><code>"F7"</code></td>
                                        <td>"Start simulation (free camera)"</td>
                                    </tr>
                                    <tr>
                                        <td>"⏸ Pause"</td>
                                        <td><code>"F6"</code></td>
                                        <td>"Pause/resume simulation"</td>
                                    </tr>
                                    <tr>
                                        <td>"⏹ Stop"</td>
                                        <td><code>"F8"</code>" / "<code>"Esc"</code></td>
                                        <td>"Stop and restore world state"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <div class="docs-callout success">
                                <strong>"Instant Feedback:"</strong>
                                " When paused, you can inspect any entity's properties, modify values, 
                                and resume without losing simulation state."
                            </div>
                        </div>

                        <div id="studio-timescale" class="docs-block">
                            <h3>"Time Scale"</h3>
                            <p>
                                "Control how fast simulation time passes relative to wall-clock time:"
                            </p>
                            <pre class="code-block"><code>{"// From Rune script
sim.realtime();           // 1x — real-time
sim.fast_hour();          // 3,600x — 1 hour per second
sim.fast_day();           // 86,400x — 1 day per second
sim.fast_year();          // 31,536,000x — 1 year per second
sim.battery_test();       // 7,200,000x — 10,000 cycles in ~10s

// Custom scale
sim.set_time_scale(1000000.0);  // 1 million x"}</code></pre>
                        </div>

                        <div id="studio-sandbox" class="docs-block">
                            <h3>"Sandboxed Testing"</h3>
                            <p>
                                "Every Play session is sandboxed. When you press Stop, the world 
                                reverts to its exact state before Play was pressed:"
                            </p>
                            <ol class="docs-list numbered">
                                <li>"Press Play — world state is captured (snapshot + binary)"</li>
                                <li>"Run simulation — entities move, physics runs, scripts execute"</li>
                                <li>"Press Stop — world is restored from snapshot"</li>
                                <li>"All changes during play are discarded (like a git revert)"</li>
                            </ol>
                            <p>
                                "This means you can test destructive scenarios (explosions, collisions, 
                                failures) without fear of losing your work."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Tick System
                    // ─────────────────────────────────────────────────────
                    <section id="tick" class="docs-section">
                        <h2 class="section-anchor">"Tick System"</h2>

                        <div id="tick-clock" class="docs-block">
                            <h3>"Simulation Clock"</h3>
                            <p>
                                "The simulation clock tracks two separate timelines:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Simulation Time"</strong>" — Virtual time in the simulated world"</li>
                                <li><strong>"Wall Time"</strong>" — Real elapsed time on your computer"</li>
                            </ul>
                            <pre class="code-block"><code>{"// Access from Rune
let sim_seconds = sim.time();        // Simulation seconds
let sim_hours = sim.time_hours();    // Simulation hours
let sim_days = sim.time_days();      // Simulation days
let sim_years = sim.time_years();    // Simulation years

let wall_seconds = sim.wall_time();  // Real elapsed seconds
let tick = sim.tick();               // Current tick number
let dt = sim.dt();                   // Timestep per tick

// Formatted output
let formatted = sim.format_time();   // \"2.5y\" or \"3.2h\""}</code></pre>
                        </div>

                        <div id="tick-compression" class="docs-block">
                            <h3>"Time Compression"</h3>
                            <p>
                                "Time compression is the ratio of simulation time to wall time. At 
                                1,000,000x compression, one second of wall time advances the simulation 
                                by 1 million seconds (~11.5 days)."
                            </p>
                            <pre class="code-block"><code>{"// Check compression ratio
let ratio = sim.compression_ratio();
log(\"Running at \" + ratio.round(0) + \"x speed\");

// Example: Battery cycle test
// 1000 cycles × 2 hours/cycle = 2000 hours
// At 7,200,000x: 2000 × 3600 / 7200000 = 1 second wall time"}</code></pre>
                        </div>

                        <div id="tick-presets" class="docs-block">
                            <h3>"Presets"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Preset"</th>
                                        <th>"Scale"</th>
                                        <th>"Use Case"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><code>"REALTIME"</code></td>
                                        <td>"1x"</td>
                                        <td>"Normal operation, debugging"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"FAST_1MIN_PER_SEC"</code></td>
                                        <td>"60x"</td>
                                        <td>"Quick preview"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"FAST_1HOUR_PER_SEC"</code></td>
                                        <td>"3,600x"</td>
                                        <td>"Thermal cycling"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"FAST_1DAY_PER_SEC"</code></td>
                                        <td>"86,400x"</td>
                                        <td>"Calendar aging"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"FAST_1WEEK_PER_SEC"</code></td>
                                        <td>"604,800x"</td>
                                        <td>"Long-term tests"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"FAST_1YEAR_PER_SEC"</code></td>
                                        <td>"31,536,000x"</td>
                                        <td>"Lifetime analysis"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"BATTERY_CYCLE_TEST"</code></td>
                                        <td>"7,200,000x"</td>
                                        <td>"10,000 cycles in ~10s"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Observability
                    // ─────────────────────────────────────────────────────
                    <section id="observability" class="docs-section">
                        <h2 class="section-anchor">"Observability"</h2>

                        <div id="observability-watchpoints" class="docs-block">
                            <h3>"WatchPoints"</h3>
                            <p>
                                "WatchPoints track variables over time, recording history for graphing 
                                and analysis:"
                            </p>
                            <pre class="code-block"><code>{"// Register a watchpoint
sim.add_watchpoint(\"voltage\", \"Cell Voltage\", \"V\");
sim.add_watchpoint(\"soc\", \"State of Charge\", \"%\");
sim.add_watchpoint(\"temperature\", \"Temperature\", \"°C\");

// Record values each tick
pub fn on_tick() {
    let v = ecs.get_sim(\"battery.voltage\");
    let s = ecs.get_sim(\"battery.soc\") * 100.0;
    let t = ecs.get_sim(\"battery.temperature_c\");
    
    sim.record(\"voltage\", v);
    sim.record(\"soc\", s);
    sim.record(\"temperature\", t);
}

// Query statistics
let min_v = sim.get_min(\"voltage\");
let max_v = sim.get_max(\"voltage\");
let avg_v = sim.get_avg(\"voltage\");"}</code></pre>
                        </div>

                        <div id="observability-breakpoints" class="docs-block">
                            <h3>"BreakPoints"</h3>
                            <p>
                                "BreakPoints pause the simulation when conditions are met:"
                            </p>
                            <pre class="code-block"><code>{"// Add breakpoints
sim.add_breakpoint(\"low_soc\", \"soc\", \"<\", 20.0);
sim.add_breakpoint(\"high_temp\", \"temperature\", \">\", 60.0);
sim.add_breakpoint(\"eol\", \"capacity\", \"<\", 80.0);

// Comparison operators: <, <=, ==, >=, >, !=

// Control breakpoints
sim.enable_breakpoint(\"low_soc\", false);  // Disable
sim.remove_breakpoint(\"high_temp\");       // Remove

// When a breakpoint triggers:
// 1. Simulation pauses automatically
// 2. Log message shows which breakpoint hit
// 3. You can inspect state, then resume"}</code></pre>
                            <div class="docs-callout warning">
                                <strong>"One-Shot Breakpoints:"</strong>
                                " Add "<code>".one_shot()"</code>" to breakpoints that should only 
                                trigger once (like end-of-life detection)."
                            </div>
                        </div>

                        <div id="observability-recorder" class="docs-block">
                            <h3>"Data Recorder"</h3>
                            <p>
                                "Record complete simulation runs for later analysis:"
                            </p>
                            <pre class="code-block"><code>{"// Start recording
sim.start_recording(\"cycle_life_test\");

// ... run simulation ...

// Stop and export
sim.stop_recording();
sim.export(\"recordings/cycle_life_test.json\");

// Export formats:
// - JSON: Full data with metadata and statistics
// - CSV: One file per variable, easy to import to Excel/Python"}</code></pre>
                        </div>

                        <div id="observability-reports" class="docs-block">
                            <h3>"Reports"</h3>
                            <p>
                                "Generate comprehensive simulation reports:"
                            </p>
                            <pre class="code-block"><code>{"// Auto-generated report includes:
// - Simulation duration (simulated and wall time)
// - Compression ratio achieved
// - Total ticks executed
// - Per-variable statistics (min, max, mean, std dev)
// - Events that occurred
// - Breakpoints that triggered

// Example output:
# Simulation Report: cycle_life_test

## Summary
- Simulation Duration: 2000.00 hours (83.33 days)
- Wall Time: 1.2 seconds
- Compression Ratio: 6,000,000x
- Total Ticks: 72,000

## Variables
### Cell Voltage (V)
- Min: 2.50
- Max: 4.20
- Mean: 3.65
- Std Dev: 0.42

### Capacity Retention (%)
- Start: 100.00
- End: 79.50
- Degradation: 20.50%"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Rune Simulation API
                    // ─────────────────────────────────────────────────────
                    <section id="rune" class="docs-section">
                        <h2 class="section-anchor">"Rune Simulation API"</h2>

                        <div id="rune-control" class="docs-block">
                            <h3>"Time Control"</h3>
                            <pre class="code-block"><code>{"// Basic controls
sim.pause();              // Pause simulation
sim.resume();             // Resume simulation
sim.step();               // Execute single tick
sim.step_n(100);          // Execute 100 ticks
sim.reset();              // Reset to initial state

// Time scale
sim.set_time_scale(scale);  // Set custom scale
sim.realtime();             // 1x
sim.fast_hour();            // 3,600x
sim.fast_day();             // 86,400x
sim.fast_year();            // 31,536,000x
sim.battery_test();         // 7,200,000x

// Run until target
sim.run_until_time(3600.0);   // Run until 1 hour simulated
sim.run_until_tick(10000);    // Run until tick 10000
sim.run_hours(24.0);          // Run for 24 simulated hours
sim.run_days(7.0);            // Run for 7 simulated days
sim.run_years(2.0);           // Run for 2 simulated years"}</code></pre>
                        </div>

                        <div id="rune-watchpoints" class="docs-block">
                            <h3>"WatchPoint API"</h3>
                            <pre class="code-block"><code>{"// Create watchpoints
sim.add_watchpoint(name, label, unit);

// Record values
sim.record(name, value);

// Query current value
let current = sim.get(name);

// Query statistics
let min = sim.get_min(name);
let max = sim.get_max(name);
let avg = sim.get_avg(name);

// Get all watchpoint names
let names = sim.watchpoint_names();"}</code></pre>
                        </div>

                        <div id="rune-breakpoints" class="docs-block">
                            <h3>"BreakPoint API"</h3>
                            <pre class="code-block"><code>{"// Create breakpoints
sim.add_breakpoint(name, variable, comparison, threshold);
// comparison: \"<\", \"<=\", \"==\", \">=\", \">\", \"!=\"

// Control breakpoints
sim.enable_breakpoint(name, enabled);
sim.remove_breakpoint(name);

// Example: Stop when battery reaches end-of-life
sim.add_breakpoint(\"eol\", \"capacity\", \"<\", 80.0);"}</code></pre>
                        </div>

                        <div id="rune-recording" class="docs-block">
                            <h3>"Recording API"</h3>
                            <pre class="code-block"><code>{"// Start/stop recording
sim.start_recording(name);
sim.stop_recording();

// Export data
sim.export(path);  // JSON or CSV based on extension

// Query state
let running = sim.is_running();
let paused = sim.is_paused();
let completed = sim.is_completed();
let reason = sim.completion_reason();"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Configuration
                    // ─────────────────────────────────────────────────────
                    <section id="config" class="docs-section">
                        <h2 class="section-anchor">"Configuration"</h2>

                        <div id="config-toml" class="docs-block">
                            <h3>"simulation.toml"</h3>
                            <p>
                                "Configure simulation parameters in a TOML file:"
                            </p>
                            <pre class="code-block"><code>{"# simulation.toml

[simulation]
tick_rate_hz = 60.0
time_scale = 3600000.0  # 1 hour per second
max_ticks_per_frame = 10
auto_start = false

[simulation.recording]
enabled = true
output_dir = \"recordings\"
format = \"both\"  # json, csv, or both
auto_export = true

[[watchpoints]]
name = \"voltage\"
label = \"Cell Voltage\"
unit = \"V\"
interval = 1
color = \"#4CAF50\"

[[watchpoints]]
name = \"temperature\"
label = \"Temperature\"
unit = \"°C\"
interval = 1
color = \"#F44336\"

[[breakpoints]]
name = \"high_temp\"
variable = \"temperature\"
comparison = \">\"
threshold = 60.0
one_shot = true

[parameters]
nominal_voltage = 3.7
capacity_ah = 100.0
target_cycles = 1000.0"}</code></pre>
                        </div>

                        <div id="config-tests" class="docs-block">
                            <h3>"Test Suites"</h3>
                            <p>
                                "Define automated test scenarios:"
                            </p>
                            <pre class="code-block"><code>{"[[tests]]
name = \"cycle_life_test\"
description = \"Run 1000 charge/discharge cycles\"
script = \"scripts/cycle_life_test.rune\"
time_scale = 7200000.0
max_time_s = 7200000.0

[tests.expected]
capacity = { min = 75.0, max = 100.0 }
cycles = { value = 1000.0, tolerance = 10.0 }

[[tests]]
name = \"thermal_stress_test\"
description = \"Test behavior under thermal cycling\"
script = \"scripts/thermal_stress_test.rune\"
time_scale = 86400.0
max_time_s = 604800.0

[tests.expected]
temperature = { min = -20.0, max = 60.0 }"}</code></pre>
                        </div>

                        <div id="config-parameters" class="docs-block">
                            <h3>"Parameters"</h3>
                            <p>
                                "Access configuration parameters from Rune scripts:"
                            </p>
                            <pre class="code-block"><code>{"// Read parameters from simulation.toml
let voltage = sim.param(\"nominal_voltage\");
let capacity = sim.param(\"capacity_ah\");
let target = sim.param(\"target_cycles\");

// Use in simulation logic
if current_cycles >= target {
    sim.complete(\"Target cycles reached\");
}"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Realism Physics
                    // ─────────────────────────────────────────────────────
                    <section id="realism" class="docs-section">
                        <h2 class="section-anchor">"Realism Physics"</h2>

                        <div id="realism-materials" class="docs-block">
                            <h3>"Material Properties"</h3>
                            <p>
                                "Define physical material properties in TOML:"
                            </p>
                            <pre class="code-block"><code>{"# In .glb.toml instance file
[material]
name = \"Aluminum_6061\"
density = 2700.0              # kg/m³
thermal_conductivity = 167.0  # W/(m·K)
specific_heat = 896.0         # J/(kg·K)
young_modulus = 68.9e9        # Pa
poisson_ratio = 0.33
yield_strength = 276e6        # Pa
melting_point = 855.0         # K
electrical_conductivity = 25e6 # S/m"}</code></pre>
                        </div>

                        <div id="realism-thermo" class="docs-block">
                            <h3>"Thermodynamics"</h3>
                            <p>
                                "Simulate heat transfer and thermal behavior:"
                            </p>
                            <pre class="code-block"><code>{"# Thermodynamic state in TOML
[thermodynamic]
temperature = 298.15  # K (25°C)
pressure = 101325.0   # Pa (1 atm)
volume = 0.001        # m³
internal_energy = 0.0 # J
entropy = 0.0         # J/K
enthalpy = 0.0        # J
moles = 1.0

# Access from Rune
let temp_k = ecs.get_temperature(\"battery\");
let temp_c = temp_k - 273.15;

# Physics laws applied automatically:
# - Fourier's law (heat conduction)
# - Newton's law of cooling (convection)
# - Stefan-Boltzmann (radiation)"}</code></pre>
                        </div>

                        <div id="realism-electro" class="docs-block">
                            <h3>"Electrochemistry"</h3>
                            <p>
                                "Simulate batteries, fuel cells, and electrochemical systems:"
                            </p>
                            <pre class="code-block"><code>{"# Electrochemical state in TOML
[electrochemical]
voltage = 3.7           # V
terminal_voltage = 3.65 # V (under load)
capacity_ah = 100.0     # Ah
soc = 0.5               # 50%
current = 0.0           # A
internal_resistance = 0.01  # Ω
ionic_conductivity = 0.1    # S/m
cycle_count = 0
c_rate = 0.0
capacity_retention = 1.0    # 100%
heat_generation = 0.0       # W
dendrite_risk = 0.0         # 0-1

# Physics laws:
# - Nernst equation (equilibrium potential)
# - Butler-Volmer (reaction kinetics)
# - Arrhenius (temperature dependence)
# - Fick's laws (diffusion)"}</code></pre>
                        </div>

                        <div id="realism-fluids" class="docs-block">
                            <h3>"Fluid Dynamics"</h3>
                            <p>
                                "Simulate pumps, pipes, and fluid systems:"
                            </p>
                            <pre class="code-block"><code>{"# Fluid state (via Garbongus integration)
[fluid]
flow_rate = 0.01      # m³/s
pressure_in = 200000  # Pa
pressure_out = 100000 # Pa
temperature = 300.0   # K
viscosity = 0.001     # Pa·s
density = 1000.0      # kg/m³

# Physics laws:
# - Navier-Stokes (fluid motion)
# - Bernoulli (energy conservation)
# - Darcy-Weisbach (pressure drop)
# - Cavitation modeling"}</code></pre>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/ui" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"UI System"</span>
                            </div>
                        </a>
                        <a href="/docs/realism" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Realism"</span>
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
