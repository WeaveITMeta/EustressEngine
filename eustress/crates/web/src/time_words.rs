// =============================================================================
// Eustress Web - Times in the reader's own zone
// =============================================================================
// Every date and time the site shows is put in the reader's time zone and their
// locale's clock, the way the browser's Intl.DateTimeFormat does: en-US gives
// "Oct 31, 2026, 2:14 PM MST", a German browser a 24-hour clock. The zone's name
// is always shown, so a time never reads as the wrong zone. Where there is no
// browser (the prerender, the unit tests) the time is written in UTC, in the
// en-US shape, with "UTC" after it.
// =============================================================================

/// `Oct 31, 2026, 9:14 PM UTC` from milliseconds since 1970, or `None` for a value that is not a time.
pub fn utc_words(ms: f64) -> Option<String> {
    if !ms.is_finite() {
        return None;
    }
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|t| t.format("%b %-d, %Y, %-I:%M %p UTC").to_string())
}

/// The browser's rendering of `ms` in its own zone and locale, with the zone's short name. `None` when the browser has
/// no answer for the value.
#[cfg(not(feature = "ssr"))]
fn browser_words(ms: f64) -> Option<String> {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    if date.get_time().is_nan() {
        return None;
    }
    let options = js_sys::Object::new();
    for (key, value) in [("year", "numeric"), ("month", "short"), ("day", "numeric"), ("hour", "numeric"), ("minute", "2-digit"), ("timeZoneName", "short")] {
        let _ = js_sys::Reflect::set(&options, &key.into(), &value.into());
    }
    let locale = web_sys::window().and_then(|w| w.navigator().language()).filter(|l| !l.is_empty()).unwrap_or_else(|| "en-US".to_string());
    let text: String = date.to_locale_string(&locale, &options).into();
    (!text.is_empty()).then_some(text)
}

/// A time in the reader's zone: from milliseconds since 1970.
pub fn when_ms(ms: f64) -> String {
    #[cfg(not(feature = "ssr"))]
    if let Some(text) = browser_words(ms) {
        return text;
    }
    utc_words(ms).unwrap_or_default()
}

/// A time in the reader's zone: from seconds since 1970 (a Worker timestamp).
pub fn when_secs(secs: i64) -> String {
    when_ms(secs as f64 * 1000.0)
}

/// A time in the reader's zone: from an ISO 8601 text. The text itself when it is not a time.
pub fn when_iso(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(t) => when_ms(t.timestamp_millis() as f64),
        Err(_) => iso.to_string(),
    }
}

/// The reader's time zone as the browser names it (`America/Phoenix`), or `None` where there is no browser or it says nothing.
pub fn browser_zone() -> Option<String> {
    #[cfg(not(feature = "ssr"))]
    {
        let options = js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new()).resolved_options();
        let zone = js_sys::Reflect::get(&options, &"timeZone".into()).ok()?.as_string()?;
        return (!zone.is_empty()).then_some(zone);
    }
    #[cfg(feature = "ssr")]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_browser_a_time_is_written_in_utc_on_a_twelve_hour_clock_with_the_zone_named() {
        // 2026-10-31T21:14:00Z
        assert_eq!(utc_words(1_793_481_240_000.0).as_deref(), Some("Oct 31, 2026, 9:14 PM UTC"));
        // Noon and midnight read as 12 PM and 12 AM, never as 0 or 24.
        assert_eq!(utc_words(1_793_448_000_000.0).as_deref(), Some("Oct 31, 2026, 12:00 PM UTC"));
        assert_eq!(utc_words(1_793_404_800_000.0).as_deref(), Some("Oct 31, 2026, 12:00 AM UTC"));
        assert_eq!(utc_words(f64::NAN), None);
        assert_eq!(utc_words(f64::INFINITY), None);
    }

    #[test]
    fn an_iso_time_and_a_worker_timestamp_say_the_same_thing_and_text_that_is_not_a_time_is_kept() {
        assert_eq!(when_iso("2026-10-31T21:14:00.000Z"), "Oct 31, 2026, 9:14 PM UTC");
        assert_eq!(when_iso("2026-10-31T14:14:00-07:00"), "Oct 31, 2026, 9:14 PM UTC", "an offset is converted, not shown as written");
        assert_eq!(when_secs(1_793_481_240), "Oct 31, 2026, 9:14 PM UTC");
        assert_eq!(when_iso("soon"), "soon");
        assert_eq!(when_ms(f64::NAN), "");
    }
}
