# Bevy 0.18 → 0.19 Migration Plan (EustressEngine)

Status: PLAN (verified). Authored 2026-06-27 from a 14-agent audit that quoted the
official [0.18→0.19 migration guide](https://bevy.org/learn/migration-guides/0-18-to-0-19/)
verbatim per-category AND grepped the actual codebase. Source of feature list:
[Bevy 0.19 release](https://bevy.org/news/bevy-0-19/).

## Verdict: BOUNDED, not blocked

The migration is mechanically large but has **no hard blocker**:

- **avian3d 0.7.0 is RELEASED** (crates.io, 2026-06-20) and targets `bevy 0.19.0`. The
  feared physics blocker is gone — clean `0.6 → 0.7` bump.
- **bevy_quinnet has NO bevy-0.19 release** (0.14 → bevy 0.15; latest 0.20 → bevy 0.18).
  The workspace comment calling 0.14 "Bevy 0.19 compatible" is **false**. BUT it is
  optional, behind the `p2p` feature, with **zero real symbol usage** (only doc comments),
  so it is de-fused by dropping it from the `p2p` feature set — not a blocker.
- lightyear 0.19 already supports bevy 0.18–0.19. No bump.

## Verified dependency / toolchain facts

| Item | 0.18 (now) | 0.19 (target) | Action |
|---|---|---|---|
| Rust MSRV | ~1.85 | **1.95.0** | ensure local/CI toolchain ≥ 1.95 (hard gate) |
| Edition | 2021 | **2024** (bevy itself) | our crates stay 2021 (compiles fine); migrate to 2024 LAST, optional |
| wgpu | 27 (lock); 28 pinned for Slint | **29.0.3** | transitive via bevy; Slint master already on wgpu 29 |
| avian3d | 0.6.1 | **0.7.0** | bump (released, targets bevy 0.19) |
| bevy_quinnet | 0.14 | none for 0.19 | de-fuse (drop from `p2p`); fix false comment |
| rodio / cpal | 0.21 / 0.16 | **0.22 / 0.17** | automatic; watch `windows`-crate conflict |
| rand | 0.8 | (0.10 in bevy, but ours is independent) | **defer** — not bevy-coupled |
| lightyear | 0.19 | 0.19 | none |

The single source of truth is the workspace `bevy = "0.18"` in `eustress/Cargo.toml`.
Three crates bypass it with literal `"0.18"` strings (engine `bevy_shader`, engine
`bevy_ecs`, workshop `bevy`) and **player-mobile pins bevy git main** — all four need an
explicit bump/reconcile. Scope: **704 .rs files** use bevy; `engine` (~200K LOC) +
`common` (~128K LOC) dominate by ~3×.

## Ordered migration phases

### Phase 0 — Dependencies + toolchain (Cargo only)
1. Toolchain ≥ 1.95.0.
2. `eustress/Cargo.toml`: `bevy 0.18 → 0.19`; `avian3d 0.6 → 0.7`.
3. Engine literals: `bevy_shader 0.18 → 0.19`, `bevy_ecs 0.18 → 0.19`.
4. De-fuse bevy_quinnet: drop from the `p2p` feature (client/Cargo.toml) + fix the false comment.
5. Reconcile player-mobile's git-main bevy pin to `0.19`.
6. Resolve the stray `bevy 0.19.0-dev` node in Cargo.lock; after bump, `cargo tree -d` to confirm a single wgpu 29 + no `windows`-crate major clash.
7. **This is where the wgpu-29 alignment unblocks the Slint `renderer-femtovg-wgpu` shared-device path** (optional follow-up, long-standing TODO).

### Phase 1 — Compile-blocking code changes (do before first green)
Ordered by crate dependency (common → engine → client/server/workshop/player-mobile):

- **Resources-as-Components** (MEDIUM) — 2 dual-derive blockers:
  - `common/terrain/config.rs:13` `TerrainConfig` — drop `Resource`, keep `Component` (load-bearing: spawned + queried in 7+ systems). Convert the one `Res<TerrainConfig>` read at `engine/part_to_terrain.rs:113` to a `Query<&TerrainConfig, With<TerrainRoot>>`. ⚠ latent bug: nothing inserts it as a resource today, so that `Res` is always `None` — confirm intended source.
  - `common/parameters.rs:558` `Parameters` — drop `Resource` (only ever a Component; trivial).
  - Rename 7 `insert_non_send_resource` → `insert_non_send` (deprecation, not hard): gui/plugin.rs:21, webview.rs:24, slint_ui.rs:2185, billboard_gui.rs:2208/2530, slint_gpu.rs:147, monaco_bridge.rs:253.
- **Scene → WorldAsset** (MEDIUM) — verified the renames ARE real (`bevy_scene → bevy_world_serialization`, `Scene → WorldAsset`, `SceneRoot → WorldAssetRoot`, `ScenePlugin → WorldSerializationPlugin`); BSN is additive in `bevy::scene`.
  - `SceneRoot(…)` → `WorldAssetRoot(…)` at: skinned_character.rs:245, asset_applicator.rs:61/72, twin.rs:170, file_watcher.rs:555/894, file_loader.rs:879. Query `&SceneRoot` → `&WorldAssetRoot` at default_scene.rs:75.
  - Plugin: server/main.rs:260 `bevy::scene::ScenePlugin` → `bevy::world_serialization::WorldSerializationPlugin`.
  - 3 Cargo.toml `bevy_scene` feature flags (server/engine/client) — **VERIFY the 0.19 feature name** that pulls in `WorldAssetRoot` (may have changed meaning to BSN). Highest-uncertainty step.
  - `#Scene0` label strings stay. Our own `crate::scene::*` / `serialization::Scene` are untouched (different types).
- **Lighting service** (MEDIUM) — `light_sync.rs` is SAFE (no field changes). Service work in `common/plugins/lighting_plugin.rs`: imports `Atmosphere`/`ScatteringMedium` `bevy::pbr → bevy::light`; `Skybox` `bevy::core_pipeline → bevy::light`; `Skybox.image` → `Some(handle)` (line 568); `earthlike → earth` (631, 657); **Atmosphere now an ENTITY, not a camera component** (re-architect apply_atmosphere_settings ~650–660 — may let us delete the ai_camera multi-atmosphere workaround). gizmo_tools.rs:50/85/97 `bevy::gizmos::light → bevy::light`.
- **Camera/HDR** (LOW) — `billboard_pipeline.rs:752` `view.hdr` → `ExtractedCamera::hdr` (touch query:700 + loop:722). `slint_ui.rs:2168` `bevy::render::view::Hdr → bevy::camera::Hdr`. TEXTURE_FORMAT_HDR/bevy_default deprecations optional.
- **Text/Font** (LOW) — `font_size: <f32>` → `FontSize::Px(<f32>)` at 8 sites: spawn.rs 1021/1127/1234, runtime_ui.rs:887, dialog_ui.rs:253/290, player-mobile/ui.rs:163/189. Our direct `cosmic-text 0.12` dep STAYS (billboard atlas).
- **Material/PBR** (LOW) — `instance_loader.rs:2143` `#Material0` → `#Material0/std` (else custom-GLB parts silently render default material at runtime). AlphaMode etc. stay via prelude.
- **System/Schedule/RenderGraph** (MEDIUM) — the Slint↔Bevy texture handoff (3 files: `ui/slint_platform.rs`, `ui/slint_bevy_adapter.rs`, and a possibly-stale `slint_bevy_adapter.rs` — disambiguate first) uses the `RenderGraph` resource + `render_graph::Node` + `RenderLabel`. Guide says the **non-camera** `RenderGraph` schedule "remains" → likely minor or no change, but **VERIFY against docs.rs** for the target bevy before editing. Risk: a mistake here = black viewport (whole editor renders through this path).

### Phase 2 — Build-fix iteration (non-greppable breakage)
After Phase 1, build and fix what the compiler surfaces that grep can't pre-find:
- **Broad-query/resource conflicts** — `Query<()>`/`Query<Entity>`/`Query<EntityMut>` now conflict with resource access; add `Without<…>` filters where startup panics.
- Any signature drift on the RenderGraph node API, Scene module paths, FontSize import path, `from_font_size` helper signature, Atmosphere/ScatteringMedium constructor names, `windows`-crate conflict.
- Per project policy: build with `cargo run` (never `cargo check`).

### Phase 3 (optional, AFTER green) — adopt new features + edition 2024
See below; none block the upgrade.

## NONE-impact categories (verified zero sites)
- **Image/Texture + Mesh** (pixel_bytes Result, DataFormat, MeshMorphWeights, strip_index_format, PlaneMeshBuilder) — zero hits.
- **Reflect + Assets** (DynamicStruct, FieldIter, Interned, AssetPath::resolve, Reader::seekable) — zero hits; we use crate-root re-exports + AssetReader/VecReader.
- **Cargo feature flags / DefaultPlugins** — we use granular subcrate features, never the 2d/3d/ui/audio meta-collections; no UiWidgetsPlugins/InputDispatchPlugin to remove.

## New 0.19 features to adopt (additive, post-upgrade)

| Feature | Value | Where | Priority |
|---|---|---|---|
| **RectLight** (area light) | the "square light" for **SurfaceLight** | surface_light.rs, light_sync.rs, classes.rs SurfaceLight | **SOON** — opt-in (no shadows/cookie yet; keep PointLight fallback); needs `area_light_luts` feature |
| **Vignette + LensDistortion** + finish photoreal post-stack | 0.19 stabilizes `bevy_post_process`/`bevy_anti_alias` — **unblocks photoreal.rs** (the exact blocker it documents) | photoreal.rs, studio_camera_bundle | **SOON** |
| **Contact Shadows** | kills peter-panning | studio_camera_bundle + light spawners (`contact_shadows_enabled`) | SOON |
| White-furnace energy conservation | better env-map reflections | automatic | NOW (free) |
| GPU light clustering / occlusion culling (stable) / skinned culling | big-scene perf (aligns with scaling) | re-tune light_cull.rs budgets; DepthPrepass already present | NOW/SOON |
| InfiniteGrid + DiagnosticsOverlay | editor polish | default_scene, editor | LATER |
| ParallaxCorrection + physical SSR | photoreal reflections | studio camera | LATER |
| Bevy TransformGizmo | replace custom gizmo? | gizmo_tools | LATER / spike only — deeply wired to ModalTool/undo/Slint, do NOT swap blindly |

⚠ Behavioral change to watch: **Bloom luma now computed in linear space** — re-check bloom intensity after enabling the post-stack.

## Open verification points (confirm at build time, not from the guide)
1. The 0.19 Cargo **feature name** for the old scene system (`bevy_scene` vs new) and whether `WorldAssetRoot`/`WorldSerializationPlugin` are in `bevy::prelude`.
2. The surviving **non-camera RenderGraph node API** surface in 0.19 docs.rs.
3. `FontSize` import path; whether `TextFont::from_font_size` param became `FontSize`.
4. Exact `Atmosphere::earth` / `ScatteringMedium::earth` constructor names + signatures.
5. `windows`-crate conflict after rodio 0.22/cpal 0.17.
6. `RectLight` public field names (width/height vs size) — confirm against the 0.19 `rect_light.rs` example.

## Effort estimate
- Phase 0 (deps): ~30 min + one long build.
- Phase 1 (code): ~30–40 verified sites across ~6 crates; mechanical but spread out.
- Phase 2 (build-fix): the real unknown — multiple ~15-min build cycles to converge the non-greppable conflicts (broad queries, RenderGraph, feature-flag semantics). Realistically a few hours of edit→build→fix.
- Total: a focused multi-hour effort, single coherent migration. Commit + push every green milestone so it can never be lost (the prior attempt was lost because it was never committed).
