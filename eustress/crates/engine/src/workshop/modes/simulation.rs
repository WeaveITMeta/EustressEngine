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
            description: "Control the simulation playback state. Actions: play (start a run, or resume a paused one), pause (freeze at the current tick), stop (end the run and reset to the initial state), set_time_scale (speed up or slow down a run). Each action is the same command as run_simulation, pause_simulation or stop_simulation. To advance a paused simulation by exact ticks, use sim_step. With several engines on one Universe, pass `pid` to control a specific one.",
            input_schema: eustress_tools::sim_ipc::with_target_props(serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Control action: play, pause, stop, set_time_scale" },
                    "time_scale": { "type": "number", "description": "Time compression factor (1.0 = realtime, 10.0 = 10x speed, 0.1 = slow motion). Required for set_time_scale; optional for play." }
                },
                "required": ["action"]
            })),
            modes: &[WorkshopMode::Simulation],
            requires_approval: false,
            stream_topics: &["workshop.simulation.control"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        use crate::workshop::tools::simulation_tools::{
            PauseSimulationTool, RunSimulationTool, StopSimulationTool,
        };
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        let refuse = |content: String| ToolResult {
            tool_name: "control_simulation".to_string(),
            tool_use_id: String::new(),
            success: false,
            content,
            structured_data: None,
            stream_topic: None,
        };

        // Each action is queued as the real simulation command, the same one
        // the matching tool sends, so the result reports what the engine was
        // actually asked to do. The input passes through whole, so `pid` and
        // the other routing fields reach the command.
        let mut result = match action {
            "play" => RunSimulationTool.execute(input, ctx),
            // `run` on a running or paused run retunes its time scale.
            "set_time_scale" => {
                if input.get("time_scale").and_then(|v| v.as_f64()).is_none() {
                    return refuse("set_time_scale needs a numeric `time_scale`.".to_string());
                }
                RunSimulationTool.execute(input, ctx)
            }
            "pause" => PauseSimulationTool.execute(input, ctx),
            "stop" => StopSimulationTool.execute(input, ctx),
            "step" => {
                return refuse(
                    "control_simulation has no step: use sim_step, which advances a paused simulation by exact ticks.".to_string(),
                )
            }
            other => {
                return refuse(format!("Unknown action '{other}'. Use: play, pause, stop, set_time_scale"));
            }
        };
        result.tool_name = "control_simulation".to_string();
        result
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
        // Nothing a tool can queue creates a breakpoint: the registry lives in
        // the running engine, and the only writers are the Rune simulation
        // bindings (`add_breakpoint`). Reporting "Breakpoint set" here
        // made a caller wait for a pause that could never come, so the call
        // says what it can and cannot do.
        let watchpoint = input.get("watchpoint").and_then(|v| v.as_str()).unwrap_or("");
        ToolResult {
            tool_name: "set_breakpoint".to_string(),
            tool_use_id: String::new(),
            success: false,
            content: format!(
                "No breakpoint was set on '{watchpoint}': breakpoints can only be created from a Rune script \
                 (the simulation bindings' add_breakpoint), not from a tool call. To stop a run at a condition, \
                 run_simulation with duration_s and read the result with await_simulation."
            ),
            structured_data: None,
            stream_topic: None,
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
        let refuse = |content: String| ToolResult {
            tool_name: "export_recording".to_string(),
            tool_use_id: String::new(),
            success: false,
            content,
            structured_data: None,
            stream_topic: None,
        };
        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("csv")
            .trim()
            .to_ascii_lowercase();
        if format != "csv" && format != "json" {
            return refuse(format!("Unknown format '{format}'. Use csv or json."));
        }

        // The name becomes a file name, so only its last path component is
        // used, and only plain characters survive: a request can never write
        // outside the recordings folder.
        let raw = input.get("filename").and_then(|v| v.as_str()).unwrap_or("recording");
        let stem: String = std::path::Path::new(raw)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("recording")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let stem = stem.trim_matches('_');
        let stem = if stem.is_empty() { "recording" } else { stem };

        // Every run's recording is exported when the run stops, to
        // `<universe>/.eustress/knowledge/recordings/<space>/sim_*.json`
        // (simulation/plugin.rs). This re-exports the newest one under the
        // requested name, as JSON or as one CSV per series.
        let space = ctx.space_root.file_name().and_then(|n| n.to_str()).unwrap_or("default");
        let dir = ctx
            .universe_root
            .join(".eustress")
            .join("knowledge")
            .join("recordings")
            .join(space);
        let newest = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("sim_") && name.ends_with(".json")
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .max_by_key(|(modified, _)| *modified)
            .map(|(_, path)| path);
        let Some(source) = newest else {
            return refuse(format!(
                "No recording to export: {} holds none. A recording is written when a simulation run stops \
                 (press Stop, or stop_simulation).",
                dir.display()
            ));
        };
        let recording: eustress_common::simulation::SimulationRecording = match std::fs::read(&source)
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
        {
            Ok(r) => r,
            Err(e) => return refuse(format!("Could not read {}: {e}", source.display())),
        };

        let (output, written) = if format == "json" {
            let path = dir.join(format!("export_{stem}.json"));
            let result = recording.export_json(&path);
            (path, result)
        } else {
            let path = dir.join(format!("export_{stem}"));
            let result = recording.export_csv(&path);
            (path, result)
        };
        match written {
            Ok(()) => ToolResult {
                tool_name: "export_recording".to_string(),
                tool_use_id: String::new(),
                success: true,
                content: format!(
                    "Exported {} ({} series, {} events) to {}",
                    source.file_name().and_then(|n| n.to_str()).unwrap_or("the recording"),
                    recording.series.len(),
                    recording.events.len(),
                    output.display()
                ),
                structured_data: Some(serde_json::json!({
                    "source": source.to_string_lossy(),
                    "path": output.to_string_lossy(),
                    "format": format,
                    "series": recording.series.len(),
                    "events": recording.events.len(),
                })),
                stream_topic: Some("workshop.simulation.export".to_string()),
            },
            Err(e) => refuse(format!("Could not write {}: {e}", output.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workshop::modes::test_context;
    use serde_json::json;

    #[test]
    fn set_breakpoint_does_not_claim_to_set_one() {
        let r = SetBreakpointTool.execute(
            json!({ "watchpoint": "temp", "condition": "greater_than", "threshold": 10.0 }),
            &test_context(),
        );
        assert!(!r.success);
    }

    #[test]
    fn export_recording_with_no_recording_is_an_error() {
        let r = ExportRecordingTool.execute(json!({ "filename": "../../escape", "format": "json" }), &test_context());
        assert!(!r.success);
        assert!(r.content.contains("No recording"));
    }

    #[test]
    fn export_recording_rejects_an_unknown_format() {
        let r = ExportRecordingTool.execute(json!({ "filename": "x", "format": "xlsx" }), &test_context());
        assert!(!r.success);
    }
}
