//! =============================================================================
//! EUSTRESS-IDENTITY — Sovereign Portable Identity (SPI)
//! =============================================================================
//! Table of Contents:
//!   1. schema     — TOML identity file structure (versioned, fork-portable)
//!   2. keypair    — Ed25519 key generation and challenge-response
//!   3. issuer     — Server-side identity issuance
//!   4. verifier   — Cross-server/fork signature verification
//!   5. revocation — Append-only revocation list
//!   6. succession — Inheritance / will system
//!   7. desktop    — OS-agnostic Desktop file loader
//!   8. history    — Contribution hash chain for portability
//!   9. witness    — HTTP client for co-signing API (Cloudflare Worker)
//! =============================================================================

pub mod schema;
pub mod keypair;
pub mod issuer;
pub mod verifier;
pub mod revocation;
pub mod succession;
pub mod desktop;
pub mod history;
#[cfg(feature = "witness")]
pub mod witness;

use thiserror::Error;

/// Schema version — all forks must honor this version's [identity] block.
pub const SCHEMA_VERSION: &str = "1.0";

/// Canonical identity TOML filename on the user's Desktop.
pub const IDENTITY_FILENAME: &str = "identity.toml";

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("Invalid or malformed key")]
    InvalidKey,

    #[error("Signature verification failed")]
    InvalidSignature,

    #[error("Issuing server is not recognized")]
    UnknownIssuer,

    #[error("Identity has been revoked")]
    Revoked,

    #[error("Identity file not found on Desktop")]
    NotFound,

    #[error("Succession list is empty — no heir designated")]
    NoSuccessor,

    #[error("Successor identity not found or revoked")]
    InvalidSuccessor,

    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("TOML parse error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Challenge expired or invalid")]
    ChallengeInvalid,

    #[error("Identity has expired — must be renewed by the issuing server")]
    Expired,
}

/// Serialize an identity file to a TOML string for delivery to the user.
pub fn to_toml_string(identity: &schema::IdentityFile) -> Result<String, IdentityError> {
    Ok(toml::to_string_pretty(identity)?)
}

/// Parse a TOML string back into an IdentityFile.
pub fn from_toml_string(s: &str) -> Result<schema::IdentityFile, IdentityError> {
    Ok(toml::from_str(s)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issuer::IdentityIssuer;
    use crate::keypair::{verify_challenge, UserKeypair};
    use crate::revocation::RevocationList;
    use crate::verifier::IdentityVerifier;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn make_test_server(domain: &str) -> IdentityIssuer {
        let key = SigningKey::generate(&mut OsRng);
        IdentityIssuer::new(key, domain.to_string())
    }

    #[test]
    fn test_issue_serialize_verify_roundtrip() {
        let server = make_test_server("server-a.test");
        let (identity_file, _private_key) = server
            .issue("alice".to_string())
            .expect("issuance should succeed");

        let toml_str = to_toml_string(&identity_file).expect("serialize");
        let parsed = from_toml_string(&toml_str).expect("deserialize");

        let mut verifier = IdentityVerifier::new();
        verifier
            .register_server("server-a.test".to_string(), &server.server_public_key_b64())
            .unwrap();

        assert!(verifier.verify(&parsed).is_ok());
    }

    #[test]
    fn test_cross_server_verification() {
        let server_a = make_test_server("server-a.test");
        let mut server_b_verifier = IdentityVerifier::new();
        server_b_verifier
            .register_server("server-a.test".to_string(), &server_a.server_public_key_b64())
            .unwrap();

        let (identity, _) = server_a.issue("bob".to_string()).unwrap();
        let toml_str = to_toml_string(&identity).unwrap();
        let parsed = from_toml_string(&toml_str).unwrap();

        // Server B never issued this — verifies because it trusts Server A
        assert!(server_b_verifier.verify(&parsed).is_ok());
    }

    #[test]
    fn test_unknown_issuer_rejected() {
        let server_a = make_test_server("server-a.test");
        let (identity, _) = server_a.issue("dave".to_string()).unwrap();
        let toml_str = to_toml_string(&identity).unwrap();
        let parsed = from_toml_string(&toml_str).unwrap();

        let verifier = IdentityVerifier::new();
        assert!(matches!(
            verifier.verify(&parsed),
            Err(IdentityError::UnknownIssuer)
        ));
    }

    #[test]
    fn test_tampered_identity_rejected() {
        let server = make_test_server("server-a.test");
        let (mut identity, _) = server.issue("charlie".to_string()).unwrap();

        // Tamper with the signed identity block
        identity.identity.user_id = "ATTACKER_ID".to_string();

        let mut verifier = IdentityVerifier::new();
        verifier
            .register_server("server-a.test".to_string(), &server.server_public_key_b64())
            .unwrap();

        assert!(matches!(
            verifier.verify(&identity),
            Err(IdentityError::InvalidSignature)
        ));
    }

    #[test]
    fn test_revoked_identity() {
        let server = make_test_server("server-a.test");
        let (identity, _) = server.issue("eve".to_string()).unwrap();
        let user_id = identity.identity.user_id.clone();

        let mut revocation = RevocationList::new("server-a.test".to_string());
        revocation.revoke(user_id.clone(), None);

        assert!(revocation.is_revoked(&user_id));
    }

    #[test]
    fn test_challenge_response() {
        use crate::keypair::build_challenge_payload;

        let server = make_test_server("server-a.test");
        let (identity, private_key_b64) = server.issue("frank".to_string()).unwrap();

        let nonce = b"server-generated-nonce-12345";
        let domain = "server-a.test";
        let timestamp = chrono::Utc::now().timestamp();

        // Build domain-bound challenge payload
        let payload = build_challenge_payload(domain, timestamp, nonce);

        let keypair = UserKeypair::from_private_b64(&private_key_b64).unwrap();
        let signature = keypair.sign_bytes(&payload);

        assert!(verify_challenge(&identity.identity.public_key, &payload, &signature).is_ok());

        // Verify that a different domain fails
        let wrong_payload = build_challenge_payload("evil.test", timestamp, nonce);
        assert!(verify_challenge(&identity.identity.public_key, &wrong_payload, &signature).is_err());
    }

    #[test]
    fn test_succession_ordered_list() {
        let server = make_test_server("server-a.test");
        let (mut identity, private_key_b64) = server.issue("grandpa".to_string()).unwrap();

        // Create heir identities
        let (heir1, _) = server.issue("child".to_string()).unwrap();
        let (heir2, _) = server.issue("grandchild".to_string()).unwrap();

        // Add succession list
        let keypair = UserKeypair::from_private_b64(&private_key_b64).unwrap();
        let heirs = vec![
            heir1.identity.public_key.clone(),
            heir2.identity.public_key.clone(),
        ];
        identity.set_succession(&keypair, heirs);

        // First in list gets everything
        assert_eq!(
            identity.succession.as_ref().unwrap().heirs[0],
            heir1.identity.public_key
        );
    }

    #[test]
    fn test_contribution_hash_chain() {
        use crate::history::ContributionHistory;

        let server = make_test_server("server-a.test");
        let (_, private_key_b64) = server.issue("worker".to_string()).unwrap();
        let keypair = UserKeypair::from_private_b64(&private_key_b64).unwrap();

        let mut history = ContributionHistory::new();
        history.record(&keypair, "session_abc", 42.5, "eustress.dev");
        history.record(&keypair, "session_def", 18.0, "eustress.dev");

        assert_eq!(history.entries.len(), 2);
        // Each entry chains to the previous hash
        assert_ne!(history.entries[0].hash, history.entries[1].hash);
        // Verify the chain is intact
        assert!(history.verify_chain(&keypair.public_key_b64()).is_ok());
    }
}
