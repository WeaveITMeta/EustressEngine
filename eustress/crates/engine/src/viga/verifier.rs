//! # VIGA Verifier
//!
//! Examines rendered output and provides feedback for the next iteration.

use bevy::prelude::*;

use super::context::VigaContext;
use super::image_utils::{ImageData, ImageComparisonResult, compare_images};

/// Verifier configuration
#[derive(Debug, Clone)]
pub struct VerifierConfig {
    /// Model to use for verification
    pub model: String,
    /// Maximum tokens for verification response
    pub max_tokens: u32,
    /// Similarity threshold to consider "good enough"
    pub similarity_threshold: f32,
    /// Whether to use visual comparison in addition to LLM
    pub use_visual_comparison: bool,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 2048,
            similarity_threshold: 0.90,
            use_visual_comparison: true,
        }
    }
}

/// Verification result
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Similarity score (0.0-1.0)
    pub similarity: f32,
    /// Whether the result is acceptable
    pub acceptable: bool,
    /// Feedback for the generator
    pub feedback: String,
    /// Specific issues identified
    pub issues: Vec<VerificationIssue>,
    /// Image comparison metrics (if available)
    pub image_comparison: Option<ImageComparisonResult>,
}

/// A specific issue identified by the verifier
#[derive(Debug, Clone)]
pub struct VerificationIssue {
    /// Issue category
    pub category: IssueCategory,
    /// Description of the issue
    pub description: String,
    /// Severity (0.0 = minor, 1.0 = critical)
    pub severity: f32,
    /// Suggested fix (if any)
    pub suggestion: Option<String>,
}

/// Categories of verification issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCategory {
    /// Missing object from reference
    MissingObject,
    /// Extra object not in reference
    ExtraObject,
    /// Wrong position
    PositionError,
    /// Wrong scale/size
    ScaleError,
    /// Wrong color
    ColorError,
    /// Wrong material
    MaterialError,
    /// Wrong lighting
    LightingError,
    /// Wrong camera angle
    CameraError,
    /// General structural issue
    StructuralError,
    /// Wrong depth/occlusion order
    DepthError,
    /// Wrong object relationships (parent-child)
    RelationshipError,
    /// Shadow mismatch
    ShadowError,
}

impl std::fmt::Display for IssueCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueCategory::MissingObject => write!(f, "Missing Object"),
            IssueCategory::ExtraObject => write!(f, "Extra Object"),
            IssueCategory::PositionError => write!(f, "Position Error"),
            IssueCategory::ScaleError => write!(f, "Scale Error"),
            IssueCategory::ColorError => write!(f, "Color Error"),
            IssueCategory::DepthError => write!(f, "Depth Error"),
            IssueCategory::RelationshipError => write!(f, "Relationship Error"),
            IssueCategory::ShadowError => write!(f, "Shadow Error"),
            IssueCategory::MaterialError => write!(f, "Material Error"),
            IssueCategory::LightingError => write!(f, "Lighting Error"),
            IssueCategory::CameraError => write!(f, "Camera Error"),
            IssueCategory::StructuralError => write!(f, "Structural Error"),
        }
    }
}

/// VIGA Verifier - examines rendered output and provides feedback
pub struct VigaVerifier {
    /// Configuration
    pub config: VerifierConfig,
}

impl Default for VigaVerifier {
    fn default() -> Self {
        Self {
            config: VerifierConfig::default(),
        }
    }
}

impl VigaVerifier {
    /// Create with custom config
    pub fn new(config: VerifierConfig) -> Self {
        Self { config }
    }
    
    /// Build the system prompt for verification
    pub fn build_system_prompt(&self) -> String {
        r#"You are the Verifier component of VIGA (Vision-as-Inverse-Graphics Agent). Your task is to compare a rendered 3D scene against a reference image and provide specific, actionable feedback.

# YOUR TASK
1. Compare the rendered scene to the reference image
2. Identify specific differences and issues
3. Provide clear, actionable feedback for improvement
4. Estimate a similarity score (0-100%)

# FEEDBACK FORMAT
Structure your response as:

## Similarity Score
[X]% - Brief overall assessment

## What's Correct
- List elements that match well

## Issues Found
For each issue:
- **[Category]**: Description of the problem
  - Suggestion: How to fix it

Categories: Missing Object, Extra Object, Position Error, Scale Error, Color Error, Material Error, Lighting Error, Camera Error

## Priority Fixes
List the 1-3 most important changes to make next, in order of priority.

# SCALE VERIFICATION (1 unit = 1 meter)
Pay special attention to realistic proportions:

## People (IDENTIFY AGE FIRST!)
- Adult: 1.7m tall
- Teenager: 1.5-1.7m tall
- Child (6-12yr): 1.1-1.5m tall
- Toddler: 0.85-1.1m tall

## Objects
- Dining table: 0.75m height, chair seat: 0.45m
- Door: 2.1m height × 0.9m width
- Ceiling: 2.4-3.0m height
- Car: 4.5m × 1.8m × 1.5m
- Sofa: 0.85m height, 2.0m width

If objects look disproportionate (e.g., a chair taller than a table, a door shorter than a person, a child scaled as an adult), flag this as a Scale Error with specific corrections.

# DEPTH & SPATIAL VERIFICATION
Check these depth cues match between reference and render:
- Occlusion order (which objects are in front)
- Relative sizes (farther = smaller)
- Ground contact points
- Shadow positions and directions

# SCENE GRAPH VERIFICATION
Verify object relationships:
- Objects resting on ground vs on other objects
- Parent-child hierarchies (lamp on table, etc.)
- Wall/ceiling attachments

# MATERIAL & LIGHTING VERIFICATION
- Do shiny objects have correct reflective materials?
- Do matte objects have correct diffuse materials?
- Does shadow direction match light source position?
- Is lighting intensity appropriate (indoor vs outdoor)?

# COARSE-TO-FINE FEEDBACK
Adjust your feedback based on iteration:
- **Iterations 1-3**: Focus on STRUCTURE (missing objects, wrong positions, scale errors)
- **Iterations 4-6**: Focus on APPEARANCE (colors, materials, lighting)
- **Iterations 7-10**: Focus on DETAILS (fine adjustments, small objects)

# GUIDELINES
- Be specific: "The red cube should be 2 units to the left" not "position is wrong"
- Be constructive: Always suggest how to fix issues
- Be concise: Focus on the most impactful changes
- Consider perspective: The camera angle affects what's visible
- Prioritize: Follow coarse-to-fine strategy based on iteration number
- Check scale: Ensure objects have realistic real-world proportions
- Identify age: If a person is visible, estimate their age first, then use age-appropriate height as reference
- Verify depth: Check occlusion order and relative sizes match reference"#.to_string()
    }
    
    /// Build the user prompt for verification
    pub fn build_verification_prompt(&self, context: &VigaContext) -> String {
        let mut prompt = String::new();
        
        prompt.push_str("# Reference Image\n");
        prompt.push_str("[First image: Reference]\n\n");
        
        prompt.push_str("# Rendered Scene\n");
        prompt.push_str("[Second image: Current render]\n\n");
        
        prompt.push_str(&format!("# Iteration\nThis is iteration {} of the reconstruction.\n\n", context.iteration + 1));
        
        if let Some(ref desc) = context.description {
            prompt.push_str(&format!("# User Description\n{}\n\n", desc));
        }
        
        // Include previous feedback if available
        if !context.accumulated_feedback.is_empty() {
            prompt.push_str("# Previous Feedback\n");
            for (i, fb) in context.accumulated_feedback.iter().rev().take(2).enumerate() {
                prompt.push_str(&format!("Iteration {}: {}\n", context.iteration - i as u32, fb));
            }
            prompt.push('\n');
        }
        
        prompt.push_str("Compare the rendered scene to the reference and provide feedback.\n");
        
        prompt
    }
    
    /// Perform visual comparison between images
    pub fn compare_visually(
        &self,
        reference: &ImageData,
        rendered: &ImageData,
    ) -> ImageComparisonResult {
        compare_images(reference, rendered)
    }
    
    /// Parse verification response from LLM
    pub fn parse_verification_response(&self, response: &str) -> VerificationResult {
        let mut similarity = 0.5; // Default
        let mut issues = Vec::new();
        let mut feedback_parts = Vec::new();
        
        // Extract similarity score
        if let Some(score_match) = extract_percentage(response) {
            similarity = score_match / 100.0;
        }
        
        // Parse issues from response
        let lines: Vec<&str> = response.lines().collect();
        let mut current_category: Option<IssueCategory> = None;
        
        for line in &lines {
            let line = line.trim();
            
            // Check for category markers
            if line.contains("Missing Object") || line.contains("missing") {
                current_category = Some(IssueCategory::MissingObject);
            } else if line.contains("Extra Object") || line.contains("extra") {
                current_category = Some(IssueCategory::ExtraObject);
            } else if line.contains("Position") || line.contains("position") {
                current_category = Some(IssueCategory::PositionError);
            } else if line.contains("Scale") || line.contains("size") {
                current_category = Some(IssueCategory::ScaleError);
            } else if line.contains("Color") || line.contains("color") {
                current_category = Some(IssueCategory::ColorError);
            } else if line.contains("Material") || line.contains("material") {
                current_category = Some(IssueCategory::MaterialError);
            } else if line.contains("Light") || line.contains("light") {
                current_category = Some(IssueCategory::LightingError);
            } else if line.contains("Camera") || line.contains("camera") || line.contains("angle") {
                current_category = Some(IssueCategory::CameraError);
            }
            
            // Extract issue descriptions (lines starting with - or *)
            if (line.starts_with('-') || line.starts_with('*')) && line.len() > 2 {
                let description = line[1..].trim().to_string();
                if !description.is_empty() && description.len() > 5 {
                    let category = current_category.unwrap_or(IssueCategory::StructuralError);
                    issues.push(VerificationIssue {
                        category,
                        description: description.clone(),
                        severity: 0.5,
                        suggestion: None,
                    });
                    feedback_parts.push(description);
                }
            }
        }
        
        // Build feedback string
        let feedback = if feedback_parts.is_empty() {
            response.to_string()
        } else {
            feedback_parts.join("\n")
        };
        
        let acceptable = similarity >= self.config.similarity_threshold;
        
        VerificationResult {
            similarity,
            acceptable,
            feedback,
            issues,
            image_comparison: None,
        }
    }
    
    /// Combine LLM verification with visual comparison
    /// 
    /// Uses 85% LLM weight because Claude's semantic understanding is more
    /// reliable for scene reconstruction than pixel-level comparison.
    /// The visual comparison provides a sanity check and detailed metrics.
    pub fn combine_results(
        &self,
        llm_result: VerificationResult,
        visual_result: ImageComparisonResult,
    ) -> VerificationResult {
        // Weight: 85% LLM assessment, 15% visual comparison
        // LLM understands "the table is correct but wrong color" semantically
        // Visual comparison catches pixel-level issues LLM might miss
        let combined_similarity = llm_result.similarity * 0.85 + visual_result.similarity * 0.15;
        
        // Add visual comparison issues
        let mut issues = llm_result.issues;
        for region in &visual_result.difference_regions {
            if region.severity > 0.3 {
                issues.push(VerificationIssue {
                    category: IssueCategory::StructuralError,
                    description: region.description.clone(),
                    severity: region.severity,
                    suggestion: None,
                });
            }
        }
        
        // Enhance feedback with visual metrics
        let enhanced_feedback = format!(
            "{}\n\n📊 Visual Metrics:\n- SSIM: {:.1}%\n- Color Match: {:.1}%\n- Structure Match: {:.1}%\n- Edge Match: {:.1}%",
            llm_result.feedback,
            visual_result.ssim * 100.0,
            visual_result.histogram_similarity * 100.0,
            visual_result.phash_similarity * 100.0,
            visual_result.edge_similarity * 100.0
        );
        
        VerificationResult {
            similarity: combined_similarity,
            acceptable: combined_similarity >= self.config.similarity_threshold,
            feedback: enhanced_feedback,
            issues,
            image_comparison: Some(visual_result),
        }
    }
}

/// Extract percentage from text (e.g., "75%" -> 75.0)
fn extract_percentage(text: &str) -> Option<f32> {
    // Look for patterns like "75%", "75 %", "[75]%"
    let mut chars = text.chars().peekable();
    let mut num_str = String::new();
    let mut found_percent = false;
    
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num_str.push(c);
        } else if c == '%' && !num_str.is_empty() {
            found_percent = true;
            break;
        } else if !num_str.is_empty() && !c.is_whitespace() && c != '[' && c != ']' {
            num_str.clear();
        }
    }
    
    if found_percent {
        num_str.parse::<f32>().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_percentage() {
        assert_eq!(extract_percentage("75%"), Some(75.0));
        assert_eq!(extract_percentage("[85]%"), Some(85.0));
        assert_eq!(extract_percentage("Score: 90%"), Some(90.0));
        assert_eq!(extract_percentage("no percentage here"), None);
    }
}
