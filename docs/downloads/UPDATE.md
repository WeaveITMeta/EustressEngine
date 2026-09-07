# Eustress Engine — releases and self-update

**Status**: current. One implementation, no alternatives.
**Date**: August 28, 2026

Eustress Engine ships from a git tag. GitHub Actions builds three platforms,
publishes them to Cloudflare R2, and the engine updates itself from the
manifest that build produced.

There is exactly one destination and it is spelled the same everywhere:

| | value |
|---|---|
| R2 bucket | `eustress-downloads` |
| Worker | `eustress-downloads` (`infrastructure/cloudflare`) |
| Host | `downloads.eustress.dev` |

> An earlier design used `eustress-releases` and `releases.eustress.dev`. That
> bucket was never created and that subdomain was never in DNS, so every
> release run uploaded into nothing. Both names are gone. If you find one in a
> comment or a doc, it is stale; delete it rather than recreating the split.

---

## 1. Cutting a release

```
bump eustress/crates/engine/Cargo.toml   ->   fast-forward Core   ->   tag
```

```bash
git tag v0.3.7
git push origin v0.3.7
```

Three gates run before anything is built, and each fails in seconds rather than
after an hour of compilation:

- **The tag is on Core.** `RELEASE.md`'s promotion model assumes releases only
  ship from Core, so a tag pushed anywhere else is refused.
- **The crate version matches the tag.** The updater compares the manifest
  against the compiled-in `CARGO_PKG_VERSION`. If those disagree, a shipped
  v0.3.7 binary reports whatever `Cargo.toml` says, `is_newer()` never
  converges, and every client re-downloads the same build on every launch.
- **The manifest is complete.** A missing artifact or an empty checksum fails
  the publish. A manifest that parses but points at nothing is worse than no
  manifest, because the updater trusts it.

`workflow_dispatch` takes a version input for a rebuild without a new tag.

---

## 2. What gets built

| platform | artifact |
|---|---|
| Windows x64 | `eustress-engine-vX.Y.Z-windows-x64.zip` |
| Windows x64 | `EustressEngine-Setup.exe` (Inno Setup) |
| macOS ARM64 | `eustress-engine-vX.Y.Z-macos-arm64.dmg` |
| Linux x64 | `eustress-engine-vX.Y.Z-linux-x64.tar.gz` |

Each is smoke-tested for an immediate startup crash before packaging. That is a
floor, not a rendering guarantee: the hosted Windows and Linux runners have no
GPU, so a graceful exit for want of an adapter is not distinguished from
success.

Windows binaries are Authenticode-signed when `WINDOWS_CERT_PFX` is set, and
the workflow warns loudly when it is not rather than shipping an unsigned
binary quietly. macOS notarisation is not wired up yet, so the `.dmg` needs a
right-click override on first open.

Linux builds on `ubuntu-22.04` for the oldest supported glibc, with LTO off and
two build jobs. v0.3.6 died on that runner with exit 143 after a shutdown
signal, which is eviction under memory pressure rather than a compile error;
the job now trades build time for headroom and reclaims ~25 GB of preinstalled
toolchains it never uses.

---

## 3. Where it goes

```
eustress-downloads/
  latest.json                                    max-age=60
  v0.3.7/
    eustress-engine-v0.3.7-windows-x64.zip       immutable
    EustressEngine-Setup.exe
    eustress-engine-v0.3.7-macos-arm64.dmg
    eustress-engine-v0.3.7-linux-x64.tar.gz
    checksums.txt
```

Uploads use `wrangler r2 object put --remote`, not the S3 API, so this repo has
one credential shape for every Cloudflare interaction. `--remote` is not
optional: without it wrangler writes to local simulated storage and reports
success.

Required secrets: `CLOUDFLARE_API_TOKEN`, scoped to Object Read & Write on
`eustress-downloads`, and `CLOUDFLARE_ACCOUNT_ID`.

**Publishing ends by fetching what it just published.** Uploading is not the
same as being reachable, and that difference is exactly what hid a missing
bucket through seven release attempts.

---

## 4. Serving

The `eustress-downloads` Worker fronts the bucket:

| route | auth | purpose |
|---|---|---|
| `/latest.json` | none | the manifest, polled by the updater and the website |
| `/vX.Y.Z/<file>` | none | artifacts, fetched by the updater |
| `/api/releases/download?platform=` | JWT | the website's sign-in-to-download flow |
| `/api/releases/stats` | none | download counts |

Artifacts are public because **the updater cannot hold a credential**. The
gated endpoint remains for the website, where it buys attribution and per-user
analytics rather than secrecy: the build is free through either path.

The artifact route matches `vX.Y.Z/filename` only. That pattern is the whole
boundary, so no other key in the bucket is reachable through it.

---

## 5. How a client updates

1. On startup the engine fetches `https://downloads.eustress.dev/latest.json`.
2. It compares `version` against its compiled-in `CARGO_PKG_VERSION`.
3. If newer, an update button appears in the status bar.
4. On click it downloads the platform artifact, verifies the SHA-256, and
   refuses to apply anything that does not match.
5. It replaces the running executable and restarts.

The updater enforces a host allowlist and follows no redirects, so a manifest
pointing somewhere else is rejected rather than followed.

---

## 6. Verifying by hand

```bash
curl -s https://downloads.eustress.dev/latest.json | jq .
npx wrangler r2 object get eustress-downloads/latest.json --remote --pipe
curl -s https://downloads.eustress.dev/v0.3.7/checksums.txt
```
