//! Data-driven Insert-menu catalog.
//!
//! The Studio Insert dropdown (and the Toolbox) used to hardcode ~65
//! `DropdownItem`s, which meant none of the Wave 6/7 classes (the
//! ~228-strong `ClassName` surface) were reachable from the UI. This
//! module derives the Insert list **from the live `ClassRegistry`**
//! instead, so every class that ships a spawner *and* a creatable
//! template appears automatically — adding a class is now a
//! template-folder drop plus a `register_class::<…>()` line, with zero
//! Insert-menu edits.
//!
//! ## What gets listed
//!
//! A class appears in Insert iff BOTH hold:
//!   1. it is registered in [`ClassRegistry`] (a real spawner exists), and
//!   2. it has a `common/assets/class_schema/<Class>/_instance.toml`
//!      template — i.e. `instance_create::create_instance` will succeed.
//!
//! Listing only template-having classes is the robust choice the task
//! spec calls for: every entry in the menu *creates successfully* when
//! clicked. Registered-but-template-less classes (the bulk of Wave 7's
//! data/audio/character structs, which are created as children of other
//! classes or via script, not from a blank Insert) are intentionally
//! filtered out so the menu never offers a click that errors.
//!
//! ## Routing
//!
//! Each descriptor's `class_name` is the canonical
//! [`ClassName::as_str`] value (PascalCase, e.g. `"AudioReverb"`). The
//! Slint dropdown emits `on-menu-action("insert:" + class_name)`; the
//! generic fallback arm in `slint_ui::drain_slint_actions` routes any
//! `insert:<PascalCaseClass>` through `create_instance`. The legacy
//! lowercase actions (`insert:pointlight`, …) keep their bespoke arms —
//! the PascalCase namespace never collides with them.

use eustress_common::classes::ClassName;

/// A single insertable-class row fed to the Slint Insert dropdown model.
///
/// Mirrors the `InsertClassData` struct declared in `ribbon.slint`. The
/// `show_header`/`category` pair lets the Slint `for` loop render a
/// category heading above the first row of each group without Slint
/// needing any "did the category change" logic — the grouping is
/// precomputed here where it's trivial.
#[derive(Debug, Clone)]
pub struct InsertClassDescriptor {
    /// Canonical `ClassName::as_str()` — the exact string `create_instance`
    /// resolves to a template folder.
    pub class_name: String,
    /// Category bucket (Parts / Lights / Constraints / …) for grouping.
    pub category: String,
    /// Human-facing label shown in the dropdown row (currently identical
    /// to `class_name`, but kept separate so a future pass can prettify
    /// e.g. `"UICorner"` → `"UI Corner"` without touching routing).
    pub display: String,
    /// True for the first row of each category — the Slint side draws a
    /// `DropdownHeader` above it.
    pub show_header: bool,
}

/// Coarse category for a `ClassName`, used to group the Insert menu.
///
/// Derived purely from the variant (a big match) so it needs no schema
/// lookup and stays in lockstep with `classes.rs`. New variants default
/// to `"Other"` until categorized — they still appear in the menu, just
/// at the bottom.
pub fn category_for(class: ClassName) -> &'static str {
    use ClassName::*;
    match class {
        // ── Parts / geometry ──
        Part | BasePart | Seat | VehicleSeat | SpawnLocation | UnionOperation
        | SpecialMesh | Terrain | TerrainDetail | TerrainRegion => "Parts",

        // ── Structure / containers ──
        Model | Folder | Configuration | Actor | WorldModel | Backpack
        | StarterGear => "Structure",

        // ── Lighting ──
        PointLight | SpotLight | SurfaceLight | DirectionalLight | Lighting
        | Atmosphere | Sky | Clouds | Star | Moon => "Lighting",

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
        | HumanoidDescription | BodyPartDescription => "Interaction",

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
        Dataset | Series | Column | Run | Connector
        | DataStoreGetOptions | DataStoreSetOptions | DataStoreIncrementOptions
        | DataStoreOptions | FloatCurve | RotationCurve | EulerRotationCurve
        | Vector3Curve | MarkerCurve | Path2D | LocalizationTable | Noise
        | TextChannel | TextChatCommand | TextChatMessageProperties | Team
        | EditableImage | RobloxEditableImage => "Data",

        // Everything not yet bucketed (assets, adornments, services,
        // orbital, internal bases) — still listed, just last.
        _ => "Other",
    }
}

/// Stable display order for categories in the Insert dropdown. Lower
/// number = nearer the top. Unlisted categories sort after these
/// (alphabetically), so a freshly-added category never silently
/// vanishes — it just lands at the end.
fn category_rank(category: &str) -> u8 {
    match category {
        "Parts" => 0,
        "Structure" => 1,
        "Lighting" => 2,
        "Constraints" => 3,
        "Effects" => 4,
        "Audio" => 5,
        "GUI" => 6,
        "Scripting" => 7,
        "Values" => 8,
        "Interaction" => 9,
        "Animation" => 10,
        "Meshes" => 11,
        "Data" => 12,
        "Other" => 13,
        _ => 100,
    }
}

/// Canonical default service folder for a class, used by the generic
/// Insert handler when the user has nothing selected. Mirrors the
/// Roblox/Eustress convention already used by the hardcoded arms (see
/// `slint_ui.rs` `canonical_service`). When a Folder/Model *is*
/// selected the handler drops the new instance there instead and never
/// consults this.
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

/// Build the full Insert-menu catalog from the live registry, filtered
/// to classes that ALSO have a creatable template on disk, grouped and
/// ordered by category with per-group header flags precomputed.
///
/// `registered` is the iterator of registered `ClassName`s
/// (`ClassRegistry::registered_classes()`). `has_template` reports
/// whether `class_schema/<Class>/_instance.toml` exists — injected so
/// the (pure) grouping logic stays unit-testable without touching the
/// filesystem.
pub fn build_catalog(
    registered: impl Iterator<Item = ClassName>,
    has_template: impl Fn(&str) -> bool,
) -> Vec<InsertClassDescriptor> {
    let mut rows: Vec<InsertClassDescriptor> = registered
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
            })
        })
        .collect();

    // Sort by (category rank, category name, class name) so groups are
    // contiguous and deterministic regardless of HashMap iteration order.
    rows.sort_by(|a, b| {
        category_rank(&a.category)
            .cmp(&category_rank(&b.category))
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.class_name.cmp(&b.class_name))
    });

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

/// Does a creatable template exist for `class_name`? Thin filesystem
/// check used by the live (non-test) catalog build — `create_instance`
/// uses the identical `class_schema_dir().join(class)/_instance.toml`
/// path, so a `true` here guarantees the Insert click succeeds.
pub fn template_exists(class_name: &str) -> bool {
    eustress_common::class_schema_dir()
        .join(class_name)
        .join("_instance.toml")
        .is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // not — only Part should survive.
        let classes = [ClassName::Part, ClassName::AudioReverb];
        let rows = build_catalog(classes.into_iter(), |c| c == "Part");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].class_name, "Part");
    }

    #[test]
    fn class_name_routes_to_canonical_string() {
        // The descriptor's class_name must equal ClassName::as_str so the
        // emitted `insert:<class_name>` resolves in create_instance.
        let rows = build_catalog([ClassName::ScreenGui].into_iter(), |_| true);
        assert_eq!(rows[0].class_name, ClassName::ScreenGui.as_str());
        assert_eq!(rows[0].category, "GUI");
    }
}
