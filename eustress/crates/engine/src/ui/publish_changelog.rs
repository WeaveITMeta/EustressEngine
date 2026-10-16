//! The Publish dialog's change logs (`docs/architecture/PUBLISH_CHANGELOG.md`).
//!
//! After a publish that changed something, the dialog offers the change log
//! Studio just built as a Markdown file. It is the text from this session, so
//! it can be saved even when storing it on the server failed. Below, the
//! listing's earlier logs, newest first, each downloadable the same way. The
//! logs are the author's record; nothing here is public.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::prelude::*;

use super::display_text::shown_line;
use super::slint_ui::{ChangeLogRow, OutputConsole, SlintUiState};
use crate::multiplayer::{ChangeLogSummary, DownloadChangeLog, MyChangeLogs, MyListingsState, RefreshMyChangeLogs};

/// What the dialog asked for, kept until the sync below can act on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeLogRequest {
    /// Save the change log of the publish that just finished.
    SaveLatest,
    /// Download an earlier log of the target listing (its id).
    Download(String),
}

static REQUESTS: Mutex<Vec<ChangeLogRequest>> = Mutex::new(Vec::new());

pub fn request(r: ChangeLogRequest) {
    REQUESTS.lock().unwrap_or_else(|e| e.into_inner()).push(r);
}

fn take_requests() -> Vec<ChangeLogRequest> {
    std::mem::take(&mut *REQUESTS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The file name a log saves as by default.
pub fn default_file_name(version: Option<u32>) -> String {
    match version {
        Some(v) => format!("CHANGELOG-v{v}.md"),
        None => "CHANGELOG.md".to_string(),
    }
}

/// A size as people read it: "3 KB", "1.2 MB".
pub fn size_text(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes < MB {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}

/// An ISO 8601 time in the author's local time, 12-hour: "Oct 2, 2026, 6:26 PM".
/// Unreadable text is shown as it came.
pub fn local_time(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(t) => t.with_timezone(&chrono::Local).format("%b %-d, %Y, %-I:%M %p").to_string(),
        Err(_) => iso.to_string(),
    }
}

/// One earlier log as the list shows it.
pub fn row(item: &ChangeLogSummary, outcome: Option<&Result<PathBuf, String>>) -> ChangeLogRow {
    let (note, failed) = match outcome {
        Some(Ok(path)) => (format!("Saved to {}", path.display()), false),
        Some(Err(why)) => (why.clone(), true),
        None => (String::new(), false),
    };
    ChangeLogRow {
        id: item.id.as_str().into(),
        title: format!("Version {}", item.version).into(),
        detail: shown_line(&format!("{} · {}", local_time(&item.created_at), size_text(item.bytes))),
        note: shown_line(&note),
        failed,
    }
}

/// Write `text` to `path` through a temporary file beside it, so a failed
/// write never leaves half a file.
pub fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn pick_save_path(version: Option<u32>) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save change log")
        .set_file_name(default_file_name(version))
        .add_filter("Markdown", &["md"])
        .save_file()
}

/// What the dialog last showed, so it is set only when it changes.
#[derive(Default)]
pub struct ChangeLogShown {
    was_open: bool,
    asked_for: Option<String>,
    latest: Option<(Option<u32>, bool)>,
    rows_key: Option<Vec<(String, Option<Result<PathBuf, String>>)>>,
    note: String,
}

/// Show the latest change log and the target listing's earlier ones; save
/// what the dialog asked for.
pub fn sync_publish_changelogs(
    slint_context: Option<NonSend<SlintUiState>>,
    progress: Option<Res<super::file_event_handler::PublishProgressHandle>>,
    logs: Option<Res<MyChangeLogs>>,
    mut refresh: MessageWriter<RefreshMyChangeLogs>,
    mut download: MessageWriter<DownloadChangeLog>,
    mut output: Option<ResMut<OutputConsole>>,
    mut shown: Local<ChangeLogShown>,
) {
    let Some(ctx) = slint_context.as_ref() else { return };
    let ui = &ctx.window;
    let open = ui.get_show_publish_dialog();
    let opened = open && !shown.was_open;
    shown.was_open = open;
    if !open {
        let _ = take_requests();
        return;
    }

    // The publish that just finished: its log, version and whether it was stored.
    let latest = progress.as_ref().and_then(|h| {
        let p = h.0.try_lock().ok()?;
        (p.complete && p.changelog.is_some()).then(|| (p.changelog_version, p.changelog_stored))
    });
    if latest != shown.latest {
        ui.set_publish_changelog_ready(latest.is_some());
        ui.set_publish_changelog_version(latest.and_then(|(v, _)| v).map_or(0, |v| v as i32));
        ui.set_publish_changelog_stored(latest.is_none_or(|(_, stored)| stored));
        shown.latest = latest;
    }

    // The earlier logs of the listing this publish goes to; a new listing has none.
    let target = ui.get_publish_target_id().to_string();
    if opened || shown.asked_for.as_deref() != Some(target.as_str()) {
        shown.asked_for = Some(target.clone());
        if !target.is_empty() {
            refresh.write(RefreshMyChangeLogs { listing_id: target.clone() });
        }
    }
    let for_target = logs.as_deref().filter(|l| !target.is_empty() && l.listing.as_deref() == Some(target.as_str()));
    let key: Option<Vec<(String, Option<Result<PathBuf, String>>)>> = for_target.map(|l| {
        l.items.iter().map(|i| (i.id.clone(), l.downloads.get(&i.id).cloned())).collect()
    });
    if key != shown.rows_key {
        let rows: Vec<ChangeLogRow> = for_target
            .map(|l| l.items.iter().map(|i| row(i, l.downloads.get(&i.id))).collect())
            .unwrap_or_default();
        ui.set_publish_changelogs(slint::ModelRc::new(slint::VecModel::from(rows)));
        shown.rows_key = key;
    }
    let note = match for_target.map(|l| &l.state) {
        _ if target.is_empty() => String::new(),
        None | Some(MyListingsState::Idle) | Some(MyListingsState::Loading) => "Loading earlier change logs...".to_string(),
        Some(MyListingsState::Error(why)) => why.clone(),
        Some(MyListingsState::Ready) if for_target.is_some_and(|l| l.items.is_empty()) => {
            "No earlier change logs for this listing.".to_string()
        }
        Some(MyListingsState::Ready) => String::new(),
    };
    if note != shown.note {
        ui.set_publish_changelogs_note(shown_line(&note));
        shown.note = note;
    }

    for r in take_requests() {
        match r {
            ChangeLogRequest::SaveLatest => {
                let latest = progress.as_ref().and_then(|h| {
                    let p = h.0.try_lock().ok()?;
                    p.changelog.clone().map(|text| (text, p.changelog_version))
                });
                let Some((text, version)) = latest else { continue };
                let Some(path) = pick_save_path(version) else { continue };
                let said = match write_file(&path, &text) {
                    Ok(()) => format!("Change log saved to {}", path.display()),
                    Err(e) => format!("The change log could not be saved to {}: {e}", path.display()),
                };
                if let Some(out) = output.as_mut() {
                    out.info(said);
                }
            }
            ChangeLogRequest::Download(log_id) => {
                if target.is_empty() {
                    continue;
                }
                let version = for_target.and_then(|l| l.items.iter().find(|i| i.id == log_id)).map(|i| i.version);
                let Some(save_to) = pick_save_path(version) else { continue };
                download.write(DownloadChangeLog { listing_id: target.clone(), log_id, save_to });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_sizes_read_as_people_expect() {
        assert_eq!(default_file_name(Some(7)), "CHANGELOG-v7.md");
        assert_eq!(default_file_name(None), "CHANGELOG.md");
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(3 * 1024 + 1), "4 KB");
        assert_eq!(size_text(1_258_291), "1.2 MB");
    }

    #[test]
    fn a_time_reads_in_twelve_hours_and_a_bad_one_as_it_came() {
        let shown = local_time("2026-10-02T01:26:00Z");
        assert!(shown.ends_with(" AM") || shown.ends_with(" PM"), "{shown}");
        assert!(shown.contains("2026"), "{shown}");
        assert_eq!(local_time("yesterday"), "yesterday");
    }

    #[test]
    fn a_row_says_where_it_saved_or_why_not() {
        let item = ChangeLogSummary {
            id: "l1".into(),
            version: 7,
            publish_hash: "h".into(),
            created_at: "2026-10-02T01:26:00Z".into(),
            bytes: 3000,
        };
        let r = row(&item, None);
        assert_eq!((r.title.as_str(), r.note.as_str(), r.failed), ("Version 7", "", false));
        assert!(r.detail.ends_with("· 3 KB"), "{}", r.detail);
        let saved = row(&item, Some(&Ok(PathBuf::from("C:/logs/CHANGELOG-v7.md"))));
        assert!(saved.note.starts_with("Saved to ") && !saved.failed);
        let failed = row(&item, Some(&Err("That log is gone.".into())));
        assert_eq!((failed.note.as_str(), failed.failed), ("That log is gone.", true));
    }

    #[test]
    fn a_write_leaves_the_whole_file_and_no_temporary() {
        let dir = std::env::temp_dir().join(format!("eustress_changelog_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("CHANGELOG-v7.md");
        write_file(&path, "# Version 7\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Version 7\n");
        assert!(!path.with_extension("md.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
