//! Firebase source — Cloud Firestore over its REST API.
//!
//! Firestore is an HTTP endpoint that speaks JSON, so it needs no cloud SDK:
//! `GET {endpoint}/documents/{collection}` returns the documents, and this
//! module normalizes them into a [`Frame`].
//!
//! ## The part worth reading
//!
//! Firestore does not return plain JSON values. Every field is wrapped in a
//! one-key object naming its type:
//!
//! ```json
//! { "sensor": { "stringValue": "bay-a-inlet" },
//!   "sample_index": { "integerValue": "1" },
//!   "pressure_psi": { "doubleValue": 14.7 },
//!   "valve_open":   { "booleanValue": true } }
//! ```
//!
//! Note `integerValue`: Firestore sends 64-bit integers as JSON **strings**,
//! because a JSON number is a double and would lose precision past 2^53. Hand
//! that payload to a generic JSON→`Frame` reader and every column comes back as
//! text. So this module's real job is [`unwrap_typed_value`]: it strips the
//! envelope and rebuilds each document as a flat JSON object of *plain* values,
//! which the shared inference in [`crate::import::frame_from_jsonl`] then types
//! exactly as it types a REST payload or a JSONL file. `sample_index` lands as
//! [`ColumnDtype::I64`](crate::ColumnDtype::I64), `pressure_psi` as `F64`,
//! `valve_open` as `Bool`.
//!
//! Sharing that one inference path — rather than growing a second, subtly
//! different one here — is why a Firestore column and a CSV column of the same
//! numbers end up with the same dtype.
//!
//! ## Configuration
//!
//! | key | default | meaning |
//! |---|---|---|
//! | `endpoint` | — | project/database URL, e.g. `https://firestore.googleapis.com/v1/projects/demo/databases/(default)` |
//! | `collection` | — | collection path (`table` is accepted as an alias) |
//! | `limit` | 1000 | maximum documents in total |
//! | `page_size` | 300 | documents per request; further pages are followed via `nextPageToken` |
//! | `order_by` | none | Firestore `orderBy` expression, e.g. `recorded_at desc` |
//! | `include_document_id` | `true` | `false` drops the synthesized `document_id` column |
//! | `secret_ref` | none | env var holding an OAuth bearer token (omit for the emulator) |
//!
//! ## Transport
//!
//! HTTP goes through the same injectable seam as the REST provider
//! ([`super::rest::HttpTransport`]), so this provider gets real TLS from the
//! `http` feature's `ureq` client and tests get real sockets against a
//! throwaway loopback server — with no network, no credentials, and no live
//! Firestore.

// This provider parses JSON and shares the HTTP seam in `rest`, both of
// which live behind `import`. Without this gate the leaf fails to build
// with default features, which is the D2 purity contract in lib.rs.
#![cfg(feature = "import")]

use std::sync::Arc;

use serde_json::{Map, Value};

use super::rest::{HttpMethod, default_transport, frame_from_json_value, parse_json, HttpRequest, HttpResponse, HttpTransport,};
use super::{validate_config, ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// Documents in total when `limit` is not configured. This only stops an
/// unbounded pull; Firestore imposes no such cap of its own.
const DEFAULT_LIMIT: usize = 1000;

/// Documents per request when `page_size` is not configured.
const DEFAULT_PAGE_SIZE: usize = 300;

/// Upper bound on how many pages one `fetch` will follow, so a server that
/// keeps handing back tokens can never spin forever.
const MAX_PAGES: usize = 1000;

/// Name of the column synthesized from each document's resource path.
const DOCUMENT_ID_COLUMN: &str = "document_id";

// ── URL escaping ─────────────────────────────────────────────────────────────
//
// Integration note: `encode_path` / `encode_query` are `pub(super)` because the
// Supabase provider needs the identical rules. They belong in a shared
// `source/url.rs` next to the shared transport; they live here only because
// this workstream owns `firebase.rs` and `supabase.rs` and nothing else.

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn percent_encode(s: &str, extra_safe: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if is_unreserved(b) || extra_safe.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Escape a path segment, keeping `/` so a nested collection path survives.
pub(super) fn encode_path(s: &str) -> String {
    percent_encode(s, b"/")
}

/// Escape a query-string value, keeping the punctuation Firestore and PostgREST
/// expressions legitimately use (`select=a,b`, `order=t.desc`, `select=*`,
/// `id=eq.3`).
pub(super) fn encode_query(s: &str) -> String {
    percent_encode(s, b",()*:!.")
}

// ── The provider ─────────────────────────────────────────────────────────────

/// A Cloud Firestore collection, read over the Firestore REST API.
pub struct FirebaseSource {
    config: SourceConfig,
    transport: Arc<dyn HttpTransport>,
}

impl std::fmt::Debug for FirebaseSource {
    /// Hand-written because the transport is a trait object. Renders only the
    /// config, which by construction holds the *name* of a credential and never
    /// its value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirebaseSource")
            .field("endpoint", &self.config.endpoint)
            .field("collection", &self.collection())
            .field("secret_ref", &self.config.secret_ref)
            .finish_non_exhaustive()
    }
}

impl FirebaseSource {
    /// Build a source over the workspace's default HTTP transport.
    ///
    /// Fails on any config the provider cannot honor, so a bad config is caught
    /// at construction rather than on the first fetch.
    pub fn new(config: SourceConfig) -> Result<Self> {
        Self::with_transport(config, default_transport())
    }

    /// Build a source over a caller-supplied transport — the seam tests use to
    /// drive the real provider against a loopback stub.
    pub fn with_transport(config: SourceConfig, transport: Arc<dyn HttpTransport>) -> Result<Self> {
        if config.kind != SourceKind::Firebase {
            return Err(DataError::Schema(format!(
                "FirebaseSource cannot serve a {} config",
                config.kind.as_str()
            )));
        }
        validate_config(&config)?;
        Ok(Self { config, transport })
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The collection path. `validate_config` guarantees one of these is set.
    fn collection(&self) -> &str {
        self.config
            .option("collection")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.config.option("table"))
            .unwrap_or("")
            .trim()
    }

    fn usize_option(&self, key: &str) -> Option<usize> {
        self.config.option(key).and_then(|s| s.trim().parse::<usize>().ok()).filter(|n| *n > 0)
    }

    /// Total documents one `fetch` will return.
    fn limit(&self) -> usize {
        self.usize_option("limit").unwrap_or(DEFAULT_LIMIT)
    }

    /// Documents per request, never larger than the total limit — a small
    /// `limit` must not make Firestore assemble a big page that is discarded.
    fn page_size(&self) -> usize {
        self.usize_option("page_size").unwrap_or(DEFAULT_PAGE_SIZE).min(self.limit())
    }

    fn include_document_id(&self) -> bool {
        !matches!(
            self.config
                .option("include_document_id")
                .map(|s| s.trim().to_ascii_lowercase())
                .as_deref(),
            Some("false" | "0" | "no")
        )
    }

    /// `{endpoint}/documents/{collection}?pageSize=…`.
    ///
    /// `/documents` is appended when the caller has not already written it, so
    /// both spellings of the endpoint work.
    fn request_url(&self, page_size: usize, page_token: Option<&str>) -> String {
        let base = self.config.endpoint.trim().trim_end_matches('/');
        let mut url = base.to_string();
        if !base.ends_with("/documents") {
            url.push_str("/documents");
        }
        url.push('/');
        url.push_str(&encode_path(self.collection()));
        url.push_str(&format!("?pageSize={page_size}"));
        if let Some(order) = self.config.option("order_by").map(str::trim).filter(|s| !s.is_empty())
        {
            url.push_str(&format!("&orderBy={}", encode_query(order)));
        }
        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", encode_query(token)));
        }
        url
    }

    /// Build one request.
    ///
    /// A `secret_ref` naming an env var that is not set is an error, never a
    /// silently unauthenticated request against a collection the caller
    /// believes is protected.
    fn request(&self, url: String) -> Result<HttpRequest> {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if let Some(name) = &self.config.secret_ref {
            let token = self.config.resolve_secret().ok_or_else(|| {
                DataError::Schema(format!(
                    "Firebase source names secret_ref '{name}' but that environment variable is \
                     not set; export it, or clear secret_ref to query an unauthenticated endpoint"
                ))
            })?;
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        Ok(HttpRequest { method: HttpMethod::Get, url, headers, body: None })
    }

    /// Issue one GET, turning a non-2xx into an error that carries Firestore's
    /// own message — far more useful than the bare status.
    fn get(&self, url: String) -> Result<Value> {
        let response = self.transport.send(&self.request(url)?)?;
        if !is_success(&response) {
            return Err(DataError::Schema(format!(
                "Firestore returned HTTP {} for collection '{}': {}",
                response.status,
                self.collection(),
                error_detail(&response.text())
            )));
        }
        parse_json(&response.text())
    }
}

impl DataSource for FirebaseSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Firebase
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        // One document proves the project, the credentials, and the collection
        // all line up, and honors the trait's "MUST NOT fetch a full dataset".
        let request = self.request(self.request_url(1, None))?;
        match self.transport.send(&request) {
            Ok(response) if is_success(&response) => {
                let count = parse_json(&response.text())
                    .map(|payload| documents_of(&payload).len())
                    .unwrap_or(0);
                Ok(ConnectionStatus::ok(format!(
                    "Firestore collection '{}' reachable ({count} document{} in the probe page)",
                    self.collection(),
                    if count == 1 { "" } else { "s" }
                )))
            }
            Ok(response) => Ok(ConnectionStatus::failed(format!(
                "Firestore returned HTTP {}: {}",
                response.status,
                error_detail(&response.text())
            ))),
            Err(e) => Ok(ConnectionStatus::failed(e.to_string())),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        let limit = self.limit();
        let page_size = self.page_size();
        let mut documents: Vec<Value> = Vec::new();
        let mut token: Option<String> = None;
        // Tokens already requested. Firestore stops sending `nextPageToken` at
        // the end of a collection; a server that repeats one would otherwise
        // loop forever, so a repeat is read as "no further pages".
        let mut requested: Vec<String> = Vec::new();

        for _ in 0..MAX_PAGES {
            let remaining = limit - documents.len();
            let payload = self.get(self.request_url(page_size.min(remaining), token.as_deref()))?;
            documents.extend(documents_of(&payload).iter().cloned());
            if documents.len() >= limit {
                documents.truncate(limit);
                break;
            }
            match next_page_token(&payload) {
                Some(t) if !requested.contains(&t) => {
                    requested.push(t.clone());
                    token = Some(t);
                }
                _ => break,
            }
        }

        let include_id = self.include_document_id();
        let rows = documents
            .iter()
            .map(|d| flatten_document(d, include_id))
            .collect::<Result<Vec<Value>>>()?;
        frame_from_json_value(&Value::Array(rows), None)
    }
}

/// Whether a transport response carries a 2xx status.
fn is_success(response: &HttpResponse) -> bool {
    (200..300).contains(&response.status)
}

/// The documents in a Firestore response.
///
/// `documents.list` wraps them in `{"documents": [...]}` and omits the key
/// entirely for an empty collection; `:runQuery` returns a bare array. Accept
/// all three, so an empty collection is an empty frame rather than an error.
fn documents_of(payload: &Value) -> &[Value] {
    if let Some(items) = payload.as_array() {
        return items;
    }
    payload.get("documents").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// The paging cursor, if the server sent a non-empty one.
fn next_page_token(payload: &Value) -> Option<String> {
    payload
        .get("nextPageToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Pull the human-readable message out of a Google API error envelope
/// (`{"error": {"message": …, "status": …}}`), falling back to the raw body so
/// nothing is ever swallowed.
fn error_detail(body: &str) -> String {
    let trimmed = body.trim();
    if let Ok(payload) = parse_json(trimmed) {
        if let Some(error) = payload.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            let status = error.get("status").and_then(Value::as_str).unwrap_or("");
            if !message.is_empty() {
                return if status.is_empty() {
                    message.to_string()
                } else {
                    format!("{status}: {message}")
                };
            }
        }
    }
    if trimmed.is_empty() {
        "(empty response body)".to_string()
    } else {
        trimmed.chars().take(400).collect()
    }
}

/// Unwrap one Firestore typed value into a plain JSON value.
///
/// This is the whole point of the module. Firestore never sends a bare scalar:
/// each field is `{"<type>Value": <payload>}`, and `integerValue` arrives as a
/// **string** so 64-bit precision survives JSON. Unwrapping here is what lets
/// the shared inference type `sample_index` as `I64` rather than text.
///
/// Anything that is not a one-key envelope is passed through untouched, and
/// composite values (`arrayValue`, `mapValue`, `geoPointValue`) keep their
/// structure — a [`Frame`] column is scalar, so the shared inference renders
/// them as compact JSON in a string column.
///
/// One documented limitation: `NaN` and `±Infinity` are not representable as
/// JSON numbers, so Firestore sends them as strings and they stay strings here.
/// A column mixing them with real doubles therefore reads as text — which keeps
/// the value visible, rather than nulling it to protect the column's dtype.
fn unwrap_typed_value(value: &Value) -> Value {
    let Some(envelope) = value.as_object() else { return value.clone() };
    // A genuine envelope has exactly one member; anything else is not one.
    if envelope.len() != 1 {
        return value.clone();
    }
    let Some((tag, payload)) = envelope.iter().next() else { return Value::Null };

    match tag.as_str() {
        "nullValue" => Value::Null,
        "booleanValue" => match payload {
            Value::Bool(_) => payload.clone(),
            // Tolerate the string spelling some emulators emit.
            Value::String(s) if s.eq_ignore_ascii_case("true") => Value::Bool(true),
            Value::String(s) if s.eq_ignore_ascii_case("false") => Value::Bool(false),
            other => other.clone(),
        },
        "integerValue" => match payload {
            // The documented shape: a decimal string, so that an i64 beyond
            // f64 precision survives the wire intact.
            Value::String(s) => match s.trim().parse::<i64>() {
                Ok(i) => Value::Number(i.into()),
                Err(_) => payload.clone(),
            },
            other => other.clone(),
        },
        "doubleValue" => match payload {
            Value::String(s) => s
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                // NaN / ±Infinity have no JSON number form; keep the label.
                .unwrap_or_else(|| payload.clone()),
            other => other.clone(),
        },
        "stringValue" | "timestampValue" | "bytesValue" | "referenceValue" => payload.clone(),
        // arrayValue / mapValue / geoPointValue, and whatever Google adds next.
        _ => payload.clone(),
    }
}

/// The trailing segment of a Firestore resource path
/// (`projects/p/databases/(default)/documents/readings/r-0001` → `r-0001`).
fn document_id(document: &Value) -> Option<&str> {
    let name = document.get("name").and_then(Value::as_str)?;
    match name.rsplit('/').next() {
        Some(id) if !id.is_empty() => Some(id),
        _ => Some(name),
    }
}

/// Rebuild one Firestore document as a flat JSON object of plain values — the
/// shape the shared JSON → [`Frame`] inference consumes.
///
/// The synthesized `document_id` is written first, so a collection that really
/// has a field of that name overwrites it rather than colliding.
fn flatten_document(document: &Value, include_document_id: bool) -> Result<Value> {
    let mut row = Map::new();
    if include_document_id {
        if let Some(id) = document_id(document) {
            row.insert(DOCUMENT_ID_COLUMN.to_string(), Value::String(id.to_string()));
        }
    }
    if let Some(fields) = document.get("fields") {
        let entries = fields.as_object().ok_or_else(|| {
            DataError::Schema(
                "Firestore document has a 'fields' member that is not an object".to_string(),
            )
        })?;
        for (name, value) in entries {
            row.insert(name.clone(), unwrap_typed_value(value));
        }
    }
    Ok(Value::Object(row))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — hermetic: no network, no credentials, no live Firestore.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::rest::testing::{SocketTransport, StubServer};
    use super::*;
    use crate::{ColumnData, ColumnDtype};
    use std::sync::Mutex;

    /// A transport that answers from a queue and records every request — the
    /// only way to exercise paging, where the second reply must differ from the
    /// first. Sockets are covered separately by the [`StubServer`] tests.
    struct QueuedTransport {
        replies: Mutex<Vec<HttpResponse>>,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl QueuedTransport {
        fn new(replies: Vec<(u16, &str)>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .rev()
                        .map(|(status, body)| HttpResponse::new(status, body))
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            })
        }

        fn urls(&self) -> Vec<String> {
            self.seen.lock().unwrap().iter().map(|r| r.url.clone()).collect()
        }

        fn header(&self, index: usize, name: &str) -> Option<String> {
            self.seen.lock().unwrap().get(index).and_then(|r| {
                r.headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone())
            })
        }

        fn request_count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    impl HttpTransport for QueuedTransport {
        fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
            self.seen.lock().unwrap().push(req.clone());
            let mut replies = self.replies.lock().unwrap();
            // The last reply repeats once the queue is drained, so a test that
            // over-fetches fails on its assertion rather than on plumbing.
            Ok(if replies.len() > 1 {
                replies.pop().expect("non-empty queue")
            } else {
                replies.last().cloned().expect("at least one reply")
            })
        }
    }

    /// The Firestore payload from the shared fixture, inlined so this unit test
    /// does not reach outside the crate source: typed fields, a `nullValue`, a
    /// document missing a field, and a paging token.
    const READINGS: &str = r#"{
      "documents": [
        {
          "name": "projects/eustress-demo/databases/(default)/documents/readings/r-0001",
          "fields": {
            "sensor":       { "stringValue": "bay-a-inlet" },
            "pressure_psi": { "doubleValue": 14.7 },
            "sample_index": { "integerValue": "1" },
            "valve_open":   { "booleanValue": true },
            "recorded_at":  { "timestampValue": "2026-08-14T17:04:11.882Z" },
            "operator":     { "stringValue": "alvarez" },
            "tags":         { "arrayValue": { "values": [ { "stringValue": "a" } ] } }
          },
          "createTime": "2026-08-14T17:04:12.001Z"
        },
        {
          "name": "projects/eustress-demo/databases/(default)/documents/readings/r-0003",
          "fields": {
            "sensor":       { "stringValue": "bay-a-inlet" },
            "pressure_psi": { "nullValue": null },
            "sample_index": { "integerValue": "-9007199254740993" },
            "valve_open":   { "booleanValue": false },
            "recorded_at":  { "timestampValue": "2026-08-14T17:04:12.884Z" },
            "operator":     { "stringValue": "alvarez" }
          },
          "createTime": "2026-08-14T17:04:13.002Z"
        }
      ]
    }"#;

    fn config(endpoint: &str) -> SourceConfig {
        SourceConfig::new(SourceKind::Firebase, endpoint).with_option("collection", "readings")
    }

    /// A source whose transport replays `replies` without a socket.
    fn queued(replies: Vec<(u16, &str)>) -> (FirebaseSource, Arc<QueuedTransport>) {
        let transport = QueuedTransport::new(replies);
        let source = FirebaseSource::with_transport(
            config("http://firestore.invalid/v1/projects/demo/databases/(default)"),
            transport.clone(),
        )
        .expect("valid firebase config");
        (source, transport)
    }

    fn f64_col(frame: &Frame, name: &str) -> Vec<Option<f64>> {
        match frame.column(name).unwrap_or_else(|| panic!("no column `{name}`")) {
            ColumnData::F64(v) => v.clone(),
            other => panic!("column `{name}` is {:?}, expected F64", other.dtype()),
        }
    }

    fn i64_col(frame: &Frame, name: &str) -> Vec<Option<i64>> {
        match frame.column(name).unwrap_or_else(|| panic!("no column `{name}`")) {
            ColumnData::I64(v) => v.clone(),
            other => panic!("column `{name}` is {:?}, expected I64", other.dtype()),
        }
    }

    fn str_col(frame: &Frame, name: &str) -> Vec<Option<String>> {
        match frame.column(name).unwrap_or_else(|| panic!("no column `{name}`")) {
            ColumnData::Str(v) => v.clone(),
            other => panic!("column `{name}` is {:?}, expected Str", other.dtype()),
        }
    }

    // ── The load-bearing behaviour: typed-value unwrapping ───────────────────

    #[test]
    fn typed_values_become_typed_columns_not_a_wall_of_strings() {
        let (source, _t) = queued(vec![(200, READINGS)]);
        let frame = source.fetch().expect("fetch");

        assert_eq!(frame.n_rows(), 2);
        let dtype = |name: &str| frame.column(name).expect(name).dtype();

        // The assertion this whole module exists to satisfy. Feed the raw
        // Firestore payload to a generic JSON reader and every one of these
        // comes back `Str`, because each value is wrapped in a typed object.
        assert_eq!(dtype("sample_index"), ColumnDtype::I64, "integerValue must unwrap to I64");
        assert_eq!(dtype("pressure_psi"), ColumnDtype::F64, "doubleValue must unwrap to F64");
        assert_eq!(dtype("valve_open"), ColumnDtype::Bool, "booleanValue must unwrap to Bool");
        assert_eq!(dtype("sensor"), ColumnDtype::Str, "stringValue stays Str");
        assert_eq!(dtype("recorded_at"), ColumnDtype::Str, "timestampValue stays Str");

        assert_eq!(i64_col(&frame, "sample_index"), vec![Some(1), Some(-9_007_199_254_740_993)]);
        assert_eq!(f64_col(&frame, "pressure_psi"), vec![Some(14.7), None]);
        assert_eq!(
            match frame.column("valve_open").unwrap() {
                ColumnData::Bool(v) => v.clone(),
                _ => unreachable!(),
            },
            vec![Some(true), Some(false)]
        );
    }

    #[test]
    fn an_integer_beyond_f64_precision_survives_exactly() {
        // -9007199254740993 is 2^53+1 in magnitude, so it has no exact f64
        // form: routing integerValue through a float would silently corrupt it,
        // which is precisely why Firestore ships integers as strings.
        let (source, _t) = queued(vec![(200, READINGS)]);
        let frame = source.fetch().expect("fetch");
        let counts = i64_col(&frame, "sample_index");
        assert_eq!(counts[1], Some(-9_007_199_254_740_993));
        assert_ne!(
            counts[1].unwrap() as f64 as i64,
            counts[1].unwrap(),
            "the fixture must actually exceed f64 precision or this test proves nothing"
        );
    }

    #[test]
    fn null_value_is_a_null_cell_and_does_not_demote_the_column() {
        let (source, _t) = queued(vec![(200, READINGS)]);
        let frame = source.fetch().expect("fetch");
        // `pressure_psi` is a double in row 0 and nullValue in row 1: the null
        // must not drag the column down to text.
        assert_eq!(frame.column("pressure_psi").unwrap().dtype(), ColumnDtype::F64);
        assert_eq!(f64_col(&frame, "pressure_psi"), vec![Some(14.7), None]);
    }

    #[test]
    fn a_field_absent_from_one_document_becomes_a_null_not_a_shifted_row() {
        let (source, _t) = queued(vec![(200, READINGS)]);
        let frame = source.fetch().expect("fetch");
        // `tags` exists only on the first document.
        let tags = str_col(&frame, "tags");
        assert_eq!(tags.len(), 2, "every column must span every document");
        assert_eq!(tags[1], None, "the document without `tags` must hold a null");
    }

    #[test]
    fn a_composite_value_keeps_its_structure_as_compact_json() {
        let (source, _t) = queued(vec![(200, READINGS)]);
        let frame = source.fetch().expect("fetch");
        let tags = str_col(&frame, "tags");
        assert_eq!(tags[0].as_deref(), Some(r#"{"values":[{"stringValue":"a"}]}"#));
    }

    #[test]
    fn mixed_integer_and_double_widen_to_one_f64_column() {
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"integerValue":"3"}}},
            {"name":"p/d/x/2","fields":{"v":{"doubleValue":2.5}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.column("v").unwrap().dtype(), ColumnDtype::F64);
        assert_eq!(f64_col(&frame, "v"), vec![Some(3.0), Some(2.5)]);
    }

    #[test]
    fn genuinely_mixed_types_fall_back_to_rendered_strings() {
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"integerValue":"3"}}},
            {"name":"p/d/x/2","fields":{"v":{"stringValue":"n/a"}}},
            {"name":"p/d/x/3","fields":{"v":{"booleanValue":true}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(
            str_col(&frame, "v"),
            vec![Some("3".into()), Some("n/a".into()), Some("true".into())]
        );
    }

    #[test]
    fn a_boolean_spelled_as_a_string_still_unwraps_to_a_bool() {
        // Some emulator builds quote booleans; a Bool column beats a text one.
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"booleanValue":"true"}}},
            {"name":"p/d/x/2","fields":{"v":{"booleanValue":false}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.column("v").unwrap().dtype(), ColumnDtype::Bool);
    }

    #[test]
    fn nan_and_infinity_stay_visible_as_text_rather_than_becoming_nulls() {
        // JSON has no NaN/Infinity literal, so Firestore sends them quoted and
        // they cannot re-enter a numeric column. Keeping the label is the
        // honest outcome; nulling it to protect the dtype would hide data.
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"doubleValue":"NaN"}}},
            {"name":"p/d/x/2","fields":{"v":{"doubleValue":1.5}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.column("v").unwrap().dtype(), ColumnDtype::Str);
        assert_eq!(str_col(&frame, "v"), vec![Some("NaN".into()), Some("1.5".into())]);
    }

    #[test]
    fn a_finite_double_sent_as_a_string_still_becomes_a_float() {
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"doubleValue":"2.5"}}},
            {"name":"p/d/x/2","fields":{"v":{"doubleValue":1.5}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.column("v").unwrap().dtype(), ColumnDtype::F64);
        assert_eq!(f64_col(&frame, "v"), vec![Some(2.5), Some(1.5)]);
    }

    #[test]
    fn a_field_that_is_not_a_typed_envelope_passes_through_untouched() {
        // Two members means it is not a `{"<type>Value": …}` wrapper, so
        // guessing at the first key alphabetically would be wrong.
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"stringValue":"a","extra":1}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(str_col(&frame, "v"), vec![Some(r#"{"extra":1,"stringValue":"a"}"#.into())]);
    }

    // ── Frame shape ──────────────────────────────────────────────────────────

    #[test]
    fn the_document_id_column_holds_the_resource_path_tail() {
        let (source, _t) = queued(vec![(200, READINGS)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(
            str_col(&frame, DOCUMENT_ID_COLUMN),
            vec![Some("r-0001".into()), Some("r-0003".into())]
        );
    }

    #[test]
    fn the_document_id_column_can_be_switched_off() {
        let transport = QueuedTransport::new(vec![(200, READINGS)]);
        let config = config("http://firestore.invalid/v1/projects/demo/databases/(default)")
            .with_option("include_document_id", "false");
        let frame = FirebaseSource::with_transport(config, transport)
            .unwrap()
            .fetch()
            .expect("fetch");
        assert!(frame.column(DOCUMENT_ID_COLUMN).is_none());
    }

    #[test]
    fn a_real_document_id_field_wins_over_the_synthetic_column() {
        let body = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"document_id":{"stringValue":"authored"}}}
        ]}"#;
        let (source, _t) = queued(vec![(200, body)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.n_cols(), 1, "the synthetic column must not duplicate the field");
        assert_eq!(str_col(&frame, DOCUMENT_ID_COLUMN), vec![Some("authored".into())]);
    }

    #[test]
    fn an_empty_collection_is_an_empty_frame_not_an_error() {
        // Firestore omits the `documents` key entirely when nothing matches.
        let (source, _t) = queued(vec![(200, "{}")]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.n_rows(), 0);
        assert_eq!(frame.n_cols(), 0);
    }

    #[test]
    fn a_bare_array_payload_from_run_query_is_accepted() {
        let body = r#"[{"name":"p/d/x/1","fields":{"v":{"integerValue":"7"}}}]"#;
        let (source, _t) = queued(vec![(200, body)]);
        assert_eq!(i64_col(&source.fetch().expect("fetch"), "v"), vec![Some(7)]);
    }

    // ── Paging ───────────────────────────────────────────────────────────────

    #[test]
    fn pages_are_followed_via_next_page_token() {
        let page1 = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"integerValue":"1"}}},
            {"name":"p/d/x/2","fields":{"v":{"integerValue":"2"}}}
        ],"nextPageToken":"tok-2"}"#;
        let page2 = r#"{"documents":[
            {"name":"p/d/x/3","fields":{"v":{"integerValue":"3"}}}
        ]}"#;
        let (source, transport) = queued(vec![(200, page1), (200, page2)]);
        let frame = source.fetch().expect("fetch");

        assert_eq!(i64_col(&frame, "v"), vec![Some(1), Some(2), Some(3)]);
        let urls = transport.urls();
        assert_eq!(urls.len(), 2, "the second page must be requested");
        assert!(!urls[0].contains("pageToken"), "the first page carries no token: {}", urls[0]);
        assert!(urls[1].contains("pageToken=tok-2"), "second request: {}", urls[1]);
    }

    #[test]
    fn a_server_that_keeps_repeating_one_token_does_not_loop_forever() {
        // The shared Firestore fixture always carries a nextPageToken, so a
        // naive walker would keep requesting until the row cap. Re-offering a
        // token already used means the server is not advancing; stop.
        let page = r#"{"documents":[{"name":"p/d/x/1","fields":{"v":{"integerValue":"1"}}}],
                       "nextPageToken":"stuck"}"#;
        let (source, transport) = queued(vec![(200, page)]);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.n_rows(), 2, "one page, one retry with the token, then stop");
        assert_eq!(transport.request_count(), 2);
    }

    #[test]
    fn the_limit_option_caps_the_total_documents_and_shrinks_the_page() {
        let page = r#"{"documents":[
            {"name":"p/d/x/1","fields":{"v":{"integerValue":"1"}}},
            {"name":"p/d/x/2","fields":{"v":{"integerValue":"2"}}},
            {"name":"p/d/x/3","fields":{"v":{"integerValue":"3"}}}
        ],"nextPageToken":"more"}"#;
        let transport = QueuedTransport::new(vec![(200, page)]);
        let config = config("http://firestore.invalid/v1/projects/demo/databases/(default)")
            .with_option("limit", "2");
        let frame = FirebaseSource::with_transport(config, transport.clone())
            .unwrap()
            .fetch()
            .expect("fetch");

        assert_eq!(i64_col(&frame, "v"), vec![Some(1), Some(2)], "limit must truncate");
        assert_eq!(transport.request_count(), 1, "the limit must stop the page walk");
        assert!(
            transport.urls()[0].contains("pageSize=2"),
            "a small limit must shrink the page: {}",
            transport.urls()[0]
        );
    }

    // ── Request shape ────────────────────────────────────────────────────────

    #[test]
    fn the_request_targets_the_documents_path_with_a_page_size() {
        let transport = QueuedTransport::new(vec![(200, "{}")]);
        let config = config("http://firestore.invalid/v1/projects/demo/databases/(default)")
            .with_option("page_size", "25")
            .with_option("order_by", "recorded_at desc");
        FirebaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");

        let url = &transport.urls()[0];
        assert!(
            url.contains("/v1/projects/demo/databases/(default)/documents/readings"),
            "unexpected url: {url}"
        );
        assert!(url.contains("pageSize=25"), "unexpected url: {url}");
        assert!(url.contains("orderBy=recorded_at%20desc"), "unexpected url: {url}");
    }

    #[test]
    fn an_endpoint_that_already_names_documents_is_not_doubled() {
        let transport = QueuedTransport::new(vec![(200, "{}")]);
        let config = config("http://firestore.invalid/v1/projects/demo/databases/(default)/documents/");
        FirebaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        let url = &transport.urls()[0];
        assert!(!url.contains("documents/documents"), "path was doubled: {url}");
        assert!(url.contains("/documents/readings"), "unexpected url: {url}");
    }

    #[test]
    fn table_is_accepted_as_an_alias_for_collection() {
        let transport = QueuedTransport::new(vec![(200, "{}")]);
        let config =
            SourceConfig::new(SourceKind::Firebase, "http://firestore.invalid/v1/projects/d/x")
                .with_option("table", "readings");
        FirebaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        assert!(transport.urls()[0].contains("/documents/readings"));
    }

    #[test]
    fn test_connection_asks_for_a_single_document() {
        let (source, transport) = queued(vec![(200, READINGS)]);
        let status = source.test_connection().expect("probe");
        assert!(status.reachable, "{}", status.detail);
        assert!(status.detail.contains("readings"), "{}", status.detail);
        assert!(
            transport.urls()[0].contains("pageSize=1"),
            "the liveness probe must not fetch the whole collection: {}",
            transport.urls()[0]
        );
    }

    // ── Credentials ──────────────────────────────────────────────────────────

    #[test]
    fn a_bearer_token_is_sent_when_secret_ref_resolves() {
        const VAR: &str = "EUSTRESS_TEST_FIREBASE_TOKEN";
        std::env::set_var(VAR, "tok-abc123");
        let transport = QueuedTransport::new(vec![(200, "{}")]);
        let config = config("http://firestore.invalid/v1/projects/d/x").with_secret_ref(VAR);
        FirebaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        std::env::remove_var(VAR);
        assert_eq!(transport.header(0, "authorization").as_deref(), Some("Bearer tok-abc123"));
    }

    #[test]
    fn no_secret_ref_means_no_authorization_header() {
        let (source, transport) = queued(vec![(200, "{}")]);
        source.fetch().expect("fetch");
        assert_eq!(
            transport.header(0, "authorization"),
            None,
            "an unauthenticated source must not invent an Authorization header"
        );
    }

    #[test]
    fn a_named_secret_that_is_not_in_the_environment_fails_loudly() {
        const VAR: &str = "EUSTRESS_TEST_FIREBASE_ABSENT";
        std::env::remove_var(VAR);
        let transport = QueuedTransport::new(vec![(200, "{}")]);
        let config = config("http://firestore.invalid/v1/projects/d/x").with_secret_ref(VAR);
        let err = FirebaseSource::with_transport(config, transport.clone())
            .unwrap()
            .fetch()
            .unwrap_err();
        assert!(err.to_string().contains(VAR), "the error must name the variable: {err}");
        assert_eq!(transport.request_count(), 0, "no request may be sent without the secret");
    }

    // ── Failure modes ────────────────────────────────────────────────────────

    #[test]
    fn a_google_error_envelope_surfaces_its_message() {
        let body = r#"{"error":{"code":403,"message":"Missing or insufficient permissions.","status":"PERMISSION_DENIED"}}"#;
        let (source, _t) = queued(vec![(403, body)]);
        let text = source.fetch().unwrap_err().to_string();
        assert!(text.contains("403"), "{text}");
        assert!(text.contains("PERMISSION_DENIED"), "{text}");
        assert!(text.contains("Missing or insufficient permissions."), "{text}");
    }

    #[test]
    fn a_failed_probe_reports_unreachable_rather_than_erroring() {
        let (source, _t) = queued(vec![(500, r#"{"error":{"message":"boom","status":"INTERNAL"}}"#)]);
        let status = source.test_connection().expect("probe returns Ok");
        assert!(!status.reachable);
        assert!(status.detail.contains("500"), "{}", status.detail);
        assert!(status.detail.contains("boom"), "{}", status.detail);
    }

    #[test]
    fn malformed_json_is_rejected() {
        let (source, _t) = queued(vec![(200, r#"{"documents": [ {"name": }"#)]);
        assert!(source.fetch().is_err(), "a truncated payload must not yield a frame");
    }

    #[test]
    fn a_fields_member_that_is_not_an_object_is_rejected() {
        let (source, _t) = queued(vec![(200, r#"{"documents":[{"name":"p/1","fields":"oops"}]}"#)]);
        let err = source.fetch().unwrap_err();
        assert!(err.to_string().contains("not an object"), "{err}");
    }

    // ── Over a real socket ───────────────────────────────────────────────────

    #[test]
    fn the_provider_works_end_to_end_over_a_loopback_socket() {
        // Everything above swaps the transport out; this proves the request the
        // provider builds is a well-formed HTTP request that a real server
        // answers, with the credential genuinely on the wire.
        const VAR: &str = "EUSTRESS_TEST_FIREBASE_SOCKET_TOKEN";
        std::env::set_var(VAR, "wire-token");
        let server = StubServer::json(READINGS);
        let config = config(&format!("{}/v1/projects/demo/databases/(default)", server.base_url))
            .with_secret_ref(VAR);
        let source = FirebaseSource::with_transport(config, Arc::new(SocketTransport)).unwrap();
        let frame = source.fetch().expect("fetch over a socket");
        std::env::remove_var(VAR);

        assert_eq!(frame.n_rows(), 2);
        assert_eq!(i64_col(&frame, "sample_index")[0], Some(1));

        let request = server.first_request();
        assert!(request.starts_with("GET /v1/projects/demo/databases/(default)/documents/readings"));
        assert!(
            request.contains("Authorization: Bearer wire-token"),
            "the token must reach the wire, not just a struct field: {request}"
        );
    }

    #[test]
    fn a_non_200_over_a_real_socket_becomes_an_error() {
        let server = StubServer::spawn(401, "application/json", r#"{"error":{"message":"nope"}}"#);
        let config = config(&format!("{}/v1/projects/demo/databases/(default)", server.base_url));
        let source = FirebaseSource::with_transport(config, Arc::new(SocketTransport)).unwrap();
        let text = source.fetch().unwrap_err().to_string();
        assert!(text.contains("401"), "{text}");
        assert!(text.contains("nope"), "{text}");
    }

    // ── Config guards ────────────────────────────────────────────────────────

    #[test]
    fn a_config_for_another_provider_is_refused() {
        let config = SourceConfig::new(SourceKind::Supabase, "https://x.example.com")
            .with_option("table", "readings");
        let err =
            FirebaseSource::with_transport(config, QueuedTransport::new(vec![(200, "{}")]))
                .unwrap_err();
        assert!(err.to_string().contains("Supabase"), "{err}");
    }

    #[test]
    fn a_config_without_a_collection_is_refused_at_construction() {
        let config = SourceConfig::new(SourceKind::Firebase, "https://x.example.com");
        assert!(FirebaseSource::new(config).is_err());
    }

    #[test]
    fn the_source_reports_its_kind() {
        let (source, _t) = queued(vec![(200, "{}")]);
        assert_eq!(source.kind(), SourceKind::Firebase);
        assert_eq!(source.config().kind, SourceKind::Firebase);
    }
}
