use thiserror::Error;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Read out of range: offset={offset} len={len}")]
    OutOfRange { offset: u64, len: usize },

    #[error("Topic '{0}' not found")]
    TopicNotFound(String),

    #[error("Subscriber limit reached for topic '{0}'")]
    SubscriberLimit(String),

    #[error("Serialization error: {0}")]
    Serialize(String),
}
