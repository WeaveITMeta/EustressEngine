// =============================================================================
// Eustress Web - Sign out: this device, or everywhere
// =============================================================================
// Two buttons, because they do two different things:
//
//   Sign out of this device   clears this browser's sign-in. Other browsers, Studio and Eustress Player stay signed in.
//   Sign out everywhere       `POST /api/auth/logout` moves the account's session floor past every sign-in already issued, so
//                             this browser, others, Studio and the Player all stop working until the next sign-in. Use it when
//                             someone else may have your sign-in.
//
// "Everywhere" is not reported as done unless the Worker said so: when it cannot be reached the page stays signed in and says
// why, so nobody leaves believing a stolen sign-in was ended. A sign-in the Worker already refuses (expired) is signed out locally,
// since there is nothing left to end. The menus draw their own buttons (their icons and classes differ); this module is what the
// buttons do.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient, ApiError};
use crate::state::AppState;

/// What pressing "Sign out everywhere" came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every sign-in is ended, or the one this page held was already refused.
    SignedOut,
    /// The Worker did not end them; say why.
    Failed(String),
}

pub fn outcome_of(answer: &Result<(), ApiError>) -> Outcome {
    match answer {
        Ok(()) | Err(ApiError::Unauthorized) => Outcome::SignedOut,
        Err(e) => {
            let why = api::failure_sentence(e);
            Outcome::Failed(format!("Could not sign out everywhere: {}. Nothing was changed.", why.trim_end_matches('.')))
        }
    }
}

pub const THIS_DEVICE_HELP: &str = "Ends this browser's sign-in. Studio, Eustress Player and other browsers stay signed in.";
pub const EVERYWHERE_HELP: &str = "Ends every sign-in of this account: this browser, other browsers, Studio and Eustress Player. Use it if someone else may have your sign-in.";

/// The two actions and the state a menu needs to draw them.
#[derive(Clone, Copy)]
pub struct SignOut {
    pub this_device: Callback<()>,
    pub everywhere: Callback<()>,
    /// Why "everywhere" did not happen, until the next try.
    pub error: RwSignal<Option<String>>,
    /// "Everywhere" is waiting for the Worker.
    pub busy: RwSignal<bool>,
}

/// `on_done` runs after a sign-out has happened: close the menu, go home.
pub fn use_sign_out(on_done: Callback<()>) -> SignOut {
    let app_state = expect_context::<AppState>();
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let local_state = app_state.clone();
    let this_device = Callback::new(move |_| {
        local_state.logout();
        on_done.run(());
    });

    let everywhere = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let state = app_state.clone();
        spawn_local(async move {
            let client = ApiClient::new(&state.api_url);
            let answer = api::auth::logout_everywhere(&client).await;
            match outcome_of(&answer) {
                Outcome::SignedOut => {
                    state.logout();
                    busy.set(false);
                    on_done.run(());
                }
                Outcome::Failed(why) => {
                    error.set(Some(why));
                    busy.set(false);
                }
            }
        });
    });

    SignOut { this_device, everywhere, error, busy }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everywhere_is_done_only_when_the_worker_said_so_or_the_sign_in_was_already_refused() {
        assert_eq!(outcome_of(&Ok(())), Outcome::SignedOut);
        assert_eq!(outcome_of(&Err(ApiError::Unauthorized)), Outcome::SignedOut, "an expired sign-in has nothing left to end");
    }

    #[test]
    fn every_other_answer_leaves_the_visitor_signed_in_and_says_so() {
        for e in [ApiError::Network("offline".to_string()), ApiError::NotFound, ApiError::Server { status: 503, message: "busy".to_string() }] {
            match outcome_of(&Err(e)) {
                Outcome::Failed(why) => {
                    assert!(why.starts_with("Could not sign out everywhere:"), "{why}");
                    assert!(why.ends_with("Nothing was changed."), "{why}");
                    assert!(!why.contains(".."), "one full stop between the reason and the next sentence: {why}");
                }
                Outcome::SignedOut => panic!("a failure must not read as signed out"),
            }
        }
    }

    #[test]
    fn a_fat_separator_stands_between_the_two_buttons_in_the_dropdown_and_the_drawer() {
        let nav = include_str!("nav.rs");
        for (menu, this_device, everywhere) in [
            ("dropdown", "sign_out_desktop.this_device.run", "sign_out_desktop.everywhere.run"),
            ("drawer", "sign_out_mobile.this_device.run", "sign_out_mobile.everywhere.run"),
        ] {
            let a = nav.find(this_device).unwrap_or_else(|| panic!("{menu}: no this-device button"));
            let b = nav.find(everywhere).unwrap_or_else(|| panic!("{menu}: no everywhere button"));
            assert!(nav[a..b].contains(r#"<div class="sign-out-divider" role="separator"></div>"#), "{menu}: no separator between the buttons");
        }
        let css = include_str!("../../style/main.css");
        assert!(css.contains(".sign-out-divider {\r\n    height: 4px;") || css.contains(".sign-out-divider {\n    height: 4px;"), "the bar is fat");
    }

    #[test]
    fn the_two_buttons_explain_themselves_differently() {
        assert_ne!(THIS_DEVICE_HELP, EVERYWHERE_HELP);
        assert!(EVERYWHERE_HELP.contains("Studio") && THIS_DEVICE_HELP.contains("stay signed in"));
    }
}
