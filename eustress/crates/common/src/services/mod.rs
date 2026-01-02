//! # Services Module
//! 
//! Service-oriented architecture - shared data types for Engine and Client.
//! 
//! ## Architecture
//! 
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     eustress-common                              │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌────────────┐ │
//! │  │   Player    │ │  Lighting   │ │    Sound    │ │  Workspace │ │
//! │  │  Service    │ │  Service    │ │   Service   │ │   Service  │ │
//! │  │  (types)    │ │  (types)    │ │   (types)   │ │   (types)  │ │
//! │  └─────────────┘ └─────────────┘ └─────────────┘ └────────────┘ │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌────────────┐ │
//! │  │     AI      │ │   Physics   │ │    Input    │ │    Run     │ │
//! │  │  Service    │ │  Service    │ │   Service   │ │   Service  │ │
//! │  │  (types)    │ │  (types)    │ │   (types)   │ │   (types)  │ │
//! │  └─────────────┘ └─────────────┘ └─────────────┘ └────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!              ┌───────────────┴───────────────┐
//!              ▼                               ▼
//! ┌─────────────────────────┐     ┌─────────────────────────┐
//! │    eustress-client      │     │    eustress-engine      │
//! │  ┌───────────────────┐  │     │  ┌───────────────────┐  │
//! │  │  PlayerPlugin     │  │     │  │  EditorPlugin     │  │
//! │  │  (Avian physics)  │  │     │  │  (gizmos, tools)  │  │
//! │  ├───────────────────┤  │     │  ├───────────────────┤  │
//! │  │  LightingPlugin   │  │     │  │  PreviewPlugin    │  │
//! │  │  (runtime sky)    │  │     │  │  (play mode)      │  │
//! │  └───────────────────┘  │     │  └───────────────────┘  │
//! └─────────────────────────┘     └─────────────────────────┘
//! ```
//! 
//! ## Services
//! 
//! | Service | Description | Common Types |
//! |---------|-------------|--------------|
//! | `PlayerService` | Player/character management | Player, Character, PlayerCamera |
//! | `LightingService` | Scene lighting, skybox | LightingService, Atmosphere, Sky |
//! | `SoundService` | Audio playback | SoundService, Sound, SoundGroup |
//! | `WorkspaceService` | Scene hierarchy | Workspace, Instance, Part |
//! | `AIService` | NPC behavior, pathfinding | AIService, NPC, Behavior |
//! | `PhysicsService` | Physics configuration | PhysicsService, CollisionGroup |
//! | `InputService` | Input handling | InputService, InputAction |
//! | `RunService` | Game loop hooks | RunService, GameState |
//! | `TweenService` | Animation/interpolation | TweenService, Tween |
//! | `TeamService` | Team management | TeamService, Team |

// Core services
pub mod player;
pub mod lighting;
pub mod workspace;
pub mod sound;
pub mod animation;

// Gameplay services  
pub mod ai;
pub mod physics;
pub mod input;
pub mod gamepad;
pub mod run;
pub mod tween;
pub mod team;
pub mod group;

// Platform services (persistence, monetization, cross-server)
pub mod datastore;
pub mod teleport;
pub mod teleport_networking;
pub mod marketplace;

// Time, logging, and history services
pub mod time;
pub mod log;
pub mod moderation;

// AI generation service (integrates with Asset System)
pub mod generation;
pub mod generation_backends;

// Scene-to-services integration
pub mod scene_services;

// Re-export all service types
pub use player::*;
pub use lighting::*;
pub use workspace::*;
pub use sound::*;
pub use animation::*;
pub use ai::*;
pub use physics::*;
pub use input::*;
pub use gamepad::*;
pub use run::*;
pub use tween::*;
pub use team::*;
pub use group::*;
pub use time::*;
pub use log::*;
pub use datastore::*;
pub use teleport::*;
pub use teleport_networking::*;
pub use marketplace::*;
pub use generation::*;
pub use scene_services::*;
pub use moderation::*;

// NOTE: Instance, Part, Model, BasePart, Humanoid, etc. are in `crate::classes`.
