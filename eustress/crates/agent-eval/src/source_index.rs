//! Where each tool is defined, read from source at run time.
//!
//! The census lists tools from the running servers, so this index only adds
//! a `path:line` and, for Workshop tools, the declared `modes`. It scans the
//! three places tool descriptors live: `eustress/crates/tools/src`,
//! `eustress/crates/mcp-server/src` and `eustress/crates/engine/src/workshop`.
//! Test modules and `*_tests.rs` files are skipped.

use std::path::{Path, PathBuf};

/// The directories scanned, relative to the repository root.
pub const SCANNED_DIRS: &[&str] = &[
    "eustress/crates/tools/src",
    "eustress/crates/mcp-server/src",
    "eustress/crates/engine/src/workshop",
];

/// One located definition.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceHit {
    /// Repository-relative, forward slashes.
    pub path: String,
    /// 1-based.
    pub line: usize,
    /// "definition" (`name: "x"`), "helper" (a `tree_tool("x"` call),
    /// "const" (`const X: &str = "x"`), or "literal" (the first other
    /// occurrence of the quoted name).
    pub kind: &'static str,
    /// The `WorkshopMode` variants on the definition's `modes:` line, when
    /// one follows the hit.
    pub modes: Option<Vec<String>>,
}

impl SourceHit {
    pub fn location(&self) -> String {
        format!("{}:{}", self.path, self.line)
    }
}

struct SourceFile {
    rel: String,
    lines: Vec<String>,
    /// Index of the first test line; lines from here on are ignored.
    test_start: usize,
}

/// Every scanned file, held in memory.
pub struct SourceIndex {
    files: Vec<SourceFile>,
}

impl SourceIndex {
    pub fn load(repo_root: &Path) -> Self {
        let mut paths = Vec::new();
        for d in SCANNED_DIRS {
            collect_rs(&repo_root.join(d), &mut paths);
        }
        paths.sort();
        let mut files = Vec::new();
        for p in paths {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            let test_start = test_region_start(&lines);
            let rel = p
                .strip_prefix(repo_root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            files.push(SourceFile { rel, lines, test_start });
        }
        SourceIndex { files }
    }

    /// Build from in-memory files (tests).
    pub fn from_sources(sources: &[(&str, &str)]) -> Self {
        let files = sources
            .iter()
            .map(|(rel, text)| {
                let lines: Vec<String> = text.lines().map(str::to_string).collect();
                let test_start = test_region_start(&lines);
                SourceFile { rel: rel.to_string(), lines, test_start }
            })
            .collect();
        SourceIndex { files }
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Locate `name`, best match first across all files.
    pub fn find(&self, name: &str) -> Option<SourceHit> {
        let quoted = format!("\"{name}\"");
        let def = format!("name: {quoted}");
        let helper = format!("tree_tool({quoted}");
        // Pass 1: `name: "x"`. Pass 2: a helper call. Pass 3: a const.
        // Pass 4: any occurrence outside comments.
        for pass in 0..4 {
            for f in &self.files {
                for (i, line) in f.lines.iter().enumerate().take(f.test_start) {
                    let t = line.trim();
                    if t.starts_with("//") || !line.contains(&quoted) {
                        continue;
                    }
                    let prev_helper = i > 0 && f.lines[i - 1].trim_end().ends_with("tree_tool(");
                    let kind = match pass {
                        0 if t.starts_with(&def) => Some("definition"),
                        1 if line.contains(&helper) || prev_helper => Some("helper"),
                        2 if t.starts_with("const ") || t.starts_with("pub const ") => Some("const"),
                        3 => Some("literal"),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        return Some(SourceHit {
                            path: f.rel.clone(),
                            line: i + 1,
                            kind,
                            modes: modes_after(&f.lines, i, f.test_start),
                        });
                    }
                }
            }
        }
        None
    }

    /// Every `name: "x"` that sits in a `ToolDefinition { .. }` literal,
    /// with its location.
    pub fn declared_names(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for f in &self.files {
            for (i, line) in f.lines.iter().enumerate().take(f.test_start) {
                let t = line.trim();
                let Some(rest) = t.strip_prefix("name: \"") else { continue };
                let Some(end) = rest.find('"') else { continue };
                let lo = i.saturating_sub(3);
                if !f.lines[lo..i].iter().any(|l| l.contains("ToolDefinition {")) {
                    continue;
                }
                out.push((rest[..end].to_string(), format!("{}:{}", f.rel, i + 1)));
            }
        }
        out
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// The first `#[cfg(test)]` that is followed by a `mod` line.
fn test_region_start(lines: &[String]) -> usize {
    for (i, l) in lines.iter().enumerate() {
        if l.trim() == "#[cfg(test)]" {
            let next = lines[i + 1..].iter().map(|s| s.trim()).find(|s| !s.is_empty());
            if next.map(|s| s.starts_with("mod ") || s.starts_with("pub mod ")).unwrap_or(false) {
                return i;
            }
        }
    }
    lines.len()
}

/// The `WorkshopMode::X` variants on the first `modes:` line within 80
/// lines after `start`, stopping at the next `fn execute` or definition.
fn modes_after(lines: &[String], start: usize, end: usize) -> Option<Vec<String>> {
    let stop = end.min(start + 80);
    for (j, l) in lines.iter().enumerate().take(stop).skip(start) {
        if j > start && (l.contains("fn execute") || l.contains("fn definition")) {
            return None;
        }
        if let Some(idx) = l.find("modes:") {
            let tail = &l[idx..];
            let mut modes = Vec::new();
            for part in tail.split("WorkshopMode::").skip(1) {
                let variant: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if !variant.is_empty() {
                    modes.push(variant);
                }
            }
            return if modes.is_empty() { None } else { Some(modes) };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOLS_RS: &str = r#"
impl ToolHandler for CreateEntityTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_entity",
            description: "Create a thing",
            input_schema: serde_json::json!({ "type": "object" }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }
    fn execute(&self) {}
}
const TOOL: &str = "particle_simulation";
// a comment naming "ghost_tool"
#[cfg(test)]
mod tests {
    fn t() { let _ = "test_only_tool"; }
}
"#;

    #[test]
    fn definitions_are_found_with_their_modes() {
        let idx = SourceIndex::from_sources(&[("eustress/crates/tools/src/entity_tools.rs", TOOLS_RS)]);
        let hit = idx.find("create_entity").unwrap();
        assert_eq!(hit.kind, "definition");
        assert_eq!(hit.line, 5);
        assert_eq!(hit.modes, Some(vec!["General".to_string(), "Simulation".to_string()]));
        assert_eq!(idx.find("particle_simulation").unwrap().kind, "const");
        assert!(idx.find("ghost_tool").is_none(), "comments do not count");
        assert!(idx.find("test_only_tool").is_none(), "test modules do not count");
        assert_eq!(idx.declared_names(), vec![("create_entity".to_string(), "eustress/crates/tools/src/entity_tools.rs:5".to_string())]);
    }
}
