//! # Procurement bridge: purchase orders into Slint and back
//!
//! Translates [`crate::manufacturing::PurchaseOrder`] records into the rows the
//! RFQ Builder and Purchase Order Tracker panels render, and turns panel
//! callbacks back into state machine calls.
//!
//! ## Why the `can_*` flags are probed rather than restated
//!
//! Each row carries a set of `can_send`, `can_confirm`, `can_approve` and so on,
//! which decide which buttons the panel offers. The obvious implementation is to
//! read the state and write out the rules again: send if draft, approve if to
//! approve, and so on. That would be a second copy of the state machine, and the
//! two copies would drift the first time a guard changes. Odoo's real
//! `button_confirm` refuses an order with no priced lines, for instance, which a
//! naive state-only check would miss, so the panel would offer a Confirm button
//! that fails.
//!
//! Instead [`can`] clones the order and *attempts* the transition. If the clone
//! accepts it, the button is offered; the clone is then dropped. The rules are
//! therefore never duplicated: the flags are the state machine's own answer.
//! A handful of clones per visible row is nothing next to a UI that can offer an
//! illegal action.

use crate::manufacturing::{
    InvoiceStatus, PurchaseError, PurchaseOrder, PurchaseOrderLine, PurchaseOrderState,
};
use crate::manufacturing::purchase::LineDisplayType;

use crate::ui::slint_ui::{ManufacturerOption, PurchaseOrderLineRow, PurchaseOrderRow};

/// Odoo's `po_double_validation_amount`: orders above this total route through
/// an approval step instead of confirming straight to `purchase`.
///
/// `None` matches Odoo's out-of-the-box behaviour, where double validation is
/// off. The `ToApprove` state and [`PurchaseOrder::button_approve`] are fully
/// implemented, so turning this on is a one-line change once there is a setting
/// to hang it off.
const DOUBLE_VALIDATION_AMOUNT: Option<f64> = None;

// ============================================================================
// Transitions
// ============================================================================

/// Does the state machine currently accept `action` on this order?
///
/// Answered by trying it on a throwaway clone, so this can never disagree with
/// [`apply_transition`].
pub fn can(order: &PurchaseOrder, action: &str) -> bool {
    let mut probe = order.clone();
    apply_transition(&mut probe, action).is_ok()
}

/// Apply a panel action to an order. The action strings are the ones
/// `procurement.slint` emits, and each maps onto one Odoo `button_*` method.
pub fn apply_transition(order: &mut PurchaseOrder, action: &str) -> Result<(), PurchaseError> {
    match action {
        "send" => order.print_quotation(),
        "confirm" => {
            let requires_approval = DOUBLE_VALIDATION_AMOUNT
                .map(|threshold| order.amount_total() > threshold)
                .unwrap_or(false);
            order.button_confirm(requires_approval)
        }
        "approve" => order.button_approve(today()),
        "lock" => order.button_done(),
        "unlock" => order.button_unlock(),
        "draft" => order.button_draft(),
        "cancel" => order.button_cancel(),
        // `action` is `&'static str`, so an unrecognised string cannot be
        // carried into the error without leaking it. This only fires if the
        // Slint side emits an action the match above does not know, which is a
        // wiring bug rather than a user-reachable state.
        _ => Err(PurchaseError::IllegalTransition {
            from: order.state,
            action: "unrecognised action",
        }),
    }
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// ============================================================================
// Line edits
// ============================================================================

/// Apply an edit from the lines table. Returns whether anything changed, so the
/// caller only persists on a real edit.
///
/// A field that does not parse is ignored rather than zeroing the value: a user
/// mid-way through typing "12" has momentarily written "1", and clobbering the
/// quantity to zero on an unparseable keystroke would be worse than waiting.
pub fn apply_line_change(
    order: &mut PurchaseOrder,
    index: usize,
    field: &str,
    value: &str,
) -> bool {
    let Some(line) = order.order_line.get_mut(index) else {
        return false;
    };
    match field {
        "description" => {
            line.name = value.to_string();
            true
        }
        "quantity" => match value.trim().parse::<f64>() {
            Ok(v) if v >= 0.0 => {
                line.product_qty = v;
                true
            }
            _ => false,
        },
        "unit-price" => match value.trim().parse::<f64>() {
            Ok(v) => {
                line.price_unit = v;
                true
            }
            _ => false,
        },
        "discount" => match value.trim().parse::<f64>() {
            Ok(v) if (0.0..=100.0).contains(&v) => {
                line.discount = v;
                true
            }
            _ => false,
        },
        "tax-percent" => match value.trim().parse::<f64>() {
            Ok(v) if v >= 0.0 => {
                line.tax_percent = v;
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// Apply an edit to an order-level field.
pub fn apply_field_change(order: &mut PurchaseOrder, field: &str, value: &str) -> bool {
    match field {
        "partner-ref" => {
            order.partner_ref = if value.trim().is_empty() {
                None
            } else {
                Some(value.to_string())
            };
            true
        }
        "notes" => {
            order.notes = if value.trim().is_empty() {
                None
            } else {
                Some(value.to_string())
            };
            true
        }
        _ => false,
    }
}

// ============================================================================
// Presentation
// ============================================================================

/// Human label for a state. Matches the labels Odoo shows, so someone who knows
/// Odoo reads this panel without translation.
pub fn state_label(state: PurchaseOrderState) -> &'static str {
    match state {
        PurchaseOrderState::Draft => "RFQ",
        PurchaseOrderState::Sent => "RFQ Sent",
        PurchaseOrderState::ToApprove => "To Approve",
        PurchaseOrderState::Purchase => "Purchase Order",
        PurchaseOrderState::Done => "Locked",
        PurchaseOrderState::Cancel => "Cancelled",
    }
}

/// Badge tint per state, on the deck palette.
fn state_color(state: PurchaseOrderState) -> slint::Color {
    let (r, g, b) = match state {
        PurchaseOrderState::Draft => (251, 191, 36),      // amber, still a draft
        PurchaseOrderState::Sent => (96, 165, 250),       // blue, out with the vendor
        PurchaseOrderState::ToApprove => (251, 146, 60),  // orange, waiting on a human
        PurchaseOrderState::Purchase => (74, 222, 128),   // green, committed
        PurchaseOrderState::Done => (34, 211, 238),       // cyan, locked
        PurchaseOrderState::Cancel => (248, 113, 113),    // red, dead
    };
    slint::Color::from_rgb_u8(r, g, b)
}

fn invoice_label(status: InvoiceStatus) -> &'static str {
    match status {
        InvoiceStatus::NothingToBill => "nothing to bill",
        InvoiceStatus::ToInvoice => "ready to bill",
        InvoiceStatus::Invoiced => "fully billed",
    }
}

fn money(value: f64) -> slint::SharedString {
    format!("{:.2}", value).into()
}

/// Quantities print without trailing zeros where they are whole, because "10"
/// reads better than "10.00" in a table of part counts.
fn qty(value: f64) -> slint::SharedString {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64).into()
    } else {
        format!("{}", value).into()
    }
}

fn line_row(line: &PurchaseOrderLine) -> PurchaseOrderLineRow {
    PurchaseOrderLineRow {
        description: line.name.clone().into(),
        product_id: line.product_id.clone().unwrap_or_default().into(),
        quantity: qty(line.product_qty),
        uom: line.product_uom.clone().into(),
        unit_price: money(line.price_unit),
        discount: qty(line.discount),
        tax_percent: qty(line.tax_percent),
        subtotal: money(line.price_subtotal()),
        qty_received: qty(line.qty_received),
        qty_invoiced: qty(line.qty_invoiced),
        display_type: match line.display_type {
            LineDisplayType::Product => "product",
            LineDisplayType::Section => "section",
            LineDisplayType::Note => "note",
        }
        .into(),
    }
}

/// Build the row a panel renders for one order.
///
/// `vendor_name` is resolved by the caller against the Manufacturer registry.
/// When it cannot be resolved the raw id is shown rather than an empty cell, so
/// an order pointing at a vendor that no longer exists is visible instead of
/// looking like it has no vendor at all.
pub fn order_row(order: &PurchaseOrder, vendor_name: &str) -> PurchaseOrderRow {
    let lines: Vec<PurchaseOrderLineRow> = order.order_line.iter().map(line_row).collect();

    PurchaseOrderRow {
        reference: order.name.clone().into(),
        manufacturer_id: order.manufacturer_id.clone().into(),
        manufacturer_name: if vendor_name.is_empty() {
            order.manufacturer_id.clone().into()
        } else {
            vendor_name.into()
        },
        state: serde_json::to_string(&order.state)
            .unwrap_or_default()
            .trim_matches('"')
            .into(),
        state_label: state_label(order.state).into(),
        state_color: state_color(order.state),
        is_rfq: order.is_rfq(),
        date_order: order.date_order.clone().into(),
        date_planned: order.date_planned.clone().unwrap_or_else(|| "-".into()).into(),
        currency: order.currency.clone().into(),
        amount_untaxed: money(order.amount_untaxed()),
        amount_tax: money(order.amount_tax()),
        amount_total: money(order.amount_total()),
        invoice_status: invoice_label(order.invoice_status()).into(),
        partner_ref: order.partner_ref.clone().unwrap_or_default().into(),
        notes: order.notes.clone().unwrap_or_default().into(),
        line_count: lines.len() as i32,
        lines: slint::ModelRc::new(slint::VecModel::from(lines)),

        can_send: can(order, "send"),
        can_confirm: can(order, "confirm"),
        can_approve: can(order, "approve"),
        can_lock: can(order, "lock"),
        can_unlock: can(order, "unlock"),
        can_cancel: can(order, "cancel"),
        // Resetting a draft to draft is legal but pointless, so it is not
        // offered. Everything else the machine accepts is.
        can_reset_draft: can(order, "draft") && order.state != PurchaseOrderState::Draft,
    }
}

/// Build the vendor picker model from the Manufacturer registry.
///
/// Only `Approved` manufacturers are offered. A `PendingAudit`, `Suspended`, or
/// `Blacklisted` vendor cannot be allocated work by the rest of the
/// manufacturing module, so offering one here would create an order the program
/// would then refuse to honour.
pub fn manufacturer_options(
    registry: &crate::manufacturing::ManufacturingProgramRegistry,
) -> Vec<ManufacturerOption> {
    registry
        .manufacturers
        .iter()
        .filter(|m| matches!(m.status, crate::manufacturing::ManufacturerStatus::Approved))
        .map(|m| {
            let mut certs: Vec<&str> = Vec::new();
            if m.certifications.iso_9001 {
                certs.push("ISO 9001");
            }
            if m.certifications.iatf_16949 {
                certs.push("IATF 16949");
            }
            if m.certifications.iso_14001 {
                certs.push("ISO 14001");
            }
            certs.extend(m.certifications.additional.iter().map(|s| s.as_str()));

            ManufacturerOption {
                id: m.id.clone().into(),
                name: m.name.clone().into(),
                status: format!("{:?}", m.status).into(),
                lead_time: format!("{} days", m.lead_time_days).into(),
                certifications: certs.join(", ").into(),
            }
        })
        .collect()
}

/// Resolve a vendor id to its display name, empty when unknown.
pub fn vendor_name<'a>(
    registry: &'a crate::manufacturing::ManufacturingProgramRegistry,
    id: &str,
) -> &'a str {
    registry
        .manufacturers
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.as_str())
        .unwrap_or("")
}

/// Does an order pass the Tracker's current filter chip?
pub fn passes_filter(order: &PurchaseOrder, filter: &str) -> bool {
    match filter {
        "rfq" => order.is_rfq(),
        "purchase" => matches!(
            order.state,
            PurchaseOrderState::Purchase | PurchaseOrderState::ToApprove
        ),
        "done" => order.state == PurchaseOrderState::Done,
        "cancel" => order.state == PurchaseOrderState::Cancel,
        _ => true,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn priced_rfq() -> PurchaseOrder {
        let mut o = PurchaseOrder::new_rfq("PO00001", "mfg-a", "2026-08-17");
        o.order_line.push(PurchaseOrderLine::new("Bearing", 10.0, 4.5));
        o
    }

    #[test]
    fn probing_agrees_with_the_state_machine_on_every_action() {
        // The whole design rests on this: `can` must never disagree with
        // `apply_transition`, for any state and any action.
        let actions = ["send", "confirm", "approve", "lock", "unlock", "draft", "cancel"];
        let states = [
            PurchaseOrderState::Draft,
            PurchaseOrderState::Sent,
            PurchaseOrderState::ToApprove,
            PurchaseOrderState::Purchase,
            PurchaseOrderState::Done,
            PurchaseOrderState::Cancel,
        ];
        for state in states {
            for action in actions {
                let mut order = priced_rfq();
                order.state = state;
                let predicted = can(&order, action);
                let actual = apply_transition(&mut order.clone(), action).is_ok();
                assert_eq!(
                    predicted, actual,
                    "can() and apply_transition() disagreed on {action} from {state:?}"
                );
            }
        }
    }

    #[test]
    fn an_unpriced_rfq_is_not_offered_a_confirm_button() {
        // A state-only check would wrongly offer Confirm here, because the
        // state is draft. Probing catches the NoOrderLines guard.
        let empty = PurchaseOrder::new_rfq("PO00002", "mfg-a", "2026-08-17");
        assert!(!can(&empty, "confirm"));
        assert!(can(&priced_rfq(), "confirm"));
    }

    #[test]
    fn an_order_with_bills_is_not_offered_a_cancel_button() {
        let mut order = priced_rfq();
        order.has_invoices = true;
        assert!(!can(&order, "cancel"));
    }

    #[test]
    fn a_draft_is_not_offered_a_pointless_reset_to_draft() {
        let row = order_row(&priced_rfq(), "Acme");
        assert!(!row.can_reset_draft);
    }

    #[test]
    fn state_serialises_to_the_odoo_wire_value_for_the_row() {
        let mut order = priced_rfq();
        order.state = PurchaseOrderState::ToApprove;
        let row = order_row(&order, "Acme");
        assert_eq!(row.state, "to approve");
        assert_eq!(row.state_label, "To Approve");
    }

    #[test]
    fn an_unresolvable_vendor_shows_its_id_rather_than_an_empty_cell() {
        let row = order_row(&priced_rfq(), "");
        assert_eq!(row.manufacturer_name, "mfg-a");
    }

    #[test]
    fn a_half_typed_quantity_does_not_clobber_the_value() {
        let mut order = priced_rfq();
        assert!(!apply_line_change(&mut order, 0, "quantity", ""));
        assert_eq!(order.order_line[0].product_qty, 10.0, "unchanged");
        assert!(apply_line_change(&mut order, 0, "quantity", "12"));
        assert_eq!(order.order_line[0].product_qty, 12.0);
    }

    #[test]
    fn an_out_of_range_discount_is_refused() {
        let mut order = priced_rfq();
        assert!(!apply_line_change(&mut order, 0, "discount", "150"));
        assert!(!apply_line_change(&mut order, 0, "discount", "-5"));
        assert!(apply_line_change(&mut order, 0, "discount", "10"));
        assert_eq!(order.order_line[0].discount, 10.0);
    }

    #[test]
    fn filters_partition_the_population() {
        let mut rfq = priced_rfq();
        let mut po = priced_rfq();
        po.button_confirm(false).unwrap();

        assert!(passes_filter(&rfq, "rfq"));
        assert!(!passes_filter(&rfq, "purchase"));
        assert!(passes_filter(&po, "purchase"));
        assert!(!passes_filter(&po, "rfq"));
        assert!(passes_filter(&rfq, "all") && passes_filter(&po, "all"));

        rfq.button_cancel().unwrap();
        assert!(passes_filter(&rfq, "cancel"));
    }
}
