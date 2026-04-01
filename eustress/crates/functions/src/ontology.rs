//! # Stage 4: Ontological Genius — Classification & Hierarchy
//!
//! - `classify(class_name)` — Look up ontology path
//! - `relate(from, to, predicate)` — Define a relationship (deferred)
//! - `engineer(domain_name)` — Create a new ontology domain (deferred)

use std::cell::RefCell;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};
use eustress_embedvec::{OntologyTree, OntologyIndex};

// ============================================================================
// Result Types
// ============================================================================

#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct OntologyPath {
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub path: String,
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub class_name: String,
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub depth: i64,
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub has_children: bool,
}

// ============================================================================
// Deferred Commands
// ============================================================================

#[derive(Debug, Clone)]
pub struct RelationCommand {
    pub from: String,
    pub to: String,
    pub predicate: String,
}

// ============================================================================
// Bridge
// ============================================================================

pub struct OntologyBridge {
    pub tree: Arc<RwLock<OntologyTree>>,
    pub index: Option<Arc<RwLock<OntologyIndex>>>,
    pub pending_relations: Vec<RelationCommand>,
    pub pending_domains: Vec<String>,
}

impl OntologyBridge {
    pub fn new(tree: Arc<RwLock<OntologyTree>>) -> Self {
        Self { tree, index: None, pending_relations: Vec::new(), pending_domains: Vec::new() }
    }
    pub fn with_index(mut self, index: Arc<RwLock<OntologyIndex>>) -> Self {
        self.index = Some(index);
        self
    }
    pub fn drain(&mut self) -> (Vec<RelationCommand>, Vec<String>) {
        (std::mem::take(&mut self.pending_relations), std::mem::take(&mut self.pending_domains))
    }
}

thread_local! {
    static ONTOLOGY_BRIDGE: RefCell<Option<OntologyBridge>> = RefCell::new(None);
}

pub fn set_ontology_bridge(bridge: OntologyBridge) {
    ONTOLOGY_BRIDGE.with(|cell| { *cell.borrow_mut() = Some(bridge); });
}

pub fn take_ontology_bridge() -> Option<OntologyBridge> {
    ONTOLOGY_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_ontology_bridge<F, R>(fallback: R, f: F) -> R
where F: FnOnce(&OntologyBridge) -> R {
    ONTOLOGY_BRIDGE.with(|cell| {
        let b = cell.borrow();
        match b.as_ref() {
            Some(bridge) => f(bridge),
            None => { warn!("[Ontology] Bridge not available"); fallback }
        }
    })
}

fn with_ontology_bridge_mut<F, R>(fallback: R, f: F) -> R
where F: FnOnce(&mut OntologyBridge) -> R {
    ONTOLOGY_BRIDGE.with(|cell| {
        let mut b = cell.borrow_mut();
        match b.as_mut() {
            Some(bridge) => f(bridge),
            None => { warn!("[Ontology] Bridge not available"); fallback }
        }
    })
}

// ============================================================================
// Rune Functions
// ============================================================================

fn empty_path(class_name: &str) -> OntologyPath {
    OntologyPath {
        path: String::new(),
        class_name: class_name.to_string(),
        depth: -1,
        has_children: false,
    }
}

/// Look up a class in the ontology tree.
///
/// Uses `OntologyTree::get_by_name()` to find the node, then
/// `OntologyTree::path_for()` to resolve its full hierarchical path.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn classify(class_name: &str) -> OntologyPath {
    with_ontology_bridge(empty_path(class_name), |bridge| {
        let tree = match bridge.tree.read() {
            Ok(g) => g,
            Err(_) => return empty_path(class_name),
        };
        if let Some(node) = tree.get_by_name(class_name) {
            // Resolve full path via the tree's path index
            let path = tree.path_for(node.id).unwrap_or_else(|| node.name.clone());
            let depth = path.matches('/').count() as i64;
            info!("[Ontology] classify({}) → {}", class_name, path);
            OntologyPath {
                path,
                class_name: class_name.to_string(),
                depth,
                has_children: !node.children.is_empty(),
            }
        } else {
            warn!("[Ontology] classify({}) → not found", class_name);
            empty_path(class_name)
        }
    })
}

/// Define a relationship between two entities (deferred).
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn relate(from: &str, to: &str, predicate: &str) {
    with_ontology_bridge_mut((), |bridge| {
        bridge.pending_relations.push(RelationCommand {
            from: from.to_string(),
            to: to.to_string(),
            predicate: predicate.to_string(),
        });
        info!("[Ontology] relate({}, {}, {})", from, to, predicate);
    });
}

/// Create a new ontology domain branch (deferred).
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn engineer(domain_name: &str) {
    with_ontology_bridge_mut((), |bridge| {
        bridge.pending_domains.push(domain_name.to_string());
        info!("[Ontology] engineer({})", domain_name);
    });
}

// ============================================================================
// Module Registration
// ============================================================================

#[cfg(feature = "rune-dsl")]
pub fn create_ontology_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "ontology"])?;
    module.ty::<OntologyPath>()?;
    module.function_meta(classify)?;
    module.function_meta(relate)?;
    module.function_meta(engineer)?;
    Ok(module)
}
