//! # Forge Client
//!
//! ## Table of Contents
//!
//! 1. **ForgeClient** - High-level client for Eustress Forge orchestration API

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::deployment::{DeploymentSpec, DeploymentStatus, DeploymentInfo};
use crate::error::{SdkError, SdkResult};
use crate::types::ServerInfo;

/// High-level client for Eustress Forge orchestration platform.
#[derive(Debug, Clone)]
pub struct ForgeClient {
    /// HTTP client
    http: Client,
    /// Base URL for the Forge API
    base_url: String,
    /// Authentication token
    auth_token: Option<String>,
}

impl ForgeClient {
    /// Create a new Forge client connected to the given API URL.
    pub async fn new(base_url: &str) -> SdkResult<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(SdkError::Http)?;

        info!("Forge client connecting to {}", base_url);

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token: None,
        })
    }

    /// Authenticate with an API key.
    pub async fn authenticate(&mut self, api_key: &str) -> SdkResult<()> {
        #[derive(Serialize)]
        struct AuthRequest<'a> {
            api_key: &'a str,
        }

        #[derive(Deserialize)]
        struct AuthResponse {
            token: String,
        }

        let response: AuthResponse = self
            .http
            .post(format!("{}/api/auth", self.base_url))
            .json(&AuthRequest { api_key })
            .send()
            .await
            .map_err(SdkError::Http)?
            .json()
            .await
            .map_err(SdkError::Http)?;

        self.auth_token = Some(response.token);
        info!("Authenticated with Forge API");
        Ok(())
    }

    /// Deploy an experience with the given specification.
    pub async fn deploy_experience(&self, spec: DeploymentSpec) -> SdkResult<DeploymentInfo> {
        let mut request = self
            .http
            .post(format!("{}/api/deployments", self.base_url))
            .json(&spec);

        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(SdkError::Http)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(SdkError::Server { status, message });
        }

        let deployment: DeploymentInfo = response.json().await.map_err(SdkError::Http)?;
        info!("Deployment created: {}", deployment.id);
        Ok(deployment)
    }

    /// Get the status of a deployment.
    pub async fn get_deployment_status(&self, deployment_id: &str) -> SdkResult<DeploymentStatus> {
        let mut request = self
            .http
            .get(format!("{}/api/deployments/{}/status", self.base_url, deployment_id));

        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(SdkError::Http)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(SdkError::Server { status, message });
        }

        response.json().await.map_err(SdkError::Http)
    }

    /// List all running servers for an experience.
    pub async fn list_servers(&self, experience_id: &str) -> SdkResult<Vec<ServerInfo>> {
        let mut request = self
            .http
            .get(format!("{}/api/experiences/{}/servers", self.base_url, experience_id));

        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(SdkError::Http)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(SdkError::Server { status, message });
        }

        response.json().await.map_err(SdkError::Http)
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check if the client is authenticated.
    pub fn is_authenticated(&self) -> bool {
        self.auth_token.is_some()
    }
}
