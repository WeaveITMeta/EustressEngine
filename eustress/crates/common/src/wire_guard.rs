//! Guards for the local sockets that read newline-delimited JSON: the
//! engine bridge and the Player's agent-control port.
//!
//! A web page can reach a loopback port. A browser's POST (text/plain needs
//! no preflight) arrives as a request line, headers, a blank line, then the
//! body. A socket that answers a bad line and keeps reading runs that body,
//! so both sockets close on an HTTP-looking first line and after any frame
//! they cannot parse. A frame is also capped, so a peer that never sends a
//! newline cannot grow a reader's buffer until the process runs out of
//! memory.

/// The largest frame either socket reads, newline included. Requests are
/// small JSON objects; this leaves room for large parameters without letting
/// one unterminated line take the process down.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// The largest frame a socket that authenticates reads before a frame has
/// passed its check: an unauthenticated peer gets no more than this read.
pub const MAX_UNAUTHENTICATED_FRAME_BYTES: usize = 64 * 1024;

/// True for a line that opens an HTTP request: it starts with a method, or
/// names an HTTP version. A JSON frame starts with `{` and never is.
pub fn looks_like_http(line: &str) -> bool {
    const METHODS: [&str; 9] = ["GET", "POST", "PUT", "OPTIONS", "HEAD", "DELETE", "PATCH", "CONNECT", "TRACE"];
    if line.starts_with('{') {
        return false;
    }
    let word = line.split_ascii_whitespace().next().unwrap_or("");
    METHODS.iter().any(|m| word.eq_ignore_ascii_case(m)) || line.contains("HTTP/1.") || line.contains("HTTP/2")
}

/// Read one frame from `reader` into `buf` (cleared first), up to and
/// including its newline, reading at most `cap` + 1 bytes. `Ok(None)` at end
/// of input; `Ok(Some(true))` for a frame within the cap; `Ok(Some(false))`
/// for one over it, which the caller refuses and closes.
pub fn read_frame<R: std::io::BufRead>(reader: &mut R, buf: &mut Vec<u8>, cap: usize) -> std::io::Result<Option<bool>> {
    use std::io::{BufRead as _, Read as _};
    buf.clear();
    let n = reader.by_ref().take(cap as u64 + 1).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(buf.len() <= cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_lines_are_recognised() {
        for line in ["GET / HTTP/1.1", "POST /rpc HTTP/1.1", "OPTIONS * HTTP/1.1", "get /x HTTP/1.0", "PRI * HTTP/2.0"] {
            assert!(looks_like_http(line), "{line}");
        }
        for line in [
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"http.request","params":{"url":"HTTP/1.1"}}"#,
            "hello",
            "X-Foo: bar",
        ] {
            assert!(!looks_like_http(line), "{line}");
        }
    }

    #[test]
    fn frames_are_read_whole_and_capped() {
        let mut buf = Vec::new();
        let mut input = std::io::Cursor::new(b"{\"a\":1}\n{\"b\":2}".to_vec());
        assert_eq!(read_frame(&mut input, &mut buf, MAX_FRAME_BYTES).unwrap(), Some(true));
        assert_eq!(buf, b"{\"a\":1}\n");
        assert_eq!(read_frame(&mut input, &mut buf, MAX_FRAME_BYTES).unwrap(), Some(true));
        assert_eq!(buf, b"{\"b\":2}", "a last frame without its newline is still a frame");
        assert_eq!(read_frame(&mut input, &mut buf, MAX_FRAME_BYTES).unwrap(), None);

        let mut endless = std::io::Cursor::new(vec![b'x'; MAX_FRAME_BYTES + 10]);
        assert_eq!(read_frame(&mut endless, &mut buf, MAX_FRAME_BYTES).unwrap(), Some(false));
        assert_eq!(buf.len(), MAX_FRAME_BYTES + 1, "reading stops one byte past the cap");

        let mut first = std::io::Cursor::new(vec![b'y'; MAX_UNAUTHENTICATED_FRAME_BYTES + 1]);
        assert_eq!(read_frame(&mut first, &mut buf, MAX_UNAUTHENTICATED_FRAME_BYTES).unwrap(), Some(false));
    }
}
