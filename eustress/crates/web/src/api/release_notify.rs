// =============================================================================
// Eustress Web - "Tell me when it is released": the sign-up behind Get notified
// =============================================================================
// An account with a verified email can ask to be written to once when a product's first release is out. The Worker keeps one
// flag per account and product and reads the address from the account when it sends, so the site never sees or sends an address.
//
//   GET    /api/release-notify/{product}   { subscribed, email_verified }
//   PUT    /api/release-notify/{product}   subscribe; 409 when the account has no verified email
//   DELETE /api/release-notify/{product}   unsubscribe
//
// `product` is `player` or `studio`. A deployment without these routes answers 404, which reads as "not open yet", never as an error
// the visitor has to decode.
// =============================================================================

use serde::Deserialize;

use super::{failure_sentence, ApiClient, ApiError};

/// The products a visitor can ask to hear about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Product {
    Player,
    Studio,
}

impl Product {
    pub fn slug(self) -> &'static str {
        match self {
            Product::Player => "player",
            Product::Studio => "studio",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Product::Player => "Eustress Player",
            Product::Studio => "Eustress Studio",
        }
    }

    fn path(self) -> String {
        format!("/api/release-notify/{}", self.slug())
    }
}

/// What the Worker says about this account and one product.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct NotifyStatus {
    #[serde(default)]
    pub subscribed: bool,
    #[serde(default)]
    pub email_verified: bool,
}

/// Where the sign-up stands, for the button and the line under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notify {
    /// Not signed in: the way in is to sign in.
    SignedOut,
    /// Signed in, and the Worker has not answered yet.
    Checking,
    /// The account has no verified email, so there is nothing to write to.
    NeedsVerifiedEmail,
    Off,
    On,
    /// A change is on its way to the Worker.
    Saving,
    /// This deployment has no sign-up yet.
    Unavailable,
    Failed(String),
}

/// What an answer about the account's sign-up means.
pub fn state_of_status(answer: &Result<NotifyStatus, ApiError>) -> Notify {
    match answer {
        Ok(s) if s.subscribed => Notify::On,
        Ok(s) if !s.email_verified => Notify::NeedsVerifiedEmail,
        Ok(_) => Notify::Off,
        Err(ApiError::NotFound) => Notify::Unavailable,
        Err(e) => Notify::Failed(failure_sentence(e)),
    }
}

/// What an answer to "subscribe" or "unsubscribe" means. `wanted` is what was asked for.
pub fn state_of_change(wanted: bool, answer: &Result<(), ApiError>) -> Notify {
    match answer {
        Ok(()) if wanted => Notify::On,
        Ok(()) => Notify::Off,
        Err(ApiError::NotFound) => Notify::Unavailable,
        // The Worker refuses a sign-up without a verified email with 409.
        Err(ApiError::Server { status: 409, .. }) => Notify::NeedsVerifiedEmail,
        Err(e) => Notify::Failed(failure_sentence(e)),
    }
}

pub async fn status(client: &ApiClient, product: Product) -> Result<NotifyStatus, ApiError> {
    client.get::<NotifyStatus>(&product.path()).await
}

pub async fn subscribe(client: &ApiClient, product: Product) -> Result<(), ApiError> {
    client.put::<serde_json::Value, _>(&product.path(), &serde_json::json!({})).await.map(|_| ())
}

pub async fn unsubscribe(client: &ApiClient, product: Product) -> Result<(), ApiError> {
    client.delete::<serde_json::Value>(&product.path()).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_up_state_follows_what_the_worker_said() {
        assert_eq!(state_of_status(&Ok(NotifyStatus { subscribed: true, email_verified: true })), Notify::On);
        assert_eq!(state_of_status(&Ok(NotifyStatus { subscribed: false, email_verified: true })), Notify::Off);
        assert_eq!(state_of_status(&Ok(NotifyStatus { subscribed: false, email_verified: false })), Notify::NeedsVerifiedEmail);
        assert_eq!(state_of_status(&Err(ApiError::NotFound)), Notify::Unavailable, "a Worker without the route means not open yet");
        assert!(matches!(state_of_status(&Err(ApiError::Network("offline".to_string()))), Notify::Failed(_)));
    }

    #[test]
    fn a_change_is_reported_as_what_happened_and_a_refusal_for_a_missing_email_says_so() {
        assert_eq!(state_of_change(true, &Ok(())), Notify::On);
        assert_eq!(state_of_change(false, &Ok(())), Notify::Off);
        assert_eq!(state_of_change(true, &Err(ApiError::Server { status: 409, message: "x".to_string() })), Notify::NeedsVerifiedEmail);
        assert_eq!(state_of_change(true, &Err(ApiError::NotFound)), Notify::Unavailable);
        assert!(matches!(state_of_change(true, &Err(ApiError::Server { status: 503, message: "busy".to_string() })), Notify::Failed(_)));
    }

    #[test]
    fn only_the_two_products_have_a_path() {
        assert_eq!(Product::Player.path(), "/api/release-notify/player");
        assert_eq!(Product::Studio.path(), "/api/release-notify/studio");
        assert_ne!(Product::Player.name(), Product::Studio.name());
    }

    #[test]
    fn a_status_missing_a_field_reads_as_false_for_it() {
        let s: NotifyStatus = serde_json::from_str("{}").unwrap();
        assert_eq!(s, NotifyStatus { subscribed: false, email_verified: false });
    }
}
