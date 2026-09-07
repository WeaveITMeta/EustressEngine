// =============================================================================
// Eustress Web - Website Service Documentation Page
// =============================================================================
// Teaches both halves of the feature, because it is only useful as a pair:
// the Eustress side (the Website service, References, the bake that runs during
// publish) and the consumer side (the manifest shape, data-eus binding, the
// hydration script, caching, and the failure policy).
//
// The load-bearing idea is the fallback rule: the element's existing text must
// already be correct, so the fetch can only improve the page. Every other rule
// on this page follows from it.
//
// Source specs: docs/design/WEBSITE_SERVICE.md and Voltec's
// docs/WEBSITE_MANIFEST_API.md. Keep this page in step with both.
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
                TocSubsection { id: "bake-flow", title: "Authoring Flow" },
                TocSubsection { id: "bake-where", title: "Where the Manifest Lands" },
            ],
        },
        TocSection {
            id: "key",
            title: "The Manifest Key",
            subsections: vec![
                TocSubsection { id: "key-what", title: "What the Key Buys" },
                TocSubsection { id: "key-not", title: "What It Does Not Buy" },
                TocSubsection { id: "key-rotate", title: "Rotating a Key" },
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
            title: "Caching, Pinning, Failure",
            subsections: vec![
                TocSubsection { id: "caching-headers", title: "Response Headers" },
                TocSubsection { id: "caching-pin", title: "Pinning a State" },
                TocSubsection { id: "caching-failure", title: "The Failure Table" },
                TocSubsection { id: "caching-build", title: "Build-Time Baking" },
            ],
        },
        TocSection {
            id: "limits",
            title: "What This Does Not Do",
            subsections: vec![
                TocSubsection { id: "limits-scope", title: "Four Honest Limits" },
                TocSubsection { id: "limits-checklist", title: "Checklist for a New Consumer" },
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
                <div class="docs-glow glow-physics"></div>
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
                            "A Space owns its numbers. Mark the values a website quotes as References,
                            publish once, and every number on the page updates from a single fetch."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "14 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/brain.svg" alt="Level" />
                                "Reference"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "New in v0.16.1"
                            </span>
                        </div>
                    </header>

                    // 1. WHY THIS EXISTS
                    <section id="overview" class="docs-section">
                        <h2 class="section-anchor">"Why This Exists"</h2>

                        <div id="overview-drift" class="subsection">
                            <h3>"Retyped Numbers Drift"</h3>
                            <p>
                                "A website that quotes a specification retypes it. Retyped numbers drift.
                                The V-Cell site carried 907 Wh/kg and 699 cycles for a week after the
                                specification said 953 and 237, because nothing connected the two."
                            </p>
                            <p>
                                "The Space already knew the right answer. It computed it. The Website
                                service is the wire between the Space that owns a number and the page
                                that displays it."
                            </p>
                        </div>

                        <div id="overview-shape" class="subsection">
                            <h3>"One Fetch, Not One Per Value"</h3>
                            <p>
                                "One constraint shapes the whole design: a page quoting twenty-five
                                numbers must cost one request. Not twenty-five. Not one endpoint per
                                value. So the publish bakes every referenced value into a single
                                manifest, and the page fetches that one document."
                            </p>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Why not serve the .pak?"</strong>
                                    <p>"A "<code>".pak"</code>" is tar plus zstd of the whole Universe
                                    directory. For the V-Cell Space that is 58 MB across 2,356 instance
                                    files, and it exists so the Player can open the world. Asking a
                                    browser to pull it, decompress it, and parse TOML to recover
                                    twenty-five scalars is the wrong shape by three orders of magnitude.
                                    The "<code>".pak"</code>" is unchanged. The manifest is a second,
                                    small object written by the same publish."</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-status" class="subsection">
                            <h3>"Status"</h3>
                            <div class="callout callout-new">
                                <img src="/assets/icons/tag.svg" alt="New" />
                                <div>
                                    <strong>"Newly built."</strong>
                                    <p>"The Website service, the bake, and the manifest route are new.
                                    This is not a long-established API, and the page says so rather than
                                    implying otherwise. Treat the property names and the route below as
                                    current rather than settled. A site built against this today should
                                    pin "<code>"schema_version"</code>", keep the baked fallbacks the
                                    failure table describes, and expect a release note if a field name
                                    moves."</p>
                                </div>
                            </div>
                            <p>
                                "The consumer contract is deliberately generic. Nothing in it is
                                specific to one site, one namespace, or one product."
                            </p>
                        </div>
                    </section>

                    // 2. THE WEBSITE SERVICE
                    <section id="service" class="docs-section">
                        <h2 class="section-anchor">"The Website Service"</h2>

                        <div id="service-folder" class="subsection">
                            <h3>"A Folder of References"</h3>
                            <p>
                                <code>"Website"</code>" is a Service, like "<code>"Lighting"</code>" or
                                "<code>"MaterialService"</code>": a folder at the root of a Space. It
                                holds "<code>"Reference"</code>" instances, one per value your site
                                displays."
                            </p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"Space layout"</span>
                                </div>
                                <pre><code class="language-text">{r#"Spaces/VCell/
  Website/
    _service.toml
    specific_energy/_instance.toml         class_name = "Reference"
    cycles_at_design_rate/_instance.toml
    part_count/_instance.toml
    can_length/_instance.toml"#}</code></pre>
                            </div>

                            <p>
                                "New Spaces get the service automatically. It appears in the Explorer
                                alongside Workspace, Lighting, and every other Service."
                            </p>
                        </div>

                        <div id="service-config" class="subsection">
                            <h3>"The Service File"</h3>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"Website/_service.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[properties]
name = "Website"
class_name = "WebsiteService"

[website]
# Stable identifier the manifest publishes under. A Space may serve more than
# one site; each site gets its own namespace.
namespace = "vcell"

# Bumped when the MEANING of a key changes, not when its value changes.
# Consumers pin against this, so a rename is a breaking change they opt into.
schema_version = 3

# Sent by the consumer on every manifest request. Revocable attribution and
# rate limiting, not secrecy. See "The Manifest Key" below.
manifest_key = "eus_pk_7f3c1a94d2e05b86"
manifest_key_rotated_at = "2026-08-26T07:41:00Z""#}</code></pre>
                            </div>

                            <p>
                                "Every field is editable in Properties, the same polymorphic panel every
                                other class uses. Namespace and schema version sit under "
                                <strong>"Data"</strong>". The key and its rotation control sit under "
                                <strong>"Security"</strong>"."
                            </p>
                        </div>

                        <div id="service-reference" class="subsection">
                            <h3>"What a Reference Is"</h3>
                            <p>
                                "A Reference is a named pointer from a manifest key to a value the Space
                                already holds. It carries the label, unit, format, and basis with it, so
                                the consuming page never reinvents any of them."
                            </p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"Website/specific_energy/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[properties]
name = "specific_energy"
class_name = "Reference"

[reference]
kind   = "instance"
source = "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
label  = "Specific energy, pack level"
unit   = "Wh/kg"
format = "{:.0}"
basis  = "derived""#}</code></pre>
                            </div>

                            <p>
                                "The manifest key defaults to the instance name. Set "<code>"key"</code>
                                " explicitly to override it."
                            </p>

                            <div class="callout callout-tip">
                                <img src="/assets/icons/check.svg" alt="Tip" />
                                <div>
                                    <strong>"basis travels with every value."</strong>
                                    <p>"A figure without its basis is not a figure. Marking a number
                                    "<code>"measured"</code>", "<code>"simulated"</code>", or
                                    "<code>"derived"</code>" in the Space lets a site render the three
                                    differently while the list of which is which stays in the one place
                                    that knows."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // 3. THE FIVE REFERENCE KINDS
                    <section id="kinds" class="docs-section">
                        <h2 class="section-anchor">"The Five Reference Kinds"</h2>

                        <div id="kinds-why" class="subsection">
                            <h3>"Why Five"</h3>
                            <p>
                                "The numbers a site quotes come from five different places. A page
                                claiming 2,341 parts and 300 x 100 x 100 mm is reading the tree, not a
                                scalar, and a design that only handles scalars solves half the problem."
                            </p>

                            <table class="docs-table">
                                <thead>
                                    <tr><th>"kind"</th><th>"Resolves"</th><th>"Example source"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><code>"instance"</code></td>
                                        <td>"A property on one instance"</td>
                                        <td><code>"Workspace/.../Enclosure#electrochemical.capacity_ah"</code></td>
                                    </tr>
                                    <tr>
                                        <td><code>"sim"</code></td>
                                        <td>"A published simulation value"</td>
                                        <td><code>"battery.specific_energy_wh_kg"</code></td>
                                    </tr>
                                    <tr>
                                        <td><code>"count"</code></td>
                                        <td>"Entities matching a path glob"</td>
                                        <td><code>"Workspace/V-Cell/V1/Assembly/**"</code></td>
                                    </tr>
                                    <tr>
                                        <td><code>"measure"</code></td>
                                        <td>"A geometric measure over a subtree"</td>
                                        <td><code>"bbox:Workspace/V-Cell/V1/Assembly"</code></td>
                                    </tr>
                                    <tr>
                                        <td><code>"expr"</code></td>
                                        <td>"Arithmetic over other references"</td>
                                        <td><code>"energy_wh / pack_mass_kg"</code></td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="kinds-instance" class="subsection">
                            <h3>"instance and sim"</h3>
                            <p>
                                <code>"instance"</code>" reads a path, a section, and a field, written
                                "<code>"path#section.field"</code>", with the path relative to the Space
                                root. Dotted fields index into TOML tables. Resolution reads the "
                                <strong>"live datamodel"</strong>" rather than the file on disk, so a
                                value the engine computed or reconciled is the one that gets baked."
                            </p>
                            <p>
                                <code>"sim"</code>" reads the published simulation namespace, and it
                                must name the run it came from."
                            </p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"A sim reference"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[reference]
kind      = "sim"
source    = "battery.capacity_retention"
run_label = "I_life_25C_5MPa_res0"
at        = "final"        # final | min | max | mean | at_cycle:N"#}</code></pre>
                            </div>

                            <p>
                                "Publishing fails without "<code>"run_label"</code>". A number lifted
                                from whatever happened to be in memory is how a figure reaches a website
                                with no way to reproduce it."
                            </p>
                        </div>

                        <div id="kinds-derived" class="subsection">
                            <h3>"count, measure, expr"</h3>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"The derived kinds"</span>
                                </div>
                                <pre><code class="language-toml">{r#"# count: how many entities match a glob
kind   = "count"
source = "Workspace/V-Cell/V1/Assembly/**"
filter = "class_name = Part"

# measure: a geometric measure over a subtree
kind   = "measure"
source = "bbox:Workspace/V-Cell/V1/Assembly"
axis   = "x"               # x | y | z | volume | surface
unit   = "mm"

# expr: arithmetic over other reference keys in the same namespace
kind   = "expr"
source = "energy_wh / pack_mass_kg""#}</code></pre>
                            </div>

                            <p>
                                <code>"measure"</code>" resolves against the same tree the viewport
                                draws. That is what makes the drawing and the datasheet agreeing a
                                property of the system instead of something a person checks."
                            </p>
                            <p>
                                <code>"expr"</code>" operands are other reference keys in the same
                                namespace. Evaluation runs as a directed acyclic graph. A cycle fails
                                the publish and names the loop."
                            </p>
                        </div>
                    </section>

                    // 4. PUBLISH AND BAKE
                    <section id="bake" class="docs-section">
                        <h2 class="section-anchor">"Publish and Bake"</h2>

                        <div id="bake-order" class="subsection">
                            <h3>"Where the Bake Runs"</h3>
                            <p>
                                "Bake runs as part of publish, after the "<code>".pak"</code>" is
                                packaged and before upload."
                            </p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"Publish"</span>
                                </div>
                                <pre><code class="language-text">{r#"resolve every Reference in dependency order
   |
   +-- unresolved, ambiguous, or type-mismatched  ->  PUBLISH FAILS
   |
emit website-manifest.json
upload .pak + website-manifest.json"#}</code></pre>
                            </div>
                        </div>

                        <div id="bake-fails" class="subsection">
                            <h3>"A Failed Reference Fails the Publish"</h3>

                            <div class="callout callout-advanced">
                                <img src="/assets/icons/help.svg" alt="Important" />
                                <div>
                                    <strong>"A failed reference fails the publish."</strong>
                                    <p>"The bake emits no "<code>"null"</code>", and it carries no
                                    previous value forward. The failure this feature exists to prevent
                                    is a website confidently displaying a stale number. A bake that
                                    degrades quietly reintroduces that failure in a new place, one layer
                                    further from anyone who would catch it."</p>
                                </div>
                            </div>

                            <p>
                                "The failure message names the reference, the source it could not
                                resolve, and the nearest candidates in the tree. "
                                <code>"specific_energy: no instance at .../Core/Enclosur (did you mean Enclosure?)"</code>
                                " is a fix. "<code>"resolution error"</code>" is a ticket."
                            </p>
                        </div>

                        <div id="bake-flow" class="subsection">
                            <h3>"Authoring Flow"</h3>
                            <ol>
                                <li>"Add a Reference under the Website service and give it the manifest
                                key you want the site to read."</li>
                                <li>"Point it at a source: an instance path and field, a simulation
                                value and its run, a glob, a measure, or an expression."</li>
                                <li>"Set label, unit, format, and basis in Properties."</li>
                                <li><strong>"Publish."</strong>" The bake resolves everything, or it
                                fails and says which reference and why."</li>
                            </ol>
                            <p>
                                "Two conveniences sit on top of that flow and land after the core:
                                "<strong>"Add to Website"</strong>" in the Explorer context menu, which
                                creates the Reference pre-filled with the source path of whatever is
                                selected, and a live preview column carrying each reference's current
                                resolved value, so a wrong path shows up before publishing rather than
                                after. The flow above works with or without them."
                            </p>
                        </div>

                        <div id="bake-where" class="subsection">
                            <h3>"Where the Manifest Lands"</h3>
                            <p>
                                "The website manifest is a "<strong>"separate object"</strong>" from the
                                simulation listing record. The listing carries the name, description,
                                and thumbnail the marketplace shows. The website manifest carries
                                values. They sit side by side under the same Universe prefix and stay
                                separate."
                            </p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"R2 layout"</span>
                                </div>
                                <pre><code class="language-text">{r#"universes/{id}/universe.pak              the packaged Universe
universes/{id}/spaces/{name}.pak         per-Space packages
universes/{id}/website-manifest.json     <- the Website service writes this
thumbnails/{id}/thumb.webp               listing thumbnail"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"One worker, one bucket."</strong>
                                    <p>"Manifests are served by the "<code>"eustress-api"</code>" worker
                                    at "<code>"api.eustress.dev"</code>", out of the
                                    "<code>"eustress-simulations"</code>" bucket, under the
                                    "<code>"universes/"</code>" prefix that publish already writes.
                                    There is one worker and one key namespace to keep in step."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // 5. THE MANIFEST KEY
                    <section id="key" class="docs-section">
                        <h2 class="section-anchor">"The Manifest Key"</h2>

                        <div id="key-what" class="subsection">
                            <h3>"What the Key Buys"</h3>
                            <p>
                                "Every manifest request carries a key issued by the Website service. The
                                key buys three things, all of them real:"
                            </p>
                            <ul>
                                <li><strong>"Attribution."</strong>" You can see which consumer is
                                calling, and how often."</li>
                                <li><strong>"Rate limiting."</strong>" Limits apply per key, so one
                                runaway consumer leaves the rest of them alone."</li>
                                <li><strong>"Revocation."</strong>" Rotate the key and the previous one
                                stops working on the next request."</li>
                            </ul>
                        </div>

                        <div id="key-not" class="subsection">
                            <h3>"What It Does Not Buy"</h3>

                            <div class="callout callout-advanced">
                                <img src="/assets/icons/shield.svg" alt="Important" />
                                <div>
                                    <strong>"The key is attribution, not secrecy."</strong>
                                    <p>"A key that a public website sends from browser JavaScript is
                                    visible to anyone who opens devtools, reads the page source, or
                                    watches the network tab. Treat it as a publishable identifier, which
                                    is why issued keys carry the "<code>"eus_pk_"</code>" prefix. It
                                    tells you who is calling and lets you cut them off. It leaves the
                                    manifest readable by anyone who copies the key out of the page. Real
                                    confidentiality needs a server-side proxy holding a secret, or
                                    short-lived signed tokens, and this is neither. Publish only values
                                    you are willing to have read."</p>
                                </div>
                            </div>

                            <p>
                                "The practical rule is the same one that governs a Reference: a value
                                that would embarrass you on a public page belongs somewhere other than
                                the Website service."
                            </p>
                        </div>

                        <div id="key-rotate" class="subsection">
                            <h3>"Rotating a Key"</h3>
                            <p>
                                "Rotate from the Website service Properties panel. Rotation issues a new
                                key, stamps "<code>"manifest_key_rotated_at"</code>", and takes effect
                                on the next publish."
                            </p>

                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Step"</th><th>"What happens"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Rotate in Properties"</td>
                                        <td>"A new "<code>"eus_pk_"</code>" key is generated and written to "<code>"_service.toml"</code></td>
                                    </tr>
                                    <tr>
                                        <td>"Publish"</td>
                                        <td>"The new key becomes the one the route accepts"</td>
                                    </tr>
                                    <tr>
                                        <td>"Update the consumer"</td>
                                        <td>"The site's key constant changes, and the site redeploys"</td>
                                    </tr>
                                    <tr>
                                        <td>"Old key"</td>
                                        <td>"Rejected with 401 from the moment the publish lands"</td>
                                    </tr>
                                </tbody>
                            </table>

                            <div class="callout callout-tip">
                                <img src="/assets/icons/check.svg" alt="Tip" />
                                <div>
                                    <strong>"Rotation is a clean break, on purpose."</strong>
                                    <p>"A consumer still holding the old key gets a 401 and keeps its
                                    baked values, so the page goes on showing the state of its last
                                    deploy instead of blanking. Rotate first, then redeploy the site."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // 6. CONSUMING THE MANIFEST
                    <section id="consume" class="docs-section">
                        <h2 class="section-anchor">"Consuming the Manifest"</h2>

                        <div id="consume-endpoint" class="subsection">
                            <h3>"The Endpoint"</h3>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"HTTP"</span>
                                </div>
                                <pre><code class="language-text">{r#"GET https://api.eustress.dev/api/simulations/{id}/website-manifest
      ?key={manifest_key}

GET https://api.eustress.dev/api/simulations/{id}/website-manifest
      ?key={manifest_key}&v={publish_hash}"#}</code></pre>
                            </div>

                            <p>
                                <code>"{id}"</code>" is the Universe UUID or "
                                <code>"{namespace}/latest"</code>", so a site can reference a Space
                                without hardcoding a UUID."
                            </p>
                            <p>
                                "The key travels as a query parameter rather than a custom header on
                                purpose. A custom request header makes the fetch non-simple, so the
                                browser sends a CORS preflight before every manifest read, and that
                                doubles the round trips this design exists to avoid. Server-side callers
                                may send "<code>"X-Eustress-Key"</code>" instead, where preflight does
                                not apply."
                            </p>
                        </div>

                        <div id="consume-shape" class="subsection">
                            <h3>"What Comes Back"</h3>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"website-manifest.json"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "namespace": "vcell",
  "schema_version": 3,
  "space_id": "8f3a1c74-...",
  "publish_hash": "sha256:1c9fa83...",
  "baked_at": "2026-08-26T07:41:00Z",
  "engine_version": "0.1.0",
  "values": {
    "specific_energy": {
      "value": 953,
      "unit": "Wh/kg",
      "label": "Specific energy, pack level",
      "basis": "derived",
      "format": "{:.0}",
      "display": "953",
      "source": "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
    },
    "cycles_at_design_rate": {
      "value": 237,
      "unit": "cycles",
      "label": "Cycles to 80% retention, 0.25C charge",
      "basis": "simulated",
      "display": "237",
      "source": "battery.capacity_retention",
      "run_label": "I_life_25C_5MPa_res0"
    }
  }
}"#}</code></pre>
                            </div>

                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Render display. Compute with value."</strong>
                                    <p>"Every entry carries both. "<code>"value"</code>" is typed, for
                                    arithmetic. "<code>"display"</code>" is formatted by the author, for
                                    the DOM. A consumer that formats "<code>"value"</code>" itself
                                    drifts from the specification's own rounding, and a consumer that
                                    parses "<code>"display"</code>" back into a number gets it wrong the
                                    first time a unit appears in it."</p>
                                </div>
                            </div>
                        </div>

                        <div id="consume-binding" class="subsection">
                            <h3>"Binding with data-eus"</h3>
                            <p>"Mark up what each element means. Fetch once, for all of them."</p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"index.html"</span>
                                </div>
                                <pre><code class="language-html">{r#"<span data-eus="vcell:specific_energy">953</span>
<span data-eus="vcell:specific_energy" data-eus-field="unit">Wh/kg</span>

<span data-eus="vcell:cycles_at_design_rate">237</span>
<span data-eus="vcell:cycles_at_design_rate" data-eus-field="basis">simulated</span>"#}</code></pre>
                            </div>

                            <p>
                                <code>"data-eus"</code>" is the namespace and the key, joined by a
                                colon. "<code>"data-eus-field"</code>" picks which field of the entry to
                                write, and defaults to "<code>"display"</code>"."
                            </p>

                            <div class="callout callout-advanced">
                                <img src="/assets/icons/shield.svg" alt="Important" />
                                <div>
                                    <strong>"The element's existing text is the fallback, and it must already be correct."</strong>
                                    <p>"Bake the current values into the HTML at build time and let the
                                    manifest correct them. The page is already right before the fetch,
                                    and the fetch can only improve it. A page that renders empty until a
                                    fetch resolves renders empty when the fetch fails, and a numeric
                                    specification that flashes blank reads as broken to exactly the
                                    audience it is meant to convince."</p>
                                </div>
                            </div>
                        </div>

                        <div id="consume-hydrate" class="subsection">
                            <h3>"The Hydration Script"</h3>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"hydrate.js"</span>
                                </div>
                                <pre><code class="language-javascript">{r#"// One fetch for every number on the page. The HTML is already correct; this
// only corrects it for publishes that landed after the last deploy.
const SPACE = 'vcell/latest';
const KEY   = 'eus_pk_7f3c1a94d2e05b86';   // publishable: visible in devtools by design
const MANIFEST =
  `https://api.eustress.dev/api/simulation/${NAMESPACE}/latest/manifest`;

async function hydrate() {
  let m;
  try {
    const res = await fetch(MANIFEST, { cache: 'default' });
    if (!res.ok) return;                 // 401, 429, 5xx: keep the baked values
    m = await res.json();
  } catch { return; }                    // offline: keep the baked values

  if (m.schema_version !== 3) {          // a rename is opt-in, never automatic
    console.warn('manifest schema', m.schema_version, 'expected 3');
    return;
  }

  for (const el of document.querySelectorAll('[data-eus]')) {
    const [ns, key] = el.dataset.eus.split(':');
    if (ns !== m.namespace) continue;
    const v = m.values[key];
    if (!v) { console.warn('no manifest key', key); continue; }
    const field = el.dataset.eusField || 'display';
    if (v[field] !== undefined) el.textContent = v[field];
  }
  document.documentElement.dataset.eusHash = m.publish_hash;
}
hydrate();"#}</code></pre>
                            </div>

                            <p>
                                "Twenty-five values cost one request. Adding a twenty-sixth costs
                                nothing. Every early return in that function leaves the baked values
                                standing, which is the whole failure policy expressed in four lines."
                            </p>
                        </div>
                    </section>

                    // 7. CACHING, PINNING, FAILURE
                    <section id="caching" class="docs-section">
                        <h2 class="section-anchor">"Caching, Pinning, Failure"</h2>

                        <div id="caching-headers" class="subsection">
                            <h3>"Response Headers"</h3>

                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Header"</th><th>"Value"</th><th>"Why"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><code>"ETag"</code></td>
                                        <td><code>"the publish hash"</code></td>
                                        <td>"The engine already computes it, and already skips uploads when it is unchanged"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"Cache-Control"</code></td>
                                        <td><code>"public, max-age=300, stale-while-revalidate=86400"</code></td>
                                        <td>"A repeat visitor costs nothing, and a stale manifest still renders while the fresh one arrives"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"Access-Control-Allow-Origin"</code></td>
                                        <td><code>"*"</code></td>
                                        <td>"Manifests are published content; the key is what identifies a caller, not the origin"</td>
                                    </tr>
                                </tbody>
                            </table>

                            <p>"Four consequences follow from those two headers:"</p>
                            <ul>
                                <li>"A repeat visit inside five minutes makes no network request."</li>
                                <li>"After five minutes the browser serves the cached copy and
                                revalidates behind it, so the visitor waits for nothing."</li>
                                <li>"An unchanged manifest returns "<code>"304"</code>" with no body."</li>
                                <li>"A publish changes the hash, so the next revalidation returns
                                "<code>"200"</code>"."</li>
                            </ul>

                            <div class="callout callout-advanced">
                                <img src="/assets/icons/help.svg" alt="Important" />
                                <div>
                                    <strong>"Skip the cache-busting query string."</strong>
                                    <p>"Appending a timestamp on every load defeats the revalidation
                                    path and turns a free "<code>"304"</code>" into a full download on
                                    every visit. Publishing is the refresh. There is one mechanism,
                                    because a second mechanism is a second thing to forget."</p>
                                </div>
                            </div>
                        </div>

                        <div id="caching-pin" class="subsection">
                            <h3>"Pinning a State"</h3>
                            <p>"A page that must show one exact state pins the hash:"</p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"Pinned request"</span>
                                </div>
                                <pre><code class="language-text">{r#"GET /api/simulations/vcell/latest/website-manifest
      ?key=eus_pk_7f3c1a94d2e05b86
      &v=sha256:1c9fa83..."#}</code></pre>
                            </div>

                            <p>
                                "A pinned response is immutable and cacheable for a year. Use it for
                                anything that quotes a specific revision: a signed document, a
                                datasheet, a figure a reader may come back to."
                            </p>
                        </div>

                        <div id="caching-failure" class="subsection">
                            <h3>"The Failure Table"</h3>

                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Condition"</th><th>"Behaviour"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Network unreachable"</td>
                                        <td>"Keep the baked values, no visible change"</td>
                                    </tr>
                                    <tr>
                                        <td>"Response not ok (401 from a rotated key, 429, 5xx)"</td>
                                        <td>"Keep the baked values, log"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"schema_version"</code>" mismatch"</td>
                                        <td>"Keep the baked values, log, apply nothing at all"</td>
                                    </tr>
                                    <tr>
                                        <td>"Key missing from the manifest"</td>
                                        <td>"Leave that element alone, log the key"</td>
                                    </tr>
                                    <tr>
                                        <td>"Manifest present, value is "<code>"null"</code></td>
                                        <td>"Should be impossible: a failed reference fails the publish"</td>
                                    </tr>
                                </tbody>
                            </table>

                            <p>
                                "One rule covers every row: "<strong>"the page is already correct before
                                the fetch, and the fetch can only improve it."</strong>" Anything else
                                turns a network problem into a credibility problem."
                            </p>
                        </div>

                        <div id="caching-build" class="subsection">
                            <h3>"Build-Time Baking"</h3>
                            <p>
                                "The same manifest feeds the build. That is what keeps the fallbacks
                                honest."
                            </p>

                            <div class="code-block large">
                                <div class="code-header">
                                    <span class="code-lang">"Pre-deploy step"</span>
                                </div>
                                <pre><code class="language-text">{r#"fetch manifest  ->  write values into the HTML  ->  deploy"#}</code></pre>
                            </div>

                            <p>
                                "Run it as a pre-deploy step, and fail the build when a
                                "<code>"data-eus"</code>" key has no manifest entry. That catches a
                                renamed reference at build time instead of on a visitor's screen, and it
                                means the committed HTML always shows the state of the last publish."
                            </p>
                            <p>
                                "With build-time baking in place, runtime hydration becomes a correction
                                for publishes that happened after the last deploy rather than the
                                primary path. Both are worth having: the build keeps the page correct,
                                the fetch keeps it current."
                            </p>
                        </div>
                    </section>

                    // 8. WHAT THIS DOES NOT DO
                    <section id="limits" class="docs-section">
                        <h2 class="section-anchor">"What This Does Not Do"</h2>

                        <div id="limits-scope" class="subsection">
                            <h3>"Four Honest Limits"</h3>

                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"01"</div>
                                    <h4>"It Does Not Push"</h4>
                                    <p>
                                        "A website learns about a change on its next fetch. With a
                                        five-minute "<code>"max-age"</code>" that is the update latency.
                                        Raising or lowering it is a consumer decision."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"02"</div>
                                    <h4>"It Does Not Version Values"</h4>
                                    <p>
                                        "The manifest is the current state. History lives in the Space's
                                        git and in the run records. A consumer that wants a time series
                                        should read telemetry instead."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"03"</div>
                                    <h4>"It Cannot Reach a Claude Artifact"</h4>
                                    <p>
                                        "Published Claude artifacts run under a strict content security
                                        policy that blocks external hosts, so an artifact cannot fetch a
                                        manifest at all. Regenerate the document on publish and stamp
                                        "<code>"publish_hash"</code>" and "<code>"baked_at"</code>" in
                                        the footer, so a reader can tell which state it describes. A
                                        paper that quietly fails to update is worse than a paper that
                                        says which day it was true."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"04"</div>
                                    <h4>"It Does Not Make Values Private"</h4>
                                    <p>
                                        "The key identifies a caller and can be revoked. It leaves the
                                        manifest readable to anyone holding a copy of the key. Values
                                        you would keep off a public page belong somewhere other than a
                                        Reference."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="limits-checklist" class="subsection">
                            <h3>"Checklist for a New Consumer"</h3>
                            <ol>
                                <li>"Pick the namespace and pin "<code>"schema_version"</code>"."</li>
                                <li>"Copy the manifest key from the Website service Properties panel."</li>
                                <li>"Mark values with "<code>"data-eus"</code>", carrying the current
                                correct value as the element's text."</li>
                                <li>"Add the hydration script once, at the end of the document."</li>
                                <li>"Add the build-time bake, and make a missing key fail the build."</li>
                                <li>"Load the page with the network disabled and read every number."</li>
                            </ol>

                            <div class="callout callout-tip">
                                <img src="/assets/icons/check.svg" alt="Tip" />
                                <div>
                                    <strong>"Step 6 is the one that matters."</strong>
                                    <p>"If the page is right with the network off, the manifest is an
                                    improvement. If it is wrong with the network off, the manifest is a
                                    dependency, and you have moved your credibility onto someone else's
                                    uptime."</p>
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
                        <a href="/docs/services" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Services"</span>
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
