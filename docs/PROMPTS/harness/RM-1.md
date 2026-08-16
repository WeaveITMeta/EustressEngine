# RM-1 — the harness reference machine

`RM-1` is the machine that every frame-cost threshold in this prompt library refers to. When a pack
writes "at 3840×2160 on the harness reference GPU", the GPU it means is the one described here.

`RM-1` is the operator's own workstation. It is not a lab machine, not a rented runner, and not a
machine the project intends to acquire. It is the box the captures are actually taken on today.

The canonical, machine-readable descriptor is
[`reference_machines.json`](reference_machines.json). This file is its prose companion; the JSON is
the source of truth, and a manifest writer fills the `hardware` block from the JSON rather than from
this page.

## The descriptor

| Field | Value |
|---|---|
| `id` | `RM-1` |
| `cpu` | Intel(R) Core(TM) i9-10980XE CPU @ 3.00GHz — 18 cores / 36 threads |
| `gpu` | NVIDIA GeForce GTX 1080 Ti |
| `gpu_driver` | `32.0.15.8253` (WDDM); NVIDIA release `582.53`, dated 2026-04-14 |
| `ram_gb` | 256 |
| `os` | Microsoft Windows 11 Pro |
| `os_build` | `10.0.26200` |
| `wgpu_backend` | Vulkan |
| `rustc_version` | `rustc 1.95.0 (59807616e 2026-04-14)` |
| `recorded_by` | Weave Solutions Admin \<admin@weave.solutions\> |
| `recorded_utc` | `2026-08-08T01:39:23Z` |

Every one of these values was read off the machine by a command, and the JSON records the literal
command beside each field in its `field_provenance` block. Nothing here was transcribed from a
settings dialog, a product page, or from memory. That is the point: a hardware pin whose provenance
is a human reading a screen is not a pin, because nothing detects it drifting.

Two of the fields deserve a note.

`wgpu_backend` is **observed, not chosen**. The engine requests `Backends::all()` with
`PowerPreference::HighPerformance` (`eustress/crates/engine/src/main.rs:148-150`), so the backend is
whatever wgpu selects at startup. On this machine it selects Vulkan, which the engine records in its
own adapter report:

```
AdapterInfo { name: "NVIDIA GeForce GTX 1080 Ti", vendor: 4318, device: 6918,
              device_type: DiscreteGpu, driver: "NVIDIA", driver_info: "582.53",
              backend: Vulkan, subgroup_min_size: 32, subgroup_max_size: 32 }
```

A capture that reports any other backend is not an `RM-1` capture, whatever hardware it ran on.

`gpu_driver` is the field that actually moves. GPUs and CPUs get replaced on the order of years;
drivers change on the order of weeks, and a driver change is the single most common way a frame-cost
number stops being comparable to the one beside it. That is why verification checks the driver
string exactly rather than checking the GPU name alone.

## Verifying the pin

From the repository root:

```
pwsh -NoProfile -File docs/PROMPTS/harness/verify_reference_machine.ps1 -Id RM-1 ; echo "RM_EXIT=$?"
```

Expected on `RM-1`:

```
missing=
gpu_match=True
driver_match=True
RM_EXIT=0
```

The script queries `Win32_VideoController` on the live machine and compares the result to the
registry. It exits 0 only when all eleven required fields — `id`, `cpu`, `gpu`, `gpu_driver`,
`ram_gb`, `os`, `os_build`, `wgpu_backend`, `rustc_version`, `recorded_by`, `recorded_utc` — are
non-empty **and** both the GPU name and the GPU driver version match string for string. A registry
with an empty `gpu_driver` exits 1. A stale driver string exits 1 while `gpu_match` still prints
`True`, which is exactly the drift the check exists to catch.

If the check fails on a machine you believe is `RM-1`, the driver moved. Re-read the fields with the
commands in `field_provenance`, update the entry, and re-record `recorded_utc`. Do not relax the
comparison.

## What resolves to RM-1

`T1_rendering_and_content.md` states seven frame-cost budgets against the harness reference GPU.
Each of them now names a specific GPU, a specific driver, and a specific backend, and each is
therefore falsifiable:

| Line | Threshold |
|---|---|
| `T1_rendering_and_content.md:1287` | added cost ≤ 2.0 ms/frame at 3840×2160 |
| `:1366` | added cost ≤ 2.0 ms/frame at 3840×2160 |
| `:1607` | added cost ≤ 1.5 ms |
| `:2195` | measured cost ≤ 2.5 ms/frame at 3840×2160 |
| `:2294` | added cost ≤ 2.5 ms |
| `:2520` | added cost ≤ 2.0 ms |
| `:4358` | `ft_p99_ms` ≤ 25.0 at 3840×2160 |

`02_CAPTURE_HARNESS.md` §7.2 requires a `hardware` block on every bundle manifest carrying `cpu`,
`gpu`, `gpu_driver`, `ram_gb` and `os`, and §7.3 makes an external-reference control inadmissible
unless that block is populated. The `manifest_hardware_block` object in the `RM-1` entry is that
block, pre-composed, so a manifest writer copies it rather than guessing at it.

## What pinning RM-1 does not claim

Pinning `RM-1` records *which* machine the millisecond budgets refer to. It is **not** a judgement
that those budgets are achievable on it.

The seven thresholds above were authored before any machine was named. A 1.5 ms budget on a
2017-generation GPU may turn out to be generous, or it may turn out to be impossible; nothing in
this document takes a position on that. What this document changes is that the question can now be
answered by a measurement instead of being unanswerable.

If a threshold proves wrong for this hardware, that is a floor change, and `00_MASTER_PROTOCOL.md`
§5.3 routes floor changes to the human as a `LOWER` decision. Re-basing a budget to fit the pinned
hardware is not a repair; it is deleting the constraint. The thresholds stay exactly as authored.

## Adding a second machine

`machines` is an array, and the schema assumes more than one entry from the start. A second machine
is appended as `RM-2` with the same eleven required fields and its own `field_provenance`; no schema
change is needed, and `verify_reference_machine.ps1 -Id RM-2` verifies it the same way. A capture
recipe names the machine it targets, so `RM-1` and `RM-2` results never silently mix.
