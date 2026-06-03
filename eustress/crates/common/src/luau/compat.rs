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
/// can be wrapped in the `FindByUUID` resolver.
#[derive(Debug, Clone, Default)]
pub struct ValueObjectContext {
    pub names: std::collections::HashSet<String>,     // all converted value-object Names
    pub ref_names: std::collections::HashSet<String>, // subset that were ObjectValue
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

    /// Core value-object rewrite pass (CONTRACT D). String/substring based to
    /// match the rest of this transformer; best-effort about skipping string
    /// literals and comments (see `line_is_skippable`).
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

        // The "receiver" preceding `.Name` / `:FindFirstChild(...)` — a dotted
        // identifier chain like `game.Workspace.Cfg` or a bare `script`. We do
        // NOT try to parse Luau; we greedily capture the identifier/`.`/`_`
        // run immediately to the left of the match site.
        //
        // Process the source line-by-line so we can (a) cheaply skip full-line
        // comments and (b) attach 1-based line numbers to warnings.
        let mut out_lines: Vec<String> = Vec::with_capacity(source.lines().count());

        for (idx, raw_line) in source.lines().enumerate() {
            let line_no = (idx as u32) + 1;

            // Whole-line comment → never rewrite; pass through verbatim.
            if raw_line.trim_start().starts_with("--") {
                out_lines.push(raw_line.to_string());
                continue;
            }

            let mut line = raw_line.to_string();

            for name in &vo.names {
                let is_ref = vo.ref_names.contains(name);

                // ---- Detect unsafe constructs (warn, do NOT rewrite) ----------
                // 1. value-object captured into a local: `local x = <recv>.Name`
                //    that is NOT immediately followed by `.Value` / `.Changed` /
                //    `:GetPropertyChangedSignal`. Such a local is later used as
                //    `x.Value`, which we cannot trace here.
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

                // ---- Ordered rewrites (longest / most specific first) ---------

                // A) Observer: `<recv>.Name.Changed:Connect`
                //    → `<recv>:GetAttributeChangedSignal("Name"):Connect`
                let pat_changed = format!(".{name}.Changed");
                changes += Self::replace_with_receiver(
                    &mut line,
                    &pat_changed,
                    |recv| format!("{recv}:GetAttributeChangedSignal(\"{name}\")"),
                );

                // B) Observer: `<recv>.Name:GetPropertyChangedSignal("Value")`
                //    → `<recv>:GetAttributeChangedSignal("Name")`
                //    (accept both quote styles)
                for q in ['"', '\''] {
                    let pat_gpcs = format!(".{name}:GetPropertyChangedSignal({q}Value{q})");
                    changes += Self::replace_with_receiver(
                        &mut line,
                        &pat_gpcs,
                        |recv| format!("{recv}:GetAttributeChangedSignal(\"{name}\")"),
                    );
                }

                // C) FindFirstChild / WaitForChild read:
                //    `<recv>:FindFirstChild("Name").Value` → read form
                //    `<recv>:WaitForChild("Name").Value`   → read form
                for method in ["FindFirstChild", "WaitForChild"] {
                    for q in ['"', '\''] {
                        let pat_find = format!(":{method}({q}{name}{q}).Value");
                        changes += Self::replace_with_receiver(
                            &mut line,
                            &pat_find,
                            |recv| Self::read_expr(recv, name, is_ref),
                        );
                    }
                }

                // D) Assignment: `<recv>.Name.Value = V` → SetAttribute form.
                //    MUST run before the bare-read rule (E) so we don't first
                //    turn the LHS into a GetAttribute call.
                let assign_marker = format!(".{name}.Value");
                changes += Self::replace_assignment_with_receiver(
                    &mut line,
                    &assign_marker,
                    name,
                    is_ref,
                    &mut warnings,
                    line_no,
                );

                // E) Bare read: `<recv>.Name.Value` → read form.
                let pat_read = format!(".{name}.Value");
                changes += Self::replace_with_receiver(
                    &mut line,
                    &pat_read,
                    |recv| Self::read_expr(recv, name, is_ref),
                );
            }

            out_lines.push(line);
        }

        // Preserve a trailing newline if the original had one (lines() drops it).
        let mut rewritten = out_lines.join("\n");
        if source.ends_with('\n') {
            rewritten.push('\n');
        }

        // Runtime-created value objects and value-objects-as-arguments are
        // global (not per-name) concerns — scan the whole source once.
        Self::warn_runtime_value_objects(source, &mut warnings);

        (rewritten, changes, warnings)
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

    /// Find every occurrence of `marker` in `line`, capture the receiver
    /// expression immediately to its left, and replace
    /// `<receiver><marker>` with `make(receiver)`. Returns the number of
    /// replacements performed.
    ///
    /// The receiver is the maximal run of `[A-Za-z0-9_.]` ending at the marker
    /// (so `game.Workspace.Cfg.Name.Value` captures receiver
    /// `game.Workspace.Cfg`). A leading `:` / call-paren just before the run is
    /// NOT consumed, so method-call results like `…):Foo` are left intact.
    fn replace_with_receiver<F>(line: &mut String, marker: &str, make: F) -> u32
    where
        F: Fn(&str) -> String,
    {
        let mut changes = 0u32;
        // Scan forward; `search_from` advances past every occurrence (rewritten
        // or skipped) so the loop always terminates. None of the replacement
        // forms re-contain their own marker, so a rewrite never re-matches.
        let mut search_from = 0usize;
        loop {
            let Some(rel) = line[search_from..].find(marker) else { break };
            let m_start = search_from + rel;

            // Capture receiver: walk left over identifier/dot chars.
            let recv_start = Self::receiver_start(line, m_start);
            if recv_start == m_start {
                // No receiver to the left (e.g. marker at line start or after a
                // bare operator) — cannot safely rewrite; skip past this marker
                // and keep scanning for later valid occurrences.
                search_from = m_start + marker.len();
                continue;
            }

            let receiver = line[recv_start..m_start].to_string();
            let replacement = make(&receiver);
            let m_end = m_start + marker.len();
            line.replace_range(recv_start..m_end, &replacement);
            changes += 1;
            // Resume scanning after the inserted text.
            search_from = recv_start + replacement.len();
        }
        changes
    }

    /// Like [`replace_with_receiver`](Self::replace_with_receiver) but for the
    /// assignment form `<receiver>.Name.Value = V`. Only rewrites when the
    /// marker is followed (after optional whitespace) by a single `=` that is
    /// NOT part of `==`, `~=`, `<=`, `>=` (i.e. a real assignment, not a
    /// comparison). Produces `<receiver>:SetAttribute("Name", V)`.
    ///
    /// For ObjectValue names, `V` is wrapped so a live instance is stored as
    /// its UUID: `recv:SetAttribute("Name", V and V:GetUuid() or "")`, and a
    /// lossy-assignment warning is recorded.
    fn replace_assignment_with_receiver(
        line: &mut String,
        marker: &str,
        name: &str,
        is_ref: bool,
        warnings: &mut Vec<TransformWarning>,
        line_no: u32,
    ) -> u32 {
        let mut changes = 0u32;
        let mut search_from = 0usize;
        loop {
            let Some(rel) = line[search_from..].find(marker) else { break };
            let m_start = search_from + rel;
            let m_end = m_start + marker.len();

            // Look past the marker for an assignment `=` (skip spaces/tabs).
            let after = &line[m_end..];
            let trimmed = after.trim_start();
            let ws_len = after.len() - trimmed.len();

            let is_assignment = {
                let bytes = trimmed.as_bytes();
                if bytes.first() == Some(&b'=') {
                    // Not `==` (next char `=`).
                    bytes.get(1) != Some(&b'=')
                } else {
                    false
                }
            };
            // Also reject comparison operators that put a char *before* `=`
            // immediately after the marker (`~=`, `<=`, `>=`): those would have
            // their operator char as `trimmed[0]`, so `is_assignment` is already
            // false for them. The `==` case is handled above.

            if !is_assignment {
                // Leave for the bare-read rule; advance past this marker.
                search_from = m_end;
                continue;
            }

            let recv_start = Self::receiver_start(line, m_start);
            if recv_start == m_start {
                search_from = m_end;
                continue;
            }
            let receiver = line[recv_start..m_start].to_string();

            // Position of the `=` and the start of the RHS value expression.
            let eq_pos = m_end + ws_len; // index of '='
            let rhs = line[eq_pos + 1..].to_string();
            let rhs_trimmed = rhs.trim();

            let value_expr = if is_ref {
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
                format!("({rhs_trimmed}) and ({rhs_trimmed}):GetUuid() or \"\"")
            } else {
                rhs_trimmed.to_string()
            };

            let replacement =
                format!("{receiver}:SetAttribute(\"{name}\", {value_expr})");
            line.replace_range(recv_start.., &replacement);
            changes += 1;
            // The remainder of the line was the RHS we just consumed; nothing
            // after it to scan.
            break;
        }
        changes
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
