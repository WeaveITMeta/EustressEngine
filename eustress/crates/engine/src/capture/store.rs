//! The capture store: the firm profile and the fetched notices, both on disk.
//!
//! Everything the pipeline needs is a file under the Space, which is what makes
//! the whole thing auditable and scriptable. A user can edit
//! `Capture/capability.toml` in the Explorer's text editor, and the next
//! screening run picks it up. Nothing is hidden in a binary store.
//!
//! ```text
//! Capture/
//!   capability.toml           the firm: NAICS, certifications, capacity, past performance
//!   _raw/<connector>/page-*.json   raw API pages, exactly as they arrived
//!   <source>-<notice>/        one folder per assessed notice (see `audit`)
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::audit::CAPTURE_DIR;
use super::model::{CapabilityStatement, PastPerformance, SetAside};
use super::sources::{self, Profile};
use super::{CaptureError, Opportunity};

/// File name of the firm profile inside `Capture/`.
pub const CAPABILITY_FILE: &str = "capability.toml";

/// The firm profile as it appears on disk.
///
/// A separate shape from [`CapabilityStatement`] on purpose: certifications are
/// plain strings here so the file stays hand-editable, rather than TOML that
/// mirrors a Rust enum's serde encoding.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityFile {
    #[serde(default)]
    pub legal_name: String,
    #[serde(default)]
    pub uei: String,
    /// Six-digit NAICS codes, or ALN numbers for grant work.
    #[serde(default)]
    pub naics: Vec<String>,
    /// `8a`, `hubzone`, `sdvosb`, `vosb`, `wosb`, `edwosb`, `small`.
    #[serde(default)]
    pub certifications: Vec<String>,
    /// Two-letter states. Empty means no geographic limit.
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub ceiling_capacity: Option<i64>,
    #[serde(default)]
    pub floor: Option<i64>,
    /// Days needed to produce a compliant response.
    #[serde(default = "default_response_days")]
    pub min_response_days: i64,
    /// Topic to paragraph. These seed the draft, so they should read the way
    /// the firm would actually write them in a proposal.
    #[serde(default)]
    pub capabilities: BTreeMap<String, String>,
    #[serde(default)]
    pub past_performance: Vec<PastPerformanceFile>,
}

fn default_response_days() -> i64 {
    10
}

/// One past-performance reference on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PastPerformanceFile {
    #[serde(default)]
    pub customer: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub naics: String,
    #[serde(default)]
    pub value: Option<i64>,
    #[serde(default)]
    pub year: Option<i32>,
    /// `true`, `false`, or omitted. Only recorded outcomes can calibrate the
    /// scoring model, so leaving this off is honest and leaving it wrong is not.
    #[serde(default)]
    pub won: Option<bool>,
    #[serde(default)]
    pub summary: String,
}

/// Parse a certification token into the set-aside it grants eligibility for.
fn parse_certification(s: &str) -> SetAside {
    match s.trim().to_ascii_lowercase().replace(['(', ')', '-', ' '], "").as_str() {
        "8a" => SetAside::EightA,
        "hubzone" | "hz" => SetAside::HubZone,
        "sdvosb" | "servicedisabledveteran" => SetAside::ServiceDisabledVeteran,
        "vosb" | "veteran" => SetAside::Veteran,
        "wosb" | "womanowned" => SetAside::WomanOwned,
        "edwosb" => SetAside::EconomicallyDisadvantagedWomanOwned,
        "small" | "smallbusiness" | "sb" => SetAside::TotalSmallBusiness,
        other => SetAside::Other(other.to_ascii_uppercase()),
    }
}

impl CapabilityFile {
    /// Convert to the form screening and scoring consume.
    pub fn to_statement(&self) -> CapabilityStatement {
        CapabilityStatement {
            legal_name: self.legal_name.clone(),
            uei: self.uei.clone(),
            naics: self.naics.clone(),
            certifications: self.certifications.iter().map(|c| parse_certification(c)).collect(),
            states: self.states.clone(),
            ceiling_capacity: self.ceiling_capacity,
            floor: self.floor,
            min_response_days: self.min_response_days.max(1),
            capabilities: self.capabilities.clone(),
            past_performance: self
                .past_performance
                .iter()
                .map(|p| PastPerformance {
                    customer: p.customer.clone(),
                    title: p.title.clone(),
                    naics: p.naics.clone(),
                    value: p.value,
                    year: p.year,
                    won: p.won,
                    summary: p.summary.clone(),
                })
                .collect(),
        }
    }

    /// Everything that has to be filled in before screening means anything.
    ///
    /// A profile with no NAICS and no certifications screens nothing out, which
    /// looks like the tool working and is in fact the tool being useless.
    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.naics.is_empty() {
            v.push("naics");
        }
        if self.certifications.is_empty() {
            v.push("certifications");
        }
        if self.capabilities.is_empty() {
            v.push("capabilities");
        }
        if self.past_performance.is_empty() {
            v.push("past_performance");
        }
        v
    }
}

/// Path of the firm profile for a Space.
pub fn capability_path(space_root: &Path) -> PathBuf {
    space_root.join(CAPTURE_DIR).join(CAPABILITY_FILE)
}

/// Load the firm profile, or report that it is absent.
pub fn load_capability(space_root: &Path) -> Result<CapabilityFile, CaptureError> {
    let path = capability_path(space_root);
    let text = std::fs::read_to_string(&path)
        .map_err(|_| CaptureError::Io(format!("{} not found; create it first", path.display())))?;
    toml::from_str(&text).map_err(|e| CaptureError::Parse(format!("{}: {e}", path.display())))
}

/// Write a commented starter profile, and never overwrite an existing one.
///
/// Returns the path and whether it was created. Silently replacing a profile a
/// firm spent an afternoon writing would be the worst bug in this module.
pub fn ensure_capability(space_root: &Path) -> Result<(PathBuf, bool), CaptureError> {
    let path = capability_path(space_root);
    if path.exists() {
        return Ok((path, false));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CaptureError::Io(format!("{}: {e}", parent.display())))?;
    }
    std::fs::write(&path, STARTER_CAPABILITY)
        .map_err(|e| CaptureError::Io(format!("{}: {e}", path.display())))?;
    Ok((path, true))
}

/// The starter profile. Every field is present and commented, because an empty
/// file teaches nothing and a firm should be able to fill this in without
/// reading the source.
const STARTER_CAPABILITY: &str = r#"# Capability Statement — the firm that answers solicitations.
#
# Screening and scoring run against this file and nothing else. Edit it here or
# in the Explorer; the next screening run picks up the change.
#
# A profile with no `naics` and no `certifications` screens nothing out, which
# looks like the tool working while it does nothing at all.

legal_name = ""
# Unique Entity ID from your SAM.gov registration.
uei = ""

# Six-digit NAICS codes you are registered under. A five-digit industry prefix
# also matches, so 541511 sees 541512.
naics = []

# What you hold: "8a", "hubzone", "sdvosb", "vosb", "wosb", "edwosb", "small".
# An unrecognised value is treated as a certification you do NOT hold, so a
# typo costs you a missed bid rather than a wasted proposal.
certifications = []

# Two-letter states you will perform in. Leave empty for no geographic limit.
states = []

# Largest contract you can staff, in whole dollars. Omit for no cap.
# ceiling_capacity = 2000000

# Smallest contract worth the bid cost. Below this you lose money by winning.
# floor = 50000

# Days you need to produce a compliant response. Anything closing sooner is
# screened out rather than ranked.
min_response_days = 10

# Topic to paragraph. These seed draft sections and decide compliance coverage,
# so write them the way you would write them in a proposal.
[capabilities]
# help_desk = "Tier 1 and Tier 2 help desk support staffed during core hours, with ticket triage and escalation."
# past_performance = "Three relevant contracts of similar scope and size, references available on request."

# Past performance. `won` is what makes the calibration harness work: without
# recorded outcomes it refuses to report a hit rate rather than inventing one.
# [[past_performance]]
# customer = "US Army"
# title = "Enterprise Help Desk"
# naics = "541511"
# value = 600000
# year = 2025
# won = true
# summary = "24x7 Tier 2 support for 4,000 seats."
"#;

/// Directory holding the raw pages a Connector fetched.
pub fn raw_dir(space_root: &Path) -> PathBuf {
    space_root.join(CAPTURE_DIR).join("_raw")
}

/// Re-read every fetched page and parse it back into notices.
///
/// Re-parsing from the raw pages rather than caching a parsed list keeps one
/// source of truth: whatever the folder holds is what the pipeline sees, and an
/// auditor reading the same files reaches the same notices.
///
/// Duplicates across pages and across Connectors collapse on notice id, newest
/// page winning, because a re-sync overlaps the previous window by design.
pub fn load_notices(space_root: &Path) -> Result<Vec<Opportunity>, CaptureError> {
    let dir = raw_dir(space_root);
    let Ok(connectors) = std::fs::read_dir(&dir) else {
        return Err(CaptureError::Io(format!(
            "{} holds no fetched pages yet; add a source and sync first",
            dir.display()
        )));
    };

    let mut by_id: BTreeMap<String, Opportunity> = BTreeMap::new();
    let mut pages_read = 0usize;

    let mut connector_dirs: Vec<PathBuf> =
        connectors.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    connector_dirs.sort();

    for cdir in connector_dirs {
        let Ok(entries) = std::fs::read_dir(&cdir) else { continue };
        let mut pages: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        pages.sort();

        for page in pages {
            let Ok(body) = std::fs::read_to_string(&page) else { continue };
            // Which parser applies is decided by the payload's own shape, not
            // by the folder name, so a renamed Connector still reads back.
            let parsed = if body.contains("opportunitiesData") {
                sources::parse_sam(&body)
            } else {
                sources::parse_grants(&body)
            };
            match parsed {
                Ok(p) => {
                    pages_read += 1;
                    for o in p.opportunities {
                        by_id.insert(format!("{}:{}", o.source.as_str(), o.notice_id), o);
                    }
                }
                Err(e) => {
                    // One malformed page must not cost the whole batch.
                    warn_page(&page, &e);
                }
            }
        }
    }

    if pages_read == 0 {
        return Err(CaptureError::Io(format!(
            "{} holds no readable pages; add a source and sync first",
            dir.display()
        )));
    }
    Ok(by_id.into_values().collect())
}

fn warn_page(path: &Path, e: &CaptureError) {
    bevy::log::warn!("capture: skipping unreadable page {}: {e}", path.display());
}

/// Requirement text per notice, from whatever the fetch actually captured.
///
/// SAM's search payload carries a LINK to the description rather than the text,
/// so a notice with no fetched attachment has no requirements to extract. That
/// reads as absent here, never as an empty requirement set: a compliance matrix
/// with no rows would otherwise claim full coverage of nothing.
pub fn load_documents(
    space_root: &Path,
    notices: &[Opportunity],
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for o in notices {
        // A fetched attachment, if one was placed alongside the record.
        let attachment = space_root
            .join(CAPTURE_DIR)
            .join(o.slug())
            .join("requirements.txt");
        if let Ok(text) = std::fs::read_to_string(&attachment) {
            if !text.trim().is_empty() {
                out.insert(o.notice_id.clone(), text);
                continue;
            }
        }
        if !o.description.trim().is_empty() {
            out.insert(o.notice_id.clone(), o.description.clone());
        }
    }
    out
}

/// Write a Connector `_instance.toml` for one capture profile.
///
/// Mirrors the Data menu's `write_connector`: inline TOML, picked up by the
/// file watcher, configured in Properties. `enabled = false` so creating a
/// source can never start unattended network traffic.
pub fn write_capture_connector(
    space_root: &Path,
    profile: Profile,
) -> Result<PathBuf, CaptureError> {
    let dir = space_root.join("DataService");
    std::fs::create_dir_all(&dir)
        .map_err(|e| CaptureError::Io(format!("{}: {e}", dir.display())))?;

    let base = match profile {
        Profile::SamOpportunities => "SAM.gov Opportunities",
        Profile::GrantsSearch => "Grants.gov Announcements",
    };
    let name = crate::space::instance_loader::unique_entity_name(&dir, base);
    let cdir = dir.join(&name);
    std::fs::create_dir_all(&cdir)
        .map_err(|e| CaptureError::Io(format!("{}: {e}", cdir.display())))?;

    let body = match profile {
        Profile::SamOpportunities => format!(
            "# SAM.gov contract opportunities.\n\
             #\n\
             # Set `posted_from` / `posted_to` (MM/dd/yyyy, at most a year apart),\n\
             # click Capture > Sources > API Key to paste your free SAM.gov key,\n\
             # then set `enabled = true`. `secret_ref` NAMES the credential and\n\
             # never holds it: the value is saved outside every Space.\n\n\
             [attributes]\n\
             source_type = \"REST\"\n\
             profile = \"{}\"\n\
             endpoint = \"{}\"\n\
             secret_ref = \"SAM_API_KEY\"\n\
             poll_seconds = 3600\n\
             format = \"json\"\n\
             enabled = false\n\
             posted_from = \"\"\n\
             posted_to = \"\"\n\
             naics = \"\"\n\
             set_aside = \"\"\n\
             notice_type = \"\"\n\
             state = \"\"\n\
             limit = \"100\"\n\n\
             [metadata]\n\
             class_name = \"Connector\"\n\
             archivable = true\n",
            profile.as_str(),
            profile.default_endpoint()
        ),
        Profile::GrantsSearch => format!(
            "# Grants.gov funding announcements.\n\
             #\n\
             # No API key required. Narrow with `aln` (Assistance Listing number),\n\
             # `keyword`, or `agencies`, then set `enabled = true`.\n\n\
             [attributes]\n\
             source_type = \"REST\"\n\
             profile = \"{}\"\n\
             endpoint = \"{}\"\n\
             poll_seconds = 3600\n\
             format = \"json\"\n\
             enabled = false\n\
             keyword = \"\"\n\
             aln = \"\"\n\
             agencies = \"\"\n\
             statuses = \"posted\"\n\
             eligibilities = \"\"\n\
             rows = \"100\"\n\n\
             [metadata]\n\
             class_name = \"Connector\"\n\
             archivable = true\n",
            profile.as_str(),
            profile.default_endpoint()
        ),
    };

    std::fs::write(cdir.join("_instance.toml"), body)
        .map_err(|e| CaptureError::Io(format!("{}: {e}", cdir.display())))?;
    Ok(cdir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eustress-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_starter_profile_parses_and_names_what_is_missing() {
        let root = temp("starter");
        let (path, created) = ensure_capability(&root).expect("writes");
        assert!(created);
        assert!(path.exists());

        let f = load_capability(&root).expect("the starter file must parse");
        let missing = f.missing_fields();
        assert!(missing.contains(&"naics"), "{missing:?}");
        assert!(missing.contains(&"certifications"), "{missing:?}");
        assert_eq!(f.min_response_days, 10, "the default has to survive the round trip");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_existing_profile_is_never_overwritten() {
        let root = temp("no-clobber");
        ensure_capability(&root).unwrap();
        let path = capability_path(&root);
        std::fs::write(&path, "legal_name = \"Real Firm\"\nnaics = [\"541511\"]\n").unwrap();

        let (_, created) = ensure_capability(&root).expect("second call");
        assert!(!created, "a second call must not create");
        assert_eq!(load_capability(&root).unwrap().legal_name, "Real Firm");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn certifications_parse_from_the_forms_people_actually_type() {
        for (input, expected) in [
            ("8(a)", SetAside::EightA),
            ("8a", SetAside::EightA),
            ("HUBZone", SetAside::HubZone),
            ("SDVOSB", SetAside::ServiceDisabledVeteran),
            ("small business", SetAside::TotalSmallBusiness),
        ] {
            assert_eq!(parse_certification(input), expected, "input {input}");
        }
    }

    #[test]
    fn an_unrecognised_certification_is_not_silently_granted() {
        let f = CapabilityFile {
            certifications: vec!["definitely-not-a-real-cert".into()],
            ..Default::default()
        };
        let s = f.to_statement();
        assert!(!s.eligible_for(&SetAside::EightA), "a typo must not unlock a set-aside");
        assert!(s.eligible_for(&SetAside::None), "open competition stays open");
    }

    #[test]
    fn notices_reload_from_raw_pages_and_dedupe_across_them() {
        let root = temp("reload");
        let dir = raw_dir(&root).join("SAM Source");
        std::fs::create_dir_all(&dir).unwrap();
        // The same notice appears on both pages, as it does when a re-sync
        // overlaps the previous window.
        std::fs::write(
            dir.join("page-001.json"),
            r#"{"totalRecords":2,"opportunitiesData":[{"noticeId":"a","title":"Old"},{"noticeId":"b","title":"B"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("page-002.json"),
            r#"{"totalRecords":2,"opportunitiesData":[{"noticeId":"a","title":"New"}]}"#,
        )
        .unwrap();

        let notices = load_notices(&root).expect("loads");
        assert_eq!(notices.len(), 2, "the duplicate collapses");
        let a = notices.iter().find(|o| o.notice_id == "a").unwrap();
        assert_eq!(a.title, "New", "the later page wins");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn both_payload_shapes_are_recognised_by_content_not_by_folder_name() {
        let root = temp("shapes");
        let d = raw_dir(&root).join("Renamed Whatever");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("page-001.json"),
            r#"{"errorcode":0,"data":{"hitCount":1,"oppHits":[{"id":"7","title":"Grant"}]}}"#,
        )
        .unwrap();
        let notices = load_notices(&root).expect("loads");
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].source, super::super::NoticeSource::Grants);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn one_bad_page_does_not_cost_the_batch() {
        let root = temp("bad-page");
        let d = raw_dir(&root).join("SAM Source");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("page-001.json"), "{ not json at all").unwrap();
        std::fs::write(
            d.join("page-002.json"),
            r#"{"totalRecords":1,"opportunitiesData":[{"noticeId":"ok","title":"Fine"}]}"#,
        )
        .unwrap();
        let notices = load_notices(&root).expect("loads despite the bad page");
        assert_eq!(notices.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn loading_with_nothing_fetched_says_what_to_do() {
        let root = temp("empty");
        match load_notices(&root) {
            Err(CaptureError::Io(m)) => assert!(m.contains("sync first"), "{m}"),
            other => panic!("expected a helpful io error, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_notice_with_no_text_contributes_no_document() {
        let root = temp("docs");
        let mut with_text = Opportunity::empty(super::super::NoticeSource::Grants, "has");
        with_text.description = "The Recipient shall report quarterly.".into();
        let without = Opportunity::empty(super::super::NoticeSource::Sam, "none");

        let docs = load_documents(&root, &[with_text, without]);
        assert_eq!(docs.len(), 1, "absent text must not become an empty document");
        assert!(docs.contains_key("has"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_written_connector_starts_disabled_and_holds_no_secret() {
        let root = temp("connector");
        let dir = write_capture_connector(&root, Profile::SamOpportunities).expect("writes");
        let text = std::fs::read_to_string(dir.join("_instance.toml")).unwrap();

        let parsed: toml::Value = toml::from_str(&text).expect("the written TOML must parse");
        assert_eq!(parsed["attributes"]["enabled"].as_bool(), Some(false));
        assert_eq!(parsed["attributes"]["profile"].as_str(), Some("sam_opportunities"));
        assert_eq!(parsed["attributes"]["secret_ref"].as_str(), Some("SAM_API_KEY"));
        assert_eq!(parsed["metadata"]["class_name"].as_str(), Some("Connector"));
        // The manifest names the variable and never holds a value.
        assert!(!text.to_ascii_lowercase().contains("api_key ="), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_connector_of_the_same_kind_gets_its_own_folder() {
        let root = temp("connector-unique");
        let a = write_capture_connector(&root, Profile::GrantsSearch).unwrap();
        let b = write_capture_connector(&root, Profile::GrantsSearch).unwrap();
        assert_ne!(a, b, "two sources must not collide on one folder");
        let _ = std::fs::remove_dir_all(&root);
    }
}
