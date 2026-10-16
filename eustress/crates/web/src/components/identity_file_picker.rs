// =============================================================================
// Eustress Web - Choosing an identity file
// =============================================================================
// A visible file field, and under it one line that says what happened to the
// file that was chosen: being read, read (with a short form of its public key so
// the member can tell which file it is), or why it could not be used. A choice
// never ends in silence. Used by every Settings step that signs with the current
// key: replacing it and changing the address.
//
// The field is an ordinary one. (The sign-up page's drop zone stretches an
// invisible input over its nearest positioned ancestor; inside Settings that
// ancestor is the whole layout, so every click, "Check my file" included, opened
// the file dialog instead.)
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::identity_file::{fingerprint, parse_identity_file, unreadable, IdentityFile, MAX_IDENTITY_FILE_BYTES, TOO_LARGE};

/// What happened to the file that was chosen.
#[derive(Clone, PartialEq, Debug)]
pub enum Picked {
    Nothing,
    Reading(String),
    Read { name: String, key: String },
    Unreadable(String),
}

/// The status line for each stage, as the page says it.
pub fn status_line(picked: &Picked) -> Option<String> {
    match picked {
        Picked::Nothing | Picked::Unreadable(_) => None,
        Picked::Reading(name) => Some(format!("Reading {name}...")),
        Picked::Read { name, key } => Some(format!("Read {name}. Its public key is {key}.")),
    }
}

/// A reason from the browser, for the sentence "We couldn't read this file: ...".
fn reason_of(error: &wasm_bindgen::JsValue) -> String {
    js_sys::Reflect::get(error, &"message".into())
        .ok()
        .and_then(|m| m.as_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "the browser would not open it. Pick the file again.".to_string())
}

/// A file field for the member's current identity file. `current` holds the file once it is read
/// (and `None` while a new choice is being read, or when it could not be); `on_pick` runs when a
/// new file is chosen, so the step that used the last one can start again.
#[component]
pub fn IdentityFilePicker(current: RwSignal<Option<IdentityFile>>, #[prop(into)] id: String, #[prop(optional)] on_pick: Option<Callback<()>>) -> impl IntoView {
    let picked = RwSignal::new(Picked::Nothing);
    let status_id = format!("{id}-status");

    let on_change = move |e: leptos::ev::Event| {
        use wasm_bindgen::JsCast;
        let Some(input) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        // The dialog was closed without a choice: what was there stays.
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        current.set(None);
        if let Some(run) = on_pick {
            run.run(());
        }
        if file.size() > MAX_IDENTITY_FILE_BYTES as f64 {
            picked.set(Picked::Unreadable(unreadable(TOO_LARGE)));
            return;
        }
        let name = file.name();
        picked.set(Picked::Reading(name.clone()));
        spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(file.text()).await {
                Ok(text) => match parse_identity_file(&text.as_string().unwrap_or_default()) {
                    Ok(read) => {
                        picked.set(Picked::Read { name, key: fingerprint(&read.public_key) });
                        current.set(Some(read));
                    }
                    Err(why) => picked.set(Picked::Unreadable(unreadable(&why))),
                },
                Err(error) => picked.set(Picked::Unreadable(unreadable(&reason_of(&error)))),
            }
        });
    };

    // The browser sends no "change" for a file it already holds, so choosing the same file again
    // (after fixing it, or after a failed check) would do nothing. Emptying the field when it is
    // opened makes every choice a change.
    let on_click = move |e: leptos::ev::MouseEvent| {
        use wasm_bindgen::JsCast;
        if let Some(input) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
            input.set_value("");
        }
    };

    view! {
        <div class="form-field">
            <label class="form-label" for=id.clone()>"Your current identity file"</label>
            <input id=id type="file" accept=".toml" class="file-picker" aria-describedby=status_id.clone() on:change=on_change on:click=on_click />
            <div id=status_id class="file-status" role="status" aria-live="polite">
                {move || {
                    let now = picked.get();
                    match (&now, status_line(&now)) {
                        (Picked::Unreadable(message), _) => view! { <div class="alert alert-error" role="alert">{message.clone()}</div> }.into_any(),
                        (Picked::Read { .. }, Some(line)) => view! { <p class="file-ok">{line}</p> }.into_any(),
                        (_, Some(line)) => view! { <p class="info-text">{line}</p> }.into_any(),
                        _ => ().into_any(),
                    }
                }}
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_of_a_choice_says_something() {
        assert_eq!(status_line(&Picked::Nothing), None);
        assert_eq!(status_line(&Picked::Reading("eustress-x.toml".into())).as_deref(), Some("Reading eustress-x.toml..."));
        assert_eq!(
            status_line(&Picked::Read { name: "eustress-x.toml".into(), key: "d75a9801...f707511a".into() }).as_deref(),
            Some("Read eustress-x.toml. Its public key is d75a9801...f707511a.")
        );
        // A refusal is shown as an alert, in the sentence it was given.
        assert_eq!(status_line(&Picked::Unreadable("We couldn't read this file: it has no private key.".into())), None);
    }
}
