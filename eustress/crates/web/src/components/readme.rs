// =============================================================================
// Eustress Web - A listing's README, drawn under its About tab
// =============================================================================
// The creator's README.md from the Website service, as the Worker serves it after review (docs/design/SIM_PAGE_README_AND_TABS.md,
// section 2). The Markdown is parsed with pulldown-cmark into events, the events into a small tree from a fixed set of nodes, and
// the tree into Leptos elements. The page never sets inner_html:
//
// - Raw HTML (block and inline) is dropped. Text between dropped tags stays text.
// - A link is an <a> only when its destination is https, is in `readme.links`, and has a domain a link may have; it opens in a new
//   tab with `nofollow noopener noreferrer ugc` and shows its domain. Any other link is its text.
// - An image is an <img> only when its source is in `readme.images` and resolves to the API's own origin (`image_src`); any other
//   image is its alt text.
// - `#` is drawn as a second-level heading (and each level one lower), so the page keeps its one title.
// - Anything else the parser could produce is its text. Nesting deeper than a fixed bound is flattened to its text, so a hostile
//   file cannot make the drawing recurse without limit.
//
// Table of Contents:
// 1. The tree (pure, tested)
// 2. Drawing it
// =============================================================================

use leptos::prelude::*;
use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag};

use crate::api::sim_page::{display_domain, image_src, Readme};

// -----------------------------------------------------------------------------
// 1. The tree
// -----------------------------------------------------------------------------

/// How deep containers may nest before the rest is flattened to text.
const MAX_DEPTH: usize = 24;

/// A table column's alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// One piece of a drawn README. Nothing else can be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Text(String),
    Paragraph(Vec<Node>),
    /// 2 to 6.
    Heading { level: u8, children: Vec<Node> },
    Emphasis(Vec<Node>),
    Strong(Vec<Node>),
    Strike(Vec<Node>),
    /// Inline code.
    Code(String),
    /// A fenced or indented code block, its text exactly as written.
    CodeBlock(String),
    /// `start` is the first number of an ordered list; an unordered list has none.
    List { start: Option<u64>, items: Vec<Vec<Node>> },
    Quote(Vec<Node>),
    Table { align: Vec<Align>, head: Vec<Vec<Node>>, rows: Vec<Vec<Vec<Node>>> },
    Rule,
    SoftBreak,
    HardBreak,
    /// An approved https link and the domain shown beside it.
    Link { href: String, domain: String, children: Vec<Node> },
    /// An approved image, its address already on the API's own origin.
    Image { src: String, alt: String },
}

/// What an open container will become when it closes.
enum Kind {
    Root,
    Paragraph,
    Heading(u8),
    Emphasis,
    Strong,
    Strike,
    CodeBlock,
    List(Option<u64>),
    Item,
    Quote,
    Table(Vec<Align>),
    TableHead,
    TableRow,
    TableCell,
    /// `Some` when the link may be drawn: its address and domain.
    Link(Option<(String, String)>),
    /// `Some` when the image may be drawn: its address.
    Image(Option<String>),
    /// Raw HTML: everything inside is dropped.
    Drop,
    /// Not drawn as itself: its children go to the parent. A block adds a break after them so words do not run together.
    Pass { block: bool },
}

struct Frame {
    kind: Kind,
    children: Vec<Node>,
    /// List items, or a table row's (or head's) cells.
    parts: Vec<Vec<Node>>,
    head: Vec<Vec<Node>>,
    rows: Vec<Vec<Vec<Node>>>,
}

impl Frame {
    fn new(kind: Kind) -> Self {
        Frame { kind, children: Vec::new(), parts: Vec::new(), head: Vec::new(), rows: Vec::new() }
    }
}

/// Add a node, joining text to the text before it.
fn push(nodes: &mut Vec<Node>, node: Node) {
    if let Node::Text(t) = &node {
        if t.is_empty() {
            return;
        }
        if let Some(Node::Text(last)) = nodes.last_mut() {
            last.push_str(t);
            return;
        }
    }
    nodes.push(node);
}

/// The plain text of some nodes: what an image's alt text is made of.
pub fn plain_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    fn walk(nodes: &[Node], out: &mut String) {
        for n in nodes {
            match n {
                Node::Text(t) | Node::Code(t) | Node::CodeBlock(t) => out.push_str(t),
                Node::SoftBreak | Node::HardBreak => out.push(' '),
                Node::Image { alt, .. } => out.push_str(alt),
                Node::Paragraph(c) | Node::Emphasis(c) | Node::Strong(c) | Node::Strike(c) | Node::Quote(c) => walk(c, out),
                Node::Heading { children, .. } | Node::Link { children, .. } => walk(children, out),
                Node::List { items, .. } => items.iter().for_each(|i| walk(i, out)),
                Node::Table { head, rows, .. } => {
                    head.iter().for_each(|c| walk(c, out));
                    rows.iter().flatten().for_each(|c| walk(c, out));
                }
                Node::Rule => {}
            }
        }
    }
    walk(nodes, &mut out);
    out
}

/// No space, control character, quote, bracket or backslash: the same address rule as the Links card.
fn clean_address(url: &str) -> bool {
    !url.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`'))
}

/// The address and domain of a link the page may draw: https, in the approved list, and a domain a link may have.
fn approved_link(dest: &str, readme: &Readme) -> Option<(String, String)> {
    if !dest.starts_with("https://") || dest.len() > 500 || !clean_address(dest) || !readme.links.iter().any(|l| l == dest) {
        return None;
    }
    display_domain(dest).map(|domain| (dest.to_string(), domain))
}

/// The address of an image the page may draw: in the approved list, and on the API's own origin.
fn approved_image(dest: &str, readme: &Readme, api_base: &str) -> Option<String> {
    if !readme.images.iter().any(|i| i == dest) {
        return None;
    }
    image_src(dest, api_base)
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 2,
        HeadingLevel::H2 => 3,
        HeadingLevel::H3 => 4,
        HeadingLevel::H4 => 5,
        HeadingLevel::H5 | HeadingLevel::H6 => 6,
    }
}

fn align_of(a: &Alignment) -> Align {
    match a {
        Alignment::None => Align::None,
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
    }
}

/// Whether a tag the page does not draw as itself is a block (its text is followed by a break).
fn is_block(tag: &Tag) -> bool {
    !matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Superscript | Tag::Subscript | Tag::Link { .. } | Tag::Image { .. }
    )
}

struct Builder<'r> {
    readme: &'r Readme,
    api_base: &'r str,
    stack: Vec<Frame>,
}

impl Builder<'_> {
    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("the root frame is never closed")
    }

    fn open(&mut self, tag: Tag) {
        let depth = self.stack.len();
        let parent = &self.stack.last().expect("the root frame").kind;
        let kind = match tag {
            // Raw HTML, a code block and an image hold only text, so they never deepen the tree and are never flattened.
            Tag::HtmlBlock => Kind::Drop,
            Tag::CodeBlock(_) => Kind::CodeBlock,
            Tag::Image { dest_url, .. } => Kind::Image(approved_image(&dest_url, self.readme, self.api_base)),
            // A list's items and a table's parts follow their container: drawn when it is drawn, flattened when it is flattened.
            Tag::Item if matches!(parent, Kind::List(_)) => Kind::Item,
            Tag::TableHead if matches!(parent, Kind::Table(_)) => Kind::TableHead,
            Tag::TableRow if matches!(parent, Kind::Table(_)) => Kind::TableRow,
            Tag::TableCell if matches!(parent, Kind::TableHead | Kind::TableRow) => Kind::TableCell,
            Tag::TableHead | Tag::TableRow => Kind::Pass { block: true },
            Tag::TableCell => Kind::Pass { block: false },
            ref t if depth > MAX_DEPTH => Kind::Pass { block: is_block(t) },
            Tag::Paragraph => Kind::Paragraph,
            Tag::Heading { level, .. } => Kind::Heading(heading_level(level)),
            Tag::Emphasis => Kind::Emphasis,
            Tag::Strong => Kind::Strong,
            Tag::Strikethrough => Kind::Strike,
            Tag::List(start) => Kind::List(start),
            Tag::Item => Kind::Item,
            Tag::BlockQuote(_) => Kind::Quote,
            Tag::Table(align) => Kind::Table(align.iter().map(align_of).collect()),
            Tag::Link { dest_url, .. } => Kind::Link(approved_link(&dest_url, self.readme)),
            ref other => Kind::Pass { block: is_block(other) },
        };
        self.stack.push(Frame::new(kind));
    }

    fn close(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let frame = self.stack.pop().expect("more than the root");
        let Frame { kind, children, parts, head, rows } = frame;
        let parent = self.top();
        let node = match kind {
            Kind::Root | Kind::Drop => return,
            Kind::Paragraph => Node::Paragraph(children),
            Kind::Heading(level) => Node::Heading { level, children },
            Kind::Emphasis => Node::Emphasis(children),
            Kind::Strong => Node::Strong(children),
            Kind::Strike => Node::Strike(children),
            Kind::CodeBlock => Node::CodeBlock(plain_text(&children)),
            Kind::List(start) => Node::List { start, items: parts },
            Kind::Quote => Node::Quote(children),
            Kind::Table(align) => Node::Table { align, head, rows },
            Kind::TableCell => {
                parent.parts.push(children);
                return;
            }
            Kind::TableHead => {
                parent.head = parts;
                return;
            }
            Kind::TableRow => {
                parent.rows.push(parts);
                return;
            }
            Kind::Item => {
                if matches!(parent.kind, Kind::List(_)) {
                    parent.parts.push(children);
                } else {
                    children.into_iter().for_each(|n| push(&mut parent.children, n));
                }
                return;
            }
            Kind::Link(Some((href, domain))) => Node::Link { href, domain, children },
            Kind::Link(None) => {
                children.into_iter().for_each(|n| push(&mut parent.children, n));
                return;
            }
            Kind::Image(src) => {
                let alt = plain_text(&children).trim().to_string();
                match src {
                    Some(src) => Node::Image { src, alt },
                    None => Node::Text(alt),
                }
            }
            Kind::Pass { block } => {
                children.into_iter().for_each(|n| push(&mut parent.children, n));
                if block {
                    push(&mut parent.children, Node::SoftBreak);
                }
                return;
            }
        };
        push(&mut parent.children, node);
    }

    fn leaf(&mut self, node: Node) {
        push(&mut self.top().children, node);
    }
}

/// The README as the tree the page draws. Pure: no page, no network. `api_base` is the API's address, the only origin an image may
/// come from.
pub fn readme_tree(readme: &Readme, api_base: &str) -> Vec<Node> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let mut b = Builder { readme, api_base, stack: vec![Frame::new(Kind::Root)] };
    for event in Parser::new_ext(&readme.markdown, options) {
        match event {
            Event::Start(tag) => b.open(tag),
            Event::End(_) => b.close(),
            Event::Text(t) => b.leaf(Node::Text(t.into_string())),
            Event::Code(t) => b.leaf(Node::Code(t.into_string())),
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::SoftBreak => b.leaf(Node::SoftBreak),
            Event::HardBreak => b.leaf(Node::HardBreak),
            Event::Rule => b.leaf(Node::Rule),
            // Not turned on, so not expected; drawn as their text if they ever come.
            Event::InlineMath(t) | Event::DisplayMath(t) | Event::FootnoteReference(t) => b.leaf(Node::Text(t.into_string())),
            Event::TaskListMarker(done) => b.leaf(Node::Text(if done { "[x] " } else { "[ ] " }.to_string())),
        }
    }
    // A well-formed stream closes everything; close whatever is left all the same.
    while b.stack.len() > 1 {
        b.close();
    }
    let root = b.stack.pop().expect("the root frame");
    root.children
}

/// Whether the README draws anything at all.
pub fn has_readme(readme: &Readme) -> bool {
    !readme.markdown.trim().is_empty() && !readme_tree(readme, "").is_empty()
}

// -----------------------------------------------------------------------------
// 2. Drawing it
// -----------------------------------------------------------------------------

fn align_class(align: &[Align], column: usize) -> &'static str {
    match align.get(column).copied().unwrap_or(Align::None) {
        Align::None => "",
        Align::Left => "rm-left",
        Align::Center => "rm-center",
        Align::Right => "rm-right",
    }
}

fn nodes_view(nodes: Vec<Node>) -> AnyView {
    nodes.into_iter().map(node_view).collect_view().into_any()
}

fn node_view(node: Node) -> AnyView {
    match node {
        Node::Text(t) => t.into_any(),
        Node::SoftBreak => "\n".into_any(),
        Node::HardBreak => view! { <br /> }.into_any(),
        Node::Rule => view! { <hr /> }.into_any(),
        Node::Paragraph(c) => view! { <p>{nodes_view(c)}</p> }.into_any(),
        Node::Heading { level, children } => {
            let c = nodes_view(children);
            match level {
                2 => view! { <h2>{c}</h2> }.into_any(),
                3 => view! { <h3>{c}</h3> }.into_any(),
                4 => view! { <h4>{c}</h4> }.into_any(),
                5 => view! { <h5>{c}</h5> }.into_any(),
                _ => view! { <h6>{c}</h6> }.into_any(),
            }
        }
        Node::Emphasis(c) => view! { <em>{nodes_view(c)}</em> }.into_any(),
        Node::Strong(c) => view! { <strong>{nodes_view(c)}</strong> }.into_any(),
        Node::Strike(c) => view! { <del>{nodes_view(c)}</del> }.into_any(),
        Node::Code(t) => view! { <code>{t}</code> }.into_any(),
        Node::CodeBlock(t) => view! { <pre class="rm-pre" tabindex="0"><code>{t}</code></pre> }.into_any(),
        Node::List { start, items } => {
            let items = items.into_iter().map(|i| view! { <li>{nodes_view(i)}</li> }).collect_view();
            match start {
                Some(n) => view! { <ol start=n.to_string()>{items}</ol> }.into_any(),
                None => view! { <ul>{items}</ul> }.into_any(),
            }
        }
        Node::Quote(c) => view! { <blockquote>{nodes_view(c)}</blockquote> }.into_any(),
        Node::Table { align, head, rows } => {
            let head = head
                .into_iter()
                .enumerate()
                .map(|(i, c)| view! { <th class=align_class(&align, i)>{nodes_view(c)}</th> })
                .collect_view();
            let rows = rows
                .into_iter()
                .map(|row| {
                    let cells = row
                        .into_iter()
                        .enumerate()
                        .map(|(i, c)| view! { <td class=align_class(&align, i)>{nodes_view(c)}</td> })
                        .collect_view();
                    view! { <tr>{cells}</tr> }
                })
                .collect_view();
            view! {
                <div class="rm-table-wrap" tabindex="0">
                    <table>
                        <thead><tr>{head}</tr></thead>
                        <tbody>{rows}</tbody>
                    </table>
                </div>
            }
            .into_any()
        }
        Node::Link { href, domain, children } => view! {
            <a class="rm-link" href=href target="_blank" rel="nofollow noopener noreferrer ugc">
                {nodes_view(children)}
                <span class="rm-ldom">{domain}</span>
            </a>
        }
        .into_any(),
        Node::Image { src, alt } => view! { <img class="rm-img" src=src alt=alt loading="lazy" decoding="async" /> }.into_any(),
    }
}

/// The README, drawn: centered, at reading width, in the README look. Nothing when it draws nothing.
pub fn readme_view(readme: &Readme, api_base: &str) -> AnyView {
    let nodes = readme_tree(readme, api_base);
    if nodes.is_empty() {
        return ().into_any();
    }
    view! { <article class="rm-body">{nodes_view(nodes)}</article> }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = "https://api.example.dev";

    fn readme(markdown: &str, links: &[&str], images: &[&str]) -> Readme {
        Readme {
            markdown: markdown.to_string(),
            links: links.iter().map(|s| s.to_string()).collect(),
            images: images.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn tree(markdown: &str, links: &[&str], images: &[&str]) -> Vec<Node> {
        readme_tree(&readme(markdown, links, images), API)
    }

    /// Every node in the tree, depth first.
    fn all(nodes: &[Node]) -> Vec<Node> {
        let mut out = Vec::new();
        fn walk(nodes: &[Node], out: &mut Vec<Node>) {
            for n in nodes {
                out.push(n.clone());
                match n {
                    Node::Paragraph(c) | Node::Emphasis(c) | Node::Strong(c) | Node::Strike(c) | Node::Quote(c) => walk(c, out),
                    Node::Heading { children, .. } | Node::Link { children, .. } => walk(children, out),
                    Node::List { items, .. } => items.iter().for_each(|i| walk(i, out)),
                    Node::Table { head, rows, .. } => {
                        head.iter().for_each(|c| walk(c, out));
                        rows.iter().flatten().for_each(|c| walk(c, out));
                    }
                    _ => {}
                }
            }
        }
        walk(nodes, &mut out);
        out
    }

    fn links_in(nodes: &[Node]) -> Vec<String> {
        all(nodes).into_iter().filter_map(|n| if let Node::Link { href, .. } = n { Some(href) } else { None }).collect()
    }

    fn images_in(nodes: &[Node]) -> Vec<String> {
        all(nodes).into_iter().filter_map(|n| if let Node::Image { src, .. } = n { Some(src) } else { None }).collect()
    }

    #[test]
    fn raw_html_produces_no_element_and_no_markup_reaches_the_text() {
        let md = "<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n<iframe src=\"https://evil.example\"></iframe>\n\nA line with <img src=x onerror=alert(2)> inline and <script>also()</script> and <iframe src=x></iframe>.\n\n<div onclick=\"x\">\n\nblock\n\n</div>";
        let t = tree(md, &["https://evil.example"], &["x"]);
        for n in all(&t) {
            assert!(matches!(n, Node::Paragraph(_) | Node::Text(_) | Node::SoftBreak), "only paragraphs of text: {n:?}");
        }
        let text = plain_text(&t);
        for bad in ["<script", "<img", "onerror", "<iframe", "<div", "onclick", "</"] {
            assert!(!text.contains(bad), "{bad} in {text:?}");
        }
        assert!(text.contains("A line with") && text.contains("block"), "the words outside the tags stay: {text:?}");
        assert!(links_in(&t).is_empty() && images_in(&t).is_empty());
    }

    #[test]
    fn only_an_approved_https_link_is_a_link_whatever_the_list_says() {
        // Every one of these is in the allow list, so only the scheme and address rules stop them.
        let bad = [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "http://example.com/plain",
            "//example.com/no-scheme",
            "https://user@example.com/",
            "https://127.0.0.1/",
            "https://example.com:8443/",
            "/relative/path",
            "mailto:a@example.com",
        ];
        for url in bad {
            let md = format!("[click]({url}) and <{url}>");
            let t = tree(&md, &[url], &[]);
            assert!(links_in(&t).is_empty(), "{url} became a link: {t:?}");
            assert!(plain_text(&t).contains("click"), "its text stays: {t:?}");
        }
        // An approved one is a link, with its domain beside it.
        let t = tree("See [our paper](https://www.example.edu/paper).", &["https://www.example.edu/paper"], &[]);
        let link = all(&t).into_iter().find(|n| matches!(n, Node::Link { .. })).expect("a link");
        assert_eq!(
            link,
            Node::Link { href: "https://www.example.edu/paper".into(), domain: "example.edu".into(), children: vec![Node::Text("our paper".into())] }
        );
        // An autolink and a reference link resolve to the same check.
        let t = tree("<https://a.example/x> and [ref][r]\n\n[r]: https://b.example/y", &["https://a.example/x", "https://b.example/y"], &[]);
        assert_eq!(links_in(&t), ["https://a.example/x", "https://b.example/y"]);
    }

    #[test]
    fn a_link_not_in_the_allow_list_is_its_text() {
        let t = tree("Go to [the **shop**](https://shop.example.com/kits) now", &["https://shop.example.com/other"], &[]);
        assert!(links_in(&t).is_empty(), "{t:?}");
        assert_eq!(
            t,
            vec![Node::Paragraph(vec![
                Node::Text("Go to the ".into()),
                Node::Strong(vec![Node::Text("shop".into())]),
                Node::Text(" now".into()),
            ])]
        );
        assert!(!plain_text(&t).contains("shop.example.com"));
    }

    #[test]
    fn an_image_not_in_the_allow_list_or_off_the_apis_origin_is_its_alt_text() {
        let ok = "/api/simulations/a/page/image/readme-0";
        let t = tree(&format!("![A cell]({ok})"), &[], &[ok]);
        assert_eq!(images_in(&t), [format!("{API}{ok}")]);
        assert_eq!(all(&t).into_iter().find(|n| matches!(n, Node::Image { .. })), Some(Node::Image { src: format!("{API}{ok}"), alt: "A cell".into() }));
        // Not in the list.
        let t = tree("![Tracking pixel](/api/simulations/a/page/image/readme-1)", &[], &[ok]);
        assert!(images_in(&t).is_empty());
        assert_eq!(plain_text(&t), "Tracking pixel");
        // In the list, but not from the API's own origin: still only its alt text.
        for src in ["https://evil.example/p.png", "http://api.example.dev/x.png", "data:image/png;base64,AAAA", "//evil.example/x.png"] {
            let t = tree(&format!("![Alt {src}]({src})"), &[], &[src]);
            assert!(images_in(&t).is_empty(), "{src}: {t:?}");
            assert!(plain_text(&t).starts_with("Alt "), "{src}: {t:?}");
        }
        // An approved image inside an approved link is both.
        let t = tree(&format!("[![Logo]({ok})](https://a.example/)"), &["https://a.example/"], &[ok]);
        assert_eq!(links_in(&t).len(), 1);
        assert_eq!(images_in(&t).len(), 1);
    }

    #[test]
    fn a_title_is_a_second_level_heading_and_each_level_moves_down_one() {
        let t = tree("# Title\n\n## Part\n\n###### Small", &[], &[]);
        assert_eq!(
            t,
            vec![
                Node::Heading { level: 2, children: vec![Node::Text("Title".into())] },
                Node::Heading { level: 3, children: vec![Node::Text("Part".into())] },
                Node::Heading { level: 6, children: vec![Node::Text("Small".into())] },
            ]
        );
    }

    #[test]
    fn a_table_keeps_its_cells_and_alignment() {
        let t = tree("| Part | Mass | Note |\n|:-----|-----:|:----:|\n| Cell | 45 g | *new* |\n| Pack | 2 kg | |\n", &[], &[]);
        let text = |s: &str| vec![Node::Text(s.into())];
        assert_eq!(
            t,
            vec![Node::Table {
                align: vec![Align::Left, Align::Right, Align::Center],
                head: vec![text("Part"), text("Mass"), text("Note")],
                rows: vec![
                    vec![text("Cell"), text("45 g"), vec![Node::Emphasis(text("new"))]],
                    vec![text("Pack"), text("2 kg"), vec![]],
                ],
            }]
        );
    }

    #[test]
    fn code_keeps_its_text_verbatim_and_html_in_code_is_text() {
        let fenced = "```rust\nfn main() {\n    let a = \"<script>x</script>\";  // two spaces\n}\n```";
        assert_eq!(tree(fenced, &[], &[]), vec![Node::CodeBlock("fn main() {\n    let a = \"<script>x</script>\";  // two spaces\n}\n".into())]);
        let indented = "Text\n\n    line one\n      line two <b>\n";
        assert_eq!(tree(indented, &[], &[])[1], Node::CodeBlock("line one\n  line two <b>\n".into()));
        let inline = tree("Run `cargo test -- --nocapture <x>` now", &[], &[]);
        assert_eq!(inline, vec![Node::Paragraph(vec![Node::Text("Run ".into()), Node::Code("cargo test -- --nocapture <x>".into()), Node::Text(" now".into())])]);
    }

    #[test]
    fn the_fixed_set_lists_quotes_rules_strike_and_breaks() {
        let text = |s: &str| vec![Node::Text(s.into())];
        assert_eq!(tree("3. three\n4. four", &[], &[]), vec![Node::List { start: Some(3), items: vec![text("three"), text("four")] }]);
        assert_eq!(tree("- a\n- b", &[], &[]), vec![Node::List { start: None, items: vec![text("a"), text("b")] }]);
        assert_eq!(
            tree("> quoted ~~old~~ *em*", &[], &[]),
            vec![Node::Quote(vec![Node::Paragraph(vec![
                Node::Text("quoted ".into()),
                Node::Strike(text("old")),
                Node::Text(" ".into()),
                Node::Emphasis(text("em")),
            ])])]
        );
        assert_eq!(tree("a\n\n---\n\nb", &[], &[])[1], Node::Rule);
        assert_eq!(
            tree("line  \nnext\nsoft", &[], &[]),
            vec![Node::Paragraph(vec![Node::Text("line".into()), Node::HardBreak, Node::Text("next".into()), Node::SoftBreak, Node::Text("soft".into())])]
        );
        // A list nested past the bound keeps every item's words.
        let deep: String = (0..60).map(|i| format!("{}- item{i}\n", "  ".repeat(i))).collect();
        let words = plain_text(&tree(&deep, &[], &[]));
        assert!((0..60).all(|i| words.contains(&format!("item{i}"))), "{words}");
    }

    #[test]
    fn deep_nesting_is_flattened_to_text_and_nothing_is_lost() {
        let md = format!("{}deep", "> ".repeat(500));
        let t = tree(&md, &[], &[]);
        fn depth(nodes: &[Node]) -> usize {
            nodes
                .iter()
                .map(|n| match n {
                    Node::Paragraph(c) | Node::Emphasis(c) | Node::Strong(c) | Node::Strike(c) | Node::Quote(c) => 1 + depth(c),
                    Node::Heading { children, .. } | Node::Link { children, .. } => 1 + depth(children),
                    Node::List { items, .. } => 1 + items.iter().map(|i| depth(i)).max().unwrap_or(0),
                    _ => 1,
                })
                .max()
                .unwrap_or(0)
        }
        assert!(depth(&t) <= MAX_DEPTH + 1, "depth {}", depth(&t));
        assert!(plain_text(&t).contains("deep"));
        assert!(tree("", &[], &[]).is_empty());
        assert!(!has_readme(&readme("<!-- only a comment -->", &[], &[])));
        assert!(has_readme(&readme("# Hi", &[], &[])));
    }
}
