//! The Connector poll runtime: the keystone that turns an inert `_instance.toml`
//! into live federal opportunity data.
//!
//! A `Connector` instance has carried `endpoint` / `poll_seconds` / `enabled`
//! since the Data menu shipped, and nothing consumed it. This module is that
//! consumer, scoped to the two capture profiles.
//!
//! # Why a thread and not a system
//!
//! `ureq` blocks. A SAM.gov page can take several seconds, and a page of 1000
//! records over a slow link takes longer. Doing that inside a Bevy system
//! stalls the frame, so the fetch runs on a detached thread and posts its
//! result back over a channel that [`drain_poll_results`] reads at whatever
//! rate the schedule happens to run.
//!
//! # Safety posture
//!
//! - **Nothing polls unless a human enabled it.** `enabled` defaults to false
//!   on create and this runtime never flips it.
//! - **A secret is looked up by NAME**, never stored in the manifest, and never
//!   logged. The value lives in the local credential store outside every Space
//!   (see [`super::credentials`]), with an environment variable of the same
//!   name as a fallback for headless and CI runs. Every log path runs the URL
//!   through `redact_query`.
//! - **Both endpoints are read-only.** There is no write path to either system
//!   anywhere in this crate.
//! - **A rate limit backs off.** A 429 stops the walk and pushes the next
//!   attempt out by [`DEFAULT_BACKOFF_SECS`] rather than retrying into the
//!   limit. The transport seam carries status and body only, so the server's
//!   own `Retry-After` is not readable; the field is kept on the error for
//!   when it is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use bevy::prelude::*;
use eustress_data::source::http::{redact_query, HttpTransport};

use super::model::Opportunity;
use super::sources::{self, Profile};
use super::CaptureError;

/// How often the scanner looks for due Connectors. Cheap: a directory listing
/// plus a TOML parse per Connector, and only when a Space is open.
const SCAN_INTERVAL_SECS: f32 = 5.0;

/// Floor on `poll_seconds`. A Connector asking for one-second polling would
/// burn a SAM.gov daily quota in under an hour, so the manifest cannot request
/// a rate the account cannot sustain.
pub const MIN_POLL_SECONDS: u64 = 60;

/// How long to wait after a rate-limit response that named no interval.
const DEFAULT_BACKOFF_SECS: u64 = 900;

/// One completed fetch, headed back to the schedule.
#[derive(Debug)]
pub struct PollOutcome {
    /// Absolute path of the Connector directory that produced this.
    pub connector: PathBuf,
    pub profile: Profile,
    pub result: Result<PollPayload, CaptureError>,
}

/// What a successful fetch returned.
#[derive(Debug, Clone)]
pub struct PollPayload {
    pub opportunities: Vec<Opportunity>,
    /// Raw response bodies, one per page, kept for the audit trail.
    pub raw_pages: Vec<String>,
    pub total_records: u64,
    pub pages_fetched: u32,
    /// Records the parser could not identify. Non-zero is worth surfacing.
    pub skipped: usize,
}

/// Poll scheduling state. One entry per Connector directory.
#[derive(Resource)]
pub struct CapturePoll {
    tx: Sender<PollOutcome>,
    /// Wrapped because `mpsc::Receiver` is `Send` but not `Sync`, and a Bevy
    /// `Resource` must be both. Uncontended in practice: only
    /// `drain_poll_results` ever takes it, once per frame.
    rx: Mutex<Receiver<PollOutcome>>,
    /// Connectors with a fetch in flight, so a slow source is not re-entered.
    inflight: Vec<PathBuf>,
    /// Seconds-since-startup at which each Connector may next be polled.
    next_due: HashMap<PathBuf, f64>,
    seconds_to_next_scan: f32,
    /// The most recent outcome, for the ribbon to report.
    pub last: Option<String>,
}

impl Default for CapturePoll {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx: Mutex::new(rx),
            inflight: Vec::new(),
            next_due: HashMap::new(),
            seconds_to_next_scan: 0.0,
            last: None,
        }
    }
}

impl CapturePoll {
    /// Force every Connector to be due on the next scan.
    ///
    /// This is what the ribbon's Sync Now button calls. It clears the schedule
    /// rather than fetching inline, so a click can never block the frame.
    pub fn request_sync_now(&mut self) {
        self.next_due.clear();
        self.seconds_to_next_scan = 0.0;
    }
}

/// A Connector's configuration, as read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorSpec {
    pub dir: PathBuf,
    pub profile: Profile,
    pub endpoint: String,
    pub enabled: bool,
    pub poll_seconds: u64,
    /// Name of the environment variable holding the API key. Never the key.
    pub secret_ref: Option<String>,
    /// Profile-specific filters, verbatim from `[attributes]`.
    pub options: HashMap<String, String>,
}

impl ConnectorSpec {
    /// Read a filter, trimmed, absent when empty.
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(|s| s.trim()).filter(|s| !s.is_empty())
    }
}

/// Parse one `_instance.toml` into a capture Connector spec.
///
/// Returns `NotACaptureConnector` for every other Connector and for every other
/// class, because this runtime must never touch a source it does not
/// understand.
pub fn parse_connector(dir: &Path, toml_text: &str) -> Result<ConnectorSpec, CaptureError> {
    let doc: toml::Value = toml::from_str(toml_text)
        .map_err(|e| CaptureError::Parse(format!("{}: {e}", dir.display())))?;

    let class = doc
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    if class != "Connector" {
        return Err(CaptureError::NotACaptureConnector);
    }

    let attrs = doc.get("attributes").and_then(toml::Value::as_table);
    let Some(attrs) = attrs else { return Err(CaptureError::NotACaptureConnector) };

    let get = |k: &str| -> Option<String> {
        attrs.get(k).and_then(|v| match v {
            toml::Value::String(s) => Some(s.clone()),
            toml::Value::Integer(i) => Some(i.to_string()),
            toml::Value::Boolean(b) => Some(b.to_string()),
            _ => None,
        })
    };

    // The profile can be declared explicitly or inferred from `source_type`,
    // so a Connector created through the generic Data menu and then pointed at
    // SAM.gov still works.
    let profile = get("profile")
        .as_deref()
        .and_then(Profile::parse)
        .or_else(|| get("source_type").as_deref().and_then(Profile::parse))
        .ok_or(CaptureError::NotACaptureConnector)?;

    let endpoint = get("endpoint")
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| profile.default_endpoint().to_string());

    let enabled = get("enabled")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false);

    let poll_seconds = get("poll_seconds")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(3600)
        .max(MIN_POLL_SECONDS);

    let secret_ref = get("secret_ref")
        .or_else(|| get("api_key_env"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let reserved = ["profile", "source_type", "endpoint", "enabled", "poll_seconds", "secret_ref", "api_key_env"];
    let options = attrs
        .iter()
        .filter(|(k, _)| !reserved.contains(&k.as_str()))
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();

    Ok(ConnectorSpec { dir: dir.to_path_buf(), profile, endpoint, enabled, poll_seconds, secret_ref, options })
}

/// Find every capture Connector under a Space's `DataService`.
pub fn discover(space_root: &Path) -> Vec<ConnectorSpec> {
    let dir = space_root.join("DataService");
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<ConnectorSpec> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let text = std::fs::read_to_string(p.join("_instance.toml")).ok()?;
            parse_connector(&p, &text).ok()
        })
        .collect();
    // Stable order so a log reads the same way twice.
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// Build the request pages for one Connector and run them to completion.
///
/// Pure with respect to the transport: pass a stub and the whole pagination,
/// error, and rate-limit path is testable with no network.
pub fn fetch_all(
    spec: &ConnectorSpec,
    transport: &dyn HttpTransport,
    api_key: Option<&str>,
    max_pages: u32,
) -> Result<PollPayload, CaptureError> {
    let mut opportunities = Vec::new();
    let mut raw_pages = Vec::new();
    let mut total_records = 0u64;
    let mut skipped = 0usize;
    let mut offset = 0u64;
    let mut pages = 0u32;

    while pages < max_pages {
        let request = match spec.profile {
            Profile::SamOpportunities => {
                let q = sam_query(spec, offset)?;
                sources::sam_request(&q, api_key.unwrap_or_default())?
            }
            Profile::GrantsSearch => {
                let q = grants_query(spec, offset);
                sources::grants_request(&q)?
            }
        };

        let response = transport
            .send(&request)
            .map_err(|e| CaptureError::Transport(format!("{} {}: {e}", request.method, redact_query(&request.url))))?;

        // The shared transport seam carries status and body only, so the
        // server's own `Retry-After` is not readable here. Report the limit
        // with no interval and let the caller apply its default backoff:
        // guessing a shorter wait would retry straight back into the limit.
        if response.status == 429 {
            return Err(CaptureError::RateLimited { retry_after_secs: None });
        }
        if !response.is_success() {
            return Err(CaptureError::Upstream(format!(
                "{} returned HTTP {}",
                redact_query(&request.url),
                response.status
            )));
        }

        let body = response.text();
        let page = match spec.profile {
            Profile::SamOpportunities => sources::parse_sam(&body)?,
            Profile::GrantsSearch => sources::parse_grants(&body)?,
        };

        total_records = page.total_records;
        skipped += page.skipped;
        let consumed = page.opportunities.len() as u64 + page.skipped as u64;
        let more = page.has_more(offset);
        opportunities.extend(page.opportunities);
        raw_pages.push(body);
        pages += 1;
        offset += consumed;

        if !more {
            break;
        }
    }

    Ok(PollPayload { opportunities, raw_pages, total_records, pages_fetched: pages, skipped })
}

/// Build a SAM query from a Connector's attributes.
fn sam_query(spec: &ConnectorSpec, offset: u64) -> Result<sources::SamQuery, CaptureError> {
    let posted_from = spec.option("posted_from").unwrap_or_default().to_string();
    let posted_to = spec.option("posted_to").unwrap_or_default().to_string();
    if posted_from.is_empty() || posted_to.is_empty() {
        return Err(CaptureError::BadQuery(
            "set posted_from and posted_to (MM/dd/yyyy) on the Connector; SAM.gov requires a \
             bounded window of at most one year"
                .into(),
        ));
    }
    Ok(sources::SamQuery {
        endpoint: spec.endpoint.clone(),
        posted_from,
        posted_to,
        naics: spec.option("naics").map(str::to_string),
        set_aside: spec.option("set_aside").map(str::to_string),
        notice_type: spec.option("notice_type").map(str::to_string),
        state: spec.option("state").map(str::to_string),
        limit: spec
            .option("limit")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100)
            .clamp(1, sources::SAM_MAX_LIMIT),
        offset: offset as u32,
    })
}

/// Build a Grants query from a Connector's attributes.
fn grants_query(spec: &ConnectorSpec, offset: u64) -> sources::GrantsQuery {
    let split = |k: &str| -> Vec<String> {
        spec.option(k)
            .map(|v| v.split(['|', ',']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    };
    let statuses = {
        let s = split("statuses");
        if s.is_empty() { vec!["posted".to_string()] } else { s }
    };
    sources::GrantsQuery {
        endpoint: spec.endpoint.clone(),
        keyword: spec.option("keyword").map(str::to_string),
        aln: spec.option("aln").map(str::to_string),
        agencies: split("agencies"),
        statuses,
        eligibilities: split("eligibilities"),
        rows: spec.option("rows").and_then(|v| v.parse().ok()).unwrap_or(100).max(1),
        start_record: offset as u32,
    }
}

/// Resolve a Connector's API key from the environment.
///
/// Returns the value, never stores it, and reports the variable NAME on
/// failure so a user can fix it without the message ever carrying a secret.
fn resolve_secret(spec: &ConnectorSpec) -> Result<Option<String>, CaptureError> {
    match spec.profile {
        Profile::GrantsSearch => Ok(None),
        Profile::SamOpportunities => {
            let name = spec.secret_ref.as_deref().unwrap_or(super::credentials::DEFAULT_SAM_KEY);
            // The saved key wins over the environment; see `credentials::get`.
            match super::credentials::get(name) {
                Some(v) => Ok(Some(v)),
                // Name the BUTTON, not the variable. Telling a grant writer to
                // set an environment variable and relaunch is not a workflow,
                // and an error message is the one place they are guaranteed to
                // be looking when they need the answer.
                None => Err(CaptureError::MissingSecret(format!(
                    "no SAM.gov API key yet. Capture ▸ Sources ▸ API Key, paste it, Save.                      It is free from your SAM.gov Account Details page. (credential: {name})"
                ))),
            }
        }
    }
}

/// Scan for due Connectors and dispatch their fetches to background threads.
fn scan_connectors(
    time: Res<Time>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut poll: ResMut<CapturePoll>,
) {
    poll.seconds_to_next_scan -= time.delta_secs();
    if poll.seconds_to_next_scan > 0.0 {
        return;
    }
    poll.seconds_to_next_scan = SCAN_INTERVAL_SECS;

    let Some(root) = space_root else { return };
    let elapsed = time.elapsed_secs_f64();

    for spec in discover(&root.0) {
        if !spec.enabled || poll.inflight.contains(&spec.dir) {
            continue;
        }
        if poll.next_due.get(&spec.dir).is_some_and(|due| elapsed < *due) {
            continue;
        }

        let api_key = match resolve_secret(&spec) {
            Ok(k) => k,
            Err(e) => {
                // Push the retry out so a missing key does not re-log every
                // scan, and say so once.
                poll.next_due.insert(spec.dir.clone(), elapsed + spec.poll_seconds as f64);
                warn!("capture: {} skipped: {e}", spec.dir.display());
                poll.last = Some(format!("{e}"));
                continue;
            }
        };

        poll.next_due.insert(spec.dir.clone(), elapsed + spec.poll_seconds as f64);
        poll.inflight.push(spec.dir.clone());

        let tx = poll.tx.clone();
        std::thread::Builder::new()
            .name("capture-poll".into())
            .spawn(move || {
                let transport = eustress_data::source::http::UreqTransport::default();
                let result = fetch_all(&spec, &transport, api_key.as_deref(), 10);
                let _ = tx.send(PollOutcome { connector: spec.dir, profile: spec.profile, result });
            })
            .ok();
    }
}

/// Receive completed fetches and file them.
fn drain_poll_results(
    mut poll: ResMut<CapturePoll>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut notifications: MessageWriter<crate::ui::notifications::NotificationEvent>,
) {
    let outcomes: Vec<PollOutcome> = match poll.rx.lock() {
        Ok(rx) => rx.try_iter().collect(),
        // A poisoned lock means a previous drain panicked. Take the data
        // anyway: discarding fetched pages would compound the original fault
        // with silent data loss, which is the worse of the two.
        Err(poisoned) => poisoned.into_inner().try_iter().collect(),
    };
    for outcome in outcomes {
        poll.inflight.retain(|p| p != &outcome.connector);
        let name = outcome
            .connector
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| outcome.connector.display().to_string());

        match outcome.result {
            Ok(payload) => {
                let summary = format!(
                    "{} fetched {} of {} notice(s) over {} page(s)",
                    name,
                    payload.opportunities.len(),
                    payload.total_records,
                    payload.pages_fetched
                );
                if payload.skipped > 0 {
                    warn!("capture: {name} skipped {} record(s) with no usable id", payload.skipped);
                }
                info!("capture: {summary}");
                poll.last = Some(summary.clone());

                if let Some(root) = space_root.as_ref() {
                    if let Err(e) = write_raw_pages(&root.0, &name, &payload) {
                        warn!("capture: {name} fetched but could not be filed: {e}");
                    }
                }
                notifications.write(crate::ui::notifications::NotificationEvent::success(
                    crate::ui::notifications::NotificationCategory::General,
                    "Capture sync",
                    summary,
                ));
            }
            Err(e) => {
                // A rate limit is not a failure of the tool; push the next
                // attempt out by what the source asked for.
                if let CaptureError::RateLimited { retry_after_secs } = &e {
                    let wait = retry_after_secs.unwrap_or(DEFAULT_BACKOFF_SECS);
                    let due = poll.next_due.entry(outcome.connector.clone()).or_insert(0.0);
                    *due += wait as f64;
                }
                warn!("capture: {name}: {e}");
                poll.last = Some(format!("{name}: {e}"));
                notifications.write(crate::ui::notifications::NotificationEvent::warning(
                    crate::ui::notifications::NotificationCategory::General,
                    "Capture sync failed",
                    format!("{name}: {e}"),
                ));
            }
        }
    }
}

/// Write the raw pages under the Space so an auditor sees exactly what arrived.
fn write_raw_pages(space_root: &Path, connector: &str, payload: &PollPayload) -> std::io::Result<()> {
    let dir = space_root.join(super::audit::CAPTURE_DIR).join("_raw").join(connector);
    std::fs::create_dir_all(&dir)?;
    for (i, body) in payload.raw_pages.iter().enumerate() {
        std::fs::write(dir.join(format!("page-{:03}.json", i + 1)), body)?;
    }
    Ok(())
}

/// Registers the Connector poll runtime.
pub struct ConnectorPollPlugin;

impl Plugin for ConnectorPollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CapturePoll>()
            .add_systems(Update, (scan_connectors, drain_poll_results));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_data::source::http::{HttpRequest, HttpResponse};

    /// A transport that replays canned responses in order and records what it
    /// was asked for. The whole pagination path is testable with no network.
    struct Stub {
        responses: Mutex<Vec<HttpResponse>>,
        seen: Mutex<Vec<String>>,
    }

    impl Stub {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self { responses: Mutex::new(responses), seen: Mutex::new(Vec::new()) }
        }
    }

    impl HttpTransport for Stub {
        fn send(&self, request: &HttpRequest) -> eustress_data::Result<HttpResponse> {
            self.seen.lock().unwrap().push(request.url.clone());
            let mut r = self.responses.lock().unwrap();
            if r.is_empty() {
                return Ok(HttpResponse::new(200, r#"{"opportunitiesData":[],"totalRecords":0}"#));
            }
            Ok(r.remove(0))
        }
    }

    fn sam_spec() -> ConnectorSpec {
        let mut options = HashMap::new();
        options.insert("posted_from".into(), "08/01/2026".into());
        options.insert("posted_to".into(), "08/23/2026".into());
        options.insert("naics".into(), "541511".into());
        ConnectorSpec {
            dir: PathBuf::from("/tmp/SAM Source"),
            profile: Profile::SamOpportunities,
            endpoint: sources::SAM_SEARCH_URL.into(),
            enabled: true,
            poll_seconds: 3600,
            secret_ref: Some("SAM_API_KEY".into()),
            options,
        }
    }

    fn page(ids: &[&str], total: u64) -> HttpResponse {
        let records: Vec<String> = ids
            .iter()
            .map(|id| format!(r#"{{"noticeId":"{id}","title":"T","naicsCode":"541511"}}"#))
            .collect();
        HttpResponse::new(
            200,
            format!(r#"{{"totalRecords":{total},"opportunitiesData":[{}]}}"#, records.join(",")),
        )
    }

    #[test]
    fn a_capture_connector_parses_its_profile_and_filters() {
        let toml = r#"
[metadata]
class_name = "Connector"

[attributes]
profile = "sam_opportunities"
endpoint = ""
enabled = true
poll_seconds = 3600
secret_ref = "SAM_API_KEY"
naics = "541511"
posted_from = "08/01/2026"
posted_to = "08/23/2026"
"#;
        let spec = parse_connector(Path::new("/tmp/c"), toml).expect("parses");
        assert_eq!(spec.profile, Profile::SamOpportunities);
        assert_eq!(spec.endpoint, sources::SAM_SEARCH_URL, "a blank endpoint takes the default");
        assert!(spec.enabled);
        assert_eq!(spec.option("naics"), Some("541511"));
        assert_eq!(spec.secret_ref.as_deref(), Some("SAM_API_KEY"));
    }

    #[test]
    fn a_non_capture_connector_is_left_alone() {
        let toml = "[metadata]\nclass_name = \"Connector\"\n\n[attributes]\nsource_type = \"PostgreSQL\"\nenabled = true\n";
        assert_eq!(parse_connector(Path::new("/tmp/c"), toml), Err(CaptureError::NotACaptureConnector));

        let part = "[metadata]\nclass_name = \"Part\"\n";
        assert_eq!(parse_connector(Path::new("/tmp/p"), part), Err(CaptureError::NotACaptureConnector));
    }

    #[test]
    fn a_connector_defaults_to_disabled_so_nothing_polls_unasked() {
        let toml = "[metadata]\nclass_name = \"Connector\"\n\n[attributes]\nprofile = \"grants_search2\"\n";
        let spec = parse_connector(Path::new("/tmp/c"), toml).expect("parses");
        assert!(!spec.enabled, "an unstated `enabled` must never mean yes");
    }

    #[test]
    fn a_reckless_poll_interval_is_clamped_to_the_floor() {
        let toml = "[metadata]\nclass_name = \"Connector\"\n\n[attributes]\nprofile = \"sam\"\npoll_seconds = 1\n";
        let spec = parse_connector(Path::new("/tmp/c"), toml).expect("parses");
        assert_eq!(spec.poll_seconds, MIN_POLL_SECONDS, "a 1s poll would burn the daily quota");
    }

    #[test]
    fn pagination_walks_every_page_and_stops_at_the_total() {
        let stub = Stub::new(vec![page(&["a", "b"], 4), page(&["c", "d"], 4)]);
        let payload = fetch_all(&sam_spec(), &stub, Some("KEY"), 10).expect("fetches");
        assert_eq!(payload.opportunities.len(), 4);
        assert_eq!(payload.pages_fetched, 2);
        let seen = stub.seen.lock().unwrap();
        assert!(seen[0].contains("offset=0"), "{}", seen[0]);
        assert!(seen[1].contains("offset=2"), "{}", seen[1]);
    }

    #[test]
    fn pagination_respects_the_page_cap_rather_than_running_away() {
        // The source claims 10,000 records and every page is full.
        let stub = Stub::new((0..20).map(|_| page(&["x", "y"], 10_000)).collect());
        let payload = fetch_all(&sam_spec(), &stub, Some("KEY"), 3).expect("fetches");
        assert_eq!(payload.pages_fetched, 3, "the cap bounds the walk");
        assert_eq!(payload.opportunities.len(), 6);
    }

    #[test]
    fn an_empty_page_terminates_even_when_the_total_disagrees() {
        let stub = Stub::new(vec![HttpResponse::new(
            200,
            r#"{"totalRecords":9999,"opportunitiesData":[]}"#,
        )]);
        let payload = fetch_all(&sam_spec(), &stub, Some("KEY"), 10).expect("fetches");
        assert_eq!(payload.pages_fetched, 1, "an empty page must not loop against a stale total");
    }

    #[test]
    fn a_rate_limit_stops_the_walk_instead_of_hammering() {
        let stub = Stub::new(vec![HttpResponse::new(429, "slow down")]);
        match fetch_all(&sam_spec(), &stub, Some("KEY"), 10) {
            Err(CaptureError::RateLimited { retry_after_secs }) => {
                // No interval: the transport seam does not surface response
                // headers, so the caller backs off by its own default rather
                // than inventing a shorter wait.
                assert_eq!(retry_after_secs, None);
            }
            other => panic!("expected a rate limit, got {other:?}"),
        }
        assert_eq!(
            stub.seen.lock().unwrap().len(),
            1,
            "a rate-limited walk must stop, not keep paging into the limit"
        );
    }

    #[test]
    fn an_http_error_names_the_status_without_leaking_the_key() {
        let stub = Stub::new(vec![HttpResponse::new(403, "forbidden")]);
        match fetch_all(&sam_spec(), &stub, Some("SUPERSECRET"), 10) {
            Err(CaptureError::Upstream(m)) => {
                assert!(m.contains("403"), "{m}");
                assert!(!m.contains("SUPERSECRET"), "the key leaked into an error: {m}");
            }
            other => panic!("expected an upstream error, got {other:?}"),
        }
    }

    #[test]
    fn a_sam_connector_without_a_date_window_says_what_to_set() {
        let mut spec = sam_spec();
        spec.options.clear();
        let stub = Stub::new(vec![]);
        match fetch_all(&spec, &stub, Some("KEY"), 1) {
            Err(CaptureError::BadQuery(m)) => {
                assert!(m.contains("posted_from"), "{m}");
                assert!(m.contains("one year"), "{m}");
            }
            other => panic!("expected a bad-query error, got {other:?}"),
        }
        assert!(stub.seen.lock().unwrap().is_empty(), "a bad query must never reach the network");
    }

    #[test]
    fn grants_needs_no_key_and_posts_its_filters() {
        let mut options = HashMap::new();
        options.insert("aln".into(), "10.225".into());
        options.insert("statuses".into(), "posted|forecasted".into());
        let spec = ConnectorSpec {
            dir: PathBuf::from("/tmp/Grants Source"),
            profile: Profile::GrantsSearch,
            endpoint: sources::GRANTS_SEARCH_URL.into(),
            enabled: true,
            poll_seconds: 3600,
            secret_ref: None,
            options,
        };
        assert_eq!(resolve_secret(&spec), Ok(None), "Grants.gov requires no credential");

        let stub = Stub::new(vec![HttpResponse::new(
            200,
            r#"{"errorcode":0,"data":{"hitCount":1,"oppHits":[{"id":"1","title":"T"}]}}"#,
        )]);
        let payload = fetch_all(&spec, &stub, None, 10).expect("fetches");
        assert_eq!(payload.opportunities.len(), 1);
    }

    #[test]
    fn a_missing_sam_key_names_the_variable_not_the_value() {
        let mut spec = sam_spec();
        spec.secret_ref = Some("A_VARIABLE_THAT_IS_NOT_SET_12345".into());
        match resolve_secret(&spec) {
            Err(CaptureError::MissingSecret(m)) => {
                assert!(m.contains("A_VARIABLE_THAT_IS_NOT_SET_12345"), "{m}");
            }
            other => panic!("expected a missing-secret error, got {other:?}"),
        }
    }

    #[test]
    fn discovery_ignores_a_directory_with_no_instance_file() {
        let root = std::env::temp_dir().join(format!("eustress-poll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ds = root.join("DataService");
        std::fs::create_dir_all(ds.join("Empty Folder")).unwrap();
        std::fs::create_dir_all(ds.join("SAM Source")).unwrap();
        std::fs::write(
            ds.join("SAM Source").join("_instance.toml"),
            "[metadata]\nclass_name = \"Connector\"\n\n[attributes]\nprofile = \"sam\"\nenabled = true\n",
        )
        .unwrap();

        let found = discover(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].profile, Profile::SamOpportunities);
        let _ = std::fs::remove_dir_all(&root);
    }
}
