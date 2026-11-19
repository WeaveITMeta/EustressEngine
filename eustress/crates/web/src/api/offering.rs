// =============================================================================
// Eustress Web - The offering (Regulation A), as the public may see it
// =============================================================================
// `GET /api/offering` (infrastructure/cloudflare/api/src/subscriptions.mjs, `publicView`): the one live offering, or none. An
// offering is live while it is testing the waters, open, or paused. Nothing on the site says a word about investing unless the
// Worker answers with a live offering: the footer's Invest link and the /invest page both read this, and a Worker without the
// route (a 404), an error, or no offering all count as "none". Before a filing, saying anything in public about a chance to
// invest can itself break the securities rules (docs/finance/REGULATION_A_TIER_2.md, Stage 0), so silence is the default.
// =============================================================================

use serde::Deserialize;

use super::ApiClient;

/// A document the offering names: where it is and the SHA-256 of its bytes.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct OfferingDoc {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

/// The live offering's public fields.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PublicOffering {
    #[serde(default)]
    pub offering_id: String,
    /// `testing`, `open` or `paused` (a live offering); anything else is treated as none.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub price_cents: u64,
    #[serde(default)]
    pub min_units: u64,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub circular: Option<OfferingDoc>,
    #[serde(default)]
    pub agreement: Option<OfferingDoc>,
    /// Set once the SEC has qualified the offering (open or paused).
    #[serde(default)]
    pub sec_file_number: Option<String>,
    #[serde(default)]
    pub qualified_on: Option<String>,
    #[serde(default)]
    pub units_available: Option<u64>,
    /// The testing-the-waters legend, while the offering is testing the waters.
    #[serde(default)]
    pub legend: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OfferingReply {
    #[serde(default)]
    offering: Option<PublicOffering>,
}

/// Whether a status is one in which the offering may be shown at all.
pub fn is_live(status: &str) -> bool {
    matches!(status, "testing" | "open" | "paused")
}

/// The live offering, or `None` for no offering, a Worker without the route, or any failure. Never an error: the answer to
/// "may the site talk about investing" is no unless the Worker plainly says yes.
pub async fn live_offering(client: &ApiClient) -> Option<PublicOffering> {
    client
        .get::<OfferingReply>("/api/offering")
        .await
        .ok()
        .and_then(|r| r.offering)
        .filter(|o| is_live(&o.status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_testing_open_and_paused_are_live() {
        for s in ["testing", "open", "paused"] {
            assert!(is_live(s), "{s}");
        }
        for s in ["draft", "closed", "", "OPEN", "anything"] {
            assert!(!is_live(s), "{s}");
        }
    }

    #[test]
    fn the_reply_reads_the_workers_public_view_and_no_offering_is_none() {
        let none: OfferingReply = serde_json::from_str(r#"{"ok":true,"offering":null}"#).unwrap();
        assert!(none.offering.is_none());
        let testing: OfferingReply = serde_json::from_str(
            r#"{"ok":true,"offering":{"offering_id":"units-2027","status":"testing","title":"Eustress units","price_cents":1000,
            "min_units":10,"states":["AZ"],"circular":null,"agreement":null,"sec_file_number":null,"qualified_on":null,
            "units_available":null,"legend":"No money or other consideration is being solicited."}}"#,
        )
        .unwrap();
        let o = testing.offering.unwrap();
        assert_eq!((o.status.as_str(), o.price_cents, o.states.len()), ("testing", 1000, 1));
        assert!(o.legend.unwrap().starts_with("No money"));
    }
}
