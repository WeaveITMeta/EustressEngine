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

use crate::wire::MAX_TICKET_CHARS;

/// Request path prefix a host serves sessions under.
pub const JOIN_PATH: &str = "/join/";
/// Request path prefix of the host's echo probe, which measures what gets
/// through the path to it (see `native`). It takes the same key as a join.
pub const PROBE_PATH: &str = "/probe/";
/// The key a pin-less, key-less loopback join sends.
pub const OPEN_KEY: &str = "open";

/// A parsed join link. Its `Debug` never prints the join key, the session's
/// secret: everything that holds a link (a launch, a join target, a notice)
/// prints it safely.
#[derive(Clone, PartialEq, Eq)]
pub struct JoinLink {
    /// Hostname or IP. An IPv6 literal keeps its brackets.
    pub host: String,
    pub port: u16,
    pub key: Option<String>,
    pub pin: Option<[u8; 32]>,
}

impl std::fmt::Debug for JoinLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinLink")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("key", &format_args!("{}", if self.key.is_some() { "<set>" } else { "<none>" }))
            .field("pin", &self.pin.as_ref().map(hex32))
            .finish()
    }
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

/// [`JoinLink::parse`] for a link from outside: a launch argument, `--connect`,
/// or a link a browser page hands its Player. `parse`'s address errors are fixed
/// text, but one still quotes a bracketed host (`[..]`), and a crafted link can
/// put query text there (a `ticket=`, `pin=` or `key=`), which
/// [`split_link_extras`] never sees, since it takes out only what follows a `?`.
/// So an error that holds `=` or `&` is replaced by one that quotes nothing.
/// Both Players parse join links through this one function.
pub fn parse_join_link(text: &str) -> Result<JoinLink, String> {
    JoinLink::parse(text).map_err(|e| {
        if e.contains('=') || e.contains('&') { "this link is malformed".to_string() } else { e }
    })
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

/// An identity ticket a signed-in Play page put in a join link
/// (`docs/networking/PLAYER_JOIN_TICKET.md`): opaque, single use, bound to
/// the host's certificate pin. Nothing prints it: its `Debug` shows
/// `JoinTicket(..)`. It cannot be copied, and it leaves only through
/// [`JoinTicket::into_secret`], into one `Hello`.
#[derive(PartialEq, Eq)]
pub struct JoinTicket(String);

impl JoinTicket {
    /// The ticket, for the one `Hello` that sends it.
    pub fn into_secret(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for JoinTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JoinTicket(..)")
    }
}

/// What a Play page adds to a link for the Player alone: the viewer's ticket,
/// and the API the link belongs to. Never part of [`JoinLink`], so no
/// `to_link()`, host file or log line carries them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkExtras {
    pub ticket: Option<JoinTicket>,
    /// The base of the API the link belongs to, from `api=production` or
    /// `api=staging`.
    pub api: Option<&'static str>,
}

impl LinkExtras {
    /// A link names the API it belongs to, and a Player takes it only from
    /// that API: `player_api` is the one this Player is set up for (its
    /// `--api-url`, else production). A link for another refuses, so no web
    /// page can point a production Player at staging.
    pub fn check_api(&self, player_api: &str) -> Result<(), String> {
        // As calls use it: no trailing `/`, so `--api-url https://x/` matches.
        let player_api = eustress_common::api_base::validate_api_base(player_api)
            .unwrap_or_else(|_| player_api.trim().trim_end_matches('/').to_string());
        match self.api {
            Some(api) if api != player_api => Err(format!(
                "This link is for the {} server; this Player is set up for {}.",
                eustress_common::api_base::server_name(api),
                eustress_common::api_base::server_name(&player_api)
            )),
            _ => Ok(()),
        }
    }
}

/// The `ticket` and `api` of an `eustress-player://` (or `eustress://`) link,
/// a join or a `play/<id>` one; any other form carries none. See
/// [`split_link_extras`].
pub fn link_extras(input: &str) -> Result<LinkExtras, String> {
    split_link_extras(input).map(|(_, extras)| extras)
}

/// [`link_extras`], and the link with its ticket taken out: parse that, so
/// no error a later step reports can quote the ticket. Refuses an API name
/// other than `production` and `staging`, a ticket that is not 1 to
/// [`MAX_TICKET_CHARS`] characters of `A-Z a-z 0-9 . _ -`, and a link that
/// names either twice. No error quotes the ticket.
pub fn split_link_extras(input: &str) -> Result<(String, LinkExtras), String> {
    let s = input.trim();
    let scheme = ["eustress-player://", "eustress://"].into_iter().find(|p| s.starts_with(p));
    let (Some(_), Some((head, query))) = (scheme, s.split_once('?')) else {
        return Ok((s.to_string(), LinkExtras::default()));
    };
    let mut extras = LinkExtras::default();
    let mut kept: Vec<&str> = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "ticket" => {
                if extras.ticket.is_some() {
                    return Err("the link names its ticket twice; join again from the gallery".into());
                }
                let ok = !v.is_empty()
                    && v.len() <= MAX_TICKET_CHARS
                    && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
                if !ok {
                    return Err("the link's ticket is malformed; join again from the gallery".into());
                }
                extras.ticket = Some(JoinTicket(v.to_string()));
            }
            "api" => {
                if extras.api.is_some() {
                    return Err("the link names its server twice".into());
                }
                let base = eustress_common::api_base::api_for_name(v).ok_or_else(|| {
                    let shown: String = v.chars().filter(char::is_ascii_graphic).take(40).collect();
                    format!("this link is for an API this Player does not know ({shown:?}); it knows production and staging")
                })?;
                extras.api = Some(base);
                kept.push(pair);
            }
            _ => kept.push(pair),
        }
    }
    let link = if kept.is_empty() { head.to_string() } else { format!("{head}?{}", kept.join("&")) };
    Ok((link, extras))
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

/// Its errors quote nothing from the link but a bracketed host: a join key can
/// sit in the address's path (`host:port/join/<key>`) when no `?` comes first.
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
            .ok_or_else(|| "the link names no port".to_string())?
    };
    if host.is_empty() {
        return Err("missing host".into());
    }
    let port: u16 = port.parse().map_err(|_| "the link's port is not a number".to_string())?;
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

    /// The one address error that quotes the link, a bracketed host's, can carry
    /// query text a crafted link put there; the scrubbed error quotes nothing.
    /// An error with nothing to hide passes through.
    #[test]
    fn a_malformed_link_never_quotes_its_key_or_pin() {
        let key = "k3y-Secret_0123456789";
        let bad = format!("eustress-player://join/[203.0.113.7&key={key}&pin={}]", "ab".repeat(32));
        let raw = JoinLink::parse(&bad).expect_err("a bracketed host with no port");
        assert!(raw.contains(key), "the unscrubbed error quotes the key, so the test can fail: {raw}");
        let e = parse_join_link(&bad).expect_err("a malformed link is refused");
        assert_eq!(e, "this link is malformed");
        assert_eq!(parse_join_link("  ").expect_err("empty"), "empty join link");
        let ok = parse_join_link(&format!("eustress-player://join/203.0.113.7:7777?key={key}")).expect("a good link");
        assert_eq!((ok.port, ok.key.as_deref()), (7777, Some(key)));
    }

    /// A join key in a link's path, with no `?` before it, is never quoted: the
    /// port and no-port errors are fixed text, in every form a link takes
    /// (Security, 2026-10-15).
    #[test]
    fn a_key_in_the_path_is_never_quoted() {
        let key = "pathK3y-Secret_0123456789";
        let links = [
            format!("eustress://203.0.113.7:7777/join/{key}"),
            format!("eustress-player://203.0.113.7:7777/join/{key}"),
            format!("eustress-player://join/203.0.113.7:7777/join/{key}"),
            format!("203.0.113.7:7777/join/{key}"),
            format!("host.example/join/{key}"),
            format!("eustress://host.example/join/{key}?pin={}", "ab".repeat(32)),
        ];
        for bad in &links {
            let errors = [JoinLink::parse(bad).expect_err("a path is not a port"), parse_join_link(bad).expect_err("refused")];
            for e in errors {
                assert!(!e.contains(key), "{bad:?} gave an error quoting its key: {e}");
            }
        }
        assert_eq!(JoinLink::parse(&links[3]).unwrap_err(), "the link's port is not a number");
        assert_eq!(JoinLink::parse(&links[4]).unwrap_err(), "the link names no port");
    }

    /// A link's Debug shows where it goes and its pin, never its join key.
    #[test]
    fn a_links_debug_never_prints_its_key() {
        let key = "k3y-Secret_0123456789";
        let link = JoinLink { host: "203.0.113.7".into(), port: 7777, key: Some(key.into()), pin: Some([0xab; 32]) };
        let shown = format!("{link:?}");
        assert!(!shown.contains(key), "the key printed: {shown}");
        assert!(shown.contains("key: <set>") && shown.contains("203.0.113.7") && shown.contains("7777"), "{shown}");
        assert!(shown.contains(&"ab".repeat(32)), "the pin is shown: {shown}");
        let open = JoinLink { key: None, pin: None, ..link };
        assert!(format!("{open:?}").contains("key: <none>"));
    }

    /// A Play page's ticket and API ride in the Player's own link, never in
    /// `JoinLink`, and nothing prints the ticket.
    #[test]
    fn a_links_ticket_and_api_are_read_and_never_printed() {
        let secret = "eit1.c2VjcmV0LXRpY2tldA.c2ln-_x";
        let pin = "ab".repeat(32);
        let link = format!("eustress-player://join/203.0.113.7:7777?key=abcdef&pin={pin}&ticket={secret}&api=staging");
        let (rest, extras) = split_link_extras(&link).unwrap();
        assert_eq!(extras.api, Some("https://api-staging.eustress.dev"));
        assert!(!format!("{extras:?}").contains(secret), "Debug printed the ticket: {extras:?}");
        assert_eq!(rest, format!("eustress-player://join/203.0.113.7:7777?key=abcdef&pin={pin}&api=staging"));
        let parsed = JoinLink::parse(&rest).unwrap();
        assert_eq!((parsed.key.as_deref(), parsed.pin), (Some("abcdef"), Some([0xab; 32])));
        assert!(!JoinLink::parse(&link).unwrap().to_link().contains(secret), "the canonical link carried the ticket");
        assert_eq!(extras.ticket.map(JoinTicket::into_secret).as_deref(), Some(secret));
        let solo = link_extras("eustress-player://play/07d084d8-1c2b?api=production").unwrap();
        assert_eq!((solo.api, solo.ticket), (Some("https://api.eustress.dev"), None));
        // Only the Player's own scheme carries extras.
        for other in [format!("https://203.0.113.7:7777/join/abcdef?ticket={secret}"), "203.0.113.7:7777".to_string()] {
            let (rest, extras) = split_link_extras(&other).unwrap();
            assert_eq!((rest.as_str(), extras), (other.as_str(), LinkExtras::default()), "{other}");
        }
        assert_eq!(link_extras("eustress-player://join/203.0.113.7:7777?key=abcdef").unwrap(), LinkExtras::default());
        // A malformed link carrying a ticket fails, once the ticket is taken
        // out, with a message that cannot quote it.
        let (rest, _) = split_link_extras(&format!("eustress-player://join/h:1?pin=zz&ticket={secret}")).unwrap();
        let e = JoinLink::parse(&rest).unwrap_err();
        assert!(!e.contains(secret), "{e}");
    }

    /// A malformed ticket, an API the Player does not know, or either named
    /// twice refuses the link, and no message quotes the ticket.
    #[test]
    fn a_bad_ticket_or_an_unknown_api_refuses_the_link() {
        for bad in ["eit1.abc<script>", "eit1.abc%2F", "eit1 abc", ""] {
            let e = link_extras(&format!("eustress-player://join/h:1?ticket={bad}")).unwrap_err();
            assert!(bad.is_empty() || !e.contains("abc"), "{e}");
        }
        assert!(link_extras(&format!("eustress-player://join/h:1?ticket={}", "a".repeat(MAX_TICKET_CHARS + 1))).is_err());
        assert!(link_extras(&format!("eustress-player://join/h:1?ticket={}", "a".repeat(MAX_TICKET_CHARS))).is_ok());
        let e = link_extras("eustress-player://join/h:1?api=https://evil.example").unwrap_err();
        assert!(e.contains("production and staging"), "{e}");
        assert!(link_extras("eustress-player://play/abc?api=Staging").is_err(), "names are exact");
        let twice = link_extras("eustress-player://join/h:1?ticket=aaaa&ticket=bbbb").unwrap_err();
        assert!(!twice.contains("aaaa") && !twice.contains("bbbb"), "{twice}");
        assert!(link_extras("eustress-player://play/abc?api=staging&api=production").is_err());
    }

    /// A link for a server other than the Player's own is refused, so no web
    /// page can point a production Player at staging.
    #[test]
    fn a_link_for_another_server_is_refused() {
        let staging_link = link_extras("eustress-player://play/abc?api=staging").unwrap();
        let e = staging_link.check_api("https://api.eustress.dev").unwrap_err();
        assert_eq!(e, "This link is for the staging server; this Player is set up for production.");
        assert_eq!(staging_link.check_api("https://api-staging.eustress.dev"), Ok(()));
        assert_eq!(staging_link.check_api("https://api-staging.eustress.dev/"), Ok(()), "a trailing / is the same API");
        let production_link = link_extras("eustress-player://play/abc?api=production").unwrap();
        assert!(production_link.check_api("https://api-staging.eustress.dev").is_err());
        let unnamed = link_extras("eustress-player://play/abc").unwrap();
        assert_eq!(unnamed.check_api("https://api.eustress.dev"), Ok(()), "a link without api= takes the Player's own");
    }
}
