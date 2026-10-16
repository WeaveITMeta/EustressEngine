//! Shell execution tools — Claude can run arbitrary shell commands.
//!
//! Gated by `requires_approval: true` so every invocation surfaces the
//! exact command to the user before it runs. The Universe root is the
//! default working directory; callers may pass a relative `cwd` that
//! must resolve inside the sandbox. Output is captured (stdout+stderr,
//! size-capped) and returned to the model so it can chain further
//! tool calls based on what the command printed.
//!
//! Intended uses (as of 2026-04): orchestrating external HTTP APIs via
//! `curl` (the Roblox Cube3D generate-object flow is the motivating
//! case), inspecting Git state, running scripts, kicking off build
//! pipelines. This tool is the escape hatch — anything we haven't
//! wrapped as a first-class tool becomes a bash invocation.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;
use std::io::Read;
use std::process::{Command, Stdio};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_S: u64 = 300;
const MAX_TIMEOUT_S: u64 = 3000;
const MAX_OUTPUT_BYTES: usize = 16_384;
/// How long a pipe may stay open once the command has exited.
const PIPE_GRACE: Duration = Duration::from_secs(2);

fn err_result(msg: String) -> ToolResult {
    ToolResult {
        tool_name: "run_bash".to_string(),
        tool_use_id: String::new(),
        success: false,
        content: msg,
        structured_data: None,
        stream_topic: None,
    }
}

/// The working directory: a folder inside the Universe, by the shared rules
/// in [`crate::sandbox`]. Empty is the Universe root.
fn resolve_cwd(ctx: &ToolContext, rel: &str) -> Option<PathBuf> {
    crate::sandbox::resolve(&ctx.universe_root, rel).ok().filter(|p| p.is_dir())
}

/// What one pipe has given so far: the first bytes, up to a cap, the total,
/// and whether the pipe has closed.
#[derive(Default)]
struct Captured {
    kept: Vec<u8>,
    total: usize,
    closed: bool,
}

/// Read `pipe` to its end on a thread of its own, so the command never
/// blocks on a full pipe. Keeps the first [`MAX_OUTPUT_BYTES`] (and a few
/// more, so the cut can land on a character boundary) and counts the rest.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Arc<Mutex<Captured>> {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let Some(mut pipe) = pipe else {
        captured.lock().unwrap_or_else(|p| p.into_inner()).closed = true;
        return captured;
    };
    let shared = Arc::clone(&captured);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            let n = match pipe.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => 0,
            };
            let mut c = shared.lock().unwrap_or_else(|p| p.into_inner());
            if n == 0 {
                c.closed = true;
                break;
            }
            c.total += n;
            let room = (MAX_OUTPUT_BYTES + 4).saturating_sub(c.kept.len());
            c.kept.extend_from_slice(&buf[..n.min(room)]);
        }
    });
    captured
}

/// The text a pipe gave, after waiting up to [`PIPE_GRACE`] for it to close,
/// and whether it is still open: a process the command started in the
/// background can hold it after the command exits.
fn collect(captured: &Mutex<Captured>) -> (String, bool) {
    let until = Instant::now() + PIPE_GRACE;
    loop {
        {
            let c = captured.lock().unwrap_or_else(|p| p.into_inner());
            if c.closed || Instant::now() >= until {
                return (shown(&c.kept, c.total), !c.closed);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The kept bytes as text, cut at [`MAX_OUTPUT_BYTES`] on a character
/// boundary, with the total when there was more.
fn shown(kept: &[u8], total: usize) -> String {
    let mut s = String::from_utf8_lossy(kept).into_owned();
    if total <= MAX_OUTPUT_BYTES {
        return s;
    }
    let mut cut = MAX_OUTPUT_BYTES.min(s.len());
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push_str(&format!("\n… [truncated, {} bytes total]", total));
    s
}

pub struct RunBashTool;

impl ToolHandler for RunBashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "run_bash",
            description: "Execute a bash command. The default working directory is the Universe root. Captures combined stdout+stderr and returns the exit code. REQUIRES USER APPROVAL for every call. Default timeout is 300s (5 min), max 3000s (50 min).\n\nGradio API pattern (Hugging Face Spaces, etc.) — TWO-STEP async, NEVER combine into one command:\n  Step 1 — Submit: POST to the Gradio endpoint with JSON payload. Use timeout_seconds=60.\n           Returns JSON with an event_id.\n  Step 2 — Poll: GET the event_id URL. Use timeout_seconds=600 (generation can take minutes).\n           Streams SSE; extract the result URL from the last `data:` line.\n  Step 3 — Download: curl -L -o to save the artifact. Use timeout_seconds=180.\n  RULES: Always use --connect-timeout 15 --max-time <appropriate> on every curl. Extract event_id between steps — never pipe step 1 into step 2.\n\nUse this tool whenever there's no first-class wrapper for the operation. Prefer dedicated tools (read_file, write_file, list_directory, etc.) when they fit — they don't require approval.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command line to execute. Can include pipes, subshells, and multi-line scripts. Quote carefully — the entire string is passed to `bash -c`."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory, relative to the Universe root. Empty or omitted → Universe root itself. Must resolve inside the Universe sandbox (no `..`)."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Hard kill timeout in seconds. Default 300s (5 min), max 3000s (50 min). Set generously for long-running tasks like mesh generation polling. The process is killed if it exceeds this.",
                        "minimum": 1,
                        "maximum": 3000
                    }
                },
                "required": ["command"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: true,
            stream_topics: &["workshop.tool.run_bash"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) if !c.trim().is_empty() => c.to_string(),
            _ => return err_result("Missing or empty `command` parameter.".into()),
        };
        let cwd_rel = input.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
        let cwd = match resolve_cwd(ctx, cwd_rel) {
            Some(p) => p,
            None => return err_result(format!(
                "`cwd` \"{}\" is outside the Universe sandbox, doesn't exist, or isn't a directory.",
                cwd_rel
            )),
        };
        let timeout = input.get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_S)
            .min(MAX_TIMEOUT_S)
            .max(1);

        // Pick the shell. On Windows, Git Bash / WSL bash is typically
        // on PATH under the dev workflow this codebase targets; we fall
        // through to `sh` on Unix. If neither resolves at spawn time
        // the error bubbles up with a readable message.
        let (shell, flag) = if cfg!(windows) {
            // Try bash first; falls through to cmd only if the process
            // fails to spawn (handled below).
            ("bash", "-c")
        } else {
            ("bash", "-c")
        };

        let start = Instant::now();
        let mut child = match Command::new(shell)
            .arg(flag)
            .arg(&command)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                // Windows fallback: try cmd.exe if bash isn't installed.
                if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound {
                    match Command::new("cmd")
                        .args(["/C", &command])
                        .current_dir(&cwd)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e2) => return err_result(format!(
                            "Failed to spawn bash and cmd fallback also failed: {} / {}",
                            e, e2
                        )),
                    }
                } else {
                    return err_result(format!("Failed to spawn `{}`: {}", shell, e));
                }
            }
        };

        // Each pipe drains on its own thread while the command runs. A
        // command that prints more than the OS pipe holds (4 KB on Windows)
        // waits for a reader, so reading only after it exits would wait out
        // the whole timeout.
        let stdout_pipe = drain(child.stdout.take());
        let stderr_pipe = drain(child.stderr.take());

        // Poll wait with timeout: `wait` offers none, so loop on try_wait
        // and kill the process past the limit.
        let deadline = start + Duration::from_secs(timeout);
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        timed_out = true;
                        break child.wait().ok();
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    return err_result(format!("wait on child failed: {}", e));
                }
            }
        };

        let (stdout, stdout_open) = collect(&stdout_pipe);
        let (stderr, stderr_open) = collect(&stderr_pipe);
        let output_still_open = stdout_open || stderr_open;
        let elapsed_ms = start.elapsed().as_millis();
        let exit_code = status.and_then(|s| s.code());
        let success = !timed_out && status.map(|s| s.success()).unwrap_or(false);

        // Shape the text body so the LLM reads stdout first (usually
        // the useful payload), then stderr, with a clear exit-status
        // line. Empty sections are omitted.
        let mut body = String::new();
        body.push_str(&format!(
            "$ {}\n(cwd: {}, took {} ms)\n",
            command,
            cwd.display(),
            elapsed_ms
        ));
        if timed_out {
            body.push_str(&format!("\nTIMEOUT after {}s — process killed.\n", timeout));
        }
        if output_still_open {
            body.push_str("\nA process the command started is still running and holds its output open; this is what it printed so far.\n");
        }
        if !stdout.is_empty() {
            body.push_str("\n── stdout ──\n");
            body.push_str(&stdout);
            if !stdout.ends_with('\n') { body.push('\n'); }
        }
        if !stderr.is_empty() {
            body.push_str("\n── stderr ──\n");
            body.push_str(&stderr);
            if !stderr.ends_with('\n') { body.push('\n'); }
        }
        body.push_str(&format!(
            "\nexit code: {}",
            exit_code.map(|c| c.to_string()).unwrap_or_else(|| "killed".to_string()),
        ));

        ToolResult {
            tool_name: "run_bash".to_string(),
            tool_use_id: String::new(),
            success,
            content: body,
            structured_data: Some(serde_json::json!({
                "command": command,
                "cwd": cwd.to_string_lossy(),
                "exit_code": exit_code,
                "timed_out": timed_out,
                "output_still_open": output_still_open,
                "elapsed_ms": elapsed_ms,
                "stdout": stdout,
                "stderr": stderr,
            })),
            stream_topic: Some("workshop.tool.run_bash".to_string()),
        }
    }
}
