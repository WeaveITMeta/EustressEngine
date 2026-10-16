// =============================================================================
// Eustress Web - Telling the account which time zone it is read in
// =============================================================================
// An email cannot know the reader's time zone, and every time in it should be in
// theirs. So once a session, when the browser's zone differs from the one the
// Worker has for the account (or it has none), the page tells it. Nothing shows.
// A failure is dropped: the emails then name UTC, which is true.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient};
use crate::state::{AppState, AuthState};
use crate::time_words::browser_zone;

/// Whether the account's zone must be sent: the browser has one and the Worker's is missing or different.
pub fn zone_to_send(browser: Option<&str>, saved: Option<&str>) -> Option<String> {
    let browser = browser.filter(|z| !z.is_empty())?;
    (saved != Some(browser)).then(|| browser.to_string())
}

#[component]
pub fn TimeZoneSync() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    // One attempt per page load: a refusal is not asked again until the page is reloaded.
    let tried = StoredValue::new(false);
    Effect::new(move |_| {
        let AuthState::Authenticated(user) = auth.get() else { return };
        if tried.get_value() {
            return;
        }
        let Some(zone) = zone_to_send(browser_zone().as_deref(), user.time_zone.as_deref()) else { return };
        tried.set_value(true);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let _ = api::set_time_zone(&client, &zone).await;
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zone_is_sent_when_the_worker_has_none_or_another_and_never_when_it_is_the_same_or_the_browser_has_none() {
        assert_eq!(zone_to_send(Some("America/Phoenix"), None).as_deref(), Some("America/Phoenix"));
        assert_eq!(zone_to_send(Some("America/Phoenix"), Some("UTC")).as_deref(), Some("America/Phoenix"));
        assert_eq!(zone_to_send(Some("America/Phoenix"), Some("America/Phoenix")), None);
        assert_eq!(zone_to_send(None, Some("America/Phoenix")), None);
        assert_eq!(zone_to_send(None, None), None);
        assert_eq!(zone_to_send(Some(""), None), None);
    }
}
