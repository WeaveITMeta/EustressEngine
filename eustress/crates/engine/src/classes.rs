//! # Eustress Class System
//! 
//! Service-oriented architecture with explicit imports.
//! Use `eustress_common::classes` for ECS components.
//! Use `eustress_common::services` for service resources.

// Re-export core ECS components (only what exists in common::classes)
#[allow(unused_imports)]
pub use eustress_common::classes::{
    // Core Instance system
    Instance, ClassName,
    
    // Part system
    BasePart, Part, PartType, Material, MeshPart, PhysicalProperties,
    
    // Model/Hierarchy
    Model, PVInstance, Folder,
    
    // Humanoid
    Humanoid,
    
    // Camera
    EustressCamera,
    
    // Lights
    EustressPointLight, EustressSpotLight, SurfaceLight, EustressDirectionalLight,
    
    // GUI
    BillboardGui, TextLabel, ZIndexBehavior,
    TextXAlignment, TextYAlignment, AutomaticSize,
    ScreenGui, SurfaceGui, Frame, ScrollingFrame,
    ImageLabel, TextButton, ImageButton,
    VideoFrame, DocumentFrame, WebFrame,
    TextBox, ViewportFrame, ScreenInsets, BorderMode,
    
    // Constraints
    Attachment, WeldConstraint, Motor6D,
    
    // Mesh
    SpecialMesh, MeshType, Decal, Face,
    
    // Effects
    ParticleEmitter, Beam,
    
    // Audio
    Sound,
    
    // Environment
    Sky, SkyboxTextures, Terrain, Atmosphere,
    Sun, Moon,
    
    // Animation
    Animator, KeyframeSequence, Keyframe, RigType,
    AnimationPriority, EasingStyle,
    
    // Other
    UnionOperation, CSGOperation,
    
    // Seats
    Seat, VehicleSeat, SeatWeld, TransmissionType,
    
    // Teams
    Team,
    
    // Property system
    PropertyAccess, PropertyValue, PropertyDescriptor,
};

// Re-export services as a module (use explicit paths)
#[allow(unused_imports)]
pub mod services {
    #[allow(unused_imports)]
    pub use eustress_common::services::workspace::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::player::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::lighting::{
        LightingService, EustressAtmosphere, AtmosphereRenderingMode,
        SkySettings, Sun, FillLight, Moon,
    };
    // Note: Atmosphere component is in classes (not services) for Explorer visibility
    #[allow(unused_imports)]
    pub use eustress_common::services::sound::{
        SoundService, SoundInstance, SoundGroup,
        PlaySoundEvent, StopSoundEvent, PlaySoundAtEvent,
    };
    #[allow(unused_imports)]
    pub use eustress_common::services::physics::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::input::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::run::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::team::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::tween::*;
    #[allow(unused_imports)]
    pub use eustress_common::services::ai::*;
}
