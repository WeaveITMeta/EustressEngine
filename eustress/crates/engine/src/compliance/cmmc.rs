//! # CMMC control register — Levels 1 and 2
//!
//! A machine-readable register of the CMMC practices Eustress must meet, with
//! each control's status backed by **evidence that CI can verify**. The point
//! is that compliance drift becomes a build failure rather than a document
//! that quietly rots: if a control cites `auth.rs:120` and that file is
//! deleted, [`tests::every_file_evidence_path_exists`] fails.
//!
//! ## What this module is not
//!
//! It is not a compliance claim. CMMC is an **organizational** certification
//! assessed against people, process, and technology — a repository cannot be
//! "CMMC compliant" any more than a hammer can be OSHA compliant. What this
//! register does is state, per control, what the software contributes and
//! what it structurally cannot, so nobody mistakes a tidy table for an
//! attestation. Submitting a score to SPRS and signing the senior-official
//! affirmation are human acts with False Claims Act consequences; no agent
//! performs them (`docs/PROMPTS/00_MASTER_PROTOCOL.md` §6).
//!
//! ## The counting, reconciled
//!
//! Level 1 is commonly cited as **15 practices**, and equally commonly as
//! **17**. Both are right. FAR 52.204-21(b)(1) has fifteen paragraphs,
//! (i)–(xv); paragraph **(ix)** bundles three distinct physical-protection
//! obligations — escort visitors, maintain physical access logs, manage
//! access devices — which map to three separate NIST SP 800-171 identifiers.
//! So: 15 FAR paragraphs, 17 NIST-mapped practice IDs, 6 domains. Level 1 is
//! also **pass/fail with no POA&M** — every applicable control must be met,
//! which is why partial status is tracked so precisely here.
//!
//! Level 2 mirrors the 110 NIST SP 800-171 controls across 14 families and
//! *does* permit a POA&M, scored via the DoD Assessment Methodology
//! ([`sprs_score`]).

use serde::{Deserialize, Serialize};

// ============================================================================
// 1. Taxonomy
// ============================================================================

/// CMMC maturity level a control belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmmcLevel {
    /// Level 1 — Federal Contract Information (FCI), annual self-assessment.
    L1,
    /// Level 2 — Controlled Unclassified Information (CUI), 110 NIST controls.
    L2,
}

/// NIST SP 800-171 control family / CMMC domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Domain {
    /// Access Control
    AC,
    /// Awareness & Training
    AT,
    /// Audit & Accountability
    AU,
    /// Configuration Management
    CM,
    /// Identification & Authentication
    IA,
    /// Incident Response
    IR,
    /// Maintenance
    MA,
    /// Media Protection
    MP,
    /// Personnel Security
    PS,
    /// Physical Protection
    PE,
    /// Risk Assessment
    RA,
    /// Security Assessment
    CA,
    /// System & Communications Protection
    SC,
    /// System & Information Integrity
    SI,
}

impl Domain {
    pub fn name(&self) -> &'static str {
        match self {
            Self::AC => "Access Control",
            Self::AT => "Awareness & Training",
            Self::AU => "Audit & Accountability",
            Self::CM => "Configuration Management",
            Self::IA => "Identification & Authentication",
            Self::IR => "Incident Response",
            Self::MA => "Maintenance",
            Self::MP => "Media Protection",
            Self::PS => "Personnel Security",
            Self::PE => "Physical Protection",
            Self::RA => "Risk Assessment",
            Self::CA => "Security Assessment",
            Self::SC => "System & Communications Protection",
            Self::SI => "System & Information Integrity",
        }
    }
}

/// Who can actually satisfy a control. This is the field that keeps the
/// register honest: no amount of Rust satisfies "escort visitors and monitor
/// visitor activity", and a register that doesn't say so invites a green
/// dashboard to be read as an attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Responsibility {
    /// Software can satisfy this control outright.
    Technical,
    /// Software contributes, but policy or process must complete it.
    Hybrid,
    /// Policy, training, or contractual process only — code cannot help.
    Organizational,
    /// Facility and physical-security measures. Out of scope for any codebase.
    Physical,
}

/// Assessment status. Defaults to [`Self::NotAssessed`] — an unexamined
/// control must never read as a passing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Status {
    /// Verified met, with evidence.
    Implemented,
    /// Some of the requirement is met; the gap is named in `notes`.
    Partial,
    /// Confirmed not met.
    NotImplemented,
    /// Out of scope for this system, with justification in `notes`.
    NotApplicable,
    /// Not yet examined. The honest default.
    #[default]
    NotAssessed,
}

impl Status {
    /// Does this count as satisfied for Level 1's all-or-nothing gate?
    pub fn is_met(&self) -> bool {
        matches!(self, Self::Implemented | Self::NotApplicable)
    }
}

/// Evidence backing a status. `FileLine` is the valuable variant — it is the
/// one CI can check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Evidence {
    /// A repo-relative path, optionally with a line number.
    FileLine { path: String, line: Option<u32> },
    /// A document (policy, SSP section, procedure).
    Document { path: String },
    /// A human process with no artifact in this repo.
    Process { description: String },
}

/// One CMMC practice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Control {
    /// CMMC practice id, e.g. `"AC.L1-3.1.1"`.
    pub id: &'static str,
    /// NIST SP 800-171 id, e.g. `"3.1.1"`.
    pub nist_id: &'static str,
    /// FAR 52.204-21(b)(1) paragraph for L1 controls, e.g. `"(ix)"`.
    pub far_paragraph: Option<&'static str>,
    pub level: CmmcLevel,
    pub domain: Domain,
    pub title: &'static str,
    /// The requirement, as written in the source standard.
    pub requirement: &'static str,
    pub responsibility: Responsibility,
    pub status: Status,
    pub evidence: Vec<Evidence>,
    /// Why the status is what it is, and what would move it.
    pub notes: &'static str,
}

// ============================================================================
// 2. The Level 1 register
// ============================================================================

fn file(path: &str) -> Evidence {
    Evidence::FileLine { path: path.to_string(), line: None }
}

/// The 17 NIST-mapped Level 1 practices (15 FAR paragraphs; (ix) expands to
/// three PE controls).
///
/// Statuses are assessed from a survey of the repository. Anything not
/// positively evidenced is [`Status::NotAssessed`] or
/// [`Status::NotImplemented`] — this register claims nothing it has not seen.
pub fn level_1_controls() -> Vec<Control> {
    vec![
        Control {
            id: "AC.L1-3.1.1", nist_id: "3.1.1", far_paragraph: Some("(i)"),
            level: CmmcLevel::L1, domain: Domain::AC,
            title: "Authorized access control",
            requirement: "Limit information system access to authorized users, processes acting on behalf of authorized users, and devices (including other information systems).",
            responsibility: Responsibility::Technical,
            status: Status::Partial,
            evidence: vec![file("eustress/crates/engine/src/auth.rs"), file("eustress/crates/identity/src/verifier.rs")],
            notes: "An auth surface and an Ed25519 identity crate (issuer/verifier/revocation) exist. Gap: no enforced authorization boundary on the engine bridge or MCP surface — those accept local connections without an identity check.",
        },
        Control {
            id: "AC.L1-3.1.2", nist_id: "3.1.2", far_paragraph: Some("(ii)"),
            level: CmmcLevel::L1, domain: Domain::AC,
            title: "Transaction & function control",
            requirement: "Limit information system access to the types of transactions and functions that authorized users are permitted to execute.",
            responsibility: Responsibility::Technical,
            status: Status::NotImplemented,
            evidence: vec![],
            notes: "No role- or permission-based gating found. Every MCP/bridge tool is callable by any connected client, including destructive ones (delete_entity, run_bash, write_file).",
        },
        Control {
            id: "AC.L1-3.1.20", nist_id: "3.1.20", far_paragraph: Some("(iii)"),
            level: CmmcLevel::L1, domain: Domain::AC,
            title: "External connections",
            requirement: "Verify and control/limit connections to and use of external information systems.",
            responsibility: Responsibility::Hybrid,
            status: Status::NotAssessed,
            evidence: vec![file("eustress/crates/engine/src/soul/xai_client.rs"), file("eustress/crates/engine/src/soul/claude_client.rs")],
            notes: "Outbound calls exist to api.anthropic.com and api.x.ai, plus http_request/run_bash tools with unrestricted egress. No allowlist. Needs a documented boundary before assessment.",
        },
        Control {
            id: "AC.L1-3.1.22", nist_id: "3.1.22", far_paragraph: Some("(iv)"),
            level: CmmcLevel::L1, domain: Domain::AC,
            title: "Control public information",
            requirement: "Control information posted or processed on publicly accessible information systems.",
            responsibility: Responsibility::Hybrid,
            status: Status::NotAssessed,
            evidence: vec![file("eustress/crates/web/src")],
            notes: "A public web surface exists. Requires a review procedure for what is published — organizational control with a technical assist.",
        },
        Control {
            id: "IA.L1-3.5.1", nist_id: "3.5.1", far_paragraph: Some("(v)"),
            level: CmmcLevel::L1, domain: Domain::IA,
            title: "Identification",
            requirement: "Identify information system users, processes acting on behalf of users, and devices.",
            responsibility: Responsibility::Technical,
            status: Status::Partial,
            evidence: vec![file("eustress/crates/identity/src/keypair.rs"), file("eustress/crates/identity/src/schema.rs")],
            notes: "Ed25519 keypair identity with issuance, succession and witness history. Gap: engine-local sessions and agent processes are not bound to an identity.",
        },
        Control {
            id: "IA.L1-3.5.2", nist_id: "3.5.2", far_paragraph: Some("(vi)"),
            level: CmmcLevel::L1, domain: Domain::IA,
            title: "Authentication",
            requirement: "Authenticate (or verify) the identities of users, processes, or devices, as a prerequisite to allowing access to organizational information systems.",
            responsibility: Responsibility::Technical,
            status: Status::Partial,
            evidence: vec![file("eustress/crates/identity/src/verifier.rs"), file("eustress/crates/engine/src/auth.rs")],
            notes: "Signature verification exists. Gap: no authentication required to reach the engine bridge; BYOK API keys are stored unencrypted at ~/.eustress_engine/soul_settings.json.",
        },
        Control {
            id: "MP.L1-3.8.3", nist_id: "3.8.3", far_paragraph: Some("(vii)"),
            level: CmmcLevel::L1, domain: Domain::MP,
            title: "Media disposal",
            requirement: "Sanitize or destroy information system media containing Federal Contract Information before disposal or release for reuse.",
            responsibility: Responsibility::Hybrid,
            status: Status::NotImplemented,
            evidence: vec![],
            notes: "Deleted entities move to .eustress/trash/ and persist indefinitely; Fjall stores are not securely erased. Sanitization is largely organizational, but the trash retention default is a technical gap.",
        },
        Control {
            id: "PE.L1-3.10.1", nist_id: "3.10.1", far_paragraph: Some("(viii)"),
            level: CmmcLevel::L1, domain: Domain::PE,
            title: "Limit physical access",
            requirement: "Limit physical access to organizational information systems, equipment, and the respective operating environments to authorized individuals.",
            responsibility: Responsibility::Physical,
            status: Status::NotApplicable,
            evidence: vec![Evidence::Process { description: "Facility control — assessed against the operating site, not this repository.".into() }],
            notes: "No code can satisfy this. Recorded so the register reconciles to all 17 practices rather than silently omitting the ones software cannot reach.",
        },
        Control {
            id: "PE.L1-3.10.3", nist_id: "3.10.3", far_paragraph: Some("(ix)"),
            level: CmmcLevel::L1, domain: Domain::PE,
            title: "Escort visitors",
            requirement: "Escort visitors and monitor visitor activity.",
            responsibility: Responsibility::Physical,
            status: Status::NotApplicable,
            evidence: vec![Evidence::Process { description: "Facility control.".into() }],
            notes: "First of the three obligations bundled into FAR paragraph (ix).",
        },
        Control {
            id: "PE.L1-3.10.4", nist_id: "3.10.4", far_paragraph: Some("(ix)"),
            level: CmmcLevel::L1, domain: Domain::PE,
            title: "Physical access logs",
            requirement: "Maintain audit logs of physical access.",
            responsibility: Responsibility::Physical,
            status: Status::NotApplicable,
            evidence: vec![Evidence::Process { description: "Facility control.".into() }],
            notes: "Second obligation from FAR paragraph (ix).",
        },
        Control {
            id: "PE.L1-3.10.5", nist_id: "3.10.5", far_paragraph: Some("(ix)"),
            level: CmmcLevel::L1, domain: Domain::PE,
            title: "Manage physical access devices",
            requirement: "Control and manage physical access devices.",
            responsibility: Responsibility::Physical,
            status: Status::NotApplicable,
            evidence: vec![Evidence::Process { description: "Facility control.".into() }],
            notes: "Third obligation from FAR paragraph (ix).",
        },
        Control {
            id: "SC.L1-3.13.1", nist_id: "3.13.1", far_paragraph: Some("(x)"),
            level: CmmcLevel::L1, domain: Domain::SC,
            title: "Boundary protection",
            requirement: "Monitor, control, and protect organizational communications at the external boundaries and key internal boundaries of information systems.",
            responsibility: Responsibility::Technical,
            status: Status::Partial,
            evidence: vec![file("eustress/crates/engine/src/engine_bridge/mod.rs")],
            notes: "TLS is available via rustls for outbound calls. Gap: the engine bridge listens on TCP and is discovered via a port file with no transport security or peer authentication.",
        },
        Control {
            id: "SC.L1-3.13.5", nist_id: "3.13.5", far_paragraph: Some("(xi)"),
            level: CmmcLevel::L1, domain: Domain::SC,
            title: "Public-facing subnetworks",
            requirement: "Implement subnetworks for publicly accessible system components that are physically or logically separated from internal networks.",
            responsibility: Responsibility::Organizational,
            status: Status::NotApplicable,
            evidence: vec![Evidence::Process { description: "Network architecture of the deployment environment.".into() }],
            notes: "A deployment-topology control. The Cloudflare Worker surface is already separated from the desktop engine, but the assessment is of the network, not the code.",
        },
        Control {
            id: "SI.L1-3.14.1", nist_id: "3.14.1", far_paragraph: Some("(xii)"),
            level: CmmcLevel::L1, domain: Domain::SI,
            title: "Flaw remediation",
            requirement: "Identify, report, and correct information and information system flaws in a timely manner.",
            responsibility: Responsibility::Hybrid,
            status: Status::Partial,
            evidence: vec![file(".github/workflows/ci.yml")],
            notes: "CI exists and runs cargo-deny for advisories. Gap: CI does not build or test the workspace, so a flaw that breaks the build is not caught by the gate.",
        },
        Control {
            id: "SI.L1-3.14.2", nist_id: "3.14.2", far_paragraph: Some("(xiii)"),
            level: CmmcLevel::L1, domain: Domain::SI,
            title: "Malicious code protection",
            requirement: "Provide protection from malicious code at appropriate locations within organizational information systems.",
            responsibility: Responsibility::Hybrid,
            status: Status::NotAssessed,
            evidence: vec![],
            notes: "Endpoint AV is an organizational control. The technical exposure worth assessing is Eustress's own execution surface: run_bash, execute_luau and execute_rune run agent-authored code, and the Luau/Rune sandbox boundary has not been assessed against this control.",
        },
        Control {
            id: "SI.L1-3.14.4", nist_id: "3.14.4", far_paragraph: Some("(xiv)"),
            level: CmmcLevel::L1, domain: Domain::SI,
            title: "Update malicious code protection",
            requirement: "Update malicious code protection mechanisms when new releases are available.",
            responsibility: Responsibility::Organizational,
            status: Status::NotApplicable,
            evidence: vec![Evidence::Process { description: "Endpoint protection maintenance on the operating environment.".into() }],
            notes: "Follows 3.14.2 and is satisfied by the same endpoint tooling.",
        },
        Control {
            id: "SI.L1-3.14.5", nist_id: "3.14.5", far_paragraph: Some("(xv)"),
            level: CmmcLevel::L1, domain: Domain::SI,
            title: "Periodic and real-time scans",
            requirement: "Perform periodic scans of the information system and real-time scans of files from external sources as files are downloaded, opened, or executed.",
            responsibility: Responsibility::Hybrid,
            status: Status::NotImplemented,
            evidence: vec![],
            notes: "Eustress downloads external content (mesh imports, Gradio artifacts, media fetch) and opens it without scanning. This one is genuinely technical and genuinely unmet.",
        },
    ]
}

// ============================================================================
// 3. Assessment roll-up
// ============================================================================

/// Summary of a register's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub total: usize,
    pub met: usize,
    pub partial: usize,
    pub not_implemented: usize,
    pub not_assessed: usize,
    /// Controls software can actually move (Technical or Hybrid).
    pub actionable_in_code: usize,
}

impl Assessment {
    /// Level 1 is pass/fail with no POA&M: every control must be met.
    pub fn level_1_passes(&self) -> bool {
        self.met == self.total
    }
}

/// Roll up a control set.
pub fn assess(controls: &[Control]) -> Assessment {
    Assessment {
        total: controls.len(),
        met: controls.iter().filter(|c| c.status.is_met()).count(),
        partial: controls.iter().filter(|c| c.status == Status::Partial).count(),
        not_implemented: controls.iter().filter(|c| c.status == Status::NotImplemented).count(),
        not_assessed: controls.iter().filter(|c| c.status == Status::NotAssessed).count(),
        actionable_in_code: controls
            .iter()
            .filter(|c| matches!(c.responsibility, Responsibility::Technical | Responsibility::Hybrid))
            .count(),
    }
}

/// DoD NIST SP 800-171 Assessment Methodology score, as submitted to SPRS.
///
/// Starts at 110 and subtracts each unmet control's weight (5, 3, or 1).
/// The floor is −203 when nothing is implemented, so a negative score is
/// normal and expected early — it is a measurement, not a grade.
///
/// `weighted_unmet` is `(control_id, weight)` for each control not met.
pub fn sprs_score(weighted_unmet: &[(&str, i32)]) -> i32 {
    110 - weighted_unmet.iter().map(|(_, w)| *w).sum::<i32>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn level_1_has_seventeen_nist_ids_across_fifteen_far_paragraphs() {
        let controls = level_1_controls();
        assert_eq!(controls.len(), 17, "17 NIST-mapped practice IDs");

        let mut paragraphs: Vec<&str> =
            controls.iter().filter_map(|c| c.far_paragraph).collect();
        paragraphs.sort_unstable();
        paragraphs.dedup();
        assert_eq!(paragraphs.len(), 15, "15 distinct FAR 52.204-21(b)(1) paragraphs");

        // Paragraph (ix) is the one that bundles three obligations.
        let ix = controls.iter().filter(|c| c.far_paragraph == Some("(ix)")).count();
        assert_eq!(ix, 3, "FAR (ix) expands to three PE controls");
    }

    #[test]
    fn level_1_spans_six_domains_including_si() {
        let controls = level_1_controls();
        let mut domains: Vec<Domain> = controls.iter().map(|c| c.domain).collect();
        domains.sort_by_key(|d| d.name());
        domains.dedup();
        assert_eq!(domains.len(), 6, "AC, IA, MP, PE, SC, SI");
        assert!(
            controls.iter().any(|c| c.domain == Domain::SI),
            "SI is frequently omitted from summaries but is FAR (xii)-(xv)"
        );
    }

    #[test]
    fn every_file_evidence_path_exists() {
        // The reason this register is code and not a spreadsheet: evidence
        // that points at a deleted file fails the build instead of rotting.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root")
            .to_path_buf();
        for control in level_1_controls() {
            for ev in &control.evidence {
                if let Evidence::FileLine { path, .. } = ev {
                    assert!(
                        repo_root.join(path).exists(),
                        "control {} cites missing evidence: {}",
                        control.id,
                        path
                    );
                }
            }
        }
    }

    #[test]
    fn a_status_claim_requires_evidence() {
        // Implemented or Partial without evidence is an assertion, not an
        // assessment — exactly what this register exists to prevent.
        for control in level_1_controls() {
            if matches!(control.status, Status::Implemented | Status::Partial) {
                assert!(
                    !control.evidence.is_empty(),
                    "control {} claims {:?} with no evidence",
                    control.id,
                    control.status
                );
            }
        }
    }

    #[test]
    fn register_does_not_currently_pass_level_1() {
        // Honest baseline. If this ever flips to passing, it must be because
        // controls were implemented — and this test will say so loudly.
        let a = assess(&level_1_controls());
        assert!(
            !a.level_1_passes(),
            "Level 1 is all-or-nothing; {} of {} met",
            a.met,
            a.total
        );
        assert!(a.not_implemented > 0 || a.partial > 0 || a.not_assessed > 0);
    }

    #[test]
    fn physical_controls_are_never_marked_technical() {
        for c in level_1_controls() {
            if c.domain == Domain::PE {
                assert_eq!(
                    c.responsibility,
                    Responsibility::Physical,
                    "{} is a facility control and code cannot satisfy it",
                    c.id
                );
            }
        }
    }

    #[test]
    fn sprs_scoring_matches_dod_methodology() {
        assert_eq!(sprs_score(&[]), 110, "all 110 implemented");
        assert_eq!(sprs_score(&[("3.1.1", 5), ("3.5.3", 5), ("3.13.11", 3)]), 97);
        // The methodology's floor is -203 when nothing is implemented.
        let nothing_done: Vec<(&str, i32)> = vec![("x", 313)];
        assert_eq!(sprs_score(&nothing_done), -203);
    }
}
