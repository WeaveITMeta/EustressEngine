// =============================================================================
// Eustress Web - Online Trust Registry Page (Industrial Design)
// =============================================================================
// Transparency dashboard for Bliss fork fairness. Shows BLS-per-contribution
// rates and deviation from median. Includes fork management: registration,
// revocation, and per-fork balance visibility.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Trust status for a fork.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustStatus {
    Trusted,
    Warning,
    Revoked,
}

impl TrustStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Trusted => "Trusted",
            Self::Warning => "Warning",
            Self::Revoked => "Revoked",
        }
    }

    fn css_class(&self) -> &'static str {
        match self {
            Self::Trusted => "status-trusted",
            Self::Warning => "status-warning",
            Self::Revoked => "status-revoked",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::Trusted => "✓",
            Self::Warning => "⚠",
            Self::Revoked => "✕",
        }
    }
}

/// A registered fork entry in the trust registry.
#[derive(Clone, Debug, PartialEq)]
pub struct ForkEntry {
    pub fork_id: String,
    pub rate: f64,
    pub median_rate: f64,
    pub deviation_pct: f64,
    pub active_users: u64,
    pub total_issued: u64,
    pub status: TrustStatus,
    pub registered_date: &'static str,
    pub endpoint: String,
    pub accounts_affected: u32,
    pub revoked_amount: u64,
}

/// A revocation event.
#[derive(Clone, Debug)]
pub struct RevocationEvent {
    pub fork_id: String,
    pub timestamp: &'static str,
    pub reason: String,
    pub accounts_affected: u32,
    pub bls_revoked: u64,
    pub revoked_by: String,
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Returns a CSS class for deviation color coding.
fn deviation_class(deviation: f64) -> &'static str {
    let abs = deviation.abs();
    if abs <= 25.0 {
        "deviation-green"
    } else if abs <= 50.0 {
        "deviation-yellow"
    } else {
        "deviation-red"
    }
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Online Trust Registry page.
#[component]
pub fn TrustRegistryPage() -> impl IntoView {
    let _app_state = expect_context::<AppState>();

    // State
    let search_query = RwSignal::new(String::new());
    let active_tab = RwSignal::new("registry".to_string());
    let selected_fork = RwSignal::new(Option::<String>::None);
    let show_revoke_modal = RwSignal::new(false);
    let revoke_reason = RwSignal::new(String::new());

    // Median rate across all forks
    let median_rate: f64 = 4.2;

    // Sample fork data
    let fork_entries = vec![
        ForkEntry {
            fork_id: "eustress.dev".to_string(),
            rate: 4.1,
            median_rate,
            deviation_pct: -2.4,
            active_users: 12_450,
            total_issued: 3_200_000,
            status: TrustStatus::Trusted,
            registered_date: "2026-01-15",
            endpoint: "https://eustress.dev/.well-known/bliss/fork.json".to_string(),
            accounts_affected: 0,
            revoked_amount: 0,
        },
        ForkEntry {
            fork_id: "neovia.fork".to_string(),
            rate: 5.0,
            median_rate,
            deviation_pct: 19.0,
            active_users: 8_320,
            total_issued: 1_850_000,
            status: TrustStatus::Trusted,
            registered_date: "2026-02-01",
            endpoint: "https://neovia.fork/.well-known/bliss/fork.json".to_string(),
            accounts_affected: 0,
            revoked_amount: 0,
        },
        ForkEntry {
            fork_id: "studio.example".to_string(),
            rate: 6.3,
            median_rate,
            deviation_pct: 50.0,
            active_users: 3_100,
            total_issued: 920_000,
            status: TrustStatus::Warning,
            registered_date: "2026-02-15",
            endpoint: "https://studio.example/.well-known/bliss/fork.json".to_string(),
            accounts_affected: 0,
            revoked_amount: 0,
        },
        ForkEntry {
            fork_id: "creative.hub".to_string(),
            rate: 3.5,
            median_rate,
            deviation_pct: -16.7,
            active_users: 5_670,
            total_issued: 1_420_000,
            status: TrustStatus::Trusted,
            registered_date: "2026-01-20",
            endpoint: "https://creative.hub/.well-known/bliss/fork.json".to_string(),
            accounts_affected: 0,
            revoked_amount: 0,
        },
        ForkEntry {
            fork_id: "rogue.fork".to_string(),
            rate: 8.9,
            median_rate,
            deviation_pct: 111.9,
            active_users: 0,
            total_issued: 78_000,
            status: TrustStatus::Revoked,
            registered_date: "2026-03-01",
            endpoint: "https://rogue.fork/.well-known/bliss/fork.json".to_string(),
            accounts_affected: 245,
            revoked_amount: 78_000,
        },
    ];

    // Revocation history
    let revocation_log = vec![
        RevocationEvent {
            fork_id: "rogue.fork".to_string(),
            timestamp: "2026-03-15 14:22 UTC",
            reason: "Rate manipulation — BLS issuance 112% above median with no legitimate contribution history".to_string(),
            accounts_affected: 245,
            bls_revoked: 78_000,
            revoked_by: "community-vote-#47".to_string(),
        },
    ];

    // KPI values
    let total_forks = fork_entries.len();
    let trusted_count = fork_entries.iter().filter(|f| f.status == TrustStatus::Trusted).count();
    let revoked_count = fork_entries.iter().filter(|f| f.status == TrustStatus::Revoked).count();
    let total_revoked_bls: u64 = fork_entries.iter().map(|f| f.revoked_amount).sum();

    // Filter entries by search
    let filter_entries = move |entries: Vec<ForkEntry>| {
        let query = search_query.get().to_lowercase();
        if query.is_empty() {
            entries
        } else {
            entries.into_iter()
                .filter(|e| e.fork_id.to_lowercase().contains(&query))
                .collect()
        }
    };

    view! {
        <div class="page page-trust-registry-industrial">
            <CentralNav active="community".to_string() />

            // Background
            <div class="trust-registry-page-bg">
                <div class="trust-registry-grid-overlay"></div>
                <div class="trust-registry-glow glow-1"></div>
                <div class="trust-registry-glow glow-2"></div>
            </div>

            // Hero Section
            <section class="trust-registry-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"TRUST REGISTRY"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="trust-registry-title">"Online Trust Registry"</h1>
                <p class="trust-registry-subtitle">"Transparency is how the community self-governs. Every fork's BLS issuance rate is public."</p>
            </section>

            // KPI Dashboard
            <section class="trust-registry-kpis">
                <div class="kpi-row">
                    <div class="kpi-card">
                        <span class="kpi-value">{total_forks.to_string()}</span>
                        <span class="kpi-label">"Registered Forks"</span>
                    </div>
                    <div class="kpi-card kpi-trusted">
                        <span class="kpi-value">{trusted_count.to_string()}</span>
                        <span class="kpi-label">"Trusted"</span>
                    </div>
                    <div class="kpi-card kpi-revoked">
                        <span class="kpi-value">{revoked_count.to_string()}</span>
                        <span class="kpi-label">"Revoked"</span>
                    </div>
                    <div class="kpi-card">
                        <span class="kpi-value">{format!("{:.1}", median_rate)}</span>
                        <span class="kpi-label">"Median Rate (BLS/contrib)"</span>
                    </div>
                    <div class="kpi-card kpi-revoked">
                        <span class="kpi-value">{format_number(total_revoked_bls)}</span>
                        <span class="kpi-label">"BLS Revoked"</span>
                    </div>
                </div>
            </section>

            // Tab bar
            <section class="trust-registry-tabs">
                <div class="tab-bar">
                    <button
                        class="tab-btn"
                        class:active=move || active_tab.get() == "registry"
                        on:click=move |_| active_tab.set("registry".to_string())
                    >"Fork Registry"</button>
                    <button
                        class="tab-btn"
                        class:active=move || active_tab.get() == "revocations"
                        on:click=move |_| active_tab.set("revocations".to_string())
                    >"Revocation Log"</button>
                    <button
                        class="tab-btn"
                        class:active=move || active_tab.get() == "howto"
                        on:click=move |_| active_tab.set("howto".to_string())
                    >"How It Works"</button>
                </div>
            </section>

            // Controls (search)
            <section class="trust-registry-controls">
                <div class="filters-row">
                    <div class="search-box">
                        <input
                            type="text"
                            class="search-input-industrial"
                            placeholder="Search forks..."
                            prop:value=move || search_query.get()
                            on:input=move |e| search_query.set(event_target_value(&e))
                        />
                    </div>
                </div>
            </section>

            // Content
            <section class="trust-registry-content">
                {move || {
                    let tab = active_tab.get();
                    if tab == "registry" {
                        view! {
                            <div class="registry-panel">
                                // Registry Table
                                <div class="leaderboard-table-full">
                                    <div class="table-header-full">
                                        <span class="col-status-icon"></span>
                                        <span class="col-fork-id">"Fork ID"</span>
                                        <span class="col-rate">"Rate"</span>
                                        <span class="col-deviation">"Deviation"</span>
                                        <span class="col-active-users">"Users"</span>
                                        <span class="col-total-issued">"Issued"</span>
                                        <span class="col-status">"Status"</span>
                                        <span class="col-actions">"Actions"</span>
                                    </div>
                                    {filter_entries(fork_entries.clone()).into_iter().map(|entry| {
                                        let dev_class = deviation_class(entry.deviation_pct);
                                        let status_class = entry.status.css_class();
                                        let status_label = entry.status.as_str();
                                        let status_icon = entry.status.icon();
                                        let deviation_display = if entry.deviation_pct >= 0.0 {
                                            format!("+{:.1}%", entry.deviation_pct)
                                        } else {
                                            format!("{:.1}%", entry.deviation_pct)
                                        };
                                        let fork_id_clone = entry.fork_id.clone();
                                        let is_revoked = entry.status == TrustStatus::Revoked;
                                        view! {
                                            <div class={format!("table-row-full {}", if is_revoked { "row-revoked" } else { "" })}>
                                                <span class={format!("col-status-icon {}", status_class)}>
                                                    {status_icon}
                                                </span>
                                                <span class="fork-id-cell">
                                                    <span class="fork-id-name">{entry.fork_id.clone()}</span>
                                                    <span class="fork-id-date">{entry.registered_date}</span>
                                                </span>
                                                <span class="rate-cell">{format!("{:.1}", entry.rate)}</span>
                                                <span class={format!("deviation-cell {}", dev_class)}>
                                                    {deviation_display}
                                                </span>
                                                <span class="active-users-cell">{format_number(entry.active_users)}</span>
                                                <span class="total-issued-cell">{format_number(entry.total_issued)}" BLS"</span>
                                                <span class={format!("status-cell {}", status_class)}>
                                                    {status_label}
                                                </span>
                                                <span class="actions-cell">
                                                    {if !is_revoked {
                                                        let fid = fork_id_clone.clone();
                                                        view! {
                                                            <button
                                                                class="btn-action btn-revoke"
                                                                title="Initiate revocation"
                                                                on:click=move |_| {
                                                                    selected_fork.set(Some(fid.clone()));
                                                                    show_revoke_modal.set(true);
                                                                }
                                                            >"Revoke"</button>
                                                        }.into_any()
                                                    } else {
                                                        view! {
                                                            <span class="revoked-label">{format!("{} accounts", entry.accounts_affected)}</span>
                                                        }.into_any()
                                                    }}
                                                </span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            </div>
                        }.into_any()
                    } else if tab == "revocations" {
                        view! {
                            <div class="revocations-panel">
                                <h3 class="panel-title">"Revocation History"</h3>
                                <p class="panel-desc">"All fork revocations are permanent and public. Revoked balances are deducted from every account that received BLS from the revoked fork."</p>
                                {revocation_log.clone().into_iter().map(|event| {
                                    view! {
                                        <div class="revocation-card">
                                            <div class="revocation-header">
                                                <span class="revocation-fork">{event.fork_id}</span>
                                                <span class="revocation-time">{event.timestamp}</span>
                                            </div>
                                            <div class="revocation-body">
                                                <p class="revocation-reason">{event.reason}</p>
                                                <div class="revocation-stats">
                                                    <div class="revocation-stat">
                                                        <span class="stat-value">{event.accounts_affected.to_string()}</span>
                                                        <span class="stat-label">"Accounts Affected"</span>
                                                    </div>
                                                    <div class="revocation-stat">
                                                        <span class="stat-value">{format_number(event.bls_revoked)} " BLS"</span>
                                                        <span class="stat-label">"Balance Revoked"</span>
                                                    </div>
                                                    <div class="revocation-stat">
                                                        <span class="stat-value">{event.revoked_by}</span>
                                                        <span class="stat-label">"Initiated By"</span>
                                                    </div>
                                                </div>
                                            </div>
                                            <div class="revocation-note">
                                                "Per-fork ledger tracking ensures only BLS issued through this fork was revoked. "
                                                "Direct contributions and other fork balances are preserved."
                                            </div>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}

                                {if revocation_log.is_empty() {
                                    Some(view! {
                                        <div class="empty-state">
                                            <p>"No revocations recorded."</p>
                                        </div>
                                    })
                                } else {
                                    None
                                }}
                            </div>
                        }.into_any()
                    } else {
                        // How it works tab
                        view! {
                            <div class="howto-panel">
                                <div class="howto-grid">
                                    <div class="howto-card">
                                        <div class="howto-number">"1"</div>
                                        <h4>"Fork Registration"</h4>
                                        <p>
                                            "Every fork registers by hosting a "
                                            <code>"/.well-known/bliss/fork.json"</code>
                                            " endpoint. The witness worker verifies ownership and adds the fork to the registry."
                                        </p>
                                    </div>
                                    <div class="howto-card">
                                        <div class="howto-number">"2"</div>
                                        <h4>"Rate Monitoring"</h4>
                                        <p>
                                            "Each fork's BLS-per-contribution rate is tracked and compared to the network median. "
                                            "Forks more than 50% above median are flagged for review."
                                        </p>
                                    </div>
                                    <div class="howto-card">
                                        <div class="howto-number">"3"</div>
                                        <h4>"Per-Fork Ledger"</h4>
                                        <p>
                                            "Every BLS credited via ForkBridge records its source fork. Each account tracks a per-fork "
                                            "balance breakdown: fork A contributed X BLS, fork B contributed Y BLS."
                                        </p>
                                    </div>
                                    <div class="howto-card">
                                        <div class="howto-number">"4"</div>
                                        <h4>"Community Revocation"</h4>
                                        <p>
                                            "If a fork is found to be gaming the system, the community can initiate revocation. "
                                            "This permanently removes the fork and deducts its contributed BLS from all accounts."
                                        </p>
                                    </div>
                                    <div class="howto-card">
                                        <div class="howto-number">"5"</div>
                                        <h4>"Surgical Revocation"</h4>
                                        <p>
                                            "Revocation only affects BLS that came from the revoked fork. Direct contribution "
                                            "earnings and BLS from other trusted forks are never touched."
                                        </p>
                                    </div>
                                    <div class="howto-card">
                                        <div class="howto-number">"6"</div>
                                        <h4>"Daily Distribution"</h4>
                                        <p>
                                            "Contributors are paid daily at UTC midnight. Each day's emission + treasury drip "
                                            "is split proportionally by contribution score. Top contributors get a boost "
                                            "when the treasury enters scarcity mode."
                                        </p>
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    }
                }}
            </section>

            // Revocation Modal
            {move || show_revoke_modal.get().then(|| {
                let fork = selected_fork.get().unwrap_or_default();
                view! {
                    <div class="modal-overlay" on:click=move |_| show_revoke_modal.set(false)>
                        <div class="modal-content revoke-modal" on:click=|e| e.stop_propagation()>
                            <h3 class="modal-title">"Revoke Fork"</h3>
                            <p class="modal-desc">
                                "You are about to initiate revocation of "
                                <strong>{fork.clone()}</strong>
                                ". This action is permanent and will:"
                            </p>
                            <ul class="revoke-consequences">
                                <li>"Remove the fork from the trust registry"</li>
                                <li>"Deduct all BLS credited via this fork from every affected account"</li>
                                <li>"Preserve direct contribution earnings and other fork balances"</li>
                                <li>"Record the revocation in the public log"</li>
                            </ul>

                            <div class="form-field">
                                <label class="form-label">"Reason for revocation"</label>
                                <textarea
                                    class="form-input revoke-reason"
                                    placeholder="Describe the reason for revocation..."
                                    prop:value=move || revoke_reason.get()
                                    on:input=move |e| revoke_reason.set(event_target_value(&e))
                                ></textarea>
                            </div>

                            <div class="modal-actions">
                                <button
                                    class="btn btn-secondary"
                                    on:click=move |_| show_revoke_modal.set(false)
                                >"Cancel"</button>
                                <button
                                    class="btn btn-danger"
                                    disabled=move || revoke_reason.get().trim().is_empty()
                                    on:click=move |_| {
                                        // In production, this calls the API to revoke the fork
                                        show_revoke_modal.set(false);
                                        revoke_reason.set(String::new());
                                        selected_fork.set(None);
                                    }
                                >"Confirm Revocation"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            <Footer />
        </div>
    }
}
