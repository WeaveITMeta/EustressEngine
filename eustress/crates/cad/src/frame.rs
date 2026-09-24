//! Placement frames: where a sketch lives in 3D.
//!
//! Every sketch coordinate is a point `(u, v)` in a right-handed frame
//! `origin + u·x + v·y`, with `z = x × y` as the sketch normal. Extrude
//! goes along `z`, a hole drills along `-z`, and a revolve happens in
//! the frame's plane.
//!
//! Before this module, `Sketch.plane` was parsed and never read: a
//! sketch declared on `"xz"` extruded along +Z from the XY plane,
//! exactly as if it had said `"xy"`. Nothing reported the mismatch, so
//! the part simply came out lying the wrong way.
//!
//! ## Built-in planes
//!
//! | name | x | y | normal | note |
//! |------|---|---|--------|------|
//! | `xy` | +X | +Y | +Z | the identity frame; every template uses it |
//! | `xz` | +X | -Z | +Y | the ground plane in a Y-up world, so it extrudes UP |
//! | `yz` | +Y | +Z | +X | |
//!
//! `xz` follows the convention of mainstream Y-up CAD (the "top" plane):
//! its second axis runs along -Z so that the normal comes out +Y. The
//! engine is Y-up, so a part sketched on `xz` stands on the floor rather
//! than lying against a wall. The normals also agree with the ones
//! Mirror and Split have always used for these names.
//!
//! A plane can also be a named reference plane (a `ReferencePlane`
//! feature), or a planar face of the model by its topological name
//! (`"Extrude1.cap_end"`), in which case the frame is resolved against
//! the geometry that exists at that point in the tree.

use truck_modeling::*;

/// A right-handed orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: Point3,
    pub x: Vector3,
    pub y: Vector3,
    /// Normal: always `x × y`.
    pub z: Vector3,
}

impl Frame {
    /// The identity frame: sketch `(u, v)` is world `(u, v, 0)`.
    pub fn xy() -> Self {
        Self {
            origin: Point3::origin(),
            x: Vector3::unit_x(),
            y: Vector3::unit_y(),
            z: Vector3::unit_z(),
        }
    }

    /// The Y-up ground plane: normal +Y, so an extrusion rises.
    pub fn xz() -> Self {
        Self {
            origin: Point3::origin(),
            x: Vector3::unit_x(),
            y: -Vector3::unit_z(),
            z: Vector3::unit_y(),
        }
    }

    pub fn yz() -> Self {
        Self {
            origin: Point3::origin(),
            x: Vector3::unit_y(),
            y: Vector3::unit_z(),
            z: Vector3::unit_x(),
        }
    }

    /// Resolve one of the three built-in plane names.
    pub fn builtin(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "xy" | "world/xy" => Some(Self::xy()),
            "xz" | "world/xz" => Some(Self::xz()),
            "yz" | "world/yz" => Some(Self::yz()),
            _ => None,
        }
    }

    /// Frame on a plane given by a point and a normal.
    ///
    /// The in-plane x axis is the world X axis projected into the plane,
    /// or world Y when X is nearly parallel to the normal. Deriving the
    /// axes from the world rather than from the face that happened to
    /// produce the plane means a sketch on a face keeps the same
    /// orientation when an upstream edit rebuilds that face.
    pub fn from_origin_normal(origin: Point3, normal: Vector3) -> Option<Self> {
        let len = normal.magnitude();
        if !(len > 1.0e-12) {
            return None;
        }
        let z = normal / len;
        let hint = if z.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        let x = (hint - z * hint.dot(z)).normalize();
        let y = z.cross(x);
        Some(Self { origin, x, y, z })
    }

    /// Frame through three points: `p0` is the origin, `p1` sets the x
    /// direction, and `p2` picks the side the y axis points to.
    pub fn from_three_points(p0: Point3, p1: Point3, p2: Point3) -> Option<Self> {
        let a = p1 - p0;
        let b = p2 - p0;
        let n = a.cross(b);
        if !(n.magnitude() > 1.0e-12) || !(a.magnitude() > 1.0e-12) {
            return None;
        }
        let x = a.normalize();
        let z = n.normalize();
        let y = z.cross(x);
        Some(Self { origin: p0, x, y, z })
    }

    /// Sketch point to world point, `w` along the normal.
    #[inline]
    pub fn point(&self, u: f64, v: f64, w: f64) -> Point3 {
        self.origin + self.x * u + self.y * v + self.z * w
    }

    /// Sketch point `[u, v]` on the plane itself.
    #[inline]
    pub fn to_world(&self, p: [f64; 2]) -> Point3 {
        self.point(p[0], p[1], 0.0)
    }

    /// World point to `[u, v, w]` in this frame.
    #[inline]
    pub fn to_local(&self, p: Point3) -> [f64; 3] {
        let d = p - self.origin;
        [d.dot(self.x), d.dot(self.y), d.dot(self.z)]
    }

    /// Signed distance of `p` from the plane, positive on the normal side.
    #[inline]
    pub fn signed_distance(&self, p: Point3) -> f64 {
        (p - self.origin).dot(self.z)
    }

    /// The same frame moved `d` along its normal.
    #[inline]
    pub fn offset(&self, d: f64) -> Self {
        Self {
            origin: self.origin + self.z * d,
            ..*self
        }
    }

    /// The same plane seen from the other side: normal and y flipped,
    /// x kept, so the frame stays right-handed.
    #[inline]
    pub fn reversed(&self) -> Self {
        Self {
            origin: self.origin,
            x: self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    /// Local-to-world matrix: columns are x, y, z and the origin.
    pub fn local_to_world(&self) -> Matrix4 {
        Matrix4::from_cols(
            self.x.extend(0.0),
            self.y.extend(0.0),
            self.z.extend(0.0),
            self.origin.to_vec().extend(1.0),
        )
    }

    /// Is this frame's plane the same geometric plane as `other`'s,
    /// regardless of which way either normal points?
    pub fn coplanar_with(&self, other: &Frame, tol: f64) -> bool {
        let parallel = self.z.cross(other.z).magnitude() < 1.0e-9;
        parallel && self.signed_distance(other.origin).abs() < tol
    }
}

/// Reflection about the plane of `frame`, as a 4x4 matrix.
///
/// Determinant -1: callers that apply it to a solid must invert the
/// solid's faces afterwards (see `mirror` in the evaluator).
pub fn reflection_about(frame: &Frame) -> Matrix4 {
    let n = frame.z;
    let o = frame.origin;
    let r00 = 1.0 - 2.0 * n.x * n.x;
    let r01 = -2.0 * n.x * n.y;
    let r02 = -2.0 * n.x * n.z;
    let r11 = 1.0 - 2.0 * n.y * n.y;
    let r12 = -2.0 * n.y * n.z;
    let r22 = 1.0 - 2.0 * n.z * n.z;
    // t = o - R·o, so points on the plane are fixed.
    let tx = o.x - (r00 * o.x + r01 * o.y + r02 * o.z);
    let ty = o.y - (r01 * o.x + r11 * o.y + r12 * o.z);
    let tz = o.z - (r02 * o.x + r12 * o.y + r22 * o.z);
    // cgmath matrices are column-major: each group of four is a column.
    Matrix4::new(
        r00, r01, r02, 0.0, //
        r01, r11, r12, 0.0, //
        r02, r12, r22, 0.0, //
        tx, ty, tz, 1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vector3, b: Vector3) -> bool {
        (a - b).magnitude() < 1.0e-12
    }

    #[test]
    fn builtin_frames_are_right_handed() {
        for f in [Frame::xy(), Frame::xz(), Frame::yz()] {
            assert!(near(f.x.cross(f.y), f.z), "{f:?}");
        }
    }

    #[test]
    fn xz_extrudes_up_in_a_y_up_world() {
        assert!(near(Frame::xz().z, Vector3::unit_y()));
        // Sketch (u, v) = (1, 2) lands at world (1, 0, -2).
        let p = Frame::xz().to_world([1.0, 2.0]);
        assert!((p - Point3::new(1.0, 0.0, -2.0)).magnitude() < 1.0e-12);
    }

    #[test]
    fn local_round_trips() {
        let f = Frame::from_origin_normal(Point3::new(1.0, 2.0, 3.0), Vector3::new(1.0, 1.0, 0.0))
            .unwrap();
        let p = f.point(0.3, -0.7, 0.25);
        let l = f.to_local(p);
        assert!((l[0] - 0.3).abs() < 1e-12 && (l[1] + 0.7).abs() < 1e-12 && (l[2] - 0.25).abs() < 1e-12);
        assert!(near(f.x.cross(f.y), f.z));
    }

    #[test]
    fn reflection_fixes_the_plane_and_flips_the_normal() {
        let f = Frame::from_origin_normal(Point3::new(0.0, 0.0, 0.5), Vector3::unit_z()).unwrap();
        let m = reflection_about(&f);
        let on_plane = Point3::new(0.2, -0.4, 0.5);
        assert!((m.transform_point(on_plane) - on_plane).magnitude() < 1e-12);
        let above = Point3::new(0.0, 0.0, 1.5);
        assert!((m.transform_point(above) - Point3::new(0.0, 0.0, -0.5)).magnitude() < 1e-12);
        assert!(m.determinant() < 0.0);
    }

    #[test]
    fn local_to_world_matches_point() {
        let f = Frame::xz().offset(0.25);
        let m = f.local_to_world();
        let a = m.transform_point(Point3::new(0.1, 0.2, 0.3));
        let b = f.point(0.1, 0.2, 0.3);
        assert!((a - b).magnitude() < 1e-12);
    }
}
