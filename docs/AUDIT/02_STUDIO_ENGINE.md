# 02 — Studio Engine

> The desktop editor. Slint UI + Bevy ECS + Fjall WorldDb store (TOML retained
> as legacy/seed + human-editable schema) + Workshop AI.
> Single most-touched system in the workspace.

## Pass changelog

- **P1 (2026-05-14):** 14 feature rows; 78 R + 58 I + 20 wiring gaps.
- **P2 (2026-05-14):** + Feature 15 Multiplayer Studio (Loro CRDT scaffold; ~80% UI / 0% wired). Cross-cut C12.
- **P3 (2026-05-14):** + 8 shipped features missed by P1 (drag-drop reparent, context menus, custom cursors, floating numeric input, toasts, keybindings, theme, accessibility). LSP & material-editor state corrected. Forecasts deepened (i18n, perf-at-10M, export versioning, collab-conflict-UX).
- **P4 (2026-05-14):** **Full retrofit to per-feature-card format (R/I/X/M).** 23 cards. Addendum blocks removed.
- **Updated 2026-05-16: storage pivot.** File-system-first superseded by the Fjall WorldDb store (MASTER C17). Live entity-component-system state now lives in `world.fjalldb/`; TOML is legacy/seed + human-editable schema. **Known gap:** the Properties panel does **not** persist edits in the default build — legacy TOML write-back is gated behind an opt-in `toml` cargo feature and the Fjall mirror only writes Transform so far. Publish archive now bundles the Fjall database + baked `.echk` chunks (was `.pak` per-instance TOML); upload mechanism unchanged.

---

## Concept summary

The Studio Engine is where creators build. It opens a Universe (a folder of Spaces); a faithful importer mirrors a Space's whole tree into the Fjall WorldDb `tree` partition on first open, after which **the database is authoritative** (TOML on disk is the legacy/seed form + human-editable schema — see MASTER C17). It exposes entities through ~16 Slint panels (Explorer / Properties / Workshop / Timeline / History / Insert / Material / Terrain / Soul / Services / Network / Simulation / Publish / Settings / Output / Ribbon), drives editing via ModalTool + gizmo overhaul, and synchronises everything via stream-teed history. Single-author storage is the **Fjall WorldDb store** (C3 — file-system-first was the prior architecture, superseded 2026-05-15); Multiplayer Studio sessions are **cloud-CRDT** (C12) — Studio detects and routes. **Known persistence gap (2026-05-16):** the Properties panel does **not** write edits back in the default build — legacy TOML write-back is behind an opt-in `toml` cargo feature, and the Fjall mirror currently persists only Transform.

The Studio also embeds AI ([Workshop](07_AI_PLATFORM.md)) with ~30 tools, ~CAD + mesh-edit kernels for geometry ([18_CAD](18_CAD_MESHGEOMETRY.md)), and exposes its tools to MCP / LSP via the engine bridge.

---

## Implementation snapshot

- **Bin entry:** [engine/src/main.rs](../../eustress/crates/engine/src/main.rs)
- **Key plugins:** SpaceFileLoaderPlugin, MoveToolPlugin, ScaleToolPlugin, RotateToolPlugin, SelectToolPlugin, UndoPlugin, MaterialSyncPlugin, BillboardGuiPlugin, WorkshopPlugin, HistoryStreamPlugin, SoulPlugin (Rune+Luau), MeshOptPlugin, PlayModePlugin.
- **Slint UI:** [engine/ui/slint/main.slint](../../eustress/crates/engine/ui/slint/main.slint) + ~52 sibling `.slint` files; drain pattern (`SlintAction` queue → single drain system).
- **Memory invariants:** single `StudioState` (never duplicate types); `.slint` compiles to Rust; `cargo run` not `cargo check`; every tool routes through `spawn_new_part_with_toml`.

---

## Top-of-doc feature index

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Viewport & camera | ✅ |
| 2 | Modal tools (Move / Rotate / Scale / Select / Measure) | ✅ |
| 3 | Selection model | 🟡 |
| 4 | Undo / Redo with stream tee | ✅ |
| 5 | File I/O round-trip (load → edit → persist) | 🟡 |
| 6 | Hot-reload (Studio + external) | ✅ |
| 7 | Slint panels suite (16 panels) | 🟡 |
| 8 | Class serialisation coverage | 🟡 |
| 9 | Workshop AI (Claude, ~30 tools) | 🟡 |
| 10 | CAD + mesh-edit kernels *(detail → [18](18_CAD_MESHGEOMETRY.md))* | 🟡 |
| 11 | Soul scripting (Rune + Luau) | 🟡 |
| 12 | Engine bridge (TCP JSON-RPC) | 🟡 |
| 13 | LSP / external editor bridge *(detail → [17](17_PLUGIN_EXTENSIBILITY.md))* | 🟡 |
| 14 | Play-in-editor mode | 🟡 |
| 15 | **Multiplayer Studio** *(P2 add)* | 🟡 80% UI / 0% wired |
| 16 | Drag-drop reparenting *(P3 add)* | ✅ |
| 17 | Right-click context menus *(P3 add)* | ✅ |
| 18 | Custom cursor overlay *(P3 add)* | ✅ |
| 19 | Floating numeric input *(P3 add)* | ✅ |
| 20 | Toast / undo notifications *(P3 add)* | ✅ |
| 21 | Keybindings system (50+ actions) *(P3 add)* | ✅ |
| 22 | Theme system + accent tokens *(P3 add)* | 🟡 |
| 23 | Accessibility manifest *(P3 add)* | 🟡 |

---

## Per-feature cards

### Feature 1 — Viewport & camera

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** selection wireframes per-shape (cyan `#00bcd4`) · hover preview · GPU pick · drag-rect select · orbit camera · frame-on-selection (F)

**Concept.** 3D viewport rendered by Bevy with perspective + orbital camera. Selection wireframes are per-shape; gizmos have `depth_bias = -1.0` so they never hide behind geometry.

**Forecasted feedback (R)**
- R1.1 Camera bookmarks (F1–F4) missing.
- R1.2 Orbit pivot should default to selection bbox center.
- R1.3 Walk-fly FPS-style camera collision absent.
- R1.4 Multi-viewport (top/side/front/perspective) unsupported.
- R1.5 Frame-on-selection stutters on large selections.

**Implications (I)**
- *Architectural:* viewport latency is the proxy for "professional feel".
- *Operational:* per-shape wireframes use gizmos with `depth_bias=-1.0` — keep that invariant.
- *Support:* "selection wireframe disappeared" tickets when invariant breaks.
- *Strategic:* the viewport is the daily creator surface.

**Risks (X)** — X1.1 Slint render-loop blocking causes input lag.

**Mitigations (M)** — M1.1 Viewport in own task; never block on Slint.

---

### Feature 2 — Modal tools (Move / Rotate / Scale / Select / Measure)

**State:** ✅ · **Effort:** Done · **Risk:** Med · **Touches:** [02], C1 (units), C9 (DisplayUnit)
**Sub-features:** gizmo handles · grid snap · axis lock · BillboardGui awareness · `ModalTool` trait · pivot modes (planned)

**Concept.** Switching tools (Q/W/E/R) re-binds input. Each tool owns its gizmo. Move is BillboardGui-aware (gizmo follows parent + units_offset; drag mutates units_offset). Persist via `persist_transform_to_toml` after release.

**Forecasted feedback (R)**
- R2.1 Geometric snap to vertices/edges only partial (grid works).
- R2.2 Pivot modes (origin / centroid / median) per-tool pending.
- R2.3 Plane lock (Shift+drag) absent.
- R2.4 Rotate gizmo angle readout meter-only; ignores DisplayUnit.
- R2.5 Measure tool readout meter-only (UNITS Stage 10).
- R2.6 Free-drag jitters on heavy scenes — fixed timestep?
- R2.7 `FloatingNumericInput` already shipped (Feature 19); wire to gizmo drags.

**Implications (I)**
- *Architectural:* `ModalTool` trait must remain the abstraction — never bypass.
- *Cross-system:* every new positional-metadata class needs a parallel awareness pass.
- *Operational:* small slowdowns destroy product feel.
- *Strategic:* gizmo overhaul shipped; further changes reuse abstraction.

**Risks (X)**
- X2.1 Snap mis-application silently mis-aligns parts.
- X2.2 Numeric input + drag race conditions.

**Mitigations (M)**
- M2.1 Snap-disabled state toast when active.
- M2.2 Numeric input is modal-blocking; drag pauses while input focused.

---

### Feature 3 — Selection model

**State:** 🟡 · **Effort:** M · **Risk:** Low · **Touches:** [02], [10_TELEMETRY]
**Sub-features:** single / additive (Shift) / box-drag / parent-aware / depth-stacked / path-based restore

**Concept.** `SelectionState` resource + `SelectionChanged` event. Selection persists across re-spawn by entity ID; TOML reload reshuffles IDs → fall back to path.

**Forecasted feedback (R)**
- R3.1 Selection empties after TOML hot-reload sometimes — path-restore.
- R3.2 Drag-select includes locked entities (should exclude).
- R3.3 `SelectionBox` / `SelectionSphere` runtime adornments missing (FEATURE_PARITY row 31).
- R3.4 Alt-click-cycle through overlapping entities missing.
- R3.5 Workshop "look here" highlight visually identical to selection.

**Implications (I)**
- *Architectural:* selection is the entry to every editing action — any flake multiplies.
- *Cross-system:* must be replicable across multiplayer co-editing (C12 Feature 15).
- *Strategic:* selection breakage is universally noticed.

**Risks (X)** — X3.1 Selection desync in Multiplayer Studio between editors.

**Mitigations (M)** — M3.1 Selection is per-editor (ephemeral); not in Loro doc.

---

### Feature 4 — Undo / Redo with stream tee

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [02], [10_TELEMETRY]
**Sub-features:** 30+ waypoint kinds · history panel · stream tee to `history.*` · revert-to-here · `SpaceRescanNeeded` on undo (trash-rename invariant) · per-editor isolation (planned)

**Concept.** `UndoStack` with named waypoints. Every action emits `history.<kind>`. Destructive ops use `fs::rename` to `.trash/`; undo renames back + triggers SpaceRescan.

**Forecasted feedback (R)**
- R4.1 Linear undo (no branches) — Ctrl-Z after redo discards redo path.
- R4.2 Long sessions need memory cap; old waypoints evict.
- R4.3 Some property writes bypass waypoint API — audit needed.
- R4.4 Multi-step actions (Group create) → N waypoints instead of 1.
- R4.5 PIE-boundary undo policy undefined.
- R4.6 Persistent undo across Studio restart unimplemented.

**Implications (I)**
- *Architectural:* undo = trust surface. Wrong undo > missing feature.
- *Cross-system:* history stream tee enables Workshop "what just happened" context.
- *Migration:* persistent undo unlocks "what did I do yesterday" workflows.
- *Strategic:* the stream tee is high-leverage — preserve.

**Risks (X)** — X4.1 Property write that misses waypoint = silently-unrecoverable change.

**Mitigations (M)** — M4.1 CI lint: any `Mut<X>` access checks for waypoint emit.

---

### Feature 5 — File I/O round-trip (load → edit → persist)

**State:** 🟡 · **Effort:** L · **Risk:** Critical · **Touches:** [02], [04_ASSETS], [16_PERSISTENCE], C3 (single-author storage), C17 (WorldDb)
**Sub-features:** Fjall WorldDb mirror (`entities` rkyv + `tree` raw-TOML) · TOML importer (legacy/seed) · atomic write (`write_temp + rename`, legacy `toml` feature) · watcher rename suppression · `toml_edit` for comment preservation · class generator macro (planned) · per-instance `unit` round-trip

**Concept.** *Updated 2026-05-16: storage pivot.* Load: a faithful importer mirrors the Space tree into the Fjall WorldDb `tree` partition on first open (legacy walker reads `_instance.toml` + class-specific TOML as the seed); after import the database is authoritative. Edit: user moves a part. Persist: the Fjall mirror currently persists **only Transform**; legacy `persist_transform_to_toml` write-back (reads `MeasureUnit`, converts engine → authored, writes, suppresses the watcher event via `rename_in_progress`) is gated behind an opt-in `toml` cargo feature and is **off in the default build**.

**Forecasted feedback (R)**
- R5.1 **Properties write-back is the headline gap (worse post-pivot).** In the default build the Properties panel persists **nothing** — legacy TOML write-back is behind the opt-in `toml` feature and the Fjall mirror only writes Transform. Non-Transform edits are in-memory only.
- R5.2 Class serialisation: 8 of 60+ types round-trip (Part, Folder, Model, Light, Script + a few). GUI/Humanoid/Constraints/Camera/Terrain unwritten.
- R5.3 Atomic writes — partial writes corrupt TOML on crash. Use `write_to_temp + rename`.
- R5.4 Concurrent external edit conflict (notify-rs + Studio writer race).
- R5.5 Recursive descendant gating during rename has been bug-source; audit other paths.
- R5.6 TOML comments lost on rewrite — use `toml_edit`, not `toml`.
- R5.7 Per-instance `unit` field must round-trip (UNITS Stage 4 shipped); spot-check edge cases.

**Implications (I)**
- *Architectural:* properties write-back is the **single biggest 1.0 blocker** — top of P1 list.
- *Cross-system:* coverage gaps mean some classes spawn but can't be edited persistently — user-visible inconsistency.
- *Migration:* every new class needs (a) component, (b) TOML schema, (c) loader, (d) writer, (e) Properties binding. Macro before more classes.
- *Operational:* atomic-rename must replace direct-write everywhere.
- *Support:* "my change didn't save" = catastrophic ticket category.
- *Strategic:* the Studio feels finished only when this works.

**Risks (X)**
- X5.1 Partial write corrupts TOML on crash.
- X5.2 Round-trip loses unit, scope, custom fields silently.

**Mitigations (M)**
- M5.1 Atomic write helper (`atomic_write_toml`) is the only sanctioned path.
- M5.2 Round-trip integration test per-class.

---

### Feature 6 — Hot-reload (Studio + external)

**State:** ✅ · **Effort:** Done · **Risk:** Med · **Touches:** [02], [10_TELEMETRY]
**Sub-features:** notify-rs + debouncer (300 ms) · `rename_in_progress` suppression set · cross-platform path canonicalisation · pause-while-batching

**Concept.** External edits in Blender / VS Code surface as fresh events; the Studio re-parses, diffs, updates ECS in place. Studio's own writes are pre-flagged in `rename_in_progress` to suppress bounce-back.

**Forecasted feedback (R)**
- R6.1 GLB transform reload vs. GLB binary reload — only TOML wired.
- R6.2 Watcher path hardcoded in some places; follow `EUSTRESS_UNIVERSE` / CLI arg.
- R6.3 Case-sensitivity on Linux vs. case-insensitive Windows + macOS occasionally bites.
- R6.4 Editor confusion when watcher paused (batching) — UI shows stale state.
- R6.5 Studio writes are *not* symmetric to external edits — actively suppressed.

**Implications (I)**
- *Architectural:* the same plumbing supports multi-process editing (LSP, MCP, future co-op).
- *Cross-system:* hot-reload is the AI workflow's foundation — agents write files, Studio sees them live.
- *Strategic:* differentiator vs. Unity/Unreal which need re-imports.

**Risks (X)** — X6.1 Suppression set leak → studio sees its own write as external → infinite loop.

**Mitigations (M)** — M6.1 Bounded TTL on suppression entries.

---

### Feature 7 — Slint panels suite (16 panels)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [02], [07_AI], [10_TELEMETRY], [11_SIM]
**Sub-features:** Explorer, Properties, Workshop, Timeline, History, Insert, Material, Terrain, Soul, Services, Network, Simulation, Publish, Settings, Output, Ribbon · drain pattern · panel docking (planned)

**Concept.** Every workflow has a panel. UI is comprehensive; ~10 panels have shells with click-handlers stubbed as `TODO`.

**Forecasted feedback (R)**
- R7.1 Timeline animation editing skeletal — frames render, drag/edit unwired.
- R7.2 Material editor: browse-only ([04] integration); writeback absent.
- R7.3 Terrain heightmap export `TODO` ([13] Feature 5).
- R7.4 Toolbox Insert submenu UI exists; handler unwired.
- R7.5 Publish backend integration is the long-pole ([04] confirmed working).
- R7.6 Several Settings toggles no-op.
- R7.7 Soul Settings: API-key works; model selection limited.
- R7.8 Services Browser read-only.
- R7.9 Network panel cosmetic.
- R7.10 Simulation Settings: watchpoint display works; parameter injection unwired.
- R7.11 Panel docking / detaching to second monitor is a power-user ask.
- R7.12 Theme switcher only partly wired (Feature 22).
- R7.13 Per-panel focus brittle — destructive-key gating in keybindings (Feature 21).

**Implications (I)**
- *Architectural:* the drain pattern is the foundation; new handlers extend it.
- *Cross-system:* "looks finished but isn't" = trust debit.
- *Operational:* every stubbed handler is silent failure.
- *Strategic:* completeness drives perceived quality.

**Risks (X)** — X7.1 Users find stubbed handlers; report as bugs.

**Mitigations (M)** — M7.1 Either ship or hide; never half-show.

---

### Feature 8 — Class serialisation coverage

**State:** 🟡 · **Effort:** L · **Risk:** Critical · **Touches:** [02], [04_ASSETS], [16_PERSISTENCE]
**Sub-features:** Part / Folder / Model / Light / Script ✅; GUI / Humanoid / Camera / Constraints / Terrain 🟠; ChunkedWorld extraction tool 🟠

**Concept.** Every class in `eustress_common::classes` needs (a) TOML schema (EEP v2), (b) loader, (c) writer, (d) Properties bindings.

**Forecasted feedback (R)**
- R8.1 GUI classes render at runtime via Slint but have no Bevy components → not selectable in viewport.
- R8.2 Constraints (Weld / Motor6D / Hinge / Spring / Rope) — no TOML loaders.
- R8.3 Humanoid partial (BodyColors, HumanoidDescription missing).
- R8.4 Camera as instance not editable.
- R8.5 Terrain voxel exists; painting in dialog only; no full round-trip.
- R8.6 ChunkedWorld extraction tool missing (see [05_SPACE_STREAMING] Feature 7).

**Implications (I)**
- *Architectural:* coverage caps what projects can serialise = what Studio can build.
- *Cross-system:* a proc-macro generator over class schema would close 80% in one effort.
- *Migration:* mid-version schema changes break old projects without migration framework.

**Risks (X)** — X8.1 New class without writer = data loss on save.

**Mitigations (M)** — M8.1 CI test: every class round-trips through TOML.

---

### Feature 9 — Workshop AI (Claude, ~30 tools)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [02], [07_AI_PLATFORM], [09_ECONOMY]
**Sub-features:** chat UI · ~30 tools · approval gate · MCP card UI · artifact preview · stream tee planned

**Concept.** Side-panel chat with Claude. Sends conversation + tool defs; Claude returns tool calls; user approves; tool runs through `eustress-tools` registry; result loops back. Memory uses `mcp__eustress__remember` / `recall`.

**Forecasted feedback (R)**
- R9.1 Tool approval cluttered when multiple tools fire.
- R9.2 Artifact preview wired for some tool kinds only.
- R9.3 Multi-turn agent loop with self-correction underimplemented.
- R9.4 Cost / per-day cap missing.
- R9.5 No stream-topic consumer ([10] gap) → agent can't see live ECS state.
- R9.6 BillboardGui / ScreenGui on_click → Soul callback is a TODO.
- R9.7 Direct entity-manipulation tool missing (today only file-level write-back).
- R9.8 Workshop session export (.md) absent.

**Implications (I)** — see [07_AI_PLATFORM] for full coverage.

**Risks (X)** — X9.1 Tool approval fatigue → users auto-approve dangerous ops.

**Mitigations (M)** — M9.1 Per-tool-kind approval default (auto-approve safe; always-ask destructive).

---

### Feature 10 — CAD + mesh-edit kernels

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [02], [18_CAD_MESHGEOMETRY]
**Sub-features:** see [18_CAD_MESHGEOMETRY.md](18_CAD_MESHGEOMETRY.md)

**Concept.** Detail audit in [18]. This row is a pointer.

**Forecasted feedback (R)**
- R10.1 Studio ribbon entries for kernels MISSING — discoverability blocker.
- R10.2 VIGA image-to-geometry wired through `ImageToGeometryTool`; unbenchmarked.

**Implications (I)** — *Strategic:* ribbon entries unlock the kernels with minimal effort.

---

### Feature 11 — Soul scripting (Rune + Luau)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [02], [01_CLIENT], [07_AI_PLATFORM]
**Sub-features:** Rune VM + Luau VM · ECS bindings · GUI bridge · Units API · hot-reload · error routing · stream tee planned

**Concept.** Two VMs side-by-side. Editor opens on double-click; hot-reload on `.rune` / `.lua` save *(Client-side broken per [01] Feature 7)*. Errors route to Output panel + `script.error` stream topic.

**Forecasted feedback (R)**
- R11.1 In-editor Monaco / Wry tab — multiple commits; user wants tabs in Workshop area.
- R11.2 Rune LSP integration is long pole ([17_PLUGIN] Feature 3).
- R11.3 Luau-Rune cross-reference undefined.
- R11.4 Sandboxing: filesystem reach must be jailed to project root.
- R11.5 Stream tee for runtime errors works; `script.print` mirror needed.

**Implications (I)**
- *Architectural:* scripting parity is the AI-writes-code bridge — can't half-do it.
- *Cross-system:* two VMs = maintenance tax; gain must justify forever.

**Risks (X)** — X11.1 Sandbox escape via mlua filesystem.

**Mitigations (M)** — M11.1 Path-jail at bind time.

---

### Feature 12 — Engine bridge (TCP JSON-RPC)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [02], [07_AI_PLATFORM], [10_TELEMETRY], [17_PLUGIN]
**Sub-features:** localhost JSON-RPC · stages 1–2 shipped (handshake, basic queries) · stages 3 (subscriptions) / 4 (tool routing) / 5 (multi-tenant) pending · port file `.eustress/engine.port`

**Concept.** Internal RPC for MCP / LSP / future plugins. Exposes ECS queries, tool dispatch, and (planned) stream-topic subscribe.

**Forecasted feedback (R)**
- R12.1 No `SubscribeTopic` method in `BridgeRequest` enum yet.
- R12.2 MCP currently polls `runtime-snapshot.json` instead of subscribing — wasteful.
- R12.3 Port discovery via `.eustress/engine.port` races with multi-Studio instances.
- R12.4 Auth: accepts any localhost client — fine for single-user.

**Implications (I)**
- *Architectural:* engine bridge unifies every external tool.
- *Cross-system:* finishing stages 3–5 closes 3+ system gaps at once.
- *Strategic:* lock the schema before external plugin authors arrive.

**Risks (X)** — X12.1 Unauth localhost = local-RCE.

**Mitigations (M)** — M12.1 Token in `.eustress/engine.port`.

---

### Feature 13 — LSP / external editor bridge

**State:** 🟡 binary builds · **Effort:** L · **Risk:** Med · **Touches:** [02], [17_PLUGIN_EXTENSIBILITY]
**Sub-features:** see [17_PLUGIN_EXTENSIBILITY.md](17_PLUGIN_EXTENSIBILITY.md)

**Concept.** Detail audit in [17]. The `eustress-lsp` binary compiles and serves; IDE-side packages (vscode-eustress, etc.) don't exist.

---

### Feature 14 — Play-in-editor mode

**State:** 🟡 · **Effort:** M · **Risk:** Med · **Touches:** [02], [03_MULTIPLAYER], [11_SIMULATION]
**Sub-features:** F5 Play · F6 Pause · F7 Solo · F8 Stop · world snapshot/restore · embedded QUIC server

**Concept.** F5 plays the current Space in an embedded server + client. Snapshot/restore so play doesn't corrupt edits.

**Forecasted feedback (R)**
- R14.1 Solo wired; Server mode spawns embedded QUIC; session cleanup on disconnect incomplete.
- R14.2 Character spawn picks first SpawnLocation only.
- R14.3 Pause→edit→stop: edits during pause should land on disk; only partially.
- R14.4 Multi-window PIE (two clients to one server) unwired.

**Implications (I)**
- *Architectural:* PIE shares QUIC contract with production [03]; reduces drift.
- *Strategic:* PIE quality drives iteration speed for scripts/physics.

**Risks (X)** — X14.1 PIE state leaks to disk on crash; project corrupt.

**Mitigations (M)** — M14.1 PIE-mode flag in snapshot; refuse to persist while flagged.

---

### Feature 15 — Multiplayer Studio  *(P2 add)*

**State:** 🟡 80% UI / 0% wired · **Effort:** XL (12–16 weeks MVP) · **Risk:** High · **Touches:** [02], [03_MULTIPLAYER], [04_ASSETS], [05_SPACE_STREAMING], [08_IDENTITY], [10_TELEMETRY], C12
**Sub-features:** Loro CRDT document · session lifecycle (host / join / leave) · presence + cursors · per-entity authority · chunk-host election · conflict viz · per-editor undo · invite links · voice/chat overlay (planned)

**Concept.** Two+ creators on the same Universe simultaneously, synced via cloud (server-resident Loro CRDT broadcasts deltas in milliseconds). Distinct from the single-author storage path (C3 — now the Fjall WorldDb store, was file-system-first pre-2026-05-15) — this mode is **cloud-first** (C12).

**Forecasted feedback (R)**
- R15.1 Loro doc schema unspecified — mirror full Instance + components or flat `{path:{prop:val}}`?
- R15.2 Chunk-authority election deterministic vs. random — random re-election re-merges Loro ops unpredictably.
- R15.3 Network partition: editors diverge; reconnect merges may re-order. UX: silent rebase vs. conflict warning?
- R15.4 Offline mode: queue ops, merge on reconnect; collides with file-fallback.
- R15.5 Selection conflict: A selects X, B deletes X — optimistic vs. fail?
- R15.6 1M+ entities: Loro checkpoint blocks editors?
- R15.7 Bandwidth: 30 edits/sec × 5 editors × 100 B = 15 KB/s per editor → 75 KB/s server.
- R15.8 Invite link backend route + claim flow.
- R15.9 5-min idle → auto-drop?
- R15.10 Slint callbacks (on-connect / on-invite / on-follow-user) all empty.
- R15.11 Permission model: any collaborator deletes anything, or ACL?
- R15.12 Read-only spectators absent from UI.

**Implications (I)**
- *Architectural:* the single-author storage path (C3 — now Fjall WorldDb, was file-system-first pre-2026-05-15) does not apply in this mode; C12 (two storage modes) applies. `SpaceLoader` must detect at open and route to either the Fjall WorldDb path or the cloud-CRDT path.
- *Cross-system:* shared with [03] (QUIC + Loro stack), [10] (`collab.edit.*` topics), [08] (JWT identifies editor), [05] (chunk authority overlap).
- *Migration:* existing single-player projects open in session mode only on opt-in.
- *Operational:* 1-hour collab × 5 editors × deltas = server storage + bandwidth.
- *Support:* "edit lost" / "conflict not visible" is the scariest ticket category.
- *Strategic:* real-time collaborative editing is the #1 retention multiplier vs. Roblox Studio.
- *Workshop AI:* Claude edits become `WorkshopOp` messages merged like any other; "pinned" policy needed.
- *Compliance:* GDPR right-to-deletion — deleted user's CRDT edits stay in history.

**Risks (X)**
- X15.1 Loro is young; API stability moving.
- X15.2 Latency > 200 ms inter-region feels sluggish; sub-100 needs client-side prediction.
- X15.3 Leaked JWT + Loro ops can trash a project.
- X15.4 Testing collab is hard; need simulator (N editors, latency, packet loss, byzantine).

**Mitigations (M)**
- M15.1 Phase-1 MVP: 2 editors, entity create/delete/property edit only; defer everything else.
- M15.2 Per-user edit rate cap (e.g. 50/sec); server validates.
- M15.3 Server-recorded session log → admin "revert to T" button.
- M15.4 Pin Loro at known-good version; gate upgrades behind benchmarks.

---

### Feature 16 — Drag-drop reparenting  *(P3 add)*

**State:** ✅ shipped · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** Explorer drop-zone · `do_reparent_node` · `ReparentNode` undo action · cross-class drop rules · folder-flattening detection

**Concept.** Drag an entity in the Explorer onto another → reparents. Full UndoStack integration.

**Forecasted feedback (R)**
- R16.1 Drop on locked parent should reject.
- R16.2 Cross-Space reparent undefined.
- R16.3 Reparent into Multiplayer Studio session — server-replicated (Feature 15).

**Implications (I)** — *Architectural:* canonical path through C2 (`create_instance` → re-attach).

**Risks (X)** — X16.1 Recursive parent loop crashes ECS.

**Mitigations (M)** — M16.1 Cycle detection pre-drop.

---

### Feature 17 — Right-click context menus  *(P3 add)*

**State:** ✅ shipped · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** `context_menu.rs` + `context_menu.slint` · per-target menus (soul-script / entity / asset / viewport) · keyboard shortcut hint

**Concept.** Right-click anywhere → contextual menu of relevant actions. Per-target wiring.

**Forecasted feedback (R)** — R17.1 Custom-tool authors want plugin API ([17_PLUGIN] hook).

**Implications (I)** — *Cross-system:* plugins (C15 WASM sandbox) extend the context menu.

---

### Feature 18 — Custom cursor overlay  *(P3 add)*

**State:** ✅ shipped · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** `cursor_badge.rs` · 16×16 viewport-rendered badge · OS-cursor tracking · per-tool icon

**Concept.** Workaround for Slint's limited cursor API. A small badge renders in the viewport, follows the OS cursor, indicates active tool.

**Forecasted feedback (R)** — R18.1 Badge lag at high refresh rates.

**Implications (I)** — *Architectural:* workaround; ideally replaced by native Slint cursor API.

---

### Feature 19 — Floating numeric input  *(P3 add)*

**State:** ✅ shipped · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** `floating_numeric_input.slint` · drag-to-scrub · type-to-edit · expression eval (e.g. `5 + 2*3`) · unit-suffix awareness

**Concept.** A numeric input field that supports drag-scrub, type, expressions, and unit suffixes. Used in Properties + gizmo readouts.

**Forecasted feedback (R)** — R19.1 Expression parser uses limited grammar; verify operator precedence.

**Implications (I)** — *Cross-system:* Properties write-back (Feature 5) hooks via this.

---

### Feature 20 — Toast / undo notifications  *(P3 add)*

**State:** ✅ shipped · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** `toast_undo.rs` + `toast_undo.slint` · 5-s auto-dismiss · hover-freezes-timer · inline undo button

**Concept.** Non-blocking notification with inline undo. Appears at top-center; dismisses after 5 s unless hovered.

**Forecasted feedback (R)** — R20.1 Stacking (3+ at once) visual.

**Implications (I)** — *Operational:* the right primitive for "what just happened" feedback.

---

### Feature 21 — Keybindings system (50+ actions)  *(P3 add)*

**State:** ✅ shipped · **Effort:** Done · **Risk:** Low · **Touches:** [02]
**Sub-features:** `keybindings.rs` ~1k LOC · 50+ actions · panel-focus gating on destructive keys · config-file serialisation · per-platform defaults

**Concept.** All shortcuts in one place; panel focus determines whether Delete deletes an entity or a character. Config persists per-user.

**Forecasted feedback (R)**
- R21.1 No in-Studio keybinding editor UI.
- R21.2 Conflict detection if user binds two actions to same key.

**Implications (I)** — *Cross-system:* the focus-gating fix in P0 batch is load-bearing.

---

### Feature 22 — Theme system + accent tokens  *(P3 add)*

**State:** 🟡 · **Effort:** S · **Risk:** Low · **Touches:** [02]
**Sub-features:** `theme.slint` design tokens · accent `#00bcd4` cyan · dark mode CSS variables · light mode incomplete · toggle persists per-user

**Concept.** Complete design-token system; dark mode fully wired; light mode CSS variables incomplete (toggle is half-cosmetic).

**Forecasted feedback (R)**
- R22.1 Light-mode panel colours unverified.
- R22.2 Per-creator accent (Bliss reward?) — out of scope.

**Implications (I)** — *Operational:* shipping light mode requires a pass on every Slint file.

---

### Feature 23 — Accessibility manifest  *(P3 add)*

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [02], [06_WEBSITE]
**Sub-features:** design-time manifest · Slint runtime bindings (ARIA-style) · screen reader · keyboard nav · color-blind modes · captions

**Concept.** Design-time manifest exists; runtime bindings not shipped. WCAG AA compliance is a launch gate for government/enterprise.

**Forecasted feedback (R)**
- R23.1 Manifest bloat at 100+ components.
- R23.2 No CI accessibility lint.
- R23.3 Color-blind modes need theme variants.

**Implications (I)** — *Compliance:* Section 508 / WCAG AA blocks government / enterprise sales.

**Risks (X)** — X23.1 Public launch with no accessibility = reputational + regulatory risk.

**Mitigations (M)** — M23.1 Ship ARIA bindings on top panels first; defer rest.

---

## Wiring / import gaps

1. Properties write-back system (atomic + rename-suppressed)
2. GUI-class TOML round-trip (ScreenGui / Frame / TextLabel / TextButton / ImageLabel)
3. Constraint TOML round-trip (Weld / Motor6D / Hinge / Spring / Rope / Align*)
4. Humanoid TOML expansion (BodyColors, HumanoidDescription)
5. Camera-as-instance promotion
6. Terrain heightmap export ([13] Feature 5)
7. ChunkedWorld extraction tool ([05] Feature 7)
8. Toolbox Insert submenu wiring
9. Workshop direct-entity-manipulation tool
10. Material editor panel writeback
11. Spatial index for raycasts at 10M-instance scale (R-tree)
12. CAD ribbon entries (Union / Negate / Extrude)
13. Mesh-edit ribbon entries (Bevel / Loop cut / Inset)
14. VIGA benchmark
15. Constraint adornments (visualise Weld / Motor6D in viewport)
16. LSP fix-up (Rune version pin, IDE extension)
17. Engine-bridge stage 3 (subscriptions)
18. GLB transform hot-reload
19. Soul-script on_click bridge (BillboardGui / ScreenGui)
20. Workshop tool stream emit
21. Multiplayer Studio: 12 wiring gaps (see Feature 15)
22. Light-mode CSS variables (Feature 22)
23. Accessibility runtime bindings (Feature 23)
24. Internationalization framework (i18n)
25. Export-format versioning (binary scene snapshot)

---

## Cross-system dependencies

- **C1 / Units** — Properties panel + tool readouts must use DisplayUnit projection (UNITS Stage 6.5 open).
- **C2 / Canonical create** — Insert + drag-drop + Workshop tools converge on `create_instance`.
- **C3 / Single-author storage** — file-system-first was the prior architecture; superseded 2026-05-15 by the Fjall WorldDb store (MASTER C17); TOML retained as legacy/seed + human-editable schema. **C12 toggles to cloud-CRDT** for Multiplayer Studio.
- **C4 / Stream-tee** — Workshop / History / Timeline / MCP all consume the same stream.
- **C5 / Duplicate types** — never re-declare shared structs.
- **C7 / Avian only** — replace any Rapier reference instantly.
- **C11 / `.eustress` world container** — Studio publishes a `.eustress` world container (Fjall WorldDb database + baked `.echk` chunks; was `.pak` pre-2026-05-16); Client never downloads. Upload mechanism unchanged.
- **C12 / Two storage modes** — Fjall WorldDb (single-author) ↔ cloud-CRDT (Multiplayer Studio); `SpaceLoader` detects at open.
- **C15 / Plugin sandbox** — WASM-first for third-party panels.
- Depends on **[03_MULTIPLAYER]** PIE; **[04_ASSETS]** asset resolver; **[10_TELEMETRY]** history tee; **[17_PLUGIN]** LSP + plugins.

---

## Open questions

- Q2.1 Monaco / Wry in-editor or LSP-only?
- Q2.2 PIE multiplayer = production binary or stripped variant?
- Q2.3 Constraint visualisation — Adornments or per-tool overlays?
- Q2.4 EEP v2 migration story.
- Q2.5 1.0 must-ship vs. nice-to-have panel cut list.
- Q2.6 Multiplayer Studio Loro schema (mirror or flat?).
- Q2.7 Internationalization framework choice (Fluent / gettext / custom).
- Q2.8 Export-versioning policy for binary snapshots.
