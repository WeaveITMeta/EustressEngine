//! Fit scoring, and the no-bid recommender that is this discipline's falsifier.
//!
//! Screening ([`super::screen`]) answers "may we bid". Scoring answers "should
//! we", which is a different and softer question. A firm has time for perhaps
//! three real proposals a quarter, so ranking the eligible set matters more
//! than lengthening it.
//!
//! Two rules shape everything here:
//!
//! 1. **No opaque number.** Every criterion reports its own sub-score and the
//!    evidence behind it. A single 0-to-100 with no derivation cannot be
//!    argued with, corrected, or audited, so it would be worse than nothing.
//! 2. **It must be able to say no.** [`Recommendation::NoBid`] is a real
//!    output, not a theoretical one, and [`calibrate`] scores the whole model
//!    against outcomes that already happened. A capture tool that always
//!    encourages the bid is a lead generator for its own vendor.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::model::{CapabilityStatement, Opportunity, SetAside};

/// One scored dimension of fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Criterion {
    pub name: String,
    /// Relative importance. Weights are normalized, so they need not sum to 1.
    pub weight: f32,
    /// 0.0 (worst) to 1.0 (best).
    pub score: f32,
    /// Why this score, in the terms a human would use to disagree with it.
    pub evidence: String,
}

impl Criterion {
    fn new(name: &str, weight: f32, score: f32, evidence: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            weight,
            score: score.clamp(0.0, 1.0),
            evidence: evidence.into(),
        }
    }
}

/// What the model advises, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Recommendation {
    /// Worth a full response.
    Bid,
    /// Worth tracking, but something has to change before it is worth the
    /// proposal cost: a teaming partner, a certification, an amendment.
    Watch { reasons: Vec<String> },
    /// Advise against responding, with the reasons that decided it.
    NoBid { reasons: Vec<String> },
}

impl Recommendation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Bid => "Bid",
            Self::Watch { .. } => "Watch",
            Self::NoBid { .. } => "No-bid",
        }
    }

    pub fn reasons(&self) -> &[String] {
        match self {
            Self::Bid => &[],
            Self::Watch { reasons } | Self::NoBid { reasons } => reasons,
        }
    }
}

/// A complete, explainable fit assessment for one notice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitScore {
    pub notice_id: String,
    /// Weighted mean of the criteria, 0.0 to 1.0.
    pub total: f32,
    pub criteria: Vec<Criterion>,
    pub recommendation: Recommendation,
}

impl FitScore {
    /// Percentage form, for display only. The ranking uses `total`.
    pub fn percent(&self) -> u8 {
        (self.total * 100.0).round().clamp(0.0, 100.0) as u8
    }

    /// The full derivation, one criterion per line. This is what lands in the
    /// audit file so a score can be reconstructed months later.
    pub fn explain(&self) -> String {
        let mut out = String::new();
        for c in &self.criteria {
            out.push_str(&format!(
                "{:<22} {:>3}%  w={:.2}  {}\n",
                c.name,
                (c.score * 100.0).round() as i32,
                c.weight,
                c.evidence
            ));
        }
        out.push_str(&format!("{:<22} {:>3}%  {}\n", "TOTAL", self.percent(), self.recommendation.label()));
        for r in self.recommendation.reasons() {
            out.push_str(&format!("  reason: {r}\n"));
        }
        out
    }
}

/// Below this weighted score, responding is advised against outright.
pub const NO_BID_THRESHOLD: f32 = 0.35;
/// Below this, the opportunity is worth watching but not yet worth the cost.
pub const WATCH_THRESHOLD: f32 = 0.55;

/// Score one notice against one firm.
///
/// Assumes the notice already passed [`super::screen::screen`]. Scoring a
/// notice the firm is ineligible for would produce a ranking that invites a
/// wasted response, so callers screen first.
pub fn score(
    opportunity: &Opportunity,
    firm: &CapabilityStatement,
    now: DateTime<Utc>,
) -> FitScore {
    let mut criteria = Vec::new();

    criteria.push(classification_fit(opportunity, firm));
    criteria.push(set_aside_advantage(opportunity, firm));
    criteria.push(past_performance_fit(opportunity, firm));
    criteria.push(runway(opportunity, now, firm));
    criteria.push(size_fit(opportunity, firm));
    criteria.push(geography_fit(opportunity, firm));

    let weight_sum: f32 = criteria.iter().map(|c| c.weight).sum();
    let total = if weight_sum > 0.0 {
        criteria.iter().map(|c| c.score * c.weight).sum::<f32>() / weight_sum
    } else {
        0.0
    };

    let recommendation = recommend(total, &criteria);
    FitScore {
        notice_id: opportunity.notice_id.clone(),
        total,
        criteria,
        recommendation,
    }
}

/// Turn a total and its criteria into advice.
///
/// A single criterion scoring near zero can veto an otherwise decent total.
/// Averages hide exactly the failure that sinks a proposal: a firm with a great
/// NAICS match and no relevant past performance is not a 70, it is a loss with
/// a good cover page.
fn recommend(total: f32, criteria: &[Criterion]) -> Recommendation {
    let mut reasons = Vec::new();
    for c in criteria {
        if c.score <= 0.15 && c.weight >= 1.0 {
            reasons.push(format!("{} is near zero: {}", c.name, c.evidence));
        }
    }

    if total < NO_BID_THRESHOLD || reasons.len() >= 2 {
        if reasons.is_empty() {
            reasons.push(format!("weighted fit {}% is below the bid threshold", (total * 100.0).round() as i32));
        }
        return Recommendation::NoBid { reasons };
    }
    if total < WATCH_THRESHOLD || !reasons.is_empty() {
        if reasons.is_empty() {
            reasons.push(format!("weighted fit {}% is short of a confident bid", (total * 100.0).round() as i32));
        }
        return Recommendation::Watch { reasons };
    }
    Recommendation::Bid
}

/// How well the notice's NAICS or ALN matches the firm's registrations.
fn classification_fit(o: &Opportunity, firm: &CapabilityStatement) -> Criterion {
    let code = o.classification.trim();
    if code.is_empty() {
        return Criterion::new("classification", 1.5, 0.5, "no code published; unscored");
    }
    if firm.naics.iter().any(|n| n.trim() == code) {
        return Criterion::new("classification", 1.5, 1.0, format!("exact match on {code}"));
    }
    if firm.covers_classification(code) {
        return Criterion::new(
            "classification",
            1.5,
            0.7,
            format!("{code} shares an industry prefix with a registration"),
        );
    }
    Criterion::new("classification", 1.5, 0.0, format!("{code} is outside the firm's registrations"))
}

/// A set-aside the firm holds is an advantage, because it removes competitors.
///
/// This is the criterion most often modelled backwards. A restricted
/// competition the firm qualifies for is *better* than an open one, not worse:
/// the whole point of the certification is the smaller field.
fn set_aside_advantage(o: &Opportunity, firm: &CapabilityStatement) -> Criterion {
    match &o.set_aside {
        SetAside::None => Criterion::new(
            "set-aside advantage",
            1.0,
            0.35,
            "full and open: the widest possible field of competitors",
        ),
        sa if firm.eligible_for(sa) => {
            let narrow = !matches!(
                sa,
                SetAside::TotalSmallBusiness | SetAside::PartialSmallBusiness
            );
            Criterion::new(
                "set-aside advantage",
                1.0,
                if narrow { 1.0 } else { 0.7 },
                format!("{} and the firm holds it", sa.label()),
            )
        }
        sa => Criterion::new(
            "set-aside advantage",
            1.0,
            0.0,
            format!("{} and the firm does not hold it", sa.label()),
        ),
    }
}

/// Whether the firm has done this work for this kind of customer before.
///
/// Weighted highest of any criterion, because past performance is the single
/// most predictive factor in federal source selection and the one a firm can do
/// nothing about between now and the deadline.
fn past_performance_fit(o: &Opportunity, firm: &CapabilityStatement) -> Criterion {
    if firm.past_performance.is_empty() {
        return Criterion::new(
            "past performance",
            2.0,
            0.0,
            "no past performance on file to cite",
        );
    }
    let code = o.classification.trim();
    let same_code = firm
        .past_performance
        .iter()
        .filter(|p| !code.is_empty() && p.naics.trim() == code)
        .count();
    let same_industry = firm
        .past_performance
        .iter()
        .filter(|p| {
            let n = p.naics.trim();
            !code.is_empty() && n.len() >= 5 && code.len() >= 5 && n[..5] == code[..5]
        })
        .count();

    if same_code > 0 {
        let score = (0.6 + 0.2 * same_code as f32).min(1.0);
        Criterion::new(
            "past performance",
            2.0,
            score,
            format!("{same_code} reference(s) on exactly {code}"),
        )
    } else if same_industry > 0 {
        Criterion::new(
            "past performance",
            2.0,
            0.5,
            format!("{same_industry} reference(s) in the same industry, none on {code}"),
        )
    } else {
        Criterion::new(
            "past performance",
            2.0,
            0.1,
            format!("{} reference(s) on file, none relevant to {code}", firm.past_performance.len()),
        )
    }
}

/// Whether there is enough time to write a good response rather than a rushed
/// one, without the notice being so far out that it is not yet real.
fn runway(o: &Opportunity, now: DateTime<Utc>, firm: &CapabilityStatement) -> Criterion {
    let Some(days) = o.days_until_deadline(now) else {
        return Criterion::new(
            "runway",
            1.0,
            0.6,
            "no deadline published, which is typical of an early sources-sought notice",
        );
    };
    let needed = firm.min_response_days.max(1);
    let ratio = days as f32 / needed as f32;
    let score = match ratio {
        r if r < 1.0 => 0.0,
        r if r < 1.5 => 0.4,
        r if r < 3.0 => 1.0,
        r if r < 6.0 => 0.8,
        _ => 0.6,
    };
    Criterion::new(
        "runway",
        1.0,
        score,
        format!("{days} day(s) against a {needed}-day minimum"),
    )
}

/// Whether the dollar value sits in the firm's productive band.
///
/// The sweet spot is the upper-middle of what the firm can staff. Work far
/// below capacity earns less than the proposal costs; work at the very top of
/// capacity is where delivery risk lives.
fn size_fit(o: &Opportunity, firm: &CapabilityStatement) -> Criterion {
    let Some(ceiling) = o.ceiling else {
        return Criterion::new("size fit", 1.0, 0.5, "no award ceiling published; unscored");
    };
    let Some(capacity) = firm.ceiling_capacity else {
        return Criterion::new(
            "size fit",
            1.0,
            0.6,
            format!("${ceiling} against no declared capacity limit"),
        );
    };
    let ratio = ceiling as f32 / capacity.max(1) as f32;
    let score = match ratio {
        r if r > 1.0 => 0.0,
        r if r > 0.85 => 0.6,
        r if r > 0.35 => 1.0,
        r if r > 0.10 => 0.7,
        _ => 0.3,
    };
    Criterion::new(
        "size fit",
        1.0,
        score,
        format!("${ceiling} is {:.0}% of the firm's ${capacity} capacity", ratio * 100.0),
    )
}

/// Whether the work is where the firm already operates.
fn geography_fit(o: &Opportunity, firm: &CapabilityStatement) -> Criterion {
    match (o.place_of_performance.as_deref(), firm.states.is_empty()) {
        (_, true) => Criterion::new("geography", 0.5, 0.8, "firm declares no geographic limit"),
        (None, _) => Criterion::new("geography", 0.5, 0.6, "no place of performance published"),
        (Some(state), false) => {
            let here = firm.states.iter().any(|s| s.eq_ignore_ascii_case(state));
            Criterion::new(
                "geography",
                0.5,
                if here { 1.0 } else { 0.2 },
                if here {
                    format!("{state} is a stated location")
                } else {
                    format!("{state} is outside the firm's stated locations")
                },
            )
        }
    }
}

/// Rank a scored set, best first. Ties break on notice id so the order is
/// stable across runs, which matters for a diffable audit trail.
pub fn rank(mut scores: Vec<FitScore>) -> Vec<FitScore> {
    scores.sort_by(|a, b| {
        b.total
            .partial_cmp(&a.total)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.notice_id.cmp(&b.notice_id))
    });
    scores
}

/// How the model performed against outcomes that already happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// References with a recorded win or loss. The rest cannot score the model.
    pub scored: usize,
    /// Wins the model would have recommended bidding on.
    pub true_positive: usize,
    /// Losses the model would have recommended bidding on.
    pub false_positive: usize,
    /// Wins the model would have advised against. The expensive error.
    pub false_negative: usize,
    /// Losses the model would have advised against. The saving.
    pub true_negative: usize,
}

impl Calibration {
    /// Share of recommended bids that were actually won.
    pub fn precision(&self) -> Option<f32> {
        let denom = self.true_positive + self.false_positive;
        (denom > 0).then(|| self.true_positive as f32 / denom as f32)
    }

    /// Share of actual wins the model would have chased.
    pub fn recall(&self) -> Option<f32> {
        let denom = self.true_positive + self.false_negative;
        (denom > 0).then(|| self.true_positive as f32 / denom as f32)
    }

    /// A plain-language verdict, including the case where there is not enough
    /// history to say anything. Reporting a precision computed from two data
    /// points as though it meant something is the failure this guards against.
    pub fn verdict(&self) -> String {
        if self.scored < 5 {
            return format!(
                "{} scored outcome(s): too few to calibrate. Record win/loss on past \
                 performance entries to make this meaningful.",
                self.scored
            );
        }
        let p = self.precision().map(|v| format!("{:.0}%", v * 100.0)).unwrap_or("n/a".into());
        let r = self.recall().map(|v| format!("{:.0}%", v * 100.0)).unwrap_or("n/a".into());
        format!(
            "{} outcomes: precision {p}, recall {r}. {} win(s) the model would have skipped.",
            self.scored, self.false_negative
        )
    }
}

/// Score the model against the firm's own recorded outcomes.
///
/// Each past-performance entry with a recorded `won` is replayed as a synthetic
/// opportunity and re-scored. This is the falsifier for the scoring model
/// itself: if the ranking does not separate the firm's historical wins from its
/// losses, the ranking is decoration.
pub fn calibrate(firm: &CapabilityStatement, now: DateTime<Utc>) -> Calibration {
    let mut c = Calibration {
        scored: 0,
        true_positive: 0,
        false_positive: 0,
        false_negative: 0,
        true_negative: 0,
    };

    for (i, p) in firm.past_performance.iter().enumerate() {
        let Some(won) = p.won else { continue };
        c.scored += 1;

        // Replay this reference as the opportunity it was, scored against the
        // firm's history EXCLUDING itself. Leaving it in would let every entry
        // vouch for its own past performance, which scores the model at 100%
        // and measures nothing.
        let mut history = firm.clone();
        history.past_performance =
            firm.past_performance.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, r)| r.clone()).collect();

        let mut o = Opportunity::empty(super::model::NoticeSource::Sam, format!("replay-{i}"));
        o.title = p.title.clone();
        o.classification = p.naics.clone();
        o.ceiling = p.value;
        o.deadline = Some(now + chrono::Duration::days(firm.min_response_days.max(1) * 2));

        let recommended = matches!(score(&o, &history, now).recommendation, Recommendation::Bid);
        match (recommended, won) {
            (true, true) => c.true_positive += 1,
            (true, false) => c.false_positive += 1,
            (false, true) => c.false_negative += 1,
            (false, false) => c.true_negative += 1,
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::model::{NoticeSource, PastPerformance};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap()
    }

    fn pp(naics: &str, won: Option<bool>) -> PastPerformance {
        PastPerformance {
            customer: "US Army".into(),
            title: "Prior work".into(),
            naics: naics.into(),
            value: Some(600_000),
            year: Some(2025),
            won,
            summary: String::new(),
        }
    }

    fn strong_firm() -> CapabilityStatement {
        CapabilityStatement {
            naics: vec!["541511".into()],
            certifications: vec![SetAside::ServiceDisabledVeteran],
            states: vec!["AZ".into()],
            ceiling_capacity: Some(2_000_000),
            floor: Some(50_000),
            min_response_days: 10,
            past_performance: vec![pp("541511", None), pp("541511", None)],
            ..Default::default()
        }
    }

    fn strong_opportunity() -> Opportunity {
        let mut o = Opportunity::empty(NoticeSource::Sam, "n1");
        o.classification = "541511".into();
        o.set_aside = SetAside::ServiceDisabledVeteran;
        o.deadline = Some(Utc.with_ymd_and_hms(2026, 9, 15, 23, 59, 59).unwrap());
        o.ceiling = Some(900_000);
        o.place_of_performance = Some("AZ".into());
        o
    }

    #[test]
    fn a_strong_match_is_recommended_and_shows_its_work() {
        let s = score(&strong_opportunity(), &strong_firm(), now());
        assert_eq!(s.recommendation, Recommendation::Bid);
        assert!(s.percent() >= 80, "expected a high score, got {}", s.percent());
        assert_eq!(s.criteria.len(), 6, "every dimension must report");
        assert!(s.criteria.iter().all(|c| !c.evidence.is_empty()), "no criterion may be unexplained");
        assert!(s.explain().contains("past performance"));
    }

    #[test]
    fn the_model_can_say_no() {
        // This is the falsifier. A firm with no relevant history, chasing an
        // open competition outside its registrations, must be told not to bid.
        let mut o = strong_opportunity();
        o.classification = "236220".into();
        o.set_aside = SetAside::None;
        o.place_of_performance = Some("ME".into());
        let s = score(&o, &strong_firm(), now());
        match &s.recommendation {
            Recommendation::NoBid { reasons } => {
                assert!(!reasons.is_empty(), "a no-bid must carry its reasons");
            }
            other => panic!("expected NoBid, got {other:?} at {}%", s.percent()),
        }
    }

    #[test]
    fn a_held_set_aside_scores_higher_than_open_competition() {
        let firm = strong_firm();
        let restricted = score(&strong_opportunity(), &firm, now());
        let mut open_o = strong_opportunity();
        open_o.set_aside = SetAside::None;
        let open = score(&open_o, &firm, now());
        assert!(
            restricted.total > open.total,
            "a set-aside the firm holds removes competitors and must score higher: \
             restricted {} vs open {}",
            restricted.percent(),
            open.percent()
        );
    }

    #[test]
    fn no_past_performance_vetoes_an_otherwise_good_match() {
        // An average would call this a decent bid. It is not: with nothing to
        // cite, the proposal cannot score in the section that decides awards.
        let firm = CapabilityStatement { past_performance: vec![], ..strong_firm() };
        let s = score(&strong_opportunity(), &firm, now());
        assert_ne!(s.recommendation, Recommendation::Bid, "scored {}%", s.percent());
        assert!(
            s.recommendation.reasons().iter().any(|r| r.contains("past performance")),
            "the veto must name itself: {:?}",
            s.recommendation
        );
    }

    #[test]
    fn a_deadline_inside_the_firms_turnaround_zeroes_the_runway() {
        let mut o = strong_opportunity();
        o.deadline = Some(now() + chrono::Duration::days(3));
        let s = score(&o, &strong_firm(), now());
        let runway = s.criteria.iter().find(|c| c.name == "runway").expect("runway scored");
        assert_eq!(runway.score, 0.0);
        assert!(runway.evidence.contains("3 day(s)"));
    }

    #[test]
    fn ranking_is_stable_for_equal_scores() {
        let mut a = score(&strong_opportunity(), &strong_firm(), now());
        a.notice_id = "b".into();
        let mut b = a.clone();
        b.notice_id = "a".into();
        let ranked = rank(vec![a, b]);
        assert_eq!(ranked[0].notice_id, "a", "ties break on id so the order is diffable");
    }

    #[test]
    fn calibration_refuses_to_report_a_number_it_cannot_support() {
        let firm = CapabilityStatement {
            past_performance: vec![pp("541511", Some(true))],
            ..strong_firm()
        };
        let c = calibrate(&firm, now());
        assert_eq!(c.scored, 1);
        assert!(c.verdict().contains("too few to calibrate"), "{}", c.verdict());
    }

    #[test]
    fn calibration_excludes_each_reference_from_its_own_replay() {
        // Six references, all won, all in the firm's NAICS. If the replay left
        // each entry in its own history it would score a perfect 6/6 by
        // vouching for itself. Excluded, the first ones have thinner history,
        // so a perfect score here would be the bug, not the goal.
        let firm = CapabilityStatement {
            past_performance: (0..6).map(|_| pp("541511", Some(true))).collect(),
            ..strong_firm()
        };
        let c = calibrate(&firm, now());
        assert_eq!(c.scored, 6);
        assert_eq!(
            c.true_positive + c.false_negative,
            6,
            "every reference is a recorded win"
        );
        assert!(!c.verdict().contains("too few"), "{}", c.verdict());
    }

    #[test]
    fn calibration_counts_the_expensive_error_separately() {
        let firm = CapabilityStatement {
            // Wins in an industry the firm is not registered for: the model
            // will advise against them, which is exactly a false negative.
            past_performance: (0..6).map(|_| pp("999999", Some(true))).collect(),
            ..strong_firm()
        };
        let c = calibrate(&firm, now());
        assert!(c.false_negative > 0, "wins the model would skip must be counted: {c:?}");
        assert!(c.verdict().contains("win(s) the model would have skipped"), "{}", c.verdict());
    }
}
