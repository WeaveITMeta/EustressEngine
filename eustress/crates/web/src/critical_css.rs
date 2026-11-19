//! The CSS a prerendered page needs for its first paint.
//!
//! Every page links the whole site's stylesheets (about 170 KB compressed),
//! and a browser paints nothing until all of them have arrived. The prerender
//! (src/bin/prerender.rs) therefore writes into each page only the rules that
//! can match that page's static copy, inline, and parks the full stylesheets
//! (their address in `data-href`), so nothing but the page and its own images
//! loads before the first paint. assets/loader.js puts the addresses back
//! (`media="all"`) before the live app mounts, and `mount_app` in lib.rs does
//! the same in case loader.js never ran. On all 54 prerendered pages the
//! inline CSS alone draws the static copy pixel for pixel as the full
//! stylesheets do, at phone and desktop width.
//!
//! A rule is left out only when a selector names a class that appears nowhere
//! in the page. Classes inside `:not()`, `:is()`, `:where()`, `:has()` and the
//! like, and anything in attribute brackets, never cause that, and at-rules
//! other than `@media`-style blocks are kept whole. So the inline CSS keeps
//! everything that can match the static copy, in the stylesheets' own order,
//! and the cascade on it is the cascade of the full stylesheets.

use std::collections::HashSet;

/// Every class name in the page's `class` attributes.
pub fn classes_in_html(html: &str) -> HashSet<String> {
    let mut present = HashSet::new();
    for quote in ['"', '\''] {
        let open = format!(" class={quote}");
        let mut rest = html;
        while let Some(at) = rest.find(&open) {
            let value = &rest[at + open.len()..];
            let Some(end) = value.find(quote) else { break };
            present.extend(value[..end].split_whitespace().map(str::to_string));
            rest = &value[end..];
        }
    }
    present
}

/// The rules of `css` that can match a page whose class names are `present`,
/// in their order, comments and whitespace between rules dropped.
pub fn critical(css: &str, present: &HashSet<String>) -> String {
    let css = strip_comments(css);
    let mut out = String::new();
    filter(&css, present, &mut out);
    out
}

/// Byte index just past the string literal that opens at `i`.
fn end_of_string(s: &[u8], i: usize) -> usize {
    let quote = s[i];
    let mut j = i + 1;
    while j < s.len() && s[j] != quote {
        if s[j] == b'\\' {
            j += 1;
        }
        j += 1;
    }
    (j + 1).min(s.len())
}

fn strip_comments(css: &str) -> String {
    let s = css.as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'"' | b'\'' => {
                let j = end_of_string(s, i);
                out.extend_from_slice(&s[i..j]);
                i = j;
            }
            b'/' if s.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(s.len(), |k| i + 2 + k + 2);
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The next `{` or `;` at this level from `i`, skipping strings, brackets and parentheses.
fn next_stop(s: &[u8], mut i: usize) -> Option<usize> {
    let (mut paren, mut bracket) = (0i32, 0i32);
    while i < s.len() {
        match s[i] {
            b'"' | b'\'' => {
                i = end_of_string(s, i);
                continue;
            }
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            b'{' | b';' if paren == 0 && bracket == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index just past the block whose `{` is at `open`.
fn end_of_block(s: &[u8], open: usize) -> usize {
    let mut depth = 0;
    let mut i = open;
    while i < s.len() {
        match s[i] {
            b'"' | b'\'' => {
                i = end_of_string(s, i);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    s.len()
}

/// At-rules whose block holds rules to filter in turn.
fn recurses(prelude: &str) -> bool {
    let name = prelude[1..].split(|c: char| c.is_whitespace() || c == '(' || c == '{').next().unwrap_or("").to_ascii_lowercase();
    matches!(name.as_str(), "media" | "supports" | "container" | "layer" | "document" | "-moz-document")
}

fn filter(css: &str, present: &HashSet<String>, out: &mut String) {
    let s = css.as_bytes();
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        let Some(stop) = next_stop(s, i) else { break };
        let prelude = css[i..stop].trim();
        if s[stop] == b';' {
            if prelude.starts_with('@') {
                out.push_str(prelude);
                out.push(';');
            }
            i = stop + 1;
            continue;
        }
        let end = end_of_block(s, stop);
        let body = &css[stop + 1..end.saturating_sub(1).max(stop + 1)];
        if prelude.starts_with('@') {
            if recurses(prelude) {
                let mut inner = String::new();
                filter(body, present, &mut inner);
                if !inner.is_empty() {
                    out.push_str(prelude);
                    out.push('{');
                    out.push_str(&inner);
                    out.push('}');
                }
            } else {
                // @font-face, @keyframes, @property and any at-rule not known here: kept whole.
                out.push_str(prelude);
                out.push('{');
                out.push_str(body);
                out.push('}');
            }
        } else if split_selectors(prelude).iter().any(|sel| required_classes(sel).iter().all(|c| present.contains(c))) {
            out.push_str(prelude);
            out.push('{');
            out.push_str(body.trim());
            out.push('}');
        }
        i = end;
    }
}

/// A selector list split at its top-level commas.
fn split_selectors(list: &str) -> Vec<&str> {
    let s = list.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut start, mut i) = (0i32, 0, 0);
    while i < s.len() {
        match s[i] {
            b'"' | b'\'' => {
                i = end_of_string(s, i);
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&list[start..]);
    parts
}

/// The classes a selector needs present: every `.name` outside brackets and
/// outside parentheses (`:not(.a)` matches without `.a`; `:is(.a, .b)` needs
/// only one of them), unescaped.
fn required_classes(sel: &str) -> Vec<String> {
    let s = sel.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'"' | b'\'' => {
                i = end_of_string(s, i);
                continue;
            }
            open @ (b'[' | b'(') => {
                let close = if open == b'[' { b']' } else { b')' };
                let mut depth = 0;
                while i < s.len() {
                    if s[i] == open {
                        depth += 1;
                    } else if s[i] == close {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    i += 1;
                }
                continue;
            }
            b'.' if s.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, b'_' | b'-' | b'\\')) => {
                let mut name = String::new();
                let mut j = i + 1;
                while j < s.len() {
                    match s[j] {
                        b'\\' if j + 1 < s.len() => {
                            name.push(s[j + 1] as char);
                            j += 2;
                        }
                        c if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c >= 0x80 => {
                            name.push(c as char);
                            j += 1;
                        }
                        _ => break,
                    }
                }
                out.push(name);
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The attribute that marks a stylesheet loaded behind the first paint.
pub const DEFERRED: &str = "data-defer-css";

/// The `href` of every `<link rel="stylesheet">` in the page, in order.
pub fn stylesheet_hrefs(page: &str) -> Vec<String> {
    let mut hrefs = Vec::new();
    let mut rest = page;
    while let Some(at) = rest.find(r#"<link rel="stylesheet" href=""#) {
        let value = &rest[at + r#"<link rel="stylesheet" href=""#.len()..];
        let Some(end) = value.find('"') else { break };
        hrefs.push(value[..end].to_string());
        rest = &value[end..];
    }
    hrefs
}

/// The page with `critical` inline before its first stylesheet, and every
/// `<link rel="stylesheet">` parked: its address moved to `data-href`, so
/// nothing is fetched before the first paint (a `media="print"` sheet would
/// still download, and compete with the page's own images), with
/// `media="print"` and `DEFERRED` until loader.js puts the address back.
/// Unchanged when the page has no stylesheet link or is already done.
pub fn inline_critical(page: &str, critical: &str) -> String {
    let marker = r#"<link rel="stylesheet" href=""#;
    if page.contains(DEFERRED) || !page.contains(marker) {
        return page.to_string();
    }
    let mut out = String::with_capacity(page.len() + critical.len() + 64);
    let mut rest = page;
    let mut first = true;
    while let Some(at) = rest.find(marker) {
        out.push_str(&rest[..at]);
        if first {
            // `</style` cannot occur in CSS that parsed as rules, but a stray one would end the element early.
            out.push_str("<style data-critical>");
            out.push_str(&critical.replace("</style", "<\\/style"));
            out.push_str("</style>");
            first = false;
        }
        let tag_len = rest[at..].find('>').map_or(rest.len() - at, |k| k + 1);
        let tag = rest[at..at + tag_len].replacen(" href=\"", " data-href=\"", 1);
        let close = if tag.ends_with("/>") { tag.len() - 2 } else { tag.len() - 1 };
        out.push_str(&tag[..close]);
        out.push_str(&format!(r#" media="print" {DEFERRED}"#));
        out.push_str(&tag[close..]);
        rest = &rest[at + tag_len..];
    }
    out.push_str(rest);
    out
}

/// `inline_critical` undone: the page as Trunk linked its stylesheets. The
/// prerender writes `/` over index.html, which is also the shell the next run
/// reads, so this keeps one page's critical CSS from reaching every other page.
pub fn strip_critical(page: &str) -> String {
    let mut out = page.replace(&format!(r#" media="print" {DEFERRED}"#), "").replace(r#"<link rel="stylesheet" data-href=""#, r#"<link rel="stylesheet" href=""#);
    while let Some(start) = out.find("<style data-critical>") {
        let Some(len) = out[start..].find("</style>") else { break };
        out.replace_range(start..start + len + "</style>".len(), "");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_rule_goes_only_when_it_names_a_class_the_page_lacks() {
        let css = "/* c */ body{margin:0} .hero{color:red} .gone{color:blue} .hero .gone{x:1} .a,.gone{y:2} h1.hero:hover{z:3}";
        let out = critical(css, &set(&["hero", "a"]));
        assert_eq!(out, "body{margin:0}.hero{color:red}.a,.gone{y:2}h1.hero:hover{z:3}");
    }

    #[test]
    fn negations_brackets_and_strings_never_drop_a_rule() {
        let present = set(&["card"]);
        for kept in [".card:not(.open){a:1}", ":is(.x,.card){a:1}", ".card:where(.y){a:1}", "[data-x=\".missing\"]{a:1}", ".card::after{content:\".nope\"}"] {
            assert_eq!(critical(kept, &present), kept, "{kept}");
        }
        assert_eq!(critical(".missing:not(.card){a:1}", &present), "");
    }

    #[test]
    fn media_blocks_are_filtered_inside_and_dropped_when_empty_while_other_at_rules_stay_whole() {
        let css = "@media (max-width:768px){.hero{a:1}.gone{b:2}}@media print{.gone{c:3}}@keyframes rise{from{opacity:0}to{opacity:1}}\
                   @font-face{font-family:'X';src:url(/x.woff2)}@supports (display:grid){.gone{d:4}}@import url(\"y.css\");";
        let out = critical(css, &set(&["hero"]));
        assert_eq!(out, "@media (max-width:768px){.hero{a:1}}@keyframes rise{from{opacity:0}to{opacity:1}}@font-face{font-family:'X';src:url(/x.woff2)}@import url(\"y.css\");");
    }

    #[test]
    fn braces_and_semicolons_inside_strings_do_not_break_the_walk() {
        let css = ".a::before{content:\"{;}\"}.b{background:url(\"a;b{c}.png\")}.c{d:1}";
        assert_eq!(critical(css, &set(&["a", "b"])), ".a::before{content:\"{;}\"}.b{background:url(\"a;b{c}.png\")}");
    }

    #[test]
    fn the_loader_and_the_app_switch_the_same_stylesheets_on() {
        let loader = include_str!("../assets/loader.js");
        assert!(loader.contains(&format!("link[{DEFERRED}]")), "loader.js enables what the prerender deferred");
        assert!(loader.contains("'data-href'"), "and puts each address back");
        let lib = include_str!("lib.rs");
        assert!(lib.contains("critical_css::DEFERRED") && lib.contains("\"data-href\""), "mount_app does too");
    }

    #[test]
    fn classes_come_from_both_quote_styles() {
        let present = classes_in_html(r#"<div class="page  page-home"><a class='btn primary'>x</a><p classic="no">"#);
        assert_eq!(present, set(&["page", "page-home", "btn", "primary"]));
    }

    #[test]
    fn the_stylesheets_load_behind_the_paint_with_the_critical_css_first() {
        let page = "<head><link rel=\"stylesheet\" href=\"/a-1.css\" integrity=\"sha384-x\"/><link rel=\"stylesheet\" href=\"/b-2.css\"></head>";
        assert_eq!(stylesheet_hrefs(page), ["/a-1.css", "/b-2.css"]);
        let out = inline_critical(page, ".a{b:1}");
        assert_eq!(
            out,
            "<head><style data-critical>.a{b:1}</style><link rel=\"stylesheet\" data-href=\"/a-1.css\" integrity=\"sha384-x\" media=\"print\" data-defer-css/>\
             <link rel=\"stylesheet\" data-href=\"/b-2.css\" media=\"print\" data-defer-css></head>"
        );
        assert!(stylesheet_hrefs(&out).is_empty(), "nothing is fetched before loader.js puts the addresses back");
        assert_eq!(inline_critical(&out, ".a{b:1}"), out, "a second run changes nothing");
        assert_eq!(strip_critical(&out), page, "and it undoes cleanly");
    }
}
