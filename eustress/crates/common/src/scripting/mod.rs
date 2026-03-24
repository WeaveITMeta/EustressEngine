//! # Scripting Types
//!
//! Shared data types for Rune and Luau scripting languages.
//! These types mirror Roblox's API for compatibility while leveraging Bevy/Avian internals.
//!
//! ## Table of Contents
//!
//! 1. **Data Types** — Vector3, CFrame, Color3, UDim, UDim2, Ray
//! 2. **Instance API** — Instance creation, hierarchy, properties
//! 3. **Events** — Signal/Connection pattern for script communication
//! 4. **Services** — RunService, Players, TweenService wrappers
//! 5. **Plugin** — Bevy plugin connecting services to frame loop
//! 6. **DataStore** — AWS DynamoDB-backed persistent storage

pub mod types;
pub mod instance;
pub mod events;
pub mod services;
pub mod plugin;
pub mod datastore;

pub use types::*;
pub use instance::*;
pub use events::*;
pub use services::*;
pub use plugin::*;
pub use datastore::*;
