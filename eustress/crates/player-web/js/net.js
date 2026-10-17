// /player/net.js: the browser Player's WebTransport link to a host.
//
// The wasm module calls openLink() once it has parsed the join link
// (split_link_extras, check_api, JoinLink) and has the WebTransport URL,
// `https://<host>:<port>/join/<key>`, and the certificate pin. This file only
// moves bytes, as the host expects (eustress-networking native.rs):
// - the certificate is pinned by its SHA-256, so no CA or name check applies;
// - the Player opens ONE bidirectional stream as soon as the session is up
//   (the host waits 10 s for it); frames leave already prefixed by
//   wire::encode_frame, and arrive as raw stream bytes for
//   wire::FrameAssembler in the wasm module;
// - datagrams are required (requireUnreliable), with no stream fallback.
//
// THE CALLER MUST SEND THE HELLO RIGHT AFTER openLink RESOLVES. The host's
// accept_bi sees the stream only when its first bytes arrive (QUIC announces
// a stream with its first frame), and after 10 s with nothing sent the host
// closes the session with "no stream opened".
//
// The frame stream is the session's spine: when either pump stops (its
// reader ends or fails, or a handler throws), the session is closed and
// onClosed reports once. A failed open reports only by the code openLink
// throws, never also through onClosed. A datagram over the session's
// maximum costs only that datagram: sendDatagram throws "datagram-too-large",
// the caller drops it and carries on, and the session stays open.
//
// Sends queue in the order they are called: each write is queued on the
// stream before its promise is returned. close() finishes the frame stream
// first, so a frame queued just before it (a Goodbye) reaches the host, and
// closes the session once the stream is done or CLOSE_GRACE_MS has passed,
// as the desktop transport waits its close grace (native.rs).
//
// It never logs, stores or reports the URL (the join key is in its path) or
// the pin. Every failure it reports is one of the fixed codes in FAILURES.
//
// NOT RUN against a host yet.

export const FAILURES = Object.freeze([
  "no-webtransport", // this browser has no WebTransport
  "bad-arguments", // the URL is not https, or the pin is not 32 bytes
  "connect-failed", // the session did not open (refused, unreachable, wrong pin or key)
  "connect-timeout", // the session did not open in time
  "stream-failed", // the frame stream could not be opened
  "no-datagrams", // the session has no datagrams
  "datagram-too-large", // a datagram exceeds the session's maximum
  "closed", // the session closed
]);

const CONNECT_TIMEOUT_MS = 10_000;
// The desktop transport's CLOSE_GRACE: how long a close waits for the frame
// stream's queued frames before closing anyway.
const CLOSE_GRACE_MS = 250;

function failure(code) {
  const err = new Error(code);
  err.code = code;
  return err;
}

function withTimeout(promise, ms, code) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(failure(code)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

async function pump(reader, deliver) {
  for (;;) {
    const { value, done } = await reader.read();
    if (done) return;
    deliver(value);
  }
}

// Open a session. `pin` is the 32-byte SHA-256 of the host's certificate.
// Handlers: onStreamBytes(Uint8Array), onDatagram(Uint8Array), onClosed(code).
// Resolves to { sendFrame, sendDatagram, maxDatagramSize, close }.
// `connectTimeoutMs` is for tests; the host itself waits 10 s.
export async function openLink(url, pin, { onStreamBytes, onDatagram, onClosed }, { connectTimeoutMs = CONNECT_TIMEOUT_MS } = {}) {
  if (typeof WebTransport !== "function") throw failure("no-webtransport");
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    throw failure("bad-arguments");
  }
  if (parsed.protocol !== "https:" || !(pin instanceof Uint8Array) || pin.length !== 32) {
    throw failure("bad-arguments");
  }

  let transport;
  try {
    transport = new WebTransport(url, {
      serverCertificateHashes: [{ algorithm: "sha-256", value: pin }],
      requireUnreliable: true,
    });
  } catch {
    throw failure("connect-failed");
  }
  // A failed open reports only by the code thrown below, so these two settle
  // quietly until the session is up.
  transport.ready.catch(() => {});
  transport.closed.catch(() => {});

  try {
    await withTimeout(transport.ready, connectTimeoutMs, "connect-timeout");
  } catch (e) {
    transport.close();
    throw failure(e && e.code === "connect-timeout" ? "connect-timeout" : "connect-failed");
  }
  if (!transport.datagrams || !transport.datagrams.readable || !transport.datagrams.writable) {
    transport.close();
    throw failure("no-datagrams");
  }

  let stream;
  try {
    stream = await transport.createBidirectionalStream();
  } catch {
    transport.close();
    throw failure("stream-failed");
  }
  const frames = stream.writable.getWriter();
  const datagrams = transport.datagrams.writable.getWriter();

  // The session is up: from here its end is reported once, by a fixed code,
  // whether the host closes it, it fails, or either pump stops.
  let ended = false;
  const end = () => {
    if (ended) return;
    ended = true;
    try {
      transport.close();
    } catch {
      // Already closed.
    }
    onClosed("closed");
  };
  transport.closed.then(end, end);
  pump(stream.readable.getReader(), onStreamBytes).then(end, end);
  pump(transport.datagrams.readable.getReader(), onDatagram).then(end, end);

  return {
    get maxDatagramSize() {
      return transport.datagrams.maxDatagramSize;
    },
    // The write is queued before anything is awaited, so frames go out in
    // the order they were sent, a Goodbye before the close that follows it.
    // Nothing waits on the writer's `ready`, so a host that stops reading
    // grows this queue: a Player sends little (its Hello, input, remote
    // calls) and the session's own timeouts end such a host. A Player that
    // uploads large frames needs a cap on queued bytes.
    async sendFrame(bytes) {
      return frames.write(bytes);
    },
    async sendDatagram(bytes) {
      if (bytes.length > transport.datagrams.maxDatagramSize) throw failure("datagram-too-large");
      return datagrams.write(bytes);
    },
    async close() {
      // A stream that had already failed or closed counts as finished.
      const finished = frames.close().catch(() => {});
      let timer;
      const grace = new Promise((resolve) => {
        timer = setTimeout(resolve, CLOSE_GRACE_MS);
      });
      await Promise.race([finished, grace]);
      clearTimeout(timer);
      try {
        transport.close({ closeCode: 0, reason: "" });
      } catch {
        // Already closed.
      }
    },
  };
}
