//! # Forge connection policy
//!
//! What a Forge server URL must be before a key is sent to it, and the
//! origin a remembered key is bound to.
//!
//! - **Scheme.** `https`, or `http` only to this machine or a private network
//!   (loopback, RFC 1918, link-local, IPv6 unique-local, `localhost` and
//!   `.local` names). An API key sent over plain `http` across the internet
//!   can be read on the way.
//! - **No redirects.** reqwest re-sends a request's body on a 307 or 308, so a
//!   redirect would carry the API key to whatever host the server names.
//!   [`http_client`] follows none.
//! - **Origin.** A remembered key belongs to the scheme, host and port it was
//!   saved for ([`Origin`]); a caller compares origins before sending it.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::{redirect, Client, Url};

/// Scheme, lowercase host and port (the scheme's default filled in): what a
/// remembered key is bound to. Two URLs that differ only in letter case, an
/// explicit default port or their path have the same origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
    }
}

/// A Forge server URL that passed the policy, and its origin.
#[derive(Debug, Clone)]
pub struct ForgeUrl {
    pub url: Url,
    pub origin: Origin,
}

/// Check `text` against the policy. `Err` is a sentence to show.
pub fn check_url(text: &str) -> Result<ForgeUrl, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter the Forge server URL.".to_string());
    }
    let url = Url::parse(text).map_err(|e| format!("{text} is not a URL: {e}"))?;
    let scheme = url.scheme().to_string();
    if scheme != "https" && scheme != "http" {
        return Err(format!("A Forge server URL starts with https:// (this one uses {scheme})."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("A Forge server URL cannot carry a user name or password; enter the key in the key field.".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("A Forge server URL cannot have a ? or # part.".to_string());
    }
    let host = match url.host_str() {
        Some(host) if !host.is_empty() => host.to_ascii_lowercase(),
        _ => return Err(format!("{text} names no server.")),
    };
    if scheme == "http" && !is_local(&host) {
        return Err(format!(
            "Forge keys are sent only over https, or over http to this computer or a private network. Use https://{}.",
            host
        ));
    }
    let port = url.port_or_known_default().ok_or_else(|| format!("{text} has no port."))?;
    Ok(ForgeUrl { origin: Origin { scheme, host, port }, url })
}

/// The origin of `text`, if it passes the policy.
pub fn origin_of(text: &str) -> Option<Origin> {
    check_url(text).ok().map(|u| u.origin)
}

/// This computer or a private network: loopback, RFC 1918, link-local,
/// IPv6 unique-local, `localhost` and `.local` names. `host` is a URL's host
/// in lower case (IPv6 in brackets).
fn is_local(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            IpAddr::V6(v6) => {
                let first = v6.segments()[0];
                v6.is_loopback()
                    || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
                    || (first & 0xfe00) == 0xfc00 // unique-local fc00::/7
                    || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback() || v4.is_private() || v4.is_link_local())
            }
        };
    }
    let name = host.trim_end_matches('.');
    name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
}

/// The HTTP client every Forge call uses: a 30 s timeout and no redirects.
pub fn http_client() -> reqwest::Result<Client> {
    Client::builder().timeout(Duration::from_secs(30)).redirect(redirect::Policy::none()).build()
}

/// The status line for a live connection: one made without a key says so,
/// so it is never mistaken for a signed-in one.
pub fn connected_line(url: &str, with_key: bool) -> String {
    if with_key {
        format!("Connected to {url}")
    } else {
        format!("Connected without a key to {url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_origin_ignores_case_an_explicit_default_port_and_the_path() {
        let a = origin_of("https://Forge.Example.com").unwrap();
        assert_eq!(origin_of("https://forge.example.com:443/api/").unwrap(), a);
        assert_eq!(origin_of("HTTPS://FORGE.EXAMPLE.COM/").unwrap(), a);
        assert_eq!(a.to_string(), "https://forge.example.com:443");
        assert_eq!(origin_of("http://localhost").unwrap(), origin_of("http://LOCALHOST:80").unwrap());
    }

    #[test]
    fn a_different_scheme_host_or_port_is_another_origin() {
        let a = origin_of("https://forge.example.com").unwrap();
        for other in [
            "https://forge.example.org",
            "https://evil.forge.example.com",
            "https://forge.example.com:8443",
            "http://192.168.1.5",
        ] {
            assert_ne!(origin_of(other).unwrap(), a, "{other}");
        }
        assert_ne!(origin_of("http://127.0.0.1:8080").unwrap(), origin_of("https://127.0.0.1:8080").unwrap());
    }

    #[test]
    fn plain_http_is_refused_for_a_public_host_and_accepted_on_a_local_one() {
        for public in ["http://forge.example.com", "http://8.8.8.8", "http://172.32.0.1", "http://[2001:db8::1]"] {
            assert!(check_url(public).is_err(), "{public} must be refused");
        }
        for local in [
            "http://localhost:4646",
            "http://127.0.0.1",
            "http://192.168.1.20:8080",
            "http://10.0.0.7",
            "http://172.16.4.4",
            "http://169.254.3.3",
            "http://[::1]:4646",
            "http://[fe80::1]",
            "http://[fd00::5]",
            "http://forge.local",
            "http://forge.localhost",
        ] {
            assert!(check_url(local).is_ok(), "{local} must be accepted: {:?}", check_url(local).err());
        }
        assert!(check_url("https://forge.example.com").is_ok());
    }

    #[test]
    fn other_schemes_credentials_and_queries_are_refused() {
        for bad in [
            "",
            "forge.example.com",
            "ftp://forge.example.com",
            "file:///etc/passwd",
            "https://user:pw@forge.example.com",
            "https://forge.example.com/?next=x",
            "https://forge.example.com/#x",
        ] {
            assert!(check_url(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn a_connection_without_a_key_says_so() {
        assert_eq!(connected_line("https://f", true), "Connected to https://f");
        assert!(connected_line("https://f", false).contains("without a key"));
    }

    /// A 307 to another server is not followed: the request (and the key in
    /// its body) never reaches the second server.
    #[test]
    fn a_redirect_is_not_followed() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        let target_port = target.local_addr().unwrap().port();
        target.set_nonblocking(true).unwrap();
        let reached = Arc::new(AtomicBool::new(false));

        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let first_port = first.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = first.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let reply = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{target_port}/api/auth\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let status = rt.block_on(async {
            http_client()
                .unwrap()
                .post(format!("http://127.0.0.1:{first_port}/api/auth"))
                .body("{\"api_key\":\"fk-secret\"}")
                .send()
                .await
                .map(|r| r.status().as_u16())
        });
        // Anything that was going to follow the redirect has done so by now.
        std::thread::sleep(Duration::from_millis(200));
        if target.accept().is_ok() {
            reached.store(true, Ordering::SeqCst);
        }
        assert_eq!(status.ok(), Some(307), "the 307 comes back to the caller");
        assert!(!reached.load(Ordering::SeqCst), "the redirect was followed: the key went to the second server");
    }
}
