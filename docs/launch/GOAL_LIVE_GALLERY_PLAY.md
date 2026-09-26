# Goal: live play from the Gallery

**Set by McKale, 2026-09-24.** Orchestrated from the session formerly titled "$60B roadmap"
(`local_16cb06c1-df8d-4fb2-a662-e9052593bfd7`); owning sessions report there.

**One line (for `/goal`):** Play Pong, then Vehicle Simulator, from eustress.dev/gallery, hosted on
the workstation and reached over UDP through Cloudflare Tunnel, with a second player joining.

---

## Definition of done

1. Pong exists as a Space, is published, and has a Gallery listing.
2. McKale hosts it on the workstation (Studio, F9) with no router port forwarding.
3. Game traffic is UDP end to end: QUIC, carrying the WebTransport session already written in
   `eustress-networking`, through Cloudflare Tunnel.
4. A second player on a different network joins from the Gallery. Both see the same ball, both
   paddles, and the same score, and each moves only their own paddle.
5. Evidence: a recording of both screens, and the host log showing the remote connection arriving
   through the tunnel.

Vehicle Simulator meets the same bar as the stretch milestone.

---

## How UDP gets through Cloudflare

Checked against Cloudflare's documentation on 2026-09-24:

- **Public hostnames on a tunnel carry HTTP, HTTPS, WebSocket, and TCP. They do not carry UDP.**
- **Private network routes on a tunnel do carry UDP**, to devices running the Cloudflare One (WARP)
  client enrolled in the Zero Trust organization, which is free for up to 50 users. UDP rides only
  when `cloudflared` reaches Cloudflare over QUIC (outbound UDP 7844); on the HTTP/2 fallback it
  stops. Tunneled UDP payloads are capped near 1,280 bytes, and QUIC needs at least 1,200, so the
  path fits with little room; `infrastructure/cloudflare/tunnel/udp-probe.ps1` measures it.
- **Public UDP to a private origin exists**: Spectrum private origins, routed through Cloudflare
  Tunnel, announced 2026-06-10. It is a closed beta for eligible Enterprise customers, with general
  availability targeted for Q4 2026, requested through a Cloudflare account team.

So the route is staged, and the engine's transport does not change:

- **Stage 1, now.** A tunnel with a private network route to the workstation's UDP 7777. The second
  player runs the Cloudflare One client enrolled in the Eustress organization. This proves UDP
  through the tunnel end to end with the WebTransport host as written.
- **Stage 2, public.** The same tunnel fronted by Spectrum private origins once the account has
  access. Anyone can join from the Gallery; nothing in the engine changes.
- **If Stage 2 is refused.** Cloudflare Realtime TURN relays UDP publicly today (1,000 GB free,
  then $0.05 per GB), but it carries WebRTC, so the transport would move from WebTransport to WebRTC
  data channels. That is a rewrite of the transport layer and waits for McKale's go.

### Risks to test early

| Risk | Test or mitigation |
|---|---|
| A web page cannot set Chrome's QUIC packet size, and the tunnel caps UDP near 1,280 bytes | In Stage 1, after the raw UDP probe passes, a real browser WebTransport handshake through the tunnel: `infrastructure/cloudflare/tunnel/webtransport-probe.html`, checked in Chrome 152 against a closed port, not yet against a live host. The host's `/probe/<key>` echo for its datagram half is written (`native.rs`, Multiplayer; not built); its host log line, `net: probe session from <remote>`, is also the evidence item 5 of the definition of done asks for. The host and the desktop Player cap their own QUIC packets at 1,250 bytes of UDP payload (`quic_transport` in `native.rs`: initial MTU 1,200, discovery capped at 1,250), so the native path fits the tunnel. A browser follows the host only through the `max_udp_payload_size` transport parameter, which both ends now advertise as 1,250 (Multiplayer; type-checked, not built). Before the handshake completes, only Chrome's own padding counts: if the probe measures Chrome's Initial packets above 1,250 bytes, raise `MAX_UDP_PAYLOAD` in `native.rs` to that size, still under the tunnel's cap, or the host drops a browser's first packets |
| Chrome's Private Network Access, becoming a Local Network Access permission prompt, may stop eustress.dev from opening WebTransport to a private address | The same Stage 1 browser test. Stage 2's public hostname removes it |
| Cloudflare Pages caps a single file at 25 MiB, and a release wasm of Bevy, Avian, and eustress-common can exceed it before wasm-opt, which is not installed | Serve the .wasm from R2 with `application/wasm`; the download worker needs a route for it |
| The site's Content Security Policy is Report-Only today | If it is enforced, `connect-src` must admit the host endpoint: the private address in Stage 1, the Spectrum hostname in Stage 2 |
| The host seeds its DataModel tree from Studio's ECS after the loader's metre conversion (`engine/src/play_datamodel/seed.rs`), while the Player's reader builds its tree from the records. If the two conversions differ, a joined Player starts with different properties than the host, and nothing corrects them until a script writes them | One record-to-property conversion in eustress-common that both use (move `property_to_dm` and the data half of `describe` beside the datamodel), and a test that builds both trees from the same records and compares them, skipping Camera instances and other host-local ones (Studio's seed adds a virtual `CurrentCamera`, `seed.rs:129`; Cameras never replicate). Client's proposed SpacePart-to-props helper is the same job, so it becomes that one function, not a second. Accepted by Multiplayer: Eustress WASM moves the conversion (gated on McKale's go), and Multiplayer reviews the switch in `seed.rs` |
| Studio's loader gives every folder instance that is not a Part, script, particle, terrain layer, GUI, media, sky, or light (Model, Folder, Camera, Humanoid, values) its folder's name with the first letter upper-cased instead of `[metadata] name`, ignores its `[transform]`, and drops its attributes and tags (`engine/src/space/file_loader.rs:3032`). Scripts that find instances by exact name, and Vehicle Simulator's configuration attributes on Models, break. The Player's reader mirrors it so both trees start equal, except for Cameras, which must keep their `[transform]` for Pong's view | Fix both sides together after the M0 build: Roblox Place Import in Studio's loader, through the shared `record` conversion, and Eustress WASM dropping the reader's mirroring. **Nested parts under a sized part: McKale chose the hidden anchor (2026-09-24).** Each sized part with nested children gets an invisible, unscaled anchor child (scale 1/size) that the children hang from, so a file's `[transform]` is relative to the parent's pose, never its size. On any reparent or resize, one reactive system recalculates the anchor, instead of changes at every call site that sets a parent. It lands after the attribute and general-branch chain, in its own edit window. **Landed 2026-09-24, type-checked, not built:** WASM's common part (20:27), Roblox Place Import's engine half (`space/pose_anchor.rs`, 20:31) and WASM's parity step (20:53). The rule applies only to a Space whose `space.toml` says `transform_rule = "parent_pose"`; the importer writes it from the next re-import on, and every other Space keeps the legacy rule. Migrating existing Spaces comes next, in its own window after the build |
| **Studio's save paths corrupt imported Spaces** (found by Roblox Place Import, 2026-09-24). An imported file says `[metadata] unit = "ft"`; the loader converts it to metres, but move, select-drag, align and distribute, scale, the mirror and Save Space write engine metres back with `unit = "ft"` still set. On reload each part moves toward the origin and shrinks to about 30%; one Save Space shrinks a whole imported Space. No saved file shows it yet | One conversion helper in `instance_loader.rs` into the file's own unit, used by every writer, with round-trip tests. Roblox Place Import is doing it now, and the next build waits for it. Until then: no saving or moving parts in imported Spaces (Vehicle Simulator 2, Place 36, ServerManagement, Mountain Ascension, Super Station Curated) |
| At full quality, the Player alone held a GTX 1080 Ti at 99% (vsync on, a 1920x1080 window), and volumetric clouds are on by default. A player on a mid-range card would get a slow or failing game | Production needs graphics quality presets chosen from the detected card, with a manual override in the Player's settings. The clouds, sky, reflections and shadow switches exist as environment variables only (`EUSTRESS_CLOUDS`, `EUSTRESS_SKY`, `EUSTRESS_SSR`, `EUSTRESS_SHADOW_DISTANCE`). Owner to assign after the shared Play runtime lands |
| Studio and the Player share the workstation's one GPU (a GTX 1080 Ti), and a Pong engine already crashed another session's GPU capture with DeviceLost | Run M0 with no other engine or GPU capture open |
| A Bevy system-parameter conflict (B0002: one system reading and writing the same resource, often through nested `SystemParam` structs) passes every type check and every build, then panics when the system first runs | Launch each new Studio build once before handing it to a test; when adding a `SystemParam` to an existing system, check its resources against the host system's other parameters |
| A lock-free type check against a stale `eustress-common` library reports errors that are not in the source. On 2026-09-24 a 64-error engine count named `TerrainVoxelWater` and `SpaceGeometry::custom_mesh_parts`, both present in the source, while a real build passed its type check | Only a real build, which compiles common from source, decides whether the tree compiles. A lock-free engine check must type-check common from source first. It must also regenerate the Slint Rust code from the current `.slint` sources: the 22:07 build on 2026-09-24 failed on a `GuiElementData` initializer that was missing two fields `main.slint` had gained at 16:53, which every lock-free check missed because it linked the previous build's generated code |

---

## Milestones

| # | Milestone | Proof |
|---|---|---|
| M0 | Native proof | A Studio host and a desktop Player on this machine; two avatars move in both windows (the Phase 4 test in `docs/AUDIT/03_MULTIPLAYER.md`) |
| M1 | Pong on this machine | Host authority; each player's input reaches the host; ball, paddles, and score replicate; Studio and the Player play a round |
| M2 | Pong through the tunnel (Stage 1) | The Player on Muse's virtual machine, through the Cloudflare One client, plays a round against McKale |
| M3 | Pong from the Gallery, desktop | Pong is listed; Play hands the join link to the desktop Player; the listing shows whether the host is live |
| M4 | Pong public (Stage 2) | A player with no Cloudflare client joins from the Gallery |
| M5 | Pong in the browser | The browser Player joins by WebTransport. Pong needs no Luau in the browser, since its scripts run on the host |
| M6 | Vehicle Simulator | Class coverage beyond Part, vehicle replication, and streaming for 338,020 instances |

---

## Owners

| Workstream | Owner session | Blocks |
|---|---|---|
| WebTransport host and Player join (written, never run); player input to the host; replication after join | Multiplayer: Client and Studio | M0, M1 |
| The Pong Space | Box Head Zombie Defense Tycoon (proposed) | M1 |
| Desktop Player draws Parts at their size and shape, and joins from a Gallery link | Client | M1, M3 |
| Player reader: records to a DataModel tree, with `bind_scene` for every instance, in eustress-common outside `luau`; and the one record-to-property conversion that Studio's seed also calls (`record_props`), and `DataModel::create_scene` | Eustress WASM (started on McKale's go, 2026-09-24) | M1, M5 |
| **One shared Play runtime** (McKale, 2026-09-24: "replicate to the client from the runtime of Studio and just do it right"). Studio's Play stays the authority, and its DataModel already replicates. The Player draws that tree with Studio's own draw code, moved out of the engine into a shared crate: `play_datamodel`'s apply, camera, audio, particles and npc, the ScreenGui and BillboardGui spawners, `billboard_gui.rs` and `gui_loader.rs`. Seed, pull, remote players and the host side of commerce stay in the engine. `tree_apply.rs` folds in. Draw-side files are frozen to other sessions until the move lands. Multiplayer's constraints for the move:
(1) On the Player, the frame order is `net_replica` applying world frames, then LocalScripts (`RunTreeScripts`), then the draw step; the draw step stays last.
(2) The draw step despawns only what it spawned, through the `FromTree` marker. The motion lane moves exactly those entities, and character roots bound to avatar entities are never despawned.
(3) Parts under a Model that a Player's Character points at belong to the avatar runtime.
(4) Replication writes through normal DataModel writes, so the draw step reads the ordinary spawn, dirty, reparent and despawn queues.
(5) GUI under Players is drawn only for the local player's PlayerGui.
(6) StarterGui originals are never drawn during Play.
`play_datamodel/mod.rs` gained two lines on 2026-09-24: `remote_players::pull_remote_characters` after `pull::pull_character`, and the host-only guard `hide_joined_player_guis`, which can go once the shared layer follows rule 5. PlayerGui follows Roblox: the host clones StarterGui into each joined player's PlayerGui and sends it only to that player, and a networked Player stops building its own copy. McKale's bar: "correct for production". That means a design first (`docs/architecture/SHARED_PLAY_RUNTIME.md`), moving code rather than copying it, Studio's Play unchanged, the parity test and the access-conflict scan green, and proof with Box Head hosted and joined Design: `docs/architecture/SHARED_PLAY_RUNTIME.md` (a new crate `eustress-play-runtime`; the tree and frame sets in common; `PlayRole` Authority or Replica). Decided 2026-09-24 under McKale's production rule. The HUD uses one GUI renderer in both apps (the billboard rasterizer gains a screen-space layer, images and clipping), rasterized per element with dirty-only redraw and GPU compositing, and its per-frame cost is measured on Box Head. The six primitive meshes move to `bundled://parts/` in common. **Steps 1 and 2 landed 2026-09-24 (20:25, 20:47), type-checked, not built.** The crate `eustress/crates/play-runtime` holds apply, camera, audio, particles (with the renderer and shader), materials, the material registry and part colliders, and the engine keeps one-line re-exports. Script terrain edits stay in the engine (`play_datamodel/terrain_edits.rs`, behind the undo stack). `play-runtime/src` is frozen to all but Client until step 4 | Client (lead), with mlua, Multiplayer and Eustress WASM | M1, M3, M6 |
| Luau on the Player: Studio's Play runtime (`common::luau::play`) on the Player's tree, with the RemoteEvent queues (gate 10) | mlua (asked by Multiplayer) | M6 |
| Player apply step: DataModel tree to ECS entities, in eustress-common outside `luau` (colliders behind `physics`), driven by the tree's frame drains, with Studio's `engine/src/play_datamodel/` as the reference. It defines the Bevy resource `PlayerTree`: the reader creates it, and Multiplayer's replica writes to it before the `TreeApply` set. The desktop Player's networked open moves onto the tree, so no Part draws twice. Until it exists, no replicated change reaches a Player's screen; later it also draws what client scripts write (gate 10). With it, the Player's camera: a Scriptable camera the Space declares in `Workspace/Camera/_instance.toml`, which a replicated `Workspace.CurrentCamera` never retargets | Client (written: `common/src/tree_apply.rs`; type-checked, not built) | M1, M5, M6 |
| Tunnel, Zero Trust organization, Spectrum request, UDP probe | Orchestrator | M2, M4 |
| Gallery listing, live indicator, Play hand-off | Website, with Multiplayer, which keeps `web/src/pages/play.rs` | M3 |
| Browser Player | Eustress WASM | M5 |
| Vehicle Simulator readiness | Eustress Vehicle Simulator | M6 |

---

## McKale's gates

1. **Builds.** Each session says when it is ready and names the command; McKale runs it, one at a time.
   The commands ready now are in the build queue below.
2. **M0's two-window test.**
3. **Installs:** `cloudflared` on the workstation; the Cloudflare One client on the second player's
   machine (Muse's virtual machine can install it).
4. **Cloudflare account steps:** the Zero Trust organization, the tunnel, the private network route,
   and the device enrollment rule. Muse can do the dashboard work from the vault.
5. **Windows Firewall:** allow inbound UDP 7777 for the engine on the workstation.
6. **The Spectrum private origins request** (Enterprise closed beta).
7. **A staging Worker.** Today `eustress-api` exists only in production, and its CORS admits only
   eustress.dev and localhost, so a staging web preview cannot reach any API. This goal changes the
   Worker several times (the live-host registry, the world routes), so a staging environment with its
   own KV namespaces and CORS for preview origins is recommended. It is an account change.
8. **The first Player release.** Both `latest.json` and `player/latest.json` return 404, so the
   download page says "not released yet" and nothing is installable. Gallery Play needs a Player to
   hand the link to. The pipeline is built (`.github/workflows/player-release.yml`, Windows, macOS
   ARM64, and Linux, publishing to R2 under `player/`), and it needs two things: the downloads Worker
   deployed, since the live one predates the repository, and a `player-v0.1.0` tag, which starts the
   release.
9. **Every deploy:** staging first, then production. Production `eustress-api` already runs
   moderation, the world routes, the commerce Durable Objects, the Bliss treasury and the live-host
   registry. Two uploads on 2026-09-24, at 12:34 and 15:13 UTC, carried them, with no staging step.
   The live version, `4584e933`, has migrations `v1-commerce`, `v2-treasury` and `v3-live-hosts` applied
   and the secrets `JWT_SECRET`, `GROK_API_KEY`, `JEV_API_KEY`, `STRIPE_SECRET_KEY` and
   `STRIPE_WEBHOOK_SECRET`. The gallery lists 0 simulations. The Worker edits since that upload
   (`index.js` and `wrangler.toml`, 2026-09-25) are in the repository only. Check what production runs
   with `npx wrangler deployments list` and `npx wrangler versions view <id>` (both read-only, with
   `--config infrastructure/cloudflare/api/wrangler.toml`).
10. **Scripts on the Player** (Vehicle Simulator only; Pong does not need it). VS runs its player flow
    in 146 client-side script files, including the dealership. On the desktop there is no VM to
    build: mlua is the whole Luau stack, and the Player already compiles it (`client/Cargo.toml`
    turns on eustress-common's `luau`) and loads a Luau plugin (`client/src/soul/mod.rs:43`). What
    the Player lacks is Studio's Play runtime, `common::luau::play`: one VM bound to the DataModel
    tree, which only Studio's `engine/src/play_datamodel/` drives today. On the Player it runs on the
    tree the reader builds and the apply step draws, with RemoteEvents crossing the network
    (`remote_out` and `remote_in` in `docs/networking/SERVER_AUTHORITY.md`). The browser is the open
    part. mlua's Luau VM is C++ that cargo compiles on every build (luau0-src, 90 files; Visual
    Studio's compiler does it silently here). mlua reaches WebAssembly only through emscripten, and
    this machine has no compiler that builds C++ for the browser (no LLVM clang, no emscripten). With
    LLVM installed (`winget install LLVM.LLVM`), Eustress WASM tests whether Luau links into the
    browser Player; the fallback is Luau as a separate emscripten module beside it.
11. **The host.** Studio's F9 now; later a dedicated headless host with its own credential, as
    Multiplayer recommends.
12. **Vehicle Simulator's webhooks.** Seven Discord webhook URLs sit in its server-side services.
    Once the publish security fix is built they never reach players with Share Source off; with Share
    Source on, publishing refuses until they are removed or rotated.
13. **The admin role** on McKale's eustress.dev account, so he can approve Pong's listing. The
    Worker's `requireAdmin` accepts an account when the `USERS` namespace holds the key
    `admin:<user id>`. These commands write to production, so McKale runs them himself, from the
    repository root:
    1. His user id: `npx wrangler kv key get --binding USERS "username:<his username>" --remote --config infrastructure/cloudflare/api/wrangler.toml`
    2. The grant, which takes effect on his next request: `npx wrangler kv key put --binding USERS "admin:<user id>" "granted <date>" --remote --config infrastructure/cloudflare/api/wrangler.toml`.
       To revoke it: `npx wrangler kv key delete --binding USERS "admin:<user id>" --remote --config infrastructure/cloudflare/api/wrangler.toml`.
    3. Once Pong is published: `POST https://api.eustress.dev/api/admin/moderation/tool` with his
       bearer token and `{"name":"moderation_approve","args":{"sim_id":"<Pong id>","rating":"all_ages","rationale":"<20 or more characters citing what was observed>"}}`,
       or the `moderation_run_tool` MCP tool.
14. **Eustress WASM's go** for the Player's reader and the shared record-to-property conversion:
    **given by McKale on 2026-09-24.** A joined Player applies a replicated write only to an
    instance in its tree, and Pong's Ball, Paddles, and Score are scene instances, so the reader is
    the last piece before a joined Player's own window shows Pong. The browser port itself still
    waits for M0.

### Edit windows

Shared files change in scheduled windows, each closed by one build, so no build compiles a half-finished change.

How a window closes and reopens:
- **Safe point:** each session reports that nothing of its own is half-applied, and lists what it landed.
- **Freeze on:** no edits under `eustress/crates`, `eustress/Cargo.toml` or `Cargo.lock`. The settled check (the full lock-free chain, with Slint regenerated from `engine/ui/slint` and play-runtime included) and the access scan run on the frozen tree, and the build starts only if both are clean.
- **Freeze lifted:** once the engine lib and the client rustc have started, because rustc reads a crate's sources when its compile starts, AND no dependency's rustc is still running. Cargo pipelines: the engine lib starts as soon as its dependencies' metadata exists, while their rlibs are still being written, and on Windows a lock-free check that maps one of those rlibs blocks cargo from replacing it. Once the dependencies' rustc processes have exited, their rlibs are final for that build and lock-free checks can run again.
- **`engine/src/main.rs` stays frozen longer.** The engine's bin compiles in its own rustc, which starts only after the lib finishes. A `main.rs` line naming a module that the compiling lib lacks fails the build at the bin step. New plugins register from inside the lib (for example in `StudioPluginSystem::build`), so `main.rs` rarely needs to change at all.

1. **Open now, closed by the stability build:** Eustress WASM's attribute and general-branch chain in a fixed order (WASM's common Part 1, then Roblox Place Import's engine half, then WASM's Part 2 with the `seed.rs` parity fixture, with no build in between); Roblox Place Import's `luau/compat.rs` script-rewrite fix after it, plus its importer changes; Client's parkour change and the avatar IK crash guard; Multiplayer's non-finite guards, the F9 stall and the version Hello; Avian's units cleanup; Terrain's `warn!` fix.
2. **After the stability build:** first Multiplayer's server-authority phase 2 on the host, meaning characters for joined players, through a shared builder extracted from `pull_character` in `play_datamodel/pull.rs`, which the next two rebase onto; then the Animation System (Roblox-style Animator, AnimationTrack and KeyframeSequence classes, the avatar walking through a default Animate script); Eustress Vehicle Simulator's scripted-constraint classes and the Rune `dm::get_instance` and `dm::set_instance`; then Roblox Place Import's hidden anchor for nested parts; then the joint building on attachment frames.

### Build queue

One at a time, after any build already running.

| # | Command | From | Proves |
|---|---|---|---|
| 1 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-networking` | Multiplayer | Protocol 3: the world, motion, input, and remote lanes, and late-join catch-up |
| 2 | `cargo build --manifest-path eustress/Cargo.toml -p eustress-engine --bin eustress-engine -p eustress-client --bin eustress-client` | Multiplayer, Client | Studio and the Player on protocol 3, with the Player's Part shapes |
| 3 | **M0.** In Studio, open a Space that gives players a character and press F9, which enters Play itself. Then run `eustress/target/debug/eustress-client --connect 127.0.0.1:7777` | Multiplayer | Both avatars move in both windows, and Studio's log shows `multiplayer: catching peer 1 up (N ops)` |
| 3b | **Pong's paddles.** Stop, open Pong, press F9, and join the same way | Multiplayer, Box Head | W and S in the Player's window move that player's paddle in Studio's window. Pong has no avatars (`character_auto_loads = false`). The Player's own window shows no script changes until the reader and apply step land |
| 4 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-common --lib space_read` | Client | The Player's shape table, wedge meshes, materials, and transform cleaning |
| 5 | In `eustress/crates/web`: `trunk build --release`, then `cargo run --no-default-features --features ssr --bin prerender -- --strict` | Website | The site builds with the Player download pages |
| 6 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-common --lib tree_read`, then the same with `--lib datamodel::record` | Eustress WASM | The Player's reader and the shared record-to-property conversion |
| 7 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-engine --lib seed::parity` | Eustress WASM | Studio's seed and the Player's reader build the same tree from the same records |
| 8 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-common --features physics --lib -- space_read tree_apply tree_read datamodel::record` | Client | The Player's reader, apply step, and shape drawing |
| 9 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-roblox-import`, then `-p eustress-common value_object_rewrite` and `-p eustress-engine the_space_toml_is_published_as_the_database_holds_it`, plus `-p eustress-engine authored_transform_tests` and `-p eustress-engine save_space_writes_each_file_in_its_own_unit`, which fails on the pre-fix code | Roblox Place Import | The importer (pose-relative nested parts, animation import, decoders) and the value-object script rewrite |
| 10 | `cargo test --manifest-path eustress/Cargo.toml -p eustress-play-runtime` | Client | The shared Play runtime crate |
| 11 | Terrain's five: `-p eustress-common --features physics --lib terrain:: -- --test-threads=1`, `-p eustress-common --features luau luau::play::terrain`, `-p eustress-engine --lib soul::rune_terrain`, `-p eustress-mcp-server`, `-p eustress-tools capability` | Terrain | The terrain overhaul |
| 12 | `EUSTRESS_MIGRATION_SAMPLE="C:\Users\miksu\Documents\Eustress\Summit Studios\Spaces\Vehicle Simulator 2" cargo test --manifest-path eustress/Cargo.toml -p eustress-engine a_real_space_moves_without_moving_anything -- --ignored --nocapture`, then the same with `Business & Ops\Spaces\Auto Team` and with `Business & Ops\Spaces\Super Station Curated` | Roblox Place Import | Migrate-on-open on COPIES of three real Spaces: Vehicle Simulator 2 (a full import, 338,027 files, several minutes), Auto Team (a small import whose posed children are Models and Folders) and Super Station Curated (no import report, with real parts nested in parts, so every nested part keeps what was drawn), plus `Mobility\Spaces\Vehicles Center` (an import from an older importer that wrote no import report: 2,394 Attachments under parts, which are never rewritten, and 492 posed Models and Folders; expect 0 at their Roblox pose). Copies keep file times (robocopy `/COPY:DAT /DCOPY:T`), because the untouched-since-import rule compares each file's time with the import report's. In each, every part stays within 1e-3 of its composed pose, and the source is never written. Migrate-on-open stays opt-in (`EUSTRESS_MIGRATE_POSE=1`) until this passes on a built binary |
| 13 | The two-window playtest in [docs/networking/PLAYTEST_SA2A.md](../networking/PLAYTEST_SA2A.md), with Studio and the Player both in light mode (`EUSTRESS_CLOUDS=off EUSTRESS_SKY=gradient`), run from copies of the binaries on a copy of the Universe | Multiplayer | Server-authority phase 2 by hand: players and characters on both sides, the Player's HUD from the host's PlayerGui, animation both ways and for a late joiner, and seats in Vehicle Simulator (sit, drive, handbrake, leave). Each check names the log line that proves it |

The engine `--lib` tests (7, 9, 11 and 12) compile the engine in test mode, about as long as a full build, so they run together in one engine test build.

---

## Status

| # | State | Updated |
|---|---|---|
| M0 | **Build 5, 2026-09-25 02:51.** Studio's library and the Player built with 0 errors (23:25 to 02:10; the settled check, including Slint regenerated from the live files, and the access scan were clean on the frozen tree). Cargo then could not launch the rustc for Studio's `main.rs` (exit 0xc0000142, DLL initialization failed) when the desktop app closed around 02:10, so that one step ran by hand with cargo's own arguments against build 5's library, into the orchestrator's scratchpad (exit 0, 99 s). Studio passed a 45 s launch check on a copy of Pong in light mode (no panic, 60 to 90 fps). Contents: avatar seats on host and Player, VehicleSeat input and scripted joints, TextBox focus, the hidden anchor and opt-in migrate-on-open, the unit-save fix, and the build 4 Slint fix. The two-window test on a fresh copy of the Games Universe (`m0test/Games3`) follows `docs/networking/PLAYTEST_SA2A.md`. Earlier: **Stability build, 2026-09-24 19:44** (built 18:41). At full quality, Studio hit DeviceLost the moment the Player joined, and the Player alone used 99% of the GTX 1080 Ti. With `EUSTRESS_CLOUDS=off` and `EUSTRESS_SKY=gradient` on both, they ran together at 66% and 5.5 GB. The joins were clean: `running eustress-client 0.1.0`, `catching peer 1 up (235 ops)`. F9 needed no save and served in 0.16 s. There was no IK crash and no guard warning, and McKale stopped the session at 19:50. **McKale's verdict: the Player is nowhere near Studio's Play for what Box Head makes.** It runs the Space's LocalScripts (`common/src/tree_scripts.rs`), but Studio's Play draws everything else from engine-only code: `engine/src/play_datamodel/` (apply, camera, audio, particles and npc, 5,647 lines). It is tied to the engine by 7 `use` imports and more inline paths: `spawn::{MeshSource, part_type_to_glb_path}`, `play_mode::PlayModePhysicsActivated`, `commands::PropertyCommand`, `space::space_asset_source::space_asset_root`, `rendering::PartEntity`, `ui::ViewportBounds`, `particles::render`, the Slint OutputConsole and the profiler (Client's count). In Studio the Space's parts already exist as loader entities that `seed.rs` binds into the tree, so `apply.rs` spawns only script-made instances; on the Player every part arrives as a spawn, so the spawn path becomes the main path there, the ScreenGui and BillboardGui spawners, `billboard_gui.rs` (3,576 lines) and `gui_loader.rs`. The Player has only `tree_apply.rs` (636 lines, parts and camera). Proposed: one shared Play runtime that both run, pending McKale's decision. Earlier, **joined live 2026-09-24 15:40** (the 15:38 build, run from copies on a copy of the Games Universe; Box Head's Space). Studio hosted through the bridge's `StartServer` (the F9 action): 4 chunks, 2.56 MB, on 127.0.0.1:7777. The Player joined with the full link in about 2 s: welcomed as peer 1, downloaded 100%, opened 696 parts, bound 859 scene instances through the tree reader, and built both avatars. Studio logged `catching peer 1 up (269 ops)` and "miksu joined the session." Two crashes followed. The Player panicked at 15:42:54 on `origin.is_finite()` in obvhs from `eustress_common::avatar::ik::solve_limb_ik`, a non-finite ray origin (Client guards it; Multiplayer checks the motion lane for a NaN source). Studio hit a GPU DeviceLost at 15:47:36 with both apps rendering on the one GTX 1080 Ti. Avatar movement is not yet checked by hand. Earlier: **Built 2026-09-24** (`eustress/target/debug/eustress-engine.exe` and `eustress-client.exe`; the engine and client build ran from 09:07 to 11:18 with 0 errors). `cargo test -p eustress-networking` failed in eustress-common: `avatar/procedural.rs:101` and `avatar/spawn.rs:143` use `avatar::anim`, which is gated behind `model-import`, and networking builds common with `physics` only. The bug is from July, surfaced by networking's lighter dependency; Multiplayer's gate fix is saved, and a re-run follows. **Studio panics about a second after launch** (run on a copy of the Games Universe): Bevy B0002, because `drain_slint_actions` gets `Option<ResMut<DisplayUnit>>` through `DrainResources` (`ui/slint_ui.rs:4360`) and `Option<Res<DisplayUnit>>` through `light_panel::EditQueries` (`ui/light_panel.rs:353`, reached via `DrainActionQueries` at `slint_ui.rs:4841`). Lights is fixing it; an engine rebuild follows, then the two-window test. The code (Multiplayer): protocol 3 (a new tick in Welcome, tagged datagrams, and the world, motion, input, and remote lanes), the replication core with late-join catch-up, the host glue (`engine/src/net_replicate.rs`), host-side `Player:IsKeyDown`, and the `/probe` echo. Both apps must be rebuilt; a protocol 2 Player is refused with a message. Commands and test steps are in the build queue | 2026-09-24 |
| M1 | **Pong plays in single-player Studio** (Box Head session; `Documents\Eustress\Games\Spaces\Pong`), with no avatar from the next build on (`Players/_service.toml`: `character_auto_loads = false`; a joining Player gets no body either, since `client/src/systems/net_play.rs` reads the same setting, and F9 then starts Play with no body on the host; not built): all logic in `ServerScriptService/PongServer`, an unclaimed paddle plays itself, the score shown as seven-segment digits of Parts. A remote player needs only: its keys on the host; replicated CFrame on `Ball`, `Paddle1`, `Paddle2`; replicated Transparency on 14 score segments. **Input is final.** PongServer's `keysOf(player)` calls `player:IsKeyDown(Enum.KeyCode.W, Up, S, Down)` when that method exists and falls back to the `InputY` attribute (verified: W, the Down arrow, scoring, and a win at 7). The Player is written to sample and send its keys every tick (`send_local_input`, `eustress-networking/src/session.rs:1554`). The host now answers `Player:IsKeyDown` for class `Player` from each player's newest input (`common/src/luau/play/instance.rs:331`, filled by `pull_player_input` in `engine/src/net_replicate.rs`; not built). Until a build includes it, the fallback runs. Pong needs no edit. **The Player draws Parts by shape** (Client: shapes, materials, and transparency in `common/src/space_read.rs`, joined worlds included; type-checked, not built). Replication applies to `common::datamodel`, which is pure Rust and outside the `luau` feature. **Camera:** Pong declares a top-down orthographic camera in `Workspace/Camera/_instance.toml` (Box Head, added; its Studio check waits for the shared GPU), which the Player's camera honors (Client). Not built: protocol 3 (Multiplayer, code complete). **The Player side is complete, type-checked, not built:** a joined world opens through Eustress WASM's reader (`common/src/tree_read.rs`, with `record_props` and `create_scene`), the tree and `SceneKeys` are inserted, Multiplayer's `net_replica` binds and applies world frames, and Client's apply step (`common/src/tree_apply.rs`) draws them, with the Space's camera (`client/src/systems/space_world.rs`). Local launches keep `space_read`. Next: a build of Studio and the Player together, then the two-window test with Pong | 2026-09-24 |
| M2 | `cloudflared` not installed; probe scripts written | 2026-09-24 |
| M3 | **Code complete on every side.** The Worker side is in production (the 2026-09-24 15:13 UTC upload, gate 9). The listing page was built on 2026-09-25 and verified in a browser against `wrangler dev --local`; the website is not deployed. No publish has ever run, against production or `wrangler dev`: no Universe holds a listing id, production lists 0 simulations, and Studio's Publish targets production only until the `EUSTRESS_API_URL` resolver lands (build 12). **Download** (Website): the site's Player section, `/downloads/player`, and the listing's Play dialog read `player/latest.json` and say "not released yet" until gate 8. **Live registry** (Website, `infrastructure/cloudflare/api/src/live.mjs`): a SQLite Durable Object per hosted simulation, bound as `LIVE_HOSTS` with migration `v3-live-hosts`, which fits the Free and Paid plans. `POST` and `DELETE /api/simulations/{id}/live` take only the author's token and store only a link in the join grammar. Private LAN addresses are kept for Stage 1, and loopback and link-local addresses are refused in every spelling. An unapproved listing shows its link to the author and admins only, and a silent host goes Offline after 90 s (the Worker suite at 148 of 148, plus end-to-end checks on `wrangler dev --local`). **Studio's heartbeat** (Multiplayer, `gallery_heartbeat` in `engine/src/multiplayer.rs`): a `POST` every 30 s and at once when the link changes, a `DELETE` on stop. It lists a LAN session (`EUSTRESS_HOST_LAN=1`) by its local network address, or the address in `EUSTRESS_HOST_PUBLIC`, and never a loopback-only session. **Listing page** (Website, `web/src/pages/experience.rs`): LIVE with the player count, or OFFLINE, polled every 30 s while the tab is visible. Play on a live listing opens the `eustress-player://join/<host>:<port>?key=<32 hex>&pin=<64 hex>` link, and the page opens no other kind (`web/src/api/live.rs`). A "Not opening? Download the Player" panel follows the click; macOS says joining from the Gallery is Windows and Linux only for now, until Multiplayer's Apple Event handler exists; phones and offline listings keep the Player dialog. The Player registers `eustress-player://` in all three installers (Client). **Listing approval:** Studio's Publish opens a review case, and an admin approves it with `moderation_approve` (gate 13). Waiting on the publish rehearsal and the publish authorization matrix on `wrangler dev --local` (after build 12), the staging decision (gate 7), the first Player release (gate 8), and the website deploy | 2026-09-26 |
| M4 | Access not requested | 2026-09-24 |
| M5 | Plan agreed with Eustress WASM: a wasm32 dependency graph (gate eustress-common's C `zstd`), the records-to-entities reader with `Part` coverage, a browser shell (WebGPU plus WebGL2), and a WebTransport transport behind `LinkEnds`. The Luau spike and HTTP world download are off M5, since Pong's scripts run on the host and a joining Player gets the world from the host. About one to two sessions of code after the gate, then three to five build rounds; finishes after M1. Gated on M0 and McKale's go | 2026-09-24 |
| M6 | **Re-imported 2026-09-24** (Roblox Place Import; exit 0 at 12:58 after 80.8 minutes) at `Documents\Eustress\Summit Studios\Spaces\Vehicle Simulator 2`, with the previous copy in `Spaces\.trash\Vehicle Simulator 2-1790275038`. Contents: 338,020 instances (163,449 Parts, 32,904 Models, 12,103 unions, 6,773 Attachments, 7,195 Luau scripts), 6,712 terrain chunks with 0 decode errors, and 443 remote and bindable events. New in this import: attributes where every reader looks, string attributes as text, Model pivots, service files, Roblox material names, and media keys where the loaders read them. Without a Roblox credential, 84,667 assets (meshes, textures, sounds) were not fetched, so 11,863 unions are stand-in blocks; 251 more fall back on a decoder bug fixed after this run, not yet applied. Server-authority phase 2 progress (2026-09-24, type-checked, not built): host characters for joined players, a per-player PlayerGui from StarterGui, animation track replication (the host validates, rate-limits and relays tracks, and late joiners catch up), and Player root binding. Seats are next. A remote player can drive only after Multiplayer's server-authority phase 2: characters for remote players in the host's DataModel, a sit and exit handshake, and the Player's avatar riding its seat. The motion lane now carries fast wheels, up to ±327 rad/s, interpolated along the sampled angular velocity. **McKale's rule for vehicles: no game-specific logic in the engine, only general classes.** The car becomes a Rune script in the Space (SimChassis 4.0), on general physics constraints that Rune and Luau create with `Instance.new` in Play: Hinge, Prismatic, Spring, Cylindrical, NoCollision and Weld constraints, and Attachments, with their Roblox properties (Eustress Vehicle Simulator, after Eustress WASM's attribute and general-branch chain and the hidden-anchor landing). Earlier blockers (Eustress Vehicle Simulator's report): the bake fix and the terrain reader are written, not built; the Player's reader now draws every BasePart class (custom meshes as blocks) and converts a Space's unit to metres as Studio does, so imported Roblox Spaces no longer draw 3.28 times too big (Client; not built); spawn replication for runtime-cloned cars (550 `:Clone()` calls) is not built; the Player already has mlua's Luau but not Studio's Play runtime on a Player tree (gate 10); the host and client-script decisions are open (gates 10, 11); VS has never been driven in Eustress. Imported CollectionService tags are invisible to both trees, because the importer writes them under `[metadata] tags` while both readers read a top-level `tags` (Roblox Place Import is fixing the importer), and Studio's folder loader drops Model attributes (see Risks). Through the tunnel, a car replicates one root transform per welded assembly, not one per part, to stay under the UDP cap | 2026-09-24 |
