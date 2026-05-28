//! LOD (level-of-detail) tier model exposed to spawners.
//!
//! A spawner's `lod_components(tier)` returns a [`ComponentBundle`] that
//! describes which components to insert and which to remove when the
//! entity enters the given tier. The tier-selection RULES (which entity
//! is at which tier each frame) live in `RENDER_CASCADE.md`; this module
//! only defines the per-class CONTRACT.
//!
//! See `docs/architecture/CLASS_REGISTRY.md` §9 for the full design.
//!
//! ## Wave 2 vs Wave 3 split
//!
//! Wave 2 (this task) defines the types only — no transition system
//! consumes them yet. Wave 3 ships the per-class bundles inside each
//! `ClassSpawner` impl AND the `apply_lod_transitions` system that
//! drains them. Both follow `RENDER_CASCADE.md`'s LOOP-3 breaker:
//! Wave 2/3 LOD touches VISUAL components only — colliders + rigid
//! bodies are physics-LOD territory and Wave 4 owns them.

use std::any::TypeId;

use bevy::reflect::Reflect;

/// Four-tier LOD model. Order matters: tiers are conceptually a ladder
/// from "fully rendered" (Hero) to "barely there" (Horizon).
///
/// See `RENDER_CASCADE.md` for the per-tier distance thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LodTier {
    /// In-frustum, focal point of attention. Full PBR, full shadows,
    /// full collider, full UI rendering. Currently the only tier the
    /// engine has — every part gets this today (pre-Wave 3).
    Hero,

    /// Visible at mid-range. Shadow casting off (`NotShadowCaster`),
    /// full material, full collider. Visibility range matches
    /// `Workspace.render_distance`.
    Active,

    /// Streamed but not actively rendered every frame.
    /// `VisibilityRange` distance cull active; collider may be
    /// downgraded to AABB only (Wave 4); material may swap to an unlit
    /// variant.
    Streamed,

    /// Beyond active range; represented by a single billboarded
    /// impostor or omitted entirely. Sound is muted; particles paused;
    /// scripts suspended via `RunService.PreRender` check.
    Horizon,
}

impl LodTier {
    /// Stable string discriminant — used by logs and the future
    /// Properties panel readout.
    pub fn as_str(&self) -> &'static str {
        match self {
            LodTier::Hero => "Hero",
            LodTier::Active => "Active",
            LodTier::Streamed => "Streamed",
            LodTier::Horizon => "Horizon",
        }
    }
}

/// What a spawner's `lod_components` returns. Empty bundles are valid —
/// they signal "no LOD-tier-specific components for this class" (e.g.
/// `Folder` has no LOD model at all and returns `ComponentBundle::default()`
/// for every tier).
pub struct ComponentBundle {
    /// Components to insert when this tier is entered. Boxed `Reflect`
    /// trait objects so the bundle can travel through the object-safe
    /// trait API; the transition system unwraps via the reflection-based
    /// insert path.
    pub insert: Vec<DynamicComponent>,

    /// Component `TypeId`s to remove when this tier is entered. The
    /// transition system calls `entity.remove_by_id(type_id)` for each.
    pub remove: Vec<TypeId>,
}

impl Default for ComponentBundle {
    fn default() -> Self {
        Self {
            insert: Vec::new(),
            remove: Vec::new(),
        }
    }
}

impl ComponentBundle {
    /// Empty bundle — no insertions, no removals. The "do nothing" tier
    /// transition (`Folder` returns this for every tier).
    pub fn empty() -> Self {
        Self::default()
    }

    /// True when neither insert nor remove carries any work — the
    /// transition system can short-circuit.
    pub fn is_empty(&self) -> bool {
        self.insert.is_empty() && self.remove.is_empty()
    }
}

/// Type-erased "spawn this component" payload.
///
/// Boxed because the bundle must be returned through the object-safe
/// `ClassSpawner` trait API. Wave 3's transition system calls
/// `entity.insert_reflect(boxed_component.0)` at apply time.
///
/// Per spec §9.1: this is intentionally a thin wrapper around
/// `Box<dyn Reflect>` — no `Send + Sync` is required at the wrapper
/// level because `Reflect` implementations the engine cares about are
/// all `Send + Sync` themselves. (If a future component is genuinely
/// `!Send`, it can't be returned from a `Send + Sync` spawner anyway —
/// see CLASS_REGISTRY.md §12 Q3.)
pub struct DynamicComponent(pub Box<dyn Reflect>);

impl DynamicComponent {
    /// Wrap any `Reflect`-implementing component for the bundle.
    pub fn new<T: Reflect>(component: T) -> Self {
        Self(Box::new(component))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_tier_as_str_round_trip() {
        assert_eq!(LodTier::Hero.as_str(), "Hero");
        assert_eq!(LodTier::Active.as_str(), "Active");
        assert_eq!(LodTier::Streamed.as_str(), "Streamed");
        assert_eq!(LodTier::Horizon.as_str(), "Horizon");
    }

    #[test]
    fn empty_bundle_is_empty() {
        let bundle = ComponentBundle::empty();
        assert!(bundle.is_empty());
        assert_eq!(bundle.insert.len(), 0);
        assert_eq!(bundle.remove.len(), 0);
    }
}
