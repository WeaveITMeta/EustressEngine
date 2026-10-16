// =============================================================================
// Eustress Web - TAKE IT DOWN: the wire contract and the rules around it
// =============================================================================
// Everything the three takedown pages know about the Worker lives here, as plain functions that run without a browser:
//
//   /take-it-down        POST /api/takedown/requests       (take_it_down.rs)
//   /cases/:reference    GET  /api/cases/{reference}       (case_status.rs, public)
//   /account/cases       GET  /api/account/cases           (account_cases.rs, signed in)
//                        POST /api/account/cases/{ref}/review
//
// Every wire name is in the CONTRACT section below and nowhere else. The pages only ever call the functions here, so a change
// to the contract is an edit to this file.
//
// Table of Contents:
// 1. CONTRACT: paths, field names, codes, limits
// 2. The request form: values, rules, the body
// 3. Answers to a request: receipt and errors
// 4. A case's public status
// 5. The account's cases: wire types, paging, review
// 6. Words
// 7. Tests
// =============================================================================

use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};

use crate::api::ApiError;

// -----------------------------------------------------------------------------
// 1. CONTRACT
// -----------------------------------------------------------------------------

// CONTRACT: confirmed with Jev 2026-10-08, live on the staging Worker (b05f2956, https://api-staging.eustress.dev).

/// POST: file a removal request. Public; the Authorization header rides along when the visitor is signed in.
pub const REQUESTS_PATH: &str = "/api/takedown/requests";
/// GET `{prefix}{reference}`: a case's public status. No session.
pub const CASE_PATH_PREFIX: &str = "/api/cases/";
/// GET `?limit=&cursor=`: the signed-in account's cases.
pub const ACCOUNT_CASES_PATH: &str = "/api/account/cases";
/// POST `{ACCOUNT_CASES_PATH}/{reference}{REVIEW_SUFFIX}`: ask for a review of a case.
pub const REVIEW_SUFFIX: &str = "/review";

/// The `relationship` values.
pub const REL_SELF: &str = "self";
pub const REL_REPRESENTATIVE: &str = "authorized_representative";

/// The `code` of an error answer.
pub const CODE_INVALID_REQUEST: &str = "invalid_request";
pub const CODE_RATE_LIMITED: &str = "rate_limited";
pub const CODE_UNAVAILABLE: &str = "unavailable";
pub const CODE_DECIDED: &str = "decided";
pub const CODE_ALREADY_ASKED: &str = "already_asked";

/// The review request's one field.
pub const STATEMENT_KEY: &str = "statement";

/// Limits the Worker enforces; the form checks the same ones before it sends.
pub const DESCRIPTION_MIN: usize = 10;
pub const DESCRIPTION_MAX: usize = 2000;
pub const LOCATIONS_MIN: usize = 1;
pub const LOCATIONS_MAX: usize = 20;
pub const STATEMENT_MIN: usize = 10;
pub const STATEMENT_MAX: usize = 2000;

/// How many cases one page of the account list asks for.
pub const PAGE_SIZE: u32 = 20;

/// A request's fields. `wire` is the name on the wire, the only place it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    Name,
    Signature,
    Relationship,
    DepictedName,
    Email,
    Locations,
    Description,
    GoodFaith,
    Minor,
}

/// The order the fields sit on the page, which is the order focus visits them in.
pub const FIELD_ORDER: [FormField; 9] = [
    FormField::Name,
    FormField::Signature,
    FormField::Relationship,
    FormField::DepictedName,
    FormField::Email,
    FormField::Locations,
    FormField::Description,
    FormField::GoodFaith,
    FormField::Minor,
];

impl FormField {
    pub fn wire(self) -> &'static str {
        match self {
            FormField::Name => "requester_name",
            FormField::Signature => "signature",
            FormField::Relationship => "relationship",
            FormField::DepictedName => "depicted_name",
            FormField::Email => "contact_email",
            FormField::Locations => "locations",
            FormField::Description => "description",
            FormField::GoodFaith => "good_faith",
            FormField::Minor => "depicts_minor",
        }
    }

    /// The field a name from an error answer points at. `locations[2]`, `locations.2` and `locations/2` all point at the list.
    pub fn from_wire(name: &str) -> Option<FormField> {
        let base = name.split(|c: char| c == '[' || c == '.' || c == '/').next().unwrap_or("").trim();
        FIELD_ORDER.into_iter().find(|f| f.wire() == base)
    }

    /// The id of the control that takes focus when this field has a problem.
    pub fn input_id(self) -> &'static str {
        match self {
            FormField::Name => "tid-name",
            FormField::Signature => "tid-signature",
            FormField::Relationship => "tid-rel-self",
            FormField::DepictedName => "tid-depicted",
            FormField::Email => "tid-email",
            FormField::Locations => "tid-locations",
            FormField::Description => "tid-description",
            FormField::GoodFaith => "tid-good-faith",
            FormField::Minor => "tid-minor",
        }
    }

    /// The id of the line that says what is wrong; the control's aria-describedby names it.
    pub fn error_id(self) -> &'static str {
        match self {
            FormField::Name => "tid-name-err",
            FormField::Signature => "tid-signature-err",
            FormField::Relationship => "tid-rel-err",
            FormField::DepictedName => "tid-depicted-err",
            FormField::Email => "tid-email-err",
            FormField::Locations => "tid-locations-err",
            FormField::Description => "tid-description-err",
            FormField::GoodFaith => "tid-good-faith-err",
            FormField::Minor => "tid-minor-err",
        }
    }
}

// -----------------------------------------------------------------------------
// 2. The request form
// -----------------------------------------------------------------------------

/// What the visitor typed, as the form holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormValues {
    pub name: String,
    pub signature: String,
    pub representative: bool,
    pub depicted_name: String,
    pub email: String,
    /// One link or listing id per line.
    pub locations_text: String,
    pub description: String,
    pub good_faith: bool,
    pub depicts_minor: bool,
}

/// The links and ids in the box: split on lines, spaces and commas, trimmed, without repeats.
pub fn parse_locations(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || c == ',') {
        let t = token.trim();
        if !t.is_empty() && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// An address shaped like name@example.com: one @, something on each side, a dot in the domain, no spaces.
pub fn is_email_shape(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() || s.len() > 254 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty() || local.len() > 64 {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2 && labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-'))
}

/// The signature is the name again: equal ignoring case and the spaces around it.
pub fn signature_matches(name: &str, signature: &str) -> bool {
    let n = name.trim().to_lowercase();
    !n.is_empty() && n == signature.trim().to_lowercase()
}

/// Every problem with the form, in page order. Empty means it can be sent.
pub fn validate(v: &FormValues) -> Vec<(FormField, String)> {
    let mut out: Vec<(FormField, String)> = Vec::new();
    if v.name.trim().is_empty() {
        out.push((FormField::Name, "Enter your full name.".to_string()));
    }
    if v.signature.trim().is_empty() {
        out.push((FormField::Signature, "Type your full name here to sign the request.".to_string()));
    } else if !signature_matches(&v.name, &v.signature) {
        out.push((FormField::Signature, "Your signature must be the same name as above. Type your full name again.".to_string()));
    }
    if v.representative && v.depicted_name.trim().is_empty() {
        out.push((FormField::DepictedName, "Enter the full name of the person shown.".to_string()));
    }
    if v.email.trim().is_empty() {
        out.push((FormField::Email, "Enter an email address we can reach you at.".to_string()));
    } else if !is_email_shape(&v.email) {
        out.push((FormField::Email, "That email address does not look right. Use the form name@example.com.".to_string()));
    }
    let locations = parse_locations(&v.locations_text);
    if locations.len() < LOCATIONS_MIN {
        out.push((FormField::Locations, "Add at least one link or listing ID. Put each one on its own line.".to_string()));
    } else if locations.len() > LOCATIONS_MAX {
        out.push((
            FormField::Locations,
            format!("Add {LOCATIONS_MAX} or fewer. You listed {}. Send the rest in a second request.", locations.len()),
        ));
    }
    let words = v.description.trim().chars().count();
    if words < DESCRIPTION_MIN {
        out.push((FormField::Description, format!("Write at least {DESCRIPTION_MIN} characters. You wrote {words}.")));
    } else if words > DESCRIPTION_MAX {
        out.push((FormField::Description, format!("Shorten this to {DESCRIPTION_MAX} characters or fewer. You wrote {words}.")));
    }
    if !v.good_faith {
        out.push((FormField::GoodFaith, "Tick the box to confirm the statement before you send.".to_string()));
    }
    out
}

/// The JSON the Worker takes. A representative's body names the person shown; a request about oneself leaves that out.
pub fn build_body(v: &FormValues) -> Value {
    let mut m = Map::new();
    m.insert(FormField::Name.wire().to_string(), json!(v.name.trim()));
    m.insert(FormField::Signature.wire().to_string(), json!(v.signature.trim()));
    m.insert(FormField::Relationship.wire().to_string(), json!(if v.representative { REL_REPRESENTATIVE } else { REL_SELF }));
    if v.representative {
        m.insert(FormField::DepictedName.wire().to_string(), json!(v.depicted_name.trim()));
    }
    m.insert(FormField::Email.wire().to_string(), json!(v.email.trim()));
    m.insert(FormField::Locations.wire().to_string(), json!(parse_locations(&v.locations_text)));
    m.insert(FormField::Description.wire().to_string(), json!(v.description.trim()));
    m.insert(FormField::GoodFaith.wire().to_string(), json!(v.good_faith));
    m.insert(FormField::Minor.wire().to_string(), json!(v.depicts_minor));
    Value::Object(m)
}

/// "2 fields need a fix" for the line above the form.
pub fn fix_summary(count: usize) -> String {
    if count == 1 {
        "1 field needs a fix. It is marked below.".to_string()
    } else {
        format!("{count} fields need a fix. Each one is marked below.")
    }
}

// -----------------------------------------------------------------------------
// 3. Answers to a request
// -----------------------------------------------------------------------------

/// One location and what became of it.
#[derive(Clone, Debug, PartialEq)]
pub struct LocationResult {
    pub location: String,
    /// "removed", "pending" or "not_found".
    pub result: String,
}

/// The 201 answer, read leniently: the request has been filed by the time this is read, so a missing piece must not turn it
/// into an error the visitor answers by filing it again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Receipt {
    pub reference: String,
    pub status: String,
    pub received_at: Option<String>,
    pub removed_at: Option<String>,
    pub locations: Vec<LocationResult>,
    pub status_url: Option<String>,
    pub message: Option<String>,
}

/// A string, or a number written as one; empty text is nothing.
fn text_of(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn parse_locations_result(v: &Value) -> Vec<LocationResult> {
    v.get("locations")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let location = text_of(item, "location")?;
                    Some(LocationResult { location, result: text_of(item, "result").unwrap_or_default() })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_receipt(v: &Value) -> Receipt {
    Receipt {
        reference: text_of(v, "reference").map(|r| normalize_reference(&r)).unwrap_or_default(),
        status: text_of(v, "status").unwrap_or_default(),
        received_at: text_of(v, "received_at"),
        removed_at: text_of(v, "removed_at"),
        locations: parse_locations_result(v),
        status_url: text_of(v, "status_url"),
        message: text_of(v, "message"),
    }
}

/// Where the result view sends the visitor to follow the case.
pub fn case_link(reference: &str) -> String {
    format!("/cases/{}", urlencoding::encode(reference))
}

// LEGAL REVIEW: what the page tells a requester happened to each item.
/// What became of one location, in words.
pub fn result_words(result: &str) -> &'static str {
    match result {
        "removed" => "Removed.",
        "pending" => "Removal is in progress.",
        "not_found" => "Not found. Check the link or ID, then send a new request for it.",
        _ => "Received.",
    }
}

/// Why a request did not go through, shaped for the form.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmitError {
    /// 400 invalid_request: a sentence, the problems by field, and problems that point at no field on this form.
    Invalid { message: String, fields: Vec<(FormField, String)>, other: Vec<String> },
    /// 429 rate_limited.
    RateLimited { message: String, retry_after_seconds: Option<u64> },
    /// 503 unavailable.
    Unavailable { message: String },
    /// Anything else: a network failure, an expired sign-in, an answer nobody expected.
    Other { message: String },
}

impl SubmitError {
    pub fn message(&self) -> &str {
        match self {
            SubmitError::Invalid { message, .. }
            | SubmitError::RateLimited { message, .. }
            | SubmitError::Unavailable { message }
            | SubmitError::Other { message } => message,
        }
    }
}

/// The problems an invalid_request answer lists. Missing fields read "Required."; a problem's own message wins over that.
pub fn field_problems(v: &Value) -> (Vec<(FormField, String)>, Vec<String>) {
    let mut fields: Vec<(FormField, String)> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    if let Some(missing) = v.get("missing").and_then(Value::as_array) {
        for name in missing.iter().filter_map(Value::as_str) {
            match FormField::from_wire(name) {
                Some(f) => {
                    if !fields.iter().any(|(x, _)| *x == f) {
                        fields.push((f, "Required.".to_string()));
                    }
                }
                None => other.push(format!("{name} is required.")),
            }
        }
    }
    if let Some(problems) = v.get("problems").and_then(Value::as_array) {
        for p in problems {
            let message = text_of(p, "message").unwrap_or_else(|| "This needs a fix.".to_string());
            match text_of(p, "field").and_then(|name| FormField::from_wire(&name)) {
                Some(f) => {
                    if let Some(slot) = fields.iter_mut().find(|(x, _)| *x == f) {
                        slot.1 = message;
                    } else {
                        fields.push((f, message));
                    }
                }
                None => other.push(message),
            }
        }
    }
    fields.sort_by_key(|(f, _)| FIELD_ORDER.iter().position(|x| x == f).unwrap_or(usize::MAX));
    (fields, other)
}

/// An answer that was not 2xx, read for the form. `error` is shown as the Worker wrote it; a sentence of our own stands in when
/// the answer has none.
pub fn parse_submit_error(e: &ApiError) -> SubmitError {
    match e {
        ApiError::Server { status, message } => {
            let v: Value = serde_json::from_str(message).unwrap_or(Value::Null);
            let code = v.get("code").and_then(Value::as_str).unwrap_or("");
            let said = text_of(&v, "error");
            if code == CODE_INVALID_REQUEST || (code.is_empty() && *status == 400) {
                let (fields, other) = field_problems(&v);
                SubmitError::Invalid {
                    message: said.unwrap_or_else(|| "Some fields need a fix. Check the marked fields, then send again.".to_string()),
                    fields,
                    other,
                }
            } else if code == CODE_RATE_LIMITED || (code.is_empty() && *status == 429) {
                SubmitError::RateLimited {
                    message: said.unwrap_or_else(|| "Too many requests from this connection. Wait a little, then send again.".to_string()),
                    retry_after_seconds: v.get("retry_after_seconds").and_then(Value::as_u64),
                }
            } else if code == CODE_UNAVAILABLE || (code.is_empty() && *status == 503) {
                SubmitError::Unavailable {
                    message: said.unwrap_or_else(|| "Removal requests are not available right now. Try again in a few minutes.".to_string()),
                }
            } else {
                SubmitError::Other { message: said.unwrap_or_else(|| format!("The server answered {status}. Try again in a minute.")) }
            }
        }
        ApiError::Unauthorized => SubmitError::Other {
            message: "Your saved sign-in is out of date. Sign out, then send the request again. You do not need an account.".to_string(),
        },
        ApiError::NotFound => SubmitError::Other {
            message: "Removal requests are not open on this site yet. Try again later.".to_string(),
        },
        ApiError::Network(_) | ApiError::Deserialize(_) => SubmitError::Other {
            message: "Could not reach Eustress. Check your connection and send the request again.".to_string(),
        },
    }
}

// -----------------------------------------------------------------------------
// 4. A case's public status
// -----------------------------------------------------------------------------

/// The characters a reference may use after EC: digits and letters without I, L, O and U.
fn is_reference_char(c: char) -> bool {
    matches!(c, '0'..='9' | 'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z')
}

/// A typed or linked reference, trimmed and in capitals.
pub fn normalize_reference(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// True for ^EC-[0-9A-HJKMNP-TV-Z]{4}(-[0-9A-HJKMNP-TV-Z]{4}){4}$: EC, then five groups of four. Capitals only; callers
/// normalize first.
pub fn is_valid_reference(s: &str) -> bool {
    let mut groups = s.split('-');
    if groups.next() != Some("EC") {
        return false;
    }
    let rest: Vec<&str> = groups.collect();
    rest.len() == 5 && rest.iter().all(|g| g.chars().count() == 4 && g.chars().all(is_reference_char))
}

pub fn case_path(reference: &str) -> String {
    format!("{CASE_PATH_PREFIX}{reference}")
}

/// A case as the public status route reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaseStatus {
    pub reference: String,
    pub kind: String,
    /// received, removed, under_review, decided or closed.
    pub status: String,
    pub received_at: Option<String>,
    pub removed_at: Option<String>,
    pub decided_at: Option<String>,
}

pub fn parse_case_status(v: &Value) -> Option<CaseStatus> {
    Some(CaseStatus {
        reference: normalize_reference(&text_of(v, "reference")?),
        kind: text_of(v, "kind").unwrap_or_default(),
        status: text_of(v, "status")?,
        received_at: text_of(v, "received_at"),
        removed_at: text_of(v, "removed_at"),
        decided_at: text_of(v, "decided_at"),
    })
}

/// What the status page shows.
#[derive(Clone, Debug, PartialEq)]
pub enum CaseView {
    Loading,
    /// The reference is not the right shape; nothing was asked of the Worker.
    Malformed,
    NotFound,
    Found(CaseStatus),
    Failed(String),
}

/// The status page's view for an answer from GET /api/cases/{reference}.
pub fn case_view_from(status: u16, body: &str) -> CaseView {
    match status {
        200..=299 => match serde_json::from_str::<Value>(body).ok().as_ref().and_then(parse_case_status) {
            Some(c) => CaseView::Found(c),
            None => CaseView::Failed("We got an answer we could not read. Try again in a minute.".to_string()),
        },
        400 => CaseView::Malformed,
        404 => CaseView::NotFound,
        429 => CaseView::Failed("Too many checks from this connection. Wait a minute, then try again.".to_string()),
        other => CaseView::Failed(format!("The server answered {other}. Try again in a minute.")),
    }
}

// LEGAL REVIEW: the sentence for each status is what a requester reads about their case. McKale must approve the wording.
/// A status in two parts: a short title and a sentence.
pub fn status_words(status: &str) -> (&'static str, &'static str) {
    match status {
        "received" => ("Received", "We have the request and are working on it."),
        "removed" => ("Removed", "The content named in this request has been removed."),
        "under_review" => ("Under review", "A person is looking at this case."),
        "decided" => ("Decided", "A decision has been made on this case."),
        "closed" => ("Closed", "This case is finished."),
        _ => ("Update pending", "This case has a status this page does not show yet. Check back later."),
    }
}

/// What kind of case, for the status page.
pub fn kind_words(kind: &str) -> &'static str {
    match kind {
        "takedown_request" => "Removal request",
        "takedown_notice" => "Removal of content",
        "listing_review" => "Listing review",
        _ => "Case",
    }
}

// -----------------------------------------------------------------------------
// 5. The account's cases
// -----------------------------------------------------------------------------

/// A time that may arrive as text or as a number; kept as text.
fn de_stamp<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ListingRef {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Reason {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub what_to_change: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Feedback {
    #[serde(default)]
    pub reasons: Vec<Reason>,
    #[serde(default)]
    pub articles: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub suggested_edit: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    /// The tags the review chose when the author gave none, and the sentence that says so.
    #[serde(default)]
    pub assigned_tags: Vec<String>,
    #[serde(default)]
    pub tags_chosen: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CaseLocation {
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub result: String,
}

/// One entry of the account's list. Kinds: listing_review, takedown_notice, takedown_request; the fields a kind does not use are
/// absent and read as empty.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CaseEntry {
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub listing: Option<ListingRef>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub headline: String,
    #[serde(default, deserialize_with = "de_stamp")]
    pub opened_at: Option<String>,
    #[serde(default, deserialize_with = "de_stamp")]
    pub decided_at: Option<String>,
    #[serde(default, deserialize_with = "de_stamp")]
    pub updated_at: Option<String>,
    #[serde(default, deserialize_with = "de_stamp")]
    pub removed_at: Option<String>,
    #[serde(default)]
    pub feedback: Option<Feedback>,
    #[serde(default)]
    pub can_appeal: bool,
    #[serde(default)]
    pub appeal: Option<Value>,
    #[serde(default)]
    pub can_request_review: bool,
    #[serde(default)]
    pub locations: Vec<CaseLocation>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CasesPage {
    #[serde(default)]
    pub cases: Vec<CaseEntry>,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// The path for one page of the list.
pub fn page_path(limit: u32, cursor: Option<&str>) -> String {
    let mut path = format!("{ACCOUNT_CASES_PATH}?limit={limit}");
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        path.push_str("&cursor=");
        path.push_str(&urlencoding::encode(c));
    }
    path
}

/// Add a page to the list, in the order the Worker gave (newest first), skipping a case already shown so a repeated
/// "Load more" cannot double a row. Returns the cursor for the next page: none when the Worker sent none, or sent the one just
/// used (which would loop).
pub fn merge_page(have: &mut Vec<CaseEntry>, page: CasesPage, used_cursor: Option<&str>) -> Option<String> {
    for entry in page.cases {
        if !have.iter().any(|c| c.reference == entry.reference) {
            have.push(entry);
        }
    }
    page.next_cursor.filter(|c| !c.is_empty() && Some(c.as_str()) != used_cursor)
}

pub fn review_path(reference: &str) -> String {
    format!("{ACCOUNT_CASES_PATH}/{}{REVIEW_SUFFIX}", urlencoding::encode(reference))
}

/// The wording problem with a review statement, or none.
pub fn validate_statement(text: &str) -> Option<String> {
    let n = text.trim().chars().count();
    if n < STATEMENT_MIN {
        Some(format!("Write at least {STATEMENT_MIN} characters. You wrote {n}."))
    } else if n > STATEMENT_MAX {
        Some(format!("Shorten this to {STATEMENT_MAX} characters or fewer. You wrote {n}."))
    } else {
        None
    }
}

pub fn review_body(statement: &str) -> Value {
    let mut m = Map::new();
    m.insert(STATEMENT_KEY.to_string(), json!(statement.trim()));
    Value::Object(m)
}

/// The 200 answer to a review request.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ReviewReply {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub status: String,
}

/// Why a review request did not go through, in words.
pub fn review_error_words(e: &ApiError) -> String {
    match e {
        ApiError::Server { status, message } => {
            let v: Value = serde_json::from_str(message).unwrap_or(Value::Null);
            match v.get("code").and_then(Value::as_str).unwrap_or("") {
                CODE_DECIDED => "This case has been decided, so it cannot be sent for review again.".to_string(),
                CODE_ALREADY_ASKED => "You already asked for a review of this case.".to_string(),
                _ => text_of(&v, "error").unwrap_or_else(|| format!("The server answered {status}. Try again in a minute.")),
            }
        }
        ApiError::Unauthorized => "Your sign-in has ended. Sign in again, then send the request.".to_string(),
        ApiError::NotFound => "We could not find that case on your account.".to_string(),
        ApiError::Network(_) | ApiError::Deserialize(_) => "Could not reach Eustress. Check your connection and try again.".to_string(),
    }
}

// -----------------------------------------------------------------------------
// 6. Words
// -----------------------------------------------------------------------------

/// `under_review` as "Under review".
pub fn humanize(raw: &str) -> String {
    let spaced = raw.trim().replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// An outcome in words. `content_restored` is the one the Worker names for a reversed removal.
pub fn outcome_words(outcome: &str) -> String {
    match outcome {
        "content_restored" => "Your content was restored.".to_string(),
        other => humanize(other),
    }
}

/// A time from the Worker (ISO text, or seconds or milliseconds since 1970) in the reader's zone.
pub fn stamp_words(raw: &str) -> String {
    let t = raw.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = t.parse::<i64>() {
            // Ten digits is seconds; more is milliseconds.
            return if t.len() <= 11 { crate::time_words::when_secs(n) } else { crate::time_words::when_ms(n as f64) };
        }
    }
    crate::time_words::when_iso(t)
}

/// Where a notice about a case sends the reader.
pub const CASES_LINK: &str = "/account/cases";

/// One plain sentence for a notification of kind "takedown", by its event: removed, removed_copy, restored or upheld.
pub fn takedown_notification_sentence(event: &str) -> Option<&'static str> {
    match event {
        "removed" => Some("Content named in a removal request was removed."),
        "removed_copy" => Some("A copy of removed content was removed too."),
        "restored" => Some("Content that had been removed was restored after a review."),
        "upheld" => Some("A review upheld the removal."),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// 7. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_REF: &str = "EC-7K2M-9QXD-4TPF-HJ3W-5XRZ";

    fn good() -> FormValues {
        FormValues {
            name: "Jane Doe".to_string(),
            signature: "Jane Doe".to_string(),
            representative: false,
            depicted_name: String::new(),
            email: "jane@example.com".to_string(),
            locations_text: "https://eustress.dev/simulation/abc".to_string(),
            description: "A private photo of me posted without my consent.".to_string(),
            good_faith: true,
            depicts_minor: false,
        }
    }

    fn fields_of(v: &FormValues) -> Vec<FormField> {
        validate(v).into_iter().map(|(f, _)| f).collect()
    }

    fn server(status: u16, body: &str) -> ApiError {
        ApiError::Server { status, message: body.to_string() }
    }

    #[test]
    fn a_complete_form_has_no_problems() {
        assert!(validate(&good()).is_empty());
    }

    #[test]
    fn the_name_is_required() {
        let mut v = good();
        v.name = "   ".to_string();
        assert_eq!(fields_of(&v), vec![FormField::Name, FormField::Signature]);
    }

    #[test]
    fn the_signature_must_be_the_name_ignoring_case_and_the_spaces_around_it() {
        let mut v = good();
        v.signature = "  jANE doE ".to_string();
        assert!(validate(&v).is_empty(), "case and edge spaces do not matter");
        v.signature = "Jane Smith".to_string();
        assert_eq!(fields_of(&v), vec![FormField::Signature]);
        v.signature = String::new();
        let found = validate(&v);
        assert_eq!(found.len(), 1);
        assert!(found[0].1.contains("sign"), "an empty signature says to sign: {}", found[0].1);
        // inner spaces still count
        assert!(!signature_matches("Jane Doe", "Jane  Doe"));
        assert!(!signature_matches("", ""), "two empty strings are not a signature");
    }

    #[test]
    fn a_representative_must_name_the_person_shown_and_someone_for_themselves_need_not() {
        let mut v = good();
        v.representative = true;
        assert_eq!(fields_of(&v), vec![FormField::DepictedName]);
        v.depicted_name = "Sam Roe".to_string();
        assert!(validate(&v).is_empty());
        v.representative = false;
        v.depicted_name = String::new();
        assert!(validate(&v).is_empty());
    }

    #[test]
    fn the_email_must_have_the_shape_of_an_address() {
        for ok in ["a@b.co", "jane.doe+x@mail.example.com", "  jane@example.com  "] {
            assert!(is_email_shape(ok), "{ok}");
        }
        for bad in ["", "jane", "jane@", "@example.com", "jane@example", "jane@@example.com", "ja ne@example.com", "jane@.com", "jane@example..com", "jane@-a.com"] {
            assert!(!is_email_shape(bad), "{bad}");
        }
        let mut v = good();
        v.email = "jane@example".to_string();
        assert_eq!(fields_of(&v), vec![FormField::Email]);
        v.email = String::new();
        assert_eq!(fields_of(&v), vec![FormField::Email]);
    }

    #[test]
    fn at_least_one_and_at_most_twenty_locations() {
        let mut v = good();
        v.locations_text = " \n , \n".to_string();
        assert_eq!(fields_of(&v), vec![FormField::Locations]);
        let twenty: Vec<String> = (0..20).map(|i| format!("https://eustress.dev/simulation/{i}")).collect();
        v.locations_text = twenty.join("\n");
        assert!(validate(&v).is_empty(), "20 is allowed");
        v.locations_text = format!("{}\nhttps://eustress.dev/simulation/21", twenty.join("\n"));
        let found = validate(&v);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, FormField::Locations);
        assert!(found[0].1.contains("20") && found[0].1.contains("21"), "{}", found[0].1);
    }

    #[test]
    fn locations_are_split_trimmed_and_not_repeated() {
        assert_eq!(parse_locations("a\n b ,c\n\na\r\n"), vec!["a", "b", "c"]);
    }

    #[test]
    fn the_description_is_ten_to_two_thousand_characters() {
        let mut v = good();
        v.description = "123456789".to_string();
        assert_eq!(fields_of(&v), vec![FormField::Description]);
        v.description = "1234567890".to_string();
        assert!(validate(&v).is_empty());
        v.description = "x".repeat(2000);
        assert!(validate(&v).is_empty());
        v.description = "x".repeat(2001);
        assert_eq!(fields_of(&v), vec![FormField::Description]);
        // characters, not bytes
        v.description = "\u{e9}".repeat(10);
        assert!(validate(&v).is_empty());
        // spaces around the text do not count
        v.description = format!("   {}   ", "y".repeat(9));
        assert_eq!(fields_of(&v), vec![FormField::Description]);
    }

    #[test]
    fn the_good_faith_box_must_be_ticked() {
        let mut v = good();
        v.good_faith = false;
        assert_eq!(fields_of(&v), vec![FormField::GoodFaith]);
    }

    #[test]
    fn problems_come_in_page_order() {
        let v = FormValues::default();
        let order: Vec<FormField> = validate(&v).into_iter().map(|(f, _)| f).collect();
        assert_eq!(
            order,
            vec![FormField::Name, FormField::Signature, FormField::Email, FormField::Locations, FormField::Description, FormField::GoodFaith]
        );
        assert_eq!(fix_summary(1), "1 field needs a fix. It is marked below.");
        assert_eq!(fix_summary(3), "3 fields need a fix. Each one is marked below.");
    }

    #[test]
    fn the_body_for_a_request_about_oneself() {
        let body = build_body(&good());
        assert_eq!(
            body,
            json!({
                "requester_name": "Jane Doe",
                "signature": "Jane Doe",
                "relationship": "self",
                "contact_email": "jane@example.com",
                "locations": ["https://eustress.dev/simulation/abc"],
                "description": "A private photo of me posted without my consent.",
                "good_faith": true,
                "depicts_minor": false
            })
        );
        assert!(body.get("depicted_name").is_none(), "a request about oneself names no one else");
    }

    #[test]
    fn the_body_for_a_representative_names_the_person_shown() {
        let mut v = good();
        v.representative = true;
        v.depicted_name = "  Sam Roe ".to_string();
        v.depicts_minor = true;
        v.locations_text = "https://eustress.dev/a\nlisting-77".to_string();
        let body = build_body(&v);
        assert_eq!(body["relationship"], "authorized_representative");
        assert_eq!(body["depicted_name"], "Sam Roe");
        assert_eq!(body["locations"], json!(["https://eustress.dev/a", "listing-77"]));
        assert_eq!(body["depicts_minor"], true);
        assert_eq!(body["good_faith"], true);
        assert!(body["locations"].is_array(), "locations go as an array of strings");
    }

    #[test]
    fn every_wire_name_is_written_once_and_the_body_uses_exactly_those() {
        let mut v = good();
        v.representative = true;
        v.depicted_name = "Sam Roe".to_string();
        let body = build_body(&v);
        let mut keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut want: Vec<&str> = FIELD_ORDER.iter().map(|f| f.wire()).collect();
        want.sort_unstable();
        assert_eq!(keys, want);
        for f in FIELD_ORDER {
            assert_eq!(FormField::from_wire(f.wire()), Some(f));
            assert!(f.input_id().starts_with("tid-") && f.error_id().ends_with("-err"));
        }
        assert_eq!(FormField::from_wire("locations[3]"), Some(FormField::Locations));
        assert_eq!(FormField::from_wire("locations.0"), Some(FormField::Locations));
        assert_eq!(FormField::from_wire("nope"), None);
    }

    #[test]
    fn an_invalid_request_answer_is_mapped_to_the_fields() {
        let body = r#"{"error":"Some fields need a fix.","code":"invalid_request","missing":["requester_name","contact_email"],"problems":[{"field":"contact_email","message":"Not an email address."},{"field":"locations[1]","message":"Not a Eustress link."},{"field":"mystery","message":"Odd."}]}"#;
        match parse_submit_error(&server(400, body)) {
            SubmitError::Invalid { message, fields, other } => {
                assert_eq!(message, "Some fields need a fix.");
                assert_eq!(
                    fields,
                    vec![
                        (FormField::Name, "Required.".to_string()),
                        (FormField::Email, "Not an email address.".to_string()),
                        (FormField::Locations, "Not a Eustress link.".to_string()),
                    ]
                );
                assert_eq!(other, vec!["Odd.".to_string()]);
            }
            other => panic!("wrong shape: {other:?}"),
        }
    }

    #[test]
    fn a_rate_limit_answer_shows_the_error_as_given_and_keeps_the_wait() {
        let body = r#"{"error":"Slow down. Try again in 30 seconds.","code":"rate_limited","retry_after_seconds":30}"#;
        assert_eq!(
            parse_submit_error(&server(429, body)),
            SubmitError::RateLimited { message: "Slow down. Try again in 30 seconds.".to_string(), retry_after_seconds: Some(30) }
        );
    }

    #[test]
    fn an_unavailable_answer_shows_the_error_as_given() {
        let body = r#"{"error":"Requests are paused for maintenance.","code":"unavailable"}"#;
        assert_eq!(parse_submit_error(&server(503, body)), SubmitError::Unavailable { message: "Requests are paused for maintenance.".to_string() });
    }

    #[test]
    fn an_answer_with_no_error_text_gets_a_sentence_of_our_own() {
        match parse_submit_error(&server(503, "")) {
            SubmitError::Unavailable { message } => assert!(message.contains("not available")),
            other => panic!("{other:?}"),
        }
        match parse_submit_error(&server(429, "<html>")) {
            SubmitError::RateLimited { message, retry_after_seconds } => {
                assert!(message.contains("Wait"));
                assert_eq!(retry_after_seconds, None);
            }
            other => panic!("{other:?}"),
        }
        match parse_submit_error(&server(400, "{}")) {
            SubmitError::Invalid { fields, other, .. } => assert!(fields.is_empty() && other.is_empty()),
            other => panic!("{other:?}"),
        }
        match parse_submit_error(&server(502, "bad gateway")) {
            SubmitError::Other { message } => assert!(message.contains("502")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_submit_error(&ApiError::Network("offline".to_string())), SubmitError::Other { .. }));
        assert!(parse_submit_error(&ApiError::Unauthorized).message().contains("Sign out"));
    }

    #[test]
    fn a_201_answer_is_read_for_the_reference_the_message_and_each_location() {
        let v = json!({
            "reference": "ec-7k2m-9qxd-4tpf-hj3w-5xrz",
            "status": "removed",
            "received_at": "2026-10-08T19:00:00Z",
            "removed_at": "2026-10-08T19:00:01Z",
            "locations": [
                { "location": "https://eustress.dev/simulation/abc", "result": "removed" },
                { "location": "listing-77", "result": "not_found" }
            ],
            "status_url": "https://eustress.dev/cases/EC-7K2M-9QXD-4TPF-HJ3W-5XRZ",
            "message": "We removed it."
        });
        let r = parse_receipt(&v);
        assert_eq!(r.reference, SAMPLE_REF);
        assert_eq!(r.message.as_deref(), Some("We removed it."));
        assert_eq!(r.locations.len(), 2);
        assert_eq!(r.locations[1], LocationResult { location: "listing-77".to_string(), result: "not_found".to_string() });
        assert_eq!(case_link(&r.reference), "/cases/EC-7K2M-9QXD-4TPF-HJ3W-5XRZ");
        assert_eq!(result_words("removed"), "Removed.");
        assert!(result_words("pending").contains("in progress"));
        assert!(result_words("not_found").contains("Not found"));
        // an answer with pieces missing is still a receipt
        let bare = parse_receipt(&json!({}));
        assert_eq!(bare, Receipt::default());
    }

    #[test]
    fn the_reference_shape_is_five_groups_of_four_without_i_l_o_u() {
        assert!(is_valid_reference(SAMPLE_REF));
        assert!(is_valid_reference("EC-0123-4567-89AB-CDEF-GHJK"));
        assert!(is_valid_reference("EC-MNPQ-RSTV-WXYZ-0000-1111"));
        // four groups was the old shape
        assert!(!is_valid_reference("EC-7K2M-9QXD-4TPF-HJ3W"));
        // six groups
        assert!(!is_valid_reference("EC-7K2M-9QXD-4TPF-HJ3W-5XRZ-AAAA"));
        for bad in ["EC-IK2M-9QXD-4TPF-HJ3W-5XRZ", "EC-LK2M-9QXD-4TPF-HJ3W-5XRZ", "EC-OK2M-9QXD-4TPF-HJ3W-5XRZ", "EC-UK2M-9QXD-4TPF-HJ3W-5XRZ"] {
            assert!(!is_valid_reference(bad), "{bad}");
        }
        for bad in ["", "EC", "EC-", "ec-7k2m-9qxd-4tpf-hj3w-5xrz", "XX-7K2M-9QXD-4TPF-HJ3W-5XRZ", "EC-7K2M-9QXD-4TPF-HJ3W-5XR", "EC-7K2M-9QXD-4TPF-HJ3W-5XRZZ", "EC7K2M-9QXD-4TPF-HJ3W-5XRZ", "EC-7K2M_9QXD-4TPF-HJ3W-5XRZ", " EC-7K2M-9QXD-4TPF-HJ3W-5XRZ"] {
            assert!(!is_valid_reference(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_reference_is_put_in_capitals_before_it_is_checked() {
        let typed = "  ec-7k2m-9qxd-4tpf-hj3w-5xrz \n";
        assert!(!is_valid_reference(typed));
        assert_eq!(normalize_reference(typed), SAMPLE_REF);
        assert!(is_valid_reference(&normalize_reference(typed)));
        assert_eq!(case_path(SAMPLE_REF), "/api/cases/EC-7K2M-9QXD-4TPF-HJ3W-5XRZ");
    }

    #[test]
    fn the_status_page_reads_each_answer_the_worker_gives() {
        let ok = r#"{"reference":"EC-7K2M-9QXD-4TPF-HJ3W-5XRZ","kind":"takedown_request","status":"removed","received_at":"2026-10-08T19:00:00Z","removed_at":"2026-10-08T19:00:01Z","decided_at":null}"#;
        match case_view_from(200, ok) {
            CaseView::Found(c) => {
                assert_eq!(c.reference, SAMPLE_REF);
                assert_eq!(c.status, "removed");
                assert_eq!(c.removed_at.as_deref(), Some("2026-10-08T19:00:01Z"));
                assert_eq!(c.decided_at, None);
            }
            other => panic!("{other:?}"),
        }
        // the bodies the staging Worker sent on 2026-10-08
        assert_eq!(case_view_from(404, r#"{"error":"No case has this reference.","code":"not_found"}"#), CaseView::NotFound);
        let bad = r#"{"error":"This is not a case reference. A reference looks like EC-7K2M-9QXD-4TPF-HJ3W-8NBR.","code":"bad_reference"}"#;
        assert_eq!(case_view_from(400, bad), CaseView::Malformed);
        assert!(matches!(case_view_from(200, "not json"), CaseView::Failed(_)));
        assert!(matches!(case_view_from(200, "{}"), CaseView::Failed(_)));
        assert!(matches!(case_view_from(500, ""), CaseView::Failed(m) if m.contains("500")));
        assert!(matches!(case_view_from(429, ""), CaseView::Failed(m) if m.contains("Wait")));
    }

    #[test]
    fn each_status_has_words_and_an_unknown_one_says_so() {
        for s in ["received", "removed", "under_review", "decided", "closed"] {
            let (title, sentence) = status_words(s);
            assert!(!title.is_empty() && sentence.ends_with('.'), "{s}");
            assert_ne!(title, "Update pending", "{s}");
        }
        assert_eq!(status_words("whatever").0, "Update pending");
        assert_eq!(kind_words("takedown_request"), "Removal request");
        assert_eq!(kind_words("zzz"), "Case");
    }

    fn entry(reference: &str) -> CaseEntry {
        CaseEntry { reference: reference.to_string(), ..CaseEntry::default() }
    }

    #[test]
    fn the_account_list_is_read_for_each_kind() {
        let body = r#"{
            "cases": [
              { "reference": "EC-AAAA-BBBB-CCCC-DDDD-EEEE", "kind": "listing_review", "listing": { "id": "l1", "name": "Tower" },
                "status": "decided", "outcome": "changes_needed", "headline": "Review of Tower", "opened_at": "2026-10-01T10:00:00Z",
                "decided_at": "2026-10-02T10:00:00Z", "updated_at": 1790000000,
                "feedback": { "reasons": [ { "title": "Unclear sign", "why": "It reads as a logo.", "what_to_change": "Remove the logo." } ],
                              "articles": ["B6"], "tests": ["T3"], "suggested_edit": "Rename it.", "note": "Thanks." },
                "can_appeal": true, "appeal": null },
              { "reference": "EC-FFFF-GGGG-HHHH-JJJJ-KKKK", "kind": "takedown_notice", "listing": { "id": "l2", "name": "Park" },
                "status": "decided", "outcome": "content_restored", "headline": "Park was removed", "opened_at": "2026-10-03T10:00:00Z",
                "removed_at": "2026-10-03T11:00:00Z", "feedback": { "reasons": [] }, "can_request_review": true },
              { "reference": "EC-MMMM-NNNN-PPPP-QQQQ-RRRR", "kind": "takedown_request", "listing": null, "status": "removed",
                "headline": "Your removal request", "opened_at": "2026-10-04T10:00:00Z", "removed_at": "2026-10-04T10:00:01Z",
                "locations": [ { "location": "listing-77", "result": "removed" } ], "feedback": null, "can_request_review": true }
            ],
            "total": 3, "next_cursor": "abc=="
        }"#;
        let page: CasesPage = serde_json::from_str(body).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.next_cursor.as_deref(), Some("abc=="));
        let review = &page.cases[0];
        assert_eq!(review.listing, Some(ListingRef { id: "l1".to_string(), name: "Tower".to_string() }));
        assert!(review.can_appeal && !review.can_request_review && review.appeal.is_none());
        assert_eq!(review.updated_at.as_deref(), Some("1790000000"), "a number is kept as text");
        let fb = review.feedback.as_ref().unwrap();
        assert_eq!((fb.articles.clone(), fb.tests.clone()), (vec!["B6".to_string()], vec!["T3".to_string()]));
        assert_eq!(fb.reasons[0].what_to_change, "Remove the logo.");
        let notice = &page.cases[1];
        assert_eq!(notice.outcome.as_deref(), Some("content_restored"));
        assert!(notice.can_request_review && notice.removed_at.is_some());
        let request = &page.cases[2];
        assert!(request.listing.is_none() && request.feedback.is_none());
        assert_eq!(request.locations, vec![CaseLocation { location: "listing-77".to_string(), result: "removed".to_string() }]);
    }

    #[test]
    fn an_empty_or_sparse_account_list_still_reads() {
        let page: CasesPage = serde_json::from_str(r#"{"cases":[],"total":0,"next_cursor":null}"#).unwrap();
        assert!(page.cases.is_empty() && page.next_cursor.is_none());
        let sparse: CasesPage = serde_json::from_str(r#"{"cases":[{"reference":"EC-AAAA-BBBB-CCCC-DDDD-EEEE"}]}"#).unwrap();
        assert_eq!(sparse.cases[0].status, "");
        assert_eq!(sparse.total, 0);
    }

    #[test]
    fn paging_asks_with_a_limit_and_an_encoded_cursor() {
        assert_eq!(page_path(20, None), "/api/account/cases?limit=20");
        assert_eq!(page_path(20, Some("")), "/api/account/cases?limit=20");
        assert_eq!(page_path(20, Some("a b+c/d==")), "/api/account/cases?limit=20&cursor=a%20b%2Bc%2Fd%3D%3D");
    }

    #[test]
    fn load_more_appends_in_the_workers_order_and_never_doubles_a_row() {
        let mut have = vec![entry("EC-1"), entry("EC-2")];
        let next = merge_page(
            &mut have,
            CasesPage { cases: vec![entry("EC-2"), entry("EC-3"), entry("EC-4")], total: 4, next_cursor: Some("c2".to_string()) },
            Some("c1"),
        );
        let refs: Vec<&str> = have.iter().map(|c| c.reference.as_str()).collect();
        assert_eq!(refs, vec!["EC-1", "EC-2", "EC-3", "EC-4"]);
        assert_eq!(next.as_deref(), Some("c2"));
        // the last page has no cursor
        let last = merge_page(&mut have, CasesPage { cases: vec![entry("EC-5")], total: 5, next_cursor: None }, Some("c2"));
        assert_eq!(last, None);
        assert_eq!(have.len(), 5);
        // a cursor that repeats the one just used would loop; stop instead
        let looped = merge_page(&mut have, CasesPage { cases: vec![], total: 5, next_cursor: Some("c2".to_string()) }, Some("c2"));
        assert_eq!(looped, None);
        assert_eq!(merge_page(&mut have, CasesPage { cases: vec![], total: 5, next_cursor: Some(String::new()) }, None), None);
    }

    #[test]
    fn a_review_statement_is_ten_to_two_thousand_characters_and_goes_as_the_statement_field() {
        assert!(validate_statement("too short").is_some());
        assert!(validate_statement("1234567890").is_none());
        assert!(validate_statement(&"x".repeat(2000)).is_none());
        assert!(validate_statement(&"x".repeat(2001)).is_some());
        assert!(validate_statement("   123456789   ").is_some(), "edge spaces do not count");
        assert_eq!(review_body("  Please look again.  "), json!({ "statement": "Please look again." }));
        assert_eq!(review_path(SAMPLE_REF), "/api/account/cases/EC-7K2M-9QXD-4TPF-HJ3W-5XRZ/review");
        let reply: ReviewReply = serde_json::from_str(r#"{"ok":true,"reference":"EC-1","status":"under_review"}"#).unwrap();
        assert!(reply.ok && reply.status == "under_review");
    }

    #[test]
    fn review_errors_are_said_in_words() {
        assert!(review_error_words(&server(409, r#"{"error":"x","code":"decided"}"#)).contains("decided"));
        assert!(review_error_words(&server(409, r#"{"error":"x","code":"already_asked"}"#)).contains("already asked"));
        assert_eq!(review_error_words(&server(400, r#"{"error":"Write more.","code":"invalid_request"}"#)), "Write more.");
        assert!(review_error_words(&server(500, "")).contains("500"));
        assert!(review_error_words(&ApiError::Unauthorized).contains("Sign in"));
        assert!(review_error_words(&ApiError::NotFound).contains("could not find"));
    }

    #[test]
    fn words_for_outcomes_stamps_and_notifications() {
        assert_eq!(humanize("under_review"), "Under review");
        assert_eq!(humanize(""), "");
        assert_eq!(outcome_words("content_restored"), "Your content was restored.");
        assert_eq!(outcome_words("changes_needed"), "Changes needed");
        assert_eq!(stamp_words("2026-10-08T19:00:00Z"), "Oct 8, 2026, 7:00 PM UTC");
        assert_eq!(stamp_words("1790000000"), stamp_words("1790000000000"), "seconds and milliseconds agree");
        assert_eq!(stamp_words("not a time"), "not a time");
        for e in ["removed", "removed_copy", "restored", "upheld"] {
            let s = takedown_notification_sentence(e).unwrap();
            assert!(s.ends_with('.'), "{e}");
        }
        assert_eq!(takedown_notification_sentence("other"), None);
        assert_eq!(CASES_LINK, "/account/cases");
    }

    #[test]
    fn nothing_here_uses_an_em_dash_or_an_en_dash() {
        let src = include_str!("takedown_wire.rs");
        assert!(!src.contains('\u{2014}') && !src.contains('\u{2013}'));
    }
}
