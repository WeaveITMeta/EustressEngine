// =============================================================================
// Eustress Web - Bliss KPI Modal Component
// =============================================================================
// Persistent top-right corner widget showing live Bliss earnings, events, and
// key network stats. Opens into a detailed modal on click.
// =============================================================================

use leptos::prelude::*;

// -----------------------------------------------------------------------------
// KPI Event type
// -----------------------------------------------------------------------------

/// A recent event in the Bliss network relevant to this user.
#[derive(Clone, Debug, PartialEq)]
pub struct BlissEvent {
    pub kind: BlissEventKind,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlissEventKind {
    Earning,
    Distribution,
    ForkUpdate,
    System,
}

impl BlissEventKind {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Earning => "/assets/icons/trending.svg",
            Self::Distribution => "/assets/icons/gift.svg",
            Self::ForkUpdate => "/assets/icons/code.svg",
            Self::System => "/assets/icons/shield.svg",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Self::Earning => "event-earning",
            Self::Distribution => "event-distribution",
            Self::ForkUpdate => "event-fork",
            Self::System => "event-system",
        }
    }
}

// -----------------------------------------------------------------------------
// Bliss KPI Modal Component
// -----------------------------------------------------------------------------

/// Top-right corner KPI widget with expandable modal.
///
/// Shows a compact badge with current BLS balance and pending earnings.
/// Clicking opens a full modal with detailed stats and event feed.
#[component]
pub fn BlissKpiModal() -> impl IntoView {
    let is_open = RwSignal::new(false);

    // User-specific KPIs (in production, fetched from API)
    let balance = RwSignal::new(127.4321_f64);
    let pending = RwSignal::new(8.5_f64);
    let contribution_score = RwSignal::new(23.7_f64);
    let session_earnings = RwSignal::new(1.23_f64);
    let cosigns_today = RwSignal::new(14_u32);
    let streak_days = RwSignal::new(5_u32);

    // Recent events
    let events = RwSignal::new(vec![
        BlissEvent {
            kind: BlissEventKind::Earning,
            message: "Earned 0.42 BLS for building session (32 min)".to_string(),
            timestamp: "2 min ago".to_string(),
        },
        BlissEvent {
            kind: BlissEventKind::Earning,
            message: "Earned 0.31 BLS for scripting session (18 min)".to_string(),
            timestamp: "28 min ago".to_string(),
        },
        BlissEvent {
            kind: BlissEventKind::Distribution,
            message: "Daily distribution: 12.50 BLS credited".to_string(),
            timestamp: "1 day ago".to_string(),
        },
        BlissEvent {
            kind: BlissEventKind::ForkUpdate,
            message: "Fork registry updated — 8 forks active".to_string(),
            timestamp: "3 days ago".to_string(),
        },
        BlissEvent {
            kind: BlissEventKind::System,
            message: "Identity co-sign chain verified (42 entries)".to_string(),
            timestamp: "5 days ago".to_string(),
        },
    ]);

    view! {
        // =====================================================================
        // Compact Badge (always visible, top-right)
        // =====================================================================
        <div class="bliss-kpi-badge" on:click=move |_| is_open.set(!is_open.get())>
            <img src="/assets/icons/bliss.svg" alt="BLS" class="kpi-badge-icon" />
            <div class="kpi-badge-values">
                <span class="kpi-badge-balance">{move || format!("{:.2}", balance.get())}</span>
                <span class="kpi-badge-pending">{move || format!("+{:.2}", pending.get())}</span>
            </div>
            <div class="kpi-badge-pulse"></div>
        </div>

        // =====================================================================
        // Expanded Modal
        // =====================================================================
        {move || {
            if is_open.get() {
                view! {
                    <div class="bliss-kpi-overlay" on:click=move |_| is_open.set(false)>
                        <div class="bliss-kpi-modal" on:click=|e| e.stop_propagation()>
                            // Header
                            <div class="kpi-modal-header">
                                <div class="kpi-modal-title">
                                    <img src="/assets/icons/bliss.svg" alt="BLS" />
                                    <h3>"Bliss Dashboard"</h3>
                                </div>
                                <button class="kpi-modal-close" on:click=move |_| is_open.set(false)>
                                    "X"
                                </button>
                            </div>

                            // Balance section
                            <div class="kpi-modal-balance">
                                <div class="kpi-balance-main">
                                    <span class="kpi-balance-label">"Balance"</span>
                                    <span class="kpi-balance-value">{move || format!("{:.4} BLS", balance.get())}</span>
                                </div>
                                <div class="kpi-balance-secondary">
                                    <div class="kpi-stat">
                                        <span class="kpi-stat-label">"Pending"</span>
                                        <span class="kpi-stat-value kpi-pending">{move || format!("+{:.4} BLS", pending.get())}</span>
                                    </div>
                                    <div class="kpi-stat">
                                        <span class="kpi-stat-label">"This Session"</span>
                                        <span class="kpi-stat-value">{move || format!("+{:.4} BLS", session_earnings.get())}</span>
                                    </div>
                                </div>
                            </div>

                            // Quick stats grid
                            <div class="kpi-modal-stats">
                                <div class="kpi-mini-stat">
                                    <span class="kpi-mini-label">"Contribution Score"</span>
                                    <span class="kpi-mini-value">{move || format!("{:.1}", contribution_score.get())}</span>
                                </div>
                                <div class="kpi-mini-stat">
                                    <span class="kpi-mini-label">"Co-signs Today"</span>
                                    <span class="kpi-mini-value">{move || cosigns_today.get().to_string()}</span>
                                </div>
                                <div class="kpi-mini-stat">
                                    <span class="kpi-mini-label">"Streak"</span>
                                    <span class="kpi-mini-value">{move || format!("{} days", streak_days.get())}</span>
                                </div>
                            </div>

                            // Event feed
                            <div class="kpi-modal-events">
                                <h4>"Recent Activity"</h4>
                                <div class="kpi-events-list">
                                    {move || {
                                        events.get().into_iter().map(|event| {
                                            let css = event.kind.css_class();
                                            let icon = event.kind.icon();
                                            view! {
                                                <div class={format!("kpi-event {}", css)}>
                                                    <img src={icon} alt="" class="kpi-event-icon" />
                                                    <div class="kpi-event-content">
                                                        <span class="kpi-event-msg">{event.message}</span>
                                                        <span class="kpi-event-time">{event.timestamp}</span>
                                                    </div>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()
                                    }}
                                </div>
                            </div>

                            // Footer link
                            <a href="/bliss" class="kpi-modal-link">
                                "View Full Dashboard"
                                <img src="/assets/icons/arrow-right.svg" alt="" />
                            </a>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }
        }}
    }
}
