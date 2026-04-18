# Eustress MCP Server — Architecture

## One-line summary

A Model Context Protocol server (`@eustress/mcp-server`) that exposes the
Eustress Universe — Spaces, Rune scripts, entities, assets, git state,
Workshop conversation history — to any MCP-compatible AI client
(Windsurf, Claude Desktop, Cursor, Zed, etc.) through a curated set of
tools.

## Why this exists

Eustress's entire project model is file-system-first: a Universe is a
directory, Spaces are subdirectories, scripts are `.rune` files,
entities are `.toml` files. This is identical to the shape most MCP
servers operate on (e.g. `@modelcontextprotocol/server-filesystem`),
which means exposing the project over MCP is a natural extension rather
than a translation layer.

Once an external IDE (primarily Windsurf, but any MCP client) can see
your Universe the way Eustress Studio does, its AI assistant can:

- Navigate Spaces and Scripts by their Universe identity, not raw paths.
- Read a script's source + summary together.
- Find an entity by name without grepping.
- Search across `.rune` and `.toml` with class/kind awareness.
- Inspect git history with Universe-aware filters.
- Write new scripts using the canonical folder-matching layout.
- Resume a prior Workshop conversation that Studio persisted.

## Why TypeScript (not Rust like LSP)

**LSP** benefits from living in Rust — it shares the `analyzer` module
with Studio's in-editor IDE features, and the LSP spec assumes a
single-binary, high-throughput server.

**MCP** benefits from TypeScript because:

1. The official SDK (`@modelcontextprotocol/sdk`) is TS-first, and every
   other marketplace entry is TS or Python. Users installing via
   Windsurf's marketplace expect `npx`-style invocation.
2. The tool surface is thin: each tool is `(args) → result`. Rust's
   expressive tool registry doesn't translate directly to MCP's JSON
   schema-only arg contracts.
3. The server is effectively a **read-only projection** of the Universe
   for v1 — no ECS, no simulation, no rendering. TypeScript's fs/path
   primitives handle that trivially without needing to build a Rust
   binary.

When the engine is running (Phase 2+), this TS server can proxy a
subset of tools to an HTTP endpoint the engine exposes — exactly the
"live ECS context" extension the LSP architecture also anticipates.
Same pattern, same opt-in flag.

## Topology

```
┌──────────────────────────────────────────────────────────────┐
│  Windsurf / Claude Desktop / Cursor / Zed                    │
│                                                              │
│   ┌──────────────────────────────────┐                       │
│   │  MCP client (built into IDE)     │                       │
│   └──────────────┬───────────────────┘                       │
│                  │ spawn(stdio)  (npx @eustress/mcp-server)  │
│                  ▼                                           │
│   ┌──────────────────────────────────┐                       │
│   │  Eustress MCP server (Node/TS)   │                       │
│   │  - universe helpers              │                       │
│   │  - tool registry (11 tools v1)   │                       │
│   └──────────────┬───────────────────┘                       │
│                  │ reads                                     │
│                  ▼                                           │
│     {universe}/Spaces/*/.../{_instance.toml,*.rune}          │
│     {universe}/.eustress/knowledge/  (Workshop archive)      │
│     {universe}/.git/                  (git plumbing)         │
│                                                              │
│                  │ optional (Phase 2)                        │
│                  ▼                                           │
│     localhost:24786 — running Eustress Studio HTTP API       │
│     (live ECS, simulation state, unsaved buffers)            │
└──────────────────────────────────────────────────────────────┘
```

## Resources (v2)

MCP has three capability families: **tools** (actions the AI invokes),
**resources** (addressable content the AI pins and refers back to), and
**prompts** (templated messages). We now implement all three, with
resources doing the heavy lifting for "things the AI wants to remember
about your project."

### @mention ↔ resource parity

Workshop's `@mention` system has a canonical-path notion:

```
@entity:Space1/V-Cell/Housing
@script:Space1/SoulService/cycle_life_test
@file:Space1/docs/design.md
```

MCP resource URIs are the same thing under an `eustress://` prefix:

```
eustress://entity/Space1/V-Cell/Housing
eustress://script/Space1/SoulService/cycle_life_test
eustress://file/Space1/docs/design.md
```

Anything you can @-mention in Studio's Workshop, you can pin as a
resource in an external IDE's chat. Symmetric context between the
in-Studio AI and the external one.

### URI scheme

| Kind           | URI shape                                    | Content                                                                  |
|----------------|----------------------------------------------|--------------------------------------------------------------------------|
| `space`        | `eustress://space/{space}`                   | Markdown overview — services, script counts, top-level listing.          |
| `script`       | `eustress://script/{space}/{+path}`          | Bundled source + summary in one markdown document (what the AI pins when referring to "that script"). |
| `entity`       | `eustress://entity/{space}/{+path}`          | `_instance.toml` + class + name.                                         |
| `file`         | `eustress://file/{space}/{+path}`            | Raw text file under a Space (`.md`, `.toml`, `.rune` raw). Binary extensions rejected. |
| `conversation` | `eustress://conversation/{session_id}`       | Persisted Workshop session JSON from `.eustress/knowledge/sessions/`.    |
| `brief`        | `eustress://brief/{product}`                 | `ideation_brief.toml` addressed by product folder name.                  |

`resources/templates/list` advertises these so clients can construct
URIs for items they learn about out-of-band — e.g. "I see a script at
`Space1/SoulService/foo`; let me pin
`eustress://script/Space1/SoulService/foo`."

### Live updates via subscription

We advertise `resources.subscribe = true`. Clients call `resources/subscribe`
with a URI they've pinned; our file watcher (backed by `chokidar` for
cross-platform correctness) emits `notifications/resources/updated` the
moment the underlying file changes. That's the killer-feature difference
between resources and tools: **pinned resources stay fresh without
re-invocation.** Save in Studio → external AI sees the new content
immediately.

The watcher is lazy — no subscribers, no watcher process. When the
default Universe changes mid-session (via `eustress_set_default_universe`),
the subscription manager re-targets seamlessly.

### What's intentionally NOT a resource

- **Live ECS state / simulation values.** These are moving targets the
  AI wants to *query at a moment*, not *keep referring to*. They belong
  as Phase-2 tools (via the engine's TCP bridge).
- **Binary assets as base64.** A 50 MB GLB over stdio is a bad time.
  Meshes/textures surface through `eustress_list_assets` (which returns
  metadata, not bytes) and a future `eustress_read_binary` tool if the
  AI genuinely needs pixels.
- **All-of-Universe bundles.** Each resource is a single file-ish thing.
  "The entire Space1" isn't a resource; it's a browse target the client
  walks through `resources/list` pagination.

## Tool surface (v1)

Every tool is:
- **Universe-rooted** — takes `universe` path as input or reads it from
  the MCP server's launch config (`EUSTRESS_UNIVERSE` env var or
  `--universe` CLI arg).
- **Read-only** by default. `eustress_create_script` is the only
  writer; it uses the same canonical folder-matching layout Studio uses.
- **Stateless across calls** — the server re-reads the filesystem per
  request, so a Studio save is visible on the next tool call without
  restarting the MCP server.

| Tool                          | Purpose                                                 |
|-------------------------------|---------------------------------------------------------|
| `eustress_list_spaces`        | Every Space under `{universe}/Spaces/`.                 |
| `eustress_list_scripts`       | All Rune script folders in a Space (or all Spaces).     |
| `eustress_read_script`        | Source + summary of one script, resolved via canonical name (falls back to legacy `Source.rune` / `Summary.md`). |
| `eustress_find_entity`        | Find an entity by name across a Space — returns path + class. |
| `eustress_list_assets`        | Enumerate assets (meshes / textures / GUIs) in a Space. |
| `eustress_search_universe`    | Text search across `.rune` and `.toml` files.           |
| `eustress_git_status`         | `git status --porcelain` scoped to the Universe.        |
| `eustress_git_log`            | Recent commits touching the Universe.                   |
| `eustress_git_diff`           | Uncommitted diff for one path or the whole Universe.    |
| `eustress_create_script`      | Write `folder/{folder}.rune + {folder}.md + _instance.toml` using the canonical layout. |
| `eustress_get_conversation`   | Load a Workshop conversation from `.eustress/knowledge/`. |

Tool names are prefixed `eustress_` so they can't collide with other
MCP servers a user has installed (e.g. `fs_` or `git_` helpers).

## Distribution

| Artifact | Hosted at |
|---|---|
| `@eustress/mcp-server` on npm | `npmjs.com/package/@eustress/mcp-server` |
| Windsurf marketplace listing | `windsurf marketplace → MCP → Eustress Engine` |
| Source | `infrastructure/mcp/server/` in this repo |
| Windsurf config template | `infrastructure/mcp/config/windsurf.json` |

Users install it either via the Windsurf marketplace (one click) or
manually by copying the config snippet into
`~/.codeium/windsurf/mcp_config.json`.

## Tool contract invariants

- **Deterministic output shape.** Every tool returns `{ content: [...] }`
  per MCP convention. Text tools return `type: "text"`. Listings return
  JSON-serialised blocks so clients can parse without heuristics.
- **Bounded results.** Listings cap at 500 items; if the user wants
  more, they re-run with a narrower filter. Prevents response-size
  blowouts the client would truncate anyway.
- **Path safety.** Every input path is resolved + checked against the
  Universe root before fs access. A tool call with
  `path: "../../etc/passwd"` errors cleanly rather than escaping the
  jail.

## Extending the tool set

1. Add the schema + implementation in `server/src/tools.ts`.
2. Register it in the `TOOLS` table.
3. Bump the `version` field in `package.json` and the
   `server_info.version` in the capability response.
4. Open a PR. If it needs live engine state (ECS, simulation), gate it
   behind the Phase 2 HTTP-bridge check and surface a clear error when
   the engine isn't running.

## Phase 2 — Engine bridge

When Eustress Studio is running with `--mcp-port 24786` (planned), the
server probes `localhost:24786/health` on startup. If reachable, extra
tools appear in the capability list:

- `eustress_query_ecs` — entity snapshot with components.
- `eustress_get_sim_value` — live simulation readings.
- `eustress_execute_rune` — run a Rune expression in the Studio VM.

Absent the engine, these tools don't appear — the client never sees
them, so AI agents don't try to call broken endpoints. Zero
configuration from the user's perspective.

## Paths in this repo

```
infrastructure/mcp/
├── ARCHITECTURE.md           (this file)
├── README.md                 developer + user quick-start
├── server/                   TypeScript MCP server source
│   ├── package.json
│   ├── tsconfig.json
│   ├── .gitignore
│   ├── src/
│   │   ├── index.ts          stdio transport + handler registration
│   │   ├── tools.ts          tool registry + schemas
│   │   └── universe.ts       fs helpers over Universe/Spaces/Scripts
│   └── README.md             (shown on npm listing)
└── config/
    └── windsurf.json         copy-paste snippet for Windsurf
```
