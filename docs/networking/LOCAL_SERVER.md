# Running a Local Eustress Server

> Inspired by the classic Minecraft server experience — port-forward once, share your IP, play together.

---

## Quick Start

### 1. Download the server binary

```bash
# From the Eustress release page, or build from source:
cargo build --release -p eustress-server
```

The binary is `eustress-server` (Linux/macOS) or `eustress-server.exe` (Windows).

### 2. Start the server

```bash
# Minimal — default port 7777, max 8 players
./eustress-server

# Full options
./eustress-server \
  --port 7777 \
  --max-players 16 \
  --tick-rate 60 \
  --scene my_world.eustress \
  --name "My Eustress Server"
```

The server also starts a **Stream Node** on port 33000 for AI agents and tooling.

### 3. Port forward on your router

Open **two ports** on your home router and point them to your machine's local IP:

| Service | Protocol | Port  | Purpose                            |
|---------|----------|-------|------------------------------------|
| Play    | UDP/QUIC | 7777  | Player connections, entity sync    |
| Stream  | TCP      | 33000 | AI agents, remote CLI, dashboards  |
| REST    | TCP      | 43000 | Browser dashboard, health check    |

> **How to find your local IP:** Run `ipconfig` (Windows) or `ip addr` (Linux/macOS).
> Look for your LAN IP, typically `192.168.x.x` or `10.0.x.x`.

### 4. Find your public IP

Visit [whatismyip.com](https://whatismyip.com) or run:

```bash
curl -s ifconfig.me
```

### 5. Share the connection string with friends

```
eustress://203.0.113.42:7777
```

Or just send the raw IP + port: `203.0.113.42:7777`

---

## Connecting as a Client

In the Eustress editor, open the **Play** menu → **Connect to Server** and enter:

```
IP:PORT    e.g.  203.0.113.42:7777
           or    friend.noip.me:7777   (dynamic DNS)
```

---

## Server Configuration File

Create `server.toml` in the same directory as the binary:

```toml
[server]
port        = 7777
max_players = 32
tick_rate   = 60       # 24 | 60 | 144
name        = "My World"
scene       = "worlds/main.eustress"

[stream]
enabled     = true
tcp_port    = 33000
rest_port   = 43000
nodes       = 1        # increase for >622 K msg/s fan-out (see scaling below)

[security]
password    = ""       # leave empty for open server
whitelist   = []       # Steam IDs or usernames, empty = allow all
```

---

## Tick Rate Guide

The server tick rate controls how frequently game state is synchronised across all clients.

| Tick Rate | Interval | Best for                                    |
|-----------|----------|---------------------------------------------|
| **24 Hz** | 41.7 ms  | Low-bandwidth links, slow-paced games       |
| **60 Hz** | 16.7 ms  | Standard — recommended for most worlds      |
| **144 Hz**| 6.9 ms   | Competitive, fast-paced, physics-intensive  |

> All clients must be able to reach at least **24 FPS** to stay in sync.
> The server is authoritative — clients with lower FPS receive corrections.

### LOD-aware replication

Entities further from each player are replicated at reduced rates automatically:

| Distance   | Replication rate at 60 Hz server |
|------------|----------------------------------|
| < 20 m     | 60 Hz (every tick)               |
| 20 – 100 m | 10 Hz (every 6 ticks)            |
| 100 – 500 m| 2 Hz  (every 30 ticks)           |
| > 500 m    | 0 Hz  (culled — no bandwidth)    |

---

## Scaling Equations

### Variables

| Symbol | Meaning |
|--------|---------|
| `P`    | Connected players |
| `Hz`   | Server tick rate (24, 60, or 144) |
| `V`    | Average visible players per client (default 20) |
| `BW`   | Available server bandwidth (bytes/s) |
| `T`    | StreamNode TCP throughput (bytes/s) |

### Per-player bandwidth

```
Upstream   (client → server):   U = 48 × Hz  bytes/s  ≈ 2.9 KB/s  @ 60 Hz
Downstream (server → client):   D = 56 × V × Hz  bytes/s
                                   = 56 × 20 × 60  ≈ 67.2 KB/s  @ 60 Hz, V=20
```

Packet breakdown:
- Input packet (upstream): 48 bytes = pos(12) + rot(16) + buttons(4) + tick(8) + padding(8)
- Entity update (downstream): 56 bytes = net_id(8) + pos(12) + rot(16) + vel(12) + flags(8)

### Server total bandwidth

```
Inbound:   S_in  = P × 48 × Hz
Outbound:  S_out = P × 56 × V × Hz
```

**Examples at 60 Hz, V = 20:**

| Players | Inbound       | Outbound      | Required BW  |
|---------|---------------|---------------|--------------|
| 8       | 23 KB/s       | 538 KB/s      | 1 Mbps       |
| 32      | 92 KB/s       | 2.2 MB/s      | 18 Mbps      |
| 100     | 288 KB/s      | 6.7 MB/s      | 54 Mbps      |
| 500     | 1.4 MB/s      | 33.6 MB/s     | 269 Mbps     |

### Maximum players for a given upload bandwidth

```
P_max(BW, Hz, V) = floor(BW / (56 × V × Hz))
```

Common uplinks:

| Upload BW   | P_max @ 24 Hz | P_max @ 60 Hz | P_max @ 144 Hz |
|-------------|---------------|---------------|----------------|
| 10 Mbps     | 372           | 148           | 62             |
| 100 Mbps    | 3,720         | 1,488         | 620            |
| 1 Gbps      | 37,202        | 14,880        | 6,200          |

### Stream Node (EustressStream TCP) throughput

```
P_stream(Hz) = floor(StreamNode_throughput / (Hz × msg_per_player))
```

Benchmarked throughputs (internal, single node):

| Transport                          | Throughput        | P_max @ 60 Hz (3 msg/player/tick) |
|------------------------------------|-------------------|-----------------------------------|
| In-process                         | ~85 M msg/s       | ∞ (no network)                    |
| TCP batch-256 (mixed topics)       | ~622 K msg/s      | ~3,455 players                    |
| TCP batch-1024 (same topic)¹       | **~836 K msg/s**  | **~4,644 players**                |
| **TCP no-ack batch-64**²           | **~1,049 K msg/s**| **~5,827 players**                |
| **TCP no-ack batch-256**²          | **~1,151 K msg/s**| **~6,394 players**                |
| **TCP no-ack batch-1024**²         | **~1,184 K msg/s**| **~6,578 players**                |
| QUIC batch-256                     | ~336 K msg/s      | ~1,866 players                    |

¹ `PublishBatchTopic` with `BatchAckCompact` — use when all messages share one topic (e.g. `scene_deltas`).
² `PublishBatchNoAck` — fire-and-forget, no ack. Use for best-effort streams (`scene_deltas`, `log/output`, `agent_observations`). Throughput bounded by TCP write bandwidth, not RTT.

### ForgeCluster horizontal scaling

```
P_cluster(N, Hz) = N × P_stream(Hz)
```

| Nodes | TCP P_max @ 60 Hz | Ports             |
|-------|-------------------|-------------------|
| 1     | ~3,455            | 33000             |
| 3     | ~10,365           | 33000–33002       |
| 5     | ~17,275           | 33000–33004       |
| 10    | ~34,550           | 33000–33009       |

Enable multi-node in `server.toml`:

```toml
[stream]
nodes = 3   # starts 3 TCP nodes on 33000, 33001, 33002
```

### Recommended server specs

| Players | CPU          | RAM   | Upload     | Config                    |
|---------|--------------|-------|------------|---------------------------|
| ≤ 32    | 2-core       | 2 GB  | 10 Mbps    | Single node, 60 Hz        |
| ≤ 100   | 4-core       | 4 GB  | 100 Mbps   | Single node, 60 Hz        |
| ≤ 500   | 8-core       | 8 GB  | 500 Mbps   | 3-node cluster, 60 Hz     |
| ≤ 2000  | 16-core      | 16 GB | 1 Gbps     | 5-node cluster, 60 Hz     |
| ≤ 10000 | 32-core      | 32 GB | 10 Gbps    | 10-node cluster, 24 Hz    |

---

## Troubleshooting

### "Connection refused" on port 7777

1. Check the server is running: `./eustress-server --port 7777`
2. Check your firewall allows UDP/7777 inbound
3. Verify port forwarding points to the **correct local IP**
4. Test locally first: connect to `127.0.0.1:7777`

### Players can connect locally but not externally

Your router NAT isn't forwarding the port. Verify:
- Port forward rule: External 7777 UDP → Internal `<your-local-ip>` 7777
- Some ISPs block ports below 1024 — try port 27777 instead
- If behind CGNAT (common on mobile/shared IPs), use a VPN or relay

### High latency / rubber-banding

Lower the tick rate to reduce bandwidth pressure:

```toml
tick_rate = 24
```

Or reduce visible players per client:

```toml
[replication]
view_distance = 50   # metres — entities beyond this are culled
```

### Stream Node not reachable

```bash
# Check it's listening
ss -tlnp | grep 33000        # Linux
netstat -an | grep 33000     # Windows

# Test subscribe from CLI
eustress stream subscribe scene_deltas --host 203.0.113.42:33000
```

---

## Dynamic DNS (no static IP)

If your public IP changes, use a free dynamic DNS service:

1. Register at **DuckDNS** or **No-IP** — get a hostname like `myworld.duckdns.org`
2. Run their updater client on your server machine
3. Share `eustress://myworld.duckdns.org:7777` instead of a raw IP

---

## Headless / Cloud Deployment

For persistent 24/7 servers, run the binary on a VPS (DigitalOcean, Hetzner, etc.):

```bash
# systemd service
[Unit]
Description=Eustress Game Server
After=network.target

[Service]
ExecStart=/opt/eustress/eustress-server --port 7777 --scene worlds/main.eustress
Restart=always
User=eustress

[Install]
WantedBy=multi-user.target
```

No port forwarding needed — VPS machines have public IPs directly.
