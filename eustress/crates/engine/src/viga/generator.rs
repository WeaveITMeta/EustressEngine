//! # VIGA Generator
//!
//! Generates Rune scripts from reference images using multimodal LLM.

use bevy::prelude::*;

use super::context::VigaContext;

/// Generator configuration
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Model to use (e.g., "claude-sonnet-4-20250514")
    pub model: String,
    /// Maximum tokens for generation
    pub max_tokens: u32,
    /// Temperature for generation
    pub temperature: f32,
    /// Whether to include planning step
    pub include_planning: bool,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 8192,
            temperature: 0.3,
            include_planning: true,
        }
    }
}

/// VIGA Generator - produces Rune scripts from images
pub struct VigaGenerator {
    /// Configuration
    pub config: GeneratorConfig,
}

impl Default for VigaGenerator {
    fn default() -> Self {
        Self {
            config: GeneratorConfig::default(),
        }
    }
}

impl VigaGenerator {
    /// Create with custom config
    pub fn new(config: GeneratorConfig) -> Self {
        Self { config }
    }
    
    /// Build the system prompt for VIGA generation
    pub fn build_system_prompt(&self) -> String {
        r#"You are VIGA (Vision-as-Inverse-Graphics Agent), an expert at converting reference images into 3D scene code for the Eustress Engine.

# YOUR TASK
Analyze the reference image and generate Rune script code that recreates the scene in 3D.

# RUNE SCRIPT API (Eustress Engine)

## Creating Objects
```rune
// Create a cube/box
soul::spawn_cube("name", x, y, z, width, height, depth);

// Create a sphere
soul::spawn_sphere("name", x, y, z, radius);

// Create a cylinder
soul::spawn_cylinder("name", x, y, z, radius, height);

// Create a plane/ground
soul::spawn_plane("name", x, y, z, width, depth);
```

## Setting Properties
```rune
// Set color (RGB 0.0-1.0)
soul::set_color("name", r, g, b);

// Set material
soul::set_material("name", "Plastic"); // Plastic, Metal, Glass, Wood, Concrete, Neon

// Set position
soul::set_position("name", x, y, z);

// Set rotation (degrees)
soul::set_rotation("name", rx, ry, rz);

// Set scale
soul::set_scale("name", sx, sy, sz);
```

## Lighting
```rune
// Add point light
soul::spawn_point_light("name", x, y, z, intensity, r, g, b);

// Add directional light (sun)
soul::spawn_directional_light("name", dx, dy, dz, intensity);

// Add spotlight
soul::spawn_spotlight("name", x, y, z, dx, dy, dz, intensity, angle);
```

## Camera
```rune
// Set camera position and target
soul::set_camera(x, y, z, target_x, target_y, target_z);
```

## Grouping
```rune
// Parent one object to another
soul::set_parent("child", "parent");
```

# SCALE REFERENCE (1 unit = 1 meter)
Use these real-world dimensions to ensure realistic proportions:

## People & Body (IMPORTANT: Identify age before sizing!)
- Adult human: 1.7m tall, 0.45m shoulder width
- Teenager (14-17yr): 1.5-1.7m tall
- Child (10-13yr): 1.3-1.5m tall
- Child (6-9yr): 1.1-1.3m tall
- Toddler (2-5yr): 0.85-1.1m tall
- Baby/Infant: 0.5-0.75m tall
- Seated adult: 1.2m from floor to head
- Seated child: 0.9-1.0m from floor to head

## Furniture
- Dining table: 0.75m height, 1.2m × 0.8m top
- Coffee table: 0.45m height, 1.0m × 0.6m top
- Desk: 0.75m height, 1.5m × 0.75m top
- Dining chair: 0.45m seat height, 0.9m total height
- Office chair: 0.45-0.55m seat height
- Sofa (3-seat): 0.85m height, 2.0m × 0.9m
- Bed (queen): 0.6m height, 2.0m × 1.5m
- Bookshelf: 1.8m height, 0.8m width, 0.3m depth
- TV stand: 0.5m height

## Architecture
- Door: 2.1m height × 0.9m width × 0.05m thick
- Window: 1.2m height × 1.0m width
- Ceiling height: 2.4-3.0m
- Wall thickness: 0.15-0.3m
- Stair step: 0.18m rise, 0.28m run
- Hallway width: 1.0-1.5m

## Vehicles
- Sedan car: 4.5m × 1.8m × 1.5m
- SUV: 4.8m × 1.9m × 1.7m
- Motorcycle: 2.2m × 0.8m × 1.1m
- Bicycle: 1.8m × 0.6m × 1.0m

## Outdoor
- Street lamp: 4-6m height
- Tree (medium): 6-10m height, 4-6m canopy
- Fire hydrant: 0.6m height
- Trash can: 0.9m height, 0.5m diameter
- Park bench: 0.45m seat height, 1.5m width

## Electronics & Objects
- Laptop: 0.35m × 0.25m × 0.02m (closed)
- Desktop monitor: 0.5m × 0.35m
- Smartphone: 0.15m × 0.07m
- Mug/cup: 0.1m height, 0.08m diameter
- Book: 0.25m × 0.18m × 0.03m
- Lamp (table): 0.5m height

## Kitchen
- Counter height: 0.9m
- Refrigerator: 1.8m × 0.8m × 0.7m
- Stove/oven: 0.9m × 0.6m × 0.6m
- Microwave: 0.3m × 0.5m × 0.4m

IMPORTANT: When you see an object in the image, cross-reference with these sizes.
If a chair is next to a table, the chair seat should be ~0.45m and table top ~0.75m.

RELATIVE SIZING STRATEGY:
1. First, identify ANY person in the image and estimate their age
2. Use age-appropriate height: adult=1.7m, teen=1.6m, child=1.2m, toddler=0.9m
3. Scale all other objects relative to that person
4. If no people visible, use furniture as reference (door=2.1m, table=0.75m height)
5. Cross-validate: a chair should reach mid-thigh on an adult (~0.45m)

# DEPTH & SPATIAL REASONING
Use these monocular depth cues to estimate 3D positions:
- **Occlusion**: Objects in front block objects behind
- **Relative size**: Farther objects appear smaller
- **Vertical position**: Objects higher in frame are usually farther (ground plane)
- **Shadows**: Shadow direction indicates light source and ground contact
- **Perspective**: Parallel lines converge toward vanishing points
- **Texture gradient**: Texture becomes denser with distance
- **Atmospheric perspective**: Distant objects appear hazier/bluer

# SCENE GRAPH (Object Relationships)
Before coding, mentally construct a scene graph:
1. Identify the GROUND PLANE (y=0)
2. List objects that REST ON the ground
3. List objects that REST ON other objects (table → objects on table)
4. List objects that are ATTACHED to walls/ceilings
5. Use set_parent() for hierarchical relationships

# MATERIAL INFERENCE
Infer materials from visual cues:
- **Shiny highlights** → Metal or Glass
- **Soft diffuse** → Plastic, Wood, or Fabric
- **Transparent/translucent** → Glass
- **Rough texture** → Concrete, Wood, or Fabric
- **Uniform matte** → Plastic or painted surface

# LIGHTING ESTIMATION
Analyze shadows and highlights to set up lighting:
- **Hard shadows** → Directional light (sun) or spotlight
- **Soft shadows** → Ambient/diffuse lighting or overcast
- **Shadow direction** → Light source is opposite to shadow
- **Highlight position** → Light source direction
- **Multiple shadows** → Multiple light sources

# OUTPUT FORMAT
1. **Scene Analysis** (2-3 sentences): Describe objects, their spatial relationships, estimated depths, and lighting
2. **Scene Graph**: List parent-child relationships
3. **Code**: Output Rune script in a ```rune code block
4. Use descriptive names for objects
5. Position objects with correct depth (z-axis for distance from camera)
6. Match colors and materials using visual inference

# COARSE-TO-FINE REFINEMENT STRATEGY
Follow this progression across iterations:

**Iterations 1-3: STRUCTURE**
- Get major objects in place
- Correct positions and sizes
- Establish scene graph hierarchy

**Iterations 4-6: APPEARANCE**
- Refine colors to match reference
- Apply correct materials
- Adjust lighting setup

**Iterations 7-10: DETAILS**
- Fine-tune positions
- Add small objects
- Perfect lighting and shadows

# ITERATION FEEDBACK
If you receive feedback from a previous iteration:
- Focus on the specific issues mentioned
- Follow the coarse-to-fine strategy for your current iteration
- Make targeted improvements
- Don't completely rewrite working code
- Preserve what's already correct"#.to_string()
    }
    
    /// Build the user prompt for initial generation
    pub fn build_initial_prompt(&self, context: &VigaContext) -> String {
        let mut prompt = String::new();
        
        prompt.push_str("# Reference Image\n");
        prompt.push_str("[Image attached]\n\n");
        
        if let Some(ref desc) = context.description {
            prompt.push_str(&format!("# User Description\n{}\n\n", desc));
        }
        
        prompt.push_str("Generate Rune script code to recreate this scene in 3D.\n");
        
        prompt
    }
    
    /// Build the user prompt for iteration with feedback
    pub fn build_iteration_prompt(&self, context: &VigaContext, feedback: &str) -> String {
        let mut prompt = String::new();
        
        prompt.push_str("# Reference Image\n");
        prompt.push_str("[Image attached]\n\n");
        
        prompt.push_str("# Current Rendered Scene\n");
        prompt.push_str("[Rendered image attached]\n\n");
        
        prompt.push_str(&format!("# Iteration {} Feedback\n{}\n\n", context.iteration, feedback));
        
        // Include current code
        if let Some(ref best_code) = context.best_code {
            prompt.push_str("# Current Code\n```rune\n");
            prompt.push_str(best_code);
            prompt.push_str("\n```\n\n");
        }
        
        prompt.push_str("Improve the code based on the feedback. Output the complete updated Rune script.\n");
        
        prompt
    }
    
    /// Build planning prompt (optional first step)
    pub fn build_planning_prompt(&self, context: &VigaContext) -> String {
        let mut prompt = String::new();
        
        prompt.push_str("# Reference Image\n");
        prompt.push_str("[Image attached]\n\n");
        
        if let Some(ref desc) = context.description {
            prompt.push_str(&format!("# User Description\n{}\n\n", desc));
        }
        
        prompt.push_str(r#"Before generating code, create a brief plan:

1. **Scene Analysis**: What objects/elements do you see?
2. **Spatial Layout**: How are objects positioned relative to each other?
3. **Materials & Colors**: What materials and colors are visible?
4. **Lighting**: What lighting conditions are present?
5. **Camera Angle**: What viewpoint should recreate this image?

Keep the plan concise (5-10 bullet points max).
"#);
        
        prompt
    }
    
    /// Extract Rune code from LLM response
    pub fn extract_rune_code(&self, response: &str) -> Option<String> {
        // Look for ```rune code block
        if let Some(start) = response.find("```rune") {
            let code_start = start + 7; // Skip "```rune"
            if let Some(newline) = response[code_start..].find('\n') {
                let actual_start = code_start + newline + 1;
                if let Some(end) = response[actual_start..].find("```") {
                    return Some(response[actual_start..actual_start + end].trim().to_string());
                }
            }
        }
        
        // Try generic code block
        if let Some(start) = response.find("```") {
            let code_start = start + 3;
            if let Some(newline) = response[code_start..].find('\n') {
                let actual_start = code_start + newline + 1;
                if let Some(end) = response[actual_start..].find("```") {
                    return Some(response[actual_start..actual_start + end].trim().to_string());
                }
            }
        }
        
        // No code block found - check if response is mostly code
        let lines: Vec<&str> = response.lines().collect();
        let code_lines = lines.iter()
            .filter(|l| l.contains("soul::") || l.starts_with("//") || l.trim().is_empty())
            .count();
        
        if code_lines > lines.len() / 2 {
            return Some(response.trim().to_string());
        }
        
        None
    }
}
