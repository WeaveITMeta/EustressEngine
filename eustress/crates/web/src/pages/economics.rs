// =============================================================================
// Eustress Web - Economics (/economics)
// =============================================================================
// How money moves through Eustress, explained in five steps and then made testable: a month of receipts followed from the buyer's card to
// the contributors the Treasury pays, and what one creator is paid, each with sliders. The arithmetic is economics_model.rs, the drawing
// is economics_chart.rs (on economics_kit.rs), and this file lays the page out.
//
// Everything the page says is a rule the Worker applies today. Proposals that are not decided (a cash-out reserve, money put into the
// Treasury and repaid) and the operating numbers (costs, tax) are not here: they are in the internal draft (economics_draft.rs, compiled
// only with the `economics-draft` feature), so the public bundle holds none of their words. A test keeps those words out.
//
// Two modes. Theoretical is the model the sliders move; Actual reads the platform's real numbers from the Worker every minute
// (economics_live.rs): the Treasury and its payouts for everyone, and for an administrator the receipts by kind, the cost ledger and
// where Eustress's half went. A signed-in creator can start the creator's sliders from their own Spaces (economics_mine.rs).
//
// The page is a model for testing and says so at the top. It is not in the nav, the footer, the sitemap or llms.txt, and the host's headers
// tell search engines to skip it (`_headers`); a test fails if any of those changes without this note being updated.
//
// Table of Contents:
// 1. The copy
// 2. The page
// 3. Controls
// 4. Tests
// =============================================================================

use leptos::prelude::*;

use super::economics_chart::{
    bliss_svg, company_rows, company_sentences, company_svg, creator_funnel, creator_notes, creator_rows, expenses_rows, expenses_svg, payouts_svg, tickets_svg, Row,
};
use super::economics_live::{self as live, AdminStatement, PublicStatement};
#[cfg_attr(feature = "ssr", allow(unused_imports))]
use super::economics_model::{
    commas, company, control, creator, dollars, cents_words, expenses, layout, Company, Control, Creator, Group, Inputs, Item, Key, Pick, PRESETS,
};
use crate::components::currency_ui::Figures;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// 1. The copy
// -----------------------------------------------------------------------------

pub struct Step {
    pub title: &'static str,
    pub body: &'static str,
    /// A page with the details: its address and the words for the link.
    pub more: Option<(&'static str, &'static str)>,
}

pub static STEPS: [Step; 5] = [
    Step {
        title: "A buyer pays",
        body: "Tickets, a plan or a Delta pack. The payment fee comes off first: Stripe's 2.9% plus 30 cents on every web payment, which is how every purchase is made today.",
        more: Some(("/tickets", "Ticket packs")),
    },
    Step {
        title: "The net is split in half",
        body: "Half goes to the Bliss Treasury and half is Eustress's. Tickets, Delta packs and plan payments all follow the same split.",
        more: None,
    },
    Step {
        title: "The Treasury pays contributors every day",
        body: "Each day the Treasury pays out 0.276% of its balance, or 0.136% while it is at or below 15% of its highest balance. The payout is shared by score, which comes from hours in Studio and from sales, so the more contributors share a day, the smaller each share.",
        more: Some(("/docs/earning#payouts-drip", "The daily drip")),
    },
    Step {
        title: "Bliss is paid in dollars to verified contributors",
        body: "A contributor has to verify their identity (18 or older), connect a Stripe account and have a Creator, Professional or Bundle plan. A share that cannot be paid, including a transfer under $0.50, stays in the Treasury.",
        more: Some(("/bliss", "Bliss")),
    },
    Step {
        title: "A creator earns Tickets when Tickets are spent on their work",
        body: "The creator receives 70% of the price as earned Tickets, which they can spend inside Eustress, and the other 30% leaves circulation. The 70% is the rule today and is not final. No storefront sells for Tickets yet, and Tickets cannot be converted to Bliss or dollars.",
        more: Some(("/docs/earning#tickets-sales", "Selling for Tickets")),
    },
];

pub const LEAD: &str = "Follow a month of receipts from a buyer's card to a creator's payout, and move the sliders to test what changes.";

pub const NOTICE: &str = "Every figure is an example, not a price and not a forecast of what anyone will be paid, and nothing you move leaves your browser.";

pub const CURRENCIES: &str = "Eustress has three currencies: Tickets buy things, Bliss rewards contributors and Delta pays for premium Studio tools. This page follows the money behind Tickets and Bliss. Delta never converts into either and is never cashed out.";

pub const STEPS_INTRO: &str = "Five steps, each a rule that is built today.";

pub const THEORETICAL_INTRO: &str = "Theoretical: a model you move with the sliders. Every figure is an example.";

pub const ACTUAL_INTRO: &str = "Actual: the Treasury's real balance and what it paid contributors, read from the platform every minute. Revenue and costs show to the administrator only.";

pub const ADMIN_COSTS_INTRO: &str = "What Eustress's half pays for. Theoretical costs follow the hosting plan's assumptions (docs/architecture/HOSTING_COST_PLAN.md), none of them measured yet; labor is your share of the half. The sales score rule is today's or a proposal to compare before the Worker changes.";

pub const MONTH_INTRO: &str = "Set what Eustress sells in a month. The chart follows the money from receipts: the payment fee comes off first, then the net is split in half, and the Treasury pays its half out to contributors.";

pub const CREATOR_INTRO: &str = "Choose how popular a creator is and how much time they spend in Studio. The first chart shows what a sale pays them in Tickets; the second shows their share of the Treasury's payout in dollars. That share depends on how many other contributors there are and on the month above, so it moves when either changes.";

pub const TICKETS_NOTE: &str = "Tickets are counted in Tickets here, never in dollars. They can be spent inside Eustress and cannot be converted to Bliss or dollars.";

pub const MONTH_NOTES: [&str; 2] = [
    "Fees come off first, then the net is split. Today a refund is taken back from both halves, and a gift to the Treasury is credited whole.",
    "These are examples: every volume, the number of other contributors and their scores, the players each update brings back, and the split of a creator's hours.",
];

pub const ASSUMPTIONS: [&str; 6] = [
    "A month is 30 days, and the Treasury is at a steady state: it pays out each month what comes in.",
    "Studio hours are 40% scripting, 40% building and 20% other time. The weights are 3.0, 2.5 and 1.0 on a Light node, and a Full node earns 10% more.",
    "Sales add 0.5 score for each Ticket a creator earns from Tickets bought with money. One buyer's purchases count up to 1,000 of those Tickets in a day, and a creator's sales score stops at 9,600 a day. Each buyer is taken to spend the average in one day. Updates have no score of their own.",
    "The other contributors are a number and an average daily score. A creator's share follows them more than anything one creator does.",
    "A Delta pack order averages $25, which sets the fixed part of Stripe's fee on Delta packs.",
    "Refunds and chargebacks are not modeled, and every contributor shown is assumed to be paid.",
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
pub fn EconomicsPage() -> impl IntoView {
    view! {
        <div class="page g-page dl-page eco-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main dl-main eco-main">
                <div class="g-wrap eco-wrap">
                    <EconomicsBody />
                </div>
            </main>
            <Footer />
        </div>
    }
}

/// Where `/economics-draft` goes in a build without the internal draft (which is every shipped build): the answer the router gives any address
/// the site does not have.
#[cfg(not(feature = "economics-draft"))]
#[component]
pub fn EconomicsDraftPage() -> impl IntoView {
    view! { <crate::pages::NotFoundPage /> }
}

/// Everything the page says and lets the reader move, without the site's frame (so tests can render it alone).
#[component]
pub fn EconomicsBody() -> impl IntoView {
    let inputs = RwSignal::new(Inputs::default());

    // A page that is not listed stays out of search results: tell crawlers that run scripts, as the response header tells those that do not.
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

    // The mode, whether the reader is an administrator (the Worker decides: its admin statement answers or refuses), the period
    // Actual mode shows and what the Worker last said about it, and the creator's own-projects picker.
    let mode = RwSignal::new(Mode::Theoretical);
    let admin = RwSignal::new(false);
    let period = RwSignal::new(live::PeriodPick::default());
    let live_state = RwSignal::new(live::LiveState::default());
    let mine_open = RwSignal::new(false);
    let mine_note = RwSignal::new(None::<String>);

    #[cfg(not(feature = "ssr"))]
    {
        // Ask once for this month's admin statement: an administrator gets it, anyone else a refusal, and the page shows the admin
        // parts only for the first.
        leptos::task::spawn_local(async move {
            if let Some(a) = live::load_admin(&live::PeriodPick::default()).await {
                live_state.update(|l| l.admin = Some(a));
                admin.set(true);
            }
        });
        // Actual mode reads its period when it opens and when the period changes, and again every minute while it is shown.
        Effect::new(move |_| {
            let p = period.get();
            if mode.get() == Mode::Actual {
                leptos::task::spawn_local(live::refresh(p, live_state, admin));
            }
        });
        let timer = gloo_timers::callback::Interval::new(60_000, move || {
            if mode.get_untracked() == Mode::Actual {
                leptos::task::spawn_local(live::refresh(period.get_untracked(), live_state, admin));
            }
        });
        let held_timer = StoredValue::new_local(Some(timer));
        on_cleanup(move || held_timer.update_value(|t| drop(t.take())));
    }

    view! {
        <header class="dl-head eco-head">
            <div class="g-kicker"><span class="g-kick-rule"></span>"Economics"</div>
            <h1 class="dl-title">"How money moves through Eustress"</h1>
            <p class="dl-lead">{LEAD}</p>
        </header>
        <p class="cur-banner eco-draft" role="note"><strong>"Model for testing. "</strong>{NOTICE}</p>
        <div class="eco-mode" role="tablist" aria-label="Mode">
            <button type="button" role="tab" class="eco-mode-tab" aria-selected=move || (mode.get() == Mode::Theoretical).to_string()
                on:click=move |_| mode.set(Mode::Theoretical)>"Theoretical"</button>
            <button type="button" role="tab" class="eco-mode-tab" aria-selected=move || (mode.get() == Mode::Actual).to_string()
                on:click=move |_| mode.set(Mode::Actual)>"Actual"</button>
            <span class="g-hint">{move || if mode.get() == Mode::Theoretical { THEORETICAL_INTRO } else { ACTUAL_INTRO }}</span>
        </div>
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
            <ol class="eco-steps eco-steps-five">
                {STEPS.iter().map(|s| view! {
                    <li class="g-glass eco-step">
                        <h3>{s.title}</h3>
                        <p>{s.body}</p>
                        {s.more.map(|(href, words)| view! { <p class="eco-more"><a href=href>{words}</a></p> })}
                    </li>
                }).collect_view()}
            </ol>
        </section>

        <Show when=move || mode.get() == Mode::Actual>
            <ActualView period=period live_state=live_state admin=admin />
        </Show>
        <div class="eco-theory" style:display=move || if mode.get() == Mode::Theoretical { "" } else { "none" }>
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
                        ("Payment fees", dollars(d.fees)),
                        ("Paid to contributors", dollars(d.treasury)),
                        ("Eustress's half", dollars(d.eustress)),
                    ] /> }
                }}
            </div>
            {move || { let (_, m, _) = calc(); view! { <Statement rows=company_rows(&m) caption="A month of Eustress as a statement" /> } }}
            <p class="eco-sentence">{move || { let (_, m, _) = calc(); company_sentences(&m).join(" ") }}</p>
            <ul class="dl-points eco-notes">
                {MONTH_NOTES.iter().map(|n| view! { <li>{*n}</li> }).collect_view()}
            </ul>
            <div class="g-glass eco-controls eco-controls-one" role="group" aria-label="Sliders for a month of Eustress">
                <ControlPanel group=Group::Receipts inputs=inputs />
            </div>
            <div class="eco-presets">
                <button type="button" class="g-btn g-btn-sm" on:click=move |_| inputs.set(Inputs::default())>"Reset all sliders"</button>
                <span class="g-hint">"Puts every slider on this page back to the example month."</span>
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
                <button type="button" class="g-btn g-btn-sm" on:click=move |_| inputs.set(Inputs::default())>"Reset all sliders"</button>
                <button type="button" class="g-chip g-chip-btn eco-mine-open" on:click=move |_| mine_open.update(|o| *o = !*o)>"My projects"</button>
            </div>
            <Show when=move || mine_open.get()>
                <MinePicker inputs=inputs note=mine_note />
            </Show>
            {move || mine_note.get().map(|n| view! { <p class="g-hint eco-mine-note">{n}</p> })}
            <p class="eco-funnel">{move || { let (i, _, c) = calc(); creator_funnel(&i, &c) }}</p>
            <figure class="g-glass eco-fig">
                <div class="eco-chart" inner_html=move || { let (i, _, c) = calc(); tickets_svg(&i, &c) }></div>
                <p class="eco-scroll-hint">"Scroll sideways to see the whole chart. The table below has the same figures."</p>
            </figure>
            <figure class="g-glass eco-fig">
                <div class="eco-chart" inner_html=move || { let (i, _, c) = calc(); bliss_svg(&i, &c) }></div>
                <p class="eco-scroll-hint">"Scroll sideways to see the whole chart. The table below has the same figures."</p>
            </figure>
            <div class="eco-readout" role="group" aria-label="The creator at a glance">
                {move || {
                    let (_, _, c) = calc();
                    view! { <Figures items=vec![
                        ("Bliss payout a month", cents_words(c.shown.bliss)),
                        ("Tickets earned", format!("{} Tickets", commas(c.tickets.earned))),
                        ("Share of score", format!("{:.1}%", if c.total > 0.0 { 100.0 * c.mine / c.total } else { 0.0 })),
                    ] /> }
                }}
            </div>
            {move || { let (i, _, c) = calc(); view! { <Statement rows=creator_rows(&i, &c) caption="What one creator is paid in a month" /> } }}
            <ul class="dl-points eco-notes">
                {move || { let (i, m, c) = calc(); creator_notes(&i, &m, &c).into_iter().map(|n| view! { <li>{n}</li> }).collect_view() }}
                <li>{TICKETS_NOTE}</li>
            </ul>
            <div class="g-glass eco-controls eco-controls-one" role="group" aria-label="Sliders for one creator">
                <ControlPanel group=Group::Creator inputs=inputs />
            </div>
        </section>

        <Show when=move || admin.get()>
            <section class="g-section" id="eco-costs" aria-labelledby="eco-costs-h">
                <div class="g-head"><h2 id="eco-costs-h">"Where Eustress's half goes (admin)"</h2><span class="g-rule"></span></div>
                <p class="eco-intro">{ADMIN_COSTS_INTRO}</p>
                <figure class="g-glass eco-fig">
                    <div class="eco-chart" inner_html=move || { let (i, m, _) = calc(); expenses_svg(&expenses(&i, &m), "a month") }></div>
                </figure>
                {move || { let (i, m, _) = calc(); view! { <Statement rows=expenses_rows(&expenses(&i, &m)) caption="Where Eustress's half goes in a month" /> } }}
                <div class="g-glass eco-controls eco-controls-one" role="group" aria-label="Sliders for costs and proposals">
                    <ControlPanel group=Group::Admin inputs=inputs />
                </div>
            </section>
        </Show>
        </div>

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
                Item::Slider(key) => view! { <Slider spec=control(key) inputs=inputs /> }.into_any(),
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
    let starting = spec.position(inputs.with_untracked(|i| i.get(key))).to_string();
    let (min, max, step) = spec.input_range();
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
                min=min.to_string()
                max=max.to_string()
                step=step.to_string()
                value=starting
                prop:value=move || spec.position(value()).to_string()
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

/// Theoretical (the sliders) or Actual (the platform's numbers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Theoretical,
    Actual,
}

/// Actual mode: the period, the Treasury and its payouts for everyone, and for an administrator the receipts, the costs and the ledger.
#[component]
fn ActualView(period: RwSignal<live::PeriodPick>, live_state: RwSignal<live::LiveState>, admin: RwSignal<bool>) -> impl IntoView {
    let choices = live::period_choices_now();
    let download = move |_| {
        #[cfg(not(feature = "ssr"))]
        live_state.with_untracked(|l| {
            if admin.get_untracked() {
                if let Some(a) = &l.admin {
                    crate::pages::login::download_file(&live::csv_name(&a.public.period.label, true), &live::admin_csv(a));
                    return;
                }
            }
            if let Some(p) = &l.public {
                crate::pages::login::download_file(&live::csv_name(&p.period.label, false), &live::public_csv(p));
            }
        });
    };
    view! {
        <section class="g-section" id="eco-actual" aria-labelledby="eco-actual-h">
            <div class="g-head"><h2 id="eco-actual-h">"Actual"</h2><span class="g-rule"></span></div>
            <p class="eco-intro">{ACTUAL_INTRO}</p>
            <div class="eco-presets">
                <label class="eco-period">
                    <span class="g-hint">"Period"</span>
                    <select on:change=move |ev| {
                        let v = event_target_value(&ev);
                        if let Some((k, at)) = v.split_once('|') {
                            period.set(live::PeriodPick { kind: if k == "quarter" { "quarter" } else { "month" }, at: at.to_string() });
                        }
                    }>
                        {choices.into_iter().map(|(k, at, words)| view! { <option value=format!("{k}|{at}")>{words}</option> }).collect_view()}
                    </select>
                </label>
                <span class="g-hint">{move || live_state.with(|l| {
                    if l.loading { "Reading the platform's numbers.".to_string() }
                    else if l.read_at.is_empty() { String::new() }
                    else { format!("Read at {}. Refreshes every minute.", l.read_at) }
                })}</span>
                <button type="button" class="g-btn g-btn-sm" on:click=download>"Download CSV"</button>
            </div>
            {move || live_state.with(|l| l.error.clone()).map(|e| view! { <p class="cur-banner" role="alert">{e}</p> })}
            {move || live_state.with(|l| l.public.clone()).map(|p| view! { <PublicActual p=p /> })}
            {move || if admin.get() { live_state.with(|l| l.admin.clone()).map(|a| view! { <AdminActual a=a live_state=live_state period=period admin=admin /> }) } else { None }}
        </section>
    }
}

/// What everyone sees in Actual mode: the Treasury now and its payouts in the period.
#[component]
fn PublicActual(p: PublicStatement) -> impl IntoView {
    let t = &p.treasury;
    let figures = vec![
        ("Treasury now", live::usd_words(t.balance_usd)),
        ("Paid out a day now", live::usd_words(t.daily_drip_usd)),
        ("Paid in the period", live::usd_words(p.contributors.paid_usd)),
        ("Paid since the start", live::usd_words(t.paid_all_time_usd)),
    ];
    let svg = payouts_svg(&live::payout_days(&p), &format!("in {}", p.period.label));
    let note = format!(
        "{} of {} days paid so far, {} contributors a day on average. The Treasury pays {} of its balance a day{}. Its highest balance, decaying 0.171% a day, is {}.",
        p.contributors.days_paid,
        p.period.months.len() * 30,
        p.contributors.contributors_per_day,
        if t.scarcity_active { "0.136%" } else { "0.276%" },
        if t.scarcity_active { ", slower while it is at or below 15% of its highest balance" } else { "" },
        live::usd_words(t.high_water_usd)
    );
    view! {
        <div class="eco-readout" role="group" aria-label="The Treasury now"><Figures items=figures /></div>
        <figure class="g-glass eco-fig"><div class="eco-chart" inner_html=svg></div></figure>
        <p class="eco-sentence">{note}</p>
    }
}

/// What an administrator sees in Actual mode: the receipts drawn as the month chart, where Eustress's half went, and the cost ledger.
#[component]
fn AdminActual(a: AdminStatement, live_state: RwSignal<live::LiveState>, period: RwSignal<live::PeriodPick>, admin: RwSignal<bool>) -> impl IntoView {
    let m = live::company_from_actual(&a);
    let e = live::expenses_from_actual(&a);
    let label = a.public.period.label.clone();
    let mut rows = company_rows(&m);
    rows.push(Row { label: "Refunded and disputed", value: format!("-{}", live::usd_words(a.receipts.totals.refunded_usd)), note: "in the period, before the halves above".to_string(), strong: false });
    let month_svg = company_svg(&Inputs::default(), &m);
    let costs_svg = expenses_svg(&e, &format!("in {label}"));
    let first_month = a.public.period.months.first().cloned().unwrap_or_default();
    view! {
        <h3 class="eco-sub">"Receipts (admin)"</h3>
        <figure class="g-glass eco-fig"><div class="eco-chart" inner_html=month_svg></div></figure>
        <Statement rows=rows caption="Receipts in the period" />
        <h3 class="eco-sub">"Where Eustress's half went (admin)"</h3>
        <figure class="g-glass eco-fig"><div class="eco-chart" inner_html=costs_svg></div></figure>
        <Statement rows=expenses_rows(&e) caption="Where Eustress's half went in the period" />
        <CostLedger month=first_month labor_share=a.expenses.labor.share live_state=live_state period=period admin=admin />
    }
}

/// The administrator's cost ledger: set one category of one month from the bills, set labor's share, rebuild the month totals.
// The handlers save only in the browser; the server render draws the form.
#[component]
fn CostLedger(month: String, labor_share: f64, live_state: RwSignal<live::LiveState>, period: RwSignal<live::PeriodPick>, admin: RwSignal<bool>) -> impl IntoView {
    let month = RwSignal::new(month);
    let category = RwSignal::new(live::COST_CATEGORIES[0].0.to_string());
    let amount = RwSignal::new(String::new());
    let note = RwSignal::new(String::new());
    let share = RwSignal::new(format!("{}", (labor_share * 1000.0).round() / 10.0));
    let said = RwSignal::new(None::<String>);
    // the server render draws the form; only the browser saves
    #[cfg(feature = "ssr")]
    let _ = (live_state, period, admin);
    let save_cost = move |_| {
        #[cfg(not(feature = "ssr"))]
        {
            let (m, c, a, n) = (month.get_untracked(), category.get_untracked(), amount.get_untracked(), note.get_untracked());
            leptos::task::spawn_local(async move {
                said.set(Some(live::put_cost(&m, &c, &a, &n).await));
                live::refresh(period.get_untracked(), live_state, admin).await;
            });
        }
    };
    let save_share = move |_| {
        #[cfg(not(feature = "ssr"))]
        {
            let v = share.get_untracked();
            leptos::task::spawn_local(async move {
                said.set(Some(live::put_labor_share(&v).await));
                live::refresh(period.get_untracked(), live_state, admin).await;
            });
        }
    };
    let rebuild = move |_| {
        #[cfg(not(feature = "ssr"))]
        leptos::task::spawn_local(async move {
            said.set(Some(live::rebuild_totals().await));
            live::refresh(period.get_untracked(), live_state, admin).await;
        });
    };
    view! {
        <div class="g-glass eco-ledger" role="group" aria-label="The cost ledger">
            <h3>"Cost ledger (admin)"</h3>
            <p class="eco-ctl-help">"Enter each month's bills; saving a category again replaces it, and 0 removes it. Labor is a share of Eustress's half."</p>
            <div class="eco-ledger-row">
                <label class="eco-ctl"><span class="eco-ctl-name">"Month"</span>
                    <input type="month" prop:value=move || month.get() on:input=move |ev| month.set(event_target_value(&ev)) /></label>
                <label class="eco-ctl"><span class="eco-ctl-name">"Category"</span>
                    <select on:change=move |ev| category.set(event_target_value(&ev))>
                        {live::COST_CATEGORIES.iter().map(|(v, w)| view! { <option value=*v>{*w}</option> }).collect_view()}
                    </select></label>
                <label class="eco-ctl"><span class="eco-ctl-name">"Amount (USD)"</span>
                    <input type="number" min="0" step="0.01" inputmode="decimal" prop:value=move || amount.get() on:input=move |ev| amount.set(event_target_value(&ev)) /></label>
                <label class="eco-ctl"><span class="eco-ctl-name">"Note"</span>
                    <input type="text" maxlength="200" prop:value=move || note.get() on:input=move |ev| note.set(event_target_value(&ev)) /></label>
                <button type="button" class="g-btn g-btn-sm" on:click=save_cost>"Save cost"</button>
            </div>
            <div class="eco-ledger-row">
                <label class="eco-ctl"><span class="eco-ctl-name">"Labor (Simbuilder), % of Eustress's half"</span>
                    <input type="number" min="0" max="100" step="1" prop:value=move || share.get() on:input=move |ev| share.set(event_target_value(&ev)) /></label>
                <button type="button" class="g-btn g-btn-sm" on:click=save_share>"Save share"</button>
                <button type="button" class="g-btn g-btn-sm" on:click=rebuild>"Rebuild month totals"</button>
            </div>
            {move || said.get().map(|s| view! { <p class="g-hint" role="status">{s}</p> })}
        </div>
    }
}

/// A signed-in creator's Spaces to start the creator's sliders from.
#[component]
fn MinePicker(inputs: RwSignal<Inputs>, note: RwSignal<Option<String>>) -> impl IntoView {
    let projects = RwSignal::new(None::<Result<Vec<super::economics_mine::MyProject>, String>>);
    let picked = RwSignal::new(std::collections::BTreeSet::<String>::new());
    #[cfg(feature = "ssr")]
    let _ = (inputs, note);
    #[cfg(not(feature = "ssr"))]
    leptos::task::spawn_local(async move { projects.set(Some(live::load_my_projects().await)) });
    let apply = move |_| {
        #[cfg(not(feature = "ssr"))]
        {
            let chosen: Vec<super::economics_mine::MyProject> = projects.with_untracked(|p| match p {
                Some(Ok(list)) => picked.with_untracked(|ids| list.iter().filter(|x| ids.contains(&x.id)).cloned().collect()),
                _ => Vec::new(),
            });
            if chosen.is_empty() {
                note.set(Some("Pick at least one Space.".to_string()));
                return;
            }
            leptos::task::spawn_local(async move {
                let ids: Vec<String> = chosen.iter().map(|p| p.id.clone()).collect();
                let sales = live::load_sales(&ids).await;
                let a = super::economics_mine::attribute(&chosen, &sales, js_sys::Date::now() / 1000.0);
                inputs.update(|i| {
                    i.set(Key::Players, a.players);
                    i.set(Key::Conversion, a.conversion);
                    if a.average > 0.0 {
                        i.set(Key::Average, a.average);
                    }
                });
                note.set(Some(a.note));
            });
        }
    };
    view! {
        <div class="g-glass eco-mine" role="group" aria-label="Start from my projects">
            {move || match projects.get() {
                None => view! { <p class="g-hint">"Reading your Spaces."</p> }.into_any(),
                Some(Err(e)) => view! { <p class="g-hint">{e}</p> }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! { <p class="g-hint">"You have no Spaces yet."</p> }.into_any(),
                Some(Ok(list)) => view! {
                    <ul class="eco-mine-list">
                        {list.into_iter().map(|p| {
                            let id = p.id.clone();
                            let id2 = p.id.clone();
                            view! {
                                <li><label>
                                    <input type="checkbox" prop:checked=move || picked.with(|s| s.contains(&id))
                                        on:change=move |_| picked.update(|s| { if !s.remove(&id2) { s.insert(id2.clone()); } }) />
                                    <span>{p.name.clone()}</span>
                                    <span class="g-hint">{format!("{} visits", commas(p.play_count as i64))}</span>
                                </label></li>
                            }
                        }).collect_view()}
                    </ul>
                    <button type="button" class="g-btn g-btn-sm" on:click=apply>"Use these Spaces"</button>
                }.into_any(),
            }}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 4. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::economics_model::{Key, CONTROLS};
    use super::*;
    use leptos::tachys::view::RenderHtml;

    /// The controls a reader who is not an administrator sees.
    fn public_controls() -> impl Iterator<Item = &'static Control> {
        CONTROLS.iter().filter(|c| c.group != Group::Admin)
    }

    fn body_html() -> String {
        let owner = Owner::new();
        owner.with(|| EconomicsBody().to_html())
    }

    /// The words a reader sees in some markup, with the tags removed.
    fn visible(html: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for c in html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => {
                    in_tag = false;
                    out.push(' ');
                }
                c if !in_tag => out.push(c),
                _ => {}
            }
        }
        out
    }

    /// Every string of copy the page owns.
    fn all_copy() -> Vec<String> {
        let mut v: Vec<String> = vec![LEAD.into(), CURRENCIES.into(), NOTICE.into(), STEPS_INTRO.into(), MONTH_INTRO.into(), CREATOR_INTRO.into(), TICKETS_NOTE.into(), THEORETICAL_INTRO.into(), ACTUAL_INTRO.into()];
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
        for c in public_controls() {
            v.push(c.label.into());
            v.push(c.help.into());
        }
        v
    }

    #[test]
    fn the_page_says_at_the_top_that_it_is_a_model_for_testing() {
        let html = body_html();
        let h1 = html.find("<h1").expect("a heading");
        let banner = html.find("Model for testing.").expect("the notice");
        let first_section = html.find("The short version").expect("the first section");
        assert!(h1 < banner && banner < first_section, "the notice sits between the heading and the content");
        assert!(NOTICE.contains("not a price and not a forecast of what anyone will be paid") && NOTICE.contains("nothing you move leaves your browser"));
        assert!(html.contains("How money moves through Eustress"));
    }

    #[test]
    fn five_steps_in_the_order_the_money_moves_each_a_rule_that_is_built() {
        assert_eq!(STEPS.len(), 5);
        let titles: Vec<&str> = STEPS.iter().map(|s| s.title).collect();
        assert!(titles[0].contains("buyer pays") && titles[1].contains("split in half") && titles[2].contains("Treasury pays") && titles[3].contains("verified") && titles[4].contains("earns Tickets"));
        assert!(STEPS_INTRO.contains("each a rule that is built today"));
        let html = body_html();
        assert_eq!(html.matches("class=\"g-glass eco-step\"").count(), 5, "five step cards");
        assert!(!html.contains("g-chip live") && !html.contains("g-chip warn"), "no status chips: nothing here is a proposal");
    }

    #[test]
    fn the_steps_state_the_rules_as_the_worker_applies_them() {
        // the Treasury's rate is not a flat rate, and Bliss is not paid to everyone
        assert!(STEPS[2].body.contains("0.276%") && STEPS[2].body.contains("0.136%") && STEPS[2].body.contains("15%"), "{}", STEPS[2].body);
        assert!(!STEPS[2].body.contains("fixed"), "the drip is not a fixed rate");
        assert!(STEPS[3].body.contains("verify their identity (18 or older)") && STEPS[3].body.contains("Stripe") && STEPS[3].body.contains("Creator, Professional or Bundle plan"));
        assert!(STEPS[3].body.contains("stays in the Treasury") && STEPS[3].body.contains("$0.50"));
        // only web payments exist today: no app-store fee is claimed as a rule
        assert!(STEPS[0].body.contains("every web payment") && !STEPS[0].body.to_lowercase().contains("app store"));
        // the creator's share is stated as today's rule and not final; Tickets do not convert
        assert!(STEPS[4].body.contains("70%") && STEPS[4].body.contains("30%") && STEPS[4].body.contains("not final"));
        assert!(STEPS[4].body.contains("cannot be converted to Bliss or dollars") && STEPS[4].body.contains("No storefront sells for Tickets yet"));
    }

    #[test]
    fn the_copy_makes_no_promise_the_site_does_not_make_and_holds_no_proposal_or_operating_number() {
        // The live Currency page says Delta is "never cashed out"; this page says the same of Delta and says nothing else with those words.
        let allowed = "is never cashed out";
        assert_eq!(all_copy().iter().filter(|t| t.contains(allowed)).count(), 1, "one sentence uses the allowed words: the one about Delta");
        assert!(CURRENCIES.contains(&format!("Delta never converts into either and {allowed}")));
        for text in all_copy().iter().chain(std::iter::once(&visible(&body_html()))) {
            let lower = text.to_lowercase().replace(allowed, "");
            for banned in [
                // house rules
                "worth", "backed by", "guarantee", "game engine", "risk-free", "passive income", "get rich", "mercury", "\u{2014}", "\u{2013}",
                // earnings claims
                "typical", "a year", "per year", "yearly", "you will earn", "you can earn", "you could earn",
                // proposals and operating numbers: not for the public page
                "reserve", "cash-out", "cash out", "cashed", "invest", "repay", "profit", "expense", "tax", "own account", "return on",
            ] {
                assert!(!lower.contains(banned), "the page says {banned:?}: {}", text.chars().take(160).collect::<String>());
            }
        }
        // Tickets are never said to be cash
        let t = TICKETS_NOTE.to_lowercase();
        assert!(t.contains("never in dollars") && t.contains("cannot be converted to bliss or dollars"));
    }

    #[test]
    fn every_internal_link_goes_to_a_route_the_app_has() {
        let routes = include_str!("../app.rs");
        let mut links: Vec<&str> = STEPS.iter().filter_map(|s| s.more.map(|m| m.0)).collect();
        links.extend(SOURCES.iter().map(|s| s.0));
        assert!(links.len() >= 9);
        for link in links {
            let path = link.split('#').next().unwrap();
            assert!(routes.contains(&format!("path!(\"{path}\")")), "app.rs has no route for {link}");
        }
        // an anchor on another page goes to an id that page has
        let earning = include_str!("docs_earning.rs");
        for id in ["payouts-drip", "tickets-sales"] {
            assert!(earning.contains(&format!("id=\"{id}\"")), "the Earning page has no #{id}");
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
        assert!(app.contains("path!(\"/economics\")") && app.contains("EconomicsPage"), "the route is wired");
        // Not listed until McKale lists it: no nav link, no footer link, no sitemap entry, no llms.txt line.
        let nav = include_str!("../components/nav.rs");
        let footer = include_str!("../components/footer.rs");
        let sitemap = include_str!("../../sitemap.xml");
        let llms = include_str!("../../llms.txt");
        for (name, text) in [("nav.rs", nav), ("footer.rs", footer), ("sitemap.xml", sitemap), ("llms.txt", llms)] {
            assert!(!text.contains("/economics"), "{name} links the Economics page; update this test and the note at the top of economics.rs when it is listed");
        }
    }

    #[test]
    fn the_hosts_headers_keep_the_page_out_of_search() {
        let headers = include_str!("../../_headers");
        for path in ["/economics\n", "/economics-draft\n"] {
            let at = headers.find(&format!("\n{path}")).unwrap_or_else(|| panic!("a rule for {path}")) + 1;
            let rule = &headers[at..];
            assert!(rule.lines().take(3).any(|l| l.trim().eq_ignore_ascii_case("X-Robots-Tag: noindex, nofollow")), "{path}: {rule}");
        }
    }

    #[test]
    fn the_body_has_every_slider_and_every_choice_and_the_default_figures() {
        let html = body_html();
        assert_eq!(html.matches("type=\"range\"").count(), public_controls().count(), "one range input for every control a reader sees");
        assert_eq!(public_controls().count(), 16);
        assert_eq!(html.matches("<select").count(), 3, "three choices");
        for p in [Pick::Pack, Pick::Node, Pick::Scale] {
            assert!(html.contains(p.label()), "{}", p.label());
        }
        assert_eq!(html.matches("<svg").count(), 3, "the month, the sale and the payout");
        assert!(!html.contains("<script"));
        for want in ["$178,992", "$168,751", "$84,375", "$84,376", "$10,241", "49,500 Tickets", "34,650 Tickets", "14,850 Tickets", "$451.92", "$161.13", "$290.79", "0.5%"] {
            assert!(html.contains(want), "the default page shows {want}");
        }
        // the statement tables read the month in order
        let a = html.find("Gross receipts").unwrap();
        let b = html.find("Eustress&#x27;s half").or_else(|| html.find("Eustress's half")).unwrap();
        assert!(a < b);
    }

    #[test]
    fn each_slider_carries_its_range_and_its_value_in_words() {
        let html = body_html();
        for c in public_controls() {
            let id = format!("id=\"eco-{:?}\"", c.key).to_lowercase();
            assert!(html.contains(&id), "{} has an id", c.label);
            assert!(html.contains(c.label), "{} is named", c.label);
            // each range input says its own limits: its tag carries this control's min, max and step and no other's
            let at = html.find(&id).expect("the slider exists");
            let start = html[..at].rfind("<input").expect("an input tag");
            let end = at + html[at..].find('>').expect("the tag closes");
            let tag = &html[start..end];
            assert!(tag.contains("type=\"range\""), "{} is a range: {tag}", c.label);
            let (min, max, step) = c.input_range();
            for (name, v) in [("min", min), ("max", max), ("step", step)] {
                assert!(tag.contains(&format!("{name}=\"{v}\"")), "{} has {name}=\"{v}\": {tag}", c.label);
            }
        }
        // the Players slider moves on positions: 0 to 1,000 on a log scale
        let at = html.find("id=\"eco-players\"").unwrap();
        let tag = &html[html[..at].rfind("<input").unwrap()..at + html[at..].find('>').unwrap()];
        assert!(tag.contains("max=\"1000\"") && tag.contains(&format!("value=\"{}\"", control(Key::Players).position(5000.0))), "{tag}");
        assert!(html.contains("$100,000") && html.contains("60 h") && html.contains("5,000"), "the values are in words");
        let _ = Key::Tickets;
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
    fn the_first_assumption_states_the_models_month_from_the_constant() {
        assert!(ASSUMPTIONS[0].starts_with(&format!("A month is {} days", super::super::economics_model::DAYS as i64)), "{}", ASSUMPTIONS[0]);
        assert!(ASSUMPTIONS[4].contains(&format!("${}", super::super::economics_model::DELTA_ORDER_USD as i64)), "{}", ASSUMPTIONS[4]);
    }

    #[test]
    fn the_assumptions_name_the_caps_the_worker_applies_to_sales_score() {
        use super::super::economics_model::{commas, VALUE_CAP_PER_DAY, VALUE_CAP_TICKETS_PER_BUYER, VALUE_PER_TICKET};
        let a = ASSUMPTIONS[2];
        assert!(a.contains(&format!("{VALUE_PER_TICKET} score for each Ticket")), "{a}");
        assert!(a.contains(&format!("up to {} of those Tickets in a day", commas(VALUE_CAP_TICKETS_PER_BUYER as i64))), "{a}");
        assert!(a.contains(&format!("stops at {} a day", commas(VALUE_CAP_PER_DAY as i64))), "{a}");
        assert!(a.contains("spend the average in one day"), "{a}");
    }

    #[cfg(not(feature = "economics-draft"))]
    #[test]
    fn without_the_draft_feature_its_address_answers_like_any_address_the_site_does_not_have() {
        let owner = Owner::new();
        let html = owner.with(|| EconomicsDraftPage().to_html());
        let any_address = owner.with(|| crate::pages::NotFoundPage().to_html());
        assert_eq!(html, any_address);
        // and the router's own fallback is that page
        assert!(include_str!("../app.rs").contains("fallback=NotFoundPage"));
    }

    #[test]
    fn every_module_of_the_draft_is_compiled_only_when_the_feature_is_on() {
        let modules = include_str!("mod.rs").replace("
", "
");
        let lines: Vec<&str> = modules.lines().collect();
        let draft: Vec<usize> = (0..lines.len()).filter(|i| lines[*i].contains("economics_draft") || lines[*i].contains("EconomicsDraftPage")).collect();
        assert_eq!(draft.len(), 5, "three modules, the re-export and the stub: {draft:?}");
        for i in draft {
            if lines[i].starts_with("pub use economics::EconomicsDraftPage") {
                assert_eq!(lines[i - 1], "#[cfg(not(feature = \"economics-draft\"))]", "the stub is for builds without the draft");
            } else {
                assert!(i > 0 && lines[i - 1] == "#[cfg(feature = \"economics-draft\")]", "line {} is not behind the feature: {}", i + 1, lines[i]);
            }
        }
        // the public modules do not use the draft: no line of code in them names its modules (a comment may say where the rest lives)
        for (name, text) in [("economics_chart.rs", include_str!("economics_chart.rs")), ("economics_model.rs", include_str!("economics_model.rs")), ("economics_kit.rs", include_str!("economics_kit.rs"))] {
            for (n, line) in text.lines().enumerate() {
                assert!(line.trim_start().starts_with("//") || !line.contains("economics_draft"), "{name}:{} uses the draft: {line}", n + 1);
            }
        }
    }

    #[test]
    fn the_markup_starts_on_the_example_month_before_any_script_runs() {
        use super::super::economics_model::Pick;
        let html = body_html();
        // every range input carries its starting value as an attribute: the example month's
        let d = Inputs::default();
        for c in public_controls() {
            let at = html.find(&format!("id=\"eco-{:?}\"", c.key).to_lowercase()).expect("the slider exists");
            let start = html[..at].rfind("<input").expect("an input tag");
            let end = at + html[at..].find('>').expect("the tag closes");
            let tag = &html[start..end];
            assert!(tag.contains(&format!("value=\"{}\"", c.position(d.get(c.key)))), "{} starts at {}: {tag}", c.label, d.get(c.key));
        }
        // and each list has its example option selected, and only that one
        for pick in [Pick::Pack, Pick::Node, Pick::Scale] {
            let want = d.picked(pick);
            let mut selected = Vec::new();
            for (value, _) in pick.options() {
                let at = html.find(&format!("<option value=\"{value}\"")).unwrap_or_else(|| panic!("an option {value}"));
                let tag = &html[at..at + html[at..].find('>').unwrap()];
                if tag.contains("selected") {
                    selected.push(value);
                }
            }
            assert_eq!(selected, [want], "{pick:?}");
        }
    }

    #[test]
    fn the_page_offers_three_creators_to_start_from_and_a_way_back_to_the_example_month() {
        let html = body_html();
        assert_eq!(html.matches("class=\"g-chip g-chip-btn\"").count(), PRESETS.len(), "a button for each preset, and My projects beside them");
        for p in PRESETS.iter() {
            assert!(html.contains(p.name), "a button for {}", p.name);
        }
        assert_eq!(PRESETS.iter().map(|p| p.name).collect::<Vec<_>>(), ["Small", "Medium", "Large"]);
        assert_eq!(html.matches("Reset all sliders").count(), 2, "a reset in each part of the page");
        assert!(!html.contains("Typical") && !html.contains(">Hit<"), "the sizes carry no earnings promise");
    }

    #[test]
    fn the_page_opens_in_theoretical_mode_and_offers_actual() {
        let html = body_html();
        let tabs = html.find("role=\"tablist\"").expect("the modes");
        let theory = html[tabs..].find("Theoretical").unwrap();
        let actual = html[tabs..].find(">Actual<").expect("an Actual tab");
        assert!(theory < actual);
        assert!(html.contains("aria-selected=\"true\"") && html.contains("aria-selected=\"false\""));
        assert!(html.contains(THEORETICAL_INTRO));
        // Actual mode reads the platform in the browser; the first render draws none of it
        assert!(!html.contains("id=\"eco-actual\""));
        assert!(ACTUAL_INTRO.contains("every minute") && ACTUAL_INTRO.contains("administrator only"));
    }

    #[test]
    fn a_reader_the_worker_has_not_called_an_administrator_sees_no_costs_and_no_proposals() {
        let html = body_html();
        for word in ["Labor", "Simbuilder", "Where Eustress", "Cost ledger", "Proposed", "eco-costs", "HOSTING_COST_PLAN"] {
            assert!(!html.contains(word), "the page shows {word}");
        }
        for c in CONTROLS.iter().filter(|c| c.group == Group::Admin) {
            assert!(!html.contains(&format!("id=\"eco-{:?}\"", c.key).to_lowercase()), "{} is drawn", c.label);
        }
        // the admin section is behind the Worker's answer, not a flag in the page
        let src = include_str!("economics.rs");
        assert!(src.contains("<Show when=move || admin.get()>"));
        assert!(src.contains("live::load_admin(&live::PeriodPick::default())"));
    }

    #[test]
    fn a_creator_can_start_from_their_own_spaces() {
        let html = body_html();
        assert!(html.contains(">My projects<"), "a button beside the presets");
        assert!(!html.contains("Use these Spaces"), "the picker opens only when asked");
    }

    #[test]
    fn a_bliss_figure_is_never_shown_without_saying_who_is_paid_it() {
        let html = body_html();
        assert!(html.contains("paid to a verified contributor on a paid plan") && html.contains("verified their identity (18 or older)"));
    }
}
