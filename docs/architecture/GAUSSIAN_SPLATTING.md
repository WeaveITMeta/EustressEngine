# Gaussian Splatting in Eustress Engine

Status: ARCHITECTURE / FEASIBILITY (verified 2026-06-27). Authored against the
`feat/bevy-0.19` branch (Bevy 0.19, wgpu 29.0.3, MSRV 1.95, edition 2024, Avian 0.7).
Companion docs: `BEVY_019_MIGRATION.md`, `SCALING_ARCHITECTURE.md`, `CAD_PLATFORM_PLAN.md`.

---

## 1. Executive summary

**Does it work with Bevy? Yes — today, with zero custom rendering code.** The crate
`bevy_gaussian_splatting` **v8.0.1** (published 2026-06-25) targets **Bevy 0.19 + wgpu 29** —
exactly our migration target — and ships a complete GPU pipeline (radix sort, instanced-quad
rasterization, `.ply`/`.gcloud`/glTF-`KHR_gaussian_splatting` loaders, four shipped sort
backends, 2D/3D/4D Gaussians). It is co-current with `feat/bevy-0.19`. (Note: `.spz` is **not**
in the crate yet — roadmap; see §3.3/§5.1 for the conversion path.)

**Can it be made to work (custom)? Yes, and it is well-trodden Bevy plumbing**, but it is weeks
of work to re-derive what the crate already ships — chiefly a correct, fast **WGSL GPU radix
sort** (the single hardest piece) and a wgpu-29 `ViewNode`. The render path
(project → bin → sort → composite) ports cleanly to wgpu compute + a blend pass; the only
genuinely hard parts are the per-frame depth sort at scale and compositing splats against
existing Bevy meshes.

**Recommendation:** **adopt `bevy_gaussian_splatting` and upstream-track it** behind a new
`crates/radiance` (or `crates/splat`) wrapper crate, feature-gated and inference-only first.
Fall back to a custom `ViewNode` (lifting `wgpu-3dgs-viewer`'s sorter) only if mixed GS+mesh
compositing or toolchain constraints force it. Do **not** embed `brush` unless we need to
*train* splats inside Eustress — its renderer is entangled with the Burn/CubeCL autodiff stack
and is not cleanly separable.

---

## 2. What Gaussian Splatting is

3D Gaussian Splatting (3DGS) — Kerbl, Kopanas, Leimkühler, Drettakis, *3D Gaussian Splatting
for Real-Time Radiance Field Rendering*, SIGGRAPH 2023 (ACM TOG 42(4); arXiv:2308.04079;
INRIA GRAPHDECO) — is a **radiance-field** representation that renders in real time by
rasterizing a cloud of anisotropic 3D Gaussians instead of marching rays through an MLP (as
NeRF does). It is an **explicit, unstructured point primitive**, which is why it fits an
ECS/wgpu engine far better than NeRF.

### 2.1 The representation

A scene is an unstructured set of Gaussian "splats." Each primitive stores:

| Field | Symbol | Storage detail |
|---|---|---|
| Position / mean | `μ ∈ ℝ³` | world space |
| Covariance | `Σ ∈ ℝ³ˣ³` | never stored directly — see below |
| Scale | `S = diag(sₓ,s_y,s_z)` | **log space** (`s = exp(param)`, keeps `s ≥ 0`) |
| Rotation | quaternion `q` | normalized at use; → rotation matrix `R` |
| Opacity | `o ∈ [0,1]` | stored **pre-sigmoid** (logit) |
| View-dependent color | SH coeffs | **degree 3 = 16 coeffs/channel** (1+3+5+7) = 48 floats |

Covariance is reconstructed from scale + rotation so it stays positive semi-definite through
gradient descent:

```
Σ = R · S · Sᵀ · Rᵀ
```

### 2.2 The render math (precise enough to implement)

Constants below are verbatim from `diff-gaussian-rasterization/cuda_rasterizer/forward.cu`
and `auxiliary.h` (the canonical reference). The whole forward path is what we port to WGSL.

**(a) 3D covariance** from `q = (r,x,y,z)` and `S`. Build `R`, form `M = S·R`, then
`Σ = Mᵀ M` (≡ `R S Sᵀ Rᵀ`); store the 6 upper-triangular entries.

```
R = [[1-2(y²+z²),  2(xy - rz),   2(xz + ry)],
     [2(xy + rz),  1-2(x²+z²),   2(yz - rx)],
     [2(xz - ry),  2(yz + rx),   1-2(x²+y²)]]
```

**(b) 3D → 2D covariance (EWA projection, the Jacobian).** View-transform the center
`t = W·μ` (`t.z` = depth). With focal lengths `focal_x, focal_y`:

```
J = [[ focal_x / t.z ,      0       , -(focal_x · t.x) / t.z² ],
     [      0        , focal_y / t.z , -(focal_y · t.y) / t.z² ]]
```

Clamp `t.x/t.z`, `t.y/t.z` to frustum bounds first (avoids extreme footprints). With `W` the
upper-left 3×3 of the view matrix:

```
Σ′ = J · W · Σ · Wᵀ · Jᵀ     (take the top-left 2×2)
```

**Low-pass / dilation filter** (anti-degeneracy, guarantees ≥ ~1px coverage): add **0.3** to
both diagonals — `Σ′[0][0] += 0.3; Σ′[1][1] += 0.3`. (Mip-Splatting replaces this ad-hoc
constant with a principled Nyquist filter — see §4.)

**(c) 2D evaluation + conic.** Invert the 2×2 `Σ′` to the **conic** `A = Σ′⁻¹`. For pixel
offset `d = (pixel − μ₂D)`:

```
power = -0.5 * (A00*d.x² + A11*d.y² + 2*A01*d.x*d.y)
alpha = min(0.99, o * exp(power))     // skip fragment if power > 0
```

**(d) Sorted front-to-back alpha compositing.** Per pixel, iterate splats in depth order with
transmittance `T = 1`:

```
C = Σ_i  c_i · α_i · T_i ,   T_i = Π_{j<i} (1 − α_j)
// i.e.  C += c_i*α_i*T;  T *= (1 − α_i);  break when T < 0.0001
```

This **order dependence is the crux**: every camera move requires re-sorting all splats by
view-space depth (§8). The reference does this with a single approximate global GPU radix sort
over `(tileID | depth)` keys, 16×16 pixel tiles, early-out at `T < 0.0001`.

**(e) SH color** (degree 0–3). `dir = normalize(μ − cam_pos)`. Constants (`auxiliary.h`):

```
SH_C0 = 0.28209479177387814
SH_C1 = 0.4886025119029199
SH_C2 = [ 1.0925484305920792, -1.0925484305920792, 0.31539156525252005,
         -1.0925484305920792,  0.5462742152960396 ]
SH_C3 = [-0.5900435899266435,  2.890611442640554, -0.4570457994644658,
          0.3731763325901154, -0.4570457994644658, 1.445305721320277,
         -0.5900435899266435 ]
result  =  SH_C0*sh[0]
result += -SH_C1*y*sh[1] + SH_C1*z*sh[2] - SH_C1*x*sh[3]
result +=  SH_C2[0]*xy*sh[4] + SH_C2[1]*yz*sh[5] + SH_C2[2]*(2zz-xx-yy)*sh[6]
        +  SH_C2[3]*xz*sh[7] + SH_C2[4]*(xx-yy)*sh[8]
result +=  SH_C3[0]*y*(3xx-yy)*sh[9] + SH_C3[1]*xy*z*sh[10]
        +  SH_C3[2]*y*(4zz-xx-yy)*sh[11] + SH_C3[3]*z*(2zz-3xx-3yy)*sh[12]
        +  SH_C3[4]*x*(4zz-xx-yy)*sh[13] + SH_C3[5]*z*(xx-yy)*sh[14]
        +  SH_C3[6]*x*(xx-3yy)*sh[15]
rgb = max(result + 0.5, 0.0)     // DC offset + clamp
```

These polynomials and `exp(power)` port **line-for-line** to WGSL.

### 2.3 Training (densification / pruning, SfM init)

Training is the genuinely hard 20% and is **out of scope for phase 1** — we port inference.
For completeness:

- Inputs: posed images + a **sparse COLMAP/SfM point cloud**. Gaussians are **initialized at
  the SfM points** (color from points, isotropic scale from nearest-neighbor distance). Vanilla
  3DGS is sensitive to init quality (MCMC removes this — §4).
- ~30,000 Adam iterations. Loss `L = (1−λ)·L₁ + λ·L_D-SSIM`, `λ = 0.2`. SH degree grown
  0 → 3 progressively.
- **Adaptive density control** every 100 iters where view-space positional gradient exceeds
  `τ_pos = 0.0002`: **clone** small under-reconstructed Gaussians, **split** large
  over-reconstructed ones (scale ÷1.6). **Prune** opacity < 0.005 and oversized splats.
  **Reset opacity** every 3000 iters to cull floaters. Densification active iter 500–15,000.

### 2.4 Where it fits vs meshes / voxels in a game engine

| Aspect | Meshes (current Eustress) | Voxels / V-Cell | Gaussian Splatting |
|---|---|---|---|
| Geometry | exact, editable | volumetric, editable | photoreal capture, *not* directly editable |
| Authoring | CAD / mesh-edit / primitives | procedural / sim | reconstructed from photos/video or imported |
| Material | PBR | per-cell | baked view-dependent (SH) |
| Physics | Avian colliders | Avian/voxel | **none intrinsically** (see §7) |
| Best for | gameplay objects, CAD | sim/structure | environments, scanned real-world scenes, backdrops |
| Cost driver | triangle/draw count | cell count | splat count + **per-frame depth sort** |

GS is a **capture/visualization primitive**, complementary to (not a replacement for) the mesh
and V-Cell pipelines. It is the natural fit for "import a scanned real place" — which dovetails
with the place-ontology and importer work — and for photoreal backdrops behind authored
geometry. It does not give you editable surfaces (use 2DGS → mesh extraction for that — §4).

---

## 3. File formats & data pipeline

### 3.1 Inputs — COLMAP / SfM (training only)

```
<scene>/
  images/                 # posed photos
  sparse/0/
    cameras.bin|txt       # intrinsics: model id, w,h, fx,fy,cx,cy
    images.bin|txt        # extrinsics: quaternion + translation per image
    points3D.bin          # sparse xyz+rgb → Gaussian init
```

Little-endian id-prefixed binaries. Reference loader supports `SIMPLE_PINHOLE`/`PINHOLE`;
other camera models must be undistorted/converted first. Relevant only when we add a trainer.

### 3.2 Trained `.ply` — de-facto interchange

Binary little-endian PLY, one vertex element per Gaussian, properties in this order:

```
x, y, z                       # position (f32)
nx, ny, nz                    # normals — present but UNUSED by 3DGS
f_dc_0..2                     # SH DC (degree-0), per RGB
f_rest_0..44                  # 45 = 15 higher-order SH coeffs × 3 channels
opacity                       # pre-sigmoid (logit)
scale_0..2                    # LOG-space (apply exp)
rot_0..3                      # quaternion (normalize at load)
```

Uncompressed → 50 MB to several GB per scene. A 30-year-old container repurposed for splats.

### 3.3 Runtime / delivery formats

| Format | Size | Bytes/splat | SH? | Notes |
|---|---|---|---|---|
| `.splat` (antimatter15) | smallest naive | **32** | no | pos 3×f32, scale 3×f32 (**exp-applied**), RGBA u8 (`R=(0.5+SH_C0·f_dc)·255`, `A=sigmoid(o)·255`), rot 4×u8 (`q/‖q‖·128+128`). Trivial `bytemuck` parse; loses view-dependence (scenes look "dead"). |
| `.ksplat` (Kellogg) | smaller, streamable | bucketed | tiered | repack of `.splat`, Three.js ecosystem default. |
| `.spz` (Niantic, OSS late-2024) | **~10× < PLY** | quantized | 8-bit | fixed-point positions (~24-bit + fractional-bits header), SH/opacity 8-bit, compact quat, gzip. Emerging **de-facto compressed delivery standard**. `github.com/nianticlabs/spz`. |
| `.sog` (Self-Organizing Gaussians) | ~80–200 MB (vs 1.5 GB PLY) | textures | yes | sort splats into a 2D grid by locality, store attrs as **textures**, compress with PNG/JPEG-XL. GPU-friendly (decode as textures). SIGGRAPH 2024. |

**Eustress delivery target:** `.gcloud` (the crate's native f16/f32 flexbuffers/bincode2
format) is the on-disk runtime form and the most compact thing the crate loads natively;
**`.ply`** for authoring/round-trip; glTF `KHR_gaussian_splatting` for scene-level interchange.
`bevy_gaussian_splatting` loads `.ply`/`.gcloud`/glTF natively and ships a `.ply → .gcloud`
converter — that pipeline is free.

> **`.spz` is NOT yet supported by the crate** (it is an unchecked roadmap item — no `io_spz`
> feature, no loader). Three ways to use `.spz` scenes: **(a)** convert `.spz → .ply/.gcloud`
> offline (Niantic's `spz` tool or `wgpu-3dgs-viewer`, then load the result) — recommended and
> available today; **(b)** read `.spz` via `wgpu-3dgs-viewer` if we ever go the custom-`ViewNode`
> route; **(c)** add an `io_spz` loader to the crate (the format is small and the crate is
> MIT/Apache — a good upstream contribution). For now, prefer `.gcloud` for shipped/web scenes.

---

## 4. SOTA & variants worth tracking (2024–2026)

| Variant | Venue | What it changes | Why we care |
|---|---|---|---|
| **2DGS** (surfels) | SIGGRAPH 2024 | collapse one scale axis → oriented planar disks with well-defined normals; depth-distortion + normal-consistency regularizers | **surface reconstruction / meshing** (TSDF) → bridge GS back to editable meshes / Avian colliders. `surfsplatting.github.io` |
| **Mip-Splatting** | CVPR 2024 (best-student HM) | 3D Nyquist smoothing filter + 2D Mip box filter replacing the ad-hoc 0.3 dilation | **alias-free at arbitrary render resolution** — essential for an editor viewport with free zoom and a web target. `autonomousvision/mip-splatting` |
| **gsplat** (nerfstudio) | library | CUDA+PyTorch rasterizer; ~4× less train memory, ~15% faster; bundles absgrad/AA/MCMC/2DGS/depth | the reference **training** backend if/when we train (Python, not in-engine). Apache-2.0 |
| **3DGS-MCMC** | NeurIPS 2024 (spotlight) | reframes splats as MCMC samples; SGLD noise + relocation instead of clone/split | **near-immunity to initialization** + exact splat-count budget — better quality, predictable memory. Default densifier in gsplat. `ubc-vision/3dgs-mcmc` |
| **3DGUT** (NVIDIA) | **CVPR 2025 Oral** | replace EWA Jacobian projection (§2.2b) with the **Unscented Transform** (sigma points through exact nonlinear projection) | see below — the natural pairing for PPISP |
| **3DGRT** (NVIDIA) | SIGGRAPH Asia 2024 | BVH over Gaussians, **ray-trace** on RT cores; secondary rays (reflections/refraction/shadows) | needs OptIX/RT hardware; pairs with 3DGUT in the `3dgrut` repo |

### 4.1 Why 3DGUT matters for Eustress (the PPISP pairing)

3DGUT (`research.nvidia.com/labs/toronto-ai/3DGUT`; code `nv-tlabs/3dgrut`) **stays a
rasterizer** but swaps the per-camera affine Jacobian for the Unscented Transform: each Gaussian
is represented by a few **sigma points**, pushed through the **exact** projection, and the 2D
Gaussian is recovered from the transformed points. Because no per-camera Jacobian is derived,
it **supports any camera model trivially** — fisheye, strong distortion, and **time-dependent
rolling shutter** — while remaining fast (beats FisheyeGS with 0.38M vs 1.07M Gaussians). Its
rendering formulation is **aligned with 3DGRT**, enabling a hybrid: **primary rays splatted
(3DGUT), secondary rays ray-traced (3DGRT)** in one representation.

For PPISP, 3DGUT is the right target because (a) it handles arbitrary/physical camera models
without rectification — matching real sensor capture, and (b) it is the same NVIDIA pipeline
family (3DGRT+3DGUT shipped together) that PPISP is designed to plug into. **Plan: standardize
on the vanilla EWA path first (what every Rust crate ships), keep the projection step modular,
and reserve 3DGUT as a swappable projection backend** once a Rust/wgpu implementation exists
(none does yet — this is custom work or a Python-side dependency).

---

## 5. "Does it work with Bevy?" — ecosystem evidence

**Yes.** The decisive fact:

### 5.1 `bevy_gaussian_splatting` (mosure) — the drop-in answer

| Property | Value |
|---|---|
| Latest | **8.0.1**, published **2026-06-25** |
| Bevy | **`^0.19.0`** (the v8.x line) |
| wgpu | **`^29`** |
| License | MIT/Apache (dual) |
| Maturity | ~270★, tracks each Bevy release within days/weeks |

Version → Bevy map (from README): **8.0 → 0.19**, 7.0 → 0.18, 6.0 → 0.17, 5.0 → 0.16,
3.0 → 0.15, 2.3 → 0.14, 2.1 → 0.13, 0.4–2.0 → 0.12, 0.1–0.3 → 0.11.

**What already works:**
- **Rendering:** instanced-quad rasterization (vertex shader expands each splat to a
  screen-space oriented quad; fragment shader evaluates the 2D Gaussian weight, premultiplied
  output). Runs on **both WebGL2 and WebGPU**.
- **Sorting (unusually complete — `src/sort/`):** four shipped backends in default features —
  `radix.wgsl` (GPU radix, production), `bitonic.wgsl` (GPU, WebGL2-friendlier), `rayon.rs`
  (CPU multithreaded), `std_sort.rs` (CPU fallback). A `temporal.wgsl` (amortize resort across
  frames) exists in-tree but is **WIP — not a default feature** ("temporal depth sorting" is
  unchecked on the roadmap). The hard part (a working GPU radix sort) is solved.
- **Loading:** `.ply` (via `ply-rs`), native `.gcloud` (f16/f32, flexbuffers/bincode2), glTF
  `KHR_gaussian_splatting` scene load/save, plus a `.ply → .gcloud` converter. **No `.spz`
  loader yet** (roadmap — see §3.3).
- **Modes:** 2D/3D/4D Gaussians (4DGS = animated clouds), gaussian particle effects,
  depth colorization, normal rendering, f16/f32, raycast/select spatial queries.
- **Roadmap / NOT yet shipped:** SH Huffman + clustering compression, LOD, lighting & shadows,
  OpenXR, skeletons, deformable radial kernel, 4DGS motion blur, temporal depth sort, `.spz` io.

**What is missing / caveats:**
- **Nightly Rust** historically required for default features (`nightly_generic_alias` / GATs).
  **Verify against our stable toolchain (MSRV 1.95)**; if it still needs nightly, disable that
  default feature or gate it. This is the #1 adoption check.
- docs.rs shows 0% documented — API discovery via examples/source.
- ~80 open issues; several features (LOD, lighting/shadows, OpenXR, 4DGS motion blur, SH
  compression, `.spz` io) are **roadmap — not yet shipped** (see the roadmap list above).
- It **owns its own render pipeline and transparency ordering**. Compositing GS *alongside*
  Bevy's opaque/transparent phases for **mixed GS+mesh scenes** needs care (§6.2, §8).

### 5.2 The rest of the Rust ecosystem

| Crate | Role | Bevy? | Use to us |
|---|---|---|---|
| **`wgpu-3dgs-viewer`** v0.7.0 (wgpu `^29`) | clean standalone wgpu lib: modular `RadixSorter`, indirect-draw instancing, `.ply`+`.spz` | no | **best reference if we go custom** — same wgpu 29 surface as Bevy 0.19; its sorter/preprocess pass are liftable into a `ViewNode` |
| **`brush`** v0.3.0 (~4.8k★) | cross-platform 3DGS **trainer**+viewer; Burn→CubeCL→WGSL; egui app; **in-browser training** | no | a *trainer*. Renderer entangled with Burn autodiff — "not easily reusable independently." Run as a separate tool, import its `.ply`/`.spz` |
| `gausplat` | experimental wgpu train+render lib | no | secondary training reference |
| `Lichtso/splatter` | small standalone wgpu engine, onesweep radix sort | no | algorithm reference only (early-stage) |
| `thomasantony/splat` | CPU viewer | no | educational only |

---

## 6. "Can it be made to work?" — concrete custom integration (Bevy 0.19 / wgpu 29)

This is the design if we **reimplement** (build path B). It is real, well-trodden plumbing —
exactly the plumbing `bevy_gaussian_splatting` already contains.

### 6.1 Pipeline shape

```
main world:  GaussianCloud asset (handle on a Camera3d / entity)
   │  ExtractComponent / ExtractResource
   ▼
render world (RenderApp):
   Prepare:  upload SoA storage buffers (RenderDevice.create_buffer_with_data, STORAGE)
   Queue:    bind groups + specialize pipelines (PipelineCache)
   ViewNode::run (per camera, in Core3d subgraph):
     (1) compute pass — PREPROCESS: project μ, build Σ′ + conic, SH→RGB, cull, emit depth keys
     (2) compute pass — RADIX SORT by depth key  (wgpu_sort / lifted RadixSorter)
     (3) render pass  — instanced quads, premultiplied front-to-back alpha blend
```

### 6.2 Bevy 0.19 render-graph integration points (real APIs)

- Sub-app: `app.get_sub_app_mut(RenderApp)` (note: wgpu-29 / Bevy-0.19 `SystemState::get_mut`
  now returns `Result`, and `WgpuSettings` is boxed — see `BEVY_019_MIGRATION.md`; the same
  era of API churn applies to render-node code).
- Node: implement a **`ViewNode`** (modern per-view form) run by **`ViewNodeRunner`**:
  ```rust
  render_app
      .add_render_graph_node::<ViewNodeRunner<GaussianNode>>(Core3d, GaussianNodeLabel)
      .add_render_graph_edges(Core3d, (Node3d::EndMainPass, GaussianNodeLabel, Node3d::Tonemapping));
  ```
  Order it after the opaque main pass (so it can read the depth buffer and occlude/blend
  against meshes) and before tonemapping — or register it as a `Transparent3d` consumer.
- Pipelines: `SpecializedRenderPipeline` + `SpecializedComputePipeline` with `PipelineCache`,
  specialized on MSAA / HDR / SH degree. Mind the **wgpu 29 `RenderPipelineDescriptor` field
  changes** the branch already hit (`f4bac72e`).
- Data movement: `ExtractComponent` (or `ExtractResource`) for the cloud handle; a custom
  `Asset` (the cloud) + `RenderAssets` for GPU-buffer prep in the **Prepare** set; **Queue**
  set to bind + dispatch.
- Blending: `BlendState` premultiplied — pre-multiply α in the shader, `src = One`,
  `dst = OneMinusSrcAlpha`; or composite in a compute pass writing a storage texture.

### 6.3 The GPU radix sort (the hard part) — WGSL

GS is back-to-front alpha compositing, so **every camera move re-sorts all splats by
view-space depth**. CPU sorting stalls at scale; the production answer is a **multi-pass WGSL
radix sort** — typically four 8-bit passes (radix 256) over a 32-bit depth key, each pass a
count → scan → scatter pipeline.

**Genuinely hard in Rust/WebGPU:** the portable WebGPU baseline has **no subgroup/wave
intrinsics**, so the optimal CUDA-style "onesweep" / warp-cooperative tile load is unavailable
— you fall back to multi-pass histogram radix or bitonic. There are also **no native f32
atomics** on some backends. If we target native Vulkan/DX12 only (not web), wgpu's subgroup
features can be enabled where supported.

**Do not reimplement this from scratch.** Depend on / vendor **`wgpu_sort`** (docs.rs) — a port
of the Fuchsia/Vulkan onesweep sort, purpose-built for sorting splats by depth; it is also where
`web-splat`'s sort originated. Or lift `wgpu-3dgs-viewer`'s `RadixSorter` (plain wgpu 29).

### 6.4 Asset loader

- `.ply` → depend on **`ply-rs`** (parse), then transform: `exp(scale)`, normalize `q`, keep
  SH raw, `sigmoid(opacity)` (or keep logit and apply in shader).
- `.splat` → `bytemuck`-cast 32-byte records directly (§3.3).
- `.spz` → port Niantic's small decoder (or shell out); `.sog` → decode textures with `image`
  + `jxl`.
- Wire as a Bevy `AssetLoader` producing a custom `GaussianCloud` asset.

### 6.5 Viewport + editor integration

- The editor viewport is already a Bevy `Camera3d`. A GS cloud is an entity carrying the cloud
  handle; the `ViewNode` runs per camera, so editor + game cameras both render it.
- Selection/inspection: surface the cloud as a Part-like entity in Explorer/Properties (the
  single polymorphic inspector) — splat count, bounds, source format, SH degree as read-only
  properties; transform editable (the cloud has a `Transform`).

### 6.6 Build-vs-buy decision

| Option | Effort | When |
|---|---|---|
| **(a) Adopt `bevy_gaussian_splatting` 8.0.1** | `Cargo.toml` line + plugin reg + nightly check | **default — it is already on Bevy 0.19/wgpu 29** |
| (b) Custom `ViewNode` (lift `wgpu-3dgs-viewer` sorter + `wgpu_sort`) | weeks | only if mixed GS+mesh compositing inside our graph, or nightly/feature conflict, forces it — **don't start blank** |
| (c) Embed `brush` renderer | research-grade | only if we must **train** splats in-engine; otherwise run brush as a separate tool |

**Recommendation: (a)**, wrapped in our own crate (§7) so we control the seam and can swap to
(b) later without touching call sites.

### 6.7 Depend vs reimplement — summary

- **Depend / vendor (do not reimplement):** `wgpu_sort` (or the crate's `radix.wgsl`),
  `ply-rs`, `glam` (Bevy already uses it), `bytemuck` + `encase` (POD + std430 layout),
  `image`/`jxl` for `.sog`.
- **Reimplement only:** the projection/EWA compute shader, conic + alpha-composite blend, tile
  -key generation, and the Bevy render-graph wiring — and only on build path B.

---

## 7. Proposed Eustress crate layout

Create a new leaf crate **`crates/radiance`** (name reflects "radiance field," leaving room for
NeRF/future capture methods; `crates/splat` is the narrower alternative).

```
crates/radiance/
  Cargo.toml          # optional-dep on bevy_gaussian_splatting; feature `gaussian-splatting`
  src/
    lib.rs            # RadiancePlugin (re-exports / wraps the upstream plugin)
    asset.rs          # GaussianCloud asset facade + loader registration
    component.rs      # SplatCloud component (handle + display metadata for Properties)
    inspector.rs      # Properties/Explorer integration (polymorphic inspector)
    convert.rs        # ply/spz import helpers; convert-to-.gcloud
    (custom/          # build path B only: view_node.rs, preprocess.wgsl, blend.wgsl, sort.rs)
```

**How it plugs in:**
- `engine` depends on `radiance` behind a cargo feature **`gaussian-splatting`** (default-off
  initially; flip on once validated). `RadiancePlugin` is added in the engine's plugin set,
  same as `LightClassPlugin` etc.
- `common` gains a `SplatCloud` class-schema entry + `_instance.toml` shape so clouds round-trip
  through `instance_create::create_instance` and the WorldDb (handle/path + transform +
  source-format metadata), consistent with the canonical instance-create convention.
- Math via `glam` (shared with Bevy/Avian). No new ML deps (no burn/candle/cubecl) on the
  inference path — keeps the workspace lean.

**Co-existence with Avian physics:** GS clouds have **no intrinsic collision**. Default: a cloud
is **visual-only**, no `RigidBody`/`Collider` — it renders, physics ignores it. For collidable
scanned environments, derive geometry separately: **2DGS → mesh extraction (TSDF)** offline →
import as a standard mesh with an Avian collider. Never attach a collider to the splat cloud
itself.

**Co-existence with the Bevy renderer:** the `ViewNode` (ours or upstream's) runs in the
existing `Core3d` subgraph after the opaque main pass, sharing the depth buffer so authored
meshes occlude/blend correctly. Tonemapping/HDR/MSAA are specialization inputs. No fork of
Bevy's core renderer.

---

## 8. Performance & scaling

| Cost driver | Behavior | Mitigation |
|---|---|---|
| **Per-frame depth sort** | O(N) GPU radix on every camera move; dominates frame time | GPU `radix.wgsl` (shipped); `temporal.wgsl` resort-amortization once it lands (WIP); native subgroups where available |
| **Per-instance buffers** | pos + cov(scale+rot) + 48 SH floats + opacity per splat; millions of splats = large VRAM + bandwidth | **f16 `.gcloud` storage** (shipped); SH Huffman + clustering compression are roadmap; `.spz`/`.sog` compression via offline conversion |
| **Overdraw** | rasterization is fill-bound; large/overlapping splats compound | early-out at `T < 0.0001`; pruning at train time; Mip filter to bound footprint |
| **10M-splat scenes** | will not live as one live draw | **persistence + streaming**, not live ECS |

**Relate to existing engine work.** This mirrors `SCALING_ARCHITECTURE.md`: 10M is a
**persistence problem, not a live-ECS problem** — stream ≤100K-ish splats by **Morton/spatial
locality**, GPU-driven cull. GS clouds slot into the same **HLOD/streaming** machinery: a cloud
is a streamable spatial asset; far regions get **decimated/quantized proxies** (`.sog`/`.spz`
LOD tiers) and near regions get full-resolution splats, exactly like the merged-cell decimated
mesh proxies in the HLOD whole-map work. The cloud's bounds feed the existing visibility/cull
toggling.

**Risks:**
1. **Nightly toolchain** requirement of the crate vs our stable MSRV 1.95 — verify first.
2. **Mixed GS+mesh compositing** — depth/order correctness against Avian-driven authored
   geometry; the crate owns its transparency order, so layering needs validation.
3. **Web target** — WebGPU lacks subgroups (slower sort); pick `bitonic` for WebGL2,
   `radix` for WebGPU. SH/`.spz` decode cost on load.
4. **No 3DGUT in Rust yet** — physical-camera/PPISP path is custom or Python-side until then.
5. **Training is hard** — keep it out of the engine (run gsplat/brush as tools).
6. **License** — crate is MIT/Apache (fine). The **INRIA reference impl + trained scenes from
   it carry a non-commercial research license** — relevant only if we use their assets/code, not
   the Rust crates. Track per-asset provenance.

---

## 9. Phased implementation roadmap

| Phase | Milestone | Deliverable | Gate |
|---|---|---|---|
| **0 — Spike** | crate builds on stable | `bevy_gaussian_splatting = "8"` compiles against `feat/bevy-0.19` on MSRV 1.95 (resolve nightly feature) | renders the demo `.ply` in a throwaway bin |
| **1 — Viewer** | `crates/radiance` + `RadiancePlugin` | a `.ply`/`.gcloud` cloud renders in the editor viewport behind authored meshes, correct depth | a scanned scene visible + camera orbit, no z-fighting vs a mesh |
| **2 — Loader** | asset pipeline | native `.ply`/`.gcloud`/glTF `AssetLoader` + `.ply → .gcloud` convert; decide the `.spz` path (offline convert vs new `io_spz` loader) | native formats load + round-trip through WorldDb instance; `.spz` decision made |
| **3 — Sorter validated** | scale + perf | profile millions of splats; choose radix vs temporal vs bitonic per platform (microprofiler only — per measurement-rig rules) | 60 FPS target on a real captured scene at production splat count |
| **4 — Editor integration** | first-class object | `SplatCloud` in Explorer/Properties (polymorphic inspector), transform gizmo, instance_create + class-schema, HLOD/streaming hookup | create/move/select/persist a cloud via normal editor flow |
| **5 — Trainer (optional)** | capture-to-scene | run **brush/gsplat as an external tool**; ingest output `.ply`/`.spz`; optional 3DGUT projection backend if a Rust impl lands | photos → splats → imported, end to end |

Build path B (custom `ViewNode`) only branches off after Phase 3 **if** a gate there fails.

---

## 10. Open questions / decisions for the team

1. **Stable vs nightly:** does `bevy_gaussian_splatting` 8.0.1 still need nightly
   (`nightly_generic_alias`) under our default features? If yes — disable that feature, or
   accept it on the GS-only feature gate? (Phase 0 blocker.)
2. **Crate name:** `crates/radiance` (room for NeRF/other capture) vs `crates/splat` (narrow,
   honest)?
3. **Mixed scenes:** is GS a **visual-only backdrop** layer, or must splats interleave per-pixel
   with authored meshes in one depth-sorted pass? (Drives §6.2 node ordering and §8 risk 2.)
4. **Web delivery format:** default to **`.gcloud`** (native, compact f16) for shipped scenes
   now; decide whether to invest in `.spz` (offline convert vs an `io_spz` loader contribution)
   for ~10× smaller web payloads, and confirm decode cost on WebGPU/WebGL2.
5. **Physics policy:** lock "splat clouds never carry Avian colliders; collidable scans go
   through 2DGS→mesh extraction." Agreed?
6. **Training in-engine vs as-a-tool:** confirm we keep training out of the engine (brush/gsplat
   external) for the foreseeable future.
7. **3DGUT priority:** is the PPISP/physical-camera path near-term enough to fund a Rust/wgpu
   3DGUT projection backend, or do we depend on a Python-side pipeline until upstream Rust
   support exists?
8. **Streaming integration:** do GS clouds reuse the HLOD merged-cell/proxy machinery directly,
   or get a parallel splat-LOD path (`.sog`/`.spz` tiers)? Prefer reuse.

---

### Sources

- Kerbl et al., *3D Gaussian Splatting*, SIGGRAPH 2023 — arXiv:2308.04079; INRIA project page;
  `graphdeco-inria/gaussian-splatting`; `diff-gaussian-rasterization` (`forward.cu`,
  `auxiliary.h`).
- Formats: antimatter15/`splat` (`.splat`); Niantic `spz`; Self-Organizing Gaussians (SOG,
  SIGGRAPH 2024); Swyvl "Gaussian Splat Formats: PLY, SOG, SPZ, KSplat."
- Variants: 2DGS (`surfsplatting.github.io`, SIGGRAPH 2024); Mip-Splatting (CVPR 2024,
  `autonomousvision/mip-splatting`); gsplat (`docs.gsplat.studio`, nerfstudio); 3DGS-MCMC
  (NeurIPS 2024, arXiv:2404.09591, `ubc-vision/3dgs-mcmc`); 3DGUT (CVPR 2025 Oral,
  `research.nvidia.com/labs/toronto-ai/3DGUT`, `nv-tlabs/3dgrut`); 3DGRT (SIGGRAPH Asia 2024).
- Rust/Bevy: `bevy_gaussian_splatting` v8.0.1 (`github.com/mosure/bevy_gaussian_splatting`,
  crates.io); `brush` v0.3.0 (`github.com/ArthurBrussee/brush`, DeepWiki); `wgpu-3dgs-viewer`
  v0.7.0 (docs.rs); `wgpu_sort` (docs.rs); `web-splat` (KeKsBoTer); `gausplat`; `Lichtso/splatter`.
- Bevy render graph: `bevy_render::render_graph` docs; Bevy discussion #9897.
- Eustress: `BEVY_019_MIGRATION.md`, `SCALING_ARCHITECTURE.md`.
