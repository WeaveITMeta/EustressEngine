// Resource resolvers — one per URI kind. Each resolver reads what it needs
// from the Universe filesystem and returns a `{uri, mimeType, text}` block
// fit for `resources/read`.
//
// Tools are actions the AI invokes; resources are memory the AI pins and
// refers back to. That separation means `eustress_read_script` (tool) and
// `eustress://script/...` (resource) can format the same data differently
// — JSON for programmatic consumption vs. markdown for pinning.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::uri::{self, EustressUri, UriKind};
use crate::universe::{
    extract_toml, list_scripts, list_spaces, read_capped, MAX_FILE_BYTES,
};

#[derive(Serialize, Debug, Clone)]
pub struct ResourceBlock {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ListedResource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

const MAX_LIST: usize = 200;

pub fn list_resources(universe: &Path) -> Vec<ListedResource> {
    let mut out: Vec<ListedResource> = Vec::new();

    for s in list_spaces(universe) {
        out.push(ListedResource {
            uri: uri::build(UriKind::Space, Some(&s.name), ""),
            name: format!("Space: {}", s.name),
            description: Some(format!(
                "Overview of {} — services, top-level scripts, counts.",
                s.name
            )),
            mime_type: Some("text/markdown".into()),
        });
        if out.len() >= MAX_LIST {
            return out;
        }
    }

    let scripts = list_scripts(universe, None);
    for s in &scripts {
        let space_root = universe.join("Spaces").join(&s.space);
        let rel = match Path::new(&s.folder).strip_prefix(&space_root) {
            Ok(r) => path_slash(r),
            Err(_) => continue,
        };
        out.push(ListedResource {
            uri: uri::build(UriKind::Script, Some(&s.space), &rel),
            name: format!("Script: {}/{}", s.space, rel),
            description: Some(format!("{} — source + summary", s.class)),
            mime_type: Some("text/markdown".into()),
        });
        if out.len() >= MAX_LIST {
            return out;
        }
    }

    // Conversations — Workshop archive.
    let session_dir = universe.join(".eustress").join("knowledge").join("sessions");
    if let Ok(read) = std::fs::read_dir(&session_dir) {
        let mut sessions: Vec<String> = read
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.strip_suffix(".json").map(|s| s.to_string())
            })
            .collect();
        sessions.sort();
        sessions.reverse();
        for id in sessions.into_iter().take(20) {
            out.push(ListedResource {
                uri: uri::build(UriKind::Conversation, None, &id),
                name: format!("Workshop conversation: {id}"),
                description: Some("Persisted Workshop chat history".into()),
                mime_type: Some("application/json".into()),
            });
            if out.len() >= MAX_LIST {
                return out;
            }
        }
    }

    for b in find_briefs(universe) {
        out.push(ListedResource {
            uri: uri::build(UriKind::Brief, None, &b.product),
            name: format!("Brief: {}", b.product),
            description: Some(format!("Product ideation brief at {}", b.rel_path)),
            mime_type: Some("application/toml".into()),
        });
        if out.len() >= MAX_LIST {
            return out;
        }
    }

    out
}

pub fn read_resource(universe: &Path, raw: &str) -> Result<ResourceBlock, String> {
    let u = uri::parse(raw).ok_or_else(|| format!("Malformed Eustress URI: {raw}"))?;
    match u.kind {
        UriKind::Space => read_space(universe, &u),
        UriKind::Script => read_script(universe, &u),
        UriKind::Entity => read_entity(universe, &u),
        UriKind::File => read_file(universe, &u),
        UriKind::Conversation => read_conversation(universe, &u),
        UriKind::Brief => read_brief(universe, &u),
    }
}

fn read_space(universe: &Path, u: &EustressUri) -> Result<ResourceBlock, String> {
    let space = u.space.as_deref().ok_or("space URI missing name")?;
    let space_dir = universe.join("Spaces").join(space);
    if !space_dir.is_dir() {
        return Err(format!("Space not found: {space}"));
    }

    let mut services: Vec<String> = Vec::new();
    if let Ok(read) = std::fs::read_dir(&space_dir) {
        for e in read.flatten() {
            if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') {
                    services.push(name);
                }
            }
        }
    }
    services.sort();

    let scripts = list_scripts(universe, Some(space));

    let mut parts = Vec::new();
    parts.push(format!("# Space: {space}"));
    parts.push(String::new());
    parts.push(format!("**Root:** `{}`", space_dir.display()));
    parts.push(String::new());
    parts.push(format!("## Services ({})", services.len()));
    for s in &services {
        parts.push(format!("- {s}"));
    }
    parts.push(String::new());
    parts.push(format!("## Scripts ({})", scripts.len()));
    for s in &scripts {
        let rel = Path::new(&s.folder)
            .strip_prefix(&space_dir)
            .map(path_slash)
            .unwrap_or_else(|_| s.folder.clone());
        parts.push(format!("- [{}] `{}`", s.class, rel));
    }

    Ok(ResourceBlock {
        uri: u.raw.clone(),
        mime_type: Some("text/markdown".into()),
        text: Some(parts.join("\n")),
        blob: None,
    })
}

fn read_script(universe: &Path, u: &EustressUri) -> Result<ResourceBlock, String> {
    let space = u.space.as_deref().ok_or("script URI missing space")?;
    let folder = universe.join("Spaces").join(space).join(&u.rel_path);
    if !folder.is_dir() {
        return Err(format!("Script folder not found: {}", folder.display()));
    }

    let name = folder.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let source_path = find_first_existing(&[
        folder.join(format!("{name}.rune")),
        folder.join(format!("{name}.luau")),
        folder.join(format!("{name}.soul")),
        folder.join("Source.rune"),
    ]);
    let summary_path = find_first_existing(&[
        folder.join(format!("{name}.md")),
        folder.join("Summary.md"),
    ]);
    let instance_toml = folder.join("_instance.toml");

    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("# Script: {space}/{}", u.rel_path));
    parts.push(String::new());
    parts.push(format!("**Folder:** `{}`", folder.display()));

    if instance_toml.exists() {
        if let Ok(text) = std::fs::read_to_string(&instance_toml) {
            let klass = extract_toml(&text, "class_name").unwrap_or_else(|| "(unknown)".into());
            parts.push(format!("**Class:** {klass}"));
        }
    }
    parts.push(String::new());

    if let Some(sp) = &summary_path {
        parts.push("## Summary".into());
        parts.push(String::new());
        let r = read_capped(sp).map_err(|e| e.to_string())?;
        parts.push(r.text.trim_end().to_string());
        if r.truncated {
            parts.push(format!("\n_[truncated at {MAX_FILE_BYTES} bytes]_"));
        }
        parts.push(String::new());
    }

    if let Some(src) = &source_path {
        parts.push("## Source".into());
        parts.push(String::new());
        let lang = if src.extension().and_then(|s| s.to_str()) == Some("luau") {
            "luau"
        } else if src.extension().and_then(|s| s.to_str()) == Some("rune") {
            "rust" // no hljs rune highlighter; rust is closest
        } else {
            ""
        };
        parts.push(format!("```{lang}"));
        let r = read_capped(src).map_err(|e| e.to_string())?;
        parts.push(r.text.trim_end().to_string());
        parts.push("```".into());
        if r.truncated {
            parts.push(format!("\n_[source truncated at {MAX_FILE_BYTES} bytes]_"));
        }
    } else {
        parts.push("_(no source file found)_".into());
    }

    Ok(ResourceBlock {
        uri: u.raw.clone(),
        mime_type: Some("text/markdown".into()),
        text: Some(parts.join("\n")),
        blob: None,
    })
}

fn read_entity(universe: &Path, u: &EustressUri) -> Result<ResourceBlock, String> {
    let space = u.space.as_deref().ok_or("entity URI missing space")?;
    let target = universe.join("Spaces").join(space).join(&u.rel_path);

    let toml_path = if target.is_dir() {
        target.join("_instance.toml")
    } else {
        target.clone()
    };
    if !toml_path.exists() {
        return Err(format!("Entity not found: {}", target.display()));
    }

    let r = read_capped(&toml_path).map_err(|e| e.to_string())?;
    let klass = extract_toml(&r.text, "class_name").unwrap_or_else(|| "(unknown)".into());
    let fallback_name = toml_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = extract_toml(&r.text, "name").unwrap_or(fallback_name);

    let mut parts = vec![
        format!("# Entity: {name}"),
        String::new(),
        format!("**Class:** {klass}"),
        format!("**Path:** `{}`", toml_path.display()),
        String::new(),
        "## _instance.toml".into(),
        String::new(),
        "```toml".into(),
        r.text.trim_end().to_string(),
        "```".into(),
    ];
    if r.truncated {
        parts.push(format!("\n_[truncated at {MAX_FILE_BYTES} bytes]_"));
    }

    Ok(ResourceBlock {
        uri: u.raw.clone(),
        mime_type: Some("text/markdown".into()),
        text: Some(parts.join("\n")),
        blob: None,
    })
}

fn read_file(universe: &Path, u: &EustressUri) -> Result<ResourceBlock, String> {
    let space = u.space.as_deref().ok_or("file URI missing space")?;
    let target = universe.join("Spaces").join(space).join(&u.rel_path);
    if !target.exists() {
        return Err(format!("File not found: {}", target.display()));
    }

    let ext = target
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{}", s.to_lowercase()))
        .unwrap_or_default();
    const BINARY: &[&str] = &[
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".mp3", ".wav", ".ogg",
        ".glb", ".gltf", ".fbx", ".obj", ".stl", ".ktx2",
    ];
    if BINARY.contains(&ext.as_str()) {
        return Err(format!(
            "Binary file not exposed as text resource: {}",
            target.display()
        ));
    }

    let r = read_capped(&target).map_err(|e| e.to_string())?;
    let mime = match ext.as_str() {
        ".md" => "text/markdown",
        ".toml" => "application/toml",
        ".json" => "application/json",
        _ => "text/plain",
    };
    let text = if r.truncated {
        format!("{}\n\n[truncated at {MAX_FILE_BYTES} bytes]", r.text)
    } else {
        r.text
    };

    Ok(ResourceBlock {
        uri: u.raw.clone(),
        mime_type: Some(mime.into()),
        text: Some(text),
        blob: None,
    })
}

fn read_conversation(universe: &Path, u: &EustressUri) -> Result<ResourceBlock, String> {
    let id = &u.rel_path;
    let session_file = universe
        .join(".eustress")
        .join("knowledge")
        .join("sessions")
        .join(format!("{id}.json"));
    if !session_file.exists() {
        return Err(format!("No such conversation: {id}"));
    }
    let text = std::fs::read_to_string(&session_file).map_err(|e| e.to_string())?;
    Ok(ResourceBlock {
        uri: u.raw.clone(),
        mime_type: Some("application/json".into()),
        text: Some(text),
        blob: None,
    })
}

fn read_brief(universe: &Path, u: &EustressUri) -> Result<ResourceBlock, String> {
    let product = &u.rel_path;
    let briefs = find_briefs(universe);
    let m = briefs
        .iter()
        .find(|b| b.product == *product)
        .ok_or_else(|| format!("No ideation brief for product: {product}"))?;
    let r = read_capped(&m.path).map_err(|e| e.to_string())?;
    let text = if r.truncated {
        format!("{}\n\n# [truncated at {MAX_FILE_BYTES} bytes]", r.text)
    } else {
        r.text
    };
    Ok(ResourceBlock {
        uri: u.raw.clone(),
        mime_type: Some("application/toml".into()),
        text: Some(text),
        blob: None,
    })
}

/// Reverse mapping — filesystem path → URI. Used by the watcher so a change
/// to `<universe>/Spaces/Space1/SoulService/foo/foo.rune` emits
/// `resources/updated` for `eustress://script/Space1/SoulService/foo`.
pub fn path_to_uri(universe: &Path, abs_path: &Path) -> Option<String> {
    let rel = abs_path.strip_prefix(universe).ok()?;
    let rel_str = path_slash(rel);
    let parts: Vec<&str> = rel_str.split('/').collect();

    // `.eustress/knowledge/sessions/<id>.json` → conversation
    if parts.len() == 4
        && parts[0] == ".eustress"
        && parts[1] == "knowledge"
        && parts[2] == "sessions"
    {
        if let Some(id) = parts[3].strip_suffix(".json") {
            return Some(uri::build(UriKind::Conversation, None, id));
        }
    }

    // `Spaces/<space>/.../ideation_brief.toml` → brief
    if parts.len() >= 3 && parts[0] == "Spaces" {
        if let Some(last) = parts.last() {
            if *last == "ideation_brief.toml" && parts.len() >= 4 {
                let product = parts[parts.len() - 2];
                return Some(uri::build(UriKind::Brief, None, product));
            }
        }
    }

    // `Spaces/<space>/<inner...>`
    if parts.len() < 3 || parts[0] != "Spaces" {
        return None;
    }
    let space = parts[1];
    let inner = parts[2..].join("/");
    let file_name = parts.last().copied().unwrap_or("");
    let parent_parts = &parts[2..parts.len() - 1];

    if !parent_parts.is_empty() {
        let parent_abs = universe.join("Spaces").join(space).join(parent_parts.join("/"));
        let parent_name = parent_parts.last().copied().unwrap_or("");
        let is_script_file = file_name == format!("{parent_name}.rune")
            || file_name == format!("{parent_name}.md")
            || file_name == "Source.rune"
            || file_name == "Summary.md"
            || file_name == "_instance.toml";
        if is_script_file && parent_abs.join("_instance.toml").exists() {
            return Some(uri::build(
                UriKind::Script,
                Some(space),
                &parent_parts.join("/"),
            ));
        }
    }

    if file_name == "_instance.toml" && !parent_parts.is_empty() {
        return Some(uri::build(
            UriKind::Entity,
            Some(space),
            &parent_parts.join("/"),
        ));
    }

    Some(uri::build(UriKind::File, Some(space), &inner))
}

fn find_first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.exists()).cloned()
}

pub struct BriefEntry {
    pub product: String,
    pub path: PathBuf,
    pub rel_path: String,
}

pub fn find_briefs(universe: &Path) -> Vec<BriefEntry> {
    let mut out = Vec::new();
    let spaces_dir = universe.join("Spaces");
    if !spaces_dir.is_dir() {
        return out;
    }
    walk_for_briefs(&spaces_dir, universe, 4, &mut out);
    out
}

fn walk_for_briefs(dir: &Path, universe: &Path, depth: i32, out: &mut Vec<BriefEntry>) {
    if depth < 0 || out.len() >= 100 {
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for e in read.flatten() {
        let ft = match e.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_file() && name == "ideation_brief.toml" {
            let product = dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let path = e.path();
            let rel_path = path
                .strip_prefix(universe)
                .map(path_slash)
                .unwrap_or_default();
            out.push(BriefEntry { product, path, rel_path });
            continue;
        }
        if ft.is_dir() && !name.starts_with('.') {
            walk_for_briefs(&e.path(), universe, depth - 1, out);
        }
    }
}

fn path_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

