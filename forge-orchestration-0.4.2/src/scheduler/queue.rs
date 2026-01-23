//! Scheduling queue with priority ordering
//!
//! Implements a priority queue for pending workloads with:
//! - Priority-based ordering
//! - FIFO within same priority
//! - Backoff for repeatedly failing workloads

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use parking_lot::Mutex;
use super::Workload;

/// Queued workload wrapper for priority ordering
#[derive(Debug)]
pub struct QueuedWorkload {
    /// The workload
    pub workload: Workload,
    /// Queue position (for FIFO within priority)
    pub sequence: u64,
    /// Number of scheduling attempts
    pub attempts: u32,
    /// Backoff until this time
    pub backoff_until: Option<chrono::DateTime<chrono::Utc>>,
}

impl QueuedWorkload {
    /// Create new queued workload
    pub fn new(workload: Workload, sequence: u64) -> Self {
        Self {
            workload,
            sequence,
            attempts: 0,
            backoff_until: None,
        }
    }

    /// Increment attempts and set backoff
    pub fn record_failure(&mut self) {
        self.attempts += 1;
        // Exponential backoff: 1s, 2s, 4s, 8s, ... up to 5 minutes
        let backoff_secs = (2_i64.pow(self.attempts.min(8)) as i64).min(300);
        self.backoff_until = Some(chrono::Utc::now() + chrono::Duration::seconds(backoff_secs));
    }

    /// Check if workload is ready to be scheduled
    pub fn is_ready(&self) -> bool {
        self.backoff_until
            .map(|t| chrono::Utc::now() >= t)
            .unwrap_or(true)
    }
}

impl PartialEq for QueuedWorkload {
    fn eq(&self, other: &Self) -> bool {
        self.workload.id == other.workload.id
    }
}

impl Eq for QueuedWorkload {}

impl PartialOrd for QueuedWorkload {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedWorkload {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first
        match self.workload.priority.cmp(&other.workload.priority) {
            Ordering::Equal => {
                // Lower sequence (earlier) first for FIFO
                other.sequence.cmp(&self.sequence)
            }
            other => other,
        }
    }
}

/// Scheduling queue
pub struct SchedulingQueue {
    /// Priority queue of pending workloads
    queue: Mutex<BinaryHeap<QueuedWorkload>>,
    /// Sequence counter for FIFO ordering
    sequence: Mutex<u64>,
}

impl SchedulingQueue {
    /// Create new scheduling queue
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(BinaryHeap::new()),
            sequence: Mutex::new(0),
        }
    }

    /// Add workload to queue
    pub fn enqueue(&self, workload: Workload) {
        let mut seq = self.sequence.lock();
        let sequence = *seq;
        *seq += 1;
        drop(seq);

        let queued = QueuedWorkload::new(workload, sequence);
        self.queue.lock().push(queued);
    }

    /// Get next workload to schedule
    pub fn dequeue(&self) -> Option<Workload> {
        let mut queue = self.queue.lock();
        
        // Find first ready workload
        let mut not_ready = Vec::new();
        
        while let Some(queued) = queue.pop() {
            if queued.is_ready() {
                // Put back the not-ready ones
                for q in not_ready {
                    queue.push(q);
                }
                return Some(queued.workload);
            } else {
                not_ready.push(queued);
            }
        }

        // Put back all not-ready workloads
        for q in not_ready {
            queue.push(q);
        }

        None
    }

    /// Peek at next workload without removing
    pub fn peek(&self) -> Option<String> {
        self.queue.lock().peek().map(|q| q.workload.id.clone())
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.queue.lock().len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    /// Remove workload from queue
    pub fn remove(&self, workload_id: &str) -> bool {
        let mut queue = self.queue.lock();
        let items: Vec<_> = std::mem::take(&mut *queue).into_vec();
        let mut found = false;
        
        for item in items {
            if item.workload.id == workload_id {
                found = true;
            } else {
                queue.push(item);
            }
        }

        found
    }

    /// Re-queue a workload with backoff
    pub fn requeue_with_backoff(&self, mut queued: QueuedWorkload) {
        queued.record_failure();
        self.queue.lock().push(queued);
    }
}

impl Default for SchedulingQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let queue = SchedulingQueue::new();

        queue.enqueue(Workload::new("low", "low").with_priority(10));
        queue.enqueue(Workload::new("high", "high").with_priority(100));
        queue.enqueue(Workload::new("medium", "medium").with_priority(50));

        assert_eq!(queue.dequeue().unwrap().id, "high");
        assert_eq!(queue.dequeue().unwrap().id, "medium");
        assert_eq!(queue.dequeue().unwrap().id, "low");
    }

    #[test]
    fn test_fifo_within_priority() {
        let queue = SchedulingQueue::new();

        queue.enqueue(Workload::new("first", "first").with_priority(50));
        queue.enqueue(Workload::new("second", "second").with_priority(50));
        queue.enqueue(Workload::new("third", "third").with_priority(50));

        assert_eq!(queue.dequeue().unwrap().id, "first");
        assert_eq!(queue.dequeue().unwrap().id, "second");
        assert_eq!(queue.dequeue().unwrap().id, "third");
    }

    #[test]
    fn test_remove() {
        let queue = SchedulingQueue::new();

        queue.enqueue(Workload::new("w1", "w1"));
        queue.enqueue(Workload::new("w2", "w2"));
        queue.enqueue(Workload::new("w3", "w3"));

        assert!(queue.remove("w2"));
        assert_eq!(queue.len(), 2);
        assert!(!queue.remove("w2")); // Already removed
    }
}
