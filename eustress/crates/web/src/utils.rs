// =============================================================================
// Eustress Web - Utility Functions
// =============================================================================
// Table of Contents:
// 1. DOM Utilities
// 2. Format Utilities
// 3. Validation Utilities
// =============================================================================

use web_sys::Window;

/// Run `f` once the event being handled has finished travelling. A handler that closes the dialog it sits in must not
/// remove it during the click: the click goes on to the elements above (the overlay, a panel that stops propagation), whose
/// handlers the removal has just dropped, and wasm-bindgen throws "closure invoked recursively or after being dropped".
pub fn after_event(f: impl FnOnce() + 'static) {
    leptos::task::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        f();
    });
}

/// Whether the reader asked their system for less motion. The prerender has no page, so it says no.
#[cfg(not(feature = "ssr"))]
pub fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
        .map(|m| m.matches())
        .unwrap_or(false)
}

#[cfg(feature = "ssr")]
pub fn prefers_reduced_motion() -> bool {
    false
}

/// The query string of the page in front of the visitor ("?a=1", or empty). The
/// prerender has no page.
#[cfg(not(feature = "ssr"))]
pub fn current_query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

#[cfg(feature = "ssr")]
pub fn current_query() -> String {
    String::new()
}

/// The value of `name` in a query string ("?a=1&b=2"), or `None`. A value is
/// taken as it is written: the tokens this reads are hex, digits and dots.
pub fn query_value(query: &str, name: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Put `path` in the address bar in place of the current address (query
/// included), without loading anything and without adding a history entry.
#[cfg(not(feature = "ssr"))]
pub fn replace_path(path: &str) {
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(path));
    }
}

#[cfg(feature = "ssr")]
pub fn replace_path(_path: &str) {}

/// Ask the browser to send no Referer from here on, so a token on a page that was
/// opened from an email cannot travel to another site.
#[cfg(not(feature = "ssr"))]
pub fn hold_back_referrer() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let Some(head) = document.head() else { return };
    if let Ok(meta) = document.create_element("meta") {
        let _ = meta.set_attribute("name", "referrer");
        let _ = meta.set_attribute("content", "no-referrer");
        let _ = head.append_child(&meta);
    }
}

#[cfg(feature = "ssr")]
pub fn hold_back_referrer() {}

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

/// Whole number with thousands separators: 12345 becomes "12,345".
pub fn format_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Bliss at exactly two decimals with thousands separators: 1234.5 becomes
/// "1,234.50". The ledger stores whole cents, so two decimals is all of it.
pub fn format_bliss_exact(bls: f64) -> String {
    let cents = (bls.abs() * 100.0).round() as u64;
    let sign = if bls < 0.0 && cents > 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", format_count(cents / 100), cents % 100)
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

#[cfg(test)]
mod email_link_tests {
    use super::query_value;

    #[test]
    fn a_value_is_found_by_name_and_only_by_name() {
        assert_eq!(query_value("?token=abc.123&x=1", "token").as_deref(), Some("abc.123"));
        assert_eq!(query_value("x=1&token=t", "token").as_deref(), Some("t"));
        assert_eq!(query_value("?mytoken=a", "token"), None);
        assert_eq!(query_value("", "token"), None);
        assert_eq!(query_value("?token", "token"), None);
        assert_eq!(query_value("?token=", "token").as_deref(), Some(""));
    }
}
