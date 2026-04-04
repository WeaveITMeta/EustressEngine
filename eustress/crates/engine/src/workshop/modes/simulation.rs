//! Simulation mode — deep simulation awareness via Eustress Streams.
//!
//! Provides tools for controlling simulation playback, managing watchpoints
//! and breakpoints, and exporting simulation recordings. All tools have
//! access to live stream data for real-time scene awareness.

use crate::workshop::tools::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Control Simulation
// ---------------------------------------------------------------------------

pub struct ControlSimulationTool;

impl ToolHandler for ControlSimulationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "control_simulation",
            description: "Control the simulation playback state. Actions: play (start/resume simulation), pause (freeze at current tick), stop (reset to initial state), step (advance one tick while paused). Also supports set_time_scale to speed up or slow down simulation time.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Control action: play, pause, stop, step, set_time_scale" },
                    "time_scale": { "type": "number", "description": "Time compression factor (1.0 = realtime, 10.0 = 10x speed, 0.1 = slow motion). Only used with set_time_scale action." }
                },
                "required": ["action"]
            }),
            modes: &[WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.simulation.control"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let time_scale = input.get("time_scale").and_then(|v| v.as_f64());

        let valid = matches!(action, "play" | "pause" | "stop" | "step" | "set_time_scale");
        if !valid {
            return ToolResult {
                tool_name: "control_simulation".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!("Unknown action '{}'. Use: play, pause, stop, step, set_time_scale", action),
                structured_data: None,
                stream_topic: None,
            };
        }

        ToolResult {
            tool_name: "control_simulation".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: match action {
                "play" => "Simulation playing".to_string(),
                "pause" => "Simulation paused".to_string(),
                "stop" => "Simulation stopped and reset".to_string(),
                "step" => "Advanced one simulation tick".to_string(),
                "set_time_scale" => format!("Time scale set to {:.1}x", time_scale.unwrap_or(1.0)),
                _ => unreachable!(),
            },
            structured_data: Some(serde_json::json!({
                "action": format!("simulation_{}", action),
                "time_scale": time_scale,
            })),
            stream_topic: Some("workshop.simulation.control".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Set Breakpoint
// ---------------------------------------------------------------------------

pub struct SetBreakpointTool;

impl ToolHandler for SetBreakpointTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "set_breakpoint",
            description: "Set a conditional breakpoint that pauses the simulation when a watchpoint value meets a condition. The simulation freezes when the condition is true, allowing inspection. Conditions: greater_than, less_than, equal_to, not_equal_to.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "watchpoint": { "type": "string", "description": "Watchpoint key name to monitor" },
                    "condition": { "type": "string", "description": "Comparison: greater_than, less_than, equal_to, not_equal_to" },
                    "threshold": { "type": "number", "description": "Value to compare against" }
                },
                "required": ["watchpoint", "condition", "threshold"]
            }),
            modes: &[WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.simulation.breakpoint"],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let watchpoint = input.get("watchpoint").and_then(|v| v.as_str()).unwrap_or("");
        let condition = input.get("condition").and_then(|v| v.as_str()).unwrap_or("");
        let threshold = input.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.0);

        ToolResult {
            tool_name: "set_breakpoint".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Breakpoint set: pause when {} {} {:.4}", watchpoint, condition, threshold),
            structured_data: Some(serde_json::json!({
                "action": "set_breakpoint",
                "watchpoint": watchpoint,
                "condition": condition,
                "threshold": threshold,
            })),
            stream_topic: Some("workshop.simulation.breakpoint".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Export Recording
// ---------------------------------------------------------------------------

pub struct ExportRecordingTool;

impl ToolHandler for ExportRecordingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "export_recording",
            description: "Export the current simulation recording to a CSV or JSON file. Captures all watchpoint time-series data from the most recent simulation run. The recording starts when play is pressed and stops when stop is pressed.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "format": { "type": "string", "description": "Export format: csv, json", "default": "csv" },
                    "filename": { "type": "string", "description": "Output filename (without extension)" }
                },
                "required": ["filename"]
            }),
            modes: &[WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.simulation.export"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let format = input.get("format").and_then(|v| v.as_str()).unwrap_or("csv");
        let filename = input.get("filename").and_then(|v| v.as_str()).unwrap_or("recording");
        let ext = if format == "json" { "json" } else { "csv" };
        let output_path = ctx.space_root.join("Workspace").join(format!("{}.{}", filename, ext));

        ToolResult {
            tool_name: "export_recording".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Exporting simulation recording to {}", output_path.display()),
            structured_data: Some(serde_json::json!({
                "action": "export_recording",
                "format": format,
                "path": output_path.to_string_lossy(),
            })),
            stream_topic: Some("workshop.simulation.export".to_string()),
        }
    }
}
