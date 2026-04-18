//! # Workshop @-mention — Bevy ↔ Slint glue
//!
//! Handles the three Slint callbacks that drive the autocomplete flow:
//! * `on-mention-query-changed(text)` — user typed a keystroke; parse the
//!   active `@token` from the tail of the input and push ranked results
//!   back to the popup model.
//! * `on-mention-commit(index)` — user pressed Enter / Tab / clicked a row
//!   in the popup; splice the chosen entry's canonical path into the input,
//!   record the MRU hit, close the popup.
//! * `on-mention-cancel()` — Escape / focus loss; close the popup without
//!   mutating the input.
//!
//! The query parser walks backwards from the tail of the input text. If
//! the nearest non-alphanumeric character is `@`, everything after it is
//! the active prefix. Whitespace or end-of-string closes any active mention.

use super::mention::{MentionEntry, MentionIndex};

/// Locate the active `@token` at the tail of `text`. Returns
/// `(byte_offset_of_at, prefix_chars_after_at)` when one is present.
///
/// Examples:
/// * `"hello @vc"` → `Some((6, "vc"))`
/// * `"hello @vc world"` → `None` (a space broke the mention)
/// * `"@foo"` → `Some((0, "foo"))`
/// * `""` → `None`
pub fn find_active_mention(text: &str) -> Option<(usize, String)> {
    for (idx, c) in text.char_indices().rev() {
        if c == '@' {
            let after = &text[idx + c.len_utf8()..];
            return Some((idx, after.to_string()));
        }
        if c.is_whitespace() {
            return None;
        }
    }
    None
}

/// Produce the input text that results from committing `canonical` at the
/// `@token` currently at byte offset `at` in `text`. The token (the `@`
/// and everything after it up to end-of-string) is replaced with
/// `@canonical ` (trailing space so the caret is ready for the next word).
pub fn splice_mention(text: &str, at: usize, canonical: &str) -> String {
    let before = &text[..at];
    format!("{}@{} ", before, canonical)
}

/// Top-k search on `MentionIndex` returning lightweight view data suitable
/// for pushing to Slint. Clipped to `TOP_K` items.
pub fn collect_popup_items(
    index: &MentionIndex,
    query: &str,
    top_k: usize,
) -> Vec<MentionItemView> {
    index.search(query, top_k).into_iter()
        .map(MentionItemView::from)
        .collect()
}

/// Plain copy of the fields Slint's `MentionItemData` needs. Keeping this
/// as a Rust-side struct (rather than constructing Slint's type directly
/// here) keeps `mention.rs` free of Slint-generated types — the callers
/// that push to the UI translate at the boundary.
pub struct MentionItemView {
    pub id: i64,
    pub name: String,
    pub qualifier: String,
    pub canonical: String,
    pub icon_hint: String,
    pub kind: &'static str,
}

impl<'a> From<&'a MentionEntry> for MentionItemView {
    fn from(e: &'a MentionEntry) -> Self {
        use super::mention::MentionKind;
        let kind = match e.kind {
            MentionKind::Entity => "entity",
            MentionKind::File => "file",
            MentionKind::Script => "script",
            MentionKind::Service => "service",
        };
        Self {
            // Slint int is 32-bit signed; we truncate the u64 hash. Collisions
            // only matter for the current popup session — the canonical path
            // remains the source of truth for commit.
            id: e.id.0 as i64,
            name: e.name.clone(),
            qualifier: e.qualifier.clone(),
            canonical: e.canonical_path.clone(),
            icon_hint: e.icon_hint.clone(),
            kind,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_trailing_mention() {
        assert_eq!(find_active_mention("hello @vc"), Some((6, "vc".to_string())));
    }

    #[test]
    fn breaks_on_whitespace() {
        assert_eq!(find_active_mention("hello @vc world"), None);
    }

    #[test]
    fn leading_at_sign() {
        assert_eq!(find_active_mention("@foo"), Some((0, "foo".to_string())));
    }

    #[test]
    fn empty_prefix_right_after_at() {
        assert_eq!(find_active_mention("text @"), Some((5, "".to_string())));
    }

    #[test]
    fn no_at_returns_none() {
        assert_eq!(find_active_mention("plain text"), None);
    }

    #[test]
    fn splice_replaces_tail() {
        let out = splice_mention("hey @vc", 4, "entity:Space1/V-Cell/VCell_Housing");
        assert_eq!(out, "hey @entity:Space1/V-Cell/VCell_Housing ");
    }

    #[test]
    fn splice_handles_leading_at() {
        let out = splice_mention("@foo", 0, "file:Space1/readme.md");
        assert_eq!(out, "@file:Space1/readme.md ");
    }
}
