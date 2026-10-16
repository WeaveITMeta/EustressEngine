# Peer motion, Part B: what a joined player may do to the host

Status: **built in scratch for build 18, after Security's design review;
McKale's two decisions (2026-10-01) are in sections 1 and 2.** Part A (build 16) stopped a
peer from choosing its own speed: every peer descriptor's `motion` is
cleared, and replicas are dressed by the Space's `SpaceCharacterPolicy`.
Security's review of Part A found what is left; this is the plan for it,
plus Security's R1 from the build 17 review.

Code: `common/eustress-networking/src/session.rs` (movement, appearance,
corrections), `wire.rs` (`ToPlayer::Correct`, protocol 5),
`common/src/avatar/space_character.rs` (the two StarterPlayer switches, a
joined player's own motion) and `profile.rs`, `engine/src/multiplayer.rs`
(R1, the Output line), `common/assets/service_properties/StarterPlayer.toml`
(the switches in Studio's Properties).

## 1. Movement: a travel budget

`host_avatar_frame` keeps a `TravelBudget` per player: metres it may still
travel across the ground, up, and down since its last accepted sample.

- **Earning.** Each sample earns travel at the player's pace for the time
  since the last *accepted* sample, at most 10 s of it (`STALL_CAP_S`), so
  lost packets and hitches are forgiven, and a peer that withholds samples
  can jump at most ten seconds of its pace, once.
- **Spending.** A sample spends the distance it moved. One that would
  overspend is refused, counts as a violation (60 in 10 s removes the
  player), and leaves the budget as it was.
- **Bank.** After an accepted sample at most one second of pace stays
  banked (`BUDGET_SECONDS`): standing still never buys a sprint, and a
  sustained run above the pace is refused within a second or two.
- **Pace across the ground:** the larger of the walk and the capped sprint
  (`capped_run_and_sprint().1`, at most 12.42 m/s unless the walk itself is
  set higher), times 1.15, plus 0.5 m/s.
- **Up:** the jump's take-off speed under the live gravity
  (`jump_velocity_under(live_gravity)`) times 1.5, or the pace across,
  whichever is more. **Down:** at most 150 m/s (`MAX_FALL_MPS`), past any
  real fall (700 m of falling reaches 117 m/s).
- **Whose pace:** the replica's live `AvatarBody.motion`, which host
  scripts change (`Humanoid.WalkSpeed`, play-runtime `apply.rs`), as it was
  at its fastest over the last second (`MOTION_GRACE_S`): a slow-down
  reaches the player a round trip later.
- **TEMPORARY floor.** Today the pace is also never below the one the
  replica's descriptor gives it. A Player's own avatar loses a host
  script's WalkSpeed when its rig binds (`finalise_metrics_on_bind`,
  `common/src/avatar/spawn.rs`) or when it respawns, so without the floor
  an honest player slowed by a script would be refused. The cost: a host
  script's slow-down is not enforced, and **a freeze (WalkSpeed 0, for
  freeze tag, a stun or a cutscene lock) does not hold against a hacked
  client.**
- **End state,** once a rebound or respawned avatar keeps its Humanoid's
  values: the live pace alone (with the one-second grace), so a freeze
  holds. Its test: a host script sets WalkSpeed 0, and a peer's movement
  one second later is a violation.
- **Before the replica exists** the budget starts at the spawn point the
  Welcome gave (`spawn_for`) when the player enters the world, at 30 m/s
  (`PRE_REPLICA_PACE_MPS`), earning at most 2 s of it per gap
  (`PRE_REPLICA_STALL_S`): the replica follows within frames.
- **Exempt:**
  - A seated replica (`AvatarSeated`): the seat's vehicle moves it. Its
    samples are kept and relayed at the seat, wherever they say it is, and
    its budget waits there, so leaving the seat, a new body for a changed
    look, and what everyone else sees all start from the seat.
  - A host script's teleport (`teleport_hold`): a sample near its target
    starts the budget again there.
  - A carrier a host script moved farther in one frame than any pace
    allows: its rider may follow it there, once.
- **Carriers** (`track_paces`): the host casts a ray 1.5 m down from each
  replica (Avian's `SpatialQuery`). The speed of what it hits is the faster
  of how the point under the replica moved since the last frame (a part a
  script tweens) and that point's physics velocity (linear plus angular),
  at most 60 m/s (`CARRIER_CAP_MPS`), and it adds to every pace above.
  - No avatar carries another, nor anything an avatar holds: players
    standing on each other would add up their speeds.
  - A replica falling past a part faster than 4 m/s relative to it is not
    riding it.
  - A carrier it left lends its speed 0.5 s longer (`CARRIER_HOLD_S`),
    through a jump's arc.
- **Rate.** The host reads each player's newest sample of a frame only,
  so a burst (a stall's backlog over a stream transport) is read as its
  last sample, and at most 40 samples a second from a player (twice
  `AVATAR_SEND_HZ`), with a second's worth banked for a burst after a
  hitch; more are dropped unread, so a flood costs neither the host nor
  the players it would be relayed to. The transport itself caps no
  datagram rate.
- **Order.** A sample is kept only when its `seq` is 1 to 4096 ahead of
  the newest kept (`MAX_SEQ_GAP`, over three minutes of loss); an older or
  repeated one is dropped, and one further ahead is refused as a
  violation, since kept, it would make every later honest sample look
  older and freeze the replica on the host while everyone else saw the
  player move. Only a sample the host kept is relayed.
- **Corrections.** On a refusal the host tells the player where it has it
  (`ToPlayer::Correct`, the last accepted sample), at most twice a second.
  The Player moves its avatar there, never to a NaN or a place past the
  world limit, not while it is seated, and not within a second of
  something on its own machine moving the avatar farther than a step (a
  host teleport, which an older correction would undo).
- **What it does not catch:**
  - Moving up to 1.15 times the pace plus 0.5 m/s.
  - Hovering or flying at that pace.
  - A peer that withholds its samples can blink up to 10 s of its pace,
    once per silence.
  - A host script's slow-down, while the descriptor floor stands (above).
- **A Space can turn the check off** (McKale, 2026-10-01): StarterPlayer
  `ServerMovementCheck`, on unless the Space sets it false, for a Space
  whose own player-side scripts move characters faster than they walk (a
  dash, a launch pad run on the player). Off, the host keeps any finite
  sample inside the world as sent; the rate, order and seat rules still
  hold, and turned on again, the check measures from the last kept place.
  - The host reads it only from its own Space (`SpaceCharacterPolicy`,
    loaded from its StarterPlayer), never from anything a player sends.
  - While hosting, the Output says plainly when the Space has it off, and
    again when it comes back on (`engine/src/multiplayer.rs`).

## 2. Appearance: what a peer may choose

`parse_appearance` (both the host and every Player, so all agree):

- **Rigs:** a peer's rig that names any `space://` asset (body or clip) is
  dropped, leaving the built-in rig for its body option: the Worker only
  issues `bundled://characters/` rigs, and a `space://` one would let a
  peer wear any model in the host's Space. A rig with more than 128 bone
  aliases, or a name longer than 64 characters, is dropped the same way.
- **Items:** a descriptor with more than 8 slots (the Worker's cap), two of
  one kind, or an item id that is not 1 to 64 letters, digits, `_`, `.` or
  `-`, is refused.
- **Body proportions** (McKale, 2026-10-01): players keep their own
  height, build and leg length by default. **A Space must turn on
  StarterPlayer `StandardPlayerBodies` for fair hitboxes and speed.**
  Without it, speed follows the body where the Space sets no
  `CharacterWalkSpeed` (the tallest walks 28.6% faster than default, the
  shortest 25% slower), and so does the host's collider (capsule radius
  0.175 to 0.388 m, height 1.45 to 2.05 m), so each player picks its own
  hitbox. On, every player's character in that Space, the host's included,
  takes the standard body: the default height, build and legs (a Robot
  keeps its one body), then the Space's own height for its character when
  it sets one; face, colours and items stay the player's. It happens in
  `SpaceCharacterPolicy::apply`, which the host, every Player and each
  player's own avatar all use, so everyone sees the same body and the host
  measures the pace the player moves at.

## 3. Appearance changes: no collider gap, no flood

- The new replica is built **before** the old one goes: the old one stays
  until the avatar runtime builds the new one, and is despawned in the
  frame the new one is claimed (`tag_replicas`). If the runtime never
  builds it (2 s), the old body stays, in the look it was built with, and
  the new look is dropped rather than retried.
- The host takes at most **one changed look per player every 5 s**, in
  the world or while the world downloads. A sooner one waits and is
  applied when the 5 s pass, or when the player enters the world (the
  newest wins), and each counts as a violation, so a flood removes the
  player. Resending the same look changes nothing and costs nothing.
- Every machine rebuilds at most **2 replicas a second** for changed
  looks, after every first spawn, so changing looks never holds up a
  player's arrival.

## 4. A joined player's own motion

The account's motion overrides (the Worker allows up to 100 m/s) drove a
joined player's own avatar where the Space sets no walk or jump, while its
replica moved at the Space's pace, so an honest player with an override
would be refused. A Player joined to a host holds `SpaceOwnsMotion`
(`common/src/avatar/space_character.rs`; inserted by `begin_join`, removed
when the session ends), and its saved avatar keeps none of the account's
motion (`profile.rs`).

## 5. Heartbeat answers that arrive late (Security's R1)

Each heartbeat POST carries a number, and the answers queue (every one is
read; an ok is never lost behind a later refusal). Any answer that may have
stored the link marks the session maybe listed, whatever its number.
Admission opens again only when every heartbeat of this hosting session is
answered and none may have stored the link: an earlier POST's refusal,
arriving after a later POST went out, leaves it on. Stopping hosting
forgets the unanswered numbers, so a late answer from before never opens
the next session (`engine/src/multiplayer.rs`).

## Release order

Protocol 5 refuses a Player on protocol 4 at Hello, with "Update whichever
is older." Players with build 18 must be published before any host runs
it.

## Staging checks (two machines)

Only a live run proves these:

- A modified client that runs past its pace is refused and moved back, and
  the avatar's physics body does not pull it away from the corrected
  place (the Player writes `Transform`, as replica correction does).
- A rider on a real moving platform, and on a lift, is never refused.
- A 3 s and a 12 s network stall (netem) while walking: the first is
  forgiven; the second is forgiven within the 1.15 margin, or moves the
  player back once, never removes it.
- In and out of a seat, at speed.
- A look change while another player stands in the same place.
- A Player on protocol 4 joining a build 18 host sees a message a person
  can act on.
- Play a Space A whose StarterPlayer turns ServerMovementCheck off, stop,
  open a Space B that leaves it on, and start a server from Edit: the
  Output shows no "turns the movement check off" warning, and a joined
  player running past its pace is refused.
- A link that loses more than 4096 samples in a row (over three minutes)
  leaves that sender frozen on the far side until it reconnects; a link
  dead that long is a disconnect anyway, so this should never be seen.

## Tests

- Movement (`session::tests`): a sustained run at twice the pace is refused
  and removes the player, with at most two corrections a second, each the
  last accepted place; walking at a pace a host script raised, with 20 ms
  of jitter, is never refused; a burst after standing still is kept within
  a second of pace; a stall is forgiven up to its cap; a seat carries a
  player anywhere and leaving it starts from the seat; a host teleport
  starts the budget again where it went; a seated player is where its
  seat is (kept, relayed and rebuilt there); samples past the rate are
  dropped unread; a burst in one frame is read as its newest sample; a
  seq leap is refused and only kept samples are relayed; a rise faster
  than a jump is
  refused and a fall is not; before its replica a player moves from its
  spawn, and waiting buys nothing; entering the world starts the budget at
  the spawn; a rider follows its carrier's jump once; a pace taken away
  counts for its grace only.
- Carriers: a tweened part lends its speed; a moving body lends its
  velocity, up to the cap; a carrier's speed is kept briefly after leaving
  it, and a slowing one lends its speed now; falling past a carrier
  borrows nothing; a carrier snapped a kilometre lends no speed; no avatar
  carries another; through Avian's ray, a platform lends its speed and a
  player's head lends none.
- Appearance: a `space://` body or clip, a long alias and too many aliases
  each drop the rig, and a bundled one is kept; too many items, two of one
  kind and odd item ids are refused.
- Replicas: a changed look replaces the body without a gap; a look the
  runtime cannot build keeps the old body; rebuilds are paced and never
  hold up a first spawn; looks change at most every 5 s and a flood
  removes the player; looks sent while downloading are paced too.
- Corrections on the Player: only sane positions, never while seated,
  never over a later host teleport.
- Wire: `ToPlayer::Correct` is tag 14.
- Own motion (`profile::tests`): a joined player keeps none of its
  account's motion.
- Heartbeat (`multiplayer::tests`): an earlier heartbeat's answer never
  releases admission; answers arriving together are each read.
- The switches: the movement check is on and standard bodies off unless
  the Space says, read as file keys or Roblox names
  (`space_character::tests`); standard bodies keep face, colours and
  items, and a Robot its body; a Space can turn the movement check off,
  and the host reads it from its own Space (`session::tests`); the Output
  says when it is off, and when it is on again (`multiplayer::tests`).
