# eustress-roblox-import

Roblox place file importer for Eustress Engine.

**Status**: Wave 1 scaffold. Not yet wired into the workspace. Public API
is stubbed; integration deferred to Wave 2 pending human approval of the
external dependency additions (rbx_dom_weak, rbx_binary, rbx_xml,
rbx_reflection_database — all MIT, all pure Rust, all rojo-rbx).

## Purpose

Parse `.rbxl`, `.rbxlx`, `.rbxm`, `.rbxmx` files (Roblox binary + XML place
and model formats) and materialise them as Eustress instances via
`eustress_common::instance_create::create_instance`. Targets idempotent
re-import (same file → same UUIDs via deterministic blake3 hashing).

## Spec

The full specification — pipeline diagram, class/property mapping tables,
asset resolution plan, idempotency strategy, performance targets, and
test strategy — lives at:

  `docs/architecture/ROBLOX_IMPORT_SPEC.md`

## Public API (planned)

```rust
use std::path::Path;
use eustress_roblox_import::{parse, import_into_space, ImportOptions};

let dom = parse(Path::new("Baseplate.rbxl"))?;
let report = import_into_space(&dom, Path::new("/path/to/SpaceRoot"),
    ImportOptions::default())?;
println!("Imported {} of {} nodes", report.total_nodes_imported,
    report.total_nodes_seen);
```

## Wave 2 todo

1. Uncomment the `rbx_*` deps in `Cargo.toml`.
2. Add `"crates/roblox-import"` to `eustress/Cargo.toml` `[workspace.members]`.
3. Replace the `todo!()` bodies in `parser.rs`, `class_map.rs`, `property_map.rs` with real implementations.
4. Wire `FileEvent::ImportRobloxPlace(PathBuf)` in `engine::ui::events` + `do_import_roblox_place` in `engine::ui::file_event_handler` (mirror `do_import_asset`).
5. Extend the viewport drop-target whitelist with the four rbx extensions.
6. Add fixtures under `tests/fixtures/` and goldens under `tests/golden/`.
