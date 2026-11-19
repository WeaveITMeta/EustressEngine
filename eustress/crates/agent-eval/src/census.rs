//! The census: list both surfaces, check every descriptor statically, then
//! invoke every tool twice (minimal valid input, then a required parameter
//! removed) against a live engine and classify each outcome.
//!
//! Routing: a tool the MCP server lists is called through the MCP server
//! over stdio, as an IDE calls it. A tool only the engine's Workshop
//! registry offers is called through the Engine Bridge's `tools.call`.
//! `--route engine-first` flips that for names both surfaces offer.
//!
//! Safety: the MCP server resolves the Space its tools act on from the
//! user's Studio settings (`~/.eustress_engine/settings.json`,
//! `last_space_path`) before anything else. The census refuses to call any
//! MCP tool unless that resolves inside the fixture Universe (or is unset),
//! so no probe can touch a real Space. `--swap-studio-last-space` points the
//! setting at the fixture for the run and restores the file byte for byte.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::classify::{classify, Classified, EffectRule, Evidence, Outcome, Route, Transport, HEADLESS_MARKERS, UNDISPATCHABLE_MARKERS};
use crate::digest::{self, DigestConfig};
use crate::inputs::{Overrides, Placeholders, Step, ToolOverride};
use crate::mcp::{McpError, McpLaunch, McpServer};
use crate::schema;
use crate::source_index::SourceIndex;
use crate::util::{excerpt, percentile, rfc3339_utc, round2};

/// The repetition floor the exit criterion requires for the valid input.
pub const REPETITION_FLOOR: usize = 5;

/// Which surface wins for a name both offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePreference {
    McpFirst,
    EngineFirst,
}

/// Everything the census needs.
#[derive(Debug, Clone)]
pub struct CensusConfig {
    pub universe: PathBuf,
    pub repo_root: PathBuf,
    pub out: PathBuf,
    pub mcp_bin: PathBuf,
    /// Start and own the engine (`eustress-headless`) when set; otherwise
    /// attach to one already serving the Universe.
    pub headless_bin: Option<PathBuf>,
    /// In attach mode, the engine's stderr file to watch for panics.
    pub engine_log: Option<PathBuf>,
    pub inputs: PathBuf,
    pub repetitions: usize,
    pub missing_param_repetitions: usize,
    pub timeout: Duration,
    pub settle: Duration,
    pub engine_start_timeout: Duration,
    pub route: RoutePreference,
    pub only: Option<BTreeSet<String>>,
    pub swap_studio_last_space: bool,
}

/// Static-only run: no engine required.
#[derive(Debug, Clone)]
pub struct StaticConfig {
    pub universe: Option<PathBuf>,
    pub repo_root: PathBuf,
    pub out: PathBuf,
    pub mcp_bin: PathBuf,
    pub timeout: Duration,
}

/// Why a run could not produce an artifact.
#[derive(Debug)]
pub enum CensusError {
    Usage(String),
    Setup(String),
    Unsafe(String),
}

/// A finished run.
pub struct CensusReport {
    pub artifact: Value,
    pub complete: bool,
    pub incomplete_reasons: Vec<String>,
}

// ---------------------------------------------------------------------------
// Tool listing
// ---------------------------------------------------------------------------

/// One tool, as the surfaces list it.
#[derive(Debug, Clone, Default)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub in_mcp: bool,
    pub in_engine: bool,
    pub read_only_hint: Option<bool>,
    pub requires_approval: Option<bool>,
}

fn merge_listings(mcp: &[Value], engine: &[Value]) -> Vec<ToolEntry> {
    let mut by_name: BTreeMap<String, ToolEntry> = BTreeMap::new();
    for t in mcp {
        let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
        let e = by_name.entry(name.to_string()).or_default();
        e.name = name.to_string();
        e.in_mcp = true;
        e.description = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
        e.schema = schema::schema_of(t).cloned().unwrap_or(Value::Null);
        let ann = t.get("annotations");
        e.read_only_hint = ann.and_then(|a| a.get("readOnlyHint")).and_then(|v| v.as_bool());
        e.requires_approval = ann.and_then(|a| a.get("destructiveHint")).and_then(|v| v.as_bool());
    }
    for t in engine {
        let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
        let e = by_name.entry(name.to_string()).or_default();
        e.in_engine = true;
        if !e.in_mcp {
            e.name = name.to_string();
            e.description = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
            e.schema = schema::schema_of(t).cloned().unwrap_or(Value::Null);
        }
        if e.requires_approval.is_none() {
            e.requires_approval = t.get("requires_approval").and_then(|v| v.as_bool());
        }
    }
    by_name.into_values().collect()
}

fn engine_listing(universe: &Path, timeout: Duration) -> Result<Vec<Value>, String> {
    let v = eustress_bridge_client::call_engine_with_timeout(universe, "tools.list", json!({}), timeout)?;
    v.get("tools")
        .and_then(|t| t.as_array())
        .cloned()
        .ok_or_else(|| format!("tools.list reply has no `tools` array: {}", excerpt(&v.to_string(), 200)))
}

fn mcp_launch(cfg_bin: &Path, universe: Option<&Path>, stderr_log: Option<PathBuf>) -> McpLaunch {
    let mut args = Vec::new();
    let mut envs = vec![("RUST_LOG".to_string(), "info".to_string())];
    if let Some(u) = universe {
        let u_s = u.to_string_lossy().to_string();
        args.push("--universe".to_string());
        args.push(u_s.clone());
        envs.push(("EUSTRESS_UNIVERSE".into(), u_s));
        if let Some(ws) = u.parent() {
            let ws_s = ws.to_string_lossy().to_string();
            envs.push(("EUSTRESS_UNIVERSES_PATH".into(), ws_s.clone()));
            envs.push(("EUSTRESS_WORKSPACE".into(), ws_s.clone()));
            envs.push(("EUSTRESS_ROOT".into(), ws_s));
        }
    }
    McpLaunch { bin: cfg_bin.to_path_buf(), args, envs, stderr_log }
}

// ---------------------------------------------------------------------------
// Static checks
// ---------------------------------------------------------------------------

fn static_section(mcp: &[Value], engine: Option<&[Value]>, index: &SourceIndex) -> (Value, usize) {
    let mut findings = schema::check_listing("mcp", mcp);
    if let Some(e) = engine {
        findings.extend(schema::check_listing("workshop", e));
    }
    let listed: BTreeSet<String> = mcp
        .iter()
        .chain(engine.unwrap_or(&[]).iter())
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    let mut declared_not_listed = Vec::new();
    for (name, loc) in index.declared_names() {
        if !listed.contains(&name) {
            declared_not_listed.push(json!({ "name": name, "source": loc }));
        }
    }
    // Same name, different schema on the two surfaces.
    let mut schema_mismatch = Vec::new();
    if let Some(e) = engine {
        for t in e {
            let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
            if let Some(m) = mcp.iter().find(|x| x.get("name").and_then(|n| n.as_str()) == Some(name)) {
                if schema::schema_of(m) != schema::schema_of(t) {
                    schema_mismatch.push(Value::String(name.to_string()));
                }
            }
        }
    }
    let errors = findings.iter().filter(|f| f.severity == "error").count();
    let warnings = findings.len() - errors;
    let mut by_code: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &findings {
        *by_code.entry(f.code).or_default() += 1;
    }
    let section = json!({
        "checks": "name is 1-64 of [A-Za-z0-9_-]; no duplicate names per surface; description present (warning under 20 characters); input schema is an object of type \"object\"; properties is an object of objects, each typed (warning otherwise); required is an array of distinct strings, each a declared property; enums non-empty; arrays declare items (warning)",
        "mcp_tools_listed": mcp.len(),
        "workshop_tools_listed": engine.map(|e| Value::from(e.len())).unwrap_or(Value::Null),
        "errors": errors,
        "warnings": warnings,
        "findings_by_code": by_code,
        "findings": findings.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
        "declared_in_source_not_listed": declared_not_listed,
        "schema_differs_between_surfaces": schema_mismatch,
        "tools_without_required_params": mcp.iter().chain(engine.unwrap_or(&[]).iter())
            .filter(|t| schema::schema_of(t).map(|s| schema::required_params(s).is_empty()).unwrap_or(true))
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .collect::<BTreeSet<_>>(),
    });
    (section, errors)
}

/// List and check without calling any tool. Needs the MCP server binary;
/// reads the Workshop listing too when an engine is serving `universe`.
pub fn run_static(cfg: &StaticConfig) -> Result<(Value, usize), CensusError> {
    let launch = mcp_launch(&cfg.mcp_bin, cfg.universe.as_deref(), None);
    let mut server = McpServer::spawn(&launch, cfg.timeout).map_err(CensusError::Setup)?;
    let listed = server.list_tools(cfg.timeout);
    let server_info = server.server_info.clone();
    server.shutdown(Duration::from_secs(5));
    let mcp = listed.map_err(CensusError::Setup)?;
    let engine = cfg.universe.as_ref().and_then(|u| engine_listing(u, cfg.timeout).ok());
    let index = SourceIndex::load(&cfg.repo_root);
    let (section, errors) = static_section(&mcp, engine.as_deref(), &index);
    let names: Vec<Value> = merge_listings(&mcp, engine.as_deref().unwrap_or(&[]))
        .iter()
        .map(|t| {
            let hit = index.find(&t.name);
            json!({
                "name": t.name,
                "mcp": t.in_mcp,
                "workshop": t.in_engine,
                "source": hit.as_ref().map(|h| h.location()),
                "source_match": hit.as_ref().map(|h| h.kind),
                "modes": hit.and_then(|h| h.modes),
            })
        })
        .collect();
    let artifact = json!({
        "schema_version": 1,
        "kind": "tool_surface_static",
        "commit": git_commit(&cfg.repo_root),
        "captured_at": rfc3339_utc(SystemTime::now()),
        "mcp_server": server_info,
        "workshop_listing": if engine.is_some() { "read from a live engine" } else { "not read (no engine serving --universe)" },
        "source_files_scanned": index.file_count(),
        "tools_listed": names.len(),
        "static": section,
        "tools": names,
    });
    write_json(&cfg.out, &artifact).map_err(CensusError::Setup)?;
    Ok((artifact, errors))
}

// ---------------------------------------------------------------------------
// Safety: where the MCP server's tools would act
// ---------------------------------------------------------------------------

fn studio_settings_files() -> Option<(PathBuf, PathBuf)> {
    let engine_dir = eustress_bridge_client::token_dir()?.parent()?.to_path_buf();
    let home = engine_dir.parent()?.to_path_buf();
    Some((engine_dir.join("settings.json"), home.join(".eustress_studio").join("settings.json")))
}

/// The settings file the MCP server reads and the Space it names, exactly
/// as `mcp-server/src/shared_registry.rs::engine_current_space` resolves it.
pub fn mcp_settings_target() -> Option<(PathBuf, PathBuf)> {
    let (current, legacy) = studio_settings_files()?;
    let path = if current.exists() { current } else { legacy };
    let text = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let last = PathBuf::from(v.get("last_space_path")?.as_str()?);
    Some((path, last))
}

fn is_inside(child: &Path, parent: &Path) -> bool {
    match (std::fs::canonicalize(child), std::fs::canonicalize(parent)) {
        (Ok(c), Ok(p)) => c.starts_with(p),
        _ => false,
    }
}

/// Restores the Studio settings file on drop.
struct SettingsSwap {
    path: PathBuf,
    backup: PathBuf,
    original: Vec<u8>,
    restored: bool,
}

impl SettingsSwap {
    fn apply(path: &Path, space: &Path) -> Result<Self, String> {
        let original = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut v: Value = serde_json::from_slice(&original).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let Some(obj) = v.as_object_mut() else {
            return Err(format!("{} is not a JSON object", path.display()));
        };
        obj.insert("last_space_path".into(), Value::String(space.to_string_lossy().to_string()));
        let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".census-backup");
        let backup = path.with_file_name(name);
        std::fs::write(&backup, &original).map_err(|e| format!("write {}: {e}", backup.display()))?;
        let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        Ok(SettingsSwap { path: path.to_path_buf(), backup, original, restored: false })
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        if std::fs::write(&self.path, &self.original).is_ok() {
            let _ = std::fs::remove_file(&self.backup);
            self.restored = true;
        } else {
            eprintln!(
                "census: could not restore {}; the original is at {}",
                self.path.display(),
                self.backup.display()
            );
        }
    }
}

impl Drop for SettingsSwap {
    fn drop(&mut self) {
        self.restore();
    }
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

struct EngineLink {
    universe: PathBuf,
    workspace: PathBuf,
    headless_bin: Option<PathBuf>,
    stderr_log: PathBuf,
    stdout_log: PathBuf,
    child: Option<Child>,
    restarts: u32,
}

impl EngineLink {
    fn managed(&self) -> bool {
        self.headless_bin.is_some()
    }

    fn ping(&self) -> bool {
        eustress_bridge_client::call_engine_with_timeout(&self.universe, "ping", json!({}), Duration::from_secs(5)).is_ok()
    }

    /// Still serving: the process this census started has not exited and a
    /// listener holds the port in the port file. A bare connect, not a ping,
    /// so an engine that is merely busy (a long tool on its main thread)
    /// does not read as dead.
    fn alive(&mut self) -> bool {
        if let Some(c) = self.child.as_mut() {
            if !matches!(c.try_wait(), Ok(None)) {
                return false;
            }
        }
        let port_file = eustress_bridge_client::port_file_path(&self.universe);
        match eustress_bridge_client::read_port_file(&port_file) {
            Some(port) => eustress_bridge_client::port_is_live(port),
            None => false,
        }
    }

    fn start(&mut self, wait: Duration) -> Result<(), String> {
        if let Some(bin) = self.headless_bin.clone() {
            let open = |p: &Path| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .map_err(|e| format!("open {}: {e}", p.display()))
            };
            let err = open(&self.stderr_log)?;
            let out = open(&self.stdout_log)?;
            let ws = self.workspace.to_string_lossy().to_string();
            let child = Command::new(&bin)
                .arg("--universe")
                .arg(&self.universe)
                .arg("--no-autoplay")
                .env("EUSTRESS_WORKSPACE", &ws)
                .env("EUSTRESS_ROOT", &ws)
                .stdin(Stdio::null())
                .stdout(Stdio::from(out))
                .stderr(Stdio::from(err))
                .spawn()
                .map_err(|e| format!("spawn {}: {e}", bin.display()))?;
            self.child = Some(child);
        }
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            if self.ping() {
                return Ok(());
            }
            if let Some(c) = self.child.as_mut() {
                if let Ok(Some(status)) = c.try_wait() {
                    return Err(format!("eustress-headless exited during startup ({status}); see {}", self.stderr_log.display()));
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!(
            "no engine answered at {} within {}s",
            eustress_bridge_client::port_file_path(&self.universe).display(),
            wait.as_secs()
        ))
    }

    /// Ask the engine this census started to exit; only past the grace
    /// period is that one child process killed, by its own handle.
    fn stop(&mut self) {
        let Some(mut child) = self.child.take() else { return };
        let _ = eustress_bridge_client::call_engine_with_timeout(&self.universe, "engine.shutdown", json!({}), Duration::from_secs(5));
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    fn restart(&mut self, wait: Duration) -> Result<(), String> {
        self.stop();
        self.restarts += 1;
        self.start(wait)
    }
}

// ---------------------------------------------------------------------------
// Panic evidence
// ---------------------------------------------------------------------------

struct LogWatch {
    files: Vec<PathBuf>,
    marks: Vec<u64>,
}

impl LogWatch {
    fn mark(&mut self) {
        self.marks = self.files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).collect();
    }

    fn panicked_since_mark(&self) -> bool {
        for (f, off) in self.files.iter().zip(self.marks.iter()) {
            let Ok(mut file) = std::fs::File::open(f) else { continue };
            if file.seek(SeekFrom::Start(*off)).is_err() {
                continue;
            }
            let mut buf = Vec::new();
            if file.read_to_end(&mut buf).is_ok() && String::from_utf8_lossy(&buf).contains("panicked at") {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

fn bridge_err_transport(e: &str) -> Transport {
    if e.contains("did not answer") {
        return Transport::Timeout;
    }
    if let Some(rest) = e.strip_prefix("engine bridge error ") {
        let (code, msg) = rest.split_once(": ").unwrap_or(("0", rest));
        return Transport::RpcError { code: code.trim().parse().unwrap_or(0), message: msg.to_string() };
    }
    if e.contains("malformed JSON") || e.contains("neither result nor error") {
        return Transport::RpcError { code: 0, message: e.to_string() };
    }
    Transport::Dropped(e.to_string())
}

/// What one probe (valid input, or missing parameter) produced.
struct ProbeResult {
    classified: Classified,
    samples_ms: Vec<f64>,
    input: Value,
    unresolved_placeholders: Vec<String>,
    changed_components: Vec<String>,
    digest_settled: bool,
    repetition_transport: BTreeMap<&'static str, usize>,
    repetition_panics: usize,
    step_failures: Vec<String>,
    engine_died: bool,
}

struct Census<'a> {
    cfg: &'a CensusConfig,
    engine: EngineLink,
    mcp: Option<McpServer>,
    mcp_launch: McpLaunch,
    mcp_restarts: u32,
    digest_cfg: DigestConfig,
    volatile: BTreeSet<String>,
    logs: LogWatch,
    placeholders: Placeholders,
    aborted: Option<String>,
    run: String,
}

impl<'a> Census<'a> {
    fn ensure_mcp(&mut self) -> bool {
        let alive = self.mcp.as_mut().map(|m| m.is_alive()).unwrap_or(false);
        if alive {
            return true;
        }
        if let Some(dead) = self.mcp.take() {
            dead.shutdown(Duration::from_millis(200));
            self.mcp_restarts += 1;
        }
        match McpServer::spawn(&self.mcp_launch, self.cfg.timeout) {
            Ok(s) => {
                self.mcp = Some(s);
                true
            }
            Err(_) => false,
        }
    }

    fn invoke(&mut self, route: Route, name: &str, input: &Value) -> (Transport, f64) {
        match route {
            Route::Mcp => {
                if !self.ensure_mcp() {
                    return (Transport::NotDispatched("the MCP server could not be started".into()), 0.0);
                }
                let timeout = self.cfg.timeout;
                let server = self.mcp.as_mut().expect("ensured above");
                let t0 = Instant::now();
                let r = server.call_tool(name, input.clone(), timeout);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let t = match r {
                    Ok(v) => Transport::Reply(v),
                    Err(McpError::Rpc { code, message }) => Transport::RpcError { code, message },
                    Err(McpError::Timeout) => Transport::Timeout,
                    Err(McpError::Dropped(d)) => Transport::Dropped(d),
                };
                if matches!(t, Transport::Dropped(_)) {
                    if let Some(dead) = self.mcp.take() {
                        dead.shutdown(Duration::from_millis(200));
                        self.mcp_restarts += 1;
                    }
                }
                (t, ms)
            }
            Route::EngineBridge => {
                let params = json!({ "name": name, "input": input, "tool_use_id": format!("census_{}_{}", self.run, name) });
                let t0 = Instant::now();
                let r = eustress_bridge_client::call_engine_with_timeout(&self.engine.universe, "tools.call", params, self.cfg.timeout);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let t = match r {
                    Ok(v) => Transport::Reply(v),
                    Err(e) => bridge_err_transport(&e),
                };
                (t, ms)
            }
        }
    }

    fn run_steps(&mut self, steps: &[Step], probe: &str, rep: usize, failures: &mut Vec<String>) {
        for s in steps {
            match s {
                Step::Bridge { method, params } => {
                    let (p, _) = self.placeholders.fill(params, probe, rep);
                    if let Err(e) = eustress_bridge_client::call_engine_with_timeout(&self.engine.universe, method, p, self.cfg.timeout) {
                        failures.push(excerpt(&format!("bridge {method}: {e}"), 200));
                    }
                }
                Step::McpTool { name, arguments } => {
                    let (a, _) = self.placeholders.fill(arguments, probe, rep);
                    if !self.ensure_mcp() {
                        failures.push(format!("mcp {name}: server not running"));
                        continue;
                    }
                    let timeout = self.cfg.timeout;
                    let server = self.mcp.as_mut().expect("ensured above");
                    match server.call_tool(name, a, timeout) {
                        Ok(v) if v.get("isError").and_then(|b| b.as_bool()) == Some(true) => {
                            failures.push(excerpt(&format!("mcp {name}: {v}"), 200))
                        }
                        Ok(_) => {}
                        Err(e) => failures.push(excerpt(&format!("mcp {name}: {e:?}"), 200)),
                    }
                }
            }
        }
    }

    /// After every probe: leave Play so the next tool starts in Edit.
    fn restore_edit_state(&mut self) {
        let _ = eustress_bridge_client::call_engine_with_timeout(&self.engine.universe, "sim.stop", json!({}), Duration::from_secs(10));
    }

    fn settle(&self) -> (digest::Digest, bool) {
        digest::settle(&self.digest_cfg, &self.volatile, Duration::from_millis(150), self.cfg.settle)
    }

    /// Recover the engine after it stopped answering. Returns false (and
    /// marks the run aborted) when that is impossible.
    fn recover_engine(&mut self) -> bool {
        if !self.engine.managed() {
            self.aborted = Some("the attached engine stopped answering; start the census with --headless-bin to let it restart the engine".into());
            return false;
        }
        match self.engine.restart(self.cfg.engine_start_timeout) {
            Ok(()) => true,
            Err(e) => {
                self.aborted = Some(format!("engine restart failed: {e}"));
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn probe(
        &mut self,
        name: &str,
        route: Route,
        template: &Value,
        rule: &EffectRule,
        ov: &ToolOverride,
        reps: usize,
        label: &str,
    ) -> ProbeResult {
        let mut res = ProbeResult {
            classified: classify(route, &Transport::NotDispatched("not run".into()), rule, &Evidence::default()),
            samples_ms: Vec::new(),
            input: Value::Null,
            unresolved_placeholders: Vec::new(),
            changed_components: Vec::new(),
            digest_settled: true,
            repetition_transport: BTreeMap::new(),
            repetition_panics: 0,
            step_failures: Vec::new(),
            engine_died: false,
        };
        for rep in 1..=reps.max(1) {
            let mut failures = Vec::new();
            self.run_steps(&ov.setup, label, rep, &mut failures);
            let (input, missing) = self.placeholders.fill(template, label, rep);
            let (before, settled_before) = if rep == 1 { self.settle() } else { (digest::Digest::new(), true) };
            self.logs.mark();
            let (transport, ms) = self.invoke(route, name, &input);
            let engine_alive = self.engine.alive();
            let transport = if engine_alive {
                transport
            } else {
                Transport::Dropped("the engine exited or stopped listening during the call".into())
            };
            let (after, settled_after) = if rep == 1 && engine_alive { self.settle() } else { (digest::Digest::new(), true) };
            let panic_logged = self.logs.panicked_since_mark();
            *res.repetition_transport.entry(transport.kind()).or_default() += 1;
            if panic_logged {
                res.repetition_panics += 1;
            }
            res.samples_ms.push(ms);
            if rep == 1 {
                let changed = if engine_alive { digest::changed(&before, &after, &self.volatile) } else { Vec::new() };
                let ev = Evidence { panic_logged, digest_changed: if engine_alive { Some(!changed.is_empty()) } else { None } };
                res.classified = classify(route, &transport, rule, &ev);
                res.input = input;
                res.unresolved_placeholders = missing;
                res.changed_components = changed.into_iter().take(12).collect();
                res.digest_settled = settled_before && settled_after;
            }
            if !engine_alive {
                res.engine_died = true;
                res.step_failures.extend(failures);
                if !self.recover_engine() {
                    break;
                }
                continue;
            }
            self.run_steps(&ov.teardown, label, rep, &mut failures);
            self.restore_edit_state();
            res.step_failures.extend(failures);
        }
        res
    }
}

fn probe_json(p: &ProbeResult, rule: &EffectRule, extra: Value) -> Value {
    let mut sorted = p.samples_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut o = json!({
        "outcome": p.classified.outcome.as_str(),
        "basis": p.classified.basis,
        "detail": p.classified.detail,
        "effect_rule": rule.as_str(),
        "effect_observed": p.classified.effect_observed,
        "payload_nonempty": p.classified.payload_nonempty,
        "latency_p50_ms": percentile(&sorted, 50.0).map(round2),
        "latency_p95_ms": percentile(&sorted, 95.0).map(round2),
        "latency_samples_ms": p.samples_ms.iter().map(|v| round2(*v)).collect::<Vec<_>>(),
        "latency_censored": p.classified.outcome == Outcome::Timeout,
        "repetitions": p.samples_ms.len(),
        "repetition_transport": p.repetition_transport,
        "repetition_panics": p.repetition_panics,
        "digest_changed_components": p.changed_components,
        "digest_settled": p.digest_settled,
        "input": p.input,
        "unresolved_placeholders": p.unresolved_placeholders,
        "step_failures": p.step_failures,
        "engine_died": p.engine_died,
    });
    if p.classified.outcome == Outcome::StructuredError {
        o["error_excerpt"] = json!(p.classified.detail);
    }
    if p.classified.outcome == Outcome::SilentNoop {
        o["silent_noop_kind"] = json!(if p.classified.basis == "malformed_result" {
            "malformed_result"
        } else if p.classified.payload_nonempty {
            "no_effect_payload_returned"
        } else {
            "no_effect_empty_payload"
        });
    }
    if let (Value::Object(dst), Value::Object(src)) = (&mut o, extra) {
        for (k, v) in src {
            dst.insert(k, v);
        }
    }
    o
}

fn find_uuids(v: &Value, out: &mut BTreeMap<String, String>) {
    match v {
        Value::Object(m) => {
            let name = m.get("name").and_then(|n| n.as_str());
            let id = m.get("uuid").or_else(|| m.get("id")).and_then(|i| i.as_str());
            if let (Some(n), Some(i)) = (name, id) {
                out.entry(n.to_string()).or_insert_with(|| i.to_string());
            }
            for sub in m.values() {
                find_uuids(sub, out);
            }
        }
        Value::Array(a) => {
            for sub in a {
                find_uuids(sub, out);
            }
        }
        _ => {}
    }
}

fn first_space(universe: &Path) -> Option<PathBuf> {
    let mut spaces: Vec<PathBuf> = std::fs::read_dir(universe.join("Spaces"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    spaces.sort();
    spaces.into_iter().next()
}

fn run_id() -> String {
    let nanos = SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:06x}", (nanos / 1000) as u64 & 0xff_ffff)
}

/// The census proper.
pub fn run_census(cfg: &CensusConfig) -> Result<CensusReport, CensusError> {
    let started = SystemTime::now();
    let universe = std::fs::canonicalize(&cfg.universe)
        .map_err(|e| CensusError::Usage(format!("--universe {}: {e}", cfg.universe.display())))?;
    if !universe.join("Spaces").is_dir() {
        return Err(CensusError::Usage(format!("{} is not a Universe (no Spaces/ folder)", universe.display())));
    }
    let workspace = universe.parent().map(Path::to_path_buf).unwrap_or_else(|| universe.clone());
    let space = first_space(&universe).ok_or_else(|| CensusError::Usage(format!("{} holds no Space", universe.display())))?;
    let space2 = std::fs::read_dir(universe.join("Spaces"))
        .ok()
        .and_then(|it| {
            let mut v: Vec<PathBuf> = it.flatten().map(|e| e.path()).filter(|p| p.is_dir() && *p != space).collect();
            v.sort();
            v.into_iter().next()
        });
    let universe_b = workspace.join("CensusUniverseB");

    // ── Safety gate ──────────────────────────────────────────────────
    let mut swap: Option<SettingsSwap> = None;
    let mut safety = json!({ "rule": "MCP tools act on the Space named by last_space_path in the Studio settings when it is a folder; every MCP call is refused unless that is unset or inside the fixture Universe" });
    if let Some((settings, target)) = mcp_settings_target() {
        safety["settings_file"] = json!(settings.to_string_lossy());
        safety["last_space_path"] = json!(target.to_string_lossy());
        if target.is_dir() && !is_inside(&target, &universe) {
            if cfg.swap_studio_last_space {
                swap = Some(SettingsSwap::apply(&settings, &space).map_err(CensusError::Setup)?);
                safety["action"] = json!("last_space_path pointed at the fixture Space for the run; the file is restored byte for byte at exit");
            } else {
                return Err(CensusError::Unsafe(format!(
                    "the MCP server would act on {} (last_space_path in {}), outside the fixture Universe {}. \
                     Close Studio and rerun with --swap-studio-last-space (the file is restored at exit, a backup is kept beside it until then), \
                     or run under an account whose Studio settings do not name a Space.",
                    target.display(),
                    settings.display(),
                    universe.display()
                )));
            }
        } else {
            safety["action"] = json!("none needed: the setting is unset, not a folder, or inside the fixture Universe");
        }
    } else {
        safety["action"] = json!("none needed: no Studio settings file");
    }

    let overrides = Overrides::load(&cfg.inputs).map_err(CensusError::Usage)?;
    let out_dir = cfg.out.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&out_dir).map_err(|e| CensusError::Setup(format!("create {}: {e}", out_dir.display())))?;
    let engine_stderr = out_dir.join("census_stderr.log");
    let engine_stdout = out_dir.join("census_engine_stdout.log");
    let mcp_stderr = out_dir.join("mcp_stderr.log");
    for f in [&engine_stdout, &mcp_stderr] {
        let _ = std::fs::write(f, b"");
    }
    if cfg.headless_bin.is_some() {
        let _ = std::fs::write(&engine_stderr, b"");
    }

    // ── The engine ───────────────────────────────────────────────────
    let mut engine = EngineLink {
        universe: universe.clone(),
        workspace: workspace.clone(),
        headless_bin: cfg.headless_bin.clone(),
        stderr_log: engine_stderr.clone(),
        stdout_log: engine_stdout.clone(),
        child: None,
        restarts: 0,
    };
    if engine.managed() && engine.ping() {
        return Err(CensusError::Setup(format!(
            "an engine already answers for {}; stop it, or omit --headless-bin to attach to it",
            universe.display()
        )));
    }
    if let Err(e) = engine.start(cfg.engine_start_timeout) {
        engine.stop();
        return Err(CensusError::Setup(e));
    }

    let mut watched = vec![mcp_stderr.clone()];
    if engine.managed() {
        watched.push(engine_stderr.clone());
    } else if let Some(l) = &cfg.engine_log {
        watched.push(l.clone());
    }

    // ── Listings ─────────────────────────────────────────────────────
    let launch = mcp_launch(&cfg.mcp_bin, Some(&universe), Some(mcp_stderr.clone()));
    let mut server = match McpServer::spawn(&launch, cfg.timeout) {
        Ok(s) => s,
        Err(e) => {
            engine.stop();
            return Err(CensusError::Setup(e));
        }
    };
    let mcp_tools = match server.list_tools(cfg.timeout) {
        Ok(t) => t,
        Err(e) => {
            server.shutdown(Duration::from_secs(5));
            engine.stop();
            return Err(CensusError::Setup(e));
        }
    };
    let server_info = server.server_info.clone();
    let engine_tools = match engine_listing(&universe, cfg.timeout) {
        Ok(t) => t,
        Err(e) => {
            server.shutdown(Duration::from_secs(5));
            engine.stop();
            return Err(CensusError::Setup(format!("engine tools.list: {e}")));
        }
    };
    let tools = merge_listings(&mcp_tools, &engine_tools);
    let index = SourceIndex::load(&cfg.repo_root);
    let (static_json, _) = static_section(&mcp_tools, Some(engine_tools.as_slice()), &index);

    // ── Fixture facts and digest calibration ─────────────────────────
    let mut placeholders = Placeholders::default();
    let run = run_id();
    for (k, v) in [
        ("universe", universe.to_string_lossy().to_string()),
        ("universe_b", universe_b.to_string_lossy().to_string()),
        ("workspace", workspace.to_string_lossy().to_string()),
        ("space", space.to_string_lossy().to_string()),
        ("space_name", space.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        ("space2_name", space2.as_ref().and_then(|s| s.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        ("run", run.clone()),
    ] {
        placeholders.vars.insert(k.to_string(), v);
    }
    if let Ok(q) = eustress_bridge_client::call_engine_with_timeout(&universe, "ecs.query", json!({ "limit": 100000 }), cfg.timeout) {
        find_uuids(&q, &mut placeholders.uuids);
    }
    let mut own_files = BTreeSet::new();
    for f in [&engine_stderr, &engine_stdout, &mcp_stderr, &cfg.out] {
        if let (Ok(c), Ok(w)) = (std::fs::canonicalize(f.parent().unwrap_or(f)), std::fs::canonicalize(&workspace)) {
            if let Ok(rel) = c.join(f.file_name().unwrap_or_default()).strip_prefix(&w) {
                own_files.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let digest_cfg = DigestConfig {
        universe: universe.clone(),
        fs_root: workspace.clone(),
        read_timeout: cfg.timeout.min(Duration::from_secs(10)),
        own_files,
    };
    let volatile = digest::calibrate(&digest_cfg, 4, Duration::from_millis(750));

    let mut census = Census {
        cfg,
        engine,
        mcp: Some(server),
        mcp_launch: launch,
        mcp_restarts: 0,
        digest_cfg,
        volatile,
        logs: LogWatch { files: watched, marks: Vec::new() },
        placeholders,
        aborted: None,
        run,
    };
    census.restore_edit_state();

    // ── Probes ───────────────────────────────────────────────────────
    let mut records = Vec::new();
    let mut totals_valid: BTreeMap<&'static str, usize> = Outcome::ALL.iter().map(|o| (o.as_str(), 0)).collect();
    let mut totals_missing = totals_valid.clone();
    let mut exposed = 0usize;
    for tool in &tools {
        let hit = index.find(&tool.name);
        let modes = hit.as_ref().and_then(|h| h.modes.clone());
        let (exposed_default, exposure_basis) = if tool.in_mcp {
            (true, "listed by the MCP server, which applies no mode filter")
        } else {
            match &modes {
                Some(m) if m.iter().any(|x| x == "General") => (true, "Workshop only; its modes include General, the default mode set"),
                Some(_) => (false, "Workshop only; its modes exclude General, so it is hidden until another mode is active"),
                None => (false, "Workshop only; modes not found in source, counted as not exposed"),
            }
        };
        if exposed_default {
            exposed += 1;
        }
        let selected = cfg.only.as_ref().map(|o| o.contains(&tool.name)).unwrap_or(true);
        if census.aborted.is_some() || !selected {
            continue;
        }
        let route = match (tool.in_mcp, tool.in_engine, cfg.route) {
            (true, true, RoutePreference::EngineFirst) => Route::EngineBridge,
            (true, _, _) => Route::Mcp,
            _ => Route::EngineBridge,
        };
        let ov = overrides.get(&tool.name).cloned().unwrap_or_default();
        let (valid_template, input_source) = match &ov.valid_input {
            Some(v) => (v.clone(), "override"),
            None => (schema::synthesize_minimal(&tool.schema), "synthesized"),
        };
        let required = schema::required_params(&tool.schema);
        let removed = ov.missing_param.clone().or_else(|| required.first().cloned());
        let mut probe_note: Option<String> = None;
        let missing_template = match (&removed, &valid_template) {
            (Some(r), Value::Object(m)) => {
                let mut m = m.clone();
                if m.remove(r).is_none() {
                    probe_note = Some(format!("`{r}` was not in the valid input; the probe sends the valid input unchanged"));
                }
                if !required.contains(r) {
                    probe_note = Some(format!("`{r}` is not declared required; removed by override"));
                }
                Value::Object(m)
            }
            (Some(_), _) => json!({}),
            (None, _) => {
                probe_note = Some("the schema declares no required parameter; the probe sends {}".into());
                json!({})
            }
        };
        let (rule, effect_why) = match &ov.effect {
            Some(c) => (c.rule.clone(), Some(c.why.clone())),
            None if tool.read_only_hint == Some(true) => (EffectRule::Payload, None),
            None => (EffectRule::Digest, None),
        };

        let valid = census.probe(&tool.name, route, &valid_template, &rule, &ov, cfg.repetitions, "v");
        let missing = census.probe(&tool.name, route, &missing_template, &rule, &ov, cfg.missing_param_repetitions, "m");
        *totals_valid.entry(valid.classified.outcome.as_str()).or_default() += 1;
        *totals_missing.entry(missing.classified.outcome.as_str()).or_default() += 1;

        records.push(json!({
            "name": tool.name,
            "source": hit.as_ref().map(|h| h.location()),
            "source_match": hit.as_ref().map(|h| h.kind),
            "surfaces": { "mcp": tool.in_mcp, "workshop": tool.in_engine },
            "route": route.as_str(),
            "exposed_default_modes": exposed_default,
            "exposure_basis": exposure_basis,
            "modes": modes,
            "requires_approval": tool.requires_approval,
            "read_only_hint": tool.read_only_hint,
            "input_source": input_source,
            "effect_why": effect_why,
            "override_note": ov.note,
            "valid_input": probe_json(&valid, &rule, json!({})),
            "missing_param": probe_json(&missing, &rule, json!({ "removed_param": removed, "probe_note": probe_note })),
        }));
    }

    // ── Teardown ─────────────────────────────────────────────────────
    if let Some(s) = census.mcp.take() {
        s.shutdown(Duration::from_secs(5));
    }
    census.engine.stop();
    if let Some(mut s) = swap.take() {
        s.restore();
    }
    if !census.engine.managed() {
        match &cfg.engine_log {
            Some(l) => {
                let _ = std::fs::copy(l, &engine_stderr);
            }
            None => {
                let _ = std::fs::write(
                    &engine_stderr,
                    b"engine stderr not captured: the census attached to an engine it did not start and no --engine-log was given\n",
                );
            }
        }
    }

    // ── Verdict ──────────────────────────────────────────────────────
    let defined = tools.len();
    let invoked = records.len();
    let mut totals: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (k, v) in totals_valid.iter().chain(totals_missing.iter()) {
        *totals.entry(*k).or_default() += v;
    }
    let total_sum: usize = totals.values().sum();
    let mut reasons = Vec::new();
    if let Some(a) = &census.aborted {
        reasons.push(format!("aborted: {a}"));
    }
    if invoked != defined {
        reasons.push(format!("{invoked} of {defined} defined descriptors invoked"));
    }
    if cfg.repetitions < REPETITION_FLOOR {
        reasons.push(format!("--repetitions {} is under the floor of {REPETITION_FLOOR}", cfg.repetitions));
    }
    if total_sum != 2 * defined {
        reasons.push(format!("outcome totals sum to {total_sum}, not {}", 2 * defined));
    }
    let missing_latency = records
        .iter()
        .filter(|r| !r["valid_input"]["latency_p50_ms"].is_number() || !r["valid_input"]["latency_p95_ms"].is_number())
        .count();
    if missing_latency > 0 {
        reasons.push(format!("{missing_latency} valid_input probes lack a numeric latency"));
    }
    if defined == 0 {
        reasons.push("no tool was listed".into());
    }
    let complete = reasons.is_empty();

    let artifact = json!({
        "schema_version": 1,
        "kind": "tool_conformance",
        "commit": git_commit(&cfg.repo_root),
        "captured_at": rfc3339_utc(started),
        "finished_at": rfc3339_utc(SystemTime::now()),
        "host": host_info(),
        "complete": complete,
        "incomplete_reasons": reasons,
        "tools_defined": defined,
        "tools_defined_mcp": tools.iter().filter(|t| t.in_mcp).count(),
        "tools_defined_workshop": tools.iter().filter(|t| t.in_engine).count(),
        "tools_defined_workshop_only": tools.iter().filter(|t| t.in_engine && !t.in_mcp).count(),
        "tools_exposed_default_modes": exposed,
        "exposure_definition": "exposed when the MCP server lists it (the MCP server applies no mode filter), or, for a Workshop-only tool, when its declared modes include General (the Workshop's default mode set)",
        "tools_invoked": invoked,
        "unclassified": defined - invoked,
        "outcome_totals": totals,
        "outcome_totals_valid_input": totals_valid,
        "outcome_totals_missing_param": totals_missing,
        "digest_method": digest::method_statement(),
        "digest_volatile_components": census.volatile.iter().cloned().collect::<Vec<_>>(),
        "classification": {
            "classes": Outcome::ALL.iter().map(|o| o.as_str()).collect::<Vec<_>>(),
            "classified_repetition": "the first repetition of each probe is classified, with digests settled before and after it; later repetitions are timed and their transport results counted",
            "undispatchable_markers": UNDISPATCHABLE_MARKERS,
            "headless_markers": HEADLESS_MARKERS,
            "panic_evidence": "a 'panicked at' line appended to census_stderr.log (engine) or mcp_stderr.log during the call, or the peer dropping the connection",
        },
        "repetitions": cfg.repetitions,
        "missing_param_repetitions": cfg.missing_param_repetitions,
        "timeout_s": cfg.timeout.as_secs_f64(),
        "route_preference": match cfg.route { RoutePreference::McpFirst => "mcp-first", RoutePreference::EngineFirst => "engine-first" },
        "fixture": {
            "universe": universe.to_string_lossy(),
            "space": space.to_string_lossy(),
            "workspace": workspace.to_string_lossy(),
            "uuids_found": census.placeholders.uuids.len(),
        },
        "inputs": overrides.source,
        "safety": safety,
        "engine": {
            "mode": if census.engine.managed() { "managed (started by the census)" } else { "attached" },
            "restarts": census.engine.restarts,
            "stderr_log": "census_stderr.log",
        },
        "mcp_server": server_info,
        "mcp_restarts": census.mcp_restarts,
        "static": static_json,
        "tools": records,
    });
    write_json(&cfg.out, &artifact).map_err(CensusError::Setup)?;
    Ok(CensusReport { artifact, complete, incomplete_reasons: reasons })
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

fn git_commit(repo: &Path) -> Value {
    let run = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let sha = run(&["rev-parse", "--short=8", "HEAD"]);
    let dirty = run(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty());
    match sha {
        Some(s) => json!({ "sha": s, "dirty_tracked_files": dirty }),
        None => json!({ "sha": "unknown", "dirty_tracked_files": Value::Null }),
    }
}

fn host_info() -> Value {
    let os_version = if cfg!(windows) {
        Command::new("cmd").args(["/C", "ver"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Command::new("uname").arg("-sr").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let cpu = std::env::var("PROCESSOR_IDENTIFIER").ok().or_else(|| {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()?
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
    });
    json!({
        "os": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "os_version": os_version,
        "cpu": cpu,
        "logical_cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
    })
}

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("create {}: {e}", p.display()))?;
    }
    let mut text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_merge_by_name_and_keep_mcp_hints() {
        let mcp = vec![json!({ "name": "read_file", "description": "d", "inputSchema": { "type": "object" },
            "annotations": { "readOnlyHint": true, "destructiveHint": false } })];
        let eng = vec![
            json!({ "name": "read_file", "description": "d", "input_schema": { "type": "object" }, "requires_approval": false }),
            json!({ "name": "estimate_tax", "description": "e", "input_schema": { "type": "object" }, "requires_approval": false }),
        ];
        let m = merge_listings(&mcp, &eng);
        assert_eq!(m.len(), 2);
        let rf = m.iter().find(|t| t.name == "read_file").unwrap();
        assert!(rf.in_mcp && rf.in_engine);
        assert_eq!(rf.read_only_hint, Some(true));
        let et = m.iter().find(|t| t.name == "estimate_tax").unwrap();
        assert!(!et.in_mcp && et.in_engine);
        assert_eq!(et.read_only_hint, None);
    }

    #[test]
    fn bridge_client_errors_map_to_transports() {
        assert!(matches!(bridge_err_transport("Eustress engine accepted the connection but did not answer 'x'"), Transport::Timeout));
        match bridge_err_transport("engine bridge error -32602: missing 'name' (tool id)") {
            Transport::RpcError { code, message } => {
                assert_eq!(code, -32602);
                assert_eq!(message, "missing 'name' (tool id)");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(bridge_err_transport("Eustress engine is not running ..."), Transport::Dropped(_)));
    }

    #[test]
    fn uuids_are_read_from_any_depth() {
        let mut out = BTreeMap::new();
        find_uuids(&json!({ "entities": [{ "name": "WelcomeCube", "uuid": "u1" }, { "name": "Baseplate", "id": "u2" }] }), &mut out);
        assert_eq!(out.get("WelcomeCube").map(String::as_str), Some("u1"));
        assert_eq!(out.get("Baseplate").map(String::as_str), Some("u2"));
    }
}
