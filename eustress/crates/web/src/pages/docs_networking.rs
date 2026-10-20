// =============================================================================
// Eustress Web - Networking Documentation Page
// =============================================================================
// Networking: every Space runs in one process today. The ports Studio opens,
// the ownership and replication design already in code, and the multiplayer plan.
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
                TocSubsection { id: "overview-today", title: "Where Networking Stands" },
                TocSubsection { id: "overview-halves", title: "Two Halves in the Code" },
            ],
        },
        TocSection {
            id: "local",
            title: "Local Play",
            subsections: vec![
                TocSubsection { id: "local-play", title: "Play Runs In-Process" },
                TocSubsection { id: "local-controls", title: "Server Controls" },
            ],
        },
        TocSection {
            id: "connections",
            title: "Connections",
            subsections: vec![
                TocSubsection { id: "connections-ports", title: "Ports Studio Opens" },
                TocSubsection { id: "connections-streams", title: "Event Streams" },
                TocSubsection { id: "connections-api", title: "Calls to eustress.dev" },
            ],
        },
        TocSection {
            id: "authority",
            title: "Authority",
            subsections: vec![
                TocSubsection { id: "authority-owners", title: "Owners" },
                TocSubsection { id: "authority-requests", title: "Ownership Requests" },
                TocSubsection { id: "authority-validation", title: "Validation" },
            ],
        },
        TocSection {
            id: "replication",
            title: "Replication",
            subsections: vec![
                TocSubsection { id: "replication-what", title: "What Replicates" },
                TocSubsection { id: "replication-messages", title: "The Message Protocol" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-transport", title: "Transport: lightyear" },
                TocSubsection { id: "roadmap-phases", title: "The Phases" },
                TocSubsection { id: "roadmap-first", title: "The First Shared Session" },
            ],
        },
    ]
}

/// The two networking modules side by side: one has the rules, the other has
/// the socket, and the planned library is the bridge between them.
#[component]
fn TwoHalvesDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 230" role="img"
                aria-label="The shared networking crate holds ownership, interest and delta rules but has no socket. The engine play server holds a QUIC endpoint and a message protocol but is never started. A dashed link marks the planned lightyear transport that will join them.">
                <defs>
                    <marker id="halves-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                // Left: the rules
                <text x="20" y="28" class="dg-title">"Shared networking crate"</text>
                <rect x="20" y="44" width="240" height="120" rx="8" class="dg-box"></rect>
                <text x="140" y="76" class="dg-label" text-anchor="middle">"Ownership rules"</text>
                <text x="140" y="100" class="dg-label" text-anchor="middle">"Interest and deltas"</text>
                <text x="140" y="124" class="dg-label" text-anchor="middle">"Physics validation"</text>
                <text x="140" y="150" class="dg-note" text-anchor="middle">"no socket"</text>

                // Right: the socket
                <text x="380" y="28" class="dg-title">"Engine play server"</text>
                <rect x="380" y="44" width="240" height="120" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="500" y="76" class="dg-label" text-anchor="middle">"QUIC endpoint, TLS"</text>
                <text x="500" y="100" class="dg-label" text-anchor="middle">"Message protocol"</text>
                <text x="500" y="124" class="dg-label" text-anchor="middle">"Sessions"</text>
                <text x="500" y="150" class="dg-note" text-anchor="middle">"never started"</text>

                // The planned bridge
                <line x1="262" y1="104" x2="378" y2="104" class="dg-line-dashed"
                    marker-start="url(#halves-arrow)" marker-end="url(#halves-arrow)"></line>
                <text x="320" y="94" class="dg-note" text-anchor="middle">"planned:"</text>
                <text x="320" y="124" class="dg-note" text-anchor="middle">"lightyear 0.29"</text>

                <text x="320" y="206" class="dg-note" text-anchor="middle">"Today: Play in Studio and the Player each run one process for one person"</text>
            </svg>
            <figcaption>
                "Each half has what the other lacks. The plan keeps the ownership rules and the
                message families, and replaces both transports with one library."
            </figcaption>
        </figure>
    }
}

/// How the server answers an ownership request: checks in order, then a grant
/// or a denial, plus the idle release back to the server.
#[component]
fn OwnershipDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 250" role="img"
                aria-label="A client sends an ownership request. The server checks the lock, distance, cooldown and competing requests. If every check passes, ownership transfers and the new owner starts predicting. Otherwise the request is denied with a reason. An idle client-owned entity returns to the server after 30 seconds.">
                <defs>
                    <marker id="own-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                <rect x="20" y="92" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="85" y="117" class="dg-label" text-anchor="middle">"Client"</text>
                <text x="85" y="135" class="dg-note" text-anchor="middle">"requests entity"</text>

                <line x1="150" y1="120" x2="208" y2="120" class="dg-line" marker-end="url(#own-arrow)"></line>

                <rect x="210" y="72" width="200" height="96" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="310" y="98" class="dg-label" text-anchor="middle">"Server checks"</text>
                <text x="310" y="120" class="dg-note" text-anchor="middle">"lock, distance,"</text>
                <text x="310" y="138" class="dg-note" text-anchor="middle">"cooldown, other requests"</text>

                <line x1="410" y1="100" x2="468" y2="64" class="dg-line" marker-end="url(#own-arrow)"></line>
                <line x1="410" y1="140" x2="468" y2="176" class="dg-line" marker-end="url(#own-arrow)"></line>

                <rect x="470" y="30" width="150" height="56" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="545" y="55" class="dg-label" text-anchor="middle">"Granted"</text>
                <text x="545" y="73" class="dg-note" text-anchor="middle">"new owner predicts"</text>

                <rect x="470" y="154" width="150" height="56" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="545" y="179" class="dg-label" text-anchor="middle">"Denied"</text>
                <text x="545" y="197" class="dg-note" text-anchor="middle">"reason sent back"</text>

                <text x="320" y="238" class="dg-note" text-anchor="middle">"Idle 30 s: ownership returns to the server, blended over 1.5 s"</text>
            </svg>
            <figcaption>
                "Every request runs the same checks in the same order, so the server alone decides
                who owns what."
            </figcaption>
        </figure>
    }
}

/// Networking documentation page.
#[component]
pub fn DocsNetworkingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-networking"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/network.svg" alt="Networking" class="toc-icon" />
                        <h2>"Networking"</h2>
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
                            <span class="current">"Networking"</span>
                        </div>
                        <h1 class="docs-title">"Networking"</h1>
                        <p class="docs-subtitle">
                            "Networking is the layer that lets several machines share one running Space: a
                            server owns the simulation and players connect to it. Today every Space runs in a
                            single process, so this page covers the connections Eustress Engine does make, the
                            authority rules already written in code, and the plan for multiplayer."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "12 min read"
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

                        <div id="overview-today" class="subsection">
                            <h3>"Where Networking Stands"</h3>
                            <p>
                                "A shared session needs three things: a transport that carries messages between
                                processes, rules for who owns each moving thing, and a description of what to
                                send. Eustress has the rules and most of the description in code. The transport
                                is the missing piece, so Play in Studio and the standalone Player each run one
                                simulation in one process, for one person."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Piece"</th><th>"State"</th><th>"What that means today"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Play in Studio"</td><td>"Works"</td><td>"Physics, scripts and your character run inside the editor."</td></tr>
                                    <tr><td>"Ownership rules"</td><td>"Written"</td><td>"Server-owned by default, one client owner at a time, arbitration on request."</td></tr>
                                    <tr><td>"Interest and delta tracking"</td><td>"Written"</td><td>"Per-player area of interest and change thresholds."</td></tr>
                                    <tr><td>"QUIC endpoint and messages"</td><td>"Written"</td><td>"Nothing in Studio starts it."</td></tr>
                                    <tr><td>"Headless server"</td><td>"Partial"</td><td>"Downloads and unpacks a published Universe, but does not load it or accept players."</td></tr>
                                    <tr><td>"Transport between processes"</td><td>"Planned"</td><td>"lightyear 0.29, described in What's Next."</td></tr>
                                    <tr><td>"Joining from the website"</td><td>"Planned"</td><td>"The play page reports that no server is available."</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Written is not running"</strong>
                                    <p>
                                        "Rows marked Written are real code with nothing to carry its messages
                                        between machines. This page describes them as the model a networked Space
                                        will use, not as something you can switch on."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-halves" class="subsection">
                            <h3>"Two Halves in the Code"</h3>
                            <p>
                                "The networking code sits in two modules that were written separately and never
                                joined. Each has what the other lacks."
                            </p>
                            <TwoHalvesDiagram />
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Module"</th><th>"Has"</th><th>"Lacks"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Shared networking crate"</td>
                                        <td>"Ownership arbitration, per-player interest, delta tracking, physics validation, prediction and interpolation markers"</td>
                                        <td>"A socket. Its start handler marks the server as running without opening a port."</td>
                                    </tr>
                                    <tr>
                                        <td>"Engine play server"</td>
                                        <td>"A QUIC endpoint with TLS, a typed message protocol with delivery channels, player sessions"</td>
                                        <td>"A caller for its accept loop, a join handshake, and anything that marks entities to send."</td>
                                    </tr>
                                </tbody>
                            </table>
                            <p>
                                "In a normal build neither module marks a single entity for replication, so a
                                working transport would have nothing to send yet. The plan in "
                                <a href="#roadmap">"What's Next"</a>" keeps the ownership rules and the message
                                families and replaces the rest with one library."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // LOCAL PLAY
                    // =========================================================
                    <section id="local" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Local Play"
                        </h2>

                        <div id="local-play" class="subsection">
                            <h3>"Play Runs In-Process"</h3>
                            <p>
                                "Play simulates the open Space inside the editor process. Physics, scripts and
                                the character tick locally, and Stop returns the Space to the state it had before
                                you pressed Play. Each mode is covered on the "<a href="/docs/studio">"Studio"</a>
                                " page."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Action"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"F5"</code></td><td>"Play, with your character"</td></tr>
                                    <tr><td><code>"F7"</code></td><td>"Run, with no character"</td></tr>
                                    <tr><td><code>"F6"</code></td><td>"Pause or resume"</td></tr>
                                    <tr><td><code>"F8"</code></td><td>"Stop"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The Roblox keymap preset moves Run to "<code>"F8"</code>" and Stop to "
                                <code>"Shift+F5"</code>". The standalone Player works the same way for one person:
                                it opens one Space from disk when it starts and runs it locally."
                            </p>
                        </div>

                        <div id="local-controls" class="subsection">
                            <h3>"Server Controls"</h3>
                            <p>
                                "The Network menu and the Test tab already carry the controls a hosted session
                                will need: Start Local Server, Stop Server, Join as Client, synthetic clients and a
                                stress test. They arrive ahead of the transport they depend on."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Start Local Server does not start a server yet"</strong>
                                    <p>
                                        "Start Local Server ("<code>"F9"</code>"), Stop Server, the Test tab's
                                        Server, Client, Local, Spawn and Disconnect buttons, the Network Panel item
                                        and the stress test have no working server behind them. Studio's play code
                                        reserves a hosting mode and a joining mode, but no menu, key or command
                                        selects either one, so these controls change nothing today."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // CONNECTIONS
                    // =========================================================
                    <section id="connections" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Connections"
                        </h2>

                        <div id="connections-ports" class="subsection">
                            <h3>"Ports Studio Opens"</h3>
                            <p>
                                "A Studio session listens on the network in three places, and a fourth listener
                                exists only in a special build. Each one serves a single job on your machine;
                                none of them carries a multiplayer session."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Listener"</th><th>"Address"</th><th>"When"</th><th>"Used by"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Engine Bridge"</td>
                                        <td>"TCP on 127.0.0.1, a port the OS assigns, written to "<code>".eustress/engine.port"</code>" in the Universe"</td>
                                        <td>"Every session"</td>
                                        <td>"The "<a href="/learn/mcp">"MCP server"</a>" and other tools on this machine"</td>
                                    </tr>
                                    <tr>
                                        <td>"Rune LSP"</td>
                                        <td>"TCP on 127.0.0.1, a port the OS assigns, written to "<code>".eustress/lsp.port"</code></td>
                                        <td>"Every session where the language server ships beside Studio"</td>
                                        <td>"Your editor, through "<a href="/learn/lsp">"Rune LSP"</a></td>
                                    </tr>
                                    <tr>
                                        <td>"Bliss node"</td>
                                        <td>"TCP 7777 on every network interface"</td>
                                        <td>"At launch, while Bliss is enabled (the default)"</td>
                                        <td>"Bliss co-signing and identity checks, see "<a href="/docs/earning">"Earning"</a></td>
                                    </tr>
                                    <tr>
                                        <td>"Stream node"</td>
                                        <td>"TCP 33000 and HTTP 43000 on 127.0.0.1"</td>
                                        <td>"Only in builds with the "<code>"stream-node"</code>" feature"</td>
                                        <td>"Tools that follow event streams"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"The Bliss node listens on every interface"</strong>
                                    <p>
                                        "It binds 0.0.0.0, so other machines on your network can reach port
                                        7777. Studio reads the setting only at startup: set "
                                        <code>"bliss_enabled"</code>" to "<code>"false"</code>" in "
                                        <code>"~/.eustress_engine/settings.json"</code>" and restart to keep the
                                        node off."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="connections-streams" class="subsection">
                            <h3>"Event Streams"</h3>
                            <p>
                                "Inside the engine, changes flow through EustressStream, an append-only log of
                                named topics held in memory. "<code>"scene_deltas"</code>" carries scene edits, "
                                <code>"sim_results"</code>" carries simulation outcomes and "
                                <code>"log/output"</code>" mirrors the Output panel. In the default build these
                                topics stay inside the process."
                            </p>
                            <p>
                                "A build with the "<code>"stream-node"</code>" feature exposes the same topics to
                                other programs on the machine over TCP, with an HTTP interface beside it. The node
                                binds 127.0.0.1 and authenticates nobody, which is why it stays out of the default
                                build."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Bash (stream-node builds)"</span>
                                </div>
                                <pre><code class="language-bash">{r#"# List topics with their stats
curl http://127.0.0.1:43000/topics

# Follow scene edits live as server-sent events
curl -N http://127.0.0.1:43000/topics/scene_deltas/stream

# Replay a topic's ring buffer from offset 0
curl -N "http://127.0.0.1:43000/topics/scene_deltas/replay?from=0""#}</code></pre>
                            </div>
                        </div>

                        <div id="connections-api" class="subsection">
                            <h3>"Calls to eustress.dev"</h3>
                            <p>
                                "Studio also makes HTTPS requests to the Eustress API at api.eustress.dev. Signing
                                in fetches a challenge from the API, which Studio signs with the Ed25519 key in your
                                identity file, and publishing uploads your Universe. These are requests from your
                                machine to a web service, not connections between players. The upload path is
                                described on the "<a href="/docs/publishing">"Publishing"</a>" page."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // AUTHORITY
                    // =========================================================
                    <section id="authority" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Authority"
                        </h2>

                        <div id="authority-owners" class="subsection">
                            <h3>"Owners"</h3>
                            <p>
                                "Authority decides whose word counts for an entity. The rules are written in the
                                shared networking crate and wait for a transport to carry them. Every networked
                                entity has exactly one owner, recorded as a client id in which 0 means the server."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Owner"</th><th>"Who simulates it"</th><th>"Everyone else"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Server (the default)"</td><td>"The server"</td><td>"Clients interpolate between the states they receive"</td></tr>
                                    <tr><td>"One client"</td><td>"That client, predicting ahead"</td><td>"The server validates it; other clients interpolate"</td></tr>
                                    <tr><td>"Locked"</td><td>"The server"</td><td>"Every ownership request is refused"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Client ownership keeps the controls responsive for the player driving something
                                while the server keeps the final say. The first entity planned for it is each
                                player's own avatar, as described in "<a href="#roadmap-first">"The First Shared Session"</a>"."
                            </p>
                        </div>

                        <div id="authority-requests" class="subsection">
                            <h3>"Ownership Requests"</h3>
                            <p>"A client asks for an entity, and the server runs the same checks in order every time:"</p>
                            <OwnershipDiagram />
                            <ol class="numbered-list">
                                <li>"A locked entity is refused."</li>
                                <li>"A request from the current owner changes nothing."</li>
                                <li>"A client too far from the entity is refused, and the reason gives the distance."</li>
                                <li>"An entity that changed owner in the last 100 ms is refused while it cools down."</li>
                                <li>"If another client's request is still pending, the lower ping wins. A client with no ping on record counts as 100 ms. Pending requests expire after 500 ms."</li>
                                <li>"Otherwise ownership transfers: the old owner stops simulating the entity and the new owner starts predicting it."</li>
                            </ol>
                            <p>
                                "A client-owned entity with no activity for 30 seconds returns to the server on its
                                own. The hand-off is blended over 1.5 seconds so physics authority moves gradually
                                rather than in one step."
                            </p>
                        </div>

                        <div id="authority-validation" class="subsection">
                            <h3>"Validation"</h3>
                            <p>
                                "The server checks what client owners report. It compares the speed of every
                                client-owned entity with a ceiling and counts a violation for each breach.
                                Violations decay over time, one per second by default, and the server raises a
                                disconnect for any client that reaches 10."
                            </p>
                            <p>
                                "Speed is the only check the server runs. An acceleration check exists in an
                                optional physics module that normal builds leave out, and the teleport-distance
                                and input-queue limits in the same configuration are not read by any system yet."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // REPLICATION
                    // =========================================================
                    <section id="replication" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "Replication"
                        </h2>

                        <div id="replication-what" class="subsection">
                            <h3>"What Replicates"</h3>
                            <p>
                                "Replication is the server sending each client the state it needs to show the
                                shared world. In the design, an entity takes part when it carries a replication
                                marker. The server then gives it a network id and works out, for each client, what
                                changed:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Motion"</strong>": position, rotation and velocity, sent only when the change passes a small threshold."</li>
                                <li><strong>"Data"</strong>": attributes, tags, parameters, documents, and image and video assets, mirrored into network copies whenever they change."</li>
                                <li><strong>"Interest"</strong>": each client receives only the entities inside its area of interest, found on a spatial grid. An entity joins inside the radius and leaves only past a wider band, so objects at the edge do not flicker in and out."</li>
                                <li><strong>"Ownership"</strong>": a client never receives updates for an entity it owns, because its own simulation is the source."</li>
                            </ul>
                            <p>
                                "Nothing attaches the replication marker in a normal build yet, which is why the
                                plan starts with a single, deliberate choice of what to send."
                            </p>
                        </div>

                        <div id="replication-messages" class="subsection">
                            <h3>"The Message Protocol"</h3>
                            <p>
                                "The engine's play server defines the messages a session will exchange, each on a
                                delivery channel that suits it. Messages are serialized with bincode and travel
                                over QUIC with TLS, using a certificate the server generates for localhost when it
                                starts."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Family"</th><th>"Messages"</th><th>"Channel"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Connection"</td><td><code>"Join"</code>", "<code>"JoinAccepted"</code>", "<code>"JoinRejected"</code>", "<code>"Disconnect"</code></td><td>"Reliable, ordered"</td></tr>
                                    <tr><td>"Heartbeat"</td><td><code>"Ping"</code>", "<code>"Pong"</code>", "<code>"AckTick"</code></td><td>"Unreliable"</td></tr>
                                    <tr><td>"Players"</td><td><code>"PlayerSpawned"</code>", "<code>"PlayerDespawned"</code></td><td>"Reliable, ordered"</td></tr>
                                    <tr><td>"Input"</td><td><code>"PlayerInput"</code></td><td>"Unreliable, latest wins"</td></tr>
                                    <tr><td>"Chat"</td><td><code>"ChatMessage"</code>", "<code>"ChatBroadcast"</code></td><td>"Reliable, ordered"</td></tr>
                                    <tr><td>"World"</td><td><code>"WorldSnapshot"</code>" (reliable); "<code>"Replication"</code>", "<code>"WorldDelta"</code></td><td>"Unreliable, latest wins"</td></tr>
                                    <tr><td>"Physics"</td><td><code>"PhysicsAuthority"</code>", "<code>"PhysicsCorrection"</code></td><td>"Reliable, ordered"</td></tr>
                                    <tr><td>"Scripts"</td><td><code>"RemoteEvent"</code>", "<code>"RemoteFunction"</code>", "<code>"RemoteFunctionReturn"</code></td><td>"Reliable, ordered"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The script family is the part your code will touch: a remote event is a name with
                                serialized arguments, and a remote function call carries an id that its return
                                value echoes, so the caller can match the answer to the question."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-transport" class="subsection">
                            <h3>"Transport: lightyear"</h3>
                            <p>
                                "Multiplayer will adopt lightyear 0.29, pinned exactly, behind a "
                                <code>"multiplayer"</code>" build feature that is off by default. Three facts
                                decided it. lightyear 0.29 targets Bevy 0.19, the version Eustress runs on. Its
                                Avian integration depends on avian3d 0.7, the version Eustress already uses, which
                                avoids two copies of the physics engine fighting over replicated bodies. And its
                                host-server mode runs client and server in one app, which is exactly the shape of
                                Play in Studio."
                            </p>
                            <p>
                                "The ownership rules will stay as they are, the message families will move onto
                                lightyear channels, and the separate replication markers in the two modules will
                                collapse into one. If the transport spike fails, the fallback is bevy_replicon 0.42
                                with bevy_replicon_renet 0.18."
                            </p>
                        </div>

                        <div id="roadmap-phases" class="subsection">
                            <h3>"The Phases"</h3>
                            <p>"Each phase ends with a test that has to pass before the next one starts:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Phase"</th><th>"Work"</th><th>"Done when"</th></tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"0. Safety"</td>
                                        <td>"Harden release builds, bind the Bliss node to loopback, commit the lockfile, and build the server and Player in CI"</td>
                                        <td>"CI fails on a broken server or Player build"</td>
                                    </tr>
                                    <tr>
                                        <td>"1. Storage"</td>
                                        <td>"Regenerate files from the WorldDb at publish, ship an allowlisted package, keep the listing id"</td>
                                        <td>"Republishing an unchanged Space produces byte-identical packages"</td>
                                    </tr>
                                    <tr>
                                        <td>"2. Server opens a world"</td>
                                        <td>"Give the headless server the storage it needs to load the Universe it downloads"</td>
                                        <td>"It reports the same entity count Studio shows for that Space"</td>
                                    </tr>
                                    <tr>
                                        <td>"3. Transport spike"</td>
                                        <td>"Two apps in one process, in a separate workspace, with one avatar replicated under prediction and rollback"</td>
                                        <td>"It works with no engine code involved, or the plan switches to the fallback"</td>
                                    </tr>
                                    <tr>
                                        <td>"4. Studio dev server"</td>
                                        <td>"Start Local Server hosts on 127.0.0.1, with LAN behind an explicit toggle and a token"</td>
                                        <td>"Play in Studio, connect the Player, and two avatars move in both windows"</td>
                                    </tr>
                                    <tr>
                                        <td>"5. Join from the website"</td>
                                        <td>"Play links that open the Player, servers that register their address, single-use join tokens"</td>
                                        <td>"A second machine joins from the website, and a forged or expired token is refused"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="roadmap-first" class="subsection">
                            <h3>"The First Shared Session"</h3>
                            <p>
                                "The first multiplayer release will replicate one thing: each player's avatar. The
                                shared avatar runtime already has a local-player mode and a remote mode, so the
                                local avatar will be marked for replication and every other machine will spawn it
                                as remote. Anchored geometry will not travel over the network at all, because every
                                machine loads the same published package."
                            </p>
                            <p>
                                "Team Create, matchmaking, voice chat, always-on servers and mobile players sit
                                outside that first release."
                            </p>
                            <div class="future-cta">
                                <p><strong>"One avatar across two machines is the first milestone. Everything else builds on it."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/publishing" class="btn-secondary-steel">"Publishing Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/universes" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Universes"</span>
                            </div>
                        </a>
                        <a href="/docs/publishing" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Publishing"</span>
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
