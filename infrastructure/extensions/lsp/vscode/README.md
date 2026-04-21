# Eustress Rune LSP

IDE support for the [Rune](https://rune-rs.github.io/) scripting language
as used in [Eustress Engine](https://eustress.dev).

The Rune language server (`eustress-lsp`) is **bundled inside Eustress
Engine** — no separate download. Install the engine, launch it on a
Universe, and this extension connects over TCP automatically.

## Features

- **Inline diagnostics** — compile errors and warnings straight from the
  Rune compiler, not a regex approximation.
- **Eustress API awareness** — `use eustress::{…}` resolves; the full
  ECS / Instance / TweenService / task surface is known to the analyzer.
- **Go to Definition** (F12) + **Find All References** (Shift+F12).
- **Hover** — diagnostic messages and symbol kinds.
- **Completion** — keywords + in-scope symbols.
- **Rename** (F2) — scope-aware; leaves string literals and comments
  alone.
- **Code Actions** (Ctrl+.) — quick fixes for common diagnostics.
- **Outline** — symbol tree in the breadcrumb bar and Outline view.
- **Syntax highlighting** — TextMate grammar works before the LSP
  connects; semantic tokens layer on top when available.

## How it works

When Eustress Engine starts, it spawns `eustress-lsp --tcp` as a child
process and writes the listening port to
`<universe>/.eustress/lsp.port`. This extension watches for that file
and attaches via TCP — one live LSP shared across every editor open on
that Universe. Zero config.

```
┌─────────────────────┐            ┌──────────────────────┐
│  Eustress Engine    │  spawns    │  eustress-lsp --tcp  │
│                     ├───────────▶│  --port-file <…>     │
└─────────────────────┘            └──────────┬───────────┘
                                              │ listens
                                              ▼
                              <universe>/.eustress/lsp.port
                                              ▲
                                              │ read
┌─────────────────────┐            ┌──────────┴───────────┐
│  VS Code / Windsurf │  TCP       │   this extension     │
│  / Cursor           │◀──────────▶│   (multi-Universe)   │
└─────────────────────┘            └──────────────────────┘
```

### Multiple Universes at once

One Windsurf window can edit files from several Universes simultaneously.
Each distinct Universe (identified by its `.eustress/lsp.port`) gets its
own language client; `documentSelector` scopes each client to that
Universe's folder tree. Cross-Universe routing is automatic — no editor
restart when you switch.

## Requirements

**Install Eustress Engine.** That's it. Download from
<https://eustress.dev/download>. The bundled LSP binary ships inside the
installer.

This extension does **not** download a separate LSP — that's a legacy
concept from the pre-bundle era. If Eustress Engine isn't installed, the
language server simply isn't available; the status-bar item tells you
so clearly and links to the download page.

## Transport fallback (advanced)

The extension picks its transport in this order:

1. **TCP** — if `<universe>/.eustress/lsp.port` exists (written by
   Eustress Engine on startup), connect to `127.0.0.1:<port>`. This is
   the normal path.
2. **stdio** — no port file found, and `eustress.serverPath` or the
   `EUSTRESS_LSP_PATH` env var points at a standalone `eustress-lsp`
   binary on disk. Useful for CI linting or headless boxes where you
   don't want the full engine running.

When both fail, the status bar shows `Rune LSP: engine not running` and
a single info toast explains how to install Eustress Engine. No
"Download Server" button — there is no separate server to download.

## Settings

- `eustress.serverPath` *(string)* — **advanced**. Absolute path to a
  standalone `eustress-lsp`. Leave empty for normal use (TCP-to-engine).
  Set this only for CI / headless / no-engine setups.
- `eustress.trace.server` *(`off` | `messages` | `verbose`)* — logs
  LSP JSON-RPC traffic to the "Eustress (Rune)" output channel. `off`
  by default; flip to `messages` while debugging a stuck connection.

## Commands

- **Eustress: Restart Rune Language Server** — bounces every active
  client; the TCP reconnect happens on the next document open.
- **Eustress: Show resolved server path** — prints which stdio fallback
  the extension would use and lists the live TCP clients currently
  attached.
- **Eustress: Set up Rune Language Server…** — opens the install /
  "how it works" / configure-path prompt.
- **Eustress: Show Rune Language Server output** — reveals the first
  live client's output channel. With multiple Universes open, each has
  its own labelled channel — use the Output panel dropdown to pick one.

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
