// =============================================================================
// Eustress Web - Simulation Documentation Page
// =============================================================================
// Simulation: the time-scaled simulation clock, Play/Pause/Stop, sim values,
// watchpoints, recordings, experiments, determinism and headless runs.
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
                TocSubsection { id: "overview-run", title: "What a Run Is" },
                TocSubsection { id: "overview-speed", title: "What Speeds Up" },
            ],
        },
        TocSection {
            id: "clock",
            title: "The Clock",
            subsections: vec![
                TocSubsection { id: "clock-model", title: "Time and Ticks" },
                TocSubsection { id: "clock-scale", title: "Setting the Time Scale" },
                TocSubsection { id: "clock-limits", title: "Limits of Compression" },
            ],
        },
        TocSection {
            id: "running",
            title: "Running",
            subsections: vec![
                TocSubsection { id: "running-controls", title: "Play, Pause, Stop" },
                TocSubsection { id: "running-stop", title: "What Stop Restores" },
                TocSubsection { id: "running-step", title: "Stepping" },
            ],
        },
        TocSection {
            id: "values",
            title: "Sim Values",
            subsections: vec![
                TocSubsection { id: "values-map", title: "One Map of Numbers" },
                TocSubsection { id: "values-scripts", title: "From Rune" },
                TocSubsection { id: "values-data", title: "Driven by Data" },
                TocSubsection { id: "values-parameters", title: "Parameters" },
            ],
        },
        TocSection {
            id: "observe",
            title: "Watch and Record",
            subsections: vec![
                TocSubsection { id: "observe-watchpoints", title: "Watchpoints" },
                TocSubsection { id: "observe-recordings", title: "Recordings" },
                TocSubsection { id: "observe-telemetry", title: "Telemetry and Snapshots" },
            ],
        },
        TocSection {
            id: "experiments",
            title: "Experiments",
            subsections: vec![
                TocSubsection { id: "experiments-loop", title: "The Experiment Loop" },
                TocSubsection { id: "experiments-compare", title: "Comparing Runs" },
                TocSubsection { id: "experiments-tools", title: "The Agent Surface" },
            ],
        },
        TocSection {
            id: "repeat",
            title: "Determinism & Headless",
            subsections: vec![
                TocSubsection { id: "repeat-exact", title: "What Repeats Exactly" },
                TocSubsection { id: "repeat-headless", title: "Headless Batch Runs" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-breakpoints", title: "Breakpoints" },
                TocSubsection { id: "roadmap-settings", title: "A Simulation File" },
            ],
        },
    ]
}

/// One Play frame: the clock advances, scripts and models write sim values,
/// and the sim values feed four observers.
#[component]
fn RunFrameDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 250" role="img"
                aria-label="Each Play frame the simulation clock advances, scripts and Dataset bindings write sim values, models publish sim values, and the sim values feed watchpoints, the recording, the telemetry log and the runtime snapshot.">
                <defs>
                    <marker id="run-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>
                <text x="20" y="24" class="dg-title">"One Play frame"</text>

                // Producers
                <rect x="20" y="40" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="85" y="64" class="dg-label" text-anchor="middle">"Clock"</text>
                <text x="85" y="84" class="dg-note" text-anchor="middle">"frame time x scale"</text>
                <line x1="150" y1="68" x2="178" y2="68" class="dg-line" marker-end="url(#run-arrow)"></line>

                <rect x="180" y="40" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="245" y="64" class="dg-label" text-anchor="middle">"Scripts, data"</text>
                <text x="245" y="84" class="dg-note" text-anchor="middle">"write values"</text>
                <line x1="310" y1="68" x2="338" y2="68" class="dg-line" marker-end="url(#run-arrow)"></line>

                <rect x="340" y="40" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="405" y="64" class="dg-label" text-anchor="middle">"Models"</text>
                <text x="405" y="84" class="dg-note" text-anchor="middle">"publish values"</text>
                <line x1="470" y1="68" x2="498" y2="68" class="dg-line" marker-end="url(#run-arrow)"></line>

                <rect x="500" y="40" width="120" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="560" y="64" class="dg-label" text-anchor="middle">"Sim values"</text>
                <text x="560" y="84" class="dg-note" text-anchor="middle">"named numbers"</text>

                // Fan-out bus
                <line x1="560" y1="96" x2="560" y2="128" class="dg-line"></line>
                <line x1="85" y1="128" x2="560" y2="128" class="dg-line"></line>
                <line x1="85" y1="128" x2="85" y2="148" class="dg-line" marker-end="url(#run-arrow)"></line>
                <line x1="245" y1="128" x2="245" y2="148" class="dg-line" marker-end="url(#run-arrow)"></line>
                <line x1="405" y1="128" x2="405" y2="148" class="dg-line" marker-end="url(#run-arrow)"></line>
                <line x1="560" y1="128" x2="560" y2="148" class="dg-line" marker-end="url(#run-arrow)"></line>

                // Observers
                <rect x="20" y="150" width="130" height="56" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="85" y="174" class="dg-label" text-anchor="middle">"Watchpoints"</text>
                <text x="85" y="194" class="dg-note" text-anchor="middle">"min, max, average"</text>

                <rect x="180" y="150" width="130" height="56" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="245" y="174" class="dg-label" text-anchor="middle">"Recording"</text>
                <text x="245" y="194" class="dg-note" text-anchor="middle">"JSON on Stop"</text>

                <rect x="340" y="150" width="130" height="56" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="405" y="174" class="dg-label" text-anchor="middle">"Telemetry"</text>
                <text x="405" y="194" class="dg-note" text-anchor="middle">"1 line per second"</text>

                <rect x="500" y="150" width="120" height="56" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="560" y="174" class="dg-label" text-anchor="middle">"Snapshot"</text>
                <text x="560" y="194" class="dg-note" text-anchor="middle">"4 times a second"</text>

                <text x="320" y="236" class="dg-note" text-anchor="middle">"Tools read the snapshot and telemetry; Stop writes the recording to disk."</text>
            </svg>
            <figcaption>
                "Every Play frame follows one path. Whatever reaches the sim-value map is
                sampled by watchpoints, added to the recording, and exposed to tools."
            </figcaption>
        </figure>
    }
}

/// Simulation documentation page.
#[component]
pub fn DocsSimulationPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-simulation"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/play.svg" alt="Simulation" class="toc-icon" />
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Simulation"</span>
                        </div>
                        <h1 class="docs-title">"Simulation"</h1>
                        <p class="docs-subtitle">
                            "A simulation in Eustress is a Play session measured by a simulation clock.
                            The clock can run faster than real time for the models that integrate against
                            it, every named value a model or script publishes is recorded, and each run is
                            saved as a file you can compare with the next one."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "16 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/cube.svg" alt="Level" />
                                "Intermediate"
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

                        <div id="overview-run" class="subsection">
                            <h3>"What a Run Is"</h3>
                            <p>
                                "A simulation run is one Play session. Press Play and the run starts; press Stop
                                and Studio restores the world and saves the run. In between, every frame follows
                                the same path: the simulation clock advances, scripts, Dataset bindings and models
                                read and write "<strong>"sim values"</strong>" (named numbers such as "
                                <code>"battery.soc"</code>"), and the engine records those values."
                            </p>
                            <RunFrameDiagram />
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Two clocks"</strong>
                                    <p>
                                        "Rigid-body physics (Avian) steps at a fixed 60 Hz of real time. The
                                        simulation clock is a second clock that can run faster than real time for
                                        the models that integrate against it. Play starts both and Pause stops both."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-speed" class="subsection">
                            <h3>"What Speeds Up"</h3>
                            <p>
                                "The time scale multiplies the simulation clock. Only systems that integrate
                                against that clock run faster; the rest of a run keeps real time:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part of a run"</th><th>"Advances by"</th><th>"Follows the time scale"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Simulation clock"</td><td>"Frame time x time scale"</td><td>"Yes"</td></tr>
                                    <tr><td>"Cell model (parts with "<code>"[electrochemical]"</code>")"</td><td>"Clock time, in short steps"</td><td>"Yes"</td></tr>
                                    <tr><td>"Dataset bindings in "<code>"by_time"</code>" mode"</td><td>"Clock time"</td><td>"Yes"</td></tr>
                                    <tr><td>"Rigid-body physics"</td><td>"Fixed 1/60 s steps of real time"</td><td>"No"</td></tr>
                                    <tr><td>"Rune "<code>"on_update(dt)"</code></td><td>"Frame time"</td><td>"No"</td></tr>
                                    <tr><td>"Heat conduction between parts"</td><td>"Frame time"</td><td>"No"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A time scale does not speed up falling parts"</strong>
                                    <p>
                                        "Avian never reads the simulation clock, so at 3,600x a dropped part still
                                        takes the same wall-clock time to land. Use the time scale for models such
                                        as a battery cell over hundreds of hours, and step mechanics with "
                                        <code>"sim_step"</code>" when you need exact control."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // THE CLOCK
                    // =========================================================
                    <section id="clock" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "The Clock"
                        </h2>

                        <div id="clock-model" class="subsection">
                            <h3>"Time and Ticks"</h3>
                            <p>
                                "The simulation clock holds simulated time, wall time, the time scale and a tick
                                count. Each Play frame it adds the frame's duration times the time scale to
                                simulated time, and counts fixed ticks of one timestep, at most 10 per frame.
                                Pause freezes the clock and Stop resets it to zero."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Math"</span>
                                </div>
                                <pre><code class="language-text">{r#"simulated time        += frame time x time scale
ticks this frame       = min(unspent time / timestep, 10)
effective compression  = simulated time / wall time"#}</code></pre>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Setting"</th><th>"Default"</th><th>"Meaning"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Time scale"</td><td>"1"</td><td>"Simulated seconds per real second"</td></tr>
                                    <tr><td>"Tick rate"</td><td>"60 Hz"</td><td>"Ticks per simulated second; the timestep is its inverse"</td></tr>
                                    <tr><td>"Max ticks per frame"</td><td>"10"</td><td>"Cap on the ticks counted in one frame"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "While a run is playing or paused, a badge at the top of the viewport shows the
                                state and the clock, for example "<code>"1.5m | Tick 5400 | 1x"</code>":
                                simulated time, ticks and the time scale."
                            </p>
                        </div>

                        <div id="clock-scale" class="subsection">
                            <h3>"Setting the Time Scale"</h3>
                            <p>
                                "Open "<strong>"Simulation Settings"</strong>" with the button beside Stop, at the
                                left end of the ribbon's tab row. Choose a preset, or Custom and a value in Custom
                                Time Scale, and press Save. The live clock takes the new scale at once, mid-run
                                included, and keeps it for later runs in the session."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Preset"</th><th>"Time scale"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Realtime (1×)"</td><td>"1"</td></tr>
                                    <tr><td>"1 min / sec (60×)"</td><td>"60"</td></tr>
                                    <tr><td>"1 hr / sec (3,600×)"</td><td>"3,600"</td></tr>
                                    <tr><td>"1 day / sec (86,400×)"</td><td>"86,400"</td></tr>
                                    <tr><td>"1 week / sec (604,800×)"</td><td>"604,800"</td></tr>
                                    <tr><td>"1 month / sec (2.63M×)"</td><td>"2,630,000"</td></tr>
                                    <tr><td>"1 year / sec (31.5M×)"</td><td>"31,536,000"</td></tr>
                                    <tr><td>"Custom"</td><td>"The Custom Time Scale field"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The same dialog sets Tick Rate and Max Ticks / Frame. Tick Rate changes the
                                clock's timestep and the step length of the cell model; physics stays at 60 Hz.
                                Agents choose the scale per run instead: "<code>"run_simulation"</code>" takes "
                                <code>"time_scale"</code>", and a run it starts from Edit without one runs at 1x."
                            </p>
                        </div>

                        <div id="clock-limits" class="subsection">
                            <h3>"Limits of Compression"</h3>
                            <p>"How much faster than real time a run actually goes depends on three limits:"</p>
                            <ul class="docs-list">
                                <li><strong>"Slow frames"</strong>": the engine's virtual clock adds at most 33 ms per frame, so below about 30 frames per second simulated time falls behind the requested scale."</li>
                                <li><strong>"Model resolution"</strong>": the cell model cuts each frame into steps of one timestep, up to 4,096 of them. At 60 frames per second that holds 1/60 s steps up to 4,096x; above that the steps lengthen (about 0.35 s at 86,400x) and the engine logs a warning."</li>
                                <li><strong>"Tick cap"</strong>": once a frame would need more than 10 ticks, the tick count stops tracking simulated time."</li>
                            </ul>
                            <p>
                                "Each recording stores the ratio it achieved as "<code>"compression_ratio"</code>
                                ", simulated time divided by wall time."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Bounding a compressed run by ticks"</strong>
                                    <p>
                                        "At 3,600x each frame advances the clock by a minute but adds only 10
                                        ticks, so a tick budget ends a run far later in simulated time than it
                                        looks. Bound compressed runs with "<code>"duration_s"</code>", which stops
                                        on simulated seconds."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // RUNNING
                    // =========================================================
                    <section id="running" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Running"
                        </h2>

                        <div id="running-controls" class="subsection">
                            <h3>"Play, Pause, Stop"</h3>
                            <p>
                                "The run controls sit at the left end of the ribbon's tab row: Play, Play with
                                Character, Pause, Stop and Simulation Settings. The keys trigger the same
                                actions and can be rebound in Keyboard Shortcuts:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Action"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"F5"</code></td><td>"Play with a character"</td></tr>
                                    <tr><td><code>"F6"</code></td><td>"Pause, or resume a paused run"</td></tr>
                                    <tr><td><code>"F7"</code></td><td>"Play without a character"</td></tr>
                                    <tr><td><code>"F8"</code>" or "<code>"Esc"</code></td><td>"Stop"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The Roblox keymap preset maps Play to "<code>"F8"</code>" and Stop to "
                                <code>"Shift+F5"</code>"; "<code>"Esc"</code>" always stops. Pause freezes
                                physics, the simulation clock, scripts and the cell model, and "<code>"F6"</code>
                                " resumes the same run."
                            </p>
                        </div>

                        <div id="running-stop" class="subsection">
                            <h3>"What Stop Restores"</h3>
                            <p>
                                "Stop returns Studio to Edit mode and puts back what the run changed.
                                Transforms, part properties and humanoid values come back from the snapshot
                                taken at Play, and so do whole "<code>"[electrochemical]"</code>" and "
                                <code>"[thermodynamic]"</code>" states, so a battery's charge, temperature and
                                cycle count rewind together. Parts deleted during the run come back, and parts
                                created during it are removed."
                            </p>
                            <p>
                                "The simulation then exports the run's recording and resets: the clock returns
                                to zero, sim values clear and watchpoint statistics reset. A run that stops itself
                                after "<code>"duration_s"</code>" goes through the same Stop."
                            </p>
                        </div>

                        <div id="running-step" class="subsection">
                            <h3>"Stepping"</h3>
                            <p>
                                "Stepping advances rigid-body physics by an exact number of fixed ticks,
                                independent of the wall clock. Agents and the CLI step through the "
                                <code>"sim_step"</code>" tool (the engine bridge's "<code>"sim.step"</code>"
                                method): 1 to 10,000 ticks of 1/60 s, after which physics is left paused so the
                                world holds still until the next step."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "pause_simulation", "arguments": {} }
{ "tool": "sim_step", "arguments": { "ticks": 120 } }
// reply: stepped 120 fixed tick(s) (2.000s sim time); read inspect_scene for the new state"#}</code></pre>
                            </div>
                            <p>
                                "Pause the run first so only the steps move the world. A step runs the fixed
                                physics schedule alone; the simulation clock, scripts and the cell model advance
                                only in Play frames. Long steps are spread across frames so the engine keeps
                                drawing, and one step can be in flight at a time."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // SIM VALUES
                    // =========================================================
                    <section id="values" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Sim Values"
                        </h2>

                        <div id="values-map" class="subsection">
                            <h3>"One Map of Numbers"</h3>
                            <p>
                                "Sim values are one flat map from names to numbers. Models publish into it,
                                scripts, agents and Dataset bindings write into it, and everything that observes
                                a run reads it."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Writer"</th><th>"What it writes"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Cell model"</td><td>"24 "<code>"battery.*"</code>" values for the "<code>"[electrochemical]"</code>" part with the largest capacity, every Play frame"</td></tr>
                                    <tr><td>"Rune scripts"</td><td>"Any key passed to "<code>"set_sim_value"</code></td></tr>
                                    <tr><td>"Agents"</td><td><code>"set_sim_value"</code>", and the overrides of "<code>"run_experiment"</code></td></tr>
                                    <tr><td>"Dataset bindings"</td><td>"The key a column is bound to"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The cell model reads three keys back. "<code>"battery.mode"</code>" selects idle
                                (0), charge (1) or discharge (2), and is 2 when unset. "
                                <code>"battery.target_current"</code>" sets the current in amperes; without it the
                                model charges at 1C and discharges at 0.5C. A "<code>"battery.current"</code>"
                                written by a script sets the current for that frame directly, which is how a
                                controller holds a cell at zero. The model republishes every other "
                                <code>"battery.*"</code>" key each frame, so writing one does not change the cell.
                                Stop clears the map."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Steer the cell from tools with mode and target current"</strong>
                                    <p>
                                        "A "<code>"battery.current"</code>" written by "<code>"set_sim_value"</code>
                                        ", "<code>"run_experiment"</code>" or a Dataset binding lands before the
                                        script frame, which replaces the frame's explicit writes, so the cell model
                                        never sees it. "<code>"battery.mode"</code>" and "
                                        <code>"battery.target_current"</code>" stay set until changed, so set those
                                        from outside a script."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="values-scripts" class="subsection">
                            <h3>"From Rune"</h3>
                            <p>
                                "Rune scripts use three functions from the "<code>"eustress"</code>" module: "
                                <code>"get_sim_value(key)"</code>" (0.0 for a missing key), "
                                <code>"set_sim_value(key, value)"</code>" and "<code>"list_sim_values()"</code>
                                ". Import them at the top of a "<code>".rune"</code>" file:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Rune"</span>
                                </div>
                                <pre><code class="language-rust">{r#"use eustress::{get_sim_value, set_sim_value};

pub fn on_update(dt) {
    // What the cell model published last frame (0.0 if absent).
    let soc = get_sim_value("battery.soc");

    // Publish a value of your own; tools and recordings see it.
    set_sim_value("pack.soc_percent", soc * 100.0);

    // Below 20 %, hold the cell idle. An explicit write wins this frame.
    if soc < 0.2 {
        set_sim_value("battery.current", 0.0);
    }
}"#}</code></pre>
                            </div>
                            <p>
                                "A script's "<code>"on_update"</code>" receives the frame's real duration in
                                seconds, not simulated time, so state a script integrates itself advances at real
                                time. See "<a href="/docs/scripting">"Scripting"</a>" for the rest of the script API."
                            </p>
                        </div>

                        <div id="values-data" class="subsection">
                            <h3>"Driven by Data"</h3>
                            <p>
                                "A Dataset column can drive a sim value during a run, so a model runs against
                                measured numbers instead of its defaults. The "<code>"data_bind"</code>" tool binds
                                one numeric column of a Dataset's CSV to one key:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "data_bind", "arguments": {
    "dataset": "DriveCycle",
    "column": "current_a",
    "target": "battery.target_current",
    "mode": "by_time",
    "time_column": "time_s"
} }"#}</code></pre>
                            </div>
                            <p>
                                "With "<code>"battery.mode"</code>" at its default of 2, that column sets the
                                discharge current second by second of simulated time."
                            </p>
                            <ul class="docs-list">
                                <li><code>"by_row"</code>": one row per frame, holding the last row at the end, or starting over with "<code>"loop"</code>" set to true."</li>
                                <li><code>"by_time"</code>": interpolates the column linearly against the simulation clock, and holds the first or last value outside the recorded span."</li>
                            </ul>
                            <p>
                                "Binding a key that is already bound replaces the old binding. "
                                <code>"data_bindings"</code>" lists bindings with the value each last wrote, and "
                                <code>"data_unbind"</code>" returns a key to the model."
                            </p>
                        </div>

                        <div id="values-parameters" class="subsection">
                            <h3>"Parameters"</h3>
                            <p>
                                "Parameters are a separate store: typed values attached to an instance and shown in
                                Properties. A part's "<code>"[parameters]"</code>" table fills them. A plain key
                                lands in the "<code>"instance"</code>" domain, and a quoted dotted key names its
                                own domain:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[parameters]
rated_capacity_ah = 40.0          # domain: instance
"telemetry.sample_rate" = 10      # domain: telemetry, key: sample_rate"#}</code></pre>
                            </div>
                            <p>
                                "Quote the dotted key: unquoted, TOML reads it as a nested table and the loader
                                stores the whole table as one JSON value. The simulation clock and the models do
                                not read parameters. To feed a number into a running model, write a sim value or
                                bind a Dataset column."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WATCH AND RECORD
                    // =========================================================
                    <section id="observe" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Watch and Record"
                        </h2>

                        <div id="observe-watchpoints" class="subsection">
                            <h3>"Watchpoints"</h3>
                            <p>
                                "A watchpoint is a sim value the engine keeps statistics for: the current value,
                                minimum, maximum, running average and a history of up to 10,000 samples, oldest
                                dropped first. During a run, each value with a watchpoint is sampled once per frame
                                at the current simulated time."
                            </p>
                            <p>
                                "The models register them. Entering Play registers nine for the cell: "
                                <code>"battery.voltage"</code>", "<code>"battery.current"</code>", "
                                <code>"battery.soc"</code>", "<code>"battery.temperature_c"</code>", "
                                <code>"battery.power"</code>", "<code>"battery.c_rate"</code>", "
                                <code>"battery.dendrite_risk"</code>", "<code>"battery.capacity_retention"</code>
                                " and "<code>"battery.cycle_count"</code>". The other sim values carry no
                                statistics but are still recorded. Stop resets every watchpoint's statistics."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Simulation Settings applies four fields"</strong>
                                    <p>
                                        "Save applies the preset, Custom Time Scale, Tick Rate and Max Ticks /
                                        Frame. The dialog's Startup, Manual Step, Auto-Stop Conditions, Output and
                                        Watchpoints sections and its binding list are not read by the engine, and
                                        its Rune API Reference shows a "<code>"sim.*"</code>" script API that the
                                        runtime does not install. Publish values with "<code>"set_sim_value"</code>
                                        " and control runs with "<a href="#experiments-tools">"the tools"</a>"."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="observe-recordings" class="subsection">
                            <h3>"Recordings"</h3>
                            <p>
                                "Every run is recorded. Entering Play starts a recording, and each frame adds one
                                sample, at the current simulated time, for every sim value, not only those with
                                watchpoints. Stop computes statistics for each series, writes the recording as
                                JSON and prints its path to Output:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Recording path"</span>
                                </div>
                                <pre><code class="language-text">{r#"<universe>/.eustress/knowledge/recordings/<space>/
    sim_20260922_143015_123_run3_pid4812.json"#}</code></pre>
                            </div>
                            <p>
                                "The name carries the UTC date and time to the millisecond, the run number and the
                                engine's process id, so engines running variants side by side never overwrite
                                each other. The file holds:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"sim_*.json"</span>
                                </div>
                                <pre><code class="language-text">{r#"metadata   name, simulation_duration_s, wall_duration_s,
           total_ticks, compression_ratio, tags
series     one entry per sim value key:
           name, label, unit,
           times[]  (simulated seconds), values[],
           stats { min, max, mean, std_dev, first, last }
events     time_s, tick, event_type, description, data"#}</code></pre>
                            </div>
                            <p>
                                "Samples follow rendered frames, so a recording grows with the run's wall-clock
                                length: a minute at 60 frames per second is 3,600 samples per value."
                            </p>
                        </div>

                        <div id="observe-telemetry" class="subsection">
                            <h3>"Telemetry and Snapshots"</h3>
                            <p>"Two lighter feeds serve tools while a run is live:"</p>
                            <ul class="docs-list">
                                <li><strong>"Telemetry"</strong>": once a second the engine appends one JSON line, a timestamp and every sim value, to "<code>"<universe>/.eustress/telemetry.jsonl"</code>". "<code>"tail_telemetry"</code>" returns the latest lines (20 by default, up to 100), optionally filtered to a list of keys."</li>
                                <li><strong>"Runtime snapshot"</strong>": four times a second the engine writes the play state and the current sim values, watchpoints included, to a snapshot file. "<code>"get_sim_value"</code>", "<code>"list_sim_values"</code>" and "<code>"get_simulation_state"</code>" read it, so what they return can trail the engine by up to 250 ms."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // EXPERIMENTS
                    // =========================================================
                    <section id="experiments" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Experiments"
                        </h2>

                        <div id="experiments-loop" class="subsection">
                            <h3>"The Experiment Loop"</h3>
                            <p>
                                "An experiment is a named run with fixed inputs and a saved result. The "
                                <code>"run_experiment"</code>" tool performs the whole loop in one call:"
                            </p>
                            <ol class="numbered-list">
                                <li>"With "<code>"create_branch"</code>" set, creates a git branch "<code>"exp/<name>-<timestamp>"</code>" in the repository that holds the Space. The Space stays on that branch afterwards."</li>
                                <li>"Writes each entry of "<code>"sim_values"</code>" as a sim value."</li>
                                <li>"Starts a run at "<code>"time_scale"</code>" (default 1) that stops itself after "<code>"duration_s"</code>" simulated seconds."</li>
                                <li>"Waits for that run to end, up to "<code>"timeout_s"</code>" of wall time (default 300 s)."</li>
                                <li>"Reads the run's telemetry lines and computes min, mean, max and last for each key."</li>
                                <li>"Saves the result to "<code>"<universe>/.eustress/experiments/<name>-<timestamp>.json"</code>" and returns it."</li>
                            </ol>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "run_experiment", "arguments": {
    "name": "charge_1c",
    "description": "Does a 1C charge stay under 45 C?",
    "sim_values": { "battery.mode": 1 },
    "duration_s": 3600,
    "time_scale": 60,
    "create_branch": true
} }"#}</code></pre>
                            </div>
                            <p>
                                "In the Workshop, "<code>"run_experiment"</code>" and "<code>"run_simulation"</code>
                                " stop for your approval unless auto mode is on."
                            </p>
                        </div>

                        <div id="experiments-compare" class="subsection">
                            <h3>"Comparing Runs"</h3>
                            <p>
                                <code>"compare_runs"</code>" lines two saved experiments up key by key: baseline,
                                candidate and the difference. Pass file names, or "<code>"latest"</code>" and "
                                <code>"latest-1"</code>". A decrease counts as an improvement unless the key is
                                listed in "<code>"higher_is_better"</code>":"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"MCP"</span>
                                </div>
                                <pre><code class="language-json">{r#"{ "tool": "compare_runs", "arguments": {
    "run_a": "latest-1",
    "run_b": "latest",
    "higher_is_better": ["battery.capacity_retention", "battery.soc"]
} }"#}</code></pre>
                            </div>
                            <p>
                                <code>"list_experiments"</code>" lists saved results newest first (20 by default)
                                with duration, time scale and final values. Because an experiment can run on its
                                own branch, a design change and the result it produced can be committed together;
                                see "<a href="/docs/universes">"Universes"</a>" for branching."
                            </p>
                        </div>

                        <div id="experiments-tools" class="subsection">
                            <h3>"The Agent Surface"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Tool"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"run_simulation"</code></td><td>"Start a run from Edit, or resume a paused one; optional "<code>"time_scale"</code>" and "<code>"duration_s"</code></td></tr>
                                    <tr><td><code>"pause_simulation"</code>", "<code>"stop_simulation"</code></td><td>"Pause, or stop and restore"</td></tr>
                                    <tr><td><code>"get_simulation_state"</code></td><td>"Play state and every sim value"</td></tr>
                                    <tr><td><code>"await_simulation"</code></td><td>"Wait for a run to end and return its final values and telemetry statistics"</td></tr>
                                    <tr><td><code>"get_sim_value"</code>", "<code>"list_sim_values"</code>", "<code>"set_sim_value"</code></td><td>"Read and write single values"</td></tr>
                                    <tr><td><code>"tail_telemetry"</code></td><td>"The latest telemetry lines"</td></tr>
                                    <tr><td><code>"sim_step"</code></td><td>"Step physics by N fixed ticks"</td></tr>
                                    <tr><td><code>"data_bind"</code>", "<code>"data_bindings"</code>", "<code>"data_unbind"</code></td><td>"Drive values from a Dataset"</td></tr>
                                    <tr><td><code>"run_experiment"</code>", "<code>"compare_runs"</code>", "<code>"list_experiments"</code></td><td>"Run, save and compare experiments"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Each is a tool of the "<a href="/learn/mcp">"MCP server"</a>". The "
                                <code>"eustress"</code>" CLI talks to the same engine bridge; see "
                                <a href="/learn/cli">"CLI & Headless"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // DETERMINISM & HEADLESS
                    // =========================================================
                    <section id="repeat" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Determinism & Headless"
                        </h2>

                        <div id="repeat-exact" class="subsection">
                            <h3>"What Repeats Exactly"</h3>
                            <p>
                                "The physics step is pinned so the same inputs give the same world: a fixed 60 Hz
                                timestep, 6 solver substeps and the default solver configuration. The engine also
                                keeps a global random seed, a fixed constant by default, and "
                                <code>"NumberRange"</code>" random draws in scripts come from a generator seeded
                                with it. A determinism test in the repository drops 16 seeded cubes, steps 120
                                ticks twice and requires identical end states."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part of a run"</th><th>"How it repeats"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"sim_step"</code></td><td>"Exactly: N fixed ticks, independent of the wall clock"</td></tr>
                                    <tr><td>"Physics during Play"</td><td>"Each step is identical, but inputs land on whichever step the frame timing gives them"</td></tr>
                                    <tr><td>"Cell model"</td><td>"Step length follows frame time, so runs agree closely rather than bit for bit"</td></tr>
                                    <tr><td>"Script "<code>"on_update(dt)"</code></td><td>"Receives real frame time"</td></tr>
                                    <tr><td>"Random numbers"</td><td>"Seeded from a fixed constant"</td></tr>
                                </tbody>
                            </table>
                            <p>"For bit-for-bit repeats of mechanics, pause and drive the world with "<code>"sim_step"</code>"."</p>
                        </div>

                        <div id="repeat-headless" class="subsection">
                            <h3>"Headless Batch Runs"</h3>
                            <p>
                                <code>"eustress-headless"</code>" runs a Space's simulation with no window. It is
                                built from the engine crate and loads the same core plugins as Studio (physics,
                                realism, scripts, sim values, recordings and the engine bridge) on a plain frame
                                loop:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Shell"</span>
                                </div>
                                <pre><code class="language-bash">{r#"eustress-headless --space <space folder> --ticks 3600"#}</code></pre>
                            </div>
                            <p>
                                "It waits 120 frames for the Space to load, enters Play, stops when the clock
                                reaches the tick count, exports the recording and exits with code 0. Without "
                                <code>"--ticks"</code>" it runs until stopped, a windowless engine you drive over
                                the bridge; "<code>"--no-autoplay"</code>" makes it wait for "
                                <code>"run_simulation"</code>". "<code>"--tick-rate"</code>" sets how often frames
                                run, not how fast simulated time passes, so a headless run takes as long as the
                                same run in Studio. The "<code>"eustress run"</code>" command wraps it; "
                                <a href="/learn/cli">"CLI & Headless"</a>" lists every flag."
                            </p>
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

                        <div id="roadmap-breakpoints" class="subsection">
                            <h3>"Breakpoints"</h3>
                            <p>
                                "The engine already checks a breakpoint registry every Play frame. A breakpoint
                                compares one sim value with a threshold ("<code>"<"</code>", "<code>"<="</code>
                                ", "<code>"=="</code>", "<code>">="</code>", "<code>">"</code>", "
                                <code>"!="</code>"), can fire once or with a cooldown, and on a hit stops the
                                simulation clock and writes a breakpoint event into the recording. Next come ways
                                to declare breakpoints from Studio, scripts and tools, and a hit that pauses the
                                whole run rather than only the clock."
                            </p>
                        </div>

                        <div id="roadmap-settings" class="subsection">
                            <h3>"A Simulation File"</h3>
                            <p>
                                "The engine carries a TOML schema for run settings (tick rate, time scale,
                                auto-start, time and tick limits, recording format, watchpoints, breakpoints,
                                test expectations and named parameters) and a CSV writer for recordings. Neither is
                                wired to a Space yet. Loading that file from a Space, the dialog's remaining
                                sections, and a size cap for "<code>"telemetry.jsonl"</code>", which grows without
                                limit today, are the next steps."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Compress the model, not the physics, and keep every run."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/realism" class="btn-secondary-steel">"Realism Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/physics" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Physics"</span>
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
