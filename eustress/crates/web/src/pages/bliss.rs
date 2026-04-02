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
use crate::wallet::{use_wallet, WalletStatus};

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
    pub avg_contribution_score: f64,
    pub network_hashrate: String,
    pub blocks_mined: u64,
    pub monthly_distribution: String,
    pub forks_registered: u64,
}

impl Default for BlissKpi {
    fn default() -> Self {
        Self {
            total_supply: "Loading...".to_string(),
            circulating_supply: "Loading...".to_string(),
            treasury_balance: "Loading...".to_string(),
            active_nodes: 0,
            total_contributors: 0,
            avg_contribution_score: 0.0,
            network_hashrate: "Loading...".to_string(),
            blocks_mined: 0,
            monthly_distribution: "Loading...".to_string(),
            forks_registered: 0,
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
    let wallet = use_wallet();

    // Wallet modal state
    let show_wallet_modal = RwSignal::new(false);
    let wallet_name = RwSignal::new(String::new());
    let private_key_input = RwSignal::new(String::new());
    let import_mode = RwSignal::new(false);
    let wallet_error = RwSignal::new(Option::<String>::None);

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
        avg_contribution_score: 23.7,
        network_hashrate: "1.2 TH/s".to_string(),
        blocks_mined: 48_291,
        monthly_distribution: "12,500 BLS".to_string(),
        forks_registered: 8,
    });

    // Investment tiers
    let tiers = vec![
        InvestmentTier {
            name: "Seed",
            amount_usd: "$25",
            bls_estimate: "~250 BLS",
            description: "Support the ecosystem at ground level",
            perks: vec!["Treasury contribution receipt", "Supporter badge on profile"],
        },
        InvestmentTier {
            name: "Growth",
            amount_usd: "$100",
            bls_estimate: "~1,050 BLS",
            description: "Meaningful treasury investment with bonus allocation",
            perks: vec!["5% bonus BLS allocation", "Supporter badge", "Early feature access"],
        },
        InvestmentTier {
            name: "Sustainer",
            amount_usd: "$500",
            bls_estimate: "~5,500 BLS",
            description: "Significant contribution to network sustainability",
            perks: vec!["10% bonus BLS allocation", "Gold badge", "Governance vote weight", "Priority fork registration"],
        },
        InvestmentTier {
            name: "Patron",
            amount_usd: "$1,000+",
            bls_estimate: "~11,500+ BLS",
            description: "Major backer powering the Bliss economy",
            perks: vec!["15% bonus BLS allocation", "Platinum badge", "Governance vote weight", "Direct line to core team", "Name in credits"],
        },
    ];

    // Wallet actions
    let create_wallet = Callback::new({
        let wallet = wallet.clone();
        move |_: ()| {
            let name = wallet_name.get();
            if name.is_empty() {
                wallet_error.set(Some("Please enter a wallet name".to_string()));
                return;
            }
            match wallet.create_wallet(&name) {
                Ok(_) => {
                    show_wallet_modal.set(false);
                    wallet_error.set(None);
                }
                Err(e) => wallet_error.set(Some(e)),
            }
        }
    });

    let import_wallet = Callback::new({
        let wallet = wallet.clone();
        move |_: ()| {
            let name = wallet_name.get();
            let key = private_key_input.get();
            if name.is_empty() || key.is_empty() {
                wallet_error.set(Some("Please enter wallet name and private key".to_string()));
                return;
            }
            match wallet.import_wallet(&name, &key) {
                Ok(_) => {
                    show_wallet_modal.set(false);
                    wallet_error.set(None);
                    private_key_input.set(String::new());
                }
                Err(e) => wallet_error.set(Some(e)),
            }
        }
    });

    let disconnect_wallet = Callback::new({
        let wallet = wallet.clone();
        move |_: ()| {
            wallet.disconnect();
        }
    });

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

                // Wallet Connection (compact)
                {move || {
                    match wallet.status.get() {
                        WalletStatus::Connected(info) => {
                            view! {
                                <div class="wallet-connected">
                                    <div class="wallet-info">
                                        <span class="wallet-label">"Connected"</span>
                                        <span class="wallet-address">{shorten_address(&info.address)}</span>
                                    </div>
                                    <div class="wallet-balance">
                                        <img src="/assets/icons/bliss.svg" alt="BLS" />
                                        <span class="balance-value">{wallet.formatted_balance()}</span>
                                    </div>
                                    <button class="disconnect-btn" on:click=move |_| disconnect_wallet.run(())>
                                        "Disconnect"
                                    </button>
                                </div>
                            }.into_any()
                        }
                        WalletStatus::Connecting => {
                            view! {
                                <div class="wallet-connecting">
                                    <span>"Connecting wallet..."</span>
                                </div>
                            }.into_any()
                        }
                        WalletStatus::Error(err) => {
                            view! {
                                <div class="wallet-error">
                                    <span class="error-text">{err}</span>
                                    <button class="connect-btn" on:click=move |_| show_wallet_modal.set(true)>
                                        "Try Again"
                                    </button>
                                </div>
                            }.into_any()
                        }
                        WalletStatus::Disconnected => {
                            view! {
                                <div class="wallet-disconnected">
                                    <button class="connect-btn" on:click=move |_| show_wallet_modal.set(true)>
                                        <img src="/assets/icons/wallet.svg" alt="Wallet" />
                                        "Connect Wallet"
                                    </button>
                                </div>
                            }.into_any()
                        }
                    }
                }}
            </section>

            // Wallet Modal
            {move || {
                if show_wallet_modal.get() {
                    view! {
                        <div class="modal-overlay" on:click=move |_| show_wallet_modal.set(false)>
                            <div class="modal wallet-modal" on:click=|e| e.stop_propagation()>
                                <h2>"Connect Wallet"</h2>
                                <div class="modal-tabs">
                                    <button
                                        class="tab-btn"
                                        class:active=move || !import_mode.get()
                                        on:click=move |_| import_mode.set(false)
                                    >"Create New"</button>
                                    <button
                                        class="tab-btn"
                                        class:active=move || import_mode.get()
                                        on:click=move |_| import_mode.set(true)
                                    >"Import Existing"</button>
                                </div>
                                <div class="modal-form">
                                    <label>"Wallet Name"</label>
                                    <input
                                        type="text"
                                        placeholder="My Bliss Wallet"
                                        prop:value=move || wallet_name.get()
                                        on:input=move |e| wallet_name.set(event_target_value(&e))
                                    />
                                    {move || {
                                        if import_mode.get() {
                                            view! {
                                                <div class="import-fields">
                                                    <label>"Private Key (hex)"</label>
                                                    <input
                                                        type="password"
                                                        placeholder="Enter your private key..."
                                                        prop:value=move || private_key_input.get()
                                                        on:input=move |e| private_key_input.set(event_target_value(&e))
                                                    />
                                                    <p class="warning">"Never share your private key with anyone!"</p>
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <p class="info">"A new wallet will be created with a secure keypair."</p>
                                            }.into_any()
                                        }
                                    }}
                                    {move || {
                                        wallet_error.get().map(|err| view! {
                                            <p class="error">{err}</p>
                                        })
                                    }}
                                    <div class="modal-actions">
                                        <button class="cancel-btn" on:click=move |_| show_wallet_modal.set(false)>
                                            "Cancel"
                                        </button>
                                        {move || {
                                            if import_mode.get() {
                                                view! {
                                                    <button class="submit-btn" on:click=move |_| import_wallet.run(())>
                                                        "Import Wallet"
                                                    </button>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <button class="submit-btn" on:click=move |_| create_wallet.run(())>
                                                        "Create Wallet"
                                                    </button>
                                                }.into_any()
                                            }
                                        }}
                                    </div>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <div></div> }.into_any()
                }
            }}

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
                                <span class="kpi-label">"Avg Score"</span>
                                <span class="kpi-value">{format!("{:.1}", k.avg_contribution_score)}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Blocks Mined"</span>
                                <span class="kpi-value">{format!("{}", k.blocks_mined)}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Monthly Distribution"</span>
                                <span class="kpi-value">{k.monthly_distribution.clone()}</span>
                            </div>
                            <div class="kpi-card">
                                <span class="kpi-label">"Registered Forks"</span>
                                <span class="kpi-value">{k.forks_registered.to_string()}</span>
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
                    "Invest in the Bliss ecosystem. 100% of funds go to the treasury — "
                    "no middlemen, no VC extraction. Your investment directly powers the network."
                </p>

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
                                <div class="tier-amount">{tier.amount_usd}</div>
                                <div class="tier-bls">{tier.bls_estimate}</div>
                                <p class="tier-desc">{tier.description}</p>
                                <ul class="tier-perks">
                                    {tier.perks.into_iter().map(|perk| {
                                        view! { <li>{perk}</li> }
                                    }).collect::<Vec<_>>()}
                                </ul>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>

                <div class="custom-invest">
                    <label>"Custom Amount (USD)"</label>
                    <div class="custom-input-row">
                        <span class="currency-prefix">"$"</span>
                        <input
                            type="number"
                            min="5"
                            placeholder="Enter amount..."
                            prop:value=move || custom_amount.get()
                            on:input=move |e| {
                                custom_amount.set(event_target_value(&e));
                                selected_tier.set(None);
                            }
                        />
                    </div>
                </div>

                <button class="invest-btn" disabled=move || {
                    selected_tier.get().is_none() && custom_amount.get().is_empty()
                }>
                    "Invest via Stripe"
                </button>
                <p class="invest-note">
                    "Powered by Stripe. Secure payment processing. "
                    "BLS tokens are allocated to your connected wallet after payment confirmation."
                </p>
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
                            "contributions and BLS is distributed monthly based on your score."
                        </p>
                    </div>

                    <div class="faq-item">
                        <h3>"Is investing in the treasury like buying tokens?"</h3>
                        <p>
                            "Not exactly. When you fund the treasury, you receive BLS at the current "
                            "rate plus a tier bonus. The funds go directly to network operations, "
                            "development, and infrastructure — not to founders or VCs."
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
                            "No. Light Node is the default and requires no setup. Full Node is for "
                            "users who want to contribute infrastructure and earn a +10% bonus. "
                            "Both are valid participation levels."
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
