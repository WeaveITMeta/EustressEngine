# Browser Player on staging

The Gallery Play dialog's **In your browser** option opens the browser Player
(`/player/host.js` plus the wasm build of `eustress-player-web`) for a published
Space.

- **Alone.** The page passes the Gallery id. The Player downloads
  `GET /api/simulations/{id}/world/manifest` and each chunk from the API the
  page belongs to (staging when the host is `staging.eustress-cbb.pages.dev`),
  checks every chunk's blake3, and opens the world with the shared Play runtime.
  When `Players.CharacterAutoLoads` is on, the local character spawns on the
  Space's SpawnLocation and the avatar runtime follows it: WASD moves, Shift
  sprints, Space jumps, Ctrl crouches, and the right mouse button looks. A
  Space that loads no character shows its own camera, moved only by its
  scripts, exactly as the desktop Player does: the page takes no mouse and
  adds no fly camera (McKale, 2026-11-14: "It needs to be the same mechanic
  as the player").
- **Signed in.** A world refused to a signed-out request (an unlisted or
  rated listing) is asked for again with the site's sign-in. The site stores
  `auth_token` as JSON (`gloo_storage`), quotes included; the Player strips
  them (`solo.rs` `token_from_storage`). Sent with the quotes, the API read
  the request as signed out and answered 403.
- **The mouse.** While the page holds the mouse, the browser keeps Esc for
  itself and the page never sees the key, so the Player watches the
  document's pointer lock (`mouse.rs`, `mouse_lock.rs`). When the browser
  lets go, the cursor comes back and a first-person view steps out, as Esc
  does on the desktop Player. A lock the browser refuses (zooming into first
  person with the wheel is not a click) is given up after a second, and the
  next click takes the mouse. In third person the mouse is held while the
  right button turns the camera, so the turn does not stop at the screen's
  edge, and let go when the button comes up.
- **A live session.** The page passes the same `eustress-player://join/...` link
  the desktop Player gets, ticket included, in memory only. The Player opens it
  over the browser's WebTransport, pinned to the host certificate. The character
  stands where the host placed it.

The browser build runs the same avatar runtime and the same scripts as the
desktop Player:

- **Played alone** it is the session's Authority (`PlayRole::Authority`): the
  Luau VM (common `tree_scripts`, the desktop Player's mlua binding layer) runs
  the Space's Scripts and LocalScripts, physics is live, and Rune runs every
  SoulScript (`common::soul::rune_play`, `RuneScope::Everything`).
- **Joined** it is a Replica: the host runs the Scripts, and this Player runs
  its LocalScripts and its own Rune (`RuneScope::ClientOnly`).
- HttpService's GetAsync, PostAsync and RequestAsync refuse in the browser
  with a script error; the rest of HttpService (JSON, GUIDs, URL encoding)
  works.

### How Luau builds for the browser

mlua's Luau is C++ (`mlua-sys`, `luau0-src`). clang has no C or C++ standard
library of its own for `wasm32-unknown-unknown`, so the VM compiles against
**wasi-sysroot 34** (libc, libc++, libc++abi, libunwind from
[WebAssembly/wasi-sdk](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34)),
with C++ exceptions as WebAssembly exceptions, because Luau raises script
errors as C++ exceptions.

- `eustress/scripts/wasm-luau-env.sh` sets the wasm32 C and C++ flags. Source it
  before any wasm32 build of `eustress-player-web` (the staging script does).
  Native builds are untouched.
- The sysroot lives at `.tools/wasi-sysroot-34.0` at the repository root
  (git-ignored), or wherever `WASI_SYSROOT` points. Only
  `include/wasm32-wasip1` and `lib/wasm32-wasip1` are needed. clang 22 or later
  is required (this machine: LLVM 23 in `C:\Program Files\LLVM`).
- `crates/player-web/build.rs` links libc++, libc++abi, libunwind and libc
  into the module. `crates/player-web/src/libc_shim.rs` defines the few calls
  wasi-libc leaves to a WASI host (clocks, environment, standard streams), so
  the module imports nothing a web page lacks.
- The module uses WebAssembly exception handling (the standard `exnref`
  instructions): Chrome and Edge 137+, Firefox 131+, Safari 18.4+. An older
  browser cannot compile the module, and the page shows "The browser Player
  could not load."

## Hosted join: what blocks it

Checked 2026-10-20 against staging.

1. **Nothing is hosting Box Head.**
   `GET https://api-staging.eustress.dev/api/simulations/07d084d8-9098-4192-bb48-84d2c428db36/live`
   returns `{"live":false}`. There is no join link to open.
2. **A public Cloudflare Tunnel hostname does not carry this traffic.**
   WebTransport is QUIC, which is UDP. A tunnel's public hostname carries HTTP,
   HTTPS, WebSocket, and TCP. It does not carry UDP. That is the same limit as
   [GOAL_LIVE_GALLERY_PLAY.md](GOAL_LIVE_GALLERY_PLAY.md) (Cloudflare's docs,
   2026-09-24). A browser on `staging.eustress-cbb.pages.dev` cannot reach a
   workstation host through that public hostname. A private address in the join
   link (`192.168.x.x`, `10.x`, `127.0.0.1`) is not reachable from the public
   internet, and Chrome's Local Network Access permission can block a public
   origin from opening one even on the same LAN. Stage 1 in that goal (a private
   network route plus the Cloudflare One client) does not apply to a public
   page. Stage 2 (Spectrum private origins) is not available on this account.
3. **The browser requires the certificate pin.** `serverCertificateHashes` is
   the only way a browser accepts the host's self-signed certificate. A link
   with no `pin` is refused (`unpinned-link`). The desktop Player can still open
   a loopback host without one.
4. **The host has to be reachable as HTTPS on its port, with datagrams.** The
   native host advertises the pin and requires datagrams (`eustress-networking`
   `native.rs`, `js/net.js` sets `requireUnreliable`). This browser build has
   not completed a handshake against a live host: staging has no session, and
   this environment has no workstation host to point at.
5. **The site's Content-Security-Policy is Report-Only.** It does not block the
   connection today. If it is enforced, `connect-src` must allow the host
   origin, including its port. Chromium checks WebTransport against `connect-src`.

Until a host is reachable on public HTTPS/QUIC with the pin in the join link,
Join in the browser will stop at "Could not join the session." The code path
is the one above; the network path is the blocker.

## What was run (2026-10-20, character)

Headless Chrome, WebGL2, SwiftShader, against
`https://api-staging.eustress.dev`, Box Head
`07d084d8-9098-4192-bb48-84d2c428db36`.

- The world downloaded (932 files) and opened (859 instances). The page reached
  "Playing."
- The local character was placed on the Space's SpawnLocation at
  `(11.0, 0.3, 11.0)`, using the Space's own body
  `space://StarterPlayer/Characters/BoxheadMasculine.glb`, masculine, height
  2.05 m. The avatar runtime logged the spawn.
- Walking was not observed. On the same frame the body spawned, Bevy quit:
  the volumetric lighting pipeline needs a fragment storage buffer, and this
  WebGL2 device reports a limit of 0. A desktop WebGPU browser, which is the
  one that already drew this map, does not take that path. This machine's
  WebGPU device is lost on startup, so movement was not retried there.
- Luau did not build then: with no C++ standard library for
  `wasm32-unknown-unknown`, `mlua-sys` failed on `<iterator>`. It builds
  against wasi-sysroot since 2026-11-13 (see "How Luau builds for the
  browser" and the run below).

## Staging deploy (2026-11-14)

The Luau build is on staging: deployment `2d073be2`, alias
`https://staging.eustress-cbb.pages.dev`. `/player/webgpu/` serves the 83.0 MiB
module as 5 parts, byte-identical to the build (SHA-256 checked after
download). From the staging origin, `host.js` joined the parts, the module
compiled and started, and the Player requested Box Head's world, which the
API refused with 403 (the listing needs sign-in). No public Space is listed
on staging, so no script has run there yet. WebGL2 was not built.

## Production deploy (2026-11-14, mouse and sign-in)

McKale: "Get it on production, I want browser to be stable." Pages deployment
`05d020cf` (branch `main`, eustress.dev), after staging `5345e611`.

- The upload was the site production already served (every page checked was
  byte-identical to `crates/web/dist`), with only `/player` changed and no
  `.well-known/assetlinks.json`. No other web change went with it.
- `/player/host.js` names the new build's folder,
  `webgpu/3b18266227074573/`. The previous build stays at `webgpu/` for
  pages that cached the older `host.js`.
- On eustress.dev the Player played The Super Station: loading, starting,
  loading-world, in-world, and drew the station. A click that the browser
  refused the mouse for (the pane was in the background) hid the cursor and
  the Player gave it back about a second later. The previous build, run the
  same way, kept the cursor hidden.
- Not verified in a browser: Esc after a granted lock and the right-drag
  hold (the test pane cannot hold the pointer lock; `mouse_lock.rs` tests
  cover the decision), and the sign-in retry (no signed-in browser here;
  `solo.rs` tests cover the token).

### Production deploy (2026-11-14, no fly camera)

McKale tried `05d020cf`: "Esc works, but the mouse is taken like it's first
person and looks around wrongly. It needs to be the same mechanic as the
player." The browser build had its own click-to-look fly camera
(`explore.rs`, from before scripts ran in the browser); the desktop Player
has none and lets the Space's scripts move the camera. It was removed.
Pages `ce6e2db6` (after staging `8e0e872e`), build folder
`webgpu/5f734e695b52d3c1/`, built from a source snapshot on F: because
another session's `common/src/page_spec.rs` test module did not parse (the
snapshot dropped only that test module; tests are not in a release build).

On eustress.dev the new build played The Super Station, and a click on the
world made no pointer-lock request (the cursor stayed). A tab still holding
the 3:45 PM `host.js` loaded the `3b18266227074573` build whole and played:
the cache case works. Locally the Station's own Top button took the camera
straight down with nothing fighting it.

### Pong drew black (2026-11-14 evening)

McKale published Pong (`9a6951e9-09da-40b7-9d23-9a6ce1cd9c08`): the browser
Player ran it (the scripts scored) but drew only black. Pong's camera is
orthographic and looks straight down. Packed variants of the same Space,
served to the Player locally, isolated it:

| variant | drawn |
|---|---|
| as published (orthographic, straight down) | black |
| perspective, straight down | yes |
| orthographic, tilted 0.5° or 0.001 rad | yes |
| orthographic, tilted 0.0001 rad | black |

0.0001 rad still rounds to exactly vertical in f32. Skipping single GPU passes
from the page (a test hook that turns a labelled pass into a no-op) showed the
sky composite is fine and the standard-material opaque pass writes black: in
an orthographic view looking straight down every level surface is seen
exactly face-on (view direction equal to the normal on every pixel), and a
shading term turns that into NaN on WebGPU. The fix is in the Play camera
step (`play-runtime/src/camera.rs` `off_vertical`): an orthographic Play
camera within 0.001 rad of vertical renders tilted exactly 0.001 rad toward
the top of the screen, roll kept (6 cm across 60 m; picking uses the same
transform). Two related changes went with it: the orthographic sky ray is
kept half a degree off vertical (`orthographic_sky.rs`, Bevy's
`fast_atan2(0, 0)` is 0/0), and orthographic sky cameras get no volumetric
god-ray fog (`sky_atmosphere.rs`, Bevy's volumetric fog marches perspective
rays). Neither alone fixed Pong; the camera tilt did. Staging `b1e0204d`,
build folder `webgpu/fb1dd3e34133001a/`. Production `2da7ddee` (McKale:
"Yes, deploy"): the published Pong played on eustress.dev, drawn straight
down, in a visible tab.

### Why each build has its own folder

eustress.dev serves `/player/host.js` and the glue `eustress_player.js` with
`Cache-Control: public, max-age=14400` (the zone's browser cache TTL), while
`host.js` fetches the wasm parts with `no-cache`. A glue file runs only the
wasm it was generated with, so replacing both in place breaks every browser
that loaded the Player in the last four hours: its cached glue meets the new
wasm and the page shows "The browser Player could not load."

`package_player.mjs` therefore writes each build to a folder named by its
glue and wasm (`webgpu/<16 hex>/`) and rewrites `host.js`'s `BUILD_DIRS` line
to name it. `--keep <dir>` copies in the builds the previous package's
`host.js` names, so a cached `host.js` still finds a whole build. The staging
script passes `--keep crates/web/dist/player` when it exists: keep that folder
equal to what the site serves after every deploy.

### Frame time: no `trace` on bevy_ecs (2026-11-15)

The browser build named `bevy_ecs` with its `trace` feature. In a browser,
bevy_log's tracing layer turns every span into a `performance.mark` and a
`performance.measure`, and `trace` wraps every system, run condition and
schedule in a span. Pong made about 2,600 of those calls into JavaScript each
frame, and the page's timing buffer grew without limit (3.9 million entries
after 25 s). `crates/player-web/Cargo.toml` now names `bevy_ecs` with no
features and says why.

Measured in Chrome on WebGPU, window visible, 600 frames, work inside the
frame callback:

| Space | Before | After |
|---|---|---|
| Pong (local world) | 23.4 ms mean, 35.1 ms p99, 64 ms worst | 8.2 ms mean, 10.4 ms p99, 11 ms worst |
| The Super Station (production world) | 71.6 ms mean, 357 ms p99 | 16.1 ms mean, 23.6 ms p99 |

A profiling build (`trace` and `debug` on, never shipped) showed no single
system above 1 ms a frame on Pong after that. Measure with the window visible:
a hidden or covered page gets frames seconds apart and proves nothing.

Production `0be18348` (2026-11-15, McKale: "Yes, deploy"), build folder
`webgpu/c874dcb34a219144/`. The upload was `crates/web/dist` (byte-identical
to what eustress.dev served, apart from Cloudflare's own email obfuscation on
ten pages) with only `/player` changed; the three earlier builds stay. On
eustress.dev the published Pong played from the play dialog on that build:
10.7 ms of work a frame, 2,861 frames in about 45 s.

mlua 0.12.2 does not compile for `wasm32-unknown-unknown` (its heap dump calls
C stdio the `libc` crate lacks there). The workspace patches it from
`.patches/mlua-0.12.2`, which refuses `Lua::heap_dump` on that target only.

## What was run (2026-11-13, scripts)

WebGPU release build with `mlua` and `realism-scripting`, packaged by
`package_player.mjs` (82.3 MiB of wasm after wasm-bindgen), served from
`http://localhost:3000` in Chrome 156 (NVIDIA, WebGPU).

- Spike first: a standalone crate on mlua 0.10.5 with Luau, built for
  `wasm32-unknown-unknown` against wasi-sysroot 34 and run through
  wasm-bindgen 0.2.128. Six checks passed (compound assignment and
  if-expressions, `error()` caught by `pcall`, a Rust callback's error caught
  by `pcall` with its message, a syntax error returned as `Err`, an uncaught
  runtime error returned as `Err`, coroutines). Flipping one expectation
  dropped the result from 63 to 61, so the harness can fail.
- Pong, packed from its Space folder into a manifest and a chunk with
  `eustress-echk`'s encoder, and served to the Player in place of the API's
  world download (staging's only listing needs sign-in, and staging lists no
  public Space). The Player played it alone as the Authority:
  `ServerScriptService.PongServer` started, printed "ready: first to 7" and
  "player joined: Player", scored point after point, printed "Right wins 0
  to 7" and "new game", and kept playing. The LocalScript
  `StarterPlayerScripts.PongInput` started. No script error was logged.
- Not verified: the picture. The app window was closed, so the browser pane
  was hidden; a hidden page gets no animation frames, the test page drove
  frames itself, and its canvas could not be captured. Keyboard input to the
  paddle was not tried. No Space with Rune scripts was run (the four Player
  Rune modules registered).
- Fixed on the way: the module's static constructors never ran (nothing
  called `__wasm_call_ctors`), so the first Luau state trapped dividing by
  zero in its allocator; `start_player` now runs them once. That also runs
  eustress-networking's `ctor` registrations, which had never run in the
  browser.

## What was run earlier (2026-10-20)

Both release builds compile (`wasm32-unknown-unknown`, `webgpu` and `webgl2`).
Against `https://api-staging.eustress.dev`, the WebGL2 build downloaded Box
Head Zombie Defense Tycoon (`07d084d8-9098-4192-bb48-84d2c428db36`), checked
the chunks, and opened the tree: 859 instances, and the page reached
"Playing."

That run was headless Chrome with no GPU. The WebGPU device was lost
immediately (`A valid external Instance reference no longer exists`). The
WebGL2 device then quit on a validation error: Bevy's atmosphere pipeline
needs a fragment storage buffer, and this WebGL2 implementation reports
`max_storage_buffers_per_shader_stage` of 0. A desktop browser with WebGPU
is the path that can draw that pipeline. This environment never got a stable
picture or a fly-camera frame, and it never saw a live host.

## Staging deploy

Staging only. The website is Pages project `eustress`, branch `staging`
(`staging.eustress-cbb.pages.dev`). Production is the same project, branch
`main`. The script refuses `main` and any other project.

From the repository root, with `wasm-bindgen` 0.2.128 on `PATH` (the version
`eustress/Cargo.lock` pins):

```bash
eustress/scripts/browser-player-staging.sh
```

That builds the WebGPU Player, packages `/player`, builds the site, and copies
the Player into `eustress/crates/web/dist/player`. It also copies
`eustress/crates/web/staging-only/.well-known/assetlinks.json` (the debug
Android Player's staging passkey link) into `dist/.well-known/`, so that
`dist` is staging's only. It does not deploy.

WebGL2 (browsers without WebGPU) is a second build:

```bash
eustress/scripts/browser-player-staging.sh --webgl2
```

Deploy the result to staging, and nowhere else:

```bash
eustress/scripts/browser-player-staging.sh --deploy
```

which runs, from `eustress/crates/web`:

```bash
npx wrangler pages deploy dist --project-name eustress --branch staging --commit-dirty=true
```

Do not pass `--branch main`. Do not deploy the API Worker from this script.

A single Pages file over 25 MiB is rejected by Cloudflare. The WebGPU release
build is 82.3 MiB after `wasm-bindgen` with the Luau VM and Rune (measured
2026-11-13; 71 MiB before them). The WebGL2 build was about 73 MiB on
2026-10-20, before them. The script does not run `wasm-opt`: that rewrite drops
the module layout the generated glue calls. A wasm over the cap is removed
from the upload and replaced with 20 MiB pieces,
`eustress_player_bg.wasm.part0`, `part1`, and so on. `/player/host.js` fetches
those pieces and joins them before it starts the module. A package that still
has the single `.wasm` uses that file instead, so a local server does not
need the split. If any file is still over 25 MiB the script stops; that whole
`/player/` folder can be served from R2 with `Content-Type: application/wasm`
on the wasm, because `host.js` loads the builds from the directory it sits in.

`package_player.mjs` copies the part meshes (`assets/parts`, the block,
cylinder, ball, and the other primitives a Part draws) next to the builds.
`--materials` also copies the texture library, about 136 MiB. Without it, a
missing texture draws as a flat color.

The API the Player calls is chosen by the site host (`state.rs`): the staging
site uses `https://api-staging.eustress.dev`. No build flag points a deploy at
production.
