//! `ForgeCluster` — manages N EustressStream nodes with consistent-hash topic routing.
//!
//! Port allocation: base_port, base_port+1, …, base_port+N-1 (default range 33000–49151).
//! Topic → node routing uses a consistent hash ring with 150 virtual nodes per physical node.

use std::net::SocketAddr;
use std::sync::Arc;

use ahash::AHasher;
use std::hash::{Hash, Hasher};
use tracing::info;

use eustress_stream::EustressStream;

use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::server::NodeServer;

// ─────────────────────────────────────────────────────────────────────────────
// ForgeCluster
// ─────────────────────────────────────────────────────────────────────────────

/// A cluster of N EustressStream TCP nodes with consistent-hash topic routing.
///
/// Default configuration: 10 nodes on ports 33000–33009.
pub struct ForgeCluster {
    /// Physical nodes, indexed 0..N.
    nodes: Vec<Arc<NodeServer>>,
    /// Sorted consistent hash ring: `(hash_point, node_index)`.
    ring: Vec<(u64, usize)>,
}

/// Per-cluster statistics.
#[derive(Debug, Clone)]
pub struct ClusterStats {
    pub node_count: usize,
    pub total_topics: usize,
    pub total_subscribers: usize,
    pub nodes: Vec<NodeStats>,
}

#[derive(Debug, Clone)]
pub struct NodeStats {
    pub port: u16,
    pub listen_addr: SocketAddr,
    pub topic_count: usize,
}

impl ForgeCluster {
    /// Start N nodes on consecutive ports starting from `base_port`.
    pub async fn start(base_port: u16, count: usize, base_config: NodeConfig) -> Result<Self, NodeError> {
        if count == 0 {
            return Err(NodeError::ClusterError("cluster size must be at least 1".to_string()));
        }

        let mut nodes = Vec::with_capacity(count);

        for i in 0..count {
            let port = base_port.checked_add(i as u16)
                .ok_or_else(|| NodeError::ClusterError(format!("port overflow at node {i}")))?;

            let config = NodeConfig {
                port,
                node_id: format!("forge-node-{i}"),
                auto_increment: false, // cluster controls ports explicitly
                ..base_config.clone()
            };

            let stream = EustressStream::new(config.stream_config.clone());
            let server = NodeServer::start(stream, config).await?;
            info!("ForgeCluster: node {i} at {}", server.listen_addr);
            nodes.push(server);
        }

        let ring = build_ring(nodes.len());

        Ok(ForgeCluster { nodes, ring })
    }

    /// Return the node responsible for the given topic.
    #[inline]
    pub fn node_for_topic(&self, topic: &str) -> &Arc<NodeServer> {
        let h = hash_topic(topic);
        let idx = ring_lookup(&self.ring, h);
        &self.nodes[idx]
    }

    /// Publish to the node responsible for this topic.
    pub fn publish(&self, topic: &str, payload: bytes::Bytes) -> u64 {
        let node = self.node_for_topic(topic);
        node.stream.producer(topic).send_bytes(payload)
    }

    /// Subscribe to the node responsible for this topic.
    pub fn subscribe_to_topic<'a>(&self, topic: &'a str) -> Option<(EustressStream, &'a str)> {
        let node = self.node_for_topic(topic);
        Some((node.stream.clone(), topic))
    }

    /// Collect aggregate stats from all nodes.
    pub fn aggregate_stats(&self) -> ClusterStats {
        let mut total_topics = 0;
        let mut total_subscribers = 0;
        let mut node_stats = Vec::new();

        for node in &self.nodes {
            let topics = node.stream.topics();
            let subs: usize = topics.iter()
                .map(|t| node.stream.subscriber_count(t))
                .sum();
            total_topics += topics.len();
            total_subscribers += subs;
            node_stats.push(NodeStats {
                port: node.listen_addr.port(),
                listen_addr: node.listen_addr,
                topic_count: topics.len(),
            });
        }

        ClusterStats {
            node_count: self.nodes.len(),
            total_topics,
            total_subscribers,
            nodes: node_stats,
        }
    }

    /// Ordered list of node addresses (port ascending).
    pub fn node_addrs(&self) -> Vec<SocketAddr> {
        self.nodes.iter().map(|n| n.listen_addr).collect()
    }

    pub fn node_count(&self) -> usize { self.nodes.len() }

    /// Shutdown all nodes.
    pub fn shutdown(&self) {
        for node in &self.nodes {
            node.shutdown();
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Consistent hash ring
// ─────────────────────────────────────────────────────────────────────────────

const VIRTUAL_NODES_PER_PHYSICAL: usize = 150;

fn build_ring(physical_count: usize) -> Vec<(u64, usize)> {
    let mut ring: Vec<(u64, usize)> = Vec::with_capacity(physical_count * VIRTUAL_NODES_PER_PHYSICAL);

    for phys in 0..physical_count {
        for virt in 0..VIRTUAL_NODES_PER_PHYSICAL {
            let key = format!("forge-node-{phys}#{virt}");
            let h = hash_topic(&key);
            ring.push((h, phys));
        }
    }

    ring.sort_unstable_by_key(|(h, _)| *h);
    ring.dedup_by_key(|(h, _)| *h); // Remove hash collisions
    ring
}

fn hash_topic(topic: &str) -> u64 {
    let mut h = AHasher::default();
    topic.hash(&mut h);
    h.finish()
}

fn ring_lookup(ring: &[(u64, usize)], hash: u64) -> usize {
    match ring.binary_search_by_key(&hash, |(h, _)| *h) {
        Ok(idx) => ring[idx].1,
        Err(idx) => ring[idx % ring.len()].1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_distributes_evenly() {
        let ring = build_ring(10);
        // Each physical node should have roughly equal ring coverage.
        let mut counts = vec![0u64; 10];
        for &(_, phys) in &ring {
            counts[phys] += 1;
        }
        let mean = counts.iter().sum::<u64>() as f64 / 10.0;
        for c in &counts {
            let ratio = *c as f64 / mean;
            // Allow up to 50% deviation with 150 virtual nodes.
            assert!(ratio > 0.5 && ratio < 1.5, "uneven ring: ratio={ratio:.2}");
        }
    }

    #[test]
    fn ring_lookup_stable() {
        let ring = build_ring(3);
        let n1 = ring_lookup(&ring, hash_topic("scene_deltas"));
        let n2 = ring_lookup(&ring, hash_topic("scene_deltas"));
        assert_eq!(n1, n2);
    }
}
