# Hosting a Multiplayer Session

A multiplayer session in Eustress is a Studio host that serves the Space it is
playing, and Players that join it. The host runs the scripts and the physics.
Every Player downloads the host's world once, then sees every avatar in the
session move in real time.

The code lives in three places:

| Part | Where |
|---|---|
| Host (Studio, and the headless engine) | `eustress/crates/engine/src/multiplayer.rs` |
| Session, wire format, transport | `eustress/crates/common/eustress-networking/src/` (`session.rs`, `wire.rs`, `join_link.rs`, `native.rs`) |
| Joining (the Player) | `eustress/crates/client/src/systems/net_play.rs` |

---

## 1. Host from Studio

1. Open the Space you want to share.
2. Press **F9**, or choose **Network > Start Local Server**, or **Start** in
   the Test tab's Server group.

Studio then:

1. writes your unsaved part edits to the Space's files, when you are editing,
   so players get what you see. Hosting is not a save: it makes no snapshot
   commit and leaves your terrain files alone;
2. bakes the Space and the Universe's shared `assets/` into `.echk` chunks
   (the same export publishing uses), off the main thread. The terrain comes
   from memory, edits and all. A migrated Space hosts its saved terrain, and
   Studio says so when you have terrain edits that are not yet saved. The
   chunks go to `.eustress/host/` in the workspace folder, never into the
   Space;
3. starts listening on port 7777;
4. enters Play with a character, when you were not already playing.

The Output panel prints the join link and the command for this computer:

```text
Join link: eustress-player://join/127.0.0.1:7777?key=9f2c...&pin=3f9a...
On this computer: eustress-client --connect 127.0.0.1:7777
```

**Stop Server**, or returning to Edit, ends the session. Pausing does not.

## 2. Join with the Player

On the same computer the key and pin are read from the file the host leaves at
`<workspace>/.eustress/hosts/<port>.link`, so the address is enough:

```bash
eustress-client --connect 127.0.0.1:7777
```

From another computer, pass the whole link, or its parts:

```bash
eustress-client "eustress-player://join/192.168.1.20:7777?key=9f2c...&pin=3f9a..."
eustress-client --connect 192.168.1.20:7777 --key 9f2c... --pin 3f9a...
```

`--name` sets the name other players see. Without it the Player uses
`EUSTRESS_PLAYER_NAME`, then the computer's user name.

Joining downloads the host's world, opens it, and places the avatar near the
Space's SpawnLocation, each player on its own spot around it. Chunks are cached
under the local data folder (`eustress/echk/`), so joining the same host again
downloads only the chunks that changed.

## 3. Let other computers in

By default a host accepts players from its own computer only. To open it:

1. Set `EUSTRESS_HOST_LAN=1` before starting Studio. The host then listens on
   every network interface, and the join link carries the computer's local
   network address.
2. Allow inbound **UDP 7777** in the firewall.
3. For players outside your network, forward **UDP 7777** on the router to
   this computer, and share the link with your public address in place of the
   local one.

The key in the link keeps anyone without it out, and the pin makes sure a
player reaches your computer rather than an impostor. Both change every time a
server starts, so share the link from the current session.

### Settings

| Variable | Default | Meaning |
|---|---|---|
| `EUSTRESS_HOST_PORT` | 7777 | Port to listen on. `0` picks any free port. |
| `EUSTRESS_HOST_LAN` | off | `1` also accepts players on the local network. |
| `EUSTRESS_HOST_MAX_PLAYERS` | 8 | Players besides the host. |
| `EUSTRESS_HOST_ON_PLAY` | off | `1` hosts whenever Play starts. |
| `EUSTRESS_HOST_PUBLIC` | unset | The address players reach this host at, for the gallery's live link (`host` or `host:port`, IPv6 in brackets): this computer's local network address when players come in through the tunnel's private route, a public name later. |

A published simulation hosted by its author, signed in, with
`EUSTRESS_HOST_LAN=1` and `EUSTRESS_HOST_PUBLIC` set, shows as live in the
gallery: Studio tells the API every 30 seconds, and again when the server
stops. The address is only ever the configured one.

## 4. What a session shares

| Data | How it travels |
|---|---|
| The world | The host's baked Space as `.echk` chunks, once per join, over the reliable stream in 256 KiB pieces. Each chunk must hash to its BLAKE3 name before the Player uses it. As in Roblox, ServerScriptService and ServerStorage stay on the host, and so does the code of every other server Script (players get the Script, not its source); the host's log names any file players do receive that holds a webhook URL. |
| Avatars | 20 samples a second per player: position, facing, movement direction, vertical speed, sprint, crouch, grounded, and a jump count. Datagrams where the connection supports them, the reliable stream otherwise. Each receiver drives the avatar with its own character controller and corrects it toward the sender's position, snapping when it is more than 4 m off. |
| Appearance | Each player's avatar descriptor, once per join. |
| Arrivals and departures | Reliable messages; the host shows a notification for each. |
| Chat | Carried by the protocol. Neither Studio nor the Player has a chat box yet. |
| Identity | A signed-in player may send a ticket from the API naming its account, bound to the host's pin; the host checks it with the API before trusting it. The host sends its own ticket the same way, so a player can check who hosts. |
| Purchases | A host script's purchase prompt reaches the player it names, and the player's answer and receipts come back, bounded and rate-limited. The host verifies every receipt with the API; nothing a player sends is trusted as sent. |
| Changes to the world | Everything the host's scripts do to instances players can see (spawns, destroys, moves, property and attribute writes, tags, sounds, particle bursts), as one ordered frame per tick. A player who joins late first gets one catch-up frame with everything since the server started. See [SERVER_AUTHORITY.md](./SERVER_AUTHORITY.md). |
| Physics | Bodies the host's physics moves, 30 times a second, as datagrams; a body that comes to rest gets its final pose reliably. |
| Input | Each player's keys, mouse and gamepad buttons, sticks, camera and aim, every tick. On the host, `Player:IsKeyDown` and `Player:IsMouseButtonPressed` answer from it. |
| Remote calls | `FireClient` and `FireAllClients` travel with the world's changes, after the writes made before them. Players' `FireServer` and `InvokeServer` reach the host's scripts. |

The Player draws changes to the world, physics, and remote calls from the
host once it keeps its own copy of the world's instance tree; until then a
player sees the world as it was baked when the server started, plus every
avatar live. The Player runs no server scripts: the host is the only
authority by construction.

## 5. Safety

| Check | Rule |
|---|---|
| Who can connect | This computer only, unless `EUSTRESS_HOST_LAN=1`. |
| Join key | 128 random bits, compared in constant time before a session is accepted. |
| Host identity | Every server start mints a self-signed ECDSA P-256 certificate valid for 14 days. Players verify its SHA-256 (the pin). A Player refuses to join another computer's host without a pin. |
| Avatar samples | Finite numbers, within 100 km of the origin, at most 80 m/s plus 6 m of slack between samples. A player with more than 60 violations in 10 seconds is removed. |
| Chat | 8 lines per 10 seconds, 400 characters each. Names are cut to 32 characters. |
| Input | One sample per tick is kept; a camera must be finite and inside the world. |
| Remote calls | At most 64 arguments, 64 KiB, tables nested 16 deep; 60 calls a second per remote, bursts of 120. The remote and every instance an argument names must be ones that player can see, and a remote's `ArgumentTypes`, when set, must match. Malformed calls count toward removing the player. |
| What players see | ServerScriptService, ServerStorage, every server Script's code, the host's camera, and each other player's `PlayerGui`, `Backpack` and `PlayerScripts` never leave the host. |
| Probe | `/probe/<key>` takes the join key and only echoes (datagrams, and up to four streams of 4 MiB, for two minutes), to measure the path to a host. |
| Sizes | Messages up to 4 MiB; a world up to 8 GiB; a chunk up to 512 MiB; paths inside a chunk checked before anything is written to disk. |

## 6. A host with no window

The headless engine (`eustress-headless`) registers the same hosting plugin.
With `EUSTRESS_HOST_ON_PLAY=1` it hosts whenever Play starts, which makes it a
dedicated host. The standalone `eustress-server` binary does not host.

## 7. Troubleshooting

| Symptom | Cause |
|---|---|
| `could not join: ... timed out` | Nothing is listening at that address: the server is stopped, the firewall blocks UDP 7777, or `EUSTRESS_HOST_LAN` is off on the host. |
| `the host did not answer within 20 seconds` | The connection opened but the host never welcomed the player. Restart the server. |
| The join is refused at once | The key is wrong. Copy the link from the current session. |
| `This host speaks protocol 3 and your Player speaks 2` | Studio and the Player were built from different versions. Rebuild or update both. |
| A certificate error | The pin is from an earlier session. Every server start has a new one. |
| `Could not start the server` with an address in use | Another program holds port 7777. Set `EUSTRESS_HOST_PORT`. |
| `The Space is still opening` | The Space's database was still loading. Start the server again in a moment. |

## 8. Browsers

The transport is WebTransport so that a browser build of the Player can join
the same hosts: a browser's `WebTransport` constructor takes the pin through
`serverCertificateHashes`, and the session and wire code are free of tokio and
of the operating system. The browser Player itself is not built yet.
