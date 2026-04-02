// =============================================================================
// Eustress Witness Worker — Ed25519 Signing
// =============================================================================

use ed25519_dalek::{SigningKey, Signer};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

/// Sign a canonical payload using the server's Ed25519 key.
///
/// The signing key is stored as a base64-encoded 32-byte seed in the
/// SIGNING_KEY secret.
pub fn sign_payload(signing_key_b64: &str, payload: &str) -> std::result::Result<String, String> {
    let key_bytes = BASE64
        .decode(signing_key_b64)
        .map_err(|e| format!("Invalid signing key base64: {}", e))?;

    if key_bytes.len() != 32 {
        return Err(format!(
            "Signing key must be 32 bytes, got {}",
            key_bytes.len()
        ));
    }

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&seed);

    let signature = signing_key.sign(payload.as_bytes());
    Ok(BASE64.encode(signature.to_bytes()))
}

/// Build the canonical cosign payload string.
pub fn build_cosign_payload(user_id: &str, contribution_hash: &str, timestamp: &str) -> String {
    format!("cosign|{}|{}|{}", user_id, contribution_hash, timestamp)
}
