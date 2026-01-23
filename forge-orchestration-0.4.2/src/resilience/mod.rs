//! Resilience patterns for distributed systems
//!
//! Provides circuit breaker, retry, and rate limiting patterns.

pub mod circuit_breaker;
pub mod retry;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use retry::{RetryPolicy, RetryConfig, ExponentialBackoff};
