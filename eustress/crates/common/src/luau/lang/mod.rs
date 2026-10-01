//! Luau as the script editor reads it: tokens for highlighting, the editing
//! conventions, completion, signature help and hover from the catalog, and
//! syntax errors from the VM's own compiler. Static only: nothing here runs a
//! script.
//!
//! The editor's Luau service (`engine/src/script_editor/luau_service.rs`)
//! adapts these to its `LanguageService` trait.

pub mod complete;
pub mod help;
pub mod lexer;

/// A lexer's state at the end of a line.
pub type LineState = u64;

/// A token's semantic class. The editor maps each to a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenClass {
    Keyword,
    ControlFlow,
    Type,
    Function,
    Method,
    Property,
    Variable,
    Parameter,
    Constant,
    Builtin,
    Number,
    String,
    StringEscape,
    Interpolation,
    Comment,
    DocComment,
    Operator,
    Punctuation,
    Attribute,
    Invalid,
    Text,
}

/// One token of a line; offsets are bytes within the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: u32,
    pub len: u32,
    pub class: TokenClass,
}

/// A syntax error the VM's compiler reports, on a 1-based line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub line: u32,
    pub message: String,
}

/// Whether the Play VM would accept `source`: its Luau compiler's first
/// syntax error, or `None`. Compiling runs nothing.
pub fn check(source: &str) -> Option<SyntaxError> {
    match mlua::Compiler::new().compile(source) {
        Ok(_) => None,
        Err(mlua::Error::SyntaxError { message, .. }) => {
            // The compiler writes `<line>: <message>`.
            let (line, text) = match message.split_once(':') {
                Some((n, rest)) if n.trim().parse::<u32>().is_ok() => (n.trim().parse().unwrap_or(1), rest.trim()),
                _ => (1, message.trim()),
            };
            Some(SyntaxError { line, message: text.to_string() })
        }
        Err(other) => Some(SyntaxError { line: 1, message: other.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiler_reports_where_a_script_breaks() {
        assert_eq!(check("local x = 1\nprint(x)\n"), None);
        let e = check("local x = 1\nif x then\n  print(x)\n").expect("a missing end is an error");
        assert!(e.line >= 2, "the error sits at or after the unclosed `if`: {e:?}");
        assert!(e.message.contains("end"), "{e:?}");
        assert!(check("local t = `a {b} c`\nlocal n: number = 1\nn += 1\ncontinue_ = 1\n").is_none(), "Luau syntax is Luau");
    }
}
