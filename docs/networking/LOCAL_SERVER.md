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

1. saves the Space, when you are editing, so players get what you see;
2. bakes the Space and the Universe's shared `assets/` into `.echk` chunks
   (the same export publishing uses), off the main thread;
3. starts listening on port 7777;
4. enters Play with a character, when you were not already playing.

The Output panel prints the join link and the command for this computer:

```text
Join link: eustress://join/127.0.0.1:7777?key=9f2c...&pin=3f9a...
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
eustress-client "eustress://join/192.168.1.20:7777?key=9f2c...&pin=3f9a..."
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

## 4. What a session shares

| Data | How it travels |
|---|---|
| The world | The host's baked Space as `.echk` chunks, once per join, over the reliable stream in 256 KiB pieces. Each chunk must hash to its BLAKE3 name before the Player uses it. |
| Avatars | 20 samples a second per player: position, facing, movement direction, vertical speed, sprint, crouch, grounded, and a jump count. Datagrams where the connection supports them, the reliable stream otherwise. Each receiver drives the avatar with its own character controller and corrects it toward the sender's position, snapping when it is more than 4 m off. |
| Appearance | Each player's avatar descriptor, once per join. |
| Arrivals and departures | Reliable messages; the host shows a notification for each. |
| Chat | Carried by the protocol. Neither Studio nor the Player has a chat box yet. |

What a session does **not** share yet:

- **Changes to the world after a player joins.** A player sees the world as it
  was baked when the server started: a part a script moves, a door that opens,
  or an object physics knocks over stays where it was on the player's screen.
- **Physics objects.** Only avatars are synchronised.
- **Scripts on the Player.** The Player runs no scripts. The host is the only
  authority by construction.

## 5. Safety

| Check | Rule |
|---|---|
| Who can connect | This computer only, unless `EUSTRESS_HOST_LAN=1`. |
| Join key | 128 random bits, compared in constant time before a session is accepted. |
| Host identity | Every server start mints a self-signed ECDSA P-256 certificate valid for 14 days. Players verify its SHA-256 (the pin). A Player refuses to join another computer's host without a pin. |
| Avatar samples | Finite numbers, within 100 km of the origin, at most 80 m/s plus 6 m of slack between samples. A player with more than 60 violations in 10 seconds is removed. |
| Chat | 8 lines per 10 seconds, 400 characters each. Names are cut to 32 characters. |
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
| A certificate error | The pin is from an earlier session. Every server start has a new one. |
| `Could not start the server` with an address in use | Another program holds port 7777. Set `EUSTRESS_HOST_PORT`. |
| `The Space is still opening` | The Space's database was still loading. Start the server again in a moment. |

## 8. Browsers

The transport is WebTransport so that a browser build of the Player can join
the same hosts: a browser's `WebTransport` constructor takes the pin through
`serverCertificateHashes`, and the session and wire code are free of tokio and
of the operating system. The browser Player itself is not built yet.
