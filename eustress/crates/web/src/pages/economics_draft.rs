// =============================================================================
// Eustress Web - Economics DRAFT (/economics-draft, feature `economics-draft`)
// =============================================================================
// The internal draft of the Economics page: the money-flow chart of a month of Eustress from the buyer's card to the profit kept, and
// what one creator takes home, with sliders for the proposals that are not decided (the Ticket Sales Reserve and the Tickets cash-out,
// Treasury investments and their repayment) and for the operating numbers (Engine Expenses, tax). None of that is for publication, so
// this page and its modules are compiled only with the `economics-draft` feature; without it the route answers as any address the site does
// not have (the stub in economics.rs) and the public bundle holds none of these words, its stylesheet included: the chart colors only this page
// uses are in style/economics_draft.css, which this page puts in a <style> of its own. The public page is economics.rs.
//
// It is unlisted (not in the nav, the footer, the sitemap or llms.txt), and the host's headers tell search engines to skip it
// (`_headers`). The arithmetic is economics_draft_model.rs and the drawing is economics_draft_chart.rs.
//
// Table of Contents:
// 1. The copy
// 2. The page
// 3. Controls
// 4. Tests
// =============================================================================

use leptos::prelude::*;

use super::economics_draft_chart::{
    company_rows, company_sentences, company_svg, creator_funnel, creator_notes, creator_rows, creator_svg, preset_months, treasury_rows, Row,
};
use super::economics_draft_model::{
    cheapest_ticket_usd, company, creator, dollars, cents_words, layout, Company, Control, Creator, Group, Inputs, Item, Pick, PLANS, PRESETS,
};
use crate::components::currency_ui::Figures;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// 1. The copy
// -----------------------------------------------------------------------------

/// How a step stands today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The rule exists in the Worker.
    Built,
    /// A draft that is not live.
    Proposed,
    /// The model's own bookkeeping, with placeholder numbers.
    Model,
}

impl Status {
    pub fn words(self) -> &'static str {
        match self {
            Status::Built => "Built",
            Status::Proposed => "Proposed",
            Status::Model => "Placeholder numbers",
        }
    }

    pub fn chip(self) -> &'static str {
        match self {
            Status::Built => "g-chip live",
            Status::Proposed => "g-chip warn",
            Status::Model => "g-chip",
        }
    }
}

pub struct Step {
    pub title: &'static str,
    pub body: &'static str,
    pub status: Status,
    /// A page with the details: its address and the words for the link.
    pub more: Option<(&'static str, &'static str)>,
}

pub static STEPS: [Step; 6] = [
    Step {
        title: "A buyer pays",
        body: "Tickets, a plan or a Delta pack. The payment fee comes off first: Stripe's 2.9% plus 30 cents on the web, 30% through an app store.",
        status: Status::Built,
        more: Some(("/tickets", "Ticket packs")),
    },
    Step {
        title: "The net is split in half",
        body: "Half goes to the Bliss Treasury and half is Eustress's. Tickets, Delta packs and plan payments all follow the same split.",
        status: Status::Built,
        more: None,
    },
    Step {
        title: "The Treasury pays contributors every day",
        body: "A fixed 0.276% of the Treasury, shared by score. Score comes from hours in Studio and from sales. The more contributors share a day, the smaller each share.",
        status: Status::Built,
        more: Some(("/docs/earning", "Scores and the daily drip")),
    },
    Step {
        title: "A creator is paid when Tickets are spent on their work",
        body: "The creator receives 70% as earned Tickets and the other 30% leaves circulation. No storefront sells for Tickets yet.",
        status: Status::Built,
        more: Some(("/docs/earning#tickets-sales", "Selling for Tickets")),
    },
    Step {
        title: "A reserve pays Tickets cash-outs",
        body: "15% of Eustress's half of each Ticket sale, and of the Tickets share of plan invoices, goes to a separate reserve. Enrolled creators could cash out Tickets they earn from sales, at a published rate. Tickets a buyer buys never cash out.",
        status: Status::Proposed,
        more: None,
    },
    Step {
        title: "Eustress pays its costs, then tax",
        body: "Engine Expenses come out of Eustress's half after the reserve, and tax is charged on what remains. The model can also show money put into the Treasury as capital that is repaid from it. Nothing like that is offered.",
        status: Status::Model,
        more: None,
    },
];

pub const LEAD: &str = "Follow a month of receipts from a buyer's card to a creator's payout, and move the sliders to test what changes.";

pub const CURRENCIES: &str = "Eustress has three currencies: Tickets buy things, Bliss rewards contributors and Delta pays for premium Studio tools. This page follows the money behind Tickets and Bliss. Delta never converts into either and is never cashed out.";

/// The colors only the draft's charts use, and the one layout rule only its page has.
pub const DRAFT_CSS: &str = include_str!("../../style/economics_draft.css");

pub const DRAFT: &str = "This is an internal draft model, not for publication: it includes proposals and operating assumptions that are not decided. Every figure is an example, not a price, a rate or a promise.";

pub const STEPS_INTRO: &str = "Six steps. Four describe rules that are built into the platform today; one is a proposal and one is the model's own bookkeeping.";

pub const MONTH_INTRO: &str = "Set what Eustress sells in a month. The chart follows the money from receipts to profit kept: the fee, the two halves, the reserve, the costs and the tax. The Bliss Treasury, the Ticket Sales Reserve, Engine Expenses and Taxes each sit in their own account.";

pub const CREATOR_INTRO: &str = "Choose how popular a creator is and how much time they spend in Studio. The chart shows what reaches them: cash from the Tickets line, cash from the Bliss line, and Tickets they can spend inside Eustress. The Bliss line comes out of the Treasury payout above, so it moves when the month above moves.";

pub const TICKETS_NOTE: &str = "Tickets bought with money have no cash value. Dollar figures for Tickets here use the pack price only as a yardstick. In the proposal, an enrolled creator could cash out Tickets they earn from sales, never Tickets they buy.";

pub const MONTH_NOTES: [&str; 3] = [
    "Fees come off first, then the net is split. Today a refund is taken back from both halves, and a gift to the Treasury is credited whole.",
    "Investments are capital, not revenue, and repayments are not an expense. A repayment comes out of the Treasury, so contributors are paid less by the same amount. Nothing here is offered.",
    "Placeholders: every volume, Engine Expenses, the tax rate, the repayment and return rates, the cash-out rate, the share of bought Tickets cashed out, and when the Treasury counts as an expense. An accountant decides the last one.",
];

pub const ASSUMPTIONS: [&str; 7] = [
    "A month is 30 days, and the Treasury is at a steady state: it pays out each month what comes in.",
    "Studio hours are 40% scripting, 40% building and 20% other time. The weights are 3.0, 2.5 and 1.0 on a Light node, and a Full node earns 10% more.",
    "Sales add 0.5 score for each Ticket a creator earns from Tickets bought with money. Updates have no score of their own.",
    "The other contributors are a number and an average daily score. The Bliss line follows them more than anything one creator does.",
    "A Delta pack order averages $25, which sets the fixed part of Stripe's fee on Delta packs.",
    "A plan's Tickets are valued at the cheapest pack's price per Ticket to find the Tickets share of its invoice.",
    "Refunds, chargebacks and app-store sales beyond the fee are not modeled.",
];

/// Pages that state the rules this page uses: address, name, and what is there.
pub static SOURCES: [(&str, &str, &str); 6] = [
    ("/currency", "Currency", "The three currencies and what each is for"),
    ("/docs/earning", "Earning", "Scores, the daily drip and the Ticket split"),
    ("/tickets", "Tickets", "Packs and what Tickets are for"),
    ("/bliss", "Bliss", "The Treasury and contributor payouts"),
    ("/delta", "Delta", "Credit for premium Studio tools"),
    ("/premium", "Premium", "Plans and what they grant"),
];

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

/// The page: the site's nav and footer around the body.
#[component]
pub fn EconomicsDraftPage() -> impl IntoView {
    view! {
        <div class="page g-page dl-page eco-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main dl-main eco-main">
                <div class="g-wrap eco-wrap">
                    <EconomicsDraftBody />
                </div>
            </main>
            <Footer />
        </div>
    }
}

/// Everything the page says and lets the reader move, without the site's frame (so tests can render it alone).
#[component]
pub fn EconomicsDraftBody() -> impl IntoView {
    let inputs = RwSignal::new(Inputs::default());

    // A draft page stays out of search results: tell crawlers that run scripts, as the response header tells those that do not.
    #[cfg(not(feature = "ssr"))]
    {
        let held = StoredValue::new_local(None::<web_sys::Element>);
        Effect::new(move |_| {
            let doc = document();
            if let Ok(meta) = doc.create_element("meta") {
                let _ = meta.set_attribute("name", "robots");
                let _ = meta.set_attribute("content", "noindex, nofollow");
                if let Ok(Some(head)) = doc.query_selector("head") {
                    let _ = head.append_child(&meta);
                    held.set_value(Some(meta));
                }
            }
        });
        on_cleanup(move || {
            held.update_value(|slot| {
                if let Some(meta) = slot.take() {
                    meta.remove();
                }
            });
        });
    }

    let calc = move || -> (Inputs, Company, Creator) {
        let i = inputs.get();
        let m = company(&i);
        let c = creator(&i, &m);
        (i, m, c)
    };

    let share = |i: usize| (100.0 * PLANS[i].tickets_share()).round() as i64;
    let plan_note = format!(
        "The reserve is funded from Ticket packs and from the Tickets share of plan invoices. A plan's Tickets are valued at the cheapest pack's price, ${:.5} each: Player Plus is {}% of its invoice and Bundle {}%.",
        cheapest_ticket_usd(),
        share(0),
        share(2)
    );

    view! {
        <style inner_html=DRAFT_CSS></style>
        <header class="dl-head eco-head">
            <div class="g-kicker"><span class="g-kick-rule"></span>"Economics"</div>
            <h1 class="dl-title">"Economics model: internal draft"</h1>
            <p class="dl-lead">{LEAD}</p>
        </header>
        <p class="cur-banner eco-draft" role="note"><strong>"Draft model. "</strong>{DRAFT}</p>
        <nav class="cur-tabs eco-jump" aria-label="On this page">
            <a href="#eco-how" class="cur-tab">"The short version"</a>
            <a href="#eco-month" class="cur-tab">"A month of Eustress"</a>
            <a href="#eco-creator" class="cur-tab">"One creator"</a>
            <a href="#eco-assume" class="cur-tab">"Assumptions"</a>
        </nav>

        <section class="g-section" id="eco-how" aria-labelledby="eco-how-h">
            <div class="g-head"><h2 id="eco-how-h">"The short version"</h2><span class="g-rule"></span></div>
            <p class="eco-intro">{CURRENCIES}</p>
            <p class="eco-intro">{STEPS_INTRO}</p>
            <ol class="eco-steps">
                {STEPS.iter().map(|s| view! {
                    <li class="g-glass eco-step">
                        <h3>{s.title}</h3>
                        <p>{s.body}</p>
                        {s.more.map(|(href, words)| view! { <p class="eco-more"><a href=href>{words}</a></p> })}
                        <span class=s.status.chip()>{s.status.words()}</span>
                    </li>
                }).collect_view()}
            </ol>
        </section>

        <section class="g-section" id="eco-month" aria-labelledby="eco-month-h">
            <div class="g-head"><h2 id="eco-month-h">"A month of Eustress"</h2><span class="g-rule"></span></div>
            <p class="eco-intro">{MONTH_INTRO}</p>
            <figure class="g-glass eco-fig">
                <div class="eco-chart" inner_html=move || { let (i, m, _) = calc(); company_svg(&i, &m) }></div>
                <p class="eco-scroll-hint">"Scroll sideways to see the whole chart. The table below has the same figures."</p>
            </figure>
            <div class="eco-readout" role="group" aria-label="The month at a glance">
                {move || {
                    let (_, m, _) = calc();
                    let d = &m.shown;
                    view! { <Figures items=vec![
                        ("Gross receipts", dollars(d.gross)),
                        ("Paid to contributors", dollars(d.drip)),
                        ("Reserve", dollars(d.reserve)),
                        ("Operating profit", dollars(d.profit)),
                        ("Taxes", dollars(d.taxes)),
                        ("Profit kept", dollars(d.kept)),
                    ] /> }
                }}
            </div>
            {move || { let (i, m, _) = calc(); view! { <Statement rows=company_rows(&i, &m) caption="A month of Eustress as an income statement" /> } }}
            {move || { let (i, m, _) = calc(); view! { <Statement rows=treasury_rows(&i, &m) caption="What the Bliss Treasury does with the month" /> } }}
            <p class="eco-sentence">{move || { let (_, m, _) = calc(); company_sentences(&m).join(" ") }}</p>
            <ul class="dl-points eco-notes">
                <li>{plan_note}</li>
                {MONTH_NOTES.iter().map(|n| view! { <li>{*n}</li> }).collect_view()}
            </ul>
            <div class="g-glass eco-controls" role="group" aria-label="Sliders for a month of Eustress">
                <ControlPanel group=Group::Receipts inputs=inputs />
                <ControlPanel group=Group::Eustress inputs=inputs />
                <div class="eco-col eco-col-reset">
                    <h3>"Start again"</h3>
                    <p class="eco-ctl-help">"Puts every slider, in both parts of this page, back to the example month."</p>
                    <button type="button" class="g-btn g-btn-sm" on:click=move |_| inputs.set(Inputs::default())>"Reset all sliders"</button>
                </div>
            </div>
        </section>

        <section class="g-section" id="eco-creator" aria-labelledby="eco-creator-h">
            <div class="g-head"><h2 id="eco-creator-h">"One creator"</h2><span class="g-rule"></span></div>
            <p class="eco-intro">{CREATOR_INTRO}</p>
            <div class="eco-presets" role="group" aria-label="Start from a creator">
                <span class="g-hint">"Start from"</span>
                {PRESETS.iter().map(|p| view! {
                    <button type="button" class="g-chip g-chip-btn" on:click=move |_| inputs.update(|i| i.apply(p))>{p.name}</button>
                }).collect_view()}
                <span class="g-hint">{move || {
                    let (i, _, _) = calc();
                    preset_months(&i).iter().map(|(name, cents)| format!("{name} {}", cents_words(*cents))).collect::<Vec<_>>().join(", ") + " a month on this network"
                }}</span>
            </div>
            <p class="eco-funnel">{move || { let (i, _, c) = calc(); creator_funnel(&i, &c) }}</p>
            <figure class="g-glass eco-fig">
                <div class="eco-chart" inner_html=move || { let (i, _, c) = calc(); creator_svg(&i, &c) }></div>
                <p class="eco-scroll-hint">"Scroll sideways to see the whole chart. The table below has the same figures."</p>
            </figure>
            <div class="eco-readout" role="group" aria-label="The creator at a glance">
                {move || {
                    let (_, _, c) = calc();
                    let k = &c.shown;
                    view! { <Figures items=vec![
                        ("Cash a month", cents_words(k.total)),
                        ("Tickets line", cents_words(k.cash)),
                        ("Bliss line", cents_words(k.bliss)),
                        ("Spendable Tickets", cents_words(k.spendable)),
                    ] /> }
                }}
            </div>
            {move || { let (i, _, c) = calc(); view! { <Statement rows=creator_rows(&i, &c) caption="What one creator gets in a month" /> } }}
            <ul class="dl-points eco-notes">
                {move || { let (i, m, c) = calc(); creator_notes(&i, &m, &c).into_iter().map(|n| view! { <li>{n}</li> }).collect_view() }}
                <li>{TICKETS_NOTE}</li>
            </ul>
            <div class="g-glass eco-controls eco-controls-one" role="group" aria-label="Sliders for one creator">
                <ControlPanel group=Group::Creator inputs=inputs />
            </div>
        </section>

        <section class="g-section" id="eco-assume" aria-labelledby="eco-assume-h">
            <div class="g-head"><h2 id="eco-assume-h">"What the model assumes"</h2><span class="g-rule"></span></div>
            <div class="g-glass dl-table-wrap eco-assume">
                <ul class="dl-points dl-rules">{ASSUMPTIONS.iter().map(|a| view! { <li>{*a}</li> }).collect_view()}</ul>
            </div>
        </section>

        <section class="g-section" aria-labelledby="eco-sources-h">
            <div class="g-head"><h2 id="eco-sources-h">"Where each rule is stated"</h2><span class="g-rule"></span></div>
            <ul class="eco-sources">
                {SOURCES.iter().map(|(href, name, what)| view! {
                    <li class="g-glass"><a href=*href><strong>{*name}</strong><span>{*what}</span></a></li>
                }).collect_view()}
            </ul>
        </section>
    }
}

/// A table of lines: a label, a figure and what it is.
#[component]
fn Statement(rows: Vec<Row>, caption: &'static str) -> impl IntoView {
    view! {
        <div class="g-glass eco-table-wrap">
            <table class="eco-table">
                <caption class="g-sr">{caption}</caption>
                <tbody>
                    {rows.into_iter().map(|r| view! {
                        <tr class=if r.strong { "strong" } else { "" }>
                            <th scope="row">{r.label}</th>
                            <td class="eco-v">{r.value}</td>
                            <td class="eco-note">{r.note}</td>
                        </tr>
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Controls
// -----------------------------------------------------------------------------

/// One group's sliders and choices, in the order the model lists them.
#[component]
fn ControlPanel(group: Group, inputs: RwSignal<Inputs>) -> impl IntoView {
    view! {
        <div class="eco-col">
            <h3>{group.title()}</h3>
            {layout(group).into_iter().map(|item| match item {
                Item::Slider(key) => view! { <Slider spec=super::economics_draft_model::control(key) inputs=inputs /> }.into_any(),
                Item::Choice(pick) => view! { <Choice pick=pick inputs=inputs /> }.into_any(),
            }).collect_view()}
        </div>
    }
}

/// A range input for one number, with its name, its value in words, and a line of help when the name does not say enough.
#[component]
fn Slider(spec: &'static Control, inputs: RwSignal<Inputs>) -> impl IntoView {
    let key = spec.key;
    let id = format!("eco-{key:?}").to_lowercase();
    let value = move || inputs.with(|i| i.get(key));
    let starting = inputs.with_untracked(|i| i.get(key)).to_string();
    view! {
        <label class="eco-ctl">
            <span class="eco-ctl-row">
                <span class="eco-ctl-name">{spec.label}</span>
                <output class="eco-ctl-val">{move || spec.unit.show(value())}</output>
            </span>
            {(!spec.help.is_empty()).then(|| view! { <span class="eco-ctl-help">{spec.help}</span> })}
            <input
                id=id
                type="range"
                min=spec.min.to_string()
                max=spec.max.to_string()
                step=spec.step.to_string()
                value=starting
                prop:value=move || value().to_string()
                aria-valuetext=move || spec.unit.show(value())
                on:input=move |ev| inputs.update(|i| i.set_text(key, &event_target_value(&ev)))
            />
        </label>
    }
}

/// A list to choose one from.
#[component]
fn Choice(pick: Pick, inputs: RwSignal<Inputs>) -> impl IntoView {
    let id = format!("eco-pick-{pick:?}").to_lowercase();
    view! {
        <label class="eco-ctl">
            <span class="eco-ctl-row"><span class="eco-ctl-name">{pick.label()}</span></span>
            <select
                id=id
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    inputs.update(|i| i.pick(pick, &v));
                }
            >
                {pick.options().into_iter().map(|(value, words)| view! {
                    <option value=value selected=inputs.with_untracked(|i| i.picked(pick) == value) prop:selected=move || inputs.with(|i| i.picked(pick) == value)>{words}</option>
                }).collect_view()}
            </select>
        </label>
    }
}

// -----------------------------------------------------------------------------
// 4. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    fn body_html() -> String {
        let owner = Owner::new();
        owner.with(|| EconomicsDraftBody().to_html())
    }

    /// Every string of copy the page owns.
    fn all_copy() -> Vec<String> {
        let mut v: Vec<String> = vec![LEAD.into(), CURRENCIES.into(), DRAFT.into(), STEPS_INTRO.into(), MONTH_INTRO.into(), CREATOR_INTRO.into(), TICKETS_NOTE.into()];
        v.extend(MONTH_NOTES.iter().map(|s| s.to_string()));
        v.extend(ASSUMPTIONS.iter().map(|s| s.to_string()));
        for s in STEPS.iter() {
            v.push(s.title.into());
            v.push(s.body.into());
            if let Some((_, w)) = s.more {
                v.push(w.into());
            }
        }
        for (_, n, w) in SOURCES.iter() {
            v.push((*n).into());
            v.push((*w).into());
        }
        for c in super::super::economics_draft_model::CONTROLS.iter() {
            v.push(c.label.into());
            v.push(c.help.into());
        }
        v
    }

    #[test]
    fn the_page_says_at_the_top_that_it_is_a_draft_and_nothing_can_be_bought() {
        let html = body_html();
        let h1 = html.find("<h1").expect("a heading");
        let banner = html.find("Draft model.").expect("the draft banner");
        let first_step = html.find("The short version").expect("the first section");
        assert!(h1 < banner && banner < first_step, "the draft notice sits between the heading and the content");
        assert!(DRAFT.contains("not a price, a rate or a promise") && DRAFT.contains("not for publication"));
        assert!(html.contains("Economics model: internal draft"));
    }

    #[test]
    fn six_steps_four_built_one_proposed_one_the_models_own() {
        assert_eq!(STEPS.len(), 6);
        let statuses: Vec<Status> = STEPS.iter().map(|s| s.status).collect();
        assert_eq!(statuses, [Status::Built, Status::Built, Status::Built, Status::Built, Status::Proposed, Status::Model]);
        // the reserve and the cash-out are the proposal; the page does not call them built
        assert!(STEPS[4].title.contains("reserve") && STEPS[4].status == Status::Proposed);
        assert!(STEPS_INTRO.contains("Four describe rules that are built") && STEPS_INTRO.contains("one is a proposal"));
        let html = body_html();
        assert_eq!(html.matches("g-chip live").count(), 4);
        assert_eq!(html.matches("g-chip warn").count(), 1);
    }

    #[test]
    fn the_copy_makes_no_promise_the_site_does_not_make() {
        for text in all_copy().iter().chain(std::iter::once(&body_html())) {
            let lower = text.to_lowercase();
            for banned in ["worth", "backed by", "guarantee", "game engine", "risk-free", "passive income", "get rich", "mercury", "\u{2014}", "\u{2013}"] {
                assert!(!lower.contains(banned), "the page says {banned:?}: {}", text.chars().take(160).collect::<String>());
            }
        }
        // Tickets are never said to be cash
        let tickets = TICKETS_NOTE.to_lowercase();
        assert!(tickets.contains("no cash value") && tickets.contains("never tickets they buy"));
    }

    #[test]
    fn the_page_does_not_make_the_investments_an_offer() {
        let html = body_html();
        assert!(html.contains("Nothing here is offered"), "the Treasury note says nothing is offered");
        assert!(STEPS[5].body.contains("Nothing like that is offered"));
        // the controls call the money a what-if
        let invest = super::super::economics_draft_model::control(super::super::economics_draft_model::Key::Invest);
        assert!(invest.help.contains("not an offer"));
    }

    #[test]
    fn every_internal_link_goes_to_a_route_the_app_has() {
        let routes = include_str!("../app.rs");
        let mut links: Vec<&str> = STEPS.iter().filter_map(|s| s.more.map(|m| m.0)).collect();
        links.extend(SOURCES.iter().map(|s| s.0));
        assert!(links.len() >= 6);
        for link in links {
            let path = link.split('#').next().unwrap();
            assert!(routes.contains(&format!("path!(\"{path}\")")), "app.rs has no route for {link}");
        }
        // and the in-page jump links go to sections that exist
        let html = body_html();
        for id in ["eco-how", "eco-month", "eco-creator", "eco-assume"] {
            assert!(html.contains(&format!("href=\"#{id}\"")) && html.contains(&format!("id=\"{id}\"")), "{id}");
        }
    }

    #[test]
    fn the_economics_route_exists_and_the_page_is_not_listed_anywhere_visitors_look() {
        let app = include_str!("../app.rs");
        assert!(app.contains("path!(\"/economics-draft\")") && app.contains("EconomicsDraftPage"), "the route is wired");
        // A draft stays unlisted until McKale lists it: no nav link, no footer link, no sitemap entry, no llms.txt line.
        let nav = include_str!("../components/nav.rs");
        let footer = include_str!("../components/footer.rs");
        let sitemap = include_str!("../../sitemap.xml");
        let llms = include_str!("../../llms.txt");
        for (name, text) in [("nav.rs", nav), ("footer.rs", footer), ("sitemap.xml", sitemap), ("llms.txt", llms)] {
            assert!(!text.contains("/economics"), "{name} links the draft Economics page; update this test and the note at the top of economics.rs when it is listed");
        }
    }

    #[test]
    fn the_draft_carries_the_colors_only_its_charts_use_in_a_style_of_its_own() {
        let html = body_html();
        assert!(html.contains("<style>") && html.contains("--eco-invest") && html.contains(".eco-svg .n-reserve"), "the draft's palette is in the page");
        // the shipped stylesheet has none of it
        let shipped = include_str!("../../style/gallery.css");
        for word in ["--eco-invest", "--eco-reserve", "--eco-expenses", "--eco-taxes", "--eco-profit", "--eco-kept", ".n-toinv", ".c-bad"] {
            assert!(!shipped.contains(word), "gallery.css has {word}");
            assert!(DRAFT_CSS.contains(word), "the draft's file has {word}");
        }
        // and index.html does not load the draft's file
        assert!(!include_str!("../../index.html").contains("economics_draft"), "index.html loads the draft's stylesheet");
    }

    #[test]
    fn the_public_model_is_the_draft_with_what_is_not_built_switched_off() {
        use super::super::economics_draft_model as d;
        use super::super::economics_model as p;
        // the same month in both: the draft's money put in and repaid is off; every control the public page has is set to the same value
        // the draft has no Roblox scale and no admin costs: those public controls have no draft twin
        fn same_key(k: p::Key) -> Option<d::Key> {
            Some(match k {
                p::Key::Tickets => d::Key::Tickets,
                p::Key::PlayerPlus => d::Key::PlayerPlus,
                p::Key::Creator => d::Key::Creator,
                p::Key::Bundle => d::Key::Bundle,
                p::Key::Professional => d::Key::Professional,
                p::Key::Delta => d::Key::Delta,
                p::Key::Players => d::Key::Players,
                p::Key::Conversion => d::Key::Conversion,
                p::Key::Average => d::Key::Average,
                p::Key::Paid => d::Key::Paid,
                p::Key::Hours => d::Key::Hours,
                p::Key::Updates => d::Key::Updates,
                p::Key::Lift => d::Key::Lift,
                p::Key::Others => d::Key::Others,
                p::Key::Score => d::Key::Score,
                p::Key::Market | p::Key::Labor | p::Key::Hosted | p::Key::HoursPerPlayer => return None,
            })
        }
        let near = |a: f64, b: f64, what: &str| assert!((a - b).abs() <= 1e-6 * a.abs().max(1.0), "{what}: public {a}, draft {b}");
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / ((1u64 << 31) as f64)
        };
        let mut compared = 0;
        for round in 0..400 {
            let mut pm = p::Inputs::default();
            let mut dm = d::Inputs::default();
            dm.invest = 0.0;
            dm.principal = 0.0;
            dm.repay = 0.0;
            dm.ret = 0.0;
            if round > 0 {
                for c in p::CONTROLS.iter() {
                    let Some(dk) = same_key(c.key) else { continue };
                    // a value both sliders can hold (the public Players slider reaches further than the draft's)
                    let top = c.max.min(d::control(dk).max);
                    let steps = ((top - c.min) / c.step).floor();
                    let v = c.min + (next() * (steps + 1.0)).floor().min(steps) * c.step;
                    pm.set(c.key, v);
                    dm.set(dk, v);
                }
                for pick in [p::Pick::Pack, p::Pick::Node] {
                    let options = pick.options();
                    let (v, _) = &options[(next() * options.len() as f64) as usize % options.len()];
                    pm.pick(pick, v);
                    dm.pick(if matches!(pick, p::Pick::Pack) { d::Pick::Pack } else { d::Pick::Node }, v);
                }
            }
            let (pc, dc) = (p::company(&pm), d::company(&dm));
            near(pc.gross, dc.gross, "gross");
            near(pc.fees, dc.fees, "fees");
            near(pc.net, dc.net, "net");
            near(pc.treasury_share, dc.treasury_share, "the Treasury's half");
            near(pc.eustress, dc.eustress, "Eustress's half");
            near(pc.drip, dc.drip, "the drip");
            assert_eq!((pc.shown.gross, pc.shown.fees, pc.shown.net, pc.shown.treasury, pc.shown.eustress), (dc.shown.gross, dc.shown.fees, dc.shown.net, dc.shown.treasury, dc.shown.eustress), "printed dollars, round {round}");
            let (pk, dk) = (p::creator(&pm, &pc), d::creator(&dm, &dc));
            for (what, a, b) in [
                ("players", pk.players, dk.players),
                ("buyers", pk.buyers, dk.buyers),
                ("spent", pk.spent, dk.spent),
                ("earned", pk.earned, dk.earned),
                ("eligible", pk.eligible, dk.eligible),
                ("counted", pk.counted, dk.counted),
                ("sales score", pk.value_score, dk.value_score),
                ("effort", pk.effort, dk.effort),
                ("total score", pk.total, dk.total),
                ("gate", pk.gate, dk.gate),
                ("Bliss", pk.bliss, dk.bliss),
            ] {
                near(a, b, what);
            }
            assert_eq!((pk.shown.effort, pk.shown.value, pk.shown.bliss), (dk.shown.effort, dk.shown.value, dk.shown.bliss), "printed cents, round {round}");
            compared += 1;
        }
        assert_eq!(compared, 400);
        // and the presets
        for (pp, dp) in p::PRESETS.iter().zip(d::PRESETS.iter()) {
            let mut pm = p::Inputs::default();
            pm.apply(pp);
            let mut dm = d::Inputs::default();
            dm.invest = 0.0;
            dm.principal = 0.0;
            dm.repay = 0.0;
            dm.apply(dp);
            let (pc, dc) = (p::company(&pm), d::company(&dm));
            assert_eq!(p::creator(&pm, &pc).shown.bliss, d::creator(&dm, &dc).shown.bliss, "{} and {}", pp.name, dp.name);
        }
    }

    #[test]
    fn the_hosts_headers_keep_the_page_out_of_search() {
        let headers = include_str!("../../_headers");
        let at = headers.find("/economics-draft").expect("a rule for /economics-draft");
        let rule = &headers[at..];
        assert!(rule.lines().take(3).any(|l| l.trim().eq_ignore_ascii_case("X-Robots-Tag: noindex, nofollow")), "{rule}");
    }

    #[test]
    fn the_body_has_every_slider_and_every_choice_and_the_default_figures() {
        use super::super::economics_draft_model::{CONTROLS, Pick};
        let html = body_html();
        assert_eq!(html.matches("type=\"range\"").count(), CONTROLS.len(), "one range input for every control");
        assert_eq!(html.matches("<select").count(), 4, "four choices");
        for p in [Pick::Pack, Pick::Channel, Pick::Basis, Pick::Node] {
            assert!(html.contains(p.label()), "{}", p.label());
        }
        assert_eq!(html.matches("<svg").count(), 2, "the two charts");
        assert!(!html.contains("<script"));
        for want in ["$178,992", "$168,751", "$84,375", "$36,255", "$9,064", "$27,191", "$509.41", "$462.63", "$46.78", "$307.24"] {
            assert!(html.contains(want), "the default page shows {want}");
        }
        // the statement tables read the month in order
        let a = html.find("Gross receipts").unwrap();
        let b = html.find("Net profit kept").unwrap();
        assert!(a < b);
    }

    #[test]
    fn each_slider_carries_its_range_and_its_value_in_words() {
        use super::super::economics_draft_model::{CONTROLS, Key};
        let html = body_html();
        for c in CONTROLS.iter() {
            let id = format!("id=\"eco-{:?}\"", c.key).to_lowercase();
            assert!(html.contains(&id), "{} has an id", c.label);
            assert!(html.contains(c.label), "{} is named", c.label);
        }
        // each range input says its own limits: its tag carries this control's min, max and step and no other's
        for c in CONTROLS.iter() {
            let at = html.find(&format!("id=\"eco-{:?}\"", c.key).to_lowercase()).expect("the slider exists");
            let start = html[..at].rfind("<input").expect("an input tag");
            let end = at + html[at..].find('>').expect("the tag closes");
            let tag = &html[start..end];
            assert!(tag.contains("type=\"range\""), "{} is a range: {tag}", c.label);
            for (name, v) in [("min", c.min), ("max", c.max), ("step", c.step)] {
                assert!(tag.contains(&format!("{name}=\"{v}\"")), "{} has {name}=\"{v}\": {tag}", c.label);
            }
        }
        assert!(html.contains("$100,000") && html.contains("$1.50") && html.contains("60 h"), "the values are in words");
        let _ = Key::Tickets;
    }

    #[test]
    fn the_plan_note_is_worked_from_the_plans_not_typed() {
        let html = body_html();
        assert!(html.contains("$0.00926 each") && html.contains("Player Plus is 83% of its invoice and Bundle 29%"), "{}", &html[..html.len().min(200)]);
    }

    #[test]
    fn the_first_assumption_states_the_models_month_from_the_constant() {
        assert!(ASSUMPTIONS[0].starts_with(&format!("A month is {} days", super::super::economics_draft_model::DAYS as i64)), "{}", ASSUMPTIONS[0]);
    }

    #[test]
    fn the_three_currencies_are_named_and_delta_is_never_said_to_convert() {
        for name in ["Tickets", "Bliss", "Delta"] {
            assert!(CURRENCIES.contains(name), "{name}");
        }
        assert!(CURRENCIES.contains("Delta never converts into either and is never cashed out"));
        assert!(body_html().contains("Eustress has three currencies"));
    }

    #[test]
    fn statuses_have_words_and_a_chip_each() {
        for s in [Status::Built, Status::Proposed, Status::Model] {
            assert!(!s.words().is_empty() && s.chip().starts_with("g-chip"));
        }
        assert_eq!(Status::Built.words(), "Built");
        assert_eq!(Status::Proposed.words(), "Proposed");
    }
}
