//! GPU instancing with per-instance colors using MaterialExtension.
//!
//! Architecture:
//! - ColoredMaterial = ExtendedMaterial<StandardMaterial, ColorExtension>
//! - Base StandardMaterial is shared by preset (Plastic, Metal, etc.)
//! - ColorExtension adds per-instance color uniform
//! - Custom shader multiplies base_color with instance color
//! - Bevy's instancing system batches entities with same base material
//!
//! This enables GPU instancing while maintaining unique colors per entity.

use bevy::prelude::*;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::render::render_resource::AsBindGroup;
use bevy::asset::embedded_asset;
use bevy_shader::ShaderRef;

/// Per-instance color component.
#[derive(Component, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct InstanceColor(pub Color);

impl Default for InstanceColor {
    fn default() -> Self {
        Self(Color::WHITE)
    }
}

/// Material extension that adds per-instance color uniform.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct ColorExtension {
    /// Per-instance color multiplied with base_color in shader at group(2) binding(0)
    #[uniform(0)]
    pub color: LinearRgba,
}

impl MaterialExtension for ColorExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://eustress_engine/parts/instanced_material.wgsl".into()
    }
}

/// Type alias for our colored material
pub type ColoredMaterial = ExtendedMaterial<StandardMaterial, ColorExtension>;

/// Plugin for GPU-instanced rendering with per-instance colors.
pub struct InstanceColorPlugin;

impl Plugin for InstanceColorPlugin {
    fn build(&self, app: &mut App) {
        // Embed the instance color shader into the binary
        embedded_asset!(app, "instanced_material.wgsl");

        app.register_type::<InstanceColor>()
            .add_plugins(MaterialPlugin::<ColoredMaterial>::default())
            .add_systems(Update, sync_instance_colors);
    }
}

/// Sync InstanceColor component changes to ColorExtension uniforms.
fn sync_instance_colors(
    mut materials: ResMut<Assets<ColoredMaterial>>,
    query: Query<(&InstanceColor, &MeshMaterial3d<ColoredMaterial>), Changed<InstanceColor>>,
) {
    for (instance_color, material_handle) in query.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.extension.color = LinearRgba::from(instance_color.0);
        }
    }
}
