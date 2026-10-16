// =============================================================================
// Eustress Web - Economics DRAFT charts (/economics-draft, feature `economics-draft`)
// =============================================================================
// The two money-flow diagrams of the internal draft page, drawn as SVG text from the draft model (economics_draft_model.rs): a month of
// Eustress, and what one creator gets. Each is a Sankey: nodes are boxes, flows are ribbons as thick as the money in them. The drawing is pure
// (numbers in, markup out), so tests read the markup, and the page sets it into a div.
//
// Colors are not set here. Every node, ribbon and word carries a class (`n-tickets`, `l-plans`, `c-reserve`); the page's stylesheet
// (style/gallery.css, ".eco-svg") gives each class its color from the Eustress palette. Everything printed on a chart is a number
// from the model or a fixed phrase, escaped; nothing a visitor types reaches the markup.
//
// Table of Contents:
// 1. A month of Eustress
// 2. One creator
// 3. Figures and notes for the page's HTML
// 4. Tests
// =============================================================================

use super::economics_draft_model::{cents_words, company, creator, dollars, Basis, Channel, Company, Creator, Inputs, Unit, PRESETS};

use super::economics_kit::{empty_svg, fmt, num, pct, Sankey, A};

// -----------------------------------------------------------------------------
// 1. A month of Eustress
// -----------------------------------------------------------------------------

/// The month as a Sankey: receipts, the fee, the two halves, the reserve and costs, then profit and tax.
pub fn company_svg(s: &Inputs, m: &Company) -> String {
    if !(m.gross > 0.0) {
        return empty_svg("Add some receipts with the sliders", 240.0);
    }
    let d = &m.shown;
    let mut sk = Sankey::new(330.0 / (m.gross + s.invest));
    let (x0, x1, x2, x3, x4) = (215.0, 425.0, 640.0, 860.0, 1075.0);
    let (w, gw, t0, gap) = (18.0, 34.0, 70.0, 84.0);

    // Each source takes the taller of its ribbon and its label, so the labels never collide.
    let srcs: Vec<(&'static str, f64, &'static str)> =
        [("tickets", s.tickets, "Ticket packs"), ("plans", m.plan_revenue, "Plan subscriptions"), ("delta", s.delta, "Delta packs")]
            .into_iter()
            .filter(|p| p.1 > 0.0)
            .collect();
    let blocks: Vec<f64> = srcs.iter().map(|p| sk.h(p.1).max(if p.0 == "plans" { 64.0 } else { 46.0 })).collect();
    let stack = 16.0 * srcs.len().saturating_sub(1) as f64 + blocks.iter().sum::<f64>();
    let cy_g = 395.0;
    let mut sy = cy_g - stack / 2.0;
    for (i, p) in srcs.iter().enumerate() {
        sk.node(p.0, x0, sy + (blocks[i] - sk.h(p.1)) / 2.0, 16.0, p.1, p.0);
        sy += blocks[i] + 16.0;
    }
    sk.node("gross", x1, cy_g - sk.h(m.gross) / 2.0, gw, m.gross, "gross");
    sk.node("invest", x1, t0, 16.0, s.invest, "invest");

    let ht = sk.h(m.treasury_in);
    let he = sk.h(m.eustress);
    let e0 = t0 + ht + 120.0;
    sk.node("treasury", x2, t0, w, m.treasury_in, "treasury");
    sk.node("eustress", x2, e0, w, m.eustress, "eustress");
    sk.node("fees", x2, e0 + he + 100.0, w, m.fees, "fees");
    sk.node("drip", x3, t0, w, m.drip, "drip");
    let y_inv = t0 + sk.h(m.drip).max(112.0) + 30.0;
    sk.node("toinv", x3, y_inv, w, m.paid_investors, "toinv");
    let mut y = (e0 - 20.0).max(y_inv + sk.h(m.paid_investors) + 80.0);
    // the reserve's label is five lines tall, so the next box starts at least 108 below it
    for (id, v, step) in [("reserve", m.reserve, 108.0), ("expenses", m.expenses, gap), ("profit", m.profit, 0.0)] {
        if v > 0.0 {
            sk.node(id, x3, y, w, v, id);
            y += (sk.h(v) + gap).max(step);
        }
    }
    let p_top = sk.get("profit").map(|p| p.y).unwrap_or(y);
    sk.node("taxes", x4, p_top - 10.0, w, m.taxes, "taxes");
    let t_h = sk.h(m.taxes);
    sk.node("kept", x4, p_top - 10.0 + t_h + 92.0, w, m.kept, "kept");

    sk.link("tickets", "gross", s.tickets, "tickets", 0.55);
    sk.link("plans", "gross", m.plan_revenue, "plans", 0.55);
    sk.link("delta", "gross", s.delta, "delta", 0.55);
    sk.link("invest", "treasury", s.invest, "invest", 0.65);
    sk.link("gross", "treasury", m.treasury_share, "treasury", 0.5);
    sk.link("gross", "eustress", m.eustress, "eustress", 0.55);
    sk.link("gross", "fees", m.fees, "fees", 0.5);
    sk.link("treasury", "drip", m.drip, "drip", 0.5);
    sk.link("treasury", "toinv", m.paid_investors, "toinv", 0.6);
    sk.link("eustress", "reserve", m.reserve_packs, "tickets", 0.65);
    sk.link("eustress", "reserve", m.reserve_plans, "plans", 0.75);
    sk.link("eustress", "expenses", m.expenses, "expenses", 0.5);
    sk.link("eustress", "profit", m.profit, "profit", 0.55);
    sk.link("profit", "taxes", m.taxes, "taxes", 0.6);
    sk.link("profit", "kept", m.kept, "kept", 0.55);
    sk.draw();

    // Gross receipts, written inside its box when the box is tall enough, above it when it is not.
    let (gx, gy, gh) = {
        let g = sk.get("gross").expect("gross exists");
        (g.x + g.w / 2.0, g.y + g.h / 2.0, g.h)
    };
    let gross_words = format!("Gross receipts {}", dollars(d.gross));
    if gh >= 230.0 {
        sk.canvas.text_up(gx, gy, &gross_words, (gh / 12.5).min(20.0), 700, "c-ongross");
    } else {
        let top = sk.get("gross").map(|g| g.y).unwrap_or(0.0);
        sk.canvas.text(gx, top - 12.0, &gross_words, 17.0, 700, "c-1", A::Middle);
    }

    for p in &srcs {
        let (nx, ny, nh) = {
            let node = sk.get(p.0).expect("source exists");
            (node.x, node.y, node.h)
        };
        sk.canvas.text(nx - 14.0, ny + nh / 2.0 - 6.0, p.2, 19.0, 700, &format!("c-{}", p.0), A::End);
        sk.canvas.text(nx - 14.0, ny + nh / 2.0 + 15.0, &format!("{} a month", fmt(p.1)), 15.0, 400, "c-2", A::End);
        if p.0 == "plans" {
            sk.canvas.text(nx - 14.0, ny + nh / 2.0 + 33.0, &format!("{} subscribers", num(m.subscribers)), 14.0, 400, "c-2", A::End);
        }
    }
    if s.invest > 0.0 {
        let iy = sk.get("invest").map(|v| v.y).unwrap_or(t0);
        sk.canvas.text(x1 - 14.0, iy + 8.0, "Treasury investments", 18.0, 700, "c-invest", A::End);
        sk.canvas.text(x1 - 14.0, iy + 28.0, &format!("{} a month, capital not revenue", fmt(s.invest)), 14.0, 400, "c-2", A::End);
    }
    let tc = x2 + w / 2.0;
    sk.canvas.text(tc, t0 - 44.0, "Bliss Treasury", 21.0, 700, "c-treasury", A::Middle);
    let invested = if s.invest > 0.0 { format!(" + {} invested", fmt(s.invest)) } else { String::new() };
    sk.canvas.text(tc, t0 - 22.0, &format!("{} in: {} from sales{invested}", dollars(d.treasury_in), dollars(d.treasury)), 14.0, 400, "c-treasury", A::Middle);
    sk.canvas.text(x2, e0 - 44.0, "Eustress share", 19.0, 700, "c-eustress", A::Start);
    sk.canvas.text(x2, e0 - 22.0, &format!("{}  (50% of net)", dollars(d.eustress)), 15.0, 400, "c-eustress", A::Start);
    {
        let (fx, fy) = {
            let f = sk.get("fees").expect("fees exists");
            (f.x + w, f.y + f.h.max(3.0) + 26.0)
        };
        let channel = if s.channel == Channel::Web { "Stripe 2.9% + $0.30" } else { "App store 30% on Ticket packs" };
        sk.canvas.text(fx, fy, &format!("Payment fees {}", dollars(d.fees)), 19.0, 700, "c-fees", A::End);
        sk.canvas.text(fx, fy + 21.0, &format!("{channel}, {} of gross", pct(m.fees, m.gross)), 14.0, 400, "c-2", A::End);
    }

    let lx = x3 + w + 14.0;
    let tax_note = if s.basis == Basis::Funded { "an expense for tax when funded" } else { "an expense for tax only when paid out" };
    let drip_words = if m.drip > 0.0 { format!("{} a month", dollars(d.drip)) } else { format!("{} a month: repayments take the whole inflow", dollars(d.drip)) };
    sk.canvas.text(lx, t0 + 24.0, "Paid to contributors", 20.0, 700, "c-drip", A::Start);
    sk.canvas.text(lx, t0 + 46.0, &drip_words, 16.0, 600, "c-drip", A::Start);
    sk.canvas.text(lx, t0 + 66.0, "from the Bliss Treasury account", 14.0, 400, "c-2", A::Start);
    sk.canvas.text(lx, t0 + 84.0, "the Bliss line of a creator's payout", 14.0, 400, "c-2", A::Start);
    sk.canvas.text(lx, t0 + 102.0, &format!("counted as {tax_note}"), 14.0, 400, "c-2", A::Start);
    if m.paid_investors > 0.0 {
        let iy = sk.get("toinv").map(|v| v.y).unwrap_or(y_inv);
        sk.canvas.text(lx, iy + 10.0, &format!("Paid to investors  {}", dollars(d.paid_investors)), 17.0, 700, "c-toinv", A::Start);
        let ret = if d.paid_return > 0 { format!(" + return {}", dollars(d.paid_return)) } else { ", no return".to_string() };
        sk.canvas.text(lx, iy + 30.0, &format!("principal {}{ret} (not an expense)", dollars(d.paid_principal)), 14.0, 400, "c-2", A::Start);
    }

    // A label for a box on the right: a title, a figure, then lines of detail.
    let label = |sk: &mut Sankey, id: &str, lines: &[String], tone: &str| {
        let (ny, nh) = match sk.get(id) {
            Some(node) => (node.y, node.h),
            None => return,
        };
        let yy = ny + (nh / 2.0).max(8.0);
        sk.canvas.text(lx, yy - 20.0, &lines[0], 20.0, 700, tone, A::Start);
        sk.canvas.text(lx, yy + 2.0, &lines[1], 16.0, 600, tone, A::Start);
        for (i, line) in lines.iter().enumerate().skip(2) {
            sk.canvas.text(lx, yy + 2.0 + (i as f64 - 1.0) * 19.0, line, 14.0, 400, "c-2", A::Start);
        }
    };
    let coverage = if m.owed > 0.0 {
        format!("cash-out owed at ${:.2} per 1,000: {}  ({}% covered)", s.rate, fmt(m.owed), (100.0 * m.reserve / m.owed).round() as i64)
    } else {
        "no cash-out owed at these settings".to_string()
    };
    label(
        &mut sk,
        "reserve",
        &[
            "Ticket Sales Reserve".to_string(),
            format!("{}  \u{b7}  {} of the Ticket half", dollars(d.reserve), Unit::Percent.show(s.reserve)),
            format!("packs {} + plan invoices {}", dollars(d.reserve_packs), dollars(d.reserve_plans)),
            "its own account; pays the Tickets cash-out".to_string(),
            coverage,
        ],
        "c-reserve",
    );
    let short = if m.shortfall > 0.0 { format!("short by {}: Eustress share is not enough", fmt(m.shortfall)) } else { "its own account".to_string() };
    label(
        &mut sk,
        "expenses",
        &["Engine Expenses".to_string(), format!("{}  \u{b7}  {} of gross", dollars(d.expenses), pct(m.expenses, m.gross)), short, "servers, Cloudflare, refunds, etc.".to_string()],
        "c-expenses",
    );
    let profit_box = sk.get("profit").filter(|p| p.v > 0.0).map(|p| (p.x + w, p.y + p.h + 24.0));
    match profit_box {
        Some((px, py)) => sk.canvas.text(px, py, &format!("Operating profit  {}", dollars(d.profit)), 17.0, 700, "c-profit", A::End),
        None => sk.canvas.text(lx, y + 4.0, "Operating profit  $0: no tax is due", 17.0, 700, "c-profit", A::Start),
    }
    let tx = x4 + w + 14.0;
    if m.profit > 0.0 {
        let ty = sk.get("taxes").map(|t| t.y).unwrap_or(0.0);
        let over = m.tax_due > m.taxes + 0.5;
        sk.canvas.text(tx, ty + 8.0, &format!("Taxes  {}", dollars(d.taxes)), 20.0, 700, "c-taxes", A::Start);
        sk.canvas.text(tx, ty + 28.0, &format!("{} of {} taxable", Unit::Percent.show(s.tax), dollars(d.taxable)), 14.0, 400, "c-2", A::Start);
        sk.canvas.text(tx, ty + 46.0, if s.basis == Basis::Funded { "Treasury and reserve deducted" } else { "only the payout is deducted" }, 14.0, 400, "c-2", A::Start);
        if over {
            sk.canvas.text(tx, ty + 64.0, &format!("tax due {} is more than the profit", dollars(d.tax_due)), 14.0, 400, "c-bad", A::Start);
        } else {
            sk.canvas.text(tx, ty + 64.0, "its own account", 14.0, 400, "c-2", A::Start);
        }
        let alt = if s.basis == Basis::Funded { "if deducted only when paid out: " } else { "if deducted when funded: " };
        sk.canvas.text(tx, ty + 82.0, &format!("{alt}{}", dollars(d.alt_due)), 14.0, 600, "c-taxes", A::Start);
        let ky = sk.get("kept").map(|k| k.y).unwrap_or(0.0);
        sk.canvas.text(tx, ky + 8.0, &format!("Net profit kept  {}", dollars(d.kept)), 20.0, 700, "c-kept", A::Start);
        sk.canvas.text(tx, ky + 28.0, "kept by Eustress after tax", 14.0, 400, "c-2", A::Start);
    }

    // The drawing ends below its lowest box and that box's label.
    let mut bottom = y + 30.0;
    for (id, below) in [("kept", 44.0), ("taxes", 24.0), ("profit", 34.0), ("expenses", 50.0), ("reserve", 90.0), ("fees", 56.0), ("toinv", 40.0)] {
        if let Some(node) = sk.get(id) {
            if node.v > 0.0 || (id == "kept" && m.profit > 0.0) {
                bottom = bottom.max(node.y + node.h + below);
            }
        }
    }
    let title = format!("A month of Eustress: {} in, {} to contributors, {} kept", dollars(d.gross), dollars(d.drip), dollars(d.kept));
    let desc = format!(
        "Gross receipts {} less payment fees {} is net {}. The Bliss Treasury takes {} and Eustress {}. Eustress pays the reserve {} and Engine Expenses {}, leaving operating profit {}; taxes are {}.",
        dollars(d.gross),
        dollars(d.fees),
        dollars(d.net),
        dollars(d.treasury),
        dollars(d.eustress),
        dollars(d.reserve),
        dollars(d.expenses),
        dollars(d.profit),
        dollars(d.taxes)
    );
    sk.canvas.finish(bottom + 20.0, &title, &desc)
}

// -----------------------------------------------------------------------------
// 2. One creator
// -----------------------------------------------------------------------------

/// What reaches one creator: the Bliss share from hours and from sales, and the sale's 70% split into cash, what is given up by cashing
/// out, and Tickets left to spend.
pub fn creator_svg(s: &Inputs, c: &Creator) -> String {
    let tot = c.bliss + c.spend_usd;
    if !(tot > 0.0) {
        return empty_svg("Add players, buyers or hours with the sliders", 240.0);
    }
    let k = &c.shown;
    let share = s.creator_share;
    let mut sk = Sankey::new(280.0 / tot);
    let (x0, x1, x2, w, top) = (330.0, 640.0, 960.0, 18.0, 40.0);
    // Each source takes the taller of its ribbon and its three-line label, so the labels never collide.
    let lh = 60.0;
    let bh = sk.h(c.bliss_effort).max(lh);
    let bs = sk.h(c.bliss_value).max(lh);
    sk.node("hours", x0, top + (bh - sk.h(c.bliss_effort)) / 2.0, 16.0, c.bliss_effort, "drip");
    let ys = top + bh + 20.0;
    sk.node("sales", x0, ys + (bs - sk.h(c.bliss_value)) / 2.0, 16.0, c.bliss_value, "drip");
    let yb = ys + bs + 70.0 + (sk.h(c.spend_usd).max(lh) - sk.h(c.spend_usd)) / 2.0;
    sk.node("spend", x0, yb, 16.0, c.spend_usd, "tickets");
    sk.node("creator", x1, yb, w, c.creator_usd, "tickets");
    let platform_usd = c.spend_usd - c.creator_usd;
    let platform_y = yb + sk.h(c.creator_usd) + 44.0;
    sk.node("platform", x1, platform_y, w, platform_usd, "gray");
    sk.node("cash", x2, top, w, c.cash_total, "cash");
    let ysp = top + sk.h(c.cash_total) + 64.0;
    sk.node("spendable", x2, ysp, w, c.spendable, "tickets");
    let ydi = ysp + sk.h(c.spendable) + 54.0;
    sk.node("discount", x2, ydi, w, c.discount, "bad");
    sk.link("hours", "cash", c.bliss_effort, "drip", 0.55);
    sk.link("sales", "cash", c.bliss_value, "drip", 0.4);
    sk.link("creator", "cash", c.cash_usd, "tickets", 0.6);
    sk.link("spend", "creator", c.creator_usd, "tickets", 0.55);
    sk.link("spend", "platform", platform_usd, "gray", 0.5);
    sk.link("creator", "spendable", c.spendable, "tickets", 0.4);
    sk.link("creator", "discount", c.discount, "bad", 0.5);
    sk.draw();

    let left = |sk: &mut Sankey, id: &str, t1: &str, t2: String, t3: String, tone: &str| {
        let (nx, ny, nh, nv) = match sk.get(id) {
            Some(node) => (node.x, node.y, node.h, node.v),
            None => return,
        };
        if !(nv > 0.0) {
            return;
        }
        let yy = ny + (nh / 2.0).max(8.0);
        sk.canvas.text(nx - 14.0, yy - 6.0, t1, 19.0, 700, tone, A::End);
        sk.canvas.text(nx - 14.0, yy + 15.0, &t2, 14.0, 400, "c-2", A::End);
        if !t3.is_empty() {
            sk.canvas.text(nx - 14.0, yy + 33.0, &t3, 14.0, 400, "c-2", A::End);
        }
    };
    left(&mut sk, "hours", "Hours in Studio", format!("{} h = {} effort score", num(s.hours), num(c.effort)), format!("pays {} of the Bliss drip", cents_words(k.effort)), "c-drip");
    left(&mut sk, "sales", "Sales score", format!("{} from {} paid Tickets", num(c.value_score), num(c.counted)), format!("pays {} of the Bliss drip", cents_words(k.value)), "c-drip");
    left(&mut sk, "spend", "Buyers' spend", format!("{} Tickets, {} bought with money", num(c.spent), Unit::Percent.show(s.paid)), format!("about {} at the pack price", cents_words(k.spend)), "c-tickets");

    let cc = x1 + w / 2.0;
    let creator_y = sk.get("creator").map(|v| v.y).unwrap_or(yb);
    if c.creator_usd > 0.0 {
        sk.canvas.text(cc, creator_y - 36.0, &format!("Creator earns {}%", Unit::Percent.show(share).trim_end_matches('%')), 17.0, 700, "c-tickets", A::Middle);
        sk.canvas.text(cc, creator_y - 16.0, &format!("{} Tickets", num(c.earned)), 14.0, 400, "c-2", A::Middle);
    }
    if platform_usd > 0.0 {
        sk.canvas.text(x1 + w + 12.0, platform_y + 8.0, &format!("Platform keeps {}%", Unit::Percent.show(100.0 - share).trim_end_matches('%')), 17.0, 700, "c-gray", A::Start);
        sk.canvas.text(x1 + w + 12.0, platform_y + 28.0, "out of circulation", 14.0, 400, "c-2", A::Start);
    }
    let rx = x2 + w + 14.0;
    sk.canvas.text(rx, top + 6.0, &format!("Cash to the creator  {}", cents_words(k.total)), 22.0, 700, "c-cash", A::Start);
    sk.canvas.text(rx, top + 29.0, &format!("Tickets cash-out {} + Bliss drip {}", cents_words(k.cash), cents_words(k.bliss)), 15.0, 600, "c-cash", A::Start);
    sk.canvas.text(rx, top + 49.0, &format!("about {} a year, one payout statement, two lines", dollars(((k.total * 12) as f64 / 100.0).round() as i64)), 14.0, 400, "c-2", A::Start);
    if c.spendable > 0.0 {
        sk.canvas.text(rx, ysp + 6.0, &format!("Spendable in Eustress  {}", cents_words(k.spendable)), 17.0, 700, "c-tickets", A::Start);
        sk.canvas.text(rx, ysp + 26.0, "Tickets not cashed out, at the pack price", 14.0, 400, "c-2", A::Start);
    }
    if c.discount > 0.0 {
        sk.canvas.text(rx, ydi + 6.0, &format!("Cash-out discount  {}", cents_words(k.discount)), 17.0, 700, "c-bad", A::Start);
        sk.canvas.text(rx, ydi + 26.0, "face value of the cashed-out Tickets, less the cash paid", 14.0, 400, "c-2", A::Start);
    }

    let mut bottom = 0.0f64;
    for (id, v) in [("spend", c.spend_usd), ("platform", platform_usd), ("discount", c.discount), ("spendable", c.spendable), ("cash", c.cash_total)] {
        if v > 0.0 {
            if let Some(node) = sk.get(id) {
                bottom = bottom.max(node.y + node.h.max(36.0) + 20.0);
            }
        }
    }
    let title = format!("What one creator gets in a month: {} in cash", cents_words(k.total));
    let desc = format!(
        "Tickets cash-out {} plus a Bliss drip share of {}. The creator keeps {}% of {} spent; {} stays spendable in Eustress.",
        cents_words(k.cash),
        cents_words(k.bliss),
        Unit::Percent.show(share).trim_end_matches('%'),
        cents_words(k.spend),
        cents_words(k.spendable)
    );
    sk.canvas.finish(bottom + 20.0, &title, &desc)
}

// -----------------------------------------------------------------------------
// 3. Figures and notes for the page's HTML
// -----------------------------------------------------------------------------

/// One line of a table under a chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: &'static str,
    pub value: String,
    pub note: String,
    /// A subtotal the eye should find: net, operating profit, profit kept.
    pub strong: bool,
}

fn row(label: &'static str, value: String, note: impl Into<String>, strong: bool) -> Row {
    Row { label, value, note: note.into(), strong }
}

/// A month as an income statement, in the order the money moves. Printed from the same whole dollars as the chart.
pub fn company_rows(s: &Inputs, m: &Company) -> Vec<Row> {
    let d = &m.shown;
    let fee_note = if s.channel == Channel::Web { "Stripe 2.9% + $0.30 on every payment" } else { "Stripe on plans and Delta, the app store's 30% on Ticket packs" };
    let tax_note = format!(
        "{} of {} taxable; {}",
        Unit::Percent.show(s.tax),
        dollars(d.taxable),
        if s.basis == Basis::Funded { "Treasury and reserve deducted when funded" } else { "only what is paid out is deducted" }
    );
    vec![
        row("Gross receipts", dollars(d.gross), "Ticket packs, plans and Delta packs", false),
        row("Payment fees", format!("-{}", dollars(d.fees)), fee_note, false),
        row("Net", dollars(d.net), "what is left to split", true),
        row("Bliss Treasury", format!("-{}", dollars(d.treasury)), "half of net, paid to contributors by score", false),
        row("Eustress share", dollars(d.eustress), "the other half", true),
        row("Ticket Sales Reserve", format!("-{}", dollars(d.reserve)), format!("{} from packs, {} from the Tickets share of plan invoices", dollars(d.reserve_packs), dollars(d.reserve_plans)), false),
        row("Engine Expenses", format!("-{}", dollars(d.expenses)), if m.shortfall > 0.0 { format!("{} short of the {} asked", dollars(m.shortfall.round() as i64), dollars(s.expenses.round() as i64)) } else { String::new() }, false),
        row("Operating profit", dollars(d.profit), "", true),
        row("Taxes", format!("-{}", dollars(d.taxes)), tax_note, false),
        row("Net profit kept", dollars(d.kept), "", true),
    ]
}

/// What the Treasury does with the month, as lines under the income statement.
pub fn treasury_rows(s: &Inputs, m: &Company) -> Vec<Row> {
    let d = &m.shown;
    let mut rows = vec![
        row("Into the Bliss Treasury", dollars(d.treasury_in), format!("{} from sales and {} invested", dollars(d.treasury), dollars(s.invest.round() as i64)), false),
        row("Paid to investors", format!("-{}", dollars(d.paid_investors)), format!("principal {}, return {}", dollars(d.paid_principal), dollars(d.paid_return)), false),
        row("Paid to contributors", format!("-{}", dollars(d.drip)), "the Bliss line, shared by score", true),
    ];
    if m.owed > 0.0 {
        rows.push(row(
            "Tickets cash-out the reserve must cover",
            dollars(m.owed.round() as i64),
            format!("at ${:.2} per 1,000 Tickets if {} of bought Tickets are earned by creators and cashed out; the reserve covers {}%", s.rate, Unit::Percent.show(s.through), (100.0 * m.reserve / m.owed).round() as i64),
            false,
        ));
    }
    rows
}

/// The two sentences that read the income statement aloud, from the printed dollars.
pub fn company_sentences(m: &Company) -> [String; 2] {
    let d = &m.shown;
    [
        format!("Gross {}, less fees {}, is net {}. Less the Treasury {} is Eustress share {}.", dollars(d.gross), dollars(d.fees), dollars(d.net), dollars(d.treasury), dollars(d.eustress)),
        format!(
            "Less the reserve {} and Engine Expenses {} is operating profit {}. Less taxes {} leaves {}.",
            dollars(d.reserve),
            dollars(d.expenses),
            dollars(d.profit),
            dollars(d.taxes),
            dollars(d.kept)
        ),
    ]
}

/// How a creator's month was reached, from the players to the Tickets spent.
pub fn creator_funnel(s: &Inputs, c: &Creator) -> String {
    let lift = if s.updates > 0.0 && s.lift > 0.0 { format!(" + {} from {} updates", num(c.players - s.players), num(s.updates)) } else { String::new() };
    format!("{} players{lift}, {} buy = {} buyers x {} Tickets = {} Tickets spent", num(s.players), Unit::Percent.show(s.conv), num(c.buyers), num(s.avg), num(c.spent))
}

/// A creator's month as a statement: two lines that never mix, and what stays spendable.
pub fn creator_rows(s: &Inputs, c: &Creator) -> Vec<Row> {
    let k = &c.shown;
    let cents_per_dollar = if c.paid_usd > 0.0 { format!("{:.0}", 100.0 * c.cash_usd / c.paid_usd) } else { "0".to_string() };
    vec![
        row("Bliss line: hours in Studio", cents_words(k.effort), format!("{} h = {} effort score", num(s.hours), num(c.effort)), false),
        row("Bliss line: sales score", cents_words(k.value), format!("{} score from {} Tickets bought with money", num(c.value_score), num(c.counted)), false),
        row("Bliss line", cents_words(k.bliss), format!("{} of {} score this month, a {} share of the drip", num(c.mine), num(c.total), pct(c.mine, c.total)), true),
        row("Tickets line: cash-out", cents_words(k.cash), format!("{} Tickets of credit, {} cashed out at ${:.2} per 1,000: {cents_per_dollar} cents for each dollar buyers paid", num(c.eligible), Unit::Percent.show(s.cash), s.rate), false),
        row("Cash a month", cents_words(k.total), format!("about {} a year", dollars(((k.total * 12) as f64 / 100.0).round() as i64)), true),
        row("Spendable in Eustress", cents_words(k.spendable), "Tickets not cashed out, at the pack price", false),
        row("Cash-out discount", cents_words(k.discount), "face value of the cashed-out Tickets, less the cash paid", false),
    ]
}

/// The lines that explain a creator's numbers: how the score is made and what the drip does.
pub fn creator_notes(s: &Inputs, m: &Company, c: &Creator) -> Vec<String> {
    let gate = if c.gate < 1.0 { format!(", scaled to {:.0}% because the network scored under 1,440 a day", c.gate * 100.0) } else { String::new() };
    vec![
        format!(
            "Studio: {} hours is {} effort score (assuming 40% scripting at 3.0, 40% building at 2.5 and 20% other time at 1.0, on a {} node). Sales: {} Tickets earned from sales paid with bought Tickets, at 0.5 each, is {} sales score.",
            num(s.hours),
            num(c.effort),
            s.node.name(),
            num(c.eligible),
            num(c.value_score)
        ),
        format!("Bliss drip: this creator's {} of {} score this month is {}. The Treasury pays out {} a month{gate}, so the share is {}.", num(c.mine), num(c.total), pct(c.mine, c.total), dollars(m.shown.drip), cents_words(c.shown.bliss)),
        format!(
            "Updates earn no score of their own: the scoring has no update category. They matter through the hours they take and the players they bring back ({} each here, an assumption).",
            Unit::Percent.show(s.lift)
        ),
    ]
}

/// The three creators the presets describe, for a table of what each earns on the default network.
pub fn preset_months(base: &Inputs) -> Vec<(&'static str, i64)> {
    PRESETS
        .iter()
        .map(|p| {
            let mut i = base.clone();
            i.apply(p);
            let m = company(&i);
            (p.name, creator(&i, &m).shown.total)
        })
        .collect()
}

// -----------------------------------------------------------------------------
// 4. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::economics_draft_model::{Key, Inputs};
    use super::*;

    fn default_charts() -> (Inputs, Company, Creator, String, String) {
        let s = Inputs::default();
        let m = company(&s);
        let c = creator(&s, &m);
        let a = company_svg(&s, &m);
        let b = creator_svg(&s, &c);
        (s, m, c, a, b)
    }

    fn texts(svg: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = svg;
        while let Some(i) = rest.find("<text ") {
            let after = &rest[i..];
            let start = after.find('>').unwrap() + 1;
            let end = after.find("</text>").unwrap();
            out.push(after[start..end].replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'"));
            rest = &after[end + 7..];
        }
        out
    }

    #[test]
    fn the_default_company_chart_prints_the_dollars_the_model_works_out() {
        let (_, _, _, svg, _) = default_charts();
        let t = texts(&svg);
        for want in [
            "Gross receipts $178,992",
            "Bliss Treasury",
            "$89,375 in: $84,375 from sales + $5,000 invested",
            "Eustress share",
            "$84,376  (50% of net)",
            "Payment fees $10,241",
            "Stripe 2.9% + $0.30, 5.7% of gross",
            "Paid to contributors",
            "$86,375 a month",
            "Paid to investors  $3,000",
            "principal $3,000, no return (not an expense)",
            "Ticket Sales Reserve",
            "$8,121  \u{b7}  15% of the Ticket half",
            "packs $7,057 + plan invoices $1,064",
            "cash-out owed at $1.50 per 1,000: $5,501  (148% covered)",
            "Engine Expenses",
            "$40,000  \u{b7}  22% of gross",
            "Operating profit  $36,255",
            "Taxes  $9,064",
            "25% of $36,255 taxable",
            "if deducted only when paid out: $11,094",
            "Net profit kept  $27,191",
        ] {
            assert!(t.iter().any(|x| x == want), "the chart says {want:?}; it says {t:#?}");
        }
    }

    #[test]
    fn the_default_creator_chart_prints_the_cents_the_model_works_out() {
        let (_, _, _, _, svg) = default_charts();
        let t = texts(&svg);
        for want in [
            "Hours in Studio",
            "60 h = 8,640 effort score",
            "pays $164.95 of the Bliss drip",
            "Sales score",
            "15,592 from 31,185 paid Tickets",
            "pays $297.68 of the Bliss drip",
            "49,500 Tickets, 90% bought with money",
            "about $561.94 at the pack price",
            "Creator earns 70%",
            "34,650 Tickets",
            "Platform keeps 30%",
            "Cash to the creator  $509.41",
            "Tickets cash-out $46.78 + Bliss drip $462.63",
            "about $6,113 a year, one payout statement, two lines",
            "Spendable in Eustress  $39.34",
            "Cash-out discount  $307.24",
        ] {
            assert!(t.iter().any(|x| x == want), "the chart says {want:?}; it says {t:#?}");
        }
    }

    #[test]
    fn a_chart_is_one_svg_with_a_title_and_a_description_and_no_script_or_color() {
        let (_, _, _, a, b) = default_charts();
        for svg in [&a, &b] {
            assert!(svg.starts_with("<svg ") && svg.ends_with("</svg>"));
            assert_eq!(svg.matches("<svg").count(), 1);
            assert!(svg.contains("role=\"img\"") && svg.contains("<title>") && svg.contains("<desc>"));
            assert!(!svg.contains("<script") && !svg.contains("javascript:") && !svg.contains("onload") && !svg.contains("onclick"));
            // the palette lives in the stylesheet: no inline color anywhere in the markup
            assert!(!svg.contains("fill=\"#") && !svg.contains("stroke=\"#") && !svg.contains("style="), "no inline color or style");
            assert!(!svg.contains('\u{2014}'), "no em dash");
        }
    }

    #[test]
    fn every_class_a_chart_uses_is_one_the_stylesheet_defines() {
        // gather the classes from several very different months, so every branch of the drawing is covered
        let mut months = vec![Inputs::default()];
        let mut a = Inputs::default();
        a.set(Key::Tickets, 1000.0);
        a.set(Key::Invest, 100_000.0);
        a.set(Key::Principal, 1_000_000.0);
        a.set(Key::Repay, 30.0);
        a.set(Key::Return, 5.0);
        months.push(a);
        let mut b = Inputs::default();
        b.basis = Basis::PaidOut;
        b.set(Key::Tax, 50.0);
        b.set(Key::Expenses, 60_000.0);
        b.set(Key::Repay, 30.0);
        b.set(Key::Principal, 1_000_000.0);
        b.set(Key::Invest, 0.0);
        months.push(b);
        let mut low = Inputs::default();
        low.set(Key::Tickets, 3000.0);
        low.set(Key::Invest, 0.0);
        low.set(Key::Principal, 0.0);
        months.push(low);
        let mut star = Inputs::default();
        star.apply(&PRESETS[2]);
        star.set(Key::Cash, 100.0);
        star.set(Key::Others, 50.0);
        months.push(star);

        // the shipped stylesheet and the draft's own
        let css = [include_str!("../../style/gallery.css"), include_str!("../../style/economics_draft.css")].concat();
        let mut used = std::collections::BTreeSet::new();
        for s in &months {
            let m = company(s);
            let c = creator(s, &m);
            for svg in [company_svg(s, &m), creator_svg(s, &c)] {
                let mut rest = svg.as_str();
                while let Some(i) = rest.find("class=\"") {
                    let after = &rest[i + 7..];
                    let end = after.find('"').unwrap();
                    for cls in after[..end].split_whitespace() {
                        used.insert(cls.to_string());
                    }
                    rest = &after[end..];
                }
            }
        }
        assert!(used.len() > 25, "the charts use a palette of classes: {used:?}");
        // a class is defined when the stylesheet has `.eco-svg .<class>` followed by something that ends the class name
        let defined = |cls: &str| {
            let needle = format!(".eco-svg .{cls}");
            css.match_indices(&needle).any(|(i, _)| !css[i + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        };
        for cls in used {
            if cls == "eco-svg" {
                continue;
            }
            assert!(defined(&cls), "the stylesheet colors .{cls}");
        }
        assert!(!defined("n-no-such-class") && !defined("n-trea"), "a prefix of a class name is not the class");
    }

    #[test]
    fn the_gross_label_is_turned_up_inside_a_tall_box_and_set_above_a_short_one() {
        let (_, _, _, tall, _) = default_charts();
        assert_eq!(tall.matches("data-up=\"1\"").count(), 1, "a tall box carries its label turned up");
        assert!(tall.contains("rotate(-90 "));
        // a box far shorter than 230 px (a small month beside a large investment) carries it above instead
        let mut s = Inputs::default();
        s.set(Key::Tickets, 10_000.0);
        for k in [Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional] {
            s.set(k, 0.0);
        }
        s.set(Key::Delta, 0.0);
        s.set(Key::Invest, 100_000.0);
        let m = company(&s);
        let short = company_svg(&s, &m);
        assert_eq!(short.matches("data-up=\"1\"").count(), 0, "{short}");
        assert!(texts(&short).iter().any(|t| t.starts_with("Gross receipts $")));
    }

    #[test]
    fn the_creator_chart_says_the_share_the_slider_sets() {
        let mut s = Inputs::default();
        s.set(Key::CreatorShare, 60.0);
        let m = company(&s);
        let c = creator(&s, &m);
        let t = texts(&creator_svg(&s, &c));
        assert!(t.iter().any(|x| x == "Creator earns 60%"), "{t:#?}");
        assert!(t.iter().any(|x| x == "Platform keeps 40%"), "{t:#?}");
        assert!(!t.iter().any(|x| x == "Creator earns 70%"));
    }

    #[test]
    fn a_month_with_no_receipts_says_how_to_get_a_chart() {
        let mut z = Inputs::default();
        for k in [Key::Tickets, Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            z.set(k, 0.0);
        }
        let m = company(&z);
        let svg = company_svg(&z, &m);
        assert!(texts(&svg).iter().any(|t| t.contains("Add some receipts")), "{svg}");
        let mut none = Inputs::default();
        for k in [Key::Players, Key::Hours, Key::Others] {
            none.set(k, 0.0);
        }
        let m2 = company(&none);
        // the Treasury still drips, but a creator with no players and no hours gets nothing
        let c = creator(&none, &m2);
        assert!(c.cash_total == 0.0);
        assert!(texts(&creator_svg(&none, &c)).iter().any(|t| t.contains("Add players")));
    }

    #[test]
    fn labels_for_boxes_that_hold_no_money_are_not_drawn() {
        let mut low = Inputs::default();
        low.set(Key::Tickets, 3000.0);
        low.set(Key::Invest, 0.0);
        low.set(Key::Principal, 0.0);
        let m = company(&low);
        let t = texts(&company_svg(&low, &m));
        assert!(t.iter().any(|x| x == "Operating profit  $0: no tax is due"));
        assert!(!t.iter().any(|x| x.starts_with("Taxes") || x.starts_with("Net profit kept")), "a month with no profit prints no tax: {t:#?}");
        assert!(!t.iter().any(|x| x == "Treasury investments"), "no investment, no investment label");
        assert!(!t.iter().any(|x| x.starts_with("Paid to investors")));
        assert!(t.iter().any(|x| x.starts_with("short by ")), "the expenses label says how short it is");
    }

    #[test]
    fn repayments_that_take_the_whole_inflow_say_so() {
        let mut s = Inputs::default();
        s.set(Key::Repay, 30.0);
        s.set(Key::Principal, 1_000_000.0);
        s.set(Key::Invest, 0.0);
        let m = company(&s);
        assert_eq!(m.drip, 0.0);
        let t = texts(&company_svg(&s, &m));
        assert!(t.iter().any(|x| x == "$0 a month: repayments take the whole inflow"), "{t:#?}");
    }

    #[test]
    fn the_paid_out_basis_and_a_tax_bill_bigger_than_the_profit_are_said_plainly() {
        let mut s = Inputs::default();
        s.basis = Basis::PaidOut;
        s.set(Key::Tax, 50.0);
        s.set(Key::Repay, 30.0);
        s.set(Key::Principal, 1_000_000.0);
        s.set(Key::Invest, 0.0);
        let m = company(&s);
        let t = texts(&company_svg(&s, &m));
        assert!(t.iter().any(|x| x == "only the payout is deducted"));
        assert!(t.iter().any(|x| x.starts_with("tax due ") && x.ends_with(" is more than the profit")), "{t:#?}");
        assert!(t.iter().any(|x| x.starts_with("if deducted when funded: ")));
        assert!(t.iter().any(|x| x == "an expense for tax only when paid out") || t.iter().any(|x| x == "counted as an expense for tax only when paid out"));
    }

    #[test]
    fn the_chart_grows_to_hold_its_boxes_and_never_shrinks_below_them() {
        // the declared height always reaches below the lowest word that is drawn
        fn lowest(svg: &str) -> f64 {
            let mut max = 0.0f64;
            let mut rest = svg;
            while let Some(i) = rest.find("<text ") {
                let after = &rest[i..];
                let y = after.split("y=\"").nth(1).and_then(|t| t.split('"').next()).and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0);
                max = max.max(y);
                rest = &after[6..];
            }
            max
        }
        fn height(svg: &str) -> f64 {
            svg.split("viewBox=\"0 0 1400 ").nth(1).and_then(|t| t.split('"').next()).and_then(|t| t.parse().ok()).unwrap()
        }
        let mut months = vec![Inputs::default()];
        let mut huge = Inputs::default();
        for k in [Key::Tickets, Key::Delta, Key::Expenses] {
            huge.set(k, 300_000.0);
        }
        huge.set(Key::Invest, 100_000.0);
        months.push(huge);
        let mut taxy = Inputs::default();
        taxy.basis = Basis::PaidOut;
        taxy.set(Key::Expenses, 60_000.0);
        months.push(taxy);
        let mut star = Inputs::default();
        star.apply(&PRESETS[2]);
        months.push(star);
        for s in &months {
            let m = company(s);
            let c = creator(s, &m);
            for svg in [company_svg(s, &m), creator_svg(s, &c)] {
                assert!(height(&svg) > lowest(&svg), "the height {} reaches the lowest word at {}", height(&svg), lowest(&svg));
            }
        }
    }

    #[test]
    fn the_income_statement_adds_up_line_by_line() {
        let (s, m, _, _, _) = default_charts();
        let rows = company_rows(&s, &m);
        let value = |label: &str| -> i64 {
            let r = rows.iter().find(|r| r.label == label).unwrap_or_else(|| panic!("a row named {label}"));
            r.value.replace(['$', ','], "").parse().unwrap()
        };
        assert_eq!(value("Gross receipts") + value("Payment fees"), value("Net"));
        assert_eq!(value("Net") + value("Bliss Treasury"), value("Eustress share"));
        assert_eq!(value("Eustress share") + value("Ticket Sales Reserve") + value("Engine Expenses"), value("Operating profit"));
        assert_eq!(value("Operating profit") + value("Taxes"), value("Net profit kept"));
        assert_eq!(rows.iter().filter(|r| r.strong).count(), 4);
    }

    #[test]
    fn the_sentences_and_rows_print_the_same_dollars_as_the_chart() {
        let (_, m, _, _, _) = default_charts();
        let [one, two] = company_sentences(&m);
        assert_eq!(one, "Gross $178,992, less fees $10,241, is net $168,751. Less the Treasury $84,375 is Eustress share $84,376.");
        assert_eq!(two, "Less the reserve $8,121 and Engine Expenses $40,000 is operating profit $36,255. Less taxes $9,064 leaves $27,191.");
    }

    #[test]
    fn the_creator_statement_has_two_lines_that_never_mix_and_adds_to_the_cent() {
        let (s, _, c, _, _) = default_charts();
        let rows = creator_rows(&s, &c);
        let get = |label: &str| rows.iter().find(|r| r.label == label).unwrap_or_else(|| panic!("a row named {label}")).value.clone();
        assert_eq!(get("Bliss line: hours in Studio"), "$164.95");
        assert_eq!(get("Bliss line: sales score"), "$297.68");
        assert_eq!(get("Bliss line"), "$462.63");
        assert_eq!(get("Tickets line: cash-out"), "$46.78");
        assert_eq!(get("Cash a month"), "$509.41");
        assert_eq!(get("Spendable in Eustress"), "$39.34");
        assert_eq!(get("Cash-out discount"), "$307.24");
        let note = rows.iter().find(|r| r.label == "Tickets line: cash-out").unwrap().note.clone();
        assert!(note.contains("9 cents for each dollar buyers paid"), "{note}");
    }

    #[test]
    fn the_funnel_and_notes_read_the_default_creator() {
        let (s, m, c, _, _) = default_charts();
        assert_eq!(creator_funnel(&s, &c), "5,000 players + 500 from 2 updates, 3% buy = 165 buyers x 300 Tickets = 49,500 Tickets spent");
        let notes = creator_notes(&s, &m, &c);
        assert!(notes[0].contains("60 hours is 8,640 effort score") && notes[0].contains("15,592 sales score") && notes[0].contains("Light node"), "{}", notes[0]);
        assert!(notes[1].contains("24,232 of 4,524,232 score") && notes[1].contains("0.5%") && notes[1].contains("$86,375") && notes[1].contains("$462.63"), "{}", notes[1]);
        assert!(!notes[1].contains("scaled to"), "a busy network is not scaled");
        assert!(notes[2].contains("no update category"), "{}", notes[2]);
        // a quiet network is scaled and says why
        let mut q = Inputs::default();
        q.set(Key::Others, 0.0);
        let mq = company(&q);
        let cq = creator(&q, &mq);
        assert!(creator_notes(&q, &mq, &cq)[1].contains("because the network scored under 1,440 a day"));
    }

    #[test]
    fn the_three_presets_earn_in_order_on_the_default_network() {
        let months = preset_months(&Inputs::default());
        assert_eq!(months.iter().map(|m| m.0).collect::<Vec<_>>(), ["Small", "Typical", "Hit"]);
        assert_eq!(months.iter().map(|m| m.1).collect::<Vec<_>>(), [6_624, 50_941, 639_464], "cents a month: $66.24, $509.41 and $6,394.64");
    }

    #[test]
    fn the_treasury_lines_say_what_the_reserve_must_cover() {
        let (s, m, _, _, _) = default_charts();
        let rows = treasury_rows(&s, &m);
        assert_eq!(rows[0].value, "$89,375");
        assert_eq!(rows[1].value, "-$3,000");
        assert_eq!(rows[2].value, "-$86,375");
        let owed = rows.iter().find(|r| r.label.starts_with("Tickets cash-out")).expect("the cash-out row");
        assert!(owed.value == "$5,501" && owed.note.contains("the reserve covers 148%"), "{owed:?}");
    }
}
