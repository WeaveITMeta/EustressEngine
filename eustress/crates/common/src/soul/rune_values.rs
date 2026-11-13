//! # `eustress::Vector3`: the Rune value type every shell shares
//!
//! One type, so a vector `eustress::dm` returns ([`super::rune_datamodel`])
//! is the vector a script builds with `Vector3::new`. Studio's ECS module
//! (`engine::soul::rune_ecs_module`) re-exports it and registers it through
//! [`install_vector3`]; a shell without that module (the Player) registers it
//! through [`create_values_module`]. Both go through the same list, so the
//! type has the same functions everywhere.
//!
//! The type carries `#[rune(item = ::eustress)]`: a type registered into a
//! namespaced module without it leaves its associated functions unreachable
//! (`Missing item ::eustress::Vector3::new`). See `create_ecs_module`'s docs.

use rune::{ContextError, Module};

/// 3D vector matching Roblox Vector3 API.
#[derive(Debug, Clone, Copy, rune::Any)]
#[rune(item = ::eustress)]
pub struct Vector3 {
    #[rune(get, set)]
    pub x: f64,
    #[rune(get, set)]
    pub y: f64,
    #[rune(get, set)]
    pub z: f64,
}

// Implement TryClone for Rune 0.14 compatibility
impl rune::alloc::clone::TryClone for Vector3 {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

impl Vector3 {
    pub const ZERO: Vector3 = Vector3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Vector3 = Vector3 { x: 1.0, y: 1.0, z: 1.0 };

    /// Rust-side constructor (not wrapped by rune macro)
    pub fn create(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[rune::function(path = Self::new)]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[rune::function(instance)]
    pub fn magnitude(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    #[rune::function(instance, path = Self::unit)]
    pub fn unit(&self) -> Self {
        let mag = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if mag > 0.0 {
            Self { x: self.x / mag, y: self.y / mag, z: self.z / mag }
        } else {
            Self::ZERO
        }
    }

    #[rune::function(instance)]
    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[rune::function(instance)]
    pub fn cross(&self, other: &Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    #[rune::function(instance)]
    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            x: self.x + (goal.x - self.x) * alpha,
            y: self.y + (goal.y - self.y) * alpha,
            z: self.z + (goal.z - self.z) * alpha,
        }
    }

    #[rune::function(instance)]
    pub fn add(&self, other: &Self) -> Self {
        Self { x: self.x + other.x, y: self.y + other.y, z: self.z + other.z }
    }

    #[rune::function(instance)]
    pub fn sub(&self, other: &Self) -> Self {
        Self { x: self.x - other.x, y: self.y - other.y, z: self.z - other.z }
    }

    #[rune::function(instance)]
    pub fn mul(&self, scalar: f64) -> Self {
        Self { x: self.x * scalar, y: self.y * scalar, z: self.z * scalar }
    }

    #[rune::function(instance)]
    pub fn div(&self, scalar: f64) -> Self {
        Self { x: self.x / scalar, y: self.y / scalar, z: self.z / scalar }
    }

    #[rune::function(instance)]
    pub fn neg(&self) -> Self {
        Self { x: -self.x, y: -self.y, z: -self.z }
    }

    /// The vector as `f32`s, for the engine's spatial queries.
    pub fn to_array(&self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

/// Register [`Vector3`] and every function it has into `module`, which must
/// be a module of the `eustress` crate.
///
/// `module.ty::<T>()` alone installs only the field protocol; each
/// `#[rune::function]` has to be registered by hand, or a script calling it
/// fails with "Missing instance function". Keep this list exhaustive.
pub fn install_vector3(module: &mut Module) -> Result<(), ContextError> {
    module.ty::<Vector3>()?;
    module.function_meta(Vector3::new)?;
    module.function_meta(Vector3::magnitude)?;
    module.function_meta(Vector3::unit)?;
    module.function_meta(Vector3::dot)?;
    module.function_meta(Vector3::cross)?;
    module.function_meta(Vector3::lerp)?;
    module.function_meta(Vector3::add)?;
    module.function_meta(Vector3::sub)?;
    module.function_meta(Vector3::mul)?;
    module.function_meta(Vector3::div)?;
    module.function_meta(Vector3::neg)?;
    Ok(())
}

/// `eustress::Vector3` on its own, for a shell that does not install Studio's
/// ECS module (which registers it itself). Installing both in one context
/// would register the type twice.
pub fn create_values_module() -> Result<Module, ContextError> {
    let mut module = Module::with_crate("eustress")?;
    install_vector3(&mut module)?;
    Ok(module)
}
