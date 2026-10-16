# Splat Objects and In-House Diffusion

**Date:** 2026-10-01
**Decisions (McKale, 2026-10-01):** Brush builds the splats. Diffusion, not a
GAN, makes the images. The goal is to write it ourselves, with no dependency
beyond Brush. The improvement that matters most is 3DGS segmentation,
classification and separate objects with accurate colliders and per-instance
properties.
**Companions:** `WORLD_MODEL_VS_MARBLE_2026_09_30.md` (section 10),
`WORLD_GENERATION_OVER_MCP_PLAN.md`.

Every claim about SpatialVortex was checked against
`E:\Libraries\SpatialVortex\aimodel` on 2026-10-01. Every claim about Brush was
checked against its repository the same day.

---

## 1. Can SpatialVortex be the solver for the diffusion model?

"Solver" names four different jobs in a diffusion system. SpatialVortex fits
two of them, partly fits one, and does not fit the fourth.

| Job | Fit | Why |
|---|---|---|
| **The optimizer that trains the model** | **Yes** | `training::sso_optimizer` is a working Spectral Sphere / Muon optimizer implemented as a Burn `SimpleOptimizer` (commit `cb49bd6`, 2026-08-08). It already trained a 4.3 M-parameter model end to end (`5e4c042`). Muon-style updates apply to 2D weight matrices, which is most of a diffusion transformer; by Muon convention, embeddings, norms and biases take a simpler update. Whether it beats AdamW on diffusion must be measured: train both at equal compute and compare the loss curves. |
| **The controller that keeps views consistent** | **Yes** | `vortex_diffusion.rs` has a four-state lifecycle (Masked, Candidate, Verified, Locked) with gates that reject and retry, plus `infill()` that fills only masked positions. Section 4 paints a world one view at a time, and each surface patch goes through exactly that lifecycle. The idea transfers directly, with surface patches in place of text tokens. |
| **The sampling solver** | **Partly** | The sampler that integrates from noise to image is small, well-understood numerics: about 200 to 400 lines for Euler and Heun steps. SpatialVortex's schedule (9 macro cycles x 9 sub-steps, phi-scaled, spending more time near clean) was built for discrete text masking and is untested on continuous images. Write a standard solver, make its timestep spacing pluggable, and let the vortex spacing compete as one option. |
| **The denoising network** | **No** | `vortex_diffusion.rs` is a masked diffusion over text token IDs with a single transformer layer. It has had no commits since 2026-07-10, its file still contains no loss or backward pass, and its weights are the untrained "harmonic init". Nothing in the crate diffuses images or latents. The image network must be written new. SpatialVortex's trainable attention (`cad4376`) is reusable as a building block. |

Two further SpatialVortex pieces belong in this pipeline:
- **Graph attention over containment** (`aa4ad87`) and the **containment
  retrieval benchmark** (`c763e3f`) run on real Eustress Spaces. They are the
  right shape for classifying a segmented object from its scene context.
- **The differentiable Hooke's-law loss** (`aa4ad87`) is a physics term that
  can train structural generation, within its stated linear-elastic scope.

---

## 2. What "no dependency beyond Brush" means in practice

- **Brush 1.0.0** (Apache-2.0) brings Burn from its git `main` branch and
  wgpu 30, patched to its author's fork. Burn is a full tensor and autodiff
  framework with wgpu and CUDA backends, so every network below can be written
  in Burn without adding another crate.
- **The engine** is on wgpu 29 through Bevy 0.19. Its lock file already
  carries wgpu 27, 28 and 29, and it has no Burn. GPU buffers cannot be shared
  between wgpu 29 and wgpu 30, so Brush and the diffusion model run in a
  **separate worker binary in its own Cargo workspace**. The worker talks to
  the engine through dataset folders and the bridge. When Bevy moves to wgpu 30,
  an in-process worker can be reconsidered.
- **Brush tracks Burn `main`.** Pin a git revision; upgrades are deliberate.
- **SpatialVortex is on Burn 0.16.** Its optimizer and attention code must be
  ported forward to Brush's Burn revision.
- **Existing engine dependencies stay**: Bevy, Avian (which brings parry and its
  convex decomposition), and the splat renderer.
- **Code can be ours; the eye for realism cannot come from code.** A diffusion
  model learns what reality looks like from images. Section 5 covers where those
  images come from.

---

## 3. The pipeline

```
 A. State        Claude blocks out the world: classes, metric geometry,
                 colliders, materials (exists today over MCP)
       |
 B. Capture      engine renders a camera path; per frame: pose, intrinsics,
                 RGB, depth, normal, instance id, class id, material id
                 -> a nerfstudio-format dataset            (engine, NEW)
       |
 C. Paint        in-house diffusion repaints each view photoreal,
                 conditioned on depth/normal/class/material; views stay
                 consistent by inpainting over known geometry   (worker, NEW)
       |
 D. Lift         Brush trains splats from the painted views with the KNOWN
                 poses, seeded from points sampled on the state surfaces
                 (ply_file_path)                                 (worker, Brush)
       |
 E. Separate     every Gaussian gets an instance by multi-view vote;
                 each instance's splats bind to its Part   (worker + engine, NEW)
       |
 F. Check        depth agreement on held-out poses; raycasts hit only state
```

Stage C is optional at first. Running B, D and E on the engine's own PBR
frames, with no diffusion at all, builds and proves the whole object path. That
is the first milestone in section 7.

---

## 4. In-house diffusion design

**Formulation: flow matching (rectified flow).** The network predicts the
velocity from noise to image; the training loss is a mean squared error on that
velocity, and sampling is Euler or Heun integration along time. It is the
simplest formulation to write correctly and the one behind current large image
and video models such as Wan 2.1.

**Autoencoder.** Our own convolutional encoder and decoder, compressing 8x
per side, trained first and frozen. Diffusion runs in its latent space.

**Denoiser.** A latent diffusion transformer. Conditioning is per pixel: depth,
normal, a learned embedding of the class id and of the material id, all
downsampled into latent space and concatenated with the noisy latent. A small
set of learned style tokens sets the look of a region. **There is no text
encoder.** Claude turns a sentence into class, material and style choices, so
the largest component of a text-to-image model (a multi-billion-parameter
language encoder) is not needed.

**Solver.** Euler and Heun steps, 20 to 50 steps, timestep spacing pluggable,
the vortex spacing as one arm. Seeded and pinned so a run can be repeated on
the same machine; outputs are stored with provenance either way.

**Optimizer.** SpatialVortex SSO on 2D weights, a simpler update on the rest;
AdamW on the same model as the control arm.

**View consistency by inpainting over known geometry.** Painting each view
independently would make neighbouring views disagree, and Brush would average
the disagreement into blur. Eustress knows the exact geometry, so:

1. Paint the first view.
2. Project its pixels onto the surface patches they show (exact, from depth).
3. Render the next view: patches already painted are fixed, unpainted patches
   are masked.
4. Inpaint only the masked region.
5. A patch becomes **Verified** when two or more views agree on it within a
   tolerance, and **Locked** after that. A patch that keeps disagreeing returns
   to **Masked**, up to a retry limit.

This is SpatialVortex's token lifecycle, applied to surface patches.

---

## 5. Where realism is learned

| Source | What it gives | Cost and caveat |
|---|---|---|
| **One-time distillation from a teacher** | Render our labelled captures, let Cosmos-Transfer1 (NVIDIA Open Model License, commercial use allowed) repaint them, and train our model on the pairs | Fastest start. The teacher is a one-time data source; nothing of it ships. Lyra itself was trained this way, on Cosmos-generated data. The licence ends if its safety guardrails are bypassed. |
| **Licensed photographs** | Real-world appearance | Each photo needs depth and segmentation to match our conditioning; that means captured RGB-D data or estimation models, which are dependencies. |
| **Our own physically based reference renderer** | Correct light transport, fully deterministic | Teaches lighting, not the richness of real materials. |
| **The flywheel** | Every world generated, captured, separated and checked becomes labelled training data | Grows over time; needs the first three to start. |

Recommended: distil once to bootstrap, then let the flywheel take over.

---

## 6. Splat objects: segmentation, classification, colliders, properties

### 6.1 Splats Eustress made (labels by construction)

The capture knows which instance every pixel belongs to, so segmentation is not
guessed:

- **Seed.** Sample points on each instance's state surface; write them as the
  initial `.ply`, with a side table from seed point to instance.
- **Train.** Brush trains as usual (one run, unmodified Brush).
- **Vote.** For each trained Gaussian, sum its blending weight per pixel times
  that pixel's instance id, over every training view. The label is the winning
  instance; the confidence is its share. Low-confidence Gaussians are checked
  against distance to the nearest state surface.
- **Comparison arm.** One Brush run per instance using Brush's existing per-view
  masks (`AlphaMode::Masked`). This is costlier, with no voting needed.
- **Objects, colliders, properties** are the existing instances: the collider is
  the authored geometry (exact), and the properties are the class schema's. The
  splat subset binds to its instance, so moving a Part moves its splats.

### 6.2 Splats from outside (phone captures, Marble, Lyra)

No labels exist, so every stage is inferred, and every stage is measured on
6.1's scenes with their labels hidden, where the truth is known:

1. **Surface.** Render depth from many views of the splats, fuse it into a
   signed distance field, extract a mesh with marching cubes.
2. **Supports.** Fit planes (RANSAC) for floor, walls and ceiling.
3. **Instances.** Build a nearest-neighbour graph over the Gaussians, weighted by
   distance, normal agreement and colour similarity; remove the supports; split
   it into connected components; merge or split components whose rendered masks
   disagree across views.
4. **Classification**, to one of the 78 class schemas, in increasing cost:
   geometric features (size, height above floor, flatness, what supports it);
   SpatialVortex's containment graph attention over the resulting scene graph;
   a small classifier trained on 6.1's perfectly labelled splat scenes; Claude
   judging rendered views over MCP when confidence stays low.
5. **Colliders.** Per object: mesh, then simplify, then convex decomposition
   (parry, already linked through Avian), then Avian colliders. Today's
   voxel box-fit (`radiance::extract_colliders`, Tier A) stays as the fallback.
6. **Properties.** Material: base colour and view-dependent strength from the
   Gaussians' spherical harmonics, matched to the nearest of the material
   library's presets. Density from the material, mass from collider volume,
   dimensions from the collider. Stored as real class properties, not
   attributes.

### 6.3 The data model

Each separated object is an ordinary Eustress instance: class, properties
(material, density, mass, anchored, can_collide), convex colliders, a bound
splat subset stored as its own chunk, and provenance (captured, generated or
vendor). Abstract view hides the splat subset; Realistic view shows it.
Physics and raycasts never see splats.

### 6.4 Measures

On 6.1 scenes with labels hidden: segmentation intersection-over-union per
Gaussian; classification accuracy over the 78 classes; collider
intersection-over-union against the authored geometry; depth agreement on
held-out poses; mass error. Thresholds are set after the first baseline run;
they are not invented in advance.

---

## 7. Order of work

Every step has an exit that can fail. Steps 1 to 3 deliver separate objects with
accurate colliders and per-instance properties **before any diffusion model
exists**.

1. **Labelled capture (engine).** A bridge verb renders a camera path and
   writes a nerfstudio dataset: pose and intrinsics per frame, RGB, depth,
   normal, instance, class and material id maps, plus the seed `.ply`.
   Exit: reprojecting depth from frame A into frame B lands within a pixel
   tolerance on the instance-id maps.
2. **Brush worker, separation by construction.** The worker trains splats from
   step 1's dataset of an existing Space, with no diffusion, and votes instance
   labels. Exit: the vote matches truth above the baseline threshold, and in
   Studio, moving a Part moves its splats.
3. **Outside splats become objects.** Section 6.2 end to end, scored on step 2's
   scenes with labels hidden, then run on a real `.ply` capture. Exit: the
   section 6.4 measures are published, including the failures.
4. **Diffusion core.** Autoencoder, denoiser and flow-matching solver in Burn;
   SSO against AdamW at equal compute; trained on distilled pairs. Exit: on
   held-out captures, painted frames keep the conditioning: depth estimated from
   the painted image agrees with the true depth.
5. **Consistent painting.** Section 4's inpainting loop with the patch
   lifecycle. Exit: cross-view reprojection disagreement below threshold, and
   Brush's reconstruction scores well on views it never trained on.
6. **Marble Bench arm E+G** with this pipeline.

---

## 8. Constraints and risks

- The engine pieces (step 1, and binding splats to instances) land only on the
  orchestrator's wave. The worker is a separate workspace, and its builds are
  coordinated with the orchestrator too.
- Burn on wgpu trains slower than CUDA-native stacks; Burn's CUDA backend is
  available for long runs on NVIDIA hardware.
- Diffusion quality is bounded by its training images; a distilled model will
  not exceed its teacher until the flywheel adds better data.
- SpatialVortex's code needs a port from Burn 0.16, and its SSO has been proven
  on a 4.3 M-parameter model, not yet on a diffusion transformer.
- Nothing in this design needs Python at run time. The teacher in section 5 is
  the only Python step, and it runs once to make training pairs.
