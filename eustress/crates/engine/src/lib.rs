//! # Eustress Engine Library
//! 
//! Desktop editor/studio functionality.

pub mod auth;
pub mod parts;
pub mod rendering;
pub mod camera;
pub mod commands;
pub mod scenes;
pub mod classes;
pub mod properties;
pub mod serialization;
pub mod plugins;
pub mod shaders;
pub mod spawn;
pub mod soul;
pub mod korah;
pub mod telemetry;
pub mod play_server;
pub mod hot_reload;
pub mod pbr_materials;
pub mod particles;
pub mod beams;
pub mod decals;
pub mod billboard_gui;
pub mod light_cookies;
pub mod csg;
pub mod attachments;
pub mod motor6d;
pub mod mesh_part;
pub mod humanoid;
pub mod animation_state_machine;
pub mod foot_ik;
pub mod physics_constraints;
pub mod play_mode;
pub mod play_mode_runtime;
pub mod notifications;
pub mod ui;
pub mod seats;
pub mod keybindings;
pub mod editor_settings;
pub mod undo;
pub mod camera_controller;
pub mod runtime;
pub mod gizmo_tools;
pub mod move_tool;
pub mod rotate_tool;
pub mod scale_tool;
pub mod select_tool;
pub mod selection_box;
pub mod selection_sync;
pub mod part_selection;
pub mod material_sync;
pub mod transform_space;
pub mod default_scene;
pub mod startup;
pub mod terrain_plugin;
pub mod clipboard;
pub mod embedded_client;
pub mod studio_plugins;
pub mod grid_snapping;
pub mod collision_snapping;
pub mod replication;
pub mod asset_resolver;
pub mod xr_support;
pub mod backend_services;
pub mod platform_support;
pub mod network_benchmark;
pub mod math_utils;
pub mod entity_utils;
pub mod usd_loader;
pub mod physics_proxy;
pub mod generative_pipeline;

// Re-exports for convenience
pub use rendering::{PartRenderingPlugin, PartChanged};
pub use commands::{SelectionManager, TransformManager};
pub use serialization::{save_scene, load_scene, load_scene_from_world, Scene, SceneMetadata};
pub use classes::{PropertyAccess, PropertyValue, PropertyDescriptor};

// Re-export plugins
pub use plugins::{
    WorkspacePlugin, LightingPlugin, SoundPlugin,
    PhysicsPlugin, InputPlugin, RunPlugin,
    AllServicesPlugin,
};
