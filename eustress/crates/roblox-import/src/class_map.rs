//! Roblox class name → Eustress `ClassName`.
//!
//! Delegates to `eustress_common::luau::compat::ClassMapping::map_class`
//! for the string-level mapping, then re-resolves to a typed `ClassName`.
//!
//! Wave 1 scaffold — body is `todo!()`. See `docs/architecture/
//! ROBLOX_IMPORT_SPEC.md` §5 for the full mapping table.

use eustress_common::classes::ClassName;

/// Map a Roblox class name (e.g. `"Part"`, `"MeshPart"`, `"SpawnLocation"`)
/// to its Eustress `ClassName` equivalent.
///
/// Returns `None` for Roblox classes with no Eustress analogue
/// (logged into `ImportReport::unmapped_classes`).
///
/// Wave 2 implementation:
/// ```ignore
/// let s = eustress_common::luau::compat::ClassMapping::map_class(rbx_class)?;
/// class_name_from_str(s)   // local helper, ClassName::as_str inverse
/// ```
pub fn roblox_to_eustress_class(rbx_class: &str) -> Option<ClassName> {
    let _ = rbx_class;
    todo!("Wave 2: delegate to compat::ClassMapping + ClassName re-resolution");
}
