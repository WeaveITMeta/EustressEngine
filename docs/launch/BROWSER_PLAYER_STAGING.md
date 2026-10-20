# Browser Player on staging

The Gallery Play dialog's **In your browser** option opens the browser Player
(`/player/host.js` plus the wasm build of `eustress-player-web`) for a published
Space.

- **Alone.** The page passes the Gallery id. The Player downloads
  `GET /api/simulations/{id}/world/manifest` and each chunk from the API the
  page belongs to (staging when the host is `staging.eustress-cbb.pages.dev`),
  checks every chunk's blake3, and opens the world with the shared Play runtime.
  Click locks the mouse. WASD moves, Shift is faster, Space and Ctrl go up and
  down, Esc releases the mouse.
- **A live session.** The page passes the same `eustress-player://join/...` link
  the desktop Player gets, ticket included, in memory only. The Player opens it
  over the browser's WebTransport, pinned to the host certificate.

The browser build does not run Luau (mlua is C++) or the avatar runtime. A
Space is drawn, and the camera can be flown. Characters, zombies, and tycoon
scripts run on a host; a joined browser shows what that host replicates.

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

## What was run (2026-10-20)

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
the Player into `eustress/crates/web/dist/player`. It does not deploy.

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

A single Pages file over 25 MiB is rejected by Cloudflare. The release build
is about 71 MiB for WebGPU and about 73 MiB for WebGL2 (measured 2026-10-20,
after `wasm-bindgen`). The script does not run `wasm-opt`: that rewrite drops
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
