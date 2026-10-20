//! Data-driven Insert menu: the class catalog and the context around it.
//!
//! The Insert dropdown in the menu bar and the Insert Object dialog (Ctrl+I,
//! the Explorer row plus button) both list what this module builds. None of it
//! is fixed at startup. Each time either surface opens, the catalog is rebuilt
//! from the class template folders on disk, and the context sections above it
//! (what usually goes inside the selection, what was inserted recently) are
//! recomputed. Dropping a template folder into `common/assets/class_schema/`
//! makes a class insertable the next time the menu opens, with no Rust or
//! Slint edit.
//!
//! ## What the catalog lists
//!
//! A class appears iff both hold:
//!   1. it has a template folder named exactly after it,
//!      `class_schema/<Class>/_instance.toml`, so
//!      `instance_create::create_instance` succeeds and every row creates
//!      something when clicked;
//!   2. [`blank_insert_allowed`] says it may be created blank. Single-instance
//!      objects (Terrain, Camera, Star, Moon), imported media and procurement
//!      records have templates but are made by their own tools.
//!
//! Registered classes without a template (most data, audio and character
//! structs, created as children or by script) are not listed, so the menu never
//! offers a click that errors.
//!
//! ## Context sections
//!
//! [`context_rows`] lists the classes that usually go inside the insert target
//! ([`suggestions_for`]), then the most recent inserts
//! (`EditorSettings::recent_inserts`, kept by [`recent_after_insert`]).
//!
//! ## Routing
//!
//! Each row's `class_name` is its action id: the window emits
//! `on-menu-action("insert:" + class_name)`. For catalog and suggested rows it
//! is the canonical [`ClassName::as_str`] value, which the generic fallback arm
//! in `slint_ui::drain_slint_actions` routes through `create_instance`. A Recent
//! row replays the exact id that was used, so a ribbon shortcut such as
//! `sphere` or `cad_box` keeps its own handler.

use std::path::Path;

use eustress_common::classes::ClassName;

/// A single insertable row fed to the Slint Insert menu and dialog.
///
/// Mirrors the `InsertClassData` struct declared in `ribbon.slint`. The
/// `show_header` flag lets the Slint `for` loop draw a heading above the
/// first row of each group without any "did the group change" logic; the
/// heading is `section` when set, otherwise `category`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertClassDescriptor {
    /// The action id, emitted as `insert:<class_name>`. The canonical
    /// `ClassName::as_str()` for catalog and suggested rows; the id that was
    /// used for a Recent row.
    pub class_name: String,
    /// Category bucket (Parts / Lighting / Constraints / …): the row's icon,
    /// and the heading of catalog groups.
    pub category: String,
    /// Human-facing label shown in the row.
    pub display: String,
    /// True for the first row of each group: the Slint side draws a heading
    /// above it.
    pub show_header: bool,
    /// Heading of a context group ("Suggested for Part", "Recent", "Shapes").
    /// Empty for catalog rows, which are headed by their category.
    pub section: String,
}

/// Coarse category for a `ClassName`, used to group the Insert menu.
///
/// Derived purely from the variant (a big match) so it needs no schema
/// lookup and stays in lockstep with `classes.rs`. New variants default
/// to `"Other"` until categorized; they still appear in the menu, just
/// at the bottom.
pub fn category_for(class: ClassName) -> &'static str {
    use ClassName::*;
    match class {
        // ── Parts / geometry ──
        Part | BasePart | Seat | VehicleSeat | SpawnLocation | UnionOperation
        | SpecialMesh => "Parts",

        // ── Structure / containers ──
        Model | Folder | Configuration | Actor | WorldModel | Backpack
        | StarterGear => "Structure",

        // ── Lighting ──
        PointLight | SpotLight | SurfaceLight | DirectionalLight | Lighting
        | Atmosphere | Sky | Clouds | Star | Moon | ReflectionProbe => "Lighting",

        // ── Constraints / movers / joints ──
        Attachment | WeldConstraint | Motor6D | HingeConstraint
        | DistanceConstraint | PrismaticConstraint | BallSocketConstraint
        | SpringConstraint | RopeConstraint | RodConstraint
        | CylindricalConstraint | TorsionSpringConstraint | UniversalConstraint
        | AlignPosition | AlignOrientation | LinearVelocity | AngularVelocity
        | VectorForce | Torque | PlaneConstraint | BodyPosition | BodyVelocity
        | BodyGyro | BodyAngularVelocity | BodyForce | BodyThrust | Weld | Motor
        | VelocityMotor | NoCollisionConstraint | RigidConstraint | LineForce
        | AnimationConstraint => "Constraints",

        // ── Physical simulations ──
        ParticleSimulation | ParticleSpecies => "Simulation",

        // ── Terrain and its non-destructive layers (layers are placed under
        // Workspace/Terrain/Layers whatever is selected) ──
        Terrain | TerrainDetail | TerrainRegion
        | TerrainSpline | TerrainSplinePoint | TerrainStamp | TerrainFlattenPad
        | TerrainNoise | TerrainMaterialFill | TerrainScatter | TerrainWaterBody => "Terrain",

        // ── Effects / post-FX / VFX ──
        ParticleEmitter | Beam | Decal | BloomEffect | BlurEffect
        | DepthOfFieldEffect | ColorCorrectionEffect | ColorGradingEffect
        | SunRaysEffect | Fire | Smoke | Sparkles | Explosion | Trail
        | ForceField | Highlight => "Effects",

        // ── Audio ──
        Sound | AudioReverb | AudioEcho | AudioDistortion | AudioEqualizer
        | AudioCompressor | AudioChorus | AudioFlanger | AudioFader
        | AudioFilter | AudioPitchShifter | AudioEmitter | AudioListener
        | AudioPlayer | AudioDeviceInput | AudioDeviceOutput | AudioAnalyzer
        | AudioSearchParams | ReverbSoundEffect | EchoSoundEffect
        | DistortionSoundEffect | EqualizerSoundEffect | CompressorSoundEffect
        | ChorusSoundEffect | FlangeSoundEffect | PitchShiftSoundEffect
        | TremoloSoundEffect => "Audio",

        // ── GUI containers + leaves + layout modifiers ──
        ScreenGui | BillboardGui | SurfaceGui | Frame | ScrollingFrame
        | TextLabel | ImageLabel | TextButton | ImageButton | TextBox
        | ViewportFrame | VideoFrame | DocumentFrame | WebFrame | CanvasGroup
        | UICorner | UIGradient | UIStroke | UIListLayout | UIGridLayout
        | UIPadding | UIAspectRatioConstraint | UIScale | UISizeConstraint
        | UITextSizeConstraint | UITableLayout | UIPageLayout | UIFlexItem
        | UIDragDetector => "GUI",

        // ── Scripting / networking ──
        SoulScript | LuauScript | LuauLocalScript | LuauModuleScript
        | WorkshopConversation | RemoteEvent | RemoteFunction | BindableEvent
        | BindableFunction | UnreliableRemoteEvent | Wire | OperationGraph
        => "Scripting",

        // ── ValueObjects ──
        StringValue | IntValue | NumberValue | BoolValue | ObjectValue
        | Color3Value | Vector3Value | CFrameValue | BrickColorValue | RayValue
        | BinaryStringValue => "Values",

        // ── Interaction / character ──
        Tool | Accessory | ClickDetector | ProximityPrompt | Dialog
        | DialogChoice | BodyColors | CharacterMesh | Shirt | Pants
        | ShirtGraphic | Humanoid | DragDetector | BuoyancySensor | HapticEffect
        | Accoutrement | AccessoryDescription | FaceControls | IKControl
        | HumanoidDescription | BodyPartDescription | Team => "Interaction",

        // ── Animation ──
        Animator | KeyframeSequence | Animation | AnimationController
        | HumanoidController | ControllerManager | AirController
        | ClimbController | GroundController | SwimController
        | SkateboardController | VehicleController | ControllerPartSensor
        | KeyframeMarker | Pose | NumberPose | CurveAnimation | AnimationRigData
        => "Animation",

        // ── Meshes / surfaces / skinning ──
        BlockMesh | FileMesh | Texture | SurfaceAppearance | MaterialVariant
        | Bone | WrapDeformer | WrapLayer | WrapTarget => "Meshes",

        // ── Data Platform + data/curves/chat/misc ──
        Dataset | Series | Column | Run | Connector | Domain | ExportTarget
        | DataStoreGetOptions | DataStoreSetOptions | DataStoreIncrementOptions
        | DataStoreOptions | FloatCurve | RotationCurve | EulerRotationCurve
        | Vector3Curve | MarkerCurve | Path2D | LocalizationTable | Noise
        | TextChannel | TextChatCommand | TextChatMessageProperties
        | EditableImage | RobloxEditableImage => "Data",

        // Everything not yet bucketed (assets, adornments, services,
        // orbital, internal bases): still listed, just last.
        _ => "Other",
    }
}

/// Stable display order for categories in the Insert dropdown. Lower
/// number = nearer the top. Unlisted categories sort after these
/// (alphabetically), so a freshly-added category never silently
/// vanishes; it just lands at the end.
fn category_rank(category: &str) -> u8 {
    match category {
        "Parts" => 0,
        "Structure" => 1,
        "Lighting" => 2,
        "Constraints" => 3,
        "Effects" => 4,
        "Simulation" => 5,
        "Terrain" => 6,
        "Audio" => 7,
        "GUI" => 8,
        "Scripting" => 9,
        "Values" => 10,
        "Interaction" => 11,
        "Animation" => 12,
        "Meshes" => 13,
        "Data" => 14,
        "Other" => 15,
        _ => 100,
    }
}

/// Canonical default service folder for a class, used by the generic
/// Insert handler when the user has nothing selected. Mirrors the
/// Roblox/Eustress convention already used by the hardcoded arms (see
/// `slint_ui.rs` `canonical_service`). When an instance or a service row
/// *is* selected the handler inserts there instead and never consults this.
pub fn default_service_for(category: &str) -> &'static str {
    match category {
        "Lighting" => "Lighting",
        "Audio" => "SoundService",
        "GUI" => "StarterGui",
        "Scripting" => "SoulService",
        // Parts, Structure, Constraints, Effects, Interaction, Values,
        // Animation, Meshes, Data, Other → world root.
        _ => "Workspace",
    }
}

/// Whether a class with a template may be created blank from the Insert
/// menu. The exceptions have templates for their own tools:
/// - one per Space: a second terrain root, camera, sun or moon fights the
///   first (the ribbon's Terrain button makes the one terrain);
/// - imported media: File > Import writes them around the file they show;
/// - procurement records: the Procurement panel creates and fills them in.
pub fn blank_insert_allowed(class: ClassName) -> bool {
    !matches!(
        class,
        ClassName::Terrain
            | ClassName::Camera
            | ClassName::Star
            | ClassName::Moon
            | ClassName::Image
            | ClassName::Video
            | ClassName::Manufacturer
            | ClassName::PurchaseOrder
            | ClassName::PurchaseOrderLine
    )
}

/// Build the Insert catalog from `classes`, keeping those `has_template`
/// accepts, grouped and ordered by category with per-group header flags
/// precomputed. Pure: the filesystem check is injected so the grouping is
/// unit-testable.
pub fn build_catalog(
    classes: impl Iterator<Item = ClassName>,
    has_template: impl Fn(&str) -> bool,
) -> Vec<InsertClassDescriptor> {
    let mut rows: Vec<InsertClassDescriptor> = classes
        .filter_map(|class| {
            let name = class.as_str();
            if !has_template(name) {
                return None;
            }
            let category = category_for(class);
            Some(InsertClassDescriptor {
                class_name: name.to_string(),
                category: category.to_string(),
                display: name.to_string(),
                show_header: false,
                section: String::new(),
            })
        })
        .collect();

    // Sort by (category rank, category name, class name) so groups are
    // contiguous and deterministic whatever order the classes arrived in,
    // and drop repeats.
    rows.sort_by(|a, b| {
        category_rank(&a.category)
            .cmp(&category_rank(&b.category))
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.class_name.cmp(&b.class_name))
    });
    rows.dedup_by(|a, b| a.class_name == b.class_name);

    // Mark the first row of each contiguous category group.
    let mut last_category: Option<String> = None;
    for row in &mut rows {
        if last_category.as_deref() != Some(row.category.as_str()) {
            row.show_header = true;
            last_category = Some(row.category.clone());
        }
    }

    rows
}

/// Every class with a creatable template in `dir` (one folder per class,
/// holding `_instance.toml`), read fresh from disk. A folder counts only
/// when its name is the class's canonical name: `ClassName::from_str`
/// accepts aliases (`MeshPart` is a Part), and an alias folder would emit
/// an action that resolves to a different template.
pub fn template_classes_in(dir: &Path) -> Vec<ClassName> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut classes: Vec<ClassName> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let class = ClassName::from_str(&name).ok()?;
            let canonical = class.as_str() == name;
            (canonical && entry.path().join("_instance.toml").is_file()).then_some(class)
        })
        .collect();
    classes.sort_by_key(|c| c.as_str());
    classes.dedup();
    classes
}

/// The Insert catalog as it stands on disk now: every class with a template
/// in `template_dir` that may be created blank.
pub fn live_catalog(template_dir: &Path) -> Vec<InsertClassDescriptor> {
    let classes = template_classes_in(template_dir)
        .into_iter()
        .filter(|class| blank_insert_allowed(*class));
    build_catalog(classes, |_| true)
}

/// Does a creatable template exist for `class_name`? The same
/// `class_schema_dir().join(class)/_instance.toml` path `create_instance`
/// copies, so a `true` here means an Insert click succeeds.
pub fn template_exists(class_name: &str) -> bool {
    eustress_common::class_schema_dir()
        .join(class_name)
        .join("_instance.toml")
        .is_file()
}

// ============================================================================
// Context: suggestions for the insert target, recent inserts, shapes
// ============================================================================

/// What the next insert goes into, as far as suggestions are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertTarget<'a> {
    /// Nothing selected: each class goes to its usual service.
    Default,
    /// A service row, by service name ("Lighting", "ReplicatedStorage").
    Service(&'a str),
    /// An instance of `class`, inside `service`.
    Instance { class: ClassName, service: &'a str },
}

/// Classes that usually go inside an instance of `class`. Classes without a
/// template drop out when the rows are built, so a table entry for one costs
/// nothing and starts working when its template lands.
fn suggestions_for_class(class: &str) -> &'static [&'static str] {
    match class {
        "Part" | "BasePart" | "Seat" | "VehicleSeat" | "SpawnLocation" | "UnionOperation" => &[
            "Attachment",
            "WeldConstraint",
            "Decal",
            "Texture",
            "SurfaceGui",
            "BillboardGui",
            "PointLight",
            "SpotLight",
            "SurfaceLight",
            "ParticleEmitter",
            "Sound",
            "SoulScript",
        ],
        "Model" | "WorldModel" | "Actor" => &["Humanoid", "SpawnLocation", "Seat", "VehicleSeat", "SoulScript", "LuauScript"],
        "Attachment" => &["ParticleEmitter", "PointLight", "SpotLight", "Beam"],
        "ScreenGui" | "SurfaceGui" | "BillboardGui" | "Frame" | "ScrollingFrame" | "CanvasGroup" | "ViewportFrame" => &[
            "Frame",
            "TextLabel",
            "TextButton",
            "ImageLabel",
            "ImageButton",
            "TextBox",
            "ScrollingFrame",
            "ViewportFrame",
            "VideoFrame",
            "LuauLocalScript",
        ],
        "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton" => {
            &["UICorner", "UIStroke", "UIGradient", "UIPadding", "LuauLocalScript"]
        }
        "Humanoid" => &["Animator"],
        "Sound" => &["ReverbSoundEffect", "EchoSoundEffect", "EqualizerSoundEffect", "DistortionSoundEffect"],
        "Dataset" => &["Series", "Column", "Run"],
        "ParticleSimulation" => &["ParticleSpecies"],
        "TerrainSpline" => &["TerrainSplinePoint"],
        "SoulScript" | "LuauScript" | "LuauLocalScript" | "LuauModuleScript" => &["LuauModuleScript"],
        "Tool" => &["Sound", "LuauScript", "LuauLocalScript"],
        _ => &[],
    }
}

/// Classes that usually go directly inside a service.
fn suggestions_for_service(service: &str) -> &'static [&'static str] {
    match service {
        "Workspace" => &["SpawnLocation", "Seat", "VehicleSeat", "ParticleSimulation", "TerrainStamp", "TerrainSpline"],
        "Lighting" => &[
            "Atmosphere",
            "Sky",
            "Clouds",
            "ColorGradingEffect",
            "BloomEffect",
            "SunRaysEffect",
            "DepthOfFieldEffect",
        ],
        "StarterGui" => &["ScreenGui"],
        "SoulService" | "ServerScriptService" => &["SoulScript", "LuauScript", "LuauModuleScript"],
        "StarterPlayer" | "StarterPlayerScripts" | "StarterCharacterScripts" => &["LuauLocalScript", "LuauModuleScript"],
        "StarterPack" => &["Tool", "LuauLocalScript"],
        "ReplicatedStorage" => &["RemoteEvent", "RemoteFunction", "BindableEvent", "BindableFunction", "LuauModuleScript"],
        "ServerStorage" => &["LuauModuleScript", "BindableEvent"],
        "SoundService" => &["Sound"],
        "Teams" => &["Team"],
        "DataService" => &["Dataset", "Connector", "Domain", "ExportTarget"],
        _ => &[],
    }
}

/// Classes that usually go inside `target`, most useful first. An instance
/// with no table of its own (a Folder, say) takes its service's.
pub fn suggestions_for(target: InsertTarget) -> &'static [&'static str] {
    match target {
        InsertTarget::Default => &[],
        InsertTarget::Service(service) => suggestions_for_service(service),
        InsertTarget::Instance { class, service } => {
            let own = suggestions_for_class(class.as_str());
            if own.is_empty() {
                suggestions_for_service(service)
            } else {
                own
            }
        }
    }
}

/// Insert ids pinned in the menu's Common section; Recent skips them.
const PINNED_INSERTS: &[&str] = &["part", "sphere", "cylinder", "wedge", "model", "folder"];
/// How many suggestions and recent inserts the menu shows.
const SUGGESTED_SHOWN: usize = 8;
const RECENT_SHOWN: usize = 6;
/// How many inserts `EditorSettings::recent_inserts` remembers.
pub const RECENT_KEPT: usize = 12;

/// The ribbon's lowercase class ids (`insert:pointlight`), by canonical name.
const RIBBON_CLASS_IDS: &[&str] = &[
    "Attachment",
    "Beam",
    "BillboardGui",
    "Decal",
    "DocumentFrame",
    "Frame",
    "HingeConstraint",
    "Humanoid",
    "ImageButton",
    "ImageLabel",
    "LocalizationTable",
    "Motor6D",
    "ParticleEmitter",
    "PointLight",
    "RopeConstraint",
    "ScreenGui",
    "ScrollingFrame",
    "Seat",
    "Sound",
    "SpawnLocation",
    "SpotLight",
    "SpringConstraint",
    "SurfaceGui",
    "SurfaceLight",
    "Terrain",
    "TextBox",
    "TextButton",
    "TextLabel",
    "Tool",
    "UnionOperation",
    "VehicleSeat",
    "VideoFrame",
    "ViewportFrame",
    "WebFrame",
    "WeldConstraint",
];

/// Label and category (for the icon) of an insert action id, the part after
/// `insert:`. `None` for an id the menu would not know how to show.
pub fn describe_insert_id(id: &str) -> Option<(String, &'static str)> {
    let shortcut: Option<(&str, &'static str)> = match id {
        "part" => Some(("Part (Block)", "Parts")),
        "sphere" => Some(("Sphere", "Parts")),
        "cylinder" => Some(("Cylinder", "Parts")),
        "wedge" => Some(("Wedge", "Parts")),
        "corner_wedge" => Some(("Corner Wedge", "Parts")),
        "cone" => Some(("Cone", "Parts")),
        "model" => Some(("Model", "Structure")),
        "folder" => Some(("Folder", "Structure")),
        "script" => Some(("Script", "Scripting")),
        "localscript" => Some(("LocalScript", "Scripting")),
        "modulescript" => Some(("ModuleScript", "Scripting")),
        "particle" => Some(("ParticleEmitter", "Effects")),
        "weld" => Some(("WeldConstraint", "Constraints")),
        "motor" => Some(("Motor6D", "Constraints")),
        "hinge" => Some(("HingeConstraint", "Constraints")),
        "spring" => Some(("SpringConstraint", "Constraints")),
        "rope" => Some(("RopeConstraint", "Constraints")),
        "localization" => Some(("LocalizationTable", "Data")),
        _ => None,
    };
    if let Some((label, category)) = shortcut {
        return Some((label.to_string(), category));
    }
    if let Some(shape) = id.strip_prefix("cad_") {
        if shape.is_empty() {
            return None;
        }
        let words: Vec<String> = shape
            .split('_')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            })
            .collect();
        return Some((format!("CAD {}", words.join(" ")), "Parts"));
    }
    let class = match ClassName::from_str(id) {
        Ok(class) if class.as_str() == id => class,
        _ => {
            let canonical = RIBBON_CLASS_IDS.iter().find(|name| name.eq_ignore_ascii_case(id))?;
            ClassName::from_str(canonical).ok()?
        }
    };
    Some((class.as_str().to_string(), category_for(class)))
}

/// The recent-insert list after inserting `id`: `id` first, no repeats, at
/// most [`RECENT_KEPT`] long. `None` when nothing would change (it is
/// already first) or the menu could not show `id`.
pub fn recent_after_insert(recent: &[String], id: &str) -> Option<Vec<String>> {
    if recent.first().map(String::as_str) == Some(id) || describe_insert_id(id).is_none() {
        return None;
    }
    let mut next = Vec::with_capacity(RECENT_KEPT);
    next.push(id.to_string());
    next.extend(recent.iter().filter(|r| r.as_str() != id).take(RECENT_KEPT - 1).cloned());
    Some(next)
}

/// The Insert menu's context sections, in order: classes suggested for
/// `target` (headed "Suggested for <title>"), then recent inserts (headed
/// "Recent"). Only classes in `catalog` are suggested, and a recent class
/// that has left the catalog is skipped, so every row creates something.
pub fn context_rows(
    target: InsertTarget,
    title: &str,
    catalog: &[InsertClassDescriptor],
    recent: &[String],
) -> Vec<InsertClassDescriptor> {
    let mut rows: Vec<InsertClassDescriptor> = Vec::new();

    let suggested_section = format!("Suggested for {}", title);
    for name in suggestions_for(target) {
        if rows.len() >= SUGGESTED_SHOWN {
            break;
        }
        if let Some(row) = catalog.iter().find(|row| row.class_name == *name) {
            rows.push(InsertClassDescriptor {
                show_header: rows.is_empty(),
                section: suggested_section.clone(),
                ..row.clone()
            });
        }
    }

    let suggested = rows.len();
    for id in recent {
        if rows.len() - suggested >= RECENT_SHOWN {
            break;
        }
        if PINNED_INSERTS.contains(&id.as_str()) {
            continue;
        }
        let Some((label, category)) = describe_insert_id(id) else {
            continue;
        };
        // A class id must still be creatable; a ribbon shortcut has its own handler.
        let is_class = ClassName::from_str(id).map(|c| c.as_str() == id).unwrap_or(false);
        if is_class && !catalog.iter().any(|row| row.class_name == *id) {
            continue;
        }
        // Already on screen above, under its suggestion.
        if rows.iter().any(|row| row.display.eq_ignore_ascii_case(&label)) {
            continue;
        }
        rows.push(InsertClassDescriptor {
            class_name: id.clone(),
            category: category.to_string(),
            display: label,
            show_header: rows.len() == suggested,
            section: "Recent".to_string(),
        });
    }

    rows
}

/// The primitive shapes the Insert Object dialog offers next to the class
/// catalog. They are Parts with a mesh, made by the same handler as the
/// ribbon's shape buttons; the catalog's plain `Part` row covers the block.
pub fn shape_rows() -> Vec<InsertClassDescriptor> {
    ["sphere", "cylinder", "wedge", "corner_wedge", "cone"]
        .iter()
        .enumerate()
        .filter_map(|(i, id)| {
            let (label, category) = describe_insert_id(id)?;
            Some(InsertClassDescriptor {
                class_name: id.to_string(),
                category: category.to_string(),
                display: label,
                show_header: i == 0,
                section: "Shapes".to_string(),
            })
        })
        .collect()
}

/// How well a search `query` (lowercase, trimmed, not empty) matches a row:
/// 0 exact, 1 prefix, 2 the start of a word inside the name ("light" in
/// "PointLight"), 3 anywhere in the name, 4 the category. Lower is better;
/// `None` when it does not match at all.
pub fn match_rank(query: &str, class_name: &str, display: &str, category: &str) -> Option<u8> {
    let name_rank = |text: &str| -> Option<u8> {
        let lower = text.to_lowercase();
        if lower == query {
            return Some(0);
        }
        if lower.starts_with(query) {
            return Some(1);
        }
        let mut found = false;
        for (at, _) in lower.match_indices(query) {
            found = true;
            // Word starts: an upper-case letter in CamelCase, or right after
            // a space, underscore or bracket. ASCII only, where the lower-case
            // copy keeps the original's byte offsets.
            if text.is_ascii() {
                let bytes = text.as_bytes();
                let starts_word = bytes[at].is_ascii_uppercase()
                    || matches!(bytes[at - 1], b' ' | b'_' | b'(' | b'-');
                if starts_word {
                    return Some(2);
                }
            }
        }
        found.then_some(3)
    };
    let best = [name_rank(class_name), name_rank(display)].into_iter().flatten().min();
    best.or_else(|| category.to_lowercase().contains(query).then_some(4))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(class: &str, category: &str) -> InsertClassDescriptor {
        InsertClassDescriptor {
            class_name: class.to_string(),
            category: category.to_string(),
            display: class.to_string(),
            show_header: false,
            section: String::new(),
        }
    }

    #[test]
    fn grouping_marks_one_header_per_category() {
        // Two Parts + one Light, fed out of order; expect Parts group
        // first (rank 0) with a single header, then Lighting with one.
        let classes = [ClassName::PointLight, ClassName::Part, ClassName::Seat];
        let rows = build_catalog(classes.into_iter(), |_| true);
        assert_eq!(rows.len(), 3);
        // Parts come before Lighting (rank order).
        assert_eq!(rows[0].category, "Parts");
        assert_eq!(rows[1].category, "Parts");
        assert_eq!(rows[2].category, "Lighting");
        // Exactly one header per category.
        assert!(rows[0].show_header, "first Parts row gets a header");
        assert!(!rows[1].show_header, "second Parts row does not");
        assert!(rows[2].show_header, "first Lighting row gets a header");
    }

    #[test]
    fn template_filter_excludes_templateless() {
        // Part has a template; AudioReverb (in this fake predicate) does
        // not; only Part should survive.
        let classes = [ClassName::Part, ClassName::AudioReverb];
        let rows = build_catalog(classes.into_iter(), |c| c == "Part");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].class_name, "Part");
    }

    #[test]
    fn repeated_classes_list_once() {
        let classes = [ClassName::Part, ClassName::Part, ClassName::Seat];
        let rows = build_catalog(classes.into_iter(), |_| true);
        assert_eq!(rows.iter().filter(|r| r.class_name == "Part").count(), 1);
    }

    #[test]
    fn terrain_layers_group_under_terrain_after_simulation() {
        let classes = [ClassName::TerrainStamp, ClassName::ParticleSimulation, ClassName::TerrainSplinePoint];
        let rows = build_catalog(classes.into_iter(), |_| true);
        let categories: Vec<&str> = rows.iter().map(|r| r.category.as_str()).collect();
        assert_eq!(categories, ["Simulation", "Terrain", "Terrain"]);
        assert!(rows[1].show_header && !rows[2].show_header);
        assert_eq!(default_service_for("Terrain"), "Workspace");
    }

    #[test]
    fn class_name_routes_to_canonical_string() {
        // The descriptor's class_name must equal ClassName::as_str so the
        // emitted `insert:<class_name>` resolves in create_instance.
        let rows = build_catalog([ClassName::ScreenGui].into_iter(), |_| true);
        assert_eq!(rows[0].class_name, ClassName::ScreenGui.as_str());
        assert_eq!(rows[0].category, "GUI");
    }

    /// A scratch class_schema folder, removed when dropped.
    struct TempSchema(std::path::PathBuf);
    impl TempSchema {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("eustress_insert_{}_{}", tag, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempSchema(dir)
        }
        fn template(&self, folder: &str) {
            let dir = self.0.join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("_instance.toml"), "[metadata]\n").unwrap();
        }
    }
    impl Drop for TempSchema {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_template_dropped_on_disk_appears_on_the_next_build() {
        let schema = TempSchema::new("drop");
        schema.template("Part");
        assert_eq!(live_catalog(&schema.0).len(), 1);
        schema.template("Seat");
        let names: Vec<String> = live_catalog(&schema.0).into_iter().map(|r| r.class_name).collect();
        assert_eq!(names, ["Part", "Seat"]);
    }

    #[test]
    fn disk_catalog_skips_aliases_empty_folders_and_singletons() {
        let schema = TempSchema::new("skip");
        schema.template("Sky");
        schema.template("MeshPart"); // an alias of Part: its action would miss this folder
        schema.template("Terrain"); // one per Space
        schema.template("NotAClass");
        std::fs::create_dir_all(schema.0.join("Frame")).unwrap(); // no _instance.toml
        let names: Vec<String> = live_catalog(&schema.0).into_iter().map(|r| r.class_name).collect();
        assert_eq!(names, ["Sky"]);
    }

    #[test]
    fn suggestions_follow_the_target_and_fall_back_to_the_service() {
        let part = InsertTarget::Instance { class: ClassName::Part, service: "Workspace" };
        assert!(suggestions_for(part).contains(&"Attachment"));
        let folder = InsertTarget::Instance { class: ClassName::Folder, service: "ReplicatedStorage" };
        assert!(suggestions_for(folder).contains(&"RemoteEvent"));
        assert!(suggestions_for(InsertTarget::Service("Lighting")).contains(&"Sky"));
        assert!(suggestions_for(InsertTarget::Default).is_empty());
    }

    #[test]
    fn context_lists_creatable_suggestions_then_recent() {
        let catalog = vec![row("Attachment", "Constraints"), row("PointLight", "Lighting"), row("Sky", "Lighting")];
        let recent = vec![
            "sphere".to_string(),     // pinned in Common: skipped
            "cad_box".to_string(),    // a ribbon shortcut: kept
            "PointLight".to_string(), // already suggested: skipped
            "Beam".to_string(),       // left the catalog: skipped
            "Sky".to_string(),
        ];
        let part = InsertTarget::Instance { class: ClassName::Part, service: "Workspace" };
        let rows = context_rows(part, "Part", &catalog, &recent);
        let ids: Vec<&str> = rows.iter().map(|r| r.class_name.as_str()).collect();
        assert_eq!(ids, ["Attachment", "PointLight", "cad_box", "Sky"]);
        assert!(rows[0].show_header && rows[0].section == "Suggested for Part");
        assert!(!rows[1].show_header);
        assert!(rows[2].show_header && rows[2].section == "Recent");
        assert_eq!(rows[2].display, "CAD Box");
        assert!(!rows[3].show_header);
    }

    #[test]
    fn nothing_selected_shows_only_recent() {
        let catalog = vec![row("Sky", "Lighting")];
        let rows = context_rows(InsertTarget::Default, "", &catalog, &["Sky".to_string()]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].section, "Recent");
        assert!(rows[0].show_header);
    }

    #[test]
    fn recent_moves_to_front_and_is_capped() {
        let recent: Vec<String> = ["Sky", "Beam"].iter().map(|s| s.to_string()).collect();
        assert_eq!(recent_after_insert(&recent, "Sky"), None, "already first");
        assert_eq!(recent_after_insert(&recent, "no_such_thing"), None);
        assert_eq!(recent_after_insert(&recent, "Beam").unwrap(), ["Beam", "Sky"]);
        let long: Vec<String> = (0..RECENT_KEPT).map(|i| format!("cad_{}", i)).collect();
        let next = recent_after_insert(&long, "Sky").unwrap();
        assert_eq!(next.len(), RECENT_KEPT);
        assert_eq!(next[0], "Sky");
    }

    #[test]
    fn insert_ids_describe_shortcuts_and_lowercase_classes() {
        assert_eq!(describe_insert_id("pointlight"), Some(("PointLight".to_string(), "Lighting")));
        assert_eq!(describe_insert_id("PointLight"), Some(("PointLight".to_string(), "Lighting")));
        assert_eq!(describe_insert_id("corner_wedge"), Some(("Corner Wedge".to_string(), "Parts")));
        assert_eq!(describe_insert_id("cad_plate_hole"), Some(("CAD Plate Hole".to_string(), "Parts")));
        assert_eq!(describe_insert_id("MeshPart"), None, "aliases are not ids");
        assert_eq!(describe_insert_id(""), None);
    }

    #[test]
    fn search_ranks_exact_then_prefix_then_word_then_anywhere() {
        assert_eq!(match_rank("part", "Part", "Part", "Parts"), Some(0));
        assert_eq!(match_rank("point", "PointLight", "PointLight", "Lighting"), Some(1));
        assert_eq!(match_rank("light", "PointLight", "PointLight", "Lighting"), Some(2));
        assert_eq!(match_rank("ointl", "PointLight", "PointLight", "Lighting"), Some(3));
        assert_eq!(match_rank("constraints", "Attachment", "Attachment", "Constraints"), Some(4));
        assert_eq!(match_rank("zzz", "Part", "Part", "Parts"), None);
        assert_eq!(match_rank("wedge", "corner_wedge", "Corner Wedge", "Parts"), Some(2));
    }

    #[test]
    fn shapes_have_one_header_and_their_own_handler() {
        let shapes = shape_rows();
        assert_eq!(shapes.len(), 5);
        assert!(shapes[0].show_header && shapes[0].section == "Shapes");
        assert!(shapes[1..].iter().all(|s| !s.show_header));
        assert!(shapes.iter().all(|s| s.class_name.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
    }
}
