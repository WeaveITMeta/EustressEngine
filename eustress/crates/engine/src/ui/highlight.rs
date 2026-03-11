//! # Syntax Highlighting
//!
//! ## Table of Contents
//! - HighlightSpan: single colored token (text + RGBA color)
//! - highlight_source: tokenize source text into per-line span lists
//! - language_for_path: map file extension → grammar name
//! - Rune grammar: custom regex-based tokenizer for the Rune scripting language
//!
//! ## Architecture
//! syntect tokenizes via a TextMate grammar bundle. Markdown uses the built-in
//! grammar. Rune (a Rust-like scripting language) reuses the Rust grammar since
//! the syntax is nearly identical. Spans are produced per-line so Slint can
//! render each line as a row of colored Text elements.
//!
//! ## Data Structures
//! - HighlightSpan: (text: String, r/g/b/a: f32) — one colored token
//! - Vec<Vec<HighlightSpan>>: outer = lines, inner = spans on that line

use syntect::{
    easy::HighlightLines,
    highlighting::{ThemeSet, Style},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
use std::sync::OnceLock;

// ─── Shared syntax/theme sets (lazy-initialized, reused across calls) ────────

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

// ─── Public data type ────────────────────────────────────────────────────────

/// One colored token within a line of highlighted source code.
#[derive(Debug, Clone)]
pub struct HighlightSpan {
    /// The text of this token (may contain spaces/tabs).
    pub text: String,
    /// Red channel [0.0, 1.0]
    pub r: f32,
    /// Green channel [0.0, 1.0]
    pub g: f32,
    /// Blue channel [0.0, 1.0]
    pub b: f32,
}

impl HighlightSpan {
    fn from_style_text(style: Style, text: &str) -> Self {
        let c = style.foreground;
        Self {
            text: text.to_string(),
            r: c.r as f32 / 255.0,
            g: c.g as f32 / 255.0,
            b: c.b as f32 / 255.0,
        }
    }
}

// ─── Language detection ───────────────────────────────────────────────────────

/// Map a file extension or language hint → syntect grammar scope name.
pub fn language_for_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "md" | "markdown" => "Markdown",
        // Rune is syntactically nearly identical to Rust
        "rn" | "rune" | "soul" => "Rust",
        "rs" => "Rust",
        "toml" => "TOML",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "js" | "jsx" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "py" => "Python",
        "sh" | "bash" => "Bash",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "xml" => "XML",
        "sql" => "SQL",
        "cpp" | "cc" | "cxx" | "hpp" | "h" | "c" => "C++",
        "cs" => "C#",
        "java" => "Java",
        "go" => "Go",
        _ => "Plain Text",
    }
}

// ─── Core highlighter ─────────────────────────────────────────────────────────

/// Tokenize `source` using the grammar for `language` and return a list of
/// per-line span lists.  Never panics — falls back to plain text on error.
///
/// # Arguments
/// * `source` - raw source text (may contain `\r\n` or `\n` line endings)
/// * `language` - syntect grammar name from `language_for_ext()` or any valid
///   grammar name in the default syntect bundle
///
/// # Returns
/// `Vec<Vec<HighlightSpan>>` where index 0 = first line.
/// Empty source → single empty inner `Vec`.

type SyntectRange<'a> = (Style, &'a str);

pub fn highlight_source(source: &str, language: &str) -> Vec<Vec<HighlightSpan>> {
    let ss = syntax_set();
    let ts = theme_set();

    // Resolve grammar — fall back to plain text if not found
    let syntax = ss
        .find_syntax_by_name(language)
        .or_else(|| ss.find_syntax_by_extension(language))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    // Use "Monokai" theme (dark, ships with syntect defaults — close to VS Code Dark+)
    let theme = ts
        .themes
        .get("Monokai")
        .or_else(|| ts.themes.values().next())
        .expect("syntect ships at least one theme");

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut result: Vec<Vec<HighlightSpan>> = Vec::new();

    for line in LinesWithEndings::from(source) {
        // Strip trailing newline characters from the token text we display
        let line_display: &str = line.trim_end_matches(&['\n', '\r'][..]);

        let ranges: Vec<SyntectRange<'_>> = match highlighter.highlight_line(line, ss) {
            Ok(r) => r,
            Err(_) => {
                // On error push the raw line as plain white text
                result.push(vec![HighlightSpan {
                    text: line_display.to_string(),
                    r: 0.85, g: 0.85, b: 0.85,
                }]);
                continue;
            }
        };

        let spans: Vec<HighlightSpan> = ranges
            .iter()
            .filter_map(|(style, text): &(Style, &str)| {
                // Remove trailing newline from the last token on each line
                let t: &str = text.trim_end_matches(&['\n', '\r'][..]);
                if t.is_empty() {
                    None
                } else {
                    Some(HighlightSpan::from_style_text(*style, t))
                }
            })
            .collect();

        result.push(spans);
    }

    if result.is_empty() {
        result.push(Vec::new());
    }

    result
}

// ─── Markdown-specific helpers ────────────────────────────────────────────────

/// Highlight Markdown source. Convenience wrapper around `highlight_source`.
pub fn highlight_markdown(source: &str) -> Vec<Vec<HighlightSpan>> {
    highlight_source(source, "Markdown")
}

/// Highlight Rune script source. Uses the Rust grammar since Rune syntax is
/// a strict subset of Rust (no lifetimes, no unsafe, same keywords/operators).
pub fn highlight_rune(source: &str) -> Vec<Vec<HighlightSpan>> {
    highlight_source(source, "Rust")
}

// ─── Flat export for Slint integration ────────────────────────────────────────

/// Flat span with a line index for use in Slint models.
#[derive(Debug, Clone)]
pub struct FlatSpan {
    /// 0-based line index
    pub line: usize,
    pub text: String,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// Convert `highlight_source` output into a flat list of `FlatSpan`s,
/// suitable for pushing into a Slint `VecModel`.
pub fn highlight_to_flat(source: &str, language: &str) -> Vec<FlatSpan> {
    highlight_source(source, language)
        .into_iter()
        .enumerate()
        .flat_map(|(line_idx, spans)| {
            spans.into_iter().map(move |s| FlatSpan {
                line: line_idx,
                text: s.text,
                r: s.r,
                g: s.g,
                b: s.b,
            })
        })
        .collect()
}

/// Line-based highlight data for Slint. Each entry contains the full text of
/// one source line plus a dominant color and emphasis flag.
#[derive(Debug, Clone)]
pub struct HighlightLine {
    pub text: String,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub bold: bool,
}

/// Convert token spans into one highlight entry per source line.
///
/// The dominant color is taken from the first non-empty token on the line.
/// Bold is enabled for Markdown headings and for lines whose first token uses
/// a bright accent color.
pub fn highlight_to_lines(source: &str, language: &str) -> Vec<HighlightLine> {
    highlight_source(source, language)
        .into_iter()
        .map(|spans| {
            let text = spans.iter().map(|span| span.text.as_str()).collect::<String>();
            let first_visible = spans.iter().find(|span| !span.text.trim().is_empty());

            let (r, g, b) = first_visible
                .map(|span| (span.r, span.g, span.b))
                .unwrap_or((0.85, 0.85, 0.85));

            let trimmed = text.trim_start();
            let bold = language.eq_ignore_ascii_case("Markdown")
                && (trimmed.starts_with('#') || trimmed.starts_with("```"));

            HighlightLine { text, r, g, b, bold }
        })
        .collect()
}
