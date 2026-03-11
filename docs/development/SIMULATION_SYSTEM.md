# Simulation System Architecture

## Overview

The Eustress Simulation System provides **tick-based time compression** for running accelerated physics simulations. This enables compressing years of simulated time into seconds of wall time — critical for battery cycling tests, thermal stress analysis, and product validation.

## Core Philosophy

| Principle | Implementation |
|-----------|----------------|
| **Core Technology (Rust)** | Generalized physics laws, ECS components, tick system |
| **Dynamic Configuration (TOML)** | Watchpoints, breakpoints, test parameters |
| **User Logic (Rune)** | Product-specific behavior, test scripts, UI updates |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Rune Scripts                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ Test Suite  │  │ UI Updates  │  │ Product-Specific Logic  │  │
│  └──────┬──────┘  └──────┬──────┘  └────────────┬────────────┘  │
└─────────┼────────────────┼──────────────────────┼───────────────┘
          │                │                      │
          ▼                ▼                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                    SimulationPlugin (Bevy)                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │ SimClock     │  │ SimState     │  │ WatchPoint/BreakPoint │  │
│  │ (time scale) │  │ (run/pause)  │  │ (observability)       │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
          │                │                      │
          ▼                ▼                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ECS Components (Bevy)                        │
│  ┌──────────────────┐  ┌──────────────────┐  ┌───────────────┐  │
│  │ ElectrochemState │  │ ThermodynamicSt  │  │ KineticState  │  │
│  │ (V-Cell)         │  │ (All products)   │  │ (V-Man)       │  │
│  └──────────────────┘  └──────────────────┘  └───────────────┘  │
└─────────────────────────────────────────────────────────────────┘
          │                │                      │
          ▼                ▼                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Physics Laws (Rust)                          │
│  ┌──────────────────┐  ┌──────────────────┐  ┌───────────────┐  │
│  │ electrochemistry │  │ thermodynamics   │  │ kinematics    │  │
│  │ (generic)        │  │ (generic)        │  │ (generic)     │  │
│  └──────────────────┘  └──────────────────┘  └───────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## File Structure

```
eustress/crates/common/src/simulation/
├── mod.rs           # Module exports
├── clock.rs         # SimulationClock with time compression
├── state.rs         # SimulationState (Running/Paused/Stepping)
├── watchpoint.rs    # WatchPoint for variable tracking
├── breakpoint.rs    # BreakPoint for conditional pause
├── recorder.rs      # SimulationRecording for data export
└── config.rs        # SimulationConfig (TOML parsing)

eustress/crates/engine/src/simulation/
├── mod.rs           # Module exports
├── plugin.rs        # SimulationPlugin (Bevy integration)
├── rune_bindings.rs # Rune script API
└── battery.rs       # Battery-specific simulation (existing)
```

## Time Compression Presets

| Preset | Scale | Use Case |
|--------|-------|----------|
| `REALTIME` | 1x | Normal operation |
| `FAST_1MIN_PER_SEC` | 60x | Quick preview |
| `FAST_1HOUR_PER_SEC` | 3,600x | Thermal cycling |
| `FAST_1DAY_PER_SEC` | 86,400x | Calendar aging |
| `FAST_1WEEK_PER_SEC` | 604,800x | Long-term tests |
| `FAST_1YEAR_PER_SEC` | 31,536,000x | Lifetime analysis |
| `BATTERY_CYCLE_TEST` | 7,200,000x | 10,000 cycles in ~10s |

## TOML Configuration

### simulation.toml

```toml
[simulation]
tick_rate_hz = 60.0
time_scale = 3600000.0  # 1 hour per second
max_ticks_per_frame = 10
auto_start = false

[simulation.recording]
enabled = true
output_dir = "recordings"
format = "both"  # json, csv, or both
auto_export = true

[[watchpoints]]
name = "voltage"
label = "Cell Voltage"
unit = "V"
interval = 1
color = "#4CAF50"

[[breakpoints]]
name = "low_soc"
variable = "soc"
comparison = "<"
threshold = 20.0
one_shot = false

[[tests]]
name = "cycle_life_test"
script = "scripts/cycle_life_test.rune"
time_scale = 7200000.0
max_time_s = 7200000.0

[parameters]
nominal_voltage = 3.7
capacity_ah = 100.0
target_cycles = 1000.0
```

## Rune API

### SimController

```rune
// Time control
sim.pause();
sim.resume();
sim.step();
sim.step_n(100);
sim.reset();

// Time scale
sim.set_time_scale(3600.0);
sim.realtime();
sim.fast_hour();
sim.fast_day();
sim.fast_year();
sim.battery_test();

// Run until target
sim.run_until_time(3600.0);  // 1 hour simulated
sim.run_until_tick(10000);
sim.run_hours(24.0);
sim.run_days(7.0);
sim.run_years(2.0);

// Watchpoints
sim.add_watchpoint("voltage", "Cell Voltage", "V");
sim.record("voltage", 3.7);
let v = sim.get("voltage");
let min = sim.get_min("voltage");
let max = sim.get_max("voltage");
let avg = sim.get_avg("voltage");

// Breakpoints
sim.add_breakpoint("low_soc", "soc", "<", 20.0);
sim.enable_breakpoint("low_soc", false);
sim.remove_breakpoint("low_soc");

// Recording
sim.start_recording("test_run");
sim.stop_recording();
sim.export("recordings/test_run.json");

// Status
let t = sim.time();           // simulation seconds
let h = sim.time_hours();     // simulation hours
let d = sim.time_days();      // simulation days
let y = sim.time_years();     // simulation years
let wall = sim.wall_time();   // wall clock seconds
let tick = sim.tick();        // tick count
let ratio = sim.compression_ratio();
let fmt = sim.format_time();  // "2.5y" or "3.2h"
```

## Bevy Systems

### SimulationPlugin

```rust
app.add_plugins(SimulationPlugin::default());

// Or with config
app.add_plugins(SimulationPlugin {
    config_path: Some("simulation.toml".into()),
});
```

### Events

| Event | Description |
|-------|-------------|
| `SimulationTickEvent` | Fired each simulation tick |
| `BreakpointHitEvent` | Fired when breakpoint triggers |
| `SimulationCompleteEvent` | Fired when simulation ends |

### Resources

| Resource | Description |
|----------|-------------|
| `SimulationClock` | Time tracking with compression |
| `SimulationState` | Execution mode (Running/Paused) |
| `WatchPointRegistry` | All active watchpoints |
| `BreakPointRegistry` | All active breakpoints |
| `ActiveRecording` | Current recording state |

## Product-Specific Implementation

### V-Cell (Battery)

| Layer | Responsibility |
|-------|----------------|
| **Rust** | Nernst equation, Butler-Volmer, thermal diffusion |
| **TOML** | Material properties, operating limits, test parameters |
| **Rune** | OCV curve (Na-S specific), degradation model, dendrite risk |

### V-Man (Robotics)

| Layer | Responsibility |
|-------|----------------|
| **Rust** | Forward/inverse kinematics, joint dynamics |
| **TOML** | Joint limits, motor specs, cycle parameters |
| **Rune** | Motion profiles, OEE calculation, wear tracking |

### V-Pump (Fluid)

| Layer | Responsibility |
|-------|----------------|
| **Rust** | Navier-Stokes (via Garbongus), cavitation physics |
| **TOML** | Pump curves, fluid properties, NPSH limits |
| **Rune** | Flow control, efficiency optimization, fault detection |

### V-Incinerator (Thermal)

| Layer | Responsibility |
|-------|----------------|
| **Rust** | Combustion stoichiometry, heat transfer |
| **TOML** | Fuel properties, emission limits, temperature setpoints |
| **Rune** | Burn profiles, ash handling, emission monitoring |

## Example: Battery Cycle Test

```rune
// Run 1000 charge/discharge cycles
sim.set_time_scale(7200000.0);  // Compress 2000 hours → ~1 second
sim.start_recording("cycle_test");

// Add watchpoints
sim.add_watchpoint("soc", "State of Charge", "%");
sim.add_watchpoint("capacity", "Capacity Retention", "%");

// Add end-of-life breakpoint
sim.add_breakpoint("eol", "capacity", "<", 80.0);

// Run simulation
sim.run_hours(2000.0);  // 1000 cycles at 2h/cycle

// Export results
sim.stop_recording();
sim.export("recordings/cycle_test.json");
```

## Output Formats

### JSON Recording

```json
{
  "metadata": {
    "name": "cycle_test",
    "simulation_duration_s": 7200000.0,
    "wall_duration_s": 1.2,
    "total_ticks": 72000,
    "compression_ratio": 6000000.0
  },
  "series": {
    "voltage": {
      "times": [0.0, 0.016, ...],
      "values": [3.7, 3.71, ...],
      "stats": { "min": 2.5, "max": 4.2, "mean": 3.6 }
    }
  },
  "events": [
    { "time_s": 3600.0, "event_type": "cycle_complete", "description": "Cycle 1" }
  ]
}
```

### CSV Export

```
recordings/
├── voltage.csv
├── current.csv
├── soc.csv
├── capacity.csv
└── events.csv
```

## Integration with Slint UI

The simulation system integrates with Slint UI via Rune scripts:

```rune
pub fn on_tick() {
    // Update UI from simulation state
    ui.set_text("VoltageLabel", "Voltage: " + sim.get("voltage").round(2) + " V");
    ui.set_property("SOCBar", "width", sim.get("soc") / 100.0);
    
    // Color-code based on state
    if sim.get("temperature") > 50.0 {
        ui.set_property("TempLabel", "color", [1.0, 0.0, 0.0, 1.0]);
    }
}
```

## Future Enhancements

1. **Parallel Simulation** — Run multiple scenarios concurrently
2. **Monte Carlo** — Statistical analysis with parameter variation
3. **Machine Learning** — Train models on simulation data
4. **Cloud Execution** — Offload heavy simulations to cloud workers
5. **Real-time Comparison** — Overlay simulation vs physical test data
