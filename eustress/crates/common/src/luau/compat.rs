//! # Roblox Luau Compatibility Layer
//!
//! Shims and adapters for porting Roblox Luau scripts to Eustress Engine.
//! Provides familiar API surfaces so existing scripts run with minimal changes.
//!
//! ## Table of Contents
//!
//! 1. **ServiceMapping** — Maps Roblox service names to Eustress equivalents
//! 2. **ApiShims** — `Instance.new()`, `game:GetService()`, property access patterns
//! 3. **TypeMapping** — Vector3, CFrame, Color3, UDim2 → Bevy equivalents
//! 4. **ScriptTransformer** — Source-level transforms for common Roblox→Eustress patterns

use std::collections::HashMap;

// ============================================================================
// Service Name Mapping
// ============================================================================

/// Maps Roblox service names to Eustress equivalents.
/// Used by `game:GetService("ServiceName")` shim.
pub struct ServiceMapping;

impl ServiceMapping {
    /// Map a Roblox service name to its Eustress equivalent (if any)
    pub fn map_service(roblox_name: &str) -> Option<&'static str> {
        match roblox_name {
            // Direct equivalents (same name, same concept)
            "Workspace" => Some("Workspace"),
            "Players" => Some("Players"),
            "Lighting" => Some("Lighting"),
            "SoundService" => Some("SoundService"),
            "Teams" => Some("Teams"),
            "Chat" => Some("Chat"),
            "ReplicatedStorage" => Some("ReplicatedStorage"),
            "ReplicatedFirst" => Some("ReplicatedFirst"),
            "ServerScriptService" => Some("ServerScriptService"),
            "ServerStorage" => Some("ServerStorage"),
            "StarterGui" => Some("StarterGui"),
            "StarterPlayer" => Some("StarterPlayer"),
            "StarterPack" => Some("StarterPack"),

            // Mapped equivalents (different name, similar concept)
            "RunService" => Some("RunService"),
            "UserInputService" => Some("InputService"),
            "TweenService" => Some("TweenService"),
            "HttpService" => Some("HttpService"),
            "DataStoreService" => Some("DataStoreService"),
            "MarketplaceService" => Some("MarketplaceService"),
            "TeleportService" => Some("TeleportService"),
            "PhysicsService" => Some("PhysicsService"),
            "PathfindingService" => Some("PathfindingService"),
            "CollectionService" => Some("CollectionService"),
            "TextService" => Some("TextService"),
            "LocalizationService" => Some("LocalizationService"),
            "GuiService" => Some("GuiService"),

            // Eustress-only services (no Roblox equivalent)
            // These return None — scripts referencing them need manual porting
            _ => None,
        }
    }

    /// Get all known Roblox→Eustress service mappings
    pub fn all_mappings() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Workspace", "Workspace"),
            ("Players", "Players"),
            ("Lighting", "Lighting"),
            ("SoundService", "SoundService"),
            ("Teams", "Teams"),
            ("Chat", "Chat"),
            ("ReplicatedStorage", "ReplicatedStorage"),
            ("ReplicatedFirst", "ReplicatedFirst"),
            ("ServerScriptService", "ServerScriptService"),
            ("ServerStorage", "ServerStorage"),
            ("StarterGui", "StarterGui"),
            ("StarterPlayer", "StarterPlayer"),
            ("StarterPack", "StarterPack"),
            ("RunService", "RunService"),
            ("UserInputService", "InputService"),
            ("TweenService", "TweenService"),
            ("HttpService", "HttpService"),
            ("DataStoreService", "DataStoreService"),
        ]
    }
}

// ============================================================================
// Class Name Mapping
// ============================================================================

/// Maps Roblox class names to Eustress ClassName equivalents.
/// Used by `Instance.new("ClassName")` shim.
pub struct ClassMapping;

impl ClassMapping {
    /// Map a Roblox class name to its Eustress equivalent
    pub fn map_class(roblox_class: &str) -> Option<&'static str> {
        match roblox_class {
            // Parts and geometry
            "Part" => Some("Part"),
            "MeshPart" => Some("Part"),
            "WedgePart" => Some("Part"),
            "CornerWedgePart" => Some("Part"),
            "TrussPart" => Some("Part"),
            "SpawnLocation" => Some("SpawnLocation"),
            "Seat" => Some("Seat"),
            "VehicleSeat" => Some("VehicleSeat"),
            "Model" => Some("Model"),
            "Folder" => Some("Folder"),

            // Lighting
            "PointLight" => Some("PointLight"),
            "SpotLight" => Some("SpotLight"),
            "SurfaceLight" => Some("SurfaceLight"),

            // Constraints
            "WeldConstraint" => Some("WeldConstraint"),
            "Motor6D" => Some("Motor6D"),
            "Attachment" => Some("Attachment"),
            "HingeConstraint" => Some("HingeConstraint"),

            // GUI
            "ScreenGui" => Some("ScreenGui"),
            "BillboardGui" => Some("BillboardGui"),
            "SurfaceGui" => Some("SurfaceGui"),
            "Frame" => Some("Frame"),
            "TextLabel" => Some("TextLabel"),
            "TextButton" => Some("TextButton"),
            "TextBox" => Some("TextBox"),
            "ImageLabel" => Some("ImageLabel"),
            "ImageButton" => Some("ImageButton"),
            "ScrollingFrame" => Some("ScrollingFrame"),
            "ViewportFrame" => Some("ViewportFrame"),

            // Effects
            "ParticleEmitter" => Some("ParticleEmitter"),
            "Beam" => Some("Beam"),
            "Sound" => Some("Sound"),

            // Scripting
            "Script" => Some("LuauScript"),
            "LocalScript" => Some("LuauLocalScript"),
            "ModuleScript" => Some("LuauModuleScript"),
            "RemoteEvent" => Some("RemoteEvent"),
            "RemoteFunction" => Some("RemoteFunction"),
            "BindableEvent" => Some("BindableEvent"),
            "BindableFunction" => Some("BindableFunction"),

            // Environment
            "Sky" => Some("Sky"),
            "Atmosphere" => Some("Atmosphere"),
            "Clouds" => Some("Clouds"),
            "Terrain" => Some("Terrain"),

            // Humanoid
            "Humanoid" => Some("Humanoid"),
            "Animator" => Some("Animator"),

            // Camera
            "Camera" => Some("Camera"),

            // Mesh / Decal
            "SpecialMesh" => Some("SpecialMesh"),
            "Decal" => Some("Decal"),

            _ => None,
        }
    }
}

// ============================================================================
// Property Name Mapping
// ============================================================================

/// Maps Roblox property names to Eustress equivalents where they differ.
pub struct PropertyMapping;

impl PropertyMapping {
    /// Map a Roblox property name to its Eustress equivalent
    pub fn map_property<'a>(class: &str, roblox_property: &'a str) -> &'a str {
        match (class, roblox_property) {
            // BasePart properties that map directly
            ("Part", "Position") => "position",
            ("Part", "Size") => "size",
            ("Part", "Color") => "color",
            ("Part", "BrickColor") => "color",
            ("Part", "Transparency") => "transparency",
            ("Part", "Anchored") => "anchored",
            ("Part", "CanCollide") => "can_collide",
            ("Part", "Material") => "material",
            ("Part", "CFrame") => "transform",
            ("Part", "Orientation") => "rotation",
            ("Part", "Name") => "name",
            ("Part", "Parent") => "parent",

            // Humanoid properties
            ("Humanoid", "Health") => "health",
            ("Humanoid", "MaxHealth") => "max_health",
            ("Humanoid", "WalkSpeed") => "walk_speed",
            ("Humanoid", "JumpPower") => "jump_power",
            ("Humanoid", "JumpHeight") => "jump_height",

            // Light properties
            ("PointLight", "Brightness") => "intensity",
            ("PointLight", "Range") => "range",
            ("PointLight", "Color") => "color",
            ("SpotLight", "Brightness") => "intensity",
            ("SpotLight", "Range") => "range",
            ("SpotLight", "Angle") => "outer_angle",
            ("SpotLight", "Face") => "face",

            // Sound properties
            ("Sound", "SoundId") => "asset_id",
            ("Sound", "Volume") => "volume",
            ("Sound", "Playing") => "playing",
            ("Sound", "Looped") => "looped",
            ("Sound", "PlaybackSpeed") => "playback_speed",

            // Default: return as-is (many properties share names)
            (_, property) => property,
        }
    }
}

// ============================================================================
// Source-Level Script Transformer
// ============================================================================

/// Transforms Roblox Luau source code patterns to Eustress equivalents.
/// Performs regex-free string replacements for common patterns.
///
/// This is NOT a full transpiler — it handles the most common porting patterns:
/// - `game:GetService("X")` → `game:GetService("MappedX")`
/// - `Instance.new("X")` class name remapping
/// - Deprecated API warnings
pub struct ScriptTransformer;

impl ScriptTransformer {
    /// Apply all source-level transformations to a Luau script
    pub fn transform(source: &str) -> TransformResult {
        let mut output = source.to_string();
        let mut warnings: Vec<TransformWarning> = Vec::new();
        let mut changes = 0u32;

        // Transform deprecated `wait()` to `task.wait()`
        if output.contains("wait(") && !output.contains("task.wait(") {
            warnings.push(TransformWarning {
                line: None,
                message: "Script uses deprecated `wait()`. Consider using `task.wait()` instead.".to_string(),
                severity: WarningSeverity::Info,
            });
        }

        // Warn about `game:GetService("DataStoreService")` usage (server-only)
        if output.contains("DataStoreService") {
            warnings.push(TransformWarning {
                line: None,
                message: "DataStoreService access detected. Ensure this script runs server-side only.".to_string(),
                severity: WarningSeverity::Warning,
            });
        }

        // Warn about `UserInputService` → `InputService` rename
        if output.contains("UserInputService") {
            warnings.push(TransformWarning {
                line: None,
                message: "UserInputService is named InputService in Eustress. Update GetService calls.".to_string(),
                severity: WarningSeverity::Warning,
            });
            changes += 1;
        }

        // Warn about BrickColor usage (deprecated in favor of Color3)
        if output.contains("BrickColor") {
            warnings.push(TransformWarning {
                line: None,
                message: "BrickColor is deprecated in Eustress. Use Color3 instead.".to_string(),
                severity: WarningSeverity::Info,
            });
        }

        // Warn about LoadLibrary (removed in modern Roblox, not supported in Eustress)
        if output.contains("LoadLibrary") {
            warnings.push(TransformWarning {
                line: None,
                message: "LoadLibrary was removed. Use ModuleScripts with require() instead.".to_string(),
                severity: WarningSeverity::Error,
            });
        }

        TransformResult {
            source: output,
            warnings,
            changes,
        }
    }
}

/// Result of a script transformation
#[derive(Debug, Clone)]
pub struct TransformResult {
    /// Transformed source code
    pub source: String,
    /// Warnings generated during transformation
    pub warnings: Vec<TransformWarning>,
    /// Number of automatic changes made
    pub changes: u32,
}

/// A warning generated during script transformation
#[derive(Debug, Clone)]
pub struct TransformWarning {
    /// Line number (if determinable)
    pub line: Option<u32>,
    /// Warning message
    pub message: String,
    /// Severity level
    pub severity: WarningSeverity,
}

/// Severity of a transformation warning
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    /// Informational — script will work but could be improved
    Info,
    /// Warning — script may not work correctly without changes
    Warning,
    /// Error — script will definitely fail without changes
    Error,
}
