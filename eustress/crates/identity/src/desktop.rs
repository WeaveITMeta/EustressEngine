//! =============================================================================
//! Desktop — Configurable identity TOML loader
//! =============================================================================
//! The identity TOML lives at a user-configured location:
//!   - Default: Desktop (tangible, visible, human-understandable)
//!   - Configurable: any path (cloud sync folder, USB, NAS, etc.)
//!
//! The location is stored in a small config file at:
//!   ~/.eustress/identity_location
//!
//! Supported: Windows, macOS, Linux (via dirs crate).
//! =============================================================================

use std::path::PathBuf;

use crate::{schema::IdentityFile, IdentityError, IDENTITY_FILENAME};

/// Config file name for identity location override.
const LOCATION_CONFIG: &str = "identity_location";

/// Get the Eustress config directory (~/.eustress/).
fn config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".eustress"))
}

/// Get the path to the identity location config file.
fn location_config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(LOCATION_CONFIG))
}

/// Read the user's configured identity directory.
/// Falls back to Desktop if no override is set.
fn identity_dir() -> Result<PathBuf, IdentityError> {
    // Check for user override
    if let Some(config_path) = location_config_path() {
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)
                .map_err(|e| IdentityError::Io(e))?;
            let custom_dir = contents.trim();
            if !custom_dir.is_empty() {
                return Ok(PathBuf::from(custom_dir));
            }
        }
    }

    // Default: Desktop
    dirs::desktop_dir().ok_or(IdentityError::NotFound)
}

/// Get the path to the identity TOML.
/// Uses the user-configured location, falling back to Desktop.
pub fn identity_path() -> Result<PathBuf, IdentityError> {
    let dir = identity_dir()?;
    Ok(dir.join(IDENTITY_FILENAME))
}

/// Set the identity storage location.
///
/// Pass a directory path (e.g. "C:\Users\me\OneDrive\Desktop" or
/// "/home/me/Dropbox"). The identity.toml will be stored there.
///
/// Pass `None` to reset to the default (Desktop).
pub fn set_identity_location(path: Option<&str>) -> Result<(), IdentityError> {
    let config_dir = config_dir().ok_or(IdentityError::NotFound)?;
    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join(LOCATION_CONFIG);

    match path {
        Some(dir) => {
            // Validate the directory exists
            let dir_path = PathBuf::from(dir);
            if !dir_path.exists() {
                return Err(IdentityError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Directory does not exist: {}", dir),
                )));
            }
            std::fs::write(&config_path, dir)?;
        }
        None => {
            // Reset to default — remove the config file
            if config_path.exists() {
                std::fs::remove_file(&config_path)?;
            }
        }
    }

    Ok(())
}

/// Get the currently configured identity location (for display to user).
pub fn get_identity_location() -> Result<PathBuf, IdentityError> {
    identity_dir()
}

/// Load the identity file from the configured location.
pub fn load_identity() -> Result<IdentityFile, IdentityError> {
    let path = identity_path()?;
    if !path.exists() {
        return Err(IdentityError::NotFound);
    }
    let contents = std::fs::read_to_string(&path)?;
    crate::from_toml_string(&contents)
}

/// Save the identity file to the configured location.
pub fn save_identity(identity: &IdentityFile) -> Result<PathBuf, IdentityError> {
    let path = identity_path()?;
    let toml_str = crate::to_toml_string(identity)?;
    std::fs::write(&path, toml_str)?;
    Ok(path)
}

/// Check if an identity file exists at the configured location.
pub fn identity_exists() -> bool {
    identity_path().map(|p| p.exists()).unwrap_or(false)
}

/// Remove the identity file (logout).
pub fn remove_identity() -> Result<(), IdentityError> {
    let path = identity_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
