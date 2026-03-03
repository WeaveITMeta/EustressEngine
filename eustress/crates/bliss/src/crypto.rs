// =============================================================================
// Bliss Cryptocurrency - Cryptographic Primitives
// =============================================================================
// Table of Contents:
// 1. Key Types - PublicKey, PrivateKey
// 2. Signature - Ed25519 digital signature
// 3. BlissCrypto - High-level crypto operations (sign, verify, hash)
// =============================================================================

use ed25519_dalek::{
    Signer, SigningKey, Verifier, VerifyingKey,
};
use serde::{Deserialize, Serialize};

use crate::error::BlissError;

// =============================================================================
// 1. Key Types
// =============================================================================

/// Ed25519 public key wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    /// Raw 32-byte public key encoded as hex.
    bytes: Vec<u8>,
}

impl PublicKey {
    /// Create from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlissError> {
        if bytes.len() != 32 {
            return Err(BlissError::Crypto("Public key must be 32 bytes".into()));
        }
        Ok(Self { bytes: bytes.to_vec() })
    }
    
    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    
    /// Hex-encoded representation.
    pub fn to_hex(&self) -> String {
        hex_encode(&self.bytes)
    }
    
    /// Convert to Ed25519 verifying key.
    fn to_verifying_key(&self) -> Result<VerifyingKey, BlissError> {
        let bytes: [u8; 32] = self.bytes.as_slice().try_into()
            .map_err(|_| BlissError::Crypto("Invalid public key length".into()))?;
        VerifyingKey::from_bytes(&bytes)
            .map_err(|e| BlissError::Crypto(format!("Invalid public key: {}", e)))
    }
}

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Ed25519 private key wrapper.
#[derive(Clone)]
pub struct PrivateKey {
    /// Raw 32-byte secret key.
    bytes: Vec<u8>,
}

impl PrivateKey {
    /// Generate a new random private key.
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        Self { bytes: signing_key.to_bytes().to_vec() }
    }
    
    /// Create from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlissError> {
        if bytes.len() != 32 {
            return Err(BlissError::Crypto("Private key must be 32 bytes".into()));
        }
        Ok(Self { bytes: bytes.to_vec() })
    }
    
    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    
    /// Derive the corresponding public key.
    pub fn public_key(&self) -> PublicKey {
        let signing_key = self.to_signing_key();
        let verifying_key = signing_key.verifying_key();
        PublicKey { bytes: verifying_key.to_bytes().to_vec() }
    }
    
    /// Convert to Ed25519 signing key.
    fn to_signing_key(&self) -> SigningKey {
        let bytes: [u8; 32] = self.bytes.as_slice().try_into()
            .expect("PrivateKey must be 32 bytes");
        SigningKey::from_bytes(&bytes)
    }
}

impl std::fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PrivateKey([REDACTED])")
    }
}

// =============================================================================
// 2. Signature
// =============================================================================

/// Ed25519 digital signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// Raw 64-byte signature.
    bytes: Vec<u8>,
}

impl Signature {
    /// Create from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlissError> {
        if bytes.len() != 64 {
            return Err(BlissError::Crypto("Signature must be 64 bytes".into()));
        }
        Ok(Self { bytes: bytes.to_vec() })
    }
    
    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    
    /// Hex-encoded representation.
    pub fn to_hex(&self) -> String {
        hex_encode(&self.bytes)
    }
    
    /// Convert to Ed25519 signature.
    fn to_ed25519(&self) -> Result<ed25519_dalek::Signature, BlissError> {
        let bytes: [u8; 64] = self.bytes.as_slice().try_into()
            .map_err(|_| BlissError::Crypto("Invalid signature length".into()))?;
        Ok(ed25519_dalek::Signature::from_bytes(&bytes))
    }
}

// =============================================================================
// 3. BlissCrypto
// =============================================================================

/// High-level cryptographic operations for the Bliss system.
pub struct BlissCrypto;

impl BlissCrypto {
    /// Generate a new keypair.
    pub fn generate_keypair() -> (PrivateKey, PublicKey) {
        let private_key = PrivateKey::generate();
        let public_key = private_key.public_key();
        (private_key, public_key)
    }
    
    /// Sign a message with a private key.
    pub fn sign(message: &[u8], private_key: &PrivateKey) -> Signature {
        let signing_key = private_key.to_signing_key();
        let sig = signing_key.sign(message);
        Signature { bytes: sig.to_bytes().to_vec() }
    }
    
    /// Verify a signature against a message and public key.
    pub fn verify(message: &[u8], signature: &Signature, public_key: &PublicKey) -> Result<bool, BlissError> {
        let verifying_key = public_key.to_verifying_key()?;
        let sig = signature.to_ed25519()?;
        Ok(verifying_key.verify(message, &sig).is_ok())
    }
    
    /// Hash data with BLAKE3 (fast, cryptographic).
    pub fn hash(data: &[u8]) -> Vec<u8> {
        blake3::hash(data).as_bytes().to_vec()
    }
    
    /// Hash data with SHA-256.
    pub fn sha256(data: &[u8]) -> Vec<u8> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().to_vec()
    }
}

// =============================================================================
// Utility: hex encoding (avoids adding hex crate dependency)
// =============================================================================

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
