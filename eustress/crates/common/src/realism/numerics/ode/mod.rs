//! ODE integrators — Euler, RK4, RK45, Velocity Verlet, BDF.
pub mod euler;
pub mod runge_kutta;
pub mod verlet;
pub mod implicit;
pub mod prelude {
    pub use super::euler::*;
    pub use super::runge_kutta::*;
    pub use super::verlet::*;
    pub use super::implicit::*;
}