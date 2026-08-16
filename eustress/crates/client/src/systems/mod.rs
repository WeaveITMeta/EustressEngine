//! Client systems for enhancement pipeline
//! 
//! Note: Player and Lighting are now in services/ module

pub mod agent_control;
pub mod frame_capture;
pub mod window_icon;
pub mod scene_loader;
pub mod space_fetch;
pub mod space_world;
pub mod enhancement_scheduler;
pub mod asset_applicator;
pub mod distance_chunking;
pub mod llm_quest;

pub use scene_loader::*;
pub use enhancement_scheduler::*;
pub use asset_applicator::*;
pub use distance_chunking::*;
pub use llm_quest::*;
