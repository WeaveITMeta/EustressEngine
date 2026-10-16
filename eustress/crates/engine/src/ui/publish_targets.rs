//! The Publish dialog's "Publish to" picker (Publish As...).
//!
//! The rows are a new listing, then the author's own listings on the API in
//! use (`multiplayer::MyListings`, from GET /api/projects), newest first,
//! this Universe's own listing marked. They are fetched each time the dialog
//! opens. A plain Publish starts on this Universe's own listing (today's
//! behaviour); File > Publish As... starts on a new listing with the picker
//! taking the keyboard. A Space-only publish has no picker: it always goes to
//! this Universe's listing.
//!
//! The pick reaches the publish as a [`PublishTarget`] ([`target_for`]):
//! this Universe's own listing is `Remembered`, the new listing `New`, and
//! any other `Existing(id)`, which replaces that listing's published version
//! (the dialog confirms that first).

use bevy::prelude::*;

use eustress_common::project_manifest::PublishTarget;

use super::slint_ui::{PublishTargetRow, SlintUiState};
use crate::multiplayer::{ListingSummary, MyListings, MyListingsState, RefreshMyListings};

/// The pick as the publish takes it. A Space-only publish, and this
/// Universe's own listing, are `Remembered`; the new listing is `New`; any
/// other listing is `Existing`.
pub fn target_for(id: &str, own: &str, space_only: bool) -> PublishTarget {
    if space_only || id == own {
        PublishTarget::Remembered
    } else if id.is_empty() {
        PublishTarget::New
    } else {
        PublishTarget::Existing(id.to_string())
    }
}

/// The author-facing word for a listing's status.
pub fn status_word(status: &str) -> &str {
    match status {
        "published" => "Listed",
        "in_review" => "In review",
        "not_listed" => "Not listed",
        "changes_requested" => "Changes requested",
        "approved_private" => "Approved, private",
        "unreviewed" => "Not reviewed yet",
        other => other,
    }
}

/// "updated today", "updated yesterday", "updated 5 days ago", "updated 3
/// months ago", from an ISO 8601 time; empty when it does not parse.
pub fn updated_ago(iso: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(iso.trim()) else { return String::new() };
    let days = (now - at.with_timezone(&chrono::Utc)).num_days().max(0);
    match days {
        0 => "updated today".into(),
        1 => "updated yesterday".into(),
        2..=59 => format!("updated {days} days ago"),
        _ => format!("updated {} months ago", days / 30),
    }
}

/// The picker's rows: the new listing first, then the author's listings.
pub fn target_rows(items: &[ListingSummary], own: &str, now: chrono::DateTime<chrono::Utc>) -> Vec<PublishTargetRow> {
    let mut rows = vec![PublishTargetRow {
        id: "".into(),
        title: "New listing".into(),
        detail: "A new gallery page, separate from any you have".into(),
    }];
    for item in items {
        let mut detail = Vec::new();
        if !own.is_empty() && item.id == own {
            detail.push("This Universe's listing".to_string());
        }
        detail.push(status_word(&item.status).to_string());
        let ago = updated_ago(&item.updated, now);
        if !ago.is_empty() {
            detail.push(ago);
        }
        let title = if item.title.trim().is_empty() { "Untitled".to_string() } else { item.title.clone() };
        rows.push(PublishTargetRow {
            id: item.id.as_str().into(),
            title: super::display_text::shown_line(&title),
            detail: detail.join(" · ").into(),
        });
    }
    rows
}

/// What the picker says under its rows while the listings load, or when
/// they cannot.
pub fn targets_note(state: &MyListingsState) -> String {
    match state {
        MyListingsState::Idle | MyListingsState::Loading => "Loading your listings...".into(),
        MyListingsState::Error(why) => why.clone(),
        MyListingsState::Ready => String::new(),
    }
}

/// Fill the picker when the Publish dialog opens and as the listings arrive.
/// The dialog's opening asks for a fresh list. The starting pick follows how
/// it opened (`publish-focus-targets`: Publish As... starts on a new listing,
/// Publish on this Universe's own), until the author picks.
pub fn sync_publish_targets(
    slint_context: Option<NonSend<SlintUiState>>,
    listings: Option<Res<MyListings>>,
    mut refresh: MessageWriter<RefreshMyListings>,
    mut was_open: Local<bool>,
    // The pick this system last made, to tell it from the author's.
    mut last_set: Local<Option<String>>,
) {
    let Some(ctx) = slint_context.as_ref() else { return };
    let ui = &ctx.window;
    let open = ui.get_show_publish_dialog();
    let opened = open && !*was_open;
    *was_open = open;
    if !open {
        return;
    }
    if opened {
        refresh.write(RefreshMyListings);
        *last_set = None;
    }
    let changed = listings.as_ref().is_some_and(|l| l.is_changed());
    if !opened && !changed {
        return;
    }

    let (items, own, note) = match listings.as_deref() {
        Some(l) => (l.items.as_slice(), l.own.clone().unwrap_or_default(), targets_note(&l.state)),
        None => (&[][..], String::new(), String::new()),
    };
    let rows = target_rows(items, &own, chrono::Utc::now());
    ui.set_publish_own_listing_id(own.as_str().into());
    ui.set_publish_targets_note(super::display_text::shown_block(&note));

    // The starting pick, kept until the author picks another.
    let current = ui.get_publish_target_id().to_string();
    let untouched = last_set.as_deref().map_or(true, |s| s == current);
    let id = if untouched {
        if ui.get_publish_focus_targets() { String::new() } else { own.clone() }
    } else {
        current
    };
    let index = rows.iter().position(|r| r.id.as_str() == id).unwrap_or(0);
    let title = rows.get(index).map(|r| r.title.to_string()).unwrap_or_else(|| "New listing".into());
    ui.set_publish_targets(slint::ModelRc::new(slint::VecModel::from(rows)));
    ui.set_publish_target_id(id.as_str().into());
    ui.set_publish_target_title(title.into());
    ui.set_publish_target_index(index as i32);
    *last_set = Some(id);
}

// ============================================================================
// Tags
// ============================================================================

/// What the tag field asks of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum TagRequest {
    /// A tag as typed (with or without its #, and a trailing comma or space).
    Add(String),
    /// The field as it stands after an edit: a trailing comma or space ends
    /// a tag.
    Typed(String),
    /// Remove the tag at this index.
    Remove(usize),
}

static TAG_REQUESTS: std::sync::Mutex<Vec<TagRequest>> = std::sync::Mutex::new(Vec::new());

/// Queue a request from the tag field; the tags system takes it next frame.
pub fn request_tag(r: TagRequest) {
    TAG_REQUESTS.lock().unwrap_or_else(|e| e.into_inner()).push(r);
}

fn take_tag_requests() -> Vec<TagRequest> {
    std::mem::take(&mut *TAG_REQUESTS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The tags a target starts with, once the author's listings have loaded,
/// and whether the review chose them: none for a new listing, a listing's own
/// otherwise. `None` when the target is not in the list, so its tags are not
/// known (and are never sent).
pub fn prefill_tags(target_id: &str, items: &[ListingSummary]) -> Option<(Vec<String>, bool)> {
    if target_id.is_empty() {
        return Some((Vec::new(), false));
    }
    items.iter().find(|i| i.id == target_id).map(|i| (i.tags.clone(), i.tags_suggested))
}

/// The tag field's state while the Publish dialog is open.
#[derive(Default)]
pub struct TagsField {
    tags: Vec<String>,
    // Loaded from the listing, or edited: only a known list is sent.
    known: bool,
    // The review chose the loaded tags; any edit makes them the author's.
    suggested: bool,
    edited: bool,
    filled_for: Option<String>,
    error: String,
    was_open: bool,
}

fn push_tags(ui: &super::slint_ui::StudioWindow, field: &TagsField) {
    let model = |tags: &[String]| {
        let row: Vec<slint::SharedString> = tags.iter().map(|t| t.as_str().into()).collect();
        slint::ModelRc::new(slint::VecModel::from(row))
    };
    let (first, rest) = field.tags.split_at(field.tags.len().min(4));
    ui.set_publish_tags(model(&field.tags));
    ui.set_publish_tags_known(field.known);
    ui.set_publish_tags_suggested(field.suggested && !field.tags.is_empty());
    ui.set_publish_tags_row_0(model(first));
    ui.set_publish_tags_row_1(model(rest));
    ui.set_publish_tags_count(field.tags.len() as i32);
    ui.set_publish_tags_error(field.error.as_str().into());
}

/// The Publish dialog's tags: pre-filled from the chosen listing (until the
/// author edits them) and kept to the Worker's rule as the author adds and
/// removes them (`eustress_common::listing_tags`).
pub fn sync_publish_tags(
    slint_context: Option<NonSend<SlintUiState>>,
    listings: Option<Res<MyListings>>,
    mut field: Local<TagsField>,
) {
    let Some(ctx) = slint_context.as_ref() else { return };
    let ui = &ctx.window;
    let open = ui.get_show_publish_dialog();
    let opened = open && !field.was_open;
    field.was_open = open;
    if !open {
        let _ = take_tag_requests();
        return;
    }
    let mut dirty = false;
    if opened {
        *field = TagsField { was_open: true, ..Default::default() };
        ui.set_publish_tags_text("".into());
        dirty = true;
    }

    if !field.edited {
        let target = ui.get_publish_target_id().to_string();
        let changed = listings.as_ref().is_some_and(|l| l.is_changed());
        if opened || changed || field.filled_for.as_deref() != Some(target.as_str()) {
            let loaded = listings
                .as_deref()
                .filter(|l| matches!(l.state, MyListingsState::Ready))
                .and_then(|l| prefill_tags(&target, &l.items));
            field.known = loaded.is_some();
            let (tags, suggested) = loaded.unwrap_or_default();
            field.tags = tags;
            field.suggested = suggested;
            field.filled_for = Some(target);
            dirty = true;
        }
    }

    for request in take_tag_requests() {
        // An edit ends a tag at a comma or a space; any other edit is typing.
        let request = match request {
            TagRequest::Typed(text) if text.ends_with([',', ' ']) => {
                if text.trim_matches([',', ' ']).is_empty() {
                    ui.set_publish_tags_text("".into());
                    continue;
                }
                TagRequest::Add(text)
            }
            TagRequest::Typed(_) => continue,
            other => other,
        };
        field.edited = true;
        field.known = true;
        field.suggested = false;
        dirty = true;
        match request {
            TagRequest::Add(raw) => {
                let raw = raw.trim_end_matches([',', ' ']);
                match eustress_common::listing_tags::add_tag(&mut field.tags, raw) {
                    Ok(()) => {
                        field.error.clear();
                        ui.set_publish_tags_text("".into());
                    }
                    Err(why) => field.error = why,
                }
            }
            TagRequest::Remove(i) => {
                if i < field.tags.len() {
                    field.tags.remove(i);
                }
                field.error.clear();
            }
            // Turned into an Add above, or skipped.
            TagRequest::Typed(_) => {}
        }
    }
    if dirty {
        push_tags(ui, &field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(id: &str, title: &str, status: &str, updated: &str) -> ListingSummary {
        ListingSummary {
            id: id.into(),
            title: title.into(),
            status: status.into(),
            updated: updated.into(),
            thumbnail_url: None,
            tags: vec!["tycoon".into()],
            tags_suggested: false,
            cover_url: None,
        }
    }

    #[test]
    fn tags_start_from_the_chosen_listing() {
        let mut judged = listing("judged-2", "Pong", "published", "");
        judged.tags_suggested = true;
        let items = [listing("own-1", "Box Head", "published", ""), judged];
        assert_eq!(prefill_tags("", &items), Some((Vec::new(), false)), "a new listing starts with none");
        assert_eq!(prefill_tags("own-1", &items), Some((vec!["tycoon".to_string()], false)));
        assert_eq!(prefill_tags("judged-2", &items), Some((vec!["tycoon".to_string()], true)), "the review's tags come as suggestions");
        assert_eq!(prefill_tags("gone-9", &items), None, "unknown tags are never sent");
    }

    #[test]
    fn a_pick_becomes_its_target() {
        assert_eq!(target_for("own-1", "own-1", false), PublishTarget::Remembered);
        assert_eq!(target_for("", "own-1", false), PublishTarget::New);
        assert_eq!(target_for("other-2", "own-1", false), PublishTarget::Existing("other-2".into()));
        assert_eq!(target_for("", "", false), PublishTarget::Remembered, "a first publish is this Universe's own");
        assert_eq!(target_for("other-2", "own-1", true), PublishTarget::Remembered, "a Space-only publish has no pick");
    }

    #[test]
    fn rows_lead_with_a_new_listing_and_mark_this_universes() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let items = [
            listing("own-1", "Box Head", "in_review", "2026-09-29T08:00:00Z"),
            listing("other-2", "", "published", "2026-10-01T09:00:00Z"),
        ];
        let rows = target_rows(&items, "own-1", now);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id.as_str(), "");
        assert_eq!(rows[0].title.as_str(), "New listing");
        assert_eq!(rows[1].detail.as_str(), "This Universe's listing · In review · updated 2 days ago");
        assert_eq!(rows[2].title.as_str(), "Untitled");
        assert_eq!(rows[2].detail.as_str(), "Listed · updated today");
    }

    #[test]
    fn ages_read_as_people_say_them() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z").unwrap().with_timezone(&chrono::Utc);
        assert_eq!(updated_ago("2026-09-30T12:00:00Z", now), "updated yesterday");
        assert_eq!(updated_ago("2026-06-01T12:00:00Z", now), "updated 4 months ago");
        assert_eq!(updated_ago("not a time", now), "");
        assert_eq!(status_word("changes_requested"), "Changes requested");
        assert_eq!(status_word("something_new"), "something_new");
    }

    #[test]
    fn the_note_says_what_the_list_is_doing() {
        assert_eq!(targets_note(&MyListingsState::Loading), "Loading your listings...");
        assert_eq!(targets_note(&MyListingsState::Error("Sign in to see your listings.".into())), "Sign in to see your listings.");
        assert_eq!(targets_note(&MyListingsState::Ready), "");
    }
}
