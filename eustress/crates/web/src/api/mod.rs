// =============================================================================
// Eustress Web - API Client Module
// =============================================================================
// Table of Contents:
// 1. Submodules
// 2. Re-exports
// 3. API Client
// 4. Error Types
// =============================================================================

pub mod auth;
pub mod community;
pub mod friends;
pub mod gallery;
pub mod marketplace;
pub mod presence_ws;
pub mod projects;

pub use auth::*;
pub use community::*;
pub use friends::*;
pub use gallery::*;
pub use marketplace::*;
pub use presence_ws::*;
pub use projects::*;

use gloo_net::http::{Request, RequestBuilder, Response};
use gloo_storage::Storage;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

// -----------------------------------------------------------------------------
// 4. Error Types
// -----------------------------------------------------------------------------

/// API error types.
#[derive(Error, Debug, Clone)]
pub enum ApiError {
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },
    
    #[error("Deserialization error: {0}")]
    Deserialize(String),
    
    #[error("Unauthorized")]
    Unauthorized,
    
    #[error("Not found")]
    NotFound,
}

// -----------------------------------------------------------------------------
// 3. API Client
// -----------------------------------------------------------------------------

/// HTTP client for API requests.
pub struct ApiClient {
    base_url: String,
}

impl ApiClient {
    /// Create a new API client.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }
    
    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    
    /// Get the auth token from localStorage.
    fn get_token() -> Option<String> {
        gloo_storage::LocalStorage::get::<String>("auth_token").ok()
    }
    
    /// Build a request with common headers.
    fn build_request(&self, method: &str, endpoint: &str) -> RequestBuilder {
        let url = format!("{}{}", self.base_url, endpoint);
        let mut req = match method {
            "GET" => Request::get(&url),
            "POST" => Request::post(&url),
            "PUT" => Request::put(&url),
            "DELETE" => Request::delete(&url),
            "PATCH" => Request::patch(&url),
            _ => Request::get(&url),
        };
        
        // Add auth header if token exists
        if let Some(token) = Self::get_token() {
            req = req.header("Authorization", &format!("Bearer {}", token));
        }
        
        req.header("Content-Type", "application/json")
    }
    
    /// Handle API response.
    async fn handle_response<T: DeserializeOwned>(response: Response) -> Result<T, ApiError> {
        let status = response.status();
        
        match status {
            200..=299 => {
                response
                    .json::<T>()
                    .await
                    .map_err(|e| ApiError::Deserialize(e.to_string()))
            }
            401 => Err(ApiError::Unauthorized),
            404 => Err(ApiError::NotFound),
            _ => {
                let message = response.text().await.unwrap_or_default();
                Err(ApiError::Server { status, message })
            }
        }
    }
    
    /// GET request.
    pub async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T, ApiError> {
        let response = self
            .build_request("GET", endpoint)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        
        Self::handle_response(response).await
    }
    
    /// POST request with JSON body.
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let response = self
            .build_request("POST", endpoint)
            .json(body)
            .map_err(|e| ApiError::Deserialize(e.to_string()))?
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        
        Self::handle_response(response).await
    }
    
    /// PUT request with JSON body.
    pub async fn put<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let response = self
            .build_request("PUT", endpoint)
            .json(body)
            .map_err(|e| ApiError::Deserialize(e.to_string()))?
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        
        Self::handle_response(response).await
    }
    
    /// DELETE request.
    pub async fn delete<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T, ApiError> {
        let response = self
            .build_request("DELETE", endpoint)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        
        Self::handle_response(response).await
    }
}
