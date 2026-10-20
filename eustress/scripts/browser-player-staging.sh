#!/usr/bin/env bash
# Build the browser Player and the website for STAGING only.
#
#   eustress/scripts/browser-player-staging.sh              # build, do not deploy
#   eustress/scripts/browser-player-staging.sh --webgl2     # also build WebGL2
#   eustress/scripts/browser-player-staging.sh --deploy     # build, then Pages branch staging
#
# Refuses a production deploy. The Pages project is `eustress` and the staging
# branch is `staging` (https://staging.eustress-cbb.pages.dev). Branch `main`
# of that project is production. This script never passes `main`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EUSTRESS="$ROOT/eustress"
WEB="$EUSTRESS/crates/web"
PAGES_PROJECT="eustress"
PAGES_BRANCH="staging"
MAX_BYTES=$((25 * 1024 * 1024))

DEPLOY=0
WEBGL2=0
for arg in "$@"; do
  case "$arg" in
    --deploy) DEPLOY=1 ;;
    --webgl2) WEBGL2=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

if [[ "$PAGES_BRANCH" == "main" || "$PAGES_PROJECT" != "eustress" ]]; then
  echo "refusing: this script only deploys project eustress, branch staging" >&2
  exit 1
fi

export PATH="${HOME}/.local/bin:${PATH}"
if ! command -v wasm-bindgen >/dev/null; then
  echo "wasm-bindgen 0.2.128 is not on PATH (see docs/launch/BROWSER_PLAYER_STAGING.md)" >&2
  exit 1
fi
BINDGEN="$(wasm-bindgen --version | awk '{print $2}')"
LOCK_BINDGEN="$(awk '
  $0 == "name = \"wasm-bindgen\"" { found=1 }
  found && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
' "$EUSTRESS/Cargo.lock")"
if [[ "$BINDGEN" != "$LOCK_BINDGEN" ]]; then
  echo "wasm-bindgen CLI ${BINDGEN} does not match Cargo.lock (${LOCK_BINDGEN})" >&2
  exit 1
fi

cd "$EUSTRESS"
echo "building WebGPU browser Player"
cargo build --release --target wasm32-unknown-unknown -p eustress-player-web --no-default-features --features webgpu
WEBGPU_WASM="target/wasm32-unknown-unknown/release/eustress_player.wasm"
if [[ "$WEBGL2" == 1 ]]; then
  echo "building WebGL2 browser Player"
  cargo build --release --target wasm32-unknown-unknown -p eustress-player-web --no-default-features --features webgl2
  # The two feature sets share one output name. Keep the WebGL2 wasm aside
  # before the next build would overwrite it; WebGPU was copied below only
  # after both exist, so rebuild WebGPU once WebGL2 has been saved.
  cp "$WEBGPU_WASM" target/wasm32-unknown-unknown/release/eustress_player_webgl2.wasm
  cargo build --release --target wasm32-unknown-unknown -p eustress-player-web --no-default-features --features webgpu
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
PKG=(node crates/player-web/package_player.mjs --workspace "$EUSTRESS" --out "$STAGE/player" --webgpu "$WEBGPU_WASM")
if [[ "$WEBGL2" == 1 ]]; then
  PKG+=(--webgl2 target/wasm32-unknown-unknown/release/eustress_player_webgl2.wasm)
fi
"${PKG[@]}"

# Pages rejects a file over 25 MiB. A wasm over the cap is stored as 20 MiB
# parts; /player/host.js concatenates eustress_player_bg.wasm.part0, part1, ...
# wasm-opt is not run: the glue indexes the module wasm-bindgen just wrote.
PART_BYTES=$((20 * 1024 * 1024))
fit_pages() {
  local file="$1"
  local size
  size="$(wc -c < "$file")"
  local label
  label="$(basename "$(dirname "$file")")/$(basename "$file")"
  if (( size <= MAX_BYTES )); then
    echo "${label}: $((size / 1048576)) MiB"
    return
  fi
  echo "${label}: $((size / 1048576)) MiB, over the 25 MiB Pages cap; splitting"
  python3 - "$file" "$PART_BYTES" <<'PY'
import pathlib, sys
path, part = pathlib.Path(sys.argv[1]), int(sys.argv[2])
data = path.read_bytes()
count = 0
for offset in range(0, len(data), part):
    path.with_name(path.name + f".part{count}").write_bytes(data[offset:offset + part])
    count += 1
path.unlink()
print(f"  {count} parts")
PY
}
while IFS= read -r -d '' file; do
  fit_pages "$file"
done < <(find "$STAGE/player" -name '*.wasm' -print0)
too_big=0
while IFS= read -r -d '' file; do
  size="$(wc -c < "$file")"
  if (( size > MAX_BYTES )); then
    echo "over the 25 MiB Pages file cap: $file" >&2
    too_big=1
  fi
done < <(find "$STAGE/player" -type f -print0)
if (( too_big )); then
  echo "a file is still over 25 MiB. Serve /player from R2; host.js loads the builds beside itself." >&2
  exit 1
fi

cd "$WEB"
if ! command -v trunk >/dev/null; then
  echo "trunk is not on PATH (cargo install trunk)" >&2
  exit 1
fi
trunk build --release
cargo run --no-default-features --features ssr --bin prerender -- --strict
node scripts/csp-inline-hashes.mjs dist
rm -rf dist/player
cp -a "$STAGE/player" dist/player
# The local test page pastes a join link. It is not part of the site.
rm -f dist/player/test.html

echo "staged at $WEB/dist/player"
if [[ "$DEPLOY" != 1 ]]; then
  echo "not deployed. Staging deploy, and no other, is:"
  echo "  (cd $WEB && npx wrangler pages deploy dist --project-name $PAGES_PROJECT --branch $PAGES_BRANCH --commit-dirty=true)"
  exit 0
fi

echo "deploying to Pages project ${PAGES_PROJECT} branch ${PAGES_BRANCH} only"
npx wrangler pages deploy dist --project-name "$PAGES_PROJECT" --branch "$PAGES_BRANCH" --commit-dirty=true
