# Gaussian Splatting: Winning All Five vs Unreal (Relightable + Physical GS)

> **Canonical engineering battle plan.** Audience: the solo founder + future engineers.
> Tone: ambitious and decisive — *here is HOW we win* — while staying honest. Every claim is labeled.
>
> **Status labels:** **SHIPPING** (production-usable today) / **USABLE-RESEARCH** (open code, reproducible, near-real-time on consumer GPU) / **RESEARCH-BET** (paper-only, offline, or not-yet-built).
> **Confidence:** HIGH / MED / LOW. **Effort:** S (<2 wk) / M (2–6 wk) / L (2–4 mo) / XL (4 mo+).
>
> We do not invent numbers. Where a number appears it is cited from the verified research briefs. Where a thing does not exist yet, we say so.

---

## 0. The one-sentence answer to "how do we beat UE and win all 5?"

We do **not** win by out-engineering Unreal's raster stack. We win by making Gaussian Splatting a **first-class, relightable, physical, AI-composable primitive in an open, web-native engine** — the exact thing Unreal can only bolt on through heavyweight, pinhole-only plugins. UE's GS plugins (XVERSE XV3DGS, Luma, community UEGaussianSplatting) are essentially the vanilla 3DGS rasterizer wrapped for the editor: **pinhole-only, no real LOD, no native compression-streaming, no dynamic lighting, weak/absent 4D, and structurally no AI/agent tool surface.** That last gap is the durable moat.

---

## 1. Thesis: GS as a native engine primitive, not a plugin

Unreal treats a splat as an imported decoration: a pretty, static, pinhole-projected point cloud you light with nothing and collide with nothing. **We treat a splat as a citizen of the scene graph** — something the engine can relight with its own PBR lights, give a collider to, animate, stream with LOD, compress, and *compose from a text prompt*.

The defensible ambition, stated plainly:

- **Relightable** — inverse-render GS into per-splat PBR (albedo/roughness/metallic/normal) and light it with our existing Bevy PBR + `LightClassPlugin` dynamic lights + shadow maps. No general-scene engine ships this. (USABLE-RESEARCH; the bold differentiator.)
- **Physical** — extract a surface, build Avian colliders, keep the splats as the visual and the mesh as the invisible proxy. (SHIPPING-grade for static; the most credible "win now" extension.)
- **AI-composable** — `intent → asset resolve → semantic place → edit-by-text → bake` in one agent loop over our MCP surface. UE's closed C++/Blueprint surface structurally cannot expose this. (The flagship moat.)
- **Open + web-native** — pure-Rust/wgpu, ships to wasm, MIT/Apache stack we fork, extend, and upstream.

We build on **`bevy_gaussian_splatting` v8.0.1** (Bevy 0.19, wgpu 29, pure Rust, MIT/Apache; native `.ply`/`.gcloud`/glTF `KHR_gaussian_splatting`; 2dgs/3dgs/4dgs; depth/normal render; GPU radix + bitonic sort). Critically, **its roadmap is our roadmap**: lighting & shadows, LOD, SH compression, OpenXR, deformable kernel, temporal sort, and `.spz` are **all "not shipped — roadmap"** on the crate. Every axis where we "win in N months" is precisely a roadmap item we implement and control.

> **Strategic anchor:** the durable moat is **orchestration, not models.** Generative GS / 3D models are commodity inputs (TRELLIS, Hunyuan, Rodin/Meshy are MIT or hosted and swappable). The orchestratable, simulation-baking, web-native engine is not. Ground every pitch on the orchestration + simulation bake.

---

## 2. The Apple / relightable-GS reality (do not misquote this)

**The founder's framing "Apple has GS with dynamic lights" is FALSE as stated. Confidence: HIGH.** Correct the record before any pitch:

| Lab | What they actually published | Relit / dynamic light? |
|---|---|---|
| **Apple** | *Drop-In Perceptual Optimization for 3DGS* (Apple ML Research, Mar 2026) — a perceptual quality loss (less blur, ~50% bitrate savings). And *HUGS: Human Gaussian Splats* — human reconstruction/animation. | **NO.** Neither touches relighting, BRDF, or dynamic lights. |
| **Meta Reality Labs** (Codec Avatars Lab) | **Relightable Gaussian Codec Avatars (RGCA)**, **CVPR 2024 Oral** (arXiv:2312.03704) — SH diffuse + spherical-Gaussian all-frequency specular, learnable radiance transfer. **Real-time relighting on a tethered consumer VR headset.** | **YES** — but **avatar/head-only**, trained per-identity from a dense light-stage capture. |

**The dynamic-light claim is real — it just belongs to Meta, not Apple, and it is avatar-only.** RGCA is the strongest "GS + dynamic light + real-time" result that exists (follow-ons: URAvatar, SqueezeMe). **No lab has shipped general-scene real-time relightable GS as a product.** That gap is exactly our opening.

> **Do-not-say list (lighting/physics):** "Apple has relightable/collidable GS" (false). "PPISP gives us collision" (false — it's photometric ISP). "Real-time GI / inter-reflection on splats" (offline only today). "We win the GS field on all 5 axes today" (we beat *UE* on all 5; we *lead the field* on 1–2 today).

---

## 3. Dynamic lighting for GS — the win

**Thesis:** inverse-render GS into per-splat PBR attributes (albedo / roughness / metallic / normal) under a Cook-Torrance BRDF, import them as PBR surfels into our G-buffer, then light them with the **PBR renderer + `LightClassPlugin` (lumens/lux dynamic lights) + shadow maps + atmosphere/sun service we already own.**

This is the one branch of the relightable-GS literature that yields "just PBR splats" a normal deferred renderer can light. The reference methods all attach Cook-Torrance microfacet BRDF parameters per Gaussian:

| Method | Decomposes each Gaussian into | Relightable? | Real-time? | Status |
|---|---|---|---|---|
| **GS-IR** (CVPR 2024) | albedo, roughness, metallic, normal + env map + baked-occlusion indirect | Yes (Cook-Torrance) | Real-time render; trains <1h on V100 | USABLE-RESEARCH |
| **Relightable 3D Gaussian / R3DG** (ECCV 2024) | normal, full BRDF, per-point incident light, baked visibility | Yes (relight + ray-trace + edit) | Yes (precomputed transfer) | USABLE-RESEARCH |
| **GaussianShader** (CVPR 2024) | normal (shortest-axis), diffuse, specular tint, roughness | Partial (reflective surfaces) | Yes | USABLE-RESEARCH |
| **3DGS-DR / DeferredGS / Ref-Gaussian** (SIGGRAPH'24 / ICLR'25) | deferred G-buffer (normal + reflection / albedo-rough-metal) | Yes | Real-time deferred | USABLE-RESEARCH |
| **IRGS** (CVPR 2025) | full rendering equation, 2D-Gaussian ray tracing, inter-reflection | Yes (best inter-reflection) | **NO — ~1 s/frame, RTX 3090** | RESEARCH-BET (offline) |
| **RGCA** (Meta, CVPR'24 Oral) | SH diffuse + spherical-Gaussian specular (avatar-specific transfer) | Yes | Yes (VR headset) | USABLE-RESEARCH / SHIPPING (avatars only) |

### The pipeline

```
 multi-view capture
   │
   ▼
 PPISP front-end  (crates/ppisp)                         [PRECONDITION — §3b]
   per-camera/per-frame exposure + white-balance +
   vignette + CRF normalization
   │   (disentangles capture-time photometric variation
   │    from true scene radiance)
   ▼
 inverse-render trainer  (GS-IR / R3DG / GaussianShader class)
   bake per-splat: albedo + roughness + metallic + normal   (Cook-Torrance)
   │
   ▼
 import as PBR surfels → bevy_gaussian_splatting (forked) → G-buffer
   │   (crate already renders depth + normal)
   ▼
 light with EXISTING engine PBR:
   LightClassPlugin (lumens/lux dynamic lights) + shadow maps + atmosphere/sun
   │
   ▼
 relit Gaussian scene under dynamic engine lights
```

### Effort / confidence / honest flags

| Stage | Effort | Confidence | Status | Honest flag |
|---|---|---|---|---|
| PPISP Rust port (front-end) | S–M | HIGH | USABLE-RESEARCH | Differentiable ISP; well-scoped; helps any capture pipeline. |
| Inverse-render trainer → PBR splats | L | MED | USABLE-RESEARCH | Open Python code exists (port or FFI). **Normals are noisy** (Gaussians have no true surface) → specular weak. |
| PBR-splat → Bevy G-buffer + `LightClassPlugin` | M | MED-HIGH | We own the renderer | Crate has depth/normal already; lighting is roadmap → **we implement it** (the point). |
| Moving-light shadows on splats | L–XL | LOW-MED | RESEARCH→USABLE | Baked per-splat visibility is static; moving shadows need runtime shadow maps / multi-pass. Expensive. |
| Indirect / inter-reflection | XL | LOW | RESEARCH-BET | Correct only in offline methods (IRGS ~1 s/frame, RTX 3090). **Do not promise real-time GI.** |

### Phasing — bake-then-light first, full inverse-render later

- **SHIP FIRST (≈6–10 wk):** PPISP front-end + **static-capture relightable GS** (albedo/roughness/metallic baked) lit by one or two dynamic `LightClassPlugin` lights with screen-space or baked shadows. Demoable, honest, real.
- **SHIP LATER (research track):** accurate moving-light shadows, inter-reflection, glossy specular parity with mesh-PBR. Flag as research-grade.

**Why this is defensible:** Meta proved relightable GS is real but capped it at avatars. General-scene relightable GS is unshipped by anyone. We have the PBR renderer, the dynamic-light system, the atmosphere/sun service, and a forkable crate whose lighting is *explicitly roadmap*. This is the boldest axis and the highest-risk one — label it **USABLE-RESEARCH, not SHIPPING**, and lead the static-capture demo.

---

## 3b. The honest PPISP correction (short and clear)

**PPISP = "Physically-Plausible Compensation and Control of Photometric Variations in Radiance Field Reconstruction."** A fully-Rust differentiable ISP (exposure / vignette / color / CRF) — `crates/ppisp` (proposed).

- **What it DOES:** disentangles capture-time camera photometric variation (auto-exposure, white-balance, vignette, tone curve) from true scene radiance, so the inverse-render optimizer does not misattribute a brighter frame to brighter albedo or a stronger light. This is well-established (WildGaussians, Robust-GS, SWAG all do per-view affine correction).
- **What it does NOT do:** it has **nothing to do with geometry, surface extraction, meshes, or colliders.** It is not a relighting method and it is not a physics mechanism.
- **Its true role:** the **photometric front-end** — a *precondition* for clean inverse rendering. You cannot decompose `observed = albedo ⊗ BRDF ⊗ light` if exposure/WB/vignette are baked into the pixels. PPISP makes the albedo/BRDF split trustworthy, which makes relightable-GS physically grounded.

> **State to anyone who conflates them:** "PPISP is photometric ISP correction. Collision comes from surface extraction + convex decomposition (§4), not PPISP. Anyone presenting PPISP as the collision mechanism is wrong."

---

## 4. Physics & collision for GS — the win

**Thesis:** GS stays the **visual**; we extract a surface mesh → decimate → convex/CoACD or CSG-primitive colliders → **Avian**. The colliders are invisible; the splats remain the only rendered surface. Our stack maps **1:1**: Avian 0.7 with `collider-from-mesh` enabled + convex decomposition path, the truck CSG/CAD kernel, and the `mesh-edit` half-edge crate.

**This is the most credible "win now" of all the GS extensions.**

### The pipeline

```
 splats
   │
   ├─ 2DGS  (indoor / bounded)   ── TSDF + Marching Cubes ──┐
   └─ GOF   (unbounded / outdoor)── Marching Tetrahedra  ───┤
                                                            ▼
                                            raw proxy mesh
                                                            │
   crates/mesh-edit (half-edge): weld + decimate to ~5–20K tris   (collider LOD only)
                                                            │
        ┌───────────────────────────────────────────────────┴───────────────┐
        ▼                                                                     ▼
   CoACD convex decomposition                               CSG primitive fit (truck kernel)
   (~16–64 hulls/object; V-HACD fallback)                   boxes/capsules/cylinders to
        │                                                    wall/floor/door regions
        ▼                                                                     ▼
   Avian: Collider::convex_hull per part → compound rigid body
        │
        ▼
   colliders INVISIBLE; splats remain the only rendered surface
```

### Tiers

**Tier A — Static collidable splat scenes (SHIP FIRST; all SHIPPING-grade deps).** `2DGS|GOF → decimate → CoACD → Avian compound colliders`. **Effort: M. Confidence: HIGH. Status: SHIPPING-grade.** 2DGS/GOF give cleaner, more watertight proxies than SuGaR → fewer degenerate hulls → stable contact. CoACD beats V-HACD on accuracy-per-hull (preserves handles, openings, tunnels). The half-edge crate's weld/decimate makes decomposition robust (manifold input → far fewer artifacts).

**Tier A′ — CSG primitive fit (architecture/blocky).** Swap CoACD for the **truck CSG kernel** fitting boxes/capsules/cylinders to segmented wall/floor/door regions. Lowest collider count, cleanest contact, parametric/editable. **Effort: M. Confidence: MED-HIGH. Status: SHIPPING-grade** (truck + Avian primitives only).

**Tier B — Deformable / dynamic splats (LATER; research track).** VR-GS recipe: tet cage (VDB → marching cubes → TetGen) + XPBD/FEM + two-level embedding skinning; collide the cage in Avian. **Real-time is proven** (VR-GS: 24–161 FPS, RTX 4090, 0.3–2.5M splats) but **VR-GS code availability is uncertain → budget a build-it-yourself.** **Effort: XL. Confidence: MED. Status: USABLE-RESEARCH.** Prototype against NVIDIA Kaolin/Simplicits offline first (the only SHIPPING mesh-free physics-on-splats, but Python/Warp — reference/bake tool, not in-engine runtime). Note: PhysGaussian (MPM) is offline (~400 substeps/frame) — generative-dynamics tool, not a runtime.

| Tier | Effort | Confidence | Status |
|---|---|---|---|
| A: static colliders (2DGS/GOF → CoACD → Avian) | M | HIGH | SHIPPING-grade |
| A′: CSG primitive fit (truck) | M | MED-HIGH | SHIPPING-grade |
| B: deformable tet-cage + XPBD | XL | MED | USABLE-RESEARCH |

> **Collider rule of thumb:** static shell → trimesh or CoACD-on-decimated; interactive dynamic objects → CoACD (cap ~16–64 hulls); blocky/architectural → CSG primitive fit.

---

## 5. Per-axis battle plan to beat Unreal

> UE's GS plugins are thin wrappers on the vanilla rasterizer. We beat them on every axis; we *lead the field* on 1–2 today and reach the rest with focused work. The AI/agent surface is the one UE structurally cannot copy.

### Axis 1 — Simulations via text/AI scene composition (the flagship moat)

| | |
|---|---|
| **How we win** | One agent loop `intent → asset resolve → semantic place → edit-by-text → bake (physics+lighting)`. UE has no native text→asset, no semantic-text selection, no LLM scene assembly, no agent-callable surface (closed C++/Blueprint). |
| **Eustress asset** | MCP surface (`image_to_geometry`, `create_entity`, `document_to_code`, `execute_luau`, `query_material`, `raycast`, `inspect_scene`); `embedvec` for asset retrieval; scene graph + Avian + `LightClassPlugin` for the bake. |
| **Confidence** | **HIGH to beat UE; HIGH to win the field.** Models are commodity (TRELLIS/Hunyuan MIT, Rodin/Meshy hosted); the orchestratable engine is not. |
| **Effort** | M–L (wiring, not research). |
| **Risk** | Text-selectable GS editing (Feature Splatting / Gaussian Grouping, Apache-2.0/open) needs a **per-scene prep pass** — interactive *after* prep, not zero-shot live. Scene composition is rooms/small-scenes today, not open worlds. Flag both. |

The defensible workflow (research only *fakes* steps 1/3/5 with slow per-scene optimization; we do them natively):

```
1. INTENT        prompt → LLM → scene graph + constraints (GALA3D-style layout)
2. ASSET RESOLVE retrieve (embedvec) ELSE generate (image_to_geometry → TRELLIS/Hunyuan,
                 or Rodin/Meshy for hero assets) → mesh/GLB preferred, GS for static dressing
3. SEMANTIC PLACE create_entity at LLM transforms; snap/collision-resolve via raycast + overlap
4. EDIT BY TEXT  semantic IDs (Gaussian Grouping / Feature Splatting) → "move the chair / delete
                 the lamp / make it brick" routes through select + update + query_material
5. BAKE          Avian colliders from mesh + LightClass lighting + PBR materials → run_simulation
                 → a SIMULATABLE, not just renderable, scene
```

### Axis 2 — Real-time

| | |
|---|---|
| **How we win** | GPU radix sort (already in the crate) + **amortized/dirty-view resort** + a **true LOD hierarchy** (Hierarchical-3DGS / Octree-GS) so frame time tracks on-screen splats, not scene size. Pair **NVIDIA 3DGUT** unscented projection for distorted/rolling-shutter cameras (WGSL-implementable). |
| **Eustress asset** | `bevy_gaussian_splatting` (GPU radix + bitonic sort shipped); `SCALING_ARCHITECTURE.md` (Morton streaming, GPU cull); HLOD whole-map (merged-cell decimated proxies, visibility-toggle). |
| **Confidence** | **MED-HIGH vs UE** (they have no real LOD). **MED to lead the field** (Niantic Spark / PlayCanvas already ship LOD + streaming). |
| **Effort** | L (LOD is the missing piece — crate LOD is roadmap, not shipped). |
| **Risk** | Raw sort alone won't win; matching shipping web engines requires real LOD + streaming. |

Reference numbers (cited): web-splat (Rust/wgpu) >200 FPS RTX 3090; Hierarchical-3DGS 30+ FPS navigating 7 km BigCity (renders ~8% of leaves); Octree-GS consistent >30 FPS. 3DGUT: 317 FPS MipNeRF360 on RTX 5090, unlocks fisheye/rolling-shutter while staying rasterizer-fast.

### Axis 3 — Web (our structural win)

| | |
|---|---|
| **How we win** | WebGPU compute (we're wgpu-native → free) + per-frame splat budget + streaming page table (Spark's 16M-splat pool model) + **SOG (~20×) runtime format / SPZ (~10×) interchange** decoded to GPU. WebGL2 fallback so we don't strand ~15%. |
| **Eustress asset** | Already ships to wasm; wgpu/Bevy 0.19 + wgpu 29; WorldDb persistence + chunking. |
| **Confidence** | **MED.** UE on web is weak → **easy to beat UE.** PlayCanvas/Spark are the real web GS leaders → "win vs UE" YES, "win the web GS category" is MED. |
| **Effort** | M–L. |
| **Risk** | Must implement SOG/SPZ decode-to-GPU + streaming; the crate does **not** support `.spz` yet (we add it). Do not re-expand to f32 in VRAM. |

### Axis 4 — Lightweight

| | |
|---|---|
| **How we win** | **SOG default (~20×), SPZ interchange (~10×), PLY import-only**; keep quantized form on GPU (16/8-bit, decode-on-upload). Optional HAC/HAC++ (50–100×) for archival. Single Rust binary / wasm. |
| **Eustress asset** | Rust/wgpu control of the upload path; WorldDb chunking composes with LOD. |
| **Confidence** | **MED-HIGH vs UE** (no native GS compression-streaming). |
| **Effort** | M. |
| **Risk** | SH compression is on the crate's roadmap, not shipped → we add it. Lowest-risk technical axis after physics-Tier-A. |

Reference: SPZ ~10× (Niantic, open); SOG/SOGS ~20× (PlayCanvas web default — 1 GB / 4M-splat PLY → ~55 MB); HAC >75×, HAC++ ~100× (research, decode-complex).

### Axis 5 — Animation

| | |
|---|---|
| **How we win** | Spacetime-Gaussian-class real-time 4D for playback (8K@60 proven; Disentangled4DGS 343 FPS RTX 3090); SC-GS sparse control points for *editable/riggable* motion. |
| **Eustress asset** | Existing animation/timeline system + Bevy; SC-GS pairs with control-point rigs; crate has 4dgs support (deformable kernel roadmap). |
| **Confidence** | **LOW-MED.** Most 4DGS is *reconstruction of captured video*, not arbitrary authored animation. |
| **Effort** | L–XL. |
| **Risk** | **HIGHEST of the 5 for "authored animation."** Win *playback of captured 4D*: plausible. Win *authored splat animation* (driving splats from a skeleton/physics rig in real time): **RESEARCH-BET.** Be honest. |

---

## 6. Honest scorecard

> Calibrated answer: we can credibly claim to **BEAT UNREAL on all five** — because UE's GS plugins are thin and UE has no AI tool surface — but "win the GS field outright" is true on 1–2 axes, achievable-with-work on 2–3, and a research bet on 1–2. **Lighting and physics are net-new capabilities, not parity items:** physics is win-now, lighting is the bold research-grade differentiator.

| Capability | Beat UE? | Win field? | Verdict | Timeline |
|---|---|---|---|---|
| **1. Text/AI composition** | YES, HIGH | YES, HIGH | **WIN NOW** (the moat) | M–L wiring |
| **2. Real-time** | YES, MED-HIGH | MED | **WIN in 2–4 mo** (need LOD) | L |
| **3. Web** | YES, MED | MED | **WIN vs UE now; field = work** | M–L |
| **4. Lightweight** | YES, MED-HIGH | MED-HIGH | **WIN in ~6–8 wk** (add SOG/SPZ) | M |
| **5. Animation** | YES on playback | LOW-MED | **Playback: months. Authored: RESEARCH-BET** | L–XL |
| **Dynamic lighting on GS** | net-new | nobody ships general-scene | **WIN in N months; RESEARCH-GRADE** (PPISP front-end ships first) | L→XL |
| **Physics/collision on GS** | net-new | Tier A genuinely shippable | **WIN NOW (Tier A static); deformable = months** | M→XL |

### The real picture (without killing ambition)

- **Win NOW (next quarter, real):** Axis 1 (text/AI composition — the flagship), Axis 4 (lightweight via SOG/SPZ), and **Physics Tier A** (static collidable splats via Avian + CoACD + truck). SHIPPING-grade or close. **Lead the pitch here — verifiable and uncopyable by UE.**
- **Win in N months of focused work:** Axis 2 (real-time — needs LOD hierarchy), Axis 3 (web — SOG/SPZ + streaming), and **static-capture relightable GS** (PPISP + GS-IR-class inverse render + `LightClassPlugin`). Credible, open code exists, real effort.
- **Research bets (pursue, flag honestly):** authored/rigged splat **animation**, accurate **moving-light shadows + inter-reflection**, and **deformable physics** (tet-cage/XPBD). This is where we out-run Meta's avatar-only ceiling — never present them as shipping.

---

## 7. Phased roadmap

> Aggressive but real. Each phase is independently demoable. Research bets run on a parallel track and never block the shipping line.

```
 PHASE 0 ── ADOPT THE CRATE                                         [S–M, SHIPPING]
   Fork bevy_gaussian_splatting v8.0.1 into the workspace.
   Render .ply/.gcloud/glTF splats live in a Eustress space.
   Milestone: a splat scene loads, sorts, and renders in-engine.

 PHASE 1 ── PHYSICS TIER A (win-now #1)                             [M, SHIPPING-grade]
   2DGS|GOF extract → mesh-edit decimate → CoACD → Avian colliders.
   Splats visible, colliders invisible. + Tier A′ CSG fit for architecture.
   Milestone: drop a physics object onto a splat floor; it collides.

 PHASE 2 ── LIGHTWEIGHT (win-now #2)                                [M]
   Add SOG (~20×) runtime + SPZ (~10×) interchange decode-to-GPU;
   keep quantized on GPU. PLY import-only.
   Milestone: a 1 GB-class PLY scene loads as ~tens of MB on the web build.

 PHASE 3 ── AI COMPOSITION (the moat)                               [M–L]
   Wire intent→resolve→place→edit→bake over MCP. embedvec retrieval +
   image_to_geometry generation + semantic-ID editing (Gaussian Grouping).
   Milestone: "type a sentence → physics-ready editable scene" in one loop.

 PHASE 4 ── RELIGHTABLE GS, STATIC-CAPTURE                          [S–M then L, USABLE-RESEARCH]
   crates/ppisp front-end → GS-IR/R3DG-class inverse render → PBR surfels →
   G-buffer → LightClassPlugin dynamic lights + shadow maps.
   Milestone: a captured scene relit live by a moving engine light (1–2 lights, baked/SS shadows).

 PHASE 5 ── REAL-TIME + WEB AT SCALE                                [L]
   LOD hierarchy (Hierarchical-3DGS/Octree-GS) + amortized/temporal sort +
   streaming page table; 3DGUT unscented projection in WGSL.
   Milestone: frame time tracks on-screen splats, not scene size; fisheye camera works.

 ── PARALLEL RESEARCH TRACK (bets — never on the critical path) ──
   R1  Deformable physics: tet-cage + XPBD (VR-GS recipe; prototype vs Kaolin/Simplicits).   [XL, RESEARCH-BET]
   R2  Accurate moving-light shadows + inter-reflection on splats (IRGS-class is offline).     [XL, RESEARCH-BET]
   R3  Authored/rigged splat ANIMATION (SC-GS control points → skeleton/physics rig).         [XL, RESEARCH-BET]
```

**Research bets called out explicitly:** R1 deformable physics, R2 real-time GI/moving shadows on splats, R3 authored splat animation. Pursue them for the long-term ceiling; pitch only the shipping phases.

---

## 8. The positioning line

> **Native relightable, physical, and AI-composable Gaussian Splatting in an open, web-native engine — a category Unreal serves only through heavyweight, pinhole-only plugins.**

Or, customer-facing:

> **"Type a sentence; get a physics-ready, editable, semantically-addressable splat scene you can simulate, relight with real dynamic lights, and keep editing by text — in one agent loop."**

No engine offers the full chain. UE has fidelity; the generative stack has assets; **only an AI-native, open, wgpu engine with an MCP tool surface fuses them into a closed generate → place → relight → simulate → re-edit loop.** Ground every claim on the orchestration + simulation bake. Treat GS/generation models as swappable commodity inputs. The `bevy_gaussian_splatting` roadmap is our roadmap — lighting, LOD, SH compression, `.spz`, deformable kernel are all "not shipped — roadmap" on an MIT, Bevy 0.19, wgpu 29, pure-Rust crate we fork, extend, and upstream. That is a plan, not hand-waving.
