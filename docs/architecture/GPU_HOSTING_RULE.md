# GPU hosting rule

CPU first. GPU option.

A hosted or sharded session runs on the CPU. A GPU is something a listing asks for, by name, and the
fleet then places it only where that GPU exists. Nothing about who may host, or how hosts are ranked,
depends on this rule.

## The rule

1. **CPU only by default.** A session Eustress hosts, and every shard of a sharded world, runs its
   authoritative simulation on the CPU. The `--server` process creates no window and no GPU device
   (`eustress/crates/engine/src/server_mode.rs`, `MinimalPlugins`).
2. **No authoritative path depends on a GPU unless the listing opts in.** Physics, scripts, particle
   simulation that feeds physics or script values, terrain edits and anything else that decides the
   state players share run on the CPU for every listing whose `gpu` is `none` or `optional`. A GPU
   path (`realism-gpu`, compute terrain, compute deformation) may write authoritative state only in a
   listing whose `gpu` is `required`.
3. **A listing that opts in is placed only on a GPU host with enough VRAM.** A listing with
   `gpu: "required"` names `gpu_min_vram_mb`. Each of its sessions is placed only on a machine that
   reports `gpu_available` and has at least that much VRAM not already held by its other GPU sessions.
   When no machine fits, the request is refused with `no_gpu_capacity`. It is never placed on a CPU
   machine.
4. **All shards of a GPU world run on GPU hosts of the same class.** A class is one adapter model, as
   the hosts report it in `gpu` (for example `NVIDIA GeForce RTX 2080 Ti`), with at least the listing's
   `gpu_min_vram_mb`. Two models give two classes, even at the same VRAM.
5. **CPU and GPU hosts never split one world** until a test proves that a shard on a GPU host and its
   neighbour on a CPU host compute the same state from the same inputs. No such test exists, so no
   world is split across them.
6. **Hosting and ranking do not change.** The most qualified hosts, ranked by ping and power
   (`rankServers` and `rankScore` in `infrastructure/cloudflare/api/src/live.mjs`,
   `local_launch::rank_servers` in Rust), host as they always have. GPU filtering applies only to a
   listing whose `gpu` is `required`, and only to which fleet machines may run its sessions.

## The listing setting

Stored on the listing record (`sim:<id>` in `SOCIAL`), set by the author with
`PATCH /api/simulations/{id}/listing` (`src/listing_edit.mjs`) and checked by
`checkGpuSetting` in `src/fleet_policy.mjs`. It is not text a reviewer reads, so changing it never sends
the listing back to review.

| `gpu` | Meaning | `gpu_min_vram_mb` | Placement |
|---|---|---|---|
| `none` (default; stored as no field) | CPU only | not accepted | any machine, exactly as the fleet always places |
| `optional` | may use a GPU for anything that is not authoritative (rendering a spectator view, an effect no player state reads) | not accepted | any machine, exactly as `none` |
| `required` | an authoritative path uses the GPU | required, a whole number of MB from 256 to 262144 | only a GPU host with that much VRAM free; else `no_gpu_capacity` |

A listing record with an unknown `gpu` value reads as `none` (`gpuSettingOf`).

## What a host reports

Every host reads its GPU from the operating system, never by creating a GPU device:
`eustress/crates/forge/src/gpu_probe.rs`, which `eustress-engine` compiles into `dedicated.rs` as well,
so the dedicated server and the fleet host agent report a GPU the same way. Sources, first answer wins:

1. `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits` (Linux and Windows with the
   NVIDIA driver): name and VRAM.
2. Linux: `/sys/class/drm/cardN/device` (PCI vendor and device, `product_name`, and
   `mem_info_vram_total` on AMD) and `/proc/driver/nvidia/gpus/*/information` for an NVIDIA model name.
3. Windows: the display adapter class in the registry through `reg query` (`DriverDesc`,
   `HardwareInformation.qwMemorySize`, else `HardwareInformation.MemorySize`).

A software or display-only adapter (llvmpipe, the Microsoft Basic Render Driver, virtio, an ASPEED or
Matrox server management display) is not a GPU: it gives an empty report. Where the platform does not
expose VRAM, `gpu_vram_mb` is 0, and such a host takes no `required` listing.

| Heartbeat | Fields | Old hosts |
|---|---|---|
| Fleet, Route 1 (`fleet_host.rs` `BeatMachine`, `fleet.mjs` `checkHeartbeat`) | `machine.gpu`, `machine.gpu_vram_mb`, `machine.gpu_available` | all three optional; absent reads as no GPU |
| `/live` (`dedicated.rs` `hardware_class`, `live.mjs` beat) | `hardware.gpu`, `hardware.cpu`, `hardware.tier`, plus `hardware.gpu_vram_mb` and `hardware.gpu_available` | `{gpu, cpu, tier}` alone is accepted as always; the two additions are kept only when in range |

The host agent's `EUSTRESS_FLEET_GPU_VRAM_MB` replaces the probed VRAM; `0` offers no GPU to the fleet.
The dedicated server probes on a background thread, so a frame never waits on it.

## Placement

In `eustress/crates/forge/src/fleet.rs` and its port `infrastructure/cloudflare/api/src/fleet_policy.mjs`:

```text
gpu fits (a new session) = gpu_available && gpu_vram_mb - sum(session.gpu_vram_mb on the machine) >= gpu_min_vram_mb
gpu class fits (a seat)  = gpu_available && gpu_vram_mb >= gpu_min_vram_mb
```

The CPU lines (80% committed, 90% activation) and the memory line apply to every session as before. A
new session of a `required` listing holds `gpu_min_vram_mb` (`Session::gpu_vram_mb`) for as long as it
runs. The control plane records that hold itself (`gv:<server_id>` in the `Fleet` Durable Object) when it
asks for the start; a host's own claim about a session's VRAM is never read. A seat in a running, warm or
starting session of a `required` listing, and the listing's warm servers, go only to machines whose GPU
fits. The refusal is `RefuseReason::NoGpuCapacity` (`no_gpu_capacity`); the allocate route answers
`503 {status: "full", code: "no_gpu_capacity", gpu_min_vram_mb}`, and the plan recommends a machine
with a GPU of that size (`recommend_add_machine` with `gpu_vram_mb`).

## Not built

- The shard placer. Sharding has no placer yet, so rule 4 is a requirement on it, not a check in code
  today. Fleet sessions of one listing are separate worlds, not shards of one.
- An authoritative GPU path. No listing can use one yet: `realism-gpu` is enabled by no crate. The
  agreement test of rule 5 is owed before any world mixes host kinds.
- The setting in Studio's publish dialog and on the website. Today it is set through the listing edit
  route only.
