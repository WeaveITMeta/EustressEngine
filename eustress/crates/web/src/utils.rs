// =============================================================================
// Eustress Web - Utility Functions
// =============================================================================
// Table of Contents:
// 1. DOM Utilities
// 2. Format Utilities
// 3. Validation Utilities
// =============================================================================

use web_sys::Window;

// -----------------------------------------------------------------------------
// 1. DOM Utilities
// -----------------------------------------------------------------------------

/// Get the browser window object.
pub fn window() -> Window {
    web_sys::window().expect("No window object available")
}

/// Get the current URL pathname.
pub fn get_pathname() -> String {
    window()
        .location()
        .pathname()
        .unwrap_or_else(|_| "/".to_string())
}

/// Navigate to a new URL (full page reload).
pub fn navigate_to(url: &str) {
    let _ = window().location().set_href(url);
}

/// Get a query parameter from the URL.
pub fn get_query_param(name: &str) -> Option<String> {
    let search = window().location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get(name)
}

// -----------------------------------------------------------------------------
// 2. Format Utilities
// -----------------------------------------------------------------------------

/// Format a file size in bytes to human-readable string.
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a duration in milliseconds to human-readable string.
pub fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let minutes = ms / 60_000;
        let seconds = (ms % 60_000) / 1000;
        format!("{}m {}s", minutes, seconds)
    }
}

/// Truncate a string to a maximum length with ellipsis.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

// -----------------------------------------------------------------------------
// 3. Validation Utilities
// -----------------------------------------------------------------------------

/// Validate an email address (basic check).
pub fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    
    let local = parts[0];
    let domain = parts[1];
    
    !local.is_empty() 
        && !domain.is_empty() 
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

/// Validate a username (alphanumeric + underscore, 3-32 chars).
pub fn is_valid_username(username: &str) -> bool {
    let len = username.len();
    if len < 3 || len > 32 {
        return false;
    }
    
    username.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Validate password strength (min 8 chars).
pub fn is_valid_password(password: &str) -> bool {
    password.len() >= 8
}
