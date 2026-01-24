//! # VIGA Context Management
//!
//! Maintains evolving contextual memory with plans, code diffs, and render history.

use std::collections::VecDeque;

/// Maximum number of iterations to keep in history
const MAX_HISTORY_SIZE: usize = 20;

/// VIGA contextual memory
#[derive(Debug, Clone, Default)]
pub struct VigaContext {
    /// Reference image (base64 data URL)
    pub reference_image: Option<String>,
    /// User's text description (optional)
    pub description: Option<String>,
    /// Current plan/strategy
    pub plan: Option<String>,
    /// Iteration history
    pub history: VecDeque<IterationHistory>,
    /// Current iteration number
    pub iteration: u32,
    /// Best similarity score achieved
    pub best_similarity: f32,
    /// Best code that achieved highest similarity
    pub best_code: Option<String>,
    /// Accumulated feedback from verifier
    pub accumulated_feedback: Vec<String>,
}

impl VigaContext {
    /// Create new context with reference image
    pub fn new(reference_image: String) -> Self {
        Self {
            reference_image: Some(reference_image),
            description: None,
            plan: None,
            history: VecDeque::new(),
            iteration: 0,
            best_similarity: 0.0,
            best_code: None,
            accumulated_feedback: Vec::new(),
        }
    }
    
    /// Create context with both image and description
    pub fn with_description(reference_image: String, description: String) -> Self {
        Self {
            reference_image: Some(reference_image),
            description: Some(description),
            plan: None,
            history: VecDeque::new(),
            iteration: 0,
            best_similarity: 0.0,
            best_code: None,
            accumulated_feedback: Vec::new(),
        }
    }
    
    /// Add iteration to history
    pub fn add_iteration(&mut self, entry: IterationHistory) {
        // Update best if this iteration improved
        if entry.similarity > self.best_similarity {
            self.best_similarity = entry.similarity;
            self.best_code = Some(entry.generated_code.clone());
        }
        
        // Add feedback to accumulated list
        if let Some(ref feedback) = entry.verifier_feedback {
            self.accumulated_feedback.push(feedback.clone());
        }
        
        // Add to history
        self.history.push_back(entry);
        
        // Trim old history
        while self.history.len() > MAX_HISTORY_SIZE {
            self.history.pop_front();
        }
        
        self.iteration += 1;
    }
    
    /// Get the last N iterations
    pub fn recent_history(&self, n: usize) -> Vec<&IterationHistory> {
        self.history.iter().rev().take(n).collect()
    }
    
    /// Generate context summary for LLM prompt
    pub fn to_prompt_context(&self) -> String {
        let mut context = String::new();
        
        // Current state
        context.push_str(&format!(
            "# VIGA Context\n\
             Iteration: {}\n\
             Best Similarity: {:.1}%\n\n",
            self.iteration,
            self.best_similarity * 100.0
        ));
        
        // User description if provided
        if let Some(ref desc) = self.description {
            context.push_str(&format!("## User Description\n{}\n\n", desc));
        }
        
        // Current plan
        if let Some(ref plan) = self.plan {
            context.push_str(&format!("## Current Plan\n{}\n\n", plan));
        }
        
        // Recent feedback summary
        if !self.accumulated_feedback.is_empty() {
            context.push_str("## Recent Feedback\n");
            for (i, feedback) in self.accumulated_feedback.iter().rev().take(3).enumerate() {
                context.push_str(&format!("{}. {}\n", i + 1, feedback));
            }
            context.push('\n');
        }
        
        // Recent code diffs
        if !self.history.is_empty() {
            context.push_str("## Recent Changes\n");
            for entry in self.history.iter().rev().take(3) {
                if let Some(ref diff) = entry.code_diff {
                    context.push_str(&format!(
                        "- Iteration {}: {} (similarity: {:.1}%)\n",
                        entry.iteration,
                        diff.summary,
                        entry.similarity * 100.0
                    ));
                }
            }
            context.push('\n');
        }
        
        context
    }
    
    /// Check if we should stop iterating
    pub fn should_stop(&self, max_iterations: u32, target_similarity: f32) -> bool {
        self.iteration >= max_iterations || self.best_similarity >= target_similarity
    }
    
    /// Reset context for new reference image
    pub fn reset(&mut self, reference_image: String) {
        self.reference_image = Some(reference_image);
        self.description = None;
        self.plan = None;
        self.history.clear();
        self.iteration = 0;
        self.best_similarity = 0.0;
        self.best_code = None;
        self.accumulated_feedback.clear();
    }
}

/// Single iteration history entry
#[derive(Debug, Clone)]
pub struct IterationHistory {
    /// Iteration number
    pub iteration: u32,
    /// Generated Rune code
    pub generated_code: String,
    /// Rendered screenshot (base64)
    pub rendered_screenshot: Option<String>,
    /// Similarity score achieved
    pub similarity: f32,
    /// Verifier feedback
    pub verifier_feedback: Option<String>,
    /// Code diff from previous iteration
    pub code_diff: Option<CodeDiff>,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Code difference between iterations
#[derive(Debug, Clone)]
pub struct CodeDiff {
    /// Previous code (if any)
    pub previous: Option<String>,
    /// Current code
    pub current: String,
    /// Summary of changes
    pub summary: String,
    /// Lines added
    pub lines_added: u32,
    /// Lines removed
    pub lines_removed: u32,
}

impl CodeDiff {
    /// Create diff from previous and current code
    pub fn from_codes(previous: Option<&str>, current: &str) -> Self {
        let (lines_added, lines_removed, summary) = if let Some(prev) = previous {
            let prev_lines: Vec<&str> = prev.lines().collect();
            let curr_lines: Vec<&str> = current.lines().collect();
            
            let added = curr_lines.iter()
                .filter(|l| !prev_lines.contains(l))
                .count() as u32;
            let removed = prev_lines.iter()
                .filter(|l| !curr_lines.contains(l))
                .count() as u32;
            
            let summary = if added > 0 && removed > 0 {
                format!("+{} -{} lines", added, removed)
            } else if added > 0 {
                format!("+{} lines", added)
            } else if removed > 0 {
                format!("-{} lines", removed)
            } else {
                "No changes".to_string()
            };
            
            (added, removed, summary)
        } else {
            let lines = current.lines().count() as u32;
            (lines, 0, format!("Initial code ({} lines)", lines))
        };
        
        Self {
            previous: previous.map(|s| s.to_string()),
            current: current.to_string(),
            summary,
            lines_added,
            lines_removed,
        }
    }
}
