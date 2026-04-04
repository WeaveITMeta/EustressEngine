//! Simulation bridge tools — expose simulation APIs to the AI agent.
//!
//! These tools bridge the gap between the Rune scripting API and the MCP
//! tool interface. They return structured intents that the Workshop system
//! processes against Bevy ECS resources (WatchPointRegistry, CollectionService).
//!
//! Tools:
//! - get_sim_value: read a simulation watchpoint
//! - set_sim_value: write a simulation watchpoint
//! - list_sim_values: list all active watchpoints with values
//! - get_tagged_entities: find entities by CollectionService tag
//! - raycast: cast a ray into the 3D scene and return hit results

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Get Simulation Value
// ---------------------------------------------------------------------------

pub struct GetSimValueTool;

impl ToolHandler for GetSimValueTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_sim_value",
            description: "Read a simulation watchpoint value from the running simulation. Watchpoints are named numeric values tracked during simulation. Common keys: voltage, soc, temperature, pressure, dendrite_risk, cycle_count, capacity_wh, efficiency.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Watchpoint key name" }
                },
                "required": ["key"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        // Intent — the Workshop system reads WatchPointRegistry and fills the result
        ToolResult {
            tool_name: "get_sim_value".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Reading watchpoint '{}'", key),
            structured_data: Some(serde_json::json!({ "action": "get_sim_value", "key": key })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Set Simulation Value
// ---------------------------------------------------------------------------

pub struct SetSimValueTool;

impl ToolHandler for SetSimValueTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "set_sim_value",
            description: "Write a simulation watchpoint value. Injects a value into the simulation that Rune scripts can read via get_sim_value(). Use to set initial conditions, override parameters, or inject test data.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Watchpoint key name" },
                    "value": { "type": "number", "description": "Numeric value to set" }
                },
                "required": ["key", "value"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.set_sim_value"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value = input.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
        ToolResult {
            tool_name: "set_sim_value".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Set watchpoint {} = {}", key, value),
            structured_data: Some(serde_json::json!({ "action": "set_sim_value", "key": key, "value": value })),
            stream_topic: Some("workshop.tool.set_sim_value".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// List All Simulation Values
// ---------------------------------------------------------------------------

pub struct ListSimValuesTool;

impl ToolHandler for ListSimValuesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_sim_values",
            description: "List all active simulation watchpoints with their current, min, max, and average values. Returns every watchpoint registered in the WatchPointRegistry.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult {
            tool_name: "list_sim_values".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: "Listing all simulation watchpoints".to_string(),
            structured_data: Some(serde_json::json!({ "action": "list_sim_values" })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Get Tagged Entities
// ---------------------------------------------------------------------------

pub struct GetTaggedEntitiesTool;

impl ToolHandler for GetTaggedEntitiesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_tagged_entities",
            description: "Find all entities with a specific CollectionService tag. Tags are assigned by Rune scripts via collection_add_tag() or Luau scripts via CollectionService:AddTag(). Returns entity IDs and names.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tag": { "type": "string", "description": "Tag name to search for" }
                },
                "required": ["tag"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let tag = input.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "get_tagged_entities".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Searching entities tagged '{}'", tag),
            structured_data: Some(serde_json::json!({ "action": "get_tagged_entities", "tag": tag })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Raycast
// ---------------------------------------------------------------------------

pub struct RaycastTool;

impl ToolHandler for RaycastTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "raycast",
            description: "Cast a ray into the 3D scene and return the first entity hit. Specify origin point and direction vector. Returns hit position, surface normal, hit entity name, and distance. Uses the same raycast engine as Rune's workspace_raycast().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "origin": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] ray origin in world coordinates" },
                    "direction": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z] ray direction (will be normalized)" },
                    "max_distance": { "type": "number", "description": "Maximum ray distance in studs (default: 1000)", "default": 1000 }
                },
                "required": ["origin", "direction"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let origin = input.get("origin").and_then(|v| v.as_array()).map(|a| {
            [
                a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
                a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
            ]
        }).unwrap_or([0.0, 0.0, 0.0]);

        let direction = input.get("direction").and_then(|v| v.as_array()).map(|a| {
            [
                a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
                a.get(1).and_then(|v| v.as_f64()).unwrap_or(-1.0),
                a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
            ]
        }).unwrap_or([0.0, -1.0, 0.0]);

        let max_distance = input.get("max_distance").and_then(|v| v.as_f64()).unwrap_or(1000.0);

        // Intent — processed by Workshop system using Avian3d physics raycast
        ToolResult {
            tool_name: "raycast".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Raycasting from [{:.1}, {:.1}, {:.1}] direction [{:.1}, {:.1}, {:.1}]",
                origin[0], origin[1], origin[2], direction[0], direction[1], direction[2]),
            structured_data: Some(serde_json::json!({
                "action": "raycast",
                "origin": origin,
                "direction": direction,
                "max_distance": max_distance,
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP Request
// ---------------------------------------------------------------------------

pub struct HttpRequestTool;

impl ToolHandler for HttpRequestTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http_request",
            description: "Make an HTTP request to an external URL. Supports GET and POST methods. Returns status code, headers, and response body. Same capability as Rune's http_request_async() and Luau's HttpService:RequestAsync().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Full URL to request" },
                    "method": { "type": "string", "description": "HTTP method: GET, POST, PUT, DELETE", "default": "GET" },
                    "body": { "type": "string", "description": "Request body (for POST/PUT)" },
                    "headers": { "type": "object", "description": "Additional HTTP headers as key-value pairs" }
                },
                "required": ["url"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.http_request"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return ToolResult {
                tool_name: "http_request".to_string(), tool_use_id: String::new(),
                success: false, content: "Missing required parameter: url".to_string(),
                structured_data: None, stream_topic: None,
            },
        };
        let method = input.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
        let body = input.get("body").and_then(|v| v.as_str());

        let result = match method.to_uppercase().as_str() {
            "GET" => ureq::get(url).call(),
            "POST" => {
                let req = ureq::post(url);
                if let Some(b) = body {
                    req.set("Content-Type", "application/json").send_string(b)
                } else {
                    req.call()
                }
            }
            "PUT" => {
                let req = ureq::put(url);
                if let Some(b) = body {
                    req.set("Content-Type", "application/json").send_string(b)
                } else {
                    req.call()
                }
            }
            "DELETE" => ureq::delete(url).call(),
            _ => return ToolResult {
                tool_name: "http_request".to_string(), tool_use_id: String::new(),
                success: false, content: format!("Unsupported method: {}. Use GET, POST, PUT, or DELETE.", method),
                structured_data: None, stream_topic: None,
            },
        };

        match result {
            Ok(resp) => {
                let status = resp.status();
                let body_text = resp.into_string().unwrap_or_default();
                let truncated = if body_text.len() > 8000 {
                    format!("{}...\n[truncated — {} bytes total]", &body_text[..8000], body_text.len())
                } else {
                    body_text
                };
                ToolResult {
                    tool_name: "http_request".to_string(),
                    tool_use_id: String::new(),
                    success: status < 400,
                    content: format!("HTTP {} {} — {} {}\n{}", method, url, status,
                        if status < 400 { "OK" } else { "Error" }, truncated),
                    structured_data: Some(serde_json::json!({ "status": status, "url": url, "method": method })),
                    stream_topic: Some("workshop.tool.http_request".to_string()),
                }
            }
            Err(e) => ToolResult {
                tool_name: "http_request".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("HTTP {} {} failed: {}", method, url, e),
                structured_data: None,
                stream_topic: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// DataStore Get
// ---------------------------------------------------------------------------

pub struct DataStoreGetTool;

impl ToolHandler for DataStoreGetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "datastore_get",
            description: "Read a value from a DataStore by key. DataStores are named key-value stores persisted to disk. Same API as Rune's datastore_get() and Luau's DataStore:GetAsync().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "store": { "type": "string", "description": "DataStore name" },
                    "key": { "type": "string", "description": "Key to look up" }
                },
                "required": ["store", "key"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let store = input.get("store").and_then(|v| v.as_str()).unwrap_or("");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "datastore_get".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Reading {}:{}", store, key),
            structured_data: Some(serde_json::json!({ "action": "datastore_get", "store": store, "key": key })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// DataStore Set
// ---------------------------------------------------------------------------

pub struct DataStoreSetTool;

impl ToolHandler for DataStoreSetTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "datastore_set",
            description: "Write a value to a DataStore by key. DataStores are named key-value stores persisted to disk. Same API as Rune's datastore_set() and Luau's DataStore:SetAsync().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "store": { "type": "string", "description": "DataStore name" },
                    "key": { "type": "string", "description": "Key to write" },
                    "value": { "type": "string", "description": "Value to store (string-serialized)" }
                },
                "required": ["store", "key", "value"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.datastore_set"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let store = input.get("store").and_then(|v| v.as_str()).unwrap_or("");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value = input.get("value").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "datastore_set".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Set {}:{} = {}", store, key, value),
            structured_data: Some(serde_json::json!({ "action": "datastore_set", "store": store, "key": key, "value": value })),
            stream_topic: Some("workshop.tool.datastore_set".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Add Tag
// ---------------------------------------------------------------------------

pub struct AddTagTool;

impl ToolHandler for AddTagTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "add_tag",
            description: "Add a CollectionService tag to an entity. Tags are used to group entities for batch queries. Same API as Rune's collection_add_tag() and Luau's CollectionService:AddTag().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": { "type": "integer", "description": "Entity ID to tag" },
                    "tag": { "type": "string", "description": "Tag name to add" }
                },
                "required": ["entity_id", "tag"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.add_tag"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let entity_id = input.get("entity_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let tag = input.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "add_tag".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Tagged entity {} with '{}'", entity_id, tag),
            structured_data: Some(serde_json::json!({ "action": "add_tag", "entity_id": entity_id, "tag": tag })),
            stream_topic: Some("workshop.tool.add_tag".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Remove Tag
// ---------------------------------------------------------------------------

pub struct RemoveTagTool;

impl ToolHandler for RemoveTagTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "remove_tag",
            description: "Remove a CollectionService tag from an entity. Same API as Rune's collection_remove_tag() and Luau's CollectionService:RemoveTag().",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": { "type": "integer", "description": "Entity ID to untag" },
                    "tag": { "type": "string", "description": "Tag name to remove" }
                },
                "required": ["entity_id", "tag"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.tool.remove_tag"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let entity_id = input.get("entity_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let tag = input.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "remove_tag".to_string(), tool_use_id: String::new(),
            success: true,
            content: format!("Removed tag '{}' from entity {}", tag, entity_id),
            structured_data: Some(serde_json::json!({ "action": "remove_tag", "entity_id": entity_id, "tag": tag })),
            stream_topic: Some("workshop.tool.remove_tag".to_string()),
        }
    }
}
