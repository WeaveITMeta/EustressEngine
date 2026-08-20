//! # Splitting chat content into renderable blocks
//!
//! Workshop replies arrive as one string of loose markdown, and the chat bubble
//! renders it as one read-only `TextInput`. That is fine for prose and wrong for
//! a table: a pipe table shows up as raw `| Vendor | Country |` rows with a
//! `|---|---|` rule under them, which is the least readable possible form of the
//! most structured thing the model produces.
//!
//! This module splits a message into an ordered run of [`Block`]s so the panel
//! can draw prose as text and tables as a grid. It deliberately does NOT
//! implement markdown. It finds tables and leaves everything else alone, because
//! every other markdown feature already reads acceptably as plain text and a
//! half-finished markdown renderer is worse than none.
//!
//! ## What counts as a table
//!
//! GitHub-flavoured pipe tables, which is what the model actually emits:
//!
//! ```text
//! | Vendor | Country | Lead time |
//! |---|---|---|
//! | Desert Light Alloys | US | 21 d |
//! ```
//!
//! A header row alone is not enough. The line after it must be a delimiter row,
//! every cell of which is dashes with optional leading or trailing colons. That
//! second-line requirement is what stops ordinary prose containing a pipe from
//! being swallowed into a table, which is the failure mode a looser detector
//! has.
//!
//! Leading and trailing pipes are optional, as in GFM. Ragged rows are padded or
//! truncated to the header width, because a grid with a short row draws a hole
//! and the alternative, refusing to render, loses the user their data.

/// One renderable run of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Prose, code fences, lists: anything not a table. Rendered verbatim.
    Text(String),
    Table(Table),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub headers: Vec<String>,
    /// Each row is exactly `headers.len()` long.
    pub rows: Vec<Vec<String>>,
}

/// Split a pipe-table row into its cells.
///
/// Returns `None` when the line cannot be a table row at all, which is the
/// cheap first test before the more expensive delimiter check.
fn split_row(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    if !t.contains('|') {
        return None;
    }
    // Optional leading and trailing pipes, per GFM.
    let inner = t.strip_prefix('|').unwrap_or(t);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let cells: Vec<String> = inner.split('|').map(|c| c.trim().to_string()).collect();
    if cells.len() < 2 {
        return None;
    }
    Some(cells)
}

/// Is this the `|---|:--:|` rule under a header?
///
/// This is the whole reason prose with a pipe in it does not become a table.
fn is_delimiter(line: &str) -> bool {
    match split_row(line) {
        None => false,
        Some(cells) => cells.iter().all(|c| {
            let c = c.trim();
            let body = c.trim_start_matches(':').trim_end_matches(':');
            !body.is_empty() && body.chars().all(|ch| ch == '-')
        }),
    }
}

/// Split a message into prose and tables, preserving order.
///
/// A message with no table comes back as exactly one [`Block::Text`], so the
/// caller can treat the no-table case as the common path without branching.
pub fn parse_blocks(content: &str) -> Vec<Block> {
    let lines: Vec<&str> = content.lines().collect();
    let mut blocks = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut i = 0;

    // Prose accumulated so far becomes a Text block, unless it is only
    // whitespace, which would render as an empty gap.
    fn flush(prose: &mut Vec<&str>, blocks: &mut Vec<Block>) {
        if !prose.is_empty() {
            let joined = prose.join("\n");
            if !joined.trim().is_empty() {
                blocks.push(Block::Text(joined.trim_matches('\n').to_string()));
            }
            prose.clear();
        }
    }

    while i < lines.len() {
        let header = split_row(lines[i]);
        let delimited = i + 1 < lines.len() && is_delimiter(lines[i + 1]);

        match (header, delimited) {
            (Some(headers), true) => {
                flush(&mut prose, &mut blocks);
                let width = headers.len();
                let mut rows = Vec::new();
                i += 2; // consume header + delimiter
                while i < lines.len() {
                    match split_row(lines[i]) {
                        Some(mut cells) => {
                            // Pad or truncate to the header width. A ragged row
                            // is common in generated markdown and dropping it
                            // would lose the user a line of real data.
                            cells.resize(width, String::new());
                            rows.push(cells);
                            i += 1;
                        }
                        None => break,
                    }
                }
                blocks.push(Block::Table(Table { headers, rows }));
            }
            _ => {
                prose.push(lines[i]);
                i += 1;
            }
        }
    }
    flush(&mut prose, &mut blocks);

    if blocks.is_empty() {
        blocks.push(Block::Text(String::new()));
    }
    blocks
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn table_of(blocks: &[Block]) -> &Table {
        blocks
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("expected a table block")
    }

    #[test]
    fn plain_prose_is_one_text_block() {
        let b = parse_blocks("Just a sentence.\nAnd another.");
        assert_eq!(b.len(), 1);
        assert!(matches!(b[0], Block::Text(_)));
    }

    #[test]
    fn empty_content_still_yields_one_block() {
        // The renderer iterates blocks; zero blocks would draw nothing at all
        // and lose the bubble entirely.
        assert_eq!(parse_blocks(""), vec![Block::Text(String::new())]);
    }

    #[test]
    fn the_vendor_table_from_workshop_parses() {
        // Copied from a real Workshop reply, leading and trailing pipes included.
        let msg = "\
What you have today are two aluminum-capable vendors:

| Vendor | Country | Processes | Lead time | Audit | OTD |
|---|---|---|---|---|---|
| Desert Light Alloys | US (SW) | extrusion, CNC mill, anodizing | 21 d | 94 | 97.4% |
| Precision Contact Works | MX | CNC turning, plating, passivation | 18 d | 96 | 98.0% |

They serve different nodes, though.";
        let blocks = parse_blocks(msg);
        assert_eq!(blocks.len(), 3, "prose, table, prose");
        assert!(matches!(blocks[0], Block::Text(_)));
        assert!(matches!(blocks[2], Block::Text(_)));

        let t = table_of(&blocks);
        assert_eq!(t.headers.len(), 6);
        assert_eq!(t.headers[0], "Vendor");
        assert_eq!(t.headers[5], "OTD");
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][0], "Desert Light Alloys");
        assert_eq!(t.rows[1][5], "98.0%");
    }

    #[test]
    fn prose_containing_a_pipe_is_not_a_table() {
        // This is the case a looser detector gets wrong, and it would eat the
        // sentence into a one-row grid.
        let b = parse_blocks("Run `ls | grep foo` and check | the output.");
        assert_eq!(b.len(), 1);
        assert!(matches!(b[0], Block::Text(_)));
    }

    #[test]
    fn a_header_without_a_delimiter_stays_prose() {
        let b = parse_blocks("| a | b |\nnot a delimiter\n| 1 | 2 |");
        assert_eq!(b.len(), 1);
        assert!(matches!(b[0], Block::Text(_)));
    }

    #[test]
    fn pipes_are_optional_at_the_edges() {
        let b = parse_blocks("a | b\n--- | ---\n1 | 2");
        let t = table_of(&b);
        assert_eq!(t.headers, vec!["a", "b"]);
        assert_eq!(t.rows, vec![vec!["1".to_string(), "2".to_string()]]);
    }

    #[test]
    fn alignment_colons_are_accepted() {
        let b = parse_blocks("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |");
        assert_eq!(table_of(&b).rows[0], vec!["1", "2", "3"]);
    }

    #[test]
    fn a_ragged_row_is_padded_rather_than_dropped() {
        let b = parse_blocks("| a | b | c |\n|---|---|---|\n| 1 | 2 |\n| 1 | 2 | 3 | 4 |");
        let t = table_of(&b);
        assert_eq!(t.rows.len(), 2, "neither row is dropped");
        assert_eq!(t.rows[0], vec!["1", "2", ""], "short row padded");
        assert_eq!(t.rows[1], vec!["1", "2", "3"], "long row truncated");
    }

    #[test]
    fn a_table_with_no_body_rows_is_still_a_table() {
        let b = parse_blocks("| a | b |\n|---|---|");
        let t = table_of(&b);
        assert_eq!(t.headers.len(), 2);
        assert!(t.rows.is_empty());
    }

    #[test]
    fn two_tables_in_one_message_both_parse() {
        // Two columns each. A single-column "table" is deliberately not one,
        // per `single_column_pipes_do_not_qualify` below.
        let msg = "| a | b |\n|---|---|\n| 1 | 2 |\n\ntext between\n\n| c | d |\n|---|---|\n| 3 | 4 |";
        let blocks = parse_blocks(msg);
        let tables: Vec<_> = blocks
            .iter()
            .filter(|b| matches!(b, Block::Table(_)))
            .collect();
        assert_eq!(tables.len(), 2);
    }

    #[test]
    fn a_table_at_the_very_start_needs_no_leading_prose() {
        let b = parse_blocks("| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(b.len(), 1);
        assert!(matches!(b[0], Block::Table(_)));
    }

    #[test]
    fn single_column_pipes_do_not_qualify() {
        // "| x |" is one cell; a table needs at least two columns or every
        // bulleted line with a trailing pipe becomes a grid.
        let b = parse_blocks("| x |\n|---|\n| y |");
        assert!(matches!(b[0], Block::Text(_)));
    }
}
