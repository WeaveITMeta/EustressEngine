/**
 * Eustress Monaco Editor Bridge
 * 
 * Table of Contents:
 * - State: Editor state tracking (tabId, dirty, language)
 * - setupEditor(): Initialize Monaco with Eustress dark theme
 * - handleRustMessage(): Process commands from Rust via IPC
 * - sendToRust(): Send messages to Rust via Wry IPC
 * - registerSoulLanguage(): Custom Soul script language definition
 * - Keybindings: Ctrl+S save, Ctrl+Shift+B build
 */

// ============================================================================
// State
// ============================================================================

/** Current editor state */
const state = {
    tabId: 0,
    dirty: false,
    language: 'plaintext',
    editor: null,
    ready: false,
};

// ============================================================================
// Editor Setup
// ============================================================================

/** Initialize Monaco editor with Eustress dark theme */
function setupEditor() {
    const container = document.getElementById('editor-container');
    const loading = document.getElementById('loading');

    // Register custom Soul language before creating editor
    registerSoulLanguage();

    // Define Eustress dark theme (Catppuccin Mocha-inspired)
    monaco.editor.defineTheme('eustress-dark', {
        base: 'vs-dark',
        inherit: true,
        rules: [
            { token: 'comment', foreground: '6c7086', fontStyle: 'italic' },
            { token: 'keyword', foreground: 'cba6f7' },
            { token: 'string', foreground: 'a6e3a1' },
            { token: 'number', foreground: 'fab387' },
            { token: 'type', foreground: '89b4fa' },
            { token: 'function', foreground: '89dceb' },
            { token: 'variable', foreground: 'cdd6f4' },
            { token: 'operator', foreground: '89dceb' },
            { token: 'delimiter', foreground: '9399b2' },
            { token: 'annotation', foreground: 'f9e2af' },
            { token: 'constant', foreground: 'fab387' },
            { token: 'tag', foreground: 'f38ba8' },
            { token: 'attribute', foreground: 'f9e2af' },
        ],
        colors: {
            'editor.background': '#1e1e2e',
            'editor.foreground': '#cdd6f4',
            'editor.lineHighlightBackground': '#313244',
            'editor.selectionBackground': '#45475a',
            'editor.inactiveSelectionBackground': '#313244',
            'editorCursor.foreground': '#f5e0dc',
            'editorWhitespace.foreground': '#45475a',
            'editorIndentGuide.background': '#313244',
            'editorIndentGuide.activeBackground': '#45475a',
            'editorLineNumber.foreground': '#6c7086',
            'editorLineNumber.activeForeground': '#cdd6f4',
            'editorGutter.background': '#1e1e2e',
            'editor.findMatchBackground': '#f9e2af33',
            'editor.findMatchHighlightBackground': '#f9e2af22',
            'editorBracketMatch.background': '#45475a',
            'editorBracketMatch.border': '#89b4fa',
            'scrollbarSlider.background': '#45475a80',
            'scrollbarSlider.hoverBackground': '#585b70',
            'scrollbarSlider.activeBackground': '#6c7086',
            'minimap.background': '#181825',
        }
    });

    // Create editor instance
    state.editor = monaco.editor.create(container, {
        value: '',
        language: 'plaintext',
        theme: 'eustress-dark',
        automaticLayout: true,
        fontSize: 13,
        fontFamily: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', Consolas, monospace",
        fontLigatures: true,
        lineNumbers: 'on',
        minimap: { enabled: true, maxColumn: 80 },
        scrollBeyondLastLine: false,
        wordWrap: 'off',
        tabSize: 4,
        insertSpaces: true,
        renderWhitespace: 'selection',
        bracketPairColorization: { enabled: true },
        guides: { bracketPairs: true, indentation: true },
        smoothScrolling: true,
        cursorBlinking: 'smooth',
        cursorSmoothCaretAnimation: 'on',
        padding: { top: 8 },
        suggest: {
            showKeywords: true,
            showSnippets: true,
            showFunctions: true,
            showVariables: true,
        },
    });

    // Hide loading indicator
    loading.classList.add('hidden');

    // ====================================================================
    // Event Listeners
    // ====================================================================

    // Content change → dirty state
    state.editor.onDidChangeModelContent(() => {
        if (!state.dirty) {
            state.dirty = true;
            sendToRust({ type: 'dirty', tab_id: state.tabId, dirty: true });
        }
        // Debounced content sync
        clearTimeout(state._contentTimer);
        state._contentTimer = setTimeout(() => {
            sendToRust({
                type: 'content_changed',
                tab_id: state.tabId,
                content: state.editor.getValue(),
            });
        }, 500);
    });

    // Cursor position change
    state.editor.onDidChangeCursorPosition((e) => {
        sendToRust({
            type: 'cursor',
            tab_id: state.tabId,
            line: e.position.lineNumber,
            column: e.position.column,
        });
    });

    // ====================================================================
    // Keybindings
    // ====================================================================

    // Ctrl+S → Save
    state.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        sendToRust({
            type: 'save',
            tab_id: state.tabId,
            content: state.editor.getValue(),
        });
        state.dirty = false;
        sendToRust({ type: 'dirty', tab_id: state.tabId, dirty: false });
    });

    // Ctrl+Shift+B → Build
    state.editor.addCommand(
        monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyB,
        () => {
            sendToRust({
                type: 'build',
                tab_id: state.tabId,
                content: state.editor.getValue(),
            });
        }
    );

    // Mark ready
    state.ready = true;
    sendToRust({ type: 'ready', tab_id: state.tabId });
}

// ============================================================================
// IPC: Rust → Monaco
// ============================================================================

/** Handle commands from Rust via Wry IPC injection */
window.handleRustMessage = function(msg) {
    if (!state.editor) return;

    switch (msg.type) {
        case 'setContent':
            state.tabId = msg.tab_id || 0;
            state.language = msg.language || 'plaintext';
            state.dirty = false;

            // Set language model
            const model = state.editor.getModel();
            if (model) {
                monaco.editor.setModelLanguage(model, state.language);
                model.setValue(msg.content || '');
            }
            break;

        case 'markSaved':
            state.dirty = false;
            break;

        case 'setReadOnly':
            state.editor.updateOptions({ readOnly: msg.read_only });
            break;

        case 'setTheme':
            monaco.editor.setTheme(msg.theme || 'eustress-dark');
            break;

        case 'focus':
            state.editor.focus();
            break;
    }
};

// ============================================================================
// IPC: Monaco → Rust
// ============================================================================

/** Send message to Rust via Wry IPC postMessage */
function sendToRust(msg) {
    try {
        // Wry IPC: window.ipc.postMessage(string)
        if (window.ipc && window.ipc.postMessage) {
            window.ipc.postMessage(JSON.stringify(msg));
        }
    } catch (e) {
        console.warn('IPC send failed:', e);
    }
}

// ============================================================================
// Soul Language Definition
// ============================================================================

/** Register custom Soul script language for Monaco */
function registerSoulLanguage() {
    if (typeof monaco === 'undefined') return;

    // Register language ID
    monaco.languages.register({ id: 'soul' });

    // Tokenizer (Monarch syntax highlighting)
    monaco.languages.setMonarchTokensProvider('soul', {
        keywords: [
            'fn', 'let', 'mut', 'const', 'if', 'else', 'while', 'for', 'in',
            'return', 'break', 'continue', 'match', 'struct', 'enum', 'impl',
            'trait', 'pub', 'use', 'mod', 'self', 'super', 'true', 'false',
            'nil', 'spawn', 'destroy', 'connect', 'disconnect', 'wait',
            'yield', 'async', 'await', 'event', 'signal', 'service',
        ],
        typeKeywords: [
            'i32', 'i64', 'f32', 'f64', 'bool', 'string', 'Vec3', 'Color3',
            'CFrame', 'Instance', 'Part', 'Model', 'Script', 'void',
        ],
        operators: [
            '=', '>', '<', '!', '~', '?', ':', '==', '<=', '>=', '!=',
            '&&', '||', '++', '--', '+', '-', '*', '/', '&', '|', '^',
            '%', '<<', '>>', '+=', '-=', '*=', '/=', '&=', '|=', '^=',
            '=>', '->', '..', '..=',
        ],
        symbols: /[=><!~?:&|+\-*\/\^%]+/,

        tokenizer: {
            root: [
                // Comments
                [/\/\/.*$/, 'comment'],
                [/\/\*/, 'comment', '@comment'],

                // Strings
                [/"([^"\\]|\\.)*$/, 'string.invalid'],
                [/"/, 'string', '@string'],

                // Numbers
                [/\d*\.\d+([eE][\-+]?\d+)?/, 'number.float'],
                [/0[xX][0-9a-fA-F]+/, 'number.hex'],
                [/0[bB][01]+/, 'number.binary'],
                [/\d+/, 'number'],

                // Identifiers and keywords
                [/[a-zA-Z_]\w*/, {
                    cases: {
                        '@keywords': 'keyword',
                        '@typeKeywords': 'type',
                        '@default': 'identifier',
                    }
                }],

                // Operators
                [/@symbols/, {
                    cases: {
                        '@operators': 'operator',
                        '@default': '',
                    }
                }],

                // Delimiters
                [/[{}()\[\]]/, 'delimiter'],
                [/[;,.]/, 'delimiter'],

                // Annotations
                [/#\[.*?\]/, 'annotation'],
            ],

            comment: [
                [/[^\/*]+/, 'comment'],
                [/\*\//, 'comment', '@pop'],
                [/[\/*]/, 'comment'],
            ],

            string: [
                [/[^\\"]+/, 'string'],
                [/\\./, 'string.escape'],
                [/"/, 'string', '@pop'],
            ],
        },
    });

    // Auto-completion for Soul keywords
    monaco.languages.registerCompletionItemProvider('soul', {
        provideCompletionItems: function(model, position) {
            const word = model.getWordUntilPosition(position);
            const range = {
                startLineNumber: position.lineNumber,
                endLineNumber: position.lineNumber,
                startColumn: word.startColumn,
                endColumn: word.endColumn,
            };

            const suggestions = [
                { label: 'fn', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'fn ${1:name}(${2:params}) {\n\t$0\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'let', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'let ${1:name} = ${2:value};', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'if', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'if ${1:condition} {\n\t$0\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'for', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'for ${1:item} in ${2:iter} {\n\t$0\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'while', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'while ${1:condition} {\n\t$0\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'match', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'match ${1:value} {\n\t${2:pattern} => $0,\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'struct', kind: monaco.languages.CompletionItemKind.Keyword, insertText: 'struct ${1:Name} {\n\t$0\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'spawn', kind: monaco.languages.CompletionItemKind.Function, insertText: 'spawn(${1:fn_name});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'connect', kind: monaco.languages.CompletionItemKind.Function, insertText: 'connect(${1:event}, ${2:handler});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
                { label: 'wait', kind: monaco.languages.CompletionItemKind.Function, insertText: 'wait(${1:seconds});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, range },
            ];

            return { suggestions };
        }
    });
}
