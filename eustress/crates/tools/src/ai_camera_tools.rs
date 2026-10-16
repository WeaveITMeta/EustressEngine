//! The AI camera tools: the agent's own off-screen camera in the running
//! engine, aimed and captured over the engine bridge. One implementation for
//! every host that drives agents, so each sees the same names, the same
//! schemas and the same capability classes (`capability.rs`: Read).
//!
//! Every tool here calls the running engine and waits for its reply, and a
//! capture waits for frames to render. A host must never dispatch them on the
//! engine's own main thread: the engine answers bridge calls on that thread,
//! so the call would wait on itself. The MCP server runs them in its own
//! process; the engine's `tools.call` runs every tool on a worker thread. A
//! host registers them with [`register`].

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolRegistry, ToolResult};
use eustress_bridge_client::call_engine;
use serde_json::Value;

/// Register the four AI camera tools.
pub fn register(registry: &mut ToolRegistry) {
    registry.register(AiCameraSetPoseTool);
    registry.register(AiCameraOrbitTool);
    registry.register(AiCameraFrameTool);
    registry.register(AiCameraCaptureTool);
}

/// The tool names [`register`] adds.
pub const NAMES: [&str; 4] = ["ai_camera_set_pose", "ai_camera_orbit", "ai_camera_frame", "ai_camera_capture"];

fn ok(tool_name: &str, content: String, data: Value) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        tool_use_id: String::new(),
        success: true,
        content,
        structured_data: Some(data),
        stream_topic: None,
    }
}

fn fail(tool_name: &str, message: String) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        tool_use_id: String::new(),
        success: false,
        content: message,
        structured_data: None,
        stream_topic: None,
    }
}

pub struct AiCameraSetPoseTool;

impl ToolHandler for AiCameraSetPoseTool {
    /// Moves only the AI's own camera; the Space and the user's view are unchanged.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_set_pose",
            description: "Place the AI's own off-screen camera, separate from the user's viewport (it never changes what the person sees). Params: position [x,y,z] (required), plus either look_at [x,y,z] or rotation [x,y,z,w]. Then call ai_camera_capture to see from this pose. Works in Edit and in Play, in every Space: the engine keeps exactly one AI camera, named \"AI Camera\", and puts it back at its starting pose whenever a Space opens. Requires Studio running; a headless engine has no renderer and answers with an error.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "position": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] world position of the AI camera." },
                    "look_at":  { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] point to look at (use this OR rotation)." },
                    "rotation": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z,w] quaternion orientation (use this OR look_at)." }
                },
                "required": ["position"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "ai_camera.set_pose", input) {
            Ok(r) => ok("ai_camera_set_pose", "AI camera repositioned.".to_string(), r),
            Err(e) => fail("ai_camera_set_pose", e),
        }
    }
}

pub struct AiCameraOrbitTool;

impl ToolHandler for AiCameraOrbitTool {
    /// Moves only the AI's own camera; the Space and the user's view are unchanged.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_orbit",
            description: "Orbit the AI's own off-screen camera around a point, to circle what you are working on. Params: center [x,y,z] (default origin), distance (default 15), yaw_deg (default 45), pitch_deg (default 30). Then call ai_camera_capture to view it. Works in Edit and in Play, in every Space: the engine keeps exactly one AI camera, named \"AI Camera\", and puts it back at its starting pose whenever a Space opens. Requires Studio running; a headless engine has no renderer and answers with an error.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "center":    { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] point to orbit around (default [0,0,0])." },
                    "distance":  { "type": "number", "description": "Distance from center (default 15)." },
                    "yaw_deg":   { "type": "number", "description": "Horizontal angle in degrees (default 45)." },
                    "pitch_deg": { "type": "number", "description": "Vertical angle in degrees (default 30)." }
                }
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "ai_camera.orbit", input) {
            Ok(r) => ok("ai_camera_orbit", "AI camera orbited.".to_string(), r),
            Err(e) => fail("ai_camera_orbit", e),
        }
    }
}

pub struct AiCameraFrameTool;

impl ToolHandler for AiCameraFrameTool {
    /// Moves only the AI's own camera; the Space and the user's view are unchanged.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_frame",
            description: "Point the AI's own off-screen camera at a named entity, backed off to a sensible distance for its size (\"go look at that part\"). Param: name (the entity's Name, e.g. \"House_Roof\"). Then call ai_camera_capture to view it. Works in Edit and in Play, in every Space: the engine keeps exactly one AI camera, named \"AI Camera\", and puts it back at its starting pose whenever a Space opens. Requires Studio running; a headless engine has no renderer and answers with an error.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the entity to frame (e.g. \"House_Floor\")." }
                },
                "required": ["name"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        match call_engine(&ctx.universe_root, "ai_camera.frame", input) {
            Ok(r) => ok("ai_camera_frame", format!("AI camera framed '{name}'."), r),
            Err(e) => fail("ai_camera_frame", e),
        }
    }
}

pub struct AiCameraCaptureTool;

impl ToolHandler for AiCameraCaptureTool {
    /// Renders the AI's own camera to a PNG; nothing in the Space changes.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ai_camera_capture",
            description: "Render the AI's own off-screen camera and return the image inline: your own eyes, separate from capture_viewport (which is the person's window). Aim it first with ai_camera_set_pose, ai_camera_orbit or ai_camera_frame. The camera powers up only for this shot, so it does not slow the person's view, and the call waits for the PNG to finish writing before it returns. Works in Edit and in Play, in every Space: the engine keeps exactly one AI camera, named \"AI Camera\", and puts it back at its starting pose whenever a Space opens. Requires Studio running; a headless engine has no renderer and answers with an error.",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolResult {
        match call_engine(&ctx.universe_root, "ai_camera.capture", serde_json::json!({})) {
            Ok(r) => crate::image_result::capture_result("ai_camera_capture", r, ctx),
            Err(e) => fail("ai_camera_capture", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What every description ends with: where the camera lives and what it needs.
    const AVAILABILITY: &str = "Works in Edit and in Play, in every Space: the engine keeps exactly one AI camera, named \"AI Camera\", and puts it back at its starting pose whenever a Space opens. Requires Studio running; a headless engine has no renderer and answers with an error.";

    /// The four tools register under their names, each classified Read and
    /// marked read-only, and each description says what the camera needs.
    #[test]
    fn the_four_tools_are_registered_and_classified() {
        let mut registry = ToolRegistry::default();
        register(&mut registry);
        let mut names = registry.tool_names();
        names.sort();
        let mut expected = NAMES.to_vec();
        expected.sort();
        assert_eq!(names, expected);
        for (def, read_only) in registry.all_tools_annotated() {
            assert_eq!(crate::capability::capability_of(def.name), Some(crate::Capability::Read), "{}", def.name);
            assert!(read_only, "{}", def.name);
            assert!(def.description.ends_with(AVAILABILITY), "{}", def.name);
        }
    }

    /// With no engine running, each tool fails with the bridge's reason
    /// instead of hanging.
    #[test]
    fn without_an_engine_each_tool_fails_plainly() {
        let universe = std::env::temp_dir().join(format!("eus_ai_camera_tools_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&universe);
        std::fs::create_dir_all(universe.join("Spaces")).unwrap();
        let ctx = ToolContext {
            space_root: universe.join("Spaces"),
            universe_root: universe.clone(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: None,
            permissions: crate::Permissions::standard(),
        };
        let mut registry = ToolRegistry::default();
        register(&mut registry);
        for (name, input) in [
            ("ai_camera_set_pose", serde_json::json!({ "position": [0, 5, 10] })),
            ("ai_camera_orbit", serde_json::json!({})),
            ("ai_camera_frame", serde_json::json!({ "name": "Baseplate" })),
            ("ai_camera_capture", serde_json::json!({})),
        ] {
            let started = std::time::Instant::now();
            let r = registry.dispatch(name, "t", input, &ctx);
            assert!(!r.success, "{name}: {}", r.content);
            assert!(!r.content.trim().is_empty(), "{name}: an empty error");
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "{name} waited on an absent engine");
        }
        let _ = std::fs::remove_dir_all(&universe);
    }
}
