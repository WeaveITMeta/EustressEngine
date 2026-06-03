//! Statistical distributions and regression.
pub mod distributions;
pub mod regression;
pub mod prelude {
    pub use super::distributions::*;
    pub use super::regression::*;
}
