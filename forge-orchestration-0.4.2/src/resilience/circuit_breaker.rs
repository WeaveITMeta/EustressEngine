//! Circuit breaker pattern for failure isolation
//!
//! Prevents cascading failures by temporarily blocking requests to failing services.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tracing::{debug, info, warn};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally
    Closed = 0,
    /// Circuit is open, requests are blocked
    Open = 1,
    /// Circuit is half-open, testing if service recovered
    HalfOpen = 2,
}

impl From<u8> for CircuitState {
    fn from(v: u8) -> Self {
        match v {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Number of successes in half-open to close circuit
    pub success_threshold: u32,
    /// Time to wait before transitioning from open to half-open
    pub reset_timeout: Duration,
    /// Time window for counting failures
    pub failure_window: Duration,
    /// Maximum concurrent requests in half-open state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            reset_timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
            half_open_max_requests: 3,
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a new config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set failure threshold
    pub fn failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold.max(1);
        self
    }

    /// Set success threshold for half-open recovery
    pub fn success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold.max(1);
        self
    }

    /// Set reset timeout
    pub fn reset_timeout(mut self, timeout: Duration) -> Self {
        self.reset_timeout = timeout;
        self
    }

    /// Set failure window
    pub fn failure_window(mut self, window: Duration) -> Self {
        self.failure_window = window;
        self
    }
}

/// Circuit breaker for protecting against cascading failures
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: AtomicU8,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    half_open_requests: AtomicU64,
    last_failure_time: RwLock<Option<Instant>>,
    opened_at: RwLock<Option<Instant>>,
    name: String,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: AtomicU8::new(CircuitState::Closed as u8),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            half_open_requests: AtomicU64::new(0),
            last_failure_time: RwLock::new(None),
            opened_at: RwLock::new(None),
            name: name.into(),
        }
    }

    /// Create with default config
    pub fn with_defaults(name: impl Into<String>) -> Self {
        Self::new(name, CircuitBreakerConfig::default())
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        self.check_state_transition();
        CircuitState::from(self.state.load(Ordering::SeqCst))
    }

    /// Check if request is allowed
    pub fn allow_request(&self) -> bool {
        self.check_state_transition();
        
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                let current = self.half_open_requests.fetch_add(1, Ordering::SeqCst);
                current < self.config.half_open_max_requests as u64
            }
        }
    }

    /// Record a successful request
    pub fn record_success(&self) {
        match self.state() {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold as u64 {
                    self.close();
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Record a failed request
    pub fn record_failure(&self) {
        *self.last_failure_time.write() = Some(Instant::now());

        match self.state() {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold as u64 {
                    self.open();
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open reopens the circuit
                self.open();
            }
            CircuitState::Open => {}
        }
    }

    /// Execute a function with circuit breaker protection
    pub async fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        if !self.allow_request() {
            return Err(CircuitBreakerError::Open);
        }

        match f.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(CircuitBreakerError::ServiceError(e))
            }
        }
    }

    /// Force circuit to open state
    pub fn open(&self) {
        let prev = self.state.swap(CircuitState::Open as u8, Ordering::SeqCst);
        if prev != CircuitState::Open as u8 {
            *self.opened_at.write() = Some(Instant::now());
            self.half_open_requests.store(0, Ordering::SeqCst);
            self.success_count.store(0, Ordering::SeqCst);
            warn!(name = %self.name, "Circuit breaker opened");
        }
    }

    /// Force circuit to closed state
    pub fn close(&self) {
        let prev = self.state.swap(CircuitState::Closed as u8, Ordering::SeqCst);
        if prev != CircuitState::Closed as u8 {
            self.failure_count.store(0, Ordering::SeqCst);
            self.success_count.store(0, Ordering::SeqCst);
            *self.opened_at.write() = None;
            info!(name = %self.name, "Circuit breaker closed");
        }
    }

    /// Transition to half-open state
    fn half_open(&self) {
        let prev = self.state.swap(CircuitState::HalfOpen as u8, Ordering::SeqCst);
        if prev != CircuitState::HalfOpen as u8 {
            self.half_open_requests.store(0, Ordering::SeqCst);
            self.success_count.store(0, Ordering::SeqCst);
            debug!(name = %self.name, "Circuit breaker half-open");
        }
    }

    /// Check and perform state transitions
    fn check_state_transition(&self) {
        if self.state.load(Ordering::SeqCst) == CircuitState::Open as u8 {
            if let Some(opened_at) = *self.opened_at.read() {
                if opened_at.elapsed() >= self.config.reset_timeout {
                    self.half_open();
                }
            }
        }
    }

    /// Get failure count
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::SeqCst)
    }

    /// Get circuit breaker name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reset the circuit breaker
    pub fn reset(&self) {
        self.state.store(CircuitState::Closed as u8, Ordering::SeqCst);
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        self.half_open_requests.store(0, Ordering::SeqCst);
        *self.last_failure_time.write() = None;
        *self.opened_at.write() = None;
        info!(name = %self.name, "Circuit breaker reset");
    }
}

/// Circuit breaker error
#[derive(Debug)]
pub enum CircuitBreakerError<E> {
    /// Circuit is open, request blocked
    Open,
    /// Service returned an error
    ServiceError(E),
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::Open => write!(f, "Circuit breaker is open"),
            CircuitBreakerError::ServiceError(e) => write!(f, "Service error: {}", e),
        }
    }
}

impl<E: std::error::Error> std::error::Error for CircuitBreakerError<E> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_closed() {
        let cb = CircuitBreaker::with_defaults("test");
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig::default().failure_threshold(3);
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_success_resets_failures() {
        let config = CircuitBreakerConfig::default().failure_threshold(3);
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig::default().failure_threshold(2);
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }
}
