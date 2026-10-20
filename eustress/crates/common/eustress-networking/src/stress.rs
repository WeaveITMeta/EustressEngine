//! Headless players for a live host.
//!
//! Each bot joins the way the Player does: WebTransport, `Hello`, `Welcome`,
//! the world chunks, `WorldReady`, then avatar datagrams and input frames.
//! The types are this crate's. Nothing here uses Bevy, so Studio and
//! `eustress-stress` run the same code.
//!
//! The host has to be one that serves that session. Studio (F9) and
//! `eustress-headless` with `EUSTRESS_HOST_ON_PLAY=1` do. The `eustress-server`
//! binary does not: its `main` says it loads no world and hosts no session.
//! A Forge allocation can be the target when it serves this protocol and you
//! have its join link (key and pin). A remote address without a pin is refused.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::join_link::JoinLink;
use crate::native::{self, join_on};
use crate::repl::input::key_bit_by_name;
use crate::repl::motion::TICK_HZ;
use crate::repl::{InputFrame, InputSample};
use crate::session::{LinkEvent, NetLink, HOST_CONN};
use crate::wire::{
    decode_body, decode_datagram, encode_datagram, encode_frame, AvatarFrame, Datagram, Hello,
    PeerId, ToHost, ToPlayer, PROTOCOL_VERSION,
};

const ENGINE_VERSION: &str = concat!("eustress-stress ", env!("CARGO_PKG_VERSION"));
/// Walking speed, m/s. Under the host's pre-replica pace (30 m/s).
const WALK_MPS: f32 = 3.0;
const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CLIENTS: u32 = 2000;

/// What one run does.
#[derive(Debug, Clone)]
pub struct StressConfig {
    pub clients: u32,
    pub duration: Duration,
    pub ramp: Duration,
    /// Avatar samples each bot sends per second. The Player sends 20.
    pub avatar_hz: u32,
    pub movement: bool,
    pub interactions: bool,
    /// Download the world's chunks before `WorldReady`. Off sends `WorldReady`
    /// straight after `Welcome`, which is what a Player with a full cache does.
    pub download_world: bool,
}

impl StressConfig {
    pub fn checked(mut self) -> Result<Self, String> {
        if self.clients == 0 || self.clients > MAX_CLIENTS {
            return Err(format!("client count must be 1 to {MAX_CLIENTS}"));
        }
        if self.duration < Duration::from_secs(1) {
            return Err("duration must be at least 1 second".into());
        }
        if self.ramp > self.duration {
            self.ramp = self.duration;
        }
        self.avatar_hz = self.avatar_hz.clamp(1, 60);
        Ok(self)
    }
}

/// Live numbers. Rates are per connected bot over the last sample window.
#[derive(Debug, Clone)]
pub struct StressSnapshot {
    pub running: bool,
    pub elapsed_secs: f32,
    pub duration_secs: f32,
    pub connected: u32,
    pub joined: u32,
    pub failed: u32,
    pub disconnects: u32,
    pub kicks: u32,
    pub tick_hz: f32,
    pub rtt_ms: f32,
    pub down_kbps: f32,
    pub up_kbps: f32,
    pub relay_p50_ms: f32,
    pub relay_p95_ms: f32,
    pub relay_mean_ms: f32,
    pub relay_loss: f32,
    pub join_p95_ms: f32,
    pub frames_received: u64,
    pub input_sent: u64,
    pub chunks_verified: u32,
    pub bot_cores: f32,
    pub report: String,
}

/// Fill a loopback address from a host file when the text has no key or pin.
///
/// A host on this machine writes `<workspace>/.eustress/hosts/<port>.link`.
/// The first workspace that has a fresh file for the port wins. A remote
/// link is returned unchanged; [`native::pin_required_reason`] rejects it
/// later if it has no pin.
pub fn resolve_link(text: &str, workspaces: &[impl AsRef<Path>]) -> Result<JoinLink, String> {
    let mut link = JoinLink::parse(text.trim())?;
    if link.is_loopback() && (link.key.is_none() || link.pin.is_none()) {
        for dir in workspaces {
            if let Some(saved) = native::read_host_file(dir.as_ref(), link.port) {
                if link.key.is_none() {
                    link.key = saved.key;
                }
                if link.pin.is_none() {
                    link.pin = saved.pin;
                }
                break;
            }
        }
    }
    Ok(link)
}

struct Samples {
    buf: Vec<u32>,
    next: usize,
    full: bool,
}

impl Samples {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
            next: 0,
            full: false,
        }
    }

    fn push(&mut self, v: u32) {
        if self.buf.len() < 4096 {
            self.buf.push(v);
            return;
        }
        self.buf[self.next] = v;
        self.next = (self.next + 1) % self.buf.len();
        self.full = true;
    }

    fn values(&self) -> &[u32] {
        &self.buf
    }
}

struct RateMark {
    at: Instant,
    up: u64,
    down: u64,
    inputs: u64,
    up_kbps: f32,
    down_kbps: f32,
    tick_hz: f32,
}

struct Stats {
    running: AtomicBool,
    started: Instant,
    duration: Duration,
    config_clients: u32,
    avatar_hz: u32,
    movement: bool,
    interactions: bool,
    download: bool,
    target: String,
    connected: AtomicU32,
    joined: AtomicU32,
    joining: AtomicU32,
    failed: AtomicU32,
    disconnects: AtomicU32,
    kicks: AtomicU32,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
    avatar_sent: AtomicU64,
    avatar_recv: AtomicU64,
    relay_expected: AtomicU64,
    input_sent: AtomicU64,
    chunks_verified: AtomicU32,
    join_ms: Mutex<Samples>,
    relay_ms: Mutex<Samples>,
    reasons: Mutex<HashMap<String, u32>>,
    rtt: Mutex<Vec<f32>>,
    sends: Mutex<HashMap<(PeerId, u32), u64>>,
    rate: Mutex<RateMark>,
    run_start: Instant,
}

impl Stats {
    fn new(config: &StressConfig, target: String) -> Self {
        let now = Instant::now();
        Self {
            running: AtomicBool::new(true),
            started: now,
            duration: config.duration,
            config_clients: config.clients,
            avatar_hz: config.avatar_hz,
            movement: config.movement,
            interactions: config.interactions,
            download: config.download_world,
            target,
            connected: AtomicU32::new(0),
            joined: AtomicU32::new(0),
            joining: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            disconnects: AtomicU32::new(0),
            kicks: AtomicU32::new(0),
            bytes_up: AtomicU64::new(0),
            bytes_down: AtomicU64::new(0),
            avatar_sent: AtomicU64::new(0),
            avatar_recv: AtomicU64::new(0),
            relay_expected: AtomicU64::new(0),
            input_sent: AtomicU64::new(0),
            chunks_verified: AtomicU32::new(0),
            join_ms: Mutex::new(Samples::new()),
            relay_ms: Mutex::new(Samples::new()),
            reasons: Mutex::new(HashMap::new()),
            rtt: Mutex::new(Vec::new()),
            sends: Mutex::new(HashMap::new()),
            rate: Mutex::new(RateMark {
                at: now,
                up: 0,
                down: 0,
                inputs: 0,
                up_kbps: 0.0,
                down_kbps: 0.0,
                tick_hz: 0.0,
            }),
            run_start: now,
        }
    }

    fn fail(&self, reason: &str) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        let mut reasons = self.reasons.lock();
        *reasons.entry(trim_reason(reason).to_string()).or_insert(0) += 1;
    }

    fn note_send(&self, peer: PeerId, seq: u32) {
        let micros = self.run_start.elapsed().as_micros() as u64;
        let mut sends = self.sends.lock();
        if sends.len() > 50_000 {
            sends.clear();
        }
        sends.insert((peer, seq), micros);
    }

    fn note_relay(&self, peer: PeerId, seq: u32) {
        self.avatar_recv.fetch_add(1, Ordering::Relaxed);
        let sent = self.sends.lock().remove(&(peer, seq));
        if let Some(sent) = sent {
            let now = self.run_start.elapsed().as_micros() as u64;
            let ms = now.saturating_sub(sent) / 1000;
            self.relay_ms.lock().push(ms.min(u32::MAX as u64) as u32);
        }
    }

    fn snapshot(&self, bot_cores: f32) -> StressSnapshot {
        let elapsed = self.started.elapsed().as_secs_f32();
        let connected = self.connected.load(Ordering::Relaxed);
        let (tick_hz, down_kbps, up_kbps) = self.rates(connected);
        let join = self.join_ms.lock();
        let relay = self.relay_ms.lock();
        let join_p95 = percentile(join.values(), 0.95);
        let relay_p50 = percentile(relay.values(), 0.50);
        let relay_p95 = percentile(relay.values(), 0.95);
        let relay_mean = mean(relay.values());
        drop(join);
        drop(relay);
        let expected = self.relay_expected.load(Ordering::Relaxed);
        let recv = self.avatar_recv.load(Ordering::Relaxed);
        let relay_loss = if expected == 0 {
            0.0
        } else {
            (1.0 - recv as f32 / expected as f32).clamp(0.0, 1.0)
        };
        let rtt_ms = {
            let rtt = self.rtt.lock();
            if rtt.is_empty() {
                0.0
            } else {
                rtt.iter().sum::<f32>() / rtt.len() as f32
            }
        };
        let snap = StressSnapshot {
            running: self.running.load(Ordering::Relaxed),
            elapsed_secs: elapsed,
            duration_secs: self.duration.as_secs_f32(),
            connected,
            joined: self.joined.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            disconnects: self.disconnects.load(Ordering::Relaxed),
            kicks: self.kicks.load(Ordering::Relaxed),
            tick_hz,
            rtt_ms,
            down_kbps,
            up_kbps,
            relay_p50_ms: relay_p50,
            relay_p95_ms: relay_p95,
            relay_mean_ms: relay_mean,
            relay_loss,
            join_p95_ms: join_p95,
            frames_received: recv,
            input_sent: self.input_sent.load(Ordering::Relaxed),
            chunks_verified: self.chunks_verified.load(Ordering::Relaxed),
            bot_cores,
            report: String::new(),
        };
        let mut snap = snap;
        snap.report = render_owned(self, &snap);
        snap
    }

    fn rates(&self, connected: u32) -> (f32, f32, f32) {
        let mut mark = self.rate.lock();
        let now = Instant::now();
        let dt = now.duration_since(mark.at).as_secs_f32();
        let up = self.bytes_up.load(Ordering::Relaxed);
        let down = self.bytes_down.load(Ordering::Relaxed);
        let inputs = self.input_sent.load(Ordering::Relaxed);
        if dt >= 0.25 {
            let bots = connected.max(1) as f32;
            mark.tick_hz = (inputs.saturating_sub(mark.inputs) as f32) / dt / bots;
            mark.up_kbps = (up.saturating_sub(mark.up) as f32) * 8.0 / dt / 1000.0 / bots;
            mark.down_kbps = (down.saturating_sub(mark.down) as f32) * 8.0 / dt / 1000.0 / bots;
            mark.at = now;
            mark.up = up;
            mark.down = down;
            mark.inputs = inputs;
        }
        (mark.tick_hz, mark.down_kbps, mark.up_kbps)
    }
}

fn trim_reason(reason: &str) -> &str {
    let reason = reason.trim();
    if reason.len() <= 180 {
        reason
    } else {
        &reason[..180]
    }
}

fn percentile(values: &[u32], p: f64) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let idx = ((v.len() - 1) as f64 * p).round() as usize;
    v[idx.min(v.len() - 1)] as f32
}

fn mean(values: &[u32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().map(|v| *v as f64).sum::<f64>() as f32 / values.len() as f32
}

fn render_owned(stats: &Stats, snap: &StressSnapshot) -> String {
    let cores = if snap.bot_cores > 0.0 {
        format!("{:.2} cores", snap.bot_cores)
    } else {
        "not measured (tokio runtime metrics need RUSTFLAGS=--cfg tokio_unstable)".to_string()
    };
    let mut out = format!(
        "Stress test\n\
         Target: {target}\n\
         Clients: {clients}   Duration: {dur:.0}s   Avatar: {hz} Hz   Move: {mv}   Interact: {inter}   Download: {dl}\n\
         \n\
         Connected: {conn}    Joined: {joined}    Failed: {failed}    Disconnects: {disc}    Kicks: {kicks}\n\
         Join time p95: {join:.0} ms\n\
         Input tick rate: {tick:.1} Hz\n\
         RTT: {rtt:.1} ms\n\
         Down: {down:.1} kbps/bot    Up: {up:.1} kbps/bot\n\
         Relay frames: {frames}    p50: {p50:.0} ms    p95: {p95:.0} ms    mean: {mean:.0} ms    loss: {loss:.1}%\n\
         Input frames sent: {inputs}\n\
         Chunks verified: {chunks}\n\
         Bot CPU: {cores}\n",
        target = stats.target,
        clients = stats.config_clients,
        dur = snap.duration_secs,
        hz = stats.avatar_hz,
        mv = yes(stats.movement),
        inter = yes(stats.interactions),
        dl = yes(stats.download),
        conn = snap.connected,
        joined = snap.joined,
        failed = snap.failed,
        disc = snap.disconnects,
        kicks = snap.kicks,
        join = snap.join_p95_ms,
        tick = snap.tick_hz,
        rtt = snap.rtt_ms,
        down = snap.down_kbps,
        up = snap.up_kbps,
        frames = snap.frames_received,
        p50 = snap.relay_p50_ms,
        p95 = snap.relay_p95_ms,
        mean = snap.relay_mean_ms,
        loss = snap.relay_loss * 100.0,
        inputs = snap.input_sent,
        chunks = snap.chunks_verified,
        cores = cores,
    );
    let reasons = stats.reasons.lock();
    if reasons.is_empty() {
        out.push_str("Failures: none\n");
    } else {
        out.push_str("Failures:\n");
        let mut rows: Vec<_> = reasons.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (reason, n) in rows {
            out.push_str(&format!("  {n}  {reason}\n"));
        }
    }
    if !snap.running {
        out.push_str("Status: finished\n");
    }
    out
}

fn yes(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

/// A run. Dropping it asks the bots to stop. The runtime stays until the
/// supervisor finishes or five seconds pass.
pub struct StressRun {
    stop: Arc<AtomicBool>,
    stats: Arc<Stats>,
    runtime: tokio::runtime::Runtime,
    supervisor: Option<tokio::task::JoinHandle<()>>,
}

impl StressRun {
    pub fn start(link: JoinLink, config: StressConfig) -> Result<Self, String> {
        if let Some(reason) = native::pin_required_reason(&link) {
            return Err(reason);
        }
        let config = config.checked()?;
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            / 2;
        let workers = workers.max(1);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .thread_name("eustress-stress")
            .build()
            .map_err(|e| format!("could not start the stress runtime: {e}"))?;
        let target = format!("{}:{}", link.host, link.port);
        let stats = Arc::new(Stats::new(&config, target));
        stats.rtt.lock().resize(config.clients as usize, 0.0);
        let stop = Arc::new(AtomicBool::new(false));
        let handle = runtime.handle().clone();
        let stats_task = Arc::clone(&stats);
        let stop_task = Arc::clone(&stop);
        let supervisor = runtime.spawn(async move {
            supervise(handle, link, config, stats_task, stop_task).await;
        });
        Ok(Self {
            stop,
            stats,
            runtime,
            supervisor: Some(supervisor),
        })
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.stats.running.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> StressSnapshot {
        self.stats
            .snapshot(bot_cores(&self.runtime, self.stats.started.elapsed()))
    }
}

impl Drop for StressRun {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let Some(supervisor) = self.supervisor.take() else {
            return;
        };
        let _ = self.runtime.block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(5), supervisor).await;
        });
    }
}

fn bot_cores(runtime: &tokio::runtime::Runtime, wall: Duration) -> f32 {
    #[cfg(tokio_unstable)]
    {
        let busy = runtime.metrics().worker_total_busy_duration();
        let wall = wall.as_secs_f32();
        if wall > 0.0 {
            return busy.as_secs_f32() / wall;
        }
    }
    let _ = (runtime, wall);
    0.0
}

async fn supervise(
    handle: tokio::runtime::Handle,
    link: JoinLink,
    config: StressConfig,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
) {
    let mut set = tokio::task::JoinSet::new();
    let n = config.clients;
    for i in 0..n {
        let delay = if n <= 1 {
            Duration::ZERO
        } else {
            config.ramp.mul_f32(i as f32 / (n - 1) as f32)
        };
        let link = link.clone();
        let stats = Arc::clone(&stats);
        let stop = Arc::clone(&stop);
        let handle = handle.clone();
        let task_handle = handle.clone();
        let movement = config.movement;
        let interactions = config.interactions;
        let download = config.download_world;
        let avatar_hz = config.avatar_hz;
        let deadline = Instant::now() + config.duration;
        set.spawn_on(
            async move {
                if !delay.is_zero() {
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = wait_stop(&stop) => return,
                    }
                }
                if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    return;
                }
                run_bot(
                    i as usize,
                    task_handle,
                    link,
                    stats,
                    stop,
                    deadline,
                    movement,
                    interactions,
                    download,
                    avatar_hz,
                    n,
                )
                .await;
            },
            &handle,
        );
    }
    while set.join_next().await.is_some() {}
    stats.running.store(false, Ordering::SeqCst);
}

async fn wait_stop(stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct Bot {
    id: usize,
    link: NetLink,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
    deadline: Instant,
    began: Instant,
    datagrams: bool,
    peer: PeerId,
    spawn: [f32; 3],
    origin: [f32; 3],
    walked_from: Instant,
    tick: u64,
    seq: u32,
    jumps: u8,
    joined: bool,
    leaving: bool,
    movement: bool,
    interactions: bool,
    download: bool,
    avatar_every: Duration,
    next_avatar: Instant,
    next_input: Instant,
    next_chat: Instant,
    next_jump: Instant,
    others: u32,
}

async fn run_bot(
    id: usize,
    handle: tokio::runtime::Handle,
    target: JoinLink,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
    deadline: Instant,
    movement: bool,
    interactions: bool,
    download: bool,
    avatar_hz: u32,
    clients: u32,
) {
    stats.joining.fetch_add(1, Ordering::Relaxed);
    let net = match join_on(&handle, &target) {
        Ok(net) => net,
        Err(reason) => {
            stats.joining.fetch_sub(1, Ordering::Relaxed);
            stats.fail(&reason);
            return;
        }
    };
    let mut bot = Bot {
        id,
        link: net,
        stats,
        stop,
        deadline,
        began: Instant::now(),
        datagrams: true,
        peer: 0,
        spawn: [0.0; 3],
        origin: [0.0; 3],
        walked_from: Instant::now(),
        tick: 0,
        seq: 0,
        jumps: 0,
        joined: false,
        leaving: false,
        movement,
        interactions,
        download,
        avatar_every: Duration::from_secs_f64(1.0 / avatar_hz as f64),
        next_avatar: Instant::now(),
        next_input: Instant::now(),
        next_chat: Instant::now() + Duration::from_secs(2),
        next_jump: Instant::now() + Duration::from_secs(3),
        others: clients.saturating_sub(1),
    };
    let mut fail: Option<String> = None;
    let join_deadline = Instant::now() + JOIN_TIMEOUT;
    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    let mut expected: HashMap<String, u64> = HashMap::new();
    let mut world_ready_sent = false;

    while !bot.leaving {
        if bot.stop.load(Ordering::Relaxed) || Instant::now() >= bot.deadline {
            break;
        }
        while let Some(event) = bot.link.poll() {
            match event {
                LinkEvent::Opened { datagrams, .. } => {
                    bot.datagrams = datagrams;
                    let hello = ToHost::Hello(Hello {
                        protocol: PROTOCOL_VERSION,
                        name: format!("Bot {:03}", bot.id + 1),
                        engine_version: ENGINE_VERSION.into(),
                        identity: None,
                    });
                    send_reliable(&bot.link, &bot.stats, &hello);
                }
                LinkEvent::Frame { body, .. } => {
                    bot.stats
                        .bytes_down
                        .fetch_add((4 + body.len()) as u64, Ordering::Relaxed);
                    match decode_body::<ToPlayer>(&body) {
                        Ok(ToPlayer::Welcome(welcome)) => {
                            if welcome.protocol != PROTOCOL_VERSION {
                                fail = Some(format!(
                                    "host speaks protocol {} and this bot speaks {PROTOCOL_VERSION}",
                                    welcome.protocol
                                ));
                                break;
                            }
                            bot.peer = welcome.peer;
                            bot.spawn = welcome.spawn;
                            bot.origin = welcome.spawn;
                            bot.walked_from = Instant::now();
                            bot.tick = welcome.tick;
                            if bot.download {
                                for chunk in welcome.world.all_chunks() {
                                    expected.insert(chunk.blake3.clone(), chunk.size);
                                }
                                let hashes: Vec<String> = expected.keys().cloned().collect();
                                send_reliable(
                                    &bot.link,
                                    &bot.stats,
                                    &ToHost::RequestChunks { hashes },
                                );
                            }
                            if !bot.download || expected.is_empty() {
                                send_reliable(&bot.link, &bot.stats, &ToHost::WorldReady);
                                world_ready_sent = true;
                            }
                        }
                        Ok(ToPlayer::Refused { reason }) => {
                            fail = Some(reason);
                            break;
                        }
                        Ok(ToPlayer::Kicked { reason }) => {
                            bot.stats.kicks.fetch_add(1, Ordering::Relaxed);
                            fail = Some(reason);
                            break;
                        }
                        Ok(ToPlayer::ChunkPiece {
                            hash,
                            offset,
                            total,
                            bytes,
                        }) => {
                            if let Err(reason) = take_piece(
                                &mut bot,
                                &mut chunks,
                                &expected,
                                &hash,
                                offset,
                                total,
                                &bytes,
                            ) {
                                fail = Some(reason);
                                break;
                            }
                            if !world_ready_sent && chunks_done(&chunks, &expected) {
                                send_reliable(&bot.link, &bot.stats, &ToHost::WorldReady);
                                world_ready_sent = true;
                            }
                        }
                        Ok(ToPlayer::ChunkMissing { hash }) => {
                            fail = Some(format!("host has no chunk {hash}"));
                            break;
                        }
                        Ok(ToPlayer::PeerJoined(_)) => {
                            // The host sends these only after it has accepted WorldReady.
                            if world_ready_sent && !bot.joined {
                                bot.mark_joined();
                            }
                        }
                        Ok(ToPlayer::Correct { position }) => {
                            bot.origin = position;
                            bot.walked_from = Instant::now();
                        }
                        Ok(ToPlayer::Avatar(frame)) => bot.on_avatar(frame),
                        Ok(ToPlayer::World(_)) => {}
                        Ok(_) => {}
                        Err(e) => {
                            fail = Some(format!("unreadable host message: {e}"));
                            break;
                        }
                    }
                }
                LinkEvent::Datagram { bytes, .. } => {
                    bot.stats
                        .bytes_down
                        .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    if let Ok(Datagram::Avatar(frame)) = decode_datagram(&bytes) {
                        bot.on_avatar(frame);
                    }
                }
                LinkEvent::PathStats { rtt_ms, .. } => {
                    if let Some(slot) = bot.stats.rtt.lock().get_mut(bot.id) {
                        *slot = rtt_ms;
                    }
                }
                LinkEvent::Closed { reason, .. } => {
                    if !bot.leaving {
                        if bot.joined {
                            bot.stats.disconnects.fetch_add(1, Ordering::Relaxed);
                        }
                        fail = Some(format!("connection closed: {reason}"));
                    }
                    break;
                }
                LinkEvent::Failed { reason } => {
                    fail = Some(reason);
                    break;
                }
                LinkEvent::Listening { .. } => {}
            }
        }
        if fail.is_some() {
            break;
        }
        if !bot.joined && Instant::now() >= join_deadline {
            fail = Some("timed out waiting to join".into());
            break;
        }
        if bot.joined {
            bot.play();
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    if bot.joined && fail.is_none() {
        bot.leaving = true;
        send_reliable(&bot.link, &bot.stats, &ToHost::Goodbye);
        bot.link.close(HOST_CONN, "stress test done");
        bot.stats.connected.fetch_sub(1, Ordering::Relaxed);
    } else if !bot.joined {
        bot.stats.joining.fetch_sub(1, Ordering::Relaxed);
        bot.stats
            .fail(&fail.unwrap_or_else(|| "left before joining".into()));
    } else if let Some(reason) = fail {
        bot.stats.connected.fetch_sub(1, Ordering::Relaxed);
        bot.stats.fail(&reason);
    }
}

impl Bot {
    fn mark_joined(&mut self) {
        self.joined = true;
        self.stats.joining.fetch_sub(1, Ordering::Relaxed);
        self.stats.joined.fetch_add(1, Ordering::Relaxed);
        self.stats.connected.fetch_add(1, Ordering::Relaxed);
        let ms = self.began.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        self.stats.join_ms.lock().push(ms);
        self.next_avatar = Instant::now();
        self.next_input = Instant::now();
    }

    fn on_avatar(&self, frame: AvatarFrame) {
        if self.joined && frame.peer != self.peer && frame.peer != 0 {
            self.stats.note_relay(frame.peer, frame.seq);
        }
    }

    fn play(&mut self) {
        let now = Instant::now();
        if self.movement && now >= self.next_avatar {
            self.seq = self.seq.wrapping_add(1);
            let t = self.walked_from.elapsed().as_secs_f32();
            let dist = (WALK_MPS * t).min(8.0);
            let angle = t * 0.6 + self.id as f32 * 0.4;
            let (s, c) = angle.sin_cos();
            let position = [
                self.origin[0] + c * dist,
                self.origin[1],
                self.origin[2] + s * dist,
            ];
            let frame = AvatarFrame {
                peer: self.peer,
                seq: self.seq,
                position,
                yaw: angle,
                direction: [c, 0.0, s],
                vertical_velocity: 0.0,
                flags: AvatarFrame::GROUNDED,
                jumps: self.jumps,
            };
            self.stats.note_send(self.peer, self.seq);
            self.stats.avatar_sent.fetch_add(1, Ordering::Relaxed);
            self.stats
                .relay_expected
                .fetch_add(self.others as u64, Ordering::Relaxed);
            if self.datagrams {
                let bytes = encode_datagram(&Datagram::Avatar(frame));
                self.stats
                    .bytes_up
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                self.link.datagram(HOST_CONN, bytes);
            } else {
                send_reliable(&self.link, &self.stats, &ToHost::Avatar(frame));
            }
            self.next_avatar = now + self.avatar_every;
        }
        if self.movement && now >= self.next_input {
            let mut sample = InputSample::default();
            sample.tick = self.tick;
            self.tick = self.tick.wrapping_add(1);
            sample.camera = self.spawn;
            if let Some(bit) = key_bit_by_name("W") {
                sample.set_key(bit);
            }
            if self.interactions && now >= self.next_jump {
                if let Some(bit) = key_bit_by_name("Space") {
                    sample.set_key(bit);
                }
                self.jumps = self.jumps.wrapping_add(1);
                self.next_jump = now + Duration::from_secs(4);
            }
            let frame = InputFrame {
                samples: vec![sample],
            };
            self.stats.input_sent.fetch_add(1, Ordering::Relaxed);
            if self.datagrams {
                let bytes = encode_datagram(&Datagram::Input(frame));
                self.stats
                    .bytes_up
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                self.link.datagram(HOST_CONN, bytes);
            } else {
                send_reliable(&self.link, &self.stats, &ToHost::Input(frame));
            }
            self.next_input = now + Duration::from_secs_f64(1.0 / TICK_HZ);
        }
        if self.interactions && now >= self.next_chat {
            send_reliable(
                &self.link,
                &self.stats,
                &ToHost::Chat {
                    text: format!("bot {} checking in", self.id + 1),
                },
            );
            self.next_chat = now + Duration::from_secs(4);
        }
    }
}

fn send_reliable(link: &NetLink, stats: &Stats, msg: &ToHost) {
    if let Ok(frame) = encode_frame(msg) {
        stats
            .bytes_up
            .fetch_add(frame.len() as u64, Ordering::Relaxed);
    }
    link.send(HOST_CONN, msg);
}

fn take_piece(
    bot: &mut Bot,
    chunks: &mut HashMap<String, Vec<u8>>,
    expected: &HashMap<String, u64>,
    hash: &str,
    offset: u64,
    total: u64,
    bytes: &[u8],
) -> Result<(), String> {
    let Some(size) = expected.get(hash).copied() else {
        return Err(format!(
            "host sent a chunk that was not in the manifest ({hash})"
        ));
    };
    if total != size {
        return Err(format!(
            "chunk {hash} is {total} bytes and the manifest says {size}"
        ));
    }
    let buf = chunks.entry(hash.to_string()).or_default();
    if offset != buf.len() as u64 {
        return Err(format!(
            "chunk {hash} piece at {offset} arrived out of order"
        ));
    }
    buf.extend_from_slice(bytes);
    if buf.len() as u64 > total {
        return Err(format!("chunk {hash} grew past its size"));
    }
    if buf.len() as u64 == total {
        let got = eustress_echk::content_hash(buf);
        if got != hash {
            return Err(format!("chunk {hash} failed its content hash"));
        }
        bot.stats.chunks_verified.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn chunks_done(chunks: &HashMap<String, Vec<u8>>, expected: &HashMap<String, u64>) -> bool {
    expected.iter().all(|(hash, size)| {
        chunks
            .get(hash)
            .is_some_and(|buf| buf.len() as u64 == *size)
    })
}

/// Run until the duration ends or [`StressRun::request_stop`], printing a
/// line a second. The snapshot's `running` is false when this returns.
pub fn run_blocking(link: JoinLink, config: StressConfig) -> Result<StressSnapshot, String> {
    let duration = config.duration;
    let run = StressRun::start(link, config)?;
    let safety = Instant::now() + duration + Duration::from_secs(45);
    while run.is_running() && Instant::now() < safety {
        std::thread::sleep(Duration::from_millis(200));
    }
    if run.is_running() {
        run.request_stop();
        let give_up = Instant::now() + Duration::from_secs(8);
        while run.is_running() && Instant::now() < give_up {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(run.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use eustress_echk::{ChunkEntry, SpaceManifest, WorldManifest};

    use crate::native::{new_join_key, start_host, HostOptions};
    use crate::session::{app_hosting, HostConfig, HostSession};
    use crate::wire::PROTOCOL_VERSION;

    #[test]
    fn percentiles_put_the_high_value_at_p95() {
        let values = [1, 2, 3, 4, 100];
        assert_eq!(percentile(&values, 0.5), 3.0);
        assert_eq!(percentile(&values, 0.95), 100.0);
        assert!(mean(&values) > 3.0);
    }

    #[test]
    fn a_remote_link_without_a_pin_is_refused_before_any_bot_starts() {
        let link = JoinLink {
            host: "203.0.113.9".into(),
            port: 7777,
            key: Some("abcdef".into()),
            pin: None,
        };
        let err = match StressRun::start(link, small_config(1, 1)) {
            Err(reason) => reason,
            Ok(_) => panic!("a remote link without a pin should be refused before any bot starts"),
        };
        assert!(err.contains("certificate pin"), "{err}");
    }

    #[test]
    fn a_loopback_address_reads_the_host_file() {
        let dir = std::env::temp_dir().join(format!("eustress-stress-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let saved = JoinLink {
            host: "127.0.0.1".into(),
            port: 7791,
            key: Some(new_join_key()),
            pin: Some([9; 32]),
        };
        native::write_host_file(&dir, &saved);
        let link = resolve_link("127.0.0.1:7791", &[dir.clone()]).unwrap();
        assert_eq!(link.pin, Some([9; 32]));
        assert_eq!(link.key, saved.key);
        let _ = std::fs::remove_dir_all(&dir);
        let remote = resolve_link("203.0.113.9:7777", &[] as &[std::path::PathBuf]).unwrap();
        assert!(remote.pin.is_none());
        let _ = PROTOCOL_VERSION;
    }

    #[test]
    fn bots_join_a_loopback_host_move_and_leave() {
        let key = new_join_key();
        let host = start_host(HostOptions {
            port: 0,
            lan: false,
            join_key: key.clone(),
        })
        .unwrap();
        let (port, pin) = wait_listening(&host);
        let (world, chunks) = tiny_world();
        let mut app = app_hosting(
            host,
            HostConfig {
                host_name: "StressHost".into(),
                max_players: 8,
                world,
                chunks,
                spawn: [0.0, 1.0, 0.0],
                sim_id: None,
            },
        );
        let target = JoinLink {
            host: "127.0.0.1".into(),
            port,
            key: Some(key),
            pin: Some(pin),
        };
        let run = StressRun::start(target, small_config(4, 3)).unwrap();
        let wall = Instant::now() + Duration::from_secs(25);
        let mut peak = 0usize;
        let mut saw_bot = false;
        while Instant::now() < wall {
            {
                let mut time = app.world_mut().resource_mut::<bevy::prelude::Time>();
                time.advance_by(Duration::from_millis(16));
            }
            app.update();
            let host_session = app.world().resource::<HostSession>();
            peak = peak.max(host_session.player_count());
            if host_session
                .player_names()
                .iter()
                .any(|n| n.starts_with("Bot "))
            {
                saw_bot = true;
            }
            let snap = run.snapshot();
            if !snap.running && snap.joined + snap.failed >= 4 {
                break;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        let snap = run.snapshot();
        assert_eq!(snap.failed, 0, "{}", snap.report);
        assert_eq!(snap.kicks, 0, "{}", snap.report);
        assert_eq!(snap.joined, 4, "{}", snap.report);
        assert_eq!(
            peak, 4,
            "the host should have seen 4 players\n{}",
            snap.report
        );
        assert_eq!(snap.chunks_verified, 4, "{}", snap.report);
        assert!(
            snap.input_sent > 0,
            "bots should have sent input\n{}",
            snap.report
        );
        assert!(
            snap.frames_received > 0,
            "relayed avatar frames should arrive\n{}",
            snap.report
        );
        assert!(snap.rtt_ms >= 0.0, "{}", snap.report);
        assert!(saw_bot, "the host should have named a joined bot");
    }

    fn small_config(clients: u32, secs: u64) -> StressConfig {
        StressConfig {
            clients,
            duration: Duration::from_secs(secs),
            ramp: Duration::from_millis(200),
            avatar_hz: 20,
            movement: true,
            interactions: true,
            download_world: true,
        }
    }

    fn tiny_world() -> (WorldManifest, HashMap<String, Arc<Vec<u8>>>) {
        let bytes = b"eustress-stress-chunk-v1".to_vec();
        let hash = eustress_echk::content_hash(&bytes);
        let mut world = WorldManifest::new("Stress", "test", 64.0);
        world.start_space = "Main".into();
        world.spaces.push(SpaceManifest {
            name: "Main".into(),
            chunks: vec![ChunkEntry {
                cx: 0,
                cz: 0,
                file: "0_0.echk".into(),
                size: bytes.len() as u64,
                count: 0,
                blake3: hash.clone(),
            }],
        });
        let mut chunks = HashMap::new();
        chunks.insert(hash, Arc::new(bytes));
        (world, chunks)
    }

    fn wait_listening(link: &NetLink) -> (u16, [u8; 32]) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            while let Some(ev) = link.poll() {
                match ev {
                    LinkEvent::Listening { port, cert_sha256 } => return (port, cert_sha256),
                    LinkEvent::Failed { reason } => panic!("host failed: {reason}"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("host did not listen");
    }
}
