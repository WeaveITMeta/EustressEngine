//! # Purchase order persistence, numbering, and registry
//!
//! [`super::purchase`] is the pure domain model: the types, the Odoo state
//! machine, and the money, with no I/O and no Bevy in it. That separation is
//! deliberate and worth keeping, because it lets the whole state machine be
//! tested in seconds without building the engine. This module is the other
//! half: where orders live on disk, how they get their numbers, and how the
//! rest of the engine reaches them.
//!
//! ## Where orders live
//!
//! Space-local, under `<space>/Manufacturing/PurchaseOrders/`, one TOML file
//! per order named after its reference, so `PO00001.toml`.
//!
//! The [`super::Manufacturer`] and [`super::Investor`] registries are global,
//! under `docs/manufacturing/`, and orders deliberately are not. Those
//! registries are reference data shared by every project; an order is the
//! working record of one project's procurement. Two Spaces must be able to hold
//! different orders against the same manufacturer, which a single global
//! directory could not express.
//!
//! ## Numbering
//!
//! Odoo allocates references from `ir.sequence`. There is no sequence table
//! here, so the next counter is the greater of a persisted counter file and one
//! past the highest reference actually present on disk. Taking the max of both
//! is what makes a duplicate reference unmintable: if the counter file is lost,
//! stale, or hand-edited, the files themselves still answer the question.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::purchase::{format_order_reference, PurchaseOrder, PurchaseOrderState};

/// Directory holding one TOML per order, relative to the Space root.
pub const ORDERS_DIR: &str = "Manufacturing/PurchaseOrders";

/// Filename of the persisted sequence counter, kept inside [`ORDERS_DIR`].
const SEQUENCE_FILE: &str = "_sequence.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
struct SequenceFile {
    next: u32,
}

// ============================================================================
// Registry
// ============================================================================

/// Every purchase order and RFQ in the open Space, plus the counter that names
/// the next one.
///
/// RFQs and purchase orders are the same records here, exactly as they are in
/// Odoo. Ask [`PurchaseOrder::is_rfq`] which face a given record is showing;
/// there is no separate RFQ collection that could fall out of step.
#[derive(Debug, Default, Resource)]
pub struct PurchaseOrderRegistry {
    /// All orders, RFQs included, sorted by reference.
    pub orders: Vec<PurchaseOrder>,
    /// Counter for the next reference to mint.
    next_sequence: u32,
    /// Space root this registry was loaded from, and saves back to.
    root: PathBuf,
}

impl PurchaseOrderRegistry {
    /// Load every order in a Space. A missing directory is not an error: it is
    /// simply a Space that has not raised an RFQ yet.
    pub fn load(space_root: &Path) -> Self {
        let dir = space_root.join(ORDERS_DIR);
        let mut orders: Vec<PurchaseOrder> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|x| x != "toml").unwrap_or(true) {
                    continue;
                }
                // The counter is not an order; skip it by name.
                if path.file_name().map(|n| n == SEQUENCE_FILE).unwrap_or(false) {
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(text) => match toml::from_str::<PurchaseOrder>(&text) {
                        Ok(order) => orders.push(order),
                        // Loud, not silent. A file that fails to parse is an
                        // order the user believes exists, and quietly dropping
                        // it is the worst outcome available for a record that
                        // may represent committed money.
                        Err(e) => tracing::error!(
                            "PurchaseOrderRegistry: {} failed to parse and was NOT loaded: {e}",
                            path.display()
                        ),
                    },
                    Err(e) => {
                        tracing::error!("PurchaseOrderRegistry: cannot read {}: {e}", path.display())
                    }
                }
            }
        }

        orders.sort_by(|a, b| a.name.cmp(&b.name));

        let persisted = std::fs::read_to_string(dir.join(SEQUENCE_FILE))
            .ok()
            .and_then(|t| toml::from_str::<SequenceFile>(&t).ok())
            .map(|s| s.next)
            .unwrap_or(1);

        // One past the highest reference actually on disk. This is the half
        // that makes a lost counter file harmless.
        let observed = orders
            .iter()
            .filter_map(|o| o.name.strip_prefix("PO"))
            .filter_map(|digits| digits.parse::<u32>().ok())
            .max()
            .map(|hi| hi + 1)
            .unwrap_or(1);

        Self {
            orders,
            next_sequence: persisted.max(observed),
            root: space_root.to_path_buf(),
        }
    }

    /// How many records are currently RFQs rather than committed orders.
    pub fn rfq_count(&self) -> usize {
        self.orders.iter().filter(|o| o.is_rfq()).count()
    }

    /// How many have been confirmed into real purchase orders.
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

    /// Open a new RFQ against a manufacturer and persist it immediately.
    ///
    /// Returns the reference it was given. Immediate persistence is the point:
    /// an RFQ that exists on screen but not on disk is exactly the silent
    /// divergence this module exists to prevent.
    pub fn create_rfq(&mut self, manufacturer_id: &str) -> String {
        let reference = format_order_reference(self.next_sequence);
        self.next_sequence += 1;

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let order = PurchaseOrder::new_rfq(reference.clone(), manufacturer_id, today);

        self.orders.push(order);
        self.orders.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_sequence();
        self.save(&reference);
        reference
    }

    /// Write one order back to disk. Call after any mutation.
    pub fn save(&self, reference: &str) {
        let Some(order) = self.get(reference) else {
            tracing::error!("PurchaseOrderRegistry::save: no such order {reference}");
            return;
        };
        let dir = self.root.join(ORDERS_DIR);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!("PurchaseOrderRegistry: cannot create {}: {e}", dir.display());
            return;
        }
        let path = dir.join(format!("{reference}.toml"));
        match toml::to_string_pretty(order) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    tracing::error!("PurchaseOrderRegistry: cannot write {}: {e}", path.display());
                }
            }
            Err(e) => tracing::error!("PurchaseOrderRegistry: cannot serialise {reference}: {e}"),
        }
    }

    /// Remove an order from the registry and from disk.
    pub fn remove(&mut self, reference: &str) -> bool {
        let Some(idx) = self.orders.iter().position(|o| o.name == reference) else {
            return false;
        };
        self.orders.remove(idx);
        let path = self.root.join(ORDERS_DIR).join(format!("{reference}.toml"));
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::error!("PurchaseOrderRegistry: cannot delete {}: {e}", path.display());
        }
        true
    }

    fn persist_sequence(&self) {
        let dir = self.root.join(ORDERS_DIR);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let file = SequenceFile {
            next: self.next_sequence,
        };
        if let Ok(text) = toml::to_string_pretty(&file) {
            let _ = std::fs::write(dir.join(SEQUENCE_FILE), text);
        }
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
            "PurchaseOrderPlugin loaded: {} RFQs, {} orders, next reference {}",
            registry.rfq_count(),
            registry.order_count(),
            format_order_reference(registry.next_sequence),
        );

        app.insert_resource(registry);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::purchase::PurchaseOrderLine;
    use super::*;

    fn temp_space(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_po_test_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(ORDERS_DIR)).unwrap();
        dir
    }

    #[test]
    fn an_empty_space_starts_numbering_at_one() {
        let root = temp_space("empty");
        let reg = PurchaseOrderRegistry::load(&root);
        assert!(reg.orders.is_empty());
        assert_eq!(reg.next_sequence, 1);
    }

    #[test]
    fn a_created_rfq_survives_a_reload() {
        let root = temp_space("roundtrip");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let reference = reg.create_rfq("mfg-acme");
        assert_eq!(reference, "PO00001");

        // Reload from disk exactly as a restart would.
        let reloaded = PurchaseOrderRegistry::load(&root);
        assert_eq!(reloaded.orders.len(), 1);
        let order = reloaded.get("PO00001").expect("order should persist");
        assert_eq!(order.manufacturer_id, "mfg-acme");
        assert!(order.is_rfq(), "a fresh order is an RFQ");
    }

    #[test]
    fn a_lost_counter_file_cannot_mint_a_duplicate_reference() {
        let root = temp_space("lostcounter");
        let mut reg = PurchaseOrderRegistry::load(&root);
        reg.create_rfq("mfg-a");
        reg.create_rfq("mfg-b");

        // Simulate the counter file being lost, stale, or never written.
        std::fs::remove_file(root.join(ORDERS_DIR).join(SEQUENCE_FILE)).unwrap();

        let mut recovered = PurchaseOrderRegistry::load(&root);
        assert_eq!(
            recovered.next_sequence, 3,
            "next reference must come from the files when the counter is gone"
        );
        let third = recovered.create_rfq("mfg-c");
        assert_eq!(third, "PO00003", "must not collide with an existing order");
    }

    #[test]
    fn the_full_state_machine_persists_through_a_reload() {
        let root = temp_space("statemachine");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let reference = reg.create_rfq("mfg-acme");

        {
            let order = reg.get_mut(&reference).unwrap();
            order
                .order_line
                .push(PurchaseOrderLine::new("Bearing", 10.0, 4.5));
            order.print_quotation().unwrap();
            order.button_confirm(false).unwrap();
        }
        reg.save(&reference);

        let reloaded = PurchaseOrderRegistry::load(&root);
        let order = reloaded.get(&reference).unwrap();
        assert_eq!(order.state, PurchaseOrderState::Purchase);
        assert!(!order.is_rfq(), "a confirmed order is no longer an RFQ");
        assert_eq!(order.order_line.len(), 1);
        assert_eq!(order.amount_untaxed(), 45.0);
    }

    #[test]
    fn counts_split_rfqs_from_committed_orders() {
        let root = temp_space("counts");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let a = reg.create_rfq("mfg-a");
        let b = reg.create_rfq("mfg-b");
        reg.create_rfq("mfg-c");

        for r in [a, b] {
            let order = reg.get_mut(&r).unwrap();
            order.order_line.push(PurchaseOrderLine::new("Part", 1.0, 1.0));
            order.button_confirm(false).unwrap();
        }

        assert_eq!(reg.rfq_count(), 1);
        assert_eq!(reg.order_count(), 2);
    }

    #[test]
    fn an_unparseable_file_is_reported_and_skipped_not_silently_dropped() {
        let root = temp_space("corrupt");
        let mut reg = PurchaseOrderRegistry::load(&root);
        reg.create_rfq("mfg-good");
        std::fs::write(
            root.join(ORDERS_DIR).join("PO09999.toml"),
            "not valid toml {{{",
        )
        .unwrap();

        let reloaded = PurchaseOrderRegistry::load(&root);
        assert_eq!(reloaded.orders.len(), 1, "the good order still loads");
        assert!(reloaded.get("PO00001").is_some());
    }

    #[test]
    fn removing_an_order_deletes_its_file() {
        let root = temp_space("remove");
        let mut reg = PurchaseOrderRegistry::load(&root);
        let reference = reg.create_rfq("mfg-a");
        assert!(reg.remove(&reference));

        let reloaded = PurchaseOrderRegistry::load(&root);
        assert!(reloaded.orders.is_empty());
    }
}
