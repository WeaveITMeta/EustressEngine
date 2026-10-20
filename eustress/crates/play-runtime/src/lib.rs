//! # The shared Play runtime
//!
//! The draw side of a Play session, which Studio's Play and the Player both
//! run, so a joined player sees what the host sees
//! (`docs/architecture/SHARED_PLAY_RUNTIME.md`): the step that draws the
//! session's DataModel tree into the ECS, the scripted camera, sounds,
//! particles, part materials and part colliders.

pub mod apply;
pub mod audio;
pub mod billboard_pipeline;
pub mod billboards;
pub mod camera;
pub mod gui_images;
pub mod hud;
pub mod hud_input;
pub mod joints;
pub mod material_registry;
pub mod materials;
pub mod part_colliders;
pub mod particle_render;
pub mod particles;
