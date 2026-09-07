# Eustress Player: Public-Launch Readiness & the Agent Control Surface

**Scope:** the standalone Client (`eustress-client`, "Eustress Player"), the avatar, and what it
would take to put either in front of strangers. Plus the agent control surface that makes
iterating on it tractable.

**Evidence base:** a 7-dimension audit producing 49 findings — 34 classified launch-blocking,
15 quality-blocking — of which 37 survived adversarial verification. Findings not verified are
marked. Everything cites `file:line`.

---

## 1. Where the Client actually is

**It cannot launch publicly, and it is not close.** The gap is not polish. Three independent
facts each make a public launch impossible on their own:

1. **The Player is never built.** All three release jobs build `--package eustress-engine`
   only (`.github/workflows/release.yml:67, :143, :226`). The Inno Setup script packages
   `eustress-engine.exe` and nothing else (`installer/windows/eustress-engine.iss:58`).
   `grep -rn "eustress-client" .github/ installer/` returns nothing. **The release pipeline has
   also never succeeded — 7 of 7 runs failed and zero GitHub Releases exist.**

2. **The Player could not load content if it existed.** `client/src/systems/scene_loader.rs` is
   entirely `#[deprecated]` legacy RON/JSON off a local CLI argument. There is no HTTP fetch, no
   `.pak` extraction, no zstd decode, no binary-scene deserializer. Studio's publish path
   (`engine/src/ui/file_event_handler.rs:701`) uploads to a bucket **nothing reads**.

3. **The Player cannot render UI.** `crates/client/Cargo.toml` omits `bevy_ui`, `bevy_ui_render`
   and `bevy_text`, and the crate has no Slint dependency. No menu, no HUD, no error message, no
   quit dialog can ever be drawn. The pause menu is a no-op shell.

### Confirmed blockers by theme

| Theme | Blocker | User impact | Effort |
|---|---|---|---|
| **Distribution** | No CI job builds the Player; installer is Studio-only | Every download button 404s | L |
| | Release pipeline 7/7 failed; zero Releases published | Nothing to download even for Studio | M |
| | `downloads.eustress.dev` is NXDOMAIN; updater manifest URL dead | Shipped copies can never update | S |
| | Nothing is code-signed | **SAC already blocks the binary on the dev machine** — public Windows users hit the same wall | M |
| | Asset root is a dev-checkout relative path; CI never copies `common/assets` | An installed build finds no character, no textures | S |
| **Content** | No download/unpack/load path in the Player | Nothing published can ever be played | XL |
| | Worker `verifyAuth` returns a string; handlers read `auth.userId` | **Every `.pak` upload 403s** — no simulation is ever stored | S |
| | `is_public` never written on publish; `handleDownloadPak` treats all as private | Listed publicly, then denied on download | S |
| | `.pak` is not self-contained — primitive meshes and character assets live outside it | A published Space is incomplete on any other machine | L |
| | `eustress-server` extracts the `.pak` then discards it and runs an empty world | Server mode serves nothing | L |
| **Identity** | Studio "Log in with identity" sets the auth token to the user's **public key** | No authentication is occurring | S |
| | `DEV_MODE = true` hardcoded in the shipping Studio binary; Steam login is a local fake | Auth is theatre | S |
| | Player has no identity of any kind | A published sim can never know who is playing | L |
| **Safety / legal** | Zero content moderation, while shipped docs tell creators it exists | Arbitrary UGC reaches players unscreened | L |
| | No takedown path implemented | **DMCA safe harbour unavailable** | M |
| | No reporting or blocking anywhere in the product | No recourse for abuse | M |
| | Downloaded scripts get unrestricted outbound HTTP from the player's machine | Arbitrary UGC code exfiltrates from user machines | M |
| | Capability kernel is advisory-only, loads a permissive law set, does not cover Luau | The sandbox does not sandbox | L |
| | No age gate, while Terms and Privacy both assert one | Stated policy is false | M |
| **Stability** | Zero panic containment in the Client | Any panic is a hard crash with no message | S |
| | No logs, console, symbols or crash reports survive a shipped run | A user crash is undiagnosable | M |
| | ESC twice captures and hides the cursor with no menu and no quit | **User is trapped in the window** | M |

The safety cluster deserves emphasis. Publishing arbitrary user code that is downloaded and
**executed on a stranger's machine**, with an advisory-only capability kernel, unrestricted
outbound HTTP, no moderation, no takedown, and no reporting — while the docs and Terms claim
otherwise — is the highest-liability item in this document. It is a larger risk than every
rendering and animation issue combined.

---

## 2. Where the character actually is

### What genuinely works now (verified running, this session)

- Real Avian physics body — kinematic `MoveAndSlide` controller with sphere-cast grounding,
  coyote time, jump buffer, slope limit, step-up. Previously there was **no `RigidBody` and no
  `Collider` at all** on either side; the insert was commented out.
- Clip retargeting onto a canonical bone space: **195 curves onto 65 bones, 0 dropped**, all four
  clips. Mixamo body/clip name suffixes differ, which had bound only 7 of 65.
- `ground=1.000 / air=0.000` after fixing a ground probe that started flush with the capsule
  bottom — a shape cast beginning in contact returns no hit, so `grounded` was false permanently.
- Feet grounded by measurement, not arithmetic. Camera, input, and facing shared by both shells
  through one sealed runtime. 30/30 tests pass.

### What a public user would see today

**A frozen mannequin.** `male_idle.glb` contains **1.78° of total motion** — measured from the
source accessors, not inferred. Standing still is indistinguishable from a static pose. Walking
works (55° of authored motion) but:

- **The camera has no occlusion handling.** It clips through walls; first-person puts the camera
  inside the character's own skull.
- **Foot IK is disabled**, so feet float and sink on anything not flat.
- **`rig_scale` is computed and never applied** — the Height slider resizes the collider and the
  camera but not the visible body.
- **Look requires holding right-mouse.** In a standalone player that is the primary control.
- **Crouch is a dead keybind.**
- Two untextured robot mannequins, zero customization wired.

### Written but deliberately disabled

`ik.rs` and `procedural.rs` are complete and unit-tested but off. Both apply **world-space
rotation deltas to local bone rotations**, and the pelvis correction writes a metres-scale offset
into 0.01-scale local space. Enabling them reproduces the arms-flung-out, character-vanishes
behaviour observed live. The solver math is sound; the *application* needs a world→local
conversion through each bone's parent rotation.

---

## 3. Minimum viable public launch

Cut aggressively. The smallest coherent thing that is not embarrassing:

**In scope**
1. Code signing (unblocks SAC for both dev and users).
2. A Player build + installer that ships `common/assets`, on a release pipeline that has
   actually succeeded once.
3. `bevy_ui` linked, plus: main menu, pause menu, quit, and a visible error surface.
4. Panic containment + a log file the user can send.
5. Download → unpack → play one published simulation, end to end.
6. Fix the two one-line worker bugs (`auth.userId`, `is_public`) that make publish/download 403.
7. Moderation gate + takedown + report path. Script sandboxing: no outbound HTTP by default.
8. Avatar: enable the procedural life layer (fixes the frozen idle), camera occlusion, single-key
   look, apply `rig_scale`.

**Explicitly out**: multiplayer (there is no wire protocol anywhere — `eustress-networking` has
zero socket/QUIC symbols), avatar customization UI, marketplace, mobile, VR, foot IK.

**Remove from the website until real**: the App Store and Play Store badges (no such apps exist;
one uses a placeholder id `id123456789`), and the Player download buttons on `/simulation/:id`.
Shipping dead links is worse than shipping no page.

---

## 4. The agent control surface

### Why this matters more than it looks

This session, four bugs were invisible to log-based instrumentation: the missing
`gltf_animation` feature, the missing `AnimatedBy` link, a ground probe starting in contact, and
a half-size ground collider. Every one lived in the seam between engine code and Bevy/Avian, and
every one was found by a human looking at the screen. My logs were green throughout, because they
measured only the code I had written.

A frame-capture tool added late found the ground bug within one run of existing.

### Design

**Transport.** Reuse the existing bridge protocol shape (`engine/src/engine_bridge/`) — TCP
JSON-RPC on a loopback port, opt-in via `EUSTRESS_AGENT_PORT`. One MCP server then drives either
shell, which is itself a parity win: Studio Play Mode and the Client become equally drivable.

**Input injection.** Bevy's `ButtonInput<KeyCode>` is a `Resource` with `press()`/`release()`.
A synthetic-input layer writes it directly in a system ordered **before**
`AvatarSystems::Input`, gated on an `AgentControl` resource so real input is untouched when the
agent is not driving. Held-key state persists across frames; taps auto-release after N frames.

**Observation payload** — the part that makes sparse feedback tractable:

```
position, velocity, yaw            — did it move, and where
grounded, planar_speed, air_time   — is the controller in the state you think
clip weights + elapsed             — is the intended animation actually playing
peak bone delta over window        — is the POSE changing (catches "plays but frozen")
rig bones bound / unresolved       — is the skeleton healthy
warn+error lines since last step   — what the engine complained about
frame PNG path                     — what it looks like
```

Each of those corresponds to a bug class this session actually hit.

**Tools**

| Tool | Signature |
|---|---|
| `client_step` | `(actions[], frames) -> Observation` |
| `client_hold` / `client_release` | `(keys[])` |
| `client_observe` | `(capture: bool) -> Observation` |
| `client_reset` | `(descriptor?, spawn?) -> Observation` |
| `client_capture` | `(count, every) -> [png_path]` |

### On ARC-AGI-3 — what transfers and what does not

**Transfers:** cheap episodes; reset-to-known-state; determinism and replay (the repo already
pins a fixed timestep and has `crates/common/tests/determinism.rs`), so a regression can be
bisected by replaying one input tape; and **dense derived observations standing in for an
explicit reward**. That last idea is the load-bearing one — "did the pose change, is it grounded,
did anything error" is exactly the signal that distinguishes progress from stasis without anyone
authoring a reward function.

**Does not transfer:** there is no reward signal here and no scoring environment. This is a
debugging loop, not an RL problem, and calling it one would be dressing up the framing. The value
is closing act→observe, not learning a policy.

---

## 5. Roadmap

Each phase has a gate that can fail.

**P0 — Unblock the loop (S).** Code signing; agent control surface; verify the Client runs under
SAC.
*Gate:* an agent presses W for 60 frames and observes `planar_speed > 1.0` with a frame showing
the character mid-stride.

**P1 — Ship a binary (M).** Player build job; installer bundling `common/assets`; exe-relative
asset root; one green release.
*Gate:* download the installer on a clean machine, launch, see the character standing on the
baseplate.

**P2 — Survivability (M).** `bevy_ui`; main menu, pause, quit, error surface; panic containment;
log file.
*Gate:* force a panic — the user sees a message and can quit; the log lands on disk.

**P3 — Content loop (XL).** Fix `auth.userId` and `is_public`; extract the binary scene parser to
`common`; download/unpack/spawn; make `.pak` self-contained including character assets.
*Gate:* publish from Studio on one machine, play it in the Player on another.

**P4 — Safety (L).** Moderation gate, takedown, reporting, age gate; capability kernel made
enforcing and extended to Luau; outbound HTTP denied by default.
*Gate:* a published sim attempting an outbound request is blocked and surfaced; a takedown removes
it from listing and download.

**P5 — Character quality (L).** Fix the world→local conversion, enable the procedural life layer
and foot IK; camera occlusion; apply `rig_scale`; single-key look; crouch.
*Gate:* standing still shows visible breathing and idle breaks; the camera never enters geometry.

---

## Uncertainties

- The GPU-clustering panic (`bevy_pbr cluster/gpu.rs`, `Buffer::get_mapped_range` validation
  error) appeared once when animation started playing and has not reproduced. Cause unknown.
- Studio parity is **unverified**. Its binary was locked by a running process for this entire
  session, so every avatar result here is Client-only. The parity contract asserts both shells
  build identical avatars, but that is a structural argument, not an observation.
- 12 of the 49 findings were not adversarially verified before the audit hit its budget; they are
  the lower-severity tail and are marked as such in the source data.
