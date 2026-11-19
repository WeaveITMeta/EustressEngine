//! # PageSpec v1: the themed, linked sim page
//!
//! A creator themes their simulation's public page from the Website service
//! and lists off-site links. Studio bakes that into one JSON document
//! (`PageSpec v1`, `docs/design/SIM_PAGE_THEME_AND_LINKS.md` section 2), the
//! Worker validates and reviews it, and the website renders only what review
//! approved.
//!
//! This module is the one place the rules live on the Studio side, so the
//! publish bake (`engine/src/website/page.rs`), the MCP tools
//! (`tools/src/website_tools.rs`) and the Properties panel agree:
//!
//! - the six preset palettes ([`PRESETS`]), the same hex values the Worker
//!   carries (see the "Preset palettes" section of the design document);
//! - the enums (font, layout, motion, link kind) and the number ranges;
//! - the URL rules for a link ([`validate_link_url`]), the same host rules as
//!   `cleanWebhookUrl` in the Worker's `src/commerce.mjs`;
//! - the label, image path and image bytes rules;
//! - [`build_page_spec`], which turns what the author set into the JSON and a
//!   list of warnings for everything it had to drop;
//! - the README rules for the page's About tab
//!   (`docs/design/SIM_PAGE_README_AND_TABS.md`): the size cap
//!   ([`README_MAX_BYTES`]), how an image path resolves
//!   ([`resolve_readme_image`]) and the image rewrite ([`rewrite_readme`]).
//!
//! Pure functions only: no file or network access, so every rule is unit
//! tested here and the callers stay thin.
//!
//! ## Drop, never half-send
//!
//! The Worker refuses a spec that breaks a rule whole. So Studio never sends a
//! value that breaks one: an invalid entry is dropped and named in a warning
//! the publish log shows, and the rest of the page still publishes.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{json, Map, Value};

/// The only `schema` value this module writes.
pub const SCHEMA: u32 = 1;
/// Longest link URL, in characters.
pub const MAX_URL_LEN: usize = 500;
/// Longest link label, in characters.
pub const MAX_LABEL_CHARS: usize = 40;
/// Largest image the Worker takes, in bytes.
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
/// Gallery images (`gallery-0` to `gallery-5`).
pub const MAX_GALLERY: usize = 6;
/// Links in all.
pub const MAX_LINKS: usize = 17;

/// The value of an enum row that means "the author chose nothing".
pub const UNSET: &str = "default";

/// Fonts the page maps to a font stack.
pub const FONTS: &[&str] = &["inter", "space-grotesk", "ibm-plex-sans", "jetbrains-mono", "merriweather"];
/// Page layouts.
pub const LAYOUTS: &[&str] = &["classic", "wide", "compact"];
/// Motion levels. The page always honours the visitor's `prefers-reduced-motion`.
pub const MOTIONS: &[&str] = &["none", "subtle", "full"];
/// Link kinds, and how many of each a page may carry.
pub const LINK_KINDS: &[(&str, usize)] = &[("website", 1), ("social", 8), ("shop", 4), ("other", 4)];

/// What an unset font, layout and motion resolve to in the baked spec.
pub const DEFAULT_PRESET: &str = "studio";
pub const DEFAULT_FONT: &str = "inter";
pub const DEFAULT_LAYOUT: &str = "classic";
pub const DEFAULT_MOTION: &str = "subtle";
/// Gallery slideshow interval, seconds: default and range.
pub const DEFAULT_SLIDESHOW_INTERVAL_S: i64 = 6;
pub const SLIDESHOW_INTERVAL_RANGE: (i64, i64) = (3, 30);
/// Banner fade, milliseconds: default and range.
pub const DEFAULT_BANNER_FADE_MS: i64 = 600;
pub const BANNER_FADE_RANGE: (i64, i64) = (0, 2000);

/// The five colour keys a palette and an override both use.
pub const COLOR_KEYS: [&str; 5] = ["primary", "accent", "background", "surface", "text"];

/// Contrast ratio (WCAG AA, normal text) `text` must reach on `background`
/// and on `surface`.
pub const MIN_TEXT_CONTRAST: f64 = 4.5;

// ============================================================================
// Presets
// ============================================================================

/// One preset's full colour set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub name: &'static str,
    pub primary: &'static str,
    pub accent: &'static str,
    pub background: &'static str,
    pub surface: &'static str,
    pub text: &'static str,
}

impl Palette {
    /// The colour under one of [`COLOR_KEYS`].
    pub fn color(&self, key: &str) -> Option<&'static str> {
        match key {
            "primary" => Some(self.primary),
            "accent" => Some(self.accent),
            "background" => Some(self.background),
            "surface" => Some(self.surface),
            "text" => Some(self.text),
            _ => None,
        }
    }
}

/// The six presets. The Worker carries the same values independently
/// (`docs/design/SIM_PAGE_THEME_AND_LINKS.md`, "Preset palettes"); change one
/// side and the other in the same commit. Every text colour reaches 4.5:1 on
/// its background and its surface (a unit test computes it).
pub const PRESETS: [Palette; 6] = [
    Palette { name: "studio", primary: "#4F8CFF", accent: "#FFB547", background: "#0E1116", surface: "#171B22", text: "#E8ECF2" },
    Palette { name: "daylight", primary: "#1F5FCC", accent: "#B93C0A", background: "#FFFFFF", surface: "#F1F4F9", text: "#111827" },
    Palette { name: "blueprint", primary: "#8DB8FF", accent: "#FFD166", background: "#0B2545", surface: "#13315C", text: "#EAF2FF" },
    Palette { name: "laboratory", primary: "#0B6E78", accent: "#B2400C", background: "#F3F9F8", surface: "#FFFFFF", text: "#0F2A2E" },
    Palette { name: "arcade", primary: "#FF4DB2", accent: "#3DF5E0", background: "#120B2E", surface: "#1E1450", text: "#F5F0FF" },
    Palette { name: "paper", primary: "#8A3B12", accent: "#2F6B4F", background: "#FAF6EE", surface: "#F0E8D6", text: "#2B2620" },
];

/// The preset called `name` (lower case), if there is one.
pub fn preset(name: &str) -> Option<&'static Palette> {
    PRESETS.iter().find(|p| p.name == name)
}

/// The preset names, in the order the Properties panel lists them.
pub fn preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.name).collect()
}

// ============================================================================
// Colour and contrast
// ============================================================================

/// `#RRGGBB` as three bytes. Exactly that shape: no short form, no alpha.
pub fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let digits = value.trim().strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&digits[0..2], 16).ok()?;
    let g = u8::from_str_radix(&digits[2..4], 16).ok()?;
    let b = u8::from_str_radix(&digits[4..6], 16).ok()?;
    Some((r, g, b))
}

/// A colour in the one spelling the spec uses: `#RRGGBB`, upper case.
pub fn normalise_hex(value: &str) -> Option<String> {
    let (r, g, b) = parse_hex(value)?;
    Some(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

fn linear_channel(c: u8) -> f64 {
    let s = c as f64 / 255.0;
    if s <= 0.03928 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
pub fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
    0.2126 * linear_channel(r) + 0.7152 * linear_channel(g) + 0.0722 * linear_channel(b)
}

/// WCAG contrast ratio between two `#RRGGBB` colours (1.0 to 21.0), or `None`
/// when either is not one.
pub fn contrast_ratio(a: &str, b: &str) -> Option<f64> {
    let la = relative_luminance(parse_hex(a)?);
    let lb = relative_luminance(parse_hex(b)?);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    Some((hi + 0.05) / (lo + 0.05))
}

// ============================================================================
// Enum helpers
// ============================================================================

/// An enum value in the spelling the spec uses: trimmed, lower case. `None`
/// when empty or [`UNSET`] ("the author chose nothing").
fn chosen(value: &str) -> Option<String> {
    let v = value.trim().to_ascii_lowercase();
    if v.is_empty() || v == UNSET {
        None
    } else {
        Some(v)
    }
}

/// `value` lower-cased when it is one of `allowed`.
pub fn check_enum(value: &str, allowed: &[&str]) -> Result<String, String> {
    let v = value.trim().to_ascii_lowercase();
    if allowed.iter().any(|a| *a == v) {
        Ok(v)
    } else {
        Err(format!("\"{}\" is not one of {}", value.trim(), allowed.join(", ")))
    }
}

// ============================================================================
// Links
// ============================================================================

/// Host suffixes a link may not point at. The same list as
/// `WEBHOOK_BLOCKED_SUFFIXES` in the Worker's `src/commerce.mjs`: keep the two
/// in step.
const BLOCKED_SUFFIXES: &[&str] = &[
    "localhost", "local", "internal", "intranet", "lan", "home", "corp", "private",
    "home.arpa", "arpa", "test", "example", "invalid", "onion",
    "eustress.dev",
    "nip.io", "sslip.io", "xip.io", "traefik.me", "localtest.me", "lvh.me", "vcap.me", "lacolhost.com",
];

fn is_dotted_quad(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

/// A link URL the Worker will accept: `https://` only, at most
/// [`MAX_URL_LEN`] characters, no user or password, no port, no IP literal, a
/// public-shaped host name (the rules of `cleanWebhookUrl`). Returns the URL
/// in its normalised form, which is what is baked.
pub fn validate_link_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("the URL is empty".to_string());
    }
    if raw.chars().count() > MAX_URL_LEN {
        return Err(format!("the URL is longer than {MAX_URL_LEN} characters"));
    }
    let url = url::Url::parse(raw).map_err(|_| "it is not a valid URL".to_string())?;
    if url.scheme() != "https" {
        return Err("only https:// links are allowed".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a link may not carry a user name or password".to_string());
    }
    if url.port().is_some() {
        return Err("a link may not name a port; use the default https port".to_string());
    }
    let host = match url.host() {
        Some(url::Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(_) => return Err("an IP address is not allowed; link a host name".to_string()),
        None => return Err("the URL has no host".to_string()),
    };
    let host = host.trim_end_matches('.').to_string();
    if is_dotted_quad(&host) {
        return Err("an IP address is not allowed; link a host name".to_string());
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    if labels.len() < 2 || !labels.iter().all(label_ok) {
        return Err("the host is not a public host name".to_string());
    }
    if BLOCKED_SUFFIXES.iter().any(|s| host == *s || host.ends_with(&format!(".{s}"))) {
        return Err("that host is not a public site a page may link".to_string());
    }
    Ok(url.as_str().to_string())
}

/// Whether text reads as a URL or a bare domain, which a label may not be.
fn looks_like_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return true;
    }
    lower.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        match token.rsplit_once('.') {
            Some((head, tld)) => {
                !head.is_empty()
                    && head.chars().any(|c| c.is_ascii_alphabetic())
                    && tld.len() >= 2
                    && tld.chars().all(|c| c.is_ascii_alphabetic())
            }
            None => false,
        }
    })
}

/// A link label: plain text, 1 to [`MAX_LABEL_CHARS`] characters, no URL, no
/// control characters. Returns it trimmed.
pub fn validate_label(raw: &str) -> Result<String, String> {
    let label = raw.trim();
    if label.is_empty() {
        return Err("the label is empty".to_string());
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(format!("the label is longer than {MAX_LABEL_CHARS} characters"));
    }
    if label.chars().any(|c| c.is_control()) {
        return Err("the label has a control character".to_string());
    }
    if looks_like_url(label) {
        return Err("the label may not be a URL; the link carries the address".to_string());
    }
    Ok(label.to_string())
}

/// A link kind in the spec's spelling.
pub fn validate_link_kind(raw: &str) -> Result<String, String> {
    let names: Vec<&str> = LINK_KINDS.iter().map(|(k, _)| *k).collect();
    check_enum(raw, &names)
}

// ============================================================================
// Images
// ============================================================================

/// Slot name of the logo.
pub const SLOT_LOGO: &str = "logo";
/// Slot name of the banner.
pub const SLOT_BANNER: &str = "banner";

/// Slot name of gallery image `index` (0 to 5).
pub fn gallery_slot(index: usize) -> String {
    format!("gallery-{index}")
}

/// Whether `slot` is a slot the Worker takes: the logo, the banner, a gallery
/// slot or a README slot ([`readme_slot`]).
pub fn is_valid_slot(slot: &str) -> bool {
    if slot == SLOT_LOGO || slot == SLOT_BANNER {
        return true;
    }
    let numbered = |n: &str, cap: usize| n.len() == 1 && n.parse::<usize>().map(|i| i < cap).unwrap_or(false);
    if let Some(n) = slot.strip_prefix("gallery-") {
        return numbered(n, MAX_GALLERY);
    }
    if let Some(n) = slot.strip_prefix("readme-") {
        return numbered(n, README_MAX_IMAGES);
    }
    false
}

/// A Space-relative image path in forward-slash form: no drive, no leading
/// slash, no `..`, an extension of png, jpg, jpeg or webp.
pub fn clean_image_path(raw: &str) -> Result<String, String> {
    let path = raw.trim().replace('\\', "/");
    if path.is_empty() {
        return Err("the path is empty".to_string());
    }
    if path.starts_with('/') || path.contains(':') {
        return Err("the path must be relative to the Space, not absolute".to_string());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("the path has a control character".to_string());
    }
    if path.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
        return Err("the path may not contain empty, . or .. segments".to_string());
    }
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return Err("only PNG, JPEG and WebP images are allowed".to_string());
    }
    Ok(path)
}

/// The content type of image bytes by their leading signature: PNG, JPEG or
/// WebP, and nothing else.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return Some("image/jpeg");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// Check image bytes before upload: PNG, JPEG or WebP by signature, at most
/// [`MAX_IMAGE_BYTES`]. Returns the content type.
pub fn check_image_bytes(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.is_empty() {
        return Err("the file is empty".to_string());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "the file is {:.1} MB; the limit is {} MB",
            bytes.len() as f64 / 1_048_576.0,
            MAX_IMAGE_BYTES / 1_048_576
        ));
    }
    sniff_image(bytes).ok_or_else(|| "the file is not a PNG, JPEG or WebP image".to_string())
}

// ============================================================================
// Building the spec
// ============================================================================

/// The theme fields as the author left them. Strings are raw; empty and
/// [`UNSET`] mean "not chosen".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeInput {
    pub preset: String,
    /// Colour overrides by key ([`COLOR_KEYS`]); an empty value is "no override".
    pub colors: BTreeMap<String, String>,
    pub font: String,
    pub layout: String,
    pub motion: String,
    pub slideshow_interval_s: Option<i64>,
    pub banner_fade_ms: Option<i64>,
}

/// One link as the author left it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkInput {
    /// The folder name in the Website service, for messages.
    pub name: String,
    pub kind: String,
    pub url: String,
    pub label: String,
}

/// Everything the author set for the page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageInput {
    pub theme: ThemeInput,
    /// Space-relative path, or empty.
    pub logo: String,
    pub banner: String,
    /// Space-relative paths; empty entries are skipped.
    pub gallery: Vec<String>,
    pub links: Vec<LinkInput>,
}

/// What [`build_page_spec`] made.
#[derive(Debug, Clone, PartialEq)]
pub struct PageBuild {
    /// The PageSpec v1 document, or `None` when nothing was set: no theme, no
    /// image and no link bakes nothing.
    pub spec: Option<Value>,
    /// `(slot, Space-relative path)` for each image the spec names, in slot order.
    pub images: Vec<(String, String)>,
    /// One line per entry that was dropped, and why.
    pub warnings: Vec<String>,
}

/// Build PageSpec v1 from what the author set, applying every rule and
/// dropping, with a warning, whatever breaks one.
pub fn build_page_spec(input: &PageInput) -> PageBuild {
    let mut warnings: Vec<String> = Vec::new();
    let mut theme_set = false;

    // Preset.
    let mut preset_name: Option<String> = None;
    if let Some(v) = chosen(&input.theme.preset) {
        match check_enum(&v, &preset_names()) {
            Ok(p) => {
                preset_name = Some(p);
                theme_set = true;
            }
            Err(e) => warnings.push(format!("theme preset dropped: {e}")),
        }
    }

    // Colour overrides.
    let mut colors: Map<String, Value> = Map::new();
    for (key, value) in &input.theme.colors {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if !COLOR_KEYS.contains(&key.as_str()) {
            warnings.push(format!("theme colour \"{key}\" dropped: the keys are {}", COLOR_KEYS.join(", ")));
            continue;
        }
        match normalise_hex(value) {
            Some(hex) => {
                colors.insert(key.clone(), Value::String(hex));
                theme_set = true;
            }
            None => warnings.push(format!("theme colour {key} dropped: \"{value}\" is not #RRGGBB")),
        }
    }

    // Enums.
    let mut pick = |label: &str, raw: &str, allowed: &[&str], default: &str, theme_set: &mut bool| -> String {
        match chosen(raw) {
            None => default.to_string(),
            Some(v) => match check_enum(&v, allowed) {
                Ok(ok) => {
                    *theme_set = true;
                    ok
                }
                Err(e) => {
                    warnings.push(format!("theme {label} dropped: {e}"));
                    default.to_string()
                }
            },
        }
    };
    let font = pick("font", &input.theme.font, FONTS, DEFAULT_FONT, &mut theme_set);
    let layout = pick("layout", &input.theme.layout, LAYOUTS, DEFAULT_LAYOUT, &mut theme_set);
    let motion = pick("motion", &input.theme.motion, MOTIONS, DEFAULT_MOTION, &mut theme_set);

    // Timings.
    let mut slideshow = DEFAULT_SLIDESHOW_INTERVAL_S;
    if let Some(v) = input.theme.slideshow_interval_s {
        if (SLIDESHOW_INTERVAL_RANGE.0..=SLIDESHOW_INTERVAL_RANGE.1).contains(&v) {
            slideshow = v;
            if v != DEFAULT_SLIDESHOW_INTERVAL_S {
                theme_set = true;
            }
        } else {
            warnings.push(format!(
                "slideshow_interval_s dropped: {v} is outside {} to {} seconds",
                SLIDESHOW_INTERVAL_RANGE.0, SLIDESHOW_INTERVAL_RANGE.1
            ));
        }
    }
    let mut fade = DEFAULT_BANNER_FADE_MS;
    if let Some(v) = input.theme.banner_fade_ms {
        if (BANNER_FADE_RANGE.0..=BANNER_FADE_RANGE.1).contains(&v) {
            fade = v;
            if v != DEFAULT_BANNER_FADE_MS {
                theme_set = true;
            }
        } else {
            warnings.push(format!(
                "banner_fade_ms dropped: {v} is outside {} to {} milliseconds",
                BANNER_FADE_RANGE.0, BANNER_FADE_RANGE.1
            ));
        }
    }

    // Contrast of the effective text colour, as the Worker will check it.
    let base = preset(preset_name.as_deref().unwrap_or(DEFAULT_PRESET)).unwrap_or(&PRESETS[0]);
    let effective = |key: &str| -> String {
        colors
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| base.color(key).unwrap_or("#000000").to_string())
    };
    for against in ["background", "surface"] {
        if let Some(ratio) = contrast_ratio(&effective("text"), &effective(against)) {
            if ratio < MIN_TEXT_CONTRAST {
                warnings.push(format!(
                    "theme text colour is {ratio:.1}:1 on {against}; {MIN_TEXT_CONTRAST}:1 is needed, so the page will use the preset's text colour"
                ));
            }
        }
    }

    // Images.
    let mut images: Vec<(String, String)> = Vec::new();
    let mut image_map: Map<String, Value> = Map::new();
    for (slot, raw) in [(SLOT_LOGO, &input.logo), (SLOT_BANNER, &input.banner)] {
        if raw.trim().is_empty() {
            continue;
        }
        match clean_image_path(raw) {
            Ok(path) => {
                image_map.insert(slot.to_string(), Value::String(slot.to_string()));
                images.push((slot.to_string(), path));
            }
            Err(e) => warnings.push(format!("{slot} image dropped: {e}")),
        }
    }
    let mut gallery: Vec<Value> = Vec::new();
    for raw in input.gallery.iter().filter(|r| !r.trim().is_empty()) {
        if gallery.len() >= MAX_GALLERY {
            warnings.push(format!("gallery image \"{}\" dropped: the gallery holds at most {MAX_GALLERY} images", raw.trim()));
            continue;
        }
        match clean_image_path(raw) {
            Ok(path) => {
                let slot = gallery_slot(gallery.len());
                gallery.push(Value::String(slot.clone()));
                images.push((slot, path));
            }
            Err(e) => warnings.push(format!("gallery image \"{}\" dropped: {e}", raw.trim())),
        }
    }
    image_map.insert("gallery".to_string(), Value::Array(gallery));

    // Links.
    let mut links: Vec<Value> = Vec::new();
    let mut per_kind: BTreeMap<String, usize> = BTreeMap::new();
    for link in &input.links {
        let name = if link.name.is_empty() { link.label.as_str() } else { link.name.as_str() };
        let kind = match validate_link_kind(&link.kind) {
            Ok(k) => k,
            Err(e) => {
                warnings.push(format!("link {name} dropped: kind {e}"));
                continue;
            }
        };
        let url = match validate_link_url(&link.url) {
            Ok(u) => u,
            Err(e) => {
                warnings.push(format!("link {name} dropped: {e}"));
                continue;
            }
        };
        let label = match validate_label(&link.label) {
            Ok(l) => l,
            Err(e) => {
                warnings.push(format!("link {name} dropped: {e}"));
                continue;
            }
        };
        let cap = LINK_KINDS.iter().find(|(k, _)| *k == kind).map(|(_, c)| *c).unwrap_or(0);
        let used = per_kind.entry(kind.clone()).or_insert(0);
        if *used >= cap || links.len() >= MAX_LINKS {
            warnings.push(format!("link {name} dropped: a page holds at most {cap} {kind} link(s)"));
            continue;
        }
        *used += 1;
        links.push(json!({ "kind": kind, "url": url, "label": label }));
    }

    let anything = theme_set || !images.is_empty() || !links.is_empty();
    if !anything {
        return PageBuild { spec: None, images, warnings };
    }

    let preset_out = preset_name.unwrap_or_else(|| DEFAULT_PRESET.to_string());
    let spec = json!({
        "schema": SCHEMA,
        "theme": {
            "preset": preset_out,
            "colors": Value::Object(colors),
            "font": font,
            "layout": layout,
            "motion": motion,
            "slideshow_interval_s": slideshow,
            "banner_fade_ms": fade,
        },
        "images": Value::Object(image_map),
        "links": links,
    });
    PageBuild { spec: Some(spec), images, warnings }
}

// ============================================================================
// Reading the settings from the Website service's TOML
// ============================================================================

/// The class name of a link child of the Website service.
pub const LINK_CLASS: &str = "PageLink";

/// The text of a TOML value that is a string, or a `{ value = "..." }` table
/// the Properties grammar may have written.
fn text_of(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Table(t) => t.get("value").and_then(text_of),
        _ => None,
    }
}

fn int_of(v: &toml::Value) -> Option<i64> {
    match v {
        toml::Value::Integer(i) => Some(*i),
        toml::Value::Float(f) if f.is_finite() => Some(f.round() as i64),
        toml::Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f.round() as i64),
        _ => None,
    }
}

/// Lower-case alphanumerics only, so `theme_preset` and `ThemePreset` match.
fn squash(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Fill the theme and image fields of `input` from a parsed
/// `Website/_service.toml`. The values are the flat scalars under `[service]`
/// (`theme_preset`, `theme_color_primary`, `slideshow_interval_s`,
/// `image_logo`, `image_gallery_0` ...); a missing one reads as unset.
pub fn read_service_doc(doc: &toml::Value, input: &mut PageInput) {
    let mut flat: BTreeMap<String, &toml::Value> = BTreeMap::new();
    if let Some(table) = doc.get("service").and_then(|v| v.as_table()) {
        for (k, v) in table {
            if !v.is_table() {
                flat.insert(squash(k), v);
            }
        }
    }
    let get_text = |key: &str| flat.get(&squash(key)).and_then(|v| text_of(v)).unwrap_or_default();
    let get_int = |key: &str| flat.get(&squash(key)).and_then(|v| int_of(v));

    input.theme.preset = get_text("theme_preset");
    for key in COLOR_KEYS {
        input.theme.colors.insert(key.to_string(), get_text(&format!("theme_color_{key}")));
    }
    input.theme.font = get_text("theme_font");
    input.theme.layout = get_text("theme_layout");
    input.theme.motion = get_text("theme_motion");
    input.theme.slideshow_interval_s = get_int("slideshow_interval_s");
    input.theme.banner_fade_ms = get_int("banner_fade_ms");
    input.logo = get_text("image_logo");
    input.banner = get_text("image_banner");
    input.gallery = (0..MAX_GALLERY).map(|i| get_text(&format!("image_gallery_{i}"))).collect();
}

/// The link a parsed `_instance.toml` describes, named by its folder, or
/// `None` when the file is not a `PageLink`.
pub fn link_from_instance_doc(name: &str, doc: &toml::Value) -> Option<LinkInput> {
    let class = doc.get("metadata").and_then(|m| m.get("class_name")).and_then(|v| v.as_str()).unwrap_or_default();
    if class != LINK_CLASS {
        return None;
    }
    let attrs = doc.get("attributes");
    let attr = |k: &str| attrs.and_then(|a| a.get(k)).and_then(text_of).unwrap_or_default();
    Some(LinkInput { name: name.to_string(), kind: attr("kind"), url: attr("url"), label: attr("label") })
}

// ============================================================================
// README (the About tab)
// ============================================================================

/// The folder a README sits in and its relative image paths resolve from: the
/// Website service's folder. The same name as `SERVICE_FOLDER` in
/// `engine/src/website/bake.rs` and `tools/src/website_tools.rs`.
pub const README_BASE_FOLDER: &str = "Website";
/// The README's file name inside [`README_BASE_FOLDER`].
pub const README_FILE: &str = "README.md";
/// Largest README the Worker takes, in bytes of UTF-8 (32 KiB). It is checked
/// on the file and again on the text as uploaded, after the image rewrite.
pub const README_MAX_BYTES: usize = 32 * 1024;
/// Most images a README shows (`readme-0` to `readme-7`).
pub const README_MAX_IMAGES: usize = 8;
/// What an uploaded README image's target is rewritten to begin with:
/// `eustress-image:readme-N`. Only Studio writes it. The Worker refuses a
/// README that names any other `eustress-image:` target, so an author's own
/// use of it is escaped ([`rewrite_readme`]).
pub const README_IMAGE_SCHEME: &str = "eustress-image:";

/// Slot name of README image `index` (0 to 7).
pub fn readme_slot(index: usize) -> String {
    format!("readme-{index}")
}

/// The README's Space-relative path, `Website/README.md`.
pub fn readme_rel_path() -> String {
    format!("{README_BASE_FOLDER}/{README_FILE}")
}

/// Check the README file's bytes: at most [`README_MAX_BYTES`], UTF-8, no
/// NUL character (the Worker refuses one). A leading byte order mark, which
/// some Windows editors write, is dropped. Returns the text; an empty or
/// blank text is the caller's to treat as no README.
pub fn check_readme_text(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > README_MAX_BYTES {
        return Err(format!(
            "it is {:.1} KiB; the limit is {} KiB",
            bytes.len() as f64 / 1024.0,
            README_MAX_BYTES / 1024
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_string())?;
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if text.contains('\0') {
        return Err("it holds a NUL character".to_string());
    }
    Ok(text)
}

/// Decode `%XX` escapes. `None` when an escape is broken or the result is not
/// UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hi = *b.get(i + 1)?;
            let lo = *b.get(i + 2)?;
            if !hi.is_ascii_hexdigit() || !lo.is_ascii_hexdigit() {
                return None;
            }
            let hex = [hi, lo];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether `s` begins with a URL scheme: a letter, then letters, digits, `+`,
/// `-` or `.`, then `:`.
fn has_scheme(s: &str) -> bool {
    let Some((head, _)) = s.split_once(':') else {
        return false;
    };
    head.chars().next().map_or(false, |c| c.is_ascii_alphabetic())
        && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The rules a README image target is held to, before and after decoding.
fn readme_target_rules(path: &str) -> Result<(), String> {
    if has_scheme(path) || path.starts_with("//") {
        return Err("a remote image (https:, data: or any other address) is never loaded; use a file inside the Space".to_string());
    }
    if path.starts_with('/') {
        return Err("the path must be relative to the README, not absolute".to_string());
    }
    if path.contains(':') {
        return Err("the path may not contain a colon".to_string());
    }
    if path.contains('\\') {
        return Err("the path may not contain a backslash; use /".to_string());
    }
    if path.contains('?') || path.contains('#') {
        return Err("the path may not carry a query (?) or a fragment (#)".to_string());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("the path has a control character".to_string());
    }
    Ok(())
}

/// Resolve a README image target to a Space-relative path, the way GitHub
/// resolves a relative path in a README: from the folder the README sits in
/// ([`README_BASE_FOLDER`]). `Assets/x.png` is `Website/Assets/x.png`, and
/// `../Assets/x.png` is `Assets/x.png`.
///
/// Refused: an empty target; an absolute path (`/x.png`, `//host/x.png`); a
/// URL scheme (`https:`, `data:`, `javascript:`, `file:`, any `scheme:`) or
/// any other colon; a backslash; a query (`?`) or fragment (`#`); a `..` that
/// climbs above the Space root. `%XX` escapes are decoded
/// (`My%20Diagram.png`) and the decoded path is held to the same rules. The
/// result then passes [`clean_image_path`].
pub fn resolve_readme_image(target: &str) -> Result<String, String> {
    let raw = target.trim();
    if raw.is_empty() {
        return Err("the image has no path".to_string());
    }
    readme_target_rules(raw)?;
    let decoded = percent_decode(raw).ok_or_else(|| "the path has a broken % escape".to_string())?;
    readme_target_rules(&decoded)?;
    let mut parts: Vec<&str> = vec![README_BASE_FOLDER];
    for segment in decoded.split('/') {
        match segment {
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("the path climbs above the Space root".to_string());
                }
            }
            other => parts.push(other),
        }
    }
    clean_image_path(&parts.join("/"))
}

/// What [`rewrite_readme`] made.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadmeBuild {
    /// The README text as uploaded: each image that loaded points at its slot
    /// (`eustress-image:readme-N`), every other image is its alt text.
    pub markdown: String,
    /// `(slot, Space-relative path)` for each image to upload, in slot order.
    pub images: Vec<(String, String)>,
    /// One line per image shown as its alt text, and why.
    pub warnings: Vec<String>,
}

/// Rewrite a README's images for upload.
///
/// `load` answers, for a resolved Space-relative path, whether the file loaded
/// as a valid image (`Err` carries why not). It is asked at most once per path,
/// in order of first appearance, and never once the [`README_MAX_IMAGES`]
/// slots are taken, so a caller may read the file inside it.
///
/// Each image (`![alt](target)`, `![alt](target "title")`,
/// `![alt](<target>)`) whose target resolves ([`resolve_readme_image`]) and
/// loads gets a slot, `readme-0` upward in order of first appearance, and its
/// target becomes `eustress-image:readme-N`; the same file reused keeps its
/// slot. Every other image (remote, missing, not an image, or past the eighth)
/// is replaced by its alt text, with a warning. Image syntax inside a fenced
/// or indented code block or an inline code span is left as written.
/// Reference-style images (`![alt][ref]`) are left as written too; the Worker
/// shows any image that is not a README slot as its alt text.
///
/// Any `eustress-image:` the author wrote is escaped to `eustress-image\:`
/// (which Markdown shows as written outside code), so the only such targets in
/// the result are the ones written here.
///
/// A hand-written scanner over CommonMark's block and inline rules for code
/// and images; it is not a full Markdown parser. The Worker checks the result
/// again and trusts nothing in it.
pub fn rewrite_readme(text: &str, mut load: impl FnMut(&str) -> Result<(), String>) -> ReadmeBuild {
    let mut rewriter = ReadmeRewriter {
        load: &mut load,
        images: Vec::new(),
        slot_of: HashMap::new(),
        failed: HashMap::new(),
        warned: HashSet::new(),
        warnings: Vec::new(),
    };
    let escaped;
    let text = if text.contains(README_IMAGE_SCHEME) {
        rewriter.warnings.push(format!(
            "README.md writes \"{README_IMAGE_SCHEME}\" itself; only Studio writes that, so it is escaped and shown as text"
        ));
        escaped = text.replace(README_IMAGE_SCHEME, "eustress-image\\:");
        escaped.as_str()
    } else {
        text
    };
    let mut markdown = String::with_capacity(text.len());
    for (start, end, inline) in markdown_blocks(text) {
        let part = &text[start..end];
        if inline {
            markdown.push_str(&rewriter.inline(part));
        } else {
            markdown.push_str(part);
        }
    }
    ReadmeBuild { markdown, images: rewriter.images, warnings: rewriter.warnings }
}

struct ReadmeRewriter<'a> {
    load: &'a mut dyn FnMut(&str) -> Result<(), String>,
    images: Vec<(String, String)>,
    slot_of: HashMap<String, String>,
    failed: HashMap<String, String>,
    warned: HashSet<String>,
    warnings: Vec<String>,
}

/// One image found by [`parse_image`], as byte offsets into its text.
struct ImageSpan {
    start: usize,
    /// Just past the closing `)`.
    end: usize,
    /// The alt text, between `![` and `]`.
    alt: (usize, usize),
    /// The destination as written, angle brackets included.
    dest: (usize, usize),
    /// The destination without angle brackets.
    target: (usize, usize),
}

impl ReadmeRewriter<'_> {
    /// Rewrite every image in a run of ordinary Markdown text.
    fn inline(&mut self, seg: &str) -> String {
        let b = seg.as_bytes();
        let mut out = String::with_capacity(seg.len());
        let (mut i, mut copied) = (0usize, 0usize);
        while i < b.len() {
            match b[i] {
                b'\\' => i += escape_len(b, i),
                b'`' => i = skip_code_span(b, i),
                b'!' if b.get(i + 1) == Some(&b'[') => match parse_image(b, i) {
                    Some(image) => {
                        out.push_str(&seg[copied..image.start]);
                        let replaced = self.replace(seg, &image);
                        out.push_str(&replaced);
                        i = image.end;
                        copied = i;
                    }
                    None => i += 1,
                },
                _ => i += 1,
            }
        }
        out.push_str(&seg[copied..]);
        out
    }

    /// The image pointed at its slot, or its alt text.
    fn replace(&mut self, seg: &str, image: &ImageSpan) -> String {
        let target = &seg[image.target.0..image.target.1];
        let outcome = resolve_readme_image(target).and_then(|path| self.slot_for(&path));
        match outcome {
            Ok(slot) => format!(
                "{}{README_IMAGE_SCHEME}{slot}{}",
                &seg[image.start..image.dest.0],
                &seg[image.dest.1..image.end]
            ),
            Err(reason) => {
                if self.warned.insert(target.to_string()) {
                    self.warnings.push(format!(
                        "README.md image \"{}\" is shown as its alt text: {reason}",
                        shown_target(target)
                    ));
                }
                // The alt text may itself hold an image (`![![a](b)](c)`).
                self.inline(&seg[image.alt.0..image.alt.1])
            }
        }
    }

    /// The slot for a resolved path: the one it already has, or the next free
    /// one when it loads.
    fn slot_for(&mut self, path: &str) -> Result<String, String> {
        if let Some(slot) = self.slot_of.get(path) {
            return Ok(slot.clone());
        }
        if let Some(reason) = self.failed.get(path) {
            return Err(reason.clone());
        }
        if self.images.len() >= README_MAX_IMAGES {
            return Err(format!("a README shows at most {README_MAX_IMAGES} images"));
        }
        match (self.load)(path) {
            Ok(()) => {
                let slot = readme_slot(self.images.len());
                self.images.push((slot.clone(), path.to_string()));
                self.slot_of.insert(path.to_string(), slot.clone());
                Ok(slot)
            }
            Err(reason) => {
                self.failed.insert(path.to_string(), reason.clone());
                Err(reason)
            }
        }
    }
}

/// A target as a warning shows it: at most 60 characters, so a `data:` URL
/// does not fill the publish log.
fn shown_target(target: &str) -> String {
    const MAX: usize = 60;
    if target.chars().count() <= MAX {
        target.to_string()
    } else {
        format!("{}...", target.chars().take(MAX).collect::<String>())
    }
}

/// Bytes a backslash at `i` covers: two when it escapes ASCII punctuation,
/// else one (a literal backslash).
fn escape_len(b: &[u8], i: usize) -> usize {
    if b.get(i + 1).map_or(false, |c| c.is_ascii_punctuation()) {
        2
    } else {
        1
    }
}

fn run_len(b: &[u8], i: usize, of: u8) -> usize {
    b[i..].iter().take_while(|&&c| c == of).count()
}

/// Past the code span opening at the backtick run at `i`: a run of N
/// backticks closes at the next run of exactly N. With no closer the run is
/// literal and only it is skipped.
fn skip_code_span(b: &[u8], i: usize) -> usize {
    let n = run_len(b, i, b'`');
    let mut j = i + n;
    while j < b.len() {
        if b[j] == b'`' {
            let m = run_len(b, j, b'`');
            if m == n {
                return j + m;
            }
            j += m;
        } else {
            j += 1;
        }
    }
    i + n
}

/// Past spaces and tabs, with at most one line ending among them.
fn skip_space(b: &[u8], mut j: usize) -> usize {
    let mut line_ending = false;
    while let Some(&c) = b.get(j) {
        match c {
            b' ' | b'\t' => j += 1,
            b'\r' | b'\n' if !line_ending => {
                line_ending = true;
                j += if c == b'\r' && b.get(j + 1) == Some(&b'\n') { 2 } else { 1 };
            }
            _ => break,
        }
    }
    j
}

/// The inline image starting at `start` (which holds `![`), or `None` when
/// the text there is not an inline image.
fn parse_image(b: &[u8], start: usize) -> Option<ImageSpan> {
    // The alt text runs to the matching `]`: brackets nest, and escapes and
    // code spans are skipped.
    let mut j = start + 2;
    let mut depth = 1usize;
    loop {
        match *b.get(j)? {
            b'\\' => j += escape_len(b, j),
            b'`' => j = skip_code_span(b, j),
            b'[' => {
                depth += 1;
                j += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                j += 1;
            }
            _ => j += 1,
        }
    }
    let alt = (start + 2, j);
    j += 1;
    if b.get(j) != Some(&b'(') {
        return None;
    }
    j = skip_space(b, j + 1);

    // The destination: `<...>` on one line, or a run with balanced
    // parentheses up to white space.
    let dest: (usize, usize);
    let target: (usize, usize);
    if b.get(j) == Some(&b'<') {
        let mut k = j + 1;
        loop {
            match *b.get(k)? {
                b'\\' => k += escape_len(b, k),
                b'>' => break,
                b'<' | b'\n' | b'\r' => return None,
                _ => k += 1,
            }
        }
        target = (j + 1, k);
        dest = (j, k + 1);
        j = k + 1;
    } else {
        let mut k = j;
        let mut parens = 0usize;
        while let Some(&c) = b.get(k) {
            match c {
                b'\\' => {
                    k += escape_len(b, k);
                    continue;
                }
                b'(' => parens += 1,
                b')' if parens == 0 => break,
                b')' => parens -= 1,
                c if c <= b' ' || c == 0x7F => break,
                _ => {}
            }
            k += 1;
        }
        if parens != 0 {
            return None;
        }
        target = (j, k);
        dest = (j, k);
        j = k;
    }

    // An optional title, after white space: "...", '...' or (...).
    let after = skip_space(b, j);
    if after > j {
        if let Some(&quote) = b.get(after) {
            if matches!(quote, b'"' | b'\'' | b'(') {
                let close = if quote == b'(' { b')' } else { quote };
                let mut k = after + 1;
                loop {
                    match *b.get(k)? {
                        b'\\' => k += escape_len(b, k),
                        c if c == close => break,
                        b'(' if quote == b'(' => return None,
                        _ => k += 1,
                    }
                }
                j = skip_space(b, k + 1);
            } else {
                j = after;
            }
        }
    }
    if b.get(j) != Some(&b')') {
        return None;
    }
    Some(ImageSpan { start, end: j + 1, alt, dest, target })
}

/// Leading indentation of a line in columns (a tab runs to the next multiple
/// of four), and the rest of the line.
fn indentation(line: &str) -> (usize, &str) {
    let mut cols = 0usize;
    for (i, c) in line.char_indices() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4 - cols % 4,
            _ => return (cols, &line[i..]),
        }
    }
    (cols, "")
}

/// A line with its block quote markers (`> `) removed.
fn strip_quote_markers(mut line: &str) -> &str {
    loop {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3 && trimmed.starts_with('>') {
            line = &trimmed[1..];
            line = line.strip_prefix(' ').unwrap_or(line);
        } else {
            return line;
        }
    }
}

/// The fence character and run length when `rest` opens a fenced code block.
fn opens_fence(rest: &str) -> Option<(u8, usize)> {
    let ch = *rest.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let n = run_len(rest.as_bytes(), 0, ch);
    if n < 3 || (ch == b'`' && rest[n..].contains('`')) {
        return None;
    }
    Some((ch, n))
}

/// Whether `rest` closes a fence of `n` `ch` characters.
fn closes_fence(rest: &str, ch: u8, n: usize) -> bool {
    let m = if rest.is_empty() { 0 } else { run_len(rest.as_bytes(), 0, ch) };
    m >= n && rest[m..].trim().is_empty()
}

/// The width of a list item marker and the spaces after it, when `rest`
/// begins with one (`-`, `*`, `+`, or one to nine digits and `.` or `)`).
fn list_marker(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let marker = match *b.first()? {
        b'-' | b'*' | b'+' => 1,
        c if c.is_ascii_digit() => {
            let digits = run_while(b, |c| c.is_ascii_digit());
            if digits > 9 || !matches!(b.get(digits), Some(b'.') | Some(b')')) {
                return None;
            }
            digits + 1
        }
        _ => return None,
    };
    match b.get(marker) {
        None => Some(marker + 1),
        Some(b' ') | Some(b'\t') => {
            let (spaces, after) = indentation(&rest[marker..]);
            if after.is_empty() || spaces > 4 {
                Some(marker + 1)
            } else {
                Some(marker + spaces)
            }
        }
        _ => None,
    }
}

fn run_while(b: &[u8], f: impl Fn(u8) -> bool) -> usize {
    b.iter().take_while(|&&c| f(c)).count()
}

/// Whether `rest` is an ATX heading (`#` to `######` then a space or nothing).
fn is_atx_heading(rest: &str) -> bool {
    let h = run_len(rest.as_bytes(), 0, b'#');
    (1..=6).contains(&h) && rest[h..].chars().next().map_or(true, |c| c == ' ' || c == '\t')
}

/// Byte ranges of `text`, in order, each `true` when it is ordinary Markdown
/// (paragraphs, headings, lists, quotes) that the image rewrite scans, and
/// `false` when it is blank lines or a fenced or indented code block, which
/// it leaves exactly as written. Follows CommonMark's rules for code blocks,
/// with list items tracked by their content column so a paragraph indented
/// under a list item is not taken for code.
fn markdown_blocks(text: &str) -> Vec<(usize, usize, bool)> {
    let mut out: Vec<(usize, usize, bool)> = Vec::new();
    // (fence character, run length, the indentation it opened under)
    let mut fence: Option<(u8, usize, usize)> = None;
    let mut list_cols: Vec<usize> = Vec::new();
    let mut para_open = false;
    let mut prev_blank = true;
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let content = line.trim_end_matches(|c: char| c == '\n' || c == '\r');
        let (indent, rest) = indentation(strip_quote_markers(content));
        let inline = if let Some((ch, n, base)) = fence {
            if indent <= base + 3 && closes_fence(rest, ch, n) {
                fence = None;
            }
            para_open = false;
            prev_blank = false;
            false
        } else if rest.trim().is_empty() {
            para_open = false;
            prev_blank = true;
            false
        } else {
            let marker = list_marker(rest);
            if prev_blank && marker.is_none() {
                list_cols.retain(|c| *c <= indent);
            }
            let base = list_cols.last().copied().unwrap_or(0);
            let opened = if indent <= base + 3 { opens_fence(rest) } else { None };
            let inline = if !para_open && indent >= base + 4 {
                false
            } else if let Some((ch, n)) = opened {
                fence = Some((ch, n, base));
                false
            } else {
                if let Some(width) = marker {
                    list_cols.retain(|c| *c <= indent);
                    list_cols.push(indent + width);
                }
                true
            };
            para_open = inline && !is_atx_heading(rest);
            prev_blank = false;
            inline
        };
        match out.last_mut() {
            Some(last) if last.2 == inline => last.1 = at,
            _ => out.push((start, at, inline)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_doc_reads_flat_scalars_in_either_spelling() {
        let doc: toml::Value = r##"[service]
theme_preset = "paper"
ThemeColorText = "#101010"
slideshow_interval_s = 9
banner_fade_ms = 250.0
image_gallery_2 = "a/b.png"
"##
            .parse()
            .unwrap();
        let mut input = PageInput::default();
        read_service_doc(&doc, &mut input);
        assert_eq!(input.theme.preset, "paper");
        assert_eq!(input.theme.colors.get("text").map(String::as_str), Some("#101010"));
        assert_eq!(input.theme.slideshow_interval_s, Some(9));
        assert_eq!(input.theme.banner_fade_ms, Some(250));
        assert_eq!(input.gallery.len(), MAX_GALLERY);
        assert_eq!(input.gallery[2], "a/b.png");
        assert_eq!(input.logo, "");
    }

    #[test]
    fn only_a_pagelink_file_is_a_link() {
        let link: toml::Value =
            r##"[metadata]
class_name = "PageLink"
[attributes]
kind = "shop"
url = "https://x.com"
label = "Buy"
"##.parse().unwrap();
        let reference: toml::Value = r##"[metadata]
class_name = "Reference"
[attributes]
kind = "count"
"##.parse().unwrap();
        let got = link_from_instance_doc("kit", &link).expect("a PageLink");
        assert_eq!((got.kind.as_str(), got.url.as_str(), got.label.as_str(), got.name.as_str()), ("shop", "https://x.com", "Buy", "kit"));
        assert!(link_from_instance_doc("ref", &reference).is_none());
    }

    fn ratio(a: &str, b: &str) -> f64 {
        contrast_ratio(a, b).expect("both are #RRGGBB")
    }

    #[test]
    fn contrast_matches_the_wcag_reference_values() {
        assert!((ratio("#000000", "#FFFFFF") - 21.0).abs() < 1e-9);
        assert!((ratio("#FFFFFF", "#FFFFFF") - 1.0).abs() < 1e-9);
        // #777777 on white is the textbook 4.48:1: just under AA.
        let r = ratio("#777777", "#FFFFFF");
        assert!(r > 4.4 && r < 4.5, "{r}");
    }

    #[test]
    fn every_preset_text_reaches_aa_on_its_background_and_surface() {
        assert_eq!(PRESETS.len(), 6);
        for p in PRESETS.iter() {
            for key in COLOR_KEYS {
                assert!(parse_hex(p.color(key).unwrap()).is_some(), "{} {key} is not #RRGGBB", p.name);
            }
            assert!(ratio(p.text, p.background) >= MIN_TEXT_CONTRAST, "{} text on background", p.name);
            assert!(ratio(p.text, p.surface) >= MIN_TEXT_CONTRAST, "{} text on surface", p.name);
            // The brand colours are used for links and buttons on the same
            // grounds, so they read too.
            assert!(ratio(p.primary, p.background) >= MIN_TEXT_CONTRAST, "{} primary on background", p.name);
            assert!(ratio(p.accent, p.background) >= MIN_TEXT_CONTRAST, "{} accent on background", p.name);
        }
    }

    #[test]
    fn the_preset_names_are_the_spec_six() {
        assert_eq!(preset_names(), vec!["studio", "daylight", "blueprint", "laboratory", "arcade", "paper"]);
        assert_eq!(preset("studio").unwrap().primary, "#4F8CFF");
        assert!(preset("neon").is_none());
    }

    #[test]
    fn hex_is_exactly_rrggbb() {
        assert_eq!(normalise_hex("#4f8cff").as_deref(), Some("#4F8CFF"));
        assert_eq!(normalise_hex(" #4F8CFF ").as_deref(), Some("#4F8CFF"));
        for bad in ["4F8CFF", "#4F8", "#4F8CFFAA", "#GGGGGG", "", "red", "#4F8CF"] {
            assert!(parse_hex(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn link_urls_follow_the_host_rules() {
        assert_eq!(validate_link_url("https://example-robotics.com").unwrap(), "https://example-robotics.com/");
        assert!(validate_link_url("https://youtube.com/@example").is_ok());
        assert!(validate_link_url("https://shop.example-robotics.com/kits?x=1").is_ok());
        for bad in [
            "",
            "http://example-robotics.com",
            "ftp://example-robotics.com",
            "javascript:alert(1)",
            "https://user:pw@example-robotics.com",
            "https://user@example-robotics.com",
            "https://example-robotics.com:8443/",
            "https://127.0.0.1/",
            "https://2130706433/",
            "https://0x7f.1/",
            "https://[::1]/",
            "https://10.0.0.5/x",
            "https://localhost/",
            "https://printer.local/",
            "https://intranet/",
            "https://api.eustress.dev/",
            "https://a.nip.io/",
            "not a url",
        ] {
            assert!(validate_link_url(bad).is_err(), "{bad} must be refused");
        }
        let long = format!("https://example-robotics.com/{}", "a".repeat(500));
        assert!(validate_link_url(&long).is_err());
    }

    #[test]
    fn labels_are_short_plain_text() {
        assert_eq!(validate_label("  Our lab ").unwrap(), "Our lab");
        assert!(validate_label("Dr. Smith's lab v2.0").is_ok());
        assert!(validate_label("e.g. docs").is_ok());
        for bad in ["", "   ", "https://example.com", "visit example.com", "www.example.com", "bad\u{7}label"] {
            assert!(validate_label(bad).is_err(), "{bad:?}");
        }
        assert!(validate_label(&"x".repeat(41)).is_err());
        assert!(validate_label(&"x".repeat(40)).is_ok());
    }

    #[test]
    fn image_paths_stay_inside_the_space() {
        assert_eq!(clean_image_path("Assets\\logo.PNG").unwrap(), "Assets/logo.PNG");
        for bad in ["", "/etc/x.png", "C:/x.png", "../x.png", "a/../x.png", "a//x.png", "logo.gif", "logo", "logo.svg"] {
            assert!(clean_image_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn image_bytes_are_sniffed_and_capped() {
        let png = [&[0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[0u8; 8][..]].concat();
        let jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0];
        let webp = [&b"RIFF"[..], &[0u8; 4][..], &b"WEBP"[..], &[0u8; 4][..]].concat();
        assert_eq!(check_image_bytes(&png), Ok("image/png"));
        assert_eq!(check_image_bytes(&jpg), Ok("image/jpeg"));
        assert_eq!(check_image_bytes(&webp), Ok("image/webp"));
        assert!(check_image_bytes(b"GIF89a....").is_err());
        assert!(check_image_bytes(&[]).is_err());
        let mut big = png.clone();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        assert!(check_image_bytes(&big).is_err());
        big.truncate(MAX_IMAGE_BYTES);
        assert_eq!(check_image_bytes(&big), Ok("image/png"));
    }

    #[test]
    fn slots_are_the_spec_names() {
        for ok in ["logo", "banner", "gallery-0", "gallery-5"] {
            assert!(is_valid_slot(ok), "{ok}");
        }
        for bad in ["gallery-6", "gallery-", "gallery-10", "Logo", "icon", "", "readme-8", "readme-", "readme-10"] {
            assert!(!is_valid_slot(bad), "{bad}");
        }
        for i in 0..README_MAX_IMAGES {
            assert!(is_valid_slot(&readme_slot(i)), "readme-{i}");
        }
    }

    fn link(name: &str, kind: &str, url: &str, label: &str) -> LinkInput {
        LinkInput { name: name.into(), kind: kind.into(), url: url.into(), label: label.into() }
    }

    #[test]
    fn nothing_set_bakes_nothing() {
        let build = build_page_spec(&PageInput::default());
        assert!(build.spec.is_none());
        assert!(build.warnings.is_empty());
        // The scaffolded defaults are "nothing set" too.
        let mut input = PageInput::default();
        input.theme.preset = UNSET.into();
        input.theme.font = UNSET.into();
        input.theme.layout = UNSET.into();
        input.theme.motion = UNSET.into();
        input.theme.slideshow_interval_s = Some(DEFAULT_SLIDESHOW_INTERVAL_S);
        input.theme.banner_fade_ms = Some(DEFAULT_BANNER_FADE_MS);
        assert!(build_page_spec(&input).spec.is_none());
    }

    #[test]
    fn a_full_page_bakes_the_contract_shape() {
        let mut input = PageInput::default();
        input.theme.preset = "Blueprint".into();
        input.theme.colors.insert("primary".into(), "#4f8cff".into());
        input.theme.colors.insert("accent".into(), String::new());
        input.theme.font = "space-grotesk".into();
        input.theme.layout = "wide".into();
        input.theme.motion = "full".into();
        input.theme.slideshow_interval_s = Some(10);
        input.theme.banner_fade_ms = Some(0);
        input.logo = "Assets/logo.png".into();
        input.gallery = vec!["Assets/a.png".into(), String::new(), "Assets/b.webp".into()];
        input.links = vec![
            link("lab", "website", "https://example-robotics.com", "Our lab"),
            link("yt", "social", "https://youtube.com/@example", "YouTube"),
            link("kit", "shop", "https://shop.example-robotics.com/kits", "Buy the kit"),
        ];
        let build = build_page_spec(&input);
        assert!(build.warnings.is_empty(), "{:?}", build.warnings);
        let spec = build.spec.expect("a page was set");
        assert_eq!(spec["schema"], 1);
        assert_eq!(spec["theme"]["preset"], "blueprint");
        assert_eq!(spec["theme"]["colors"], json!({ "primary": "#4F8CFF" }));
        assert_eq!(spec["theme"]["font"], "space-grotesk");
        assert_eq!(spec["theme"]["layout"], "wide");
        assert_eq!(spec["theme"]["motion"], "full");
        assert_eq!(spec["theme"]["slideshow_interval_s"], 10);
        assert_eq!(spec["theme"]["banner_fade_ms"], 0);
        assert_eq!(spec["images"]["logo"], "logo");
        assert!(spec["images"].get("banner").is_none());
        assert_eq!(spec["images"]["gallery"], json!(["gallery-0", "gallery-1"]));
        assert_eq!(spec["links"].as_array().unwrap().len(), 3);
        assert_eq!(spec["links"][0], json!({ "kind": "website", "url": "https://example-robotics.com/", "label": "Our lab" }));
        assert_eq!(
            build.images,
            vec![
                ("logo".to_string(), "Assets/logo.png".to_string()),
                ("gallery-0".to_string(), "Assets/a.png".to_string()),
                ("gallery-1".to_string(), "Assets/b.webp".to_string()),
            ]
        );
    }

    #[test]
    fn links_alone_bake_a_default_theme() {
        let mut input = PageInput::default();
        input.links = vec![link("lab", "website", "https://example-robotics.com", "Our lab")];
        let spec = build_page_spec(&input).spec.expect("a link is something");
        assert_eq!(spec["theme"]["preset"], DEFAULT_PRESET);
        assert_eq!(spec["theme"]["font"], DEFAULT_FONT);
        assert_eq!(spec["theme"]["layout"], DEFAULT_LAYOUT);
        assert_eq!(spec["theme"]["motion"], DEFAULT_MOTION);
        assert_eq!(spec["theme"]["colors"], json!({}));
        assert_eq!(spec["images"]["gallery"], json!([]));
    }

    #[test]
    fn invalid_entries_are_dropped_with_a_warning_and_the_rest_publishes() {
        let mut input = PageInput::default();
        input.theme.preset = "neon".into();
        input.theme.colors.insert("primary".into(), "blue".into());
        input.theme.colors.insert("sparkle".into(), "#FFFFFF".into());
        input.theme.font = "comic-sans".into();
        input.theme.slideshow_interval_s = Some(99);
        input.theme.banner_fade_ms = Some(-1);
        input.logo = "../secret.png".into();
        input.links = vec![
            link("ok", "social", "https://youtube.com/@example", "YouTube"),
            link("http", "social", "http://youtube.com/@example", "YouTube"),
            link("kind", "blog", "https://example-robotics.com", "Blog"),
            link("label", "other", "https://example-robotics.com", "https://example-robotics.com"),
        ];
        let build = build_page_spec(&input);
        let joined = build.warnings.join("\n");
        for needle in ["preset", "primary", "sparkle", "font", "slideshow_interval_s", "banner_fade_ms", "logo", "link http", "link kind", "link label"] {
            assert!(joined.contains(needle), "no warning mentions {needle}:\n{joined}");
        }
        let spec = build.spec.expect("the valid link still publishes");
        assert_eq!(spec["links"].as_array().unwrap().len(), 1);
        assert_eq!(spec["links"][0]["label"], "YouTube");
        // Nothing invalid reached the spec.
        assert_eq!(spec["theme"]["preset"], DEFAULT_PRESET);
        assert_eq!(spec["theme"]["colors"], json!({}));
        assert_eq!(spec["theme"]["slideshow_interval_s"], DEFAULT_SLIDESHOW_INTERVAL_S);
        assert!(build.images.is_empty());
    }

    #[test]
    fn the_per_kind_and_gallery_caps_hold() {
        let mut input = PageInput::default();
        for i in 0..10 {
            input.links.push(link(&format!("s{i}"), "social", &format!("https://site{i}.com/p"), "Profile"));
        }
        input.links.push(link("w1", "website", "https://one.com", "One"));
        input.links.push(link("w2", "website", "https://two.com", "Two"));
        for i in 0..5 {
            input.links.push(link(&format!("shop{i}"), "shop", &format!("https://shop{i}.com"), "Shop"));
        }
        for i in 0..5 {
            input.links.push(link(&format!("o{i}"), "other", &format!("https://docs{i}.com"), "Docs"));
        }
        input.gallery = (0..8).map(|i| format!("g/{i}.png")).collect();
        let build = build_page_spec(&input);
        let links = build.spec.as_ref().unwrap()["links"].as_array().unwrap().clone();
        let count = |k: &str| links.iter().filter(|l| l["kind"] == k).count();
        assert_eq!(count("social"), 8);
        assert_eq!(count("website"), 1);
        assert_eq!(count("shop"), 4);
        assert_eq!(count("other"), 4);
        assert_eq!(links.len(), 17);
        assert_eq!(build.spec.as_ref().unwrap()["images"]["gallery"].as_array().unwrap().len(), MAX_GALLERY);
        assert_eq!(build.images.len(), MAX_GALLERY);
        assert!(build.warnings.iter().any(|w| w.contains("gallery image")));
    }

    #[test]
    fn a_text_colour_that_fails_contrast_is_warned_about() {
        let mut input = PageInput::default();
        input.theme.preset = "studio".into();
        input.theme.colors.insert("text".into(), "#202020".into());
        let build = build_page_spec(&input);
        assert!(build.warnings.iter().any(|w| w.contains("text colour")), "{:?}", build.warnings);
    }

    // ------------------------------------------------------------------------
    // README
    // ------------------------------------------------------------------------

    #[test]
    fn readme_images_resolve_from_the_website_folder_like_github() {
        assert_eq!(resolve_readme_image("Assets/x.png").unwrap(), "Website/Assets/x.png");
        assert_eq!(resolve_readme_image("./Assets/x.png").unwrap(), "Website/Assets/x.png");
        assert_eq!(resolve_readme_image("../Assets/x.png").unwrap(), "Assets/x.png");
        assert_eq!(resolve_readme_image("Assets/../../Assets/x.png").unwrap(), "Assets/x.png");
        assert_eq!(resolve_readme_image(" diagram.webp ").unwrap(), "Website/diagram.webp");
        assert_eq!(resolve_readme_image("My%20Diagram.png").unwrap(), "Website/My Diagram.png");
    }

    #[test]
    fn readme_image_targets_that_leave_the_space_are_refused() {
        for bad in [
            // empty
            "",
            "   ",
            // absolute
            "/Assets/x.png",
            "//cdn.example.com/x.png",
            // a URL scheme, any scheme
            "http://example.com/x.png",
            "https://example.com/x.png",
            "HTTPS://example.com/x.png",
            "data:image/png;base64,iVBORw0KGgo=",
            "javascript:alert(1)",
            "file:///C:/x.png",
            "eustress-image:readme-0",
            "custom+scheme:x.png",
            "C:/x.png",
            // a colon anywhere
            "Assets/a:b.png",
            // a backslash, written or escaped
            "Assets\\x.png",
            "Assets%5Cx.png",
            // a query or a fragment, written or escaped
            "x.png?v=2",
            "x.png#top",
            "x%3Fv=2.png",
            // .. above the Space root
            "../../x.png",
            "../../../etc/x.png",
            "Assets/../../../x.png",
            "%2E%2E/%2E%2E/x.png",
            // a broken escape
            "x%2.png",
            "x%zz.png",
            // not an image clean_image_path takes
            "Assets/x.gif",
            "Assets//x.png",
        ] {
            assert!(resolve_readme_image(bad).is_err(), "{bad:?} must be refused");
        }
        // Each refusal says why.
        assert!(resolve_readme_image("https://example.com/x.png").unwrap_err().contains("remote"));
        assert!(resolve_readme_image("../../x.png").unwrap_err().contains("Space root"));
        assert!(resolve_readme_image("/x.png").unwrap_err().contains("absolute"));
        assert!(resolve_readme_image("a\\b.png").unwrap_err().contains("backslash"));
        assert!(resolve_readme_image("x.png?v=1").unwrap_err().contains("query"));
    }

    /// A lookup over the files that "loaded", recording each path asked.
    fn rewrite(text: &str, loads: &[&str]) -> (ReadmeBuild, Vec<String>) {
        let mut asked: Vec<String> = Vec::new();
        let build = rewrite_readme(text, |path| {
            asked.push(path.to_string());
            if loads.contains(&path) {
                Ok(())
            } else {
                Err("there is no such file in the Space".to_string())
            }
        });
        (build, asked)
    }

    #[test]
    fn a_readme_image_that_loads_points_at_its_slot_in_every_form() {
        let text = "# Lab\n\n![Cell](Assets/cell.png) and ![Plot](<Assets/plot.png> \"The plot\") and ![Cell again](./Assets/cell.png 'same')\n";
        let (build, asked) = rewrite(text, &["Website/Assets/cell.png", "Website/Assets/plot.png"]);
        assert!(build.warnings.is_empty(), "{:?}", build.warnings);
        assert_eq!(
            build.markdown,
            "# Lab\n\n![Cell](eustress-image:readme-0) and ![Plot](eustress-image:readme-1 \"The plot\") and ![Cell again](eustress-image:readme-0 'same')\n"
        );
        assert_eq!(
            build.images,
            vec![
                ("readme-0".to_string(), "Website/Assets/cell.png".to_string()),
                ("readme-1".to_string(), "Website/Assets/plot.png".to_string()),
            ]
        );
        assert_eq!(asked, vec!["Website/Assets/cell.png", "Website/Assets/plot.png"], "each file is loaded once");
    }

    #[test]
    fn the_same_file_twice_gets_one_slot() {
        let (build, asked) = rewrite("![a](x.png) ![b](x.png) ![c](../Website/x.png)", &["Website/x.png"]);
        assert_eq!(build.markdown, "![a](eustress-image:readme-0) ![b](eustress-image:readme-0) ![c](eustress-image:readme-0)");
        assert_eq!(build.images.len(), 1);
        assert_eq!(asked.len(), 1);
    }

    #[test]
    fn the_ninth_image_becomes_its_alt_text() {
        let paths: Vec<String> = (0..9).map(|i| format!("Website/i{i}.png")).collect();
        let loads: Vec<&str> = paths.iter().map(String::as_str).collect();
        let text: String = (0..9).map(|i| format!("![Image {i}](i{i}.png)\n")).collect();
        let (build, asked) = rewrite(&text, &loads);
        assert_eq!(build.images.len(), README_MAX_IMAGES);
        assert_eq!(build.images[7], ("readme-7".to_string(), "Website/i7.png".to_string()));
        assert!(build.markdown.contains("![Image 7](eustress-image:readme-7)"));
        assert!(build.markdown.ends_with("\nImage 8\n"), "{}", build.markdown);
        assert_eq!(asked.len(), README_MAX_IMAGES, "the ninth file is never read");
        assert_eq!(build.warnings.len(), 1, "{:?}", build.warnings);
        assert!(build.warnings[0].contains("i8.png") && build.warnings[0].contains("at most 8"));
        // A ninth reference to a file that already has a slot still shows.
        let (again, _) = rewrite(&format!("{text}![Again](i0.png)"), &loads);
        assert!(again.markdown.ends_with("![Again](eustress-image:readme-0)"));
    }

    #[test]
    fn a_remote_or_missing_image_becomes_its_alt_text_with_a_warning() {
        let text = "Pixel: ![tracker](https://evil.example/p.png) data: ![d](data:image/png;base64,AAAA) gone: ![Missing](Assets/gone.png) kept: ![Ok](ok.png)";
        let (build, asked) = rewrite(text, &["Website/ok.png"]);
        assert_eq!(build.markdown, "Pixel: tracker data: d gone: Missing kept: ![Ok](eustress-image:readme-0)");
        assert_eq!(asked, vec!["Website/Assets/gone.png", "Website/ok.png"], "a remote image is never looked up");
        assert_eq!(build.warnings.len(), 3, "{:?}", build.warnings);
        assert!(build.warnings[0].contains("https://evil.example/p.png") && build.warnings[0].contains("remote"));
        assert!(build.warnings[2].contains("Assets/gone.png") && build.warnings[2].contains("no such file"));
        assert_eq!(build.images, vec![("readme-0".to_string(), "Website/ok.png".to_string())]);
    }

    #[test]
    fn an_image_inside_a_link_becomes_link_text() {
        let (build, _) = rewrite("[![build](https://ci.example/badge.svg)](https://ci.example/run)", &[]);
        assert_eq!(build.markdown, "[build](https://ci.example/run)");
    }

    #[test]
    fn image_syntax_inside_code_is_left_as_written() {
        let text = "\
Intro ![a](a.png)

```markdown
![fenced](a.png)
```

~~~
![tilde](https://evil.example/p.png)
~~~

    ![indented](a.png)

Inline `![span](a.png)` and ``![double `tick`](a.png)`` here.

- item

      ![code in a list item](a.png)

- item two

  ![para in a list item](a.png)
";
        let (build, asked) = rewrite(text, &["Website/a.png"]);
        let expected = text
            .replace("Intro ![a](a.png)", "Intro ![a](eustress-image:readme-0)")
            .replace("![para in a list item](a.png)", "![para in a list item](eustress-image:readme-0)");
        assert_eq!(build.markdown, expected);
        assert!(build.warnings.is_empty(), "{:?}", build.warnings);
        assert_eq!(asked, vec!["Website/a.png"]);
    }

    #[test]
    fn a_paragraph_continuation_is_not_an_indented_code_block() {
        let (build, _) = rewrite("A paragraph\n    ![still text](a.png)\n", &["Website/a.png"]);
        assert_eq!(build.markdown, "A paragraph\n    ![still text](eustress-image:readme-0)\n");
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end() {
        let text = "```\n![a](a.png)\n\nstill code ![b](a.png)\n";
        let (build, asked) = rewrite(text, &["Website/a.png"]);
        assert_eq!(build.markdown, text);
        assert!(asked.is_empty());
    }

    #[test]
    fn an_escaped_bang_and_a_reference_image_are_left_alone() {
        let text = "\\![not](a.png) ![ref][logo]\n\n[logo]: a.png\n";
        let (build, asked) = rewrite(text, &["Website/a.png"]);
        assert_eq!(build.markdown, text);
        assert!(asked.is_empty());
    }

    #[test]
    fn an_authors_own_eustress_image_target_is_escaped() {
        let (build, asked) = rewrite("![x](eustress-image:readme-3) and `eustress-image:readme-9`", &[]);
        assert!(!build.markdown.contains(README_IMAGE_SCHEME), "{}", build.markdown);
        assert_eq!(build.markdown, "x and `eustress-image\\:readme-9`");
        assert!(asked.is_empty());
        assert!(build.images.is_empty());
        assert!(build.warnings.iter().any(|w| w.contains("only Studio")), "{:?}", build.warnings);
    }

    #[test]
    fn readme_text_is_capped_utf8_and_has_no_nul() {
        assert_eq!(check_readme_text(b"# Hi\n"), Ok("# Hi\n"));
        assert_eq!(check_readme_text("\u{FEFF}# Hi".as_bytes()), Ok("# Hi"), "a byte order mark is dropped");
        let exact = vec![b'a'; README_MAX_BYTES];
        assert!(check_readme_text(&exact).is_ok());
        let over = vec![b'a'; README_MAX_BYTES + 1];
        assert!(check_readme_text(&over).unwrap_err().contains("32 KiB"));
        assert!(check_readme_text(&[0xFF, 0xFE, b'a']).unwrap_err().contains("UTF-8"));
        assert!(check_readme_text(b"a\0b").unwrap_err().contains("NUL"));
    }
}
