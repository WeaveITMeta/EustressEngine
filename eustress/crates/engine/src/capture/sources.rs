//! SAM.gov and Grants.gov: request construction and response parsing.
//!
//! Both halves are pure. `*_request` builds an [`HttpRequest`] without sending
//! it, and `parse_*` turns a response body into [`Opportunity`] values without
//! touching the network. That split is what lets the whole ingest path be
//! tested against recorded payloads, and it is why the UI can show a user the
//! exact request before anything is enabled.
//!
//! ## Secrets
//!
//! SAM requires an API key. It is passed in as a value resolved from
//! [`SourceConfig::secret_ref`](eustress_data::source::SourceConfig) at the
//! call site and never stored here, never written to a manifest, and never
//! logged: every log path runs the URL through
//! [`redact_query`](eustress_data::source::http::redact_query) first.

use eustress_data::source::http::{HttpMethod, HttpRequest};
use serde_json::Value;

use super::model::{
    parse_grants_date, parse_money, parse_sam_date, NoticeSource, Opportunity, SetAside,
};
use super::CaptureError;

/// Default SAM.gov opportunities search endpoint.
pub const SAM_SEARCH_URL: &str = "https://api.sam.gov/opportunities/v2/search";
/// Default Grants.gov search endpoint.
pub const GRANTS_SEARCH_URL: &str = "https://api.grants.gov/v1/api/search2";
/// Grants.gov single-opportunity detail endpoint.
pub const GRANTS_FETCH_URL: &str = "https://api.grants.gov/v1/api/fetchOpportunity";

/// SAM caps a page at 1000 records and rejects anything larger.
pub const SAM_MAX_LIMIT: u32 = 1000;

/// Which ingest profile a Connector describes.
///
/// Carried in `SourceConfig.options["profile"]` rather than as a new
/// `SourceKind` variant, because both profiles ARE REST sources: they differ
/// in query shape and response schema, not in transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    SamOpportunities,
    GrantsSearch,
}

impl Profile {
    /// Stable wire name, as written into a Connector's `[attributes]`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SamOpportunities => "sam_opportunities",
            Self::GrantsSearch => "grants_search2",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sam_opportunities" | "sam" | "sam.gov" => Some(Self::SamOpportunities),
            "grants_search2" | "grants" | "grants.gov" => Some(Self::GrantsSearch),
            _ => None,
        }
    }

    pub fn source(self) -> NoticeSource {
        match self {
            Self::SamOpportunities => NoticeSource::Sam,
            Self::GrantsSearch => NoticeSource::Grants,
        }
    }

    /// The endpoint used when a Connector leaves `endpoint` blank.
    pub fn default_endpoint(self) -> &'static str {
        match self {
            Self::SamOpportunities => SAM_SEARCH_URL,
            Self::GrantsSearch => GRANTS_SEARCH_URL,
        }
    }
}

/// One page of a SAM.gov opportunities search.
#[derive(Debug, Clone)]
pub struct SamQuery {
    pub endpoint: String,
    /// `MM/dd/yyyy`. SAM rejects a window wider than one year.
    pub posted_from: String,
    pub posted_to: String,
    /// NAICS codes to filter on. SAM accepts one `ncode` per request, so more
    /// than one code means more than one request.
    pub naics: Option<String>,
    /// `typeOfSetAside` code, e.g. `SBA`, `SDVOSBC`.
    pub set_aside: Option<String>,
    /// Notice type code: `o` solicitation, `p` presolicitation, `r` sources
    /// sought, `k` combined synopsis.
    pub notice_type: Option<String>,
    pub state: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for SamQuery {
    fn default() -> Self {
        Self {
            endpoint: SAM_SEARCH_URL.to_string(),
            posted_from: String::new(),
            posted_to: String::new(),
            naics: None,
            set_aside: None,
            notice_type: None,
            state: None,
            limit: 100,
            offset: 0,
        }
    }
}

/// Build the SAM.gov search request for one page.
///
/// `api_key` is the resolved secret value, never a reference to one. It goes in
/// the query string because that is the only place SAM accepts it.
pub fn sam_request(q: &SamQuery, api_key: &str) -> Result<HttpRequest, CaptureError> {
    if api_key.trim().is_empty() {
        return Err(CaptureError::MissingSecret(
            "SAM.gov requires an API key; set the Connector's secret_ref to the name of an \
             environment variable holding it"
                .into(),
        ));
    }
    if q.posted_from.trim().is_empty() || q.posted_to.trim().is_empty() {
        return Err(CaptureError::BadQuery(
            "SAM.gov requires postedFrom and postedTo in MM/dd/yyyy".into(),
        ));
    }
    if q.limit == 0 || q.limit > SAM_MAX_LIMIT {
        return Err(CaptureError::BadQuery(format!(
            "SAM.gov limit must be 1..={SAM_MAX_LIMIT}, got {}",
            q.limit
        )));
    }

    let mut params: Vec<(String, String)> = vec![
        ("api_key".into(), api_key.to_string()),
        ("postedFrom".into(), q.posted_from.clone()),
        ("postedTo".into(), q.posted_to.clone()),
        ("limit".into(), q.limit.to_string()),
        ("offset".into(), q.offset.to_string()),
    ];
    if let Some(n) = q.naics.as_deref().filter(|s| !s.trim().is_empty()) {
        params.push(("ncode".into(), n.to_string()));
    }
    if let Some(s) = q.set_aside.as_deref().filter(|s| !s.trim().is_empty()) {
        params.push(("typeOfSetAside".into(), s.to_string()));
    }
    if let Some(t) = q.notice_type.as_deref().filter(|s| !s.trim().is_empty()) {
        params.push(("ptype".into(), t.to_string()));
    }
    if let Some(st) = q.state.as_deref().filter(|s| !s.trim().is_empty()) {
        params.push(("state".into(), st.to_string()));
    }

    let query: Vec<String> =
        params.iter().map(|(k, v)| format!("{}={}", k, urlencode(v))).collect();
    let url = format!("{}?{}", q.endpoint.trim_end_matches('?'), query.join("&"));
    Ok(HttpRequest::get(url).with_header("Accept", "application/json"))
}

/// One page of a Grants.gov search.
#[derive(Debug, Clone)]
pub struct GrantsQuery {
    pub endpoint: String,
    pub keyword: Option<String>,
    /// Assistance Listing (formerly CFDA) number, e.g. `10.310`.
    pub aln: Option<String>,
    /// Agency codes, e.g. `USDA-NIFA`.
    pub agencies: Vec<String>,
    /// `forecasted`, `posted`, `closed`, `archived`. Defaults to `posted`,
    /// because a closed opportunity cannot be responded to.
    pub statuses: Vec<String>,
    /// Applicant-type codes the firm qualifies as.
    pub eligibilities: Vec<String>,
    pub rows: u32,
    /// Zero-based index of the first record on this page.
    pub start_record: u32,
}

impl Default for GrantsQuery {
    fn default() -> Self {
        Self {
            endpoint: GRANTS_SEARCH_URL.to_string(),
            keyword: None,
            aln: None,
            agencies: Vec::new(),
            statuses: vec!["posted".into()],
            eligibilities: Vec::new(),
            rows: 100,
            start_record: 0,
        }
    }
}

/// Build the Grants.gov search request for one page. No key, no auth header.
pub fn grants_request(q: &GrantsQuery) -> Result<HttpRequest, CaptureError> {
    if q.rows == 0 {
        return Err(CaptureError::BadQuery("Grants.gov rows must be at least 1".into()));
    }
    let body = serde_json::json!({
        "keyword": q.keyword.clone().unwrap_or_default(),
        "oppNum": "",
        "aln": q.aln.clone().unwrap_or_default(),
        "agencies": q.agencies,
        "oppStatuses": q.statuses.join("|"),
        "eligibilities": q.eligibilities.join("|"),
        "fundingCategories": "",
        "rows": q.rows,
        "startRecordNum": q.start_record,
    });
    Ok(HttpRequest::post_json(q.endpoint.clone(), body.to_string()))
}

/// Request the full record for one Grants.gov opportunity.
///
/// The search response carries no description or award ceiling; those live only
/// behind this call. Screening on the search payload alone would silently treat
/// every grant as having no ceiling.
pub fn grants_detail_request(opportunity_id: &str) -> HttpRequest {
    let body = serde_json::json!({ "opportunityId": opportunity_id });
    HttpRequest::post_json(GRANTS_FETCH_URL, body.to_string())
}

/// Parse a SAM.gov search response.
///
/// Liberal in what it accepts and loud about what it drops: a record with no
/// `noticeId` has no stable identity, cannot be deduped, and cannot be filed on
/// disk, so it is skipped and counted rather than given a synthetic id that
/// would duplicate on the next sync.
pub fn parse_sam(body: &str) -> Result<ParsedPage, CaptureError> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| CaptureError::Parse(format!("SAM.gov: {e}")))?;

    // SAM reports its own errors with HTTP 200 in some deployments.
    if let Some(msg) = root.get("error").and_then(json_error_message) {
        return Err(CaptureError::Upstream(format!("SAM.gov: {msg}")));
    }

    let records = root
        .get("opportunitiesData")
        .and_then(Value::as_array)
        .ok_or_else(|| CaptureError::Parse("SAM.gov: no 'opportunitiesData' array".into()))?;

    let total = root.get("totalRecords").and_then(Value::as_u64).unwrap_or(records.len() as u64);
    let mut out = Vec::with_capacity(records.len());
    let mut skipped = 0usize;

    for r in records {
        let Some(id) = str_field(r, "noticeId").filter(|s| !s.is_empty()) else {
            skipped += 1;
            continue;
        };
        let mut o = Opportunity::empty(NoticeSource::Sam, id);
        o.title = str_field(r, "title").unwrap_or_default();
        // `fullParentPathName` is a dotted hierarchy, most-general first. The
        // last segment is the buying office, which is the useful one.
        o.agency = str_field(r, "fullParentPathName")
            .map(|p| p.rsplit('.').next().unwrap_or(&p).trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| str_field(r, "organizationType"))
            .unwrap_or_default();
        o.solicitation_number = str_field(r, "solicitationNumber").unwrap_or_default();
        o.classification = str_field(r, "naicsCode").unwrap_or_default();
        o.set_aside = SetAside::parse_sam(&str_field(r, "typeOfSetAside").unwrap_or_default());
        o.deadline = str_field(r, "responseDeadLine").as_deref().and_then(parse_sam_date);
        o.posted = str_field(r, "postedDate").as_deref().and_then(parse_sam_date);
        o.notice_type = str_field(r, "type").unwrap_or_default();
        o.url = str_field(r, "uiLink").unwrap_or_default();
        o.ceiling = r
            .get("award")
            .and_then(|a| a.get("amount"))
            .and_then(value_as_money);
        o.place_of_performance = r
            .get("placeOfPerformance")
            .and_then(|p| p.get("state"))
            .and_then(|s| s.get("code"))
            .and_then(Value::as_str)
            .map(str::to_string);
        o.attachments = r
            .get("resourceLinks")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();

        // SAM's `description` field is a URL to fetch the text, not the text.
        // Storing the link under its own key keeps that honest: an empty
        // `description` here means "not fetched", never "no requirements".
        if let Some(link) = str_field(r, "description").filter(|s| s.starts_with("http")) {
            o.extra.insert("description_link".into(), link);
        } else if let Some(text) = str_field(r, "description") {
            o.description = text;
        }
        if let Some(c) = str_field(r, "classificationCode") {
            o.extra.insert("psc".into(), c);
        }
        if let Some(d) = str_field(r, "typeOfSetAsideDescription") {
            o.extra.insert("set_aside_description".into(), d);
        }
        out.push(o);
    }

    Ok(ParsedPage { opportunities: out, total_records: total, skipped })
}

/// Parse a Grants.gov `search2` response.
pub fn parse_grants(body: &str) -> Result<ParsedPage, CaptureError> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| CaptureError::Parse(format!("Grants.gov: {e}")))?;

    // Grants.gov signals failure in the body with HTTP 200.
    let code = root.get("errorcode").and_then(Value::as_i64).unwrap_or(0);
    if code != 0 {
        let msg = root.get("msg").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(CaptureError::Upstream(format!("Grants.gov error {code}: {msg}")));
    }

    let data = root.get("data").unwrap_or(&root);
    let hits = data
        .get("oppHits")
        .and_then(Value::as_array)
        .ok_or_else(|| CaptureError::Parse("Grants.gov: no 'data.oppHits' array".into()))?;

    let total = data.get("hitCount").and_then(Value::as_u64).unwrap_or(hits.len() as u64);
    let mut out = Vec::with_capacity(hits.len());
    let mut skipped = 0usize;

    for r in hits {
        let Some(id) = str_field(r, "id").filter(|s| !s.is_empty()) else {
            skipped += 1;
            continue;
        };
        let mut o = Opportunity::empty(NoticeSource::Grants, &id);
        o.title = str_field(r, "title").unwrap_or_default();
        o.agency = str_field(r, "agency")
            .or_else(|| str_field(r, "agencyName"))
            .or_else(|| str_field(r, "agencyCode"))
            .unwrap_or_default();
        o.solicitation_number = str_field(r, "number").unwrap_or_default();
        // Assistance Listing number. `alnist` is the current key; `cfdaList`
        // is the legacy one and still appears on older records.
        o.classification = first_of_array(r, "alnist")
            .or_else(|| first_of_array(r, "cfdaList"))
            .unwrap_or_default();
        o.deadline = str_field(r, "closeDate").as_deref().and_then(parse_grants_date);
        o.posted = str_field(r, "openDate").as_deref().and_then(parse_grants_date);
        o.notice_type = str_field(r, "docType").unwrap_or_default();
        o.url = format!("https://www.grants.gov/search-results-detail/{id}");
        if let Some(s) = str_field(r, "oppStatus") {
            o.extra.insert("opp_status".into(), s);
        }
        if let Some(a) = str_field(r, "agencyCode") {
            o.extra.insert("agency_code".into(), a);
        }
        out.push(o);
    }

    Ok(ParsedPage { opportunities: out, total_records: total, skipped })
}

/// Fold a Grants.gov `fetchOpportunity` detail response into an opportunity.
///
/// Search results carry no description and no award ceiling, so an opportunity
/// screened without this has an unknown budget. Returns `false` when the
/// payload held nothing new, so a caller can tell "enriched" from "no detail
/// published" instead of guessing.
pub fn merge_grants_detail(o: &mut Opportunity, body: &str) -> Result<bool, CaptureError> {
    let root: Value = serde_json::from_str(body)
        .map_err(|e| CaptureError::Parse(format!("Grants.gov detail: {e}")))?;
    let syn = root.get("synopsis").or_else(|| root.get("data").and_then(|d| d.get("synopsis")));
    let Some(syn) = syn else { return Ok(false) };

    let mut changed = false;
    if let Some(d) = str_field(syn, "synopsisDesc").filter(|s| !s.is_empty()) {
        o.description = strip_html(&d);
        changed = true;
    }
    if let Some(c) = syn.get("awardCeiling").and_then(value_as_money) {
        o.ceiling = Some(c);
        changed = true;
    }
    if let Some(f) = syn.get("awardFloor").and_then(value_as_money) {
        o.extra.insert("award_floor".into(), f.to_string());
        changed = true;
    }
    if let Some(e) = str_field(syn, "applicantEligibilityDesc").filter(|s| !s.is_empty()) {
        o.extra.insert("eligibility".into(), strip_html(&e));
        changed = true;
    }
    Ok(changed)
}

/// One parsed page, with enough context to drive pagination honestly.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPage {
    pub opportunities: Vec<Opportunity>,
    /// What the source says the full result set holds.
    pub total_records: u64,
    /// Records dropped for want of a usable id. Non-zero is worth surfacing:
    /// a silent drop looks identical to a source with fewer results.
    pub skipped: usize,
}

impl ParsedPage {
    /// Whether another page exists after this one.
    ///
    /// Counts skipped records too: they occupied a slot in the source's own
    /// numbering, so ignoring them would stop paging early and quietly truncate
    /// the result set.
    pub fn has_more(&self, offset: u64) -> bool {
        let consumed = offset + self.opportunities.len() as u64 + self.skipped as u64;
        // An empty page always terminates, even if the source overstates its
        // own total. Trusting `total_records` alone would loop forever.
        !self.is_empty() && consumed < self.total_records
    }

    /// Whether the page carried no records at all.
    pub fn is_empty(&self) -> bool {
        self.opportunities.is_empty() && self.skipped == 0
    }
}

/// Percent-encode a query-string value.
///
/// Hand-rolled because the engine has no url crate and this needs only the
/// unreserved set from RFC 3986. Everything else is escaped, so an API key or a
/// NAICS filter containing a delimiter cannot forge an extra parameter.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Read a string field, trimmed, treating JSON `null` and `""` as absent.
fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "null")
        .map(str::to_string)
}

/// First element of a string array field.
fn first_of_array(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Money from either a JSON number or a formatted string.
fn value_as_money(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => parse_money(s),
        _ => None,
    }
}

/// Pull a message out of the several error envelopes SAM has shipped.
fn json_error_message(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(_) => v
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| v.get("code").and_then(Value::as_str))
            .map(str::to_string),
        _ => None,
    }
}

/// Strip HTML tags and decode the handful of entities the grant descriptions
/// actually use. Grants.gov returns marked-up text; requirement extraction runs
/// on words, and a `<br>` inside a sentence would split it into two.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            // A tag becomes a SPACE, not nothing. `</p><br>` between two
            // sentences would otherwise yield "partnerships.Second line.",
            // and requirement extraction only splits on a period followed by
            // whitespace, so the two would be read as one requirement.
            // The whitespace collapse at the end removes any surplus.
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sam_request_encodes_and_orders_parameters() {
        let q = SamQuery {
            posted_from: "08/01/2026".into(),
            posted_to: "08/23/2026".into(),
            naics: Some("541511".into()),
            set_aside: Some("SDVOSBC".into()),
            limit: 250,
            offset: 500,
            ..Default::default()
        };
        let r = sam_request(&q, "KEY123").expect("builds");
        assert_eq!(r.method, HttpMethod::Get);
        assert!(r.url.contains("postedFrom=08%2F01%2F2026"), "slashes are escaped: {}", r.url);
        assert!(r.url.contains("ncode=541511"));
        assert!(r.url.contains("typeOfSetAside=SDVOSBC"));
        assert!(r.url.contains("limit=250"));
        assert!(r.url.contains("offset=500"));
    }

    #[test]
    fn sam_request_refuses_to_build_without_a_key() {
        let q = SamQuery {
            posted_from: "08/01/2026".into(),
            posted_to: "08/23/2026".into(),
            ..Default::default()
        };
        assert!(matches!(sam_request(&q, "  "), Err(CaptureError::MissingSecret(_))));
    }

    #[test]
    fn sam_request_enforces_the_documented_page_cap() {
        let q = SamQuery {
            posted_from: "08/01/2026".into(),
            posted_to: "08/23/2026".into(),
            limit: 5000,
            ..Default::default()
        };
        assert!(matches!(sam_request(&q, "K"), Err(CaptureError::BadQuery(_))));
    }

    #[test]
    fn a_key_never_appears_in_a_redacted_url() {
        let q = SamQuery {
            posted_from: "08/01/2026".into(),
            posted_to: "08/23/2026".into(),
            ..Default::default()
        };
        let r = sam_request(&q, "SUPERSECRET").expect("builds");
        let safe = eustress_data::source::http::redact_query(&r.url);
        assert!(!safe.contains("SUPERSECRET"), "redacted url still leaked the key: {safe}");
    }

    const SAM_PAGE: &str = r#"{
      "totalRecords": 2,
      "opportunitiesData": [
        {
          "noticeId": "abc123",
          "title": "Network Modernization",
          "solicitationNumber": "W911-26-R-0001",
          "fullParentPathName": "DEPT OF DEFENSE.DEPT OF THE ARMY.ACC-APG",
          "postedDate": "2026-08-01",
          "type": "Solicitation",
          "typeOfSetAside": "SDVOSBC",
          "typeOfSetAsideDescription": "SDVOSB Set-Aside",
          "responseDeadLine": "2026-09-01T17:00:00-04:00",
          "naicsCode": "541511",
          "classificationCode": "D302",
          "uiLink": "https://sam.gov/opp/abc123/view",
          "description": "https://api.sam.gov/opps/v3/opportunities/abc123/description",
          "award": { "amount": "$1,500,000.00" },
          "placeOfPerformance": { "state": { "code": "AZ", "name": "Arizona" } },
          "resourceLinks": ["https://sam.gov/api/.../sow.pdf"]
        },
        { "title": "No id, must be skipped" }
      ]
    }"#;

    #[test]
    fn sam_page_parses_every_decision_bearing_field() {
        let page = parse_sam(SAM_PAGE).expect("parses");
        assert_eq!(page.opportunities.len(), 1);
        assert_eq!(page.skipped, 1, "the id-less record is dropped, not invented");

        let o = &page.opportunities[0];
        assert_eq!(o.notice_id, "abc123");
        assert_eq!(o.agency, "ACC-APG", "the last path segment is the buying office");
        assert_eq!(o.classification, "541511");
        assert_eq!(o.set_aside, SetAside::ServiceDisabledVeteran);
        assert_eq!(o.ceiling, Some(1_500_000));
        assert_eq!(o.place_of_performance.as_deref(), Some("AZ"));
        assert_eq!(o.attachments.len(), 1);
        assert_eq!(o.deadline.map(|d| d.to_rfc3339()).as_deref(), Some("2026-09-01T21:00:00+00:00"));
    }

    #[test]
    fn sam_description_link_is_not_mistaken_for_description_text() {
        let page = parse_sam(SAM_PAGE).expect("parses");
        let o = &page.opportunities[0];
        assert!(o.description.is_empty(), "an unfetched description must stay empty");
        assert!(
            o.extra.get("description_link").is_some(),
            "the link has to survive so the text can be fetched later"
        );
    }

    #[test]
    fn sam_reports_an_upstream_error_rather_than_returning_nothing() {
        let body = r#"{"error":{"code":"API_KEY_INVALID","message":"An invalid api_key"}}"#;
        assert!(matches!(parse_sam(body), Err(CaptureError::Upstream(_))));
    }

    const GRANTS_PAGE: &str = r#"{
      "errorcode": 0, "msg": "success",
      "data": {
        "hitCount": 1,
        "oppHits": [{
          "id": "358123",
          "number": "USDA-NIFA-CFP-010101",
          "title": "Community Food Projects",
          "agency": "National Institute of Food and Agriculture",
          "agencyCode": "USDA-NIFA",
          "openDate": "08/01/2026",
          "closeDate": "09/30/2026",
          "oppStatus": "posted",
          "docType": "synopsis",
          "alnist": ["10.225"]
        }]
      }
    }"#;

    #[test]
    fn grants_page_parses_and_builds_a_public_url() {
        let page = parse_grants(GRANTS_PAGE).expect("parses");
        assert_eq!(page.opportunities.len(), 1);
        let o = &page.opportunities[0];
        assert_eq!(o.notice_id, "358123");
        assert_eq!(o.classification, "10.225");
        assert_eq!(o.source, NoticeSource::Grants);
        assert!(o.url.ends_with("358123"));
        assert_eq!(o.deadline.map(|d| d.to_rfc3339()).as_deref(), Some("2026-09-30T23:59:59+00:00"));
    }

    #[test]
    fn grants_surfaces_its_in_body_error_code() {
        let body = r#"{"errorcode": 1, "msg": "Invalid request"}"#;
        match parse_grants(body) {
            Err(CaptureError::Upstream(m)) => assert!(m.contains("Invalid request")),
            other => panic!("expected an upstream error, got {other:?}"),
        }
    }

    #[test]
    fn grants_detail_supplies_the_ceiling_search_omits() {
        let mut o = Opportunity::empty(NoticeSource::Grants, "358123");
        assert_eq!(o.ceiling, None);
        let detail = r#"{"synopsis":{
            "synopsisDesc":"<p>Projects &amp; partnerships.</p><br>Second line.",
            "awardCeiling":"400000","awardFloor":25000
        }}"#;
        assert!(merge_grants_detail(&mut o, detail).expect("parses"));
        assert_eq!(o.ceiling, Some(400_000));
        assert_eq!(o.extra.get("award_floor").map(String::as_str), Some("25000"));
        assert_eq!(o.description, "Projects & partnerships. Second line.");
    }

    #[test]
    fn grants_detail_with_nothing_new_says_so() {
        let mut o = Opportunity::empty(NoticeSource::Grants, "1");
        assert!(!merge_grants_detail(&mut o, r#"{"other":1}"#).expect("parses"));
    }

    #[test]
    fn grants_request_joins_multi_value_filters_with_a_pipe() {
        let q = GrantsQuery {
            statuses: vec!["posted".into(), "forecasted".into()],
            eligibilities: vec!["25".into(), "12".into()],
            rows: 50,
            ..Default::default()
        };
        let r = grants_request(&q).expect("builds");
        let body = r.body_text().expect("has a body");
        assert!(body.contains("\"oppStatuses\":\"posted|forecasted\""), "{body}");
        assert!(body.contains("\"eligibilities\":\"25|12\""), "{body}");
        assert_eq!(r.method, HttpMethod::Post);
    }

    #[test]
    fn pagination_counts_skipped_records_and_stops_on_an_empty_page() {
        let page = parse_sam(SAM_PAGE).expect("parses");
        // 1 kept + 1 skipped == totalRecords of 2, so this page is the last.
        assert!(!page.has_more(0));

        let wide = ParsedPage { total_records: 500, ..page.clone() };
        assert!(wide.has_more(0), "2 of 500 consumed leaves more to fetch");
        assert!(!wide.has_more(498), "the last page terminates");

        let empty = ParsedPage { opportunities: vec![], total_records: 500, skipped: 0 };
        assert!(!empty.has_more(0), "an empty page must terminate even against a stale total");
    }

    #[test]
    fn profile_round_trips_its_wire_name() {
        for p in [Profile::SamOpportunities, Profile::GrantsSearch] {
            assert_eq!(Profile::parse(p.as_str()), Some(p));
        }
        assert_eq!(Profile::parse("nonsense"), None);
    }
}
