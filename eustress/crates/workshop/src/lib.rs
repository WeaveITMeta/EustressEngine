//! # eustress-workshop
//!
//! JARVIS-inspired physical-digital twin workshop system for Eustress Engine.
//!
//! Each registered tool is a `.tool.toml` file on disk. The file IS the tool's
//! entity definition, properties panel schema, mesh spawn config, and IoT binding
//! in one place. The in-memory `ToolIndex` is always rebuilt from these files —
//! the file system is the single source of truth.
//!
//! ## Table of Contents
//!
//! | Module            | Purpose                                                          |
//! |-------------------|------------------------------------------------------------------|
//! | `registry`        | `RegisteredTool`, `ToolCapability`, `ToolIndex` — TOML-backed    |
//! | `storage`         | `StorageUnit`, `StorageManager` — folder-per-container hierarchy |
//! | `status`          | `ToolStatus`, `IoTTelemetry` — live GPS + sensor data            |
//! | `guide`           | `BuildGuide`, `BuildStep` — AI step-by-step build instructions   |
//! | `procurement`     | `MissingItem`, `PurchaseList`, Amazon PA-API v5 + Alexa          |
//! | `knowledge`       | `ToolKnowledge` — embedded know-how database injected into AI    |
//! | `twin`            | `WorkshopTwinPlugin` — Bevy ECS digital twin Space integration   |
//!
//! ## File Layout
//!
//! The folder tree IS the physical layout of the workshop.
//! Each folder is a container (zone, bench, shelf, bin, drawer, cabinet).
//! Each folder contains an `instance.toml` defining what type of container it is.
//! Tools sit inside their current container's folder as `.tool.toml` files.
//! When GPS detects a tool has moved, `StorageManager` moves the file automatically.
//!
//! ```
//! my-workshop/
//! ├── .workshop/
//! │   └── workshop.toml              ← Workshop metadata (name, MQTT broker, origin GPS)
//! ├── tools/
//! │   ├── instance.toml              ← Root zone: "Workshop Floor"
//! │   ├── bench-3/
//! │   │   ├── instance.toml          ← StorageKind::Bench
//! │   │   ├── torque-wrench-3_8.tool.toml
//! │   │   ├── right-shelf/
//! │   │   │   ├── instance.toml      ← StorageKind::Shelf
//! │   │   │   └── caliper-150mm.tool.toml
//! │   │   └── left-drawer/
//! │   │       ├── instance.toml      ← StorageKind::Drawer
//! │   │       └── 5mm-drill-bit.tool.toml
//! │   ├── tool-cabinet-a/
//! │   │   ├── instance.toml          ← StorageKind::Cabinet
//! │   │   ├── bin-1/
//! │   │   │   ├── instance.toml      ← StorageKind::Bin
//! │   │   │   └── hex-bolt-m6.tool.toml
//! │   │   └── top-drawer/
//! │   │       ├── instance.toml      ← StorageKind::Drawer
//! │   │       └── combination-wrench-set.tool.toml
//! │   ├── cnc-bay/
//! │   │   ├── instance.toml          ← StorageKind::Zone
//! │   │   └── shopbot-cnc-router.tool.toml
//! │   └── milwaukee-m18-drill.tool.toml  ← Currently on the workshop floor (root)
//! ├── movement_log.toml              ← Append-only audit trail of all tool movements
//! └── guides/
//!     └── aluminium-bracket-assembly.guide.toml
//! ```
//!
//! ## Design Principles
//!
//! - **TOML files are the database** — every tool is a git-diffable text file
//! - **In-memory index** — `ToolIndex` is rebuilt at startup from TOML files via Rayon
//! - **File watching** — `notify` hot-reloads changes into the index without restart
//! - **AI-writable** — the AI can create, edit, and search `.tool.toml` files directly
//! - **Mesh-as-spec** — the same TOML that defines tool properties also drives the 3D entity

pub mod guide;
pub mod knowledge;
pub mod procurement;
pub mod registry;
pub mod status;
pub mod storage;

#[cfg(feature = "bevy-twin")]
pub mod twin;

/// Re-export the most commonly used types at crate root
pub use guide::{BuildGuide, BuildStep, StepRequirement};
pub use knowledge::ToolKnowledge;
pub use procurement::{MissingItem, PurchaseList};
pub use registry::{RegisteredTool, ToolCapability, ToolCategory, ToolIndex, ToolRegistry};
pub use status::{
    CubePacket, IoTTelemetry, KineticChipState, KineticPhase, KineticThresholds,
    LiveStatusStore, OperationalState, ToolLocation, ToolStatus,
};
pub use storage::{
    BoundingVolume, ContainerIndex, ContainerSummary, MovementTrigger, StorageKind,
    StorageManager, StoragePath, StorageUnit, ToolMovementRecord,
};
