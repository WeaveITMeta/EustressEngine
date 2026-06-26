# Eustress Engine

<p align="center">
  <img alt="License: Eustress Community" src="https://img.shields.io/badge/License-Eustress_Community-blue.svg">
  <img alt="Made with Rust" src="https://img.shields.io/badge/Made_with-Rust-orange.svg?logo=rust&logoColor=white">
  <img alt="Bevy 0.19" src="https://img.shields.io/badge/Bevy-0.19-232326.svg">
  <img alt="UI: Slint" src="https://img.shields.io/badge/UI-Slint-2379f4.svg">
  <img alt="Platform: Windows | macOS | Linux" src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg">
  <a href="https://discord.gg/FztQJJacbw"><img alt="Discord" src="https://img.shields.io/badge/Discord-Join-5865F2.svg?logo=discord&logoColor=white"></a>
</p>

> **The platform Roblox should have been.** A universal world-model engine, source-available, forkable, AI- and MCP-native, and photoreal, built to **simulate the world, not just render a scene.**

<p align="center">
  <img src="docs/marketing/screenshot.png" alt="Eustress Engine studio: a live V-Cell battery simulation in the native 3D viewport, with the scene Explorer, real-time Properties, and the built-in AI Workshop assistant" width="900">
</p>

<p align="center"><em>The Eustress Engine studio in action: a native Bevy 3D viewport running a live <strong>V-Cell</strong> battery-monitor simulation, alongside the scene Explorer, real-time Properties, and the built-in <strong>AI Workshop</strong> assistant, all in a single Rust + Slint window.</em></p>

## Mission

Eustress exists to be the **open substrate for modeling reality**, a world-model engine general enough that _anything you can describe, you can build and run inside it_. Games are the on-ramp; the destination is everything else a simulation can become: training grounds for AI agents, living digital twins of factories and markets, governance and justice models, and laboratories where scientists and engineers validate their own theories against a photoreal, real-time world.

It rests on a single load-bearing bet: **the engine simulates the world rather than rendering a scene.** Render-first engines draw what you tell them to draw; Eustress is built to _compute what would actually happen_, with millions of entities evolving under real, rewritable laws. If that bet holds, Eustress is infrastructure and games are merely the first thing built on top of it. If it doesn't, it's one more engine in a crowded field. Everything else follows from that one fact.

And it must be **owned by the people who build it.** Eustress is the platform Roblox should have been, but source-available, forkable, and merit-paid, so the builders who create the worlds and the engineers who extend the engine capture the value they create instead of renting it back from a landlord.

## What it is

Eustress is **not (only) a game engine.** It's a general-purpose **simulation and proof-of-work substrate** that anything can be built on: AI model training, manufacturing dashboards, financial-market visualization, government and justice systems, kernel-law science validation, life-science research, and games as one use case among many. If you can model it, you can build it inside Eustress.

## Why it's different

- **Simulation-first.** Built to drive millions of entities under real kernel-level laws; the design target is order-of-_a-year-of-simulation-per-second_ throughput, not just frames on screen.
- **Source-available & forkable.** Closed source kills the moat; an open community ships faster. Read it, fork it, embed it, rewrite it — free until you reach commercial scale.
- **AI-native.** A built-in **Workshop** AI assistant plus a **Model Context Protocol (MCP)** bridge let AI agents inspect, drive, and build inside a _live_ world, even from their own independent off-screen camera, so the AI can _see_ what it is making and iterate alongside you.
- **Kernel laws: the gold-collar unlock.** Engineers and scientists can rewrite how the engine processes physics, chemistry, and more _at the kernel level_ to validate their **own** simulations (e.g. the bundled V-Cell solid-state-battery model). Lose this and it is just a game engine; this is _the_ unlock.
- **Photoreal & native.** One Rust window: a native **Bevy** 3D viewport with a declarative **Slint** UI overlay, and no web stack, no IPC, no overhead.
- **Data-native.** The same studio that builds a world also ingests, models, and charts data: `Dataset`s are first-class instances with live stats, curve fits, and interactive charts, so a digital twin's telemetry lives in the same scene as the parts it describes.

## Principles (the non-negotiables)

1. **Source-available & forkable.** Not a slogan, but a velocity thesis: an open community ships faster than any closed team, and closing the source would kill the moat. Eustress stays free to read, fork, and modify; a commercial license is required only at large scale (see [License](#license)).
2. **The gold-collar unlock.** Engineers and scientists can rewrite the engine's **kernel laws** (the physics, chemistry, and rules that govern a simulation) to validate their _own_ models inside a real-time, photoreal world. This is the unlock everything else serves.
3. **The two-token wall.** Buyer-money and builder-money never share a denomination, and the split that funds builders is **constitutional, not a tunable cut**, the lever Roblox kept for itself and refused here by design.
4. **Merit only.** Standing comes from contribution, not connections: a meritocracy, automated and open, not an oligarchy.

## Architecture

| Layer | Choice |
|---|---|
| Language | 100% Rust |
| Render core | Bevy 0.19 |
| UI | Slint (declarative, native) |
| Physics | Avian |
| World store | Binary, log-structured **WorldDb** on [Fjall](https://github.com/fjall-rs/fjall) (LSM-tree), holding live entity state as compact records so a world scales to millions of entities and loads fast |
| Platforms | Desktop (Windows, macOS, Linux); mobile player in progress |

Eustress is a Rust monorepo. The most important crates (`eustress/crates/`):

| Crate | Role |
|---|---|
| `engine` | Desktop 3D editor / studio: viewport, Explorer, Properties, build & transform tools |
| `client` · `player-mobile` | Generative player / renderer |
| `common` | Shared scene format, instance classes, services, units, and realism / kernel laws |
| `worlddb` · `eustress-fjall` | Binary simulation store (the Fjall LSM-tree `WorldDb`) |
| `data` | Data Platform analytics leaf: columnar frames, stats, curve fits, and clustering (Polars/Arrow), bridged into the world store |
| `mcp` · `mcp-server` | Model Context Protocol, letting AI inspect and drive the live engine |
| `workshop` | Built-in AI Workshop assistant |
| `cad` | CAD / B-rep kernel (via `truck`) |
| `mesh-edit` | Half-edge mesh editing (extrude, inset, …) |
| `embedvec` · `spatial-llm` | Vector + spatial AI |
| `stream` · `stream-node` | Real-time streaming |
| `bliss` · `identity` · `server` · `web` | Economy, identity, backend, and web surfaces |

## Prerequisites

- Rust (latest stable) and Cargo

## Quick start

```bash
cd eustress

# Run the studio (editor)
cargo run-studio

# Run the player (generative client)
cargo run-client
```

There is also a helper script: `./build-and-run.ps1 engine` (or `client`).

### Production build

```bash
cd eustress
cargo build --workspace --release      # binaries → eustress/target/release/
```

## Studio features

- Native **Bevy 3D viewport** with a **Slint** overlay, in a single window with zero IPC
- **Scene Explorer** hierarchy + **real-time Properties** editor
- **Move / Rotate / Scale** gizmos and smart build tools
- **Live AI co-creation**: the Workshop assistant and MCP bridge let AI build with you, with its own independent camera to view its work
- **Kernel-law realism** sections (thermodynamic, electrochemical, …) attached per entity
- **Data Platform**: `Dataset` instances with a live Schema/Stats Properties inspector, interactive charts, and a Data Grid (see [Data Platform](#data-platform))
- Console / output panel, undo history, and a timeline

## Data Platform

The same studio that builds a 3D world is also a **data workbench**. Eustress treats data as a first-class citizen, so a digital twin's telemetry, an experiment's measurements, or a market's history live in the same scene as the parts they describe, on the same store, under the same tools.

- **Datasets are instances.** A `Dataset` sits in the Explorer alongside `Part` and `Light`, under a `DataService`, nesting `Series` (columns / timeseries) and `Run` (scenarios) the way a Model nests parts.
- **One polymorphic inspector.** Select a Dataset and the same Properties panel that shows a Part's Appearance / Physics shows the data's **Schema**, **Source & provenance**, **live Stats** (n, mean, min/max, σ), and **Storage**, computed on the fly from the backing data.
- **Interactive charts & grids.** A Dataset opens as a chart tab: an auto-scaling plot with point hover, a least-squares fit and its equation, Chart / Grid / Split views, and adjustable axes, plus a spreadsheet-style Data Grid.
- **Analysis built in.** The `eustress-data` crate (Polars / Arrow-backed) supplies the stats, curve fits, and clustering (k-means, kNN) that the **Data** ribbon runs on the selected Dataset.
- **Connect & persist.** A `Connector` configures an external source (CSV, REST, stream); datasets, parts, and simulation state all persist in the same copy-on-write **WorldDb**, so you can fork a world and rehearse a scenario against real data.

It is **domain-agnostic**: climate modeling is the first tenant, but nothing about a factory line, a portfolio, or a genome is baked into the engine.

## Project structure

```
eustress/                  # Cargo workspace
├── Cargo.toml
├── crates/
│   ├── engine/            # Desktop editor / studio
│   ├── client/            # Player / renderer
│   ├── common/            # Scene format, classes, kernel laws
│   ├── worlddb/           # Binary WorldDb trait
│   ├── eustress-fjall/    # Fjall LSM-tree backend
│   ├── data/              # Data Platform: frames, stats, fits, clustering
│   ├── mcp-server/        # MCP server (AI tooling)
│   ├── workshop/          # AI Workshop assistant
│   ├── cad/  mesh-edit/   # CAD + mesh kernels
│   └── …                  # embedvec, spatial-llm, stream, bliss, identity, web, …
├── assets/
└── docs/
```

## The economy that pays its builders

Eustress is designed so contribution converts to income **without the platform skimming**, a real labor market, not a company store:

- **Two-token wall.** **Tickets** (bought in USD, spent across the publication gallery) are buyer-money; **Bliss** (earned through contribution, cash-outable to USD) is builder-money. They never share a denomination.
- **Constitutional 50/50.** Half of every Ticket dollar structurally funds builders, half funds the engine, written into how the tokens work, not a cut the platform can quietly change later (the "Robux mistake" Eustress refuses by design).
- **Merit-only ladder.** Install the engine, contribute (PRs, fixes), earn rank, become a Contributor, earn Bliss, publish, and cash out. Demand flows in, the constitution routes half to merit, and contribution draws it back out as cash.

## Contributing

Eustress is source-available and merit-based; that is the velocity thesis, not a slogan. Install it, find something that bugs you, and open a PR. Contribution is the on-ramp to the ladder above; rank and Bliss follow the work.

## License

**Source-available and free** under the **Eustress Community License** — use it, modify it, fork it, and ship it at no cost. A **commercial license is required only once your product crosses a scale threshold** (currently **US $1M annual revenue _or_ 100k monthly active users** — see the [LICENSE](LICENSE) for the exact terms). The engine stays free for individuals, teams, and the community; large-scale commercial users support its development.

> Third-party dependencies (Bevy, Slint, and other crates) retain their own permissive licenses (MIT / Apache-2.0).
