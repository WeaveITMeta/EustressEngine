//! # Join links
//!
//! Everything a player needs to reach a host, as one string:
//!
//! ```text
//! eustress-player://join/192.168.1.20:7777?key=Qx7…&pin=3f9a…
//! ```
//!
//! `eustress-player://` is the Player's own URL scheme, so a link clicked
//! anywhere opens the Player; `eustress://` belongs to Studio. Both parse.
//!
//! - `host:port` — where the host listens.
//! - `key` — the session's join secret. The host refuses any session whose
//!   request path does not carry it, before a connection exists.
//! - `pin` — SHA-256 of the host's certificate, 64 hex characters. The host
//!   mints a fresh self-signed ECDSA P-256 certificate each session, valid 14
//!   days, which is exactly what a browser's `serverCertificateHashes` accepts,
//!   so the same pin serves a desktop player and a browser one.
//!
//! Also accepted: the short form `eustress://host:port`, a bare `host:port`,
//! and the WebTransport URL itself, `https://host:port/join/<key>`. A link
//! without a pin can only reach a host on this machine (see
//! [`JoinLink::is_loopback`]); anywhere else the certificate has to be pinned.

/// Request path prefix a host serves sessions under.
pub const JOIN_PATH: &str = "/join/";
/// Request path prefix of the host's echo probe, which measures what gets
/// through the path to it (see `native`). It takes the same key as a join.
pub const PROBE_PATH: &str = "/probe/";
/// The key a pin-less, key-less loopback join sends.
pub const OPEN_KEY: &str = "open";

/// A parsed join link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinLink {
    /// Hostname or IP. An IPv6 literal keeps its brackets.
    pub host: String,
    pub port: u16,
    pub key: Option<String>,
    pub pin: Option<[u8; 32]>,
}

impl JoinLink {
    /// Parse any accepted form. The error names what was wrong.
    pub fn parse(input: &str) -> Result<Self, String> {
        let s = input.trim();
        if s.is_empty() {
            return Err("empty join link".into());
        }
        if let Some(rest) = s.strip_prefix("https://") {
            // https://host:port/join/<key>
            let (authority, path) = match rest.find('/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, ""),
            };
            let (host, port) = split_host_port(authority)?;
            let key = key_from_path(path).map(str::to_owned);
            if let Some(k) = &key {
                check_key(k)?;
            }
            return Ok(Self { host, port, key, pin: None });
        }
        let rest = s
            .strip_prefix("eustress-player://join/")
            .or_else(|| s.strip_prefix("eustress-player://"))
            .or_else(|| s.strip_prefix("eustress://join/"))
            .or_else(|| s.strip_prefix("eustress://"))
            .unwrap_or(s);
        let (authority, query) = match rest.find('?') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, ""),
        };
        let (host, port) = split_host_port(authority.trim_end_matches('/'))?;
        let mut link = Self { host, port, key: None, pin: None };
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            match k {
                "key" => {
                    check_key(v)?;
                    link.key = Some(v.to_string());
                }
                "pin" => {
                    link.pin = Some(parse_hex32(v).ok_or_else(|| {
                        format!("pin must be 64 hex characters, got {} characters", v.len())
                    })?);
                }
                // Unknown parameters are ignored, so a newer host's link still
                // works with an older player.
                _ => {}
            }
        }
        Ok(link)
    }

    /// The canonical `eustress-player://join/…` form, which opens the Player.
    pub fn to_link(&self) -> String {
        let mut s = format!("eustress-player://join/{}:{}", self.host, self.port);
        let mut sep = '?';
        if let Some(k) = &self.key {
            s.push(sep);
            s.push_str("key=");
            s.push_str(k);
            sep = '&';
        }
        if let Some(p) = &self.pin {
            s.push(sep);
            s.push_str("pin=");
            s.push_str(&hex32(p));
        }
        s
    }

    /// The URL a WebTransport client opens.
    pub fn webtransport_url(&self) -> String {
        format!(
            "https://{}:{}{JOIN_PATH}{}",
            self.host,
            self.port,
            self.key.as_deref().unwrap_or(OPEN_KEY)
        )
    }

    /// True when the host is this machine, the one place a certificate can go
    /// unpinned: nothing sits between the two processes to intercept.
    pub fn is_loopback(&self) -> bool {
        let h = self.host.trim_start_matches('[').trim_end_matches(']');
        h.eq_ignore_ascii_case("localhost")
            || h == "::1"
            || h.parse::<std::net::Ipv4Addr>().map(|ip| ip.is_loopback()).unwrap_or(false)
    }
}

/// The join key a request path carries, if any.
pub fn key_from_path(path: &str) -> Option<&str> {
    key_after(JOIN_PATH, path)
}

/// The key a probe request path carries, if any.
pub fn probe_key_from_path(path: &str) -> Option<&str> {
    key_after(PROBE_PATH, path)
}

fn key_after<'a>(prefix: &str, path: &'a str) -> Option<&'a str> {
    let key = path.strip_prefix(prefix)?;
    let key = key.split(['?', '#', '/']).next().unwrap_or("");
    (!key.is_empty()).then_some(key)
}

fn check_key(k: &str) -> Result<(), String> {
    let ok = (4..=128).contains(&k.len())
        && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err("join key must be 4 to 128 letters, digits, '-' or '_'".into())
    }
}

fn split_host_port(authority: &str) -> Result<(String, u16), String> {
    let (host, port) = if let Some(end) = authority.find(']') {
        // [v6]:port
        let host = &authority[..=end];
        let port = authority[end + 1..]
            .strip_prefix(':')
            .ok_or_else(|| format!("missing port after {host}"))?;
        (host, port)
    } else {
        authority
            .rsplit_once(':')
            .ok_or_else(|| format!("missing :port in {authority:?}"))?
    };
    if host.is_empty() {
        return Err("missing host".into());
    }
    let port: u16 = port.parse().map_err(|_| format!("bad port {port:?}"))?;
    if port == 0 {
        return Err("port 0 is not joinable".into());
    }
    Ok((host.to_string(), port))
}

/// 32 bytes as 64 lowercase hex characters.
pub fn hex32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// 64 hex characters (either case, optional `:` separators) as 32 bytes.
pub fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let digits: Vec<u8> = s.bytes().filter(|&b| b != b':').collect();
    if digits.len() != 64 {
        return None;
    }
    let nib = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; 32];
    for (i, pair) in digits.chunks(2).enumerate() {
        out[i] = (nib(pair[0])? << 4) | nib(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_link_round_trips() {
        let link = JoinLink {
            host: "192.168.1.20".into(),
            port: 7777,
            key: Some("Qx7_abc-123".into()),
            pin: Some([0xab; 32]),
        };
        assert!(link.to_link().starts_with("eustress-player://join/192.168.1.20:7777?"));
        assert_eq!(JoinLink::parse(&link.to_link()).unwrap(), link);
        // Links Studio printed before the Player had its own scheme still parse.
        let studio = link.to_link().replacen("eustress-player://", "eustress://", 1);
        assert_eq!(JoinLink::parse(&studio).unwrap(), link);
        assert_eq!(link.webtransport_url(), "https://192.168.1.20:7777/join/Qx7_abc-123");
    }

    #[test]
    fn short_forms_parse() {
        let a = JoinLink::parse("127.0.0.1:7777").unwrap();
        assert_eq!((a.host.as_str(), a.port, a.key.is_none(), a.pin.is_none()), ("127.0.0.1", 7777, true, true));
        assert!(a.is_loopback());
        let b = JoinLink::parse("eustress://203.0.113.42:7777").unwrap();
        assert!(!b.is_loopback());
        let c = JoinLink::parse("https://[::1]:7777/join/secret123").unwrap();
        assert_eq!((c.host.as_str(), c.key.as_deref()), ("[::1]", Some("secret123")));
        assert!(c.is_loopback());
    }

    #[test]
    fn bad_links_say_why() {
        assert!(JoinLink::parse("").is_err());
        assert!(JoinLink::parse("host-without-port").is_err());
        assert!(JoinLink::parse("h:0").is_err());
        assert!(JoinLink::parse("h:7777?pin=abc").unwrap_err().contains("64 hex"));
        assert!(JoinLink::parse("h:7777?key=../../x").is_err());
    }

    #[test]
    fn pins_accept_dotted_hex() {
        let dotted = vec!["ab"; 32].join(":");
        assert_eq!(parse_hex32(&dotted), Some([0xab; 32]));
        assert_eq!(hex32(&[0x0f; 32]), "0f".repeat(32));
    }

    #[test]
    fn request_paths_yield_keys() {
        assert_eq!(key_from_path("/join/abc123"), Some("abc123"));
        assert_eq!(key_from_path("/join/abc123?x=1"), Some("abc123"));
        assert_eq!(key_from_path("/join/"), None);
        assert_eq!(key_from_path("/other"), None);
        assert_eq!(probe_key_from_path("/probe/abc123"), Some("abc123"));
        assert_eq!(probe_key_from_path("/join/abc123"), None);
        assert_eq!(key_from_path("/probe/abc123"), None, "a probe is never a join");
    }
}
