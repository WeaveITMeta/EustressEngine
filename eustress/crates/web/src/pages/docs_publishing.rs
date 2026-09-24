// =============================================================================
// Eustress Web - Publishing Documentation Page
// =============================================================================
// Publishing: how Studio packages a Universe, uploads it to eustress.dev and
// submits it for review, and what the website shows once review decides.
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
                TocSubsection { id: "overview-what", title: "What Publishing Does" },
                TocSubsection { id: "overview-needs", title: "Before You Publish" },
            ],
        },
        TocSection {
            id: "studio",
            title: "Publish from Studio",
            subsections: vec![
                TocSubsection { id: "studio-open", title: "Opening the Dialog" },
                TocSubsection { id: "studio-fields", title: "The Publish Dialog" },
                TocSubsection { id: "studio-progress", title: "While It Uploads" },
            ],
        },
        TocSection {
            id: "upload",
            title: "What Gets Uploaded",
            subsections: vec![
                TocSubsection { id: "upload-package", title: "The World as Chunks" },
                TocSubsection { id: "upload-steps", title: "Upload and Storage" },
                TocSubsection { id: "upload-files", title: "Files Publishing Writes" },
            ],
        },
        TocSection {
            id: "review",
            title: "Review",
            subsections: vec![
                TocSubsection { id: "review-inputs", title: "What Review Reads" },
                TocSubsection { id: "review-ladder", title: "The Review Ladder" },
                TocSubsection { id: "review-outcomes", title: "Outcomes and Ratings" },
            ],
        },
        TocSection {
            id: "web",
            title: "On the Website",
            subsections: vec![
                TocSubsection { id: "web-gallery", title: "The Gallery" },
                TocSubsection { id: "web-listing", title: "Listing and Play Pages" },
                TocSubsection { id: "web-projects", title: "Your Projects" },
            ],
        },
        TocSection {
            id: "updates",
            title: "Updates",
            subsections: vec![
                TocSubsection { id: "updates-again", title: "Publishing Again" },
                TocSubsection { id: "updates-space", title: "Updating One Space" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-listing", title: "Removing a Listing" },
                TocSubsection { id: "roadmap-review", title: "Review Goes Live" },
                TocSubsection { id: "roadmap-play", title: "Playing Published Worlds" },
            ],
        },
    ]
}

/// The publish path from Studio to the Gallery: five stops, and the rule that
/// decides the last one.
#[component]
fn PublishFlowDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 200" role="img"
                aria-label="Studio saves the Universe and bakes it into chunks, the Eustress API finds or creates the listing and names the chunks it lacks, Cloudflare R2 stores those chunks, review reads the dossier and four captured views, and the Gallery shows the listing only after review approves it and it is marked Public.">
                <defs>
                    <marker id="publish-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                <text x="10" y="40" class="dg-title">"Publish Universe"</text>

                <rect x="10" y="70" width="100" height="60" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="60" y="96" class="dg-label" text-anchor="middle">"Studio"</text>
                <text x="60" y="116" class="dg-note" text-anchor="middle">"save, bake"</text>

                <line x1="110" y1="100" x2="138" y2="100" class="dg-line" marker-end="url(#publish-arrow)"></line>

                <rect x="140" y="70" width="100" height="60" rx="8" class="dg-box"></rect>
                <text x="190" y="96" class="dg-label" text-anchor="middle">"API"</text>
                <text x="190" y="116" class="dg-note" text-anchor="middle">"listing, diff"</text>

                <line x1="240" y1="100" x2="268" y2="100" class="dg-line" marker-end="url(#publish-arrow)"></line>

                <rect x="270" y="70" width="100" height="60" rx="8" class="dg-box"></rect>
                <text x="320" y="96" class="dg-label" text-anchor="middle">"R2"</text>
                <text x="320" y="116" class="dg-note" text-anchor="middle">"new chunks"</text>

                <line x1="370" y1="100" x2="398" y2="100" class="dg-line" marker-end="url(#publish-arrow)"></line>

                <rect x="400" y="70" width="100" height="60" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="450" y="96" class="dg-label" text-anchor="middle">"Review"</text>
                <text x="450" y="116" class="dg-note" text-anchor="middle">"dossier, views"</text>

                <line x1="500" y1="100" x2="528" y2="100" class="dg-line" marker-end="url(#publish-arrow)"></line>

                <rect x="530" y="70" width="100" height="60" rx="8" class="dg-box"></rect>
                <text x="580" y="96" class="dg-label" text-anchor="middle">"Gallery"</text>
                <text x="580" y="116" class="dg-note" text-anchor="middle">"if approved"</text>

                <text x="320" y="170" class="dg-note" text-anchor="middle">"A listing is served only when review approved it and you marked it Public"</text>
            </svg>
            <figcaption>
                "Studio bakes the world and uploads what changed; the API decides what the Gallery shows."
            </figcaption>
        </figure>
    }
}

/// Publishing documentation page.
#[component]
pub fn DocsPublishingPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-publishing"></div>
            </div>

            <div class="docs-layout">
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Publishing"</span>
                        </div>
                        <h1 class="docs-title">"Publishing"</h1>
                        <p class="docs-subtitle">
                            "Publishing uploads a Universe from Studio to eustress.dev. Studio bakes every
                            Space into chunks, uploads the chunks eustress.dev does not already hold into the
                            Universe's listing and submits it for review, and the Gallery lists it once review
                            approves it and you have marked it Public."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "13 min read"
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

                        <div id="overview-what" class="subsection">
                            <h3>"What Publishing Does"</h3>
                            <p>
                                "Publishing takes the Universe you are working in, with every Space inside it,
                                and stores it on eustress.dev under a listing: a name, a description, a genre and
                                a thumbnail. You do it from one dialog in Studio. The Eustress API at
                                api.eustress.dev keeps the listing record and stores the world's chunks in
                                Cloudflare R2."
                            </p>
                            <PublishFlowDiagram />
                            <p>
                                "Publishing stores a Universe so people can find it. It does not start servers or
                                host sessions; where multiplayer stands is covered on the "
                                <a href="/docs/networking">"Networking"</a>" page."
                            </p>
                        </div>

                        <div id="overview-needs" class="subsection">
                            <h3>"Before You Publish"</h3>
                            <ul class="docs-list">
                                <li><strong>"An open Space"</strong>": publishing starts from the Space you have open and publishes the Universe that contains it. Players start in the Space you had open."</li>
                                <li><strong>"An Eustress account"</strong>": registering on eustress.dev checks a government ID and your age, then gives you an identity file named "<code>"eustress-<username>.toml"</code>"."</li>
                                <li><strong>"A Studio sign-in"</strong>": choose Sign In on the ribbon, browse to your identity file and press Sign In with Identity. Studio signs a challenge from the API with the file's Ed25519 private key and receives the session token that every publish request carries."</li>
                            </ul>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"The age check reads your document"</strong>
                                    <p>
                                        "The minimum age is 18, raised where the local age of majority is higher:
                                        19 in Canada and South Korea, 20 in Thailand, and 21 in Singapore,
                                        Indonesia, the United Arab Emirates and Egypt. The check uses the date of
                                        birth read from your ID document, not the one you type, and an unreadable
                                        date is refused rather than guessed. If your computer has no usable
                                        camera, registration shows a QR code that opens the phone capture page, "
                                        <code>"/verify"</code>", with your session attached."
                                    </p>
                                </div>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Signing in offline leaves you unable to publish"</strong>
                                    <p>
                                        "If the API cannot be reached when you sign in, Studio still shows you as
                                        signed in with a local identity, but the API refuses requests without a
                                        session token, so a publish fails at its first step. Sign in again once
                                        you are online."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // PUBLISH FROM STUDIO
                    // =========================================================
                    <section id="studio" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Publish from Studio"
                        </h2>

                        <div id="studio-open" class="subsection">
                            <h3>"Opening the Dialog"</h3>
                            <p>"Both publish commands are in the File menu:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Command"</th><th>"Shortcut"</th><th>"What it publishes"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Publish Universe"</td><td><code>"Ctrl+P"</code></td><td>"Every Space in the Universe, and its shared assets"</td></tr>
                                    <tr><td>"Publish Space"</td><td><code>"Ctrl+Shift+P"</code></td><td>"Only the open Space, as an update to a published Universe"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The Roblox keymap preset gives "<code>"Ctrl+Shift+P"</code>" to the Properties
                                filter and moves Publish Space to "<code>"Ctrl+Alt+Shift+P"</code>". Publish Space
                                works once the Universe has been published, as "
                                <a href="#updates-space">"Updating One Space"</a>" explains."
                            </p>
                        </div>

                        <div id="studio-fields" class="subsection">
                            <h3>"The Publish Dialog"</h3>
                            <p>
                                "The dialog's left side lists the files that will be published, with the open Space
                                marked "<em>"primary"</em>". The right side holds the listing:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Field"</th><th>"What it does"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Simulation Name"</td><td>"The listing name. It starts as the Universe folder's name, and Publish stays disabled while it is empty."</td></tr>
                                    <tr><td>"Description"</td><td>"The listing description."</td></tr>
                                    <tr><td>"Genre"</td><td>"All, Adventure, Building, Comedy, Fighting, FPS, Horror, Medieval, Military, Naval, RPG, Sci-Fi, Sports, Town and City, or Western."</td></tr>
                                    <tr><td>"Public"</td><td>"On by default. On: "<em>"Anyone can play"</em>", once review approves. Off: "<em>"Only you can access"</em>"."</td></tr>
                                    <tr><td>"Share Source"</td><td>"Allow source access and reuse. Studio records the choice in the Space's publish manifest and in the review dossier; the listing on the website does not carry it yet."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "A line under the fields names the storage target, Cloudflare R2. The button reads
                                Publish, and Publishing while the upload runs."
                            </p>
                        </div>

                        <div id="studio-progress" class="subsection">
                            <h3>"While It Uploads"</h3>
                            <p>
                                "Publish saves the Space first, shows "<em>"Publishing Universe: baking every Space
                                and uploading what changed."</em>" and does the rest on a background thread, so you
                                can keep working. Four problems stop it before anything is uploaded:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"No open Space"</strong>": "<em>"Publish requires an open Space folder."</em></li>
                                <li><strong>"No session"</strong>": "<em>"Sign in to publish."</em></li>
                                <li><strong>"A Space still opening"</strong>": "<em>"The Space is still opening. Publish again in a moment."</em></li>
                                <li><strong>"A Website reference that does not resolve"</strong>": the message names the reference and the nearest candidates. See "<a href="/docs/website">"Website Service"</a>"."</li>
                            </ul>
                            <p>
                                "When nothing changed since the last publish, neither the world nor the listing
                                text, Studio uploads nothing and reports "<em>"No changes since the last
                                publish"</em>"."
                            </p>
                            <p>
                                "Studio does not show the outcome in the editor yet. The final line goes to the
                                engine log, "<code>"~/.eustress_engine/logs/engine-<pid>.log"</code>": "
                                <code>"Published successfully:"</code>" followed by the listing id and a summary of
                                the review, or "<code>"Publish failed:"</code>" followed by the reason. The
                                Projects page on eustress.dev shows the same status."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT GETS UPLOADED
                    // =========================================================
                    <section id="upload" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "What Gets Uploaded"
                        </h2>

                        <div id="upload-package" class="subsection">
                            <h3>"The World as Chunks"</h3>
                            <p>
                                "Studio bakes each Space into "<code>".echk"</code>" chunks: small containers of
                                files, each named by the BLAKE3 hash of its bytes. A Space's entities are read
                                from its database in their current state, so edits you have not saved to a file
                                still go up; files that exist only in the Space folder, such as meshes an import
                                wrote, are added to them. Entities are grouped by position into 256 m squares, so a
                                change in one part of a map changes only that part's chunk. Files with no position
                                (scripts, meshes, textures) share a chunk, split whenever it grows past 64 MB, and
                                the Universe's shared "<code>"assets"</code>" folder goes up in chunks of about
                                32 MB."
                            </p>
                            <p>"A publish leaves out:"</p>
                            <ul class="docs-list">
                                <li><strong>"Hidden folders"</strong>": any name starting with a dot, such as "<code>".eustress"</code>" and "<code>".git"</code></li>
                                <li><strong>"The database files themselves"</strong>": "<code>"world.fjalldb"</code>" and "<code>"header.bin"</code>", whose content the chunks already carry"</li>
                                <li><strong>"Operating system and temporary files"</strong>": "<code>"Thumbs.db"</code>", "<code>"desktop.ini"</code>", and anything ending in "<code>".lock"</code>" or "<code>".tmp"</code></li>
                            </ul>
                            <p>
                                "A manifest lists every Space's chunks, the asset chunks, and the Space players
                                open first. Its BLAKE3 hash is the publish's content id, in the form "
                                <code>"blake3:<hex>"</code>". The API records it; the Player checks every chunk it
                                downloads against its name, so a chunk that does not match is never opened."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"One file larger than 95 MB stops a publish"</strong>
                                    <p>
                                        "A chunk goes up in a single request, and a request carries at most 95 MB.
                                        Chunks are split to stay under that, but a single file larger than it
                                        cannot be split, and the publish stops naming it."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="upload-steps" class="subsection">
                            <h3>"Upload and Storage"</h3>
                            <ol class="numbered-list">
                                <li>
                                    <strong>"Find or create the listing."</strong>
                                    " The first publish of a Universe creates its listing and keeps the id in the
                                    Universe's "<code>".eustress/sync.toml"</code>" at once, so a publish that
                                    fails halfway retries into the same listing. Every later publish goes to that
                                    listing. Every listing records 10 as its player limit."
                                </li>
                                <li>
                                    <strong>"Compare."</strong>
                                    " Studio sends the manifest, and the API answers with the chunks it does not
                                    hold yet."
                                </li>
                                <li>
                                    <strong>"Upload the new chunks."</strong>
                                    " Only those, one request each. The API checks each is an "
                                    <code>".echk"</code>" container and stores it in Cloudflare R2 at "
                                    <code>"universes/<id>/chunks/<hash>.echk"</code>". A republish that changed one
                                    building uploads one chunk."
                                </li>
                                <li>
                                    <strong>"Commit."</strong>
                                    " The API confirms every chunk is stored at the size the manifest gives,
                                    stores the manifest, and points the listing at it. New content or new listing
                                    text puts the listing back to pending review."
                                </li>
                                <li>
                                    <strong>"Upload the Website manifest"</strong>
                                    ", when the Space has a Website service. If this upload fails, the publish
                                    fails, so a website never keeps showing numbers from the previous publish."
                                </li>
                                <li>
                                    <strong>"Upload the thumbnail."</strong>
                                    " The first of "<code>"thumbnail.png"</code>", "<code>"thumbnail.webp"</code>
                                    " and "<code>"thumbnail.jpg"</code>" found in the Universe's "
                                    <code>".eustress"</code>" folder, up to 5 MB."
                                </li>
                                <li>
                                    <strong>"Submit for review."</strong>
                                    " Covered in the next section. If submission does not go through, the
                                    listing stays pending while the world is already stored."
                                </li>
                            </ol>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Frame the shot before you press Publish"</strong>
                                    <p>
                                        "Unless "<code>".eustress/thumbnail.png"</code>" was written in the last 5
                                        minutes, Studio captures the viewport, scales it to 512 by 288 pixels and
                                        saves it there, replacing the file. Because the PNG is uploaded ahead of
                                        any WebP or JPEG, the thumbnail is normally whatever the viewport shows
                                        when you publish."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="upload-files" class="subsection">
                            <h3>"Files Publishing Writes"</h3>
                            <p>"A publish leaves these files behind, all of them inside the Universe folder:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"File"</th><th>"Location"</th><th>"Holds"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"thumbnail.png"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The viewport capture, 512 by 288"</td></tr>
                                    <tr><td><code>"publish/"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The baked chunks, kept so an unchanged Space is not rewritten"</td></tr>
                                    <tr><td><code>".last_publish_hash"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The BLAKE3 hash of the last committed manifest"</td></tr>
                                    <tr><td><code>".last_publish_state"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"What the last publish sent, so an unchanged one is skipped"</td></tr>
                                    <tr><td><code>"moderation-dossier.json"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The evidence review reads"</td></tr>
                                    <tr><td><code>"capture-0.png"</code>" to "<code>"capture-3.png"</code></td><td>"Universe "<code>".eustress/moderation/"</code></td><td>"The review views"</td></tr>
                                    <tr><td><code>"sync.toml"</code></td><td>"Universe and open Space "<code>".eustress/"</code></td><td>"The listing id ("<code>"experience_id"</code>")"</td></tr>
                                    <tr><td><code>"publish.toml"</code>", "<code>"publish-journal.toml"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The listing fields and visibility you chose, and publish checkpoints"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // REVIEW
                    // =========================================================
                    <section id="review" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Review"
                        </h2>

                        <div id="review-inputs" class="subsection">
                            <h3>"What Review Reads"</h3>
                            <p>
                                "Review is the gate between an upload and the Gallery. It never runs your
                                Universe: Studio builds the evidence from the live scene while it publishes, and
                                the API judges that."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"The dossier"</strong>": a measured digest of the scene (counts, bounds, hierarchy, how varied its materials and colors are, how much is left at defaults, how many parts duplicate each other), the text people will read, the scripts, the asset names, and any links or contact details found in them."</li>
                                <li><strong>"Views of the scene"</strong>": the off-screen AI camera captures the scene from several angles while the upload runs, framing all of it."</li>
                            </ul>
                        </div>

                        <div id="review-ladder" class="subsection">
                            <h3>"The Review Ladder"</h3>
                            <p>"The API runs the cheapest checks first, and each layer decides how much of the next one runs:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Layer"</th><th>"Reads"</th><th>"Can decide"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"L0 Deterministic"</td><td>"The stored world and the dossier digest"</td><td>"Flag empty or default-only scenes"</td></tr>
                                    <tr><td>"L1 Text classifier (Jev, from TypeSafe)"</td><td>"The dossier text"</td><td>"Quarantine, hold, reject or ask for changes; it never approves"</td></tr>
                                    <tr><td>"L2a Judge (xAI Grok)"</td><td>"The views and a case summary"</td><td>"Approve, reject or hold under the Eustress AI Guardian Policy, version 1.2"</td></tr>
                                    <tr><td>"L2b Agent"</td><td>"The case record and the moderation playbook"</td><td>"Settle gray-band cases through guarded tools, in at most 4 rounds"</td></tr>
                                    <tr><td>"L3 People"</td><td>"Everything"</td><td>"Every hold and quarantine, every legal-lane call, and appeals the agent does not settle"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Two rules are enforced in code rather than in prompts: nothing is listed without
                                a recorded decision, and no model can approve content in a hard category. Those
                                categories are sexual content involving minors, terrorism or extremist promotion,
                                planning of mass-casualty attacks, intimate or sexual imagery of real people
                                shared without consent, and doxxing or targeted harassment. Only a person can clear
                                them."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Deployment status"</strong>
                                    <p>
                                        "As of September 2026 this review gate is built and tested in code but not
                                        yet deployed to the live API. "<a href="#roadmap-review">"Review Goes Live"</a>
                                        " lists the remaining steps."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="review-outcomes" class="subsection">
                            <h3>"Outcomes and Ratings"</h3>
                            <p>"Studio polls the outcome for about 18 seconds and puts it in the summary line of the log:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Status"</th><th>"Meaning"</th><th>"Summary in the log"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"approved"</code></td><td>"Passed review. Listed in the Gallery if Public; otherwise only you can open it."</td><td><em>"listed in the Gallery"</em>" with the rating, or "<em>"approved, private"</em></td></tr>
                                    <tr><td><code>"rejected"</code></td><td>"Not listed, with a suggested edit."</td><td><em>"not listed"</em>" and the edit"</td></tr>
                                    <tr><td><code>"changes_requested"</code></td><td>"Reads as directed at children under 13. Remove off-platform links, contact details, personal-data collection and chance-based or real-money mechanics, or describe it for an older audience."</td><td><em>"changes requested before listing"</em></td></tr>
                                    <tr><td><code>"held"</code></td><td>"Waiting for a person."</td><td><em>"held for human review"</em></td></tr>
                                    <tr><td><code>"quarantined"</code></td><td>"Under legal review. Nobody but an administrator can download it, and publishing from your account pauses until a person releases it."</td><td><em>"under legal review"</em></td></tr>
                                    <tr><td><code>"pending"</code></td><td>"No decision yet."</td><td><em>"review pending"</em></td></tr>
                                </tbody>
                            </table>
                            <p>
                                "An approved listing carries one of four ratings: "<code>"all_ages"</code>", "
                                <code>"teen_13"</code>", "<code>"mature_17"</code>" or "<code>"adult_18"</code>
                                ", with "<code>"teen_13"</code>" when the judge gives none. The API accepts one
                                open appeal at a time for a listing that was rejected, held, quarantined or asked
                                for changes, with 10 to 2,000 characters of explanation. The website has no appeal
                                form yet."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // ON THE WEBSITE
                    // =========================================================
                    <section id="web" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "On the Website"
                        </h2>

                        <div id="web-gallery" class="subsection">
                            <h3>"The Gallery"</h3>
                            <p>
                                "The "<a href="/gallery">"Gallery"</a>" lists a simulation only when two things are
                                true: you marked it Public, and review approved it. The same rule gates the listing
                                page, the world download, the play request and the thumbnail. The Gallery's API
                                returns eligible listings newest first. Its featured shelf holds approved listings
                                that review marked as featured, either through the judge's quality grade or when a
                                reviewer approves, and never one rated "<code>"adult_18"</code>"."
                            </p>
                        </div>

                        <div id="web-listing" class="subsection">
                            <h3>"Listing and Play Pages"</h3>
                            <p>
                                "Each listing has a page at "<code>"eustress.dev/simulation/<id>"</code>" with its
                                name, creator, description and visit count. The API returns a listing that is not
                                approved only to its author and to administrators; anyone else gets the same
                                not-found answer as for an id that does not exist."
                            </p>
                            <p>
                                "The listing page's Play Now button opens a dialog titled "
                                <em>"Eustress Player Required"</em>", with a download link and a Try Again button
                                that opens an "<code>"eustress://play/<id>"</code>" link. "
                                <code>"eustress.dev/play/<id>"</code>" counts a visit (the number the listing
                                shows) and gives the same link, with the command that opens the simulation in
                                the Eustress Player:"
                            </p>
                            <div class="code-block">
                                <pre><code>"eustress-client --sim <id>"</code></pre>
                            </div>
                            <p>
                                "The Player downloads the world's manifest and every chunk it has not cached,
                                checks each chunk against its name, and opens the Space you had open when you
                                published. A published simulation plays solo."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"The Player is not in the installer yet"</strong>
                                    <p>
                                        "The Studio installer registers "<code>"eustress://"</code>" links for
                                        Studio, and it does not include the Player, so the link does not open a
                                        simulation today. The Player builds from source as "
                                        <code>"eustress-client"</code>"."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="web-projects" class="subsection">
                            <h3>"Your Projects"</h3>
                            <p>
                                "The Projects page on eustress.dev lists everything you have published, with a
                                status badge:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Badge"</th><th>"Review states"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Published"</td><td>"Approved and Public, so listed in the Gallery"</td></tr>
                                    <tr><td>"Under Review"</td><td>"Pending, classifying, held, appealed, quarantined or changes requested"</td></tr>
                                    <tr><td>"Draft"</td><td>"Rejected, ready to fix and publish again; a private listing that passed review also shows here"</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // UPDATES
                    // =========================================================
                    <section id="updates" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Updates"
                        </h2>

                        <div id="updates-again" class="subsection">
                            <h3>"Publishing Again"</h3>
                            <p>
                                "Publishing a Universe again updates its listing: same id, same page, and only the
                                chunks that changed go up. The API counts the listing's version up by one each
                                time the world changes. New content goes back to review, and the listing leaves
                                the Gallery until review approves it again, so publish when a version is ready
                                for people to see."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A new listing only when the old one is gone"</strong>
                                    <p>
                                        "When the kept id names a listing that was removed, or one that belongs to
                                        another account (a Universe folder copied from someone else), Studio
                                        creates a new listing and keeps its id instead."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="updates-space" class="subsection">
                            <h3>"Updating One Space"</h3>
                            <p>
                                "Publish Space updates one Space of a published Universe. It bakes only the open
                                Space, swaps it into the world the listing already plays, keeps every other Space
                                exactly as published, and refreshes the shared assets. The listing returns to
                                pending review, because the content that people see has changed."
                            </p>
                            <p>
                                "Publish Space reads the listing id ("<code>"experience_id"</code>") from the
                                Universe's "<code>".eustress/sync.toml"</code>" and stops with "
                                <em>"Publish the Universe first before publishing individual Spaces."</em>" when
                                it is missing. A listing published before chunks existed must be published as a
                                whole Universe once before its Spaces can be updated one at a time."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"07"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-listing" class="subsection">
                            <h3>"Removing a Listing"</h3>
                            <p>
                                "The API has no route to delete or unpublish a listing yet. Chunks a republish
                                stops naming stay in storage until a cleanup job removes them; the Player only
                                ever downloads the chunks the current manifest names."
                            </p>
                        </div>

                        <div id="roadmap-review" class="subsection">
                            <h3>"Review Goes Live"</h3>
                            <p>
                                "Deploying the review gate comes next: setting its classifier key, reviewing older
                                listings that predate the gate (25 per nightly run, oldest first, hidden until
                                then), working the first held cases by hand, and calibrating the thresholds on real
                                publishes. After that, the API will compute its own scene digest from the uploaded
                                package, and perceptual hashes of the review views will block re-uploads of removed
                                content."
                            </p>
                        </div>

                        <div id="roadmap-play" class="subsection">
                            <h3>"Playing Published Worlds"</h3>
                            <p>
                                "The Player opens a published simulation today from its command line. Next: the
                                installer ships the Player and registers "<code>"eustress://play/"</code>" links
                                to it, so Play Now opens the simulation; a browser build plays it on the listing
                                page; and a listing can have hosted sessions that players join together. The
                                multiplayer phases are listed on the "
                                <a href="/docs/networking#roadmap-phases">"Networking"</a>" page."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Publish from Studio today. Play Now in the Player and in the browser comes next."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/website" class="btn-secondary-steel">"Website Service Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/networking" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Networking"</span>
                            </div>
                        </a>
                        <a href="/docs/website" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Website Service"</span>
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
