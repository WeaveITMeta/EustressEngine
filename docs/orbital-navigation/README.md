# Eustress Orbital Navigation System

> A comprehensive framework for spaceship-centric travel, Earth-centric orbital grids, and real-time celestial navigation using telescope imagery and 3D reconstruction.

## Overview

The Eustress Orbital Navigation System provides a production-ready solution for mapping geostationary points observed through telescope lenses into relative 3D Euclidean space. It combines computer vision (SAM 3D), orbital mechanics, and hierarchical coordinate systems to enable jitter-free navigation at planetary scales.

## Documentation Index

| Document | Description |
|----------|-------------|
| [01_OVERVIEW.md](./01_OVERVIEW.md) | System architecture and core concepts |
| [02_COORDINATE_SYSTEMS.md](./02_COORDINATE_SYSTEMS.md) | WGS84, ECEF, and Relative Euclidean Regions |
| [03_SPACESHIP_CENTRIC_TRAVEL.md](./03_SPACESHIP_CENTRIC_TRAVEL.md) | Floating origin and ship-relative navigation |
| [04_EARTH_CENTRIC_ORBITAL_GRIDS.md](./04_EARTH_CENTRIC_ORBITAL_GRIDS.md) | Earth-relative orbital tracking and grids |
| [05_NAVIGATION_SYSTEM.md](./05_NAVIGATION_SYSTEM.md) | Navigation arrays and mainframe architecture |
| [06_SAM3D_INTEGRATION.md](./06_SAM3D_INTEGRATION.md) | Telescope imagery and 3D reconstruction |
| [07_DYNAMIC_OBJECTS.md](./07_DYNAMIC_OBJECTS.md) | Satellites, planets, stars, and debris tracking |
| [08_PHYSICS_FOUNDATIONS.md](./08_PHYSICS_FOUNDATIONS.md) | Orbital mechanics and projection geometry |
| [09_IMPLEMENTATION_GUIDE.md](./09_IMPLEMENTATION_GUIDE.md) | Rust/Bevy implementation details |
| [10_API_REFERENCE.md](./10_API_REFERENCE.md) | Component and system API documentation |
| [11_INTERPLANETARY_EXTENSION.md](./11_INTERPLANETARY_EXTENSION.md) | Mars missions and interplanetary travel |

## Key Features

- **Floating-Origin Architecture**: Zero jitter at planetary scales using `big_space` integer grids with up to 128-bit effective precision
- **Hierarchical Regions**: Seamless scale transitions from planetary (level 0) to sub-meter (level 24+)
- **Dual Reference Frames**: Switch between Earth-centric and spaceship-centric coordinate systems
- **Real-Time Tracking**: SGP4 propagation for satellites, ephemerides for planets/stars
- **SAM 3D Integration**: Segment and reconstruct 3D models from telescope imagery
- **Physics-Grounded**: Built on projective geometry, Kepler's laws, and WGS84 geodetic standards

## Quick Start

```rust
use eustress_orbital::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EustressOrbitalPlugin)
        .add_systems(Startup, setup_spaceship)
        .run();
}

fn setup_spaceship(mut commands: Commands) {
    // Spawn spaceship as floating origin
    commands.spawn((
        Camera3d::default(),
        OrbitalCoords::from_geodetic(28.5, -80.6, 400_000.0), // ISS-like orbit
        FloatingOrigin,
        SpaceshipMarker,
    ));
}
```

## Dependencies

```toml
[dependencies]
bevy = "0.15"
big_space = "0.7"
proj = "0.6"
sgp4 = "0.18"
pracstro = "0.3"
nalgebra = "0.33"
```

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                     EUSTRESS ORBITAL NAVIGATION                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐          │
│  │  Telescope   │───▶│   SAM 3D     │───▶│  3D Models   │          │
│  │   Imagery    │    │  Segmentation│    │   (Cache)    │          │
│  └──────────────┘    └──────────────┘    └──────────────┘          │
│                                                 │                    │
│  ┌──────────────┐    ┌──────────────┐          ▼                    │
│  │  TLE Data    │───▶│    SGP4      │    ┌──────────────┐          │
│  │  (Satellites)│    │  Propagation │───▶│   Orbital    │          │
│  └──────────────┘    └──────────────┘    │   Coords     │          │
│                                          └──────────────┘          │
│  ┌──────────────┐    ┌──────────────┐          │                    │
│  │ Ephemerides  │───▶│   pracstro   │──────────┤                    │
│  │(Planets/Stars│    │  Positions   │          │                    │
│  └──────────────┘    └──────────────┘          ▼                    │
│                                          ┌──────────────┐          │
│                                          │   Region     │          │
│                                          │  Registry    │          │
│                                          └──────────────┘          │
│                                                 │                    │
│         ┌───────────────────┬─────────────────┼─────────────┐      │
│         ▼                   ▼                 ▼             ▼      │
│  ┌──────────────┐    ┌──────────────┐  ┌──────────────┐           │
│  │ Earth-Centric│    │  Spaceship   │  │  Navigation  │           │
│  │    Grid      │    │   Centric    │  │   Arrays     │           │
│  └──────────────┘    └──────────────┘  └──────────────┘           │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

## License

Part of the Eustress Engine project. See repository root for license details.
