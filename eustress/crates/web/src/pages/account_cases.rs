// =============================================================================
// Eustress Web - /account/cases: the signed-in account's cases
// =============================================================================
// Reviews of the account's listings, removals of its content, and the removal requests it filed, newest first. The Worker's emails
// link here. Reads GET /api/account/cases (a session; the answer is private and not cached), twenty at a time with "Load more".
// A case can carry plain-words feedback, a link to the review panel for an appeal, or a form to ask for a review
// (POST /api/account/cases/{reference}/review).
//
// Wire names and parsing are in takedown_wire.rs.
//
// Table of Contents:
// 1. Reading a page
// 2. The page
// 3. The list
// 4. One case
// 5. Asking for a review
// 6. Tests
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use super::take_it_down::{focus_id, CopyReference};
use super::takedown_wire::*;
use crate::api::{failure_sentence, ApiClient, ApiError};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

// -----------------------------------------------------------------------------
// 1. Reading a page
// -----------------------------------------------------------------------------

/// What reading a page of cases came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    Page(CasesPage),
    SignedOut,
    Failed(String),
}

pub fn classify(answer: Result<CasesPage, ApiError>) -> Load {
    match answer {
        Ok(page) => Load::Page(page),
        Err(ApiError::Unauthorized) => Load::SignedOut,
        Err(ApiError::NotFound) => Load::Failed("Cases are not open on this site yet. Try again later.".to_string()),
        Err(e) => Load::Failed(failure_sentence(&e)),
    }
}

async fn fetch_page(cursor: Option<String>) -> Load {
    let client = ApiClient::new(&crate::state::api_base());
    classify(client.get::<CasesPage>(&page_path(PAGE_SIZE, cursor.as_deref())).await)
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    Unknown,
    SignedOut,
    SignedIn,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Waiting,
    SignedOut,
    Loading,
    Ready,
    Failed(String),
}

#[component]
pub fn AccountCasesPage() -> impl IntoView {
    let auth = expect_context::<AppState>().auth;
    // A Memo, so the app's periodic session refresh (which sets the account again with the same facts) does not reload the list.
    let session = Memo::new(move |_| match auth.get() {
        AuthState::Authenticated(_) => Session::SignedIn,
        AuthState::Unauthenticated => Session::SignedOut,
        AuthState::Unknown => Session::Unknown,
    });
    let cases = RwSignal::new(Vec::<CaseEntry>::new());
    let total = RwSignal::new(0u64);
    let next = RwSignal::new(Option::<String>::None);
    let phase = RwSignal::new(Phase::Waiting);
    let selected = RwSignal::new(Option::<String>::None);
    let asked = RwSignal::new(Vec::<(String, String)>::new());

    Effect::new(move |_| match session.get() {
        Session::Unknown => phase.set(Phase::Waiting),
        Session::SignedOut => phase.set(Phase::SignedOut),
        Session::SignedIn => {
            phase.set(Phase::Loading);
            spawn_local(async move {
                match fetch_page(None).await {
                    Load::Page(page) => {
                        total.set(page.total);
                        let mut have = Vec::new();
                        let cursor = merge_page(&mut have, page, None);
                        cases.set(have);
                        next.set(cursor);
                        phase.set(Phase::Ready);
                    }
                    Load::SignedOut => phase.set(Phase::SignedOut),
                    Load::Failed(why) => phase.set(Phase::Failed(why)),
                }
            });
        }
    });

    view! {
        <div class="page page-legal page-takedown">
            <CentralNav active="".to_string() />
            <main class="legal-container tid-container">
                <div class="legal-header">
                    <h1>"Your cases"</h1>
                    <p class="legal-updated">"Reviews of your listings, removals of your content, and removal requests you sent."</p>
                </div>
                <div class="legal-content">
                    {move || match phase.get() {
                        Phase::Waiting | Phase::Loading => view! {
                            <section class="legal-section" role="status"><p>"Loading your cases..."</p></section>
                        }.into_any(),
                        Phase::SignedOut => view! { <SignInNotice /> }.into_any(),
                        Phase::Failed(why) => view! {
                            <section class="legal-section" role="alert">
                                <h2>"We could not load your cases"</h2>
                                <p>{why}</p>
                            </section>
                        }.into_any(),
                        Phase::Ready => match selected.get() {
                            Some(reference) => view! { <CaseDetail cases=cases reference=reference selected=selected asked=asked /> }.into_any(),
                            None => view! { <CaseList cases=cases total=total next=next selected=selected /> }.into_any(),
                        },
                    }}
                </div>
            </main>
            <Footer />
        </div>
    }
}

/// Signed out: the way in is to sign in.
#[component]
pub fn SignInNotice() -> impl IntoView {
    view! {
        <section class="legal-section">
            <h2>"Sign in to see your cases"</h2>
            <p>"Your cases belong to your account, so they show only when you are signed in."</p>
            <p><a class="tid-btn tid-btn-primary" href="/login">"Sign in"</a></p>
            <p>"To check a removal request without an account, use the reference you were given."</p>
        </section>
    }
}

// -----------------------------------------------------------------------------
// 3. The list
// -----------------------------------------------------------------------------

/// A row in words: the headline, the status, and when it last changed.
pub fn row_parts(c: &CaseEntry) -> (String, String, String) {
    let headline = if c.headline.trim().is_empty() { kind_words(&c.kind).to_string() } else { c.headline.trim().to_string() };
    let when = c.updated_at.as_deref().or(c.opened_at.as_deref()).map(stamp_words).unwrap_or_default();
    (headline, humanize(&c.status), when)
}

#[component]
fn CaseList(
    cases: RwSignal<Vec<CaseEntry>>,
    total: RwSignal<u64>,
    next: RwSignal<Option<String>>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let more_busy = RwSignal::new(false);
    let more_error = RwSignal::new(Option::<String>::None);
    let load_more = move |_| {
        if more_busy.get_untracked() {
            return;
        }
        let Some(cursor) = next.get_untracked() else { return };
        more_busy.set(true);
        more_error.set(None);
        spawn_local(async move {
            match fetch_page(Some(cursor.clone())).await {
                Load::Page(page) => {
                    total.set(page.total);
                    let mut have = cases.get_untracked();
                    let following = merge_page(&mut have, page, Some(&cursor));
                    cases.set(have);
                    next.set(following);
                }
                Load::SignedOut => more_error.set(Some("Your sign-in has ended. Sign in again, then load more.".to_string())),
                Load::Failed(why) => more_error.set(Some(why)),
            }
            more_busy.set(false);
        });
    };
    view! {
        <section class="legal-section">
            {move || if cases.with(|c| c.is_empty()) {
                view! { <p>"You have no cases. When one of your listings is reviewed, or you send a removal request, it shows here."</p> }.into_any()
            } else {
                view! {
                    <p class="tid-count" aria-live="polite">
                        {move || {
                            let shown = cases.with(|c| c.len()) as u64;
                            format!("Showing {} of {} cases, newest first.", shown, total.get().max(shown))
                        }}
                    </p>
                    <ul class="tid-cases">
                        {move || cases.get().iter().map(|c| {
                            let (headline, status, when) = row_parts(c);
                            let reference = c.reference.clone();
                            let open = c.reference.clone();
                            view! {
                                <li>
                                    <button type="button" class="tid-case-row" on:click=move |_| selected.set(Some(open.clone()))>
                                        <span class="tid-case-head">{headline}</span>
                                        <span class="tid-case-meta">
                                            <span class="tid-case-ref">{reference}</span>
                                            <span class="tid-badge">{status}</span>
                                            <span class="tid-case-when">{when}</span>
                                        </span>
                                    </button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any()
            }}
            {move || next.get().map(|_| view! {
                <button type="button" class="tid-btn" disabled=move || more_busy.get() on:click=load_more>
                    {move || if more_busy.get() { "Loading..." } else { "Load more" }}
                </button>
            })}
            {move || more_error.get().map(|m| view! { <p class="tid-error" role="alert">{m}</p> })}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 4. One case
// -----------------------------------------------------------------------------

/// "Articles cited: B6, B7", or nothing for an empty list.
pub fn list_line(label: &str, items: &[String]) -> Option<String> {
    let kept: Vec<&str> = items.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if kept.is_empty() {
        None
    } else {
        Some(format!("{label}: {}", kept.join(", ")))
    }
}

/// True when the feedback has anything to show.
pub fn feedback_has_content(fb: &Feedback) -> bool {
    !fb.reasons.is_empty()
        || fb.articles.iter().any(|s| !s.trim().is_empty())
        || fb.tests.iter().any(|s| !s.trim().is_empty())
        || fb.suggested_edit.as_deref().is_some_and(|s| !s.trim().is_empty())
        || fb.note.as_deref().is_some_and(|s| !s.trim().is_empty())
        || crate::tags::tags_chosen_line(fb.tags_chosen.as_deref()).is_some()
}

fn feedback_view(fb: Feedback) -> impl IntoView {
    let reasons = fb
        .reasons
        .iter()
        .map(|r| {
            let why = (!r.why.trim().is_empty()).then(|| view! { <p><strong>"Why: "</strong>{r.why.clone()}</p> });
            let change = (!r.what_to_change.trim().is_empty()).then(|| view! { <p><strong>"What to change: "</strong>{r.what_to_change.clone()}</p> });
            view! { <li><strong>{r.title.clone()}</strong>{why}{change}</li> }
        })
        .collect_view();
    let has_reasons = !fb.reasons.is_empty();
    let articles = list_line("Articles cited", &fb.articles);
    let tests = list_line("Tests applied", &fb.tests);
    let edit = fb.suggested_edit.clone().filter(|s| !s.trim().is_empty());
    let note = fb.note.clone().filter(|s| !s.trim().is_empty());
    let tags_line = crate::tags::tags_chosen_line(fb.tags_chosen.as_deref());
    view! {
        <div class="tid-feedback">
            <h3>"What we found"</h3>
            {has_reasons.then(|| view! { <ol class="tid-reasons">{reasons}</ol> })}
            {articles.map(|t| view! { <p>{t}</p> })}
            {tests.map(|t| view! { <p>{t}</p> })}
            {edit.map(|t| view! { <p><strong>"Suggested change: "</strong>{t}</p> })}
            {note.map(|t| view! { <p><strong>"Note: "</strong>{t}</p> })}
            {tags_line.map(|t| view! { <p>{t}</p> })}
        </div>
    }
}

fn dated(label: &'static str, raw: &Option<String>) -> Option<(&'static str, String)> {
    raw.as_deref().map(|r| (label, stamp_words(r)))
}

#[component]
fn CaseDetail(
    cases: RwSignal<Vec<CaseEntry>>,
    reference: String,
    selected: RwSignal<Option<String>>,
    asked: RwSignal<Vec<(String, String)>>,
) -> impl IntoView {
    let wanted = reference.clone();
    let entry = Memo::new(move |_| cases.with(|c| c.iter().find(|x| x.reference == wanted).cloned()));
    view! {
        <section class="legal-section tid-detail">
            <button type="button" class="tid-btn" on:click=move |_| selected.set(None)>"Back to all cases"</button>
            {move || match entry.get() {
                None => view! { <p>"That case is not in your list."</p> }.into_any(),
                Some(c) => view! { <CaseBody case=c asked=asked /> }.into_any(),
            }}
        </section>
    }
}

/// One case in full: what it is, where it stands, what was found, and what can be done next.
#[component]
pub fn CaseBody(case: CaseEntry, asked: RwSignal<Vec<(String, String)>>) -> impl IntoView {
    let (headline, status, _) = row_parts(&case);
    let reference = case.reference.clone();
    let status_now = {
        let reference = reference.clone();
        // After a review request is sent, the Worker's new status stands in for the old one without reloading the list.
        move || asked.with(|a| a.iter().find(|(r, _)| *r == reference).map(|(_, s)| humanize(s)).filter(|s| !s.is_empty())).unwrap_or_else(|| status.clone())
    };
    let outcome = case.outcome.clone().filter(|o| !o.trim().is_empty()).map(|o| outcome_words(&o));
    let listing_name = case.listing.as_ref().map(|l| l.name.clone()).filter(|n| !n.trim().is_empty());
    let dates: Vec<(&'static str, String)> = [
        dated("Opened", &case.opened_at),
        dated("Removed", &case.removed_at),
        dated("Decided", &case.decided_at),
        dated("Last updated", &case.updated_at),
    ]
    .into_iter()
    .flatten()
    .collect();
    let locations = case
        .locations
        .iter()
        .map(|l| view! { <li><span class="tid-loc">{l.location.clone()}</span><span class="tid-loc-result">{result_words(&l.result)}</span></li> })
        .collect_view();
    let has_locations = !case.locations.is_empty();
    let feedback = case.feedback.clone().filter(feedback_has_content);

    // A listing review can be appealed in the review panel that already exists; this page links to it.
    let appeal = if case.kind == "listing_review" {
        let id = case.listing.as_ref().map(|l| l.id.clone()).filter(|i| !i.is_empty());
        match (case.can_appeal, id) {
            (true, Some(id)) => Some(view! {
                <p><a class="tid-btn tid-btn-primary" href=format!("/projects?review={id}")>"Open the review and appeal"</a></p>
            }.into_any()),
            _ if case.appeal.is_some() => Some(view! { <p>"An appeal is on file for this case."</p> }.into_any()),
            _ => None,
        }
    } else {
        None
    };
    let review = case.can_request_review.then(|| view! { <ReviewForm reference=reference.clone() asked=asked /> });
    view! {
        <div class="tid-case-body">
            <h2>{headline}</h2>
            <p class="tid-label">{kind_words(&case.kind)}</p>
            <CopyReference reference=case.reference.clone() id="tid-detail-ref" />
            <p><strong>"Status: "</strong>{status_now}</p>
            {outcome.map(|o| view! { <p>{o}</p> })}
            {listing_name.map(|n| view! { <p><strong>"Listing: "</strong>{n}</p> })}
            <dl class="tid-dates">
                {dates.into_iter().map(|(label, when)| view! { <dt>{label}</dt><dd>{when}</dd> }).collect_view()}
            </dl>
            {has_locations.then(|| view! {
                <h3>"What happened to each item"</h3>
                <ul class="tid-locations">{locations}</ul>
            })}
            {feedback.map(feedback_view)}
            {appeal}
            {review}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 5. Asking for a review
// -----------------------------------------------------------------------------

#[component]
fn ReviewForm(reference: String, asked: RwSignal<Vec<(String, String)>>) -> impl IntoView {
    let statement = RwSignal::new(String::new());
    let problem = RwSignal::new(Option::<String>::None);
    let busy = RwSignal::new(false);
    let watched = reference.clone();
    let done = Memo::new(move |_| asked.with(|a| a.iter().find(|(r, _)| *r == watched).map(|(_, s)| s.clone())));
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if busy.get_untracked() || done.get_untracked().is_some() {
            return;
        }
        let text = statement.get_untracked();
        if let Some(why) = validate_statement(&text) {
            problem.set(Some(why));
            focus_id("tid-review-statement");
            return;
        }
        problem.set(None);
        busy.set(true);
        let body = review_body(&text);
        let path = review_path(&reference);
        let reference = reference.clone();
        spawn_local(async move {
            let client = ApiClient::new(&crate::state::api_base());
            let answer = client.post::<ReviewReply, _>(&path, &body).await;
            busy.set(false);
            match answer {
                Ok(reply) => asked.update(|a| a.push((reference, reply.status))),
                Err(e) => problem.set(Some(review_error_words(&e))),
            }
        });
    };
    view! {
        <div class="tid-review">
            <h3>"Ask for a review"</h3>
            <p>"If you think this was decided wrongly, tell us why and a person will look again."</p>
            {move || done.get().map(|status| {
                let tail = if status.trim().is_empty() { String::new() } else { format!(" Status: {}.", humanize(&status)) };
                view! { <p class="tid-ok" role="status">{format!("Your request for a review was sent.{tail}")}</p> }
            })}
            <form class="tid-form" novalidate on:submit=on_submit>
                <div class="tid-field">
                    <label for="tid-review-statement">"Why should we look again?"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                    <textarea id="tid-review-statement" rows="5" aria-required="true"
                        disabled=move || done.get().is_some()
                        aria-invalid=move || if problem.get().is_some() { "true" } else { "false" }
                        aria-describedby=move || if problem.get().is_some() { "tid-review-hint tid-review-err" } else { "tid-review-hint" }
                        prop:value=move || statement.get()
                        on:input=move |ev: leptos::ev::Event| {
                            statement.set(event_target_value(&ev));
                            problem.set(None);
                        }></textarea>
                    <p class="tid-hint" id="tid-review-hint">
                        {move || format!("{STATEMENT_MIN} to {STATEMENT_MAX} characters. You have written {}.", statement.get().trim().chars().count())}
                    </p>
                    {move || problem.get().map(|m| view! { <p class="tid-error" id="tid-review-err" role="alert">{m}</p> })}
                </div>
                <button type="submit" class="tid-btn tid-btn-primary" disabled=move || busy.get() || done.get().is_some()>
                    {move || if busy.get() { "Sending..." } else { "Ask for a review" }}
                </button>
            </form>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 6. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    fn html<V: RenderHtml>(build: impl FnOnce() -> V) -> String {
        let owner = Owner::new();
        owner.with(|| build().to_html())
    }

    fn review_case() -> CaseEntry {
        CaseEntry {
            reference: "EC-AAAA-BBBB-CCCC-DDDD-EEEE".to_string(),
            kind: "listing_review".to_string(),
            listing: Some(ListingRef { id: "l1".to_string(), name: "Tower".to_string() }),
            status: "decided".to_string(),
            outcome: Some("changes_needed".to_string()),
            headline: "Review of Tower".to_string(),
            opened_at: Some("2026-10-01T10:00:00Z".to_string()),
            updated_at: Some("2026-10-02T10:00:00Z".to_string()),
            feedback: Some(Feedback {
                reasons: vec![Reason { title: "Unclear sign".to_string(), why: "It reads as a logo.".to_string(), what_to_change: "Remove the logo.".to_string() }],
                articles: vec!["B6".to_string()],
                tests: vec!["T3".to_string()],
                suggested_edit: Some("Rename it.".to_string()),
                note: Some("Thanks for building.".to_string()),
                ..Feedback::default()
            }),
            can_appeal: true,
            ..CaseEntry::default()
        }
    }

    fn notice_case() -> CaseEntry {
        CaseEntry {
            reference: "EC-FFFF-GGGG-HHHH-JJJJ-KKKK".to_string(),
            kind: "takedown_notice".to_string(),
            listing: Some(ListingRef { id: "l2".to_string(), name: "Park".to_string() }),
            status: "decided".to_string(),
            outcome: Some("content_restored".to_string()),
            headline: "Park was removed".to_string(),
            removed_at: Some("2026-10-03T11:00:00Z".to_string()),
            feedback: Some(Feedback { reasons: vec![Reason { title: "Reported".to_string(), ..Reason::default() }], ..Feedback::default() }),
            can_request_review: true,
            ..CaseEntry::default()
        }
    }

    fn request_case() -> CaseEntry {
        CaseEntry {
            reference: "EC-MMMM-NNNN-PPPP-QQQQ-RRRR".to_string(),
            kind: "takedown_request".to_string(),
            status: "removed".to_string(),
            headline: "Your removal request".to_string(),
            locations: vec![CaseLocation { location: "listing-77".to_string(), result: "removed".to_string() }],
            can_request_review: true,
            ..CaseEntry::default()
        }
    }

    fn body(c: CaseEntry) -> String {
        html(|| {
            let asked = RwSignal::new(Vec::<(String, String)>::new());
            view! { <CaseBody case=c asked=asked /> }
        })
    }

    #[test]
    fn a_listing_review_shows_its_feedback_in_words_and_links_to_the_existing_appeal_panel() {
        let page = body(review_case());
        for needle in ["Review of Tower", "Unclear sign", "It reads as a logo.", "Remove the logo.", "Articles cited: B6", "Tests applied: T3", "Rename it.", "Thanks for building.", "Listing: ", "Tower"] {
            assert!(page.contains(needle), "{needle}: {page}");
        }
        assert!(page.contains("href=\"/projects?review=l1\""), "{page}");
        assert!(page.contains(">Copy<") && page.contains("value=\"EC-AAAA-BBBB-CCCC-DDDD-EEEE\""));
        assert!(!page.contains("Ask for a review"), "an appeal goes through the review panel, not a second form");
    }

    #[test]
    fn a_listing_review_with_an_appeal_on_file_says_so_and_without_one_offers_none() {
        let mut c = review_case();
        c.can_appeal = false;
        c.appeal = Some(serde_json::json!({ "status": "open" }));
        let page = body(c.clone());
        assert!(page.contains("An appeal is on file") && !page.contains("/projects?review="), "{page}");
        c.appeal = None;
        let none = body(c);
        assert!(!none.contains("An appeal is on file") && !none.contains("/projects?review="));
    }

    #[test]
    fn a_takedown_notice_says_when_content_was_restored_and_offers_a_review() {
        let page = body(notice_case());
        assert!(page.contains("Your content was restored."), "{page}");
        assert!(page.contains("Reported") && page.contains("Removed"), "{page}");
        assert!(page.contains("Ask for a review") && page.contains("id=\"tid-review-statement\"") && page.contains("for=\"tid-review-statement\""));
        assert!(!page.contains("/projects?review="));
    }

    #[test]
    fn a_takedown_request_lists_each_location_and_offers_a_review() {
        let page = body(request_case());
        assert!(page.contains("listing-77") && page.contains("Removed."), "{page}");
        assert!(page.contains("Ask for a review"));
        assert!(!page.contains("What we found"), "no feedback for a request the account filed");
    }

    #[test]
    fn a_case_that_cannot_be_reviewed_has_no_review_form() {
        let mut c = request_case();
        c.can_request_review = false;
        assert!(!body(c).contains("Ask for a review"));
    }

    #[test]
    fn the_list_shows_a_row_per_case_and_load_more_only_while_there_is_a_next_cursor() {
        let render = |next_cursor: Option<&str>| {
            html(|| {
                let cases = RwSignal::new(vec![review_case(), notice_case(), request_case()]);
                let total = RwSignal::new(7u64);
                let next = RwSignal::new(next_cursor.map(String::from));
                let selected = RwSignal::new(Option::<String>::None);
                view! { <CaseList cases=cases total=total next=next selected=selected /> }
            })
        };
        let more = render(Some("c2"));
        assert_eq!(more.matches("tid-case-row").count(), 3, "{more}");
        assert!(more.contains("Load more") && more.contains("Showing 3 of 7 cases, newest first."), "{more}");
        for r in ["EC-AAAA-BBBB-CCCC-DDDD-EEEE", "EC-FFFF-GGGG-HHHH-JJJJ-KKKK", "EC-MMMM-NNNN-PPPP-QQQQ-RRRR"] {
            assert!(more.contains(r), "{r}");
        }
        let last = render(None);
        assert!(!last.contains("Load more"), "{last}");
        let empty = html(|| {
            view! { <CaseList cases=RwSignal::new(Vec::new()) total=RwSignal::new(0u64) next=RwSignal::new(None) selected=RwSignal::new(None) /> }
        });
        assert!(empty.contains("You have no cases"), "{empty}");
    }

    #[test]
    fn a_row_is_the_headline_the_status_and_the_last_change() {
        let (headline, status, when) = row_parts(&review_case());
        assert_eq!((headline.as_str(), status.as_str(), when.as_str()), ("Review of Tower", "Decided", "Oct 2, 2026, 10:00 AM UTC"));
        let mut c = request_case();
        c.headline = "  ".to_string();
        assert_eq!(row_parts(&c).0, "Removal request");
        c.opened_at = Some("2026-10-04T10:00:00Z".to_string());
        assert_eq!(row_parts(&c).2, "Oct 4, 2026, 10:00 AM UTC", "opened stands in when there is no update");
    }

    #[test]
    fn a_page_read_is_sorted_into_the_list_signed_out_or_failed() {
        let page = CasesPage { cases: vec![request_case()], total: 1, next_cursor: None };
        assert_eq!(classify(Ok(page.clone())), Load::Page(page));
        assert_eq!(classify(Err(ApiError::Unauthorized)), Load::SignedOut);
        assert!(matches!(classify(Err(ApiError::NotFound)), Load::Failed(m) if m.contains("not open")));
        assert!(matches!(classify(Err(ApiError::Server { status: 500, message: String::new() })), Load::Failed(m) if m.contains("500")));
    }

    #[test]
    fn signed_out_visitors_are_asked_to_sign_in() {
        let page = html(|| view! { <SignInNotice /> });
        assert!(page.contains("Sign in to see your cases") && page.contains("href=\"/login\""));
    }

    #[test]
    fn the_feedback_lines_skip_what_is_empty() {
        assert_eq!(list_line("Tests applied", &["T3".to_string(), " ".to_string(), "T4".to_string()]), Some("Tests applied: T3, T4".to_string()));
        assert_eq!(list_line("Tests applied", &[]), None);
        assert!(!feedback_has_content(&Feedback::default()));
        assert!(feedback_has_content(&Feedback { note: Some("x".to_string()), ..Feedback::default() }));
        assert!(!feedback_has_content(&Feedback { note: Some("  ".to_string()), ..Feedback::default() }));
    }

    #[test]
    fn the_cases_page_routes_and_the_profile_link_exist() {
        assert!(include_str!("../app.rs").contains("path!(\"/account/cases\")"));
        assert!(include_str!("profile.rs").contains("href=\"/account/cases\""));
    }
}
