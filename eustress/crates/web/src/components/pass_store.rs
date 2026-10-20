// =============================================================================
// Eustress Web - Pass store
// =============================================================================
// The passes a listed simulation sells, on its page. A pass bought here is the
// player's own in every session: a host reads a joining player's passes, and
// scripts see them through `UserOwnsGamePassAsync`. Products granted on each
// purchase are sold inside the simulation only, where its scripts grant them.
//
// Buying is live: real Tickets, at the price the player was shown. A changed
// price refuses the purchase rather than charge a different one.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{buy_product, commerce_error_text, get_catalog, get_owned_passes, get_ticket_balance, CommerceProduct};
use crate::state::{AppState, AuthState};
use crate::utils::format_count;

fn tickets(n: u64) -> String {
    format!("{} {}", format_count(n), if n == 1 { "Ticket" } else { "Tickets" })
}

/// The simulation's passes, with Buy for a signed-in player. Renders nothing
/// when the simulation sells no pass.
#[component]
pub fn PassStore(sim_id: String) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api = StoredValue::new(app_state.api_url.clone());
    let sim = StoredValue::new(sim_id);

    let passes = RwSignal::new(Vec::<CommerceProduct>::new());
    let owned = RwSignal::new(Vec::<u64>::new());
    let balance = RwSignal::new(Option::<u64>::None);
    let confirming = RwSignal::new(Option::<CommerceProduct>::None);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(Option::<(bool, String)>::None);
    let signed_in = Memo::new(move |_| matches!(auth.get(), AuthState::Authenticated(_)));

    // The catalog is public: the store shows signed out too.
    spawn_local(async move {
        if let Ok(list) = get_catalog(&api.get_value(), &sim.get_value()).await {
            passes.set(list.into_iter().filter(|p| p.is_pass() && p.active).collect());
        }
    });

    Effect::new(move |_| {
        if !signed_in.get() {
            owned.set(Vec::new());
            balance.set(None);
            return;
        }
        spawn_local(async move {
            if let Ok(numbers) = get_owned_passes(&api.get_value(), &sim.get_value()).await {
                owned.set(numbers);
            }
            if let Ok(tkt) = get_ticket_balance(&api.get_value()).await {
                balance.set(Some(tkt));
            }
        });
    });

    let buy = move |_| {
        let Some(pass) = confirming.get_untracked() else { return };
        busy.set(true);
        spawn_local(async move {
            match buy_product(&api.get_value(), &sim.get_value(), &pass).await {
                Ok(_) => {
                    owned.update(|o| o.push(pass.number));
                    balance.update(|b| {
                        if let Some(b) = b {
                            *b = b.saturating_sub(pass.price);
                        }
                    });
                    message.set(Some((true, format!("{} is yours. It is active the next time you play.", pass.name))));
                }
                Err(e) => message.set(Some((false, format!("{} was not bought: {}", pass.name, commerce_error_text(&e))))),
            }
            confirming.set(None);
            busy.set(false);
        });
    };

    view! {
        {move || (!passes.with(|p| p.is_empty())).then(|| view! {
            <section class="pass-store" aria-labelledby="pass-store-h">
                <div class="pass-store-head">
                    <h3 id="pass-store-h">"Passes"</h3>
                    {move || balance.get().map(|b| view! { <span class="pass-store-balance">{format!("You have {}", tickets(b))}</span> })}
                </div>
                <p class="pass-store-sub">"Bought once, yours in every session of this simulation."</p>
                {move || message.get().map(|(ok, text)| view! {
                    <p class="pass-store-message" class:is-error=!ok role="status">{text}</p>
                })}
                <ul class="pass-store-list">
                    {move || passes.get().into_iter().map(|p| {
                        let number = p.number;
                        let icon = p.icon.clone().filter(|i| !i.is_empty()).unwrap_or_else(|| "/assets/icons/ticket.svg".to_string());
                        let choose = {
                            let p = p.clone();
                            move |_| {
                                message.set(None);
                                confirming.set(Some(p.clone()));
                            }
                        };
                        view! {
                            <li class="pass-store-item">
                                <img src=icon alt="" class="pass-store-icon" loading="lazy" />
                                <div class="pass-store-text">
                                    <span class="pass-store-name">{p.name.clone()}</span>
                                    {(!p.description.is_empty()).then(|| view! { <span class="pass-store-desc">{p.description.clone()}</span> })}
                                </div>
                                <span class="pass-store-price">{tickets(p.price)}</span>
                                {move || {
                                    if owned.with(|o| o.contains(&number)) {
                                        view! { <span class="pass-store-owned">"Owned"</span> }.into_any()
                                    } else if signed_in.get() {
                                        view! { <button type="button" class="btn btn-primary pass-store-buy" disabled=move || busy.get() on:click=choose.clone()>"Buy"</button> }.into_any()
                                    } else {
                                        view! { <a href="/login" class="pass-store-signin">"Sign in to buy"</a> }.into_any()
                                    }
                                }}
                            </li>
                        }
                    }).collect_view()}
                </ul>
                {move || confirming.get().map(|pass| {
                    let after = balance.get().map(|b| {
                        if b >= pass.price {
                            format!("You have {}; {} after.", tickets(b), tickets(b - pass.price))
                        } else {
                            format!("You have {}: not enough. Get Tickets first.", tickets(b))
                        }
                    });
                    view! {
                        <div class="pass-store-confirm" role="dialog" aria-modal="true" aria-labelledby="pass-store-confirm-h">
                            <h4 id="pass-store-confirm-h">{format!("Buy {} for {}?", pass.name, tickets(pass.price))}</h4>
                            {after.map(|a| view! { <p>{a}</p> })}
                            <p class="pass-store-sub">"A real purchase: Tickets move to the creator now."</p>
                            <div class="pass-store-confirm-actions">
                                <button type="button" class="creator-btn" disabled=move || busy.get() on:click=move |_| confirming.set(None)>"Cancel"</button>
                                <button type="button" class="btn btn-primary" disabled=move || busy.get() on:click=buy>"Buy"</button>
                            </div>
                        </div>
                    }
                })}
            </section>
        })}
    }
}
