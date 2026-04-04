// =============================================================================
// Eustress Web - Earning Documentation Page
// =============================================================================
// The economics of Eustress: Bliss (BLS) proof-of-contribution currency,
// Tickets (TKT) purchasable currency, treasury mechanics, USD payouts,
// and the marketplace.
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
                TocSubsection { id: "overview-currencies", title: "Two Currencies" },
                TocSubsection { id: "overview-flow", title: "Money Flow" },
                TocSubsection { id: "overview-principles", title: "Economic Principles" },
            ],
        },
        TocSection {
            id: "bliss",
            title: "Bliss (BLS)",
            subsections: vec![
                TocSubsection { id: "bliss-what", title: "What Is Bliss" },
                TocSubsection { id: "bliss-supply", title: "Supply & Emission" },
                TocSubsection { id: "bliss-distribution", title: "Daily Distribution" },
            ],
        },
        TocSection {
            id: "contributions",
            title: "Contribution Types",
            subsections: vec![
                TocSubsection { id: "contributions-weights", title: "Weight Table" },
                TocSubsection { id: "contributions-scoring", title: "Scoring Formula" },
                TocSubsection { id: "contributions-examples", title: "Examples" },
            ],
        },
        TocSection {
            id: "earning-bls",
            title: "Earning BLS",
            subsections: vec![
                TocSubsection { id: "earning-bls-witness", title: "Witness Co-Signing" },
                TocSubsection { id: "earning-bls-nodes", title: "Node Bonuses" },
                TocSubsection { id: "earning-bls-payouts", title: "Payout Cycle" },
            ],
        },
        TocSection {
            id: "tickets",
            title: "Tickets (TKT)",
            subsections: vec![
                TocSubsection { id: "tickets-packages", title: "Packages" },
                TocSubsection { id: "tickets-revenue", title: "Revenue Split" },
                TocSubsection { id: "tickets-usage", title: "Usage" },
            ],
        },
        TocSection {
            id: "treasury",
            title: "Treasury",
            subsections: vec![
                TocSubsection { id: "treasury-funding", title: "How It Grows" },
                TocSubsection { id: "treasury-drip", title: "Daily Drip" },
                TocSubsection { id: "treasury-guarantees", title: "Guarantees" },
            ],
        },
        TocSection {
            id: "usd-payouts",
            title: "USD Payouts",
            subsections: vec![
                TocSubsection { id: "usd-payouts-stripe", title: "Stripe Connect" },
                TocSubsection { id: "usd-payouts-kyc", title: "KYC Verification" },
                TocSubsection { id: "usd-payouts-taxes", title: "Tax Handling" },
            ],
        },
        TocSection {
            id: "marketplace",
            title: "Marketplace",
            subsections: vec![
                TocSubsection { id: "marketplace-selling", title: "Selling Items" },
                TocSubsection { id: "marketplace-splits", title: "Revenue Splits" },
                TocSubsection { id: "marketplace-api", title: "Scripting API" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Earning documentation page — Bliss (BLS), Tickets (TKT), treasury, and payouts.
#[component]
pub fn DocsEarningPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-earning"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/trending.svg" alt="Earning" class="toc-icon" />
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

                // Main Content
                <main class="docs-content">
                    // Hero
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Earning"</span>
                        </div>
                        <h1 class="docs-title">"Earning on Eustress"</h1>
                        <p class="docs-subtitle">
                            "Build, contribute, and earn real money. Eustress pays developers "
                            "directly for their work through Bliss (BLS), a proof-of-contribution "
                            "currency backed by a self-sustaining treasury."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "25 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "All Levels"
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

                        <div id="overview-currencies" class="docs-block">
                            <h3>"Two Currencies"</h3>
                            <p>
                                "The Eustress economy runs on two distinct currencies, each serving "
                                "a different purpose:"
                            </p>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"BLS"</div>
                                    <h4>"Bliss"</h4>
                                    <p>
                                        "Earned by contributing to the ecosystem. Development, creation, "
                                        "education, moderation — every meaningful contribution earns Bliss. "
                                        "Bliss is converted to USD daily and paid out via Stripe."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"TKT"</div>
                                    <h4>"Tickets"</h4>
                                    <p>
                                        "Purchased with USD via Stripe. Used to buy items in the marketplace, "
                                        "unlock premium content, and support creators. 50% of Ticket revenue "
                                        "flows into the Bliss treasury."
                                    </p>
                                </div>
                            </div>
                            <div class="docs-callout info">
                                <strong>"Key Distinction:"</strong>
                                " Developers earn Bliss (BLS) for building — NOT Tickets. "
                                "Tickets are a spending currency purchased by players. "
                                "Bliss is a reward currency earned by contributors."
                            </div>
                        </div>

                        <div id="overview-flow" class="docs-block">
                            <h3>"Money Flow"</h3>
                            <p>
                                "Here is how money moves through the Eustress economy:"
                            </p>
                            <pre class="code-block"><code>{"Players purchase Tickets (TKT) with USD via Stripe
                    │
                    ▼
        ┌───────────────────────┐
        │   Ticket Revenue      │
        │   (100% of USD)       │
        └───────────┬───────────┘
                    │
          ┌─────────┴─────────┐
          ▼                   ▼
    ┌───────────┐      ┌───────────┐
    │ Treasury  │      │ Platform  │
    │   50%     │      │   50%     │
    └─────┬─────┘      └───────────┘
          │
          ▼ Daily drip (0.276%/day)
    ┌───────────┐
    │Contributors│ ← 100% of drip goes here
    │ earn BLS  │
    └─────┬─────┘
          │
          ▼ Converted to USD
    ┌───────────┐
    │ Stripe    │ ← Daily payouts
    │ Connect   │
    └───────────┘"}</code></pre>
                            <p>
                                "The treasury is a one-way valve: it only grows. Ticket sales fill it, "
                                "and the daily drip distributes a small percentage to contributors. "
                                "There are zero deductions from the treasury — 100% of the drip goes "
                                "to the people who build."
                            </p>
                        </div>

                        <div id="overview-principles" class="docs-block">
                            <h3>"Economic Principles"</h3>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"01"</div>
                                    <h4>"Builders Get Paid"</h4>
                                    <p>
                                        "Every meaningful contribution earns BLS. The more you build, "
                                        "the more you earn. No gatekeepers, no applications, no waiting."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"02"</div>
                                    <h4>"Treasury Never Shrinks"</h4>
                                    <p>
                                        "The treasury only grows. It is funded by Ticket sales and never "
                                        "deducted from. The daily drip is a percentage of the total, "
                                        "ensuring sustainability forever."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"03"</div>
                                    <h4>"No Investor Tokens"</h4>
                                    <p>
                                        "Investors and treasury funders do NOT receive tokens. They fund "
                                        "the ecosystem because they believe in it — not to extract value "
                                        "from it."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"04"</div>
                                    <h4>"Transparent Economics"</h4>
                                    <p>
                                        "Every payout, every drip, every contribution score is auditable. "
                                        "The ledger is public. The math is open source."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 2. Bliss (BLS)
                    // ─────────────────────────────────────────────────────
                    <section id="bliss" class="docs-section">
                        <h2 class="section-anchor">"2. Bliss (BLS)"</h2>

                        <div id="bliss-what" class="docs-block">
                            <h3>"What Is Bliss"</h3>
                            <p>
                                "Bliss (BLS) is a proof-of-contribution cryptocurrency that represents "
                                "your share of the Eustress ecosystem's value. Unlike speculative tokens, "
                                "Bliss can only be earned through real, verified contributions."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Property"</th>
                                        <th>"Value"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Symbol"</td>
                                        <td>"BLS"</td>
                                    </tr>
                                    <tr>
                                        <td>"Full Name"</td>
                                        <td>"Bliss"</td>
                                    </tr>
                                    <tr>
                                        <td>"Decimals"</td>
                                        <td>"18"</td>
                                    </tr>
                                    <tr>
                                        <td>"Initial Supply"</td>
                                        <td>"100,000,000 (100M)"</td>
                                    </tr>
                                    <tr>
                                        <td>"Hard Cap"</td>
                                        <td>"None — tail emission ensures perpetual rewards"</td>
                                    </tr>
                                    <tr>
                                        <td>"Consensus"</td>
                                        <td>"Proof-of-Contribution (PoC)"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <div class="docs-callout success">
                                <strong>"Not Hard-Capped:"</strong>
                                " Bliss is intentionally NOT hard-capped. Tail emission ensures that "
                                "there are always rewards available for new contributors, even decades "
                                "from now. Early contributors benefit from lower total supply, but "
                                "the system never runs dry."
                            </div>
                        </div>

                        <div id="bliss-supply" class="docs-block">
                            <h3>"Supply & Emission"</h3>
                            <p>
                                "Bliss uses a halving emission schedule with a permanent tail emission "
                                "floor, ensuring the ecosystem always has rewards to distribute:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Period"</th>
                                        <th>"Annual Emission Rate"</th>
                                        <th>"Approx. New BLS/Year"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Years 0–4"</td>
                                        <td>"5.0%"</td>
                                        <td>"5,000,000"</td>
                                    </tr>
                                    <tr>
                                        <td>"Years 4–8"</td>
                                        <td>"2.5%"</td>
                                        <td>"~2,625,000"</td>
                                    </tr>
                                    <tr>
                                        <td>"Years 8–12"</td>
                                        <td>"1.25%"</td>
                                        <td>"~1,380,000"</td>
                                    </tr>
                                    <tr>
                                        <td>"Years 12–16"</td>
                                        <td>"0.625%"</td>
                                        <td>"~720,000"</td>
                                    </tr>
                                    <tr>
                                        <td>"Years 16+"</td>
                                        <td>"0.5% (floor)"</td>
                                        <td>"~580,000+"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <pre class="code-block"><code>{"// Emission rate calculation
fn emission_rate(years_since_launch: f64) -> f64 {
    let halvings = (years_since_launch / 4.0).floor() as u32;
    let rate = 0.05 * (0.5_f64).powi(halvings as i32);
    rate.max(0.005) // minimum 0.5% forever
}

// Example: Year 10 → 0.05 * 0.5^2 = 0.0125 (1.25%)
// Example: Year 20 → max(0.05 * 0.5^5, 0.005) = 0.005 (0.5%)"}</code></pre>
                        </div>

                        <div id="bliss-distribution" class="docs-block">
                            <h3>"Daily Distribution"</h3>
                            <p>
                                "Bliss is distributed once per day at UTC midnight. The process is "
                                "fully automated and deterministic:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"00:00 UTC"</strong>
                                    " — Snapshot all pending contribution scores"
                                </li>
                                <li>
                                    <strong>"00:01 UTC"</strong>
                                    " — Calculate each contributor's share of the daily emission pool"
                                </li>
                                <li>
                                    <strong>"00:02 UTC"</strong>
                                    " — Mint new BLS according to the emission schedule"
                                </li>
                                <li>
                                    <strong>"00:03 UTC"</strong>
                                    " — Distribute BLS proportionally to all contributors"
                                </li>
                                <li>
                                    <strong>"00:05 UTC"</strong>
                                    " — Treasury drip: convert BLS share to USD, queue Stripe payouts"
                                </li>
                            </ul>
                            <pre class="code-block"><code>{"// Your daily BLS share
let your_score = your_weighted_contributions;
let total_score = all_contributors_weighted_sum;
let daily_emission = total_supply * emission_rate / 365.0;

let your_bls = daily_emission * (your_score / total_score);"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 3. Contribution Types
                    // ─────────────────────────────────────────────────────
                    <section id="contributions" class="docs-section">
                        <h2 class="section-anchor">"3. Contribution Types"</h2>

                        <div id="contributions-weights" class="docs-block">
                            <h3>"Weight Table"</h3>
                            <p>
                                "Each type of contribution has a weight multiplier that reflects "
                                "its impact on the ecosystem. Higher weights reward harder, more "
                                "impactful work:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Type"</th>
                                        <th>"Weight"</th>
                                        <th>"Description"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><strong>"Development"</strong></td>
                                        <td>"3.0x"</td>
                                        <td>"Code commits, bug fixes, new features, engine work"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Creation"</strong></td>
                                        <td>"2.5x"</td>
                                        <td>"3D models, textures, audio, animations, shaders"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Education"</strong></td>
                                        <td>"2.2x"</td>
                                        <td>"Tutorials, courses, workshops, mentoring"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Collaboration"</strong></td>
                                        <td>"2.0x"</td>
                                        <td>"Code reviews, design discussions, pair programming"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Optimization"</strong></td>
                                        <td>"2.0x"</td>
                                        <td>"Performance improvements, memory reduction, profiling"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"QualityAssurance"</strong></td>
                                        <td>"1.8x"</td>
                                        <td>"Testing, bug reports with reproductions, test suites"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Moderation"</strong></td>
                                        <td>"1.5x"</td>
                                        <td>"Community moderation, content review, safety"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Documentation"</strong></td>
                                        <td>"1.5x"</td>
                                        <td>"API docs, guides, README updates, translations"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"ActiveTime"</strong></td>
                                        <td>"1.0x"</td>
                                        <td>"Time spent actively using and testing the platform"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="contributions-scoring" class="docs-block">
                            <h3>"Scoring Formula"</h3>
                            <p>
                                "Your daily contribution score is calculated as the sum of all "
                                "your verified contributions, each multiplied by its type weight:"
                            </p>
                            <pre class="code-block"><code>{"// Contribution scoring
struct Contribution {
    contribution_type: ContributionType,
    base_score: f64,       // determined by magnitude of work
    witness_signature: Ed25519Signature,  // co-signed by Worker
}

fn daily_score(contributions: &[Contribution]) -> f64 {
    contributions.iter()
        .map(|c| c.base_score * c.contribution_type.weight())
        .sum()
}

// Example: A developer who commits code (3.0x) and writes docs (1.5x)
// Code commit base_score: 10.0  → 10.0 * 3.0 = 30.0
// Docs update base_score:  5.0  →  5.0 * 1.5 =  7.5
// Daily total: 37.5"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Base Score:"</strong>
                                " The base score for each contribution is determined by the magnitude "
                                "and quality of the work. A 500-line feature gets a higher base score "
                                "than a 5-line typo fix. The witness Worker validates the score."
                            </div>
                        </div>

                        <div id="contributions-examples" class="docs-block">
                            <h3>"Examples"</h3>
                            <p>
                                "Here are some real-world contribution examples and their approximate "
                                "daily scores:"
                            </p>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"A"</div>
                                    <h4>"Full-Time Developer"</h4>
                                    <p>
                                        "8 hours of engine work: 3 code commits (base 10 each) + "
                                        "1 code review (base 5) + active time (base 8). "
                                        "Score: 30*3.0 + 5*2.0 + 8*1.0 = 108.0"
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"B"</div>
                                    <h4>"Content Creator"</h4>
                                    <p>
                                        "Published a tutorial video (base 15) + wrote companion "
                                        "docs (base 5) + active time (base 3). "
                                        "Score: 15*2.2 + 5*1.5 + 3*1.0 = 43.5"
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"C"</div>
                                    <h4>"3D Artist"</h4>
                                    <p>
                                        "Uploaded 3 asset packs (base 8 each) + active time (base 4). "
                                        "Score: 24*2.5 + 4*1.0 = 64.0"
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"D"</div>
                                    <h4>"Community Moderator"</h4>
                                    <p>
                                        "6 hours of moderation (base 12) + filed 2 bug reports with "
                                        "reproductions (base 6 each). "
                                        "Score: 12*1.5 + 12*1.8 = 39.6"
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 4. Earning BLS
                    // ─────────────────────────────────────────────────────
                    <section id="earning-bls" class="docs-section">
                        <h2 class="section-anchor">"4. Earning BLS"</h2>

                        <div id="earning-bls-witness" class="docs-block">
                            <h3>"Witness Co-Signing"</h3>
                            <p>
                                "Every contribution must be co-signed by a witness Worker running "
                                "on "<code>"api.eustress.dev"</code>" to prevent fraud. This ensures "
                                "that only real, verified work earns Bliss:"
                            </p>
                            <pre class="code-block"><code>{"// Contribution validation flow
//
// 1. Developer performs work (commit, asset upload, etc.)
// 2. Client submits contribution claim to api.eustress.dev
// 3. Worker validates the claim:
//    - Verifies the work exists (commit hash, asset hash, etc.)
//    - Checks for duplicates (no double-claiming)
//    - Validates timestamps (no future-dating)
//    - Assesses base score based on magnitude
// 4. Worker co-signs with Ed25519 signature
// 5. Signed contribution is recorded in the ledger

struct WitnessedContribution {
    contributor: PublicKey,
    contribution: Contribution,
    witness: WorkerSignature,    // api.eustress.dev signs this
    timestamp: DateTime<Utc>,
    ledger_entry: u64,
}"}</code></pre>
                            <div class="docs-callout warning">
                                <strong>"Anti-Fraud:"</strong>
                                " Contributions without a valid witness co-signature are rejected. "
                                "The Worker checks for gaming patterns: artificial commits, bot activity, "
                                "circular reviews, and inflated active time. Violators are permanently "
                                "banned from earning."
                            </div>
                        </div>

                        <div id="earning-bls-nodes" class="docs-block">
                            <h3>"Node Bonuses"</h3>
                            <p>
                                "Contributors who run Eustress nodes earn a bonus multiplier on "
                                "all their contributions:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Node Type"</th>
                                        <th>"Multiplier"</th>
                                        <th>"Description"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"Light Node"</td>
                                        <td>"1.0x (no bonus)"</td>
                                        <td>"Standard client, no additional infrastructure"</td>
                                    </tr>
                                    <tr>
                                        <td>"Full Node"</td>
                                        <td>"1.1x (+10% bonus)"</td>
                                        <td>"Runs full validation, relays data, stores ledger"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <pre class="code-block"><code>{"// Node bonus applied to final score
let base_daily_score = calculate_daily_score(&contributions);
let node_multiplier = match node_type {
    NodeType::Light => 1.0,
    NodeType::Full  => 1.1,  // +10% bonus
};

let final_score = base_daily_score * node_multiplier;

// Example: 100.0 base score on a Full Node
// final_score = 100.0 * 1.1 = 110.0"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Full Node Requirements:"</strong>
                                " Running a Full Node requires stable internet, at least 50GB storage, "
                                "and uptime of 95%+. The node validates contributions, relays data "
                                "to peers, and maintains a local copy of the contribution ledger."
                            </div>
                        </div>

                        <div id="earning-bls-payouts" class="docs-block">
                            <h3>"Payout Cycle"</h3>
                            <p>
                                "The daily payout cycle works in three phases:"
                            </p>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"I"</div>
                                    <h4>"Accumulate"</h4>
                                    <p>
                                        "Throughout the day (00:00–23:59 UTC), contributions are "
                                        "witnessed, scored, and accumulated in your pending balance. "
                                        "You can see your pending score in real time."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"II"</div>
                                    <h4>"Snapshot"</h4>
                                    <p>
                                        "At 00:00 UTC, the system snapshots all pending scores. "
                                        "Your share is your_score / total_all_scores. This ratio "
                                        "determines your BLS allocation from the daily emission."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"III"</div>
                                    <h4>"Distribute"</h4>
                                    <p>
                                        "BLS is minted according to the emission schedule and "
                                        "distributed proportionally. Treasury drip is calculated "
                                        "and USD payouts are queued via Stripe Connect."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 5. Tickets (TKT)
                    // ─────────────────────────────────────────────────────
                    <section id="tickets" class="docs-section">
                        <h2 class="section-anchor">"5. Tickets (TKT)"</h2>

                        <div id="tickets-packages" class="docs-block">
                            <h3>"Packages"</h3>
                            <p>
                                "Tickets are purchased with USD via Stripe. Five packages are "
                                "available, with larger packages offering better value:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Package"</th>
                                        <th>"Price (USD)"</th>
                                        <th>"Tickets"</th>
                                        <th>"$/Ticket"</th>
                                        <th>"Bonus"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><strong>"Starter"</strong></td>
                                        <td>"$4.99"</td>
                                        <td>"400"</td>
                                        <td>"$0.01248"</td>
                                        <td>"—"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Standard"</strong></td>
                                        <td>"$9.99"</td>
                                        <td>"880"</td>
                                        <td>"$0.01135"</td>
                                        <td>"+10%"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Mega"</strong></td>
                                        <td>"$19.99"</td>
                                        <td>"1,840"</td>
                                        <td>"$0.01086"</td>
                                        <td>"+15%"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Super"</strong></td>
                                        <td>"$49.99"</td>
                                        <td>"5,000"</td>
                                        <td>"$0.01000"</td>
                                        <td>"+25%"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Ultra"</strong></td>
                                        <td>"$99.99"</td>
                                        <td>"10,800"</td>
                                        <td>"$0.00926"</td>
                                        <td>"+35%"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="tickets-revenue" class="docs-block">
                            <h3>"Revenue Split"</h3>
                            <p>
                                "Every dollar spent on Tickets is split exactly 50/50:"
                            </p>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"50%"</div>
                                    <h4>"Bliss Treasury"</h4>
                                    <p>
                                        "Funds the treasury that pays contributors daily. This is "
                                        "the fuel that keeps the earning engine running. Every Ticket "
                                        "purchase directly supports the developers building Eustress."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"50%"</div>
                                    <h4>"Platform Revenue"</h4>
                                    <p>
                                        "Funds Eustress operations: infrastructure, development team, "
                                        "support, and growth. This ensures the platform itself is "
                                        "sustainable long-term."
                                    </p>
                                </div>
                            </div>
                            <pre class="code-block"><code>{"// Revenue split on every Ticket purchase
fn process_ticket_purchase(usd_amount: f64) {
    let treasury_share = usd_amount * 0.50;  // 50% → Bliss treasury
    let platform_share = usd_amount * 0.50;  // 50% → platform ops

    treasury.deposit(treasury_share);
    platform.deposit(platform_share);

    // Example: Player buys $49.99 Super package
    // → $24.995 goes to treasury (pays contributors)
    // → $24.995 goes to platform (pays infrastructure)
}"}</code></pre>
                        </div>

                        <div id="tickets-usage" class="docs-block">
                            <h3>"Usage"</h3>
                            <p>
                                "Tickets can be spent on:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"Marketplace Items"</strong>
                                    " — 3D models, textures, audio packs, templates, plugins"
                                </li>
                                <li>
                                    <strong>"Game Passes"</strong>
                                    " — Developer-defined premium access passes for experiences"
                                </li>
                                <li>
                                    <strong>"Cosmetics"</strong>
                                    " — Avatar items, skins, effects, animations"
                                </li>
                                <li>
                                    <strong>"Premium Content"</strong>
                                    " — Exclusive tutorials, courses, and workshops"
                                </li>
                                <li>
                                    <strong>"Tips"</strong>
                                    " — Direct tips to creators and developers you appreciate"
                                </li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"No Expiration:"</strong>
                                " Tickets never expire. Once purchased, they remain in your account "
                                "indefinitely. There are no maintenance fees or hidden deductions."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 6. Treasury
                    // ─────────────────────────────────────────────────────
                    <section id="treasury" class="docs-section">
                        <h2 class="section-anchor">"6. Treasury"</h2>

                        <div id="treasury-funding" class="docs-block">
                            <h3>"How It Grows"</h3>
                            <p>
                                "The treasury is funded exclusively by Ticket sales. 50% of every "
                                "dollar spent on Tickets flows directly into the treasury. There are "
                                "no other revenue sources for the treasury — it is purely player-funded."
                            </p>
                            <pre class="code-block"><code>{"// Treasury growth (simplified)
//
// Day 1: 1,000 Ticket purchases → $500 to treasury
//         Treasury balance: $500
//
// Day 2: 1,500 Ticket purchases → $750 to treasury
//         Daily drip: $500 * 0.00276 = $1.38 paid to contributors
//         Treasury balance: $500 - $1.38 + $750 = $1,248.62
//
// Day 30: Accumulated deposits far exceed drip
//          Treasury balance keeps growing
//
// The treasury ONLY grows because daily deposits >> daily drip"}</code></pre>
                            <div class="docs-callout success">
                                <strong>"One-Way Valve:"</strong>
                                " The treasury is never deducted from beyond the daily drip. No "
                                "emergency withdrawals. No investor payouts. No management fees. "
                                "100% of the drip goes to contributors. The treasury only grows."
                            </div>
                        </div>

                        <div id="treasury-drip" class="docs-block">
                            <h3>"Daily Drip"</h3>
                            <p>
                                "The treasury releases funds daily using exponential decay at a "
                                "rate of 0.276% per day. This means the drip automatically scales "
                                "with the treasury size:"
                            </p>
                            <pre class="code-block"><code>{"// Treasury drip calculation
const DAILY_DRIP_RATE: f64 = 0.00276; // 0.276% per day

fn daily_drip(treasury_balance: f64) -> f64 {
    treasury_balance * DAILY_DRIP_RATE
}

// Examples at different treasury sizes:
//
// Treasury: $10,000    → Daily drip: $27.60
// Treasury: $100,000   → Daily drip: $276.00
// Treasury: $1,000,000 → Daily drip: $2,760.00
//
// As the treasury grows, contributor payouts grow proportionally.
// The percentage stays constant, so the treasury is never depleted."}</code></pre>
                            <p>
                                "The 0.276% daily rate means approximately 63% of the treasury "
                                "is distributed per year, but since new deposits constantly flow in "
                                "from Ticket sales, the effective balance grows over time."
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Treasury Size"</th>
                                        <th>"Daily Drip"</th>
                                        <th>"Monthly Drip"</th>
                                        <th>"Annual Drip"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"$10,000"</td>
                                        <td>"$27.60"</td>
                                        <td>"$828"</td>
                                        <td>"$10,074"</td>
                                    </tr>
                                    <tr>
                                        <td>"$100,000"</td>
                                        <td>"$276"</td>
                                        <td>"$8,280"</td>
                                        <td>"$100,740"</td>
                                    </tr>
                                    <tr>
                                        <td>"$1,000,000"</td>
                                        <td>"$2,760"</td>
                                        <td>"$82,800"</td>
                                        <td>"$1,007,400"</td>
                                    </tr>
                                    <tr>
                                        <td>"$10,000,000"</td>
                                        <td>"$27,600"</td>
                                        <td>"$828,000"</td>
                                        <td>"$10,074,000"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="treasury-guarantees" class="docs-block">
                            <h3>"Guarantees"</h3>
                            <div class="principles-grid">
                                <div class="principle-card">
                                    <div class="principle-number">"01"</div>
                                    <h4>"100% to Contributors"</h4>
                                    <p>
                                        "Every cent of the daily drip goes to contributors. Zero "
                                        "management fees. Zero platform deductions from the treasury. "
                                        "The platform is funded by its 50% share of Ticket revenue."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"02"</div>
                                    <h4>"Never Depleted"</h4>
                                    <p>
                                        "Exponential decay means the treasury asymptotically approaches "
                                        "zero but never reaches it. Combined with continuous Ticket "
                                        "sales deposits, it perpetually grows."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"03"</div>
                                    <h4>"Auditable"</h4>
                                    <p>
                                        "Treasury balance, daily drip amounts, and all payouts are "
                                        "recorded on the public ledger. Anyone can verify the math."
                                    </p>
                                </div>
                                <div class="principle-card">
                                    <div class="principle-number">"04"</div>
                                    <h4>"No Investor Tokens"</h4>
                                    <p>
                                        "The treasury exists to pay builders, not investors. People who "
                                        "fund the ecosystem do so because they believe in it — not to "
                                        "extract tokens or returns."
                                    </p>
                                </div>
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 7. USD Payouts
                    // ─────────────────────────────────────────────────────
                    <section id="usd-payouts" class="docs-section">
                        <h2 class="section-anchor">"7. USD Payouts"</h2>

                        <div id="usd-payouts-stripe" class="docs-block">
                            <h3>"Stripe Connect"</h3>
                            <p>
                                "USD payouts are handled through Stripe Connect Custom accounts. "
                                "This provides bank-grade security, global coverage, and automatic "
                                "tax handling:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"Daily Payouts"</strong>
                                    " — Your earned BLS share is converted to USD and paid out daily"
                                </li>
                                <li>
                                    <strong>"Direct Deposit"</strong>
                                    " — Funds go directly to your bank account or debit card"
                                </li>
                                <li>
                                    <strong>"Global Coverage"</strong>
                                    " — Stripe supports 46+ countries for payouts"
                                </li>
                                <li>
                                    <strong>"Instant Payouts"</strong>
                                    " — Available in supported regions for a small fee"
                                </li>
                            </ul>
                            <pre class="code-block"><code>{"// Payout flow
//
// 1. Daily BLS distribution at UTC midnight
// 2. Your BLS share → converted to USD at current rate
// 3. USD queued in Stripe Connect
// 4. Stripe processes payout to your bank (1-2 business days)
//
// Minimum payout: $0.50 (below this, balance rolls over)
// Payout schedule: Daily (configurable to weekly/monthly)"}</code></pre>
                        </div>

                        <div id="usd-payouts-kyc" class="docs-block">
                            <h3>"KYC Verification"</h3>
                            <p>
                                "To receive USD payouts, you must complete KYC (Know Your Customer) "
                                "identity verification. This is required by law and handled through "
                                "Stripe Identity:"
                            </p>
                            <div class="docs-callout warning">
                                <strong>"Required for Payouts:"</strong>
                                " You can earn BLS without KYC, but converting to USD requires "
                                "verified identity. Your BLS balance accumulates until verification "
                                "is complete."
                            </div>
                            <p>
                                "Eustress supports contributors from 72 IRS Qualified Intermediary "
                                "jurisdictions, covering most of the world:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Region"</th>
                                        <th>"Coverage"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td>"North America"</td>
                                        <td>"United States, Canada, Mexico"</td>
                                    </tr>
                                    <tr>
                                        <td>"Europe"</td>
                                        <td>"All EU/EEA countries, UK, Switzerland"</td>
                                    </tr>
                                    <tr>
                                        <td>"Asia-Pacific"</td>
                                        <td>"Japan, South Korea, Australia, New Zealand, Singapore, and more"</td>
                                    </tr>
                                    <tr>
                                        <td>"Latin America"</td>
                                        <td>"Brazil, Argentina, Chile, Colombia, and more"</td>
                                    </tr>
                                    <tr>
                                        <td>"Other"</td>
                                        <td>"Israel, South Africa, UAE, and additional QI jurisdictions"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <p>
                                "KYC verification typically takes under 5 minutes and requires a "
                                "government-issued photo ID and a selfie."
                            </p>
                        </div>

                        <div id="usd-payouts-taxes" class="docs-block">
                            <h3>"Tax Handling"</h3>
                            <p>
                                "Stripe handles tax form generation automatically:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"US Contributors"</strong>
                                    " — 1099-K or 1099-MISC forms generated by Stripe annually"
                                </li>
                                <li>
                                    <strong>"International Contributors"</strong>
                                    " — W-8BEN forms collected during KYC; tax reporting follows "
                                    "local jurisdiction rules"
                                </li>
                                <li>
                                    <strong>"Real-Time Tracking"</strong>
                                    " — All earnings visible in your dashboard with exportable "
                                    "CSV reports for your accountant"
                                </li>
                            </ul>
                            <div class="docs-callout info">
                                <strong>"Tax Responsibility:"</strong>
                                " Eustress provides the forms; you are responsible for filing taxes "
                                "according to your jurisdiction's laws. Consult a tax professional "
                                "for guidance specific to your situation."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 8. Marketplace
                    // ─────────────────────────────────────────────────────
                    <section id="marketplace" class="docs-section">
                        <h2 class="section-anchor">"8. Marketplace"</h2>

                        <div id="marketplace-selling" class="docs-block">
                            <h3>"Selling Items"</h3>
                            <p>
                                "The Eustress Marketplace lets you sell digital items for Tickets. "
                                "Players spend Tickets to purchase your items, and you earn Tickets "
                                "from each sale:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"3D Models"</strong>
                                    " — Characters, environments, props, vehicles"
                                </li>
                                <li>
                                    <strong>"Textures & Materials"</strong>
                                    " — PBR materials, texture packs, shaders"
                                </li>
                                <li>
                                    <strong>"Audio"</strong>
                                    " — Sound effects, music tracks, ambient loops"
                                </li>
                                <li>
                                    <strong>"Templates"</strong>
                                    " — Project templates, scene templates, UI kits"
                                </li>
                                <li>
                                    <strong>"Plugins"</strong>
                                    " — Engine plugins, script libraries, tools"
                                </li>
                                <li>
                                    <strong>"Game Passes"</strong>
                                    " — Access passes for premium experience features"
                                </li>
                            </ul>
                        </div>

                        <div id="marketplace-splits" class="docs-block">
                            <h3>"Revenue Splits"</h3>
                            <p>
                                "When a player purchases your item on the marketplace:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Recipient"</th>
                                        <th>"Share"</th>
                                        <th>"Description"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><strong>"Developer"</strong></td>
                                        <td>"70%"</td>
                                        <td>"You earn 70% of the Ticket price in Tickets"</td>
                                    </tr>
                                    <tr>
                                        <td><strong>"Platform"</strong></td>
                                        <td>"30%"</td>
                                        <td>"Eustress takes 30% to fund marketplace infrastructure"</td>
                                    </tr>
                                </tbody>
                            </table>
                            <pre class="code-block"><code>{"// Marketplace sale example
//
// You list a 3D character model for 500 Tickets
// A player purchases it
//
// You receive:    500 * 0.70 = 350 Tickets
// Platform gets:  500 * 0.30 = 150 Tickets
//
// Your Tickets can be:
// - Spent on other marketplace items
// - Held in your account indefinitely"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Dual Income:"</strong>
                                " Marketplace sales earn you Tickets (TKT), while your contributions "
                                "to the ecosystem earn you Bliss (BLS). Active developers benefit "
                                "from both revenue streams simultaneously."
                            </div>
                        </div>

                        <div id="marketplace-api" class="docs-block">
                            <h3>"Scripting API"</h3>
                            <p>
                                "The MarketplaceService provides a scripting API for integrating "
                                "purchases directly into your experiences:"
                            </p>
                            <pre class="code-block"><code>{"// MarketplaceService API

// Prompt a player to purchase a product
// Returns: PurchaseResult (Purchased, Cancelled, Error)
MarketplaceService::PromptPurchase(player, product_id)

// Get information about a product
// Returns: ProductInfo { name, price, description, creator }
MarketplaceService::GetProductInfo(product_id)

// Check if a player owns a specific game pass
// Returns: bool
MarketplaceService::PlayerOwnsGamePass(player, pass_id)"}</code></pre>
                            <p>
                                "Example usage in a Soul script:"
                            </p>
                            <pre class="code-block"><code>{"// vip_door.soul
//
// When a player touches the VIP door:
//   If the player owns the \"VIP Pass\" game pass, open the door.
//   Otherwise, prompt them to purchase it for 200 Tickets.

When player touches VIPDoor {
    if MarketplaceService::PlayerOwnsGamePass(player, \"vip-pass-001\") {
        open_door(VIPDoor)
        show_message(player, \"Welcome, VIP!\")
    } else {
        let result = MarketplaceService::PromptPurchase(player, \"vip-pass-001\")
        if result == Purchased {
            open_door(VIPDoor)
            show_message(player, \"Thanks for purchasing! Welcome, VIP!\")
        }
    }
}"}</code></pre>
                            <p>
                                "The full MarketplaceService API reference:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr>
                                        <th>"Method"</th>
                                        <th>"Parameters"</th>
                                        <th>"Returns"</th>
                                        <th>"Description"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr>
                                        <td><code>"PromptPurchase"</code></td>
                                        <td>"player, product_id"</td>
                                        <td>"PurchaseResult"</td>
                                        <td>"Shows purchase dialog to player"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"GetProductInfo"</code></td>
                                        <td>"product_id"</td>
                                        <td>"ProductInfo"</td>
                                        <td>"Fetches product metadata"</td>
                                    </tr>
                                    <tr>
                                        <td><code>"PlayerOwnsGamePass"</code></td>
                                        <td>"player, pass_id"</td>
                                        <td>"bool"</td>
                                        <td>"Checks game pass ownership"</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/publishing" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Publishing"</span>
                            </div>
                        </a>
                        <a href="/docs/philosophy" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Philosophy"</span>
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
