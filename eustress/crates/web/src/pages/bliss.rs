// =============================================================================
// Eustress Web - Bliss Investment Portal
// =============================================================================
// Bliss (BLS) — Proof-of-Contribution cryptocurrency for the Eustress ecosystem.
// This page is NOT pay-to-win. It is an investment portal for funding the Bliss
// treasury, viewing live KPIs, and understanding how to participate as a node.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Live KPI data fetched from the Bliss API.
#[derive(Clone, Debug, PartialEq)]
pub struct BlissKpi {
    pub total_supply: String,
    pub circulating_supply: String,
    pub treasury_balance: String,
    pub active_nodes: u64,
    pub total_contributors: u64,
    pub avg_bliss_earned: String,
    pub network_hashrate: String,
    pub blocks_mined: u64,
    pub daily_distribution: String,
}

impl Default for BlissKpi {
    fn default() -> Self {
        Self {
            total_supply: "Loading...".to_string(),
            circulating_supply: "Loading...".to_string(),
            treasury_balance: "Loading...".to_string(),
            active_nodes: 0,
            total_contributors: 0,
            avg_bliss_earned: "Loading...".to_string(),
            network_hashrate: "Loading...".to_string(),
            blocks_mined: 0,
            daily_distribution: "Loading...".to_string(),
        }
    }
}

/// Investment tier for treasury funding.
#[derive(Clone, Debug, PartialEq)]
pub struct InvestmentTier {
    pub name: &'static str,
    pub amount_usd: &'static str,
    pub bls_estimate: &'static str,
    pub description: &'static str,
    pub perks: Vec<&'static str>,
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Bliss investment portal — fund the treasury, view KPIs, learn about nodes.
#[component]
pub fn BlissPage() -> impl IntoView {
    let _app_state = expect_context::<AppState>();

    // Investment state
    let selected_tier = RwSignal::new(Option::<usize>::None);
    let custom_amount = RwSignal::new(String::new());

    // KPI data (simulated — in production, fetched from /api/bliss/kpis)
    let kpi = RwSignal::new(BlissKpi {
        total_supply: "2,847,391 BLS".to_string(),
        circulating_supply: "1,923,456 BLS".to_string(),
        treasury_balance: "$47,832 USD".to_string(),
        active_nodes: 342,
        total_contributors: 1_847,
        avg_bliss_earned: "7.41 BLS".to_string(),
        network_hashrate: "1.2 TH/s".to_string(),
        blocks_mined: 48_291,
        daily_distribution: "13,699 BLS".to_string(),
    });

    // Payment mode: one-time or recurring (monthly)
    let is_recurring = RwSignal::new(false);

    // Investment tiers — flat rate: 10 BLS per $1
    let tiers = vec![
        InvestmentTier {
            name: "Seed",
            amount_usd: "$25",
            bls_estimate: "250 BLS",
            description: "Support the ecosystem at ground level",
            perks: vec![],
        },
        InvestmentTier {
            name: "Growth",
            amount_usd: "$100",
            bls_estimate: "1,000 BLS",
            description: "Meaningful treasury investment",
            perks: vec![],
        },
        InvestmentTier {
            name: "Sustainer",
            amount_usd: "$500",
            bls_estimate: "5,000 BLS",
            description: "Significant contribution to network sustainability",
            perks: vec![],
        },
        InvestmentTier {
            name: "Patron",
            amount_usd: "$1,000+",
            bls_estimate: "10,000+ BLS",
            description: "Major backer powering the Bliss economy",
            perks: vec![],
        },
    ];

    view! {
        <div class="page page-bliss-industrial">
            <CentralNav active="".to_string() />

            // Background
            <div class="bliss-bg">
                <div class="bliss-grid-overlay"></div>
                <div class="bliss-glow glow-1"></div>
                <div class="bliss-glow glow-2"></div>
                <div class="bliss-glow glow-3"></div>
            </div>

            // =========================================================
            // Hero — Investment Portal identity
            // =========================================================
            <section class="bliss-hero">
                <div class="bliss-logo">
                    <img src="/assets/icons/bliss.svg" alt="Bliss" class="bliss-logo-icon" />
                </div>
                <h1 class="bliss-title">"Bliss (BLS)"</h1>
                <p class="bliss-tagline">"Invest in the Future of Digital Labor"</p>
                <p class="bliss-description">
                    "Bliss is a Proof-of-Contribution cryptocurrency where your creative work "
                    "earns real value. Fund the treasury to grow the ecosystem, run a node to "
                    "strengthen the network, or simply build and earn."
                </p>

                // Identity wallet — your identity.toml IS your wallet
                <div class="identity-wallet-section">
                    <div class="identity-wallet-info">
                        <img src="/assets/icons/wallet.svg" alt="Wallet" class="identity-wallet-icon" />
                        <div>
                            <p class="identity-wallet-title">"Your identity.toml is your wallet"</p>
                            <p class="identity-wallet-desc">
                                "No separate wallet needed. Your Ed25519 keypair holds your BLS. "
                                "Add beneficiaries to your identity.toml to designate asset transfers."
                            </p>
                        </div>
                    </div>
                    <a href="/login" class="btn btn-primary identity-wallet-btn">"Sign In to View Balance"</a>
                </div>
            </section>

            // =========================================================
            // Live KPIs Dashboard
            // =========================================================
            <section class="bliss-kpi-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/trending.svg" alt="KPIs" class="section-icon" />
                    <h2>"Network Dashboard"</h2>
                    <span class="live-indicator">"LIVE"</span>
                </div>

                <div class="kpi-grid">
                    {move || {
                        let k = kpi.get();
                        view! {
                            <div class="kpi-card kpi-primary">
                                <span class="kpi-label">"Treasury Balance"</span>
                                <span class="kpi-value">{k.treasury_balance.clone()}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Total Supply"</span>
                                <span class="kpi-value">{k.total_supply.clone()}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Circulating"</span>
                                <span class="kpi-value">{k.circulating_supply.clone()}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Active Nodes"</span>
                                <span class="kpi-value">{k.active_nodes.to_string()}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Contributors"</span>
                                <span class="kpi-value">{format!("{}", k.total_contributors)}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Avg Bliss/Day"</span>
                                <span class="kpi-value">{k.avg_bliss_earned.clone()}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Blocks Mined"</span>
                                <span class="kpi-value">{format!("{}", k.blocks_mined)}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Daily Distribution"</span>
                                <span class="kpi-value">{k.daily_distribution.clone()}</span>
                            </div>
                        }
                    }}
                </div>
            </section>

            // =========================================================
            // Fund the Treasury (Stripe integration)
            // =========================================================
            <section class="bliss-invest-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/gift.svg" alt="Invest" class="section-icon" />
                    <h2>"Fund the Treasury"</h2>
                </div>
                <p class="section-subtitle">
                    "100% goes to the treasury. No middlemen, no VC extraction. "
                    "Flat rate: 10 BLS per $1."
                </p>

                // One-time / Recurring toggle
                <div class="payment-mode-tabs">
                    <button
                        class="payment-mode-tab"
                        class:active=move || !is_recurring.get()
                        on:click=move |_| is_recurring.set(false)
                    >"One-time"</button>
                    <button
                        class="payment-mode-tab"
                        class:active=move || is_recurring.get()
                        on:click=move |_| is_recurring.set(true)
                    >"Monthly"</button>
                </div>

                <div class="tier-grid">
                    {tiers.into_iter().enumerate().map(|(i, tier)| {
                        let is_selected = move || selected_tier.get() == Some(i);
                        view! {
                            <div
                                class="tier-card"
                                class:selected=is_selected
                                on:click=move |_| selected_tier.set(Some(i))
                            >
                                <h3 class="tier-name">{tier.name}</h3>
                                <div class="tier-price">{tier.amount_usd}
                                    <span class="tier-frequency">
                                        {move || if is_recurring.get() { "/mo" } else { "" }}
                                    </span>
                                </div>
                                <div class="tier-bls">{tier.bls_estimate}
                                    <span class="tier-frequency">
                                        {move || if is_recurring.get() { "/mo" } else { "" }}
                                    </span>
                                </div>
                                <p class="tier-desc">{tier.description}</p>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>

                <div class="custom-invest-card">
                    <div class="custom-invest-header">
                        <span class="custom-invest-label">"Or enter a custom amount"</span>
                    </div>
                    <div class="custom-invest-input">
                        <span class="currency-symbol">"$"</span>
                        <input
                            type="number"
                            min="5"
                            step="1"
                            class="custom-amount-input"
                            placeholder="0"
                            prop:value=move || custom_amount.get()
                            on:input=move |e| {
                                custom_amount.set(event_target_value(&e));
                                selected_tier.set(None);
                            }
                        />
                        <span class="currency-code">"USD"</span>
                    </div>
                    {move || {
                        let amt = custom_amount.get().parse::<f64>().unwrap_or(0.0);
                        if amt >= 5.0 {
                            let bls = amt * 10.0; // flat rate: 10 BLS per $1
                            Some(view! {
                                <p class="custom-estimate">{format!("~{:.0} BLS", bls)}</p>
                            })
                        } else {
                            None
                        }
                    }}
                </div>

                <button class="invest-btn" disabled=move || {
                    selected_tier.get().is_none() && custom_amount.get().is_empty()
                }>
                    {move || if is_recurring.get() { "Subscribe Monthly" } else { "Fund Treasury" }}
                </button>
                <div class="invest-footer">
                    <img src="/assets/icons/shield.svg" alt="Secure" class="invest-footer-icon" />
                    <p class="invest-note">
                        "Secure payment via Stripe. 100% goes to the treasury. "
                        "BLS allocated to your wallet after confirmation."
                    </p>
                </div>
            </section>

            // =========================================================
            // Why Bliss — Better than traditional money
            // =========================================================
            <section class="bliss-why-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/help.svg" alt="Why" class="section-icon" />
                    <h2>"Why Bliss?"</h2>
                </div>
                <p class="section-subtitle">"What makes Bliss different from traditional money and other cryptocurrencies"</p>

                <div class="why-grid">
                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/trending.svg" alt="Earn" />
                        </div>
                        <h3>"You Earn by Creating"</h3>
                        <p>
                            "Traditional money requires selling your time to someone else. "
                            "Bliss rewards you directly for building, scripting, teaching, "
                            "and collaborating. Your creative output has intrinsic value."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/shield.svg" alt="No Inflation" />
                        </div>
                        <h3>"Predictable Tail Emission"</h3>
                        <p>
                            "No unlimited printing. Bliss uses a halving schedule with a "
                            "permanent tail emission floor — enough to reward contributors "
                            "forever, never enough to devalue holdings."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/network.svg" alt="Decentralized" />
                        </div>
                        <h3>"Fork-Portable"</h3>
                        <p>
                            "Your identity and earnings travel with you. If a server shuts "
                            "down or you disagree with governance, take your TOML identity "
                            "file to any fork. No lock-in, ever."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/code.svg" alt="Transparent" />
                        </div>
                        <h3>"Fully Open Source"</h3>
                        <p>
                            "Every line of the blockchain, wallet, and distribution system "
                            "is open source. Anyone can audit the code, run a fork, or "
                            "propose changes. No black-box tokenomics."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/wallet.svg" alt="Self Custody" />
                        </div>
                        <h3>"True Self-Custody"</h3>
                        <p>
                            "Ed25519 keypairs generated on your machine. Your keys never "
                            "leave your device. No custodial wallets, no exchanges holding "
                            "your funds, no \"freeze\" buttons."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/heart.svg" alt="Community" />
                        </div>
                        <h3>"Community Governed"</h3>
                        <p>
                            "The Online Trust Registry lets the community flag forks that "
                            "deviate from fair rates. No central authority decides who earns "
                            "what — the network self-regulates."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/clock.svg" alt="Daily" />
                        </div>
                        <h3>"Daily Payouts"</h3>
                        <p>
                            "Get paid every day at UTC midnight — not monthly, not quarterly. "
                            "Yesterday's contributions become today's earnings. "
                            "The fastest feedback loop in crypto."
                        </p>
                    </div>

                    <div class="why-card">
                        <div class="why-icon">
                            <img src="/assets/icons/network.svg" alt="KYC" />
                        </div>
                        <h3>"KYC Compliant"</h3>
                        <p>
                            "72 IRS QI-approved jurisdictions. Identity verified once, "
                            "recognized everywhere. Sybil-proof by design — one person, "
                            "one account, real contributions only."
                        </p>
                    </div>
                </div>
            </section>

            // =========================================================
            // Node Participation
            // =========================================================
            <section class="bliss-nodes-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/network.svg" alt="Nodes" class="section-icon" />
                    <h2>"Node Participation"</h2>
                </div>
                <p class="section-subtitle">
                    "Every Eustress Engine instance can contribute to the Bliss network. "
                    "Choose your participation level."
                </p>

                <div class="node-comparison">
                    // Light Node
                    <div class="node-card node-light">
                        <div class="node-badge">"DEFAULT"</div>
                        <h3>"Light Node"</h3>
                        <p class="node-tagline">"Earn by creating — zero setup"</p>
                        <ul class="node-features">
                            <li>"Automatic — runs when you use Eustress Engine"</li>
                            <li>"Contributions co-signed by Cloudflare witness"</li>
                            <li>"Monthly BLS distribution based on contribution score"</li>
                            <li>"No port forwarding, no server setup"</li>
                            <li>"Opt-out at any time in settings"</li>
                            <li>"Minimal bandwidth — only sends contribution hashes"</li>
                        </ul>
                        <div class="node-requirements">
                            <h4>"Requirements"</h4>
                            <p>"Internet connection (for co-signing only)"</p>
                        </div>
                    </div>

                    // Heavy Node
                    <div class="node-card node-heavy">
                        <div class="node-badge node-badge-opt-in">"OPT-IN"</div>
                        <h3>"Full Node"</h3>
                        <p class="node-tagline">"Strengthen the network, earn more"</p>
                        <ul class="node-features">
                            <li>"Everything in Light Node"</li>
                            <li>"+10% bonus on all BLS rewards"</li>
                            <li>"Store and validate blockchain data"</li>
                            <li>"Participate in block production"</li>
                            <li>"Serve as a peer for other nodes"</li>
                            <li>"Cloudflare Tunnel — no port forwarding needed"</li>
                        </ul>
                        <div class="node-requirements">
                            <h4>"Requirements"</h4>
                            <p>"2GB+ RAM, 10GB+ disk, stable internet"</p>
                        </div>
                    </div>
                </div>
            </section>

            // =========================================================
            // How Bliss Innovates
            // =========================================================
            <section class="bliss-innovation-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/cube.svg" alt="Innovation" class="section-icon" />
                    <h2>"How Bliss Innovates"</h2>
                </div>

                <div class="innovation-list">
                    <div class="innovation-item">
                        <span class="innovation-num">"01"</span>
                        <div class="innovation-content">
                            <h3>"Dual-Authority Proof"</h3>
                            <p>
                                "Every contribution requires two signatures: yours (proving you did the work) "
                                "and the Cloudflare witness (proving it happened legitimately). Neither party "
                                "can forge alone. AI can fake user signatures but cannot fake the independent "
                                "server co-signature."
                            </p>
                        </div>
                    </div>

                    <div class="innovation-item">
                        <span class="innovation-num">"02"</span>
                        <div class="innovation-content">
                            <h3>"Contribution Hash Chains"</h3>
                            <p>
                                "Your work history is a signed, append-only chain. Each entry includes "
                                "a hash of the previous entry. Tampering with any past contribution "
                                "breaks the chain — detectable by any verifier."
                            </p>
                        </div>
                    </div>

                    <div class="innovation-item">
                        <span class="innovation-num">"03"</span>
                        <div class="innovation-content">
                            <h3>"Sovereign Portable Identity"</h3>
                            <p>
                                "One TOML file on your Desktop = your entire identity. "
                                "Ed25519 keypair, contribution history, server attestations, "
                                "succession plan. Take it anywhere. No accounts, no emails, no passwords."
                            </p>
                        </div>
                    </div>

                    <div class="innovation-item">
                        <span class="innovation-num">"04"</span>
                        <div class="innovation-content">
                            <h3>"Online Trust Registry"</h3>
                            <p>
                                "Every fork publishes its issuance rate. The network computes "
                                "the median and flags outliers. Forks issuing too much or too little "
                                "are visible to all. No central authority needed — just math."
                            </p>
                        </div>
                    </div>

                    <div class="innovation-item">
                        <span class="innovation-num">"05"</span>
                        <div class="innovation-content">
                            <h3>"Non-Coercive by Design"</h3>
                            <p>
                                "Light Node is opt-out, not opt-in. You can earn without running "
                                "any infrastructure. Full Node is opt-in for those who want to "
                                "contribute more. The system works for everyone, not just power users."
                            </p>
                        </div>
                    </div>

                    <div class="innovation-item">
                        <span class="innovation-num">"06"</span>
                        <div class="innovation-content">
                            <h3>"Surgical Fork Revocation"</h3>
                            <p>
                                "If a fork is caught gaming the system, only the BLS it issued gets revoked. "
                                "Your direct contributions and earnings from other forks are never touched. "
                                "Per-fork ledger tracking makes revocation precise, not punitive."
                            </p>
                        </div>
                    </div>
                </div>
            </section>

            // =========================================================
            // FAQ
            // =========================================================
            <section class="faq-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/help.svg" alt="FAQ" class="section-icon" />
                    <h2>"Frequently Asked Questions"</h2>
                </div>

                <div class="faq-list">
                    <div class="faq-item">
                        <h3>"How do I earn BLS?"</h3>
                        <p>
                            "Use Eustress Engine. Your building, scripting, design, and collaboration "
                            "activities are automatically tracked. The Cloudflare witness co-signs your "
                            "contributions and BLS is distributed daily at UTC midnight based on your score."
                        </p>
                    </div>

                    <div class="faq-item">
                        <h3>"Where does treasury funding go?"</h3>
                        <p>
                            "100% goes to contributors. When you fund the treasury, you receive BLS "
                            "at the flat rate (10 BLS per $1). The treasury drips daily to active "
                            "contributors — your investment directly rewards the people building on Eustress."
                        </p>
                    </div>

                    <div class="faq-item">
                        <h3>"What happens if a server goes down?"</h3>
                        <p>
                            "Your identity.toml file contains everything you need. Take it to any "
                            "other Eustress fork and your contribution history, wallet, and identity "
                            "carry over. Fork portability is a core design principle."
                        </p>
                    </div>

                    <div class="faq-item">
                        <h3>"Do I need to run a Full Node?"</h3>
                        <p>
                            "No. Light Node is the default and runs automatically when you use the engine. "
                            "Full Node is opt-in for users who want to store blockchain data and produce "
                            "blocks, earning a +10% BLS bonus."
                        </p>
                    </div>

                    <div class="faq-item">
                        <h3>"How is Bliss different from mining?"</h3>
                        <p>
                            "Mining rewards hardware investment. Bliss rewards creative contribution. "
                            "You earn by building things, not by running GPUs. Proof-of-Contribution "
                            "values human work over electricity consumption."
                        </p>
                    </div>

                    <div class="faq-item">
                        <h3>"Can AI-generated contributions game the system?"</h3>
                        <p>
                            "The dual-authority model makes this hard. The Cloudflare witness "
                            "independently validates that contribution events actually occurred in "
                            "the engine. AI can't forge the server co-signature. Rate limiting "
                            "and the Online Trust Registry catch statistical anomalies."
                        </p>
                    </div>
                </div>
            </section>

            // Documentation Link
            <section class="docs-cta">
                <div class="docs-card">
                    <img src="/assets/icons/book.svg" alt="Docs" class="docs-icon" />
                    <div class="docs-content">
                        <h3>"Bliss Documentation"</h3>
                        <p>"Deep dive into tokenomics, contribution scoring, fork registration, and wallet integration"</p>
                    </div>
                    <a href="/docs/bliss" class="docs-link">
                        "View Documentation"
                        <img src="/assets/icons/arrow-right.svg" alt="Arrow" />
                    </a>
                </div>
            </section>

            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Shorten an address for display (first 6 + last 4 chars)
fn shorten_address(address: &str) -> String {
    if address.len() > 12 {
        format!("{}...{}", &address[..6], &address[address.len()-4..])
    } else {
        address.to_string()
    }
}
