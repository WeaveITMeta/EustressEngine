// =============================================================================
// Eustress Web - Economics charts (/economics)
// =============================================================================
// The money-flow diagrams of the Economics page, drawn as SVG text from the model (economics_model.rs): a month of Eustress receipts, what
// a sale pays a creator in Tickets, and a creator's share of the Treasury's payout in dollars. Each is a Sankey: boxes joined by ribbons as
// thick as the money in them (economics_kit.rs). The drawing is pure (numbers in, markup out), so tests read the markup, and the page sets
// it into a div. Colors come from classes the stylesheet defines; nothing is colored here.
//
// Table of Contents:
// 1. A month of Eustress
// 2. One creator: Tickets and Bliss
// 3. Figures and notes for the page's HTML
// 4. Tests
// =============================================================================

use super::economics_kit::{empty_svg, fmt, num, pct, Sankey, A};
use super::economics_model::{cents_words, commas, dollars, Company, Creator, Expenses, Inputs, Unit, DAYS, VALUE_CAP_PER_DAY, VALUE_CAP_TICKETS_PER_BUYER, VALUE_PER_TICKET};

// -----------------------------------------------------------------------------
// 1. A month of Eustress
// -----------------------------------------------------------------------------

/// The month as a Sankey: receipts, the payment fee, then the two halves, and the Treasury's half paid out to contributors.
pub fn company_svg(_s: &Inputs, m: &Company) -> String {
    if !(m.gross > 0.0) {
        return empty_svg("Add some receipts with the sliders", 240.0);
    }
    let d = &m.shown;
    let mut sk = Sankey::new(330.0 / m.gross);
    let (x0, x1, x2, x3) = (215.0, 425.0, 640.0, 860.0);
    let (w, gw, t0) = (18.0, 34.0, 70.0);

    // Each source takes the taller of its ribbon and its label, so the labels never collide.
    let srcs: Vec<(&'static str, f64, &'static str)> =
        [("tickets", m.tickets_usd, "Ticket packs"), ("plans", m.plan_revenue, "Plan subscriptions"), ("delta", m.delta_usd, "Delta packs"), ("gifts", m.gifts_usd, "Gifts to the Treasury")]
            .into_iter()
            .filter(|p| p.1 > 0.0)
            .collect();
    let blocks: Vec<f64> = srcs.iter().map(|p| sk.h(p.1).max(if p.0 == "plans" { 64.0 } else { 46.0 })).collect();
    let stack = 16.0 * srcs.len().saturating_sub(1) as f64 + blocks.iter().sum::<f64>();
    let cy_g = 340.0;
    let mut sy = cy_g - stack / 2.0;
    for (i, p) in srcs.iter().enumerate() {
        sk.node(p.0, x0, sy + (blocks[i] - sk.h(p.1)) / 2.0, 16.0, p.1, p.0);
        sy += blocks[i] + 16.0;
    }
    sk.node("gross", x1, cy_g - sk.h(m.gross) / 2.0, gw, m.gross, "gross");

    let ht = sk.h(m.treasury_share);
    let he = sk.h(m.eustress);
    let e0 = t0 + ht + 90.0;
    sk.node("treasury", x2, t0, w, m.treasury_share, "treasury");
    sk.node("eustress", x2, e0, w, m.eustress, "eustress");
    sk.node("fees", x2, e0 + he + 90.0, w, m.fees, "fees");
    sk.node("drip", x3, t0, w, m.drip, "drip");

    sk.link("tickets", "gross", m.tickets_usd, "tickets", 0.55);
    sk.link("plans", "gross", m.plan_revenue, "plans", 0.55);
    sk.link("delta", "gross", m.delta_usd, "delta", 0.55);
    sk.link("gifts", "gross", m.gifts_usd, "gifts", 0.55);
    sk.link("gross", "treasury", m.treasury_share, "treasury", 0.5);
    sk.link("gross", "eustress", m.eustress, "eustress", 0.55);
    sk.link("gross", "fees", m.fees, "fees", 0.5);
    sk.link("treasury", "drip", m.drip, "drip", 0.5);
    sk.draw();

    // Gross receipts, written inside its box: the ribbons are scaled to the whole month, so the box is always the full 330 px tall.
    let (gx, gy) = {
        let g = sk.get("gross").expect("gross exists");
        (g.x + g.w / 2.0, g.y + g.h / 2.0)
    };
    sk.canvas.text_up(gx, gy, &format!("Gross receipts {}", dollars(d.gross)), 20.0, 700, "c-ongross");

    for p in &srcs {
        let (nx, ny, nh) = {
            let node = sk.get(p.0).expect("source exists");
            (node.x, node.y, node.h)
        };
        sk.canvas.text(nx - 14.0, ny + nh / 2.0 - 6.0, p.2, 19.0, 700, &format!("c-{}", p.0), A::End);
        sk.canvas.text(nx - 14.0, ny + nh / 2.0 + 15.0, &format!("{} {}", fmt(p.1), if m.actual { "in the period" } else { "a month" }), 15.0, 400, "c-2", A::End);
        if p.0 == "plans" {
            let one = m.subscribers.round() == 1.0;
            let what = match (m.actual, one) { (true, true) => "plan invoice", (true, false) => "plan invoices", (false, true) => "subscriber", (false, false) => "subscribers" };
            sk.canvas.text(nx - 14.0, ny + nh / 2.0 + 33.0, &format!("{} {what}", num(m.subscribers)), 14.0, 400, "c-2", A::End);
        }
    }
    let tc = x2 + w / 2.0;
    sk.canvas.text(tc, t0 - 44.0, "Bliss Treasury", 21.0, 700, "c-treasury", A::Middle);
    sk.canvas.text(tc, t0 - 22.0, &format!("{} from sales", dollars(d.treasury)), 14.0, 400, "c-treasury", A::Middle);

    // Eustress's half sits to the right of its box, where nothing else is.
    let (ey, eh) = {
        let e = sk.get("eustress").expect("eustress exists");
        (e.y, e.h)
    };
    let ex = x2 + w + 16.0;
    sk.canvas.text(ex, ey + eh / 2.0 - 12.0, "Eustress's half", 20.0, 700, "c-eustress", A::Start);
    sk.canvas.text(ex, ey + eh / 2.0 + 10.0, &format!("{}  \u{b7}  {} of net", dollars(d.eustress), pct(m.eustress, m.net)), 16.0, 600, "c-eustress", A::Start);
    sk.canvas.text(ex, ey + eh / 2.0 + 30.0, "the other half of every sale, after the fee", 14.0, 400, "c-2", A::Start);

    {
        let (fx, fy) = {
            let f = sk.get("fees").expect("fees exists");
            (f.x + w, f.y + f.h.max(3.0) + 26.0)
        };
        sk.canvas.text(fx, fy, &format!("Payment fees {}", dollars(d.fees)), 19.0, 700, "c-fees", A::End);
        sk.canvas.text(fx, fy + 21.0, &format!("Stripe 2.9% + $0.30, {} of gross", pct(m.fees, m.gross)), 14.0, 400, "c-2", A::End);
    }

    let lx = x3 + w + 14.0;
    sk.canvas.text(lx, t0 + 24.0, "Paid to contributors", 20.0, 700, "c-drip", A::Start);
    if m.actual {
        sk.canvas.text(lx, t0 + 46.0, &format!("{} in the period", fmt(m.drip)), 16.0, 600, "c-drip", A::Start);
        sk.canvas.text(lx, t0 + 66.0, "paid from the Treasury's balance,", 14.0, 400, "c-2", A::Start);
        sk.canvas.text(lx, t0 + 84.0, "0.276% of it a day, shared by score", 14.0, 400, "c-2", A::Start);
    } else {
        sk.canvas.text(lx, t0 + 46.0, &format!("{} a month", dollars(d.treasury)), 16.0, 600, "c-drip", A::Start);
        sk.canvas.text(lx, t0 + 66.0, "the Bliss Treasury pays out what comes in,", 14.0, 400, "c-2", A::Start);
        sk.canvas.text(lx, t0 + 84.0, "shared by score every day", 14.0, 400, "c-2", A::Start);
    }

    // The drawing ends below its lowest box and that box's label.
    let mut bottom = t0 + sk.h(m.drip) + 100.0;
    for (id, below) in [("fees", 56.0), ("eustress", 50.0), ("treasury", 20.0)] {
        if let Some(node) = sk.get(id) {
            if node.v > 0.0 {
                bottom = bottom.max(node.y + node.h + below);
            }
        }
    }
    let title = format!("A month of Eustress: {} in, {} to contributors", dollars(d.gross), dollars(d.treasury));
    let desc = format!(
        "Gross receipts {} less payment fees {} is net {}. The Bliss Treasury takes half, {}, and pays it to contributors. The other half, {}, is Eustress's.",
        dollars(d.gross),
        dollars(d.fees),
        dollars(d.net),
        dollars(d.treasury),
        dollars(d.eustress)
    );
    sk.canvas.finish(bottom + 20.0, &title, &desc)
}

// -----------------------------------------------------------------------------
// 2. One creator: Tickets and Bliss
// -----------------------------------------------------------------------------

/// What a sale pays a creator, in Tickets: the creator's share, and what leaves circulation.
pub fn tickets_svg(_s: &Inputs, c: &Creator) -> String {
    let t = &c.tickets;
    if t.spent <= 0 {
        return empty_svg("Add players or buyers with the sliders", 200.0);
    }
    let mut sk = Sankey::new(220.0 / t.spent as f64);
    let (x0, x1, w, top) = (330.0, 700.0, 18.0, 40.0);
    sk.node("spend", x0, top, 16.0, t.spent as f64, "tickets");
    sk.node("creator", x1, top, w, t.earned as f64, "tickets");
    let platform_y = top + sk.h(t.earned as f64) + 44.0;
    sk.node("platform", x1, platform_y, w, t.platform as f64, "gray");
    sk.link("spend", "creator", t.earned as f64, "tickets", 0.55);
    sk.link("spend", "platform", t.platform as f64, "gray", 0.5);
    sk.draw();

    let (sy, sh) = {
        let n = sk.get("spend").expect("spend exists");
        (n.y, n.h)
    };
    let yy = sy + sh / 2.0;
    sk.canvas.text(x0 - 14.0, yy - 6.0, "Buyers' spend", 19.0, 700, "c-tickets", A::End);
    sk.canvas.text(x0 - 14.0, yy + 15.0, &format!("{} Tickets", commas(t.spent)), 14.0, 400, "c-2", A::End);
    sk.canvas.text(x0 - 14.0, yy + 33.0, &format!("{} bought with money", Unit::Percent.show(c_paid_share(c))), 14.0, 400, "c-2", A::End);

    let rx = x1 + w + 14.0;
    let (cy, ch) = {
        let n = sk.get("creator").expect("creator exists");
        (n.y, n.h)
    };
    sk.canvas.text(rx, cy + ch / 2.0 - 6.0, &format!("Creator earns {}", Unit::Percent.show(100.0 * super::economics_model::CREATOR_SHARE)), 19.0, 700, "c-tickets", A::Start);
    sk.canvas.text(rx, cy + ch / 2.0 + 15.0, &format!("{} Tickets", commas(t.earned)), 16.0, 600, "c-tickets", A::Start);
    sk.canvas.text(rx, cy + ch / 2.0 + 35.0, "to spend inside Eustress", 14.0, 400, "c-2", A::Start);
    if t.platform > 0 {
        sk.canvas.text(rx, platform_y + 8.0, &format!("Platform keeps {}", Unit::Percent.show(100.0 * (1.0 - super::economics_model::CREATOR_SHARE))), 17.0, 700, "c-gray", A::Start);
        sk.canvas.text(rx, platform_y + 28.0, &format!("{} Tickets, out of circulation", commas(t.platform)), 14.0, 400, "c-2", A::Start);
    }

    let bottom = (platform_y + sk.h(t.platform as f64).max(36.0)).max(top + sk.h(t.spent as f64)) + 20.0;
    let title = format!("What a sale pays a creator: {} Tickets earned of {} spent", commas(t.earned), commas(t.spent));
    let desc = format!(
        "Buyers spend {} Tickets on this creator's work. The creator earns {} of them, to spend inside Eustress, and {} leave circulation.",
        commas(t.spent),
        commas(t.earned),
        commas(t.platform)
    );
    sk.canvas.finish(bottom + 20.0, &title, &desc)
}

/// The share of a creator's spend that was bought with money, as a percent. (The model keeps it as `spent_paid` over `spent`.)
fn c_paid_share(c: &Creator) -> f64 {
    if c.spent > 0.0 {
        100.0 * c.spent_paid / c.spent
    } else {
        0.0
    }
}

/// A creator's share of the Treasury's payout, in dollars: what hours in Studio earn and what sales earn.
pub fn bliss_svg(s: &Inputs, c: &Creator) -> String {
    if !(c.bliss > 0.0) {
        return empty_svg("Add hours in Studio or sales with the sliders", 200.0);
    }
    let k = &c.shown;
    let mut sk = Sankey::new(180.0 / c.bliss);
    let (x0, x2, w, top) = (330.0, 960.0, 18.0, 40.0);
    // Each source takes the taller of its ribbon and its three-line label, so the labels never collide.
    let lh = 60.0;
    let bh = sk.h(c.bliss_effort).max(lh);
    let bs = sk.h(c.bliss_value).max(lh);
    sk.node("hours", x0, top + (bh - sk.h(c.bliss_effort)) / 2.0, 16.0, c.bliss_effort, "drip");
    let ys = top + bh + 20.0;
    sk.node("sales", x0, ys + (bs - sk.h(c.bliss_value)) / 2.0, 16.0, c.bliss_value, "drip");
    let payout_y = top + (bh + 20.0 + bs - sk.h(c.bliss)) / 2.0;
    sk.node("payout", x2, payout_y, w, c.bliss, "cash");
    sk.link("hours", "payout", c.bliss_effort, "drip", 0.55);
    sk.link("sales", "payout", c.bliss_value, "drip", 0.4);
    sk.draw();

    let left = |sk: &mut Sankey, id: &str, t1: &str, t2: String, t3: String| {
        let (nx, ny, nh, nv) = match sk.get(id) {
            Some(node) => (node.x, node.y, node.h, node.v),
            None => return,
        };
        if !(nv > 0.0) {
            return;
        }
        let yy = ny + (nh / 2.0).max(8.0);
        sk.canvas.text(nx - 14.0, yy - 6.0, t1, 19.0, 700, "c-drip", A::End);
        sk.canvas.text(nx - 14.0, yy + 15.0, &t2, 14.0, 400, "c-2", A::End);
        sk.canvas.text(nx - 14.0, yy + 33.0, &t3, 14.0, 400, "c-2", A::End);
    };
    left(&mut sk, "hours", "Hours in Studio", format!("{} h = {} effort score", num(s.hours), num(c.effort)), format!("pays {} of the payout", cents_words(k.effort)));
    left(&mut sk, "sales", "Sales score", format!("{} from {} Tickets earned", num(c.value_score), num(c.counted)), format!("pays {} of the payout", cents_words(k.value)));

    let (py, ph) = {
        let n = sk.get("payout").expect("payout exists");
        (n.y, n.h)
    };
    let rx = x2 + w + 14.0;
    sk.canvas.text(rx, py + ph / 2.0 - 8.0, &format!("Bliss payout  {}", cents_words(k.bliss)), 22.0, 700, "c-cash", A::Start);
    sk.canvas.text(rx, py + ph / 2.0 + 14.0, "a month, in dollars", 15.0, 600, "c-cash", A::Start);
    sk.canvas.text(rx, py + ph / 2.0 + 34.0, "paid to a verified contributor on a paid plan", 14.0, 400, "c-2", A::Start);

    let bottom = (ys + bs).max(py + ph + 40.0) + 20.0;
    let title = format!("A creator's share of the Treasury's payout: {} a month", cents_words(k.bliss));
    let desc = format!(
        "Hours in Studio earn {} and sales earn {}, together {} a month, paid in dollars to a verified contributor on a paid plan.",
        cents_words(k.effort),
        cents_words(k.value),
        cents_words(k.bliss)
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
    /// A subtotal the eye should find: net, Eustress's half, the Bliss payout.
    pub strong: bool,
}

fn row(label: &'static str, value: String, note: impl Into<String>, strong: bool) -> Row {
    Row { label, value, note: note.into(), strong }
}

/// A month as a short statement, in the order the money moves. Printed from the same whole dollars as the chart.
pub fn company_rows(m: &Company) -> Vec<Row> {
    let d = &m.shown;
    vec![
        row("Gross receipts", dollars(d.gross), "Ticket packs, plans and Delta packs", false),
        row("Payment fees", format!("-{}", dollars(d.fees)), "Stripe 2.9% + $0.30 on every payment", false),
        row("Net", dollars(d.net), "what is left to split", true),
        row("Bliss Treasury", format!("-{}", dollars(d.treasury)), "half of net, paid to contributors by score", false),
        row("Eustress's half", dollars(d.eustress), "the other half", true),
        row("Paid to contributors", dollars(d.treasury), "the Treasury pays out in a month what comes in", false),
    ]
}

/// The sentences that read the month aloud, from the printed dollars.
pub fn company_sentences(m: &Company) -> [String; 2] {
    let d = &m.shown;
    [
        format!("Gross {}, less fees {}, is net {}.", dollars(d.gross), dollars(d.fees), dollars(d.net)),
        format!("Half, {}, goes to the Bliss Treasury and is paid to contributors; the other half, {}, is Eustress's.", dollars(d.treasury), dollars(d.eustress)),
    ]
}

/// How a creator's month was reached, from the players to the Tickets spent.
pub fn creator_funnel(s: &Inputs, c: &Creator) -> String {
    let lift = if s.updates > 0.0 && s.lift > 0.0 { format!(" + {} from {} updates", num(c.players - s.players), num(s.updates)) } else { String::new() };
    format!("{} players{lift}, {} buy = {} buyers x {} Tickets = {} Tickets spent", num(s.players), Unit::Percent.show(s.conv), num(c.buyers), num(s.avg), num(c.spent))
}

/// A creator's month as a statement: what a sale pays in Tickets and what the Bliss payout adds up to.
pub fn creator_rows(s: &Inputs, c: &Creator) -> Vec<Row> {
    let k = &c.shown;
    let t = &c.tickets;
    vec![
        row("Tickets spent on this creator's work", format!("{} Tickets", commas(t.spent)), format!("{} bought with money", Unit::Percent.show(c_paid_share(c))), false),
        row("Creator earns", format!("{} Tickets", commas(t.earned)), "to spend inside Eustress", true),
        row("Leaves circulation", format!("{} Tickets", commas(t.platform)), "the rest of every sale", false),
        row("Bliss payout: hours in Studio", cents_words(k.effort), format!("{} h = {} effort score", num(s.hours), num(c.effort)), false),
        row("Bliss payout: sales score", cents_words(k.value), format!("{} score from {} Tickets earned from sales paid with bought Tickets", num(c.value_score), num(c.counted)), false),
        row("Bliss payout", cents_words(k.bliss), format!("{} of {} score this month, a {} share of the Treasury's payout", num(c.mine), num(c.total), pct(c.mine, c.total)), true),
    ]
}

/// The lines that explain a creator's numbers: how the score is made, what the payout does, and who is paid it.
pub fn creator_notes(s: &Inputs, m: &Company, c: &Creator) -> Vec<String> {
    let gate = if c.gate < 1.0 { format!(", scaled to {:.0}% because the network scored under 1,440 a day", c.gate * 100.0) } else { String::new() };
    let mut notes = vec![
        format!(
            "Studio: {} hours is {} effort score (assuming 40% scripting at 3.0, 40% building at 2.5 and 20% other time at 1.0, on a {} node). Sales: {} Tickets earned from sales paid with bought Tickets, at 0.5 each, is {} sales score.",
            num(s.hours),
            num(c.effort),
            s.node.name(),
            num(c.counted),
            num(c.value_score)
        ),
        format!(
            "Bliss payout: this creator's {} of {} score this month is {}. The Treasury pays out {} a month{gate}, so the share is {}.",
            num(c.mine),
            num(c.total),
            pct(c.mine, c.total),
            dollars(m.shown.treasury),
            cents_words(c.shown.bliss)
        ),
        format!(
            "Updates earn no score of their own: the scoring has no update category. They matter through the hours they take and the players they bring back ({} each here, an assumption).",
            Unit::Percent.show(s.lift)
        ),
        "The payout assumes this creator has verified their identity (18 or older), connected a Stripe account and has a Creator, Professional or Bundle plan. A share that cannot be paid stays in the Treasury.".to_string(),
    ];
    // a cap that holds the sales score down says so, with the numbers, and says nothing when it does not bind
    if c.counted < c.eligible {
        notes.push(format!(
            "One buyer's purchases from one creator count up to {} of the creator's Tickets in a day, so {} of the {} Tickets this creator earns from sales paid with bought Tickets score. This model takes each buyer to spend the average in one day.",
            commas(VALUE_CAP_TICKETS_PER_BUYER as i64),
            num(c.counted),
            num(c.eligible)
        ));
    }
    if VALUE_PER_TICKET * c.counted > VALUE_CAP_PER_DAY * DAYS {
        notes.push(format!(
            "A creator's sales score stops at {} a day, {} in this month, and this creator's sales would earn {}.",
            commas(VALUE_CAP_PER_DAY as i64),
            num(VALUE_CAP_PER_DAY * DAYS),
            num(VALUE_PER_TICKET * c.counted)
        ));
    }
    notes
}

// -----------------------------------------------------------------------------
// 4. Where Eustress's half goes (admin view)
// -----------------------------------------------------------------------------

/// Eustress's half as a Sankey: labor, servers, Cloudflare, the domain with tools and other costs, and what Eustress keeps. When the
/// costs are larger than the half, the shortfall comes in beside it.
pub fn expenses_svg(e: &Expenses, period: &str) -> String {
    let costs = e.total();
    if !(e.half > 0.0) && !(costs > 0.0) {
        return empty_svg("Nothing came in and nothing was spent", 200.0);
    }
    let half = e.half.max(0.0);
    let short = (costs - half).max(0.0);
    let inflow = half + short;
    let mut sk = Sankey::new(330.0 / inflow.max(1e-9));
    let (x0, x1, w) = (300.0, 900.0, 18.0);
    let top = 60.0;
    sk.node("half", x0, top, w, half, "eustress");
    if short > 0.0 {
        sk.node("short", x0, top + sk.h(half) + 24.0, w, short, "short");
    }
    let misc = e.domain + e.tools + e.other;
    let outs: [(&'static str, f64, &'static str, String); 5] = [
        ("labor", e.labor, "labor", "Labor (Simbuilder)".to_string()),
        ("servers", e.servers, "servers", "Game servers and relay".to_string()),
        ("cloud", e.cloudflare, "cloud", "Cloudflare".to_string()),
        ("misc", misc, "gray", "Domain, tools and other".to_string()),
        ("kept", e.retained.max(0.0), "eustress", "Eustress keeps".to_string()),
    ];
    let mut y = top;
    for (id, v, tone, _) in outs.iter() {
        if *v > 0.0 {
            sk.node(id, x1, y, w, *v, tone);
            y += sk.h(*v).max(44.0) + 16.0;
        }
    }
    // costs are paid from the half first, in the order listed, and the shortfall pays what the half cannot
    let mut half_left = half;
    for (id, v, tone, _) in outs.iter() {
        if *v <= 0.0 {
            continue;
        }
        let from_half = v.min(half_left);
        half_left -= from_half;
        if from_half > 0.0 {
            sk.link("half", id, from_half, tone, 0.5);
        }
        let rest = v - from_half;
        if rest > 0.0 && short > 0.0 {
            sk.link("short", id, rest, "short", 0.5);
        }
    }
    sk.draw();

    sk.canvas.text(x0 - 14.0, top + sk.h(half) / 2.0 - 6.0, "Eustress's half", 20.0, 700, "c-eustress", A::End);
    sk.canvas.text(x0 - 14.0, top + sk.h(half) / 2.0 + 16.0, &format!("{} {}", fmt(e.half), period), 15.0, 600, "c-eustress", A::End);
    if short > 0.0 {
        let n = sk.get("short").expect("short exists");
        let (ny, nh) = (n.y, n.h);
        sk.canvas.text(x0 - 14.0, ny + nh / 2.0 - 6.0, "Shortfall", 19.0, 700, "c-short", A::End);
        sk.canvas.text(x0 - 14.0, ny + nh / 2.0 + 15.0, &format!("{} more than came in", fmt(short)), 14.0, 400, "c-2", A::End);
    }
    let mut bottom = top + sk.h(inflow) + 60.0;
    for (id, v, tone, label) in outs.iter() {
        if *v <= 0.0 {
            continue;
        }
        let n = sk.get(id).expect("node exists");
        let (ny, nh) = (n.y, n.h);
        let tx = x1 + w + 14.0;
        let class = if *tone == "gray" { "c-gray".to_string() } else { format!("c-{tone}") };
        sk.canvas.text(tx, ny + nh / 2.0 - 4.0, label, 18.0, 700, &class, A::Start);
        sk.canvas.text(tx, ny + nh / 2.0 + 16.0, &format!("{}  \u{b7}  {} of the half", fmt(*v), pct(*v, e.half.max(costs))), 14.0, 400, "c-2", A::Start);
        bottom = bottom.max(ny + nh.max(30.0) + 30.0);
    }
    let title = format!("Where Eustress's half goes: {} {}", fmt(e.half), period);
    let desc = format!(
        "Of Eustress's half, {}: labor {}, servers {}, Cloudflare {}, domain, tools and other {}; {} {}.",
        fmt(e.half),
        fmt(e.labor),
        fmt(e.servers),
        fmt(e.cloudflare),
        fmt(misc),
        if e.retained >= 0.0 { "Eustress keeps" } else { "a shortfall of" },
        fmt(e.retained.abs())
    );
    sk.canvas.finish(bottom + 20.0, &title, &desc)
}

/// Eustress's half as a statement, in the order the money goes.
pub fn expenses_rows(e: &Expenses) -> Vec<Row> {
    vec![
        row("Eustress's half", fmt(e.half), "after the payment fee and the Treasury's half", true),
        row("Labor (Simbuilder)", format!("-{}", fmt(e.labor)), format!("{} of the half", pct(e.labor, e.half)), false),
        row("Game servers and relay", format!("-{}", fmt(e.servers)), format!("{} player hours", num(e.player_hours)), false),
        row("Cloudflare", format!("-{}", fmt(e.cloudflare)), "Workers, Durable Objects, R2", false),
        row("Domain, tools and other", format!("-{}", fmt(e.domain + e.tools + e.other)), "", false),
        row(if e.retained >= 0.0 { "Eustress keeps" } else { "Shortfall" }, fmt(e.retained), "what is left of the half", true),
    ]
}

// -----------------------------------------------------------------------------
// 5. The Treasury's payouts, day by day (Actual mode)
// -----------------------------------------------------------------------------

/// One day's payout from the Treasury.
#[derive(Clone, Debug, PartialEq)]
pub struct PayoutDay {
    pub date: String,
    pub paid_usd: f64,
    pub contributors: u32,
}

/// The period's payouts as bars, one a day, with the highest day and the total written over them.
pub fn payouts_svg(days: &[PayoutDay], period: &str) -> String {
    let paid: f64 = days.iter().map(|d| d.paid_usd).sum();
    if days.is_empty() || !(paid > 0.0) {
        return empty_svg("No payouts in this period yet", 200.0);
    }
    let mut c = super::economics_kit::Canvas::default();
    let (x0, x1, base, tall) = (90.0, super::economics_kit::WIDTH - 40.0, 250.0, 170.0);
    let top = days.iter().map(|d| d.paid_usd).fold(0.0, f64::max);
    // a slot for each day of a month at least, so a period two days old shows two narrow bars, not two walls
    let step = (x1 - x0) / days.len().max(30) as f64;
    let bar = (step * 0.7).clamp(2.0, 28.0);
    for (i, d) in days.iter().enumerate() {
        let h = if d.paid_usd > 0.0 { (d.paid_usd / top * tall).max(2.0) } else { 0.0 };
        if h > 0.0 {
            c.rects.push_str(&format!(
                "<rect class=\"n-drip\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                super::economics_kit::n(x0 + i as f64 * step),
                super::economics_kit::n(base - h),
                super::economics_kit::n(bar),
                super::economics_kit::n(h)
            ));
        }
    }
    c.text(x0, 40.0, &format!("Paid to contributors {}: {}", period, cents_words((paid * 100.0).round() as i64)), 20.0, 700, "c-drip", A::Start);
    c.text(x0, 64.0, &format!("{} days, the highest {}", days.len(), cents_words((top * 100.0).round() as i64)), 14.0, 400, "c-2", A::Start);
    c.text(x0, base + 24.0, &days[0].date, 13.0, 400, "c-3", A::Start);
    c.text(x1, base + 24.0, &days[days.len() - 1].date, 13.0, 400, "c-3", A::End);
    let title = format!("The Treasury's payouts {}: {}", period, cents_words((paid * 100.0).round() as i64));
    let desc = format!("{} days of payouts, from {} to {}, the highest {}.", days.len(), days[0].date, days[days.len() - 1].date, cents_words((top * 100.0).round() as i64));
    c.finish(base + 50.0, &title, &desc)
}

// -----------------------------------------------------------------------------
// 6. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::economics_model::{company, creator, expenses, Key, Pick, PRESETS};
    use super::*;

    struct Default3 {
        s: Inputs,
        m: Company,
        c: Creator,
    }

    fn default_month() -> Default3 {
        let s = Inputs::default();
        let m = company(&s);
        let c = creator(&s, &m);
        Default3 { s, m, c }
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

    fn all_three(s: &Inputs) -> [String; 3] {
        let m = company(s);
        let c = creator(s, &m);
        [company_svg(s, &m), tickets_svg(s, &c), bliss_svg(s, &c)]
    }

    /// The drawings the admin view and Actual mode add, for a month: where Eustress's half goes, the same month with a shortfall,
    /// a month with gifts to the Treasury, and a period of payouts.
    fn every_drawing(s: &Inputs) -> Vec<String> {
        let m = company(s);
        let e = expenses(s, &m);
        let mut short = e.clone();
        short.labor = e.half;
        short.retained = e.half - short.total();
        let mut gifts = m.clone();
        gifts.gifts_usd = 1000.0;
        gifts.gross += 1000.0;
        gifts.actual = true;
        let days = vec![PayoutDay { date: "2026-10-01".into(), paid_usd: 12.5, contributors: 3 }, PayoutDay { date: "2026-10-02".into(), paid_usd: 0.0, contributors: 0 }];
        let mut out = all_three(s).to_vec();
        out.extend([expenses_svg(&e, "a month"), expenses_svg(&short, "a month"), company_svg(s, &gifts), payouts_svg(&days, "in 2026-10")]);
        out
    }

    #[test]
    fn the_default_month_chart_prints_the_dollars_the_model_works_out() {
        let d = default_month();
        let t = texts(&company_svg(&d.s, &d.m));
        for want in [
            "Gross receipts $178,992",
            "Ticket packs",
            "$100,000 a month",
            "Plan subscriptions",
            "5,305 subscribers",
            "Delta packs",
            "Bliss Treasury",
            "$84,375 from sales",
            "Eustress's half",
            "$84,376  \u{b7}  50% of net",
            "Payment fees $10,241",
            "Stripe 2.9% + $0.30, 5.7% of gross",
            "Paid to contributors",
            "$84,375 a month",
        ] {
            assert!(t.iter().any(|x| x == want), "the chart says {want:?}; it says {t:#?}");
        }
    }

    #[test]
    fn the_default_creator_charts_print_the_tickets_and_the_cents_the_model_works_out() {
        let d = default_month();
        let a = texts(&tickets_svg(&d.s, &d.c));
        for want in ["Buyers' spend", "49,500 Tickets", "90% bought with money", "Creator earns 70%", "34,650 Tickets", "to spend inside Eustress", "Platform keeps 30%", "14,850 Tickets, out of circulation"] {
            assert!(a.iter().any(|x| x == want), "the Tickets chart says {want:?}; it says {a:#?}");
        }
        let b = texts(&bliss_svg(&d.s, &d.c));
        for want in [
            "Hours in Studio",
            "60 h = 8,640 effort score",
            "pays $161.13 of the payout",
            "Sales score",
            "15,592 from 31,185 Tickets earned",
            "pays $290.79 of the payout",
            "Bliss payout  $451.92",
            "a month, in dollars",
            "paid to a verified contributor on a paid plan",
        ] {
            assert!(b.iter().any(|x| x == want), "the Bliss chart says {want:?}; it says {b:#?}");
        }
    }

    #[test]
    fn a_chart_is_one_svg_with_a_title_and_a_description_and_no_script_or_color() {
        for svg in all_three(&Inputs::default()) {
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
        let mut low = Inputs::default();
        low.set(Key::Tickets, 3000.0);
        for k in [Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional] {
            low.set(k, 0.0);
        }
        low.set(Key::Delta, 1000.0);
        months.push(low);
        let mut few = Inputs::default();
        few.set(Key::Tickets, 10_000.0);
        for k in [Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            few.set(k, 0.0);
        }
        months.push(few);
        let mut star = Inputs::default();
        star.apply(&PRESETS[2]);
        star.set(Key::Others, 50.0);
        months.push(star);
        // nothing sold and nobody working: the charts that say how to get a chart
        let mut zero = Inputs::default();
        for k in [Key::Tickets, Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta, Key::Players, Key::Hours, Key::Others] {
            zero.set(k, 0.0);
        }
        months.push(zero);

        let css = include_str!("../../style/gallery.css");
        let mut used = std::collections::BTreeSet::new();
        for s in &months {
            for svg in every_drawing(s) {
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
        assert!(used.len() > 18, "the charts use a palette of classes: {used:?}");
        // a class is defined when the stylesheet has `.eco-svg .<class>` followed by something that ends the class name
        let defined = |cls: &str| {
            let needle = format!(".eco-svg .{cls}");
            css.match_indices(&needle).any(|(i, _)| !css[i + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        };
        for cls in &used {
            if cls == "eco-svg" {
                continue;
            }
            assert!(defined(cls), "the stylesheet colors .{cls}");
        }
        assert!(!defined("n-no-such-class") && !defined("n-trea"), "a prefix of a class name is not the class");
        // and the other way: the shipped stylesheet colors no class the public charts do not use, so none of the draft's palette is in it
        let block = &css[css.find("/* ---- The Economics page (/economics)").expect("the page's block in gallery.css")..];
        let block = &block[..block.find("/* ---- end of the Economics page ---- */").expect("the end of the block")];
        let mut colored = std::collections::BTreeSet::new();
        for (i, _) in block.match_indices(".eco-svg .") {
            colored.insert(block[i + 10..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect::<String>());
        }
        assert!(colored.len() > 30, "{colored:?}");
        for cls in &colored {
            assert!(used.contains(cls), "the shipped stylesheet colors .{cls}, which no public chart uses");
        }
        for word in ["invest", "toinv", "reserve", "expenses", "taxes", "profit", "kept", "-bad"] {
            assert!(!block.contains(word), "the shipped block has {word}");
        }
    }

    #[test]
    fn the_gross_label_is_turned_up_inside_its_box_whatever_the_size_of_the_month() {
        let d = default_month();
        let big = company_svg(&d.s, &d.m);
        assert_eq!(big.matches("data-up=\"1\"").count(), 1, "the box carries its label turned up");
        assert!(big.contains("rotate(-90 ") && big.contains("class=\"c-ongross w7\""));
        // the ribbons are scaled to the month, so a tiny month draws a box as tall as a large one
        let mut s = Inputs::default();
        s.set(Key::Tickets, 1000.0);
        for k in [Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            s.set(k, 0.0);
        }
        let m = company(&s);
        let small = company_svg(&s, &m);
        assert_eq!(small.matches("data-up=\"1\"").count(), 1);
        assert!(texts(&small).iter().any(|t| t.starts_with("Gross receipts $")));
        assert!(small.contains("height=\"330\""), "the gross box is 330 px tall: {small}");
    }

    #[test]
    fn eustresss_half_goes_to_labor_servers_cloudflare_and_the_rest_and_the_parts_add_up() {
        let s = Inputs::default();
        let m = company(&s);
        let e = expenses(&s, &m);
        let svg = expenses_svg(&e, "a month");
        let t = texts(&svg);
        for want in ["Eustress's half", "Labor (Simbuilder)", "Game servers and relay", "Cloudflare", "Domain, tools and other", "Eustress keeps"] {
            assert!(t.iter().any(|x| x == want), "{want} in {t:?}");
        }
        assert!(!t.iter().any(|x| x == "Shortfall"));
        assert!(svg.contains("class=\"n-labor\"") && svg.contains("class=\"l-servers\""));
        let rows = expenses_rows(&e);
        assert_eq!(rows.first().unwrap().label, "Eustress's half");
        assert_eq!(rows.last().unwrap().label, "Eustress keeps");
        assert_eq!(rows.iter().filter(|r| r.strong).count(), 2);
        // a month whose costs are larger than the half draws the shortfall coming in beside it
        let mut short = e.clone();
        short.labor = e.half;
        short.retained = e.half - short.total();
        let svg = expenses_svg(&short, "a month");
        let t = texts(&svg);
        assert!(t.iter().any(|x| x == "Shortfall") && !t.iter().any(|x| x == "Eustress keeps"), "{t:?}");
        assert!(svg.contains("class=\"n-short\""));
        assert_eq!(expenses_rows(&short).last().unwrap().label, "Shortfall");
        // nothing in and nothing out says so
        let none = Expenses { half: 0.0, player_hours: 0.0, servers: 0.0, cloudflare: 0.0, domain: 0.0, tools: 0.0, other: 0.0, labor: 0.0, retained: 0.0 };
        assert!(texts(&expenses_svg(&none, "a month")).iter().any(|x| x.contains("Nothing came in")));
    }

    #[test]
    fn the_payouts_are_a_bar_a_day_with_the_total_written_over_them() {
        let days = vec![
            PayoutDay { date: "2026-10-01".into(), paid_usd: 10.0, contributors: 3 },
            PayoutDay { date: "2026-10-02".into(), paid_usd: 0.0, contributors: 0 },
            PayoutDay { date: "2026-10-03".into(), paid_usd: 20.5, contributors: 4 },
        ];
        let svg = payouts_svg(&days, "in 2026-10");
        assert_eq!(svg.matches("<rect class=\"n-drip\"").count(), 2, "a day that paid nothing has no bar");
        let t = texts(&svg);
        assert!(t.iter().any(|x| x == "Paid to contributors in 2026-10: $30.50"), "{t:?}");
        assert!(t.iter().any(|x| x == "3 days, the highest $20.50"));
        assert!(t.iter().any(|x| x == "2026-10-01") && t.iter().any(|x| x == "2026-10-03"));
        assert!(texts(&payouts_svg(&[], "in 2026-10")).iter().any(|x| x.contains("No payouts")));
    }

    #[test]
    fn a_month_that_follows_roblox_draws_from_the_scaled_receipts() {
        let mut r = Inputs::default();
        r.pick(Pick::Scale, "roblox");
        let m = company(&r);
        let t = texts(&company_svg(&r, &m));
        assert!(t.iter().any(|x| x == "Gross receipts $170,666,667"), "{t:?}");
        assert!(t.iter().any(|x| x.starts_with("$95,348,")), "Ticket packs scaled with the month: {t:?}");
    }

    #[test]
    fn a_month_with_no_receipts_says_how_to_get_a_chart() {
        let mut z = Inputs::default();
        for k in [Key::Tickets, Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            z.set(k, 0.0);
        }
        let m = company(&z);
        assert!(texts(&company_svg(&z, &m)).iter().any(|t| t.contains("Add some receipts")));
        // a creator with no players and no hours has nothing to draw
        let mut none = Inputs::default();
        for k in [Key::Players, Key::Hours, Key::Others] {
            none.set(k, 0.0);
        }
        let m2 = company(&none);
        let c = creator(&none, &m2);
        assert!(texts(&tickets_svg(&none, &c)).iter().any(|t| t.contains("Add players or buyers")));
        assert!(texts(&bliss_svg(&none, &c)).iter().any(|t| t.contains("Add hours in Studio or sales")));
    }

    #[test]
    fn labels_for_boxes_that_hold_nothing_are_not_drawn() {
        let mut few = Inputs::default();
        few.set(Key::Tickets, 10_000.0);
        for k in [Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            few.set(k, 0.0);
        }
        let m = company(&few);
        let t = texts(&company_svg(&few, &m));
        assert!(!t.iter().any(|x| x == "Plan subscriptions" || x == "Delta packs" || x.ends_with("subscribers")), "no plans, no plan label: {t:#?}");
        assert!(t.iter().any(|x| x == "Ticket packs"));
        // a creator whose hours are none draws only the sales box; whose sales are none, only the hours box
        let mut hours_only = Inputs::default();
        hours_only.set(Key::Conversion, 0.0);
        let mh = company(&hours_only);
        let ch = creator(&hours_only, &mh);
        let b = texts(&bliss_svg(&hours_only, &ch));
        assert!(b.iter().any(|x| x == "Hours in Studio") && !b.iter().any(|x| x == "Sales score"), "{b:#?}");
        let mut sales_only = Inputs::default();
        sales_only.set(Key::Hours, 0.0);
        let ms = company(&sales_only);
        let cs = creator(&sales_only, &ms);
        let b2 = texts(&bliss_svg(&sales_only, &cs));
        assert!(b2.iter().any(|x| x == "Sales score") && !b2.iter().any(|x| x == "Hours in Studio"), "{b2:#?}");
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
        for k in [Key::Tickets, Key::Delta] {
            huge.set(k, 300_000.0);
        }
        months.push(huge);
        let mut star = Inputs::default();
        star.apply(&PRESETS[2]);
        months.push(star);
        let mut few = Inputs::default();
        few.set(Key::Tickets, 2000.0);
        for k in [Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            few.set(k, 0.0);
        }
        few.set(Key::PlayerPlus, 100.0);
        months.push(few);
        for s in &months {
            for svg in all_three(s) {
                assert!(height(&svg) > lowest(&svg), "the height {} reaches the lowest word at {}", height(&svg), lowest(&svg));
            }
        }
    }

    #[test]
    fn the_statement_adds_up_line_by_line() {
        let d = default_month();
        let rows = company_rows(&d.m);
        let value = |label: &str| -> i64 {
            let r = rows.iter().find(|r| r.label == label).unwrap_or_else(|| panic!("a row named {label}"));
            r.value.replace(['$', ','], "").parse().unwrap()
        };
        assert_eq!(value("Gross receipts") + value("Payment fees"), value("Net"));
        assert_eq!(value("Net") + value("Bliss Treasury"), value("Eustress's half"));
        assert_eq!(value("Paid to contributors"), -value("Bliss Treasury"));
        assert_eq!(rows.iter().filter(|r| r.strong).count(), 2);
        let [one, two] = company_sentences(&d.m);
        assert_eq!(one, "Gross $178,992, less fees $10,241, is net $168,751.");
        assert_eq!(two, "Half, $84,375, goes to the Bliss Treasury and is paid to contributors; the other half, $84,376, is Eustress's.");
    }

    #[test]
    fn the_creator_statement_adds_up_and_keeps_tickets_and_dollars_apart() {
        let d = default_month();
        let rows = creator_rows(&d.s, &d.c);
        let get = |label: &str| rows.iter().find(|r| r.label == label).unwrap_or_else(|| panic!("a row named {label}")).value.clone();
        assert_eq!(get("Tickets spent on this creator's work"), "49,500 Tickets");
        assert_eq!(get("Creator earns"), "34,650 Tickets");
        assert_eq!(get("Leaves circulation"), "14,850 Tickets");
        assert_eq!(get("Bliss payout: hours in Studio"), "$161.13");
        assert_eq!(get("Bliss payout: sales score"), "$290.79");
        assert_eq!(get("Bliss payout"), "$451.92");
        // Tickets are counts and never dollars
        for r in rows.iter().filter(|r| r.label.contains("Tickets") || r.label.contains("earns") || r.label.contains("circulation")) {
            assert!(r.value.ends_with("Tickets"), "{}: {}", r.label, r.value);
        }
        assert_eq!(rows.iter().filter(|r| r.strong).count(), 2);
    }

    #[test]
    fn the_funnel_and_notes_read_the_default_creator() {
        let d = default_month();
        assert_eq!(creator_funnel(&d.s, &d.c), "5,000 players + 500 from 2 updates, 3% buy = 165 buyers x 300 Tickets = 49,500 Tickets spent");
        let notes = creator_notes(&d.s, &d.m, &d.c);
        assert!(notes[0].contains("60 hours is 8,640 effort score") && notes[0].contains("15,592 sales score") && notes[0].contains("Light node"), "{}", notes[0]);
        assert!(notes[1].contains("24,232 of 4,524,232 score") && notes[1].contains("0.5%") && notes[1].contains("$84,375") && notes[1].contains("$451.92"), "{}", notes[1]);
        assert!(!notes[1].contains("scaled to"), "a busy network is not scaled");
        assert!(notes[2].contains("no update category"), "{}", notes[2]);
        assert!(notes[3].contains("verified their identity (18 or older)") && notes[3].contains("Creator, Professional or Bundle plan") && notes[3].contains("stays in the Treasury"), "{}", notes[3]);
        // a quiet network is scaled and says why
        let mut q = Inputs::default();
        q.set(Key::Others, 0.0);
        let mq = company(&q);
        let cq = creator(&q, &mq);
        assert!(creator_notes(&q, &mq, &cq)[1].contains("because the network scored under 1,440 a day"));
    }

    #[test]
    fn a_cap_that_holds_the_sales_score_down_is_named_with_its_numbers_and_otherwise_left_unsaid() {
        let d = default_month();
        assert_eq!(creator_notes(&d.s, &d.m, &d.c).len(), 4, "no cap holds the example creator down");
        // a hit creator reaches the daily cap: 604,800 Tickets would earn 302,400 sales score, and 9,600 a day stops it at 288,000
        let mut big = Inputs::default();
        big.apply(&PRESETS[2]);
        let mb = company(&big);
        let cb = creator(&big, &mb);
        let notes = creator_notes(&big, &mb, &cb);
        assert_eq!(notes.len(), 5, "{notes:?}");
        assert!(notes[4].contains("stops at 9,600 a day, 288,000 in this month") && notes[4].contains("would earn 302,400"), "{}", notes[4]);
        // buyers who each spend a lot at once reach the per-buyer cap
        let mut whales = Inputs::default();
        whales.set(Key::Average, 2000.0);
        let mw = company(&whales);
        let cw = creator(&whales, &mw);
        let notes = creator_notes(&whales, &mw, &cw);
        let cap = notes.iter().find(|n| n.starts_with("One buyer's purchases")).expect("the per-buyer cap is named");
        assert!(cap.contains("count up to 1,000 of the creator's Tickets in a day") && cap.contains("165,000 of the 207,900 Tickets") && cap.contains("each buyer to spend the average in one day"), "{cap}");
        // the chart and the table print the Tickets that count
        assert!(texts(&bliss_svg(&whales, &cw)).iter().any(|t| t == "82,500 from 165,000 Tickets earned"), "{:?}", texts(&bliss_svg(&whales, &cw)));
        assert!(creator_rows(&whales, &cw).iter().any(|r| r.note.contains("82,500 score from 165,000 Tickets")));
    }

    /// The words a reader sees in some markup, with the tags removed.
    fn visible(html: &str) -> String {
        let mut out = String::new();
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

    #[test]
    fn no_chart_or_table_carries_a_proposal_or_an_operating_number() {
        // The public page holds only rules that are built: no reserve, cash-out, investment, repayment, cost or tax. Several months, so
        // every label that can appear is checked.
        let mut months = vec![Inputs::default()];
        let mut star = Inputs::default();
        star.apply(&PRESETS[2]);
        months.push(star);
        let mut quiet = Inputs::default();
        quiet.set(Key::Others, 0.0);
        months.push(quiet);
        for s in &months {
            let m = company(s);
            let c = creator(s, &m);
            let mut words = String::new();
            for svg in all_three(s) {
                words.push_str(&visible(&svg));
            }
            for r in company_rows(&m).iter().chain(creator_rows(s, &c).iter()) {
                words.push_str(&format!(" {} {} {}", r.label, r.value, r.note));
            }
            for n in creator_notes(s, &m, &c).iter().chain(company_sentences(&m).iter()) {
                words.push(' ');
                words.push_str(n);
            }
            let lower = words.to_lowercase();
            for banned in ["reserve", "cash-out", "cash out", "cashed", "invest", "repay", "repaid", "return", "expense", "tax", "profit", "mercury", "own account", "worth", "backed", "discount", "owed", "covered"] {
                assert!(!lower.contains(banned), "a public label says {banned:?}: {words}");
            }
        }
    }
}
