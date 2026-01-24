//! # VIGA Rune API
//!
//! Scene building functions exposed to Rune scripts for VIGA-generated code.
//! These functions provide a high-level API for creating 3D scenes from
//! vision-as-inverse-graphics agent output.

use std::collections::HashMap;

/// Scene building functions for VIGA-generated Rune scripts
pub mod soul {
    /// Spawn a cube/box at the specified position
    /// 
    /// # Arguments
    /// * `name` - Unique name for the object
    /// * `x`, `y`, `z` - Position in world space (meters)
    /// * `width`, `height`, `depth` - Dimensions (meters)
    pub fn spawn_cube(name: &str, x: f64, y: f64, z: f64, width: f64, height: f64, depth: f64) {
        // This is a stub - actual implementation is in the Rune VM bindings
        println!("soul::spawn_cube({}, {}, {}, {}, {}, {}, {})", name, x, y, z, width, height, depth);
    }
    
    /// Spawn a sphere at the specified position
    /// 
    /// # Arguments
    /// * `name` - Unique name for the object
    /// * `x`, `y`, `z` - Position in world space (meters)
    /// * `radius` - Sphere radius (meters)
    pub fn spawn_sphere(name: &str, x: f64, y: f64, z: f64, radius: f64) {
        println!("soul::spawn_sphere({}, {}, {}, {}, {})", name, x, y, z, radius);
    }
    
    /// Spawn a cylinder at the specified position
    /// 
    /// # Arguments
    /// * `name` - Unique name for the object
    /// * `x`, `y`, `z` - Position in world space (meters)
    /// * `radius` - Cylinder radius (meters)
    /// * `height` - Cylinder height (meters)
    pub fn spawn_cylinder(name: &str, x: f64, y: f64, z: f64, radius: f64, height: f64) {
        println!("soul::spawn_cylinder({}, {}, {}, {}, {}, {})", name, x, y, z, radius, height);
    }
    
    /// Spawn a plane/ground at the specified position
    /// 
    /// # Arguments
    /// * `name` - Unique name for the object
    /// * `x`, `y`, `z` - Position in world space (meters)
    /// * `width`, `depth` - Plane dimensions (meters)
    pub fn spawn_plane(name: &str, x: f64, y: f64, z: f64, width: f64, depth: f64) {
        println!("soul::spawn_plane({}, {}, {}, {}, {}, {})", name, x, y, z, width, depth);
    }
    
    /// Set the color of an object (RGB 0.0-1.0)
    /// 
    /// # Arguments
    /// * `name` - Object name
    /// * `r`, `g`, `b` - RGB color components (0.0-1.0)
    pub fn set_color(name: &str, r: f64, g: f64, b: f64) {
        println!("soul::set_color({}, {}, {}, {})", name, r, g, b);
    }
    
    /// Set the material of an object
    /// 
    /// # Arguments
    /// * `name` - Object name
    /// * `material` - Material name: "Plastic", "Metal", "Glass", "Wood", "Concrete", "Neon"
    pub fn set_material(name: &str, material: &str) {
        println!("soul::set_material({}, {})", name, material);
    }
    
    /// Set the position of an object
    /// 
    /// # Arguments
    /// * `name` - Object name
    /// * `x`, `y`, `z` - New position in world space (meters)
    pub fn set_position(name: &str, x: f64, y: f64, z: f64) {
        println!("soul::set_position({}, {}, {}, {})", name, x, y, z);
    }
    
    /// Set the rotation of an object (degrees)
    /// 
    /// # Arguments
    /// * `name` - Object name
    /// * `rx`, `ry`, `rz` - Rotation angles in degrees
    pub fn set_rotation(name: &str, rx: f64, ry: f64, rz: f64) {
        println!("soul::set_rotation({}, {}, {}, {})", name, rx, ry, rz);
    }
    
    /// Set the scale of an object
    /// 
    /// # Arguments
    /// * `name` - Object name
    /// * `sx`, `sy`, `sz` - Scale factors
    pub fn set_scale(name: &str, sx: f64, sy: f64, sz: f64) {
        println!("soul::set_scale({}, {}, {}, {})", name, sx, sy, sz);
    }
    
    /// Spawn a point light
    /// 
    /// # Arguments
    /// * `name` - Light name
    /// * `x`, `y`, `z` - Position
    /// * `intensity` - Light intensity
    /// * `r`, `g`, `b` - Light color (0.0-1.0)
    pub fn spawn_point_light(name: &str, x: f64, y: f64, z: f64, intensity: f64, r: f64, g: f64, b: f64) {
        println!("soul::spawn_point_light({}, {}, {}, {}, {}, {}, {}, {})", name, x, y, z, intensity, r, g, b);
    }
    
    /// Spawn a directional light (sun)
    /// 
    /// # Arguments
    /// * `name` - Light name
    /// * `dx`, `dy`, `dz` - Direction vector
    /// * `intensity` - Light intensity
    pub fn spawn_directional_light(name: &str, dx: f64, dy: f64, dz: f64, intensity: f64) {
        println!("soul::spawn_directional_light({}, {}, {}, {}, {})", name, dx, dy, dz, intensity);
    }
    
    /// Spawn a spotlight
    /// 
    /// # Arguments
    /// * `name` - Light name
    /// * `x`, `y`, `z` - Position
    /// * `dx`, `dy`, `dz` - Direction vector
    /// * `intensity` - Light intensity
    /// * `angle` - Spotlight cone angle in degrees
    pub fn spawn_spotlight(name: &str, x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64, intensity: f64, angle: f64) {
        println!("soul::spawn_spotlight({}, {}, {}, {}, {}, {}, {}, {}, {})", name, x, y, z, dx, dy, dz, intensity, angle);
    }
    
    /// Set camera position and target
    /// 
    /// # Arguments
    /// * `x`, `y`, `z` - Camera position
    /// * `target_x`, `target_y`, `target_z` - Look-at target
    pub fn set_camera(x: f64, y: f64, z: f64, target_x: f64, target_y: f64, target_z: f64) {
        println!("soul::set_camera({}, {}, {}, {}, {}, {})", x, y, z, target_x, target_y, target_z);
    }
    
    /// Set parent-child relationship
    /// 
    /// # Arguments
    /// * `child` - Child object name
    /// * `parent` - Parent object name
    pub fn set_parent(child: &str, parent: &str) {
        println!("soul::set_parent({}, {})", child, parent);
    }
    
    /// Wait for specified duration (for animations)
    /// 
    /// # Arguments
    /// * `seconds` - Duration to wait
    pub fn wait(seconds: f64) {
        println!("soul::wait({})", seconds);
    }
    
    /// Delete an object by name
    /// 
    /// # Arguments
    /// * `name` - Object name to delete
    pub fn delete(name: &str) {
        println!("soul::delete({})", name);
    }
    
    /// Clear all objects in the scene
    pub fn clear_scene() {
        println!("soul::clear_scene()");
    }
}

/// VIGA function registry for documentation generation
#[derive(Default)]
pub struct VigaFunctionRegistry {
    functions: HashMap<String, VigaFunctionInfo>,
}

/// Information about a VIGA function
#[derive(Clone)]
pub struct VigaFunctionInfo {
    pub name: String,
    pub params: Vec<(String, String)>, // (name, type)
    pub doc: String,
}

impl VigaFunctionRegistry {
    /// Create registry with all VIGA functions
    pub fn new() -> Self {
        let mut registry = Self::default();
        
        registry.register(VigaFunctionInfo {
            name: "spawn_cube".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("x".to_string(), "f64".to_string()),
                ("y".to_string(), "f64".to_string()),
                ("z".to_string(), "f64".to_string()),
                ("width".to_string(), "f64".to_string()),
                ("height".to_string(), "f64".to_string()),
                ("depth".to_string(), "f64".to_string()),
            ],
            doc: "Spawn a cube/box at the specified position".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "spawn_sphere".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("x".to_string(), "f64".to_string()),
                ("y".to_string(), "f64".to_string()),
                ("z".to_string(), "f64".to_string()),
                ("radius".to_string(), "f64".to_string()),
            ],
            doc: "Spawn a sphere at the specified position".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "spawn_cylinder".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("x".to_string(), "f64".to_string()),
                ("y".to_string(), "f64".to_string()),
                ("z".to_string(), "f64".to_string()),
                ("radius".to_string(), "f64".to_string()),
                ("height".to_string(), "f64".to_string()),
            ],
            doc: "Spawn a cylinder at the specified position".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "set_color".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("r".to_string(), "f64".to_string()),
                ("g".to_string(), "f64".to_string()),
                ("b".to_string(), "f64".to_string()),
            ],
            doc: "Set the color of an object (RGB 0.0-1.0)".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "set_material".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("material".to_string(), "str".to_string()),
            ],
            doc: "Set the material: Plastic, Metal, Glass, Wood, Concrete, Neon".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "set_position".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("x".to_string(), "f64".to_string()),
                ("y".to_string(), "f64".to_string()),
                ("z".to_string(), "f64".to_string()),
            ],
            doc: "Set the position of an object".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "spawn_point_light".to_string(),
            params: vec![
                ("name".to_string(), "str".to_string()),
                ("x".to_string(), "f64".to_string()),
                ("y".to_string(), "f64".to_string()),
                ("z".to_string(), "f64".to_string()),
                ("intensity".to_string(), "f64".to_string()),
                ("r".to_string(), "f64".to_string()),
                ("g".to_string(), "f64".to_string()),
                ("b".to_string(), "f64".to_string()),
            ],
            doc: "Spawn a point light".to_string(),
        });
        
        registry.register(VigaFunctionInfo {
            name: "set_camera".to_string(),
            params: vec![
                ("x".to_string(), "f64".to_string()),
                ("y".to_string(), "f64".to_string()),
                ("z".to_string(), "f64".to_string()),
                ("target_x".to_string(), "f64".to_string()),
                ("target_y".to_string(), "f64".to_string()),
                ("target_z".to_string(), "f64".to_string()),
            ],
            doc: "Set camera position and look-at target".to_string(),
        });
        
        registry
    }
    
    fn register(&mut self, info: VigaFunctionInfo) {
        self.functions.insert(info.name.clone(), info);
    }
    
    /// Generate documentation for LLM prompts
    pub fn generate_docs(&self) -> String {
        let mut docs = String::from("# VIGA Scene Building API\n\n");
        docs.push_str("All functions are in the `soul::` namespace.\n\n");
        
        for (name, info) in &self.functions {
            docs.push_str(&format!("## soul::{}\n", name));
            docs.push_str(&format!("{}\n\n", info.doc));
            docs.push_str("```rune\n");
            let params: Vec<String> = info.params.iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect();
            docs.push_str(&format!("soul::{}({})\n", name, params.join(", ")));
            docs.push_str("```\n\n");
        }
        
        docs
    }
}
