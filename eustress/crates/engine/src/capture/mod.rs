//! Government opportunity capture: solicitation to first draft.
//!
//! The sell-side counterpart to Government mode's `procurement` discipline.
//! That one models a city issuing an RFP; this one models a small contractor,
//! grant writer, or nonprofit answering one.
//!
//! # Shape
//!
//! ```text
//! Connector (_instance.toml, enabled = true)
//!   └─ poll        SAM.gov / Grants.gov over the shared HTTP seam
//!        └─ screen deterministic gates: NAICS, set-aside, deadline, capacity
//!             └─ score      per-criterion fit, with a no-bid recommendation
//!                  └─ extract   every "shall" in the attached statement of work
//!                       └─ matrix   requirement to response section, gaps shown
//!                            └─ outline  a seeded first draft, every claim cited
//!                                 └─ audit  one folder per notice, `cat`-readable
//! ```
//!
//! # Two rules
//!
//! **The engine drafts; a human files.** Nothing here submits a bid, signs a
//! certification, or transmits to either system. Both APIs are read-only in
//! this code path and there is no write path to add later by accident.
//!
//! **Everything is falsifiable.** [`score::calibrate`] scores the ranking
//! against outcomes that already happened, and [`compliance::ComplianceMatrix::gaps`]
//! reports what the firm cannot answer. A capture tool that only ever
//! encourages the bid is a lead generator for whoever sold it.
//!
//! # Layering
//!
//! Every module except [`poll`] and [`layout`] is pure: no Bevy, no clock, no
//! filesystem, no network. That is what lets the decision path be tested
//! offline with no account and no API key, which matters because these
//! decisions cost a firm a week of unpaid labour each.

use bevy::prelude::*;

pub mod audit;
pub mod compliance;
pub mod credentials;
pub mod layout;
pub mod model;
pub mod outline;
pub mod poll;
pub mod requirements;
pub mod ribbon;
pub mod score;
pub mod screen;
pub mod sources;
pub mod store;

pub use model::{CapabilityStatement, NoticeSource, Opportunity, PastPerformance, SetAside};

/// Anything that can go wrong between a Connector and a draft.
///
/// Deliberately not a catch-all string: a missing API key, a rate limit, and a
/// schema change need different responses from the user, and flattening them
/// into one message means every failure looks like "it broke".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// A required secret was not resolvable. Names what is missing, never its
    /// value.
    MissingSecret(String),
    /// The query could not be built from the Connector's configuration.
    BadQuery(String),
    /// The transport failed: DNS, connect, TLS, read.
    Transport(String),
    /// The source answered, and the answer was an error.
    Upstream(String),
    /// The source answered with something this parser does not recognise.
    Parse(String),
    /// The source asked us to slow down. Carries seconds to wait when it said.
    RateLimited { retry_after_secs: Option<u64> },
    /// Filesystem failure writing the audit trail.
    Io(String),
    /// The Connector is not one of the capture profiles.
    NotACaptureConnector,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSecret(m) => write!(f, "missing credential: {m}"),
            Self::BadQuery(m) => write!(f, "bad query: {m}"),
            Self::Transport(m) => write!(f, "transport failure: {m}"),
            Self::Upstream(m) => write!(f, "{m}"),
            Self::Parse(m) => write!(f, "unrecognized response: {m}"),
            Self::RateLimited { retry_after_secs: Some(s) } => {
                write!(f, "rate limited; retry in {s}s")
            }
            Self::RateLimited { retry_after_secs: None } => write!(f, "rate limited"),
            Self::Io(m) => write!(f, "io: {m}"),
            Self::NotACaptureConnector => {
                write!(f, "this Connector is not a SAM.gov or Grants.gov source")
            }
        }
    }
}

impl std::error::Error for CaptureError {}

/// A whole pipeline run over one batch of notices, ready to be written to disk.
#[derive(Debug, Clone)]
pub struct PipelineRun {
    pub screened: usize,
    pub passed: Vec<Assessment>,
    pub rejected: Vec<screen::ScreenResult>,
    /// Rejection reasons by frequency. The report that tells a firm which one
    /// registration would unlock the most pipeline.
    pub top_reasons: Vec<(String, usize)>,
}

impl PipelineRun {
    /// One line for the ribbon, the status bar, and the log.
    pub fn headline(&self) -> String {
        let bids = self
            .passed
            .iter()
            .filter(|a| matches!(a.score.recommendation, score::Recommendation::Bid))
            .count();
        format!(
            "{} screened, {} eligible, {} worth bidding",
            self.screened,
            self.passed.len(),
            bids
        )
    }

    /// Eligible notices, best fit first.
    pub fn ranked(&self) -> Vec<&Assessment> {
        let mut v: Vec<&Assessment> = self.passed.iter().collect();
        v.sort_by(|a, b| {
            b.score
                .total
                .partial_cmp(&a.score.total)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.opportunity.notice_id.cmp(&b.opportunity.notice_id))
        });
        v
    }
}

/// Everything the pipeline concluded about one notice that cleared screening.
#[derive(Debug, Clone)]
pub struct Assessment {
    pub opportunity: Opportunity,
    pub screen: screen::ScreenResult,
    pub score: score::FitScore,
    /// Present only when requirement text was available to extract from.
    pub matrix: Option<compliance::ComplianceMatrix>,
    pub outline: Option<outline::ResponseOutline>,
}

/// Run screening, scoring, and (where text allows) drafting over one batch.
///
/// `documents` maps a notice id to its requirement text, from a fetched
/// attachment or the notice's own description. A notice with no text still gets
/// screened and scored: the matrix is simply absent, which reads differently
/// from a matrix with no rows and must.
pub fn run_pipeline(
    opportunities: &[Opportunity],
    firm: &CapabilityStatement,
    documents: &std::collections::BTreeMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
) -> PipelineRun {
    let summary = screen::screen_all(opportunities, firm, now);
    let by_id: std::collections::BTreeMap<&str, &Opportunity> =
        opportunities.iter().map(|o| (o.notice_id.as_str(), o)).collect();

    let mut passed = Vec::new();
    for s in &summary.passed {
        let Some(o) = by_id.get(s.notice_id.as_str()) else { continue };
        let fit = score::score(o, firm, now);

        let (matrix, plan) = match documents.get(&o.notice_id) {
            Some(text) if !text.trim().is_empty() => {
                let reqs = requirements::extract(text);
                let m = compliance::build(&o.notice_id, &reqs, &firm.capabilities);
                let p = outline::build(o, &m, &firm.capabilities);
                (Some(m), Some(p))
            }
            _ => (None, None),
        };

        passed.push(Assessment {
            opportunity: (*o).clone(),
            screen: s.clone(),
            score: fit,
            matrix,
            outline: plan,
        });
    }

    PipelineRun {
        screened: summary.screened,
        passed,
        top_reasons: summary.top_reasons(),
        rejected: summary.rejected,
    }
}

/// Write a whole run to the Space's `Capture/` folder.
///
/// Returns one receipt per notice. A failure on one notice does not abort the
/// rest: a single unwritable folder must not cost a firm the other 22
/// assessments in the batch.
pub fn write_run(
    space_root: &std::path::Path,
    run: &PipelineRun,
    fetched_at: &str,
) -> (Vec<audit::WriteReceipt>, Vec<CaptureError>) {
    let mut receipts = Vec::new();
    let mut errors = Vec::new();
    for a in &run.passed {
        let record = audit::CaptureRecord {
            opportunity: &a.opportunity,
            raw_source: None,
            screen: Some(&a.screen),
            score: Some(&a.score),
            matrix: a.matrix.as_ref(),
            outline: a.outline.as_ref(),
            fetched_at: fetched_at.to_string(),
        };
        match audit::write_record(space_root, &record) {
            Ok(r) => receipts.push(r),
            Err(e) => errors.push(e),
        }
    }
    (receipts, errors)
}

/// Registers the capture runtime: Connector polling and deal-room layout.
pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(poll::ConnectorPollPlugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::BTreeMap;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap()
    }

    fn firm() -> CapabilityStatement {
        let mut capabilities = BTreeMap::new();
        capabilities.insert(
            "help_desk".into(),
            "Tier 2 help desk support during core hours with triage and escalation.".into(),
        );
        CapabilityStatement {
            naics: vec!["541511".into()],
            certifications: vec![SetAside::ServiceDisabledVeteran],
            ceiling_capacity: Some(2_000_000),
            floor: Some(50_000),
            min_response_days: 10,
            capabilities,
            past_performance: vec![PastPerformance {
                customer: "US Army".into(),
                title: "Prior help desk".into(),
                naics: "541511".into(),
                value: Some(600_000),
                year: Some(2025),
                won: Some(true),
                summary: String::new(),
            }],
            ..Default::default()
        }
    }

    fn notice(id: &str, naics: &str) -> Opportunity {
        let mut o = Opportunity::empty(NoticeSource::Sam, id);
        o.title = format!("Notice {id}");
        o.classification = naics.into();
        o.set_aside = SetAside::ServiceDisabledVeteran;
        o.deadline = Some(chrono::Utc.with_ymd_and_hms(2026, 9, 30, 23, 59, 59).unwrap());
        o.ceiling = Some(800_000);
        o
    }

    #[test]
    fn the_pipeline_separates_eligible_from_ineligible_and_says_why() {
        let good = notice("good", "541511");
        let wrong_naics = notice("wrong", "236220");
        let run = run_pipeline(&[good, wrong_naics], &firm(), &BTreeMap::new(), now());

        assert_eq!(run.screened, 2);
        assert_eq!(run.passed.len(), 1);
        assert_eq!(run.rejected.len(), 1);
        assert_eq!(run.top_reasons[0].0, "wrong classification");
        assert!(run.headline().starts_with("2 screened, 1 eligible"), "{}", run.headline());
    }

    #[test]
    fn a_notice_with_no_document_is_still_screened_and_scored() {
        let run = run_pipeline(&[notice("n1", "541511")], &firm(), &BTreeMap::new(), now());
        let a = &run.passed[0];
        assert!(a.matrix.is_none(), "absent text must read as absent, not as zero requirements");
        assert!(a.outline.is_none());
        assert!(a.score.total > 0.0, "scoring does not depend on the document");
    }

    #[test]
    fn a_notice_with_a_document_gets_a_matrix_and_a_draft() {
        let mut docs = BTreeMap::new();
        docs.insert(
            "n1".to_string(),
            "C.1 The Contractor shall provide Tier 2 help desk support during core hours \
             with triage and escalation.\n\
             C.2 The Contractor shall hold an active facility clearance."
                .to_string(),
        );
        let run = run_pipeline(&[notice("n1", "541511")], &firm(), &docs, now());
        let a = &run.passed[0];
        let m = a.matrix.as_ref().expect("matrix built");
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.gaps().len(), 1, "the clearance requirement is unanswered");
        let p = a.outline.as_ref().expect("outline built");
        assert!(!p.blocked().is_empty(), "the gap must block a section");
    }

    #[test]
    fn ranking_is_deterministic_across_runs() {
        let batch = [notice("b", "541511"), notice("a", "541511"), notice("c", "541511")];
        let first: Vec<String> = run_pipeline(&batch, &firm(), &BTreeMap::new(), now())
            .ranked()
            .iter()
            .map(|a| a.opportunity.notice_id.clone())
            .collect();
        let second: Vec<String> = run_pipeline(&batch, &firm(), &BTreeMap::new(), now())
            .ranked()
            .iter()
            .map(|a| a.opportunity.notice_id.clone())
            .collect();
        assert_eq!(first, second, "a re-run must produce a diffable identical order");
        assert_eq!(first, vec!["a", "b", "c"], "equal scores break on id");
    }

    #[test]
    fn a_run_writes_one_auditable_folder_per_eligible_notice() {
        let root = std::env::temp_dir()
            .join(format!("eustress-capture-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let run = run_pipeline(
            &[notice("n1", "541511"), notice("n2", "236220")],
            &firm(),
            &BTreeMap::new(),
            now(),
        );
        let (receipts, errors) = write_run(&root, &run, "2026-08-23T12:00:00Z");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(receipts.len(), 1, "only the eligible notice is filed");

        // The folder answers "why" without the engine.
        let score_txt = std::fs::read_to_string(receipts[0].directory.join("score.txt")).unwrap();
        assert!(score_txt.contains("past performance"), "{score_txt}");
        assert!(audit::verify_record(&receipts[0].directory).unwrap().intact);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn errors_report_what_is_missing_without_leaking_it() {
        let e = CaptureError::MissingSecret("SAM_API_KEY".into());
        let msg = e.to_string();
        assert!(msg.contains("SAM_API_KEY"), "{msg}");
        assert!(msg.contains("missing credential"), "{msg}");
    }
}
