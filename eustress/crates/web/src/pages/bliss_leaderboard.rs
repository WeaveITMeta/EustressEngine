// =============================================================================
// Eustress Web - Bliss Leaderboard
// =============================================================================
// The accounts that earned the most BLS over the last 7 or 30 settled days,
// read from `/api/ledger/leaderboard`, which sums the public daily
// distribution records. Accounts appear by the same ids those records
// publish; a signed-in contributor sees their own row marked.
//
// BLS is earned by contributing. Funding the treasury earns none, so there is
// no ranking of money put in.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

const API_URL: &str = "https://api.eustress.dev";

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct LeaderRow {
    rank: u32,
    account: String,
    bls: f64,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    days_active: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct Leaderboard {
    days: u32,
    #[serde(default)]
    settled_days: u32,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    contributors: u32,
    #[serde(default)]
    entries: Vec<LeaderRow>,
}

/// 2dp with thousands separators, the ledger's own precision.
fn fmt_bls(v: f64) -> String {
    let cents = (v.abs() * 100.0).round() as u64;
    let digits = (cents / 100).to_string();
    let mut s = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            s.push(',');
        }
        s.push(c);
    }
    format!("{}{}.{:02}", if v < 0.0 { "-" } else { "" }, s, cents % 100)
}

/// The short form of an account id shown in the table: its first 8
/// characters, which is how the public distribution records can be matched.
fn account_tag(id: &str) -> String {
    format!("Contributor {}", id.get(..8).unwrap_or(id))
}

#[component]
pub fn BlissLeaderboardPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let days = RwSignal::new(30_u32);
    let board = RwSignal::new(Option::<Leaderboard>::None);
    let error = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(true);

    Effect::new(move |_| {
        let window = days.get();
        loading.set(true);
        spawn_local(async move {
            let url = format!("{}/api/ledger/leaderboard?days={}", API_URL, window);
            match gloo_net::http::Request::get(&url).send().await {
                Ok(resp) if resp.ok() => match resp.json::<Leaderboard>().await {
                    Ok(data) => {
                        board.set(Some(data));
                        error.set(None);
                    }
                    Err(e) => error.set(Some(format!("Could not read the leaderboard: {e}"))),
                },
                Ok(resp) => error.set(Some(format!("The leaderboard answered {}", resp.status()))),
                Err(e) => error.set(Some(format!("Could not reach the leaderboard: {e}"))),
            }
            loading.set(false);
        });
    });

    let me = move || match app_state.auth.get() {
        AuthState::Authenticated(u) => Some(u.id.to_string()),
        _ => None,
    };

    view! {
        <div class="page page-leaderboard-industrial">
            <CentralNav active="".to_string() />

            <div class="leaderboard-bg">
                <div class="leaderboard-grid-overlay"></div>
                <div class="leaderboard-glow glow-1"></div>
                <div class="leaderboard-glow glow-2"></div>
            </div>

            <section class="leaderboard-hero">
                <div class="hero-header-lines">
                    <span class="header-line"></span>
                    <span class="header-dot"></span>
                    <span class="header-line"></span>
                </div>
                <h1 class="leaderboard-title">"Bliss Leaderboard"</h1>
                <p class="leaderboard-subtitle">"The contributors who earned the most BLS, from the public ledger"</p>
            </section>

            <section class="leaderboard-controls">
                <div class="tab-bar">
                    <button
                        class="tab-btn"
                        class:active=move || days.get() == 7
                        on:click=move |_| days.set(7)
                    >"Last 7 days"</button>
                    <button
                        class="tab-btn"
                        class:active=move || days.get() == 30
                        on:click=move |_| days.set(30)
                    >"Last 30 days"</button>
                </div>
            </section>

            <section class="leaderboard-content">
                {move || {
                    if let Some(msg) = error.get() {
                        return view! { <p class="ledger-hint">{msg}</p> }.into_any();
                    }
                    let Some(b) = board.get() else {
                        return view! {
                            <p class="ledger-hint">{move || if loading.get() { "Loading the leaderboard..." } else { "No data yet." }}</p>
                        }.into_any();
                    };
                    if b.entries.is_empty() {
                        return view! {
                            <p class="ledger-hint">
                                "No BLS has been distributed in this window yet. BLS is credited after "
                                "UTC midnight for the work done the day before."
                            </p>
                        }.into_any();
                    }
                    let mine = me();
                    let window = match (&b.from, &b.to) {
                        (Some(f), Some(t)) if f != t => format!("{f} to {t}"),
                        (Some(f), _) => f.clone(),
                        _ => String::new(),
                    };
                    let podium = b.entries.iter().take(3).cloned().collect::<Vec<_>>();
                    let rows = b.entries.clone();
                    view! {
                        <p class="ledger-hint">
                            {format!(
                                "{} contributors earned BLS over {} settled day{}{}.",
                                b.contributors,
                                b.settled_days,
                                if b.settled_days == 1 { "" } else { "s" },
                                if window.is_empty() { String::new() } else { format!(" ({window})") },
                            )}
                        </p>
                        <div class="podium">
                            {podium.into_iter().enumerate().map(|(i, e)| {
                                let class = match i { 0 => "podium-gold", 1 => "podium-silver", _ => "podium-bronze" };
                                let you = mine.as_deref() == Some(e.account.as_str());
                                view! {
                                    <div class={format!("podium-card {class}")}>
                                        <span class="podium-rank">{format!("#{}", e.rank)}</span>
                                        <span class="podium-name">
                                            {if you { "You".to_string() } else { account_tag(&e.account) }}
                                        </span>
                                        <span class="podium-bls">{format!("{} BLS", fmt_bls(e.bls))}</span>
                                        <span class="podium-score">{format!("Score {:.1}", e.score)}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                        <div class="leaderboard-table">
                            <div class="table-header">
                                <span class="col-rank">"#"</span>
                                <span class="col-name">"Contributor"</span>
                                <span class="col-bls">"BLS earned"</span>
                                <span class="col-score">"Score"</span>
                                <span class="col-node">"Days active"</span>
                            </div>
                            {rows.into_iter().map(|e| {
                                let you = mine.as_deref() == Some(e.account.as_str());
                                view! {
                                    <div class="table-row" class:table-row-you=move || you>
                                        <span class="col-rank">{e.rank.to_string()}</span>
                                        <span class="col-name">
                                            {if you { "You".to_string() } else { account_tag(&e.account) }}
                                        </span>
                                        <span class="col-bls">{fmt_bls(e.bls)}</span>
                                        <span class="col-score">{format!("{:.1}", e.score)}</span>
                                        <span class="col-node">{e.days_active.to_string()}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                }}
            </section>

            <section class="leaderboard-info">
                <div class="info-card">
                    <h3>"How rankings work"</h3>
                    <p>
                        "Accounts are ranked by the BLS the nightly emission credited them over the "
                        "window. Each day's emission is shared by that day's contribution score. "
                        "Every figure here is summed from the public daily distribution records."
                    </p>
                </div>
                <div class="info-card">
                    <h3>"Who is listed"</h3>
                    <p>
                        "Accounts appear by the start of their public account id, the same ids the "
                        "distribution records publish. Sign in and your own row reads You. Funding "
                        "the treasury earns no BLS, so it does not appear here."
                    </p>
                </div>
            </section>

            <Footer />
        </div>
    }
}
