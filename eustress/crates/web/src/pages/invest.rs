// =============================================================================
// Eustress Web - Invest (/invest)
// =============================================================================
// The Regulation A offering, as the Worker publishes it (api/offering.rs). With no live offering this page says only that nothing
// is offered: before a filing, a public word about a future chance to invest can itself break the securities rules
// (docs/finance/REGULATION_A_TIER_2.md, Stage 0). The footer links here only while an offering is live, the page is not in the
// sitemap, and the host tells search engines to skip it (_headers).
//
// While an offering is live the page shows its public facts, the testing-the-waters legend when there is one, and the offering
// circular. Subscribing from the website is not built yet (the Worker's subscription API is), and the page says so.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::offering::{live_offering, PublicOffering};
use crate::api::ApiClient;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

/// What the page knows about the offering.
#[derive(Clone, Debug, PartialEq)]
enum Offer {
    Checking,
    None,
    Live(PublicOffering),
}

/// The words for a live offering's status.
pub fn status_words(status: &str) -> &'static str {
    match status {
        "testing" => "Testing the waters: no money is accepted",
        "open" => "Open",
        "paused" => "Paused",
        _ => "Not offered",
    }
}

/// Cents as dollars: 1000 is "$10.00".
pub fn dollars(cents: u64) -> String {
    format!("${}.{:02}", crate::utils::format_count(cents / 100), cents % 100)
}

/// The statement shown whenever no offering is live.
pub const NOTHING_OFFERED: &str = "Eustress is not offering ownership. Nothing on this site is an offer to sell, or a request for an offer to buy, any security, and no money for one is accepted.";

fn live_view(o: PublicOffering) -> impl IntoView {
    let qualified = o.sec_file_number.clone().zip(o.qualified_on.clone());
    let states = if o.states.is_empty() { "None named".to_string() } else { o.states.join(", ") };
    view! {
        <section class="legal-section">
            <h2>{if o.title.is_empty() { "The offering".to_string() } else { o.title.clone() }}</h2>
            <ul>
                <li>{format!("Status: {}", status_words(&o.status))}</li>
                {(o.price_cents > 0).then(|| view! { <li>{format!("Price per unit: {}", dollars(o.price_cents))}</li> })}
                {(o.min_units > 0).then(|| view! { <li>{format!("Smallest purchase: {} units", crate::utils::format_count(o.min_units))}</li> })}
                <li>{format!("States: {states}")}</li>
                {o.units_available.map(|n| view! { <li>{format!("Units available: {}", crate::utils::format_count(n))}</li> })}
                {qualified.map(|(file, day)| view! { <li>{format!("Qualified by the SEC on {day}, file number {file}")}</li> })}
            </ul>
            {o.legend.clone().map(|legend| view! { <p><strong>{legend}</strong></p> })}
        </section>
        <section class="legal-section">
            <h2>"Read before anything else"</h2>
            {o.circular.clone().map(|doc| view! {
                <p><a href=doc.url target="_blank" rel="noopener noreferrer">"The offering circular"</a></p>
                <p class="legal-updated">{format!("SHA-256 {}", doc.sha256)}</p>
            })}
            {o.circular.is_none().then(|| view! { <p>"The offering circular is not posted yet."</p> })}
            <p>"Subscribing from this website is not open yet."</p>
        </section>
    }
}

#[component]
pub fn InvestPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let offer = RwSignal::new(Offer::Checking);

    // In the browser only: the Worker's answer. Anything but a live offering is none.
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            offer.set(match live_offering(&client).await {
                Some(o) => Offer::Live(o),
                None => Offer::None,
            });
        });
    });

    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />

            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Invest"</h1>
                </div>
                <div class="legal-content">
                    {move || match offer.get() {
                        Offer::Checking => view! { <section class="legal-section"><p>"Checking for an offering…"</p></section> }.into_any(),
                        Offer::None => view! { <section class="legal-section"><p>{NOTHING_OFFERED}</p></section> }.into_any(),
                        Offer::Live(o) => live_view(o).into_any(),
                    }}
                </div>
            </div>

            <Footer />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    #[test]
    fn money_and_status_read_the_way_a_person_says_them() {
        assert_eq!(dollars(1000), "$10.00");
        assert_eq!(dollars(123_456_789), "$1,234,567.89");
        assert_eq!(status_words("testing"), "Testing the waters: no money is accepted");
        assert_eq!(status_words("draft"), "Not offered");
    }

    #[test]
    fn a_testing_offering_shows_its_legend_and_says_subscribing_is_not_open() {
        let owner = Owner::new();
        let o = PublicOffering {
            status: "testing".into(),
            title: "Eustress units".into(),
            price_cents: 1000,
            legend: Some("No money or other consideration is being solicited.".into()),
            ..Default::default()
        };
        let html = owner.with(|| live_view(o).to_html());
        assert!(html.contains("No money or other consideration is being solicited."), "{html}");
        assert!(html.contains("Subscribing from this website is not open yet."), "{html}");
        assert!(html.contains("not posted yet"), "{html}");
    }

    #[test]
    fn the_invest_page_is_listed_nowhere_and_the_footer_links_it_only_behind_a_live_offering() {
        let app = include_str!("../app.rs");
        assert!(app.contains("path!(\"/invest\")") && app.contains("InvestPage"), "the route is wired");
        for (name, text) in [
            ("nav.rs", include_str!("../components/nav.rs")),
            ("sitemap.xml", include_str!("../../sitemap.xml")),
            ("llms.txt", include_str!("../../llms.txt")),
        ] {
            assert!(!text.contains("/invest"), "{name} links the Invest page; it is said nowhere until an offering is live");
        }
        // The one link is the footer's, drawn only when the Worker answered with a live offering.
        let footer = include_str!("../components/footer.rs");
        assert_eq!(footer.matches("href=\"/invest\"").count(), 1);
        assert!(footer.contains("{move || invest.get().then(|| view! { <a href=\"/invest\""), "the footer's Invest link is gated");
        assert!(footer.contains("live_offering(&client).await.is_some()"));
        // The host keeps the page out of search and serves it as an app page.
        let headers = include_str!("../../_headers");
        let at = headers.find("\n/invest\n").expect("a header rule for /invest") + 1;
        assert!(headers[at..].lines().nth(1).unwrap_or("").contains("X-Robots-Tag: noindex, nofollow"));
        assert!(include_str!("../../_redirects").lines().any(|l| l.trim() == "/invest / 200"));
    }

    #[test]
    fn with_nothing_live_the_page_offers_nothing() {
        assert!(NOTHING_OFFERED.contains("not offering ownership") && NOTHING_OFFERED.contains("no money for one is accepted"));
    }
}
