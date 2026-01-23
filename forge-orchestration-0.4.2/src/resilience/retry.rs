//! Retry policies for transient failures
//!
//! Provides configurable retry strategies with exponential backoff.

use std::time::Duration;
use tracing::debug;

/// Retry policy configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub multiplier: f64,
    /// Add jitter to delays
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Create a new retry config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum retries
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set initial delay
    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set maximum delay
    pub fn max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set backoff multiplier
    pub fn multiplier(mut self, mult: f64) -> Self {
        self.multiplier = mult.max(1.0);
        self
    }

    /// Enable/disable jitter
    pub fn jitter(mut self, enabled: bool) -> Self {
        self.jitter = enabled;
        self
    }
}

/// Exponential backoff calculator
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    config: RetryConfig,
    attempt: u32,
}

impl ExponentialBackoff {
    /// Create a new exponential backoff
    pub fn new(config: RetryConfig) -> Self {
        Self { config, attempt: 0 }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(RetryConfig::default())
    }

    /// Get the next delay, or None if max retries exceeded
    pub fn next_delay(&mut self) -> Option<Duration> {
        if self.attempt >= self.config.max_retries {
            return None;
        }

        let delay = self.calculate_delay();
        self.attempt += 1;
        Some(delay)
    }

    /// Calculate delay for current attempt
    fn calculate_delay(&self) -> Duration {
        let base_delay = self.config.initial_delay.as_millis() as f64;
        let multiplied = base_delay * self.config.multiplier.powi(self.attempt as i32);
        let capped = multiplied.min(self.config.max_delay.as_millis() as f64);

        let delay_ms = if self.config.jitter {
            // Add up to 25% jitter
            let jitter = capped * 0.25 * rand_jitter();
            capped + jitter
        } else {
            capped
        };

        Duration::from_millis(delay_ms as u64)
    }

    /// Reset the backoff
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Get current attempt number
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Check if more retries are available
    pub fn has_more(&self) -> bool {
        self.attempt < self.config.max_retries
    }
}

/// Simple pseudo-random jitter (0.0 to 1.0)
fn rand_jitter() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos % 1000) as f64 / 1000.0
}

/// Retry policy for executing operations with retries
pub struct RetryPolicy {
    config: RetryConfig,
}

impl RetryPolicy {
    /// Create a new retry policy
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(RetryConfig::default())
    }

    /// Execute an async operation with retries
    pub async fn execute<F, Fut, T, E>(&self, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut backoff = ExponentialBackoff::new(self.config.clone());

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if let Some(delay) = backoff.next_delay() {
                        debug!(
                            attempt = backoff.attempt(),
                            delay_ms = delay.as_millis(),
                            error = ?e,
                            "Retrying after failure"
                        );
                        tokio::time::sleep(delay).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Execute with a custom retry condition
    pub async fn execute_if<F, Fut, T, E, C>(&self, mut operation: F, should_retry: C) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
        C: Fn(&E) -> bool,
    {
        let mut backoff = ExponentialBackoff::new(self.config.clone());

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if should_retry(&e) {
                        if let Some(delay) = backoff.next_delay() {
                            debug!(
                                attempt = backoff.attempt(),
                                delay_ms = delay.as_millis(),
                                error = ?e,
                                "Retrying after retryable failure"
                            );
                            tokio::time::sleep(delay).await;
                        } else {
                            return Err(e);
                        }
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff() {
        let config = RetryConfig::default()
            .max_retries(3)
            .initial_delay(Duration::from_millis(100))
            .multiplier(2.0)
            .jitter(false);

        let mut backoff = ExponentialBackoff::new(config);

        let d1 = backoff.next_delay().unwrap();
        assert_eq!(d1, Duration::from_millis(100));

        let d2 = backoff.next_delay().unwrap();
        assert_eq!(d2, Duration::from_millis(200));

        let d3 = backoff.next_delay().unwrap();
        assert_eq!(d3, Duration::from_millis(400));

        assert!(backoff.next_delay().is_none());
    }

    #[test]
    fn test_backoff_max_delay() {
        let config = RetryConfig::default()
            .max_retries(10)
            .initial_delay(Duration::from_secs(1))
            .max_delay(Duration::from_secs(5))
            .multiplier(2.0)
            .jitter(false);

        let mut backoff = ExponentialBackoff::new(config);

        // Skip first few
        for _ in 0..5 {
            backoff.next_delay();
        }

        // Should be capped at max_delay
        let delay = backoff.next_delay().unwrap();
        assert!(delay <= Duration::from_secs(5));
    }

    #[test]
    fn test_backoff_reset() {
        let config = RetryConfig::default().max_retries(2);
        let mut backoff = ExponentialBackoff::new(config);

        backoff.next_delay();
        backoff.next_delay();
        assert!(backoff.next_delay().is_none());

        backoff.reset();
        assert!(backoff.next_delay().is_some());
    }

    #[tokio::test]
    async fn test_retry_policy_success() {
        let policy = RetryPolicy::new(RetryConfig::default().max_retries(3));
        let mut attempts = 0;

        let result: Result<i32, &str> = policy.execute(|| {
            attempts += 1;
            async move {
                if attempts < 2 {
                    Err("fail")
                } else {
                    Ok(42)
                }
            }
        }).await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts, 2);
    }
}
