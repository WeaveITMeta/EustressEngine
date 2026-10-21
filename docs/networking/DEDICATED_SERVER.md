# A dedicated server on your own PC

Staging only. This branch does not deploy the Worker, does not change a
port, a DNS record, or a firewall, and does not push to `main`.

The game host is `eustress-engine --server`. It is the same WebTransport
host Studio uses for Play: an ECDSA P-256 certificate valid a minute under
14 days, the SHA-256 of that certificate in the join link, server Luau
scripts from the published source world, and a heartbeat to
`https://api-staging.eustress.dev`. `eustress-server` does not do any of
that. It only downloads a world and runs it with no Player protocol.

`eustress-forge` is the local control plane. It runs forge-orchestration
0.6 (`ForgeBuilder::run`) on `127.0.0.1:8080` with no Nomad client, and a
small compatibility server on `127.0.0.1:8090` that speaks the routes
Studio already calls: `POST /api/auth`, `POST /api/deployments`,
`GET /api/deployments/{id}/status`, and `GET /api/experiences/{id}/servers`.
Each instance is one process. The first UDP port is 7777. The next instance
on the same computer takes 7778, then 7779. Studio asks for one server
(`min_servers: 1`). The scale ceiling of 5 is not started.

The job id is a UUID in `EUSTRESS_SERVER_ID`. It is not the process id, the
port, or the IP address. The server credential (`esrv_` plus 64 hex
characters) and the author's session stay in the environment. They are not
command arguments.

## What you do

1. Sign Studio in to **staging** (`https://api-staging.eustress.dev`).
   A production sign-in is refused by that API with 401. The output and the
   server log say so.
2. Publish the Space to staging, with **Share Source** on, so the dedicated
   server can download server Luau scripts. If the listing has no source
   world, the server says it loaded the player world and that server scripts
   will not run. Whether Box Head
   (`07d084d8-9098-4192-bb48-84d2c428db36`) has a source world was not
   checked from this tree.
3. Build `eustress-forge` and `eustress-engine`. Put `eustress-engine` on
   `PATH`, or set `EUSTRESS_ENGINE_BIN` to it.
4. Set the public address players will use, and the region name you want
   listed:

   ```text
   EUSTRESS_HOST_PUBLIC=203.0.113.7
   EUSTRESS_HOST_REGION=us-east-1
   ```

   `EUSTRESS_HOST_PUBLIC` is required for the gallery to list the session.
   Loopback and link-local addresses are refused. `EUSTRESS_HOST_LAN=0`
   still wins if it is set before the process starts; leave it unset and
   the server listens beyond this machine.
5. Start the control plane and leave it running:

   ```text
   eustress-forge
   ```

   The API key is `local-dev`, or `FORGE_LOCAL_API_KEY` if you set one.
   The compatibility server binds to loopback only.
6. In Studio, Network → **Allocate Server**. The Forge field starts at
   `http://127.0.0.1:8090`. If you are not already connected, Allocate
   connects there with the key `local-dev` and deploys. A Forge you already
   connected to another URL is left alone.
7. The child downloads the published world into
   `~/.eustress_engine/dedicated/<listing id>/` and hosts that copy. It does
   not open the Space files the editor has open.
8. Forward **UDP 7777** on the router to this computer's LAN address, UDP
   7777. Not TCP. The Player and the browser both speak QUIC on that UDP
   port. Only 7777 is the port to forward unless you also forward 7778 and
   the ports after it. Those are extra instances on the same PC.
9. Windows: Windows Defender Firewall, Inbound Rules, New Rule, Port, UDP,
   7777, Allow the connection. Linux: allow inbound `udp/7777`.
10. Stop by stopping `eustress-forge` (it asks the children to exit) or by
    writing `dedicated.stop` next to the status file. The child then
    `DELETE`s `/live`.

The status file for a Forge instance is
`~/.eustress_engine/dedicated/<listing id>/<job id>.json`.

## What the forward test means

On start the host can ask a public STUN server where its UDP socket
appeared. An answer such as `203.0.113.7:7777` means the router kept the
port for traffic **this computer started**. It is not proof that a new
connection from the internet is accepted. A test from inside the same
network often fails even when the forward is right (NAT hairpin). A player
on another network is the proof. This branch did not run that test.

## Certificate and heartbeat

The host mints a new ECDSA P-256 certificate each time it starts, valid a
minute under 14 days. The join link's `pin` is the SHA-256 of that
certificate. The heartbeat sends the same value as `cert_sha256`. The
Worker refuses a hash that does not match the pin. The browser pins it
with `serverCertificateHashes`.

The heartbeat is `POST /api/simulations/{id}/live` on the staging API,
every 30 seconds, with the server credential. The Worker code that accepts
`esrv_` is in this branch and is **not deployed**. Until it is, a heartbeat
against the live staging API fails. That failure was not papered over.

Each heartbeat can also send:

- `server_id` — the job id
- `region` — `EUSTRESS_HOST_REGION` (a name such as `us-east-1`), stored
  beside the existing country object, not inside it
- `latency_ms` — how long a GET to the API took, once that has been
  measured. It is omitted until then. It is not the player's ping, and it
  is never reported as 0 ms.
- `hardware` — `{ gpu, cpu, tier }` with tier `unknown`, `low`, `mid`, or
  `high`

The listing shows one row per server: players, free seats, region, "N ms to
the API" or "not measured", and the hardware tier. Full servers are last
and are not the Join target. Quick Join is the busiest server that is not
full, scored as

```text
0.5 * fullness + 0.3 * (tier / 3) + 0.2 * ping
```

`ping` is `1 - min(latency_ms, 300) / 300` when measured, and 0 when it is
not. A missing measurement does not rank as the closest server.

## What was not verified

- Booting Box Head, and server scripts actually running inside it.
- A heartbeat that the live staging Worker accepts. The Worker in this
  branch is not deployed.
- A Player join, and a browser join.
- An inbound UDP forward. STUN only shows the outbound mapping.
- That the listing `07d084d8-9098-4192-bb48-84d2c428db36` has Share Source on.
