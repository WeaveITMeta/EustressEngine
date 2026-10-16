# Hosting cost plan

Status: **research and plan, 2026-10-01.** Nothing is bought, no account is
opened, and nothing here is measured on a real session yet. Every number is
either cited (Sources, at the end) or labelled as an assumption. The
assumption that moves the answer most is how many sessions fit on one
server, and only the stress test can settle it
([docs/networking/STRESS_TEST.md](../networking/STRESS_TEST.md), not built).

## The answer

| | Per player-hour (CCU-hour) | Per 1,000 DAU per month |
|---|---|---|
| Roblox, infrastructure and trust and safety, all of it | **$0.0095 to $0.0125** | **$690 to $920** |
| Roblox, estimated share for game servers alone | $0.004 to $0.009 | $290 to $660 |
| **Eustress, recommended mix, all in** | **about $0.0008** (range $0.0004 to $0.0025) | **about $56** (range $30 to $180) |
| How much cheaper than Roblox's all-in figure | **92% to 94%** at the middle estimate | |

The recommended mix: creators host most sessions on their own machines with
a small relay we run, dedicated servers with bandwidth included in the price
carry public and always-on worlds with many sessions packed on each box, and
Cloudflare runs only the control plane (registry, admission, worlds on R2),
never the simulation.

Beating Roblox by 90% rests on three things:

1. **Bandwidth that costs nothing per gigabyte.** Outgoing traffic is the
   largest cost at a hyperscaler: at AWS's $0.09/GB, the egress for one player
   alone costs about as much as Roblox spends per player-hour on everything.
   Dedicated servers with unlimited traffic (Hetzner) or unmetered traffic
   (OVH) remove that line entirely.
2. **Creators hosting a share of the hours.** A creator-hosted hour costs us
   about $0.0001 (the relay and the registry).
3. **Packing.** At the middle estimate (30 small sessions on an 8-core,
   64 GB box, half full on average), our own servers cost $0.0017 per
   player-hour. That alone is 82% to 86% below Roblox's all-in figure, short
   of 90%. The mix reaches 90% when creators host more than about half of all
   hours, or with no creator hosting at all once a box holds about 50 small
   sessions. The stress test decides which.

## What exists today

- **Creator-hosted sessions.** Studio hosts the Space it is playing (F9);
  Players join over WebTransport (QUIC over UDP), download the world as
  `.echk` chunks from the host, and see what the host's scripts and physics
  do. Protocol v3 is server-authoritative: a player sends input, the host
  decides where everything is ([SERVER_AUTHORITY.md](../networking/SERVER_AUTHORITY.md)).
- **The gallery's live-host registry** on Cloudflare: one SQLite Durable
  Object per hosted simulation (`live.mjs`), a heartbeat from Studio every
  30 s, and admission that checks every join once with the Worker
  ([HOSTED_ADMISSION.md](../networking/HOSTED_ADMISSION.md)).
- **Getting UDP to a home machine** goes through Cloudflare Tunnel: a private
  network route works today for players running the Cloudflare One client;
  public UDP to a private origin (Spectrum) is an Enterprise closed beta
  ([GOAL_LIVE_GALLERY_PLAY.md](../launch/GOAL_LIVE_GALLERY_PLAY.md)).
- **A headless runtime** exists (`eustress-headless`, `engine/Cargo.toml`), and
  the bridge and the `eustress` command line drive it
  ([HEADLESS_RUNTIME.md](./HEADLESS_RUNTIME.md)). It does not yet host a
  gallery session with its own credential (gate 11 of the gallery goal).
- **Load:** sessions above 4 players have not been load-tested
  ([MAX_PLAYERS.md](../networking/MAX_PLAYERS.md)). No number below for CPU,
  memory or bandwidth per session is measured.

## Units and assumptions

- **CCU-hour:** one player connected to one session for one hour. Roblox's
  "hours engaged" is the closest public figure and is treated as the same
  unit.
- **Hours per DAU per day: 2.4**, Roblox's own 2024 figure (73.5 billion hours
  over 82.9 million average DAU is 2.42; Q2 2025 is 2.69). So 1,000 DAU is
  about 73,000 CCU-hours a month and about 100 concurrent players on average.
  Servers are sized for the daily peak, assumed to be **2 times the average**.
- **Bandwidth per player, server to player:** 30 KB/s middle (108 MB an hour),
  10 KB/s low, 100 KB/s high (physics-heavy driving). The protocol allows up to
  256 KB/s per player by default and spends less when there is less to send.
  Roblox's own guidance is to optimize a game above 100 KB/s on its server.
  Player to server traffic (input at 60 Hz, avatar frames at 20 Hz) is a few
  KB/s and arriving traffic is free at every provider here.
- **One small session** (Box Head or Pong class, up to 12 players): **0.4 of a
  CPU thread** and **1 GB of memory** on average, middle estimate. A large
  physics session (Vehicle Simulator class) is assumed to need 1 thread and
  3 GB. For scale, Roblox gives each server 6.4 GB plus 100 MB per player as
  its memory allowance, and one or two CPU threads.
- **Average players per session: 8.**
- **Average use of a server we pay for by the month: 50%** (peaks need the
  rest), plus **25%** on top for spare boxes, monitoring and failures.
- **Prices** are list prices checked on 2026-10-01, before tax. Hetzner raised
  prices in April and June 2026 because of the DRAM and SSD shortage; memory is
  now the most volatile line.

## The Roblox baseline

From Roblox's filings:

| | FY2024 | Q2 2025 |
|---|---|---|
| Infrastructure and trust & safety expense | $915.4 M (includes $113.7 M stock-based compensation) | $260.7 M |
| Hours engaged | 73.5 billion | 27.4 billion |
| Average DAU | 82.9 million | 111.8 million |
| **Per hour engaged** | **$0.0125** ($0.0109 without stock compensation) | **$0.0095** |
| **Per DAU per month** | **$0.92** | **$0.78** |

What the line holds, in Roblox's words: data center operations, third-party
cloud services, co-located facilities, network and bandwidth, depreciation of
servers, plus the people and services of trust and safety (moderation,
customer support). Roblox owns nearly all of its game servers: over 140,000
servers in data centers across 23 cities (10-K, December 2024).

The filing does not split game servers from the rest (asset delivery, avatar,
chat, Studio services, moderation staff). This plan assumes game servers are
**40% to 75%** of the line, which gives the second row of the answer table,
$0.004 to $0.009 per hour. Treat that as a range with wide error bars; the
all-in figure is the one that is filed.

Roblox's scale buys it things a small fleet cannot match: owned hardware
amortized over years, its own network, and 23 regions.

## The options, costed

Per CCU-hour at the middle estimate, with the low to high range where the
assumptions move it. Per 1,000 DAU is that times 73,000 CCU-hours a month,
variable cost only (the fixed floor is in its own section below).

| Option | Per CCU-hour | Per 1,000 DAU per month | Transport change |
|---|---|---|---|
| 1. Creator hosts, players connect directly | about $0.00001 | under $1 | none |
| 2a. Creator hosts, we relay with our own UDP relay | about $0.0001 | about $7 | small (relay) |
| 2b. Creator hosts, Cloudflare Realtime TURN relays | $0.0054 | $394 | WebRTC rewrite |
| 3a. Cloudflare Containers, 1/4 vCPU and 1 GiB per session | $0.0061 | $445 | TCP only |
| 3b. Cloudflare Containers, 1 vCPU and 6 GiB per session | $0.0186 | $1,358 | TCP only |
| 4. Dedicated servers, bandwidth included, packed | **$0.0017** ($0.0005 to $0.0070) | **$122** ($35 to $508) | none |
| 5a. Burst on Hetzner Cloud, hourly | $0.0013 | $95 | none |
| 5b. Burst on AWS spot (Graviton) | $0.0112 | $818 | none |

### 1. Creator hosts, players connect directly (today)

Our cost is the registry: a heartbeat every 30 s (a Worker request and a
Durable Object request each), one admission per join, and the listing page's
polls. About 240 requests per session-hour at $0.15 to $0.30 per million:
under $0.0001 per session-hour, about $0.00001 per player-hour. Worlds for
published simulations come from R2, which charges no egress.

The creator pays with upload bandwidth: 8 players at 30 KB/s is about
2 Mbit/s, so a home connection carries roughly 20 to 40 players at the middle
estimate.

Gives up: the session ends when the creator leaves; the host's address is
exposed to players; NAT and firewalls block many homes (today's tunnel needs
the Cloudflare One client on the player's side); latency depends on where the
creator lives; and the host is trusted, so a host can cheat (players cannot,
because the host decides where everything is).

### 2. Creator hosts, we relay

The relay fixes NAT (the host keeps one outgoing connection to the relay),
hides the host's address, and lets a gallery player join without any client
software.

- **2a, our own relay.** A small Rust UDP forwarder on a dedicated server with
  unlimited traffic. It does not decrypt anything: QUIC stays end to end, and
  the certificate pin the join link carries still proves the host. One 1 Gbit
  port at 60% use forwards about 2,500 players at 30 KB/s. On an AX42-class
  box ($117 a month, $0.16 an hour): $0.00006 per player-hour, about $0.0001
  with the 25% overhead. **This is the cheapest public path and needs no
  change to the transport.**
- **2b, Cloudflare Realtime TURN.** $0.05 per GB delivered after 1,000 GB free
  a month: $0.0054 per player-hour at the middle estimate. It carries WebRTC,
  so the transport would move from WebTransport to WebRTC data channels, a
  rewrite. Half of Roblox's all-in cost: useful as a fallback, not as the
  default.
- **Spectrum to a private origin** (public UDP through the tunnel) would also
  work with no relay of our own, but it is Enterprise-only and its price is
  not public. It cannot be costed yet.

### 3. Cloudflare for game sessions

- **Containers** bill only while running ($0.000020 per vCPU-second,
  $0.0000025 per GiB-second, egress $0.025 per GB in North America and
  Europe). A small session sized at 1/4 vCPU and 1 GiB costs $0.0275 an hour,
  or $0.0061 per player-hour with egress; a full vCPU and 6 GiB costs $0.0186.
  Containers accept HTTP and, since August 2026, inbound TCP through Spectrum;
  they do not take UDP, so a session would run on the WebSocket fallback
  (motion and input on the reliable stream, with head-of-line blocking). The
  only real strength is paying nothing while a world is idle, which suits
  rarely visited worlds, not volume.
- **Durable Objects** cannot run the engine (128 MB, JavaScript or WASM, no
  native Bevy), and that is fine: they already run the registry and
  admission, and they are the right home for matchmaking, presence, lobby
  chat and DataStore. Their cost per player-hour rounds to zero.
- **R2** holds every published world with no egress charge, which matters
  below.

### 4. Dedicated servers with bandwidth included, packed

`eustress-headless` processes, many per box, on servers whose price includes
traffic. Reference box: **Hetzner AX42**, 8 cores and 16 threads, 64 GB DDR5,
**$117.10 a month** plus $59 setup (June 2026 price), unlimited traffic on a
1 Gbit port.

| Case | Threads per session | Memory per session | Sessions per box | Players per session | Average use | Per CCU-hour (with 25% overhead) |
|---|---|---|---|---|---|---|
| Optimistic | 0.2 | 1 GB | 58 (memory-bound) | 12 | 60% | $0.0005 |
| **Middle** | 0.4 | 1 GB | 30 (CPU-bound) | 8 | 50% | **$0.0017** |
| Pessimistic | 1.0 | 3 GB | 12 | 6 | 40% | $0.0070 |

Bandwidth never binds: the middle case sends about 58 Mbit/s from a 1 Gbit
port.

Where the boxes are: Hetzner's dedicated servers are in Germany and Finland
only. US players need a US provider with bandwidth in the price; OVHcloud US
lists Advance-1 (6 cores, 32 GB and up, 1 Gbit public) from $115 a month,
and whether its US public bandwidth is unmetered must be confirmed before
counting on it. Assume US costs 1 to 1.7 times the German figure.

Gives up: a box that fails drops its sessions (no migration yet); a few
regions instead of Roblox's 23 cities, so distant players see higher
latency; basic DDoS protection only; memory prices that rose sharply in 2026
(Hetzner's 128 GB memory add-on went up 575%); and running a fleet is real
work for one person.

### 5. Burst capacity

- **Hetzner Cloud, by the hour.** CCX33: 8 dedicated vCPU, 32 GB, €62.49 a
  month cap, billed by the hour; 20 TB of traffic included in Europe, 1 TB in
  the US, then €1 per TB. About 15 small sessions, about $0.0013 per
  player-hour while it is up. Good for evening peaks.
- **AWS spot.** A c7g.xlarge (4 vCPU, 8 GB) is about $0.057 an hour on spot,
  which is cheap compute, but egress at $0.09 per GB adds $0.0097 per
  player-hour. That is about Roblox's whole all-in figure. Hyperscaler spot is
  for emergencies only, and would need an ARM build of the headless host.

## The recommended mix

| Tier | Runs on | For | Per CCU-hour |
|---|---|---|---|
| Creator-hosted | The creator's Studio, through our UDP relay | Free worlds, friends, testing, small public sessions | about $0.0001 |
| Eustress-hosted | Dedicated boxes in 2 to 3 regions, sized for the median day; Hetzner Cloud hourly for peaks | Always-on and popular gallery worlds, paid tiers, worlds whose creator is offline | about $0.0017 |
| Control plane | Cloudflare Workers, Durable Objects, R2 | Registry, admission, matchmaking, worlds, DataStore | under $0.00005 |

**Blended example:** 70% of hours creator-hosted, 30% on our servers, plus an
allowance of $0.0001 to $0.0003 per player-hour for storage, the control
plane and moderation (Jev and the Grok judge run per publish, not per hour;
per-message chat moderation would add to this):

0.7 × $0.0001 + 0.3 × $0.0017 + $0.0002 = **about $0.0008 per player-hour,
about $56 per 1,000 DAU a month.**

Against Roblox's all-in $0.0095 to $0.0125: **92% to 94% cheaper.** Against
the estimated game-server share alone ($0.004 to $0.009), counting only our
hosting ($0.0006): 85% to 93% cheaper.

How the result moves:

| Creator-hosted share of hours | Our servers at the middle density | Our servers at the optimistic density |
|---|---|---|
| 0% | $0.0019, 80% to 85% cheaper | $0.0007, 93% to 94% cheaper |
| 50% | $0.0011, 88% to 91% cheaper | $0.0005, 95% to 96% cheaper |
| 70% | $0.0008, 92% to 94% cheaper | $0.0004, 96% to 97% cheaper |

So "beat 90%" holds at the optimistic density whatever creators host, and at
the middle density once creators host more than about half of all hours (the
50% row sits on the line). Raising density is the strongest lever we
control, and
it is mostly memory: a headless host that loads no render assets, and several
sessions of the same world sharing one copy of its read-only data, cut the
memory per session.

### The fixed floor

Variable cost is small enough that the minimum fleet dominates early on. Two
regions, each with one game box and one relay, is about $470 a month whatever
the DAU. At 1,000 DAU (about 100 players on average, 200 at peak) one middle-
density box per region holds the whole peak. So:

- **Below about 10,000 DAU:** creator hosting with our relay, one game box in
  one region for worlds that must stay up, Hetzner Cloud by the hour as the
  spare.
- **From 10,000 DAU:** a second and third region, boxes added as peak
  concurrency grows. The fixed floor is about half the bill at 10,000 DAU
  and about a tenth at 100,000 DAU.

## What we must build or measure

In order:

1. **The stress test** (designed in [STRESS_TEST.md](../networking/STRESS_TEST.md), not built). It produces the
   numbers this plan assumes: CPU and memory per session and per player for
   Pong, Box Head and Vehicle Simulator, and bytes per player per second on
   the motion and world lanes. Run it against `eustress-headless`, not only
   Studio.
2. **A headless gallery host:** `eustress-headless` hosting a published world
   from its `.echk` chunks, with its own credential, the heartbeat and
   admission. Gate 11 of the gallery goal.
3. **Worlds from R2, not from the host.** Today the host sends the world as
   `ChunkPiece` messages at join. Fetching published chunks from R2 (free
   egress) takes the largest single transfer off every host, creator-hosted or
   ours.
4. **The UDP relay** for creator-hosted sessions, with per-session limits and
   abuse controls. Until it exists, public creator-hosted play waits on
   Spectrum's access and price.
5. **A box supervisor and placement:** one process per session for isolation,
   a supervisor per box, and placement chosen by a Durable Object (Forge's
   scheduler in `crates/forge` can grow into this).
6. **Metering:** CCU-hours, egress and peak concurrency per session and per
   box, so this plan's numbers can be replaced with measured ones.
7. **Open questions to answer before buying:** Spectrum's price; whether US
   bare-metal offers (OVHcloud US, others) have truly unmetered public
   bandwidth; how far memory prices move.

## Where the older plans differ

[INFRASTRUCTURE.md](./INFRASTRUCTURE.md) and [EUSTRESS_FORGE.md](./EUSTRESS_FORGE.md) estimate Roblox at "$500M+ a year,
80 million DAU, 68 billion hours" and Eustress at "$1,136 a month for 1
million users". The filed FY2024 figures are $915.4 M, 82.9 M DAU and 73.5
billion hours. The older Eustress estimate sizes servers by user count, leaves
out bandwidth and memory, and uses Hetzner Cloud prices from before the 2026
increases; 1 million DAU is about 100,000 average concurrent players, which
this plan's middle density puts at about 1,060 boxes, about $124,000 a month
on our own servers alone, or about $58,000 a month for the blended mix.

## Sources

- Roblox, Form 10-K for FY2024: [sec.gov](https://www.sec.gov/Archives/edgar/data/1315098/000131509825000033/rblx-20241231.htm) (82.9 M average DAU, 73.5 billion hours, over 140,000 servers in 23 cities); FY2024 infrastructure and trust & safety of $915,418 thousand with $113,708 thousand stock-based compensation, as reported in the Q4 2024 results ([Q4 2024 call](https://www.fool.com/earnings/call-transcripts/2025/02/06/roblox-rblx-q4-2024-earnings-call-transcript/)).
- Roblox, Q2 2025 shareholder letter exhibit: [sec.gov](https://www.sec.gov/Archives/edgar/data/1315098/000131509825000261/rblx-20250630xexhibit991.htm) ($260,684 thousand infrastructure and trust & safety; 111.8 M DAU; 27.4 billion hours).
- Roblox server memory allowance, 6.4 GB plus 100 MB per player: [Roblox DevForum](https://devforum.roblox.com/t/even-more-server-memory/3293192); bandwidth guidance (optimize above 100 KB/s): [Roblox DevForum](https://devforum.roblox.com/t/the-bandwith-art-learning-how-to-optimize-and-improve-your-game-network-performance/3865361).
- Cloudflare Containers pricing: [developers.cloudflare.com](https://developers.cloudflare.com/containers/pricing/); Durable Objects pricing: [developers.cloudflare.com](https://developers.cloudflare.com/durable-objects/platform/pricing/); Realtime TURN: [developers.cloudflare.com](https://developers.cloudflare.com/realtime/turn/); inbound TCP to Workers and Containers: [blog.cloudflare.com](https://blog.cloudflare.com/grpc-workers/).
- Hetzner dedicated servers and unlimited traffic: [hetzner.com](https://www.hetzner.com/dedicated-rootserver/matrix-ax/); AX42 price from 15 June 2026: [whtop.com](https://www.whtop.com/amp/plans/hetzner.com/128296); 2026 price adjustment: [docs.hetzner.com](https://docs.hetzner.com/general/infrastructure-and-availability/price-adjustment/); Hetzner Cloud plans and traffic: [betterstack.com](https://betterstack.com/community/guides/web-servers/hetzner-cloud-review/), [costgoat.com](https://costgoat.com/pricing/hetzner).
- OVHcloud US dedicated servers: [us.ovhcloud.com](https://us.ovhcloud.com/bare-metal/prices/); Game range price rise: [techradar.com](https://techradar.com/pro/ovh-cloud-tells-users-to-expect-87-percent-price-rise-with-gaming-servers-hit-particularly-hard).
- AWS egress and spot prices: [egresscost.com](https://egresscost.com/aws/data-transfer-pricing), [holori.com](https://calculator.holori.com/aws/ec2/c7g.xlarge/us-east-1).
