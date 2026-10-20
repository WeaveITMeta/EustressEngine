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
            // Legacy surface joints. Studio's join tool and every pre-2016
            // build emit these instead of the modern constraints, and they are
            // STRUCTURAL — dropping them lets an imported assembly fall apart
            // the moment physics runs. `ManualWeld`/`Snap`/`Glue` are all rigid
            // welds carrying the same Part0/Part1 + C0/C1 payload as `Weld`;
            // the `Rotate*` family are hinges, motor-driven in the P/V variants.
            "ManualWeld" => Some("Weld"),
            "Snap" => Some("Weld"),
            "Glue" => Some("Weld"),
            "Rotate" => Some("HingeConstraint"),
            "RotateP" => Some("Motor"),
            "RotateV" => Some("VelocityMotor"),
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

            // ── Wave 7 final-9: each maps to its OWN class (no lossy collapse) ──
            // Actor is a Model subclass but its identity is the parallel-Luau
            // execution boundary (task.desynchronize) — NOT a plain Model.
            "Actor" => Some("Actor"),
            // WorldModel is a distinct physics-isolated model container.
            "WorldModel" => Some("WorldModel"),
            // ColorGradingEffect is its own post-FX (adds a tonemapper) —
            // distinct property set from ColorCorrectionEffect.
            "ColorGradingEffect" => Some("ColorGradingEffect"),
            // TerrainDetail / TerrainRegion are child/data objects of Terrain,
            // not Terrain itself — own classes so they don't spawn bogus terrain.
            "TerrainDetail" => Some("TerrainDetail"),
            "TerrainRegion" => Some("TerrainRegion"),
            // Team already has a ClassName variant; wire its compat arm.
            "Team" => Some("Team"),

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

/// Context describing which Roblox `ValueObject`s the importer folded into
/// attributes on their parent instance. Drives [`ScriptTransformer::transform_value_objects`],
/// which rewrites `.Value` reads/writes/observers on those names into the
/// attribute APIs (`GetAttribute`/`SetAttribute`/`GetAttributeChangedSignal`).
///
/// The importer (`roblox-import`) constructs this while materializing a model:
/// every `NumberValue`/`StringValue`/`IntValue`/`BoolValue`/`ObjectValue`/… child
/// is removed and its current `.Value` is stored as an attribute on the parent,
/// keyed by the value-object's `Name`. That `Name` goes into [`names`](Self::names).
/// The subset that were `ObjectValue` (an instance reference, stored as a UUID
/// string attribute) additionally go into [`ref_names`](Self::ref_names) so reads
/// can be wrapped in the `FindByUUID` resolver. The subset that were
/// `IntConstrainedValue` or `DoubleConstrainedValue` go into
/// [`range_names`](Self::range_names): their range was folded beside the value
/// as `<Name>_MinValue` and `<Name>_MaxValue`.
#[derive(Debug, Clone, Default)]
pub struct ValueObjectContext {
    pub names: std::collections::HashSet<String>,     // all converted value-object Names
    pub ref_names: std::collections::HashSet<String>, // subset that were ObjectValue
    pub range_names: std::collections::HashSet<String>, // subset that were *ConstrainedValue
}

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

    /// Apply all standard transforms PLUS a value-object→attribute rewrite pass.
    ///
    /// This is the entry point the importer (`roblox-import` / `instance_loader`)
    /// calls for scripts whose sibling `ValueObject`s were folded into attributes.
    /// It first runs every pass from [`transform`](Self::transform) (so the
    /// returned source is a superset of the legacy behaviour), then rewrites
    /// `.Value` reads/writes/observers for each `Name` in `vo` per CONTRACT D:
    ///
    /// - `X.Name.Value`            (read)       → `X:GetAttribute("Name")`
    /// - `X.Name.Value = V`        (assignment) → `X:SetAttribute("Name", V)`
    /// - `X:FindFirstChild("Name").Value`       → `X:GetAttribute("Name")`
    /// - `X:WaitForChild("Name").Value`         → `X:GetAttribute("Name")`
    /// - `X.Name.Changed:Connect(F)`            → `X:GetAttributeChangedSignal("Name"):Connect(F)`
    /// - `X.Name:GetPropertyChangedSignal("Value"):Connect(F)`
    ///                                          → `X:GetAttributeChangedSignal("Name"):Connect(F)`
    /// - for a constrained value (`vo.range_names`): `X.Name.MinValue` and
    ///   `X.Name.MaxValue` → `X:GetAttribute("Name_MinValue")` and
    ///   `X:GetAttribute("Name_MaxValue")`, assignments → `SetAttribute`, and
    ///   `X.Name.ConstrainedValue` like `X.Name.Value`
    ///
    /// For names that were `ObjectValue` (`vo.ref_names`), a value READ is
    /// additionally wrapped in the runtime resolver
    /// (`FindByUUID(X:GetAttribute("Name"))`) and an assignment stores the
    /// referent's UUID (`X:SetAttribute("Name", inst and inst:GetUuid() or "")`,
    /// recording a warning since storing a live ref as a UUID is lossy).
    ///
    /// Patterns that cannot be rewritten safely with string substitution
    /// (a value-object captured into a local, `Instance.new("NumberValue")`
    /// created at runtime, or a value-object passed as a function argument)
    /// are NOT rewritten — instead a [`WarningSeverity::Warning`] is recorded
    /// so the porter can fix them by hand.
    ///
    /// The existing [`transform`](Self::transform) method is intentionally left
    /// unchanged for back-compat; this method delegates to it.
    pub fn transform_value_objects(source: &str, vo: &ValueObjectContext) -> TransformResult {
        // Run the standard passes first; build on their result + warnings.
        let mut result = Self::transform(source);

        // Nothing folded → standard transform is the whole story.
        if vo.names.is_empty() {
            return result;
        }

        let (rewritten, vo_changes, vo_warnings) =
            Self::rewrite_value_object_access(&result.source, vo);

        result.source = rewritten;
        result.changes += vo_changes;
        result.warnings.extend(vo_warnings);
        result
    }

    /// Core value-object rewrite pass (CONTRACT D). String based, like the
    /// rest of this transformer; whole-line comments are skipped.
    ///
    /// Every rewrite works on the SUFFIX that reaches the value object
    /// (`.Name.Value`, `:WaitForChild("Name").Value`, ...), so the receiver
    /// may be any expression, a call chain included:
    /// `A:WaitForChild("B"):WaitForChild("Name").Value` becomes
    /// `A:WaitForChild("B"):GetAttribute("Name")`. The receiver is needed only
    /// to read the value again for a compound assignment and to wrap an
    /// ObjectValue read in `FindByUUID`; a receiver that is not a plain
    /// dotted name is not duplicated, and those two forms get a warning.
    ///
    /// The context holds every value object's name in the place (thousands);
    /// each line tries only the names it mentions, in sorted order, so the
    /// output does not depend on hash order.
    ///
    /// Returns the rewritten source, the number of substitutions made (each
    /// counts toward `TransformResult.changes`), and any warnings raised for
    /// constructs that could not be rewritten.
    fn rewrite_value_object_access(
        source: &str,
        vo: &ValueObjectContext,
    ) -> (String, u32, Vec<TransformWarning>) {
        let mut changes = 0u32;
        let mut warnings: Vec<TransformWarning> = Vec::new();
        let mut out_lines: Vec<String> = Vec::with_capacity(source.lines().count());

        for (idx, raw_line) in source.lines().enumerate() {
            let line_no = (idx as u32) + 1;

            // Whole-line comment → never rewrite; pass through verbatim.
            if raw_line.trim_start().starts_with("--") {
                out_lines.push(raw_line.to_string());
                continue;
            }
            let names = Self::names_on_line(raw_line, vo);
            if names.is_empty() {
                out_lines.push(raw_line.to_string());
                continue;
            }

            let mut line = raw_line.to_string();
            for name in names {
                let is_ref = vo.ref_names.contains(name);

                // A value object captured into a local (`local v = obj.Name`)
                // is later used as `v.Value`, which cannot be traced here.
                if Self::has_unsafe_local_capture(&line, name) {
                    warnings.push(TransformWarning {
                        line: Some(line_no),
                        message: format!(
                            "Value-object '{name}' appears to be captured into a local \
                             (e.g. `local v = obj.{name}`); its later `.Value` use cannot be \
                             rewritten automatically. Replace with `obj:GetAttribute(\"{name}\")`.",
                        ),
                        severity: WarningSeverity::Warning,
                    });
                }

                // Observers: `.Name.Changed` and
                // `.Name:GetPropertyChangedSignal("Value")` →
                // `:GetAttributeChangedSignal("Name")`.
                let signal = format!(":GetAttributeChangedSignal(\"{name}\")");
                changes += Self::replace_suffix(&mut line, &format!(".{name}.Changed"), &signal);
                for q in ['"', '\''] {
                    changes += Self::replace_suffix(
                        &mut line,
                        &format!(".{name}:GetPropertyChangedSignal({q}Value{q})"),
                        &signal,
                    );
                }

                // The value itself, through a member or a child lookup.
                let mut markers = vec![format!(".{name}.Value")];
                for method in ["FindFirstChild", "WaitForChild"] {
                    for q in ['"', '\''] {
                        markers.push(format!(":{method}({q}{name}{q}).Value"));
                    }
                }
                for marker in &markers {
                    changes += Self::rewrite_value_access(
                        &mut line,
                        marker,
                        name,
                        is_ref,
                        &mut warnings,
                        line_no,
                    );
                }

                // A constrained value's range was folded beside it, and
                // `ConstrainedValue` is another name for its value.
                if vo.range_names.contains(name) {
                    for (member, attribute) in [
                        ("ConstrainedValue", name.to_string()),
                        ("MinValue", format!("{name}_MinValue")),
                        ("MaxValue", format!("{name}_MaxValue")),
                    ] {
                        let mut markers = vec![format!(".{name}.{member}")];
                        for method in ["FindFirstChild", "WaitForChild"] {
                            for q in ['"', '\''] {
                                markers.push(format!(":{method}({q}{name}{q}).{member}"));
                            }
                        }
                        for marker in &markers {
                            changes += Self::rewrite_value_access(
                                &mut line,
                                marker,
                                &attribute,
                                false,
                                &mut warnings,
                                line_no,
                            );
                        }
                    }
                }
            }
            out_lines.push(line);
        }

        // Preserve a trailing newline if the original had one (lines() drops it).
        let mut rewritten = out_lines.join("\n");
        if source.ends_with('\n') {
            rewritten.push('\n');
        }

        // Runtime-created value objects are a whole-source concern.
        Self::warn_runtime_value_objects(source, &mut warnings);

        (rewritten, changes, warnings)
    }

    /// The folded value-object names a line mentions, in sorted order: its
    /// identifiers, and the contents of its string literals (a name reached
    /// through `FindFirstChild("Car Spawn Delay")` may hold spaces).
    fn names_on_line<'a>(line: &str, vo: &'a ValueObjectContext) -> Vec<&'a String> {
        let mut found: std::collections::BTreeSet<&'a String> = std::collections::BTreeSet::new();
        for token in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if let Some(name) = vo.names.get(token) {
                found.insert(name);
            }
        }
        for q in ['"', '\''] {
            let mut rest = line;
            while let Some(open) = rest.find(q) {
                let after = &rest[open + 1..];
                let Some(close) = after.find(q) else { break };
                if let Some(name) = vo.names.get(&after[..close]) {
                    found.insert(name);
                }
                rest = &after[close + 1..];
            }
        }
        found.into_iter().collect()
    }

    /// Replace every occurrence of `marker` that is not followed by an
    /// identifier character (so `.Name.ChangedX` is left alone) with
    /// `replacement`. Returns the number of replacements.
    fn replace_suffix(line: &mut String, marker: &str, replacement: &str) -> u32 {
        let mut changes = 0u32;
        let mut search_from = 0usize;
        while let Some(rel) = line[search_from..].find(marker) {
            let start = search_from + rel;
            let end = start + marker.len();
            if Self::continues_identifier(line, end) {
                search_from = end;
                continue;
            }
            line.replace_range(start..end, replacement);
            changes += 1;
            search_from = start + replacement.len();
        }
        changes
    }

    /// Whether the character at byte `at` continues an identifier.
    fn continues_identifier(line: &str, at: usize) -> bool {
        line[at..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    /// Rewrite every use of one value marker (`.Name.Value`,
    /// `:FindFirstChild("Name").Value`, ...) on a line:
    ///
    /// - `<m> = V`   → `:SetAttribute("Name", V)`
    /// - `<m> += V`  → `:SetAttribute("Name", <recv>:GetAttribute("Name") + (V))`
    ///   (every compound operator; needs a plain dotted receiver)
    /// - `<m>`       → `:GetAttribute("Name")`, or for an ObjectValue
    ///   `FindByUUID(<recv>:GetAttribute("Name"))` (needs a plain receiver)
    ///
    /// An assigned value ends at a `;` or a `--` comment outside brackets and
    /// strings, which stay after the rewrite; a value that continues on the
    /// next line is not rewritten and gets a warning. For an ObjectValue the
    /// stored value is the referent's UUID (a warning notes the loss).
    fn rewrite_value_access(
        line: &mut String,
        marker: &str,
        name: &str,
        is_ref: bool,
        warnings: &mut Vec<TransformWarning>,
        line_no: u32,
    ) -> u32 {
        const COMPOUND: [&str; 8] = ["//=", "..=", "+=", "-=", "*=", "/=", "%=", "^="];
        let mut changes = 0u32;
        let mut search_from = 0usize;
        while let Some(rel) = line[search_from..].find(marker) {
            let m_start = search_from + rel;
            let m_end = m_start + marker.len();
            // `.Name.ValueMap` is another member.
            if Self::continues_identifier(line, m_end) {
                search_from = m_end;
                continue;
            }
            let after = &line[m_end..];
            let trimmed = after.trim_start();
            let op_at = m_end + (after.len() - trimmed.len());
            let compound = COMPOUND.iter().copied().find(|op| trimmed.starts_with(op));
            let plain = compound.is_none() && trimmed.starts_with('=') && !trimmed.starts_with("==");

            if plain || compound.is_some() {
                let op_len = compound.map_or(1, str::len);
                let rhs_start = op_at + op_len;
                let Some(rhs_end) = Self::expression_end(line, rhs_start) else {
                    warnings.push(TransformWarning {
                        line: Some(line_no),
                        message: format!(
                            "The value assigned to value-object '{name}' continues on the next \
                             line; rewrite it by hand as `obj:SetAttribute(\"{name}\", value)`.",
                        ),
                        severity: WarningSeverity::Warning,
                    });
                    search_from = m_end;
                    continue;
                };
                let rhs = line[rhs_start..rhs_end].trim().to_string();
                let value = match compound {
                    None => rhs,
                    Some(op) => {
                        let recv_start = Self::receiver_start(line, m_start);
                        if is_ref || recv_start == m_start {
                            warnings.push(TransformWarning {
                                line: Some(line_no),
                                message: format!(
                                    "Compound assignment to value-object '{name}' through a \
                                     call result can't be rewritten; write \
                                     `obj:SetAttribute(\"{name}\", obj:GetAttribute(\"{name}\") ...)`.",
                                ),
                                severity: WarningSeverity::Warning,
                            });
                            search_from = m_end;
                            continue;
                        }
                        let receiver = &line[recv_start..m_start];
                        format!(
                            "{receiver}:GetAttribute(\"{name}\") {} ({rhs})",
                            &op[..op.len() - 1]
                        )
                    }
                };
                let value = if is_ref {
                    warnings.push(TransformWarning {
                        line: Some(line_no),
                        message: format!(
                            "Assignment to ObjectValue '{name}' stores only the referent's UUID \
                             (live-instance reference is lossy); rewritten to \
                             `:SetAttribute(\"{name}\", inst and inst:GetUuid() or \"\")`. \
                             Verify the right-hand side is an Instance.",
                        ),
                        severity: WarningSeverity::Warning,
                    });
                    format!("({value}) and ({value}):GetUuid() or \"\"")
                } else {
                    value
                };
                // Keep the spacing that separated the value from what follows.
                let tail_gap = &line[rhs_start..rhs_end];
                let gap = &tail_gap[tail_gap.trim_end().len()..];
                let replacement = format!(":SetAttribute(\"{name}\", {value}){gap}");
                line.replace_range(m_start..rhs_end, &replacement);
                changes += 1;
                search_from = m_start + replacement.len();
                continue;
            }

            // A read.
            if is_ref {
                let recv_start = Self::receiver_start(line, m_start);
                if recv_start == m_start {
                    warnings.push(TransformWarning {
                        line: Some(line_no),
                        message: format!(
                            "ObjectValue '{name}' is read through a call result; wrap it by \
                             hand as `FindByUUID(obj:GetAttribute(\"{name}\"))`.",
                        ),
                        severity: WarningSeverity::Warning,
                    });
                    search_from = m_end;
                    continue;
                }
                let receiver = line[recv_start..m_start].to_string();
                let replacement = Self::read_expr(&receiver, name, true);
                line.replace_range(recv_start..m_end, &replacement);
                changes += 1;
                search_from = recv_start + replacement.len();
            } else {
                let replacement = format!(":GetAttribute(\"{name}\")");
                line.replace_range(m_start..m_end, &replacement);
                changes += 1;
                search_from = m_start + replacement.len();
            }
        }
        changes
    }

    /// Where the expression starting at byte `from` ends on this line: at a
    /// `;` or a `--` comment outside brackets and strings, else the line's
    /// end. `None` when a bracket is still open at the end of the line (the
    /// expression continues on the next one).
    fn expression_end(line: &str, from: usize) -> Option<usize> {
        let bytes = line.as_bytes();
        let mut depth = 0i32;
        let mut quote: Option<u8> = None;
        let mut i = from;
        while i < bytes.len() {
            let c = bytes[i];
            if let Some(q) = quote {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
                i += 1;
                continue;
            }
            match c {
                b'"' | b'\'' => quote = Some(c),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b';' if depth == 0 => return Some(i),
                b'-' if depth == 0 && bytes.get(i + 1) == Some(&b'-') => return Some(i),
                _ => {}
            }
            i += 1;
        }
        (depth <= 0 && quote.is_none()).then_some(bytes.len())
    }

    /// Build the read-side expression for a value-object access.
    /// Plain value → `recv:GetAttribute("Name")`.
    /// ObjectValue  → `FindByUUID(recv:GetAttribute("Name"))` (resolve the UUID
    /// string attribute back to a live Instance via the runtime resolver).
    fn read_expr(recv: &str, name: &str, is_ref: bool) -> String {
        if is_ref {
            format!("FindByUUID({recv}:GetAttribute(\"{name}\"))")
        } else {
            format!("{recv}:GetAttribute(\"{name}\")")
        }
    }

    /// Index where the receiver expression (maximal `[A-Za-z0-9_.]` run) that
    /// ends at `marker_start` begins. Returns `marker_start` itself when there
    /// is no identifier char immediately to the left.
    fn receiver_start(line: &str, marker_start: usize) -> usize {
        let bytes = line.as_bytes();
        let mut i = marker_start;
        while i > 0 {
            let c = bytes[i - 1];
            let is_ident = c.is_ascii_alphanumeric() || c == b'_' || c == b'.';
            if is_ident {
                i -= 1;
            } else {
                break;
            }
        }
        i
    }

    /// Heuristic: does this line capture the value-object into a local without
    /// immediately dereferencing it? Matches `local <ident> = <recv>.Name`
    /// where the char after `.Name` is NOT `.` or `:` (so it is the whole RHS,
    /// i.e. the object itself is being aliased, not its `.Value`/`.Changed`).
    fn has_unsafe_local_capture(line: &str, name: &str) -> bool {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("local ") {
            return false;
        }
        // Must contain `= ... .Name` and that `.Name` not be followed by `.`/`:`.
        let needle = format!(".{name}");
        let Some(eq) = trimmed.find('=') else { return false };
        let rhs = &trimmed[eq + 1..];
        let mut search_from = 0usize;
        while let Some(rel) = rhs[search_from..].find(&needle) {
            let pos = search_from + rel;
            let after = pos + needle.len();
            // Char immediately after `.Name`.
            let next = rhs[after..].chars().next();
            // A bare alias (`= obj.Name` end-of-expr, or followed by space, `)`,
            // `,`) with no `.`/`:` deref is the unsafe case.
            match next {
                Some('.') | Some(':') => { /* dereferenced → handled elsewhere */ }
                // Identifier continuation means it's `.NameOther`, not our name.
                Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
                _ => return true,
            }
            search_from = after;
        }
        false
    }

    /// Whole-source warnings for value-object constructs that are inherently
    /// unrewritable by substring rules: runtime `Instance.new("…Value")` and
    /// (heuristically) value-objects handed to functions.
    fn warn_runtime_value_objects(source: &str, warnings: &mut Vec<TransformWarning>) {
        const VALUE_CLASSES: [&str; 11] = [
            "NumberValue", "StringValue", "IntValue", "BoolValue", "ObjectValue",
            "Color3Value", "Vector3Value", "CFrameValue", "BrickColorValue",
            "RayValue", "BinaryStringValue",
        ];
        for (idx, raw_line) in source.lines().enumerate() {
            let line_no = (idx as u32) + 1;
            if raw_line.trim_start().starts_with("--") {
                continue;
            }
            for class in VALUE_CLASSES {
                // `Instance.new("NumberValue")` (either quote style).
                let dq = format!("Instance.new(\"{class}\")");
                let sq = format!("Instance.new('{class}')");
                if raw_line.contains(&dq) || raw_line.contains(&sq) {
                    warnings.push(TransformWarning {
                        line: Some(line_no),
                        message: format!(
                            "`Instance.new(\"{class}\")` creates a value object at runtime; the \
                             importer's attribute folding does not apply to it. Port to a parent \
                             attribute via `:SetAttribute(...)` / `:GetAttribute(...)` manually.",
                        ),
                        severity: WarningSeverity::Warning,
                    });
                }
            }
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

#[cfg(test)]
mod value_object_rewrite_tests {
    use super::*;

    fn ctx(names: &[&str], refs: &[&str]) -> ValueObjectContext {
        ValueObjectContext {
            names: names.iter().map(|s| s.to_string()).collect(),
            ref_names: refs.iter().map(|s| s.to_string()).collect(),
            range_names: Default::default(),
        }
    }

    fn rewrite(src: &str, vo: &ValueObjectContext) -> String {
        ScriptTransformer::transform_value_objects(src, vo).source
    }

    #[test]
    fn a_constrained_values_range_reads_and_writes_its_attributes() {
        let mut vo = ctx(&["NitroAmount"], &[]);
        vo.range_names.insert("NitroAmount".into());
        assert_eq!(
            rewrite("local cap = car.Handling.Nitro.NitroAmount.MaxValue", &vo),
            "local cap = car.Handling.Nitro:GetAttribute(\"NitroAmount_MaxValue\")"
        );
        assert_eq!(rewrite("n.NitroAmount.MinValue = 5", &vo), "n:SetAttribute(\"NitroAmount_MinValue\", 5)");
        assert_eq!(
            rewrite("x = n:FindFirstChild(\"NitroAmount\").ConstrainedValue", &vo),
            "x = n:GetAttribute(\"NitroAmount\")"
        );
        assert_eq!(
            rewrite("n.NitroAmount.Value = n.NitroAmount.MaxValue", &vo),
            "n:SetAttribute(\"NitroAmount\", n:GetAttribute(\"NitroAmount_MaxValue\"))"
        );
        // Only a constrained value's MaxValue is folded.
        let plain = ctx(&["Speed"], &[]);
        assert_eq!(rewrite("x = car.Speed.MaxValue", &plain), "x = car.Speed.MaxValue");
    }

    #[test]
    fn a_child_lookup_assignment_becomes_set_attribute() {
        let vo = ctx(&["data"], &[]);
        assert_eq!(
            rewrite("script:WaitForChild(\"data\").Value = game:GetService(\"HttpService\"):JSONEncode(VehicleData)", &vo),
            "script:SetAttribute(\"data\", game:GetService(\"HttpService\"):JSONEncode(VehicleData))"
        );
        assert_eq!(rewrite("script.data.Value = x", &vo), "script:SetAttribute(\"data\", x)");
    }

    #[test]
    fn a_read_through_a_call_chain_is_rewritten() {
        let vo = ctx(&["CarSpawnDelay"], &[]);
        assert_eq!(
            rewrite("local d = RS:WaitForChild(\"VehicleEvents\"):WaitForChild(\"CarSpawnDelay\").Value", &vo),
            "local d = RS:WaitForChild(\"VehicleEvents\"):GetAttribute(\"CarSpawnDelay\")"
        );
        assert_eq!(
            rewrite("wait(game:GetService(\"ReplicatedStorage\").CarSpawnDelay.Value)", &vo),
            "wait(game:GetService(\"ReplicatedStorage\"):GetAttribute(\"CarSpawnDelay\"))"
        );
    }

    #[test]
    fn compound_assignments_read_the_value_again() {
        let vo = ctx(&["Count"], &[]);
        assert_eq!(rewrite("cfg.Count.Value += 1", &vo), "cfg:SetAttribute(\"Count\", cfg:GetAttribute(\"Count\") + (1))");
        // A child lookup on a plain receiver reads the value the same way.
        assert_eq!(
            rewrite("a:WaitForChild(\"Count\").Value += 1", &vo),
            "a:SetAttribute(\"Count\", a:GetAttribute(\"Count\") + (1))"
        );
        // A receiver that is a call result is never evaluated twice: it is
        // left alone, with a warning.
        let src = "RS:WaitForChild(\"Cfg\"):WaitForChild(\"Count\").Value += 1";
        let through_call = ScriptTransformer::transform_value_objects(src, &vo);
        assert_eq!(through_call.source, src, "left alone, never broken");
        assert!(through_call.warnings.iter().any(|w| w.message.contains("Compound")));
    }

    #[test]
    fn comments_and_statements_after_an_assignment_survive() {
        let vo = ctx(&["Speed", "Gear"], &[]);
        assert_eq!(rewrite("car.Speed.Value = 5 -- max", &vo), "car:SetAttribute(\"Speed\", 5) -- max");
        assert_eq!(
            rewrite("car.Speed.Value = 1; car.Gear.Value = 2", &vo),
            "car:SetAttribute(\"Speed\", 1); car:SetAttribute(\"Gear\", 2)"
        );
    }

    #[test]
    fn comparisons_are_reads_and_longer_members_are_left_alone() {
        let vo = ctx(&["Count", "data"], &[]);
        assert_eq!(rewrite("if cfg.Count.Value == 3 then", &vo), "if cfg:GetAttribute(\"Count\") == 3 then");
        assert_eq!(rewrite("x = obj.data.ValueMap", &vo), "x = obj.data.ValueMap");
    }

    #[test]
    fn a_value_continuing_on_the_next_line_is_not_broken() {
        let vo = ctx(&["Cfg"], &[]);
        let r = ScriptTransformer::transform_value_objects("obj.Cfg.Value = {\n  a = 1,\n}", &vo);
        assert_eq!(r.source, "obj.Cfg.Value = {\n  a = 1,\n}");
        assert!(r.warnings.iter().any(|w| w.message.contains("next line")));
    }

    #[test]
    fn object_values_resolve_by_uuid() {
        let vo = ctx(&["Target"], &["Target"]);
        assert_eq!(rewrite("local t = cfg.Target.Value", &vo), "local t = FindByUUID(cfg:GetAttribute(\"Target\"))");
        let r = ScriptTransformer::transform_value_objects("local t = f():WaitForChild(\"Target\").Value", &vo);
        assert_eq!(r.source, "local t = f():WaitForChild(\"Target\").Value");
        assert!(r.warnings.iter().any(|w| w.message.contains("FindByUUID")));
    }

    #[test]
    fn observers_and_the_output_are_stable() {
        let vo = ctx(&["A", "B"], &[]);
        let src = "x.A.Changed:Connect(f) x.B:GetPropertyChangedSignal(\"Value\"):Connect(g)";
        let want = "x:GetAttributeChangedSignal(\"A\"):Connect(f) x:GetAttributeChangedSignal(\"B\"):Connect(g)";
        for _ in 0..8 {
            assert_eq!(rewrite(src, &vo), want);
        }
        assert_eq!(rewrite("print(nothing.Here)", &vo), "print(nothing.Here)");
    }
}
