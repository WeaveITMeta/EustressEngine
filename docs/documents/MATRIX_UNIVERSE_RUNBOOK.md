# Matrix Universe — Build + Test Runbook

Status as of 2026-06-16: **the Matrix Universe scaffold already exists on disk.** This
runbook covers the two things that still require the live engine / MCP and cannot be
done with files alone — (1) making the engine GUI actually *boot into* Matrix, and
(2) seeding the first `SimCell` test once the Forge `sim` module (decision D2) is built.

The on-disk creation step is already done; it is documented here only so the layout is
reproducible.

---

## 0. What already exists on disk (no engine needed)

Created at `C:\Users\miksu\Documents\Eustress\Matrix\` (the real Documents folder,
NOT OneDrive — `workspace_root()` deliberately resolves `%USERPROFILE%\Documents\Eustress`):

```
Matrix/
├── .eustress/
│   ├── assets/parts/        # ball/block/cone/corner_wedge/cylinder/wedge .glb (engine defaults)
│   ├── assets/meshes/       # empty (custom-mesh dir)
│   └── knowledge/
└── Spaces/
    └── Sandbox/
        ├── space.toml                     # [space].name = "Sandbox"  (discovery marker)
        ├── simulation.toml                # default play-mode config (tick_rate_hz=60, auto_start=false)
        ├── .gitignore
        ├── .eustress/
        │   ├── project.toml               # discovery marker
        │   ├── local/  knowledge/
        ├── Workspace/                      # service marker #2 for looks_like_space_root
        │   ├── _service.toml
        │   ├── Baseplate/_instance.toml    # 512×1×512 anchored block
        │   └── WelcomeCube/_instance.toml  # 4×4×4 blue block
        ├── Lighting/   (_service.toml + Atmosphere/Moon/Sky/Sun .instance.toml)
        ├── MaterialService/  (_service.toml + full default .mat.toml set)
        ├── Players StarterGui StarterPack StarterPlayerScripts StarterCharacterScripts
        ├── ReplicatedStorage ServerStorage ServerScriptService SoulService
        ├── SoundService AdornmentService Teams Chat   (each with _service.toml)
        └── src/
```

All 47 TOML files parse cleanly. Detection predicates verified:
- `is_universe_dir(Matrix)` → **true** (non-hidden dir with a `Spaces/` child).
- `looks_like_space_root(Matrix/Spaces/Sandbox)` → **true** (`Workspace/` + `space.toml` + `.eustress/project.toml`).

Because `open_space()` runs `ensure_space_integrity()` on every open, any service /
lighting / material / `simulation.toml` / `space.toml` that were somehow missing would be
re-generated from the canonical templates on first open. The scaffold is intentionally a
superset, so nothing should need repair.

> **Recreating from scratch (if ever needed):** prefer the engine scaffold over hand-authoring.
> Run `FileEvent::NewUniverseConfirmed("Matrix")` → `space_ops::create_universe_folder`, then
> `NewSpaceConfirmed("Sandbox")` → `create_space_in_universe` → `scaffold_new_space` + `open_space`.
> Driven live this is the New Universe + New Space dialog, or via the engine bridge's `invoke_action`.

---

## 1. Register / confirm the Universe (MCP, read-only)

The `UniverseRegistry` Bevy resource rescans the workspace every 5 s and a notify watcher
fires on new `space.toml` / `project.toml`, so Matrix surfaces automatically. Confirm:

1. `mcp__eustress__list_universes` → **`Matrix`** must appear in the list.
2. `mcp__eustress__list_spaces { universe: "Matrix" }` → **`Sandbox`** must appear.

If Matrix does not appear within ~10 s, the engine is reading a different workspace root —
check that `EUSTRESS_WORKSPACE` is unset (it overrides Documents) and that the engine is
pointed at `C:\Users\miksu\Documents\Eustress` and not the OneDrive-redirected Documents.

---

## 2. Select Matrix for the MCP session (file-writing tools)

`mcp__eustress__set_active_universe { universe: "C:\\Users\\miksu\\Documents\\Eustress\\Matrix" }`

This only changes which Universe **this MCP session** resolves paths against (the path must
contain `Spaces/`). It does **not** change what the engine GUI opens — see step 3.

---

## 3. Make the engine GUI boot into Matrix  ← the real gotcha

**Startup trap (verified):** the GUI boot at `main.rs:220` calls `default_space_root()`,
which resolves `~/.eustress_engine/settings.json : last_space_path` FIRST, then falls back to
the first-alphabetical Space. It does **NOT** read the `.default_universe` sentinel.

Current live state (2026-06-16):
- `settings.json : last_space_path` = `...\ARC-AGI-3\Spaces\game_vc33-5430563c`  ← engine boots here today
- `.default_universe` = `Vehicle Simulator`  ← ignored by the GUI boot

So `set_next_launch_universe` / the `.default_universe` sentinel alone will **silently open
the wrong Space.** Use ONE of these instead:

**Option A — open it live (preferred; persists automatically).**
With the engine running, drive the open-space action on
`C:\Users\miksu\Documents\Eustress\Matrix\Spaces\Sandbox`:
- via the engine bridge open-space action (`do_open_space_path` → `open_space`), or
- manually via the Universes panel → Matrix → Sandbox.

`open_space()` sets `SpaceRoot` AND (via `slint_ui.rs:10392`) writes `last_space_path`, so the
choice persists across restarts.

**Option B — pre-write `last_space_path` for an unattended/headless boot.**
Only when the engine is NOT running (it overwrites `settings.json` on clean exit). Set:
```json
"last_space_path": "C:\\Users\\miksu\\Documents\\Eustress\\Matrix\\Spaces\\Sandbox"
```
in `C:\Users\miksu\.eustress_engine\settings.json`.

**Optional consistency:** also call
`mcp__eustress__set_next_launch_universe { universe: "Matrix" }` to update the
`.default_universe` sentinel for headless/streaming code (`port_file.rs`,
`stream_node_plugin.rs`). Remember this does NOT drive the GUI boot.

---

## 4. Confirm live

- `mcp__eustress__list_space_contents` / `mcp__eustress__inspect_scene` → Sandbox's
  `Workspace` shows **Baseplate** + **WelcomeCube**, lighting children present.
- `mcp__eustress__capture_viewport` or `ai_camera_capture` → a green/grey baseplate with a
  blue cube confirms meshes resolved (parts loaded from `Matrix/.eustress/assets/parts/`).

If primitives render as a magenta/checker fallback, the Universe-level `parts/*.glb` did not
resolve — re-run `ensure_universe_default_parts` (it runs automatically on `open_space`, so a
second open fixes it) or confirm the 6 GLBs are present in `Matrix/.eustress/assets/parts/`.

---

## 5. First `SimCell` test (DEFERRED until the Forge `sim` module exists)

> The first SimCell test depends on work that is **not built yet**: decision D2 puts the
> agents-in-sims scheduling code in a NEW `sim` module **inside** `eustress/crates/forge`
> (re-exporting `SimCell`, `SimWorld`, `AgentPolicy`, `Region3D`, `GangScheduler`, … from
> forge-orchestration 0.6.0). Phase 0b (Kernel + Rune DSL + validator, decision D3) is FIRST
> on the critical path and gates this. Do not attempt the SimCell test before those land.

When the `sim` module is in place, seed the first cell against Matrix/Sandbox as follows:

1. **Build only the small crate**, never the full engine:
   `cargo build -p eustress-forge` (per repo rules — the monolithic engine is ~10–15 min and
   must not be rebuilt mid-session).
2. **Map a `SimCell` to the Sandbox spatial substrate.** A `SimCell` carries a `Region3D`;
   align it to the engine's Morton streaming cell over the Baseplate so the agent's interest
   region matches resident geometry. Substrate reference:
   `eustress/crates/engine/src/space/residency.rs` and
   `docs/architecture/SCALING_ARCHITECTURE.md` (stream ≤100K by Morton camera-locality).
3. **State split (binding decision).** `WorldDb` (`eustress/crates/worlddb/src/`) remains the
   local entity source-of-truth for Sandbox; Forge `RaftStateStore` owns ONLY the
   cell-shared / cross-node replicated slice. In a single-node dev test the Raft store is not
   required (it is behind the `raft` / `raft-persist` cargo features and off by default), so
   the first test runs entirely against the local `world.fjalldb/` under Sandbox.
4. **Drive one agent tick.** Place one agent (`AgentPolicy`) in the `SimWorld`, give it a
   trivial goal (e.g. move toward the WelcomeCube), run a single scheduler tick
   (`TickDeadlineScheduler` / `GangScheduler` per the chosen algorithm), and read back the
   resulting entity transform via `mcp__eustress__inspect_scene` to confirm the sim mutated
   the live ECS through the WorldDb seam.
5. **Optional Rune monitor.** Once the Kernel/validator (Phase 0b) exists, a Rune script in
   `Sandbox/SoulService/` can watch the agent's position each tick; current Rune execution
   path: `eustress/crates/engine/src/soul/rune_ecs_module.rs`.

Acceptance for "Matrix is build+test ready for the sim work": steps 1–4 of section 0–4
(scaffold present, Universe+Space listed, GUI boots into Sandbox, Baseplate+WelcomeCube
visible) are all satisfiable **today**; section 5 unblocks the moment the Forge `sim` module
and Phase 0b land.

---

## Risks / gotchas (carried from the Matrix design)

- **Startup-selection trap:** `.default_universe` is NOT read by `default_space_root()`.
  Use step 3 Option A/B to actually boot into Matrix.
- **OneDrive:** create/inspect under `C:\Users\miksu\Documents\Eustress`, never the
  OneDrive-redirected Documents. `EUSTRESS_WORKSPACE` wins if set.
- **MCP `set_active_universe` vs engine `open_space` are different state.** Do both (steps 2
  and 3) or file-writing MCP tools target Matrix while the viewport still shows the old Space.
- **Name collisions:** `scaffold_new_space` / `create_universe_folder` error if the target
  dir already exists — `Matrix/` and `Matrix/Spaces/Sandbox/` now exist, so do not re-run the
  scaffold against them; open them instead.
- **Do not touch `crates/forge` or the kernel crate from this readiness task** — the SimCell
  test (section 5) is a forward reference, executed in a later workflow.
