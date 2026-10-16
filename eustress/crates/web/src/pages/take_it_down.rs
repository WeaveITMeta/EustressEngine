// =============================================================================
// Eustress Web - /take-it-down: remove an intimate image shared without consent
// =============================================================================
// A public notice of how the process works, and the request form behind it. No account is needed. The form posts to
// POST /api/takedown/requests (the Worker takes it from anyone; a signed-in visitor's token rides along) and answers with a case
// reference, which links to /cases/{reference}.
//
// Every wire name, limit and rule is in takedown_wire.rs. This file is the notice, the form and the result view.
//
// Table of Contents:
// 1. Browser helpers: focus, select, clipboard
// 2. The reference with a Copy button
// 3. The page and the notice
// 4. The form
// 5. The result view
// 6. Tests
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::takedown_wire::*;
use crate::api::ApiClient;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// 1. Browser helpers
// -----------------------------------------------------------------------------

/// Move focus to the element with this id, if it is on the page.
pub fn focus_id(id: &str) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(id)) {
        if let Ok(el) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = el.focus();
        }
    }
}

/// Focus once the page has put the element in place.
pub fn focus_soon(id: &'static str) {
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(30).await;
        focus_id(id);
    });
}

/// Select the text of the input with this id, so Ctrl+C copies it.
fn select_input(id: &str) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(id)) {
        if let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() {
            input.select();
        }
    }
}

/// Begin writing to the clipboard, inside the click that asked for it. `None` when the browser has no clipboard to offer, as on
/// a page that is not secure.
fn start_clipboard_write(text: &str) -> Option<js_sys::Promise> {
    let nav: JsValue = web_sys::window()?.navigator().into();
    let clipboard = js_sys::Reflect::get(&nav, &JsValue::from_str("clipboard")).ok()?;
    if clipboard.is_undefined() || clipboard.is_null() {
        return None;
    }
    let write: js_sys::Function = js_sys::Reflect::get(&clipboard, &JsValue::from_str("writeText")).ok()?.dyn_into().ok()?;
    write.call1(&clipboard, &JsValue::from_str(text)).ok()?.dyn_into::<js_sys::Promise>().ok()
}

// -----------------------------------------------------------------------------
// 2. The reference with a Copy button
// -----------------------------------------------------------------------------

/// What the line under the reference says when the browser would not copy for us and the text is selected instead.
const SELECTED_NOTE: &str = "The reference is selected. Press Ctrl+C, or Command+C on a Mac, to copy it.";

/// A case reference in a read-only box with a Copy button. If the clipboard is not available or refuses, the text in the box is
/// selected and the line beside the button says how to copy it.
#[component]
pub fn CopyReference(reference: String, id: &'static str) -> impl IntoView {
    let note = RwSignal::new(String::new());
    let text = reference.clone();
    let copy = move |_| match start_clipboard_write(&text) {
        Some(promise) => spawn_local(async move {
            if JsFuture::from(promise).await.is_ok() {
                note.set("Copied.".to_string());
            } else {
                select_input(id);
                note.set(SELECTED_NOTE.to_string());
            }
        }),
        None => {
            select_input(id);
            note.set(SELECTED_NOTE.to_string());
        }
    };
    view! {
        <div class="tid-ref">
            <label class="tid-sr" for=id>"Case reference"</label>
            <input id=id class="tid-ref-input" type="text" readonly value=reference on:focus=move |_| select_input(id) />
            <button type="button" class="tid-btn" on:click=copy>"Copy"</button>
            <span class="tid-copy-note" role="status">{move || note.get()}</span>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. The page and the notice
// -----------------------------------------------------------------------------

#[component]
pub fn TakeItDownPage() -> impl IntoView {
    view! {
        <div class="page page-legal page-takedown">
            <CentralNav active="".to_string() />
            <main class="legal-container tid-container">
                <div class="legal-header">
                    <h1>"Remove intimate images"</h1>
                    <p class="legal-updated">"For intimate images and videos shared without consent, including ones made by AI. No account needed."</p>
                </div>
                <TakeItDownNotice />
                <TakedownRequest />
            </main>
            <Footer />
        </div>
    }
}

/// How the process works, in plain words.
#[component]
pub fn TakeItDownNotice() -> impl IntoView {
    // LEGAL REVIEW: McKale must read every sentence of this notice before launch. It states the scope, the 48 hour limit, the
    // removal of identical copies, and the rule on false requests.
    view! {
        <div class="legal-content tid-notice">
            <section class="legal-section">
                <h2>"What this page is for"</h2>
                <p>"If an intimate image or video of you was shared without your consent, you can ask us to remove it. This page is how you ask."</p>
                <p>"It applies to images and videos that are real, edited, or made by AI."</p>
            </section>

            <section class="legal-section">
                <h2>"Who can ask"</h2>
                <ul>
                    <li>"You can ask, if you are the person shown."</li>
                    <li>"Someone you have authorized to act for you can ask for you. They must give your name."</li>
                </ul>
            </section>

            <section class="legal-section">
                <h2>"What counts"</h2>
                <ul>
                    <li>"An intimate image or video of a real person who can be identified."</li>
                    <li>"Published without that person's consent."</li>
                    <li>"Real, edited, or made by AI. All three count."</li>
                </ul>
            </section>

            <section class="legal-section">
                <h2>"What to include"</h2>
                <ul>
                    <li>"Your full name, and your name typed again as your signature."</li>
                    <li>"Whether the image is of you, or you are acting for the person shown. If you are acting for them, their name."</li>
                    <li>"An email address we can reach you at."</li>
                    <li>"Where the content is: links to it on Eustress, or listing IDs. You can list up to 20."</li>
                    <li>"A short description of the content."</li>
                    <li>"A tick to confirm, in good faith, that the request is true."</li>
                </ul>
                <p>"If the person shown is under 18, tick the box on the form that says so."</p>
            </section>

            <section class="legal-section">
                <h2>"What happens next"</h2>
                <ul>
                    <li>"If the request is valid, we remove the content right away."</li>
                    <li>"We remove it within 48 hours at the latest."</li>
                    <li>"We also remove known identical copies."</li>
                    <li>"You get a case reference. Use it to check the status of your case at any time."</li>
                </ul>
                <p>"We use what you send only to handle this request."</p>
            </section>

            <section class="legal-section">
                <h2>"Do not send a false request"</h2>
                <p>"A false request, or one made in bad faith, is not allowed."</p>
            </section>

            <section class="legal-section">
                <h2>"You do not need an account"</h2>
                <p>"Fill in the form below. You do not need to sign in or make an account."</p>
            </section>
        </div>
    }
}

/// The form, or after it is sent, the result.
#[component]
pub fn TakedownRequest() -> impl IntoView {
    let receipt = RwSignal::new(Option::<Receipt>::None);
    view! {
        <section class="tid-request" id="request" aria-label="Removal request">
            {move || match receipt.get() {
                Some(r) => view! { <ReceiptView receipt=r /> }.into_any(),
                None => view! { <TakedownForm receipt=receipt /> }.into_any(),
            }}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 4. The form
// -----------------------------------------------------------------------------

type Errors = Vec<(FormField, String)>;

fn error_text(errors: RwSignal<Errors>, f: FormField) -> Option<String> {
    errors.with(|e| e.iter().find(|(x, _)| *x == f).map(|(_, m)| m.clone()))
}

fn invalid_attr(errors: RwSignal<Errors>, f: FormField) -> &'static str {
    if errors.with(|e| e.iter().any(|(x, _)| *x == f)) {
        "true"
    } else {
        "false"
    }
}

/// The ids a control is described by: its hint, and its error line while it has one.
fn describedby(errors: RwSignal<Errors>, f: FormField, hint: Option<&'static str>) -> Option<String> {
    let mut ids: Vec<&str> = Vec::new();
    if let Some(h) = hint {
        ids.push(h);
    }
    if errors.with(|e| e.iter().any(|(x, _)| *x == f)) {
        ids.push(f.error_id());
    }
    if ids.is_empty() {
        None
    } else {
        Some(ids.join(" "))
    }
}

fn clear_error(errors: RwSignal<Errors>, f: FormField) {
    errors.update(|e| e.retain(|(x, _)| *x != f));
}

/// The line under a control that says what is wrong with it.
fn error_line(errors: RwSignal<Errors>, f: FormField) -> impl IntoView {
    move || error_text(errors, f).map(|m| view! { <p class="tid-error" id=f.error_id()>{m}</p> })
}

#[component]
fn TakedownForm(receipt: RwSignal<Option<Receipt>>) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let signature = RwSignal::new(String::new());
    let representative = RwSignal::new(false);
    let depicted = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let locations = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let good_faith = RwSignal::new(false);
    let minor = RwSignal::new(false);

    let errors = RwSignal::new(Errors::new());
    let general = RwSignal::new(Option::<String>::None);
    let busy = RwSignal::new(false);

    let typed = move |sig: RwSignal<String>, f: FormField| {
        move |ev: leptos::ev::Event| {
            sig.set(event_target_value(&ev));
            clear_error(errors, f);
        }
    };
    let ticked = move |sig: RwSignal<bool>, f: FormField| {
        move |ev: leptos::ev::Event| {
            sig.set(event_target_checked(&ev));
            clear_error(errors, f);
        }
    };

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let values = FormValues {
            name: name.get_untracked(),
            signature: signature.get_untracked(),
            representative: representative.get_untracked(),
            depicted_name: depicted.get_untracked(),
            email: email.get_untracked(),
            locations_text: locations.get_untracked(),
            description: description.get_untracked(),
            good_faith: good_faith.get_untracked(),
            depicts_minor: minor.get_untracked(),
        };
        let found = validate(&values);
        if let Some((first, _)) = found.first() {
            let first = *first;
            general.set(Some(fix_summary(found.len())));
            errors.set(found);
            focus_id(first.input_id());
            return;
        }
        errors.set(Vec::new());
        general.set(None);
        busy.set(true);
        let body = build_body(&values);
        spawn_local(async move {
            let client = ApiClient::new(&crate::state::api_base());
            let answer = client.post::<serde_json::Value, _>(REQUESTS_PATH, &body).await;
            busy.set(false);
            match answer {
                Ok(value) => {
                    receipt.set(Some(parse_receipt(&value)));
                    focus_soon("tid-result-title");
                }
                Err(e) => match parse_submit_error(&e) {
                    SubmitError::Invalid { message, fields, other } => {
                        let first = fields.first().map(|(f, _)| *f);
                        let mut text = message;
                        for line in other {
                            text.push(' ');
                            text.push_str(&line);
                        }
                        general.set(Some(text));
                        errors.set(fields);
                        if let Some(f) = first {
                            focus_id(f.input_id());
                        }
                    }
                    other => general.set(Some(other.message().to_string())),
                },
            }
        });
    };

    view! {
        <form class="tid-form" novalidate on:submit=on_submit aria-labelledby="tid-form-title">
            <h2 id="tid-form-title">"Send a removal request"</h2>
            <p class="tid-legend"><span class="tid-req" aria-hidden="true">"*"</span>" means the field is required."</p>

            {move || general.get().map(|m| view! { <div class="tid-alert" role="alert" id="tid-general">{m}</div> })}

            <div class="tid-field">
                <label for="tid-name">"Your full name"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                <input id="tid-name" type="text" autocomplete="name" aria-required="true"
                    aria-invalid=move || invalid_attr(errors, FormField::Name)
                    aria-describedby=move || describedby(errors, FormField::Name, None)
                    prop:value=move || name.get() on:input=typed(name, FormField::Name) />
                {error_line(errors, FormField::Name)}
            </div>

            <div class="tid-field">
                <label for="tid-signature">"Your signature"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                <input id="tid-signature" type="text" autocomplete="off" aria-required="true"
                    aria-invalid=move || invalid_attr(errors, FormField::Signature)
                    aria-describedby=move || describedby(errors, FormField::Signature, Some("tid-signature-hint"))
                    prop:value=move || signature.get() on:input=typed(signature, FormField::Signature) />
                <p class="tid-hint" id="tid-signature-hint">"Type your full name again. It works as your electronic signature."</p>
                {error_line(errors, FormField::Signature)}
            </div>

            <fieldset class="tid-fieldset"
                aria-describedby=move || describedby(errors, FormField::Relationship, None)>
                <legend>"Who is shown in the content?"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></legend>
                <label class="tid-choice" for="tid-rel-self">
                    <input id="tid-rel-self" type="radio" name="tid-relationship"
                        prop:checked=move || !representative.get()
                        on:change=move |_| {
                            representative.set(false);
                            clear_error(errors, FormField::DepictedName);
                            clear_error(errors, FormField::Relationship);
                        } />
                    " I am the person shown."
                </label>
                <label class="tid-choice" for="tid-rel-rep">
                    <input id="tid-rel-rep" type="radio" name="tid-relationship"
                        prop:checked=move || representative.get()
                        on:change=move |_| {
                            representative.set(true);
                            clear_error(errors, FormField::Relationship);
                        } />
                    " I am acting for the person shown. They authorized me."
                </label>
                {error_line(errors, FormField::Relationship)}
            </fieldset>

            {move || representative.get().then(|| view! {
                <div class="tid-field">
                    <label for="tid-depicted">"Full name of the person shown"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                    <input id="tid-depicted" type="text" autocomplete="off" aria-required="true"
                        aria-invalid=move || invalid_attr(errors, FormField::DepictedName)
                        aria-describedby=move || describedby(errors, FormField::DepictedName, None)
                        prop:value=move || depicted.get() on:input=typed(depicted, FormField::DepictedName) />
                    {error_line(errors, FormField::DepictedName)}
                </div>
            })}

            <div class="tid-field">
                <label for="tid-email">"Your email address"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                <input id="tid-email" type="email" autocomplete="email" aria-required="true"
                    aria-invalid=move || invalid_attr(errors, FormField::Email)
                    aria-describedby=move || describedby(errors, FormField::Email, Some("tid-email-hint"))
                    prop:value=move || email.get() on:input=typed(email, FormField::Email) />
                <p class="tid-hint" id="tid-email-hint">"We write to this address about your request."</p>
                {error_line(errors, FormField::Email)}
            </div>

            <div class="tid-field">
                <label for="tid-locations">"Where the content is"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                <textarea id="tid-locations" rows="4" aria-required="true" spellcheck="false"
                    aria-invalid=move || invalid_attr(errors, FormField::Locations)
                    aria-describedby=move || describedby(errors, FormField::Locations, Some("tid-locations-hint"))
                    prop:value=move || locations.get() on:input=typed(locations, FormField::Locations)></textarea>
                <p class="tid-hint" id="tid-locations-hint">
                    {move || format!("One link or listing ID per line. Up to {LOCATIONS_MAX}. You have listed {}.", parse_locations(&locations.get()).len())}
                </p>
                {error_line(errors, FormField::Locations)}
            </div>

            <div class="tid-field">
                <label for="tid-description">"Short description"<span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span></label>
                <textarea id="tid-description" rows="5" aria-required="true"
                    aria-invalid=move || invalid_attr(errors, FormField::Description)
                    aria-describedby=move || describedby(errors, FormField::Description, Some("tid-description-hint"))
                    prop:value=move || description.get() on:input=typed(description, FormField::Description)></textarea>
                <p class="tid-hint" id="tid-description-hint">
                    {move || format!("Say what the content is. {DESCRIPTION_MIN} to {DESCRIPTION_MAX} characters. You have written {}.", description.get().trim().chars().count())}
                </p>
                {error_line(errors, FormField::Description)}
            </div>

            <div class="tid-field tid-check">
                <label class="tid-choice" for="tid-minor">
                    <input id="tid-minor" type="checkbox"
                        aria-invalid=move || invalid_attr(errors, FormField::Minor)
                        aria-describedby=move || describedby(errors, FormField::Minor, None)
                        prop:checked=move || minor.get() on:change=ticked(minor, FormField::Minor) />
                    " The person shown is under 18."
                </label>
                {error_line(errors, FormField::Minor)}
            </div>

            // LEGAL REVIEW: the good-faith statement the requester ticks. McKale must approve the exact words.
            <div class="tid-field tid-check">
                <label class="tid-choice" for="tid-good-faith">
                    <input id="tid-good-faith" type="checkbox" aria-required="true"
                        aria-invalid=move || invalid_attr(errors, FormField::GoodFaith)
                        aria-describedby=move || describedby(errors, FormField::GoodFaith, None)
                        prop:checked=move || good_faith.get() on:change=ticked(good_faith, FormField::GoodFaith) />
                    " I state in good faith that this content shows an intimate image or video of me, or of the person I act for, and that it was shared without consent. The information in this request is true."
                    <span class="tid-req" aria-hidden="true">" *"</span><span class="tid-sr">" (required)"</span>
                </label>
                {error_line(errors, FormField::GoodFaith)}
            </div>

            <button type="submit" class="tid-btn tid-btn-primary" disabled=move || busy.get() aria-busy=move || if busy.get() { "true" } else { "false" }>
                {move || if busy.get() { "Sending..." } else { "Send removal request" }}
            </button>
        </form>
    }
}

// -----------------------------------------------------------------------------
// 5. The result view
// -----------------------------------------------------------------------------

// LEGAL REVIEW: the fallback confirmation, and the line when no reference comes back, are said to a requester. McKale must approve both.
/// What the result view says when the answer carried no message of its own.
const RECEIVED_DEFAULT: &str = "We received your removal request.";

/// What the result view says when the answer carried no reference.
const NO_REFERENCE: &str = "Your request was received, but its case reference did not come back. Write to legal@eustress.dev and say you sent a removal request.";

/// The case reference, a Copy button, what became of each location, and the link to the status page.
#[component]
pub fn ReceiptView(receipt: Receipt) -> impl IntoView {
    let message = receipt.message.clone().unwrap_or_else(|| RECEIVED_DEFAULT.to_string());
    let reference_block = if receipt.reference.is_empty() {
        view! { <p>{NO_REFERENCE}</p> }.into_any()
    } else {
        let link = case_link(&receipt.reference);
        view! {
            <p class="tid-label">"Your case reference"</p>
            <CopyReference reference=receipt.reference.clone() id="tid-ref" />
            <p>"Keep this reference. You need it to check on your case."</p>
            <p><a class="tid-btn tid-btn-primary" href=link>"Check the status of this case"</a></p>
        }
        .into_any()
    };
    let rows = receipt
        .locations
        .iter()
        .map(|l| {
            view! {
                <li>
                    <span class="tid-loc">{l.location.clone()}</span>
                    <span class="tid-loc-result">{result_words(&l.result)}</span>
                </li>
            }
        })
        .collect_view();
    let has_rows = !receipt.locations.is_empty();
    view! {
        <div class="tid-result" role="status" id="tid-result">
            <h2 id="tid-result-title" tabindex="-1">"We received your request"</h2>
            <p>{message}</p>
            {reference_block}
            {has_rows.then(|| view! {
                <h3>"What happened to each item"</h3>
                <ul class="tid-locations">{rows}</ul>
            })}
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

    const SAMPLE_REF: &str = "EC-7K2M-9QXD-4TPF-HJ3W-5XRZ";

    fn receipt() -> Receipt {
        Receipt {
            reference: SAMPLE_REF.to_string(),
            status: "removed".to_string(),
            message: Some("We removed it.".to_string()),
            locations: vec![
                LocationResult { location: "https://eustress.dev/simulation/abc".to_string(), result: "removed".to_string() },
                LocationResult { location: "listing-77".to_string(), result: "pending".to_string() },
            ],
            ..Receipt::default()
        }
    }

    #[test]
    fn the_result_view_shows_the_reference_the_message_each_location_and_links_to_the_case() {
        let page = html(|| view! { <ReceiptView receipt=receipt() /> });
        assert!(page.contains("role=\"status\""), "{page}");
        assert!(page.contains(&format!("value=\"{SAMPLE_REF}\"")), "{page}");
        assert!(page.contains("href=\"/cases/EC-7K2M-9QXD-4TPF-HJ3W-5XRZ\""), "{page}");
        assert!(page.contains("We removed it."));
        assert!(page.contains("https://eustress.dev/simulation/abc") && page.contains("Removed."));
        assert!(page.contains("listing-77") && page.contains("Removal is in progress."));
        assert!(page.contains(">Copy<"), "{page}");
    }

    #[test]
    fn a_result_without_a_reference_does_not_offer_a_link_to_nowhere() {
        let page = html(|| view! { <ReceiptView receipt=Receipt::default() /> });
        assert!(!page.contains("/cases/") && page.contains("did not come back"), "{page}");
    }

    #[test]
    fn the_form_has_a_label_for_every_control_a_required_marker_and_the_send_button() {
        let page = html(|| view! { <TakedownRequest /> });
        for id in ["tid-name", "tid-signature", "tid-rel-self", "tid-rel-rep", "tid-email", "tid-locations", "tid-description", "tid-minor", "tid-good-faith"] {
            assert!(page.contains(&format!("id=\"{id}\"")), "{id}");
            assert!(page.contains(&format!("for=\"{id}\"")), "a label for {id}");
        }
        assert!(page.contains("Send removal request"));
        assert!(page.contains("novalidate"));
        assert!(page.contains("(required)"), "required fields are marked for a screen reader");
        assert!(page.contains("The person shown is under 18."));
        assert!(page.contains("aria-required=\"true\""));
        // the depicted person's name only appears for a representative
        assert!(!page.contains("id=\"tid-depicted\""));
    }

    #[test]
    fn the_notice_says_the_five_things_it_must() {
        let page = html(|| view! { <TakeItDownNotice /> });
        for needle in [
            "Who can ask",
            "AI",
            "without that person",
            "What to include",
            "right away",
            "48 hours",
            "identical copies",
            "false request",
            "do not need an account",
        ] {
            assert!(page.contains(needle), "the notice mentions {needle}");
        }
    }

    #[test]
    fn the_legal_copy_is_marked_for_review_and_cites_no_statute_section() {
        let src = include_str!("take_it_down.rs");
        assert!(src.matches(concat!("// LEGAL ", "REVIEW")).count() >= 3, "the notice, the good-faith words and the fallback lines are marked");
        let pages = [html(|| view! { <TakeItDownNotice /> }), html(|| view! { <TakedownRequest /> })];
        for page in pages {
            for needle in ["U.S.C", "USC", "Section ", "\u{a7}"] {
                assert!(!page.contains(needle), "no citation: {needle}");
            }
        }
    }

    #[test]
    fn no_visible_text_on_the_page_uses_an_em_dash_or_an_en_dash() {
        let dashes = ['\u{2014}', '\u{2013}'];
        let rendered = [
            html(|| view! { <TakeItDownNotice /> }),
            html(|| view! { <TakedownRequest /> }),
            html(|| view! { <ReceiptView receipt=receipt() /> }),
            html(|| view! { <ReceiptView receipt=Receipt::default() /> }),
        ];
        for page in rendered {
            assert!(!page.contains(dashes), "a dash in the rendered page");
        }
        for (name, src) in [
            ("take_it_down.rs", include_str!("take_it_down.rs")),
            ("case_status.rs", include_str!("case_status.rs")),
            ("account_cases.rs", include_str!("account_cases.rs")),
            ("takedown_wire.rs", include_str!("takedown_wire.rs")),
        ] {
            assert!(!src.contains(dashes), "a dash in {name}");
        }
        for t in [SELECTED_NOTE, RECEIVED_DEFAULT, NO_REFERENCE] {
            assert!(!t.contains(dashes), "{t}");
        }
    }

    #[test]
    fn the_footer_reaches_the_page_through_acts_and_the_sitemap_the_support_page_and_the_routes_carry_it() {
        // McKale (2026-10-09): the footer links Acts, and Acts links the form; no separate footer entry.
        let footer = include_str!("../components/footer.rs");
        assert!(footer.contains("href=\"/acts\"") && !footer.contains("href=\"/take-it-down\""));
        assert!(include_str!("acts.rs").contains("href=\"/take-it-down\""), "Acts links the request form");
        assert!(include_str!("../../sitemap.xml").contains("<loc>https://eustress.dev/take-it-down</loc>"));
        assert!(include_str!("support.rs").contains("href=\"/take-it-down\""));
        let app = include_str!("../app.rs");
        assert!(app.contains("path!(\"/take-it-down\")") && app.contains("path!(\"/cases/:reference\")") && app.contains("path!(\"/account/cases\")"));
        assert!(include_str!("../../index.html").contains("style/take_it_down.css"));
        assert!(include_str!("profile.rs").contains("href=\"/account/cases\""));
    }
}
