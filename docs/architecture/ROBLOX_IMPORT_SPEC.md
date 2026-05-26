# Roblox Place Importer — Specification (FINAL)

**Status**: Implementation-ready. Wave 2 ships everything in this document.
**Crate**: `eustress-roblox-import` (location: `eustress/crates/roblox-import/`)
**Owner**: hyperskymeta@gmail.com
**Last revised**: 2026-05-26

---

## 0. What this document is

This spec is the single source of truth for the Roblox-place importer. It is
**not** an aspirational sketch. Every section names concrete crates, exact
property paths, deterministic algorithms, and on-disk targets. A Wave-2
implementer should be able to execute this document end-to-end without
asking "do we really skip terrain?" or "what do we do for CSG?". The answers
are below in §6 and §7 respectively, and they are "no, we solve it" and
"baked-mesh extraction + opt-in CSG recomputation through `eustress-cad`".

The previous draft (commit history of this file) deferred terrain, CSG, and
event classes. That was the wrong call. Roblox terrain is a documented
binary blob, CSG bodies ship as baked meshes inside the same blob, and the
four event/function classes are empty containers with no properties beyond
`Name`. All three are solved here.

---

## Table of Contents
1. Goals + What can ALL be ported
2. Pipeline architecture
3. Crate dependencies + licensing
4. Module structure
5. Service mapping (Roblox service → Eustress destination folder)
6. Terrain — full voxel import
7. CSG (UnionOperation / NegateOperation / IntersectOperation)
8. Events + Functions (RemoteEvent, RemoteFunction, BindableEvent, BindableFunction)
9. ClassName mapping (the long table)
10. Property mapping (Variant → PropertyValue)
11. Asset reference resolution
12. Idempotency + deterministic UUIDs
13. Error reporting (`ImportReport`)
14. Studio integration (File → Import → Roblox Place)
15. `import_into_space` API
16. Cross-platform constraints
17. Performance targets
18. Test strategy
19. Phase 2 (genuinely deferred — short list)

---

## 1. Goals + What can ALL be ported

Eustress becomes a **Roblox-place importer of record**. A user with an existing
`.rbxl`, `.rbxlx`, `.rbxm`, or `.rbxmx` file can:

1. Drag-drop the file into the Studio viewport, **or**
2. Use **File → Import → Roblox Place…** from the menu bar.

The importer parses the file, walks the Roblox DataModel tree, **routes every
child of every service to its matching Eustress service folder** (§5), maps each
instance to its closest Eustress `ClassName`, materialises one `_instance.toml`
per node via the canonical `eustress_common::instance_create::create_instance`
pipeline, decodes terrain into Eustress voxel chunks (§6), extracts CSG baked
meshes to `.glb` assets (§7), creates first-class event/function entities (§8),
and surfaces an `ImportReport` summarising what made it through.

### What ALL can be ported

We target **≈95% of Roblox-place content**, on first import, with no manual
fixup. The list of supported content:

| Domain                | Ported | How                                                   |
| --------------------- | ------ | ----------------------------------------------------- |
| Parts (Part/MeshPart/Wedge/CornerWedge/Truss)      | ✅ | §9 ClassName, §10 Property — direct geometry mapping. |
| Models + Folders                                    | ✅ | §9 — containers preserved as-is.                      |
| Lighting (Sky, Atmosphere, Clouds, lights)          | ✅ | §9 + §5 — Lighting service routing.                   |
| Terrain (voxels, materials, water)                  | ✅ | §6 — SmoothGrid decoded to Eustress chunks.           |
| CSG (UnionOperation / NegateOperation / IntersectOperation) | ✅ | §7 — baked mesh + opt-in recompute. |
| Scripts (Script / LocalScript / ModuleScript)       | ✅ | §9 — source copied, transformed via `compat::ScriptTransformer`. |
| Events / Functions (Remote*/Bindable*)              | ✅ | §8 — first-class entities under the correct service.  |
| GUI (ScreenGui/SurfaceGui/BillboardGui + children)  | ✅ | §9 — direct mapping for Frame/TextLabel/TextButton/ImageLabel/etc. |
| Sounds + SoundService                               | ✅ | §9 + §5 + §11 — SoundId resolved by asset_resolver.  |
| Particles, Beams, Decals, Attachments               | ✅ | §9 — direct mapping.                                  |
| Constraints (Weld, Motor6D, Hinge, etc.)            | ✅ | §9 — direct mapping (Avian-compatible).               |
| Humanoid + Animator                                 | ✅ | §9 — direct mapping.                                  |
| Camera                                              | ✅ | §9 — direct mapping.                                  |
| Players / Teams / Chat config                       | ✅ | §5 — service-folder placement; runtime semantics follow Eustress conventions. |
| StarterGui / StarterPack / StarterPlayerScripts     | ✅ | §5 — template-folder placement.                       |
| ReplicatedStorage / ServerScriptService / ServerStorage | ✅ | §5 — script/asset containers.                     |
| MaterialVariants                                    | ✅ | §5 — routed to `MaterialService/`.                    |
| `rbxassetid://` references                          | ⚠️ Partial | §11 — placeholder paths + warnings unless `asset_fetcher` is supplied. |

### What is the 5% gap

Three categories of content we explicitly do **not** import:

1. **Roblox Studio plugins themselves** — Eustress does not host the Roblox
   Studio plugin runtime, and never will. Plugins are arbitrary Lua bound to
   Roblox-Studio-only APIs (`plugin:GetMouse()`, `DockWidgetPluginGui`, etc.).
   The importer skips `Plugin` instances with a clear warning.
2. **Roblox CDN avatar / marketplace assets** — `rbxassetid://` references
   require fetching from Roblox's CDN. We do not implement that fetch in-tree
   to avoid burning ToS or breaking on network outages. The `AssetFetcher`
   trait (§11) lets an integrator plug in a community mirror; default is
   no-network.
3. **Roblox-proprietary services with no Eustress cognate** —
   `MarketplaceService`, `TeleportService`, `BadgeService`,
   `GroupService`, `NotificationService`. The `compat::ServiceMapping` table
   maps the names so that scripts referencing `game:GetService("…")` resolve
   to Eustress equivalents (Eustress provides its own `ShopService`,
   `PortalService`, etc.). Instances *inside* these services in the source
   `.rbxl` are routed to `_imported/<ServiceName>/` with a warning — the
   user can decide what to do with them after import.

Outside those three buckets, the importer is comprehensive.

**Success criterion**: importing the Roblox Studio default place (`Baseplate`,
2 spawn locations, lighting, atmosphere, sky, terrain) reproduces a
visually-equivalent Eustress Space with no manual editing.

**Stretch success criterion**: importing a real community place such as
"Adopt Me!" stripped of CDN-only assets (≈80,000 instances) succeeds with a
report that itemises only the marketplace/plugin gaps — every Part, light,
script, terrain voxel, and CSG body lands.

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
│  │ parser   │ →  │ RobloxDom       │ →  │ service_router     │      │
│  │ (rbx_*)  │    │ (rbx_dom_weak)  │    │ (Roblox service →  │      │
│  └──────────┘    └─────────────────┘    │  Eustress folder)  │      │
│                                          └─────────┬──────────┘     │
│                                                    │                │
│                                                    ▼                │
│                                          ┌────────────────────┐     │
│                                          │ class_map          │     │
│                                          │ (Roblox class →    │     │
│                                          │  Eustress ClassName│     │
│                                          └─────────┬──────────┘     │
│                                                    │                │
│         ┌──────────────────────────────┬───────────┴────┬───────┐   │
│         ▼                              ▼                ▼       ▼   │
│  ┌──────────────┐  ┌──────────────────┐  ┌────────────┐ ┌──────┐    │
│  │ terrain      │  │ csg              │  │ events     │ │ ...  │    │
│  │ (§6)         │  │ (§7)             │  │ (§8)       │ │ misc │    │
│  │ SmoothGrid → │  │ MeshData →       │  │ remote/    │ │      │    │
│  │ Eustress     │  │  baked .glb +    │  │ bindable → │ │      │    │
│  │ chunks       │  │  CSGSource (opt) │  │ entities   │ │      │    │
│  └──────┬───────┘  └────────┬─────────┘  └─────┬──────┘ └──┬───┘    │
│         │                   │                  │            │       │
│         └─────────┬─────────┴──────────────────┴────────────┘       │
│                   ▼                                                 │
│        ┌────────────────────────┐                                   │
│        │ property_map           │                                   │
│        │ (Variant →             │                                   │
│        │  PropertyValue +       │                                   │
│        │  InstanceOverrides)    │                                   │
│        └─────────┬──────────────┘                                   │
│                  │                                                  │
│                  ▼                                                  │
│  ┌────────────────────────┐  ┌────────────────────────┐             │
│  │ identity (uuid v5 via  │  │ asset_resolver         │             │
│  │ blake3(referent + salt)│  │ (rbxassetid://, …)     │             │
│  └─────────┬──────────────┘  └─────────┬──────────────┘             │
│            │                            │                           │
│            └──────────────┬─────────────┘                           │
│                           ▼                                         │
│                ┌─────────────────────┐                              │
│                │ import_report       │                              │
│                │  (built incrementally)                             │
│                └─────────┬───────────┘                              │
└──────────────────────────┼──────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────┐
│ eustress-common::instance_create::create_instance(...)              │
│   - dest_dir = service_router.route_for(<service>) (see §5)         │
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

The four boxes spanning the middle of the pipeline (terrain, csg, events,
misc) are dispatched at the per-node walker layer. Each owns one Roblox
class family. Misc covers everything not handled by a dedicated dispatcher.

---

## 3. Crate dependencies + licensing

All four target crates are first-party Roblox-ecosystem tools by **rojo-rbx**
(MIT-licensed; same project ownership and release cadence). They are pure
Rust with no native deps.

| Dep             | Version (Wave 2 target) | License        | Notes                                                               |
| --------------- | ----------------------- | -------------- | ------------------------------------------------------------------- |
| `rbx_dom_weak`  | `3.0` (latest stable)   | MIT            | In-memory DataModel (`WeakDom`, `Instance`, `Variant`).             |
| `rbx_binary`    | `1.0`                   | MIT            | Reads `.rbxl` / `.rbxm` into a `WeakDom`.                           |
| `rbx_xml`       | `1.0`                   | MIT            | Reads `.rbxlx` / `.rbxmx` into a `WeakDom`. Also writes if needed.  |
| `rbx_reflection_database` | `1.0`         | MIT            | Bundled property schema (versioned). Required to round-trip enums and to identify the `SmoothGrid` / `MaterialColors` property shapes for Terrain. |

In-tree deps already present in the workspace and reused by this crate:

| Dep                       | Used in §  | Purpose                                                  |
| ------------------------- | ---------- | -------------------------------------------------------- |
| `eustress-common`         | everywhere | `ClassName` enum, `instance_create`, `terrain::*`, `luau::compat`, `units`. |
| `eustress-cad`            | §7         | `truck-shapeops` boolean ops for CSG recomputation.       |
| `blake3`                  | §12        | Deterministic per-node UUIDs.                            |
| `uuid`                    | §12        | UUID type.                                                |
| `tracing`                 | logging    | Workspace-standard.                                       |
| `serde`/`serde_json`      | §13        | `ImportReport` serialisation.                            |
| `thiserror`               | §13        | `ImportError`.                                            |
| `hex`                     | helpers    | Asset-path safe encoding.                                 |
| `glam` (transitive via common) | §10   | `Vec3`, `Quat`, `Mat3` for CFrame conversion.            |

For the CSG path (§7) we additionally need a tiny mesh-export helper:

| Dep                  | Version | License        | Notes                                                       |
| -------------------- | ------- | -------------- | ----------------------------------------------------------- |
| `gltf` (writer mode) | `1.4`   | MIT/Apache-2.0 | Writes `.glb` for the baked CSG mesh. Workspace-shared once. |

**License compatibility**: Eustress crates are unlicensed in-tree but follow
MIT/Apache norms. All deps are MIT or MIT/Apache — fully compatible. No
GPL/AGPL transitive risk. **deny.toml** action: none required.

**Multiple-versions risk** (workspace `deny.toml` has `multiple-versions =
"warn"`): the rbx_* family shares a single MAJOR cadence, so cargo's
resolver converges on one copy each. The `gltf` crate is already evaluated
for compatibility (used elsewhere by Bevy's loader and by mesh-export
tools). If a future rbx_dom_weak dep pulls in a second `thiserror` major
we'll pin and document.

**Cargo.toml in the scaffold** lists every Roblox-format dep
commented-out in the current scaffold. Wave 2 uncomments those lines and
adds the `gltf` writer dep.

---

## 4. Module structure

```
eustress/crates/roblox-import/
├── Cargo.toml          # package metadata, deps stubbed (commented)
├── README.md           # crate purpose + link to this spec
└── src/
    ├── lib.rs          # module exports, crate-level docs
    ├── parser.rs       # file → RobloxDom (4 format detection)
    ├── service_router.rs # Roblox service class → space-relative folder (§5)
    ├── class_map.rs    # Roblox class → eustress ClassName
    ├── property_map.rs # Roblox Variant → eustress PropertyValue/Overrides
    ├── terrain.rs      # SmoothGrid + MaterialColors decode (§6)
    ├── csg.rs          # UnionOperation/NegateOperation/IntersectOperation (§7)
    ├── events.rs       # RemoteEvent/RemoteFunction/BindableEvent/BindableFunction (§8)
    ├── asset_resolver.rs # rbxassetid:// + rbxasset:// + http(s):// (§11)
    ├── identity.rs     # blake3(referent + space_salt) → Uuid (§12)
    ├── import_report.rs# ImportReport struct
    ├── error.rs        # ImportError enum
    └── walk.rs         # tree walker that dispatches to the specialists
```

**Module responsibilities**:

- **`parser`**: Detects format by magic bytes (binary: `<roblox!`;
  XML: `<roblox `). Dispatches to `rbx_binary::from_reader` or
  `rbx_xml::from_reader`. Returns a `RobloxDom` wrapper holding the
  `WeakDom` and a copy of the source path for diagnostics.

- **`service_router`**: One struct `ServiceRouter` (see §5). Maps the
  Roblox service class of an instance's nearest service ancestor to the
  on-disk folder inside the target Space.

- **`walk`**: Depth-first traverses the DOM rooted at `DataModel`. For each
  child of `DataModel` (each service), looks up the destination via
  `service_router`. For each descendant, dispatches by class family:
  - Terrain → `terrain::import_terrain`.
  - UnionOperation / NegateOperation / IntersectOperation → `csg::import_csg`.
  - RemoteEvent / RemoteFunction / BindableEvent / BindableFunction →
    `events::import_event`.
  - Anything else → the generic `class_map` + `property_map` path.

- **`class_map`**: Single function
  `roblox_to_eustress_class(rbx_class: &str) -> Option<ClassName>`.
  Delegates to `eustress_common::luau::compat::ClassMapping::map_class`
  for the string mapping, then re-resolves the returned `&str` against
  the `ClassName` enum via a local `from_str`-style helper.

- **`property_map`**: Per-Variant translator + bulk transform returning
  `PropertyBag` (typed `InstanceOverrides` + `HashMap<String, PropertyValue>`
  for extras).

- **`terrain`**: §6.

- **`csg`**: §7.

- **`events`**: §8.

- **`identity`**: One function `entity_uuid(space_salt, referent) -> Uuid`
  using blake3. Real implementation, has unit tests.

- **`import_report`** + **`error`**: see §13. Real types, not stubs.

---

## 5. Service mapping (Roblox service → Eustress destination folder)

Roblox places contain a top-level `DataModel` whose children are services —
`Workspace`, `Lighting`, `Players`, etc. Eustress mirrors this with one
folder per service under `<space_root>/<ServiceName>/`, each containing a
`_service.toml` with the service's properties. **Children of a Roblox
service must land in the matching Eustress service folder**, not in a
single dumping ground. This is what makes the import look right in
Explorer.

### Service router table

The disk layout under `<space_root>/` is confirmed by inspection of
`C:/Users/miksu/Documents/Eustress/Universe1/Spaces/Space1/`:

```
Space1/
├── Workspace/             # 3D world content
├── Lighting/              # Sky, Atmosphere, lights, post-FX
├── Players/               # Player container (template)
├── StarterGui/            # Per-player GUI templates
├── StarterPack/           # Per-player tool inventory
├── StarterCharacterScripts/  # Per-character scripts
├── StarterPlayerScripts/  # Per-player scripts
├── ServerStorage/         # Server-only assets
├── ServerScriptService/   # Server-only scripts
├── ReplicatedStorage/     # Client+server shared
├── SoundService/          # Audio defaults + groups
├── Chat/                  # Chat config
├── Teams/                 # Team definitions
├── MaterialService/       # MaterialVariants
├── SoulService/           # Eustress-only — AI brain. Importer NEVER touches.
├── AdornmentService/      # Eustress-only — editor gizmos. Importer NEVER touches.
├── _retired_layers/       # Eustress historical. Importer NEVER touches.
├── header.bin             # Space header
├── simulation.toml        # Sim config
├── space.toml             # Space metadata
└── world.fjalldb/         # Binary entity store
```

The router maps each Roblox service to a destination:

| Roblox service (`game:GetService(…)`) | Eustress destination       | Notes                                                                  |
| ------------------------------------- | -------------------------- | ---------------------------------------------------------------------- |
| `Workspace`                           | `Workspace/`               | World-positioned 3D content. CFrame interpreted directly.              |
| `Lighting`                            | `Lighting/`                | Sky, Atmosphere, lights, post-FX. Properties merged into `_service.toml`.|
| `Players`                             | `Players/`                 | Container template (no live player instances at import time).          |
| `StarterGui`                          | `StarterGui/`              | Per-player GUI templates.                                              |
| `StarterPack`                         | `StarterPack/`             | Per-player tool inventory.                                             |
| `StarterPlayer`                       | (split)                    | See "StarterPlayer special case" below.                                |
| `StarterPlayer / StarterPlayerScripts`| `StarterPlayerScripts/`    | LocalScripts that run per player.                                      |
| `StarterPlayer / StarterCharacterScripts` | `StarterCharacterScripts/` | LocalScripts that run per character spawn.                           |
| `ReplicatedStorage`                   | `ReplicatedStorage/`       | Shared client+server.                                                  |
| `ReplicatedFirst`                     | `ReplicatedStorage/_replicated_first/` | Eustress collapses; `_replicated_first/` flagged as priority. |
| `ServerScriptService`                 | `ServerScriptService/`     | Server-only scripts.                                                   |
| `ServerStorage`                       | `ServerStorage/`           | Server-only assets.                                                    |
| `SoundService`                        | `SoundService/`            | Audio defaults + groups; properties merged into `_service.toml`.       |
| `Chat`                                | `Chat/`                    | Chat config.                                                           |
| `Teams`                               | `Teams/`                   | Team definitions.                                                      |
| `MaterialService`                     | `MaterialService/`         | MaterialVariants. Eustress cognate.                                    |
| `Debris`                              | (no folder)                | Runtime-only service; instances don't have children in source files.   |
| `RunService`/`UserInputService`/`TweenService`/`HttpService`/`DataStoreService`/`PathfindingService`/`CollectionService`/`TextService`/`LocalizationService`/`GuiService`/`PhysicsService` | (no folder) | These are runtime APIs with no persistent children. Skipped silently.   |
| `MarketplaceService`/`TeleportService`/`BadgeService`/`GroupService`/`NotificationService` | `_imported/<ServiceName>/` | No Eustress cognate. Children copied with `_imported/` prefix + warning so the user can triage. |
| **anything else**                     | `_imported/<ServiceName>/` | Unknown service. Children copied verbatim under `_imported/` + warning.  |

#### Eustress-only services (importer never touches)

- `SoulService/` — AI-native NPC brain. Roblox has no equivalent. The
  importer must not create files anywhere under this folder.
- `AdornmentService/` — editor-side gizmo / adornment instances. Strictly
  editor-internal.
- `_retired_layers/` — Eustress historical layers (per memory). Never
  touched.

The router enforces this with a hardcoded deny-list; any code path that
attempts to resolve into one of those three folder names returns an error.

#### StarterPlayer special case

In Roblox, `StarterPlayer` is a service whose two children
`StarterPlayerScripts` and `StarterCharacterScripts` are containers for
per-player and per-character LocalScripts respectively. Eustress flattens
this — the disk layout already has `StarterPlayerScripts/` and
`StarterCharacterScripts/` as sibling top-level service folders.

The router splits at the `StarterPlayer` boundary:

- A LocalScript at `StarterPlayer/StarterPlayerScripts/MyScript` lands at
  `StarterPlayerScripts/MyScript/_instance.toml`.
- A LocalScript at `StarterPlayer/StarterCharacterScripts/Reset` lands at
  `StarterCharacterScripts/Reset/_instance.toml`.
- Properties on the `StarterPlayer` service itself (e.g.
  `CharacterWalkSpeed`, `CharacterJumpHeight`, `LoadCharacterAppearance`)
  are merged into a synthesised `StarterPlayerScripts/_service.toml`
  block under a `[starter_player]` sub-table. The Wave-2 engine reads this
  block to honour the source defaults.

### ServiceRouter API

```rust
pub struct ServiceRouter {
    space_root: PathBuf,
    deny: HashSet<&'static str>, // SoulService, AdornmentService, _retired_layers
}

impl ServiceRouter {
    pub fn new(space_root: PathBuf) -> Self { /* … */ }

    /// Returns the absolute on-disk destination folder for a child of the
    /// named Roblox service. Returns `None` for runtime-only services
    /// (Debris, RunService, …) — the walker skips their subtrees.
    /// Returns Err for the deny-listed Eustress-only folders, which the
    /// walker treats as a hard fault (it should never get there).
    pub fn route_for(&self, roblox_service: &str)
        -> Result<Option<PathBuf>, RouterError>;

    /// True when `path` is inside SoulService/AdornmentService/_retired_layers.
    pub fn is_off_limits(&self, path: &Path) -> bool;
}
```

The router is **the only** place that opens a `<service>/_service.toml`
for read-modify-write. If the service folder doesn't exist, the router
creates it with a minimal `_service.toml` that records the class name and
nothing else; downstream property writes layer on top.

---

## 6. Terrain — full voxel import

Roblox stores terrain as a single `Terrain` instance, child of `Workspace`,
with three binary properties of interest:

| Roblox property | Variant kind     | Eustress destination                                          |
| --------------- | ---------------- | ------------------------------------------------------------- |
| `MaterialColors`| `MaterialColors` | Per-material `[material_colors]` table in `Workspace/Terrain/_instance.toml`. |
| `SmoothGrid`    | `BinaryString`   | Decoded → Eustress voxel chunks under `Workspace/Terrain/voxel_chunks/`. |
| `PhysicsGrid`   | `BinaryString`   | Legacy / collision-only. We **read it for parity check** then drop it — Eustress derives physics from the visual mesh (1:1 trimesh colliders per the `physics` feature in `eustress-common::terrain::chunk`). |

### 6.1 Cell size + coordinate system

- Roblox terrain cells are **4 studs** per side.
- Eustress is meter-native and `STUD_TO_METERS = 1.0` (per
  `services::workspace`).
- Therefore: 1 Roblox terrain voxel = **4 m × 4 m × 4 m** in Eustress.
- Roblox terrain is volumetric (3D voxel grid), not heightmap. Our import
  preserves that.

### 6.2 SmoothGrid encoding (what the BinaryString contains)

The `SmoothGrid` payload is documented in `rbx-binary`'s source and in the
rojo-rbx ecosystem repos. The on-disk layout:

```
SmoothGrid := Header || ChunkRecord*
Header     := u8 version (always 1)
ChunkRecord := i32 cx, i32 cy, i32 cz  // chunk grid coords (1 chunk = 32×32×32 cells)
            || u8[] zlib-compressed-payload
Payload    := per-cell records (32^3 cells = 32768 cells)
PerCell    := u8 material_id     // 0..N from the Material enum, 0 = Air
            || u8 occupancy_q    // 0..255, quantised occupancy fraction
```

The 32^3 = 32,768 cells per chunk are stored in **YXZ order** (Y outer, X
middle, Z inner) per Roblox's convention. We respect that order on read so
spatial layout is preserved.

The chunk grid coordinate `(cx, cy, cz)` means the chunk's origin in
world-voxel-space is `(cx*32, cy*32, cz*32)`, i.e. multiply by 32 to get
voxel indices, by `32*4 = 128` studs to get the chunk's world origin in
studs (and equivalently in meters for Eustress).

**Implementation reference** for the decoder: rojo-rbx's `rbx-binary` parses
SmoothGrid in `src/serializer/state.rs::serialize_terrain` (writer side) and
`src/deserializer/state.rs::deserialize_terrain` (reader side). The
`rbx_dom_weak` `Variant::MaterialColors` decoder + the public `SmoothGrid`
binary blob give us both halves. Our decoder uses `flate2` for zlib
(already in workspace via Bevy's asset loaders) and parses the per-cell
records directly.

### 6.3 Material enum mapping (Roblox → Eustress)

Roblox's smooth-terrain material list (≈26 entries) maps to Eustress's
8-entry `TerrainMaterial` enum from
`eustress/crates/common/src/terrain/material.rs:15`. Mapping is by visual
intent — when in doubt, pick the closest Eustress material:

| Roblox material   | Eustress `TerrainMaterial` | Note                                  |
| ----------------- | -------------------------- | ------------------------------------- |
| `Air`             | (occupancy = 0)            | Empty cell, no material slot needed.  |
| `Grass`           | `Grass`                    | Direct.                               |
| `Sand`            | `Sand`                     | Direct.                               |
| `Rock`            | `Rock`                     | Direct.                               |
| `Water`           | (special — water layer)    | Routed to Eustress water; see §6.5.   |
| `Ice`             | `Snow`                     | Closest available; logged as `MaterialApproximation`. |
| `Mud`             | `Mud`                      | Direct.                               |
| `Concrete`        | `Concrete`                 | Direct.                               |
| `Sandstone`       | `Rock`                     | Closest; logged.                      |
| `Limestone`       | `Rock`                     | Closest; logged.                      |
| `Cobblestone`     | `Rock`                     | Closest; logged.                      |
| `Asphalt`         | `Asphalt`                  | Direct.                               |
| `Snow`            | `Snow`                     | Direct.                               |
| `Slate`           | `Rock`                     | Closest; logged.                      |
| `Marble`          | `Rock`                     | Closest; logged.                      |
| `Granite`         | `Rock`                     | Closest; logged.                      |
| `Glacier`         | `Snow`                     | Closest; logged.                      |
| `LeafyGrass`      | `Grass`                    | Closest.                              |
| `Pavement`        | `Concrete`                 | Closest.                              |
| `Brick`           | `Concrete`                 | Closest; logged.                      |
| `Wood`            | `Dirt`                     | Closest natural-tone material; logged. |
| `WoodPlanks`      | `Dirt`                     | Same as Wood.                         |
| `Salt`            | `Snow`                     | Closest white granular.               |
| `Basalt`          | `Rock`                     | Closest.                              |
| `Ground`          | `Dirt`                     | Direct.                               |
| `CrackedLava`     | `Rock`                     | Closest; the color override (from `MaterialColors`) usually carries the lava tint. |

All approximations are logged once per source material into
`ImportReport.terrain_material_approximations` so the user knows what
collapsed. The `MaterialColors` table (next subsection) carries the source
color, which **overrides** the Eustress material's default color on a
per-chunk basis — so even though `Marble → Rock`, the marble's white tint
survives in the rendered output.

### 6.4 MaterialColors handling

`MaterialColors` is a 23-entry table (one Color3 per non-Air material).
We preserve it verbatim under `Workspace/Terrain/_instance.toml`:

```toml
[material_colors]
Grass     = [0.35, 0.55, 0.25]
Sand      = [0.76, 0.70, 0.50]
Rock      = [0.50, 0.45, 0.40]
# … etc., 23 entries
```

The Eustress terrain renderer reads this block on chunk-mesh generation
and applies the per-source-material color as a vertex tint on top of the
Eustress material's base color. Implementation hook:
`eustress-common::terrain::material::TerrainMaterialConfig` already has a
`textures: [Option<Handle<Image>>; 8]` slot — we add a parallel `tints:
[Option<Color>; 8]` slot (sourced from `MaterialColors`) that the shader
multiplies in. This is a small additive change to `material.rs` and the
splat shader.

### 6.5 Water layer

Roblox treats `Water` as one of the terrain materials, painted into the
voxel grid. Eustress's terrain crate already has a separate water module
at `eustress/crates/common/src/terrain/water.rs`. We extract every voxel
whose Roblox material is `Water` into a Eustress water-surface description:

- For each contiguous water region (flood-fill on the voxel grid), record
  the bounding XZ rectangle, the top-Y level, and the average opacity
  from the source `Terrain.WaterTransparency` property.
- Emit one `WaterRoot` entity per region, with the rectangle + level +
  source `WaterColor` / `WaterWaveSize`. Stored under
  `Workspace/Terrain/water/<region-N>/_instance.toml`.

### 6.6 Eustress on-disk shape

```
<space_root>/Workspace/Terrain/
├── _instance.toml              # Terrain instance; material_colors + global props
└── voxel_chunks/
    ├── chunk_-2_0_-1.bin       # Per-chunk binary blob, name = "chunk_<cx>_<cy>_<cz>.bin"
    ├── chunk_-2_0_0.bin
    ├── chunk_-1_0_-1.bin
    └── … one per non-empty chunk
└── water/
    ├── region-0/_instance.toml
    └── region-1/_instance.toml
```

Each `chunk_<cx>_<cy>_<cz>.bin` is a tiny LZ4-compressed binary record:

```
u8  version (1)
u8  material_count (typically <= 8)
u8  flags
[ for each of 32768 cells: ]
    u8 material_id   // 0..material_count; 0 = Air
    u8 occupancy_q   // 0..255
```

LZ4 chosen over zlib for read-time speed (terrain meshing is the hot path
on space load). Workspace already pulls `lz4_flex` via the worlddb crate.

### 6.7 Streaming + load behaviour

The Eustress engine's `chunk_spawn_system` (see
`eustress/crates/common/src/terrain/chunk.rs:90`) is the runtime consumer.
It already scans for chunks within view distance of the camera and
generates meshes. To make it consume imported chunks rather than its
existing procedural-noise generator, we:

1. Add a `TerrainSource` resource that points to either `Procedural` (the
   existing path) or `Imported { voxel_chunks_dir: PathBuf }`.
2. The chunk-spawn system reads the source on each tick; on `Imported`,
   it loads `chunk_<cx>_<cy>_<cz>.bin` from disk on the worker pool and
   builds the mesh from the cell records.
3. The TOML field `[terrain] source = "imported"` on
   `Workspace/Terrain/_instance.toml` flips the resource. The importer
   sets this field as the last step of `terrain::import_terrain`.

This means the importer doesn't have to spawn Bevy entities directly — it
writes files, the watcher + chunk-spawn system pick them up, and meshing
runs lazily as the camera approaches. **No frame-time penalty for
unvisited chunks.**

### 6.8 Failure modes

| Failure                                       | Behaviour                                                                                                     |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `SmoothGrid` zlib decompression fails         | Log to `ImportReport.terrain_decode_errors`, skip that chunk; continue with the rest.                         |
| Unknown material id (newer Roblox release)    | Map to `Rock` (closest safe default), log to `ImportReport.terrain_material_approximations`.                  |
| Cell count != 32768 per chunk                 | Hard error → abort import. This is a format violation; better to fail loud than write corrupt terrain.        |
| `MaterialColors` length != expected entries   | Pad with default colors, log warning.                                                                         |
| Empty terrain (no `Terrain` instance present) | Skip entirely. The import succeeds without touching `Workspace/Terrain/`.                                     |

---

## 7. CSG (UnionOperation / NegateOperation / IntersectOperation)

Roblox CSG operations come in two storable shapes inside a `.rbxl`:

1. **Baked mesh** — every shipped CSG instance carries a triangulated
   render mesh + a collision mesh inside `BinaryString` properties on
   the instance:
   - `MeshData` — render mesh (vertices, normals, UVs, triangles).
   - `PhysicalConfigData` — collision/physics mesh (decimated convex
     pieces).
   - `ChildData` — the original operand tree (so Studio can re-execute
     the CSG when the user edits the source primitives).
2. **Pure operand tree** — older or partial saves may only store
   `ChildData` and rely on Studio's CSG engine to bake on open. We
   re-execute these via `truck-shapeops` (§7.2).

### 7.1 Primary path — baked-mesh extraction (the 99% case)

For each `UnionOperation` / `NegateOperation` / `IntersectOperation`
instance with `MeshData` present:

1. Read the `MeshData` binary blob via `rbx_dom_weak`. Roblox's CSG mesh
   format is a custom layout documented in the `rbx-mesh` crate
   (rojo-rbx ecosystem). It's a packed sequence of:
   - u32 version
   - u32 vertex_count
   - per-vertex: position (Vec3 f32), normal (Vec3 f32), uv (Vec2 f32),
     color (Color3uint8), tangent (Vec3 f32 — derived if absent)
   - u32 triangle_count
   - per-triangle: 3 × u32 vertex indices
2. Convert to a Eustress mesh by writing to `.glb` (gltf binary, 1
   primitive, indexed triangles) with our standard vertex layout:
   POSITION, NORMAL, TEXCOORD_0, COLOR_0, TANGENT.
3. Place the `.glb` at
   `<space_root>/<service>/<csg-instance-folder>/csg.glb`.
4. Create a `Part` instance at
   `<space_root>/<service>/<csg-instance-folder>/_instance.toml` with:
   - `class_name = "Part"`
   - `[asset] mesh = "csg.glb"`, `scene = "Scene0"`
   - `[transform]` from the source `CFrame`
   - `[properties]` from the source `BrickColor`/`Color`/`Material`/etc.
5. Stash the original operand tree for the optional recompute path:
   write `csg_source.bin` (the verbatim `ChildData` payload) alongside
   `csg.glb`. The `Recompute CSG` Studio command (§7.3) reads this.

The `Part` class is already used for arbitrary asset-meshed parts (see
`eustress-common::instance_create::InstanceOverrides::asset_mesh` —
`eustress/crates/common/src/instance_create.rs:52`). No new class is
needed. The instance still appears in Explorer with the original Roblox
name and parent.

`NegateOperation` (subtract) and `IntersectOperation` follow the exact
same path — the baked mesh already represents the operation's result.
The original operation type is recorded in `[properties.csg_op]` as
`"union"` / `"negate"` / `"intersect"` for the recompute path.

**This is the default and covers ≈99% of real Roblox places.**

### 7.2 Fallback path — CSG re-execution via `truck-shapeops`

When `MeshData` is absent but `ChildData` is present (rare — happens only
for `.rbxl`s saved with the operands kept and the bake stripped, or for
brand-new operations not yet baked), we re-execute the CSG tree in-process
using the existing `eustress-cad` crate:

1. Parse `ChildData` (a tree of primitive BaseParts: Block/Ball/Cylinder
   with CFrames + Sizes).
2. For each operand, build a `truck` solid via
   `eustress_cad::feature::Feature::Extrude` (Block), or a primitive
   sphere/cylinder helper.
3. Walk the operand tree, applying `boolean_or` / `boolean_not` /
   `boolean_and` from `eustress_cad::eval` (already wired —
   `eustress/crates/cad/src/eval.rs:680`).
4. Tessellate the resulting `Solid` with `truck-meshalgo` to triangles.
5. Write the triangles as `csg.glb` and create the `Part` instance as in
   §7.1.

`truck-shapeops` is **already a workspace dep** (`eustress/Cargo.toml:117`)
so this requires no new dependencies. The full chain (parse → BRep → mesh
→ glb) lives in this crate's `csg.rs` module and reuses `eustress-cad`'s
public functions.

### 7.3 Recompute on demand (Studio command)

For users who want to modify the operands of an imported CSG, we add a
Studio context-menu action: right-click a CSG-derived `Part` → **Recompute
CSG**. This:

1. Reads `csg_source.bin` next to `csg.glb`.
2. Re-executes the tree via §7.2.
3. Overwrites `csg.glb` in place.
4. File watcher re-loads the mesh; the part updates live.

This is a Wave-2 nice-to-have, not blocking. The CSG entity works without
it; recompute is for the rare edit case.

### 7.4 Failure modes

| Failure                                                | Behaviour                                                                                       |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| `MeshData` decode fails (malformed)                    | Fall through to §7.2 if `ChildData` present; else log + skip with a bounding-box `Part` fallback.|
| Both `MeshData` and `ChildData` absent                 | Log `CsgEmpty` warning; create AABB-sized Block as last resort.                                  |
| `truck-shapeops` boolean op returns `None` (tolerance) | Try once with a 10× tolerance multiplier per `eustress-cad`'s convention. If still failing, log + create AABB Block. |
| `.glb` write fails (disk full / permissions)           | Hard error → abort import; the partial Space is left for the user to inspect.                    |

---

## 8. Events + Functions (RemoteEvent / RemoteFunction / BindableEvent / BindableFunction)

All four classes are first-class in Eustress (confirmed in
`eustress/crates/common/src/classes.rs:259-262`):

```rust
RemoteEvent,        // One-way event channel (client↔server)
RemoteFunction,     // Request-response channel (client↔server)
BindableEvent,      // In-process one-way event (same context)
BindableFunction,   // In-process request-response (same context)
```

They are essentially **empty containers**: in Roblox, a `RemoteEvent` has no
properties beyond `Name` and `Parent`; signals are dispatched dynamically
from script code via `:FireServer()` / `:FireClient()` / `:Connect()`. There
is no static signature to preserve.

### 8.1 Import behaviour

For each event/function instance the walker visits:

1. Determine the destination folder via the service router (§5). Common
   placements:
   - `RemoteEvent` / `RemoteFunction` → typically `ReplicatedStorage/` (so
     both client and server can resolve the path) or `ServerScriptService/`
     (server-only).
   - `BindableEvent` / `BindableFunction` → wherever the parent script
     lives; same-context, so `ServerScriptService/` or
     `ReplicatedStorage/` is normal.
2. Create the entity via `instance_create::create_instance` with
   class_name set to the matching Eustress `ClassName`, name set from
   the Roblox `Name` property.
3. No additional properties to write. The `_instance.toml` is just
   metadata + class name.

### 8.2 Script-side resolution

When the importer materialises `Script` / `LocalScript` /
`ModuleScript` bodies (§9), the bodies still contain Luau code like:

```lua
local remote = ReplicatedStorage:WaitForChild("MyRemote")
remote:FireServer(data)
```

The Eustress Luau runtime (mlua-based, configured via the
`compat::ScriptTransformer` — `eustress/crates/common/src/luau/compat.rs`)
resolves `:WaitForChild` against the live entity tree at runtime.
Because we have materialised the `MyRemote` entity at
`ReplicatedStorage/MyRemote/_instance.toml` as a real `RemoteEvent` entity,
the lookup succeeds with no further import-time work.

### 8.3 OnServerInvoke / OnServerEvent callbacks

These are script-side connection points (`remote.OnServerInvoke =
function(player, payload) … end`). They live in `Script` bodies, not on
the `RemoteFunction` instance itself. Therefore: no import-time work
beyond the script body transfer.

### 8.4 Edge cases

| Edge case                                              | Behaviour                                                                                          |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------- |
| RemoteEvent placed under `Workspace/Part/`              | Allowed. The entity goes into `Workspace/Part/MyRemote/`. Scripts using `script.Parent.MyRemote` resolve correctly. |
| Duplicate names (RemoteEvent `Fire` and `Fire` siblings)| Allowed; `instance_create` runs `unique_entity_name`, so disk folders become `Fire/` and `Fire-1/`. The Roblox referent → uuid map (§12) preserves identity. The Luau runtime resolves via Name+Parent, so this is a script-author problem either way. |
| BindableEvent at game root (parented to DataModel itself)| Routed to `ReplicatedStorage/_root_bindables/<Name>/` with a `_root_origin` flag in the TOML so script lookups starting at `game.MyEvent` can find it. |

### 8.5 Failure modes

| Failure                                       | Behaviour                                                                                                     |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Destination service folder doesn't exist      | Router creates it. Never a failure source.                                                                    |
| Name collision after `unique_entity_name`     | Log to `ImportReport.name_collisions` with original + final names.                                            |

---

## 9. ClassName mapping (the long table)

Delegates to `eustress-common::luau::compat::ClassMapping::map_class` for
the string lookup, then `ClassName::from_str` for the typed value.

| Roblox class             | Eustress `ClassName`     | Service router default      | Notes                                                                  |
| ------------------------ | ------------------------ | --------------------------- | ---------------------------------------------------------------------- |
| `Part`                   | `Part`                   | inherited from ancestor     | Direct.                                                                |
| `MeshPart`               | `Part`                   | inherited                   | `MeshId` → `asset_mesh` override.                                      |
| `WedgePart`              | `Part` (shape=Wedge)     | inherited                   | `PartType::Wedge` sticker.                                             |
| `CornerWedgePart`        | `Part` (shape=CornerWedge)| inherited                  | `PartType::CornerWedge`.                                               |
| `TrussPart`              | `Part`                   | inherited                   | Approximated as Block + tagged with `_truss = true`; the Eustress mesh template ships a truss lattice variant. Logged as `Approximation::Truss` when the truss variant is unavailable. |
| `SpawnLocation`          | `SpawnLocation`          | inherited                   | Direct.                                                                |
| `Seat`                   | `Seat`                   | inherited                   | Direct.                                                                |
| `VehicleSeat`            | `VehicleSeat`            | inherited                   | Direct.                                                                |
| `Model`                  | `Model`                  | inherited                   | Direct (container).                                                    |
| `Folder`                 | `Folder`                 | inherited                   | Direct.                                                                |
| `PointLight`             | `PointLight`             | inherited                   | Direct.                                                                |
| `SpotLight`              | `SpotLight`              | inherited                   | Direct.                                                                |
| `SurfaceLight`           | `SurfaceLight`           | inherited                   | Direct.                                                                |
| `WeldConstraint`         | `WeldConstraint`         | inherited                   | Direct.                                                                |
| `Motor6D`                | `Motor6D`                | inherited                   | Direct.                                                                |
| `Attachment`             | `Attachment`             | inherited                   | Direct.                                                                |
| `HingeConstraint`        | `HingeConstraint`        | inherited                   | Direct.                                                                |
| `RopeConstraint`         | `RopeConstraint`         | inherited                   | Direct (Avian RopeConstraint).                                         |
| `SpringConstraint`       | `SpringConstraint`       | inherited                   | Direct (Avian SpringConstraint).                                       |
| `PrismaticConstraint`    | `PrismaticConstraint`    | inherited                   | Direct (Avian PrismaticConstraint).                                    |
| `BallSocketConstraint`   | `BallSocketConstraint`   | inherited                   | Direct (Avian BallSocketConstraint).                                   |
| `ScreenGui`              | `ScreenGui`              | `StarterGui/`               | Direct.                                                                |
| `BillboardGui`           | `BillboardGui`           | inherited                   | Direct.                                                                |
| `SurfaceGui`             | `SurfaceGui`             | inherited                   | Direct.                                                                |
| `Frame`                  | `Frame`                  | inherited                   | Direct.                                                                |
| `TextLabel`              | `TextLabel`              | inherited                   | Direct.                                                                |
| `TextButton`             | `TextButton`             | inherited                   | Direct.                                                                |
| `TextBox`                | `TextBox`                | inherited                   | Direct.                                                                |
| `ImageLabel`             | `ImageLabel`             | inherited                   | Direct; `Image` → `asset_path`.                                        |
| `ImageButton`            | `ImageButton`             | inherited                   | Direct; `Image` → `asset_path`.                                        |
| `ScrollingFrame`         | `ScrollingFrame`         | inherited                   | Direct.                                                                |
| `ViewportFrame`          | `ViewportFrame`          | inherited                   | Direct.                                                                |
| `ParticleEmitter`        | `ParticleEmitter`        | inherited                   | Direct.                                                                |
| `Beam`                   | `Beam`                   | inherited                   | Direct.                                                                |
| `Sound`                  | `Sound`                  | inherited                   | `SoundId` → `asset_path`. Placeholder unless `AssetFetcher` set.       |
| `Script`                 | `LuauScript`             | inherited                   | Source preserved; `ScriptTransformer::transform` invoked.              |
| `LocalScript`            | `LuauLocalScript`        | inherited                   | Same.                                                                  |
| `ModuleScript`           | `LuauModuleScript`       | inherited                   | Same.                                                                  |
| `RemoteEvent`            | `RemoteEvent`            | inherited                   | §8.                                                                    |
| `RemoteFunction`         | `RemoteFunction`         | inherited                   | §8.                                                                    |
| `BindableEvent`          | `BindableEvent`          | inherited                   | §8.                                                                    |
| `BindableFunction`       | `BindableFunction`       | inherited                   | §8.                                                                    |
| `Sky`                    | `Sky`                    | `Lighting/`                 | Direct.                                                                |
| `Atmosphere`             | `Atmosphere`             | `Lighting/`                 | Direct.                                                                |
| `Clouds`                 | `Clouds`                 | `Lighting/`                 | Direct.                                                                |
| `Terrain`                | `Terrain`                | `Workspace/`                | **§6 — full voxel import.**                                            |
| `Humanoid`               | `Humanoid`               | inherited                   | Direct.                                                                |
| `Animator`               | `Animator`               | inherited                   | Direct.                                                                |
| `Camera`                 | `Camera`                 | inherited                   | Direct.                                                                |
| `SpecialMesh`            | `SpecialMesh`            | inherited                   | Direct; `MeshId` → `asset_mesh`.                                       |
| `Decal`                  | `Decal`                  | inherited                   | Direct; `Texture` → `asset_path`.                                      |
| `Texture` (BasePart texture child) | `Decal` + `_texture_tiled = true` | inherited | Roblox `Texture` is a tiling `Decal`; we preserve the tiling flag.    |
| `UnionOperation`         | `Part` (asset-meshed)    | inherited                   | **§7 — baked mesh extraction + opt-in recompute.**                     |
| `NegateOperation`        | `Part` (asset-meshed)    | inherited                   | **§7.**                                                                |
| `IntersectOperation`     | `Part` (asset-meshed)    | inherited                   | **§7.**                                                                |
| `Team`                   | `Team`                   | `Teams/`                    | Direct.                                                                |
| `MaterialVariant`        | (Eustress material entry)| `MaterialService/`          | Written as a TOML record under `MaterialService/<Name>/_instance.toml` matching the Eustress material schema. |
| `ProximityPrompt`        | (extras-only)            | inherited                   | Stored as extras for Wave-3 first-class support.                       |
| `Tool`                   | `Tool`                   | inherited or `StarterPack/` | Direct; tools under `StarterPack/` get a per-player tool template.     |
| `Accessory` / `Accoutrement` | `Accessory`          | inherited                   | Direct; mesh + attachment.                                              |
| `BodyVelocity` / `BodyAngularVelocity` / `BodyPosition` (deprecated mover) | extras-only | inherited | Logged as `LegacyMover` warning; equivalent constraint suggested.       |
| **anything else**        | _unmapped_               | `_imported/Unmapped/`       | Logged as `ImportReport::unmapped_classes`; subtree skipped.           |

**Service classes** (`Workspace`, `Lighting`, `Players`, etc.) are not
materialised as instances. They become routing decisions per §5.

---

## 10. Property mapping

| Roblox `Variant`            | Eustress equivalent                 | Conversion notes                                                                 |
| --------------------------- | ----------------------------------- | -------------------------------------------------------------------------------- |
| `Bool`                      | `PropertyValue::Bool`               | Direct.                                                                          |
| `String`                    | `PropertyValue::String`             | Direct.                                                                          |
| `Int32`                     | `PropertyValue::Int(i32)`           | Direct.                                                                          |
| `Int64`                     | `PropertyValue::Int(i32)`           | Truncated with `TruncatedInt` warning if out of i32 range.                       |
| `Float32`                   | `PropertyValue::Float(f32)`         | Direct.                                                                          |
| `Float64`                   | `PropertyValue::Float(f32)`         | Downcast to f32; loss accepted (Eustress is f32 everywhere).                     |
| `Vector2`                   | `PropertyValue::Vector2`            | Direct.                                                                          |
| `Vector2int16`              | `PropertyValue::Vector2`            | Cast i16 → f32.                                                                  |
| `Vector3`                   | `PropertyValue::Vector3 (Vec3)`     | Direct — `STUD_TO_METERS = 1.0` confirmed (`services::workspace`).               |
| `Vector3int16`              | `PropertyValue::Vector3`            | Cast i16 → f32.                                                                  |
| `CFrame`                    | `PropertyValue::Transform`          | Rotation matrix → `Quat::from_mat3`; position → translation; scale = 1.          |
| `OptionalCFrame`            | `PropertyValue::Transform` or absent| `None` → property omitted (template default used).                               |
| `Color3`                    | `PropertyValue::Color3`             | f32 [0..1].                                                                      |
| `Color3uint8`               | `PropertyValue::Color3`             | u8/255 → f32.                                                                    |
| `BrickColor`                | `PropertyValue::Color3`             | Static 128-entry palette lookup (rbx_dom_weak ships this).                       |
| `UDim`                      | `PropertyValue::UDim`               | Direct — Eustress has a first-class `UDim` struct in `crate::ui_types`.          |
| `UDim2`                     | `PropertyValue::UDim2`              | Direct via `crate::ui_types::UDim2`.                                             |
| `Rect`                      | `PropertyValue::Rect`               | Direct — `crate::ui_types::Rect`. Wave-2 promotion from extras.                  |
| `Enum`                      | `PropertyValue::Enum(String)`       | Label resolved via `rbx_reflection_database`; raw int kept as fallback.          |
| `EnumItem`                  | `PropertyValue::Enum(String)`       | Same.                                                                            |
| `ContentId` / `Content`     | `InstanceOverrides::asset_path`     | See §11.                                                                         |
| `String` (ProtectedString — script source) | inline TOML `source` field | Routed to script TOML.                                                |
| `BinaryString`              | base64 string in `extras`           | Default; for specific properties (Terrain SmoothGrid, CSG MeshData) we decode in the dedicated dispatchers (§6, §7) and the property never lands in `extras`. |
| `SharedString`              | base64 string in `extras`           | Same default; CSG dispatcher consumes specific cases.                            |
| `NumberSequence`            | `PropertyValue::NumberSequence`     | Round-tripped as `[[keypoints]]` array. Promoted from extras for Wave 2.         |
| `ColorSequence`             | `PropertyValue::ColorSequence`      | Same.                                                                            |
| `NumberRange`               | `PropertyValue::NumberRange`        | `[f32; 2]` array. Promoted.                                                      |
| `PhysicalProperties`        | inline fields in `[properties.physics]` | `density`, `friction`, `elasticity`, `friction_weight`, `elasticity_weight`. |
| `Ray`                       | `[f32; 6]` array in `extras`        | Rare; sufficient for round-trip.                                                 |
| `Region3` / `Region3int16`  | `[f32; 6]` array in `extras`        | Rare; sufficient for round-trip.                                                 |
| `Faces` / `Axes`            | bitset string in `extras`           | Same.                                                                            |
| `MaterialColors`            | `[material_colors]` table on Terrain instance | §6.4.                                                                  |
| `Font`                      | `PropertyValue::Font` (family + weight + style) | Promoted from extras when the GUI dispatcher consumes it.            |
| `Tags`                      | `[metadata.tags]` array              | Direct; Eustress already has a tags subsystem.                                   |
| `Attributes`                | `[properties.attributes]` table      | Direct; the recent (2026-05-25) Properties-panel work supports this.              |
| `Ref` (Instance reference)  | `Uuid` (via §12) in `extras`         | Resolved by referent → uuid lookup. Unresolved refs logged.                      |
| `UniqueId`                  | preserved in `[metadata.roblox_unique_id]` | For cross-import correlation.                                              |
| `SecurityCapabilities`      | dropped with `SecurityCapDiscarded` warning | Roblox-internal access control with no Eustress cognate.                    |
| `NetAssetRef`               | `InstanceOverrides::asset_path`     | Same as Content.                                                                 |

**`extras` block**: properties without a first-class slot get written into a
`[properties.extras]` sub-table. The file watcher already round-trips unknown
keys, so no engine work is needed for preservation. Promotion to first-class
slots happens as new property kinds become useful; each promotion is a
small focused PR against `PropertyValue`.

### 10.1 CFrame conversion (the rotation gotcha)

Roblox CFrames store rotation as a row-major 3×3 matrix. `glam` uses
column-major. The conversion:

```rust
fn roblox_cframe_to_transform(cf: &rbx_types::CFrame) -> Transform {
    // Roblox: rows are basis vectors (right, up, back).
    let right = Vec3::new(cf.orientation.x.x, cf.orientation.x.y, cf.orientation.x.z);
    let up    = Vec3::new(cf.orientation.y.x, cf.orientation.y.y, cf.orientation.y.z);
    let back  = Vec3::new(cf.orientation.z.x, cf.orientation.z.y, cf.orientation.z.z);
    // glam Mat3::from_cols expects column basis vectors.
    let mat = Mat3::from_cols(right, up, back);
    let rotation = Quat::from_mat3(&mat);
    let translation = Vec3::new(cf.position.x, cf.position.y, cf.position.z);
    Transform { translation, rotation, scale: Vec3::ONE }
}
```

Two unit tests cover identity CFrame and a 90°-about-Y to catch sign flips.

---

## 11. Asset reference resolution

Roblox URIs:
- `rbxassetid://NNNNNNNNN` — by numeric ID, on Roblox CDN.
- `rbxasset://path/to/file` — packaged with Studio install.
- `http(s)://...` — direct URL (deprecated but in the wild).

**Default behaviour**: emit placeholder local path
`assets/_unresolved/<scheme>/<id-or-path>` + `AssetWarning` per occurrence.
No network.

**`AssetFetcher` trait** (lets an integrator plug in a community mirror):

```rust
pub trait AssetFetcher: Send + Sync {
    fn fetch(&self, asset_id: u64) -> Result<Vec<u8>, FetchError>;
}
```

`ImportOptions.asset_fetcher: Option<Arc<dyn AssetFetcher>>`. Default `None`
keeps the no-network behaviour; an integrator can plug in a community
mirror, a local file cache, or a CDN proxy. When set, the resolver:

1. Calls `fetch(asset_id)`.
2. Writes the bytes to
   `<universe_root>/assets/<kind>/rbx-<asset_id>.<ext>` where `kind` is
   inferred from the property (Image → `images/`, Sound → `audio/`, etc.)
   and `<ext>` from the bytes' magic header.
3. Returns the relative path for `asset_path` / `asset_mesh`.

`rbxasset://` is handled by checking the user's local Roblox Studio install
(if present) at `%LOCALAPPDATA%/Roblox/Versions/`. If found, copy locally
and use; if not, emit warning.

`http(s)://` is treated like `rbxassetid://` from the AssetFetcher's
perspective — the integrator decides whether to follow it.

---

## 12. Idempotency + deterministic UUIDs

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

Same `(salt, referent)` → byte-exact same uuid → re-importing unchanged
places produces zero worlddb churn. Reference broader identity story:
`docs/AUDIT/08_IDENTITY_TRUST.md`.

### 12.1 Ref resolution

Roblox properties of type `Ref` (e.g. `WeldConstraint.Part0` /
`Motor6D.Part1`) carry referent strings. The importer:

1. First pass: walk every instance, build `HashMap<RobloxReferent, Uuid>`.
2. Second pass: materialise instances; for any `Ref` property, look up the
   target referent → uuid. Write the uuid into the `_instance.toml`'s
   `[references.<property_name>]` field.
3. Unresolved refs (target wasn't in the source DOM — Roblox occasionally
   serialises external refs) get logged to
   `ImportReport.unresolved_refs` and the field is omitted.

The Eustress entity-spawn machinery resolves uuid → entity at world-load
time. This is the existing `eustress-worlddb` reference convention.

---

## 13. Error reporting

### `ImportError` (hard failure — short-circuits)

```rust
#[derive(thiserror::Error, Debug)]
pub enum ImportError {
    #[error("could not read source file {0}: {1}")]
    Io(PathBuf, std::io::Error),

    #[error("unknown Roblox file format (magic bytes did not match .rbxl/.rbxlx/.rbxm/.rbxmx)")]
    UnknownFormat,

    #[error("rbx_binary parse failed: {0}")]
    BinaryParse(String),

    #[error("rbx_xml parse failed: {0}")]
    XmlParse(String),

    #[error("destination Space root {0} does not exist")]
    NoSpaceRoot(PathBuf),

    #[error("service router could not resolve service {0}: {1}")]
    ServiceRouter(String, String),

    #[error("instance_create failed for class {class}: {source}")]
    InstanceCreate { class: String, source: eustress_common::instance_create::CreateError },

    #[error("attempt to write into Eustress-only folder {0}")]
    OffLimits(PathBuf),

    #[error("terrain decode failure (hard violation): {0}")]
    TerrainFormat(String),
}
```

See `src/error.rs` for the live definition.

### `ImportReport` (soft, accumulated)

```rust
pub struct ImportReport {
    pub source_path: PathBuf,
    pub format: RobloxFormat,
    pub total_nodes_seen: usize,
    pub total_nodes_imported: usize,
    pub class_counts: Vec<ClassCount>,
    pub unmapped_classes: Vec<UnmappedClass>,
    pub unmapped_properties: Vec<UnmappedProperty>,
    pub asset_warnings: Vec<AssetWarning>,
    pub script_warnings: Vec<ScriptWarning>,
    pub approximations: Vec<Approximation>,

    // New in this spec:
    pub terrain_material_approximations: Vec<TerrainMaterialApproximation>,
    pub terrain_decode_errors: Vec<TerrainDecodeError>,
    pub terrain_chunks_imported: usize,
    pub csg_baked_extracted: usize,
    pub csg_recomputed: usize,
    pub csg_fallback_aabb: usize,
    pub events_imported: usize,
    pub unresolved_refs: Vec<UnresolvedRef>,
    pub name_collisions: Vec<NameCollision>,

    pub elapsed: Duration,
}
```

See `src/import_report.rs` for all live types: `ImportReport`, `ClassCount`,
`UnmappedClass`, `UnmappedProperty`, `AssetWarning`, `ScriptWarning`,
`Approximation`, plus the new fields above.

The engine's UI integration renders this as a modal post-import dialog.
The report is also archived to
`<space_root>/.eustress/import_reports/<ts>.json`.

---

## 14. Studio integration

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
│   [✓] Import terrain voxels                           │
│   [✓] Extract baked CSG meshes                        │
│   [ ] Download assets from rbxassetid:// (requires    │
│       network — uses configured AssetFetcher)         │
│                                                        │
│             [ Cancel ]    [ Import ▶ ]                │
└────────────────────────────────────────────────────────┘
```

After import, a second modal shows the `ImportReport` summary with sections
for: classes imported (counts), terrain chunks decoded, CSG meshes
extracted, events created, unmapped classes, asset warnings, and
approximations.

### Post-import quick actions

The report modal includes one-click actions for common follow-ups:

- **"Recompute CSG for N parts"** — re-execute every imported CSG body
  via §7.3 if the user wants the live operand tree.
- **"Download N missing assets"** — runs the AssetFetcher (if configured)
  for every unresolved `rbxassetid://` reference.
- **"Export report as JSON"** — saves `import_report.json` somewhere
  user-chosen.

---

## 15. `import_into_space` API

```rust
/// Options controlling an import call.
#[derive(Debug, Default)]
pub struct ImportOptions {
    /// Service routing rules. Default `ServiceRouter::new(space_root)`
    /// covers every standard Roblox service.
    pub service_router: ServiceRouter,

    /// Whether to decode SmoothGrid voxel data (§6). Default: true.
    pub import_terrain: bool,

    /// Whether to extract baked CSG MeshData (§7.1). Default: true.
    pub extract_csg_baked: bool,

    /// Whether to re-execute CSG from ChildData when MeshData is absent
    /// (§7.2). Default: true. (Disable for fastest-possible import.)
    pub recompute_csg_when_missing: bool,

    /// Whether to invoke `compat::ScriptTransformer` on Luau bodies.
    /// Default: true. Disable to keep the source verbatim.
    pub transform_scripts: bool,

    /// Optional asset fetcher (§11). Default `None` → no network.
    pub asset_fetcher: Option<Arc<dyn AssetFetcher>>,

    /// Number of worker threads for parallel materialisation.
    /// Default: `rayon::current_num_threads()`.
    pub parallelism: usize,
}

/// Single entry point. Walks the DOM, writes to disk via instance_create,
/// returns a populated report. Blocking; designed to run on a worker
/// thread (the Studio integration spawns it there).
pub fn import_into_space(
    dom: &RobloxDom,
    space_root: &Path,
    options: ImportOptions,
) -> Result<ImportReport, ImportError>;
```

### Parallelism strategy

Per-node materialisation (`instance_create` call + property writes) is
embarrassingly parallel **once the referent → uuid map is built**. The
first pass (build the map, walk the DOM, classify each node) is
sequential and cheap (≤ 100ms for 100k instances). The second pass
distributes the per-node writes across a rayon thread pool. The terrain
chunk decode and CSG mesh extraction are also rayon-parallel.

The only serialisation point is `_service.toml` writes (one per service),
which happens once per service at the start of the second pass.

---

## 16. Cross-platform constraints

- **Pure Rust, no native libs.** All deps verified pure-Rust.
- **No filesystem assumptions beyond `std::path`.**
- **No tokio/async.** Parser blocking. `import_into_space` runs in Bevy
  exclusive system or on a worker thread.
- **Builds for Linux / Mac / Windows.** WASM not a target (the file
  watcher and worldb don't run there either).
- **No global mutable state.** Each `import_into_space` call is
  independent; concurrent imports into different Spaces are safe.

---

## 17. Performance targets

Raised from the previous draft. Targets are wall-clock with
`parallelism = rayon::current_num_threads()` on an 8-core 2024-class
laptop.

| Scenario                                   | Target  | Stretch |
| ------------------------------------------ | ------- | ------- |
| 1 000-instance place                       | < 0.2 s | < 0.1 s |
| 10 000-instance place                      | < 2 s   | < 1 s   |
| 100 000-instance place (Adopt-Me-scale)    | < 10 s  | < 5 s   |
| 1 000 000-instance place (large UGC world) | < 60 s  | < 30 s  |
| Terrain: 1 km³ at 4-stud voxels (15.6M cells) | < 5 s | < 2 s |
| CSG: 1 000 union ops, baked-mesh extraction | < 3 s  | < 1 s   |

Bottlenecks expected:

- Per-node `instance_create` filesystem writes are the floor. Wave 2 will
  measure; if they dominate at 100k+ scale, we'll add a batched
  fast-path that writes to a tmp directory and atomically renames at the
  end, plus a direct-worldb-write option that skips the watcher round-trip.
- CFrame → Quat trig (inlined ~50 ns/instance — negligible).
- blake3 (~200 ns/instance — negligible).
- Terrain SmoothGrid zlib decompression dominates terrain import; it's
  per-chunk parallelisable.
- CSG `.glb` writes are the floor for CSG; ~500 KB/mesh average, parallel.

---

## 18. Test strategy

### Unit tests
- `class_map`: every entry in §9 exercised.
- `property_map`: each Variant arm round-trip tested. CFrame identity +
  90°-about-Y golden test.
- `identity`: golden-uuid test (same salt + same referent → same uuid).
- `service_router`: every Roblox service maps to the expected folder; the
  three deny-listed Eustress-only folders return errors.
- `terrain`: SmoothGrid decoder with handcrafted single-chunk fixture
  (one Grass voxel, one Rock voxel, one Water voxel, rest Air); verify
  chunk file shape + material count.
- `csg`: synthesised `MeshData` blob (cube, 24 verts / 12 tris) decoded
  + written → re-read → vertex/triangle counts match.
- `events`: each of the four event/function classes ends up at the
  correct folder with the correct class_name; no extra properties.

### Integration tests (fixtures)
- `tests/fixtures/baseplate.rbxlx` — Studio default (XML).
- `tests/fixtures/baseplate.rbxl` — same default, binary.
- `tests/fixtures/spawn-and-script.rbxlx` — SpawnLocation + Script.
- `tests/fixtures/rbxassetid-decal.rbxlx` — asset reference path.
- `tests/fixtures/terrain-small.rbxl` — 4×4×4 cells of mixed materials.
- `tests/fixtures/csg-cube-minus-sphere.rbxl` — single NegateOperation
  with baked MeshData.
- `tests/fixtures/csg-no-bake.rbxl` — UnionOperation with ChildData only
  (forces §7.2 fallback path).
- `tests/fixtures/events.rbxlx` — ReplicatedStorage with one of each
  event/function class.
- `tests/fixtures/services-everywhere.rbxlx` — one child of every
  service type (Workspace, Lighting, Players, …) to verify the router.

Each fixture's expected output is committed as a golden directory tree
under `tests/golden/<fixture>/`. The test runs the import into a tempdir
and asserts directory-tree + TOML equality.

### Benchmark
- `benches/import.rs` (criterion):
  - Synthetic 10k-Part place; wall-clock < 2s on CI hardware.
  - Synthetic 100k-Part place; wall-clock < 10s.
  - Terrain 1 km³ decode; < 5s.

### End-to-end test (manual / smoke)
- A handful of public-domain `.rbxl` files (Studio default templates +
  community-licensed places) live in
  `tests/fixtures/community/`. CI runs them through `import_into_space`
  against a tempdir; on PRs the failures are surfaced as artefacts.

---

## 19. Phase 2 (genuinely deferred — short list)

The previous draft listed five "future work flags" pessimistically. This
spec ships everything in Wave 2. The honest deferred list is short:

1. **Live script execution wiring** — the `compat::ScriptTransformer`
   transforms source; the actual `mlua` runtime that executes Luau scripts
   inside Eustress is being wired separately. The importer's contribution
   is complete: it produces TOML + Luau bodies in the right shape. When
   the runtime lands, imported scripts will just start running.
2. **Round-trip export** (Eustress → `.rbxl` via `rbx_xml` writer) — not
   in scope. If a user wants to take an Eustress Space back to Roblox
   that's a separate exporter crate.
3. **Asset mirror integration** — the `AssetFetcher` trait is in this
   spec, but shipping a default community-mirror implementation is
   deferred to a separate `eustress-roblox-assets` crate so the network
   dep is quarantined.
4. **Packages / Replicas** — Roblox's package system (shared assets that
   update in-place) maps to a future Eustress package system; until that
   lands, packages import as flat copies and the package marker is
   preserved in `[metadata.roblox_package]`.

Everything else — terrain, CSG, events, services, scripts, GUI, particles,
sounds — ships in Wave 2 per this spec.
