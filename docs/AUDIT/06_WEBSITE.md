# 06 — Website

> Marketing, project discovery, identity / KYC, downloads, monetization,
> simulation launch deep-links, leaderboards. **Play button is a P0 launch blocker.**
> Leptos CSR (WASM) at `eustress.dev`, talking to `api.eustress.dev` Worker.

## Pass changelog

- **P1 (2026-05-14):** 10 feature rows; 50 R + 38 I + 12 wiring gaps.
- **P3 (2026-05-14):** **Play button = dead end** escalated **P0**. +9 missing features (blog, status, bug bounty, accessibility, i18n, cookie, GDPR, editor canvas, creator analytics). Editor / dashboard / Stripe state corrected.
- **P4 (2026-05-14):** **Full retrofit to per-feature-card format.** 19 cards. Addendum blocks removed.
- **Updated 2026-05-16: storage pivot.** C11 footer corrected — the publish flow uploads the `.eustress` world container (Fjall WorldDb + baked `.echk` chunks), not `.pak`; upload mechanism unchanged. See MASTER C17.

---

## Concept summary

The Website is the customer-acquisition + content-discovery front door. Logged-in users see projects, friends, leaderboards; logged-out users see marketing, premium, gallery, downloads. Identity is Ed25519-signed KYC today; **C14 (P4)** mandates an OAuth-first alternative with KYC deferred to first monetisation. Sessions are JWT minted by the Cloudflare Worker.

The site is *feature-complete for marketing* but *incomplete on monetisation, simulation launch, real-time telemetry, accessibility, i18n*. The **biggest single blocker** is the Play button: today it shows server status and a download fallback. Without a working `eustress://` deep-link (C13), the gallery is window-shopping only.

---

## Implementation snapshot

- **Crate:** [eustress-web](../../eustress/crates/web/) — Leptos 0.7 CSR / WASM bundled by Trunk
- **Routing:** 52 pages in `app.rs`; AppState context holds auth, dark mode, jurisdiction, errors
- **Backend:** [eustress-backend](../../eustress/crates/backend/) Axum / sqlx / SQLite; Cloudflare Worker fronts auth + KYC
- **State:** JWT in localStorage; restored on app startup
- **CSR-only** today (Trunk targets WASM); SSR feature in `Cargo.toml` comment but unused

---

## Top-of-doc feature index

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Marketing pages (home, about, premium, careers, press) | ✅ |
| 2 | Identity / KYC sign-up (Ed25519 + Grok verify) | ✅ |
| 3 | OAuth alternative (Discord / Google / GitHub) *(C14)* | 🔴 |
| 4 | Project / experience discovery | 🟡 |
| 5 | Simulation launch (Play button → server join) *(C13)* **— P0** | 🔴 P0 |
| 6 | Downloads page (`latest.json` consumer) | 🟡 |
| 7 | Leaderboards + community stats | 🟡 |
| 8 | Monetisation surfaces (Bliss / Premium / Marketplace) | 🟡 / 🔴 |
| 9 | Friends / parties UI | 🟡 |
| 10 | Telemetry & analytics ingest | 🟠 |
| 11 | Blog / news page *(P3 add)* | 🔴 |
| 12 | Status / incidents page *(P3 add)* | 🔴 |
| 13 | Public bug-bounty page *(P3 add)* | 🔴 |
| 14 | Security disclosure page *(P3 add)* | 🔴 |
| 15 | Accessibility features (WCAG AA) *(P3 add)* | 🔴 |
| 16 | i18n / multi-language *(P3 add)* | 🔴 |
| 17 | Cookie consent UI / banner *(P3 add)* | 🔴 |
| 18 | GDPR data-export self-serve *(P3 add)* | 🔴 |
| 19 | In-browser Editor (WASM canvas) *(P3 add)* | 🟡 |

---

## Per-feature cards

### Feature 1 — Marketing pages

**State:** ✅ · **Effort:** Done · **Risk:** Med · **Touches:** [06]
**Sub-features:** Home · About · Premium · Press · Careers · Learn · 15+ doc pages · Schema.org (SoftwareApplication, Organization, WebAPI) · OG / Twitter meta · sitemap.xml · robots.txt · dark mode toggle

**Concept.** 52 pages routed; comprehensive. Schema.org structured data is excellent (SoftwareApplication v0.16.1 + Organization + WebAPI EEP + MCP).

**Forecasted feedback (R)**
- R1.1 CSR-only loses 20–30% organic discovery vs. SSR/pre-render.
- R1.2 No pricing-comparison table.
- R1.3 Marketing copy needs unified voice.
- R1.4 Hero is static; animated demo would drive signups.
- R1.5 Dark mode partial; light-mode CSS variables missing.
- R1.6 Localisation absent (en-US only).

**Implications (I)**
- *Architectural:* CSR vs. SSR is the SEO question; decide before paid marketing.
- *Cross-system:* localisation requires i18n framework (Feature 16).
- *Strategic:* first impression for ~80% of traffic; underinvesting is irrational.

**Risks (X)** — X1.1 CSR fails SEO at scale.

**Mitigations (M)** — M1.1 Leptos SSR or static pre-render for top marketing pages.

---

### Feature 2 — Identity / KYC sign-up

**State:** ✅ functional / 🔴 friction · **Effort:** L (refactor for C14) · **Risk:** High (funnel) · **Touches:** [06], [08_IDENTITY], C14
**Sub-features:** birthday + government-ID type per jurisdiction · ID front/back upload (gloo_net multipart) · Grok-AI verification · Ed25519 keypair generated client-side · signed challenge → JWT · jurisdiction detection from `cf-ipcountry`

**Concept.** Three-step KYC flow. Worker accepts ID images, Grok AI extracts name + accepts/rejects. Ed25519 keypair lives client-side; never transmitted. JWT issued by Cloudflare Worker.

**Forecasted feedback (R)**
- R2.1 KYC-first signup likely halves funnel conversion vs. OAuth-first industry norm — **C14** addresses.
- R2.2 Grok-AI dependency = single point of failure; need fallback queue.
- R2.3 VPN users see wrong jurisdiction default.
- R2.4 Email backup mentioned; no SMTP wired.
- R2.5 Ed25519 private key stored client-side → UX horror story on clear localStorage.
- R2.6 COPPA / GDPR-K compliance needs audit.

**Implications (I)**
- *Funnel:* C14 OAuth-first signup with KYC deferred is the highest-leverage growth move.
- *Architectural:* lost-key recovery needs server-side state that the SPI model avoids — careful design.
- *Strategic:* signup conversion is the top growth metric.
- *Compliance:* age gating must run pre-monetisation.

**Risks (X)** — X2.1 5% annual key loss without recovery = 5% annual account loss.

**Mitigations (M)** — M2.1 C14: OAuth + KYC-deferred unlocks normal-funnel signup.

---

### Feature 3 — OAuth alternative  *(C14)*

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [06], [08_IDENTITY], C14
**Sub-features:** Discord OAuth · Google OAuth · GitHub OAuth · KYC deferred to first monetisation · account-linking flow · 2FA forced for OAuth-only accounts

**Concept.** Industry-standard. OAuth signup first; Ed25519 keypair generated post-OAuth + bound to the account; KYC required only at first monetisation event (creator publish, marketplace sale, Bliss withdrawal). C14 cross-cut.

**Forecasted feedback (R)**
- R3.1 Each provider needs app registration + secrets.
- R3.2 Account linking (OAuth user later does KYC) needs a merge flow.
- R3.3 Discord mentioned in code comments; no active impl.
- R3.4 Privacy: friends-list scope (rich invites) vs. minimum scope (just email).

**Implications (I)**
- *Funnel:* OAuth-first likely 2× signup conversion.
- *Cross-system:* account-linking complicates [08_IDENTITY] auth model.
- *Strategic:* OAuth + KYC-deferred is the standard pattern; deviating costs growth.

**Risks (X)** — X3.1 Reduced security for OAuth-only accounts (no Ed25519 challenge).

**Mitigations (M)** — M3.1 Force 2FA for OAuth-only; offer keypair upgrade anytime.

---

### Feature 4 — Project / experience discovery

**State:** 🟡 (substring search; no embedvec) · **Effort:** M · **Risk:** Low · **Touches:** [06], [20_SEARCH_DISCOVERY]
**Sub-features:** Gallery (paginated) · search (substring today) · featured / trending · marketplace listing · tags · semantic search (embedvec) planned · creator profile

**Concept.** Gallery + Marketplace pages. Substring search; no semantic. embedvec is production-grade but unwired into the gallery endpoint.

**Forecasted feedback (R)**
- R4.1 Substring near-zero relevance on creative names.
- R4.2 No trending algorithm (play_count delta over time).
- R4.3 Project tags exist in TOML; not surfaced on web.
- R4.4 Spatial / scene-similarity search (find "vibe") is the AI differentiator.
- R4.5 Pagination cursor; no jump-to-page.

**Implications (I)**
- *Strategic:* discovery is the long-tail retention lever.
- *Cross-system:* embedvec wiring closes [20] Feature 7.

**Risks (X)** — X4.1 Cold-start gallery shows random projects to new visitors.

**Mitigations (M)** — M4.1 Hybrid: substring + semantic + featured + popularity.

---

### Feature 5 — Simulation launch (Play button)  **— P0**

**State:** 🔴 **P0 launch blocker** · **Effort:** L · **Risk:** Critical · **Touches:** [01], [03_MULTIPLAYER], [06], [12_INFRASTRUCTURE], [15_MOBILE], C13
**Sub-features:** `eustress://play/{sim_id}?token={join_token}` URL scheme · backend `/api/simulations/{id}/play` → server allocation · OS protocol handler registration (installer) · mobile universal-link / app-link · fallback to download page · session token TTL · re-join after install

**Concept.** Click "Play" → backend allocates server via Forge → returns `eustress://play/{sim_id}?token=...` → OS launches installed Client → Client claims URL, parses token, opens QUIC to server. C13 cross-cut. Today: `/play/{sim_id}` shows server status + "Download Engine" fallback only.

**Forecasted feedback (R)**
- R5.1 **No code today launches anything.** UI shows server status; falls back to download.
- R5.2 Custom protocol handler registered per OS at install time ([12_INFRASTRUCTURE] Feature 5).
- R5.3 If Client not installed, Play falls back to download page + token survives install.
- R5.4 Mobile: universal-link (iOS) / app-link (Android).
- R5.5 Forum-pasted Play link should not auto-launch — confirm dialog.
- R5.6 In-browser preview (wasm Client) is a long-term option ([06] Feature 19).

**Implications (I)**
- *Strategic:* **THE conversion event.** Until fixed, gallery is window-shopping.
- *Cross-system:* C13 coordinates with [12_INFRASTRUCTURE] (installer reg), [01] (Client claims URL), [03_MULTIPLAYER] (token validation in QUIC), [08_IDENTITY] (token issuance), [15_MOBILE] (universal links).
- *Architectural:* token-handoff contract shared with [03] Feature 8.
- *Migration:* OS-handler reg requires installer changes per platform.
- *Support:* "Play button does nothing" = #1 ticket category right now.
- *Operational:* token TTL short (60 s) → tight clock-sync.

**Risks (X)**
- X5.1 Browser denies `eustress://` launch (popup blocker, OS policy).
- X5.2 Token leaked via clipboard / shoulder-surf → unauthorised join.
- X5.3 OS handler hijacking (malicious app claims `eustress://`).

**Mitigations (M)**
- M5.1 Fallback button: "Open in installed app" + install prompt.
- M5.2 Tokens single-use + short-TTL.
- M5.3 Installer registers exclusive handler (Windows DefaultProgs reg).

---

### Feature 6 — Downloads page

**State:** 🟡 (UI exists; no `latest.json` consumer) · **Effort:** S · **Risk:** Low · **Touches:** [06], [12_INFRASTRUCTURE]
**Sub-features:** `latest.json` parser · platform auto-detect (navigator.userAgent) · direct download links per platform · version history toggle · SHA-256 verify link · mobile (TestFlight / Play Store) links

**Concept.** `latest.json` exists at `releases.eustress.dev/latest.json` (CI produces); download page must consume it.

**Forecasted feedback (R)** — R6.1 Auto-detect + fallback dropdown. R6.2 Checksum UX (link to verify tool). R6.3 Release notes from GitHub Releases API. R6.4 Mobile downloads — same page or separate?

**Implications (I)** — *Strategic:* downloads must be 100% reliable; this is the install flow.

---

### Feature 7 — Leaderboards + community stats

**State:** 🟡 read; no WebSocket · **Effort:** M · **Risk:** Low · **Touches:** [06], [10_TELEMETRY]
**Sub-features:** Bliss leaderboard (top earners) · Community stats (global counters) · per-experience top scores · WebSocket real-time updates planned · personal / friends leaderboards · anti-cheat (auth on increment)

**Concept.** Reads work; real-time absent (no WS server). play_count incremented from `curl` POST today — needs auth.

**Forecasted feedback (R)** — R7.1 Edge cache `/api/community/leaderboard` for 30 s. R7.2 Friends-only leaderboards. R7.3 Anti-cheat: server-validated session for increment.

**Implications (I)** — *Strategic:* competitive engagement driver.

**Risks (X)** — X7.1 Spoofable score increments.

**Mitigations (M)** — M7.1 `/api/events/ingest` requires JWT + session token.

---

### Feature 8 — Monetisation surfaces

**State:** 🟡 UI / 🔴 backend · **Effort:** XL · **Risk:** Critical · **Touches:** [06], [09_ECONOMY]
**Sub-features:** Bliss tier purchase ($0.99–$49.99) · Premium subscription (3 tiers) · Marketplace listing + purchase · Stripe checkout link (endpoint missing) · Steam IAP (missing) · creator payouts (missing) · refund flow (missing) · tax compliance (missing)

**Concept.** Frontend ~90% built; backend ~5%. Stripe checkout link goes to nonexistent endpoint. Detailed audit in [09_ECONOMY].

**Forecasted feedback (R)** — see [09_ECONOMY] Features 2 / 3 / 8 / 10. R8.1 Bliss / Premium UI is theatre until checkout works.

**Implications (I)** — *Strategic:* monetisation pages must work before any paid marketing.

**Risks (X)** — X8.1 Users upgrade Premium expecting features; get nothing.

**Mitigations (M)** — M8.1 Disable purchase buttons until [09] backend ships.

---

### Feature 9 — Friends / parties UI

**State:** 🟡 types ✅ / backend 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [06], [03_MULTIPLAYER]
**Sub-features:** Friends page UI · `Friend`, `FriendStatus`, `JoinResponse` types · presence WS types · party UI planned · invite UI planned

**Concept.** Types in `web/src/api/friends.rs`. Backend `/api/friends`, presence WS server absent. See [03_MULTIPLAYER] Feature 10.

**Forecasted feedback (R)** — R9.1 UI without backend = theatre.

**Implications (I)** — *Strategic:* social = retention.

---

### Feature 10 — Telemetry & analytics ingest

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [06], [10_TELEMETRY]
**Sub-features:** pageview · event capture · session duration · churn · GDPR-compliant (no PII) · in-game events fed from [10] stream-node

**Concept.** Frontend has no event-capture library (no Posthog / Plausible / custom). Backend has no `/api/events`.

**Forecasted feedback (R)**
- R10.1 Vendor lock-in if Posthog / DataDog chosen.
- R10.2 GDPR-compliant requires careful event design.
- R10.3 In-game telemetry feeds from [10_TELEMETRY] stream-node → backend ingest.

**Implications (I)** — *Operational:* without telemetry, every product decision is unguided.

---

### Feature 11 — Blog / news page  *(P3 add)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [06]
**Sub-features:** `/blog` route · MDX/markdown content pipeline · RSS feed · author tags · tag filter

**Concept.** No `/blog` route. Industry standard; SEO + community-trust value.

**Forecasted feedback (R)** — R11.1 Content management — markdown files or headless CMS?

**Implications (I)** — *Strategic:* content marketing is the cheap-CAC channel.

---

### Feature 12 — Status / incidents page  *(P3 add)*

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [06], [12_INFRASTRUCTURE]
**Sub-features:** `/status` route · service health (API / R2 / servers) · incident timeline · incident-postmortem links · uptime stats

**Concept.** No `/status` dashboard. Industry standard for SaaS / cloud games. Feeds [12_INFRASTRUCTURE] monitoring.

**Forecasted feedback (R)** — R12.1 Self-host vs. StatusPage / Atlassian Statuspage.

**Implications (I)** — *Operational:* during outage, users need a single source of truth.

---

### Feature 13 — Public bug-bounty page  *(P3 add)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [06], [08_IDENTITY]
**Sub-features:** `/bounty` · scope · reward tiers · disclosure timeline · safe-harbour terms

**Concept.** Industry standard for trust-building. Touches [08_IDENTITY] for vuln triage.

**Implications (I)** — *Strategic:* signals security maturity.

---

### Feature 14 — Security disclosure page  *(P3 add)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [06], [08_IDENTITY]
**Sub-features:** security.txt · `/security` page · PGP key · contact · scope · responsible-disclosure terms

**Concept.** RFC 9116 `security.txt` + a human-readable page.

**Implications (I)** — *Compliance:* SOC 2 / ISO 27001 ask for it.

---

### Feature 15 — Accessibility features (WCAG AA)  *(P3 add)*

**State:** 🔴 · **Effort:** L · **Risk:** High · **Touches:** [02_STUDIO], [06]
**Sub-features:** ARIA labels · high-contrast mode · keyboard nav · screen reader compat · captions for video · focus indicators

**Concept.** Section 508 / WCAG AA non-compliance blocks government / enterprise sales.

**Forecasted feedback (R)** — R15.1 Top marketing pages first; gallery + dashboard second. R15.2 [02_STUDIO] also needs runtime ARIA (Feature 23).

**Implications (I)** — *Compliance:* mandated for many enterprise contracts.

**Risks (X)** — X15.1 Public launch without accessibility = reputational + regulatory risk.

**Mitigations (M)** — M15.1 ARIA audit + automated test (axe-core) in CI.

---

### Feature 16 — i18n / multi-language  *(P3 add)*

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [02_STUDIO], [06]
**Sub-features:** Fluent / gettext / custom framework · language selector · RTL support · per-`[lang]` route · locale-aware date / currency / unit formatting

**Concept.** en-US only. Multi-region launch (EU, APAC, LATAM) needs i18n + RTL.

**Forecasted feedback (R)** — R16.1 Translation file format (FTL, PO, JSON). R16.2 Crowdsourced translation (Crowdin / Weblate). R16.3 RTL (Arabic, Hebrew) affects CSS layout.

**Implications (I)** — *Strategic:* unlocks 60%+ of global TAM.

---

### Feature 17 — Cookie consent UI / banner  *(P3 add)*

**State:** 🔴 · **Effort:** S · **Risk:** High (EU compliance) · **Touches:** [06], [10_TELEMETRY]
**Sub-features:** consent banner · opt-in / opt-out / categories (necessary / analytics / marketing) · cookies-page integration · localStorage persistence

**Concept.** `/cookies` page exists; no consent UI. EU cookie law: without consent, every EU pageview is a violation.

**Forecasted feedback (R)** — R17.1 Geo-gated banner (only show in EU+UK+CA).

**Implications (I)** — *Compliance:* mandated.

**Risks (X)** — X17.1 Pre-banner analytics calls = violation.

**Mitigations (M)** — M17.1 Default analytics off until consent.

---

### Feature 18 — GDPR data-export self-serve  *(P3 add)*

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [06], [08_IDENTITY], [16_PERSISTENCE]
**Sub-features:** `/api/gdpr/export` · user-driven data download · access log · deletion request flow · 30-day SLA · audit log

**Concept.** GDPR Article 15 (right of access) + Article 17 (right of erasure). Self-serve = scalable; alternative is support-ticket queue.

**Forecasted feedback (R)** — R18.1 Cascade delete across [04], [09], [10], [11], [16].

**Implications (I)** — *Compliance:* mandatory for EU users.

---

### Feature 19 — In-browser Editor (WASM canvas)  *(P3 add)*

**State:** 🟡 (canvas; engine not loaded) · **Effort:** XL · **Risk:** High · **Touches:** [02_STUDIO], [06]
**Sub-features:** `/editor/{id}` route · canvas + toolbar + properties + output log · WASM Engine load (`// TODO`) · Slint-on-WASM compatibility · file-system shim (OPFS) · WebRTC for multiplayer studio

**Concept.** Browser-based Studio. Page layout shipped; line 29 of `editor.rs`: `// TODO: Actually load the Eustress WASM engine here`. Canvas waits for engine.

**Forecasted feedback (R)**
- R19.1 Bevy on WASM works; Slint-on-WASM unverified.
- R19.2 File-system access in browser requires OPFS (Origin Private File System).
- R19.3 Performance: 10× slower than native expected.
- R19.4 Multiplayer Studio over WebRTC ([02] Feature 15) feasible.

**Implications (I)** — *Strategic:* "no install required" is the conversion-funnel dream.

**Risks (X)** — X19.1 Quality gap vs. native disappoints users → bad reviews.

**Mitigations (M)** — M19.1 Ship as "Preview Mode" with clear "Install for full Studio" CTA.

---

## Wiring / import gaps

1. `eustress://` deep-link emit on Play (C13)
2. OAuth providers Discord / Google / GitHub (C14)
3. Stripe checkout + IAP backend (→ [09_ECONOMY])
4. Marketplace checkout (→ [09_ECONOMY])
5. embedvec semantic gallery search (→ [20_SEARCH])
6. Real-time leaderboard WebSocket
7. `latest.json` consumer on Downloads page
8. Telemetry ingest endpoint + frontend SDK
9. Creator payout dashboard (→ [09_ECONOMY])
10. SMTP service for email backup (lost-key flow)
11. CDN cache headers
12. Lost-key recovery flow (→ [08_IDENTITY])
13. Blog / News page + content pipeline
14. Status / incidents page
15. Bug-bounty + security-disclosure pages
16. WCAG AA accessibility pass
17. i18n framework
18. Cookie consent banner (EU)
19. GDPR self-serve data export
20. In-browser Editor: WASM engine load
21. Pre-rendered / SSR for top marketing pages

---

## Cross-system dependencies

- **C2 / Canonical create** — web publish routes through `create_instance`.
- **C8 / AI consent** — gallery filters by `ai_training` flag if needed.
- **C11 / `.eustress` world container** — Publish flow uploads the `.eustress` world container (Fjall WorldDb + baked `.echk` chunks; was `.pak` pre-2026-05-16) — Feature 0 in [04]. Upload mechanism unchanged.
- **C13 / `eustress://`** — Play button emits the URL (Feature 5).
- **C14 / KYC-deferred OAuth** — Feature 3 implementation.
- Depends on **[03_MULTIPLAYER]** sessions / friends; **[04_ASSETS]** upload + download; **[08_IDENTITY]** auth; **[09_ECONOMY]** checkout; **[10_TELEMETRY]** ingest; **[20_SEARCH]** semantic.

---

## Open questions

- Q6.1 OAuth vs. KYC-first launch policy (C14 says OAuth-first).
- Q6.2 Stripe vs. Steam IAP primary checkout.
- Q6.3 SSR / pre-render decision.
- Q6.4 In-browser play preview — 1.0 or 2.0?
- Q6.5 Telemetry vendor (Posthog / Plausible / custom).
- Q6.6 i18n framework choice (Fluent / gettext / custom).
- Q6.7 Bug-bounty rewards budget.
