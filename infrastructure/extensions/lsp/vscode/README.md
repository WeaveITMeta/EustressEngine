# Eustress Rune LSP

IDE support for the [Rune](https://rune-rs.github.io/) scripting
language as used in [Eustress Engine](https://eustress.dev).

## Features

- **Inline diagnostics** — compile errors and warnings straight from the
  Rune compiler, not a regex approximation.
- **Go to Definition** (F12) + **Find All References** (Shift+F12).
- **Hover** — diagnostic messages and symbol kinds.
- **Completion** — keywords + in-scope symbols.
- **Rename** (F2) — scope-aware; leaves string literals and comments
  alone.
- **Code Actions** (Ctrl+.) — quick fixes for common diagnostics like
  "missing semicolon".
- **Outline** — symbol tree in the breadcrumb bar and Outline view.
- **Syntax highlighting** — TextMate grammar works before the LSP
  connects; semantic tokens layer on top when available.

## Requirements

You need the `eustress-lsp` binary on disk. The extension looks for it
in this order:

1. `eustress.serverPath` setting (absolute path).
2. `EUSTRESS_LSP_PATH` environment variable.
3. `<workspace>/target/release/eustress-lsp(.exe)` — for engine devs.
4. A bundled server inside the extension (only when the `.vsix` was
   packaged with `build-vsix.sh --bundle`).
5. `PATH`.

If none of the above resolve, the extension opens a download prompt
pointing at <https://eustress.dev/learn#ide-integration>.

### Getting the binary

**Prebuilt (recommended):**

```bash
# Download from https://eustress.dev/downloads/lsp/
# Place anywhere on your PATH, then:
eustress-lsp --version
```

**From source:**

```bash
git clone https://github.com/eustressengine/eustress
cd eustress/eustress
cargo build --bin eustress-lsp --features lsp --release
# Binary at target/release/eustress-lsp
```

## Settings

- `eustress.serverPath` *(string)* — absolute path override.
- `eustress.trace.server` *(`off` | `messages` | `verbose`)* — logs LSP
  JSON-RPC traffic to the "Eustress (Rune)" output channel. `off` by
  default; flip to `messages` while debugging a stuck connection.

## Commands

- **Eustress: Restart Rune Language Server** — bounces the server after
  editing settings or replacing the binary.
- **Eustress: Show resolved server path** — prints which server the
  extension picked, for diagnostics.

## Compatibility

Works in:

- Visual Studio Code 1.85 and newer
- Windsurf (Codeium)
- Cursor
- Any VS Code fork that ships `vscode-languageclient` 9.x

For Neovim, Helix, Zed, Emacs, and Kakoune, you don't need this
extension — point your native LSP client at `eustress-lsp` directly.
See `ARCHITECTURE.md` in the repo for per-editor config snippets.

## License

MIT
