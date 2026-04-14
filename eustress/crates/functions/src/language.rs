//! # Stage 8: Language — Embedding, Tokenization & Lexical Analysis
//!
//! Exposes text processing capabilities to Rune scripts for natural language
//! embedding, tokenization, and lightweight grammar-based lexing.
//!
//! ## Table of Contents
//! 1. Result types     — Token, LexResult
//! 2. LanguageBridge   — thread-local access to PropertyEmbedder
//! 3. Rune functions   — embed_query / tokenize / lex
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                    | Purpose                                                    |
//! |-----------------------------|------------------------------------------------------------|
//! | `embed_query(text)`         | Convert text to embedding vector (CSV f32 string)          |
//! | `tokenize(text)`            | Split text into tokens with type classification            |
//! | `lex(text, grammar)`        | Parse text against a named grammar for DSL/command parsing |
//!
//! ## Backing
//! - `embed_query`: `PropertyEmbedder::embed_query()` from eustress-embedvec
//! - `tokenize`: Whitespace + punctuation splitter with token type inference
//! - `lex`: Keyword-based grammar matching (built-in grammars: "soul", "command", "math")
//!
//! ## Bridge population
//!
//! ```rust,ignore
//! use eustress_functions::language::{LanguageBridge, set_language_bridge};
//! use eustress_embedvec::SimpleHashEmbedder;
//! use std::sync::Arc;
//!
//! set_language_bridge(LanguageBridge {
//!     embedder: Arc::new(SimpleHashEmbedder::new(128)),
//! });
//! ```
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::language;
//!
//! pub fn semantic_tag(description) {
//!     let vec_csv = language::embed_query(description);
//!     let tokens = language::tokenize(description);
//!     for tok in tokens {
//!         eustress::log_info(&format!("[{}] {}", tok.kind, tok.text));
//!     }
//! }
//! ```

use std::cell::RefCell;
use std::sync::Arc;
use tracing::{info, warn};

use eustress_embedvec::PropertyEmbedder;

// ============================================================================
// 1. Result Types
// ============================================================================

/// A single token produced by `tokenize()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct Token {
    /// The raw text of the token
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub text: String,
    /// Token kind: "word", "number", "punctuation", "whitespace", "operator"
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub kind: String,
    /// 0-indexed byte offset in the original string
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub offset: i64,
}

/// A lexed match produced by `lex()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct LexResult {
    /// Whether the grammar matched successfully
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub matched: bool,
    /// The grammar name that was applied
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub grammar: String,
    /// Matched rule or command name (empty if no match)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub rule: String,
    /// Extracted arguments/operands as a CSV string
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub args: String,
}

impl LexResult {
    fn no_match(grammar: &str) -> Self {
        Self {
            matched: false,
            grammar: grammar.to_string(),
            rule: String::new(),
            args: String::new(),
        }
    }
}

// ============================================================================
// 2. LanguageBridge — thread-local embedder access
// ============================================================================

/// Bridge providing Rune access to a `PropertyEmbedder` for text embedding.
pub struct LanguageBridge {
    /// The embedder used for `embed_query()`
    pub embedder: Arc<dyn PropertyEmbedder + Send + Sync>,
}

thread_local! {
    static LANGUAGE_BRIDGE: RefCell<Option<LanguageBridge>> = RefCell::new(None);
}

/// Install the language bridge before Rune execution.
pub fn set_language_bridge(bridge: LanguageBridge) {
    LANGUAGE_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the bridge after Rune execution.
pub fn take_language_bridge() -> Option<LanguageBridge> {
    LANGUAGE_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&LanguageBridge) -> R,
{
    LANGUAGE_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Language] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Convert natural language text into an embedding vector.
///
/// Uses the configured `PropertyEmbedder::embed_query()` to produce a
/// dense vector representation. Returns the vector as a comma-separated
/// f32 string for interop with other DSL functions (e.g. `proximity::compose`).
///
/// # Arguments
/// * `text` — Any natural language string
///
/// # Returns
/// Comma-separated f32 values (e.g. `"0.123,-0.456,0.789,..."`),
/// or empty string if the bridge is unavailable.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn embed_query(text: &str) -> String {
    with_bridge(String::new(), |bridge| {
        match bridge.embedder.embed_query(text) {
            Ok(embedding) => {
                let csv = embedding
                    .iter()
                    .map(|v| format!("{:.6}", v))
                    .collect::<Vec<_>>()
                    .join(",");

                info!(
                    "[Language] embed_query('{}') → {} dims",
                    &text[..text.len().min(40)],
                    embedding.len()
                );

                csv
            }
            Err(e) => {
                warn!("[Language] embed_query failed: {}", e);
                String::new()
            }
        }
    })
}

/// Split text into classified tokens.
///
/// Splits on whitespace and punctuation boundaries. Each token has:
/// - `text`   — the raw token string
/// - `kind`   — "word", "number", "punctuation", "operator"
/// - `offset` — byte offset in the original string
///
/// # Arguments
/// * `text` — Input string to tokenize
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn tokenize(text: &str) -> rune::runtime::Vec {
    let mut tokens = rune::runtime::Vec::new();
    let mut current = String::new();
    let mut current_offset: usize = 0;
    let mut char_offset: usize = 0;

    for ch in text.chars() {
        let ch_len = ch.len_utf8();

        if ch.is_alphanumeric() || ch == '_' || ch == '\'' {
            if current.is_empty() {
                current_offset = char_offset;
            }
            current.push(ch);
        } else {
            // Flush accumulated word/number token
            if !current.is_empty() {
                let kind = classify_token(&current);
                let tok = Token {
                    text: current.clone(),
                    kind,
                    offset: current_offset as i64,
                };
                if let Ok(v) = rune::to_value(tok) {
                    let _ = tokens.push(v);
                }
                current.clear();
            }

            // Emit single-char token (skip whitespace)
            if !ch.is_whitespace() {
                let kind = if "+-*/=<>!&|^~%".contains(ch) {
                    "operator"
                } else {
                    "punctuation"
                };
                let tok = Token {
                    text: ch.to_string(),
                    kind: kind.to_string(),
                    offset: char_offset as i64,
                };
                if let Ok(v) = rune::to_value(tok) {
                    let _ = tokens.push(v);
                }
            }
        }

        char_offset += ch_len;
    }

    // Flush trailing token
    if !current.is_empty() {
        let kind = classify_token(&current);
        let tok = Token {
            text: current.clone(),
            kind,
            offset: current_offset as i64,
        };
        if let Ok(v) = rune::to_value(tok) {
            let _ = tokens.push(v);
        }
    }

    info!(
        "[Language] tokenize('{}') → {} tokens",
        &text[..text.len().min(40)],
        tokens.len()
    );

    tokens
}

/// Parse text against a named grammar and extract structured matches.
///
/// ## Built-in Grammars
///
/// | Grammar    | Matches                                             | Example input           |
/// |------------|-----------------------------------------------------|-------------------------|
/// | `soul`     | Soul script commands (`on`, `wait`, `fire`, `set`)  | `"on Touched do fire"`  |
/// | `command`  | Slash-style commands (`/verb arg1 arg2`)             | `"/move 10 0 5"`        |
/// | `math`     | Simple binary expressions (`a op b`)                | `"mass * velocity"`     |
///
/// Returns `LexResult` with `matched=false` and empty fields if no rule fires.
///
/// # Arguments
/// * `text`    — Input text to lex
/// * `grammar` — Grammar name to apply
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn lex(text: &str, grammar: &str) -> LexResult {
    let result = apply_grammar(text.trim(), grammar);

    info!(
        "[Language] lex('{}', '{}') → matched={} rule='{}'",
        &text[..text.len().min(40)],
        grammar,
        result.matched,
        result.rule
    );

    result
}

// ============================================================================
// Helpers
// ============================================================================

/// Classify a word-boundary token as "word" or "number".
fn classify_token(s: &str) -> String {
    if s.parse::<f64>().is_ok() {
        "number".to_string()
    } else {
        "word".to_string()
    }
}

/// Apply a named grammar to input text.
fn apply_grammar(text: &str, grammar: &str) -> LexResult {
    match grammar {
        "soul" => lex_soul(text),
        "command" => lex_command(text),
        "math" => lex_math(text),
        unknown => {
            warn!("[Language] Unknown grammar '{}' — supported: soul, command, math", unknown);
            LexResult::no_match(unknown)
        }
    }
}

/// Soul script grammar: `on <event> do <action> [arg]`
fn lex_soul(text: &str) -> LexResult {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        // "on <event> do <action>"
        ["on", event, "do", action] => LexResult {
            matched: true,
            grammar: "soul".to_string(),
            rule: "on_do".to_string(),
            args: format!("{},{}", event, action),
        },
        // "on <event> do <action> <arg>"
        ["on", event, "do", action, arg] => LexResult {
            matched: true,
            grammar: "soul".to_string(),
            rule: "on_do_arg".to_string(),
            args: format!("{},{},{}", event, action, arg),
        },
        // "wait <seconds>"
        ["wait", secs] => LexResult {
            matched: true,
            grammar: "soul".to_string(),
            rule: "wait".to_string(),
            args: secs.to_string(),
        },
        // "fire <event>"
        ["fire", event] => LexResult {
            matched: true,
            grammar: "soul".to_string(),
            rule: "fire".to_string(),
            args: event.to_string(),
        },
        // "set <property> <value>"
        ["set", property, value] => LexResult {
            matched: true,
            grammar: "soul".to_string(),
            rule: "set".to_string(),
            args: format!("{},{}", property, value),
        },
        _ => LexResult::no_match("soul"),
    }
}

/// Command grammar: `/<verb> [args...]`
fn lex_command(text: &str) -> LexResult {
    if !text.starts_with('/') {
        return LexResult::no_match("command");
    }

    let rest = &text[1..];
    let mut parts = rest.splitn(2, ' ');
    let verb = parts.next().unwrap_or("").trim();
    let args = parts.next().unwrap_or("").trim();

    if verb.is_empty() {
        return LexResult::no_match("command");
    }

    LexResult {
        matched: true,
        grammar: "command".to_string(),
        rule: verb.to_string(),
        args: args.replace(' ', ","),
    }
}

/// Math grammar: simple binary expression `<a> <op> <b>`
fn lex_math(text: &str) -> LexResult {
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.as_slice() {
        [a, op, b] if ["+", "-", "*", "/", "^", "%"].contains(op) => LexResult {
            matched: true,
            grammar: "math".to_string(),
            rule: op.to_string(),
            args: format!("{},{}", a, b),
        },
        _ => LexResult::no_match("math"),
    }
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `language` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_language_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "language"])?;

    module.ty::<Token>()?;
    module.ty::<LexResult>()?;

    module.function_meta(embed_query)?;
    module.function_meta(tokenize)?;
    module.function_meta(lex)?;

    Ok(module)
}
