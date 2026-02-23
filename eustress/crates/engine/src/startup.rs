//! Startup and File Association Handler
//!
//! Handles command-line arguments for opening .ron scene files directly.
//! Enables double-click to open scenes like Roblox .rbxl files.
//!
//! ## File Extensions
//! - `.eustress` - Eustress scene file (RON format)
//! - `.ron` - Generic RON file (also supported)
//!
//! ## Usage
//! ```bash
//! # Open a scene directly
//! eustress-engine.exe my_scene.eustress
//!
//! # Or with full path
//! eustress-engine.exe "C:\Projects\my_game\scenes\level1.eustress"
//! ```
//!
//! ## Windows File Association
//! Run `eustress-engine.exe --register` as admin to register file associations.

use bevy::prelude::*;
use std::path::PathBuf;

// ============================================================================
// Startup Arguments
// ============================================================================

/// Command-line arguments parsed at startup
#[derive(Resource, Default, Debug, Clone)]
pub struct StartupArgs {
    /// Scene file to load on startup (if any)
    pub scene_file: Option<PathBuf>,
    
    /// Whether to register file associations (Windows only)
    pub register_associations: bool,
    
    /// Whether to unregister file associations (Windows only)
    pub unregister_associations: bool,
    
    /// Start in play mode immediately
    pub play_mode: bool,
    
    /// Start as server (headless)
    pub server_mode: bool,
    
    /// Verbose logging
    pub verbose: bool,
}

impl StartupArgs {
    /// Parse command-line arguments
    pub fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut result = Self::default();
        
        let mut i = 1; // Skip program name
        while i < args.len() {
            let arg = &args[i];
            
            match arg.as_str() {
                "--register" | "-r" => {
                    result.register_associations = true;
                }
                "--unregister" | "-u" => {
                    result.unregister_associations = true;
                }
                "--play" | "-p" => {
                    result.play_mode = true;
                }
                "--server" | "-s" => {
                    result.server_mode = true;
                }
                "--verbose" | "-v" => {
                    result.verbose = true;
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                _ => {
                    // Check if it's a file path
                    if !arg.starts_with('-') {
                        let path = PathBuf::from(arg);
                        if is_scene_file(&path) {
                            result.scene_file = Some(path);
                        } else {
                            eprintln!("Warning: Unknown argument or unsupported file: {}", arg);
                        }
                    }
                }
            }
            i += 1;
        }
        
        result
    }
}

/// Check if a path is a supported scene file
fn is_scene_file(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "eustressengine" | "eustress" | "ron" | "escene" | "json")
    } else {
        false
    }
}

/// Print help message
fn print_help() {
    println!(r#"
Eustress Engine - Game Development Studio

USAGE:
    eustress-engine.exe [OPTIONS] [SCENE_FILE]

ARGUMENTS:
    [SCENE_FILE]    Path to a .eustress or .ron scene file to open

OPTIONS:
    -h, --help          Show this help message
    -r, --register      Register file associations (Windows, requires admin)
    -u, --unregister    Unregister file associations (Windows, requires admin)
    -p, --play          Start in play mode immediately
    -s, --server        Start as headless server
    -v, --verbose       Enable verbose logging

EXAMPLES:
    eustress-engine.exe                         # Start with empty scene
    eustress-engine.exe my_game.eustress        # Open a scene file
    eustress-engine.exe --play level1.eustress  # Open and immediately play
    eustress-engine.exe --register              # Register .eustress extension

FILE EXTENSIONS:
    .eustress   Eustress scene file (recommended)
    .ron        RON format scene file
    .escene     Eustress scene (alternative)
"#);
}

// ============================================================================
// Cross-Platform File Association
// ============================================================================

/// Cross-platform file association registration
pub mod file_association {
    use std::path::PathBuf;
    
    /// Register .eustress file association (platform-specific)
    pub fn register() -> Result<(), String> {
        #[cfg(target_os = "windows")]
        return windows::register();
        
        #[cfg(target_os = "macos")]
        return macos::register();
        
        #[cfg(target_os = "linux")]
        return linux::register();
        
        #[cfg(target_os = "redox")]
        return redox::register();
        
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux", target_os = "redox")))]
        Err("File association not supported on this platform".to_string())
    }
    
    /// Unregister .eustress file association (platform-specific)
    pub fn unregister() -> Result<(), String> {
        #[cfg(target_os = "windows")]
        return windows::unregister();
        
        #[cfg(target_os = "macos")]
        return macos::unregister();
        
        #[cfg(target_os = "linux")]
        return linux::unregister();
        
        #[cfg(target_os = "redox")]
        return redox::unregister();
        
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux", target_os = "redox")))]
        Err("File association not supported on this platform".to_string())
    }
    
    /// Get the executable path
    fn get_exe_path() -> Result<PathBuf, String> {
        std::env::current_exe()
            .map_err(|e| format!("Failed to get executable path: {}", e))
    }
    
    // ========================================================================
    // Windows Implementation
    // ========================================================================
    
    #[cfg(target_os = "windows")]
    pub mod windows {
        use super::*;
        use std::process::Command;
        
        pub fn register() -> Result<(), String> {
            let exe_path = get_exe_path()?;
            let exe_str = exe_path.to_string_lossy();
            
            let commands = vec![
                format!(r#"reg add "HKEY_CLASSES_ROOT\.eustress" /ve /d "EustressScene" /f"#),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene" /ve /d "Eustress Scene File" /f"#),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene\DefaultIcon" /ve /d "\"{}\"" /f"#, exe_str),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene\shell\open\command" /ve /d "\"{}\" \"%1\"" /f"#, exe_str),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene\shell\edit" /ve /d "Edit with Eustress Engine" /f"#),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene\shell\edit\command" /ve /d "\"{}\" \"%1\"" /f"#, exe_str),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene\shell\play" /ve /d "Play in Eustress" /f"#),
                format!(r#"reg add "HKEY_CLASSES_ROOT\EustressScene\shell\play\command" /ve /d "\"{}\" --play \"%1\"" /f"#, exe_str),
            ];
            
            for cmd in commands {
                let output = Command::new("cmd")
                    .args(["/C", &cmd])
                    .output()
                    .map_err(|e| format!("Failed to run registry command: {}", e))?;
                
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if stderr.contains("Access is denied") {
                        return Err("Administrator privileges required. Run as admin.".to_string());
                    }
                }
            }
            
            notify_shell_change();
            println!("✅ File associations registered successfully!");
            Ok(())
        }
        
        pub fn unregister() -> Result<(), String> {
            let commands = vec![
                r#"reg delete "HKEY_CLASSES_ROOT\.eustress" /f"#,
                r#"reg delete "HKEY_CLASSES_ROOT\EustressScene" /f"#,
            ];
            
            for cmd in commands {
                let _ = Command::new("cmd").args(["/C", cmd]).output();
            }
            
            notify_shell_change();
            println!("✅ File associations removed");
            Ok(())
        }
        
        fn notify_shell_change() {
            #[link(name = "shell32")]
            extern "system" {
                fn SHChangeNotify(wEventId: i32, uFlags: u32, dwItem1: *const (), dwItem2: *const ());
            }
            
            const SHCNE_ASSOCCHANGED: i32 = 0x08000000;
            const SHCNF_IDLIST: u32 = 0;
            
            unsafe {
                SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
            }
        }
    }
    
    // ========================================================================
    // macOS Implementation
    // ========================================================================
    
    #[cfg(target_os = "macos")]
    pub mod macos {
        use super::*;
        use std::fs;
        use std::process::Command;
        
        /// Register via Launch Services and Info.plist
        /// Note: For full integration, the app should be bundled as .app
        pub fn register() -> Result<(), String> {
            let exe_path = get_exe_path()?;
            
            // Check if we're in an app bundle
            let app_bundle = exe_path
                .parent()
                .and_then(|p| p.parent())
                .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false));
            
            if let Some(bundle_path) = app_bundle {
                // We're in an app bundle - register via Launch Services
                let status = Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
                    .args(["-f", &bundle_path.to_string_lossy()])
                    .status()
                    .map_err(|e| format!("Failed to run lsregister: {}", e))?;
                
                if status.success() {
                    println!("✅ File associations registered via Launch Services");
                    println!("   .eustress files will open with Eustress Engine");
                    return Ok(());
                }
            }
            
            // Not in app bundle - create a minimal .app wrapper
            let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
            let app_dir = format!("{}/Applications/Eustress Engine.app", home);
            let contents_dir = format!("{}/Contents", app_dir);
            let macos_dir = format!("{}/MacOS", contents_dir);
            
            // Create directory structure
            fs::create_dir_all(&macos_dir)
                .map_err(|e| format!("Failed to create app bundle: {}", e))?;
            
            // Create symlink to executable
            let exe_link = format!("{}/eustress-engine", macos_dir);
            let _ = fs::remove_file(&exe_link); // Remove if exists
            std::os::unix::fs::symlink(&exe_path, &exe_link)
                .map_err(|e| format!("Failed to create symlink: {}", e))?;
            
            // Create Info.plist
            let info_plist = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>eustress-engine</string>
    <key>CFBundleIdentifier</key>
    <string>com.eustress.engine</string>
    <key>CFBundleName</key>
    <string>Eustress Engine</string>
    <key>CFBundleDisplayName</key>
    <string>Eustress Engine</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Eustress Scene</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSHandlerRank</key>
            <string>Owner</string>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>eustress</string>
                <string>escene</string>
            </array>
            <key>CFBundleTypeMIMETypes</key>
            <array>
                <string>application/x-eustress-scene</string>
            </array>
        </dict>
    </array>
    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>
            <string>com.eustress.scene</string>
            <key>UTTypeDescription</key>
            <string>Eustress Scene File</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.data</string>
                <string>public.text</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>eustress</string>
                    <string>escene</string>
                </array>
                <key>public.mime-type</key>
                <string>application/x-eustress-scene</string>
            </dict>
        </dict>
    </array>
</dict>
</plist>"#);
            
            fs::write(format!("{}/Info.plist", contents_dir), info_plist)
                .map_err(|e| format!("Failed to write Info.plist: {}", e))?;
            
            // Register with Launch Services
            let _ = Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
                .args(["-f", &app_dir])
                .status();
            
            println!("✅ File associations registered!");
            println!("   Created app bundle at: {}", app_dir);
            println!("   .eustress files will now open with Eustress Engine");
            
            Ok(())
        }
        
        pub fn unregister() -> Result<(), String> {
            let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
            let app_dir = format!("{}/Applications/Eustress Engine.app", home);
            
            // Unregister from Launch Services
            let _ = Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
                .args(["-u", &app_dir])
                .status();
            
            // Remove app bundle
            let _ = fs::remove_dir_all(&app_dir);
            
            println!("✅ File associations removed");
            Ok(())
        }
    }
    
    // ========================================================================
    // Linux Implementation (XDG Desktop Entry)
    // ========================================================================
    
    #[cfg(target_os = "linux")]
    pub mod linux {
        use super::*;
        use std::fs;
        use std::process::Command;
        
        pub fn register() -> Result<(), String> {
            let exe_path = get_exe_path()?;
            let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
            
            // Create directories
            let apps_dir = format!("{}/.local/share/applications", home);
            let mime_dir = format!("{}/.local/share/mime/packages", home);
            let icons_dir = format!("{}/.local/share/icons/hicolor/256x256/apps", home);
            
            fs::create_dir_all(&apps_dir).map_err(|e| format!("Failed to create apps dir: {}", e))?;
            fs::create_dir_all(&mime_dir).map_err(|e| format!("Failed to create mime dir: {}", e))?;
            fs::create_dir_all(&icons_dir).map_err(|e| format!("Failed to create icons dir: {}", e))?;
            
            // Create .desktop file
            let desktop_entry = format!(r#"[Desktop Entry]
Type=Application
Name=Eustress Engine
Comment=3D Game Development Studio
Exec="{}" %f
Icon=eustress-engine
Terminal=false
Categories=Development;Game;Graphics;3DGraphics;
MimeType=application/x-eustress-scene;

[Desktop Action Play]
Name=Play in Eustress
Exec="{}" --play %f

[Desktop Action Edit]
Name=Edit with Eustress Engine
Exec="{}" %f
"#, exe_path.display(), exe_path.display(), exe_path.display());
            
            fs::write(format!("{}/eustress-engine.desktop", apps_dir), desktop_entry)
                .map_err(|e| format!("Failed to write desktop file: {}", e))?;
            
            // Create MIME type definition
            let mime_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
    <mime-type type="application/x-eustress-scene">
        <comment>Eustress Scene File</comment>
        <comment xml:lang="en">Eustress Scene File</comment>
        <glob pattern="*.eustress"/>
        <glob pattern="*.escene"/>
        <icon name="eustress-engine"/>
    </mime-type>
</mime-info>
"#;
            
            fs::write(format!("{}/eustress-scene.xml", mime_dir), mime_xml)
                .map_err(|e| format!("Failed to write MIME file: {}", e))?;
            
            // Update MIME database
            let _ = Command::new("update-mime-database")
                .arg(format!("{}/.local/share/mime", home))
                .status();
            
            // Update desktop database
            let _ = Command::new("update-desktop-database")
                .arg(&apps_dir)
                .status();
            
            // Set as default handler
            let _ = Command::new("xdg-mime")
                .args(["default", "eustress-engine.desktop", "application/x-eustress-scene"])
                .status();
            
            println!("✅ File associations registered!");
            println!("   Desktop entry: {}/eustress-engine.desktop", apps_dir);
            println!("   MIME type: application/x-eustress-scene");
            println!("   .eustress files will now open with Eustress Engine");
            
            Ok(())
        }
        
        pub fn unregister() -> Result<(), String> {
            let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
            
            // Remove files
            let _ = fs::remove_file(format!("{}/.local/share/applications/eustress-engine.desktop", home));
            let _ = fs::remove_file(format!("{}/.local/share/mime/packages/eustress-scene.xml", home));
            
            // Update databases
            let _ = Command::new("update-mime-database")
                .arg(format!("{}/.local/share/mime", home))
                .status();
            let _ = Command::new("update-desktop-database")
                .arg(format!("{}/.local/share/applications", home))
                .status();
            
            println!("✅ File associations removed");
            Ok(())
        }
    }
    
    // ========================================================================
    // Redox OS Implementation
    // ========================================================================
    
    #[cfg(target_os = "redox")]
    pub mod redox {
        use super::*;
        use std::fs;
        
        /// Redox uses a simpler file association system via /etc/mime.types
        /// and orbital (the window manager) configuration
        pub fn register() -> Result<(), String> {
            let exe_path = get_exe_path()?;
            let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
            
            // Create Orbital app entry
            let orbital_dir = format!("{}/.orbital", home);
            fs::create_dir_all(&orbital_dir)
                .map_err(|e| format!("Failed to create orbital dir: {}", e))?;
            
            // Create app manifest for Orbital
            let manifest = format!(r#"{{
    "name": "Eustress Engine",
    "description": "3D Game Development Studio",
    "exec": "{}",
    "icon": "eustress-engine",
    "mime_types": ["application/x-eustress-scene"],
    "extensions": ["eustress", "escene"]
}}"#, exe_path.display());
            
            fs::write(format!("{}/eustress-engine.json", orbital_dir), manifest)
                .map_err(|e| format!("Failed to write manifest: {}", e))?;
            
            // Add to user's mime.types
            let mime_entry = "application/x-eustress-scene eustress escene\n";
            let mime_file = format!("{}/.mime.types", home);
            
            // Append if not already present
            let existing = fs::read_to_string(&mime_file).unwrap_or_default();
            if !existing.contains("x-eustress-scene") {
                let mut content = existing;
                content.push_str(mime_entry);
                fs::write(&mime_file, content)
                    .map_err(|e| format!("Failed to update mime.types: {}", e))?;
            }
            
            println!("✅ File associations registered for Redox OS!");
            println!("   .eustress files will open with Eustress Engine");
            
            Ok(())
        }
        
        pub fn unregister() -> Result<(), String> {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());
            
            // Remove Orbital manifest
            let _ = fs::remove_file(format!("{}/.orbital/eustress-engine.json", home));
            
            // Remove from mime.types (read, filter, write)
            let mime_file = format!("{}/.mime.types", home);
            if let Ok(content) = fs::read_to_string(&mime_file) {
                let filtered: String = content
                    .lines()
                    .filter(|line| !line.contains("x-eustress-scene"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = fs::write(&mime_file, filtered);
            }
            
            println!("✅ File associations removed");
            Ok(())
        }
    }
}

// Keep backward compatibility alias
pub use file_association::register as register_file_association;
pub use file_association::unregister as unregister_file_association;

// ============================================================================
// Startup Plugin
// ============================================================================

/// Plugin that handles startup arguments and scene loading
pub struct StartupPlugin;

impl Plugin for StartupPlugin {
    fn build(&self, app: &mut App) {
        // Get args from resource (already inserted in main.rs)
        // We need to check if it exists, otherwise parse fresh
        let args = if let Some(existing) = app.world().get_resource::<StartupArgs>() {
            existing.clone()
        } else {
            let parsed = StartupArgs::parse();
            app.insert_resource(parsed.clone());
            parsed
        };
        
        // Handle registration commands (exit after)
        if args.register_associations {
            match file_association::register() {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("❌ Failed to register: {}", e);
                    std::process::exit(1);
                }
            }
        }
        
        if args.unregister_associations {
            match file_association::unregister() {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("❌ Failed to unregister: {}", e);
                    std::process::exit(1);
                }
            }
        }
        
        // Add startup system to load scene if specified
        if args.scene_file.is_some() {
            app.add_systems(Startup, load_startup_scene.after(crate::default_scene::setup_default_scene));
        }
    }
}

/// System to load scene specified in command-line arguments
fn load_startup_scene(
    args: Res<StartupArgs>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Some(ref scene_path) = args.scene_file {
        info!("📂 Loading scene from command line: {:?}", scene_path);
        
        // Check if file exists
        if !scene_path.exists() {
            error!("❌ Scene file not found: {:?}", scene_path);
            return;
        }
        
        // Load the scene
        match load_scene_file(scene_path, &mut commands, &asset_server, &mut meshes, &mut materials) {
            Ok(()) => {
                info!("✅ Scene loaded successfully: {:?}", scene_path);
                
                // Update window title with scene name
                // TODO: Update window title
            }
            Err(e) => {
                error!("❌ Failed to load scene: {}", e);
            }
        }
    }
}

/// Load a scene file (RON format)
fn load_scene_file(
    path: &PathBuf,
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) -> Result<(), String> {
    info!("📂 Loading scene file: {:?}", path);
    
    // Use the unified scene loader
    let scene = crate::serialization::load_unified_scene(path)
        .map_err(|e| format!("Failed to parse scene file: {}", e))?;
    
    info!("📄 Scene '{}' loaded - {} entities", scene.metadata.name, scene.entities.len());
    
    // Spawn entities from scene (file-system-first: .glb meshes via AssetServer)
    spawn_scene(commands, asset_server, meshes, materials, &scene);
    
    Ok(())
}

/// Spawn entities from a unified scene (file-system-first: .glb meshes via AssetServer)
fn spawn_scene(
    commands: &mut Commands,
    asset_server: &AssetServer,
    _meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    scene: &eustress_common::scene::Scene,
) {
    use crate::classes::{Instance, BasePart, Part, PartType, ClassName, Material};
    use eustress_common::scene::EntityClass;
    
    for entity in &scene.entities {
        // Get transform from entity - rotation is quaternion [x, y, z, w]
        let transform = Transform {
            translation: Vec3::new(
                entity.transform.position[0],
                entity.transform.position[1],
                entity.transform.position[2],
            ),
            rotation: Quat::from_xyzw(
                entity.transform.rotation[0],
                entity.transform.rotation[1],
                entity.transform.rotation[2],
                entity.transform.rotation[3],
            ),
            scale: Vec3::new(
                entity.transform.scale[0],
                entity.transform.scale[1],
                entity.transform.scale[2],
            ),
        };
        
        // Create mesh, material, and components based on class
        match &entity.class {
            EntityClass::Part(part_data) => {
                let size = Vec3::new(part_data.size[0], part_data.size[1], part_data.size[2]);
                
                // Determine shape type
                let shape_type = match part_data.shape.as_str() {
                    "Ball" => PartType::Ball,
                    "Cylinder" => PartType::Cylinder,
                    "Wedge" => PartType::Wedge,
                    "CornerWedge" => PartType::CornerWedge,
                    _ => PartType::Block,
                };
                
                let color = Color::srgba(
                    part_data.color[0],
                    part_data.color[1],
                    part_data.color[2],
                    1.0 - part_data.transparency,
                );
                
                // Create Instance component for Explorer
                let instance = Instance {
                    id: entity.id,
                    name: entity.name.clone(),
                    class_name: ClassName::Part,
                    archivable: true,
                    ..Default::default()
                };
                
                // Parse material from string
                let material = match part_data.material.as_str() {
                    "SmoothPlastic" => Material::SmoothPlastic,
                    "Wood" => Material::Wood,
                    "WoodPlanks" => Material::WoodPlanks,
                    "Metal" => Material::Metal,
                    "CorrodedMetal" => Material::CorrodedMetal,
                    "DiamondPlate" => Material::DiamondPlate,
                    "Foil" => Material::Foil,
                    "Grass" => Material::Grass,
                    "Concrete" => Material::Concrete,
                    "Brick" => Material::Brick,
                    "Granite" => Material::Granite,
                    "Marble" => Material::Marble,
                    "Slate" => Material::Slate,
                    "Sand" => Material::Sand,
                    "Fabric" => Material::Fabric,
                    "Glass" => Material::Glass,
                    "Neon" => Material::Neon,
                    "Ice" => Material::Ice,
                    _ => Material::Plastic,
                };
                
                // Create BasePart component for selection and physics
                // Lock Baseplate by default (prevents accidental movement)
                let is_baseplate = entity.name == "Baseplate";
                let base_part = BasePart {
                    cframe: transform,
                    size,
                    color,
                    material,
                    transparency: part_data.transparency,
                    reflectance: part_data.reflectance,
                    anchored: part_data.anchored,
                    can_collide: part_data.can_collide,
                    locked: is_baseplate,
                    ..default()
                };
                
                // Create Part component with shape
                let part = Part { shape: shape_type };
                
                info!("  Spawning '{}' ({:?}) at {:?} size {:?}", entity.name, shape_type, transform.translation, size);
                
                // File-system-first: spawn via .glb mesh loaded by AssetServer
                crate::spawn::spawn_part_glb(
                    commands,
                    asset_server,
                    materials,
                    instance,
                    base_part,
                    part,
                );
            }
            EntityClass::MeshPart(_mesh_data) => {
                let instance = Instance {
                    id: entity.id,
                    name: entity.name.clone(),
                    class_name: ClassName::MeshPart,
                    archivable: true,
                    ..Default::default()
                };
                
                let base_part = BasePart {
                    cframe: transform,
                    size: Vec3::ONE,
                    ..default()
                };
                
                let part = Part { shape: PartType::Block };
                
                // File-system-first: spawn via .glb mesh loaded by AssetServer
                crate::spawn::spawn_part_glb(
                    commands,
                    asset_server,
                    materials,
                    instance,
                    base_part,
                    part,
                );
            }
            EntityClass::Folder => {
                let instance = Instance {
                    id: entity.id,
                    name: entity.name.clone(),
                    class_name: ClassName::Folder,
                    archivable: true,
                    ..Default::default()
                };
                commands.spawn((instance, Name::new(entity.name.clone())));
            }
            EntityClass::Model(_) => {
                let instance = Instance {
                    id: entity.id,
                    name: entity.name.clone(),
                    class_name: ClassName::Model,
                    archivable: true,
                    ..Default::default()
                };
                commands.spawn((instance, Name::new(entity.name.clone())));
            }
            EntityClass::Terrain(_) => continue, // Handled separately
            _ => continue, // Skip other types for now
        };
    }
    
    info!("✅ Spawned {} entities from scene", scene.entities.len());
}

// ============================================================================
// Scene File Resource
// ============================================================================

/// Currently open scene file
#[derive(Resource, Default, Debug, Clone)]
pub struct CurrentSceneFile {
    /// Path to the currently open scene
    pub path: Option<PathBuf>,
    
    /// Whether the scene has unsaved changes
    pub dirty: bool,
    
    /// Scene display name (filename without extension)
    pub name: String,
}

impl CurrentSceneFile {
    /// Create from a file path
    pub fn from_path(path: PathBuf) -> Self {
        let name = path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        
        Self {
            path: Some(path),
            dirty: false,
            name,
        }
    }
    
    /// Mark as having unsaved changes
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
    
    /// Mark as saved
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
    
    /// Get display title for window
    pub fn window_title(&self) -> String {
        let dirty_marker = if self.dirty { "*" } else { "" };
        format!("{}{} - Eustress Engine", dirty_marker, self.name)
    }
}
