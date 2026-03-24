//! # Scripting Data Types
//!
//! Roblox-compatible data types for scripting: Vector3, CFrame, Color3, UDim, UDim2, Ray.
//! These are pure Rust types that can be used by both Rune and Luau runtimes.
//!
//! ## Table of Contents
//!
//! 1. **Vector2** — 2D vector with operators
//! 2. **Vector3** — 3D vector with full Roblox API
//! 3. **CFrame** — Coordinate frame (position + rotation)
//! 4. **Color3** — RGB color with constructors and lerp
//! 5. **UDim / UDim2** — UI dimension types
//! 6. **Ray** — Origin + direction for raycasting
//! 7. **NumberRange** — Min/max range
//! 8. **TweenInfo** — Animation parameters

use std::ops::{Add, Sub, Mul, Div, Neg};

// ============================================================================
// 1. Vector2
// ============================================================================

/// 2D vector matching Roblox Vector2 API.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector2 {
    pub x: f64,
    pub y: f64,
}

impl Vector2 {
    pub const ZERO: Vector2 = Vector2 { x: 0.0, y: 0.0 };
    pub const ONE: Vector2 = Vector2 { x: 1.0, y: 1.0 };
    pub const X_AXIS: Vector2 = Vector2 { x: 1.0, y: 0.0 };
    pub const Y_AXIS: Vector2 = Vector2 { x: 0.0, y: 1.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn magnitude(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn unit(&self) -> Self {
        let mag = self.magnitude();
        if mag > 0.0 {
            Self { x: self.x / mag, y: self.y / mag }
        } else {
            Self::ZERO
        }
    }

    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn cross(&self, other: &Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            x: self.x + (goal.x - self.x) * alpha,
            y: self.y + (goal.y - self.y) * alpha,
        }
    }

    pub fn min(&self, other: &Self) -> Self {
        Self {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
        }
    }

    pub fn max(&self, other: &Self) -> Self {
        Self {
            x: self.x.max(other.x),
            y: self.y.max(other.y),
        }
    }

    pub fn abs(&self) -> Self {
        Self { x: self.x.abs(), y: self.y.abs() }
    }

    pub fn floor(&self) -> Self {
        Self { x: self.x.floor(), y: self.y.floor() }
    }

    pub fn ceil(&self) -> Self {
        Self { x: self.x.ceil(), y: self.y.ceil() }
    }
}

impl Add for Vector2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl Sub for Vector2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

impl Mul<f64> for Vector2 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self { x: self.x * rhs, y: self.y * rhs }
    }
}

impl Mul<Vector2> for Vector2 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self { x: self.x * rhs.x, y: self.y * rhs.y }
    }
}

impl Div<f64> for Vector2 {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        Self { x: self.x / rhs, y: self.y / rhs }
    }
}

impl Div<Vector2> for Vector2 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self { x: self.x / rhs.x, y: self.y / rhs.y }
    }
}

impl Neg for Vector2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self { x: -self.x, y: -self.y }
    }
}

// ============================================================================
// 2. Vector3
// ============================================================================

/// 3D vector matching Roblox Vector3 API.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub const ZERO: Vector3 = Vector3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Vector3 = Vector3 { x: 1.0, y: 1.0, z: 1.0 };
    pub const X_AXIS: Vector3 = Vector3 { x: 1.0, y: 0.0, z: 0.0 };
    pub const Y_AXIS: Vector3 = Vector3 { x: 0.0, y: 1.0, z: 0.0 };
    pub const Z_AXIS: Vector3 = Vector3 { x: 0.0, y: 0.0, z: 1.0 };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Convert to Bevy Vec3 (f32)
    pub fn to_vec3(&self) -> bevy::math::Vec3 {
        bevy::math::Vec3::new(self.x as f32, self.y as f32, self.z as f32)
    }

    /// Create from Bevy Vec3
    pub fn from_vec3(v: bevy::math::Vec3) -> Self {
        Self { x: v.x as f64, y: v.y as f64, z: v.z as f64 }
    }

    /// Convert to [f32; 3] array
    pub fn to_array_f32(&self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }

    /// Convert to [f64; 3] array
    pub fn to_array(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    pub fn magnitude(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn unit(&self) -> Self {
        let mag = self.magnitude();
        if mag > 0.0 {
            Self { x: self.x / mag, y: self.y / mag, z: self.z / mag }
        } else {
            Self::ZERO
        }
    }

    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: &Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            x: self.x + (goal.x - self.x) * alpha,
            y: self.y + (goal.y - self.y) * alpha,
            z: self.z + (goal.z - self.z) * alpha,
        }
    }

    pub fn min(&self, other: &Self) -> Self {
        Self {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
            z: self.z.min(other.z),
        }
    }

    pub fn max(&self, other: &Self) -> Self {
        Self {
            x: self.x.max(other.x),
            y: self.y.max(other.y),
            z: self.z.max(other.z),
        }
    }

    pub fn abs(&self) -> Self {
        Self { x: self.x.abs(), y: self.y.abs(), z: self.z.abs() }
    }

    pub fn floor(&self) -> Self {
        Self { x: self.x.floor(), y: self.y.floor(), z: self.z.floor() }
    }

    pub fn ceil(&self) -> Self {
        Self { x: self.x.ceil(), y: self.y.ceil(), z: self.z.ceil() }
    }

    pub fn angle(&self, other: &Self) -> f64 {
        let dot = self.dot(other);
        let mag_product = self.magnitude() * other.magnitude();
        if mag_product > 0.0 {
            (dot / mag_product).clamp(-1.0, 1.0).acos()
        } else {
            0.0
        }
    }

    /// Fuzzy equality check
    pub fn fuzzy_eq(&self, other: &Self, epsilon: f64) -> bool {
        (self.x - other.x).abs() < epsilon &&
        (self.y - other.y).abs() < epsilon &&
        (self.z - other.z).abs() < epsilon
    }
}

impl Add for Vector3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

impl Sub for Vector3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z }
    }
}

impl Mul<f64> for Vector3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs }
    }
}

impl Mul<Vector3> for Vector3 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self { x: self.x * rhs.x, y: self.y * rhs.y, z: self.z * rhs.z }
    }
}

impl Div<f64> for Vector3 {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        Self { x: self.x / rhs, y: self.y / rhs, z: self.z / rhs }
    }
}

impl Div<Vector3> for Vector3 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self { x: self.x / rhs.x, y: self.y / rhs.y, z: self.z / rhs.z }
    }
}

impl Neg for Vector3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self { x: -self.x, y: -self.y, z: -self.z }
    }
}

impl std::fmt::Display for Vector3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}, {}", self.x, self.y, self.z)
    }
}

// ============================================================================
// 3. CFrame — Coordinate Frame (Position + Rotation)
// ============================================================================

/// Coordinate frame matching Roblox CFrame API.
/// Internally stores position + rotation matrix (3x3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CFrame {
    /// Position component
    pub position: Vector3,
    /// Rotation matrix stored as column vectors (right, up, -look)
    /// m[0] = RightVector, m[1] = UpVector, m[2] = -LookVector
    rotation: [[f64; 3]; 3],
}

impl Default for CFrame {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl CFrame {
    pub const IDENTITY: CFrame = CFrame {
        position: Vector3::ZERO,
        rotation: [
            [1.0, 0.0, 0.0],  // RightVector (X)
            [0.0, 1.0, 0.0],  // UpVector (Y)
            [0.0, 0.0, 1.0],  // -LookVector (Z)
        ],
    };

    /// Create CFrame at position with identity rotation
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            position: Vector3::new(x, y, z),
            rotation: Self::IDENTITY.rotation,
        }
    }

    /// Create CFrame from position Vector3
    pub fn from_position(pos: Vector3) -> Self {
        Self {
            position: pos,
            rotation: Self::IDENTITY.rotation,
        }
    }

    /// Create CFrame looking from `position` towards `look_at` with optional up vector
    pub fn look_at(position: Vector3, look_at: Vector3, up: Option<Vector3>) -> Self {
        let up = up.unwrap_or(Vector3::Y_AXIS);
        let look = (look_at - position).unit();
        
        if look.magnitude() < 1e-10 {
            return Self::from_position(position);
        }

        let right = up.cross(&look).unit();
        let actual_up = look.cross(&right);

        Self {
            position,
            rotation: [
                [right.x, right.y, right.z],
                [actual_up.x, actual_up.y, actual_up.z],
                [-look.x, -look.y, -look.z],  // Negate look for Roblox convention
            ],
        }
    }

    /// Create CFrame from Euler angles (radians) in XYZ order
    pub fn angles(rx: f64, ry: f64, rz: f64) -> Self {
        Self::from_euler_angles_xyz(rx, ry, rz)
    }

    /// Create CFrame from Euler angles (radians) in XYZ order
    pub fn from_euler_angles_xyz(rx: f64, ry: f64, rz: f64) -> Self {
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();

        // Combined rotation matrix for XYZ order
        Self {
            position: Vector3::ZERO,
            rotation: [
                [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
                [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
                [-sy, sx * cy, cx * cy],
            ],
        }
    }

    /// Create CFrame from Euler angles (radians) in YXZ order (Roblox default)
    pub fn from_euler_angles_yxz(ry: f64, rx: f64, rz: f64) -> Self {
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();

        Self {
            position: Vector3::ZERO,
            rotation: [
                [cy * cz + sy * sx * sz, cz * sy * sx - cy * sz, cx * sy],
                [cx * sz, cx * cz, -sx],
                [cy * sx * sz - cz * sy, sy * sz + cy * cz * sx, cy * cx],
            ],
        }
    }

    /// Create CFrame from axis-angle rotation
    pub fn from_axis_angle(axis: Vector3, angle: f64) -> Self {
        let axis = axis.unit();
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;

        Self {
            position: Vector3::ZERO,
            rotation: [
                [t * axis.x * axis.x + c, t * axis.x * axis.y - s * axis.z, t * axis.x * axis.z + s * axis.y],
                [t * axis.x * axis.y + s * axis.z, t * axis.y * axis.y + c, t * axis.y * axis.z - s * axis.x],
                [t * axis.x * axis.z - s * axis.y, t * axis.y * axis.z + s * axis.x, t * axis.z * axis.z + c],
            ],
        }
    }

    /// Create CFrame from rotation matrix components
    pub fn from_matrix(
        pos: Vector3,
        right: Vector3,
        up: Vector3,
        back: Vector3,
    ) -> Self {
        Self {
            position: pos,
            rotation: [
                [right.x, right.y, right.z],
                [up.x, up.y, up.z],
                [back.x, back.y, back.z],
            ],
        }
    }

    /// Convert to Bevy Transform
    pub fn to_transform(&self) -> bevy::transform::components::Transform {
        use bevy::math::{Quat, Vec3};
        
        let quat = self.to_quaternion();
        bevy::transform::components::Transform {
            translation: self.position.to_vec3(),
            rotation: Quat::from_xyzw(quat[0] as f32, quat[1] as f32, quat[2] as f32, quat[3] as f32),
            scale: Vec3::ONE,
        }
    }

    /// Create from Bevy Transform
    pub fn from_transform(transform: &bevy::transform::components::Transform) -> Self {
        let pos = Vector3::from_vec3(transform.translation);
        let quat = transform.rotation;
        Self::from_position(pos) * Self::from_quaternion([
            quat.x as f64,
            quat.y as f64,
            quat.z as f64,
            quat.w as f64,
        ])
    }

    /// Convert rotation to quaternion [x, y, z, w]
    pub fn to_quaternion(&self) -> [f64; 4] {
        let m = &self.rotation;
        let trace = m[0][0] + m[1][1] + m[2][2];

        if trace > 0.0 {
            let s = 0.5 / (trace + 1.0).sqrt();
            [
                (m[1][2] - m[2][1]) * s,
                (m[2][0] - m[0][2]) * s,
                (m[0][1] - m[1][0]) * s,
                0.25 / s,
            ]
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
            [
                0.25 * s,
                (m[1][0] + m[0][1]) / s,
                (m[2][0] + m[0][2]) / s,
                (m[1][2] - m[2][1]) / s,
            ]
        } else if m[1][1] > m[2][2] {
            let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
            [
                (m[1][0] + m[0][1]) / s,
                0.25 * s,
                (m[2][1] + m[1][2]) / s,
                (m[2][0] - m[0][2]) / s,
            ]
        } else {
            let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
            [
                (m[2][0] + m[0][2]) / s,
                (m[2][1] + m[1][2]) / s,
                0.25 * s,
                (m[0][1] - m[1][0]) / s,
            ]
        }
    }

    /// Create from quaternion [x, y, z, w]
    pub fn from_quaternion(q: [f64; 4]) -> Self {
        let [x, y, z, w] = q;
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        let wx = w * x;
        let wy = w * y;
        let wz = w * z;

        Self {
            position: Vector3::ZERO,
            rotation: [
                [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
                [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
                [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
            ],
        }
    }

    // ========== Accessors ==========

    /// X component of position
    pub fn x(&self) -> f64 { self.position.x }
    /// Y component of position
    pub fn y(&self) -> f64 { self.position.y }
    /// Z component of position
    pub fn z(&self) -> f64 { self.position.z }

    /// Right vector (X axis in local space)
    pub fn right_vector(&self) -> Vector3 {
        Vector3::new(self.rotation[0][0], self.rotation[0][1], self.rotation[0][2])
    }

    /// Up vector (Y axis in local space)
    pub fn up_vector(&self) -> Vector3 {
        Vector3::new(self.rotation[1][0], self.rotation[1][1], self.rotation[1][2])
    }

    /// Look vector (negative Z axis in local space)
    pub fn look_vector(&self) -> Vector3 {
        Vector3::new(-self.rotation[2][0], -self.rotation[2][1], -self.rotation[2][2])
    }

    /// Back vector (Z axis in local space, opposite of LookVector)
    pub fn back_vector(&self) -> Vector3 {
        Vector3::new(self.rotation[2][0], self.rotation[2][1], self.rotation[2][2])
    }

    // ========== Transformations ==========

    /// Inverse of this CFrame
    pub fn inverse(&self) -> Self {
        // Transpose rotation (inverse of orthonormal matrix)
        let inv_rot = [
            [self.rotation[0][0], self.rotation[1][0], self.rotation[2][0]],
            [self.rotation[0][1], self.rotation[1][1], self.rotation[2][1]],
            [self.rotation[0][2], self.rotation[1][2], self.rotation[2][2]],
        ];

        // Inverse position = -R^T * p
        let inv_pos = Vector3::new(
            -(inv_rot[0][0] * self.position.x + inv_rot[0][1] * self.position.y + inv_rot[0][2] * self.position.z),
            -(inv_rot[1][0] * self.position.x + inv_rot[1][1] * self.position.y + inv_rot[1][2] * self.position.z),
            -(inv_rot[2][0] * self.position.x + inv_rot[2][1] * self.position.y + inv_rot[2][2] * self.position.z),
        );

        Self {
            position: inv_pos,
            rotation: inv_rot,
        }
    }

    /// Transform a point from local to world space
    pub fn point_to_world_space(&self, point: Vector3) -> Vector3 {
        Vector3::new(
            self.rotation[0][0] * point.x + self.rotation[1][0] * point.y + self.rotation[2][0] * point.z + self.position.x,
            self.rotation[0][1] * point.x + self.rotation[1][1] * point.y + self.rotation[2][1] * point.z + self.position.y,
            self.rotation[0][2] * point.x + self.rotation[1][2] * point.y + self.rotation[2][2] * point.z + self.position.z,
        )
    }

    /// Transform a point from world to local space
    pub fn point_to_object_space(&self, point: Vector3) -> Vector3 {
        let p = point - self.position;
        Vector3::new(
            self.rotation[0][0] * p.x + self.rotation[0][1] * p.y + self.rotation[0][2] * p.z,
            self.rotation[1][0] * p.x + self.rotation[1][1] * p.y + self.rotation[1][2] * p.z,
            self.rotation[2][0] * p.x + self.rotation[2][1] * p.y + self.rotation[2][2] * p.z,
        )
    }

    /// Transform a vector (direction) from local to world space (no translation)
    pub fn vector_to_world_space(&self, vector: Vector3) -> Vector3 {
        Vector3::new(
            self.rotation[0][0] * vector.x + self.rotation[1][0] * vector.y + self.rotation[2][0] * vector.z,
            self.rotation[0][1] * vector.x + self.rotation[1][1] * vector.y + self.rotation[2][1] * vector.z,
            self.rotation[0][2] * vector.x + self.rotation[1][2] * vector.y + self.rotation[2][2] * vector.z,
        )
    }

    /// Transform a vector (direction) from world to local space (no translation)
    pub fn vector_to_object_space(&self, vector: Vector3) -> Vector3 {
        Vector3::new(
            self.rotation[0][0] * vector.x + self.rotation[0][1] * vector.y + self.rotation[0][2] * vector.z,
            self.rotation[1][0] * vector.x + self.rotation[1][1] * vector.y + self.rotation[1][2] * vector.z,
            self.rotation[2][0] * vector.x + self.rotation[2][1] * vector.y + self.rotation[2][2] * vector.z,
        )
    }

    /// Transform another CFrame from local to world space
    pub fn to_world_space(&self, cf: CFrame) -> CFrame {
        *self * cf
    }

    /// Transform another CFrame from world to local space
    pub fn to_object_space(&self, cf: CFrame) -> CFrame {
        self.inverse() * cf
    }

    /// Spherical linear interpolation
    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        let pos = self.position.lerp(&goal.position, alpha);
        
        // SLERP for rotation via quaternion
        let q1 = self.to_quaternion();
        let q2 = goal.to_quaternion();
        
        let mut dot = q1[0] * q2[0] + q1[1] * q2[1] + q1[2] * q2[2] + q1[3] * q2[3];
        
        let q2 = if dot < 0.0 {
            dot = -dot;
            [-q2[0], -q2[1], -q2[2], -q2[3]]
        } else {
            q2
        };

        let result = if dot > 0.9995 {
            // Linear interpolation for very close quaternions
            let q = [
                q1[0] + alpha * (q2[0] - q1[0]),
                q1[1] + alpha * (q2[1] - q1[1]),
                q1[2] + alpha * (q2[2] - q1[2]),
                q1[3] + alpha * (q2[3] - q1[3]),
            ];
            let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
            [q[0] / len, q[1] / len, q[2] / len, q[3] / len]
        } else {
            let theta_0 = dot.acos();
            let theta = theta_0 * alpha;
            let sin_theta = theta.sin();
            let sin_theta_0 = theta_0.sin();
            
            let s0 = (theta_0 - theta).cos() - dot * sin_theta / sin_theta_0;
            let s1 = sin_theta / sin_theta_0;
            
            [
                s0 * q1[0] + s1 * q2[0],
                s0 * q1[1] + s1 * q2[1],
                s0 * q1[2] + s1 * q2[2],
                s0 * q1[3] + s1 * q2[3],
            ]
        };

        let mut cf = Self::from_quaternion(result);
        cf.position = pos;
        cf
    }

    /// Get Euler angles in XYZ order (radians)
    pub fn to_euler_angles_xyz(&self) -> (f64, f64, f64) {
        let m = &self.rotation;
        let sy = -m[2][0];
        
        if sy.abs() < 0.99999 {
            let rx = m[2][1].atan2(m[2][2]);
            let ry = sy.asin();
            let rz = m[1][0].atan2(m[0][0]);
            (rx, ry, rz)
        } else {
            // Gimbal lock
            let rx = (-m[1][2]).atan2(m[1][1]);
            let ry = if sy > 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
            let rz = 0.0;
            (rx, ry, rz)
        }
    }

    /// Get Euler angles in YXZ order (Roblox default)
    pub fn to_euler_angles_yxz(&self) -> (f64, f64, f64) {
        let m = &self.rotation;
        let sx = -m[2][1];
        
        if sx.abs() < 0.99999 {
            let ry = m[2][0].atan2(m[2][2]);
            let rx = sx.asin();
            let rz = m[0][1].atan2(m[1][1]);
            (ry, rx, rz)
        } else {
            // Gimbal lock
            let ry = (-m[0][2]).atan2(m[0][0]);
            let rx = if sx > 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
            let rz = 0.0;
            (ry, rx, rz)
        }
    }

    /// Get rotation as axis-angle
    pub fn to_axis_angle(&self) -> (Vector3, f64) {
        let q = self.to_quaternion();
        let angle = 2.0 * q[3].acos();
        let s = (1.0 - q[3] * q[3]).sqrt();
        
        if s < 0.001 {
            (Vector3::X_AXIS, angle)
        } else {
            (Vector3::new(q[0] / s, q[1] / s, q[2] / s), angle)
        }
    }

    /// Get rotation components (for Roblox compatibility)
    pub fn components(&self) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) {
        (
            self.position.x, self.position.y, self.position.z,
            self.rotation[0][0], self.rotation[1][0], self.rotation[2][0],
            self.rotation[0][1], self.rotation[1][1], self.rotation[2][1],
            self.rotation[0][2], self.rotation[1][2], self.rotation[2][2],
        )
    }
}

impl Mul for CFrame {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        // Matrix multiplication for rotation
        let mut rot = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                rot[i][j] = self.rotation[i][0] * rhs.rotation[0][j]
                          + self.rotation[i][1] * rhs.rotation[1][j]
                          + self.rotation[i][2] * rhs.rotation[2][j];
            }
        }

        // Transform position
        let pos = self.point_to_world_space(rhs.position);

        Self { position: pos, rotation: rot }
    }
}

impl Mul<Vector3> for CFrame {
    type Output = Vector3;
    fn mul(self, rhs: Vector3) -> Vector3 {
        self.point_to_world_space(rhs)
    }
}

impl Add<Vector3> for CFrame {
    type Output = Self;
    fn add(self, rhs: Vector3) -> Self {
        Self {
            position: self.position + rhs,
            rotation: self.rotation,
        }
    }
}

impl Sub<Vector3> for CFrame {
    type Output = Self;
    fn sub(self, rhs: Vector3) -> Self {
        Self {
            position: self.position - rhs,
            rotation: self.rotation,
        }
    }
}

impl std::fmt::Display for CFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}, {}", self.position.x, self.position.y, self.position.z)
    }
}

// ============================================================================
// 4. Color3
// ============================================================================

/// RGB color matching Roblox Color3 API.
/// Components are in 0.0-1.0 range.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color3 {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Color3 {
    pub const WHITE: Color3 = Color3 { r: 1.0, g: 1.0, b: 1.0 };
    pub const BLACK: Color3 = Color3 { r: 0.0, g: 0.0, b: 0.0 };
    pub const RED: Color3 = Color3 { r: 1.0, g: 0.0, b: 0.0 };
    pub const GREEN: Color3 = Color3 { r: 0.0, g: 1.0, b: 0.0 };
    pub const BLUE: Color3 = Color3 { r: 0.0, g: 0.0, b: 1.0 };

    /// Create from 0.0-1.0 float components
    pub fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// Create from 0-255 integer components
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
        }
    }

    /// Create from HSV (hue 0-1, saturation 0-1, value 0-1)
    pub fn from_hsv(h: f64, s: f64, v: f64) -> Self {
        if s <= 0.0 {
            return Self::new(v, v, v);
        }

        let h = (h % 1.0) * 6.0;
        let i = h.floor() as i32;
        let f = h - i as f64;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));

        match i % 6 {
            0 => Self::new(v, t, p),
            1 => Self::new(q, v, p),
            2 => Self::new(p, v, t),
            3 => Self::new(p, q, v),
            4 => Self::new(t, p, v),
            _ => Self::new(v, p, q),
        }
    }

    /// Create from hex string (e.g., "#FF0000" or "FF0000")
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Self::from_rgb(r, g, b))
    }

    /// Convert to HSV (hue 0-1, saturation 0-1, value 0-1)
    pub fn to_hsv(&self) -> (f64, f64, f64) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let delta = max - min;

        let v = max;
        let s = if max > 0.0 { delta / max } else { 0.0 };

        let h = if delta <= 0.0 {
            0.0
        } else if max == self.r {
            ((self.g - self.b) / delta) % 6.0 / 6.0
        } else if max == self.g {
            ((self.b - self.r) / delta + 2.0) / 6.0
        } else {
            ((self.r - self.g) / delta + 4.0) / 6.0
        };

        (if h < 0.0 { h + 1.0 } else { h }, s, v)
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        format!("{:02X}{:02X}{:02X}",
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
        )
    }

    /// Linear interpolation
    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            r: self.r + (goal.r - self.r) * alpha,
            g: self.g + (goal.g - self.g) * alpha,
            b: self.b + (goal.b - self.b) * alpha,
        }
    }

    /// Convert to Bevy Color
    pub fn to_bevy_color(&self) -> bevy::color::Color {
        bevy::color::Color::srgb(self.r as f32, self.g as f32, self.b as f32)
    }

    /// Create from Bevy Color
    pub fn from_bevy_color(color: bevy::color::Color) -> Self {
        let srgba = color.to_srgba();
        Self::new(srgba.red as f64, srgba.green as f64, srgba.blue as f64)
    }

    /// Convert to [u8; 3] array (0-255)
    pub fn to_rgb_u8(&self) -> [u8; 3] {
        [
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
        ]
    }
}

impl std::fmt::Display for Color3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}, {}", self.r, self.g, self.b)
    }
}

// ============================================================================
// 5. UDim / UDim2 — UI Dimensions
// ============================================================================

/// 1D UI dimension (scale + offset)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UDim {
    pub scale: f64,
    pub offset: f64,
}

impl UDim {
    pub fn new(scale: f64, offset: f64) -> Self {
        Self { scale, offset }
    }
}

impl Add for UDim {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            scale: self.scale + rhs.scale,
            offset: self.offset + rhs.offset,
        }
    }
}

impl Sub for UDim {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            scale: self.scale - rhs.scale,
            offset: self.offset - rhs.offset,
        }
    }
}

/// 2D UI dimension (x and y UDims)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UDim2 {
    pub x: UDim,
    pub y: UDim,
}

impl UDim2 {
    pub fn new(x_scale: f64, x_offset: f64, y_scale: f64, y_offset: f64) -> Self {
        Self {
            x: UDim::new(x_scale, x_offset),
            y: UDim::new(y_scale, y_offset),
        }
    }

    pub fn from_scale(x_scale: f64, y_scale: f64) -> Self {
        Self::new(x_scale, 0.0, y_scale, 0.0)
    }

    pub fn from_offset(x_offset: f64, y_offset: f64) -> Self {
        Self::new(0.0, x_offset, 0.0, y_offset)
    }

    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            x: UDim::new(
                self.x.scale + (goal.x.scale - self.x.scale) * alpha,
                self.x.offset + (goal.x.offset - self.x.offset) * alpha,
            ),
            y: UDim::new(
                self.y.scale + (goal.y.scale - self.y.scale) * alpha,
                self.y.offset + (goal.y.offset - self.y.offset) * alpha,
            ),
        }
    }

    /// Width component
    pub fn width(&self) -> UDim { self.x }
    /// Height component
    pub fn height(&self) -> UDim { self.y }
}

impl Add for UDim2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl Sub for UDim2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

// ============================================================================
// 6. Ray
// ============================================================================

/// Ray for raycasting (origin + direction)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vector3,
    pub direction: Vector3,
}

impl Ray {
    pub fn new(origin: Vector3, direction: Vector3) -> Self {
        Self { origin, direction }
    }

    /// Get unit direction
    pub fn unit(&self) -> Self {
        Self {
            origin: self.origin,
            direction: self.direction.unit(),
        }
    }

    /// Get point along ray at distance t
    pub fn point_at(&self, t: f64) -> Vector3 {
        self.origin + self.direction * t
    }

    /// Closest point on ray to a given point
    pub fn closest_point(&self, point: Vector3) -> Vector3 {
        let dir = self.direction.unit();
        let v = point - self.origin;
        let t = v.dot(&dir).max(0.0);
        self.origin + dir * t
    }

    /// Distance from ray to point
    pub fn distance(&self, point: Vector3) -> f64 {
        let closest = self.closest_point(point);
        (point - closest).magnitude()
    }
}

// ============================================================================
// 7. NumberRange
// ============================================================================

/// Number range (min, max)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    pub min: f64,
    pub max: f64,
}

impl NumberRange {
    pub fn new(min: f64, max: f64) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    pub fn single(value: f64) -> Self {
        Self { min: value, max: value }
    }

    /// Check if value is within range
    pub fn contains(&self, value: f64) -> bool {
        value >= self.min && value <= self.max
    }

    /// Clamp value to range
    pub fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }

    /// Random value within range
    pub fn random(&self) -> f64 {
        use rand::Rng;
        rand::thread_rng().gen_range(self.min..=self.max)
    }
}

impl Default for NumberRange {
    fn default() -> Self {
        Self { min: 0.0, max: 1.0 }
    }
}

// ============================================================================
// 8. TweenInfo
// ============================================================================

/// Easing style for tweens
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EasingStyle {
    #[default]
    Linear,
    Sine,
    Back,
    Quad,
    Quart,
    Quint,
    Bounce,
    Elastic,
    Exponential,
    Circular,
    Cubic,
}

/// Easing direction for tweens
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EasingDirection {
    In,
    #[default]
    Out,
    InOut,
}

/// Tween configuration matching Roblox TweenInfo
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenInfo {
    /// Duration in seconds
    pub time: f64,
    /// Easing style
    pub easing_style: EasingStyle,
    /// Easing direction
    pub easing_direction: EasingDirection,
    /// Number of times to repeat (0 = once, -1 = infinite)
    pub repeat_count: i32,
    /// Whether to reverse on each repeat
    pub reverses: bool,
    /// Delay before starting (seconds)
    pub delay_time: f64,
}

impl Default for TweenInfo {
    fn default() -> Self {
        Self {
            time: 1.0,
            easing_style: EasingStyle::default(),
            easing_direction: EasingDirection::default(),
            repeat_count: 0,
            reverses: false,
            delay_time: 0.0,
        }
    }
}

impl TweenInfo {
    pub fn new(
        time: f64,
        easing_style: EasingStyle,
        easing_direction: EasingDirection,
        repeat_count: i32,
        reverses: bool,
        delay_time: f64,
    ) -> Self {
        Self {
            time,
            easing_style,
            easing_direction,
            repeat_count,
            reverses,
            delay_time,
        }
    }

    /// Apply easing function to alpha (0-1)
    pub fn ease(&self, alpha: f64) -> f64 {
        let alpha = alpha.clamp(0.0, 1.0);
        
        let eased = match self.easing_direction {
            EasingDirection::In => self.ease_in(alpha),
            EasingDirection::Out => self.ease_out(alpha),
            EasingDirection::InOut => self.ease_in_out(alpha),
        };
        
        eased
    }

    fn ease_in(&self, t: f64) -> f64 {
        match self.easing_style {
            EasingStyle::Linear => t,
            EasingStyle::Sine => 1.0 - (t * std::f64::consts::FRAC_PI_2).cos(),
            EasingStyle::Quad => t * t,
            EasingStyle::Cubic => t * t * t,
            EasingStyle::Quart => t * t * t * t,
            EasingStyle::Quint => t * t * t * t * t,
            EasingStyle::Exponential => if t == 0.0 { 0.0 } else { 2.0_f64.powf(10.0 * (t - 1.0)) },
            EasingStyle::Circular => 1.0 - (1.0 - t * t).sqrt(),
            EasingStyle::Back => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                c3 * t * t * t - c1 * t * t
            },
            EasingStyle::Elastic => {
                if t == 0.0 || t == 1.0 { t }
                else {
                    let c4 = (2.0 * std::f64::consts::PI) / 3.0;
                    -2.0_f64.powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * c4).sin()
                }
            },
            EasingStyle::Bounce => 1.0 - self.bounce_out(1.0 - t),
        }
    }

    fn ease_out(&self, t: f64) -> f64 {
        match self.easing_style {
            EasingStyle::Linear => t,
            EasingStyle::Sine => (t * std::f64::consts::FRAC_PI_2).sin(),
            EasingStyle::Quad => 1.0 - (1.0 - t) * (1.0 - t),
            EasingStyle::Cubic => 1.0 - (1.0 - t).powi(3),
            EasingStyle::Quart => 1.0 - (1.0 - t).powi(4),
            EasingStyle::Quint => 1.0 - (1.0 - t).powi(5),
            EasingStyle::Exponential => if t == 1.0 { 1.0 } else { 1.0 - 2.0_f64.powf(-10.0 * t) },
            EasingStyle::Circular => (1.0 - (t - 1.0) * (t - 1.0)).sqrt(),
            EasingStyle::Back => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            },
            EasingStyle::Elastic => {
                if t == 0.0 || t == 1.0 { t }
                else {
                    let c4 = (2.0 * std::f64::consts::PI) / 3.0;
                    2.0_f64.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
                }
            },
            EasingStyle::Bounce => self.bounce_out(t),
        }
    }

    fn ease_in_out(&self, t: f64) -> f64 {
        if t < 0.5 {
            self.ease_in(t * 2.0) / 2.0
        } else {
            0.5 + self.ease_out((t - 0.5) * 2.0) / 2.0
        }
    }

    fn bounce_out(&self, t: f64) -> f64 {
        let n1 = 7.5625;
        let d1 = 2.75;

        if t < 1.0 / d1 {
            n1 * t * t
        } else if t < 2.0 / d1 {
            let t = t - 1.5 / d1;
            n1 * t * t + 0.75
        } else if t < 2.5 / d1 {
            let t = t - 2.25 / d1;
            n1 * t * t + 0.9375
        } else {
            let t = t - 2.625 / d1;
            n1 * t * t + 0.984375
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector3_operations() {
        let a = Vector3::new(1.0, 2.0, 3.0);
        let b = Vector3::new(4.0, 5.0, 6.0);
        
        assert_eq!(a + b, Vector3::new(5.0, 7.0, 9.0));
        assert_eq!(a - b, Vector3::new(-3.0, -3.0, -3.0));
        assert_eq!(a * 2.0, Vector3::new(2.0, 4.0, 6.0));
        assert_eq!(a.dot(&b), 32.0);
        assert_eq!(a.cross(&b), Vector3::new(-3.0, 6.0, -3.0));
    }

    #[test]
    fn test_cframe_identity() {
        let cf = CFrame::IDENTITY;
        let point = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(cf * point, point);
    }

    #[test]
    fn test_cframe_translation() {
        let cf = CFrame::new(10.0, 20.0, 30.0);
        let point = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(cf * point, Vector3::new(11.0, 22.0, 33.0));
    }

    #[test]
    fn test_cframe_inverse() {
        let cf = CFrame::new(10.0, 20.0, 30.0) * CFrame::angles(0.5, 0.3, 0.1);
        let inv = cf.inverse();
        let result = cf * inv;
        
        assert!(result.position.fuzzy_eq(&Vector3::ZERO, 1e-10));
    }

    #[test]
    fn test_color3_conversions() {
        let c = Color3::from_rgb(255, 128, 0);
        assert!((c.r - 1.0).abs() < 0.01);
        assert!((c.g - 0.502).abs() < 0.01);
        assert!((c.b - 0.0).abs() < 0.01);

        let hex = Color3::from_hex("#FF8000").unwrap();
        assert_eq!(hex.to_rgb_u8(), [255, 128, 0]);
    }

    #[test]
    fn test_tween_easing() {
        let info = TweenInfo::default();
        assert_eq!(info.ease(0.0), 0.0);
        assert_eq!(info.ease(1.0), 1.0);
        assert!((info.ease(0.5) - 0.5).abs() < 0.01); // Linear
    }
}
