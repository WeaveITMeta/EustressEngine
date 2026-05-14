//! Canonical entity-creation pipeline — engine-side façade.
//!
//! The actual implementation lives in
//! [`eustress_common::instance_create`] so that the out-of-process MCP
//! server (`eustress-tools`) can call the same helper. This module is
//! a thin Bevy-typed wrapper: callers in the engine that already hold
//! a `Vec3` for position/scale can keep doing so, and we convert at
//! the boundary.
//!
//! All real logic — template lookup, parent-first recursive copy,
//! TOML override patching, unique-name allocation — is in common.

use std::path::{Path, PathBuf};
use bevy::math::Vec3;

pub use eustress_common::instance_create::{
    CreatedInstance, CreateError,
};

/// Bevy-friendly override struct. Mirrors
/// [`eustress_common::instance_create::InstanceOverrides`] but uses
/// `Vec3` for the geometric fields so engine call sites don't have to
/// translate. Converted to the common form inside [`create_instance`].
#[derive(Debug, Clone, Default)]
pub struct InstanceOverrides {
    pub display_name: Option<String>,
    pub position: Option<Vec3>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<Vec3>,
    pub color_rgba: Option<[f32; 4]>,
    pub material: Option<String>,
    pub anchored: Option<bool>,
    pub can_collide: Option<bool>,
    pub asset_mesh: Option<String>,
    pub asset_path: Option<String>,
    pub unit_symbol: Option<String>,
}

impl From<InstanceOverrides> for eustress_common::instance_create::InstanceOverrides {
    fn from(value: InstanceOverrides) -> Self {
        Self {
            display_name: value.display_name,
            position: value.position.map(|v| [v.x, v.y, v.z]),
            rotation: value.rotation,
            scale: value.scale.map(|v| [v.x, v.y, v.z]),
            color_rgba: value.color_rgba,
            material: value.material,
            anchored: value.anchored,
            can_collide: value.can_collide,
            asset_mesh: value.asset_mesh,
            asset_path: value.asset_path,
            unit_symbol: value.unit_symbol,
        }
    }
}

/// Engine-facing entry point. Forwards to the common implementation.
pub fn create_instance(
    dest_dir: &Path,
    class_name: &str,
    requested_name: Option<&str>,
    overrides: InstanceOverrides,
) -> Result<CreatedInstance, CreateError> {
    eustress_common::instance_create::create_instance(
        dest_dir,
        class_name,
        requested_name,
        overrides.into(),
    )
}

/// Convenience re-export so callers can `use crate::space::instance_create::*`
/// and get both the override struct and the result types.
#[allow(dead_code)]
pub fn folder_path_of(result: &CreatedInstance) -> &PathBuf {
    &result.folder_path
}
