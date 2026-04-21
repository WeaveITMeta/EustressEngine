# Eustress MCP Server — Architecture

## One-line summary

A [Model Context Protocol](https://modelcontextprotocol.io) server that exposes
the Eustress Universe — Spaces, Rune scripts, entities, assets, git state,
Workshop conversation history — to any MCP-compatible AI client (Windsurf,
Claude Desktop, Cursor, Zed) through a curated set of tools and live resources.

Ships as a native Rust binary (`eustress-mcp`, ~2.7 MB stripped) alongside
`eustress-engine` and `eustress-lsp`. Same `cargo build --release`, same
installer bundle, zero runtime dependencies.

## Why this exists

Eustress's project model is file-system-first: a Universe is a directory,
Spaces are subdirectories, scripts are `.rune` files, entities are `.toml`
files. Exposing the project over MCP is therefore a natural extension rather
than a translation layer — the wire format IS the filesystem layout.

Once an external IDE can see your Universe the way Eustress Engine does, its
AI assistant can:

- Navigate Spaces and Scripts by their Universe identity, not raw paths.
- Read a script's source + summary together.
- Find an entity by name without grepping.
- Search across `.rune` and `.toml` with class/kind awareness.
- Inspect git history with Universe-aware filters.
- Write new scripts using the canonical folder-matching layout.
- Resume a prior Workshop conversation that the engine persisted.

## Why Rust (the April 2026 rewrite)

v1 shipped as TypeScript on top of the official MCP SDK. That was the
fastest path to a feature-complete server when the SDK was TS-first. By
early 2026 the numbers made the rewrite obvious:

| | Bun-compiled TS | Rust |
|---|---:|---:|
| Binary size | 111 MB | **2.7 MB** (41× smaller) |
| Startup | ~350 ms | ~8 ms |
| Memory (idle) | ~60 MB | ~6 MB |
| Runtime deps | Bun runtime baked in | None |
| Source deps | `@modelcontextprotocol/sdk`, `chokidar`, `zod` | `serde_json`, `tokio`, `notify`, `toml` |
| Ships with engine? | Separately built, copied into installer | `cargo build --release` produces it alongside `eustress-engine` |

Hand-rolled JSON-RPC 2.0 framing over stdio keeps the surface minimal —
MCP is a tight spec and the SDK was adding bulk rather than value. The
`notify` crate replaces `chokidar` for cross-platform file watching; the
real `toml` parser replaces the TS version's per-line substring scan.

Feature-identical to v1: 13 tools, 6 resource kinds, live subscriptions,
dynamic Universe resolution, URI scheme matching Workshop `@mention`
canonical paths.

## Topology

```
┌──────────────────────────────────────────────────────────────┐
│  Windsurf / Claude Desktop / Cursor / Zed                    │
│                                                              │
│   ┌──────────────────────────────────┐                       │
│   │  MCP client (built into IDE)     │                       │
│   └──────────────┬───────────────────┘                       │
│                  │ spawn(stdio) — eustress-mcp.exe           │
│                  ▼                                           │
│   ┌──────────────────────────────────┐                       │
│   │  Eustress MCP server (Rust)      │                       │
│   │  - JSON-RPC 2.0 over stdio       │                       │
│   │  - universe fs helpers           │                       │
│   │  - 13 tools + 6 resource kinds   │                       │
│   │  - notify-backed file watcher    │                       │
│   └──────────────┬───────────────────┘                       │
│                  │ reads                                     │
│                  ▼                                           │
│     {universe}/Spaces/*/.../{_instance.toml,*.rune}          │
│     {universe}/.eustress/knowledge/  (Workshop archive)      │
│     {universe}/.git/                  (git plumbing)         │
│                                                              │
│                  │ optional (Phase 2)                        │
│                  ▼                                           │
│     localhost:24786 — running Eustress Engine HTTP API       │
│     (live ECS, simulation state, unsaved buffers)            │
└──────────────────────────────────────────────────────────────┘
```

## Module layout

```
eustress/crates/mcp-server/
├── Cargo.toml
├── ARCHITECTURE.md          this file
├── README.md                quick-start for end users
└── src/
    ├── main.rs              stdio entry + JSON-RPC 2.0 dispatch loop
    ├── uri.rs               eustress:// URI parse/build + RFC 6570 templates
    ├── universe.rs          fs helpers — spaces, scripts, entities, search
    ├── resources.rs         6 resource resolvers + path→URI reverse map
    ├── tools.rs             13 tools — schemas + handlers
    └── watcher.rs           notify + subscription manager (lazy-started)
```

Each module matches one responsibility. `main.rs` knows the wire protocol
but knows nothing about Eustress concepts; `tools.rs` and `resources.rs`
know Eustress but nothing about MCP framing. That split is what makes the
binary small: you can audit every transport byte in a single file, and
every filesystem touch in another.

## Resources

MCP has three capability families: **tools** (actions the AI invokes),
**resources** (addressable content the AI pins and refers back to), and
**prompts** (templated messages). This server implements tools + resources
fully; prompts is present but empty, solely so client probes don't get
"method not found."

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

Anything you can @-mention in the engine's Workshop, you can pin as a resource
in an external IDE's chat. Symmetric context between the in-engine AI and
the external one.

### URI scheme

| Kind           | URI shape                                    | Content                                                                  |
|----------------|----------------------------------------------|--------------------------------------------------------------------------|
| `space`        | `eustress://space/{space}`                   | Markdown overview — services, script counts, top-level listing.          |
| `script`       | `eustress://script/{space}/{+path}`          | Bundled source + summary in one markdown document.                       |
| `entity`       | `eustress://entity/{space}/{+path}`          | `_instance.toml` + class + name.                                         |
| `file`         | `eustress://file/{space}/{+path}`            | Raw text file under a Space (`.md`, `.toml`, `.rune`). Binary rejected.  |
| `conversation` | `eustress://conversation/{session_id}`       | Persisted Workshop session JSON from `.eustress/knowledge/sessions/`.    |
| `brief`        | `eustress://brief/{product}`                 | `ideation_brief.toml` addressed by product folder name.                  |

`resources/templates/list` advertises these so clients can construct URIs
for items they learn about out-of-band — e.g. "I see a script at
`Space1/SoulService/foo`; let me pin
`eustress://script/Space1/SoulService/foo`."

### Live updates via subscription

The server advertises `resources.subscribe = true`. Clients call
`resources/subscribe` with a URI they've pinned; the file watcher (backed
by the `notify` crate with a 120 ms `notify-debouncer-full` window to
coalesce write-then-rename saves) emits `notifications/resources/updated`
the moment the underlying file changes.

That's the killer-feature difference between resources and tools:
**pinned resources stay fresh without re-invocation**. Save in Eustress Engine →
external AI sees the new content immediately.

The watcher is lazy:

- No subscribers → no watcher.
- First subscribe on an unwatched Universe starts one recursive watch on
  `Spaces/` plus a non-recursive watch on `.eustress/knowledge/sessions/`.
- Default-Universe swap (via `eustress_set_default_universe`) retargets the
  watcher in place — old handles dropped, new ones opened.
- Last unsubscribe tears the watcher down.

Outgoing notifications share stdout with tool responses. A
`tokio::sync::Mutex<Stdout>` serialises writes so two concurrent emissions
can't interleave bytes mid-JSON.

### What's intentionally NOT a resource

- **Live ECS state / simulation values.** These are moving targets the AI
  wants to *query at a moment*, not *keep referring to*. They belong as
  Phase-2 tools over the engine's TCP bridge.
- **Binary assets as base64.** A 50 MB GLB over stdio is a bad time.
  Meshes/textures surface through `eustress_list_assets` (metadata only)
  and will get a future `eustress_read_binary` tool if needed.
- **All-of-Universe bundles.** Each resource is a single file-ish thing.
  "The entire Space1" is a browse target the client walks through
  `resources/list`.

## Tool surface

Every tool is:

- **Universe-rooted** — takes `universe` as input, or resolves from a
  `path` arg, or falls back to the server's default (`EUSTRESS_UNIVERSE`
  env / `--universe` CLI / `eustress_set_default_universe` / cwd walk).
- **Read-only** by default. `eustress_create_script` is the only writer;
  it uses the engine's canonical folder-matching layout.
- **Stateless across calls** — the server re-reads the filesystem per
  request, so a save in Eustress Engine is visible on the next tool call without
  restarting the server.

| Tool                            | Purpose                                                 |
|---------------------------------|---------------------------------------------------------|
| `eustress_list_universes`       | Discover Universes under search roots + cwd.            |
| `eustress_set_default_universe` | Change the default Universe mid-session.                |
| `eustress_list_spaces`          | Every Space under `{universe}/Spaces/`.                 |
| `eustress_list_scripts`         | All Rune script folders in a Space (or all Spaces).     |
| `eustress_read_script`          | Source + summary of one script, canonical + legacy fallbacks. |
| `eustress_find_entity`          | Find an entity by name across a Space — path + class.   |
| `eustress_list_assets`          | Enumerate assets (meshes / textures / GUIs / audio).    |
| `eustress_search_universe`      | Text search across `.rune`, `.toml`, `.md`.             |
| `eustress_git_status`           | `git status --porcelain` scoped to the Universe.        |
| `eustress_git_log`              | Recent commits touching the Universe (default 20).      |
| `eustress_git_diff`             | Uncommitted diff for one path or the whole Universe.    |
| `eustress_create_script`        | Write `folder/{folder}.rune + {folder}.md + _instance.toml`. |
| `eustress_get_conversation`     | Load a Workshop conversation from `.eustress/knowledge/`. |

Tool names are prefixed `eustress_` so they can't collide with other MCP
servers a user has installed (e.g. `fs_` or `git_` helpers).

## Universe resolution

Precedence, first match wins:

1. Explicit `universe` arg in the tool call.
2. Walk up from a `path` arg until a directory with `Spaces/` is found.
3. `state.current_universe` — the session default (launch config or
   `eustress_set_default_universe`).
4. Walk up from process CWD.
5. (For `resources/list` / `resources/read` only) shallow scan of
   `EUSTRESS_UNIVERSES_PATH` search roots.

If none resolve, `resources/list` returns a single synthetic
`eustress://help/setup` resource with markdown explaining how to point the
server at a Universe — better than an empty list that would leave the LLM
guessing.

## Distribution

| Artifact                    | Location                                                        |
|-----------------------------|-----------------------------------------------------------------|
| Source                      | `eustress/crates/mcp-server/` in this repo                      |
| Build command               | `cargo build --release --bin eustress-mcp`                      |
| Binary output (host)        | `eustress/target/release/eustress-mcp` (`+ .exe` on Windows)   |
| Bundled with engine         | Yes — same installer, shipped next to `eustress-engine.exe`    |
| Engine Help menu            | "Help → Setup MCP" deep-links to per-IDE config snippets        |

The installer references `eustress/target/release/eustress-mcp.exe` so
every `cargo build --release` produces a valid installer input without
additional steps. No Node.js, no bun, no npm.

### Historical npm package

`@eustress/mcp-server` on npm was marked deprecated at v0.2.2 and removed
from the repo in the April 2026 rewrite. Existing npm installs keep
working (they were fully self-contained); new users install via the
engine bundle. The TypeScript source has been retired from the tree —
consult git history if you need it.

## Tool contract invariants

- **Deterministic output shape.** Every tool returns `{ content: [...] }`
  per MCP convention. Text tools return `type: "text"`. Listings return
  JSON-stringified blocks so clients can parse without heuristics.
- **Bounded results.** Listings cap at 500 items (`MAX_LIST_ITEMS`);
  search caps at 200 matches (`MAX_SEARCH_MATCHES`); file reads cap at
  256 KB (`MAX_FILE_BYTES`) with an explicit `truncated: true` flag so
  the LLM knows to narrow the query.
- **Path safety.** Every input path goes through `resolve_in_universe`,
  which normalises `..` components and rejects anything that escapes the
  Universe root. A tool call with `path: "../../etc/passwd"` errors
  cleanly rather than escaping the jail.
- **Binary rejection.** The `file` resource kind rejects known binary
  extensions up front — meshes, textures, audio — rather than shipping
  garbled bytes as "text."

## Extending the tool set

1. Add a new `fn foo_schema() -> Value` and `fn foo_handler(args, state) -> ToolResult`
   in `src/tools.rs`.
2. Append a `ToolDescriptor { name, description, input_schema, handler }`
   entry in `all_tools()`.
3. Bump `version` in `Cargo.toml` — the value flows into the `initialize`
   response's `serverInfo.version` automatically via `env!("CARGO_PKG_VERSION")`.
4. Open a PR. If the tool needs live engine state (ECS, simulation), gate
   it behind the Phase 2 HTTP-bridge check and surface a clear error when
   the engine isn't running.

## Phase 2 — Engine bridge

When Eustress Engine is running with `--mcp-port 24786` (planned), the
server probes `localhost:24786/health` on startup. If reachable, extra
tools appear in the capability list:

- `eustress_query_ecs` — entity snapshot with components.
- `eustress_get_sim_value` — live simulation readings.
- `eustress_execute_rune` — run a Rune expression in the engine's Rune VM.

Absent the engine, these tools don't appear — the client never sees them,
so AI agents don't try to call broken endpoints. Zero configuration from
the user's perspective.

## Environment variables

| Variable                    | Purpose                                                        |
|-----------------------------|----------------------------------------------------------------|
| `EUSTRESS_UNIVERSE`         | Absolute path to a single Universe; forces it as the default.  |
| `EUSTRESS_UNIVERSES_PATH`   | OS-delimited list of search roots for `eustress_list_universes`. |
| `RUST_LOG`                  | Tracing filter (e.g. `debug`, `eustress_mcp=trace`).            |

Everything written to stderr — nothing to stdout except the JSON-RPC
stream. Stdin EOF triggers a clean shutdown.
