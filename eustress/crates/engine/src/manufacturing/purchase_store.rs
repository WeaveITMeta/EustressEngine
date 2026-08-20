//! # Purchase order persistence: DataService-backed, Space-local, lazy
//!
//! [`super::purchase`] is the pure domain model: the types, the Odoo state
//! machine, and the money, with no I/O and no Bevy in it. That separation is
//! deliberate, because it lets the whole state machine be tested in seconds
//! without building the engine. This module is the other half: where orders
//! live, how they are numbered, and how the rest of the engine reaches them.
//!
//! ## Orders are Space objects, not loose files
//!
//! They live under the Space's own `DataService`, as real class instances:
//!
//! ```text
//! DataService/
//!   Vendors/                       Folder
//!     Kazakh Scandium Refinery/    Manufacturer
//!   PurchaseOrders/                Folder, carries `next_sequence`
//!     PO00001/                     PurchaseOrder
//!       001 Scandium oxide 4N/     PurchaseOrderLine
//!       002 Certificate of analysis/
//! ```
//!
//! An earlier version kept them as flat TOML in a bare `Manufacturing/`
//! directory. That directory had no `_service.toml`, so the service loader never
//! walked it and nothing in it could ever appear in the Explorer or Properties.
//! They were data the engine read but did not own. Modelling them as instances
//! under `DataService` makes them selectable, inspectable, and scriptable like
//! every other object, and it mirrors how the Data Platform already models
//! `Dataset` and its child `Series`.
//!
//! Every field lives in `[attributes]`, which the Properties panel renders and
//! edits generically and type-preservingly. That is the same choice `Dataset`
//! makes, and it avoids a hand-written query in `sync_properties_to_slint`,
//! which is already at its system-parameter ceiling.
//!
//! ## Created only on demand
//!
//! Nothing here is scaffolded when a Space is created. `Vendors/` appears the
//! first time a vendor is registered and `PurchaseOrders/` the first time an RFQ
//! is raised, so a Space that never opens the Procurement tools stays clean. A
//! missing directory is not an error, it is simply a Space that has not bought
//! anything.
//!
//! ## Vendors are per-Space
//!
//! Deliberately not global. A supply base varies from project to project, and
//! forcing every Space to share one vendor list means an order in one project
//! can name a vendor that only exists for another.
//!
//! ## Numbering
//!
//! Odoo allocates references from `ir.sequence`. There is no sequence table
//! here, so the next counter is the greater of the `next_sequence` attribute on
//! the `PurchaseOrders` folder and one past the highest reference actually on
//! disk. Taking the max of both is what makes a duplicate reference unmintable:
//! if the attribute is lost, stale, or hand-edited, the folders still answer.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::purchase::{
    format_order_reference, LineDisplayType, PurchaseOrder, PurchaseOrderLine, PurchaseOrderState,
};

/// Orders live under the Space's DataService, relative to the Space root.
pub const ORDERS_DIR: &str = "DataService/PurchaseOrders";
/// Vendors likewise. Space-local, never global.
pub const VENDORS_DIR: &str = "DataService/Vendors";

const INSTANCE: &str = "_instance.toml";

// ============================================================================
// Instance TOML helpers
// ============================================================================

/// Build the `[metadata]` + `[attributes]` document for one instance.
fn instance_doc(class_name: &str, name: &str, attrs: toml::value::Table) -> String {
    let mut root = toml::value::Table::new();

    let mut meta = toml::value::Table::new();
    meta.insert("class_name".into(), class_name.into());
    meta.insert("archivable".into(), true.into());
    root.insert("metadata".into(), toml::Value::Table(meta));

    let mut props = toml::value::Table::new();
    props.insert("name".into(), name.into());
    props.insert("class_name".into(), class_name.into());
    root.insert("properties".into(), toml::Value::Table(props));

    root.insert("attributes".into(), toml::Value::Table(attrs));

    toml::to_string_pretty(&toml::Value::Table(root))
        .unwrap_or_else(|e| format!("# serialisation failed: {e}\n"))
}

fn read_doc(path: &Path) -> Option<toml::value::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<toml::Value>(&text) {
        Ok(toml::Value::Table(t)) => Some(t),
        _ => None,
    }
}

fn attrs_of(doc: &toml::value::Table) -> Option<&toml::value::Table> {
    doc.get("attributes")?.as_table()
}

fn s(a: &toml::value::Table, k: &str) -> Option<String> {
    a.get(k)?.as_str().map(str::to_string)
}
fn f(a: &toml::value::Table, k: &str) -> f64 {
    a.get(k)
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        .unwrap_or(0.0)
}
fn i(a: &toml::value::Table, k: &str) -> i64 {
    a.get(k)
        .and_then(|v| v.as_integer().or_else(|| v.as_float().map(|x| x as i64)))
        .unwrap_or(0)
}
fn b(a: &toml::value::Table, k: &str) -> bool {
    a.get(k).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Odoo's exact wire values, which are also what the domain model serialises to.
fn state_from_wire(w: &str) -> PurchaseOrderState {
    match w {
        "sent" => PurchaseOrderState::Sent,
        "to approve" => PurchaseOrderState::ToApprove,
        "purchase" => PurchaseOrderState::Purchase,
        "done" => PurchaseOrderState::Done,
        "cancel" => PurchaseOrderState::Cancel,
        _ => PurchaseOrderState::Draft,
    }
}

fn state_to_wire(s: PurchaseOrderState) -> &'static str {
    match s {
        PurchaseOrderState::Draft => "draft",
        PurchaseOrderState::Sent => "sent",
        PurchaseOrderState::ToApprove => "to approve",
        PurchaseOrderState::Purchase => "purchase",
        PurchaseOrderState::Done => "done",
        PurchaseOrderState::Cancel => "cancel",
    }
}

fn display_from_wire(w: &str) -> LineDisplayType {
    match w {
        "line_section" => LineDisplayType::Section,
        "line_note" => LineDisplayType::Note,
        _ => LineDisplayType::Product,
    }
}

fn display_to_wire(d: LineDisplayType) -> &'static str {
    match d {
        LineDisplayType::Product => "",
        LineDisplayType::Section => "line_section",
        LineDisplayType::Note => "line_note",
    }
}

/// Folder name for a child entity: safe on every filesystem, and ordered.
///
/// Prefixed with the sequence so the Explorer lists lines in order, since it
/// sorts children by name.
fn line_folder(index: usize, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if r#"\/:*?"<>|."#.contains(c) { '-' } else { c })
        .collect();
    let safe = safe.trim().chars().take(48).collect::<String>();
    let safe = safe.trim_end().to_string();
    if safe.is_empty() {
        format!("{:03}", index + 1)
    } else {
        format!("{:03} {}", index + 1, safe)
    }
}

// ============================================================================
// Registry
// ============================================================================

/// Every purchase order and RFQ in the open Space, plus the counter that names
/// the next one.
///
/// RFQs and purchase orders are the same records here, exactly as in Odoo. Ask
/// [`PurchaseOrder::is_rfq`] which face a record is showing; there is no
/// separate RFQ collection that could fall out of step.
#[derive(Debug, Default, Resource)]
pub struct PurchaseOrderRegistry {
    /// All orders, RFQs included, sorted by reference.
    pub orders: Vec<PurchaseOrder>,
    next_sequence: u32,
    root: PathBuf,
}

impl PurchaseOrderRegistry {
    /// Load every order in a Space. A missing directory means no orders yet.
    pub fn load(space_root: &Path) -> Self {
        let dir = space_root.join(ORDERS_DIR);
        let mut orders: Vec<PurchaseOrder> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                match Self::read_order(&path) {
                    Some(o) => orders.push(o),
                    // Loud, not silent. A folder that fails to parse is an order
                    // the user believes exists, and quietly dropping a record
                    // that may represent committed money is the worst outcome
                    // available.
                    None => tracing::error!(
                        "PurchaseOrderRegistry: {} did not parse as a PurchaseOrder and was NOT loaded",
                        path.display()
                    ),
                }
            }
        }

        orders.sort_by(|a, b| a.name.cmp(&b.name));

        let persisted = read_doc(&dir.join(INSTANCE))
            .as_ref()
            .and_then(attrs_of)
            .map(|a| i(a, "next_sequence") as u32)
            .unwrap_or(1);

        // One past the highest reference actually on disk. This is the half that
        // makes a lost counter harmless.
        let observed = orders
            .iter()
            .filter_map(|o| o.name.strip_prefix("PO"))
            .filter_map(|d| d.parse::<u32>().ok())
            .max()
            .map(|hi| hi + 1)
            .unwrap_or(1);

        Self {
            orders,
            next_sequence: persisted.max(observed).max(1),
            root: space_root.to_path_buf(),
        }
    }

    fn read_order(dir: &Path) -> Option<PurchaseOrder> {
        let doc = read_doc(&dir.join(INSTANCE))?;
        if doc
            .get("metadata")?
            .as_table()?
            .get("class_name")?
            .as_str()?
            != "PurchaseOrder"
        {
            return None;
        }
        let a = attrs_of(&doc)?;

        let mut order = PurchaseOrder::new_rfq(
            s(a, "reference").unwrap_or_else(|| {
                dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            }),
            s(a, "manufacturer_id").unwrap_or_default(),
            s(a, "date_order").unwrap_or_default(),
        );
        order.state = state_from_wire(&s(a, "state").unwrap_or_else(|| "draft".into()));
        order.date_approve = s(a, "date_approve").filter(|v| !v.is_empty());
        order.date_planned = s(a, "date_planned").filter(|v| !v.is_empty());
        order.currency = s(a, "currency").unwrap_or_else(|| "USD".into());
        order.partner_ref = s(a, "partner_ref").filter(|v| !v.is_empty());
        order.origin = s(a, "origin").filter(|v| !v.is_empty());
        order.buyer = s(a, "buyer").filter(|v| !v.is_empty());
        order.notes = s(a, "notes").filter(|v| !v.is_empty());
        order.has_invoices = b(a, "has_invoices");

        // Line children, ordered by their `sequence` attribute rather than by
        // folder name, so a hand-renamed folder cannot reorder the money.
        let mut lines: Vec<(i32, PurchaseOrderLine)> = Vec::new();
        if let Ok(children) = std::fs::read_dir(dir) {
            for child in children.flatten() {
                let cp = child.path();
                if !cp.is_dir() {
                    continue;
                }
                if let Some(l) = Self::read_line(&cp) {
                    lines.push(l);
                }
            }
        }
        lines.sort_by_key(|(seq, _)| *seq);
        order.order_line = lines.into_iter().map(|(_, l)| l).collect();

        Some(order)
    }

    fn read_line(dir: &Path) -> Option<(i32, PurchaseOrderLine)> {
        let doc = read_doc(&dir.join(INSTANCE))?;
        if doc
            .get("metadata")?
            .as_table()?
            .get("class_name")?
            .as_str()?
            != "PurchaseOrderLine"
        {
            return None;
        }
        let a = attrs_of(&doc)?;
        let seq = i(a, "sequence") as i32;

        let mut line = PurchaseOrderLine::new(
            s(a, "name").unwrap_or_default(),
            f(a, "product_qty"),
            f(a, "price_unit"),
        );
        line.sequence = seq;
        line.product_id = s(a, "product_id").filter(|v| !v.is_empty());
        line.product_uom = s(a, "product_uom").unwrap_or_else(|| "pcs".into());
        line.discount = f(a, "discount");
        line.tax_percent = f(a, "tax_percent");
        line.date_planned = s(a, "date_planned").filter(|v| !v.is_empty());
        line.qty_received = f(a, "qty_received");
        line.qty_invoiced = f(a, "qty_invoiced");
        line.display_type =
            display_from_wire(&s(a, "display_type").unwrap_or_default());

        Some((seq, line))
    }

    pub fn rfq_count(&self) -> usize {
        self.orders.iter().filter(|o| o.is_rfq()).count()
    }

    pub fn order_count(&self) -> usize {
        self.orders
            .iter()
            .filter(|o| {
                matches!(
                    o.state,
                    PurchaseOrderState::Purchase | PurchaseOrderState::Done
                )
            })
            .count()
    }

    pub fn get(&self, reference: &str) -> Option<&PurchaseOrder> {
        self.orders.iter().find(|o| o.name == reference)
    }

    pub fn get_mut(&mut self, reference: &str) -> Option<&mut PurchaseOrder> {
        self.orders.iter_mut().find(|o| o.name == reference)
    }

    /// Open a new RFQ against a vendor and persist it immediately.
    ///
    /// Immediate persistence is the point: an RFQ on screen but not on disk is
    /// exactly the silent divergence this module exists to prevent.
    pub fn create_rfq(&mut self, manufacturer_id: &str) -> String {
        let reference = format_order_reference(self.next_sequence);
        self.next_sequence += 1;

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let order = PurchaseOrder::new_rfq(reference.clone(), manufacturer_id, today);

        self.orders.push(order);
        self.orders.sort_by(|a, b| a.name.cmp(&b.name));
        self.save(&reference);
        self.persist_sequence();
        reference
    }

    /// Write one order and all its lines back to disk.
    ///
    /// Line folders are rewritten wholesale rather than patched, because a line
    /// can be renamed, reordered or deleted, and a stale folder left behind
    /// would load as a phantom line the next time the Space opens.
    pub fn save(&self, reference: &str) {
        let Some(order) = self.get(reference) else {
            tracing::error!("PurchaseOrderRegistry::save: no such order {reference}");
            return;
        };
        let dir = self.root.join(ORDERS_DIR).join(reference);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!("PurchaseOrderRegistry: cannot create {}: {e}", dir.display());
            return;
        }
        self.ensure_orders_folder();

        let mut a = toml::value::Table::new();
        a.insert("reference".into(), order.name.clone().into());
        a.insert("manufacturer_id".into(), order.manufacturer_id.clone().into());
        a.insert("state".into(), state_to_wire(order.state).into());
        a.insert("date_order".into(), order.date_order.clone().into());
        a.insert("currency".into(), order.currency.clone().into());
        a.insert("has_invoices".into(), order.has_invoices.into());
        for (k, v) in [
            ("date_approve", &order.date_approve),
            ("date_planned", &order.date_planned),
            ("partner_ref", &order.partner_ref),
            ("origin", &order.origin),
            ("buyer", &order.buyer),
            ("notes", &order.notes),
        ] {
            if let Some(val) = v {
                a.insert(k.into(), val.clone().into());
            }
        }
        // Money is derived, never authored. Written so Properties can show it,
        // and recomputed from the lines on every save so it cannot drift.
        a.insert("amount_untaxed".into(), order.amount_untaxed().into());
        a.insert("amount_tax".into(), order.amount_tax().into());
        a.insert("amount_total".into(), order.amount_total().into());
        a.insert("line_count".into(), (order.order_line.len() as i64).into());

        if let Err(e) = std::fs::write(
            dir.join(INSTANCE),
            instance_doc("PurchaseOrder", &order.name, a),
        ) {
            tracing::error!("PurchaseOrderRegistry: cannot write {}: {e}", dir.display());
            return;
        }

        // Rewrite the line children.
        let keep: Vec<String> = order
            .order_line
            .iter()
            .enumerate()
            .map(|(idx, l)| line_folder(idx, &l.name))
            .collect();
        if let Ok(existing) = std::fs::read_dir(&dir) {
            for e in existing.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let nm = p.file_name().map(|n| n.to_string_lossy().to_string());
                    if nm.map(|n| !keep.contains(&n)).unwrap_or(false) {
                        let _ = std::fs::remove_dir_all(&p);
                    }
                }
            }
        }
        for (idx, line) in order.order_line.iter().enumerate() {
            let ldir = dir.join(&keep[idx]);
            if std::fs::create_dir_all(&ldir).is_err() {
                continue;
            }
            let mut la = toml::value::Table::new();
            la.insert("name".into(), line.name.clone().into());
            la.insert("sequence".into(), (line.sequence as i64).into());
            la.insert("product_qty".into(), line.product_qty.into());
            la.insert("product_uom".into(), line.product_uom.clone().into());
            la.insert("price_unit".into(), line.price_unit.into());
            la.insert("discount".into(), line.discount.into());
            la.insert("tax_percent".into(), line.tax_percent.into());
            la.insert("qty_received".into(), line.qty_received.into());
            la.insert("qty_invoiced".into(), line.qty_invoiced.into());
            la.insert("display_type".into(), display_to_wire(line.display_type).into());
            la.insert("price_subtotal".into(), line.price_subtotal().into());
            if let Some(ref p) = line.product_id {
                la.insert("product_id".into(), p.clone().into());
            }
            if let Some(ref d) = line.date_planned {
                la.insert("date_planned".into(), d.clone().into());
            }
            let _ = std::fs::write(
                ldir.join(INSTANCE),
                instance_doc("PurchaseOrderLine", &line.name, la),
            );
        }
    }

    /// Remove an order from the registry and from disk.
    pub fn remove(&mut self, reference: &str) -> bool {
        let Some(idx) = self.orders.iter().position(|o| o.name == reference) else {
            return false;
        };
        self.orders.remove(idx);
        let path = self.root.join(ORDERS_DIR).join(reference);
        if let Err(e) = std::fs::remove_dir_all(&path) {
            tracing::error!("PurchaseOrderRegistry: cannot delete {}: {e}", path.display());
        }
        true
    }

    /// Create the container Folder, on demand only.
    fn ensure_orders_folder(&self) {
        let dir = self.root.join(ORDERS_DIR);
        let inst = dir.join(INSTANCE);
        if inst.exists() || std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let mut a = toml::value::Table::new();
        a.insert("next_sequence".into(), (self.next_sequence as i64).into());
        let _ = std::fs::write(&inst, instance_doc("Folder", "PurchaseOrders", a));
    }

    fn persist_sequence(&self) {
        let dir = self.root.join(ORDERS_DIR);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let mut a = toml::value::Table::new();
        a.insert("next_sequence".into(), (self.next_sequence as i64).into());
        let _ = std::fs::write(dir.join(INSTANCE), instance_doc("Folder", "PurchaseOrders", a));
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Loads the open Space's purchase orders at startup.
pub struct PurchaseOrderPlugin;

impl Plugin for PurchaseOrderPlugin {
    fn build(&self, app: &mut App) {
        let root = crate::space::default_space_root();
        let registry = PurchaseOrderRegistry::load(&root);

        tracing::info!(
            "PurchaseOrderPlugin loaded: {} RFQs, {} orders, next reference {} (from {})",
            registry.rfq_count(),
            registry.order_count(),
            format_order_reference(registry.next_sequence),
            root.join(ORDERS_DIR).display(),
        );

        app.insert_resource(registry);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_space(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_po_ds_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_space_with_no_orders_creates_nothing() {
        let root = temp_space("lazy");
        let reg = PurchaseOrderRegistry::load(&root);
        assert!(reg.orders.is_empty());
        assert_eq!(reg.next_sequence, 1);
        // The whole point of on-demand: loading must not scaffold folders.
        assert!(
            !root.join(ORDERS_DIR).exists(),
            "loading an empty Space must not create DataService/PurchaseOrders"
        );
    }

    #[test]
    fn raising_an_rfq_creates_the_folders_and_survives_a_reload() {
        let root = temp_space("roundtrip");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let reference = reg.create_rfq("voltec-aln-thermal");
        assert_eq!(reference, "PO00001");
        assert!(root.join(ORDERS_DIR).join("PO00001").join(INSTANCE).exists());

        let reloaded = PurchaseOrderRegistry::load(&root);
        assert_eq!(reloaded.orders.len(), 1);
        let o = reloaded.get("PO00001").unwrap();
        assert_eq!(o.manufacturer_id, "voltec-aln-thermal");
        assert!(o.is_rfq());
    }

    #[test]
    fn lines_persist_as_child_instances_in_sequence_order() {
        let root = temp_space("lines");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let r = reg.create_rfq("v");
        {
            let o = reg.get_mut(&r).unwrap();
            let mut a = PurchaseOrderLine::new("Zeta part", 2.0, 10.0);
            a.sequence = 20;
            let mut b = PurchaseOrderLine::new("Alpha part", 3.0, 5.0);
            b.sequence = 10;
            o.order_line.push(a);
            o.order_line.push(b);
        }
        reg.save(&r);

        let reloaded = PurchaseOrderRegistry::load(&root);
        let o = reloaded.get(&r).unwrap();
        assert_eq!(o.order_line.len(), 2);
        // Ordered by `sequence`, not by folder name.
        assert_eq!(o.order_line[0].name, "Alpha part");
        assert_eq!(o.order_line[1].name, "Zeta part");
        assert_eq!(o.amount_untaxed(), 2.0 * 10.0 + 3.0 * 5.0);
    }

    #[test]
    fn a_removed_line_does_not_come_back_as_a_phantom() {
        let root = temp_space("phantom");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let r = reg.create_rfq("v");
        {
            let o = reg.get_mut(&r).unwrap();
            o.order_line.push(PurchaseOrderLine::new("Keep", 1.0, 1.0));
            o.order_line.push(PurchaseOrderLine::new("Drop", 1.0, 1.0));
        }
        reg.save(&r);
        {
            let o = reg.get_mut(&r).unwrap();
            o.order_line.retain(|l| l.name == "Keep");
        }
        reg.save(&r);

        let reloaded = PurchaseOrderRegistry::load(&root);
        assert_eq!(reloaded.get(&r).unwrap().order_line.len(), 1);
    }

    #[test]
    fn the_state_machine_survives_a_reload() {
        let root = temp_space("states");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let r = reg.create_rfq("v");
        {
            let o = reg.get_mut(&r).unwrap();
            o.order_line.push(PurchaseOrderLine::new("Bearing", 10.0, 4.5));
            o.print_quotation().unwrap();
            o.button_confirm(false).unwrap();
        }
        reg.save(&r);

        let reloaded = PurchaseOrderRegistry::load(&root);
        let o = reloaded.get(&r).unwrap();
        assert_eq!(o.state, PurchaseOrderState::Purchase);
        assert!(!o.is_rfq());
        assert_eq!(o.amount_untaxed(), 45.0);
    }

    #[test]
    fn section_lines_round_trip_on_their_odoo_wire_value() {
        let root = temp_space("section");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let r = reg.create_rfq("v");
        {
            let o = reg.get_mut(&r).unwrap();
            let mut sec = PurchaseOrderLine::new("QUALIFICATION LOT", 0.0, 0.0);
            sec.display_type = LineDisplayType::Section;
            o.order_line.push(sec);
            o.order_line.push(PurchaseOrderLine::new("Oxide", 50.0, 4100.0));
        }
        reg.save(&r);

        let reloaded = PurchaseOrderRegistry::load(&root);
        let o = reloaded.get(&r).unwrap();
        assert_eq!(o.order_line[0].display_type, LineDisplayType::Section);
        // A section carries no money.
        assert_eq!(o.amount_untaxed(), 50.0 * 4100.0);
    }

    #[test]
    fn a_lost_counter_cannot_mint_a_duplicate_reference() {
        let root = temp_space("counter");
        let mut reg = PurchaseOrderRegistry::load(&root);
        reg.create_rfq("a");
        reg.create_rfq("b");
        // Blow away the folder instance that carries `next_sequence`.
        let _ = std::fs::remove_file(root.join(ORDERS_DIR).join(INSTANCE));

        let mut recovered = PurchaseOrderRegistry::load(&root);
        assert_eq!(recovered.next_sequence, 3);
        assert_eq!(recovered.create_rfq("c"), "PO00003");
    }

    #[test]
    fn a_folder_of_the_wrong_class_is_not_loaded_as_an_order() {
        let root = temp_space("wrongclass");
        let mut reg = PurchaseOrderRegistry::load(&root);
        reg.create_rfq("a");
        let stray = root.join(ORDERS_DIR).join("NotAnOrder");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(
            stray.join(INSTANCE),
            instance_doc("Folder", "NotAnOrder", toml::value::Table::new()),
        )
        .unwrap();

        let reloaded = PurchaseOrderRegistry::load(&root);
        assert_eq!(reloaded.orders.len(), 1);
    }
}
