// =============================================================================
// Eustress Web - The "verify your identity" banner
// =============================================================================
// An account made before identity verification was required can sign in and
// read, and the Worker refuses everything else with `kyc_required`. The Worker
// marks such an account with `kyc_required: true` on sign-in and on
// GET /api/auth/me. This banner tells that account, on every page, and takes it
// to /verify-account. It can be hidden for the visit: the choice lives in this
// component, so a reload brings it back.
// =============================================================================

use leptos::prelude::*;

use crate::state::{AppState, AuthState};

/// The page that verifies an existing account.
pub const VERIFY_ACCOUNT_PATH: &str = "/verify-account";

pub const BANNER_TEXT: &str = "Verify your identity to keep using Eustress. You can still sign in and look around, but publishing, buying and changing settings need a verified account.";

pub const BANNER_BUTTON: &str = "Verify now";

/// Whether the banner shows: only for a signed-in account the Worker says must
/// verify, not once it was hidden for this visit, and not on the page that does
/// the verifying.
pub fn banner_visible(auth: &AuthState, dismissed: bool, path: &str) -> bool {
    !dismissed
        && path.trim_end_matches('/') != VERIFY_ACCOUNT_PATH
        && matches!(auth, AuthState::Authenticated(user) if user.kyc_required)
}

/// The banner itself, shown for the signed-in account that must verify.
#[component]
pub fn KycBanner() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let dismissed = RwSignal::new(false);
    let path = leptos_router::hooks::use_location().pathname;
    let show = Memo::new(move |_| banner_visible(&auth.get(), dismissed.get(), &path.get()));

    view! {
        <Show when=move || show.get()>
            <KycBannerBar on_dismiss=Callback::new(move |_: ()| dismissed.set(true)) />
        </Show>
    }
}

/// The bar: the sentence, the button to the verifying page, and a way to hide it for now.
#[component]
pub fn KycBannerBar(on_dismiss: Callback<()>) -> impl IntoView {
    view! {
        <div class="kyc-banner" role="status">
            <span class="kyc-banner-text">{BANNER_TEXT}</span>
            <a class="btn btn-primary kyc-banner-verify" href=VERIFY_ACCOUNT_PATH>{BANNER_BUTTON}</a>
            <button type="button" class="btn btn-secondary kyc-banner-dismiss" on:click=move |_| on_dismiss.run(())>"Not now"</button>
        </div>
    }
}
