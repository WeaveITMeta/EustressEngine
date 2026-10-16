// =============================================================================
// Eustress Web - Currency (/currency)
// =============================================================================
// Eustress has three currencies, each with one job. This page tells them apart and sends the reader to the page that explains
// the one they want: Tickets (/tickets), Bliss (/bliss), Delta (/delta). Every line is a fact the pages behind it also state.
// The three pages share one look and one strip of tabs (components/currency_ui.rs).
// =============================================================================

use leptos::prelude::*;

use crate::components::currency_ui::{Currency, CurrencyShell};

/// One currency's card: its name, the one job it does, three points, and the page with the details.
struct Card {
    currency: Currency,
    job: &'static str,
    points: [&'static str; 3],
}

fn cards() -> [Card; 3] {
    [
        Card {
            currency: Currency::Tickets,
            job: "Spend them in the marketplace and on game passes.",
            points: [
                "Bought with money in packs",
                "Spent on marketplace items, game passes and simulation features",
                "Half of each sale, after the payment fee, funds the Bliss contributor treasury",
            ],
        },
        Card {
            currency: Currency::Bliss,
            job: "Earned by contributing to the platform.",
            points: [
                "Credited to contributors from the treasury",
                "Cash-out needs identity verification and a Creator, Professional or Bundle plan",
                "Your balance shows in the top bar",
            ],
        },
        Card {
            currency: Currency::Delta,
            job: "Pays for premium Studio tools. Most tools are free.",
            points: [
                "A trial on identity verification, monthly grants with some plans, and packs",
                "Credits expire after 12 months",
                "Never converts to Bliss or Tickets and is never cashed out",
            ],
        },
    ]
}

#[component]
pub fn CurrencyPage() -> impl IntoView {
    view! {
        <CurrencyShell title="Three currencies, three jobs" lead="Tickets buy things, Bliss rewards contributors, Delta pays for premium Studio tools. They do not convert into each other.">
            <section class="cur-grid" aria-label="The three currencies">
                {cards().into_iter().map(|c| view! {
                    <article class="g-glass dl-card">
                        <h2 class="cur-name">{c.currency.name()}</h2>
                        <p class="dl-purpose">{c.job}</p>
                        <ul class="dl-points">{c.points.iter().map(|p| view! { <li>{*p}</li> }).collect_view()}</ul>
                        <div class="dl-card-action"><a class="g-btn g-btn-primary g-btn-lg" href=c.currency.path()>{format!("Go to {}", c.currency.name())}</a></div>
                    </article>
                }).collect_view()}
            </section>
            <p class="g-hint dl-note">"Plans and their monthly grants are on the "<a href="/premium">"Premium page"</a>"."</p>
        </CurrencyShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_cards_go_to_the_three_detail_pages_in_order() {
        let c = cards();
        assert_eq!([c[0].currency.path(), c[1].currency.path(), c[2].currency.path()], ["/tickets", "/bliss", "/delta"]);
        assert_eq!([c[0].currency.name(), c[1].currency.name(), c[2].currency.name()], ["Tickets", "Bliss", "Delta"]);
    }

    #[test]
    fn delta_is_never_described_as_convertible_or_cashable() {
        let delta = &cards()[2];
        let text = delta.points.join(" ").to_lowercase();
        assert!(text.contains("never converts") && text.contains("never cashed out"));
    }

    #[test]
    fn tickets_are_not_said_to_pay_the_treasury_before_the_payment_fee() {
        // The Worker takes the processor's fee first and splits the rest (treasury.mjs splitSale), so "half of what Tickets bring in"
        // would be more than the treasury gets.
        let tickets = &cards()[0];
        assert!(tickets.points.iter().any(|p| p.contains("after the payment fee")));
    }
}
