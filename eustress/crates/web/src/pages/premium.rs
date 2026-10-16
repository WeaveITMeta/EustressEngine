// =============================================================================
// Eustress Web - Premium Page (Industrial Design)
// =============================================================================
// Subscription tiers: Eustress Pro, Eustress Player+, and Bundle
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::{ApiClient, ApiError};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Premium subscription tier.
#[derive(Clone, Debug, PartialEq)]
pub struct PremiumTier {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub price_monthly: f64,
    pub price_yearly: f64,
    pub features: Vec<String>,
    pub is_popular: bool,
    pub icon: String,
}

// -----------------------------------------------------------------------------
// The return from Stripe
// -----------------------------------------------------------------------------

/// The query string of the page in front of the visitor. The prerender has no
/// page.
#[cfg(not(feature = "ssr"))]
fn current_query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

#[cfg(feature = "ssr")]
fn current_query() -> String {
    String::new()
}

/// The plan named by `?subscribed=<plan>`, as a name to show. Only the four
/// plans are known; anything else in the address shows nothing, so a link
/// cannot put its own words on the page.
fn subscribed_plan_name(search: &str) -> Option<&'static str> {
    let plan = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("subscribed="))?;
    match plan {
        "player_plus" | "player-plus" => Some("Player Plus"),
        "creator" => Some("Creator"),
        "professional" => Some("Professional"),
        "bundle" => Some("Bundle"),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// The plans, in the order the page shows them: Player Plus, Creator, Bundle (the most popular), then Professional on the far right.
pub fn plan_tiers() -> Vec<PremiumTier> {
    vec![
        PremiumTier {
            id: "player-plus".to_string(),
            name: "Player Plus".to_string(),
            subtitle: "For Players".to_string(),
            price_monthly: 4.99,
            price_yearly: 49.99,
            features: vec![
                "450 Tickets monthly".to_string(),
                "Exclusive profile badge".to_string(),
                "Premium profile flair".to_string(),
                "Subscriber-only cosmetics".to_string(),
                "Priority matchmaking".to_string(),
                "Subscriber-only events".to_string(),
                "10 GB cloud saves".to_string(),
            ],
            is_popular: false,
            icon: "/assets/icons/gamepad.svg".to_string(),
        },
        PremiumTier {
            id: "creator".to_string(),
            name: "Creator".to_string(),
            subtitle: "For Creators".to_string(),
            price_monthly: 9.99,
            price_yearly: 99.99,
            features: vec![
                "40% revenue share".to_string(),
                "1 TB asset storage".to_string(),
                "Priority publishing queue".to_string(),
                "Advanced analytics dashboard".to_string(),
                "24hr priority support".to_string(),
                "1,000 Delta monthly".to_string(),
                "Bliss cash-out (needs identity verification)".to_string(),
                "Early access to features".to_string(),
            ],
            is_popular: false,
            icon: "/assets/icons/eustress-gear.svg".to_string(),
        },
        PremiumTier {
            id: "bundle".to_string(),
            name: "Bundle".to_string(),
            subtitle: "Player Plus + Creator".to_string(),
            price_monthly: 12.99,
            price_yearly: 129.99,
            features: vec![
                "All Player Plus perks".to_string(),
                "All Creator perks".to_string(),
                "400 Tickets + 900 Delta monthly".to_string(),
                "Bliss cash-out (needs identity verification)".to_string(),
                "10 GB cloud saves".to_string(),
                "Unlimited asset storage".to_string(),
                "40% revenue share".to_string(),
                "Save $1.99/mo vs separate".to_string(),
            ],
            is_popular: true,
            icon: "/assets/icons/sparkles.svg".to_string(),
        },
        // Professional: only what is approved is listed (price, Delta, cash-out).
        // Its other perks, and any comparison with another vendor's seat, are not
        // decided; they are added here when they are.
        PremiumTier {
            id: "professional".to_string(),
            name: "Professional".to_string(),
            subtitle: "For professional work".to_string(),
            price_monthly: 129.0,
            price_yearly: 1290.0,
            features: vec![
                "16,000 Delta monthly".to_string(),
                "192,000 Delta with yearly billing".to_string(),
                "Bliss cash-out (needs identity verification)".to_string(),
            ],
            is_popular: false,
            icon: "/assets/icons/eustress-gear.svg".to_string(),
        },
    ]
}

/// Premium subscription page.
#[component]
pub fn PremiumPage() -> impl IntoView {
    let billing_yearly = RwSignal::new(true);
    
    // Define premium tiers - matches SUBSCRIPTIONS.md
    let tiers = plan_tiers();
    
    view! {
        <div class="page page-premium-industrial">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="premium-bg">
                <div class="premium-grid-overlay"></div>
                <div class="premium-glow glow-1"></div>
                <div class="premium-glow glow-2"></div>
            </div>
            
            // After a plan purchase Stripe returns here with ?subscribed=<plan>.
            {subscribed_plan_name(&current_query()).map(|name| view! {
                <section class="subscribed-banner" role="status">
                    <h2>{format!("Thank you. You are subscribed to {name}.")}</h2>
                    <p>"Your plan starts as soon as Stripe confirms the payment."</p>
                    <a href="/dashboard" class="btn btn-primary">"Go to my dashboard"</a>
                </section>
            })}

            // Hero Section
            <section class="premium-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"PREMIUM"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="premium-title">"Unlock Your Full Potential"</h1>
                <p class="premium-tagline">"Choose the plan that fits your creative journey"</p>
                
                // Billing Toggle
                <div class="billing-toggle">
                    <button 
                        class="toggle-btn"
                        class:active=move || !billing_yearly.get()
                        on:click=move |_| billing_yearly.set(false)
                    >
                        "Monthly"
                    </button>
                    <button 
                        class="toggle-btn"
                        class:active=move || billing_yearly.get()
                        on:click=move |_| billing_yearly.set(true)
                    >
                        "Yearly"
                        <span class="save-badge">"Save 17%"</span>
                    </button>
                </div>
            </section>
            
            // Pricing Cards
            <section class="pricing-section">
                <div class="pricing-grid">
                    {tiers.into_iter().map(|tier| {
                        let tier_id = tier.id.clone();
                        let is_yearly = billing_yearly;
                        
                        view! {
                            <div class="pricing-card" class:popular=tier.is_popular>
                                {tier.is_popular.then(|| view! {
                                    <div class="popular-badge">"Most Popular"</div>
                                })}
                                
                                <div class="card-icon">
                                    <img src=tier.icon.clone() alt=tier.name.clone() />
                                </div>
                                
                                <h3 class="card-name">{tier.name.clone()}</h3>
                                <p class="card-subtitle">{tier.subtitle.clone()}</p>
                                
                                <div class="card-price">
                                    <span class="currency">"$"</span>
                                    <span class="amount">
                                        {move || if is_yearly.get() {
                                            format!("{:.2}", tier.price_yearly / 12.0)
                                        } else {
                                            format!("{:.2}", tier.price_monthly)
                                        }}
                                    </span>
                                    <span class="period">"/mo"</span>
                                </div>
                                
                                <p class="billed-text">
                                    {move || if is_yearly.get() {
                                        format!("Billed ${:.2}/year", tier.price_yearly)
                                    } else {
                                        "Billed monthly".to_string()
                                    }}
                                </p>
                                
                                <ul class="feature-list">
                                    {tier.features.iter().map(|feature| {
                                        view! {
                                            <li>
                                                <span class="check-icon">"✓"</span>
                                                {feature.clone()}
                                            </li>
                                        }
                                    }).collect::<Vec<_>>()}
                                </ul>
                                
                                <SubscribeButton plan=tier_id.replace('-', "_") yearly=billing_yearly />
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </section>
            
            // Comparison Table
            <section class="comparison-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/trending.svg" alt="Compare" class="section-icon" />
                    <h2>"Compare Plans"</h2>
                </div>
                
                <div class="comparison-table-wrapper">
                    <table class="premium-comparison-table">
                        <thead>
                            <tr>
                                <th class="feature-col">"Feature"</th>
                                <th>"Free"</th>
                                <th>"Player+"</th>
                                <th>"Creator"</th>
                                <th class="highlight">"Bundle"</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr>
                                <td>"Monthly Tickets"</td>
                                <td>"0"</td>
                                <td>"450"</td>
                                <td>"0"</td>
                                <td>"400"</td>
                            </tr>
                            <tr>
                                <td>"Monthly Delta"</td>
                                <td>"1,000 (trial)"</td>
                                <td>"0"</td>
                                <td>"1,000"</td>
                                <td>"900"</td>
                            </tr>
                            <tr>
                                <td>"Bliss cash-out"</td>
                                <td><span class="cross">"No"</span></td>
                                <td><span class="cross">"No"</span></td>
                                <td><span class="check">"✓"</span></td>
                                <td><span class="check">"✓"</span></td>
                            </tr>
                            <tr>
                                <td>"Profile Badge"</td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                            </tr>
                            <tr>
                                <td>"Subscriber Cosmetics"</td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                            </tr>
                            <tr>
                                <td>"Priority Queue"</td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                            </tr>
                            <tr>
                                <td>"Cloud Saves"</td>
                                <td>"1 GB"</td>
                                <td>"10 GB"</td>
                                <td>"1 GB"</td>
                                <td>"10 GB"</td>
                            </tr>
                            <tr>
                                <td>"Asset Storage"</td>
                                <td>"10 GB"</td>
                                <td>"10 GB"</td>
                                <td>"1 TB"</td>
                                <td>"Unlimited"</td>
                            </tr>
                            <tr>
                                <td>"Revenue Share"</td>
                                <td>"25%"</td>
                                <td>"25%"</td>
                                <td>"40%"</td>
                                <td>"40%"</td>
                            </tr>
                            <tr>
                                <td>"Priority Support"</td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="cross">"—"</span></td>
                                <td>"24hr"</td>
                                <td>"24hr"</td>
                            </tr>
                            <tr>
                                <td>"Advanced Analytics"</td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="cross">"—"</span></td>
                                <td><span class="check">"✓"</span></td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </section>
            
            // FAQ Section
            <section class="faq-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/book.svg" alt="FAQ" class="section-icon" />
                    <h2>"Frequently Asked Questions"</h2>
                </div>
                
                <div class="faq-grid">
                    <div class="faq-item">
                        <h4>"Can I cancel anytime?"</h4>
                        <p>"Yes! Cancel your subscription at any time. You'll keep your benefits until the end of your billing period."</p>
                    </div>
                    <div class="faq-item">
                        <h4>"Do I keep my Bliss if I cancel?"</h4>
                        <p>"Absolutely. Any Bliss you've earned or received stays in your account forever, even after canceling."</p>
                    </div>
                    <div class="faq-item">
                        <h4>"What is Delta?"</h4>
                        <p>"Delta is the usage credit for premium Studio tools. Verified accounts get a one-time trial of 1,000, Creator, Professional and Bundle add more every month, and you can buy more. Delta never converts to Bliss or Tickets, and it can't be cashed out."</p>
                    </div>
                    <div class="faq-item">
                        <h4>"Who can cash out Bliss?"</h4>
                        <p>"Bliss cash-out needs identity verification (KYC) and a Creator, Professional or Bundle plan."</p>
                    </div>
                    <div class="faq-item">
                        <h4>"Can I upgrade or downgrade?"</h4>
                        <p>"Yes, you can change your plan at any time. Upgrades take effect immediately; downgrades apply at your next billing cycle."</p>
                    </div>
                    <div class="faq-item">
                        <h4>"Is there a free trial?"</h4>
                        <p>"Eustress is free to use forever. Premium plans add extra features and benefits, but the core experience is always free."</p>
                    </div>
                </div>
            </section>
            
            // CTA Section
            <section class="premium-cta-section">
                <div class="cta-card">
                    <h2>"Still Have Questions?"</h2>
                    <p>"Our team is here to help you find the perfect plan"</p>
                    <div class="cta-buttons">
                        <a href="https://discord.gg/DGP9my8DYN" class="btn-cta primary">
                            "Chat on Discord"
                        </a>
                        <a href="/about" class="btn-cta secondary">
                            "Learn More"
                        </a>
                    </div>
                </div>
            </section>
            
            <Footer />
        </div>
    }
}

/// Starts checkout for `plan` (`player_plus`, `creator`, `professional` or `bundle`) at the
/// interval the page is showing. The Worker answers with a Stripe page to open.
#[component]
fn SubscribeButton(plan: String, yearly: RwSignal<bool>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let busy = RwSignal::new(false);
    let note = RwSignal::new(String::new());
    let needs_sign_in = RwSignal::new(false);

    let start = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        note.set(String::new());
        needs_sign_in.set(false);
        let api_url = app_state.api_url.clone();
        let plan = plan.clone();
        let interval = if yearly.get_untracked() { "yearly" } else { "monthly" };
        spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let body = serde_json::json!({ "plan": plan, "interval": interval });
            match client.post::<serde_json::Value, _>("/api/subscriptions/checkout", &body).await {
                Ok(answer) => match answer.get("url").and_then(|u| u.as_str()) {
                    // Only a Stripe page is opened, whatever the answer says.
                    Some(url) if url.starts_with("https://checkout.stripe.com/") => {
                        if let Some(win) = web_sys::window() {
                            let _ = win.location().set_href(url);
                        }
                    }
                    _ => note.set("Checkout did not return a page to open. Try again.".to_string()),
                },
                Err(ApiError::Unauthorized) => needs_sign_in.set(true),
                Err(ApiError::NotFound) => note.set("Subscriptions are not open yet.".to_string()),
                Err(ApiError::Server { status, message }) => {
                    let text = serde_json::from_str::<serde_json::Value>(&message)
                        .ok()
                        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                        .unwrap_or_else(|| format!("Checkout failed ({status}). Try again."));
                    note.set(text);
                }
                Err(_) => note.set("Could not reach checkout. Check your connection.".to_string()),
            }
            busy.set(false);
        });
    };

    view! {
        <button class="subscribe-btn" disabled=move || busy.get() on:click=start>
            {move || if busy.get() { "Opening checkout..." } else { "Subscribe" }}
        </button>
        <Show when=move || needs_sign_in.get()>
            <p class="subscribe-note"><a href="/login">"Sign in to subscribe"</a></p>
        </Show>
        <Show when=move || !note.get().is_empty()>
            <p class="subscribe-note">{move || note.get()}</p>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plans_run_player_plus_creator_bundle_professional_and_only_the_bundle_is_most_popular() {
        let tiers = plan_tiers();
        let ids: Vec<&str> = tiers.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["player-plus", "creator", "bundle", "professional"]);
        let popular: Vec<&str> = tiers.iter().filter(|t| t.is_popular).map(|t| t.id.as_str()).collect();
        assert_eq!(popular, ["bundle"]);
    }
}
