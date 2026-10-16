// =============================================================================
// Eustress Web - Economics DRAFT model (/economics-draft, feature `economics-draft`)
// =============================================================================
// The full internal model: one month of Eustress receipts, where each dollar goes (the reserve, Engine Expenses, tax, Treasury
// investments and their repayment), and what one creator takes home (the Tickets cash-out and the Bliss line). These are proposals and
// operating assumptions that are not decided and are not for publication, so this module is compiled only with the `economics-draft`
// feature; the public page (economics_model.rs) holds the rules that are built today and none of these words. It is pure and tested;
// economics_draft_chart.rs draws it and economics_draft.rs lays out the page. Every rule here is either built in the
// Worker (the constant names its source) or a draft that the page says is a draft. Nothing here is a price: the volumes, the costs,
// the tax rate and the creator's numbers are inputs the reader moves.
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

/// A plan with a monthly price and the Tickets it grants each billing period (the Worker reads `tickets_per_invoice` from the
/// price). The Premium page lists the same plans; a test keeps the two in step.
pub struct Plan {
    pub name: &'static str,
    pub usd: f64,
    pub tickets: f64,
}

pub static PLANS: [Plan; 4] = [
    Plan { name: "Player Plus", usd: 4.99, tickets: 450.0 },
    Plan { name: "Creator", usd: 9.99, tickets: 0.0 },
    Plan { name: "Bundle", usd: 12.99, tickets: 400.0 },
    Plan { name: "Professional", usd: 129.0, tickets: 0.0 },
];

/// The price of one Ticket in the cheapest pack. A plan's Tickets are valued at this price to find the Tickets share of its invoice.
pub fn cheapest_ticket_usd() -> f64 {
    PACKS.iter().map(|p| p.usd / p.tickets).fold(f64::INFINITY, f64::min)
}

impl Plan {
    /// The share of one invoice that is its Tickets, valued at the cheapest pack's price per Ticket (at most 1). A plan that grants no
    /// Tickets has none.
    pub fn tickets_share(&self) -> f64 {
        if self.tickets > 0.0 {
            (self.tickets * cheapest_ticket_usd() / self.usd).min(1.0)
        } else {
            0.0
        }
    }
}

/// Stripe on the web: 2.9% plus 30 cents a payment (`splitSale` in treasury.mjs).
pub const STRIPE_PERCENT: f64 = 0.029;
pub const STRIPE_FIXED_USD: f64 = 0.30;
/// An app store's cut, modeled in `splitSale` for iOS, Android and Steam. No app-store purchase flow exists yet.
pub const STORE_PERCENT: f64 = 0.30;
/// The average Delta pack order, used only to price the fixed part of Stripe's fee on Delta packs.
pub const DELTA_ORDER_USD: f64 = 25.0;

/// Studio's contribution weights (docs: Earning): scripting 3.0, building 2.5, anything else 1.0. The model assumes a 40/40/20 mix
/// of a creator's hours, which is an assumption and says so on the page.
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
/// A day mints and drips its whole allowance only when the day's total score reaches this (`FULL_DAY_SCORE`, bliss.mjs).
pub const FULL_DAY_SCORE: f64 = 1440.0;
/// A month is thirty days here.
pub const DAYS: f64 = 30.0;

// -----------------------------------------------------------------------------
// 2. Inputs and the controls that set them
// -----------------------------------------------------------------------------

/// Where Ticket packs are bought. Plan invoices and Delta packs are always web payments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Web,
    Store,
}

/// When the Treasury and the reserve count as an expense for tax: in the month they are funded, or only in the month they are paid out.
/// An accountant decides which applies; the page shows both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    Funded,
    PaidOut,
}

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

/// Everything the reader can move. Money is dollars a month, percentages are 0 to 100.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    /// Ticket pack sales, in dollars a month, and which pack the buyers choose (an index into `PACKS`).
    pub tickets: f64,
    pub pack: usize,
    pub channel: Channel,
    /// Subscribers to each plan, in the order of `PLANS`.
    pub subscribers: [f64; 4],
    pub delta: f64,
    /// The share of Eustress's half of each Ticket sale that goes to the Ticket Sales Reserve (a draft: 15%).
    pub reserve: f64,
    /// The Tickets cash-out rate, in dollars per 1,000 eligible Tickets (a draft: $1.50).
    pub rate: f64,
    /// The share of bought Tickets that creators earn and cash out, for the coverage check.
    pub through: f64,
    pub expenses: f64,
    pub tax: f64,
    pub basis: Basis,
    pub invest: f64,
    pub principal: f64,
    pub repay: f64,
    pub ret: f64,
    /// The creator's share of a sale, in percent. The Worker pays 70% today (`CREATOR_SHARE`, commerce.mjs).
    pub creator_share: f64,
    pub players: f64,
    pub conv: f64,
    pub avg: f64,
    pub paid: f64,
    pub hours: f64,
    pub node: NodeKind,
    pub updates: f64,
    pub lift: f64,
    pub cash: f64,
    pub others: f64,
    pub score: f64,
}

impl Default for Inputs {
    /// The default month: the one the first money-flow chart was worked from.
    fn default() -> Self {
        Inputs {
            tickets: 100_000.0,
            pack: DEFAULT_PACK,
            channel: Channel::Web,
            subscribers: [3000.0, 1500.0, 800.0, 5.0],
            delta: 38_000.0,
            reserve: 15.0,
            rate: 1.5,
            through: 50.0,
            expenses: 40_000.0,
            tax: 25.0,
            basis: Basis::Funded,
            invest: 5000.0,
            principal: 60_000.0,
            repay: 5.0,
            ret: 0.0,
            creator_share: 70.0,
            players: 5000.0,
            conv: 3.0,
            avg: 300.0,
            paid: 90.0,
            hours: 60.0,
            node: NodeKind::Light,
            updates: 2.0,
            lift: 5.0,
            cash: 100.0,
            others: 300.0,
            score: 500.0,
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
    Reserve,
    Rate,
    Through,
    Expenses,
    Tax,
    Invest,
    Principal,
    Repay,
    Return,
    CreatorShare,
    Players,
    Conversion,
    Average,
    Paid,
    Hours,
    Updates,
    Lift,
    Cash,
    Others,
    Score,
}

/// How a control's value is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Dollars,
    Percent,
    Count,
    Hours,
    Rate,
}

impl Unit {
    pub fn show(self, v: f64) -> String {
        match self {
            Unit::Dollars => dollars(v.round() as i64),
            Unit::Percent => format!("{}%", (v * 100.0).round() / 100.0),
            Unit::Count => commas(v.round() as i64),
            Unit::Hours => format!("{} h", commas(v.round() as i64)),
            Unit::Rate => format!("${v:.2}"),
        }
    }
}

/// The three groups the controls sit in on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Receipts,
    Eustress,
    Creator,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Receipts, Group::Eustress, Group::Creator];

    pub fn title(self) -> &'static str {
        match self {
            Group::Receipts => "What Eustress sells in a month",
            Group::Eustress => "Eustress's half, the reserve and investors",
            Group::Creator => "One creator",
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
}

macro_rules! control {
    ($key:ident, $group:ident, $label:expr, $help:expr, $unit:ident, $min:expr, $max:expr, $step:expr) => {
        Control { key: Key::$key, group: Group::$group, label: $label, help: $help, unit: Unit::$unit, min: $min, max: $max, step: $step }
    };
}

pub static CONTROLS: [Control; 26] = [
    control!(Tickets, Receipts, "Ticket pack sales", "", Dollars, 0.0, 300_000.0, 1000.0),
    control!(PlayerPlus, Receipts, "Player Plus subscribers ($4.99, 450 Tickets)", "", Count, 0.0, 20_000.0, 100.0),
    control!(Creator, Receipts, "Creator subscribers ($9.99)", "", Count, 0.0, 20_000.0, 100.0),
    control!(Bundle, Receipts, "Bundle subscribers ($12.99, 400 Tickets)", "", Count, 0.0, 20_000.0, 100.0),
    control!(Professional, Receipts, "Professional subscribers ($129)", "", Count, 0.0, 200.0, 1.0),
    control!(Delta, Receipts, "Delta pack sales", "", Dollars, 0.0, 300_000.0, 1000.0),
    control!(Reserve, Eustress, "Reserve: share of Eustress's half of Ticket revenue", "Draft. Paid by packs and by the Tickets share of plan invoices.", Percent, 0.0, 50.0, 1.0),
    control!(Rate, Eustress, "Tickets cash-out rate per 1,000 Tickets", "Draft. What an eligible creator is paid for 1,000 earned Tickets.", Rate, 0.0, 8.0, 0.25),
    control!(Through, Eustress, "Bought Tickets that creators earn and cash out", "Checks the reserve: how much cash-out the rate could owe.", Percent, 0.0, 100.0, 5.0),
    control!(Expenses, Eustress, "Engine Expenses a month", "Servers, Cloudflare, refunds and the rest of running Eustress.", Dollars, 0.0, 300_000.0, 500.0),
    control!(Tax, Eustress, "Tax rate on profit", "A placeholder: an accountant sets the rate.", Percent, 0.0, 50.0, 1.0),
    control!(Invest, Eustress, "New Treasury investments a month", "A what-if, not an offer. Capital, not revenue.", Dollars, 0.0, 100_000.0, 500.0),
    control!(Principal, Eustress, "Investor principal outstanding", "", Dollars, 0.0, 1_000_000.0, 5000.0),
    control!(Repay, Eustress, "Principal repaid each month", "Repayments come out of the Treasury, so contributors are paid less.", Percent, 0.0, 30.0, 1.0),
    control!(Return, Eustress, "Return paid to investors each month", "Also paid from the Treasury.", Percent, 0.0, 5.0, 0.1),
    control!(CreatorShare, Creator, "Creator's share of a sale", "The Worker pays 70% today. The rate is not final.", Percent, 0.0, 100.0, 1.0),
    control!(Players, Creator, "Players a month (popularity)", "", Count, 0.0, 200_000.0, 500.0),
    control!(Conversion, Creator, "Conversion: players who buy", "", Percent, 0.0, 20.0, 0.5),
    control!(Average, Creator, "Tickets each buyer spends", "", Count, 0.0, 2000.0, 25.0),
    control!(Paid, Creator, "Spend that was bought with money", "Only credit from Tickets bought with money is eligible to cash out and counts toward sales score.", Percent, 0.0, 100.0, 5.0),
    control!(Hours, Creator, "Hours in Studio a month", "Paid through the Bliss line only.", Hours, 0.0, 300.0, 5.0),
    control!(Updates, Creator, "Updates a month", "Updates earn no score of their own; they matter through the players they bring back.", Count, 0.0, 12.0, 1.0),
    control!(Lift, Creator, "Players gained per update", "An assumption, not a rule.", Percent, 0.0, 20.0, 1.0),
    control!(Cash, Creator, "Redeemable credit cashed out", "", Percent, 0.0, 100.0, 5.0),
    control!(Others, Creator, "Other contributors active each day", "The Bliss line follows this number: the Treasury pays the same pool however many share it.", Count, 0.0, 5000.0, 50.0),
    control!(Score, Creator, "Their average daily score", "", Count, 0.0, 3200.0, 50.0),
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
        Group::Receipts => vec![S(Key::Tickets), C(Pick::Pack), C(Pick::Channel), S(Key::PlayerPlus), S(Key::Creator), S(Key::Bundle), S(Key::Professional), S(Key::Delta)],
        Group::Eustress => vec![
            S(Key::Reserve),
            S(Key::Rate),
            S(Key::Through),
            S(Key::Expenses),
            S(Key::Tax),
            C(Pick::Basis),
            S(Key::Invest),
            S(Key::Principal),
            S(Key::Repay),
            S(Key::Return),
        ],
        Group::Creator => vec![
            S(Key::CreatorShare),
            S(Key::Players),
            S(Key::Conversion),
            S(Key::Average),
            S(Key::Paid),
            S(Key::Hours),
            C(Pick::Node),
            S(Key::Updates),
            S(Key::Lift),
            S(Key::Cash),
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
            Key::Reserve => self.reserve,
            Key::Rate => self.rate,
            Key::Through => self.through,
            Key::Expenses => self.expenses,
            Key::Tax => self.tax,
            Key::Invest => self.invest,
            Key::Principal => self.principal,
            Key::Repay => self.repay,
            Key::Return => self.ret,
            Key::CreatorShare => self.creator_share,
            Key::Players => self.players,
            Key::Conversion => self.conv,
            Key::Average => self.avg,
            Key::Paid => self.paid,
            Key::Hours => self.hours,
            Key::Updates => self.updates,
            Key::Lift => self.lift,
            Key::Cash => self.cash,
            Key::Others => self.others,
            Key::Score => self.score,
        }
    }

    /// Set a control from what a range input reports: its value as text. Text that is not a number leaves the control as it was.
    pub fn set_text(&mut self, key: Key, text: &str) {
        if let Ok(v) = text.trim().parse::<f64>() {
            self.set(key, v);
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
            Key::Reserve => self.reserve = v,
            Key::Rate => self.rate = v,
            Key::Through => self.through = v,
            Key::Expenses => self.expenses = v,
            Key::Tax => self.tax = v,
            Key::Invest => self.invest = v,
            Key::Principal => self.principal = v,
            Key::Repay => self.repay = v,
            Key::Return => self.ret = v,
            Key::CreatorShare => self.creator_share = v,
            Key::Players => self.players = v,
            Key::Conversion => self.conv = v,
            Key::Average => self.avg = v,
            Key::Paid => self.paid = v,
            Key::Hours => self.hours = v,
            Key::Updates => self.updates = v,
            Key::Lift => self.lift = v,
            Key::Cash => self.cash = v,
            Key::Others => self.others = v,
            Key::Score => self.score = v,
        }
    }
}

/// A choice from a short list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Pack,
    Channel,
    Basis,
    Node,
}

impl Pick {
    pub fn label(self) -> &'static str {
        match self {
            Pick::Pack => "Ticket pack bought",
            Pick::Channel => "Where Ticket packs are bought",
            Pick::Basis => "Treasury and reserve count as an expense for tax",
            Pick::Node => "Studio node",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Pick::Pack | Pick::Channel => Group::Receipts,
            Pick::Basis => Group::Eustress,
            Pick::Node => Group::Creator,
        }
    }

    /// The choices as (value, words).
    pub fn options(self) -> Vec<(&'static str, String)> {
        match self {
            Pick::Pack => ["starter", "standard", "mega", "super", "ultra"]
                .iter()
                .zip(PACKS.iter())
                .map(|(v, p)| (*v, format!("{} {}, {} Tickets", p.name, price_words(p.usd), commas(p.tickets as i64))))
                .collect(),
            Pick::Channel => vec![("web", "Web: Stripe 2.9% + $0.30".to_string()), ("store", "App store: 30%".to_string())],
            Pick::Basis => vec![("funded", "Yes, when funded".to_string()), ("paid", "No, only when paid out".to_string())],
            Pick::Node => vec![("light", "Light, 1.0".to_string()), ("full", "Full, 1.1".to_string())],
        }
    }
}

impl Inputs {
    /// The value of a choice, as one of the `options` values.
    pub fn picked(&self, pick: Pick) -> &'static str {
        match pick {
            Pick::Pack => ["starter", "standard", "mega", "super", "ultra"][self.pack.min(4)],
            Pick::Channel => match self.channel {
                Channel::Web => "web",
                Channel::Store => "store",
            },
            Pick::Basis => match self.basis {
                Basis::Funded => "funded",
                Basis::PaidOut => "paid",
            },
            Pick::Node => match self.node {
                NodeKind::Light => "light",
                NodeKind::Full => "full",
            },
        }
    }

    /// Choose by value. A value that is not one of the options changes nothing.
    pub fn pick(&mut self, pick: Pick, value: &str) {
        match (pick, value) {
            (Pick::Pack, v) => {
                if let Some(i) = ["starter", "standard", "mega", "super", "ultra"].iter().position(|x| *x == v) {
                    self.pack = i;
                }
            }
            (Pick::Channel, "web") => self.channel = Channel::Web,
            (Pick::Channel, "store") => self.channel = Channel::Store,
            (Pick::Basis, "funded") => self.basis = Basis::Funded,
            (Pick::Basis, "paid") => self.basis = Basis::PaidOut,
            (Pick::Node, "light") => self.node = NodeKind::Light,
            (Pick::Node, "full") => self.node = NodeKind::Full,
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

/// One plan's line in the month.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanLine {
    pub name: &'static str,
    pub subscribers: f64,
    pub revenue: f64,
    /// The share of one invoice that is its Tickets, valued at the cheapest pack's price per Ticket (at most 1).
    pub tickets_share: f64,
}

/// The figures a label prints, whole dollars, worked in the order the money moves from rounded steps so every printed line adds up to
/// the dollar: gross less fees is net, the Treasury and Eustress's half make net, and so on down to the profit kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyShown {
    pub gross: i64,
    pub fees: i64,
    pub net: i64,
    pub treasury: i64,
    pub eustress: i64,
    pub reserve_packs: i64,
    pub reserve_plans: i64,
    pub reserve: i64,
    pub expenses: i64,
    pub profit: i64,
    pub taxes: i64,
    pub kept: i64,
    pub treasury_in: i64,
    pub paid_investors: i64,
    pub drip: i64,
    pub paid_principal: i64,
    pub paid_return: i64,
    pub taxable: i64,
    pub tax_due: i64,
    /// The tax if the other treatment of the Treasury and reserve applied.
    pub alt_due: i64,
}

/// A month of Eustress, in dollars.
#[derive(Clone, Debug, PartialEq)]
pub struct Company {
    pub pack_tickets: f64,
    pub granted_tickets: f64,
    pub subscribers: f64,
    pub plans: [PlanLine; 4],
    pub plan_revenue: f64,
    pub plan_fees: f64,
    pub plan_ticket_net: f64,
    pub fee_tickets: f64,
    pub fee_delta: f64,
    pub gross: f64,
    pub fees: f64,
    pub net: f64,
    /// Half of net: the Bliss Treasury's share of sales.
    pub treasury_share: f64,
    /// The other half: Eustress's.
    pub eustress: f64,
    pub reserve_packs: f64,
    pub reserve_plans: f64,
    pub reserve: f64,
    pub available: f64,
    pub expenses: f64,
    pub shortfall: f64,
    pub profit: f64,
    /// What enters the Treasury: its half of sales plus new investments.
    pub treasury_in: f64,
    pub paid_investors: f64,
    pub paid_principal: f64,
    pub paid_return: f64,
    /// What the Treasury pays contributors in a month, at a steady state.
    pub drip: f64,
    pub taxable: f64,
    pub tax_due: f64,
    pub taxes: f64,
    pub kept: f64,
    pub alt_taxable: f64,
    pub alt_due: f64,
    pub eligible_bought: f64,
    /// What the Tickets cash-out would owe at the chosen rate and cash-out share.
    pub owed: f64,
    pub shown: CompanyShown,
}

pub fn company(s: &Inputs) -> Company {
    let web = s.channel == Channel::Web;
    let pack = &PACKS[s.pack.min(PACKS.len() - 1)];
    let pack_tickets = s.tickets / pack.usd * pack.tickets;
    let fee_tickets = if web { s.tickets * STRIPE_PERCENT + (s.tickets / pack.usd) * STRIPE_FIXED_USD } else { s.tickets * STORE_PERCENT };

    let mut plan_revenue = 0.0;
    let mut plan_fees = 0.0;
    let mut plan_ticket_net = 0.0;
    let mut granted = 0.0;
    let mut subscribers = 0.0;
    let plans: [PlanLine; 4] = std::array::from_fn(|i| {
        let p = &PLANS[i];
        let n = s.subscribers[i];
        let revenue = n * p.usd;
        let fee = n * (p.usd * STRIPE_PERCENT + STRIPE_FIXED_USD);
        let share = p.tickets_share();
        plan_revenue += revenue;
        plan_fees += fee;
        plan_ticket_net += (revenue - fee) * share;
        granted += n * p.tickets;
        subscribers += n;
        PlanLine { name: p.name, subscribers: n, revenue, tickets_share: share }
    });

    let fee_delta = s.delta * STRIPE_PERCENT + (s.delta / DELTA_ORDER_USD) * STRIPE_FIXED_USD;
    let gross = s.tickets + plan_revenue + s.delta;
    let fees = fee_tickets + plan_fees + fee_delta;
    let net = gross - fees;
    let treasury_share = net / 2.0;
    let eustress = net - treasury_share;
    let reserve_packs = (s.tickets - fee_tickets) / 2.0 * s.reserve / 100.0;
    let reserve_plans = plan_ticket_net / 2.0 * s.reserve / 100.0;
    let reserve = reserve_packs + reserve_plans;
    let available = eustress - reserve;
    let expenses = s.expenses.min(available.max(0.0));
    let shortfall = s.expenses - expenses;
    let profit = (available - expenses).max(0.0);

    let treasury_in = treasury_share + s.invest;
    let want_principal = s.principal * s.repay / 100.0;
    let want_return = s.principal * s.ret / 100.0;
    let want = want_principal + want_return;
    let paid_investors = want.min(treasury_in);
    let paid_principal = if want > 0.0 { paid_investors * want_principal / want } else { 0.0 };
    let paid_return = paid_investors - paid_principal;
    let drip = treasury_in - paid_investors;

    let paid_out_taxable = (net - expenses - drip.min(treasury_share)).max(0.0);
    let taxable = if s.basis == Basis::Funded { profit } else { paid_out_taxable };
    let tax_due = taxable * s.tax / 100.0;
    let taxes = tax_due.min(profit);
    let kept = profit - taxes;
    let alt_taxable = if s.basis == Basis::Funded { paid_out_taxable } else { profit };
    let alt_due = alt_taxable * s.tax / 100.0;

    let eligible_bought = (if web { pack_tickets } else { 0.0 }) + granted;
    let owed = eligible_bought * (s.through / 100.0) * (s.creator_share / 100.0) * s.rate / 1000.0;

    // The printed dollars, worked in order from rounded steps.
    let r = |x: f64| x.round() as i64;
    let invest = r(s.invest);
    let d_gross = r(gross);
    let d_fees = r(fees);
    let d_net = d_gross - d_fees;
    let d_treasury = d_net.div_euclid(2);
    let d_eustress = d_net - d_treasury;
    let d_reserve_packs = r(reserve_packs);
    let d_reserve_plans = r(reserve_plans);
    let d_reserve = d_reserve_packs + d_reserve_plans;
    let d_expenses = r(s.expenses).min((d_eustress - d_reserve).max(0));
    let d_profit = (d_eustress - d_reserve - d_expenses).max(0);
    let d_in = d_treasury + invest;
    let d_paid_investors = r(paid_investors).min(d_in);
    let d_drip = d_in - d_paid_investors;
    let d_paid_principal = r(paid_principal).min(d_paid_investors);
    let d_paid_return = d_paid_investors - d_paid_principal;
    let d_taxable_funded = d_profit;
    let d_taxable_paid = (d_net - d_expenses - d_drip.min(d_treasury)).max(0);
    let d_taxable = if s.basis == Basis::Funded { d_taxable_funded } else { d_taxable_paid };
    let d_tax_due = r(d_taxable as f64 * s.tax / 100.0);
    let d_taxes = d_tax_due.min(d_profit);
    let d_kept = d_profit - d_taxes;
    let d_alt_due = r((if s.basis == Basis::Funded { d_taxable_paid } else { d_taxable_funded }) as f64 * s.tax / 100.0);
    let shown = CompanyShown {
        gross: d_gross,
        fees: d_fees,
        net: d_net,
        treasury: d_treasury,
        eustress: d_eustress,
        reserve_packs: d_reserve_packs,
        reserve_plans: d_reserve_plans,
        reserve: d_reserve,
        expenses: d_expenses,
        profit: d_profit,
        taxes: d_taxes,
        kept: d_kept,
        treasury_in: d_in,
        paid_investors: d_paid_investors,
        drip: d_drip,
        paid_principal: d_paid_principal,
        paid_return: d_paid_return,
        taxable: d_taxable,
        tax_due: d_tax_due,
        alt_due: d_alt_due,
    };

    Company {
        pack_tickets,
        granted_tickets: granted,
        subscribers,
        plans,
        plan_revenue,
        plan_fees,
        plan_ticket_net,
        fee_tickets,
        fee_delta,
        gross,
        fees,
        net,
        treasury_share,
        eustress,
        reserve_packs,
        reserve_plans,
        reserve,
        available,
        expenses,
        shortfall,
        profit,
        treasury_in,
        paid_investors,
        paid_principal,
        paid_return,
        drip,
        taxable,
        tax_due,
        taxes,
        kept,
        alt_taxable,
        alt_due,
        eligible_bought,
        owed,
        shown,
    }
}

// -----------------------------------------------------------------------------
// 4. One creator
// -----------------------------------------------------------------------------

/// The figures a creator label prints, in whole cents, worked in order from rounded steps so every line adds up to the cent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatorShown {
    pub effort: i64,
    pub value: i64,
    pub bliss: i64,
    pub cash: i64,
    pub total: i64,
    pub spend: i64,
    pub creator: i64,
    pub platform: i64,
    pub discount: i64,
    pub spendable: i64,
}

/// What one creator gets in a month. Money is dollars; Tickets and score are counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Creator {
    /// Dollars per Ticket at the chosen pack's price: a yardstick, not a redemption value.
    pub price: f64,
    pub players: f64,
    pub buyers: f64,
    pub spent: f64,
    pub spent_paid: f64,
    pub earned: f64,
    pub eligible: f64,
    /// The part of `eligible` that scores: one buyer's Tickets count up to the per-buyer cap, taking each buyer to spend the average in one day.
    pub counted: f64,
    pub cashed: f64,
    pub cash_usd: f64,
    pub value_score: f64,
    pub effort: f64,
    pub mine: f64,
    pub total: f64,
    /// The share of a day's drip that is paid at all: the network's score over a full day's 1,440, at most 1.
    pub gate: f64,
    pub pool: f64,
    pub bliss_effort: f64,
    pub bliss_value: f64,
    pub bliss: f64,
    pub spend_usd: f64,
    pub paid_usd: f64,
    pub creator_usd: f64,
    pub cashed_face: f64,
    pub discount: f64,
    pub spendable: f64,
    pub cash_total: f64,
    pub shown: CreatorShown,
}

/// Whole units, without the float dust a product of percentages leaves behind.
fn whole(x: f64) -> f64 {
    ((x * 1e6).round() / 1e6).floor()
}

pub fn creator(s: &Inputs, m: &Company) -> Creator {
    let pack = &PACKS[s.pack.min(PACKS.len() - 1)];
    let share = s.creator_share / 100.0;
    let price = pack.usd / pack.tickets;
    let players = s.players * (1.0 + s.lift / 100.0 * s.updates);
    let buyers = players * s.conv / 100.0;
    let spent = buyers * s.avg;
    let spent_paid = spent * s.paid / 100.0;
    let earned = whole(spent * share);
    let eligible = whole(spent_paid * share);
    let counted = whole(buyers * (s.avg * s.paid / 100.0 * share).min(VALUE_CAP_TICKETS_PER_BUYER));
    let cashed = eligible * s.cash / 100.0;
    let cash_usd = cashed * s.rate / 1000.0;
    let value_score = whole(VALUE_PER_TICKET * counted).min(VALUE_CAP_PER_DAY * DAYS);
    let effort = (s.hours * 60.0 * effort_per_minute() * s.node.bonus()).min(EFFORT_CAP_PER_DAY * DAYS);
    let mine = effort + value_score;
    let total = s.others * s.score * DAYS + mine;
    let gate = (total / DAYS / FULL_DAY_SCORE).min(1.0);
    let pool = m.drip * gate;
    let bliss_effort = if total > 0.0 { pool * effort / total } else { 0.0 };
    let bliss_value = if total > 0.0 { pool * value_score / total } else { 0.0 };
    let bliss = bliss_effort + bliss_value;
    let spend_usd = spent * price;
    let paid_usd = spent_paid * price;
    let creator_usd = spend_usd * share;
    let cashed_face = cashed * price;
    let discount = (cashed_face - cash_usd).max(0.0);
    let spendable = (creator_usd - cashed_face).max(0.0);

    let cents = |x: f64| (x * 100.0).round() as i64;
    let c_effort = cents(bliss_effort);
    let c_value = cents(bliss_effort + bliss_value) - c_effort;
    let c_bliss = c_effort + c_value;
    let c_cash = cents(cash_usd);
    let c_total = c_bliss + c_cash;
    let c_spend = cents(spend_usd);
    let c_creator = cents(creator_usd);
    let c_face = cents(cashed_face);
    let shown = CreatorShown {
        effort: c_effort,
        value: c_value,
        bliss: c_bliss,
        cash: c_cash,
        total: c_total,
        spend: c_spend,
        creator: c_creator,
        platform: c_spend - c_creator,
        discount: (c_face - c_cash).max(0),
        spendable: (c_creator - c_face).max(0),
    };

    Creator {
        price,
        players,
        buyers,
        spent,
        spent_paid,
        earned,
        eligible,
        counted,
        cashed,
        cash_usd,
        value_score,
        effort,
        mine,
        total,
        gate,
        pool,
        bliss_effort,
        bliss_value,
        bliss,
        spend_usd,
        paid_usd,
        creator_usd,
        cashed_face,
        discount,
        spendable,
        cash_total: cash_usd + bliss,
        shown,
    }
}

/// A creator to start from: the same month's numbers for a small, a typical and a hit creator. Presets move only the creator's
/// controls.
pub struct Preset {
    pub name: &'static str,
    pub values: [(Key, f64); 5],
}

pub static PRESETS: [Preset; 3] = [
    Preset { name: "Small", values: [(Key::Players, 500.0), (Key::Conversion, 2.0), (Key::Average, 150.0), (Key::Hours, 20.0), (Key::Updates, 1.0)] },
    Preset { name: "Typical", values: [(Key::Players, 5000.0), (Key::Conversion, 3.0), (Key::Average, 300.0), (Key::Hours, 60.0), (Key::Updates, 2.0)] },
    Preset { name: "Hit", values: [(Key::Players, 50_000.0), (Key::Conversion, 4.0), (Key::Average, 400.0), (Key::Hours, 120.0), (Key::Updates, 4.0)] },
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
        near(m.fees, fee_t + fee_p + fee_d, 1e-6, "fees");
        near(m.fees, 10_241.271, 0.01, "fees, literal");
        near(m.net, 168_750.729, 0.01, "net, literal");
        near(m.treasury_share, 84_375.3645, 0.01, "the Treasury's half, literal");
        near(m.reserve_packs, 7_057.2748, 0.01, "reserve from packs, literal");
        // A plan's Tickets are valued at the cheapest pack: $99.99 for 10,800 Tickets.
        let y = 99.99 / 10_800.0;
        near(m.plans[0].tickets_share, 450.0 * y / 4.99, 1e-9, "Player Plus Tickets share");
        near(m.plans[0].tickets_share, 0.8349, 0.0005, "Player Plus share, literal");
        near(m.plans[2].tickets_share, 0.2851, 0.0005, "Bundle share, literal");
        assert_eq!((m.plans[1].tickets_share, m.plans[3].tickets_share), (0.0, 0.0), "Creator and Professional grant no Tickets");
        let pp_net = 3000.0 * (4.99 - (4.99 * 0.029 + 0.3));
        let bu_net = 800.0 * (12.99 - (12.99 * 0.029 + 0.3));
        near(m.reserve_plans, (pp_net * (450.0 * y / 4.99) + bu_net * (400.0 * y / 12.99)) / 2.0 * 0.15, 1e-6, "reserve from plan invoices");
        near(m.reserve_plans, 1064.5, 0.6, "plan reserve, literal");
        near(m.reserve, m.reserve_packs + m.reserve_plans, 1e-9, "the reserve is packs plus plans");
        // Cash-out owed: bought Tickets (web packs and plan grants) x 50% x 70% x $1.50 per 1,000.
        let bought = 100_000.0 / 9.99 * 880.0 + 3000.0 * 450.0 + 800.0 * 400.0;
        near(m.owed, bought * 0.5 * 0.7 * 1.5 / 1000.0, 1e-6, "cash-out owed");
        near(m.owed, 5501.4, 0.5, "owed, literal");
        // The Treasury: its half of sales plus 5,000 invested, less 5% of 60,000 repaid, with no return.
        near(m.treasury_in, m.treasury_share + 5000.0, 1e-9, "Treasury in");
        near(m.paid_investors, 3000.0, 1e-9, "principal repaid");
        near(m.drip, m.treasury_share + 5000.0 - 3000.0, 1e-9, "drip");
        // Profit and tax, with the Treasury and reserve deducted when funded: Eustress's half less the reserve less expenses, taxed at 25%.
        near(m.profit, m.eustress - m.reserve - 40_000.0, 1e-9, "profit");
        near(m.taxes, m.profit * 0.25, 1e-9, "taxes");
        near(m.kept, m.profit * 0.75, 1e-9, "kept");
        // The same month deducting only what is paid out: the sales-funded payout comes off, the reserve does not.
        let mut p = s.clone();
        p.basis = Basis::PaidOut;
        let mp = company(&p);
        near(mp.taxable, mp.net - 40_000.0 - mp.drip.min(mp.treasury_share), 1e-9, "paid-out taxable");
        near(mp.taxable - m.taxable, m.reserve, 1e-6, "the paid-out basis taxes the reserve as well, the only difference this month");
        near(m.alt_due, mp.tax_due, 1e-9, "the other treatment of the funded view is the paid-out tax");
        near(mp.alt_due, m.tax_due, 1e-9, "the other treatment of the paid-out view is the funded tax");
    }

    #[test]
    fn the_default_month_prints_the_dollars_the_chart_was_checked_against() {
        let m = company(&Inputs::default());
        let d = &m.shown;
        assert_eq!(
            (d.gross, d.fees, d.net, d.treasury, d.eustress, d.reserve_packs, d.reserve_plans, d.reserve, d.expenses, d.profit, d.taxes, d.kept),
            (178_992, 10_241, 168_751, 84_375, 84_376, 7_057, 1_064, 8_121, 40_000, 36_255, 9_064, 27_191)
        );
        assert_eq!((d.treasury_in, d.paid_investors, d.drip, d.paid_principal, d.paid_return), (89_375, 3_000, 86_375, 3_000, 0));
        assert_eq!((d.taxable, d.tax_due, d.alt_due), (36_255, 9_064, 11_094));
    }

    #[test]
    fn the_default_creator_matches_the_hand_worked_numbers() {
        let s = Inputs::default();
        let m = company(&s);
        let c = creator(&s, &m);
        near(c.players, 5500.0, 1e-9, "players with updates");
        near(c.buyers, 165.0, 1e-9, "buyers");
        near(c.spent, 49_500.0, 1e-6, "Tickets spent");
        assert_eq!(c.eligible, 31_185.0);
        assert_eq!(c.earned, 34_650.0);
        near(c.effort, 60.0 * 60.0 * 2.4, 1e-9, "effort score");
        assert_eq!(c.value_score, 15_592.0);
        near(c.total, 300.0 * 500.0 * 30.0 + 8640.0 + 15_592.0, 1e-9, "the network's score");
        near(c.cash_usd, 31_185.0 * 1.5 / 1000.0, 1e-9, "Tickets-line cash");
        near(c.bliss, m.drip * (8640.0 + 15_592.0) / (4_500_000.0 + 8640.0 + 15_592.0), 1e-6, "Bliss share");
        near(c.price, 9.99 / 880.0, 1e-12, "dollars per Ticket");
        let d = &c.shown;
        assert_eq!(
            (d.effort, d.value, d.bliss, d.cash, d.total, d.spend, d.creator, d.platform, d.discount, d.spendable),
            (16_495, 29_768, 46_263, 4_678, 50_941, 56_194, 39_336, 16_858, 30_724, 3_934)
        );
    }

    #[test]
    fn the_creator_share_is_an_input_and_seventy_percent_is_the_worker_rule() {
        assert_eq!(Inputs::default().creator_share, 70.0);
        let mut s = Inputs::default();
        s.creator_share = 60.0;
        let m = company(&s);
        let c = creator(&s, &m);
        assert_eq!(c.earned, 29_700.0, "60% of 49,500");
        assert!(c.creator_usd < creator(&Inputs::default(), &company(&Inputs::default())).creator_usd);
        // the platform keeps the rest
        near(c.shown.platform as f64 / c.shown.spend as f64, 0.4, 0.001, "platform share at 60%");
        // and the cash-out liability moves with it
        assert!(m.owed < company(&Inputs::default()).owed);
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
        s.channel = if rng.next() < 0.5 { Channel::Web } else { Channel::Store };
        s.basis = if rng.next() < 0.5 { Basis::Funded } else { Basis::PaidOut };
        s.node = if rng.next() < 0.5 { NodeKind::Light } else { NodeKind::Full };
        s
    }

    fn all_finite(m: &Company, c: &Creator) -> bool {
        let f = [
            m.pack_tickets, m.granted_tickets, m.gross, m.fees, m.net, m.treasury_share, m.eustress, m.reserve, m.available, m.expenses, m.shortfall, m.profit,
            m.treasury_in, m.paid_investors, m.drip, m.taxable, m.tax_due, m.taxes, m.kept, m.alt_due, m.eligible_bought, m.owed, c.price, c.players, c.buyers, c.spent,
            c.earned, c.eligible, c.counted, c.cashed, c.cash_usd, c.value_score, c.effort, c.mine, c.total, c.gate, c.pool, c.bliss, c.spend_usd, c.creator_usd, c.discount,
            c.spendable, c.cash_total,
        ];
        f.iter().all(|x| x.is_finite())
    }

    #[test]
    fn money_is_conserved_down_the_income_statement_in_four_thousand_random_months() {
        let mut rng = Lcg(12345);
        for _ in 0..4000 {
            let s = random_month(&mut rng);
            let m = company(&s);
            let c = creator(&s, &m);
            assert!(all_finite(&m, &c), "finite numbers in {s:?}");
            if m.gross > 0.0 {
                near(m.gross - m.fees, m.net, 1e-6, "net");
                near(m.treasury_share + m.eustress, m.net, 1e-6, "the two halves make net");
                near(m.eustress - m.reserve - m.expenses - m.profit, 0.0, 1e-6, "Eustress's half splits into reserve, expenses and profit");
                near(m.taxes + m.kept, m.profit, 1e-6, "profit splits into taxes and kept");
                near(m.treasury_in, m.drip + m.paid_investors, 1e-6, "the Treasury pays out what comes in");
                near(m.paid_principal + m.paid_return, m.paid_investors, 1e-6, "investor payments split");
            }
            assert!(m.drip >= -1e-9 && m.profit >= -1e-9 && m.taxes >= -1e-9 && m.kept >= -1e-9 && m.expenses >= -1e-9, "no negative flows in {s:?}");
            assert!(m.taxes <= m.profit + 1e-9, "taxes never exceed profit");
            assert!(m.shortfall >= -1e-9 && (m.expenses + m.shortfall - s.expenses).abs() < 1e-6, "expenses plus shortfall is the ask");
            // the printed dollars add up exactly
            let d = &m.shown;
            assert_eq!(d.gross - d.fees, d.net, "net in {s:?}");
            assert_eq!(d.treasury + d.eustress, d.net, "halves");
            assert_eq!(d.reserve, d.reserve_packs + d.reserve_plans, "reserve");
            assert!(d.eustress - d.reserve - d.expenses == d.profit || d.profit == 0, "profit in {s:?}");
            assert_eq!(d.taxes + d.kept, d.profit, "tax split in {s:?}");
            assert_eq!(d.drip + d.paid_investors, d.treasury_in, "Treasury");
            assert_eq!(d.treasury_in, d.treasury + s.invest.round() as i64, "Treasury in");
            assert_eq!(d.paid_principal + d.paid_return, d.paid_investors, "investor split");
            // one creator
            let k = &c.shown;
            assert_eq!(k.effort + k.value, k.bliss, "Bliss split");
            assert_eq!(k.bliss + k.cash, k.total, "total");
            assert_eq!(k.creator + k.platform, k.spend, "the sale splits between creator and platform");
            assert!((k.cash + k.discount + k.spendable - k.creator).abs() <= 1, "the creator's share splits into cash, discount and spendable in {s:?}");
            assert!(c.bliss_effort + c.bliss_value <= m.drip + 1e-9, "a creator never gets more than the drip");
            assert!(c.cashed <= c.eligible + 1e-9 && c.eligible <= c.earned, "cash-out is limited to credit from paid spending");
            assert!((0.0..=1.0).contains(&c.gate), "the gate is a share");
            // a day pays emission x min(1, score / 1,440) split by score: a quiet network pays a contributor's own score over a full day,
            // a busy one pays by share of the total
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
        for k in [Key::Tickets, Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta, Key::Invest, Key::Principal, Key::Players, Key::Hours, Key::Others] {
            z.set(k, 0.0);
        }
        let m = company(&z);
        let c = creator(&z, &m);
        assert!(m.gross == 0.0 && m.drip == 0.0 && c.cash_total == 0.0);
        assert!(all_finite(&m, &c));
    }

    #[test]
    fn a_lone_contributor_on_a_quiet_network_is_paid_for_their_own_score() {
        // 10 hours is 1,440 score a month; 100 a day elsewhere is a quiet network. A thirtieth of the drip, for one full day's score.
        let mut q = Inputs::default();
        q.set(Key::Players, 0.0);
        q.set(Key::Hours, 10.0);
        q.set(Key::Others, 1.0);
        q.set(Key::Score, 100.0);
        q.set(Key::Updates, 0.0);
        let m = company(&q);
        let c = creator(&q, &m);
        assert!(c.gate < 1.0);
        near(c.bliss, m.drip / 30.0, 1e-6, "a thirtieth of the drip");
    }

    #[test]
    fn a_heavy_paid_out_month_can_owe_more_tax_than_it_has_profit() {
        let mut t = Inputs::default();
        t.basis = Basis::PaidOut;
        t.set(Key::Repay, 30.0);
        t.set(Key::Principal, 1_000_000.0);
        t.set(Key::Invest, 0.0);
        t.set(Key::Return, 0.0);
        t.set(Key::Tax, 50.0);
        let m = company(&t);
        assert!(m.tax_due > m.taxes, "tax due {} against taxes {}", m.tax_due, m.taxes);
    }

    #[test]
    fn a_low_volume_month_cannot_cover_the_expenses_and_pays_no_tax() {
        let mut lo = Inputs::default();
        lo.set(Key::Tickets, 1000.0);
        for k in [Key::PlayerPlus, Key::Creator, Key::Bundle, Key::Professional, Key::Delta] {
            lo.set(k, 0.0);
        }
        let m = company(&lo);
        assert!(m.shortfall > 0.0 && m.profit == 0.0 && m.taxes == 0.0);
    }

    #[test]
    fn the_app_store_takes_thirty_percent_and_makes_no_pack_eligible_for_cash_out() {
        let mut st = Inputs::default();
        st.channel = Channel::Store;
        let m = company(&st);
        near(m.fee_tickets, 30_000.0, 1e-9, "store fee on packs");
        near(m.eligible_bought, 3000.0 * 450.0 + 800.0 * 400.0, 1e-9, "only plan-granted Tickets are eligible when packs are bought in a store");
    }

    #[test]
    fn repayments_cut_what_contributors_are_paid_dollar_for_dollar() {
        let base = company(&{
            let mut s = Inputs::default();
            s.set(Key::Invest, 0.0);
            s.set(Key::Principal, 0.0);
            s
        });
        let default = company(&Inputs::default());
        near(default.drip - base.drip, 2000.0, 1e-9, "5,000 in less 3,000 back is 2,000 more for contributors");
        let mut heavy = Inputs::default();
        heavy.set(Key::Repay, 15.0);
        near(company(&heavy).drip, base.drip + 5000.0 - 9000.0, 1e-9, "repaying 9,000 against 5,000 invested costs contributors 4,000");
        let mut ret = heavy.clone();
        ret.set(Key::Return, 1.0);
        near(company(&ret).drip, company(&heavy).drip - 600.0, 1e-9, "a 1% return on 60,000 is 600 more out of the Treasury");
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
        for p in [Pick::Pack, Pick::Channel, Pick::Basis, Pick::Node] {
            assert_eq!(choices.iter().filter(|x| **x == p).count(), 1, "{p:?} appears once");
        }
    }

    #[test]
    fn a_plans_tickets_share_is_worked_from_the_cheapest_pack() {
        near(PLANS[0].tickets_share(), 450.0 * (99.99 / 10_800.0) / 4.99, 1e-12, "Player Plus");
        near(PLANS[2].tickets_share(), 400.0 * (99.99 / 10_800.0) / 12.99, 1e-12, "Bundle");
        assert_eq!((PLANS[1].tickets_share(), PLANS[3].tickets_share()), (0.0, 0.0));
        // at the Standard pack's price Player Plus would be more than its whole invoice, so the share caps at 1
        assert!(450.0 * (9.99 / 880.0) / 4.99 > 1.0);
    }

    #[test]
    fn a_plan_that_grants_more_than_its_price_caps_at_the_whole_invoice() {
        let generous = Plan { name: "Generous", usd: 4.99, tickets: 5000.0 };
        assert_eq!(generous.tickets_share(), 1.0, "Tickets cannot be more than all of an invoice");
        let stingy = Plan { name: "Stingy", usd: 4.99, tickets: 10.0 };
        assert!(stingy.tickets_share() > 0.0 && stingy.tickets_share() < 0.05);
    }

    #[test]
    fn a_control_is_kept_inside_its_range_and_ignores_what_is_not_a_number() {
        let mut s = Inputs::default();
        s.set(Key::Reserve, 500.0);
        assert_eq!(s.reserve, 50.0);
        s.set(Key::Reserve, -3.0);
        assert_eq!(s.reserve, 0.0);
        s.set(Key::Reserve, 12.0);
        s.set(Key::Reserve, f64::NAN);
        s.set(Key::Reserve, f64::INFINITY);
        assert_eq!(s.reserve, 12.0);
    }

    #[test]
    fn a_range_input_reports_text_and_the_control_follows_it() {
        let mut s = Inputs::default();
        s.set_text(Key::Tickets, "250000");
        assert_eq!(s.tickets, 250_000.0);
        s.set_text(Key::Rate, " 2.75 ");
        assert_eq!(s.rate, 2.75);
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
        for pick in [Pick::Pack, Pick::Channel, Pick::Basis, Pick::Node] {
            for (value, _) in pick.options() {
                s.pick(pick, value);
                assert_eq!(s.picked(pick), value);
            }
            let before = s.clone();
            s.pick(pick, "no such choice");
            assert_eq!(s, before);
        }
        assert_eq!(Pick::Pack.options().len(), PACKS.len());
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
        assert_eq!(PRESETS[1].values.map(|(k, v)| (k, v)), [(Key::Players, 5000.0), (Key::Conversion, 3.0), (Key::Average, 300.0), (Key::Hours, 60.0), (Key::Updates, 2.0)], "Typical is the default creator");
    }

    // ---- the constants tie to what the site and the Worker say ----

    #[test]
    fn the_cheapest_ticket_is_in_the_ultra_pack() {
        near(cheapest_ticket_usd(), 99.99 / 10_800.0, 1e-12, "cheapest price per Ticket");
        for p in PACKS.iter() {
            assert!(p.usd / p.tickets >= cheapest_ticket_usd() - 1e-15);
        }
    }

    #[test]
    fn the_plans_here_are_the_plans_the_premium_page_lists() {
        let tiers = crate::pages::premium::plan_tiers();
        let named = |id: &str| tiers.iter().find(|t| t.id == id).unwrap_or_else(|| panic!("the Premium page lists {id}"));
        for (plan, id) in PLANS.iter().zip(["player-plus", "creator", "bundle", "professional"]) {
            let t = named(id);
            assert_eq!(t.name, plan.name);
            near(t.price_monthly, plan.usd, 1e-9, plan.name);
            let text = t.features.join(" | ");
            if plan.tickets > 0.0 {
                assert!(text.contains(&format!("{} Tickets", plan.tickets as i64)), "{}: the page lists {} Tickets in {text}", plan.name, plan.tickets);
            } else {
                assert!(!text.contains("Tickets"), "{}: the page lists no Tickets but the model grants none: {text}", plan.name);
            }
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
        assert_eq!(cents_words(46_263), "$462.63");
        assert_eq!(cents_words(5), "$0.05");
        assert_eq!(cents_words(123_456), "$1,234.56");
        assert_eq!(cents_words(999_999), "$9,999.99");
        assert_eq!(cents_words(1_000_000), "$10,000");
        assert_eq!(cents_words(1_000_049), "$10,000");
        assert_eq!(Unit::Percent.show(0.1), "0.1%");
        assert_eq!(Unit::Percent.show(3.0), "3%");
        assert_eq!(Unit::Rate.show(1.5), "$1.50");
        assert_eq!(Unit::Hours.show(60.0), "60 h");
        assert_eq!(Unit::Dollars.show(100_000.0), "$100,000");
    }
}
