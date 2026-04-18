# Eustress LSP Extension — Architecture

## One-line summary

Eustress ships a standalone `eustress-lsp` stdio binary (built from the
engine crate, feature-gated). A thin VS Code extension spawns that binary
as its language-server and wires it to any file matching `*.rune`. The
same extension works unmodified in VS Code, Windsurf, Cursor, and any
other VS Code fork.

## Why stdio (and not TCP-to-studio)?

The studio already embeds the analyzer (`ScriptAnalysisPlugin` in
`eustress/crates/engine/src/script_editor/plugin.rs`). It would be
tempting to have the extension connect to a TCP port the studio opens,
and get "live" intelligence from the running engine.

We **don't** do that as the default, because:

1. **External editing is a first-class workflow, not a fallback.** Users
   edit `.rune` files without Eustress Studio open all the time (CI, git
   bisect, diff review). A TCP-to-studio model would silently break the
   moment studio is closed.
2. **File-system-first means analysis is stateless.** The analyzer reads
   source text and produces diagnostics. It doesn't need live ECS state
   to be correct. Keeping the LSP binary independent keeps the contract
   clean.
3. **One binary, one code path.** The same crate compiled with
   `--features lsp --bin eustress-lsp` produces a server every editor
   can use. The main engine binary still links zero LSP code.

A **future** Studio-embedded TCP mode (`eustress-engine --lsp-port 24785`)
can be added without changing the extension — it'll be a transport option
the user flips via the `eustress.transport` setting. Until someone asks
for live ECS context in their external editor, stdio stays the default.

## Runtime topology

```
┌──────────────────────────────────────────────────────────────┐
│  User's VS Code / Windsurf / Cursor / Zed / Neovim / Helix   │
│                                                              │
│   ┌─────────────────────────────┐                            │
│   │ eustress-vscode extension   │ ◄── only VS Code family    │
│   │  (TypeScript, ~100 LOC)     │     needs this shim        │
│   └──────────────┬──────────────┘                            │
│                  │ spawn(stdio)                              │
│                  ▼                                           │
│   ┌─────────────────────────────┐                            │
│   │   eustress-lsp (Rust bin)   │ ◄── same binary for all    │
│   │   tower-lsp + analyzer      │     editors                │
│   └──────────────┬──────────────┘                            │
│                  │ reads / writes .rune files                │
│                  ▼                                           │
│      project/src/*.rune                                      │
└──────────────────────────────────────────────────────────────┘
```

Neovim / Helix / Zed / Emacs / Kakoune / Sublime LSP all have native
LSP clients — they need **zero** extension code, just a config pointing
at the binary. VS Code and its forks (Windsurf, Cursor) require the
extension shim because VS Code's LSP client lives in a package
(`vscode-languageclient`) that individual extensions have to bundle.

## Distribution

| Artifact                         | Source                                          | Hosted at                                 |
|----------------------------------|-------------------------------------------------|-------------------------------------------|
| `eustress-lsp` binaries (win/mac/linux) | `cargo build --bin eustress-lsp --features lsp --release` | `eustress.dev/downloads/lsp/`             |
| `eustress-rune-lsp-<version>.vsix`  | `infrastructure/extensions/lsp/scripts/build-vsix.sh` | `eustress.dev/downloads/extensions/` |
| Marketplace listing (VS Code)    | same `.vsix` uploaded to marketplace.visualstudio.com | `marketplace.visualstudio.com/items?itemName=eustress.rune-lsp` |
| Open VSX listing (Windsurf, Cursor, etc.) | same `.vsix` uploaded to open-vsx.org         | `open-vsx.org/extension/eustress/rune-lsp` |
| Landing page card                | `eustress.dev/learn` site repo                  | `eustress.dev/learn#ide-integration`      |

The `/learn` page card points users at three options:
1. **Install from marketplace** (recommended — one click).
2. **Download `.vsix`** (for offline / air-gapped users).
3. **Install via CLI** — `code --install-extension eustress.rune-lsp`.

## Server binary resolution (extension side)

On activation the extension looks for `eustress-lsp` in this order:

1. `eustress.serverPath` setting (absolute path, user override).
2. `EUSTRESS_LSP_PATH` environment variable.
3. `${workspaceFolder}/target/release/eustress-lsp(.exe)` — local dev.
4. Bundled binary at `<extension>/server/eustress-lsp(.exe)` if the
   VSIX included pre-built binaries (optional; adds ~6 MB per platform).
5. System `PATH`.

If none of these resolve, the extension shows a "Download server" prompt
with a link to `eustress.dev/downloads/lsp/` and exits cleanly. No
silent failures.

## Grammar vs semantic highlighting

The extension ships a **TextMate grammar** (`rune.tmLanguage.json`) that
provides basic tokenisation for Rune: keywords, strings, comments,
numbers, operators, function definitions. This gives usable highlighting
before the LSP connects and remains the fallback if the binary fails to
start.

Richer, AST-accurate highlighting arrives through
`textDocument/semanticTokens` from the LSP server, layered on top of the
TextMate grammar — same model VS Code uses for `rust-analyzer` and
`tsserver`. The `EustressLsp` server doesn't implement semantic tokens
yet (Phase 5+ polish); the TextMate grammar stands alone until it does.

## Update + versioning

Extension version and server version are tracked together:

- `vscode/package.json` → `version: "0.1.0"`.
- `eustress-lsp` binary prints `env!("CARGO_PKG_VERSION")` on the
  `server_info` field of `initialize`.
- Extension logs a warning if the server reports a different major
  version than the extension was built against — avoids protocol drift
  when users pin old binaries.

## Why "similar to Luau LSP"

Luau LSP (`JohnnyMorganz/luau-lsp`) is our north star because it proved
the model works at scale:

- Single Rust binary.
- Two-piece distribution: binary + VS Code extension.
- Hosts on GitHub Releases for the binary, VS Code Marketplace for the
  extension.
- Minimal extension glue, maximum server-side intelligence.

Eustress LSP adopts the same shape with two differences:
1. Our binary lives in the engine repo rather than a separate crate,
   because the analyzer must stay in lock-step with the engine's Rune
   integration. Feature-gating keeps the main engine build clean.
2. We expose a future Studio-embedded TCP mode as a flag. Luau LSP
   has no equivalent because Roblox Studio doesn't share its analyzer.

## Paths in this repo

```
infrastructure/extensions/lsp/
├── ARCHITECTURE.md           (this file)
├── README.md                 developer quick-start
├── vscode/                   VS Code / Windsurf / Cursor extension
│   ├── package.json
│   ├── tsconfig.json
│   ├── .vscodeignore
│   ├── src/extension.ts      activate + LSP client + server resolution
│   ├── syntaxes/rune.tmLanguage.json
│   ├── language-configuration.json
│   ├── README.md             (shown on marketplace listing)
│   └── icons/rune.svg
└── scripts/
    └── build-vsix.sh         packages the extension
```
