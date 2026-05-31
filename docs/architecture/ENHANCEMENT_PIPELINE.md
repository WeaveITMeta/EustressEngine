# ⚠️ DEPRECATED — see SCALING_ARCHITECTURE.md

> This document described a runtime Python generation server (FLUX textures, TripoSR
> meshes) called over HTTP to "enhance" primitives into photoreal assets while playing.
> **That runtime enhancement model is not how photorealism is achieved.** Per-frame
> network asset generation cannot coexist with a 16.6 ms frame budget at scale.
>
> The current plan keeps AI asset generation **only as an offline, bake-time path** with
> a SHA256 content cache, producing scale-ready assets (LOD chains, meshlet clusters,
> octahedral impostors, bindless/virtual textures) that the runtime merely *loads*.
> Photorealism is achieved by the renderer (GPU-driven culling + GTAO/SSR/TAA/volumetrics
> now, ray-traced GI later) plus baked asset quality.
>
> **See [SCALING_ARCHITECTURE.md](SCALING_ARCHITECTURE.md) §6 (asset bake pipeline) and
> §5 (rendering).** The historical content below is retained for context only.

---

# 🔥 Eustress Enhancement Pipeline - Setup Guide (HISTORICAL)

The original setup guide (Python `generation_server.py`, stub/production modes, FLUX +
TripoSR over HTTP on port 8001, `~/.cache/eustress/enhancement/` SHA256 cache, RON test
scenes) is obsolete as a *runtime* design. The one durable idea — aggressive SHA256
content caching of generated assets so nothing regenerates twice — survives in
SCALING_ARCHITECTURE.md §6 as part of the offline bake pipeline writing into
`.eustress/assets/`.
