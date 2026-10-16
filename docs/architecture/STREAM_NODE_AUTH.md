# Authenticated, multi-party stream node

**Status:** decided 2026-10-09 (McKale chose the recommended option on all four decisions). Security reviewed this design on 2026-10-09, approved it, decided the browser path, approved the pinning verifier as one sanctioned `dangerous()` call, and recorded the three decisions under "Security decisions recorded". Landed in build 32 on 2026-10-15, where it compiles and its stream-node suites pass. Owner: Eustress Streams (`crates/stream-node/**`, engine `stream_node_plugin.rs`). Security reviews each staged file before it lands. Code lands only on the orchestrator's wave.

This document is the specification the implementation follows. "The node" means `eustress-stream-node` in every form: the standalone binary, the `ForgeCluster` nodes, and the engine's `StreamNodePlugin`.

## Decisions

1. **Encryption: TLS 1.3 on every network listener.** TCP, QUIC and REST use a certificate and private key that the operator supplies as PEM files. There is no plaintext mode, loopback included. The node never generates a certificate at startup.
2. **Enrollment: by public key, locally.** The operator enrolls each party by the Ed25519 public key in that party's identity file. The node makes no runtime call to the identity server.
3. **Loopback: every network connection logs in,** including the CLI, the MCP tools and the benches. The Unix socket and shared-memory transports are not in any shipped build (see Local transports).
4. **Grants: explicit per topic, default deny.** Publishing to a reserved topic also requires `admin = true`.

## Security requirements

Each requirement is specified in the section named in brackets.

1. TLS 1.3 only on every listener, and 0-RTT disabled on QUIC. [Transport]
2. Every client pins the node's certificate, loopback included. There is no option to skip verification, and no accept-any verifier anywhere in the crate. The handshake signature is verified for real, so a pinned certificate presented with another key is refused. The crate has exactly one `dangerous()` call, which installs the pinning verifier. [Transport]
3. Transcripts length-prefix every variable field, and each channel has its own label. [Login transcript]
4. Pre-login limits: the 5-second deadline counts from accept and includes the TLS handshake; 16 pre-login connections per source IP; `party_id` format is checked before any lookup; warn lines are rate-limited. Refusals by a cap are retried with jitter by native clients. [Limits before login]
5. **The REST login always requires the certificate binding, so only native clients can log in.** Browsers never log in and never hold a party key. A browser uses a short-lived token that a native login minted. [REST and SSE]
6. At startup the node verifies its private key, `parties.toml` and the folder holding it, and refuses to start its listeners on any failure. The owner-only helper moves from the engine into `eustress-identity`. [Files and permissions]
7. The admin flag does not replace the materializer's own path checks. [Authorization]
8. The Unix socket and shared-memory transports compile only under a non-default feature for the benches, and no shipped build enables it. [Local transports]
9. A default-configured engine opens no stream port. [Engine integration]
10. Revocation, key changes and grant removal end every connection, session, token and stream that depends on them, including one blocked on a write. [Authorization, Reply discipline]
11. Per-address and per-party limits cannot be evaded by address choice, by IPv6 prefix, or by holding a slot before the address is validated. [Limits before login, Transport]
12. Replies stay in request order, and a cancelled client call cannot corrupt the reply queue. [Reply discipline]

## Security decisions recorded

- **One sanctioned `dangerous()` call.** Approved (2026-10-09). rustls routes every custom verifier through `dangerous()`; the name flags the risk and does not forbid a correct verifier. The alternative, a root-store pin with no `dangerous()` call, was rejected: it would cost the rotation promise and buy nothing a correct pinned verifier lacks.
- **No certificate date check.** Recorded under Transport, next to the verifier.
- **16 outstanding REST challenges per source IP, with a 429 answer at the cap.** Native clients log in at start and then every hour, and each challenge lives 5 seconds, so 16 at once from one address is already a burst. Retries use jitter, at most three attempts.
- **Unix socket and shared-memory transports removed from every non-bench build.** See Local transports.

## Transport

- **TLS 1.3 only.** The server configuration and every client configuration admit TLS 1.3 and nothing older. A TLS 1.2 exporter is unique only with the extended master secret, so allowing TLS 1.2 would bring back the relay that the exporter prevents.
- **QUIC early data is off in both directions.** The server accepts no 0-RTT data, and clients send none.
- **Pinning on every client, loopback included.** Each client configuration holds the SHA-256 hash of the node certificate's SubjectPublicKeyInfo (SPKI). The pin is on the leaf certificate, and the node presents one certificate. The SPKI hash survives a renewal that keeps the key. A client refuses any other certificate. There is no skip option.
- **The handshake signature is verified for real.** This is the most important line in the transport. `verify_tls13_signature` delegates to `rustls::crypto::verify_tls13_signature` with the crypto provider's signature verification algorithms. It never returns a bare `Ok`. A pin check alone accepts anyone who replays the public certificate, because the certificate is public. `verify_tls12_signature` returns an error, since TLS 1.2 is never accepted.
- **The one sanctioned `dangerous()` call.** rustls 0.23 installs a custom certificate verifier only through `dangerous()`. The pinning verifier therefore needs exactly one such call, in `transport.rs`, which installs `PinnedServerVerifier`. The ratchet test enforces that: exactly one `dangerous()` call in the crate, exactly one verifier type, `verify_tls13_signature` as described above, and `verify_tls12_signature` returning an error.
- **No certificate date check. Do not add one.** The verifier ignores certificate validity dates on purpose. With an SPKI pin, the pinned private key is the trust anchor. Whoever holds that key can mint a certificate with any dates, so a date check adds no security against a key compromise. What it adds is an outage on the day an operator forgets to renew. A compromised key is handled by the planned pin change under Certificate rotation, not by a date check. Anyone who proposes a date check to "fix" this must first show a case that a date check stops and a pin change does not.
- **The alternative that was rejected.** A root store holding only the node certificate would avoid `dangerous()`, but every certificate renewal would then change the pin. That costs the rotation promise and buys nothing a correct pinned verifier lacks.
- **The certificate is operator-supplied PEM.** The node refuses to start its listeners if the certificate or key is missing. Test code may generate a certificate. Production code never does.
- The node presents the first certificate in the PEM file. Other certificates in the file are ignored with a warning.
- **The certificate file is read by path.** It is public, and operators may keep certificates in shared folders, so the owner-only check does not apply to it. Its integrity is covered twice: clients pin the SPKI, and the node refuses a certificate that does not pair with the owner-only key. Someone who can write the file can only stop the node. At startup the node logs the loaded certificate's SPKI pin at info, so an operator can see which certificate is live.
- **QUIC address validation.** The QUIC listener answers an address it has not validated with a Retry. Only a validated address takes a pre-login slot, and the slot is taken after validation, never before.
- **QUIC datagrams are disabled.** The node never reads datagrams, so the datagram receive buffer is set to none on every QUIC endpoint.

## Wire protocol (TCP and QUIC)

Existing frames do not change. Two new frame pairs are **appended to the end** of the enums, because bincode encodes a variant by its position. Inserting in the middle would break every older peer.

Appended to `ClientFrame`:

- `Hello { protocol_version: u16, party_id: String, signature: Vec<u8> }`

Appended to `ServerFrame`:

- `Challenge { node_id: String, nonce: [u8; 32] }`
- `Welcome { party_id: String }`
- `AuthFailed { reason: String }`, where `reason` is one of two fixed strings. `"auth_failed"` covers an unknown party, a malformed `party_id`, a disabled party and a bad signature, so a response never reveals which parties exist. `"version_unsupported"` covers a protocol mismatch.
- `Subscribed { head: u64 }`, the success reply to `Subscribe`. `head` is the topic's head when the subscription starts. It is written before any `Message` for that subscription, replays included.

The protocol version is 1. A `Hello` sent after login gets `Error` with code 400.

## Reply discipline

Every frame that expects a reply gets exactly one reply, and the replies come back in the order the frames arrived:

| Request | Reply on success | Reply on refusal |
|---|---|---|
| `Publish` | `Ack` | `Error` |
| `PublishBatch` | `BatchAck` | `Error` |
| `PublishBatchTopic` | `BatchAckCompact` | `Error` |
| `Subscribe` | `Subscribed` | `Error` |
| `ListTopics` | `TopicList` | none, because it is not refused |
| `Ping` | `Pong` | none, because it is not refused |

Frames that expect no reply (`PublishNoAck`, `PublishBatchNoAck`, `Unsubscribe`) never get one, refused or not. `Message` frames are not replies: they go to a subscription and never complete a request.

A client keeps one first-in, first-out queue of its pending requests across all kinds. A reply completes the front request. An `Error` completes the front request with the error. A reply whose kind does not match the front request is a protocol error, and the client closes the connection. A refused `Subscribe` removes the subscriber's channel.

This needs no request identifiers. The transport is ordered, and every reply-expecting request gets exactly one reply.

**Ordering on the node.** The node handles reply-expecting requests one at a time per connection, in arrival order, and writes each reply before it reads the next such request. Nothing that expects a reply runs on a spawned task or behind an await that lets a later reply go out first. Replays and live `Message` frames go through the subscription's forwarding task and may interleave freely with replies.

**One writer per client connection.** The client sends its requests through one writer task, fed by a channel. For each request the task adds the request to the queue and then writes its frame, in that order and without interruption. A caller dropped before its request reaches the channel never sends it. A caller dropped after the send loses only its own reply.

**Writes have a deadline.** Every write to a peer has a 10-second deadline, on node connections, forwarders, REST responses and client writes alike. A write that exceeds it closes the connection, so a peer that stops reading cannot hold a slot, a subscription or a party's quota open.

**Forwarders and unsubscribe.** Unsubscribe stops the subscription's forwarder. The forwarder checks its stop flag after it has taken the writer lock and before it writes, never before. A check made before the lock would allow this order: the forwarder sees no stop, Unsubscribe sets it, a new `Subscribe` writes `Subscribed`, and then the old forwarder writes a stale `Message` after `Subscribed`. With the check under the lock, every frame the old forwarder writes goes out before `Subscribed`, and the client drops it as unregistered. The forwarder is not aborted, because an aborted write can leave half a frame on the wire. Closing a connection may abort its forwarders, because the stream closes with it.

**Subscription channels.** A client registers a subscription's channel when its `Subscribed` reply arrives, not when it sends `Subscribe`. Every `Message` that precedes that reply on the wire, including one from an earlier subscription to the same topic, is dropped, so a stale message cannot reach a new subscription.

## Login transcript

The signature is Ed25519 over the transcript below. Each variable-length field is preceded by its length as a two-byte little-endian unsigned integer. Fixed-length fields carry no prefix.

| Field | Encoding | TCP | QUIC | REST |
|---|---|---|---|---|
| label | fixed ASCII bytes, then `0x00` | `eustress-stream-login-v1` | `eustress-stream-login-quic-v1` | `eustress-stream-login-rest-v1` |
| node_id | u16 LE length, then UTF-8 (at most 64 bytes) | yes | yes | yes |
| nonce | 32 bytes | yes | yes | yes |
| channel binding | see below | exporter | exporter | certificate SHA-256 |
| party_id | u16 LE length, then UTF-8 (1 to 64 bytes) | yes | yes | yes |
| protocol_version | 2 bytes, LE | yes | yes | yes |
| token_class | 1 byte (REST only) | no | no | yes: `0x01` session, `0x02` browser |

Channel binding:

- **TCP:** 32 bytes of the TLS 1.3 exporter, label `eustress-stream-login-v1`, empty context.
- **QUIC:** 32 bytes of the QUIC connection's TLS 1.3 exporter, label `eustress-stream-login-quic-v1`, empty context.
- **REST:** 32 bytes of the SHA-256 of the node's DER certificate. The node rebuilds the transcript with the hash of its own certificate. A client that saw a different certificate signed a different transcript, so the signature fails.

The exporter prevents a relay. A man in the middle that terminates TLS toward the client and opens its own TLS to the node gets a different exporter on each leg, so a signature made on one leg fails on the other. The distinct labels stop a signature made on one channel from being presented on another.

## Login sequence (TCP and QUIC)

1. On accept, the node starts the login deadline (see Limits before login) and completes TLS. For QUIC, the login runs on the first bidirectional stream.
2. The node sends `Challenge` with a fresh 32-byte nonce from `OsRng`.
3. The client replies with `Hello`.
4. The node checks, in order: the protocol version; the `party_id` format; that the party exists and is enabled; and the signature over this channel's transcript, using this connection's nonce. On success it sends `Welcome` and binds the connection to the party's grants. On any failure it sends `AuthFailed` with the fixed reason and closes the connection.

The nonce is per connection and single use, so a signature captured on one connection cannot be replayed on another.

The party's session is registered before `Welcome` is sent, and the party is checked again right after registration, so a revocation that lands during login is never missed.

## Limits before login

- The 5-second login deadline counts from accept and includes the TLS handshake. A slow handshake therefore cannot hold a slot past the deadline.
- The first frame must be `Hello`. Frames before `Welcome` are capped at 64 KB.
- At most 128 connections may be pre-login at once, and at most 16 from any one source IP. Further connections are refused at accept.
- Per-address limits key an IPv6 address by its /64 prefix, after IPv4-mapped addresses are folded to IPv4. One host controls a whole /64, so keying by the full address would leave the per-address limits empty for IPv6.
- REST TLS handshakes have their own limits: 128 total, 16 per source IP, and a 5-second handshake deadline.
- **REST challenges.** At most 16 may be outstanding from one source IP. The 17th gets `429 Too Many Requests` with `Retry-After: 1`. At most 4096 may be outstanding in total. Beyond that, the answer is `503 Service Unavailable` with `Retry-After: 1`. Each challenge lives 5 seconds and is single use.
- **Retries by native clients.** A native client refused by any of these caps (a 429 or 503 on REST, or a connection closed before `Challenge` arrives on TCP or QUIC) makes at most three attempts in total. Before each retry it waits the `Retry-After` value, or one second if none is given, plus random jitter of 0 to 500 milliseconds. After the third refusal it fails with an error that names the refusal. The TCP and QUIC retry follows the same rule as REST, because the burst argument is the same. A client cannot tell a refusal at accept from a dropped connection, so the bound is what keeps either case from becoming a retry storm. A close after `Challenge` has arrived is never retried, and `AuthFailed` is never retried.
- `party_id` must be 1 to 64 bytes of `[A-Za-z0-9_-]`. Anything else is refused before any party lookup, with the same `"auth_failed"` text.
- A failed login writes a warn line, rate-limited per source IP to one line per 10 seconds. The count of suppressed lines is reported on the next line admitted for that source, so a burst that simply stops is still summarized. `party_id` is logged only when it matches an enrolled party. Signatures and public keys are never logged.
- REST refusals write warn lines, rate-limited per party with the same window and suppressed count.

## Authorization

Parties are enrolled in `parties.toml`, whose path comes from the node's configuration:

```toml
[[party]]
id = "north-plant"
public_key = "<base64 Ed25519 public key from the party's identity file>"
enabled = true
admin = false
max_subscriptions = 8
subscribe = ["scene_deltas", "log/output"]
publish = ["log/output"]
```

Rules:

- **Default deny.** A topic missing from `subscribe` cannot be subscribed to, and a topic missing from `publish` cannot be published to.
- **Reserved topics.** `scene_deltas` and `agent_commands` also require `admin = true` to publish. `scene_deltas` is consumed by the materializer that writes a Space to disk. `agent_commands` is the inbound command path.
- **The admin flag does not replace the materializer's own checks.** The materializer keeps its path checks (`space://` containment) on whatever a scene delta asks it to write. The admin flag decides who may publish a scene delta at all. Both checks apply, and neither replaces the other.
- **Subscription quota.** `max_subscriptions` (default 8) caps one party's simultaneous subscriptions. A subscribe beyond the quota returns `ServerFrame::Error`, not a silent drop. This keeps one party from taking a topic's 64 subscriber slots.
- **Live reload and revocation.** The node re-reads `parties.toml` each second, and re-verifies its folder with it. A disabled or removed party loses its live connections, its sessions, its tokens and its SSE streams within that second. This holds even for a connection whose dispatch is waiting on a write. A connection ended this way has its forwarders aborted, its subscriber slots and quota released, and its queued messages dropped.
- **Grant removal.** On each reload, a connection that holds a subscription to a topic it is no longer granted loses that subscription, and an SSE stream on such a topic ends. A repeated `Subscribe` to a topic the connection already holds is checked against the grant first, like any `Subscribe`.
- **Key changes.** Each session, token and stream records the fingerprint of its party's public key when it is created. A reload that finds a different key for the party id ends everything recorded under the old fingerprint. A key change is therefore a revocation of the old key.
- **Entries that cannot be used.** An entry without `enabled` reads as disabled. An entry with a malformed id or key, and every entry sharing a duplicated id, is left out and logged at error. The rest of the file still applies.
- **A bad or unprotected file.** A `parties.toml` that fails to parse, or fails the permission check below, keeps the last good set and logs at error level.

## Files and permissions

- At startup the node checks, with `eustress_identity::private_file::verify`, the TLS private key file, `parties.toml`, and the folder that holds `parties.toml`. If any check fails, the node refuses to start its listeners and logs the path and the account the check names.
- For `parties.toml` the check refuses any access entry beyond the owner and SYSTEM. The danger is another account writing the file and enrolling itself as admin.
- The `party add` and `party revoke` subcommands write `parties.toml` through `write_private`. `party add` rewrites the file through TOML serialization, so hand-written comments are lost; a fixed header comment is written.
- The reload check runs the same verify on `parties.toml`. A failure keeps the last good set, as described under Authorization.
- `private_file.rs` moves from the engine crate into `eustress-identity`. The engine re-exports it, so there is one implementation. This is a separate change, landed before the stream-node change.
- A native client verifies its identity file with the same helper before it reads the private key.
- Each key file is opened with `eustress_identity::private_file::verify_open`, and its contents are read through the handle that call returned: the TLS private key, `parties.toml`, and the identity file a native client reads. The file that is read is therefore the file that was checked. A read by path after the check is not allowed.
- `verify` and `verify_open` refuse a file or folder owned by another account. The refusal names the owner, and `private_file::not_owner` returns it, so the startup log can say who owns the file.
- `verify_open` opens the file with read and delete sharing, and without write sharing. A reload's read therefore never blocks a rename over the file (`party revoke`), and no writer can change the file under a check. The handle keeps the file object it verified, even if the path is renamed over during the read.

## REST and SSE

- Every route except the two login routes requires a bearer token. A request without one, or with an expired one, gets `401`.
- **Routes.** `POST /topics/{name}/publish` needs a publish grant. `GET /topics/{name}/stream` needs a subscribe grant and streams SSE. `GET /topics/{name}/replay/{from}` needs a subscribe grant, streams SSE from offset `from`, and takes its offset from the path, because a bearer route refuses query parameters. `GET /topics` lists only the topics the party is granted. `GET /health` answers with the node's status. The login routes are `POST /v1/login/challenge` and `POST /v1/login`. There is no replay route that takes its offset from the query string.
- **Challenge:** `POST /v1/login/challenge` returns `{"nonce": "<base64, 32 bytes>", "node_id": "<node id>"}`. The client needs `node_id` because the transcript includes it. The caps and answers are in Limits before login.
- **Login:** `POST /v1/login` takes `{"party_id": "...", "nonce": "<base64>", "token_class": "session" | "browser", "signature": "<base64>"}`. The node looks up the nonce among its live challenges, from any source address. An unknown, expired or used nonce is refused as a bad signature would be, after one dummy signature check so the timing matches. The node rebuilds the REST transcript with its own `node_id`, the nonce, its own certificate hash, the party id, the protocol version and the token class. A login without a valid signature gets `401`.
- **Token classes:** `session` lasts one hour. `browser` lasts 15 minutes. The class is part of the signed transcript, so it cannot be changed in transit.
- **Browsers never log in and never hold a party key.** A browser receives a `browser` token that a native login minted, from `eustress-stream-node party token` or from the engine handing a token to its own webview (see Client commands). The browser sends it as `Authorization: Bearer`.
- Tokens are stored as SHA-256 hashes in a table bounded at 4096 entries. A login is refused when the table is full. A token is bound to its party. Disabling a party drops its tokens within a second.
- At most 32 tokens may be live for one party. A login that would exceed that is refused with `429` and `Retry-After: 1` for that party only. Other parties are unaffected.
- Tokens travel only in the `Authorization: Bearer` header. A bearer route refuses every query parameter, because its explicit allowlist is empty. The query string is percent-decoded before the check, so an encoded name such as `%74oken` is refused as well.
- Browser streams use `fetch` with the header, not `EventSource`.
- An SSE stream ends when its token reaches `expires_at`, and when its party's grant for the topic is removed on a reload. Nothing outlives its token.
- The CORS allow list stays loopback-only and gains `authorization` among its allowed request headers.
- **Accepted residual risk:** a script on the dashboard origin can use a browser token for at most 15 minutes, and only within that party's grants. The party key never leaves the native client.

## Client commands

`eustress-stream-node party token <party-id> --node <host:port> --pin <SPKI SHA-256, hex> --identity <path> [--scope browser|session]`

- It runs on the party's own machine as a native client. It is not a node-side command.
- It verifies the identity file with `private_file::verify` before it reads the key. A file that fails the check is not read.
- It connects to the node's REST port over TLS 1.3, pinned to `--pin`. It fetches a challenge, reads the node certificate from the handshake, performs the REST login with the certificate binding, and prints the token and its expiry. `--scope` defaults to `browser`. It follows the retry rule under Limits before login.
- Identity files store keys in different encodings. The engine and the web app store hex; `parties.toml` and the identity crate use base64. `party token` and `party add --public-key` accept either.

The engine may do the same from its own signed-in identity and hand the token to its own webview. That engine work is not part of this specification.

## Local transports

- The Unix socket transport (`uds.rs`), the shared-memory transport (`shm.rs`) and `StreamNode::start_shm_bridge` compile only under the non-default feature `bench-transports`. That feature exists for the benches.
- The crate refuses to compile with `bench-transports` unless `bench-harness` is also on, through a `compile_error!` in `lib.rs`. Only the two bench entries enable `bench-harness`, through `required-features`. The engine never does.
- No shipped build enables `bench-transports`. The build requests include `cargo tree` checks that the engine's dependency on `eustress-stream-node`, and the crate's default features, show neither `bench-transports` nor `bench-harness`.
- Doc examples that use these transports are gated or marked `ignore`, so the doc tests build without the feature.
- The non-default feature `test-hooks` compiles one test-only pause point into the subscription forwarders. An integration test cannot see `cfg(test)`, so the paused-forwarder test uses this feature instead. No shipped build enables it, and the build requests check it the same way as `bench-transports`.
- The reason is Decision 3. A shared-memory section on Windows can be opened by other local accounts unless its security descriptor says otherwise, and the Unix socket has no login. Neither may be reachable from a shipped build.

## Behavior choices

These follow from the implementation and are part of the specification for review.

- `ListTopics` and `GET /topics` show only the topics the party holds a grant on.
- A refused fire-and-forget publish (`PublishNoAck`, `PublishBatchNoAck`) sends nothing back. Those senders never read a reply, so an `Error` frame would be taken for the answer to their next acknowledged request. The refusal is logged at warn, rate-limited like the login warnings. An acknowledged publish refused by its grants (`Publish`, `PublishBatch`, `PublishBatchTopic`) returns `Error` in place of its `Ack`.
- `PublishBatch` is all or nothing: one refused topic refuses the whole batch and publishes nothing.
- A connection that gets more than 100 refused no-ack publishes (`PublishNoAck`, `PublishBatchNoAck`) within one 60-second window is closed, with one warn line that names the party. The window is fixed and starts at the first refusal. The sender never reads a reply, so this is the only bound on a party that keeps sending refused frames.
- Grants are read from the live party set on every frame, so a narrowed grant applies at the next frame.
- Unknown and disabled parties cost one dummy signature check, so their refusals take about as long as a wrong-key refusal. The dummy check verifies against the RFC 8032 section 7.1 test-1 public key, and a unit test pins that constant to the RFC bytes, so a typo cannot make the check return before the signature is verified.

## Certificate rotation

- **Renewal with the same key** (a new certificate for the same private key): no client changes. The SPKI pin still matches. Replace the PEM files and restart the node. The DER certificate hash changes, but clients compute it from the certificate they see, so the REST binding needs no client update either.
- **Renewal with a new key:** a planned pin change. Before the switch, the operator sends the new SPKI SHA-256 to every party, and each party updates its pin. A client that still holds the old pin refuses the new certificate until its pin is updated. The switch is planned so that no party is cut off during the window. A compromised key is handled this way, and never by a date check.

## Transport and bind

- The default bind stays `127.0.0.1`.
- A non-loopback bind additionally requires at least one enabled party. With none, the node refuses to start.
- All listeners share one certificate and key.

## Engine integration

The engine's `StreamNodePlugin` changes in four ways:

1. **Runtime lifetime.** `NodeServer::start` spawns its accept loops on the runtime it runs on. The plugin currently runs `start` on a temporary runtime that is dropped when its startup thread ends, which cancels those loops, so the listeners close right after startup. The runtime is held for the plugin's lifetime, in the `StreamNodeHandle` resource or on a thread that stays in `block_on` until shutdown.
2. **REST.** `NodeServer` does not start REST. The plugin starts REST with the same TLS configuration when `rest` is true.
3. **Configured only.** The plugin starts no listener unless `tls_cert`, `tls_key` and `parties_path` are all set, the key and certificate files load, and the parties file passes the startup check. The node's own startup check repeats the file checks, so either layer alone refuses the start.
4. **Default off.** The engine's default configuration names none of those paths, so a default-configured engine starts no stream node. Turning the node on is a behavior change for any build with `stream-node`, which is why it stays off until configured. Where the configuration comes from is an open item.

Stale comments that say the node is unauthenticated are corrected in `engine/src/lib.rs`, `engine/src/app_core.rs` and the `stream-node` feature comment in `engine/Cargo.toml`.

Regression tests, in `stream_node_plugin.rs`:
- after the startup path returns, a pinned client can log in. The test must fail when the runtime is dropped at the end of startup;
- a default-configured plugin opens no stream port: a TCP connect to the configured port is refused, and the `StreamNodeHandle` resource is absent.

## Tests

Each test must fail when the guard it covers is removed.

Transport:
- a client pinned to a certificate refuses a node that presents a different certificate, on TCP, QUIC and REST. This test fails when the pin comparison is removed;
- **the pinned certificate with a different key:** a test server uses a custom `ResolvesServerCert` that returns the pinned certificate paired with a different signing key. The CertificateVerify signature therefore does not match the certificate. The client must refuse, on TCP and on QUIC. This test fails when `verify_tls13_signature` returns `Ok` without checking, and it is the only test that proves the handshake signature is checked;
- a client restricted to TLS 1.2 cannot connect on TCP. QUIC cannot be configured for TLS 1.2 at all, so the QUIC guard is the type system;
- the QUIC server accepts no early data;
- a ratchet test over the crate's source enforces the sanctioned-call rule: exactly one `dangerous()` call, located in `transport.rs` and installing `PinnedServerVerifier`; exactly one verifier type; no `HandshakeSignatureValid::assertion`; `verify_tls13_signature` delegates to rustls with the provider's algorithms and never returns a bare `Ok`; `verify_tls12_signature` returns an error.

Login:
- an enrolled key logs in and receives `Welcome`;
- under the `test-hooks` feature, the verify calls made by the dummy check are counted: an unknown-party login and a bad REST nonce each make exactly one. A key that decodes but never reaches verify, or an early return added later, fails this test;
- a wrong key, an unknown party and a disabled party each receive `AuthFailed("auth_failed")`, with identical bytes;
- a malformed `party_id` (empty, longer than 64 bytes, or containing a space) is refused before lookup, with the same reply;
- a `Hello` signed for connection A is rejected on connection B;
- a `Hello` with `protocol_version = 2` receives `AuthFailed("version_unsupported")`;
- a signature made on the TCP channel is refused on QUIC and on REST (label separation);
- two transcripts whose fields split differently, such as node id `ab` with party id `cd` and node id `abc` with party id `d`, produce different bytes;
- a first frame that is not `Hello` closes the connection;
- a pre-login frame over 64 KB closes the connection;
- a TCP or QUIC connection closed before `Challenge` is retried, at most three attempts in total, with the wait and jitter of the retry rule; a connection closed after `Challenge` arrives is not retried; `AuthFailed` is not retried;
- a TLS handshake that never completes closes the connection at 5 seconds, measured on the paused clock, and frees its slot;
- the 17th pre-login connection from one source IP is refused while a connection from another source IP is accepted, measured on the paused clock. Use an injected-address unit test for the per-IP limiter and an accept-level test from `127.0.0.1`;
- the relay test: a proxy with its own certificate, which the client is configured to trust, forwards the login to a real node. The node must refuse it. Removing the exporter from the transcript must make this test fail.

Authorization:
- publishing without a `publish` grant returns `Error`, and the topic's offset does not advance;
- publishing to `scene_deltas` by a party without `admin = true` returns `Error`, even when `scene_deltas` is in its `publish` list;
- subscribing to a topic missing from `subscribe` is refused;
- subscribing beyond `max_subscriptions` returns `Error`, and the topic's subscriber count is unchanged;
- disabling a party drops its live connection and its SSE stream within the one-second check, measured on the paused clock;
- a `PublishBatch` with one refused topic publishes nothing;
- a refused `PublishNoAck` sends no frame, and the next acknowledged publish's `Ack` reaches its own caller;
- a connection that gets 100 refused no-ack publishes in one window stays open, and the 101st closes it with one warn line that names the party (paused clock; the window resets after 60 seconds).
- a refused `Publish` returns its `Error` to its own caller, and the next accepted publish returns its own offset;
- a refused `Subscribe` returns an error to its caller and leaves no subscriber channel;
- an accepted `Subscribe` returns after `Subscribed`, and no `Message` for it arrives before that;
- a reply whose kind does not match the front request closes the connection as a protocol error;
- mutations: a client that does not complete the front request on `Error` fails the first test; a server that sends no `Subscribed` fails the third.
- one connection, pipelined back to back: an acknowledged publish that is refused, a `Subscribe` that succeeds, a no-ack publish, a `Subscribe` that is refused, and an acknowledged publish that succeeds. The replies arrive as `Error`, `Subscribed`, nothing, `Error`, `Ack`, and the client's waiters complete in exactly that order. A mutation that reorders the handler, for example by answering a request on a spawned task, must fail this test;
- the same five requests, sent through the client's own calls with the futures started in order, complete their waiters in that order.

Files:
- a `parties.toml` with an extra access entry for another account fails the startup check, and no listener starts;
- a `parties.toml` that becomes unprotected after startup keeps the last good set at reload;
- a private key file with an extra access entry fails the startup check.
- a key file or `parties.toml` read by path after its check fails the ratchet test that scans the loaders for path reads of key material (mutation: a path read after the check);
- an owner refusal from the helper stops startup and reaches the log with the owner's name (one propagation test in the node; the helper's own tests cover the refusal).
- after the helper's sharing change, `party revoke` succeeds while a reload holds the parties file open (Windows only; ignored until MCP's change is on disk, see Landing);
- the startup log names the loaded certificate's SPKI pin at info.

REST:
- no token gets `401`;
- an expired token gets `401`;
- a login whose signature was made over a different certificate hash (a relay) is refused;
- a login with an unknown, expired or already-used nonce is refused, with the same reply as a bad signature;
- a login with no signature, or with an unknown `token_class`, is refused;
- a `browser` token expires at 15 minutes and a `session` token at one hour, measured on the paused clock;
- a token in the query string is refused;
- an SSE stream opened with `fetch` and the header delivers a published message;
- **the 17th challenge from one source IP gets `429` with `Retry-After: 1`;**
- **a native client refused with 429 retries after the wait and succeeds; a client refused on all three attempts fails with an error that names the refusal;**
- **the 4097th outstanding challenge gets `503` with `Retry-After: 1`;**
- the jitter stays within 0 to 500 milliseconds (a test with an injected random source).

Client commands:
- `party token` refuses to read an identity file that fails the permission check;
- `party token` refuses a node whose certificate SPKI does not match `--pin`.

Engine:
- the regression tests under Engine integration.

Revocation, grants, keys and limits:
- a party is revoked while its dispatch waits on a write to a peer that has stopped reading: the connection closes within the check interval plus the write deadline, on the paused clock;
- a stalled peer's write exceeds the 10-second deadline and the connection closes;
- an SSE stream ends at its token's `expires_at`, and ends at the first reload where its grant for the topic is removed;
- a `Subscribe`, an `Unsubscribe` and a second `Subscribe` on one connection deliver no stale `Message` from the first subscription;
- a stale `Message` that arrives before a new subscription's `Subscribed` is not delivered to that subscription;
- an `Unsubscribe` while the forwarder is in the middle of a frame leaves the stream decodable: the next frame reads whole (a mutation that aborts the forwarder must fail this test);
- with a forwarder paused between receiving a message and taking the writer lock, an `Unsubscribe` and a new `Subscribe` run on the same topic; when the forwarder resumes, no `Message` reaches the new subscriber before its first new one (a mutation that checks the stop flag outside the lock must fail this test);
- a repeated `Subscribe` to a topic that is no longer granted is refused, and a reload that removes a grant ends that connection's subscription;
- a QUIC connection from an unvalidated address receives a Retry and takes no pre-login slot;
- a client request whose caller is dropped under backpressure never leaves a queued request without its frame, and the next reply reaches its own caller (paused clock, with a timeout);
- the 33rd live token for one party gets `429` for that party, and another party can still log in;
- a key change under the same party id ends the sessions, tokens and streams recorded under the old fingerprint;
- 17 connections from 17 addresses in one /64 are capped at 16;
- a revocation between `Welcome` and registration is seen by the session re-check;
- `?%74oken=` and any other query parameter on a bearer route are refused;
- REST refusal logs are rate-limited per party;
- a reload whose parties folder has gained an extra access entry keeps the last good set.

Mutation checks, run before landing:
- remove the signature check (the wrong-key test must fail);
- remove the grant check (the grant tests must fail);
- remove the exporter from the transcript (the relay test must fail);
- remove the pin comparison (the different-certificate test must fail);
- make `verify_tls13_signature` return `Ok` without checking (the pinned-certificate-with-another-key test must fail);
- remove the REST certificate binding (the REST relay test must fail);
- drop the engine's runtime at the end of startup (the runtime regression test must fail);
- remove the configured-only guard from the plugin (the default-off test must fail).
- remove the write deadline (the stalled-peer test must fail);
- remove the revocation race from the dispatch wait (the stalled-peer revocation test must fail);
- remove the SSE expiry check or the stream's grant check (the stream tests must fail);
- remove the grant check on a repeated `Subscribe` (the repeat-subscribe test must fail);
- remove the unsubscribe abort (the stale-message test must fail);
- remove the QUIC Retry (the unvalidated-address test must fail);
- move a write out of the writer task (the cancellation test must fail);
- remove the per-party token cap (the 33rd-token test must fail);
- remove the key fingerprint check (the key-change test must fail);
- key IPv6 by the full address (the /64 test must fail);
- remove the session re-check after registration (the revocation-window test must fail);
- remove the query allowlist (the encoded-token test must fail).
- register a subscription's channel at `Subscribe` time instead of at `Subscribed` (the stale-message test must fail);
- abort the forwarder on `Unsubscribe` instead of stopping it between frames (the decodable-stream test must fail).
- move the stop check outside the writer lock (the paused-forwarder test must fail);
- restore the `TIMING_KEY_B64` typo, or return before the verify call in the dummy check (the timing-key test and the verify-count test must fail) (#53);

## Landing

1. Security reviews each staged file before it lands.
   The Windows revoke test in `tests/auth_files.rs` carries `#[ignore]` until MCP's FILE_SHARE_DELETE change is on disk. In the wave that lands the stream node, remove the attribute once MCP confirms the change is on disk. If the stream node would land before that change, keep the attribute and remove it in the wave where MCP's change lands.
2. Order:
   1. The `private_file` move: `identity/src/private_file.rs` is added, `engine/src/private_file.rs` becomes a re-export, the engine depends on `eustress-identity` with `default-features = false` (so the witness client is not compiled into any engine build), and the Windows dependency the helper needs is added to `identity/Cargo.toml`.
   2. The stream-node change, which depends on step 1.
   3. The engine plugin change, which depends on step 2.
3. Stream-node files, all under `crates/stream-node/` unless noted:
   - `src/auth.rs` (new): transcript, parties loader, party table, token table, nonce table, pre-login limiter, REST caps;
   - `src/transport.rs` (new): the TLS 1.3 configurations, the pinning verifier (the one sanctioned call), and the QUIC configuration with early data off;
   - `src/protocol.rs`: appended frames;
   - `src/handler.rs`: login gate, and grant and quota checks in `dispatch`;
   - `src/server.rs`: TLS accept, pre-login limits, and startup checks;
   - `src/quic.rs`: login on the first stream, the configured certificate, and the client-side pin;
   - `src/rest.rs`: login routes, the 429 and 503 answers, bearer middleware, and the CORS header;
   - `src/config.rs`: `tls_cert`, `tls_key`, `parties_path`;
   - `src/client.rs`: login before the first frame, with the pin, and the retry rule;
   - `src/error.rs`: the new error variants;
   - `src/lib.rs`: modules, exports and `StreamNode`, with the `bench-transports` gating and the `compile_error!` guard;
   - `src/uds.rs` and `src/shm.rs`: gated under `bench-transports`;
   - `src/bin/eustress-stream-node.rs`: `party add`, `party revoke`, `party list` and `party token` subcommands;
   - `Cargo.toml`: the `eustress-identity` path dependency with `default-features = false`, so the witness HTTP client (`reqwest`) is not compiled into the node, `tokio-rustls` 0.26 with `default-features = false`, `rcgen` as a dev-dependency only, the features `bench-transports`, `bench-harness` and `test-hooks` (all non-default), and `required-features` on the two shared-memory and Unix-socket benches;
   - `tests/auth_login.rs`, `tests/auth_grants.rs`, `tests/auth_rest.rs`, `tests/auth_transport.rs` and `tests/auth_files.rs` (new);
   - `tests/auth_revocation.rs` (new): the revocation, grant, key-change, limit and writer tests listed under Tests.
   - `tests/sse_subscription_lifetime.rs`: REST now needs TLS and a bearer token;
   - `tests/common/mod.rs`: shared fixtures;
   - `benches/network_throughput.rs`: clients log in; `benches/shm_throughput.rs` and `benches/uds_throughput.rs`: gated;
   - outside the crate: `crates/engine/src/stream_node_plugin.rs` (the Engine integration changes), `crates/engine/src/lib.rs`, `crates/engine/src/app_core.rs` and `crates/engine/Cargo.toml` (comments only).
4. Line endings: each file keeps the line endings it already has. New files are CRLF. Lines that were already there are not reworded, so the diff shows only the change. New lines contain no em dashes or en dashes.
5. `Cargo.toml` changes get `cargo metadata` from the orchestrator before landing, per BUILD_STATUS rule 4.
6. Build requests to the orchestrator:
   - `cargo test -p eustress-identity` after step 1;
   - `cargo test -p eustress-stream-node --features quic`, because the QUIC tests are behind the feature;
   - `cargo test -p eustress-stream-node`, for the rest;
   - `cargo test -p eustress-stream-node --features bench-transports,bench-harness` and the two bench targets, so the gated code is still proven;
   - `cargo test -p eustress-stream-node --features test-hooks`, for the paused-forwarder test, which needs the test-only pause point. `test-hooks` is checked like `bench-transports`: the engine graph and the default set must not show it.
   - `cargo tree -p eustress-engine -e features -i eustress-stream-node`, which must show no `bench-transports` and no `bench-harness`, and must show the package only under `--features stream-node`;
   - `cargo tree -p eustress-stream-node -e features`, which must show neither gating feature in the default set; `cargo tree -p eustress-stream-node -e features -i eustress-identity`, which must show no `witness` feature on the node's own graph; `cargo tree -p eustress-engine --features stream-node -e features -i eustress-identity`, which must show no `witness` feature anywhere in the engine graph, so the engine's own dependency on `eustress-identity` must also use `default-features = false`; and `cargo tree -p eustress-stream-node -e normal -i reqwest`, which must show no match, kept as a second check;
   - `cargo check -p eustress-engine --features stream-node`;
   - the default `cargo build -p eustress-engine`.

## Open items

- **Engine configuration.** Where the engine finds the certificate, the key and the parties file is undecided. Until it is, the engine starts no node. This is a decision for McKale, not Security.
- **Pre-existing, not in this change.** `NodeConfig::max_connections` is not enforced, so authenticated connections after login are not bounded by it. A topic name containing `/` must be percent-encoded in REST paths. `effective_rest_port` overflows above port 55535.
- **The engine's own bridge port** has no authentication. It follows the same token rules as this design and is tracked separately.
- **The engine's webview token handoff** is a later engine change. It is not part of this specification.
