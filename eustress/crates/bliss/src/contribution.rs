// =============================================================================
// Bliss Cryptocurrency - Contribution Tracking
// =============================================================================
// Table of Contents:
// 1. ContributionType - categories of user contributions
// 2. ContributionWeight - reward multipliers per type
// 3. Contribution - a single recorded contribution
// 4. ContributionTracker - tracks and rewards contributions
// =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::blockchain::Blockchain;
use crate::crypto::BlissCrypto;
use crate::error::BlissError;
use crate::wallet::WalletAddress;

// =============================================================================
// 1. ContributionType
// =============================================================================

/// Categories of contributions that earn Bliss tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContributionType {
    /// Creating 3D models, places, and assets.
    Building,
    /// Writing Soul scripts and game logic.
    Scripting,
    /// UI/UX design, texturing, visual work.
    Design,
    /// Team work, communication, helping others.
    Collaboration,
    /// Tutorials, mentoring, documentation.
    Teaching,
}

impl ContributionType {
    /// Get the display name for this contribution type.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContributionType::Building => "Building",
            ContributionType::Scripting => "Scripting",
            ContributionType::Design => "Design",
            ContributionType::Collaboration => "Collaboration",
            ContributionType::Teaching => "Teaching",
        }
    }
}

// =============================================================================
// 2. ContributionWeight
// =============================================================================

/// Reward multiplier configuration for each contribution type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionWeight {
    /// Base reward in micro-BLS per contribution unit.
    pub base_reward: u64,
    /// Multiplier for this contribution type.
    pub multiplier: f64,
}

impl ContributionWeight {
    /// Calculate the actual reward for a given weight.
    pub fn calculate_reward(&self, weight: f64) -> u64 {
        (self.base_reward as f64 * self.multiplier * weight) as u64
    }
}

/// Default weights matching the table in lib.rs documentation.
pub fn default_weights() -> HashMap<ContributionType, ContributionWeight> {
    let mut weights = HashMap::new();
    let base = 1_000; // 0.001 BLS base reward
    
    weights.insert(ContributionType::Building, ContributionWeight {
        base_reward: base,
        multiplier: 2.5,
    });
    weights.insert(ContributionType::Scripting, ContributionWeight {
        base_reward: base,
        multiplier: 3.0,
    });
    weights.insert(ContributionType::Design, ContributionWeight {
        base_reward: base,
        multiplier: 2.0,
    });
    weights.insert(ContributionType::Collaboration, ContributionWeight {
        base_reward: base,
        multiplier: 2.0,
    });
    weights.insert(ContributionType::Teaching, ContributionWeight {
        base_reward: base,
        multiplier: 2.2,
    });
    
    weights
}

// =============================================================================
// 3. Contribution
// =============================================================================

/// A single recorded user contribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    /// Unique contribution ID (BLAKE3 hash of contents).
    pub id: String,
    /// User ID who made the contribution.
    pub user_id: String,
    /// Type of contribution.
    pub contribution_type: ContributionType,
    /// Contribution weight (effort/quality factor, typically 0.1 - 10.0).
    pub weight: f64,
    /// Human-readable description of the contribution.
    pub description: String,
    /// Evidence references (asset IDs, commit hashes, URLs).
    pub evidence: Vec<String>,
    /// When the contribution was recorded.
    pub timestamp: DateTime<Utc>,
    /// Reward amount in micro-BLS (filled after processing).
    pub reward: u64,
    /// Whether the reward has been distributed.
    pub distributed: bool,
}

impl Contribution {
    /// Generate a deterministic ID for this contribution.
    pub fn compute_id(user_id: &str, description: &str, timestamp: &DateTime<Utc>) -> String {
        let data = format!("{}:{}:{}", user_id, description, timestamp.timestamp_millis());
        let hash = BlissCrypto::hash(data.as_bytes());
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// =============================================================================
// 4. ContributionTracker
// =============================================================================

/// Tracks user contributions and distributes Bliss token rewards.
pub struct ContributionTracker {
    /// Reward weights per contribution type.
    weights: HashMap<ContributionType, ContributionWeight>,
    /// All recorded contributions (in-memory).
    contributions: Vec<Contribution>,
    /// Map of user_id -> wallet address for reward distribution.
    user_wallets: HashMap<String, WalletAddress>,
}

impl ContributionTracker {
    /// Create a new tracker with default weights.
    pub fn new() -> Self {
        Self {
            weights: default_weights(),
            contributions: Vec::new(),
            user_wallets: HashMap::new(),
        }
    }
    
    /// Create a tracker with custom weights.
    pub fn with_weights(weights: HashMap<ContributionType, ContributionWeight>) -> Self {
        Self {
            weights,
            contributions: Vec::new(),
            user_wallets: HashMap::new(),
        }
    }
    
    /// Register a user's wallet address for reward distribution.
    pub fn register_wallet(&mut self, user_id: &str, address: WalletAddress) {
        self.user_wallets.insert(user_id.to_string(), address);
    }
    
    /// Record a contribution and calculate the reward.
    pub fn record(&mut self, mut contribution: Contribution) -> Result<&Contribution, BlissError> {
        // Validate weight
        if contribution.weight <= 0.0 {
            return Err(BlissError::Contribution("Weight must be positive".into()));
        }
        
        // Generate ID if not set
        if contribution.id.is_empty() {
            contribution.id = Contribution::compute_id(
                &contribution.user_id,
                &contribution.description,
                &contribution.timestamp,
            );
        }
        
        // Check for duplicates
        if self.contributions.iter().any(|c| c.id == contribution.id) {
            return Err(BlissError::DuplicateContribution(contribution.id));
        }
        
        // Calculate reward
        let weight_config = self.weights.get(&contribution.contribution_type)
            .cloned()
            .unwrap_or(ContributionWeight { base_reward: 1_000, multiplier: 1.0 });
        contribution.reward = weight_config.calculate_reward(contribution.weight);
        contribution.timestamp = Utc::now();
        
        self.contributions.push(contribution);
        Ok(self.contributions.last().unwrap())
    }
    
    /// Distribute pending rewards to wallets via the blockchain.
    pub async fn distribute_rewards(&mut self, blockchain: &mut Blockchain) -> Result<u64, BlissError> {
        let mut total_distributed: u64 = 0;
        
        for contribution in self.contributions.iter_mut() {
            if contribution.distributed {
                continue;
            }
            
            let wallet_address = match self.user_wallets.get(&contribution.user_id) {
                Some(address) => address.clone(),
                None => {
                    tracing::warn!("No wallet registered for user {}", contribution.user_id);
                    continue;
                }
            };
            
            // Credit reward to the blockchain
            blockchain.credit(
                &wallet_address,
                contribution.reward,
                Some(format!("{}: {}", contribution.contribution_type.as_str(), contribution.description)),
            ).await?;
            
            contribution.distributed = true;
            total_distributed += contribution.reward;
        }
        
        Ok(total_distributed)
    }
    
    /// Get all contributions for a user.
    pub fn get_user_contributions(&self, user_id: &str) -> Vec<&Contribution> {
        self.contributions.iter()
            .filter(|c| c.user_id == user_id)
            .collect()
    }
    
    /// Get total earned (distributed + pending) for a user.
    pub fn get_user_total_earned(&self, user_id: &str) -> u64 {
        self.contributions.iter()
            .filter(|c| c.user_id == user_id)
            .map(|c| c.reward)
            .sum()
    }
    
    /// Get number of pending (undistributed) contributions.
    pub fn pending_count(&self) -> usize {
        self.contributions.iter().filter(|c| !c.distributed).count()
    }
}

impl Default for ContributionTracker {
    fn default() -> Self {
        Self::new()
    }
}
