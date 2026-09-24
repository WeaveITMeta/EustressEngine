# Networking

Two things leave the computer a world is authored on, and both use the same
container, `.echk`:

| Path | What happens | Guide |
|---|---|---|
| **A hosted session** | Studio hosts the Space it is playing; Players join over WebTransport, download the world as `.echk` chunks, and see every avatar live. | [LOCAL_SERVER.md](./LOCAL_SERVER.md) |
| **A published simulation** | Studio uploads a Universe to the gallery as `.echk` chunks; the Player downloads it from the API and plays it solo. | [Publishing](#publishing) below |

## Layers

```text
eustress-echk            the chunk container and the world manifest; no IO
      │
eustress-networking
  ├── wire               messages and framing (bincode, 4 MiB cap)
  ├── join_link          eustress://join/<host:port>?key=…&pin=…
  ├── session            host and player state machines, avatar sync, validation
  └── native             the WebTransport transport (wtransport on quinn); desktop only
      │
engine  multiplayer.rs   Start Server / Stop Server, the host's bake
client  net_play.rs      joining, opening the world that arrived
```

`wire`, `join_link` and `session` use no tokio, no sockets and no clock: the
transport feeds them events through channels. That split is what lets a browser
build supply its own transport later without touching the session.

| Crate | Path |
|---|---|
| `eustress-echk` | `eustress/crates/echk` |
| `eustress-networking` | `eustress/crates/common/eustress-networking` |
| Studio host | `eustress/crates/engine/src/multiplayer.rs` |
| Studio export and publish | `eustress/crates/engine/src/space/echk_export.rs`, `echk_publish.rs` |
| Player | `eustress/crates/client/src/systems/net_play.rs`, `space_fetch.rs`, `live_world.rs` |
| Worker routes | `infrastructure/cloudflare/api/src/world.mjs` |

## The `.echk` container

```text
magic   "ECHK"   version u32 LE (1)   count u32 LE
count × (path_len u32 LE, path, data_len u32 LE, data)
```

Each record is one file of a Space, at its path inside the Space. A chunk is
named by the BLAKE3 hash of its bytes, and every reader checks that hash before
using it. A world manifest (JSON) lists each Space's chunks, the chunks of the
Universe's shared `assets/`, and the Space a player opens first.

Exporting a Space takes each entity's current state from the Space's database
(including cores a large Space streams from), adds files that exist only in
the Space folder, and buckets entities into 256 m chunks. Files without a
position share one chunk, which is split whenever it grows past 64 MiB.

## Publishing

**Publish** in Studio exports every Space of the Universe, then:

1. `POST /api/simulations/{id}/world/begin` with the manifest; the API answers
   with the chunks it lacks;
2. `PUT /api/simulations/{id}/world/chunks/{hash}` for each of those;
3. `POST /api/simulations/{id}/world/commit`: the listing now plays this
   manifest and goes back to review.

Only changed chunks are uploaded, so a republish that touched one corner of a
map uploads that corner. The listing is created on the first publish and its id
is kept in the Universe's `.eustress/sync.toml`, so every later publish lands in
the same listing. **Publish Space** swaps one Space into the published world
and keeps the others.

The Player plays a published simulation with:

```bash
eustress-client --sim <simulation id>
```

It reads `GET /api/simulations/{id}/world/manifest`, downloads the chunks it
has not cached from `GET /api/simulations/{id}/world/chunks/{hash}`, checks
each against its name, and opens the start Space. Listings published before
`.echk` answer the manifest request with `409` and are fetched as a `.pak`
from `/download` instead.

Both read routes follow the gallery's gate: an approved public listing is open
to everyone, anything else only to its author and administrators, and a
listing under legal review to administrators alone.

## Not built yet

- World changes after a player joins a hosted session: players see the world as
  it was when the server started, plus live avatars.
- Physics objects in a hosted session: only avatars are synchronised.
- A chat box: chat travels in the protocol, and neither shell shows it.
- The browser Player, and joining a hosted session from a browser.
- Multiplayer for published simulations: a listing plays solo, and no server
  registers itself with the API.
