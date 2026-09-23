//! The SPH smoothing kernel.

/// The cubic spline kernel (Monaghan 1992) in three dimensions, with its
/// normalisation hoisted out of the loop for solvers that evaluate it
/// millions of times per step with one support radius.
#[derive(Clone, Copy, Debug)]
pub struct CubicSpline {
    /// Support radius (the kernel is zero beyond it).
    pub h: f32,
    inv_h: f32,
    sigma: f32,
    grad_sigma: f32,
}

impl CubicSpline {
    pub fn new(h: f32) -> Self {
        let h = h.max(f32::MIN_POSITIVE);
        Self {
            h,
            inv_h: 1.0 / h,
            sigma: 8.0 / (std::f32::consts::PI * h * h * h),
            grad_sigma: 48.0 / (std::f32::consts::PI * h * h * h * h),
        }
    }

    /// Kernel value W(r).
    #[inline]
    pub fn w(&self, r: f32) -> f32 {
        let q = r * self.inv_h;
        if q <= 0.5 {
            self.sigma * (6.0 * (q * q * q - q * q) + 1.0)
        } else if q <= 1.0 {
            let t = 1.0 - q;
            self.sigma * 2.0 * t * t * t
        } else {
            0.0
        }
    }

    /// Radial derivative dW/dr (non-positive inside the support).
    #[inline]
    pub fn dw(&self, r: f32) -> f32 {
        let q = r * self.inv_h;
        if q <= 0.5 {
            self.grad_sigma * q * (3.0 * q - 2.0)
        } else if q <= 1.0 {
            let t = 1.0 - q;
            -self.grad_sigma * t * t
        } else {
            0.0
        }
    }

    /// Laplacian W'' + 2 W'/r.
    #[inline]
    pub fn laplacian(&self, r: f32) -> f32 {
        let q = r * self.inv_h;
        let s = self.sigma * self.inv_h * self.inv_h;
        if q <= 0.5 {
            s * (72.0 * q - 36.0)
        } else if q <= 1.0 {
            s * 12.0 * (1.0 - q) * (2.0 * q - 1.0) / q
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_integrates_to_one() {
        let h = 0.5;
        let k = CubicSpline::new(h);
        let n = 4000;
        let dr = h / n as f32;
        let mut total = 0.0f64;
        for i in 0..n {
            let r = (i as f32 + 0.5) * dr;
            total += (4.0 * std::f32::consts::PI * r * r * k.w(r) * dr) as f64;
        }
        assert!((total - 1.0).abs() < 1e-3, "integral {total}");
    }

    #[test]
    fn derivative_matches_finite_difference() {
        let h = 0.037;
        let k = CubicSpline::new(h);
        let dr = 1e-3 * h;
        for i in 1..40 {
            let r = h * i as f32 / 40.0;
            let fd = (k.w(r + dr) - k.w(r - dr)) / (2.0 * dr);
            assert!((k.dw(r) - fd).abs() <= 2e-3 * k.dw(0.5 * h).abs(), "r {r}: dw {} vs {fd}", k.dw(r));
        }
    }
}
