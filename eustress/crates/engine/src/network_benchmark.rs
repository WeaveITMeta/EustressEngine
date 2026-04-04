//! # Network Stress Test
//!
//! Spawns synthetic QUIC clients against the local PlayServer to measure
//! throughput, latency, and error rates.  Hardware capabilities are detected
//! via `sysinfo` so the UI can auto-scale defaults.

use bevy::prelude::*;
use bevy::ecs::system::NonSend;
use sysinfo::System;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering}};
use std::time::Instant;

// ── Hardware detection ─────────────────────────────────────────────────────────

/// Detected machine capabilities used to size stress-test defaults.
#[derive(Resource, Debug, Clone)]
pub struct MachineCapabilities {
    pub cpu_cores: usize,
    pub total_ram_mb: u64,
    pub recommended_max_clients: i32,
}

impl Default for MachineCapabilities {
    fn default() -> Self {
        Self { cpu_cores: 4, total_ram_mb: 8192, recommended_max_clients: 50 }
    }
}

impl MachineCapabilities {
    /// Probe the current machine and compute recommended limits.
    pub fn detect() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        let cpu_cores = sys.cpus().len().max(1);
        let total_ram_mb = sys.total_memory() / (1024 * 1024);

        // Each synthetic client uses ~1 tokio task + ~4 KB state.
        // Heuristic: allow up to cores × 25 clients, capped by RAM (1 MB per 4 clients).
        let by_cpu = (cpu_cores * 25) as i32;
        let by_ram = (total_ram_mb / 4) as i32;
        let recommended_max_clients = by_cpu.min(by_ram).min(500).max(10);

        info!(
            "🖥️  Machine capabilities: {} cores, {} MB RAM → max {} stress clients",
            cpu_cores, total_ram_mb, recommended_max_clients
        );

        Self { cpu_cores, total_ram_mb, recommended_max_clients }
    }
}

// ── Stress test runtime state ──────────────────────────────────────────────────

/// Shared counters updated by async client tasks, read by Bevy system each frame.
#[derive(Clone)]
pub struct StressCounters {
    pub total_messages: Arc<AtomicU64>,
    pub total_errors: Arc<AtomicU32>,
    pub latency_sum_us: Arc<AtomicU64>,
    pub latency_count: Arc<AtomicU64>,
    pub running: Arc<AtomicBool>,
    pub clients_connected: Arc<AtomicU32>,
}

impl Default for StressCounters {
    fn default() -> Self {
        Self {
            total_messages: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU32::new(0)),
            latency_sum_us: Arc::new(AtomicU64::new(0)),
            latency_count: Arc::new(AtomicU64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            clients_connected: Arc::new(AtomicU32::new(0)),
        }
    }
}

/// Bevy resource that owns the stress test lifecycle.
#[derive(Resource)]
pub struct StressTestState {
    pub counters: StressCounters,
    pub start_time: Option<Instant>,
    pub duration_secs: u64,
    pub target_clients: i32,
    pub message_rate: i32,
    pub simulate_movement: bool,
    pub simulate_interactions: bool,
    /// Handle to the tokio runtime running the clients.
    pub abort_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Default for StressTestState {
    fn default() -> Self {
        Self {
            counters: StressCounters::default(),
            start_time: None,
            duration_secs: 60,
            target_clients: 1,
            message_rate: 10,
            simulate_movement: true,
            simulate_interactions: false,
            abort_handle: None,
        }
    }
}

// ── Stress test runner ─────────────────────────────────────────────────────────

/// Launch `num_clients` synthetic QUIC connections to localhost:server_port.
/// Each client sends `msg_rate` messages per second for `duration_secs`.
pub fn start_stress_test(
    state: &mut StressTestState,
    server_port: u16,
) {
    // Already running — don't double-start
    if state.counters.running.load(Ordering::Relaxed) { return; }
    let counters = state.counters.clone();
    counters.running.store(true, Ordering::SeqCst);
    counters.total_messages.store(0, Ordering::SeqCst);
    counters.total_errors.store(0, Ordering::SeqCst);
    counters.latency_sum_us.store(0, Ordering::SeqCst);
    counters.latency_count.store(0, Ordering::SeqCst);
    counters.clients_connected.store(0, Ordering::SeqCst);
    state.start_time = Some(Instant::now());

    let num_clients = state.target_clients as usize;
    let msg_rate = state.message_rate as u64;
    let duration = std::time::Duration::from_secs(state.duration_secs);
    let simulate_movement = state.simulate_movement;

    let handle = tokio::spawn(async move {
        // Stagger client connections over 1 second to avoid thundering herd
        let stagger = std::time::Duration::from_millis(
            (1000 / num_clients.max(1) as u64).max(5)
        );

        let mut client_handles = Vec::with_capacity(num_clients);

        for i in 0..num_clients {
            let c = counters.clone();
            let h = tokio::spawn(run_synthetic_client(
                i, server_port, msg_rate, duration, simulate_movement, c,
            ));
            client_handles.push(h);
            tokio::time::sleep(stagger).await;

            if !counters.running.load(Ordering::SeqCst) {
                break; // Early stop requested
            }
        }

        // Wait for all clients to finish
        for h in client_handles {
            let _ = h.await;
        }

        counters.running.store(false, Ordering::SeqCst);
    });

    state.abort_handle = Some(handle);
}

/// A single synthetic client that connects, sends messages, and measures latency.
async fn run_synthetic_client(
    id: usize,
    server_port: u16,
    msg_rate: u64,
    duration: std::time::Duration,
    simulate_movement: bool,
    counters: StressCounters,
) {
    // Build a self-signed QUIC client config (skip server cert verification for localhost)
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap()
    ));

    let mut endpoint = match quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()) {
        Ok(e) => e,
        Err(_) => {
            counters.total_errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    endpoint.set_default_client_config(client_config);

    let addr = format!("127.0.0.1:{}", server_port).parse().unwrap();
    let connection = match endpoint.connect(addr, "localhost") {
        Ok(connecting) => match connecting.await {
            Ok(conn) => conn,
            Err(_) => {
                counters.total_errors.fetch_add(1, Ordering::Relaxed);
                return;
            }
        },
        Err(_) => {
            counters.total_errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    counters.clients_connected.fetch_add(1, Ordering::Relaxed);

    let interval = std::time::Duration::from_millis((1000 / msg_rate.max(1)).max(1));
    let deadline = Instant::now() + duration;

    while Instant::now() < deadline && counters.running.load(Ordering::Relaxed) {
        let send_start = Instant::now();

        // Build a payload — position update if simulating movement, else a ping
        let payload = if simulate_movement {
            let t = send_start.elapsed().as_secs_f32();
            format!(
                r#"{{"type":"position","id":{},"x":{},"y":0,"z":{}}}"#,
                id,
                (t * 0.5 + id as f32).sin() * 50.0,
                (t * 0.3 + id as f32).cos() * 50.0,
            )
        } else {
            format!(r#"{{"type":"ping","id":{}}}"#, id)
        };

        match connection.open_uni().await {
            Ok(mut stream) => {
                let data = payload.as_bytes();
                if stream.write_all(data).await.is_ok() {
                    let _ = stream.finish();
                    let latency_us = send_start.elapsed().as_micros() as u64;
                    counters.total_messages.fetch_add(1, Ordering::Relaxed);
                    counters.latency_sum_us.fetch_add(latency_us, Ordering::Relaxed);
                    counters.latency_count.fetch_add(1, Ordering::Relaxed);
                } else {
                    counters.total_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {
                counters.total_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        tokio::time::sleep(interval).await;
    }

    counters.clients_connected.fetch_sub(1, Ordering::Relaxed);
    connection.close(0u32.into(), b"stress test done");
}

/// Skip TLS verification for localhost stress testing.
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

// ── Bevy plugin ────────────────────────────────────────────────────────────────

pub struct NetworkBenchmarkPlugin;

impl Plugin for NetworkBenchmarkPlugin {
    fn build(&self, app: &mut App) {
        let caps = MachineCapabilities::detect();
        app.insert_resource(caps)
            .init_resource::<StressTestState>()
            .add_systems(Update, sync_stress_test_to_ui);
    }
}

/// Each frame: push stress counters into Slint UI properties.
fn sync_stress_test_to_ui(
    stress: Res<StressTestState>,
    caps: Res<MachineCapabilities>,
    slint: Option<NonSend<crate::ui::slint_ui::SlintUiState>>,
) {
    let Some(slint) = slint else { return };
    let ui = &slint.window;

    // Push hardware-derived max into slider maximum
    let max = caps.recommended_max_clients;
    if ui.get_stress_max_clients() != max {
        ui.set_stress_max_clients(max);
    }

    // Push live counters
    let c = &stress.counters;
    let running = c.running.load(Ordering::Relaxed);
    ui.set_stress_running(running);
    ui.set_stress_total_messages(c.total_messages.load(Ordering::Relaxed) as i32);
    ui.set_stress_errors(c.total_errors.load(Ordering::Relaxed) as i32);

    let count = c.latency_count.load(Ordering::Relaxed);
    let avg = if count > 0 {
        (c.latency_sum_us.load(Ordering::Relaxed) as f64 / count as f64 / 1000.0) as f32
    } else {
        0.0
    };
    ui.set_stress_avg_latency(avg);

    // Progress
    if let Some(start) = stress.start_time {
        let elapsed = start.elapsed().as_secs_f32();
        let total = stress.duration_secs as f32;
        ui.set_stress_progress((elapsed / total).min(1.0));
        let connected = c.clients_connected.load(Ordering::Relaxed);
        ui.set_stress_status(
            format!("{} clients connected • {:.0}s / {:.0}s", connected, elapsed, total).into(),
        );

        // Auto-stop when time elapsed
        if elapsed >= total && running {
            c.running.store(false, Ordering::SeqCst);
        }
    }
}
