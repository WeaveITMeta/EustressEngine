//! # Spawn Helpers
//!
//! Functions to spawn Eustress class entities with proper Bevy 0.17 components.

#![allow(dead_code)]

use bevy::prelude::*;
use bevy::asset::AssetServer;
use bevy::pbr::StandardMaterial;
use bevy::pbr::decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt};
use bevy::light::{NotShadowCaster, TransmittedShadowReceiver};
use bevy::math::primitives::{Cuboid, Sphere, Cylinder};
use bevy::ui::{self, widget::Text as UiText};
use avian3d::prelude::{Collider, RigidBody};

#[allow(unused_imports)]
use crate::classes::{
    Instance, BasePart, Part, PartType, MeshPart, Model, Humanoid,
    EustressCamera, EustressPointLight, EustressSpotLight, SurfaceLight, EustressDirectionalLight,
    Sound, Attachment, WeldConstraint, Motor6D, ParticleEmitter, Beam,
    Sky, SkyboxTextures, Terrain, BillboardGui, TextLabel, Folder,
    SpecialMesh, Decal, UnionOperation, Animator, KeyframeSequence, Atmosphere,
    ScreenGui, SurfaceGui, Frame, ScrollingFrame, ImageLabel, TextButton, ImageButton,
    VideoFrame, DocumentFrame, WebFrame, TextBox, ViewportFrame,
};
use crate::rendering::PartEntity;
use eustress_common::{Attributes, Tags};

// ============================================================================
// Part Spawning
// ============================================================================

/// Spawn a Part entity with mesh, material, and physics collider
/// 
/// NOTE: Meshes are created at ACTUAL SIZE (BasePart.size).
/// Transform.scale should remain Vec3::ONE.
/// The Scale Tool updates BasePart.size and regenerates the mesh.
pub fn spawn_part(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    instance: Instance,
    base_part: BasePart,
    part: Part,
) -> Entity {
    let name = instance.name.clone();
    let size = base_part.size;
    
    // Create mesh at ACTUAL SIZE - transform.scale stays at Vec3::ONE
    let mesh = match part.shape {
        PartType::Block => meshes.add(Cuboid::from_size(size)),
        PartType::Ball => meshes.add(Sphere::new(size.x / 2.0)),
        PartType::Cylinder => meshes.add(Cylinder::new(size.x / 2.0, size.y)),
        PartType::Wedge => meshes.add(Cuboid::from_size(size)), // TODO: proper wedge mesh
        PartType::CornerWedge => meshes.add(Cuboid::from_size(size)), // TODO: proper corner wedge
        PartType::Cone => meshes.add(Cylinder::new(size.x / 2.0, size.y)), // TODO: proper cone
    };
    
    // Create collider at ACTUAL SIZE for physics raycasting
    let collider = match part.shape {
        PartType::Ball => Collider::sphere(size.x / 2.0),
        PartType::Cylinder | PartType::Cone => Collider::cylinder(size.x / 2.0, size.y),
        _ => Collider::cuboid(size.x, size.y, size.z),
    };
    
    // Create material with special handling for Glass
    let (roughness, metallic, reflectance) = base_part.material.pbr_params();
    let is_glass = matches!(base_part.material, crate::classes::Material::Glass);
    let transparency = base_part.transparency;
    
    let material = materials.add(StandardMaterial {
        base_color: base_part.color,
        perceptual_roughness: roughness,
        metallic,
        reflectance,
        alpha_mode: if transparency > 0.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        // Glass material gets specular/diffuse transmission for colored shadows
        specular_transmission: if is_glass { 0.9 } else { 0.0 },
        diffuse_transmission: if is_glass { 0.3 } else { 0.0 },
        thickness: if is_glass { 0.5 } else { 0.0 },
        ior: if is_glass { 1.5 } else { 1.5 }, // Index of refraction for glass
        ..default()
    });
    
    // Objects with >= 50% transparency don't cast shadows
    let no_shadow = transparency >= 0.5;
    // Glass with < 50% transparency gets TransmittedShadowReceiver for colored shadows
    let glass_with_transmission = is_glass && transparency < 0.5;
    
    let entity = commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        base_part.cframe, // Transform with scale = Vec3::ONE
        instance,
        base_part,
        part,
        collider,
        RigidBody::Static, // Static body for editor (no physics simulation)
        Name::new(name),
        PartEntity { part_id: String::new() },
        // Default Attributes and Tags for all entities (Phase 3)
        Attributes::new(),
        Tags::new(),
    )).id();
    
    // Highly transparent objects (>=50%) should not cast shadows
    if no_shadow {
        commands.entity(entity).insert(NotShadowCaster);
    }
    
    // Glass material with < 50% transparency gets colored shadow transmission
    if glass_with_transmission {
        commands.entity(entity).insert(TransmittedShadowReceiver);
    }
    
    entity
}

/// Spawn a MeshPart entity (custom mesh from asset)
pub fn spawn_mesh_part(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    instance: Instance,
    base_part: BasePart,
    mesh_part: MeshPart,
) -> Entity {
    let name = instance.name.clone();
    let size = base_part.size;
    
    // Load mesh from asset or use default cube
    let mesh: Handle<Mesh> = if !mesh_part.mesh_id.is_empty() {
        asset_server.load(&mesh_part.mesh_id)
    } else {
        // Use a procedural cube as default placeholder
        meshes.add(Cuboid::from_size(size))
    };
    
    // Create collider based on size for physics raycasting
    // Use cuboid as approximation for custom meshes
    let collider = Collider::cuboid(size.x, size.y, size.z);
    
    // Create material with special handling for Glass
    let (roughness, metallic, reflectance) = base_part.material.pbr_params();
    let is_glass = matches!(base_part.material, crate::classes::Material::Glass);
    let transparency = base_part.transparency;
    
    let material = materials.add(StandardMaterial {
        base_color: base_part.color,
        perceptual_roughness: roughness,
        metallic,
        reflectance,
        alpha_mode: if transparency > 0.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        // Glass material gets specular/diffuse transmission for colored shadows
        specular_transmission: if is_glass { 0.9 } else { 0.0 },
        diffuse_transmission: if is_glass { 0.3 } else { 0.0 },
        thickness: if is_glass { 0.5 } else { 0.0 },
        ior: if is_glass { 1.5 } else { 1.5 },
        ..default()
    });
    
    // Objects with >= 50% transparency don't cast shadows
    let no_shadow = transparency >= 0.5;
    // Glass with < 50% transparency gets TransmittedShadowReceiver for colored shadows
    let glass_with_transmission = is_glass && transparency < 0.5;
    
    let entity = commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        base_part.cframe,
        instance,
        base_part,
        mesh_part,
        collider,
        RigidBody::Static,
        Name::new(name),
        PartEntity { part_id: String::new() },
        Attributes::new(),
        Tags::new(),
    )).id();
    
    // Highly transparent objects (>=50%) should not cast shadows
    if no_shadow {
        commands.entity(entity).insert(NotShadowCaster);
    }
    
    // Glass material with < 50% transparency gets colored shadow transmission
    if glass_with_transmission {
        commands.entity(entity).insert(TransmittedShadowReceiver);
    }
    
    entity
}

// ============================================================================
// Model/Container Spawning
// ============================================================================

/// Spawn a Model entity (container, no geometry)
pub fn spawn_model(
    commands: &mut Commands,
    instance: Instance,
    model: Model,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        model,
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

/// Spawn a Folder entity (organizational container)
pub fn spawn_folder(
    commands: &mut Commands,
    instance: Instance,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        Folder::default(),
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

/// Spawn a Folder entity with domain configuration
pub fn spawn_folder_with_config(
    commands: &mut Commands,
    instance: Instance,
    folder: Folder,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        folder,
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

// ============================================================================
// Humanoid Spawning
// ============================================================================

/// Spawn a Humanoid entity
pub fn spawn_humanoid(
    commands: &mut Commands,
    instance: Instance,
    humanoid: Humanoid,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        humanoid,
        Name::new(name),
    )).id()
}

// ============================================================================
// Camera Spawning
// ============================================================================

/// Spawn a Camera entity
pub fn spawn_camera(
    commands: &mut Commands,
    instance: Instance,
    camera: EustressCamera,
    transform: Transform,
) -> Entity {
    use bevy::core_pipeline::tonemapping::Tonemapping;
    
    let name = instance.name.clone();
    commands.spawn((
        Camera3d::default(),
        Tonemapping::Reinhard,  // Avoid magenta bug from missing LUT textures
        transform,
        instance,
        camera,
        Name::new(name),
    )).id()
}

// ============================================================================
// Light Spawning
// ============================================================================
//
// Light Textures (Bevy 0.17+):
// - PointLightTexture: Cubemap texture for omnidirectional light cookies
// - SpotLightTexture: 2D texture projected onto illuminated surfaces
// - DirectionalLightTexture: 2D texture for sun/directional light patterns
//
// To add a light texture, use the `texture` field on the Eustress light component
// and load the asset via AssetServer. The spawn functions below will need to be
// extended to accept AssetServer and load textures when the field is Some.

/// Spawn a PointLight entity
/// 
/// If `light.texture` is Some, a PointLightTexture component should be added
/// with a cubemap texture loaded from the asset path.
pub fn spawn_point_light(
    commands: &mut Commands,
    instance: Instance,
    light: EustressPointLight,
    transform: Transform,
) -> Entity {
    let name = instance.name.clone();
    // TODO: If light.texture is Some, load cubemap and add PointLightTexture component
    commands.spawn((
        PointLight {
            color: light.color,
            intensity: light.brightness * 1000.0,
            range: light.range,
            shadows_enabled: light.shadows,
            ..default()
        },
        transform,
        instance,
        light,
        Name::new(name),
    )).id()
}

/// Spawn a SpotLight entity
/// 
/// If `light.texture` is Some, a SpotLightTexture component should be added
/// with a 2D texture loaded from the asset path.
pub fn spawn_spot_light(
    commands: &mut Commands,
    instance: Instance,
    light: EustressSpotLight,
    transform: Transform,
) -> Entity {
    let name = instance.name.clone();
    // TODO: If light.texture is Some, load texture and add SpotLightTexture component
    commands.spawn((
        SpotLight {
            color: light.color,
            intensity: light.brightness * 1000.0,
            range: light.range,
            inner_angle: light.angle * 0.8,
            outer_angle: light.angle,
            shadows_enabled: light.shadows,
            ..default()
        },
        transform,
        instance,
        light,
        Name::new(name),
    )).id()
}

/// Spawn a SurfaceLight entity
pub fn spawn_surface_light(
    commands: &mut Commands,
    instance: Instance,
    light: SurfaceLight,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        PointLight {
            color: light.color,
            intensity: light.brightness * 500.0,
            range: light.range,
            shadows_enabled: light.shadows,
            ..default()
        },
        Transform::default(),
        instance,
        light,
        Name::new(name),
    )).id()
}

/// Spawn a DirectionalLight entity (sun/global light)
/// 
/// If `light.texture` is Some, a DirectionalLightTexture component should be added
/// with a 2D texture loaded from the asset path (e.g., cloud shadows).
pub fn spawn_directional_light(
    commands: &mut Commands,
    instance: Instance,
    light: EustressDirectionalLight,
    transform: Transform,
) -> Entity {
    let name = instance.name.clone();
    // TODO: If light.texture is Some, load texture and add DirectionalLightTexture component
    commands.spawn((
        bevy::prelude::DirectionalLight {
            color: light.color,
            illuminance: light.brightness * 10000.0,
            shadows_enabled: light.shadows,
            shadow_depth_bias: light.shadow_depth_bias,
            shadow_normal_bias: light.shadow_normal_bias,
            ..default()
        },
        transform,
        instance,
        light,
        Name::new(name),
    )).id()
}

// ============================================================================
// Sound Spawning
// ============================================================================

/// Spawn a Sound entity
pub fn spawn_sound(
    commands: &mut Commands,
    _asset_server: &AssetServer,
    instance: Instance,
    sound: Sound,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        sound,
        Name::new(name),
    )).id()
}

// ============================================================================
// Constraint Spawning
// ============================================================================

/// Spawn an Attachment entity
pub fn spawn_attachment(
    commands: &mut Commands,
    instance: Instance,
    attachment: Attachment,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::from_translation(attachment.position),
        Visibility::default(),
        instance,
        attachment,
        Name::new(name),
    )).id()
}

/// Spawn a WeldConstraint entity
pub fn spawn_weld_constraint(
    commands: &mut Commands,
    instance: Instance,
    weld: WeldConstraint,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        weld,
        Name::new(name),
    )).id()
}

/// Spawn a Motor6D entity
pub fn spawn_motor6d(
    commands: &mut Commands,
    instance: Instance,
    motor: Motor6D,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        motor,
        Name::new(name),
    )).id()
}

// ============================================================================
// Effect Spawning
// ============================================================================

/// Spawn a ParticleEmitter entity
pub fn spawn_particle_emitter(
    commands: &mut Commands,
    instance: Instance,
    emitter: ParticleEmitter,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        emitter,
        Name::new(name),
    )).id()
}

/// Spawn a Beam entity
pub fn spawn_beam(
    commands: &mut Commands,
    instance: Instance,
    beam: Beam,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        beam,
        Name::new(name),
    )).id()
}

// ============================================================================
// Environment Spawning
// ============================================================================

/// Spawn a Sky entity
pub fn spawn_sky(
    commands: &mut Commands,
    instance: Instance,
    sky: Sky,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        sky,
        Name::new(name),
    )).id()
}

/// Spawn an Atmosphere entity
pub fn spawn_atmosphere(
    commands: &mut Commands,
    instance: Instance,
    atmosphere: Atmosphere,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        atmosphere,
        Name::new(name),
    )).id()
}

/// Spawn a Terrain entity
#[allow(dead_code)]
pub fn spawn_terrain(
    commands: &mut Commands,
    instance: Instance,
    terrain: Terrain,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        terrain,
        Name::new(name),
    )).id()
}

// ============================================================================
// GUI Spawning - Bevy UI Integration
// ============================================================================

/// Spawn a ScreenGui entity (root UI container for screen-space UI)
/// Maps to: Bevy Node (fullscreen, absolute position)
pub fn spawn_screen_gui(
    commands: &mut Commands,
    instance: Instance,
    gui: ScreenGui,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        instance,
        gui,
        Name::new(name),
        // Bevy UI components for rendering
        ui::Node {
            width: ui::Val::Percent(100.0),
            height: ui::Val::Percent(100.0),
            position_type: ui::PositionType::Absolute,
            ..default()
        },
        ui::GlobalZIndex(0), // ScreenGui display_order maps to this
    )).id()
}

/// Spawn a BillboardGui entity (3D world-space UI that faces camera)
/// Maps to: Transform + Visibility + BillboardGuiMarker for 3D positioning
/// Children (TextLabel, etc.) are rendered via egui overlay system
pub fn spawn_billboard_gui(
    commands: &mut Commands,
    instance: Instance,
    gui: BillboardGui,
) -> Entity {
    let name = instance.name.clone();
    
    // Position offset from parent
    let offset = Vec3::new(gui.units_offset[0], gui.units_offset[1], gui.units_offset[2]);
    
    // BillboardGui needs Transform for 3D positioning relative to parent
    // The actual UI rendering happens via billboard_gui.rs render_billboard_gui_egui system
    commands.spawn((
        Transform::from_translation(offset),
        Visibility::default(),
        instance,
        gui,
        Name::new(name),
        // Marker for billboard rendering system
        BillboardGuiMarker,
    )).id()
}

/// Marker component for BillboardGui entities (for rendering system queries)
#[derive(Component, Default)]
pub struct BillboardGuiMarker;

/// Spawn a SurfaceGui entity (UI rendered on a part's surface)
/// Maps to: Custom render-to-texture (requires parent BasePart)
pub fn spawn_surface_gui(
    commands: &mut Commands,
    instance: Instance,
    gui: SurfaceGui,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        gui,
        Name::new(name),
        // Marker for surface rendering system
        SurfaceGuiMarker,
    )).id()
}

/// Marker component for SurfaceGui entities
#[derive(Component, Default)]
pub struct SurfaceGuiMarker;

/// Spawn a Frame entity (basic UI container)
/// Maps to: Bevy Node with BackgroundColor and BorderColor
pub fn spawn_frame(
    commands: &mut Commands,
    instance: Instance,
    frame: Frame,
) -> Entity {
    let name = instance.name.clone();
    
    // Convert Eustress colors to Bevy
    let bg_color = Color::srgba(
        frame.background_color3[0],
        frame.background_color3[1],
        frame.background_color3[2],
        1.0 - frame.background_transparency,
    );
    let border_color = Color::srgb(
        frame.border_color3[0],
        frame.border_color3[1],
        frame.border_color3[2],
    );
    
    commands.spawn((
        instance,
        frame.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(frame.size_offset[0]),
            height: ui::Val::Px(frame.size_offset[1]),
            left: ui::Val::Px(frame.position_offset[0]),
            top: ui::Val::Px(frame.position_offset[1]),
            border: ui::UiRect::all(ui::Val::Px(frame.border_size_pixel as f32)),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::BorderColor::from(border_color),
        ui::ZIndex(frame.z_index),
    )).id()
}

/// Spawn a ScrollingFrame entity (scrollable container)
/// Maps to: Bevy Node with ScrollPosition and Overflow::scroll()
pub fn spawn_scrolling_frame(
    commands: &mut Commands,
    instance: Instance,
    frame: ScrollingFrame,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        frame.background_color3[0],
        frame.background_color3[1],
        frame.background_color3[2],
        1.0 - frame.background_transparency,
    );
    let border_color = Color::srgb(
        frame.border_color3[0],
        frame.border_color3[1],
        frame.border_color3[2],
    );
    
    commands.spawn((
        instance,
        frame.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(frame.size_offset[0]),
            height: ui::Val::Px(frame.size_offset[1]),
            left: ui::Val::Px(frame.position_offset[0]),
            top: ui::Val::Px(frame.position_offset[1]),
            border: ui::UiRect::all(ui::Val::Px(frame.border_size_pixel as f32)),
            overflow: ui::Overflow::scroll(),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::BorderColor::from(border_color),
        ui::ZIndex(frame.z_index),
        ui::ScrollPosition::default(),
    )).id()
}

/// Spawn a TextLabel entity (non-interactive text display)
/// When parented to BillboardGui: rendered via egui overlay system (no Bevy UI components)
/// When parented to ScreenGui/Frame: uses Bevy UI components
pub fn spawn_text_label(
    commands: &mut Commands,
    instance: Instance,
    label: TextLabel,
) -> Entity {
    let name = instance.name.clone();
    
    // TextLabel stores all properties in the component itself
    // The rendering system (billboard_gui.rs or UI system) reads these properties
    // and renders appropriately based on parent type
    commands.spawn((
        instance,
        label,
        Name::new(name),
        // Marker for text label queries
        TextLabelMarker,
    )).id()
}

/// Spawn a TextLabel entity with full Bevy UI components (for ScreenGui/Frame parents)
pub fn spawn_text_label_ui(
    commands: &mut Commands,
    instance: Instance,
    label: TextLabel,
) -> Entity {
    let name = instance.name.clone();
    
    let text_color = Color::srgba(
        label.text_color3[0],
        label.text_color3[1],
        label.text_color3[2],
        1.0 - label.text_transparency,
    );
    let bg_color = Color::srgba(
        label.background_color3[0],
        label.background_color3[1],
        label.background_color3[2],
        1.0 - label.background_transparency,
    );
    
    // Convert alignment
    let justify = match label.text_x_alignment {
        crate::classes::TextXAlignment::Left => ui::JustifyContent::FlexStart,
        crate::classes::TextXAlignment::Center => ui::JustifyContent::Center,
        crate::classes::TextXAlignment::Right => ui::JustifyContent::FlexEnd,
    };
    let align = match label.text_y_alignment {
        crate::classes::TextYAlignment::Top => ui::AlignItems::FlexStart,
        crate::classes::TextYAlignment::Center => ui::AlignItems::Center,
        crate::classes::TextYAlignment::Bottom => ui::AlignItems::FlexEnd,
    };
    
    commands.spawn((
        instance,
        label.clone(),
        Name::new(name),
        TextLabelMarker,
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(label.size[0]),
            height: ui::Val::Px(label.size[1]),
            left: ui::Val::Px(label.position[0]),
            top: ui::Val::Px(label.position[1]),
            justify_content: justify,
            align_items: align,
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::ZIndex(label.z_index),
        // Text component
        UiText::new(label.text.clone()),
        bevy::text::TextColor(text_color),
        bevy::text::TextFont {
            font_size: label.font_size,
            ..default()
        },
        ui::widget::Label, // Marker for non-interactive text
    )).id()
}

/// Marker component for TextLabel entities
#[derive(Component, Default)]
pub struct TextLabelMarker;

/// Spawn an ImageLabel entity (non-interactive image display)
/// Maps to: Bevy ImageNode
pub fn spawn_image_label(
    commands: &mut Commands,
    asset_server: &AssetServer,
    instance: Instance,
    label: ImageLabel,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        label.background_color3[0],
        label.background_color3[1],
        label.background_color3[2],
        1.0 - label.background_transparency,
    );
    
    // Load image if path is set
    let image_handle: Handle<Image> = if !label.image.is_empty() {
        asset_server.load(&label.image)
    } else {
        Handle::default()
    };
    
    commands.spawn((
        instance,
        label.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(label.size_offset[0]),
            height: ui::Val::Px(label.size_offset[1]),
            left: ui::Val::Px(label.position_offset[0]),
            top: ui::Val::Px(label.position_offset[1]),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::ZIndex(label.z_index),
        // Image component
        ui::widget::ImageNode::new(image_handle),
    )).id()
}

/// Spawn a TextButton entity (clickable text)
/// Maps to: Bevy Button + Text
pub fn spawn_text_button(
    commands: &mut Commands,
    instance: Instance,
    button: TextButton,
) -> Entity {
    let name = instance.name.clone();
    
    let text_color = Color::srgba(
        button.text_color3[0],
        button.text_color3[1],
        button.text_color3[2],
        1.0 - button.text_transparency,
    );
    let bg_color = Color::srgba(
        button.background_color3[0],
        button.background_color3[1],
        button.background_color3[2],
        1.0 - button.background_transparency,
    );
    let border_color = Color::srgb(
        button.border_color3[0],
        button.border_color3[1],
        button.border_color3[2],
    );
    
    commands.spawn((
        instance,
        button.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(button.size_offset[0]),
            height: ui::Val::Px(button.size_offset[1]),
            left: ui::Val::Px(button.position_offset[0]),
            top: ui::Val::Px(button.position_offset[1]),
            border: ui::UiRect::all(ui::Val::Px(button.border_size_pixel as f32)),
            justify_content: ui::JustifyContent::Center,
            align_items: ui::AlignItems::Center,
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::BorderColor::from(border_color),
        ui::ZIndex(button.z_index),
        // Button marker for interaction
        ui::widget::Button,
        ui::Interaction::default(),
        // Text
        UiText::new(button.text.clone()),
        bevy::text::TextColor(text_color),
        bevy::text::TextFont {
            font_size: button.font_size,
            ..default()
        },
    )).id()
}

/// Spawn an ImageButton entity (clickable image)
/// Maps to: Bevy Button + ImageNode
pub fn spawn_image_button(
    commands: &mut Commands,
    asset_server: &AssetServer,
    instance: Instance,
    button: ImageButton,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        button.background_color3[0],
        button.background_color3[1],
        button.background_color3[2],
        1.0 - button.background_transparency,
    );
    let border_color = Color::srgb(
        button.border_color3[0],
        button.border_color3[1],
        button.border_color3[2],
    );
    
    // Load image if path is set
    let image_handle: Handle<Image> = if !button.image.is_empty() {
        asset_server.load(&button.image)
    } else {
        Handle::default()
    };
    
    commands.spawn((
        instance,
        button.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(button.size_offset[0]),
            height: ui::Val::Px(button.size_offset[1]),
            left: ui::Val::Px(button.position_offset[0]),
            top: ui::Val::Px(button.position_offset[1]),
            border: ui::UiRect::all(ui::Val::Px(button.border_size_pixel as f32)),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::BorderColor::from(border_color),
        ui::ZIndex(button.z_index),
        // Button marker for interaction
        ui::widget::Button,
        ui::Interaction::default(),
        // Image
        ui::widget::ImageNode::new(image_handle),
    )).id()
}

/// Spawn a TextBox entity (text input field)
/// Maps to: Custom text input (Bevy has no native text input)
pub fn spawn_text_box(
    commands: &mut Commands,
    instance: Instance,
    text_box: TextBox,
) -> Entity {
    let name = instance.name.clone();
    
    let text_color = Color::srgba(
        text_box.text_color3[0],
        text_box.text_color3[1],
        text_box.text_color3[2],
        1.0 - text_box.text_transparency,
    );
    let bg_color = Color::srgba(
        text_box.background_color3[0],
        text_box.background_color3[1],
        text_box.background_color3[2],
        1.0 - text_box.background_transparency,
    );
    let border_color = Color::srgb(
        text_box.border_color3[0],
        text_box.border_color3[1],
        text_box.border_color3[2],
    );
    
    commands.spawn((
        instance,
        text_box.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(text_box.size_offset[0]),
            height: ui::Val::Px(text_box.size_offset[1]),
            left: ui::Val::Px(text_box.position_offset[0]),
            top: ui::Val::Px(text_box.position_offset[1]),
            border: ui::UiRect::all(ui::Val::Px(text_box.border_size_pixel as f32)),
            padding: ui::UiRect::all(ui::Val::Px(4.0)),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::BorderColor::from(border_color),
        ui::ZIndex(text_box.z_index),
        // Text display
        UiText::new(text_box.text.clone()),
        bevy::text::TextColor(text_color),
        bevy::text::TextFont {
            font_size: text_box.font_size,
            ..default()
        },
        // Marker for text input handling
        TextBoxMarker,
        ui::Interaction::default(),
    )).id()
}

/// Marker component for TextBox entities (for input handling system)
#[derive(Component, Default)]
pub struct TextBoxMarker;

/// Spawn a ViewportFrame entity (3D viewport in UI)
/// Maps to: Bevy ViewportNode (direct mapping!)
pub fn spawn_viewport_frame(
    commands: &mut Commands,
    instance: Instance,
    frame: ViewportFrame,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        frame.background_color3[0],
        frame.background_color3[1],
        frame.background_color3[2],
        1.0 - frame.background_transparency,
    );
    
    commands.spawn((
        instance,
        frame.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(frame.size_offset[0]),
            height: ui::Val::Px(frame.size_offset[1]),
            left: ui::Val::Px(frame.position_offset[0]),
            top: ui::Val::Px(frame.position_offset[1]),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::ZIndex(frame.z_index),
        // ViewportNode marker for 3D rendering (requires camera setup)
        ViewportFrameMarker,
    )).id()
}

/// Spawn a VideoFrame entity (video display in UI)
/// Maps to: Custom video texture → ImageNode
pub fn spawn_video_frame(
    commands: &mut Commands,
    instance: Instance,
    frame: VideoFrame,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        frame.background_color3[0],
        frame.background_color3[1],
        frame.background_color3[2],
        1.0 - frame.background_transparency,
    );
    
    commands.spawn((
        instance,
        frame.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(frame.size_offset[0]),
            height: ui::Val::Px(frame.size_offset[1]),
            left: ui::Val::Px(frame.position_offset[0]),
            top: ui::Val::Px(frame.position_offset[1]),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::ZIndex(frame.z_index),
        // Placeholder image - video system will update this
        ui::widget::ImageNode::default(),
        VideoFrameMarker,
    )).id()
}

/// Marker component for VideoFrame entities
#[derive(Component, Default)]
pub struct VideoFrameMarker;

/// Spawn a DocumentFrame entity (document display in UI)
/// Maps to: Custom PDF/doc rendering
pub fn spawn_document_frame(
    commands: &mut Commands,
    instance: Instance,
    frame: DocumentFrame,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        frame.background_color3[0],
        frame.background_color3[1],
        frame.background_color3[2],
        1.0 - frame.background_transparency,
    );
    
    commands.spawn((
        instance,
        frame.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(frame.size_offset[0]),
            height: ui::Val::Px(frame.size_offset[1]),
            left: ui::Val::Px(frame.position_offset[0]),
            top: ui::Val::Px(frame.position_offset[1]),
            overflow: ui::Overflow::scroll(),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::ZIndex(frame.z_index),
        ui::ScrollPosition::default(),
        DocumentFrameMarker,
    )).id()
}

/// Marker component for DocumentFrame entities
#[derive(Component, Default)]
pub struct DocumentFrameMarker;

/// Spawn a WebFrame entity (web content in UI)
/// Maps to: Custom CEF/WebView integration
pub fn spawn_web_frame(
    commands: &mut Commands,
    instance: Instance,
    frame: WebFrame,
) -> Entity {
    let name = instance.name.clone();
    
    let bg_color = Color::srgba(
        frame.background_color3[0],
        frame.background_color3[1],
        frame.background_color3[2],
        1.0 - frame.background_transparency,
    );
    
    commands.spawn((
        instance,
        frame.clone(),
        Name::new(name),
        // Bevy UI components
        ui::Node {
            width: ui::Val::Px(frame.size_offset[0]),
            height: ui::Val::Px(frame.size_offset[1]),
            left: ui::Val::Px(frame.position_offset[0]),
            top: ui::Val::Px(frame.position_offset[1]),
            ..default()
        },
        ui::BackgroundColor(bg_color),
        ui::ZIndex(frame.z_index),
        // Placeholder - WebView system will render to texture
        ui::widget::ImageNode::default(),
        WebFrameMarker,
    )).id()
}

/// Marker component for WebFrame entities
#[derive(Component, Default)]
pub struct WebFrameMarker;

/// Marker component for ViewportFrame entities (for 3D rendering system)
#[derive(Component, Default)]
pub struct ViewportFrameMarker;

// ============================================================================
// Other Spawning
// ============================================================================

/// Spawn a SpecialMesh entity
pub fn spawn_special_mesh(
    commands: &mut Commands,
    instance: Instance,
    mesh: SpecialMesh,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        mesh,
        Name::new(name),
    )).id()
}

/// Spawn a Decal entity using Bevy's native ForwardDecal system (Bevy 0.16+)
/// 
/// Creates a forward decal that projects a texture onto nearby surfaces.
/// The decal uses the depth buffer to project onto geometry in the scene.
/// 
/// # Requirements
/// - Camera must have `DepthPrepass` component
/// - On WebGPU, camera must have `Msaa::Off`
pub fn spawn_decal(
    commands: &mut Commands,
    asset_server: &AssetServer,
    decal_materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    instance: Instance,
    decal: Decal,
    parent_transform: Transform,
) -> Entity {
    let name = instance.name.clone();
    
    // Load the decal texture
    let texture_handle = if !decal.texture.is_empty() {
        Some(asset_server.load(&decal.texture))
    } else {
        None
    };
    
    // Create the ForwardDecalMaterial
    let material = decal_materials.add(ForwardDecalMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(decal.color[0], decal.color[1], decal.color[2], decal.color[3] * decal.alpha()),
            base_color_texture: texture_handle,
            alpha_mode: AlphaMode::Blend,
            unlit: false,
            ..default()
        },
        extension: ForwardDecalMaterialExt {
            depth_fade_factor: decal.depth_fade_factor,
        },
    });
    
    // Calculate transform: position at parent + rotation based on face
    let decal_transform = Transform {
        translation: parent_transform.translation,
        rotation: parent_transform.rotation * decal.face.to_rotation(),
        scale: Vec3::splat(1.0), // Decal size controlled by scale
    };
    
    commands.spawn((
        ForwardDecal,
        MeshMaterial3d(material),
        decal_transform,
        instance,
        decal,
        Name::new(name),
    )).id()
}

/// Spawn a simple decal without parent transform (for standalone decals)
pub fn spawn_decal_at(
    commands: &mut Commands,
    asset_server: &AssetServer,
    decal_materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    texture_path: impl Into<String>,
    position: Vec3,
    scale: f32,
    depth_fade: f32,
) -> Entity {
    let path: String = texture_path.into();
    let texture_handle = asset_server.load(&path);
    
    let material = decal_materials.add(ForwardDecalMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(texture_handle),
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: ForwardDecalMaterialExt {
            depth_fade_factor: depth_fade,
        },
    });
    
    commands.spawn((
        ForwardDecal,
        MeshMaterial3d(material),
        Transform::from_translation(position).with_scale(Vec3::splat(scale)),
        Name::new(format!("Decal_{}", path)),
    )).id()
}

/// Spawn a UnionOperation entity
pub fn spawn_union(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    instance: Instance,
    base_part: BasePart,
    union_op: UnionOperation,
) -> Entity {
    let name = instance.name.clone();
    
    // Create a simple cube mesh for now (CSG would need proper implementation)
    let mesh = meshes.add(Cuboid::from_size(base_part.size));
    
    let (roughness, metallic, reflectance) = base_part.material.pbr_params();
    let material = materials.add(StandardMaterial {
        base_color: base_part.color,
        perceptual_roughness: roughness,
        metallic,
        reflectance,
        ..default()
    });
    
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        base_part.cframe,
        instance,
        base_part,
        union_op,
        Name::new(name),
        PartEntity { part_id: String::new() },
    )).id()
}

/// Spawn an Animator entity
pub fn spawn_animator(
    commands: &mut Commands,
    instance: Instance,
    animator: Animator,
    _parent: Entity,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        animator,
        Name::new(name),
    )).id()
}

/// Spawn a KeyframeSequence entity
pub fn spawn_keyframe_sequence(
    commands: &mut Commands,
    instance: Instance,
    sequence: KeyframeSequence,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        sequence,
        Name::new(name),
    )).id()
}
