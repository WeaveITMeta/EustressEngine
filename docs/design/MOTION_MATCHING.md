# Motion matching

Status: designed 2026-09-30. Phase 1 (the pure core) is being written; nothing is built or run yet. Companion to `ANIMATION_SYSTEM.md`, which owns the track model, the `Animator` runtime and the `Animate` script this builds on.

## What it is

A character's locomotion pose comes from a database of clip frames. Several times a second the matcher compares the character's current pose, its velocity and the path it is about to take against every frame in the database and plays from the frame that fits best. Switches between clips are hidden by inertialization, not crossfades. Feet stay planted through contact data, and parkour moves are clips whose root path is warped to land on the ledge the climb system found.

It replaces the gait part of the default `Animate` script (the weights by ground speed). Everything else keeps its place:

- **Action tracks** (a wave, an attack, an emote) stay ordinary `AnimationTrack`s at a higher priority than the matcher, so they layer over the gait as they do today.
- **The body** stays with the kinematic controller. The matcher reads the controller's velocity and facing; it never moves the capsule.
- **Procedural layers** (breathing, landing flex, foot IK, facing) run on the matcher's output, in the order below.

## Classes

Both are classes with properties, like the rest of the animation system. A Space switches the matcher on, swaps the database or tunes the weights without touching Rust.

**`MotionDatabase`** holds `Animation` children, one per clip.

| Where | Name | Meaning |
|---|---|---|
| Child `Animation` attribute | `Speed` | Root travel speed in m/s for an in-place clip. Mixamo clips use their authored speeds (walk 1.45, run 3.9). |
| | `Heading` | Direction of travel against the facing, degrees: 0 forward, 90 right, 180 backward. |
| | `YawRate` | Turning rate of the root, degrees per second. |
| | `Tags` | Comma list (`locomotion`, `crouch`, `vault`). A matcher can require or exclude tags. |
| | `Loop` | Whether the clip wraps. |
| Property | `SampleRate` | Frames per second the database is built at. Default 30. |
| | `TrajectoryTimes` | Seconds ahead the path is sampled at. Default `0.2,0.4,0.6,1.0`. |
| | `TrajectoryWeight`, `FacingWeight`, `PoseWeight`, `VelocityWeight` | How much each feature group counts in the cost. |
| Read only | `ClipCount`, `FrameCount` | Size of the built database. |

**`MotionMatcher`** is a child of a `Humanoid` (or `AnimationController`).

| Name | Meaning |
|---|---|
| `Database` | The `MotionDatabase` it searches. |
| `Enabled` | Off by default. Off means the `Animate` script's gait plays, as today. |
| `SearchInterval` | Seconds between searches. Default 0.1. |
| `BlendTime` | Seconds an inertialized switch takes to settle. Default 0.2. |
| `MinDwell` | Seconds a clip plays before another may replace it. Default 0.15. |
| `Hysteresis` | The fraction a candidate must beat the current clip's cost by. Default 0.1. |
| `Responsiveness` | Half-life, seconds, of the spring that predicts the character's velocity and facing. Default 0.15. |
| `FootLock` | Whether contact data feeds the foot IK. |
| `Debug` | Draws the predicted path and prints the clip it picks. |
| `CurrentClip`, `CurrentTime`, `LastCost` (read only) | What it is playing and how well it fits. |

`MotionMatcher:SetTags(require, exclude)` narrows the search (a Space's crouch toggle sets `crouch`). The matcher's tracks are local to each machine and never become `TrackOp`s.

## The database

Every frame of every clip becomes one row of features, all in the character's root frame (metres, root at the origin, forward is -Z, +X is right):

| Group | Values | Count (4 trajectory samples) |
|---|---|---|
| Trajectory positions | x, z of the root at each trajectory time | 8 |
| Trajectory facing | unit facing direction at each trajectory time | 8 |
| Foot positions | left and right, 3 each | 6 |
| Foot velocities | left and right, 3 each | 6 |
| Root velocity | x, vertical, z | 3 |

Each dimension has its mean removed and each group is divided by its own spread, so a group's weight, not its units, decides how much it counts. Weights apply at search time, so tuning needs no rebuild. The build is deterministic: the same clips and sample rate give the same frame indices on every machine.

**Root motion.** The bundled Mixamo clips are in place (the hips travel about 8 cm per cycle), so a clip's root velocity and path come from its `Speed`, `Heading` and `YawRate` attributes. A clip that does carry root motion uses it. Root velocity is a matching feature only: the hips stay pinned and the controller moves the body.

**Contacts.** A foot is in contact on a frame when its world-space speed is under a small threshold and its sole is within 5 cm of the clip's lowest foot height, smoothed over three frames. World-space speed is the foot's speed through the body plus the clip's root velocity: a planted foot in an in-place walk moves backward through the body at ground speed, so the body-relative speed alone would mark no foot as planted. Two bits per frame are stored.

**Looping and ends.** A looping clip wraps. A clip that does not loop is excluded from the search once less than `BlendTime` remains, and its path beyond the end holds the last velocity.

## The runtime

### Where the matcher sits

The matcher is a source of tracks, not a second owner of the pose. It decides "clip X at time t" and `drive_graphs` expresses that as one node at full weight with a seek time, exactly as it does for any playing track. Priority blending, coverage classes, part rigs and the `AnimationTrack` API keep working, and action tracks still layer above it.

Frame order in `PostUpdate`:

1. Bevy samples the graph.
2. `AnimatorSet::RootMotion` pins the hips.
3. `AnimatorSet::Inertialize` applies the decaying offset.
4. `AnimatorSet::Procedural`: breathing and landing flex.
5. `AnimatorSet::Ik`: foot placement and climb grips, last of the pose writers.
6. `AnimatorSet::Facing`, `Overrides`, `Joints`.

`AvatarSystems::PostAnim` sits after `Inertialize`, so the avatar's procedural layers see the smoothed pose.

### Trajectory

The desired velocity (input direction times target speed) and desired facing go through a critically damped spring with the `Responsiveness` half-life. Stepping that spring forward at 30 Hz gives the root position and facing at each trajectory time, which are then expressed in the root's current frame. The controller follows the same spring, so the prediction and the body agree. A remote character uses its replicated velocity as the desired velocity.

### Search

A linear scan over every candidate frame. At ten thousand frames, ten searches a second and two hundred characters that is a few million multiply-adds a frame, so there is no search tree; characters stagger their search ticks by a phase taken from their identity. A search runs on the interval, immediately when a non-looping clip ends, and immediately when the desired speed changes by more than 0.5 m/s or the heading by more than 30 degrees.

To stop thrashing:

- The current clip's cost is multiplied by `1 - Hysteresis`, so a candidate must clearly win.
- Nothing switches inside `MinDwell`, except when the character leaves or reaches the ground.
- A match in the same clip within 0.25 s of the current time is ignored.

### Inertialization

When the matcher switches, the pose that was on screen and its velocity are compared with the new clip's pose and velocity, and the difference becomes an offset that decays to nothing. No two clips are ever blended.

- Rotations: the offset is the shortest rotation from the new pose to the displayed pose, stored as a scaled axis; the new pose is composed with the decaying offset each frame.
- Translations: the hips only.
- Decay: a critically damped spring, `x(t) = (x0 + (v0 + y*x0) * t) * e^(-y*t)`, with `y = 5.8 / BlendTime`, so the offset has fallen to 2% of its start when `BlendTime` has passed.
- A switch during an unfinished decay starts from the displayed pose, which already contains the old offset.
- Inertialization always runs before the foot IK, never after it.

## Foot locking

The matched frame's two contact bits feed the existing foot IK as stance hints. The lock point is taken on the first contact frame and released when the bit clears, and lock targets blend over 0.1 s on lock and release, so a switch in mid-stance cannot pop. The IK's own slide limit (the lock trails the foot by at most 0.35 of a leg) stays as the safety net.

## Parkour and motion warping

Clip-based vault and mantle moves replace the keyed IK limb paths for those moves; two systems posing the same limbs would fight. The climb system keeps everything that is about the world: detecting the ledge, wall and vault height, the phase state machine, the target transforms (hand points, ledge top, landing), the capsule's path, and the continuous moves (hang, shimmy, transfer) that procedural IK already does well. `ClimbPhase::clip_hint` is the seam where it names the clip.

Warping is root-only. A clip marks warp windows with `Keyframe` markers named `WarpStart:<target>` and `WarpEnd:<target>`, for the targets `HandsOnLedge`, `FeetOnTop` and `Landing`. For each window the warper compares the clip's own root displacement with the displacement to the climb system's target, scales it per axis between 0.6 and 1.6, and turns the yaw to face the target. The warped root path becomes the capsule's path for the move; the hips stay pinned, and the existing hand IK glues the hands to the ledge inside the `HandsOnLedge` window. There is no per-joint warping.

## Multiplayer

Every machine runs the matcher for every character it draws, fed from the avatar and motion lanes (velocity, facing, grounded, jump). A remote character's desired velocity is its replicated velocity through the same spring. Nothing new is replicated and the matcher's tracks never log `TrackOp`s, which would flood the lane at ten a second per character. The results need not match bit for bit, because the matcher is cosmetic and the controller owns the capsule. The host checks plausibility on the controller, as it does now, and the matcher stays off on a headless host.

## The clip library

A matcher is only as good as its database. With the four in-place Mixamo clips per body it reproduces today's blend and adds inertialized transitions. The quality comes from this set, downloaded with root motion (not "In Place") and retargeted by bone name:

| Group | Clips |
|---|---|
| Idle | two idles |
| Walk and run | forward at each |
| Starts | from idle: forward, left 90, right 90, 180, at walk and run |
| Stops | plant left and plant right, at walk and run; run to stop |
| Turns in place | 90 and 180, both sides |
| Strafes and backward | strafe left and right; walk back; jog back |
| Jump | take-off, airborne loop, soft landing, hard landing |
| Parkour | one-hand vault, low mantle, high mantle, ledge hang idle |

About thirty clips, around ten thousand frames at 30 Hz.

## Phases

| Phase | Delivers | Visible at the end | Status |
|---|---|---|---|
| 1. The pure core | `animation/motion/`: trajectory prediction, features and database build, contact derivation, search with hysteresis and dwell, inertialization. Pure Rust, tested on synthetic clips. | Nothing on screen; the tests pass. | Being written. |
| 2. Shadow, then drive | `MotionDatabase` and `MotionMatcher` classes, the clip sampler that builds a database from glTF and clip records, the `Inertialize` set. The matcher runs beside the `Animate` script with `Debug` on, then drives the graph with the four existing clips. | It looks like today's gait, with the path and chosen clip drawn. `Enabled` toggles it. | Designed. |
| 3. The library | The thirty clips, tags, live contacts, foot-lock hints. | Starts, stops, turns and strafes that the blend cannot make. | Designed. Needs the clips. |
| 4. Jumps and remote characters | Jump phases as matched clips; the matcher on replicas. | Another player's avatar moves as well as yours. | Designed. |
| 5. Parkour | Vault and mantle clips with warp windows replacing the keyed IK paths. | Vaulting a fence lands the hands and feet on it at any height. | Designed. |

Not built: a search tree or learned matching, per-joint warping, contact extraction from in-place clips, replication of the matcher's state, matching for the upper body (action tracks stay tracks), any warp editor beyond markers.

## Risks and the early signal for each

1. **Content quality.** Mixamo clips disagree on speed and foot phase, so starts and stops may still pop. Signal: phase 2 looks worse than the `Animate` blend, and the first ten library clips do not close the gap.
2. **The controller and the clip disagree on velocity,** so the feet slide. Signal: more than 10 cm of slide per step in the `Debug` readout after phase 3. The cure, making the controller follow the clip's velocity, is a much larger change.
3. **Graph cost.** Every database clip is a started node in the graph, even at weight 0. Signal: frame time in a hundred-character test grows with the matcher on. Mitigation: stop inactive nodes and start only the matched one at the switch.

## Tests

Phase 1 runs on synthetic clips: a synthetic walk must match itself at the right phase; a run trajectory must pick the run clip; a 180 degree turn trajectory must pick the turn clip; staying put must not switch; the dwell and the hysteresis must hold; inertialization must start on the displayed pose, keep its velocity, take the short way round a rotation and settle within `BlendTime`.
