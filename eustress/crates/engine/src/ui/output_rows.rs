//! The Output panel's rows (ui/slint/output.slint).
//!
//! `build` turns OutputConsole's entries into the rows the panel lists: the
//! level, source and search filters applied together, identical lines in a
//! row collapsed into one with a count, Play's start and stop turned into
//! divider rows, and each line's script, file, line and stack carried for the
//! row's link, its jump and its stack. It also counts every level before the
//! filters, for the toggles.
//!
//! Which rows show their stack is kept here (`toggle_expanded`), keyed by the
//! entry's id, which survives the buffer trimming from the front.
//!
//! Pure data, no Slint types: `sync_bevy_to_slint` converts `RowData` to the
//! generated `OutputRow`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::slint_ui::{LogEntry, LogLevel};

/// One row, before it becomes a Slint `OutputRow`.
#[derive(Clone, Debug, PartialEq)]
pub struct RowData {
    pub id: u64,
    /// "line" or "divider".
    pub kind: &'static str,
    /// 0 info, 1 warning, 2 error, 3 debug.
    pub level: i32,
    pub source: String,
    pub badge: String,
    pub time: String,
    pub text: String,
    pub count: i32,
    pub script: String,
    pub script_label: String,
    pub file: String,
    pub line: i32,
    pub stack: String,
    pub expanded: bool,
}

/// The panel's filters, read from the toolbar.
pub struct Filters<'a> {
    /// "", "all", "luau", "rune", "system" (Engine) or "bliss".
    pub source: &'a str,
    pub search: &'a str,
    /// Shown levels: info, warnings, errors, debug.
    pub levels: [bool; 4],
}

pub struct Built {
    pub rows: Vec<RowData>,
    /// Lines of each level before any filter: info, warnings, errors, debug.
    pub counts: [i32; 4],
}

pub fn level_index(level: LogLevel) -> usize {
    match level {
        LogLevel::Info => 0,
        LogLevel::Warn => 1,
        LogLevel::Error => 2,
        LogLevel::Debug => 3,
    }
}

/// The source a line is filtered and badged by. Bliss writes its lines as
/// "system"; they read as their own source.
pub fn display_source(entry: &LogEntry) -> &str {
    if entry.source == "system" && entry.message.trim_start().starts_with("Bliss") {
        "bliss"
    } else {
        entry.source.as_str()
    }
}

/// The badge's word for a source.
pub fn badge(source: &str) -> String {
    match source {
        "luau" => "Luau".into(),
        "rune" => "Rune".into(),
        "bliss" => "Bliss".into(),
        "system" | "" => "Engine".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Engine".into(),
            }
        }
    }
}

/// The message without a leading "[script]" (drain_output's format) or
/// "script:line:" (a Luau error's, as Roblox writes it) that names the line's
/// own script, since the row shows the script and line as its link.
pub fn strip_script_prefix<'a>(message: &'a str, script: &str) -> &'a str {
    if script.is_empty() {
        return message;
    }
    let mut text = message;
    if let Some(after) = text.strip_prefix('[').and_then(|r| r.strip_prefix(script)).and_then(|r| r.strip_prefix(']')) {
        text = after.trim_start();
    }
    if let Some(after) = text.strip_prefix(script).and_then(|r| r.strip_prefix(':')) {
        let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            if let Some(rest) = after[digits..].strip_prefix(':') {
                text = rest.trim_start();
            }
        }
    }
    text
}

/// The link's short form of an instance path: its last name.
pub fn short_label(script: &str) -> &str {
    script.rsplit('.').next().unwrap_or(script)
}

/// A Play line as a divider's words: "▶ Play started" is "Play started", and
/// "■ Play stopped; the Space is back…" is "Play stopped".
pub fn divider_text(message: &str) -> String {
    let words = message.trim_start_matches(|c: char| !c.is_alphanumeric());
    let words = words.split(';').next().unwrap_or(words);
    words.trim().to_string()
}

fn matches_search(needle_lower: &str, parts: &[&str]) -> bool {
    needle_lower.is_empty() || parts.iter().any(|p| p.to_lowercase().contains(needle_lower))
}

/// The rows for the panel. See the module comment.
pub fn build(entries: &[LogEntry], filters: &Filters, expanded: &HashSet<u64>) -> Built {
    let mut counts = [0i32; 4];
    let mut rows: Vec<RowData> = Vec::new();
    let needle = filters.search.trim().to_lowercase();

    for entry in entries {
        let level = level_index(entry.level);
        counts[level] += 1;

        // Play's start, pause, resume and stop: a divider, shown whatever
        // the level and source filters are, so a session keeps its frame.
        if entry.source == "play" {
            let text = divider_text(&entry.message);
            if matches_search(&needle, &[&text]) {
                rows.push(RowData {
                    id: entry.id,
                    kind: "divider",
                    level: level as i32,
                    source: "play".into(),
                    badge: String::new(),
                    time: entry.timestamp.clone(),
                    text,
                    count: 1,
                    script: String::new(),
                    script_label: String::new(),
                    file: String::new(),
                    line: 0,
                    stack: String::new(),
                    expanded: false,
                });
            }
            continue;
        }

        if !filters.levels[level] {
            continue;
        }
        let source = display_source(entry);
        let source_ok = match filters.source {
            "" | "all" => true,
            wanted => source == wanted,
        };
        if !source_ok {
            continue;
        }
        let text = strip_script_prefix(&entry.message, &entry.script);
        if !matches_search(&needle, &[text, &entry.script, &entry.file]) {
            continue;
        }
        let stack = entry.stack.join("\n");

        // The same line again, straight after itself: count it on the row.
        if let Some(last) = rows.last_mut() {
            if last.kind == "line"
                && last.level == level as i32
                && last.source == source
                && last.text == text
                && last.script == entry.script
                && last.file == entry.file
                && last.line == entry.line as i32
                && last.stack == stack
            {
                last.count += 1;
                last.time = entry.timestamp.clone();
                continue;
            }
        }

        rows.push(RowData {
            id: entry.id,
            kind: "line",
            level: level as i32,
            source: source.to_string(),
            badge: badge(source),
            time: entry.timestamp.clone(),
            text: text.to_string(),
            count: 1,
            script: entry.script.clone(),
            script_label: short_label(&entry.script).to_string(),
            file: entry.file.clone(),
            line: i32::try_from(entry.line).unwrap_or(0),
            expanded: !stack.is_empty() && expanded.contains(&entry.id),
            stack,
        });
    }

    Built { rows, counts }
}

/// One entry as copied text: time, level, source, script and line, the
/// message, and its stack indented under it.
pub fn copy_text(entry: &LogEntry) -> String {
    let level = match entry.level {
        LogLevel::Info => "Info",
        LogLevel::Warn => "Warning",
        LogLevel::Error => "Error",
        LogLevel::Debug => "Debug",
    };
    let source = badge(display_source(entry));
    let text = strip_script_prefix(&entry.message, &entry.script);
    let mut out = format!("{} {} [{}]", entry.timestamp, level, source);
    if !entry.script.is_empty() {
        out.push(' ');
        out.push_str(&entry.script);
        if entry.line > 0 {
            out.push_str(&format!(":{}", entry.line));
        }
    }
    out.push_str("  ");
    out.push_str(text);
    for frame in &entry.stack {
        out.push_str("\n    ");
        out.push_str(frame);
    }
    out
}

/// Every row the filters show, as copied text (a collapsed row once, with
/// its count).
pub fn visible_text(entries: &[LogEntry], filters: &Filters) -> String {
    let built = build(entries, filters, &HashSet::new());
    let by_id: std::collections::HashMap<u64, &LogEntry> = entries.iter().map(|e| (e.id, e)).collect();
    let mut out = String::new();
    for row in &built.rows {
        if !out.is_empty() {
            out.push('\n');
        }
        if row.kind == "divider" {
            out.push_str(&format!("── {} {} ──", row.text, row.time));
            continue;
        }
        if let Some(entry) = by_id.get(&row.id) {
            out.push_str(&copy_text(entry));
            if row.count > 1 {
                out.push_str(&format!("  (×{})", row.count));
            }
        }
    }
    out
}

static EXPANDED: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
static EXPAND_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Show or hide a row's stack.
pub fn toggle_expanded(id: u64) {
    let mut guard = EXPANDED.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if !set.remove(&id) {
        set.insert(id);
    }
    EXPAND_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// The rows showing their stack.
pub fn expanded_ids() -> HashSet<u64> {
    EXPANDED.lock().unwrap_or_else(|p| p.into_inner()).clone().unwrap_or_default()
}

/// Moves whenever a stack is shown or hidden, so the panel rebuilds.
pub fn expand_epoch() -> u64 {
    EXPAND_EPOCH.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, level: LogLevel, source: &str, message: &str, script: &str) -> LogEntry {
        LogEntry {
            id,
            level,
            message: message.to_string(),
            timestamp: format!("15:02:{:02}", id),
            source: source.to_string(),
            script: script.to_string(),
            file: String::new(),
            line: 0,
            stack: Vec::new(),
        }
    }

    fn all() -> Filters<'static> {
        Filters { source: "all", search: "", levels: [true, true, true, false] }
    }

    #[test]
    fn repeats_collapse_into_one_row_with_a_count() {
        let entries: Vec<LogEntry> = (1..=3)
            .map(|i| entry(i, LogLevel::Info, "luau", "[Workspace.Spawner] spawned zombie", "Workspace.Spawner"))
            .collect();
        let built = build(&entries, &all(), &HashSet::new());
        assert_eq!(built.rows.len(), 1);
        assert_eq!(built.rows[0].count, 3);
        assert_eq!(built.rows[0].text, "spawned zombie");
        assert_eq!(built.rows[0].script_label, "Spawner");
        assert_eq!(built.rows[0].id, 1, "a collapsed row keeps its first id");
        assert_eq!(built.rows[0].time, "15:02:03", "and shows the latest time");
        assert_eq!(built.counts[0], 3, "counts count every line");
    }

    #[test]
    fn a_different_line_between_repeats_breaks_the_run() {
        let entries = vec![
            entry(1, LogLevel::Info, "luau", "a", ""),
            entry(2, LogLevel::Info, "luau", "b", ""),
            entry(3, LogLevel::Info, "luau", "a", ""),
        ];
        assert_eq!(build(&entries, &all(), &HashSet::new()).rows.len(), 3);
    }

    #[test]
    fn filters_work_together() {
        let entries = vec![
            entry(1, LogLevel::Error, "luau", "boom", "Workspace.A"),
            entry(2, LogLevel::Error, "rune", "boom", "Workspace.B"),
            entry(3, LogLevel::Info, "luau", "boom", "Workspace.C"),
            entry(4, LogLevel::Error, "luau", "fine", "Workspace.D"),
        ];
        let f = Filters { source: "luau", search: "BOOM", levels: [false, true, true, false] };
        let built = build(&entries, &f, &HashSet::new());
        assert_eq!(built.rows.len(), 1);
        assert_eq!(built.rows[0].id, 1);
        assert_eq!(built.counts, [1, 0, 3, 0]);
    }

    #[test]
    fn search_matches_the_script_path_too() {
        let entries = vec![entry(1, LogLevel::Info, "luau", "hello", "Workspace.NavGrid")];
        let f = Filters { source: "all", search: "navgrid", levels: [true; 4] };
        assert_eq!(build(&entries, &f, &HashSet::new()).rows.len(), 1);
    }

    #[test]
    fn play_lines_are_dividers_whatever_the_filters() {
        let entries = vec![
            entry(1, LogLevel::Info, "play", "▶ Play started", ""),
            entry(2, LogLevel::Info, "play", "■ Play stopped; the Space is back to its state before Play", ""),
        ];
        let f = Filters { source: "rune", search: "", levels: [false; 4] };
        let rows = build(&entries, &f, &HashSet::new()).rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "divider");
        assert_eq!(rows[0].text, "Play started");
        assert_eq!(rows[1].text, "Play stopped");
    }

    #[test]
    fn bliss_lines_read_as_their_own_source() {
        let entries = vec![
            entry(1, LogLevel::Info, "system", "Bliss: node online", ""),
            entry(2, LogLevel::Info, "system", "Opened SoulScript", ""),
        ];
        let rows = build(&entries, &all(), &HashSet::new()).rows;
        assert_eq!(rows[0].badge, "Bliss");
        assert_eq!(rows[1].badge, "Engine");
        let only_bliss = Filters { source: "bliss", search: "", levels: [true; 4] };
        assert_eq!(build(&entries, &only_bliss, &HashSet::new()).rows.len(), 1);
    }

    #[test]
    fn a_stack_shows_only_when_expanded() {
        let mut e = entry(7, LogLevel::Error, "luau", "nil index", "Workspace.A");
        e.stack = vec!["A.luau:88".into(), "B.luau:51".into()];
        e.file = "C:/x/A.luau".into();
        e.line = 88;
        let entries = vec![e];
        let closed = build(&entries, &all(), &HashSet::new()).rows;
        assert!(!closed[0].expanded);
        assert_eq!(closed[0].stack, "A.luau:88\nB.luau:51");
        assert_eq!(closed[0].line, 88);
        let open = build(&entries, &all(), &HashSet::from([7])).rows;
        assert!(open[0].expanded);
        let text = copy_text(&entries[0]);
        assert_eq!(text, "15:02:07 Error [Luau] Workspace.A:88  nil index\n    A.luau:88\n    B.luau:51");
    }

    #[test]
    fn a_prefix_naming_another_script_stays() {
        assert_eq!(strip_script_prefix("[Workspace.B] hi", "Workspace.A"), "[Workspace.B] hi");
        assert_eq!(strip_script_prefix("[Workspace.A] hi", "Workspace.A"), "hi");
        assert_eq!(strip_script_prefix("[Workspace.A] hi", ""), "[Workspace.A] hi");
    }

    #[test]
    fn a_luau_error_loses_its_own_path_and_line() {
        let s = "ServerScriptService.AbilityDirector";
        assert_eq!(strip_script_prefix("ServerScriptService.AbilityDirector:88: attempt to index nil", s), "attempt to index nil");
        assert_eq!(strip_script_prefix("[ServerScriptService.AbilityDirector] ServerScriptService.AbilityDirector:88: boom", s), "boom");
        // Not a line number: kept.
        assert_eq!(strip_script_prefix("ServerScriptService.AbilityDirector:ready", s), "ServerScriptService.AbilityDirector:ready");
    }
}
