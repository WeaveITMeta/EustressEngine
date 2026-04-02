// =============================================================================
// Eustress Web - Bliss Leaderboard (Top Investors & Contributors)
// =============================================================================
// Rankings for BLS investors and contributors. Anonymous users see
// the leaderboard but their own rank shows as "Anonymous Investor #N".
// Logged-in users see their profile name and can claim their position.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct InvestorEntry {
    pub rank: u32,
    pub display_name: String,
    pub is_anonymous: bool,
    pub total_invested_usd: f64,
    pub bls_holdings: f64,
    pub tier: InvestorTier,
    pub contribution_score: f64,
    pub node_type: &'static str,
    pub joined: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvestorTier {
    Patron,
    Sustainer,
    Growth,
    Seed,
    Contributor,
}

impl InvestorTier {
    fn display_name(&self) -> &'static str {
        match self {
            Self::Patron => "Patron",
            Self::Sustainer => "Sustainer",
            Self::Growth => "Growth",
            Self::Seed => "Seed",
            Self::Contributor => "Contributor",
        }
    }

    fn css_class(&self) -> &'static str {
        match self {
            Self::Patron => "tier-patron",
            Self::Sustainer => "tier-sustainer",
            Self::Growth => "tier-growth",
            Self::Seed => "tier-seed",
            Self::Contributor => "tier-contributor",
        }
    }
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

#[component]
pub fn BlissLeaderboardPage() -> impl IntoView {
    let _app_state = expect_context::<AppState>();

    let active_tab = RwSignal::new("investors".to_string());
    let search_query = RwSignal::new(String::new());

    // Sample investor data (in production, fetched from API)
    let investors = vec![
        InvestorEntry { rank: 1, display_name: "Anonymous Investor #1".to_string(), is_anonymous: true, total_invested_usd: 15_000.0, bls_holdings: 172_500.0, tier: InvestorTier::Patron, contribution_score: 0.0, node_type: "Full Node", joined: "2026-01-15" },
        InvestorEntry { rank: 2, display_name: "CryptoBuilder".to_string(), is_anonymous: false, total_invested_usd: 8_500.0, bls_holdings: 98_175.0, tier: InvestorTier::Patron, contribution_score: 847.2, node_type: "Full Node", joined: "2026-02-01" },
        InvestorEntry { rank: 3, display_name: "Anonymous Investor #3".to_string(), is_anonymous: true, total_invested_usd: 5_000.0, bls_holdings: 55_000.0, tier: InvestorTier::Sustainer, contribution_score: 0.0, node_type: "Light Node", joined: "2026-02-10" },
        InvestorEntry { rank: 4, display_name: "EustressWhale".to_string(), is_anonymous: false, total_invested_usd: 3_200.0, bls_holdings: 43_200.0, tier: InvestorTier::Sustainer, contribution_score: 1_234.5, node_type: "Full Node", joined: "2026-01-20" },
        InvestorEntry { rank: 5, display_name: "Anonymous Investor #5".to_string(), is_anonymous: true, total_invested_usd: 2_000.0, bls_holdings: 22_000.0, tier: InvestorTier::Sustainer, contribution_score: 0.0, node_type: "Light Node", joined: "2026-03-01" },
        InvestorEntry { rank: 6, display_name: "BuilderDAO".to_string(), is_anonymous: false, total_invested_usd: 1_000.0, bls_holdings: 11_500.0, tier: InvestorTier::Growth, contribution_score: 567.8, node_type: "Full Node", joined: "2026-03-05" },
        InvestorEntry { rank: 7, display_name: "NightOwlDev".to_string(), is_anonymous: false, total_invested_usd: 500.0, bls_holdings: 8_750.0, tier: InvestorTier::Growth, contribution_score: 2_340.1, node_type: "Full Node", joined: "2026-02-15" },
        InvestorEntry { rank: 8, display_name: "Anonymous Investor #8".to_string(), is_anonymous: true, total_invested_usd: 100.0, bls_holdings: 1_050.0, tier: InvestorTier::Seed, contribution_score: 0.0, node_type: "Light Node", joined: "2026-03-20" },
    ];

    // Top contributors (earn, not invest)
    let contributors = vec![
        InvestorEntry { rank: 1, display_name: "ScriptWizard".to_string(), is_anonymous: false, total_invested_usd: 0.0, bls_holdings: 45_230.0, tier: InvestorTier::Contributor, contribution_score: 4_523.0, node_type: "Full Node", joined: "2026-01-01" },
        InvestorEntry { rank: 2, display_name: "VoxelArchitect".to_string(), is_anonymous: false, total_invested_usd: 0.0, bls_holdings: 38_120.0, tier: InvestorTier::Contributor, contribution_score: 3_812.0, node_type: "Full Node", joined: "2026-01-05" },
        InvestorEntry { rank: 3, display_name: "PixelTeacher".to_string(), is_anonymous: false, total_invested_usd: 0.0, bls_holdings: 29_450.0, tier: InvestorTier::Contributor, contribution_score: 2_945.0, node_type: "Light Node", joined: "2026-01-10" },
        InvestorEntry { rank: 4, display_name: "Anonymous".to_string(), is_anonymous: true, total_invested_usd: 0.0, bls_holdings: 22_100.0, tier: InvestorTier::Contributor, contribution_score: 2_210.0, node_type: "Light Node", joined: "2026-02-01" },
        InvestorEntry { rank: 5, display_name: "MeshMaster".to_string(), is_anonymous: false, total_invested_usd: 0.0, bls_holdings: 18_900.0, tier: InvestorTier::Contributor, contribution_score: 1_890.0, node_type: "Full Node", joined: "2026-02-15" },
        InvestorEntry { rank: 6, display_name: "LuauLord".to_string(), is_anonymous: false, total_invested_usd: 0.0, bls_holdings: 15_600.0, tier: InvestorTier::Contributor, contribution_score: 1_560.0, node_type: "Light Node", joined: "2026-03-01" },
    ];

    let filter_entries = move |entries: Vec<InvestorEntry>| {
        let query = search_query.get().to_lowercase();
        if query.is_empty() {
            entries
        } else {
            entries
                .into_iter()
                .filter(|e| e.display_name.to_lowercase().contains(&query))
                .collect()
        }
    };

    view! {
        <div class="page page-leaderboard-industrial">
            <CentralNav active="".to_string() />

            // Background
            <div class="leaderboard-bg">
                <div class="leaderboard-grid-overlay"></div>
                <div class="leaderboard-glow glow-1"></div>
                <div class="leaderboard-glow glow-2"></div>
            </div>

            // Header
            <section class="leaderboard-hero">
                <div class="hero-header-lines">
                    <span class="header-line"></span>
                    <span class="header-dot"></span>
                    <span class="header-line"></span>
                </div>
                <h1 class="leaderboard-title">"Bliss Leaderboard"</h1>
                <p class="leaderboard-subtitle">"Top investors and contributors powering the Bliss economy"</p>
            </section>

            // Tabs + Search
            <section class="leaderboard-controls">
                <div class="tab-bar">
                    <button
                        class="tab-btn"
                        class:active=move || active_tab.get() == "investors"
                        on:click=move |_| active_tab.set("investors".to_string())
                    >"Investors"</button>
                    <button
                        class="tab-btn"
                        class:active=move || active_tab.get() == "contributors"
                        on:click=move |_| active_tab.set("contributors".to_string())
                    >"Contributors"</button>
                </div>

                <div class="search-bar">
                    <input
                        type="text"
                        placeholder="Search..."
                        prop:value=move || search_query.get()
                        on:input=move |e| search_query.set(event_target_value(&e))
                    />
                </div>
            </section>

            // Leaderboard Table
            <section class="leaderboard-content">
                {move || {
                    let tab = active_tab.get();
                    let entries = if tab == "investors" {
                        filter_entries(investors.clone())
                    } else {
                        filter_entries(contributors.clone())
                    };
                    let is_investor_tab = tab == "investors";

                    view! {
                        // Podium (top 3)
                        <div class="podium">
                            {entries.iter().take(3).enumerate().map(|(i, entry)| {
                                let podium_class = match i {
                                    0 => "podium-gold",
                                    1 => "podium-silver",
                                    _ => "podium-bronze",
                                };
                                view! {
                                    <div class={format!("podium-card {}", podium_class)}>
                                        <span class="podium-rank">{format!("#{}", entry.rank)}</span>
                                        <div class="podium-avatar">
                                            {if entry.is_anonymous {
                                                view! { <span class="avatar-anon">"?"</span> }.into_any()
                                            } else {
                                                view! { <span class="avatar-letter">{entry.display_name.chars().next().unwrap_or('?').to_string()}</span> }.into_any()
                                            }}
                                        </div>
                                        <span class="podium-name">{entry.display_name.clone()}</span>
                                        <span class="podium-bls">{format_number(entry.bls_holdings)} " BLS"</span>
                                        {if is_investor_tab {
                                            view! { <span class="podium-usd">{format!("${}", format_number(entry.total_invested_usd))}</span> }.into_any()
                                        } else {
                                            view! { <span class="podium-score">{format!("Score: {}", format_number(entry.contribution_score))}</span> }.into_any()
                                        }}
                                        <span class={format!("tier-badge {}", entry.tier.css_class())}>{entry.tier.display_name()}</span>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        // Full table
                        <div class="leaderboard-table">
                            <div class="table-header">
                                <span class="col-rank">"#"</span>
                                <span class="col-name">"Name"</span>
                                <span class="col-bls">"BLS Holdings"</span>
                                {if is_investor_tab {
                                    view! { <span class="col-invested">"Invested"</span> }.into_any()
                                } else {
                                    view! { <span class="col-score">"Score"</span> }.into_any()
                                }}
                                <span class="col-tier">"Tier"</span>
                                <span class="col-node">"Node"</span>
                            </div>

                            {entries.into_iter().map(|entry| {
                                view! {
                                    <div class="table-row">
                                        <span class="col-rank">{entry.rank.to_string()}</span>
                                        <span class="col-name">
                                            {if entry.is_anonymous {
                                                view! { <span class="name-anon">{entry.display_name.clone()}</span> }.into_any()
                                            } else {
                                                view! { <span class="name-public">{entry.display_name.clone()}</span> }.into_any()
                                            }}
                                        </span>
                                        <span class="col-bls">{format!("{} BLS", format_number(entry.bls_holdings))}</span>
                                        {if is_investor_tab {
                                            view! { <span class="col-invested">{format!("${}", format_number(entry.total_invested_usd))}</span> }.into_any()
                                        } else {
                                            view! { <span class="col-score">{format_number(entry.contribution_score)}</span> }.into_any()
                                        }}
                                        <span class={format!("col-tier {}", entry.tier.css_class())}>{entry.tier.display_name()}</span>
                                        <span class="col-node">{entry.node_type}</span>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }
                }}
            </section>

            // Info section
            <section class="leaderboard-info">
                <div class="info-card">
                    <h3>"Anonymous Investing"</h3>
                    <p>
                        "You don't need an account to invest. Treasury contributions via Stripe "
                        "are anonymous by default. Log in with your Eustress Identity to claim "
                        "your position on the leaderboard and display your username."
                    </p>
                </div>
                <div class="info-card">
                    <h3>"How Rankings Work"</h3>
                    <p>
                        "Investors are ranked by total USD invested in the treasury. "
                        "Contributors are ranked by cumulative contribution score. "
                        "Both earn BLS through their respective paths."
                    </p>
                </div>
            </section>

            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn format_number(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else if n == 0.0 {
        "0".to_string()
    } else {
        format!("{:.0}", n)
    }
}
