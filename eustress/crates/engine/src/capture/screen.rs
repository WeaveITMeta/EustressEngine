//! Deterministic screening: the hard gates that decide whether a firm *can*
//! respond at all, before anything expensive runs.
//!
//! This is the highest-leverage code in the capture pipeline and the least
//! glamorous. Screening 500 notices down to the 20 a firm is actually eligible
//! for needs no model, no embedding, and no network. It needs NAICS, a set-aside
//! certification, a calendar, and a capacity ceiling.
//!
//! Every rejection carries its reason. A screen that returns a shorter list
//! without saying why is unauditable, and a firm cannot tell a good filter from
//! a broken one.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::model::{CapabilityStatement, Opportunity};

/// Why a notice did not survive screening.
///
/// Ordered by how early the gate runs, so the first failure reported is the
/// most fundamental one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rejection {
    /// The response window has already closed.
    Closed { days_past: i64 },
    /// The window closes sooner than the firm can produce a compliant response.
    TooSoon { days_left: i64, days_needed: i64 },
    /// The firm does not hold the required certification.
    NotEligible { required: String },
    /// The classification code is outside the firm's registrations.
    WrongClassification { code: String },
    /// Larger than the firm can staff.
    OverCapacity { ceiling: i64, capacity: i64 },
    /// Too small to cover the cost of bidding.
    BelowFloor { ceiling: i64, floor: i64 },
    /// Outside the firm's stated geography.
    WrongGeography { state: String },
}

impl Rejection {
    /// One line a human can act on, for the report and the audit file.
    pub fn explain(&self) -> String {
        match self {
            Self::Closed { days_past } => {
                format!("response window closed {days_past} day(s) ago")
            }
            Self::TooSoon { days_left, days_needed } => format!(
                "{days_left} day(s) left, {days_needed} needed to respond"
            ),
            Self::NotEligible { required } => {
                format!("restricted to {required}, which the firm does not hold")
            }
            Self::WrongClassification { code } => {
                format!("classification {code} is outside the firm's registrations")
            }
            Self::OverCapacity { ceiling, capacity } => {
                format!("ceiling ${ceiling} exceeds capacity ${capacity}")
            }
            Self::BelowFloor { ceiling, floor } => {
                format!("ceiling ${ceiling} is below the ${floor} bid floor")
            }
            Self::WrongGeography { state } => {
                format!("performance in {state} is outside the firm's stated geography")
            }
        }
    }
}

/// The outcome of screening one notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenResult {
    pub notice_id: String,
    /// Empty means the notice passed every gate.
    pub rejections: Vec<Rejection>,
    /// Gates that could not run because the source published nothing to test.
    ///
    /// Kept separate from a rejection on purpose: "no ceiling published" is not
    /// evidence the work is too large, and treating unknown as failure would
    /// throw away most sources-sought notices, which is where the early and
    /// most winnable work lives.
    pub unknowns: Vec<String>,
}

impl ScreenResult {
    pub fn passed(&self) -> bool {
        self.rejections.is_empty()
    }

    /// Every rejection reason, newline-joined, for the on-disk record.
    pub fn explain(&self) -> String {
        self.rejections.iter().map(Rejection::explain).collect::<Vec<_>>().join("\n")
    }
}

/// Run every hard gate against one notice.
///
/// Collects all failures rather than short-circuiting on the first. A firm
/// deciding whether to chase a waiver, add a NAICS registration, or partner on
/// a bid needs the whole picture, and re-running the screen once per question
/// would be the slow way to get it.
pub fn screen(
    opportunity: &Opportunity,
    firm: &CapabilityStatement,
    now: DateTime<Utc>,
) -> ScreenResult {
    let mut rejections = Vec::new();
    let mut unknowns = Vec::new();

    match opportunity.days_until_deadline(now) {
        // Closedness is decided by `is_open`, which compares instants, NOT by
        // `days < 0`. Whole-day counting truncates toward zero, so a notice
        // that closed six hours ago counts as 0 days and would slip past a
        // negative test into TooSoon: rejected either way, but for the wrong
        // reason, and passable outright for a firm with no turnaround minimum.
        Some(days) if !opportunity.is_open(now) => {
            rejections.push(Rejection::Closed { days_past: (-days).max(0) })
        }
        Some(days) if days < firm.min_response_days => rejections.push(Rejection::TooSoon {
            days_left: days,
            days_needed: firm.min_response_days,
        }),
        Some(_) => {}
        None => unknowns.push("no response deadline published".into()),
    }

    if opportunity.set_aside.is_restricted() && !firm.eligible_for(&opportunity.set_aside) {
        rejections
            .push(Rejection::NotEligible { required: opportunity.set_aside.label() });
    }

    if opportunity.classification.trim().is_empty() {
        unknowns.push("no NAICS or ALN published".into());
    } else if !firm.naics.is_empty() && !firm.covers_classification(&opportunity.classification) {
        rejections.push(Rejection::WrongClassification {
            code: opportunity.classification.clone(),
        });
    } else if firm.naics.is_empty() {
        unknowns.push("firm has no NAICS registrations to match against".into());
    }

    match opportunity.ceiling {
        Some(c) => {
            if let Some(cap) = firm.ceiling_capacity {
                if c > cap {
                    rejections.push(Rejection::OverCapacity { ceiling: c, capacity: cap });
                }
            }
            if let Some(floor) = firm.floor {
                if c < floor {
                    rejections.push(Rejection::BelowFloor { ceiling: c, floor });
                }
            }
        }
        None => unknowns.push("no award ceiling published".into()),
    }

    match opportunity.place_of_performance.as_deref() {
        Some(state) if !firm.states.is_empty() => {
            let matches = firm.states.iter().any(|s| s.eq_ignore_ascii_case(state));
            if !matches {
                rejections.push(Rejection::WrongGeography { state: state.to_string() });
            }
        }
        Some(_) => {}
        None => unknowns.push("no place of performance published".into()),
    }

    ScreenResult { notice_id: opportunity.notice_id.clone(), rejections, unknowns }
}

/// Screen a batch and report what survived and what the gates removed.
pub fn screen_all(
    opportunities: &[Opportunity],
    firm: &CapabilityStatement,
    now: DateTime<Utc>,
) -> ScreenSummary {
    let mut passed = Vec::new();
    let mut rejected = Vec::new();
    for o in opportunities {
        let r = screen(o, firm, now);
        if r.passed() {
            passed.push(r);
        } else {
            rejected.push(r);
        }
    }
    ScreenSummary { screened: opportunities.len(), passed, rejected }
}

/// Batch screening outcome. `screened` is the denominator every report quotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSummary {
    pub screened: usize,
    pub passed: Vec<ScreenResult>,
    pub rejected: Vec<ScreenResult>,
}

impl ScreenSummary {
    /// A one-line headline: "23 of 500 passed screening".
    pub fn headline(&self) -> String {
        format!("{} of {} passed screening", self.passed.len(), self.screened)
    }

    /// Rejection reasons by frequency, most common first.
    ///
    /// This is the report that tells a firm which single registration or
    /// certification would unlock the most pipeline, which is a better use of
    /// the data than the pass list alone.
    pub fn top_reasons(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for r in &self.rejected {
            for rej in &r.rejections {
                let key = match rej {
                    Rejection::Closed { .. } => "closed",
                    Rejection::TooSoon { .. } => "too soon",
                    Rejection::NotEligible { .. } => "not eligible",
                    Rejection::WrongClassification { .. } => "wrong classification",
                    Rejection::OverCapacity { .. } => "over capacity",
                    Rejection::BelowFloor { .. } => "below floor",
                    Rejection::WrongGeography { .. } => "wrong geography",
                };
                *counts.entry(key.to_string()).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::model::{NoticeSource, SetAside};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap()
    }

    fn firm() -> CapabilityStatement {
        CapabilityStatement {
            legal_name: "Example Systems LLC".into(),
            naics: vec!["541511".into()],
            certifications: vec![SetAside::ServiceDisabledVeteran],
            states: vec!["AZ".into()],
            ceiling_capacity: Some(2_000_000),
            floor: Some(50_000),
            min_response_days: 10,
            ..Default::default()
        }
    }

    fn winnable() -> Opportunity {
        let mut o = Opportunity::empty(NoticeSource::Sam, "good");
        o.classification = "541511".into();
        o.set_aside = SetAside::ServiceDisabledVeteran;
        o.deadline = Some(Utc.with_ymd_and_hms(2026, 9, 30, 23, 59, 59).unwrap());
        o.ceiling = Some(750_000);
        o.place_of_performance = Some("AZ".into());
        o
    }

    #[test]
    fn a_fully_matching_notice_passes_every_gate() {
        let r = screen(&winnable(), &firm(), now());
        assert!(r.passed(), "unexpected rejections: {:?}", r.rejections);
        assert!(r.unknowns.is_empty());
    }

    #[test]
    fn every_failing_gate_reports_not_just_the_first() {
        let mut o = winnable();
        o.set_aside = SetAside::EightA;
        o.classification = "236220".into();
        o.ceiling = Some(9_000_000);
        o.place_of_performance = Some("TX".into());

        let r = screen(&o, &firm(), now());
        assert!(!r.passed());
        assert_eq!(r.rejections.len(), 4, "all four gates report: {:?}", r.rejections);
        // The explanation must name the missing certification, since that is
        // the one a firm can actually act on.
        assert!(r.explain().contains("8(a)"), "{}", r.explain());
    }

    #[test]
    fn a_closed_window_is_rejected_and_says_how_late() {
        let mut o = winnable();
        o.deadline = Some(Utc.with_ymd_and_hms(2026, 8, 1, 23, 59, 59).unwrap());
        // 21 whole days elapsed, plus twelve hours. Whole-day counting reports
        // the 21 rather than rounding a partial day up.
        let r = screen(&o, &firm(), now());
        assert_eq!(r.rejections, vec![Rejection::Closed { days_past: 21 }]);
    }

    #[test]
    fn a_notice_that_closed_hours_ago_is_closed_not_merely_too_soon() {
        // The off-by-one that loses a bid, from the other side. Whole-day
        // counting truncates a six-hour overrun to zero, so a `days < 0` test
        // would call this TooSoon and a firm with no turnaround minimum would
        // see it pass screening outright.
        let mut o = winnable();
        o.deadline = Some(now() - chrono::Duration::hours(6));
        assert!(!o.is_open(now()));

        let r = screen(&o, &firm(), now());
        assert_eq!(r.rejections, vec![Rejection::Closed { days_past: 0 }]);

        let no_minimum = CapabilityStatement { min_response_days: 0, ..firm() };
        assert!(
            !screen(&o, &no_minimum, now()).passed(),
            "a closed notice must never pass, whatever the firm's turnaround"
        );
    }

    #[test]
    fn a_window_shorter_than_the_firms_turnaround_is_rejected() {
        let mut o = winnable();
        o.deadline = Some(Utc.with_ymd_and_hms(2026, 8, 27, 23, 59, 59).unwrap());
        let r = screen(&o, &firm(), now());
        assert_eq!(r.rejections, vec![Rejection::TooSoon { days_left: 4, days_needed: 10 }]);
    }

    #[test]
    fn unknown_is_not_failure() {
        // A sources-sought notice with no deadline, ceiling, or place of
        // performance. Every one of those is unknown, none is a rejection.
        let mut o = Opportunity::empty(NoticeSource::Sam, "sources-sought");
        o.classification = "541511".into();
        let r = screen(&o, &firm(), now());
        assert!(r.passed(), "unknowns must not reject: {:?}", r.rejections);
        assert_eq!(r.unknowns.len(), 3);
    }

    #[test]
    fn an_unrestricted_notice_is_open_to_a_firm_with_no_certifications() {
        let mut o = winnable();
        o.set_aside = SetAside::None;
        let bare = CapabilityStatement {
            naics: vec!["541511".into()],
            min_response_days: 10,
            ..Default::default()
        };
        assert!(screen(&o, &bare, now()).passed());
    }

    #[test]
    fn a_bid_below_the_floor_is_rejected_because_winning_it_loses_money() {
        let mut o = winnable();
        o.ceiling = Some(12_000);
        let r = screen(&o, &firm(), now());
        assert_eq!(r.rejections, vec![Rejection::BelowFloor { ceiling: 12_000, floor: 50_000 }]);
    }

    #[test]
    fn a_firm_with_no_geography_limit_is_not_screened_on_location() {
        let mut o = winnable();
        o.place_of_performance = Some("AK".into());
        let anywhere = CapabilityStatement { states: vec![], ..firm() };
        assert!(screen(&o, &anywhere, now()).passed());
    }

    #[test]
    fn the_summary_ranks_the_reason_that_costs_the_most_pipeline() {
        let mut wrong_cert = winnable();
        wrong_cert.notice_id = "a".into();
        wrong_cert.set_aside = SetAside::EightA;
        let mut wrong_cert2 = wrong_cert.clone();
        wrong_cert2.notice_id = "b".into();
        let mut too_big = winnable();
        too_big.notice_id = "c".into();
        too_big.ceiling = Some(50_000_000);

        let s = screen_all(&[winnable(), wrong_cert, wrong_cert2, too_big], &firm(), now());
        assert_eq!(s.passed.len(), 1);
        assert_eq!(s.headline(), "1 of 4 passed screening");
        let reasons = s.top_reasons();
        assert_eq!(reasons[0], ("not eligible".to_string(), 2));
        assert_eq!(reasons[1], ("over capacity".to_string(), 1));
    }
}
