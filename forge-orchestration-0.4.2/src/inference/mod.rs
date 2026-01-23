//! AI/ML inference utilities for Forge
//!
//! Provides request batching, streaming responses, and inference optimization.

pub mod batch;
pub mod streaming;

pub use batch::{BatchConfig, BatchProcessor, BatchRequest, BatchResult};
pub use streaming::{StreamingResponse, StreamingConfig, StreamEvent};
