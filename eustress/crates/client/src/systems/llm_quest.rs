//! LLM-powered quest graph executor
//! Uses local LLM to generate dynamic narrative from connection graph

use bevy::prelude::*;
use eustress_common::{Connection, ConnectionType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resource to track quest state
#[derive(Resource, Default)]
pub struct QuestState {
    /// Player inventory
    pub inventory: HashMap<String, u32>,
    /// Quest flags set by player actions
    pub flags: HashMap<String, String>,
    /// Visited connections (by connection ID)
    pub visited_connections: Vec<u32>,
    /// Current narrative context
    pub narrative_context: String,
}

/// Message when player triggers a connection
#[derive(bevy::prelude::Message)]
pub struct ConnectionTriggeredEvent {
    pub connection: Connection,
}

/// LLM request/response for narrative generation
#[derive(Serialize, Deserialize)]
struct NarrativeRequest {
    connection: String,
    condition: String,
    player_state: PlayerState,
    context: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerState {
    inventory: HashMap<String, u32>,
    flags: HashMap<String, String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct NarrativeResponse {
    narrative: String,
    success: bool,
    updated_flags: Option<HashMap<String, String>>,
}

/// System to handle connection triggers
pub fn quest_executor_system(
    mut events: MessageReader<ConnectionTriggeredEvent>,
    mut quest_state: ResMut<QuestState>,
) {
    for event in events.read() {
        info!("🎭 Connection triggered: {}", event.connection.narrative);
        
        // Check condition
        if !evaluate_condition(&event.connection.condition, &quest_state) {
            info!("❌ Condition not met: {}", event.connection.condition);
            continue;
        }
        
        // Mark as visited
        quest_state.visited_connections.push(event.connection.id);
        
        // Handle different connection types
        match event.connection.connection_type {
            ConnectionType::QuestStep => {
                info!("📜 Quest step completed");
                // Spawn async LLM call for dynamic narrative
                spawn_llm_narrative_generation(&event.connection, &quest_state);
            }
            ConnectionType::DialogueOption => {
                info!("💬 Dialogue triggered");
                // Show dialogue UI with narrative
            }
            ConnectionType::Portal => {
                info!("🌀 Portal activated");
                // Teleport player
            }
            _ => {
                info!("➡️  Connection traversed");
            }
        }
        
        // Update context
        quest_state.narrative_context = event.connection.narrative.clone();
    }
}

/// Evaluate a condition string
fn evaluate_condition(condition: &str, state: &QuestState) -> bool {
    if condition.is_empty() {
        return true;
    }
    
    // Simple parser for conditions like:
    // "player has item:rusty_key"
    // "flag:door_unlocked equals true"
    
    if condition.starts_with("player has item:") {
        let item_name = condition.strip_prefix("player has item:").unwrap();
        return state.inventory.get(item_name).map(|&count| count > 0).unwrap_or(false);
    }
    
    if condition.starts_with("flag:") {
        let parts: Vec<&str> = condition.split(" equals ").collect();
        if parts.len() == 2 {
            let flag_name = parts[0].strip_prefix("flag:").unwrap();
            let expected_value = parts[1];
            return state.flags.get(flag_name).map(|v| v == expected_value).unwrap_or(false);
        }
    }
    
    // Default: always true for simple conditions
    true
}

/// Spawn async task to call LLM for narrative generation
fn spawn_llm_narrative_generation(connection: &Connection, state: &QuestState) {
    let connection_clone = connection.clone();
    let state_clone = PlayerState {
        inventory: state.inventory.clone(),
        flags: state.flags.clone(),
    };
    let context = state.narrative_context.clone();
    
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            match call_llm_for_narrative(&connection_clone, &state_clone, &context).await {
                Ok(response) => {
                    info!("✨ LLM generated narrative: {}", response.narrative);
                    // TODO: Send back to main thread via channel
                }
                Err(e) => {
                    error!("❌ LLM call failed: {}", e);
                }
            }
        });
    });
}

/// Call local LLM server for narrative generation
async fn call_llm_for_narrative(
    connection: &Connection,
    player_state: &PlayerState,
    context: &str,
) -> Result<NarrativeResponse, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    
    let request = NarrativeRequest {
        connection: connection.narrative.clone(),
        condition: connection.condition.clone(),
        player_state: player_state.clone(),
        context: context.to_string(),
    };
    
    // Call local LLM server (to be implemented)
    let response = client
        .post("http://127.0.0.1:8002/narrative")
        .json(&request)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    
    let narrative_response: NarrativeResponse = response.json().await?;
    Ok(narrative_response)
}

/// System to update quest flags from player actions
#[allow(dead_code)]
pub fn update_quest_flags_system(
    // TODO: Listen to player interaction events
    // Update QuestState based on item pickups, NPC interactions, etc.
) {
    // Placeholder
}
