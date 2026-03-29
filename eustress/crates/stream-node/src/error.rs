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
}
