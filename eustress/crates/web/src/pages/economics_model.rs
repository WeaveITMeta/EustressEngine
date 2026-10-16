// =============================================================================
// Eustress Web - Economics model (/economics)
// =============================================================================
// The arithmetic behind the Economics page: one month of Eustress receipts split by the rules that are built today, and what one creator
// is paid under them. It is pure and tested; economics_chart.rs draws it and economics.rs lays out the page. Every rule here is one the
// Worker applies (the constant names its source). Nothing here is a price: the volumes and the creator's numbers are inputs the reader
// moves.
//
// What is not here is on purpose: proposals that are not decided (a cash-out reserve, money put into the Treasury and repaid) and tax
// are not for publication. They live in economics_draft_model.rs, compiled only with the `economics-draft` feature.
//
// The admin view (shown only to an account the Worker calls an administrator) adds what Eustress's half pays for: servers, Cloudflare,
// the domain and labor, from the assumptions in docs/architecture/HOSTING_COST_PLAN.md (none of them measured yet), and the
// sales-score rules proposed to replace the daily cap, so their effect can be seen before the Worker changes.
//
// Table of Contents:
// 1. Constants: packs, plans and rates
// 2. Inputs and the controls that set them
// 3. A month of Eustress
// 4. One creator
// 5. Tests
// =============================================================================

// -----------------------------------------------------------------------------
// 1. Constants
// -----------------------------------------------------------------------------

/// A Ticket pack: its price and the Tickets it holds, bonus included. These are the Worker's `TICKET_PACKAGES`
/// (infrastructure/cloudflare/api/src/index.js); no page sells from this list.
pub struct Pack {
    pub name: &'static str,
    pub usd: f64,
    pub tickets: f64,
}

pub static PACKS: [Pack; 5] = [
    Pack { name: "Starter", usd: 4.99, tickets: 400.0 },
    Pack { name: "Standard", usd: 9.99, tickets: 880.0 },
    Pack { name: "Mega", usd: 19.99, tickets: 1840.0 },
    Pack { name: "Super", usd: 49.99, tickets: 5000.0 },
    Pack { name: "Ultra", usd: 99.99, tickets: 10800.0 },
];

/// The pack a month starts with: Standard.
pub const DEFAULT_PACK: usize = 1;

/// A plan and its monthly price. The Premium page lists the same plans; a test keeps the two in step.
pub struct Plan {
    pub name: &'static str,
    pub usd: f64,
}

pub static PLANS: [Plan; 4] = [
    Plan { name: "Player Plus", usd: 4.99 },
    Plan { name: "Creator", usd: 9.99 },
    Plan { name: "Bundle", usd: 12.99 },
    Plan { name: "Professional", usd: 129.0 },
];

/// Stripe on the web: 2.9% plus 30 cents a payment (`splitSale` in treasury.mjs). Every purchase is made on the web today.
pub const STRIPE_PERCENT: f64 = 0.029;
pub const STRIPE_FIXED_USD: f64 = 0.30;
/// The average Delta pack order, used only to price the fixed part of Stripe's fee on Delta packs.
pub const DELTA_ORDER_USD: f64 = 25.0;

/// The creator's share of a sale, paid as earned Tickets (`CREATOR_SHARE`, commerce.mjs). The rest leaves circulation. The rate is not
/// final; the page says so.
pub const CREATOR_SHARE: f64 = 0.7;

/// Studio's contribution weights (docs: Earning): scripting 3.0, building 2.5, anything else 1.0. The model assumes a 40/40/20 mix of a
/// creator's hours, which is an assumption and says so on the page.
pub const WEIGHT_SCRIPTING: f64 = 3.0;
pub const WEIGHT_BUILDING: f64 = 2.5;
pub const WEIGHT_OTHER: f64 = 1.0;
pub const MIX_SCRIPTING: f64 = 0.4;
pub const MIX_BUILDING: f64 = 0.4;
pub const MIX_OTHER: f64 = 0.2;

/// The score a minute of the assumed mix earns on a Light node: 0.4 x 3.0 + 0.4 x 2.5 + 0.2 x 1.0.
pub fn effort_per_minute() -> f64 {
    MIX_SCRIPTING * WEIGHT_SCRIPTING + MIX_BUILDING * WEIGHT_BUILDING + MIX_OTHER * WEIGHT_OTHER
}

/// Sales earn a creator 0.5 score per Ticket received from a sale paid with bought Tickets (`VALUE_SCORE_PER_TICKET`, bliss.mjs),
/// capped per creator per day (`VALUE_CAP_SCORE_PER_CREATOR`).
pub const VALUE_PER_TICKET: f64 = 0.5;
pub const VALUE_CAP_PER_DAY: f64 = 9600.0;
/// Of one buyer's purchases from one creator in a day, at most this many of the creator's Tickets score (`VALUE_CAP_TICKETS_PER_BUYER`).
pub const VALUE_CAP_TICKETS_PER_BUYER: f64 = 1000.0;
/// Effort score stops at 3,200 per account per UTC day.
pub const EFFORT_CAP_PER_DAY: f64 = 3200.0;
/// A day pays out its whole allowance only when the day's total score reaches this (`FULL_DAY_SCORE`, bliss.mjs).
pub const FULL_DAY_SCORE: f64 = 1440.0;
/// A month is thirty days here.
pub const DAYS: f64 = 30.0;

/// Roblox's second quarter of 2026 (shareholder letter, 2026-07-30): 123 million average daily users and $1.6 billion of bookings in
/// the quarter. The page measures Eustress against a share of that market.
pub const ROBLOX_DAU: f64 = 123_000_000.0;
pub const ROBLOX_BOOKINGS_MONTH_USD: f64 = 1_600_000_000.0 / 3.0;
/// The share of Roblox's market McKale measures against, in percent.
pub const ROBLOX_SHARE_MARK: f64 = 32.0;
/// The Players slider's ceiling: 32% of Roblox's daily users, as one creator's players in a month.
pub const PLAYERS_MAX: f64 = 39_360_000.0;

/// Roblox's bookings per daily user per month (about $4.34). Turns receipts set by hand into daily players for the server costs.
pub fn spend_per_daily_player() -> f64 {
    ROBLOX_BOOKINGS_MONTH_USD / ROBLOX_DAU
}

/// What Eustress's half pays for, in the admin view. Assumptions from docs/architecture/HOSTING_COST_PLAN.md (2026-10-01), list
/// prices with nothing measured: an hour a player spends on Eustress's servers (middle density, with 25% spare), on a creator's own
/// machine through Eustress's relay, and on Cloudflare's control plane; the fixed fleet of two regions; the Workers plan; the domain.
pub const SERVER_USD_PER_PLAYER_HOUR: f64 = 0.0017;
pub const RELAY_USD_PER_PLAYER_HOUR: f64 = 0.0001;
pub const CONTROL_PLANE_USD_PER_PLAYER_HOUR: f64 = 0.00005;
pub const FLEET_FLOOR_USD: f64 = 470.0;
pub const CLOUDFLARE_PLAN_USD: f64 = 5.0;
pub const DOMAIN_USD: f64 = 10.0 / 12.0;

// -----------------------------------------------------------------------------
// 2. Inputs and the controls that set them
// -----------------------------------------------------------------------------

/// A Studio node: Light earns 1.0 on score, Full 1.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Light,
    Full,
}

impl NodeKind {
    pub fn bonus(self) -> f64 {
        match self {
            NodeKind::Light => 1.0,
            NodeKind::Full => 1.1,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Light => "Light",
            NodeKind::Full => "Full",
        }
    }
}

/// Where a month's receipts come from: the receipt sliders, or a share of Roblox's market split as the sliders split it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    ByHand,
    Roblox,
}

/// How a creator's sales become score. `Today` is the Worker's rule; the other two are proposals to compare (admin view).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SalesRule {
    /// Sales score stops at 9,600 a day.
    Today,
    /// No daily stop; one buyer still counts for at most 1,000 Tickets a day.
    NoDailyCap,
    /// Full score up to 9,600 a day, then growing with the logarithm of the rest.
    Diminishing,
}

/// Everything the reader can move. Money is dollars a month, percentages are 0 to 100.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    /// Ticket pack sales, in dollars a month, and which pack the buyers choose (an index into `PACKS`).
    pub tickets: f64,
    pub pack: usize,
    /// Subscribers to each plan, in the order of `PLANS`.
    pub subscribers: [f64; 4],
    pub delta: f64,
    pub players: f64,
    pub conv: f64,
    pub avg: f64,
    pub paid: f64,
    pub hours: f64,
    pub node: NodeKind,
    pub updates: f64,
    pub lift: f64,
    pub others: f64,
    pub score: f64,
    pub scale: Scale,
    /// Percent of Roblox's market, when `scale` is `Roblox`.
    pub market: f64,
    pub rule: SalesRule,
    /// Admin view: labor as a percent of Eustress's half, the percent of play hours on creators' own machines, and hours a day.
    pub labor: f64,
    pub hosted: f64,
    pub hours_per_player: f64,
}

impl Default for Inputs {
    /// The example month the page opens on.
    fn default() -> Self {
        Inputs {
            tickets: 100_000.0,
            pack: DEFAULT_PACK,
            subscribers: [3000.0, 1500.0, 800.0, 5.0],
            delta: 38_000.0,
            players: 5000.0,
            conv: 3.0,
            avg: 300.0,
            paid: 90.0,
            hours: 60.0,
            node: NodeKind::Light,
            updates: 2.0,
            lift: 5.0,
            others: 300.0,
            score: 500.0,
            scale: Scale::ByHand,
            market: ROBLOX_SHARE_MARK,
            rule: SalesRule::Today,
            labor: 30.0,
            hosted: 70.0,
            hours_per_player: 2.4,
        }
    }
}

/// A number the reader moves with a slider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Tickets,
    PlayerPlus,
    Creator,
    Bundle,
    Professional,
    Delta,
    Players,
    Conversion,
    Average,
    Paid,
    Hours,
    Updates,
    Lift,
    Others,
    Score,
    Market,
    Labor,
    Hosted,
    HoursPerPlayer,
}

/// How a control's value is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Dollars,
    Percent,
    Count,
    Hours,
    /// Hours with one decimal: "2.4 h".
    HoursFine,
}

impl Unit {
    pub fn show(self, v: f64) -> String {
        match self {
            Unit::Dollars => dollars(v.round() as i64),
            Unit::Percent => format!("{}%", (v * 100.0).round() / 100.0),
            Unit::Count => commas(v.round() as i64),
            Unit::Hours => format!("{} h", commas(v.round() as i64)),
            Unit::HoursFine => format!("{} h", (v * 10.0).round() / 10.0),
        }
    }
}

/// The two groups the controls sit in on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Receipts,
    Creator,
    /// Shown only to an administrator.
    Admin,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Receipts, Group::Creator, Group::Admin];

    pub fn title(self) -> &'static str {
        match self {
            Group::Receipts => "What Eustress sells in a month",
            Group::Creator => "One creator",
            Group::Admin => "Costs and proposals (admin)",
        }
    }
}

/// One slider: what it sets, its words, its range. The page builds its sliders from this table, so a control exists in one place.
pub struct Control {
    pub key: Key,
    pub group: Group,
    pub label: &'static str,
    /// A line under the label for a control whose meaning is not obvious; empty when the label says it.
    pub help: &'static str,
    pub unit: Unit,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// A slider whose range spans orders of magnitude moves along a log scale (`position`).
    pub log: bool,
}

/// The positions of a log slider: 0 is nothing, 1 to 1,000 run from 100 to the control's maximum on a log scale.
pub const LOG_POSITIONS: f64 = 1000.0;
const LOG_FLOOR: f64 = 100.0;

/// Two significant figures: 5,021 is 5,000 and 39,360,000 stays 39,000,000 only at the very end, where the maximum is kept.
fn two_figures(v: f64) -> f64 {
    if v <= 0.0 {
        return 0.0;
    }
    let p = 10f64.powf(v.log10().floor() - 1.0);
    (v / p).round() * p
}

impl Control {
    /// What the range input carries: its own min, max and step.
    pub fn input_range(&self) -> (f64, f64, f64) {
        if self.log {
            (0.0, LOG_POSITIONS, 1.0)
        } else {
            (self.min, self.max, self.step)
        }
    }

    /// The range input's position for a value.
    pub fn position(&self, v: f64) -> f64 {
        if !self.log {
            return v;
        }
        if v <= 0.0 {
            return 0.0;
        }
        let (lo, hi) = (LOG_FLOOR.log10(), self.max.log10());
        (((v.max(LOG_FLOOR).log10() - lo) / (hi - lo)) * (LOG_POSITIONS - 1.0) + 1.0).round().clamp(1.0, LOG_POSITIONS)
    }

    /// The value at a range input's position.
    pub fn from_position(&self, p: f64) -> f64 {
        if !self.log {
            return p;
        }
        let p = p.round().clamp(0.0, LOG_POSITIONS);
        if p == 0.0 {
            return 0.0;
        }
        if p == LOG_POSITIONS {
            return self.max;
        }
        let (lo, hi) = (LOG_FLOOR.log10(), self.max.log10());
        two_figures(10f64.powf(lo + (hi - lo) * (p - 1.0) / (LOG_POSITIONS - 1.0))).min(self.max)
    }
}

macro_rules! control {
    ($key:ident, $group:ident, $label:expr, $help:expr, $unit:ident, $min:expr, $max:expr, $step:expr) => {
        Control { key: Key::$key, group: Group::$group, label: $label, help: $help, unit: Unit::$unit, min: $min, max: $max, step: $step, log: false }
    };
    (log $key:ident, $group:ident, $label:expr, $help:expr, $unit:ident, $min:expr, $max:expr, $step:expr) => {
        Control { key: Key::$key, group: Group::$group, label: $label, help: $help, unit: Unit::$unit, min: $min, max: $max, step: $step, log: true }
    };
}

pub static CONTROLS: [Control; 19] = [
    control!(Tickets, Receipts, "Ticket pack sales", "", Dollars, 0.0, 300_000.0, 1000.0),
    control!(PlayerPlus, Receipts, "Player Plus subscribers ($4.99 a month)", "", Count, 0.0, 20_000.0, 100.0),
    control!(Creator, Receipts, "Creator subscribers ($9.99 a month)", "", Count, 0.0, 20_000.0, 100.0),
    control!(Bundle, Receipts, "Bundle subscribers ($12.99 a month)", "", Count, 0.0, 20_000.0, 100.0),
    control!(Professional, Receipts, "Professional subscribers ($129 a month)", "", Count, 0.0, 200.0, 1.0),
    control!(Delta, Receipts, "Delta pack sales", "", Dollars, 0.0, 300_000.0, 1000.0),
    control!(log Players, Creator, "Players a month (popularity)", "Up to 39.36 million: 32% of Roblox's daily players.", Count, 0.0, PLAYERS_MAX, 1.0),
    control!(Conversion, Creator, "Conversion: players who buy", "", Percent, 0.0, 20.0, 0.5),
    control!(Average, Creator, "Tickets each buyer spends", "", Count, 0.0, 2000.0, 25.0),
    control!(Paid, Creator, "Spend that was bought with money", "Sales score counts only Tickets that were bought with money.", Percent, 0.0, 100.0, 5.0),
    control!(Hours, Creator, "Hours in Studio a month", "Paid through the Bliss payout.", Hours, 0.0, 300.0, 5.0),
    control!(Updates, Creator, "Updates a month", "Updates earn no score of their own; they matter through the players they bring back.", Count, 0.0, 12.0, 1.0),
    control!(Lift, Creator, "Players gained per update", "An assumption, not a rule.", Percent, 0.0, 20.0, 1.0),
    control!(Others, Creator, "Other contributors active each day", "The Treasury pays out the same amount however many share it, so more contributors means a smaller share each.", Count, 0.0, 5000.0, 50.0),
    control!(Score, Creator, "Their average daily score", "", Count, 0.0, 3200.0, 50.0),
    control!(Market, Receipts, "Share of Roblox's market", "When receipts follow Roblox: 32% is about 39 million daily players and $171 million a month.", Percent, 0.0, 100.0, 0.5),
    control!(Labor, Admin, "Labor (Simbuilder): share of Eustress's half", "", Percent, 0.0, 100.0, 1.0),
    control!(Hosted, Admin, "Play hours on creators' own machines", "The rest run on Eustress's servers.", Percent, 0.0, 100.0, 5.0),
    control!(HoursPerPlayer, Admin, "Hours a daily player plays", "Roblox's 2024 figure is 2.4.", HoursFine, 0.5, 6.0, 0.1),
];

/// The control for a key.
pub fn control(key: Key) -> &'static Control {
    CONTROLS.iter().find(|c| c.key == key).expect("every key has a control")
}

/// One thing in a group's panel: a slider, or a list to choose from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Slider(Key),
    Choice(Pick),
}

/// A group's panel in reading order. The page builds each panel from this, so a control is placed in one spot.
pub fn layout(group: Group) -> Vec<Item> {
    use Item::{Choice as C, Slider as S};
    match group {
        Group::Receipts => vec![S(Key::Tickets), C(Pick::Pack), S(Key::PlayerPlus), S(Key::Creator), S(Key::Bundle), S(Key::Professional), S(Key::Delta), C(Pick::Scale), S(Key::Market)],
        Group::Admin => vec![S(Key::Labor), S(Key::Hosted), S(Key::HoursPerPlayer), C(Pick::Rule)],
        Group::Creator => vec![
            S(Key::Players),
            S(Key::Conversion),
            S(Key::Average),
            S(Key::Paid),
            S(Key::Hours),
            C(Pick::Node),
            S(Key::Updates),
            S(Key::Lift),
            S(Key::Others),
            S(Key::Score),
        ],
    }
}

impl Inputs {
    pub fn get(&self, key: Key) -> f64 {
        match key {
            Key::Tickets => self.tickets,
            Key::PlayerPlus => self.subscribers[0],
            Key::Creator => self.subscribers[1],
            Key::Bundle => self.subscribers[2],
            Key::Professional => self.subscribers[3],
            Key::Delta => self.delta,
            Key::Players => self.players,
            Key::Conversion => self.conv,
            Key::Average => self.avg,
            Key::Paid => self.paid,
            Key::Hours => self.hours,
            Key::Updates => self.updates,
            Key::Lift => self.lift,
            Key::Others => self.others,
            Key::Score => self.score,
            Key::Market => self.market,
            Key::Labor => self.labor,
            Key::Hosted => self.hosted,
            Key::HoursPerPlayer => self.hours_per_player,
        }
    }

    /// Set a control from what a range input reports: its position as text (the value itself, except on a log slider). Text that
    /// is not a number leaves the control as it was.
    pub fn set_text(&mut self, key: Key, text: &str) {
        if let Ok(p) = text.trim().parse::<f64>() {
            self.set(key, control(key).from_position(p));
        }
    }

    /// Set a control, kept inside its range. A value that is not a number leaves the control as it was.
    pub fn set(&mut self, key: Key, value: f64) {
        if !value.is_finite() {
            return;
        }
        let c = control(key);
        let v = value.clamp(c.min, c.max);
        match key {
            Key::Tickets => self.tickets = v,
            Key::PlayerPlus => self.subscribers[0] = v,
            Key::Creator => self.subscribers[1] = v,
            Key::Bundle => self.subscribers[2] = v,
            Key::Professional => self.subscribers[3] = v,
            Key::Delta => self.delta = v,
            Key::Players => self.players = v,
            Key::Conversion => self.conv = v,
            Key::Average => self.avg = v,
            Key::Paid => self.paid = v,
            Key::Hours => self.hours = v,
            Key::Updates => self.updates = v,
            Key::Lift => self.lift = v,
            Key::Others => self.others = v,
            Key::Score => self.score = v,
            Key::Market => self.market = v,
            Key::Labor => self.labor = v,
            Key::Hosted => self.hosted = v,
            Key::HoursPerPlayer => self.hours_per_player = v,
        }
    }
}

/// A choice from a short list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Pack,
    Node,
    Scale,
    Rule,
}

const PACK_VALUES: [&str; 5] = ["starter", "standard", "mega", "super", "ultra"];

impl Pick {
    pub fn label(self) -> &'static str {
        match self {
            Pick::Pack => "Ticket pack bought",
            Pick::Node => "Studio node",
            Pick::Scale => "Receipts",
            Pick::Rule => "Sales score rule",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Pick::Pack | Pick::Scale => Group::Receipts,
            Pick::Node => Group::Creator,
            Pick::Rule => Group::Admin,
        }
    }

    /// The choices as (value, words).
    pub fn options(self) -> Vec<(&'static str, String)> {
        match self {
            Pick::Pack => PACK_VALUES
                .iter()
                .zip(PACKS.iter())
                .map(|(v, p)| (*v, format!("{} {}, {} Tickets", p.name, price_words(p.usd), commas(p.tickets as i64))))
                .collect(),
            Pick::Node => vec![("light", "Light, 1.0".to_string()), ("full", "Full, 1.1".to_string())],
            Pick::Scale => vec![("hand", "Set by the sliders above".to_string()), ("roblox", "A share of Roblox's market".to_string())],
            Pick::Rule => vec![
                ("today", "Today: stops at 9,600 a day".to_string()),
                ("no_daily_cap", "Proposed: no daily stop".to_string()),
                ("diminishing", "Proposed: slower above 9,600 a day".to_string()),
            ],
        }
    }
}

impl Inputs {
    /// The value of a choice, as one of the `options` values.
    pub fn picked(&self, pick: Pick) -> &'static str {
        match pick {
            Pick::Pack => PACK_VALUES[self.pack.min(PACK_VALUES.len() - 1)],
            Pick::Node => match self.node {
                NodeKind::Light => "light",
                NodeKind::Full => "full",
            },
            Pick::Scale => match self.scale {
                Scale::ByHand => "hand",
                Scale::Roblox => "roblox",
            },
            Pick::Rule => match self.rule {
                SalesRule::Today => "today",
                SalesRule::NoDailyCap => "no_daily_cap",
                SalesRule::Diminishing => "diminishing",
            },
        }
    }

    /// Choose by value. A value that is not one of the options changes nothing.
    pub fn pick(&mut self, pick: Pick, value: &str) {
        match (pick, value) {
            (Pick::Pack, v) => {
                if let Some(i) = PACK_VALUES.iter().position(|x| *x == v) {
                    self.pack = i;
                }
            }
            (Pick::Node, "light") => self.node = NodeKind::Light,
            (Pick::Node, "full") => self.node = NodeKind::Full,
            (Pick::Scale, "hand") => self.scale = Scale::ByHand,
            (Pick::Scale, "roblox") => self.scale = Scale::Roblox,
            (Pick::Rule, "today") => self.rule = SalesRule::Today,
            (Pick::Rule, "no_daily_cap") => self.rule = SalesRule::NoDailyCap,
            (Pick::Rule, "diminishing") => self.rule = SalesRule::Diminishing,
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// Words for money (shared with the chart)
// -----------------------------------------------------------------------------

/// A whole number with thousands separators: 1234567 is "1,234,567".
pub fn commas(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Whole dollars: "$1,234".
pub fn dollars(n: i64) -> String {
    if n < 0 {
        format!("-${}", commas(-n))
    } else {
        format!("${}", commas(n))
    }
}

/// Dollars and cents from a count of cents: "$1,234.56". From $10,000 up the cents are dropped, as on the charts.
pub fn cents_words(cents: i64) -> String {
    if cents.abs() >= 1_000_000 {
        return dollars((cents as f64 / 100.0).round() as i64);
    }
    let sign = if cents < 0 { "-" } else { "" };
    let c = cents.abs();
    format!("{sign}${}.{:02}", commas(c / 100), c % 100)
}

/// A pack's price the way the packs are priced: "$4.99".
pub fn price_words(usd: f64) -> String {
    format!("${usd:.2}")
}

// -----------------------------------------------------------------------------
// 3. A month of Eustress
// -----------------------------------------------------------------------------

/// The figures a label prints, whole dollars, worked in the order the money moves from rounded steps so every printed line adds up to
/// the dollar: gross less fees is net, and the two halves make net.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyShown {
    pub gross: i64,
    pub fees: i64,
    pub net: i64,
    /// The Bliss Treasury's half, which the Treasury pays to contributors.
    pub treasury: i64,
    pub eustress: i64,
}

/// A month of Eustress, in dollars.
#[derive(Clone, Debug, PartialEq)]
pub struct Company {
    /// Receipts by kind, after the scale is applied: Ticket packs, Delta packs and gifts to the Treasury (plans are `plan_revenue`).
    pub tickets_usd: f64,
    pub delta_usd: f64,
    pub gifts_usd: f64,
    /// Drawn from the Worker's statement of a real period, not from the sliders.
    pub actual: bool,
    /// Daily players behind the month: a share of Roblox's, or the receipts at Roblox's spend per player.
    pub daily_players: f64,
    pub subscribers: f64,
    pub plan_revenue: f64,
    pub fee_tickets: f64,
    pub fee_plans: f64,
    pub fee_delta: f64,
    pub gross: f64,
    pub fees: f64,
    pub net: f64,
    /// Half of net: the Bliss Treasury's share of sales.
    pub treasury_share: f64,
    /// The other half: Eustress's.
    pub eustress: f64,
    /// What the Treasury pays contributors in a month, at a steady state: what comes in goes out.
    pub drip: f64,
    pub shown: CompanyShown,
}

/// The receipts a month runs on: the sliders, or a share of Roblox's monthly bookings split as the sliders split them (an even
/// split when every receipt slider is at zero).
pub fn scaled(s: &Inputs) -> Inputs {
    if s.scale != Scale::Roblox {
        return s.clone();
    }
    let target = ROBLOX_BOOKINGS_MONTH_USD * s.market / 100.0;
    let plans: f64 = PLANS.iter().zip(s.subscribers).map(|(p, n)| n * p.usd).sum();
    let whole = s.tickets + plans + s.delta;
    let mut out = s.clone();
    if whole > 0.0 {
        let k = target / whole;
        out.tickets = s.tickets * k;
        out.delta = s.delta * k;
        for n in out.subscribers.iter_mut() {
            *n *= k;
        }
    } else {
        out.tickets = target / 3.0;
        out.delta = target / 3.0;
        let each = target / 3.0 / PLANS.len() as f64;
        for (n, p) in out.subscribers.iter_mut().zip(PLANS.iter()) {
            *n = each / p.usd;
        }
    }
    out
}

pub fn company(s: &Inputs) -> Company {
    let daily_players_share = if s.scale == Scale::Roblox { Some(ROBLOX_DAU * s.market / 100.0) } else { None };
    let s = &scaled(s);
    let pack = &PACKS[s.pack.min(PACKS.len() - 1)];
    let fee_tickets = s.tickets * STRIPE_PERCENT + (s.tickets / pack.usd) * STRIPE_FIXED_USD;
    let mut plan_revenue = 0.0;
    let mut fee_plans = 0.0;
    let mut subscribers = 0.0;
    for (plan, n) in PLANS.iter().zip(s.subscribers) {
        plan_revenue += n * plan.usd;
        fee_plans += n * (plan.usd * STRIPE_PERCENT + STRIPE_FIXED_USD);
        subscribers += n;
    }
    let fee_delta = s.delta * STRIPE_PERCENT + (s.delta / DELTA_ORDER_USD) * STRIPE_FIXED_USD;
    let gross = s.tickets + plan_revenue + s.delta;
    let fees = fee_tickets + fee_plans + fee_delta;
    let net = gross - fees;
    let treasury_share = net / 2.0;
    let eustress = net - treasury_share;

    // The printed dollars, worked in order from rounded steps.
    let d_gross = gross.round() as i64;
    let d_fees = fees.round() as i64;
    let d_net = d_gross - d_fees;
    let d_treasury = d_net.div_euclid(2);
    let shown = CompanyShown { gross: d_gross, fees: d_fees, net: d_net, treasury: d_treasury, eustress: d_net - d_treasury };

    let daily_players = daily_players_share.unwrap_or(gross / spend_per_daily_player());
    Company {
        tickets_usd: s.tickets,
        delta_usd: s.delta,
        gifts_usd: 0.0,
        actual: false,
        daily_players,
        subscribers,
        plan_revenue,
        fee_tickets,
        fee_plans,
        fee_delta,
        gross,
        fees,
        net,
        treasury_share,
        eustress,
        drip: treasury_share,
        shown,
    }
}

// -----------------------------------------------------------------------------
// Where Eustress's half goes (admin view)
// -----------------------------------------------------------------------------

/// A month's costs out of Eustress's half, in dollars. `retained` is what is left, negative when the costs are larger.
#[derive(Clone, Debug, PartialEq)]
pub struct Expenses {
    pub half: f64,
    pub player_hours: f64,
    pub servers: f64,
    pub cloudflare: f64,
    pub domain: f64,
    pub tools: f64,
    pub other: f64,
    pub labor: f64,
    pub retained: f64,
}

impl Expenses {
    pub fn total(&self) -> f64 {
        self.servers + self.cloudflare + self.domain + self.tools + self.other + self.labor
    }
}

/// Theoretical: the costs the hosting plan's assumptions give for the month's daily players, and labor at its share of the half.
pub fn expenses(s: &Inputs, m: &Company) -> Expenses {
    let player_hours = m.daily_players * s.hours_per_player * DAYS;
    let hosted = s.hosted / 100.0;
    let servers = FLEET_FLOOR_USD + player_hours * ((1.0 - hosted) * SERVER_USD_PER_PLAYER_HOUR + hosted * RELAY_USD_PER_PLAYER_HOUR);
    let cloudflare = CLOUDFLARE_PLAN_USD + player_hours * CONTROL_PLANE_USD_PER_PLAYER_HOUR;
    // the half as the month chart prints it, so the two charts agree to the dollar
    let half = m.shown.eustress as f64;
    let labor = half.max(0.0) * s.labor / 100.0;
    let mut e = Expenses { half, player_hours, servers, cloudflare, domain: DOMAIN_USD, tools: 0.0, other: 0.0, labor, retained: 0.0 };
    e.retained = e.half - e.total();
    e
}


// -----------------------------------------------------------------------------
// 4. One creator
// -----------------------------------------------------------------------------

/// The Ticket counts a label prints, whole Tickets: what buyers spend, what the creator earns, what leaves circulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketsShown {
    pub spent: i64,
    pub earned: i64,
    pub platform: i64,
}

/// The Bliss figures a label prints, whole cents, worked in order so the two parts add up to the payout to the cent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlissShown {
    pub effort: i64,
    pub value: i64,
    pub bliss: i64,
}

/// What one creator is paid in a month: Tickets for sales, and a share of the Treasury's payout in dollars.
#[derive(Clone, Debug, PartialEq)]
pub struct Creator {
    pub players: f64,
    pub buyers: f64,
    pub spent: f64,
    pub spent_paid: f64,
    pub earned: f64,
    /// Tickets earned from sales paid with bought Tickets: the part that earns sales score.
    pub eligible: f64,
    /// The part of `eligible` that scores: one buyer's Tickets count up to the per-buyer cap, taking each buyer to spend the average in one day.
    pub counted: f64,
    pub value_score: f64,
    pub effort: f64,
    pub mine: f64,
    pub total: f64,
    /// The share of a day's payout that is paid at all: the network's score over a full day's 1,440, at most 1.
    pub gate: f64,
    pub pool: f64,
    pub bliss_effort: f64,
    pub bliss_value: f64,
    pub bliss: f64,
    pub tickets: TicketsShown,
    pub shown: BlissShown,
}

/// Whole units, without the float dust a product of percentages leaves behind.
fn whole(x: f64) -> f64 {
    ((x * 1e6).round() / 1e6).floor()
}

/// A month's sales score under a rule, from the score the counted Tickets earn before any daily stop.
pub fn sales_score(raw: f64, rule: SalesRule) -> f64 {
    let cap = VALUE_CAP_PER_DAY * DAYS;
    match rule {
        SalesRule::Today => raw.min(cap),
        SalesRule::NoDailyCap => raw,
        SalesRule::Diminishing => {
            if raw <= cap {
                raw
            } else {
                whole(cap * (1.0 + (raw / cap).ln()))
            }
        }
    }
}

pub fn creator(s: &Inputs, m: &Company) -> Creator {
    let players = s.players * (1.0 + s.lift / 100.0 * s.updates);
    let buyers = players * s.conv / 100.0;
    let spent = buyers * s.avg;
    let spent_paid = spent * s.paid / 100.0;
    let earned = whole(spent * CREATOR_SHARE);
    let eligible = whole(spent_paid * CREATOR_SHARE);
    let counted = whole(buyers * (s.avg * s.paid / 100.0 * CREATOR_SHARE).min(VALUE_CAP_TICKETS_PER_BUYER));
    let value_score = sales_score(whole(VALUE_PER_TICKET * counted), s.rule);
    let effort = (s.hours * 60.0 * effort_per_minute() * s.node.bonus()).min(EFFORT_CAP_PER_DAY * DAYS);
    let mine = effort + value_score;
    let total = s.others * s.score * DAYS + mine;
    let gate = (total / DAYS / FULL_DAY_SCORE).min(1.0);
    let pool = m.drip * gate;
    let bliss_effort = if total > 0.0 { pool * effort / total } else { 0.0 };
    let bliss_value = if total > 0.0 { pool * value_score / total } else { 0.0 };
    let bliss = bliss_effort + bliss_value;

    let spent_whole = spent.round() as i64;
    let earned_whole = earned as i64;
    let cents = |x: f64| (x * 100.0).round() as i64;
    let c_effort = cents(bliss_effort);
    let c_value = cents(bliss_effort + bliss_value) - c_effort;

    Creator {
        players,
        buyers,
        spent,
        spent_paid,
        earned,
        eligible,
        counted,
        value_score,
        effort,
        mine,
        total,
        gate,
        pool,
        bliss_effort,
        bliss_value,
        bliss,
        tickets: TicketsShown { spent: spent_whole, earned: earned_whole, platform: (spent_whole - earned_whole).max(0) },
        shown: BlissShown { effort: c_effort, value: c_value, bliss: c_effort + c_value },
    }
}

/// A creator to start from: three sizes of the same creator. Presets move only the creator's controls.
pub struct Preset {
    pub name: &'static str,
    pub values: [(Key, f64); 5],
}

pub static PRESETS: [Preset; 3] = [
    Preset { name: "Small", values: [(Key::Players, 500.0), (Key::Conversion, 2.0), (Key::Average, 150.0), (Key::Hours, 20.0), (Key::Updates, 1.0)] },
    Preset { name: "Medium", values: [(Key::Players, 5000.0), (Key::Conversion, 3.0), (Key::Average, 300.0), (Key::Hours, 60.0), (Key::Updates, 2.0)] },
    Preset { name: "Large", values: [(Key::Players, 50_000.0), (Key::Conversion, 4.0), (Key::Average, 400.0), (Key::Hours, 120.0), (Key::Updates, 4.0)] },
];

impl Inputs {
    pub fn apply(&mut self, preset: &Preset) {
        for (key, value) in preset.values {
            self.set(key, value);
        }
    }
}

// -----------------------------------------------------------------------------
// 5. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64, tol: f64, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: got {a}, want {b}");
    }

    // ---- the default month, worked by hand ----

    #[test]
    fn the_default_month_matches_the_hand_worked_numbers() {
        let s = Inputs::default();
        let m = company(&s);
        near(m.gross, 100_000.0 + (3000.0 * 4.99 + 1500.0 * 9.99 + 800.0 * 12.99 + 5.0 * 129.0) + 38_000.0, 1e-6, "gross");
        near(m.gross, 178_992.0, 0.01, "gross, literal");
        // Stripe 2.9% + $0.30 a payment: packs of $9.99, each invoice at its own price, Delta orders of $25.
        let fee_t = 100_000.0 * 0.029 + 0.30 * (100_000.0 / 9.99);
        let fee_p = 3000.0 * (4.99 * 0.029 + 0.3) + 1500.0 * (9.99 * 0.029 + 0.3) + 800.0 * (12.99 * 0.029 + 0.3) + 5.0 * (129.0 * 0.029 + 0.3);
        let fee_d = 38_000.0 * 0.029 + 0.3 * 38_000.0 / 25.0;
        near(m.fee_tickets, fee_t, 1e-6, "fee on packs");
        near(m.fee_plans, fee_p, 1e-6, "fee on plan invoices");
        near(m.fee_delta, fee_d, 1e-6, "fee on Delta packs");
        near(m.fees, fee_t + fee_p + fee_d, 1e-6, "fees");
        near(m.fees, 10_241.271, 0.01, "fees, literal");
        near(m.net, 168_750.729, 0.01, "net, literal");
        near(m.treasury_share, 84_375.3645, 0.01, "the Treasury's half, literal");
        near(m.eustress, 84_375.3645, 0.01, "Eustress's half, literal");
        near(m.drip, m.treasury_share, 1e-9, "the Treasury pays out what comes in");
        near(m.subscribers, 5305.0, 1e-9, "subscribers");
        let d = &m.shown;
        assert_eq!((d.gross, d.fees, d.net, d.treasury, d.eustress), (178_992, 10_241, 168_751, 84_375, 84_376));
    }

    #[test]
    fn the_default_creator_matches_the_hand_worked_numbers() {
        let s = Inputs::default();
        let m = company(&s);
        let c = creator(&s, &m);
        near(c.players, 5500.0, 1e-9, "players with updates");
        near(c.buyers, 165.0, 1e-9, "buyers");
        near(c.spent, 49_500.0, 1e-6, "Tickets spent");
        assert_eq!(c.earned, 34_650.0);
        assert_eq!(c.eligible, 31_185.0);
        near(c.effort, 60.0 * 60.0 * 2.4, 1e-9, "effort score");
        assert_eq!(c.value_score, 15_592.0);
        near(c.total, 300.0 * 500.0 * 30.0 + 8640.0 + 15_592.0, 1e-9, "the network's score");
        near(c.bliss, m.drip * (8640.0 + 15_592.0) / (4_500_000.0 + 8640.0 + 15_592.0), 1e-6, "Bliss share");
        near(c.bliss, 451.918, 0.001, "Bliss share, literal");
        assert_eq!(c.tickets, TicketsShown { spent: 49_500, earned: 34_650, platform: 14_850 });
        assert_eq!(c.shown, BlissShown { effort: 16_113, value: 29_079, bliss: 45_192 });
    }

    #[test]
    fn a_sale_pays_the_creator_seventy_percent_in_tickets_and_the_rest_leaves() {
        assert_eq!(CREATOR_SHARE, 0.7, "the Worker's rule today");
        let s = Inputs::default();
        let c = creator(&s, &company(&s));
        assert_eq!(c.tickets.earned + c.tickets.platform, c.tickets.spent);
        near(c.tickets.earned as f64 / c.tickets.spent as f64, 0.7, 0.001, "the creator's share of spend");
    }

    #[test]
    fn the_three_presets_are_three_sizes_and_each_earns_more_than_the_one_before() {
        let months: Vec<i64> = PRESETS
            .iter()
            .map(|p| {
                let mut i = Inputs::default();
                i.apply(p);
                let m = company(&i);
                creator(&i, &m).shown.bliss
            })
            .collect();
        assert_eq!(months, [6_325, 45_192, 536_038], "cents a month: $63.25, $451.92 and $5,360.38 on the example network");
        assert_eq!(PRESETS.iter().map(|p| p.name).collect::<Vec<_>>(), ["Small", "Medium", "Large"]);
    }

    #[test]
    fn a_creators_sales_score_stops_at_the_per_buyer_cap_and_at_the_daily_cap() {
        // the Worker's constants, as bliss.mjs states them
        assert_eq!((VALUE_PER_TICKET, VALUE_CAP_TICKETS_PER_BUYER, VALUE_CAP_PER_DAY), (0.5, 1000.0, 9600.0));
        // 100 buyers who each spend 2,000 Tickets (1,400 of them the creator's) count 1,000 each: 100,000 of the 140,000 earned
        let mut s = Inputs::default();
        s.players = 1000.0;
        s.lift = 0.0;
        s.conv = 10.0;
        s.avg = 2000.0;
        s.paid = 100.0;
        let c = creator(&s, &company(&s));
        assert_eq!((c.buyers, c.eligible, c.counted, c.value_score), (100.0, 140_000.0, 100_000.0, 50_000.0));
        // below the cap every Ticket counts
        s.avg = 1000.0;
        let c = creator(&s, &company(&s));
        assert_eq!((c.eligible, c.counted, c.value_score), (70_000.0, 70_000.0, 35_000.0));
        // 1,428 Tickets is where a buyer's share (999.6) reaches the cap
        s.avg = 1428.0;
        let c = creator(&s, &company(&s));
        assert!(c.counted == c.eligible && c.counted == 99_960.0, "{} {}", c.counted, c.eligible);
        s.avg = 1429.0;
        let c = creator(&s, &company(&s));
        assert_eq!((c.eligible, c.counted), (100_030.0, 100_000.0));
        // the daily cap: a hit creator earns 302,400 of sales score, 9,600 a day stops it at 288,000 in a month
        let mut big = Inputs::default();
        big.apply(&PRESETS[2]);
        let c = creator(&big, &company(&big));
        assert_eq!((c.counted, c.value_score), (604_800.0, 288_000.0));
        assert!(VALUE_PER_TICKET * c.counted > c.value_score);
    }

    #[test]
    fn effort_stops_at_the_daily_ceiling_whatever_the_hours() {
        // the Hours slider stops at 300 a month, short of the 3,200 a day the Worker allows, so the ceiling is reached only through the field
        let mut s = Inputs::default();
        s.hours = 1000.0;
        let c = creator(&s, &company(&s));
        assert_eq!(c.effort, EFFORT_CAP_PER_DAY * DAYS);
        s.hours = control(Key::Hours).max;
        let c = creator(&s, &company(&s));
        assert!(c.effort < EFFORT_CAP_PER_DAY * DAYS, "the slider's own maximum is under the ceiling: {}", c.effort);
        // a Full node earns a tenth more on the same hours
        let mut full = Inputs::default();
        full.pick(Pick::Node, "full");
        let (light, full) = (creator(&Inputs::default(), &company(&Inputs::default())).effort, creator(&full, &company(&full)).effort);
        assert!((full / light - 1.1).abs() < 1e-9, "{full} against {light}");
    }

    // ---- Roblox, for scale ----

    #[test]
    fn the_players_ceiling_is_32_percent_of_robloxs_daily_players() {
        assert_eq!(PLAYERS_MAX, ROBLOX_DAU * ROBLOX_SHARE_MARK / 100.0);
        assert_eq!(control(Key::Players).max, PLAYERS_MAX);
        near(spend_per_daily_player(), 1_600_000_000.0 / 3.0 / 123_000_000.0, 1e-12, "Roblox's bookings per daily player a month");
        near(spend_per_daily_player(), 4.336, 0.001, "about $4.34");
    }

    #[test]
    fn a_share_of_roblox_sets_the_month_and_keeps_the_sliders_mix() {
        let hand = Inputs::default();
        let mut r = Inputs::default();
        r.pick(Pick::Scale, "roblox");
        r.set(Key::Market, 32.0);
        let (mh, mr) = (company(&hand), company(&r));
        near(mr.gross, ROBLOX_BOOKINGS_MONTH_USD * 0.32, 1e-3, "32% of Roblox's month");
        near(mr.gross, 170_666_666.67, 0.01, "literal");
        near(mr.daily_players, 39_360_000.0, 1e-6, "32% of Roblox's daily players");
        // the mix is the sliders'
        near(mr.tickets_usd / mr.gross, mh.tickets_usd / mh.gross, 1e-12, "Ticket packs' share");
        near(mr.delta_usd / mr.gross, mh.delta_usd / mh.gross, 1e-12, "Delta packs' share");
        near(mr.plan_revenue / mr.gross, mh.plan_revenue / mh.gross, 1e-12, "plans' share");
        // the fee and the split still apply
        near(mr.treasury_share + mr.eustress, mr.net, 1e-3, "halves");
        assert!(mr.fees > 0.0 && mr.fees < mr.gross * 0.06);
        // by hand, daily players follow the receipts at Roblox's spend per player
        near(mh.daily_players, mh.gross / spend_per_daily_player(), 1e-9, "daily players by hand");
        near(mh.daily_players, 41_280.0, 1.0, "about 41,280");
        // nothing on the sliders splits a share of Roblox evenly
        let mut z = r.clone();
        for k in [Key::Tickets, Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            z.set(k, 0.0);
        }
        let mz = company(&z);
        near(mz.tickets_usd, mz.gross / 3.0, 1e-3, "a third in packs");
        near(mz.delta_usd, mz.gross / 3.0, 1e-3, "a third in Delta");
        near(mz.plan_revenue, mz.gross / 3.0, 1e-3, "a third in plans");
        // and the share moves the month in proportion
        r.set(Key::Market, 16.0);
        near(company(&r).gross * 2.0, mr.gross, 1e-3, "half the share, half the month");
    }

    #[test]
    fn the_players_slider_moves_on_a_log_scale_and_lands_on_round_numbers() {
        let c = control(Key::Players);
        assert!(c.log);
        assert_eq!(c.input_range(), (0.0, LOG_POSITIONS, 1.0));
        assert_eq!(c.from_position(0.0), 0.0);
        assert_eq!(c.from_position(LOG_POSITIONS), PLAYERS_MAX);
        assert_eq!(c.from_position(1.0), 100.0);
        // the presets' sizes are found again from their own positions
        for v in [500.0, 5_000.0, 50_000.0, 1_000_000.0] {
            assert_eq!(c.from_position(c.position(v)), v, "{v}");
        }
        // every position is a two-figure number, and they only grow
        let mut last = -1.0;
        for p in 0..=1000 {
            let v = c.from_position(p as f64);
            assert!(v >= last, "position {p}");
            last = v;
            if v > 0.0 && v < PLAYERS_MAX {
                let digits = format!("{}", v as i64).trim_end_matches('0').len();
                assert!(digits <= 2, "{v} at {p}");
            }
        }
        // a range input reports a position; the control takes the value
        let mut s = Inputs::default();
        s.set_text(Key::Players, &format!("{}", c.position(50_000.0)));
        assert_eq!(s.players, 50_000.0);
        s.set_text(Key::Players, "1000");
        assert_eq!(s.players, PLAYERS_MAX);
        // a linear slider is unchanged
        let t = control(Key::Tickets);
        assert_eq!((t.position(12_000.0), t.from_position(12_000.0), t.input_range()), (12_000.0, 12_000.0, (0.0, 300_000.0, 1000.0)));
    }

    // ---- the sales score rules ----

    #[test]
    fn the_sales_score_rules_agree_below_the_daily_stop_and_differ_above_it() {
        let cap = VALUE_CAP_PER_DAY * DAYS;
        for rule in [SalesRule::Today, SalesRule::NoDailyCap, SalesRule::Diminishing] {
            assert_eq!(sales_score(15_592.0, rule), 15_592.0, "{rule:?} below the stop");
            assert_eq!(sales_score(cap, rule), cap, "{rule:?} at the stop");
        }
        let raw = cap * 10.0;
        assert_eq!(sales_score(raw, SalesRule::Today), cap);
        assert_eq!(sales_score(raw, SalesRule::NoDailyCap), raw);
        assert_eq!(sales_score(raw, SalesRule::Diminishing), (cap * (1.0 + 10f64.ln())).floor());
        // slower is still more: popularity keeps paying above the stop
        assert!(sales_score(cap * 2.0, SalesRule::Diminishing) > cap && sales_score(cap * 4.0, SalesRule::Diminishing) > sales_score(cap * 2.0, SalesRule::Diminishing));
        // a hit creator: today the payout stops growing with players; under a proposal it keeps growing
        let paid = |rule: SalesRule, players: f64| {
            let mut s = Inputs::default();
            s.apply(&PRESETS[2]);
            s.rule = rule;
            s.players = players;
            creator(&s, &company(&s)).bliss
        };
        assert_eq!(paid(SalesRule::Today, 1_000_000.0), paid(SalesRule::Today, 10_000_000.0), "today: more players, same payout");
        assert!(paid(SalesRule::NoDailyCap, 10_000_000.0) > paid(SalesRule::NoDailyCap, 1_000_000.0));
        assert!(paid(SalesRule::Diminishing, 10_000_000.0) > paid(SalesRule::Diminishing, 1_000_000.0));
        assert!(paid(SalesRule::Diminishing, 10_000_000.0) < paid(SalesRule::NoDailyCap, 10_000_000.0));
        // and the proposals never pay more than the whole payout
        assert!(paid(SalesRule::NoDailyCap, PLAYERS_MAX) <= company(&Inputs::default()).drip);
    }

    // ---- costs (admin view) ----

    #[test]
    fn the_costs_follow_the_hosting_plan_and_labor_is_a_share_of_eustresss_half() {
        let s = Inputs::default();
        let m = company(&s);
        let e = expenses(&s, &m);
        near(e.player_hours, m.daily_players * 2.4 * 30.0, 1e-6, "player hours");
        near(e.servers, 470.0 + e.player_hours * (0.3 * 0.0017 + 0.7 * 0.0001), 1e-6, "servers: the fleet, our servers and the relay");
        near(e.cloudflare, 5.0 + e.player_hours * 0.00005, 1e-9, "Cloudflare");
        near(e.domain, 10.0 / 12.0, 1e-12, "the domain");
        near(e.labor, m.shown.eustress as f64 * 0.30, 1e-6, "labor at 30% of the half");
        near(e.retained, e.half - e.total(), 1e-9, "what is left");
        near(e.half, 84_376.0, 0.0, "the half, as the month chart prints it");
        // literal, worked by hand: 41,280 daily players, 2,972,160 player hours
        near(e.player_hours, 2_972_160.0, 50.0, "player hours, literal");
        near(e.servers, 2_193.85, 0.1, "servers, literal");
        near(e.cloudflare, 153.61, 0.1, "Cloudflare, literal");
        // all on Eustress's servers costs more; labor does not change the server bill
        let mut ours = s.clone();
        ours.hosted = 0.0;
        assert!(expenses(&ours, &m).servers > e.servers);
        let mut no_labor = s.clone();
        no_labor.labor = 0.0;
        let e0 = expenses(&no_labor, &m);
        near(e0.labor, 0.0, 0.0, "no labor");
        near(e0.servers, e.servers, 1e-9, "same servers");
        // a month that loses money says so
        let mut big = Inputs::default();
        big.pick(Pick::Scale, "roblox");
        big.market = 0.01;
        let mb = company(&big);
        let mut tiny_half = big.clone();
        tiny_half.labor = 100.0;
        assert!(expenses(&tiny_half, &mb).retained < 0.0, "labor of the whole half and servers on top is a loss");
    }

    // ---- properties over many months ----

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f64) / ((1u64 << 31) as f64)
        }
        fn pick(&mut self, c: &Control) -> f64 {
            let n = ((c.max - c.min) / c.step).floor();
            c.min + (self.next() * (n + 1.0)).floor().min(n) * c.step
        }
    }

    fn random_month(rng: &mut Lcg) -> Inputs {
        let mut s = Inputs::default();
        for c in CONTROLS.iter() {
            let v = rng.pick(c);
            s.set(c.key, v);
        }
        s.pack = (rng.next() * 5.0).floor().min(4.0) as usize;
        s.node = if rng.next() < 0.5 { NodeKind::Light } else { NodeKind::Full };
        s
    }

    #[test]
    fn money_is_conserved_down_the_month_and_through_a_creator_in_four_thousand_random_months() {
        let mut rng = Lcg(12345);
        for _ in 0..4000 {
            let s = random_month(&mut rng);
            let m = company(&s);
            let c = creator(&s, &m);
            let floats = [m.gross, m.fees, m.net, m.treasury_share, m.eustress, m.drip, c.players, c.buyers, c.spent, c.earned, c.eligible, c.counted, c.value_score, c.effort, c.total, c.gate, c.pool, c.bliss];
            assert!(floats.iter().all(|x| x.is_finite() && *x >= 0.0), "finite, non-negative numbers in {s:?}");
            near(m.gross - m.fees, m.net, 1e-6, "net");
            near(m.treasury_share + m.eustress, m.net, 1e-6, "the two halves make net");
            near(m.drip, m.treasury_share, 1e-9, "the drip is the Treasury's half");
            // the printed dollars add up exactly
            let d = &m.shown;
            assert_eq!(d.gross - d.fees, d.net, "net in {s:?}");
            assert_eq!(d.treasury + d.eustress, d.net, "halves in {s:?}");
            // one creator
            assert_eq!(c.shown.effort + c.shown.value, c.shown.bliss, "Bliss split in {s:?}");
            assert_eq!(c.tickets.earned + c.tickets.platform, c.tickets.spent, "Tickets split in {s:?}");
            assert!(c.eligible <= c.earned, "credit from paid spending is part of what is earned");
            assert!(c.counted <= c.eligible + 1e-9, "the score counts no more than was earned");
            assert!(c.value_score <= VALUE_PER_TICKET * c.counted + 1e-9 && c.value_score <= VALUE_CAP_PER_DAY * DAYS, "the sales score is capped");
            assert!(c.bliss_effort + c.bliss_value <= m.drip + 1e-9, "a creator never gets more than the payout");
            assert!((0.0..=1.0).contains(&c.gate), "the gate is a share");
            // a day pays out its allowance x min(1, score / 1,440) split by score: a quiet network pays a contributor for their own score
            // over a full day, a busy one pays by share of the total
            if c.total > 0.0 && c.gate < 1.0 {
                near(c.bliss, m.drip * c.mine / (DAYS * FULL_DAY_SCORE), 1e-6, "a quiet network pays score over 1,440 a day");
            }
            if c.total > 0.0 && c.gate == 1.0 {
                near(c.bliss, m.drip * c.mine / c.total, 1e-6, "a busy network pays by share");
            }
        }
    }

    // ---- edges ----

    #[test]
    fn an_empty_month_is_empty_and_has_no_nan() {
        let mut z = Inputs::default();
        for k in [Key::Tickets, Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta, Key::Players, Key::Hours, Key::Others] {
            z.set(k, 0.0);
        }
        let m = company(&z);
        let c = creator(&z, &m);
        assert!(m.gross == 0.0 && m.drip == 0.0 && c.bliss == 0.0);
        assert!(c.bliss.is_finite() && c.gate.is_finite() && c.pool.is_finite());
    }

    #[test]
    fn a_lone_contributor_on_a_quiet_network_is_paid_for_their_own_score() {
        // 10 hours is 1,440 score a month; 100 a day elsewhere is a quiet network. A thirtieth of the payout, for one full day's score.
        let mut q = Inputs::default();
        q.set(Key::Players, 0.0);
        q.set(Key::Hours, 10.0);
        q.set(Key::Others, 1.0);
        q.set(Key::Score, 100.0);
        q.set(Key::Updates, 0.0);
        let m = company(&q);
        let c = creator(&q, &m);
        assert!(c.gate < 1.0);
        near(c.bliss, m.drip / 30.0, 1e-6, "a thirtieth of the payout");
    }

    #[test]
    fn the_share_moves_with_the_number_of_other_contributors_more_than_with_anything_else() {
        let share = |others: f64| {
            let mut s = Inputs::default();
            s.set(Key::Others, others);
            creator(&s, &company(&s)).bliss
        };
        assert!(share(0.0) > share(10.0) && share(10.0) > share(300.0) && share(300.0) > share(5000.0));
        assert!(share(0.0) / share(300.0) > 50.0, "alone, the same creator is paid far more of the same payout");
    }

    #[test]
    fn hours_pay_only_through_the_bliss_payout_and_updates_only_through_players() {
        let base = Inputs::default();
        let bliss = |f: &dyn Fn(&mut Inputs)| {
            let mut s = base.clone();
            f(&mut s);
            let m = company(&s);
            creator(&s, &m)
        };
        let none = bliss(&|s| s.set(Key::Hours, 0.0));
        assert_eq!(none.bliss_effort, 0.0, "no hours, no effort share");
        assert!(none.bliss_value > 0.0, "sales still earn a share");
        let more = bliss(&|s| s.set(Key::Hours, 150.0));
        assert!(more.bliss_effort > creator(&base, &company(&base)).bliss_effort);
        // an update has no score of its own: with no lift it changes nothing
        let a = bliss(&|s| {
            s.set(Key::Lift, 0.0);
            s.set(Key::Updates, 0.0);
        });
        let b = bliss(&|s| {
            s.set(Key::Lift, 0.0);
            s.set(Key::Updates, 12.0);
        });
        assert_eq!(a.bliss, b.bliss);
        let c = bliss(&|s| s.set(Key::Updates, 12.0));
        assert!(c.players > a.players && c.spent > a.spent, "updates matter through the players they bring back");
    }

    // ---- the controls ----

    #[test]
    fn every_control_has_a_range_that_holds_its_default_and_a_step_that_reaches_it() {
        let d = Inputs::default();
        for c in CONTROLS.iter() {
            assert!(c.min < c.max && c.step > 0.0, "{} has a range", c.label);
            let v = d.get(c.key);
            assert!(v >= c.min && v <= c.max, "{}: the default {v} is inside {} to {}", c.label, c.min, c.max);
            let steps = (v - c.min) / c.step;
            assert!((steps - steps.round()).abs() < 1e-9, "{}: the default {v} is on a step", c.label);
            assert!(!c.label.is_empty(), "every control has words");
        }
    }

    #[test]
    fn every_key_has_exactly_one_control_and_set_and_get_agree() {
        let keys: Vec<Key> = CONTROLS.iter().map(|c| c.key).collect();
        for (i, k) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(k), "{k:?} appears twice");
        }
        for c in CONTROLS.iter() {
            let mut s = Inputs::default();
            let target = c.min + c.step;
            s.set(c.key, target);
            assert_eq!(s.get(c.key), target, "{} round-trips", c.label);
            // and it moved nothing else
            let mut expected = Inputs::default();
            expected.set(c.key, target);
            assert_eq!(s, expected);
        }
    }

    #[test]
    fn every_control_and_every_choice_is_laid_out_once_in_its_own_group() {
        let mut sliders = Vec::new();
        let mut choices = Vec::new();
        for g in Group::ALL {
            for item in layout(g) {
                match item {
                    Item::Slider(k) => {
                        assert_eq!(control(k).group, g, "{k:?} sits in its own group");
                        sliders.push(k);
                    }
                    Item::Choice(p) => {
                        assert_eq!(p.group(), g, "{p:?} sits in its own group");
                        choices.push(p);
                    }
                }
            }
        }
        assert_eq!(sliders.len(), CONTROLS.len(), "every slider is laid out");
        for c in CONTROLS.iter() {
            assert_eq!(sliders.iter().filter(|k| **k == c.key).count(), 1, "{} appears once", c.label);
        }
        for p in [Pick::Pack, Pick::Node] {
            assert_eq!(choices.iter().filter(|x| **x == p).count(), 1, "{p:?} appears once");
        }
    }

    #[test]
    fn a_control_is_kept_inside_its_range_and_ignores_what_is_not_a_number() {
        let mut s = Inputs::default();
        s.set(Key::Conversion, 500.0);
        assert_eq!(s.conv, 20.0);
        s.set(Key::Conversion, -3.0);
        assert_eq!(s.conv, 0.0);
        s.set(Key::Conversion, 12.0);
        s.set(Key::Conversion, f64::NAN);
        s.set(Key::Conversion, f64::INFINITY);
        assert_eq!(s.conv, 12.0);
    }

    #[test]
    fn a_range_input_reports_text_and_the_control_follows_it() {
        let mut s = Inputs::default();
        s.set_text(Key::Tickets, "250000");
        assert_eq!(s.tickets, 250_000.0);
        s.set_text(Key::Hours, " 22.5 ");
        assert_eq!(s.hours, 22.5);
        s.set_text(Key::Tickets, "not a number");
        s.set_text(Key::Tickets, "");
        s.set_text(Key::Tickets, "NaN");
        assert_eq!(s.tickets, 250_000.0, "text that is not a number changes nothing");
        s.set_text(Key::Tickets, "9e99");
        assert_eq!(s.tickets, 300_000.0, "a number past the range is held at the end of it");
        s.set_text(Key::Tickets, "-5");
        assert_eq!(s.tickets, 0.0);
    }

    #[test]
    fn a_choice_is_set_by_value_and_an_unknown_value_changes_nothing() {
        let mut s = Inputs::default();
        for pick in [Pick::Pack, Pick::Node] {
            for (value, _) in pick.options() {
                s.pick(pick, value);
                assert_eq!(s.picked(pick), value);
            }
            let before = s.clone();
            s.pick(pick, "no such choice");
            assert_eq!(s, before);
        }
        assert_eq!(Pick::Pack.options().len(), PACKS.len());
        assert_eq!(Inputs::default().picked(Pick::Pack), "standard");
    }

    #[test]
    fn a_preset_moves_only_the_creators_controls() {
        let mut s = Inputs::default();
        s.apply(&PRESETS[2]);
        assert_eq!((s.players, s.conv, s.avg, s.hours, s.updates), (50_000.0, 4.0, 400.0, 120.0, 4.0));
        let mut expected = Inputs::default();
        expected.players = 50_000.0;
        expected.conv = 4.0;
        expected.avg = 400.0;
        expected.hours = 120.0;
        expected.updates = 4.0;
        assert_eq!(s, expected, "nothing else changed");
        for p in PRESETS.iter() {
            for (k, _) in p.values {
                assert_eq!(control(k).group, Group::Creator, "{} sets only creator controls", p.name);
            }
        }
        assert_eq!(PRESETS[1].values.map(|(k, v)| (k, v)), [(Key::Players, 5000.0), (Key::Conversion, 3.0), (Key::Average, 300.0), (Key::Hours, 60.0), (Key::Updates, 2.0)], "Medium is the default creator");
    }

    // ---- the constants tie to what the site and the Worker say ----

    #[test]
    fn the_plans_here_are_the_plans_the_premium_page_lists() {
        let tiers = crate::pages::premium::plan_tiers();
        let named = |id: &str| tiers.iter().find(|t| t.id == id).unwrap_or_else(|| panic!("the Premium page lists {id}"));
        for (plan, id) in PLANS.iter().zip(["player-plus", "creator", "bundle", "professional"]) {
            let t = named(id);
            assert_eq!(t.name, plan.name);
            near(t.price_monthly, plan.usd, 1e-9, plan.name);
        }
        // and the slider labels carry the same prices
        for (plan, key) in PLANS.iter().zip([Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional]) {
            let want = if plan.usd.fract() == 0.0 { format!("${}", plan.usd as i64) } else { format!("${:.2}", plan.usd) };
            assert!(control(key).label.contains(&want), "{} names its price {want}", control(key).label);
        }
    }

    #[test]
    fn the_pack_table_is_the_worker_table() {
        // index.js TICKET_PACKAGES: starter 400, standard 800+80, mega 1600+240, super 4000+1000, ultra 8000+2800.
        let want = [(4.99, 400.0), (9.99, 880.0), (19.99, 1840.0), (49.99, 5000.0), (99.99, 10_800.0)];
        for (p, (usd, tickets)) in PACKS.iter().zip(want) {
            assert_eq!((p.usd, p.tickets), (usd, tickets), "{}", p.name);
        }
        let worker = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../infrastructure/cloudflare/api/src/index.js"));
        if let Ok(js) = worker {
            for (p, (usd, tickets)) in PACKS.iter().zip(want) {
                let line = js.lines().find(|l| l.contains(&format!("name: '{}'", p.name)) && l.contains("total:")).unwrap_or_else(|| panic!("the Worker lists {}", p.name));
                assert!(line.contains(&format!("usd: {usd}")) && line.contains(&format!("total: {}", tickets as i64)), "the Worker's {} line differs: {line}", p.name);
            }
        }
    }

    // ---- words for money ----

    #[test]
    fn money_is_written_with_separators_and_the_cents_drop_from_ten_thousand() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(1_234_567), "1,234,567");
        assert_eq!(commas(-1_234), "-1,234");
        assert_eq!(dollars(178_992), "$178,992");
        assert_eq!(dollars(-5), "-$5");
        assert_eq!(cents_words(45_192), "$451.92");
        assert_eq!(cents_words(5), "$0.05");
        assert_eq!(cents_words(123_456), "$1,234.56");
        assert_eq!(cents_words(999_999), "$9,999.99");
        assert_eq!(cents_words(1_000_000), "$10,000");
        assert_eq!(cents_words(1_000_049), "$10,000");
        assert_eq!(Unit::Percent.show(0.5), "0.5%");
        assert_eq!(Unit::Percent.show(3.0), "3%");
        assert_eq!(Unit::Hours.show(60.0), "60 h");
        assert_eq!(Unit::Dollars.show(100_000.0), "$100,000");
        assert_eq!(Unit::Count.show(5000.0), "5,000");
        assert_eq!(price_words(4.99), "$4.99");
    }
}
