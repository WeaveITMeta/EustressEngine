// =============================================================================
// Eustress Web - Universes Documentation Page
// =============================================================================
// Universes: the staging environment for reality. The thesis (why a forkable
// world matters) and the technical nuance (how a Universe is actually built:
// copy-on-write world database, Spaces, branches, rollout, connectors, agents).
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
            id: "concept",
            title: "The Concept",
            subsections: vec![
                TocSubsection { id: "concept-what", title: "What Is a Universe" },
                TocSubsection { id: "concept-staging", title: "The Staging-Server Analogy" },
            ],
        },
        TocSection {
            id: "allow",
            title: "What Universes Let You Do",
            subsections: vec![
                TocSubsection { id: "allow-whatif", title: "Run What-If Safely" },
                TocSubsection { id: "allow-ai", title: "Let AI Act Without Fear" },
                TocSubsection { id: "allow-scale", title: "Real Data, Real Scale" },
            ],
        },
        TocSection {
            id: "thesis",
            title: "The Thesis",
            subsections: vec![
                TocSubsection { id: "thesis-gap", title: "Reality Has No Staging Server" },
                TocSubsection { id: "thesis-trust", title: "Trust to Act" },
            ],
        },
        TocSection {
            id: "anatomy",
            title: "Anatomy of a Universe",
            subsections: vec![
                TocSubsection { id: "anatomy-hierarchy", title: "Universe, Space, Instance" },
                TocSubsection { id: "anatomy-worlddb", title: "The World Database" },
                TocSubsection { id: "anatomy-cow", title: "Copy-on-Write Branches" },
            ],
        },
        TocSection {
            id: "loop",
            title: "Fork · Rehearse · Commit",
            subsections: vec![
                TocSubsection { id: "loop-fork", title: "Fork" },
                TocSubsection { id: "loop-rehearse", title: "Rehearse" },
                TocSubsection { id: "loop-commit", title: "Commit" },
            ],
        },
        TocSection {
            id: "substrate",
            title: "The Substrate",
            subsections: vec![
                TocSubsection { id: "substrate-layers", title: "Five Layers, One Substrate" },
                TocSubsection { id: "substrate-verbs", title: "The Verbs" },
            ],
        },
        TocSection {
            id: "applications",
            title: "Where It Pays",
            subsections: vec![
                TocSubsection { id: "applications-domains", title: "Decisions You Can't Take Back" },
                TocSubsection { id: "applications-status", title: "Where We Are" },
            ],
        },
    ]
}

/// Universes documentation page.
#[component]
pub fn DocsUniversesPage() -> impl IntoView {
    let active_section = RwSignal::new("concept".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-philosophy"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/grid.svg" alt="Universes" class="toc-icon" />
                        <h2>"Universes"</h2>
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
                            <span class="current">"Universes"</span>
                        </div>
                        <h1 class="docs-title">"Universes"</h1>
                        <p class="docs-subtitle">
                            "A Universe is a staging environment for reality — a forkable world you can
                            branch, rehearse, and commit. Here is the idea, and how it actually works."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "12 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/brain.svg" alt="Level" />
                                "Conceptual"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    <section id="concept" class="docs-section">
                        <h2 class="section-anchor">"The Concept"</h2>

                        <div id="concept-what" class="docs-block">
                            <h3>"What Is a Universe"</h3>
                            <p>
                                "A Universe is the world Eustress simulates — a living, spatial, data-rich
                                model of some slice of reality: a power grid, a factory floor, a supply
                                network, a city. It holds Spaces, Services, and Instances, fed by live data
                                and driven by physics and agents."
                            </p>
                            <p>
                                "What separates a Universe from a dashboard or a static digital twin is one
                                property: you can "<strong>"fork the entire thing, cheaply"</strong>" —
                                and run the copy forward to see what happens."
                            </p>
                        </div>

                        <div id="concept-staging" class="docs-block">
                            <h3>"The Staging-Server Analogy"</h3>
                            <div class="docs-callout success">
                                <strong>"Software never deploys untested code to production."</strong>
                                " It branches, tests on a staging copy, and merges only what works. The
                                physical and operational world has no staging copy — until now. A Universe
                                is that staging copy of reality: branch it, try the decision, keep only what
                                is proven."
                            </div>
                        </div>
                    </section>

                    <section id="allow" class="docs-section">
                        <h2 class="section-anchor">"What Universes Let You Do"</h2>
                        <div class="docs-block">
                            <p>
                                "A Universe is not a chart to look at — it is a world to act in. Because you
                                can fork it, the things you could never do to the real system become routine.
                                For the person making the decision, that changes what is possible:"
                            </p>
                        </div>

                        <div class="principles-grid">
                            <div id="allow-whatif" class="principle-card">
                                <div class="principle-number">"01"</div>
                                <h4>"Run What-If Safely"</h4>
                                <p>
                                    "Try a change against a copy of the real system. If it goes wrong,
                                    discard the branch — production never saw it, and nothing is at stake."
                                </p>
                            </div>
                            <div id="allow-compare" class="principle-card">
                                <div class="principle-number">"02"</div>
                                <h4>"Compare Before You Commit"</h4>
                                <p>
                                    "Fork several options, run them forward, and see which wins on your own
                                    metrics — before spending a dollar or touching the live system."
                                </p>
                            </div>
                            <div id="allow-risk" class="principle-card">
                                <div class="principle-number">"03"</div>
                                <h4>"Put a Number on Risk"</h4>
                                <p>
                                    "Quantify the cost of being wrong — the blackout, the stockout, the
                                    failed part — while it is still hypothetical and still cheap to avoid."
                                </p>
                            </div>
                            <div id="allow-ai" class="principle-card">
                                <div class="principle-number">"04"</div>
                                <h4>"Let AI Act Without Fear"</h4>
                                <p>
                                    "Hand an agent a decision. It proves the move inside the fork and commits
                                    only what is verified — with a record of why it won."
                                </p>
                            </div>
                            <div id="allow-replay" class="principle-card">
                                <div class="principle-number">"05"</div>
                                <h4>"Rewind, Change One Thing, Replay"</h4>
                                <p>
                                    "Every branch is a save point. Step back, vary a single input, and re-run
                                    — as many times as you need to understand the system."
                                </p>
                            </div>
                            <div id="allow-scale" class="principle-card">
                                <div class="principle-number">"06"</div>
                                <h4>"Real Data, Real Scale"</h4>
                                <p>
                                    "Connect live sources so rehearsals reflect reality — then scale the same
                                    Universe from your laptop to thousands of parallel branches in the cloud."
                                </p>
                            </div>
                        </div>

                        <div class="docs-callout success">
                            <strong>"The bottom line:"</strong>
                            " a Universe turns irreversible, expensive, one-shot decisions into cheap,
                            repeatable experiments — so the version of a decision that reaches reality is the
                            one that already proved itself."
                        </div>
                    </section>

                    <section id="thesis" class="docs-section">
                        <h2 class="section-anchor">"The Thesis"</h2>

                        <div id="thesis-gap" class="docs-block">
                            <h3>"Reality Has No Staging Server"</h3>
                            <p>
                                "You cannot A/B test a power grid, fork a supply chain, or roll back a
                                surgery. So the most consequential decisions in the economy are still made
                                the old way — modeled roughly, argued over, then committed and hoped."
                            </p>
                            <div class="manifesto-quote">
                                <blockquote>
                                    "A decision you cannot take back deserves a place to be wrong first."
                                </blockquote>
                            </div>
                        </div>

                        <div id="thesis-trust" class="docs-block">
                            <h3>"Trust to Act"</h3>
                            <p>
                                "AI is crossing from generating text to taking actions on real systems. The
                                bottleneck is not intelligence — models already out-reason us. It is "
                                <strong>"trust to act"</strong>" on something irreversible."
                            </p>
                            <p>
                                "A Universe is where an agent earns that trust: it acts inside the fork,
                                fails safely, and proves the decision before it ever touches the real thing.
                                As AI learns to act, a trustworthy place to be wrong becomes the scarce
                                resource — and that place is a Universe."
                            </p>
                        </div>
                    </section>

                    <section id="anatomy" class="docs-section">
                        <h2 class="section-anchor">"Anatomy of a Universe"</h2>

                        <div id="anatomy-hierarchy" class="docs-block">
                            <h3>"Universe, Space, Instance"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Layer"</th><th>"What it is"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Universe"</td><td>"The forkable world — a collection of Spaces, branchable as one"</td></tr>
                                    <tr><td>"Space"</td><td>"One scene or domain; persists as a .eustress directory"</td></tr>
                                    <tr><td>"Service"</td><td>"Workspace, DataService, Lighting, Players — typed containers"</td></tr>
                                    <tr><td>"Instance"</td><td>"The entities themselves — parts, datasets, connectors, agents"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="anatomy-worlddb" class="docs-block">
                            <h3>"The World Database"</h3>
                            <p>
                                "Each Space is backed by a copy-on-write world database — a log-structured
                                merge-tree (Fjall). Live entity state, datasets, and timeseries live in
                                partitions, not loose files. That database is what makes a Universe forkable
                                instead of merely viewable."
                            </p>
                        </div>

                        <div id="anatomy-cow" class="docs-block">
                            <h3>"Copy-on-Write Branches"</h3>
                            <div class="docs-callout info">
                                <strong>"Branching does not copy gigabytes."</strong>
                                " A branch shares its parent's data and writes only what changes
                                (copy-on-write). A whole world forks in milliseconds — the unglamorous key
                                that makes fork → rehearse → commit economical at scale."
                            </div>
                            <pre class="code-block"><code>{"let branch = universe.branch();   // cheap copy-on-write fork
// ... run the branch forward, mutate freely ...
branch.commit();                  // promote changes to the parent
// or
branch.discard();                 // throw the whole world away, free
universe.digest();                // content hash - compare branches"}</code></pre>
                        </div>
                    </section>

                    <section id="loop" class="docs-section">
                        <h2 class="section-anchor">"Fork · Rehearse · Commit"</h2>

                        <div class="principles-grid">
                            <div id="loop-fork" class="principle-card">
                                <div class="principle-number">"01"</div>
                                <h4>"Fork"</h4>
                                <p>
                                    "Pull live reality in through connectors, then branch the entire
                                    Universe with copy-on-write. A whole world, forked in milliseconds."
                                </p>
                            </div>
                            <div id="loop-rehearse" class="principle-card">
                                <div class="principle-number">"02"</div>
                                <h4>"Rehearse"</h4>
                                <p>
                                    "Run thousands of branches in parallel — headless rollout. Drop AI
                                    agents inside, and set adversarial ones loose to hunt the failures you
                                    did not think of."
                                </p>
                            </div>
                            <div id="loop-commit" class="principle-card">
                                <div class="principle-number">"03"</div>
                                <h4>"Commit"</h4>
                                <p>
                                    "Compare outcomes across the tree and promote one branch back to the
                                    parent — with a record of why it won. Everything else is discarded for
                                    free."
                                </p>
                            </div>
                        </div>
                    </section>

                    <section id="substrate" class="docs-section">
                        <h2 class="section-anchor">"The Substrate"</h2>

                        <div id="substrate-layers" class="docs-block">
                            <h3>"Five Layers, One Substrate"</h3>
                            <p>"A Universe is the combination — and that combination is the moat:"</p>
                            <ul class="docs-list">
                                <li><strong>"Spatial twin"</strong>" — real-time 3D and physics; data with a place in the world"</li>
                                <li><strong>"Data platform"</strong>" — columnar Datasets and Series with stats, fit, spectral, clustering, anomaly"</li>
                                <li><strong>"World database"</strong>" — copy-on-write branches make forking the whole world cheap"</li>
                                <li><strong>"Agent runtime"</strong>" — AI acts inside the fork, not just observes it"</li>
                                <li><strong>"Live connectors"</strong>" — REST, streams, SQL, cloud; reality, pulled in continuously"</li>
                            </ul>
                        </div>

                        <div id="substrate-verbs" class="docs-block">
                            <h3>"The Verbs"</h3>
                            <p>"The operations you run on a Universe — the same loop underneath each:"</p>
                            <ul class="docs-list">
                                <li><strong>"Connect"</strong>" — bind a live source as a Connector instance"</li>
                                <li><strong>"Record"</strong>" — capture timeseries as the world runs"</li>
                                <li><strong>"Branch / Compare"</strong>" — fork the world, then diff outcomes"</li>
                                <li><strong>"Fit / Anomaly"</strong>" — model and flag what the data is doing"</li>
                                <li><strong>"Overlay / Dashboard"</strong>" — color the twin by a metric; compose the view"</li>
                            </ul>
                        </div>
                    </section>

                    <section id="applications" class="docs-section">
                        <h2 class="section-anchor">"Where It Pays"</h2>

                        <div id="applications-domains" class="docs-block">
                            <h3>"Decisions You Can't Take Back"</h3>
                            <ul class="docs-list">
                                <li><strong>"Energy"</strong>" — fork every grid contingency before it cascades"</li>
                                <li><strong>"Industrials"</strong>" — stress-test a supply network; prove the compliance record"</li>
                                <li><strong>"Mobility"</strong>" — mine the autonomy long tail with adversarial agents"</li>
                                <li><strong>"Healthcare"</strong>" — rehearse the surge instead of improvising it"</li>
                                <li><strong>"Manufacturing"</strong>" — validate the line before you build it"</li>
                            </ul>
                        </div>

                        <div id="applications-status" class="docs-block">
                            <h3>"Where We Are"</h3>
                            <div class="docs-callout info">
                                <strong>"Built in the open."</strong>
                                " The primitives are real today — copy-on-write world-forking, headless
                                rollout, live connectors, the analysis verbs, agents-in-simulation, physics.
                                The full fork-rehearse-commit loop at economy scale is what we are building
                                toward, in public."
                            </div>
                            <div class="future-cta">
                                <p><strong>"Branch a world. Rehearse the decision. Commit only what's proven."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/simulation" class="btn-secondary-steel">"Simulation Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/philosophy" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Philosophy"</span>
                            </div>
                        </a>
                        <a href="/docs/scripting" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Scripting"</span>
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
