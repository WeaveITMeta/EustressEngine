//! This computer's GPU, read from the operating system.
//!
//! No GPU device is created, no graphics API is loaded, and no window is
//! touched: a headless server and the fleet host agent report a GPU without
//! ever using one (docs/architecture/GPU_HOSTING_RULE.md). The sources, first
//! answer wins:
//!
//! 1. `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`
//!    where the NVIDIA driver is installed (Linux and Windows): name and VRAM.
//! 2. Linux: each `/sys/class/drm/cardN/device` (the PCI vendor and device,
//!    `product_name` where the driver has one, `mem_info_vram_total` on AMD),
//!    and `/proc/driver/nvidia/gpus/*/information` for an NVIDIA model name.
//! 3. Windows: the display adapter class in the registry, read with
//!    `reg query`: `DriverDesc` and `HardwareInformation.qwMemorySize` (or
//!    `HardwareInformation.MemorySize`).
//!
//! When several adapters answer, a hardware GPU wins over none, then the one
//! with the most VRAM. A software or basic display adapter (llvmpipe, the
//! Microsoft Basic Render Driver, a virtual or server management display) is
//! not a GPU: it gives an empty report. An empty report means no GPU is known.
//!
//! Std only: `eustress-engine` compiles this same file into `dedicated.rs`
//! (`#[path]`), so the dedicated server and the host agent read a GPU the same
//! way. A probe that cannot answer gives an empty report; nothing here panics,
//! and no program is waited on for longer than [`PROGRAM_TIMEOUT`].

use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// The longest any one probe program is waited on.
pub const PROGRAM_TIMEOUT: Duration = Duration::from_secs(5);

/// One GPU as the heartbeat reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuReport {
    /// The adapter's name as the operating system gives it. Empty when no
    /// hardware GPU was found.
    pub name: String,
    /// Dedicated VRAM in MB. 0 where the platform does not expose it.
    pub vram_mb: u64,
    /// A hardware GPU was found: `name` is not empty.
    pub available: bool,
}

impl GpuReport {
    /// A report for an adapter called `name`. A software or display-only
    /// adapter gives the empty report.
    pub fn of(name: &str, vram_mb: u64) -> Self {
        let name: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(160).collect();
        if name.is_empty() || is_not_a_gpu(&name) {
            return Self::default();
        }
        Self { name, vram_mb, available: true }
    }
}

/// Names of adapters that draw a desktop but are not a GPU a session could use.
pub fn is_not_a_gpu(name: &str) -> bool {
    const NOT: &[&str] = &[
        "llvmpipe",
        "softpipe",
        "swiftshader",
        "lavapipe",
        "basic render",
        "basic display",
        "remote display",
        "indirect display",
        "virtio",
        "qemu",
        "bochs",
        "cirrus",
        "vmware svga",
        "virtualbox",
        "hyper-v video",
        "aspeed",
        "matrox",
    ];
    let lower = name.to_ascii_lowercase();
    NOT.iter().any(|k| lower.contains(k))
}

/// The report for this computer, probed once per process.
pub fn cached() -> &'static GpuReport {
    REPORT.get_or_init(probe)
}

static REPORT: OnceLock<GpuReport> = OnceLock::new();
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Probe on a background thread, once. A caller on a frame loop uses this and
/// [`ready`] so the probe never holds a frame.
pub fn start() {
    if !STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        std::thread::spawn(|| {
            cached();
        });
    }
}

/// The report once the probe has finished; `None` before.
pub fn ready() -> Option<&'static GpuReport> {
    REPORT.get()
}

/// Read every source now. Prefer [`cached`].
pub fn probe() -> GpuReport {
    let mut found = Vec::new();
    if let Some(text) = run("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]) {
        found.extend(parse_nvidia_smi(&text));
    }
    if !found.iter().any(|g| g.available) {
        found.extend(os_adapters());
    }
    best(found)
}

/// A hardware GPU before none, then the most VRAM, then the first listed.
pub fn best(found: Vec<GpuReport>) -> GpuReport {
    let mut best: Option<GpuReport> = None;
    for report in found {
        let better = match &best {
            None => true,
            Some(b) => (report.available, report.vram_mb) > (b.available, b.vram_mb),
        };
        if better {
            best = Some(report);
        }
    }
    best.unwrap_or_default()
}

/// `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`:
/// one `name, MiB` line per GPU. A memory field that is not a number (`[N/A]`
/// on some virtual GPUs) is 0.
pub fn parse_nvidia_smi(text: &str) -> Vec<GpuReport> {
    text.lines()
        .filter_map(|line| {
            let (name, memory) = line.rsplit_once(',')?;
            Some(GpuReport::of(name, memory.trim().parse().unwrap_or(0)))
        })
        .filter(|g| g.available)
        .collect()
}

/// `/proc/driver/nvidia/gpus/*/information`: the `Model:` line.
pub fn parse_nvidia_proc(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "Model").then(|| value.trim().to_string()).filter(|v| !v.is_empty())
    })
}

/// A name for a DRM card from its PCI vendor and device ids, when the driver
/// gives no `product_name`.
pub fn drm_name(vendor: &str, device: &str) -> String {
    let device = device.trim();
    match vendor.trim().to_ascii_lowercase().as_str() {
        "0x1002" => format!("AMD GPU {device}"),
        "0x10de" => format!("NVIDIA GPU {device}"),
        "0x8086" => format!("Intel GPU {device}"),
        "0x1af4" => "virtio GPU".into(),
        "0x1234" => "QEMU standard VGA".into(),
        "0x15ad" => "VMware SVGA".into(),
        "0x1414" => "Microsoft Hyper-V video".into(),
        "0x1a03" => "ASPEED server display".into(),
        "0x102b" => "Matrox server display".into(),
        "0x80ee" => "VirtualBox display".into(),
        other => format!("GPU {other}:{device}"),
    }
}

/// `reg query <display adapter class>\NNNN`: the adapter's `DriverDesc` and
/// its VRAM from `HardwareInformation.qwMemorySize` (REG_QWORD), else
/// `HardwareInformation.MemorySize` (REG_DWORD, or REG_BINARY little-endian).
/// `None` when the key names no adapter.
pub fn parse_reg_adapter(text: &str) -> Option<GpuReport> {
    let mut name = None;
    let mut qword = None;
    let mut dword = None;
    for line in text.lines() {
        let line = line.trim();
        let Some(at) = line.find(" REG_") else { continue };
        let key = line[..at].trim();
        let rest = line[at..].trim();
        let (kind, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let value = value.trim();
        match key {
            "DriverDesc" if kind == "REG_SZ" => name = Some(value.to_string()),
            "HardwareInformation.qwMemorySize" => qword = reg_number(kind, value),
            "HardwareInformation.MemorySize" => dword = reg_number(kind, value),
            _ => {}
        }
    }
    let bytes = qword.or(dword).unwrap_or(0);
    Some(GpuReport::of(&name?, bytes / (1024 * 1024)))
}

fn reg_number(kind: &str, value: &str) -> Option<u64> {
    match kind {
        "REG_QWORD" | "REG_DWORD" => u64::from_str_radix(value.trim_start_matches("0x").trim_start_matches("0X"), 16).ok(),
        "REG_BINARY" => {
            let hex = value.trim();
            if hex.len() % 2 != 0 || hex.len() > 16 || hex.is_empty() {
                return None;
            }
            let mut out = 0u64;
            for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
                let byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
                out |= u64::from(byte) << (8 * i);
            }
            Some(out)
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn os_adapters() -> Vec<GpuReport> {
    let mut nvidia_names = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc/driver/nvidia/gpus") {
        for entry in entries.flatten() {
            if let Some(name) = std::fs::read_to_string(entry.path().join("information")).ok().and_then(|t| parse_nvidia_proc(&t)) {
                nvidia_names.push(name);
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        let mut cards: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_prefix("card"))
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            })
            .collect();
        cards.sort();
        let mut nvidia = nvidia_names.iter();
        for card in cards {
            let device = card.join("device");
            let read = |file: &str| std::fs::read_to_string(device.join(file)).ok().map(|s| s.trim().to_string());
            let vendor = read("vendor").unwrap_or_default();
            let vram_mb = read("mem_info_vram_total").and_then(|v| v.parse::<u64>().ok()).map_or(0, |b| b / (1024 * 1024));
            let name = read("product_name")
                .filter(|n| !n.is_empty())
                .or_else(|| if vendor == "0x10de" { nvidia.next().cloned() } else { None })
                .unwrap_or_else(|| drm_name(&vendor, &read("device").unwrap_or_default()));
            out.push(GpuReport::of(&name, vram_mb));
        }
    }
    if !out.iter().any(|g| g.available) {
        out.extend(nvidia_names.iter().map(|n| GpuReport::of(n, 0)));
    }
    out
}

#[cfg(windows)]
fn os_adapters() -> Vec<GpuReport> {
    const CLASS: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut out = Vec::new();
    let mut misses = 0;
    for i in 0..32u32 {
        match run("reg", &["query", &format!(r"{CLASS}\{i:04}")]) {
            Some(text) => {
                misses = 0;
                out.extend(parse_reg_adapter(&text));
            }
            None => {
                misses += 1;
                if misses >= 2 {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(not(any(target_os = "linux", windows)))]
fn os_adapters() -> Vec<GpuReport> {
    Vec::new()
}

/// Run `program` and return what it printed when it exits with success
/// inside [`PROGRAM_TIMEOUT`]. A program that runs longer is ended (this
/// child only, by its own handle).
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up behind Studio.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut bytes);
        let _ = send.send(bytes);
    });
    match receive.recv_timeout(PROGRAM_TIMEOUT) {
        Ok(bytes) => {
            let status = child.wait().ok()?;
            status.success().then(|| String::from_utf8_lossy(&bytes).into_owned())
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_lines_give_name_and_vram() {
        let text = "NVIDIA GeForce RTX 2080 Ti, 11264\nNVIDIA A100-SXM4-40GB, 40960\n";
        let found = parse_nvidia_smi(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0], GpuReport { name: "NVIDIA GeForce RTX 2080 Ti".into(), vram_mb: 11_264, available: true });
        assert_eq!(best(found).vram_mb, 40_960, "the most VRAM wins");
        assert_eq!(parse_nvidia_smi("GRID A100-4C, [N/A]")[0].vram_mb, 0, "unknown VRAM is 0");
        assert!(parse_nvidia_smi("").is_empty());
    }

    #[test]
    fn a_software_or_server_display_adapter_is_not_a_gpu() {
        for name in [
            "Microsoft Basic Render Driver",
            "llvmpipe (LLVM 15.0.7, 256 bits)",
            "ASPEED server display",
            "virtio GPU",
            "Matrox server display",
        ] {
            assert_eq!(GpuReport::of(name, 4096), GpuReport::default(), "{name}");
        }
        assert!(GpuReport::of("AMD Radeon RX 6800", 16_368).available);
        let picked = best(vec![GpuReport::default(), GpuReport::of("Intel(R) UHD Graphics 630", 0)]);
        assert_eq!(picked.name, "Intel(R) UHD Graphics 630", "a hardware GPU wins over none, even with VRAM unknown");
    }

    #[test]
    fn the_registry_adapter_key_gives_name_and_vram() {
        let text = r"
HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0000
    DriverDesc    REG_SZ    NVIDIA GeForce RTX 2080 Ti
    HardwareInformation.qwMemorySize    REG_QWORD    0x2c0000000
    HardwareInformation.MemorySize    REG_BINARY    00000000

HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0000\Settings
";
        let found = parse_reg_adapter(text).unwrap();
        assert_eq!(found, GpuReport { name: "NVIDIA GeForce RTX 2080 Ti".into(), vram_mb: 11_264, available: true });

        let dword = "    DriverDesc    REG_SZ    AMD Radeon RX 580\n    HardwareInformation.MemorySize    REG_DWORD    0x20000000\n";
        assert_eq!(parse_reg_adapter(dword).unwrap().vram_mb, 512);
        let binary = "    DriverDesc    REG_SZ    AMD Radeon RX 580\n    HardwareInformation.MemorySize    REG_BINARY    00000000\n";
        assert_eq!(parse_reg_adapter(binary).unwrap().vram_mb, 0);
        let le = "    DriverDesc    REG_SZ    AMD Radeon RX 580\n    HardwareInformation.MemorySize    REG_BINARY    00000080\n";
        assert_eq!(parse_reg_adapter(le).unwrap().vram_mb, 2_048, "0x80000000 little-endian is 2 GB");
        let basic = "    DriverDesc    REG_SZ    Microsoft Basic Display Adapter\n";
        assert!(!parse_reg_adapter(basic).unwrap().available);
        assert!(parse_reg_adapter("    Class    REG_SZ    Display\n").is_none());
    }

    #[test]
    fn drm_cards_are_named_from_their_pci_ids() {
        assert_eq!(drm_name("0x1002", "0x73bf"), "AMD GPU 0x73bf");
        assert!(!GpuReport::of(&drm_name("0x1a03", "0x2000"), 0).available, "a BMC display is not a GPU");
        assert_eq!(
            parse_nvidia_proc("Model: \t\t NVIDIA GeForce RTX 3090\nIRQ:   42\n").as_deref(),
            Some("NVIDIA GeForce RTX 3090")
        );
    }
}
