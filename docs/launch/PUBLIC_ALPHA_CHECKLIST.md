# Public Alpha — Launch Checklist

Working checklist for taking Eustress from "runs on the founder's machine" to
"strangers can download it, use it, and tell us what they want." Items are
ordered by blocking-ness within each section. Keep this document honest: check
things off only when verified end-to-end, not when the code merges.

Legend: **[BLOCKER]** must be done before the link goes out · **[SHOULD]**
embarrassing if missing · **[NICE]** genuinely optional for an alpha.

---

## 1 · Distribution

- [ ] **[BLOCKER] `downloads.eustress.dev` R2 bucket provisioned** and the
  updater manifest uploaded. The updater + installer + CI pipeline are already
  fixed and compiled; this is the last infrastructure step between a build and
  a downloadable link.
- [ ] **[BLOCKER] One clean install test on a machine that has never seen the
  repo** (fresh Windows VM): installer runs, engine launches, no missing-DLL
  or Smart-App-Control surprises, uninstall leaves nothing weird.
- [ ] **[BLOCKER] Fix `eustress-lsp.exe` child-process leak on parent exit** —
  an alpha tester's task manager filling with orphaned processes is the kind
  of thing that ends up in a screenshot on X.
- [ ] **[SHOULD] Code-sign the installer + exe.** Unsigned binaries mean
  SmartScreen scare dialogs for every tester; expect drop-off if skipped.
- [ ] **[SHOULD] Crash reporting decision.** Sentry is already linked into the
  build — either wire it to a real DSN behind the same telemetry toggle, or
  strip it. An alpha that crashes silently teaches nothing.
- [ ] **[NICE] Auto-update smoke test:** ship 0.1.0 to a VM, publish 0.1.1,
  confirm the updater takes it.

## 2 · First-run experience

- [ ] **[BLOCKER] Cold-start test on a blank profile** (no `%LOCALAPPDATA%/
  Eustress`, no settings.json): default Universe/Space opens, no panic, ribbon
  and Modes dropdown render, a discipline can be picked, a Part can be placed,
  Ctrl+Z works. This is the ten-minute session every tester actually has.
- [ ] **[BLOCKER] The telemetry first-run notice appears once** and the
  Settings ▸ Notifications ▸ Privacy toggle genuinely stops recording
  (verified by watching the JSONL not grow).
- [ ] **[SHOULD] A 5-minute "what do I do first" surface:** one starter Space
  that shows off a discipline (the park-build palette exists; point at it) +
  a Help-menu link to a getting-started doc. Testers who don't know what to
  click produce no telemetry.
- [ ] **[SHOULD] Dream-button toast copy review** — it currently reads
  "on the roadmap / your click was counted as a vote." Confirm that's the
  message you want thousands of first impressions built on.
- [ ] **[NICE] First-run mode suggestion** (e.g., open the Modes dropdown once
  on first launch so the discipline surface — the whole point of the alpha —
  gets discovered).

## 3 · Telemetry & feedback loop (the reason this alpha exists)

- [x] Click capture at the MenuAction choke point, wired/dream aware.
- [x] Honest dream-button toast (no silent no-ops anywhere on the surface).
- [x] Local JSONL + install-id rotation + on-by-default with visible toggle.
- [x] Aggregate-only uploader with offline-safe outbox.
- [x] `eustress-api` Worker ingest + comments + admin summary (deployed,
  smoke-tested in production).
- [x] `/admin/telemetry` page — live counts merged into the
  mode ▸ discipline ▸ tab ▸ section ▸ button hierarchy, with a recent-requests
  feed, a search/activity-filtered tree, comments rendered under the button
  they were written about, and a Copy button on every demand row that yields a
  paste-ready implementation brief.
- [ ] **[BLOCKER] Purge synthetic telemetry before the link goes out.** Every
  counter currently in the TELEMETRY namespace came from this session's smoke
  tests (`smoke-test-0001`, `e2e-verify-a1b2c3`). Real demand is only legible
  from a zero baseline — otherwise the wiring queue ranks a test click.
- [x] End-to-end verification with the real engine build: outbox session
  payload → startup drain → production Worker → KV counters incremented
  (verified 2026-07-24 with the shipping binary against api.eustress.dev).
  Remaining human check: one real ribbon click session confirming the toast
  and the counts on /admin/telemetry — a two-minute pass during normal use.
- [ ] **[SHOULD] In-engine feedback dialog live check** — Help ▸ Send
  Feedback is built into the current binary and the Worker comment endpoint
  is smoke-tested; send one real comment and confirm it appears in the
  admin Comments panel.
- [ ] **[SHOULD] Weekly ritual:** look at the wiring queue, pick the top 1-2
  dream tools by installs, wire them, mention it in the changelog. The alpha's
  promise is "clicks become features" — keep it visibly true.
- [ ] **[NICE] Uninstall survey link** in the uninstaller.

## 4 · Legal & policy

- [ ] **[BLOCKER] Privacy note published on eustress.dev** covering the usage
  telemetry (what's collected, that it's anonymous, how to opt out, install-id
  rotation). The client is honest in-app; the website must match.
- [ ] **[BLOCKER] License surface check:** installer + site say
  **source-available (PolyForm Shield)** — never "open source" — and
  commercial contact is licensing@eustress.dev.
- [ ] **[SHOULD] Terms for the alpha** (no warranty, data may be wiped,
  feedback may be used). One page, plain language.
- [ ] **[SHOULD] Positioning pass on all public copy:** Eustress is an
  AI-native orchestrator / substrate — the phrase "game engine" should not
  appear anywhere a tester will read.

## 5 · Stability floor

- [ ] **[BLOCKER] The 10 core interactions are unbreakable** (the
  fix-foundation-first list): select, move, scale, rotate, undo/redo, save,
  open, insert, delete, camera. A discipline tourist will still do all ten.
- [ ] **[BLOCKER] Save/load round-trip torture:** create → save → quit →
  reopen → identical scene, including imported splats (the disk-persistence
  fix is in; verify it on a fresh install).
- [ ] **[SHOULD] A 30-minute soak session** (mixed clicking across 5+ modes)
  with the log watched for panics/warn-storms; memory stable.
- [ ] **[SHOULD] Known-issues list published** — testers forgive known bugs
  and report duplicates otherwise.
- [ ] **[NICE] Perf statement:** alpha targets edit-mode comfort, not the
  131K-instance benchmark; say so to pre-empt "it lags on my huge scene."

## 6 · Website & account path

- [ ] **[BLOCKER] Download page** with version, SHA-256, system requirements,
  and the privacy note linked beside the button.
- [ ] **[SHOULD] Sign-up remains optional for the alpha** — the engine works
  logged-out, and telemetry stays account-free either way (that separation is
  a stated privacy property; don't quietly break it).
- [ ] **[SHOULD] /admin + /admin/telemetry checked on mobile** — you'll be
  looking at the wiring queue from your phone.
- [ ] **[NICE] A "what is this" 90-second read** on the landing page aimed at
  the discipline surface (the screenshot IS the pitch).

## 7 · Launch-day mechanics

- [ ] **[SHOULD] Announcement thread drafted** (X via @Simbuilder; the
  discipline-switch GIF is the hook; reply-to-repliers is the lever).
- [ ] **[SHOULD] Feedback triage inbox decided** (GitHub issues vs. a single
  email) and linked from the site + Help menu.
- [ ] **[SHOULD] Rollback plan:** previous installer stays downloadable;
  updater manifest can be reverted in one R2 edit.
- [ ] **[NICE] A day-one telemetry sanity check scheduled** — confirm sessions
  are arriving before assuming silence means no users.

---

*Maintained by hand. When an item completes, check it here in the same commit
as the change that completed it.*
