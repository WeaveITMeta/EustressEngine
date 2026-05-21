# 17 — Plugin & Extensibility

> Third-party plugin API, MCP server protocol (Model Context Protocol), public tool
> registry, LSP for external editors, Slint UI extension, plugin lifecycle.
> The **ecosystem boundary** — how anyone outside the core team extends Eustress.

## Pass changelog

- **P3 (2026-05-14):** New doc; 11 features.

---

## Concept summary

**Plugin & Extensibility** defines the *outside-in* surface: how third parties extend Eustress without modifying the core. Three principal channels exist:

1. **MCP (Model Context Protocol)** — out-of-process JSON-RPC tools, callable by AI clients (Claude Desktop, Cursor, Windsurf, etc.). Eustress's tool registry from [07_AI_PLATFORM] is exposed via MCP server (`eustress-mcp-server` binary).
2. **LSP (Language Server Protocol)** — stdio + TCP language server for `.rune` / `.lua` / `.toml` editing in any LSP-compliant editor (VS Code, Helix, Zed, JetBrains).
3. **In-engine plugins** — Slint custom components, Rust crates that depend on `eustress-common` and register systems / resources / classes; hot-reloadable script bindings.

The boundary is the public-API question. Once external developers write plugins, breaking changes become permanently expensive. Versioning, capability discovery, permission scoping, and signing all sit here.

---

## Implementation snapshot

**Crates / files:**
- [eustress-mcp-server](../../eustress/crates/mcp-server/) — stdio JSON-RPC, MCP protocol v2025-06-18, ~4 files
- [eustress-tools](../../eustress/crates/tools/) — shared tool registry (15 modules, 52 tools); Bevy-optional via features
- [eustress-mcp](../../eustress/crates/mcp/) — protocol types crate
- [engine/src/script_editor/lsp.rs](../../eustress/crates/engine/src/script_editor/) — embedded LSP integration in Studio
- `bin/eustress_lsp.rs` — stdio + TCP LSP server binary (under `feature = "lsp"`)
- [engine/src/engine_bridge/](../../eustress/crates/engine/src/engine_bridge/) — TCP JSON-RPC for live ECS queries
- [engine/ui/slint/](../../eustress/crates/engine/ui/slint/) — Slint components

**Working:**
- MCP server binary builds + serves
- Tool registry shared between Studio in-process and MCP out-of-process
- LSP binary builds; basic hover + completion for Rune
- Engine bridge stages 1–2

**Stubbed / missing:**
- MCP file watcher (`watcher.rs` is a stub)
- MCP `/resources/subscribe` streaming
- Live ECS bridge from MCP → engine (`ToolContext.live` unimplemented)
- Public Rust plugin API (no documented trait / lifecycle / sandbox)
- Plugin marketplace / discovery
- Plugin signing / permission scoping
- IDE-side extensions (VS Code, etc.)
- Slint custom-component registration API

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | MCP server (stdio JSON-RPC) | 🟡 |
| 2 | Shared tool registry (Studio + MCP) | ✅ |
| 3 | LSP server binary (stdio + TCP) | 🟡 |
| 4 | LSP IDE extensions (VS Code, Helix, Zed) | 🔴 |
| 5 | Engine bridge (TCP JSON-RPC) — live ECS | 🟡 |
| 6 | MCP `/resources/subscribe` streaming | 🔴 |
| 7 | Public Rust plugin trait + lifecycle | 🔴 |
| 8 | Plugin sandbox / permission scoping | 🔴 |
| 9 | Plugin signing + marketplace | 🔴 |
| 10 | Slint custom-component registration API | 🔴 |
| 11 | Hot-reload of plugin code | 🔴 |

---

## Detailed per-feature cards (top 6)

### Feature 1 — MCP server (stdio JSON-RPC)

**State:** 🟡 · **Effort:** M · **Risk:** Med · **Touches:** [07], [17]
**Sub-features:** stdio transport · `initialize` / `tools/list` / `tools/call` / `resources/*` · universe discovery via env var or argument · MCP protocol v2025-06-18 · shared `eustress-tools` registry

**Concept.** External AI clients (Claude Desktop, Cursor, Windsurf) launch `eustress-mcp-server` as a subprocess; communicate over stdio JSON-RPC. The server exposes the same tools as Studio's Workshop — `create_entity`, `update_entity`, `query_entities`, `execute_rune`, etc.

**Forecasted feedback (R)**
- R1.1 File watcher stubbed → IDEs don't see live file changes; they re-list periodically.
- R1.2 Multi-universe: two Cursor windows on two projects = two MCP servers; no shared state.
- R1.3 `ToolContext.live` unimplemented → spatial raycast / live ECS queries return nothing.
- R1.4 `/resources/subscribe` not implemented; IDEs poll.
- R1.5 Universe path discovery is fragile; env var + CLI arg + cwd detection all coexist.
- R1.6 Auth: open localhost = local-RCE risk if engine port unprotected.

**Implications (I)**
- *Architectural:* MCP is the API contract for external AI; breaking changes = ecosystem churn.
- *Cross-system:* live-bridge wired → MCP + LSP both go reactive; major unblock for AI tooling.
- *Operational:* binary distribution alongside engine (`eustress-mcp-server` ships in installer).
- *Strategic:* MCP-first positioning is the bet that wins the Claude / Cursor user base.

**Risks (X)**
- X1.1 Open localhost TCP = any process can inject ECS mutations.
- X1.2 Tool dispatch latency (~10 ms round-trip) unsuitable for inner-loop raycasts.

**Mitigations (M)**
- M1.1 Token-gated TCP port; token written to `.eustress/engine.port`.
- M1.2 Batch raycasts; don't round-trip per-ray.

---

### Feature 3 / 4 — LSP server + IDE extensions

**State:** 🟡 binary builds; IDE side missing · **Effort:** L · **Risk:** Med · **Touches:** [02], [07], [17]
**Sub-features:** stdio + TCP transport · port-file discovery · Rune hover / completion / goto-def / diagnostics · Luau parallel support · TOML schema-aware completion · per-IDE extension package

**Concept.** The LSP server exposes language services for `.rune`, `.lua`, `.toml` (instance + GUI). It runs as a separate process; IDE extensions (VS Code `eustress-vscode`, Helix config, Zed extension) connect.

**Forecasted feedback (R)**
- R3.1 Binary builds; integrations don't exist (no `vscode-eustress` extension).
- R3.2 Rune ecosystem-wide LSP coverage uneven; pin Rune version.
- R3.3 TOML completion needs schemas in `eustress_common::class_schema` — already partial.
- R3.4 Goto-def across `_instance.toml` (legacy/seed + human-editable; live state is now the Fjall WorldDb store, see MASTER C17) + script + class definition is a graph problem.
- R3.5 Diagnostics from script compile must surface inline.

**Implications (I)**
- *Strategic:* serious scripting needs serious LSP; users will reject the platform without it.
- *Cross-system:* IDE extensions amplify the website's developer-docs strategy.

**Risks (X)** — X3.1 LSP performance at scale (10k files in a Universe) needs profiling.

**Mitigations (M)** — M3.1 Index TOML schemas at boot; cache on disk.

---

### Feature 7 / 8 — Public Rust plugin trait + sandbox

**State:** 🔴 · **Effort:** XL · **Risk:** High · **Touches:** [17]
**Sub-features:** `EustressPlugin` trait with `build(&mut App)` · plugin manifest TOML (name, version, capabilities, deps) · dynamic load via `libloading` · permission scoping (ECS read / ECS write / file IO / network) · capability handshake at load

**Concept.** A plugin author writes a Rust crate that depends on `eustress-common` and implements an `EustressPlugin` trait. Eustress loads the `.so` / `.dll` at startup if listed in `_project/plugins.toml`. Permissions declared in manifest; engine enforces at the system-param level.

**Forecasted feedback (R)**
- R7.1 Rust dynamic-loading via `libloading` requires `#[no_mangle]` constructors; rough.
- R7.2 ABI stability across Bevy versions is fragile; recompile-on-engine-update.
- R7.3 WASM-based plugins (cross-platform, sandboxable) is an alternative; weigh trade-offs.
- R7.4 Permission scoping is hard; ECS reads/writes are unrestricted today.
- R7.5 Capability handshake at load → plugin declares "needs net" → user approves once.

**Implications (I)**
- *Strategic:* unlocks third-party-developed gameplay layers (similar to Roblox Studio plugins).
- *Architectural:* commits to an ABI; breaking changes cost every plugin.
- *Operational:* plugin update story tied to engine update; bad plugin can crash engine.
- *Support burden:* "engine crashes on launch" tickets become "which plugin caused it" debugging.

**Risks (X)**
- X7.1 Malicious plugin steals identity.toml / wipes project.
- X7.2 Plugin ABI break on engine upgrade = stranded plugins = ecosystem freeze.

**Mitigations (M)**
- M7.1 WASM plugin runtime as primary path (sandboxed by default); native plugin as opt-in for trusted authors.
- M7.2 Semver ABI rules + automatic compatibility check.

---

### Feature 9 — Plugin marketplace + signing

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [04_ASSETS], [08_IDENTITY], [09_ECONOMY], [17]
**Sub-features:** plugin discovery on website · author signature (Ed25519, same as Identity) · revocation list (compromised key) · per-plugin reviews + ratings · monetisation (free / one-time / subscription)

**Concept.** A creator-facing surface to publish plugins. Same `.eustress`-world-container-style upload path (R2 via the Cloudflare Worker; was `.pak`-style pre-2026-05-16, see MASTER C17)? Or a separate plugin format. Each plugin signed by author's Identity key; revocation list propagates compromised keys.

**Forecasted feedback (R)**
- R9.1 Marketplace UI shared with [09_ECONOMY] Marketplace (one tab vs. distinct).
- R9.2 Plugin signing reuses [08_IDENTITY] Ed25519 infrastructure.
- R9.3 Review queue (moderation) needs human + automated.
- R9.4 Plugin install in Studio: Settings → Plugins → Browse → Install.

**Implications (I)** — *Cross-system:* shared identity + marketplace pays off here.

**Risks (X)** — X9.1 Malicious plugin distribution = supply-chain attack.

**Mitigations (M)** — M9.1 Default plugins WASM-sandboxed; native ones flagged + extra approval.

---

### Feature 10 — Slint custom-component registration

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [02_STUDIO], [17]
**Sub-features:** plugin registers `Component` via Slint API · custom Slint panels visible in Studio toolbar · property bindings exposed to plugins · Workshop tool wrappers can invoke

**Concept.** A plugin author drops in a `.slint` file + Rust glue; the panel appears in the Studio's left-rail or dockable area. Bindings to ECS via shared abstractions.

**Forecasted feedback (R)** — R10.1 Slint version compatibility across plugins is brittle.

**Implications (I)** — *Cross-system:* third-party Studio panels are the killer feature for vertical specialisation (animator panel, mod builder, etc.).

---

### Feature 11 — Hot-reload of plugin code

**State:** 🔴 · **Effort:** L · **Risk:** High · **Touches:** [02_STUDIO], [17]
**Sub-features:** plugin `.so` / `.wasm` recompile detection · state preservation across reload · partial-reload (systems only, not resources)

**Concept.** Developer edits a plugin's Rust source, recompiles → engine notices, unloads old, loads new, restores ECS state. Standard hot-reload UX.

**Forecasted feedback (R)** — R11.1 Bevy's plugin model isn't hot-unload-friendly; design carefully.

**Implications (I)** — *Strategic:* makes plugin development feel like script development.

**Risks (X)** — X11.1 State-restore bugs corrupt project on hot-swap.

**Mitigations (M)** — M11.1 Disable in production; dev-mode only.

---

## Wiring / import gaps (top 8)

1. MCP file watcher (`notify` subscription)
2. MCP `/resources/subscribe` streaming
3. Live ECS bridge via `.eustress/engine.port`
4. Rust plugin trait + manifest TOML schema
5. WASM plugin runtime (`wasmtime` / `wasmer`)
6. Plugin permission scoping
7. Slint custom-component registration API
8. VS Code extension `vscode-eustress` (LSP client + sidebar)

---

## Cross-system dependencies

- **[02_STUDIO]** plugins drop panels into Studio UI.
- **[07_AI_PLATFORM]** tool registry is the foundation for MCP.
- **[08_IDENTITY]** plugin signatures via creator's Identity key.
- **[09_ECONOMY]** plugin marketplace + payouts.
- **[10_TELEMETRY]** engine_bridge subscribe enables MCP / LSP reactive consumers.
- **[12_INFRASTRUCTURE]** plugin distribution via R2; CI builds the IDE extensions.

---

## Open questions

- Q17.1 WASM-only or native-allowed plugins?
- Q17.2 Plugin marketplace shares UI with main Marketplace or has its own?
- Q17.3 ABI stability strategy across engine versions?
- Q17.4 Permission model: capability list or fine-grained per-system?
- Q17.5 Free vs. paid plugin policy?
- Q17.6 Apple Developer requirements for plugin distribution on macOS?
