// =============================================================================
// Eustress Web - Economics, Actual mode (/economics)
// =============================================================================
// The real numbers behind the Economics page, read from the Worker for a calendar month or a quarter and refreshed every minute:
//
//   GET /api/economics/actual          public: the Treasury now, its payouts day by day in the period, Bliss
//   GET /api/admin/economics/actual    an administrator: also receipts by kind, fees, refunds, Eustress's half, the cost ledger and
//                                       labor at its share
//
// Everything here turns those statements into what the page already draws (a Company for the month chart, Expenses for where
// Eustress's half goes, PayoutDay bars) and into CSV for the quarterly downloads. Nothing is estimated: a figure the Worker does not
// send is not shown. Costs are what an administrator entered from the bills (PUT /api/admin/economics/costs).
//
// Table of Contents:
// 1. Statements
// 2. Into the page's figures
// 3. CSV
// 4. Periods
// 5. Reading and writing (in the browser)
// 6. Tests
// =============================================================================

use std::collections::BTreeMap;

use serde::Deserialize;

use super::economics_chart::PayoutDay;
use super::economics_model::{Company, CompanyShown, Expenses};

// -----------------------------------------------------------------------------
// 1. Statements
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Period {
    pub kind: String,
    pub label: String,
    pub months: Vec<String>,
    pub from: String,
    pub to: String,
    pub current: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TreasuryNow {
    pub balance_usd: f64,
    pub high_water_usd: f64,
    pub daily_drip_usd: f64,
    pub scarcity_active: bool,
    pub paid_all_time_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct DayPaid {
    pub date: String,
    pub paid_usd: f64,
    pub contributors: u32,
    pub drip_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Contributors {
    pub paid_usd: f64,
    pub days_paid: u32,
    pub contributors_per_day: f64,
    pub days: Vec<DayPaid>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BlissNow {
    pub supply: f64,
    pub distributed: f64,
}

/// What anyone may read: `GET /api/economics/actual`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PublicStatement {
    pub period: Period,
    pub generated_at: String,
    pub treasury: TreasuryNow,
    pub contributors: Contributors,
    pub bliss: BlissNow,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct KindLine {
    pub name: String,
    pub count: u64,
    pub gross_usd: f64,
    pub fees_usd: f64,
    pub treasury_usd: f64,
    pub eustress_usd: f64,
    pub tickets: u64,
    pub delta: u64,
    pub refunded_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReceiptTotals {
    pub gross_usd: f64,
    pub fees_usd: f64,
    pub net_usd: f64,
    pub treasury_usd: f64,
    pub eustress_usd: f64,
    pub refunded_usd: f64,
    pub treasury_after_refunds_usd: f64,
    pub eustress_after_refunds_usd: f64,
    pub plan_invoices: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Receipts {
    pub by_kind: BTreeMap<String, KindLine>,
    pub totals: ReceiptTotals,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CostLine {
    pub usd: f64,
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Labor {
    pub share: f64,
    pub usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Costs {
    pub by_month: BTreeMap<String, BTreeMap<String, CostLine>>,
    pub by_category: BTreeMap<String, f64>,
    pub labor: Labor,
    pub total_usd: f64,
    pub eustress_half_usd: f64,
    pub retained_usd: f64,
}

/// What an administrator reads: `GET /api/admin/economics/actual`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AdminStatement {
    #[serde(flatten)]
    pub public: PublicStatement,
    pub receipts: Receipts,
    pub expenses: Costs,
}

/// The cost categories an administrator can enter, as the Worker names them, with their words.
pub const COST_CATEGORIES: [(&str, &str); 5] =
    [("cloudflare", "Cloudflare"), ("servers", "Game servers and relay"), ("domain", "Domain"), ("tools", "Tools"), ("other", "Other")];

// -----------------------------------------------------------------------------
// 2. Into the page's figures
// -----------------------------------------------------------------------------

fn kind(s: &AdminStatement, k: &str) -> KindLine {
    s.receipts.by_kind.get(k).cloned().unwrap_or_default()
}

/// The real period as the month chart draws it: receipts by kind, the fee, the two halves, and what the Treasury paid out.
pub fn company_from_actual(s: &AdminStatement) -> Company {
    let t = &s.receipts.totals;
    let (tickets, plans, delta, gifts) = (kind(s, "ticket_purchase"), kind(s, "subscription"), kind(s, "delta_purchase"), kind(s, "treasury_fund"));
    let d_gross = t.gross_usd.round() as i64;
    let d_fees = t.fees_usd.round() as i64;
    let d_treasury = t.treasury_usd.round() as i64;
    Company {
        tickets_usd: tickets.gross_usd,
        delta_usd: delta.gross_usd,
        gifts_usd: gifts.gross_usd,
        actual: true,
        daily_players: 0.0,
        subscribers: t.plan_invoices as f64,
        plan_revenue: plans.gross_usd,
        fee_tickets: tickets.fees_usd,
        fee_plans: plans.fees_usd,
        fee_delta: delta.fees_usd,
        gross: t.gross_usd,
        fees: t.fees_usd,
        net: t.net_usd,
        treasury_share: t.treasury_usd,
        eustress: t.eustress_usd,
        drip: s.public.contributors.paid_usd,
        shown: CompanyShown { gross: d_gross, fees: d_fees, net: d_gross - d_fees, treasury: d_treasury, eustress: d_gross - d_fees - d_treasury },
    }
}

/// Where Eustress's half went in the real period: the cost ledger, labor at its share, and what is left.
pub fn expenses_from_actual(s: &AdminStatement) -> Expenses {
    let c = &s.expenses;
    let cat = |k: &str| c.by_category.get(k).copied().unwrap_or(0.0);
    Expenses {
        half: c.eustress_half_usd,
        player_hours: 0.0,
        servers: cat("servers"),
        cloudflare: cat("cloudflare"),
        domain: cat("domain"),
        tools: cat("tools"),
        other: cat("other"),
        labor: c.labor.usd,
        retained: c.retained_usd,
    }
}

pub fn payout_days(p: &PublicStatement) -> Vec<PayoutDay> {
    p.contributors.days.iter().map(|d| PayoutDay { date: d.date.clone(), paid_usd: d.paid_usd, contributors: d.contributors }).collect()
}

// -----------------------------------------------------------------------------
// 3. CSV
// -----------------------------------------------------------------------------

/// One CSV field: quoted when it holds a comma, a quote or a line break, with its quotes doubled. A field that starts with a formula
/// character gets a leading apostrophe, so a spreadsheet shows it as text instead of running it.
pub fn csv_field(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn money(v: f64) -> String {
    format!("{:.2}", v)
}

fn line(out: &mut String, cells: &[&str]) {
    out.push_str(&cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
}

/// The public statement as CSV: section, item, amount in dollars, note.
pub fn public_csv(p: &PublicStatement) -> String {
    let mut out = String::new();
    line(&mut out, &["section", "item", "usd", "note"]);
    line(&mut out, &["period", &p.period.label, "", &format!("{} to {}{}", p.period.from, p.period.to, if p.period.current { ", in progress" } else { "" })]);
    line(&mut out, &["period", "generated", "", &p.generated_at]);
    let t = &p.treasury;
    line(&mut out, &["treasury", "balance now", &money(t.balance_usd), ""]);
    line(&mut out, &["treasury", "highest balance (decaying)", &money(t.high_water_usd), ""]);
    line(&mut out, &["treasury", "payout a day now", &money(t.daily_drip_usd), if t.scarcity_active { "scarce: 0.136% of the balance" } else { "0.276% of the balance" }]);
    line(&mut out, &["treasury", "paid to contributors, all time", &money(t.paid_all_time_usd), ""]);
    line(&mut out, &["contributors", "paid in the period", &money(p.contributors.paid_usd), &format!("{} days", p.contributors.days_paid)]);
    for d in &p.contributors.days {
        line(&mut out, &["payout", &d.date, &money(d.paid_usd), &format!("{} contributors", d.contributors)]);
    }
    line(&mut out, &["bliss", "supply", &format!("{}", p.bliss.supply), "BLS"]);
    line(&mut out, &["bliss", "distributed", &format!("{}", p.bliss.distributed), "BLS"]);
    out
}

/// The administrator's statement as CSV: the public lines, then receipts by kind, the totals, the cost ledger by month and labor.
pub fn admin_csv(s: &AdminStatement) -> String {
    let mut out = public_csv(&s.public);
    for (k, r) in &s.receipts.by_kind {
        let name = if r.name.is_empty() { k.as_str() } else { r.name.as_str() };
        line(&mut out, &["receipts", name, &money(r.gross_usd), &format!("{} payments, fees {}, refunded {}", r.count, money(r.fees_usd), money(r.refunded_usd))]);
    }
    let t = &s.receipts.totals;
    for (item, v) in [
        ("gross", t.gross_usd),
        ("payment fees", t.fees_usd),
        ("net", t.net_usd),
        ("Bliss Treasury's half", t.treasury_usd),
        ("Eustress's half", t.eustress_usd),
        ("refunded and disputed", t.refunded_usd),
        ("Eustress's half after refunds", t.eustress_after_refunds_usd),
    ] {
        line(&mut out, &["totals", item, &money(v), ""]);
    }
    for (month, rows) in &s.expenses.by_month {
        for (cat, c) in rows {
            line(&mut out, &["costs", &format!("{month} {cat}"), &money(c.usd), &c.note]);
        }
    }
    let c = &s.expenses;
    line(&mut out, &["costs", "labor (Simbuilder)", &money(c.labor.usd), &format!("{}% of Eustress's half", (c.labor.share * 1000.0).round() / 10.0)]);
    line(&mut out, &["costs", "total", &money(c.total_usd), ""]);
    line(&mut out, &["costs", if c.retained_usd >= 0.0 { "Eustress keeps" } else { "shortfall" }, &money(c.retained_usd), ""]);
    out
}

/// The file name of a download: the period and which statement.
pub fn csv_name(period: &str, admin: bool) -> String {
    format!("eustress-economics-{}{}.csv", period, if admin { "-full" } else { "" })
}

// -----------------------------------------------------------------------------
// 4. Periods
// -----------------------------------------------------------------------------

/// The periods a reader can pick, newest first: this month and the five before it, this quarter and the four before it.
/// Each is (kind, at, words).
pub fn period_choices(year: i32, month: u32) -> Vec<(&'static str, String, String)> {
    const NAMES: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let mut out = Vec::new();
    let (mut y, mut m) = (year, month);
    for i in 0..6 {
        let words = if i == 0 { format!("This month ({} {y})", NAMES[(m - 1) as usize]) } else { format!("{} {y}", NAMES[(m - 1) as usize]) };
        out.push(("month", format!("{y}-{m:02}"), words));
        if m == 1 {
            m = 12;
            y -= 1;
        } else {
            m -= 1;
        }
    }
    let (mut y, mut q) = (year, (month - 1) / 3 + 1);
    for i in 0..5 {
        let words = if i == 0 { format!("This quarter (Q{q} {y})") } else { format!("Q{q} {y}") };
        out.push(("quarter", format!("{y}-Q{q}"), words));
        if q == 1 {
            q = 4;
            y -= 1;
        } else {
            q -= 1;
        }
    }
    out
}

// -----------------------------------------------------------------------------
// 5. Reading and writing (in the browser)
// -----------------------------------------------------------------------------

/// The period Actual mode shows: a month (at YYYY-MM) or a quarter (at YYYY-Qn); an empty `at` is the current one.
#[derive(Clone, Debug, PartialEq)]
pub struct PeriodPick {
    pub kind: &'static str,
    pub at: String,
}

impl Default for PeriodPick {
    fn default() -> Self {
        PeriodPick { kind: "month", at: String::new() }
    }
}

impl PeriodPick {
    /// The query string the Worker's statements take.
    pub fn query(&self) -> String {
        if self.at.is_empty() {
            format!("period={}", self.kind)
        } else {
            format!("period={}&at={}", self.kind, urlencoding::encode(&self.at))
        }
    }
}

/// What the Worker last said about the period.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveState {
    pub public: Option<PublicStatement>,
    pub admin: Option<AdminStatement>,
    pub error: Option<String>,
    pub loading: bool,
    /// When it was read, in the reader's own time.
    pub read_at: String,
}

/// Dollars and cents the way the page writes money: "$1,234.50".
pub fn usd_words(v: f64) -> String {
    super::economics_model::cents_words((v * 100.0).round() as i64)
}

/// The periods to pick from, as of today in the reader's time zone (a fixed date on the server, which renders no page for this).
pub fn period_choices_now() -> Vec<(&'static str, String, String)> {
    #[cfg(not(feature = "ssr"))]
    {
        let d = js_sys::Date::new_0();
        period_choices(d.get_utc_full_year() as i32, d.get_utc_month() + 1)
    }
    #[cfg(feature = "ssr")]
    {
        period_choices(2026, 10)
    }
}

#[cfg(not(feature = "ssr"))]
fn api() -> String {
    crate::state::api_base()
}

/// The public statement. Read without the account's token: the route allows any origin and sends no credentials.
#[cfg(not(feature = "ssr"))]
pub async fn load_public(p: &PeriodPick) -> Result<PublicStatement, String> {
    let url = format!("{}/api/economics/actual?{}", api(), p.query());
    let res = gloo_net::http::Request::get(&url).send().await.map_err(|_| "The platform's numbers could not be read. Try again in a minute.".to_string())?;
    if !res.ok() {
        return Err(format!("The platform answered {} for this period.", res.status()));
    }
    res.json::<PublicStatement>().await.map_err(|_| "The platform's answer could not be read.".to_string())
}

/// The administrator's statement, or None for anyone else (the Worker refuses them).
#[cfg(not(feature = "ssr"))]
pub async fn load_admin(p: &PeriodPick) -> Option<AdminStatement> {
    crate::api::ApiClient::new(&api()).get::<AdminStatement>(&format!("/api/admin/economics/actual?{}", p.query())).await.ok()
}

/// Read the period again: the public statement, and the administrator's when the reader is one.
#[cfg(not(feature = "ssr"))]
pub async fn refresh(p: PeriodPick, state: leptos::prelude::RwSignal<LiveState>, admin: leptos::prelude::RwSignal<bool>) {
    use leptos::prelude::*;
    state.update(|l| l.loading = true);
    let public = load_public(&p).await;
    let full = if admin.get_untracked() { load_admin(&p).await } else { None };
    let at = js_sys::Date::new_0().to_locale_time_string("en-US").as_string().unwrap_or_default();
    state.update(|l| {
        match public {
            Ok(s) => {
                l.public = Some(s);
                l.error = None;
            }
            Err(e) => l.error = Some(e),
        }
        if full.is_some() {
            l.admin = full;
        }
        l.loading = false;
        l.read_at = at;
    });
}

/// Set one category of one month's costs. Returns what to tell the administrator.
#[cfg(not(feature = "ssr"))]
pub async fn put_cost(month: &str, category: &str, amount: &str, note: &str) -> String {
    let Ok(v) = amount.trim().parse::<f64>() else { return "The amount is not a number.".to_string() };
    let body = serde_json::json!({ "month": month, "category": category, "amount_usd": v, "note": note });
    match crate::api::ApiClient::new(&api()).put::<serde_json::Value, _>("/api/admin/economics/costs", &body).await {
        Ok(_) => format!("Saved {category} for {month}."),
        Err(e) => format!("Not saved: {e}"),
    }
}

/// Set labor's share of Eustress's half, given in percent.
#[cfg(not(feature = "ssr"))]
pub async fn put_labor_share(percent: &str) -> String {
    let Ok(v) = percent.trim().parse::<f64>() else { return "The share is not a number.".to_string() };
    let body = serde_json::json!({ "labor_share": v / 100.0 });
    match crate::api::ApiClient::new(&api()).put::<serde_json::Value, _>("/api/admin/economics/settings", &body).await {
        Ok(_) => format!("Labor is {v}% of Eustress's half."),
        Err(e) => format!("Not saved: {e}"),
    }
}

/// Recompute the Treasury's month totals from its records.
#[cfg(not(feature = "ssr"))]
pub async fn rebuild_totals() -> String {
    match crate::api::ApiClient::new(&api()).post::<serde_json::Value, _>("/api/admin/economics/rebuild", &serde_json::json!({})).await {
        Ok(v) => format!("Rebuilt {} months from {} payments.", v.get("months").and_then(|x| x.as_u64()).unwrap_or(0), v.get("deposits").and_then(|x| x.as_u64()).unwrap_or(0)),
        Err(e) => format!("Not rebuilt: {e}"),
    }
}

/// The signed-in creator's Spaces.
#[cfg(not(feature = "ssr"))]
pub async fn load_my_projects() -> Result<Vec<super::economics_mine::MyProject>, String> {
    match crate::api::ApiClient::new(&api()).get::<super::economics_mine::MyProjects>("/api/projects").await {
        Ok(p) => Ok(p.projects),
        Err(crate::api::ApiError::Unauthorized) => Err("Sign in to start from your own Spaces.".to_string()),
        Err(e) => Err(format!("Your Spaces could not be read: {e}")),
    }
}

/// The newest sales (up to 100 each) of the picked Spaces, in live mode.
#[cfg(not(feature = "ssr"))]
pub async fn load_sales(ids: &[String]) -> Vec<super::economics_mine::MySale> {
    let mut out = Vec::new();
    for id in ids {
        if let Ok(list) = crate::api::list_sales(&api(), crate::api::CommerceMode::Live, id).await {
            out.extend(list.data.into_iter().map(|s| super::economics_mine::MySale {
                buyer: s.buyer_id,
                tickets: s.amount,
                created: s.created,
                refunded: s.status == "refunded",
            }));
        }
    }
    out
}

// -----------------------------------------------------------------------------
// 6. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A statement as the Worker sends it (the shape tests/economics.test.mjs checks on the Worker's side).
    const ADMIN_JSON: &str = r#"{
      "period": {"kind":"month","label":"2026-10","months":["2026-10"],"from":"2026-10-01","to":"2026-10-31","current":true},
      "generated_at": "2026-10-07T20:00:00.000Z",
      "treasury": {"balance_usd": 1234.5, "high_water_usd": 1500, "daily_drip_usd": 3.41, "scarcity_active": false, "paid_all_time_usd": 99.99},
      "contributors": {"paid_usd": 20.5, "days_paid": 2, "contributors_per_day": 3.5,
        "days": [{"date":"2026-10-05","paid_usd":10,"contributors":3,"drip_usd":10.2},{"date":"2026-10-06","paid_usd":10.5,"contributors":4,"drip_usd":10.6}]},
      "bliss": {"supply": 1000000, "distributed": 4000},
      "receipts": {
        "by_kind": {
          "ticket_purchase": {"name":"Ticket packs","count":2,"gross_usd":29.98,"fees_usd":1.47,"treasury_usd":14.26,"eustress_usd":14.25,"tickets":2720,"delta":0,"refunded_usd":9.99},
          "subscription": {"name":"Plans","count":1,"gross_usd":9.99,"fees_usd":0.59,"treasury_usd":4.7,"eustress_usd":4.7,"tickets":0,"delta":0,"refunded_usd":0},
          "treasury_fund": {"name":"Gifts to the Treasury","count":1,"gross_usd":50,"fees_usd":1.75,"treasury_usd":48.25,"eustress_usd":0,"tickets":0,"delta":0,"refunded_usd":0}
        },
        "totals": {"gross_usd":89.97,"fees_usd":3.81,"net_usd":86.16,"treasury_usd":67.21,"eustress_usd":18.95,"refunded_usd":9.99,"treasury_after_refunds_usd":62.51,"eustress_after_refunds_usd":14.25,"plan_invoices":1}
      },
      "expenses": {
        "by_month": {"2026-10": {"cloudflare": {"usd": 5, "note": "Workers Paid, \"base\""}, "servers": {"usd": 2.5, "note": ""}}},
        "by_category": {"cloudflare": 5, "servers": 2.5, "domain": 0, "tools": 0, "other": 0},
        "labor": {"share": 0.3, "usd": 4.28}, "total_usd": 11.78, "eustress_half_usd": 14.25, "retained_usd": 2.47
      },
      "settings": {"labor_share": 0.3}
    }"#;

    fn admin() -> AdminStatement {
        serde_json::from_str(ADMIN_JSON).expect("the statement parses")
    }

    #[test]
    fn the_admin_statement_parses_and_the_public_one_is_its_first_half() {
        let s = admin();
        assert_eq!(s.public.period.label, "2026-10");
        assert_eq!(s.public.contributors.days.len(), 2);
        assert_eq!(s.receipts.by_kind["ticket_purchase"].tickets, 2720);
        assert_eq!(s.expenses.by_month["2026-10"]["cloudflare"].usd, 5.0);
        let p: PublicStatement = serde_json::from_str(ADMIN_JSON).expect("the public part parses alone");
        assert_eq!(p, s.public);
        // a statement missing parts still parses, empty
        let bare: AdminStatement = serde_json::from_str("{}").expect("empty parses");
        assert_eq!(bare.receipts.totals.gross_usd, 0.0);
    }

    #[test]
    fn a_real_month_draws_as_the_month_chart_does() {
        let s = admin();
        let m = company_from_actual(&s);
        assert!(m.actual);
        assert_eq!((m.tickets_usd, m.plan_revenue, m.delta_usd, m.gifts_usd), (29.98, 9.99, 0.0, 50.0));
        assert_eq!((m.gross, m.fees, m.net, m.treasury_share, m.eustress), (89.97, 3.81, 86.16, 67.21, 18.95));
        assert_eq!(m.drip, 20.5, "what the Treasury paid in the period");
        assert_eq!(m.subscribers, 1.0);
        assert_eq!(m.shown.gross - m.shown.fees, m.shown.net);
        assert_eq!(m.shown.treasury + m.shown.eustress, m.shown.net);
        // the chart says what is real, not the model's steady state
        let svg = super::super::economics_chart::company_svg(&super::super::economics_model::Inputs::default(), &m);
        assert!(svg.contains("in the period") && !svg.contains("pays out what comes in"), "{svg}");
        assert!(svg.contains("Gifts to the Treasury"));
    }

    #[test]
    fn a_real_month_of_costs_draws_as_the_costs_chart_does() {
        let e = expenses_from_actual(&admin());
        assert_eq!((e.half, e.cloudflare, e.servers, e.labor, e.retained), (14.25, 5.0, 2.5, 4.28, 2.47));
        assert!((e.half - e.total() - e.retained).abs() < 0.011, "the Worker's sum agrees with the parts");
        let svg = super::super::economics_chart::expenses_svg(&e, "in 2026-10");
        assert!(svg.contains("Labor (Simbuilder)") && svg.contains("Eustress keeps"));
    }

    #[test]
    fn csv_quotes_what_it_must_and_never_starts_a_formula() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a, b"), "\"a, b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(csv_field("-12.50"), "-12.50", "a negative number stays a number");
    }

    #[test]
    fn the_downloads_hold_every_line_of_the_statement() {
        let s = admin();
        let p = public_csv(&s.public);
        assert!(p.starts_with("section,item,usd,note\r\n"));
        for want in ["treasury,balance now,1234.50,", "payout,2026-10-05,10.00,3 contributors", "payout,2026-10-06,10.50,4 contributors", "contributors,paid in the period,20.50,2 days"] {
            assert!(p.contains(want), "{want} in\n{p}");
        }
        for word in ["receipts", "costs", "labor", "Eustress"] {
            assert!(!p.contains(word), "the public download says {word}");
        }
        let a = admin_csv(&s);
        assert!(a.starts_with(&p), "the full download begins with the public one");
        for want in [
            "receipts,Ticket packs,29.98,",
            "receipts,Gifts to the Treasury,50.00,",
            "totals,Eustress's half after refunds,14.25,",
            "costs,2026-10 cloudflare,5.00,\"Workers Paid, \"\"base\"\"\"",
            "costs,labor (Simbuilder),4.28,30% of Eustress's half",
            "costs,Eustress keeps,2.47,",
        ] {
            assert!(a.contains(want), "{want} in\n{a}");
        }
        assert_eq!(a.lines().count(), a.matches("\r\n").count(), "every line ends CRLF");
        assert_eq!((csv_name("2026-Q4", false), csv_name("2026-Q4", true)), ("eustress-economics-2026-Q4.csv".to_string(), "eustress-economics-2026-Q4-full.csv".to_string()));
    }

    #[test]
    fn the_periods_run_back_six_months_and_five_quarters_across_the_new_year() {
        let p = period_choices(2026, 2);
        assert_eq!(p.len(), 11);
        assert_eq!(p[0], ("month", "2026-02".to_string(), "This month (February 2026)".to_string()));
        assert_eq!(p[1].1, "2026-01");
        assert_eq!(p[2].1, "2025-12");
        assert_eq!(p[5].1, "2025-09");
        assert_eq!(p[6], ("quarter", "2026-Q1".to_string(), "This quarter (Q1 2026)".to_string()));
        assert_eq!(p[7].1, "2025-Q4");
        assert_eq!(p[10].1, "2025-Q1");
        assert_eq!(period_choices(2026, 12)[6].1, "2026-Q4");
    }

    #[test]
    fn the_cost_categories_are_the_workers() {
        let worker = include_str!("../../../../../infrastructure/cloudflare/api/src/economics.mjs");
        assert!(worker.contains("export const COST_CATEGORIES = ['cloudflare', 'servers', 'domain', 'tools', 'other'];"));
        assert_eq!(COST_CATEGORIES.map(|c| c.0), ["cloudflare", "servers", "domain", "tools", "other"]);
    }
}
