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
    /// Tessellation deviation tolerance in meters. `None` uses
    /// [`crate::eval::DEFAULT_MESH_TOLERANCE`]. Smaller = smoother
    /// curved surfaces, more triangles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_tolerance: Option<f64>,
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

    pub fn kind_label(&self) -> &'static str {
        match self {
            FeatureEntry::Sketch { .. } => "sketch",
            FeatureEntry::Feature { body, .. } => body.op_label(),
            FeatureEntry::Suppressed { .. } => "suppressed",
        }
    }
}

impl FeatureTree {
    /// Reorder entry at `from` to index `to` (clamped).
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() {
            return false;
        }
        let entry = self.entries.remove(from);
        let to = to.min(self.entries.len());
        self.entries.insert(to, entry);
        true
    }

    /// Suppress entry `index` (Feature/Sketch → Suppressed). Idempotent.
    pub fn suppress(&mut self, index: usize) -> Result<(), String> {
        let entry = self.entries.get(index).cloned().ok_or("index out of range")?;
        if entry.is_suppressed() {
            return Ok(());
        }
        let name = entry.name().to_string();
        // Store the full tagged entry so unsuppress is lossless.
        let body = toml::Value::try_from(entry).map_err(|e| format!("serialize: {e}"))?;
        self.entries[index] = FeatureEntry::Suppressed { name, body };
        Ok(())
    }

    /// Un-suppress: rehydrate from stored TOML value.
    pub fn unsuppress(&mut self, index: usize) -> Result<(), String> {
        let body = match self.entries.get(index) {
            Some(FeatureEntry::Suppressed { body, .. }) => body.clone(),
            Some(_) => return Ok(()),
            None => return Err("index out of range".into()),
        };
        let restored: FeatureEntry = body
            .try_into()
            .map_err(|e: toml::de::Error| format!("deserialize: {e}"))?;
        self.entries[index] = restored;
        Ok(())
    }

    /// Delete entry at index.
    pub fn delete(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        true
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
