//! # Purchase Orders & RFQs — Odoo `purchase` model, ported to Rust
//!
//! A faithful port of Odoo 17.0's `purchase.order` / `purchase.order.line`
//! data model and state machine, with the vendor bound to Eustress's own
//! [`super::Manufacturer`] registry instead of Odoo's `res.partner`.
//!
//! ## The one structural fact that drives this whole module
//!
//! **An RFQ is not a separate entity.** In Odoo it is a `purchase.order` in
//! state `draft` or `sent`; confirming it turns that same record into a
//! Purchase Order, preserving its id, lines, and history. A port that
//! introduces a distinct `RequestForQuote` type — and then has to copy fields
//! across on confirmation — has diverged from Odoo on the most structural
//! point in the model, and inherits a class of bugs (drifted copies, lost
//! provenance, two sources of truth) that Odoo's design does not have.
//!
//! So: one [`PurchaseOrder`] type, one lifecycle, and [`PurchaseOrder::is_rfq`]
//! to ask which face it is currently showing.
//!
//! ```text
//!   Draft ──print_quotation──▶ Sent ──confirm──▶ ToApprove ──approve──▶ Purchase
//!     ▲                          │                   │                    │  ▲
//!     │                          └───────confirm─────┘                 done│  │unlock
//!     └──────────── set_draft ────────────────────────────────┐            ▼  │
//!                                                             │          Locked
//!   (cancel is reachable from any non-terminal state) ────────┴──▶ Cancelled
//! ```
//!
//! ## Provenance and licensing
//!
//! Odoo Community is LGPL-3.0 and Eustress is PolyForm Shield 1.0.0. This port
//! is written from the *data model* — field names, types, and state values,
//! recorded in the transpile reference — which is the functional interface, not
//! Odoo's implementation. No Odoo source is copied here. Whether to take a
//! deeper dependency on LGPL code is a licence decision, and licence decisions
//! are human-only (`docs/PROMPTS/00_MASTER_PROTOCOL.md` §6).
//!
//! ## Money
//!
//! Odoo stores monetary values as floats and rounds through
//! `currency_id.round()`. This port keeps `f64` for fidelity and rounds at the
//! same boundaries via [`round_currency`]. That is Odoo-faithful, not
//! financially ideal — minor-unit integers or a decimal type would be the
//! stronger choice, and is the natural follow-up if these orders ever settle
//! real money rather than model a program.

use serde::{Deserialize, Serialize};

// ============================================================================
// 1. State
// ============================================================================

/// Lifecycle state. Wire values match Odoo's `state` selection exactly, so a
/// record can round-trip to and from an Odoo instance without translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurchaseOrderState {
    /// Odoo `draft` — labelled "RFQ".
    #[serde(rename = "draft")]
    Draft,
    /// Odoo `sent` — "RFQ Sent". Quotation has gone to the vendor.
    #[serde(rename = "sent")]
    Sent,
    /// Odoo `to approve` — awaiting internal approval.
    #[serde(rename = "to approve")]
    ToApprove,
    /// Odoo `purchase` — a confirmed Purchase Order.
    #[serde(rename = "purchase")]
    Purchase,
    /// Odoo `done` — "Locked".
    #[serde(rename = "done")]
    Done,
    /// Odoo `cancel`.
    #[serde(rename = "cancel")]
    Cancel,
}

impl PurchaseOrderState {
    /// The exact Odoo wire string.
    pub fn as_odoo_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Sent => "sent",
            Self::ToApprove => "to approve",
            Self::Purchase => "purchase",
            Self::Done => "done",
            Self::Cancel => "cancel",
        }
    }

    /// Odoo's UI label for the state.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Draft => "RFQ",
            Self::Sent => "RFQ Sent",
            Self::ToApprove => "To Approve",
            Self::Purchase => "Purchase Order",
            Self::Done => "Locked",
            Self::Cancel => "Cancelled",
        }
    }

    /// True while the record is still a Request For Quote rather than a
    /// committed order — Odoo's `draft` and `sent`.
    pub fn is_rfq(&self) -> bool {
        matches!(self, Self::Draft | Self::Sent)
    }

    /// No further transitions are expected from here.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Cancel)
    }
}

/// Odoo's `invoice_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum InvoiceStatus {
    /// `no` — nothing to bill yet.
    #[serde(rename = "no")]
    #[default]
    NothingToBill,
    /// `to invoice` — received quantities are awaiting a bill.
    #[serde(rename = "to invoice")]
    ToInvoice,
    /// `invoiced` — fully billed.
    #[serde(rename = "invoiced")]
    Invoiced,
}

/// Why a transition was refused. Odoo raises `UserError`; Rust returns this so
/// an illegal transition is a value the caller must handle rather than a
/// panic — the state machine is enforced at the type boundary, not by
/// convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurchaseError {
    /// The attempted transition isn't legal from the current state.
    IllegalTransition {
        from: PurchaseOrderState,
        action: &'static str,
    },
    /// Odoo blocks cancelling an order that already has vendor bills.
    CancelBlockedByInvoices,
    /// An order must have at least one non-display line to be confirmed.
    NoOrderLines,
}

impl std::fmt::Display for PurchaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IllegalTransition { from, action } => write!(
                f,
                "cannot {} an order in state '{}' ({})",
                action,
                from.as_odoo_str(),
                from.label()
            ),
            Self::CancelBlockedByInvoices => {
                write!(f, "cannot cancel: the order already has vendor bills")
            }
            Self::NoOrderLines => write!(f, "cannot confirm an order with no order lines"),
        }
    }
}

impl std::error::Error for PurchaseError {}

// ============================================================================
// 2. Order lines
// ============================================================================

/// Odoo's `display_type` — lets a line act as a section header or a note
/// rather than a priced item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LineDisplayType {
    /// A normal priced product line.
    #[serde(rename = "")]
    #[default]
    Product,
    /// Odoo `line_section` — a heading.
    #[serde(rename = "line_section")]
    Section,
    /// Odoo `line_note` — free text.
    #[serde(rename = "line_note")]
    Note,
}

/// A single line — Odoo `purchase.order.line`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseOrderLine {
    /// Odoo `name` — the description shown on the order.
    pub name: String,
    /// Odoo `sequence`, default 10; controls display order.
    #[serde(default = "default_sequence")]
    pub sequence: i32,
    /// Odoo `product_id`. Free-text product key rather than a FK, since
    /// Eustress addresses parts by path/name rather than an ERP product table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    /// Odoo `product_qty`.
    pub product_qty: f64,
    /// Odoo `product_uom` — unit of measure ("pcs", "kg", "m").
    #[serde(default = "default_uom")]
    pub product_uom: String,
    /// Odoo `price_unit`.
    pub price_unit: f64,
    /// Odoo `discount` — a percentage, 0.0–100.0.
    #[serde(default)]
    pub discount: f64,
    /// Odoo `taxes_id`, flattened to a summed percentage. Odoo supports a
    /// many2many of tax records with their own computation modes; this port
    /// carries the effective total percentage, which covers the common case.
    #[serde(default)]
    pub tax_percent: f64,
    /// Odoo `date_planned` — expected arrival, ISO 8601.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_planned: Option<String>,
    /// Odoo `qty_received`.
    #[serde(default)]
    pub qty_received: f64,
    /// Odoo `qty_invoiced`.
    #[serde(default)]
    pub qty_invoiced: f64,
    /// Odoo `display_type`.
    #[serde(default)]
    pub display_type: LineDisplayType,
}

fn default_sequence() -> i32 {
    10
}

fn default_uom() -> String {
    "pcs".to_string()
}

impl PurchaseOrderLine {
    /// A priced product line.
    pub fn new(name: impl Into<String>, product_qty: f64, price_unit: f64) -> Self {
        Self {
            name: name.into(),
            sequence: default_sequence(),
            product_id: None,
            product_qty,
            product_uom: default_uom(),
            price_unit,
            discount: 0.0,
            tax_percent: 0.0,
            date_planned: None,
            qty_received: 0.0,
            qty_invoiced: 0.0,
            display_type: LineDisplayType::Product,
        }
    }

    /// A section heading — carries no price (Odoo `line_section`).
    pub fn section(name: impl Into<String>) -> Self {
        Self {
            display_type: LineDisplayType::Section,
            ..Self::new(name, 0.0, 0.0)
        }
    }

    /// A free-text note (Odoo `line_note`).
    pub fn note(name: impl Into<String>) -> Self {
        Self {
            display_type: LineDisplayType::Note,
            ..Self::new(name, 0.0, 0.0)
        }
    }

    /// Whether this line contributes to the order totals. Section and note
    /// lines never do — matching Odoo, which excludes them from `_compute_amount`.
    pub fn is_priced(&self) -> bool {
        self.display_type == LineDisplayType::Product
    }

    /// Odoo `price_subtotal` — quantity × unit price, less the discount
    /// percentage, before tax.
    pub fn price_subtotal(&self) -> f64 {
        if !self.is_priced() {
            return 0.0;
        }
        let gross = self.product_qty * self.price_unit;
        round_currency(gross * (1.0 - self.discount / 100.0))
    }

    /// Odoo `price_tax` — tax on the discounted subtotal.
    pub fn price_tax(&self) -> f64 {
        if !self.is_priced() {
            return 0.0;
        }
        round_currency(self.price_subtotal() * self.tax_percent / 100.0)
    }

    /// Odoo `price_total` — `price_subtotal` + `price_tax`.
    pub fn price_total(&self) -> f64 {
        round_currency(self.price_subtotal() + self.price_tax())
    }

    /// Odoo `qty_to_invoice` — received but not yet billed.
    pub fn qty_to_invoice(&self) -> f64 {
        self.qty_received - self.qty_invoiced
    }
}

// ============================================================================
// 3. The order (and therefore the RFQ)
// ============================================================================

/// Odoo `purchase.order`. In [`PurchaseOrderState::Draft`] or
/// [`PurchaseOrderState::Sent`] this record *is* the RFQ; confirming it makes
/// the same record a Purchase Order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseOrder {
    /// Odoo `name` — the order reference, e.g. `"PO00001"`.
    pub name: String,
    /// Odoo `partner_id`, bound to [`super::Manufacturer::id`] rather than
    /// `res.partner`: in Eustress the vendor is a registered manufacturer.
    pub manufacturer_id: String,
    /// Odoo `state`.
    pub state: PurchaseOrderState,
    /// Odoo `date_order` — order deadline, ISO 8601.
    pub date_order: String,
    /// Odoo `date_approve` — set when approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_approve: Option<String>,
    /// Odoo `date_planned` — expected arrival; the earliest line date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_planned: Option<String>,
    /// Odoo `currency_id` — ISO 4217, e.g. `"USD"`.
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Odoo `partner_ref` — the vendor's own reference for this order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_ref: Option<String>,
    /// Odoo `origin` — the source document that generated this order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Odoo `user_id` — the buyer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buyer: Option<String>,
    /// Odoo `notes` — terms and conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Odoo `order_line`.
    #[serde(default)]
    pub order_line: Vec<PurchaseOrderLine>,
    /// Set when a vendor bill exists; gates cancellation as Odoo does.
    #[serde(default)]
    pub has_invoices: bool,
}

fn default_currency() -> String {
    "USD".to_string()
}

impl PurchaseOrder {
    /// Open a new RFQ against a manufacturer. Odoo's default state is `draft`,
    /// which is precisely an RFQ — so this constructor *is* "create RFQ".
    pub fn new_rfq(
        name: impl Into<String>,
        manufacturer_id: impl Into<String>,
        date_order: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            manufacturer_id: manufacturer_id.into(),
            state: PurchaseOrderState::Draft,
            date_order: date_order.into(),
            date_approve: None,
            date_planned: None,
            currency: default_currency(),
            partner_ref: None,
            origin: None,
            buyer: None,
            notes: None,
            order_line: Vec::new(),
            has_invoices: false,
        }
    }

    /// Is this record currently an RFQ rather than a committed order?
    pub fn is_rfq(&self) -> bool {
        self.state.is_rfq()
    }

    /// Odoo `amount_untaxed`.
    pub fn amount_untaxed(&self) -> f64 {
        round_currency(self.order_line.iter().map(|l| l.price_subtotal()).sum())
    }

    /// Odoo `amount_tax`.
    pub fn amount_tax(&self) -> f64 {
        round_currency(self.order_line.iter().map(|l| l.price_tax()).sum())
    }

    /// Odoo `amount_total`.
    pub fn amount_total(&self) -> f64 {
        round_currency(self.amount_untaxed() + self.amount_tax())
    }

    /// Odoo `invoice_status`, derived from line quantities.
    pub fn invoice_status(&self) -> InvoiceStatus {
        let priced: Vec<_> = self.order_line.iter().filter(|l| l.is_priced()).collect();
        if priced.is_empty() || self.state.is_rfq() {
            return InvoiceStatus::NothingToBill;
        }
        if priced.iter().any(|l| l.qty_to_invoice() > 0.0) {
            InvoiceStatus::ToInvoice
        } else if priced.iter().all(|l| l.qty_invoiced >= l.product_qty) {
            InvoiceStatus::Invoiced
        } else {
            InvoiceStatus::NothingToBill
        }
    }

    /// Lines that actually carry a price.
    pub fn priced_lines(&self) -> impl Iterator<Item = &PurchaseOrderLine> {
        self.order_line.iter().filter(|l| l.is_priced())
    }

    // --- transitions (Odoo's `button_*` / `print_quotation`) ---------------

    /// Odoo `print_quotation` — the RFQ has been sent to the vendor.
    /// `draft` → `sent`.
    pub fn print_quotation(&mut self) -> Result<(), PurchaseError> {
        match self.state {
            PurchaseOrderState::Draft => {
                self.state = PurchaseOrderState::Sent;
                Ok(())
            }
            from => Err(PurchaseError::IllegalTransition {
                from,
                action: "send",
            }),
        }
    }

    /// Odoo `button_confirm`. From an RFQ state this moves to `to approve`
    /// when approval is required, otherwise straight to `purchase` — Odoo's
    /// auto-approve behaviour, expressed here as an explicit argument rather
    /// than a hidden company setting.
    pub fn button_confirm(&mut self, requires_approval: bool) -> Result<(), PurchaseError> {
        if !self.state.is_rfq() {
            return Err(PurchaseError::IllegalTransition {
                from: self.state,
                action: "confirm",
            });
        }
        if !self.order_line.iter().any(|l| l.is_priced()) {
            return Err(PurchaseError::NoOrderLines);
        }
        self.state = if requires_approval {
            PurchaseOrderState::ToApprove
        } else {
            PurchaseOrderState::Purchase
        };
        Ok(())
    }

    /// Odoo `button_approve` — `to approve` → `purchase`, stamping
    /// `date_approve`.
    pub fn button_approve(&mut self, approved_at: impl Into<String>) -> Result<(), PurchaseError> {
        match self.state {
            PurchaseOrderState::ToApprove => {
                self.state = PurchaseOrderState::Purchase;
                self.date_approve = Some(approved_at.into());
                Ok(())
            }
            from => Err(PurchaseError::IllegalTransition {
                from,
                action: "approve",
            }),
        }
    }

    /// Odoo `button_done` — lock a confirmed order.
    pub fn button_done(&mut self) -> Result<(), PurchaseError> {
        match self.state {
            PurchaseOrderState::Purchase => {
                self.state = PurchaseOrderState::Done;
                Ok(())
            }
            from => Err(PurchaseError::IllegalTransition {
                from,
                action: "lock",
            }),
        }
    }

    /// Odoo `button_unlock` — `done` → `purchase`.
    pub fn button_unlock(&mut self) -> Result<(), PurchaseError> {
        match self.state {
            PurchaseOrderState::Done => {
                self.state = PurchaseOrderState::Purchase;
                Ok(())
            }
            from => Err(PurchaseError::IllegalTransition {
                from,
                action: "unlock",
            }),
        }
    }

    /// Odoo `button_draft` — reset to RFQ.
    pub fn button_draft(&mut self) -> Result<(), PurchaseError> {
        match self.state {
            PurchaseOrderState::Done => Err(PurchaseError::IllegalTransition {
                from: self.state,
                action: "reset to draft",
            }),
            _ => {
                self.state = PurchaseOrderState::Draft;
                self.date_approve = None;
                Ok(())
            }
        }
    }

    /// Odoo `button_cancel`. Refused when vendor bills exist, and refused from
    /// terminal states.
    pub fn button_cancel(&mut self) -> Result<(), PurchaseError> {
        if self.has_invoices {
            return Err(PurchaseError::CancelBlockedByInvoices);
        }
        if self.state.is_terminal() {
            return Err(PurchaseError::IllegalTransition {
                from: self.state,
                action: "cancel",
            });
        }
        self.state = PurchaseOrderState::Cancel;
        Ok(())
    }
}

/// Round to 2 decimal places, standing in for Odoo's `currency_id.round()`.
/// See the module docs on why this is `f64` rather than a decimal type.
pub fn round_currency(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Format a sequential reference the way Odoo's `ir.sequence` does for
/// purchase orders: `PO` + 5-digit zero-padded counter.
pub fn format_order_reference(counter: u32) -> String {
    format!("PO{:05}", counter)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn rfq_with_lines() -> PurchaseOrder {
        let mut po = PurchaseOrder::new_rfq("PO00001", "mfr-acme", "2026-08-09T00:00:00Z");
        po.order_line.push(PurchaseOrderLine::new("Aluminium plate 6061", 10.0, 25.0));
        po
    }

    #[test]
    fn state_wire_values_match_odoo_exactly() {
        // These strings are the interop contract with a real Odoo instance.
        assert_eq!(PurchaseOrderState::Draft.as_odoo_str(), "draft");
        assert_eq!(PurchaseOrderState::Sent.as_odoo_str(), "sent");
        assert_eq!(PurchaseOrderState::ToApprove.as_odoo_str(), "to approve");
        assert_eq!(PurchaseOrderState::Purchase.as_odoo_str(), "purchase");
        assert_eq!(PurchaseOrderState::Done.as_odoo_str(), "done");
        assert_eq!(PurchaseOrderState::Cancel.as_odoo_str(), "cancel");
        // …and Odoo labels draft as "RFQ", which is why there is no separate type.
        assert_eq!(PurchaseOrderState::Draft.label(), "RFQ");
    }

    #[test]
    fn an_rfq_becomes_a_purchase_order_without_changing_identity() {
        let mut po = rfq_with_lines();
        assert!(po.is_rfq());
        let reference = po.name.clone();
        let line_count = po.order_line.len();

        po.print_quotation().unwrap();
        assert_eq!(po.state, PurchaseOrderState::Sent);
        assert!(po.is_rfq(), "a sent RFQ is still an RFQ");

        po.button_confirm(false).unwrap();
        assert_eq!(po.state, PurchaseOrderState::Purchase);
        assert!(!po.is_rfq());

        // The whole point of Odoo's design: same record, same lines.
        assert_eq!(po.name, reference);
        assert_eq!(po.order_line.len(), line_count);
    }

    #[test]
    fn confirm_routes_through_approval_when_required() {
        let mut po = rfq_with_lines();
        po.button_confirm(true).unwrap();
        assert_eq!(po.state, PurchaseOrderState::ToApprove);
        assert!(po.date_approve.is_none());

        po.button_approve("2026-08-10T12:00:00Z").unwrap();
        assert_eq!(po.state, PurchaseOrderState::Purchase);
        assert_eq!(po.date_approve.as_deref(), Some("2026-08-10T12:00:00Z"));
    }

    #[test]
    fn empty_rfq_cannot_be_confirmed() {
        let mut po = PurchaseOrder::new_rfq("PO00002", "mfr-acme", "2026-08-09T00:00:00Z");
        assert_eq!(po.button_confirm(false), Err(PurchaseError::NoOrderLines));
        // A section alone is not an order.
        po.order_line.push(PurchaseOrderLine::section("Frame parts"));
        assert_eq!(po.button_confirm(false), Err(PurchaseError::NoOrderLines));
    }

    #[test]
    fn illegal_transitions_are_refused_not_silently_ignored() {
        let mut po = rfq_with_lines();
        // Cannot approve something that was never confirmed.
        assert!(matches!(
            po.button_approve("now"),
            Err(PurchaseError::IllegalTransition { .. })
        ));
        // Cannot lock an RFQ.
        assert!(matches!(
            po.button_done(),
            Err(PurchaseError::IllegalTransition { .. })
        ));
        // Cannot re-send an already-sent RFQ.
        po.print_quotation().unwrap();
        assert!(matches!(
            po.print_quotation(),
            Err(PurchaseError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn cancel_is_blocked_by_vendor_bills() {
        let mut po = rfq_with_lines();
        po.button_confirm(false).unwrap();
        po.has_invoices = true;
        assert_eq!(po.button_cancel(), Err(PurchaseError::CancelBlockedByInvoices));
        po.has_invoices = false;
        po.button_cancel().unwrap();
        assert_eq!(po.state, PurchaseOrderState::Cancel);
    }

    #[test]
    fn locked_orders_cannot_be_reset_to_draft() {
        let mut po = rfq_with_lines();
        po.button_confirm(false).unwrap();
        po.button_done().unwrap();
        assert!(matches!(
            po.button_draft(),
            Err(PurchaseError::IllegalTransition { .. })
        ));
        // …but unlocking first is the supported route back.
        po.button_unlock().unwrap();
        po.button_draft().unwrap();
        assert_eq!(po.state, PurchaseOrderState::Draft);
        assert!(po.date_approve.is_none(), "approval stamp clears on reset");
    }

    #[test]
    fn totals_apply_discount_before_tax() {
        let mut line = PurchaseOrderLine::new("Widget", 10.0, 100.0);
        line.discount = 10.0; // 1000 -> 900
        line.tax_percent = 8.25; // 900 * 0.0825 = 74.25
        assert_eq!(line.price_subtotal(), 900.0);
        assert_eq!(line.price_tax(), 74.25);
        assert_eq!(line.price_total(), 974.25);
    }

    #[test]
    fn section_and_note_lines_never_affect_totals() {
        let mut po = rfq_with_lines(); // 10 x 25.00 = 250.00
        po.order_line.push(PurchaseOrderLine::section("Fasteners"));
        po.order_line.push(PurchaseOrderLine::note("Deliver to dock B"));
        assert_eq!(po.amount_untaxed(), 250.0);
        assert_eq!(po.amount_total(), 250.0);
        assert_eq!(po.priced_lines().count(), 1);
    }

    #[test]
    fn invoice_status_tracks_received_versus_billed() {
        let mut po = rfq_with_lines();
        // An RFQ has nothing to bill regardless of quantities.
        assert_eq!(po.invoice_status(), InvoiceStatus::NothingToBill);

        po.button_confirm(false).unwrap();
        assert_eq!(po.invoice_status(), InvoiceStatus::NothingToBill);

        po.order_line[0].qty_received = 10.0;
        assert_eq!(po.invoice_status(), InvoiceStatus::ToInvoice);

        po.order_line[0].qty_invoiced = 10.0;
        assert_eq!(po.invoice_status(), InvoiceStatus::Invoiced);
    }

    #[test]
    fn order_reference_matches_odoo_sequence_format() {
        assert_eq!(format_order_reference(1), "PO00001");
        assert_eq!(format_order_reference(42), "PO00042");
        assert_eq!(format_order_reference(99999), "PO99999");
    }

    #[test]
    fn state_serialises_to_odoo_wire_strings() {
        // Round-trip through serde so a persisted order stays Odoo-compatible.
        let json = serde_json::to_string(&PurchaseOrderState::ToApprove).unwrap();
        assert_eq!(json, "\"to approve\"");
        let back: PurchaseOrderState = serde_json::from_str("\"to approve\"").unwrap();
        assert_eq!(back, PurchaseOrderState::ToApprove);
    }
}
