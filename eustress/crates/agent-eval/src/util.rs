//! Small helpers with no dependencies: a stable hash, UTC timestamps,
//! percentiles and one-line excerpts.

use std::time::{SystemTime, UNIX_EPOCH};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, 64 bit. Not cryptographic; it only has to tell two world states
/// apart, and it is stable across runs and platforms.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_extend(FNV_OFFSET, bytes)
}

/// Continue an FNV-1a hash with more bytes.
pub fn fnv1a64_extend(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a point in time.
pub fn rfc3339_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Nearest-rank percentile of an ascending slice. `None` when empty.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    let idx = rank.clamp(1, n) - 1;
    Some(sorted[idx])
}

/// Round to 0.01 ms for the artifact.
pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// One line, at most `max_chars` characters, cut on a char boundary.
pub fn excerpt(s: &str, max_chars: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max_chars).collect();
    format!("{cut}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fnv_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn timestamps_are_utc_calendar_dates() {
        assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        // 2026-11-16T18:22:41Z
        let t = UNIX_EPOCH + Duration::from_secs(1_794_853_361);
        assert_eq!(rfc3339_utc(t), "2026-11-16T18:22:41Z");
        // A leap day.
        let t = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(rfc3339_utc(t), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn nearest_rank_percentiles() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&v, 50.0), Some(3.0));
        assert_eq!(percentile(&v, 95.0), Some(5.0));
        assert_eq!(percentile(&[7.0], 95.0), Some(7.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn excerpts_are_one_line_and_bounded() {
        assert_eq!(excerpt("a\nb", 10), "a b");
        assert_eq!(excerpt("abcdef", 3), "abc...");
    }
}
