//! From a [`Blueprint`] to GUI instances: one [`Frame`] per ScreenGui (the
//! HUD, then each screen), each a flat list of [`Node`]s in paint order.
//!
//! The preview draws these nodes straight into the Studio overlay, and
//! Insert writes the same nodes to the Space, so what the preview shows is
//! what Play shows.
//!
//! Only what the overlay and Play actually draw is used: position, size and
//! AnchorPoint as UDim2, fills with transparency, borders, `corner_radius`
//! on the element itself, text with size, colour, weight (from the font
//! name) and horizontal alignment. Drop shadows are a darker copy of a
//! plate a few pixels down; list and grid layouts are computed here into
//! plain positions.
//!
//! Paint order is creation order: every node's ZIndex is its index plus
//! the frame's base, so a child always paints over its parent and a shadow
//! under its plate.

use super::blueprint::*;
use super::catalog::{self, KitDef, Outline, Plate};

pub type Rgba = [f32; 4];

/// What a node does in Play, for the scripts Insert writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    None,
    /// The frame's full-screen root; hidden until opened (screens only).
    Root,
    /// A button that opens a screen.
    Open(String),
    /// A button that closes this frame's screen.
    Close,
    /// A button the game handles: `UI.on(action, fn(item))`.
    Action { action: String, item: String },
    /// The text that shows an element's value.
    Value(String),
    /// The fill of an element's bar; its width is the value's fraction.
    Fill(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    /// "ScreenGui", "Frame", "TextLabel", "TextButton" or "TextBox".
    pub class: &'static str,
    pub parent: Option<usize>,
    /// UDim2 as `[x_scale, x_offset, y_scale, y_offset]`.
    pub position: [f32; 4],
    pub size: [f32; 4],
    pub anchor: [f32; 2],
    /// Fill colour, 0 to 1; alpha 0 is no fill.
    pub bg: Rgba,
    pub border: f32,
    pub border_color: Rgba,
    pub corner: f32,
    pub text: String,
    pub text_color: Rgba,
    pub font: String,
    pub font_size: f32,
    /// "Left", "Center" or "Right".
    pub align: &'static str,
    pub visible: bool,
    pub role: Role,
}

impl Node {
    fn new(class: &'static str, name: &str) -> Self {
        Self {
            name: name.to_string(),
            class,
            parent: None,
            position: [0.0; 4],
            size: [1.0, 0.0, 1.0, 0.0],
            anchor: [0.0, 0.0],
            bg: [0.0; 4],
            border: 0.0,
            border_color: [0.0; 4],
            corner: 0.0,
            text: String::new(),
            text_color: [1.0; 4],
            font: String::new(),
            font_size: 14.0,
            align: "Center",
            visible: true,
            role: Role::None,
        }
    }

    fn at(mut self, position: [f32; 4], size: [f32; 4]) -> Self {
        self.position = position;
        self.size = size;
        self
    }

    fn anchored(mut self, ax: f32, ay: f32) -> Self {
        self.anchor = [ax, ay];
        self
    }

    fn fill(mut self, c: Rgba) -> Self {
        self.bg = c;
        self
    }

    fn outline(mut self, px: f32, c: Rgba) -> Self {
        self.border = px;
        self.border_color = c;
        self
    }

    fn round(mut self, r: f32) -> Self {
        self.corner = r;
        self
    }

    fn text(mut self, t: &str, color: Rgba, size: f32, font: &str) -> Self {
        self.text = t.to_string();
        self.text_color = color;
        self.font_size = size;
        self.font = font.to_string();
        self
    }

    fn align(mut self, a: &'static str) -> Self {
        self.align = a;
        self
    }

    fn role(mut self, r: Role) -> Self {
        self.role = r;
        self
    }
}

/// One ScreenGui's worth of nodes. `nodes[0]` is the ScreenGui.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// "hud" or the screen's id.
    pub id: String,
    pub title: String,
    pub kind: Option<ScreenKind>,
    /// ZIndex of `nodes[0]`; each node adds its index.
    pub z_base: i32,
    pub nodes: Vec<Node>,
}

impl Frame {
    /// The ScreenGui's name in the Space: `<Name>_HUD`, `<Name>_Shop`.
    pub fn gui_name(&self, blueprint_name: &str) -> String {
        if self.id == "hud" {
            format!("{blueprint_name}_HUD")
        } else {
            format!("{blueprint_name}_{}", pascal_name(&self.id))
        }
    }

    /// The names from the ScreenGui down to node `i`, not counting the
    /// ScreenGui itself: what a script walks with FindFirstChild.
    pub fn path(&self, mut i: usize) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(p) = self.nodes[i].parent {
            out.push(self.nodes[i].name.clone());
            i = p;
        }
        out.reverse();
        out
    }
}

// ============================================================================
// Colour
// ============================================================================

fn rgb(c: Rgb) -> Rgba {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
}

fn rgba(c: Rgb, a: f32) -> Rgba {
    let mut out = rgb(c);
    out[3] = a;
    out
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let l = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round().clamp(0.0, 255.0) as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])]
}

fn luminance(c: Rgb) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

/// Text that reads on `c`: near-black on light colours, white on dark.
fn ink_on(c: Rgb) -> Rgb {
    if luminance(c) > 0.6 { [24, 20, 28] } else { [255, 255, 255] }
}

const GO_GREEN: Rgb = [70, 192, 92];
const PARCHMENT: Rgb = [244, 228, 196];
const BROWN: Rgb = [110, 74, 44];
const BADGE_RED: Rgb = [228, 52, 60];

/// Everything one element or screen needs to paint itself.
struct Paint<'a> {
    pal: &'a Palette,
    kit: &'static KitDef,
    heading: &'a str,
    body: String,
}

impl<'a> Paint<'a> {
    fn new(bp: &'a Blueprint, kit: &'static KitDef) -> Self {
        let body = if kit.font.is_empty() { bp.body_font.clone() } else { kit.font.to_string() };
        Self { pal: &bp.palette, kit, heading: &bp.heading_font, body }
    }

    /// The plate's own colour, ignoring its opacity.
    fn plate_rgb(&self) -> Rgb {
        let p = self.pal;
        match self.kit.plate {
            Plate::Dark | Plate::Clear => p.dark,
            Plate::DeepPrimary => mix(p.primary, p.dark, 0.45),
            Plate::Light => mix(p.light, p.primary, 0.08),
            Plate::Glass => [255, 255, 255],
            Plate::Parchment => PARCHMENT,
            Plate::Black => [0, 0, 0],
        }
    }

    fn plate(&self) -> Rgba {
        match self.kit.plate {
            Plate::Clear => [0.0; 4],
            _ => rgba(self.plate_rgb(), self.kit.alpha),
        }
    }

    /// Screens need a solid backing even in see-through kits.
    fn panel(&self) -> Rgba {
        match self.kit.plate {
            Plate::Clear | Plate::Glass => rgba(self.pal.dark, 0.94),
            _ => rgba(self.plate_rgb(), self.kit.alpha.max(0.94)),
        }
    }

    /// Text on a plate.
    fn ink(&self) -> Rgba {
        match self.kit.plate {
            Plate::Light => rgb(self.pal.dark),
            Plate::Parchment => rgb([58, 40, 26]),
            _ => rgb(self.pal.light),
        }
    }

    /// Text on a screen panel.
    fn panel_ink(&self) -> Rgba {
        match self.kit.plate {
            Plate::Clear | Plate::Glass => rgb(self.pal.light),
            _ => self.ink(),
        }
    }

    fn muted(&self) -> Rgba {
        let mut c = self.ink();
        c[3] = 0.66;
        c
    }

    fn line(&self) -> (f32, Rgba) {
        let p = self.pal;
        let c = match self.kit.outline {
            Outline::None => return (0.0, [0.0; 4]),
            Outline::Ink => rgb(mix(p.dark, [0, 0, 0], 0.5)),
            Outline::Primary => rgb(p.primary),
            Outline::Accent => rgb(p.accent),
            Outline::Frost => [1.0, 1.0, 1.0, 0.35],
            Outline::Light => rgb(p.light),
            Outline::Brown => rgb(BROWN),
        };
        (self.kit.outline_px, c)
    }

    fn caption(&self, s: &str) -> String {
        if self.kit.upper { s.to_uppercase() } else { s.to_string() }
    }
}

// ============================================================================
// The builder
// ============================================================================

struct Canvas {
    nodes: Vec<Node>,
}

impl Canvas {
    /// Add `node` under `parent`, renaming it if a sibling already has its
    /// name, so every node has one path a script can follow.
    fn add(&mut self, parent: usize, mut node: Node) -> usize {
        node.parent = Some(parent);
        let taken = |name: &str, nodes: &[Node]| nodes.iter().any(|n| n.parent == Some(parent) && n.name == name);
        if taken(&node.name, &self.nodes) {
            let base = node.name.clone();
            let mut k = 2;
            while taken(&format!("{base}{k}"), &self.nodes) {
                k += 1;
            }
            node.name = format!("{base}{k}");
        }
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// A plate in the kit's look, with its shadow and stripe. Returns the
    /// plate, which children go into.
    fn plate(&mut self, parent: usize, name: &str, pos: [f32; 4], size: [f32; 4], anchor: [f32; 2], pt: &Paint) -> usize {
        self.plate_with(parent, name, pos, size, anchor, pt, pt.plate())
    }

    #[allow(clippy::too_many_arguments)]
    fn plate_with(&mut self, parent: usize, name: &str, pos: [f32; 4], size: [f32; 4], anchor: [f32; 2], pt: &Paint, fill: Rgba) -> usize {
        let kit = pt.kit;
        if kit.shadow && fill[3] > 0.0 {
            let shadow_pos = [pos[0], pos[1], pos[2], pos[3] + 4.0];
            self.add(
                parent,
                Node::new("Frame", &format!("{name}Shadow"))
                    .at(shadow_pos, size)
                    .anchored(anchor[0], anchor[1])
                    .fill(rgba(mix(pt.pal.dark, [0, 0, 0], 0.4), 0.6))
                    .round(kit.radius),
            );
        }
        let (line_px, line_c) = pt.line();
        let plate = self.add(
            parent,
            Node::new("Frame", name)
                .at(pos, size)
                .anchored(anchor[0], anchor[1])
                .fill(fill)
                .outline(if fill[3] > 0.0 { line_px } else { 0.0 }, line_c)
                .round(kit.radius),
        );
        if kit.stripe {
            self.add(
                plate,
                Node::new("Frame", "Stripe").at([0.0, 0.0, 0.0, 0.0], [0.0, 4.0, 1.0, 0.0]).fill(rgb(pt.pal.accent)),
            );
        }
        plate
    }

    /// A text label; on a plate-less kit it gets a dark copy 1 px behind it
    /// so it reads over a bright sky.
    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, parent: usize, name: &str, pos: [f32; 4], size: [f32; 4], text: &str, color: Rgba, font_size: f32, font: &str, align: &'static str, pt: &Paint) -> usize {
        if pt.kit.plate == Plate::Clear {
            self.add(
                parent,
                Node::new("TextLabel", &format!("{name}Shade"))
                    .at([pos[0], pos[1] + 1.5, pos[2], pos[3] + 1.5], size)
                    .text(text, [0.0, 0.0, 0.0, 0.85], font_size, font)
                    .align(align),
            );
        }
        self.add(parent, Node::new("TextLabel", name).at(pos, size).text(text, color, font_size, font).align(align))
    }

    /// A clickable text button with a solid fill.
    #[allow(clippy::too_many_arguments)]
    fn button(&mut self, parent: usize, name: &str, pos: [f32; 4], size: [f32; 4], anchor: [f32; 2], text: &str, fill: Rgb, font_size: f32, pt: &Paint, role: Role) -> usize {
        let kit = pt.kit;
        if kit.shadow {
            self.add(
                parent,
                Node::new("Frame", &format!("{name}Shadow"))
                    .at([pos[0], pos[1], pos[2], pos[3] + 3.0], size)
                    .anchored(anchor[0], anchor[1])
                    .fill(rgba(mix(fill, [0, 0, 0], 0.55), 1.0))
                    .round(kit.radius.min(12.0)),
            );
        }
        let (line_px, line_c) = pt.line();
        self.add(
            parent,
            Node::new("TextButton", name)
                .at(pos, size)
                .anchored(anchor[0], anchor[1])
                .fill(rgb(fill))
                .outline(line_px.min(2.0), line_c)
                .round(kit.radius.min(12.0))
                .text(&pt.caption(text), rgb(ink_on(fill)), font_size, pt.heading)
                .role(role),
        )
    }
}

fn udim(xs: f32, xo: f32, ys: f32, yo: f32) -> [f32; 4] {
    [xs, xo, ys, yo]
}

fn px(w: f32, h: f32) -> [f32; 4] {
    [0.0, w, 0.0, h]
}

/// "80" of "100" as a fraction; "3/8" on its own also works.
pub fn fraction(value: &str, max: &str) -> Option<f32> {
    let num = |s: &str| s.trim().trim_end_matches('%').replace(',', "").parse::<f32>().ok();
    if let Some((a, b)) = value.split_once('/') {
        let (a, b) = (num(a)?, num(b)?);
        return (b > 0.0).then(|| (a / b).clamp(0.0, 1.0));
    }
    let v = num(value)?;
    match num(max) {
        Some(m) if m > 0.0 => Some((v / m).clamp(0.0, 1.0)),
        _ if value.trim().ends_with('%') => Some((v / 100.0).clamp(0.0, 1.0)),
        _ => None,
    }
}

// ============================================================================
// HUD
// ============================================================================

const MARGIN: f32 = 16.0;
const GAP: f32 = 10.0;

/// An element's size in pixels, for stacking.
fn element_size(e: &Element) -> (f32, f32) {
    match e.kind {
        ElementKind::Currency => (180.0, 44.0),
        ElementKind::Counter => (if e.max.is_empty() { 140.0 } else { 170.0 }, 66.0),
        ElementKind::Objective => (300.0, 58.0),
        ElementKind::Meter => (250.0, 34.0),
        ElementKind::Timer => (150.0, 62.0),
        ElementKind::Hotbar => {
            let n = e.items.len().clamp(1, 9) as f32;
            (n * 58.0 + (n - 1.0) * 6.0, 84.0)
        }
        ElementKind::TeamScore => (380.0, 60.0),
        ElementKind::Minimap => (150.0, 150.0),
        ElementKind::Button => (140.0, 42.0),
        ElementKind::Menu => (160.0, (e.links.len().max(1) as f32) * 50.0 - 8.0),
        ElementKind::Tracker => (300.0, 38.0 + 28.0 * e.items.len().max(1) as f32),
        ElementKind::Controls => (200.0, 34.0 + 26.0 * e.items.len().max(1) as f32),
        ElementKind::Banner => (440.0, 56.0),
    }
}

/// The HUD's ScreenGui: every visible element, stacked by anchor.
pub fn hud_frame(bp: &Blueprint) -> Frame {
    let mut cv = Canvas { nodes: vec![Node::new("ScreenGui", "HUD")] };
    // Offsets already used at each anchor, measured from its edge.
    let mut used: std::collections::HashMap<Anchor, f32> = std::collections::HashMap::new();
    // Middle anchors centre their whole stack, so measure it first.
    let mut middle_total: std::collections::HashMap<Anchor, f32> = std::collections::HashMap::new();
    for e in bp.hud.iter().filter(|e| e.visible) {
        if matches!(e.anchor, Anchor::Left | Anchor::Center | Anchor::Right) {
            let h = element_size(e).1;
            let t = middle_total.entry(e.anchor).or_insert(-GAP);
            *t += h + GAP;
        }
    }

    for e in bp.hud.iter().filter(|e| e.visible) {
        let kit = if e.look.is_empty() { super::generate::resolved_kit(bp) } else { catalog::kit(&e.look).unwrap_or(super::generate::resolved_kit(bp)) };
        let pt = Paint::new(bp, kit);
        let (w, h) = element_size(e);
        let (fx, fy) = e.anchor.fraction();
        let x_off = match e.anchor {
            Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => MARGIN,
            Anchor::TopRight | Anchor::Right | Anchor::BottomRight => -MARGIN,
            _ => 0.0,
        };
        let stacked = used.entry(e.anchor).or_insert(0.0);
        let (y_off, ay) = match e.anchor {
            Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => (MARGIN + *stacked, 0.0),
            Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight => (-(MARGIN + *stacked), 1.0),
            _ => (-middle_total.get(&e.anchor).copied().unwrap_or(h) / 2.0 + *stacked, 0.0),
        };
        *stacked += h + GAP;
        let pos = udim(fx, x_off, fy, y_off);
        let anchor = [fx, ay];
        draw_element(&mut cv, e, pos, px(w, h), anchor, &pt);
    }

    Frame { id: "hud".to_string(), title: "HUD".to_string(), kind: None, z_base: 10, nodes: cv.nodes }
}

fn draw_element(cv: &mut Canvas, e: &Element, pos: [f32; 4], size: [f32; 4], anchor: [f32; 2], pt: &Paint) {
    let (w, h) = (size[1], size[3]);
    let ink = pt.ink();
    let accent = rgb(pt.pal.accent);
    let body = pt.body.clone();
    let heading = pt.heading;
    match e.kind {
        ElementKind::Currency => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            let badge = cv.add(
                root,
                Node::new("Frame", "Badge").at(udim(0.0, 7.0, 0.5, 0.0), px(30.0, 30.0)).anchored(0.0, 0.5)
                    .fill(accent).round(15.0),
            );
            cv.add(badge, Node::new("TextLabel", "Icon").text(if e.icon.is_empty() { "$" } else { e.icon.as_str() }, rgb(ink_on(pt.pal.accent)), 18.0, heading));
            cv.label(root, "Caption", udim(0.0, 44.0, 0.0, 3.0), px(w - 90.0, 14.0), &pt.caption(&e.label), pt.muted(), 11.0, &body, "Left", pt);
            let v = cv.label(root, "Value", udim(0.0, 44.0, 0.0, 15.0), px(w - 90.0, 26.0), &e.value, ink, 22.0, heading, "Left", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
            if !e.opens.is_empty() {
                cv.button(root, "Add", udim(1.0, -7.0, 0.5, 0.0), px(30.0, 30.0), [1.0, 0.5], "+", GO_GREEN, 20.0, pt, Role::Open(e.opens.clone()));
            }
        }
        ElementKind::Counter => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            cv.label(root, "Caption", udim(0.0, 0.0, 0.0, 6.0), udim(1.0, 0.0, 0.0, 14.0), &pt.caption(&e.label), accent, 12.0, &body, "Center", pt);
            let shown = pad_digits(&e.value, e.digits);
            let text = if e.max.is_empty() { shown } else { format!("{shown}/{}", e.max) };
            let v = cv.label(root, "Value", udim(0.0, 0.0, 0.0, 22.0), udim(1.0, 0.0, 0.0, 38.0), &text, ink, 32.0, heading, "Center", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
        }
        ElementKind::Objective => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            cv.label(root, "Caption", udim(0.0, 14.0, 0.0, 7.0), udim(1.0, -28.0, 0.0, 14.0), &pt.caption(&e.label), accent, 11.0, &body, "Left", pt);
            let v = cv.label(root, "Value", udim(0.0, 14.0, 0.0, 23.0), udim(1.0, -28.0, 0.0, 26.0), &e.value, ink, 17.0, &body, "Left", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
        }
        ElementKind::Meter => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            let badge = cv.add(
                root,
                Node::new("Frame", "Badge").at(udim(0.0, 5.0, 0.5, 0.0), px(24.0, 24.0)).anchored(0.0, 0.5)
                    .fill(accent).round(12.0),
            );
            cv.add(badge, Node::new("TextLabel", "Icon").text(if e.icon.is_empty() { "+" } else { e.icon.as_str() }, rgb(ink_on(pt.pal.accent)), 14.0, heading));
            let track = cv.add(
                root,
                Node::new("Frame", "Track").at(udim(0.0, 36.0, 0.5, 0.0), udim(1.0, -96.0, 0.0, 14.0)).anchored(0.0, 0.5)
                    .fill(rgba(mix(pt.pal.dark, [0, 0, 0], 0.3), 0.85)).round(7.0),
            );
            let f = fraction(&e.value, &e.max).unwrap_or(1.0);
            let fill = cv.add(track, Node::new("Frame", "Fill").at([0.0; 4], udim(f, 0.0, 1.0, 0.0)).fill(accent).round(7.0));
            cv.nodes[fill].role = Role::Fill(e.id.clone());
            cv.label(track, "Caption", udim(0.0, 6.0, 0.0, 0.0), udim(1.0, -6.0, 1.0, 0.0), &pt.caption(&e.label), rgb(ink_on(pt.pal.accent)), 10.0, &body, "Left", pt);
            let pct = format!("{}%", (f * 100.0).round() as i32);
            let v = cv.label(root, "Value", udim(1.0, -54.0, 0.0, 0.0), udim(0.0, 48.0, 1.0, 0.0), &pct, ink, 15.0, heading, "Right", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
        }
        ElementKind::Timer => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            cv.label(root, "Caption", udim(0.0, 0.0, 0.0, 6.0), udim(1.0, 0.0, 0.0, 14.0), &pt.caption(&e.label), accent, 11.0, &body, "Center", pt);
            let v = cv.label(root, "Value", udim(0.0, 0.0, 0.0, 22.0), udim(1.0, 0.0, 0.0, 34.0), &e.value, ink, 30.0, heading, "Center", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
        }
        ElementKind::Hotbar => {
            let root = cv.add(0, Node::new("Frame", &e.id).at(pos, size).anchored(anchor[0], anchor[1]));
            let selected = e.value.trim().parse::<usize>().unwrap_or(1).max(1);
            let current = e.items.get(selected - 1).cloned().unwrap_or_else(|| e.label.clone());
            let v = cv.label(root, "Value", udim(0.0, 0.0, 0.0, 0.0), udim(1.0, 0.0, 0.0, 20.0), &pt.caption(&current), ink_or_light(pt), 15.0, heading, "Center", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
            for (i, item) in e.items.iter().take(9).enumerate() {
                let x = i as f32 * 64.0;
                let slot = cv.plate(root, &format!("Slot{}", i + 1), udim(0.0, x, 0.0, 26.0), px(58.0, 58.0), [0.0, 0.0], pt);
                if i + 1 == selected {
                    cv.nodes[slot].border = 3.0;
                    cv.nodes[slot].border_color = accent;
                }
                cv.label(slot, "Key", udim(0.0, 5.0, 0.0, 3.0), px(14.0, 14.0), &format!("{}", i + 1), pt.muted(), 11.0, &body, "Left", pt);
                cv.label(slot, "Item", udim(0.0, 3.0, 0.0, 18.0), udim(1.0, -6.0, 1.0, -22.0), &short(item, 9), ink, 11.0, &body, "Center", pt);
                cv.add(slot, Node::new("TextButton", "Select").role(Role::Action { action: "hotbar".to_string(), item: item.clone() }));
            }
        }
        ElementKind::TeamScore => {
            let root = cv.add(0, Node::new("Frame", &e.id).at(pos, size).anchored(anchor[0], anchor[1]));
            let (va, vb) = e.value.split_once('-').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).unwrap_or(("0".to_string(), "0".to_string()));
            let left_name = e.items.first().cloned().unwrap_or_else(|| "Blue".to_string());
            let right_name = e.items.get(1).cloned().unwrap_or_else(|| "Red".to_string());
            for (side, name, value, color, x, ax) in [("Left", left_name, va, [64, 140, 255], 0.0, 0.0), ("Right", right_name, vb, [232, 64, 72], 1.0, 1.0)] {
                let plate = cv.plate_with(root, side, udim(x, 0.0, 0.0, 0.0), px(140.0, 60.0), [ax, 0.0], pt, rgb(color));
                cv.label(plate, "Team", udim(0.0, 0.0, 0.0, 4.0), udim(1.0, 0.0, 0.0, 14.0), &pt.caption(&name), [1.0, 1.0, 1.0, 0.8], 11.0, &body, "Center", pt);
                let v = cv.label(plate, "Value", udim(0.0, 0.0, 0.0, 18.0), udim(1.0, 0.0, 0.0, 38.0), &value, [1.0; 4], 32.0, heading, "Center", pt);
                cv.nodes[v].role = Role::Value(format!("{}_{}", e.id, side.to_lowercase()));
            }
            let mid = cv.plate(root, "Versus", udim(0.5, 0.0, 0.0, 6.0), px(90.0, 48.0), [0.5, 0.0], pt);
            cv.label(mid, "VS", udim(0.0, 0.0, 0.0, 2.0), udim(1.0, 0.0, 0.0, 26.0), "VS", ink, 22.0, heading, "Center", pt);
            cv.label(mid, "Goal", udim(0.0, 0.0, 0.0, 28.0), udim(1.0, 0.0, 0.0, 14.0), &pt.caption(&e.label), pt.muted(), 9.0, &body, "Center", pt);
        }
        ElementKind::Minimap => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            cv.add(
                root,
                Node::new("Frame", "Map").at(udim(0.0, 8.0, 0.0, 8.0), udim(1.0, -16.0, 1.0, -16.0))
                    .fill(rgba(mix(pt.pal.dark, pt.pal.primary, 0.12), 0.9)).round((pt.kit.radius - 4.0).max(0.0)),
            );
            for (name, p, s) in [("GridH", udim(0.0, 8.0, 0.5, 0.0), udim(1.0, -16.0, 0.0, 1.0)), ("GridV", udim(0.5, 0.0, 0.0, 8.0), udim(0.0, 1.0, 1.0, -16.0))] {
                cv.add(root, Node::new("Frame", name).at(p, s).fill([1.0, 1.0, 1.0, 0.12]));
            }
            cv.add(root, Node::new("Frame", "Player").at(udim(0.5, 0.0, 0.5, 0.0), px(12.0, 12.0)).anchored(0.5, 0.5).fill(rgb(BADGE_RED)).round(6.0).outline(2.0, [1.0; 4]));
            cv.label(root, "North", udim(0.5, -10.0, 0.0, 10.0), px(20.0, 16.0), "N", ink, 13.0, heading, "Center", pt);
        }
        ElementKind::Button => {
            let fill = if pt.kit.plate == Plate::Light || pt.kit.plate == Plate::Parchment { pt.plate_rgb() } else { mix(pt.pal.dark, pt.pal.primary, 0.25) };
            let role = if e.opens.is_empty() { Role::Action { action: snake_id(&e.label), item: String::new() } } else { Role::Open(e.opens.clone()) };
            cv.button(0, &e.id, pos, size, anchor, &e.label, fill, 17.0, pt, role);
        }
        ElementKind::Menu => {
            let root = cv.add(0, Node::new("Frame", &e.id).at(pos, size).anchored(anchor[0], anchor[1]));
            for (i, link) in e.links.iter().enumerate() {
                let y = i as f32 * 50.0;
                let fill = if pt.kit.plate == Plate::Light || pt.kit.plate == Plate::Parchment { pt.plate_rgb() } else { mix(pt.pal.dark, pt.pal.primary, 0.25) };
                let role = if link.opens.is_empty() { Role::Action { action: snake_id(&link.label), item: String::new() } } else { Role::Open(link.opens.clone()) };
                cv.button(root, &pascal_name(&link.label), udim(0.0, 0.0, 0.0, y), udim(1.0, 0.0, 0.0, 42.0), [0.0, 0.0], &link.label, fill, 15.0, pt, role);
                if !link.badge.is_empty() {
                    let b = cv.add(
                        root,
                        Node::new("Frame", &format!("{}Badge", pascal_name(&link.label)))
                            .at(udim(1.0, 6.0, 0.0, y - 6.0), px(22.0, 22.0)).anchored(1.0, 0.0)
                            .fill(rgb(BADGE_RED)).round(11.0).outline(2.0, [1.0; 4]),
                    );
                    cv.add(b, Node::new("TextLabel", "Count").text(&link.badge, [1.0; 4], 12.0, heading));
                }
            }
        }
        ElementKind::Tracker => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            let done = e.items.iter().filter(|line| line_done(line)).count();
            cv.label(root, "Title", udim(0.0, 12.0, 0.0, 8.0), udim(1.0, -60.0, 0.0, 18.0), &pt.caption(&e.label), accent, 13.0, heading, "Left", pt);
            cv.label(root, "Count", udim(1.0, -52.0, 0.0, 8.0), px(40.0, 18.0), &format!("{done}/{}", e.items.len()), ink, 13.0, heading, "Right", pt);
            for (i, line) in e.items.iter().enumerate() {
                let y = 34.0 + i as f32 * 28.0;
                let (text, progress) = line.split_once('|').unwrap_or((line.as_str(), ""));
                let is_done = line_done(line);
                cv.add(
                    root,
                    Node::new("Frame", &format!("Check{}", i + 1)).at(udim(0.0, 12.0, 0.0, y + 3.0), px(16.0, 16.0))
                        .fill(if is_done { accent } else { [0.0; 4] }).outline(2.0, ink).round(3.0),
                );
                cv.label(root, &format!("Line{}", i + 1), udim(0.0, 36.0, 0.0, y), udim(1.0, -100.0, 0.0, 22.0), &pt.caption(text), if is_done { pt.muted() } else { ink }, 13.0, &body, "Left", pt);
                cv.label(root, &format!("Progress{}", i + 1), udim(1.0, -66.0, 0.0, y), px(54.0, 22.0), progress, pt.muted(), 12.0, &body, "Right", pt);
            }
        }
        ElementKind::Controls => {
            let root = cv.plate(0, &e.id, pos, size, anchor, pt);
            cv.label(root, "Title", udim(0.0, 12.0, 0.0, 7.0), udim(1.0, -24.0, 0.0, 16.0), &pt.caption(&e.label), accent, 12.0, heading, "Left", pt);
            for (i, line) in e.items.iter().enumerate() {
                let y = 30.0 + i as f32 * 26.0;
                let (key, action) = line.split_once('|').unwrap_or((line.as_str(), ""));
                let chip = cv.add(
                    root,
                    Node::new("Frame", &format!("Key{}", i + 1)).at(udim(0.0, 12.0, 0.0, y), px(46.0, 20.0))
                        .fill(rgba(pt.pal.light, 0.9)).round(4.0),
                );
                cv.add(chip, Node::new("TextLabel", "Text").text(key, rgb(pt.pal.dark), 11.0, heading));
                cv.label(root, &format!("Action{}", i + 1), udim(0.0, 66.0, 0.0, y), udim(1.0, -76.0, 0.0, 20.0), action, ink, 13.0, &body, "Left", pt);
            }
        }
        ElementKind::Banner => {
            let root = cv.plate_with(0, &e.id, pos, size, anchor, pt, rgb(mix(pt.pal.primary, pt.pal.dark, 0.2)));
            let on = rgb(ink_on(mix(pt.pal.primary, pt.pal.dark, 0.2)));
            cv.label(root, "Caption", udim(0.0, 0.0, 0.0, 6.0), udim(1.0, 0.0, 0.0, 16.0), &pt.caption(&e.label), on, 13.0, heading, "Center", pt);
            let v = cv.label(root, "Value", udim(0.0, 0.0, 0.0, 24.0), udim(1.0, 0.0, 0.0, 26.0), &e.value, on, 18.0, &body, "Center", pt);
            cv.nodes[v].role = Role::Value(e.id.clone());
        }
    }
    let _ = (w, h);
}

/// Plate-less text uses the palette's light colour; everything else its ink.
fn ink_or_light(pt: &Paint) -> Rgba {
    match pt.kit.plate {
        Plate::Light | Plate::Parchment => rgb(pt.pal.light),
        _ => pt.ink(),
    }
}

fn line_done(line: &str) -> bool {
    line.split_once('|').and_then(|(_, p)| fraction(p, "")).is_some_and(|f| f >= 1.0)
}

fn pad_digits(value: &str, digits: u32) -> String {
    let d = digits.min(9) as usize;
    if d > 0 && !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) && value.len() < d {
        format!("{}{}", "0".repeat(d - value.len()), value)
    } else {
        value.to_string()
    }
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('.');
        out
    }
}

// ============================================================================
// Screens
// ============================================================================

/// A screen's ScreenGui. Its `Root` covers the screen and starts hidden;
/// the navigation script shows it.
pub fn screen_frame(bp: &Blueprint, s: &Screen) -> Frame {
    let pt = Paint::new(bp, super::generate::resolved_kit(bp));
    let mut cv = Canvas { nodes: vec![Node::new("ScreenGui", &pascal_name(&s.id))] };
    let ink = pt.panel_ink();
    let accent = rgb(pt.pal.accent);
    let heading = pt.heading;
    let body = pt.body.clone();
    let dim = match s.kind {
        ScreenKind::Loading => rgba(pt.pal.dark, 1.0),
        ScreenKind::MainMenu => rgba(pt.pal.dark, 0.84),
        _ => [0.0, 0.0, 0.0, 0.5],
    };
    let root = cv.add(0, Node::new("Frame", "Root").fill(dim).role(Role::Root));
    cv.nodes[root].visible = false;

    match s.kind {
        ScreenKind::Loading => {
            // A soft glow behind the title in the primary colour.
            cv.add(root, Node::new("Frame", "Glow").at(udim(0.5, 0.0, 0.42, 0.0), udim(0.7, 0.0, 0.5, 0.0)).anchored(0.5, 0.5).fill(rgba(pt.pal.primary, 0.12)).round(400.0));
            cv.add(root, Node::new("TextLabel", "Title").at(udim(0.5, 0.0, 0.4, 0.0), udim(0.9, 0.0, 0.0, 70.0)).anchored(0.5, 0.5).text(&bp.title.to_uppercase(), rgb(pt.pal.primary), 56.0, heading));
            cv.add(root, Node::new("TextLabel", "Tagline").at(udim(0.5, 0.0, 0.4, 48.0), udim(0.8, 0.0, 0.0, 24.0)).anchored(0.5, 0.0).text(&bp.tagline, rgba(pt.pal.light, 0.8), 18.0, &body));
            cv.add(root, Node::new("TextLabel", "Status").at(udim(0.3, 0.0, 0.78, -24.0), udim(0.2, 0.0, 0.0, 20.0)).text("Loading...", rgb(pt.pal.light), 14.0, &body).align("Left"));
            let track = cv.add(root, Node::new("Frame", "Track").at(udim(0.3, 0.0, 0.78, 0.0), udim(0.4, 0.0, 0.0, 14.0)).fill(rgba(pt.pal.light, 0.15)).round(7.0));
            let fill = cv.add(track, Node::new("Frame", "Fill").at([0.0; 4], udim(0.44, 0.0, 1.0, 0.0)).fill(accent).round(7.0));
            cv.nodes[fill].role = Role::Fill("loading".to_string());
            let tip = s.items.first().map(|i| i.name.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| bp.tagline.clone());
            cv.add(root, Node::new("TextLabel", "Tip").at(udim(0.5, 0.0, 0.78, 30.0), udim(0.6, 0.0, 0.0, 22.0)).anchored(0.5, 0.0).text(&format!("Tip: {tip}"), rgba(pt.pal.light, 0.7), 14.0, &body));
        }
        ScreenKind::MainMenu => {
            cv.add(root, Node::new("TextLabel", "Logo").at(udim(0.0, 48.0, 0.0, 40.0), udim(0.6, 0.0, 0.0, 64.0)).text(&s.title.to_uppercase(), rgb(pt.pal.primary), 52.0, heading).align("Left"));
            cv.add(root, Node::new("TextLabel", "Tagline").at(udim(0.0, 50.0, 0.0, 104.0), udim(0.6, 0.0, 0.0, 22.0)).text(&bp.tagline, rgba(pt.pal.light, 0.75), 16.0, &body).align("Left"));
            for (i, item) in s.items.iter().enumerate() {
                let y = 150.0 + i as f32 * 58.0;
                let first = i == 0;
                let fill = if first { GO_GREEN } else { mix(pt.pal.dark, pt.pal.light, 0.12) };
                let role = if !item.opens.is_empty() {
                    Role::Open(item.opens.clone())
                } else if first {
                    Role::Action { action: "play".to_string(), item: item.name.clone() }
                } else {
                    Role::Action { action: snake_id(&item.name), item: item.name.clone() }
                };
                let h = if first { 58.0 } else { 46.0 };
                let y = if first { y - 6.0 } else { y + 6.0 };
                cv.button(root, &pascal_name(&item.name), udim(0.0, 48.0, 0.0, y), px(if first { 300.0 } else { 260.0 }, h), [0.0, 0.0], &item.name, fill, if first { 24.0 } else { 17.0 }, &pt, role);
            }
            if let Some(offer) = s.tabs.first() {
                let card = cv.plate_with(root, "Offer", udim(1.0, -48.0, 0.5, 0.0), px(310.0, 200.0), [1.0, 0.5], &pt, rgb(pt.pal.secondary));
                let on = rgb(ink_on(pt.pal.secondary));
                cv.add(card, Node::new("TextLabel", "Limited").at(udim(0.0, 16.0, 0.0, 14.0), udim(1.0, -32.0, 0.0, 14.0)).text("LIMITED TIME", rgb(pt.pal.accent), 12.0, heading).align("Left"));
                cv.add(card, Node::new("TextLabel", "Name").at(udim(0.0, 16.0, 0.0, 34.0), udim(1.0, -32.0, 0.0, 34.0)).text(offer, on, 28.0, heading).align("Left"));
                cv.add(card, Node::new("TextLabel", "Detail").at(udim(0.0, 16.0, 0.0, 74.0), udim(1.0, -32.0, 0.0, 40.0)).text("A head start for new players, for a limited time.", on, 13.0, &body).align("Left"));
                cv.button(card, "ViewOffer", udim(0.0, 16.0, 1.0, -16.0), px(150.0, 40.0), [0.0, 1.0], "View Offer", GO_GREEN, 16.0, &pt, Role::Action { action: "offer".to_string(), item: offer.clone() });
            }
        }
        _ => {
            let (pw, ph) = match s.kind {
                ScreenKind::Results | ScreenKind::Codes => (0.42, 0.56),
                _ => (0.62, 0.74),
            };
            let panel = cv.plate_with(root, "Panel", udim(0.5, 0.0, 0.5, 0.0), udim(pw, 0.0, ph, 0.0), [0.5, 0.5], &pt, pt.panel());
            cv.add(panel, Node::new("TextLabel", "Title").at(udim(0.0, 22.0, 0.0, 14.0), udim(1.0, -90.0, 0.0, 36.0)).text(&pt.caption(&s.title), ink, 28.0, heading).align("Left"));
            cv.add(panel, Node::new("Frame", "Rule").at(udim(0.0, 20.0, 0.0, 58.0), udim(1.0, -40.0, 0.0, 2.0)).fill(rgba(pt.pal.accent, 0.6)));
            cv.button(panel, "Close", udim(1.0, -14.0, 0.0, 14.0), px(36.0, 36.0), [1.0, 0.0], "X", BADGE_RED, 18.0, &pt, Role::Close);
            let body_frame = cv.add(panel, Node::new("Frame", "Body").at(udim(0.0, 20.0, 0.0, 72.0), udim(1.0, -40.0, 1.0, -88.0)));
            draw_screen_body(&mut cv, body_frame, s, &pt, ink);
        }
    }

    let z_base = 1000 + 400 * bp.screens.iter().position(|x| x.id == s.id).unwrap_or(0) as i32;
    Frame { id: s.id.clone(), title: s.title.clone(), kind: Some(s.kind), z_base, nodes: cv.nodes }
}

/// Category tabs down the left of a body; returns the scale the content
/// starts at.
fn tabs_column(cv: &mut Canvas, body: usize, tabs: &[String], pt: &Paint) -> f32 {
    if tabs.is_empty() {
        return 0.0;
    }
    let tab_area = cv.add(body, Node::new("Frame", "Tabs").at([0.0; 4], udim(0.25, 0.0, 1.0, 0.0)));
    for (i, t) in tabs.iter().take(7).enumerate() {
        let fill = if i == 0 { pt.pal.primary } else { mix(pt.pal.dark, pt.pal.light, 0.1) };
        cv.button(tab_area, &pascal_name(t), udim(0.0, 0.0, 0.0, i as f32 * 44.0), udim(1.0, -10.0, 0.0, 38.0), [0.0, 0.0], t, fill, 14.0, pt, Role::Action { action: "tab".to_string(), item: t.clone() });
    }
    0.27
}

fn draw_screen_body(cv: &mut Canvas, body: usize, s: &Screen, pt: &Paint, ink: Rgba) {
    let accent = rgb(pt.pal.accent);
    let heading = pt.heading;
    let font = pt.body.clone();
    let card_fill = rgba(mix(pt.panel_rgb_for_cards(), pt.pal.light, 0.08), 1.0);
    let muted = { let mut m = ink; m[3] = 0.66; m };
    match s.kind {
        ScreenKind::Shop => {
            let x0 = tabs_column(cv, body, &s.tabs, pt);
            let area = cv.add(body, Node::new("Frame", "Items").at(udim(x0, 0.0, 0.0, 0.0), udim(1.0 - x0, 0.0, 1.0, 0.0)));
            let items: Vec<&Item> = s.items.iter().take(6).collect();
            let cols = 2usize;
            let rows = items.len().div_ceil(cols).max(1);
            for (i, item) in items.iter().enumerate() {
                let (c, r) = ((i % cols) as f32, (i / cols) as f32);
                let card = cv.add(
                    area,
                    Node::new("Frame", &pascal_name(&item.name))
                        .at(udim(c / cols as f32, 4.0, r / rows as f32, 4.0), udim(1.0 / cols as f32, -8.0, 1.0 / rows as f32, -8.0))
                        .fill(card_fill).round(pt.kit.radius.min(10.0)).outline(1.0, rgba(pt.pal.light, 0.12)),
                );
                cv.add(card, Node::new("TextLabel", "Name").at(udim(0.0, 12.0, 0.0, 10.0), udim(1.0, -24.0, 0.0, 22.0)).text(&item.name, ink, 18.0, heading).align("Left"));
                cv.add(card, Node::new("TextLabel", "Detail").at(udim(0.0, 12.0, 0.0, 34.0), udim(1.0, -24.0, 0.0, 34.0)).text(&item.detail, muted, 13.0, &font).align("Left"));
                let price_fill = if super::generate::is_robux_price(&item.value) { GO_GREEN } else { pt.pal.accent };
                cv.button(card, "Buy", udim(1.0, -10.0, 1.0, -10.0), px(104.0, 34.0), [1.0, 1.0], &item.value, price_fill, 15.0, pt, Role::Action { action: "buy".to_string(), item: item.name.clone() });
            }
        }
        ScreenKind::Grid => {
            let items: Vec<&Item> = s.items.iter().take(12).collect();
            let cols = if items.len() > 4 { 4 } else { items.len().max(1) };
            let rows = items.len().div_ceil(cols).max(1);
            for (i, item) in items.iter().enumerate() {
                let (c, r) = ((i % cols) as f32, (i / cols) as f32);
                let slot = cv.add(
                    body,
                    Node::new("Frame", &pascal_name(&item.name))
                        .at(udim(c / cols as f32, 5.0, r / rows as f32, 5.0), udim(1.0 / cols as f32, -10.0, 1.0 / rows as f32, -10.0))
                        .fill(card_fill).round(pt.kit.radius.min(12.0))
                        .outline(if i == 0 { 3.0 } else { 1.0 }, if i == 0 { accent } else { rgba(pt.pal.light, 0.12) }),
                );
                cv.add(slot, Node::new("Frame", "Art").at(udim(0.5, 0.0, 0.42, 0.0), udim(0.0, 44.0, 0.0, 44.0)).anchored(0.5, 0.5).fill(rgba(pt.pal.primary, 0.35)).round(22.0));
                cv.add(slot, Node::new("TextLabel", "Name").at(udim(0.0, 6.0, 1.0, -28.0), udim(1.0, -12.0, 0.0, 22.0)).text(&item.name, ink, 14.0, &font));
                cv.add(slot, Node::new("TextButton", "Select").role(Role::Action { action: "select".to_string(), item: item.name.clone() }));
            }
        }
        ScreenKind::List => {
            let items: Vec<&Item> = s.items.iter().take(6).collect();
            let n = items.len().max(3) as f32;
            for (i, item) in items.iter().enumerate() {
                let row = cv.add(
                    body,
                    Node::new("Frame", &pascal_name(&item.name))
                        .at(udim(0.0, 0.0, i as f32 / n, 4.0), udim(1.0, 0.0, 1.0 / n, -8.0))
                        .fill(card_fill).round(pt.kit.radius.min(10.0)),
                );
                cv.add(row, Node::new("Frame", "Icon").at(udim(0.0, 10.0, 0.5, 0.0), px(40.0, 40.0)).anchored(0.0, 0.5).fill(rgba(pt.pal.primary, 0.5)).round(8.0));
                cv.add(row, Node::new("TextLabel", "Name").at(udim(0.0, 62.0, 0.0, 8.0), udim(0.55, -62.0, 0.0, 20.0)).text(&item.name, ink, 16.0, heading).align("Left"));
                cv.add(row, Node::new("TextLabel", "Detail").at(udim(0.0, 62.0, 0.0, 28.0), udim(0.55, -62.0, 0.0, 18.0)).text(&item.detail, muted, 12.0, &font).align("Left"));
                let f = fraction(&item.value, "");
                if let Some(f) = f {
                    let track = cv.add(row, Node::new("Frame", "Track").at(udim(0.56, 0.0, 0.5, 0.0), udim(0.22, 0.0, 0.0, 12.0)).anchored(0.0, 0.5).fill(rgba(pt.pal.light, 0.15)).round(6.0));
                    cv.add(track, Node::new("Frame", "Fill").at([0.0; 4], udim(f, 0.0, 1.0, 0.0)).fill(accent).round(6.0));
                    cv.add(row, Node::new("TextLabel", "Progress").at(udim(0.56, 0.0, 0.5, 8.0), udim(0.22, 0.0, 0.0, 16.0)).text(&item.value, muted, 11.0, &font));
                }
                let ready = f.map_or(true, |f| f >= 1.0);
                let label = if item.value == "craft" { "Craft" } else { "Claim" };
                cv.button(row, label, udim(1.0, -10.0, 0.5, 0.0), px(96.0, 34.0), [1.0, 0.5], label, if ready { GO_GREEN } else { mix(pt.pal.dark, pt.pal.light, 0.2) }, 14.0, pt, Role::Action { action: snake_id(label), item: item.name.clone() });
            }
        }
        ScreenKind::Settings => {
            let x0 = tabs_column(cv, body, &s.tabs, pt);
            let area = cv.add(body, Node::new("Frame", "Rows").at(udim(x0, 0.0, 0.0, 0.0), udim(1.0 - x0, 0.0, 1.0, 0.0)));
            for (i, item) in s.items.iter().take(7).enumerate() {
                let y = i as f32 * 52.0;
                let row = cv.add(area, Node::new("Frame", &pascal_name(&item.name)).at(udim(0.0, 0.0, 0.0, y), udim(1.0, 0.0, 0.0, 46.0)).fill(card_fill).round(pt.kit.radius.min(8.0)));
                cv.add(row, Node::new("TextLabel", "Name").at(udim(0.0, 14.0, 0.0, 5.0), udim(1.0, -100.0, 0.0, 20.0)).text(&item.name, ink, 15.0, heading).align("Left"));
                cv.add(row, Node::new("TextLabel", "Detail").at(udim(0.0, 14.0, 0.0, 24.0), udim(1.0, -100.0, 0.0, 16.0)).text(&item.detail, muted, 11.0, &font).align("Left"));
                let on = item.value.eq_ignore_ascii_case("on");
                let sw = cv.add(
                    row,
                    Node::new("TextButton", "Switch").at(udim(1.0, -14.0, 0.5, 0.0), px(52.0, 26.0)).anchored(1.0, 0.5)
                        .fill(if on { accent } else { rgba(pt.pal.light, 0.2) }).round(13.0)
                        .role(Role::Action { action: "setting".to_string(), item: item.name.clone() }),
                );
                cv.add(sw, Node::new("Frame", "Knob").at(udim(if on { 1.0 } else { 0.0 }, if on { -3.0 } else { 3.0 }, 0.5, 0.0), px(20.0, 20.0)).anchored(if on { 1.0 } else { 0.0 }, 0.5).fill([1.0; 4]).round(10.0));
            }
        }
        ScreenKind::Results => {
            for (i, item) in s.items.iter().take(6).enumerate() {
                let y = i as f32 * 36.0;
                cv.add(body, Node::new("TextLabel", &format!("{}Name", pascal_name(&item.name))).at(udim(0.0, 10.0, 0.0, y), udim(0.6, 0.0, 0.0, 30.0)).text(&item.name, muted, 16.0, &font).align("Left"));
                cv.add(body, Node::new("TextLabel", &format!("{}Value", pascal_name(&item.name))).at(udim(0.6, 0.0, 0.0, y), udim(0.4, -10.0, 0.0, 30.0)).text(&item.value, ink, 20.0, heading).align("Right"));
            }
            cv.button(body, "PlayAgain", udim(0.5, -6.0, 1.0, 0.0), px(150.0, 44.0), [1.0, 1.0], "Play Again", GO_GREEN, 16.0, pt, Role::Action { action: "play_again".to_string(), item: String::new() });
            cv.button(body, "Done", udim(0.5, 6.0, 1.0, 0.0), px(150.0, 44.0), [0.0, 1.0], "Done", mix(pt.pal.dark, pt.pal.light, 0.2), 16.0, pt, Role::Close);
        }
        ScreenKind::Codes => {
            let hint = s.items.first().map(|i| i.detail.clone()).unwrap_or_default();
            cv.add(body, Node::new("TextLabel", "Hint").at(udim(0.0, 0.0, 0.0, 4.0), udim(1.0, 0.0, 0.0, 36.0)).text(&hint, muted, 14.0, &font));
            let field = cv.add(
                body,
                Node::new("TextBox", "Code").at(udim(0.5, 0.0, 0.0, 52.0), udim(0.9, 0.0, 0.0, 48.0)).anchored(0.5, 0.0)
                    .fill(rgba(pt.pal.light, 0.1)).outline(2.0, accent).round(8.0)
                    .text("Enter code", muted, 18.0, &font),
            );
            let _ = field;
            cv.button(body, "Redeem", udim(0.5, 0.0, 0.0, 116.0), px(180.0, 46.0), [0.5, 0.0], "Redeem", GO_GREEN, 18.0, pt, Role::Action { action: "redeem".to_string(), item: String::new() });
        }
        ScreenKind::Pass => {
            let tiers: Vec<&Item> = s.items.iter().take(6).collect();
            let n = tiers.len().max(1) as f32;
            let track = cv.add(body, Node::new("Frame", "Progress").at(udim(0.0, 4.0, 0.0, 4.0), udim(1.0, -8.0, 0.0, 12.0)).fill(rgba(pt.pal.light, 0.15)).round(6.0));
            cv.add(track, Node::new("Frame", "Fill").at([0.0; 4], udim(0.4, 0.0, 1.0, 0.0)).fill(accent).round(6.0));
            let row = cv.add(body, Node::new("Frame", "Tiers").at(udim(0.0, 0.0, 0.0, 28.0), udim(1.0, 0.0, 1.0, -90.0)));
            for (i, t) in tiers.iter().enumerate() {
                let unlocked = (i as f32) < n * 0.4;
                let card = cv.add(
                    row,
                    Node::new("Frame", &format!("Tier{}", i + 1))
                        .at(udim(i as f32 / n, 4.0, 0.0, 0.0), udim(1.0 / n, -8.0, 1.0, 0.0))
                        .fill(card_fill).round(pt.kit.radius.min(10.0))
                        .outline(if unlocked { 2.0 } else { 1.0 }, if unlocked { accent } else { rgba(pt.pal.light, 0.12) }),
                );
                cv.add(card, Node::new("TextLabel", "Tier").at(udim(0.0, 0.0, 0.0, 8.0), udim(1.0, 0.0, 0.0, 16.0)).text(&format!("TIER {}", i + 1), accent, 11.0, heading));
                cv.add(card, Node::new("Frame", "Art").at(udim(0.5, 0.0, 0.45, 0.0), px(42.0, 42.0)).anchored(0.5, 0.5).fill(rgba(pt.pal.primary, if unlocked { 0.8 } else { 0.3 })).round(10.0));
                cv.add(card, Node::new("TextLabel", "Reward").at(udim(0.0, 4.0, 1.0, -34.0), udim(1.0, -8.0, 0.0, 28.0)).text(&t.name, ink, 12.0, &font));
            }
            cv.button(body, "Premium", udim(1.0, 0.0, 1.0, 0.0), px(200.0, 44.0), [1.0, 1.0], "Unlock Premium", GO_GREEN, 16.0, pt, Role::Action { action: "buy_pass".to_string(), item: s.title.clone() });
        }
        ScreenKind::Leaderboard => {
            let header = cv.add(body, Node::new("Frame", "Header").at([0.0; 4], udim(1.0, 0.0, 0.0, 28.0)));
            cv.add(header, Node::new("TextLabel", "Rank").at(udim(0.0, 10.0, 0.0, 0.0), udim(0.1, 0.0, 1.0, 0.0)).text("#", muted, 12.0, heading).align("Left"));
            cv.add(header, Node::new("TextLabel", "Player").at(udim(0.1, 10.0, 0.0, 0.0), udim(0.5, 0.0, 1.0, 0.0)).text("PLAYER", muted, 12.0, heading).align("Left"));
            cv.add(header, Node::new("TextLabel", "Score").at(udim(0.6, 0.0, 0.0, 0.0), udim(0.4, -10.0, 1.0, 0.0)).text("SCORE", muted, 12.0, heading).align("Right"));
            for (i, item) in s.items.iter().take(8).enumerate() {
                let y = 32.0 + i as f32 * 40.0;
                let row = cv.add(body, Node::new("Frame", &format!("Row{}", i + 1)).at(udim(0.0, 0.0, 0.0, y), udim(1.0, 0.0, 0.0, 34.0)).fill(if i == 0 { rgba(pt.pal.accent, 0.25) } else { card_fill }).round(pt.kit.radius.min(6.0)));
                cv.add(row, Node::new("TextLabel", "Rank").at(udim(0.0, 10.0, 0.0, 0.0), udim(0.1, 0.0, 1.0, 0.0)).text(&format!("{}", i + 1), ink, 15.0, heading).align("Left"));
                let who = if item.detail.is_empty() { item.name.clone() } else { format!("{}  ({})", item.name, item.detail) };
                cv.add(row, Node::new("TextLabel", "Player").at(udim(0.1, 10.0, 0.0, 0.0), udim(0.5, 0.0, 1.0, 0.0)).text(&who, ink, 15.0, &font).align("Left"));
                cv.add(row, Node::new("TextLabel", "Score").at(udim(0.6, 0.0, 0.0, 0.0), udim(0.4, -10.0, 1.0, 0.0)).text(&item.value, ink, 15.0, heading).align("Right"));
            }
        }
        ScreenKind::Info | ScreenKind::Loading | ScreenKind::MainMenu => {
            let mut y = 0.0;
            for (i, item) in s.items.iter().take(6).enumerate() {
                cv.add(body, Node::new("TextLabel", &format!("Heading{}", i + 1)).at(udim(0.0, 6.0, 0.0, y), udim(1.0, -12.0, 0.0, 24.0)).text(&item.name, rgb(pt.pal.accent), 18.0, heading).align("Left"));
                cv.add(body, Node::new("TextLabel", &format!("Text{}", i + 1)).at(udim(0.0, 6.0, 0.0, y + 26.0), udim(1.0, -12.0, 0.0, 46.0)).text(&item.detail, ink, 15.0, &font).align("Left"));
                y += 82.0;
            }
        }
    }
}

impl Paint<'_> {
    /// The base colour cards on a screen panel are mixed from.
    fn panel_rgb_for_cards(&self) -> Rgb {
        match self.kit.plate {
            Plate::Clear | Plate::Glass => self.pal.dark,
            _ => self.plate_rgb(),
        }
    }
}

/// How `kit` paints a plate in `palette`: (fill, text, outline colour,
/// outline px). The Design tab's kit thumbnails.
pub fn kit_swatch(palette: &Palette, kit: &'static KitDef) -> (Rgba, Rgba, Rgba, f32) {
    let bp = Blueprint { palette: palette.clone(), ..Default::default() };
    let pt = Paint::new(&bp, kit);
    let (line_px, line) = pt.line();
    let fill = match kit.plate {
        // Plate-less text sits on the world; show it on a faint plate.
        Plate::Clear => [0.0, 0.0, 0.0, 0.25],
        _ => pt.plate(),
    };
    (fill, pt.ink(), line, line_px)
}

/// Every frame of the blueprint: the HUD, then each screen.
pub fn frames(bp: &Blueprint) -> Vec<Frame> {
    let mut out = vec![hud_frame(bp)];
    for s in &bp.screens {
        out.push(screen_frame(bp, s));
    }
    out
}

// ============================================================================
// Resolving to pixels (the preview)
// ============================================================================

/// Each node's rect in pixels, `(x, y, w, h)`, inside a viewport of
/// `size`: the same arithmetic as the Studio overlay's `resolve_gui_rect`.
/// Parents always come before their children in `nodes`.
pub fn resolve(nodes: &[Node], size: (f32, f32)) -> Vec<(f32, f32, f32, f32)> {
    let mut out: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let (px_, py, pw, ph) = match n.parent {
            Some(p) if p < out.len() => out[p],
            _ => (0.0, 0.0, size.0, size.1),
        };
        if n.class == "ScreenGui" {
            out.push((0.0, 0.0, size.0, size.1));
            continue;
        }
        let w = (n.size[0] * pw + n.size[1]).max(1.0);
        let h = (n.size[2] * ph + n.size[3]).max(1.0);
        let x = px_ + n.position[0] * pw + n.position[1] - n.anchor[0] * w;
        let y = py + n.position[2] * ph + n.position[3] - n.anchor[1] * h;
        out.push((x, y, w, h));
    }
    out
}

/// Whether each node shows, counting hidden ancestors.
pub fn shown(nodes: &[Node]) -> Vec<bool> {
    let mut out: Vec<bool> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let parent_shown = n.parent.map_or(true, |p| out.get(p).copied().unwrap_or(true));
        out.push(parent_shown && n.visible);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_builder::generate::{generate, seed_for};

    #[test]
    fn every_genre_lays_out_without_overlapping_names() {
        for g in catalog::GENRES {
            let bp = generate(g.example, seed_for(g.example));
            for f in frames(&bp) {
                assert_eq!(f.nodes[0].class, "ScreenGui");
                for (i, n) in f.nodes.iter().enumerate().skip(1) {
                    let p = n.parent.expect("every node but the ScreenGui has a parent");
                    assert!(p < i, "parents come first");
                    let clash = f.nodes.iter().enumerate().any(|(j, m)| j != i && m.parent == n.parent && m.name == n.name);
                    assert!(!clash, "{}: two siblings named {}", f.id, n.name);
                }
            }
        }
    }

    #[test]
    fn screens_start_hidden_and_the_hud_shows() {
        let bp = generate("a tycoon", seed_for("a tycoon"));
        let fr = frames(&bp);
        assert!(shown(&fr[0].nodes).iter().skip(1).any(|s| *s));
        for f in &fr[1..] {
            assert!(shown(&f.nodes).iter().skip(1).all(|s| !*s), "{} shows before it is opened", f.id);
        }
    }

    #[test]
    fn every_screen_can_be_closed_or_is_fullscreen() {
        let bp = generate("a battle royale with emotes", 4242);
        for f in frames(&bp).into_iter().skip(1) {
            let full = f.kind.map_or(false, |k| k.is_fullscreen());
            let closes = f.nodes.iter().any(|n| n.role == Role::Close);
            assert!(full || closes, "{} has no Close", f.id);
        }
    }

    #[test]
    fn anchors_resolve_inside_the_view() {
        let bp = generate("a racing game", 7);
        let hud = hud_frame(&bp);
        let rects = resolve(&hud.nodes, (1280.0, 720.0));
        for (n, r) in hud.nodes.iter().zip(&rects).skip(1) {
            if n.parent == Some(0) && !n.name.ends_with("Shadow") {
                assert!(r.0 >= -1.0 && r.1 >= -1.0 && r.0 + r.2 <= 1281.0 && r.1 + r.3 <= 721.0, "{} at {:?}", n.name, r);
            }
        }
    }

    #[test]
    fn fractions() {
        assert_eq!(fraction("80", "100"), Some(0.8));
        assert_eq!(fraction("3/4", ""), Some(0.75));
        assert_eq!(fraction("1,500", "3,000"), Some(0.5));
        assert_eq!(fraction("abc", ""), None);
        assert_eq!(pad_digits("79", 4), "0079");
    }
}
