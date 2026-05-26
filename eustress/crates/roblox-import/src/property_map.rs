//! Roblox `Variant` properties → Eustress `PropertyValue` /
//! `InstanceOverrides`.
//!
//! Wave 1 scaffold — body is `todo!()`. See `docs/architecture/
//! ROBLOX_IMPORT_SPEC.md` §6 for the per-variant conversion table.

use std::collections::HashMap;

use eustress_common::classes::ClassName;

/// A mapped property set, ready to feed `create_instance` and to
/// emit into a `[properties.extras]` TOML block.
///
/// The split mirrors `InstanceOverrides`: the well-known slots
/// (position / rotation / color / material / anchored / can_collide /
/// asset_path / asset_mesh) flow through `overrides`; everything else
/// lives in `extras` and is round-tripped opaquely.
///
/// Wave 1: the typed fields are placeholders; `PropertyValue` and
/// `InstanceOverrides` integration land in Wave 2 once the rbx_dom_weak
/// dep is in scope and we can wire the actual Variant → PropertyValue
/// translation per spec §6.
#[derive(Debug, Default)]
pub struct PropertyBag {
    /// Will map onto `eustress_common::instance_create::InstanceOverrides`
    /// in Wave 2 — typed as `()` here so the scaffold compiles without
    /// committing to a specific shape that Wave 2 may refine.
    pub overrides: (),
    /// Remaining properties, keyed by Eustress property name.
    /// Value type stubbed as `String` for Wave 1; becomes
    /// `eustress_common::classes::PropertyValue` in Wave 2.
    pub extras: HashMap<String, String>,
}

/// Transform a Roblox property map (as decoded by `rbx_dom_weak`) into
/// a `PropertyBag` shaped for the target Eustress class.
///
/// `rbx_props` is `HashMap<String, rbx_dom_weak::types::Variant>` in
/// Wave 2; typed as `HashMap<String, ()>` here so the scaffold compiles
/// without the dep.
pub fn map_properties(
    rbx_props: &HashMap<String, ()>,
    target_class: ClassName,
) -> PropertyBag {
    let _ = (rbx_props, target_class);
    todo!("Wave 2: implement per-Variant arm from spec §6");
}
