//! # Find in Scripts
//!
//! The search behind the Ctrl+Shift+F modal: which lines of which scripts hold
//! a piece of text, and the small state machine the modal keeps between frames.
//! Pure Rust over text, with no Bevy and no UI, so every rule is tested without
//! a window.
//!
//! The caller gathers the scripts (the engine's script components, plus the
//! text of any script open in a tab) into [`ScriptText`]s; [`search`] returns at
//! most [`MAX_HITS`] [`ScriptHit`]s, in script order and then line order, one
//! per matching line. The match is a substring, compared one character at a
//! time, so no byte offset ever lands inside a character. A hit also says where
//! its match sits in the script's own line, in characters, and [`selection_for`]
//! turns that into the byte range the editor selects.

use std::collections::HashSet;
use std::path::Path;

/// Most matches one search returns. A broader query stops here and says so.
pub const MAX_HITS: usize = 500;

/// Most characters of a line a hit keeps. A longer line is cut around the match
/// and the cut ends are marked with an ellipsis.
pub const MAX_TEXT_CHARS: usize = 200;

/// How many characters a cut line keeps in front of the match.
const CONTEXT_BEFORE: usize = 40;

/// What a script's location says when it has no file.
pub const IN_DATABASE: &str = "Space database";

/// Most characters of a script's name or location a row keeps.
pub const MAX_LABEL_CHARS: usize = 200;

/// Most characters of a row's line, or of one part of it, that a row keeps: the
/// line's own [`MAX_TEXT_CHARS`] and the ellipsis at each end.
pub const MAX_ROW_TEXT_CHARS: usize = MAX_TEXT_CHARS + 2;

/// One script to search. `id` is the caller's key for it (an entity, say),
/// handed back on each of its hits.
#[derive(Debug, Clone, Copy)]
pub struct ScriptText<'a> {
    pub id: u64,
    pub name: &'a str,
    pub location: &'a str,
    pub source: &'a str,
}

/// One matching line. `text` is the line as a row shows it (trimmed, at most
/// about [`MAX_TEXT_CHARS`] characters); `match_start` and `match_len` are the
/// match's byte span inside `text`, always on character boundaries. `col` and
/// `chars` place the match in the script's own line, in characters, which is
/// what [`selection_for`] turns into the editor's byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptHit {
    pub id: u64,
    pub name: String,
    pub location: String,
    /// 1-based line number in the script's source.
    pub line: usize,
    pub text: String,
    pub match_start: usize,
    pub match_len: usize,
    /// Characters from the start of the script's line, before any trimming, to
    /// the match.
    pub col: usize,
    /// Characters in the match.
    pub chars: usize,
}

/// The result of one search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchOutcome {
    pub hits: Vec<ScriptHit>,
    /// More lines matched than [`MAX_HITS`]; the rest were not collected.
    pub truncated: bool,
}

/// Search `scripts` for `query`. A blank query finds nothing. Matching is a
/// substring test; without `match_case` it ignores case, comparing the
/// lowercase of each character. Lines split at `\n` and lose one trailing `\r`.
/// A line is reported once, at its first match.
pub fn search(scripts: &[ScriptText], query: &str, match_case: bool) -> SearchOutcome {
    let mut outcome = SearchOutcome::default();
    if query.trim().is_empty() {
        return outcome;
    }
    let needle: Vec<char> = query.chars().collect();
    for script in scripts {
        for (index, raw) in script.source.split('\n').enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let Some((start, len)) = find_in_line(line, &needle, match_case) else {
                continue;
            };
            if outcome.hits.len() == MAX_HITS {
                outcome.truncated = true;
                return outcome;
            }
            let (text, match_start, match_len) = display_line(line, start, len);
            outcome.hits.push(ScriptHit {
                id: script.id,
                name: script.name.to_string(),
                location: script.location.to_string(),
                line: index + 1,
                text,
                match_start,
                match_len,
                col: line[..start].chars().count(),
                chars: line[start..start + len].chars().count(),
            });
        }
    }
    outcome
}

/// Whether two characters are the same for the search.
fn same_char(a: char, b: char, match_case: bool) -> bool {
    if a == b {
        return true;
    }
    if match_case {
        return false;
    }
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}

/// The first match of `needle` in `line`, as a byte start and a byte length.
fn find_in_line(line: &str, needle: &[char], match_case: bool) -> Option<(usize, usize)> {
    let first = *needle.first()?;
    for (start, c) in line.char_indices() {
        if !same_char(c, first, match_case) {
            continue;
        }
        let mut rest = line[start..].chars();
        let all_match = needle.iter().all(|&n| rest.next().is_some_and(|c| same_char(c, n, match_case)));
        if all_match {
            let end = line.len() - rest.as_str().len();
            return Some((start, end - start));
        }
    }
    None
}

/// Byte offset of the `n`th character of `s`, or its end.
fn byte_of_char(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// The line as a row shows it, with the match's byte span inside it: trimmed,
/// and when still longer than [`MAX_TEXT_CHARS`], a window of that many
/// characters that keeps the match, its cut ends marked with an ellipsis.
fn display_line(line: &str, start: usize, len: usize) -> (String, usize, usize) {
    let lead = line.len() - line.trim_start().len();
    let body = line.trim();
    // The match holds a character that is not blank, so the trim leaves some of it.
    let from = start.max(lead) - lead;
    let to = (start + len).min(lead + body.len()).saturating_sub(lead).max(from);
    let total = body.chars().count();
    if total <= MAX_TEXT_CHARS {
        return (body.to_string(), from, to - from);
    }
    let match_char = body[..from].chars().count();
    let first = match_char.saturating_sub(CONTEXT_BEFORE).min(total - MAX_TEXT_CHARS);
    let last = first + MAX_TEXT_CHARS;
    let (byte_first, byte_last) = (byte_of_char(body, first), byte_of_char(body, last));
    let mut text = String::new();
    if first > 0 {
        text.push('…');
    }
    let lead_len = text.len();
    text.push_str(&body[byte_first..byte_last]);
    if last < total {
        text.push('…');
    }
    let match_start = lead_len + from - byte_first;
    let match_len = to.min(byte_last).saturating_sub(from);
    (text, match_start, match_len)
}

/// How a script's location reads: its path inside the Space when it has one,
/// else [`IN_DATABASE`]. A binary core's synthetic `__bin_` path names no file,
/// so it reads as the database too.
pub fn location_label(path: Option<&Path>, space_root: Option<&Path>) -> String {
    let Some(path) = path else {
        return IN_DATABASE.to_string();
    };
    if path.components().any(|c| c.as_os_str().to_string_lossy().starts_with("__bin_")) {
        return IN_DATABASE.to_string();
    }
    let shown = space_root.and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path);
    shown.to_string_lossy().replace('\\', "/")
}

/// A hit's text cut at its match into the part before it, the match and the
/// part after. Slint cannot cut a string by offset, so the window gets the
/// three parts. A span that is not on character boundaries gives the whole
/// text as the first part.
pub fn split_for_display(text: &str, match_start: usize, match_len: usize) -> (&str, &str, &str) {
    let end = match_start.saturating_add(match_len).min(text.len());
    match (text.get(..match_start), text.get(match_start..end), text.get(end..)) {
        (Some(before), Some(hit), Some(after)) => (before, hit, after),
        _ => (text, "", ""),
    }
}

/// `s` with every blank made a no-break space. A text layout may drop the
/// blanks at the end of a part when it measures it, which would draw the next
/// part against it; a no-break space keeps its width.
pub fn keep_blanks(s: &str) -> String {
    s.replace(' ', "\u{a0}")
}

/// What a row draws for a string that came from outside (a script's name or
/// path, a line of its text): one line of at most `max_chars` characters. Each
/// line break and control character becomes a blank, and a longer string is cut
/// and ends with an ellipsis, so the result is never longer than `max_chars`
/// characters. Only the first `max_chars + 1` characters are read, so a huge
/// string costs no more than a short one. Running it twice changes nothing.
pub fn row_text(s: &str, max_chars: usize) -> String {
    let max = max_chars.max(1);
    let mut kept: Vec<char> = s.chars().take(max + 1).map(blank_if_break).collect();
    if kept.len() > max {
        kept.truncate(max - 1);
        kept.push('…');
    }
    kept.into_iter().collect()
}

/// A line break or control character as a blank; any other character as is.
fn blank_if_break(c: char) -> char {
    if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
        ' '
    } else {
        c
    }
}

/// The byte range to select in `buffer` for a match: `chars` characters,
/// starting `col` characters into the 1-based `line`. A hit counts in
/// characters and the editor's offsets are bytes, so this walks the line by
/// character; a line with multi-byte characters before the match still selects
/// the match. Where there is nothing to select (an empty match, or a line now
/// too short to hold the match's start because the script changed since the
/// search) the range is empty at the start of the line; a line past the end of
/// the buffer gives the buffer's end. A match that now runs past the end of its
/// line is cut there, before the line break.
pub fn selection_for(buffer: &str, line: usize, col: usize, chars: usize) -> (usize, usize) {
    let mut at = 0;
    for _ in 1..line.max(1) {
        match buffer[at..].find('\n') {
            Some(i) => at += i + 1,
            None => return (buffer.len(), buffer.len()),
        }
    }
    let rest = &buffer[at..];
    let row = rest.find('\n').map_or(rest, |i| &rest[..i]);
    let row = row.strip_suffix('\r').unwrap_or(row);
    match select_in_line(row, col, chars) {
        Some((from, to)) => (at + from, at + to),
        None => (at, at),
    }
}

/// The byte range of `chars` characters starting `col` characters into `row`,
/// or `None` for an empty match or a start past the end of the row.
fn select_in_line(row: &str, col: usize, chars: usize) -> Option<(usize, usize)> {
    if chars == 0 {
        return None;
    }
    let from = row.char_indices().nth(col)?.0;
    let to = row[from..].char_indices().nth(chars).map_or(row.len(), |(i, _)| from + i);
    Some((from, to))
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// What the modal keeps between frames: the query and its two switches, the
/// matches of the last search, the highlighted row, and the requests the
/// window made that the engine answers next frame. The `*_changed` flags say
/// what the window's copy is missing.
#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub match_case: bool,
    pub hits: Vec<ScriptHit>,
    pub truncated: bool,
    /// How many scripts the last search read.
    pub scripts_searched: usize,
    /// The highlighted row of `hits`.
    pub selected: usize,
    /// The query, the case switch or the scripts changed: search again.
    pub stale: bool,
    /// The window's list is not `hits`.
    pub list_changed: bool,
    /// The window's highlighted row is not `selected`.
    pub selection_changed: bool,
    /// The match to open, chosen by a click or Enter. Kept as the match itself,
    /// so a search that lands first cannot change which script it means.
    pub open: Option<ScriptHit>,
    /// The match to select once the opened script's text is in the editor.
    pub jump: Option<Jump>,
}

impl SearchState {
    /// The query was typed.
    pub fn type_query(&mut self, query: String) {
        if self.query != query {
            self.query = query;
            self.stale = true;
        }
    }

    /// The Match case box was set.
    pub fn set_match_case(&mut self, on: bool) {
        if self.match_case != on {
            self.match_case = on;
            self.stale = true;
        }
    }

    /// The modal opened: the scripts may have changed since the last search.
    pub fn reopen(&mut self) {
        self.stale = true;
        self.list_changed = true;
        self.selection_changed = true;
    }

    /// The modal closed: let go of the matches. The query stays for next time.
    pub fn close(&mut self) {
        self.hits = Vec::new();
        self.truncated = false;
        self.scripts_searched = 0;
        self.selected = 0;
        self.open = None;
        self.stale = false;
    }

    /// Take the result of a search that read `scripts_searched` scripts.
    pub fn finish(&mut self, outcome: SearchOutcome, scripts_searched: usize) {
        self.hits = outcome.hits;
        self.truncated = outcome.truncated;
        self.scripts_searched = scripts_searched;
        self.selected = 0;
        self.stale = false;
        self.list_changed = true;
        self.selection_changed = true;
    }

    /// Up, Down, Page Up or Page Down: move the highlight, staying on the list.
    pub fn move_selection(&mut self, delta: i32) {
        let Some(last) = self.hits.len().checked_sub(1) else {
            return;
        };
        let next = (self.selected as i64 + i64::from(delta)).clamp(0, last as i64) as usize;
        if next != self.selected {
            self.selected = next;
            self.selection_changed = true;
        }
    }

    /// A click or Enter on `row`. Returns false when no such row exists.
    pub fn request_open(&mut self, row: i32) -> bool {
        let Some(hit) = usize::try_from(row).ok().and_then(|row| self.hits.get(row)) else {
            return false;
        };
        self.open = Some(hit.clone());
        self.selected = row as usize;
        self.selection_changed = true;
        true
    }

    /// The line under the title: how many matches in how many scripts, or that
    /// the list was cut. Empty until a query is typed.
    pub fn status(&self) -> String {
        if self.query.trim().is_empty() {
            return String::new();
        }
        if self.truncated {
            return format!("{MAX_HITS} shown; narrow the search to see the rest");
        }
        let n = self.hits.len();
        if n == 0 {
            return match self.scripts_searched {
                0 => "This Space has no scripts".to_string(),
                s => format!("No matches in {s} {}", plural(s, "script")),
            };
        }
        let scripts = self.hits.iter().map(|h| h.id).collect::<HashSet<_>>().len();
        format!("{n} {} in {scripts} {}", plural(n, "result"), plural(scripts, "script"))
    }
}

/// Frames the opened tab must stay settled before the match is selected, so the
/// editor has been laid out with the new text.
pub const JUMP_READY_FRAMES: u32 = 2;

/// Frames to wait for the opened tab before giving the jump up.
pub const JUMP_GIVE_UP_FRAMES: u32 = 300;

/// What the next frame does with a pending [`Jump`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpStep {
    /// The tab is not showing its text yet.
    Wait,
    /// Select the match and set the scroll position now.
    Apply,
    /// The tab is gone or never settled; forget the jump.
    Drop,
}

/// A selection waiting for the opened script's tab to show its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jump {
    /// The id of the tab the script opened in.
    pub tab: u32,
    /// 1-based line holding the match.
    pub line: usize,
    /// Where the match sits in that line, in characters (`ScriptHit::col` and
    /// `ScriptHit::chars`); [`selection_for`] makes bytes of them.
    pub col: usize,
    pub chars: usize,
    ready: u32,
    waited: u32,
}

impl Jump {
    pub fn new(tab: u32, line: usize, col: usize, chars: usize) -> Self {
        Self {
            tab,
            line,
            col,
            chars,
            ready: 0,
            waited: 0,
        }
    }

    /// One frame. `tab_is_active`: the tab is still the one shown. `settled`:
    /// the tab's text is in the editor and no tab switch is in flight.
    pub fn step(&mut self, tab_is_active: bool, settled: bool) -> JumpStep {
        if !tab_is_active {
            return JumpStep::Drop;
        }
        self.waited += 1;
        if self.waited > JUMP_GIVE_UP_FRAMES {
            return JumpStep::Drop;
        }
        if !settled {
            self.ready = 0;
            return JumpStep::Wait;
        }
        self.ready += 1;
        if self.ready >= JUMP_READY_FRAMES {
            JumpStep::Apply
        } else {
            JumpStep::Wait
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script<'a>(id: u64, name: &'a str, source: &'a str) -> ScriptText<'a> {
        ScriptText {
            id,
            name,
            location: "SoulService/x.rune",
            source,
        }
    }

    fn hits(source: &str, query: &str, match_case: bool) -> Vec<ScriptHit> {
        search(&[script(1, "x", source)], query, match_case).hits
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        for query in ["", " ", "   ", "\t", " \n "] {
            let outcome = search(&[script(1, "x", "spawn\nspawn")], query, false);
            assert!(outcome.hits.is_empty(), "{query:?} found {:?}", outcome.hits);
            assert!(!outcome.truncated);
        }
    }

    #[test]
    fn case_insensitive_spawn_finds_spawn_and_match_case_does_not() {
        let source = "local Spawn = 1";
        let found = hits(source, "spawn", false);
        assert_eq!(found.len(), 1);
        assert_eq!(&found[0].text[found[0].match_start..found[0].match_start + found[0].match_len], "Spawn");
        assert!(hits(source, "spawn", true).is_empty());
        assert_eq!(hits(source, "Spawn", true).len(), 1);
    }

    #[test]
    fn line_numbers_are_one_based_and_crlf_loses_its_cr() {
        let found = hits("first\r\nsecond spawn\r\nthird\r\n", "spawn", false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].text, "second spawn");
        assert!(!found[0].text.contains('\r'));
        assert_eq!(found[0].match_start + found[0].match_len, found[0].text.len());
        // The first line is line 1.
        assert_eq!(hits("spawn\nx", "spawn", false)[0].line, 1);
    }

    #[test]
    fn non_ascii_text_before_the_match_slices_cleanly() {
        let found = hits("café spawn", "SPAWN", false);
        assert_eq!(found.len(), 1);
        let hit = &found[0];
        assert_eq!(hit.match_start, "café ".len());
        assert_eq!(&hit.text[hit.match_start..hit.match_start + hit.match_len], "spawn");
        // Multi-byte characters in the query, in both cases.
        assert_eq!(hits("École", "école", false).len(), 1);
        assert!(hits("École", "école", true).is_empty());
        assert_eq!(hits("日本語 spawn 日本語", "日本語", true)[0].match_start, 0);
        // A character whose lowercase is two characters does not panic.
        assert!(hits("İstanbul", "i", false).len() <= 1);
        assert_eq!(hits("İstanbul", "stan", false)[0].match_start, "İ".len());
    }

    #[test]
    fn more_than_the_limit_is_cut_and_says_so() {
        let source = "spawn\n".repeat(MAX_HITS + 1);
        let outcome = search(&[script(1, "x", &source)], "spawn", false);
        assert_eq!(outcome.hits.len(), MAX_HITS);
        assert!(outcome.truncated);
        assert_eq!(outcome.hits.last().map(|h| h.line), Some(MAX_HITS));
        // Exactly the limit is not a cut.
        let exact = "spawn\n".repeat(MAX_HITS);
        let outcome = search(&[script(1, "x", &exact)], "spawn", false);
        assert_eq!(outcome.hits.len(), MAX_HITS);
        assert!(!outcome.truncated);
    }

    #[test]
    fn the_limit_counts_across_scripts() {
        let a = "spawn\n".repeat(300);
        let b = "spawn\n".repeat(300);
        let outcome = search(&[script(1, "a", &a), script(2, "b", &b)], "spawn", false);
        assert_eq!(outcome.hits.len(), MAX_HITS);
        assert!(outcome.truncated);
        assert_eq!(outcome.hits.iter().filter(|h| h.id == 2).count(), 200);
    }

    #[test]
    fn a_line_is_reported_once_at_its_first_match() {
        let found = hits("spawn spawn spawn", "spawn", false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].match_start, 0);
    }

    #[test]
    fn hits_keep_script_order_and_carry_their_script() {
        let a = ScriptText {
            id: 7,
            name: "A",
            location: "SoulService/A.rune",
            source: "x\nspawn",
        };
        let b = ScriptText {
            id: 9,
            name: "B",
            location: IN_DATABASE,
            source: "spawn\nspawn",
        };
        let found = search(&[a, b], "spawn", false).hits;
        let order: Vec<(u64, usize)> = found.iter().map(|h| (h.id, h.line)).collect();
        assert_eq!(order, vec![(7, 2), (9, 1), (9, 2)]);
        assert_eq!((found[0].name.as_str(), found[0].location.as_str()), ("A", "SoulService/A.rune"));
        assert_eq!(found[1].location, IN_DATABASE);
    }

    #[test]
    fn the_text_is_trimmed_and_the_span_follows_the_trim() {
        let found = hits("\t\t  local spawn = 1  \r", "spawn", false);
        assert_eq!(found[0].text, "local spawn = 1");
        assert_eq!(&found[0].text[found[0].match_start..found[0].match_start + found[0].match_len], "spawn");
        // A query that begins in the leading blanks keeps the part that survives the trim.
        let found = hits("    x = 1", "  x", false);
        assert_eq!(found[0].text, "x = 1");
        assert_eq!((found[0].match_start, found[0].match_len), (0, 1));
    }

    #[test]
    fn a_long_line_is_cut_around_the_match() {
        let line = format!("{}spawn{}", "a".repeat(300), "b".repeat(300));
        let found = hits(&line, "SPAWN", false);
        let hit = &found[0];
        assert!(hit.text.starts_with('…') && hit.text.ends_with('…'));
        assert_eq!(hit.text.chars().count(), MAX_TEXT_CHARS + 2);
        assert_eq!(&hit.text[hit.match_start..hit.match_start + hit.match_len], "spawn");
        // A match near the end keeps the window full and cuts only the front.
        let tail = format!("{}spawn", "a".repeat(400));
        let hit = &hits(&tail, "spawn", false)[0];
        assert!(hit.text.starts_with('…') && !hit.text.ends_with('…'));
        assert_eq!(hit.text.chars().count(), MAX_TEXT_CHARS + 1);
        assert_eq!(&hit.text[hit.match_start..hit.match_start + hit.match_len], "spawn");
        // A match near the start cuts only the end.
        let head = format!("spawn{}", "b".repeat(400));
        let hit = &hits(&head, "spawn", false)[0];
        assert!(!hit.text.starts_with('…') && hit.text.ends_with('…'));
        assert_eq!((hit.match_start, hit.match_len), (0, 5));
        // Multi-byte characters in the cut window.
        let wide = format!("{}spawn{}", "é".repeat(300), "日".repeat(300));
        let hit = &hits(&wide, "spawn", false)[0];
        assert_eq!(&hit.text[hit.match_start..hit.match_start + hit.match_len], "spawn");
    }

    #[test]
    fn a_match_longer_than_the_window_is_clipped_to_it() {
        let query = "q".repeat(300);
        let line = format!("{}{}", "z".repeat(10), query);
        let hit = &hits(&line, &query, true)[0];
        assert!(hit.match_start + hit.match_len <= hit.text.len());
        assert!(hit.text.is_char_boundary(hit.match_start + hit.match_len));
    }

    #[test]
    fn a_hit_splits_into_before_the_match_and_after() {
        let hit = &hits("café SPAWN() end", "spawn", false)[0];
        let (before, mid, after) = split_for_display(&hit.text, hit.match_start, hit.match_len);
        assert_eq!((before, mid, after), ("café ", "SPAWN", "() end"));
        // The three parts are the whole text.
        assert_eq!(format!("{before}{mid}{after}"), hit.text);
        // A span that is not on character boundaries, or past the end, keeps the text whole.
        assert_eq!(split_for_display("é", 1, 1), ("é", "", ""));
        assert_eq!(split_for_display("abc", 9, 2), ("abc", "", ""));
        // A span that runs past the end is cut to it.
        assert_eq!(split_for_display("abc", 1, 99), ("a", "bc", ""));
        assert_eq!(split_for_display("", 0, 0), ("", "", ""));
    }

    #[test]
    fn blanks_keep_their_width_as_no_break_spaces() {
        assert_eq!(keep_blanks("a b  c"), "a\u{a0}b\u{a0}\u{a0}c");
        assert_eq!(keep_blanks("no_blanks"), "no_blanks");
        assert_eq!(keep_blanks(" "), "\u{a0}");
    }

    #[test]
    fn the_query_is_searched_as_typed() {
        // Inner and edge blanks are part of the query.
        assert_eq!(hits("local x", "al x", false).len(), 1);
        assert!(hits("localx", "al x", false).is_empty());
        assert_eq!(hits("foo bar", "foo ", false).len(), 1);
        assert!(hits("foobar", "foo ", false).is_empty());
        // A query with a newline can match no line.
        assert!(hits("a\nb", "a\nb", false).is_empty());
    }

    #[test]
    fn a_source_with_no_match_or_no_text_gives_nothing() {
        assert!(hits("", "spawn", false).is_empty());
        assert!(hits("no such word", "spawn", false).is_empty());
        assert!(search(&[], "spawn", false).hits.is_empty());
    }

    #[test]
    fn locations_read_as_a_path_in_the_space_or_the_database() {
        let root = Path::new("C:/Spaces/Demo");
        let file = Path::new("C:/Spaces/Demo/SoulService/Door/Door.rune");
        assert_eq!(location_label(Some(file), Some(root)), "SoulService/Door/Door.rune");
        assert_eq!(location_label(Some(file), None), "C:/Spaces/Demo/SoulService/Door/Door.rune");
        assert_eq!(location_label(Some(Path::new("D:/elsewhere/x.luau")), Some(root)), "D:/elsewhere/x.luau");
        assert_eq!(location_label(None, Some(root)), IN_DATABASE);
        let core = Path::new("C:/Spaces/Demo/Workspace/__bin_Part_00000000000000ff/_instance.toml");
        assert_eq!(location_label(Some(core), Some(root)), IN_DATABASE);
    }

    /// What `selection_for` selects in `buffer` for `hit`.
    fn selected<'a>(buffer: &'a str, hit: &ScriptHit) -> &'a str {
        let (from, to) = selection_for(buffer, hit.line, hit.col, hit.chars);
        &buffer[from..to]
    }

    #[test]
    fn a_hit_places_its_match_in_the_whole_line_in_characters() {
        // Leading blanks are trimmed from the row's text but counted in the column.
        let found = hits("\t\t  local spawn = 1", "spawn", false);
        assert_eq!((found[0].col, found[0].chars), (10, 5));
        assert_eq!(found[0].match_start, 6, "the row's own span starts at its trimmed text");
        // A multi-byte character before the match counts once, not by its bytes.
        let before = "local x = café + ";
        let found = hits("local x = café + spawn(1)", "spawn", false);
        assert_eq!(found[0].col, before.chars().count());
        assert_ne!(before.chars().count(), before.len(), "this line has more bytes than characters before the match");
        // One inside the match counts once too.
        let found = hits("local x = café + spawn(1)", "CAFÉ", false);
        assert_eq!((found[0].col, found[0].chars), (10, 4));
        // A long line is cut for the row, but the place is in the whole line.
        let line = format!("{}spawn{}", "a".repeat(300), "b".repeat(300));
        let found = hits(&line, "SPAWN", false);
        assert_eq!((found[0].col, found[0].chars), (300, 5));
        assert!(found[0].match_start < 300, "the row's text is a window, so its offset is not the line's");
    }

    #[test]
    fn the_selection_is_in_bytes_even_when_the_line_is_not_ascii() {
        let buffer = "-- naïve café script\nlocal x = café + spawn(1)\nprint(x)\n";
        let line_two = "-- naïve café script\n".len();
        let on_line_two = |query: &str| hits(buffer, query, false).into_iter().find(|h| h.line == 2).expect("a match on line 2");

        let hit = on_line_two("spawn");
        assert_eq!(selected(buffer, &hit), "spawn");
        let (from, to) = selection_for(buffer, hit.line, hit.col, hit.chars);
        // "café" is five bytes, so the match starts one byte after its character column.
        assert_eq!(from, line_two + "local x = café + ".len());
        assert_eq!(to - from, "spawn".len());
        assert_eq!(hit.col, "local x = café + ".chars().count());
        assert_ne!(from - line_two, hit.col, "bytes and characters differ on this line");

        // A match with a multi-byte character inside it is selected whole.
        let hit = on_line_two("CAFÉ + S");
        assert_eq!(selected(buffer, &hit), "café + s");
        let (from, to) = selection_for(buffer, hit.line, hit.col, hit.chars);
        assert_eq!(to - from, "café + s".len());
        assert_ne!(hit.chars, to - from, "the match is 8 characters and 9 bytes");
    }

    #[test]
    fn nothing_to_select_falls_back_to_the_line_start() {
        let buffer = "first\nsecond spawn here\nthird\n";
        let line_two = "first\n".len();
        // An empty match selects nothing, at the start of the line and not at its column.
        assert_eq!(selection_for(buffer, 2, 7, 0), (line_two, line_two));
        // A start past the end of the line (the script changed since the search).
        assert_eq!(selection_for(buffer, 2, 99, 5), (line_two, line_two));
        assert_eq!(selection_for(buffer, 2, "second spawn here".chars().count(), 1), (line_two, line_two));
        // A match that now runs past the end of its line is cut there, not carried into the next line.
        let (from, to) = selection_for(buffer, 2, 7, 99);
        assert_eq!(&buffer[from..to], "spawn here");
        // A line past the end of the buffer gives the end; line 0 reads as line 1.
        assert_eq!(selection_for(buffer, 9, 0, 3), (buffer.len(), buffer.len()));
        assert_eq!(selection_for(buffer, 0, 0, 5), (0, 5));
        assert_eq!(selection_for("", 1, 0, 1), (0, 0));
        // A two-byte line break is not part of the line, and the next line starts after both bytes.
        let crlf = "a\r\nb spawn\r\nc\r\n";
        let (from, to) = selection_for(crlf, 2, 2, 99);
        assert_eq!(&crlf[from..to], "spawn");
        let (from, to) = selection_for(crlf, 3, 0, 1);
        assert_eq!(&crlf[from..to], "c");
    }

    #[test]
    fn a_row_string_is_one_bounded_line() {
        assert_eq!(row_text("Door", 10), "Door");
        // Line breaks and control characters become blanks, one for one.
        assert_eq!(row_text("a\tb\r\nc\u{2028}d\u{0}e\u{85}f", 40), "a b  c d e f");
        // Exactly the limit is kept; one more is cut, and the result keeps to the limit.
        assert_eq!(row_text("12345", 5), "12345");
        assert_eq!(row_text("123456", 5), "1234…");
        let long = "x".repeat(5_000);
        let cut = row_text(&long, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with('…'));
        // It counts characters, not bytes, and never splits one.
        let wide = "日本語".repeat(100);
        assert_eq!(row_text(&wide, 7), "日本語日本語…");
        assert_eq!(row_text("日本語日本語", 6), "日本語日本語");
        // Cutting twice changes nothing.
        assert_eq!(row_text(&row_text(&wide, 7), 7), row_text(&wide, 7));
        assert_eq!(row_text("", 3), "");
        assert_eq!(row_text("abc", 0), "…", "a limit of zero is read as one");
    }

    fn found(n: usize, scripts: usize) -> SearchOutcome {
        let hits = (0..n)
            .map(|i| ScriptHit {
                id: (i % scripts.max(1)) as u64,
                name: "s".into(),
                location: "l".into(),
                line: i + 1,
                text: "t".into(),
                match_start: 0,
                match_len: 1,
                col: 0,
                chars: 1,
            })
            .collect();
        SearchOutcome { hits, truncated: false }
    }

    #[test]
    fn the_status_line_counts_results_and_scripts() {
        let mut state = SearchState::default();
        assert_eq!(state.status(), "");
        state.type_query("spawn".into());
        state.finish(found(0, 1), 12);
        assert_eq!(state.status(), "No matches in 12 scripts");
        state.finish(found(0, 1), 1);
        assert_eq!(state.status(), "No matches in 1 script");
        state.finish(found(0, 1), 0);
        assert_eq!(state.status(), "This Space has no scripts");
        state.finish(found(1, 1), 4);
        assert_eq!(state.status(), "1 result in 1 script");
        state.finish(found(7, 3), 40);
        assert_eq!(state.status(), "7 results in 3 scripts");
        let mut cut = found(MAX_HITS, 9);
        cut.truncated = true;
        state.finish(cut, 40);
        assert_eq!(state.status(), "500 shown; narrow the search to see the rest");
        // A blank query says nothing, whatever the last results were.
        state.type_query("  ".into());
        assert_eq!(state.status(), "");
    }

    #[test]
    fn typing_marks_the_search_stale_and_a_result_clears_it() {
        let mut state = SearchState::default();
        state.type_query("a".into());
        assert!(state.stale);
        state.finish(found(3, 1), 1);
        assert!(!state.stale && state.list_changed && state.selection_changed);
        // The same text again changes nothing.
        state.type_query("a".into());
        assert!(!state.stale);
        state.set_match_case(true);
        assert!(state.stale);
        state.finish(found(3, 1), 1);
        state.set_match_case(true);
        assert!(!state.stale);
        state.reopen();
        assert!(state.stale);
    }

    #[test]
    fn the_highlight_moves_and_stays_on_the_list() {
        let mut state = SearchState::default();
        state.move_selection(1);
        assert_eq!(state.selected, 0, "an empty list has nothing to move to");
        assert!(!state.selection_changed);
        state.finish(found(25, 1), 1);
        state.selection_changed = false;
        state.move_selection(1);
        assert_eq!(state.selected, 1);
        assert!(state.selection_changed);
        state.move_selection(10);
        assert_eq!(state.selected, 11);
        state.move_selection(100);
        assert_eq!(state.selected, 24);
        state.selection_changed = false;
        state.move_selection(1);
        assert_eq!(state.selected, 24);
        assert!(!state.selection_changed, "no change at the last row");
        state.move_selection(-100);
        assert_eq!(state.selected, 0);
        state.move_selection(i32::MIN);
        state.move_selection(i32::MAX);
        assert_eq!(state.selected, 24);
    }

    #[test]
    fn a_new_result_puts_the_highlight_back_on_the_first_row() {
        let mut state = SearchState::default();
        state.finish(found(5, 1), 1);
        state.move_selection(3);
        state.finish(found(5, 1), 1);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn opening_a_row_selects_it_and_refuses_rows_that_are_not_there() {
        let mut state = SearchState::default();
        assert!(!state.request_open(0));
        state.finish(found(3, 1), 1);
        assert!(state.request_open(2));
        assert_eq!(state.selected, 2);
        assert_eq!(state.open.as_ref().map(|hit| hit.line), Some(3));
        state.open = None;
        assert!(!state.request_open(3));
        assert!(!state.request_open(-1));
        assert_eq!(state.open, None);
    }

    #[test]
    fn an_open_request_keeps_its_match_when_a_search_lands_first() {
        let mut state = SearchState::default();
        state.finish(found(3, 1), 1);
        assert!(state.request_open(2));
        // A newer search replaces the list before the request is served.
        state.finish(found(1, 1), 1);
        assert_eq!(state.hits.len(), 1);
        assert_eq!(state.open.as_ref().map(|hit| hit.line), Some(3), "the request is still for the match that was clicked");
    }

    #[test]
    fn closing_lets_go_of_the_matches_but_keeps_the_query() {
        let mut state = SearchState::default();
        state.type_query("spawn".into());
        state.set_match_case(true);
        state.finish(found(4, 2), 9);
        state.request_open(1);
        state.close();
        assert!(state.hits.is_empty());
        assert_eq!((state.selected, state.open), (0, None));
        assert_eq!((state.query.as_str(), state.match_case), ("spawn", true));
        assert!(!state.stale);
    }

    #[test]
    fn a_jump_waits_for_a_settled_tab_and_then_applies_once_ready() {
        let mut jump = Jump::new(4, 12, 3, 5);
        assert_eq!((jump.tab, jump.line, jump.col, jump.chars), (4, 12, 3, 5), "the jump keeps the match's place");
        assert_eq!(jump.step(true, false), JumpStep::Wait);
        assert_eq!(jump.step(true, true), JumpStep::Wait, "the first settled frame only starts the count");
        assert_eq!(jump.step(true, true), JumpStep::Apply);
        // A frame that unsettles resets the count.
        let mut jump = Jump::new(4, 12, 3, 5);
        assert_eq!(jump.step(true, true), JumpStep::Wait);
        assert_eq!(jump.step(true, false), JumpStep::Wait);
        assert_eq!(jump.step(true, true), JumpStep::Wait);
        assert_eq!(jump.step(true, true), JumpStep::Apply);
    }

    #[test]
    fn a_jump_is_dropped_when_its_tab_is_gone_or_never_settles() {
        let mut jump = Jump::new(4, 12, 3, 5);
        assert_eq!(jump.step(false, true), JumpStep::Drop);
        let mut jump = Jump::new(4, 12, 3, 5);
        for _ in 0..JUMP_GIVE_UP_FRAMES {
            assert_eq!(jump.step(true, false), JumpStep::Wait);
        }
        assert_eq!(jump.step(true, false), JumpStep::Drop);
    }
}
