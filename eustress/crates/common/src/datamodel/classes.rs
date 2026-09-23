//! Class knowledge the DataModel needs: Roblox inheritance for `IsA`, the
//! service list, and the properties a freshly made `Instance.new(class)`
//! starts with.
//!
//! Positions and sizes are metres (the engine is metre-native; studs are a
//! display unit only).

use crate::scripting::{CFrame, Color3, UDim2, Vector3};

use super::{DmValue, EnumItem};

/// The chain of base classes for `class`, most derived first, ending in
/// `Instance`. Unknown classes are treated as plain `Instance`s.
pub fn class_ancestry(class: &str) -> &'static [&'static str] {
    match class {
        "Part" | "Ball" => &["Part", "FormFactorPart", "BasePart", "PVInstance", "Instance"],
        "WedgePart" => &["WedgePart", "FormFactorPart", "BasePart", "PVInstance", "Instance"],
        "CornerWedgePart" => &["CornerWedgePart", "BasePart", "PVInstance", "Instance"],
        "TrussPart" => &["TrussPart", "BasePart", "PVInstance", "Instance"],
        "MeshPart" => &["MeshPart", "TriangleMeshPart", "BasePart", "PVInstance", "Instance"],
        "UnionOperation" => &["UnionOperation", "PartOperation", "TriangleMeshPart", "BasePart", "PVInstance", "Instance"],
        "SpawnLocation" => &["SpawnLocation", "Part", "FormFactorPart", "BasePart", "PVInstance", "Instance"],
        "Seat" => &["Seat", "Part", "FormFactorPart", "BasePart", "PVInstance", "Instance"],
        "VehicleSeat" => &["VehicleSeat", "BasePart", "PVInstance", "Instance"],
        "Workspace" => &["Workspace", "WorldRoot", "Model", "PVInstance", "Instance"],
        "Model" => &["Model", "PVInstance", "Instance"],
        "Actor" => &["Actor", "Model", "PVInstance", "Instance"],
        "Tool" => &["Tool", "BackpackItem", "Instance"],
        "Accessory" => &["Accessory", "Accoutrement", "Instance"],
        "Camera" => &["Camera", "Instance"],
        "Humanoid" => &["Humanoid", "Instance"],
        "Player" => &["Player", "Instance"],
        "Folder" => &["Folder", "Instance"],
        "Configuration" => &["Configuration", "Instance"],
        "Script" => &["Script", "BaseScript", "LuaSourceContainer", "Instance"],
        "LocalScript" => &["LocalScript", "Script", "BaseScript", "LuaSourceContainer", "Instance"],
        "ModuleScript" => &["ModuleScript", "LuaSourceContainer", "Instance"],
        "SoulScript" => &["SoulScript", "LuaSourceContainer", "Instance"],
        "ScreenGui" => &["ScreenGui", "LayerCollector", "GuiBase2d", "Instance"],
        "BillboardGui" => &["BillboardGui", "LayerCollector", "GuiBase2d", "Instance"],
        "SurfaceGui" => &["SurfaceGui", "SurfaceGuiBase", "LayerCollector", "GuiBase2d", "Instance"],
        "Frame" => &["Frame", "GuiObject", "GuiBase2d", "Instance"],
        "ScrollingFrame" => &["ScrollingFrame", "GuiObject", "GuiBase2d", "Instance"],
        "TextLabel" => &["TextLabel", "GuiLabel", "GuiObject", "GuiBase2d", "Instance"],
        "ImageLabel" => &["ImageLabel", "GuiLabel", "GuiObject", "GuiBase2d", "Instance"],
        "TextButton" => &["TextButton", "GuiButton", "GuiObject", "GuiBase2d", "Instance"],
        "ImageButton" => &["ImageButton", "GuiButton", "GuiObject", "GuiBase2d", "Instance"],
        "TextBox" => &["TextBox", "GuiObject", "GuiBase2d", "Instance"],
        "ViewportFrame" => &["ViewportFrame", "GuiObject", "GuiBase2d", "Instance"],
        "UIListLayout" => &["UIListLayout", "UIGridStyleLayout", "UILayout", "UIComponent", "UIBase", "Instance"],
        "UIGridLayout" => &["UIGridLayout", "UIGridStyleLayout", "UILayout", "UIComponent", "UIBase", "Instance"],
        "UIPadding" | "UICorner" | "UIStroke" | "UIScale" | "UIGradient" | "UIAspectRatioConstraint"
        | "UISizeConstraint" | "UITextSizeConstraint" => &["UIComponent", "UIBase", "Instance"],
        "PointLight" => &["PointLight", "Light", "Instance"],
        "SpotLight" => &["SpotLight", "Light", "Instance"],
        "SurfaceLight" => &["SurfaceLight", "Light", "Instance"],
        "Sound" => &["Sound", "Instance"],
        "SoundGroup" => &["SoundGroup", "Instance"],
        "ParticleEmitter" => &["ParticleEmitter", "Instance"],
        "Beam" => &["Beam", "Instance"],
        "Trail" => &["Trail", "Instance"],
        "Fire" | "Smoke" | "Sparkles" => &["Instance"],
        "Explosion" => &["Explosion", "Instance"],
        "Attachment" => &["Attachment", "Instance"],
        "Decal" => &["Decal", "FaceInstance", "Instance"],
        "Texture" => &["Texture", "Decal", "FaceInstance", "Instance"],
        "SpecialMesh" => &["SpecialMesh", "FileMesh", "DataModelMesh", "Instance"],
        "WeldConstraint" => &["WeldConstraint", "Instance"],
        "Weld" => &["Weld", "JointInstance", "Instance"],
        "Motor6D" => &["Motor6D", "Motor", "JointInstance", "Instance"],
        "BindableEvent" => &["BindableEvent", "Instance"],
        "BindableFunction" => &["BindableFunction", "Instance"],
        "RemoteEvent" => &["RemoteEvent", "BaseRemoteEvent", "Instance"],
        "RemoteFunction" => &["RemoteFunction", "Instance"],
        "StringValue" | "NumberValue" | "IntValue" | "BoolValue" | "ObjectValue" | "Vector3Value"
        | "CFrameValue" | "Color3Value" | "BrickColorValue" | "RayValue" => &["ValueBase", "Instance"],
        "Team" => &["Team", "Instance"],
        "ClickDetector" => &["ClickDetector", "Instance"],
        "ProximityPrompt" => &["ProximityPrompt", "Instance"],
        "Animator" => &["Animator", "Instance"],
        "Animation" => &["Animation", "Instance"],
        "Sky" => &["Sky", "Instance"],
        "Atmosphere" => &["Atmosphere", "Instance"],
        "Terrain" => &["Terrain", "BasePart", "PVInstance", "Instance"],
        _ => &["Instance"],
    }
}

/// Roblox `Instance:IsA(base)`.
pub fn class_is_a(class: &str, base: &str) -> bool {
    if class == base || base == "Instance" {
        return true;
    }
    if class_ancestry(class).iter().any(|c| *c == base) {
        return true;
    }
    // Value objects all share ValueBase even though the table above names
    // only the base.
    base == "ValueBase" && class.ends_with("Value")
}

/// True for every class that renders and collides as a part.
pub fn is_base_part(class: &str) -> bool {
    class_is_a(class, "BasePart")
}

/// True for 2D GUI objects (Frame, TextLabel, ...), not the layer containers.
pub fn is_gui_object(class: &str) -> bool {
    class_is_a(class, "GuiObject")
}

/// Services a Space can hold, in the order `game:GetChildren()` lists them.
pub const SERVICE_CLASSES: &[&str] = &[
    "Workspace",
    "Players",
    "Lighting",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "SoundService",
    "Teams",
    "Chat",
    "MaterialService",
    "SoulService",
    "RunService",
    "UserInputService",
    "TweenService",
    "Debris",
    "CollectionService",
    "HttpService",
    "DataStoreService",
    "ContextActionService",
    "PathfindingService",
    "PhysicsService",
    "TextService",
    "GuiService",
    "MarketplaceService",
    "TeleportService",
    "BadgeService",
];

/// Services whose contents never render or collide (Roblox semantics).
pub fn is_storage_service(class: &str) -> bool {
    matches!(
        class,
        "ReplicatedStorage" | "ServerStorage" | "ServerScriptService" | "StarterPack" | "StarterGui" | "StarterPlayer"
            | "ReplicatedFirst" | "Lighting" | "SoundService" | "Teams" | "Chat" | "MaterialService"
            | "SoulService"
    )
}

pub fn is_service_class(class: &str) -> bool {
    SERVICE_CLASSES.contains(&class)
        || matches!(class, "StarterPlayerScripts" | "StarterCharacterScripts" | "Website" | "DataService"
            | "ExperimentService" | "AdornmentService" | "CustomService")
}

/// Properties an `Instance.new(class)` starts with. Engine-seeded instances
/// get their values from the ECS instead.
pub fn default_properties(class: &str) -> Vec<(&'static str, DmValue)> {
    let mut props: Vec<(&'static str, DmValue)> = Vec::new();
    if is_base_part(class) {
        let (size, shape) = match class {
            "SpawnLocation" => (Vector3::new(4.0, 1.0, 4.0), "Block"),
            "Seat" => (Vector3::new(2.0, 1.0, 2.0), "Block"),
            "WedgePart" => (Vector3::new(4.0, 1.0, 2.0), "Wedge"),
            "CornerWedgePart" => (Vector3::new(2.0, 2.0, 2.0), "CornerWedge"),
            "MeshPart" => (Vector3::new(1.0, 1.0, 1.0), "Block"),
            _ => (Vector3::new(4.0, 1.0, 2.0), "Block"),
        };
        props.push(("CFrame", DmValue::CFrame(CFrame::new(0.0, 0.5, 0.0))));
        props.push(("Size", DmValue::Vector3(size)));
        props.push(("Color", DmValue::Color3(Color3::new(163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0))));
        props.push(("Material", DmValue::Enum(EnumItem::new("Material", "Plastic"))));
        props.push(("Shape", DmValue::Enum(EnumItem::new("PartType", shape))));
        props.push(("Transparency", DmValue::Number(0.0)));
        props.push(("Reflectance", DmValue::Number(0.0)));
        props.push(("Anchored", DmValue::Bool(false)));
        props.push(("CanCollide", DmValue::Bool(true)));
        props.push(("CanTouch", DmValue::Bool(true)));
        props.push(("CanQuery", DmValue::Bool(true)));
        props.push(("CastShadow", DmValue::Bool(true)));
        props.push(("Massless", DmValue::Bool(false)));
        props.push(("Locked", DmValue::Bool(false)));
        props.push(("AssemblyLinearVelocity", DmValue::Vector3(Vector3::ZERO)));
        props.push(("AssemblyAngularVelocity", DmValue::Vector3(Vector3::ZERO)));
        props.push(("CollisionGroup", DmValue::String("Default".into())));
        if class == "MeshPart" {
            props.push(("MeshId", DmValue::String(String::new())));
            props.push(("TextureID", DmValue::String(String::new())));
        }
        if class == "SpawnLocation" {
            props.push(("Enabled", DmValue::Bool(true)));
            props.push(("Neutral", DmValue::Bool(true)));
            props.push(("Duration", DmValue::Number(0.0)));
            props.push(("Anchored", DmValue::Bool(true)));
        }
        return props;
    }
    match class {
        "Model" | "Actor" => {
            props.push(("PrimaryPart", DmValue::Nil));
        }
        "Humanoid" => {
            props.push(("Health", DmValue::Number(100.0)));
            props.push(("MaxHealth", DmValue::Number(100.0)));
            props.push(("WalkSpeed", DmValue::Number(4.5)));
            props.push(("JumpPower", DmValue::Number(14.0)));
            props.push(("JumpHeight", DmValue::Number(2.0)));
            props.push(("AutoRotate", DmValue::Bool(true)));
            props.push(("HipHeight", DmValue::Number(0.0)));
            props.push(("MoveDirection", DmValue::Vector3(Vector3::ZERO)));
            props.push(("PlatformStand", DmValue::Bool(false)));
            props.push(("Sit", DmValue::Bool(false)));
            props.push(("DisplayName", DmValue::String(String::new())));
        }
        "Camera" => {
            props.push(("CFrame", DmValue::CFrame(CFrame::new(0.0, 20.0, 20.0))));
            props.push(("Focus", DmValue::CFrame(CFrame::IDENTITY)));
            props.push(("FieldOfView", DmValue::Number(70.0)));
            props.push(("CameraType", DmValue::Enum(EnumItem::new("CameraType", "Custom"))));
            props.push(("CameraSubject", DmValue::Nil));
            props.push(("ViewportSize", DmValue::Vector2(crate::scripting::Vector2::new(1280.0, 720.0))));
            // Eustress extensions: the same projection switch the editor's
            // Perspective control drives.
            props.push(("Projection", DmValue::Enum(EnumItem::new("CameraProjection", "Perspective"))));
            props.push(("OrthographicSize", DmValue::Number(40.0)));
        }
        "ScreenGui" => {
            props.push(("Enabled", DmValue::Bool(true)));
            props.push(("DisplayOrder", DmValue::Number(0.0)));
            props.push(("IgnoreGuiInset", DmValue::Bool(false)));
            props.push(("ResetOnSpawn", DmValue::Bool(true)));
        }
        "BillboardGui" => {
            props.push(("Enabled", DmValue::Bool(true)));
            props.push(("Adornee", DmValue::Nil));
            props.push(("AlwaysOnTop", DmValue::Bool(false)));
            props.push(("Size", DmValue::UDim2(UDim2::new(0.0, 200.0, 0.0, 50.0))));
            props.push(("StudsOffset", DmValue::Vector3(Vector3::new(0.0, 2.0, 0.0))));
            props.push(("MaxDistance", DmValue::Number(100.0)));
        }
        "Frame" | "ScrollingFrame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton"
        | "ViewportFrame" => {
            props.push(("Position", DmValue::UDim2(UDim2::new(0.0, 0.0, 0.0, 0.0))));
            let size = match class {
                "Frame" | "ImageLabel" | "ImageButton" => UDim2::new(0.0, 100.0, 0.0, 100.0),
                "ScrollingFrame" => UDim2::new(0.0, 200.0, 0.0, 200.0),
                _ => UDim2::new(0.0, 200.0, 0.0, 50.0),
            };
            props.push(("Size", DmValue::UDim2(size)));
            props.push(("AnchorPoint", DmValue::Vector2(crate::scripting::Vector2::new(0.0, 0.0))));
            props.push(("Visible", DmValue::Bool(true)));
            props.push(("BackgroundColor3", DmValue::Color3(Color3::new(1.0, 1.0, 1.0))));
            props.push(("BackgroundTransparency", DmValue::Number(0.0)));
            props.push(("BorderColor3", DmValue::Color3(Color3::new(0.106, 0.165, 0.208))));
            props.push(("BorderSizePixel", DmValue::Number(1.0)));
            props.push(("ZIndex", DmValue::Number(1.0)));
            props.push(("LayoutOrder", DmValue::Number(0.0)));
            props.push(("Rotation", DmValue::Number(0.0)));
            if matches!(class, "TextLabel" | "TextButton" | "TextBox") {
                let text = if class == "TextButton" { "Button" } else { "" };
                props.push(("Text", DmValue::String(text.into())));
                props.push(("TextColor3", DmValue::Color3(Color3::new(0.0, 0.0, 0.0))));
                props.push(("TextSize", DmValue::Number(14.0)));
                props.push(("TextTransparency", DmValue::Number(0.0)));
                props.push(("TextScaled", DmValue::Bool(false)));
                props.push(("TextWrapped", DmValue::Bool(false)));
                props.push(("Font", DmValue::Enum(EnumItem::new("Font", "SourceSans"))));
                props.push(("TextXAlignment", DmValue::Enum(EnumItem::new("TextXAlignment", "Center"))));
                props.push(("TextYAlignment", DmValue::Enum(EnumItem::new("TextYAlignment", "Center"))));
                props.push(("RichText", DmValue::Bool(false)));
            }
            if matches!(class, "ImageLabel" | "ImageButton") {
                props.push(("Image", DmValue::String(String::new())));
                props.push(("ImageColor3", DmValue::Color3(Color3::new(1.0, 1.0, 1.0))));
                props.push(("ImageTransparency", DmValue::Number(0.0)));
            }
            if matches!(class, "TextButton" | "ImageButton") {
                props.push(("AutoButtonColor", DmValue::Bool(true)));
                props.push(("Active", DmValue::Bool(true)));
            }
        }
        "PointLight" | "SpotLight" | "SurfaceLight" => {
            props.push(("Brightness", DmValue::Number(1.0)));
            props.push(("Color", DmValue::Color3(Color3::new(1.0, 1.0, 1.0))));
            props.push(("Range", DmValue::Number(8.0)));
            props.push(("Enabled", DmValue::Bool(true)));
            props.push(("Shadows", DmValue::Bool(false)));
            if class != "PointLight" {
                props.push(("Angle", DmValue::Number(90.0)));
                props.push(("Face", DmValue::Enum(EnumItem::new("NormalId", "Front"))));
            }
        }
        "Sound" => {
            props.push(("SoundId", DmValue::String(String::new())));
            props.push(("Volume", DmValue::Number(0.5)));
            props.push(("PlaybackSpeed", DmValue::Number(1.0)));
            props.push(("Looped", DmValue::Bool(false)));
            props.push(("Playing", DmValue::Bool(false)));
            props.push(("TimePosition", DmValue::Number(0.0)));
            props.push(("RollOffMaxDistance", DmValue::Number(60.0)));
        }
        "ParticleEmitter" => {
            props.push(("Enabled", DmValue::Bool(true)));
            props.push(("Rate", DmValue::Number(20.0)));
            props.push(("Lifetime", DmValue::NumberRange(crate::scripting::NumberRange::new(1.0, 2.0))));
            props.push(("Speed", DmValue::NumberRange(crate::scripting::NumberRange::new(3.0, 5.0))));
            props.push(("SpreadAngle", DmValue::Vector2(crate::scripting::Vector2::new(10.0, 10.0))));
            props.push(("Color", DmValue::ColorSequence(vec![(0.0, Color3::new(1.0, 1.0, 1.0)), (1.0, Color3::new(1.0, 1.0, 1.0))])));
            props.push(("Size", DmValue::NumberSequence(vec![(0.0, 0.3), (1.0, 0.3)])));
            props.push(("Transparency", DmValue::NumberSequence(vec![(0.0, 0.0), (1.0, 1.0)])));
            props.push(("LightEmission", DmValue::Number(0.0)));
            props.push(("Acceleration", DmValue::Vector3(Vector3::ZERO)));
            props.push(("Drag", DmValue::Number(0.0)));
            props.push(("Texture", DmValue::String(String::new())));
        }
        "Attachment" => {
            props.push(("CFrame", DmValue::CFrame(CFrame::IDENTITY)));
            props.push(("Visible", DmValue::Bool(false)));
        }
        "Script" | "LocalScript" => {
            props.push(("Source", DmValue::String(String::new())));
            props.push(("Enabled", DmValue::Bool(true)));
            props.push(("Disabled", DmValue::Bool(false)));
        }
        "ModuleScript" => {
            props.push(("Source", DmValue::String(String::new())));
        }
        "StringValue" => props.push(("Value", DmValue::String(String::new()))),
        "NumberValue" | "IntValue" => props.push(("Value", DmValue::Number(0.0))),
        "BoolValue" => props.push(("Value", DmValue::Bool(false))),
        "ObjectValue" => props.push(("Value", DmValue::Nil)),
        "Vector3Value" => props.push(("Value", DmValue::Vector3(Vector3::ZERO))),
        "CFrameValue" => props.push(("Value", DmValue::CFrame(CFrame::IDENTITY))),
        "Color3Value" => props.push(("Value", DmValue::Color3(Color3::new(1.0, 1.0, 1.0)))),
        "Team" => {
            props.push(("TeamColor", DmValue::Color3(Color3::new(1.0, 1.0, 1.0))));
            props.push(("AutoAssignable", DmValue::Bool(true)));
        }
        "Player" => {
            props.push(("UserId", DmValue::Number(1.0)));
            props.push(("DisplayName", DmValue::String("Player".into())));
            props.push(("Character", DmValue::Nil));
            props.push(("Team", DmValue::Nil));
            props.push(("Neutral", DmValue::Bool(true)));
        }
        "Explosion" => {
            props.push(("Position", DmValue::Vector3(Vector3::ZERO)));
            props.push(("BlastRadius", DmValue::Number(4.0)));
            props.push(("BlastPressure", DmValue::Number(500000.0)));
            props.push(("Visible", DmValue::Bool(true)));
        }
        _ => {}
    }
    props
}

/// The enum type a string-valued write to `prop` should become, so
/// `part.Material = "Neon"` and `part.Material = Enum.Material.Neon` both
/// store the same `EnumItem`.
pub fn enum_type_of(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "Material" => "Material",
        "Shape" => "PartType",
        "CameraType" => "CameraType",
        "Projection" => "CameraProjection",
        "Font" => "Font",
        "TextXAlignment" => "TextXAlignment",
        "TextYAlignment" => "TextYAlignment",
        "Face" => "NormalId",
        "SizeConstraint" => "SizeConstraint",
        "ScaleType" => "ScaleType",
        "FillDirection" => "FillDirection",
        "HorizontalAlignment" => "HorizontalAlignment",
        "VerticalAlignment" => "VerticalAlignment",
        "SortOrder" => "SortOrder",
        "EasingStyle" => "EasingStyle",
        "EasingDirection" => "EasingDirection",
        _ => return None,
    })
}
