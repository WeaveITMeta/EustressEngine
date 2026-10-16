// =============================================================================
// Eustress Web - Economics drawing kit (/economics)
// =============================================================================
// What the Economics page's charts are drawn with: text for SVG, and a Sankey (boxes joined by ribbons as thick as the money in them).
// Numbers go in and markup comes out, so tests read the markup. Colors are not set here: every box, ribbon and word carries a class
// (`n-tickets`, `l-plans`, `c-drip`) that the stylesheet colors from the Eustress palette (style/gallery.css, ".eco-svg"). Everything
// printed on a chart is a number from the model or a fixed phrase, escaped; nothing a visitor types reaches the markup.
//
// Table of Contents:
// 1. Words and numbers
// 2. Text and the Sankey
// 3. Tests
// =============================================================================

use super::economics_model::{commas, dollars};

/// The width every chart is drawn in; the page scales it to the column.
pub const WIDTH: f64 = 1400.0;

// -----------------------------------------------------------------------------
// 1. Words and numbers
// -----------------------------------------------------------------------------

/// Escape text for an SVG text node or attribute.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A coordinate or size with at most two decimals and no trailing zeros.
pub fn n(x: f64) -> String {
    let s = format!("{x:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "" | "-" | "-0" => "0".to_string(),
        s => s.to_string(),
    }
}

/// A count of units, rounded: "49,500".
pub fn num(x: f64) -> String {
    commas(x.round() as i64)
}

/// Whole dollars from a float.
pub fn fmt(x: f64) -> String {
    dollars(x.round() as i64)
}

/// A share as a percent: one decimal under 10%, none from 10%; "0%" when the whole is not positive.
pub fn pct(part: f64, whole: f64) -> String {
    if !(whole > 0.0) {
        return "0%".to_string();
    }
    let p = 100.0 * part / whole;
    if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

// -----------------------------------------------------------------------------
// 2. Text and the Sankey
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum A {
    Start,
    Middle,
    End,
}

/// The three layers of a drawing, in the order they are painted: ribbons, boxes, words.
#[derive(Default)]
pub struct Canvas {
    pub links: String,
    pub rects: String,
    pub texts: String,
}

impl Canvas {
    /// A line of text. `tone` is a class such as "c-drip"; `weight` is 400, 600 or 700.
    pub fn text(&mut self, x: f64, y: f64, s: &str, size: f64, weight: u16, tone: &str, a: A) {
        let anchor = match a {
            A::Start => "",
            A::Middle => " text-anchor=\"middle\"",
            A::End => " text-anchor=\"end\"",
        };
        let w = match weight {
            700 => " w7",
            600 => " w6",
            _ => "",
        };
        self.texts.push_str(&format!("<text class=\"{tone}{w}\" x=\"{}\" y=\"{}\"{anchor} font-size=\"{}\">{}</text>", n(x), n(y), n(size), esc(s)));
    }

    /// Text turned a quarter, reading upward, centred on a point.
    pub fn text_up(&mut self, cx: f64, cy: f64, s: &str, size: f64, weight: u16, tone: &str) {
        let w = if weight == 700 { " w7" } else { "" };
        self.texts.push_str(&format!(
            "<text class=\"{tone}{w}\" x=\"{x}\" y=\"{y}\" transform=\"rotate(-90 {x} {y})\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"{}\" data-up=\"1\">{}</text>",
            n(size),
            esc(s),
            x = n(cx),
            y = n(cy)
        ));
    }

    /// The finished drawing: an SVG with its ribbons, boxes and words, a title and a description for readers who cannot see it.
    pub fn finish(self, height: f64, title: &str, desc: &str) -> String {
        format!(
            "<svg class=\"eco-svg\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"{t}\"><title>{t}</title><desc>{d}</desc>{}{}{}</svg>",
            self.links,
            self.rects,
            self.texts,
            w = n(WIDTH),
            h = n(height.ceil()),
            t = esc(title),
            d = esc(desc)
        )
    }
}

pub struct Node {
    pub id: &'static str,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub v: f64,
    /// The palette class suffix: "tickets" gives the fill class `n-tickets`.
    pub tone: &'static str,
    /// How much of the box's height its outgoing and incoming ribbons have used so far.
    pub out: f64,
    pub inn: f64,
}

/// Boxes and ribbons whose thickness is proportional to money: `k` pixels a unit, never under 3.
pub struct Sankey {
    pub k: f64,
    pub nodes: Vec<Node>,
    pub canvas: Canvas,
}

impl Sankey {
    pub fn new(k: f64) -> Self {
        Sankey { k, nodes: Vec::new(), canvas: Canvas::default() }
    }

    /// The height of a box or ribbon that holds `v`.
    pub fn h(&self, v: f64) -> f64 {
        if v > 0.0 {
            (v * self.k).max(3.0)
        } else {
            0.0
        }
    }

    pub fn node(&mut self, id: &'static str, x: f64, y: f64, w: f64, v: f64, tone: &'static str) {
        let h = self.h(v);
        self.nodes.push(Node { id, x, y, w, h, v, tone, out: 0.0, inn: 0.0 });
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn pos(&self, id: &str) -> usize {
        self.nodes.iter().position(|n| n.id == id).expect("a ribbon joins two boxes that exist")
    }

    /// A ribbon from one box to another; each box hands out its height top to bottom in the order the ribbons are made.
    pub fn link(&mut self, from: &str, to: &str, v: f64, tone: &str, opacity: f64) {
        if !(v > 0.0) {
            return;
        }
        let (a, b) = (self.pos(from), self.pos(to));
        let w = self.h(v);
        let y0 = self.nodes[a].y + self.nodes[a].out + w / 2.0;
        self.nodes[a].out += w;
        let y1 = self.nodes[b].y + self.nodes[b].inn + w / 2.0;
        self.nodes[b].inn += w;
        let x0 = self.nodes[a].x + self.nodes[a].w;
        let x1 = self.nodes[b].x;
        let mx = (x0 + x1) / 2.0;
        self.canvas.links.push_str(&format!(
            "<path class=\"l-{tone}\" d=\"M{},{} C{},{} {},{} {},{}\" stroke-width=\"{}\" opacity=\"{}\"/>",
            n(x0),
            n(y0),
            n(mx),
            n(y0),
            n(mx),
            n(y1),
            n(x1),
            n(y1),
            n(w),
            n(opacity)
        ));
    }

    /// Paint every box that holds money.
    pub fn draw(&mut self) {
        for node in &self.nodes {
            if node.v > 0.0 {
                self.canvas.rects.push_str(&format!(
                    "<rect class=\"n-{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                    node.tone,
                    n(node.x),
                    n(node.y),
                    n(node.w),
                    n(node.h.max(3.0))
                ));
            }
        }
    }
}

/// A chart with nothing to show: one line saying how to get something.
pub fn empty_svg(message: &str, height: f64) -> String {
    let mut c = Canvas::default();
    c.text(WIDTH / 2.0, height / 2.0, message, 22.0, 400, "c-3", A::Middle);
    c.finish(height, message, message)
}

// -----------------------------------------------------------------------------
// 3. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_so_no_markup_can_be_written_into_a_chart() {
        assert_eq!(esc("a & b < c > \"d\" 'e'"), "a &amp; b &lt; c &gt; &quot;d&quot; &#39;e&#39;");
        assert_eq!(esc("plain $1,234"), "plain $1,234");
        let mut c = Canvas::default();
        c.text(1.0, 2.0, "<script>alert(1)</script>", 14.0, 400, "c-1", A::Start);
        assert!(!c.texts.contains("<script>") && c.texts.contains("&lt;script&gt;"));
        let svg = Canvas::default().finish(100.0, "a \"title\" <b>", "d & d");
        assert!(svg.contains("aria-label=\"a &quot;title&quot; &lt;b&gt;\"") && svg.contains("<desc>d &amp; d</desc>"), "{svg}");
    }

    #[test]
    fn coordinates_are_short_and_never_negative_zero() {
        assert_eq!(n(0.0), "0");
        assert_eq!(n(-0.001), "0");
        assert_eq!(n(10.0), "10");
        assert_eq!(n(10.5), "10.5");
        assert_eq!(n(10.456), "10.46");
        assert_eq!(n(100.0), "100");
        assert_eq!(n(-3.25), "-3.25");
    }

    #[test]
    fn numbers_are_written_the_way_the_labels_print_them() {
        assert_eq!(num(49_499.6), "49,500");
        assert_eq!(fmt(84_375.4), "$84,375");
        assert_eq!(pct(5.0, 100.0), "5.0%");
        assert_eq!(pct(22.3, 100.0), "22%");
        assert_eq!(pct(1.0, 0.0), "0%");
        assert_eq!(pct(1.0, -5.0), "0%");
    }

    #[test]
    fn a_ribbon_is_as_thick_as_its_money_and_no_thinner_than_three() {
        let mut sk = Sankey::new(0.01);
        sk.node("a", 0.0, 0.0, 10.0, 1000.0, "tickets");
        sk.node("b", 100.0, 0.0, 10.0, 1000.0, "plans");
        sk.link("a", "b", 600.0, "tickets", 0.5);
        sk.link("a", "b", 1.0, "plans", 0.5);
        sk.link("a", "b", 0.0, "plans", 0.5);
        assert_eq!(sk.canvas.links.matches("<path").count(), 2, "a ribbon with no money is not drawn");
        assert!(sk.canvas.links.contains("stroke-width=\"6\""), "600 at 0.01 a unit is 6 px: {}", sk.canvas.links);
        assert!(sk.canvas.links.contains("stroke-width=\"3\""), "a ribbon is never thinner than 3 px");
        // the first ribbon is centred 3 px down the box, and the second starts where the first ended: 6 px down, centred 1.5 px lower
        assert!(sk.canvas.links.contains("M10,3 ") && sk.canvas.links.contains("M10,7.5 "), "{}", sk.canvas.links);
    }

    #[test]
    fn a_box_with_no_money_is_not_painted_and_a_small_one_is_never_thinner_than_three() {
        let mut sk = Sankey::new(0.001);
        sk.node("empty", 0.0, 0.0, 10.0, 0.0, "tickets");
        sk.node("tiny", 20.0, 0.0, 10.0, 1.0, "plans");
        sk.draw();
        assert_eq!(sk.canvas.rects.matches("<rect").count(), 1);
        assert!(sk.canvas.rects.contains("class=\"n-plans\"") && sk.canvas.rects.contains("height=\"3\""), "{}", sk.canvas.rects);
        assert_eq!(sk.h(0.0), 0.0);
        assert_eq!(sk.h(-5.0), 0.0);
        assert!(sk.get("empty").is_some() && sk.get("nowhere").is_none());
    }

    #[test]
    fn an_empty_chart_says_how_to_get_one() {
        let svg = empty_svg("Add some receipts", 200.0);
        assert!(svg.starts_with("<svg ") && svg.ends_with("</svg>") && svg.contains(">Add some receipts</text>"), "{svg}");
        assert!(svg.contains("viewBox=\"0 0 1400 200\""));
    }

    #[test]
    fn text_turned_up_carries_its_marker_and_its_centre() {
        let mut c = Canvas::default();
        c.text_up(20.0, 100.0, "Gross", 18.0, 700, "c-ongross");
        assert!(c.texts.contains("rotate(-90 20 100)") && c.texts.contains("data-up=\"1\"") && c.texts.contains("class=\"c-ongross w7\""), "{}", c.texts);
    }
}
