// =============================================================================
// Eustress Web - Admin Analytics (/admin)
// =============================================================================
// Single-admin dashboard for the "unique visits per sign-up" funnel.
// Data comes from GET /api/admin/stats on the Cloudflare Worker, which is gated
// server-side by requireAdmin() (your record has role: "admin"). The page only
// *displays* — the real authorization lives in the Worker, which returns 403 to
// non-admins (rendered here as the "access denied" state).
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::{ApiClient, ApiError};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Response model (mirrors handleAdminStats in infrastructure/cloudflare/api)
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct DayPoint {
    date: String,
    count: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct KeyCount {
    key: String,
    count: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Signups {
    total: i64,
    by_day: Vec<DayPoint>,
    by_decision: Vec<KeyCount>,
    by_id_type: Vec<KeyCount>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Visits {
    total_pageviews: i64,
    total_unique: i64,
    unique_by_day: Vec<DayPoint>,
    #[allow(dead_code)]
    pageviews_by_day: Vec<DayPoint>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Funnel {
    unique_visits_per_signup: f64,
    signup_conversion_rate: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Accounts {
    admins: i64,
    banned: i64,
    with_email: i64,
    stripe_connected: i64,
    bliss_total: i64,
    capped: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct AdminStats {
    generated_at: String,
    analytics_enabled: bool,
    signups: Signups,
    visits: Visits,
    funnel: Funnel,
    accounts: Accounts,
}

#[derive(Clone, Debug, PartialEq)]
enum Load {
    Loading,
    Denied,
    Error(String),
    Ready(AdminStats),
}

// -----------------------------------------------------------------------------
// Small render helpers
// -----------------------------------------------------------------------------

/// A compact CSS bar chart of the last ~30 days of a day-series.
fn day_bars(points: &[DayPoint]) -> impl IntoView {
    let max = points.iter().map(|p| p.count).max().unwrap_or(1).max(1);
    let recent: Vec<DayPoint> = points.iter().rev().take(30).rev().cloned().collect();
    if recent.is_empty() {
        return view! { <p class="admin-empty">"No data in range yet."</p> }.into_any();
    }
    let bars = recent
        .into_iter()
        .map(|p| {
            let pct = (p.count as f64 / max as f64 * 100.0).max(3.0);
            let style = format!("height:{:.1}%", pct);
            let title = format!("{} — {}", p.date, p.count);
            view! {
                <div class="admin-bar" title=title>
                    <div class="admin-bar-fill" style=style></div>
                </div>
            }
            .into_any()
        })
        .collect::<Vec<_>>();
    view! { <div class="admin-bars">{bars}</div> }.into_any()
}

/// A simple two-column breakdown table.
fn kv_rows(points: &[KeyCount]) -> impl IntoView {
    if points.is_empty() {
        return view! { <p class="admin-empty">"No data."</p> }.into_any();
    }
    let rows = points
        .iter()
        .cloned()
        .map(|p| {
            view! {
                <tr>
                    <td class="admin-kv-key">{p.key}</td>
                    <td class="admin-kv-count">{p.count}</td>
                </tr>
            }
            .into_any()
        })
        .collect::<Vec<_>>();
    view! { <table class="admin-table"><tbody>{rows}</tbody></table> }.into_any()
}

// -----------------------------------------------------------------------------
// Page component
// -----------------------------------------------------------------------------

/// Admin analytics dashboard.
#[component]
pub fn AdminPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = app_state.api_url.clone();
    let state = RwSignal::new(Load::Loading);

    // Fetch once on mount. The Worker gate decides admin-ness; a 401/403 maps
    // to the "denied" state regardless of any client-side guess.
    Effect::new(move |_| {
        let api_url = api_url.clone();
        spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match client.get::<AdminStats>("/api/admin/stats").await {
                Ok(stats) => state.set(Load::Ready(stats)),
                Err(ApiError::Unauthorized) => state.set(Load::Denied),
                Err(ApiError::Server { status: 403, .. }) => state.set(Load::Denied),
                Err(e) => state.set(Load::Error(e.to_string())),
            }
        });
    });

    let body = move || match state.get() {
        Load::Loading => view! {
            <div class="loading-state">
                <div class="spinner"></div>
                <p>"Loading analytics…"</p>
            </div>
        }
        .into_any(),

        Load::Denied => view! {
            <div class="admin-denied">
                <h2>"Admin access required"</h2>
                <p>"This dashboard is restricted to the Eustress admin. Sign in with the admin identity to continue."</p>
                <a class="btn-primary" href="/login">"Sign in"</a>
            </div>
        }
        .into_any(),

        Load::Error(msg) => view! {
            <div class="alert alert-error admin-alert">
                <strong>"Couldn't load analytics. "</strong>
                <span>{msg}</span>
            </div>
        }
        .into_any(),

        Load::Ready(s) => {
            let vps = if s.analytics_enabled {
                format!("{:.2}", s.funnel.unique_visits_per_signup)
            } else {
                "—".to_string()
            };
            let conv = if s.analytics_enabled {
                format!("{:.2}%", s.funnel.signup_conversion_rate)
            } else {
                "—".to_string()
            };
            let uniques = if s.analytics_enabled { s.visits.total_unique.to_string() } else { "—".to_string() };
            let pageviews = if s.analytics_enabled { s.visits.total_pageviews.to_string() } else { "—".to_string() };

            let analytics_off_banner = (!s.analytics_enabled).then(|| {
                view! {
                    <div class="alert admin-notice">
                        <strong>"Visit tracking isn't live yet. "</strong>
                        <span>"Sign-up metrics are real; visit/conversion figures will populate once the ANALYTICS KV namespace is bound and the beacon starts recording."</span>
                    </div>
                }
            });

            view! {
                <div class="admin-wrap">
                    {analytics_off_banner}

                    // Headline funnel
                    <section class="admin-headline">
                        <div class="admin-hero-card admin-hero-primary">
                            <span class="admin-hero-value">{vps}</span>
                            <span class="admin-hero-label">"Unique visits / sign-up"</span>
                        </div>
                        <div class="admin-hero-card">
                            <span class="admin-hero-value">{conv}</span>
                            <span class="admin-hero-label">"Sign-up conversion"</span>
                        </div>
                        <div class="admin-hero-card">
                            <span class="admin-hero-value">{s.signups.total}</span>
                            <span class="admin-hero-label">"Total sign-ups"</span>
                        </div>
                        <div class="admin-hero-card">
                            <span class="admin-hero-value">{uniques}</span>
                            <span class="admin-hero-label">"Unique visits"</span>
                        </div>
                        <div class="admin-hero-card">
                            <span class="admin-hero-value">{pageviews}</span>
                            <span class="admin-hero-label">"Pageviews"</span>
                        </div>
                    </section>

                    // Sign-ups over time
                    <section class="dashboard-section">
                        <div class="section-header-industrial">
                            <h2>"Sign-ups / day"</h2>
                        </div>
                        {day_bars(&s.signups.by_day)}
                    </section>

                    // Unique visits over time
                    <section class="dashboard-section">
                        <div class="section-header-industrial">
                            <h2>"Unique visits / day"</h2>
                        </div>
                        {day_bars(&s.visits.unique_by_day)}
                    </section>

                    // Breakdowns
                    <section class="admin-grid-2">
                        <div class="dashboard-section">
                            <div class="section-header-industrial"><h2>"Sign-ups by KYC decision"</h2></div>
                            {kv_rows(&s.signups.by_decision)}
                        </div>
                        <div class="dashboard-section">
                            <div class="section-header-industrial"><h2>"Sign-ups by ID type"</h2></div>
                            {kv_rows(&s.signups.by_id_type)}
                        </div>
                    </section>

                    // Account health
                    <section class="dashboard-section">
                        <div class="section-header-industrial"><h2>"Accounts"</h2></div>
                        <div class="stats-grid">
                            <div class="stat-card"><span class="stat-value">{s.accounts.admins}</span><span class="stat-label">"Admins"</span></div>
                            <div class="stat-card"><span class="stat-value">{s.accounts.with_email}</span><span class="stat-label">"With email"</span></div>
                            <div class="stat-card"><span class="stat-value">{s.accounts.stripe_connected}</span><span class="stat-label">"Stripe-connected"</span></div>
                            <div class="stat-card"><span class="stat-value">{s.accounts.banned}</span><span class="stat-label">"Banned"</span></div>
                            <div class="stat-card"><span class="stat-value">{s.accounts.bliss_total}</span><span class="stat-label">"Bliss (total)"</span></div>
                        </div>
                    </section>

                    <p class="admin-generated">"Generated " {s.generated_at}</p>
                </div>
            }
            .into_any()
        }
    };

    view! {
        <div class="page page-admin">
            <CentralNav active="admin".to_string() />
            <section class="dashboard-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"ADMIN"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="dashboard-title">"Analytics"</h1>
                <p class="dashboard-subtitle">"Visits, sign-ups, and the conversion funnel."</p>
            </section>

            <div class="dashboard-container">
                {body}
            </div>

            <Footer />
        </div>
    }
}
