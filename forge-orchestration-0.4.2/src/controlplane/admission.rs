//! Admission controllers for validating and mutating resources
//!
//! Implements Kubernetes-style admission control with:
//! - Validating webhooks
//! - Mutating webhooks
//! - Built-in admission controllers

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Admission result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionResult {
    /// Whether the request is allowed
    pub allowed: bool,
    /// Reason for denial (if not allowed)
    pub reason: Option<String>,
    /// Warnings to return to the user
    pub warnings: Vec<String>,
    /// Patch to apply (for mutating admission)
    pub patch: Option<serde_json::Value>,
    /// Patch type (e.g., "JSONPatch")
    pub patch_type: Option<String>,
}

impl AdmissionResult {
    /// Create an allowed result
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            reason: None,
            warnings: Vec::new(),
            patch: None,
            patch_type: None,
        }
    }

    /// Create a denied result
    pub fn denied(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
            warnings: Vec::new(),
            patch: None,
            patch_type: None,
        }
    }

    /// Add a warning
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add a JSON patch
    pub fn with_patch(mut self, patch: serde_json::Value) -> Self {
        self.patch = Some(patch);
        self.patch_type = Some("JSONPatch".to_string());
        self
    }
}

/// Admission request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionRequest {
    /// Unique request ID
    pub uid: String,
    /// Operation type
    pub operation: Operation,
    /// Resource kind
    pub kind: String,
    /// Resource namespace
    pub namespace: Option<String>,
    /// Resource name
    pub name: Option<String>,
    /// The object being admitted
    pub object: Option<serde_json::Value>,
    /// The old object (for UPDATE/DELETE)
    pub old_object: Option<serde_json::Value>,
    /// User info
    pub user_info: UserInfo,
}

/// Operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    /// Create operation
    Create,
    /// Update operation
    Update,
    /// Delete operation
    Delete,
    /// Connect operation
    Connect,
}

/// User information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// Username
    pub username: String,
    /// User UID
    pub uid: Option<String>,
    /// Groups
    pub groups: Vec<String>,
}

impl Default for UserInfo {
    fn default() -> Self {
        Self {
            username: "system:anonymous".to_string(),
            uid: None,
            groups: vec!["system:unauthenticated".to_string()],
        }
    }
}

/// Admission controller trait
pub trait AdmissionController: Send + Sync {
    /// Controller name
    fn name(&self) -> &str;

    /// Whether this controller handles the given resource
    fn handles(&self, kind: &str, operation: Operation) -> bool;

    /// Validate the request
    fn validate(&self, request: &AdmissionRequest) -> AdmissionResult;

    /// Mutate the request (optional)
    fn mutate(&self, _request: &AdmissionRequest) -> Option<serde_json::Value> {
        None
    }
}

/// Validation webhook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWebhook {
    /// Webhook name
    pub name: String,
    /// Webhook URL
    pub url: String,
    /// Failure policy
    pub failure_policy: FailurePolicy,
    /// Resources to match
    pub rules: Vec<WebhookRule>,
    /// Timeout in seconds
    pub timeout_seconds: u32,
}

/// Mutating webhook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutatingWebhook {
    /// Webhook name
    pub name: String,
    /// Webhook URL
    pub url: String,
    /// Failure policy
    pub failure_policy: FailurePolicy,
    /// Resources to match
    pub rules: Vec<WebhookRule>,
    /// Timeout in seconds
    pub timeout_seconds: u32,
    /// Reinvocation policy
    pub reinvocation_policy: ReinvocationPolicy,
}

/// Failure policy for webhooks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailurePolicy {
    /// Fail the request if webhook fails
    Fail,
    /// Ignore webhook failures
    Ignore,
}

/// Reinvocation policy for mutating webhooks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReinvocationPolicy {
    /// Never reinvoke
    Never,
    /// Reinvoke if object was modified
    IfNeeded,
}

/// Webhook rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookRule {
    /// API groups to match
    pub api_groups: Vec<String>,
    /// API versions to match
    pub api_versions: Vec<String>,
    /// Resources to match
    pub resources: Vec<String>,
    /// Operations to match
    pub operations: Vec<Operation>,
}

/// Built-in resource quota admission controller
pub struct ResourceQuotaAdmission {
    /// Namespace quotas
    quotas: std::collections::HashMap<String, ResourceQuota>,
}

/// Resource quota
#[derive(Debug, Clone)]
pub struct ResourceQuota {
    /// CPU limit (millicores)
    pub cpu_limit: u64,
    /// Memory limit (MB)
    pub memory_limit: u64,
    /// GPU limit
    pub gpu_limit: u32,
    /// Workload count limit
    pub workload_limit: u32,
}

impl ResourceQuotaAdmission {
    /// Create new resource quota admission controller
    pub fn new() -> Self {
        Self {
            quotas: std::collections::HashMap::new(),
        }
    }

    /// Set quota for namespace
    pub fn set_quota(&mut self, namespace: impl Into<String>, quota: ResourceQuota) {
        self.quotas.insert(namespace.into(), quota);
    }
}

impl Default for ResourceQuotaAdmission {
    fn default() -> Self {
        Self::new()
    }
}

impl AdmissionController for ResourceQuotaAdmission {
    fn name(&self) -> &str {
        "ResourceQuota"
    }

    fn handles(&self, kind: &str, operation: Operation) -> bool {
        kind == "Workload" && operation == Operation::Create
    }

    fn validate(&self, request: &AdmissionRequest) -> AdmissionResult {
        let namespace = match &request.namespace {
            Some(ns) => ns,
            None => return AdmissionResult::allowed(),
        };

        let quota = match self.quotas.get(namespace) {
            Some(q) => q,
            None => return AdmissionResult::allowed(),
        };

        // Check workload count (simplified - real impl would track current usage)
        if quota.workload_limit == 0 {
            return AdmissionResult::denied(format!(
                "Namespace {} has reached workload limit",
                namespace
            ));
        }

        AdmissionResult::allowed()
    }
}

/// Built-in default values admission controller
pub struct DefaultsAdmission;

impl DefaultsAdmission {
    /// Create new defaults admission controller
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultsAdmission {
    fn default() -> Self {
        Self::new()
    }
}

impl AdmissionController for DefaultsAdmission {
    fn name(&self) -> &str {
        "Defaults"
    }

    fn handles(&self, kind: &str, operation: Operation) -> bool {
        kind == "Workload" && operation == Operation::Create
    }

    fn validate(&self, _request: &AdmissionRequest) -> AdmissionResult {
        AdmissionResult::allowed()
    }

    fn mutate(&self, request: &AdmissionRequest) -> Option<serde_json::Value> {
        let obj = request.object.as_ref()?;
        
        let mut patches = Vec::new();

        // Add default namespace if not set
        if obj.get("metadata").and_then(|m| m.get("namespace")).is_none() {
            patches.push(serde_json::json!({
                "op": "add",
                "path": "/metadata/namespace",
                "value": "default"
            }));
        }

        // Add default priority if not set
        if obj.get("spec").and_then(|s| s.get("priority")).is_none() {
            patches.push(serde_json::json!({
                "op": "add",
                "path": "/spec/priority",
                "value": 0
            }));
        }

        if patches.is_empty() {
            None
        } else {
            Some(serde_json::Value::Array(patches))
        }
    }
}

/// Admission controller chain
pub struct AdmissionChain {
    /// Validating controllers
    validators: Vec<Arc<dyn AdmissionController>>,
    /// Mutating controllers
    mutators: Vec<Arc<dyn AdmissionController>>,
}

impl AdmissionChain {
    /// Create new admission chain
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
            mutators: Vec::new(),
        }
    }

    /// Add validating controller
    pub fn add_validator<T: AdmissionController + 'static>(mut self, controller: T) -> Self {
        self.validators.push(Arc::new(controller));
        self
    }

    /// Add mutating controller
    pub fn add_mutator<T: AdmissionController + 'static>(mut self, controller: T) -> Self {
        self.mutators.push(Arc::new(controller));
        self
    }

    /// Run admission for a request
    pub fn admit(&self, mut request: AdmissionRequest) -> AdmissionResult {
        // Run mutating admission first
        let mut all_patches = Vec::new();
        
        for mutator in &self.mutators {
            if mutator.handles(&request.kind, request.operation) {
                if let Some(patch) = mutator.mutate(&request) {
                    if let Some(patches) = patch.as_array() {
                        all_patches.extend(patches.clone());
                    }
                }
            }
        }

        // Apply patches to request object
        if !all_patches.is_empty() {
            // In a real implementation, we'd apply JSON patches here
            // For now, just track that patches exist
        }

        // Run validating admission
        let mut warnings = Vec::new();
        
        for validator in &self.validators {
            if validator.handles(&request.kind, request.operation) {
                let result = validator.validate(&request);
                
                if !result.allowed {
                    return result;
                }
                
                warnings.extend(result.warnings);
            }
        }

        let mut result = AdmissionResult::allowed();
        result.warnings = warnings;
        
        if !all_patches.is_empty() {
            result = result.with_patch(serde_json::Value::Array(all_patches));
        }

        result
    }
}

impl Default for AdmissionChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admission_result() {
        let allowed = AdmissionResult::allowed();
        assert!(allowed.allowed);

        let denied = AdmissionResult::denied("test reason");
        assert!(!denied.allowed);
        assert_eq!(denied.reason, Some("test reason".to_string()));
    }

    #[test]
    fn test_defaults_admission() {
        let controller = DefaultsAdmission::new();
        
        let request = AdmissionRequest {
            uid: "test".to_string(),
            operation: Operation::Create,
            kind: "Workload".to_string(),
            namespace: None,
            name: Some("test".to_string()),
            object: Some(serde_json::json!({
                "metadata": {"name": "test"},
                "spec": {}
            })),
            old_object: None,
            user_info: UserInfo::default(),
        };

        let patch = controller.mutate(&request);
        assert!(patch.is_some());
    }

    #[test]
    fn test_admission_chain() {
        let chain = AdmissionChain::new()
            .add_mutator(DefaultsAdmission::new())
            .add_validator(ResourceQuotaAdmission::new());

        let request = AdmissionRequest {
            uid: "test".to_string(),
            operation: Operation::Create,
            kind: "Workload".to_string(),
            namespace: Some("default".to_string()),
            name: Some("test".to_string()),
            object: Some(serde_json::json!({})),
            old_object: None,
            user_info: UserInfo::default(),
        };

        let result = chain.admit(request);
        assert!(result.allowed);
    }
}
