//! # AI Generation Backends
//!
//! Implementations for different AI backends.
//!
//! ## Supported Backends
//!
//! - **Ollama** - Local LLM for text generation
//! - **ComfyUI** - Local Stable Diffusion for textures
//! - **Meshy** - Cloud 3D mesh generation API
//! - **Tripo3D** - Cloud 3D mesh generation API

#[allow(unused_imports)]
use super::generation::{AIBackend, GenerationRequest, GenerationResult};
#[allow(unused_imports)]
use crate::assets::ContentHash;
#[allow(unused_imports)]
use crate::scene::{NodeCategory, DetailLevel};
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use std::time::Instant;

/// Error type for backend operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("API error: {0}")]
    Api(String),
    
    #[error("Timeout after {0}s")]
    Timeout(u64),
    
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    
    #[error("Backend not configured")]
    NotConfigured,
    
    #[error("Unsupported category: {0:?}")]
    UnsupportedCategory(NodeCategory),
}

// ============================================================================
// Ollama Backend (Local LLM)
// ============================================================================

/// Ollama request format
#[derive(Serialize)]
#[allow(dead_code)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

/// Ollama response format
#[derive(Deserialize)]
#[allow(dead_code)]
struct OllamaResponse {
    response: String,
}

/// Call Ollama for text generation
#[cfg(feature = "async-assets")]
pub async fn call_ollama(
    endpoint: &str,
    model: &str,
    prompt: &str,
) -> Result<String, BackendError> {
    let client = reqwest::Client::new();
    
    let request = OllamaRequest {
        model: model.to_string(),
        prompt: prompt.to_string(),
        stream: false,
    };
    
    let response = client
        .post(format!("{}/api/generate", endpoint))
        .json(&request)
        .timeout(std::time::Duration::from_secs(60))
        .write()
        .await
        .map_err(|e| BackendError::Network(e.to_string()))?;
    
    if !response.status().is_success() {
        return Err(BackendError::Api(format!("HTTP {}", response.status())));
    }
    
    let ollama_response: OllamaResponse = response
        .json()
        .await
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
    
    Ok(ollama_response.response)
}

// ============================================================================
// ComfyUI Backend (Stable Diffusion)
// ============================================================================

/// ComfyUI workflow prompt
#[derive(Serialize)]
#[allow(dead_code)]
struct ComfyUIPrompt {
    prompt: serde_json::Value,
    client_id: String,
}

/// Queue a ComfyUI workflow
#[cfg(feature = "async-assets")]
pub async fn queue_comfyui_workflow(
    endpoint: &str,
    workflow_json: &str,
    prompt_text: &str,
) -> Result<String, BackendError> {
    let client = reqwest::Client::new();
    
    // Parse workflow and inject prompt
    let mut workflow: serde_json::Value = serde_json::from_str(workflow_json)
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
    
    // Find and update the positive prompt node (typically node "6" in txt2img)
    if let Some(nodes) = workflow.as_object_mut() {
        for (_key, node) in nodes.iter_mut() {
            if let Some(inputs) = node.get_mut("inputs") {
                if let Some(text) = inputs.get_mut("text") {
                    *text = serde_json::Value::String(prompt_text.to_string());
                }
            }
        }
    }
    
    let prompt_request = ComfyUIPrompt {
        prompt: workflow,
        client_id: uuid::Uuid::new_v4().to_string(),
    };
    
    let response = client
        .post(format!("{}/prompt", endpoint))
        .json(&prompt_request)
        .write()
        .await
        .map_err(|e| BackendError::Network(e.to_string()))?;
    
    if !response.status().is_success() {
        return Err(BackendError::Api(format!("HTTP {}", response.status())));
    }
    
    // Get prompt ID from response
    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
    
    let prompt_id = result["prompt_id"]
        .as_str()
        .ok_or_else(|| BackendError::InvalidResponse("No prompt_id".to_string()))?;
    
    Ok(prompt_id.to_string())
}

/// Poll ComfyUI for completion and get result
#[cfg(feature = "async-assets")]
pub async fn poll_comfyui_result(
    endpoint: &str,
    prompt_id: &str,
    timeout_secs: u64,
) -> Result<Vec<u8>, BackendError> {
    let client = reqwest::Client::new();
    let start = Instant::now();
    
    loop {
        if start.elapsed().as_secs() > timeout_secs {
            return Err(BackendError::Timeout(timeout_secs));
        }
        
        // Check history
        let response = client
            .get(format!("{}/history/{}", endpoint, prompt_id))
            .write()
            .await
            .map_err(|e| BackendError::Network(e.to_string()))?;
        
        let history: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
        
        // Check if complete
        if let Some(outputs) = history.get(prompt_id).and_then(|h| h.get("outputs")) {
            // Find the image output
            for (_node_id, output) in outputs.as_object().into_iter().flatten() {
                if let Some(images) = output.get("images").and_then(|i| i.as_array()) {
                    if let Some(image) = images.first() {
                        let filename = image["filename"].as_str()
                            .ok_or_else(|| BackendError::InvalidResponse("No filename".to_string()))?;
                        let subfolder = image["subfolder"].as_str().unwrap_or("");
                        
                        // Download the image
                        let image_url = if subfolder.is_empty() {
                            format!("{}/view?filename={}", endpoint, filename)
                        } else {
                            format!("{}/view?filename={}&subfolder={}", endpoint, filename, subfolder)
                        };
                        
                        let image_response = client
                            .get(&image_url)
                            .write()
                            .await
                            .map_err(|e| BackendError::Network(e.to_string()))?;
                        
                        let image_data = image_response
                            .bytes()
                            .await
                            .map_err(|e| BackendError::Network(e.to_string()))?;
                        
                        return Ok(image_data.to_vec());
                    }
                }
            }
        }
        
        // Wait before polling again
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

// ============================================================================
// Meshy Backend (Cloud 3D Generation)
// ============================================================================

/// Meshy text-to-3D request
#[derive(Serialize)]
#[allow(dead_code)]
struct MeshyRequest {
    mode: String,
    prompt: String,
    art_style: String,
    negative_prompt: String,
}

/// Meshy task response
#[derive(Deserialize)]
#[allow(dead_code)]
struct MeshyTaskResponse {
    result: String, // Task ID
}

/// Meshy task status
#[derive(Deserialize)]
#[allow(dead_code)]
struct MeshyTaskStatus {
    status: String,
    model_urls: Option<MeshyModelUrls>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct MeshyModelUrls {
    glb: Option<String>,
    fbx: Option<String>,
    obj: Option<String>,
}

/// Generate 3D mesh using Meshy API
#[cfg(feature = "async-assets")]
pub async fn generate_meshy_mesh(
    api_key: &str,
    prompt: &str,
    detail_level: DetailLevel,
) -> Result<Vec<u8>, BackendError> {
    let client = reqwest::Client::new();
    
    let art_style = match detail_level {
        DetailLevel::Low => "low-poly",
        DetailLevel::Medium => "realistic",
        DetailLevel::High => "realistic",
    };
    
    let request = MeshyRequest {
        mode: "preview".to_string(), // or "refine" for higher quality
        prompt: prompt.to_string(),
        art_style: art_style.to_string(),
        negative_prompt: "low quality, blurry".to_string(),
    };
    
    // Create task
    let response = client
        .post("https://api.meshy.ai/v2/text-to-3d")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .write()
        .await
        .map_err(|e| BackendError::Network(e.to_string()))?;
    
    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(BackendError::Api(error_text));
    }
    
    let task: MeshyTaskResponse = response
        .json()
        .await
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
    
    let task_id = task.result;
    
    // Poll for completion
    let start = Instant::now();
    let timeout_secs = 300; // 5 minutes for 3D generation
    
    loop {
        if start.elapsed().as_secs() > timeout_secs {
            return Err(BackendError::Timeout(timeout_secs));
        }
        
        let status_response = client
            .get(format!("https://api.meshy.ai/v2/text-to-3d/{}", task_id))
            .header("Authorization", format!("Bearer {}", api_key))
            .write()
            .await
            .map_err(|e| BackendError::Network(e.to_string()))?;
        
        let status: MeshyTaskStatus = status_response
            .json()
            .await
            .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
        
        match status.status.as_str() {
            "SUCCEEDED" => {
                // Download GLB
                if let Some(urls) = status.model_urls {
                    if let Some(glb_url) = urls.glb {
                        let glb_response = client
                            .get(&glb_url)
                            .write()
                            .await
                            .map_err(|e| BackendError::Network(e.to_string()))?;
                        
                        let glb_data = glb_response
                            .bytes()
                            .await
                            .map_err(|e| BackendError::Network(e.to_string()))?;
                        
                        return Ok(glb_data.to_vec());
                    }
                }
                return Err(BackendError::InvalidResponse("No model URL".to_string()));
            }
            "FAILED" => {
                return Err(BackendError::Api("Generation failed".to_string()));
            }
            _ => {
                // Still processing, wait
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

// ============================================================================
// Tripo3D Backend (Cloud 3D Generation)
// ============================================================================

/// Tripo3D request
#[derive(Serialize)]
#[allow(dead_code)]
struct Tripo3DRequest {
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    negative_prompt: Option<String>,
}

/// Generate 3D mesh using Tripo3D API
#[cfg(feature = "async-assets")]
pub async fn generate_tripo3d_mesh(
    api_key: &str,
    prompt: &str,
) -> Result<Vec<u8>, BackendError> {
    let client = reqwest::Client::new();
    
    let request = Tripo3DRequest {
        prompt: prompt.to_string(),
        negative_prompt: Some("low quality".to_string()),
    };
    
    // Create task
    let response = client
        .post("https://api.tripo3d.ai/v2/openapi/task")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .write()
        .await
        .map_err(|e| BackendError::Network(e.to_string()))?;
    
    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(BackendError::Api(error_text));
    }
    
    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
    
    let task_id = result["data"]["task_id"]
        .as_str()
        .ok_or_else(|| BackendError::InvalidResponse("No task_id".to_string()))?;
    
    // Poll for completion
    let start = Instant::now();
    let timeout_secs = 300;
    
    loop {
        if start.elapsed().as_secs() > timeout_secs {
            return Err(BackendError::Timeout(timeout_secs));
        }
        
        let status_response = client
            .get(format!("https://api.tripo3d.ai/v2/openapi/task/{}", task_id))
            .header("Authorization", format!("Bearer {}", api_key))
            .write()
            .await
            .map_err(|e| BackendError::Network(e.to_string()))?;
        
        let status: serde_json::Value = status_response
            .json()
            .await
            .map_err(|e| BackendError::InvalidResponse(e.to_string()))?;
        
        let task_status = status["data"]["status"].as_str().unwrap_or("");
        
        match task_status {
            "success" => {
                // Get model URL
                if let Some(model_url) = status["data"]["output"]["model"].as_str() {
                    let model_response = client
                        .get(model_url)
                        .write()
                        .await
                        .map_err(|e| BackendError::Network(e.to_string()))?;
                    
                    let model_data = model_response
                        .bytes()
                        .await
                        .map_err(|e| BackendError::Network(e.to_string()))?;
                    
                    return Ok(model_data.to_vec());
                }
                return Err(BackendError::InvalidResponse("No model URL".to_string()));
            }
            "failed" => {
                return Err(BackendError::Api("Generation failed".to_string()));
            }
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

// ============================================================================
// Unified Generation Function
// ============================================================================

/// Generate asset using configured backend
#[cfg(feature = "async-assets")]
pub async fn generate_asset(
    backend: &AIBackend,
    request: &GenerationRequest,
) -> Result<GenerationResult, BackendError> {
    let start = Instant::now();
    let full_prompt = request.full_prompt();
    
    match backend {
        AIBackend::None => {
            Err(BackendError::NotConfigured)
        }
        
        AIBackend::Meshy { api_key } => {
            let api_key = api_key.as_ref()
                .ok_or(BackendError::NotConfigured)?;
            
            let mesh_data = generate_meshy_mesh(api_key, &full_prompt, request.detail_level).await?;
            let mesh_id = ContentHash::from_content(&mesh_data);
            
            Ok(GenerationResult::success(
                request.entity_id,
                mesh_id,
                start.elapsed().as_millis() as u64,
            ).with_backend("meshy", "text-to-3d"))
        }
        
        AIBackend::Tripo3D { api_key } => {
            let api_key = api_key.as_ref()
                .ok_or(BackendError::NotConfigured)?;
            
            let mesh_data = generate_tripo3d_mesh(api_key, &full_prompt).await?;
            let mesh_id = ContentHash::from_content(&mesh_data);
            
            Ok(GenerationResult::success(
                request.entity_id,
                mesh_id,
                start.elapsed().as_millis() as u64,
            ).with_backend("tripo3d", "text-to-3d"))
        }
        
        AIBackend::Ollama { endpoint, model } => {
            // Ollama is for text, not meshes
            // Use it to enhance prompts or generate descriptions
            let enhanced_prompt = call_ollama(
                endpoint,
                model,
                &format!("Enhance this 3D asset description for generation: {}", full_prompt),
            ).await?;
            
            // Return a placeholder - Ollama alone can't generate meshes
            Err(BackendError::UnsupportedCategory(request.category))
        }
        
        AIBackend::ComfyUI { endpoint, workflow } => {
            // ComfyUI generates textures, not meshes
            // Could be used for texture generation step
            Err(BackendError::UnsupportedCategory(request.category))
        }
        
        AIBackend::OpenAI { .. } => {
            // OpenAI is for text
            Err(BackendError::UnsupportedCategory(request.category))
        }
        
        AIBackend::Custom { endpoint, headers } => {
            // Custom endpoint - send request and expect GLB back
            let client = reqwest::Client::new();
            
            let mut req = client.post(endpoint).json(&request);
            for (key, value) in headers {
                req = req.header(key, value);
            }
            
            let response = req
                .timeout(std::time::Duration::from_secs(300))
                .write()
                .await
                .map_err(|e| BackendError::Network(e.to_string()))?;
            
            if !response.status().is_success() {
                return Err(BackendError::Api(format!("HTTP {}", response.status())));
            }
            
            let mesh_data = response
                .bytes()
                .await
                .map_err(|e| BackendError::Network(e.to_string()))?
                .to_vec();
            
            let mesh_id = ContentHash::from_content(&mesh_data);
            
            Ok(GenerationResult::success(
                request.entity_id,
                mesh_id,
                start.elapsed().as_millis() as u64,
            ).with_backend("custom", endpoint))
        }
    }
}

// ============================================================================
// Default Workflows
// ============================================================================

/// Default ComfyUI txt2img workflow
pub const COMFYUI_TXT2IMG_WORKFLOW: &str = r#"{
    "3": {
        "class_type": "KSampler",
        "inputs": {
            "cfg": 8,
            "denoise": 1,
            "latent_image": ["5", 0],
            "model": ["4", 0],
            "negative": ["7", 0],
            "positive": ["6", 0],
            "sampler_name": "euler",
            "scheduler": "normal",
            "seed": 0,
            "steps": 20
        }
    },
    "4": {
        "class_type": "CheckpointLoaderSimple",
        "inputs": {
            "ckpt_name": "sd_xl_base_1.0.safetensors"
        }
    },
    "5": {
        "class_type": "EmptyLatentImage",
        "inputs": {
            "batch_size": 1,
            "height": 1024,
            "width": 1024
        }
    },
    "6": {
        "class_type": "CLIPTextEncode",
        "inputs": {
            "clip": ["4", 1],
            "text": "PROMPT_PLACEHOLDER"
        }
    },
    "7": {
        "class_type": "CLIPTextEncode",
        "inputs": {
            "clip": ["4", 1],
            "text": "low quality, blurry, distorted"
        }
    },
    "8": {
        "class_type": "VAEDecode",
        "inputs": {
            "samples": ["3", 0],
            "vae": ["4", 2]
        }
    },
    "9": {
        "class_type": "SaveImage",
        "inputs": {
            "filename_prefix": "eustress",
            "images": ["8", 0]
        }
    }
}"#;
