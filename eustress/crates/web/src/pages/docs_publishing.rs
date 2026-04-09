// =============================================================================
// Eustress Web - Publishing Documentation Page (Industrial Design)
// =============================================================================
// Comprehensive publishing documentation with floating TOC
// Covers: publishing flow, Identity.toml, Cloudflare R2, simulation pages,
// versioning, Forge servers, discovery, and content guidelines.
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
                TocSubsection { id: "overview-workflow", title: "Publish Workflow" },
                TocSubsection { id: "overview-requirements", title: "Requirements" },
            ],
        },
        TocSection {
            id: "prepare",
            title: "Prepare to Publish",
            subsections: vec![
                TocSubsection { id: "prepare-identity", title: "Identity.toml" },
                TocSubsection { id: "prepare-metadata", title: "Simulation Metadata" },
                TocSubsection { id: "prepare-thumbnail", title: "Thumbnail & Media" },
                TocSubsection { id: "prepare-age-rating", title: "Age Rating" },
            ],
        },
        TocSection {
            id: "flow",
            title: "Publishing Flow",
            subsections: vec![
                TocSubsection { id: "flow-packaging", title: "Packaging" },
                TocSubsection { id: "flow-upload", title: "Upload to R2" },
                TocSubsection { id: "flow-integrity", title: "Content-Addressable Storage" },
                TocSubsection { id: "flow-status", title: "Publish Status" },
            ],
        },
        TocSection {
            id: "simulation-page",
            title: "Simulation Page",
            subsections: vec![
                TocSubsection { id: "simulation-page-url", title: "URL Structure" },
                TocSubsection { id: "simulation-page-layout", title: "Page Layout" },
                TocSubsection { id: "simulation-page-analytics", title: "Analytics" },
            ],
        },
        TocSection {
            id: "versioning",
            title: "Updates & Versioning",
            subsections: vec![
                TocSubsection { id: "versioning-republish", title: "Re-Publishing" },
                TocSubsection { id: "versioning-history", title: "Version History" },
                TocSubsection { id: "versioning-rollback", title: "Rollback" },
            ],
        },
        TocSection {
            id: "forge",
            title: "Forge Servers",
            subsections: vec![
                TocSubsection { id: "forge-overview", title: "How Forge Works" },
                TocSubsection { id: "forge-scaling", title: "Auto-Scaling" },
                TocSubsection { id: "forge-regions", title: "Regions & Latency" },
            ],
        },
        TocSection {
            id: "discovery",
            title: "Discovery",
            subsections: vec![
                TocSubsection { id: "discovery-tags", title: "Tags & Categories" },
                TocSubsection { id: "discovery-search", title: "Search & Featured" },
                TocSubsection { id: "discovery-trending", title: "Trending Algorithm" },
            ],
        },
        TocSection {
            id: "guidelines",
            title: "Content Guidelines",
            subsections: vec![
                TocSubsection { id: "guidelines-allowed", title: "What's Allowed" },
                TocSubsection { id: "guidelines-moderation", title: "AI Moderation" },
                TocSubsection { id: "guidelines-dmca", title: "DMCA Process" },
                TocSubsection { id: "guidelines-tos", title: "Terms of Service" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Publishing documentation page with floating TOC.
#[component]
pub fn DocsPublishingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-publishing"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/upload.svg" alt="Publishing" class="toc-icon" />
                        <h2>"Publishing"</h2>
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
                            <span class="current">"Publishing"</span>
                        </div>
                        <h1 class="docs-title">"Publishing"</h1>
                        <p class="docs-subtitle">
                            "Ship your simulation to the world. One-click publish from the engine to the
                            eustress.dev gallery, where players discover and play your creation instantly.
                            No server setup, no deployment pipelines, no infrastructure to manage."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "20 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Beginner"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // 1. Overview
                    // ─────────────────────────────────────────────────────
                    <section id="overview" class="docs-section">
                        <h2 class="section-anchor">"1. Overview"</h2>

                        <div id="overview-intro" class="docs-block">
                            <h3>"Introduction"</h3>
                            <p>
                                "Publishing is the final step between building your simulation and sharing it
                                with players worldwide. Eustress makes this as simple as clicking a button in
                                the engine. Your simulation is packaged, uploaded, and listed in the gallery
                                automatically — no CI/CD pipelines, no manual server provisioning, no app store
                                review queues."
                            </p>
                            <div class="docs-callout info">
                                <strong>"Key Concept:"</strong>
                                " When you publish, the engine packages your entire simulation into a content-addressable
                                bundle and uploads it to Cloudflare R2. Players worldwide can discover and join your
                                simulation within seconds of publish completion."
                            </div>
                            <p>
                                "The Eustress gallery at eustress.dev is where players browse, search, and launch
                                simulations. Every published simulation gets its own page with description, screenshots,
                                ratings, and live player counts. Think of it as a global arcade — your simulation is
                                one button press away from being in it."
                            </p>
                        </div>

                        <div id="overview-workflow" class="docs-block">
                            <h3>"Publish Workflow"</h3>
                            <p>"The end-to-end publish process has four stages:"</p>
                            <ol class="docs-list numbered">
                                <li><strong>"Prepare"</strong>" — Configure Identity.toml and simulation metadata"</li>
                                <li><strong>"Package"</strong>" — Engine bundles .eustress/ directory with all assets"</li>
                                <li><strong>"Upload"</strong>" — Bundle is pushed to Cloudflare R2 via api.eustress.dev"</li>
                                <li><strong>"Live"</strong>" — Simulation appears in the gallery and is playable"</li>
                            </ol>
                        </div>

                        <div id="overview-requirements" class="docs-block">
                            <h3>"Requirements"</h3>
                            <p>"Before you can publish, ensure the following:"</p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <h4>"Eustress Account"</h4>
                                    <p>"Registered developer account on eustress.dev with verified email."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Identity.toml"</h4>
                                    <p>"Developer identity file in your project root with valid auth credentials."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Simulation Metadata"</h4>
                                    <p>"Name, description, thumbnail, tags, and category configured."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Age Rating"</h4>
                                    <p>"Self-assessed content rating (Everyone, Teen, Mature)."</p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 2. Prepare to Publish
                    // ─────────────────────────────────────────────────────
                    <section id="prepare" class="docs-section">
                        <h2 class="section-anchor">"2. Prepare to Publish"</h2>

                        <div id="prepare-identity" class="docs-block">
                            <h3>"Identity.toml"</h3>
                            <p>
                                "Your Identity.toml file is the developer identity that ties your simulation to your
                                eustress.dev account. It lives at the root of your project directory and is created
                                automatically when you sign in through the engine."
                            </p>
                            <pre class="code-block"><code>{"# Identity.toml — Developer Identity
# Auto-generated by the Eustress engine on first sign-in.
# Do NOT share this file or commit it to version control.

[identity]
developer_id = \"dev_8f3a2b1c4d5e6f70\"
display_name = \"StudioName\"
email = \"dev@example.com\"

[auth]
token = \"eus_tok_...\"
expires_at = \"2026-12-31T23:59:59Z\"

[profile]
avatar_url = \"https://eustress.dev/avatars/dev_8f3a2b1c.png\"
verified = true"}</code></pre>
                            <div class="docs-callout warning">
                                <strong>"Security:"</strong>
                                " Never commit Identity.toml to version control. It contains your auth token.
                                The default .gitignore template already excludes it."
                            </div>
                        </div>

                        <div id="prepare-metadata" class="docs-block">
                            <h3>"Simulation Metadata"</h3>
                            <p>
                                "Simulation metadata is configured in your project's Simulation.toml file. This
                                defines how your simulation appears in the gallery."
                            </p>
                            <pre class="code-block"><code>{"# Simulation.toml — Simulation Metadata

[simulation]
name = \"Crystal Caverns\"
description = \"Explore procedurally generated crystal caves with friends.\"
version = \"1.0.0\"

[simulation.gallery]
category = \"Adventure\"
tags = [\"exploration\", \"multiplayer\", \"procedural\", \"caves\"]
max_players = 16
thumbnail = \"assets/gallery/thumbnail.png\"

[simulation.screenshots]
images = [
    \"assets/gallery/screen1.png\",
    \"assets/gallery/screen2.png\",
    \"assets/gallery/screen3.png\",
]"}</code></pre>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"name"</code>
                                    <span>"Display name in the gallery. 3-64 characters, must be unique."</span>
                                </div>
                                <div class="api-row">
                                    <code>"description"</code>
                                    <span>"Short description shown on the simulation card. Max 280 characters."</span>
                                </div>
                                <div class="api-row">
                                    <code>"version"</code>
                                    <span>"Semantic version string. Incremented automatically on re-publish."</span>
                                </div>
                                <div class="api-row">
                                    <code>"category"</code>
                                    <span>"Primary category: Adventure, Simulation, Strategy, Social, Creative, Horror, Racing, RPG."</span>
                                </div>
                                <div class="api-row">
                                    <code>"tags"</code>
                                    <span>"Up to 8 searchable tags. Lowercase, alphanumeric, hyphens allowed."</span>
                                </div>
                                <div class="api-row">
                                    <code>"max_players"</code>
                                    <span>"Maximum concurrent players per server instance. Range: 1-100."</span>
                                </div>
                                <div class="api-row">
                                    <code>"thumbnail"</code>
                                    <span>"Path to 16:9 thumbnail image. Minimum 1280x720, PNG or WebP."</span>
                                </div>
                            </div>
                        </div>

                        <div id="prepare-thumbnail" class="docs-block">
                            <h3>"Thumbnail & Media"</h3>
                            <p>"Your thumbnail is the first thing players see. Make it count."</p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <h4>"Thumbnail"</h4>
                                    <p>"1280x720 minimum, 16:9 aspect ratio. PNG or WebP. No text overlays — the gallery adds your title."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Screenshots"</h4>
                                    <p>"Up to 10 screenshots, 1920x1080 recommended. Show diverse gameplay moments."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Icon"</h4>
                                    <p>"Optional 512x512 square icon. Used in search results and player's library."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Video"</h4>
                                    <p>"Optional 30-second gameplay trailer. MP4, max 50MB. Auto-plays on hover in gallery."</p>
                                </div>
                            </div>
                        </div>

                        <div id="prepare-age-rating" class="docs-block">
                            <h3>"Age Rating"</h3>
                            <p>
                                "Every simulation must declare a content rating. This is self-assessed but subject to
                                review by the moderation system."
                            </p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"Everyone"</code>
                                    <span>"Suitable for all ages. No violence, no mature themes, no user-generated text chat."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Teen"</code>
                                    <span>"Mild cartoon violence, competitive gameplay, moderated text chat allowed."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Mature"</code>
                                    <span>"Realistic violence, complex themes, unmoderated voice chat. Age verification required."</span>
                                </div>
                            </div>
                            <pre class="code-block"><code>{"# In Simulation.toml
[simulation.rating]
content_rating = \"Teen\"
descriptors = [\"mild-violence\", \"online-interactions\"]"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 3. Publishing Flow
                    // ─────────────────────────────────────────────────────
                    <section id="flow" class="docs-section">
                        <h2 class="section-anchor">"3. Publishing Flow"</h2>

                        <div id="flow-packaging" class="docs-block">
                            <h3>"Packaging"</h3>
                            <p>
                                "When you click Publish in the engine, the first step is packaging. The engine
                                collects everything needed to run your simulation into the .eustress/ directory."
                            </p>
                            <pre class="code-block"><code>{".eustress/
  manifest.json        # Package manifest with hashes
  simulation.wasm      # Compiled simulation logic
  assets/
    models/            # 3D models (glTF, compressed)
    textures/          # Textures (KTX2, basis-compressed)
    audio/             # Audio files (Opus-encoded)
    scenes/            # Scene definitions
  Simulation.toml      # Metadata (copied from project root)
  thumbnail.webp       # Gallery thumbnail (optimized)"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Optimization:"</strong>
                                " The packaging step automatically compresses textures to KTX2/Basis,
                                encodes audio to Opus, and strips unused assets. A 2GB project directory
                                typically packages down to 200-400MB."
                            </div>
                        </div>

                        <div id="flow-upload" class="docs-block">
                            <h3>"Upload to R2"</h3>
                            <p>
                                "The packaged bundle is uploaded to Cloudflare R2 via the api.eustress.dev endpoint.
                                Uploads are chunked and resumable — if your connection drops mid-upload, the engine
                                picks up where it left off."
                            </p>
                            <pre class="code-block"><code>{"// Internal upload flow (simplified)
// You don't write this code — the engine handles it automatically.

POST api.eustress.dev/v1/publish/init
  -> { upload_id, presigned_urls[] }

PUT r2.eustress.dev/chunks/{upload_id}/{chunk_n}
  -> { etag }

POST api.eustress.dev/v1/publish/finalize
  -> { simulation_id, version, gallery_url }"}</code></pre>
                            <ul class="docs-list">
                                <li><strong>"Chunked uploads"</strong>" — 8MB chunks with parallel upload (4 concurrent)"</li>
                                <li><strong>"Resumable"</strong>" — Drop your connection and resume later, no re-upload"</li>
                                <li><strong>"Progress bar"</strong>" — Real-time upload progress in the engine UI"</li>
                                <li><strong>"Compression"</strong>" — Chunks are zstd-compressed before upload, saving 30-50% bandwidth"</li>
                            </ul>
                        </div>

                        <div id="flow-integrity" class="docs-block">
                            <h3>"Content-Addressable Storage"</h3>
                            <p>
                                "Every file in the bundle is stored by its BLAKE3 hash. This provides three
                                key guarantees:"
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <h4>"Integrity"</h4>
                                    <p>"Every byte is verified. Corrupted uploads are detected and rejected immediately."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Deduplication"</h4>
                                    <p>"Shared assets across versions are stored once. Re-publishing with minor changes uploads only the diff."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Immutability"</h4>
                                    <p>"Published content cannot be silently altered. The hash is the identity."</p>
                                </div>
                            </div>
                            <pre class="code-block"><code>{"// manifest.json (generated during packaging)
{
  \"simulation_id\": \"sim_7a3f2e1b\",
  \"version\": \"1.2.0\",
  \"files\": {
    \"simulation.wasm\": {
      \"hash\": \"blake3:a1b2c3d4e5f6...\",
      \"size\": 4218432
    },
    \"assets/textures/terrain.ktx2\": {
      \"hash\": \"blake3:f6e5d4c3b2a1...\",
      \"size\": 8392704
    }
  },
  \"total_size\": 314572800,
  \"published_at\": \"2026-04-03T12:00:00Z\"
}"}</code></pre>
                        </div>

                        <div id="flow-status" class="docs-block">
                            <h3>"Publish Status"</h3>
                            <p>"After upload completes, your simulation goes through a brief processing pipeline:"</p>
                            <ol class="docs-list numbered">
                                <li><strong>"Uploaded"</strong>" — All chunks received and verified"</li>
                                <li><strong>"Processing"</strong>" — Asset validation, thumbnail generation, metadata indexing"</li>
                                <li><strong>"Screening"</strong>" — Automated content moderation scan (typically under 60 seconds)"</li>
                                <li><strong>"Live"</strong>" — Simulation is visible in the gallery and playable"</li>
                            </ol>
                            <div class="docs-callout info">
                                <strong>"Typical Time:"</strong>
                                " From clicking Publish to Live status takes 2-5 minutes depending on bundle size.
                                You receive a notification in the engine when your simulation goes live."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 4. Simulation Page
                    // ─────────────────────────────────────────────────────
                    <section id="simulation-page" class="docs-section">
                        <h2 class="section-anchor">"4. Simulation Page"</h2>

                        <div id="simulation-page-url" class="docs-block">
                            <h3>"URL Structure"</h3>
                            <p>
                                "Every published simulation gets a permanent URL on eustress.dev. The URL uses
                                the simulation's unique identifier, which is assigned on first publish and never changes."
                            </p>
                            <pre class="code-block"><code>{"https://eustress.dev/simulation/sim_7a3f2e1b

# URL anatomy:
# eustress.dev       — Platform root
# /simulation/       — Simulation namespace
# sim_7a3f2e1b       — Unique simulation ID (stable across versions)"}</code></pre>
                            <p>
                                "You can also set a custom slug after your first publish, giving you a friendlier URL:"
                            </p>
                            <pre class="code-block"><code>{"https://eustress.dev/simulation/crystal-caverns"}</code></pre>
                        </div>

                        <div id="simulation-page-layout" class="docs-block">
                            <h3>"Page Layout"</h3>
                            <p>"Your simulation page displays everything players need to decide whether to play:"</p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"Hero Banner"</code>
                                    <span>"Your thumbnail displayed at full width with a Play button overlay."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Description"</code>
                                    <span>"Full description with markdown support. Up to 4000 characters."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Screenshots"</code>
                                    <span>"Carousel of up to 10 screenshots. Click to expand fullscreen."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Ratings"</code>
                                    <span>"Community star rating (1-5) and written reviews."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Player Count"</code>
                                    <span>"Real-time count of active players. Updates every 30 seconds."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Developer Info"</code>
                                    <span>"Your studio name, avatar, and link to your other simulations."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Version History"</code>
                                    <span>"Changelog entries for each published version."</span>
                                </div>
                            </div>
                        </div>

                        <div id="simulation-page-analytics" class="docs-block">
                            <h3>"Analytics"</h3>
                            <p>
                                "Your developer dashboard at eustress.dev/dashboard provides analytics for each
                                published simulation:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Daily Active Players"</strong>" — Unique players per day over the last 30 days"</li>
                                <li><strong>"Session Duration"</strong>" — Average time spent per play session"</li>
                                <li><strong>"Retention"</strong>" — Day-1, Day-7, Day-30 return rates"</li>
                                <li><strong>"Geography"</strong>" — Player distribution by region"</li>
                                <li><strong>"Performance"</strong>" — Server tick rate, memory usage, crash reports"</li>
                                <li><strong>"Ratings"</strong>" — Rating distribution and recent reviews"</li>
                            </ul>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 5. Updates & Versioning
                    // ─────────────────────────────────────────────────────
                    <section id="versioning" class="docs-section">
                        <h2 class="section-anchor">"5. Updates & Versioning"</h2>

                        <div id="versioning-republish" class="docs-block">
                            <h3>"Re-Publishing"</h3>
                            <p>
                                "To update your simulation, simply click Publish again. The engine detects that
                                this simulation has been published before and performs a delta upload — only new
                                or changed files are transmitted."
                            </p>
                            <pre class="code-block"><code>{"// Delta upload behavior:
// Version 1.0.0 — Full upload: 300MB
// Version 1.1.0 — Changed 3 textures, 1 script: uploads only 12MB
// Version 1.2.0 — New audio files added: uploads only 45MB
//
// Content-addressable storage means unchanged files are never re-uploaded."}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Automatic Versioning:"</strong>
                                " The version in Simulation.toml is auto-incremented on each publish if you
                                haven't manually changed it. Patch version bumps by default (1.0.0 -> 1.0.1)."
                            </div>
                        </div>

                        <div id="versioning-history" class="docs-block">
                            <h3>"Version History"</h3>
                            <p>
                                "Every publish creates an immutable version snapshot. Players always get the latest
                                version when they join, but the full history is preserved on the server."
                            </p>
                            <pre class="code-block"><code>{"# Version history for Crystal Caverns
#
# v1.2.0  2026-04-03  Added underwater caves biome
# v1.1.0  2026-03-20  Multiplayer voice chat, new crystal types
# v1.0.1  2026-03-15  Fixed spawning bug near lava pools
# v1.0.0  2026-03-10  Initial release"}</code></pre>
                            <p>
                                "Players automatically receive the latest version. There is no manual update button —
                                when a player joins, they always get the current version. If they are in an active
                                session when you publish, they continue on the old version until they rejoin."
                            </p>
                        </div>

                        <div id="versioning-rollback" class="docs-block">
                            <h3>"Rollback"</h3>
                            <p>
                                "Shipped a broken update? Roll back to any previous version from the developer dashboard
                                or directly from the engine."
                            </p>
                            <pre class="code-block"><code>{"// Rollback via the engine CLI
eustress publish rollback --to 1.1.0

// Or from the dashboard:
// eustress.dev/dashboard/sim_7a3f2e1b/versions
// Click \"Activate\" on any previous version"}</code></pre>
                            <div class="docs-callout warning">
                                <strong>"Note:"</strong>
                                " Rollback is instant because all previous versions are preserved in R2.
                                No re-upload required. New players immediately get the rolled-back version."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 6. Forge Servers
                    // ─────────────────────────────────────────────────────
                    <section id="forge" class="docs-section">
                        <h2 class="section-anchor">"6. Forge Servers"</h2>

                        <div id="forge-overview" class="docs-block">
                            <h3>"How Forge Works"</h3>
                            <p>
                                "Forge is the Eustress server fleet. When a player clicks Play on your simulation,
                                a Forge server spins up automatically via HashiCorp Nomad. You don't provision servers,
                                configure networking, or manage infrastructure. Forge handles all of it."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-card">
                                    <h4>"Zero Configuration"</h4>
                                    <p>"No server setup. No Docker, no Kubernetes, no SSH. Publish and it just works."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Scale to Zero"</h4>
                                    <p>"No players? No servers running. No cost when idle. Servers spin up on demand."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Scale to Thousands"</h4>
                                    <p>"Viral moment? Forge scales automatically. Thousands of concurrent servers across regions."</p>
                                </div>
                                <div class="feature-card">
                                    <h4>"Sub-Second Startup"</h4>
                                    <p>"WASM-based servers start in under 500ms. Players never wait in a loading queue."</p>
                                </div>
                            </div>
                        </div>

                        <div id="forge-scaling" class="docs-block">
                            <h3>"Auto-Scaling"</h3>
                            <p>"Forge scaling is driven by player demand:"</p>
                            <ol class="docs-list numbered">
                                <li><strong>"Player clicks Play"</strong>" — Matchmaking finds an existing server with room, or requests a new one"</li>
                                <li><strong>"Nomad schedules allocation"</strong>" — New server binary is placed on the nearest available node"</li>
                                <li><strong>"Server boots"</strong>" — WASM simulation loads from R2 cache, starts in under 500ms"</li>
                                <li><strong>"Player connects"</strong>" — QUIC connection established, game begins"</li>
                                <li><strong>"Server drains"</strong>" — When the last player leaves, server stays warm for 5 minutes, then shuts down"</li>
                            </ol>
                            <pre class="code-block"><code>{"// Scaling behavior (you don't configure this — it's automatic)
//
// 0 players   -> 0 servers   (scale to zero)
// 1 player    -> 1 server    (cold start ~500ms)
// 50 players  -> 4 servers   (max_players=16 per server)
// 500 players -> 32 servers  (spread across 3 regions)
// 5000 players -> 313 servers (spread across all regions)"}</code></pre>
                        </div>

                        <div id="forge-regions" class="docs-block">
                            <h3>"Regions & Latency"</h3>
                            <p>
                                "Forge servers are deployed across global regions. Players are automatically
                                routed to the lowest-latency region."
                            </p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"us-east"</code>
                                    <span>"Ashburn, Virginia, USA"</span>
                                </div>
                                <div class="api-row">
                                    <code>"us-west"</code>
                                    <span>"Los Angeles, California, USA"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eu-west"</code>
                                    <span>"London, United Kingdom"</span>
                                </div>
                                <div class="api-row">
                                    <code>"eu-central"</code>
                                    <span>"Frankfurt, Germany"</span>
                                </div>
                                <div class="api-row">
                                    <code>"asia-east"</code>
                                    <span>"Tokyo, Japan"</span>
                                </div>
                                <div class="api-row">
                                    <code>"asia-southeast"</code>
                                    <span>"Singapore"</span>
                                </div>
                                <div class="api-row">
                                    <code>"oceania"</code>
                                    <span>"Sydney, Australia"</span>
                                </div>
                                <div class="api-row">
                                    <code>"south-america"</code>
                                    <span>"Sao Paulo, Brazil"</span>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 7. Discovery
                    // ─────────────────────────────────────────────────────
                    <section id="discovery" class="docs-section">
                        <h2 class="section-anchor">"7. Discovery"</h2>

                        <div id="discovery-tags" class="docs-block">
                            <h3>"Tags & Categories"</h3>
                            <p>
                                "Tags and categories are the primary way players find your simulation. Choose them
                                carefully — they directly impact your visibility in search and browse results."
                            </p>
                            <div class="api-table">
                                <div class="api-row">
                                    <code>"Adventure"</code>
                                    <span>"Exploration, quests, open-world, story-driven experiences."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Simulation"</code>
                                    <span>"Physics sandboxes, life sims, management, building."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Strategy"</code>
                                    <span>"Real-time or turn-based strategy, tower defense, resource management."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Social"</code>
                                    <span>"Hangout spaces, virtual events, social hubs."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Creative"</code>
                                    <span>"Art tools, music makers, sandbox building."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Horror"</code>
                                    <span>"Survival horror, psychological horror, co-op scares."</span>
                                </div>
                                <div class="api-row">
                                    <code>"Racing"</code>
                                    <span>"Vehicles, racing, flight simulators."</span>
                                </div>
                                <div class="api-row">
                                    <code>"RPG"</code>
                                    <span>"Role-playing, character progression, loot, dungeons."</span>
                                </div>
                            </div>
                        </div>

                        <div id="discovery-search" class="docs-block">
                            <h3>"Search & Featured"</h3>
                            <p>
                                "The gallery supports full-text search across simulation names, descriptions, and tags.
                                Results are ranked by relevance, with active player count as a tiebreaker."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Search"</strong>" — Full-text search with typo tolerance and synonym matching"</li>
                                <li><strong>"Browse"</strong>" — Filter by category, rating, player count, or age rating"</li>
                                <li><strong>"Featured"</strong>" — Curated weekly rotation selected by the Eustress team"</li>
                                <li><strong>"New & Notable"</strong>" — Recently published simulations with strong early ratings"</li>
                                <li><strong>"Top Rated"</strong>" — Highest-rated simulations with minimum review threshold"</li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"Getting Featured:"</strong>
                                " The Featured rotation is curated by the Eustress team. High-quality simulations
                                with strong player retention and positive ratings are considered. No application
                                process — we discover and feature organically."
                            </div>
                        </div>

                        <div id="discovery-trending" class="docs-block">
                            <h3>"Trending Algorithm"</h3>
                            <p>
                                "The Trending section surfaces simulations with rapidly growing player bases.
                                The algorithm considers:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Player velocity"</strong>" — Rate of new player joins over the last 24 hours"</li>
                                <li><strong>"Active ratio"</strong>" — Current players vs. peak players (engagement signal)"</li>
                                <li><strong>"Session length"</strong>" — Average session duration (quality signal)"</li>
                                <li><strong>"Rating momentum"</strong>" — Recent ratings trend (improving or declining)"</li>
                                <li><strong>"Freshness"</strong>" — Recency of last update (active development bonus)"</li>
                            </ul>
                            <p>
                                "Trending recalculates hourly. A small simulation with 10 players today and 200 players
                                tomorrow will rank higher than an established simulation with steady 5000 players.
                                The algorithm rewards growth, not absolute size."
                            </p>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 8. Content Guidelines
                    // ─────────────────────────────────────────────────────
                    <section id="guidelines" class="docs-section">
                        <h2 class="section-anchor">"8. Content Guidelines"</h2>

                        <div id="guidelines-allowed" class="docs-block">
                            <h3>"What's Allowed"</h3>
                            <p>
                                "Eustress is an open platform. Most content is welcome as long as it respects
                                other players and complies with applicable law."
                            </p>
                            <div class="comparison-cards">
                                <div class="lang-card soul">
                                    <h4>"Allowed"</h4>
                                    <ul class="docs-list">
                                        <li>"Original creative works"</li>
                                        <li>"Competitive and cooperative gameplay"</li>
                                        <li>"Stylized or cartoon violence (with appropriate rating)"</li>
                                        <li>"Social and educational experiences"</li>
                                        <li>"Fan-made content with original assets"</li>
                                        <li>"Mature themes with correct age rating"</li>
                                    </ul>
                                </div>
                                <div class="lang-card">
                                    <h4>"Not Allowed"</h4>
                                    <ul class="docs-list">
                                        <li>"Harassment, bullying, or hate speech"</li>
                                        <li>"Content exploiting minors"</li>
                                        <li>"Copyright-infringing assets"</li>
                                        <li>"Real-money gambling"</li>
                                        <li>"Malware, exploits, or data harvesting"</li>
                                        <li>"Impersonation of real individuals"</li>
                                    </ul>
                                </div>
                            </div>
                        </div>

                        <div id="guidelines-moderation" class="docs-block">
                            <h3>"AI Moderation"</h3>
                            <p>
                                "Every simulation is scanned by an AI moderation system during the publish pipeline.
                                The system checks for:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Asset scanning"</strong>" — Textures and models checked for prohibited content"</li>
                                <li><strong>"Text analysis"</strong>" — In-simulation text, UI strings, and metadata reviewed"</li>
                                <li><strong>"Behavior analysis"</strong>" — Scripting patterns checked for malicious behavior"</li>
                                <li><strong>"Rating verification"</strong>" — Content compared against declared age rating"</li>
                            </ul>
                            <div class="docs-callout warning">
                                <strong>"False Positives:"</strong>
                                " If your simulation is incorrectly flagged, you can appeal through the developer
                                dashboard. Human review is completed within 24 hours."
                            </div>
                        </div>

                        <div id="guidelines-dmca" class="docs-block">
                            <h3>"DMCA Process"</h3>
                            <p>
                                "Eustress complies with the Digital Millennium Copyright Act. If you believe a
                                simulation infringes on your copyright:"
                            </p>
                            <ol class="docs-list numbered">
                                <li><strong>"File a claim"</strong>" — Submit a DMCA takedown notice via eustress.dev/legal/dmca"</li>
                                <li><strong>"Review"</strong>" — Our legal team reviews the claim within 48 hours"</li>
                                <li><strong>"Action"</strong>" — Infringing content is removed and the developer notified"</li>
                                <li><strong>"Counter-notice"</strong>" — Developers can file a counter-notice if they believe the claim is invalid"</li>
                            </ol>
                            <p>
                                "Repeat infringers face permanent account suspension. We take intellectual property
                                rights seriously."
                            </p>
                        </div>

                        <div id="guidelines-tos" class="docs-block">
                            <h3>"Terms of Service"</h3>
                            <p>
                                "By publishing on Eustress, you agree to the Developer Terms of Service. Key points:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"You retain ownership"</strong>" — Your simulation remains your intellectual property"</li>
                                <li><strong>"License grant"</strong>" — You grant Eustress a license to host, distribute, and cache your content"</li>
                                <li><strong>"Takedown rights"</strong>" — You can unpublish your simulation at any time"</li>
                                <li><strong>"Content responsibility"</strong>" — You are responsible for the content of your simulation"</li>
                                <li><strong>"Revenue terms"</strong>" — See the Earning documentation for monetization terms"</li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"Full Terms:"</strong>
                                " Read the complete Developer Terms of Service at eustress.dev/legal/developer-tos.
                                The terms are written in plain English — no legal obfuscation."
                            </div>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/audio" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Audio"</span>
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
