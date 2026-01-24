//! # VIGA Agent
//!
//! Self-reflective agent that alternates between Generator and Verifier roles.

use bevy::prelude::*;

/// Agent role in the VIGA loop
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    /// Generator: Writes and executes scene programs
    Generator,
    /// Verifier: Examines rendered output and provides feedback
    Verifier,
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRole::Generator => write!(f, "Generator"),
            AgentRole::Verifier => write!(f, "Verifier"),
        }
    }
}

/// Agent state in the VIGA pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentState {
    /// Idle, waiting for work
    #[default]
    Idle,
    /// Analyzing reference image
    AnalyzingReference,
    /// Planning scene structure
    Planning,
    /// Generating Rune code
    Generating,
    /// Waiting for code execution
    Executing,
    /// Waiting for scene render
    Rendering,
    /// Verifying rendered output
    Verifying,
    /// Providing feedback for next iteration
    Feedback,
    /// Completed successfully
    Complete,
    /// Failed with error
    Failed,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentState::Idle => write!(f, "Idle"),
            AgentState::AnalyzingReference => write!(f, "Analyzing reference image..."),
            AgentState::Planning => write!(f, "Planning scene structure..."),
            AgentState::Generating => write!(f, "Generating Rune code..."),
            AgentState::Executing => write!(f, "Executing code..."),
            AgentState::Rendering => write!(f, "Rendering scene..."),
            AgentState::Verifying => write!(f, "Verifying output..."),
            AgentState::Feedback => write!(f, "Generating feedback..."),
            AgentState::Complete => write!(f, "Complete"),
            AgentState::Failed => write!(f, "Failed"),
        }
    }
}

/// VIGA Agent - self-reflective agent for vision-as-inverse-graphics
#[derive(Debug, Clone)]
pub struct VigaAgent {
    /// Current role
    pub role: AgentRole,
    /// Current state
    pub state: AgentState,
    /// Current iteration
    pub iteration: u32,
    /// Maximum iterations
    pub max_iterations: u32,
    /// Target similarity threshold (0.0-1.0)
    pub target_similarity: f32,
    /// Current similarity score
    pub current_similarity: f32,
    /// Last error message
    pub last_error: Option<String>,
}

impl Default for VigaAgent {
    fn default() -> Self {
        Self {
            role: AgentRole::Generator,
            state: AgentState::Idle,
            iteration: 0,
            max_iterations: 10,
            target_similarity: 0.90,
            current_similarity: 0.0,
            last_error: None,
        }
    }
}

impl VigaAgent {
    /// Create new agent with custom settings
    pub fn new(max_iterations: u32, target_similarity: f32) -> Self {
        Self {
            max_iterations,
            target_similarity,
            ..Default::default()
        }
    }
    
    /// Start a new generation task
    pub fn start(&mut self) {
        self.role = AgentRole::Generator;
        self.state = AgentState::AnalyzingReference;
        self.iteration = 0;
        self.current_similarity = 0.0;
        self.last_error = None;
    }
    
    /// Transition to next state
    pub fn next_state(&mut self) {
        self.state = match (&self.role, &self.state) {
            // Generator flow
            (AgentRole::Generator, AgentState::Idle) => AgentState::AnalyzingReference,
            (AgentRole::Generator, AgentState::AnalyzingReference) => AgentState::Planning,
            (AgentRole::Generator, AgentState::Planning) => AgentState::Generating,
            (AgentRole::Generator, AgentState::Generating) => AgentState::Executing,
            (AgentRole::Generator, AgentState::Executing) => AgentState::Rendering,
            (AgentRole::Generator, AgentState::Rendering) => {
                // Switch to Verifier role
                self.role = AgentRole::Verifier;
                AgentState::Verifying
            }
            
            // Verifier flow
            (AgentRole::Verifier, AgentState::Verifying) => AgentState::Feedback,
            (AgentRole::Verifier, AgentState::Feedback) => {
                // Check if we should continue or complete
                if self.should_continue() {
                    self.iteration += 1;
                    self.role = AgentRole::Generator;
                    AgentState::Generating // Skip analysis on subsequent iterations
                } else {
                    AgentState::Complete
                }
            }
            
            // Terminal states
            (_, AgentState::Complete) => AgentState::Complete,
            (_, AgentState::Failed) => AgentState::Failed,
            
            // Default: stay in current state
            _ => self.state,
        };
    }
    
    /// Check if agent should continue iterating
    pub fn should_continue(&self) -> bool {
        self.iteration < self.max_iterations && self.current_similarity < self.target_similarity
    }
    
    /// Mark as failed with error
    pub fn fail(&mut self, error: String) {
        self.state = AgentState::Failed;
        self.last_error = Some(error);
    }
    
    /// Update similarity score
    pub fn update_similarity(&mut self, similarity: f32) {
        self.current_similarity = similarity;
    }
    
    /// Check if complete
    pub fn is_complete(&self) -> bool {
        matches!(self.state, AgentState::Complete)
    }
    
    /// Check if failed
    pub fn is_failed(&self) -> bool {
        matches!(self.state, AgentState::Failed)
    }
    
    /// Check if done (complete or failed)
    pub fn is_done(&self) -> bool {
        self.is_complete() || self.is_failed()
    }
    
    /// Get status message
    pub fn status_message(&self) -> String {
        format!(
            "[{}] {} - Iteration {}/{} - Similarity: {:.1}%",
            self.role,
            self.state,
            self.iteration + 1,
            self.max_iterations,
            self.current_similarity * 100.0
        )
    }
}
