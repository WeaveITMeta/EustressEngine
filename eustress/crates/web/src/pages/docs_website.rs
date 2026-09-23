// =============================================================================
// Eustress Web - Website Service Documentation Page
// =============================================================================
// The Website service and its References, the bake that runs during publish,
// and the consumer side: the manifest route, data-eus binding, hydration,
// caching, keys and the failure policy.
//
// The load-bearing idea is the fallback rule: the element's existing text must
// already be correct, so the fetch can only improve the page. Every other rule
// on this page follows from it.
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
            title: "Why This Exists",
            subsections: vec![
                TocSubsection { id: "overview-drift", title: "Retyped Numbers Drift" },
                TocSubsection { id: "overview-shape", title: "One Fetch, Not One Per Value" },
                TocSubsection { id: "overview-status", title: "Status" },
            ],
        },
        TocSection {
            id: "service",
            title: "The Website Service",
            subsections: vec![
                TocSubsection { id: "service-folder", title: "A Folder of References" },
                TocSubsection { id: "service-config", title: "The Service File" },
                TocSubsection { id: "service-reference", title: "What a Reference Is" },
            ],
        },
        TocSection {
            id: "kinds",
            title: "The Five Reference Kinds",
            subsections: vec![
                TocSubsection { id: "kinds-why", title: "Why Five" },
                TocSubsection { id: "kinds-instance", title: "instance and sim" },
                TocSubsection { id: "kinds-derived", title: "count, measure, expr" },
            ],
        },
        TocSection {
            id: "bake",
            title: "Publish and Bake",
            subsections: vec![
                TocSubsection { id: "bake-order", title: "Where the Bake Runs" },
                TocSubsection { id: "bake-fails", title: "A Failed Reference Fails the Publish" },
                TocSubsection { id: "bake-format", title: "Formatting display" },
                TocSubsection { id: "bake-where", title: "Where the Manifest Lands" },
            ],
        },
        TocSection {
            id: "consume",
            title: "Consuming the Manifest",
            subsections: vec![
                TocSubsection { id: "consume-endpoint", title: "The Endpoint" },
                TocSubsection { id: "consume-shape", title: "What Comes Back" },
                TocSubsection { id: "consume-binding", title: "Binding with data-eus" },
                TocSubsection { id: "consume-hydrate", title: "The Hydration Script" },
            ],
        },
        TocSection {
            id: "caching",
            title: "Caching, Keys, Failure",
            subsections: vec![
                TocSubsection { id: "caching-headers", title: "Response Headers" },
                TocSubsection { id: "caching-pin", title: "Pinning a State" },
                TocSubsection { id: "caching-keys", title: "Access Keys" },
                TocSubsection { id: "caching-failure", title: "The Failure Table" },
            ],
        },
        TocSection {
            id: "practice",
            title: "Consumer Practice",
            subsections: vec![
                TocSubsection { id: "practice-build", title: "Build-Time Baking" },
                TocSubsection { id: "practice-limits", title: "Four Honest Limits" },
                TocSubsection { id: "practice-checklist", title: "Checklist for a New Consumer" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-keys", title: "Keys from Studio" },
                TocSubsection { id: "roadmap-authoring", title: "Faster Authoring" },
            ],
        },
    ]
}

/// Website service documentation page.
#[component]
pub fn DocsWebsitePage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-website"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/web.svg" alt="Website" class="toc-icon" />
                        <h2>"Website Service"</h2>
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
                            <span class="current">"Website Service"</span>
                        </div>
                        <h1 class="docs-title">"Website Service"</h1>
                        <p class="docs-subtitle">
                            "The Website service lets a Space publish the numbers a website quotes. Mark
                            each value as a Reference, publish once, and every number on the page updates
                            from one small manifest fetched in a single request."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "19 min read"
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
                    // WHY THIS EXISTS
                    // =========================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Why This Exists"
                        </h2>

                        <div id="overview-drift" class="subsection">
                            <h3>"Retyped Numbers Drift"</h3>
                            <p>
                                "A website that quotes a specification usually retypes it, and retyped numbers
                                drift: the specification changes, the page does not, and nothing connects the
                                two until somebody notices."
                            </p>
                            <p>
                                "The Space already knew the right answer. It computed it. The Website service is
                                the wire between the Space that owns a number and the page that displays it."
                            </p>
                        </div>

                        <div id="overview-shape" class="subsection">
                            <h3>"One Fetch, Not One Per Value"</h3>
                            <p>
                                "One constraint shapes the whole design: a page quoting twenty-five numbers must
                                cost one request, not twenty-five and not one endpoint per value. So the publish
                                bakes every referenced value into a single manifest, and the page fetches that
                                one document."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Why not serve the .pak?"</strong>
                                    <p>
                                        "A "<code>".pak"</code>" is the whole Universe folder as a zstd-compressed
                                        tar, and it exists to carry the world. Asking a browser to download it,
                                        decompress it and parse TOML to recover a few scalars is the wrong shape.
                                        The "<code>".pak"</code>" is unchanged; the manifest is a second, small
                                        object written by the same publish, capped at 1 MB."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-status" class="subsection">
                            <h3>"Status"</h3>
                            <div class="callout callout-new">
                                <img src="/assets/icons/star.svg" alt="New" />
                                <div>
                                    <strong>"Newly built"</strong>
                                    <p>
                                        "The service, all five Reference kinds, the bake at publish and the
                                        manifest routes are new. Studio publishes manifests today, and the witness
                                        at "<code>"api.eustress.dev"</code>" serves them with caching, pinning and
                                        rate limits. Access keys are half built: the witness checks a key when a
                                        publisher attaches one, but Studio does not mint or attach keys yet, so every
                                        manifest Studio publishes is open to any caller. Pin "
                                        <code>"schema_version"</code>" and keep the baked fallbacks described below
                                        in case a field name moves."
                                    </p>
                                </div>
                            </div>
                            <p>
                                "The consumer contract is deliberately generic. Nothing in it is specific to one
                                site, one namespace or one product."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // THE WEBSITE SERVICE
                    // =========================================================
                    <section id="service" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "The Website Service"
                        </h2>

                        <div id="service-folder" class="subsection">
                            <h3>"A Folder of References"</h3>
                            <p>
                                <code>"Website"</code>" is a service, like "<code>"Lighting"</code>" or "
                                <code>"MaterialService"</code>": a folder at the root of a Space that holds one "
                                <code>"Reference"</code>" per value your site displays. Every Space has one. New
                                Spaces are scaffolded with it, and Studio adds it to an older Space the next time
                                that Space opens, so it appears in the Explorer beside Workspace and the other
                                services."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Space layout"</span>
                                </div>
                                <pre><code class="language-text">{r#"Spaces/VCell/
  Website/
    _service.toml
    specific_energy/_instance.toml
    cycles_at_design_rate/_instance.toml
    part_count/_instance.toml
    can_length/_instance.toml"#}</code></pre>
                            </div>
                        </div>

                        <div id="service-config" class="subsection">
                            <h3>"The Service File"</h3>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Website/_service.toml (excerpt)"</span>
                                </div>
                                <pre><code class="language-toml">{r##"[service]
class_name = "Website"
icon = "website"

# The identifier the manifest publishes under, and the name a page writes
# in data-eus="{namespace}:{key}". Publish fails while it is empty.
namespace = "vcell"

# Bumped when the MEANING of a key changes, never when a value does.
# Consumers pin against it, so a rename is a change they opt into.
schema_version = 3"##}</code></pre>
                            </div>
                            <p>
                                "Select the Website service in the Explorer to edit both in Properties, where they
                                appear under "<strong>"Publishing"</strong>" as "<code>"Namespace"</code>" and "
                                <code>"SchemaVersion"</code>". A new Space starts with an empty namespace and
                                schema version 1. Keep every field a flat scalar under "<code>"[service]"</code>
                                ": that is the shape Studio reads and writes back."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A namespace belongs to the first account that publishes it"</strong>
                                    <p>
                                        "The namespace becomes part of a URL and a storage key, so use lowercase
                                        letters, digits and hyphens, start with a letter or digit, and keep it to 64
                                        characters. The witness records the first account to publish a namespace as
                                        its owner, and a publish from any other account under that namespace fails
                                        with "<em>"Namespace already claimed"</em>". Renaming a Space's namespace
                                        releases the old one."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="service-reference" class="subsection">
                            <h3>"What a Reference Is"</h3>
                            <p>
                                "A Reference is a named pointer from a manifest key to a value the Space already
                                holds. It carries the label, unit, format and basis with it, so the consuming page
                                never reinvents any of them. Each Reference is a folder under Website whose "
                                <code>"_instance.toml"</code>" declares "<code>"class_name = Reference"</code>
                                " and keeps its fields in "<code>"[attributes]"</code>":"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Website/specific_energy/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r##"[metadata]
class_name = "Reference"
archivable = true

[attributes]
kind   = "instance"
source = "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
label  = "Specific energy, pack level"
unit   = "Wh/kg"
format = "{:.0}"
basis  = "derived""##}</code></pre>
                            </div>
                            <p>
                                "The folder name is the manifest key; set "<code>"key"</code>" to publish under a
                                different name, and "<code>"label"</code>" defaults to the key. An agent can write
                                these files with the "<code>"website_setup"</code>" and "
                                <code>"website_add_reference"</code>" tools, which Workshop and the "
                                <a href="/learn/mcp">"MCP server"</a>" both offer. "<code>"website_status"</code>
                                " lists what a publish would bake, and "<code>"website_manifest_url"</code>
                                " prints the URL and markup for whoever maintains the site."
                            </p>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"basis travels with every value"</strong>
                                    <p>
                                        "A figure without its basis is not a figure. "<code>"instance"</code>", "
                                        <code>"sim"</code>" and "<code>"expr"</code>" References must declare one
                                        (measured, simulated, derived or authored), or the publish fails; "
                                        <code>"count"</code>" and "<code>"measure"</code>" carry counted and measured
                                        on their own. A site can then render a simulated number differently from a
                                        measured one while the list of which is which stays in the Space."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // THE FIVE REFERENCE KINDS
                    // =========================================================
                    <section id="kinds" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "The Five Reference Kinds"
                        </h2>

                        <div id="kinds-why" class="subsection">
                            <h3>"Why Five"</h3>
                            <p>
                                "The numbers a site quotes come from five different places. A page claiming 2,341
                                parts and 300 x 100 x 100 mm is reading the tree, not a scalar, and a design that
                                only handled scalars would solve half the problem."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"kind"</th><th>"Resolves"</th><th>"Example source"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"instance"</code></td><td>"A property on one instance"</td><td><code>"Workspace/.../Enclosure#electrochemical.capacity_ah"</code></td></tr>
                                    <tr><td><code>"sim"</code></td><td>"A value from a recorded experiment run"</td><td><code>"battery.capacity_retention"</code></td></tr>
                                    <tr><td><code>"count"</code></td><td>"Instances matching a path glob"</td><td><code>"Workspace/V-Cell/V1/Assembly/**"</code></td></tr>
                                    <tr><td><code>"measure"</code></td><td>"A bounding-box measure over a subtree"</td><td><code>"bbox:Workspace/V-Cell/V1/Assembly"</code></td></tr>
                                    <tr><td><code>"expr"</code></td><td>"Arithmetic over other References"</td><td><code>"energy_wh / pack_mass_kg"</code></td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="kinds-instance" class="subsection">
                            <h3>"instance and sim"</h3>
                            <p>
                                <code>"instance"</code>" reads "<code>"path#section.field"</code>", with the path
                                relative to the Space root and dotted fields indexing into TOML tables. Resolution
                                reads the "<strong>"live datamodel"</strong>" first: the instance, transform,
                                material, thermodynamic, electrochemical, attributes and parameters sections come
                                from the running Space, so a value the engine computed or reconciled is the one
                                that gets baked. Other sections have no live form and are read from the stored
                                instance text."
                            </p>
                            <p>
                                <code>"sim"</code>" reads a recorded experiment run, never the live per-frame
                                values, which carry no run identity. The publish looks in the Universe's "
                                <code>".eustress/experiments"</code>" folder for the newest run record whose name
                                equals "<code>"run_label"</code>", then reads the value "<code>"source"</code>
                                " names:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"A sim reference"</span>
                                </div>
                                <pre><code class="language-toml">{r##"[attributes]
kind      = "sim"
source    = "battery.capacity_retention"
run_label = "I_life_25C_5MPa_res0"
at        = "final"        # final | min | max | mean | at_cycle:N
basis     = "simulated""##}</code></pre>
                            </div>
                            <p>
                                <code>"final"</code>" reads the run's final value; "<code>"min"</code>", "
                                <code>"max"</code>" and "<code>"mean"</code>" read its statistics; "
                                <code>"at_cycle:N"</code>" reads the first telemetry sample where the counter named
                                by "<code>"cycle_key"</code>" reaches N. A missing "<code>"run_label"</code>" fails
                                the publish, and so does a label no run carries, with the nearest existing
                                labels suggested. Runs come from experiments; see "<a href="/docs/simulation">"Simulation"</a>"."
                            </p>
                        </div>

                        <div id="kinds-derived" class="subsection">
                            <h3>"count, measure, expr"</h3>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"The derived kinds"</span>
                                </div>
                                <pre><code class="language-toml">{r##"# count: instances under a path glob
kind   = "count"
source = "Workspace/V-Cell/V1/Assembly/**"
filter = "class_name = Part"   # or class_name != Folder, or tag = <name>

# measure: a bounding box over a subtree, converted from meters
kind   = "measure"
source = "bbox:Workspace/V-Cell/V1/Assembly"
axis   = "x"                   # x | y | z | volume | surface
unit   = "mm"

# expr: arithmetic over other reference keys in this namespace
kind   = "expr"
source = "energy_wh / pack_mass_kg"
basis  = "derived""##}</code></pre>
                            </div>
                            <ul class="docs-list">
                                <li><strong>"count"</strong>": "<code>"**"</code>" matches one or more path segments, so "<code>"Assembly/**"</code>" counts what is under Assembly and never Assembly itself. "<code>"*"</code>" stays inside one segment and "<code>"?"</code>" matches one character. A count of zero fails the publish, since it almost always means a wrong path."</li>
                                <li><strong>"measure"</strong>": only "<code>"bbox:"</code>" is computed. It walks every part under the path, the same walk the selection box uses, so the datasheet and the viewport agree by construction. "<code>"axis"</code>" defaults to x, and "<code>"unit"</code>" is a length (m, cm, mm, ft or in) converted from meters. For volume and surface the number is in that unit cubed or squared, so put the cube or square in "<code>"format"</code>", as in "<code>"{:.2} m³"</code>". The prefixes "<code>"hull:"</code>", "<code>"mesh:"</code>" and "<code>"convex:"</code>" are reserved and fail as not implemented."</li>
                                <li><strong>"expr"</strong>": operands are other reference keys, and their units ride along to the manifest without entering the arithmetic. Each expression runs after everything it names, a cycle fails and names the loop, and division by zero or a non-finite result fails too."</li>
                            </ul>
                        </div>
                    </section>

                    // =========================================================
                    // PUBLISH AND BAKE
                    // =========================================================
                    <section id="bake" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Publish and Bake"
                        </h2>

                        <div id="bake-order" class="subsection">
                            <h3>"Where the Bake Runs"</h3>
                            <p>
                                "The bake runs inside "<a href="/docs/publishing">"Publish"</a>", in two halves.
                                Before anything uploads, Studio resolves every Reference against the live Space,
                                and a failure stops the publish there, before a listing exists. After the "
                                <code>".pak"</code>" uploads, Studio stamps the manifest with the new simulation
                                id and the "<code>".pak"</code>"'s hash and sends it to the witness. If that upload
                                fails, the publish fails too."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Publish"</span>
                                </div>
                                <pre><code class="language-text">{r#"resolve every Reference, in dependency order
   |
   +-- any failure  ->  publish stops, no listing created
   |
create the listing, upload the .pak
stamp simulation_id and publish_hash
PUT the manifest  ->  a failed upload fails the publish"#}</code></pre>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"An unchanged Universe does not republish"</strong>
                                    <p>
                                        "Studio hashes the "<code>".pak"</code>" and skips the upload, manifest
                                        included, when the hash matches the last publish. References read live
                                        values, but the "<code>".pak"</code>" is built from the files on disk, so
                                        save the change that moved a number before you publish."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="bake-fails" class="subsection">
                            <h3>"A Failed Reference Fails the Publish"</h3>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"No null, no previous value"</strong>
                                    <p>
                                        "The bake emits no "<code>"null"</code>" and carries no previous value
                                        forward. The failure this feature exists to prevent is a website confidently
                                        displaying a stale number, and a bake that degraded quietly would bring that
                                        failure back one layer further from anyone who would catch it."
                                    </p>
                                </div>
                            </div>
                            <p>"The error names the reference, the source it could not resolve and the nearest candidates in the tree:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Studio notification"</span>
                                </div>
                                <pre><code class="language-text">{r#"Publish stopped: specific_energy: no instance at Workspace/V-Cell/V1/Core/Enclosur (did you mean Workspace/V-Cell/V1/Core/Enclosure?)"#}</code></pre>
                            </div>
                            <p>
                                "That is a fix, where "<em>"resolution error"</em>" would be a ticket. The same
                                rule covers every other failure: a missing basis or run label, two References
                                claiming one key, an unknown operand, a format the engine cannot apply, a count of
                                zero, and a value with no JSON form, such as infinity."
                            </p>
                        </div>

                        <div id="bake-format" class="subsection">
                            <h3>"Formatting display"</h3>
                            <p>
                                <code>"format"</code>" turns the typed value into the "<code>"display"</code>
                                " string. The grammar is small and explicit, and a spec outside it fails the
                                publish instead of rounding some other way:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th><code>"format"</code></th><th>"2341.5 renders as"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"none, or "<code>"{}"</code></td><td><code>"2341.5"</code></td></tr>
                                    <tr><td><code>"{:.0}"</code></td><td><code>"2342"</code></td></tr>
                                    <tr><td><code>"{:.2}"</code></td><td><code>"2341.50"</code></td></tr>
                                    <tr><td><code>"{:,}"</code></td><td><code>"2,341.5"</code></td></tr>
                                    <tr><td><code>"{:,.0}"</code></td><td><code>"2,342"</code></td></tr>
                                    <tr><td><code>"{:e}"</code></td><td><code>"2.3415e3"</code></td></tr>
                                </tbody>
                            </table>
                            <p>
                                <code>"{:.Ne}"</code>" sets the decimals in scientific notation. Text around the
                                placeholder is kept, so "<code>"{:.0} Wh/kg"</code>" renders "
                                <code>"953 Wh/kg"</code>" for 952.6, and "<code>"{{"</code>" and "
                                <code>"}}"</code>" write literal braces. A whole number prints without a decimal
                                point: 237, not 237.0."
                            </p>
                        </div>

                        <div id="bake-where" class="subsection">
                            <h3>"Where the Manifest Lands"</h3>
                            <p>
                                "The manifest is a "<strong>"separate object"</strong>" from the simulation
                                listing. The listing carries the name, description and thumbnail the gallery
                                shows; the manifest carries values. They sit side by side under the same Universe
                                prefix and are written by different requests, so a manifest upload never touches
                                the listing."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Storage layout"</span>
                                </div>
                                <pre><code class="language-text">{r#"universes/{id}/universe.pak              the packaged Universe
universes/{id}/website-manifest.json     the manifest for that publish
thumbnails/{id}/thumb.webp               listing thumbnail"#}</code></pre>
                            </div>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"One worker, one bucket"</strong>
                                    <p>
                                        "Manifests are served by the "<code>"eustress-api"</code>" worker at "
                                        <code>"api.eustress.dev"</code>" out of the "<code>"eustress-simulations"</code>
                                        " bucket, under the "<code>"universes/"</code>" prefix that publish already
                                        writes. The worker also keeps a small index from each namespace to its
                                        latest publish, which is how the namespace route finds the newest manifest."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // CONSUMING THE MANIFEST
                    // =========================================================
                    <section id="consume" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Consuming the Manifest"
                        </h2>

                        <div id="consume-endpoint" class="subsection">
                            <h3>"The Endpoint"</h3>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"HTTP"</span>
                                </div>
                                <pre><code class="language-text">{r#"GET https://api.eustress.dev/api/simulation/{namespace}/latest/manifest
GET https://api.eustress.dev/api/simulation/{simulation_id}/manifest"#}</code></pre>
                            </div>
                            <p>
                                "Use the namespace route. Every publish creates a new listing with a new
                                simulation id, so a URL holding an id is right until the next publish, while the
                                namespace route always serves the latest. The id route is for pinning one publish.
                                The segment counts as an id when it is shaped like a UUID and as a namespace
                                otherwise, and the form without "<code>"latest"</code>" works too."
                            </p>
                            <p>
                                "The route answers any origin with "<code>"Access-Control-Allow-Origin: *"</code>
                                ", and answers CORS preflights the same way, cached for a day."
                            </p>
                        </div>

                        <div id="consume-shape" class="subsection">
                            <h3>"What Comes Back"</h3>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"website-manifest.json"</span>
                                </div>
                                <pre><code class="language-json">{r##"{
  "namespace": "vcell",
  "schema_version": 3,
  "simulation_id": "8f3a1c04-1111-2222-3333-444444444444",
  "publish_hash": "4f9c2b7e...",
  "baked_at": "2026-09-06T07:41:00Z",
  "engine_version": "0.3.6",
  "values": {
    "cycles_at_design_rate": {
      "value": 237,
      "unit": "cycles",
      "label": "Cycles to 80% retention",
      "basis": "simulated",
      "display": "237",
      "source": "battery.capacity_retention",
      "run_label": "I_life_25C_5MPa_res0"
    },
    "specific_energy": {
      "value": 952.6,
      "unit": "Wh/kg",
      "label": "Specific energy, pack level",
      "basis": "derived",
      "format": "{:.0}",
      "display": "953",
      "source": "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
    }
  }
}"##}</code></pre>
                            </div>
                            <p>
                                "Values are keyed by reference name and sorted. "<code>"publish_hash"</code>" is the
                                BLAKE3 digest of the published "<code>".pak"</code>" as 64 hex characters, and it
                                doubles as the ETag. "<code>"value"</code>" is a number, string or boolean, never
                                null; "<code>"unit"</code>" and "<code>"format"</code>" appear when the Reference
                                sets them, and "<code>"run_label"</code>" only on sim values."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Render display. Compute with value."</strong>
                                    <p>
                                        "Every entry carries both. "<code>"value"</code>" is typed, for arithmetic. "
                                        <code>"display"</code>" is formatted by the author, for the DOM. A consumer
                                        that formats "<code>"value"</code>" itself drifts from the author's rounding,
                                        and a consumer that parses "<code>"display"</code>" back into a number gets it
                                        wrong the first time a unit appears in it."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="consume-binding" class="subsection">
                            <h3>"Binding with data-eus"</h3>
                            <p>"Mark up what each element means. Fetch once, for all of them."</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"index.html"</span>
                                </div>
                                <pre><code class="language-text">{r#"<span data-eus="vcell:specific_energy">953</span>
<span data-eus="vcell:specific_energy" data-eus-field="unit">Wh/kg</span>

<span data-eus="vcell:cycles_at_design_rate">237</span>
<span data-eus="vcell:cycles_at_design_rate" data-eus-field="basis">simulated</span>"#}</code></pre>
                            </div>
                            <p>
                                <code>"data-eus"</code>" is the namespace and the key, joined by a colon. "
                                <code>"data-eus-field"</code>" picks which field of the entry to write, and
                                defaults to "<code>"display"</code>"."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/shield.svg" alt="Important" />
                                <div>
                                    <strong>"The element's existing text is the fallback, and it must already be correct"</strong>
                                    <p>
                                        "Bake the current values into the HTML at build time and let the manifest
                                        correct them. The page is already right before the fetch, and the fetch can
                                        only improve it. A page that renders empty until a fetch resolves renders
                                        empty when the fetch fails, and a numeric specification that flashes blank
                                        reads as broken to exactly the audience it is meant to convince."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="consume-hydrate" class="subsection">
                            <h3>"The Hydration Script"</h3>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"hydrate.js"</span>
                                </div>
                                <pre><code class="language-text">{r#"// One fetch for every number on the page. The HTML is already correct; this
// only corrects it for publishes that landed after the last deploy.
const NAMESPACE = 'vcell';
const SCHEMA = 3;      // the schema_version this page was written against
const KEY = '';        // only if the publisher uses a key; public by design
const MANIFEST =
  `https://api.eustress.dev/api/simulation/${NAMESPACE}/latest/manifest`;

async function hydrate() {
  let m;
  try {
    const res = await fetch(MANIFEST, {
      headers: KEY ? { 'X-Eustress-Key': KEY } : {},
      cache: 'default',                  // let the browser revalidate
    });
    if (!res.ok) return;                 // 401, 404, 429, 5xx: keep the baked values
    m = await res.json();
  } catch { return; }                    // offline: keep the baked values

  if (m.schema_version !== SCHEMA) {     // a rename is opt-in, never automatic
    console.warn('eustress: schema', m.schema_version, 'expected', SCHEMA);
    return;
  }

  for (const el of document.querySelectorAll('[data-eus]')) {
    const [ns, key] = el.dataset.eus.split(':');
    if (ns !== m.namespace) continue;
    const v = m.values[key];
    if (!v) { console.warn('eustress: no manifest key', key); continue; }
    const field = el.dataset.eusField || 'display';
    if (v[field] !== undefined) el.textContent = v[field];
  }
  document.documentElement.dataset.eusHash = m.publish_hash;
}
hydrate();"#}</code></pre>
                            </div>
                            <p>
                                "Twenty-five values cost one request, and a twenty-sixth costs nothing. Every early
                                return leaves the baked values standing, which is the whole failure policy in a few
                                lines. A request without the key header is a simple CORS request, so an open
                                manifest costs no preflight at all."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // CACHING, KEYS, FAILURE
                    // =========================================================
                    <section id="caching" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Caching, Keys, Failure"
                        </h2>

                        <div id="caching-headers" class="subsection">
                            <h3>"Response Headers"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Header"</th><th>"Value"</th><th>"Why"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"ETag"</code></td><td>"The publish hash, in quotes"</td><td>"The engine already computes it for every publish"</td></tr>
                                    <tr><td><code>"Cache-Control"</code></td><td><code>"public, max-age=300, stale-while-revalidate=86400"</code></td><td>"A repeat visitor costs nothing, and a stale copy renders while a fresh one arrives"</td></tr>
                                    <tr><td><code>"Vary"</code></td><td><code>"X-Eustress-Key"</code></td><td>"A shared cache never hands a keyed response to a caller without the key"</td></tr>
                                    <tr><td><code>"Access-Control-Allow-Origin"</code></td><td><code>"*"</code></td><td>"Manifests are published content; a key, not the origin, identifies a caller"</td></tr>
                                    <tr><td><code>"Access-Control-Expose-Headers"</code></td><td><code>"ETag"</code></td><td>"Cross-origin JavaScript, such as a build step, can read the hash"</td></tr>
                                </tbody>
                            </table>
                            <p>"Four consequences follow:"</p>
                            <ul class="docs-list">
                                <li>"A repeat visit inside five minutes makes no network request."</li>
                                <li>"After five minutes the browser serves the cached copy and revalidates behind it, so the visitor waits for nothing."</li>
                                <li>"An unchanged manifest answers "<code>"304"</code>" with no body."</li>
                                <li>"A publish changes the hash, so the next revalidation returns "<code>"200"</code>"."</li>
                            </ul>
                            <p>
                                "Error responses carry "<code>"Cache-Control: no-store"</code>", so a rejection never
                                sticks in a cache. Reads are limited to 120 a minute per key, or per namespace for
                                an open manifest, counted per Cloudflare location."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Skip the cache-busting query string"</strong>
                                    <p>
                                        "Appending a timestamp on every load defeats revalidation, turns a free "
                                        <code>"304"</code>" into a full download on every visit and spends the rate
                                        limit on nothing. Publishing is the refresh. There is one mechanism, because
                                        a second mechanism is a second thing to forget."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="caching-pin" class="subsection">
                            <h3>"Pinning a State"</h3>
                            <p>
                                "A page that must show one exact state pins it, passing "<code>"publish_hash"</code>
                                " exactly as the manifest carries it. A pinned response is immutable and cacheable
                                for a year."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Pinned request"</span>
                                </div>
                                <pre><code class="language-text">{r#"GET https://api.eustress.dev/api/simulation/{simulation_id}/manifest?v={publish_hash}"#}</code></pre>
                            </div>
                            <p>
                                "Pin through the simulation id. The namespace route moves to each new publish, so a
                                namespace pin answers "<code>"404"</code>" with "
                                <code>"pinned_hash_not_available"</code>" as soon as you publish again, rather than
                                serving a different state. The id route keeps serving the publish it names. Use it
                                for anything that quotes a specific revision: a signed document, a datasheet, a
                                figure a reader may come back to."
                            </p>
                        </div>

                        <div id="caching-keys" class="subsection">
                            <h3>"Access Keys"</h3>
                            <p>
                                "A manifest is open unless its publisher attached a key to the upload. Studio does
                                not attach one yet, so manifests published from Studio today need no key. The
                                witness side of keys is already in place:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Sending"</strong>": consumers send the key in the "<code>"X-Eustress-Key"</code>" header, or as "<code>"?key="</code>" where a header is impossible. A key in a URL lands in access logs, Referer headers and browser history, so prefer the header."</li>
                                <li><strong>"Checking"</strong>": the witness stores only a SHA-256 hash of the key. A missing key gets "<code>"401 auth_key_required"</code>" and a wrong one "<code>"401 auth_key_invalid"</code>"."</li>
                                <li><strong>"Rotating"</strong>": when a key changes, the previous one keeps working for 30 days by default, so a live site can redeploy on its own schedule. A window of 0 cuts the old key off at once."</li>
                            </ul>
                            <p>
                                "The Access rows in the Website service's Properties ("<code>"KeyId"</code>", "
                                <code>"RotateKey"</code>" and the rest) are not wired to the witness yet: turning "
                                <code>"RotateKey"</code>" on mints no key."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/shield.svg" alt="Important" />
                                <div>
                                    <strong>"The key is attribution, not secrecy"</strong>
                                    <p>
                                        "A key that a public website sends from browser JavaScript is visible to anyone
                                        who opens devtools, reads the page source or watches the network tab. It tells
                                        the author who is calling, lets a rate limit apply per caller and can be cut
                                        off by rotation. It leaves the manifest readable by anyone who copies the key
                                        out of the page. Real confidentiality needs a server-side proxy holding a
                                        secret, or short-lived signed tokens, and this is neither. Publish only values
                                        you are willing to have read."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="caching-failure" class="subsection">
                            <h3>"The Failure Table"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Condition"</th><th>"Behavior"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Network unreachable"</td><td>"Keep the baked values, no visible change"</td></tr>
                                    <tr><td><code>"401"</code>": key missing, wrong, or past its overlap window"</td><td>"Keep the baked values, log"</td></tr>
                                    <tr><td><code>"404"</code>": unknown namespace, no manifest yet, or a pin that no longer matches"</td><td>"Keep the baked values, log"</td></tr>
                                    <tr><td><code>"429"</code>": over the rate limit, with "<code>"Retry-After: 60"</code></td><td>"Keep the baked values, back off"</td></tr>
                                    <tr><td>"Any other error"</td><td>"Keep the baked values, log"</td></tr>
                                    <tr><td><code>"schema_version"</code>" mismatch"</td><td>"Keep the baked values, log, apply nothing at all"</td></tr>
                                    <tr><td>"Key missing from the manifest"</td><td>"Leave that element alone, log the key"</td></tr>
                                    <tr><td>"A "<code>"null"</code>" value"</td><td>"Cannot happen: a failed reference fails the publish"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "One rule covers every row: "<strong>"the page is already correct before the fetch,
                                and the fetch can only improve it."</strong>" Anything else turns a network problem
                                into a credibility problem."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // CONSUMER PRACTICE
                    // =========================================================
                    <section id="practice" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "Consumer Practice"
                        </h2>

                        <div id="practice-build" class="subsection">
                            <h3>"Build-Time Baking"</h3>
                            <p>"The same manifest feeds the build. That is what keeps the fallbacks honest."</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Pre-deploy step"</span>
                                </div>
                                <pre><code class="language-text">{r#"fetch manifest  ->  write values into the HTML  ->  deploy"#}</code></pre>
                            </div>
                            <p>
                                "Run it as a pre-deploy step, and fail the build when a "<code>"data-eus"</code>
                                " key has no manifest entry. That catches a renamed reference at build time instead
                                of on a visitor's screen, and it means the committed HTML always shows the state of
                                the last publish. With build-time baking in place, runtime hydration becomes a
                                correction for publishes that landed after the last deploy: the build keeps the
                                page correct, and the fetch keeps it current."
                            </p>
                        </div>

                        <div id="practice-limits" class="subsection">
                            <h3>"Four Honest Limits"</h3>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/clock.svg" alt="No push" />
                                    </div>
                                    <h4>"It Does Not Push"</h4>
                                    <p>"A site learns about a publish on its next fetch. With a five-minute max-age, that is the update latency."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/list.svg" alt="Scalars" />
                                    </div>
                                    <h4>"It Carries Scalars"</h4>
                                    <p>"Each value is one number, string or boolean, in a manifest of at most 1 MB."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/shield.svg" alt="Public" />
                                    </div>
                                    <h4>"It Does Not Make Values Private"</h4>
                                    <p>"Manifests are open today, and a key would name callers, not hide values."</p>
                                </div>
                                <div class="feature-card">
                                    <div class="feature-icon">
                                        <img src="/assets/icons/archive.svg" alt="One state" />
                                    </div>
                                    <h4>"It Follows the Latest Publish"</h4>
                                    <p>"The namespace route shows the newest state. An older one is reachable only by its simulation id."</p>
                                </div>
                            </div>
                        </div>

                        <div id="practice-checklist" class="subsection">
                            <h3>"Checklist for a New Consumer"</h3>
                            <ol class="numbered-list">
                                <li>"Pick the namespace and pin "<code>"schema_version"</code>"."</li>
                                <li>"If the publisher uses a key, get it from them. Manifests Studio publishes today are open."</li>
                                <li>"Mark values with "<code>"data-eus"</code>", carrying the current correct value as the element's text."</li>
                                <li>"Add the hydration script once, at the end of the document."</li>
                                <li>"Add the build-time bake, and make a missing key fail the build."</li>
                                <li>"Load the page with the network disabled and read every number."</li>
                            </ol>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Step 6 is the one that matters"</strong>
                                    <p>
                                        "If the page is right with the network off, the manifest is an improvement.
                                        If it is wrong with the network off, the manifest is a dependency, and you
                                        have moved your credibility onto someone else's uptime."
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

                        <div id="roadmap-keys" class="subsection">
                            <h3>"Keys from Studio"</h3>
                            <p>
                                "Turning on "<code>"RotateKey"</code>" will mint a key, show it once and never store
                                it in the Space, because "<code>"Website/_service.toml"</code>" ships inside the
                                published "<code>".pak"</code>". The next publish will attach it, and the previous key
                                will keep working for "<code>"KeyOverlapDays"</code>". Keys will come in two kinds,
                                told apart by prefix: "<code>"eus_pk_"</code>" for a browser key that ships in a
                                page, and "<code>"eus_bk_"</code>" for a build key held as a CI secret, each
                                revocable on its own."
                            </p>
                        </div>

                        <div id="roadmap-authoring" class="subsection">
                            <h3>"Faster Authoring"</h3>
                            <p>
                                <strong>"Add to Website"</strong>" in the Explorer's context menu will create a
                                Reference pre-filled with the selected instance's path, and the Website service
                                will show each Reference's current resolved value, so a wrong path shows up before
                                publishing rather than after. Measures beyond bounding boxes will each arrive under
                                their own prefix, so no existing Reference changes meaning."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Publish once. Every number on the page follows."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/publishing" class="btn-secondary-steel">"Publishing Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/publishing" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Publishing"</span>
                            </div>
                        </a>
                        <a href="/docs/earning" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Earning"</span>
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
