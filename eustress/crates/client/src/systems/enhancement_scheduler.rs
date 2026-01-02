//! Enhancement scheduler - The brain of the AI pipeline
//! Manages cache, triggers generation, and handles background tasks

use bevy::prelude::*;
use eustress_common::{NodeCategory, DetailLevel};
use crate::components::{PendingEnhancement, EnhancingInProgress, Enhanced};
use sha2::{Sha256, Digest};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Resource for managing enhancement cache
#[derive(Resource)]
pub struct EnhancementCache {
    pub path: PathBuf,
}

impl Default for EnhancementCache {
    fn default() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("eustress/enhancement");
        
        std::fs::create_dir_all(&cache_dir).ok();
        
        Self { path: cache_dir }
    }
}

/// Message to trigger enhancement of a node
#[derive(bevy::prelude::Message)]
#[allow(dead_code)]
pub struct EnhanceNodeEvent {
    pub entity: Entity,
    pub node_id: u32,
    pub prompt: String,
    pub category: NodeCategory,
    pub detail_level: DetailLevel,
}

/// Channel for async generation results
#[derive(Resource)]
pub struct GenerationResultChannel {
    pub sender: Arc<Mutex<std::sync::mpsc::Sender<GenerationResult>>>,
    pub receiver: Arc<Mutex<std::sync::mpsc::Receiver<GenerationResult>>>,
}

impl Default for GenerationResultChannel {
    fn default() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            sender: Arc::new(Mutex::new(sender)),
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

/// Result from background generation
#[allow(dead_code)]
pub struct GenerationResult {
    pub entity: Entity,
    pub node_id: u32,
    pub cache_key: String,
    pub asset_path: PathBuf,
    pub success: bool,
    pub error: Option<String>,
}

/// Setup the enhancement cache
pub fn setup_enhancement_cache(mut commands: Commands) {
    let cache = EnhancementCache::default();
    info!("📦 Enhancement cache: {:?}", cache.path);
    commands.insert_resource(cache);
    commands.insert_resource(GenerationResultChannel::default());
}

/// Generate cache key from prompt + category + detail level
pub fn cache_key(prompt: &str, category: &NodeCategory, detail: &DetailLevel) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    hasher.update(format!("{:?}{:?}", category, detail).as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string() // First 16 hex chars
}

/// Main enhancement scheduler system
/// Checks for pending enhancements and triggers generation
pub fn enhancement_scheduler_system(
    mut commands: Commands,
    cache: Res<EnhancementCache>,
    channel: Res<GenerationResultChannel>,
    pending_query: Query<(Entity, &PendingEnhancement), Without<EnhancingInProgress>>,
    enhancing_query: Query<&EnhancingInProgress>,
) {
    // Check for completed generations first
    if let Ok(receiver) = channel.receiver.lock() {
        while let Ok(result) = receiver.try_recv() {
            handle_generation_result(&mut commands, result);
        }
    }
    
    // Limit concurrent generations to avoid overwhelming the GPU
    let concurrent_limit = 2;
    let current_count = enhancing_query.iter().count();
    
    if current_count >= concurrent_limit {
        return;
    }
    
    // Process pending enhancements
    for (entity, pending) in pending_query.iter().take(concurrent_limit - current_count) {
        let key = cache_key(&pending.prompt, &pending.category, &pending.detail_level);
        let cache_path = cache.path.join(format!("{}.glb", key));
        
        // Check cache first
        if cache_path.exists() {
            info!("⚡ Cache hit for: {}", pending.prompt);
            commands.entity(entity).insert(Enhanced {
                cache_key: key.clone(),
                generated_at: std::time::SystemTime::now(),
            });
            commands.entity(entity).remove::<PendingEnhancement>();
            
            // Send immediate load event
            if let Ok(sender) = channel.sender.lock() {
                sender.send(GenerationResult {
                    entity,
                    node_id: pending.node_id,
                    cache_key: key,
                    asset_path: cache_path,
                    success: true,
                    error: None,
                }).ok();
            }
            continue;
        }
        
        // Start generation
        info!("🎨 Starting enhancement: '{}'", pending.prompt);
        info!("   Category: {:?}, Detail: {:?}", pending.category, pending.detail_level);
        
        commands.entity(entity).insert(EnhancingInProgress {
            started_at: std::time::Instant::now(),
        });
        
        // Spawn async generation task
        trigger_generation(
            entity,
            pending.node_id,
            pending.prompt.clone(),
            pending.category,
            pending.detail_level,
            cache_path,
            key.clone(),
            channel.sender.clone(),
        );
    }
}

/// Trigger async generation via local Python server
fn trigger_generation(
    entity: Entity,
    node_id: u32,
    prompt: String,
    category: NodeCategory,
    detail: DetailLevel,
    cache_path: PathBuf,
    key: String,
    sender: Arc<Mutex<std::sync::mpsc::Sender<GenerationResult>>>,
) {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let result = generate_asset(&prompt, &category, &detail, &cache_path).await;
            
            if let Ok(sender) = sender.lock() {
                sender.send(GenerationResult {
                    entity,
                    node_id,
                    cache_key: key,
                    asset_path: cache_path.clone(),
                    success: result.is_ok(),
                    error: result.err().map(|e| e.to_string()),
                }).ok();
            }
        });
    });
}

/// Call the local Python generation server
async fn generate_asset(
    prompt: &str,
    category: &NodeCategory,
    _detail: &DetailLevel,
    cache_path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    
    // Add category context to prompt
    let enhanced_prompt = format!("{} ({})", prompt, category_context(category));
    
    info!("🌐 Calling generation server: {}", enhanced_prompt);
    
    let response = client
        .post("http://127.0.0.1:8001/mesh")
        .json(&serde_json::json!({ "prompt": enhanced_prompt }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Server returned: {}", response.status()).into());
    }
    
    let data: serde_json::Value = response.json().await?;
    let glb_base64 = data["glb_base64"]
        .as_str()
        .ok_or("Missing glb_base64 in response")?;
    
    // Decode and save to cache
    use base64::{Engine as _, engine::general_purpose};
    let glb_bytes = general_purpose::STANDARD.decode(glb_base64)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    
    std::fs::write(cache_path, glb_bytes)?;
    
    info!("✅ Generated and cached: {:?}", cache_path);
    Ok(())
}

/// Add context hints based on category
fn category_context(category: &NodeCategory) -> &'static str {
    match category {
        NodeCategory::Terrain => "natural terrain, landscape",
        NodeCategory::Structure => "architectural structure, building",
        NodeCategory::Prop => "game object, prop",
        NodeCategory::NPC => "character, creature",
        NodeCategory::LightSource => "light fixture",
        NodeCategory::Portal => "magical portal, gateway",
        _ => "3D asset",
    }
}

/// Handle completed generation results
fn handle_generation_result(commands: &mut Commands, result: GenerationResult) {
    if result.success {
        info!("🎉 Enhancement complete for entity {:?}", result.entity);
        commands.entity(result.entity).insert(Enhanced {
            cache_key: result.cache_key,
            generated_at: std::time::SystemTime::now(),
        });
        commands.entity(result.entity).remove::<EnhancingInProgress>();
        commands.entity(result.entity).remove::<PendingEnhancement>();
    } else {
        error!("❌ Enhancement failed: {:?}", result.error);
        commands.entity(result.entity).remove::<EnhancingInProgress>();
        // Keep PendingEnhancement to potentially retry later
    }
}

// Note: Add this crate for base64 decoding
// You'll need to add to Cargo.toml: base64 = "0.21"
