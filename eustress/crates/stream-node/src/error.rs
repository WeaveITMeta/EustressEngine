use thiserror::Error;

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Frame encode error: {0}")]
    Encode(String),

    #[error("Frame decode error: {0}")]
    Decode(String),

    #[error("Frame too large: {0} bytes")]
    FrameTooLarge(usize),

    #[error("Topic not found: {0}")]
    TopicNotFound(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Port {0} already in use (port range exhausted)")]
    PortInUse(u16),

    #[error("Cluster error: {0}")]
    ClusterError(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Subscribe limit reached on topic '{0}'")]
    SubscribeLimit(String),

    /// The node refused the login. Carries the node's fixed reason:
    /// `auth_failed` or `version_unsupported`. Never retried.
    #[error("Login refused: {0}")]
    AuthFailed(String),

    /// A certificate, key, pin or TLS handshake problem. Never retried.
    #[error("TLS error: {0}")]
    Tls(String),

    /// The node refused to start its listeners. The text names what failed.
    #[error("Startup refused: {0}")]
    Startup(String),

    /// The peer did not follow the login protocol.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// A client's identity file could not be used.
    #[error("Identity file: {0}")]
    Identity(String),

    /// One attempt was refused by a node limit before the login began: a 429
    /// or 503 on REST, or a connection closed before `Challenge` on TCP or
    /// QUIC. The native clients retry this, and only this.
    #[error("Refused by a node limit: {reason}")]
    Refused {
        /// What the node answered.
        reason: String,
        /// The node's `Retry-After`, when it gave one.
        retry_after: Option<std::time::Duration>,
    },

    /// The node refused this request with an `Error` frame: no grant, a
    /// quota reached, a topic full. The connection stays open. Unlike
    /// `Refused`, this comes after login and is never retried.
    #[error("The node refused the request ({code}): {message}")]
    RequestRefused {
        /// The `Error` frame's code: 400, 403, 429 or 503.
        code: u32,
        /// The `Error` frame's text.
        message: String,
    },

    /// Every attempt the retry rule allows was refused by a node limit.
    #[error("Refused by a node limit on all {attempts} attempts; the last refusal: {reason}")]
    RetriesExhausted {
        /// Attempts made, the first included.
        attempts: u32,
        /// The last refusal.
        reason: String,
    },
}
