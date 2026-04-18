#!/usr/bin/env bash
# Build the Eustress Rune LSP VS Code extension into a distributable .vsix.
#
# Usage:
#   ./build-vsix.sh                 # extension only (users supply the binary)
#   ./build-vsix.sh --bundle <dir>  # also bundles platform binaries under ./server/
#
# When `--bundle` is given, `<dir>` should contain pre-built binaries named
# eustress-lsp-<platform>(.exe). The script copies the one matching the host
# into the VSIX's `server/` folder. For cross-platform multi-bundle VSIX
# publishing, run this script once per target with the matching binary.

set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
EXT_DIR="${SCRIPT_DIR}/../vscode"
DIST_DIR="${SCRIPT_DIR}/../dist"

BUNDLE_SRC=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --bundle)
            BUNDLE_SRC="$2"
            shift 2
            ;;
        *)
            echo "Unknown arg: $1" >&2
            exit 2
            ;;
    esac
done

mkdir -p "${DIST_DIR}"
cd "${EXT_DIR}"

# ─── Install step: npm, not bun ──────────────────────────────────────────────
# bun 1.3.11 on Windows silently fails to populate some of vsce's transitive
# deps (empty `node_modules/glob/` and `node_modules/typescript/` directories
# despite a successful-looking install), which breaks `vsce package`. Same
# `package.json` installs cleanly with npm. We use npm here *only* for
# install — `bun run` still drives the compile + package steps below, so
# scripts stay fast and the package.json remains unchanged.
#
# When bun patches this on Windows we can swap `npm ci` → `bun install
# --frozen-lockfile` and delete this comment. Track: https://github.com/oven-sh/bun/issues
echo "→ Installing dependencies (npm — see note above)..."
npm ci --no-audit --no-fund

echo "→ Compiling TypeScript (bun run)..."
bun run compile

if [[ -n "${BUNDLE_SRC}" ]]; then
    echo "→ Bundling server binaries from ${BUNDLE_SRC}..."
    mkdir -p server
    # Copy whatever binaries exist — vsce will package them all.
    for bin in "${BUNDLE_SRC}"/eustress-lsp*; do
        if [[ -f "$bin" ]]; then
            cp "$bin" "server/$(basename "$bin")"
        fi
    done
else
    # Make sure we're not shipping stale binaries from a previous bundled build.
    rm -rf server
fi

echo "→ Packaging (bun run, calls node_modules/.bin/vsce)..."
bun run package

echo "✓ VSIX written to ${DIST_DIR}/"
ls -lh "${DIST_DIR}"/*.vsix
