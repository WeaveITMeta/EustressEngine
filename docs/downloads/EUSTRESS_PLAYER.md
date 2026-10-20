# Eustress Player

Eustress Player plays Spaces published to the Gallery and joins live sessions that Eustress Studio hosts. It is in Public Alpha.

## Getting it

Download it from [eustress.dev/downloads/player](https://eustress.dev/downloads/player). No account is needed; Studio's downloads need a free account. When the page shows no release, none has shipped yet.

Each release is also served directly:

| Path on `https://downloads.eustress.dev` | What it is |
|---|---|
| `/player/latest.json` | The current release: version, date, and each file's address, size and SHA-256 |
| `/player/latest/windows-x64-installer` | The current Windows installer |
| `/player/latest/windows-x64` | The current Windows zip |
| `/player/latest/macos-arm64` | The current macOS disk image |
| `/player/latest/linux-x64` | The current Linux archive |
| `/player/vX.Y.Z/<file>` | A given release's files, with `checksums.txt` |

## Publishing a build to staging

A Player build can be published where the staging website can download it, without replacing the public `player/latest.json` and without a GitHub Release.

On GitHub: Actions → **Player Release** → **Run workflow**. The workflow file is [`.github/workflows/player-release.yml`](../../.github/workflows/player-release.yml).

| Input | Staging | Production |
|---|---|---|
| `version` | `X.Y.Z`, the same as `version` in `eustress/crates/client/Cargo.toml`, without a leading `player-v` | the same |
| `channel` | `staging` (the default) | `production` |
| `publish` | on, to upload. Off builds the installers and keeps them as run artifacts | the same |

`channel=staging` uploads into the `eustress-downloads` bucket under the prefix `staging/player`. The manifest's `channel` field is `staging`. The verify step then fetches that channel:

| Path on `https://downloads.eustress.dev` | What it is |
|---|---|
| `/staging/player/latest.json` | The staging manifest |
| `/staging/player/latest/<platform>` | Redirects to the current staging file |
| `/staging/player/vX.Y.Z/<file>` | That staging build's files, with `checksums.txt` |

`<platform>` is a key of the manifest: `windows-x64-installer`, `windows-x64`, `macos-arm64` or `linux-x64`.

A push of a `player-vX.Y.Z` tag is always production: it writes `player/` and creates the GitHub Release. A manual run with `channel=production` writes those same `player/` paths and does not create a GitHub Release, because a dispatched run has no tag to attach one to. Anything else that is dispatched, including a run that leaves `channel` at its default, writes `staging/player` only.

The staging website (`staging.eustress-cbb.pages.dev`, which talks to `api-staging.eustress.dev`, the API a Player link names with `?api=staging`) reads `/staging/player/latest.json`. [eustress.dev/downloads/player](https://eustress.dev/downloads/player) keeps reading `/player/latest.json`.

Those staging paths answer only after the download Worker that serves them is deployed. The routes are in `infrastructure/cloudflare/download-worker.js`. This workflow does not deploy the Worker.

If the upload fails with HTTP 403, `CLOUDFLARE_API_TOKEN` needs **R2 Object Read & Write** on the `eustress-downloads` bucket. The upload step says so in the log.

## Computers it runs on

| Computer | File | Installs to | Opens `eustress-player://` links |
|---|---|---|---|
| Windows 10 or later, x64 | `EustressPlayer-Setup.exe`, or `eustress-player-vX.Y.Z-windows-x64.zip` | Your account only, with no administrator prompt: `%LOCALAPPDATA%\Programs\Eustress Player` | Yes |
| macOS 11 or later, Apple Silicon | `eustress-player-vX.Y.Z-macos-arm64.dmg` | Applications | Not yet: macOS hands a link to an app as an Apple Event, which the Player does not read yet |
| Linux x64 | `eustress-player-vX.Y.Z-linux-x64.tar.gz` | `~/.local/share/eustress-player` | Yes, through its desktop entry |

The Linux build is made on Ubuntu 22.04, so it needs glibc 2.35 or newer. Playing in a browser is not available yet.

The Player draws 3D with your graphics card: DirectX 12 or Vulkan on Windows, Vulkan on Linux, Metal on macOS. Minimum memory and graphics requirements have not been measured yet.

## Installing

**Windows.** Run `EustressPlayer-Setup.exe`. While the builds are unsigned, SmartScreen may stop it: choose More info, then Run anyway. Setup can start the Player when it finishes.

**macOS.** Open the disk image and drag Eustress Player to Applications. While the app is not signed or notarized, macOS blocks the first open: in System Settings, open Privacy & Security and choose Open Anyway.

**Linux.** Unpack the archive and run its installer:

```bash
tar xzf eustress-player-vX.Y.Z-linux-x64.tar.gz && ./eustress-player/install.sh
```

It installs the Player and its assets to `~/.local/share/eustress-player`, puts a link at `~/.local/bin/eustress-client`, adds the desktop entry and icons, and makes the Player the handler for `eustress-player://` links (`xdg-mime`). It then names any system library the Player needs that is missing, with its Debian and Ubuntu package:

| Needed by | Libraries |
|---|---|
| Every desktop | `libasound.so.2` (without it the Player does not start), `libxkbcommon.so.0`, `libvulkan.so.1` and a Vulkan driver |
| An X11 desktop | `libxkbcommon-x11.so.0`, `libX11.so.6`, `libX11-xcb.so.1`, `libXcursor.so.1`, `libXi.so.6`, `libXrandr.so.2` |
| A Wayland desktop | `libwayland-client.so.0`, `libwayland-cursor.so.0` |

The Player shows its error messages in a window through `zenity`; without it they go to the log and the terminal only.

**Checking a download.** Compare its SHA-256 with the release's `checksums.txt`:

```powershell
Get-FileHash .\EustressPlayer-Setup.exe -Algorithm SHA256
```

```bash
sha256sum eustress-player-vX.Y.Z-linux-x64.tar.gz
```

## Removing it

- **Windows:** Settings, Apps, Installed apps, Eustress Player, Uninstall. This also removes its `eustress-player://` registration.
- **macOS:** drag Eustress Player from Applications to the Trash.
- **Linux:** run the installer from the release you unpacked with `--uninstall`: `./eustress-player/install.sh --uninstall`.

## Playing

From the Gallery, press Play. The browser asks to open Eustress Player, and hands it one of two links:

- `eustress-player://play/<simulation id>` downloads the published world and plays it alone.
- `eustress-player://join/<host>:<port>?key=...&pin=...` joins a live session. When you are signed in, the Gallery adds a ticket that lets you in once. If the join fails, press Play in the Gallery again for a new one.

Started on its own, the Player opens the Space in `EUSTRESS_SPACE` when that is set; otherwise the movement test course when your Eustress folder has one (`Movement/Spaces/Climbing`); otherwise a built-in demo scene.

## Command line

On Windows the installed Player is `%LOCALAPPDATA%\Programs\Eustress Player\eustress-client.exe`; on macOS, `/Applications/Eustress Player.app/Contents/MacOS/eustress-client`; on Linux the installer puts `eustress-client` on `~/.local/bin`.

| Command | What it does |
|---|---|
| `eustress-client` | Open the local Space, as described under Playing |
| `eustress-client <scene file>` | Open a legacy scene file |
| `eustress-client --connect <host:port>` | Join a Studio host. For a host on this computer the key and pin are found automatically |
| `  --key <join key>` `--pin <64 hex>` `--name <display name>` | With `--connect`: the join key, the host's certificate pin, and your display name |
| `eustress-client "eustress-player://join/<host:port>?key=...&pin=..."` | Join a host by its link |
| `eustress-client --sim <simulation id>` | Play a published simulation |
| `eustress-client "eustress-player://play/<simulation id>"` | The same, by its link |
| `eustress-client --api-url <url>` | Use this Eustress server, for testing (see below) |
| `eustress-client --version` | Print the version and exit |

Links written with `eustress://` in place of `eustress-player://` open the same way.

| Environment variable | What it does |
|---|---|
| `EUSTRESS_API_URL` | The Eustress server, as `--api-url` sets it |
| `EUSTRESS_SPACE` | An absolute path to a Space folder to open when the Player starts with no link |
| `EUSTRESS_WORKSPACE` | Your Eustress folder; the default is `Documents/Eustress` in your home folder |
| `EUSTRESS_PLAYER_NAME` | Your display name when joining; otherwise your computer's user name |
| `EUSTRESS_PLAYER_REGISTER` | `0` stops this copy of the Player from registering itself for `eustress-player://` links (Windows) |

## Which Player a link opens (Windows)

Setup registers the installed Player for `eustress-player://` links, for your account. The Player also registers itself, for your account, each time it starts. So the Player you started last is the one a link opens. A copy you run for testing takes the links over unless you start it with `EUSTRESS_PLAYER_REGISTER=0`.

## Production and staging

A Gallery link names the Eustress server it belongs to (`api=production` or `api=staging`), and a Player opens only links for its own server. A link for the other server shows "This link is for the staging server; this Player is set up for the production server." and nothing is downloaded or joined.

To test against staging on Windows, start the installed Player once with the staging server:

```powershell
& "$env:LOCALAPPDATA\Programs\Eustress Player\eustress-client.exe" --api-url https://api-staging.eustress.dev
```

It registers itself with that server, so links from the staging Gallery open it. Start the Player normally to return links to production. On Linux the desktop entry always starts the production Player; run it from a terminal with `--api-url` and the link instead.

## Logs and problems

Each run writes `.eustress_engine/logs/player-<process id>.log` in your home folder (on Windows, `%USERPROFILE%\.eustress_engine\logs`). The newest 8 are kept. A crash and a link the Player cannot open are also shown in a message box.

| Problem | What to do |
|---|---|
| A Gallery link does nothing | Windows: start the Player once, so it registers. Linux: run `install.sh` again. macOS: links do not reach the Player yet; use `--sim <simulation id>` |
| "Eustress Player could not open this link" | Copy the link again from the Gallery or the host. If it names the other server, see Production and staging |
| "Join again from the gallery." | The Gallery's ticket works once. Press Play in the Gallery for a new one |
| The Player does not start on Linux | Run `install.sh` again and install the libraries it names |

## Player and Studio

The Player plays published Spaces and joins live sessions, and needs no account to download. Eustress Studio builds and publishes Spaces and hosts live sessions; its download needs a free account.
