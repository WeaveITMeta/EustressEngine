//! # Moderation Service
//!
//! Content moderation and safety.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Moderation service resource
#[derive(Resource, Default, Clone, Debug)]
pub struct ModerationService {
    pub reports: Vec<ModerationReport>,
    pub bans: Vec<Ban>,
}

/// Moderation report
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModerationReport {
    pub id: Uuid,
    pub reporter_id: Uuid,
    pub target_id: Uuid,
    pub reason: String,
    pub timestamp_ms: i64,
    pub status: ReportStatus,
}

/// Report status
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReportStatus {
    Pending,
    Reviewed,
    ActionTaken,
    Dismissed,
}

/// Ban record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ban {
    pub user_id: Uuid,
    pub reason: String,
    pub issued_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub permanent: bool,
}
