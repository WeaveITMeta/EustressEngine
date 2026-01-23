//! # Soul Context
//!
//! Execution context for Soul scripts.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Script execution context
#[derive(Clone, Debug, Default)]
pub struct ScriptContext {
    pub variables: HashMap<String, ContextValue>,
    pub scope_stack: Vec<Scope>,
}

impl ScriptContext {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn set(&mut self, name: &str, value: ContextValue) {
        self.variables.insert(name.to_string(), value);
    }
    
    pub fn get(&self, name: &str) -> Option<&ContextValue> {
        self.variables.get(name)
    }
    
    pub fn push_scope(&mut self) {
        self.scope_stack.push(Scope::default());
    }
    
    pub fn pop_scope(&mut self) -> Option<Scope> {
        self.scope_stack.pop()
    }
}

/// Scope for variable resolution
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub locals: HashMap<String, ContextValue>,
}

/// Context value types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ContextValue {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    Entity(Entity),
    Vec3(Vec3),
    List(Vec<ContextValue>),
    Map(HashMap<String, Box<ContextValue>>),
}

impl Default for ContextValue {
    fn default() -> Self {
        Self::Nil
    }
}
