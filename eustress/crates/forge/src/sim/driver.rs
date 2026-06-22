//! Per-cell reconcile driver — pure async helpers the ENGINE plugin calls
//! off-thread. This crate stays Bevy-free; the engine owns the Bevy systems,
//! the per-cell launch latch, and the Phase-0b
//! OBSERVE→infer→RSI→Kernel-validate→ACT→persist loop. Everything here is the
//! pure forge calls so the seam is testable without Bevy.

use super::{
    store_set_json, BoxedStateStore, NodeResources, ReconcileReport, Reconciler, SimBinding,
    StateStore,
};
use forge_orchestration::autoscaler::{Autoscaler, AutoscalerConfig};
use std::sync::Arc;

use crate::error::{EustressForgeError, Result};

fn map_forge_err<E: Into<forge_orchestration::ForgeError>>(e: E) -> EustressForgeError {
    EustressForgeError::Orchestration(e.into())
}

/// Run ONE reconcile pass and return the report plus the fresh bindings.
///
/// Verified 0.6.0: `Reconciler::reconcile_once(&mut self) -> Result<ReconcileReport>`
/// auto-discovers desired cells by scanning `keys::SIMCELLS`; `sim_bindings()`
/// returns the committed gang bindings. Register nodes (idempotently) and
/// submit SimCells BEFORE calling this.
pub async fn reconcile_once(rec: &mut Reconciler) -> Result<(ReconcileReport, Vec<SimBinding>)> {
    let report = rec.reconcile_once().await.map_err(map_forge_err)?;
    Ok((report, rec.sim_bindings()))
}

/// Build a single-node [`Reconciler`] for the Matrix milestone: wrap a default
/// [`Autoscaler`] over the given store and register one node.
///
/// Verified 0.6.0: `Reconciler::new(store: BoxedStateStore, autoscaler: Arc<Autoscaler>)`;
/// `Autoscaler::new(AutoscalerConfig) -> Result<Self>`;
/// `Reconciler::register_node(&mut self, NodeResources)`.
pub fn single_node_reconciler(store: BoxedStateStore, node: NodeResources) -> Result<Reconciler> {
    let autoscaler = Autoscaler::new(AutoscalerConfig::default()).map_err(map_forge_err)?;
    let mut rec = Reconciler::new(store, Arc::new(autoscaler));
    rec.register_node(node);
    Ok(rec)
}

/// All-or-nothing check: a committed [`SimBinding`] must place EVERY expected
/// member. The gang scheduler only emits a binding when it committed, so the
/// binding existing + `placements.len() == expected_members` proves the whole
/// gang (world + N agents) landed together.
pub fn binding_is_complete(b: &SimBinding, expected_members: usize) -> bool {
    b.placements.len() == expected_members
}

/// Find the binding for a given cell id among a batch of bindings.
pub fn find_binding<'a>(bindings: &'a [SimBinding], cell_id: &str) -> Option<&'a SimBinding> {
    bindings.iter().find(|b| b.cell_id == cell_id)
}

/// Convenience: submit a SimCell's JSON under its canonical key via an owned
/// store handle (the demo + engine driver both have a `BoxedStateStore`).
/// Mirrors [`super::cell_sync::submit_sim_cell`] but takes the `Arc` form.
pub async fn submit_sim_cell_arc(store: &BoxedStateStore, cell: &super::SimCell) -> Result<()> {
    store_set_json(
        store.as_ref() as &dyn StateStore,
        &forge_orchestration::storage::keys::simcell(&cell.id),
        cell,
    )
    .await
    .map_err(map_forge_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::NodeId;

    fn binding(cell_id: &str, n: usize) -> SimBinding {
        SimBinding {
            cell_id: cell_id.to_string(),
            placements: (0..n).map(|i| (format!("m{i}"), NodeId::new())).collect(),
            reservations: Vec::new(),
        }
    }

    #[test]
    fn binding_is_complete_checks_all_members() {
        let b = binding("sim-cell-0000001-0000002-0000003", 3);
        assert!(binding_is_complete(&b, 3));
        assert!(!binding_is_complete(&b, 4)); // a member missing => gang not whole
        assert!(!binding_is_complete(&b, 2));
    }

    #[test]
    fn find_binding_matches_cell_id() {
        let bs = vec![binding("sim-cell-0000001-0000000-0000000", 1), binding("sim-cell-0000002-0000000-0000000", 2)];
        assert!(find_binding(&bs, "sim-cell-0000002-0000000-0000000").is_some());
        assert!(find_binding(&bs, "sim-cell-9999999-0000000-0000000").is_none());
    }
}
