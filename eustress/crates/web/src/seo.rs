// =============================================================================
// Eustress Web - Search and Agent Discovery
// =============================================================================
// What a crawler or an AI agent reads about each page, kept in one place so
// the static HTML and the live app say the same thing.
//
// - The prerender bin (src/bin/prerender.rs) calls `splice` to write each
//   route's head into its static HTML, writes `sitemap.xml` with a `lastmod`
//   per URL, writes `llms-full.txt` from the rendered docs pages, and writes
//   `404.html` from the router's own not-found page.
// - The live app (`TitleSync` in app.rs) calls `head_fields` after every
//   navigation, so the description and the canonical address follow the page.
//
// Both read the one table below. Nothing here touches the browser, the
// filesystem or the clock, so all of it runs under `cargo test`.
//
// Table of Contents:
// 1. Descriptions
// 2. Head fields: description, canonical address
// 3. Splicing a page into the shell
// 4. Sitemap
// 5. HTML to text, and llms-full.txt
// 6. Tests
// =============================================================================

pub const SITE: &str = "https://eustress.dev";

// -----------------------------------------------------------------------------
// 1. Descriptions
// -----------------------------------------------------------------------------

/// Said for an address with no entry of its own (a Space's page, a profile, a
/// sign-in page). It is the home page's description.
pub const DEFAULT_DESCRIPTION: &str = "Source-available simulation and data platform built in Rust. Model a battery cell or a power grid, drive it with real data, and compress time up to a year per second.";

/// What each page is, in plain words. Used for `description`, `og:description`
/// and `twitter:description`. One sentence, under 175 characters, no em dash.
/// Every route in sitemap.xml has a line here (a test holds that), and `/404`
/// describes the not-found page.
pub const DESCRIPTIONS: &[(&str, &str)] = &[
    ("/", DEFAULT_DESCRIPTION),
    ("/about", "What Eustress is: a source-available simulation and data platform built in Rust, and what you can do with it."),
    ("/ai", "AI tools in Eustress, from natural-language scripting with Soul to procedural world generation."),
    ("/acts", "The laws Eustress answers to, in plain words: what each law means on Eustress, your rights, and how fast we answer."),
    ("/blog", "Essays and deep dives from Eustress on simulation, ownership and the future of independent studios."),
    ("/blog/indie-studios", "An essay for indie studios on who owns the platform they build on, and what a source-available simulation platform changes."),
    ("/currency", "The three currencies on Eustress: Tickets buy things, Bliss rewards contributors, and Delta pays for premium Studio tools. They do not convert into each other."),
    ("/delta", "Delta pays for premium Studio tools. Most tools are free and never use it. See your balance and the packs on offer."),
    ("/bliss", "Bliss (BLS) is the Eustress platform currency: a Proof-of-Contribution ledger that credits contributors a daily emission."),
    ("/bliss/history", "Your Bliss wallet ledger: every BLS credit and spend on your account, newest first."),
    ("/bliss-leaderboard", "The contributors who earned the most BLS, ranked from the public ledger, and how the rankings work."),
    ("/careers", "Eustress is built and operated by one founder, @Simbuilder, working with AI assistants."),
    ("/community", "The Eustress community: the people building on Eustress, who is hosting right now, and what they published."),
    ("/contact", "Contact the Eustress team with a question or feedback."),
    ("/cookies", "How Eustress uses cookies and similar storage, and the choices you have."),
    ("/dmca", "The Eustress DMCA policy: how to report copyright infringement and how takedown notices are handled."),
    ("/docs/getting-started", "Install Eustress, create a Universe and a Space, add a part, press Play, save, and run your first script."),
    ("/docs/studio", "A tour of Studio, the Eustress editor: Explorer, Properties, Insert Object, tools and snapping, undo and History, shortcuts, saving and modes."),
    ("/docs/building", "Build a Space in Eustress: parts and their properties in meters, Models, booleans, the material library, terrain, lights and atmosphere."),
    ("/docs/perspective", "Switch between 2D and 3D views and between perspective and orthographic projection without converting the world."),
    ("/docs/cad", "Parametric CAD in Eustress on the truck B-rep kernel: feature trees, variables, booleans, assemblies and export."),
    ("/docs/importing", "Bring Roblox places, glTF and GLB models, USD, Gaussian splats, images and video into a Eustress Space."),
    ("/docs/ui", "Build ScreenGui and BillboardGui interfaces for the people inside a Eustress Space."),
    ("/docs/audio", "The Sound object and SoundService in Eustress: supported formats, how playback works, and where audio stands today."),
    ("/docs/scripting", "Script a Space in Rune, Luau or SoulScript: where scripts live, when they run, and the API they can reach."),
    ("/docs/services", "Every service a Eustress Space starts with, its role and its properties."),
    ("/docs/physics", "Physics in Eustress: Avian rigid bodies at 60 Hz in SI units, colliders, joints, queries, characters, deformation and fracture."),
    ("/docs/simulation", "The Eustress simulation clock and time scale, running and stepping, watchpoints, breakpoints, recordings and experiments."),
    ("/docs/realism", "The physical-law libraries in Eustress (materials, thermodynamics, electrochemistry) with a worked battery case study."),
    ("/docs/universes", "How a Eustress Space is stored: files, the WorldDb and git history, branching, the op-log, and several engines on one Universe."),
    ("/docs/networking", "Networking in Eustress: what runs in one process today, the authority rules in code, and the multiplayer plan."),
    ("/docs/publishing", "Publish a Universe from Studio: the package, review, the Gallery and listing pages."),
    ("/docs/website", "Let a website read live values out of a published Eustress Space with the Website Service."),
    ("/docs/earning", "How Eustress scores contributions, the daily Bliss (BLS) emission, the USD treasury drip, and Tickets (TKT)."),
    ("/docs/philosophy", "Why Eustress is built the way it is, and the mechanism behind each principle."),
    ("/download", "Download Eustress Studio, the editor for building, simulating and publishing Spaces."),
    ("/downloads/player", "Download Eustress Player to play and join published Spaces."),
    ("/gallery", "The Eustress Gallery: browse published Spaces, filter by tag, and open one to play or read about."),
    ("/leaderboard", "The Eustress leaderboard: the people who define the community, ranked."),
    ("/learn", "Guides for building a Space, giving it behavior, simulating it, publishing it, and handing it to AI agents."),
    ("/learn/mcp", "The Eustress MCP server: let an AI agent create, inspect and simulate inside a Space, with the full tool inventory and access classes."),
    ("/learn/cli", "The eustress command line (open, instances, close, bridge, run) and how to run Spaces headless with no window."),
    ("/learn/ide", "Edit Eustress scripts from an external editor, with the Rune extension for VS Code-compatible editors."),
    ("/learn/lsp", "eustress-lsp, the language server for Rune scripts, over stdio or TCP."),
    ("/license", "Eustress is source-available under PolyForm Shield 1.0.0: free for everyone except for competing with Eustress itself. Commercial licenses cover the rest."),
    ("/marketplace", "The Eustress Marketplace: assets, Soul scripts and avatar items from the community."),
    ("/premium", "Eustress plans and pricing, billed monthly or yearly."),
    ("/press", "Official assets and information for press coverage of Eustress."),
    ("/privacy", "The Eustress privacy policy: what data Eustress collects, why, and the choices you have."),
    ("/support", "Get help with Eustress. Pick the channel that fits your problem: Discord, GitHub issues or email."),
    ("/take-it-down", "Report intimate images or videos shared without consent, including ones made by AI. No account needed."),
    ("/terms", "The terms of service for Eustress Engine and eustress.dev."),
    ("/trust-registry", "The Eustress Online Trust Registry: every fork's BLS issuance rate is public, so the community can govern itself in the open."),
    ("/404", "That address does not exist on eustress.dev. Go to the home page, the docs or the Gallery."),
];

/// A path as the site names it: no query, no fragment, no trailing slash
/// (except the root), always with a leading slash.
pub fn normalize_path(path: &str) -> String {
    let end = path.find(|c| c == '?' || c == '#').unwrap_or(path.len());
    let mut p = path[..end].trim().to_string();
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

/// The description for a path; the default for a path with no entry.
pub fn description_for(path: &str) -> &'static str {
    let path = normalize_path(path);
    DESCRIPTIONS
        .iter()
        .find(|(route, _)| *route == path)
        .map(|(_, text)| *text)
        .unwrap_or(DEFAULT_DESCRIPTION)
}

/// The one address a page is known by: the site, then the path, with no query
/// and no fragment. The root keeps its slash, as the shell writes it.
pub fn canonical_url(path: &str) -> String {
    format!("{SITE}{}", normalize_path(path))
}

// -----------------------------------------------------------------------------
// 2. Head fields: description, canonical address
// -----------------------------------------------------------------------------

/// One tag in the `<head>` that follows the page: `tag[key="name"]`, whose
/// `attr` holds `value`.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadField {
    pub tag: &'static str,
    pub key: &'static str,
    pub name: &'static str,
    pub attr: &'static str,
    pub value: String,
}

impl HeadField {
    /// The CSS selector the live app finds the tag by.
    pub fn selector(&self) -> String {
        format!("{}[{}=\"{}\"]", self.tag, self.key, self.name)
    }

    /// The text the shell writes just before the value, which the prerender
    /// finds the tag by.
    pub fn open(&self) -> String {
        format!("<{} {}=\"{}\" {}=\"", self.tag, self.key, self.name, self.attr)
    }
}

/// Every head field that follows the path (the title is separate: it is named
/// after the page's heading). The prerender writes these into the static HTML
/// and `TitleSync` writes them after each navigation, so both say the same.
pub fn head_fields(path: &str) -> Vec<HeadField> {
    let description = description_for(path).to_string();
    let url = canonical_url(path);
    let field = |tag, key, name, attr, value: &String| HeadField { tag, key, name, attr, value: value.clone() };
    vec![
        field("meta", "name", "description", "content", &description),
        field("meta", "property", "og:description", "content", &description),
        field("meta", "name", "twitter:description", "content", &description),
        field("link", "rel", "canonical", "href", &url),
        field("meta", "property", "og:url", "content", &url),
        field("meta", "name", "twitter:url", "content", &url),
    ]
}

// -----------------------------------------------------------------------------
// 3. Splicing a page into the shell
// -----------------------------------------------------------------------------

/// The mount point exactly as index.html writes it.
pub const APP_MOUNT: &str = r#"<div id="app"></div>"#;
pub const APP_OPEN: &str = r#"<div id="app">"#;

/// The rendered markup is fenced inside `#app` so a later run can lift it
/// back out. `/` is written over `index.html`, which is also the shell every
/// other route is spliced into, so without this the bin could run once per
/// `trunk build`.
pub const RENDER_START: &str = "<!--prerender-->";
pub const RENDER_END: &str = "<!--/prerender-->";

/// An address no route owns. The router answers it with its not-found page,
/// which is what `404.html` is made from.
pub const NOT_FOUND_PROBE: &str = "/404.html";

/// Visible text of a fragment. Each tag becomes a space so `<span>of</span>
/// <br>Creation` reads "of Creation", not "ofCreation"; callers fold runs.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Text of the first `<h1>` in the page, tags stripped and whitespace folded.
pub fn first_h1(html: &str) -> Option<String> {
    let start = html.find("<h1")?;
    let open_end = start + html[start..].find('>')? + 1;
    let close = open_end + html[open_end..].find("</h1>")?;
    let text = strip_tags(&html[open_end..close]);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// Replace the text between the first `open` and the `close` that follows it,
/// keeping both markers. Returns false when the shell has no such field.
pub fn replace_between(page: &mut String, open: &str, close: &str, value: &str) -> bool {
    let Some(start) = page.find(open) else { return false };
    let value_start = start + open.len();
    let Some(len) = page[value_start..].find(close) else { return false };
    page.replace_range(value_start..value_start + len, value);
    true
}

/// Remove the whole tag that begins with `open`. Returns false when the shell
/// has no such tag.
pub fn remove_tag(page: &mut String, open: &str) -> bool {
    let Some(start) = page.find(open) else { return false };
    let Some(len) = page[start..].find('>') else { return false };
    page.replace_range(start..start + len + 1, "");
    true
}

/// Text safe inside a double-quoted attribute.
pub fn attr_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The shell with this route's markup inside `#app` and its own address,
/// description and title in the head.
pub fn splice(shell: &str, route: &str, body: &str) -> Result<String, String> {
    let Some(mount) = shell.find(APP_MOUNT) else {
        return Err(format!("shell has no `{APP_MOUNT}`"));
    };
    let mut page = String::with_capacity(shell.len() + body.len());
    page.push_str(&shell[..mount]);
    page.push_str(APP_OPEN);
    page.push_str(RENDER_START);
    page.push_str(body);
    page.push_str(RENDER_END);
    page.push_str("</div>");
    page.push_str(&shell[mount + APP_MOUNT.len()..]);

    // Every prerendered page is canonical for its own address and says what it
    // is in its own words. The shell says `/` everywhere, which is right for
    // the SPA fallback and wrong for a page.
    for field in head_fields(route) {
        replace_between(&mut page, &field.open(), "\"", &attr_escape(&field.value));
    }

    // The home page keeps the hand-written title. Every other route is named
    // after its own heading, so a search result or a link preview says what
    // the page is rather than what the site is.
    if route != "/" {
        if let Some(h1) = first_h1(body) {
            let title = format!("{h1} | Eustress Engine");
            let attr = title.replace('"', "&quot;");
            replace_between(&mut page, "<title>", "</title>", &title);
            replace_between(&mut page, r#"<meta name="title" content=""#, "\"", &attr);
            replace_between(&mut page, r#"<meta property="og:title" content=""#, "\"", &attr);
            replace_between(&mut page, r#"<meta name="twitter:title" content=""#, "\"", &attr);
        }
    }
    Ok(page)
}

/// The shell with the router's not-found page inside `#app`: what Cloudflare
/// Pages serves, with a 404 status, for an address that is no file and no
/// rewrite. It names no address of its own (no canonical, no `og:url`) and
/// asks to stay out of search results.
pub fn splice_not_found(shell: &str, body: &str) -> Result<String, String> {
    let mut page = splice(shell, "/404", body)?;
    for field in head_fields("/404").into_iter().filter(|f| f.name.ends_with("url") || f.name == "canonical") {
        remove_tag(&mut page, &field.open());
    }
    replace_between(&mut page, r#"<meta name="robots" content=""#, "\"", "noindex");
    Ok(page)
}

/// `index.html` as Trunk wrote it. When a previous run has already put the
/// home page inside `#app`, the fenced render is lifted out again; the head
/// needs no undoing because `/` keeps the shell's own title and fields.
pub fn pristine_shell(html: &str) -> Option<String> {
    if html.contains(APP_MOUNT) {
        return Some(html.to_string());
    }
    let start = html.find(APP_OPEN)?;
    let after_open = start + APP_OPEN.len();
    if !html[after_open..].starts_with(RENDER_START) {
        return None;
    }
    let end = after_open + html[after_open..].find(RENDER_END)? + RENDER_END.len();
    let rest = html[end..].strip_prefix("</div>")?;
    let mut shell = String::with_capacity(html.len());
    shell.push_str(&html[..start]);
    shell.push_str(APP_MOUNT);
    shell.push_str(rest);
    Some(shell)
}

// -----------------------------------------------------------------------------
// 4. Sitemap
// -----------------------------------------------------------------------------

/// Every `<loc>` in the sitemap, as a site-relative path.
pub fn routes_from_sitemap(xml: &str) -> Vec<String> {
    let mut routes = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<loc>") {
        let after = &rest[start + "<loc>".len()..];
        let Some(end) = after.find("</loc>") else { break };
        if let Some(path) = after[..end].trim().strip_prefix(SITE) {
            routes.push(if path.is_empty() { "/".to_string() } else { path.to_string() });
        }
        rest = &after[end..];
    }
    routes
}

/// `YYYY-MM-DD` (UTC) of a Unix time, by the civil-from-days rule.
pub fn civil_date(unix_secs: u64) -> String {
    let z = (unix_secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

/// The date every URL is stamped with: `SOURCE_DATE_EPOCH` (the standard
/// variable for a reproducible build) when it is set and is a number, else the
/// moment of the build.
pub fn build_date(source_date_epoch: Option<&str>, now_unix_secs: u64) -> String {
    let secs = source_date_epoch.and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(now_unix_secs);
    civil_date(secs)
}

/// The sitemap with a `<lastmod>` after each `<loc>`. A `<lastmod>` already
/// there is replaced, so running it again on its own output changes nothing.
pub fn sitemap_with_lastmod(xml: &str, date: &str) -> String {
    let mut out = String::with_capacity(xml.len() + 64 * 64);
    let mut rest = xml;
    while let Some(open) = rest.find("<url>") {
        let inner = open + "<url>".len();
        let Some(len) = rest[inner..].find("</url>") else { break };
        out.push_str(&rest[..inner]);
        let mut block = rest[inner..inner + len].to_string();
        while let Some(s) = block.find("<lastmod>") {
            match block[s..].find("</lastmod>") {
                Some(e) => block.replace_range(s..s + e + "</lastmod>".len(), ""),
                None => break,
            }
        }
        if let Some(loc_end) = block.find("</loc>") {
            block.insert_str(loc_end + "</loc>".len(), &format!("<lastmod>{date}</lastmod>"));
        }
        out.push_str(&block);
        rest = &rest[inner + len..];
    }
    out.push_str(rest);
    out
}

// -----------------------------------------------------------------------------
// 5. HTML to text, and llms-full.txt
// -----------------------------------------------------------------------------

/// The line under the title of llms.txt and of llms-full.txt.
pub const LLMS_SUMMARY: &str = "Eustress is a source-available simulation and data platform built in Rust. Model the real world, from a battery cell to a power grid, drive it with real data, compress time for clock-driven models by up to a year per second, and let AI agents work inside the same Spaces you do.";

/// The guides that go into llms-full.txt: the docs and the learn pages.
pub fn is_doc_route(route: &str) -> bool {
    route.starts_with("/docs/") || route == "/learn" || route.starts_with("/learn/")
}

/// The Markdown docs llms.txt lists, by their published path.
pub const MARKDOWN_DOCS: &[&str] = &[
    "docs/api/WEBSITE_SERVICE_API.md",
    "docs/api/WEBSITE_MANIFEST_INTEGRATION.md",
    "docs/design/WEBSITE_SERVICE.md",
];

/// One rendered guide.
pub struct DocPage {
    pub route: String,
    pub body: String,
}

/// One Markdown doc, as published.
pub struct MarkdownDoc {
    pub path: String,
    pub text: String,
}

/// The guide's own `<main>` when it has one; the whole fragment otherwise.
pub fn main_of(html: &str) -> &str {
    let mut from = 0;
    while let Some(at) = html[from..].find("<main") {
        let start = from + at;
        let next = html[start + 5..].chars().next();
        if matches!(next, Some(' ') | Some('>') | Some('/')) {
            if let Some(open_end) = html[start..].find('>') {
                let inner = start + open_end + 1;
                let end = html[inner..].rfind("</main>").map(|e| inner + e).unwrap_or(html.len());
                return &html[inner..end];
            }
        }
        from = start + 5;
    }
    html
}

/// `&amp;`, `&#x27;` and the like, back to the characters they stand for.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(semi) = after.find(';').filter(|s| *s <= 10) {
            if let Some(c) = entity_char(&after[..semi]) {
                out.push(c);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn entity_char(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(|c| c == 'x' || c == 'X') {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// The value of `name="..."` inside a tag's source, if it has one.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

fn is_void(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img" | "input" | "meta" | "link" | "col" | "source" | "wbr")
}

/// Elements whose content is not the page's words: drawings, scripts, and the
/// page's own navigation.
fn is_skipped(name: &str, tag: &str) -> bool {
    matches!(name, "svg" | "script" | "style" | "nav" | "footer" | "aside" | "button" | "noscript" | "template" | "select" | "head")
        || attribute(tag, "class").map_or(false, |c| c.split_whitespace().any(|w| w == "docs-breadcrumb"))
}

/// The index of the `>` that ends the tag opened at `start`, quotes respected.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in html[start + 1..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, '>') => return Some(start + 1 + i),
            _ => {}
        }
    }
    None
}

struct Reader {
    out: String,
    breaks: usize,
    space: bool,
    lists: Vec<Option<usize>>,
    /// Per open `<a>`: where its label starts in `out` and its address, when it has one.
    links: Vec<Option<(usize, String)>>,
    pre: bool,
    cell: bool,
    heading: bool,
    cells: usize,
    columns: usize,
    skip: Option<(String, usize)>,
    offset: usize,
}

impl Reader {
    fn new(offset: usize) -> Self {
        Reader {
            out: String::new(),
            breaks: 0,
            space: false,
            lists: Vec::new(),
            links: Vec::new(),
            pre: false,
            cell: false,
            heading: false,
            cells: 0,
            columns: 0,
            skip: None,
            offset,
        }
    }

    /// Ask for `n` line breaks before the next words. Inside a table cell or a
    /// heading a break is a space.
    fn want_break(&mut self, n: usize) {
        if self.cell {
            self.space = true;
        } else if !self.pre {
            self.breaks = self.breaks.max(n);
        }
    }

    fn flush_breaks(&mut self) {
        if self.breaks > 0 && !self.out.is_empty() {
            while self.out.ends_with(' ') {
                self.out.pop();
            }
            let have = self.out.chars().rev().take_while(|c| *c == '\n').count();
            for _ in have..self.breaks {
                self.out.push('\n');
            }
        }
        self.breaks = 0;
    }

    fn emit(&mut self, text: &str) {
        self.flush_breaks();
        if self.space {
            if let Some(last) = self.out.chars().last() {
                if !last.is_whitespace() && last != '[' {
                    self.out.push(' ');
                }
            }
        }
        self.space = false;
        self.out.push_str(text);
    }

    fn raw(&mut self, text: &str) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        self.out.push_str(text);
    }

    fn text(&mut self, chunk: &str) {
        if self.skip.is_some() {
            return;
        }
        let decoded = decode_entities(chunk);
        if self.pre {
            self.out.push_str(&decoded);
            return;
        }
        let folded = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
        if folded.is_empty() {
            if !decoded.is_empty() {
                self.space = true;
            }
            return;
        }
        if decoded.starts_with(char::is_whitespace) {
            self.space = true;
        }
        let folded = if self.cell { folded.replace('|', "\\|") } else { folded };
        self.emit(&folded);
        if decoded.ends_with(char::is_whitespace) {
            self.space = true;
        }
    }

    fn tag(&mut self, inner: &str) {
        let closing = inner.starts_with('/');
        let body = inner.trim_start_matches('/');
        let name_len = body.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(body.len());
        let name = body[..name_len].to_ascii_lowercase();
        if name.is_empty() || name.starts_with('!') || name.starts_with('?') {
            return;
        }
        let self_closing = inner.ends_with('/') || is_void(&name);

        if let Some((skipping, depth)) = self.skip.as_mut() {
            if name == *skipping && !self_closing {
                if closing {
                    *depth -= 1;
                    if *depth == 0 {
                        self.skip = None;
                    }
                } else {
                    *depth += 1;
                }
            }
            return;
        }
        if !closing && !self_closing && is_skipped(&name, inner) {
            self.skip = Some((name, 1));
            return;
        }
        if self.heading && !matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            self.space = true;
        }
        // The facts under a guide's title (reading time, level, date) are spans with no space between them.
        if name == "span" && attribute(inner, "class").map_or(false, |c| c.split_whitespace().any(|w| w == "meta-item")) {
            self.space = true;
        }

        match (name.as_str(), closing) {
            ("br", _) | ("hr", _) => self.want_break(if name == "hr" { 2 } else { 1 }),
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                let level = (name.as_bytes()[1] - b'0') as usize + self.offset;
                self.want_break(2);
                self.flush_breaks();
                self.out.push_str(&"#".repeat(level.min(6)));
                self.out.push(' ');
                self.space = false;
                self.heading = true;
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => {
                self.heading = false;
                self.want_break(2);
            }
            ("ul", false) | ("ol", false) => {
                self.want_break(if self.lists.is_empty() { 2 } else { 1 });
                self.lists.push(if name == "ol" { Some(1) } else { None });
            }
            ("ul", true) | ("ol", true) => {
                self.lists.pop();
                self.want_break(if self.lists.is_empty() { 2 } else { 1 });
            }
            ("li", false) => {
                self.want_break(1);
                self.flush_breaks();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "- ".to_string(),
                };
                let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                self.out.push_str(&indent);
                self.out.push_str(&marker);
                self.space = false;
            }
            ("table", _) => self.want_break(2),
            ("tr", false) => {
                self.cell = false;
                self.want_break(1);
                self.flush_breaks();
                self.out.push('|');
                self.cells = 0;
            }
            ("tr", true) => self.columns = self.cells,
            ("th", false) | ("td", false) => {
                self.cell = true;
                self.space = true;
            }
            ("th", true) | ("td", true) => {
                self.cell = false;
                self.raw(" |");
                self.cells += 1;
            }
            ("thead", true) => {
                self.want_break(1);
                self.flush_breaks();
                self.out.push('|');
                for _ in 0..self.columns {
                    self.out.push_str(" --- |");
                }
            }
            ("pre", false) => {
                self.want_break(2);
                self.flush_breaks();
                self.out.push_str("```\n");
                self.pre = true;
            }
            ("pre", true) => {
                self.pre = false;
                if !self.out.ends_with('\n') {
                    self.out.push('\n');
                }
                self.out.push_str("```");
                self.want_break(2);
            }
            ("code", false) if !self.pre => self.emit("`"),
            ("code", true) if !self.pre => self.raw("`"),
            ("a", false) => {
                let href = attribute(inner, "href").unwrap_or("");
                if !href.is_empty() && !href.starts_with('#') && !href.starts_with("javascript:") {
                    self.emit("[");
                    self.links.push(Some((self.out.len(), absolute(href))));
                } else {
                    self.links.push(None);
                }
            }
            ("a", true) => {
                if let Some(Some((label_at, href))) = self.links.pop() {
                    let label = self.out[label_at..].to_string();
                    if label.trim().is_empty() {
                        // A link with no words, such as an icon: nothing is left of it.
                        self.out.truncate(label_at - 1);
                    } else if label.contains('\n') {
                        // A card: headings and paragraphs inside one link. Markdown has no link over
                        // several blocks, so the words stay and the address follows them.
                        self.out.truncate(label_at - 1);
                        self.out.push_str(label.trim_start());
                        self.out.push_str(&format!("\n<{href}>"));
                        self.want_break(1);
                    } else {
                        self.raw(&format!("]({href})"));
                    }
                }
            }
            (
                "p" | "div" | "section" | "article" | "header" | "main" | "figure" | "figcaption" | "details" | "summary"
                | "blockquote" | "dl" | "dt" | "dd" | "address",
                _,
            ) => self.want_break(if name == "div" { 1 } else { 2 }),
            _ => {}
        }
    }

    fn finish(self) -> String {
        let mut lines: Vec<String> = Vec::new();
        let mut fenced = false;
        for line in self.out.lines() {
            if line.starts_with("```") {
                fenced = !fenced;
            }
            let line = if fenced { line } else { line.trim_end() };
            if !fenced {
                // A heading with no words leaves nothing.
                if !line.is_empty() && line.starts_with('#') && line.chars().all(|c| c == '#' || c == ' ') {
                    continue;
                }
                if line.is_empty() && lines.last().map_or(true, |l| l.is_empty()) {
                    continue;
                }
            }
            lines.push(line.to_string());
        }
        while lines.last().map_or(false, |l| l.is_empty()) {
            lines.pop();
        }
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// `/docs/studio` is written for the site; an agent reading a file needs the
/// whole address.
fn absolute(href: &str) -> String {
    if href.starts_with('/') && !href.starts_with("//") {
        format!("{SITE}{href}")
    } else {
        href.to_string()
    }
}

/// The page's words as Markdown: headings, lists, tables, code blocks, links
/// with their full address. `heading_offset` pushes every heading down that
/// many levels, so a page can sit under a title of its own.
pub fn html_to_text(html: &str, heading_offset: usize) -> String {
    let mut reader = Reader::new(heading_offset);
    let mut at = 0;
    while at < html.len() {
        let Some(p) = html[at..].find('<') else {
            reader.text(&html[at..]);
            break;
        };
        if p > 0 {
            reader.text(&html[at..at + p]);
        }
        let start = at + p;
        if html[start..].starts_with("<!--") {
            match html[start..].find("-->") {
                Some(e) => at = start + e + 3,
                None => break,
            }
            continue;
        }
        match tag_end(html, start) {
            Some(end) => {
                reader.tag(&html[start + 1..end]);
                at = end + 1;
            }
            None => {
                reader.text(&html[start..]);
                break;
            }
        }
    }
    reader.finish()
}

/// llms-full.txt: the full text of the docs, in the llms.txt convention (an H1
/// with the site's name, a blockquote summary, then sections), so an agent can
/// read every guide in one request. `date` is the build date.
pub fn llms_full(pages: &[DocPage], markdown: &[MarkdownDoc], date: &str) -> String {
    let mut out = String::new();
    out.push_str("# Eustress Engine: full documentation\n\n");
    out.push_str(&format!("> {LLMS_SUMMARY}\n\n"));
    out.push_str(&format!(
        "This file holds the full text of every guide at {SITE}/docs and {SITE}/learn, and of the Markdown docs for the Website Service, generated from the site on {date}. The index of the site is {SITE}/llms.txt.\n"
    ));
    for page in pages {
        let text = html_to_text(main_of(&page.body), 1);
        out.push_str("\n---\n\n");
        out.push_str(&format!("Source: {SITE}{}\n\n", page.route));
        out.push_str(text.trim_end());
        out.push('\n');
    }
    for doc in markdown {
        out.push_str("\n---\n\n");
        out.push_str(&format!("Source: {SITE}/{}\n\n", doc.path));
        out.push_str(doc.text.replace("\r\n", "\n").trim_end());
        out.push('\n');
    }
    out
}

// -----------------------------------------------------------------------------
// 6. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = include_str!("../index.html");
    const SITEMAP: &str = include_str!("../sitemap.xml");
    const ROBOTS: &str = include_str!("../robots.txt");
    const REDIRECTS: &str = include_str!("../_redirects");
    const LLMS: &str = include_str!("../llms.txt");
    const APP: &str = include_str!("app.rs");
    const PRERENDER: &str = include_str!("bin/prerender.rs");

    fn count(haystack: &str, needle: &str) -> usize {
        haystack.matches(needle).count()
    }

    // ---- descriptions -------------------------------------------------------

    #[test]
    fn every_sitemap_route_has_its_own_description_and_nothing_else_does() {
        let routes = routes_from_sitemap(SITEMAP);
        assert!(routes.len() >= 50, "the sitemap lists the public routes: {}", routes.len());
        for route in &routes {
            assert!(DESCRIPTIONS.iter().any(|(r, _)| r == route), "no description for {route}");
        }
        for (route, _) in DESCRIPTIONS {
            assert!(*route == "/404" || routes.contains(&route.to_string()), "{route} is described but not in the sitemap");
        }
    }

    #[test]
    fn descriptions_are_plain_sentences() {
        let mut seen: Vec<&str> = Vec::new();
        for (route, text) in DESCRIPTIONS {
            assert!((40..=175).contains(&text.chars().count()), "{route}: {} characters", text.chars().count());
            assert!(text.ends_with('.'), "{route} ends with a full stop");
            for dash in ['\u{2014}', '\u{2013}'] {
                assert!(!text.contains(dash), "{route} has a long dash");
            }
            assert!(!text.to_lowercase().contains("game engine"), "{route} calls Eustress a game engine");
            assert!(!text.contains('"') && !text.contains('<') && !text.contains('&'), "{route} has a character that needs escaping");
            assert!(!seen.contains(text), "{route} repeats another page's description");
            seen.push(text);
        }
    }

    #[test]
    fn a_path_without_an_entry_gets_the_default_and_paths_are_normalized() {
        assert_eq!(description_for("/simulation/abc"), DEFAULT_DESCRIPTION);
        assert_eq!(description_for("/docs/studio"), description_for("/docs/studio/?x=1#top"));
        assert_ne!(description_for("/docs/studio"), DEFAULT_DESCRIPTION);
        assert_eq!(description_for("/"), DEFAULT_DESCRIPTION);
        assert_eq!(normalize_path("docs/cad/"), "/docs/cad");
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("/gallery?tag=a#b"), "/gallery");
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn the_canonical_address_has_the_site_the_path_and_no_query() {
        assert_eq!(canonical_url("/"), "https://eustress.dev/");
        assert_eq!(canonical_url("/gallery?tag=horror"), "https://eustress.dev/gallery");
        assert_eq!(canonical_url("/docs/studio/"), "https://eustress.dev/docs/studio");
        assert_eq!(canonical_url("/simulation/abc#play"), "https://eustress.dev/simulation/abc");
    }

    #[test]
    fn head_fields_name_six_tags_the_shell_has() {
        let fields = head_fields("/docs/cad");
        assert_eq!(fields.len(), 6);
        for f in &fields {
            assert_eq!(count(INDEX, &f.open()), 1, "the shell has one {}", f.open());
        }
        assert_eq!(fields[0].selector(), r#"meta[name="description"]"#);
        assert_eq!(fields[3].selector(), r#"link[rel="canonical"]"#);
        assert_eq!(fields[3].value, "https://eustress.dev/docs/cad");
        assert_eq!(fields[1].value, description_for("/docs/cad"));
    }

    #[test]
    fn the_shell_says_the_home_pages_description_in_all_three_places() {
        for f in head_fields("/") {
            let at = INDEX.find(&f.open()).expect("tag in the shell") + f.open().len();
            let end = at + INDEX[at..].find('"').unwrap();
            assert_eq!(&INDEX[at..end], attr_escape(&f.value), "{}", f.name);
        }
    }

    // ---- the product framing -------------------------------------------------
    // McKale (2026-10-15): Eustress is a source-available simulation and data platform built in Rust, and it also builds
    // games and interactive worlds. Comparing it to game engines is fine; calling it one is not. The product noun is
    // Simulation, not Experience.

    const NLWEB: &str = include_str!("../assets/.well-known/nlweb.json");
    const NL_PRODUCT: &str = include_str!("../assets/.well-known/nlweb/product.json");
    const NL_FEATURES: &str = include_str!("../assets/.well-known/nlweb/features.json");
    const NL_FAQ: &str = include_str!("../assets/.well-known/nlweb/faq.json");

    /// How every lead opens.
    const LEAD_SENTENCE: &str = "Eustress is a source-available simulation and data platform built in Rust";
    /// What a lead says about games.
    const GAMES_SECOND: &str = "it also builds games and interactive worlds.";
    /// Words that sell instead of saying.
    const HYPE: [&str; 7] = ["unprecedented", "next-generation", "next generation", "stunning", "professional-grade", "cutting-edge", "revolutionary"];
    /// Ways of saying what Eustress is that this pass removed.
    const OLD_IDENTITY: [&str; 4] = ["game creation and play", "interactive 3d experience", "game development platform", "for game development"];

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("valid JSON")
    }

    fn text_at<'a>(value: &'a serde_json::Value, pointer: &str) -> &'a str {
        value.pointer(pointer).and_then(|v| v.as_str()).unwrap_or_else(|| panic!("no text at {pointer}"))
    }

    /// Every string in the document, except the ones under the key SKIP.
    fn strings_of(value: &serde_json::Value, skip: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|v| strings_of(v, skip, out)),
            serde_json::Value::Object(o) => o.iter().filter(|(k, _)| k.as_str() != skip).for_each(|(_, v)| strings_of(v, skip, out)),
            _ => {}
        }
    }

    /// The description line under llms.txt's title, without its marker.
    fn llms_lead() -> &'static str {
        LLMS.lines().find_map(|line| line.strip_prefix("> ")).expect("llms.txt has a description line")
    }

    /// Where the text calls Eustress a game engine, if it does. "Game engines" (the field, or the others) and the
    /// "Game Engine Comparison" heading are not that.
    fn calls_it_a_game_engine(text: &str) -> Option<String> {
        let lower = text.to_lowercase();
        let mut from = 0;
        while let Some(found) = lower[from..].find("game engine") {
            let at = from + found;
            let after = &lower[at + "game engine".len()..];
            if !(after.starts_with('s') || after.starts_with(" comparison")) {
                let before = lower[..at].chars().count();
                return Some(lower.chars().skip(before.saturating_sub(30)).take(80).collect());
            }
            from = at + "game engine".len();
        }
        None
    }

    /// A sentence the pass wrote: no long dash, no hype, no old identity, no "experiences", no game engine.
    fn assert_plain_copy(label: &str, text: &str) {
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!text.contains(dash), "{label} has a long dash: {text}");
        }
        let lower = text.to_lowercase();
        for word in HYPE.iter().chain(OLD_IDENTITY.iter()) {
            assert!(!lower.contains(word), "{label} says {word:?}: {text}");
        }
        assert!(!lower.contains("experiences"), "{label}: the product noun is Simulation, not Experience: {text}");
        assert_eq!(calls_it_a_game_engine(text), None, "{label} calls Eustress a game engine");
    }

    #[test]
    fn the_files_open_with_the_simulation_and_data_platform_sentence_and_name_games_second() {
        let (nl, product, features, faq) = (json(NLWEB), json(NL_PRODUCT), json(NL_FEATURES), json(NL_FAQ));
        let with_games = format!("{LEAD_SENTENCE}; {GAMES_SECOND}");
        for (label, text) in [
            ("nlweb.json", text_at(&nl, "/description")),
            ("product.json", text_at(&product, "/description")),
            ("faq.json: what is Eustress", text_at(&faq, "/mainEntity/0/acceptedAnswer/text")),
        ] {
            assert!(text.starts_with(&with_games), "{label} opens with the lead sentence: {text}");
        }
        assert!(llms_lead().starts_with(&format!("{LEAD_SENTENCE}.")), "llms.txt opens with the lead sentence: {}", llms_lead());
        assert!(LLMS.contains("Eustress also builds games and interactive worlds."), "llms.txt names games as the second capability");
        let ld = json_ld();
        for (label, text) in [
            ("features.json", text_at(&features, "/description")),
            ("the index.html Organization", ld[0]["description"].as_str().expect("the Organization description")),
            ("the home description", DEFAULT_DESCRIPTION),
            ("the /about description", description_for("/about")),
        ] {
            assert!(text.to_lowercase().contains("simulation and data platform"), "{label} says what Eustress is: {text}");
        }
    }

    #[test]
    fn the_files_never_call_eustress_a_game_engine_and_keep_that_keyword_for_later() {
        let mut files: Vec<(&str, String)> = vec![("llms.txt", LLMS.to_string()), ("index.html", INDEX.to_string())];
        for (label, source) in [("nlweb.json", NLWEB), ("product.json", NL_PRODUCT), ("features.json", NL_FEATURES), ("faq.json", NL_FAQ)] {
            let mut strings = Vec::new();
            strings_of(&json(source), "keywords", &mut strings);
            files.push((label, strings.join("\n")));
        }
        for (label, text) in &files {
            assert_eq!(calls_it_a_game_engine(text), None, "{label} calls Eustress a game engine");
            assert!(!text.to_lowercase().contains("experiences"), "{label}: the product noun is Simulation, not Experience");
        }
        let keywords: Vec<String> = json(NL_PRODUCT)["keywords"]
            .as_array()
            .expect("keywords")
            .iter()
            .map(|k| k.as_str().expect("a keyword").to_lowercase())
            .collect();
        let at = |word: &str| keywords.iter().position(|k| k == word).unwrap_or_else(|| panic!("no keyword {word:?}"));
        assert_eq!(at("simulation platform"), 0, "the first keyword is what Eustress is");
        assert!(at("game engine") > at("simulation and data platform"), "game engine is a later keyword");
        assert!(at("rust game engine") > at("rust simulation engine"), "rust game engine is a later keyword");
    }

    #[test]
    fn the_rewritten_file_copy_is_plain() {
        let (nl, product, features, faq) = (json(NLWEB), json(NL_PRODUCT), json(NL_FEATURES), json(NL_FAQ));
        let mut copy: Vec<(&str, String)> = vec![
            ("llms.txt", LLMS.to_string()),
            ("nlweb.json", text_at(&nl, "/description").to_string()),
            ("product.json", text_at(&product, "/description").to_string()),
            ("product.json: Engine", text_at(&product, "/products/0/description").to_string()),
            ("product.json: Player", text_at(&product, "/products/1/description").to_string()),
            ("features.json", text_at(&features, "/description").to_string()),
            ("features.json: Soul", text_at(&features, "/scripting/description").to_string()),
        ];
        for (index, label) in [(0, "faq.json: what is Eustress"), (1, "faq.json: is it free"), (3, "faq.json: visual scripting"), (8, "faq.json: Engine and Player")] {
            copy.push((label, text_at(&faq, &format!("/mainEntity/{index}/acceptedAnswer/text")).to_string()));
        }
        for (label, text) in &copy {
            assert_plain_copy(label, text);
        }
    }

    /// The page at ROUTE as the prerender draws it.
    #[cfg(feature = "ssr")]
    fn route_html(route: &str) -> String {
        use hydration_context::{SharedContext, SsrSharedContext};
        use leptos::prelude::*;
        use leptos::tachys::view::RenderHtml;
        use leptos_router::location::RequestUrl;
        let _ = leptos::task::Executor::init_futures_executor();
        let shared: std::sync::Arc<dyn SharedContext + Send + Sync> = std::sync::Arc::new(SsrSharedContext::new());
        let owner = Owner::new_root(Some(shared));
        let html = owner.with(|| {
            provide_context(RequestUrl::new(route));
            crate::app::App().to_html()
        });
        owner.cleanup();
        html
    }

    /// The text of markup that holds only inline tags: tags gone (a line break reads as a space), entities decoded.
    #[cfg(feature = "ssr")]
    fn plain(html: &str) -> String {
        let (mut out, mut tag, mut in_tag) = (String::new(), String::new(), false);
        for c in html.chars() {
            if in_tag {
                if c == '>' {
                    in_tag = false;
                    if tag.starts_with("br") {
                        out.push(' ');
                    }
                    tag.clear();
                } else {
                    tag.push(c);
                }
            } else if c == '<' {
                in_tag = true;
            } else {
                out.push(c);
            }
        }
        decode_entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The whole page as words: every tag reads as a space.
    #[cfg(feature = "ssr")]
    fn words(html: &str) -> String {
        decode_entities(&strip_tags(html)).split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The text of the Nth element whose class attribute is exactly CLASS, up to the first closing paragraph tag.
    #[cfg(feature = "ssr")]
    fn paragraph(html: &str, class: &str, nth: usize) -> String {
        let marker = format!("class=\"{class}\"");
        let mut at = 0;
        for i in 0..=nth {
            let found = html[at..].find(&marker).unwrap_or_else(|| panic!("no {marker} number {i}"));
            at += found + marker.len();
        }
        let open_end = at + html[at..].find('>').expect("the end of the tag") + 1;
        let close = open_end + html[open_end..].find("</p>").expect("the end of the paragraph");
        plain(&html[open_end..close])
    }

    /// The paragraphs that say what Eustress is, on the pages that say it.
    #[cfg(feature = "ssr")]
    fn lead_paragraphs() -> Vec<(&'static str, String)> {
        let (home, about, press) = (route_html("/"), route_html("/about"), route_html("/press"));
        vec![
            ("the home hero paragraph", paragraph(&home, "hero-description", 0)),
            ("the About lead", paragraph(&about, "mission-text", 0)),
            ("the Press short description", paragraph(&press, "description-text", 0)),
            ("the Press long description", paragraph(&press, "description-text", 1)),
        ]
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_pages_open_with_the_simulation_and_data_platform_sentence_and_name_games_second() {
        let with_games = format!("{LEAD_SENTENCE}; {GAMES_SECOND}");
        for (label, text) in lead_paragraphs() {
            assert!(text.starts_with(&with_games), "{label} opens with the lead sentence: {text}");
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn no_page_in_the_pass_calls_eustress_a_game_engine_or_sells_experiences() {
        for route in ["/", "/about", "/press", "/careers", "/download", "/downloads/player"] {
            let text = words(&route_html(route));
            assert_eq!(calls_it_a_game_engine(&text), None, "{route} calls Eustress a game engine");
            let lower = text.to_lowercase();
            assert!(!lower.contains("experiences"), "{route}: the product noun is Simulation, not Experience");
            for old in OLD_IDENTITY {
                assert!(!lower.contains(old), "{route} says {old:?}");
            }
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_rewritten_page_copy_is_plain() {
        let (about, careers) = (route_html("/about"), route_html("/careers"));
        let mut copy = lead_paragraphs();
        copy.push(("the About tagline", paragraph(&about, "about-tagline", 0)));
        copy.push(("the About Engine card", paragraph(&about, "product-desc", 0)));
        copy.push(("the About Player card", paragraph(&about, "product-desc", 1)));
        copy.push(("the Careers team paragraph", paragraph(&careers, "team-description", 0)));
        for (label, text) in &copy {
            assert_plain_copy(label, text);
        }
    }

    // ---- splice -------------------------------------------------------------

    const BODY: &str = "<main><h1>Studio</h1><p>The editor.</p></main>";

    #[test]
    fn a_page_gets_its_own_description_address_and_title() {
        let page = splice(INDEX, "/docs/studio", BODY).unwrap();
        let want = description_for("/docs/studio");
        assert_eq!(count(&page, &format!(r#"<meta name="description" content="{want}">"#)), 1);
        assert_eq!(count(&page, &format!(r#"<meta property="og:description" content="{want}">"#)), 1);
        assert_eq!(count(&page, &format!(r#"<meta name="twitter:description" content="{want}">"#)), 1);
        assert_eq!(count(&page, r#"<link rel="canonical" href="https://eustress.dev/docs/studio">"#), 1);
        assert_eq!(count(&page, r#"<meta property="og:url" content="https://eustress.dev/docs/studio">"#), 1);
        assert_eq!(count(&page, r#"<meta name="twitter:url" content="https://eustress.dev/docs/studio">"#), 1);
        assert!(page.contains("<title>Studio | Eustress Engine</title>"));
        assert!(page.contains(&format!("{APP_OPEN}{RENDER_START}{BODY}{RENDER_END}</div>")));
        assert_eq!(count(&page, "name=\"description\""), 1);
        assert!(!page.contains(DEFAULT_DESCRIPTION), "the home description does not leak into another page");
    }

    #[test]
    fn the_home_page_keeps_its_title_and_the_shell_round_trips() {
        let page = splice(INDEX, "/", "<h1>Anything</h1>").unwrap();
        assert!(page.contains("<title>Eustress Engine | Simulation &amp; Data Platform</title>"));
        assert_eq!(pristine_shell(&page).as_deref(), Some(INDEX), "home splice then lift is the shell again");
        let again = splice(&pristine_shell(&page).unwrap(), "/", "<h1>Anything</h1>").unwrap();
        assert_eq!(again, page, "a second run writes the same page");
    }

    #[test]
    fn a_description_is_escaped_inside_its_attribute() {
        assert_eq!(attr_escape(r#"a "b" & <c>"#), "a &quot;b&quot; &amp; &lt;c&gt;");
    }

    #[test]
    fn the_not_found_page_has_no_address_of_its_own_and_asks_to_stay_out_of_search() {
        let page = splice_not_found(INDEX, "<h1>Page Not Found</h1>").unwrap();
        assert!(page.contains("<title>Page Not Found | Eustress Engine</title>"));
        assert!(!page.contains("rel=\"canonical\""));
        assert!(!page.contains("og:url"));
        assert!(!page.contains("twitter:url"));
        assert!(page.contains(r#"<meta name="robots" content="noindex">"#));
        assert!(page.contains(description_for("/404")));
        assert!(page.contains(r#"<div id="app"><!--prerender--><h1>Page Not Found</h1>"#));
    }

    #[test]
    fn a_shell_without_the_mount_point_is_refused() {
        assert!(splice("<html></html>", "/about", "x").is_err());
    }

    // ---- sitemap ------------------------------------------------------------

    #[test]
    fn every_url_gets_exactly_one_lastmod_and_a_second_run_changes_nothing() {
        let once = sitemap_with_lastmod(SITEMAP, "2026-10-15");
        let urls = count(SITEMAP, "<url>");
        assert!(urls >= 50);
        assert_eq!(count(&once, "<lastmod>2026-10-15</lastmod>"), urls);
        assert_eq!(count(&once, "<lastmod>"), urls);
        assert_eq!(sitemap_with_lastmod(&once, "2026-10-15"), once);
        let later = sitemap_with_lastmod(&once, "2026-10-16");
        assert_eq!(count(&later, "<lastmod>2026-10-16</lastmod>"), urls);
        assert_eq!(count(&later, "2026-10-15"), 0, "an old date is replaced, not kept");
        assert_eq!(routes_from_sitemap(&once), routes_from_sitemap(SITEMAP), "the routes are the same");
        assert!(once.contains("<url><loc>https://eustress.dev/about</loc><lastmod>2026-10-15</lastmod></url>"));
    }

    #[test]
    fn dates_are_utc_civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(999_993_600), "2001-09-09");
        assert_eq!(civil_date(1_709_164_800), "2024-02-29");
        assert_eq!(civil_date(1_792_022_400), "2026-10-15");
        assert_eq!(civil_date(1_792_022_400 + 86_399), "2026-10-15");
        assert_eq!(civil_date(1_792_022_400 + 86_400), "2026-10-16");
        assert_eq!(build_date(Some("1709164800"), 5), "2024-02-29", "SOURCE_DATE_EPOCH wins");
        assert_eq!(build_date(Some("soon"), 1_792_022_400), "2026-10-15", "a bad value falls back to now");
        assert_eq!(build_date(None, 0), "1970-01-01");
    }

    // ---- html to text, llms-full.txt ----------------------------------------

    #[test]
    fn markup_becomes_markdown() {
        let html = concat!(
            "<!--prerender--><!><nav><a href=\"/\">Home</a></nav><main class=\"docs-content\">",
            "<div class=\"docs-breadcrumb\"><a href=\"/learn\">Learn</a></div>",
            "<h1 class=\"t\"><span>01</span>The Window</h1><p>Studio is <strong>one</strong> window; see <a href=\"/docs/cad\">CAD</a>",
            " &amp; <a href=\"#x\">here</a>, it&#x27;s <code>Ctrl+Z</code>.</p>",
            "<svg viewBox=\"0 0 1 1\"><text>drawing</text></svg><button>Copy</button>",
            "<ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul><ol><li>a</li><li>b</li></ol>",
            "<table><thead><tr><th>Key</th><th>Does</th></tr></thead><tbody><tr><td>F5</td><td>a | b</td></tr></tbody></table>",
            "<pre><code>let x = 1;\nlet y = &lt;2&gt;;</code></pre><h2>Next</h2><p>Last.</p></main><footer>Footer words</footer>"
        );
        let text = html_to_text(main_of(html), 1);
        assert!(text.starts_with("## 01 The Window\n\nStudio is one window; see [CAD](https://eustress.dev/docs/cad) & here, it's `Ctrl+Z`.\n"), "{text}");
        assert!(!text.contains("drawing") && !text.contains("Copy") && !text.contains("Footer") && !text.contains("Learn"), "{text}");
        assert!(text.contains("\n- one\n- two\n  - nested\n"), "{text}");
        assert!(text.contains("\n1. a\n2. b\n"), "{text}");
        assert!(text.contains("\n| Key | Does |\n| --- | --- |\n| F5 | a \\| b |\n"), "{text}");
        assert!(text.contains("\n```\nlet x = 1;\nlet y = <2>;\n```\n"), "{text}");
        assert!(text.contains("\n### Next\n\nLast.\n"), "{text}");
        assert!(!text.contains('<') || text.contains("<2>"), "{text}");
        assert!(!text.contains("\n\n\n"));
        // the whole page, when there is no <main>
        let whole = html_to_text("<nav>menu</nav><h2>Only</h2><footer>f</footer>", 0);
        assert_eq!(whole, "## Only\n");
    }

    #[test]
    fn a_link_around_several_blocks_keeps_the_words_and_follows_them_with_the_address() {
        let html = "<ol><li><a href=\"/docs/x\"><div>1</div><h4>Title</h4><p>Words here.</p></a></li><li><a href=\"https://x.test/\">Plain</a></li></ol>";
        assert_eq!(html_to_text(html, 0), "1. 1\n\n#### Title\n\nWords here.\n<https://eustress.dev/docs/x>\n\n2. [Plain](https://x.test/)\n");
        let meta = html_to_text("<div><span class=\"meta-item\">23 min read</span><span class=\"meta-item\">Beginner</span></div>", 0);
        assert_eq!(meta, "23 min read Beginner\n");
    }

    #[test]
    fn an_empty_link_or_heading_leaves_nothing() {
        let text = html_to_text("<p>a<a href=\"/x\"><svg></svg></a>b</p><h3></h3><p>c</p>", 0);
        assert_eq!(text, "ab\n\nc\n");
    }

    #[test]
    fn the_full_text_file_follows_the_llms_convention() {
        let pages = vec![
            DocPage { route: "/docs/a".into(), body: "<div><main><h1>Alpha</h1><p>First guide.</p></main></div>".into() },
            DocPage { route: "/learn".into(), body: "<h1>Learn</h1><h2>Part</h2><p>Second.</p>".into() },
        ];
        let md = vec![MarkdownDoc { path: "docs/api/X.md".into(), text: "# X\r\n\r\nmarkdown body\r\n".into() }];
        let text = llms_full(&pages, &md, "2026-10-15");
        assert!(text.starts_with(&format!("# Eustress Engine: full documentation\n\n> {LLMS_SUMMARY}\n\n")));
        assert!(text.contains("generated from the site on 2026-10-15"));
        let a = text.find("Source: https://eustress.dev/docs/a\n\n## Alpha\n\nFirst guide.\n").expect("first page");
        let b = text.find("Source: https://eustress.dev/learn\n\n## Learn\n\n### Part\n\nSecond.\n").expect("second page");
        let c = text.find("Source: https://eustress.dev/docs/api/X.md\n\n# X\n\nmarkdown body\n").expect("markdown doc");
        assert!(a < b && b < c);
        assert!(!text.contains('\r'));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn the_guides_are_the_docs_and_the_learn_pages() {
        for yes in ["/docs/studio", "/learn", "/learn/mcp"] {
            assert!(is_doc_route(yes), "{yes}");
        }
        for no in ["/", "/about", "/learner", "/docs", "/blog/docs/x", "/download"] {
            assert!(!is_doc_route(no), "{no}");
        }
        let guides = routes_from_sitemap(SITEMAP).into_iter().filter(|r| is_doc_route(r)).count();
        assert!(guides >= 23, "the sitemap lists the guides: {guides}");
    }

    #[test]
    fn llms_txt_points_at_the_full_text_and_says_the_same_summary() {
        assert!(LLMS.contains("](https://eustress.dev/llms-full.txt)"));
        assert!(LLMS.contains(&format!("> {LLMS_SUMMARY}")));
        for path in MARKDOWN_DOCS {
            assert!(LLMS.contains(&format!("https://eustress.dev/{path}")), "{path} is listed in llms.txt");
            assert!(INDEX.contains(path), "{path} is copied by Trunk");
        }
    }

    #[test]
    fn the_prerender_writes_everything_this_module_builds() {
        for call in [
            "splice(&shell, route, &body)",
            "splice_not_found(&shell, &body)",
            "sitemap_with_lastmod(",
            "llms_full(",
            "args.dist.join(\"llms-full.txt\")",
            "args.dist.join(\"404.html\")",
            "build_date(",
            "NOT_FOUND_PROBE",
            "is_doc_route(",
        ] {
            assert!(PRERENDER.contains(call), "prerender.rs no longer calls {call}");
        }
    }

    // ---- the live app -------------------------------------------------------

    #[test]
    fn the_live_app_writes_the_same_fields_the_prerender_does() {
        let sync = APP.split("fn TitleSync").nth(1).expect("TitleSync").split("mod dom").next().unwrap();
        assert!(sync.contains("for field in crate::seo::head_fields(&pathname.get_untracked())"), "TitleSync reads the shared head fields for the current path");
        assert!(sync.contains("set_attribute(field.attr"), "and writes each one");
        assert!(PRERENDER.contains("splice(&shell, route, &body)"), "the prerender reads the same table through splice");
    }

    #[test]
    fn an_unknown_address_is_the_not_found_page() {
        assert!(APP.contains("<Routes fallback=NotFoundPage>"));
        assert!(!APP.contains("fallback=|| \"Not found.\""));
    }

    // ---- robots.txt ---------------------------------------------------------

    #[derive(Default)]
    struct Group {
        agents: Vec<String>,
        rules: Vec<(bool, String)>,
    }

    fn parse_robots(text: &str) -> Vec<Group> {
        let mut groups: Vec<Group> = Vec::new();
        let mut last_was_agent = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap().trim();
            let Some((key, value)) = line.split_once(':') else { continue };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match key.as_str() {
                "user-agent" => {
                    if !last_was_agent {
                        groups.push(Group::default());
                    }
                    groups.last_mut().unwrap().agents.push(value.to_ascii_lowercase());
                    last_was_agent = true;
                }
                "allow" | "disallow" => {
                    last_was_agent = false;
                    if let Some(g) = groups.last_mut() {
                        if !value.is_empty() {
                            g.rules.push((key == "allow", value.to_string()));
                        }
                    }
                }
                _ => {}
            }
        }
        groups
    }

    fn rule_matches(rule: &str, path: &str) -> bool {
        match rule.strip_suffix('$') {
            Some(exact) => glob(exact, path, true),
            None => glob(rule, path, false),
        }
    }

    fn glob(pattern: &str, path: &str, whole: bool) -> bool {
        match pattern.split_once('*') {
            None => if whole { path == pattern } else { path.starts_with(pattern) },
            Some((head, tail)) => {
                path.starts_with(head)
                    && (0..=path.len() - head.len()).any(|i| path.is_char_boundary(head.len() + i) && glob(tail, &path[head.len() + i..], whole))
            }
        }
    }

    fn group_of<'a>(groups: &'a [Group], agent: &str) -> &'a Group {
        let agent = agent.to_ascii_lowercase();
        groups.iter().find(|g| g.agents.contains(&agent)).or_else(|| groups.iter().find(|g| g.agents.iter().any(|a| a == "*"))).expect("a group")
    }

    /// Google's rule (RFC 9309): the longest matching rule wins; a tie is allowed.
    fn longest_match_allows(group: &Group, path: &str) -> bool {
        let mut best: Option<(usize, bool)> = None;
        for (allow, rule) in &group.rules {
            if rule_matches(rule, path) {
                let len = rule.len();
                best = match best {
                    Some((l, a)) if l > len || (l == len && a) => Some((l, a)),
                    _ => Some((len, *allow)),
                };
            }
        }
        best.map_or(true, |(_, allow)| allow)
    }

    /// The older rule: the first matching line, in file order, decides.
    fn first_match_allows(group: &Group, path: &str) -> bool {
        group.rules.iter().find(|(_, rule)| rule_matches(rule, path)).map_or(true, |(allow, _)| *allow)
    }

    const AI_AGENTS: [&str; 5] = ["GPTBot", "ClaudeBot", "CCBot", "Google-Extended", "Applebot-Extended"];
    const AI_OPEN: [&str; 14] = [
        "/llms.txt", "/llms-full.txt", "/docs/studio", "/docs/api/WEBSITE_SERVICE_API.md", "/learn", "/learn/mcp", "/about", "/license", "/download",
        "/downloads/player", "/docs/getting-started", "/learn/cli", "/docs/x", "/learn/",
    ];
    const AI_CLOSED: [&str; 14] = [
        "/", "/gallery", "/marketplace", "/community", "/blog", "/blog/indie-studios", "/bliss", "/simulation/abc", "/profile/ada", "/dashboard", "/login",
        "/contact", "/press", "/sitemap.xml",
    ];

    #[test]
    fn each_ai_crawler_may_read_the_docs_and_nothing_else() {
        let groups = parse_robots(ROBOTS);
        for agent in AI_AGENTS {
            let group = group_of(&groups, agent);
            assert_eq!(group.agents, vec![agent.to_ascii_lowercase()], "{agent} has a group of its own");
            for path in AI_OPEN {
                assert!(longest_match_allows(group, path), "{agent} may read {path} (longest match)");
                assert!(first_match_allows(group, path), "{agent} may read {path} (first match)");
            }
            for path in AI_CLOSED {
                assert!(!longest_match_allows(group, path), "{agent} may not read {path} (longest match)");
                assert!(!first_match_allows(group, path), "{agent} may not read {path} (first match)");
            }
        }
    }

    #[test]
    fn the_ai_groups_name_exactly_the_agreed_paths_with_the_catch_all_last() {
        let groups = parse_robots(ROBOTS);
        for agent in AI_AGENTS {
            let rules = &group_of(&groups, agent).rules;
            let allows: Vec<&str> = rules.iter().filter(|(a, _)| *a).map(|(_, p)| p.as_str()).collect();
            assert_eq!(
                allows,
                ["/llms.txt", "/llms-full.txt", "/docs/", "/learn", "/about", "/license", "/download", "/downloads/player"],
                "{agent}"
            );
            assert_eq!(rules.last(), Some(&(false, "/".to_string())), "{agent}: Disallow: / comes after every Allow");
            assert_eq!(rules.len(), 9, "{agent}");
        }
    }

    #[test]
    fn every_other_crawler_gets_the_general_group_unchanged() {
        let groups = parse_robots(ROBOTS);
        assert_eq!(groups.len(), 1 + AI_AGENTS.len());
        let general = group_of(&groups, "Googlebot");
        assert_eq!(general.agents, vec!["*".to_string()]);
        let expected: Vec<(bool, &str)> = vec![
            (true, "/"), (false, "/admin"), (false, "/creator"), (false, "/dashboard"), (false, "/friends"), (false, "/kyc"), (false, "/projects"),
            (false, "/purchases"), (false, "/settings"), (false, "/tickets"), (false, "/verify"),
        ];
        let actual: Vec<(bool, &str)> = general.rules.iter().map(|(a, p)| (*a, p.as_str())).collect();
        assert_eq!(actual, expected);
        for agent in ["Googlebot", "bingbot", "SomeNewBot"] {
            let group = group_of(&groups, agent);
            for path in ["/", "/gallery", "/docs/studio", "/marketplace", "/community", "/llms.txt"] {
                assert!(longest_match_allows(group, path), "{agent} may read {path}");
            }
            for path in ["/admin", "/dashboard", "/settings", "/verify", "/projects/abc"] {
                assert!(!longest_match_allows(group, path), "{agent} may not read {path}");
            }
        }
        assert!(ROBOTS.contains("Sitemap: https://eustress.dev/sitemap.xml"));
    }

    #[test]
    fn the_paths_robots_txt_allows_are_routes_the_app_has() {
        let routes = app_routes();
        for p in ["/about", "/license", "/download", "/downloads/player", "/learn"] {
            assert!(routes.iter().any(|r| r == p), "{p} is a route");
        }
        assert!(routes.iter().any(|r| r.starts_with("/docs/")) && routes.iter().any(|r| r.starts_with("/learn/")));
        assert!(!routes.iter().any(|r| r == "/docs"), "/docs has no index page: only /docs/<guide>");
    }

    // ---- _redirects ---------------------------------------------------------

    /// Every `path!("...")` in the router.
    fn app_routes() -> Vec<String> {
        let mut routes = Vec::new();
        let mut rest = APP;
        while let Some(at) = rest.find("path!(\"") {
            let after = &rest[at + 7..];
            let end = after.find('"').unwrap();
            routes.push(after[..end].to_string());
            rest = &after[end..];
        }
        routes
    }

    fn rules() -> Vec<(String, String, u16)> {
        REDIRECTS
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                assert!(parts.len() == 3, "a rule is `from to status`: {l}");
                (parts[0].to_string(), parts[1].to_string(), parts[2].parse().expect("status"))
            })
            .collect()
    }

    /// `/simulation/:id` and `/simulation/:slug` are the same shape.
    fn shape(path: &str) -> String {
        path.split('/').map(|s| if s.starts_with(':') { ":" } else { s }).collect::<Vec<_>>().join("/")
    }

    #[test]
    fn only_the_apps_own_pages_are_rewritten_to_the_shell() {
        let rules = rules();
        assert!(!rules.iter().any(|(from, _, _)| from.contains('*')), "no catch-all: an unknown address must reach 404.html");
        for (from, to, status) in &rules {
            if *status == 200 {
                // "/" is the shell. A rewrite to "/index.html" answers 308 to "/" on Pages (staging, 2026-10-15).
                assert_eq!(to, "/", "{from}");
            } else {
                assert!(matches!(status, 301 | 302), "{from}");
            }
        }
        let rewritten: Vec<&str> = rules.iter().filter(|(_, _, s)| *s == 200).map(|(f, _, _)| f.as_str()).collect();
        for family in ["/simulation/:id", "/marketplace/:id", "/profile/:username"] {
            assert!(rewritten.contains(&family), "{family} is rewritten");
        }
        // the old addresses still redirect
        assert!(rules.contains(&("/simulations".into(), "/gallery".into(), 301)));
        assert!(rules.contains(&("/play/:id".into(), "/simulation/:id?play=1".into(), 302)));
        assert!(rules.contains(&("/creator".into(), "/projects".into(), 301)));
        assert!(rules.contains(&("/creator/:id".into(), "/projects/:id".into(), 301)));
        assert!(rules.len() < 100, "Cloudflare Pages allows 100 dynamic rules: {}", rules.len());
    }

    #[test]
    fn every_route_the_app_has_is_a_file_a_redirect_or_a_rewrite() {
        let sitemap = routes_from_sitemap(SITEMAP);
        let rules = rules();
        // The internal draft answers like an address the site does not have, status included.
        let unlisted_on_purpose = ["/economics-draft"];
        for route in app_routes() {
            if sitemap.contains(&route) || unlisted_on_purpose.contains(&route.as_str()) {
                continue;
            }
            assert!(rules.iter().any(|(from, _, _)| shape(from) == shape(&route)), "{route} would answer 404: it is no prerendered file, redirect or rewrite");
        }
        for (from, _, status) in &rules {
            if *status == 200 {
                assert!(app_routes().iter().any(|r| shape(r) == shape(from)), "{from} is rewritten but the app has no such route");
            }
        }
        assert!(!rules.iter().any(|(from, _, _)| from == "/economics-draft"));
        // a prerendered route is never rewritten over its own file
        for (from, _, status) in &rules {
            assert!(!(*status == 200 && sitemap.contains(from)), "{from} has a prerendered file");
        }
    }

    // ---- index.html structured data ------------------------------------------

    fn json_ld() -> Vec<serde_json::Value> {
        let mut blocks = Vec::new();
        let mut rest = INDEX;
        while let Some(at) = rest.find("<script type=\"application/ld+json\">") {
            let after = &rest[at + "<script type=\"application/ld+json\">".len()..];
            let end = after.find("</script>").unwrap();
            blocks.push(serde_json::from_str(&after[..end]).expect("each block is valid JSON"));
            rest = &after[end..];
        }
        blocks
    }

    #[test]
    fn the_structured_data_is_valid_and_only_says_what_the_site_stands_behind() {
        let blocks = json_ld();
        let types: Vec<&str> = blocks.iter().map(|b| b["@type"].as_str().unwrap()).collect();
        assert_eq!(types, ["Organization", "SoftwareApplication", "WebSite"]);
        assert!(!INDEX.contains("WebAPI") && !INDEX.contains("SoftwareSourceCode"));
        assert_eq!(
            blocks[0]["sameAs"],
            serde_json::json!(["https://x.com/eustressengine", "https://discord.gg/DGP9my8DYN", "https://github.com/WeaveITMeta/EustressEngine"])
        );
        assert_eq!(blocks[1]["operatingSystem"], "Windows, macOS, Linux");
        assert!(blocks[1].get("screenshot").is_none() && blocks[1].get("releaseNotes").is_none());
        assert!(!INDEX.contains("store.steampowered.com") && !INDEX.contains("twitter.com/eustressengine") && !INDEX.contains("discord.gg/eustress\""));
        assert_eq!(blocks[0]["contactPoint"]["url"], "https://discord.gg/DGP9my8DYN");
    }

    #[test]
    fn every_address_in_the_structured_data_exists() {
        let routes = routes_from_sitemap(SITEMAP);
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut seen = 0;
        for block in json_ld() {
            let mut stack = vec![block];
            while let Some(v) = stack.pop() {
                match v {
                    serde_json::Value::String(s) => {
                        if let Some(path) = s.strip_prefix("https://eustress.dev") {
                            seen += 1;
                            let path = path.split('?').next().unwrap();
                            if let Some(asset) = path.strip_prefix("/assets/") {
                                assert!(assets.join(asset).is_file(), "{s} is not in assets/");
                            } else {
                                let route = if path.is_empty() { "/" } else { path };
                                assert!(routes.iter().any(|r| r == route), "{s} is not a page in the sitemap");
                            }
                        }
                    }
                    serde_json::Value::Array(a) => stack.extend(a),
                    serde_json::Value::Object(o) => stack.extend(o.into_iter().map(|(_, v)| v)),
                    _ => {}
                }
            }
        }
        assert!(seen >= 3, "the check looked at {seen} addresses");
    }
}
