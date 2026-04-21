// Eustress MCP URI scheme — `eustress://<kind>/<...>` addressable resources.
//
// URIs here are intentionally 1:1 with Workshop's `@mention` canonical paths
// so the two systems share a single mental model: anything you can @-mention
// in Eustress Engine, you can pin as an MCP resource in an external editor.
//
// Shape
//   eustress://<kind>/<space>/<...path>
//
// Kinds (v1)
//   space         folder-like; Space overview
//   script        a SoulScript/Rune/Luau folder — source + summary + diagnostics
//   entity        a folder-based entity, identified by its `_instance.toml`
//   file          any text file under a Space (markdown, toml, rune raw)
//   conversation  a Workshop session archive (no space in URI)
//   brief         an ideation_brief.toml (no space; looked up across Spaces)

use serde::Serialize;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UriKind {
    Space,
    Script,
    Entity,
    File,
    Conversation,
    Brief,
}

impl UriKind {
    fn as_str(self) -> &'static str {
        match self {
            UriKind::Space => "space",
            UriKind::Script => "script",
            UriKind::Entity => "entity",
            UriKind::File => "file",
            UriKind::Conversation => "conversation",
            UriKind::Brief => "brief",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "space" => UriKind::Space,
            "script" => UriKind::Script,
            "entity" => UriKind::Entity,
            "file" => UriKind::File,
            "conversation" => UriKind::Conversation,
            "brief" => UriKind::Brief,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct EustressUri {
    pub kind: UriKind,
    /// Space name for space-qualified kinds. `None` for conversation/brief.
    pub space: Option<String>,
    /// Tail path after `<kind>/<space?>/`. Empty string when absent.
    pub rel_path: String,
    /// Original URI string, preserved for echo in responses.
    pub raw: String,
}

const SCHEME: &str = "eustress://";

pub fn parse(raw: &str) -> Option<EustressUri> {
    let body = raw.strip_prefix(SCHEME)?;
    let (kind_str, rest) = match body.find('/') {
        Some(idx) => (&body[..idx], &body[idx + 1..]),
        None => (body, ""),
    };
    let kind = UriKind::from_str(kind_str)?;

    // conversation + brief aren't space-qualified — the whole rest is the id.
    if matches!(kind, UriKind::Conversation | UriKind::Brief) {
        return Some(EustressUri {
            kind,
            space: None,
            rel_path: rest.to_string(),
            raw: raw.to_string(),
        });
    }

    // Space-qualified: first segment is the Space, remainder is the tail.
    let (space, rel_path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx + 1..]),
        None => (rest, ""),
    };
    if space.is_empty() {
        return None;
    }
    Some(EustressUri {
        kind,
        space: Some(space.to_string()),
        rel_path: rel_path.to_string(),
        raw: raw.to_string(),
    })
}

pub fn build(kind: UriKind, space: Option<&str>, rel_path: &str) -> String {
    if matches!(kind, UriKind::Conversation | UriKind::Brief) {
        return format!("{}{}/{}", SCHEME, kind.as_str(), rel_path);
    }
    let tail = if rel_path.is_empty() {
        String::new()
    } else {
        format!("/{}", rel_path)
    };
    format!(
        "{}{}/{}{}",
        SCHEME,
        kind.as_str(),
        space.unwrap_or(""),
        tail,
    )
}

/// URI templates advertised via `resources/templates/list`. Clients use these
/// to construct URIs for resources they discover out-of-band. `{+path}` is the
/// RFC 6570 "reserved expansion" form that preserves slashes.
#[derive(Serialize)]
pub struct UriTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<&'static str>,
}

pub fn templates() -> Vec<UriTemplate> {
    vec![
        UriTemplate {
            uri_template: "eustress://space/{space}",
            name: "Space",
            description: "A Space in the current Universe. The resource body is an overview of its services + top-level scripts/entities.",
            mime_type: Some("text/markdown"),
        },
        UriTemplate {
            uri_template: "eustress://script/{space}/{+path}",
            name: "Script",
            description: "A Rune/Luau script folder. Returns source + summary + (when available) live diagnostics in one bundled markdown document.",
            mime_type: Some("text/markdown"),
        },
        UriTemplate {
            uri_template: "eustress://entity/{space}/{+path}",
            name: "Entity",
            description: "A folder-based entity identified by its `_instance.toml`. Returns the TOML plus a summary of children.",
            mime_type: Some("text/markdown"),
        },
        UriTemplate {
            uri_template: "eustress://file/{space}/{+path}",
            name: "File",
            description: "Any text file under a Space — README, design notes, raw .rune or .toml. Binary files are rejected.",
            mime_type: Some("text/plain"),
        },
        UriTemplate {
            uri_template: "eustress://conversation/{session_id}",
            name: "Workshop conversation",
            description: "A persisted Workshop chat session from `.eustress/knowledge/sessions/`.",
            mime_type: Some("application/json"),
        },
        UriTemplate {
            uri_template: "eustress://brief/{product}",
            name: "Ideation brief",
            description: "An `ideation_brief.toml` generated by Workshop, addressed by product name.",
            mime_type: Some("application/toml"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_script() {
        let u = parse("eustress://script/Space1/SoulService/foo").unwrap();
        assert_eq!(u.kind, UriKind::Script);
        assert_eq!(u.space.as_deref(), Some("Space1"));
        assert_eq!(u.rel_path, "SoulService/foo");
        assert_eq!(
            build(u.kind, u.space.as_deref(), &u.rel_path),
            "eustress://script/Space1/SoulService/foo",
        );
    }

    #[test]
    fn conversation_has_no_space() {
        let u = parse("eustress://conversation/abc-123").unwrap();
        assert!(u.space.is_none());
        assert_eq!(u.rel_path, "abc-123");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not-a-uri").is_none());
        assert!(parse("eustress://unknown/x").is_none());
        assert!(parse("eustress://space/").is_none()); // empty space
    }
}
