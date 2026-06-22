# Forge Vendor Remediations — `forge-orchestration-0.4.2/`

**Status:** Audit complete. Vendored copy verified byte-for-byte identical to published upstream.
**Verdict:** SAFE TO DELETE — but deletion **awaits explicit user sign-off** and must be a **trash-move, not `rm`**.
**Date:** 2026-06-16
**Scope of this document:** Records the audit of the vendored directory `E:\Workspace\EustressEngine\forge-orchestration-0.4.2/`, whether it carries any unique local modifications, what (if anything) must be forward-ported to forge-orchestration 0.6.0, and the deletion recommendation. This document does not delete or move anything — analysis only.

---

## 1. What the vendored directory is (and is not)

`E:\Workspace\EustressEngine\forge-orchestration-0.4.2/` is a copy of the published crates.io crate `forge-orchestration` at version `0.4.2`, sitting at the repository root.

Critically, it is **not** wired into the build:

- The wrapper crate `eustress/crates/forge/Cargo.toml` (line 13) declares:
  ```toml
  forge-orchestration = { version = "0.4.2", default-features = false, features = ["nomad", "quic", "tls-ring"] }
  ```
  This is a **registry version dependency**, resolved from crates.io — **not** a `path = "../../forge-orchestration-0.4.2"` dependency.
- The workspace manifest `eustress/Cargo.toml` has `[patch.crates-io]` entries for **only** `gpu-allocator`, `fjall`, `lsm-tree`, and `wgpu`. **There is no `forge-orchestration` patch entry anywhere.**
- `eustress/Cargo.lock` pins `forge-orchestration` v0.4.2 to source `registry+https://github.com/rust-lang/crates.io-index` with checksum `717e9e03289d9cd5f6858c23309f93e4de7dc6b1af367b2b4e3bcb393b98de96`.
- No git submodule and no `.cargo/config` reference point at the vendored path.

**Conclusion: the vendored directory is fully orphaned from the build graph.** Cargo compiles against the registry copy, not this directory. Deleting it does not change what compiles.

---

## 2. Audit method

Two independent passes were run (an audit pass and an adversarial refutation pass). Both reached the same conclusion.

1. **Pristine baseline.** A known-good published `forge-orchestration-0.4.2` was located in the local cargo registry cache at:
   `C:\Users\miksu\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\forge-orchestration-0.4.2`
   This is the canonical unmodified extraction of the crates.io artifact.

2. **File-tree symmetry.** The vendored tree was compared against the pristine tree.
   - Files present in vendored but absent from pristine: **all under `./target/`** (untracked compiled build cache — `.rustc_info.json`, `CACHEDIR.TAG`, `debug/`, `flycheck1/`, `tmp/`).
   - Files present in pristine but absent from vendored: only `.cargo-ok` (the registry extraction marker).
   - **Zero vendored-only SOURCE files.**

3. **Recursive content diff.** `diff -r` across the trees (excluding `target/` and `.cargo-ok`) returned **exit code 0** — no content differences in any `src/**/*.rs`, examples, benches, or manifests.

4. **SHA-256 equality.** Every shared file was hashed and compared: **44 shared files, 0 mismatches, all byte-identical.** This explicitly includes `Cargo.toml`, `Cargo.toml.orig`, `Cargo.lock`, and `README.md`.

5. **Edit-marker scan.** Grep over the vendored `src` for `eustress | worlddb | fjall | bespoke | remediat | HACK | FIXME | XXX | PATCH | TODO` found **no local edit markers**. The only `patch` hits are legitimate upstream Kubernetes admission-webhook JSONPatch domain vocabulary in `src/controlplane/admission.rs` — present identically in the pristine upstream.

6. **Build-graph orphaning check.** Confirmed the registry-version dependency, the absence of any `forge-orchestration` `[patch]` entry, the `Cargo.lock` registry pin + checksum, and the absence of any submodule / `.cargo/config` reference (see Section 1).

7. **Git tracking check.** The 44 source files **are git-tracked** — committed in `3f6c17a1` ("feat: add MCP server, parameters system, forge orchestration, ..."). `git diff --quiet HEAD -- forge-orchestration-0.4.2` reports the working tree is identical to the committed version. The `??` shown in some git-status snapshots is caused **only** by the untracked nested `target/` build cache, not by the source.

---

## 3. Does the vendored directory have unique local modifications?

**No.** The vendored `forge-orchestration-0.4.2/` is a byte-for-byte unmodified copy of the published crates.io 0.4.2 release.

- No eustress-specific patches, hacks, or fixes are embedded in the source.
- No edit markers, no local remediations, no bespoke code.
- Both manifests (`Cargo.toml`, `Cargo.toml.orig`), `Cargo.lock`, and `README.md` are byte-identical to pristine.

The vendored `Cargo.toml` is the standard cargo-normalized registry manifest (the `AUTOMATICALLY GENERATED` header, package name `forge-orchestration`, no path deps, no patch section) — i.e., exactly what crates.io ships.

---

## 4. Remediations to forward-port to 0.6.0

**None.** Because the vendored copy carries zero unique local modifications, there is nothing eustress-specific to re-apply on top of forge-orchestration 0.6.0. There are no embedded patches to forward-port.

This is a separate concern from the 0.4.2 → 0.6.0 **upgrade** itself, which is driven entirely by upstream's **additive** API surface and is documented elsewhere:

- **Migration guide:** `docs/documents/forge-eustress-integration.md`
- **Wrapper crate to update:** `eustress/crates/forge/`

For reference, the 0.6.0 delta the upgrade work concerns (none of which lives in the vendored dir) is:

- `scheduler::{sim, gang, deadline, reconcile}` plus `algorithms/optimized/placement/preemption/queue` (not feature-gated). New re-exports include `SimCell`, `SimWorld`, `AgentPolicy`, `CoPlacement`, `Region3D`, `GangGroup`, `MemberRole`, `SimMember`, `GangScheduler`, `GangDecision`, `GangReservation`, `DeadlineQueue`, `TickDeadlineScheduler`, `MissPolicy`, `DeadlineEntry`, `Eligibility`, `TickOutcome`, `Reconciler`, `Assignment`, `ReconcileReport`, `MetricsSource`, `TaskStatus`, `BinPackScheduler`, `SpreadScheduler`, `GpuLocalityScheduler`, `LearnedScheduler`.
- `storage::RaftStateStore` behind cargo features `raft = ["dep:openraft"]` and `raft-persist = ["raft", "dep:fjall"]` (not in a default build).
- 0.6.0 **retains every** 0.4.2 module (autoscaler, builder, controlplane, error, federation, inference, job, metrics, moe, networking, nomad, resilience, runtime, scheduler, sdk, storage, types), so game-server hosting remains fully intact.
- On Windows, keep `tls-ring` (avoids the cmake/MSVC requirement of `tls-aws-lc`).

Per binding user decision **D2**, the agents-in-sims scheduling code will live in a **new `sim` module inside `eustress/crates/forge`**, not in a separate crate. Per the state-layer decision, WorldDb remains the local entity source-of-truth and Forge's `RaftStateStore` owns only the cell-shared / cross-node replicated slice.

---

## 5. DELETE RECOMMENDATION

**SAFE TO DELETE.** The vendored `forge-orchestration-0.4.2/` is a verified byte-for-byte unmodified copy of published crates.io 0.4.2 (clean recursive diff + SHA-256 equality on all 44 shared files, both manifests, `Cargo.lock`, and `README.md`). It contains zero local remediations, so nothing is lost on deletion and there is nothing to forward-port.

Nothing is at risk because the identical source remains available in **three** places after deletion:
1. The cargo registry cache (`C:\Users\miksu\.cargo\registry\src\...\forge-orchestration-0.4.2`).
2. crates.io / docs.rs (`https://docs.rs/crate/forge-orchestration/0.4.2/source/`).
3. Git history (committed in `3f6c17a1`; recoverable via `git checkout 3f6c17a1 -- forge-orchestration-0.4.2`).

### Guardrails (binding)

- **Deletion awaits explicit user sign-off.** Per the binding decision, "Vendored 0.4.2 must be EVALUATED before any deletion. NO agent may delete it. Deletion is the user's call after review." This document is that evaluation; it does **not** authorize or perform deletion.
- **When approved, trash-move — do not `rm`.** Per the project's reversibility rule, removal must be a reversible move to trash/recycle, never a hard `rm -rf` or `Remove-Item`. (Git history makes it recoverable regardless, but the trash-move rule still applies.)
- **The nested `target/` build cache** inside the vendored dir is untracked artifact bloat (hundreds of compiled files) and can be cleaned independently of the source-dir decision, with or without user sign-off on the source.

### Residual value if kept

The only reason to keep the directory is as an **offline reference** for the 0.4.2 → 0.6.0 delta review. That value is low, since the same source is in the registry cache and on docs.rs. If kept, at minimum clean the nested `target/` cache.

---

## 6. One-line summary

The vendored `forge-orchestration-0.4.2/` is unmodified published upstream with **no local changes and nothing to forward-port**; it is **orphaned from the build graph**; it is **safe to delete**, but deletion is the **user's call** and must be a **trash-move, not `rm`**.
