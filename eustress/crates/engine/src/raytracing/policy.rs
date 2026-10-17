//! The ray-tracing switch (Auto, On, Off) and the guards that keep Auto safe
//! to ship as the default.
//!
//! # The switch
//!
//! Read once in `main`, before the app exists, because the renderer is made
//! before any plugin: `EUSTRESS_RAYTRACING` when it holds `auto`, `on` or
//! `off` (`1`, `true`, `yes` and `0`, `false`, `no` still work), else the
//! `ray_tracing` key of `~/.eustress_engine/settings.json`, else Auto. The
//! Settings > Graphics choice is written there as `"auto"`, `"on"` or `"off"`.
//! A choice takes effect at the next launch: the renderer is never switched
//! inside a session.
//!
//! - Auto (the default): ray tracing when the build has it, the GPU passes
//!   the gate, the GPU is real hardware, DLSS Ray Reconstruction is there to
//!   denoise it ([`Gate::denoiser`]), and no guard below has turned it off
//!   for this GPU.
//! - On: ray tracing whenever the build and the GPU can run it, denoised or
//!   not, whatever the guards recorded. A run that survives clears a crash
//!   record, so Auto can use ray tracing again afterwards.
//! - Off: never.
//!
//! # The guards
//!
//! What the guards learn is kept per GPU identity (adapter name, PCI vendor
//! and device ids, driver, driver version and graphics API) under
//! `<local data>/Eustress/raytracing/`, so a driver update or another card is
//! tested afresh.
//!
//! - Crash guard. A launch that is about to ray trace writes
//!   `launch-<pid>.json` naming its GPU and holds an OS file lock on it for
//!   as long as the marker stands. The marker is removed once the app has run
//!   [`SURVIVAL_FRAMES`] frames with ray tracing on, and on a clean exit. A
//!   later launch that can take a marker's lock knows its process ended
//!   without removing it, and records "crashed" for that GPU, so Auto starts
//!   without ray tracing there. A marker whose lock is held belongs to another
//!   Eustress window that is still starting, and is left alone, which a plain
//!   flag in a shared file could not tell apart from a crash.
//! - Render errors. Bevy 0.19's default `RenderErrorHandler` answers every
//!   wgpu error it catches (device lost, out of memory, validation, internal)
//!   by logging it, sending `AppExit::error()` and stopping the render
//!   schedule, so the app quits at the end of that frame. While ray tracing
//!   is on, the handler here first puts the crash marker back (with the error
//!   as its reason) and marks the session failed, so no later step removes
//!   it, then does exactly what Bevy's default does. A panic whose location is
//!   in the renderer's crates is treated the same way.
//! - Performance guard. Once loading has settled, while the window is
//!   focused, at full speed and in Edit mode, the median frame time of each
//!   10 second window is compared with a 33.3 ms budget. Over it in Auto, the
//!   GPU is recorded "too slow" and a notification says so: this session keeps
//!   ray tracing, the next launch in Auto draws without it, and Settings can
//!   force it On. Under it in On, an earlier "too slow" record is cleared.
//!   It never switches renderers inside a session: changing the lighting
//!   under someone mid-edit is worse than a slow session.
//! - Texture limit. Solari 0.19 binds the textures of every StandardMaterial
//!   (traced or not) into arrays of [`SOLARI_TEXTURE_LIMIT`] entries; past
//!   that its bind group fails validation and the app quits. The distinct
//!   material textures are counted from material asset events, and at
//!   [`TEXTURE_FALLBACK_AT`] the session falls back to raster at once
//!   ([`RasterFallback`]): Solari leaves every camera, everything traced
//!   leaves the traced scene, the proxies go. The Space is recorded, so Auto
//!   opens it without ray tracing next time, until a session sees it settled
//!   under [`TEXTURE_CLEAR_BELOW`]. This applies in On too: it is a hard
//!   limit, not a judgement.
//! - Settings Off applies at once through the same fallback
//!   ([`RasterFallbackRequest`]). Auto and On, from raster, apply at the next
//!   launch.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::asset::AssetEvent;
use bevy::prelude::*;
use bevy::render::error_handler::{RenderError, RenderErrorHandler, RenderErrorPolicy};
use bevy::time::Real;
use serde_json::{json, Value};

use super::{GpuCapabilities, RaytracingLaunch, RaytracingStatus};

// ============================================================================
// The switch
// ============================================================================

/// The launch variable. While it holds a valid value it wins over the
/// Settings choice, like the engine's other launch variables, so an A/B run
/// measures what it asked for.
pub const ENV_SWITCH: &str = "EUSTRESS_RAYTRACING";

/// The `settings.json` key the Settings > Graphics choice is saved under
/// (`EditorSettings::ray_tracing`). Read here from the file, because the
/// renderer is made before `EditorSettings` loads.
pub const SETTINGS_KEY: &str = "ray_tracing";

/// The ray-tracing choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RayTracingMode {
    /// Ray tracing where the GPU can run it well, denoised by DLSS Ray
    /// Reconstruction. The default.
    #[default]
    Auto,
    /// Ray tracing whenever the build and the GPU can run it, denoised or not.
    On,
    /// Never.
    Off,
}

impl RayTracingMode {
    /// A launch-variable or Settings value. `auto`, `on` and `off`, and the
    /// flag spellings Phase 1 read: `1`, `true`, `yes` for On and `0`,
    /// `false`, `no` for Off. Anything else is not a choice.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "on" | "1" | "true" | "yes" => Some(Self::On),
            "off" | "0" | "false" | "no" => Some(Self::Off),
            _ => None,
        }
    }

    /// The saved Settings value. A string as [`Self::parse`] reads it, or the
    /// Phase 1 boolean: `true` was an explicit opt-in, so On; `false` was the
    /// default that every save wrote whether or not anyone chose it, so it
    /// carries no choice and reads as Auto.
    pub fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::String(text) => Self::parse(text),
            Value::Bool(true) => Some(Self::On),
            Value::Bool(false) => Some(Self::Auto),
            _ => None,
        }
    }

    /// The saved spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    /// For people.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::On => "On",
            Self::Off => "Off",
        }
    }

    /// The position in the Settings dialog's Auto / On / Off control.
    pub fn index(self) -> i32 {
        match self {
            Self::Auto => 0,
            Self::On => 1,
            Self::Off => 2,
        }
    }

    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Auto),
            1 => Some(Self::On),
            2 => Some(Self::Off),
            _ => None,
        }
    }
}

/// Saved as `"auto"`, `"on"` or `"off"`.
impl serde::Serialize for RayTracingMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Never fails: `EditorSettings` is one derived struct, so a field that
/// refused its value would reset every setting to its default. An unknown
/// value reads as Auto.
impl<'de> serde::Deserialize<'de> for RayTracingMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Ok(Self::from_json(&value).unwrap_or_default())
    }
}

/// Where this run's choice came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchSource {
    /// `EUSTRESS_RAYTRACING`, which wins over Settings while it is set.
    Env,
    /// The Settings choice in `settings.json`.
    Settings,
    /// Neither: the default, Auto.
    Default,
}

/// This run's choice: the launch variable when it holds a valid value, else
/// the saved Settings choice, else Auto. An empty or unknown launch variable
/// is passed over, so a typo does not silently turn ray tracing off.
pub fn requested_mode(env: Option<&str>, settings_json: Option<&str>) -> (RayTracingMode, SwitchSource) {
    if let Some(mode) = env.and_then(RayTracingMode::parse) {
        return (mode, SwitchSource::Env);
    }
    let saved = settings_json
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|settings| settings.get(SETTINGS_KEY).and_then(RayTracingMode::from_json));
    match saved {
        Some(mode) => (mode, SwitchSource::Settings),
        None => (RayTracingMode::Auto, SwitchSource::Default),
    }
}

/// A launch variable that is set but holds no choice, for the startup log.
pub fn ignored_env_value(env: Option<&str>) -> Option<String> {
    let value = env?;
    if value.trim().is_empty() || RayTracingMode::parse(value).is_some() {
        return None;
    }
    Some(format!(
        "{ENV_SWITCH}={value:?} is not auto, on or off, so the Settings choice decides"
    ))
}

/// The saved settings, from the path `EditorSettings` uses.
pub fn saved_settings_text() -> Option<String> {
    let path = dirs::home_dir()?.join(".eustress_engine").join("settings.json");
    fs::read_to_string(path).ok()
}

// ============================================================================
// The decision
// ============================================================================

/// What the gate read from the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    /// The device has every feature Solari needs.
    pub supported: bool,
    /// A software rasterizer (wgpu's `DeviceType::Cpu`: WARP, llvmpipe,
    /// lavapipe). Some report ray queries, at a speed no one would choose.
    pub software: bool,
    /// DLSS Ray Reconstruction can denoise Solari's output: the build
    /// compiles DLSS and the device reported it when it was made. Bevy Solari
    /// 0.19 has no denoiser of its own, so Auto ray traces only with this.
    pub denoiser: bool,
}

impl Gate {
    /// `denoiser`: the device's report of DLSS Ray Reconstruction, false in a
    /// build without DLSS.
    pub fn of(caps: &GpuCapabilities, denoiser: bool) -> Self {
        Self { supported: caps.ray_tracing, software: caps.device_type == "Cpu", denoiser }
    }
}

impl RaytracingStatus {
    /// The decision, in the order the facts are known. `gate` is `None` when
    /// no GPU could be tested (no graphics backend); `verdict` is the guard
    /// record for this GPU, which only Auto heeds.
    pub fn decide(
        mode: RayTracingMode,
        compiled: bool,
        gate: Option<Gate>,
        verdict: Option<VerdictKind>,
    ) -> Self {
        if mode == RayTracingMode::Off {
            return Self::Off;
        }
        if !compiled {
            return Self::NotInBuild;
        }
        let Some(gate) = gate.filter(|gate| gate.supported) else {
            return Self::Unsupported;
        };
        if mode == RayTracingMode::On {
            return Self::Active;
        }
        if gate.software {
            return Self::SoftwareGpu;
        }
        if !gate.denoiser {
            return Self::NoDenoiser;
        }
        match verdict {
            Some(VerdictKind::Crashed) => Self::AfterCrash,
            Some(VerdictKind::TooSlow) => Self::TooSlow,
            None => Self::Active,
        }
    }

    /// Auto opens a Space recorded as over Solari's texture limit without ray
    /// tracing. On tries it anyway; the live fallback still guards it.
    pub fn after_space_verdict(self, mode: RayTracingMode, space_over_limit: bool) -> Self {
        if self == Self::Active && mode == RayTracingMode::Auto && space_over_limit {
            Self::SpaceTooManyTextures
        } else {
            self
        }
    }

    /// Auto ray traces only when the crash guard can protect the next launch:
    /// without a marker, a GPU that crashes would crash at every launch.
    pub fn after_arming(self, mode: RayTracingMode, armed: bool) -> Self {
        if self == Self::Active && mode == RayTracingMode::Auto && !armed {
            Self::NoGuard
        } else {
            self
        }
    }
}

// ============================================================================
// Guard records
// ============================================================================

/// The GPU a guard record belongs to. A new driver, card or graphics API is
/// a new identity, so it is tested afresh.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuIdentity {
    pub adapter: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub driver: String,
    pub driver_info: String,
    pub backend: String,
}

impl GpuIdentity {
    pub fn of(caps: &GpuCapabilities) -> Self {
        Self {
            adapter: caps.adapter.clone(),
            vendor_id: caps.vendor_id,
            device_id: caps.device_id,
            driver: caps.driver.clone(),
            driver_info: caps.driver_info.clone(),
            backend: caps.backend.clone(),
        }
    }

    /// The record key: every field, so any change is a new key.
    pub fn key(&self) -> String {
        format!(
            "{:04x}:{:04x}|{}|{}|{}|{}",
            self.vendor_id, self.device_id, self.adapter, self.driver, self.driver_info, self.backend
        )
    }

    fn to_json(&self) -> Value {
        json!({
            "adapter": self.adapter,
            "vendor_id": self.vendor_id,
            "device_id": self.device_id,
            "driver": self.driver,
            "driver_info": self.driver_info,
            "backend": self.backend,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
        let id = |name: &str| value.get(name).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
        Some(Self {
            adapter: text("adapter")?,
            vendor_id: id("vendor_id")?,
            device_id: id("device_id")?,
            driver: text("driver")?,
            driver_info: text("driver_info")?,
            backend: text("backend")?,
        })
    }
}

/// Why Auto stays off on a GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictKind {
    /// A launch with ray tracing on did not get through its first
    /// [`SURVIVAL_FRAMES`] frames, or hit a render error.
    Crashed,
    /// The performance guard measured frames over the budget.
    TooSlow,
}

impl VerdictKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Crashed => "crashed",
            Self::TooSlow => "too_slow",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "crashed" => Some(Self::Crashed),
            "too_slow" => Some(Self::TooSlow),
            _ => None,
        }
    }
}

/// One guard record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub kind: VerdictKind,
    /// What happened, for the log and for support.
    pub detail: String,
    /// Seconds since the Unix epoch.
    pub recorded_unix: u64,
}

/// Every GPU's record, as `gpu_verdicts.json` holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardRecords {
    gpus: BTreeMap<String, (GpuIdentity, Verdict)>,
    spaces: BTreeMap<String, SpaceVerdict>,
}

/// A Space whose materials reached the texture limit under ray tracing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceVerdict {
    pub distinct_textures: usize,
    pub recorded_unix: u64,
}

/// The record key for a Space: its root path with forward slashes and no
/// trailing slash, lower-cased on Windows, where paths ignore case.
pub fn space_key(root: &Path) -> String {
    let text = root.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

impl GuardRecords {
    /// Lenient: an entry that cannot be read is dropped, never the file.
    pub fn from_json(text: &str) -> Self {
        let mut records = Self::default();
        let Ok(root) = serde_json::from_str::<Value>(text) else {
            return records;
        };
        let Some(gpus) = root.get("gpus").and_then(Value::as_object) else {
            return records;
        };
        for entry in gpus.values() {
            let Some(identity) = entry.get("identity").and_then(GpuIdentity::from_json) else {
                continue;
            };
            let Some(kind) = entry.get("verdict").and_then(Value::as_str).and_then(VerdictKind::parse) else {
                continue;
            };
            let verdict = Verdict {
                kind,
                detail: entry.get("detail").and_then(Value::as_str).unwrap_or_default().to_string(),
                recorded_unix: entry.get("recorded_unix").and_then(Value::as_u64).unwrap_or(0),
            };
            records.gpus.insert(identity.key(), (identity, verdict));
        }
        if let Some(spaces) = root.get("spaces").and_then(Value::as_object) {
            for (key, entry) in spaces {
                let Some(distinct_textures) = entry.get("distinct_textures").and_then(Value::as_u64) else {
                    continue;
                };
                let recorded_unix = entry.get("recorded_unix").and_then(Value::as_u64).unwrap_or(0);
                records.spaces.insert(
                    key.clone(),
                    SpaceVerdict { distinct_textures: distinct_textures as usize, recorded_unix },
                );
            }
        }
        records
    }

    pub fn to_json(&self) -> String {
        let spaces: serde_json::Map<String, Value> = self
            .spaces
            .iter()
            .map(|(key, v)| {
                (key.clone(), json!({ "distinct_textures": v.distinct_textures, "recorded_unix": v.recorded_unix }))
            })
            .collect();
        let gpus: serde_json::Map<String, Value> = self
            .gpus
            .iter()
            .map(|(key, (identity, verdict))| {
                let entry = json!({
                    "identity": identity.to_json(),
                    "verdict": verdict.kind.as_str(),
                    "detail": verdict.detail,
                    "recorded_unix": verdict.recorded_unix,
                });
                (key.clone(), entry)
            })
            .collect();
        let root = json!({ "version": 1, "gpus": gpus, "spaces": spaces });
        serde_json::to_string_pretty(&root).unwrap_or_default()
    }

    pub fn verdict(&self, identity: &GpuIdentity) -> Option<&Verdict> {
        self.gpus.get(&identity.key()).map(|(_, verdict)| verdict)
    }

    pub fn len(&self) -> usize {
        self.gpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.gpus.is_empty() && self.spaces.is_empty()
    }

    pub fn space_verdict(&self, key: &str) -> Option<&SpaceVerdict> {
        self.spaces.get(key)
    }

    pub fn record_space(&mut self, key: String, verdict: SpaceVerdict) -> bool {
        if self.spaces.get(&key) == Some(&verdict) {
            return false;
        }
        self.spaces.insert(key, verdict);
        true
    }

    pub fn clear_space(&mut self, key: &str) -> bool {
        self.spaces.remove(key).is_some()
    }

    /// Record a verdict. "Too slow" never replaces "crashed": a crash is the
    /// stronger reason, and clearing it takes a run that survives.
    pub fn record(&mut self, identity: &GpuIdentity, verdict: Verdict) -> bool {
        let key = identity.key();
        if verdict.kind == VerdictKind::TooSlow
            && self.gpus.get(&key).is_some_and(|(_, v)| v.kind == VerdictKind::Crashed)
        {
            return false;
        }
        if self.gpus.get(&key).is_some_and(|(_, v)| *v == verdict) {
            return false;
        }
        self.gpus.insert(key, (identity.clone(), verdict));
        true
    }

    /// Remove this GPU's record if it is of `kind`.
    pub fn clear(&mut self, identity: &GpuIdentity, kind: VerdictKind) -> bool {
        let key = identity.key();
        if self.gpus.get(&key).is_some_and(|(_, v)| v.kind == kind) {
            self.gpus.remove(&key);
            true
        } else {
            false
        }
    }
}

/// A launch marker: written before a launch ray traces, removed once it has
/// survived. One left behind by a process that is gone is a crash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchMarker {
    pub identity: GpuIdentity,
    pub pid: u32,
    pub started_unix: u64,
    /// Set when the session failed after the marker went up: a render error
    /// or a panic in the renderer.
    pub reason: Option<String>,
}

impl LaunchMarker {
    pub fn to_json(&self) -> String {
        json!({
            "identity": self.identity.to_json(),
            "pid": self.pid,
            "started_unix": self.started_unix,
            "reason": self.reason,
        })
        .to_string()
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let value = serde_json::from_str::<Value>(text).ok()?;
        Some(Self {
            identity: GpuIdentity::from_json(value.get("identity")?)?,
            pid: value.get("pid").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok())?,
            started_unix: value.get("started_unix").and_then(Value::as_u64)?,
            reason: value.get("reason").and_then(Value::as_str).map(str::to_string),
        })
    }

    /// What the next launch records for it.
    pub fn verdict(&self, now_unix: u64) -> Verdict {
        Verdict {
            kind: VerdictKind::Crashed,
            detail: self.reason.clone().unwrap_or_else(|| {
                format!("a launch with ray tracing on stopped within its first {SURVIVAL_FRAMES} frames")
            }),
            recorded_unix: now_unix,
        }
    }
}

/// Turn markers left behind by ended processes into "crashed" records.
/// Returns the keys of the GPUs it recorded.
pub fn absorb_stale_markers(records: &mut GuardRecords, stale: &[LaunchMarker], now_unix: u64) -> Vec<String> {
    let mut recorded = Vec::new();
    for marker in stale {
        if records.record(&marker.identity, marker.verdict(now_unix)) {
            recorded.push(marker.identity.key());
        }
    }
    recorded
}

// ============================================================================
// The store on disk
// ============================================================================

/// The guard folder: `gpu_verdicts.json` and the launch markers.
#[derive(Clone, Debug)]
pub struct GuardStore {
    dir: PathBuf,
}

/// What a launch learned from the store before it decided.
#[derive(Clone, Debug, Default)]
pub struct LaunchCheck {
    /// This GPU's record, after any stale markers were absorbed.
    pub verdict: Option<Verdict>,
    /// A marker left by an ended launch on this GPU was found just now.
    pub crash_found: bool,
    /// The launch Space's texture-limit record.
    pub space_verdict: Option<SpaceVerdict>,
}

const RECORDS_FILE: &str = "gpu_verdicts.json";
const MARKER_PREFIX: &str = "launch-";
const MARKER_SUFFIX: &str = ".json";
/// An empty marker younger than this may be a launch between creating its
/// file and taking its lock; older, it is debris from a crash in that gap.
const EMPTY_MARKER_GRACE_SECS: u64 = 60;

impl GuardStore {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `<local data>/Eustress/raytracing`, beside the engine's other per-user
    /// local data (`Eustress/telemetry`, `Eustress/keys`). Local, not
    /// roaming: a GPU record means nothing on another machine.
    pub fn default_location() -> Option<Self> {
        Some(Self::at(dirs::data_local_dir()?.join("Eustress").join("raytracing")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn marker_path(&self, owner: u32) -> PathBuf {
        self.dir.join(format!("{MARKER_PREFIX}{owner}{MARKER_SUFFIX}"))
    }

    pub fn load(&self) -> GuardRecords {
        fs::read_to_string(self.dir.join(RECORDS_FILE))
            .map(|text| GuardRecords::from_json(&text))
            .unwrap_or_default()
    }

    /// Written to a temporary file and renamed over the old one, so a crash
    /// mid-write leaves the previous records whole.
    pub fn save(&self, records: &GuardRecords) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(RECORDS_FILE);
        let tmp = self.dir.join(format!("{RECORDS_FILE}.tmp-{}", std::process::id()));
        fs::write(&tmp, records.to_json())?;
        fs::rename(&tmp, &path)
    }

    /// Load, change, and save only if `change` says it changed something.
    pub fn update(&self, change: impl FnOnce(&mut GuardRecords) -> bool) -> io::Result<bool> {
        let mut records = self.load();
        if !change(&mut records) {
            return Ok(false);
        }
        self.save(&records)?;
        Ok(true)
    }

    /// Take every marker whose process has ended: its lock is free. A marker
    /// whose lock is held is a live launch and is skipped. Each stale marker
    /// is removed while its lock is held, so two launches never both take it.
    pub fn collect_stale_markers(&self, now_unix: u64) -> Vec<LaunchMarker> {
        let mut stale = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return stale;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !(name.starts_with(MARKER_PREFIX) && name.ends_with(MARKER_SUFFIX)) {
                continue;
            }
            let path = entry.path();
            // A marker being removed by its owner cannot be opened (Windows:
            // delete pending), and a held lock means a live owner: skip both.
            let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            if file.try_lock().is_err() {
                continue;
            }
            let mut text = String::new();
            if (&file).read_to_string(&mut text).is_err() {
                continue;
            }
            match LaunchMarker::from_json(&text) {
                Some(marker) => {
                    let _ = fs::remove_file(&path);
                    drop(file);
                    stale.push(marker);
                }
                None => {
                    let old = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .is_some_and(|t| now_unix.saturating_sub(t.as_secs()) > EMPTY_MARKER_GRACE_SECS);
                    if text.trim().is_empty() && old {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        stale
    }

    /// Absorb the stale markers, then read this GPU's record.
    pub fn launch_check(&self, identity: &GpuIdentity, space: Option<&str>, now_unix: u64) -> io::Result<LaunchCheck> {
        let stale = self.collect_stale_markers(now_unix);
        let mut records = self.load();
        let recorded = absorb_stale_markers(&mut records, &stale, now_unix);
        if !recorded.is_empty() {
            self.save(&records)?;
        }
        Ok(LaunchCheck {
            verdict: records.verdict(identity).cloned(),
            crash_found: recorded.contains(&identity.key()),
            space_verdict: space.and_then(|key| records.space_verdict(key).cloned()),
        })
    }

    /// Put up this process's marker and hold its lock.
    pub fn arm(&self, identity: &GpuIdentity, now_unix: u64, reason: Option<String>) -> io::Result<MarkerHandle> {
        self.arm_as(std::process::id(), identity, now_unix, reason)
    }

    /// [`Self::arm`] under any owner number, so tests can stand in for
    /// several processes.
    pub fn arm_as(
        &self,
        owner: u32,
        identity: &GpuIdentity,
        now_unix: u64,
        reason: Option<String>,
    ) -> io::Result<MarkerHandle> {
        fs::create_dir_all(&self.dir)?;
        let path = self.marker_path(owner);
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
        // Blocking: only another launch's brief look can hold it.
        file.lock()?;
        let mut handle = MarkerHandle {
            path,
            file,
            marker: LaunchMarker { identity: identity.clone(), pid: owner, started_unix: now_unix, reason },
        };
        handle.write()?;
        Ok(handle)
    }
}

/// A marker this process holds. Dropping it without [`Self::clear`] leaves
/// the file behind with its lock released, which is how a crash looks.
#[derive(Debug)]
pub struct MarkerHandle {
    path: PathBuf,
    file: File,
    marker: LaunchMarker,
}

impl MarkerHandle {
    fn write(&mut self) -> io::Result<()> {
        self.file.set_len(0)?;
        let mut file = &self.file;
        io::Seek::seek(&mut file, io::SeekFrom::Start(0))?;
        file.write_all(self.marker.to_json().as_bytes())?;
        // On disk before anything ray traces: a driver fault can take the
        // whole machine down, not only this process.
        self.file.sync_all()
    }

    pub fn marker(&self) -> &LaunchMarker {
        &self.marker
    }

    pub fn set_reason(&mut self, reason: String) -> io::Result<()> {
        self.marker.reason = Some(reason);
        self.write()
    }

    /// Remove the marker. The file goes before the lock is released, so no
    /// other launch can take it in between and mistake it for a crash.
    pub fn clear(self) -> io::Result<()> {
        let removed = fs::remove_file(&self.path);
        drop(self.file);
        removed
    }
}

// ============================================================================
// The session
// ============================================================================

/// Frames a ray-traced session must run before its launch counts as safe:
/// ten seconds at 60 frames per second, by which time Solari has built its
/// pipelines and traced the scene.
pub const SURVIVAL_FRAMES: u32 = 600;

/// This process's ray-traced session, from the launch decision to exit.
#[derive(Debug)]
pub struct Session {
    store: GuardStore,
    identity: GpuIdentity,
    marker: Option<MarkerHandle>,
    failure: Option<String>,
    /// Whose marker this is: the process id.
    owner: u32,
    /// Ray tracing ended in this session (the live fallback): later render
    /// errors and panics happen in raster and say nothing about ray tracing.
    ended: bool,
}

impl Session {
    /// Put up the marker. Fails when the marker cannot be written.
    pub fn begin(store: GuardStore, identity: GpuIdentity, now_unix: u64) -> io::Result<Self> {
        Self::begin_as(store, std::process::id(), identity, now_unix)
    }

    /// [`Self::begin`] under any owner number (the process id in production),
    /// so tests can stand in for several processes with the same code.
    fn begin_as(store: GuardStore, owner: u32, identity: GpuIdentity, now_unix: u64) -> io::Result<Self> {
        let marker = store.arm_as(owner, &identity, now_unix, None)?;
        Ok(Self { store, identity, marker: Some(marker), failure: None, owner, ended: false })
    }

    pub fn identity(&self) -> &GpuIdentity {
        &self.identity
    }

    pub fn armed(&self) -> bool {
        self.marker.is_some()
    }

    pub fn failed(&self) -> bool {
        self.failure.is_some()
    }

    /// The launch got through [`SURVIVAL_FRAMES`]: take the marker down and
    /// clear a "crashed" record (an On run just showed it works now). Not
    /// after a failure: that marker must stay.
    pub fn survived(&mut self) -> io::Result<()> {
        if self.failure.is_some() {
            return Ok(());
        }
        if let Some(marker) = self.marker.take() {
            marker.clear()?;
        }
        let identity = self.identity.clone();
        self.store.update(|records| records.clear(&identity, VerdictKind::Crashed))?;
        Ok(())
    }

    /// A render error or a renderer panic: the marker goes back up (or
    /// stays) with the reason, and nothing later takes it down.
    pub fn fail(&mut self, reason: String, now_unix: u64) -> io::Result<()> {
        if self.ended {
            return Ok(());
        }
        if self.failure.is_none() {
            self.failure = Some(reason.clone());
        }
        match &mut self.marker {
            Some(marker) => marker.set_reason(reason),
            None => {
                self.marker = Some(self.store.arm_as(self.owner, &self.identity, now_unix, Some(reason))?);
                Ok(())
            }
        }
    }

    /// Ray tracing ended for this session without a failure (the live
    /// fallback): take the marker down, and leave the records alone, since
    /// the session did not run long enough to say anything about the GPU.
    pub fn end_tracing(&mut self) -> io::Result<()> {
        if self.failure.is_some() {
            return Ok(());
        }
        self.ended = true;
        match self.marker.take() {
            Some(marker) => marker.clear(),
            None => Ok(()),
        }
    }

    /// The app is closing normally: take the marker down, unless the
    /// session failed.
    pub fn clean_exit(&mut self) -> io::Result<()> {
        if self.failure.is_some() {
            return Ok(());
        }
        match self.marker.take() {
            Some(marker) => marker.clear(),
            None => Ok(()),
        }
    }

    pub fn record_too_slow(&self, detail: String, now_unix: u64) -> io::Result<bool> {
        let identity = self.identity.clone();
        let verdict = Verdict { kind: VerdictKind::TooSlow, detail, recorded_unix: now_unix };
        self.store.update(|records| records.record(&identity, verdict))
    }

    pub fn clear_too_slow(&self) -> io::Result<bool> {
        let identity = self.identity.clone();
        self.store.update(|records| records.clear(&identity, VerdictKind::TooSlow))
    }

    /// For a crash that never reached this process's own code (a test that
    /// stands in for one): drop the marker without removing its file.
    #[cfg(test)]
    fn abandon(mut self) {
        drop(self.marker.take());
    }
}

/// The one session of this process. A static, not a resource: the clean
/// exit is noticed in `main` after `App::run` has consumed the app, and the
/// panic hook and render error handler have no `World` of their own to look
/// in.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Start the session for a launch that is about to ray trace. The error is
/// for the startup log; the caller decides what Auto does without a marker.
pub fn begin_session(store: GuardStore, identity: GpuIdentity) -> Result<(), String> {
    let session = Session::begin(store, identity, now_unix())
        .map_err(|e| format!("could not write the ray-tracing crash marker: {e}"))?;
    if let Ok(mut slot) = SESSION.lock() {
        *slot = Some(session);
    }
    Ok(())
}

fn with_session<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    // `try_lock`: this runs from a panic hook too, where waiting on a lock
    // the panicking thread might hold would hang the crash.
    let mut slot = SESSION.try_lock().ok()?;
    slot.as_mut().map(f)
}

/// For `main`, once `App::run` has returned: a normal close takes the marker
/// down. After a render error the app also closes through here, and the
/// marker stays.
pub fn note_clean_exit() {
    with_session(|session| {
        if let Err(e) = session.clean_exit() {
            eprintln!("Ray tracing: could not remove the crash marker: {e}");
        }
    });
}

/// A render error or renderer panic while ray tracing.
pub fn note_render_failure(reason: String) {
    with_session(|session| {
        if let Err(e) = session.fail(reason, now_unix()) {
            eprintln!("Ray tracing: could not record the failure: {e}");
        }
    });
}

// ============================================================================
// Render errors and panics
// ============================================================================

/// What the marker says about a render error.
pub fn render_error_reason(kind: &str, description: &str) -> String {
    let description = description.trim();
    if description.is_empty() {
        format!("{kind} render error while ray tracing")
    } else {
        format!("{kind} render error while ray tracing: {description}")
    }
}

/// Bevy's default answer, after the failure is noted. The default handler
/// is built and called rather than copied, so it stays whatever Bevy makes it.
fn handle_render_error(error: &RenderError, main_world: &mut World, render_world: &mut World) -> RenderErrorPolicy {
    note_render_failure(render_error_reason(&format!("{:?}", error.ty), &error.description));
    (RenderErrorHandler::default().0)(error, main_world, render_world)
}

/// The crates a ray-traced frame runs through. A panic located in one of
/// them while ray tracing is on counts against ray tracing; a panic in a
/// script, a tool or the physics does not.
const RENDERER_PATHS: &[&str] = &[
    "bevy_solari",
    "bevy_render",
    "bevy_pbr",
    "bevy_core_pipeline",
    "bevy_anti_alias",
    "wgpu",
    "naga",
    "dlss",
    "/raytracing/",
];

pub fn panic_implicates_renderer(file: &str) -> bool {
    let file = file.replace('\\', "/");
    RENDERER_PATHS.iter().any(|part| file.contains(part))
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(location) = info.location() {
            if panic_implicates_renderer(location.file()) {
                note_render_failure(format!(
                    "panic at {}:{} while ray tracing",
                    location.file(),
                    location.line()
                ));
            }
        }
        previous(info);
    }));
}

// ============================================================================
// The performance guard
// ============================================================================

/// The performance guard's numbers. `EUSTRESS_RT_FRAME_BUDGET_MS` and
/// `EUSTRESS_RT_SLOW_WINDOW_SECS` tune the first two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerfBudget {
    /// A window whose median frame is over this is too slow. 33.3 ms is
    /// 30 frames per second, the floor below which an editor feels broken.
    pub frame_budget_ms: f32,
    /// Seconds of eligible frames per verdict.
    pub window_secs: f32,
    /// Seconds after loading stops before frames count: a Space streams in
    /// over several frames after the bulk load ends.
    pub settle_secs: f32,
    /// Seconds after the session starts before frames count: Solari builds
    /// its pipelines and acceleration structures first.
    pub warmup_secs: f32,
}

impl Default for PerfBudget {
    fn default() -> Self {
        Self { frame_budget_ms: 33.3, window_secs: 10.0, settle_secs: 5.0, warmup_secs: 10.0 }
    }
}

pub const ENV_FRAME_BUDGET_MS: &str = "EUSTRESS_RT_FRAME_BUDGET_MS";
pub const ENV_SLOW_WINDOW_SECS: &str = "EUSTRESS_RT_SLOW_WINDOW_SECS";

impl PerfBudget {
    /// The defaults, with any valid launch variable applied. A value that is
    /// not a positive number is ignored.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Self {
        let positive = |name: &str| {
            get(name)
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
        };
        let mut budget = Self::default();
        if let Some(ms) = positive(ENV_FRAME_BUDGET_MS) {
            budget.frame_budget_ms = ms;
        }
        if let Some(secs) = positive(ENV_SLOW_WINDOW_SECS) {
            budget.window_secs = secs;
        }
        budget
    }
}

/// One frame, as the guard sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSample {
    /// The real time this frame took.
    pub dt_ms: f32,
    /// Seconds since the ray-traced session's first frame.
    pub since_start_secs: f32,
    /// A Space is loading (`file_loader::bulk_load_active`).
    pub loading: bool,
    /// Focused, not minimized and at full speed (`PowerMode::Active`). The
    /// other power modes cap the frame rate on purpose (Quiet is 30 frames
    /// per second, exactly the budget), so their frames say nothing.
    pub interactive: bool,
    /// Edit mode: a running simulation's own cost is not ray tracing's.
    pub editing: bool,
    /// The editing frame-rate cap's interval (`IdleSettings::active_update_ms`,
    /// 0 when uncapped). A cap at or over the budget hides the GPU's speed.
    pub cap_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PerfOutcome {
    /// The window's median frame was over the budget.
    Slow { median_ms: f32 },
    /// At or under it.
    Fast { median_ms: f32 },
}

/// Collects a continuous stretch of eligible frames and judges it. An
/// ineligible frame starts the stretch over, so a verdict always rests on
/// one unbroken run of interactive editing.
#[derive(Clone, Debug)]
pub struct PerfGuard {
    budget: PerfBudget,
    window: Vec<f32>,
    window_secs: f32,
    last_loading_secs: Option<f32>,
    stopped: bool,
}

impl PerfGuard {
    pub fn new(budget: PerfBudget) -> Self {
        Self { budget, window: Vec::new(), window_secs: 0.0, last_loading_secs: None, stopped: false }
    }

    pub fn budget(&self) -> PerfBudget {
        self.budget
    }

    /// No more verdicts this session.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.restart();
    }

    fn restart(&mut self) {
        self.window.clear();
        self.window_secs = 0.0;
    }

    pub fn observe(&mut self, frame: FrameSample) -> Option<PerfOutcome> {
        if self.stopped {
            return None;
        }
        if frame.loading {
            self.last_loading_secs = Some(frame.since_start_secs);
            self.restart();
            return None;
        }
        let settled = self
            .last_loading_secs
            .is_none_or(|t| frame.since_start_secs - t >= self.budget.settle_secs);
        let eligible = frame.since_start_secs >= self.budget.warmup_secs
            && settled
            && frame.interactive
            && frame.editing
            && frame.cap_ms < self.budget.frame_budget_ms
            && frame.dt_ms.is_finite()
            && frame.dt_ms > 0.0;
        if !eligible {
            self.restart();
            return None;
        }
        self.window.push(frame.dt_ms);
        self.window_secs += frame.dt_ms / 1000.0;
        if self.window_secs < self.budget.window_secs {
            return None;
        }
        let median_ms = median(&mut self.window);
        self.restart();
        Some(if median_ms > self.budget.frame_budget_ms {
            PerfOutcome::Slow { median_ms }
        } else {
            PerfOutcome::Fast { median_ms }
        })
    }
}

/// The median, the mean of the middle two for an even count. Sorts in place.
pub fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// What a performance verdict does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfAction {
    /// Auto, too slow: record it for the next launch and tell the user once.
    RecordTooSlow,
    /// On, fast enough: an earlier "too slow" was wrong, or is out of date.
    ClearTooSlow,
    Nothing,
}

pub fn perf_action(mode: RayTracingMode, outcome: PerfOutcome) -> PerfAction {
    match (mode, outcome) {
        (RayTracingMode::Auto, PerfOutcome::Slow { .. }) => PerfAction::RecordTooSlow,
        (RayTracingMode::On, PerfOutcome::Fast { .. }) => PerfAction::ClearTooSlow,
        _ => PerfAction::Nothing,
    }
}

/// The notification for a "too slow" record. The first sentence becomes the
/// toast's title (`drain_manager_to_toasts` splits at the first ". ").
pub fn too_slow_message(median_ms: f32, budget_ms: f32) -> String {
    format!(
        "Ray tracing is too slow on this GPU. Frames took {median_ms:.0} ms against a {budget_ms:.0} ms budget. \
         It stays on until you close Eustress; from the next launch Auto draws without it. \
         To keep it, choose On in Settings > Graphics > Ray tracing."
    )
}

/// What the guards did this session, for the Settings note.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardEvents {
    /// This session recorded "too slow".
    pub slow_recorded: bool,
    /// Ray tracing stopped in this session, and why.
    pub fallback: Option<FallbackReason>,
}

#[derive(Resource)]
struct PerfWatch {
    guard: PerfGuard,
    mode: RayTracingMode,
    started_secs: Option<f32>,
    cleared: bool,
}

// ============================================================================
// Systems
// ============================================================================

/// The guards for a session that ray traces. Added by `RaytracingPlugin`
/// only then, so a raster session keeps Bevy's own error handler and hook.
pub(super) fn add_session_systems(app: &mut App, launch: &RaytracingLaunch) {
    app.insert_resource(RenderErrorHandler(handle_render_error))
        .insert_resource(PerfWatch {
            guard: PerfGuard::new(PerfBudget::from_env(|name| std::env::var(name).ok())),
            mode: launch.mode,
            started_secs: None,
            cleared: false,
        })
        .add_systems(
            Last,
            (count_survival_frames, watch_frame_budget).run_if(not(resource_exists::<RasterFallback>)),
        );
    install_panic_hook();
}

/// Whether the launch has now survived.
pub fn survival_due(frames: u32) -> bool {
    frames >= SURVIVAL_FRAMES
}

fn count_survival_frames(mut frames: Local<u32>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *frames += 1;
    if survival_due(*frames) {
        *done = true;
        with_session(|session| match session.survived() {
            Ok(()) => info!("Ray tracing: {SURVIVAL_FRAMES} frames drawn; launch marker removed"),
            Err(e) => warn!("Ray tracing: could not remove the launch marker: {e}"),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn watch_frame_budget(
    time: Res<Time<Real>>,
    focus: Option<Res<crate::window_focus::WindowFocusState>>,
    idle: Option<Res<crate::window_focus::IdleSettings>>,
    play: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut watch: ResMut<PerfWatch>,
    mut events: ResMut<GuardEvents>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    use crate::window_focus::PowerMode;
    let now = time.elapsed_secs();
    let started = *watch.started_secs.get_or_insert(now);
    let frame = FrameSample {
        dt_ms: time.delta_secs() * 1000.0,
        since_start_secs: now - started,
        loading: crate::space::file_loader::bulk_load_active(),
        interactive: focus.is_none_or(|f| f.focused && !f.minimized && f.power_mode == PowerMode::Active),
        editing: play.is_none_or(|s| *s.get() == crate::play_mode::PlayModeState::Editing),
        cap_ms: idle.map_or(0.0, |i| i.active_update_ms as f32),
    };
    let Some(outcome) = watch.guard.observe(frame) else {
        return;
    };
    let budget_ms = watch.guard.budget().frame_budget_ms;
    match perf_action(watch.mode, outcome) {
        PerfAction::RecordTooSlow => {
            let PerfOutcome::Slow { median_ms } = outcome else { return };
            watch.guard.stop();
            let detail = format!("median frame {median_ms:.1} ms over the {budget_ms:.1} ms budget");
            warn!("Ray tracing: too slow on this GPU ({detail}); Auto draws without it from the next launch");
            info!(
                "Ray tracing: a live fallback would now take Solari off every camera and empty the traced \
                 scene for the rest of this session; it is kept for the next launch, because changing the \
                 lighting mid-edit is worse than a slow session"
            );
            match with_session(|session| session.record_too_slow(detail, now_unix())) {
                Some(Ok(_)) => {}
                Some(Err(e)) => warn!("Ray tracing: could not save the too-slow record: {e}"),
                None => warn!("Ray tracing: no guard session; the too-slow record is not saved"),
            }
            events.slow_recorded = true;
            if let Some(mut notifications) = notifications {
                notifications.warning(too_slow_message(median_ms, budget_ms));
            }
        }
        PerfAction::ClearTooSlow => {
            if !watch.cleared {
                watch.cleared = true;
                if let Some(Ok(true)) = with_session(|session| session.clear_too_slow()) {
                    info!("Ray tracing: fast enough on this GPU now; its too-slow record is cleared");
                }
            }
        }
        PerfAction::Nothing => {
            if let PerfOutcome::Slow { median_ms } = outcome {
                // On: the person chose it. Say it once, then stop measuring.
                info!("Ray tracing (On): median frame {median_ms:.1} ms, over the {budget_ms:.1} ms budget");
                watch.guard.stop();
            }
        }
    }
}

// ============================================================================
// The texture limit and the live fallback
// ============================================================================

/// The textures Solari 0.19 can bind: `MAX_TEXTURE_COUNT` in bevy_solari's
/// scene/binder.rs, the length of its texture and sampler binding arrays.
/// Whenever anything is traced, its binder gathers the textures of EVERY
/// StandardMaterial (`material_assets.iter()`), traced or not, one entry per
/// distinct image; past this the bind group fails wgpu validation and Bevy's
/// render error handler quits the app.
pub const SOLARI_TEXTURE_LIMIT: usize = 5_000;

/// Distinct material textures at which a ray-traced session falls back to
/// raster, at once. Under the limit, because a load can add many images in
/// one frame.
pub const TEXTURE_FALLBACK_AT: usize = 4_800;

/// A Space recorded as over the limit is cleared when a session sees it
/// settled under this, so a Space that shrank is traced again.
pub const TEXTURE_CLEAR_BELOW: usize = 4_000;

/// Seconds without loading before a Space counts as settled.
pub const SPACE_SETTLE_SECS: f32 = 5.0;

pub fn texture_limit_reached(distinct: usize) -> bool {
    distinct >= TEXTURE_FALLBACK_AT
}

/// Whether a Space's over-limit record should go: it has one, it has been
/// settled for [`SPACE_SETTLE_SECS`], and it is well under the limit.
pub fn space_clear_due(has_verdict: bool, settled_secs: Option<f32>, distinct: usize) -> bool {
    has_verdict && settled_secs.is_some_and(|s| s >= SPACE_SETTLE_SECS) && distinct < TEXTURE_CLEAR_BELOW
}

/// The four textures of a StandardMaterial that Solari binds, in its order.
pub fn solari_textures(material: &StandardMaterial) -> [Option<AssetId<Image>>; 4] {
    [
        &material.base_color_texture,
        &material.normal_map_texture,
        &material.emissive_texture,
        &material.metallic_roughness_texture,
    ]
    .map(|texture| texture.as_ref().map(Handle::id))
}

/// The distinct images Solari's binder would bind, kept from material asset
/// events: a material's textures are counted when it appears or changes and
/// released when it changes or goes. Nothing walks every material, which
/// matters with 28,000 of them.
#[derive(Resource, Default, Debug)]
pub struct TextureCensus {
    materials: HashMap<AssetId<StandardMaterial>, [Option<AssetId<Image>>; 4]>,
    images: HashMap<AssetId<Image>, u32>,
    /// Material events read. A frame without material changes reads none.
    events: u64,
}

impl TextureCensus {
    pub fn distinct(&self) -> usize {
        self.images.len()
    }

    pub fn events(&self) -> u64 {
        self.events
    }

    pub fn set(&mut self, material: AssetId<StandardMaterial>, textures: [Option<AssetId<Image>>; 4]) {
        match self.materials.insert(material, textures) {
            Some(old) if old == textures => return,
            Some(old) => self.release(old),
            None => {}
        }
        for id in textures.into_iter().flatten() {
            *self.images.entry(id).or_default() += 1;
        }
    }

    pub fn remove(&mut self, material: AssetId<StandardMaterial>) {
        if let Some(old) = self.materials.remove(&material) {
            self.release(old);
        }
    }

    fn release(&mut self, textures: [Option<AssetId<Image>>; 4]) {
        for id in textures.into_iter().flatten() {
            if let Entry::Occupied(mut held) = self.images.entry(id) {
                *held.get_mut() -= 1;
                if *held.get() == 0 {
                    held.remove();
                }
            }
        }
    }
}

/// Why ray tracing stopped mid-session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    /// The materials reached this many distinct textures.
    TooManyTextures { distinct: usize },
    /// Settings > Graphics > Ray tracing: Off, which applies at once.
    TurnedOff,
    /// Auto, and DLSS Ray Reconstruction did not start although the device
    /// reported it (Bevy's `DlssPlugin` could not start DLSS, as when its
    /// libraries are missing beside the exe), so nothing would denoise
    /// Solari. Decided in `RaytracingPlugin::finish`, before the first frame.
    NoDenoiser,
}

impl FallbackReason {
    pub fn describe(&self) -> String {
        match self {
            Self::TooManyTextures { distinct } => format!(
                "the materials use {distinct} distinct textures; ray tracing can bind {SOLARI_TEXTURE_LIMIT}"
            ),
            Self::TurnedOff => "turned off in Settings".to_string(),
            Self::NoDenoiser => {
                "DLSS Ray Reconstruction did not start, and Auto ray traces only with a denoiser".to_string()
            }
        }
    }

    /// The one-time notification, when there is one (turning it off
    /// oneself needs no news). The first sentence is the toast's title.
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::TooManyTextures { distinct } => Some(format!(
                "Ray tracing is off for this session. This Space uses {distinct} distinct textures, close to \
                 the {SOLARI_TEXTURE_LIMIT} ray tracing can bind, so Eustress switched to standard lighting to \
                 keep running. It opens without ray tracing next time."
            )),
            // DLSS not starting changes nothing on screen: the session falls
            // back before its first frame.
            Self::TurnedOff | Self::NoDenoiser => None,
        }
    }
}

/// Ray tracing stopped in this session. Once present it stays: every tracing
/// system stops, and the fallback sweep keeps the traced scene empty.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct RasterFallback {
    pub reason: FallbackReason,
}

/// Asks for the live fallback. Settings > Graphics > Ray tracing: Off sends
/// it; nothing happens unless ray tracing is running.
#[derive(Message, Clone, Debug)]
pub struct RasterFallbackRequest(pub FallbackReason);

/// Where the guards keep their records: the default folder in the engine,
/// a temporary one in tests.
#[derive(Resource, Clone, Debug)]
pub struct GuardFolder(pub GuardStore);

/// Ray tracing ended for this session without a failure: the crash marker
/// comes down.
pub fn end_tracing_session() {
    with_session(|session| {
        if let Err(e) = session.end_tracing() {
            warn!("Ray tracing: could not remove the launch marker: {e}");
        }
    });
}

/// The census, the limit watch, Settings' Off, the announcement and the
/// Space-record clearing. Added by `RaytracingPlugin` in every session: the
/// census is what clears a Space record, and costs one map update per
/// material event.
pub(super) fn add_fallback_systems(app: &mut App) {
    app.init_resource::<TextureCensus>()
        .add_message::<RasterFallbackRequest>()
        .add_systems(
            Last,
            (count_material_textures, watch_texture_limit, handle_fallback_requests)
                .chain()
                .in_set(super::RaytracingSystems::Decide),
        )
        .add_systems(
            Last,
            (
                announce_raster_fallback.run_if(resource_added::<RasterFallback>),
                watch_space_verdict,
            )
                .in_set(super::RaytracingSystems::Fallback),
        );
}

pub(super) fn count_material_textures(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    mut census: ResMut<TextureCensus>,
) {
    for event in events.read() {
        census.events += 1;
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id } => {
                match materials.get(id) {
                    Some(material) => census.set(id, solari_textures(material)),
                    None => census.remove(id),
                }
            }
            AssetEvent::Removed { id } => census.remove(id),
            _ => {}
        }
    }
}

fn watch_texture_limit(
    census: Res<TextureCensus>,
    launch: Option<Res<RaytracingLaunch>>,
    fallback: Option<Res<RasterFallback>>,
    mut commands: Commands,
) {
    if fallback.is_some() || !launch.is_some_and(|l| l.status.is_active()) {
        return;
    }
    let distinct = census.distinct();
    if texture_limit_reached(distinct) {
        commands.insert_resource(RasterFallback { reason: FallbackReason::TooManyTextures { distinct } });
    }
}

fn handle_fallback_requests(
    mut requests: MessageReader<RasterFallbackRequest>,
    launch: Option<Res<RaytracingLaunch>>,
    fallback: Option<Res<RasterFallback>>,
    mut commands: Commands,
) {
    let Some(request) = requests.read().last() else {
        return;
    };
    if fallback.is_some() || !launch.is_some_and(|l| l.status.is_active()) {
        return;
    }
    commands.insert_resource(RasterFallback { reason: request.0.clone() });
}

/// Once, as the fallback begins: log it, tell the Settings note, take the
/// crash marker down, record the Space, and notify.
fn announce_raster_fallback(
    fallback: Res<RasterFallback>,
    space: Option<Res<crate::space::SpaceRoot>>,
    folder: Option<Res<GuardFolder>>,
    mut events: ResMut<GuardEvents>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let reason = &fallback.reason;
    warn!("Ray tracing: off for the rest of this session ({}); drawing with standard lighting", reason.describe());
    events.fallback = Some(reason.clone());
    end_tracing_session();
    if let FallbackReason::TooManyTextures { distinct } = *reason {
        match (space, folder) {
            (Some(space), Some(folder)) => {
                let key = space_key(&space.0);
                let verdict = SpaceVerdict { distinct_textures: distinct, recorded_unix: now_unix() };
                if let Err(e) = folder.0.update(|records| records.record_space(key, verdict)) {
                    warn!("Ray tracing: could not save this Space's texture record: {e}");
                }
            }
            _ => warn!("Ray tracing: no Space or guard folder; this Space's texture record is not saved"),
        }
    }
    if let (Some(text), Some(mut notifications)) = (reason.notice(), notifications) {
        notifications.warning(text);
    }
}

#[derive(Default)]
struct SpaceWatch {
    key: Option<String>,
    has_verdict: bool,
    settled_since: Option<f32>,
}

/// Clears the open Space's over-limit record once a session sees it settled
/// well under the limit, in any mode.
fn watch_space_verdict(
    time: Res<Time<Real>>,
    space: Option<Res<crate::space::SpaceRoot>>,
    folder: Option<Res<GuardFolder>>,
    census: Res<TextureCensus>,
    mut watch: Local<SpaceWatch>,
) {
    let (Some(space), Some(folder)) = (space, folder) else {
        return;
    };
    if space.is_changed() || watch.key.is_none() {
        let key = space_key(&space.0);
        watch.has_verdict = folder.0.load().space_verdict(&key).is_some();
        watch.key = Some(key);
        watch.settled_since = None;
    }
    if !watch.has_verdict {
        return;
    }
    let now = time.elapsed_secs();
    if crate::space::file_loader::bulk_load_active() {
        watch.settled_since = None;
        return;
    }
    let since = *watch.settled_since.get_or_insert(now);
    if space_clear_due(watch.has_verdict, Some(now - since), census.distinct()) {
        watch.has_verdict = false;
        let key = watch.key.clone().unwrap_or_default();
        match folder.0.update(|records| records.clear_space(&key)) {
            Ok(_) => info!(
                "Ray tracing: this Space has {} distinct textures now; its texture record is cleared",
                census.distinct()
            ),
            Err(e) => warn!("Ray tracing: could not clear this Space's texture record: {e}"),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use RayTracingMode::{Auto, Off, On};

    // ---- the switch

    #[test]
    fn launch_variable_wins_with_a_valid_value_only() {
        let saved_off = Some(r#"{"ray_tracing": "off"}"#);
        assert_eq!(requested_mode(Some("on"), saved_off), (On, SwitchSource::Env));
        assert_eq!(requested_mode(Some(" AUTO "), saved_off), (Auto, SwitchSource::Env));
        assert_eq!(requested_mode(Some("off"), Some(r#"{"ray_tracing": "on"}"#)), (Off, SwitchSource::Env));
        // Phase 1 spellings.
        assert_eq!(requested_mode(Some("1"), None), (On, SwitchSource::Env));
        assert_eq!(requested_mode(Some("true"), None), (On, SwitchSource::Env));
        assert_eq!(requested_mode(Some("0"), None), (Off, SwitchSource::Env));
        assert_eq!(requested_mode(Some("false"), None), (Off, SwitchSource::Env));
        // Empty or unknown: the Settings choice decides.
        assert_eq!(requested_mode(Some(""), saved_off), (Off, SwitchSource::Settings));
        assert_eq!(requested_mode(Some("maybe"), saved_off), (Off, SwitchSource::Settings));
        assert!(ignored_env_value(Some("maybe")).is_some());
        assert!(ignored_env_value(Some("")).is_none());
        assert!(ignored_env_value(Some("on")).is_none());
        assert!(ignored_env_value(None).is_none());
    }

    #[test]
    fn settings_value_and_the_phase_1_boolean() {
        let read = |json: &str| requested_mode(None, Some(json));
        assert_eq!(read(r#"{"ray_tracing": "auto"}"#), (Auto, SwitchSource::Settings));
        assert_eq!(read(r#"{"ray_tracing": "on"}"#), (On, SwitchSource::Settings));
        assert_eq!(read(r#"{"ray_tracing": "off"}"#), (Off, SwitchSource::Settings));
        assert_eq!(read(r#"{"ray_tracing": true}"#), (On, SwitchSource::Settings));
        // Phase 1 wrote false on every save: no choice, so Auto.
        assert_eq!(read(r#"{"ray_tracing": false}"#), (Auto, SwitchSource::Settings));
        assert_eq!(read(r#"{"snap_size": 1.0}"#), (Auto, SwitchSource::Default));
        assert_eq!(read(r#"{"ray_tracing": 3}"#), (Auto, SwitchSource::Default));
        assert_eq!(read("not json"), (Auto, SwitchSource::Default));
        assert_eq!(requested_mode(None, None), (Auto, SwitchSource::Default));
    }

    #[test]
    fn saved_field_round_trips_and_never_fails() {
        for mode in [Auto, On, Off] {
            let text = serde_json::to_string(&mode).unwrap();
            assert_eq!(text, format!("\"{}\"", mode.as_str()));
            assert_eq!(serde_json::from_str::<RayTracingMode>(&text).unwrap(), mode);
            assert_eq!(RayTracingMode::from_index(mode.index()), Some(mode));
        }
        assert_eq!(serde_json::from_str::<RayTracingMode>("true").unwrap(), On);
        assert_eq!(serde_json::from_str::<RayTracingMode>("false").unwrap(), Auto);
        for odd in ["null", "7", "{}", "[1]", "\"sometimes\""] {
            assert_eq!(serde_json::from_str::<RayTracingMode>(odd).unwrap(), Auto, "{odd}");
        }
        assert_eq!(RayTracingMode::from_index(3), None);
    }

    // ---- the decision table

    #[test]
    fn decision_table() {
        use RaytracingStatus as S;
        use VerdictKind::{Crashed, TooSlow};
        // A `dlss` build on an RTX GPU whose driver offers Ray Reconstruction.
        let rtx = Some(Gate { supported: true, software: false, denoiser: true });
        // The same GPU with no denoiser: a build without DLSS, or a driver
        // without Ray Reconstruction.
        let raw_rtx = Some(Gate { supported: true, software: false, denoiser: false });
        let lavapipe = Some(Gate { supported: true, software: true, denoiser: false });
        let pascal = Some(Gate { supported: false, software: false, denoiser: false });
        #[rustfmt::skip]
        let table: &[(RayTracingMode, bool, Option<Gate>, Option<VerdictKind>, RaytracingStatus)] = &[
            // Off wins over everything.
            (Off,  true,  rtx,      None,          S::Off),
            (Off,  false, None,     Some(Crashed), S::Off),
            (Off,  true,  raw_rtx,  None,          S::Off),
            // The build, then the hardware, for Auto and On alike.
            (Auto, false, rtx,      None,          S::NotInBuild),
            (On,   false, rtx,      None,          S::NotInBuild),
            (Auto, true,  pascal,   None,          S::Unsupported),
            (On,   true,  pascal,   None,          S::Unsupported),
            (Auto, true,  None,     None,          S::Unsupported),
            (On,   true,  None,     None,          S::Unsupported),
            // Auto heeds the software adapter and both guards.
            (Auto, true,  rtx,      None,          S::Active),
            (Auto, true,  lavapipe, None,          S::SoftwareGpu),
            (Auto, true,  rtx,      Some(Crashed), S::AfterCrash),
            (Auto, true,  rtx,      Some(TooSlow), S::TooSlow),
            (Auto, true,  lavapipe, Some(Crashed), S::SoftwareGpu),
            // Auto traces only with a denoiser, whatever the guards recorded.
            (Auto, true,  raw_rtx,  None,          S::NoDenoiser),
            (Auto, true,  raw_rtx,  Some(Crashed), S::NoDenoiser),
            (Auto, true,  raw_rtx,  Some(TooSlow), S::NoDenoiser),
            // On forces it past both.
            (On,   true,  rtx,      Some(Crashed), S::Active),
            (On,   true,  rtx,      Some(TooSlow), S::Active),
            (On,   true,  lavapipe, None,          S::Active),
            (On,   true,  raw_rtx,  None,          S::Active),
        ];
        for &(mode, compiled, gate, verdict, want) in table {
            assert_eq!(
                RaytracingStatus::decide(mode, compiled, gate, verdict),
                want,
                "{mode:?} compiled={compiled} gate={gate:?} verdict={verdict:?}"
            );
        }
    }

    #[test]
    fn auto_traces_only_with_a_denoiser_and_on_without_one() {
        use RaytracingStatus as S;
        let mut rtx = GpuCapabilities { adapter: "NVIDIA GeForce RTX 2080 Ti".into(), ..Default::default() };
        rtx.set_features(super::super::required_features());
        let with = Some(Gate::of(&rtx, true));
        let without = Some(Gate::of(&rtx, false));
        let no_denoiser = RaytracingStatus::decide(Auto, true, without, None);
        assert_eq!(no_denoiser, S::NoDenoiser);
        assert!(!no_denoiser.is_active(), "no denoiser, yet the launch ray traces");
        assert_eq!(
            no_denoiser.after_space_verdict(Auto, false).after_arming(Auto, true),
            S::NoDenoiser,
            "a later step turned it back on"
        );
        assert_eq!(RaytracingStatus::decide(Auto, true, with, None), S::Active);
        assert_eq!(RaytracingStatus::decide(On, true, without, None), S::Active, "On traces denoised or not");
        assert_eq!(RaytracingStatus::decide(Off, true, without, None), S::Off);
        assert_eq!(RaytracingStatus::decide(Off, true, with, None), S::Off);
    }

    #[test]
    fn auto_needs_a_marker_and_on_does_not() {
        use RaytracingStatus as S;
        assert_eq!(S::Active.after_arming(Auto, true), S::Active);
        assert_eq!(S::Active.after_arming(Auto, false), S::NoGuard);
        assert_eq!(S::Active.after_arming(On, false), S::Active);
        assert_eq!(S::Unsupported.after_arming(Auto, false), S::Unsupported);
    }

    // ---- records

    fn gpu(driver_info: &str) -> GpuIdentity {
        GpuIdentity {
            adapter: "NVIDIA GeForce RTX 2080 Ti".into(),
            vendor_id: 0x10DE,
            device_id: 0x1E07,
            driver: "NVIDIA".into(),
            driver_info: driver_info.into(),
            backend: "Vulkan".into(),
        }
    }

    fn verdict(kind: VerdictKind) -> Verdict {
        Verdict { kind, detail: "x".into(), recorded_unix: 1 }
    }

    #[test]
    fn identity_change_is_a_fresh_start() {
        let mut records = GuardRecords::default();
        records.record(&gpu("581.57"), verdict(VerdictKind::Crashed));
        assert!(records.verdict(&gpu("581.57")).is_some());
        assert!(records.verdict(&gpu("582.53")).is_none(), "a driver update kept the record");
        let other_card = GpuIdentity { device_id: 0x2204, ..gpu("581.57") };
        assert!(records.verdict(&other_card).is_none());
        let other_api = GpuIdentity { backend: "Dx12".into(), ..gpu("581.57") };
        assert!(records.verdict(&other_api).is_none());
        let renamed = GpuIdentity { adapter: "NVIDIA GeForce RTX 3090".into(), ..gpu("581.57") };
        assert!(records.verdict(&renamed).is_none());
    }

    #[test]
    fn crash_outranks_too_slow_and_clear_takes_only_its_kind() {
        let id = gpu("581.57");
        let mut records = GuardRecords::default();
        assert!(records.record(&id, verdict(VerdictKind::Crashed)));
        assert!(!records.record(&id, verdict(VerdictKind::TooSlow)), "too slow replaced a crash");
        assert_eq!(records.verdict(&id).unwrap().kind, VerdictKind::Crashed);
        assert!(!records.clear(&id, VerdictKind::TooSlow));
        assert!(records.clear(&id, VerdictKind::Crashed));
        assert!(records.is_empty());
        assert!(records.record(&id, verdict(VerdictKind::TooSlow)));
        assert!(records.record(&id, verdict(VerdictKind::Crashed)), "a crash did not replace too slow");
    }

    #[test]
    fn records_round_trip_and_bad_entries_are_dropped() {
        let mut records = GuardRecords::default();
        records.record(&gpu("581.57"), Verdict { kind: VerdictKind::TooSlow, detail: "41 ms".into(), recorded_unix: 7 });
        records.record(&gpu("582.53"), verdict(VerdictKind::Crashed));
        let text = records.to_json();
        assert_eq!(GuardRecords::from_json(&text), records);
        let mut value: Value = serde_json::from_str(&text).unwrap();
        value["gpus"]["junk"] = json!({ "identity": { "adapter": 5 }, "verdict": "crashed" });
        value["gpus"]["odd"] = json!({ "identity": gpu("1").to_json(), "verdict": "melted" });
        assert_eq!(GuardRecords::from_json(&value.to_string()), records);
        assert!(GuardRecords::from_json("{").is_empty());
        assert!(GuardRecords::from_json("[]").is_empty());
    }

    #[test]
    fn stale_markers_become_crash_records() {
        let mut records = GuardRecords::default();
        let marker = LaunchMarker { identity: gpu("581.57"), pid: 4, started_unix: 3, reason: None };
        let failed = LaunchMarker { reason: Some("DeviceLost render error".into()), ..marker.clone() };
        assert_eq!(absorb_stale_markers(&mut records, &[marker], 10), vec![gpu("581.57").key()]);
        let v = records.verdict(&gpu("581.57")).unwrap();
        assert_eq!((v.kind, v.recorded_unix), (VerdictKind::Crashed, 10));
        assert!(v.detail.contains("600 frames"), "{}", v.detail);
        let mut records = GuardRecords::default();
        absorb_stale_markers(&mut records, &[failed], 10);
        assert_eq!(records.verdict(&gpu("581.57")).unwrap().detail, "DeviceLost render error");
        assert_eq!(absorb_stale_markers(&mut records, &[], 10), Vec::<String>::new());
    }

    // ---- the store, on disk

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("eustress_rt_guard_{}_{}_{}", name, std::process::id(), now_unix()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn store(&self) -> GuardStore {
            GuardStore::at(&self.0)
        }
        fn markers(&self) -> usize {
            fs::read_dir(&self.0)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(MARKER_PREFIX))
                .count()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn crash_at_launch_turns_auto_off_on_that_gpu_only() {
        let dir = TempDir::new("crash");
        let id = gpu("581.57");
        // A launch arms, then its process dies before surviving.
        Session::begin_as(dir.store(), 101, id.clone(), 5).unwrap().abandon();
        assert_eq!(dir.markers(), 1);
        let check = dir.store().launch_check(&id, None, 9).unwrap();
        assert!(check.crash_found);
        assert_eq!(check.verdict.as_ref().map(|v| v.kind), Some(VerdictKind::Crashed));
        assert_eq!(dir.markers(), 0, "the stale marker was not taken");
        let gate = Gate { supported: true, software: false, denoiser: true };
        let status = RaytracingStatus::decide(Auto, true, Some(gate), check.verdict.map(|v| v.kind));
        assert_eq!(status, RaytracingStatus::AfterCrash);
        // The next launch still knows, but did not find the crash itself.
        let again = dir.store().launch_check(&id, None, 12).unwrap();
        assert!(!again.crash_found);
        assert!(again.verdict.is_some());
        // A driver update is a fresh start.
        assert!(dir.store().launch_check(&gpu("582.53"), None, 12).unwrap().verdict.is_none());
    }

    #[test]
    fn a_crash_on_one_gpu_is_recorded_for_that_gpu_only() {
        let dir = TempDir::new("other_gpu");
        Session::begin_as(dir.store(), 151, gpu("581.57"), 5).unwrap().abandon();
        // The next launch runs on a newer driver: it absorbs the old marker
        // under the old identity, and its own GPU starts clean.
        let check = dir.store().launch_check(&gpu("582.53"), None, 9).unwrap();
        assert!(!check.crash_found, "a crash on another GPU identity was reported as this one's");
        assert!(check.verdict.is_none());
        assert_eq!(dir.store().load().verdict(&gpu("581.57")).map(|v| v.kind), Some(VerdictKind::Crashed));
    }

    #[test]
    fn a_live_launch_is_not_a_crash() {
        let dir = TempDir::new("live");
        let id = gpu("581.57");
        let live = Session::begin_as(dir.store(), 202, id.clone(), 5).unwrap();
        let check = dir.store().launch_check(&id, None, 6).unwrap();
        assert!(!check.crash_found, "another window still starting was taken for a crash");
        assert!(check.verdict.is_none());
        assert_eq!(dir.markers(), 1, "a live marker was removed");
        drop(live);
    }

    #[test]
    fn survival_and_clean_exit_take_the_marker_down() {
        let dir = TempDir::new("survive");
        let id = gpu("581.57");
        dir.store().update(|r| r.record(&id, verdict(VerdictKind::Crashed))).unwrap();
        let mut session = Session::begin_as(dir.store(), 303, id.clone(), 5).unwrap();
        session.survived().unwrap();
        assert_eq!(dir.markers(), 0);
        assert!(dir.store().load().verdict(&id).is_none(), "an On run that survived kept the crash record");
        session.clean_exit().unwrap();
        // A session that closes before surviving clears its marker too.
        let mut short = Session::begin_as(dir.store(), 304, id.clone(), 5).unwrap();
        short.clean_exit().unwrap();
        assert_eq!(dir.markers(), 0);
        assert!(!dir.store().launch_check(&id, None, 9).unwrap().crash_found);
    }

    #[test]
    fn render_error_after_survival_leaves_a_marker_for_the_next_launch() {
        let dir = TempDir::new("render_error");
        let id = gpu("581.57");
        let mut session = Session::begin_as(dir.store(), 404, id.clone(), 5).unwrap();
        session.survived().unwrap();
        assert_eq!(dir.markers(), 0);
        let reason = render_error_reason("DeviceLost", "Device lost while tracing");
        session.fail(reason.clone(), 50).unwrap();
        assert!(session.failed());
        // Bevy's handler then sends AppExit, and main notes a clean exit.
        session.clean_exit().unwrap();
        session.survived().unwrap();
        assert_eq!(dir.markers(), 1, "the failure's marker was taken down");
        session.abandon();
        let check = dir.store().launch_check(&id, None, 60).unwrap();
        assert!(check.crash_found);
        assert_eq!(check.verdict.unwrap().detail, reason);
    }

    #[test]
    fn render_error_before_survival_keeps_the_first_marker() {
        let dir = TempDir::new("early_error");
        let id = gpu("581.57");
        let mut session = Session::begin_as(dir.store(), 505, id.clone(), 5).unwrap();
        session.fail("Validation render error while ray tracing".into(), 6).unwrap();
        session.clean_exit().unwrap();
        assert_eq!(dir.markers(), 1);
        session.abandon();
        let check = dir.store().launch_check(&id, None, 7).unwrap();
        assert_eq!(check.verdict.unwrap().detail, "Validation render error while ray tracing");
    }

    #[test]
    fn too_slow_is_saved_and_cleared_per_gpu() {
        let dir = TempDir::new("slow");
        let id = gpu("581.57");
        let session = Session::begin_as(dir.store(), 606, id.clone(), 5).unwrap();
        assert!(session.record_too_slow("41 ms".into(), 9).unwrap());
        assert_eq!(dir.store().load().verdict(&id).unwrap().kind, VerdictKind::TooSlow);
        assert!(dir.store().load().verdict(&gpu("582.53")).is_none());
        assert!(session.clear_too_slow().unwrap());
        assert!(dir.store().load().verdict(&id).is_none());
        assert!(!session.clear_too_slow().unwrap(), "nothing to clear still wrote");
    }

    #[test]
    fn empty_marker_is_left_while_it_may_be_starting() {
        let dir = TempDir::new("empty");
        fs::write(dir.0.join("launch-707.json"), "").unwrap();
        assert!(dir.store().collect_stale_markers(now_unix()).is_empty());
        assert_eq!(dir.markers(), 1, "a fresh empty marker was removed");
        assert!(dir.store().collect_stale_markers(now_unix() + 3600).is_empty());
        assert_eq!(dir.markers(), 0, "an old empty marker was kept");
    }

    // ---- panics

    #[test]
    fn panics_in_the_renderer_count_and_others_do_not() {
        for file in [
            r"C:\Users\x\.cargo\registry\src\index.crates.io-1\bevy_solari-0.19.0\src\scene\blas.rs",
            "/home/x/.cargo/registry/src/i/wgpu-core-29.0.4/src/device/mod.rs",
            r"crates\engine\src\raytracing\sky.rs",
            "C:/x/bevy_render-0.19.0/src/renderer/mod.rs",
            "C:/x/dlss_wgpu-4.0.0/src/lib.rs",
        ] {
            assert!(panic_implicates_renderer(file), "{file}");
        }
        for file in [
            r"crates\engine\src\play_mode.rs",
            "crates/common/src/scripting/luau.rs",
            "C:/x/avian3d-0.5.0/src/lib.rs",
            "crates/engine/src/raytracing_notes.rs",
        ] {
            assert!(!panic_implicates_renderer(file), "{file}");
        }
    }

    #[test]
    fn render_error_reason_reads_well() {
        assert_eq!(render_error_reason("DeviceLost", ""), "DeviceLost render error while ray tracing");
        assert_eq!(
            render_error_reason("Validation", " bad bind group "),
            "Validation render error while ray tracing: bad bind group"
        );
    }

    #[test]
    fn survival_after_exactly_the_frames() {
        assert!(!survival_due(SURVIVAL_FRAMES - 1));
        assert!(survival_due(SURVIVAL_FRAMES));
    }

    // ---- the performance guard

    fn interactive(since: f32, dt_ms: f32) -> FrameSample {
        FrameSample { dt_ms, since_start_secs: since, loading: false, interactive: true, editing: true, cap_ms: 8.0 }
    }

    /// Feed `secs` of frames of `dt_ms` from `start`; the last outcome.
    fn run(guard: &mut PerfGuard, start: f32, secs: f32, dt_ms: f32) -> (Option<PerfOutcome>, f32) {
        let mut t = start;
        let mut last = None;
        while t < start + secs {
            t += dt_ms / 1000.0;
            if let Some(outcome) = guard.observe(interactive(t, dt_ms)) {
                last = Some(outcome);
            }
        }
        (last, t)
    }

    #[test]
    fn slow_and_fast_windows() {
        let mut guard = PerfGuard::new(PerfBudget::default());
        let (outcome, _) = run(&mut guard, 10.0, 10.5, 50.0);
        assert!(matches!(outcome, Some(PerfOutcome::Slow { median_ms }) if (median_ms - 50.0).abs() < 0.01));
        let mut guard = PerfGuard::new(PerfBudget::default());
        let (outcome, _) = run(&mut guard, 10.0, 10.5, 16.0);
        assert!(matches!(outcome, Some(PerfOutcome::Fast { .. })));
        // Exactly at the budget is not over it.
        let mut guard = PerfGuard::new(PerfBudget { frame_budget_ms: 40.0, ..PerfBudget::default() });
        let (outcome, _) = run(&mut guard, 10.0, 10.5, 40.0);
        assert!(matches!(outcome, Some(PerfOutcome::Fast { .. })));
    }

    #[test]
    fn median_ignores_spikes() {
        let mut guard = PerfGuard::new(PerfBudget::default());
        let mut t = 10.0;
        let mut outcome = None;
        // 16 ms frames with a 900 ms shader-compile spike every tenth frame.
        for i in 0..2000 {
            let dt = if i % 10 == 0 { 900.0 } else { 16.0 };
            t += dt / 1000.0;
            outcome = outcome.or(guard.observe(interactive(t, dt)));
        }
        assert!(matches!(outcome, Some(PerfOutcome::Fast { .. })), "{outcome:?}");
    }

    #[test]
    fn no_verdict_before_warmup_or_while_loading_or_settling() {
        // Both longer than the 10 s window, so a guard that skipped either
        // would have judged a window inside it.
        let mut guard = PerfGuard::new(PerfBudget { warmup_secs: 20.0, settle_secs: 15.0, ..PerfBudget::default() });
        // Warm-up: slow frames count for nothing.
        let (outcome, t) = run(&mut guard, 0.0, 19.9, 50.0);
        assert_eq!(outcome, None);
        // Loading, then settling: still nothing.
        assert_eq!(guard.observe(FrameSample { loading: true, ..interactive(t, 50.0) }), None);
        let (outcome, t) = run(&mut guard, t, 14.9, 50.0);
        assert_eq!(outcome, None);
        // Then a full window.
        let (outcome, _) = run(&mut guard, t, 10.5, 50.0);
        assert!(matches!(outcome, Some(PerfOutcome::Slow { .. })));
    }

    #[test]
    fn throttled_frames_restart_the_window() {
        let budget = PerfBudget::default();
        for odd in [
            |f: FrameSample| FrameSample { interactive: false, ..f },
            |f: FrameSample| FrameSample { editing: false, ..f },
            |f: FrameSample| FrameSample { cap_ms: 33.3, ..f },
        ] {
            let mut guard = PerfGuard::new(budget);
            let mut t = 10.0;
            let mut outcome = None;
            // 20 s of slow frames, every 100th one ineligible: no window
            // ever completes unbroken.
            for i in 0..400 {
                t += 0.05;
                let frame = interactive(t, 50.0);
                let frame = if i % 100 == 99 { odd(frame) } else { frame };
                outcome = outcome.or(guard.observe(frame));
            }
            assert_eq!(outcome, None);
        }
        // A cap below the budget does not hide anything.
        let mut guard = PerfGuard::new(budget);
        let mut t = 10.0;
        let mut outcome = None;
        for _ in 0..400 {
            t += 0.05;
            outcome = outcome.or(guard.observe(FrameSample { cap_ms: 16.7, ..interactive(t, 50.0) }));
        }
        assert!(matches!(outcome, Some(PerfOutcome::Slow { .. })));
    }

    #[test]
    fn a_stopped_guard_says_nothing_more() {
        let mut guard = PerfGuard::new(PerfBudget::default());
        guard.stop();
        assert_eq!(run(&mut guard, 10.0, 30.0, 50.0).0, None);
    }

    #[test]
    fn budget_from_launch_variables() {
        let get = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
        };
        assert_eq!(PerfBudget::from_env(get(&[])), PerfBudget::default());
        let tuned = PerfBudget::from_env(get(&[(ENV_FRAME_BUDGET_MS, "16.7"), (ENV_SLOW_WINDOW_SECS, " 20 ")]));
        assert_eq!((tuned.frame_budget_ms, tuned.window_secs), (16.7, 20.0));
        for bad in ["-5", "0", "NaN", "inf", "fast"] {
            let pairs: &'static [(&'static str, &'static str)] = Box::leak(Box::new([(ENV_FRAME_BUDGET_MS, bad)]));
            assert_eq!(PerfBudget::from_env(get(pairs)).frame_budget_ms, 33.3, "{bad}");
        }
    }

    #[test]
    fn what_each_verdict_does() {
        let slow = PerfOutcome::Slow { median_ms: 50.0 };
        let fast = PerfOutcome::Fast { median_ms: 10.0 };
        assert_eq!(perf_action(Auto, slow), PerfAction::RecordTooSlow);
        assert_eq!(perf_action(Auto, fast), PerfAction::Nothing);
        assert_eq!(perf_action(On, slow), PerfAction::Nothing);
        assert_eq!(perf_action(On, fast), PerfAction::ClearTooSlow);
        assert_eq!(perf_action(Off, slow), PerfAction::Nothing);
        let message = too_slow_message(48.4, 33.3);
        assert!(message.starts_with("Ray tracing is too slow on this GPU. "));
        assert!(message.contains("48 ms") && message.contains("33 ms") && message.contains("On"));
    }

    // ---- the texture limit and the per-Space record

    fn image(n: u128) -> Option<AssetId<Image>> {
        Some(Handle::<Image>::from(bevy::asset::uuid::Uuid::from_u128(n + 1)).id())
    }

    fn material_id(n: u128) -> AssetId<StandardMaterial> {
        Handle::<StandardMaterial>::from(bevy::asset::uuid::Uuid::from_u128(n + 1)).id()
    }

    #[test]
    fn fallback_threshold_sits_under_solaris_limit() {
        assert!(TEXTURE_FALLBACK_AT < SOLARI_TEXTURE_LIMIT);
        assert!(TEXTURE_CLEAR_BELOW < TEXTURE_FALLBACK_AT);
        assert!(!texture_limit_reached(TEXTURE_FALLBACK_AT - 1));
        assert!(texture_limit_reached(TEXTURE_FALLBACK_AT));
        assert!(texture_limit_reached(SOLARI_TEXTURE_LIMIT + 10));
    }

    #[test]
    fn census_counts_distinct_images_as_solari_does() {
        let mut census = TextureCensus::default();
        // Two materials on one image: one entry, as Solari's push_if_absent.
        census.set(material_id(1), [image(10), None, None, None]);
        census.set(material_id(2), [image(10), None, None, None]);
        assert_eq!(census.distinct(), 1);
        // The same image in two slots of one material: still one.
        census.set(material_id(3), [image(11), image(11), None, None]);
        assert_eq!(census.distinct(), 2);
        // All four slots count.
        census.set(material_id(4), [image(20), image(21), image(22), image(23)]);
        assert_eq!(census.distinct(), 6);
        // A changed material releases what it no longer uses.
        census.set(material_id(4), [image(20), None, None, None]);
        assert_eq!(census.distinct(), 3);
        // Setting the same textures again changes nothing.
        census.set(material_id(4), [image(20), None, None, None]);
        assert_eq!(census.distinct(), 3);
        // A shared image stays while any material holds it.
        census.remove(material_id(1));
        assert_eq!(census.distinct(), 3);
        census.remove(material_id(2));
        assert_eq!(census.distinct(), 2);
        census.remove(material_id(2));
        census.remove(material_id(3));
        census.remove(material_id(4));
        assert_eq!(census.distinct(), 0);
    }

    #[test]
    fn census_follows_material_events_only() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_resource::<TextureCensus>()
            .add_systems(Last, count_material_textures);
        let shared = Handle::<Image>::from(bevy::asset::uuid::Uuid::from_u128(7));
        let mut handles = Vec::new();
        for n in 0..50u128 {
            let own = Handle::<Image>::from(bevy::asset::uuid::Uuid::from_u128(1000 + n));
            let material = StandardMaterial {
                base_color_texture: Some(own),
                normal_map_texture: Some(shared.clone()),
                ..default()
            };
            handles.push(app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(material));
        }
        app.update();
        assert_eq!(app.world().resource::<TextureCensus>().distinct(), 51);
        // Quiet frames read nothing: no material is walked again.
        let read = app.world().resource::<TextureCensus>().events();
        app.update();
        app.update();
        assert_eq!(app.world().resource::<TextureCensus>().events(), read);
        // An edit to one material is one event.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&handles[0]).unwrap().base_color_texture = None;
        app.update();
        assert_eq!(app.world().resource::<TextureCensus>().distinct(), 50);
        assert_eq!(app.world().resource::<TextureCensus>().events(), read + 1);
        // A removed material releases its textures.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().remove(&handles[1]);
        app.update();
        assert_eq!(app.world().resource::<TextureCensus>().distinct(), 49);
    }

    #[test]
    fn space_records_round_trip_and_keys_ignore_case_and_slashes() {
        let mut records = GuardRecords::default();
        records.record(&gpu("581.57"), verdict(VerdictKind::Crashed));
        let a = space_key(Path::new(r"C:\Users\X\Documents\Eustress\Games\Spaces\VS2\"));
        // Another spelling of the same folder: case and slashes differ.
        let b = space_key(Path::new("c:/users/x/documents/eustress/games/spaces/vs2"));
        if cfg!(windows) {
            assert_eq!(a, b);
        }
        assert!(!a.ends_with('/') && !a.contains('\\'));
        assert!(records.record_space(a.clone(), SpaceVerdict { distinct_textures: 4_812, recorded_unix: 3 }));
        assert!(!records.record_space(a.clone(), SpaceVerdict { distinct_textures: 4_812, recorded_unix: 3 }));
        let text = records.to_json();
        let back = GuardRecords::from_json(&text);
        assert_eq!(back, records);
        assert_eq!(back.space_verdict(&a).map(|v| v.distinct_textures), Some(4_812));
        assert!(back.space_verdict("c:/elsewhere").is_none());
        let mut back = back;
        assert!(back.clear_space(&a));
        assert!(!back.clear_space(&a));
    }

    #[test]
    fn auto_opens_a_space_over_the_limit_without_ray_tracing() {
        use RaytracingStatus as S;
        assert_eq!(S::Active.after_space_verdict(Auto, true), S::SpaceTooManyTextures);
        assert_eq!(S::Active.after_space_verdict(Auto, false), S::Active);
        assert_eq!(S::Active.after_space_verdict(On, true), S::Active, "On tries it anyway");
        assert_eq!(S::Unsupported.after_space_verdict(Auto, true), S::Unsupported);
        // From the store, at launch.
        let dir = TempDir::new("space");
        let key = space_key(Path::new("C:/Spaces/Heavy"));
        dir.store()
            .update(|r| r.record_space(key.clone(), SpaceVerdict { distinct_textures: 4_900, recorded_unix: 1 }))
            .unwrap();
        let check = dir.store().launch_check(&gpu("581.57"), Some(&key), 2).unwrap();
        assert_eq!(check.space_verdict.map(|v| v.distinct_textures), Some(4_900));
        let other = dir.store().launch_check(&gpu("581.57"), Some("c:/spaces/light"), 2).unwrap();
        assert!(other.space_verdict.is_none());
    }

    #[test]
    fn a_space_record_clears_only_settled_and_well_under() {
        assert!(space_clear_due(true, Some(SPACE_SETTLE_SECS), TEXTURE_CLEAR_BELOW - 1));
        assert!(!space_clear_due(true, Some(SPACE_SETTLE_SECS), TEXTURE_CLEAR_BELOW));
        assert!(!space_clear_due(true, Some(SPACE_SETTLE_SECS - 0.1), 10), "cleared while loading had just stopped");
        assert!(!space_clear_due(true, None, 10), "cleared while loading");
        assert!(!space_clear_due(false, Some(60.0), 10));
    }

    #[test]
    fn fallback_ends_tracing_without_touching_records() {
        let dir = TempDir::new("end_tracing");
        let id = gpu("581.57");
        dir.store().update(|r| r.record(&id, verdict(VerdictKind::Crashed))).unwrap();
        let mut session = Session::begin_as(dir.store(), 808, id.clone(), 5).unwrap();
        session.end_tracing().unwrap();
        assert_eq!(dir.markers(), 0, "the marker stayed after a live fallback");
        // A render error after the fallback happens in raster: no marker.
        session.fail("Validation render error while ray tracing".into(), 7).unwrap();
        assert_eq!(dir.markers(), 0, "a raster render error after the fallback blamed ray tracing");
        assert_eq!(dir.store().load().verdict(&id).map(|v| v.kind), Some(VerdictKind::Crashed), "a fallback cleared a crash record");
        // After a failure, the marker stays even through a fallback.
        let mut failed = Session::begin_as(dir.store(), 809, id.clone(), 5).unwrap();
        failed.fail("DeviceLost render error while ray tracing".into(), 6).unwrap();
        failed.end_tracing().unwrap();
        assert_eq!(dir.markers(), 1);
        failed.abandon();
    }

    #[test]
    fn fallback_reasons_read_well() {
        let heavy = FallbackReason::TooManyTextures { distinct: 4_812 };
        assert!(heavy.notice().unwrap().starts_with("Ray tracing is off for this session. "));
        assert!(heavy.describe().contains("4812") && heavy.describe().contains("5000"));
        assert_eq!(FallbackReason::TurnedOff.notice(), None);
        assert_eq!(FallbackReason::NoDenoiser.notice(), None);
        assert!(FallbackReason::NoDenoiser.describe().starts_with("DLSS Ray Reconstruction did not start"));
    }

    #[test]
    fn median_of_odd_and_even() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&mut []), 0.0);
    }
}
