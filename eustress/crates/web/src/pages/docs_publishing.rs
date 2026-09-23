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
                TocSubsection { id: "upload-package", title: "The Package" },
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
                TocSubsection { id: "roadmap-listing", title: "Updating a Listing" },
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
                aria-label="Studio saves and packages the Universe, the Eustress API creates a listing, Cloudflare R2 stores the package, review reads the dossier and four captured views, and the Gallery shows the listing only after review approves it and it is marked Public.">
                <defs>
                    <marker id="publish-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                <text x="10" y="40" class="dg-title">"Publish Universe"</text>

                <rect x="10" y="70" width="100" height="60" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="60" y="96" class="dg-label" text-anchor="middle">"Studio"</text>
                <text x="60" y="116" class="dg-note" text-anchor="middle">"save, package"</text>

                <line x1="110" y1="100" x2="138" y2="100" class="dg-line" marker-end="url(#publish-arrow)"></line>

                <rect x="140" y="70" width="100" height="60" rx="8" class="dg-box"></rect>
                <text x="190" y="96" class="dg-label" text-anchor="middle">"API"</text>
                <text x="190" y="116" class="dg-note" text-anchor="middle">"new listing"</text>

                <line x1="240" y1="100" x2="268" y2="100" class="dg-line" marker-end="url(#publish-arrow)"></line>

                <rect x="270" y="70" width="100" height="60" rx="8" class="dg-box"></rect>
                <text x="320" y="96" class="dg-label" text-anchor="middle">"R2"</text>
                <text x="320" y="116" class="dg-note" text-anchor="middle">"stores the .pak"</text>

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
                "Studio does the packaging and uploads; the API decides what the Gallery shows."
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
                            "Publishing uploads a Universe from Studio to eustress.dev. Studio packages the
                            Universe folder into one compressed file, creates a listing for it and submits it
                            for review, and the Gallery lists it once review approves it and you have marked it
                            Public."
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
                                api.eustress.dev keeps the listing record and stores the package in Cloudflare R2."
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
                                <li><strong>"An open Space"</strong>": publishing starts from the Space you have open and packages the Universe that contains it."</li>
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
                                    <tr><th>"Command"</th><th>"Shortcut"</th><th>"What it packages"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Publish Universe"</td><td><code>"Ctrl+P"</code></td><td>"The whole Universe folder, every Space in it"</td></tr>
                                    <tr><td>"Publish Space"</td><td><code>"Ctrl+Shift+P"</code></td><td>"Only the open Space, as an update to a published Universe"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The Roblox keymap preset gives "<code>"Ctrl+Shift+P"</code>" to the Properties
                                filter and moves Publish Space to "<code>"Ctrl+Alt+Shift+P"</code>". Use Publish
                                Universe for now: Publish Space depends on a listing id that Studio does not record
                                yet, as "<a href="#updates-space">"Updating One Space"</a>" explains."
                            </p>
                        </div>

                        <div id="studio-fields" class="subsection">
                            <h3>"The Publish Dialog"</h3>
                            <p>
                                "The dialog's left side lists the files that will be packaged, with the open Space
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
                                "Publish saves the Space first, shows "<em>"Publishing Universe... packaging all
                                Spaces and uploading."</em>" and does the rest on a background thread, so you can
                                keep working. Three problems stop it before anything is uploaded:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"No open Space"</strong>": "<em>"Publish requires an open Space folder."</em></li>
                                <li><strong>"No session"</strong>": "<em>"Sign in to publish."</em></li>
                                <li><strong>"A Website reference that does not resolve"</strong>": the message names the reference and the nearest candidates. See "<a href="/docs/website">"Website Service"</a>"."</li>
                            </ul>
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
                            <h3>"The Package"</h3>
                            <p>
                                "A package is a "<code>".pak"</code>" file: a tar archive of the Universe folder,
                                compressed with zstd at level 3. Studio archives the folder as it sits on disk, so
                                the package holds every Space with its files and database, plus the Universe's "
                                <code>".eustress"</code>" metadata. It leaves out:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Development folders"</strong>": "<code>".git"</code>", "<code>"node_modules"</code>" and "<code>"target"</code></li>
                                <li><strong>"Operating system files"</strong>": "<code>".DS_Store"</code>", "<code>"Thumbs.db"</code>" and "<code>"desktop.ini"</code></li>
                                <li><strong>"Temporary files"</strong>": anything ending in "<code>".lock"</code>" or "<code>".tmp"</code></li>
                            </ul>
                            <p>
                                "Studio hashes the finished package with BLAKE3 and sends the hash with the
                                listing as its content id, in the form "<code>"blake3:<hex>"</code>". The API
                                records it, but de-duplicates on its own fingerprint of the stored object, because
                                a hash a client sends is only a claim."
                            </p>
                        </div>

                        <div id="upload-steps" class="subsection">
                            <h3>"Upload and Storage"</h3>
                            <ol class="numbered-list">
                                <li>
                                    <strong>"Create the listing."</strong>
                                    " Studio sends the name, description, genre, Public setting and content id.
                                    The API answers with a new listing id and starts the listing as pending
                                    review. Every listing records 10 as its player limit."
                                </li>
                                <li>
                                    <strong>"Upload the package."</strong>
                                    " Packages under 100 MB go up in one request; larger ones go up in 95 MB
                                    parts. The API stores the file in Cloudflare R2 at "
                                    <code>"universes/<id>/universe.pak"</code>" and refuses a single-request
                                    upload above 500 MB."
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
                                    listing stays pending while the package is already stored."
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
                                    <tr><td><code>".last_publish_hash"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The BLAKE3 hash of the last uploaded package"</td></tr>
                                    <tr><td><code>"moderation-dossier.json"</code></td><td>"Universe "<code>".eustress/"</code></td><td>"The evidence review reads"</td></tr>
                                    <tr><td><code>"capture-0.png"</code>" to "<code>"capture-3.png"</code></td><td>"Universe "<code>".eustress/moderation/"</code></td><td>"The review views"</td></tr>
                                    <tr><td><code>"publish.toml"</code>", "<code>"publish-journal.toml"</code>", "<code>"sync.toml"</code></td><td>"Space "<code>".eustress/"</code></td><td>"The listing fields and visibility you chose, and publish checkpoints"</td></tr>
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
                                    <tr><td>"L0 Deterministic"</td><td>"The stored package and the dossier digest"</td><td>"Reuse the decision already made on an identical package; flag empty or default-only scenes"</td></tr>
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
                                page, the package download, the play request and the thumbnail. The Gallery's API
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
                                that opens an "<code>"eustress://play/<id>"</code>" link. No Eustress program
                                registers that link yet, so it does not open the simulation. "
                                <code>"eustress.dev/play/<id>"</code>" counts a visit (the number the listing
                                shows) and asks the API for a running server. No server registers itself today, so
                                that page reports that no server is available."
                            </p>
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
                                "Every Publish Universe creates a new listing with a new id, and review judges it
                                from the start. The earlier listing stays where it is, and the API numbers every
                                listing version 1."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"Publishing again creates a second listing"</strong>
                                    <p>
                                        "Studio does not keep the listing id after a publish, so it cannot update
                                        the listing it made last time, and the API has no route to delete or
                                        unpublish a listing. Publish when a version is ready for people to see,
                                        rather than after every change."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="updates-space" class="subsection">
                            <h3>"Updating One Space"</h3>
                            <p>
                                "Publish Space is built to update one Space of a published Universe: it packages
                                only the open Space's folder, uploads it to "
                                <code>"universes/<id>/spaces/<name>.pak"</code>" beside the Universe package, and
                                returns the listing to pending review, because the content that people see has
                                changed."
                            </p>
                            <p>
                                "Before it opens the dialog, Publish Space looks for the listing id ("
                                <code>"experience_id"</code>") in the Universe's "<code>".eustress/sync.toml"</code>
                                " and stops with "<em>"Publish the Universe first before publishing individual
                                Spaces."</em>" when it is missing. Studio does not write that id after a Universe
                                publish yet, so the check stops every Space publish today."
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
                            <h3>"Updating a Listing"</h3>
                            <p>
                                "The storage plan will keep the listing id after a publish, so the next Publish
                                updates the same listing instead of creating another, and it will move the publish
                                manifests to the Universe root. The same work will regenerate files from the
                                WorldDb at publish time and replace the exclusion list with an allowlist of what a
                                package may contain. Its test: republishing an unchanged Space will produce a
                                byte-identical package."
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
                                "Opening a published simulation from the website is the last phase of the
                                multiplayer plan: play links that open the Player, servers that register their
                                address, and single-use join tokens. The phases are listed on the "
                                <a href="/docs/networking#roadmap-phases">"Networking"</a>" page."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Publish from Studio today. Updates in place and playable listings come next."</strong></p>
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
