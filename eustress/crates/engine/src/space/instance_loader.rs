//! Instance loader - loads .glb.toml files as entity instances
//!
//! Architecture:
//! - Mesh assets live in assets/meshes/ (shared, reusable)
//! - Instance files (.glb.toml) live in Workspace/ (unique per entity)
//! - Each .toml references a mesh asset and defines instance-specific properties

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Instance definition loaded from .glb.toml file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDefinition {
    pub asset: AssetReference,
    pub transform: TransformData,
    pub properties: InstanceProperties,
    pub metadata: InstanceMetadata,
    /// Optional realism material properties (dynamic on any class)
    #[serde(default)]
    pub material: Option<TomlMaterialProperties>,
    /// Optional thermodynamic state (dynamic on any class)
    #[serde(default)]
    pub thermodynamic: Option<TomlThermodynamicState>,
    /// Optional electrochemical state (dynamic on any class)
    #[serde(default)]
    pub electrochemical: Option<TomlElectrochemicalState>,
}

/// Reference to a shared mesh asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReference {
    /// Path to mesh file (relative to Space root)
    pub mesh: String,
    /// glTF scene name (usually "Scene0")
    #[serde(default = "default_scene")]
    pub scene: String,
}

fn default_scene() -> String {
    "Scene0".to_string()
}

/// Transform data (position, rotation, scale)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion (x, y, z, w)
    pub scale: [f32; 3],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl From<TransformData> for Transform {
    fn from(data: TransformData) -> Self {
        Transform {
            translation: Vec3::from_array(data.position),
            rotation: Quat::from_xyzw(
                data.rotation[0],
                data.rotation[1],
                data.rotation[2],
                data.rotation[3],
            ),
            scale: Vec3::from_array(data.scale),
        }
    }
}

impl From<Transform> for TransformData {
    fn from(transform: Transform) -> Self {
        Self {
            position: transform.translation.to_array(),
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: transform.scale.to_array(),
        }
    }
}

/// Instance-specific properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProperties {
    #[serde(default = "default_color")]
    pub color: [f32; 4], // RGBA
    #[serde(default)]
    pub transparency: f32,
    #[serde(default)]
    pub anchored: bool,
    #[serde(default = "default_true")]
    pub can_collide: bool,
    #[serde(default = "default_true")]
    pub cast_shadow: bool,
    #[serde(default)]
    pub reflectance: f32,
}

fn default_color() -> [f32; 4] {
    [0.5, 0.5, 0.5, 1.0]
}

fn default_true() -> bool {
    true
}

impl Default for InstanceProperties {
    fn default() -> Self {
        Self {
            color: default_color(),
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
        }
    }
}

/// Instance metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceMetadata {
    #[serde(default = "default_class_name")]
    pub class_name: String,
    #[serde(default = "default_true")]
    pub archivable: bool,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_modified: String,
}

fn default_class_name() -> String {
    "Part".to_string()
}

impl Default for InstanceMetadata {
    fn default() -> Self {
        Self {
            class_name: default_class_name(),
            archivable: true,
            created: String::new(),
            last_modified: String::new(),
        }
    }
}

// ============================================================================
// TOML-serializable realism property structs
// ============================================================================

/// Material properties as they appear in .glb.toml [material] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlMaterialProperties {
    #[serde(default = "default_material_name")]
    pub name: String,
    #[serde(default)]
    pub young_modulus: f32,
    #[serde(default)]
    pub poisson_ratio: f32,
    #[serde(default)]
    pub yield_strength: f32,
    #[serde(default)]
    pub ultimate_strength: f32,
    #[serde(default)]
    pub fracture_toughness: f32,
    #[serde(default)]
    pub hardness: f32,
    #[serde(default)]
    pub thermal_conductivity: f32,
    #[serde(default)]
    pub specific_heat: f32,
    #[serde(default)]
    pub thermal_expansion: f32,
    #[serde(default)]
    pub melting_point: f32,
    #[serde(default)]
    pub density: f32,
    #[serde(default)]
    pub friction_static: f32,
    #[serde(default)]
    pub friction_kinetic: f32,
    #[serde(default)]
    pub restitution: f32,
    /// Domain-specific extensions (porosity, electrical_conductivity, role, etc.)
    /// Accepts both numeric and string values from TOML; only f64 values
    /// are forwarded to the realism MaterialProperties component.
    #[serde(default)]
    pub custom: HashMap<String, toml::Value>,
}

fn default_material_name() -> String {
    "Steel".to_string()
}

impl TomlMaterialProperties {
    /// Convert to realism MaterialProperties component
    pub fn to_component(&self) -> eustress_common::realism::materials::prelude::MaterialProperties {
        eustress_common::realism::materials::prelude::MaterialProperties {
            name: self.name.clone(),
            young_modulus: self.young_modulus,
            poisson_ratio: self.poisson_ratio,
            yield_strength: self.yield_strength,
            ultimate_strength: self.ultimate_strength,
            fracture_toughness: self.fracture_toughness,
            hardness: self.hardness,
            thermal_conductivity: self.thermal_conductivity,
            specific_heat: self.specific_heat,
            thermal_expansion: self.thermal_expansion,
            melting_point: self.melting_point,
            density: self.density,
            friction_static: self.friction_static,
            friction_kinetic: self.friction_kinetic,
            restitution: self.restitution,
            custom_properties: self.custom.iter()
                .filter_map(|(k, v)| match v {
                    toml::Value::Float(f) => Some((k.clone(), *f)),
                    toml::Value::Integer(i) => Some((k.clone(), *i as f64)),
                    _ => None, // skip strings, bools, etc.
                })
                .collect(),
        }
    }
}

/// Thermodynamic state as it appears in .glb.toml [thermodynamic] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlThermodynamicState {
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_pressure")]
    pub pressure: f32,
    #[serde(default)]
    pub volume: f32,
    #[serde(default)]
    pub internal_energy: f32,
    #[serde(default)]
    pub entropy: f32,
    #[serde(default)]
    pub enthalpy: f32,
    #[serde(default = "default_one")]
    pub moles: f32,
}

fn default_temperature() -> f32 { 298.15 }
fn default_pressure() -> f32 { 101_325.0 }
fn default_one() -> f32 { 1.0 }

impl TomlThermodynamicState {
    /// Convert to realism ThermodynamicState component
    pub fn to_component(&self) -> eustress_common::realism::particles::prelude::ThermodynamicState {
        eustress_common::realism::particles::prelude::ThermodynamicState {
            temperature: self.temperature,
            pressure: self.pressure,
            volume: self.volume,
            internal_energy: self.internal_energy,
            entropy: self.entropy,
            enthalpy: self.enthalpy,
            moles: self.moles,
        }
    }
}

/// Electrochemical state as it appears in .glb.toml [electrochemical] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlElectrochemicalState {
    #[serde(default = "default_voltage")]
    pub voltage: f32,
    #[serde(default = "default_voltage")]
    pub terminal_voltage: f32,
    #[serde(default)]
    pub capacity_ah: f32,
    #[serde(default = "default_one")]
    pub soc: f32,
    #[serde(default)]
    pub current: f32,
    #[serde(default)]
    pub internal_resistance: f32,
    #[serde(default)]
    pub ionic_conductivity: f32,
    #[serde(default)]
    pub cycle_count: u32,
    #[serde(default)]
    pub c_rate: f32,
    #[serde(default = "default_one")]
    pub capacity_retention: f32,
    #[serde(default)]
    pub heat_generation: f32,
    #[serde(default)]
    pub dendrite_risk: f32,
}

fn default_voltage() -> f32 { 2.23 }

impl TomlElectrochemicalState {
    /// Convert to realism ElectrochemicalState component
    pub fn to_component(&self) -> eustress_common::realism::particles::prelude::ElectrochemicalState {
        eustress_common::realism::particles::prelude::ElectrochemicalState {
            voltage: self.voltage,
            terminal_voltage: self.terminal_voltage,
            capacity_ah: self.capacity_ah,
            soc: self.soc,
            current: self.current,
            internal_resistance: self.internal_resistance,
            ionic_conductivity: self.ionic_conductivity,
            cycle_count: self.cycle_count,
            c_rate: self.c_rate,
            capacity_retention: self.capacity_retention,
            heat_generation: self.heat_generation,
            dendrite_risk: self.dendrite_risk,
        }
    }
}

/// Component marking an entity as loaded from an instance file
#[derive(Component, Debug, Clone)]
pub struct InstanceFile {
    /// Path to the .glb.toml file
    pub toml_path: PathBuf,
    /// Path to the referenced mesh asset
    pub mesh_path: PathBuf,
    /// Instance name (derived from filename)
    pub name: String,
}

/// Load instance definition from .glb.toml file
pub fn load_instance_definition(toml_path: &Path) -> Result<InstanceDefinition, String> {
    let toml_str = std::fs::read_to_string(toml_path)
        .map_err(|e| format!("Failed to read {}: {}", toml_path.display(), e))?;
    
    let instance: InstanceDefinition = toml::from_str(&toml_str)
        .map_err(|e| format!("Failed to parse {}: {}", toml_path.display(), e))?;
    
    Ok(instance)
}

/// Write instance definition to .glb.toml file
pub fn write_instance_definition(
    toml_path: &Path,
    instance: &InstanceDefinition,
) -> Result<(), String> {
    let toml_str = toml::to_string_pretty(instance)
        .map_err(|e| format!("Failed to serialize instance: {}", e))?;
    
    std::fs::write(toml_path, toml_str)
        .map_err(|e| format!("Failed to write {}: {}", toml_path.display(), e))?;
    
    Ok(())
}

/// Known primitive mesh filenames that map to engine asset parts
const PRIMITIVE_MESHES: &[(&str, &str, eustress_common::classes::PartType)] = &[
    ("block", "parts/block.glb", eustress_common::classes::PartType::Block),
    ("ball", "parts/ball.glb", eustress_common::classes::PartType::Ball),
    ("cylinder", "parts/cylinder.glb", eustress_common::classes::PartType::Cylinder),
    ("wedge", "parts/wedge.glb", eustress_common::classes::PartType::Wedge),
    ("corner_wedge", "parts/corner_wedge.glb", eustress_common::classes::PartType::CornerWedge),
    ("cone", "parts/cone.glb", eustress_common::classes::PartType::Cone),
];

/// Spawn entity from instance definition, loading actual GLB meshes.
///
/// - **Primitives** (block.glb, ball.glb, etc.): loaded from engine `assets/parts/`
/// - **Custom meshes** (V-Cell, user models): resolved relative to the .glb.toml
///   file's parent directory and loaded as a GLTF scene via AssetServer
///
/// Scale from [transform] sets the entity size via Transform.scale.
pub fn spawn_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    toml_path: PathBuf,
    instance: InstanceDefinition,
) -> Entity {
    // Extract instance name from filename (strip .glb.toml → name)
    let name = toml_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .trim_end_matches(".glb")
        .to_string();
    
    // Resolve the mesh path: check if it's a known primitive or a custom GLB
    let mesh_ref = instance.asset.mesh.to_lowercase();
    let primitive = PRIMITIVE_MESHES.iter().find(|(hint, _, _)| {
        let fname = mesh_ref.rsplit('/').next().unwrap_or(&mesh_ref);
        fname.contains(hint)
    });
    
    let (is_custom_mesh, part_shape) = if let Some((_, _, shape)) = primitive {
        (false, *shape)
    } else {
        // Custom mesh — default to Block shape for bounding-box purposes
        (true, eustress_common::classes::PartType::Block)
    };
    
    // Determine the absolute path for the GLB mesh file
    let toml_dir = toml_path.parent().unwrap_or(Path::new("."));
    let absolute_mesh_path = toml_dir.join(&instance.asset.mesh);
    
    // Build material from properties
    let [r, g, b, a] = instance.properties.color;
    let transparency = instance.properties.transparency;
    let alpha = a * (1.0 - transparency);
    let material_handle = materials.add(StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: 0.7,
        metallic: 0.0,
        reflectance: instance.properties.reflectance,
        ..default()
    });
    
    // Parse class name (legacy "AdvancedPart" maps to Part via ClassName::from_str)
    let class_name = eustress_common::classes::ClassName::from_str(
        &instance.metadata.class_name
    ).unwrap_or(eustress_common::classes::ClassName::Part);
    
    let scale = Vec3::from_array(instance.transform.scale);
    
    // Build BasePart so the Properties panel can read/display part properties
    let base_part = eustress_common::classes::BasePart {
        size: scale,
        color: Color::srgba(r, g, b, a),
        transparency,
        reflectance: instance.properties.reflectance,
        anchored: instance.properties.anchored,
        can_collide: instance.properties.can_collide,
        cframe: Transform::from(instance.transform.clone()),
        ..default()
    };
    
    let transform = Transform::from(instance.transform);
    
    if is_custom_mesh && absolute_mesh_path.exists() {
        // ── Custom GLB mesh: load the full GLTF scene ──
        // Bevy's AssetServer accepts absolute paths on Windows
        let scene_path = format!(
            "{}#Scene0",
            absolute_mesh_path.to_string_lossy().replace('\\', "/")
        );
        let scene_handle: Handle<Scene> = asset_server.load(scene_path);
        
        let mut entity_commands = commands.spawn((
            SceneRoot(scene_handle),
            transform,
            eustress_common::classes::Instance {
                name: name.clone(),
                class_name,
                archivable: instance.metadata.archivable,
                id: 0,
                ai: false,
            },
            base_part,
            eustress_common::classes::Part { shape: part_shape },
            eustress_common::default_scene::PartEntityMarker {
                part_id: name.clone(),
            },
            InstanceFile {
                toml_path: toml_path.clone(),
                mesh_path: absolute_mesh_path,
                name: name.clone(),
            },
            Name::new(name.clone()),
        ));
        
        // Attach realism components if present in TOML
        if let Some(ref mat) = instance.material {
            entity_commands.insert(mat.to_component());
            info!("  + MaterialProperties: {}", mat.name);
        }
        if let Some(ref thermo) = instance.thermodynamic {
            entity_commands.insert(thermo.to_component());
            info!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
        }
        if let Some(ref echem) = instance.electrochemical {
            entity_commands.insert(echem.to_component());
            info!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
        }
        
        let entity = entity_commands.id();
        info!("Spawned custom mesh '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
        entity
    } else {
        // ── Primitive mesh: load from engine assets/parts/ ──
        let glb_path = if let Some((_, asset_path, _)) = primitive {
            *asset_path
        } else {
            "parts/block.glb" // fallback
        };
        let mesh_handle: Handle<Mesh> = asset_server.load(
            format!("{}#Mesh0/Primitive0", glb_path)
        );
        
        let mut entity_commands = commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            transform,
            eustress_common::classes::Instance {
                name: name.clone(),
                class_name,
                archivable: instance.metadata.archivable,
                id: 0,
                ai: false,
            },
            base_part,
            eustress_common::classes::Part { shape: part_shape },
            eustress_common::default_scene::PartEntityMarker {
                part_id: name.clone(),
            },
            InstanceFile {
                toml_path: toml_path.clone(),
                mesh_path: absolute_mesh_path,
                name: name.clone(),
            },
            Name::new(name.clone()),
        ));
        
        // Attach realism components if present in TOML
        if let Some(ref mat) = instance.material {
            entity_commands.insert(mat.to_component());
            info!("  + MaterialProperties: {}", mat.name);
        }
        if let Some(ref thermo) = instance.thermodynamic {
            entity_commands.insert(thermo.to_component());
            info!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
        }
        if let Some(ref echem) = instance.electrochemical {
            entity_commands.insert(echem.to_component());
            info!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
        }
        
        let entity = entity_commands.id();
        info!("Spawned primitive '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
        entity
    }
}

/// Recursively collect all .glb.toml files from a directory
fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_toml_files(&path, out);
        } else if path.to_string_lossy().ends_with(".glb.toml") {
            out.push(path);
        }
    }
}

/// System to load all .glb.toml instance files from Workspace (recursive)
pub fn load_instance_files_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<super::file_loader::SpaceFileRegistry>,
) {
    // Get Space root path (TODO: make configurable)
    let space_root = PathBuf::from("C:/Users/miksu/Documents/Eustress/Universe1/spaces/Space1");
    
    if !space_root.exists() {
        warn!("Space path does not exist: {:?}", space_root);
        return;
    }
    
    // Recursively scan Workspace for .glb.toml files
    let workspace_path = space_root.join("Workspace");
    if !workspace_path.exists() {
        return;
    }
    
    let mut toml_files = Vec::new();
    collect_toml_files(&workspace_path, &mut toml_files);
    
    for path in toml_files {
        // Load instance definition
        match load_instance_definition(&path) {
            Ok(instance) => {
                let entity = spawn_instance(
                    &mut commands,
                    &asset_server,
                    &mut materials,
                    path.clone(),
                    instance,
                );
                
                // Register in SpaceFileRegistry
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .trim_end_matches(".glb")
                    .to_string();
                
                registry.register(
                    path.clone(),
                    entity,
                    super::file_loader::FileMetadata {
                        path: path.clone(),
                        file_type: super::file_loader::FileType::Toml,
                        service: "Workspace".to_string(),
                        name,
                        size: 0,
                        modified: std::time::SystemTime::now(),
                        children: Vec::new(),
                    },
                );
            }
            Err(e) => {
                error!("Failed to load instance file {:?}: {}", path, e);
            }
        }
    }
}

/// System to write instance changes back to .glb.toml files
pub fn write_instance_changes_system(
    instances: Query<(&Transform, &InstanceFile), Changed<Transform>>,
) {
    for (transform, instance_file) in instances.iter() {
        // Load current instance definition
        let mut instance = match load_instance_definition(&instance_file.toml_path) {
            Ok(inst) => inst,
            Err(e) => {
                error!("Failed to load instance for write-back: {}", e);
                continue;
            }
        };
        
        // Update transform
        instance.transform = TransformData::from(*transform);
        
        // Update last_modified timestamp
        instance.metadata.last_modified = chrono::Utc::now().to_rfc3339();
        
        // Write back to file
        if let Err(e) = write_instance_definition(&instance_file.toml_path, &instance) {
            error!("Failed to write instance: {}", e);
        } else {
            debug!("💾 Wrote transform changes to {:?}", instance_file.toml_path);
        }
    }
}
