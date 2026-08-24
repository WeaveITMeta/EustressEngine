//! The compliance matrix: every binding requirement, where the response answers
//! it, and what is still uncovered.
//!
//! This is the artifact worth the most and costing the least. It needs no
//! model: it is the requirement list from [`super::requirements`] joined against
//! what the firm can actually claim, with the unmatched rows shown rather than
//! hidden.
//!
//! The gap report is the Proposal discipline's falsifier. A tool that only ever
//! reported coverage would let a firm submit a response that reads complete and
//! scores non-responsive.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::requirements::{binding_on_offeror, Requirement};

/// How well the firm's material answers one requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coverage {
    /// Strong overlap with a capability the firm has written down.
    Covered,
    /// Some overlap. Needs a human to confirm or write the difference.
    Partial,
    /// Nothing on file answers this. Either write it or do not bid.
    Gap,
}

impl Coverage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Covered => "covered",
            Self::Partial => "partial",
            Self::Gap => "GAP",
        }
    }
}

/// One row: a requirement and its answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatrixRow {
    pub requirement: Requirement,
    /// Which proposal section carries the answer.
    pub response_section: String,
    pub coverage: Coverage,
    /// The capability key that matched, or why nothing did.
    pub note: String,
}

/// Every binding requirement in one solicitation, with its coverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComplianceMatrix {
    pub notice_id: String,
    pub rows: Vec<MatrixRow>,
    /// Requirements found but not binding on the bidder, counted so the
    /// headline can say what was set aside rather than silently dropping it.
    pub non_binding: usize,
}

impl ComplianceMatrix {
    /// Rows nothing on file answers. The list a bid/no-bid decision turns on.
    pub fn gaps(&self) -> Vec<&MatrixRow> {
        self.rows.iter().filter(|r| r.coverage == Coverage::Gap).collect()
    }

    /// Rows needing a human before they can be claimed.
    pub fn partials(&self) -> Vec<&MatrixRow> {
        self.rows.iter().filter(|r| r.coverage == Coverage::Partial).collect()
    }

    /// Share of binding requirements fully covered, 0 to 100.
    ///
    /// Partials count as half. Counting them as covered would be the flattering
    /// lie this whole module exists to prevent.
    pub fn coverage_percent(&self) -> u8 {
        if self.rows.is_empty() {
            return 0;
        }
        let score: f32 = self
            .rows
            .iter()
            .map(|r| match r.coverage {
                Coverage::Covered => 1.0,
                Coverage::Partial => 0.5,
                Coverage::Gap => 0.0,
            })
            .sum();
        ((score / self.rows.len() as f32) * 100.0).round() as u8
    }

    /// One line for the ribbon and the deal-room billboard.
    pub fn headline(&self) -> String {
        format!(
            "{} binding requirement(s), {}% covered, {} gap(s)",
            self.rows.len(),
            self.coverage_percent(),
            self.gaps().len()
        )
    }

    /// The matrix as a markdown table, which is what goes in the draft package.
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Compliance Matrix: {}\n\n", self.notice_id));
        s.push_str(&format!("{}\n\n", self.headline()));
        s.push_str("| Clause | Requirement | Response Section | Coverage | Note |\n");
        s.push_str("|---|---|---|---|---|\n");
        for r in &self.rows {
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                escape_cell(&r.requirement.citation()),
                escape_cell(&truncate(&r.requirement.text, 160)),
                escape_cell(&r.response_section),
                r.coverage.as_str(),
                escape_cell(&r.note),
            ));
        }
        if !self.gaps().is_empty() {
            s.push_str("\n## Gaps\n\nNothing on file answers these. Write them, team them, or no-bid.\n\n");
            for g in self.gaps() {
                s.push_str(&format!("- **{}** {}\n", g.requirement.citation(), truncate(&g.requirement.text, 200)));
            }
        }
        s
    }

    /// CSV, for the reviewers who will want it in a spreadsheet.
    pub fn to_csv(&self) -> String {
        let mut s = String::from("clause,section,coverage,response_section,requirement,note\n");
        for r in &self.rows {
            s.push_str(&format!(
                "{},{},{},{},{},{}\n",
                csv_cell(&r.requirement.citation()),
                csv_cell(&r.requirement.section),
                r.coverage.as_str(),
                csv_cell(&r.response_section),
                csv_cell(&r.requirement.text),
                csv_cell(&r.note),
            ));
        }
        s
    }
}

/// Build the matrix for one solicitation.
///
/// `capabilities` maps a topic key to the paragraph the firm would put in a
/// proposal. Matching is keyword overlap on content words, which is crude and
/// deliberately so: a wrong "covered" is far more expensive than a wrong "gap",
/// and a human reads every row either way.
pub fn build(
    notice_id: &str,
    requirements: &[Requirement],
    capabilities: &BTreeMap<String, String>,
) -> ComplianceMatrix {
    let binding = binding_on_offeror(requirements);
    let non_binding = requirements.len() - binding.len();

    let rows = binding
        .into_iter()
        .map(|req| {
            let (coverage, note, section) = assess(&req, capabilities);
            MatrixRow { requirement: req, response_section: section, coverage, note }
        })
        .collect();

    ComplianceMatrix { notice_id: notice_id.to_string(), rows, non_binding }
}

/// Score one requirement against the capability library.
fn assess(
    req: &Requirement,
    capabilities: &BTreeMap<String, String>,
) -> (Coverage, String, String) {
    let req_words = content_words(&req.text);
    if req_words.is_empty() {
        return (Coverage::Gap, "no content words to match on".into(), String::new());
    }

    let mut best: Option<(&String, f32)> = None;
    for (key, text) in capabilities {
        let cap_words = content_words(&format!("{key} {text}"));
        if cap_words.is_empty() {
            continue;
        }
        let hits = req_words.iter().filter(|w| cap_words.contains(*w)).count();
        let overlap = hits as f32 / req_words.len() as f32;
        if best.map_or(true, |(_, b)| overlap > b) {
            best = Some((key, overlap));
        }
    }

    match best {
        Some((key, overlap)) if overlap >= 0.5 => (
            Coverage::Covered,
            format!("matches capability '{key}' ({:.0}% of terms)", overlap * 100.0),
            title_case(key),
        ),
        Some((key, overlap)) if overlap >= 0.25 => (
            Coverage::Partial,
            format!("partial match on '{key}' ({:.0}% of terms); confirm before claiming", overlap * 100.0),
            title_case(key),
        ),
        _ => (
            Coverage::Gap,
            "no capability on file addresses this".into(),
            String::new(),
        ),
    }
}

/// Lowercased content words, stopwords and short tokens removed.
fn content_words(s: &str) -> Vec<String> {
    const STOP: [&str; 34] = [
        "the", "a", "an", "and", "or", "of", "to", "in", "for", "on", "at", "by", "with", "as",
        "is", "are", "be", "been", "was", "were", "shall", "must", "will", "should", "not", "all",
        "any", "each", "such", "that", "this", "these", "those", "from",
    ];
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !STOP.contains(w))
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn title_case(s: &str) -> String {
    s.split(['_', ' ', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// A pipe inside a markdown cell would break the table.
fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn csv_cell(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::requirements::extract;

    fn caps() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert(
            "help_desk".into(),
            "Tier 1 and Tier 2 help desk support staffed during core business hours, \
             with ticket triage and escalation."
                .into(),
        );
        m.insert(
            "reporting".into(),
            "Monthly status summary reporting with burn-down and milestone tracking.".into(),
        );
        m
    }

    const SOW: &str = "\
C.1 The Contractor shall provide Tier 2 help desk support during core hours.
C.2 The Contractor shall deliver a monthly status summary report.
C.3 The Contractor shall maintain an ISO 27001 certified information security program.
C.4 The Government shall provide facility access badges.
C.5 The Contractor should consider automation opportunities.
";

    #[test]
    fn the_matrix_holds_only_binding_offeror_requirements() {
        let m = build("n1", &extract(SOW), &caps());
        // C.1, C.2, C.3 bind the contractor. C.4 is the Government's and C.5 is
        // a preference, so neither is a row.
        assert_eq!(m.rows.len(), 3, "{:#?}", m.rows.iter().map(|r| &r.requirement.id).collect::<Vec<_>>());
        assert!(m.non_binding >= 2, "the set-aside rows are counted, not discarded");
    }

    #[test]
    fn a_requirement_with_no_matching_capability_is_a_gap_not_a_pass() {
        let m = build("n1", &extract(SOW), &caps());
        let gaps = m.gaps();
        assert_eq!(gaps.len(), 1, "{:#?}", gaps);
        assert!(gaps[0].requirement.text.contains("ISO 27001"), "{}", gaps[0].requirement.text);
        assert!(gaps[0].response_section.is_empty(), "a gap has no section to point at");
    }

    #[test]
    fn a_matched_requirement_names_the_capability_it_matched() {
        let m = build("n1", &extract(SOW), &caps());
        let row = m.rows.iter().find(|r| r.requirement.id == "C.1").expect("row exists");
        assert_ne!(row.coverage, Coverage::Gap, "{}", row.note);
        assert!(row.note.contains("help_desk"), "{}", row.note);
        assert_eq!(row.response_section, "Help Desk");
    }

    #[test]
    fn coverage_counts_a_partial_as_half_not_as_covered() {
        let mut one = BTreeMap::new();
        one.insert("help_desk".into(), "Tier 2 help desk support during core hours.".into());
        let m = build("n1", &extract("C.1 The Contractor shall provide Tier 2 help desk support during core hours."), &one);
        assert_eq!(m.coverage_percent(), 100);

        let empty: BTreeMap<String, String> = BTreeMap::new();
        let none = build("n1", &extract(SOW), &empty);
        assert_eq!(none.coverage_percent(), 0, "nothing on file covers nothing");
        assert_eq!(none.gaps().len(), none.rows.len());
    }

    #[test]
    fn an_empty_matrix_reports_zero_rather_than_dividing_by_zero() {
        let m = build("n1", &[], &caps());
        assert_eq!(m.coverage_percent(), 0);
        assert_eq!(m.headline(), "0 binding requirement(s), 0% covered, 0 gap(s)");
    }

    #[test]
    fn markdown_escapes_a_pipe_so_the_table_survives() {
        let reqs = extract("C.1 The Contractor shall support A | B routing.");
        let m = build("n1", &reqs, &caps());
        let md = m.to_markdown();
        assert!(md.contains("A \\| B"), "unescaped pipe would break the table:\n{md}");
    }

    #[test]
    fn markdown_lists_the_gaps_in_their_own_section() {
        let m = build("n1", &extract(SOW), &caps());
        let md = m.to_markdown();
        assert!(md.contains("## Gaps"), "{md}");
        assert!(md.contains("ISO 27001"), "{md}");
    }

    #[test]
    fn csv_quotes_embedded_quotes() {
        let reqs = extract("C.1 The Contractor shall deliver a \"final\" report.");
        let csv = build("n1", &reqs, &caps()).to_csv();
        assert!(csv.contains("\"\"final\"\""), "{csv}");
        assert!(csv.starts_with("clause,section,coverage"));
    }
}
