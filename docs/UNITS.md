# Dynamic Unit System

Engine-native unit: **meter**. `1 ECS unit = 1 meter`. Avian gravity, raycasts, gizmo math, mesh AABBs, BasePart.size, Transform.translation — all in meters.

## Surfaces

| Layer | What lives here | Conversion |
| --- | --- | --- |
| **Display** | Properties panel readouts, status-bar dropdown, gizmo overlays, Measure tool | `meters → DisplayUnit` (Stage 5) |
| **Engine-Native** | ECS components, Avian physics, asset loaders, gizmo math, raycasts | identity |
| **Authored** | `_instance.toml`, `.gui.toml`, MCP create_entity arg | `authored → meters` at load (Stage 3); `meters → authored` at write (Stage 4) |

## Single source of truth

`eustress_common::units` — six-way `Unit` enum, `MeasureUnit` Component, `DisplayUnit` Resource, conversion helpers.

| API | Use |
| --- | --- |
| `convert(v, from, to)` | f64 length conversion |
| `convert_f32` / `convert_vec3_f32` / `convert_vec3_f64` | Vec3 + width-specific variants |
| `authored_to_engine_vec3_f32(v, authored)` | At load boundary |
| `engine_to_authored_vec3_f32(v, authored)` | At save boundary |
| `format_length_in(meters, display_unit)` | For UI readouts (with epsilon-snap) |
| `Unit::from_symbol(s)` | Strict — canonical `"m"`/`"cm"`/`"mm"`/`"ft"`/`"in"`/`"studs"` only |
| `Unit::from_any(s)` | Lenient — accepts `"meters"`, `"feet"`, etc. |
| `epsilon_snap_to_grain(v, grain, eps)` | Display-side jitter killer |

## Constants

- `Unit::Foot = 0.3048 m` (1959 yard)
- `Unit::Inch = 0.0254 m` (foot / 12)
- `Unit::Stud = 9.815 / 196.8 m` ≈ 0.04987 m (historic Roblox-ratio, kept exact)

## On-disk shape

```toml
[metadata]
class_name = "Part"
unit = "ft"  # authoring unit (optional; missing = meters)

[transform]
position = [5.0, 0.0, 0.0]  # in "ft" → engine reads 1.524 m
scale    = [1.0, 1.0, 1.0]  # in "ft" → engine reads 0.3048 m cube
```

## Feature flag

`units_v1` (engine + common): gates the load-time authored→meters conversion. **ON by default** as of 2026-05-13 (in both `eustress-common`'s default features and `eustress-engine`'s `core` tier). Identity short-circuits when `authored == Meter`, so meter-authored files pay zero cost. Use `--no-default-features` to disable as an escape hatch when diagnosing a regression.

## Verification matrix

| # | Setup | Action | Expected |
| --- | --- | --- | --- |
| 1 | New Space, no `default_unit` | Insert > Part | TOML has no `metadata.unit`; ECS scale = `[1, 1, 1]` m |
| 2 | Set `default_unit = "ft"` in `_project/settings.toml` | Insert > Part | TOML has `metadata.unit = "ft"`; with `units_v1` ON, file `scale = [3.281, …]` but ECS `BasePart.size = [1, 1, 1]` m |
| 3 | File with `unit = "ft"`, `position = [5, 0, 0]` | Load | ECS `Transform.translation = [1.524, 0, 0]` m |
| 4 | Same as #3 | Move gizmo by +1 m on X | TOML `position = [8.281, 0, 0]` ft (5 + 3.281) |
| 5 | Same as #3 | Status-bar DisplayUnit → "ft" | Properties panel shows `Position: 5.000, 0.000, 0.000 ft` (Stage 5 partial; full Properties-panel display in Stage 6 follow-up) |
| 6 | Same as #3 | Properties panel: change Unit row from "ft" → "m" | TOML `unit = "m"`, `position = [1.524, 0, 0]` (values reinterpreted; physical size preserved) |
| 7 | MCP `create_entity({"size": [5,5,5], "unit": "ft"})` | Spawn | TOML `unit = "ft"`, `scale = [5,5,5]` ft; ECS `BasePart.size = [1.524, …]` m |
| 8 | Rune script | `Units.from_meters(1.524, "ft")` | Returns `(5.0, true)` |
| 9 | Luau script | `Units.to_meters(5.0, "feet")` | Returns `(1.524, true)` |
| 10 | BillboardGui `unit = "ft"`, `max_distance = 10.0` | Load | `BillboardGui.max_distance = 3.048` m |
| 11 | Same as #10 | Status-bar DisplayUnit → "studs" | Distance/position labels render in studs (Stage 10 follow-up) |

## Risk mitigations

- **Stage 3 feature flag**: `units_v1` (off by default) gates the load-time conversion; verifies meter-authored files stay byte-identical.
- **Centralised normaliser** (Stage 4): `engine_to_authored_*` + `authored_to_engine_*` helpers — grep for boundary callsites.
- **Epsilon snap** (Stage 0): `epsilon_snap_to_grain` collapses `4.99999` → `5.0` on display only; model layer keeps full precision.

## Known gaps

- Per-ModalTool DisplayUnit awareness needs Resource plumbing through `ModalTool` trait methods (Move/Scale/Rotate gizmo readouts still meter-only).
- Properties panel renders authored-unit values for length-typed fields when DisplayUnit differs from authored unit; display-unit projection is Stage 6.5 follow-up.
- Settings panel UI for changing the Space-default unit is not wired; users edit `_project/settings.toml` manually for now.
- `Move/Scale/Rotate/Measure` tool readouts use meters; full DisplayUnit-aware label rendering is Stage 10 follow-up.
