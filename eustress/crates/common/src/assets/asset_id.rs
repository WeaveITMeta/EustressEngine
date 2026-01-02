//! # ContentHash - Content-Addressable Asset Identifier
//!
//! SHA256 hash of asset content, encoded as Base58 for human readability.
//! This enables automatic deduplication and integrity verification.
//!
//! ## Example
//!
//! ```rust
//! use eustress_common::assets::ContentHash;
//!
//! let data = b"Hello, Eustress!";
//! let id = ContentHash::from_content(data);
//! 
//! // Base58 encoded: "2NEpo7TZRRrMA8YJU7D5g..."
//! println!("Content Hash: {}", id);
//!
//! // Verify integrity
//! assert!(id.verify(data));
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::fmt;

/// Content-addressable asset identifier (renamed from ContentHash to avoid Bevy conflict)
/// 
/// A 32-byte SHA256 hash that uniquely identifies asset content.
/// Two identical files will always have the same ContentHash.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash([u8; 32]);

/// Type alias for backward compatibility
pub type AssetContentId = ContentHash;

impl ContentHash {
    /// Create a ContentHash from raw content bytes
    /// 
    /// # Example
    /// 
    /// ```rust
    /// let id = ContentHash::from_content(b"model data...");
    /// ```
    pub fn from_content(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        Self(hasher.finalize().into())
    }
    
    /// Create a ContentHash from a hex string (64 characters)
    pub fn from_hex(hex: &str) -> Result<Self, ContentHashError> {
        if hex.len() != 64 {
            return Err(ContentHashError::InvalidLength { expected: 64, got: hex.len() });
        }
        
        let mut bytes = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk)
                .map_err(|_| ContentHashError::InvalidHex)?;
            bytes[i] = u8::from_str_radix(s, 16)
                .map_err(|_| ContentHashError::InvalidHex)?;
        }
        
        Ok(Self(bytes))
    }
    
    /// Create a ContentHash from a Base58 string
    pub fn from_base58(s: &str) -> Result<Self, ContentHashError> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| ContentHashError::InvalidBase58)?;
        
        if bytes.len() != 32 {
            return Err(ContentHashError::InvalidLength { expected: 32, got: bytes.len() });
        }
        
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
    
    /// Convert to Base58 string (human-readable, like IPFS CIDs)
    pub fn to_base58(&self) -> String {
        bs58::encode(&self.0).into_string()
    }
    
    /// Convert to hex string (64 characters)
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
    
    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    
    /// Verify that content matches this ContentHash
    /// 
    /// Returns true if SHA256(data) == this ID
    pub fn verify(&self, data: &[u8]) -> bool {
        Self::from_content(data) == *self
    }
    
    /// Create a "null" ContentHash (all zeros) - used as placeholder
    pub fn null() -> Self {
        Self([0u8; 32])
    }
    
    /// Check if this is the null ContentHash
    pub fn is_null(&self) -> bool {
        self.0 == [0u8; 32]
    }
    
    /// Create from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", &self.to_base58()[..12])
    }
}

impl Default for ContentHash {
    fn default() -> Self {
        Self::null()
    }
}

impl std::str::FromStr for ContentHash {
    type Err = ContentHashError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try Base58 first (shorter), then hex
        Self::from_base58(s).or_else(|_| Self::from_hex(s))
    }
}

/// Errors when parsing ContentHash
#[derive(Debug, Clone, thiserror::Error)]
pub enum ContentHashError {
    #[error("Invalid length: expected {expected}, got {got}")]
    InvalidLength { expected: usize, got: usize },
    
    #[error("Invalid hex encoding")]
    InvalidHex,
    
    #[error("Invalid Base58 encoding")]
    InvalidBase58,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_from_content() {
        let data = b"Hello, Eustress!";
        let id = ContentHash::from_content(data);
        
        // Same content = same ID
        let id2 = ContentHash::from_content(data);
        assert_eq!(id, id2);
        
        // Different content = different ID
        let id3 = ContentHash::from_content(b"Different data");
        assert_ne!(id, id3);
    }
    
    #[test]
    fn test_verify() {
        let data = b"Test data for verification";
        let id = ContentHash::from_content(data);
        
        assert!(id.verify(data));
        assert!(!id.verify(b"Corrupted data"));
    }
    
    #[test]
    fn test_base58_roundtrip() {
        let data = b"Roundtrip test";
        let id = ContentHash::from_content(data);
        
        let base58 = id.to_base58();
        let parsed = ContentHash::from_base58(&base58).unwrap();
        
        assert_eq!(id, parsed);
    }
    
    #[test]
    fn test_hex_roundtrip() {
        let data = b"Hex roundtrip";
        let id = ContentHash::from_content(data);
        
        let hex = id.to_hex();
        assert_eq!(hex.len(), 64);
        
        let parsed = ContentHash::from_hex(&hex).unwrap();
        assert_eq!(id, parsed);
    }
    
    #[test]
    fn test_null() {
        let null = ContentHash::null();
        assert!(null.is_null());
        
        let real = ContentHash::from_content(b"data");
        assert!(!real.is_null());
    }
}
