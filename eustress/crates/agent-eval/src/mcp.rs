//! A minimal MCP client over stdio: spawn `eustress-mcp`, handshake, list and
//! call tools, each request with its own deadline.
//!
//! The server answers requests concurrently and writes one JSON object per
//! line on stdout. A reader thread forwards each line into a channel, so a
//! request can wait with a deadline and a dead server shows up as a
//! disconnected channel rather than a hang.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The protocol revision the census asks for.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// How to start the server.
#[derive(Debug, Clone)]
pub struct McpLaunch {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
    /// The server's stderr is appended here (panic evidence).
    pub stderr_log: Option<PathBuf>,
}

/// Why a request produced no result.
#[derive(Debug, Clone)]
pub enum McpError {
    Rpc { code: i64, message: String },
    Timeout,
    Dropped(String),
}

/// A running server.
pub struct McpServer {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<String>,
    next_id: u64,
    pub server_info: Value,
}

impl McpServer {
    /// Spawn and complete the `initialize` handshake.
    pub fn spawn(launch: &McpLaunch, handshake_timeout: Duration) -> Result<Self, String> {
        if !launch.bin.is_file() {
            return Err(format!("MCP server binary not found at {}", launch.bin.display()));
        }
        let mut cmd = Command::new(&launch.bin);
        cmd.args(&launch.args).stdin(Stdio::piped()).stdout(Stdio::piped());
        for (k, v) in &launch.envs {
            cmd.env(k, v);
        }
        match &launch.stderr_log {
            Some(p) => {
                let f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .map_err(|e| format!("open {}: {e}", p.display()))?;
                cmd.stderr(Stdio::from(f));
            }
            None => {
                cmd.stderr(Stdio::null());
            }
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn {}: {e}", launch.bin.display()))?;
        let stdout = child.stdout.take().ok_or("MCP server has no stdout pipe")?;
        let stdin = child.stdin.take().ok_or("MCP server has no stdin pipe")?;
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let mut server = McpServer { child, stdin: Some(stdin), rx, next_id: 1, server_info: Value::Null };
        let init = server
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "eustress-agent-census", "version": env!("CARGO_PKG_VERSION") },
                }),
                handshake_timeout,
            )
            .map_err(|e| format!("MCP initialize failed: {e:?}"))?;
        server.server_info = init.get("serverInfo").cloned().unwrap_or(Value::Null);
        server.notify("notifications/initialized", json!({}));
        Ok(server)
    }

    fn write_frame(&mut self, frame: &Value) -> Result<(), String> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err("stdin already closed".into());
        };
        let mut line = serde_json::to_string(frame).map_err(|e| e.to_string())?;
        line.push('\n');
        stdin.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }

    /// Send a notification (no reply).
    pub fn notify(&mut self, method: &str, params: Value) {
        let _ = self.write_frame(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// One request, answered within `timeout` or abandoned (a cancellation is
    /// sent, and the server suppresses the late reply).
    pub fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let frame = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.write_frame(&frame) {
            return Err(McpError::Dropped(format!("write to MCP server failed: {e}")));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                self.cancel(id);
                return Err(McpError::Timeout);
            }
            match self.rx.recv_timeout(deadline - now) {
                Ok(line) => {
                    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                        continue;
                    };
                    if v.get("id").and_then(|i| i.as_u64()) != Some(id) {
                        continue; // a notification, or a late reply to an old id
                    }
                    if let Some(e) = v.get("error") {
                        return Err(McpError::Rpc {
                            code: e.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                            message: e.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string(),
                        });
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.cancel(id);
                    return Err(McpError::Timeout);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(McpError::Dropped("MCP server closed stdout (process exited)".into()));
                }
            }
        }
    }

    fn cancel(&mut self, id: u64) {
        self.notify("notifications/cancelled", json!({ "requestId": id, "reason": "census deadline" }));
    }

    /// Every tool, following `nextCursor` if the server paginates.
    pub fn list_tools(&mut self, timeout: Duration) -> Result<Vec<Value>, String> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..1000 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, timeout).map_err(|e| format!("tools/list failed: {e:?}"))?;
            if let Some(tools) = page.get("tools").and_then(|t| t.as_array()) {
                all.extend(tools.iter().cloned());
            } else {
                return Err(format!("tools/list reply has no `tools` array: {page}"));
            }
            match page.get("nextCursor").and_then(|c| c.as_str()) {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => return Ok(all),
            }
        }
        Err("tools/list paginated past 1000 pages".into())
    }

    /// `tools/call`.
    pub fn call_tool(&mut self, name: &str, arguments: Value, timeout: Duration) -> Result<Value, McpError> {
        self.request("tools/call", json!({ "name": name, "arguments": arguments }), timeout)
    }

    /// True while the process runs.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Close stdin (the server exits on EOF) and wait for it. Only if it is
    /// still running after the grace period is this census's own child
    /// process killed, by its handle.
    pub fn shutdown(mut self, grace: Duration) {
        self.stdin.take();
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
