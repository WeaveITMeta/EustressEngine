#!/usr/bin/env bash
# The classifier only. It does not talk to Cloudflare.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=r2-upload.sh
source "$here/r2-upload.sh"

BUCKET=eustress-downloads
msg=$(explain_r2_auth_failure "Authentication error [code: 10000]")
printf '%s\n' "$msg" | grep -q 'authentication error 10000'
printf '%s\n' "$msg" | grep -q 'latest.json'
printf '%s\n' "$msg" | grep -q 'R2_ACCESS_KEY_ID'

if explain_r2_auth_failure "network unreachable" >/dev/null; then
  echo "a transport failure was classified as an auth failure" >&2
  exit 1
fi

echo "ok"
