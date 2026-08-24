//! The capture domain: one federal opportunity, the firm that answers it, and
//! the vocabulary shared by screening, scoring, drafting, and the deal room.
//!
//! Everything here is pure data plus pure functions. No Bevy, no filesystem, no
//! network. That is deliberate: screening decides whether a firm spends a week
//! of unpaid labour on a bid, so it has to be testable offline, in CI, with no
//! account and no key.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// Which public system a notice came from.
///
/// The two have different id spaces, different deadline semantics, and
/// different response templates, so the origin has to survive ingest rather
/// than being flattened into one shape that fits neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeSource {
    /// SAM.gov contract opportunities (FAR).
    Sam,
    /// Grants.gov funding opportunities (2 CFR 200).
    Grants,
}

impl NoticeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sam => "sam",
            Self::Grants => "grants",
        }
    }
}

/// A federal set-aside category.
///
/// Modelled as an enum rather than a bare string because eligibility is a
/// *decision*, and a typo in a string comparison silently drops every
/// opportunity a firm was actually qualified for. `Other` keeps unknown codes
/// visible instead of discarding them: a code we do not recognise must never
/// read as "unrestricted".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetAside {
    /// Full and open competition.
    None,
    TotalSmallBusiness,
    PartialSmallBusiness,
    EightA,
    HubZone,
    ServiceDisabledVeteran,
    Veteran,
    WomanOwned,
    EconomicallyDisadvantagedWomanOwned,
    /// A code the parser did not recognise. Treated as restricted.
    Other(String),
}

impl SetAside {
    /// Parse a SAM.gov `typeOfSetAside` code.
    ///
    /// SAM uses short codes; the same category appears with a `C` (total) or
    /// `S` (partial) suffix. Both map to the same certification requirement,
    /// because a firm either holds the certification or it does not.
    pub fn parse_sam(code: &str) -> Self {
        match code.trim().to_ascii_uppercase().as_str() {
            "" | "NONE" => Self::None,
            "SBA" | "SBP" => Self::TotalSmallBusiness,
            "8A" | "8AN" => Self::EightA,
            "HZC" | "HZS" => Self::HubZone,
            "SDVOSBC" | "SDVOSBS" => Self::ServiceDisabledVeteran,
            "VSA" | "VSS" => Self::Veteran,
            "WOSB" | "WOSBSS" => Self::WomanOwned,
            "EDWOSB" | "EDWOSBSS" => Self::EconomicallyDisadvantagedWomanOwned,
            other => Self::Other(other.to_string()),
        }
    }

    /// Human label for the ribbon, the deal room billboard, and the audit file.
    pub fn label(&self) -> String {
        match self {
            Self::None => "Unrestricted".into(),
            Self::TotalSmallBusiness => "Small Business".into(),
            Self::PartialSmallBusiness => "Partial Small Business".into(),
            Self::EightA => "8(a)".into(),
            Self::HubZone => "HUBZone".into(),
            Self::ServiceDisabledVeteran => "SDVOSB".into(),
            Self::Veteran => "VOSB".into(),
            Self::WomanOwned => "WOSB".into(),
            Self::EconomicallyDisadvantagedWomanOwned => "EDWOSB".into(),
            Self::Other(c) => format!("Set-aside ({c})"),
        }
    }

    /// Whether competition is restricted to holders of a certification.
    pub fn is_restricted(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// One solicitation or funding announcement, normalized across both sources.
///
/// Field names stay close to the source vocabulary so an auditor comparing this
/// against the raw payload does not have to learn a second set of words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opportunity {
    /// Stable id from the source: SAM `noticeId`, or Grants `opportunityId`.
    pub notice_id: String,
    pub source: NoticeSource,
    pub title: String,
    /// Issuing agency or department, as published.
    pub agency: String,
    /// The solicitation or funding-opportunity number a human would quote.
    pub solicitation_number: String,
    /// Primary NAICS code (SAM) or ALN/CFDA number (Grants). Empty when the
    /// source did not publish one.
    pub classification: String,
    pub set_aside: SetAside,
    /// Response deadline in UTC. `None` when the source published none, which
    /// is a real state for sources-sought and special notices.
    pub deadline: Option<DateTime<Utc>>,
    pub posted: Option<DateTime<Utc>>,
    /// Award ceiling in whole dollars, when published.
    pub ceiling: Option<i64>,
    /// Place of performance, two-letter state where the source gives one.
    pub place_of_performance: Option<String>,
    /// Notice type, e.g. `Solicitation`, `Presolicitation`, `Sources Sought`.
    pub notice_type: String,
    /// Canonical public URL for a human to open.
    pub url: String,
    /// Links to attached documents (SOW, PWS, RFP, NOFO). Metadata only; the
    /// fetcher resolves these separately.
    pub attachments: Vec<String>,
    /// Free-text description as published, unmodified.
    pub description: String,
    /// Anything the source published that this struct has no field for. Kept
    /// so ingest is lossless enough to re-derive a decision later.
    pub extra: BTreeMap<String, String>,
}

impl Opportunity {
    /// A blank notice from one source. Parsers fill what the payload carries.
    pub fn empty(source: NoticeSource, notice_id: impl Into<String>) -> Self {
        Self {
            notice_id: notice_id.into(),
            source,
            title: String::new(),
            agency: String::new(),
            solicitation_number: String::new(),
            classification: String::new(),
            set_aside: SetAside::None,
            deadline: None,
            posted: None,
            ceiling: None,
            place_of_performance: None,
            notice_type: String::new(),
            url: String::new(),
            attachments: Vec::new(),
            description: String::new(),
            extra: BTreeMap::new(),
        }
    }

    /// Whole days from `now` until the deadline. Negative once it has passed,
    /// `None` when no deadline was published.
    ///
    /// Deliberately whole days rather than a duration: every downstream display
    /// is "N days left", and rounding once here keeps the screen, the score,
    /// and the deal-room label from disagreeing by a day at the boundary.
    pub fn days_until_deadline(&self, now: DateTime<Utc>) -> Option<i64> {
        self.deadline.map(|d| (d - now).num_days())
    }

    /// Whether the response window is still open at `now`.
    ///
    /// A notice with no published deadline counts as open: sources-sought and
    /// special notices routinely omit one, and silently dropping them would
    /// hide the earliest and most valuable signal in the pipeline.
    pub fn is_open(&self, now: DateTime<Utc>) -> bool {
        self.deadline.map_or(true, |d| d > now)
    }

    /// Directory-safe form of the notice id, for the on-disk audit trail.
    ///
    /// Grants publishes bare integers and SAM publishes 32-character hex, but
    /// neither is guaranteed, so anything outside `[A-Za-z0-9._-]` collapses to
    /// an underscore rather than being trusted into a path.
    pub fn slug(&self) -> String {
        let s: String = self
            .notice_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
            .collect();
        let trimmed = s.trim_matches('_').to_string();
        if trimmed.is_empty() {
            format!("{}_unnamed", self.source.as_str())
        } else {
            format!("{}-{trimmed}", self.source.as_str())
        }
    }
}

/// The firm answering. Screening and scoring run against this and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapabilityStatement {
    pub legal_name: String,
    /// Unique Entity ID from SAM registration.
    pub uei: String,
    /// NAICS codes the firm is registered under.
    pub naics: Vec<String>,
    /// Certifications held, which is what set-aside eligibility tests against.
    pub certifications: Vec<SetAside>,
    /// States the firm will perform in. Empty means no geographic limit.
    pub states: Vec<String>,
    /// Largest contract the firm can staff, in whole dollars. `None` = no cap.
    pub ceiling_capacity: Option<i64>,
    /// Smallest contract worth the bid cost. Bids below this lose money even
    /// when won, which is the most common unforced error in small-firm capture.
    pub floor: Option<i64>,
    /// Minimum days needed to produce a compliant response.
    pub min_response_days: i64,
    /// Short capability paragraphs keyed by topic, used to seed draft sections.
    pub capabilities: BTreeMap<String, String>,
    /// Past performance references, newest first.
    pub past_performance: Vec<PastPerformance>,
}

impl CapabilityStatement {
    /// Whether the firm holds the certification a set-aside requires.
    ///
    /// Unrestricted competition is open to everyone. An unrecognised code is
    /// restricted and NOT held, so a parser gap costs a missed bid rather than
    /// a wasted one: the failure that shows up in a report beats the failure
    /// that shows up as a rejected proposal.
    pub fn eligible_for(&self, set_aside: &SetAside) -> bool {
        match set_aside {
            SetAside::None => true,
            // A total small-business set-aside is open to any small business,
            // which every other certification here already implies.
            SetAside::TotalSmallBusiness | SetAside::PartialSmallBusiness => {
                !self.certifications.is_empty()
            }
            other => self.certifications.contains(other),
        }
    }

    /// Whether a NAICS or ALN code is one the firm is registered under.
    ///
    /// Matches on the six-digit code, and also accepts a five-digit industry
    /// prefix so a firm registered at the industry level still sees its own
    /// sub-codes.
    pub fn covers_classification(&self, code: &str) -> bool {
        let code = code.trim();
        if code.is_empty() || self.naics.is_empty() {
            return false;
        }
        self.naics.iter().any(|n| {
            let n = n.trim();
            n == code || (n.len() >= 5 && code.len() >= 5 && n[..5] == code[..5])
        })
    }
}

/// One past-performance reference, used both for scoring and for draft seeding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PastPerformance {
    pub customer: String,
    pub title: String,
    pub naics: String,
    pub value: Option<i64>,
    pub year: Option<i32>,
    /// Whether the firm won this one. Populated outcomes are what the
    /// calibration harness scores against.
    pub won: Option<bool>,
    pub summary: String,
}

/// Parse a SAM.gov date, which arrives in several shapes across fields.
///
/// SAM publishes `postedDate` as `yyyy-MM-dd` and response deadlines as an
/// offset timestamp, and older records carry a bare `MM/dd/yyyy`. A deadline is
/// the number that decides whether a firm can respond at all, so all three are
/// handled here rather than being left to whichever call site hits them first.
///
/// A date with no time component resolves to 23:59:59 UTC, not midnight. A
/// federal deadline is the end of its published day, and rounding it to the
/// start silently discards the final 24 hours of a response window.
pub fn parse_sam_date(raw: &str) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    // Full RFC 3339 with an offset, which is what response deadlines carry.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // `yyyy-MM-dd'T'HH:mm:ss` with no offset. Documented as Eastern, but the
    // absence of an offset means we cannot prove it, so it is read as UTC and
    // the ambiguity stays visible instead of being invented away.
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(Utc.from_utc_datetime(&ndt));
    }
    for fmt in ["%Y-%m-%d", "%m/%d/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return end_of_day(d);
        }
    }
    None
}

/// Parse a Grants.gov date, which is `MM/dd/yyyy` throughout.
pub fn parse_grants_date(raw: &str) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(s, "%m/%d/%Y").ok().and_then(end_of_day)
}

/// The last instant of a calendar day, in UTC. See [`parse_sam_date`].
fn end_of_day(d: NaiveDate) -> Option<DateTime<Utc>> {
    d.and_hms_opt(23, 59, 59).map(|ndt| Utc.from_utc_datetime(&ndt))
}

/// Parse a published dollar amount into whole dollars.
///
/// Award ceilings arrive as `"$1,500,000.00"`, `"1500000"`, or an empty string,
/// and occasionally as a float in a JSON number. Anything unparseable is `None`
/// rather than zero: a missing ceiling and a ceiling of nothing are different
/// facts, and conflating them would let a screen reject real work.
pub fn parse_money(raw: &str) -> Option<i64> {
    let cleaned: String =
        raw.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    if cleaned.is_empty() {
        return None;
    }
    if let Ok(i) = cleaned.parse::<i64>() {
        return Some(i);
    }
    cleaned.parse::<f64>().ok().map(|f| f.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    #[test]
    fn set_aside_codes_map_total_and_partial_to_one_certification() {
        assert_eq!(SetAside::parse_sam("SDVOSBC"), SetAside::ServiceDisabledVeteran);
        assert_eq!(SetAside::parse_sam("SDVOSBS"), SetAside::ServiceDisabledVeteran);
        assert_eq!(SetAside::parse_sam("8AN"), SetAside::EightA);
        assert_eq!(SetAside::parse_sam(""), SetAside::None);
    }

    #[test]
    fn an_unknown_set_aside_code_is_restricted_not_open() {
        let unknown = SetAside::parse_sam("ZZZ");
        assert_eq!(unknown, SetAside::Other("ZZZ".into()));
        assert!(unknown.is_restricted(), "an unparsed code must never read as unrestricted");
        let firm = CapabilityStatement {
            certifications: vec![SetAside::EightA],
            ..Default::default()
        };
        assert!(!firm.eligible_for(&unknown));
    }

    #[test]
    fn a_date_with_no_time_is_the_end_of_its_day() {
        // The whole point: 2026-09-01 as a deadline means the firm has all of
        // 1 September, not none of it.
        let d = parse_sam_date("2026-09-01").expect("parses");
        assert_eq!(d.to_rfc3339(), "2026-09-01T23:59:59+00:00");
        let g = parse_grants_date("09/01/2026").expect("parses");
        assert_eq!(d, g, "both sources must land on the same instant");
    }

    #[test]
    fn an_offset_deadline_is_converted_not_truncated() {
        // 2026-09-01 17:00 Eastern is 21:00 UTC the same day.
        let d = parse_sam_date("2026-09-01T17:00:00-04:00").expect("parses");
        assert_eq!(d.to_rfc3339(), "2026-09-01T21:00:00+00:00");
    }

    #[test]
    fn days_until_deadline_counts_whole_days() {
        let mut o = Opportunity::empty(NoticeSource::Sam, "abc");
        o.deadline = parse_sam_date("2026-09-10");
        assert_eq!(o.days_until_deadline(utc(2026, 9, 1)), Some(9));
        assert!(o.is_open(utc(2026, 9, 1)));
        assert!(!o.is_open(utc(2026, 9, 20)));
    }

    #[test]
    fn a_notice_with_no_deadline_stays_open() {
        let o = Opportunity::empty(NoticeSource::Sam, "abc");
        assert!(o.is_open(utc(2026, 9, 1)), "sources-sought notices publish no deadline");
        assert_eq!(o.days_until_deadline(utc(2026, 9, 1)), None);
    }

    #[test]
    fn naics_matches_exactly_or_at_the_industry_prefix() {
        let firm = CapabilityStatement { naics: vec!["541511".into()], ..Default::default() };
        assert!(firm.covers_classification("541511"));
        assert!(firm.covers_classification("541512"), "same five-digit industry");
        assert!(!firm.covers_classification("236220"));
        assert!(!firm.covers_classification(""), "an empty code matches nothing");
    }

    #[test]
    fn slug_is_path_safe_and_names_its_source() {
        let mut o = Opportunity::empty(NoticeSource::Grants, "../../etc/passwd");
        assert_eq!(o.slug(), "grants-.._.._etc_passwd");
        o.notice_id = "3f9a1b".into();
        assert_eq!(o.slug(), "grants-3f9a1b");
    }

    #[test]
    fn money_distinguishes_missing_from_zero() {
        assert_eq!(parse_money("$1,500,000.00"), Some(1_500_000));
        assert_eq!(parse_money("250000"), Some(250_000));
        assert_eq!(parse_money(""), None);
        assert_eq!(parse_money("N/A"), None);
        assert_eq!(parse_money("0"), Some(0));
    }

    #[test]
    fn small_business_set_aside_accepts_any_certified_firm() {
        let hz = CapabilityStatement {
            certifications: vec![SetAside::HubZone],
            ..Default::default()
        };
        assert!(hz.eligible_for(&SetAside::TotalSmallBusiness));
        assert!(!hz.eligible_for(&SetAside::EightA), "HUBZone is not 8(a)");
        let none = CapabilityStatement::default();
        assert!(none.eligible_for(&SetAside::None));
        assert!(!none.eligible_for(&SetAside::TotalSmallBusiness));
    }
}
