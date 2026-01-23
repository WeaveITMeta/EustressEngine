//! Streaming response support for AI/ML inference
//!
//! Provides Server-Sent Events (SSE) and token streaming for LLM inference.

use std::pin::Pin;
use std::task::{Context, Poll};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Configuration for streaming responses
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Buffer size for the stream channel
    pub buffer_size: usize,
    /// Include timing information in events
    pub include_timing: bool,
    /// Heartbeat interval in milliseconds (0 to disable)
    pub heartbeat_ms: u64,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            buffer_size: 32,
            include_timing: true,
            heartbeat_ms: 15000, // 15 seconds
        }
    }
}

/// A streaming event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Event type (e.g., "token", "done", "error")
    pub event: String,
    /// Event data
    pub data: String,
    /// Event ID (optional)
    pub id: Option<String>,
    /// Timestamp in milliseconds
    pub timestamp_ms: Option<u64>,
}

impl StreamEvent {
    /// Create a new stream event
    pub fn new(event: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            event: event.into(),
            data: data.into(),
            id: None,
            timestamp_ms: None,
        }
    }

    /// Create a token event
    pub fn token(token: impl Into<String>) -> Self {
        Self::new("token", token)
    }

    /// Create a done event
    pub fn done() -> Self {
        Self::new("done", "[DONE]")
    }

    /// Create an error event
    pub fn error(msg: impl Into<String>) -> Self {
        Self::new("error", msg)
    }

    /// Create a heartbeat event
    pub fn heartbeat() -> Self {
        Self::new("heartbeat", "")
    }

    /// Set event ID
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp_ms: u64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }

    /// Format as SSE
    pub fn to_sse(&self) -> String {
        let mut result = String::new();
        
        if let Some(id) = &self.id {
            result.push_str(&format!("id: {}\n", id));
        }
        
        result.push_str(&format!("event: {}\n", self.event));
        
        // Handle multi-line data
        for line in self.data.lines() {
            result.push_str(&format!("data: {}\n", line));
        }
        if self.data.is_empty() {
            result.push_str("data: \n");
        }
        
        result.push('\n');
        result
    }
}

/// Streaming response for inference
pub struct StreamingResponse {
    rx: mpsc::Receiver<StreamEvent>,
    config: StreamingConfig,
}

impl StreamingResponse {
    /// Create a new streaming response with a sender
    pub fn new(config: StreamingConfig) -> (Self, StreamSender) {
        let (tx, rx) = mpsc::channel(config.buffer_size);
        let response = Self { rx, config };
        let sender = StreamSender { tx };
        (response, sender)
    }

    /// Create with default config
    pub fn default_config() -> (Self, StreamSender) {
        Self::new(StreamingConfig::default())
    }
}

impl Stream for StreamingResponse {
    type Item = StreamEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx).poll_recv(cx)
    }
}

/// Sender for streaming events
#[derive(Clone)]
pub struct StreamSender {
    tx: mpsc::Sender<StreamEvent>,
}

impl StreamSender {
    /// Send an event
    pub async fn send(&self, event: StreamEvent) -> Result<(), StreamError> {
        self.tx.send(event).await.map_err(|_| StreamError::Closed)
    }

    /// Send a token
    pub async fn send_token(&self, token: impl Into<String>) -> Result<(), StreamError> {
        self.send(StreamEvent::token(token)).await
    }

    /// Send done signal
    pub async fn send_done(&self) -> Result<(), StreamError> {
        self.send(StreamEvent::done()).await
    }

    /// Send error
    pub async fn send_error(&self, msg: impl Into<String>) -> Result<(), StreamError> {
        self.send(StreamEvent::error(msg)).await
    }

    /// Check if receiver is still connected
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// Streaming error
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// Stream was closed
    #[error("Stream closed")]
    Closed,
    /// Send failed
    #[error("Send failed: {0}")]
    SendFailed(String),
}

/// Token stream for LLM inference
pub struct TokenStream {
    tokens: Vec<String>,
    index: usize,
    delay_ms: u64,
}

impl TokenStream {
    /// Create a new token stream from a list of tokens
    pub fn new(tokens: Vec<String>) -> Self {
        Self {
            tokens,
            index: 0,
            delay_ms: 0,
        }
    }

    /// Set delay between tokens (for simulation)
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    /// Stream tokens to a sender
    pub async fn stream_to(&mut self, sender: &StreamSender) -> Result<(), StreamError> {
        while let Some(token) = self.next_token() {
            sender.send_token(token).await?;
            
            if self.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            }
        }
        
        sender.send_done().await
    }

    /// Get next token
    pub fn next_token(&mut self) -> Option<String> {
        if self.index < self.tokens.len() {
            let token = self.tokens[self.index].clone();
            self.index += 1;
            Some(token)
        } else {
            None
        }
    }

    /// Reset stream
    pub fn reset(&mut self) {
        self.index = 0;
    }

    /// Get remaining token count
    pub fn remaining(&self) -> usize {
        self.tokens.len().saturating_sub(self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_event_sse() {
        let event = StreamEvent::token("Hello")
            .with_id("1")
            .with_timestamp(12345);
        
        let sse = event.to_sse();
        assert!(sse.contains("id: 1"));
        assert!(sse.contains("event: token"));
        assert!(sse.contains("data: Hello"));
    }

    #[test]
    fn test_stream_event_multiline() {
        let event = StreamEvent::new("message", "line1\nline2\nline3");
        let sse = event.to_sse();
        
        assert!(sse.contains("data: line1"));
        assert!(sse.contains("data: line2"));
        assert!(sse.contains("data: line3"));
    }

    #[tokio::test]
    async fn test_streaming_response() {
        let (mut response, sender) = StreamingResponse::default_config();
        
        tokio::spawn(async move {
            sender.send_token("Hello").await.unwrap();
            sender.send_token(" World").await.unwrap();
            sender.send_done().await.unwrap();
        });

        use futures::StreamExt;
        let events: Vec<_> = response.collect().await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event, "token");
        assert_eq!(events[2].event, "done");
    }

    #[tokio::test]
    async fn test_token_stream() {
        let tokens = vec!["Hello".to_string(), " ".to_string(), "World".to_string()];
        let mut stream = TokenStream::new(tokens);
        
        assert_eq!(stream.remaining(), 3);
        assert_eq!(stream.next_token(), Some("Hello".to_string()));
        assert_eq!(stream.remaining(), 2);
        
        stream.reset();
        assert_eq!(stream.remaining(), 3);
    }
}
