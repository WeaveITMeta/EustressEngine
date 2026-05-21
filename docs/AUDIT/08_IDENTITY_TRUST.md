# 08 — Identity, Trust & Safety

> Ed25519 sovereign portable identity, JWT, KYC ontology, succession, witness
> co-signing, content moderation, CSAM detection, GDPR / COPPA gating, audit log,
> anti-cheat. Touches every other system.

## Pass changelog

- **P2 (2026-05-14):** New system doc; 15 features, 8 cards expanded, 12 wiring gaps.
- **P4 (2026-05-14):** State corrections from secondary critique: KYC jurisdictions dict is **empty** (not 72 countries — spec-only). Succession activation logic (inactivity timer, heir claim, proof-of-life) is **0%**; true state ~40%, not 80% (sig + verify are signature-complete only). `.pak` signing dependency surfaced for asset-chain provenance.
- **Updated 2026-05-16: storage pivot.** The signing dependency now targets the `.eustress` world container (publish hash over the Fjall WorldDb database + baked `.echk` chunks), not `.pak`. `.pak`→`.eustress` corrections in the Feature 11 provenance references. See MASTER C17.

---

## Concept summary

**Sovereign Portable Identity (SPI).** Users generate Ed25519 keypairs locally; the server never sees the private key. Challenge-response auth: client requests a nonce, signs locally with the private key, server verifies via the public key. JWT issued by a Cloudflare Worker (`api.eustress.dev`); validated by the Axum backend. The identity is portable: it survives server forks, works across jurisdictions, and supports succession (named heir public keys), contribution history (hash-chained portable record), and witness co-signing (Cloudflare Worker at `witness.eustress.dev` signs hashes to attest contributions).

**Trust spine.** Identity gates access. KYC ontology (72 countries today, target ~195) drives monetisation eligibility, age gating, and jurisdiction-specific UI. Content moderation pipeline (designed; ML inference stubbed) covers classification, reporting, appeal, ban + escalation. CSAM detection (zero implementation today) must integrate PhotoDNA + 24-hour NCMEC CyberTipline reporting. GDPR / COPPA / COPPA 2.0 / TIDA / DMCA frameworks live in `docs/legal/` — code-side compliance is mostly absent.

The core crypto is **production-quality (~95%)**. The peripheral safety infrastructure (recovery, MFA, OAuth, CSAM, ban appeals, anti-cheat) is **0–20%**.

---

## Implementation snapshot

**Crates:**
- [eustress-identity](../../eustress/crates/identity/) — 10 modules: schema, keypair, issuer, verifier, revocation, succession, history, desktop, witness, lib
- [eustress-backend/src/auth.rs](../../eustress/crates/backend/src/) — Axum JWT validation, `AuthUser` extractor, `Claims` struct
- [eustress/crates/web/src/api/auth.rs](../../eustress/crates/web/src/api/) — Web auth API + `RegisterRequest` + identity-backup email
- `infrastructure/cloudflare/api/src/index.js` — Unified Eustress Worker (Auth + KYC + Co-sign) with KV namespaces (USERS, CHALLENGES, KYC_STATUS) + R2 bucket (KYC_BUCKET)
- `infrastructure/cloudflare/witness/src/` — Co-signing Worker, fork info, revocation publication
- [engine/src/soul/audit_log.rs](../../eustress/crates/engine/src/soul/) — Claude API call audit (SoulService/Logs)
- [docs/legal/](../legal/) — CSAM, COPPA, GDPR, TIDA, DMCA frameworks
- [docs/moderation/](../moderation/) — Moderation API spec

**Auth flows (working):**
1. Ed25519 challenge-response — `POST /api/auth/challenge` → nonce → client signs locally → `POST /api/auth/verify-challenge` → JWT.
2. Email/password (legacy) — `POST /api/auth/login` → KV check → JWT.
3. Identity registration — keypair generated client-side; optional email + `identity.toml` backup auto-mailed.

**JWT:** HS256, shared secret in `JWT_SECRET` env; Claims = `(sub, exp, iat)`.

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Ed25519 keypair generation + zeroization | ✅ |
| 2 | Challenge-response auth (relay + replay safe) | ✅ |
| 3 | Server identity issuance (signed identity.toml, 1-yr) | ✅ |
| 4 | Cross-server verification | ✅ |
| 5 | Revocation list (`/.well-known/eustress-revoked`) | ✅ |
| 6 | Succession / heir keys | 🟡 *(P3 correction: was ✅; 6 open blockers → 40%, not 80%)* |
| 7 | Hash-chained contribution history | ✅ |
| 8 | Witness co-signing (Cloudflare Worker) | ✅ |
| 9 | JWT token lifecycle | ✅ |
| 10 | Content moderation pipeline (ML) | 🟡 spec; no ML weights |
| 11 | Content reporting + appeals | 🟡 |
| 12 | CSAM detection (PhotoDNA + NCMEC) | 🔴 |
| 13 | GDPR data-subject rights | 🟡 frameworks; no automation |
| 14 | COPPA age gating | 🔴 |
| 15 | OAuth alternative (Discord / Google / GitHub) | 🔴 |
| 16 | Account recovery / MFA | 🔴 |
| 17 | Anti-cheat heuristics | 🔴 |
| 18 | Audit log (user actions) | 🟡 Claude calls only |

---

## Detailed per-feature cards (top 8)

### Feature 2 — Ed25519 Challenge-Response Auth

**State:** ✅ ~95% · **Effort:** Done · **Risk:** Med (recovery) · **Touches:** all
**Sub-features:** domain-bound challenge · timestamp-bound nonce · server-side zeroization · client-side private-key generation · `verify_challenge` in [keypair.rs](../../eustress/crates/identity/src/keypair.rs)

**Concept.** Client sends public key → server creates a nonce bound to (domain, timestamp), stores in `CHALLENGES` KV → client signs nonce locally with private key → server verifies signature → JWT issued. Domain binding prevents relay; timestamp prevents replay; nonce collision astronomically improbable.

**Forecasted feedback (R)**
- R2.1 Key loss = account loss. No built-in recovery (`identity.toml` deletion is permanent).
- R2.2 Multi-step (challenge → sign → verify) is UX-heavier than email/password.
- R2.3 Mobile Safari blocks file-based access; needs a different export path (passkey?).
- R2.4 Keychain integration on macOS / Windows: store private key in OS keychain; user doesn't see a file.
- R2.5 Hardware token support (YubiKey) would be a security upgrade.

**Implications (I)**
- *Architectural:* server is **stateless** w.r.t. private keys — no breach can extract user secrets.
- *Cross-system:* same key signs published `.eustress` world containers (Feature 11 below — provenance chain). *(Was `.pak` pre-2026-05-16; the signed artifact is now the world container's publish hash.)*
- *Migration:* legacy email/password accounts need a one-time keypair-bind flow.
- *Operational cost:* zero — no password-reset support tickets... *except* lost-key tickets.
- *Support burden:* "I lost my identity.toml" → high-frequency ticket once base grows.
- *Strategic:* "you own your identity" is a marketing pillar matching Web3 sensibilities.

**Risks (X)**
- X2.1 No recovery → 5% annual key loss = 5% annual account loss = brutal churn.
- X2.2 Stolen laptop with unencrypted identity.toml → account theft.

**Mitigations (M)**
- M2.1 OS-keychain integration; never write the raw key to a file.
- M2.2 Optional email-based recovery (rotates to new key; old key marked revoked).
- M2.3 Multi-device key sync via encrypted backup to user-controlled cloud.

---

### Feature 6 — Succession / Will

**State:** ✅ 80% · **Effort:** Done (mostly) · **Risk:** Med · **Touches:** [08], [09]
**Sub-features:** ordered heir list signed by owner · heir public keys in succession block of identity.toml · automatic inheritance on death

**Concept.** Owner names ordered heirs. On death/incapacity, the first heir inherits the entire account. Portable across servers (no third-party verification).

**Forecasted feedback (R)**
- R6.1 Death/incapacity proof is undefined — could deadlock accounts for years.
- R6.2 Inactivity = death? After how long (1 year? 5?) — needs spec.
- R6.3 Heir contact: how is the heir notified the deceased's account is theirs?
- R6.4 Multiple heirs splitting Bliss balance is undefined (currently winner-takes-all).
- R6.5 Heir publishes their own identity — does it use the deceased's username?

**Implications (I)**
- *Cross-system:* [09_ECONOMY] — Bliss balance + accumulated marketplace earnings transfer to heir.
- *Migration:* existing accounts have empty succession block; trigger a one-time prompt.
- *Strategic:* attractive to long-term creators (legacy preservation).
- *Compliance:* probate law varies by jurisdiction; SPI may conflict with traditional inheritance court orders.

**Risks (X)**
- X6.1 Heir collusion ("inactive owner"); abuse via fake death claim.
- X6.2 Account dormant 10 years — who legitimately inherits if no heir was named?

**Mitigations (M)**
- M6.1 Inactivity timer (configurable, default 1 yr) → heir claim window.
- M6.2 Heir-claim requires the heir's own signed proof-of-life + delay window for owner re-activation.

---

### Feature 12 — CSAM Detection

**State:** 🔴 5% · **Effort:** L · **Risk:** Critical · **Touches:** [04], [06], [08]
**Sub-features:** PhotoDNA hashing · NCMEC CyberTipline API (24-hr report mandate) · evidence preservation (90 days) · zero-tolerance auto-removal · no appeals

**Concept.** Every uploaded image / video is hashed against PhotoDNA's CSAM database. Match → instant removal, 24-hour mandatory report to NCMEC, evidence preserved 90 days. Penalties for non-reporting are $150K first offense, $300K subsequent.

**Forecasted feedback (R)**
- R12.1 Zero implementation; system cannot detect today.
- R12.2 PhotoDNA SDK integration is non-trivial — requires partnership with Microsoft.
- R12.3 NCMEC CyberTipline API has its own auth + format.
- R12.4 90-day preservation requires a secure storage tier (encrypted, access-logged).
- R12.5 False positives (e.g. anatomy reference art) — what's the appeal path?

**Implications (I)**
- *Compliance:* the largest regulatory risk on the platform. Non-compliance is criminal.
- *Architectural:* every upload boundary ([04_ASSETS] textures, [02_STUDIO] thumbnails, [06_WEBSITE] avatars) must funnel through this scanner.
- *Operational cost:* PhotoDNA is free for compliant operators; NCMEC reporting is mandatory but free.
- *Support burden:* false positives need a 24-hour human review path.

**Risks (X)**
- X12.1 Liability if missed.
- X12.2 Reputational disaster if a false positive is mishandled.

**Mitigations (M)**
- M12.1 Implement PhotoDNA pre-launch — blocking ship gate.
- M12.2 Dedicated trust-and-safety review queue with SLA.

---

### Feature 15 — OAuth alternative (KYC-deferred)

**State:** 🔴 5% · **Effort:** M · **Risk:** Med · **Touches:** [06], [08]
**Sub-features:** Discord / Google / GitHub OAuth · KYC deferred until creator monetisation · account-linking flow

**Concept.** Mandatory KYC-first signup is industry-anomalous; ~half the funnel will bail. Adding OAuth-first sign-up with KYC deferred (only required for marketplace selling or Bliss withdrawal) matches industry norms.

**Forecasted feedback (R)**
- R15.1 Each provider requires its own app registration + secrets management.
- R15.2 Account linking (OAuth user later does KYC) needs a merge flow.
- R15.3 Discord OAuth has the strongest network-effect alignment.
- R15.4 Account-takeover via OAuth (compromised Discord = compromised Eustress) — 2FA required.

**Implications (I)**
- *Strategic:* signup-conversion likely 2× with OAuth-first.
- *Architectural:* OAuth users without keypair → must generate one in the background on first sign-in.
- *Cross-system:* [06_WEBSITE] login page restructure; [09_ECONOMY] gates KYC at first payout.

**Risks (X)** — X15.1 Reduced security for OAuth-only accounts (no Ed25519 challenge).

**Mitigations (M)** — M15.1 Force 2FA for OAuth-only; offer to upgrade to keypair anytime.

---

### Feature 16 — Account Recovery / MFA

**State:** 🔴 0% · **Effort:** L · **Risk:** Critical · **Touches:** [08]
**Sub-features:** email recovery code · TOTP MFA · hardware token (FIDO2) · device verification

**Concept.** Today, lost identity.toml = permanent account loss. Recovery options: (a) email-based recovery rotates to new key; (b) TOTP MFA (Authenticator app) for sensitive actions; (c) hardware token for power users.

**Forecasted feedback (R)**
- R16.1 Recovery email is the weakest link — if email compromised, account compromised.
- R16.2 TOTP is bare minimum industry standard.
- R16.3 SMS-MFA is not recommended (SIM swap attacks).
- R16.4 FIDO2 / WebAuthn would integrate with existing OS keychain story.

**Implications (I)**
- *Architectural:* recovery flow needs server-side state that the SPI model deliberately avoids — careful design.
- *Support burden:* without this, lost-key tickets are unsolvable.

**Risks (X)** — X16.1 No recovery = no large user base; account loss is a deal-breaker.

**Mitigations (M)** — M16.1 Pre-launch must-have. Implement TOTP + email recovery before public launch.

---

### Feature 10 — Content Moderation Pipeline

**State:** 🟡 60% spec · **Effort:** L · **Risk:** High · **Touches:** [04], [06], [08]
**Sub-features:** Candle ML inference stub · classification (toxicity / CSAM / spam) · confidence threshold (0.9) · review queue · per-tier rate limit · 7-day appeal window

**Concept.** Every user-generated content (chat msg, project listing, avatar) is classified. Below confidence threshold → review queue. Action: warn / hide / remove / ban (with duration). Appeals within 7 days.

**Forecasted feedback (R)**
- R10.1 ML weights absent; classification returns hardcoded "allow".
- R10.2 Appeal reviewers unspecified — humans? LLMs? Quorum?
- R10.3 No SLA on appeal review time.
- R10.4 Rate limits by tier are reasonable; need monitoring dashboard.

**Implications (I)** — *Compliance:* GDPR, COPPA, regional content laws all run through this pipeline.

**Risks (X)** — X10.1 Without ML, the system is blind; first viral toxic content is a PR event.

**Mitigations (M)** — M10.1 Train toxicity + spam classifiers on open datasets; ship in MVP. CSAM gets its own path (Feature 12).

---

### Feature 17 — Anti-cheat Heuristics

**State:** 🔴 0% · **Effort:** L · **Risk:** High · **Touches:** [01], [03], [08]
**Sub-features:** speed-cap validation · position-jump detection · script CPU budget · RPC rate limit · client fingerprinting

**Concept.** Server-side sanity checks on client-submitted state. Speedhacks (movement > max_speed), teleport hacks (position delta > tick budget), script-CPU exhaustion, RPC flood — all rejected + flagged.

**Forecasted feedback (R)**
- R17.1 No validation exists today; multiplayer is wide-open.
- R17.2 Per-script CPU budget needs Luau / Rune tick-counting (already partial in Soul).
- R17.3 Witness can detect contribution-spike outliers (high-level fraud); not low-level cheats.

**Implications (I)** — *Strategic:* unshippable for competitive multiplayer without it.

**Risks (X)** — X17.1 Day-one speedhacks = bad reviews + retention drop.

**Mitigations (M)** — M17.1 Speed + position validation as part of [03_MULTIPLAYER] Feature 3 (server-auth validation) — coordinate.

---

### Feature 18 — User Action Audit Log

**State:** 🟡 50% (Claude only) · **Effort:** M · **Risk:** Med · **Touches:** all
**Sub-features:** structured event log · per-user query API · retention policy · GDPR right-to-audit · audit log for moderation decisions

**Concept.** Every significant user action (login, report, appeal, ban, purchase, project publish, …) is logged with timestamp + actor + target. Today only Claude API calls are logged (in SoulService/Logs).

**Forecasted feedback (R)**
- R18.1 Only Claude calls logged; user actions absent.
- R18.2 Files-as-DB (TOML logs) — no full-text indexing; queries require filesystem scan.
- R18.3 Retention policy unspecified.
- R18.4 GDPR right-to-audit gives users a queryable view of their own log.

**Implications (I)** — *Compliance:* mandatory for moderation defensibility (ban appeal proof).

**Risks (X)** — X18.1 Without audit log, every dispute is unwinnable.

**Mitigations (M)** — M18.1 Centralised event sink in [10_TELEMETRY] consuming `identity.*` topics.

---

## Wiring / import gaps (top 12)

1. Lost-key recovery flow (email + new-key rotation + revoke old).
2. TOTP MFA + WebAuthn.
3. OAuth providers (Discord first; Google / GitHub second).
4. Moderation ML weights (toxicity, spam) via `candle`.
5. PhotoDNA integration + NCMEC CyberTipline API client.
6. KYC jurisdiction auto-detect (IP geolocation + override).
7. Ban-appeal workflow + SLA dashboard.
8. Anti-cheat heuristics (speed / position / RPC rate).
9. GDPR right-to-deletion cascade (across [04_ASSETS], [09_ECONOMY], [10_TELEMETRY]).
10. Chat-filter integration on real-time channels.
11. Signed `.eustress` world-container provenance chain (creator key signs publish hash; was `.pak` pre-2026-05-16).
12. User-action audit log → [10_TELEMETRY] `identity.*` topics.

---

## Cross-system dependencies

- **[01_CLIENT]** JWT in QUIC handshake (currently missing — [03_MULTIPLAYER] gap 1).
- **[02_STUDIO]** creator identity in publish flow; succession determines abandoned-project ownership.
- **[03_MULTIPLAYER]** server validates client JWT; ban-list propagates via revocation.
- **[04_ASSETS]** signed asset chain; KYC gates monetisation eligibility.
- **[06_WEBSITE]** sign-up / login / KYC UI; OAuth surface.
- **[09_ECONOMY]** KYC gates Bliss earning + Stripe Connect payouts.
- **[10_TELEMETRY]** identity events stream tee.

---

## Open policy questions

- Q8.1 KYC threshold: at what $earnings / day does KYC kick in? Suggested $10/day.
- Q8.2 Region exclusions: which sanctioned countries are blocked entirely (Iran, NK, Syria, Crimea)?
- Q8.3 COPPA (under-13) vs COPPA 2.0 (13–16) — which one applies, in which region?
- Q8.4 Ban-appeal SLA: 5 business days?
- Q8.5 Child accounts: who signs the keypair? Parent-link?
- Q8.6 Inactivity = death threshold: 365 days?
- Q8.7 Audit-log retention: 7 years (compliance) or shorter?
- Q8.8 CSAM false-positive appeal path without contacting NCMEC?
- Q8.9 VPN/proxy detection vs. legitimate use — flag for review, no auto-block?
- Q8.10 Witness centralisation — do we move to a quorum of 3 witnesses to harden against single-Worker compromise?
