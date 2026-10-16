// =============================================================================
// Eustress Web - /cases/:reference: the public status of one case
// =============================================================================
// Anyone with a case reference can see where the case stands. The read is GET /api/cases/{reference} with no session: it is sent
// without the visitor's token, so a stale sign-in cannot turn a public read into a 401. A reference that is not the right shape is
// turned away before anything is asked of the Worker (takedown_wire::is_valid_reference).
//
// Table of Contents:
// 1. The read
// 2. The page
// 3. Tests
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use super::take_it_down::CopyReference;
use super::takedown_wire::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// 1. The read
// -----------------------------------------------------------------------------

/// Ask the Worker about a reference that has already passed the shape check. No Authorization header, no cookies.
async fn fetch_case(reference: &str) -> CaseView {
    let url = format!("{}{}", crate::state::api_base(), case_path(reference));
    match gloo_net::http::Request::get(&url).header("Accept", "application/json").send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            case_view_from(status, &body)
        }
        Err(_) => CaseView::Failed("Could not reach Eustress. Check your connection and try again.".to_string()),
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[component]
pub fn CaseStatusPage() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    let reference = Memo::new(move |_| params.read().get("reference").unwrap_or_default());
    let state = RwSignal::new(CaseView::Loading);

    Effect::new(move |_| {
        let wanted = normalize_reference(&reference.get());
        if !is_valid_reference(&wanted) {
            state.set(CaseView::Malformed);
            return;
        }
        state.set(CaseView::Loading);
        spawn_local(async move {
            let result = fetch_case(&wanted).await;
            // A visitor who moved on to another reference while this was out keeps the newer page.
            if normalize_reference(&reference.get_untracked()) == wanted {
                state.set(result);
            }
        });
    });

    view! {
        <div class="page page-legal page-takedown">
            <CentralNav active="".to_string() />
            <main class="legal-container tid-container">
                <div class="legal-header">
                    <h1>"Case status"</h1>
                    <p class="legal-updated">"Check where a case stands with its reference."</p>
                </div>
                <div class="legal-content">
                    {move || match state.get() {
                        CaseView::Loading => view! {
                            <section class="legal-section" role="status"><p>"Looking up the case..."</p></section>
                        }.into_any(),
                        CaseView::Malformed => view! {
                            <section class="legal-section" role="alert">
                                <h2>"That reference is not the right shape"</h2>
                                <p>"A reference starts with EC and has five groups of four letters and numbers, like EC-XXXX-XXXX-XXXX-XXXX-XXXX. It never uses the letters I, L, O or U."</p>
                                <p>"Check it against your confirmation, then enter it again."</p>
                            </section>
                            <LookupForm />
                        }.into_any(),
                        CaseView::NotFound => view! {
                            <section class="legal-section" role="alert">
                                <h2>"We could not find that case"</h2>
                                <p>"The reference is the right shape, but no case has it. Check each character, then enter it again."</p>
                            </section>
                            <LookupForm />
                        }.into_any(),
                        CaseView::Failed(why) => view! {
                            <section class="legal-section" role="alert">
                                <h2>"We could not check the case"</h2>
                                <p>{why}</p>
                            </section>
                        }.into_any(),
                        CaseView::Found(case) => view! { <CaseFound case=case /> }.into_any(),
                    }}
                </div>
            </main>
            <Footer />
        </div>
    }
}

/// A case that was found: its reference, its status in words, and its dates.
#[component]
fn CaseFound(case: CaseStatus) -> impl IntoView {
    let (title, sentence) = status_words(&case.status);
    let dates: Vec<(&'static str, String)> = [
        ("Received", case.received_at.as_deref()),
        ("Removed", case.removed_at.as_deref()),
        ("Decided", case.decided_at.as_deref()),
    ]
    .into_iter()
    .filter_map(|(label, raw)| raw.map(|r| (label, stamp_words(r))))
    .collect();
    view! {
        <section class="legal-section tid-case">
            <p class="tid-label">{kind_words(&case.kind)}</p>
            <CopyReference reference=case.reference.clone() id="tid-case-ref" />
            <h2 class="tid-status">{title}</h2>
            <p>{sentence}</p>
            <dl class="tid-dates">
                {dates.into_iter().map(|(label, when)| view! { <dt>{label}</dt><dd>{when}</dd> }).collect_view()}
            </dl>
            <p><a href="/take-it-down">"About removal requests"</a></p>
        </section>
    }
}

/// A box to enter another reference.
#[component]
fn LookupForm() -> impl IntoView {
    let typed = RwSignal::new(String::new());
    let problem = RwSignal::new(Option::<String>::None);
    let navigate = leptos_router::hooks::use_navigate();
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let wanted = normalize_reference(&typed.get_untracked());
        if is_valid_reference(&wanted) {
            problem.set(None);
            navigate(&format!("/cases/{wanted}"), Default::default());
        } else {
            problem.set(Some("That is not the right shape. It starts with EC and has five groups of four.".to_string()));
            super::take_it_down::focus_id("tid-lookup");
        }
    };
    view! {
        <form class="tid-form tid-lookup" novalidate on:submit=on_submit>
            <div class="tid-field">
                <label for="tid-lookup">"Case reference"</label>
                <input id="tid-lookup" type="text" autocomplete="off" spellcheck="false"
                    aria-invalid=move || if problem.get().is_some() { "true" } else { "false" }
                    aria-describedby=move || problem.get().map(|_| "tid-lookup-err")
                    prop:value=move || typed.get()
                    on:input=move |ev: leptos::ev::Event| typed.set(event_target_value(&ev)) />
                {move || problem.get().map(|m| view! { <p class="tid-error" id="tid-lookup-err">{m}</p> })}
            </div>
            <button type="submit" class="tid-btn tid-btn-primary">"Check status"</button>
        </form>
    }
}

// -----------------------------------------------------------------------------
// 3. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    fn html<V: RenderHtml>(build: impl FnOnce() -> V) -> String {
        let owner = Owner::new();
        owner.with(|| build().to_html())
    }

    fn found(status: &str) -> CaseStatus {
        CaseStatus {
            reference: "EC-7K2M-9QXD-4TPF-HJ3W-5XRZ".to_string(),
            kind: "takedown_request".to_string(),
            status: status.to_string(),
            received_at: Some("2026-10-08T19:00:00Z".to_string()),
            removed_at: Some("2026-10-08T19:00:01Z".to_string()),
            decided_at: None,
        }
    }

    #[test]
    fn a_found_case_shows_its_reference_status_and_dates_in_words() {
        let page = html(|| view! { <CaseFound case=found("removed") /> });
        assert!(page.contains("value=\"EC-7K2M-9QXD-4TPF-HJ3W-5XRZ\""), "{page}");
        assert!(page.contains("Removed") && page.contains("has been removed"), "{page}");
        assert!(page.contains("Oct 8, 2026, 7:00 PM UTC"), "{page}");
        assert!(page.contains("Removal request"));
        assert!(!page.contains("Decided"), "a date the case does not have is not shown");
    }

    #[test]
    fn the_status_page_is_public_and_checks_the_shape_before_asking() {
        let src = include_str!("case_status.rs");
        // the public read does not go through ApiClient, which would attach a sign-in token
        let read = &src[src.find("async fn fetch_case").unwrap()..src.find("pub fn CaseStatusPage").unwrap()];
        assert!(!read.contains("ApiClient") && !read.contains("Authorization"), "no session on a public read");
        let page = &src[src.find("pub fn CaseStatusPage").unwrap()..src.find("fn CaseFound").unwrap()];
        assert!(page.find("is_valid_reference").unwrap() < page.find("fetch_case").unwrap(), "the shape is checked first");
        assert!(page.contains("normalize_reference"), "capitals before the check");
    }

    #[test]
    fn the_malformed_and_not_found_pages_offer_another_try() {
        // LookupForm needs the router to navigate, so it is read from the source here and exercised in the browser
        let src = include_str!("case_status.rs");
        assert!(src.contains("<label for=\"tid-lookup\">") && src.contains("id=\"tid-lookup\"") && src.contains("Check status"));
        assert_eq!(src.matches(concat!("<Lookup", "Form />")).count(), 2, "both the malformed and the not found views carry it");
    }
}
