//! Ribbon dispatch for the Public Sector mode's live tools.
//!
//! One entry point, [`handle`], so `slint_ui.rs` carries a single routing arm
//! instead of twenty. Everything here reads and writes files under the Space,
//! which means every button is reproducible from the command line and every
//! result is inspectable without the engine.
//!
//! Each handler returns a [`Report`]: a toast title, a one-line summary, and
//! the detail that goes to the Output console. Nothing here is silent, because
//! a capture tool whose button appears to do nothing is indistinguishable from
//! one that is broken.

use std::path::Path;

use chrono::Utc;

use super::sources::Profile;
use super::{audit, layout, store, CaptureError, PipelineRun};

/// What a handled action produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Report(Report),
    /// Ask the poll runtime to treat every Connector as due. The caller owns
    /// that resource, so the decision comes back rather than reaching across.
    RequestSync,
    /// Open the API key dialog for one credential. The value never passes
    /// through here: the dialog writes it straight to the credential store.
    OpenApiKeyDialog { name: String, status: String, present: bool, from_environment: bool },
}

/// A tool's answer, in the three shapes the UI needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Toast title. Short.
    pub title: String,
    /// One line, for the toast body and the status bar.
    pub summary: String,
    /// The full answer, for the Output console. May be many lines.
    pub detail: String,
}

impl Report {
    fn new(title: &str, summary: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { title: title.to_string(), summary: summary.into(), detail: detail.into() }
    }
}

/// How many rows a list-shaped report prints before it says how many it cut.
const LIST_LIMIT: usize = 25;

/// Route one ribbon tool id.
///
/// Returns `None` for an id this mode does not own, so the caller falls through
/// to its own dispatch untouched.
pub fn handle(action: &str, space_root: &Path) -> Option<Result<Outcome, CaptureError>> {
    if !action.starts_with("pcap:") && !action.starts_with("pprp:") && !action.starts_with("pcmp:")
    {
        return None;
    }
    let result = match action {
        "pcap:add_sam_source" => add_source(space_root, Profile::SamOpportunities),
        "pcap:add_grants_source" => add_source(space_root, Profile::GrantsSearch),
        "pcap:sync_now" => return Some(Ok(Outcome::RequestSync)),
        "pcap:source_status" => source_status(space_root),
        "pcap:api_key_status" => return Some(Ok(api_key_dialog(space_root))),
        "pcap:capability_statement" => capability_statement(space_root),
        "pcap:screen_pipeline" => screen_report(space_root),
        "pcap:rejection_report" => rejection_report(space_root),
        "pcap:score_pipeline" => score_report(space_root),
        "pcap:no_bid_report" => no_bid_report(space_root),
        "pcap:calibrate_model" => calibrate_report(space_root),
        "pcap:capture_record_index" | "pcmp:capture_record_index" => record_index(space_root),
        "pcap:build_deal_room" => build_deal_room(space_root),
        "pprp:extract_requirements" => extract_report(space_root),
        "pprp:compliance_matrix" => matrix_report(space_root),
        "pprp:gap_report" => gap_report(space_root),
        "pprp:export_matrix_csv" => export_matrix(space_root),
        "pprp:response_outline" => outline_report(space_root),
        "pprp:draft_response" => draft_report(space_root),
        "pcmp:verify_audit_trail" => verify_trail(space_root),
        _ => return None,
    };
    Some(result.map(Outcome::Report))
}

/// Assemble a pipeline run from what is on disk, and file the result.
///
/// Every analysis tool runs this. It is pure local computation over already
/// fetched pages, so re-running per click keeps one code path and guarantees
/// the report matches what the folders hold.
fn run(space_root: &Path) -> Result<(PipelineRun, usize), CaptureError> {
    let firm = store::load_capability(space_root)?.to_statement();
    let notices = store::load_notices(space_root)?;
    let documents = store::load_documents(space_root, &notices);
    let now = Utc::now();
    let run = super::run_pipeline(&notices, &firm, &documents, now);
    let (receipts, errors) = super::write_run(space_root, &run, &now.to_rfc3339());
    for e in &errors {
        bevy::log::warn!("capture: a record could not be filed: {e}");
    }
    Ok((run, receipts.len()))
}

fn add_source(space_root: &Path, profile: Profile) -> Result<Report, CaptureError> {
    let dir = store::write_capture_connector(space_root, profile)?;
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let next = match profile {
        Profile::SamOpportunities =>
            "Three steps: click API Key and paste your free SAM.gov key, set \
             posted_from and posted_to (MM/dd/yyyy, at most a year apart) in \
             Properties, then set enabled = true.",
        Profile::GrantsSearch =>
            "Narrow it with aln, keyword, or agencies in Properties, then set enabled = true. \
             Grants.gov needs no key.",
    };
    Ok(Report::new(
        "Source added",
        format!("Created '{name}' in DataService, disabled"),
        format!("{}\n\n{next}\n\nNothing polls until you enable it.", dir.display()),
    ))
}

/// Open the key dialog, pre-addressed to the credential the Space's own SAM
/// Connector names.
///
/// A firm running two clients from one machine keeps a key per Connector via
/// `secret_ref`, so the dialog has to follow the Connector rather than assume
/// the default name.
fn api_key_dialog(space_root: &Path) -> Outcome {
    let name = super::poll::discover(space_root)
        .into_iter()
        .find(|s| s.profile == Profile::SamOpportunities)
        .and_then(|s| s.secret_ref)
        .unwrap_or_else(|| super::credentials::DEFAULT_SAM_KEY.to_string());
    let status = super::credentials::status(&name);
    Outcome::OpenApiKeyDialog {
        name: status.name.clone(),
        status: status.summary(),
        present: status.present,
        from_environment: status.from_environment,
    }
}

fn source_status(space_root: &Path) -> Result<Report, CaptureError> {
    let specs = super::poll::discover(space_root);
    if specs.is_empty() {
        return Ok(Report::new(
            "No sources",
            "No capture Connectors in this Space",
            "Add a SAM.gov or Grants.gov source first.",
        ));
    }
    let enabled = specs.iter().filter(|s| s.enabled).count();
    let mut detail = String::new();
    for s in &specs {
        let name = s.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        detail.push_str(&format!(
            "{:<40} {:<20} {:>8}  every {}s\n",
            name,
            s.profile.as_str(),
            if s.enabled { "enabled" } else { "disabled" },
            s.poll_seconds
        ));
    }
    let raw = store::raw_dir(space_root);
    detail.push_str(&format!("\nFetched pages land in {}\n", raw.display()));
    Ok(Report::new(
        "Source status",
        format!("{} source(s), {enabled} enabled", specs.len()),
        detail,
    ))
}

fn capability_statement(space_root: &Path) -> Result<Report, CaptureError> {
    let (path, created) = store::ensure_capability(space_root)?;
    let file = store::load_capability(space_root)?;
    let missing = file.missing_fields();
    let summary = if created {
        "Created a starter capability statement".to_string()
    } else if missing.is_empty() {
        format!("{} is complete", file.legal_name)
    } else {
        format!("{} field(s) still empty", missing.len())
    };
    let mut detail = format!("{}\n\n", path.display());
    if missing.is_empty() {
        detail.push_str("Every screening input is filled in.\n");
    } else {
        detail.push_str("Screening runs against this file. Still empty:\n");
        for m in &missing {
            let why = match *m {
                "naics" => "without it nothing is filtered by industry",
                "certifications" => "without them every set-aside reads as out of reach",
                "capabilities" => "without them every requirement scores as a gap",
                "past_performance" => "without it the calibration harness cannot run",
                _ => "",
            };
            detail.push_str(&format!("  {m:<18} {why}\n"));
        }
    }
    Ok(Report::new("Capability statement", summary, detail))
}

fn screen_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, filed) = run(space_root)?;
    let mut detail = format!("{}\n\n", r.headline());
    detail.push_str(&format!("{filed} record(s) written under Capture/\n\n"));
    for a in r.ranked().into_iter().take(LIST_LIMIT) {
        let days = a
            .opportunity
            .days_until_deadline(Utc::now())
            .map(|d| format!("{d}d"))
            .unwrap_or_else(|| "-".into());
        detail.push_str(&format!(
            "{:>3}%  {:<8} {:>5}  {}\n",
            a.score.percent(),
            a.score.recommendation.label(),
            days,
            truncate(&a.opportunity.title, 70)
        ));
    }
    if r.passed.len() > LIST_LIMIT {
        detail.push_str(&format!("... and {} more\n", r.passed.len() - LIST_LIMIT));
    }
    Ok(Report::new("Screening", r.headline(), detail))
}

fn rejection_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    if r.rejected.is_empty() {
        return Ok(Report::new(
            "Rejections",
            "Nothing was screened out",
            "Every fetched notice cleared the gates. If that seems too good, check that \
             capability.toml actually has NAICS codes and certifications in it.",
        ));
    }
    let mut detail = format!("{} of {} screened out.\n\n", r.rejected.len(), r.screened);
    detail.push_str("By reason, most costly first:\n");
    for (reason, count) in &r.top_reasons {
        detail.push_str(&format!("  {count:>4}  {reason}\n"));
    }
    detail.push_str(
        "\nThe top line is the single change that would unlock the most pipeline: a \
         registration to add, a certification to pursue, or a capacity limit to revisit.\n",
    );
    let top = r.top_reasons.first().map(|(k, v)| format!("{v} x {k}")).unwrap_or_default();
    Ok(Report::new("Rejections", format!("{} screened out, mostly {top}", r.rejected.len()), detail))
}

fn score_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let ranked = r.ranked();
    if ranked.is_empty() {
        return Ok(Report::new(
            "Fit",
            "Nothing cleared screening",
            "Run the rejection report to see which gate is removing everything.",
        ));
    }
    let mut detail = format!("{}\n\n", r.headline());
    for a in ranked.iter().take(10) {
        detail.push_str(&format!(
            "── {:>3}%  {}  [{}]\n",
            a.score.percent(),
            truncate(&a.opportunity.title, 60),
            a.opportunity.notice_id
        ));
        detail.push_str(&a.score.explain());
        detail.push('\n');
    }
    if ranked.len() > 10 {
        detail.push_str(&format!("... and {} more, all filed under Capture/\n", ranked.len() - 10));
    }
    Ok(Report::new("Fit", r.headline(), detail))
}

fn no_bid_report(space_root: &Path) -> Result<Report, CaptureError> {
    use super::score::Recommendation;
    let (r, _) = run(space_root)?;
    let against: Vec<_> = r
        .ranked()
        .into_iter()
        .filter(|a| !matches!(a.score.recommendation, Recommendation::Bid))
        .collect();

    if against.is_empty() {
        return Ok(Report::new(
            "No-bid",
            "Nothing is advised against",
            "Every eligible notice scored as worth bidding. That is unusual; if the pipeline \
             is large, check that capability.toml has real past performance in it, since an \
             empty history is what most often flattens the ranking.",
        ));
    }
    let mut detail = format!("{} of {} eligible notice(s) not recommended.\n\n", against.len(), r.passed.len());
    for a in against.iter().take(LIST_LIMIT) {
        detail.push_str(&format!(
            "{:<8} {:>3}%  {}\n",
            a.score.recommendation.label(),
            a.score.percent(),
            truncate(&a.opportunity.title, 66)
        ));
        for reason in a.score.recommendation.reasons() {
            detail.push_str(&format!("           {reason}\n"));
        }
    }
    if against.len() > LIST_LIMIT {
        detail.push_str(&format!("... and {} more\n", against.len() - LIST_LIMIT));
    }
    Ok(Report::new("No-bid", format!("{} not recommended", against.len()), detail))
}

fn calibrate_report(space_root: &Path) -> Result<Report, CaptureError> {
    let firm = store::load_capability(space_root)?.to_statement();
    let c = super::score::calibrate(&firm, Utc::now());
    let mut detail = format!("{}\n\n", c.verdict());
    detail.push_str(&format!("scored outcomes     {}\n", c.scored));
    detail.push_str(&format!("recommended, won    {}\n", c.true_positive));
    detail.push_str(&format!("recommended, lost   {}\n", c.false_positive));
    detail.push_str(&format!("advised against, won  {}   <- the expensive error\n", c.false_negative));
    detail.push_str(&format!("advised against, lost {}\n", c.true_negative));
    detail.push_str(
        "\nEach reference is replayed against a history that excludes itself, so a entry \
         cannot vouch for its own past performance. Record `won = true/false` on past \
         performance entries in capability.toml to make this sharper.\n",
    );
    Ok(Report::new("Calibration", c.verdict(), detail))
}

fn record_index(space_root: &Path) -> Result<Report, CaptureError> {
    let records = audit::list_records(space_root);
    if records.is_empty() {
        return Ok(Report::new(
            "Records",
            "No capture records yet",
            "Run screening once and every assessed notice gets a folder under Capture/.",
        ));
    }
    let mut detail = format!("{} record(s) under Capture/\n\n", records.len());
    for p in records.iter().take(LIST_LIMIT * 2) {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        detail.push_str(&format!("  {name}\n"));
    }
    if records.len() > LIST_LIMIT * 2 {
        detail.push_str(&format!("... and {} more\n", records.len() - LIST_LIMIT * 2));
    }
    detail.push_str(
        "\nEach folder holds the raw payload, the screen, the score with its derivation, the \
         compliance matrix, the draft, and a Merkle manifest over all of it.\n",
    );
    Ok(Report::new("Records", format!("{} capture record(s)", records.len()), detail))
}

fn build_deal_room(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let ranked = r.ranked();
    if ranked.is_empty() {
        return Ok(Report::new(
            "Deal room",
            "Nothing to place",
            "No notice cleared screening, so the room would be empty.",
        ));
    }
    let room = layout::build(&ranked, Utc::now());
    let written = layout::write_room(space_root, &room)?;
    let (bid, watch, no) = room.lane_counts();
    let detail = format!(
        "{written} node(s) written to {}\n\n\
         Lanes:  {bid} bid, {watch} watch, {no} no-bid\n\
         X  days to deadline, nearest the origin closes soonest\n\
         Y  fit score, taller is a better fit\n\
         Z  the lane: bid at the front, then watch, then no-bid\n\
         size   award ceiling\n\n\
         The file watcher spawns these; open the MindSpace tab and walk in.\n",
        layout::DEAL_ROOM_DIR
    );
    Ok(Report::new("Deal room", room.headline(), detail))
}

fn extract_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let with_text: Vec<_> = r.passed.iter().filter(|a| a.matrix.is_some()).collect();
    if with_text.is_empty() {
        return Ok(Report::new(
            "Requirements",
            "No requirement text available",
            "SAM.gov's search payload carries a LINK to the description, not the text, so a \
             notice has no requirements until its attachment is fetched. Drop the statement \
             of work as `requirements.txt` inside the notice's folder under Capture/ and run \
             this again.",
        ));
    }
    let total: usize = with_text.iter().filter_map(|a| a.matrix.as_ref()).map(|m| m.rows.len()).sum();
    let mut detail = format!(
        "{} of {} eligible notice(s) had text to read; {total} binding requirement(s) found.\n\n",
        with_text.len(),
        r.passed.len()
    );
    for a in with_text.iter().take(LIST_LIMIT) {
        let m = a.matrix.as_ref().expect("filtered");
        detail.push_str(&format!(
            "{:>4} reqs  {:>3}% covered  {}\n",
            m.rows.len(),
            m.coverage_percent(),
            truncate(&a.opportunity.title, 60)
        ));
    }
    Ok(Report::new("Requirements", format!("{total} binding requirement(s)"), detail))
}

fn matrix_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let matrices: Vec<_> = r.passed.iter().filter_map(|a| a.matrix.as_ref().map(|m| (a, m))).collect();
    if matrices.is_empty() {
        return extract_report(space_root);
    }
    let mut detail = String::new();
    for (a, m) in matrices.iter().take(LIST_LIMIT) {
        detail.push_str(&format!(
            "{}\n  {}\n  compliance.md written to Capture/{}/\n\n",
            truncate(&a.opportunity.title, 70),
            m.headline(),
            a.opportunity.slug()
        ));
    }
    let worst = matrices
        .iter()
        .min_by_key(|(_, m)| m.coverage_percent())
        .map(|(_, m)| m.coverage_percent())
        .unwrap_or(0);
    Ok(Report::new(
        "Compliance matrix",
        format!("{} matrix(es) written, lowest coverage {worst}%", matrices.len()),
        detail,
    ))
}

fn gap_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let mut rows: Vec<(String, String)> = Vec::new();
    for a in &r.passed {
        let Some(m) = a.matrix.as_ref() else { continue };
        for g in m.gaps() {
            rows.push((
                truncate(&a.opportunity.title, 46),
                format!("{}: {}", g.requirement.citation(), truncate(&g.requirement.text, 90)),
            ));
        }
    }
    if rows.is_empty() {
        let assessed = r.passed.iter().filter(|a| a.matrix.is_some()).count();
        if assessed == 0 {
            return extract_report(space_root);
        }
        return Ok(Report::new(
            "Gaps",
            "No unanswered requirements",
            "Everything binding in the assessed solicitations matches a capability on file.",
        ));
    }
    let mut detail = format!("{} unanswered requirement(s).\n\n", rows.len());
    for (title, gap) in rows.iter().take(LIST_LIMIT * 2) {
        detail.push_str(&format!("{title}\n    {gap}\n"));
    }
    if rows.len() > LIST_LIMIT * 2 {
        detail.push_str(&format!("... and {} more\n", rows.len() - LIST_LIMIT * 2));
    }
    detail.push_str(
        "\nEach of these is a write-it, team-it, or no-bid decision. Adding the capability to \
         capability.toml closes it everywhere at once.\n",
    );
    Ok(Report::new("Gaps", format!("{} unanswered requirement(s)", rows.len()), detail))
}

fn export_matrix(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let mut written = 0usize;
    for a in &r.passed {
        if a.matrix.is_some() {
            written += 1;
        }
    }
    if written == 0 {
        return extract_report(space_root);
    }
    Ok(Report::new(
        "Export",
        format!("{written} compliance.csv file(s) written"),
        format!(
            "One compliance.csv per assessed notice, under Capture/<notice>/.\n\
             Columns: clause, section, coverage, response_section, requirement, note.\n\
             {} record folder(s) in total.\n",
            audit::list_records(space_root).len()
        ),
    ))
}

fn outline_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let outlines: Vec<_> = r.passed.iter().filter_map(|a| a.outline.as_ref().map(|o| (a, o))).collect();
    if outlines.is_empty() {
        return extract_report(space_root);
    }
    let mut detail = String::new();
    for (a, o) in outlines.iter().take(LIST_LIMIT) {
        detail.push_str(&format!("{}\n  {}\n", truncate(&a.opportunity.title, 70), o.headline()));
        for s in o.blocked() {
            detail.push_str(&format!("    blocked: {} {}\n", s.number, s.title));
        }
        detail.push('\n');
    }
    let ready = outlines.iter().filter(|(_, o)| o.blocked().is_empty()).count();
    Ok(Report::new(
        "Outline",
        format!("{} outline(s), {ready} with no blocked section", outlines.len()),
        detail,
    ))
}

fn draft_report(space_root: &Path) -> Result<Report, CaptureError> {
    let (r, _) = run(space_root)?;
    let drafted: Vec<_> = r.passed.iter().filter(|a| a.outline.is_some()).collect();
    if drafted.is_empty() {
        return extract_report(space_root);
    }
    let mut detail = String::new();
    for a in drafted.iter().take(LIST_LIMIT) {
        let o = a.outline.as_ref().expect("filtered");
        detail.push_str(&format!(
            "Capture/{}/draft.md   {}% writable\n",
            a.opportunity.slug(),
            o.readiness_percent()
        ));
    }
    detail.push_str(
        "\nSkeleton drafts. Every paragraph is either your own capability text or a marked \
         gap, and every claim carries the clause it answers. Nothing was invented.\n\n\
         The engine drafts; you file. Nothing here submits anything.\n",
    );
    Ok(Report::new(
        "Draft",
        format!("{} draft(s) written", drafted.len()),
        detail,
    ))
}

fn verify_trail(space_root: &Path) -> Result<Report, CaptureError> {
    let records = audit::list_records(space_root);
    if records.is_empty() {
        return Ok(Report::new(
            "Verify",
            "No records to verify",
            "Run screening once and every assessed notice gets a folder under Capture/.",
        ));
    }
    let mut intact = 0usize;
    let mut altered = Vec::new();
    for dir in &records {
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match audit::verify_record(dir) {
            Ok(v) if v.intact => intact += 1,
            Ok(v) => altered.push(format!("{name}: {}", v.explain())),
            Err(e) => altered.push(format!("{name}: unreadable: {e}")),
        }
    }
    let summary = if altered.is_empty() {
        format!("{intact} record(s) intact")
    } else {
        format!("{} of {} record(s) ALTERED", altered.len(), records.len())
    };
    let mut detail = format!("{summary}\n\n");
    for a in altered.iter().take(LIST_LIMIT) {
        detail.push_str(&format!("  {a}\n"));
    }
    detail.push_str(
        "\nVerification re-derives each folder's Merkle root from its own files. It proves the \
         folder has not changed since it was written. It does NOT attest who wrote it.\n",
    );
    Ok(Report::new("Verify", summary, detail))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("eustress-ribbon-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A Space with a real profile and one fetched page.
    fn seeded(name: &str) -> std::path::PathBuf {
        let root = temp(name);
        std::fs::create_dir_all(root.join("Capture")).unwrap();
        std::fs::write(
            store::capability_path(&root),
            r#"
legal_name = "Example Systems LLC"
naics = ["541511"]
certifications = ["sdvosb"]
ceiling_capacity = 2000000
floor = 50000
min_response_days = 10

[capabilities]
help_desk = "Tier 2 help desk support during core hours with triage and escalation."

[[past_performance]]
customer = "US Army"
title = "Enterprise Help Desk"
naics = "541511"
value = 600000
year = 2025
won = true
summary = "24x7 Tier 2 support."
"#,
        )
        .unwrap();

        let raw = store::raw_dir(&root).join("SAM Source");
        std::fs::create_dir_all(&raw).unwrap();
        let deadline = (Utc::now() + chrono::Duration::days(45)).format("%Y-%m-%d").to_string();
        std::fs::write(
            raw.join("page-001.json"),
            format!(
                r#"{{"totalRecords":2,"opportunitiesData":[
                {{"noticeId":"fit","title":"Tier 2 Help Desk Support","naicsCode":"541511",
                  "typeOfSetAside":"SDVOSBC","responseDeadLine":"{deadline}",
                  "award":{{"amount":"900000"}},"uiLink":"https://sam.gov/opp/fit/view"}},
                {{"noticeId":"nofit","title":"Bridge Construction","naicsCode":"237310",
                  "responseDeadLine":"{deadline}"}}
                ]}}"#
            ),
        )
        .unwrap();
        root
    }

    #[test]
    fn an_id_from_another_mode_is_not_claimed() {
        let root = temp("foreign");
        assert!(handle("data:import", &root).is_none());
        assert!(handle("csg:union", &root).is_none());
        assert!(handle("gprc:rfp_composer", &root).is_none(), "the buyer-side id belongs to Government");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unhandled_id_inside_our_prefix_is_declined_rather_than_faked() {
        let root = temp("unhandled");
        assert!(handle("pcap:teaming_agreement_tracker", &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_key_dialog_follows_the_connector_that_names_the_credential() {
        let root = temp("keydialog");
        // With no Connector yet, the dialog addresses the default credential.
        let Some(Ok(Outcome::OpenApiKeyDialog { name, present, .. })) =
            handle("pcap:api_key_status", &root)
        else {
            panic!("expected the key dialog")
        };
        assert_eq!(name, super::super::credentials::DEFAULT_SAM_KEY);
        let _ = present;

        // A Connector naming its own credential retargets the dialog, which is
        // what lets one machine hold a key per client.
        let ds = root.join("DataService").join("Client B");
        std::fs::create_dir_all(&ds).unwrap();
        std::fs::write(
            ds.join("_instance.toml"),
            "[metadata]
class_name = \"Connector\"

[attributes]
profile = \"sam\"
secret_ref = \"SAM_API_KEY_CLIENT_B\"
",
        )
        .unwrap();
        let Some(Ok(Outcome::OpenApiKeyDialog { name, .. })) =
            handle("pcap:api_key_status", &root)
        else {
            panic!("expected the key dialog")
        };
        assert_eq!(name, "SAM_API_KEY_CLIENT_B");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_now_asks_the_caller_rather_than_reaching_for_the_resource() {
        let root = temp("sync");
        assert_eq!(handle("pcap:sync_now", &root), Some(Ok(Outcome::RequestSync)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn adding_a_source_creates_a_disabled_connector_and_says_what_is_next() {
        let root = temp("add");
        let Some(Ok(Outcome::Report(r))) = handle("pcap:add_sam_source", &root) else {
            panic!("expected a report")
        };
        assert!(r.summary.contains("disabled"), "{}", r.summary);
        assert!(r.detail.contains("Nothing polls until you enable it"), "{}", r.detail);
        // The guidance must name the BUTTON. Telling a grant writer to set an
        // environment variable and relaunch is not a workflow, and this
        // assertion is what keeps that decision from regressing.
        assert!(r.detail.contains("API Key"), "{}", r.detail);
        assert!(
            !r.detail.contains("environment variable"),
            "the setup guidance must not send the user to an environment variable: {}",
            r.detail
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn screening_with_no_profile_says_to_create_one() {
        let root = temp("no-profile");
        let Some(Err(e)) = handle("pcap:screen_pipeline", &root) else {
            panic!("expected an error")
        };
        assert!(matches!(e, CaptureError::Io(_)), "{e:?}");
        assert!(e.to_string().contains("capability.toml"), "{e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn screening_separates_fit_from_no_fit_and_files_the_survivor() {
        let root = seeded("screen");
        let Some(Ok(Outcome::Report(r))) = handle("pcap:screen_pipeline", &root) else {
            panic!("expected a report")
        };
        assert!(r.summary.starts_with("2 screened, 1 eligible"), "{}", r.summary);
        assert_eq!(audit::list_records(&root).len(), 1, "the eligible notice is filed");
        assert!(audit::verify_record(&audit::list_records(&root)[0]).unwrap().intact);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_rejection_report_names_the_gate_that_costs_the_most() {
        let root = seeded("reject");
        let Some(Ok(Outcome::Report(r))) = handle("pcap:rejection_report", &root) else {
            panic!("expected a report")
        };
        assert!(r.detail.contains("wrong classification"), "{}", r.detail);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_score_report_shows_its_derivation_not_just_a_number() {
        let root = seeded("score");
        let Some(Ok(Outcome::Report(r))) = handle("pcap:score_pipeline", &root) else {
            panic!("expected a report")
        };
        assert!(r.detail.contains("past performance"), "{}", r.detail);
        assert!(r.detail.contains("set-aside advantage"), "{}", r.detail);
        assert!(r.detail.contains("TOTAL"), "{}", r.detail);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_deal_room_writes_nodes_the_file_watcher_can_spawn() {
        let root = seeded("dealroom");
        let Some(Ok(Outcome::Report(r))) = handle("pcap:build_deal_room", &root) else {
            panic!("expected a report")
        };
        assert!(r.summary.contains("node(s)"), "{}", r.summary);
        let folder = root.join(layout::DEAL_ROOM_DIR).join("_instance.toml");
        assert!(folder.exists(), "the deal room folder must exist on disk");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn requirement_tools_explain_the_sam_description_link_rather_than_reporting_zero() {
        // The seeded page has no requirement text, which is the normal SAM
        // case. Reporting "0 requirements" would read as a compliant-with-
        // nothing solicitation, which is the dangerous answer.
        let root = seeded("extract");
        let Some(Ok(Outcome::Report(r))) = handle("pprp:extract_requirements", &root) else {
            panic!("expected a report")
        };
        assert!(r.detail.contains("requirements.txt"), "{}", r.detail);
        assert!(r.detail.contains("carries a LINK"), "{}", r.detail);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_dropped_statement_of_work_flows_all_the_way_to_a_draft() {
        let root = seeded("draft");
        // Screen once so the record folder exists, then drop the SOW in it.
        handle("pcap:screen_pipeline", &root).unwrap().unwrap();
        std::fs::write(
            root.join("Capture").join("sam-fit").join("requirements.txt"),
            "C.1 The Contractor shall provide Tier 2 help desk support during core hours \
             with triage and escalation.\n\
             C.2 The Contractor shall hold an active facility clearance.\n",
        )
        .unwrap();

        let Some(Ok(Outcome::Report(r))) = handle("pprp:draft_response", &root) else {
            panic!("expected a report")
        };
        assert!(r.summary.contains("1 draft"), "{}", r.summary);
        let draft = std::fs::read_to_string(root.join("Capture").join("sam-fit").join("draft.md")).unwrap();
        assert!(draft.contains("Tier 2 help desk"), "the firm's own words seed the draft");
        assert!(draft.contains("C.1"), "every claim cites the clause it answers");

        // And the gap surfaces rather than being papered over.
        let Some(Ok(Outcome::Report(g))) = handle("pprp:gap_report", &root) else {
            panic!("expected a report")
        };
        assert!(g.detail.contains("facility clearance"), "{}", g.detail);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verification_catches_an_edited_record() {
        let root = seeded("verify");
        handle("pcap:screen_pipeline", &root).unwrap().unwrap();
        let dir = audit::list_records(&root).remove(0);

        let Some(Ok(Outcome::Report(clean))) = handle("pcmp:verify_audit_trail", &root) else {
            panic!("expected a report")
        };
        assert!(clean.summary.contains("intact"), "{}", clean.summary);

        let target = dir.join("score.json");
        let text = std::fs::read_to_string(&target).unwrap().replace("\"total\"", "\"tota1\"");
        std::fs::write(&target, text).unwrap();

        let Some(Ok(Outcome::Report(dirty))) = handle("pcmp:verify_audit_trail", &root) else {
            panic!("expected a report")
        };
        assert!(dirty.summary.contains("ALTERED"), "{}", dirty.summary);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn calibration_refuses_a_number_it_cannot_support() {
        let root = seeded("calibrate");
        let Some(Ok(Outcome::Report(r))) = handle("pcap:calibrate_model", &root) else {
            panic!("expected a report")
        };
        // One recorded outcome is not a hit rate, and saying so is the point.
        assert!(r.summary.contains("too few to calibrate"), "{}", r.summary);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_wired_id_is_actually_routed() {
        // The WIRED dict in gen_tool_metadata.py is the single source of truth
        // for `ToolMeta.wired`. An id marked wired but not routed here reaches
        // a silent no-op, which is the exact failure the honesty toast exists
        // to prevent.
        let root = seeded("routed");
        for id in [
            "pcap:add_sam_source",
            "pcap:add_grants_source",
            "pcap:sync_now",
            "pcap:source_status",
            "pcap:api_key_status",
            "pcap:capability_statement",
            "pcap:screen_pipeline",
            "pcap:rejection_report",
            "pcap:score_pipeline",
            "pcap:no_bid_report",
            "pcap:calibrate_model",
            "pcap:capture_record_index",
            "pcap:build_deal_room",
            "pprp:extract_requirements",
            "pprp:compliance_matrix",
            "pprp:gap_report",
            "pprp:export_matrix_csv",
            "pprp:response_outline",
            "pprp:draft_response",
            "pcmp:capture_record_index",
            "pcmp:verify_audit_trail",
        ] {
            let out = handle(id, &root);
            assert!(out.is_some(), "'{id}' is marked wired but nothing routes it");
            assert!(out.unwrap().is_ok(), "'{id}' failed against a seeded Space");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
