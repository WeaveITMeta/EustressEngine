//! Request batching for AI/ML inference
//!
//! Provides dynamic batching to improve throughput for inference workloads.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use tokio::sync::{oneshot, Notify};
use tracing::{debug, info};

/// Configuration for batch processing
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Maximum wait time before processing a partial batch
    pub max_wait_ms: u64,
    /// Minimum batch size to trigger immediate processing
    pub min_batch_size: usize,
    /// Enable dynamic batch sizing based on load
    pub dynamic_sizing: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 32,
            max_wait_ms: 50,
            min_batch_size: 1,
            dynamic_sizing: true,
        }
    }
}

impl BatchConfig {
    /// Create a new batch config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum batch size
    pub fn max_size(mut self, size: usize) -> Self {
        self.max_batch_size = size.max(1);
        self
    }

    /// Set maximum wait time in milliseconds
    pub fn max_wait(mut self, ms: u64) -> Self {
        self.max_wait_ms = ms;
        self
    }

    /// Set minimum batch size for immediate processing
    pub fn min_size(mut self, size: usize) -> Self {
        self.min_batch_size = size.max(1);
        self
    }
}

/// A request in the batch queue
pub struct BatchRequest<T> {
    /// Request payload
    pub payload: T,
    /// Response channel
    response_tx: oneshot::Sender<BatchResult<T>>,
    /// Request arrival time
    arrived_at: Instant,
}

/// Result of a batched request
#[derive(Debug)]
pub struct BatchResult<T> {
    /// Response payload
    pub payload: T,
    /// Batch size this request was processed with
    pub batch_size: usize,
    /// Time spent waiting in queue
    pub queue_time_ms: u64,
    /// Processing time
    pub process_time_ms: u64,
}

/// Batch processor for inference requests
pub struct BatchProcessor<T: Send + 'static> {
    config: BatchConfig,
    queue: Arc<Mutex<VecDeque<BatchRequest<T>>>>,
    notify: Arc<Notify>,
    stats: Arc<Mutex<BatchStats>>,
}

/// Statistics for batch processing
#[derive(Debug, Default, Clone)]
pub struct BatchStats {
    /// Total requests processed
    pub total_requests: u64,
    /// Total batches processed
    pub total_batches: u64,
    /// Average batch size
    pub avg_batch_size: f64,
    /// Average queue time in ms
    pub avg_queue_time_ms: f64,
    /// Average processing time in ms
    pub avg_process_time_ms: f64,
}

impl<T: Send + 'static> BatchProcessor<T> {
    /// Create a new batch processor
    pub fn new(config: BatchConfig) -> Self {
        Self {
            config,
            queue: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
            stats: Arc::new(Mutex::new(BatchStats::default())),
        }
    }

    /// Submit a request and wait for the result
    pub async fn submit(&self, payload: T) -> Result<BatchResult<T>, BatchError> {
        let (tx, rx) = oneshot::channel();
        
        let request = BatchRequest {
            payload,
            response_tx: tx,
            arrived_at: Instant::now(),
        };

        {
            let mut queue = self.queue.lock();
            queue.push_back(request);
            
            // Notify if we've reached max batch size
            if queue.len() >= self.config.max_batch_size {
                self.notify.notify_one();
            }
        }

        // Also notify to start the timer
        self.notify.notify_one();

        rx.await.map_err(|_| BatchError::Cancelled)
    }

    /// Get current queue length
    pub fn queue_len(&self) -> usize {
        self.queue.lock().len()
    }

    /// Get batch statistics
    pub fn stats(&self) -> BatchStats {
        self.stats.lock().clone()
    }

    /// Collect a batch of requests (up to max_batch_size)
    pub fn collect_batch(&self) -> Vec<(T, oneshot::Sender<BatchResult<T>>, Instant)> {
        let mut queue = self.queue.lock();
        let batch_size = queue.len().min(self.config.max_batch_size);
        
        let mut batch = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            if let Some(req) = queue.pop_front() {
                batch.push((req.payload, req.response_tx, req.arrived_at));
            }
        }
        
        batch
    }

    /// Complete a batch with results
    pub fn complete_batch(&self, results: Vec<(T, oneshot::Sender<BatchResult<T>>, Instant, T)>) {
        let batch_size = results.len();
        let process_start = Instant::now();
        
        let mut total_queue_time = 0u64;
        
        for (_, tx, arrived_at, response) in results {
            let queue_time = arrived_at.elapsed().as_millis() as u64;
            total_queue_time += queue_time;
            
            let result = BatchResult {
                payload: response,
                batch_size,
                queue_time_ms: queue_time,
                process_time_ms: process_start.elapsed().as_millis() as u64,
            };
            
            let _ = tx.send(result);
        }

        // Update stats
        let mut stats = self.stats.lock();
        stats.total_requests += batch_size as u64;
        stats.total_batches += 1;
        
        let n = stats.total_batches as f64;
        stats.avg_batch_size = stats.avg_batch_size * (n - 1.0) / n + batch_size as f64 / n;
        stats.avg_queue_time_ms = stats.avg_queue_time_ms * (n - 1.0) / n 
            + (total_queue_time as f64 / batch_size as f64) / n;
        
        debug!(batch_size = batch_size, "Batch completed");
    }

    /// Wait for a batch to be ready
    pub async fn wait_for_batch(&self) -> bool {
        let timeout = Duration::from_millis(self.config.max_wait_ms);
        
        tokio::select! {
            _ = self.notify.notified() => {
                // Check if we have enough for a batch
                self.queue.lock().len() >= self.config.min_batch_size
            }
            _ = tokio::time::sleep(timeout) => {
                // Timeout - process whatever we have
                !self.queue.lock().is_empty()
            }
        }
    }
}

impl<T: Send + 'static> Clone for BatchProcessor<T> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            queue: self.queue.clone(),
            notify: self.notify.clone(),
            stats: self.stats.clone(),
        }
    }
}

/// Batch processing error
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    /// Request was cancelled
    #[error("Request cancelled")]
    Cancelled,
    /// Queue is full
    #[error("Queue full")]
    QueueFull,
    /// Processing failed
    #[error("Processing failed: {0}")]
    ProcessingFailed(String),
}

/// Simple batch collector for manual batch processing
pub struct BatchCollector<T> {
    items: Vec<T>,
    max_size: usize,
    created_at: Instant,
    max_wait: Duration,
}

impl<T> BatchCollector<T> {
    /// Create a new batch collector
    pub fn new(max_size: usize, max_wait_ms: u64) -> Self {
        Self {
            items: Vec::with_capacity(max_size),
            max_size,
            created_at: Instant::now(),
            max_wait: Duration::from_millis(max_wait_ms),
        }
    }

    /// Add an item to the batch
    pub fn add(&mut self, item: T) -> bool {
        if self.items.len() < self.max_size {
            self.items.push(item);
            true
        } else {
            false
        }
    }

    /// Check if batch is ready (full or timeout)
    pub fn is_ready(&self) -> bool {
        self.items.len() >= self.max_size || self.created_at.elapsed() >= self.max_wait
    }

    /// Check if batch is full
    pub fn is_full(&self) -> bool {
        self.items.len() >= self.max_size
    }

    /// Get current batch size
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Take the collected items
    pub fn take(self) -> Vec<T> {
        self.items
    }

    /// Time since batch was created
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_collector() {
        let mut collector: BatchCollector<i32> = BatchCollector::new(3, 100);
        
        assert!(collector.add(1));
        assert!(collector.add(2));
        assert!(!collector.is_full());
        assert!(collector.add(3));
        assert!(collector.is_full());
        assert!(!collector.add(4)); // Should fail, batch is full
        
        let items = collector.take();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_batch_processor_stats() {
        let processor: BatchProcessor<String> = BatchProcessor::new(BatchConfig::default());
        
        let stats = processor.stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_batches, 0);
    }
}
