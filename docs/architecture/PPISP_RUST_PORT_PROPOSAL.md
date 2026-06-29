# PPISP: A Fully-Rust Port Proposal for Eustress

> Canonical architecture proposal. Audience: senior Rust / Bevy / wgpu systems engineers.
> Status: **PROPOSAL** (no code landed). Branch context: `feat/bevy-0.19` (Bevy 0.19, wgpu 29.0.3, Avian 0.7, edition 2024, MSRV 1.95).
> Companion docs: [`SCALING_ARCHITECTURE.md`](./SCALING_ARCHITECTURE.md). A Gaussian-Splatting / radiance-field reconstruction doc (referred to below as the **GS doc**) does not yet exist; PPISP is the photometric front-half of that future pipeline and this proposal assumes it will be created alongside.

---

## 0. Provenance and honesty note

This proposal is built from a full extraction of NVIDIA's PPISP reference (Apache-2.0, `github.com/nv-tlabs/ppisp`, paper arXiv:2601.18336, Deutsch et al. 2026). The math in §3–§4 is taken from a **reconciled, verified spec** across six independent source extractions (PyTorch reference + tests, the CUDA forward kernel, the CUDA backward kernel + ABI, the PyTorch front-end module, the paper, and `report.py`).

A handful of facts rest on `.cu`/`.cuh` lines that were **not quoted verbatim** in any single extraction. Every such item is flagged inline as **`MUST CONFIRM`** with the exact source location. None of them block the recommended first milestone (an inference-only forward port); they matter only for bit-exact training parity. Do not treat a `MUST CONFIRM` item as settled until the named file is diffed.

The four ZCA constants, the homography construction, and the CRF curve are **NVIDIA-authored Apache-2.0 math** — porting them carries an attribution obligation (see §7).

---

## 1. Executive summary

PPISP ("Physically-Plausible ISP," per the paper title and abstract) is a small, differentiable, physically-grounded camera-ISP model — four transforms (per-frame **exposure**, per-camera chromatic **vignetting**, per-frame **color/white-balance homography**, per-camera **CRF tone curve**) applied to rendered radiance and **jointly optimized with a radiance-field reconstruction**. It is not a renderer or a splatting method; it is the photometric-consistency layer that makes real multi-camera capture usable for 3D reconstruction, plus a tiny per-camera CNN "controller" that predicts per-frame parameters at novel views (an auto-exposure / auto-white-balance analog) so novel-view rendering can be evaluated fairly without leaking ground-truth pixels.

**The recommended fully-Rust approach** is a new `crates/ppisp` workspace member that shares Bevy's existing `wgpu::Device`/`Queue` (no second GPU context, no CUDA, no PyTorch, no Python). Ship it in two layers: (1) a pure-Rust, scalar-generic **CPU reference** (`f32`/`f64`) that mirrors `torch_reference.py` exactly and serves as the correctness oracle; (2) a **wgpu 29 compute path** for production. For the four transforms and the regularization reduction, **hand-write WGSL forward + backward kernels** sharing Bevy's `RenderDevice` — autodiff frameworks buy nothing here because every adjoint is already closed-form in the reference and the CRF backward contains an *intentional* gradient truncation that a generic autodiff engine would get "correct" and therefore wrong. Use **`burn` 0.21 on its `Wgpu`/`cubecl` backend** only for the controller MLP (where real autodiff helps) and as the `NdArray<f64>` reference backend for gradient checks. Train offline, export the four parameter tensors + controller weights, and load them into a Bevy `ViewNode` post-process for inference — the common case inside the engine.

---

## 2. What PPISP is and why Eustress wants it

### 2.1 The thing it is (and the thing it is not)

PPISP is a **differentiable post-render image-signal-processor**. It takes the raw radiance `L` produced by a radiance-field renderer and applies, in a fixed order, the four operations a real camera applies between sensor irradiance and a stored pixel:

```
I = G( C( V( E(L; Δt); μ,α ); {Δc_k} ); τ,η,γ,ξ )
       └CRF┘ └color┘ └vignette┘ └exposure┘
```

It is **NOT**:

- a Gaussian-Splatting or NeRF method (it has no geometry, no Gaussians, no rays);
- a renderer (it never produces radiance, only transforms it);
- a tonemapper in the Bevy sense (although its CRF *is* a learned tonemap and will likely replace/precede Bevy's built-in tonemapping for views it owns).

It **pairs with** a Gaussian-Splatting trainer (the future GS doc): GS produces radiance `L` from a set of Gaussians; PPISP models the per-frame/per-camera photometric variation between the training photos so the GS optimizer is not forced to bake exposure jumps, vignettes, and white-balance drift into geometry and color.

### 2.2 Why Eustress wants it

Eustress is greenfield for radiance fields — there is no GS/NeRF/point-cloud code anywhere in the workspace today. When that capability arrives, the dominant real-world input will be **multi-camera video of physical scenes** (the V-Supreme / Voltec capture program, site scans for the CAD platform, climate/field data capture for the Data Platform). Such captures are photometrically inconsistent by construction: different cameras, auto-exposure swings, per-lens vignetting, per-frame white balance. Without compensation, the reconstruction optimizer treats those inconsistencies as scene content — "spurious color shifts and geometric artifacts" (paper §1).

Concrete benefits:

1. **Clean, photometrically consistent splat captures** from real multi-camera video — the reconstruction sees a consistent target and spends its capacity on geometry/radiance, not on hiding ISP variation.
2. **Disentangled, interpretable controls.** Exposure is exactly "EV stops," vignetting is "radial falloff about an optical center," color is "white-balance + chromatic cross-talk," CRF is "toe/shoulder/gamma." These are editable in the Studio inspector (§5.f), unlike opaque GLO latents or bilateral grids.
3. **Fair novel-view rendering.** The controller predicts per-frame exposure + color at viewpoints that have no training frame, mimicking a real camera's AE/AWB — so the engine can render a novel pose photorealistically without the GT-leaking "corrective map" the field normally uses for evaluation.
4. **Cheap.** Reported runtime on an RTX 5090 at Mip-NeRF 360 resolution: 0.10 ms without the controller (~3% over a 3.24 ms base render), 0.84 ms with it (~26%) — cheaper than BilaRF's bilateral grid (1.17 ms).

Headline accuracy (paper Table 1, raw novel-view PSNR, "3DGUT + PPISP w/ controller"): BilaRF dataset 24.12, Mip-NeRF 360 28.15, Tanks & Temples 24.62, Waymo 25.69, PPISP-Auto 22.87 — SoTA raw PSNR on all five, with the controller closing the gap to the privileged GT-aligned metric.

---

## 3. The algorithm, precisely

All tensors are `float32`, contiguous, single CUDA device in the reference. One kernel launch processes **one `(camera, frame)` image**: `camera_idx` and `frame_idx` are scalar arguments, not per-thread. Sentinels: `frame_idx == -1` disables exposure + color; `camera_idx == -1` disables vignetting + CRF.

**Color space:** the pipeline is **linear-light throughout**. The CRF is the only nonlinearity. sRGB encode/decode is a *viewer/boundary* concern only — `report.py` applies inverse-sRGB purely for matplotlib display, never as part of the pipeline. (Reconciled across all six sources; settled by `report.py` `_apply_crf` vs `_srgb_inverse_oetf` call sites.)

### 3.1 Parameter shapes and parameterization

| Paper | Code attribute | Shape | Layout / parameterization | Identity init |
|---|---|---|---|---|
| Δt (§4.1) | `exposure_params` | `[F]` | per-frame scalar, **log2 / EV stops**, additive in log space | `zeros` ⇒ gain `2^0 = 1` |
| μ,α (§4.2) | `vignetting_params` | `[C, 3, 5]` | per camera × RGB channel: `[ox, oy, a0, a1, a2]` (optical-center offset + 3 even-radial coeffs) | `zeros` ⇒ centered, falloff 1 |
| {Δc_k} (§4.3) | `color_params` | `[F, 8]` | per frame: 4 latent `(dr,dg)` pairs ordered **`[Blue, Red, Green, Neutral]`** = `[db_r,db_g, dr_r,dr_g, dg_r,dg_g, dgray_r,dgray_g]` (`dgray` = white point) | `zeros` ⇒ identity homography |
| τ,η,γ,ξ (§4.4) | `crf_params` | `[C, 3, 4]` | per camera × channel: `[toe_raw, shoulder_raw, gamma_raw, center_raw]` (raw = pre-activation) | `softplus_inverse` so toe≈shoulder≈gamma≈1, center=0.5 |

`softplus_inverse(x, min, eps=1e-5) = log(expm1(max(eps, x - min)))`. CRF raw inits: toe/shoulder = `softplus_inverse(1.0, 0.3)`, gamma = `softplus_inverse(1.0, 0.1)`, center_raw = 0.

### 3.2 Composition order (unanimous, load-bearing)

**Exposure → Vignetting → Color → CRF**, each reading the previous `rgb` in place. Getting this order wrong makes every learned checkpoint meaningless.

### 3.3 Forward formulas

**Exposure** (per-frame, gated `frame_idx != -1`):
```
rgb_out = rgb_in * exp2(exposure_params[frame_idx])      # no clamp
```

**Vignetting** (per-camera, per-channel, gated `camera_idx != -1`). Normalize pixel coords by the **max** dimension (kernel-authoritative; aspect-preserving):
```
max_res = max(W, H)
uv = ((px - 0.5*W)/max_res, (py - 0.5*H)/max_res)        # center→0, square corner→±0.5
# per channel ch with [ox,oy,a0,a1,a2]:
d  = uv - (ox, oy)
r2 = d.x*d.x + d.y*d.y;  r4 = r2*r2;  r6 = r4*r2
falloff = clamp(1 + a0*r2 + a1*r4 + a2*r6, 0, 1)         # even poly, Horner via FMA
rgb_out[ch] = rgb_in[ch] * falloff
```
> **MUST CONFIRM (low risk):** `report.py` builds its preview grid as `linspace(-0.5,0.5)` (unit-half square), *not* max-dim normalized. The kernel (`ppisp_math.cuh::apply_vignetting`) is authoritative; `report.py` is a square-preview convenience. Confirm `ppisp_math.cuh` uses `max(W,H)`.

**Color homography** (per-frame, gated `frame_idx != -1`). Build `H` from the 8 latents, then apply in RGI (Red, Green, Intensity) space.

*Build (paper Eq. 9–12):*
```
offset_k = ZCA_block_k · latent_k                 # k ∈ {Blue, Red, Green, Neutral}
# source chromaticities (homogeneous): s_b=(0,0,1) s_r=(1,0,1) s_g=(0,1,1) s_gray=(1/3,1/3,1)
t_k = s_k + (offset_k.x, offset_k.y, 0)           # targets
T = [t_b | t_r | t_g]                             # targets as COLUMNS
skew(t_gray) = [[0,-tg.z,tg.y],[tg.z,0,-tg.x],[-tg.y,tg.x,0]]
M = skew(t_gray) · T
lam = nullspace(M)                                # via row-pair cross products (see DISCREPANCY)
S_inv = [[-1,-1,1],[1,0,0],[0,1,0]]               # fixed constant
H = T · diag(lam) · S_inv
H = H / H[2,2]                                     # normalize so H[2,2] ≈ 1
```
The four symmetric 2×2 ZCA blocks (bake verbatim, Apache-2.0):

| Point | m00 | m01 | m10 | m11 |
|---|---|---|---|---|
| Blue | 0.0480542 | -0.0043631 | -0.0043631 | 0.0481283 |
| Red | 0.0580570 | -0.0179872 | -0.0179872 | 0.0431061 |
| Green | 0.0433336 | -0.0180537 | -0.0180537 | 0.0580500 |
| Neutral | 0.0128369 | -0.0034654 | -0.0034654 | 0.0128158 |

*Apply (paper Eq. 7–8):*
```
intensity = r + g + b                             # SUM of channels (NOT luma)
rgi_in  = (r, g, intensity)
rgi_out = H · rgi_in
norm    = intensity / (rgi_out.z + 1e-5)
rgi_out *= norm
r_out, g_out = rgi_out.x, rgi_out.y
b_out = rgi_out.z - r_out - g_out                 # B = I − R − G
```

> **DISCREPANCY — nullspace selection (MUST CONFIRM, training-relevant).** The PyTorch reference (`torch_reference.py:41-116`) picks the row-pair cross product with **largest squared magnitude** (branchless argmax). The CUDA kernel (per the forward/backward extractions) uses a **threshold-fallback**: `cross(r0,r1)`; if `‖·‖² < 1e-20` use `cross(r0,r2)`; else `cross(r1,r2)`. These diverge only at near-degenerate configs but produce *different* gradients there, and the CUDA **backward recomputes the same conditional branch it used in forward** — which only makes sense for the threshold form. **For training parity, replicate the threshold-fallback (CUDA form).** Confirm by diffing `ppisp_math.cuh::compute_homography` against `torch_reference.py:41-116`. If they genuinely differ, the kernel wins for matching the trained optimizer; surface it in the validation suite with a near-degenerate golden vector.

> **DISCREPANCY — H normalization eps (MUST CONFIRM, negligible).** Reference: `H / (H[2,2] + 1e-10)`. Kernel: guarded scale `if |s|>1e-20: H *= 1/s`. Differs only when `H[2,2]≈0`. Use the kernel form for production.

**CRF** (per-camera, per-channel, gated `camera_idx != -1`). Input clamped to `[0,1]` first.
```
toe      = 0.3 + softplus(toe_raw)
shoulder = 0.3 + softplus(shoulder_raw)
gamma    = 0.1 + softplus(gamma_raw)
center   = sigmoid(center_raw)
lerp_val = toe + center*(shoulder - toe)
a = (shoulder*center) / lerp_val ;  b = 1 - a
if x <= center:  y = a * clamp(x/center,      min=eps)^toe
else:            y = 1 - b * clamp((1-x)/(1-center), min=eps)^shoulder
out = clamp(y, min=eps)^gamma                     # eps = 1e-6
```
> **MUST CONFIRM (gradient-relevant):** reference uses `clamp(y,1e-6)^gamma` and `eps=1e-6` inside the interior `pow` bases; one extraction of the kernel reports `max(0,y)^gamma`. The `eps` form is **load-bearing for gradients** (avoids the `pow(0,·)` zero/NaN-gradient pathology; the backward uses `ln(y_clamped + 1e-8)`). **Port the `eps=1e-6` clamps.** Confirm `apply_crf_ppisp` in `ppisp_math.cuh`.

### 3.4 Regularization (on the 4 ISP params only; each term gated by weight > 0)

| Term | Weight (default) | Formula |
|---|---|---|
| Exposure mean | `exposure_mean = 1.0` | `w · smooth_l1(mean(exposure_params), 0; β=0.1)` |
| Vig center | `vig_center = 0.02` | `w · mean(sum(oc², dim=-1))`, `oc = vig[:,:,:2]` |
| Vig non-positivity | `vig_non_pos = 0.01` | `w · mean(relu(vig[:,:,2:]))` (linear hinge) |
| Vig channel variance | `vig_channel = 0.1` | `w · mean(var(vignetting_params, dim=1, unbiased=False))` |
| Color mean | `color_mean = 1.0` | `w · smooth_l1(mean(color_params @ ZCA_block_diag, dim=0), 0; β=0.005)` |
| CRF channel variance | `crf_channel = 0.1` | `w · mean(var(crf_params, dim=1, unbiased=False))` |

`smooth_l1(x,β) = 0.5·x²/β if |x|<β else |x|−0.5·β`. **Variances are `unbiased=False`** (divide by N, not N−1) — a singleton dim yields exactly 0. The reference saves `frame_mean_sums` (length `1 + 8 = 9`: Σexposure + 8 color-offset sums) for backward.

> **DISCREPANCY — vig non-positivity hinge (UNRESOLVED, training-relevant).** Code/tests reduce as **linear** `mean(relu(alphas))`; the **paper** §4.6 writes a **squared** hinge `Σ[α_j]_+²` (and `‖μ‖²` for center, which both agree on). The published Eq. and the shipped code cannot both be right. **Trust the code (linear relu)** for matching the trained checkpoint; flag the paper divergence. Settle via `ppisp_impl.cu::ppisp_regularization_camera_param_loss_kernel`.

### 3.5 Controller (per camera; present iff `use_controller`)

Predicts per-frame params from rendered radiance for novel views — 9 outputs (1 exposure + 8 color). Architecture (D and E agree exactly):

```
radiance [H,W,3] → permute→[1,3,H,W] (detached inside controller)
Conv2d(3,16,k=1) → MaxPool2d(3,stride=3) → ReLU
Conv2d(16,32,k=1) → ReLU
Conv2d(32,64,k=1) → AdaptiveAvgPool2d((5,5)) → Flatten   # 64*5*5 = 1600
concat prior_exposure[1]                                  # input_dim = 1601
Linear(1601,128)+ReLU, then 2× (Linear(128,128)+ReLU)
exposure_head = Linear(128,1)→scalar ;  color_head = Linear(128,8)
```

- 1×1 convs ⇒ per-pixel matmuls; the 5×5 pool = "metering zones." Sub-millisecond network.
- **Activation:** at 80% of training (`controller_activation_ratio=0.8`); `_controller_activation_step = int(ratio * max_iters)`.
- **Distillation** (lives in `PPISP.forward`, not the controller): when active, set `requires_grad=False` on all four ISP params and `detach()` the input so gradients flow only into the controller; the scene (Gaussians) should also be frozen.
- **Override path:** `is_novel_view = (camera_idx != -1 AND frame_idx == -1)`. Novel view + controller trained ⇒ run controller, feed its outputs as the 1-row exposure/color tensors with `frame_idx_for_kernel = 0`. Novel view + controller untrained ⇒ zeros (identity). Metadata: concatenating the EXIF relative-exposure prior lifts HDR-NeRF PSNR 17.86 → 34.30 (paper Table 3).

---

## 4. The differentiable-training requirement

Training jointly optimizes the four ISP params with the scene under a photometric loss + `L_reg`. The backward path has two properties that drive the entire Rust GPU design:

1. **No autograd tape — backward recomputes every forward intermediate** from `(params, rgb_in, rgb_out, v_rgb_out)`. The ABI passes `rgb_out` back into backward precisely so the CRF backward can reuse the activated output. Adjoints are all closed-form (exposure: `grad_e = dot(grad_out, rgb_out)·ln2`; vignetting: polynomial in `r2` gated by the clamp; color: the full `compute_homography_bwd` chain reversing ZCA → targets → skew → `M=skew·T` → conditional nullspace → diag → two matmuls → projective normalize; CRF: piecewise power-law adjoints).

2. **Parameter gradients are a scatter-add by index, not by pixel.** Exposure/color accumulate by `frame_idx`; vignetting/CRF by `camera_idx` (×3 channels). Every pixel in the launch `+=` into the *same* tiny param slice (pre-zeroed `zeros_like`). `grad_rgb_in` is the one exception — a unique per-pixel write, no reduction. In CUDA this is `cub::BlockReduce` → one `atomicAdd` per block per param. In WGSL there is **no native f32 atomic** (§5.b) — this is the single structural hazard of the port.

> **INTENTIONAL ASYMMETRY (must replicate exactly).** The CRF backward (`ppisp_math_bwd.cuh` ~lines 921–934) deliberately flows gradients through `a,b` only, **not** through the per-channel divisions `x/center`, `(1−x)/(1−center)` w.r.t. `center` via the base term. This is a *deliberate divergence from the Slang reference*. A faithful port must truncate identically or it will not bit-match the trained optimizer. **This is the decisive reason not to use a generic autodiff engine for the CRF backward** — it would compute the mathematically complete gradient and disagree.

> **NOT DIRECTLY READ (need the `.cu`):** the exact `atomicAdd` accumulation / per-thread→param reduction lives in `ppisp_impl.cu` and was never quoted. The Rust port chooses its own reduction (recommended: two-pass workgroup reduce, §5.b) so this is a design choice, not a transcription.

---

## 5. The fully-Rust solution

### 5.a Crate layout

New workspace member `crates/ppisp`, dependency-light, no CUDA, no PyTorch:

```
crates/ppisp/
  Cargo.toml
  src/
    lib.rs            # PpispModule, PpispConfig, public API (§5.c)
    reference/        # pure-Rust CPU oracle, generic over Scalar: f32|f64
      mod.rs
      exposure.rs vignetting.rs color.rs crf.rs    # forward + hand-written backward
      homography.rs # build H + compute_homography_bwd (glam Mat3/DMat3)
      reg.rs        # 6 regularization terms + backward
    gpu/
      mod.rs        # wgpu pipeline mgmt, shares Bevy RenderDevice/RenderQueue
      buffers.rs    # SoA param layout, encase ShaderType structs
      shaders/
        ppisp_forward.wgsl
        ppisp_backward.wgsl
        reg_reduce.wgsl        # workgroup reduce + 2nd-pass partial sum
    controller/      # burn 0.21 CNN+MLP (train + inference)
    optim/           # Adam + LinearLR→ExponentialLR schedule (hand-rolled or burn)
    report.rs        # plotters + printpdf PDF/JSON export
    constants.rs     # ZCA blocks, S_inv, source chromaticities (Apache-2.0 attribution)
  tests/
    fixtures/        # golden vectors exported from torch_reference.py
    forward.rs reg.rs gradcheck.rs optional_inputs.rs
```

**Dependencies (depend, do not reimplement):**

| Crate | ~Version | Role |
|---|---|---|
| `bevy` | 0.19 | `RenderApp`, `ViewNode`, `ViewTarget`, `RenderDevice`/`RenderQueue`, render graph |
| `wgpu` | 29.0.3 | transitive via Bevy; reuse its device — do not create a second instance |
| `glam` | 0.30 | host-side `Mat3`/`Vec3` (+ `DMat3`/`DVec3` for the f64 reference) |
| `bytemuck` | 1.x | `Pod`/`Zeroable` param structs |
| `encase` | 0.12 | std140/std430 WGSL struct layout (avoids hand-padding vec3/vec2 footguns) |
| `burn` + `burn-wgpu` + `burn-autodiff` | 0.21 | controller MLP train/inference on the shared device; `NdArray<f64>` reference backend for gradcheck |
| `cubecl` | 0.10 | (transitive via burn-wgpu) optional `#[cube]` kernels |
| `approx` | 0.5 | test tolerances |
| `ndarray-npy` *or* `serde_json` | — | golden-vector fixtures |
| `plotters` + `plotters-svg`, `printpdf`, `image` | — | report export (§5.f) |

**Reimplement (small, exact, the core value):** the four transform kernels (forward + backward) in WGSL; the homography build/backward; the regularization reduction; the `softplus_inverse` identity init; the ZCA constant table.

`crates/ppisp` depends on the future GS/radiance crate only at the *integration* seam (it consumes a radiance `ViewTarget` and writes a corrected one). The transform math itself has no GS dependency — `crates/ppisp` is a standalone differentiable ISP that the GS trainer calls.

### 5.b GPU strategy decision

**Recommendation: hand-write WGSL forward + backward compute shaders on wgpu 29, sharing Bevy's `RenderDevice`, for the four transforms and the regularization reduction. Use `burn` only for the controller MLP and as the f64 CPU gradient oracle. Do not use candle.**

Rationale: every adjoint is already closed-form in `ppisp_math_bwd.cuh` — the hard derivation is done, transcription is an afternoon per transform. A generic autodiff layer (burn-autodiff over the transforms expressed as many small tensor ops) would be slower, would not capture the fused semantics, and — critically — would compute the *mathematically complete* CRF gradient, disagreeing with the reference's intentional `a,b`-only truncation (§4). burn's value is real for the controller (a genuine NN where autograd saves work) and as an `NdArray<f64>` reference for gradcheck (§6).

#### Recommendation matrix per component

| Component | (i) hand-WGSL on shared device | (ii) burn + cubecl autodiff | (iii) candle | **Pick** |
|---|---|---|---|---|
| 4 transform **forward** kernels | ideal: per-pixel SIMD map, closed-form | overkill, fusion loss | CPU/Metal only, no wgpu | **(i)** |
| 4 transform **backward** kernels | closed-form adjoints already written; replicates intentional CRF truncation | would compute the *wrong* (complete) CRF grad | no wgpu | **(i)** |
| Reg-loss **fused reduction** | workgroup reduce + 2-pass partials (deterministic, testable) | possible but no win | no wgpu | **(i)** |
| **Controller MLP** (train + infer) | doable but fiddly (1×1 conv = matmul, adaptive pool = reduction) | autograd + `nn` modules, same shared device | tiny MLP ok on CPU but 2nd tensor stack + host hops | **(ii) burn** |
| **Gradcheck oracle** | n/a | `NdArray<f64>` reference backend | candle f64 CPU possible | **(ii) burn NdArray\<f64\>** (or plain Rust f64) |

Device sharing: `burn-wgpu` exposes `WgpuDevice::Existing` + `init_device(WgpuSetup{ instance, adapter, device, queue, .. })`, explicitly intended for "some existing wgpu setup (eg. egui or **bevy**)." Clone the `Arc`s out of Bevy's render world (`RenderDevice::wgpu_device()`, `RenderQueue`) and hand them to burn so PPISP, the controller, and the renderer all live on one `wgpu::Device` — zero cross-context copies.

#### The two WGSL constraints, handled concretely

**No f64 in WGSL.** WGSL has no `f64` type (`SHADER_F64` is SPIR-V/Vulkan-only, not WGSL source). You therefore cannot run a true f64 finite-difference gradcheck on the GPU. Mitigation (matches the Python strategy): run gradcheck against the **CPU f64 reference** (`reference/` generic over `f64`, or burn `NdArray<f64>`) at tight tolerance, and cross-check the **f32 GPU path** against that reference at the loose tolerances the Python suite already uses (`eps=1e-3, atol=5e-3, rtol=5e-2`). Honest risk: catastrophic-cancellation bugs a CUDA f64 gradcheck would catch can hide under f32 tolerance — budget extra validation on the homography backward specifically.

**No native f32 atomics in core WGSL.** Gradient scatter-add by `(camera,frame)` is inherently a float `atomicAdd`, which core WGSL lacks. Three options, in order:

1. **Two-pass deterministic reduction (recommended).** Pass 1: each workgroup reduces its pixels' param-grad contributions in `var<workgroup>` shared memory (`workgroupBarrier()` tree reduce — the hand-rolled replacement for `cub::BlockReduce`) and writes **one partial per workgroup** to a scratch buffer keyed by index. Pass 2: a tiny kernel sums partials into the final grad buffer. No device-wide atomics; deterministic → keeps tight tolerances; matches the kernel's block-reduce structure. `grad_rgb_in` needs no atomics (unique slot).
2. **CAS-loop f32 atomic emulation** on `atomic<u32>` + `bitcast` — correct but contention-heavy exactly in the dense scatter-add regime; keep as portable fallback.
3. **`Features::SHADER_FLOAT32_ATOMIC`** — native f32 `atomicAdd`, but **Vulkan/Metal only, not WebGPU/DX12**. For a desktop Bevy build, request it and fast-path; keep option 1 as the portable default.

```wgsl
// Portable f32 atomic-add fallback (option 2). Prefer the two-pass reduce.
fn atomic_add_f32(p: ptr<storage, atomic<u32>, read_write>, v: f32) {
    var old = atomicLoad(p);
    loop {
        let next = bitcast<u32>(bitcast<f32>(old) + v);
        let r = atomicCompareExchangeWeak(p, old, next);
        if (r.exchanged) { break; }
        old = r.old_value;
    }
}
```

**Matrix major-order + float3 packing footguns.** WGSL `matNxN` are column-major; the CUDA structs are row-major with a custom `operator*` — build matrices column-by-column or transpose indexing in `compute_homography`. Do **not** store `rgb` as `array<vec3<f32>>` (WGSL pads vec3 to 16 B, mismatching packed CUDA `float3[]`); use a flat `array<f32>` indexed `3*tid + {0,1,2}`, or `encase` for the param structs.

### 5.c Data types, buffer layouts, and the Rust API surface

**Parameter SoA, by index (mirrors the torch row-major tensors):**

```rust
// Host-side, encase ShaderType for std430 storage buffers.
#[derive(ShaderType, Clone, Copy)]
struct VignettingChannel { ox: f32, oy: f32, a0: f32, a1: f32, a2: f32 } // [C*3] flat
#[derive(ShaderType, Clone, Copy)]
struct ColorLatent { b: Vec2, r: Vec2, g: Vec2, n: Vec2 }                // [F]  (=8 floats)
#[derive(ShaderType, Clone, Copy)]
struct CrfChannel { toe_raw: f32, shoulder_raw: f32, gamma_raw: f32, center_raw: f32 } // [C*3]
// exposure_params: array<f32> length F
// rgb: flat array<f32> length 3*N ; pixel_coords: array<f32> length 2*N (optional)
```

Indexing matches the CUDA `reinterpret_cast`: vignetting `[camera_idx*3 + ch]`, crf likewise; exposure `[frame_idx]`; color `[frame_idx]`. `H` is built **once per frame on the host** (glam `Mat3`, the nullspace cross-products) and uploaded as a uniform — never per pixel.

**Sentinels:** `Option<u32>` → `-1` (`i32::MAX` sentinel or a `u32` flag uniform) for `camera_idx`/`frame_idx`. The override novel-view path uploads a single-row exposure/color buffer and the shader reads index 0.

**Public API (mirrors the PyTorch `PPISP(nn.Module)`):**

```rust
pub struct PpispConfig { /* use_controller, controller_distillation, controller_activation_ratio,
                            6 reg weights, ppisp_lr=0.002, ppisp_eps=1e-15, betas=(0.9,0.999),
                            controller_lr=0.001, scheduler_warmup=500, decay_max=30000,
                            start_factor=0.01, final_factor=0.01, .. */ }

pub struct PpispModule {
    exposure: Vec<f32>,                  // [F]
    vignetting: Vec<VignettingChannel>,  // [C*3]
    color: Vec<ColorLatent>,             // [F]
    crf: Vec<CrfChannel>,                // [C*3]
    controllers: Vec<Controller>,        // one per camera (burn), empty if disabled
    cfg: PpispConfig,
    gpu: PpispGpu,                        // pipelines + shared RenderDevice handle
}

impl PpispModule {
    pub fn new(num_cameras: usize, num_frames: usize, cfg: PpispConfig) -> Self;
    pub fn from_state(state: &PpispState, cfg: PpispConfig) -> Self; // infer C,F from shapes

    /// Forward over one (camera, frame) image. coords/resolution optional (synth pixel centers).
    pub fn forward(&self, rgb: &RgbBuffer, coords: Option<&CoordBuffer>,
                   resolution: Option<(u32,u32)>,
                   camera_idx: Option<u32>, frame_idx: Option<u32>,
                   exposure_prior: Option<f32>) -> RgbBuffer;

    pub fn regularization_loss(&self) -> f32;        // forward only
    pub fn regularization_backward(&self, v_loss: f32) -> PpispGrads;
    pub fn backward(&self, /* saved fwd tensors */ v_rgb_out: &RgbBuffer) -> (PpispGrads, RgbBuffer);

    pub fn create_optimizers(&self) -> Optimizers;   // §5.d
    pub fn create_schedulers(&self, max_iters: u32) -> Schedulers;
    pub fn export_report(&self, path: &Path);        // §5.f
}
```

Optional-input semantics to preserve (tested): `coords=None` ⇒ synth pixel centers `(x+0.5, y+0.5)`, row-major, stacked `(grid_x, grid_y)`; `resolution=None` ⇒ `(W,H)` from `[H,W,3]` (note the axis swap).

### 5.d Optimizer / scheduler

The reference uses Adam (`lr=0.002, eps=1e-15, betas=(0.9,0.999)`) for the four ISP params as one group, a separate Adam (`lr=0.001`, **no scheduler**) for the controller, and a `SequentialLR`: `LinearLR(start_factor=0.01, total_iters=500)` then `ExponentialLR(gamma = 0.01^(1/30000))`, milestone `[500]`. (`scheduler_base_lr` is a declared-but-dead field — do not read it.)

**Recommendation: hand-roll Adam + the two-phase LR schedule** in `optim/`. It is ~80 lines, has no GPU component for the ISP params (they are tiny `Vec<f32>` updated on the host from the reduced grads), and avoids coupling the ISP optimizer to burn's optimizer API churn. Use **burn's optimizer** only for the controller (it already lives in burn). `eps=1e-15` is unusually small — preserve it exactly; it matters for parity.

### 5.e Controller MLP (burn) + distillation/freeze

Run train + inference in **burn 0.21 on the shared `Wgpu` backend** (same device as PPISP and Bevy). The network is `nn::conv::Conv2d` (1×1) + `MaxPool2d` + `AdaptiveAvgPool2d((5,5))` + `Linear` trunk + two heads — all stock burn modules; autograd is free and correct here (no intentional truncation). Inference is sub-millisecond; for the absolute lowest-friction path it can even run on burn's CPU backend, but the shared `Wgpu` device avoids host↔device hops.

Distillation/freeze logic lives in `PpispModule::forward` (not the controller), exactly as in the reference: when `controller_trained && controller_distillation`, skip ISP-param grad accumulation (the Rust analog of `requires_grad=False`) and detach the radiance input so only controller grads flow. Activation gate: `scheduler.last_epoch() >= controller_activation_step`. Override gate: `is_novel_view || (controller_trained && camera_idx != -1)`.

Do **not** reach for candle (no production wgpu backend; would be a second tensor stack on CPU/Metal) or `ort`/ONNX (heavyweight for a sub-ms net).

### 5.f Integration into Bevy/Eustress

**Inference (the common in-engine case)** — a post-process over a rendered radiance `ViewTarget`:

- A `RenderApp` sub-graph node implementing `ViewNode` (PPISP is per-camera ⇒ `ViewNode` is the natural fit; the camera entity carries `camera_idx`):
  ```rust
  impl ViewNode for PpispNode {
      type ViewQuery = (&'static ViewTarget, &'static PpispViewParams);
      fn run(&self, _g, ctx: &mut RenderContext, (target, params): QueryItem<Self::ViewQuery>, world)
          -> Result<(), NodeRunError> { /* ping-pong via target.post_process_write(); dispatch */ }
  }
  ```
- Register and order: `render_graph.add_render_graph_node::<ViewNodeRunner<PpispNode>>(Core3d, PpispLabel)` then `add_render_graph_edges(Core3d, (Tonemapping, PpispLabel, Upscaling))`. **Placement decision:** PPISP's CRF *is* a learned tonemap, so for views PPISP owns, run it **instead of / before** Bevy's tonemapping, then sRGB-encode last. Make this explicit in the node ordering rather than stacking two tonemaps.
- Params reach the render world via `ExtractComponentPlugin<PpispViewParams>` (the per-view exposure scalar + 8 color latents + per-camera vignette/CRF + the host-built `H`), uploaded in `RenderSet::Prepare` with `RenderQueue::write_buffer`; bind groups built in `RenderSet::Queue`. Per-camera arrays go in a `storage<read>` buffer indexed by `camera_idx`.
- Pipeline: `SpecializedRenderPipeline` + `PipelineCache`; resources needing the device created in the plugin's `finish()` (device not ready in `build()`).

**Training (offline, optional in-engine)** — run the forward+backward+reduce compute passes and the burn controller from a system in the `Render` schedule or a standalone job sharing the device, with explicit submit/sync ordering against the render schedule (both submit to one `Queue`). Grad buffers zero-initialized each step (clear pass or `write_buffer` of zeros), mirroring torch `zeros_like`. The recommended first deliverable trains **offline** and the engine only consumes exported params.

**Editor (Studio):** expose the four parameter blocks in the polymorphic Properties inspector (per memory: Properties is the single inspector) — EV slider, vignette center/coeffs, color chromaticity offsets, CRF toe/shoulder/gamma/center — driving `PpispViewParams` live. The PDF/JSON report (`export_ppisp_report` analog) is rebuilt in Rust with `plotters`/`plotters-svg` for the curve panels (exposure-over-frames, vignette falloff, CRF per channel, chromaticity-shift trajectories), `image` for the raster panels (gray bars, vignette mask, barycentric chromaticity triangle), composed into a PDF with `printpdf`. The chromaticity triangle is a per-pixel raster fill — port directly and cache it (it is `@lru_cache`d in Python).

---

## 6. Validation plan

Port the Python test contract one-to-one as Rust `#[test]`s, named after their Python originals for traceability. Two-layer strategy mirroring the Python f64-probe / f32-kernel split.

**Golden vectors.** Run `torch_reference.py` + the regularization formula once with the test seeds (`seed=42`, identity params, the `color_params[0,7]=1` case, the boundary-value case, and a **near-degenerate homography** case to expose the nullspace DISCREPANCY) and serialize input+output arrays to `tests/fixtures/` (`.npy` via `ndarray-npy`, or flat JSON). Commit them. Each Rust test loads a fixture, runs the CPU reference, and asserts against golden output.

**Tolerances (adopt the Python suite's exactly):**

| Check | Comparison | Tolerance |
|---|---|---|
| Forward (basic, batch sizes, identity, no-cam/no-frame, large) | CPU/GPU vs golden | `max_diff < 3e-6` |
| Forward, multi-seed | 10 seeds | `atol 1e-5` |
| Backward per-group | analytic vs golden | `atol 1e-4` |
| Gradcheck (analytic vs finite-diff) | f64 reference | `eps=1e-3, atol=5e-3, rtol=5e-2` |
| Reg loss | vs golden | `1e-6` |
| Reg grads | vs golden | `atol 5e-6, rtol 5e-5` |
| Reg explicit color value | `color_params[0,7]=1` | `1e-7` |
| Reg channel-variance-zero | equal-across-channels | loss `1e-10`, grad `1e-8` |
| Large multiblock (19 cam × 513 frame) | nondeterministic reduce | `loss 2e-5, grad 1e-5, rtol 1e-4` |
| Optional inputs (None vs explicit) | fwd / bwd | `1e-6` / `1e-5` |

**Finite-difference gradcheck in Rust.** No autograd on the reference, so hand-write the backward and verify against **central differences in f64** on the CPU reference: `g_fd = (L(θ+εe_i) − L(θ−εe_i)) / (2ε)`, `ε=1e-3` (matching the Python central-diff test), `assert_relative_eq!(g_analytic, g_fd, max_relative=5e-2, epsilon=5e-3)`. In f64 you can tighten beyond the Python floor — do so to catch real analytic bugs, especially in `compute_homography_bwd`.

**WGSL-f32 vs f64 reference.** Run the GPU f32 path against the same fixtures at the **loose** tolerances (`3e-6` forward; `5e-3`/`5e-2` for grads) — never gradcheck the f32 GPU kernel against f64 finite differences at tight tolerance. Use the deterministic two-pass reduction so the reg-loss tests can stay tight rather than adopting the atomic-nondeterminism floor.

**Edge cases to cover explicitly:** `frame_idx=-1` / `camera_idx=-1` switches; identity params are NOT a no-op for CRF (`softplus(0)=ln2`, `sigmoid(0)=0.5`); `num_cameras=1`/`num_frames=1` (singleton variance → exactly 0, no divide-by-zero); `var(unbiased=False)` divides by N; the `(W,H)` vs `[H,W]` swap and `(x+0.5,y+0.5)` pixel centers; the near-degenerate nullspace case (catches the §3.3 DISCREPANCY if reference and kernel truly differ).

---

## 7. License / attribution

PPISP is **Apache-2.0** (NVIDIA). All third-party deps of the reference are permissive — PyTorch (BSD-3), NumPy (BSD-3), Matplotlib (PSF/BSD-like), setuptools/wheel/pytest (MIT). **No copyleft, no GPL** — nothing constrains Eustress's licensing.

Obligations for the Rust port:

- **Retain the Apache-2.0 NOTICE / headers and note modifications (Apache §4)** on the parts derived from NVIDIA's own source: the four ZCA blocks, `S_inv`, the source chromaticities, the analytic homography construction, the CRF curve, and the regularization math. Put the attribution in `crates/ppisp/constants.rs` and the crate header / `NOTICE`.
- Retain BSD/MIT/PSF notices only for deps you actually keep — a from-scratch Rust reimplementation keeps none of PyTorch/NumPy/Matplotlib, so only the Apache-2.0 obligation on NVIDIA-derived math remains.
- The build uses `--use_fast_math` (approximate transcendentals, flush-to-zero) — **bit-exact reproduction against the original is not guaranteed even in principle**, which is why the validation tolerances above are the right target, not bit-equality.

**Rust replacements for the dropped deps:** PyTorch/autograd → hand-WGSL + burn (controller); NumPy → glam/ndarray; Matplotlib+PDF → plotters + printpdf + image; setuptools/wheel → Cargo; pytest → `#[test]`.

---

## 8. Phased milestone plan

| Phase | Deliverable | Depends on | Effort | Key risk |
|---|---|---|---|---|
| **0** | Resolve the 4 `MUST CONFIRM` items by diffing `ppisp_math.cuh` / `ppisp_impl.cu` / `torch_reference.py` (nullspace form, H-norm eps, CRF interior eps, vig hinge) | source tree | S | The nullspace reference↔kernel mismatch is real (then golden vectors must encode the kernel form) |
| **1** | **CPU reference** (`reference/`, generic `f32`/`f64`) — all 4 forward + backward + reg, passing golden-vector + f64 gradcheck tests | Phase 0 | M | homography backward correctness (cancellation hides under f32) |
| **2** | **WGSL forward** kernels on shared Bevy device + forward golden tests at `3e-6` | Phase 1 | M | matrix major-order / float3 packing; max-dim normalization |
| **3** | **WGSL backward + reg reduction** (two-pass deterministic) + grad golden tests | Phase 2 | L | **no f32 atomics** → two-pass reduce correctness; CRF intentional truncation; barrier placement |
| **4** | **Controller** (burn 0.21, shared device) train + inference + distillation/freeze | Phase 1 | M | burn↔Bevy device sharing + submit/sync ordering; burn API churn |
| **5** | **Bevy inference integration** — `ViewNode` post-process, param extract/upload, placement vs tonemapping | Phases 2,4 | M | render-graph ordering; CRF-vs-tonemap placement |
| **6** | **Offline training loop** co-optimizing with the GS trainer (GS doc) | Phases 3,4 + GS crate | L | joint scheduling/freezing across two optimizers + controller phase; needs the GS trainer to exist |
| **7** | **Editor + report** — Properties inspector controls + plotters/printpdf PDF/JSON | Phase 5 | M | none material |

Effort: S ≈ a day, M ≈ a few days, L ≈ 1–2 weeks. **Recommended first shippable target: Phases 0→1→2→5** (offline-trained params, in-engine inference) — this captures the photometric-consistency and controllable-ISP payoff without porting the trainer. Phases 3/6 (GPU training in Rust) are the genuinely hard, optional half; defer until the GS trainer lands and only if in-engine training is required.

**Ranked risks:** (1) f64-less GPU gradcheck masking cancellation bugs — mitigate with the f64 CPU oracle; (2) no native f32 atomics — two-pass reduction from the start; (3) shared-device submit/sync ordering between burn/cubecl and Bevy; (4) hand-porting `compute_homography_bwd` correctly; (5) the unresolved nullspace/vig-hinge spec-vs-code discrepancies (Phase 0 gate); (6) burn/cubecl API churn (pin exact versions).

---

## 9. Build vs buy vs autodiff — recommendation and decision log

**Recommendation in one line:** *Buy* the device/framework plumbing (Bevy wgpu, burn for the controller, glam/encase/plotters); *build* (hand-write) the four transform forward+backward WGSL kernels and the reg reduction; do **not** use generic autodiff for the ISP transforms (use it only for the controller). Train offline, ship inference.

**Decision log:**

| # | Decision | Why |
|---|---|---|
| D1 | Hand-write transform backward in WGSL, not burn-autodiff | Adjoints already closed-form; CRF has an *intentional* gradient truncation autodiff would "correct" into disagreement; fused kernel beats many small ops |
| D2 | burn for the controller MLP + as f64 gradcheck oracle | A real NN where autograd helps; burn shares Bevy's device via `WgpuDevice::Existing`; `NdArray<f64>` gives the gradcheck reference WGSL cannot |
| D3 | Reject candle | No production wgpu backend; would add a second tensor stack on CPU/Metal with host hops; `burn-candle` bridge deprecated |
| D4 | Two-pass deterministic reduction over float-atomic CAS | WGSL lacks native f32 atomics; deterministic reduce keeps tight test tolerances and matches the `cub::BlockReduce` structure; `SHADER_FLOAT32_ATOMIC` is Vulkan/Metal-only |
| D5 | Hand-roll Adam + LR schedule for the ISP params | ~80 lines, host-side on tiny Vecs, decouples from burn optimizer churn; preserve `eps=1e-15` exactly |
| D6 | CPU reference (`f32`/`f64`) is the source of truth, not the GPU | Mirrors the Python f64-probe / f32-kernel split; the only way to gradcheck without GPU f64 |
| D7 | Train offline (PyTorch/CUDA or Rust Phase 6), ship only forward to the engine first | The forward path maps cleanly to a `ViewNode`; in-engine training is the hard, optional half |
| D8 | Kernel (CUDA) form wins all reference↔kernel discrepancies for production | The trained optimizer was produced by the kernel; the paper/CPU-reference variants are flagged but the checkpoint must match the kernel — pending the Phase 0 confirms |

---

### Appendix A — `MUST CONFIRM` checklist (Phase 0 gate)

1. **Nullspace selection** — `ppisp_math.cuh::compute_homography` threshold-fallback (`1e-20`) vs `torch_reference.py:41-116` argmax-magnitude. Replicate the kernel form; encode a near-degenerate golden vector.
2. **H normalization eps** — kernel guarded `1/s` vs reference additive `1e-10`. Use kernel form (negligible outside degeneracy).
3. **CRF interior + final-gamma eps** — confirm `eps=1e-6` clamps on the `pow` bases and the final `clamp(y,1e-6)^gamma` in `apply_crf_ppisp` (load-bearing for gradients).
4. **Vig non-positivity hinge** — linear `mean(relu(α))` (code/tests) vs squared `Σ[α]_+²` (paper). Confirm `ppisp_impl.cu::ppisp_regularization_camera_param_loss_kernel`; trust code.
5. **atomicAdd reduction** — `ppisp_impl.cu` per-thread→param mapping (informational only; Rust uses its own two-pass reduce).

Source files that settle these: `ppisp/src/ppisp_math.cuh`, `ppisp/src/ppisp_impl.cu`, `tests/torch_reference.py:41-116`.
