// =============================================================================
// Lob API client (Business > Marketing, postal mail)
// =============================================================================
//
// Studio checks a user's own Lob API key here. The key is the user's: Lob bills
// their account for postage, and Studio calls api.lob.com directly, so the key
// never passes through the Eustress Worker.
//
// Security conditions (2026-10-08): the fixed HTTPS base URL, redirects off, a
// total request timeout, a connect timeout, and neither the key nor the
// Authorization header in any error, log or telemetry. This module logs nothing,
// and its error type carries no payload, so a failure cannot leak the key.
//
// Blocking by design. Callers run it on a background thread.
// =============================================================================

use std::time::Duration;

/// The only Lob host this module talks to.
pub const LOB_BASE_URL: &str = "https://api.lob.com";

/// A read of the address list, used only to test that a key is accepted.
///
/// UNCONFIRMED COST: Lob's documentation does not say whether this call is billed or
/// sends mail, and its pricing page does not say either. Before the Test button ships
/// to users, confirm with Lob support or with a check on McKale's own account. Until
/// then, do not describe this check as free.
const CHECK_PATH: &str = "/v1/addresses?limit=1";

/// The total budget for one check: the whole request, as ureq's overall timeout
/// bounds it. DNS resolution cannot be interrupted, so it is the one part ureq
/// does not bound tightly.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// The budget for the connect phase (TCP and TLS). ureq's overall timeout does not
/// cover it, and without this it waits up to 30 s before the overall deadline is
/// even consulted.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The answer to "does this key work?". It carries no payload, so it can be shown,
/// logged or compared without ever touching the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobKeyCheck {
    /// Lob accepted the key (HTTP 200).
    Ok,
    /// Lob refused the key (HTTP 401 or 403).
    Rejected,
    /// No usable answer: a timeout fired, the connection failed, a redirect
    /// (never followed), or any other status.
    Unreachable,
}

/// Checks a Lob API key against api.lob.com. Blocks for at most `CHECK_TIMEOUT`.
/// An empty key is rejected without a request.
pub fn check_lob_key(key: &str) -> LobKeyCheck {
    if key.trim().is_empty() {
        return LobKeyCheck::Rejected;
    }
    check_at(LOB_BASE_URL, key, CHECK_TIMEOUT)
}

/// The agent every check uses. One place sets the bounds, so the test reads them here.
fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(timeout)
        .timeout_connect(CONNECT_TIMEOUT)
        .redirects(0)
        .build()
}

/// The check against any base URL, with any timeout. Private: the public function
/// is fixed to Lob's HTTPS host. Tests reach this with a local listener.
fn check_at(base_url: &str, key: &str, timeout: Duration) -> LobKeyCheck {
    let agent = build_agent(timeout);
    let url = format!("{}{}", base_url, CHECK_PATH);
    let request = agent.get(&url).set("Authorization", &basic_auth(key));
    match request.call() {
        Ok(resp) if resp.status() == 200 => LobKeyCheck::Ok,
        Ok(_) => LobKeyCheck::Unreachable,
        Err(ureq::Error::Status(401 | 403, _)) => LobKeyCheck::Rejected,
        Err(_) => LobKeyCheck::Unreachable,
    }
}

/// Lob's basic auth: the key is the user name and the password is empty.
fn basic_auth(key: &str) -> String {
    format!("Basic {}", base64_encode(format!("{}:", key).as_bytes()))
}

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648). Written here so the module needs no new
/// crate; the engine's lock has base64 only as a transitive dependency.
fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Instant;

    #[test]
    fn the_public_check_is_fixed_to_lobs_https_host() {
        assert_eq!(LOB_BASE_URL, "https://api.lob.com");
    }

    #[test]
    fn an_empty_key_is_rejected_without_a_request() {
        let started = Instant::now();
        assert_eq!(check_lob_key(""), LobKeyCheck::Rejected);
        assert_eq!(check_lob_key("   "), LobKeyCheck::Rejected);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    /// The bounds, read from the agent's Debug output the way ureq's own test reads
    /// them. Removing the `timeout_connect` line makes the output say `Some(30s)`,
    /// so this test fails.
    #[test]
    fn the_agent_bounds_connect_and_the_whole_request() {
        let debug = format!("{:?}", build_agent(CHECK_TIMEOUT));
        assert!(debug.contains("timeout_connect: Some(10s)"), "the connect phase is not bounded at 10 s");
        assert!(debug.contains("timeout: Some(15s)"), "the whole request is not bounded at 15 s");
        assert!(debug.contains("redirects: 0"), "redirects are not off");
    }

    /// Accepts one connection, reads the request, and answers with `status_line`.
    /// `extra` adds header lines.
    fn serve_once(status_line: &str, extra: &str) -> String {
        let status = status_line.to_string();
        let extra = extra.to_string();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let reply = format!(
                    "HTTP/1.1 {}\r\n{}Content-Length: 0\r\nConnection: close\r\n\r\n",
                    status, extra
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        format!("http://{}", addr)
    }

    /// Accepts connections forever, counts each one before answering, and answers
    /// 200 OK. Any count above zero means the client followed a redirect to here.
    fn counting_server() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if let Ok(mut s) = stream {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let mut buf = [0u8; 4096];
                    let _ = s.read(&mut buf);
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        });
        (format!("http://{}", addr), hits)
    }

    #[test]
    fn a_valid_key_is_ok() {
        let base = serve_once("200 OK", "");
        assert_eq!(check_at(&base, "live_test", CHECK_TIMEOUT), LobKeyCheck::Ok);
    }

    #[test]
    fn a_refused_key_is_rejected_on_401_and_403() {
        let base = serve_once("401 Unauthorized", "");
        assert_eq!(check_at(&base, "k", CHECK_TIMEOUT), LobKeyCheck::Rejected);
        let base = serve_once("403 Forbidden", "");
        assert_eq!(check_at(&base, "k", CHECK_TIMEOUT), LobKeyCheck::Rejected);
    }

    /// The redirect target answers 200 OK. If the client followed the redirect, the
    /// check would return Ok and the counter would rise, so either assertion fails.
    #[test]
    fn a_redirect_is_not_followed() {
        let (second, hits) = counting_server();
        let location = format!("Location: {}/elsewhere\r\n", second);
        let first = serve_once("302 Found", &location);
        assert_eq!(check_at(&first, "k", CHECK_TIMEOUT), LobKeyCheck::Unreachable);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "the redirect target was contacted");
    }

    #[test]
    fn a_server_error_is_unreachable() {
        let base = serve_once("500 Internal Server Error", "");
        assert_eq!(check_at(&base, "k", CHECK_TIMEOUT), LobKeyCheck::Unreachable);
    }

    /// A listener that accepts and never answers: the case the timeout exists for.
    /// The check runs with a 1 s bound on its own thread, so a missing timeout means
    /// the answer never arrives and the 10 s watchdog below fails the test instead of
    /// hanging the run. The production bound, checked here, is 15 s at most.
    #[test]
    fn a_silent_server_is_unreachable_within_the_bound() {
        assert!(CHECK_TIMEOUT <= Duration::from_secs(15), "the production bound must stay within 15 s");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming() {
                if let Ok(mut s) = stream {
                    let mut buf = [0u8; 4096];
                    let _ = s.read(&mut buf);
                    held.push(s); // keep the socket open; never write
                }
            }
        });
        let base = format!("http://{}", addr);
        let bound = Duration::from_secs(1);
        let watchdog = Duration::from_secs(10);
        let (tx, rx) = mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let _ = tx.send(check_at(&base, "k", bound));
        });
        let answer = rx
            .recv_timeout(watchdog)
            .expect("the check did not return: the total timeout is missing or ineffective");
        assert_eq!(answer, LobKeyCheck::Unreachable);
        assert!(started.elapsed() < watchdog);
    }
}
