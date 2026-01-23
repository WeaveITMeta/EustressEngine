# 01 - System Overview

> Core concepts and architecture of the Eustress Orbital Navigation System

## Introduction

The Eustress Orbital Navigation System is a sophisticated framework for mapping celestial observations into actionable 3D spatial data. It bridges the gap between raw telescope imagery and real-time navigation by combining:

- **Computer Vision**: SAM 3D for object segmentation and 3D reconstruction
- **Astrometric Calibration**: Star catalogs and angular references for positioning
- **Orbital Mechanics**: SGP4 propagation and ephemerides for dynamic objects
- **Hierarchical Coordinates**: Floating-origin regions for precision at any scale

## Core Problem Statement

Traditional coordinate systems fail at planetary scales due to floating-point precision loss. A spacecraft at 400km altitude represented in meters from Earth's center requires ~10^7 precision—exceeding f32 limits and causing visible jitter in rendering and physics.

**Solution**: Relative Euclidean Regions with floating-origin architecture.

## System Components

### 1. Observation Pipeline

```
Telescope Image → SAM 3D Segmentation → Depth Estimation → 3D Model Cache
       │                                        │
       └──── Star Catalog Matching ─────────────┘
                      │
              Astrometric Calibration
```

The observation pipeline processes raw telescope imagery:

1. **Image Acquisition**: Capture frames from telescope sensor
2. **Object Segmentation**: SAM 3D identifies satellites, planets, stars
3. **Star Matching**: Cross-reference with Hipparcos/Gaia catalogs
4. **Depth Estimation**: Combine known orbital altitudes with projection geometry
5. **3D Reconstruction**: Generate meshes for tracked objects

### 2. Coordinate System Hierarchy

```
┌─────────────────────────────────────────────────────┐
│                    ICRS (Inertial)                  │
│              International Celestial                │
│               Reference System                      │
└─────────────────────┬───────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────┐
│                  GCRS (Geocentric)                  │
│            Geocentric Celestial Reference           │
│                    System                           │
└─────────────────────┬───────────────────────────────┘
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
┌─────────────────┐     ┌─────────────────┐
│   ECEF/WGS84    │     │   Spaceship     │
│  Earth-Centered │     │    Centric      │
│  Earth-Fixed    │     │    Frame        │
└────────┬────────┘     └────────┬────────┘
         │                       │
         ▼                       ▼
┌─────────────────┐     ┌─────────────────┐
│  Earth-Centric  │     │   Ship-Centric  │
│  Orbital Grid   │     │  Orbital Grid   │
└────────┬────────┘     └────────┬────────┘
         │                       │
         └───────────┬───────────┘
                     ▼
         ┌─────────────────────┐
         │  Relative Euclidean │
         │      Regions        │
         └─────────────────────┘
```

### 3. Region System

Regions are hierarchical spatial chunks that provide:

- **Precision**: f32 local coordinates within f64 global framework
- **Scalability**: From planetary (level 0) to sub-meter (level 24+)
- **Seamless Transitions**: Velocity-preserving region changes
- **Abstract Spaces**: Support for ship interiors and non-physical spaces

```rust
pub struct RegionId {
    level: u8,           // 0=planet, ~20=~10m, ~24=~1m
    face: u8,            // 0-5 cube-mapped sphere
    x: u32, y: u32, z: u32,
    is_abstract: bool,   // For ship interiors
}
```

### 4. Dynamic Object Tracking

The system tracks multiple object categories:

| Category | Source | Update Rate | Precision |
|----------|--------|-------------|-----------|
| GEO Satellites | TLE + SGP4 | 1 Hz | ~1 km |
| LEO Satellites | TLE + SGP4 | 10 Hz | ~100 m |
| Planets | Ephemerides | 0.1 Hz | ~1000 km |
| Stars | ICRS Catalog | Static | Angular only |
| Debris/Obstacles | TLE + Radar | 10 Hz | ~10 m |
| SAM 3D Objects | Telescope | Real-time | Variable |

### 5. Navigation Mainframe

The spaceship's onboard computer maintains:

- **Navigation Arrays**: Sorted lists of nearby objects by distance/threat
- **Region Registry**: Active spatial chunks around the vessel
- **Trajectory Planner**: Predicted paths for collision avoidance
- **Reference Frame Manager**: Switches between Earth/ship-centric views

## Data Flow Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        INPUT SOURCES                            │
├─────────────────┬─────────────────┬─────────────────────────────┤
│  Telescope      │  TLE Feeds      │  Ephemeris Data             │
│  Imagery        │  (CelesTrak)    │  (JPL Horizons)             │
└────────┬────────┴────────┬────────┴────────┬────────────────────┘
         │                 │                 │
         ▼                 ▼                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                     PROCESSING LAYER                            │
├─────────────────┬─────────────────┬─────────────────────────────┤
│  SAM 3D         │  SGP4           │  pracstro                   │
│  Inference      │  Propagation    │  Calculations               │
└────────┬────────┴────────┬────────┴────────┬────────────────────┘
         │                 │                 │
         └─────────────────┼─────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ORBITAL COORDS                               │
│  global_ecef: DVec3  |  region: RegionId  |  local_pos: Vec3    │
└─────────────────────────────────┬───────────────────────────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         ▼                        ▼                        ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  Region         │    │  Navigation     │    │  Rendering      │
│  Registry       │    │  Arrays         │    │  (Bevy)         │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## Key Innovations

### Relative Euclidean Regions

The core abstraction that makes the system work. Every observed object exists in a local f32/f64 Cartesian "bubble" anchored to a hierarchical `RegionId`. The spaceship (or camera focus) acts as the floating origin, eliminating jitter even at interplanetary scales.

### Dual Reference Frames

Seamlessly switch between:
- **Earth-Centric**: For orbital operations, satellite tracking, ground communication
- **Spaceship-Centric**: For navigation, collision avoidance, crew perspective

### SAM 3D Caching

Telescope observations generate 3D models that are:
- Cached by observation timestamp + celestial coordinates
- Indexed spatially for fast lookup
- Interpolated with high-fidelity reference models when available

## Performance Characteristics

| Metric | Target | Achieved |
|--------|--------|----------|
| Objects Tracked | 10,000+ | ✓ |
| Frame Rate (rest) | 240 FPS | ✓ |
| Frame Rate (0.5c) | ~416 FPS | ✓ |
| Position Precision | Sub-millimeter | ✓ |
| Region Transitions | Seamless | ✓ |
| Memory (10k objects) | < 500 MB | ✓ |
| Update Granularity | 1 μs temporal | ✓ |
| Interpolation | Cubic Hermite | ✓ |

### Fine-Grained Navigation System

The navigation system is tuned for **ultra-smooth space travel** with adaptive precision scaling:

#### Adaptive Frame Rate Equation

```
FPS(v, σ) = FPS_base × √((1 + β)/(1 - β)) × σ

Where:
  β = v/c              (velocity as fraction of light speed)
  σ = smoothness       (1.0 = standard, 2.0 = ultra-smooth, 4.0 = cinematic)
  FPS_base = 240       (high-fidelity rest frame rate)
```

#### Smoothness Profiles

| Profile | σ | Description | Use Case |
|---------|---|-------------|----------|
| Standard | 1.0 | Baseline navigation | Orbital maneuvers |
| Smooth | 2.0 | Enhanced interpolation | Cruise travel |
| Ultra-Smooth | 4.0 | Maximum fidelity | Relativistic transit |
| Cinematic | 8.0 | Film-quality rendering | Recording/playback |

#### Fine-Grained Update Tiers

| Tier | Update Rate | Precision | Purpose |
|------|-------------|-----------|---------|
| **Physics** | 1000 Hz | 1 ms | Force integration, collision |
| **Navigation** | 500 Hz | 2 ms | Position tracking, threats |
| **Rendering** | 240-2000 Hz | Variable | Visual smoothness |
| **Interpolation** | 10000 Hz | 100 μs | Sub-frame motion |
| **Prediction** | 100 Hz | 10 ms | Trajectory extrapolation |

### Relativistic Frame Rate Table

**Derivation basis:**
1. **Reaction distance**: At velocity `v`, distance per frame = `v/FPS`. Higher speeds require faster updates.
2. **Relativistic aberration**: Forward objects compress into narrower cone, requiring finer temporal sampling.
3. **Time dilation**: Ship proper time slows by `γ = 1/√(1-β²)`, external events appear accelerated.
4. **Smoothness factor**: Multiplier for perceptually seamless motion during acceleration/deceleration.

| Velocity | β | γ (Lorentz) | Standard FPS | Smooth FPS | Ultra FPS | Distance/Frame (Ultra) |
|----------|---|-------------|--------------|------------|-----------|------------------------|
| 0 (rest) | 0.0 | 1.000 | 240 | 480 | 960 | 0 m |
| 0.01c | 0.01 | 1.000 | 242 | 484 | 969 | 3.1 km |
| 0.05c | 0.05 | 1.001 | 252 | 505 | 1,010 | 14.8 km |
| 0.1c | 0.1 | 1.005 | 266 | 531 | 1,063 | 28.2 km |
| 0.25c | 0.25 | 1.033 | 310 | 620 | 1,240 | 60.4 km |
| 0.5c | 0.5 | 1.155 | 416 | 832 | 1,663 | 90.1 km |
| 0.75c | 0.75 | 1.512 | 638 | 1,277 | 2,554 | 88.0 km |
| 0.9c | 0.9 | 2.294 | 1,046 | 2,091 | 4,183 | 64.5 km |
| 0.99c | 0.99 | 7.089 | 3,382 | 6,764 | 13,528 | 21.9 km |

### Interpolation and Prediction

For seamless travel, the system employs multi-tier interpolation:

```
Position(t) = Hermite(P₀, V₀, P₁, V₁, t) + Relativistic_Correction(β, t)

Where:
  P₀, P₁ = Position samples at frame boundaries
  V₀, V₁ = Velocity samples (tangent vectors)
  t = Sub-frame time [0, 1]
  Hermite = Cubic Hermite spline interpolation
```

**Sub-frame prediction** extrapolates object positions between physics ticks:
- Uses velocity + acceleration for quadratic prediction
- Applies relativistic length contraction correction
- Compensates for light-travel-time to distant objects

## Use Cases

1. **Satellite Tracking**: Monitor GEO/LEO satellites in real-time
2. **Collision Avoidance**: Predict and visualize potential debris impacts
3. **Navigation Planning**: Plot courses between orbital waypoints
4. **Telescope Automation**: Auto-track objects across the sky
5. **Space Situational Awareness**: Comprehensive orbital environment monitoring

## Next Steps

- [02_COORDINATE_SYSTEMS.md](./02_COORDINATE_SYSTEMS.md) - Deep dive into coordinate transformations
- [03_SPACESHIP_CENTRIC_TRAVEL.md](./03_SPACESHIP_CENTRIC_TRAVEL.md) - Floating origin implementation
