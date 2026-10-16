# Two machines: join a Studio host (MP-01), play a published Space (GW-01)

**Status:** written 2026-10-07 from a trace of the current code (Studio, the Player, the
networking crate and the Worker). Nothing here has been run yet. Machine A hosts and publishes; machine B
plays. Its three fixes (the host certificate, the IPv6 gallery rule, the purchase ticket) land in build 26, so run it on build 26 or later. Run every PowerShell block in a normal (not admin) window unless it says otherwise.

Related: [STAGING_PUBLISH_TEST.md](STAGING_PUBLISH_TEST.md) (sign-in, review and the gallery in detail),
[PLAYTEST_SA2A.md](../networking/PLAYTEST_SA2A.md) (the server-authority checks),
[LOCAL_SERVER.md](../networking/LOCAL_SERVER.md) (every hosting variable).

---

## 0. Before you start

| Need | How to check |
|---|---|
| Build 26 Studio on A, build 26 Player on B (the same build: a different one is refused with "This host speaks protocol N") | Copy `E:\Workspace\EustressEngine\eustress\target\last-good\eustress-client.exe` to B. It is one file, about 170 MB. |
| The Visual C++ 2015-2022 runtime (x64) on B: the Player needs `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll` and `MSVCP140.dll` | On B: `Test-Path "$env:WINDIR\System32\vcruntime140_1.dll"` must say `True`. If not, install Microsoft's "Visual C++ Redistributable for Visual Studio 2015-2022 (x64)" on B. |
| Both machines on the same local network, its profile **Private** | On A: `Get-NetConnectionProfile` shows `NetworkCategory : Private`. |
| UDP port 7777 open on A for Studio | The first time Studio hosts on the network, Windows asks to allow it: allow **Private networks**. If no prompt came and joins time out, an admin PowerShell on A (McKale's call, it changes the firewall): `New-NetFirewallRule -DisplayName "Eustress Studio host (UDP 7777)" -Direction Inbound -Protocol UDP -LocalPort 7777 -Action Allow -Profile Private` |
| A's local network address | On A: `(Get-NetIPConfiguration \| Where-Object { $_.IPv4DefaultGateway -and $_.NetAdapter.Status -eq 'Up' } \| Select-Object -First 1).IPv4Address.IPAddress`. Write it down as `<LAN>`. |
| Smart App Control on B | A new unsigned `eustress-client.exe` can be blocked on a Windows 11 machine with Smart App Control on. If B blocks it, stop and tell McKale; never turn Smart App Control off to get past it. |

Logs: Studio `C:\Users\<you>\.eustress_engine\logs\engine-<number>.log`, the Player
`C:\Users\<you>\.eustress_engine\logs\player-<number>.log` (newest file).

---

## Part 1 (MP-01): join a Studio host across the network

### On A: host

```powershell
$env:EUSTRESS_HOST_LAN = '1'
& "E:\Workspace\EustressEngine\eustress\target\last-good\eustress-engine.exe"
```

Without `EUSTRESS_HOST_LAN=1` a host listens on this computer only and its link names `127.0.0.1`; B can then
never reach it. Open Box Head (`Documents\Eustress\Games`, Space "Box Head Zombie Defense Tycoon") and press
F9 (or Start Server).

**You should see:** the toast "Hosting on your local network on port 7777", and in the Output panel
`Join link: eustress-player://join/<address>:7777?key=...&pin=...`. Copy that whole line.

**Check the address:** `<address>` must be `<LAN>`. Studio guesses it from the route to the internet, which
can be a VPN or Cloudflare WARP adapter. If it differs, use the link with `<address>` replaced by `<LAN>`; the
key and pin stay as they are.

### On B: join

```powershell
& "<folder>\eustress-client.exe" 'eustress-player://join/<LAN>:7777?key=<key>&pin=<pin>'
```

Keep the single quotes: the `&` needs them.

**You should see:** the Player window, the Box Head world, your avatar. On A: the toast "<name> joined." and
the Output line "<name> joined the session.". A's log: `net: <name> (<B's address>) said Hello as peer 1`.

### Then the server-authority checks

Run every check in [PLAYTEST_SA2A.md](../networking/PLAYTEST_SA2A.md) with B as the Player. The finish list
counts MP-01 done when: both machines list 2 players, the joined player's UI resets once per respawn, a host
emote plays on both in phase, a host Sound is heard on B, and a host teleport of the joined player holds.

### If the join fails

| What B shows | What it means |
|---|---|
| Nothing at all: no window | The link did not parse; a release Player shows no message for that (known gap, Client). Run `$p = Start-Process "<folder>\eustress-client.exe" -ArgumentList '<link>' -Wait -PassThru; $p.ExitCode`: `2` means the link is malformed. Copy it again, whole. |
| "The host did not answer" | A is not reachable on UDP 7777: the firewall (section 0), a Public network profile, the wrong address in the link, or Studio started without `EUSTRESS_HOST_LAN=1`. |
| "Could not reach the host" with `NotValidYet` in the Player log | B's clock is behind A's. Build 26 starts the host's certificate a day early, so this should not appear; if it does, the build lacks that fix. |
| "The host did not let you in" with "This host speaks protocol N" | A and B run different builds. |
| Stuck after the world downloads, no avatar | Read the Player log for `open` errors (known gap, Client: a world the Player cannot open shows nothing). |

### Purchases after 10 minutes (build 26's host-ticket refresh)

Only for a Space that sells something, hosted by a signed-in Studio. Keep A hosting, and join from B at
minute 11 and again at minute 21 after the server started (leave and join again). **You should see** the
purchase prompt both times. A's log has no `no host identity ticket` warning. Before build 26 a join after
minute 10 never showed a purchase prompt. Report either failure to Tickets, Bliss & Delta.

---

## Part 2 (GW-01): publish to staging, play on B

### On A: publish a copy of Pong

Close Studio first (a Universe's database is locked while it is open), then copy the Games Universe with only
Pong in it: publishing a Universe publishes every Space in it.

```powershell
Copy-Item -Recurse "C:\Users\miksu\Documents\Eustress\Games" "C:\Users\miksu\Documents\Eustress\Pong Staging Test"
Remove-Item -Recurse "C:\Users\miksu\Documents\Eustress\Pong Staging Test\Spaces\Box Head Zombie Defense Tycoon"
$env:EUSTRESS_API_URL = 'https://api-staging.eustress.dev'
& "E:\Workspace\EustressEngine\eustress\target\last-good\eustress-engine.exe"
```

Sign in with your staging identity file ([STAGING_PUBLISH_TEST.md](STAGING_PUBLISH_TEST.md), step 2). Open
"Pong Staging Test", wait until the Space has opened, press Ctrl+P. Name "Pong (staging test)", Public on,
Share source off, Publish.

**You should see:**

1. The engine log line `Published successfully: <listing id> (Published <listing id>: ...)`, and in the
   Publish dialog **Download change log (.md)**.
2. The listing id saved for staging:
   `Get-Content "C:\Users\miksu\Documents\Eustress\Pong Staging Test\.eustress\sync.toml"` shows
   `"https://api-staging.eustress.dev" = "<listing id>"` under `[remote.listings]`. (`experience_id` is
   production's listing and stays empty.)
3. The review: Listed within a few minutes, or held for a person (step 4 of STAGING_PUBLISH_TEST tells you how
   to approve it as an admin).

### Webhook refusal

In the copy, put a fake Discord webhook in a file players receive, for example a LocalScript in
StarterPlayerScripts containing `local hook = "https://discord.com/api/webhooks/0/test"`, and publish.
**You should see** the publish refused, naming the file. Remove the line again. A webhook inside
ServerScriptService or ServerStorage is not refused: those services never leave A (unless Share source is on).

### On B: play it

The Player has no sign-in of its own, and `eustress-player://` links do not say which API they belong to, so
point it at staging on the command line:

```powershell
& "<folder>\eustress-client.exe" --api-url https://api-staging.eustress.dev --sim <listing id>
```

**Before approval** only the author can play it, and only from a machine where Studio is signed in to staging
as the author (the Player reads Studio's saved sign-in). Anyone else sees "This simulation is not open to you".
**After approval** anyone can, with no sign-in.

**You should see:** the Pong world with its parts and meshes; your avatar walks; LocalScripts run.

**What you will not see:** Pong's ball move. Pong's game logic is a server Script
(`ServerScriptService/PongServer`). A published world never carries server Scripts, and a Player playing alone
runs LocalScripts only, so nothing server-driven happens without a host. To play Pong, host it on A (Part 1)
and join from B. This is the design today, recorded as a decision for McKale below.

**ServerScriptService is absent from the download.** While the Player runs, on B:

```powershell
$live = Get-ChildItem "$env:LOCALAPPDATA\eustress\live" -Directory | Sort-Object LastWriteTime | Select-Object -Last 1
Test-Path (Join-Path $live.FullName "Universe\Spaces\Current\ServerScriptService")
```

must say `False`.

### Joining from the gallery (optional)

Hosting a listed session from the staging gallery needs `EUSTRESS_HOST_PUBLIC=<LAN>:7777` as well, and B needs:

- Studio signed in to staging on B, because the Player reads Studio's sign-in, and a listed session refuses a Player without one ("A ticket is required to join").
- The Player run once with `--api-url https://api-staging.eustress.dev`, so the `eustress-player://` link opens a staging Player.

Until the listing is approved, the gallery admits only the listing's author: B's Studio must be signed in as
the same account that publishes on A, and anyone else is refused with "This session is not available right
now." (once that Worker change is deployed to staging). Steps 6 and 7 of STAGING_PUBLISH_TEST cover the rest. `EUSTRESS_HOST_PUBLIC` must be an IPv4 address or a
name: from build 26, an IPv6 address keeps the session off the gallery, and the Output says why, because a
host listens on IPv4 only.

---

## Known gaps, and who has them

| Gap | Effect on this test | Owner |
|---|---|---|
| The Player has no sign-in of its own | Listed sessions and unapproved listings need Studio signed in on B | McKale's decision, then Website and Client |
| Links do not name their API | Use `--api-url` on B | Website and Client |
| Server Scripts do not run when a published world is played alone | Pong's ball stays still under `--sim` | McKale's decision |
| Meshes in a Universe's shared `assets` folder do not show on the Player | Parts using them are invisible (Pong has none) | Disk and UI (the shared-mesh carry plan) |
| `--sim` of an imported Space spawns the avatar at unconverted coordinates | Not Pong | Client |
| A malformed link, or a world the Player cannot open, shows nothing on screen | See the failure table | Client |
| Re-committing an identical world voided its approval (a MaxPlayers-only republish) | Fixed in the Worker working tree; needs a staging deploy before it holds | Jev |
| A host listens on IPv4 only | Use IPv4 addresses throughout | Multiplayer (later build) |

## What to record

For each check: pass or fail, the time, and on a failure the last 40 lines of the Player log on B and of the
engine log on A. Send failures to the owner in the table, or to Multiplayer for Part 1.
