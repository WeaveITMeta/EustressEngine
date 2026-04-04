// =============================================================================
// Eustress Web - Networking Documentation Page (Industrial Design)
// =============================================================================
// Comprehensive networking documentation with floating TOC
// Covers: QUIC transport, replication, matchmaking, remote events,
// server authority, lag compensation, and API reference.
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
                TocSubsection { id: "overview-security", title: "Security Model" },
            ],
        },
        TocSection {
            id: "transport",
            title: "QUIC Transport",
            subsections: vec![
                TocSubsection { id: "transport-why", title: "Why QUIC" },
                TocSubsection { id: "transport-connections", title: "Connection Lifecycle" },
                TocSubsection { id: "transport-channels", title: "Channels" },
                TocSubsection { id: "transport-tls", title: "TLS Encryption" },
            ],
        },
        TocSection {
            id: "replication",
            title: "State Replication",
            subsections: vec![
                TocSubsection { id: "replication-components", title: "Replicated Components" },
                TocSubsection { id: "replication-ownership", title: "Ownership" },
                TocSubsection { id: "replication-interest", title: "Interest Management" },
                TocSubsection { id: "replication-delta", title: "Delta Compression" },
            ],
        },
        TocSection {
            id: "authority",
            title: "Server Authority",
            subsections: vec![
                TocSubsection { id: "authority-model", title: "Authority Model" },
                TocSubsection { id: "authority-validation", title: "Input Validation" },
                TocSubsection { id: "authority-rollback", title: "Rollback and Prediction" },
            ],
        },
        TocSection {
            id: "events",
            title: "Remote Events",
            subsections: vec![
                TocSubsection { id: "events-client-server", title: "Client to Server" },
                TocSubsection { id: "events-server-client", title: "Server to Client" },
                TocSubsection { id: "events-broadcast", title: "Broadcast" },
                TocSubsection { id: "events-reliability", title: "Reliability Modes" },
            ],
        },
        TocSection {
            id: "matchmaking",
            title: "Matchmaking",
            subsections: vec![
                TocSubsection { id: "matchmaking-teleport", title: "TeleportService" },
                TocSubsection { id: "matchmaking-reserved", title: "Reserved Servers" },
                TocSubsection { id: "matchmaking-regions", title: "Region Selection" },
            ],
        },
        TocSection {
            id: "optimization",
            title: "Optimization",
            subsections: vec![
                TocSubsection { id: "optimization-bandwidth", title: "Bandwidth Management" },
                TocSubsection { id: "optimization-interpolation", title: "Interpolation" },
                TocSubsection { id: "optimization-lag", title: "Lag Compensation" },
            ],
        },
        TocSection {
            id: "api",
            title: "API Reference",
            subsections: vec![
                TocSubsection { id: "api-components", title: "Components" },
                TocSubsection { id: "api-resources", title: "Resources" },
                TocSubsection { id: "api-events", title: "Events" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Networking documentation page with floating TOC.
#[component]
pub fn DocsNetworkingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-networking"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
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

                // Main Content
                <main class="docs-content">
                    // Hero
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Networking"</span>
                        </div>
                        <h1 class="docs-title">"Networking System"</h1>
                        <p class="docs-subtitle">
                            "Build real-time multiplayer experiences with QUIC transport, automatic state replication, 
                            server-authoritative physics, and built-in matchmaking. Encrypted by default."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "35 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate"
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
                                "Eustress networking is built on QUIC (RFC 9000) — the same protocol powering HTTP/3. 
                                It provides multiplexed, encrypted, low-latency connections out of the box. Unlike 
                                traditional game networking stacks that bolt TCP and UDP together, QUIC gives you 
                                the reliability of TCP with the speed of UDP in a single protocol."
                            </p>
                            <div class="docs-callout info">
                                <strong>"Key Advantage:"</strong>
                                " Every connection is TLS 1.3 encrypted by default. No unencrypted game traffic, 
                                no man-in-the-middle attacks, no packet sniffing. Security is not optional."
                            </div>
                        </div>

                        <div id="overview-architecture" class="docs-block">
                            <h3>"Architecture"</h3>
                            <p>"Eustress uses a client-server architecture with dedicated servers:"</p>
                            <div class="architecture-diagram">
                                <div class="arch-node client">"Client A"</div>
                                <div class="arch-arrow">"QUIC"</div>
                                <div class="arch-node server">"Dedicated Server"</div>
                                <div class="arch-arrow">"QUIC"</div>
                                <div class="arch-node client">"Client B"</div>
                            </div>
                            <ul class="docs-list">
                                <li><strong>"Dedicated Servers"</strong>" — Run at 120 tick rate with server-authoritative physics"</li>
                                <li><strong>"Clients"</strong>" — Send inputs, receive state updates, run prediction locally"</li>
                                <li><strong>"No peer-to-peer"</strong>" — All game state flows through the server for security"</li>
                            </ul>
                        </div>

                        <div id="overview-security" class="docs-block">
                            <h3>"Security Model"</h3>
                            <p>"The server is the single source of truth. Clients send inputs, never state:"</p>
                            <ul class="docs-list">
                                <li>"All player inputs are validated server-side before being applied"</li>
                                <li>"Physics simulation runs authoritatively on the server"</li>
                                <li>"Clients predict locally for responsiveness, server corrects on mismatch"</li>
                                <li>"Rate limiting prevents input flooding and denial-of-service"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // QUIC Transport
                    // ─────────────────────────────────────────────────────
                    <section id="transport" class="docs-section">
                        <h2 class="section-anchor">"QUIC Transport"</h2>

                        <div id="transport-why" class="docs-block">
                            <h3>"Why QUIC"</h3>
                            <div class="comparison-cards">
                                <div class="lang-card">
                                    <h4>"TCP"</h4>
                                    <p>"Reliable but slow. Head-of-line blocking means one lost packet stalls everything."</p>
                                </div>
                                <div class="lang-card">
                                    <h4>"UDP"</h4>
                                    <p>"Fast but unreliable. You rebuild reliability yourself, poorly."</p>
                                </div>
                                <div class="lang-card soul">
                                    <h4>"QUIC"</h4>
                                    <p>"Best of both. Independent streams, built-in encryption, 0-RTT reconnection, no head-of-line blocking."</p>
                                </div>
                            </div>
                        </div>

                        <div id="transport-connections" class="docs-block">
                            <h3>"Connection Lifecycle"</h3>
                            <ol class="docs-list numbered">
                                <li><strong>"Handshake"</strong>" — TLS 1.3 handshake (1-RTT, 0-RTT on reconnect)"</li>
                                <li><strong>"Authentication"</strong>" — Client sends auth token, server validates"</li>
                                <li><strong>"Replication"</strong>" — Server sends initial world state snapshot"</li>
                                <li><strong>"Steady State"</strong>" — Client sends inputs, server sends delta updates"</li>
                                <li><strong>"Disconnection"</strong>" — Graceful close or timeout after 30 seconds"</li>
                            </ol>
                        </div>

                        <div id="transport-channels" class="docs-block">
                            <h3>"Channels"</h3>
                            <p>"QUIC streams are used as logical channels for different data types:"</p>
                            <pre class="code-block"><code>{"// Channel configuration
NetworkChannels {
    // Reliable ordered — for important state changes
    reliable: Channel::ReliableOrdered,
    
    // Reliable unordered — for events that must arrive but order doesn't matter
    events: Channel::ReliableUnordered,
    
    // Unreliable — for frequent updates where latest-wins (position, rotation)
    state: Channel::Unreliable,
}"}</code></pre>
                        </div>

                        <div id="transport-tls" class="docs-block">
                            <h3>"TLS Encryption"</h3>
                            <p>
                                "All QUIC connections use TLS 1.3 with certificate pinning. The server's certificate 
                                is validated against known certificates distributed with the game client. This prevents 
                                impersonation and man-in-the-middle attacks even on compromised networks."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // State Replication
                    // ─────────────────────────────────────────────────────
                    <section id="replication" class="docs-section">
                        <h2 class="section-anchor">"State Replication"</h2>

                        <div id="replication-components" class="docs-block">
                            <h3>"Replicated Components"</h3>
                            <p>
                                "Mark components for replication by adding the "<code>"Replicated"</code>" component. 
                                The networking system automatically synchronizes these across all connected clients."
                            </p>
                            <pre class="code-block"><code>{"// Mark an entity for replication
commands.spawn((
    Transform::from_xyz(0.0, 1.0, 0.0),
    RigidBody::Dynamic,
    Player { health: 100.0 },
    Replicated,  // This entity's state is synchronized to all clients
    Name::new(\"Player\"),
));"}</code></pre>
                        </div>

                        <div id="replication-ownership" class="docs-block">
                            <h3>"Ownership"</h3>
                            <p>
                                "Each replicated entity has an owner — typically the server or a specific client. 
                                Only the owner can modify the entity's authoritative state. Other clients receive 
                                read-only replicated views."
                            </p>
                            <pre class="code-block"><code>{"// Assign ownership to a specific client
commands.entity(player_entity).insert(
    NetworkOwner(client_id)
);"}</code></pre>
                        </div>

                        <div id="replication-interest" class="docs-block">
                            <h3>"Interest Management"</h3>
                            <p>
                                "Not every client needs every entity. Interest management filters which entities 
                                are replicated to each client based on distance, visibility, or custom logic. 
                                This dramatically reduces bandwidth for large worlds."
                            </p>
                            <pre class="code-block"><code>{"// Only replicate entities within 200 meters of the player
app.add_plugins(InterestManagementPlugin {
    radius: 200.0,
    update_interval_ms: 100,
});"}</code></pre>
                        </div>

                        <div id="replication-delta" class="docs-block">
                            <h3>"Delta Compression"</h3>
                            <p>
                                "Only changed component fields are sent over the network. If a Transform's rotation 
                                changes but position stays the same, only the rotation bytes are transmitted. This 
                                reduces bandwidth by 60-80% compared to full-state replication."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Server Authority
                    // ─────────────────────────────────────────────────────
                    <section id="authority" class="docs-section">
                        <h2 class="section-anchor">"Server Authority"</h2>

                        <div id="authority-model" class="docs-block">
                            <h3>"Authority Model"</h3>
                            <p>
                                "The server runs the canonical simulation. Clients send player inputs (keys pressed, 
                                mouse movement) and the server applies them to the simulation. The server then 
                                replicates the resulting state back to all clients."
                            </p>
                        </div>

                        <div id="authority-validation" class="docs-block">
                            <h3>"Input Validation"</h3>
                            <p>"Every client input is validated before application:"</p>
                            <ul class="docs-list">
                                <li><strong>"Rate limiting"</strong>" — Maximum input frequency enforced (120 Hz)"</li>
                                <li><strong>"Range checks"</strong>" — Movement speed cannot exceed configured maximums"</li>
                                <li><strong>"State checks"</strong>" — Actions validated against current game state (cannot attack while dead)"</li>
                                <li><strong>"Timestamp validation"</strong>" — Inputs with impossible timestamps are rejected"</li>
                            </ul>
                        </div>

                        <div id="authority-rollback" class="docs-block">
                            <h3>"Rollback and Prediction"</h3>
                            <p>
                                "Clients predict movement locally for instant responsiveness. When the server's 
                                authoritative state arrives, the client compares predictions. If they match, no 
                                correction needed. If they diverge, the client smoothly interpolates to the 
                                server state over a few frames to avoid visual pops."
                            </p>
                            <div class="docs-callout info">
                                <strong>"Tip:"</strong>
                                " For most games, the default prediction and correction settings work well. 
                                Tune "<code>"prediction_tolerance"</code>" and "<code>"correction_speed"</code>
                                " only if you see visual artifacts."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Remote Events
                    // ─────────────────────────────────────────────────────
                    <section id="events" class="docs-section">
                        <h2 class="section-anchor">"Remote Events"</h2>

                        <div id="events-client-server" class="docs-block">
                            <h3>"Client to Server"</h3>
                            <p>"Clients fire remote events to request actions from the server:"</p>
                            <pre class="code-block"><code>{"#[derive(RemoteEvent, Serialize, Deserialize)]
struct UseAbilityRequest {
    ability_id: u32,
    target: Option<Entity>,
}

// Client sends
remote_events.send_to_server(UseAbilityRequest {
    ability_id: 1,
    target: Some(enemy_entity),
});"}</code></pre>
                        </div>

                        <div id="events-server-client" class="docs-block">
                            <h3>"Server to Client"</h3>
                            <p>"Server sends events to specific clients or broadcasts to all:"</p>
                            <pre class="code-block"><code>{"// Send to specific client
remote_events.send_to_client(client_id, ChatMessage {
    sender: \"Server\".into(),
    text: \"Welcome to the game!\".into(),
});

// Send to all clients
remote_events.broadcast(GameAnnouncement {
    text: \"Round starting in 5 seconds!\".into(),
});"}</code></pre>
                        </div>

                        <div id="events-broadcast" class="docs-block">
                            <h3>"Broadcast"</h3>
                            <p>
                                "Broadcast events are sent to all connected clients simultaneously. Use them for 
                                game-wide announcements, weather changes, or phase transitions."
                            </p>
                        </div>

                        <div id="events-reliability" class="docs-block">
                            <h3>"Reliability Modes"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"Reliable"</code>
                                    <span>"Guaranteed delivery, ordered. Use for important state changes."</span>
                                </div>
                                <div class="api-row">
                                    <code>"ReliableUnordered"</code>
                                    <span>"Guaranteed delivery, any order. Use for events where arrival matters but order does not."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Unreliable"</code>
                                    <span>"Fire and forget. Use for frequent updates where latest value wins."</span>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Matchmaking
                    // ─────────────────────────────────────────────────────
                    <section id="matchmaking" class="docs-section">
                        <h2 class="section-anchor">"Matchmaking"</h2>

                        <div id="matchmaking-teleport" class="docs-block">
                            <h3>"TeleportService"</h3>
                            <p>
                                "TeleportService moves players between experiences and servers. It handles all 
                                the complexity of disconnecting from one server and connecting to another, 
                                preserving player data across the transition."
                            </p>
                            <pre class="code-block"><code>{"// Teleport a player to a different experience
teleport_service.teleport_to_experience(player_entity, ExperienceId(\"abc123\"));

// Teleport to a specific server
teleport_service.teleport_to_server(player_entity, server_address);

// Teleport a group of players together
teleport_service.teleport_group(player_entities, ExperienceId(\"abc123\"));"}</code></pre>
                        </div>

                        <div id="matchmaking-reserved" class="docs-block">
                            <h3>"Reserved Servers"</h3>
                            <p>
                                "Reserved servers are private instances that persist as long as at least one 
                                player is connected. Use them for private matches, party systems, or instanced content."
                            </p>
                            <pre class="code-block"><code>{"// Create a reserved server
let access_code = teleport_service.reserve_server(ExperienceId(\"abc123\")).await?;

// Share the access code with friends
// They join via:
teleport_service.teleport_to_reserved(player_entity, access_code);"}</code></pre>
                        </div>

                        <div id="matchmaking-regions" class="docs-block">
                            <h3>"Region Selection"</h3>
                            <p>
                                "Eustress automatically selects the lowest-latency region for each player. 
                                Supported regions include US-East, US-West, EU-West, EU-Central, Asia-East, 
                                Asia-Southeast, Oceania, and South America."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // Optimization
                    // ─────────────────────────────────────────────────────
                    <section id="optimization" class="docs-section">
                        <h2 class="section-anchor">"Optimization"</h2>

                        <div id="optimization-bandwidth" class="docs-block">
                            <h3>"Bandwidth Management"</h3>
                            <ul class="docs-list">
                                <li><strong>"Delta compression"</strong>" — Only changed fields are transmitted"</li>
                                <li><strong>"Interest management"</strong>" — Only nearby entities are replicated"</li>
                                <li><strong>"Quantization"</strong>" — Positions and rotations compressed to fewer bits"</li>
                                <li><strong>"Priority system"</strong>" — Important entities update more frequently"</li>
                            </ul>
                        </div>

                        <div id="optimization-interpolation" class="docs-block">
                            <h3>"Interpolation"</h3>
                            <p>
                                "Remote entities are rendered with interpolation between received states, creating 
                                smooth visual movement even when network updates arrive at variable intervals. 
                                The default interpolation delay is 100ms (configurable)."
                            </p>
                        </div>

                        <div id="optimization-lag" class="docs-block">
                            <h3>"Lag Compensation"</h3>
                            <p>
                                "For hit detection, the server rewinds entity positions to the time the shot was 
                                fired (based on the client's latency). This ensures fair gameplay regardless 
                                of connection quality."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // API Reference
                    // ─────────────────────────────────────────────────────
                    <section id="api" class="docs-section">
                        <h2 class="section-anchor">"API Reference"</h2>

                        <div id="api-components" class="docs-block">
                            <h3>"Components"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"Replicated"</code>
                                    <span>"Marks an entity for network replication"</span>
                                </div>
                                <div class="api-row">
                                    <code>"NetworkOwner(ClientId)"</code>
                                    <span>"Assigns network ownership of an entity to a client"</span>
                                </div>
                                <div class="api-row">
                                    <code>"ReplicationGroup(u32)"</code>
                                    <span>"Groups entities for atomic replication"</span>
                                </div>
                                <div class="api-row">
                                    <code>"NetworkPriority(f32)"</code>
                                    <span>"Higher priority entities replicate more frequently"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-resources" class="docs-block">
                            <h3>"Resources"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"NetworkServer"</code>
                                    <span>"Server-side networking state and client connections"</span>
                                </div>
                                <div class="api-row">
                                    <code>"NetworkClient"</code>
                                    <span>"Client-side connection state and server info"</span>
                                </div>
                                <div class="api-row">
                                    <code>"NetworkStats"</code>
                                    <span>"Round-trip time, packet loss, bandwidth usage"</span>
                                </div>
                            </div>
                        </div>

                        <div id="api-events" class="docs-block">
                            <h3>"Events"</h3>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"ClientConnected(ClientId)"</code>
                                    <span>"Fired on server when a client connects"</span>
                                </div>
                                <div class="api-row">
                                    <code>"ClientDisconnected(ClientId)"</code>
                                    <span>"Fired on server when a client disconnects"</span>
                                </div>
                                <div class="api-row">
                                    <code>"ServerConnected"</code>
                                    <span>"Fired on client when connection to server succeeds"</span>
                                </div>
                                <div class="api-row">
                                    <code>"ServerDisconnected"</code>
                                    <span>"Fired on client when connection to server is lost"</span>
                                </div>
                            </div>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/scripting" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Scripting"</span>
                            </div>
                        </a>
                        <a href="/docs/physics" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Physics System"</span>
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
