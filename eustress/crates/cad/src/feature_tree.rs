//! The ordered feature tree: sketches + features in evaluation order.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::{Feature, Sketch, Quantity};

/// Top-level on-disk shape of a part's `features.toml`.
///
/// ```toml
/// [variables]
/// length   = "50 mm"
/// width    = "30 mm"
/// thickness = "2 mm"
///
/// [[entry]]
/// name = "Sketch1"
/// kind = "sketch"
/// plane = "xy"
/// # ... sketch body ...
///
/// [[entry]]
/// name = "Extrude1"
/// kind = "feature"
/// # ... feature body ...
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureTree {
    /// Named parameters referenceable from dimensions + feature inputs.
    /// Values are `Quantity`s; expressions (`length * 0.6`) are
    /// resolved by the evaluator.
    #[serde(default)]
    pub variables: HashMap<String, String>, // raw expression strings

    /// Ordered list of sketches + features. Evaluation walks in
    /// declaration order.
    #[serde(default, rename = "entry")]
    pub entries: Vec<FeatureEntry>,

    /// Optional metadata — author, created, last_modified etc. Kept
    /// parallel to the InstanceDefinition metadata used elsewhere.
    #[serde(default)]
    pub metadata: TreeMetadata,
}

impl Default for FeatureTree {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            entries: Vec::new(),
            metadata: TreeMetadata::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeMetadata {
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_modified: String,
    #[serde(default)]
    pub author: String,
}

/// One tree entry — either a Sketch or a Feature. Tagged by `kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FeatureEntry {
    Sketch {
        name: String,
        #[serde(flatten)]
        body: Sketch,
    },
    Feature {
        name: String,
        #[serde(flatten)]
        body: Feature,
    },
    /// Suppressed — evaluator skips. Kept for rollback-bar UX where
    /// the user wants to preview without the feature.
    Suppressed {
        name: String,
        /// The serialized body, preserved so re-enabling is lossless.
        #[serde(flatten)]
        body: toml::Value,
    },
}

impl FeatureEntry {
    pub fn name(&self) -> &str {
        match self {
            FeatureEntry::Sketch { name, .. }     => name,
            FeatureEntry::Feature { name, .. }    => name,
            FeatureEntry::Suppressed { name, .. } => name,
        }
    }

    pub fn is_suppressed(&self) -> bool {
        matches!(self, FeatureEntry::Suppressed { .. })
    }
}

/// Resolve a variable reference or literal quantity string to a
/// concrete `Quantity`. Variables can reference other variables;
/// depth-limited to 32 to catch loops.
pub fn resolve_quantity(s: &str, vars: &HashMap<String, String>) -> Option<Quantity> {
    resolve_quantity_depth(s, vars, 0)
}

fn resolve_quantity_depth(s: &str, vars: &HashMap<String, String>, depth: u8) -> Option<Quantity> {
    if depth > 32 { return None; }
    // First try: direct parse ("50 mm").
    if let Some(q) = Quantity::parse(s) { return Some(q); }
    // Next: variable lookup.
    if let Some(var_expr) = vars.get(s) {
        return resolve_quantity_depth(var_expr, vars, depth + 1);
    }
    None
}
