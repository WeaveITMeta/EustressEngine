// =============================================================================
// Eustress Web - Legal index (/acts)
// =============================================================================
//
// The laws Eustress answers to, one card each, linking to its page under
// /legal/:act (pages/legal_acts.rs holds every word of those pages, and the
// cards' one-line summaries).

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::pages::legal_acts::{route_of, ACTS, DRAFT};

#[component]
pub fn ActsPage() -> impl IntoView {
    view! {
        <div class="page g-page lg-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <ActsBody />
            </main>
            <Footer />
        </div>
    }
}

/// The index without the site's frame (so tests can render it alone).
#[component]
pub fn ActsBody() -> impl IntoView {
    view! {
        <div class="g-wrap lg-wrap">
            <header class="lg-index-head g-rise">
                <div class="g-kicker"><span class="g-kick-rule"></span>"Legal"</div>
                <h1 class="lg-title">"The laws Eustress answers to"</h1>
                <p class="lg-lede">"Each page says what the law is in plain words, what it means on Eustress, your rights and how to use them, and how fast we answer. Where something is not built yet, the page says so."</p>
                {DRAFT.then(|| view! { <div class="g-chips"><span class="g-chip warn">"Draft for legal review"</span></div> })}
            </header>

            <div class="lg-cards">
                {ACTS.iter().map(|a| view! {
                    <a href=route_of(a) class="g-glass lg-card">
                        <span class="lg-card-code">{a.short}</span>
                        <span class="lg-card-name">{a.name}</span>
                        <p>{a.card}</p>
                        <span class="lg-card-go">"Read the page"</span>
                    </a>
                }).collect_view()}
            </div>

            <section class="g-section">
                <div class="g-head"><h2>"Contacts"</h2><span class="g-rule"></span></div>
                <dl class="g-kv g-glass lg-contacts">
                    <dt>"Privacy"</dt><dd><a class="lg-link" href="mailto:privacy@eustress.dev">"privacy@eustress.dev"</a></dd>
                    <dt>"Copyright"</dt><dd><a class="lg-link" href="mailto:dmca@eustress.dev">"dmca@eustress.dev"</a></dd>
                    <dt>"Child safety"</dt><dd><a class="lg-link" href="mailto:safety@eustress.dev">"safety@eustress.dev"</a></dd>
                    <dt>"Intimate images"</dt><dd><a class="lg-link" href="/take-it-down">"The TAKE IT DOWN request form"</a></dd>
                    <dt>"Other legal"</dt><dd><a class="lg-link" href="mailto:legal@eustress.dev">"legal@eustress.dev"</a></dd>
                </dl>
            </section>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_links_to_its_act_page() {
        let owner = Owner::new();
        let html = owner.with(|| view! { <ActsBody /> }.to_html());
        for a in ACTS {
            assert!(html.contains(&format!("href=\"/legal/{}\"", a.slug)), "{}", a.slug);
        }
        // The old card addresses had no pages behind them.
        assert!(!html.contains("href=\"/acts/"));
        // The audit's overclaims stay off the cards.
        assert!(!html.to_lowercase().contains("immediate reporting to ncmec"));
        assert!(!html.to_lowercase().contains("parental consent requirements"));
    }
}
