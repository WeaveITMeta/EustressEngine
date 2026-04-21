# Eustress Engine — Release Guide

How to ship a new downloadable build of the Eustress Engine.

---

## Branch model

```
main  ────●───●───●───●───●──  (active, may break)
                   │
Core  ─────────────●────────── (fast-forward only to verified commits)
                   │
                   └─ tag v0.x.y → CI builds → downloads.eustress.dev
```

- **`main`** — active development. May contain incomplete features, experimental refactors, and broken builds between commits.
- **`Core`** — stable snapshot branch. Only ever fast-forwards to a `main` commit that has been manually verified. Zero divergence, zero cherry-picks, zero merge conflicts.

The release pipeline is triggered by a git tag on **Core only**. `main` tags are not used for user-facing releases.

---

## Version scheme

Semantic versioning, prefixed `v`:

| Segment | When to bump | Example |
|---|---|---|
| Patch (`v0.3.x`) | Bug fixes, small UX tweaks, no new surface | `v0.3.6` → `v0.3.7` |
| Minor (`v0.x.0`) | New features, new panels, new APIs | `v0.3.6` → `v0.4.0` |
| Major (`v1.0.0`) | Breaking changes, data migrations, API removals | `v0.9.0` → `v1.0.0` |

Latest released tag lives in [git tags](https://github.com/WeaveITMeta/EustressEngine/tags). Check before incrementing.

---

## Pre-release checklist

Run through this on `main` before promoting any commit to `Core`.

- [ ] `cargo studio` builds cleanly (no warnings treated as errors)
- [ ] `cargo core` builds cleanly with the stable tier
- [ ] Engine launches — no panic on startup
- [ ] Foundation interactions work:
  - [ ] Open a Space, see entities in Explorer
  - [ ] Select an entity, Properties panel populates
  - [ ] Move/rotate/scale gizmos respond
  - [ ] Drag-and-drop reparenting in Explorer
  - [ ] Ctrl+S saves to disk
  - [ ] Ctrl+Z undoes the last action
  - [ ] Right-click context menu opens
  - [ ] Script editor opens on double-click of a SoulScript
  - [ ] Build / Summarize buttons don't panic
  - [ ] Tab close (X) works
- [ ] No regressions in the Slint UI renderer (no black viewport, no frozen input)
- [ ] Recent `git log` reviewed — nothing committed that references local paths or temp files

If any check fails, fix on `main` and try again. **Never** promote a commit with a known regression.

---

## Release procedure

### 1. Pick a verified commit

On `main`, identify the SHA you've validated against the checklist above:

```bash
git log --oneline -10
# → copy the commit SHA you just verified
```

### 2. Fast-forward Core

```bash
git checkout Core
git pull --ff-only origin Core
git merge --ff-only <sha>
git push origin Core
```

If the `merge --ff-only` fails, it means `Core` has diverged from `main`'s history. Investigate — do **not** force-push or create a merge commit.

### 3. Tag and push

```bash
git tag -a v0.3.7 -m "Eustress Engine v0.3.7 — <short summary>"
git push origin v0.3.7
```

The annotated tag (`-a`) embeds the release notes pointer. The push triggers [the release workflow](.github/workflows/release.yml).

### 4. Return to main for continued development

```bash
git checkout main
```

That's it on your end. CI handles the rest.

---

## What CI does automatically

Triggered by `v*` tag push on any branch (typically Core):

1. **Build three platforms in parallel:**
   - Windows x64 → `.zip` with executable + assets
   - macOS ARM64 (Apple Silicon) → `.dmg` with signed `.app` bundle
   - Linux x64 → `.tar.gz` with executable + `install.sh` + desktop file
2. **Compute SHA-256 checksums** for each artifact
3. **Generate `latest.json`** — download manifest consumed by the in-app updater and the website download page
4. **Upload to Cloudflare R2** at `s3://eustress-releases/${VERSION}/`, exposed publicly at:
   - `https://releases.eustress.dev/v0.3.7/eustress-engine-v0.3.7-windows-x64.zip`
   - `https://releases.eustress.dev/v0.3.7/eustress-engine-v0.3.7-macos-arm64.dmg`
   - `https://releases.eustress.dev/v0.3.7/eustress-engine-v0.3.7-linux-x64.tar.gz`
   - `https://releases.eustress.dev/latest.json` (manifest, 5-minute cache)
5. **Create a GitHub Release** with auto-generated notes from commits since the last tag, artifacts attached

### Typical wall-clock time

- Windows build: ~20 min
- macOS build: ~25 min
- Linux build: ~15 min (fastest, mold linker)
- Upload + publish: ~2 min

Total: ~30 min from tag push to download link going live.

---

## Build tiers

The `Core` branch currently ships with the `core` cargo feature tier enabled — which today includes **every feature**. As the engine matures, individual features may be stripped from the `core` tier in `crates/engine/Cargo.toml`:

| Tier | Command | Intended audience |
|---|---|---|
| `studio` (default) | `cargo studio` / `cargo run-studio` | Active development |
| `core` (release) | `cargo core` / `cargo run-core` | Downloadable stable build |

Feature gates live in [`eustress/crates/engine/Cargo.toml`](eustress/crates/engine/Cargo.toml) under `[features]`. The CI release workflow builds with default features today; if `core` diverges from default, update the build step to `cargo build --release --no-default-features --features core`.

---

## Hotfix procedure

For a critical bug in a released version:

1. Fix on `main` first (always).
2. Verify the fix against the pre-release checklist.
3. Fast-forward `Core` to the fix commit: `git merge --ff-only <fix-sha>`.
4. Tag a patch bump: `git tag -a v0.3.8 -m "..." && git push origin v0.3.8`.
5. CI ships it.

Never tag `Core` without the commit first existing on `main`. The invariant is: every commit in `Core`'s history also lives in `main`'s history.

---

## Rollback

To pull a broken release:

1. **Delete the tag** locally and on origin:
   ```bash
   git tag -d v0.3.7
   git push --delete origin v0.3.7
   ```
2. **Delete the GitHub Release** from the [releases page](https://github.com/WeaveITMeta/EustressEngine/releases). (The release artifacts are auto-created by CI but not auto-cleaned on tag delete.)
3. **Remove the R2 objects** via the Cloudflare dashboard, or:
   ```bash
   aws s3 rm s3://eustress-releases/v0.3.7/ --recursive --endpoint-url https://<ACCT>.r2.cloudflarestorage.com
   ```
4. **Revert `latest.json`** by re-tagging the prior known-good version. CI overwrites `latest.json` on every release, so re-tagging `v0.3.6-rollback` pointing at the old commit (or just re-running the previous tag's workflow) restores the old manifest.

`Core` itself does **not** need to be reset — it stays pointing at whatever commit matches the latest good release. Tags are just labels.

---

## First-time release setup

Required GitHub Actions secrets (set in repo settings → Secrets and variables → Actions):

| Secret | Purpose |
|---|---|
| `R2_ACCESS_KEY` | Cloudflare R2 access key ID |
| `R2_SECRET_KEY` | Cloudflare R2 secret access key |
| `CF_ACCOUNT_ID` | Cloudflare account ID (in R2 endpoint URL) |

DNS for `releases.eustress.dev` must CNAME to the R2 public bucket URL. Verify with:

```bash
curl -I https://releases.eustress.dev/latest.json
```

---

## Reference commands

```bash
# Promote a verified main commit to Core and release
git checkout Core
git merge --ff-only <sha>
git push origin Core
git tag -a v0.3.7 -m "Release notes summary"
git push origin v0.3.7
git checkout main

# Local release build (matches what CI produces).
# `lsp` is in the default `core` feature, so this produces BOTH bins:
#   target/release/eustress-engine.exe   (the Studio)
#   target/release/eustress-lsp.exe      (the Rune LSP server)
# Both ship inside the installer.
cd eustress
cargo build --release --package eustress-engine

# Or using the tier alias
cargo core

# Inspect the generated artifacts — you should see both bins.
ls -lh eustress/target/release/eustress-engine*
ls -lh eustress/target/release/eustress-lsp*
```
