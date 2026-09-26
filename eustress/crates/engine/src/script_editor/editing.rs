//! # Editing commands
//!
//! The text transforms behind the script editor's keys: typing with bracket
//! and quote pairs, Enter with auto-indent, Tab and Shift+Tab, the comment
//! toggle, find, go to line, bracket matching, and the editor's own undo
//! history. Pure functions over a text and a selection, so each one is
//! tested without a window.
//!
//! Offsets are bytes into the text, always on character boundaries. The
//! editor sends the selection as `anchor` (where it started) and `cursor`
//! (where the caret is), and gets back the new text and selection.

use std::time::{Duration, Instant};

/// One indent level.
pub const INDENT: &str = "    ";

/// A selection: `anchor` where it started, `cursor` where the caret is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub cursor: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self { anchor: at, cursor: at }
    }
    pub fn start(&self) -> usize {
        self.anchor.min(self.cursor)
    }
    pub fn end(&self) -> usize {
        self.anchor.max(self.cursor)
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }
    /// Clamped into `text` and onto character boundaries.
    pub fn clamp(self, text: &str) -> Self {
        let fix = |mut i: usize| {
            i = i.min(text.len());
            while i > 0 && !text.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        Self { anchor: fix(self.anchor), cursor: fix(self.cursor) }
    }
}

/// The result of an editing command: the whole new text and its selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub selection: Selection,
}

/// One replacement: `remove` bytes at `at` become `insert`.
#[derive(Debug, Clone)]
struct Change {
    at: usize,
    remove: usize,
    insert: String,
}

/// Apply non-overlapping `changes` (any order) and map `sel` through them.
fn apply_changes(text: &str, mut changes: Vec<Change>, sel: Selection) -> Edit {
    changes.sort_by_key(|c| c.at);
    // Where an old offset lands: changes wholly before it shift it, an
    // insertion exactly at it pushes it along, and an offset inside removed
    // text lands where the replacement starts.
    let map = |o: usize| {
        let mut delta = 0isize;
        for c in &changes {
            if c.at > o {
                break;
            }
            if c.remove == 0 {
                delta += c.insert.len() as isize;
                continue;
            }
            if o < c.at + c.remove {
                return (c.at as isize + delta).max(0) as usize;
            }
            delta += c.insert.len() as isize - c.remove as isize;
        }
        (o as isize + delta).max(0) as usize
    };
    let mut out = String::with_capacity(text.len() + 16);
    let mut from = 0;
    for c in &changes {
        out.push_str(&text[from..c.at]);
        out.push_str(&c.insert);
        from = c.at + c.remove;
    }
    out.push_str(&text[from..]);
    let selection = Selection { anchor: map(sel.anchor), cursor: map(sel.cursor) }.clamp(&out);
    Edit { text: out, selection }
}

/// Byte offset where the line holding `offset` starts.
pub fn line_start(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
}

/// Byte offset where the line holding `offset` ends (before its `\n`).
pub fn line_end(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[offset..].find('\n').map_or(text.len(), |i| offset + i)
}

/// Byte offset of the start of 1-based `line`, or the text's end past the
/// last line.
pub fn line_start_offset(text: &str, line: usize) -> usize {
    let mut at = 0;
    for _ in 1..line.max(1) {
        match text[at..].find('\n') {
            Some(i) => at += i + 1,
            None => return text.len(),
        }
    }
    at
}

/// The 1-based line holding `offset`.
pub fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Start offsets of the lines a selection touches. A selection that ends at
/// the very start of a line leaves that line out.
fn touched_lines(text: &str, sel: Selection) -> Vec<usize> {
    let first = line_start(text, sel.start());
    let mut last_offset = sel.end();
    if !sel.is_empty() && last_offset > first && line_start(text, last_offset) == last_offset {
        last_offset -= 1;
    }
    let last = line_start(text, last_offset);
    let mut starts = vec![first];
    let mut at = first;
    while at < last {
        match text[at..].find('\n') {
            Some(i) => {
                at += i + 1;
                starts.push(at);
            }
            None => break,
        }
    }
    starts
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Type `ch`: replaces the selection, with the language's pairs. An opener
/// wraps a selection, or closes itself when the caret sits before
/// whitespace, a closer or the end of the line. A closer typed right before
/// the same closer steps over it. A closing bracket typed on an empty line
/// moves out one indent level first.
pub fn type_char(text: &str, sel: Selection, ch: char, pairs: &[(char, char)]) -> Edit {
    let sel = sel.clamp(text);
    let (start, end) = (sel.start(), sel.end());
    let next = text[end..].chars().next();
    let prev = text[..start].chars().next_back();
    let mut buf = [0u8; 4];
    let typed = ch.encode_utf8(&mut buf).to_string();

    // Step over a closer (or a closing quote) that is already there.
    let is_closer = pairs.iter().any(|&(_, c)| c == ch);
    if sel.is_empty() && is_closer && next == Some(ch) {
        return Edit { text: text.to_string(), selection: Selection::caret(end + ch.len_utf8()) };
    }

    if let Some(&(open, close)) = pairs.iter().find(|&&(o, _)| o == ch) {
        if !sel.is_empty() {
            let inner = &text[start..end];
            let insert = format!("{open}{inner}{close}");
            let edit = apply_changes(text, vec![Change { at: start, remove: end - start, insert }], Selection::caret(start));
            let a = start + open.len_utf8();
            return Edit { text: edit.text, selection: Selection { anchor: a, cursor: a + inner.len() } };
        }
        let quote = open == close;
        let closes_here = match next {
            None => true,
            Some(c) => c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | ':'),
        };
        // `it's` and `x'` stay a single quote.
        let word_before = prev.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if closes_here && !(quote && word_before) {
            let insert = format!("{open}{close}");
            let edit = apply_changes(text, vec![Change { at: start, remove: 0, insert }], Selection::caret(start));
            return Edit { text: edit.text, selection: Selection::caret(start + open.len_utf8()) };
        }
    }

    // A closing bracket on an otherwise empty line moves out one level.
    let line_from = line_start(text, start);
    let before_on_line = &text[line_from..start];
    if sel.is_empty()
        && matches!(ch, ')' | ']' | '}')
        && !before_on_line.is_empty()
        && before_on_line.chars().all(|c| c == ' ' || c == '\t')
    {
        let cut = before_on_line.len().min(INDENT.len());
        let edit = apply_changes(
            text,
            vec![Change { at: start - cut, remove: cut, insert: typed }],
            Selection::caret(start),
        );
        let caret = start - cut + ch.len_utf8();
        return Edit { text: edit.text, selection: Selection::caret(caret) };
    }

    let edit = apply_changes(text, vec![Change { at: start, remove: end - start, insert: typed }], Selection::caret(start));
    Edit { text: edit.text, selection: Selection::caret(start + ch.len_utf8()) }
}

/// Enter: a new line indented like the current one, one level deeper when
/// `indent_after` says the line opens a block. Between a bracket pair the
/// closer moves to its own line below.
pub fn newline(text: &str, sel: Selection, indent_after: impl Fn(&str) -> bool) -> Edit {
    let sel = sel.clamp(text);
    let (start, end) = (sel.start(), sel.end());
    let from = line_start(text, start);
    let before = &text[from..start];
    let base = leading_whitespace(before).to_string();
    let deeper = indent_after(before);
    let inner = if deeper { format!("{base}{INDENT}") } else { base.clone() };
    let prev = before.trim_end_matches([' ', '\t']).chars().next_back();
    let next = text[end..].chars().next();
    let between_pair = matches!((prev, next), (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}')));
    let insert = if between_pair && deeper {
        format!("\n{inner}\n{base}")
    } else {
        format!("\n{inner}")
    };
    let edit = apply_changes(text, vec![Change { at: start, remove: end - start, insert }], Selection::caret(start));
    Edit { text: edit.text, selection: Selection::caret(start + 1 + inner.len()) }
}

/// Tab: a selection across lines indents each line; otherwise spaces up to
/// the next indent stop replace the selection.
pub fn indent(text: &str, sel: Selection) -> Edit {
    let sel = sel.clamp(text);
    let lines = touched_lines(text, sel);
    let multi_line = !sel.is_empty() && text[sel.start()..sel.end()].contains('\n');
    if multi_line {
        let changes = lines.iter().map(|&at| Change { at, remove: 0, insert: INDENT.to_string() }).collect();
        return apply_changes(text, changes, sel);
    }
    let from = line_start(text, sel.start());
    let column = text[from..sel.start()].chars().count();
    let spaces = INDENT.len() - column % INDENT.len();
    let insert = " ".repeat(spaces);
    let (start, end) = (sel.start(), sel.end());
    let edit = apply_changes(text, vec![Change { at: start, remove: end - start, insert }], Selection::caret(start));
    Edit { text: edit.text, selection: Selection::caret(start + spaces) }
}

/// Shift+Tab: each touched line loses one indent level (up to four spaces,
/// or one tab).
pub fn outdent(text: &str, sel: Selection) -> Edit {
    let sel = sel.clamp(text);
    let changes = touched_lines(text, sel)
        .into_iter()
        .filter_map(|at| {
            let rest = &text[at..line_end(text, at)];
            let remove = if rest.starts_with('\t') {
                1
            } else {
                rest.bytes().take(INDENT.len()).take_while(|&b| b == b' ').count()
            };
            (remove > 0).then(|| Change { at, remove, insert: String::new() })
        })
        .collect();
    apply_changes(text, changes, sel)
}

/// Ctrl+/: comment the touched lines with `marker`, or uncomment them when
/// every non-blank one is already commented. The marker goes at the lines'
/// shared indent, with one space after it.
pub fn toggle_comment(text: &str, sel: Selection, marker: &str) -> Edit {
    let sel = sel.clamp(text);
    let lines: Vec<(usize, &str)> = touched_lines(text, sel)
        .into_iter()
        .map(|at| (at, &text[at..line_end(text, at)]))
        .collect();
    let code: Vec<&(usize, &str)> = lines.iter().filter(|(_, l)| !l.trim().is_empty()).collect();
    if code.is_empty() {
        return Edit { text: text.to_string(), selection: sel };
    }
    let all_commented = code.iter().all(|(_, l)| l.trim_start().starts_with(marker));
    let changes = if all_commented {
        code.iter()
            .map(|(at, l)| {
                let lead = leading_whitespace(l).len();
                let after = &l[lead + marker.len()..];
                let remove = marker.len() + usize::from(after.starts_with(' '));
                Change { at: at + lead, remove, insert: String::new() }
            })
            .collect()
    } else {
        let lead = code.iter().map(|(_, l)| leading_whitespace(l).len()).min().unwrap_or(0);
        code.iter()
            .map(|(at, _)| Change { at: at + lead, remove: 0, insert: format!("{marker} ") })
            .collect()
    };
    apply_changes(text, changes, sel)
}

/// Every occurrence of `query`, as byte ranges, in order. Case-insensitive
/// matching folds ASCII letters only, so every range stays exact.
pub fn find_all(text: &str, query: &str, match_case: bool) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if match_case {
        let mut from = 0;
        while let Some(i) = text[from..].find(query) {
            out.push((from + i, from + i + query.len()));
            from += i + query.len().max(1);
        }
    } else {
        let hay = text.as_bytes();
        let needle = query.as_bytes();
        let mut i = 0;
        while i + needle.len() <= hay.len() {
            if hay[i..i + needle.len()].eq_ignore_ascii_case(needle)
                && text.is_char_boundary(i)
                && text.is_char_boundary(i + needle.len())
            {
                out.push((i, i + needle.len()));
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Replace every range with `with` in one edit, keeping the caret where it
/// was in the text around the replacements.
pub fn replace_ranges(text: &str, ranges: &[(usize, usize)], with: &str, sel: Selection) -> Edit {
    let changes = ranges
        .iter()
        .map(|&(a, b)| Change { at: a, remove: b - a, insert: with.to_string() })
        .collect();
    apply_changes(text, changes, sel.clamp(text))
}

/// The bracket matching the one at `offset` or just before it: the pair's
/// two byte offsets. Brackets inside strings and comments are not told
/// apart; the result is a hint for the eye.
pub fn matching_bracket(text: &str, offset: usize) -> Option<(usize, usize)> {
    let offset = offset.min(text.len());
    let at = |i: usize| text[i..].chars().next();
    let candidates = [Some(offset), text[..offset].char_indices().next_back().map(|(i, _)| i)];
    for i in candidates.into_iter().flatten() {
        let Some(c) = at(i) else { continue };
        let (open, close, forward) = match c {
            '(' => ('(', ')', true),
            '[' => ('[', ']', true),
            '{' => ('{', '}', true),
            ')' => ('(', ')', false),
            ']' => ('[', ']', false),
            '}' => ('{', '}', false),
            _ => continue,
        };
        let mut depth = 0i32;
        if forward {
            for (j, d) in text[i..].char_indices() {
                if d == open {
                    depth += 1;
                } else if d == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((i, i + j));
                    }
                }
            }
        } else {
            for (j, d) in text[..=i].char_indices().rev() {
                if d == close {
                    depth += 1;
                } else if d == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some((j, i));
                    }
                }
            }
        }
    }
    None
}

/// Where the text differs first: the length of the common prefix, on a
/// character boundary. The caret for an undo step.
pub fn first_difference(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while n > 0 && !(a.is_char_boundary(n) && b.is_char_boundary(n)) {
        n -= 1;
    }
    n
}

/// Hover markdown as plain text for a popup that draws no markdown: code
/// fences, bold and code markers go, headings lose their `#`, and runs of
/// blank lines shrink to one.
pub fn markdown_to_plain(markdown: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            continue;
        }
        let line = trimmed.trim_start_matches('#').trim_start();
        let line = line.replace("**", "").replace('`', "");
        if line.trim().is_empty() && out.last().map_or(true, |l| l.is_empty()) {
            continue;
        }
        out.push(if line.trim().is_empty() { String::new() } else { line });
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// A text and caret the editor can return to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub text: String,
    pub cursor: usize,
}

/// The editor's own undo and redo, per tab. Edits in quick succession join
/// one step, so undo takes back a burst of typing rather than a character.
#[derive(Debug, Default, Clone)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<Instant>,
}

impl History {
    /// Steps kept per tab.
    pub const LIMIT: usize = 200;
    /// Edits closer together than this join one step.
    pub const GROUP: Duration = Duration::from_millis(700);

    /// Record the text as it was before an edit made at `now`.
    pub fn record(&mut self, before: &str, cursor: usize, now: Instant) {
        let joins = self.last_edit.is_some_and(|t| now.saturating_duration_since(t) < Self::GROUP);
        self.last_edit = Some(now);
        self.redo.clear();
        if joins && !self.undo.is_empty() {
            return;
        }
        if self.undo.last().is_some_and(|s| s.text == before) {
            return;
        }
        self.undo.push(Snapshot { text: before.to_string(), cursor });
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    /// The next edit starts a new step (after a command, a paste, an undo).
    pub fn break_group(&mut self) {
        self.last_edit = None;
    }

    /// Step back: returns the snapshot to restore, keeping `current` for redo.
    pub fn undo(&mut self, current: &str, cursor: usize) -> Option<Snapshot> {
        let back = self.undo.pop()?;
        self.redo.push(Snapshot { text: current.to_string(), cursor });
        self.last_edit = None;
        Some(back)
    }

    /// Step forward again after an undo.
    pub fn redo(&mut self, current: &str, cursor: usize) -> Option<Snapshot> {
        let forward = self.redo.pop()?;
        self.undo.push(Snapshot { text: current.to_string(), cursor });
        self.last_edit = None;
        Some(forward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')];

    /// `|` marks the caret; `[` .. `]` is not used, selections are explicit.
    fn at(text: &str) -> (String, Selection) {
        let i = text.find('|').expect("caret mark");
        (text.replacen('|', "", 1), Selection::caret(i))
    }

    fn show(e: &Edit) -> String {
        let mut s = e.text.clone();
        s.insert(e.selection.cursor, '|');
        s
    }

    #[test]
    fn brackets_close_themselves_and_step_over() {
        let (t, s) = at("f|");
        let e = type_char(&t, s, '(', PAIRS);
        assert_eq!(show(&e), "f(|)");
        let e = type_char(&e.text, e.selection, ')', PAIRS);
        assert_eq!(show(&e), "f()|");
        let (t, s) = at("|x");
        assert_eq!(show(&type_char(&t, s, '(', PAIRS)), "(|x", "no pair before a word");
    }

    #[test]
    fn quotes_pair_but_not_inside_words() {
        let (t, s) = at("print(|)");
        assert_eq!(show(&type_char(&t, s, '"', PAIRS)), "print(\"|\")");
        let (t, s) = at("it|");
        let pairs: &[(char, char)] = &[('\'', '\'')];
        assert_eq!(show(&type_char(&t, s, '\'', pairs)), "it'|");
    }

    #[test]
    fn an_opener_wraps_the_selection() {
        let e = type_char("a b c", Selection { anchor: 2, cursor: 3 }, '[', PAIRS);
        assert_eq!(e.text, "a [b] c");
        assert_eq!(e.selection, Selection { anchor: 3, cursor: 4 });
    }

    #[test]
    fn a_closing_brace_on_an_empty_line_moves_out() {
        let (t, s) = at("if x {\n        |");
        assert_eq!(show(&type_char(&t, s, '}', PAIRS)), "if x {\n    }|");
    }

    #[test]
    fn enter_keeps_and_deepens_indent() {
        let opens = |l: &str| l.trim_end().ends_with('{');
        let (t, s) = at("    let x = 1;|");
        assert_eq!(show(&newline(&t, s, opens)), "    let x = 1;\n    |");
        let (t, s) = at("fn f() {|}");
        assert_eq!(show(&newline(&t, s, opens)), "fn f() {\n    |\n}");
    }

    #[test]
    fn tab_and_shift_tab() {
        let (t, s) = at("ab|");
        assert_eq!(show(&indent(&t, s)), "ab  |", "spaces to the next stop");
        let t = "a\nb\nc";
        let e = indent(t, Selection { anchor: 0, cursor: 3 });
        assert_eq!(e.text, "    a\n    b\nc");
        let e = outdent(&e.text, Selection { anchor: 0, cursor: 11 });
        assert_eq!(e.text, "a\nb\nc");
        assert_eq!(outdent("  x", Selection::caret(3)).text, "x");
    }

    #[test]
    fn comment_toggle_round_trips() {
        let t = "    a\n\n    b";
        let e = toggle_comment(t, Selection { anchor: 0, cursor: t.len() }, "--");
        assert_eq!(e.text, "    -- a\n\n    -- b");
        let back = toggle_comment(&e.text, Selection { anchor: 0, cursor: e.text.len() }, "--");
        assert_eq!(back.text, t);
    }

    #[test]
    fn find_is_exact_and_case_folding_is_ascii_only() {
        assert_eq!(find_all("aAa", "a", true), [(0, 1), (2, 3)]);
        assert_eq!(find_all("aAa", "a", false), [(0, 1), (1, 2), (2, 3)]);
        assert_eq!(find_all("éÉ", "é", false), [(0, 2)]);
        let e = replace_ranges("a-a", &[(0, 1), (2, 3)], "bb", Selection::caret(3));
        assert_eq!(e.text, "bb-bb");
    }

    #[test]
    fn lines_and_brackets() {
        let t = "ab\ncd\n";
        assert_eq!(line_start_offset(t, 2), 3);
        assert_eq!(line_start_offset(t, 9), t.len());
        assert_eq!(line_of(t, 4), 2);
        assert_eq!(matching_bracket("f(a[1])", 1), Some((1, 6)));
        assert_eq!(matching_bracket("f(a[1])", 7), Some((1, 6)), "the bracket just before the caret");
        assert_eq!(matching_bracket("abc", 1), None);
    }

    #[test]
    fn hover_markdown_reads_as_plain_text() {
        let md = "## `spawn`\n\n```rune\nfn spawn(name: String)\n```\n\n\n**Creates** a part.\n";
        assert_eq!(markdown_to_plain(md), "spawn\n\nfn spawn(name: String)\n\nCreates a part.");
        assert_eq!(markdown_to_plain(""), "");
    }

    #[test]
    fn undo_joins_a_burst_and_redo_returns() {
        let t0 = Instant::now();
        let mut h = History::default();
        h.record("", 0, t0);
        h.record("a", 1, t0 + Duration::from_millis(100));
        h.record("ab", 2, t0 + Duration::from_millis(200));
        let back = h.undo("abc", 3).unwrap();
        assert_eq!(back.text, "", "the burst is one step");
        let fwd = h.redo(&back.text, back.cursor).unwrap();
        assert_eq!(fwd.text, "abc");
        h.record("abc", 3, t0 + Duration::from_secs(5));
        assert!(h.redo("abcd", 4).is_none(), "a new edit clears redo");
        assert_eq!(first_difference("héllo", "hélp"), "hél".len());
    }
}
