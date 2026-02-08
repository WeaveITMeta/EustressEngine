// ============================================================================
// Eustress Engine - Entity Utilities
// Entity ID helpers and conversion utilities
// ============================================================================

use bevy::prelude::*;

/// Convert entity to a stable ID string for serialization
pub fn entity_to_id_string(entity: Entity) -> String {
    format!("{}v{}", entity.index(), entity.generation())
}

/// Parse entity from ID string (returns None if invalid)
pub fn id_string_to_entity(id: &str) -> Option<Entity> {
    let parts: Vec<&str> = id.split('v').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let index: u32 = parts[0].parse().ok()?;
    let generation: u32 = parts[1].parse().ok()?;
    
    Some(Entity::from_bits(
        ((generation as u64) << 32) | (index as u64)
    ))
}

/// Get a human-readable name for an entity
pub fn get_entity_display_name(
    entity: Entity,
    names: &Query<&Name>,
) -> String {
    names
        .get(entity)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|_| format!("Entity_{}", entity.index()))
}

/// Convert entity to a numeric ID
pub fn entity_to_id(entity: Entity) -> u64 {
    entity.to_bits()
}
