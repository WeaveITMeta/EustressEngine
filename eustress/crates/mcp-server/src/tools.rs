// Tool registry for the Eustress MCP server. Each tool is a pair of (JSON
// Schema description, handler). Schema is what the client sees in
// `tools/list`; handler runs on `tools/call`.
//
// All tools take `universe?: string` as their first input so clients can
// target a specific Universe even without the server launch config. When
// absent, the server's default Universe (from env/arg) is used.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::universe::{
    discover_universes, find_entity, find_universe_root, list_scripts, list_spaces,
    read_capped, resolve_in_universe, search_universe, MAX_FILE_BYTES,
};

/// Mutable state held by main.rs and passed to each handler.
pub struct ServerState {
    pub current_universe: Option<PathBuf>,
    pub search_roots: Vec<PathBuf>,
}

/// Tool result shape. Matches the MCP CallToolResult schema: `content` is a
/// list of text blocks; `isError` flips the error flag the client renders.
pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok_json(v: &impl serde::Serialize) -> Self {
        let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
        Self {
            content: vec![json!({ "type": "text", "text": text })],
            is_error: false,
        }
    }
    pub fn ok_text(t: impl Into<String>) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": t.into() })],
            is_error: false,
        }
    }
    pub fn err(msg: impl AsRef<str>) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": format!("Error: {}", msg.as_ref()) })],
            is_error: true,
        }
    }
    pub fn to_json(&self) -> Value {
        json!({
            "content": self.content,
            "isError": self.is_error,
        })
    }
}

pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub handler: fn(&Value, &mut ServerState) -> ToolResult,
}

/// Precedence: explicit arg → walk-from-path arg → current default → walk from cwd.
fn resolve_universe(args: &Value, state: &ServerState) -> Option<PathBuf> {
    if let Some(u) = args.get("universe").and_then(|v| v.as_str()) {
        let trimmed = u.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        if !p.is_empty() {
            let start = if Path::new(p).is_absolute() {
                PathBuf::from(p)
            } else {
                let base = state
                    .current_universe
                    .clone()
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                base.join(p)
            };
            if let Some(found) = find_universe_root(&start) {
                return Some(found);
            }
        }
    }
    if let Some(u) = &state.current_universe {
        return Some(u.clone());
    }
    find_universe_root(&std::env::current_dir().unwrap_or_default())
}

fn require_universe(args: &Value, state: &ServerState) -> Result<PathBuf, ToolResult> {
    resolve_universe(args, state).ok_or_else(|| {
        ToolResult::err(
            "No Universe configured. Pass `universe` explicitly, or call \
             `eustress_list_universes` + `eustress_set_default_universe` first.",
        )
    })
}

fn universe_arg_fragment() -> Value {
    json!({
        "universe": {
            "type": "string",
            "description": "Absolute path to the Universe root. Optional — auto-resolves from `path` arg, or falls back to the server's current default."
        }
    })
}

// ─── eustress_list_universes ─────────────────────────────────────────────

fn list_universes_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "extra_roots": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Additional directories to scan (in addition to the server's configured search roots)."
            }
        }
    })
}

fn list_universes_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let extra: Vec<PathBuf> = args
        .get("extra_roots")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default();
    let mut roots = state.search_roots.clone();
    roots.extend(extra);
    let mut discovered = discover_universes(&roots);
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(cu) = find_universe_root(&cwd) {
            if !discovered.contains(&cu) {
                discovered.push(cu);
            }
        }
    }
    discovered.sort();
    ToolResult::ok_json(&json!({
        "current": state.current_universe.as_ref().map(|p| p.display().to_string()),
        "search_roots": roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "universes": discovered.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    }))
}

// ─── eustress_set_default_universe ──────────────────────────────────────

fn set_default_universe_schema() -> Value {
    json!({
        "type": "object",
        "required": ["universe"],
        "properties": {
            "universe": {
                "type": "string",
                "description": "Absolute path to a Universe root (a folder containing `Spaces/`)."
            }
        }
    })
}

fn set_default_universe_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let requested = match args.get("universe").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => u,
        _ => return ToolResult::err("`universe` is required"),
    };
    let abs = PathBuf::from(requested);
    if !abs.join("Spaces").is_dir() {
        return ToolResult::err(format!(
            "'{}' is not a Universe (no Spaces/ directory found).",
            abs.display()
        ));
    }
    let previous = state.current_universe.clone();
    state.current_universe = Some(abs.clone());
    let changed = previous.as_ref() != Some(&abs);
    ToolResult::ok_json(&json!({
        "previous": previous.map(|p| p.display().to_string()),
        "current": state.current_universe.as_ref().map(|p| p.display().to_string()),
        "changed": changed,
    }))
}

// ─── eustress_list_spaces ────────────────────────────────────────────────

fn list_spaces_schema() -> Value {
    json!({ "type": "object", "properties": universe_arg_fragment() })
}

fn list_spaces_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let spaces = list_spaces(&u);
    ToolResult::ok_json(&json!({
        "universe": u.display().to_string(),
        "spaces": spaces,
    }))
}

// ─── eustress_list_scripts ───────────────────────────────────────────────

fn list_scripts_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["space"] = json!({
        "type": "string",
        "description": "Optional Space name to scope the listing. Omit to walk every Space."
    });
    json!({ "type": "object", "properties": props })
}

fn list_scripts_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let space = args.get("space").and_then(|v| v.as_str());
    let scripts = list_scripts(&u, space);
    ToolResult::ok_json(&json!({
        "universe": u.display().to_string(),
        "count": scripts.len(),
        "scripts": scripts,
    }))
}

// ─── eustress_read_script ────────────────────────────────────────────────

fn read_script_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["path"] = json!({
        "type": "string",
        "description": "Absolute path to the script folder or its source file."
    });
    json!({
        "type": "object",
        "required": ["path"],
        "properties": props,
    })
}

fn read_script_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let p = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return ToolResult::err("`path` is required"),
    };
    let resolved = match resolve_in_universe(&u, p) {
        Ok(r) => r,
        Err(e) => return ToolResult::err(e),
    };
    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(_) => return ToolResult::err(format!("no such path: {}", resolved.display())),
    };
    let folder = if meta.is_dir() {
        resolved.clone()
    } else {
        resolved
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(resolved.clone())
    };

    let name = folder
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let candidates = [
        folder.join(format!("{name}.rune")),
        folder.join("Source.rune"),
    ];
    let mut source_path: Option<PathBuf> = candidates.iter().find(|p| p.exists()).cloned();
    if source_path.is_none() {
        if let Ok(read) = std::fs::read_dir(&folder) {
            for e in read.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.ends_with(".rune") || n.ends_with(".luau") {
                    source_path = Some(e.path());
                    break;
                }
            }
        }
    }
    let summary_candidates = [
        folder.join(format!("{name}.md")),
        folder.join("Summary.md"),
    ];
    let summary_path: Option<PathBuf> = summary_candidates.iter().find(|p| p.exists()).cloned();

    let source = match &source_path {
        Some(p) => match read_capped(p) {
            Ok(r) => Some(json!({ "text": r.text, "truncated": r.truncated })),
            Err(_) => None,
        },
        None => None,
    };
    let summary = match &summary_path {
        Some(p) => match read_capped(p) {
            Ok(r) => Some(json!({ "text": r.text, "truncated": r.truncated })),
            Err(_) => None,
        },
        None => None,
    };

    ToolResult::ok_json(&json!({
        "folder": folder.display().to_string(),
        "name": name,
        "source_path": source_path.map(|p| p.display().to_string()),
        "summary_path": summary_path.map(|p| p.display().to_string()),
        "source": source,
        "summary": summary,
        "byte_cap": MAX_FILE_BYTES,
    }))
}

// ─── eustress_find_entity ────────────────────────────────────────────────

fn find_entity_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["query"] = json!({ "type": "string", "description": "Name substring to match." });
    props["space"] = json!({ "type": "string", "description": "Optional Space name to scope the search." });
    json!({ "type": "object", "required": ["query"], "properties": props })
}

fn find_entity_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.is_empty() => q,
        _ => return ToolResult::err("`query` is required"),
    };
    let space = args.get("space").and_then(|v| v.as_str());
    let matches = find_entity(&u, query, space);
    ToolResult::ok_json(&json!({
        "universe": u.display().to_string(),
        "query": query,
        "count": matches.len(),
        "matches": matches,
    }))
}

// ─── eustress_list_assets ────────────────────────────────────────────────

fn list_assets_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["space"] = json!({ "type": "string", "description": "Optional Space name (omit for all)." });
    props["kind"] = json!({
        "type": "string",
        "enum": ["meshes", "textures", "gui", "audio", "all"],
        "description": "Asset family filter. Default: \"all\"."
    });
    json!({ "type": "object", "properties": props })
}

fn list_assets_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let space = args.get("space").and_then(|v| v.as_str());
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("all");

    let kind_exts: &[(&str, &[&str])] = &[
        ("meshes", &[".glb", ".gltf", ".fbx", ".obj", ".stl"]),
        ("textures", &[".png", ".jpg", ".jpeg", ".webp", ".tga", ".ktx2"]),
        ("gui", &[".slint"]),
        ("audio", &[".wav", ".mp3", ".ogg", ".flac"]),
    ];
    let exts: Vec<&str> = if kind == "all" {
        kind_exts.iter().flat_map(|(_, es)| es.iter().copied()).collect()
    } else {
        kind_exts
            .iter()
            .find(|(n, _)| *n == kind)
            .map(|(_, es)| es.to_vec())
            .unwrap_or_default()
    };

    let spaces = list_spaces(&u);
    let target: Vec<_> = spaces
        .iter()
        .filter(|s| space.map_or(true, |f| f == s.name))
        .cloned()
        .collect();

    let mut out: Vec<Value> = Vec::new();
    for s in &target {
        walk(Path::new(&s.path), &mut |p: &Path| {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_lowercase()))
                .unwrap_or_default();
            if !exts.contains(&ext.as_str()) {
                return;
            }
            let k = kind_exts
                .iter()
                .find(|(_, es)| es.contains(&ext.as_str()))
                .map(|(n, _)| *n)
                .unwrap_or("other");
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            out.push(json!({
                "path": p.display().to_string(),
                "space": s.name,
                "kind": k,
                "size": size,
            }));
        });
        if out.len() >= 500 {
            break;
        }
    }
    out.truncate(500);
    ToolResult::ok_json(&json!({
        "universe": u.display().to_string(),
        "kind": kind,
        "count": out.len(),
        "assets": out,
    }))
}

fn walk(dir: &Path, cb: &mut dyn FnMut(&Path)) {
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
        if ft.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            walk(&e.path(), cb);
        } else if ft.is_file() {
            cb(&e.path());
        }
    }
}

// ─── eustress_search_universe ────────────────────────────────────────────

fn search_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["query"] = json!({ "type": "string", "description": "Text to search for." });
    props["space"] = json!({ "type": "string", "description": "Optional Space to scope the search." });
    json!({ "type": "object", "required": ["query"], "properties": props })
}

fn search_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.is_empty() => q,
        _ => return ToolResult::err("`query` is required"),
    };
    let space = args.get("space").and_then(|v| v.as_str());
    let matches = search_universe(&u, query, space);
    ToolResult::ok_json(&json!({
        "universe": u.display().to_string(),
        "query": query,
        "count": matches.len(),
        "matches": matches,
    }))
}

// ─── Git wrappers ────────────────────────────────────────────────────────

fn run_git(universe: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(universe)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "git exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git_status_schema() -> Value {
    json!({ "type": "object", "properties": universe_arg_fragment() })
}

fn git_status_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match run_git(&u, &["status", "--porcelain=v1"]) {
        Ok(out) => {
            let entries: Vec<_> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let status = line.get(..2).unwrap_or("").trim();
                    let path = line.get(3..).unwrap_or("");
                    json!({ "status": status, "path": path })
                })
                .collect();
            ToolResult::ok_json(&json!({
                "universe": u.display().to_string(),
                "count": entries.len(),
                "entries": entries,
            }))
        }
        Err(e) => ToolResult::err(format!("git status failed: {e}")),
    }
}

fn git_log_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["limit"] = json!({ "type": "integer", "description": "Max commits to return. 1-200." });
    json!({ "type": "object", "properties": props })
}

fn git_log_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let limit_raw = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);
    let limit = limit_raw.clamp(1, 200);
    let limit_str = format!("-n{limit}");
    match run_git(&u, &["log", &limit_str, "--pretty=format:%H%x09%an%x09%ar%x09%s"]) {
        Ok(out) => {
            let commits: Vec<_> = out
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let mut parts = line.splitn(4, '\t');
                    let hash = parts.next().unwrap_or("").to_string();
                    let author = parts.next().unwrap_or("").to_string();
                    let date = parts.next().unwrap_or("").to_string();
                    let subject = parts.next().unwrap_or("").to_string();
                    json!({ "hash": hash, "author": author, "date": date, "subject": subject })
                })
                .collect();
            ToolResult::ok_json(&json!({
                "universe": u.display().to_string(),
                "count": commits.len(),
                "commits": commits,
            }))
        }
        Err(e) => ToolResult::err(format!("git log failed: {e}")),
    }
}

fn git_diff_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["path"] = json!({ "type": "string", "description": "Optional path scope (relative to Universe or absolute)." });
    props["staged"] = json!({ "type": "boolean", "description": "If true, show staged diff (--cached). Default false." });
    json!({ "type": "object", "properties": props })
}

fn git_diff_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let staged = args.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut diff_args: Vec<String> = vec!["diff".into()];
    if staged {
        diff_args.push("--cached".into());
    }
    if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        if !p.is_empty() {
            let resolved = match resolve_in_universe(&u, p) {
                Ok(r) => r,
                Err(e) => return ToolResult::err(e),
            };
            let rel = resolved
                .strip_prefix(&u)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| resolved.display().to_string());
            diff_args.push("--".into());
            diff_args.push(rel);
        }
    }
    let refs: Vec<&str> = diff_args.iter().map(|s| s.as_str()).collect();
    match run_git(&u, &refs) {
        Ok(out) => {
            if out.is_empty() {
                ToolResult::ok_text("(no changes)")
            } else {
                ToolResult::ok_text(out)
            }
        }
        Err(e) => ToolResult::err(format!("git diff failed: {e}")),
    }
}

// ─── eustress_create_script ──────────────────────────────────────────────

fn create_script_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["space"] = json!({ "type": "string", "description": "Target Space name." });
    props["service"] = json!({ "type": "string", "description": "Service folder (e.g. \"SoulService\", \"StarterPlayerScripts\")." });
    props["name"] = json!({ "type": "string", "description": "Script folder name. Used for the source + summary file names too." });
    props["body"] = json!({ "type": "string", "description": "Initial Rune source. Optional; a minimal stub is written if omitted." });
    props["summary"] = json!({ "type": "string", "description": "Initial summary markdown. Optional." });
    props["class"] = json!({
        "type": "string",
        "enum": ["Script", "SoulScript", "LocalScript", "ModuleScript"],
        "description": "Script class_name. Default: \"Script\"."
    });
    json!({
        "type": "object",
        "required": ["space", "service", "name"],
        "properties": props,
    })
}

fn create_script_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let space = args.get("space").and_then(|v| v.as_str()).unwrap_or("");
    let service = args.get("service").and_then(|v| v.as_str()).unwrap_or("");
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let klass = args.get("class").and_then(|v| v.as_str()).unwrap_or("Script");
    if space.is_empty() || service.is_empty() || name.is_empty() {
        return ToolResult::err("space, service, and name are all required");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || !name.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
    {
        return ToolResult::err(format!("invalid script name: {name}"));
    }

    let folder = u.join("Spaces").join(space).join(service).join(name);
    if folder.exists() {
        return ToolResult::err(format!(
            "folder already exists: {}",
            folder.display()
        ));
    }
    if let Err(e) = std::fs::create_dir_all(&folder) {
        return ToolResult::err(format!("cannot create folder: {e}"));
    }

    let default_body = "pub fn main() {\n    println(\"Hello from Rune!\");\n}\n";
    let body = args
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or(default_body);
    let default_summary = format!("# {name}\n\nNew script.\n");
    let summary = args
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or(&default_summary);

    let instance_toml = format!(
        "[metadata]\nclass_name = \"{klass}\"\narchivable = true\n\n[script]\nsource = \"{name}.rune\"\n"
    );

    let instance_path = folder.join("_instance.toml");
    let rune_path = folder.join(format!("{name}.rune"));
    let md_path = folder.join(format!("{name}.md"));
    if let Err(e) = std::fs::write(&instance_path, instance_toml) {
        return ToolResult::err(format!("write _instance.toml: {e}"));
    }
    if let Err(e) = std::fs::write(&rune_path, body) {
        return ToolResult::err(format!("write {name}.rune: {e}"));
    }
    if let Err(e) = std::fs::write(&md_path, summary) {
        return ToolResult::err(format!("write {name}.md: {e}"));
    }

    ToolResult::ok_json(&json!({
        "created": true,
        "folder": folder.display().to_string(),
        "files": [
            instance_path.display().to_string(),
            rune_path.display().to_string(),
            md_path.display().to_string(),
        ],
    }))
}

// ─── eustress_get_conversation ──────────────────────────────────────────

fn get_conversation_schema() -> Value {
    let mut props = universe_arg_fragment();
    props["session_id"] = json!({ "type": "string", "description": "Workshop session id." });
    json!({
        "type": "object",
        "required": ["session_id"],
        "properties": props,
    })
}

fn get_conversation_handler(args: &Value, state: &mut ServerState) -> ToolResult {
    let u = match require_universe(args, state) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let sid = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return ToolResult::err("`session_id` is required"),
    };
    let session_file = u
        .join(".eustress")
        .join("knowledge")
        .join("sessions")
        .join(format!("{sid}.json"));
    let text = match std::fs::read_to_string(&session_file) {
        Ok(t) => t,
        Err(_) => return ToolResult::err(format!("no such session: {sid}")),
    };
    // Validate JSON then re-emit pretty so the AI sees clean structure.
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => ToolResult::ok_json(&v),
        Err(_) => ToolResult::ok_text(text),
    }
}

pub fn all_tools() -> Vec<ToolDescriptor> {
    // Only ONE hand-rolled tool remains: `set_active_universe`. Every
    // other listing / read / write / search tool that used to live
    // here as `eustress_*` has been migrated to the shared
    // `eustress-tools` registry (see `shared_registry.rs`), so
    // exposing both surfaces produced ~30 duplicate schemas in
    // `tools/list` — bloating the LLM's context and splitting its
    // routing heuristic across two equivalent tools per operation.
    //
    // `set_active_universe` stays hand-rolled because it's the only
    // tool that needs to *mutate* the server's live
    // `ServerState.current_universe`. The shared-registry tools
    // receive an immutable `ToolContext` per call and can't reach
    // the MCP server's session state. The shared crate's
    // `set_next_launch_universe` writes the on-disk sentinel for the
    // next engine launch — complementary, but doesn't change what
    // this MCP session resolves paths against.
    vec![
        ToolDescriptor {
            name: "set_active_universe",
            description: "Switch the MCP session's active Universe. Every subsequent tool call that resolves paths against the Universe (read_file, list_directory, list_space_contents, query_entities, git_*, run_bash, etc.) will operate on the new Universe immediately. Use this when the server's startup-resolved Universe isn't the one you want to work in — the alternative `set_next_launch_universe` only writes a sentinel file read by the ENGINE on its next launch, and has no effect on the current MCP session.",
            input_schema: set_default_universe_schema,
            handler: set_default_universe_handler,
        },
    ]
}
