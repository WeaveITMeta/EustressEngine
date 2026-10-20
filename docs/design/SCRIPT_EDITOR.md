# Script Editor

The code window in Studio: one editor shell that hosts every scripting
language, with each language's knowledge behind one trait. The shell owns
text, files, focus, drawing and keys. A language service owns what the text
means. The shell never branches on language.

## Ownership

| Area | Owner | Files |
|---|---|---|
| Editor shell: drawing, caret, keys, popups | Rune session | `ui/slint/script_editor.slint`, `ui/slint/completion_popup.slint` |
| Save, dirty state, close prompt, focus | Rune session | `ui/slint_ui.rs` (script arms), `ui/center_tabs.rs` |
| Highlight pipeline and theme | Rune session | `ui/highlight.rs`, `script_editor/theme.rs` |
| `LanguageService` trait and registry | Rune session | `script_editor/language.rs` |
| Analysis scheduling | Rune session | `script_editor/plugin.rs` |
| Rune service | Rune session | `script_editor/rune_service.rs` (wraps `analyzer.rs`) |
| Luau service | mlua session | `script_editor/luau_service.rs` |
| `main.slint` wiring | Rune session, landed in the UI session's windows | `ui/slint/main.slint` |

## The trait

```rust
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
    fn indent_after(&self, line: &str) -> bool;
    fn dedent_line(&self, line: &str) -> bool;

    // Intelligence. Static only: a service never executes user code.
    fn analyze(&self, source: &str) -> Analysis;
    fn complete(&self, cx: &CompletionContext) -> Vec<Completion>;
    fn signature_help(&self, source: &str, offset: usize) -> Option<SignatureHelp>;
    fn hover(&self, source: &str, offset: usize, analysis: &Analysis) -> Option<Hover>;
    fn definition(&self, source: &str, offset: usize, analysis: &Analysis) -> Option<Location>;
}
```

`Token { start: u32, len: u32, class: TokenClass }`, offsets in bytes within
the line. `LineState` is a `u64` the service encodes however it likes (block
comment depth, long-string level, interpolation nesting); the shell only
stores and compares it.

`TokenClass` is semantic: `Keyword`, `ControlFlow`, `Type`, `Function`,
`Method`, `Property`, `Variable`, `Parameter`, `Constant`, `Builtin`,
`Number`, `String`, `StringEscape`, `Interpolation`, `Comment`, `DocComment`,
`Operator`, `Punctuation`, `Attribute`, `Invalid`, `Text`. Services never
choose colours.

`Analysis` is what `analyze` returns and what `hover` and `definition` read
back:

```rust
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
    /// The service's own index (a Luau type graph, a Rune unit's items).
    /// The shell stores it with the revision it came from and hands it back
    /// unchanged; only the service that made it downcasts it.
    pub service_data: Option<Arc<dyn Any + Send + Sync>>,
}
```

`complete` receives everything the shell knows about the caret:

```rust
pub struct CompletionContext<'a> {
    pub source: &'a str,
    pub offset: usize,
    /// The identifier characters immediately before the caret.
    pub prefix: &'a str,
    /// The expression before a `.` or `:` access, as written (`game.Workspace`).
    pub receiver: Option<&'a str>,
    pub access: Option<Access>,
    /// Set when the caret sits inside a string argument, for completions
    /// such as `GetService("` or `FindFirstChild("`.
    pub in_string: Option<StringArg>,
    /// Script, LocalScript or ModuleScript, from the Rojo suffix or the
    /// instance's class.
    pub script_class: Option<ScriptClass>,
    /// The Space's instance tree, for completing children by name. `None`
    /// until the tree view is wired.
    pub instance_tree: Option<Arc<dyn InstanceTreeView>>,
}

pub enum Access { Dot, Colon }
pub struct StringArg { pub callee: String, pub arg_index: u32 }
pub enum ScriptClass { Script, LocalScript, ModuleScript }
```

`LanguageRegistry` maps a source path to a service. It reads the real file
name, never the tab title, and understands Rojo suffixes:
`.server.luau`, `.client.luau`, `.module.luau`, `.lua` go to Luau; `.rune`
and `.soul` to Rune; `.md` to Markdown; everything else to a syntect
fallback service.

Rune and Luau use hand-written lexers rather than TextMate grammars. The
syntect build here loads only its bundled syntax dump (no `yaml-load`, no
`plist-load`), JSON grammars are unsupported in any syntect mode, and a lexer
yields semantic classes, per-line state and nested Luau string interpolation
directly. `infrastructure/extensions/lsp/vscode/syntaxes/rune.tmLanguage.json`
stays the reference list of Rune keywords, so the external extension and the
editor agree.

## Theme

`script_editor/theme.rs` maps each `TokenClass` to a colour derived from the
Slint `Theme` tokens, per palette, so the editor follows the Studio theme.
Selection is `accent-eustress` (`#00bcd4`). The syntect fallback keeps its own
theme until each of its languages has a service.

## Highlight pipeline

1. An edit records the new text and sets `CodeEditorState.revision`.
2. One system owns tokenization. It keeps per-line `(tokens, end_state)`,
   re-lexes from the first changed line and stops when an end state matches
   the cache, then pushes spans for the visible window only.
3. Spans are pushed to Slint only when the revision or the visible window
   changes. They are never cleared on a frame without an edit.
4. The analyzer reads the revision; it does not share a dirty flag with the
   highlighter.

## Save

- Save writes a tab's code to the file it was read from
  (`center_tabs::code_source_file`): the entity's `LoadedFromFile.path` for a
  tab opened from the Explorer (the loader records the exact source file,
  honouring an `_instance.toml` `[script] source`), and the file itself for a
  tab opened by path. For a script folder it is the source file inside it;
  the canonical `<folder>.rune` is used only to create one for a folder that
  has none.
- The write keeps the file's line endings, is flushed to disk, and puts the
  same bytes in the Space's Fjall `tree`, so a save is durable even in the
  watcher's start-up grace period.
- Save goes through the file watcher as an ordinary edit. It does not use the
  `RecentlyWrittenFiles` self-write mark: a marked path makes the watcher skip
  both the `SoulScriptData` refresh (so Play would run the old code) and the
  disk to Fjall `tree` mirror. Nothing writes script source back from the
  ECS, so there is no loop for the mark to prevent.
- Ctrl+S while the editor has focus, and the Save button, save the active
  tab's code, from either view of a script tab. A Summary saves itself as it
  is edited, and so does a Markdown document.
- Code edits mark the tab dirty through `CenterTabManager::mark_dirty`; a
  successful write calls `mark_clean`. Both patch the one tab's dot in place
  and never rebuild the strip, which would re-push the text mid-typing.
- Closing tabs saves each dirty one first, and keeps every tab open if any
  save fails. The Save, Don't Save, Cancel prompt replaces this in P0's
  second step.
- A Summary (`.md`) edit updates only the summary. The watcher's
  folder-script fallback applies to the script's source file alone.

## Focus

The editor reports focus to the engine keyboard gate, so WASD, Delete,
Ctrl+Z and the other scene shortcuts stay in the editor while it has focus.
The flag is ignored whenever the active tab is not a script, so a destroyed
editor can never leave the gate stuck.

## Analysis

Per language, off the main thread, debounced, static only. The Rune analyzer
compiles and indexes symbols; it does not run `on_init`, `on_update` or any
other entry point. Luau files and plain code files never go through the Rune
analyzer, and a tab in another language clears the previous file's Rune
diagnostics. The analyzer follows `StudioState::script_content_revision`,
which the highlighter bumps when it applies an edit, so the two never compete
for one dirty flag.

## Order of work

- **P0, data safety.** Save and dirty state, close prompt, editor focus, the
  Summary fallback.
- **P1, colour that stays.** Single tokenization owner, no per-frame clears,
  language from the source path, Rune and Luau lexers, the Eustress theme,
  visible-line tokenization, a live Ln and Col readout, measured tabs.
- **P2, language smarts.** Completion filtered by language, member completion
  after `.` and `:`, signature help, hover, go to definition, diagnostics per
  language.
- **P3, editor basics.** Find and replace (Ctrl+F, Ctrl+H), go to line
  (Ctrl+G), bracket matching and auto-close, auto-indent on Enter, Tab and
  Shift+Tab block indent, Ctrl+/ comment toggle, current-line highlight, an
  editor-scoped undo and redo. Later: folding, multiple cursors, minimap.

## Tests

- Save writes the recorded source path, never a new `<folder>.rune` beside a
  `.luau`; dirty and clean transitions; the close prompt's three outcomes.
- The registry maps every extension and Rojo suffix to the right service.
- Each lexer: keywords, numbers, strings with escapes, block comments across
  lines, Luau long strings and nested interpolation, end-state round trips.
- Completion per language: a `.luau` file is never offered Rune's `fn`,
  `let` or `impl`.
