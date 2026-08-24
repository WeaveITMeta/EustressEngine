//! Pull numbered requirements out of a solicitation's own text.
//!
//! Deterministic text processing, no model. A statement of work says "shall"
//! several hundred times, and every one of those is a row a proposal has to
//! answer. Missing one is how a compliant-looking response gets scored
//! non-responsive.
//!
//! Two distinctions do most of the work here:
//!
//! - **Who the obligation binds.** "The Contractor shall provide..." is a row
//!   in the matrix. "The Government shall not be liable..." is not. A matrix
//!   padded with the buyer's own obligations buries the ones that matter.
//! - **A prohibition is still a requirement.** "shall not subcontract" has to
//!   appear, because violating it is as disqualifying as omitting a service.

use serde::{Deserialize, Serialize};

/// Which modal verb created the obligation. Kept because the strength differs:
/// `shall` and `must` are binding, `should` is a preference a proposal can
/// trade against, and conflating them costs either compliance or price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modal {
    Shall,
    Must,
    Will,
    Should,
    /// "is required to", "is responsible for".
    Required,
}

impl Modal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shall => "shall",
            Self::Must => "must",
            Self::Will => "will",
            Self::Should => "should",
            Self::Required => "required",
        }
    }

    /// Whether failing this makes a response non-responsive.
    pub fn is_binding(self) -> bool {
        matches!(self, Self::Shall | Self::Must | Self::Required)
    }
}

/// Who the obligation binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Actor {
    /// The bidder. These are the matrix rows.
    Offeror,
    /// The buying agency. Context, not a deliverable.
    Government,
    /// No subject identified, so it stays in the matrix rather than being
    /// dropped on a guess.
    Unclear,
}

/// One extracted obligation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    /// Clause id as published (`C.3.1`, `L.2`), or a synthesized `R-###` when
    /// the document numbers nothing. A synthesized id is marked by
    /// `derived_id` so a citation never implies the document said it.
    pub id: String,
    pub derived_id: bool,
    pub text: String,
    pub modal: Modal,
    pub actor: Actor,
    /// Nearest preceding section heading, for context in the matrix.
    pub section: String,
    /// 1-based line in the source text, so a citation can be checked.
    pub line: usize,
}

impl Requirement {
    /// The citation a draft paragraph carries back to the source.
    pub fn citation(&self) -> String {
        if self.derived_id {
            format!("{} (line {})", self.id, self.line)
        } else {
            self.id.clone()
        }
    }
}

/// Extract every obligation from a solicitation document.
///
/// Accepts plain text: an attachment converted from PDF, a pasted SOW, or a
/// Grants.gov synopsis. Line numbers are preserved so a reviewer can find any
/// row in the original.
pub fn extract(document: &str) -> Vec<Requirement> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut derived = 0usize;

    for (idx, raw_line) in document.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(h) = heading(line) {
            section = h;
            // A heading can still carry an obligation on the same line, so
            // fall through rather than skipping to the next line.
        }

        let clause = clause_id(line);
        for sentence in sentences(line) {
            let Some(modal) = modal_of(&sentence) else { continue };
            let id = match &clause {
                Some(c) => c.clone(),
                None => {
                    derived += 1;
                    format!("R-{derived:03}")
                }
            };
            out.push(Requirement {
                derived_id: clause.is_none(),
                id,
                text: sentence.trim().to_string(),
                modal,
                actor: actor_of(&sentence),
                section: section.clone(),
                line: line_no,
            });
        }
    }
    out
}

/// Only the obligations that bind the bidder and carry real weight.
///
/// This is what the compliance matrix is built from. `Unclear` is kept: an
/// unattributed "shall" in a statement of work is almost always the
/// contractor's, and dropping it would be the expensive guess.
pub fn binding_on_offeror(reqs: &[Requirement]) -> Vec<Requirement> {
    reqs.iter()
        .filter(|r| r.modal.is_binding() && r.actor != Actor::Government)
        .cloned()
        .collect()
}

/// Split a line into sentences on terminal punctuation.
///
/// Abbreviations common in solicitations (`U.S.`, `No.`, section numbers like
/// `C.3.1`) would otherwise split mid-sentence and truncate a requirement.
fn sentences(line: &str) -> Vec<String> {
    let bytes: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 0..bytes.len() {
        let c = bytes[i];
        if c != '.' && c != ';' && c != '!' && c != '?' {
            continue;
        }
        // A period between digits or single letters is a numbering separator
        // or an abbreviation, not a sentence end.
        if c == '.' {
            let prev = if i > 0 { bytes[i - 1] } else { ' ' };
            let next = bytes.get(i + 1).copied().unwrap_or(' ');
            if !next.is_whitespace() {
                continue;
            }
            if prev.is_ascii_uppercase() && (i < 2 || !bytes[i - 2].is_alphanumeric()) {
                continue;
            }
        }
        let piece: String = bytes[start..=i].iter().collect();
        if !piece.trim().is_empty() {
            out.push(piece);
        }
        start = i + 1;
    }
    let tail: String = bytes[start.min(bytes.len())..].iter().collect();
    if !tail.trim().is_empty() {
        out.push(tail);
    }
    out
}

/// The strongest modal present in a sentence.
///
/// Strongest wins, because a sentence carrying both "shall" and "should" is
/// binding on the "shall" and treating it as a preference loses compliance.
fn modal_of(sentence: &str) -> Option<Modal> {
    let s = format!(" {} ", sentence.to_ascii_lowercase());
    if s.contains(" shall ") || s.contains(" shall,") {
        return Some(Modal::Shall);
    }
    if s.contains(" must ") {
        return Some(Modal::Must);
    }
    if s.contains(" is required to ")
        || s.contains(" are required to ")
        || s.contains(" is responsible for ")
        || s.contains(" are responsible for ")
    {
        return Some(Modal::Required);
    }
    if s.contains(" will ") {
        return Some(Modal::Will);
    }
    if s.contains(" should ") {
        return Some(Modal::Should);
    }
    None
}

/// Who the sentence binds, from the subject preceding the modal.
fn actor_of(sentence: &str) -> Actor {
    let lower = sentence.to_ascii_lowercase();
    let modal_pos = [" shall", " must", " will", " should", " is required", " are required"]
        .iter()
        .filter_map(|m| lower.find(m))
        .min();
    let subject = match modal_pos {
        Some(p) => &lower[..p],
        None => &lower[..],
    };

    const OFFEROR: [&str; 7] = [
        "contractor",
        "offeror",
        "bidder",
        "vendor",
        "applicant",
        "recipient",
        "proposal",
    ];
    const GOVERNMENT: [&str; 6] = [
        "government",
        "agency",
        "contracting officer",
        "the co ",
        "awarding official",
        "program officer",
    ];

    // Nearest subject to the modal wins: "The Government will notify the
    // Contractor" binds the Government, and scanning left-to-right without
    // this would call it an offeror obligation.
    let last_offeror = OFFEROR.iter().filter_map(|w| subject.rfind(w)).max();
    let last_gov = GOVERNMENT.iter().filter_map(|w| subject.rfind(w)).max();
    match (last_offeror, last_gov) {
        (Some(o), Some(g)) => {
            if o > g {
                Actor::Offeror
            } else {
                Actor::Government
            }
        }
        (Some(_), None) => Actor::Offeror,
        (None, Some(_)) => Actor::Government,
        (None, None) => Actor::Unclear,
    }
}

/// A published clause id at the start of a line, e.g. `C.3.1`, `3.2.1`, `L.2`.
fn clause_id(line: &str) -> Option<String> {
    let token = line.split_whitespace().next()?;
    let trimmed = token.trim_end_matches(['.', ')', ':']);
    if trimmed.is_empty() || trimmed.len() > 12 {
        return None;
    }
    let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
    let shaped = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '(' || c == ')');
    // Require a separator so an ordinary capitalized first word is not read as
    // a clause id.
    let separated = trimmed.contains('.') || trimmed.contains('-');
    (has_digit && shaped && separated).then(|| trimmed.to_string())
}

/// A section heading: short, no terminal period, and either all-caps or
/// clause-numbered.
fn heading(line: &str) -> Option<String> {
    if line.len() > 90 || line.ends_with('.') {
        return None;
    }
    let words = line.split_whitespace().count();
    if words == 0 || words > 12 {
        return None;
    }
    let letters: String = line.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return None;
    }
    let all_caps = letters.chars().all(|c| c.is_uppercase());
    let numbered = clause_id(line).is_some();
    // A numbered line that also carries a modal is a requirement, not a
    // heading, so only a numbered line with no obligation counts.
    if numbered && modal_of(line).is_some() {
        return None;
    }
    (all_caps || numbered).then(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOW: &str = "\
SECTION C - STATEMENT OF WORK

C.3.1 The Contractor shall provide Tier 2 help desk support during core hours.
C.3.2 The Contractor shall not subcontract more than 50% of the work.
C.3.3 The Government will furnish workspace and network access.
C.3.4 The Contractor is responsible for all travel costs.
C.4 Reporting
The Contractor should provide a monthly status summary in U.S. Government format.
This paragraph has no obligation in it at all.
";

    #[test]
    fn every_obligation_is_extracted_with_its_published_clause_id() {
        let reqs = extract(SOW);
        let ids: Vec<&str> = reqs.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"C.3.1"), "{ids:?}");
        assert!(ids.contains(&"C.3.2"), "{ids:?}");
        assert!(reqs.iter().all(|r| !r.text.is_empty()));
    }

    #[test]
    fn a_prohibition_is_still_a_requirement() {
        let reqs = extract(SOW);
        let r = reqs.iter().find(|r| r.id == "C.3.2").expect("the shall-not clause is extracted");
        assert_eq!(r.modal, Modal::Shall);
        assert!(r.text.contains("shall not subcontract"));
        assert!(binding_on_offeror(&reqs).iter().any(|x| x.id == "C.3.2"));
    }

    #[test]
    fn a_government_obligation_is_not_a_matrix_row() {
        let reqs = extract(SOW);
        let gov = reqs.iter().find(|r| r.id == "C.3.3").expect("extracted");
        assert_eq!(gov.actor, Actor::Government, "{}", gov.text);
        assert!(
            !binding_on_offeror(&reqs).iter().any(|r| r.id == "C.3.3"),
            "the buyer's own obligations must stay out of the compliance matrix"
        );
    }

    #[test]
    fn nearest_subject_to_the_modal_decides_the_actor() {
        // "The Government will notify the Contractor" mentions the contractor
        // AFTER the modal, so the obligation is the Government's.
        let reqs = extract("1.1 The Government will notify the Contractor of any change.");
        assert_eq!(reqs[0].actor, Actor::Government, "{}", reqs[0].text);

        // Reversed, the contractor is the subject.
        let reqs = extract("1.2 The Contractor will notify the Government of any change.");
        assert_eq!(reqs[0].actor, Actor::Offeror, "{}", reqs[0].text);
    }

    #[test]
    fn is_responsible_for_counts_as_binding() {
        let reqs = extract(SOW);
        let r = reqs.iter().find(|r| r.id == "C.3.4").expect("extracted");
        assert_eq!(r.modal, Modal::Required);
        assert!(r.modal.is_binding());
    }

    #[test]
    fn should_is_extracted_but_is_not_binding() {
        let reqs = extract(SOW);
        let r = reqs.iter().find(|r| r.modal == Modal::Should).expect("the should clause");
        assert!(!r.modal.is_binding());
        assert!(
            !binding_on_offeror(&reqs).iter().any(|x| x.line == r.line),
            "a preference is not a compliance row"
        );
    }

    #[test]
    fn an_abbreviation_does_not_truncate_a_requirement() {
        // "U.S. Government format" must survive intact.
        let reqs = extract("C.9 The Contractor shall deliver files in U.S. Government format.");
        assert_eq!(reqs.len(), 1, "{reqs:#?}");
        assert!(reqs[0].text.contains("U.S. Government format"), "{}", reqs[0].text);
    }

    #[test]
    fn section_context_travels_with_each_requirement() {
        let reqs = extract(SOW);
        let r = reqs.iter().find(|r| r.id == "C.3.1").expect("extracted");
        assert_eq!(r.section, "SECTION C - STATEMENT OF WORK");
    }

    #[test]
    fn an_unnumbered_document_gets_derived_ids_that_admit_it() {
        let reqs = extract("The Contractor shall deliver monthly.\nThe vendor must be insured.");
        assert_eq!(reqs.len(), 2);
        assert!(reqs.iter().all(|r| r.derived_id));
        assert_eq!(reqs[0].id, "R-001");
        assert!(reqs[0].citation().contains("line 1"), "{}", reqs[0].citation());
    }

    #[test]
    fn a_line_with_no_obligation_produces_no_row() {
        let reqs = extract("This paragraph has no obligation in it at all.");
        assert!(reqs.is_empty(), "{reqs:#?}");
    }

    #[test]
    fn a_published_clause_id_cites_itself_without_a_line_number() {
        let reqs = extract("L.2.1 The Offeror shall submit a technical volume.");
        assert_eq!(reqs[0].citation(), "L.2.1");
        assert!(!reqs[0].derived_id);
    }
}
