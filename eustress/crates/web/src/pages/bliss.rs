// =============================================================================
// Eustress Web - Bliss Page (Cryptocurrency)
// =============================================================================
// Bliss (BLS) - Proof-of-Contribution cryptocurrency for the Eustress ecosystem
// Earn through contributions or purchase on exchanges
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;
use crate::wallet::{use_wallet, WalletStatus};

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Contribution type with weight multiplier
#[derive(Clone, Debug, PartialEq)]
pub struct ContributionType {
    pub name: &'static str,
    pub description: &'static str,
    pub weight: &'static str,
    pub icon: &'static str,
}

/// Exchange/DEX option for purchasing BLS
#[derive(Clone, Debug, PartialEq)]
pub struct ExchangeOption {
    pub name: &'static str,
    pub url: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Bliss cryptocurrency page - earn or purchase BLS tokens.
#[component]
pub fn BlissPage() -> impl IntoView {
    // Get app state and wallet
    let _app_state = expect_context::<AppState>();
    let wallet = use_wallet();
    
    // Wallet connection state
    let show_wallet_modal = RwSignal::new(false);
    let wallet_name = RwSignal::new(String::new());
    let private_key_input = RwSignal::new(String::new());
    let import_mode = RwSignal::new(false);
    let wallet_error = RwSignal::new(Option::<String>::None);
    
    // Contribution types with weights
    let contribution_types = vec![
        ContributionType {
            name: "Building",
            description: "Create 3D models, places, and assets",
            weight: "2.5x",
            icon: "/assets/icons/cube.svg",
        },
        ContributionType {
            name: "Scripting",
            description: "Write Soul scripts and game logic",
            weight: "3.0x",
            icon: "/assets/icons/code.svg",
        },
        ContributionType {
            name: "Design",
            description: "UI/UX design, texturing, visual work",
            weight: "2.0x",
            icon: "/assets/icons/image.svg",
        },
        ContributionType {
            name: "Collaboration",
            description: "Team work, communication, helping others",
            weight: "2.0x",
            icon: "/assets/icons/network.svg",
        },
        ContributionType {
            name: "Teaching",
            description: "Tutorials, mentoring, documentation",
            weight: "2.2x",
            icon: "/assets/icons/book.svg",
        },
    ];
    
    // Exchange options for purchasing BLS
    let exchanges = vec![
        ExchangeOption {
            name: "Uniswap",
            url: "https://app.uniswap.org/#/swap?outputCurrency=BLS_CONTRACT_ADDRESS",
            icon: "/assets/icons/uniswap.svg",
            description: "Decentralized exchange on Ethereum",
        },
        ExchangeOption {
            name: "Coinbase",
            url: "https://www.coinbase.com/price/bliss",
            icon: "/assets/icons/coinbase.svg",
            description: "Centralized exchange with fiat on-ramp",
        },
    ];
    
    // Wallet actions - use Callback for FnMut compatibility
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
            
            // Hero Section
            <section class="bliss-hero">
                <div class="bliss-logo">
                    <img src="/assets/icons/bliss.svg" alt="Bliss" class="bliss-logo-icon" />
                </div>
                <h1 class="bliss-title">"Bliss (BLS)"</h1>
                <p class="bliss-tagline">"Proof-of-Contribution Cryptocurrency"</p>
                <p class="bliss-description">
                    "Earn BLS tokens by contributing to the Eustress ecosystem. Build, script, 
                    design, collaborate, and teach - your work has real value."
                </p>
                
                // Wallet Connection
                {move || {
                    match wallet.status.get() {
                        WalletStatus::Connected(info) => {
                            view! {
                                <div class="wallet-connected">
                                    <div class="wallet-info">
                                        <span class="wallet-label">"Connected Wallet"</span>
                                        <span class="wallet-address">{shorten_address(&info.address)}</span>
                                    </div>
                                    <div class="wallet-balance">
                                        <img src="/assets/icons/bliss.svg" alt="BLS" />
                                        <span class="balance-value">{wallet.formatted_balance()}</span>
                                    </div>
                                    <div class="wallet-pending">
                                        <span class="pending-label">"Pending Rewards"</span>
                                        <span class="pending-value">{wallet.formatted_pending()}</span>
                                    </div>
                                    <div class="wallet-score">
                                        <span class="score-label">"Contribution Score"</span>
                                        <span class="score-value">{format!("{:.2}", wallet.contribution_score.get())}</span>
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
                                    <p class="connect-hint">"Connect your Bliss wallet to view balance and earn rewards"</p>
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
                                    >
                                        "Create New"
                                    </button>
                                    <button 
                                        class="tab-btn"
                                        class:active=move || import_mode.get()
                                        on:click=move |_| import_mode.set(true)
                                    >
                                        "Import Existing"
                                    </button>
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
            
            // Earn BLS Section - Contribution Types
            <section class="earn-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/trending.svg" alt="Earn" class="section-icon" />
                    <h2>"Earn BLS Through Contributions"</h2>
                </div>
                <p class="section-subtitle">"Your contributions are tracked and rewarded monthly"</p>
                
                <div class="contribution-grid">
                    {contribution_types.into_iter().map(|ct| {
                        view! {
                            <div class="contribution-card">
                                <div class="contribution-icon">
                                    <img src={ct.icon} alt={ct.name} />
                                </div>
                                <h3>{ct.name}</h3>
                                <p>{ct.description}</p>
                                <div class="contribution-weight">
                                    <span class="weight-label">"Weight"</span>
                                    <span class="weight-value">{ct.weight}</span>
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </section>
            
            // Buy BLS Section
            <section class="buy-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/gift.svg" alt="Buy" class="section-icon" />
                    <h2>"Purchase BLS"</h2>
                </div>
                <p class="section-subtitle">"Buy BLS tokens on supported exchanges"</p>
                
                <div class="exchange-grid">
                    {exchanges.into_iter().map(|ex| {
                        view! {
                            <a href={ex.url} target="_blank" rel="noopener" class="exchange-card">
                                <img src={ex.icon} alt={ex.name} class="exchange-icon" />
                                <h3>{ex.name}</h3>
                                <p>{ex.description}</p>
                            </a>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </section>
            
            // What is Bliss Section
            <section class="bliss-info-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/help.svg" alt="Info" class="section-icon" />
                    <h2>"What is Bliss?"</h2>
                </div>
                
                <div class="info-grid">
                    <div class="info-card">
                        <div class="info-icon">
                            <img src="/assets/icons/trending.svg" alt="PoC" />
                        </div>
                        <h3>"Proof-of-Contribution"</h3>
                        <p>"Earn tokens by actively contributing to the ecosystem. Building, scripting, and teaching all count."</p>
                    </div>
                    
                    <div class="info-card">
                        <div class="info-icon">
                            <img src="/assets/icons/calendar.svg" alt="Monthly" />
                        </div>
                        <h3>"Monthly Distribution"</h3>
                        <p>"Rewards are calculated and distributed on the 1st of each month based on your contribution score."</p>
                    </div>
                    
                    <div class="info-card">
                        <div class="info-icon">
                            <img src="/assets/icons/shield.svg" alt="Secure" />
                        </div>
                        <h3>"Self-Custody"</h3>
                        <p>"Your keys, your tokens. Bliss uses Ed25519 cryptography for secure wallet management."</p>
                    </div>
                    
                    <div class="info-card">
                        <div class="info-icon">
                            <img src="/assets/icons/network.svg" alt="Network" />
                        </div>
                        <h3>"Node Participation"</h3>
                        <p>"Run a Light Node for free or opt into Full Node for +10% bonus rewards."</p>
                    </div>
                    
                    <div class="info-card">
                        <div class="info-icon">
                            <img src="/assets/icons/heart.svg" alt="Support" />
                        </div>
                        <h3>"Support Creators"</h3>
                        <p>"Tip creators, purchase assets, and fund experiences with BLS tokens."</p>
                    </div>
                    
                    <div class="info-card">
                        <div class="info-icon">
                            <img src="/assets/icons/gift.svg" alt="Transfer" />
                        </div>
                        <h3>"Transferable"</h3>
                        <p>"Send BLS to any address, trade on exchanges, or convert to in-game currency."</p>
                    </div>
                </div>
            </section>
            
            // FAQ Section
            <section class="faq-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/help.svg" alt="FAQ" class="section-icon" />
                    <h2>"Frequently Asked Questions"</h2>
                </div>
                
                <div class="faq-list">
                    <div class="faq-item">
                        <h3>"How do I earn BLS?"</h3>
                        <p>"Connect your wallet and use Eustress Engine. Your building, scripting, design, and collaboration activities are automatically tracked and converted to contribution scores."</p>
                    </div>
                    
                    <div class="faq-item">
                        <h3>"When are rewards distributed?"</h3>
                        <p>"Rewards are calculated and distributed on the 1st of each month. Your pending rewards show your estimated payout for the current cycle."</p>
                    </div>
                    
                    <div class="faq-item">
                        <h3>"What are the contribution weights?"</h3>
                        <p>"Different activities have different weights: Scripting (3.0x), Building (2.5x), Teaching (2.2x), Design (2.0x), Collaboration (2.0x). Time spent is rewarded linearly."</p>
                    </div>
                    
                    <div class="faq-item">
                        <h3>"Can I buy BLS instead of earning it?"</h3>
                        <p>"Yes! BLS is available on Uniswap and Coinbase. This helps liquidity and lets you participate even if you prefer not to contribute directly."</p>
                    </div>
                    
                    <div class="faq-item">
                        <h3>"What is the Full Node bonus?"</h3>
                        <p>"Opt into running a Full Node to earn +10% bonus on all rewards. Full Nodes help secure the network and store blockchain data."</p>
                    </div>
                </div>
            </section>
            
            // Documentation Link
            <section class="docs-cta">
                <div class="docs-card">
                    <img src="/assets/icons/book.svg" alt="Docs" class="docs-icon" />
                    <div class="docs-content">
                        <h3>"Bliss Documentation"</h3>
                        <p>"Learn more about the Bliss economy, contribution scoring, and wallet integration"</p>
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
