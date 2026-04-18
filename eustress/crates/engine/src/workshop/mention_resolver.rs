//! # Workshop mention resolver
//!
//! Parses `@kind:space/path` tokens out of a user's chat message and
//! produces Anthropic-API-shaped content blocks (extra text blocks for
//! file/entity excerpts, `image` blocks for pictures) that travel with
//! the user message to Claude.
//!
//! ## Invocation order
//!
//! The resolver runs inside `dispatch_chat_request` *before* the request
//! is queued for Claude. Inputs:
//!
//! * `text` — the last user message's content string (raw, still containing
//!   the `@` tokens Claude sees verbatim).
//! * [`MentionIndex`] — looked up to find the [`MentionEntry`] for each
//!   token. Missing entries are silently skipped; an unresolved `@foo`
//!   still reaches Claude as plain text.
//!
//! Each resolved token produces zero or more extra content blocks:
//!
//! | Kind      | Extra block(s)                                         |
//! |-----------|--------------------------------------------------------|
//! | Entity    | 1 × text block summarising class + space + canonical   |
//! | Script    | 1 × text block with the script contents (truncated)    |
//! | Service   | 1 × text block with the service metadata               |
//! | File      | 1 × text block (small text files) OR                   |
//! |           | 1 × image block (png/jpg/webp ≤ 4 MiB)                 |
//!
//! Large files are truncated at [`MAX_INLINE_TEXT_BYTES`] with a trailing
//! `[… N more bytes elided]` marker so Claude still sees the head plus a
//! signal that more exists.

use super::mention::{MentionEntry, MentionIndex, MentionKind};
use base64::Engine as _;
use serde_json::{json, Value};
use std::path::Path;

/// Hard cap on how many @refs we'll resolve per message. Prevents a single
/// turn from ballooning the request body by thousands of files.
pub const MAX_RESOLVED_MENTIONS: usize = 16;

/// Inline text files up to this size; beyond it we truncate and annotate.
/// Claude handles ~200 k tokens; one file at 32 KiB ≈ 8 k tokens, leaves
/// plenty of room for conversation history + multiple attachments.
pub const MAX_INLINE_TEXT_BYTES: usize = 32 * 1024;

/// Images over this size are summarised rather than inlined, to respect
/// Anthropic's ~5 MiB per-image limit and stay within practical base64
/// expansion overhead.
pub const MAX_INLINE_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

/// Scan `text` for `@kind:space/path` tokens in stable order (first
/// occurrence wins when a single ref appears twice). Returns owned strings
/// ready to key [`MentionIndex::entries`].
pub fn extract_mention_refs(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let start = i + 1;
            let mut end = start;
            // A mention token continues until whitespace or end-of-string.
            // We deliberately allow `:`, `/`, `-`, `.` inside — those are
            // part of canonical paths like `@entity:Space1/V-Cell/Housing`.
            while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            if end > start {
                let slice = &text[start..end];
                // Canonical tokens always include a `:` after the kind.
                if slice.contains(':') && !out.iter().any(|x| x == slice) {
                    out.push(slice.to_string());
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Produce content blocks for all @refs found in `text`. Returns an empty
/// Vec when nothing resolves. Caller appends these blocks to the user
/// message's existing `content` array.
pub fn resolve_mentions_to_blocks(
    text: &str,
    index: &MentionIndex,
    space_root: Option<&Path>,
    universe_root: Option<&Path>,
) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let refs = extract_mention_refs(text);
    let mut resolved = 0usize;

    for canonical in refs {
        if resolved >= MAX_RESOLVED_MENTIONS { break; }

        // Look up the entry by scanning entries. The MentionIndex keys by
        // MentionId (hash of kind + canonical); we recompute both.
        let Some(entry) = find_entry_by_canonical(index, &canonical) else {
            // Token like "@foo" without a registered handle — leave as
            // plain text for Claude to interpret.
            continue;
        };

        let block = match entry.kind {
            MentionKind::Entity => Some(entity_block(entry)),
            MentionKind::Service => Some(service_block(entry)),
            MentionKind::Script => script_block(entry, space_root, universe_root),
            MentionKind::File => file_block(entry, space_root, universe_root),
        };

        if let Some(b) = block {
            out.push(b);
            resolved += 1;
        }
    }

    out
}

/// Walk entries to find one whose canonical matches. MentionIndex doesn't
/// expose a canonical→entry index today; entries are typically small per
/// message so linear scan is acceptable.
fn find_entry_by_canonical<'a>(
    index: &'a MentionIndex,
    canonical: &str,
) -> Option<&'a MentionEntry> {
    // Fast path: canonical path already carries the kind prefix, so we can
    // recompute the exact MentionId.
    // Format is `kind:space/path`; split on first `:`.
    let (kind_str, _) = canonical.split_once(':')?;
    let kind = match kind_str {
        "entity" => MentionKind::Entity,
        "file" => MentionKind::File,
        "script" => MentionKind::Script,
        "service" => MentionKind::Service,
        _ => return None,
    };
    let id = super::mention::MentionId::from_canonical(kind, canonical);
    index.get(id)
        // Fallback: linear scan. Useful when the canonical was persisted
        // before a refactor changed the hashing convention.
        .or_else(|| index.entries().values().find(|e| e.canonical_path == canonical))
}

fn entity_block(entry: &MentionEntry) -> Value {
    let body = format!(
        "[@{0}] {1} — {2}\n(Entity reference; use tools like query_entities with name=\"{1}\" to inspect runtime state.)",
        entry.canonical_path, entry.name, entry.qualifier,
    );
    json!({ "type": "text", "text": body })
}

fn service_block(entry: &MentionEntry) -> Value {
    let body = format!(
        "[@{0}] {1} service in {2}\n{3}",
        entry.canonical_path, entry.name, entry.space, entry.qualifier,
    );
    json!({ "type": "text", "text": body })
}

fn script_block(
    entry: &MentionEntry,
    space_root: Option<&Path>,
    universe_root: Option<&Path>,
) -> Option<Value> {
    let path = resolve_entry_path(entry, space_root, universe_root)?;
    let bytes = std::fs::read(&path).ok()?;
    let (text, truncated) = coerce_to_text_with_truncation(&bytes);
    let body = format!(
        "[@{0}] {1} — Soul Script\n---\n{2}{3}",
        entry.canonical_path,
        entry.name,
        text,
        if truncated { "\n[…truncated]" } else { "" },
    );
    Some(json!({ "type": "text", "text": body }))
}

fn file_block(
    entry: &MentionEntry,
    space_root: Option<&Path>,
    universe_root: Option<&Path>,
) -> Option<Value> {
    let path = resolve_entry_path(entry, space_root, universe_root)?;
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if is_image_ext(&ext) {
        inline_image_block(entry, &path, &ext)
    } else {
        inline_text_block(entry, &path)
    }
}

fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif")
}

fn inline_image_block(entry: &MentionEntry, path: &Path, ext: &str) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_INLINE_IMAGE_BYTES {
        // Too large to inline; surface as a text description so Claude at
        // least knows the reference existed.
        return Some(json!({
            "type": "text",
            "text": format!(
                "[@{0}] {1} — image at {2} ({3} bytes, too large to inline)",
                entry.canonical_path, entry.name, path.display(), meta.len(),
            ),
        }));
    }
    let bytes = std::fs::read(path).ok()?;
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let media_type = match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    Some(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": data,
        },
    }))
}

fn inline_text_block(entry: &MentionEntry, path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    let (text, truncated) = coerce_to_text_with_truncation(&bytes);
    let body = format!(
        "[@{0}] {1} — {2}\n---\n{3}{4}",
        entry.canonical_path,
        entry.name,
        path.display(),
        text,
        if truncated { "\n[…truncated]" } else { "" },
    );
    Some(json!({ "type": "text", "text": body }))
}

/// Interpret a byte buffer as UTF-8 text, substituting the lossy-decoded
/// form and truncating at [`MAX_INLINE_TEXT_BYTES`] on the byte level.
fn coerce_to_text_with_truncation(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > MAX_INLINE_TEXT_BYTES;
    let slice = if truncated { &bytes[..MAX_INLINE_TEXT_BYTES] } else { bytes };
    (String::from_utf8_lossy(slice).into_owned(), truncated)
}

/// Resolve an entry to an absolute filesystem path. Static entries use
/// `{universe_root}/Spaces/{space}/{rel_path}`; live ECS entries don't
/// have a meaningful on-disk path (their `rel_path` is `@ecs/{index}`).
fn resolve_entry_path(
    entry: &MentionEntry,
    space_root: Option<&Path>,
    universe_root: Option<&Path>,
) -> Option<std::path::PathBuf> {
    if entry.rel_path.starts_with("@ecs/") {
        return None;
    }
    // Prefer Universe root so we can reach other Spaces, fall back to the
    // current Space's parent.
    let universe = universe_root.map(|u| u.to_path_buf())
        .or_else(|| space_root.and_then(|s| s.parent().map(|p| p.to_path_buf())))?;
    if entry.space.is_empty() {
        Some(universe.join(&entry.rel_path))
    } else {
        Some(universe.join("Spaces").join(&entry.space).join(&entry.rel_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_ref() {
        let refs = extract_mention_refs("check out @entity:Space1/V-Cell/Housing here");
        assert_eq!(refs, vec!["entity:Space1/V-Cell/Housing".to_string()]);
    }

    #[test]
    fn extract_multiple_refs() {
        let refs = extract_mention_refs(
            "compare @file:Space1/a.png with @file:Space2/b.png please",
        );
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().any(|r| r == "file:Space1/a.png"));
        assert!(refs.iter().any(|r| r == "file:Space2/b.png"));
    }

    #[test]
    fn deduplicates_refs() {
        let refs = extract_mention_refs("@file:s/a.png then again @file:s/a.png");
        assert_eq!(refs, vec!["file:s/a.png".to_string()]);
    }

    #[test]
    fn plain_at_is_skipped() {
        // Tokens without `:` aren't canonical paths — ignore.
        let refs = extract_mention_refs("hey @alice how are you");
        assert!(refs.is_empty());
    }

    #[test]
    fn email_address_is_skipped() {
        // Email-style `@` without leading whitespace: our scanner grabs
        // from `@` forward; "alice@host.com" has no leading `@`, so we
        // start at the embedded `@` and capture `host.com` — which has no
        // `:`, so it's rejected.
        let refs = extract_mention_refs("ping alice@host.com");
        assert!(refs.is_empty());
    }

    #[test]
    fn truncation_marker_appended() {
        let input = vec![b'a'; MAX_INLINE_TEXT_BYTES + 10];
        let (s, truncated) = coerce_to_text_with_truncation(&input);
        assert!(truncated);
        assert_eq!(s.len(), MAX_INLINE_TEXT_BYTES);
    }
}
