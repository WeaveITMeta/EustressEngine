// =============================================================================
// Eustress Web - Tickets Page
// =============================================================================
// Purchase Tickets (TKT) for marketplace, game passes, and simulation features.
// 50% of revenue funds the Bliss contributor treasury.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

const API_URL: &str = "https://api.eustress.dev";

#[derive(Debug, Clone, Deserialize)]
struct TicketPackage {
    id: String,
    name: String,
    usd: f64,
    base: u64,
    bonus: u64,
    total: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct PackagesResponse {
    packages: Vec<TicketPackage>,
}

#[component]
pub fn TicketsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();

    let packages = RwSignal::new(Vec::<TicketPackage>::new());
    let selected = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(false);

    // Fetch packages on load
    spawn_local(async move {
        if let Ok(resp) = gloo_net::http::Request::get(&format!("{}/api/tickets/packages", API_URL))
            .send().await
        {
            if let Ok(data) = resp.json::<PackagesResponse>().await {
                packages.set(data.packages);
            }
        }
    });

    view! {
        <div class="page page-tickets">
            <CentralNav active="".to_string() />

            <div class="tickets-bg">
                <div class="tickets-grid-overlay"></div>
            </div>

            <section class="tickets-hero">
                <h1 class="tickets-title">"Tickets"</h1>
                <p class="tickets-subtitle">
                    "Purchase Tickets for the marketplace, game passes, and simulation features. "
                    "50% of every purchase funds the Bliss contributor treasury."
                </p>

                // Balance (if signed in)
                {move || {
                    match app_state.auth.get() {
                        AuthState::Authenticated(user) => {
                            view! {
                                <div class="tickets-balance-display">
                                    <span class="tickets-balance-icon">"🎟"</span>
                                    <span class="tickets-balance-amount">{user.ticket_balance.to_string()}</span>
                                    <span class="tickets-balance-label">"Tickets"</span>
                                </div>
                            }.into_any()
                        }
                        _ => view! {
                            <a href="/login" class="btn btn-primary">"Sign In to Buy Tickets"</a>
                        }.into_any()
                    }
                }}
            </section>

            // Packages grid
            <section class="tickets-packages">
                <div class="packages-grid">
                    {move || packages.get().into_iter().map(|pkg| {
                        let pkg_id = pkg.id.clone();
                        let is_selected = move || selected.get() == Some(pkg_id.clone());
                        let is_best = pkg.id == "ultra";
                        let is_popular = pkg.id == "mega";

                        view! {
                            <div
                                class="ticket-card"
                                class:selected=is_selected
                                class:best-value=is_best
                                class:popular=is_popular
                                on:click={
                                    let id = pkg.id.clone();
                                    move |_| selected.set(Some(id.clone()))
                                }
                            >
                                {if is_popular {
                                    Some(view! { <span class="ticket-badge popular-badge">"MOST POPULAR"</span> })
                                } else if is_best {
                                    Some(view! { <span class="ticket-badge best-badge">"BEST VALUE"</span> })
                                } else {
                                    None
                                }}

                                <h3 class="ticket-card-name">{pkg.name.clone()}</h3>
                                <div class="ticket-card-amount">
                                    <span class="ticket-card-icon">"🎟"</span>
                                    <span class="ticket-card-count">{format_tickets(pkg.base)}</span>
                                </div>
                                {if pkg.bonus > 0 {
                                    Some(view! {
                                        <span class="ticket-card-bonus">{format!("+{} Bonus", format_tickets(pkg.bonus))}</span>
                                        <span class="ticket-card-total">{format!("Total: {} Tickets", format_tickets(pkg.total))}</span>
                                    })
                                } else {
                                    Some(view! {
                                        <span class="ticket-card-total">{format!("Total: {} Tickets", format_tickets(pkg.total))}</span>
                                    })
                                }}
                                <div class="ticket-card-price">{format!("${:.2}", pkg.usd)}</div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>

                // Buy button
                <button
                    class="btn btn-primary tickets-buy-btn"
                    disabled=move || selected.get().is_none() || loading.get()
                    on:click=move |_| {
                        let Some(pkg_id) = selected.get() else { return };
                        let token: Option<String> = {
                            use gloo_storage::Storage;
                            gloo_storage::LocalStorage::get("auth_token").ok()
                        };
                        let Some(token) = token else {
                            if let Some(w) = web_sys::window() {
                                let _ = w.location().set_href("/login");
                            }
                            return;
                        };

                        loading.set(true);
                        spawn_local(async move {
                            let body = serde_json::json!({ "package": pkg_id });
                            let resp = gloo_net::http::Request::post(&format!("{}/api/tickets/checkout", API_URL))
                                .header("Authorization", &format!("Bearer {}", token))
                                .header("Content-Type", "application/json")
                                .body(body.to_string())
                                .unwrap()
                                .send()
                                .await;

                            loading.set(false);
                            if let Ok(resp) = resp {
                                if let Ok(data) = resp.json::<serde_json::Value>().await {
                                    if let Some(url) = data.get("url").and_then(|v| v.as_str()) {
                                        if let Some(w) = web_sys::window() {
                                            let _ = w.location().set_href(url);
                                        }
                                    } else if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
                                        if let Some(w) = web_sys::window() {
                                            let _ = w.alert_with_message(&format!("Error: {}", err));
                                        }
                                    }
                                }
                            }
                        });
                    }
                >
                    {move || if loading.get() { "Processing..." } else { "Buy Tickets" }}
                </button>

                <p class="tickets-note">
                    "Secure payment via Stripe. 50% funds the Bliss contributor treasury."
                </p>
            </section>

            <Footer />
        </div>
    }
}

fn format_tickets(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
