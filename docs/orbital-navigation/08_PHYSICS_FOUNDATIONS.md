# 08 - Physics Foundations

> Orbital mechanics, projection geometry, and the mathematical foundations of space navigation

## Overview

This document covers the physics principles underlying the Eustress Orbital Navigation System: Keplerian orbital mechanics, gravitational perturbations, projective geometry for telescope imaging, and relativistic corrections for high-precision applications.

## Orbital Mechanics

### Two-Body Problem

The foundation of orbital mechanics is the two-body problem: a spacecraft of negligible mass orbiting a central body.

#### Gravitational Parameters

```rust
/// Gravitational constants
pub mod constants {
    /// Gravitational constant (m³/kg/s²)
    pub const G: f64 = 6.67430e-11;
    
    /// Earth's gravitational parameter μ = GM (m³/s²)
    pub const GM_EARTH: f64 = 3.986004418e14;
    
    /// Earth's equatorial radius (m)
    pub const R_EARTH: f64 = 6_378_137.0;
    
    /// Earth's J2 zonal harmonic (oblateness)
    pub const J2_EARTH: f64 = 1.08263e-3;
    
    /// Sun's gravitational parameter (m³/s²)
    pub const GM_SUN: f64 = 1.32712440018e20;
    
    /// Moon's gravitational parameter (m³/s²)
    pub const GM_MOON: f64 = 4.9048695e12;
    
    /// Speed of light (m/s)
    pub const C: f64 = 299_792_458.0;
    
    /// Astronomical Unit (m)
    pub const AU: f64 = 149_597_870_700.0;
}
```

#### Orbital Elements

The six classical orbital elements uniquely define an orbit:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      CLASSICAL ORBITAL ELEMENTS                         │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Shape and Size:                                                        │
│  ├── a  : Semi-major axis (size of orbit)                               │
│  └── e  : Eccentricity (shape: 0=circle, 0<e<1=ellipse)                │
│                                                                          │
│  Orientation:                                                           │
│  ├── i  : Inclination (tilt from equatorial plane)                      │
│  ├── Ω  : Right Ascension of Ascending Node (RAAN)                      │
│  └── ω  : Argument of Periapsis (orientation in orbital plane)          │
│                                                                          │
│  Position:                                                              │
│  └── ν  : True Anomaly (current position in orbit)                      │
│      or M: Mean Anomaly (for propagation)                               │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

```rust
/// Classical Keplerian orbital elements
#[derive(Clone, Copy, Debug)]
pub struct OrbitalElements {
    /// Semi-major axis (meters)
    pub semi_major_axis: f64,
    /// Eccentricity (dimensionless, 0 ≤ e < 1 for ellipse)
    pub eccentricity: f64,
    /// Inclination (radians)
    pub inclination: f64,
    /// Right Ascension of Ascending Node (radians)
    pub raan: f64,
    /// Argument of periapsis (radians)
    pub arg_periapsis: f64,
    /// True anomaly at epoch (radians)
    pub true_anomaly: f64,
    /// Mean anomaly at epoch (radians) - alternative to true anomaly
    pub mean_anomaly_epoch: f64,
    /// Epoch (Julian Date)
    pub epoch: f64,
}

impl OrbitalElements {
    /// Calculate orbital period (seconds)
    pub fn period(&self) -> f64 {
        2.0 * std::f64::consts::PI * (self.semi_major_axis.powi(3) / GM_EARTH).sqrt()
    }
    
    /// Calculate mean motion (rad/s)
    pub fn mean_motion(&self) -> f64 {
        (GM_EARTH / self.semi_major_axis.powi(3)).sqrt()
    }
    
    /// Calculate specific orbital energy (J/kg)
    pub fn specific_energy(&self) -> f64 {
        -GM_EARTH / (2.0 * self.semi_major_axis)
    }
    
    /// Calculate specific angular momentum magnitude (m²/s)
    pub fn specific_angular_momentum(&self) -> f64 {
        (GM_EARTH * self.semi_major_axis * (1.0 - self.eccentricity.powi(2))).sqrt()
    }
    
    /// Calculate periapsis radius (meters)
    pub fn periapsis(&self) -> f64 {
        self.semi_major_axis * (1.0 - self.eccentricity)
    }
    
    /// Calculate apoapsis radius (meters)
    pub fn apoapsis(&self) -> f64 {
        self.semi_major_axis * (1.0 + self.eccentricity)
    }
    
    /// Calculate velocity at given true anomaly (m/s)
    pub fn velocity_at_anomaly(&self, true_anomaly: f64) -> f64 {
        let r = self.radius_at_anomaly(true_anomaly);
        (GM_EARTH * (2.0 / r - 1.0 / self.semi_major_axis)).sqrt()
    }
    
    /// Calculate radius at given true anomaly (meters)
    pub fn radius_at_anomaly(&self, true_anomaly: f64) -> f64 {
        let p = self.semi_major_axis * (1.0 - self.eccentricity.powi(2));
        p / (1.0 + self.eccentricity * true_anomaly.cos())
    }
}
```

### Kepler's Equation

Kepler's equation relates mean anomaly M to eccentric anomaly E:

```
M = E - e·sin(E)
```

```rust
/// Solve Kepler's equation using Newton-Raphson iteration
pub fn solve_kepler(mean_anomaly: f64, eccentricity: f64, tolerance: f64) -> f64 {
    let m = mean_anomaly % (2.0 * std::f64::consts::PI);
    
    // Initial guess
    let mut e_anomaly = if eccentricity < 0.8 {
        m
    } else {
        std::f64::consts::PI
    };
    
    // Newton-Raphson iteration
    for _ in 0..50 {
        let f = e_anomaly - eccentricity * e_anomaly.sin() - m;
        let f_prime = 1.0 - eccentricity * e_anomaly.cos();
        
        let delta = f / f_prime;
        e_anomaly -= delta;
        
        if delta.abs() < tolerance {
            break;
        }
    }
    
    e_anomaly
}

/// Convert eccentric anomaly to true anomaly
pub fn eccentric_to_true_anomaly(eccentric_anomaly: f64, eccentricity: f64) -> f64 {
    let beta = eccentricity / (1.0 + (1.0 - eccentricity.powi(2)).sqrt());
    eccentric_anomaly + 2.0 * (beta * eccentric_anomaly.sin() / (1.0 - beta * eccentric_anomaly.cos())).atan()
}

/// Convert true anomaly to eccentric anomaly
pub fn true_to_eccentric_anomaly(true_anomaly: f64, eccentricity: f64) -> f64 {
    2.0 * ((1.0 - eccentricity).sqrt() * (true_anomaly / 2.0).tan())
        .atan2((1.0 + eccentricity).sqrt())
}
```

### State Vector ↔ Orbital Elements

```rust
/// Convert position and velocity to orbital elements
pub fn state_to_elements(position: DVec3, velocity: DVec3) -> OrbitalElements {
    let r = position.length();
    let v = velocity.length();
    
    // Specific angular momentum
    let h = position.cross(velocity);
    let h_mag = h.length();
    
    // Node vector (points to ascending node)
    let n = DVec3::new(-h.y, h.x, 0.0);
    let n_mag = n.length();
    
    // Eccentricity vector
    let e_vec = ((v * v - GM_EARTH / r) * position - position.dot(velocity) * velocity) / GM_EARTH;
    let e = e_vec.length();
    
    // Semi-major axis
    let specific_energy = v * v / 2.0 - GM_EARTH / r;
    let a = -GM_EARTH / (2.0 * specific_energy);
    
    // Inclination
    let i = (h.z / h_mag).acos();
    
    // Right Ascension of Ascending Node
    let raan = if n_mag > 1e-10 {
        let omega = (n.x / n_mag).acos();
        if n.y < 0.0 { 2.0 * std::f64::consts::PI - omega } else { omega }
    } else {
        0.0
    };
    
    // Argument of periapsis
    let arg_periapsis = if n_mag > 1e-10 && e > 1e-10 {
        let omega = (n.dot(e_vec) / (n_mag * e)).acos();
        if e_vec.z < 0.0 { 2.0 * std::f64::consts::PI - omega } else { omega }
    } else {
        0.0
    };
    
    // True anomaly
    let true_anomaly = if e > 1e-10 {
        let nu = (e_vec.dot(position) / (e * r)).acos();
        if position.dot(velocity) < 0.0 { 2.0 * std::f64::consts::PI - nu } else { nu }
    } else {
        0.0
    };
    
    // Mean anomaly
    let eccentric_anomaly = true_to_eccentric_anomaly(true_anomaly, e);
    let mean_anomaly = eccentric_anomaly - e * eccentric_anomaly.sin();
    
    OrbitalElements {
        semi_major_axis: a,
        eccentricity: e,
        inclination: i,
        raan,
        arg_periapsis,
        true_anomaly,
        mean_anomaly_epoch: mean_anomaly,
        epoch: 0.0, // Set by caller
    }
}

/// Convert orbital elements to position and velocity
pub fn elements_to_state(elements: &OrbitalElements, true_anomaly: f64) -> (DVec3, DVec3) {
    let e = elements.eccentricity;
    let a = elements.semi_major_axis;
    let i = elements.inclination;
    let raan = elements.raan;
    let omega = elements.arg_periapsis;
    
    // Position in orbital plane
    let r = a * (1.0 - e * e) / (1.0 + e * true_anomaly.cos());
    let x_orb = r * true_anomaly.cos();
    let y_orb = r * true_anomaly.sin();
    
    // Velocity in orbital plane
    let p = a * (1.0 - e * e);
    let h = (GM_EARTH * p).sqrt();
    let vx_orb = -GM_EARTH / h * true_anomaly.sin();
    let vy_orb = GM_EARTH / h * (e + true_anomaly.cos());
    
    // Rotation matrices
    let cos_raan = raan.cos();
    let sin_raan = raan.sin();
    let cos_i = i.cos();
    let sin_i = i.sin();
    let cos_omega = omega.cos();
    let sin_omega = omega.sin();
    
    // Combined rotation: R_z(-Ω) * R_x(-i) * R_z(-ω)
    let r11 = cos_raan * cos_omega - sin_raan * sin_omega * cos_i;
    let r12 = -cos_raan * sin_omega - sin_raan * cos_omega * cos_i;
    let r21 = sin_raan * cos_omega + cos_raan * sin_omega * cos_i;
    let r22 = -sin_raan * sin_omega + cos_raan * cos_omega * cos_i;
    let r31 = sin_omega * sin_i;
    let r32 = cos_omega * sin_i;
    
    let position = DVec3::new(
        r11 * x_orb + r12 * y_orb,
        r21 * x_orb + r22 * y_orb,
        r31 * x_orb + r32 * y_orb,
    );
    
    let velocity = DVec3::new(
        r11 * vx_orb + r12 * vy_orb,
        r21 * vx_orb + r22 * vy_orb,
        r31 * vx_orb + r32 * vy_orb,
    );
    
    (position, velocity)
}
```

## Gravitational Perturbations

### J2 Perturbation (Earth's Oblateness)

Earth's equatorial bulge causes orbital precession:

```rust
/// J2 perturbation effects on orbital elements
pub struct J2Perturbation {
    /// Rate of change of RAAN (rad/s)
    pub raan_rate: f64,
    /// Rate of change of argument of periapsis (rad/s)
    pub arg_periapsis_rate: f64,
    /// Rate of change of mean anomaly (rad/s) - secular
    pub mean_anomaly_rate: f64,
}

impl J2Perturbation {
    pub fn calculate(elements: &OrbitalElements) -> Self {
        let a = elements.semi_major_axis;
        let e = elements.eccentricity;
        let i = elements.inclination;
        
        let n = (GM_EARTH / a.powi(3)).sqrt(); // Mean motion
        let p = a * (1.0 - e * e); // Semi-latus rectum
        
        let j2_factor = -1.5 * J2_EARTH * (R_EARTH / p).powi(2) * n;
        
        // RAAN precession (negative = westward for prograde orbits)
        let raan_rate = j2_factor * i.cos();
        
        // Argument of periapsis precession
        let arg_periapsis_rate = j2_factor * (2.0 - 2.5 * i.sin().powi(2));
        
        // Mean anomaly secular drift
        let mean_anomaly_rate = j2_factor * (1.0 - 1.5 * i.sin().powi(2)) 
            * (1.0 - e * e).sqrt();
        
        Self {
            raan_rate,
            arg_periapsis_rate,
            mean_anomaly_rate,
        }
    }
    
    /// Apply perturbations over time interval
    pub fn apply(&self, elements: &mut OrbitalElements, dt: f64) {
        elements.raan += self.raan_rate * dt;
        elements.arg_periapsis += self.arg_periapsis_rate * dt;
        elements.mean_anomaly_epoch += self.mean_anomaly_rate * dt;
        
        // Normalize angles to [0, 2π)
        elements.raan = elements.raan.rem_euclid(2.0 * std::f64::consts::PI);
        elements.arg_periapsis = elements.arg_periapsis.rem_euclid(2.0 * std::f64::consts::PI);
        elements.mean_anomaly_epoch = elements.mean_anomaly_epoch.rem_euclid(2.0 * std::f64::consts::PI);
    }
}

/// Sun-synchronous orbit inclination for given altitude
pub fn sun_synchronous_inclination(altitude_km: f64) -> f64 {
    let a = (R_EARTH + altitude_km * 1000.0);
    let n = (GM_EARTH / a.powi(3)).sqrt();
    
    // Required RAAN rate for sun-synchronous: 360°/year ≈ 1.991e-7 rad/s
    let required_raan_rate = 2.0 * std::f64::consts::PI / (365.25 * 86400.0);
    
    // Solve: raan_rate = -1.5 * J2 * (R/a)^2 * n * cos(i) = required_raan_rate
    let cos_i = -required_raan_rate / (1.5 * J2_EARTH * (R_EARTH / a).powi(2) * n);
    
    cos_i.acos()
}
```

### Third-Body Perturbations

```rust
/// Third-body gravitational acceleration
pub fn third_body_acceleration(
    satellite_pos: DVec3,      // Satellite position (GCRS)
    perturber_pos: DVec3,      // Perturbing body position (GCRS)
    gm_perturber: f64,         // Gravitational parameter of perturber
) -> DVec3 {
    // Vector from satellite to perturber
    let r_sat_to_perturber = perturber_pos - satellite_pos;
    let r_sat_to_perturber_mag = r_sat_to_perturber.length();
    
    // Vector from Earth to perturber
    let r_perturber_mag = perturber_pos.length();
    
    // Third-body acceleration (indirect + direct terms)
    gm_perturber * (
        r_sat_to_perturber / r_sat_to_perturber_mag.powi(3)
        - perturber_pos / r_perturber_mag.powi(3)
    )
}
```

## Projective Geometry

### Pinhole Camera Model

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      PINHOLE CAMERA GEOMETRY                            │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│                    3D World Point P(X, Y, Z)                            │
│                              │                                          │
│                              │                                          │
│                              ▼                                          │
│                    ┌─────────────────┐                                  │
│                    │   Lens/Pinhole  │                                  │
│                    │   (focal point) │                                  │
│                    └────────┬────────┘                                  │
│                             │ f (focal length)                          │
│                             ▼                                          │
│              ┌──────────────────────────────┐                          │
│              │      Image Plane             │                          │
│              │   p(x, y) = (fX/Z, fY/Z)     │                          │
│              └──────────────────────────────┘                          │
│                                                                          │
│  Projection: [x]   [f  0  0  0] [X]                                    │
│              [y] = [0  f  0  0] [Y]                                    │
│              [w]   [0  0  1  0] [Z]                                    │
│                                 [1]                                    │
│                                                                          │
│  Image coords: (x/w, y/w) = (fX/Z, fY/Z)                               │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

```rust
/// Pinhole camera projection
#[derive(Clone, Debug)]
pub struct PinholeCamera {
    /// Focal length (pixels)
    pub focal_length: f64,
    /// Principal point (pixels)
    pub principal_point: [f64; 2],
    /// Image dimensions
    pub image_size: [u32; 2],
}

impl PinholeCamera {
    /// Project 3D point to 2D image coordinates
    pub fn project(&self, point_camera: DVec3) -> Option<[f64; 2]> {
        if point_camera.z <= 0.0 {
            return None; // Behind camera
        }
        
        let x = self.focal_length * point_camera.x / point_camera.z + self.principal_point[0];
        let y = self.focal_length * point_camera.y / point_camera.z + self.principal_point[1];
        
        // Check if within image bounds
        if x >= 0.0 && x < self.image_size[0] as f64 
            && y >= 0.0 && y < self.image_size[1] as f64 {
            Some([x, y])
        } else {
            None
        }
    }
    
    /// Unproject 2D image point to 3D ray (unit vector in camera frame)
    pub fn unproject(&self, pixel: [f64; 2]) -> DVec3 {
        let x = (pixel[0] - self.principal_point[0]) / self.focal_length;
        let y = (pixel[1] - self.principal_point[1]) / self.focal_length;
        
        DVec3::new(x, y, 1.0).normalize()
    }
    
    /// Get field of view (radians)
    pub fn fov_horizontal(&self) -> f64 {
        2.0 * (self.image_size[0] as f64 / (2.0 * self.focal_length)).atan()
    }
    
    pub fn fov_vertical(&self) -> f64 {
        2.0 * (self.image_size[1] as f64 / (2.0 * self.focal_length)).atan()
    }
}
```

### Lens Distortion Models

```rust
/// Brown-Conrady distortion model
#[derive(Clone, Debug)]
pub struct BrownConradyDistortion {
    /// Radial distortion coefficients (k1, k2, k3, k4, k5, k6)
    pub radial: [f64; 6],
    /// Tangential distortion coefficients (p1, p2)
    pub tangential: [f64; 2],
}

impl BrownConradyDistortion {
    /// Apply distortion to normalized coordinates
    pub fn distort(&self, point: [f64; 2]) -> [f64; 2] {
        let x = point[0];
        let y = point[1];
        let r2 = x * x + y * y;
        let r4 = r2 * r2;
        let r6 = r4 * r2;
        
        let [k1, k2, k3, k4, k5, k6] = self.radial;
        let [p1, p2] = self.tangential;
        
        // Radial distortion factor
        let radial_num = 1.0 + k1 * r2 + k2 * r4 + k3 * r6;
        let radial_den = 1.0 + k4 * r2 + k5 * r4 + k6 * r6;
        let radial = radial_num / radial_den;
        
        // Tangential distortion
        let dx_tang = 2.0 * p1 * x * y + p2 * (r2 + 2.0 * x * x);
        let dy_tang = p1 * (r2 + 2.0 * y * y) + 2.0 * p2 * x * y;
        
        [
            x * radial + dx_tang,
            y * radial + dy_tang,
        ]
    }
    
    /// Remove distortion (iterative)
    pub fn undistort(&self, distorted: [f64; 2], iterations: usize) -> [f64; 2] {
        let mut point = distorted;
        
        for _ in 0..iterations {
            let distorted_estimate = self.distort(point);
            point[0] += distorted[0] - distorted_estimate[0];
            point[1] += distorted[1] - distorted_estimate[1];
        }
        
        point
    }
}
```

### Depth from Parallax

```rust
/// Calculate depth from parallax between two observations
pub fn depth_from_parallax(
    pixel_1: [f64; 2],
    pixel_2: [f64; 2],
    camera: &PinholeCamera,
    baseline: f64,  // Distance between observation positions (meters)
) -> f64 {
    // Disparity in pixels
    let disparity = ((pixel_1[0] - pixel_2[0]).powi(2) 
        + (pixel_1[1] - pixel_2[1]).powi(2)).sqrt();
    
    if disparity < 1e-6 {
        return f64::INFINITY; // No parallax = infinite distance
    }
    
    // Depth = baseline * focal_length / disparity
    baseline * camera.focal_length / disparity
}

/// Triangulate 3D position from two observations
pub fn triangulate(
    ray_1: DVec3,           // Ray direction from position 1
    position_1: DVec3,      // Observer position 1
    ray_2: DVec3,           // Ray direction from position 2
    position_2: DVec3,      // Observer position 2
) -> DVec3 {
    // Find closest point between two rays
    // Ray 1: P1 + t1 * D1
    // Ray 2: P2 + t2 * D2
    
    let d1 = ray_1.normalize();
    let d2 = ray_2.normalize();
    let d12 = position_2 - position_1;
    
    let d1_dot_d2 = d1.dot(d2);
    let d1_dot_d12 = d1.dot(d12);
    let d2_dot_d12 = d2.dot(d12);
    
    let denom = 1.0 - d1_dot_d2 * d1_dot_d2;
    
    if denom.abs() < 1e-10 {
        // Rays are parallel
        return position_1 + d1 * d1_dot_d12;
    }
    
    let t1 = (d1_dot_d12 - d1_dot_d2 * d2_dot_d12) / denom;
    let t2 = (d1_dot_d2 * d1_dot_d12 - d2_dot_d12) / denom;
    
    // Midpoint of closest approach
    let point_1 = position_1 + d1 * t1;
    let point_2 = position_2 + d2 * t2;
    
    (point_1 + point_2) * 0.5
}
```

## Relativistic Corrections

### Special Relativity (Velocity Effects)

```rust
/// Lorentz factor γ = 1/√(1 - v²/c²)
pub fn lorentz_factor(velocity: f64) -> f64 {
    let beta = velocity / C;
    1.0 / (1.0 - beta * beta).sqrt()
}

/// Time dilation: proper_time = coordinate_time / γ
pub fn time_dilation(coordinate_time: f64, velocity: f64) -> f64 {
    coordinate_time / lorentz_factor(velocity)
}

/// Relativistic Doppler factor
pub fn doppler_factor(velocity: f64, angle: f64) -> f64 {
    // angle = 0 for approaching, π for receding
    let beta = velocity / C;
    let gamma = lorentz_factor(velocity);
    
    1.0 / (gamma * (1.0 + beta * angle.cos()))
}

/// Aberration of light
pub fn aberration(
    true_direction: DVec3,  // True direction to star (unit vector)
    observer_velocity: DVec3, // Observer velocity
) -> DVec3 {
    let v = observer_velocity;
    let v_mag = v.length();
    let beta = v_mag / C;
    let gamma = lorentz_factor(v_mag);
    
    if beta < 1e-10 {
        return true_direction;
    }
    
    let v_unit = v / v_mag;
    let cos_theta = true_direction.dot(v_unit);
    
    // Aberration formula
    let apparent = (true_direction / gamma + v_unit * (cos_theta + beta / gamma) 
        / (1.0 + beta * cos_theta)).normalize();
    
    apparent
}

/// Smoothness profile for fine-grained navigation
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SmoothnessProfile {
    Standard,    // σ = 1.0 - Baseline navigation
    Smooth,      // σ = 2.0 - Enhanced interpolation
    UltraSmooth, // σ = 4.0 - Maximum fidelity
    Cinematic,   // σ = 8.0 - Film-quality rendering
}

impl SmoothnessProfile {
    pub fn factor(&self) -> f64 {
        match self {
            Self::Standard => 1.0,
            Self::Smooth => 2.0,
            Self::UltraSmooth => 4.0,
            Self::Cinematic => 8.0,
        }
    }
}

/// Fine-grained relativistic frame rate for smooth space travel
/// 
/// FPS(v, σ) = FPS_base × √((1 + β)/(1 - β)) × σ
///
/// Where:
///   β = v/c (velocity as fraction of light speed)
///   σ = smoothness factor (1.0 to 8.0)
///   FPS_base = 240 (high-fidelity rest frame rate)
pub fn relativistic_frame_rate(velocity: f64, base_fps: f64) -> f64 {
    let beta = velocity / C;
    
    if beta < 1e-10 {
        return base_fps;
    }
    
    if beta >= 1.0 {
        return f64::INFINITY;
    }
    
    base_fps * ((1.0 + beta) / (1.0 - beta)).sqrt()
}

/// Adaptive frame rate with smoothness profile
pub fn adaptive_frame_rate(velocity: f64, base_fps: f64, profile: SmoothnessProfile) -> f64 {
    relativistic_frame_rate(velocity, base_fps) * profile.factor()
}

/// Calculate minimum safe frame rate for given velocity and reaction distance
pub fn navigation_frame_rate(
    velocity: f64,           // Ship velocity (m/s)
    base_fps: f64,           // Base frame rate at rest (e.g., 240)
    min_reaction_dist: f64,  // Minimum safe reaction distance (m)
) -> f64 {
    let relativistic_fps = relativistic_frame_rate(velocity, base_fps);
    let reaction_fps = velocity / min_reaction_dist;
    relativistic_fps.max(reaction_fps)
}

/// Fine-grained update tier configuration
#[derive(Clone, Debug)]
pub struct UpdateTiers {
    /// Physics integration rate (Hz) - force, collision
    pub physics_hz: f64,
    /// Navigation update rate (Hz) - position, threats
    pub navigation_hz: f64,
    /// Rendering frame rate (Hz) - visual output
    pub rendering_hz: f64,
    /// Interpolation rate (Hz) - sub-frame motion
    pub interpolation_hz: f64,
    /// Prediction rate (Hz) - trajectory extrapolation
    pub prediction_hz: f64,
}

impl Default for UpdateTiers {
    fn default() -> Self {
        Self {
            physics_hz: 1000.0,      // 1 ms precision
            navigation_hz: 500.0,    // 2 ms precision
            rendering_hz: 240.0,     // Base visual rate
            interpolation_hz: 10000.0, // 100 μs sub-frame
            prediction_hz: 100.0,    // 10 ms extrapolation
        }
    }
}

impl UpdateTiers {
    /// Scale all tiers for relativistic velocity
    pub fn scale_for_velocity(&self, velocity: f64, profile: SmoothnessProfile) -> Self {
        let scale = ((1.0 + velocity / C) / (1.0 - velocity / C)).sqrt() * profile.factor();
        Self {
            physics_hz: self.physics_hz * scale,
            navigation_hz: self.navigation_hz * scale,
            rendering_hz: self.rendering_hz * scale,
            interpolation_hz: self.interpolation_hz * scale,
            prediction_hz: self.prediction_hz * scale,
        }
    }
}

/// Cubic Hermite spline interpolation for smooth position
pub fn hermite_interpolate(
    p0: DVec3,  // Position at t=0
    v0: DVec3,  // Velocity at t=0
    p1: DVec3,  // Position at t=1
    v1: DVec3,  // Velocity at t=1
    t: f64,     // Interpolation parameter [0, 1]
) -> DVec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    
    // Hermite basis functions
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    
    p0 * h00 + v0 * h10 + p1 * h01 + v1 * h11
}

/// Relativistic length contraction correction
pub fn length_contraction_correction(position: DVec3, velocity: DVec3) -> DVec3 {
    let v_mag = velocity.length();
    if v_mag < 1e-10 {
        return position;
    }
    
    let beta = v_mag / C;
    let gamma = lorentz_factor(v_mag);
    let v_unit = velocity / v_mag;
    
    // Contract only the component parallel to velocity
    let parallel = position.dot(v_unit) * v_unit;
    let perpendicular = position - parallel;
    
    perpendicular + parallel / gamma
}

/// Light travel time compensation for distant objects
pub fn light_time_correction(
    observer_pos: DVec3,
    object_pos: DVec3,
    object_vel: DVec3,
) -> DVec3 {
    let distance = (object_pos - observer_pos).length();
    let light_time = distance / C;
    
    // Extrapolate object position back by light travel time
    object_pos - object_vel * light_time
}

/// Sub-frame position prediction with relativistic corrections
pub fn predict_position(
    position: DVec3,
    velocity: DVec3,
    acceleration: DVec3,
    dt: f64,
    observer_velocity: DVec3,
) -> DVec3 {
    // Quadratic prediction
    let predicted = position + velocity * dt + acceleration * 0.5 * dt * dt;
    
    // Apply relativistic corrections if moving fast
    let rel_velocity = velocity - observer_velocity;
    let rel_speed = rel_velocity.length();
    
    if rel_speed > 0.01 * C {
        length_contraction_correction(predicted, rel_velocity)
    } else {
        predicted
    }
}
```

### Fine-Grained Relativistic Navigation Table

| Velocity | β | γ | Standard (σ=1) | Smooth (σ=2) | Ultra (σ=4) | Distance/Frame (Ultra) |
|----------|---|---|----------------|--------------|-------------|------------------------|
| 0 (rest) | 0.0 | 1.000 | 240 Hz | 480 Hz | 960 Hz | 0 m |
| 0.01c | 0.01 | 1.000 | 242 Hz | 484 Hz | 969 Hz | 3.1 km |
| 0.05c | 0.05 | 1.001 | 252 Hz | 505 Hz | 1,010 Hz | 14.8 km |
| 0.1c | 0.1 | 1.005 | 266 Hz | 531 Hz | 1,063 Hz | 28.2 km |
| 0.25c | 0.25 | 1.033 | 310 Hz | 620 Hz | 1,240 Hz | 60.4 km |
| 0.5c | 0.5 | 1.155 | 416 Hz | 832 Hz | 1,663 Hz | 90.1 km |
| 0.75c | 0.75 | 1.512 | 638 Hz | 1,277 Hz | 2,554 Hz | 88.0 km |
| 0.9c | 0.9 | 2.294 | 1,046 Hz | 2,091 Hz | 4,183 Hz | 64.5 km |
| 0.99c | 0.99 | 7.089 | 3,382 Hz | 6,764 Hz | 13,528 Hz | 21.9 km |

**Note**: At 0.5c with Ultra-Smooth profile (σ=4), the system runs at 1,663 Hz with 
sub-frame interpolation at 16,630 Hz, achieving ~90 km/frame for smooth visual travel.
Hermite interpolation + relativistic corrections ensure seamless motion perception.

### General Relativity (Gravitational Effects)

```rust
/// Gravitational time dilation factor at radius r from mass M
pub fn gravitational_time_dilation(r: f64, gm: f64) -> f64 {
    // τ/t = √(1 - 2GM/rc²)
    let schwarzschild_factor = 2.0 * gm / (r * C * C);
    (1.0 - schwarzschild_factor).sqrt()
}

/// Combined time dilation for GPS satellites
pub fn gps_time_dilation(altitude_m: f64, velocity_ms: f64) -> f64 {
    let r = R_EARTH + altitude_m;
    
    // Gravitational effect (speeds up clock relative to ground)
    let grav_factor = gravitational_time_dilation(r, GM_EARTH) 
        / gravitational_time_dilation(R_EARTH, GM_EARTH);
    
    // Velocity effect (slows down clock)
    let vel_factor = 1.0 / lorentz_factor(velocity_ms);
    
    grav_factor * vel_factor
}

/// Shapiro delay (light travel time increase near massive body)
pub fn shapiro_delay(
    r_emit: f64,    // Distance of emitter from mass center
    r_recv: f64,    // Distance of receiver from mass center
    d: f64,         // Straight-line distance between emitter and receiver
    gm: f64,        // Gravitational parameter of central body
) -> f64 {
    // Δt = (2GM/c³) * ln((r_emit + r_recv + d) / (r_emit + r_recv - d))
    let factor = 2.0 * gm / (C * C * C);
    factor * ((r_emit + r_recv + d) / (r_emit + r_recv - d)).ln()
}
```

## Numerical Integration

### Runge-Kutta 4th Order

```rust
/// State vector for orbital integration
#[derive(Clone, Copy)]
pub struct OrbitalState {
    pub position: DVec3,
    pub velocity: DVec3,
}

/// Acceleration function type
pub type AccelerationFn = fn(f64, &OrbitalState) -> DVec3;

/// RK4 integration step
pub fn rk4_step(
    t: f64,
    state: OrbitalState,
    dt: f64,
    acceleration: AccelerationFn,
) -> OrbitalState {
    let k1_v = acceleration(t, &state);
    let k1_r = state.velocity;
    
    let state_2 = OrbitalState {
        position: state.position + k1_r * (dt / 2.0),
        velocity: state.velocity + k1_v * (dt / 2.0),
    };
    let k2_v = acceleration(t + dt / 2.0, &state_2);
    let k2_r = state_2.velocity;
    
    let state_3 = OrbitalState {
        position: state.position + k2_r * (dt / 2.0),
        velocity: state.velocity + k2_v * (dt / 2.0),
    };
    let k3_v = acceleration(t + dt / 2.0, &state_3);
    let k3_r = state_3.velocity;
    
    let state_4 = OrbitalState {
        position: state.position + k3_r * dt,
        velocity: state.velocity + k3_v * dt,
    };
    let k4_v = acceleration(t + dt, &state_4);
    let k4_r = state_4.velocity;
    
    OrbitalState {
        position: state.position + (k1_r + k2_r * 2.0 + k3_r * 2.0 + k4_r) * (dt / 6.0),
        velocity: state.velocity + (k1_v + k2_v * 2.0 + k3_v * 2.0 + k4_v) * (dt / 6.0),
    }
}

/// Two-body acceleration
pub fn two_body_acceleration(_t: f64, state: &OrbitalState) -> DVec3 {
    let r = state.position.length();
    -GM_EARTH / (r * r * r) * state.position
}

/// Full force model with perturbations
pub fn full_acceleration(t: f64, state: &OrbitalState, ephemeris: &EphemerisEngine) -> DVec3 {
    let r = state.position.length();
    
    // Central body
    let a_central = -GM_EARTH / (r * r * r) * state.position;
    
    // J2 perturbation
    let z2 = state.position.z * state.position.z;
    let r2 = r * r;
    let j2_factor = 1.5 * J2_EARTH * GM_EARTH * R_EARTH * R_EARTH / (r2 * r2 * r);
    let a_j2 = DVec3::new(
        state.position.x * (5.0 * z2 / r2 - 1.0),
        state.position.y * (5.0 * z2 / r2 - 1.0),
        state.position.z * (5.0 * z2 / r2 - 3.0),
    ) * j2_factor;
    
    // Third body (Moon)
    let jd = 2451545.0 + t / 86400.0; // Simplified
    let moon_pos = ephemeris.position_geocentric(CelestialBody::Moon, jd);
    let a_moon = third_body_acceleration(state.position, moon_pos, GM_MOON);
    
    // Third body (Sun)
    let sun_pos = ephemeris.position_geocentric(CelestialBody::Sun, jd);
    let a_sun = third_body_acceleration(state.position, sun_pos, GM_SUN);
    
    a_central + a_j2 + a_moon + a_sun
}
```

## Summary

The physics foundations provide:

1. **Orbital Mechanics**: Kepler's laws, orbital elements, state vector conversions
2. **Perturbations**: J2 oblateness, third-body effects, atmospheric drag
3. **Projective Geometry**: Camera models, distortion correction, triangulation
4. **Relativistic Effects**: Time dilation, aberration, Shapiro delay
5. **Numerical Methods**: High-fidelity propagation with RK4 integration

## Next Steps

- [09_IMPLEMENTATION_GUIDE.md](./09_IMPLEMENTATION_GUIDE.md) - Full Rust/Bevy implementation
- [10_API_REFERENCE.md](./10_API_REFERENCE.md) - Complete API documentation
