//! # Group Service
//!
//! Group/guild management.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Group service resource
#[derive(Resource, Default, Clone, Debug)]
pub struct GroupService {
    pub groups: std::collections::HashMap<Uuid, Group>,
}

/// Group definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub members: Vec<Uuid>,
    pub rank_definitions: Vec<GroupRank>,
}

/// Group rank definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupRank {
    pub id: u32,
    pub name: String,
    pub permissions: Vec<String>,
}
