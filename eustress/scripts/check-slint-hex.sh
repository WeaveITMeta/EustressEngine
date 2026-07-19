#!/usr/bin/env bash
# ────────────────────────────────────────────────────────────────────────────
# Theme lint: fail if any hardcoded hex color remains in a Slint UI file
# outside theme.slint (the one legitimate palette home).
#
# Every color in the UI must route through a `Theme.<token>` design token so
# that swapping the theme (a TOML palette) recolors the WHOLE app. A genuinely
# non-theme literal (e.g. an icon-on-accent white) may stay if the line is
# marked with a trailing `// theme-exempt: <reason>` — the lint honors that.
#
# Usage:  eustress/scripts/check-slint-hex.sh
# Exit:   0 = clean, 1 = offenders found (printed).
# ────────────────────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SLINT_DIR="$SCRIPT_DIR/../crates/engine/ui/slint"

if ! command -v rg >/dev/null 2>&1; then
  echo "ripgrep (rg) is required for this lint." >&2
  exit 2
fi

# #rgb / #rgba / #rrggbb / #rrggbbaa color literals, excluding theme.slint.
# Then drop: lines marked // theme-exempt, and pure-comment lines (content
# after the file:line: prefix starts with //).
hits="$(rg -n --glob '*.slint' -g '!theme.slint' '#[0-9a-fA-F]{3,8}\b' "$SLINT_DIR" \
  | grep -v 'theme-exempt' \
  | grep -vE ':[[:space:]]*//' \
  || true)"

if [ -n "$hits" ]; then
  count="$(printf '%s\n' "$hits" | wc -l | tr -d ' ')"
  echo "FAIL: $count hardcoded hex color(s) outside theme.slint."
  echo "Route each through a Theme.<token>, or mark the line // theme-exempt: <reason>."
  echo "----------------------------------------------------------------------"
  printf '%s\n' "$hits"
  exit 1
fi

echo "OK: no hardcoded hex colors outside theme.slint."
