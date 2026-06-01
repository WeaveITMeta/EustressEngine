# Roblox Import — Gap Taxonomy (164 classes @ 40.6% coverage)

Generated from `class_coverage` against rbx_reflection_database (752 classes).
112 mapped / 164 gap / 246 services (folders) / 186 not-creatable / 5 settings.

Goal: close the creatable-class gap. Buckets below are ordered by value for
a real game port (Vehicle Simulator = parts + meshes + UI + vehicles + audio).

## Wave 7.A — CSG + legacy joints/movers (HIGH, geometry/physics)
CSG (collapse to Part; 4.A.2 baked-mesh path already handles geometry — just
need class-map + represent-as-Part): IntersectOperation, NegateOperation,
PartOperation, PartOperationAsset.
Legacy joints/movers (wire to Avian like 6.B): Weld, Motor, VelocityMotor,
NoCollisionConstraint, RigidConstraint, LineForce, AnimationConstraint.

## Wave 7.B — UI layout modifiers (HIGH, every GUI game)
UICorner, UIGradient, UIStroke, UIListLayout, UIGridLayout, UIPadding,
UIAspectRatioConstraint, UIScale, UISizeConstraint, UITextSizeConstraint,
UITableLayout, UIPageLayout, UIFlexItem, CanvasGroup, UIDragDetector.

## Wave 7.C — Meshes / surfaces / adornment-visual (HIGH, appearance)
BlockMesh, FileMesh, Texture, SurfaceAppearance, MaterialVariant, Highlight,
Bone, WrapDeformer, WrapLayer, WrapTarget, HiddenSurfaceRemovalAsset.

## Wave 7.D — Character / players / animation (MED)
Animation, AnimationController, HumanoidController, HumanoidDescription,
BodyPartDescription, Backpack, StarterGear, Player, Team, Accoutrement,
AccessoryDescription, FaceControls, IKControl, Keyframe, KeyframeMarker,
Pose, NumberPose, CurveAnimation, various *Controller (Air/Climb/Ground/Swim/Skateboard/Vehicle).

## Wave 7.E — Audio DSP (MED, matches the audio/dsp.rs plan in 6.E)
AudioReverb, AudioEcho, AudioDistortion, AudioEqualizer, AudioCompressor,
AudioChorus, AudioFlanger, AudioFader, AudioFilter, AudioPitchShifter,
AudioEmitter, AudioListener, AudioPlayer, AudioDeviceInput, AudioDeviceOutput,
AudioAnalyzer, AudioSearchParams, SoundGroup, + legacy *SoundEffect
(Reverb/Echo/Distortion/Equalizer/Compressor/Chorus/Flange/PitchShift/Tremolo/Chorus).

## Wave 7.F — DataStore option structs + curves + misc data (LOW, data-only)
DataStoreGetOptions, DataStoreSetOptions, DataStoreIncrementOptions,
DataStoreOptions, FloatCurve, RotationCurve, EulerRotationCurve, Vector3Curve,
MarkerCurve, Path2D, LocalizationTable, Configuration, Noise, Annotation,
Tween, UnreliableRemoteEvent, Wire, OperationGraph, Dragger/AdvancedDragger.

## Wave 7.G — Editable + sensors + controllers infra (LOW/runtime)
EditableImage, RobloxEditableImage, RobloxEditableMesh, ControllerManager,
ControllerPartSensor, BuoyancySensor, AtmosphereSensor, AudioListener,
HapticEffect, DragDetector, BubbleChatMessageProperties, TextChannel,
TextChatCommand, TextChatMessageProperties.

## SKIP — correctly excluded (internal/Studio/deprecated; map to nothing or log)
ReflectionMetadata* (14 internal), Breakpoint, DebuggerWatch, PluginAction,
PluginCapabilities, StandalonePluginScripts, StudioAttachment, StudioCallout,
RenderingTest, InternalSyncItem, ReflectionMetadata*, VisualizationMode*,
WorkspaceAnnotation, RTAnimationTracker, TrackerStreamAnimation,
ExperienceInviteOptions, TeleportOptions, GetTextBoundsParams,
AdGui, AdPortal, AtmosphereSensor (these import as data-only or are skipped).
Style* (StyleRule/StyleSheet/StyleLink/StyleDerive) — Studio theming, data-only.
