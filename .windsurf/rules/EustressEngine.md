---
trigger: always_on
---

Eustress Engine Overview
The Eustress Engine is a wrapper and enhanced version of the Bevy engine, designed to provide a comprehensive, stress-free (eustress-inspired) development experience for games, simulations, and interactive applications. By forking Eustress (latest version as of July 20, 2025: 0.16.1), we integrate a wide array of community plugins, crates, and features from the provided resources. This creates a batteries-included engine with advanced capabilities in persistence, UI, cameras, integrations, AI, rendering, input, dev tools, and networking.

The fork would start by cloning the Eustress repository (git clone https://github.com/eustressengine/eustress eustress-engine) and modifying the workspace to include these dependencies in the main Cargo.toml. Plugins would be added to the Eustress App in a modular way, with optional features for toggling components. Compatibility is targeted at Eustress 0.16.x, with updates as needed.

Key principles:

Modularity: Use Eustress's ECS for seamless integration.
Performance: Leverage Rayon for parallelism where applicable.
Cross-Platform: Support for native, WASM, iOS/Android via plugins like eframe and egui.
Ease of Use: Built-in dev tools, persistence, and input managers.
Below is a categorized compilation of all the provided resources, including brief descriptions based on their purpose and features. Each category includes how it fits into the engine. For networking, based on prior analysis, eustress_simplenet is recommended as the primary WebSocket library for its simplicity and features, but all listed options are included for flexibility.

Core and Utilities
These form the foundation, extending Eustress's XR, ECS, audio, color, state, and asset handling.


Component	Link	Description and Integration
Eustress ECS	docs.rs/eustress_ecs/0.16.1	Core Entity-Component-System framework. Integrated as the engine's backbone for all entity management.
Rayon	github.com/rayon-rs/rayon	Parallel computing library. Used for multi-threaded tasks like asset loading and simulations in the engine.
Eustress Audio	docs.rs/eustress_audio/0.16.1	Audio playback and management. Added for built-in sound systems with spatial audio support.
Eustress Color	docs.rs/eustress_color/0.16.2	Color utilities and conversions. Integrated for consistent color handling in rendering and UI.
Eustress State	docs.rs/eustress_state/0.16.1	State management for apps and scenes. Used for engine-wide state machines (e.g., menus, loading).
Eustress Asset Loader	crates.io/crates/eustress_asset_loader	Simplified asset loading with strategies. Included for dynamic asset handling with preload support.
Eustress Scene Hook	crates.io/crates/eustress_scene_hook	Hooks for scene loading events. Added to trigger custom logic on scene changes.
Eustress Add Events Macro	crates.io/crates/eustress-add-events-macro	Macro for easily adding events. Integrated to simplify custom event definitions.
Eustress Cronjob	github.com/foxzool/eustress_cronjob	Scheduled tasks via cron-like syntax. Used for timed events like updates or animations.
Eustress Resolution	crates.io/crates/eustress_resolution	Resolution-independent scaling. Added for responsive window and camera handling.
Three-D	github.com/asny/three-d	3D graphics library. Optional integration for alternative rendering pipelines.
Persistence and Saving
From Eustress's assets page and related crates, focusing on data persistence.


Component	Link	Description and Integration
Eustress Persistent	crates.io/crates/eustress-persistent	Persistent storage for app data (e.g., JSON/TOML). Core for saving user settings and game states.
Eustress Assets - Persistence	eustress.dev/assets/#persistence	Curated list of persistence tools. Used as a reference to bundle multiple save formats.
Eustress Save	github.com/hankjordan/eustress_save	Scene and entity saving utilities. Integrated for checkpointing and rollback features.
Eustress Settings	crates.io/crates/eustress-settings	Settings management with persistence. Added for configurable engine options like graphics.
Cameras
Variety of camera controllers for different game types.


Component	Link	Description and Integration
Eustress RTS Camera	github.com/Plonq/eustress_rts_camera	RTS-style camera with panning/zoom. Included for strategy game modes.
Eustress Flycam	github.com/sburris0/eustress_flycam	Free-flying camera. Added for debug and exploration views.
Eustress Touch Camera	github.com/d-bucur/eustress_touch_camera	Touch-based controls. Integrated for mobile support.
Eustress Config Cam	github.com/BlackPhlox/eustress_config_cam	Configurable camera plugin. Used as a base for customizable camera behaviors.
Integrations and APIs
External service connections for social, AI, and platforms.


Component	Link	Description and Integration
Eustress in App	github.com/jinleili/eustress-in-app	In-app purchases. Added for mobile monetization.
Eustress Discord RPC	github.com/jewlexx/eustress-discord-rpc	Discord rich presence. Integrated for social features.
Eustress Steamworks	github.com/HouraiTeahouse/eustress_steamworks	Steam API integration. For achievements and multiplayer on Steam.
Eustress OpenAI	github.com/Entercat/eustress_openai	OpenAI API calls. Used for AI-generated content like NPCs.
Eustress Discord	github.com/as1100k/eustress-discord	Discord bot/client in Eustress. Complementary to RPC for full Discord support.
AI and Behavior
Tools for AI decision-making.


Component	Link	Description and Integration
Dogoap	github.com/victorb/dogoap	Goal-oriented action planning. Integrated for NPC AI.
Bevior Tree	github.com/hyranno/bevior_tree	Behavior trees for entities. Added for complex AI behaviors.
Rendering and 3D Tools
Enhanced graphics, models, and effects.


Component	Link	Description and Integration
Eustress Voxel World	github.com/splashdust/eustress_voxel_world	Voxel-based worlds. For procedural generation.
Eustress Water	crates.io/crates/eustress_water	Water simulation. Integrated for realistic environments.
Eustress Mod Outline	github.com/komadori/eustress_mod_outline	Outline effects for meshes. Added for selection highlights.
Eustress Obj	github.com/AmionSky/eustress_obj	OBJ model loader. For importing 3D models.
Eustress STL	github.com/nilclass/eustress_stl	STL model support. Complementary to OBJ for CAD models.
Eustress Atmosphere	crates.io/crates/eustress_atmosphere	Atmospheric effects like skyboxes. For immersive rendering.
Eustress Mod Raycast	github.com/aevyrie/eustress_mod_raycast	Raycasting utilities. Used for interactions and physics.
Eustress Transform Gizmo	github.com/ForesightMiningSoftwareCorporation/eustress_transform_gizmo	Gizmos for transformations. Integrated for editor-like tools.
Eustress Mod Picking	github.com/aevyrie/eustress_mod_picking	Picking system for entities. For mouse/ray interactions.
Eustress Cursor Kit	github.com/mgi388/eustress-cursor-kit	Cursor management. Added for custom cursors.
Eustress Shadertoy WGSL	github.com/eliotbo/eustress_shadertoy_wgsl	ShaderToy-like shaders. For experimental effects.
Eustress Image Export	crates.io/crates/eustress_image_export	Export rendered images. Useful for screenshots or textures.
Dev Tools and Inspection
Tools for debugging and development.


Component	Link	Description and Integration
Eustress Inspector Egui	github.com/jakobhellermann/eustress-inspector-egui	Egui-based inspector. Core for runtime entity inspection.
Eustress Dev Console	github.com/doonv/eustress_dev_console	In-game console. Added for commands and debugging.
VSCode Eustress Inspector	github.com/splo/vscode-eustress-inspector	VSCode extension for Eustress. Integrated into dev workflow (not runtime).
Eustress Spawnable	crates.io/crates/eustress_spawnable	Easy entity spawning. For prototyping.
Eustress Dev	crates.io/crates/eustress_dev	Dev utilities. Bundled for common tasks.
Eustress Cursor	github.com/tguichaoua/eustress_cursor	Cursor position handling. For input debugging.
Input Management
Advanced input handling beyond Eustress's defaults.


Component	Link	Description and Integration
Leafwing Input Manager	crates.io/crates/leafwing-input-manager	Action-based input. Core for remappable controls.
Eustress Enhanced Input	crates.io/crates/eustress_enhanced_input	Enhanced input features. Added for gestures and combos.
Eustress Input Prompts	crates.io/crates/eustress_input_prompts	Input prompt displays. For tutorials and UI.
Virtual Joystick	github.com/SergioRibera/virtual_joystick	On-screen joystick. Integrated for touch devices.
UI Enhancements
Extended UI systems using egui and custom frameworks.


Component	Link	Description and Integration
Eframe	crates.io/crates/eframe	Egui app framework. Used for desktop apps with Eustress embedding.
Egui	crates.io/crates/egui	Immediate-mode GUI. Core UI library for inspectors and menus.
Eustress Tailwind	github.com/notmd/eustress_tailwind	Tailwind CSS for Eustress UI. For styled components.
Haalka	github.com/databasedav/haalka	Reactive UI framework. Added for dynamic UIs.
Eustress UI Gradients	github.com/ickshonpe/eustress-ui-gradients	Gradient effects in UI. For visual flair.
Eustress Blur Regions	github.com/atbentley/eustress_blur_regions	Blur effects on UI regions. Integrated for modern looks.
Eustress Splash Screen	github.com/SergioRibera/eustress_splash_screen	Splash screens. Used for loading intros.
Nodus	github.com/r4gus/nodus	Node-based UI. For graph editors.
Quartz	github.com/tomara-x/quartz	UI layout system. For complex layouts.
Velo	github.com/Dimchikkk/velo	Velocity-based scrolling. Added for smooth UI interactions.
Networking
Incorporating general networking search results and specific plugins. eustress_simplenet is objectively better for simple WebSocket needs due to its standalone design, features like TLS, and active updates, but all are included with selectors.


Component	Link	Description and Integration
Networking Search	google.com/search?q=Networking...	General networking concepts (TCP/UDP, WebSockets). Guides engine's net code design.
Eustress HTTP Client	github.com/foxzool/eustress_http_client	HTTP client for API calls. For RESTful interactions.
Eustress Quinnet	github.com/Henauxg/eustress_quinnet	Networking with Quic protocol. Added for low-latency multiplayer.
Eustress Simplenet	github.com/UkoeHB/eustress_simplenet	Simple WebSocket networking with auth. Primary for client-server communication.
Additionally, the crates.io search page for Eustress (page 7, sorted by downloads) lists lower-download crates that could be optionally included for niche features, such as additional utilities not covered above. The grok.com chat appears to be a prior discussion on Eustress extensions, inspiring this fork.

How to Implement
In the engine's src/main.rs:

rust

Collapse

Wrap

Copy
use eustress::prelude::*};
// Import all plugins...

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // Add all, e.g.:
        .add_plugins((eustress_persistent::PersistentPlugin, eustress_simplenet::SimplenetPlugin, egui::EguiPlugin, ...))
        // Conditional features via .add_plugin_if(cfg.feature = "steam", eustress_steamworks::SteamworksPlugin)
        .run();
}
Fork the repo, add dependencies in Cargo.toml(e.g.,eustress = "0.16", eustress_persistent = "0.3"`, etc., using latest versions), and resolve any conflicts (e.g., dependency versions or overlapping plugins).

If you'd like a full Cargo.toml, code examples for specific integrations, or to prioritize certain components, let me know! We can refine for performance or test compatibility.