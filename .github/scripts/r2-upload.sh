#!/usr/bin/env bash
# Upload a release directory to the eustress-downloads R2 bucket.
#
# Run 39053358971 failed here: wrangler 4 returned HTTP 403, authentication
# error 10000. The token was present and could identify the account, but it
# did not have R2 write, so player/latest.json was never stored and the
# Worker answers 404. This script says that, passes the account id wrangler
# needs, and uses S3 credentials when those secrets are set instead.
#
# Required env: CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID, BUCKET, VERSION
# Optional: PREFIX (no trailing slash; the Player uses "player")
# Optional: R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY — S3 credentials for the
#           bucket. When both are set, aws s3 is used and the API token is
#           not asked to write objects.
#
# Usage: r2-upload.sh <directory>
# The directory holds the artifacts and latest.json. latest.json is uploaded
# last, as the object the site polls.

# A matching auth failure prints a GitHub error and returns 0.
# Anything else returns 1, and the caller prints wrangler's own output.
explain_r2_auth_failure() {
  local out="$1"
  if printf '%s\n' "$out" | grep -Eq '10000|Authentication error'; then
    echo "::error::R2 refused the upload (authentication error 10000). CLOUDFLARE_API_TOKEN is set and can call the Cloudflare API, but it does not have R2 write on bucket ${BUCKET:-eustress-downloads}. Grant that token Workers R2 Storage:Edit for the bucket, or set R2_ACCESS_KEY_ID and R2_SECRET_ACCESS_KEY (S3 credentials). Nothing was published, so latest.json was not updated and downloads.eustress.dev will 404 it."
    return 0
  fi
  return 1
}

if [ "${BASH_SOURCE[0]}" != "$0" ]; then
  return 0 2>/dev/null || exit 0
fi

set -euo pipefail

dir="${1:-.}"
if [ ! -d "$dir" ]; then
  echo "::error::no release directory: $dir"
  exit 1
fi

if [ -z "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
  echo "::error::CLOUDFLARE_ACCOUNT_ID is not set, so the upload cannot name the account that owns ${BUCKET:-the bucket}."
  exit 1
fi
if [ -z "${BUCKET:-}" ] || [ -z "${VERSION:-}" ]; then
  echo "::error::BUCKET and VERSION are required."
  exit 1
fi

prefix=""
if [ -n "${PREFIX:-}" ]; then
  prefix="${PREFIX}/"
fi

use_s3=0
if [ -n "${R2_ACCESS_KEY_ID:-}" ] && [ -n "${R2_SECRET_ACCESS_KEY:-}" ]; then
  use_s3=1
  if ! command -v aws >/dev/null 2>&1; then
    echo "::error::R2_ACCESS_KEY_ID is set, but the aws CLI is not installed, so the S3 fallback cannot run."
    exit 1
  fi
elif [ -z "${CLOUDFLARE_API_TOKEN:-}" ]; then
  echo "::error::CLOUDFLARE_API_TOKEN is not set, so downloads.eustress.dev cannot serve this release."
  exit 1
fi

put() {
  local key="$1" file="$2"
  shift 2
  echo "uploading ${key}"
  if [ "$use_s3" = 1 ]; then
    AWS_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY" \
      aws s3 cp "$file" "s3://${BUCKET}/${key}" \
      --endpoint-url "https://${CLOUDFLARE_ACCOUNT_ID}.r2.cloudflarestorage.com" \
      --only-show-errors "$@"
    return
  fi
  local out
  if ! out=$(npx --yes wrangler@4 r2 object put "${BUCKET}/${key}" \
      --file "$file" --remote --account-id "$CLOUDFLARE_ACCOUNT_ID" "$@" 2>&1); then
    printf '%s\n' "$out"
    explain_r2_auth_failure "$out" || true
    exit 1
  fi
  printf '%s\n' "$out"
}

cd "$dir"
for f in *; do
  [ "$f" = "latest.json" ] && continue
  [ -f "$f" ] || continue
  put "${prefix}v${VERSION}/${f}" "$f"
done

if [ ! -f latest.json ]; then
  echo "::error::latest.json is missing, so the stable download path would 404."
  exit 1
fi
if [ "$use_s3" = 1 ]; then
  put "${prefix}latest.json" latest.json \
    --content-type application/json \
    --cache-control "public, max-age=60"
else
  put "${prefix}latest.json" latest.json \
    --content-type application/json \
    --cache-control "public, max-age=60"
fi
