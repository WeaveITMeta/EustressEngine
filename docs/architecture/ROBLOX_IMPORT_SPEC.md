# Roblox Place Importer — Specification

**Status**: Wave 1 (spec + scaffold). Not yet wired into the workspace.
**Crate**: `eustress-roblox-import` (location: `eustress/crates/roblox-import/`)
**Owner**: hyperskymeta@gmail.com
**Last revised**: 2026-05-26

---

## Table of Contents
1. Goals
2. Pipeline architecture
3. Crate dependencies & licensing
4. Module structure
5. ClassName mapping
6. Property mapping
7. Asset reference resolution
8. Idempotency & deterministic UUIDs
9. Error reporting (`ImportReport`)
10. Studio integration (File → Import → Roblox Place)
11. Cross-platform constraints
12. Performance targets
13. Test strategy
14. Future work flags

---

## 1. Goals

Eustress becomes a **Roblox-place importer**. A user with an existing `.rbxl`,
`.rbxlx`, `.rbxm`, or `.rbxmx` file can:

1. Drag-drop the file into the Studio viewport, **or**
2. Use **File → Import → Roblox Place…** from the menu bar.

The importer parses the file, walks the Roblox DataModel tree, maps each
instance to its closest Eustress `ClassName`, materialises one
`_instance.toml` per node via the canonical
`eustress_common::instance_create::create_instance` pipeline, and surfaces an
`ImportReport` summarising what made it through.

**Non-goals (Wave 1)**:
- No script execution. `Script` / `LocalScript` / `ModuleScript` bodies are
  copied verbatim into `LuauScript` / `LuauLocalScript` / `LuauModuleScript`
  TOML payloads but the source-level rewrites belong to `compat.rs`'s
  `ScriptTransformer` (already shipped) — invoked at materialisation time,
  not during parse.
- No asset download. `rbxassetid://` references become placeholder paths
  + a warning in the report (see §7).
- No terrain voxel data. `Terrain` becomes an empty `Terrain` placeholder.
- No `UnionOperation` CSG resolution. Body is preserved as opaque payload;
  the part falls back to its `MeshData` mesh if present, otherwise a default
  Block primitive sized to the AABB.

**Success criterion**: importing the Roblox Studio default place (`Baseplate`,
2 spawn locations, lighting, atmosphere, sky, terrain) reproduces a
visually-equivalent Eustress Space with no manual editing.

---

## 2. Pipeline architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│ STUDIO  (engine crate)                                              │
│                                                                     │
│   File menu → ImportRobloxPlace(path)                               │
│      │                                                              │
│      └─→ FileEvent::ImportRobloxPlace(PathBuf) ────┐                │
│                                                    │                │
│   Drop target on viewport ─→ (.rbxl|.rbxlx|.rbxm|.rbxmx) ─┘         │
└────────────────────────────────────────────────────┬────────────────┘
                                                     │
                                                     ▼
┌─────────────────────────────────────────────────────────────────────┐
│ eustress-roblox-import                          (this crate)        │
│                                                                     │
│  ┌──────────┐    ┌─────────────────┐    ┌────────────────────┐      │
│  │ parser   │ →  │ RobloxDom       │ →  │ class_map          │      │
│  │ (rbx_*)  │    │ (rbx_dom_weak)  │    │ (Roblox → Eustress)│      │
│  └──────────┘    └─────────────────┘    └─────────┬──────────┘      │
│                                                   │                 │
│                                                   ▼                 │
│                                         ┌────────────────────┐      │
│                                         │ property_map       │      │
│                                         │ (Variant → PropValue│      │
│                                         │  + InstanceOverrides│      │
│                                         └─────────┬──────────┘      │
│                                                   │                 │
│                                                   ▼                 │
│  ┌─────────────┐                       ┌────────────────────┐       │
│  │ identity    │ ← blake3(referent +   │ asset_resolver     │       │
│  │ (uuid v5    │   space salt)         │ (rbxassetid://)    │       │
│  │  per node)  │                       └─────────┬──────────┘       │
│  └──────┬──────┘                                 │                  │
│         │                                        │                  │
│         └────────────────┬───────────────────────┘                  │
│                          ▼                                          │
│                ┌─────────────────────┐                              │
│                │ import_report       │                              │
│                │  (built incrementally)                             │
│                └─────────┬───────────┘                              │
└──────────────────────────┼──────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────┐
│ eustress-common::instance_create::create_instance(...)              │
│   - copies class template                                           │
│   - patches _instance.toml with overrides + asset refs              │
│   - writes to <space_root>/<service>/<entity>/                      │
└──────────────────────────┬──────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────┐
│ file_watcher (engine::space::file_watcher) → ECS spawn              │
│ → eustress-worlddb (fjall) → live in Studio                         │
└─────────────────────────────────────────────────────────────────────┘
```

The new crate is **leaf-position**: it produces filesystem writes through the
`instance_create` API and never touches Bevy, the watcher, or fjall directly.
This keeps it portable (no bevy in deps) and testable against a temp dir.

---

## 3. Crate dependencies & licensing

All three target crates are first-party Roblox-ecosystem tools by **rojo-rbx**
(MIT-licensed; same project ownership and release cadence). They are pure
Rust with no native deps.

| Dep             | Version (Wave 2 target) | License        | Notes                                                               |
| --------------- | ----------------------- | -------------- | ------------------------------------------------------------------- |
| `rbx_dom_weak`  | `3.0` (latest stable)   | MIT            | In-memory DataModel (`WeakDom`, `Instance`, `Variant`).             |
| `rbx_binary`    | `1.0`                   | MIT            | Reads `.rbxl` / `.rbxm` into a `WeakDom`.                           |
| `rbx_xml`       | `1.0`                   | MIT            | Reads `.rbxlx` / `.rbxmx` into a `WeakDom`. Also writes if needed.  |
| `rbx_reflection_database` | `1.0`         | MIT            | Bundled property schema (versioned). Required to round-trip enums. |

**License compatibility**: Eustress crates are unlicensed in-tree but follow
MIT/Apache norms. All four deps are pure MIT — fully compatible. No GPL/AGPL
transitive risk. **deny.toml** action: none required; MIT is on the implicit
allowlist via crates.io and the workspace already ships MIT crates (e.g.
`reqwest`, `serde`).

**Multiple-versions risk** (workspace `deny.toml` has `multiple-versions =
"warn"`): the rbx_* family shares a single MAJOR cadence, so cargo's resolver
converges on one copy each. If a future rbx_dom_weak dep pulls in a second
`thiserror` major we'll pin and document.

**Not yet added.** Cargo.toml in the scaffold lists every dep
**commented-out** with the exact version line we'll uncomment in Wave 2 after
human approval.

---

## 4. Module structure

```
eustress/crates/roblox-import/
├── Cargo.toml          # package metadata, deps stubbed (commented)
├── README.md           # crate purpose + link to this spec
└── src/
    ├── lib.rs          # module exports, crate-level docs
    ├── parser.rs       # file → RobloxDom (4 format detection)
    ├── class_map.rs    # Roblox class → eustress ClassName
    ├── property_map.rs # Roblox Variant → eustress PropertyValue/Overrides
    ├── identity.rs     # blake3(referent + space_salt) → Uuid (REAL impl)
    ├── import_report.rs# ImportReport struct (REAL, not stub)
    └── error.rs        # ImportError enum (REAL, not stub)
```

**Module responsibilities**:

- **`parser`**: Detects format by magic bytes (binary: `<roblox!`;
  XML: `<roblox `). Dispatches to `rbx_binary::from_reader` or
  `rbx_xml::from_reader`. Returns a `RobloxDom` wrapper holding the
  `WeakDom` and a copy of the source path for diagnostics.

- **`class_map`**: Single function
  `roblox_to_eustress_class(rbx_class: &str) -> Option<ClassName>`.
  Delegates to `eustress_common::luau::compat::ClassMapping::map_class`
  for the string mapping, then re-resolves the returned `&str` against
  the `ClassName` enum via a local `from_str`-style helper.

- **`property_map`**: Per-Variant translator + bulk transform returning
  `PropertyBag` (typed `InstanceOverrides` + `HashMap<String, PropertyValue>`
  for extras).

- **`identity`**: One function `entity_uuid(space_salt, referent) -> Uuid`
  using blake3. Real implementation, has unit tests.

- **`import_report`** + **`error`**: see §9. Real types, not stubs.

---

## 5. ClassName mapping

Delegates to `eustress-common::luau::compat::ClassMapping::map_class`. Table
as of 2026-05-26:

| Roblox class       | Eustress `ClassName`     | Notes                                                                  |
| ------------------ | ------------------------ | ---------------------------------------------------------------------- |
| `Part`             | `Part`                   | Direct.                                                                |
| `MeshPart`         | `Part`                   | `MeshId` mapped to `asset_mesh` in overrides.                          |
| `WedgePart`        | `Part` (shape=Wedge)     | `PartType::Wedge` sticker on creation.                                 |
| `CornerWedgePart`  | `Part` (shape=CornerWedge)| `PartType::CornerWedge`.                                              |
| `TrussPart`        | `Part`                   | Wave 1: degrades to Block; logged as `ApproximatedShape`.              |
| `SpawnLocation`    | `SpawnLocation`          | Direct.                                                                |
| `Seat`             | `Seat`                   | Direct.                                                                |
| `VehicleSeat`      | `VehicleSeat`            | Direct.                                                                |
| `Model`            | `Model`                  | Direct (container).                                                    |
| `Folder`           | `Folder`                 | Direct.                                                                |
| `PointLight`       | `PointLight`             | Direct.                                                                |
| `SpotLight`        | `SpotLight`              | Direct.                                                                |
| `SurfaceLight`     | `SurfaceLight`           | Direct.                                                                |
| `WeldConstraint`   | `WeldConstraint`         | Direct.                                                                |
| `Motor6D`          | `Motor6D`                | Direct.                                                                |
| `Attachment`       | `Attachment`             | Direct.                                                                |
| `HingeConstraint`  | `HingeConstraint`        | Direct.                                                                |
| `ScreenGui`        | `ScreenGui`              | Direct.                                                                |
| `BillboardGui`     | `BillboardGui`           | Direct.                                                                |
| `SurfaceGui`       | `SurfaceGui`             | Direct.                                                                |
| `Frame`            | `Frame`                  | Direct.                                                                |
| `TextLabel`        | `TextLabel`              | Direct.                                                                |
| `TextButton`       | `TextButton`             | Direct.                                                                |
| `TextBox`          | `TextBox`                | Direct.                                                                |
| `ImageLabel`       | `ImageLabel`             | Direct; `Image` → `asset_path`.                                        |
| `ImageButton`      | `ImageButton`            | Direct; `Image` → `asset_path`.                                        |
| `ScrollingFrame`   | `ScrollingFrame`         | Direct.                                                                |
| `ViewportFrame`    | `ViewportFrame`          | Direct.                                                                |
| `ParticleEmitter`  | `ParticleEmitter`        | Direct.                                                                |
| `Beam`             | `Beam`                   | Direct.                                                                |
| `Sound`            | `Sound`                  | `SoundId` → `asset_path` (placeholder Wave 1).                         |
| `Script`           | `LuauScript`             | Source preserved; `ScriptTransformer::transform` invoked.              |
| `LocalScript`      | `LuauLocalScript`        | Same.                                                                  |
| `ModuleScript`     | `LuauModuleScript`       | Same.                                                                  |
| `RemoteEvent`      | `RemoteEvent`            | Direct.                                                                |
| `RemoteFunction`   | `RemoteFunction`         | Direct.                                                                |
| `BindableEvent`    | `BindableEvent`          | Direct.                                                                |
| `BindableFunction` | `BindableFunction`       | Direct.                                                                |
| `Sky`              | `Sky`                    | Direct.                                                                |
| `Atmosphere`       | `Atmosphere`             | Direct.                                                                |
| `Clouds`           | `Clouds`                 | Direct.                                                                |
| `Terrain`          | `Terrain`                | **Empty placeholder Wave 1**; voxel data discarded with warning.       |
| `Humanoid`         | `Humanoid`               | Direct.                                                                |
| `Animator`         | `Animator`               | Direct.                                                                |
| `Camera`           | `Camera`                 | Direct.                                                                |
| `SpecialMesh`      | `SpecialMesh`            | Direct; `MeshId` → `asset_mesh`.                                       |
| `Decal`            | `Decal`                  | Direct; `Texture` → `asset_path`.                                      |
| `UnionOperation`   | `Part` (fallback)        | CSG body discarded; AABB-sized Block + `ApproximatedShape` warning.    |
| **anything else**  | _unmapped_               | Logged as `ImportReport::unmapped_classes`; subtree skipped.           |

**Service classes** (`Workspace`, `Lighting`, `Players`, etc.) become destination
directories under `<space_root>/<ServiceName>/`, not materialised as instances.

---

## 6. Property mapping

| Roblox `Variant`            | Eustress equivalent                 | Conversion notes                                                                 |
| --------------------------- | ----------------------------------- | -------------------------------------------------------------------------------- |
| `Bool`                      | `PropertyValue::Bool`               | Direct.                                                                          |
| `String`                    | `PropertyValue::String`             | Direct.                                                                          |
| `Int32`/`Int64`             | `PropertyValue::Int(i32)`           | i64 truncated with `TruncatedInt` warning if out of i32 range.                   |
| `Float32`/`Float64`         | `PropertyValue::Float(f32)`         | f64 downcast to f32; loss accepted (Eustress is f32 everywhere).                 |
| `Vector2`                   | `PropertyValue::Vector2`            | Direct.                                                                          |
| `Vector3`                   | `PropertyValue::Vector3 (Vec3)`     | Direct — `STUD_TO_METERS = 1.0` confirmed (see `services::workspace`).           |
| `CFrame`                    | `PropertyValue::Transform`          | Rotation matrix → `Quat::from_mat3`; position → translation; scale = 1.          |
| `Color3`                    | `PropertyValue::Color3`             | f32 [0..1].                                                                      |
| `Color3uint8`               | `PropertyValue::Color3`             | u8/255 → f32.                                                                    |
| `BrickColor`                | `PropertyValue::Color3`             | Static 128-entry palette lookup (rbx_dom_weak ships this).                       |
| `UDim`                      | `PropertyValue::Float (offset)`     | `scale` discarded with `LossyUDim` warning.                                      |
| `UDim2`                     | `PropertyValue::UDim2`              | Direct via `crate::ui_types::UDim2`.                                             |
| `Enum` (token + reflection) | `PropertyValue::Enum(String)`       | Label resolved via `rbx_reflection_database`; raw int kept as fallback.          |
| `Content` (rbxassetid)      | `InstanceOverrides::asset_path`     | See §7.                                                                          |
| `ProtectedString` (source)  | inline TOML `source` field          | Routed to script TOML via `extras`.                                              |
| `BinaryString`              | base64 string in `extras`           | Wave 1: opaque preservation.                                                     |
| `SharedString`              | base64 string in `extras`           | Same.                                                                            |
| `NumberSequence`            | JSON keyframe array in `extras`     | Wave 2 owns a real `NumberSequence` PropertyValue.                               |
| `ColorSequence`             | JSON keyframe array in `extras`     | Same.                                                                            |
| `NumberRange`               | `[f32; 2]` array in `extras`        | Same.                                                                            |
| `Rect`                      | `[f32; 4]` array in `extras`        | Same.                                                                            |
| `PhysicalProperties`        | fields in `extras`                  | Same.                                                                            |
| `Ray`                       | `[f32; 6]` array in `extras`        | Same.                                                                            |
| `Faces` / `Axes`            | bitset string in `extras`           | Same.                                                                            |
| `Ref` (Instance reference)  | `Uuid` (via §8) in `extras`         | Resolved by referent → uuid lookup. If unresolved, logged.                       |

**`extras` block**: properties without a first-class slot get written into a
`[properties.extras]` sub-table. The file watcher already round-trips unknown
keys, so no engine work is needed for Wave 1 to preserve them.

---

## 7. Asset reference resolution

Roblox URIs:
- `rbxassetid://NNNNNNNNN` — by numeric ID, on Roblox CDN.
- `rbxasset://...` — packaged with Studio install.
- `http(s)://...` — direct URL (deprecated).

**Wave 1**: emit placeholder local paths + `AssetWarning` per occurrence. No
network.

**Wave 2 hook** — `AssetFetcher` trait:
```rust
pub trait AssetFetcher: Send + Sync {
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, FetchError>;
}
```
`ImportOptions.asset_fetcher: Option<Arc<dyn AssetFetcher>>`. Default `None`
keeps Wave 1 behaviour; an integrator can plug in a community mirror.

---

## 8. Idempotency & deterministic UUIDs

```rust
pub fn entity_uuid(space_salt: &[u8], referent: &str) -> Uuid {
    let mut hasher = blake3::Hasher::new();
    hasher.update(space_salt);
    hasher.update(b":eustress-roblox-import:");
    hasher.update(referent.as_bytes());
    let hash = hasher.finalize();
    let bytes: [u8; 16] = hash.as_bytes()[..16].try_into().unwrap();
    Uuid::from_bytes(bytes)
}
```

- `space_salt` = Space's own UUID. Different Spaces → different uuids.
- `referent` = Roblox per-DataModel ID (e.g. `RBX0123456789ABCDEF`).
- `blake3` already a workspace dep. First 16 bytes is collision-safe.

Same `(salt, referent)` → byte-exact same uuid → re-importing unchanged places
produces zero worlddb churn. Reference broader identity story:
`docs/AUDIT/08_IDENTITY_TRUST.md`.

---

## 9. Error reporting

### `ImportError` (hard failure — short-circuits)
See `src/error.rs` for the real `thiserror`-backed enum.

### `ImportReport` (soft, accumulated)
See `src/import_report.rs` for all real types: `ImportReport`, `ClassCount`,
`UnmappedClass`, `UnmappedProperty`, `AssetWarning`, `ScriptWarning`,
`Approximation`.

The engine's UI integration (Wave 2) renders this as a modal post-import
dialog. The report is also archived to
`<space_root>/.eustress/import_reports/<ts>.json`.

---

## 10. Studio integration (Wave 2)

### File menu
`File → Import → Roblox Place…` next to existing import entries. Mirrors
`do_import_asset` menu shape.

### Drop target
Extend viewport drop-target whitelist with `.rbxl`, `.rbxlx`, `.rbxm`,
`.rbxmx`. Route to new `do_import_roblox_place(world, path)`.

### Modal UI
```
┌────────────────────────────────────────────────────────┐
│ Import Roblox Place                                [X] │
├────────────────────────────────────────────────────────┤
│  File:    Baseplate.rbxl  (12.4 KB)                    │
│  Format:  Binary place file (.rbxl)                    │
│                                                        │
│  Destination Space:                                    │
│   ◉ Active Space:  Universe1 / Baseplate              │
│   ○ New Space:     [____________] in Universe1        │
│                                                        │
│  Options:                                              │
│   [✓] Preserve Roblox referents (idempotent re-import)│
│   [✓] Transform scripts via compat layer              │
│   [ ] Download assets from rbxassetid:// (Wave 2)     │
│                                                        │
│             [ Cancel ]    [ Import ▶ ]                │
└────────────────────────────────────────────────────────┘
```

After import, a second modal shows the `ImportReport` summary.

---

## 11. Cross-platform constraints

- **Pure Rust, no native libs.** All deps verified pure-Rust.
- **No filesystem assumptions beyond `std::path`.**
- **No tokio/async.** Parser blocking. `import_into_space` runs in Bevy
  exclusive system. Wave 2 may push to worker thread.
- **Builds for Linux / Mac / Windows.** WASM not a target.

---

## 12. Performance targets

| Scenario                                | Wave 1 target | Stretch |
| --------------------------------------- | ------------- | ------- |
| 1 000-instance place                    | < 0.5 s       | < 0.2 s |
| 10 000-instance place                   | < 5 s         | < 2 s   |
| 100 000-instance place (Adopt-Me-scale) | < 60 s        | < 20 s  |

Bottlenecks expected: per-node `instance_create` fs writes (batchable Wave 2
via direct worlddb API); CFrame → Quat trig (inlined ~50ns); blake3 (~200ns).

---

## 13. Test strategy

### Unit tests
- `class_map`: every entry in §5 exercised.
- `property_map`: each Variant arm round-trip tested.
- `identity`: golden-uuid test.
- `asset_resolver`: URL parsing for 3 schemes.

### Integration tests (fixtures)
- `tests/fixtures/baseplate.rbxlx` (Studio default, XML)
- `tests/fixtures/baseplate.rbxl` (binary, same expected output)
- `tests/fixtures/spawn-and-script.rbxlx`
- `tests/fixtures/rbxassetid-decal.rbxlx`

### Benchmark
- Synthetic 10k-Part place; wall-clock < 5s.

---

## 14. Future work flags

- **Script execution** (Wave 3): wire mlua via existing `mlua = "0.10"` dep.
- **Terrain** (Wave 4): `eustress-terrain` adapter for voxel data.
- **Packages / Replicas** (Wave 5): one Eustress entity + N references.
- **UnionOperation CSG** (Wave 6): use existing `truck-shapeops` dep.
- **Round-trip export**: Eustress → `.rbxl` via `rbx_xml` writer (not planned).
- **Asset mirror**: live download via `AssetFetcher` hook — belongs in
  separate `eustress-roblox-assets` crate to quarantine network deps.
