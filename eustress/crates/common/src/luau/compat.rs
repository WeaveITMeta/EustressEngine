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
            // EditableMesh is a runtime AssetService construct; we map it to
            // Part (per the Wave 6 decision — no dedicated EditableMesh class).
            "EditableMesh" => Some("Part"),
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
            // Modern constraints & movers (Wave 6.B)
            "RodConstraint" => Some("RodConstraint"),
            "CylindricalConstraint" => Some("CylindricalConstraint"),
            "TorsionSpringConstraint" => Some("TorsionSpringConstraint"),
            "UniversalConstraint" => Some("UniversalConstraint"),
            "AlignPosition" => Some("AlignPosition"),
            "AlignOrientation" => Some("AlignOrientation"),
            "LinearVelocity" => Some("LinearVelocity"),
            "AngularVelocity" => Some("AngularVelocity"),
            "VectorForce" => Some("VectorForce"),
            "Torque" => Some("Torque"),
            // Roblox class is "Plane"; Eustress variant is PlaneConstraint
            "Plane" => Some("PlaneConstraint"),
            // Legacy body movers (deprecated in Roblox, still round-tripped)
            "BodyPosition" => Some("BodyPosition"),
            "BodyVelocity" => Some("BodyVelocity"),
            "BodyGyro" => Some("BodyGyro"),
            "BodyAngularVelocity" => Some("BodyAngularVelocity"),
            "BodyForce" => Some("BodyForce"),
            "BodyThrust" => Some("BodyThrust"),

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
            // Post-processing & VFX (Wave 6.C)
            "BloomEffect" => Some("BloomEffect"),
            "BlurEffect" => Some("BlurEffect"),
            "DepthOfFieldEffect" => Some("DepthOfFieldEffect"),
            "ColorCorrectionEffect" => Some("ColorCorrectionEffect"),
            "SunRaysEffect" => Some("SunRaysEffect"),
            "Fire" => Some("Fire"),
            "Smoke" => Some("Smoke"),
            "Sparkles" => Some("Sparkles"),
            "Explosion" => Some("Explosion"),
            "Trail" => Some("Trail"),
            "ForceField" => Some("ForceField"),

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

            // Interaction & character (Wave 6.D)
            "Tool" => Some("Tool"),
            "Accessory" => Some("Accessory"),
            // Legacy Hat is an Accessory in modern Roblox
            "Hat" => Some("Accessory"),
            "ClickDetector" => Some("ClickDetector"),
            "ProximityPrompt" => Some("ProximityPrompt"),
            "Dialog" => Some("Dialog"),
            "DialogChoice" => Some("DialogChoice"),
            "BodyColors" => Some("BodyColors"),
            "CharacterMesh" => Some("CharacterMesh"),
            "Shirt" => Some("Shirt"),
            "Pants" => Some("Pants"),
            "ShirtGraphic" => Some("ShirtGraphic"),

            // Camera
            "Camera" => Some("Camera"),

            // Mesh / Decal
            "SpecialMesh" => Some("SpecialMesh"),
            "Decal" => Some("Decal"),

            // ValueObjects (Wave 6.A) — 1:1 name parity with Roblox
            "StringValue" => Some("StringValue"),
            "IntValue" => Some("IntValue"),
            "NumberValue" => Some("NumberValue"),
            "BoolValue" => Some("BoolValue"),
            "ObjectValue" => Some("ObjectValue"),
            "Color3Value" => Some("Color3Value"),
            "Vector3Value" => Some("Vector3Value"),
            "CFrameValue" => Some("CFrameValue"),
            "BrickColorValue" => Some("BrickColorValue"),
            "RayValue" => Some("RayValue"),
            "BinaryStringValue" => Some("BinaryStringValue"),
            // ── Wave 7.A CSG (collapse to Part; geometry via 4.A.2 baked mesh) ──
            "IntersectOperation" => Some("Part"),
            "NegateOperation" => Some("Part"),
            "PartOperation" => Some("Part"),
            "PartOperationAsset" => Some("Part"),
            // ── Wave 7.A legacy joints/movers ──
            "Weld" => Some("Weld"),
            "Motor" => Some("Motor"),
            "VelocityMotor" => Some("VelocityMotor"),
            "NoCollisionConstraint" => Some("NoCollisionConstraint"),
            "RigidConstraint" => Some("RigidConstraint"),
            "LineForce" => Some("LineForce"),
            "AnimationConstraint" => Some("AnimationConstraint"),
            // ── Wave 7.B UI layout modifiers ──
            "UICorner" => Some("UICorner"),
            "UIGradient" => Some("UIGradient"),
            "UIStroke" => Some("UIStroke"),
            "UIListLayout" => Some("UIListLayout"),
            "UIGridLayout" => Some("UIGridLayout"),
            "UIPadding" => Some("UIPadding"),
            "UIAspectRatioConstraint" => Some("UIAspectRatioConstraint"),
            "UIScale" => Some("UIScale"),
            "UISizeConstraint" => Some("UISizeConstraint"),
            "UITextSizeConstraint" => Some("UITextSizeConstraint"),
            "UITableLayout" => Some("UITableLayout"),
            "UIPageLayout" => Some("UIPageLayout"),
            "UIFlexItem" => Some("UIFlexItem"),
            "CanvasGroup" => Some("CanvasGroup"),
            "UIDragDetector" => Some("UIDragDetector"),
            // ── Wave 7.C meshes / surfaces / visual adornments ──
            "BlockMesh" => Some("BlockMesh"),
            "FileMesh" => Some("FileMesh"),
            "Texture" => Some("Texture"),
            "SurfaceAppearance" => Some("SurfaceAppearance"),
            "MaterialVariant" => Some("MaterialVariant"),
            "Highlight" => Some("Highlight"),
            "Bone" => Some("Bone"),
            "WrapDeformer" => Some("WrapDeformer"),
            "WrapLayer" => Some("WrapLayer"),
            "WrapTarget" => Some("WrapTarget"),

            // Wave 7.D character / players / animation
            "Animation" => Some("Animation"),
            "AnimationController" => Some("AnimationController"),
            "HumanoidController" => Some("HumanoidController"),
            "ControllerManager" => Some("ControllerManager"),
            "AirController" => Some("AirController"),
            "ClimbController" => Some("ClimbController"),
            "GroundController" => Some("GroundController"),
            "SwimController" => Some("SwimController"),
            "SkateboardController" => Some("SkateboardController"),
            "VehicleController" => Some("VehicleController"),
            "ControllerPartSensor" => Some("ControllerPartSensor"),
            "HumanoidDescription" => Some("HumanoidDescription"),
            "BodyPartDescription" => Some("BodyPartDescription"),
            "Backpack" => Some("Backpack"),
            "StarterGear" => Some("StarterGear"),
            "Accoutrement" => Some("Accoutrement"),
            "AccessoryDescription" => Some("AccessoryDescription"),
            "FaceControls" => Some("FaceControls"),
            "IKControl" => Some("IKControl"),
            "KeyframeMarker" => Some("KeyframeMarker"),
            "Pose" => Some("Pose"),
            "NumberPose" => Some("NumberPose"),
            "CurveAnimation" => Some("CurveAnimation"),
            "AnimationRigData" => Some("AnimationRigData"),

            // Wave 7.E audio DSP effects + routing
            "AudioReverb" => Some("AudioReverb"),
            "AudioEcho" => Some("AudioEcho"),
            "AudioDistortion" => Some("AudioDistortion"),
            "AudioEqualizer" => Some("AudioEqualizer"),
            "AudioCompressor" => Some("AudioCompressor"),
            "AudioChorus" => Some("AudioChorus"),
            "AudioFlanger" => Some("AudioFlanger"),
            "AudioFader" => Some("AudioFader"),
            "AudioFilter" => Some("AudioFilter"),
            "AudioPitchShifter" => Some("AudioPitchShifter"),
            "AudioEmitter" => Some("AudioEmitter"),
            "AudioListener" => Some("AudioListener"),
            "AudioPlayer" => Some("AudioPlayer"),
            "AudioDeviceInput" => Some("AudioDeviceInput"),
            "AudioDeviceOutput" => Some("AudioDeviceOutput"),
            "AudioAnalyzer" => Some("AudioAnalyzer"),
            "AudioSearchParams" => Some("AudioSearchParams"),
            "ReverbSoundEffect" => Some("ReverbSoundEffect"),
            "EchoSoundEffect" => Some("EchoSoundEffect"),
            "DistortionSoundEffect" => Some("DistortionSoundEffect"),
            "EqualizerSoundEffect" => Some("EqualizerSoundEffect"),
            "CompressorSoundEffect" => Some("CompressorSoundEffect"),
            "ChorusSoundEffect" => Some("ChorusSoundEffect"),
            "FlangeSoundEffect" => Some("FlangeSoundEffect"),
            "PitchShiftSoundEffect" => Some("PitchShiftSoundEffect"),
            "TremoloSoundEffect" => Some("TremoloSoundEffect"),

            // Wave 7.F data structs / curves / misc
            "DataStoreGetOptions" => Some("DataStoreGetOptions"),
            "DataStoreSetOptions" => Some("DataStoreSetOptions"),
            "DataStoreIncrementOptions" => Some("DataStoreIncrementOptions"),
            "DataStoreOptions" => Some("DataStoreOptions"),
            "FloatCurve" => Some("FloatCurve"),
            "RotationCurve" => Some("RotationCurve"),
            "EulerRotationCurve" => Some("EulerRotationCurve"),
            "Vector3Curve" => Some("Vector3Curve"),
            "MarkerCurve" => Some("MarkerCurve"),
            "Path2D" => Some("Path2D"),
            "LocalizationTable" => Some("LocalizationTable"),
            "Configuration" => Some("Configuration"),
            "Noise" => Some("Noise"),
            "UnreliableRemoteEvent" => Some("UnreliableRemoteEvent"),
            "Wire" => Some("Wire"),
            "OperationGraph" => Some("OperationGraph"),

            // Wave 7.G editable / sensors / chat
            "EditableImage" => Some("EditableImage"),
            "RobloxEditableImage" => Some("RobloxEditableImage"),
            // Roblox editable meshes are imported as static Parts (editable-Parts decision).
            // (Bare "EditableMesh" is already mapped to Part in the Parts/geometry block above.)
            "RobloxEditableMesh" => Some("Part"),
            "BuoyancySensor" => Some("BuoyancySensor"),
            "DragDetector" => Some("DragDetector"),
            "TextChannel" => Some("TextChannel"),
            "TextChatCommand" => Some("TextChatCommand"),
            "TextChatMessageProperties" => Some("TextChatMessageProperties"),
            "HapticEffect" => Some("HapticEffect"),

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
