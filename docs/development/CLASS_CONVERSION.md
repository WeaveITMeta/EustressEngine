# Class Conversion System

This document defines the rules for converting Eustress instances from one class to another in Studio. The conversion tool in the toolbar uses this matrix to determine valid targets and which TOML sections survive the conversion.

---

## Table of Contents

1. [Overview](#overview)
2. [Conversion Categories](#conversion-categories)
3. [Full Conversion Matrix](#full-conversion-matrix)
4. [Section Survival Rules](#section-survival-rules)
5. [File Operations](#file-operations)
6. [Slint UI Integration](#slint-ui-integration)
7. [Implementation Checklist](#implementation-checklist)

---

## Overview

### The Problem

A user selects an instance in Studio and wants to convert it to a different class. For example:
- Convert a `Part` to a `Seat` (add sit functionality)
- Convert a `Folder` to a `Model` (add pivot/primary part)
- Convert a `Part` (primitive) to a `Part` (mesh) by adding an asset reference

### The Constraint

The file extension (`.part.toml`, `.seat.toml`, etc.) is **load-authoritative** — the engine reads the extension first to know which components to attach. Changing `class_name` inside the TOML without renaming the file creates a mismatch.

### The Solution

The conversion tool:
1. Reads the source TOML
2. Keeps compatible sections
3. Adds target-class sections with defaults
4. Writes a new file with the correct extension
5. Deletes the old file
6. File watcher triggers entity re-spawn

---

## Conversion Categories

Classes are grouped by what they fundamentally represent. Conversions within a category are generally safe; cross-category conversions are often impossible.

### Category A: Solid Geometry (BasePart descendants)

These all have `[transform]`, `[geometry]`, `[appearance]`, `[physics]`, `[rendering]`.

| Class | Extension | Unique Sections |
|-------|-----------|-----------------|
| Part | `.part.toml` | `[asset]` (optional mesh) |
| Seat | `.seat.toml` | `[seat]` |
| VehicleSeat | `.vehicleseat.toml` | `[vehicleseat]` |
| SpawnLocation | `.spawn.toml` | `[spawn]` |

**All Category A classes are mutually convertible.**

### Category B: Containers (Folder-based)

These are directories with `_instance.toml` marker files.

| Class | Marker File | Unique Sections |
|-------|-------------|-----------------|
| Model | `_instance.toml` | `[model]` (primary_part, world_pivot) |
| Folder | `_instance.toml` | (none) |

**Model ↔ Folder is a trivial conversion** — just change `class_name` and add/remove `[model]`.

### Category C: GUI Containers

These are directories with `_instance.toml` marker files, but for UI.

| Class | Marker File | Unique Sections |
|-------|-------------|-----------------|
| ScreenGui | `_instance.toml` | `[screengui]` |
| BillboardGui | `_instance.toml` | `[billboardgui]` |
| SurfaceGui | `_instance.toml` | `[surfacegui]` |
| Frame | `_instance.toml` | `[frame]` |
| ScrollingFrame | `_instance.toml` | `[scrollingframe]` |

**GUI containers can convert within Category C** but not to Category A/B.

### Category D: GUI Leaves

These are individual UI elements, not containers.

| Class | Extension | Unique Sections |
|-------|-----------|-----------------|
| TextLabel | `.textlabel.toml` | `[text]` |
| TextButton | `.textbutton.toml` | `[text]`, `[button]` |
| TextBox | `.textbox.toml` | `[text]`, `[textbox]` |
| ImageLabel | `.imagelabel.toml` | `[image]` |
| ImageButton | `.imagebutton.toml` | `[image]`, `[button]` |

**GUI leaves can convert within Category D** (e.g., TextLabel → TextButton).

### Category E: Lights

| Class | Extension | Unique Sections |
|-------|-----------|-----------------|
| PointLight | `.pointlight.toml` | `[light]` |
| SpotLight | `.spotlight.toml` | `[light]`, `[spot]` |
| SurfaceLight | `.surfacelight.toml` | `[light]`, `[surface]` |
| DirectionalLight | `.dirlight.toml` | `[light]` |

**Lights can convert within Category E.**

### Category F: Constraints/Attachments

| Class | Extension | Unique Sections |
|-------|-----------|-----------------|
| Attachment | `.attachment.toml` | `[attachment]` |
| WeldConstraint | `.weld.toml` | `[weld]` |
| Motor6D | `.motor6d.toml` | `[motor6d]` |

**Constraints can convert within Category F** but with caveats (Motor6D has more fields than Weld).

### Category X: Non-Convertible

These classes have fundamentally unique data structures:

| Class | Why Non-Convertible |
|-------|---------------------|
| Terrain | Voxel heightmap data, not mesh/primitive |
| Humanoid | Character controller state machine |
| Camera | Viewport binding, projection matrix |
| SoulScript | Source code, not instance data |
| Sound | Audio asset reference |
| ParticleEmitter | Particle system configuration |
| Beam | Two-attachment span |
| Decal | Surface projection |
| Sun, Moon, Atmosphere, Clouds, Sky | Environment singletons |
| SolarSystem, CelestialBody, RegionChunk | Orbital system |

---

## Full Conversion Matrix

### Legend

- ✓ = Direct conversion (same category)
- ○ = Conversion with data loss (cross-category, some sections dropped)
- ✗ = Not convertible

### Category A: Solid Geometry

| From ↓ / To → | Part | Seat | VehicleSeat | SpawnLocation |
|---------------|------|------|-------------|---------------|
| **Part** | — | ✓ | ✓ | ✓ |
| **Seat** | ✓ | — | ✓ | ✓ |
| **VehicleSeat** | ✓ | ✓ | — | ✓ |
| **SpawnLocation** | ✓ | ✓ | ✓ | — |

### Category B: Containers

| From ↓ / To → | Model | Folder |
|---------------|-------|--------|
| **Model** | — | ✓ |
| **Folder** | ✓ | — |

### Category C: GUI Containers

| From ↓ / To → | ScreenGui | BillboardGui | SurfaceGui | Frame | ScrollingFrame |
|---------------|-----------|--------------|------------|-------|----------------|
| **ScreenGui** | — | ○ | ○ | ✗ | ✗ |
| **BillboardGui** | ○ | — | ○ | ✗ | ✗ |
| **SurfaceGui** | ○ | ○ | — | ✗ | ✗ |
| **Frame** | ✗ | ✗ | ✗ | — | ✓ |
| **ScrollingFrame** | ✗ | ✗ | ✗ | ✓ | — |

Note: ScreenGui/BillboardGui/SurfaceGui are root-level GUI containers. Frame/ScrollingFrame are child containers. Cross-level conversion loses context.

### Category D: GUI Leaves

| From ↓ / To → | TextLabel | TextButton | TextBox | ImageLabel | ImageButton |
|---------------|-----------|------------|---------|------------|-------------|
| **TextLabel** | — | ✓ | ✓ | ○ | ○ |
| **TextButton** | ✓ | — | ✓ | ○ | ○ |
| **TextBox** | ✓ | ✓ | — | ○ | ○ |
| **ImageLabel** | ○ | ○ | ○ | — | ✓ |
| **ImageButton** | ○ | ○ | ○ | ✓ | — |

Note: Text ↔ Image conversions lose the text or image content respectively.

### Category E: Lights

| From ↓ / To → | PointLight | SpotLight | SurfaceLight | DirectionalLight |
|---------------|------------|-----------|--------------|------------------|
| **PointLight** | — | ✓ | ✓ | ✓ |
| **SpotLight** | ✓ | — | ✓ | ✓ |
| **SurfaceLight** | ✓ | ✓ | — | ✓ |
| **DirectionalLight** | ✓ | ✓ | ✓ | — |

### Category F: Constraints

| From ↓ / To → | Attachment | WeldConstraint | Motor6D |
|---------------|------------|----------------|---------|
| **Attachment** | — | ✗ | ✗ |
| **WeldConstraint** | ✗ | — | ✓ |
| **Motor6D** | ✗ | ✓ | — |

Note: Attachment is a position marker, not a constraint. Weld ↔ Motor6D works but Motor6D loses animation fields when going to Weld.

### Cross-Category Conversions

| From Category | To Category | Result |
|---------------|-------------|--------|
| A (Geometry) | B (Container) | ✗ |
| A (Geometry) | C (GUI) | ✗ |
| A (Geometry) | D (GUI Leaf) | ✗ |
| A (Geometry) | E (Light) | ✗ |
| B (Container) | A (Geometry) | ✗ |
| C (GUI) | D (GUI Leaf) | ✗ |
| Any | X (Non-Convertible) | ✗ |

---

## Section Survival Rules

When converting, sections are handled as follows:

### Always Preserved

| Section | Survives | Notes |
|---------|----------|-------|
| `[instance]` | ✓ | `class_name` updated to target |
| `[metadata]` | ✓ | `last_modified` updated |
| `[tags]` | ✓ | |
| `[attributes]` | ✓ | User-defined key-values |
| `[parameters]` | ✓ | Domain parameters |
| `[consent]` | ✓ | AI consent tracking |

### Category A (Geometry) Conversions

| Section | Part → Seat | Seat → Part | Part → VehicleSeat |
|---------|-------------|-------------|---------------------|
| `[transform]` | ✓ | ✓ | ✓ |
| `[geometry]` | ✓ | ✓ | ✓ |
| `[appearance]` | ✓ | ✓ | ✓ |
| `[physics]` | ✓ | ✓ | ✓ |
| `[rendering]` | ✓ | ✓ | ✓ |
| `[asset]` | ✓ | ✓ | ✓ |
| `[seat]` | Added (defaults) | Dropped | Dropped |
| `[vehicleseat]` | — | — | Added (defaults) |

### Category B (Container) Conversions

| Section | Model → Folder | Folder → Model |
|---------|----------------|----------------|
| `[instance]` | ✓ (`class_name` = "Folder") | ✓ (`class_name` = "Model") |
| `[model]` | Dropped | Added (defaults) |

### Category D (GUI Leaf) Conversions

| Section | TextLabel → TextButton | TextLabel → ImageLabel |
|---------|------------------------|------------------------|
| `[gui]` | ✓ | ✓ |
| `[text]` | ✓ | Dropped |
| `[button]` | Added (defaults) | — |
| `[image]` | — | Added (defaults) |

### Category E (Light) Conversions

| Section | PointLight → SpotLight | SpotLight → PointLight |
|---------|------------------------|------------------------|
| `[transform]` | ✓ | ✓ |
| `[light]` | ✓ | ✓ |
| `[spot]` | Added (defaults) | Dropped |

---

## File Operations

### Conversion Procedure

```
1. User selects instance in Studio
2. User activates Convert tool
3. Tool reads source file path and class
4. Tool queries conversion matrix for valid targets
5. UI displays valid target classes (grayed out = invalid)
6. User selects target class
7. Tool executes conversion:

   a. Parse source TOML
   b. Update [instance].class_name
   c. Update [metadata].last_modified
   d. Drop incompatible sections
   e. Add target-class sections with defaults
   f. Compute new file path:
      - Same directory
      - Same base name
      - New extension (or same _instance.toml for containers)
   g. Write new file
   h. Delete old file (if extension changed)
   i. File watcher triggers reload
```

### File Path Examples

| Source | Target Class | Result |
|--------|--------------|--------|
| `Workspace/Chair.part.toml` | Seat | `Workspace/Chair.seat.toml` |
| `Workspace/Chair.seat.toml` | Part | `Workspace/Chair.part.toml` |
| `Workspace/Props/_instance.toml` (Folder) | Model | `Workspace/Props/_instance.toml` (same file, `class_name` changed) |
| `StarterGui/HUD/Title.textlabel.toml` | TextButton | `StarterGui/HUD/Title.textbutton.toml` |

---

## Slint UI Integration

### Tool State

```slint
export struct ConversionTarget {
    class_name: string,
    extension: string,
    enabled: bool,        // false if conversion not valid
    data_loss: bool,      // true if ○ (cross-category with loss)
}

export struct ConversionToolState {
    active: bool,
    selected_instance_path: string,
    selected_class: string,
    valid_targets: [ConversionTarget],
}
```

### UI Layout

When the tool is active and an instance is selected:

```
┌─────────────────────────────────────┐
│ Convert: Chair (Part)               │
├─────────────────────────────────────┤
│ ● Part          (current)           │
│ ○ Seat          ✓                   │
│ ○ VehicleSeat   ✓                   │
│ ○ SpawnLocation ✓                   │
│ ○ Model         ✗ (incompatible)    │
│ ○ Terrain       ✗ (incompatible)    │
└─────────────────────────────────────┘
        [ Convert ]  [ Cancel ]
```

### Data Loss Warning

If the user selects a target with `data_loss = true`:

```
┌─────────────────────────────────────┐
│ ⚠ Warning: Data Loss                │
├─────────────────────────────────────┤
│ Converting TextLabel → ImageLabel   │
│ will drop the following sections:   │
│                                     │
│   • [text] (text content, font)     │
│                                     │
│ This cannot be undone.              │
└─────────────────────────────────────┘
      [ Convert Anyway ]  [ Cancel ]
```

### Querying Valid Targets

The Slint UI calls into Rust to get valid targets:

```rust
pub fn get_valid_conversion_targets(source_class: &str) -> Vec<ConversionTarget> {
    let category = get_category(source_class);
    
    CONVERSION_MATRIX
        .iter()
        .map(|(target_class, target_ext)| {
            let target_category = get_category(target_class);
            let (enabled, data_loss) = match (category, target_category) {
                (a, b) if a == b => (true, false),   // Same category
                (a, b) if compatible(a, b) => (true, true),  // Cross with loss
                _ => (false, false),                  // Incompatible
            };
            ConversionTarget {
                class_name: target_class.to_string(),
                extension: target_ext.to_string(),
                enabled,
                data_loss,
            }
        })
        .collect()
}
```

---

## Implementation Checklist

### Prerequisites (TOML Implementation)

- [ ] All class TOML schemas implemented in engine
- [ ] File watcher hot-reload working for all classes
- [ ] Write-back system working for all classes

### Conversion System

- [ ] Define `ConversionCategory` enum in Rust
- [ ] Implement `get_category(class_name)` function
- [ ] Implement `CONVERSION_MATRIX` static table
- [ ] Implement `get_valid_conversion_targets()` function
- [ ] Implement `convert_instance()` function:
  - [ ] Parse source TOML
  - [ ] Apply section survival rules
  - [ ] Add default sections for target class
  - [ ] Write new file
  - [ ] Delete old file (if needed)
- [ ] Add undo support (store old file content before delete)

### Slint UI

- [ ] Add `ConversionToolState` to Studio state
- [ ] Create conversion tool button in toolbar
- [ ] Create target selection popup
- [ ] Create data loss warning dialog
- [ ] Wire up to Rust conversion functions

### Testing

- [ ] Unit tests for all Category A conversions
- [ ] Unit tests for all Category B conversions
- [ ] Unit tests for all Category D conversions
- [ ] Unit tests for all Category E conversions
- [ ] Integration test: convert Part → Seat → VehicleSeat → Part round-trip
- [ ] Integration test: convert Model → Folder → Model round-trip
- [ ] Verify file watcher picks up conversions correctly

---

## Changelog

### v1.0 (2026-03-02)

- Initial specification
- Defined 6 conversion categories
- Full conversion matrix for all class types
- Section survival rules
- Slint UI integration notes
