//! # Language services
//!
//! The seam between the editor shell and each scripting language. The shell
//! owns text, files, focus, drawing and keys; a [`LanguageService`] owns what
//! the text means: its tokens, its editing conventions and its static
//! analysis. The shell holds a `dyn LanguageService` chosen by
//! [`language_for_path`] and never branches on the language itself.
//!
//! Design note: `docs/design/SCRIPT_EDITOR.md`.

use std::any::Any;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use super::analyzer::{Diagnostic, Range, Severity, Symbol, SymbolKind};

/// Which service a file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Rune,
    Luau,
    Markdown,
    /// Every other text file: highlighted by the syntect fallback, no
    /// analysis.
    Plain,
}

/// A lexer's state at the end of a line. Each service encodes it however it
/// likes (block comment depth, long-string level, interpolation nesting); the
/// shell only stores and compares it.
pub type LineState = u64;

/// Semantic class of a token. Services choose classes; the theme chooses
/// colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// One token of a line. Offsets are bytes within the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub start: u32,
    pub len: u32,
    pub class: TokenClass,
}

/// What `analyze` returns, and what `hover` and `definition` read back.
#[derive(Clone, Default)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
    /// The service's own index (a Luau type graph, a Rune unit's items). The
    /// shell stores it with the revision it came from and hands it back
    /// unchanged; only the service that made it downcasts it.
    pub service_data: Option<Arc<dyn Any + Send + Sync>>,
}

impl fmt::Debug for Analysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analysis")
            .field("diagnostics", &self.diagnostics)
            .field("symbols", &self.symbols)
            .field("service_data", &self.service_data.as_ref().map(|_| "<service data>"))
            .finish()
    }
}

/// The member access right before the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// `receiver.`
    Dot,
    /// `receiver:` (a Luau method call).
    Colon,
}

/// The string argument the caret sits in, for completions such as
/// `GetService("` or `FindFirstChild("`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringArg {
    /// The called name as written (`GetService`, `game:GetService`).
    pub callee: String,
    /// Zero-based index of the argument holding the caret.
    pub arg_index: u32,
}

/// The script's run context, from its Rojo suffix or its instance's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptClass {
    Script,
    LocalScript,
    ModuleScript,
}

/// A read-only view of the Space's instance tree, for completing children by
/// name.
pub trait InstanceTreeView: Send + Sync {
    /// `(name, class_name)` of each child of the instance at `path`, a dotted
    /// path from the root (`game.Workspace.Map`). Empty when the path names
    /// nothing.
    fn children(&self, path: &str) -> Vec<(String, String)>;
}

/// Everything the shell knows about the caret when it asks for completions.
#[derive(Clone)]
pub struct CompletionContext<'a> {
    pub source: &'a str,
    /// Byte offset of the caret in `source`.
    pub offset: usize,
    /// The identifier characters immediately before the caret.
    pub prefix: &'a str,
    /// The expression before a `.` or `:` access, as written
    /// (`game.Workspace`).
    pub receiver: Option<&'a str>,
    pub access: Option<Access>,
    pub in_string: Option<StringArg>,
    pub script_class: Option<ScriptClass>,
    /// `None` until the tree view is wired.
    pub instance_tree: Option<Arc<dyn InstanceTreeView>>,
}

/// What a completion inserts, for its icon and sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    Keyword,
    Snippet,
    Function,
    Method,
    Property,
    Event,
    Variable,
    Constant,
    Class,
    Enum,
    Module,
    Service,
    Instance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Shown in the list and matched against the prefix.
    pub label: String,
    pub kind: CompletionKind,
    /// One line beside the label: a signature or a type.
    pub detail: String,
    /// Replaces the prefix. May differ from the label (a snippet body, a
    /// quoted service name).
    pub insert_text: String,
    pub documentation: Option<String>,
}

/// The call the caret is inside, with its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    /// The whole signature as displayed (`FindFirstChild(name: string, recursive: boolean?)`).
    pub label: String,
    /// Byte range of each parameter within `label`.
    pub parameters: Vec<(u32, u32)>,
    /// Index into `parameters` of the argument holding the caret.
    pub active_parameter: u32,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub markdown: String,
    /// Byte range in the source the hover describes.
    pub byte_range: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// `None` for a place in the same file.
    pub path: Option<PathBuf>,
    pub range: Range,
    pub byte_range: (u32, u32),
}

/// One language's knowledge. Implementations are stateless or internally
/// synchronised: the shell calls tokenization on the main thread and analysis
/// on a worker.
pub trait LanguageService: Send + Sync + 'static {
    fn id(&self) -> LanguageId;

    // Highlighting. A line is lexed from the state the previous line ended
    // in, so the shell lexes only visible lines and stops re-lexing once a
    // line's end state matches the cached one.
    fn initial_state(&self) -> LineState;
    fn tokenize_line(&self, line: &str, state: LineState) -> (Vec<Token>, LineState);

    // Editing conventions.
    fn line_comment(&self) -> Option<&'static str>;
    fn auto_close_pairs(&self) -> &'static [(char, char)];
    /// Whether Enter after `line` indents one level deeper.
    fn indent_after(&self, line: &str) -> bool;
    /// Whether `line`, as typed so far, closes a block and moves out one
    /// level (`end`, `}`, `else`).
    fn dedent_line(&self, line: &str) -> bool;

    // Intelligence. Static only: a service never executes user code.
    fn analyze(&self, source: &str) -> Analysis;
    fn complete(&self, cx: &CompletionContext) -> Vec<Completion>;
    fn signature_help(&self, source: &str, offset: usize) -> Option<SignatureHelp>;
    fn hover(&self, source: &str, offset: usize, analysis: &Analysis) -> Option<Hover>;
    fn definition(&self, source: &str, offset: usize, analysis: &Analysis) -> Option<Location>;
}

/// The language of a source file, from its real file name (never a tab
/// title). Understands Rojo's run-context suffixes.
pub fn language_for_path(path: &Path) -> LanguageId {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "luau" | "lua" => LanguageId::Luau,
        "rune" | "rn" | "soul" => LanguageId::Rune,
        "md" | "markdown" => LanguageId::Markdown,
        _ => LanguageId::Plain,
    }
}

/// The run context a Rojo suffix names: `.server.luau` is a Script,
/// `.client.luau` a LocalScript, `.module.luau` a ModuleScript (and the same
/// for `.lua`). `None` when the file name does not say; the instance's class
/// decides then.
pub fn script_class_for_path(path: &Path) -> Option<ScriptClass> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let stem = name
        .strip_suffix(".luau")
        .or_else(|| name.strip_suffix(".lua"))?;
    if stem.ends_with(".server") {
        Some(ScriptClass::Script)
    } else if stem.ends_with(".client") {
        Some(ScriptClass::LocalScript)
    } else if stem.ends_with(".module") {
        Some(ScriptClass::ModuleScript)
    } else {
        None
    }
}

/// The services the shell can hand a file to, one per [`LanguageId`].
#[derive(Default, Clone)]
pub struct LanguageRegistry {
    services: Vec<Arc<dyn LanguageService>>,
}

impl LanguageRegistry {
    /// Add a service, replacing any earlier one for the same language.
    pub fn register(&mut self, service: Arc<dyn LanguageService>) {
        let id = service.id();
        self.services.retain(|s| s.id() != id);
        self.services.push(service);
    }

    pub fn get(&self, id: LanguageId) -> Option<Arc<dyn LanguageService>> {
        self.services.iter().find(|s| s.id() == id).cloned()
    }

    /// The service for a source file, or `None` when its language has none
    /// registered (the shell then highlights with the syntect fallback).
    pub fn for_path(&self, path: &Path) -> Option<Arc<dyn LanguageService>> {
        self.get(language_for_path(path))
    }
}

impl fmt::Debug for LanguageRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.services.iter().map(|s| s.id())).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_map_to_their_language() {
        let cases = [
            ("S/ClientController/ClientController.client.luau", LanguageId::Luau),
            ("S/Main/Main.server.luau", LanguageId::Luau),
            ("S/Util/Util.module.luau", LanguageId::Luau),
            ("S/Util/init.server.lua", LanguageId::Luau),
            ("S/Old/Old.lua", LanguageId::Luau),
            ("S/Door/Door.rune", LanguageId::Rune),
            ("S/Door/Door.soul", LanguageId::Rune),
            ("S/Door/Door.rn", LanguageId::Rune),
            ("S/Door/Door.md", LanguageId::Markdown),
            ("README.MARKDOWN", LanguageId::Markdown),
            ("Cargo.toml", LanguageId::Plain),
            ("main.rs", LanguageId::Plain),
            ("Makefile", LanguageId::Plain),
        ];
        for (path, want) in cases {
            assert_eq!(language_for_path(Path::new(path)), want, "{path}");
        }
    }

    #[test]
    fn rojo_suffixes_name_the_run_context() {
        let cases = [
            ("A.server.luau", Some(ScriptClass::Script)),
            ("init.server.luau", Some(ScriptClass::Script)),
            ("A.client.luau", Some(ScriptClass::LocalScript)),
            ("A.client.lua", Some(ScriptClass::LocalScript)),
            ("A.module.luau", Some(ScriptClass::ModuleScript)),
            ("A.luau", None),
            ("A.rune", None),
        ];
        for (path, want) in cases {
            assert_eq!(script_class_for_path(Path::new(path)), want, "{path}");
        }
    }

    struct Stub(LanguageId);
    impl LanguageService for Stub {
        fn id(&self) -> LanguageId { self.0 }
        fn initial_state(&self) -> LineState { 0 }
        fn tokenize_line(&self, _: &str, state: LineState) -> (Vec<Token>, LineState) { (Vec::new(), state) }
        fn line_comment(&self) -> Option<&'static str> { None }
        fn auto_close_pairs(&self) -> &'static [(char, char)] { &[] }
        fn indent_after(&self, _: &str) -> bool { false }
        fn dedent_line(&self, _: &str) -> bool { false }
        fn analyze(&self, _: &str) -> Analysis { Analysis::default() }
        fn complete(&self, _: &CompletionContext) -> Vec<Completion> { Vec::new() }
        fn signature_help(&self, _: &str, _: usize) -> Option<SignatureHelp> { None }
        fn hover(&self, _: &str, _: usize, _: &Analysis) -> Option<Hover> { None }
        fn definition(&self, _: &str, _: usize, _: &Analysis) -> Option<Location> { None }
    }

    #[test]
    fn the_registry_hands_each_file_to_its_own_service() {
        let mut reg = LanguageRegistry::default();
        reg.register(Arc::new(Stub(LanguageId::Rune)));
        reg.register(Arc::new(Stub(LanguageId::Luau)));
        reg.register(Arc::new(Stub(LanguageId::Luau)));

        assert_eq!(reg.for_path(Path::new("a.client.luau")).map(|s| s.id()), Some(LanguageId::Luau));
        assert_eq!(reg.for_path(Path::new("a.rune")).map(|s| s.id()), Some(LanguageId::Rune));
        assert!(reg.for_path(Path::new("a.toml")).is_none());
        assert_eq!(format!("{reg:?}"), "[Rune, Luau]", "a second Luau service replaces the first");
    }
}
