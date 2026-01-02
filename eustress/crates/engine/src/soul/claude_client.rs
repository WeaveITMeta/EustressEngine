//! # Claude API Client
//!
//! HTTP client for Anthropic's Claude API with error feedback loop.
//! Supports code generation, validation, and automatic error correction.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use eustress_common::soul::{ClaudeConfig, ModelTier, SoulAST};
use super::scope::{ScriptLocation, SystemPromptBuilder};

// ============================================================================
// API Types
// ============================================================================

/// Claude API request message
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Claude API request
#[derive(Debug, Clone, Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

/// Claude API response content block
#[derive(Debug, Clone, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}

/// Claude API response
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>,
}

/// Token usage info
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// API error response
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

// ============================================================================
// Client Errors
// ============================================================================

/// Claude client errors
#[derive(Debug, Clone)]
pub enum ClaudeError {
    /// No API key configured
    NoApiKey,
    /// Rate limited
    RateLimited { retry_after: Option<u64> },
    /// Network error
    NetworkError(String),
    /// API error
    ApiError { error_type: String, message: String },
    /// Invalid response
    InvalidResponse(String),
    /// Timeout
    Timeout,
    /// Compilation error in generated code
    CompilationError { errors: Vec<CompileError> },
}

impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeError::NoApiKey => write!(f, "No API key configured"),
            ClaudeError::RateLimited { retry_after } => {
                if let Some(secs) = retry_after {
                    write!(f, "Rate limited, retry after {} seconds", secs)
                } else {
                    write!(f, "Rate limited")
                }
            }
            ClaudeError::NetworkError(e) => write!(f, "Network error: {}", e),
            ClaudeError::ApiError { error_type, message } => {
                write!(f, "API error ({}): {}", error_type, message)
            }
            ClaudeError::InvalidResponse(e) => write!(f, "Invalid response: {}", e),
            ClaudeError::Timeout => write!(f, "Request timed out"),
            ClaudeError::CompilationError { errors } => {
                write!(f, "Compilation errors: {}", errors.len())
            }
        }
    }
}

impl std::error::Error for ClaudeError {}

/// Compilation error from rustc
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub code: Option<String>,
    pub suggestion: Option<String>,
}

// ============================================================================
// Generation Result
// ============================================================================

/// Result of code generation with feedback
#[derive(Debug, Clone)]
pub struct GenerationResult {
    /// Generated Rust code
    pub code: String,
    /// Model used
    pub model: String,
    /// Generation duration
    pub duration_ms: u64,
    /// Number of fix iterations
    pub fix_iterations: u32,
    /// Token usage
    pub tokens_used: u32,
    /// Warnings from compilation
    pub warnings: Vec<String>,
    /// Whether code was fixed automatically
    pub was_fixed: bool,
}

// ============================================================================
// Claude Client
// ============================================================================

/// Claude API client with error feedback loop
pub struct ClaudeClient {
    config: ClaudeConfig,
    /// Maximum fix iterations before giving up
    max_fix_iterations: u32,
    /// System prompt for code generation
    system_prompt: String,
    /// System prompt for error fixing
    fix_prompt: String,
}

impl ClaudeClient {
    /// Create a new Claude client
    pub fn new(config: ClaudeConfig) -> Self {
        Self {
            config,
            max_fix_iterations: 10,
            system_prompt: Self::default_system_prompt(),
            fix_prompt: Self::default_fix_prompt(),
        }
    }
    
    /// Get a clone of the config (for spawning async tasks)
    pub fn get_config(&self) -> ClaudeConfig {
        self.config.clone()
    }
    
    /// Default system prompt for code generation
    fn default_system_prompt() -> String {
        r#"You are an expert Rust and Bevy game engine developer for the Eustress game engine. Your task is to interpret ANY natural language description and generate working Rust/Bevy code that brings it to life in a 3D game world.

# INTERPRETATION GUIDELINES
1. Read the user's description as CREATIVE INTENT - they want to see it in 3D
2. Extract entities, objects, relationships, and behaviors from the text
3. For scientific/educational content: create visual representations
4. If sizes/positions aren't specified, use reasonable defaults
5. Make it VISUALLY INTERESTING - add colors, materials, animations

# CRITICAL REQUIREMENTS
1. Generate ONLY valid Rust code - no explanations, no markdown
2. Use Bevy 0.15+ ECS patterns
3. All code must compile without errors
4. Generate a Plugin implementation
5. ALWAYS spawn at least one visible entity

# EUSTRESS ENGINE API

## Required Imports
```rust
use bevy::prelude::*;
use bevy::math::primitives::{Cuboid, Sphere, Cylinder};
use avian3d::prelude::{Collider, RigidBody};
```

## Spawning Parts (Cubes, Spheres, etc.)
```rust
fn spawn_entities(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Spawn a cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(width, height, depth))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            ..default()
        })),
        Transform::from_xyz(x, y, z),
        Collider::cuboid(width/2.0, height/2.0, depth/2.0),
        RigidBody::Static,
        Name::new("MyCube"),
    ));

    // Spawn a sphere
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(radius))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            emissive: LinearRgba::rgb(r, g, b), // For glowing objects
            ..default()
        })),
        Transform::from_xyz(x, y, z),
        Collider::sphere(radius),
        RigidBody::Static,
        Name::new("MySphere"),
    ));

    // Spawn a cylinder
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(radius, height))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            ..default()
        })),
        Transform::from_xyz(x, y, z),
        Collider::cylinder(radius, height),
        RigidBody::Static,
        Name::new("MyCylinder"),
    ));
}
```

## Finding Existing Entities by Name
```rust
fn find_entity_system(
    query: Query<(Entity, &Name, &Transform)>,
) {
    for (entity, name, transform) in &query {
        if name.as_str() == "Welcome Cube" {
            // Found it! Use transform.translation for position
            let pos = transform.translation;
        }
    }
}
```

## AI Training Opt-In (Quality Control)
```rust
// Mark entities for SpatialVortex training data export
use eustress_common::classes::Instance;

fn mark_for_training(
    mut query: Query<&mut Instance>,
) {
    for mut instance in &mut query {
        // Only high-quality, curated entities should be marked
        instance.ai = true;  // Opts into training data export
    }
}

// Check if entity is in training set
fn check_training_status(
    query: Query<&Instance>,
) {
    for instance in &query {
        if instance.ai {
            // This entity will be exported to SpatialVortex
        }
    }
}
```

## Marker Components for Queries
```rust
#[derive(Component)]
struct Planet {
    orbit_radius: f32,
    orbit_speed: f32,
}

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Rotates {
    speed: f32,
}
```

## Animation Systems
```rust
// Self-rotation
fn rotate_system(time: Res<Time>, mut query: Query<(&mut Transform, &Rotates)>) {
    for (mut transform, rotates) in &mut query {
        transform.rotate_y(time.delta_secs() * rotates.speed);
    }
}

// Orbit around origin
fn orbit_system(time: Res<Time>, mut query: Query<(&mut Transform, &Planet)>) {
    for (mut transform, planet) in &mut query {
        let angle = time.elapsed_secs() * planet.orbit_speed;
        transform.translation.x = planet.orbit_radius * angle.cos();
        transform.translation.z = planet.orbit_radius * angle.sin();
    }
}
```

## Plugin Structure
```rust
pub struct MyScriptPlugin;

impl Plugin for MyScriptPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_entities)
           .add_systems(Update, (rotate_system, orbit_system));
    }
}
```

## Color Reference
- Red: Color::srgb(1.0, 0.0, 0.0)
- Green: Color::srgb(0.0, 1.0, 0.0)
- Blue: Color::srgb(0.0, 0.0, 1.0)
- Yellow: Color::srgb(1.0, 1.0, 0.0)
- Orange: Color::srgb(1.0, 0.5, 0.0)
- White: Color::WHITE
- Gray: Color::srgb(0.5, 0.5, 0.5)

## Parameter Scopes (Data Filtering)
Scopes enable selective data filtering across Data Sources, Domains, and Instances.
Use scopes to highlight specific subsets of data for visualization or processing.

### Scope Types
- **ExplicitScope**: Data Source level - explicit list of source IDs
- **QueryScope**: Domain level - query-based filtering with conditions  
- **TagScope**: Instance level - tag-based entity filtering

### Scope Operations
- ScopeOperation::Include - Match ANY criteria (OR)
- ScopeOperation::Exclude - Blacklist matching items
- ScopeOperation::IncludeAll - Match ALL criteria (AND)

### Creating Scopes
```rust
// ExplicitScope - Data Source level
let source_scope = ExplicitScope::new("Production Sources")
    .with_source("hospital_fhir")
    .with_source("icu_mqtt")
    .with_operation(ScopeOperation::Include)
    .with_min_rank(100);  // Require rank 100+

// QueryScope - Domain level
let domain_scope = QueryScope::new("Critical Patients")
    .where_eq("status", "critical")
    .with_condition("priority", QueryOperator::GreaterThan, "5")
    .with_domains(vec!["Patient".into(), "Bed".into()])
    .with_operation(ScopeOperation::IncludeAll);

// TagScope - Instance level
let instance_scope = TagScope::new("ICU Monitoring")
    .with_tag("icu")
    .with_tag("monitored")
    .with_operation(ScopeOperation::Include);

// Combined ParameterScope
let scope = ParameterScope::new("icu_critical", "ICU Critical Patients")
    .with_source_scope(source_scope)
    .with_domain_scope(domain_scope)
    .with_instance_scope(instance_scope)
    .with_priority(10)
    .with_color([1.0, 0.0, 0.0, 1.0]);  // Red
```

### ActiveScopes Resource
```rust
// Access active scopes
let mut scopes = world.resource_mut::<ActiveScopes>();

// Register and activate
scopes.register_scope(icu_scope);
scopes.activate("icu_critical");
scopes.deactivate("general_ward");
scopes.toggle("emergency");

// Check if data passes active scopes
if scopes.source_in_scope("hospital_fhir") { /* source passes */ }
if scopes.domain_in_scope("Patient") { /* domain passes */ }
if scopes.instance_in_scope("hospital_fhir", "Patient", &entity_tags) { /* all pass */ }

// Permission-based access
scopes.set_user(user_id, group_memberships);
if scopes.user_can_use_scope("admin_scope") {
    scopes.activate("admin_scope");
}
let available = scopes.get_available_scopes();  // Scopes user can access
```

### Scope Cascading
Scopes cascade: Data Source → Domain → Instance. Each level can only narrow (not expand) parent scope.

## Scale Reference (1 unit = 1 meter)
CRITICAL: In Eustress Engine, 1 unit equals exactly 1 meter. Use real-world metric measurements.

### Human & Character Dimensions
- Adult human height: 1.7-1.8 meters (average ~1.75m)
- Child height: 1.0-1.4 meters
- Human shoulder width: 0.45 meters
- Human arm span: ~1.7 meters (roughly equal to height)
- Eye level (standing): 1.6 meters

### Architectural Measurements
- Standard door: 2.1m height × 0.9m width
- Double door: 2.1m height × 1.8m width
- Single story height (floor to floor): 3.0 meters
- Ceiling height (floor to ceiling): 2.4-2.7 meters
- Standard stair step: 0.18m rise × 0.28m tread
- Hallway width: 1.2-1.5 meters minimum
- Standard window: 1.2m height × 0.9m width

### Furniture & Objects
- Dining table: 0.75m height, 1.5m × 0.9m surface
- Chair seat height: 0.45 meters
- Desk: 0.75m height
- Bed (single): 2.0m × 1.0m
- Bed (double): 2.0m × 1.5m
- Sofa: 0.45m seat height, 2.0m length
- Kitchen counter: 0.9m height

### Vehicles
- Sedan car: 4.5m length × 1.8m width × 1.5m height
- SUV: 4.8m length × 1.9m width × 1.7m height
- Bus: 12m length × 2.5m width × 3.2m height
- Bicycle: 1.8m length × 0.6m width × 1.1m height

### Outdoor & Environment
- Street lane width: 3.5 meters
- Sidewalk width: 1.5-2.0 meters
- Parking space: 5.0m × 2.5m
- Tree (small): 3-5 meters height
- Tree (large): 10-20 meters height
- Street lamp: 5-8 meters height

### Game Objects (scaled for visualization)
- Small pickup item: 0.2-0.5 meters
- Crate/box: 0.5-1.0 meters
- Platform: 2-4 meters wide
- Planet (scaled for scene): 5-50 meters depending on visualization

# OUTPUT FORMAT
Start with imports, then components, then systems, then Plugin.
NO explanations, NO markdown formatting, ONLY Rust code."#.to_string()
    }
    
    /// Default prompt for fixing compilation errors
    fn default_fix_prompt() -> String {
        r#"The following Rust code has compilation errors. Fix ALL errors while maintaining the original functionality.

RULES:
1. Output ONLY the corrected code - no explanations
2. Fix all errors listed below
3. Do not change the overall structure unless necessary
4. Ensure all imports are correct
5. Make minimal changes to fix the issues

COMPILATION ERRORS:
{errors}

ORIGINAL CODE:
```rust
{code}
```

Output the corrected code:"#.to_string()
    }
    
    /// Generate code from Soul AST with automatic error correction (legacy)
    pub fn generate_with_feedback(
        &self,
        ast: &SoulAST,
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        let start = Instant::now();
        let tier = self.config.select_tier(ast.complexity_score());
        
        // Initial generation
        let prompt = self.build_generation_prompt(ast);
        let mut code = self.call_api(&prompt, tier)?;
        let mut iterations = 0;
        let total_tokens = 0;
        let mut was_fixed = false;
        
        // Extract code from response
        code = self.extract_code(&code);
        
        // Validation and fix loop
        loop {
            let validation = validator.validate_syntax(&code);
            
            if validation.valid {
                // Code compiles! Return success
                return Ok(GenerationResult {
                    code,
                    model: self.config.model_for_tier(tier).to_string(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    fix_iterations: iterations,
                    tokens_used: total_tokens,
                    warnings: validation.warnings,
                    was_fixed,
                });
            }
            
            // Check if we've exceeded max iterations
            iterations += 1;
            if iterations > self.max_fix_iterations {
                return Err(ClaudeError::CompilationError {
                    errors: validation.errors.iter().map(|e| CompileError {
                        message: e.clone(),
                        line: None,
                        column: None,
                        code: None,
                        suggestion: None,
                    }).collect(),
                });
            }
            
            // Request fix from Claude
            was_fixed = true;
            let fix_prompt = self.build_fix_prompt(&code, &validation.errors);
            
            // Use higher tier for fixes
            let fix_tier = tier.upgrade().unwrap_or(tier);
            code = self.call_api(&fix_prompt, fix_tier)?;
            code = self.extract_code(&code);
        }
    }
    
    /// Generate code from RAW MARKDOWN - no AST parsing required
    /// This interprets any natural language description and generates Bevy code
    /// 
    /// # Arguments
    /// * `markdown` - The user's free-form description
    /// * `script_name` - Name for the generated plugin
    /// * `scene_entities` - List of entity names currently in the scene
    /// * `validator` - Code validator for syntax checking
    pub fn generate_from_markdown(
        &self,
        markdown: &str,
        script_name: &str,
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        self.generate_from_markdown_with_context(markdown, script_name, &[], validator)
    }
    
    /// Generate code from markdown with scene context (legacy - string names only)
    pub fn generate_from_markdown_with_context(
        &self,
        markdown: &str,
        script_name: &str,
        scene_entities: &[String],
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        let start = Instant::now();
        
        // Use Sonnet tier for natural language interpretation (good balance)
        let tier = ModelTier::Sonnet;
        
        // Build prompt with raw markdown and scene context
        let prompt = self.build_markdown_prompt_with_context(markdown, script_name, scene_entities);
        info!("🧠 Sending to Claude: {} chars of markdown, {} scene entities", 
              markdown.len(), scene_entities.len());
        
        self.generate_with_prompt(prompt, tier, validator)
    }
    
    /// Generate code from markdown with full scene context (positions, colors, sizes)
    pub fn generate_from_markdown_with_scene(
        &self,
        markdown: &str,
        script_name: &str,
        scene_context: &[super::build_pipeline::SceneEntityContext],
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        // Use default SoulService location
        self.generate_from_markdown_with_scope(
            markdown,
            script_name,
            scene_context,
            ScriptLocation::SoulService,
            validator,
        )
    }
    
    /// Generate code from markdown with scope-aware system prompt
    /// 
    /// This is the primary generation method that uses the script's location
    /// to determine the appropriate system prompt and API surface.
    /// 
    /// # Arguments
    /// * `markdown` - The user's free-form description
    /// * `script_name` - Name for the generated plugin/script
    /// * `scene_context` - Scene entities with positions, colors, sizes
    /// * `location` - Where the script is located (determines scope and available events)
    /// * `validator` - Code validator for syntax checking
    pub fn generate_from_markdown_with_scope(
        &self,
        markdown: &str,
        script_name: &str,
        scene_context: &[super::build_pipeline::SceneEntityContext],
        location: ScriptLocation,
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        // Use Sonnet tier for natural language interpretation (good balance)
        let tier = ModelTier::Sonnet;
        
        // Build scope-aware prompt
        let prompt = self.build_scope_aware_prompt(markdown, script_name, scene_context, &location);
        info!("🧠 Sending to Claude: {} chars of markdown, {} scene entities, scope: {}", 
              markdown.len(), scene_context.len(), location.display_name());
        
        // Use scope-aware system prompt
        let system_prompt = SystemPromptBuilder::build_system_prompt(&location);
        self.generate_with_prompt_and_system(prompt, system_prompt, tier, validator)
    }
    
    /// Internal: generate code with a pre-built prompt (uses default system prompt)
    fn generate_with_prompt(
        &self,
        prompt: String,
        tier: ModelTier,
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        self.generate_with_prompt_and_system(prompt, self.system_prompt.clone(), tier, validator)
    }
    
    /// Internal: generate code with a custom system prompt
    fn generate_with_prompt_and_system(
        &self,
        prompt: String,
        system_prompt: String,
        tier: ModelTier,
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        let start = Instant::now();
        
        let mut code = self.call_api_with_system(&prompt, &system_prompt, tier)?;
        let mut iterations = 0;
        let total_tokens = 0;
        let mut was_fixed = false;
        
        // Extract code from response
        code = self.extract_code(&code);
        info!("📝 Generated {} chars of Rune code", code.len());
        
        // For Rune scripts, skip Rust validation - Rune validates at runtime
        // Just return the generated code directly
        let validation = validator.validate_rune_syntax(&code);
        
        Ok(GenerationResult {
            code,
            model: self.config.model_for_tier(tier).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
            fix_iterations: iterations,
            tokens_used: total_tokens,
            warnings: validation.warnings,
            was_fixed,
        })
    }
    
    /// Build prompt from raw markdown content (no scene context)
    fn build_markdown_prompt(&self, markdown: &str, script_name: &str) -> String {
        self.build_markdown_prompt_with_context(markdown, script_name, &[])
    }
    
    /// Build prompt from raw markdown with scene context
    fn build_markdown_prompt_with_context(
        &self, 
        markdown: &str, 
        script_name: &str,
        scene_entities: &[String],
    ) -> String {
        let scene_context = if scene_entities.is_empty() {
            String::new()
        } else {
            format!(
                r#"
# SCENE CONTEXT
The following entities already exist in the scene and can be referenced by name:
{}

You can find these entities using:
```rust
fn find_entity(query: Query<(Entity, &Name, &Transform)>) {{
    for (entity, name, transform) in &query {{
        if name.as_str() == "EntityName" {{
            // Use transform.translation for position
        }}
    }}
}}
```
"#,
                scene_entities.iter()
                    .map(|e| format!("- \"{}\"", e))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        
        format!(
            r#"Create a Bevy plugin called "{script_name}Plugin" based on this description:
{scene_context}
# USER DESCRIPTION
---
{markdown}
---

Generate complete, compilable Rust code that visualizes this in 3D.
Remember: Output ONLY Rust code, no explanations."#,
            script_name = script_name,
            scene_context = scene_context,
            markdown = markdown
        )
    }
    
    /// Build prompt from raw markdown with full scene context (positions, colors, sizes)
    fn build_markdown_prompt_with_scene(
        &self, 
        markdown: &str, 
        script_name: &str,
        scene_context: &[super::build_pipeline::SceneEntityContext],
    ) -> String {
        let context_str = if scene_context.is_empty() {
            String::new()
        } else {
            let entities_list = scene_context.iter()
                .map(|e| {
                    let mut desc = format!(
                        "- \"{}\" ({}) at position ({:.1}, {:.1}, {:.1})",
                        e.name, e.entity_type, e.position.0, e.position.1, e.position.2
                    );
                    if let Some(ref color) = e.color {
                        desc.push_str(&format!(", color: {}", color));
                    }
                    if let Some(size) = e.size {
                        desc.push_str(&format!(", size: ({:.1}, {:.1}, {:.1})", size.0, size.1, size.2));
                    }
                    desc
                })
                .collect::<Vec<_>>()
                .join("\n");
            
            format!(
                r#"
# SCENE CONTEXT
The following entities already exist in the scene with their properties:
{entities_list}

To reference an existing entity by name:
```rust
fn find_entity(query: Query<(Entity, &Name, &Transform)>) {{
    for (entity, name, transform) in &query {{
        if name.as_str() == "EntityName" {{
            let position = transform.translation;
        }}
    }}
}}
```
"#,
                entities_list = entities_list
            )
        };
        
        format!(
            r#"Create a Bevy plugin called "{script_name}Plugin" based on this description:
{context_str}
# USER DESCRIPTION
---
{markdown}
---

Generate complete, compilable Rust code that visualizes this in 3D.
Remember: Output ONLY Rust code, no explanations."#,
            script_name = script_name,
            context_str = context_str,
            markdown = markdown
        )
    }
    
    /// Build a scope-aware prompt for code generation (Rune scripts)
    /// 
    /// This prompt is tailored to the script's location, providing appropriate
    /// context about what the script can do and what events are available.
    fn build_scope_aware_prompt(
        &self,
        markdown: &str,
        script_name: &str,
        scene_context: &[super::build_pipeline::SceneEntityContext],
        _location: &ScriptLocation,
    ) -> String {
        // Build scene context section
        let context_str = if scene_context.is_empty() {
            String::new()
        } else {
            let entities_list = scene_context.iter()
                .map(|e| {
                    let mut desc = format!(
                        "- \"{}\" ({}) at ({:.1}, {:.1}, {:.1})",
                        e.name, e.entity_type, e.position.0, e.position.1, e.position.2
                    );
                    if let Some(ref color) = e.color {
                        desc.push_str(&format!(", color: {}", color));
                    }
                    if let Some(size) = e.size {
                        desc.push_str(&format!(", size: ({:.1}, {:.1}, {:.1})", size.0, size.1, size.2));
                    }
                    desc
                })
                .collect::<Vec<_>>()
                .join("\n");
            
            format!(
                r#"
# SCENE ENTITIES (you can reference these by name)
{}
"#,
                entities_list
            )
        };
        
        // Simple Rune-focused task description
        let location_instructions = format!(
            r#"# TASK: {}
Generate Rune script code to accomplish the following:
"#,
            script_name
        );
        
        format!(
            r#"{context_str}
{location_instructions}

# USER DESCRIPTION
---
{markdown}
---

Output ONLY Rune script code using the soul:: functions. No explanations, no markdown blocks."#,
            context_str = context_str,
            location_instructions = location_instructions,
            markdown = markdown
        )
    }
    
    /// Build the generation prompt from AST
    fn build_generation_prompt(&self, ast: &SoulAST) -> String {
        let ast_json = ast.to_json().unwrap_or_else(|_| "{}".to_string());
        
        format!(
            r#"Generate Rust/Bevy code for the following Soul script specification:

Scene: {}
Service: {:?}
Script Type: {:?}

Soul AST:
{}

Required Instances to bind:
{}

Event Handlers:
{}

Generate a complete, compilable Rust module with a Plugin implementation."#,
            ast.scene,
            ast.service,
            ast.script_type,
            ast_json,
            ast.required_instances.iter()
                .map(|r| format!("- {}: {:?}", r.name, r.expected_class))
                .collect::<Vec<_>>()
                .join("\n"),
            ast.event_handlers.iter()
                .map(|h| format!("- {}: when {:?} then {:?}", h.name, h.when, h.then_actions))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
    
    /// Build the fix prompt for compilation errors
    fn build_fix_prompt(&self, code: &str, errors: &[String]) -> String {
        self.fix_prompt
            .replace("{errors}", &errors.join("\n"))
            .replace("{code}", code)
    }
    
    /// Call the Claude API (uses default system prompt)
    fn call_api(&self, prompt: &str, tier: ModelTier) -> Result<String, ClaudeError> {
        self.call_api_with_system(prompt, &self.system_prompt, tier)
    }
    
    /// Call the Claude API with a custom system prompt
    fn call_api_with_system(&self, prompt: &str, system_prompt: &str, tier: ModelTier) -> Result<String, ClaudeError> {
        let api_key = self.config.api_key.as_ref()
            .ok_or(ClaudeError::NoApiKey)?;
        
        let model = self.config.model_for_tier(tier);
        let max_tokens = tier.max_tokens();
        let temperature = tier.temperature();
        let timeout = Duration::from_secs(tier.timeout_secs());
        
        let request = ClaudeRequest {
            model: model.to_string(),
            max_tokens,
            temperature,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            system: Some(system_prompt.to_string()),
        };
        
        // Make HTTP request using ureq
        let response = ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .timeout(timeout)
            .send_json(&request);
        
        match response {
            Ok(resp) => {
                let body: ClaudeResponse = resp.into_json()
                    .map_err(|e| ClaudeError::InvalidResponse(e.to_string()))?;
                
                // Extract text from response
                let text = body.content.iter()
                    .filter_map(|c| c.text.clone())
                    .collect::<Vec<_>>()
                    .join("");
                
                if text.is_empty() {
                    return Err(ClaudeError::InvalidResponse("Empty response".to_string()));
                }
                
                Ok(text)
            }
            Err(ureq::Error::Status(429, _)) => {
                Err(ClaudeError::RateLimited { retry_after: None })
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(ClaudeError::ApiError {
                    error_type: format!("HTTP {}", code),
                    message: body,
                })
            }
            Err(ureq::Error::Transport(e)) => {
                if e.to_string().contains("timed out") {
                    Err(ClaudeError::Timeout)
                } else {
                    Err(ClaudeError::NetworkError(e.to_string()))
                }
            }
        }
    }
    
    /// Extract code from Claude response (handles markdown blocks)
    fn extract_code(&self, response: &str) -> String {
        // Check for rust code block
        if let Some(start) = response.find("```rust") {
            let code_start = start + 7;
            if let Some(end) = response[code_start..].find("```") {
                return response[code_start..code_start + end].trim().to_string();
            }
        }
        
        // Check for generic code block
        if let Some(start) = response.find("```") {
            let after_backticks = start + 3;
            // Skip language identifier
            let code_start = response[after_backticks..].find('\n')
                .map(|n| after_backticks + n + 1)
                .unwrap_or(after_backticks);
            
            if let Some(end) = response[code_start..].find("```") {
                return response[code_start..code_start + end].trim().to_string();
            }
        }
        
        // No code blocks, return as-is
        response.trim().to_string()
    }
    
    /// Generate code without validation (for testing)
    pub fn generate_raw(&self, prompt: &str, tier: ModelTier) -> Result<String, ClaudeError> {
        self.call_api(prompt, tier)
    }
    
    /// Check if API key is configured
    pub fn has_api_key(&self) -> bool {
        self.config.has_api_key()
    }
}

impl Default for ClaudeClient {
    fn default() -> Self {
        Self::new(ClaudeConfig::default())
    }
}

// ============================================================================
// Vision API Support (Project Korah)
// ============================================================================

/// Image content for Claude Vision API
#[derive(Debug, Clone, Serialize)]
pub struct ImageContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub source: ImageSource,
}

/// Image source for Vision API
#[derive(Debug, Clone, Serialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Text content for Vision API
#[derive(Debug, Clone, Serialize)]
pub struct TextContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Mixed content (text or image) for Vision API
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum VisionContent {
    Text(TextContent),
    Image(ImageContent),
}

/// Vision-enabled message
#[derive(Debug, Clone, Serialize)]
pub struct VisionMessage {
    pub role: String,
    pub content: Vec<VisionContent>,
}

/// Vision-enabled request
#[derive(Debug, Clone, Serialize)]
pub struct VisionRequest {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub messages: Vec<VisionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

impl ClaudeClient {
    /// Generate code with vision support (screenshot + optional reference image)
    /// 
    /// This is the primary method for Project Korah's AI architecture workflow.
    /// 
    /// # Arguments
    /// * `prompt` - User's natural language instruction
    /// * `screenshot` - Current viewport screenshot (base64 PNG, without data URL prefix)
    /// * `reference_image` - Optional reference image (base64 PNG, without data URL prefix)
    /// * `scene_context` - Scene context in TOON format
    /// * `build_phase` - Current build phase context
    /// * `validator` - Code validator
    pub fn generate_with_vision(
        &self,
        prompt: &str,
        screenshot: Option<&str>,
        reference_image: Option<&str>,
        scene_context: &str,
        build_phase: &str,
        validator: &super::validator::SoulValidator,
    ) -> Result<GenerationResult, ClaudeError> {
        let start = Instant::now();
        let tier = ModelTier::Sonnet; // Vision works best with Sonnet
        
        // Build the vision request
        let full_prompt = self.build_vision_prompt(prompt, scene_context, build_phase);
        let request = self.build_vision_request(
            &full_prompt,
            screenshot,
            reference_image,
            tier,
        )?;
        
        info!("🧠 Sending vision request to Claude: {} chars prompt, screenshot: {}, reference: {}",
            prompt.len(),
            screenshot.is_some(),
            reference_image.is_some()
        );
        
        // Make the API call
        let response = self.call_vision_api(&request)?;
        let mut code = self.extract_code(&response);
        
        info!("📝 Generated {} chars of Rune code", code.len());
        
        // Validate Rune syntax
        let validation = validator.validate_rune_syntax(&code);
        
        Ok(GenerationResult {
            code,
            model: self.config.model_for_tier(tier).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
            fix_iterations: 0,
            tokens_used: 0, // TODO: Extract from response
            warnings: validation.warnings,
            was_fixed: false,
        })
    }
    
    /// Build the prompt for vision-based generation
    fn build_vision_prompt(&self, user_prompt: &str, scene_context: &str, build_phase: &str) -> String {
        format!(
            r#"# SCENE CONTEXT (TOON Format)
{scene_context}

{build_phase}

# USER REQUEST
{user_prompt}

Generate Rune script code to accomplish this. Use the soul:: API functions.
Output ONLY Rune code, no explanations or markdown blocks."#,
            scene_context = scene_context,
            build_phase = build_phase,
            user_prompt = user_prompt
        )
    }
    
    /// Build a vision-enabled API request
    fn build_vision_request(
        &self,
        prompt: &str,
        screenshot: Option<&str>,
        reference_image: Option<&str>,
        tier: ModelTier,
    ) -> Result<VisionRequest, ClaudeError> {
        let mut content: Vec<VisionContent> = Vec::new();
        
        // Add screenshot if provided
        if let Some(screenshot_data) = screenshot {
            // Strip data URL prefix if present
            let data = screenshot_data
                .strip_prefix("data:image/png;base64,")
                .unwrap_or(screenshot_data);
            
            content.push(VisionContent::Image(ImageContent {
                content_type: "image".to_string(),
                source: ImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: data.to_string(),
                },
            }));
            
            content.push(VisionContent::Text(TextContent {
                content_type: "text".to_string(),
                text: "This is the current viewport screenshot showing the scene.".to_string(),
            }));
        }
        
        // Add reference image if provided
        if let Some(ref_data) = reference_image {
            let data = ref_data
                .strip_prefix("data:image/png;base64,")
                .unwrap_or(ref_data);
            
            content.push(VisionContent::Image(ImageContent {
                content_type: "image".to_string(),
                source: ImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: data.to_string(),
                },
            }));
            
            content.push(VisionContent::Text(TextContent {
                content_type: "text".to_string(),
                text: "This is a reference image showing what the user wants to create.".to_string(),
            }));
        }
        
        // Add the main prompt
        content.push(VisionContent::Text(TextContent {
            content_type: "text".to_string(),
            text: prompt.to_string(),
        }));
        
        Ok(VisionRequest {
            model: self.config.model_for_tier(tier).to_string(),
            max_tokens: tier.max_tokens(),
            temperature: tier.temperature(),
            messages: vec![VisionMessage {
                role: "user".to_string(),
                content,
            }],
            system: Some(self.vision_system_prompt()),
        })
    }
    
    /// System prompt for vision-based architecture generation
    fn vision_system_prompt(&self) -> String {
        r#"You are an expert 3D architect and Rune script developer for the Eustress game engine.

# YOUR ROLE
Analyze screenshots and reference images to understand spatial context, then generate Rune scripts that create or modify 3D scenes.

# COORDINATE SYSTEM
- 1 unit = 1 meter
- Y-axis is UP
- Human scale: ~1.8m height, doors ~2.1m, ceilings ~3m

# RUNE API (soul:: functions)
## Spawning
- spawn_part(shape, w, h, d) -> entity_id  // shape: "cube", "sphere", "cylinder"
- spawn_model(name) -> entity_id
- spawn_point_light() -> entity_id

## Transforms
- set_position(entity_id, x, y, z)
- set_rotation(entity_id, pitch, yaw, roll)  // degrees
- set_size(entity_id, w, h, d)

## Appearance
- set_color(entity_id, r, g, b, a)  // 0.0-1.0
- set_material(entity_id, "Plastic" | "Metal" | "Wood" | "Concrete" | "Brick")
- set_anchored(entity_id, true/false)

## Lights
- set_light_brightness(entity_id, brightness)
- set_light_range(entity_id, range)
- set_light_color(entity_id, r, g, b)

## Finding Entities
- find_entity_by_name(name) -> entity_id  // Returns 0 if not found

# OUTPUT FORMAT
Output ONLY valid Rune script code. No explanations, no markdown.
Start with pub fn main() { ... }"#.to_string()
    }
    
    /// Call the Claude Vision API
    fn call_vision_api(&self, request: &VisionRequest) -> Result<String, ClaudeError> {
        let api_key = self.config.api_key.as_ref()
            .ok_or(ClaudeError::NoApiKey)?;
        
        let timeout = Duration::from_secs(120); // Vision requests can take longer
        
        let response = ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .timeout(timeout)
            .send_json(request);
        
        match response {
            Ok(resp) => {
                let body: ClaudeResponse = resp.into_json()
                    .map_err(|e| ClaudeError::InvalidResponse(e.to_string()))?;
                
                let text = body.content.iter()
                    .filter_map(|c| c.text.clone())
                    .collect::<Vec<_>>()
                    .join("");
                
                if text.is_empty() {
                    return Err(ClaudeError::InvalidResponse("Empty response".to_string()));
                }
                
                Ok(text)
            }
            Err(ureq::Error::Status(429, _)) => {
                Err(ClaudeError::RateLimited { retry_after: None })
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(ClaudeError::ApiError {
                    error_type: format!("HTTP {}", code),
                    message: body,
                })
            }
            Err(ureq::Error::Transport(e)) => {
                if e.to_string().contains("timed out") {
                    Err(ClaudeError::Timeout)
                } else {
                    Err(ClaudeError::NetworkError(e.to_string()))
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_code_rust_block() {
        let client = ClaudeClient::default();
        let response = r#"Here's the code:

```rust
fn main() {
    println!("Hello");
}
```

That should work!"#;
        
        let code = client.extract_code(response);
        assert!(code.contains("fn main()"));
        assert!(code.contains("println!"));
        assert!(!code.contains("```"));
    }
    
    #[test]
    fn test_extract_code_no_block() {
        let client = ClaudeClient::default();
        let response = "fn main() { }";
        
        let code = client.extract_code(response);
        assert_eq!(code, "fn main() { }");
    }
}
