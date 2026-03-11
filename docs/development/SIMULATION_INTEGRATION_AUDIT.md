# Simulation System Integration Audit

**Date:** March 5, 2026  
**Status:** ✅ All Critical Gaps Fixed

---

## Executive Summary

This audit verifies the integration between the tick-based simulation system, Play/Pause/Stop controls, TOML hot-reload, Rune bindings, and the realism physics crate.

---

## ✅ VERIFIED: Play/Pause/Stop Button Integration

| Feature | Status | Location | Details |
|---------|--------|----------|---------|
| Play Solo (F7) | ✅ Working | `play_mode.rs:1381-1390` | Starts simulation, enables physics |
| Play with Character (F5) | ✅ Working | `play_mode.rs:1358-1368` | Spawns character, starts simulation |
| Pause/Resume (F6) | ✅ Working | `play_mode.rs:1371-1378` | Pauses physics AND simulation clock |
| Stop (F8/Escape) | ✅ Working | `play_mode.rs:1394-1412` | Restores world, resets simulation |
| UI Button handlers | ✅ Working | `play_mode.rs:1288-1341` | Slint → Bevy event flow |

### Integration Flow
```
Slint UI Button Click
    ↓
SlintAction::PlaySolo/Pause/Stop
    ↓
StudioState.play_solo_requested = true
    ↓
handle_play_mode_ui_buttons() system
    ↓
StartPlayEvent / TogglePauseEvent / StopPlayEvent
    ↓
PlayModeState transition (Editing → Playing → Paused → Editing)
    ↓
SimulationPlugin responds to state changes
```

---

## ✅ VERIFIED: Sandboxed State Restoration (Git-like Revert)

| Feature | Status | Location |
|---------|--------|----------|
| World snapshot on Play | ✅ Working | `play_mode.rs:644-717` |
| Binary scene serialization | ✅ Working | `play_mode.rs:723-742` |
| Restore on Stop | ✅ Working | `play_mode.rs:914-1046` |
| Deleted entity restoration | ✅ Working | `play_mode.rs:983-1045` |

### How It Works
1. **On Play Start:**
   - `WorldSnapshot` captures all entity transforms, components
   - Full scene serialized to `.eustress` binary in temp folder
   - Snapshot pushed to `SnapshotStack`

2. **On Stop:**
   - All entities spawned during play are despawned (`SpawnedDuringPlayMode` marker)
   - Original entities restored from snapshot
   - If entities were deleted during play, full scene reloaded from binary

---

## ✅ VERIFIED: Realism Crate Integration

| Component | Status | Location |
|-----------|--------|----------|
| RealismPlugin | ✅ Added | `main.rs:216-217` |
| SimulationPlugin | ✅ Added | `main.rs:218-219` |
| Physics (avian3d) | ✅ Working | `main.rs:213-214` |
| Physics pause in editor | ✅ Working | `play_mode.rs:853` |
| Physics resume on play | ✅ Working | `play_mode.rs:1495-1501` |

### Simulation ↔ PlayMode Integration
```rust
// SimulationPlugin now responds to PlayModeState:
.add_systems(OnEnter(PlayModeState::Playing), on_play_start)
.add_systems(OnEnter(PlayModeState::Paused), on_play_pause)
.add_systems(OnEnter(PlayModeState::Editing), on_play_stop)

// Tick advancement only runs during Playing state:
.add_systems(PreUpdate, advance_simulation_clock.run_if(in_state(PlayModeState::Playing)))
```

---

## ✅ VERIFIED: TOML Hot-Reload

| File Type | Status | Location |
|-----------|--------|----------|
| `.soul` scripts | ✅ Working | `file_watcher.rs:193-216` |
| `.glb` models | ✅ Working | `file_watcher.rs:219-234` |
| `.glb.toml` instances | ✅ **FIXED** | `file_watcher.rs:236-321` |
| `.part.toml` instances | ✅ **FIXED** | `file_watcher.rs:236-321` |
| Textures (png/jpg/tga) | ✅ Working | Bevy auto-reload |

### TOML Instance Hot-Reload (NEW)
When a `.glb.toml` or `.part.toml` file is modified:
1. File watcher detects change (300ms debounce)
2. TOML parsed into `InstanceDefinition`
3. ECS components updated:
   - `Transform` (position, rotation, scale)
   - `MaterialProperties` (density, conductivity, etc.)
   - `ThermodynamicState` (temperature, pressure, etc.)
   - `ElectrochemicalState` (voltage, SOC, etc.)

---

## ✅ VERIFIED: ECS → Rune Bindings

| Component | Status | Location |
|-----------|--------|----------|
| ECSBindings resource | ✅ Working | `rune_ecs_bindings.rs:19-36` |
| Entity snapshots | ✅ Working | `rune_ecs_bindings.rs:38-85` |
| Sync system | ✅ Working | `rune_ecs_bindings.rs:152-256` |
| Aggregated values | ✅ Working | `rune_ecs_bindings.rs:238-254` |

### Available in Rune Scripts
```rune
// Entity-level access
let voltage = ecs.get_voltage("VCell_Cathode");
let temp = ecs.get_temperature("VCell_Cathode");
let soc = ecs.get_soc("VCell_Cathode");

// Aggregated simulation values
let total_voltage = ecs.get_sim("battery.voltage");
let avg_soc = ecs.get_sim("battery.soc");
let power = ecs.get_sim("battery.power");
```

---

## ✅ VERIFIED: Tick Simulation System

| Component | Status | Location |
|-----------|--------|----------|
| SimulationClock | ✅ Working | `common/simulation/clock.rs` |
| SimulationState | ✅ Working | `common/simulation/state.rs` |
| WatchPointRegistry | ✅ Working | `common/simulation/watchpoint.rs` |
| BreakPointRegistry | ✅ Working | `common/simulation/breakpoint.rs` |
| SimulationRecording | ✅ Working | `common/simulation/recorder.rs` |
| SimulationConfig | ✅ Working | `common/simulation/config.rs` |

### Time Compression Presets
| Preset | Scale | Use Case |
|--------|-------|----------|
| `REALTIME` | 1x | Normal operation |
| `FAST_1HOUR_PER_SEC` | 3,600x | Thermal cycling |
| `FAST_1DAY_PER_SEC` | 86,400x | Calendar aging |
| `FAST_1YEAR_PER_SEC` | 31,536,000x | Lifetime analysis |
| `BATTERY_CYCLE_TEST` | 7,200,000x | 10,000 cycles in ~10s |

---

## ✅ VERIFIED: Breakpoint/Watchpoint/Recorder Rune API

### SimController API (Rune)
```rune
// Time control
sim.pause();
sim.resume();
sim.step();
sim.reset();

// Time scale
sim.set_time_scale(3600.0);
sim.realtime();
sim.fast_hour();
sim.fast_day();
sim.battery_test();

// Run until target
sim.run_until_time(3600.0);
sim.run_hours(24.0);
sim.run_years(2.0);

// Watchpoints
sim.add_watchpoint("voltage", "Cell Voltage", "V");
sim.record("voltage", 3.7);
let v = sim.get("voltage");
let min = sim.get_min("voltage");

// Breakpoints
sim.add_breakpoint("low_soc", "soc", "<", 20.0);
sim.enable_breakpoint("low_soc", false);

// Recording
sim.start_recording("test_run");
sim.stop_recording();
sim.export("recordings/test_run.json");
```

---

## ✅ VERIFIED: Binary Cache Performance

| Aspect | Status | Details |
|--------|--------|---------|
| HashMap O(1) lookup | ✅ Working | Entity → File, File → Entity |
| File watcher debounce | ✅ 300ms | Prevents rapid-fire events |
| Lazy loading | ✅ Working | Files loaded on demand |
| Incremental updates | ✅ Working | Only changed components updated |

### Scalability Notes
- Current implementation uses `HashMap` which is O(1) for lookup
- For millions of entities, consider:
  - Chunked storage (spatial partitioning)
  - Memory-mapped files for large datasets
  - Parallel iteration with Rayon
- ECS itself (Bevy) is designed for millions of entities

---

## Architecture Summary

```
┌─────────────────────────────────────────────────────────────────┐
│                     Slint UI (Ribbon Buttons)                    │
│  [▶ Play] [⏸ Pause] [⏹ Stop] [⏩ Fast] [🔴 Record]              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    PlayModeState (Bevy State)                    │
│              Editing ←→ Playing ←→ Paused                        │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│ SimulationPlugin│ │ RealismPlugin   │ │ PhysicsPlugins  │
│ (tick clock)    │ │ (materials,     │ │ (avian3d)       │
│                 │ │  thermo, etc.)  │ │                 │
└─────────────────┘ └─────────────────┘ └─────────────────┘
              │               │               │
              └───────────────┼───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ECS Components (Bevy World)                   │
│  Transform, MaterialProperties, ThermodynamicState,             │
│  ElectrochemicalState, RigidBody, Collider, etc.                │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│ ECSBindings     │ │ WatchPoints     │ │ File Watcher    │
│ (Rune access)   │ │ (time series)   │ │ (TOML hot-reload│
└─────────────────┘ └─────────────────┘ └─────────────────┘
              │               │               │
              └───────────────┼───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Rune Scripts (.rune)                          │
│  - Test automation (cycle_life_test.rune)                       │
│  - UI updates (battery_hud.rune)                                │
│  - Product-specific logic                                        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    TOML Configuration                            │
│  - simulation.toml (watchpoints, breakpoints, tests)            │
│  - *.glb.toml (instance properties, materials, states)          │
└─────────────────────────────────────────────────────────────────┘
```

---

## Files Modified in This Audit

| File | Change |
|------|--------|
| `engine/src/simulation/plugin.rs` | Integrated with PlayModeState |
| `engine/src/main.rs` | Added SimulationPlugin |
| `engine/src/space/file_watcher.rs` | Added TOML instance hot-reload |

---

## Conclusion

All critical integration points are now verified and working:

1. ✅ **Play/Pause/Stop** buttons control both physics AND simulation tick clock
2. ✅ **Sandboxed testing** with full world restoration on Stop
3. ✅ **Realism crate** integrated and running during play mode
4. ✅ **TOML hot-reload** for instance files (properties update in real-time)
5. ✅ **Tick simulation** with time compression (years → seconds)
6. ✅ **Breakpoints/Watchpoints** with Rune API
7. ✅ **ECS → Rune bindings** for real-time data access
8. ✅ **Binary cache** with O(1) lookup performance

The system is ready for Voltec V-Cell battery testing and generalizes to any product simulation.
