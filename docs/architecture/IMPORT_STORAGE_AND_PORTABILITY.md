# Import Storage Backends + Space Portability

Decision record (2026-06-01). Drives Wave 8: direct-binary import, the
headless space tool, and lossless serialization proof. Goal: a `.rbxl`
imports straight into a portable, scalable, perfectly-round-tripping
`.eustress` space — binary by default.

## The three formats (and when each wins)

| Format | Human-reads | Machine-reads @ scale | Role |
|--------|-------------|------------------------|------|
| TOML (`_instance.toml` tree) | trivial (text) | terrible (N files = N syscalls) | authoring / debug face |
| Fjall LSM-tree (`world.fjalldb/`) | opaque | excellent (1 handle, O(log n), range scans, Morton locality) | runtime/scale container |
| rkyv `ArchInstanceCore` | opaque | excellent (zero-copy `access`, no parse) | the value inside each Fjall key |

Binary = Fjall(container) + rkyv(values). One file handle, zero-copy reads,
Morton spatial keys. The only sane choice for 10K–10M entities (Vehicle
Simulator). TOML stays as the human/debug projection, produced on demand
via `arch_to_instance(core) -> InstanceDefinition -> toml`.

## Reading binary world state — you need Fjall as the entry point

You cannot read `world.fjalldb/` as raw bytes (compressed LSM blocks). You
open it through the `WorldDb` trait, which already exposes a full read API:
- `iter_instance_cores()` — all entities (rkyv bytes)
- `iter_instance_cores_in_region(min, max)` — Morton spatial scan
- `iter_class(class_name)` / `iter_class_capped` — by ClassName
- `get_entity_core_by_uuid(uuid)` — point lookup
- `iter_all_classes()` — class histogram
- `get_component` / `iter_component`, `iter_tree`, `list_dir` — files + components
Each rkyv value decodes via `rkyv::access::<ArchInstanceCore>` (pointer-cast,
zero-copy). The headless tool (below) wraps exactly this.

## ImportStorage option (ImportOptions, default BinaryDirect)

```rust
enum ImportStorage {
    BinaryDirect, // DEFAULT, RECOMMENDED — write Fjall cores directly,
                  //   skipping the TOML intermediary. Scales to millions.
    TomlFolders,  // readable _instance.toml tree (current behavior) — dev/debug
    Hybrid,       // per-part: representation_for_part decides
                  //   binary-classed (bare Part + primitive) -> Fjall core
                  //   file-natured (custom mesh / script / GUI)  -> TOML
}
```
CRITICAL: even BinaryDirect MUST honor `representation_for_part` — custom-mesh
(V-Cell!), scripts, and GUI are *inherently* file-natured and always write
TOML (the engine's file-watcher hot-loads them). "BinaryDirect" means
"primitives go straight to Fjall"; it never forces a file-natured class into
a core. So BinaryDirect and Hybrid differ only in whether plain primitives
ALSO get a TOML (Hybrid debug) — in practice BinaryDirect == Hybrid with the
TOML-for-primitives suppressed. Keep all three for clarity; document the overlap.

## Wave 8 build sequence

### 8.A — Importer direct-binary backend (#80)
The importer is engine-free today (deps: eustress-common only). Add an
OPTIONAL worlddb-writing backend behind a feature/option so the engine-free
default still builds:
- New trait `ImportSink` with two impls: `TomlSink` (current create_instance
  path) and `BinarySink` (writes ArchInstanceCore via worlddb).
- materializer calls `sink.write(class, def, parent)` instead of hardcoding
  create_instance. Sink choice from ImportStorage.
- BinarySink reuses instance_to_arch (the inverse of arch_to_instance) to
  bake the core, then worlddb put_instance_core + the 5 index stores
  (uuid, path, class_index, Morton). Deterministic UUID already done (identity.rs).
- Dep note: roblox-import gains an optional `eustress-worlddb` dep (orchestrator
  edits Cargo.toml). Keep TomlSink the no-worlddb fallback.

### 8.B — Headless space tool: `eustress-space` bin
New bin (crate: worlddb has the read API; put the bin in engine or a thin
new `eustress-space` crate that deps worlddb only — NOT the full engine, so
it stays fast + portable):
- `open <path>`   — entity count, class histogram (iter_all_classes), world
  bounds (Morton min/max), Fjall block/segment count. The "did it load" check.
- `verify <path>` — iterate every core, rkyv::access + CheckBytes, report any
  that fail round-trip. THE serialization-correctness gate.
- `export <path> --toml [outdir]` — arch_to_instance each core -> TOML tree.
  The portability escape hatch (binary -> readable).
This is what makes a `.eustress` portable + inspectable WITHOUT the engine.

### 8.C — Lossless proof
- Property test: for every one of the 228 mapped ClassNames, instance_to_arch
  -> arch_to_instance is identity (extend arch_instance.rs roundtrip test to
  the full class set, not just the core sample).
- Integration: import Vehicle Simulator --binary -> `eustress-space verify`
  must report 0 round-trip failures.

## How a `.eustress` space is opened (surface this in the Space directory)
A migrated space = `header.bin` (rkyv, carries migrated_at) + `world.fjalldb/`
+ `assets/` + `schema/`. `space_is_migrated()` gates binary vs legacy-TOML
load. The engine's `open_space()` already does this. The new `eustress-space`
bin gives a NO-ENGINE way to open/verify/export — that's the portability story
(GDPR: your data, in an open format, openable by a standalone tool you control).

## Editing imported objects (already mostly solved)
1. Studio human: select -> Properties panel -> `mirror_binary_ecs_changes`
   persists component edits to the core automatically (next frame).
2. AI/MCP (#79 landed): ecs.update mutates live components -> mirror persists.
3. TOML projection: serve arch_to_instance(core) -> TOML, edit text, apply
   back via instance_to_arch. TOML ergonomics on a binary core.
Editing is the most-solved part; the gaps are 8.A (direct import) + 8.B (tool).

## BINARY-PERSISTENCE GAP (verified 2026-06-01) — folds into 8.A

CONFIRMED: Wave 6/7 spawners all have `serialize() -> Vec::new()` (stub). The
binary core baker `world_db_binary::core_from_components` builds its
InstanceDefinition from ONLY {Instance, Transform, BasePart, Tags, mesh} and
sets `extra: HashMap::new()`. It has NO knowledge of the 106 typed config
components (UICorner, AudioReverb, VectorForce, Tool, StringValue, …). So:
- TOML save: ✅ lossless (spawner export_to_toml → [properties] table).
- Binary/Fjall save: ❌ class-specific fields DROPPED (mirror writes empty extra).
ALSO: the mirror system's `Changed<>` filter is {Transform,BasePart,Tags,
Instance} — it doesn't even fire when a typed config component changes, so an
edit to e.g. UICorner.corner_radius never persists to the core at all.

FIX (one path, all classes — NOT 106 serialize impls):
- core_from_components gains &World + &ClassRegistry access; for the entity's
  ClassName, call the registered spawner's export_to_toml(world, entity), parse
  its [properties]/extras table into def.extra. instance_to_arch ALREADY bakes
  def.extra (EXTRA_KEY) into the core, and arch_to_instance restores it — so the
  round-trip closes with one change at the baker, reusing the working TOML export.
- Add the typed config components to the mirror's Changed<> filter (or a
  generation/dirty bump on spawner apply_edit) so edits actually trigger a save.
This is Wave 8.A scope (same place the importer's BinarySink lands).
