// =============================================================================
// Eustress Web - Earning Documentation Page
// =============================================================================
// Earning: how Studio work becomes Bliss (BLS) through the witness, the daily
// emission and USD treasury drip, identity verification, and Tickets (TKT).
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
                TocSubsection { id: "overview-how", title: "How Earning Works" },
                TocSubsection { id: "overview-currencies", title: "BLS and Tickets" },
                TocSubsection { id: "overview-status", title: "What Runs Today" },
            ],
        },
        TocSection {
            id: "studio",
            title: "Studio Setup",
            subsections: vec![
                TocSubsection { id: "studio-signin", title: "Signing In" },
                TocSubsection { id: "studio-badge", title: "The Bliss Badge" },
                TocSubsection { id: "studio-nodes", title: "Light and Full Nodes" },
                TocSubsection { id: "studio-optout", title: "Turning Earning Off" },
            ],
        },
        TocSection {
            id: "contributions",
            title: "Earning BLS",
            subsections: vec![
                TocSubsection { id: "contributions-tracking", title: "What Studio Measures" },
                TocSubsection { id: "contributions-score", title: "Scores and Weights" },
                TocSubsection { id: "contributions-limits", title: "Limits the Witness Enforces" },
            ],
        },
        TocSection {
            id: "distribution",
            title: "Daily Distribution",
            subsections: vec![
                TocSubsection { id: "distribution-emission", title: "Emission Schedule" },
                TocSubsection { id: "distribution-gate", title: "The Effort Gate" },
                TocSubsection { id: "distribution-share", title: "Your Share" },
                TocSubsection { id: "distribution-ledger", title: "The Public Ledger" },
            ],
        },
        TocSection {
            id: "payouts",
            title: "USD Payouts",
            subsections: vec![
                TocSubsection { id: "payouts-treasury", title: "The Treasury" },
                TocSubsection { id: "payouts-drip", title: "The Daily Drip" },
                TocSubsection { id: "payouts-identity", title: "Identity Verification" },
                TocSubsection { id: "payouts-connect", title: "Connecting Stripe" },
            ],
        },
        TocSection {
            id: "tickets",
            title: "Tickets",
            subsections: vec![
                TocSubsection { id: "tickets-packages", title: "Packages" },
                TocSubsection { id: "tickets-split", title: "Where the Money Goes" },
                TocSubsection { id: "tickets-sales", title: "Selling for Tickets" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-attestation", title: "Proof of Work" },
                TocSubsection { id: "roadmap-marketplace", title: "Marketplace and Spending" },
                TocSubsection { id: "roadmap-chain", title: "The Bliss Chain" },
            ],
        },
    ]
}

/// One day of earning: Studio reports work, the witness scores it, and the
/// midnight run cuts a BLS share and a USD share from the same day score.
#[component]
fn EarningFlowDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 230" role="img"
                aria-label="Studio sends work to the witness every 5 minutes. The witness adds co-signed scores to your day score. At 00:00 UTC the day score sets your share of the BLS emission and your share of the treasury's USD drip.">
                <defs>
                    <marker id="earn-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                <rect x="10" y="86" width="112" height="56" rx="8" class="dg-box"></rect>
                <text x="66" y="110" class="dg-label" text-anchor="middle">"Studio"</text>
                <text x="66" y="128" class="dg-note" text-anchor="middle">"time, by type"</text>

                <line x1="122" y1="114" x2="180" y2="114" class="dg-line" marker-end="url(#earn-arrow)"></line>
                <text x="151" y="104" class="dg-note" text-anchor="middle">"every 5 min"</text>

                <rect x="180" y="86" width="112" height="56" rx="8" class="dg-box"></rect>
                <text x="236" y="110" class="dg-label" text-anchor="middle">"Witness"</text>
                <text x="236" y="128" class="dg-note" text-anchor="middle">"weights, limits"</text>

                <line x1="292" y1="114" x2="348" y2="114" class="dg-line" marker-end="url(#earn-arrow)"></line>
                <text x="320" y="104" class="dg-note" text-anchor="middle">"score"</text>

                <rect x="348" y="86" width="104" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="400" y="110" class="dg-label" text-anchor="middle">"Day score"</text>
                <text x="400" y="128" class="dg-note" text-anchor="middle">"per UTC day"</text>

                <line x1="452" y1="104" x2="520" y2="62" class="dg-line-accent" marker-end="url(#earn-arrow)"></line>
                <line x1="452" y1="124" x2="520" y2="170" class="dg-line-accent" marker-end="url(#earn-arrow)"></line>
                <text x="484" y="119" class="dg-note" text-anchor="middle">"00:00 UTC"</text>

                <rect x="520" y="30" width="112" height="56" rx="8" class="dg-box dg-box-violet"></rect>
                <text x="576" y="54" class="dg-label" text-anchor="middle">"BLS"</text>
                <text x="576" y="72" class="dg-note" text-anchor="middle">"share of emission"</text>

                <rect x="520" y="146" width="112" height="56" rx="8" class="dg-box"></rect>
                <text x="576" y="170" class="dg-label" text-anchor="middle">"USD"</text>
                <text x="576" y="188" class="dg-note" text-anchor="middle">"share of the drip"</text>
            </svg>
            <figcaption>
                "Studio reports work and the witness scores it. At midnight UTC one day score
                sets two separate shares: BLS from the day's emission and dollars from the
                treasury's drip."
            </figcaption>
        </figure>
    }
}

/// Earning documentation page.
#[component]
pub fn DocsEarningPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-earning"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/bliss.svg" alt="Earning" class="toc-icon" />
                        <h2>"Earning"</h2>
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
                            <span class="current">"Earning"</span>
                        </div>
                        <h1 class="docs-title">"Earning"</h1>
                        <p class="docs-subtitle">
                            "Bliss (BLS) is the contribution ledger behind earning on Eustress. Studio
                            reports the time you spend building, a witness service scores it, and at UTC
                            midnight your day's score earns a share of that day's BLS and, once you have
                            verified your identity and connected Stripe, a share of the treasury's USD drip."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "17 min read"
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

                        <div id="overview-how" class="subsection">
                            <h3>"How Earning Works"</h3>
                            <p>
                                "Earning has three moving parts. Studio measures the time you spend building
                                and sorts it into contribution types. The witness, a Cloudflare Worker at "
                                <code>"api.eustress.dev"</code>", checks each submission, weights it and adds
                                it to your score for the current UTC day. At 00:00 UTC a scheduled run closes
                                the day that just ended: it credits BLS in proportion to each contributor's
                                score, then pays out a slice of the USD treasury by the same score."
                            </p>
                            <EarningFlowDiagram />
                            <p>
                                "The two payouts are separate flows cut from one score. The dollars you receive
                                are your share of that day's treasury drip, not a conversion of your BLS, and a
                                USD payout leaves your BLS balance untouched."
                            </p>
                        </div>

                        <div id="overview-currencies" class="subsection">
                            <h3>"BLS and Tickets"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th></th><th>"Bliss (BLS)"</th><th>"Tickets (TKT)"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"How you get it"</td><td>"Earned from your daily contribution score"</td><td>"Bought with USD at "<a href="/tickets">"/tickets"</a></td></tr>
                                    <tr><td>"Precision"</td><td>"2 decimals (1 BLS = 100 minor units)"</td><td>"Whole Tickets"</td></tr>
                                    <tr><td>"Where it is kept"</td><td>"An append-only ledger the witness publishes"</td><td>"A balance on your account"</td></tr>
                                    <tr><td>"Becomes dollars?"</td><td>"No. Dollars come from the treasury drip"</td><td>"No"</td></tr>
                                </tbody>
                            </table>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Two currencies, kept apart"</strong>
                                    <p>
                                        "BLS is earned; Tickets are bought, or received from a sale. The witness has
                                        no exchange between them, and neither one converts to dollars. Dollars reach
                                        contributors one way: the treasury's daily drip."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-status" class="subsection">
                            <h3>"What Runs Today"</h3>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part"</th><th>"Today"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Contribution tracking in Studio"</td><td>"Built, on by default"</td></tr>
                                    <tr><td>"Scoring, limits and co-signing"</td><td>"Built, in the witness"</td></tr>
                                    <tr><td>"Daily BLS emission and the public ledger"</td><td>"Built, runs at 00:00 UTC"</td></tr>
                                    <tr><td>"USD drip to Stripe Connect"</td><td>"Built, needs identity verification and a connected account"</td></tr>
                                    <tr><td>"Buying Tickets, funding the treasury"</td><td>"Built"</td></tr>
                                    <tr><td>"Spending Tickets in a marketplace"</td><td>"Sale rule built in the witness, no listings yet"</td></tr>
                                    <tr><td>"Spending BLS"</td><td>"Burn endpoint built, nothing spends BLS yet"</td></tr>
                                    <tr><td>"Full node duties, the Bliss chain"</td><td>"Designed, not built"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The witness is a single service run by Eustress, and its ledger is off-chain: a
                                balance is a record the witness keeps and publishes, not a token on a blockchain."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // STUDIO SETUP
                    // =========================================================
                    <section id="studio" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "Studio Setup"
                        </h2>

                        <div id="studio-signin" class="subsection">
                            <h3>"Signing In"</h3>
                            <p>
                                "Earning needs a session with the witness. Sign in with the identity file you
                                downloaded when you registered on eustress.dev ("
                                <code>"eustress-<username>.toml"</code>"). Studio reads its "
                                <code>"public_key"</code>" and "<code>"private_key"</code>", signs a challenge
                                from the witness with the Ed25519 key and receives a session token. At launch,
                                Studio signs in again with your most recent identity."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"An identity file without its private key cannot earn"</strong>
                                    <p>
                                        "Studio still signs you in locally, but it cannot answer the witness's
                                        challenge, so it submits nothing and the badge says so. Load the complete
                                        file registration downloaded. The session token itself lasts 72 hours and
                                        Studio does not renew it while running, so after three days in one session,
                                        open your identity again or restart Studio. Time you worked meanwhile stays
                                        in the local file and is sent after you sign in."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="studio-badge" class="subsection">
                            <h3>"The Bliss Badge"</h3>
                            <p>
                                "The Bliss badge sits beside your account badge in the ribbon and shows your
                                balance to 2 decimals. Click it for the balance, a "<strong>"Pending"</strong>
                                " line, the node mode and your bonus multiplier. While you are earning, Pending
                                reads "<code>"+N pts today"</code>": the score the witness has credited for the
                                current UTC day plus Studio's estimate for work it has not sent yet. When earning
                                is blocked, the Pending line says why:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Pending line begins"</th><th>"Meaning"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><em>"Sign in to earn BLS"</em></td><td>"No identity is signed in."</td></tr>
                                    <tr><td><em>"Identity not verified"</em></td><td>"Studio has no session from the witness: the file lacks a private key, the witness was unreachable at sign-in, or it rejected the identity."</td></tr>
                                    <tr><td><em>"Not syncing"</em></td><td>"Three submissions in a row failed for a reason other than sign-in. Work is saved locally and retried."</td></tr>
                                    <tr><td><em>"Bliss disabled"</em></td><td>"Earning is turned off in settings."</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Each credited submission logs a line in Output, such as "
                                <code>"Bliss: +15.0 pts co-signed (Development)"</code>". Unsent time is saved
                                to "<code>"~/.eustress_engine/bliss_tracker.toml"</code>" every 30 seconds and on
                                exit. The balance in that file is only a display cache that the next heartbeat
                                overwrites; the ledger itself lives with the witness."
                            </p>
                        </div>

                        <div id="studio-nodes" class="subsection">
                            <h3>"Light and Full Nodes"</h3>
                            <p>
                                "The badge dropdown offers two node modes under "<strong>"NODE MODE"</strong>
                                ". "<strong>"Light Node"</strong>" is the default, at 1.0x. "
                                <strong>"Full Node"</strong>" multiplies the score of every co-signed
                                submission by 1.1. Studio saves the choice as "<code>"bliss_node_mode"</code>
                                " in "<code>"~/.eustress_engine/settings.json"</code>" and restores it at launch."
                            </p>
                            <p>
                                "The witness never takes the mode from a submission. It uses the mode Studio last
                                reported in its heartbeat, which Studio sends every 90 seconds, so a switch counts
                                from the next heartbeat. When Bliss is enabled, Studio also starts a small local
                                node service on port 7777 that answers health and identity checks. Earning does
                                not route through it: the tracker talks to the witness directly."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"What Full does today"</strong>
                                    <p>
                                        "The Full option describes chain storage and block production. Those duties
                                        belong to the Bliss chain, which is designed but not built, so today both
                                        modes run the same local service and differ only in the multiplier."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="studio-optout" class="subsection">
                            <h3>"Turning Earning Off"</h3>
                            <p>
                                "Earning is on by default. To turn it off, close Studio, set "
                                <code>"bliss_enabled"</code>" to "<code>"false"</code>" in the settings file and
                                start Studio again. Studio then skips the local node, submits no work and the
                                badge reads "<em>"Bliss disabled"</em>". It still tallies active seconds in its
                                local file, and sends them if you turn earning back on."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"~/.eustress_engine/settings.json (excerpt)"</span>
                                </div>
                                <pre><code class="language-json">{r#"{
  "bliss_enabled": false,
  "bliss_node_mode": "Light"
}"#}</code></pre>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // EARNING BLS
                    // =========================================================
                    <section id="contributions" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Earning BLS"
                        </h2>

                        <div id="contributions-tracking" class="subsection">
                            <h3>"What Studio Measures"</h3>
                            <p>
                                "Studio counts a moment as work only when its window has focus and you pressed a
                                key, clicked, scrolled or moved the mouse in the last 60 seconds. It files each
                                such moment under one contribution type, taking the most valuable signal first:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Signal in the last 120 seconds"</th><th>"Type"</th><th>"Weight"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"You edited text in a script tab"</td><td><code>"Development"</code></td><td>"3.0"</td></tr>
                                    <tr><td>"You made a change that lands on the undo stack"</td><td><code>"Creation"</code></td><td>"2.5"</td></tr>
                                    <tr><td>"Neither: navigating, inspecting, testing"</td><td><code>"ActiveTime"</code></td><td>"1.0"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Undo and redo are not new work; only a new undoable change is. Every 5 minutes
                                Studio submits what it has gathered, in chunks of at least 60 seconds and at most
                                one hour, up to 6 chunks at a time, so an offline backlog drains steadily after
                                you reconnect."
                            </p>
                        </div>

                        <div id="contributions-score" class="subsection">
                            <h3>"Scores and Weights"</h3>
                            <p>"The witness turns each submission into a score measured in weighted minutes:"</p>
                            <div class="equation-card">
                                <div class="equation">"score = weight x node bonus x minutes"</div>
                                <div class="equation-label">"Node bonus: Light 1.0, Full 1.1"</div>
                            </div>
                            <p>
                                "An hour of scripting on a Light node scores 3.0 x 1.0 x 60 = 180. The weights
                                come from the witness's own table, which lists more types than Studio sends. A
                                type outside the table is rejected."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Type"</th><th>"Weight"</th><th>"Sent by Studio"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"Development"</code></td><td>"3.0"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"Creation"</code></td><td>"2.5"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"Education"</code></td><td>"2.2"</td><td>"No"</td></tr>
                                    <tr><td><code>"Collaboration"</code></td><td>"2.0"</td><td>"No"</td></tr>
                                    <tr><td><code>"Optimization"</code></td><td>"2.0"</td><td>"No"</td></tr>
                                    <tr><td><code>"QualityAssurance"</code></td><td>"1.8"</td><td>"No"</td></tr>
                                    <tr><td><code>"Moderation"</code></td><td>"1.5"</td><td>"No"</td></tr>
                                    <tr><td><code>"Documentation"</code></td><td>"1.5"</td><td>"No"</td></tr>
                                    <tr><td><code>"ActiveTime"</code></td><td>"1.0"</td><td>"Yes"</td></tr>
                                    <tr><td><code>"Custom"</code></td><td>"1.0"</td><td>"No"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Sales add a second kind of score, value score, covered under "
                                <a href="#tickets-sales">"Selling for Tickets"</a>"."
                            </p>
                        </div>

                        <div id="contributions-limits" class="subsection">
                            <h3>"Limits the Witness Enforces"</h3>
                            <p>
                                "A submission is a claim, and the witness is the only trust boundary. Whatever a
                                client sends, it bounds what any one account can earn:"
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Known types only."</strong>" A type outside the weight table is rejected."</li>
                                <li><strong>"One hour per submission."</strong>" Duration is clamped to 1 to 3,600 seconds."</li>
                                <li><strong>"120 submissions an hour"</strong>" per account."</li>
                                <li><strong>"Each piece of work once."</strong>" Studio hashes your account, the day, the type, the duration and a chunk counter into every submission, and the witness refuses a hash it has seen in the last 2 days."</li>
                                <li><strong>"ActiveTime needs presence."</strong>" Credited ActiveTime cannot exceed the time the witness saw you online through heartbeats, and one heartbeat adds at most 150 seconds of presence."</li>
                                <li><strong>"A daily ceiling."</strong>" Effort score stops at 3,200 per account per UTC day. Sixteen hours of Development on a Full node comes to 3,168."</li>
                                <li><strong>"The bonus is observed."</strong>" The 1.1x comes from the heartbeat mode, never from the submission."</li>
                            </ul>
                            <p>
                                "When a limit trims a submission to nothing, Studio puts that time back in its
                                local tally and offers it again at a later flush."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A co-signature is a receipt"</strong>
                                    <p>
                                        "The witness stamps each accepted submission with a SHA-256 digest of your
                                        account, the contribution hash and the time. It records that the witness
                                        accepted the submission under the rules above, which cap what any account
                                        can earn in a day."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // =========================================================
                    // DAILY DISTRIBUTION
                    // =========================================================
                    <section id="distribution" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Daily Distribution"
                        </h2>

                        <div id="distribution-emission" class="subsection">
                            <h3>"Emission Schedule"</h3>
                            <p>
                                "BLS started from a supply of 100,000,000. Each year the supply grows by an
                                emission rate that halves every 4 years and never falls below 0.5%."
                            </p>
                            <div class="stats-grid">
                                <div class="stat-card">
                                    <div class="stat-value">"100,000,000"</div>
                                    <div class="stat-label">"Initial supply"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"5%"</div>
                                    <div class="stat-label">"First-year rate"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"4 years"</div>
                                    <div class="stat-label">"Halving period"</div>
                                </div>
                                <div class="stat-card">
                                    <div class="stat-value">"0.5%"</div>
                                    <div class="stat-label">"Floor, forever"</div>
                                </div>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Whole years since genesis"</th><th>"Annual rate"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"0 to 3"</td><td>"5%"</td></tr>
                                    <tr><td>"4 to 7"</td><td>"2.5%"</td></tr>
                                    <tr><td>"8 to 11"</td><td>"1.25%"</td></tr>
                                    <tr><td>"12 to 15"</td><td>"0.625%"</td></tr>
                                    <tr><td>"16 and later"</td><td>"0.5% (the floor)"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Genesis is the date of the first distribution, and a year counts only when 365
                                whole days have passed. A day's emission ceiling is the current supply times the
                                annual rate, divided by 365. At the genesis supply that is 100,000,000 x 5% / 365,
                                about 13,698.63 BLS. Supply grows by exactly what each day mints, so the ceiling
                                creeps up with it."
                            </p>
                        </div>

                        <div id="distribution-gate" class="subsection">
                            <h3>"The Effort Gate"</h3>
                            <p>"The ceiling is a maximum, not a promise. A day mints its ceiling scaled by how much work the network did:"</p>
                            <div class="equation-card">
                                <div class="equation">"minted = ceiling x min(1, total day score / 1,440)"</div>
                                <div class="equation-label">"1,440 is 8 hours of Development: 8 x 60 x 3.0"</div>
                            </div>
                            <p>
                                "A day whose contributors scored 1,440 or more in total mints the whole ceiling.
                                A day at 720 mints half, and the other half is never created. Supply tracks the
                                work that happened rather than the calendar, while each contributor's share of what
                                is minted stays proportional to score."
                            </p>
                        </div>

                        <div id="distribution-share" class="subsection">
                            <h3>"Your Share"</h3>
                            <p>
                                "After midnight UTC the witness reads every contributor's score for the day that
                                ended: effort score from co-signing plus any value score from sales. Your credit
                                is your fraction of the minted pool, rounded down to a hundredth of a BLS:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Worked example, at the genesis supply"</span>
                                </div>
                                <pre><code class="language-text">{r#"ceiling       13,698.63 BLS     100,000,000 x 5% / 365
day score     720               all contributors together
utilization   720 / 1,440     = 0.5
pool          6,849.31 BLS
your score    180               one hour of Development, Light node
your credit   180 / 720       = 0.25 of the pool  ->  1,712.32 BLS"#}</code></pre>
                            </div>
                            <p>
                                "Rounding every credit down keeps the total inside the pool, and the leftover
                                fractions are never minted. Banned accounts receive nothing. Each day is recorded
                                once, so a rerun of the job credits nobody twice."
                            </p>
                            <div class="callout callout-tip">
                                <img src="/assets/icons/sparkles.svg" alt="Tip" />
                                <div>
                                    <strong>"Credit lands after midnight UTC"</strong>
                                    <p>
                                        "Work you do today is scored today and credited by the run at 00:00 UTC that
                                        closes the day. Until then it shows as Pending on the badge."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="distribution-ledger" class="subsection">
                            <h3>"The Public Ledger"</h3>
                            <p>
                                "Balances are an append-only ledger of integer entries in minor units (1 BLS =
                                100), and a balance is the sum of its entries. After each nightly run the witness
                                snapshots the whole ledger to storage. Three read endpoints need no sign-in and
                                answer any origin, so anyone can re-derive a day's emission by hand:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"GET on api.eustress.dev"</th><th>"Returns"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"/api/ledger/summary"</code></td><td>"Supply, total distributed and burned, circulating BLS, the treasury balance, the emission model and the last 60 distributions"</td></tr>
                                    <tr><td><code>"/api/ledger/distribution/{date}"</code></td><td>"One day's record: ceiling, total score, utilization, and each recipient's score and credit, by account id"</td></tr>
                                    <tr><td><code>"/api/ledger/history/{account id}"</code></td><td>"One account's balance, day by day"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Signed in on the website, "<a href="/bliss/history">"/bliss/history"</a>" shows
                                your own Wallet Ledger: balance, credited days, the average per credited day, a
                                cumulative chart and a row for each credited day. "<a href="/bliss">"/bliss"</a>" shows the
                                network's live figures, including the treasury balance, supply and the day's
                                emission ceiling, refreshed every 20 seconds."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // USD PAYOUTS
                    // =========================================================
                    <section id="payouts" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "USD Payouts"
                        </h2>

                        <div id="payouts-treasury" class="subsection">
                            <h3>"The Treasury"</h3>
                            <p>"The treasury is a USD balance the witness keeps for contributors. Two things add to it:"</p>
                            <ul class="docs-list">
                                <li><strong>"Ticket sales."</strong>" Half of each purchase's net revenue, after the payment channel's fee (see "<a href="#tickets-split">"Where the Money Goes"</a>")."</li>
                                <li><strong>"Direct funding."</strong>" Anyone can fund it from the Fund the Treasury section of "<a href="/bliss">"/bliss"</a>", one-time or monthly. The witness adds the full amount of each completed checkout; for a monthly subscription that is the first payment, because the witness does not record renewals yet."</li>
                            </ul>
                            <p>
                                "Only the daily payout takes money out, and only by what Stripe actually
                                transferred. The dollars sit in Eustress's Stripe account until a transfer pays
                                them out. Funding the treasury is a gift to the contributor pool: it earns the
                                funder no BLS and no Tickets."
                            </p>
                        </div>

                        <div id="payouts-drip" class="subsection">
                            <h3>"The Daily Drip"</h3>
                            <p>"After the BLS distribution, the same midnight run pays out a fixed fraction of the treasury:"</p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Mode"</th><th>"When"</th><th>"Daily drip"</th><th>"Split"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Normal"</td><td>"Treasury above 15% of its high-water mark"</td><td>"0.276% of the treasury"</td><td>"By score"</td></tr>
                                    <tr><td>"Scarcity"</td><td>"Treasury at or below 15% of its high-water mark"</td><td>"0.136% of the treasury"</td><td>"By score, with the top 25% of contributors counted double"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "The high-water mark is the treasury's highest balance. Deposits raise it, and
                                after each payout run it decays 0.171% a day toward the current balance, so one
                                large deposit cannot hold the system in scarcity forever. A $10,000 treasury drips
                                $27.60 in normal mode. With no new deposits and every day's drip paid out, a
                                balance halves in about 251 days."
                            </p>
                            <p>
                                "The drip is divided by the same day score as BLS, among contributors who have a
                                connected Stripe account and are not banned. Without a connected account your
                                score still earns BLS but takes no part in that day's split. Each transfer is
                                rounded down to the cent, one under $0.50 is skipped and stays in the treasury,
                                and a day whose whole drip is under $0.50 pays nothing. Every transfer carries an
                                idempotency key built from the date and your account, so a retried run cannot
                                pay you twice."
                            </p>
                        </div>

                        <div id="payouts-identity" class="subsection">
                            <h3>"Identity Verification"</h3>
                            <p>
                                "Payouts go to verified adults. Registering on eustress.dev includes identity
                                verification as step 2 of 3, and the witness checks that record again before it
                                creates a payout account."
                            </p>
                            <ol class="numbered-list">
                                <li>"Photograph the front of a government photo ID, and the back when it has one. JPEG, PNG, WebP and PDF files up to 12 MB are accepted, and the witness checks that each file's bytes match its type."</li>
                                <li>"On a desktop without a good camera, scan the QR code and finish on your phone at "<a href="/verify">"/verify"</a>" within 30 minutes. Your name and date of birth stay on the server and never travel through the code."</li>
                                <li>"The witness sends the images to xAI's Grok model, which checks the document, reads your name and date of birth and screens the application in one call. If that check cannot run, the application is rejected and flagged for manual review rather than approved."</li>
                            </ol>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"Age is read from the document"</strong>
                                    <p>
                                        "The minimum age is 18, raised where the local age of majority is higher:
                                        19 in Canada and South Korea, 20 in Thailand, and 21 in Singapore, Indonesia,
                                        the United Arab Emirates and Egypt. The date of birth on the document
                                        decides; if it cannot be read, verification stops rather than falling back to
                                        the date you typed. An account the payout gate finds under age is told the
                                        date payouts unlock."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="payouts-connect" class="subsection">
                            <h3>"Connecting Stripe"</h3>
                            <p>
                                "Signed in on eustress.dev, open "<a href="/bliss">"/bliss"</a>" and choose "
                                <strong>"Connect Bank Account"</strong>". The witness checks your verification and
                                age, then creates a Stripe Connect Custom account in the country recorded when you
                                verified, with the date of birth read from your document. It passes your ID images
                                to Stripe and sends you to Stripe's hosted onboarding to add what Stripe needs to
                                pay you, such as a bank account. When you finish, Stripe returns you to /bliss."
                            </p>
                            <p>
                                "Your account joins the split at the next midnight run. A transfer Stripe refuses
                                is skipped, and that amount stays in the treasury."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // TICKETS
                    // =========================================================
                    <section id="tickets" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"06"</span>
                            "Tickets"
                        </h2>

                        <div id="tickets-packages" class="subsection">
                            <h3>"Packages"</h3>
                            <p>
                                "Tickets (TKT) are bought at "<a href="/tickets">"/tickets"</a>" through Stripe
                                Checkout, in five packages. Larger packages add bonus Tickets:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Package"</th><th>"Price"</th><th>"Tickets"</th><th>"Bonus"</th><th>"Total"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Starter"</td><td>"$4.99"</td><td>"400"</td><td>"0"</td><td>"400"</td></tr>
                                    <tr><td>"Standard"</td><td>"$9.99"</td><td>"800"</td><td>"80"</td><td>"880"</td></tr>
                                    <tr><td>"Mega"</td><td>"$19.99"</td><td>"1,600"</td><td>"240"</td><td>"1,840"</td></tr>
                                    <tr><td>"Super"</td><td>"$49.99"</td><td>"4,000"</td><td>"1,000"</td><td>"5,000"</td></tr>
                                    <tr><td>"Ultra"</td><td>"$99.99"</td><td>"8,000"</td><td>"2,800"</td><td>"10,800"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Tickets are credited when Stripe confirms the payment through a signed webhook.
                                The witness refuses a webhook whose signature does not verify or is more than 5
                                minutes old, and processes each checkout session once."
                            </p>
                        </div>

                        <div id="tickets-split" class="subsection">
                            <h3>"Where the Money Goes"</h3>
                            <p>
                                "The witness takes the payment channel's fee off the top, then splits the rest
                                evenly between the treasury and the platform:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Channel"</th><th>"Fee taken first"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Web (Stripe)"</td><td>"2.9% + $0.30"</td></tr>
                                    <tr><td>"iOS, Android, Steam"</td><td>"30%"</td></tr>
                                </tbody>
                            </table>
                            <p>"Every purchase today goes through web checkout. On the Standard package:"</p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"$9.99 Standard package, web"</span>
                                </div>
                                <pre><code class="language-text">{r#"price             $9.99
channel fee       $9.99 x 2.9% + $0.30   =  $0.59
net               $9.99 - $0.59          =  $9.40
to the treasury   50% of net             =  $4.70
to the platform   50% of net             =  $4.70"#}</code></pre>
                            </div>
                        </div>

                        <div id="tickets-sales" class="subsection">
                            <h3>"Selling for Tickets"</h3>
                            <p>
                                "The witness already carries the rule a sale follows. When a buyer spends Tickets
                                on a creator's product, the creator receives 70% of the price, rounded down to a
                                whole Ticket, and the platform keeps the rest. The creator also earns value score:
                                0.5 per Ticket received, added to that day's score with no daily ceiling. A sale of
                                1,000 Tickets gives the creator 700 Tickets and 350 value score, which counts
                                toward both the day's BLS and its USD drip."
                            </p>
                            <div class="callout callout-info">
                                <img src="/assets/icons/help.svg" alt="Info" />
                                <div>
                                    <strong>"No storefront uses it yet"</strong>
                                    <p>
                                        "The marketplace API returns no listings, so "<a href="/marketplace">"/marketplace"</a>
                                        " has nothing for sale and Tickets bought today stay in your balance.
                                        Tickets convert to neither BLS nor dollars."
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
                            <span class="section-number">"07"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-attestation" class="subsection">
                            <h3>"Proof of Work"</h3>
                            <p>
                                "Contributions will be anchored to artifacts the witness can check for itself,
                                such as a published Space or a pushed commit, so Development and Creation scores
                                rest on the work they describe."
                            </p>
                        </div>

                        <div id="roadmap-marketplace" class="subsection">
                            <h3>"Marketplace and Spending"</h3>
                            <p>
                                "Listings will arrive at /marketplace and sell for Tickets under the 70/30 rule
                                above, with value score for the creator. Inside experiences, "
                                <code>"MarketplaceService"</code>" will connect to the same catalog; in Luau and
                                Rune it is a placeholder today, where "<code>"PromptPurchase"</code>" opens nothing
                                and "<code>"PlayerOwnsGamePass"</code>" returns false. Features that spend BLS will
                                use the witness's spend endpoint, which burns BLS rather than moving it, so the
                                emission schedule stays the only source of new BLS."
                            </p>
                        </div>

                        <div id="roadmap-chain" class="subsection">
                            <h3>"The Bliss Chain"</h3>
                            <p>
                                "The decentralization plan moves the ledger onto the Bliss chain, run by
                                independent nodes. Today's emission and treasury rules will become the chain's mint
                                rule, BLS will gain peer-to-peer transfers, nodes that store published Spaces will
                                earn from a storage contribution type, and Full nodes will take on real duties."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Build in Studio. The witness keeps the count."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/bliss" class="btn-secondary-steel">"Open Bliss"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/website" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Website Service"</span>
                            </div>
                        </a>
                        <a href="/learn/mcp" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"MCP Server"</span>
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
