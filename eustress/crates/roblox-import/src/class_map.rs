//! Roblox class name → Eustress [`ClassName`].
//!
//! Delegates to [`eustress_common::luau::compat::ClassMapping::map_class`]
//! for the string-level mapping, then re-resolves to a typed `ClassName`
//! via [`ClassName::from_str`].
//!
//! For Roblox classes that the compat layer doesn't cover but
//! [`ClassName::from_str`] does (e.g. classes added to the Eustress enum
//! after the compat table was last updated, or Roblox classes whose name
//! happens to be identical to a Eustress one — `Folder`, `Camera`,
//! `Atmosphere`), we fall through to a direct `from_str` attempt.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §9.

use eustress_common::classes::ClassName;
use eustress_common::luau::compat::ClassMapping;

/// Map a Roblox class name (e.g. `"Part"`, `"MeshPart"`, `"SpawnLocation"`)
/// to its Eustress `ClassName` equivalent.
///
/// Returns `None` for Roblox classes with no Eustress analogue. The
/// caller (the materializer) logs these into
/// `ImportReport::unmapped_classes` and skips the subtree.
///
/// ## Lookup order
///
/// 1. Direct `compat::ClassMapping::map_class` — covers the curated
///    list in the spec §9.
/// 2. Fallback `ClassName::from_str` — catches names the compat layer
///    didn't list but the enum knows (e.g. `Folder`, `Atmosphere`,
///    `Sky`, `Camera`, lights, constraints, GUI nodes).
///
/// Returns `None` only when both paths fail.
pub fn roblox_to_eustress_class(rbx_class: &str) -> Option<ClassName> {
    if let Some(s) = ClassMapping::map_class(rbx_class) {
        if let Ok(class) = ClassName::from_str(s) {
            return Some(class);
        }
    }
    // Fallback: many Roblox classes have identical names in Eustress
    // (e.g. `Folder`, `Sky`, `Atmosphere`, `Camera`, `Beam`,
    // `Attachment`, the constraint and light variants, the GUI
    // primitives). `ClassName::from_str` already understands legacy
    // aliases (`MeshPart` → `Part`, `Script` → `LuauScript`, etc.) so
    // it doubles as our recovery path for compat-table gaps.
    ClassName::from_str(rbx_class).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_basic_parts() {
        assert_eq!(roblox_to_eustress_class("Part"), Some(ClassName::Part));
        assert_eq!(roblox_to_eustress_class("MeshPart"), Some(ClassName::Part));
        assert_eq!(
            roblox_to_eustress_class("SpawnLocation"),
            Some(ClassName::SpawnLocation)
        );
    }

    #[test]
    fn maps_containers() {
        assert_eq!(roblox_to_eustress_class("Folder"), Some(ClassName::Folder));
        assert_eq!(roblox_to_eustress_class("Model"), Some(ClassName::Model));
    }

    #[test]
    fn maps_lights() {
        assert_eq!(
            roblox_to_eustress_class("PointLight"),
            Some(ClassName::PointLight)
        );
        assert_eq!(
            roblox_to_eustress_class("SpotLight"),
            Some(ClassName::SpotLight)
        );
        assert_eq!(
            roblox_to_eustress_class("SurfaceLight"),
            Some(ClassName::SurfaceLight)
        );
    }

    #[test]
    fn maps_environment() {
        assert_eq!(roblox_to_eustress_class("Sky"), Some(ClassName::Sky));
        assert_eq!(
            roblox_to_eustress_class("Atmosphere"),
            Some(ClassName::Atmosphere)
        );
        assert_eq!(roblox_to_eustress_class("Clouds"), Some(ClassName::Clouds));
        assert_eq!(
            roblox_to_eustress_class("Terrain"),
            Some(ClassName::Terrain)
        );
    }

    #[test]
    fn maps_scripts() {
        assert_eq!(
            roblox_to_eustress_class("Script"),
            Some(ClassName::LuauScript)
        );
        assert_eq!(
            roblox_to_eustress_class("LocalScript"),
            Some(ClassName::LuauLocalScript)
        );
        assert_eq!(
            roblox_to_eustress_class("ModuleScript"),
            Some(ClassName::LuauModuleScript)
        );
    }

    #[test]
    fn maps_events() {
        assert_eq!(
            roblox_to_eustress_class("RemoteEvent"),
            Some(ClassName::RemoteEvent)
        );
        assert_eq!(
            roblox_to_eustress_class("RemoteFunction"),
            Some(ClassName::RemoteFunction)
        );
        assert_eq!(
            roblox_to_eustress_class("BindableEvent"),
            Some(ClassName::BindableEvent)
        );
        assert_eq!(
            roblox_to_eustress_class("BindableFunction"),
            Some(ClassName::BindableFunction)
        );
    }

    #[test]
    fn maps_csg_operations() {
        // CSG operations all collapse to a baked-mesh Part per spec §7,
        // but at the class-name layer they only need to land on a
        // recognised ClassName. `UnionOperation` has its own enum
        // variant; the negate/intersect variants legacy-route to `Part`
        // via the materializer §7 dispatcher.
        assert_eq!(
            roblox_to_eustress_class("UnionOperation"),
            Some(ClassName::UnionOperation)
        );
    }

    #[test]
    fn unmapped_returns_none() {
        assert_eq!(roblox_to_eustress_class("Plugin"), None);
        assert_eq!(roblox_to_eustress_class("MarketplaceService"), None);
        assert_eq!(roblox_to_eustress_class("RandomNonsenseClass"), None);
    }

    #[test]
    fn maps_constraints() {
        assert_eq!(
            roblox_to_eustress_class("WeldConstraint"),
            Some(ClassName::WeldConstraint)
        );
        assert_eq!(
            roblox_to_eustress_class("HingeConstraint"),
            Some(ClassName::HingeConstraint)
        );
        assert_eq!(
            roblox_to_eustress_class("Motor6D"),
            Some(ClassName::Motor6D)
        );
        assert_eq!(
            roblox_to_eustress_class("Attachment"),
            Some(ClassName::Attachment)
        );
    }

    #[test]
    fn maps_gui_primitives() {
        assert_eq!(
            roblox_to_eustress_class("ScreenGui"),
            Some(ClassName::ScreenGui)
        );
        assert_eq!(roblox_to_eustress_class("Frame"), Some(ClassName::Frame));
        assert_eq!(
            roblox_to_eustress_class("TextLabel"),
            Some(ClassName::TextLabel)
        );
        assert_eq!(
            roblox_to_eustress_class("TextButton"),
            Some(ClassName::TextButton)
        );
        assert_eq!(
            roblox_to_eustress_class("ImageLabel"),
            Some(ClassName::ImageLabel)
        );
    }
}
