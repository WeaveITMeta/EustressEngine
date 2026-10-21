//! Local process launcher.
//!
//! forge-orchestration 0.6 records a job at `POST /api/v1/jobs` and, when a
//! Nomad client is configured, submits it there. It does not start a process
//! on this computer. This module does. One job is one `eustress-engine
//! --server` process. Several jobs are several processes, each on its own
//! UDP port, starting at 7777.
//!
//! The server credential and the author's session stay in the process
//! environment. They are not command arguments, and they are not fields of
//! the job posted to the control plane.
//!
//! Quick Join rank, shared with the staging Worker
//! (`infrastructure/cloudflare/api/src/live.mjs`) and the listing page:
//!
//! ```text
//! non-full before full
//! score = 0.5 * fullness + 0.3 * (tier / 3) + 0.2 * ping
//! then more players
//! ```
//!
//! `fullness` is players/max, clamped to 0..=1. `tier` is 0 unknown, 1 low,
//! 2 mid, 3 high. `ping` is `1 - min(latency_ms, 300) / 300` when a latency
//! was measured, and 0 when it was not. A missing measurement is not 0 ms.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use forge_orchestration::{Driver, Job, Task};

/// Staging API a local job heartbeats unless the deployment names another.
pub const STAGING_API: &str = "https://api-staging.eustress.dev";

/// First UDP port. Further instances on this computer take the next free port.
pub const FIRST_PORT: u16 = 7777;

/// How many processes one deployment may start. Studio asks for `min_servers`
/// (1) and names 5 as the scale ceiling. The ceiling is not started.
pub const MAX_INSTANCES_PER_DEPLOYMENT: u32 = 32;

/// Environment keys that must never appear in argv or in the control-plane job.
pub const SECRET_ENV: &[&str] = &["EUSTRESS_SERVER_TOKEN", "EUSTRESS_AUTHOR_TOKEN"];

/// One process the launcher will start.
#[derive(Clone, Debug)]
pub struct SpawnPlan {
    pub deployment_id: String,
    pub instance_id: String,
    pub port: u16,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The job recorded at `/api/v1/jobs`. Its task environment has no token.
    pub job: Job,
}

/// Hardware class reported for this computer. The Worker stores it; it does
/// not invent a GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardwareClass {
    pub gpu: String,
    pub cpu: String,
    pub tier: u8,
}

impl HardwareClass {
    pub fn tier_name(&self) -> &'static str {
        tier_name(self.tier)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "gpu": self.gpu,
            "cpu": self.cpu,
            "tier": self.tier_name(),
        })
    }
}

pub fn tier_name(tier: u8) -> &'static str {
    match tier {
        3 => "high",
        2 => "mid",
        1 => "low",
        _ => "unknown",
    }
}

/// 0 unknown, 1 low, 2 mid, 3 high. An empty string is unknown. A name that
/// matches none of the known parts is low: the machine named a part, and
/// that is not the same as having no information.
pub fn part_tier(text: &str) -> u8 {
    let t = text.to_ascii_lowercase();
    if t.trim().is_empty() {
        return 0;
    }
    const HIGH: &[&str] = &[
        "4090", "4080", "3090", "3080", "a100", "h100", "7900", "6900", "threadripper", "xeon", "epyc", "i9", "ryzen 9", "ryzen9",
    ];
    const MID: &[&str] = &[
        "4070", "4060", "3070", "3060", "2080", "2070", "6800", "6700", "arc ", "i7", "i5", "ryzen 7", "ryzen 5", "ryzen7", "ryzen5",
    ];
    const LOW: &[&str] = &["uhd", "iris", "vega", "celeron", "atom", "i3", "ryzen 3"];
    if HIGH.iter().any(|k| t.contains(k)) {
        return 3;
    }
    if MID.iter().any(|k| t.contains(k)) {
        return 2;
    }
    if LOW.iter().any(|k| t.contains(k)) {
        return 1;
    }
    1
}

pub fn classify(gpu: &str, cpu: &str) -> HardwareClass {
    let gpu_tier = part_tier(gpu);
    let cpu_tier = part_tier(cpu);
    HardwareClass {
        gpu: gpu.trim().to_string(),
        cpu: cpu.trim().to_string(),
        tier: gpu_tier.max(cpu_tier),
    }
}

/// CPU model from `/proc/cpuinfo` when that file exists. Empty otherwise.
pub fn cpu_model() -> String {
    let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") else {
        return String::new();
    };
    text.lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim() == "model name" {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// This computer's class. The GPU line is filled by `gpu_model` when the
/// caller has one; this read does not start a program.
pub fn local_hardware(gpu: &str) -> HardwareClass {
    classify(gpu, &cpu_model())
}

/// A server the rank compares. `latency_ms` is `None` when it was not measured.
#[derive(Clone, Debug)]
pub struct RankServer {
    pub id: String,
    pub players: u32,
    pub max_players: u32,
    pub tier: u8,
    pub latency_ms: Option<u32>,
}

impl RankServer {
    pub fn full(&self) -> bool {
        self.max_players > 0 && self.players >= self.max_players
    }

    pub fn free(&self) -> u32 {
        self.max_players.saturating_sub(self.players)
    }
}

/// Ping term. Missing latency scores 0. It is not treated as 0 ms, which
/// would be the best ping.
pub fn ping_score(latency_ms: Option<u32>) -> f64 {
    match latency_ms {
        Some(ms) if ms >= 1 => 1.0 - (ms.min(300) as f64) / 300.0,
        _ => 0.0,
    }
}

pub fn rank_score(server: &RankServer) -> f64 {
    let max = server.max_players.max(1) as f64;
    let players = (server.players as f64).min(max);
    let fullness = (players / max).clamp(0.0, 1.0);
    let tier = (server.tier.min(3) as f64) / 3.0;
    0.5 * fullness + 0.3 * tier + 0.2 * ping_score(server.latency_ms)
}

/// Busiest non-full first. Full servers last. Ties break toward more players.
pub fn rank_servers(mut servers: Vec<RankServer>) -> Vec<RankServer> {
    servers.sort_by(|a, b| {
        a.full().cmp(&b.full()).then_with(|| {
            rank_score(b)
                .partial_cmp(&rank_score(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        }).then_with(|| b.players.cmp(&a.players))
    });
    servers
}

/// What one deployment asked to start.
#[derive(Clone, Debug)]
pub struct LaunchRequest {
    pub deployment_id: String,
    pub experience_id: String,
    pub version: String,
    pub region_variant: String,
    pub region_name: String,
    pub count: u32,
    pub max_players: u32,
    pub public_host: Option<String>,
    pub api: String,
    /// Author session or an `esrv_` token. Copied into the process environment
    /// only. Never into `args` or `job`.
    pub author_token: Option<String>,
    pub ports_in_use: Vec<u16>,
}

/// `eustress-engine`, a sibling of this binary, or `EUSTRESS_ENGINE_BIN`.
pub fn engine_program() -> String {
    if let Ok(path) = std::env::var("EUSTRESS_ENGINE_BIN") {
        if !path.trim().is_empty() {
            return path;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(engine_file_name());
            if sibling.is_file() {
                return sibling.to_string_lossy().into_owned();
            }
        }
    }
    "eustress-engine".into()
}

fn engine_file_name() -> &'static str {
    if cfg!(windows) {
        "eustress-engine.exe"
    } else {
        "eustress-engine"
    }
}

fn take_port(used: &mut Vec<u16>) -> u16 {
    let mut port = FIRST_PORT;
    while used.contains(&port) {
        port = port.saturating_add(1);
    }
    used.push(port);
    port
}

fn status_dir(experience_id: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    home.join(".eustress_engine").join("dedicated").join(experience_id)
}

/// Plans `count` processes. Does not start them.
pub fn plan(request: &LaunchRequest) -> Vec<SpawnPlan> {
    let count = request.count.min(MAX_INSTANCES_PER_DEPLOYMENT);
    let program = engine_program();
    let mut used = request.ports_in_use.clone();
    let mut plans = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let port = take_port(&mut used);
        let instance_id = uuid::Uuid::new_v4().to_string();
        let status = status_dir(&request.experience_id).join(format!("{instance_id}.json"));
        let mut args = vec![
            "--server".to_string(),
            "--sim".to_string(),
            request.experience_id.clone(),
            "--env".to_string(),
            if request.api.contains("api-staging.eustress.dev") { "staging" } else { "production" }.to_string(),
            "--api-url".to_string(),
            request.api.clone(),
            "--port".to_string(),
            port.to_string(),
            "--status-file".to_string(),
            status.display().to_string(),
        ];
        if let Some(public) = request.public_host.as_ref().filter(|h| !h.trim().is_empty()) {
            args.push("--public".into());
            args.push(public.clone());
        }
        let mut env = vec![
            ("EUSTRESS_SERVER_ID".into(), instance_id.clone()),
            ("EUSTRESS_API_URL".into(), request.api.clone()),
            ("EUSTRESS_HOST_LAN".into(), "1".into()),
            ("EUSTRESS_HOST_PORT".into(), port.to_string()),
            ("EUSTRESS_HOST_ON_PLAY".into(), "1".into()),
            ("EUSTRESS_HOST_REGION".into(), request.region_name.clone()),
        ];
        if let Some(public) = request.public_host.as_ref().filter(|h| !h.trim().is_empty()) {
            env.push(("EUSTRESS_HOST_PUBLIC".into(), public.clone()));
        }
        if let Some(token) = request.author_token.as_ref().filter(|t| !t.is_empty()) {
            if token.starts_with("esrv_") {
                env.push(("EUSTRESS_SERVER_TOKEN".into(), token.clone()));
            } else {
                env.push(("EUSTRESS_AUTHOR_TOKEN".into(), token.clone()));
            }
        }
        let job = job_for(&request.experience_id, &instance_id, &program, &args, &request.region_name);
        plans.push(SpawnPlan {
            deployment_id: request.deployment_id.clone(),
            instance_id,
            port,
            program: program.clone(),
            args,
            env,
            job,
        });
    }
    plans
}

fn job_for(experience_id: &str, instance_id: &str, program: &str, args: &[String], region_name: &str) -> Job {
    let task = Task::new("game")
        .driver(Driver::Exec)
        .command(program)
        .args(args.to_vec())
        .metadata("port", args.iter().skip_while(|a| *a != "--port").nth(1).cloned().unwrap_or_default())
        .metadata("experience_id", experience_id);
    let mut job = Job::new(format!("game-{experience_id}"))
        .with_group("game", task)
        .metadata("experience_id", experience_id)
        .metadata("region", region_name)
        .datacenters(vec![region_name]);
    job.id = instance_id.to_string();
    job
}

/// True when a plan would leak a credential into argv or the posted job.
pub fn leaks_secret(plan: &SpawnPlan) -> bool {
    let job = serde_json::to_string(&plan.job).unwrap_or_default();
    let args = plan.args.join(" ");
    plan.env.iter().any(|(key, value)| {
        if !SECRET_ENV.contains(&key.as_str()) {
            return false;
        }
        args.contains(value) || job.contains(value)
    })
}

/// Start one planned process. The caller owns the child.
pub fn spawn(plan: &SpawnPlan) -> std::io::Result<std::process::Child> {
    if let Some(path) = plan.args.iter().skip_while(|a| *a != "--status-file").nth(1) {
        if let Some(dir) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let mut command = Command::new(&plan.program);
    command.args(&plan.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    command.spawn()
}

/// POST the job to the control plane. Failure is reported and does not stop
/// the local spawn: the control plane's HTTP map does not execute jobs.
pub fn post_job(control_base: &str, job: &Job) -> Result<(), String> {
    let body = serde_json::to_vec(job).map_err(|e| e.to_string())?;
    let url = format!("{}/api/v1/jobs", control_base.trim_end_matches('/'));
    http_post(&url, &body)
}

fn http_post(url: &str, body: &[u8]) -> Result<(), String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("{url} is not an http url"))?;
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{path}");
    let mut stream = std::net::TcpStream::connect_timeout(
        &host_port.parse().map_err(|e| format!("{host_port}: {e}"))?,
        std::time::Duration::from_secs(2),
    )
    .map_err(|e| format!("control plane at {host_port} is unreachable: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
    use std::io::{Read, Write};
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    let status = text.split_whitespace().nth(1).unwrap_or("0");
    if status.starts_with('2') {
        Ok(())
    } else {
        Err(format!("control plane answered {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(count: u32) -> LaunchRequest {
        LaunchRequest {
            deployment_id: "dep".into(),
            experience_id: "07d084d8-9098-4192-bb48-84d2c428db36".into(),
            version: "latest".into(),
            region_variant: "UsEast".into(),
            region_name: "us-east-1".into(),
            count,
            max_players: 100,
            public_host: Some("203.0.113.7".into()),
            api: STAGING_API.into(),
            author_token: Some("author-session-secret".into()),
            ports_in_use: Vec::new(),
        }
    }

    #[test]
    fn three_instances_take_three_ports_and_keep_the_token_out_of_the_job() {
        let plans = plan(&request(3));
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].port, 7777);
        assert_eq!(plans[1].port, 7778);
        assert_eq!(plans[2].port, 7779);
        let mut ids = std::collections::HashSet::new();
        for plan in &plans {
            assert!(ids.insert(plan.instance_id.clone()));
            assert!(plan.args.windows(2).any(|w| w[0] == "--server" || w[0] == "--env" && w[1] == "staging"));
            assert!(plan.args.iter().any(|a| a == "--server"));
            assert!(plan.args.windows(2).any(|w| w[0] == "--env" && w[1] == "staging"));
            assert!(plan.args.windows(2).any(|w| w[0] == "--api-url" && w[1] == STAGING_API));
            assert!(plan.args.windows(2).any(|w| w[0] == "--public" && w[1] == "203.0.113.7"));
            assert_eq!(plan.env.iter().find(|(k, _)| k == "EUSTRESS_SERVER_ID").map(|(_, v)| v.as_str()), Some(plan.instance_id.as_str()));
            let port_text = plan.port.to_string();
            assert_eq!(
                plan.env.iter().find(|(k, _)| k == "EUSTRESS_HOST_PORT").map(|(_, v)| v.as_str()),
                Some(port_text.as_str())
            );
            assert!(plan.env.iter().any(|(k, v)| k == "EUSTRESS_AUTHOR_TOKEN" && v == "author-session-secret"));
            assert!(!plan.env.iter().any(|(k, _)| k == "EUSTRESS_SERVER_TOKEN"));
            assert!(!leaks_secret(plan), "token leaked: {:?}", plan.args);
            assert_eq!(plan.job.id, plan.instance_id);
            let posted = serde_json::to_string(&plan.job).unwrap();
            assert!(!posted.contains("author-session-secret"));
            assert!(!plan.args.iter().any(|a| a.contains("author-session-secret")));
        }
    }

    #[test]
    fn an_esrv_token_is_the_server_credential_and_still_not_an_argument() {
        let mut request = request(1);
        let token = format!("esrv_{}", "ab".repeat(32));
        request.author_token = Some(token.clone());
        let plan = &plan(&request)[0];
        assert!(plan.env.iter().any(|(k, v)| k == "EUSTRESS_SERVER_TOKEN" && v == &token));
        assert!(!plan.env.iter().any(|(k, _)| k == "EUSTRESS_AUTHOR_TOKEN"));
        assert!(!leaks_secret(plan));
    }

    #[test]
    fn ports_already_taken_are_skipped_and_the_first_stays_7777() {
        let mut request = request(2);
        request.ports_in_use = vec![7777];
        let plans = plan(&request);
        assert_eq!(plans[0].port, 7778);
        assert_eq!(plans[1].port, 7779);
        let fresh = plan(&LaunchRequest { ports_in_use: Vec::new(), ..request });
        assert_eq!(fresh[0].port, 7777);
    }

    #[test]
    fn rank_prefers_the_busiest_open_server_and_does_not_treat_a_missing_ping_as_zero() {
        let ranked = rank_servers(vec![
            RankServer { id: "far".into(), players: 8, max_players: 10, tier: 1, latency_ms: Some(200) },
            RankServer { id: "close".into(), players: 1, max_players: 10, tier: 3, latency_ms: Some(20) },
            RankServer { id: "full".into(), players: 10, max_players: 10, tier: 3, latency_ms: Some(5) },
            RankServer { id: "unmeasured".into(), players: 1, max_players: 10, tier: 3, latency_ms: None },
        ]);
        assert_eq!(ranked[0].id, "far");
        assert_eq!(ranked.last().unwrap().id, "full");
        let close = ranked.iter().position(|s| s.id == "close").unwrap();
        let unmeasured = ranked.iter().position(|s| s.id == "unmeasured").unwrap();
        assert!(close < unmeasured, "a measured ping outranks a missing one at the same fullness");
        assert_eq!(ping_score(None), 0.0);
        assert!(ping_score(Some(1)) > ping_score(Some(300)));
        assert_eq!(ping_score(Some(0)), 0.0, "0 ms is not a measurement");
    }

    #[test]
    fn the_local_control_plane_is_built_without_nomad() {
        let forge = crate::ForgeBuilder::new().with_metrics(false).build().expect("forge");
        assert!(!forge.has_nomad());
    }

    #[test]
    fn hardware_tiers_follow_the_named_part() {
        assert_eq!(classify("", "").tier, 0);
        assert_eq!(classify("RTX 4090", "Intel i5").tier, 3);
        assert_eq!(classify("UHD Graphics", "Intel i7").tier, 2);
        assert_eq!(classify("", "AMD EPYC 7763").tier_name(), "high");
        assert_eq!(part_tier("something unnamed"), 1);
    }
}
