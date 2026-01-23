# 05 - Navigation System

> Navigation arrays, mainframe architecture, and real-time spatial awareness

## Overview

The Navigation System is the spaceship's central intelligence for spatial awareness. It maintains sorted arrays of nearby objects, predicts trajectories, manages collision avoidance, and provides the crew with actionable navigation data. This document details the mainframe architecture and its subsystems.

## Mainframe Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      NAVIGATION MAINFRAME                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │                    SENSOR FUSION LAYER                             │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │ │
│  │  │ Telescope│  │  Radar   │  │   TLE    │  │Ephemeris │          │ │
│  │  │  (SAM3D) │  │  Returns │  │  Catalog │  │  (Stars) │          │ │
│  │  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘          │ │
│  │       └─────────────┴─────────────┴─────────────┘                 │ │
│  │                           │                                        │ │
│  │                    ┌──────▼──────┐                                │ │
│  │                    │   Tracker   │                                │ │
│  │                    │   Fusion    │                                │ │
│  │                    └──────┬──────┘                                │ │
│  └───────────────────────────┼────────────────────────────────────────┘ │
│                              │                                          │
│  ┌───────────────────────────▼────────────────────────────────────────┐ │
│  │                    NAVIGATION ARRAYS                               │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │ │
│  │  │   Nearby     │  │   Threat     │  │   Target     │            │ │
│  │  │   Objects    │  │   Queue      │  │   List       │            │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘            │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                              │                                          │
│  ┌───────────────────────────▼────────────────────────────────────────┐ │
│  │                    PREDICTION ENGINE                               │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │ │
│  │  │  Trajectory  │  │  Collision   │  │   Maneuver   │            │ │
│  │  │  Propagator  │  │  Detector    │  │   Planner    │            │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘            │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                              │                                          │
│  ┌───────────────────────────▼────────────────────────────────────────┐ │
│  │                    OUTPUT SYSTEMS                                  │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │ │
│  │  │   Display    │  │   Alerts     │  │   Autopilot  │            │ │
│  │  │   Renderer   │  │   Manager    │  │   Interface  │            │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘            │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Navigation Arrays

### Core Data Structures

```rust
use bevy::prelude::*;
use std::collections::BinaryHeap;

/// Primary navigation state resource
#[derive(Resource)]
pub struct NavigationState {
    /// Current ship position (ECEF)
    pub origin_ecef: DVec3,
    /// Current ship velocity (ECEF)
    pub origin_velocity: DVec3,
    /// Current region
    pub origin_region: RegionId,
    /// Active reference frame
    pub reference_frame: ReferenceFrame,
    /// Last update timestamp
    pub last_update: f64,
}

/// Nearby objects sorted by distance
#[derive(Resource)]
pub struct NearbyObjectsArray {
    /// Objects within detection range, sorted by distance
    pub objects: Vec<NearbyObject>,
    /// Maximum tracking distance (meters)
    pub max_range: f64,
    /// Maximum objects to track
    pub max_objects: usize,
}

#[derive(Clone, Debug)]
pub struct NearbyObject {
    pub entity: Entity,
    pub distance: f64,
    pub relative_position: Vec3,
    pub relative_velocity: Vec3,
    pub object_type: ObjectType,
    pub threat_level: ThreatLevel,
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectType {
    Satellite,
    Debris,
    Planet,
    Moon,
    Star,
    Station,
    Vessel,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreatLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}
```

### Threat Queue

```rust
/// Priority queue for potential collision threats
#[derive(Resource)]
pub struct ThreatQueue {
    /// Threats sorted by time-to-closest-approach
    pub threats: BinaryHeap<CollisionThreat>,
    /// Minimum approach distance to consider a threat (meters)
    pub threat_threshold: f64,
    /// Time horizon for threat detection (seconds)
    pub time_horizon: f64,
}

#[derive(Clone, Debug)]
pub struct CollisionThreat {
    pub entity: Entity,
    pub time_to_closest_approach: f64,
    pub closest_approach_distance: f64,
    pub relative_velocity_at_closest: f64,
    pub threat_level: ThreatLevel,
    pub recommended_maneuver: Option<AvoidanceManeuver>,
}

impl Ord for CollisionThreat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering: smallest time = highest priority
        other.time_to_closest_approach
            .partial_cmp(&self.time_to_closest_approach)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl PartialOrd for CollisionThreat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for CollisionThreat {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for CollisionThreat {}

#[derive(Clone, Debug)]
pub struct AvoidanceManeuver {
    pub delta_v: Vec3,
    pub execute_time: f64,
    pub maneuver_type: ManeuverType,
    pub fuel_cost: f64,
}
```

### Target List

```rust
/// List of designated navigation targets
#[derive(Resource)]
pub struct TargetList {
    /// All designated targets
    pub targets: Vec<NavigationTarget>,
    /// Currently selected primary target
    pub primary: Option<usize>,
    /// Secondary targets for multi-point navigation
    pub secondary: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct NavigationTarget {
    pub entity: Option<Entity>,
    pub name: String,
    pub position_ecef: DVec3,
    pub velocity_ecef: DVec3,
    pub target_type: TargetType,
    pub waypoint_data: Option<WaypointData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetType {
    /// Tracked orbital object
    OrbitalObject,
    /// Fixed position in space
    FixedPoint,
    /// Waypoint in a route
    Waypoint,
    /// Rendezvous target
    Rendezvous,
    /// Docking port
    DockingPort,
}

#[derive(Clone, Debug)]
pub struct WaypointData {
    pub sequence_number: u32,
    pub arrival_time: Option<f64>,
    pub hold_time: Option<f64>,
    pub approach_velocity: Option<f64>,
}
```

## Update Systems

### Nearby Objects Update

```rust
fn update_nearby_objects(
    time: Res<Time>,
    nav_state: Res<NavigationState>,
    mut nearby: ResMut<NearbyObjectsArray>,
    objects: Query<(Entity, &OrbitalCoords, &OrbitalObject, Option<&Name>)>,
) {
    nearby.objects.clear();
    
    let ship_pos = nav_state.origin_ecef;
    let ship_vel = nav_state.origin_velocity;
    
    for (entity, coords, obj, name) in &objects {
        let distance = coords.global_ecef.distance(ship_pos);
        
        if distance > nearby.max_range {
            continue;
        }
        
        let relative_pos = (coords.global_ecef - ship_pos).as_vec3();
        let relative_vel = (coords.velocity_ecef() - ship_vel).as_vec3();
        
        // Calculate threat level based on approach
        let threat_level = calculate_threat_level(
            relative_pos,
            relative_vel,
            distance,
        );
        
        nearby.objects.push(NearbyObject {
            entity,
            distance,
            relative_position: relative_pos,
            relative_velocity: relative_vel,
            object_type: obj.object_type,
            threat_level,
            name: name.map(|n| n.to_string()),
        });
    }
    
    // Sort by distance
    nearby.objects.sort_by(|a, b| {
        a.distance.partial_cmp(&b.distance).unwrap()
    });
    
    // Truncate to max objects
    nearby.objects.truncate(nearby.max_objects);
}

fn calculate_threat_level(
    relative_pos: Vec3,
    relative_vel: Vec3,
    distance: f64,
) -> ThreatLevel {
    // Closing velocity (negative = approaching)
    let closing_rate = relative_vel.dot(relative_pos.normalize());
    
    if closing_rate >= 0.0 {
        // Moving away
        return ThreatLevel::None;
    }
    
    // Time to closest approach (simplified)
    let tca = -distance / closing_rate.abs() as f64;
    
    // Closest approach distance (simplified linear)
    let perpendicular_vel = relative_vel - relative_pos.normalize() * closing_rate;
    let miss_distance = (perpendicular_vel * tca as f32).length() as f64;
    
    match (tca, miss_distance) {
        (t, d) if t < 60.0 && d < 100.0 => ThreatLevel::Critical,
        (t, d) if t < 300.0 && d < 500.0 => ThreatLevel::High,
        (t, d) if t < 900.0 && d < 1000.0 => ThreatLevel::Medium,
        (t, d) if t < 3600.0 && d < 5000.0 => ThreatLevel::Low,
        _ => ThreatLevel::None,
    }
}
```

### Threat Detection System

```rust
fn update_threat_queue(
    nav_state: Res<NavigationState>,
    nearby: Res<NearbyObjectsArray>,
    mut threats: ResMut<ThreatQueue>,
) {
    threats.threats.clear();
    
    for obj in &nearby.objects {
        if obj.threat_level == ThreatLevel::None {
            continue;
        }
        
        // Detailed closest approach calculation
        let (tca, cpa, vel_at_cpa) = calculate_closest_approach(
            obj.relative_position,
            obj.relative_velocity,
        );
        
        if tca > threats.time_horizon || cpa > threats.threat_threshold {
            continue;
        }
        
        // Calculate avoidance maneuver
        let maneuver = if obj.threat_level >= ThreatLevel::Medium {
            Some(calculate_avoidance_maneuver(
                obj.relative_position,
                obj.relative_velocity,
                tca,
                cpa,
            ))
        } else {
            None
        };
        
        threats.threats.push(CollisionThreat {
            entity: obj.entity,
            time_to_closest_approach: tca,
            closest_approach_distance: cpa,
            relative_velocity_at_closest: vel_at_cpa,
            threat_level: obj.threat_level,
            recommended_maneuver: maneuver,
        });
    }
}

fn calculate_closest_approach(
    rel_pos: Vec3,
    rel_vel: Vec3,
) -> (f64, f64, f64) {
    // Time of closest approach: t = -dot(r, v) / dot(v, v)
    let v_dot_v = rel_vel.dot(rel_vel);
    
    if v_dot_v < 1e-10 {
        // Stationary relative to us
        return (f64::INFINITY, rel_pos.length() as f64, 0.0);
    }
    
    let tca = (-rel_pos.dot(rel_vel) / v_dot_v).max(0.0) as f64;
    
    // Position at closest approach
    let pos_at_cpa = rel_pos + rel_vel * tca as f32;
    let cpa = pos_at_cpa.length() as f64;
    
    // Relative velocity magnitude at CPA (unchanged for linear motion)
    let vel_at_cpa = rel_vel.length() as f64;
    
    (tca, cpa, vel_at_cpa)
}

fn calculate_avoidance_maneuver(
    rel_pos: Vec3,
    rel_vel: Vec3,
    tca: f64,
    cpa: f64,
) -> AvoidanceManeuver {
    // Simple perpendicular avoidance
    // Move perpendicular to both relative position and velocity
    
    let avoid_dir = rel_pos.cross(rel_vel).normalize_or_zero();
    
    // Delta-V needed to increase miss distance to safe threshold
    let safe_distance = 1000.0; // 1 km
    let needed_displacement = safe_distance - cpa;
    
    // v = d / t, but we want to be conservative
    let delta_v_magnitude = (needed_displacement / tca * 2.0).min(10.0) as f32;
    
    let delta_v = avoid_dir * delta_v_magnitude;
    
    // Execute at half the time to closest approach
    let execute_time = tca / 2.0;
    
    // Estimate fuel cost (simplified: 1 kg per m/s for 10000 kg ship)
    let fuel_cost = delta_v_magnitude as f64 * 10.0;
    
    AvoidanceManeuver {
        delta_v,
        execute_time,
        maneuver_type: ManeuverType::Custom,
        fuel_cost,
    }
}
```

## Trajectory Prediction

### Propagation Engine

```rust
#[derive(Resource)]
pub struct TrajectoryPropagator {
    /// Propagation time step (seconds)
    pub time_step: f64,
    /// Maximum propagation horizon (seconds)
    pub max_horizon: f64,
    /// Cached trajectories
    pub cache: std::collections::HashMap<Entity, PredictedTrajectory>,
}

#[derive(Clone, Debug)]
pub struct PredictedTrajectory {
    /// Entity this trajectory belongs to
    pub entity: Entity,
    /// Trajectory points (position, velocity, time)
    pub points: Vec<TrajectoryPoint>,
    /// Propagation method used
    pub method: PropagationMethod,
    /// Uncertainty cone (grows with time)
    pub uncertainty_km_per_hour: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct TrajectoryPoint {
    pub position_ecef: DVec3,
    pub velocity_ecef: DVec3,
    pub time: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropagationMethod {
    /// Two-body Keplerian
    Kepler,
    /// SGP4 for TLE-based objects
    Sgp4,
    /// Numerical integration with perturbations
    HighFidelity,
    /// Linear extrapolation (short-term only)
    Linear,
}

impl TrajectoryPropagator {
    pub fn propagate(
        &mut self,
        entity: Entity,
        initial_state: (DVec3, DVec3),
        method: PropagationMethod,
        duration: f64,
    ) -> PredictedTrajectory {
        let (pos, vel) = initial_state;
        let mut points = Vec::new();
        
        let steps = (duration / self.time_step).ceil() as usize;
        
        match method {
            PropagationMethod::Linear => {
                for i in 0..=steps {
                    let t = i as f64 * self.time_step;
                    points.push(TrajectoryPoint {
                        position_ecef: pos + vel * t,
                        velocity_ecef: vel,
                        time: t,
                    });
                }
            }
            PropagationMethod::Kepler => {
                let elements = state_to_elements(pos, vel);
                for i in 0..=steps {
                    let t = i as f64 * self.time_step;
                    let (p, v) = propagate_kepler(&elements, t);
                    points.push(TrajectoryPoint {
                        position_ecef: p,
                        velocity_ecef: v,
                        time: t,
                    });
                }
            }
            PropagationMethod::Sgp4 => {
                // Would use sgp4 crate here
                // Fallback to Kepler for now
                let elements = state_to_elements(pos, vel);
                for i in 0..=steps {
                    let t = i as f64 * self.time_step;
                    let (p, v) = propagate_kepler(&elements, t);
                    points.push(TrajectoryPoint {
                        position_ecef: p,
                        velocity_ecef: v,
                        time: t,
                    });
                }
            }
            PropagationMethod::HighFidelity => {
                // Numerical integration with J2, drag, solar pressure
                // Simplified to Kepler for this example
                let elements = state_to_elements(pos, vel);
                for i in 0..=steps {
                    let t = i as f64 * self.time_step;
                    let (p, v) = propagate_kepler(&elements, t);
                    points.push(TrajectoryPoint {
                        position_ecef: p,
                        velocity_ecef: v,
                        time: t,
                    });
                }
            }
        }
        
        let trajectory = PredictedTrajectory {
            entity,
            points,
            method,
            uncertainty_km_per_hour: match method {
                PropagationMethod::Linear => 10.0,
                PropagationMethod::Kepler => 1.0,
                PropagationMethod::Sgp4 => 0.1,
                PropagationMethod::HighFidelity => 0.01,
            },
        };
        
        self.cache.insert(entity, trajectory.clone());
        trajectory
    }
}
```

## Alert System

### Alert Manager

```rust
#[derive(Resource)]
pub struct AlertManager {
    pub active_alerts: Vec<NavigationAlert>,
    pub alert_history: Vec<NavigationAlert>,
    pub max_history: usize,
}

#[derive(Clone, Debug)]
pub struct NavigationAlert {
    pub id: u64,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub message: String,
    pub timestamp: f64,
    pub acknowledged: bool,
    pub related_entity: Option<Entity>,
    pub auto_dismiss_time: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertType {
    CollisionWarning,
    ProximityAlert,
    TrajectoryDeviation,
    FuelLow,
    SystemMalfunction,
    TargetAcquired,
    TargetLost,
    ManeuverRequired,
    ManeuverComplete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Caution,
    Warning,
    Critical,
}

impl AlertManager {
    pub fn raise_alert(&mut self, alert: NavigationAlert) {
        // Check for duplicate
        if self.active_alerts.iter().any(|a| {
            a.alert_type == alert.alert_type 
            && a.related_entity == alert.related_entity
            && !a.acknowledged
        }) {
            return;
        }
        
        self.active_alerts.push(alert);
        self.active_alerts.sort_by(|a, b| b.severity.cmp(&a.severity));
    }
    
    pub fn acknowledge(&mut self, alert_id: u64) {
        if let Some(alert) = self.active_alerts.iter_mut().find(|a| a.id == alert_id) {
            alert.acknowledged = true;
        }
    }
    
    pub fn dismiss(&mut self, alert_id: u64) {
        if let Some(idx) = self.active_alerts.iter().position(|a| a.id == alert_id) {
            let alert = self.active_alerts.remove(idx);
            self.alert_history.push(alert);
            
            if self.alert_history.len() > self.max_history {
                self.alert_history.remove(0);
            }
        }
    }
    
    pub fn update(&mut self, current_time: f64) {
        // Auto-dismiss expired alerts
        let to_dismiss: Vec<u64> = self.active_alerts
            .iter()
            .filter(|a| {
                a.auto_dismiss_time.map(|t| current_time > t).unwrap_or(false)
            })
            .map(|a| a.id)
            .collect();
        
        for id in to_dismiss {
            self.dismiss(id);
        }
    }
}

fn generate_collision_alerts(
    threats: Res<ThreatQueue>,
    mut alerts: ResMut<AlertManager>,
    time: Res<Time>,
    names: Query<&Name>,
) {
    let mut next_id = time.elapsed_secs_f64() as u64 * 1000;
    
    for threat in threats.threats.iter() {
        let severity = match threat.threat_level {
            ThreatLevel::Critical => AlertSeverity::Critical,
            ThreatLevel::High => AlertSeverity::Warning,
            ThreatLevel::Medium => AlertSeverity::Caution,
            _ => AlertSeverity::Info,
        };
        
        let name = names.get(threat.entity)
            .map(|n| n.to_string())
            .unwrap_or_else(|_| format!("Object {:?}", threat.entity));
        
        let message = format!(
            "Collision risk with {} - CPA: {:.0}m in {:.0}s",
            name,
            threat.closest_approach_distance,
            threat.time_to_closest_approach,
        );
        
        alerts.raise_alert(NavigationAlert {
            id: next_id,
            alert_type: AlertType::CollisionWarning,
            severity,
            message,
            timestamp: time.elapsed_secs_f64(),
            acknowledged: false,
            related_entity: Some(threat.entity),
            auto_dismiss_time: None,
        });
        
        next_id += 1;
    }
}
```

## Navigation Display Data

### Display State

```rust
#[derive(Resource)]
pub struct NavigationDisplay {
    /// Current display mode
    pub mode: DisplayMode,
    /// Zoom level (meters per screen unit)
    pub scale: f64,
    /// Display center offset from ship
    pub center_offset: Vec3,
    /// Visible layers
    pub layers: DisplayLayers,
    /// Selected object for info panel
    pub selected: Option<Entity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    /// Top-down orbital view
    Orbital,
    /// Forward-facing tactical view
    Tactical,
    /// 3D perspective view
    Perspective,
    /// Star map / celestial view
    Celestial,
}

#[derive(Clone, Copy, Debug)]
pub struct DisplayLayers {
    pub show_grid: bool,
    pub show_trajectories: bool,
    pub show_threats: bool,
    pub show_targets: bool,
    pub show_debris: bool,
    pub show_satellites: bool,
    pub show_celestial: bool,
    pub show_coverage: bool,
}

impl Default for DisplayLayers {
    fn default() -> Self {
        Self {
            show_grid: true,
            show_trajectories: true,
            show_threats: true,
            show_targets: true,
            show_debris: true,
            show_satellites: true,
            show_celestial: false,
            show_coverage: false,
        }
    }
}
```

### Navigation HUD Data

```rust
#[derive(Clone, Debug)]
pub struct NavigationHudData {
    /// Ship state
    pub altitude_km: f64,
    pub velocity_ms: f64,
    pub orbital_period_min: f64,
    pub inclination_deg: f64,
    
    /// Nearest object
    pub nearest_object: Option<String>,
    pub nearest_distance_km: f64,
    
    /// Primary target
    pub target_name: Option<String>,
    pub target_distance_km: f64,
    pub target_relative_velocity_ms: f64,
    pub target_bearing: (f64, f64), // azimuth, elevation
    
    /// Threat summary
    pub threat_count: usize,
    pub highest_threat: ThreatLevel,
    pub time_to_next_threat: Option<f64>,
    
    /// Fuel and resources
    pub fuel_remaining_kg: f64,
    pub delta_v_remaining_ms: f64,
}

fn compute_hud_data(
    nav_state: Res<NavigationState>,
    nearby: Res<NearbyObjectsArray>,
    targets: Res<TargetList>,
    threats: Res<ThreatQueue>,
    ship: Query<&SpaceshipPhysics, With<SpaceshipMarker>>,
) -> NavigationHudData {
    let ship_physics = ship.single();
    
    // Compute orbital elements
    let geo = ecef_to_geodetic(nav_state.origin_ecef);
    let altitude_km = geo.altitude / 1000.0;
    let velocity_ms = nav_state.origin_velocity.length();
    
    // Simplified orbital period (circular orbit approximation)
    let orbital_radius = nav_state.origin_ecef.length();
    let orbital_period_min = 2.0 * std::f64::consts::PI 
        * (orbital_radius.powi(3) / GM_EARTH).sqrt() / 60.0;
    
    // Inclination from velocity vector
    let h = nav_state.origin_ecef.cross(nav_state.origin_velocity);
    let inclination_deg = (h.z / h.length()).acos().to_degrees();
    
    // Nearest object
    let (nearest_object, nearest_distance_km) = nearby.objects.first()
        .map(|o| (o.name.clone(), o.distance / 1000.0))
        .unwrap_or((None, f64::INFINITY));
    
    // Primary target
    let (target_name, target_distance_km, target_relative_velocity_ms, target_bearing) = 
        targets.primary
            .and_then(|idx| targets.targets.get(idx))
            .map(|t| {
                let rel_pos = t.position_ecef - nav_state.origin_ecef;
                let rel_vel = t.velocity_ecef - nav_state.origin_velocity;
                let distance = rel_pos.length() / 1000.0;
                let velocity = rel_vel.length();
                
                // Bearing (simplified)
                let azimuth = rel_pos.y.atan2(rel_pos.x).to_degrees();
                let elevation = (rel_pos.z / rel_pos.length()).asin().to_degrees();
                
                (Some(t.name.clone()), distance, velocity, (azimuth, elevation))
            })
            .unwrap_or((None, 0.0, 0.0, (0.0, 0.0)));
    
    // Threat summary
    let threat_count = threats.threats.len();
    let highest_threat = threats.threats.iter()
        .map(|t| t.threat_level)
        .max()
        .unwrap_or(ThreatLevel::None);
    let time_to_next_threat = threats.threats.peek()
        .map(|t| t.time_to_closest_approach);
    
    // Fuel (from ship physics)
    let fuel_remaining_kg = 1000.0; // Placeholder
    let delta_v_remaining_ms = fuel_remaining_kg / ship_physics.mass as f64 * 3000.0; // Isp ~300s
    
    NavigationHudData {
        altitude_km,
        velocity_ms,
        orbital_period_min,
        inclination_deg,
        nearest_object,
        nearest_distance_km,
        target_name,
        target_distance_km,
        target_relative_velocity_ms,
        target_bearing,
        threat_count,
        highest_threat,
        time_to_next_threat,
        fuel_remaining_kg,
        delta_v_remaining_ms,
    }
}
```

## Plugin Integration

```rust
pub struct NavigationSystemPlugin;

impl Plugin for NavigationSystemPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .insert_resource(NavigationState::default())
            .insert_resource(NearbyObjectsArray {
                objects: Vec::new(),
                max_range: 100_000_000.0, // 100,000 km
                max_objects: 1000,
            })
            .insert_resource(ThreatQueue {
                threats: BinaryHeap::new(),
                threat_threshold: 10_000.0, // 10 km
                time_horizon: 86400.0, // 24 hours
            })
            .insert_resource(TargetList::default())
            .insert_resource(AlertManager {
                active_alerts: Vec::new(),
                alert_history: Vec::new(),
                max_history: 100,
            })
            .insert_resource(TrajectoryPropagator {
                time_step: 60.0,
                max_horizon: 86400.0,
                cache: std::collections::HashMap::new(),
            })
            .insert_resource(NavigationDisplay::default())
            // Systems
            .add_systems(Update, (
                update_nearby_objects,
                update_threat_queue,
                generate_collision_alerts,
            ).chain());
    }
}
```

## Next Steps

- [06_SAM3D_INTEGRATION.md](./06_SAM3D_INTEGRATION.md) - Telescope imagery processing
- [07_DYNAMIC_OBJECTS.md](./07_DYNAMIC_OBJECTS.md) - Object tracking details
