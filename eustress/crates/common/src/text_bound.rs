//! How much of an outside string Eustress draws at once.
//!
//! A string from a file, a Space, a model or a peer can be any size, and
//! drawing text costs in proportion to it: Studio's Slint UI used to abort
//! past 32,767 px of text (its renderer keeps pixels as `i16`; the engine
//! patches it to clip), and the label rasterizer that draws ScreenGuis and
//! BillboardGuis (`play-runtime`) froze a frame for seconds on a megabyte
//! label. Text from outside is bounded here before it is drawn:
//! - [`bound_block`] for wrapped text sized to its content;
//! - [`bound_block_lines`], the same in fewer lines;
//! - [`bound_block_nowrap`] for unwrapped multi-line text;
//! - [`bound_line`] for one-line labels;
//! - [`page`] where the whole string can be read a page at a time.
//!
//! Only on text that is shown, never on a value that is edited and saved
//! back: a bounded value there would save truncated.
//!
//! Sizes are counted pessimistically, in width units: a plain ASCII character
//! is 2, a capital or a wide ASCII glyph 3, a tab 8, anything else 6 (CJK,
//! emoji and other scripts draw wider than Latin). A wrapped line holds 40
//! units, about 20 plain characters, which is the narrowest panel. Every line
//! break a text layout honours counts (`\n`, `\r`, `\r\n`, vertical tab, form
//! feed, NEL, U+2028, U+2029), and a character keeps at most four combining
//! marks, since stacked marks grow a line upwards. Bounding twice changes
//! nothing, so a bounded value compares equal to itself.

use std::borrow::Cow;

/// Width units in one drawn line of wrapped text.
pub const UNITS_PER_LINE: usize = 40;
/// Drawn lines in one block or page, its closing note included. At 16 px
/// text and 200% scale that is under half of the renderer's limit, even in a
/// panel half as wide as the estimate assumes.
pub const BLOCK_LINES: usize = 400;
/// Width units in one line of unwrapped text.
pub const NOWRAP_LINE_UNITS: usize = 1_200;
/// Width units in a one-line label.
pub const LINE_UNITS: usize = 400;
/// Combining marks kept after one character.
pub const MAX_MARKS: usize = 4;

// The note that closes a cut block: a blank line, then the note, which the
// estimate wraps to two lines.
const NOTE_LINES: usize = 3;

/// Whether the Text wraps its lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Wrapped,
    NoWrap,
}

/// One page of a long string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub text: String,
    /// This page, from 0.
    pub index: usize,
    /// How many pages the string has (at least 1).
    pub count: usize,
}

/// A character's width in units.
pub fn char_units(c: char) -> usize {
    match c {
        '\t' => 8,
        'A'..='Z' | '@' | '%' | '#' | '&' | 'm' | 'w' => 3,
        ' '..='~' => 2,
        c if c.is_ascii_control() => 0,
        _ => 6,
    }
}

fn is_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{0483}'..='\u{0489}'
        | '\u{0591}'..='\u{05BD}'
        | '\u{0610}'..='\u{061A}'
        | '\u{064B}'..='\u{065F}'
        | '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}')
}

/// The string as it is drawn: every line break a `\n`, and at most
/// [`MAX_MARKS`] combining marks after a character. Borrowed when nothing
/// changes.
fn normalize(s: &str) -> Cow<'_, str> {
    let mut marks = 0;
    let clean = s.chars().all(|c| {
        if is_mark(c) {
            marks += 1;
            marks <= MAX_MARKS
        } else {
            marks = 0;
            c == '\n' || !is_break(c)
        }
    });
    if clean {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut marks = 0;
    while let Some(c) = chars.next() {
        if is_mark(c) {
            marks += 1;
            if marks <= MAX_MARKS {
                out.push(c);
            }
            continue;
        }
        marks = 0;
        if is_break(c) {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Whether `s` has more than one line.
pub fn has_more_lines(s: &str) -> bool {
    s.chars().any(is_break)
}

/// What follows the first line of `s`, or `None` for a one-line string.
pub fn after_first_line(s: &str) -> Option<&str> {
    let (at, c) = s.char_indices().find(|(_, c)| is_break(*c))?;
    let rest = &s[at + c.len_utf8()..];
    Some(if c == '\r' { rest.strip_prefix('\n').unwrap_or(rest) } else { rest })
}

/// Lines a wrapped Text draws for `s`, by the estimate above.
pub fn drawn_lines(s: &str) -> usize {
    let s = normalize(s);
    let (mut lines, mut units) = (1, 0);
    for c in s.chars() {
        if c == '\n' {
            lines += 1;
            units = 0;
            continue;
        }
        let u = char_units(c);
        if units + u > UNITS_PER_LINE && units > 0 {
            lines += 1;
            units = 0;
        }
        units += u;
    }
    lines
}

/// The part of `s` (already normalized) from byte `start` that draws in at
/// most `budget` lines, and the byte where it stopped. A line too wide for
/// an unwrapped Text keeps its first [`NOWRAP_LINE_UNITS`] units and ends in
/// an ellipsis.
fn cut(s: &str, start: usize, wrap: Wrap, budget: usize) -> (String, usize) {
    let mut out = String::new();
    let (mut lines, mut units) = (1, 0);
    let mut clipped = false;
    // How much of `out` leaves this line room for an ellipsis, so a clipped
    // line clips to itself.
    let room = NOWRAP_LINE_UNITS - char_units('…');
    let mut fits = 0;
    for (i, c) in s[start..].char_indices() {
        let at = start + i;
        if c == '\n' {
            if lines == budget {
                return (out, at + 1);
            }
            lines += 1;
            units = 0;
            clipped = false;
            out.push('\n');
            fits = out.len();
            continue;
        }
        let u = char_units(c);
        match wrap {
            Wrap::Wrapped => {
                if units + u > UNITS_PER_LINE && units > 0 {
                    if lines == budget {
                        return (out, at);
                    }
                    lines += 1;
                    units = 0;
                }
                units += u;
                out.push(c);
            }
            Wrap::NoWrap => {
                if clipped {
                    continue;
                }
                if units + u > NOWRAP_LINE_UNITS {
                    out.truncate(fits);
                    out.push('…');
                    clipped = true;
                    continue;
                }
                units += u;
                out.push(c);
                if units <= room {
                    fits = out.len();
                }
            }
        }
    }
    (out, s.len())
}

fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(d);
    }
    out
}

fn bound(s: &str, wrap: Wrap, lines: usize) -> String {
    let lines = lines.clamp(NOTE_LINES + 1, BLOCK_LINES);
    let s = normalize(s);
    let (body, end) = cut(&s, 0, wrap, lines);
    if end >= s.len() {
        return body;
    }
    // It does not fit: make room for the note.
    let (mut body, end) = cut(&s, 0, wrap, lines - NOTE_LINES);
    let rest = &s[end..];
    let more_lines = rest.matches('\n').count();
    let note = if more_lines == 0 {
        let n = rest.chars().count();
        format!("… {} more character{} not shown", grouped(n), if n == 1 { "" } else { "s" })
    } else {
        let n = more_lines + 1;
        format!("… {} more lines not shown", grouped(n))
    };
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push('\n');
    body.push_str(&note);
    body
}

/// Wrapped text sized to its content: at most [`BLOCK_LINES`] drawn lines,
/// closing with a note of what is not shown.
pub fn bound_block(s: &str) -> String {
    bound(s, Wrap::Wrapped, BLOCK_LINES)
}

/// [`bound_block`] in at most `lines` drawn lines (4 to [`BLOCK_LINES`]),
/// for a surface where drawing costs more than a panel's text does.
pub fn bound_block_lines(s: &str, lines: usize) -> String {
    bound(s, Wrap::Wrapped, lines)
}

/// Unwrapped multi-line text: at most [`BLOCK_LINES`] lines, each at most
/// [`NOWRAP_LINE_UNITS`] units wide.
pub fn bound_block_nowrap(s: &str) -> String {
    bound(s, Wrap::NoWrap, BLOCK_LINES)
}

/// A one-line label: the first line, at most [`LINE_UNITS`] units, ending in
/// an ellipsis when anything was left out. The ellipsis fits inside the
/// bound, so a bounded label bounds to itself.
pub fn bound_line(s: &str) -> String {
    let room = LINE_UNITS - char_units('…');
    let mut out = String::new();
    // `fits`: how much of `out` leaves room for the ellipsis.
    let (mut units, mut marks, mut fits) = (0, 0, 0);
    for c in s.chars() {
        if is_mark(c) {
            marks += 1;
            if marks > MAX_MARKS {
                continue;
            }
        } else {
            marks = 0;
        }
        let u = char_units(c);
        if is_break(c) || units + u > LINE_UNITS {
            out.truncate(fits);
            out.push('…');
            return out;
        }
        units += u;
        out.push(c);
        if units <= room {
            fits = out.len();
        }
    }
    out
}

/// Page `index` of `s`, each page at most `lines` drawn lines (at most
/// [`BLOCK_LINES`]); an index past the end gives the last page. Read in
/// order, the pages of wrapped text are the whole string.
pub fn page(s: &str, wrap: Wrap, lines: usize, index: usize) -> Page {
    let lines = lines.clamp(1, BLOCK_LINES);
    let s = normalize(s);
    let mut starts = vec![0];
    loop {
        let start = *starts.last().unwrap_or(&0);
        let (_, end) = cut(&s, start, wrap, lines);
        if end >= s.len() || end == start {
            break;
        }
        starts.push(end);
    }
    let count = starts.len();
    let index = index.min(count - 1);
    let (text, _) = cut(&s, starts[index], wrap, lines);
    Page { text, index, count }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fits(s: &str) {
        let lines = drawn_lines(s);
        assert!(lines <= BLOCK_LINES, "{lines} drawn lines");
    }

    #[test]
    fn short_text_is_left_as_it_is() {
        for s in ["", "Hello", "Two\nlines", "Ünïcødé ✓ 日本語"] {
            assert_eq!(bound_block(s), s);
            assert_eq!(bound_block_nowrap(s), s);
        }
        assert_eq!(bound_line("Part"), "Part");
        assert_eq!(drawn_lines(""), 1);
        assert_eq!(drawn_lines("a\n"), 2, "a trailing break draws an empty line");
    }

    #[test]
    fn ten_thousand_lines_are_bounded_and_say_so() {
        let s = "x\n".repeat(10_000);
        let b = bound_block(&s);
        fits(&b);
        assert!(b.ends_with("more lines not shown"), "{}", &b[b.len() - 60..]);
        let empty = "\n".repeat(10_000);
        fits(&bound_block(&empty));
        fits(&bound_block_nowrap(&empty));
    }

    #[test]
    fn one_long_line_is_bounded_both_ways() {
        let s = "a".repeat(131_000);
        let b = bound_block(&s);
        fits(&b);
        assert!(b.ends_with("more characters not shown"));
        let n = bound_block_nowrap(&s);
        assert!(n.chars().count() <= NOWRAP_LINE_UNITS / 2 + 1, "{}", n.chars().count());
        assert!(n.ends_with('…'));
    }

    #[test]
    fn wide_glyphs_tabs_and_capitals_count_wide() {
        for c in ['語', '\t', 'W', '😀'] {
            let s: String = std::iter::repeat(c).take(25_000).collect();
            fits(&bound_block(&s));
            assert!(bound_line(&s).ends_with('…'));
        }
        // A CJK character is 6 units, six to a 40-unit line: 1,000 of them
        // are 167 drawn lines.
        assert_eq!(drawn_lines(&"語".repeat(1_000)), 167);
    }

    #[test]
    fn every_line_break_counts() {
        for br in ["\r\n", "\r", "\u{0B}", "\u{0C}", "\u{85}", "\u{2028}", "\u{2029}"] {
            let s = format!("x{br}").repeat(10_000);
            assert!(drawn_lines(&s) > 9_000, "{br:?}");
            fits(&bound_block(&s));
            assert_eq!(bound_line(&s), "x…");
        }
    }

    #[test]
    fn stacked_marks_are_capped() {
        let s = format!("a{}b", "\u{0301}".repeat(10_000));
        let b = bound_block(&s);
        assert_eq!(b.chars().filter(|c| is_mark(*c)).count(), MAX_MARKS);
        assert!(b.starts_with('a') && b.ends_with('b'));
        assert_eq!(bound_line(&s).chars().filter(|c| is_mark(*c)).count(), MAX_MARKS);
    }

    #[test]
    fn a_label_is_one_short_line() {
        assert_eq!(bound_line("Name\nsecond line"), "Name…");
        let long = "n".repeat(10_000);
        let l = bound_line(&long);
        assert_eq!(l.chars().count(), (LINE_UNITS - char_units('…')) / 2 + 1);
        assert!(l.ends_with('…'));
        let exact = "n".repeat(LINE_UNITS / 2);
        assert_eq!(bound_line(&exact), exact, "a label exactly at the bound stays whole");
    }

    #[test]
    fn a_smaller_budget_bounds_to_it() {
        let s = "x\n".repeat(10_000);
        let b = bound_block_lines(&s, 200);
        assert!(drawn_lines(&b) <= 200, "{}", drawn_lines(&b));
        assert!(b.ends_with("more lines not shown"));
        assert_eq!(bound_block_lines(&b, 200), b, "and bounds to itself");
        assert_eq!(bound_block_lines("short", 200), "short");
        assert!(drawn_lines(&bound_block_lines(&s, 1)) <= 4, "the budget has a floor that holds the note");
    }

    #[test]
    fn bounding_twice_changes_nothing() {
        // Studio compares a shown value with the bounded new one before it
        // sets it; a second bound that moved would set it every frame.
        let inputs = [
            "short".to_string(),
            "n".repeat(10_000),
            "W".repeat(133),
            "x\n".repeat(10_000),
            "a".repeat(131_000),
            format!("a{}b", "\u{0301}".repeat(50)),
            "語".repeat(25_000),
            format!("{}\n{}", "q".repeat(500), "r".repeat(500)),
        ];
        for s in &inputs {
            let line = bound_line(s);
            assert_eq!(bound_line(&line), line, "bound_line of {:?}…", s.chars().take(12).collect::<String>());
            let block = bound_block(s);
            assert_eq!(bound_block(&block), block, "bound_block of {:?}…", s.chars().take(12).collect::<String>());
            let nowrap = bound_block_nowrap(s);
            assert_eq!(bound_block_nowrap(&nowrap), nowrap, "bound_block_nowrap of {:?}…", s.chars().take(12).collect::<String>());
        }
    }

    #[test]
    fn pages_read_in_order_are_the_whole_string() {
        let s: String = (0..5_000).map(|i| format!("line {i}\n")).collect();
        let first = page(&s, Wrap::Wrapped, BLOCK_LINES, 0);
        assert!(first.count > 1);
        let mut joined = String::new();
        for i in 0..first.count {
            let p = page(&s, Wrap::Wrapped, BLOCK_LINES, i);
            assert_eq!((p.index, p.count), (i, first.count));
            assert!(drawn_lines(&p.text) <= BLOCK_LINES, "page {i}");
            joined.push_str(&p.text);
        }
        // A page that ends at a line break leaves the break out.
        assert_eq!(joined.replace('\n', ""), s.replace('\n', ""));
        assert_eq!(page(&s, Wrap::Wrapped, BLOCK_LINES, 9_999).index, first.count - 1);
        // A short page: 40 lines of unwrapped text.
        let short = page(&s, Wrap::NoWrap, 40, 2);
        assert_eq!(short.text.lines().count(), 40);
        assert!(short.text.starts_with("line 80"), "{}", &short.text[..20]);
        assert_eq!(short.count, 125);
        assert_eq!(page("short", Wrap::Wrapped, BLOCK_LINES, 0), Page { text: "short".into(), index: 0, count: 1 });
    }

    #[test]
    fn the_first_line_splits_off_at_any_break() {
        assert_eq!(after_first_line("one"), None);
        assert_eq!(after_first_line("one\ntwo\nthree"), Some("two\nthree"));
        assert_eq!(after_first_line("one\r\ntwo"), Some("two"));
        assert_eq!(after_first_line("one\u{2028}two"), Some("two"));
        assert!(has_more_lines("a\rb") && !has_more_lines("a b"));
    }

    #[test]
    fn counts_read_as_people_write_them() {
        assert_eq!(grouped(7), "7");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}

