//! Deterministic UUID derivation for Roblox referents.
//!
//! Same `(space_salt, referent)` → byte-exact same `Uuid`, so re-importing
//! an unchanged place produces zero worlddb churn. See `docs/architecture/
//! ROBLOX_IMPORT_SPEC.md` §8 (idempotency) and the broader identity
//! discussion at `docs/AUDIT/08_IDENTITY_TRUST.md`.
//!
//! This module is **real** (not stubbed) — it has no external deps
//! beyond `blake3` + `uuid`, both already in the workspace.

use uuid::Uuid;

/// Derive a stable Eustress entity UUID from a Roblox referent and a
/// per-Space salt.
///
/// - `space_salt` — typically the Space's own UUID bytes (16 B). Ensures
///   the same `.rbxl` imported into two different Spaces produces two
///   different uuid sets.
/// - `referent` — the Roblox-side per-DataModel ID (`"RBX..."` string).
///   Stable across Studio sessions, so re-importing the same file into
///   the same Space yields the same uuids.
///
/// Uses the first 16 bytes of `blake3(space_salt || ":eustress-roblox-
/// import:" || referent)`. blake3 is already a workspace dep; the
/// truncation is collision-safe at any realistic scale.
pub fn entity_uuid(space_salt: &[u8], referent: &str) -> Uuid {
    let mut hasher = blake3::Hasher::new();
    hasher.update(space_salt);
    hasher.update(b":eustress-roblox-import:");
    hasher.update(referent.as_bytes());
    let hash = hasher.finalize();
    let bytes: [u8; 16] = hash.as_bytes()[..16]
        .try_into()
        .expect("blake3 output is 32 B, first 16 always available");
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_uuid() {
        let a = entity_uuid(b"space-salt-1", "RBX0123456789ABCDEF");
        let b = entity_uuid(b"space-salt-1", "RBX0123456789ABCDEF");
        assert_eq!(a, b, "identity must be deterministic");
    }

    #[test]
    fn different_salts_produce_different_uuids() {
        let a = entity_uuid(b"space-salt-1", "RBX0123456789ABCDEF");
        let b = entity_uuid(b"space-salt-2", "RBX0123456789ABCDEF");
        assert_ne!(a, b, "salt must propagate into the uuid");
    }

    #[test]
    fn different_referents_produce_different_uuids() {
        let a = entity_uuid(b"salt", "RBX0000000000000001");
        let b = entity_uuid(b"salt", "RBX0000000000000002");
        assert_ne!(a, b, "referent must propagate into the uuid");
    }
}
