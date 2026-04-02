//! =============================================================================
//! Verifier — Cross-server/fork identity verification
//! =============================================================================
//! Any EustressEngine fork can verify an identity file issued by another
//! server, as long as it trusts that server's public key.
//! Trust is established by registering known server public keys.
//! =============================================================================

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::collections::HashMap;

use crate::{
    issuer::canonical_payload,
    schema::IdentityFile,
    IdentityError,
};

/// Verifies identity files from any trusted server.
pub struct IdentityVerifier {
    /// Map of server domain → server public key.
    /// Populated from config, well-known endpoints, or manual trust.
    known_servers: HashMap<String, VerifyingKey>,
}

impl IdentityVerifier {
    pub fn new() -> Self {
        Self {
            known_servers: HashMap::new(),
        }
    }

    /// Register a trusted server's public key.
    /// After this, identity files issued by that server will be accepted.
    pub fn register_server(
        &mut self,
        domain: String,
        public_key_b64: &str,
    ) -> Result<(), IdentityError> {
        let key = crate::keypair::decode_public_key(public_key_b64)?;
        self.known_servers.insert(domain, key);
        Ok(())
    }

    /// Remove trust for a server (e.g. if compromised).
    pub fn revoke_server(&mut self, domain: &str) {
        self.known_servers.remove(domain);
    }

    /// Check if a server is trusted.
    pub fn is_trusted(&self, domain: &str) -> bool {
        self.known_servers.contains_key(domain)
    }

    /// Verify an identity file's server signature.
    ///
    /// This confirms:
    ///   1. The issuing server is in our trust list
    ///   2. The identity block was signed by that server's key
    ///   3. None of the signed fields have been tampered with
    ///   4. The identity has not expired
    pub fn verify(&self, identity: &IdentityFile) -> Result<(), IdentityError> {
        let meta = &identity.identity;

        // Check we know the issuing server
        let server_key = self
            .known_servers
            .get(&meta.issued_by)
            .ok_or(IdentityError::UnknownIssuer)?;

        // Check identity has not expired
        if Utc::now() > meta.expires_at {
            return Err(IdentityError::Expired);
        }

        // Reconstruct the canonical payload (must match issuer exactly)
        let payload = canonical_payload(
            &meta.version,
            &meta.user_id,
            &meta.public_key,
            &meta.issued_by,
            &meta.issued_at.to_rfc3339(),
            &meta.expires_at.to_rfc3339(),
        );

        // Decode and verify server signature
        let sig_bytes = STANDARD
            .decode(&meta.server_signature)
            .map_err(|_| IdentityError::InvalidSignature)?;
        let sig_bytes: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidSignature)?;
        let signature = Signature::from_bytes(&sig_bytes);

        server_key
            .verify(payload.as_bytes(), &signature)
            .map_err(|_| IdentityError::InvalidSignature)?;

        Ok(())
    }

    /// Full verification: server signature + expiration + revocation check.
    pub fn verify_with_revocation(
        &self,
        identity: &IdentityFile,
        revocation: &crate::revocation::RevocationList,
    ) -> Result<(), IdentityError> {
        // Check revocation first (fast path reject)
        if revocation.is_revoked(&identity.identity.user_id) {
            return Err(IdentityError::Revoked);
        }

        // Verify server signature + expiration
        self.verify(identity)
    }
}

impl Default for IdentityVerifier {
    fn default() -> Self {
        Self::new()
    }
}
