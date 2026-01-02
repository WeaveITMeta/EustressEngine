//! # Soul Parser
//!
//! Parses .md files into Soul AST using pulldown-cmark.
//! Extracts YAML frontmatter, sections, and prose structure.

#[allow(unused_imports)]
use super::{
    SoulAST, ScriptFrontmatter, ScriptService, ScriptType,
    EventHandler, FunctionDef, GlobalVar, QueryDef, RawSection,
};

// ============================================================================
// Parser
// ============================================================================

/// Soul script parser
pub struct SoulParser {
    /// Current parsing state
    #[allow(dead_code)]
    state: ParseState,
    /// Accumulated warnings
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ParseState {
    Start,
    Frontmatter,
    Content,
    InSection,
    InCodeBlock,
}

impl Default for SoulParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SoulParser {
    pub fn new() -> Self {
        Self {
            state: ParseState::Start,
            warnings: Vec::new(),
        }
    }
    
    /// Parse a .md file into AST
    pub fn parse(&mut self, source: &str, source_path: &str) -> Result<SoulAST, ParseError> {
        let mut ast = SoulAST::default();
        ast.source_path = source_path.to_string();
        
        // Split frontmatter and content
        let (frontmatter_str, content) = self.split_frontmatter(source)?;
        
        // Parse frontmatter
        if let Some(fm_str) = frontmatter_str {
            ast.frontmatter = self.parse_frontmatter(&fm_str)?;
            ast.scene = ast.frontmatter.scene.clone();
            
            if let Some(ref service_str) = ast.frontmatter.service {
                ast.service = ScriptService::from_str(service_str)
                    .unwrap_or(ScriptService::Workspace);
            }
            
            if let Some(ref type_str) = ast.frontmatter.script_type {
                ast.script_type = ScriptType::from_str(type_str)
                    .unwrap_or(ScriptType::Mixed);
            }
        }
        
        // Parse content sections
        self.parse_content(&content, &mut ast)?;
        
        // Add warnings
        ast.warnings = self.warnings.clone();
        
        Ok(ast)
    }
    
    /// Split YAML frontmatter from content
    fn split_frontmatter<'a>(&self, source: &'a str) -> Result<(Option<String>, &'a str), ParseError> {
        let source = source.trim();
        
        if !source.starts_with("---") {
            return Ok((None, source));
        }
        
        // Find closing ---
        let rest = &source[3..];
        if let Some(end_idx) = rest.find("\n---") {
            let frontmatter = rest[..end_idx].trim().to_string();
            let content = rest[end_idx + 4..].trim();
            Ok((Some(frontmatter), content))
        } else {
            Err(ParseError::InvalidFrontmatter("Missing closing ---".to_string()))
        }
    }
    
    /// Parse YAML frontmatter
    fn parse_frontmatter(&mut self, yaml_str: &str) -> Result<ScriptFrontmatter, ParseError> {
        // Simple YAML parsing (key: value format)
        let mut fm = ScriptFrontmatter::default();
        
        for line in yaml_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
                
                match key.as_str() {
                    "scene" => fm.scene = value,
                    "service" => fm.service = Some(value),
                    "type" => fm.script_type = Some(value),
                    "unit" => fm.unit = Some(value),
                    "version" => fm.version = Some(value),
                    "author" => fm.author = Some(value),
                    "description" => fm.description = Some(value),
                    _ => {
                        self.warnings.push(format!("Unknown frontmatter key: {}", key));
                    }
                }
            }
        }
        
        if fm.scene.is_empty() {
            return Err(ParseError::MissingField("scene".to_string()));
        }
        
        Ok(fm)
    }
    
    /// Parse content sections
    fn parse_content(&mut self, content: &str, ast: &mut SoulAST) -> Result<(), ParseError> {
        let mut current_section: Option<RawSection> = None;
        // Reserved for future hierarchical section tracking
        let mut _current_heading = String::new();
        let mut _current_level = 0u8;
        
        for line in content.lines() {
            let trimmed = line.trim();
            
            // Check for headings
            if let Some(heading) = self.parse_heading(trimmed) {
                // Save previous section
                if let Some(section) = current_section.take() {
                    self.process_section(section, ast);
                }
                
                _current_heading = heading.1.clone();
                _current_level = heading.0;
                current_section = Some(RawSection {
                    heading: heading.1,
                    level: heading.0,
                    content: Vec::new(),
                    section_type: None,
                });
            } else if let Some(ref mut section) = current_section {
                // Add line to current section
                section.content.push(line.to_string());
                
                // Check for type markers
                if trimmed.contains("🔵") || trimmed.to_lowercase().contains("meta") {
                    section.section_type = Some(ScriptType::Meta);
                } else if trimmed.contains("🟢") || trimmed.to_lowercase().contains("plausible") {
                    section.section_type = Some(ScriptType::Plausible);
                }
            } else if !trimmed.is_empty() {
                // Content before first heading - treat as globals
                if trimmed.starts_with("Global:") || trimmed.starts_with("global:") {
                    if let Some(global) = self.parse_global(trimmed) {
                        ast.add_global(global);
                    }
                }
            }
        }
        
        // Save final section
        if let Some(section) = current_section {
            self.process_section(section, ast);
        }
        
        Ok(())
    }
    
    /// Parse a heading line
    fn parse_heading(&self, line: &str) -> Option<(u8, String)> {
        let trimmed = line.trim();
        
        if trimmed.starts_with("######") {
            Some((6, trimmed[6..].trim().to_string()))
        } else if trimmed.starts_with("#####") {
            Some((5, trimmed[5..].trim().to_string()))
        } else if trimmed.starts_with("####") {
            Some((4, trimmed[4..].trim().to_string()))
        } else if trimmed.starts_with("###") {
            Some((3, trimmed[3..].trim().to_string()))
        } else if trimmed.starts_with("##") {
            Some((2, trimmed[2..].trim().to_string()))
        } else if trimmed.starts_with('#') {
            Some((1, trimmed[1..].trim().to_string()))
        } else {
            None
        }
    }
    
    /// Parse a global variable line
    fn parse_global(&self, line: &str) -> Option<GlobalVar> {
        // Format: "Global: name = value (description)"
        let content = line.strip_prefix("Global:")
            .or_else(|| line.strip_prefix("global:"))?
            .trim();
        
        let (name_value, description) = if let Some(idx) = content.find('(') {
            let desc_end = content.rfind(')')?;
            (content[..idx].trim(), Some(content[idx+1..desc_end].to_string()))
        } else {
            (content, None)
        };
        
        let (name, value) = name_value.split_once('=')?;
        
        Some(GlobalVar {
            name: name.trim().to_string(),
            value: value.trim().to_string(),
            type_hint: None,
            description,
        })
    }
    
    /// Process a completed section into AST nodes
    fn process_section(&mut self, section: RawSection, ast: &mut SoulAST) {
        let heading_lower = section.heading.to_lowercase();
        
        // Determine section type by heading
        if heading_lower.contains("global") {
            // Parse globals from section
            for line in &section.content {
                let trimmed = line.trim();
                if trimmed.starts_with("Global:") || trimmed.starts_with("global:") 
                   || trimmed.starts_with('-') {
                    if let Some(global) = self.parse_global_from_line(trimmed) {
                        ast.add_global(global);
                    }
                }
            }
        } else if heading_lower.starts_with("on ") || heading_lower.contains("event") {
            // Parse event handler
            if let Some(handler) = self.parse_event_handler(&section) {
                ast.add_event_handler(handler);
            }
        } else if heading_lower.starts_with("function:") || heading_lower.contains("function") {
            // Parse function
            if let Some(func) = self.parse_function(&section) {
                ast.add_function(func);
            }
        } else {
            // Store as raw section
            ast.raw_sections.push(section);
        }
    }
    
    /// Parse global from a content line
    fn parse_global_from_line(&self, line: &str) -> Option<GlobalVar> {
        let content = line.trim_start_matches('-').trim();
        
        if content.starts_with("Global:") || content.starts_with("global:") {
            return self.parse_global(content);
        }
        
        // Try "name = value" format
        if let Some((name, value)) = content.split_once('=') {
            return Some(GlobalVar {
                name: name.trim().to_string(),
                value: value.trim().to_string(),
                type_hint: None,
                description: None,
            });
        }
        
        None
    }
    
    /// Parse event handler from section
    fn parse_event_handler(&self, section: &RawSection) -> Option<EventHandler> {
        let mut handler = EventHandler {
            name: section.heading.clone(),
            when: None,
            if_condition: None,
            else_branch: None,
            then_actions: Vec::new(),
            handler_type: section.section_type.unwrap_or(ScriptType::Mixed),
            meta_check: None,
            plausible_edit: None,
        };
        
        let mut in_then = false;
        let mut in_else = false;
        
        for line in &section.content {
            let trimmed = line.trim();
            let lower = trimmed.to_lowercase();
            
            if lower.starts_with("- when:") || lower.starts_with("when:") {
                handler.when = Some(trimmed.split_once(':')?.1.trim().to_string());
                in_then = false;
                in_else = false;
            } else if lower.starts_with("- if:") || lower.starts_with("if:") {
                handler.if_condition = Some(trimmed.split_once(':')?.1.trim().to_string());
                in_then = false;
                in_else = false;
            } else if lower.starts_with("- then:") || lower.starts_with("then:") {
                in_then = true;
                in_else = false;
                let rest = trimmed.split_once(':').map(|(_, v)| v.trim());
                if let Some(action) = rest {
                    if !action.is_empty() {
                        handler.then_actions.push(action.to_string());
                    }
                }
            } else if lower.starts_with("- else:") || lower.starts_with("else:") {
                in_then = false;
                in_else = true;
                handler.else_branch = Some(Vec::new());
            } else if lower.starts_with("- meta check:") || lower.starts_with("meta check:") {
                handler.meta_check = Some(trimmed.split_once(':')?.1.trim().to_string());
                handler.handler_type = ScriptType::Meta;
            } else if lower.starts_with("- plausible") || lower.contains("plausible edit") {
                handler.plausible_edit = Some(trimmed.split_once(':').map(|(_, v)| v.trim().to_string()).unwrap_or_default());
                if handler.handler_type != ScriptType::Meta {
                    handler.handler_type = ScriptType::Plausible;
                }
            } else if trimmed.starts_with('-') && (in_then || in_else) {
                let action = trimmed.trim_start_matches('-').trim().to_string();
                if in_then {
                    handler.then_actions.push(action);
                } else if let Some(ref mut else_branch) = handler.else_branch {
                    else_branch.push(action);
                }
            }
        }
        
        Some(handler)
    }
    
    /// Parse function from section
    fn parse_function(&self, section: &RawSection) -> Option<FunctionDef> {
        let name = section.heading
            .strip_prefix("Function:")
            .or_else(|| section.heading.strip_prefix("function:"))
            .unwrap_or(&section.heading)
            .trim()
            .to_string();
        
        let mut func = FunctionDef {
            name,
            params: Vec::new(),
            returns: None,
            body: Vec::new(),
            instance: None,
        };
        
        let mut in_body = false;
        
        for line in &section.content {
            let trimmed = line.trim();
            let lower = trimmed.to_lowercase();
            
            if lower.starts_with("- params:") || lower.starts_with("params:") {
                let params_str = trimmed.split_once(':')?.1.trim();
                func.params = params_str.split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
            } else if lower.starts_with("- returns:") || lower.starts_with("returns:") {
                func.returns = Some(trimmed.split_once(':')?.1.trim().to_string());
            } else if lower.starts_with("- instance:") || lower.starts_with("instance:") {
                func.instance = Some(trimmed.split_once(':')?.1.trim().to_string());
            } else if lower.starts_with("- body:") || lower.starts_with("body:") {
                in_body = true;
                let rest = trimmed.split_once(':').map(|(_, v)| v.trim());
                if let Some(line) = rest {
                    if !line.is_empty() {
                        func.body.push(line.to_string());
                    }
                }
            } else if trimmed.starts_with('-') && in_body {
                func.body.push(trimmed.trim_start_matches('-').trim().to_string());
            }
        }
        
        Some(func)
    }
}

// ============================================================================
// Parse Error
// ============================================================================

/// Parser error types
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Invalid frontmatter format
    InvalidFrontmatter(String),
    /// Missing required field
    MissingField(String),
    /// Invalid section structure
    InvalidSection(String),
    /// IO error
    IoError(String),
    /// Generic error
    Other(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidFrontmatter(msg) => write!(f, "Invalid frontmatter: {}", msg),
            ParseError::MissingField(field) => write!(f, "Missing required field: {}", field),
            ParseError::InvalidSection(msg) => write!(f, "Invalid section: {}", msg),
            ParseError::IoError(msg) => write!(f, "IO error: {}", msg),
            ParseError::Other(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_frontmatter() {
        let source = r#"---
scene: playground
service: Workspace
type: plausible
---

# Test Content
"#;
        
        let mut parser = SoulParser::new();
        let ast = parser.parse(source, "test.md").unwrap();
        
        assert_eq!(ast.scene, "playground");
        assert_eq!(ast.service, ScriptService::Workspace);
        assert_eq!(ast.script_type, ScriptType::Plausible);
    }
    
    #[test]
    fn test_parse_event_handler() {
        let source = r#"---
scene: test
---

## On Player Land
- When: Player falls faster than 2 meters per second
- Then:
  - Boost upward force by 50%
  - Spawn particles
"#;
        
        let mut parser = SoulParser::new();
        let ast = parser.parse(source, "test.md").unwrap();
        
        assert_eq!(ast.event_handlers.len(), 1);
        assert_eq!(ast.event_handlers[0].name, "On Player Land");
        assert!(ast.event_handlers[0].when.is_some());
        assert!(!ast.event_handlers[0].then_actions.is_empty());
    }
}
