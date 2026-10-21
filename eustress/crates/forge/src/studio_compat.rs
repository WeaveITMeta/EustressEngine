//! Studio's Forge client, unchanged, talking to a server on this computer.
//!
//! `eustress-forge-sdk` posts `POST /api/auth`, `POST /api/deployments`,
//! `GET /api/deployments/{id}/status`, and `GET /api/experiences/{id}/servers`.
//! forge-orchestration serves `/api/v1/jobs` and does not serve those routes.
//! This module answers the Studio routes and, for each instance, builds the
//! `/api/v1/jobs` job the control plane records.
//!
//! The listener binds to loopback only. A missing `Authorization` header is
//! accepted, because the socket is not reachable from another machine. A
//! wrong bearer is refused. The API key is `FORGE_LOCAL_API_KEY` or
//! `local-dev`.
//!
//! `GET /api/deployments/{id}/status` is the deployment status as a JSON
//! string (`"Active"`), which is how the SDK enum deserializes.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::local_launch::{self, HardwareClass, LaunchRequest, RankServer, SpawnPlan, STAGING_API};

pub const LOCAL_API_KEY: &str = "local-dev";
pub const COMPAT_ADDR: &str = "127.0.0.1:8090";
pub const CONTROL_ADDR: &str = "127.0.0.1:8080";

#[derive(Clone, Debug)]
struct Instance {
    id: Uuid,
    deployment_id: Uuid,
    experience_id: String,
    version: String,
    region_variant: String,
    region_name: String,
    port: u16,
    address: String,
    max_players: u32,
    players: u32,
    started_at: DateTime<Utc>,
    hardware: HardwareClass,
    latency_ms: Option<u32>,
    status_file: String,
    alive: bool,
}

#[derive(Clone, Debug)]
struct Deployment {
    id: Uuid,
    experience_id: String,
    version: String,
    region_variant: String,
    status: &'static str,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    control_plane: String,
}

pub struct CompatState {
    api_key: String,
    token: String,
    deployments: HashMap<Uuid, Deployment>,
    instances: Vec<Instance>,
    ports: Vec<u16>,
    hardware: HardwareClass,
    control_base: String,
    children: Vec<std::process::Child>,
}

impl CompatState {
    pub fn new(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            token: api_key.clone(),
            api_key,
            deployments: HashMap::new(),
            instances: Vec::new(),
            ports: Vec::new(),
            hardware: local_launch::local_hardware(""),
            control_base: format!("http://{CONTROL_ADDR}"),
            children: Vec::new(),
        }
    }

    pub fn from_env() -> Self {
        let key = std::env::var("FORGE_LOCAL_API_KEY").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| LOCAL_API_KEY.to_string());
        let mut state = Self::new(key);
        if let Ok(base) = std::env::var("EUSTRESS_FORGE_CONTROL") {
            if !base.trim().is_empty() {
                state.control_base = base;
            }
        }
        state
    }

    /// Stop every child this process started.
    pub fn stop_children(&mut self) {
        for child in self.children.iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.children.clear();
        for instance in &self.instances {
            if !instance.status_file.is_empty() {
                let path = std::path::Path::new(&instance.status_file);
                let stop = path.with_file_name("dedicated.stop");
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(stop, b"stop\n");
            }
        }
    }
}

pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub plans: Vec<SpawnPlan>,
}

/// Answer one Studio request. `authorization` is the raw header value.
pub fn handle(state: &mut CompatState, method: &str, path: &str, body: &str, authorization: Option<&str>) -> Reply {
    let path = path.split('?').next().unwrap_or(path);
    if method == "POST" && path == "/api/auth" {
        return auth(state, body);
    }
    if let Some(reject) = reject_bearer(state, authorization) {
        return reject;
    }
    match (method, path) {
        ("POST", "/api/deployments") => deploy(state, body),
        ("GET", path) if path.starts_with("/api/deployments/") && path.ends_with("/status") => deployment_status(state, path),
        ("GET", path) if path.starts_with("/api/experiences/") && path.ends_with("/servers") => list_servers(state, path),
        _ => Reply { status: 404, body: json!({ "error": "not found" }), plans: Vec::new() },
    }
}

fn auth(state: &CompatState, body: &str) -> Reply {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let key = parsed.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    if key != state.api_key {
        return Reply { status: 401, body: json!({ "error": "unknown api key" }), plans: Vec::new() };
    }
    Reply { status: 200, body: json!({ "token": state.token }), plans: Vec::new() }
}

fn reject_bearer(state: &CompatState, authorization: Option<&str>) -> Option<Reply> {
    let Some(header) = authorization.map(str::trim).filter(|h| !h.is_empty()) else {
        return None;
    };
    let token = header.strip_prefix("Bearer ").unwrap_or(header).trim();
    if token == state.token || token == state.api_key {
        None
    } else {
        Some(Reply { status: 401, body: json!({ "error": "unauthorized" }), plans: Vec::new() })
    }
}

fn deploy(state: &mut CompatState, body: &str) -> Reply {
    let parsed: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Reply { status: 400, body: json!({ "error": "deployment body is not json" }), plans: Vec::new() },
    };
    let experience_id = parsed.get("experience_id").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if experience_id.is_empty() {
        return Reply { status: 400, body: json!({ "error": "experience_id is required" }), plans: Vec::new() };
    }
    let version = parsed.get("version").and_then(|v| v.as_str()).unwrap_or("latest").to_string();
    let region_variant = parsed
        .get("regions")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("UsEast");
    let (region_variant, region_name) = region_pair(region_variant);
    let min_servers = parsed.get("min_servers").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    let max_players = parsed.get("max_players_per_server").and_then(|v| v.as_u64()).unwrap_or(32) as u32;
    let author_token = parsed
        .pointer("/env/EUSTRESS_AUTHOR_TOKEN")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| parsed.pointer("/env/EUSTRESS_SERVER_TOKEN").and_then(|v| v.as_str()).map(str::to_string));
    let public_host = parsed.pointer("/env/EUSTRESS_HOST_PUBLIC").and_then(|v| v.as_str()).map(str::to_string);
    let api = parsed
        .pointer("/env/EUSTRESS_API_URL")
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("https://") || s.starts_with("http://127.0.0.1") || s.starts_with("http://localhost"))
        .unwrap_or(STAGING_API)
        .to_string();
    let region_name = parsed
        .pointer("/env/EUSTRESS_HOST_REGION")
        .and_then(|v| v.as_str())
        .filter(|s| is_token(s))
        .unwrap_or(region_name)
        .to_string();

    let id = Uuid::new_v4();
    let now = Utc::now();
    let count = min_servers.min(local_launch::MAX_INSTANCES_PER_DEPLOYMENT);
    let plans = local_launch::plan(&LaunchRequest {
        deployment_id: id.to_string(),
        experience_id: experience_id.clone(),
        version: version.clone(),
        region_variant: region_variant.to_string(),
        region_name: region_name.clone(),
        count,
        max_players,
        public_host,
        api,
        author_token,
        ports_in_use: state.ports.clone(),
    });
    for plan in &plans {
        state.ports.push(plan.port);
        let status_file = plan
            .args
            .windows(2)
            .find(|w| w[0] == "--status-file")
            .map(|w| w[1].clone())
            .unwrap_or_default();
        state.instances.push(Instance {
            id: Uuid::parse_str(&plan.instance_id).unwrap_or_else(|_| Uuid::new_v4()),
            deployment_id: id,
            experience_id: experience_id.clone(),
            version: version.clone(),
            region_variant: region_variant.to_string(),
            region_name: region_name.clone(),
            port: plan.port,
            address: format!("0.0.0.0:{}", plan.port),
            max_players,
            players: 0,
            started_at: now,
            hardware: state.hardware.clone(),
            latency_ms: None,
            status_file,
            alive: true,
        });
    }
    let status = if plans.is_empty() { "Pending" } else { "Active" };
    state.deployments.insert(
        id,
        Deployment {
            id,
            experience_id: experience_id.clone(),
            version: version.clone(),
            region_variant: region_variant.to_string(),
            status,
            created_at: now,
            updated_at: now,
            control_plane: "pending".into(),
        },
    );
    let info = deployment_json(state.deployments.get(&id).unwrap(), plans.len() as u32, 0);
    Reply { status: 200, body: info, plans }
}

fn deployment_status(state: &CompatState, path: &str) -> Reply {
    let id = path.trim_start_matches("/api/deployments/").trim_end_matches("/status");
    let Ok(id) = Uuid::parse_str(id) else {
        return Reply { status: 404, body: json!({ "error": "unknown deployment" }), plans: Vec::new() };
    };
    match state.deployments.get(&id) {
        Some(deployment) => Reply { status: 200, body: json!(deployment.status), plans: Vec::new() },
        None => Reply { status: 404, body: json!({ "error": "unknown deployment" }), plans: Vec::new() },
    }
}

fn list_servers(state: &mut CompatState, path: &str) -> Reply {
    let experience = path.trim_start_matches("/api/experiences/").trim_end_matches("/servers");
    refresh_from_status(state);
    let mut rows: Vec<Value> = Vec::new();
    let mut rank: Vec<RankServer> = Vec::new();
    for instance in &state.instances {
        if instance.experience_id != experience || !instance.alive {
            continue;
        }
        rank.push(RankServer {
            id: instance.id.to_string(),
            players: instance.players,
            max_players: instance.max_players,
            tier: instance.hardware.tier,
            latency_ms: instance.latency_ms.filter(|ms| *ms >= 1),
        });
    }
    let order = local_launch::rank_servers(rank);
    for ranked in order {
        let Some(instance) = state.instances.iter().find(|i| i.id.to_string() == ranked.id) else { continue };
        let mut row = json!({
            "id": instance.id,
            "experience_id": instance.experience_id,
            "version": instance.version,
            "region": instance.region_variant,
            "address": instance.address,
            "player_count": instance.players,
            "max_players": instance.max_players,
            "status": "Running",
            "started_at": instance.started_at.to_rfc3339(),
            "cpu_usage": 0.0,
            "memory_mb": 0.0,
            "free_slots": instance.max_players.saturating_sub(instance.players),
            "region_name": instance.region_name,
            "hardware": instance.hardware.json(),
        });
        if let Some(ms) = instance.latency_ms.filter(|ms| *ms >= 1) {
            row["latency_ms"] = json!(ms);
        }
        rows.push(row);
    }
    Reply { status: 200, body: Value::Array(rows), plans: Vec::new() }
}

fn refresh_from_status(state: &mut CompatState) {
    for instance in &mut state.instances {
        let text = std::fs::read_to_string(&instance.status_file).ok();
        let Some(text) = text else { continue };
        let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
        if let Some(players) = value.get("players").and_then(|v| v.as_u64()) {
            instance.players = players as u32;
        }
        if let Some(max) = value.get("max_players").and_then(|v| v.as_u64()).filter(|n| *n > 0) {
            instance.max_players = max as u32;
        }
        if let Some(ms) = value.get("latency_ms").and_then(|v| v.as_u64()).filter(|n| *n >= 1) {
            instance.latency_ms = Some(ms as u32);
        }
        if let Some(gpu) = value.get("gpu").and_then(|v| v.as_str()) {
            let cpu = value.get("cpu").and_then(|v| v.as_str()).unwrap_or("");
            instance.hardware = local_launch::classify(gpu, cpu);
        }
    }
}

fn deployment_json(deployment: &Deployment, server_count: u32, player_count: u32) -> Value {
    json!({
        "id": deployment.id,
        "experience_id": deployment.experience_id,
        "version": deployment.version,
        "status": deployment.status,
        "regions": [deployment.region_variant],
        "server_count": server_count,
        "player_count": player_count,
        "created_at": deployment.created_at.to_rfc3339(),
        "updated_at": deployment.updated_at.to_rfc3339(),
    })
}

fn region_pair(name: &str) -> (&'static str, &'static str) {
    match name {
        "UsWest" => ("UsWest", "us-west-2"),
        "EuWest" => ("EuWest", "eu-west-1"),
        "EuNorth" => ("EuNorth", "eu-north-1"),
        "ApNortheast" => ("ApNortheast", "ap-northeast-1"),
        "ApSoutheast" => ("ApSoutheast", "ap-southeast-1"),
        "SaEast" => ("SaEast", "sa-east-1"),
        _ => ("UsEast", "us-east-1"),
    }
}

fn is_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Record each job on the control plane, then start the process. A control
/// plane that is down does not stop the process: `/api/v1/jobs` only stores
/// the job.
pub fn launch(state: &mut CompatState, plans: &[SpawnPlan]) {
    for plan in plans {
        match local_launch::post_job(&state.control_base, &plan.job) {
            Ok(()) => note_control(state, &plan.deployment_id, "recorded"),
            Err(error) => {
                tracing::warn!("control plane did not record job {}: {error}", plan.instance_id);
                note_control(state, &plan.deployment_id, "unreachable");
            }
        }
        match local_launch::spawn(plan) {
            Ok(child) => state.children.push(child),
            Err(error) => {
                tracing::error!("could not start {}: {error}", plan.instance_id);
                if let Ok(id) = Uuid::parse_str(&plan.deployment_id) {
                    if let Some(deployment) = state.deployments.get_mut(&id) {
                        deployment.status = "Failed";
                        deployment.updated_at = Utc::now();
                    }
                }
                if let Some(instance) = state.instances.iter_mut().find(|i| i.id.to_string() == plan.instance_id) {
                    instance.alive = false;
                }
            }
        }
    }
}

fn note_control(state: &mut CompatState, deployment_id: &str, note: &str) {
    if let Ok(id) = Uuid::parse_str(deployment_id) {
        if let Some(deployment) = state.deployments.get_mut(&id) {
            if deployment.control_plane != "unreachable" || note == "unreachable" {
                deployment.control_plane = note.to_string();
            }
        }
    }
}

/// Bind `addr` only when it is a loopback address.
pub fn loopback(addr: &str) -> Result<SocketAddr, String> {
    let parsed: SocketAddr = addr.parse().map_err(|e| format!("{addr} is not a socket address: {e}"))?;
    if !parsed.ip().is_loopback() {
        return Err(format!("{addr} is not a loopback address. The Studio compatibility server stays on this computer."));
    }
    Ok(parsed)
}

/// Serve until the process is stopped. Each accepted connection is one
/// request. After a deployment is planned, the processes are started.
pub fn serve(addr: &str, state: Arc<Mutex<CompatState>>) -> Result<(), String> {
    let bound = loopback(addr)?;
    let listener = TcpListener::bind(bound).map_err(|e| format!("bind {addr}: {e}"))?;
    eprintln!("Studio Forge compatibility on http://{addr} (key {})", state.lock().api_key);
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let Ok((method, path, body, authorization)) = read_request(&mut stream) else { continue };
        let plans = {
            let mut state = state.lock();
            let reply = handle(&mut state, &method, &path, &body, authorization.as_deref());
            let plans = reply.plans.clone();
            let _ = write_response(&mut stream, reply.status, &reply.body);
            plans
        };
        if !plans.is_empty() {
            let mut state = state.lock();
            launch(&mut state, &plans);
        }
    }
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String, String, Option<String>), String> {
    let mut buf = [0u8; 65536];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf[..n]);
    let (head, body_so_far) = text.split_once("\r\n\r\n").ok_or_else(|| "incomplete request".to_string())?;
    let mut lines = head.lines();
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut authorization = None;
    let mut length = 0usize;
    for line in lines {
        if let Some(value) = line.split_once(':').and_then(|(name, value)| {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
            if name.eq_ignore_ascii_case("authorization") {
                Some(value.trim().to_string())
            } else {
                None
            }
        }) {
            authorization = Some(value);
        }
    }
    let mut body = body_so_far.to_string();
    while body.len() < length && body.len() < 1_000_000 {
        let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        body.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    if body.len() > length {
        body.truncate(length);
    }
    Ok((method, path, body, authorization))
}

fn write_response(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> CompatState {
        CompatState::new(LOCAL_API_KEY)
    }

    #[test]
    fn auth_issues_a_token_and_a_wrong_key_is_refused() {
        let mut state = state();
        let ok = handle(&mut state, "POST", "/api/auth", r#"{"api_key":"local-dev"}"#, None);
        assert_eq!(ok.status, 200);
        assert_eq!(ok.body["token"], "local-dev");
        let bad = handle(&mut state, "POST", "/api/auth", r#"{"api_key":"nope"}"#, None);
        assert_eq!(bad.status, 401);
    }

    #[test]
    fn a_deployment_starts_min_servers_and_status_is_the_json_string() {
        let mut state = state();
        let body = r#"{
            "experience_id":"07d084d8-9098-4192-bb48-84d2c428db36",
            "version":"latest",
            "regions":["UsEast"],
            "min_servers":1,
            "max_servers":5,
            "max_players_per_server":100,
            "env":{"EUSTRESS_AUTHOR_TOKEN":"session-secret","EUSTRESS_HOST_PUBLIC":"203.0.113.7"}
        }"#;
        let reply = handle(&mut state, "POST", "/api/deployments", body, None);
        assert_eq!(reply.status, 200, "{:?}", reply.body);
        assert_eq!(reply.body["status"], "Active");
        assert_eq!(reply.body["server_count"], 1);
        assert_eq!(reply.body["regions"][0], "UsEast");
        assert_eq!(reply.plans.len(), 1);
        assert!(reply.plans[0].args.iter().any(|a| a == "--server"));
        assert!(!local_launch::leaks_secret(&reply.plans[0]));
        let id = reply.body["id"].as_str().unwrap();
        let status = handle(&mut state, "GET", &format!("/api/deployments/{id}/status"), "", Some("Bearer local-dev"));
        assert_eq!(status.status, 200);
        assert_eq!(status.body, json!("Active"));
        let wrong = handle(&mut state, "GET", &format!("/api/deployments/{id}/status"), "", Some("Bearer other"));
        assert_eq!(wrong.status, 401);
        let servers = handle(&mut state, "GET", "/api/experiences/07d084d8-9098-4192-bb48-84d2c428db36/servers", "", None);
        assert_eq!(servers.status, 200);
        let list = servers.body.as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["player_count"], 0);
        assert_eq!(list[0]["max_players"], 100);
        assert_eq!(list[0]["free_slots"], 100);
        assert_eq!(list[0]["region"], "UsEast");
        assert_eq!(list[0]["region_name"], "us-east-1");
        assert!(list[0].get("latency_ms").is_none(), "an unmeasured ping is omitted, not 0");
        assert!(list[0]["hardware"]["tier"].is_string());
        assert_eq!(list[0]["address"], "0.0.0.0:7777");
    }

    #[test]
    fn five_is_the_ceiling_and_only_min_servers_are_planned() {
        let mut state = state();
        let body = r#"{"experience_id":"box","version":"latest","regions":["UsEast"],"min_servers":3,"max_servers":5,"max_players_per_server":8}"#;
        let reply = handle(&mut state, "POST", "/api/deployments", body, None);
        assert_eq!(reply.plans.len(), 3);
        assert_eq!(reply.body["server_count"], 3);
        let ports: Vec<u16> = reply.plans.iter().map(|p| p.port).collect();
        assert_eq!(ports, vec![7777, 7778, 7779]);
    }

    #[test]
    fn the_compatibility_listener_refuses_a_public_address() {
        assert!(loopback("127.0.0.1:8090").is_ok());
        assert!(loopback("0.0.0.0:8090").is_err());
        assert!(loopback("203.0.113.7:8090").is_err());
    }
}
