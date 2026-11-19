// =============================================================================
// Eustress Web - A listing's themed page: what the Worker says, and the rules that make it safe to draw
// =============================================================================
// A creator themes the public page of a Space from the Website service in Studio (docs/design/SIM_PAGE_THEME_AND_LINKS.md). The
// Worker validates, reviews, and serves only the approved parts:
//
//   GET /api/simulations/{id}/page    the approved theme, images (as addresses) and links, and the reviewed README
//
// A listing may have a README and no theme: the Worker then answers `theme: null`, and the page keeps its own look. The README is
// drawn by components/readme.rs (docs/design/SIM_PAGE_README_AND_TABS.md).
//
// A 404, a failed request, or an answer this file cannot read all mean "no themed page": the page draws exactly what it draws
// without one. Nothing here ever fails the page.
//
// The theme is data, never CSS. Every colour is checked again here as `#RRGGBB` and reaches the page only as a custom property
// value on the page's own root element; fonts, layouts, presets and motion are enums that choose a CSS class from a fixed list;
// the two timings are clamped numbers. No author string is ever put into CSS, HTML or a style attribute.
//
// Table of Contents:
// 1. What the Worker says
// 2. Fetch
// 3. The theme: colours, classes, style
// 4. Motion
// 5. Images
// 6. Links
// =============================================================================

use serde::Deserialize;

use super::ApiClient;

// -----------------------------------------------------------------------------
// 1. What the Worker says (PageSpec v1, the public view)
// -----------------------------------------------------------------------------

/// The five colour slots. Each is `#RRGGBB` or absent (the preset's colour is used).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PageColors {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PageTheme {
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub colors: PageColors,
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub layout: String,
    #[serde(default)]
    pub motion: String,
    /// Read as a number of any kind, then clamped: a Worker that sends 6.0 or 6 means the same.
    #[serde(default)]
    pub slideshow_interval_s: Option<f64>,
    #[serde(default)]
    pub banner_fade_ms: Option<f64>,
}

/// The approved images as addresses (the Worker's own image route). A slot that is not approved is absent.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PageImages {
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub gallery: Vec<String>,
}

/// One approved link, as the Worker serves it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PageLink {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub label: String,
}

/// The listing's README, as the Worker serves it (docs/design/SIM_PAGE_README_AND_TABS.md, section 2): the reviewed Markdown, and the
/// only link targets and image sources the page may draw from it. A link or an image that is not in these lists is drawn as text.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Readme {
    #[serde(default, deserialize_with = "null_as_default")]
    pub markdown: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub links: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub images: Vec<String>,
}

/// The public view of a listing's page. A listing with a README and no theme comes as `theme: null`: every part is optional.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct SimPage {
    #[serde(default)]
    pub theme: Option<PageTheme>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub images: PageImages,
    #[serde(default, deserialize_with = "null_as_default")]
    pub links: Vec<PageLink>,
    #[serde(default)]
    pub readme: Option<Readme>,
}

/// A `null` reads as the empty value, the same as an absent key.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Option::unwrap_or_default)
}

impl SimPage {
    /// Whether the page carries anything to draw at all.
    pub fn has_content(&self) -> bool {
        self.theme.is_some()
            || self.images.logo.is_some()
            || self.images.banner.is_some()
            || !self.images.gallery.is_empty()
            || !self.links.is_empty()
            || self.readme.as_ref().is_some_and(|r| !r.markdown.trim().is_empty())
    }
}

/// The answer, either as the page itself or wrapped as `{ "page": ... }`; both read the same.
#[derive(Debug, Default, Deserialize)]
struct PageReply {
    #[serde(default)]
    page: Option<SimPage>,
    #[serde(flatten)]
    top: SimPage,
}

impl PageReply {
    fn into_page(self) -> SimPage {
        self.page.unwrap_or(self.top)
    }
}

// -----------------------------------------------------------------------------
// 2. Fetch
// -----------------------------------------------------------------------------

/// The listing's approved page, or nothing. Every failure (404, network, an unreadable answer, an empty page) is nothing.
pub async fn fetch_page(client: &ApiClient, id: &str) -> Option<SimPage> {
    let path = format!("/api/simulations/{}/page", urlencoding::encode(id));
    client.get::<PageReply>(&path).await.ok().map(PageReply::into_page).filter(SimPage::has_content)
}

// -----------------------------------------------------------------------------
// 3. The theme: colours, classes, style
// -----------------------------------------------------------------------------

/// `#RRGGBB`: a hash and six hex digits, nothing else. The only author-chosen text that ever reaches a style value.
pub fn is_hex_colour(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

/// A colour the page may use, lower-cased, or nothing.
fn clean_colour(c: &Option<String>) -> Option<String> {
    c.as_deref().filter(|s| is_hex_colour(s)).map(|s| s.to_ascii_lowercase())
}

/// A colour's relative luminance (WCAG 2.x). The input is a validated `#RRGGBB`.
fn luminance(hex: &str) -> f64 {
    let channel = |at: usize| {
        let v = u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0) as f64 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

/// The WCAG contrast ratio of two validated colours, 1 to 21.
pub fn contrast_ratio(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// A preset's name as the page knows it, and its five colours: primary, accent, background, surface, text.
/// An unknown preset is `studio`. Every preset's text reaches 4.5:1 on its background and surface.
pub fn preset_of(name: &str) -> (&'static str, [&'static str; 5]) {
    match name {
        "daylight" => ("daylight", ["#2563eb", "#b45309", "#f5f7fa", "#ffffff", "#14181f"]),
        "blueprint" => ("blueprint", ["#7dd3fc", "#fde68a", "#0b2a4a", "#12385f", "#eaf4ff"]),
        "laboratory" => ("laboratory", ["#0f766e", "#7c3aed", "#f1f5f4", "#ffffff", "#10201e"]),
        "arcade" => ("arcade", ["#ff3ea5", "#38f2d0", "#120b22", "#1d1236", "#f6eeff"]),
        "paper" => ("paper", ["#8b4513", "#b45309", "#f4efe6", "#fbf8f2", "#2a2118"]),
        _ => ("studio", ["#4f8cff", "#ffb547", "#0e1116", "#171b22", "#e8ecf2"]),
    }
}

fn font_of(name: &str) -> &'static str {
    match name {
        "space-grotesk" => "space-grotesk",
        "ibm-plex-sans" => "ibm-plex-sans",
        "jetbrains-mono" => "jetbrains-mono",
        "merriweather" => "merriweather",
        _ => "inter",
    }
}

fn layout_of(name: &str) -> &'static str {
    match name {
        "wide" => "wide",
        "compact" => "compact",
        _ => "classic",
    }
}

/// How much the page moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    None,
    Subtle,
    Full,
}

impl Motion {
    /// `none` and `full` are themselves; anything else (including nothing said) is the gentle default.
    pub fn parse(s: &str) -> Motion {
        match s {
            "none" => Motion::None,
            "full" => Motion::Full,
            _ => Motion::Subtle,
        }
    }

    fn class(self) -> &'static str {
        match self {
            Motion::None => "none",
            Motion::Subtle => "subtle",
            Motion::Full => "full",
        }
    }
}

/// The theme after every rule has been applied: only values the page may use.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeView {
    pub preset: &'static str,
    pub font: &'static str,
    pub layout: &'static str,
    pub motion: Motion,
    /// primary, accent, background, surface, text: lower-case `#rrggbb`, each one validated.
    pub colors: [String; 5],
    pub interval_s: u32,
    pub fade_ms: u32,
}

/// The slots, in the order of [`ThemeView::colors`], as the custom properties the stylesheet reads.
const COLOUR_VARS: [&str; 5] = ["--sp-primary", "--sp-accent", "--sp-bg", "--sp-surface", "--sp-text"];

/// Apply every rule to a theme the Worker sent. A colour that is not `#RRGGBB` is the preset's. If the text does not reach 4.5:1
/// on the background and on the surface it is the preset's text, and if that still does not (an author's light background under a
/// dark preset's text), the background and surface are the preset's too: the page never shows text a reader cannot read.
pub fn theme_view(t: &PageTheme) -> ThemeView {
    let (preset, base) = preset_of(&t.preset);
    let pick = |c: &Option<String>, i: usize| clean_colour(c).unwrap_or_else(|| base[i].to_string());
    let mut colors = [
        pick(&t.colors.primary, 0),
        pick(&t.colors.accent, 1),
        pick(&t.colors.background, 2),
        pick(&t.colors.surface, 3),
        pick(&t.colors.text, 4),
    ];
    let readable = |c: &[String; 5]| contrast_ratio(&c[4], &c[2]) >= 4.5 && contrast_ratio(&c[4], &c[3]) >= 4.5;
    if !readable(&colors) {
        colors[4] = base[4].to_string();
        if !readable(&colors) {
            colors[2] = base[2].to_string();
            colors[3] = base[3].to_string();
        }
    }
    let plan = motion_plan(Motion::parse(&t.motion), false, t.slideshow_interval_s, t.banner_fade_ms);
    ThemeView {
        preset,
        font: font_of(&t.font),
        layout: layout_of(&t.layout),
        motion: Motion::parse(&t.motion),
        colors,
        interval_s: plan.interval_ms / 1000,
        fade_ms: if Motion::parse(&t.motion) == Motion::None { 0 } else { clamp_fade(t.banner_fade_ms) },
    }
}

/// The class list of the page's own root element. Every word comes from a fixed list; an unthemed page has the one inert word.
pub fn root_class(theme: Option<&ThemeView>) -> String {
    match theme {
        None => "sp-root".to_string(),
        Some(t) => format!("sp-root sp-themed sp-preset-{} sp-font-{} sp-layout-{} sp-motion-{}", t.preset, t.font, t.layout, t.motion.class()),
    }
}

/// The root element's style attribute: the five validated colours and the banner's fade, as custom properties. Nothing else.
pub fn root_style(theme: Option<&ThemeView>) -> String {
    let Some(t) = theme else { return String::new() };
    let mut s = String::new();
    for (name, value) in COLOUR_VARS.iter().zip(t.colors.iter()) {
        // The last line of defence: a value that is not a hex colour is never written.
        if is_hex_colour(value) {
            s.push_str(name);
            s.push(':');
            s.push_str(value);
            s.push(';');
        }
    }
    s.push_str(&format!("--sp-fade:{}ms;", t.fade_ms));
    s
}

// -----------------------------------------------------------------------------
// 4. Motion
// -----------------------------------------------------------------------------

/// What the page does with time: whether anything moves by itself, the gap between gallery slides, and the banner's fade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPlan {
    /// Whether the gallery advances on its own. The arrows and dots always work.
    pub animate: bool,
    pub interval_ms: u32,
    pub fade_ms: u32,
}

fn clamp_fade(v: Option<f64>) -> u32 {
    v.filter(|n| n.is_finite()).map(|n| n.round().clamp(0.0, 2000.0) as u32).unwrap_or(600)
}

fn clamp_interval_s(v: Option<f64>) -> u32 {
    v.filter(|n| n.is_finite()).map(|n| n.round().clamp(3.0, 30.0) as u32).unwrap_or(6)
}

/// The decision: `none` moves nothing; a reader who asked for less motion gets nothing moving whatever the theme says; `subtle` and
/// `full` fade the banner and run the slideshow at the given timings (clamped to 3 to 30 seconds and 0 to 2000 ms).
pub fn motion_plan(motion: Motion, reduced: bool, interval_s: Option<f64>, fade_ms: Option<f64>) -> MotionPlan {
    let interval_ms = clamp_interval_s(interval_s) * 1000;
    if motion == Motion::None || reduced {
        return MotionPlan { animate: false, interval_ms, fade_ms: 0 };
    }
    MotionPlan { animate: true, interval_ms, fade_ms: clamp_fade(fade_ms) }
}

// -----------------------------------------------------------------------------
// 5. Images
// -----------------------------------------------------------------------------

/// `https://host[:port]` of an https address, lower-cased, or nothing.
fn origin_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    Some(format!("https://{}", authority.to_ascii_lowercase()))
}

/// The address an `<img>` may use, or nothing. Only the API's own origin is allowed: a path is joined onto the API's address, and an
/// absolute https address must be on that same origin. An address with a space, a control character, a quote, a bracket or a
/// backslash is never used.
pub fn image_src(url: &str, api_base: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.len() > 600 || url.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`')) {
        return None;
    }
    if url.starts_with("//") {
        return None;
    }
    if url.starts_with('/') {
        return Some(format!("{}{}", api_base.trim_end_matches('/'), url));
    }
    let theirs = origin_of(url)?;
    (origin_of(api_base.trim_end_matches('/')).as_deref() == Some(theirs.as_str())).then(|| url.to_string())
}

/// The gallery's usable addresses: at most six, none twice.
pub fn gallery_srcs(images: &PageImages, api_base: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for u in &images.gallery {
        if let Some(src) = image_src(u, api_base) {
            if !out.contains(&src) && out.len() < 6 {
                out.push(src);
            }
        }
    }
    out
}

// -----------------------------------------------------------------------------
// 6. Links
// -----------------------------------------------------------------------------

/// The group a link sits in. The order here is the order the page shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkKind {
    Website,
    Social,
    Shop,
    Other,
}

impl LinkKind {
    /// A kind the page does not know is `Other`.
    pub fn parse(s: &str) -> LinkKind {
        match s {
            "website" => LinkKind::Website,
            "social" => LinkKind::Social,
            "shop" => LinkKind::Shop,
            _ => LinkKind::Other,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            LinkKind::Website => "Website",
            LinkKind::Social => "Social",
            LinkKind::Shop => "Shop",
            LinkKind::Other => "Other",
        }
    }
}

/// A link the page will draw: its address is https, its label is short plain text, its domain is computed from the address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkView {
    pub kind: LinkKind,
    pub url: String,
    pub label: String,
    pub domain: String,
}

/// The host of an https address, lower-case and without "www.", or nothing when the address is not one a link may be: no user or
/// password, no port, no IPv6 literal, no IPv4 literal, no empty label, only letters, digits and hyphens in a label.
pub fn display_domain(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() || host.contains('@') || host.contains(':') || host.starts_with('[') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty() || l.starts_with('-') || l.ends_with('-') || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        return None;
    }
    // An IPv4 literal is all digits and dots.
    if labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let shown = host.strip_prefix("www.").unwrap_or(&host);
    // "www.com" would leave a one-label host; keep the whole thing then.
    Some(if shown.contains('.') { shown.to_string() } else { host.clone() })
}

/// The links the page may draw, in the order the Worker sent them: https only, a clean address of at most 500 characters, a label
/// that is not empty (cut to 40 characters, control characters dropped), at most 17, no address twice. Anything else is skipped.
pub fn visible_links(links: &[PageLink]) -> Vec<LinkView> {
    let mut out: Vec<LinkView> = Vec::new();
    for l in links {
        if out.len() >= 17 {
            break;
        }
        let url = l.url.as_str();
        if url.len() > 500
            || !url.starts_with("https://")
            || url.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`'))
            || out.iter().any(|seen| seen.url == url)
        {
            continue;
        }
        let Some(domain) = display_domain(url) else { continue };
        let label: String = l.label.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(40).collect();
        let label = label.trim().to_string();
        if label.is_empty() {
            continue;
        }
        out.push(LinkView { kind: LinkKind::parse(&l.kind), url: url.to_string(), label, domain });
    }
    out
}

/// The links in their groups (Website, Social, Shop, Other), a group with no link left out, each group in the order sent.
pub fn grouped(links: &[LinkView]) -> Vec<(LinkKind, Vec<LinkView>)> {
    [LinkKind::Website, LinkKind::Social, LinkKind::Shop, LinkKind::Other]
        .into_iter()
        .filter_map(|k| {
            let items: Vec<LinkView> = links.iter().filter(|l| l.kind == k).cloned().collect();
            (!items.is_empty()).then_some((k, items))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(kind: &str, url: &str, label: &str) -> PageLink {
        PageLink { kind: kind.to_string(), url: url.to_string(), label: label.to_string() }
    }

    #[test]
    fn a_colour_is_hash_and_six_hex_digits_and_nothing_else() {
        for ok in ["#4F8CFF", "#000000", "#abcdef", "#ABCDEF"] {
            assert!(is_hex_colour(ok), "{ok}");
        }
        for bad in ["", "#fff", "#12345", "#1234567", "4F8CFF", "#GGGGGG", "red", "#4F8CF;", "#4F8CFF;x:y", "url(x)", "#4f8cff\n", " #4f8cff", "#4f8cf\u{00e9}"] {
            assert!(!is_hex_colour(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_style_carries_only_validated_colours_and_a_number_whatever_the_author_sent() {
        let theme = PageTheme {
            preset: "studio\"><script>".to_string(),
            colors: PageColors {
                primary: Some("red;background:url(//evil)".to_string()),
                accent: Some("#FFB547".to_string()),
                background: Some("}</style><script>".to_string()),
                surface: None,
                text: Some("#E8ECF2".to_string()),
            },
            font: "x;font-family:evil".to_string(),
            layout: "\" onload=\"x".to_string(),
            motion: "full;x".to_string(),
            slideshow_interval_s: Some(f64::NAN),
            banner_fade_ms: Some(99999.0),
        };
        let v = theme_view(&theme);
        let style = root_style(Some(&v));
        let class = root_class(Some(&v));
        // Rebuilt from the preset and the one valid override; the hostile strings are nowhere.
        assert_eq!(style, "--sp-primary:#4f8cff;--sp-accent:#ffb547;--sp-bg:#0e1116;--sp-surface:#171b22;--sp-text:#e8ecf2;--sp-fade:2000ms;");
        assert_eq!(class, "sp-root sp-themed sp-preset-studio sp-font-inter sp-layout-classic sp-motion-subtle");
        for bad in ["script", "evil", "onload", "url(", "<", ">", "\""] {
            assert!(!style.contains(bad) && !class.contains(bad), "{bad}");
        }
        assert_eq!(root_class(None), "sp-root");
        assert_eq!(root_style(None), "");
    }

    #[test]
    fn unreadable_text_falls_back_to_the_presets_and_then_to_the_presets_background() {
        // Light text on a light background: the preset's text takes over (the preset is dark, the background override stays dark).
        let t = PageTheme { preset: "studio".into(), colors: PageColors { background: Some("#202020".into()), text: Some("#303030".into()), ..Default::default() }, ..Default::default() };
        let v = theme_view(&t);
        assert_eq!(v.colors[2], "#202020");
        assert_eq!(v.colors[4], "#e8ecf2");
        // A white background under the dark preset's pale text cannot be read at all: the preset's own background comes back too.
        let t = PageTheme { preset: "studio".into(), colors: PageColors { background: Some("#ffffff".into()), surface: Some("#fafafa".into()), ..Default::default() }, ..Default::default() };
        let v = theme_view(&t);
        assert_eq!((v.colors[2].as_str(), v.colors[3].as_str(), v.colors[4].as_str()), ("#0e1116", "#171b22", "#e8ecf2"));
        // Every preset reaches the bar on its own.
        for p in ["studio", "daylight", "blueprint", "laboratory", "arcade", "paper"] {
            let (_, c) = preset_of(p);
            assert!(contrast_ratio(c[4], c[2]) >= 4.5 && contrast_ratio(c[4], c[3]) >= 4.5, "{p}");
        }
        assert!((contrast_ratio("#000000", "#ffffff") - 21.0).abs() < 0.01);
    }

    #[test]
    fn a_domain_is_the_host_without_www_and_an_odd_address_has_none() {
        assert_eq!(display_domain("https://www.Example-Robotics.com/kits?a=1#x").as_deref(), Some("example-robotics.com"));
        assert_eq!(display_domain("https://shop.example.com").as_deref(), Some("shop.example.com"));
        assert_eq!(display_domain("https://youtube.com/@example").as_deref(), Some("youtube.com"));
        assert_eq!(display_domain("https://xn--bcher-kva.example/").as_deref(), Some("xn--bcher-kva.example"));
        for bad in [
            "http://example.com",
            "https://",
            "https://user:pw@example.com",
            "https://example.com@evil.com",
            "https://example.com:8443/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://localhost/",
            "https://exa mple.com",
            "https://-bad.example.com",
            "https://exa_mple.com",
            "javascript:alert(1)",
        ] {
            assert_eq!(display_domain(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_https_links_with_a_label_are_drawn_and_they_are_grouped_in_a_fixed_order() {
        let links = vec![
            link("shop", "https://shop.example.com/kits", "Buy the kit"),
            link("social", "https://youtube.com/@example", "YouTube"),
            link("website", "https://www.example.com", "Our lab"),
            link("social", "http://insecure.example.com", "Insecure"),
            link("other", "javascript:alert(1)", "Script"),
            link("other", "https://example.com/a b", "Space in it"),
            link("other", "https://example.com/doc", "   "),
            link("mystery", "https://docs.example.com", "Docs"),
            link("social", "https://youtube.com/@example", "Again"),
            link("website", "https://user@evil.example.com", "Userinfo"),
            link("website", "https://example.com:81/", "Port"),
        ];
        let seen = visible_links(&links);
        let urls: Vec<&str> = seen.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, ["https://shop.example.com/kits", "https://youtube.com/@example", "https://www.example.com", "https://docs.example.com"]);
        assert_eq!(seen[2].domain, "example.com");
        let groups = grouped(&seen);
        let kinds: Vec<&str> = groups.iter().map(|g| g.0.title()).collect();
        assert_eq!(kinds, ["Website", "Social", "Shop", "Other"], "an unknown kind is Other");
        assert_eq!(groups[3].1[0].label, "Docs");
        // A long label is cut, a long address is dropped, and no more than 17 are drawn.
        let long = visible_links(&[link("other", "https://example.com/x", &"a".repeat(80))]);
        assert_eq!(long[0].label.chars().count(), 40);
        assert!(visible_links(&[link("other", &format!("https://example.com/{}", "a".repeat(500)), "Long")]).is_empty());
        let many: Vec<PageLink> = (0..30).map(|i| link("other", &format!("https://example.com/{i}"), "x")).collect();
        assert_eq!(visible_links(&many).len(), 17);
        assert!(grouped(&[]).is_empty());
    }

    #[test]
    fn the_motion_decision_honours_none_and_the_readers_request_and_clamps_the_timings() {
        let go = motion_plan(Motion::Subtle, false, Some(6.0), Some(600.0));
        assert_eq!(go, MotionPlan { animate: true, interval_ms: 6000, fade_ms: 600 });
        assert!(motion_plan(Motion::Full, false, None, None).animate);
        assert_eq!(motion_plan(Motion::Full, false, None, None), MotionPlan { animate: true, interval_ms: 6000, fade_ms: 600 });
        assert!(!motion_plan(Motion::None, false, Some(6.0), Some(600.0)).animate);
        assert_eq!(motion_plan(Motion::Full, true, Some(6.0), Some(600.0)), MotionPlan { animate: false, interval_ms: 6000, fade_ms: 0 });
        assert_eq!(motion_plan(Motion::None, false, Some(6.0), Some(600.0)).fade_ms, 0);
        assert_eq!(motion_plan(Motion::Subtle, false, Some(1.0), Some(-5.0)), MotionPlan { animate: true, interval_ms: 3000, fade_ms: 0 });
        assert_eq!(motion_plan(Motion::Subtle, false, Some(500.0), Some(9000.0)), MotionPlan { animate: true, interval_ms: 30_000, fade_ms: 2000 });
        assert_eq!(Motion::parse("none"), Motion::None);
        assert_eq!(Motion::parse("full"), Motion::Full);
        assert_eq!(Motion::parse("subtle"), Motion::Subtle);
        assert_eq!(Motion::parse(""), Motion::Subtle);
        // `none` is the one word that turns the fade off in the style too.
        let none = theme_view(&PageTheme { motion: "none".into(), banner_fade_ms: Some(900.0), ..Default::default() });
        assert!(root_style(Some(&none)).ends_with("--sp-fade:0ms;"));
        assert!(root_class(Some(&none)).ends_with("sp-motion-none"));
    }

    #[test]
    fn an_image_comes_only_from_the_apis_own_origin() {
        let api = "https://api.example.dev";
        assert_eq!(image_src("/api/simulations/a/page/image/logo", api).as_deref(), Some("https://api.example.dev/api/simulations/a/page/image/logo"));
        assert_eq!(image_src("/api/x", "https://api.example.dev/").as_deref(), Some("https://api.example.dev/api/x"));
        assert_eq!(image_src("/api/x", "").as_deref(), Some("/api/x"));
        assert_eq!(image_src("https://API.example.dev/api/x", api).as_deref(), Some("https://API.example.dev/api/x"));
        for bad in [
            "https://evil.example/x.png",
            "https://api.example.dev.evil.example/x",
            "https://api.example.dev@evil.example/x",
            "//evil.example/x",
            "http://api.example.dev/x",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            "/ok\" onerror=\"x",
            "/a b",
            "",
        ] {
            assert_eq!(image_src(bad, api), None, "{bad}");
        }
        assert_eq!(image_src("https://api.example.dev/x", ""), None, "with no API origin to compare, an absolute address is not used");
        let imgs = PageImages {
            logo: None,
            banner: None,
            gallery: (0..9).map(|i| format!("/g/{}", i % 8)).chain(["https://evil.example/z".to_string()]).collect(),
        };
        let g = gallery_srcs(&imgs, api);
        assert_eq!(g.len(), 6, "at most six, none twice, none from elsewhere");
        assert!(g.iter().all(|u| u.starts_with("https://api.example.dev/g/")));
    }

    #[test]
    fn the_answer_reads_bare_or_wrapped_and_a_broken_one_is_nothing() {
        let bare = r##"{"theme":{"preset":"paper","colors":{"primary":"#112233"},"slideshow_interval_s":8,"banner_fade_ms":400.0},"images":{"logo":"/l","gallery":["/g0"]},"links":[{"kind":"website","url":"https://a.example","label":"A"}],"extra":1}"##;
        let wrapped = format!("{{\"page\":{bare}}}");
        let a: PageReply = serde_json::from_str(bare).unwrap();
        let b: PageReply = serde_json::from_str(&wrapped).unwrap();
        let (a, b) = (a.into_page(), b.into_page());
        assert_eq!(a, b);
        assert_eq!(a.theme.as_ref().unwrap().preset, "paper");
        assert_eq!(a.theme.as_ref().unwrap().slideshow_interval_s, Some(8.0));
        assert_eq!(a.links.len(), 1);
        assert!(a.has_content());
        let empty: PageReply = serde_json::from_str("{}").unwrap();
        assert!(!empty.into_page().has_content());
        assert!(serde_json::from_str::<PageReply>("[1,2]").is_err());
    }

    #[test]
    fn a_readme_without_a_theme_reads_as_a_page_with_content_and_no_theme() {
        let body = r##"{"theme":null,"images":null,"links":null,"readme":{"markdown":"# Battery lab\n\nSee [our paper](https://example.edu/paper)","links":["https://example.edu/paper"],"images":null}}"##;
        let page = serde_json::from_str::<PageReply>(body).unwrap().into_page();
        assert!(page.theme.is_none() && page.links.is_empty() && page.images == PageImages::default());
        let readme = page.readme.as_ref().expect("the readme");
        assert_eq!(readme.links, ["https://example.edu/paper"]);
        assert!(readme.images.is_empty(), "a null list is an empty list");
        assert!(page.has_content(), "a README alone is a page");
        // Wrapped reads the same; no readme, or a blank one, is not content on its own.
        let wrapped = serde_json::from_str::<PageReply>(&format!("{{\"page\":{body}}}")).unwrap().into_page();
        assert_eq!(wrapped, page);
        let none = serde_json::from_str::<PageReply>(r#"{"theme":null,"readme":null}"#).unwrap().into_page();
        assert!(none.readme.is_none() && !none.has_content());
        let blank = serde_json::from_str::<PageReply>(r#"{"readme":{"markdown":"  \n"}}"#).unwrap().into_page();
        assert!(!blank.has_content());
    }
}
