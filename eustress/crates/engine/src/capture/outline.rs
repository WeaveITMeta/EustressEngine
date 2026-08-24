//! The response outline, and the deterministic first draft built on it.
//!
//! A federal response has a fixed shape: FAR solicitations are structured by
//! Sections L and M, and a grant NOFO names its required narrative sections.
//! Neither shape needs inventing, so the outline is a template selected by
//! source and then populated from the compliance matrix.
//!
//! The draft this produces is skeletal on purpose. Every paragraph it emits is
//! either the firm's own capability text or a clearly marked gap, and every one
//! carries the clause citation it answers. A generation backend improves the
//! prose later; it is not needed for the document to be useful, and building
//! the structure first means a model outage degrades quality rather than
//! removing the feature.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::compliance::{ComplianceMatrix, Coverage};
use super::model::{NoticeSource, Opportunity};

/// Which response template applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateKind {
    /// FAR contract proposal, structured by Sections L and M.
    Far,
    /// Grant application, structured by the NOFO's narrative sections.
    Nofo,
}

impl TemplateKind {
    pub fn for_source(source: NoticeSource) -> Self {
        match source {
            NoticeSource::Sam => Self::Far,
            NoticeSource::Grants => Self::Nofo,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Far => "FAR",
            Self::Nofo => "NOFO",
        }
    }
}

/// One section of the response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub number: String,
    pub title: String,
    /// What an evaluator is looking for here, in one line.
    pub intent: String,
    /// Capability keys whose text seeds this section.
    pub capability_keys: Vec<String>,
    /// Clause citations assigned to this section.
    pub citations: Vec<String>,
    /// Seeded prose. Never invented: firm text, or a marked placeholder.
    pub draft: String,
    /// Requirements assigned here that nothing on file answers.
    pub gaps: Vec<String>,
}

impl Section {
    /// Whether this section can be written from what the firm has on file.
    pub fn is_writable(&self) -> bool {
        !self.capability_keys.is_empty() && self.gaps.is_empty()
    }
}

/// A complete response skeleton.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseOutline {
    pub notice_id: String,
    pub title: String,
    pub kind: TemplateKind,
    pub sections: Vec<Section>,
}

impl ResponseOutline {
    /// Sections still carrying an unanswered requirement.
    pub fn blocked(&self) -> Vec<&Section> {
        self.sections.iter().filter(|s| !s.gaps.is_empty()).collect()
    }

    /// Share of sections that can be written today.
    pub fn readiness_percent(&self) -> u8 {
        if self.sections.is_empty() {
            return 0;
        }
        let ready = self.sections.iter().filter(|s| s.is_writable()).count();
        ((ready as f32 / self.sections.len() as f32) * 100.0).round() as u8
    }

    pub fn headline(&self) -> String {
        format!(
            "{} template, {} section(s), {}% writable, {} blocked",
            self.kind.as_str(),
            self.sections.len(),
            self.readiness_percent(),
            self.blocked().len()
        )
    }

    /// The draft document. This is the file a user opens and edits.
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Response Draft: {}\n\n", self.title));
        s.push_str(&format!("Notice: `{}`  \n", self.notice_id));
        s.push_str(&format!("Template: {}  \n", self.kind.as_str()));
        s.push_str(&format!("Status: {}\n\n", self.headline()));
        s.push_str(
            "> Skeleton draft. Every paragraph below is either the firm's own capability text \
             or a marked gap. Nothing here was invented, and every claim carries the clause it \
             answers.\n\n",
        );

        for sec in &self.sections {
            s.push_str(&format!("## {} {}\n\n", sec.number, sec.title));
            s.push_str(&format!("*{}*\n\n", sec.intent));
            if !sec.citations.is_empty() {
                s.push_str(&format!("**Answers:** {}\n\n", sec.citations.join(", ")));
            }
            if sec.draft.is_empty() {
                s.push_str("_No capability text on file for this section. Write it._\n\n");
            } else {
                s.push_str(&sec.draft);
                s.push_str("\n\n");
            }
            if !sec.gaps.is_empty() {
                s.push_str("**Unanswered requirements in this section:**\n\n");
                for g in &sec.gaps {
                    s.push_str(&format!("- {g}\n"));
                }
                s.push('\n');
            }
        }
        s
    }
}

/// FAR proposal sections, in submission order.
fn far_template() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "1",
            "Executive Summary",
            "One page an evaluator can carry into a debrief: who the firm is and why this scope fits it",
        ),
        (
            "2",
            "Understanding of Requirements",
            "Proves the scope was read, in the solicitation's own vocabulary",
        ),
        (
            "3",
            "Technical Approach",
            "How each requirement is met, in the order Section L lists them",
        ),
        (
            "4",
            "Management Approach",
            "Who runs it, how risk is handled, and what the reporting rhythm is",
        ),
        ("5", "Staffing and Key Personnel", "Named people, cleared and available on day one"),
        ("6", "Past Performance", "Relevant, recent, and same-size contracts with references"),
        ("7", "Price", "Basis of estimate traceable to the technical approach"),
        (
            "8",
            "Assumptions and Exceptions",
            "Anything qualified, stated plainly rather than buried",
        ),
    ]
}

/// Grant narrative sections, in the order a NOFO normally requires them.
fn nofo_template() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("1", "Project Abstract", "The whole project in one page, readable by a non-specialist"),
        ("2", "Statement of Need", "The problem, evidenced with data about the service area"),
        ("3", "Project Description", "Goals, objectives, and the activities that reach them"),
        ("4", "Logic Model", "Inputs to activities to outputs to outcomes, made explicit"),
        ("5", "Evaluation Plan", "How success is measured, by whom, and against what baseline"),
        (
            "6",
            "Organizational Capacity",
            "Why this applicant can execute: staff, governance, prior awards",
        ),
        ("7", "Budget Narrative", "Every line tied to an activity, with the indirect rate stated"),
        ("8", "Sustainability", "What happens to the work after the period of performance"),
    ]
}

/// Build the outline and its seeded draft.
///
/// Requirements are assigned to sections by the compliance matrix's own
/// `response_section`, which came from the capability that matched. Anything the
/// matrix could not place lands in the technical or project-description section,
/// because an unplaced requirement must stay visible rather than vanish between
/// two data structures.
pub fn build(
    opportunity: &Opportunity,
    matrix: &ComplianceMatrix,
    capabilities: &BTreeMap<String, String>,
) -> ResponseOutline {
    let kind = TemplateKind::for_source(opportunity.source);
    let template = match kind {
        TemplateKind::Far => far_template(),
        TemplateKind::Nofo => nofo_template(),
    };
    // The catch-all for requirements the matrix could not place.
    let fallback_title = match kind {
        TemplateKind::Far => "Technical Approach",
        TemplateKind::Nofo => "Project Description",
    };

    let mut sections: Vec<Section> = template
        .into_iter()
        .map(|(number, title, intent)| Section {
            number: number.to_string(),
            title: title.to_string(),
            intent: intent.to_string(),
            capability_keys: Vec::new(),
            citations: Vec::new(),
            draft: String::new(),
            gaps: Vec::new(),
        })
        .collect();

    for row in &matrix.rows {
        let target = if row.response_section.is_empty() {
            fallback_title
        } else {
            &row.response_section
        };
        let idx = sections
            .iter()
            .position(|s| s.title.eq_ignore_ascii_case(target))
            .or_else(|| sections.iter().position(|s| s.title == fallback_title))
            .unwrap_or(0);

        let section = &mut sections[idx];
        let citation = row.requirement.citation();
        if !section.citations.contains(&citation) {
            section.citations.push(citation.clone());
        }
        match row.coverage {
            Coverage::Gap => section
                .gaps
                .push(format!("{citation}: {}", row.requirement.text)),
            _ => {
                // The note carries the capability key that matched.
                if let Some(key) = capability_key_from_note(&row.note) {
                    if capabilities.contains_key(&key) && !section.capability_keys.contains(&key) {
                        section.capability_keys.push(key);
                    }
                }
            }
        }
    }

    for section in &mut sections {
        section.draft = seed_draft(section, capabilities);
    }

    ResponseOutline {
        notice_id: matrix.notice_id.clone(),
        title: if opportunity.title.is_empty() {
            matrix.notice_id.clone()
        } else {
            opportunity.title.clone()
        },
        kind,
        sections,
    }
}

/// Compose a section's draft from the firm's own capability paragraphs.
///
/// Every sentence traceable to text the firm wrote, every claim followed by the
/// clauses it answers. Nothing is generated.
fn seed_draft(section: &Section, capabilities: &BTreeMap<String, String>) -> String {
    if section.capability_keys.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for key in &section.capability_keys {
        if let Some(text) = capabilities.get(key) {
            out.push_str(text.trim());
            out.push_str("\n\n");
        }
    }
    if !section.citations.is_empty() {
        out.push_str(&format!("*Addresses: {}*", section.citations.join(", ")));
    }
    out.trim_end().to_string()
}

/// Recover the capability key the matrix recorded in its note.
fn capability_key_from_note(note: &str) -> Option<String> {
    let start = note.find('\'')? + 1;
    let rest = &note[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::compliance;
    use crate::capture::requirements::extract;

    fn caps() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert(
            "past_performance".into(),
            "Three relevant contracts of similar scope and size, references available.".into(),
        );
        m.insert(
            "technical_approach".into(),
            "Tier 2 help desk support staffed during core hours with triage and escalation.".into(),
        );
        m
    }

    const SOW: &str = "\
C.1 The Contractor shall provide Tier 2 help desk support during core hours with triage.
C.2 The Contractor shall maintain an ISO 27001 certified information security program.
";

    fn opportunity(source: NoticeSource) -> Opportunity {
        let mut o = Opportunity::empty(source, "n1");
        o.title = "Network Modernization".into();
        o
    }

    fn outline_for(source: NoticeSource) -> ResponseOutline {
        let reqs = extract(SOW);
        let matrix = compliance::build("n1", &reqs, &caps());
        build(&opportunity(source), &matrix, &caps())
    }

    #[test]
    fn the_template_follows_the_source() {
        assert_eq!(outline_for(NoticeSource::Sam).kind, TemplateKind::Far);
        assert_eq!(outline_for(NoticeSource::Grants).kind, TemplateKind::Nofo);
        let nofo = outline_for(NoticeSource::Grants);
        assert!(
            nofo.sections.iter().any(|s| s.title == "Budget Narrative"),
            "a grant needs a budget narrative, which a FAR proposal does not"
        );
    }

    #[test]
    fn an_unanswered_requirement_blocks_its_section_rather_than_disappearing() {
        let o = outline_for(NoticeSource::Sam);
        let blocked = o.blocked();
        assert!(!blocked.is_empty(), "the ISO 27001 gap has to surface somewhere");
        let all_gaps: Vec<&String> = o.sections.iter().flat_map(|s| &s.gaps).collect();
        assert!(
            all_gaps.iter().any(|g| g.contains("ISO 27001")),
            "gap text must survive into the outline: {all_gaps:?}"
        );
    }

    #[test]
    fn a_covered_requirement_seeds_its_section_with_the_firms_own_words() {
        let o = outline_for(NoticeSource::Sam);
        let seeded: Vec<&Section> = o.sections.iter().filter(|s| !s.draft.is_empty()).collect();
        assert!(!seeded.is_empty(), "at least one section must be seeded");
        let joined: String = seeded.iter().map(|s| s.draft.as_str()).collect();
        assert!(joined.contains("Tier 2 help desk"), "{joined}");
        assert!(joined.contains("Addresses:"), "a seeded claim must cite what it answers");
    }

    #[test]
    fn the_draft_never_invents_prose_for_an_empty_section() {
        let empty: BTreeMap<String, String> = BTreeMap::new();
        let matrix = compliance::build("n1", &extract(SOW), &empty);
        let o = build(&opportunity(NoticeSource::Sam), &matrix, &empty);
        assert_eq!(o.readiness_percent(), 0);
        let md = o.to_markdown();
        assert!(md.contains("_No capability text on file for this section. Write it._"), "{md}");
        assert!(!md.contains("lorem"), "no filler prose may appear");
    }

    #[test]
    fn every_citation_reaches_the_rendered_document() {
        let o = outline_for(NoticeSource::Sam);
        let md = o.to_markdown();
        assert!(md.contains("C.1"), "{md}");
        assert!(md.contains("C.2"), "{md}");
    }

    #[test]
    fn readiness_counts_a_blocked_section_as_not_writable() {
        let o = outline_for(NoticeSource::Sam);
        assert!(o.readiness_percent() < 100, "a section with a gap is not writable");
        for s in o.blocked() {
            assert!(!s.is_writable(), "section {} claims writable while blocked", s.number);
        }
    }

    #[test]
    fn an_unplaced_requirement_lands_in_the_catch_all_not_nowhere() {
        // A matrix row whose capability match produced no section title.
        let empty: BTreeMap<String, String> = BTreeMap::new();
        let matrix = compliance::build("n1", &extract(SOW), &empty);
        let o = build(&opportunity(NoticeSource::Sam), &matrix, &empty);
        let placed: usize = o.sections.iter().map(|s| s.citations.len()).sum();
        assert_eq!(placed, matrix.rows.len(), "every row must be placed somewhere");
        let tech = o.sections.iter().find(|s| s.title == "Technical Approach").expect("exists");
        assert!(!tech.citations.is_empty(), "the catch-all must actually catch");
    }

    #[test]
    fn the_headline_reports_the_template_and_the_blockers() {
        let h = outline_for(NoticeSource::Sam).headline();
        assert!(h.starts_with("FAR template"), "{h}");
        assert!(h.contains("blocked"), "{h}");
    }
}
