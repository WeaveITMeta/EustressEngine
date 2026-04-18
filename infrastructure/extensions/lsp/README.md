# Eustress LSP — Developer README

This directory is the home of the Eustress language-server extension
(VS Code / Windsurf / Cursor) that ships to users at
`eustress.dev/learn#ide-integration`.

For the design rationale (why stdio, why a separate binary, why a VS Code
shim) read [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## What lives where

```
infrastructure/extensions/lsp/
├── ARCHITECTURE.md         design doc (start here)
├── README.md               this file — developer quick-start
├── vscode/                 the extension source
│   ├── package.json
│   ├── tsconfig.json
│   ├── src/extension.ts
│   ├── syntaxes/rune.tmLanguage.json
│   ├── language-configuration.json
│   ├── icons/rune.svg
│   └── README.md           (shown on the VS Code marketplace listing)
├── scripts/
│   └── build-vsix.sh       packages the extension into a .vsix
├── test_fixtures/          manual-verification .rune scripts (see its README)
│   ├── clean.rune
│   ├── broken.rune
│   └── README.md
└── dist/                   (generated — committed artefacts live in CI)
```

The server binary itself is **not** in this directory. It lives in
`eustress/crates/engine/src/script_editor/lsp.rs` and is compiled by:

```bash
cd eustress
cargo build --bin eustress-lsp --features lsp --release
```

Output: `eustress/target/release/eustress-lsp(.exe)`.

## Local dev loop

```bash
# 1. Build the server once, or run cargo-watch.
cd eustress
cargo build --bin eustress-lsp --features lsp

# 2. Hack on the extension.
cd ../infrastructure/extensions/lsp/vscode
npm ci          # see "Why npm for install?" below
bun run watch   # recompiles TypeScript on save (bun resolves tsc from node_modules/.bin)

# 3. Launch an Extension Development Host.
# In VS Code, open this directory (vscode/) and press F5. A second
# VS Code window opens with the extension loaded; open any .rune file.
```

The extension's server-path resolution (see
[`src/extension.ts`](./vscode/src/extension.ts)) picks up
`<workspace>/target/release/eustress-lsp` automatically, so you don't
need to edit settings while developing against the engine repo.

### Why npm for install?

bun 1.3.11 on Windows doesn't populate some of `vsce`'s legacy transitive
deps (notably `glob@7`) — directories are created but left empty, which
breaks `vsce package` at runtime with `Cannot find module 'glob'`. The
same `package.json` installs cleanly with npm in ~17 s.

We use **npm for install only**. `bun run compile` / `bun run package`
still drive the actual work; they just need the tree on disk to be
correct, which npm guarantees. When bun patches this on Windows, swap
the one `npm ci` line in `scripts/build-vsix.sh` back to `bun install
--frozen-lockfile` — nothing else changes.

On Linux / macOS, `bun install` works fine for this extension too. The
script uses npm unconditionally so the build is reproducible across CI
hosts; change it if you're sure your Windows pipeline stays bun-safe.

## Building a .vsix

```bash
cd infrastructure/extensions/lsp/scripts
./build-vsix.sh
# → ../dist/rune-lsp-0.1.0.vsix
```

With bundled server binaries:

```bash
mkdir -p /tmp/eustress-bins
cp eustress/target/release/eustress-lsp         /tmp/eustress-bins/eustress-lsp-linux
cp eustress/target/release/eustress-lsp.exe     /tmp/eustress-bins/eustress-lsp-windows.exe
# ...etc for macOS, then:
./build-vsix.sh --bundle /tmp/eustress-bins
```

Bundled VSIXes run out-of-the-box; unbundled VSIXes rely on the user
having `eustress-lsp` on PATH (smaller download, manual setup).

## Before first publish

VS Code Marketplace requires a **128×128 PNG** for the extension icon
(the one shown on the marketplace card and in the Extensions sidebar).
We ship `vscode/icons/rune.svg` for the in-editor file icon, which VS
Code accepts as SVG; the marketplace icon is a separate asset.

Steps:

1. Render `vscode/icons/rune.svg` to `vscode/icons/rune.png` at
   128×128 (any tool works: Inkscape `--export-type=png --export-width=128`,
   ImageMagick `convert`, Figma export).
2. Add `"icon": "icons/rune.png"` to `vscode/package.json` between
   `"keywords"` and `"repository"`.
3. Run `scripts/build-vsix.sh`.

We deliberately don't commit the PNG — leaving it to publish-time keeps
the repo SVG-only and re-exportable from the source of truth.

## Publishing

- **VS Code Marketplace** — `bunx vsce publish` from the `vscode/`
  directory once you have a publisher token for `eustress`.
- **Open VSX** (Windsurf, Cursor, VSCodium) — `bunx ovsx publish` with
  the same VSIX.
- **eustress.dev** — upload the `.vsix` to `eustress.dev/downloads/extensions/`
  and link it from the `/learn#ide-integration` card.

## Versioning invariants

- `vscode/package.json` `version` field **must** match the
  `eustress-engine` crate version in `eustress/crates/engine/Cargo.toml`
  at tagging time. The server reports its version on
  `initialize.server_info`; the extension logs a warning if it
  disagrees.
- Bump both in lockstep. CI enforces this with a check that
  greps both files.

## Testing

The extension itself has no unit tests (it's a 100-line shim). The
*real* test surface is the Rust analyzer, which has 15 unit tests in
`eustress/crates/engine/src/script_editor/analyzer.rs` — those exercise
the same code paths LSP requests end up hitting.

For end-to-end verification, install the built `.vsix` into a fresh VS
Code profile and open the two files in
[`test_fixtures/`](./test_fixtures/) — each one's top-of-file comment
lists the interactions to try (F12, F2, Shift+F12, Ctrl+., etc.).
Manual until `@vscode/test-electron` is wired; at that point the
fixtures get promoted into automated assertions without rewriting.
