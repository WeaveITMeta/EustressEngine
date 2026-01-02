// =============================================================================
// Eustress Web - Premium Page (Industrial Design)
// =============================================================================
// Subscription tiers: Eustress Pro, Eustress Player+, and Bundle
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

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
// Main Component
// -----------------------------------------------------------------------------

/// Premium subscription page.
#[component]
pub fn PremiumPage() -> impl IntoView {
    let billing_yearly = RwSignal::new(true);
    
    // Define premium tiers - matches SUBSCRIPTIONS.md
    let tiers = vec![
        PremiumTier {
            id: "player-plus".to_string(),
            name: "Player Plus".to_string(),
            subtitle: "For Players".to_string(),
            price_monthly: 4.99,
            price_yearly: 49.99,
            features: vec![
                "500 Bliss monthly".to_string(),
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
            id: "creator-pro".to_string(),
            name: "Creator Pro".to_string(),
            subtitle: "For Creators".to_string(),
            price_monthly: 9.99,
            price_yearly: 99.99,
            features: vec![
                "40% revenue share (vs 25%)".to_string(),
                "1 TB asset storage".to_string(),
                "Priority publishing queue".to_string(),
                "Advanced analytics dashboard".to_string(),
                "24hr priority support".to_string(),
                "500 Bliss monthly".to_string(),
                "Early access to features".to_string(),
            ],
            is_popular: true,
            icon: "/assets/icons/eustress-gear.svg".to_string(),
        },
        PremiumTier {
            id: "bundle".to_string(),
            name: "Bundle".to_string(),
            subtitle: "Player Plus + Creator Pro".to_string(),
            price_monthly: 12.99,
            price_yearly: 129.99,
            features: vec![
                "All Player Plus perks".to_string(),
                "All Creator Pro perks".to_string(),
                "1,000 Bliss monthly (combined)".to_string(),
                "10 GB cloud saves".to_string(),
                "Unlimited asset storage".to_string(),
                "40% revenue share".to_string(),
                "Save $1.99/mo vs separate".to_string(),
            ],
            is_popular: false,
            icon: "/assets/icons/sparkles.svg".to_string(),
        },
    ];
    
    view! {
        <div class="page page-premium-industrial">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="premium-bg">
                <div class="premium-grid-overlay"></div>
                <div class="premium-glow glow-1"></div>
                <div class="premium-glow glow-2"></div>
            </div>
            
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
                                
                                <a href=format!("/subscribe?tier={}", tier_id) class="subscribe-btn">
                                    "Subscribe"
                                </a>
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
                                <th class="highlight">"Pro"</th>
                                <th>"Bundle"</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr>
                                <td>"Monthly Bliss"</td>
                                <td>"0"</td>
                                <td>"500"</td>
                                <td>"500"</td>
                                <td>"1,000"</td>
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
