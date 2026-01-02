//! # Soul Validator (Stub)
//!
//! Rune validation is now handled in rune_api.rs via validate_rune_script().
//! This module is kept for interface compatibility.

// ============================================================================
// Validation Result
// ============================================================================

/// Result of code validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// ============================================================================
// Validator (Stub)
// ============================================================================

/// Soul code validator (stub - Rune validation is in rune_api.rs)
#[derive(Default, Clone)]
pub struct SoulValidator;

impl SoulValidator {
    /// Validate code (stub - always passes, real validation is in rune_api.rs)
    pub fn validate(&self, _code: &str) -> ValidationResult {
        ValidationResult {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    
    /// Validate Rust syntax (stub - always passes)
    pub fn validate_syntax(&self, _code: &str) -> ValidationResult {
        ValidationResult {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    
    /// Validate Rune syntax (stub - real validation is in rune_api.rs)
    pub fn validate_rune_syntax(&self, _code: &str) -> ValidationResult {
        ValidationResult {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
}
