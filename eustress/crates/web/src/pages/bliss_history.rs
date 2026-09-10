// =============================================================================
// Eustress Web - Bliss Wallet Ledger
// =============================================================================
// A contributor's own BLS history: every append-only ledger entry, a running
// balance, and a cumulative chart. Reads the public transparency endpoint
// `/api/ledger/history/{id}`, which returns entries already grouped by day
// with a cumulative total.
//
// Balances are integer minor units server-side (1 BLS = 100), so every figure
// here is rendered at exactly 2 decimals — never more.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

const API_URL: &str = "https://api.eustress.dev";

/// One day of ledger activity for this account.
#[derive(Debug, Clone, Deserialize, PartialEq)]
struct LedgerPoint {
    /// ISO date, or the literal "opening" for the migrated pre-ledger balance.
    date: String,
    change: f64,
    cumulative: f64,
    #[serde(default)]
    kinds: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct LedgerHistory {
    #[serde(default)]
    balance: f64,
    #[serde(default)]
    points: u64,
    #[serde(default)]
    series: Vec<LedgerPoint>,
}

/// 2dp with thousands separators — matches the ledger's own precision.
fn fmt_bls(v: f64) -> String {
    let neg = v < 0.0;
    let cents = (v.abs() * 100.0).round() as u64;
    let whole = cents / 100;
    let frac = cents % 100;
    let mut s = String::new();
    let digits = whole.to_string();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            s.push(',');
        }
        s.push(c);
    }
    format!("{}{}.{:02}", if neg { "-" } else { "" }, s, frac)
}

/// Is this the migrated opening entry rather than a daily emission credit?
fn is_opening(p: &LedgerPoint) -> bool {
    p.date == "opening" || p.kinds.iter().any(|k| k == "migration_opening")
}

/// Cumulative area + line chart. Hand-built SVG — a charting library would be
/// far more weight than a single series needs.
fn chart_view(series: &[LedgerPoint]) -> impl IntoView {
    const W: f64 = 900.0;
    const H: f64 = 260.0;
    const L: f64 = 78.0;
    const R: f64 = 18.0;
    const T: f64 = 14.0;
    const B: f64 = 30.0;

    if series.is_empty() {
        return view! { <p class="ledger-empty">"No entries yet."</p> }.into_any();
    }

    let iw = W - L - R;
    let ih = H - T - B;
    let max_y = series.iter().fold(0.0_f64, |m, p| m.max(p.cumulative)).max(1.0);
    // Round the axis top to a clean step so each tick names a real value.
    let step = 10_f64.powf((max_y / 4.0).log10().floor());
    let top = (max_y / step).ceil() * step;

    let n = series.len().max(2) as f64;
    let x_at = |i: usize| L + (i as f64 / (n - 1.0)) * iw;
    let y_at = |v: f64| T + ih - (v / top) * ih;

    let mut line = String::new();
    let mut area = format!("M {:.1} {:.1}", x_at(0), y_at(0.0));
    for (i, p) in series.iter().enumerate() {
        let (x, y) = (x_at(i), y_at(p.cumulative));
        if i == 0 {
            line.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            line.push_str(&format!(" L {x:.1} {y:.1}"));
        }
        area.push_str(&format!(" L {x:.1} {y:.1}"));
    }
    area.push_str(&format!(
        " L {:.1} {:.1} Z",
        x_at(series.len() - 1),
        y_at(0.0)
    ));

    // Y ticks
    let ticks: Vec<(f64, String)> = (0..=4)
        .map(|i| {
            let v = top / 4.0 * i as f64;
            (y_at(v), fmt_bls(v))
        })
        .collect();

    // X labels — first, last, and a few between so they never crowd.
    let every = ((series.len() as f64 / 6.0).ceil() as usize).max(1);
    let xlabels: Vec<(f64, String)> = series
        .iter()
        .enumerate()
        .filter(|(i, _)| *i == 0 || *i == series.len() - 1 || i % every == 0)
        .map(|(i, p)| {
            let label = if is_opening(p) {
                "open".to_string()
            } else {
                p.date.get(5..).unwrap_or(&p.date).to_string()
            };
            (x_at(i), label)
        })
        .collect();

    let last_x = x_at(series.len() - 1);
    let last_y = y_at(series[series.len() - 1].cumulative);

    view! {
        <svg class="ledger-chart" viewBox=format!("0 0 {W} {H}") role="img"
             aria-label="Cumulative Bliss balance">
            <defs>
                <linearGradient id="blissFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stop-color="var(--bliss-accent, #00bcd4)" stop-opacity="0.28" />
                    <stop offset="100%" stop-color="var(--bliss-accent, #00bcd4)" stop-opacity="0" />
                </linearGradient>
            </defs>
            {ticks.into_iter().map(|(y, label)| view! {
                <g>
                    <line x1=L y1=y x2=(W - R) y2=y class="ledger-grid" />
                    <text x=(L - 10.0) y=(y + 4.0) text-anchor="end" class="ledger-axis">{label}</text>
                </g>
            }).collect_view()}
            <path d=area fill="url(#blissFill)" />
            <path d=line class="ledger-line" fill="none" />
            <circle cx=last_x cy=last_y r="7" class="ledger-endpoint-halo" />
            <circle cx=last_x cy=last_y r="3.5" class="ledger-endpoint" />
            {xlabels.into_iter().map(|(x, label)| view! {
                <text x=x y=(H - 9.0) text-anchor="middle" class="ledger-axis">{label}</text>
            }).collect_view()}
        </svg>
    }
    .into_any()
}

/// Bliss wallet ledger — the signed-in contributor's own entry history.
#[component]
pub fn BlissHistoryPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let history = RwSignal::new(Option::<LedgerHistory>::None);
    let error = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(true);

    // Fetch whenever we know who the user is. `auth` starts as Unknown while
    // the session is restored from localStorage, so this re-runs once it
    // resolves rather than firing a request with no account id.
    Effect::new(move |_| {
        let user_id = match app_state.auth.get() {
            AuthState::Authenticated(u) => Some(u.id.to_string()),
            AuthState::Unauthenticated => {
                loading.set(false);
                None
            }
            AuthState::Unknown => None,
        };
        let Some(user_id) = user_id else { return };

        loading.set(true);
        spawn_local(async move {
            let url = format!("{}/api/ledger/history/{}", API_URL, user_id);
            match gloo_net::http::Request::get(&url).send().await {
                Ok(resp) => match resp.json::<LedgerHistory>().await {
                    Ok(data) => {
                        history.set(Some(data));
                        error.set(None);
                    }
                    Err(e) => error.set(Some(format!("Could not read the ledger: {e}"))),
                },
                Err(e) => error.set(Some(format!("Could not reach the ledger: {e}"))),
            }
            loading.set(false);
        });
    });

    view! {
        <div class="page page-ledger">
            <CentralNav active="".to_string() />

            <div class="ledger-wrap">
                <header class="ledger-head">
                    <div>
                        <h1>"Wallet Ledger"</h1>
                        <p class="ledger-sub">
                            "Every BLS credit to your account, newest first. Balances are exact to "
                            "two decimals — the ledger stores whole cents, not floating point."
                        </p>
                    </div>
                    <a href="/bliss" class="ledger-back">"Bliss overview"</a>
                </header>

                {move || {
                    if matches!(app_state.auth.get(), AuthState::Unauthenticated) {
                        return view! {
                            <div class="ledger-card ledger-signin">
                                <p>"Sign in to see your wallet ledger."</p>
                                <a href="/login" class="btn btn-primary">"Sign in"</a>
                            </div>
                        }.into_any();
                    }
                    if let Some(msg) = error.get() {
                        return view! {
                            <div class="ledger-card ledger-error">
                                <p>{msg}</p>
                                <p class="ledger-hint">
                                    "Your balance is safe — this only affects loading the view."
                                </p>
                            </div>
                        }.into_any();
                    }
                    match history.get() {
                        None => view! {
                            <div class="ledger-card">
                                <p class="ledger-hint">
                                    {move || if loading.get() { "Loading your ledger…" } else { "No ledger data yet." }}
                                </p>
                            </div>
                        }.into_any(),
                        Some(h) if h.series.is_empty() => view! {
                            <div class="ledger-card">
                                <p class="ledger-hint">
                                    "No entries yet. BLS is credited at UTC midnight for the work "
                                    "you did the day before."
                                </p>
                            </div>
                        }.into_any(),
                        Some(h) => {
                            let emissions: Vec<LedgerPoint> =
                                h.series.iter().filter(|p| !is_opening(p)).cloned().collect();
                            let credited = emissions.len();
                            let avg = if credited > 0 {
                                emissions.iter().map(|p| p.change).sum::<f64>() / credited as f64
                            } else { 0.0 };
                            let last_date = emissions.last()
                                .map(|p| p.date.clone())
                                .unwrap_or_else(|| "—".to_string());
                            let opening = h.series.iter().find(|p| is_opening(p))
                                .map(|p| p.change).unwrap_or(0.0);
                            let chart = chart_view(&h.series);
                            let mut rows: Vec<LedgerPoint> = h.series.clone();
                            rows.reverse();

                            view! {
                                <div class="ledger-hero">
                                    <div>
                                        <span class="ledger-label">"Balance"</span>
                                        <div class="ledger-balance">
                                            {fmt_bls(h.balance)}<span class="ledger-unit">"BLS"</span>
                                        </div>
                                    </div>
                                    <div class="ledger-stats">
                                        <div class="ledger-stat">
                                            <span class="ledger-label">"Credited days"</span>
                                            <span class="ledger-stat-v">{credited}</span>
                                        </div>
                                        <div class="ledger-stat">
                                            <span class="ledger-label">"Avg / credited day"</span>
                                            <span class="ledger-stat-v">{fmt_bls(avg)}</span>
                                        </div>
                                        <div class="ledger-stat">
                                            <span class="ledger-label">"Last credited"</span>
                                            <span class="ledger-stat-v">{last_date}</span>
                                        </div>
                                    </div>
                                </div>

                                <div class="ledger-card">
                                    <h2 class="ledger-h2">"Cumulative balance"</h2>
                                    {chart}
                                </div>

                                <div class="ledger-card">
                                    <h2 class="ledger-h2">"Entries"</h2>
                                    <div class="ledger-tablewrap">
                                        <table class="ledger-table">
                                            <thead>
                                                <tr>
                                                    <th>"Date"</th>
                                                    <th>"Type"</th>
                                                    <th class="r">"Credit"</th>
                                                    <th class="r">"Balance after"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {rows.into_iter().map(|p| {
                                                    let open = is_opening(&p);
                                                    let date = if open { "Opening".to_string() } else { p.date.clone() };
                                                    let kind = if open { "migrated" } else { "emission" };
                                                    view! {
                                                        <tr>
                                                            <td class="num">{date}</td>
                                                            <td>
                                                                <span class=move || if open {
                                                                    "ledger-chip ledger-chip-open"
                                                                } else { "ledger-chip" }>{kind}</span>
                                                            </td>
                                                            <td class="r num">{format!("+{}", fmt_bls(p.change))}</td>
                                                            <td class="r num">{fmt_bls(p.cumulative)}</td>
                                                        </tr>
                                                    }
                                                }).collect_view()}
                                            </tbody>
                                        </table>
                                    </div>
                                    {(opening > 0.0).then(|| view! {
                                        <p class="ledger-note">
                                            "The opening entry consolidates everything earned before the "
                                            "append-only ledger existed."
                                        </p>
                                    })}
                                </div>
                            }.into_any()
                        }
                    }
                }}
            </div>

            <Footer />
        </div>
    }
}
