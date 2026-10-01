//! Rigid poses (a translation and a rotation, no scale) for writing each
//! instance's `[transform]` relative to its parent's pose, the `ParentPose`
//! rule a Space marks in its `space.toml`.
//!
//! Roblox gives a part's CFrame in world space and an Attachment's relative
//! to its part. The importer keeps, for every node it writes, two world
//! poses: the one Roblox means (`roblox`) and the one the loader will
//! compose from the files (`written`, which includes a cylinder part's axis
//! correction). A node's `[transform]` is its written world relative to its
//! parent's written world, so a reader that composes parent by parent lands
//! every node where Roblox had it.

/// A translation (studs) and a unit quaternion `[x, y, z, w]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose {
    pub t: [f32; 3],
    pub r: [f32; 4],
}

impl Pose {
    pub const IDENTITY: Pose = Pose { t: [0.0; 3], r: [0.0, 0.0, 0.0, 1.0] };

    /// `self` followed by `local`: the world pose of a child whose pose
    /// relative to `self` is `local`.
    pub fn compose(&self, local: &Pose) -> Pose {
        let rt = rotate(self.r, local.t);
        Pose {
            t: [self.t[0] + rt[0], self.t[1] + rt[1], self.t[2] + rt[2]],
            r: normalize(mul(self.r, local.r)),
        }
    }

    pub fn inverse(&self) -> Pose {
        let ri = conjugate(self.r);
        let t = rotate(ri, [-self.t[0], -self.t[1], -self.t[2]]);
        Pose { t, r: ri }
    }

    /// `world` relative to `self`.
    pub fn relative(&self, world: &Pose) -> Pose {
        self.inverse().compose(world)
    }
}

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn conjugate(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

fn normalize(q: [f32; 4]) -> [f32; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 1e-9 { [q[0] / n, q[1] / n, q[2] / n, q[3] / n] } else { [0.0, 0.0, 0.0, 1.0] }
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let p = mul(mul(q, [v[0], v[1], v[2], 0.0]), conjugate(q));
    [p[0], p[1], p[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn relative_then_compose_is_the_identity() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let parent = Pose { t: [10.0, 2.0, -3.0], r: [0.0, h, 0.0, h] }; // 90 degrees about Y
        let world = Pose { t: [10.0, 2.0, 2.0], r: [0.0, 0.0, 0.0, 1.0] };
        let local = parent.relative(&world);
        // 5 studs along world +Z is 5 along the parent's local -X after a
        // +90 degree turn about Y.
        assert!(close(local.t, [-5.0, 0.0, 0.0]), "{:?}", local.t);
        let back = parent.compose(&local);
        assert!(close(back.t, world.t));
        assert!((0..4).all(|i| (back.r[i] - world.r[i]).abs() < 1e-5 || (back.r[i] + world.r[i]).abs() < 1e-5));
    }
}
