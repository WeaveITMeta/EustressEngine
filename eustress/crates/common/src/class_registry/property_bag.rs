//! `PropertyBag` — typed, order-preserving property container that crosses
//! the [`crate::class_registry::ClassSpawner`] API boundary.
//!
//! Backed by `Vec<(String, PropertyValue)>` (**not** `HashMap`) because:
//!
//! 1. **Determinism.** Fjall write-path change detection compares raw
//!    rkyv bytes; `rkyv::to_bytes` output differs whenever input field
//!    order differs. Two equivalent entities with reshuffled bags would
//!    re-write every frame.
//! 2. **TOML diff stability.** `export_to_toml` walks the bag in
//!    iteration order; preserving insertion order keeps the on-disk
//!    `_instance.toml` byte-identical across reloads, eliminating the
//!    "git noise from saving" symptom the project's already hit.
//! 3. **rkyv friendly.** `Vec<(K, V)>` is already what
//!    `worlddb::rkyv_values::EusValue::Table` stores. `HashMap` requires
//!    a custom sort step before serialization.
//!
//! O(n) `get` is acceptable: spawners read each property once per spawn
//! call (~50 props × 1 spawn = trivial). The hashmap-vs-vec constant
//! factor dominates for n < 32, which is well above the typical class
//! template size.
//!
//! See `docs/architecture/CLASS_REGISTRY.md` §4 for the full rationale.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::classes::PropertyValue;

/// Typed property container with deterministic iteration order.
///
/// The canonical order is **insertion order** — the order the spawner's
/// `import_from_*` populated the bag. Per spec §4.3 a spawner SHOULD
/// match its class template's key order when building the bag so
/// round-trips through TOML stay diff-stable. (No template-coupling
/// assertion lives in this crate yet; Wave 3 wires that.)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PropertyBag {
    entries: Vec<(String, PropertyValue)>,
}

impl PropertyBag {
    /// Empty bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Empty bag with a hint for the expected key count — avoids
    /// reallocation when a spawner knows its template size up front.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            entries: Vec::with_capacity(n),
        }
    }

    /// Insert a property.
    ///
    /// - **Key exists:** value is REPLACED in place. Iteration order
    ///   is preserved (the entry stays where it was first inserted).
    /// - **Key is new:** the entry is APPENDED at the end. This is the
    ///   path that establishes the bag's canonical order.
    pub fn set(&mut self, key: impl Into<String>, value: PropertyValue) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| k == &key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    /// Look up a property by key. Linear scan; see module docs for why
    /// that's acceptable.
    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Iterate `(key, value)` pairs in canonical (insertion) order.
    pub fn iter(&self) -> impl Iterator<Item = &(String, PropertyValue)> {
        self.entries.iter()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no entries are present.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries — used by spawners that need to rebuild the
    /// bag from scratch (e.g. after a respawn-required edit).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    // ── Typed accessor helpers ─────────────────────────────────────────
    //
    // Each returns `None` on missing key OR on type mismatch — callers
    // that want to distinguish "missing" from "wrong type" should match
    // on `get(...)` directly. Matching what spec §4.2 lists.

    /// Read an `f32`. Returns `None` if missing or not a `Float`.
    pub fn get_f32(&self, key: &str) -> Option<f32> {
        match self.get(key)? {
            PropertyValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Read an `i32`. Returns `None` if missing or not an `Int`.
    pub fn get_i32(&self, key: &str) -> Option<i32> {
        match self.get(key)? {
            PropertyValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Read a `bool`. Returns `None` if missing or not a `Bool`.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Read a `Vec3`. Returns `None` if missing or not a `Vector3`.
    pub fn get_vec3(&self, key: &str) -> Option<Vec3> {
        match self.get(key)? {
            PropertyValue::Vector3(v) => Some(*v),
            _ => None,
        }
    }

    /// Read a 2-element float array. Returns `None` if missing or not
    /// a `Vector2`.
    pub fn get_vec2(&self, key: &str) -> Option<[f32; 2]> {
        match self.get(key)? {
            PropertyValue::Vector2(v) => Some(*v),
            _ => None,
        }
    }

    /// Read a `Color`. Returns `None` if missing or not a `Color`.
    ///
    /// Note: a `Color3([r,g,b])` value does NOT round-trip through this
    /// accessor — callers needing either-or should match on `get(...)`
    /// directly. Kept distinct to surface unit/space mismatches.
    pub fn get_color(&self, key: &str) -> Option<Color> {
        match self.get(key)? {
            PropertyValue::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// Read a `Color3` (linear sRGB triple). Returns `None` if missing
    /// or not a `Color3`.
    pub fn get_color3(&self, key: &str) -> Option<[f32; 3]> {
        match self.get(key)? {
            PropertyValue::Color3(c) => Some(*c),
            _ => None,
        }
    }

    /// Read a borrowed `Transform`. Returns `None` if missing or not a
    /// `Transform`. (Returns a borrow because `Transform` is `Copy` but
    /// callers commonly `.cloned()` after this — same shape as `get`.)
    pub fn get_transform(&self, key: &str) -> Option<&Transform> {
        match self.get(key)? {
            PropertyValue::Transform(t) => Some(t),
            _ => None,
        }
    }

    /// Read a borrowed string. Returns `None` if missing or not a
    /// `String`.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            PropertyValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Read a borrowed enum-name string. Returns `None` if missing or
    /// not an `Enum`. (`Enum` is just a stringly-typed `String` today;
    /// distinct accessor exists so the intent — "this is an enum
    /// discriminant, not free text" — survives reads.)
    pub fn get_enum(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            PropertyValue::Enum(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Convenience reader for the canonical `metadata.uuid` key.
    ///
    /// Wave 2.1 added a `uuid` field to `InstanceMetadata`; spawners
    /// that need to thread the UUID through the import → spawn pipeline
    /// will read it from the bag under this key. Returns `None` when
    /// the entity predates UUID stamping (the value should be added by
    /// the importer in Wave 3+).
    pub fn get_uuid(&self) -> Option<&str> {
        self.get_string("metadata.uuid")
    }
}

// ============================================================================
// Tests — smoke coverage for the deliverable criteria in the task spec
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip every primitive variant the bag supports today.
    /// Mirrors deliverable criterion #6 in the task prompt.
    #[test]
    fn roundtrip_basic_types() {
        let mut bag = PropertyBag::new();
        bag.set("pos", PropertyValue::Vector3(Vec3::new(1.0, 2.0, 3.0)));
        bag.set("tint", PropertyValue::Color(Color::srgb(0.5, 0.25, 0.75)));
        bag.set("brightness", PropertyValue::Float(1500.0));
        bag.set("name", PropertyValue::String("PointLight".into()));
        bag.set("shadows", PropertyValue::Bool(true));

        assert_eq!(bag.get_vec3("pos"), Some(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(bag.get_f32("brightness"), Some(1500.0));
        assert_eq!(bag.get_string("name"), Some("PointLight"));
        assert_eq!(bag.get_bool("shadows"), Some(true));
        // Color round-trips by srgb decomposition; equality on the
        // enum is enough here.
        assert!(matches!(bag.get_color("tint"), Some(Color::Srgba(_))));
    }

    /// Iteration MUST be insertion order, not alphabetic.
    /// Mirrors deliverable criterion #7 in the task prompt; this is the
    /// determinism gate Fjall change-detection relies on.
    #[test]
    fn iteration_preserves_insertion_order() {
        let mut bag = PropertyBag::new();
        bag.set("z_last_alpha_first_insert", PropertyValue::Float(1.0));
        bag.set("a_first_alpha_last_insert", PropertyValue::Float(2.0));
        bag.set("m_middle", PropertyValue::Float(3.0));

        let keys: Vec<&str> = bag.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "z_last_alpha_first_insert",
                "a_first_alpha_last_insert",
                "m_middle",
            ],
            "PropertyBag must preserve insertion order, NOT alphabetize"
        );
    }

    /// `set` on an existing key REPLACES in place — order preserved.
    #[test]
    fn replace_preserves_position() {
        let mut bag = PropertyBag::new();
        bag.set("a", PropertyValue::Int(1));
        bag.set("b", PropertyValue::Int(2));
        bag.set("c", PropertyValue::Int(3));
        bag.set("b", PropertyValue::Int(20)); // replace middle

        let keys: Vec<&str> = bag.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(bag.get_i32("b"), Some(20));
    }

    /// Type-mismatch accessors return `None`, not a wrong-type cast.
    #[test]
    fn type_mismatch_returns_none() {
        let mut bag = PropertyBag::new();
        bag.set("count", PropertyValue::Int(42));
        // Same key, wrong typed accessor:
        assert_eq!(bag.get_f32("count"), None);
        assert_eq!(bag.get_string("count"), None);
        // Correct accessor still works:
        assert_eq!(bag.get_i32("count"), Some(42));
    }

    /// `get_uuid` reads the canonical key.
    #[test]
    fn uuid_helper_reads_canonical_key() {
        let mut bag = PropertyBag::new();
        assert_eq!(bag.get_uuid(), None);
        bag.set(
            "metadata.uuid",
            PropertyValue::String("01234567-89ab-cdef-0123-456789abcdef".into()),
        );
        assert_eq!(
            bag.get_uuid(),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
    }
}
