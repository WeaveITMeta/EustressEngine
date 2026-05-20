# Generative World Layer — Design

**Status:** Spec — awaiting approval before Phase 1. Roadmap covers v1 through
endgame; v2+ phases are spec'd at lighter detail.

## Goal

Add a generative world layer that lets the engine produce, render, and record
AI-generated content using Google Gemini (Imagen for images/textures, Veo for
video) plus a pluggable text-to-3D mesh provider. The Bevy renderer stays the
spine. The engine can record its viewport to MP4. Multiple named cameras live
in the scene with one designated as the main render camera at a time. MCP
cinematography tools drive recording, camera switching, simulation control,
and cutscene authoring from Workshop conversations and external MCP clients.
Genie-style neural simulation is scaffolding only, not implemented in v1.

Imagen and the mesh provider carry the depth of "generative content the
simulation runs on." Imagen produces PBR textures that bind to materials
sampled every frame. The mesh provider produces actual GLB geometry that
becomes addressable ECS entities with colliders, transforms, scripting
handles, and replication channels. Veo produces 2D video frames — content
*for* the scene (cutscene B-roll, animated skyboxes, looped material
textures, camera-path reference clips), not state *of* the scene. The
`NeuralSimulator` trait scaffolded for v4 is the seam where a future
frame-generation world model joins the stack as a *predictive* signal
alongside instanced geometry, not as a replacement for it.

## Roadmap

The generative world layer ships in four major versions plus an endgame state.
v1 lands the local generative loop and cinematography surface. v2 polishes
production use (cache, audio, Python removal). v3 brings local inference and
hybrid routing. v4 wires in predictive / generative world systems. The endgame
state is what the layer enables once all four versions are in.

| Version | Phases | Headline capability                                                            |
| ------- | ------ | ------------------------------------------------------------------------------ |
| v1      | 1–8    | Local generative loop + cinematography via Workshop / MCP                      |
| v2      | 9–13   | Production polish: persistent cache, audio, Python removal                     |
| v3      | 14–17  | Local model inference, hybrid routing, third-party providers                   |
| v4      | 18–21  | Predictive frames, neural simulation seam, generative NPC behavior             |
| Endgame | —      | Continuous worldgen, Workshop multimodal observation, learned authoring style  |

Phase detail decays with horizon. v1 phases have full Scope / Out of scope /
Deliverables / Acceptance / Dependencies sections. v2–v4 phases use a lighter
template (Goal / What it adds / Depends on / Risks). The endgame section
describes capabilities, not numbered phases.

## Brains

**Generative AI is not a feature in the engine — it is how you operate the
engine.**

There is no single generative brain. There is an agent loop sharing the same
controls a human uses. Claude in Workshop reasons and converses. A shared MCP
tool registry lets that loop drive the camera, pause the simulation, generate
textures and meshes, record cutscenes, watch the playback through VIGA's
vision feedback, and iterate on the take in the same conversation. The same
registry is what external IDEs, the editor UI, and remote agents hit — they
are all peers operating one Bevy world. Generation is not bolted on; it is
another tool the agent calls, indistinguishable from clicking the same button
yourself.

### Named roles in the stack

| Role           | Who                                                                  | What it does                                       |
| -------------- | -------------------------------------------------------------------- | -------------------------------------------------- |
| **Strategist** | Claude (Workshop loop)                                               | Plans, decides, converses, dispatches tools        |
| **Hands**      | `eustress-tools` MCP registry                                        | Cameras, sim, recording, file I/O, generation      |
| **Senses**     | VIGA vision-feedback loop                                            | Watches output, compares to intent, drives iteration |
| **Generators** | Imagen / Veo / Meshy or Tripo / (later) local models                 | Produce pixels, polygons, frames                   |
| **World**      | Bevy ECS + simulation                                                | The substrate every agent shares                   |
| **Memory**     | embedvec + Workshop session persistence                              | Recalls prior sessions, prompts, takes             |
| **Imagination**| `NeuralSimulator` (v4+)                                              | Predicts unrealized world states                   |

### Why this is the right shape

Most generative-AI-in-engine work treats AI as a content producer bolted onto
authoring tools — a sidebar that spits out assets. Eustress treats AI as a
peer to the simulation: same Bevy events, same MCP registry, same render
passes, addressable from a chat panel, an external IDE, or a script in the
same way. When Claude calls `camera_set_active`, what happens is exactly
what happens when the user clicks the dropdown. There is no "AI mode." There
is the engine, and a registered set of agents that includes the user.

The differentiator is not any one model. It is that there is one set of
controls and every agent — human or LLM — operates through the same surface.
Workshop iterates on a cutscene in conversation because the conversation has
the same tools the editor has. External IDEs become engine clients for free
because they reach the same registry. Swapping Gemini for a local model in
v3 is a provider impl swap, not a re-architecture.

### How this differs from frame-generation world models

Frame-generation world models (Genie 1/2/3, generic video diffusion models
with action conditioning) generate frames from a learned latent
representation. There is no scene graph, no instanced mesh, no rigid-body
solver, no addressable entities. The "3D world" exists only as the model's
internal state that gets decoded to pixels. A viewer cannot walk around an
object, query its bounds, attach a physics joint to it, or replicate it over
a network because the object itself does not exist — only frames depicting
it exist.

The mesh providers in this layer produce actual GLB geometry that Bevy
instances into the ECS. The mesh has colliders, a transform, scripting
handles, replication channels, and persistence. The simulation runs on it
identically to a hand-authored asset. Recording captures the simulation of
real geometry, not an imagined latent.

Both approaches are useful; they answer different questions. Frame-gen world
models are strong when the goal is "produce a plausible video of a
scenario." Instanced geometry is required when the goal is "simulate a
scenario the user can interact with, script against, replicate to peers, or
modify with code." This layer is built for the second case. The
`NeuralSimulator` trait scaffolded for v4 is the seam where a future
frame-gen model can be integrated as a *predictive* signal alongside the
instanced ECS, not as a replacement for it.

## What this enables

Concrete walkthroughs per version. Detail decays with horizon.

**v1 — Workbench cutaway.** User opens Workshop and types "drop a stylized
industrial workbench into the scene." Claude calls `gen_mesh`, the GLB lands
in `<universe>/assets/meshes/`, and `mesh_import` spawns it as an addressable
entity. User says "now record a 5-second cutaway." Claude calls `sim_pause`,
`camera_spawn`, `camera_set_active` with a 500ms transition, `record_start`,
`sim_resume`, waits 5 seconds of sim time, then `record_stop`. An MP4 lands
in `<universe>/SoulService/Recordings/`. User critiques: "tighter framing,
hold longer on the gauge." Claude updates the cutscene shot list and
re-records. The conversation iterates until the take is good.

**v2 — Cached iteration.** Same loop, now cached. Repeated prompts hit the
disk cache; audio captures the simulation's diegetic sound; the cutscene
timeline UI lets the user scrub between shots and tweak transitions without
re-running the recording.

**v3 — Offline draft.** Same loop, now optionally offline. `LocalProvider`
runs FLUX-class image gen on the user's GPU; hybrid routing sends quick
prototypes to local and final takes to Gemini per the policy file.

**v4 — Predicted continuation.** User asks Workshop "show me what happens if
the load doubles." Claude runs the sim forward through `NeuralSimulator`,
predicting frames past the actual play horizon, and records the predicted
continuation as part of the same MP4.

**Endgame — Streamed terrain.** User flies the camera into unexplored
territory; mesh providers and `NeuralSimulator` extend the world ahead of
the camera in real time; the recording captures the generated terrain as if
it had always been there.

## Architecture overview

### Crate layout

A new workspace member `crates/genworld` owns the trait surface and provider
impls. The existing stub at
[eustress/crates/engine/src/generative_pipeline.rs](../eustress/crates/engine/src/generative_pipeline.rs)
becomes a thin adapter that re-exports `genworld` types and registers
`GenWorldPlugin` into the engine `App` (current registration point is
`eustress/crates/engine/src/main.rs:504`).

```
eustress/crates/genworld/
├── Cargo.toml
├── src/
│   ├── lib.rs              # public surface, GenWorldPlugin
│   ├── provider.rs         # ContentProvider trait, request/response types
│   ├── error.rs            # GenError + Result
│   ├── settings.rs         # GenSettings resource, loaded from env
│   ├── events.rs           # request + response events for Bevy ECS
│   ├── mock.rs             # MockProvider — canned responses, no network
│   ├── gemini/
│   │   ├── mod.rs          # GeminiProvider
│   │   ├── imagen.rs       # text → image
│   │   └── veo.rs          # text → video
│   ├── mesh/
│   │   ├── mod.rs          # MeshProvider trait
│   │   └── (impl module added in Phase 3)
│   └── neural_sim.rs       # NeuralSimulator trait — scaffolding only
└── tests/
    └── gemini_wiremock.rs  # offline HTTP tests using `wiremock`
```

The crate is engine-agnostic at its core: provider impls have no `bevy::*`
imports and only depend on Bevy for `GenWorldPlugin` glue. Provider modules
stay unit-testable without bringing up an `App`.

### `ContentProvider` and `MeshProvider` traits

```rust
#[async_trait::async_trait]
pub trait ContentProvider: Send + Sync + 'static {
    async fn generate_image(&self, req: ImageRequest) -> Result<ImageResponse, GenError>;
    async fn generate_video(&self, req: VideoRequest) -> Result<VideoResponse, GenError>;
}

#[async_trait::async_trait]
pub trait MeshProvider: Send + Sync + 'static {
    async fn generate_mesh(&self, req: MeshRequest) -> Result<MeshResponse, GenError>;
}
```

`ImageResponse` carries either raw PNG/JPEG bytes or a remote URL the engine
can fetch lazily. `VideoResponse` carries an MP4 byte blob and metadata.
`MeshResponse` carries a GLB byte blob — the canonical format
[mesh_import](../eustress/crates/engine/src/mesh_import.rs) already speaks. The
provider impls never touch the filesystem; the Bevy plugin side decides where
bytes land.

The mesh side is its own trait because v1 mesh providers are not Google APIs
(Meshy / Tripo). Splitting keeps `GeminiProvider` from needing a
`generate_mesh` shim if/when Google ships a first-party 3D model.

### `GenWorldPlugin` (Bevy 0.18)

Registered from `engine::generative_pipeline`. Owns the following surface:

- **Resources.** `GenSettings` (env-loaded once at startup), `ProviderRegistry`
  (Arc-held trait objects: image+video provider, mesh provider),
  `JobRegistry` (in-flight tasks keyed by `JobId`).
- **Events.** Request side: `GenerateImageRequest`, `GenerateVideoRequest`,
  `GenerateMeshRequest`, each carrying `{ prompt, params, requester: Entity }`
  and (for video/mesh) a `save_path`. Response side: `ImageGenerated`,
  `VideoGenerated`, `MeshGenerated`, plus `GenerationFailed { job_id,
  requester, error, kind }`.
- **Systems (Update).** `dispatch_generation_requests` drains request events
  and spawns work onto Bevy's `AsyncComputeTaskPool`.
  `poll_generation_jobs` polls `JobRegistry`, writes returned bytes to disk
  under the Universe root (default `<universe>/assets/{images,videos,meshes}/`
  for content, `<universe>/SoulService/Recordings/` for recordings), feeds the
  asset path back through `AssetServer::load`, and emits the matching
  `*Generated` event with the resulting `Handle`.

A Video entity referencing `assets/videos/forge_loop.mp4` is identical at
the asset layer to a Mesh entity referencing `assets/meshes/cube.glb`.
Generated content is indistinguishable from hand-authored content.

Async approach: tokio is already running in the engine binary for QUIC and
Play Server traffic ([eustress/Cargo.toml:63](../eustress/Cargo.toml#L63),
[eustress/crates/engine/Cargo.toml:175](../eustress/crates/engine/Cargo.toml#L175)).
`GenWorldPlugin` spawns its provider calls onto Bevy's `AsyncComputeTaskPool`,
which transparently picks up the existing tokio runtime via `reqwest`'s
internal handle. This matches in spirit the sync-bridge pattern in
[workshop/claude_bridge.rs](../eustress/crates/engine/src/workshop/claude_bridge.rs)
(spawn → poll → emit) but uses Bevy's task pool rather than
`std::thread::spawn` + `Arc<Mutex<Option<Result>>>`, so a second async runtime
is never introduced.

### `GeminiProvider`

Wraps `reqwest::Client` (already an engine dep at
[eustress/crates/engine/Cargo.toml:128](../eustress/crates/engine/Cargo.toml#L128)).
Two surfaces:

- **Imagen.** Text-to-image; used for textured planes and material textures.
  This is the surface that deepens the simulation in a structural way:
  Imagen output binds to PBR materials sampled every frame by the renderer.
- **Veo.** Text-to-video; long-poll style — submit job, poll status, fetch MP4
  bytes on completion.

Veo's role in this layer is narrow and honest: Veo produces 2D frames. The
output is content *for* the scene — cutscene B-roll, animated skyboxes,
looped material textures, camera-path reference clips — not state *of* the
scene. There is no scene graph inside a Veo response, no addressable
entities, no colliders, no scripting handles. Veo shines in cutscene
authoring (where a video clip is exactly the right primitive), in skybox
synthesis (where a looped clip drives the environment shader), and in
looped material animation (where a clip becomes a flipbook texture). It
does not produce world state. Deepening the simulation is Imagen's and the
mesh provider's job; Veo is content composition.

Endpoint URLs, model IDs, request/response shapes, and Veo polling intervals
are read from `https://ai.google.dev/gemini-api/docs` at Phase 2 implementation
time and are deliberately not bound in this spec. The trait contract is:
`GeminiProvider::new(api_key)` constructs from env, the trait methods are
async, and they return typed responses. Anything below the trait boundary can
be rewritten when Google reshapes the API.

### Mesh provider impls

Deferred to Phase 3 against the chosen vendor's current docs. `MeshProvider`
is the only surface engine code binds to; the concrete impl (Meshy or Tripo)
is selected at Phase 3 impl time. Generated GLBs drop into the same watch
tree consumed by
[mesh_import](../eustress/crates/engine/src/mesh_import.rs), so no new
spawn code is needed.

### `MockProvider`

Returns deterministic canned responses: a 256x256 checkerboard PNG, a 60-frame
test-pattern MP4, a cube GLB. No network, no API key required. Lives in
`genworld::mock` with no feature flag — small, useful in release for
diagnostic toggles, and the basis for every offline example and integration
test.

### Where each provider fits

| Provider                | Integration surface                                                                | Depth of simulation impact                         |
| ----------------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------- |
| Imagen (Gemini)         | PBR textures, sprite atlases, normal/roughness maps, cutscene title cards          | Deep — textures bind to materials rendered every frame |
| Mesh provider (Meshy/Tripo) | Actual GLB geometry spawned into the ECS                                       | Deep — geometry has colliders, physics, scripting, replication |
| Veo (Gemini)            | Cutscene B-roll, animated skyboxes, looped material textures, camera-path reference clips | Narrow — Veo output is 2D frames; useful as content, not as world state |
| Claude (Anthropic)      | Workshop orchestration, MCP tool dispatch, scenario generation, NPC behavior scripting | Deep — drives the agent loop, runs scripts via Rune/Luau, composes shots |
| `NeuralSimulator` (v4+) | Predictive frame generation alongside instanced geometry                           | Speculative — first impl in Phase 18; gated on outside research |
| Local models (v3+)      | Same surface as Gemini providers, run on user hardware                             | Same as the API providers they replace            |

Imagen and the mesh providers carry the weight of "deepens the simulation."
Veo is content for the scene, not state of the scene — it shines in cutscene
authoring, skyboxes, and looped material animation. Claude is the strategist
that composes the others.

### Recording — `engine::recording`

[engine::video](../eustress/crates/engine/src/video/mod.rs) is the existing
playback module (mp4 + openh264 decode). Recording is a separate module,
`engine::recording`, owned by the engine crate (not `genworld`). It registers:

- `RecordingPlugin` — events + systems.
- `StartRecording { camera: Option<Entity>, output_path: Option<PathBuf>, fps: Option<u32> }`
  event. When `camera` is `None` the system binds to the entity referenced by
  `ActiveCameraName`. When `output_path` is `None` the system auto-names to
  `<universe>/SoulService/Recordings/<timestamp>.mp4`. When `fps` is `None`
  the default from `RecordingConfig.default_fps` applies.
- `StopRecording` event.
- `RecordCutMarker { label: Option<String> }` event for marking shot
  boundaries inside a single MP4.
- `RecordingConfig` resource: `{ auto_record_on_play: bool,
  follow_sim_time: bool, default_fps: u32, output_root: PathBuf }`.
- `RecordingState` resource: `Idle` | `Recording { job }` | `Suspended { job }`.
- A per-frame readback system that runs after rendering.

The starting point for GPU readback is the screenshot pattern already in use
at
[eustress/crates/engine/src/ui/file_event_handler.rs:547](../eustress/crates/engine/src/ui/file_event_handler.rs#L547),
which uses `bevy::render::view::screenshot::Screenshot::primary_window()`
with an observer on `ScreenshotCaptured`. That is a single-frame helper.
Continuous recording uses a dedicated `Camera` rendering into
`RenderTarget::Image` with per-frame readback of the GPU image; the exact
Bevy 0.18 surface is wired in Phase 5. The per-frame `Screenshot` entity is
the documented fallback if continuous readback proves costly to wire.

Encoder backend lives behind a Cargo feature flag `video-export`:

- **Default: `ffmpeg-sidecar`.** Handles H.264 encode, MP4 container, and
  audio in one shot. Requires `ffmpeg` on PATH (or vendored).
- **Alternative: in-process `openh264` + `mp4`.** Both crates are already
  engine deps at
  [eustress/crates/engine/Cargo.toml:155](../eustress/crates/engine/Cargo.toml#L155)
  for the decode path; both are encode-capable. Trade-off is muxing MP4 by
  hand vs. delegating to ffmpeg.
- A `VideoEncoder` trait sits behind the feature flag so either backend can
  swap in. The encoder accepts a sentinel `Frame::Skip` value so suspended
  recordings produce no frozen-frame artifacts in the output MP4.

Outputs land in `<universe>/SoulService/Recordings/<timestamp>.mp4` by
default. Recordings are runtime artifacts of play sessions and live under
`SoulService/Recordings/` alongside other session traces, distinct from
generated content under `<universe>/assets/`. The Universe-root convention
matches the one used by `mesh_import` and the sim recording config at
[.cargo/simulation.toml](../.cargo/simulation.toml).

### Integration with existing systems

The generative layer composes with five engine subsystems that already
ship: `engine::play_mode`, `engine::camera`, `engine::saved_viewpoints`,
`engine::simulation`, `engine::workshop`. Plus the MCP surfaces in
`crates/mcp`, `crates/mcp-server`, and `engine::engine_bridge`. This
section pins the contracts at the seams.

#### PlayMode lifecycle ↔ recording

`engine::play_mode::PlayModeState`
([eustress/crates/engine/src/play_mode.rs](../eustress/crates/engine/src/play_mode.rs))
is a Bevy `States` enum with `Editor`, `Playing`, and `Paused` variants. F5
and F7 enter `Playing`; F6 toggles `Paused`; F8 returns to `Editor`. Runtime
side effects of the state transitions are owned by
[engine::play_mode_runtime](../eustress/crates/engine/src/play_mode_runtime.rs).

`engine::recording` installs three state-driven systems against this enum:

- An `OnEnter(PlayModeState::Playing)` system inspects
  `RecordingConfig.auto_record_on_play`. When set, it emits
  `StartRecording { camera: None /* bind to active */, output_path: None
  /* auto-name */, fps: None /* default */ }`. The handler that consumes
  the event resolves the active camera through `ActiveCameraName` and opens
  the encoder against its render target.
- An `OnExit(PlayModeState::Playing)` system fires whenever play mode
  transitions to any non-`Playing` state (typically `Editor`). If
  `RecordingState` is in `Recording` or `Suspended`, it emits
  `StopRecording`, which finalizes the encoder and flushes the container.
- An `OnEnter(PlayModeState::Paused)` system consults
  `RecordingConfig.follow_sim_time`. When set (default `true`), it
  transitions `RecordingState::Recording` to `RecordingState::Suspended`.
  While suspended, the per-frame readback system emits `Frame::Skip` to the
  encoder so the produced MP4 has no frozen-frame stretch.
  `OnEnter(PlayModeState::Playing)` from `Paused` reverses the transition.
  When `follow_sim_time` is `false`, frame capture continues unchanged
  during `Paused`, allowing cinematic camera moves over a frozen scene.

A recording captures the active camera's view of the simulated scene, not
the editor scene. The active-camera binding is re-resolved on every
`SetActiveCameraEvent`, so mid-recording switches reroute the frames
without closing the encoder.

`RecordingConfig` defaults:

| Field                  | Default                                       |
| ---------------------- | --------------------------------------------- |
| `auto_record_on_play`  | `true`                                        |
| `follow_sim_time`      | `true`                                        |
| `default_fps`          | `60`                                          |
| `output_root`          | `<universe>/SoulService/Recordings/`          |

Manual `record_start` / `record_stop` MCP tools work regardless of
`PlayModeState`. They emit the same `StartRecording` / `StopRecording`
events; the play-mode-driven systems are additive triggers, not gatekeepers.

#### Multi-camera and active-camera switching

`engine::camera`
([eustress/crates/engine/src/camera.rs](../eustress/crates/engine/src/camera.rs))
ships one `StudioCamera` with `Orbit` / `Free` / `FirstPerson` modes, with
input driven by
[engine::camera_controller](../eustress/crates/engine/src/camera_controller.rs).
The generative layer extends this with named cameras and a designated
main render camera. New surface in `engine::camera`:

- `NamedCamera { name: String }` component on each addressable camera
  entity. Names are scene-unique; duplicate spawn returns an error.
- `ActiveCameraName(Option<String>)` resource pointing at the current
  main render camera by name.
- `SetActiveCameraEvent { name: String, transition: Option<CameraTransition> }`
  switches the main render camera.
- `CameraTransition { duration: Duration, easing: Easing }` interpolates
  pose (translation, rotation) and FOV from the previous camera to the
  target. `Easing` covers `Linear`, `EaseInOut`, `Cubic`.
- `CameraTransitionState` component on the target camera while a
  transition is in flight.
- System `apply_active_camera` reads `ActiveCameraName` and toggles
  `Camera::is_active` so exactly one `NamedCamera` renders to the primary
  window. The semantically correct Bevy 0.18 symbol is wired at impl time.
- System `interpolate_camera_transitions` drives any in-flight
  `CameraTransitionState` to completion.

Spawning policy: the existing `StudioCamera` is tagged
`NamedCamera { name: "editor".into() }` during plugin startup, so
`ActiveCameraName` is `Some("editor".into())` from the first frame.
User-created cameras come from `camera_spawn` (or scripts).

Relation to
[engine::saved_viewpoints](../eustress/crates/engine/src/saved_viewpoints.rs):
viewpoints are saved *poses*; named cameras are *entities* that persist
their own pose. The two compose through an optional `camera: Option<String>`
field added to `SaveViewpointEvent` / `LoadViewpointEvent` /
`DeleteViewpointEvent`. Absent means "apply to active camera," preserving
existing behavior. Recording binds through `ActiveCameraName`, so
mid-recording switches reroute frames without encoder reset.

#### Simulation pause/resume/step

`engine::play_mode` already owns play/pause through `PlayModeState`. The
generative layer adds three events that drive the same state transitions
without going through keybindings:

- `PauseSimulationEvent` — transitions `PlayModeState` to `Paused`.
- `ResumeSimulationEvent` — transitions `PlayModeState` from `Paused` to
  `Playing`. No-op when not currently paused.
- `StepSimulationEvent { ticks: u32 }` — runs the existing simulation
  tick loop for `ticks` frames while paused, then returns to `Paused`.
  Uses the hooks under
  [engine::simulation](../eustress/crates/engine/src/simulation/) to
  advance the world by exactly `ticks` updates regardless of wall-clock
  frame rate.

These events are what the MCP `sim_pause` / `sim_resume` / `sim_step`
tools emit. Manual F5 / F6 / F8 keybindings remain the user-facing path;
MCP is the programmatic path. Both share the same `PlayModeState` and
therefore all downstream effects including the recording lifecycle
systems above.

#### MCP tool registration in `eustress-tools`

All new tools register in the existing `eustress-tools` workspace crate
([eustress/crates/tools/src/lib.rs:63](../eustress/crates/tools/src/lib.rs#L63)
— `register_all_tools`). The tool surface added by this layer:

| Group        | Tools                                                                                                                                          |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Recording    | `record_start`, `record_stop`, `record_cut`, `record_status`                                                                                   |
| Camera       | `camera_list`, `camera_spawn`, `camera_delete`, `camera_set_active`, `camera_move`, `camera_frame_entity`, `camera_transition`, `camera_dolly` |
| Simulation   | `sim_pause`, `sim_resume`, `sim_step`, `sim_status`                                                                                            |
| Generative   | `gen_image`, `gen_video_submit`, `gen_video_status`, `gen_mesh`, `gen_provider_status`                                                         |
| Cutscene     | `cutscene_define`, `cutscene_record`, `cutscene_list`                                                                                          |

Each tool follows the existing tool pattern: a `ToolDefinition` (name,
description, JSON schema) and a handler that takes ECS access via the
registry's Bevy bridge and emits the appropriate event. Recording tools
forward to `StartRecording` / `StopRecording` / `RecordCutMarker`. Camera
tools forward to `SetActiveCameraEvent` and direct ECS writes for
spawn/move/delete; `camera_frame_entity` solves a pose framing the target
AABB. Simulation tools forward to `PauseSimulationEvent` /
`ResumeSimulationEvent` / `StepSimulationEvent`. Generative tools drive
`GenerateImageRequest` / `GenerateVideoRequest` / `GenerateMeshRequest`
and `JobRegistry` lookups. Cutscene tools register/load/execute
`Cutscene` definitions.

Handlers are **synchronous from Claude's perspective**: they emit the
event, then poll a completion condition (or a `JobRegistry` entry) with a
per-tool timeout, then return the result. This avoids race conditions when
Claude chains tools (for example `record_start` followed immediately by
`cutscene_record` — the second tool sees the first tool's effects because
the first tool did not return until the recording transitioned to
`RecordingState::Recording`). Default per-tool timeouts:

| Tool category              | Timeout |
| -------------------------- | ------- |
| Camera, simulation, record | 5 s     |
| Cutscene execution         | 5 min   |
| Image generation           | 90 s    |
| Video submission           | 30 s    |
| Video status               | 5 s     |
| Mesh generation            | 5 min   |

The tools are exposed across three callers:

- **Workshop's Claude agentic loop.** Tool calls route through
  `eustress::workshop::tools::ToolRegistry`
  ([eustress/crates/engine/src/workshop/tools/mod.rs](../eustress/crates/engine/src/workshop/tools/mod.rs))
  which delegates to `eustress-tools`. No Workshop-specific code is added
  for the new tools.
- **External MCP clients** (Claude Desktop, Cursor, Windsurf) via
  [crates/mcp-server](../eustress/crates/mcp-server/) sitting atop
  [crates/mcp](../eustress/crates/mcp/). The server enumerates the same
  registry.
- **Sibling processes** via the `engine_bridge` JSON-RPC surface
  ([eustress/crates/engine/src/engine_bridge/](../eustress/crates/engine/src/engine_bridge/)),
  which exposes a localhost TCP transport for the same tool list.

#### Workshop conversational dispatch

[engine::workshop](../eustress/crates/engine/src/workshop/) already runs a
Claude agentic loop and routes tool calls through
`eustress::workshop::tools::ToolRegistry`, which delegates to
`eustress-tools`. Registering the new tools in `eustress-tools` makes them
conversationally callable from Workshop with zero Workshop-specific code.
Worked example, user to Workshop: "Record a 5-second cutaway of the
battery from the side, then resume." Claude calls:

1. `sim_pause`
2. `camera_spawn(name="cutaway", position=..., look_at=...)`
3. `camera_set_active(name="cutaway", transition={duration: 0.5})`
4. `record_start`
5. `sim_resume`
6. *(waits ~5 s of sim time)*
7. `record_stop`
8. `camera_set_active(name="editor")`

Each step is a discrete MCP tool call surfaced to the user in the Workshop
chat with approval gates per the existing MCP approval flow. The MP4 lands
under `<universe>/SoulService/Recordings/<timestamp>.mp4`. The synchronous
handler contract guarantees that step 4 returns only after `RecordingState`
is `Recording`, so step 5 cannot race the recording open.

#### Cutscene composition

A cutscene is a typed sequence of shots:

```rust
pub struct Cutscene {
    pub name: String,
    pub shots: Vec<Shot>,
}

pub struct Shot {
    pub camera: String,                       // named camera to make active
    pub transition: Option<CameraTransition>, // how to get there
    pub duration: Duration,                   // sim time on this shot
    pub sim_paused: bool,                     // pause sim during this shot
    pub on_enter: Vec<ShotAction>,            // camera_move, gen_image overlay, etc.
}
```

`ShotAction` is an enum covering the per-shot actions the cutscene runner
can take: `CameraMove`, `CameraDolly`, `FrameEntity`, `GenImageOverlay`,
and `RecordCut`. Each variant maps onto an existing tool's event so the
runner has no new code paths to maintain.

`cutscene_record(name)` validates the cutscene (every shot's `camera` must
resolve to an existing `NamedCamera`; every `FrameEntity.target` must
resolve), emits `StartRecording`, then walks `shots` in order:

1. Emit `SetActiveCameraEvent { name: shot.camera, transition: shot.transition }`.
2. If `shot.sim_paused`, emit `PauseSimulationEvent`; otherwise
   `ResumeSimulationEvent`.
3. Execute each `on_enter` action against the current shot's camera.
4. Sleep `shot.duration` of sim time (wall-clock when sim is paused).
5. Emit `RecordCutMarker { label: Some(shot.camera.clone()) }`.

After the final shot, emit `StopRecording`. The recording captures every
shot in a single MP4 with cut markers separating them. Validation failure
refuses the cutscene before any state change, naming the first invalid
reference. Cutscenes load from TOML files under `<universe>/Cutscenes/`,
mirroring the convention used for other authored content; `cutscene_list`
enumerates both runtime-registered and on-disk cutscenes.

#### Iterative cutscene loop

The combination of Workshop's Claude agentic loop, the `engine::viga`
vision-feedback pipeline ([eustress/crates/engine/src/viga/](../eustress/crates/engine/src/viga/)),
and the cinematography tools above composes into an iterative cutscene
authoring loop. This is the primary user-facing payoff of the layer:

1. **Compose.** User describes the desired shot in conversation. Claude
   calls `cutscene_define` or composes shots ad-hoc through `sim_pause`
   / `camera_*` / `record_*` tools.
2. **Record.** Claude calls `cutscene_record`. The simulation runs from
   the chosen cameras and the recording captures the active camera's view
   as MP4 frames, landing under `<universe>/SoulService/Recordings/`.
3. **Observe.** The completed MP4 path is fed back into the conversation.
   Workshop's multimodal context reads the recording.
4. **Critique.** User responds in natural language ("tighter framing on
   the second shot", "hold the camera longer on the gauge"). Claude maps
   the critique to concrete tool calls — `camera_move`,
   `camera_frame_entity`, or a `Cutscene` edit.
5. **Re-record.** Cycle repeats until the take meets intent.

`engine::viga` is the existing vision-as-inverse-graphics pipeline that
converges on a static reference image through generate → render → compare
→ iterate. The cinematography surface generalizes this: instead of
converging on a fixed reference, the loop converges on a user's
natural-language description of a shot. Same shape, different target.

### `NeuralSimulator` trait

```rust
#[async_trait::async_trait]
pub trait NeuralSimulator: Send + Sync + 'static {
    /// Given a window of past (frame, action) pairs, predict the next frame.
    async fn step(&self, history: &[NeuralFrame], action: &NeuralAction)
        -> Result<NeuralFrame, GenError>;
}
```

Lives in `genworld::neural_sim`. No engine code calls this in v1.

Honest disclosure in the trait doc-comment: no public Google API exposes
Project Genie as a callable service today; Genie 2/3 are research
artifacts. The trait is scaffolding for future providers (Google, Meta
V-JEPA, World Labs). The natural caller when an impl lands is
[engine::viga](../eustress/crates/engine/src/viga/) — its
iterate-and-verify loop is already shaped like a neural simulator step
(registration sits at `eustress/crates/engine/src/main.rs:506`).

## Configuration & secrets

Env vars are loaded once at `GenWorldPlugin` startup via `std::env::var`,
never logged, never committed:

- `GEMINI_API_KEY` — required for `GeminiProvider`. When absent,
  `GenSettings.gemini` is `None` and image/video requests fail with
  `GenError::ProviderUnavailable`. `MockProvider` stays available.
- `MESH_PROVIDER_API_KEY` — required for the chosen mesh provider. Same
  fallback behavior.
- `GENWORLD_OUTPUT_ROOT` — optional override for the Universe root that
  generated content and recordings sit under. Defaults to the active
  Universe directory; generated content lands under
  `<root>/assets/{images,videos,meshes}/`, recordings under
  `<root>/SoulService/Recordings/`.

BYOK follows the existing convention: `ANTHROPIC_API_KEY` is consumed by
`SoulServiceSettings` at
[eustress/crates/common/src/soul/config.rs:255](../eustress/crates/common/src/soul/config.rs#L255),
and `SoulServiceSettings::effective_api_key()` is the workspace accessor.
The new Gemini and mesh keys are added either to `SoulServiceSettings` or to
a sibling `GenWorldSettings` in the same module so the BYOK UI in
[soul_settings.slint](../eustress/crates/engine/ui/slint/) can be extended in
one place.

A new `eustress/.env.example` lists the new keys with placeholder values.
`.env` and `.env.*.local` are already gitignored.

## Failure modes

Failure modes listed below cover v1 surface area. Each later phase adds its
own failure modes documented in that phase's Risks line; only failures that
change v1 behavior are added here.

User-facing behavior in the Slint generation panel and MCP tool surface:

| Failure                                                                              | Behavior                                                                                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| API key missing/misconfigured                                                        | Generation button disabled with tooltip "Set GEMINI_API_KEY in Soul Settings → API Keys". MockProvider remains available.                          |
| API rate limit (HTTP 429)                                                            | Toast "Rate limited — retry in N seconds" via `engine::notifications`. Job kept in `JobRegistry` until manually dismissed; no auto-retry storm.   |
| Network failure / DNS / TLS error                                                    | Toast with first line of error. Job marked `Failed` in registry. Provider stays available for retry.                                              |
| Partial generation (Veo job times out mid-poll)                                      | Job marked `Stalled` after configurable timeout (default 10 min). User can resume or cancel from panel.                                          |
| Generated GLB import fails (mesh provider returned junk)                             | Toast + `GenerationFailed` event with the import error. Bytes preserved on disk for inspection. Mesh is *not* spawned.                            |
| Output directory unwritable                                                          | Generation refused upfront with toast "Cannot write to <path>"; nothing dispatched.                                                              |
| Recording: encoder process crash                                                     | Recording stopped; partial MP4 left on disk (ffmpeg flushes container on SIGTERM). Toast surfaces stderr tail.                                   |
| Recording: GPU readback queue overrun                                                | Drop oldest frame, log warning every 60 dropped frames. Visible in `engine::frame_diagnostics` HUD.                                              |
| No active camera (`ActiveCameraName` is `None`) when recording starts                | `StartRecording` handler refuses with toast "No active camera; spawn or pick one first." Pre-flight check rejects the event before encoder open. |
| MCP `camera_set_active` for an unknown name                                          | Tool returns error with the list of valid `NamedCamera` names. No state change; no `SetActiveCameraEvent` emitted.                                |
| `sim_pause` while already paused / `sim_resume` while already playing                | Tools return success no-op (idempotent), log at trace. `PlayModeState` is untouched.                                                              |
| Cutscene shot references a camera that does not exist                                | Cutscene refused at validation time with the missing camera name surfaced. No partial execution; no recording started.                            |
| Recording-in-flight when leaving Play mode unexpectedly (crash, panic-recovery)      | Encoder receives `SIGTERM`, partial MP4 preserved on disk, a `.recovered` sidecar JSON records the last-good cut marker offset.                   |
| MCP tool timeout (post-condition never met within the per-tool budget)               | Tool returns timeout error; the underlying Bevy event may still process, but the tool reports failure to Claude with the elapsed wait.            |
| Duplicate `camera_spawn` name                                                        | Tool returns error naming the existing camera; no entity spawned.                                                                                 |
| `camera_delete` targeting the active camera                                          | Tool refuses with "Switch active camera before deleting." No deletion.                                                                            |

## Non-goals (out of scope at every horizon)

- Training pipelines for the user's own foundation models from scratch. Only
  fine-tuning against pre-existing base models is in scope (v4 / endgame).
- Generic AI agents that operate outside the engine context. The layer is
  about generating content the engine renders and records, not about
  building a general-purpose autonomous agent product.
- Real-time camera teleoperation from a remote networked client. The MCP and
  `engine_bridge` surfaces stay local-process-only across every version;
  remote control is a deployment concern outside this layer.
- Reimplementing the existing Anthropic HTTP client. The canonical client at
  [eustress/crates/engine/src/soul/claude_client.rs](../eustress/crates/engine/src/soul/claude_client.rs)
  stays the workspace Claude path; `genworld` does not replace it.
- Replacing the Bevy renderer with a neural renderer. Neural simulation
  produces frames that the existing renderer composites into the final
  image; the rasterizer stays the spine end-to-end.

## Project decisions

- **UI.** New Slint panel in
  [eustress/crates/engine/ui/slint/](../eustress/crates/engine/ui/slint/). No
  egui (egui is workspace-removed; see comment at
  [eustress/Cargo.toml:44](../eustress/Cargo.toml#L44)).
- **Orchestration split.** Gemini is the media provider (Imagen for images,
  Veo for video) plus a pluggable text-to-3D mesh provider. The existing
  Claude-driven [workshop](../eustress/crates/engine/src/workshop/) +
  [viga](../eustress/crates/engine/src/viga/) flow stays as the orchestrator
  and may call into `genworld` for media synthesis. The canonical Anthropic
  HTTP client at
  [eustress/crates/engine/src/soul/claude_client.rs](../eustress/crates/engine/src/soul/claude_client.rs)
  is not replaced.
- **Crate layout.** `crates/genworld` owns trait + provider impls.
  `engine::generative_pipeline` becomes the thin adapter that re-exports
  `genworld` types and registers `GenWorldPlugin`.
- **Recording module name.** `engine::recording`. `engine::video` stays
  reserved for the existing playback module.
- **Recording lifecycle is bound to `PlayModeState`.** Entering `Playing`
  starts recording when `RecordingConfig.auto_record_on_play=true`
  (default). Exiting `Playing` finalizes the MP4. Manual `record_start` /
  `record_stop` MCP tools work in either mode and bypass the auto-trigger
  flag.
- **Pause behavior.** A pause in the simulation pauses frame capture by
  default (`follow_sim_time=true`). The user can opt into capture-while-
  paused for cinematic camera moves over frozen scenes by flipping the
  flag.
- **Camera switching.** Implemented as `NamedCamera` component +
  `ActiveCameraName` resource + `SetActiveCameraEvent`. The existing
  `StudioCamera` becomes the named `"editor"` camera. Existing
  `saved_viewpoints` composes — viewpoints are poses applicable to any
  named camera through the optional `camera` field on viewpoint events.
- **Cinematography tool ownership.** All cinematography tools live in
  `eustress-tools`. Workshop's Claude conversational loop and external MCP
  clients (via `crates/mcp-server`) and sibling processes (via
  `engine_bridge`) share the same registry; no Workshop-specific tool
  plumbing is added.
- **MCP tool synchrony.** MCP tools are synchronous from the caller's view:
  emit event, poll completion condition with timeout, return result. This
  prevents tool-chaining races when Claude composes multiple steps in a
  single turn.
- **Encoder backend.** `ffmpeg-sidecar` is the v1 default — covers H.264,
  container, and audio with no native encoder code to maintain. In-process
  `openh264` + `mp4` is documented as a swappable alternative behind the
  same `VideoEncoder` trait. Both behind the `video-export` Cargo feature.
  The encoder accepts a `Frame::Skip` sentinel so suspended recordings
  produce continuous MP4s with no frozen-frame artifact.
- **Mesh provider.** Meshy or Tripo, decided at Phase 3 impl time against
  current vendor docs. `MeshProvider` trait insulates the rest of the code.
- **`NeuralSimulator` trait location.** `crates/genworld`. Expected caller
  when an impl exists is `engine::viga`.
- **Output paths.** Generated content lands under
  `<universe>/assets/{meshes,images,videos}/` so it is indistinguishable
  from hand-authored assets. Recordings land under
  `<universe>/SoulService/Recordings/` as runtime artifacts of play
  sessions. The `<universe>/Generated/` bucket from earlier drafts is
  removed. Cutscene definitions live under `<universe>/Cutscenes/`.
- **Python pipeline.** Deprecated in favor of the Rust path. Phase 8 adds
  `#[deprecated]` annotations, doc banners, and legacy headers on the Python
  scripts. Removal lands in Phase 11 (v2).
- **CI.** No CI gate for genworld. Local `cargo check --workspace` and
  `cargo clippy --workspace -- -D warnings` only. CI today
  ([.github/workflows/ci.yml](../.github/workflows/ci.yml)) does not build
  the main `eustress/` workspace, and that stays as-is for v1.
- **Roadmap structure.** The roadmap is structured into four major versions
  plus an endgame state. v1 phases have full Scope / Out of scope /
  Deliverables / Acceptance / Dependencies sections. v2 through v4 phases
  use a lighter Goal / What it adds / Depends on / Risks template. The
  endgame is described as a set of capabilities, not numbered phases.

# ────────────────────────────────────────────────────────────────────
# v1 — Local generative loop + cinematography
# ────────────────────────────────────────────────────────────────────

v1 stands up the trait surface, the Gemini and mesh providers, the recording
pipeline, the multi-camera system, the full MCP cinematography tool set, the
Slint generation panel, and the Python pipeline deprecation pass. By the end
of v1 a user can prompt for content, switch cameras, and record cutscenes
end-to-end from Workshop and external MCP clients. Genie-style neural
simulation is scaffolding only; persistent cache, audio synthesis, and Python
removal are deferred to v2.

## Phase 1: Scaffold `crates/genworld`

**Scope.** Add the workspace crate with trait surface, request/response
types, error type, settings loader, Bevy plugin glue, and `MockProvider`.
Wire it into `engine::generative_pipeline` as the thin adapter that
registers `GenWorldPlugin` into the engine `App`. Add an in-crate example
demonstrating end-to-end mock generation.

**Out of scope.** No network. No Gemini code. No mesh code. No recording.
No camera changes. No MCP tools. No Slint panel. No deletion or
deprecation of the legacy Python path.

**Deliverables.**

- `eustress/crates/genworld/Cargo.toml` and source tree as laid out under
  *Crate layout* above. Added to the workspace members list in
  [eustress/Cargo.toml](../eustress/Cargo.toml).
- `ContentProvider`, `MeshProvider`, `NeuralSimulator` traits (the third
  defined but not invoked).
- `ImageRequest`/`Response`, `VideoRequest`/`Response`,
  `MeshRequest`/`Response`, `GenError`, `JobId`, `GenKind` types.
- `GenSettings` resource loaded from env in `GenWorldPlugin::build`.
- `GenWorldPlugin` registering the seven events and two systems above.
- `MockProvider` returning canned PNG / MP4 / GLB.
- `engine::generative_pipeline` updated to re-export `genworld` types and
  register `GenWorldPlugin`.
- In-crate example `eustress/crates/genworld/examples/genworld_hello.rs`
  driving the mock end-to-end. Runs via `cargo run --example
  genworld_hello -p genworld`.
- New deps on `genworld` Cargo.toml:
  - `bevy` (workspace) — plugin/event/resource integration.
  - `async-trait` — `ContentProvider` and siblings are async trait objects.
  - `serde`, `serde_json` (workspace) — request/response (de)serialization.
  - `thiserror` (workspace if present, else add) — `GenError`.

**Acceptance.** `cargo check --workspace` and `cargo clippy --workspace --
-D warnings` pass locally. `cargo run --example genworld_hello -p genworld`
prints a `MeshGenerated`/`ImageGenerated`/`VideoGenerated` sequence using
`MockProvider`. Engine binary builds and starts; nothing visible changes
in-app yet.

**Dependencies.** None.

## Phase 2: GeminiProvider (Imagen + Veo)

**Scope.** Real `GeminiProvider` implementation against the live Gemini API
docs. Imagen first (text-to-image), Veo second (text-to-video with
long-poll). Offline tests against a `wiremock` server so `cargo test` does
not need a live `GEMINI_API_KEY`.

Imagen is the deeper integration — its output binds to PBR materials the
renderer samples every frame. Veo ships in this phase as the cutscene /
skybox / looped-texture provider; its 2D-frame output is content for the
scene, not state of the scene.

**Out of scope.** Mesh provider. Recording. Camera changes. MCP tools.
Slint panel. Python deprecation. Gemini audio surfaces.

**Deliverables.**

- `genworld::gemini::imagen` — `ContentProvider::generate_image`
  implementation. Endpoint, model, request/response shape read from
  `https://ai.google.dev/gemini-api/docs` at impl time and not bound here.
- `genworld::gemini::veo` — `ContentProvider::generate_video`
  implementation with submit → poll → fetch flow. Polling interval and
  status-shape read from live docs.
- `genworld::gemini::GeminiProvider::new(api_key) -> Self`, constructed
  from `GEMINI_API_KEY` in `GenSettings`.
- Wired into `ProviderRegistry` when `GenSettings.gemini.is_some()`;
  falls back to `MockProvider` otherwise.
- `eustress/crates/genworld/tests/gemini_wiremock.rs` covering happy
  path, HTTP 429, and Veo poll timeout. `wiremock` mounted as a
  dev-dependency only.
- New deps on `genworld` Cargo.toml:
  - `reqwest` (workspace) — HTTP client, JSON support; already used by
    engine for Claude.
  - `tokio` (workspace) — runtime features needed for `reqwest` async
    body handling.
  - `base64` — Imagen returns base64-encoded image payloads; needed to
    decode to bytes before writing to disk.
  - `wiremock` (dev-dep) — offline HTTP mocking for unit tests so CI and
    contributors without a key can still run `cargo test`.

**Acceptance.** With `GEMINI_API_KEY` set, the in-crate example
`genworld_hello` (extended in this phase or a new `gemini_hello.rs`)
returns a real Imagen PNG and writes it to
`<universe>/assets/images/<timestamp>.png`, and a Veo MP4 lands at
`<universe>/assets/videos/<timestamp>.mp4`. `cargo test -p genworld`
passes with no live key. Local `cargo clippy --workspace -- -D warnings`
clean.

**Dependencies.** Phase 1.

## Phase 3: Mesh provider

**Scope.** Concrete `MeshProvider` implementation (Meshy or Tripo,
decided at impl time against their current docs). GLBs land in the
filesystem under the Universe root and are picked up by the existing
mesh import watcher.

**Out of scope.** Recording. Camera changes. MCP tools. Slint panel.
Python deprecation.

**Deliverables.**

- `genworld::mesh::<vendor>` — concrete implementation of `MeshProvider`.
  Endpoint, model, request/response shape read from the vendor's live docs
  at impl time. The vendor choice is recorded in the commit message with a
  one-line justification.
- `MESH_PROVIDER_API_KEY` plumbed through `GenSettings`. Absent →
  `ProviderRegistry.mesh` is `None` and mesh requests fail with
  `GenError::ProviderUnavailable`.
- Generated GLBs written to `<universe>/assets/meshes/<timestamp>.glb`.
  Spawn happens through
  [mesh_import](../eustress/crates/engine/src/mesh_import.rs); no new
  spawn code in `genworld` or the engine.
- Test mock under `wiremock` for the chosen vendor's request flow.
- Any new deps added to `genworld` Cargo.toml are accompanied by a one-line
  justification in the commit message.

**Acceptance.** With `MESH_PROVIDER_API_KEY` set, the example produces a
real GLB on disk, and the engine binary auto-spawns it as an Instance
entity through the existing mesh-import watcher. Without the key,
`MockProvider`'s cube GLB still works. `cargo test -p genworld` passes
without a live key.

**Dependencies.** Phase 1.

## Phase 4: Multi-camera system

**Scope.** Extend `engine::camera` with named cameras and a designated main
render camera. Tag the existing `StudioCamera` as `NamedCamera { name:
"editor" }` on startup. Add the events, resource, and systems that switch
the active camera and interpolate transitions. No MCP wiring in this
phase; the surface this phase ships is the Bevy-side API that later phases
call.

**Out of scope.** MCP tools (Phase 6). Slint UI for camera picking
(Phase 7). Recording integration (Phase 5 binds recording to
`ActiveCameraName`).

**Deliverables.**

- `NamedCamera { name: String }` component in `engine::camera`.
- `ActiveCameraName(Option<String>)` resource. Initialized to
  `Some("editor".into())` during plugin startup when the default
  `StudioCamera` is spawned with `NamedCamera { name: "editor".into() }`.
- `SetActiveCameraEvent { name, transition }` and
  `CameraTransition { duration, easing }` types in `engine::camera`.
- `CameraTransitionState` component on the target camera during a
  transition.
- System `apply_active_camera` that reads `ActiveCameraName` and toggles
  `Camera::is_active` on `NamedCamera` entities to elect the single main
  render camera.
- System `interpolate_camera_transitions` that drives `CameraTransitionState`
  to completion and clears it.
- Optional `camera: Option<String>` field added to existing
  `SaveViewpointEvent` / `LoadViewpointEvent` / `DeleteViewpointEvent`
  ([eustress/crates/engine/src/saved_viewpoints.rs](../eustress/crates/engine/src/saved_viewpoints.rs))
  so viewpoints can target a specific named camera. Absent value means
  "active camera," preserving existing behavior.
- Helper API `engine::camera::spawn_named_camera(name, pose, fov)` used by
  later phases.

**Acceptance.** A system test spawns a second camera with
`NamedCamera { name: "cutaway" }`, fires `SetActiveCameraEvent { name:
"cutaway".into(), transition: None }`, and confirms that
`Camera::is_active` toggles from `editor` to `cutaway` and the primary
window's render target reflects the swap on the next frame. A second test
exercises a non-zero `CameraTransition` and confirms pose interpolation
completes within the requested duration. `cargo check --workspace` clean.

**Dependencies.** None on `genworld`; can land in parallel with Phases 2
or 3.

## Phase 5: Recording + PlayMode integration

**Scope.** Add `engine::recording` with `RecordingPlugin`,
`RecordingConfig`, `StartRecording` / `StopRecording` / `RecordCutMarker`
events, `RecordingState` resource, the per-frame readback system, and the
`ffmpeg-sidecar` encoder backend behind the engine-crate `video-export`
Cargo feature. Wire the play-mode-driven lifecycle hooks: auto-record on
`OnEnter(Playing)`, suspend frame capture on `OnEnter(Paused)` when
`follow_sim_time=true`, finalize on `OnExit(Playing)`. Recording binds to
the active camera through `ActiveCameraName`.

**Out of scope.** Audio recording. Multi-camera recording (a single main
camera at a time is captured; switches reroute the stream). Streaming
output. MCP tools (Phase 6). Slint UI surface (Phase 7).

**Deliverables.**

- `eustress/crates/engine/src/recording/mod.rs` — `RecordingPlugin`,
  `RecordingConfig` (with the defaults table from *Architecture
  overview*; `output_root` defaults to
  `<universe>/SoulService/Recordings/`), `RecordingState`,
  `StartRecording`, `StopRecording`, `RecordCutMarker`.
- Per-frame readback system. Starting point is the
  `Screenshot::primary_window()` pattern at
  [eustress/crates/engine/src/ui/file_event_handler.rs:547](../eustress/crates/engine/src/ui/file_event_handler.rs#L547);
  the continuous variant uses a dedicated `Camera` with
  `RenderTarget::Image` and per-frame readback. The exact Bevy 0.18
  symbols are wired against the live API at impl time.
- Active-camera binding: the readback system resolves the entity
  referenced by `ActiveCameraName` each frame, so mid-recording switches
  via `SetActiveCameraEvent` reroute without encoder reset.
- Play-mode-driven systems:
  - `OnEnter(PlayModeState::Playing)` → emit `StartRecording` when
    `RecordingConfig.auto_record_on_play` is set.
  - `OnExit(PlayModeState::Playing)` → emit `StopRecording` when
    `RecordingState` is `Recording` or `Suspended`.
  - `OnEnter(PlayModeState::Paused)` → transition
    `RecordingState::Recording` → `RecordingState::Suspended` when
    `RecordingConfig.follow_sim_time` is set.
  - `OnEnter(PlayModeState::Playing)` from `Paused` → reverse the suspend.
- `VideoEncoder` trait + `FfmpegSidecarEncoder` implementation behind
  `--features video-export`. Encoder accepts `Frame::Skip` to advance the
  timestamp without writing a frame.
- Output path: `<universe>/SoulService/Recordings/<timestamp>.mp4`.
- Integration test: a headless harness drives `PlayModeState` through
  `Editor → Playing → Paused → Playing → Editor`, confirming the MP4 is
  finalized with the expected duration (paused stretch excluded) and one
  cut marker. Run manually, not in CI.
- New deps on `engine` Cargo.toml:
  - `ffmpeg-sidecar` — wraps the ffmpeg CLI for H.264 + MP4 + audio
    without committing to a native encoder; default backend for v1.

**Acceptance.** With `--features video-export`, pressing F5 enters
`Playing`, an MP4 starts under
`<universe>/SoulService/Recordings/<timestamp>.mp4`; F6 pauses the sim and
the frame timestamps in the MP4 stop advancing; F6 again resumes and the
MP4 continues; F8 returns to `Editor` and the MP4 is finalized and plays
back in a standard player. Without the feature, the engine builds and
runs as before. Frame-drop logging surfaces in `engine::frame_diagnostics`
when the GPU readback queue overruns.

**Dependencies.** Phase 4 (`ActiveCameraName` must exist before recording
can bind to it).

## Phase 6: MCP cinematography tools + Workshop integration

**Scope.** Register the full cinematography tool set in `eustress-tools`.
Plumb `PauseSimulationEvent` / `ResumeSimulationEvent` /
`StepSimulationEvent` through `engine::play_mode`. Each tool emits the
appropriate Bevy event and synchronously waits for the matching
completion signal with the per-tool timeout. Workshop picks up the tools
through the existing `eustress::workshop::tools::ToolRegistry` delegation.
`crates/mcp-server` exposes the same tools to external MCP clients;
`engine_bridge` exposes them to sibling processes.

**Out of scope.** Slint UI surface (Phase 7). Python deprecation
(Phase 8). New providers beyond what Phases 2 and 3 ship.

**Deliverables.**

- `PauseSimulationEvent`, `ResumeSimulationEvent`,
  `StepSimulationEvent { ticks }` defined and wired in
  `engine::play_mode` against
  [engine::simulation](../eustress/crates/engine/src/simulation/).
- New tool modules under
  [eustress/crates/tools/](../eustress/crates/tools/) covering every entry
  in the *MCP tool registration* table (recording, camera, simulation,
  generative, cutscene groups). Each module defines a `ToolDefinition`
  with name, description, and JSON schema, plus a synchronous handler.
- All tools registered through
  [eustress-tools::register_all_tools](../eustress/crates/tools/src/lib.rs#L63).
- Cutscene runtime: `Cutscene` / `Shot` / `ShotAction` types per
  *Cutscene composition*; TOML loader for `<universe>/Cutscenes/`;
  validation + execution in `cutscene_record`.
- Synchronous handlers per *MCP tool registration* (emit, poll
  completion, return result, timeout error on miss).
- `crates/mcp-server` and `engine_bridge` enumerate the new tools from
  `eustress-tools`. Workshop requires no code change — its dispatch
  already delegates through
  [engine::workshop::tools](../eustress/crates/engine/src/workshop/tools/mod.rs).

**Acceptance.** Drive the worked-example conversation from *Workshop
conversational dispatch* end-to-end in Workshop and produce an MP4 under
`<universe>/SoulService/Recordings/` that visually contains the 5-second
cutaway from the spawned `cutaway` camera. The same tool sequence,
issued from an external MCP client through `crates/mcp-server`, produces
an equivalent MP4. A `cutscene_record` call against a TOML-defined
cutscene under `<universe>/Cutscenes/` executes the shot list and records
a single MP4 with cut markers between shots.

**Dependencies.** Phases 4 (cameras) and 5 (recording). Phases 2 and 3
add the generative providers but are not blockers for the recording /
camera / simulation tool groups.

## Phase 7: Slint generation panel

**Scope.** A new Slint panel that exposes prompt input, provider toggle,
recording controls, current recording status, active camera selection,
and cutscene library. Wired through the existing slint↔bevy adapter.

**Out of scope.** Settings UI for keys (that lives in
`soul_settings.slint`; cross-referenced but not re-implemented here).
Headless builds.

**Deliverables.**

- New Slint component under
  [eustress/crates/engine/ui/slint/](../eustress/crates/engine/ui/slint/).
- Wiring through
  [eustress/crates/engine/src/slint_bevy_adapter.rs](../eustress/crates/engine/src/slint_bevy_adapter.rs)
  to dispatch `GenerateImageRequest` / `GenerateMeshRequest` /
  `GenerateVideoRequest` / `StartRecording` / `StopRecording` /
  `SetActiveCameraEvent` events into Bevy.
- Subscriptions on the response events (`ImageGenerated`,
  `MeshGenerated`, `VideoGenerated`, `GenerationFailed`) drive panel
  state (toasts, job list, preview thumbnail).
- Recording status surface: `Recording` / `Idle` / `Paused` indicator,
  elapsed time, output path of the in-flight MP4 under
  `<universe>/SoulService/Recordings/`.
- Active camera name display plus a dropdown picker enumerating every
  `NamedCamera` entity. Selecting a name emits `SetActiveCameraEvent` with
  a default 0.3 s ease-in-out transition.
- Cutscene library list reading `<universe>/Cutscenes/` plus runtime
  registrations from `cutscene_define`, with a per-row Run button that
  emits the equivalent of `cutscene_record`.
- BYOK key fields added in `soul_settings.slint` next to the existing
  `ANTHROPIC_API_KEY` field; reads/writes go through
  `SoulServiceSettings`-style accessors.
- Engine feature `genworld-panel` so headless builds can skip the UI
  surface.

**Acceptance.** `cargo run --release -p eustress-engine` launches the
engine, the new panel is reachable from the existing UI shell, typing a
prompt and clicking Generate produces a visible image/mesh in the scene
(real or mock depending on key presence), Record/Stop produces an MP4
under `<universe>/SoulService/Recordings/`, the dropdown reflects all
named cameras and switching swaps the render target, and clicking Run on
a listed cutscene produces a recorded MP4 matching the cutscene's shots.

**Dependencies.** Phases 1, 2, 3, 4, 5, 6.

## Phase 8: Python pipeline deprecation

**Scope.** Mark the legacy Python generation pipeline as deprecated so
new contributors see the signal everywhere they might land. No code
deletion. Removal lands in Phase 11 (v2).

**Out of scope.** Deleting `generation_server.py` or
`generation_server_production.py`. Deleting
`client::plugins::enhancement_plugin` or `client::systems::enhancement_*`.
Migrating any in-flight callers.

**Deliverables.**

- `#[deprecated(note = "superseded by genworld; will be removed in v0.2")]`
  on the public surface of `client::plugins::enhancement_plugin` and
  `client::systems::enhancement_*`.
- "Superseded by `docs/generative-world-layer.md`" banner at the top of
  [docs/architecture/ENHANCEMENT_PIPELINE.md](architecture/ENHANCEMENT_PIPELINE.md)
  and
  [docs/architecture/THE_LAST_GAME_ENGINE.md](architecture/THE_LAST_GAME_ENGINE.md).
- Legacy comment header on `eustress/generation_server.py` and
  `eustress/generation_server_production.py` pointing here.
- No code deletion in this PR.

**Acceptance.** `cargo check --workspace` and `cargo clippy --workspace
-- -D warnings` still pass (deprecation warnings are warnings, not
errors, and only fire when the deprecated items are referenced). The
doc banners render in any markdown viewer. The Python files still run
for anyone with the existing setup.

**Dependencies.** Phases 1, 2, 3, 4, 5, 6, 7.

# ────────────────────────────────────────────────────────────────────
# v2 — Production polish
# ────────────────────────────────────────────────────────────────────

v2 turns the v1 loop into something a user runs every day. A persistent
on-disk cache keyed by prompt+params hash means repeated prompts no longer
re-bill the API. An audio provider track lands so cutscenes carry diegetic
sound (and optional synthesized music/SFX). The deprecated Python pipeline
disappears. A timeline UI in Slint makes cutscene authoring direct rather
than text-only. Multiplayer-replicated generated content propagates through
the existing `lightyear` + `bevy_quinnet` layer so co-op and spectator
clients converge on the same generated assets.

## Phase 9: Persistent generation cache

**Goal.** Repeated prompts hit a SHA256-keyed on-disk cache instead of
re-paying the provider API.

**What it adds.**

- `genworld::cache::GenerationCache` resource backed by
  `<universe>/.eustress/genworld_cache/`.
- Cache key = `SHA256(provider_id || request_kind || canonical_json(request))`.
- Cache stores raw provider bytes (PNG/MP4/GLB) plus a small JSON sidecar
  with prompt, params, provider id, generation timestamp.
- `dispatch_generation_requests` checks the cache before spawning a
  provider call; cache hits emit the matching `*Generated` event
  synchronously on the next Update tick with the cached bytes.
- Cache invalidation: any change to prompt, model id, or params produces a
  fresh key. Old entries stay on disk; an `eustress_cache_gc` tool prunes
  entries older than a configurable threshold.
- `GenWorldSettings.cache_enabled` (default `true`) and `cache_root` path
  override for users who want the cache outside the Universe (shared cache
  across projects).

**Depends on.** Phase 1 (provider plumbing), Phase 2 (real responses to
cache), Phase 3 (mesh responses to cache).

**Risks / open questions.** Cache thrash on prompt iteration during
authoring sessions disk-pressures slow drives; consider a per-Universe LRU
cap as a follow-up if it becomes a real problem.

## Phase 10: Audio synthesis + cutscene audio capture

**Goal.** Cutscenes carry audio. Generated music or SFX can be synthesized
alongside the existing image/video/mesh providers.

**What it adds.**

- `AudioProvider` trait in `genworld::provider` alongside `ContentProvider`
  and `MeshProvider`. Shape mirrors `ContentProvider`: an async
  `generate_audio(req: AudioRequest) -> AudioResponse` returning encoded
  audio bytes plus metadata.
- First impl uses whichever audio gen API Google ships at the time, or a
  third-party provider if Google has nothing usable; concrete choice
  decided at impl time per the "do not invent" discipline.
- `RecordingPlugin` extension: capture the simulation's audio mix (Bevy
  audio output bus) alongside the video readback. ffmpeg-sidecar accepts
  the audio stream as a second input; the encoder muxes both into one MP4.
- New MCP tools: `gen_audio`, `record_audio_status`. Cutscene `ShotAction`
  gains `PlayAudio(handle)` and `GenAudioOverlay(prompt)` variants.
- The audio surface honors the same Phase 9 cache.

**Depends on.** Phase 5 (recording pipeline), Phase 9 (cache).

**Risks / open questions.** Provider availability for music/SFX gen — if
no usable provider exists at impl time, scope reduces to diegetic-only
capture and the synthesis surface ships as a trait with `MockProvider`
returning silence.

## Phase 11: Python pipeline removal

**Goal.** The deprecated Python pipeline is gone, the docs no longer
reference it as a live path.

**What it adds.**

- Deletion of
  [eustress/crates/engine/src/client/plugins/enhancement_plugin/](../eustress/crates/engine/src/client/plugins/enhancement_plugin/)
  (or whatever the `client::plugins::enhancement_plugin` path resolves to
  at this point).
- Deletion of `client::systems::enhancement_*` modules.
- Deletion of `eustress/generation_server.py` and
  `eustress/generation_server_production.py`.
- Deletion or full rewrite of
  [docs/architecture/ENHANCEMENT_PIPELINE.md](architecture/ENHANCEMENT_PIPELINE.md)
  and
  [docs/architecture/THE_LAST_GAME_ENGINE.md](architecture/THE_LAST_GAME_ENGINE.md)
  — superseded by this doc; replace with a one-line redirect or remove
  outright.
- Workspace dependency cleanup: any crate dep introduced solely for the
  Python bridge gets pruned.
- A migration note in the PR description for anyone with local automation
  pointing at the Python entry points.

**Depends on.** Phase 8 (deprecation pass), Phases 9 and 10 (the
production-equivalent capabilities — cache and audio — must be in before
deletion so no functional regression lands).

**Risks / open questions.** Out-of-tree callers — any third-party tooling
the team has personally wired up needs a heads-up; the deprecation pass
in Phase 8 is supposed to give that window.

## Phase 12: Cutscene timeline UI

**Goal.** Cutscene authoring is a direct visual surface, not a text-only
TOML edit.

**What it adds.**

- New Slint timeline panel: horizontal track of shots, each shot a
  draggable block with start/duration/camera/transition fields.
- Click a shot to expand its `ShotAction` list inline; add/remove/reorder
  actions with drag.
- Preview button per shot: emits the camera switch and shot duration
  without recording, so authors can scrub shot framing before committing
  to an MP4.
- Save / Save As / Load against `<universe>/Cutscenes/*.toml`. Round-trips
  the same format Phase 6 ships.
- Marker overlay on the timeline showing the last recorded MP4's cut
  markers (when one exists) so re-runs converge toward the intended pace.
- The panel publishes the same `cutscene_record` event the MCP tool emits,
  so panel-triggered recordings produce identical artifacts.

**Depends on.** Phase 6 (cutscene runtime + TOML loader), Phase 7 (Slint
panel infrastructure), Phase 5 (recording pipeline for preview vs commit).

**Risks / open questions.** Slint's timeline-control ergonomics — if the
drag/snap behavior is awkward to hand-roll, the fallback is a simpler
table-with-buttons surface that still beats TOML editing.

## Phase 13: Multiplayer-replicated generated content

**Goal.** Generated meshes, images, and cutscene recordings propagate to
all clients in a `lightyear` session so co-op and spectator clients see
the same assets without re-requesting them.

**What it adds.**

- A `GeneratedAsset` component carrying the cache-key hash from Phase 9
  attached to spawned entities (textured planes, mesh instances, cutscene
  playback entities).
- Replication channel registration in `lightyear` 0.19 / `bevy_quinnet`:
  `GeneratedAsset` replicates by hash; receiving clients look up the
  hash in their local cache and, on miss, request the bytes from the
  authority over a dedicated reliable channel.
- An authority-side `serve_generated_bytes` system streams the cached
  payload on demand. Clients write the bytes into their own cache and
  resolve the asset locally.
- For cutscene recordings (MP4), the same hash-replicate flow applies;
  clients can opt out of MP4 propagation through a settings flag for
  bandwidth-sensitive sessions.
- Tools surface: `gen_provider_status` extends to report cache hit/miss
  rates per peer in a session.

**Depends on.** Phase 9 (cache + hash key), Phase 6 (tool surface for
status reporting). Underlying `lightyear` / `bevy_quinnet` integration
already in the engine.

**Risks / open questions.** Bandwidth — a session generating heavy mesh
assets every minute saturates a typical broadband uplink; the opt-out
flag is the v2 mitigation. Cache poisoning across peers (authority sends
bogus bytes) is mitigated by recomputing the hash on receive.

# ────────────────────────────────────────────────────────────────────
# v3 — Local inference and hybrid routing
# ────────────────────────────────────────────────────────────────────

v3 takes the layer off the cloud dependency. A `LocalProvider` runs models
on the user's machine through candle or burn. A `HybridProvider` composes
local and cloud providers behind a per-request routing policy so the user
gets cheap/fast/private when they want it and high-fidelity cloud when
they need it. Mesh generation gains multi-stage refinement passes. A
plugin manifest format lets third-party crates ship providers, replacing
compile-time selection with run-time discovery.

## Phase 14: Local model inference adapter

**Goal.** A `ContentProvider` implementation that runs models on the
user's machine, resurrecting the ambition of the deprecated Python
pipeline behind the same `ContentProvider` trait.

**What it adds.**

- `genworld::local::LocalProvider` with candle or burn as the runtime;
  framework choice decided at impl time against the state of each
  ecosystem (model zoo coverage, GPU backend support, ergonomics).
- First-target capabilities: image generation (FLUX-class model) and mesh
  generation (TripoSR-class model). Video generation is deferred unless a
  consumer-runnable video model is available at the time.
- Model loading: weights cached under
  `<universe>/.eustress/genworld_models/` with a manifest naming the model
  family, quantization, and source URL. First-run downloads on prompt;
  subsequent runs are local-only.
- Device selection: CUDA, Metal, Vulkan, CPU. Auto-detected with override
  via `GenWorldSettings.local_device`.
- `LocalProvider` honors the Phase 9 cache the same way cloud providers
  do — local responses go in, repeated prompts skip the model.
- No new MCP tool surface; `gen_image` / `gen_mesh` route to whichever
  provider the routing policy (Phase 15) picks.

**Depends on.** Phase 1 (provider trait), Phase 9 (cache).

**Risks / open questions.** Model weight licensing — open-weights need a
license-check pass so the layer doesn't ship a model the user can't
legally run. First-run download size — multi-GB weights are a poor
first-launch experience; lazy download on first prompt is the chosen
trade-off.

## Phase 15: Hybrid routing

**Goal.** Per-request routing across local, cloud, and third-party
providers driven by a user-editable policy.

**What it adds.**

- `genworld::hybrid::HybridProvider` wrapping any combination of
  `GeminiProvider`, `LocalProvider`, and third-party providers (Phase 17).
- Routing policy file at `<universe>/genworld_policy.toml`. Policy fields:
  - Cost ceiling per request (in user-configured currency).
  - Latency target (millis).
  - Quality tier (`draft` | `standard` | `final`).
  - Privacy flag (when set, never routes to a cloud provider).
  - Per-tool overrides (`gen_image` can have a different policy than
    `gen_video`).
- Hot-reloadable: a file watcher reloads the policy without restarting
  the engine.
- Routing decision logs in `engine::notifications` at trace level so
  users can see which provider served each request.
- `ProviderRegistry` exposes routing inspection through
  `gen_provider_status` for both human readers and automated MCP
  consumers.

**Depends on.** Phases 2, 14 (at least two providers to route between),
Phase 9 (cache lookup precedes routing).

**Risks / open questions.** Policy expressiveness vs. complexity — the
TOML schema needs to stay readable. The current decision is a small set
of well-named fields with sensible defaults rather than a full
expression language.

## Phase 16: Quality refinement passes

**Goal.** Mesh generation produces higher-quality output through a
multi-stage pipeline rather than a single one-shot call.

**What it adds.**

- `MeshRefinementPipeline` resource composing a sequence of
  `MeshProvider` calls:
  1. Rough mesh — initial geometry from prompt.
  2. Texture pass — UV unwrap + texture synthesis (often a separate
     `ContentProvider::generate_image` call against the unwrapped UV).
  3. Retopology pass — clean topology from the rough mesh.
  4. LOD generation — multiple decimation levels for runtime use.
- Each stage is independently swappable; the pipeline composes whatever
  stages have providers registered.
- Image generation gains an optional upscale pass (separate
  `ContentProvider` call against a model trained for super-resolution).
- Stages can run on different providers — for example rough mesh from a
  cloud provider, retopology from a local provider, texturing from
  whichever the routing policy elects.
- Cache (Phase 9) keys against the full pipeline manifest so cache hits
  short-circuit the whole pipeline, not just the first stage.

**Depends on.** Phase 3 (mesh provider), Phase 14 (a second mesh-capable
provider for cross-provider composition), Phase 15 (routing).

**Risks / open questions.** Cumulative latency — a four-stage pipeline
adds up quickly; per-stage timeouts and partial-result returns are the
mitigation.

## Phase 17: Third-party provider plugins

**Goal.** Third-party crates can ship `ContentProvider` /
`MeshProvider` / `AudioProvider` / `NeuralSimulator` impls and have the
engine discover them at run time.

**What it adds.**

- Plugin manifest format (`genworld_plugin.toml`) declaring:
  - Plugin name, version, author, license.
  - Provider impls offered (kinds + capabilities).
  - Required env vars / secrets (so the BYOK UI knows what to ask).
  - Load order (priority hint for the registry).
- A plugin discovery system that scans
  `<install>/plugins/genworld/*/genworld_plugin.toml` and
  `<universe>/.eustress/genworld_plugins/*/genworld_plugin.toml`.
- Plugins ship as dynamically loaded crates (likely `libloading`-backed)
  exposing a C-ABI registration entry point that registers their
  providers into `ProviderRegistry`.
- Key isolation: a plugin's secrets live in its own slot in the BYOK
  store and are never visible to other plugins.
- Capability discovery: the registry exposes a query API
  (`ProviderRegistry::capabilities_for(kind)`) that the routing policy
  (Phase 15) consults.
- Compile-time provider selection still works as the fallback; plugins
  are additive.

**Depends on.** Phase 15 (routing — without it, plugin providers would
have no way to be selected), Phases 14 / 10 (multiple provider kinds to
make the registry surface meaningful).

**Risks / open questions.** Plugin ABI stability across Rust versions —
dynamic loading of Rust code is notoriously fragile. The mitigation is
to define the plugin ABI through a stable C surface (function pointers,
opaque types) even though both sides are written in Rust.

# ────────────────────────────────────────────────────────────────────
# v4 — Predictive and generative world systems
# ────────────────────────────────────────────────────────────────────

v4 takes the layer past content generation into world-state generation.
The `NeuralSimulator` trait scaffolded in v1 gets its first concrete
impl — this is the path to generative world state, not Veo. Veo's 2D
output remains content for cutscenes, skyboxes, and looped textures;
`NeuralSimulator` is where predicted world frames live. Predictive
replay extends recordings past the end of the playthrough through neural
prediction. Training data export lets users fine-tune external world
models on their own scenes. NPC behavior in the simulation gets sampled
from a model rather than scripted, with the soul/Rune surface still in
place underneath.

## Phase 18: First `NeuralSimulator` impl

**Goal.** The trait scaffolded in v1 gets a concrete implementation.

**What it adds.**

- A real `NeuralSimulator` impl. Concrete vendor decided at impl time
  against whichever world-model API is callable then. No public Google
  API exposes Project Genie as a callable service at the v1/v2/v3 time
  horizon; this is the phase where that may change. Other candidates at
  the time of writing: Meta V-JEPA, World Labs, Decart.
- The impl conforms to the `NeuralSimulator::step` contract scaffolded
  in v1; the trait shape lets the concrete impl swap without disturbing
  callers.
- An engine-side `engine::viga` integration that uses the impl as one
  hypothesis source in its iterate-and-verify loop.
- A `gen_neural_step` MCP tool surfacing the trait method directly for
  programmatic callers.

**Depends on.** Phase 1 (trait scaffolding), Phase 15 (routing — local
vs. cloud `NeuralSimulator` providers benefit from the same policy
surface).

**Risks / open questions.** API availability — this is the phase most
exposed to "no provider exists yet." The fallback is a `LocalProvider`
impl backed by V-JEPA-class research weights when callable APIs lag.

## Phase 19: Predictive replay

**Goal.** A recorded MP4 can be extended past the actual playthrough
through neural prediction.

**What it adds.**

- A `predict_replay` MCP tool taking an existing recording handle plus a
  desired extension duration plus a final-action descriptor.
- The tool feeds the recording's tail frames + the final action into
  `NeuralSimulator::step` in a loop, decoding each predicted frame and
  appending it to the MP4 through the same encoder used for live
  recording.
- Predicted frames are tagged in a sidecar JSON
  (`<recording>.predicted.json`) so the timeline UI (Phase 12) can show
  where the real footage ends and the prediction begins.
- An "extend" affordance in the timeline UI calls `predict_replay`
  directly with the timeline's currently selected recording.
- Cutscenes (Phase 6) gain a `PredictiveShot` variant on `Shot` that
  generates its frames through the neural simulator instead of running
  the engine simulation.

**Depends on.** Phase 18 (`NeuralSimulator` impl), Phase 12 (timeline UI
to surface the affordance), Phase 5 (recording encoder reuse).

**Risks / open questions.** Frame coherence at the predicted/real
boundary — a hard cut between real and predicted footage looks bad; a
short crossfade is the planned mitigation.

## Phase 20: Training data export

**Goal.** The engine produces labeled training data from its own
playthroughs in a format usable by external model trainers.

**What it adds.**

- A `.eustress-trace` bundle format: zstd-compressed CBOR (or similar
  efficient binary container — final choice at impl time) carrying per-
  frame:
  - The rendered image.
  - The active camera pose.
  - All entity transform deltas since the previous frame.
  - All player inputs since the previous frame.
  - The active `PlayModeState` and sim-tick number.
- New MCP tool `export_training_data` taking a recording handle and
  emitting a `.eustress-trace` bundle alongside the MP4.
- A `RecordingConfig.export_training_data` flag (default `false`) that
  emits the bundle automatically alongside every recording.
- A reference decoder crate (`eustress-trace-decoder`) so external
  trainers can consume the format without depending on the engine.
- Documentation of the format under
  [docs/](.) so third-party training pipelines have a stable target.

**Depends on.** Phase 5 (recording infrastructure to attach the export
to), Phase 17 (the plugin system is the entry point for external
trainers that consume the export).

**Risks / open questions.** Bundle size — full per-frame state at 60 fps
is heavy; the format's compression has to earn its keep. A coarser
keyframe-plus-delta layout is the backup if the naive shape is too big.

## Phase 21: Generative NPC behavior

**Goal.** NPC actions in the simulation are sampled from a language or
multi-modal model at each decision beat; the consequences play through
the existing physics/animation stack and record as standard MP4.

**What it adds.**

- A `GenerativeNPCBehavior` component carrying a model selector (Gemini
  / local / third-party) and a decision-interval setting.
- A per-beat dispatch system that gathers the NPC's local context
  (visible entities, recent player actions, NPC's own state) and asks
  the chosen model for the next action.
- Action format: a typed enum that maps to existing engine actions
  (move, animate, speak, interact) so the NPC's decisions plug into the
  existing animation and physics surfaces without new primitives.
- Integration with the existing soul/Rune scripting surface: a script
  can compose with or override the generative behavior on a per-NPC
  basis.
- Recording (Phase 5) captures the result the same way it captures any
  other gameplay; no special path for generative NPCs.
- Determinism toggle: a `seed` field on `GenerativeNPCBehavior` makes
  decisions reproducible for cutscene re-records.

**Depends on.** Phase 18 (the neural-simulator-shaped infrastructure
generalizes to behavior sampling), Phase 15 (routing decides which model
gets each decision), the existing soul/Rune surface in the engine.

**Risks / open questions.** Latency at the decision beat — model calls
on a per-NPC-per-second basis pressure cost and frame rate; the
mitigation is a routing policy that prefers local providers for
behavior beats and reserves cloud providers for narrative-critical
moments.

## Endgame — What the layer enables

Once all four versions are in, the layer supports the following
capabilities. These are destinations, not phases.

**Continuous worldgen as the camera explores.** As the active camera
moves into unseen territory, `NeuralSimulator` plus a mesh provider
extend the world ahead of the camera in real time, Genie-style. The
recording pipeline captures this seamlessly because the recording
pipeline doesn't distinguish authored content from generated content —
both are rendered through the same Bevy renderer and read back through
the same encoder. Built on Phases 18 (neural simulator), 14 (local
inference for the latency budget), 13 (multiplayer replication so
co-op clients see the same generated territory), and the v1 recording
infrastructure.

**Workshop multimodal observation.** Claude in Workshop receives the
in-flight recording (or finished MP4s) as multimodal context and
proposes camera moves, cuts, and shot changes based on what it sees,
not just what it was told. The Workshop tool surface gains
"observe_recording" verbs that hand the model the current frame and
the recent frame window. Built on the existing `engine::viga`
vision-as-inverse-graphics loop, extended through Phase 12 (timeline
UI surfaces the model's suggestions) and Phase 5 (recording is what
Claude observes).

**User-trained models on the user's own footage.** The
`.eustress-trace` exports from Phase 20 feed into a local training
pipeline. Users can fine-tune a generation or simulation model on
their own scenes and load it through the Phase 17 plugin system. The
training pipeline itself is third-party; the layer supplies the data,
the format, and the plugin load path. This is the loop that justifies
the training data export as more than an artifact.

**Cross-session authoring memory.** The engine learns the user's
authoring style — preferred camera framings, cut pacing, prompt
vocabulary — and pre-generates suggestions in Workshop
conversations. Built on the existing `embedvec` and memory
infrastructure, with `genworld` contributing the prompt-and-response
history as training signal. The user-visible affordance is Workshop
proposing prompts and camera moves that match the user's prior
choices on similar scenes.

**Self-improving cutscene library.** Cutscenes that get re-recorded
or edited iterate against a generated quality signal (Workshop's
evaluations, user feedback through the timeline UI). Frequently used
scenes converge toward higher production value over time without
per-shot direction. Built on Phase 12 (the timeline UI is where the
quality signal is captured), Phase 6 (the cutscene runtime is what
re-records the iterations), and the cross-session authoring memory
above (the model learns from each iteration).

## Milestone verification

### v1 done

The user does the following and the layer works end-to-end:

1. Set or do not set `GEMINI_API_KEY` and `MESH_PROVIDER_API_KEY` in the
   environment (or in `soul_settings.slint`).
2. Run `cargo run --release -p eustress-engine`.
3. Open the new Slint generation panel.
4. Type a prompt. Click Generate.
   - With keys set: a real Imagen image appears as a textured plane
     (texture at `<universe>/assets/images/<timestamp>.png`), or a real
     mesh from the chosen vendor spawns into the scene (GLB at
     `<universe>/assets/meshes/<timestamp>.glb`).
   - Without keys set: a `MockProvider` image or cube GLB appears
     instead, with a tooltip pointing to the missing key.
5. Press F5 to enter Play mode. Recording auto-starts and the panel shows
   `Recording`. Fly through the scene. Press F6 to pause; the panel shows
   `Paused` and the in-flight MP4 timestamp stops advancing. Press F6 to
   resume; the MP4 continues. Press F8 to return to Editor; the MP4 is
   finalized under `<universe>/SoulService/Recordings/<timestamp>.mp4`
   and plays back in a standard player.
6. From the panel's camera dropdown, switch from `editor` to a second
   named camera spawned via Workshop or a script; the render target
   swaps with the configured transition.
7. In Workshop, issue the conversation from *Workshop conversational
   dispatch*. Claude executes the eight-tool sequence, each step shown
   in the Workshop chat. The resulting MP4 contains the 5-second cutaway
   shot.
8. Define a cutscene in `<universe>/Cutscenes/<name>.toml`. Run it from
   the panel's cutscene library. The recorded MP4 walks the configured
   shots and contains a cut marker between each.
9. Switch the active camera mid-recording through `camera_set_active` or
   the panel dropdown. The resulting MP4 contains no visible artifact at
   the cut: the frames before the cut are from the previous camera, the
   frames after are from the new camera, and the timestamps are
   continuous.

`cargo check --workspace` and `cargo clippy --workspace -- -D warnings`
pass locally. `cargo test -p genworld` passes without a live API key.

### v2 done

Repeated prompts skip the provider API and resolve from
`<universe>/.eustress/genworld_cache/` instantly. Cutscene MP4s carry
audio — both the simulation's diegetic mix and any synthesized music or
SFX placed on shots. The Python pipeline (`generation_server*.py`,
`client::plugins::enhancement_plugin`, `client::systems::enhancement_*`)
is gone from the tree. The Slint timeline UI lets a user assemble,
preview, and save a cutscene without ever opening the TOML. Two clients
in a `lightyear` session generate a mesh on one side and see the same
mesh on the other side without re-requesting it.

### v3 done

The engine generates an image or mesh with no network connection through
`LocalProvider`. The `genworld_policy.toml` at the Universe root routes
the same `gen_image` call to different providers based on cost, latency,
and privacy. A four-stage mesh refinement pipeline produces a textured,
retopologized mesh with LODs from a single prompt. A third-party plugin
crate dropped into `<install>/plugins/genworld/` is discovered at
startup, its providers register, and they take part in routing
decisions.

### v4 done

A `NeuralSimulator` impl is registered and `gen_neural_step` returns a
predicted frame from a window of past frames. A recording finalized in
Play mode can be extended past its real footage through `predict_replay`
and the timeline UI shows where the real frames end. Every recording
optionally emits a `.eustress-trace` bundle alongside the MP4. NPCs
tagged with `GenerativeNPCBehavior` take model-sampled actions in the
simulation, recorded as standard MP4, with a `seed` flag making the same
playthrough reproducible.

### Endgame

The active camera moves into unseen territory and the world extends to
meet it in real time, recorded seamlessly. Claude in Workshop watches
the recording in flight and proposes the next shot based on what it
sees. A user fine-tunes a model on their own `.eustress-trace` exports
and loads it through the plugin system. Workshop pre-suggests prompts
and camera moves that match the user's authoring style. Cutscenes
re-recorded against a quality signal converge toward higher production
value without per-shot direction. None of this is a phase; it's what
the layer is for.
