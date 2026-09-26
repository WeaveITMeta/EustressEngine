//! # The Luau service
//!
//! Highlighting, editing conventions, completion, signature help, hover,
//! go-to-definition and syntax errors for `.luau` files. The knowledge lives
//! in `eustress_common::luau`: the lexer and the completion engine in
//! [`lang`], and the members a script can reach in the catalog the Play VM
//! resolves them through, so the editor offers exactly what runs. This file
//! adapts them to [`LanguageService`].
//!
//! Syntax errors come from full-moon ([`luau_parse`]), every one with its
//! span. The VM's own Luau compiler has the last word: code the parser
//! accepts but the VM refuses still shows the compiler's error. Nothing is
//! run.

use eustress_common::luau::lang::{self, complete as lc, help};

use super::luau_parse;

use super::language::{
    Access, Analysis, Completion, CompletionContext, CompletionKind, Diagnostic, Hover, LanguageId, LanguageService,
    LineState, Location, Range, ScriptClass, Severity, SignatureHelp, Symbol, SymbolKind, Token, TokenClass,
};

pub struct LuauService;

fn token_class(c: lang::TokenClass) -> TokenClass {
    use lang::TokenClass as L;
    match c {
        L::Keyword => TokenClass::Keyword,
        L::ControlFlow => TokenClass::ControlFlow,
        L::Type => TokenClass::Type,
        L::Function => TokenClass::Function,
        L::Method => TokenClass::Method,
        L::Property => TokenClass::Property,
        L::Variable => TokenClass::Variable,
        L::Parameter => TokenClass::Parameter,
        L::Constant => TokenClass::Constant,
        L::Builtin => TokenClass::Builtin,
        L::Number => TokenClass::Number,
        L::String => TokenClass::String,
        L::StringEscape => TokenClass::StringEscape,
        L::Interpolation => TokenClass::Interpolation,
        L::Comment => TokenClass::Comment,
        L::DocComment => TokenClass::DocComment,
        L::Operator => TokenClass::Operator,
        L::Punctuation => TokenClass::Punctuation,
        L::Attribute => TokenClass::Attribute,
        L::Invalid => TokenClass::Invalid,
        L::Text => TokenClass::Text,
    }
}

fn completion_kind(k: lc::ItemKind) -> CompletionKind {
    use lc::ItemKind as K;
    match k {
        K::Method => CompletionKind::Method,
        K::Property | K::Field => CompletionKind::Property,
        K::Event => CompletionKind::Event,
        // A callback is assigned (`remote.OnServerInvoke = ...`), never
        // called, so it must not take the `()` a function gets on accept.
        K::Callback => CompletionKind::Property,
        K::Function => CompletionKind::Function,
        K::Constant => CompletionKind::Constant,
        K::Library => CompletionKind::Module,
        K::Class => CompletionKind::Class,
        K::Service => CompletionKind::Service,
        K::EnumType | K::EnumItem => CompletionKind::Enum,
        K::Keyword => CompletionKind::Keyword,
        K::Variable => CompletionKind::Variable,
        K::Child => CompletionKind::Instance,
    }
}

fn script_class_name(c: Option<ScriptClass>) -> Option<&'static str> {
    c.map(|c| match c {
        ScriptClass::Script => "Script",
        ScriptClass::LocalScript => "LocalScript",
        ScriptClass::ModuleScript => "ModuleScript",
    })
}

/// 1-based line and column of a byte offset.
fn line_col(source: &str, offset: usize) -> (u32, u32) {
    let before = &source[..offset.min(source.len())];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) as u32 + 1;
    (line, col)
}

fn range_of(source: &str, start: usize, end: usize) -> Range {
    let (start_line, start_column) = line_col(source, start);
    let (end_line, end_column) = line_col(source, end);
    Range { start_line, start_column, end_line, end_column }
}

/// The byte range of 1-based `line`, without its line break.
fn line_bytes(source: &str, line: u32) -> (usize, usize) {
    let mut start = 0;
    for (i, l) in source.split_inclusive('\n').enumerate() {
        if i + 1 == line as usize {
            return (start, start + l.trim_end_matches(['\n', '\r']).len());
        }
        start += l.len();
    }
    (source.len(), source.len())
}

impl LanguageService for LuauService {
    fn id(&self) -> LanguageId {
        LanguageId::Luau
    }

    fn initial_state(&self) -> LineState {
        lang::lexer::initial_state()
    }

    fn tokenize_line(&self, line: &str, state: LineState) -> (Vec<Token>, LineState) {
        let (tokens, end) = lang::lexer::tokenize_line(line, state);
        let tokens = tokens.into_iter().map(|t| Token { start: t.start, len: t.len, class: token_class(t.class) }).collect();
        (tokens, end)
    }

    fn line_comment(&self) -> Option<&'static str> {
        Some("--")
    }

    fn auto_close_pairs(&self) -> &'static [(char, char)] {
        &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\''), ('`', '`')]
    }

    fn indent_after(&self, line: &str) -> bool {
        lang::lexer::indent_after(line)
    }

    fn dedent_line(&self, line: &str) -> bool {
        lang::lexer::dedent_line(line)
    }

    fn analyze(&self, source: &str) -> Analysis {
        let diag = |start: usize, end: usize, message: String| Diagnostic {
            range: range_of(source, start, end),
            byte_range: (start as u32, end as u32),
            severity: Severity::Error,
            message,
            source: "luau",
        };
        let mut diagnostics: Vec<Diagnostic> =
            luau_parse::syntax_errors(source).into_iter().map(|e| diag(e.start, e.end, e.message)).collect();
        // What the parser accepts, the VM's compiler still checks.
        if diagnostics.is_empty() {
            if let Some(err) = lang::check(source) {
                let (start, end) = line_bytes(source, err.line);
                diagnostics.push(diag(start, end, err.message));
            }
        }
        let symbols = help::functions(source)
            .into_iter()
            .map(|(name, start, end)| Symbol {
                name,
                kind: SymbolKind::Function,
                range: range_of(source, start, end),
                byte_range: (start as u32, end as u32),
            })
            .collect();
        Analysis { diagnostics, symbols, service_data: None }
    }

    fn complete(&self, cx: &CompletionContext) -> Vec<Completion> {
        let script_class = script_class_name(cx.script_class);
        let before = &cx.source[..cx.offset.min(cx.source.len())];
        let locals = lc::locals(before, script_class);
        // The receiver as the file has it, calls and indexes included
        // (`game:GetService("Players").LocalPlayer`); the shell's reading
        // stops at a call. The access character sits just before the prefix.
        let from_source = cx.access.and_then(|_| {
            let access_at = cx.offset.checked_sub(cx.prefix.len() + 1)?;
            help::expression_before(cx.source, access_at)
        });
        let context = lc::Context {
            prefix: cx.prefix,
            receiver: from_source.as_deref().or(cx.receiver),
            access: cx.access.map(|a| match a {
                Access::Dot => lc::Access::Dot,
                Access::Colon => lc::Access::Colon,
            }),
            in_string: cx.in_string.as_ref().map(|s| (s.callee.as_str(), s.arg_index)),
            script_class,
            locals: &locals,
        };
        lc::complete(&context)
            .into_iter()
            .map(|i| Completion {
                insert_text: i.label.clone(),
                label: i.label,
                kind: completion_kind(i.kind),
                detail: i.detail,
                documentation: (!i.doc.is_empty()).then(|| i.doc.to_string()),
            })
            .collect()
    }

    fn signature_help(&self, source: &str, offset: usize) -> Option<SignatureHelp> {
        let s = help::signature_at(source, offset, None)?;
        Some(SignatureHelp {
            label: s.label,
            parameters: s.params,
            active_parameter: s.active,
            documentation: (!s.doc.is_empty()).then(|| s.doc.to_string()),
        })
    }

    fn hover(&self, source: &str, offset: usize, _analysis: &Analysis) -> Option<Hover> {
        let (markdown, byte_range) = help::hover_at(source, offset, None)?;
        Some(Hover { markdown, byte_range })
    }

    fn definition(&self, source: &str, offset: usize, _analysis: &Analysis) -> Option<Location> {
        let (start, end) = help::definition_at(source, offset)?;
        Some(Location { path: None, range: range_of(source, start, end), byte_range: (start as u32, end as u32) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx<'a>(source: &'a str, receiver: Option<&'a str>, access: Option<Access>) -> CompletionContext<'a> {
        CompletionContext {
            source,
            offset: source.len(),
            prefix: "",
            receiver,
            access,
            in_string: None,
            script_class: Some(ScriptClass::LocalScript),
            instance_tree: None,
        }
    }

    #[test]
    fn a_local_players_methods_complete_after_a_colon() {
        let src = "local p = game:GetService(\"Players\").LocalPlayer\np:";
        let items = LuauService.complete(&cx(src, Some("game:GetService(\"Players\").LocalPlayer"), Some(Access::Colon)));
        let labels: Vec<&str> = items.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"GetMouse") && labels.contains(&"Kick"), "{labels:?}");
        assert!(!labels.contains(&"FireServer"), "{labels:?}");
        assert!(items.iter().all(|c| c.kind == CompletionKind::Method));
    }

    #[test]
    fn the_receiver_comes_from_the_source_calls_and_all() {
        // The shell's receiver stops at the call: "LocalPlayer" alone.
        let src = "game:GetService(\"Players\").LocalPlayer:";
        let items = LuauService.complete(&cx(src, Some("LocalPlayer"), Some(Access::Colon)));
        assert!(items.iter().any(|c| c.label == "GetMouse"), "{items:?}");
    }

    #[test]
    fn callbacks_take_no_parentheses() {
        let src = "local f = Instance.new(\"RemoteFunction\")\nf.";
        let items = LuauService.complete(&cx(src, Some("f"), Some(Access::Dot)));
        let on = items.iter().find(|c| c.label == "OnServerInvoke").expect("a callback");
        assert_eq!(on.kind, CompletionKind::Property);
    }

    #[test]
    fn luau_files_are_never_offered_rune_words() {
        let items = LuauService.complete(&cx("", None, None));
        for rune in ["fn", "let", "impl", "struct", "pub"] {
            assert!(!items.iter().any(|c| c.label == rune), "{rune} offered in Luau");
        }
        assert!(items.iter().any(|c| c.label == "local"));
    }

    #[test]
    fn a_broken_snippet_reports_its_error_and_a_clean_one_none() {
        let a = LuauService.analyze("local x = 1\nif x then\n  print(x)\n");
        assert!(!a.diagnostics.is_empty(), "a missing `end` is an error");
        assert!(a.diagnostics.iter().all(|d| d.severity == Severity::Error && d.source == "luau"));
        // `continue` outside a loop: whether the parser or the VM's compiler
        // catches it, the editor reports it.
        assert!(!LuauService.analyze("continue\n").diagnostics.is_empty());
        let a = LuauService.analyze("local function add(a: number, b: number): number\n  return a + b\nend\n");
        assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
        assert_eq!(a.symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["add"]);
    }

    #[test]
    fn tokens_map_one_to_one() {
        let (tokens, end) = LuauService.tokenize_line("local s = `hi {name}`", LuauService.initial_state());
        assert_eq!(end, LuauService.initial_state());
        assert_eq!(tokens[0].class, TokenClass::Keyword);
        assert!(tokens.iter().any(|t| t.class == TokenClass::Interpolation));
    }
}
