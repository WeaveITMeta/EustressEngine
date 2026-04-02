//! =============================================================================
//! Keypair — Ed25519 key generation, challenge-response, zeroize
//! =============================================================================
//! One keypair serves two roles:
//!   1. EustressEngine login (challenge-response)
//!   2. Bliss wallet address (derived from public key)
//! The private key never touches the server after issuance.
//! =============================================================================

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

use crate::IdentityError;

/// User's Ed25519 keypair. Private key is zeroized on drop.
pub struct UserKeypair {
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}

impl UserKeypair {
    /// Generate a fresh keypair for a new user.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Encode public key as base64 for storage in TOML.
    pub fn public_key_b64(&self) -> String {
        STANDARD.encode(self.verifying_key.as_bytes())
    }

    /// Encode private key as base64 for secure local storage.
    /// This NEVER goes to the server.
    pub fn private_key_b64(&self) -> String {
        STANDARD.encode(self.signing_key.to_bytes())
    }

    /// Reconstruct keypair from base64-encoded private key.
    pub fn from_private_b64(encoded: &str) -> Result<Self, IdentityError> {
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| IdentityError::InvalidKey)?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidKey)?;
        let signing_key = SigningKey::from_bytes(&bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Sign a challenge nonce (for login proof-of-possession).
    pub fn sign_challenge(&self, nonce: &[u8]) -> String {
        let signature = self.signing_key.sign(nonce);
        STANDARD.encode(signature.to_bytes())
    }

    /// Sign arbitrary bytes and return base64 signature.
    pub fn sign_bytes(&self, data: &[u8]) -> String {
        let signature = self.signing_key.sign(data);
        STANDARD.encode(signature.to_bytes())
    }
}

// NOTE: No custom Drop needed. The inner `ed25519_dalek::SigningKey` implements
// `ZeroizeOnDrop` when compiled with the `zeroize` feature (which we enable).
// The previous impl called `to_bytes()` which creates a *copy* on the stack,
// then zeroized only that copy — leaving the actual key material untouched.
// By relying on SigningKey's own Drop, the real key bytes are zeroized.

/// Build a domain-and-timestamp-bound challenge payload.
///
/// SECURITY: The challenge includes the server domain and a timestamp to prevent:
///   - Relay attacks (challenge from server A replayed to server B)
///   - Replay attacks (old challenge reused after window expires)
///
/// Format: "challenge|{domain}|{timestamp_secs}|{nonce_hex}"
pub fn build_challenge_payload(domain: &str, timestamp_secs: i64, nonce: &[u8]) -> Vec<u8> {
    format!(
        "challenge|{}|{}|{}",
        domain,
        timestamp_secs,
        hex::encode(nonce),
    )
    .into_bytes()
}

/// Verify a challenge-response signature against a base64-encoded public key.
/// Used by servers to prove the user holds the private key matching their TOML.
///
/// For domain-bound challenges, pass the output of `build_challenge_payload()`
/// as the `payload` parameter. For legacy compatibility, raw nonces are also accepted.
pub fn verify_challenge(
    public_key_b64: &str,
    payload: &[u8],
    signature_b64: &str,
) -> Result<(), IdentityError> {
    let verifying_key = decode_public_key(public_key_b64)?;
    let signature = decode_signature(signature_b64)?;
    verifying_key
        .verify(payload, &signature)
        .map_err(|_| IdentityError::InvalidSignature)
}

/// Verify a signature over arbitrary data against a base64-encoded public key.
pub fn verify_signed_data(
    public_key_b64: &str,
    data: &[u8],
    signature_b64: &str,
) -> Result<(), IdentityError> {
    let verifying_key = decode_public_key(public_key_b64)?;
    let signature = decode_signature(signature_b64)?;
    verifying_key
        .verify(data, &signature)
        .map_err(|_| IdentityError::InvalidSignature)
}

/// Decode a base64-encoded Ed25519 public key.
pub fn decode_public_key(b64: &str) -> Result<VerifyingKey, IdentityError> {
    let bytes = STANDARD
        .decode(b64)
        .map_err(|_| IdentityError::InvalidKey)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| IdentityError::InvalidKey)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| IdentityError::InvalidKey)
}

fn decode_signature(b64: &str) -> Result<Signature, IdentityError> {
    let bytes = STANDARD
        .decode(b64)
        .map_err(|_| IdentityError::InvalidSignature)?;
    let bytes: [u8; 64] = bytes
        .try_into()
        .map_err(|_| IdentityError::InvalidSignature)?;
    Ok(Signature::from_bytes(&bytes))
}
