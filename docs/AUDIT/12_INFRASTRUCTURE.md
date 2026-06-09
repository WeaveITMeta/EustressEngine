# 12 — Infrastructure & DevOps

> Eustress Forge (Nomad + Consul orchestration), Terraform on AWS, Cloudflare R2 +
> Worker edge, release pipeline (GitHub Actions), installer build, code signing,
> Vault secrets, monitoring stack, multi-region. The platform's ops backbone.

## Pass changelog

- **P2 (2026-05-14):** New system doc; 14 features, 8 cards expanded, 12 wiring gaps. Maturity ~55%.
- **P4 (2026-05-14):** State correction from secondary critique: Vault state should be **10% (references in Nomad job specs; cluster not deployed)** not 0%. Consul directory **confirmed empty**.

---

## Concept summary

**Eustress Forge** is a Rust-native multiplayer orchestration platform replacing the deprecated Kubernetes / Agones architecture. Built on **HashiCorp Nomad** (lightweight, milliseconds-scale scaling, <0.5% overhead vs. K8s's 3–7%) + **Consul** (service mesh, health, config). Claimed 70–90% cost reduction at scale.

**Layers:**
- **Control plane (Rust)**: `ForgeController` — server lifecycle (spawn / terminate / route), session routing, autoscaling, health monitoring.
- **Data plane (Nomad)**: parameterised job specs for game servers, physics workers, AI inference workers.
- **Service mesh (Consul)**: service discovery, health checks, configuration management.
- **Infra-as-code (Terraform)**: multi-AZ AWS VPC with Nomad servers (reserved EC2) + clients (90%+ Spot for cost).
- **Release pipeline (GitHub Actions)**: tag-driven multi-platform builds (Win/Mac/Linux) → R2 → `latest.json` manifest → in-app updater.
- **Edge / CDN (Cloudflare)**: Workers for downloads, `api.eustress.dev` for asset + auth + KYC + witness co-sign; R2 buckets for releases, assets, KYC documents.

State: release pipeline ✅; orchestration foundation ✅; **security gaps** (code signing, Vault, Consul ACLs) **block production**. Multi-region, on-prem, hybrid all roadmap.

---

## Implementation snapshot

**Code locations:**
- `.github/workflows/release.yml` — multi-platform release (~30 min wall-clock, 3 platforms parallel)
- `.github/workflows/ci.yml` — cargo-deny security scan + WGSL validation (stale Rust-WASM-demo / pnpm-web / Playwright jobs targeting removed directories were dropped 2026-06-09); **no desktop engine build in CI**
- `infrastructure/forge/terraform/` — AWS VPC, ASGs, IAM, S3, CloudWatch
- `infrastructure/forge/nomad/` — 4 job specs (forge-orchestrator, gameserver, physics, ai); meta-vars `experience_id`, `server_id`, `max_players`; QUIC port 4433
- `infrastructure/forge/consul/` — **empty directory** (TODO)
- `infrastructure/forge/scripts/deploy.sh` — bash orchestrator (preflight → tf init/plan/apply → nomad jobs → health verify)
- [eustress/crates/forge](../../eustress/crates/forge/), [eustress/crates/forge-sdk](../../eustress/crates/forge-sdk/) — Rust SDK
- [eustress/installer/](../../eustress/installer/) — Inno Setup MSI template; macOS DMG auto-build in CI; Linux `install.sh`

**Working:**
- ✅ CI multi-platform release (Win ZIP, macOS DMG, Linux tar.gz)
- ✅ Version manifest (`latest.json`) per release
- ✅ R2 bucket layout (`eustress-releases/${VERSION}/`)
- ✅ Cloudflare Download Worker (JWT-validated, public `latest.json`)
- ✅ Nomad cluster (single-region, 3 server + N spot client nodes)
- ✅ Nomad job specs (parameterised, register with Consul)
- ✅ Forge SDK (Rust crate; ForgeController, Nomad job submission)

**Incomplete / Missing:**
- 🟡 macOS notarisation (icon bundling done; signing + notarisation absent)
- 🔴 Windows authenticode (executable unsigned → SmartScreen warnings)
- 🔴 Vault integration (references in job specs; not deployed)
- 🔴 Consul config (directory empty)
- 🔴 Prometheus scrape + Grafana
- 🔴 Multi-region Terraform modules
- 🔴 Spot interruption handler systemd service
- 🔴 Backup / DR runbooks
- 🟡 Linux installer (.deb / systemd)
- 🟡 In-app auto-update binary download (latest.json read; no fetch+apply)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | CI build (Rust + Web) | 🟡 partial (no desktop engine) |
| 2 | Multi-platform release | ✅ |
| 3 | Code signing (macOS notarisation) | 🔴 *(P3 correction: was 🟡 20%; icon bundling is decorative, signing+notarisation absent → 0%)* |
| 4 | Code signing (Windows authenticode) | 🔴 0% |
| 5 | Installer (Win/Mac/Linux) | 🟡 partial |
| 6 | Version manifest (`latest.json`) | ✅ |
| 7 | R2 bucket layout | ✅ |
| 8 | CDN (Cloudflare Workers) | 🟢 |
| 9 | Nomad cluster | ✅ |
| 10 | Consul mesh | 🟡 stub (config empty) |
| 11 | Vault secret management | 🔴 0% |
| 12 | Prometheus / Grafana / Alerting | 🔴 |
| 13 | Multi-region failover | 🔴 |
| 14 | Backup / DR runbooks | 🔴 |

---

## Detailed per-feature cards (top 8)

### Feature 2 — Multi-platform release pipeline

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [06], [12]
**Sub-features:** branch model (main → Core fast-forward → tag) · 3-platform parallel build (Win 20 min, macOS 25 min, Linux 15 min) · SHA-256 checksums · `latest.json` manifest · R2 upload via AWS S3 CLI · GitHub Release with auto notes · 5-min CDN cache · rollback procedure

**Concept.** Tag a verified `main` commit on the `Core` branch; CI builds, signs (partially), checksums, uploads to R2, publishes manifest + GitHub Release. Total wall-clock ~30 min. Documented in [RELEASE.md](../../RELEASE.md).

**Forecasted feedback (R)**
- R2.1 Solid; main risk is GitHub or R2 rate limits at release cadence.
- R2.2 Auto-notes from commits — quality varies; manual edit on release.
- R2.3 Rollback procedure documented; re-tag previous version restores `latest.json`.

**Implications (I)**
- *Architectural:* release tags are the only consumer-facing version surface.
- *Operational cost:* ~$0 per release; CI minutes are the main cost.
- *Strategic:* matches indie-game standard; not behind any peer.

**Risks (X)** — X2.1 GitHub Actions outage blocks all releases.

**Mitigations (M)** — M2.1 Documented manual fallback (local `cargo build` + R2 upload + manual `latest.json` update).

---

### Feature 3 — macOS notarisation

**State:** 🟡 20% · **Effort:** S (2–3 days) · **Risk:** High (blocking) · **Touches:** [12]
**Sub-features:** Apple Developer account · App-specific password / Developer ID Application cert · `notarytool submit` integration · staple ticket to .app · Gatekeeper check

**Concept.** macOS Gatekeeper refuses to run un-notarised binaries (or warns prominently). Notarisation: build .app, codesign with Developer ID, submit to Apple's notary service, wait for ticket, staple ticket onto .app. Apple Developer account required (~$99/yr).

**Forecasted feedback (R)**
- R3.1 Icon bundling already done; signing + notarisation absent.
- R3.2 Without notarisation, users see "App is damaged" — terrible first impression.
- R3.3 Notary service can take minutes to hours — async CI step.
- R3.4 M1 / M2 distribution requires Apple Silicon binary signed for arm64.

**Implications (I)**
- *Operational cost:* $99/yr + cert renewal.
- *Strategic:* macOS users are a high-value cohort; un-notarised = lost installs.

**Risks (X)** — X3.1 Failed notarisation blocks release indefinitely.

**Mitigations (M)** — M3.1 Test notarisation on a beta build before tagging release.

---

### Feature 4 — Windows authenticode

**State:** 🔴 0% · **Effort:** S (1–2 days + cert procurement) · **Risk:** Med (UX) · **Touches:** [12]
**Sub-features:** EV cert (HW token) or DV/OV cert · `signtool` integration in CI · timestamp server · SmartScreen reputation

**Concept.** Without authenticode, Windows SmartScreen warns or blocks. EV certs ($400+/yr) earn immediate SmartScreen trust; OV certs ($100/yr) require reputation accrual.

**Forecasted feedback (R)**
- R4.1 Unsigned exe = SmartScreen "Unrecognized App, Don't Run" full-screen warning.
- R4.2 IT departments block unsigned executables.
- R4.3 EV cert requires HW token (DigiCert / Sectigo) — physical inconvenience.

**Implications (I)** — *Strategic:* enterprise distribution requires it.

**Risks (X)** — X4.1 First-week install conversion craters without it.

**Mitigations (M)** — M4.1 Start with OV ($100/yr) + build SmartScreen reputation; upgrade to EV at scale.

---

### Feature 11 — Vault secret management

**State:** 🔴 0% · **Effort:** M (3–5 days) · **Risk:** Critical · **Touches:** [12]
**Sub-features:** Terraform Vault cluster · Nomad-Vault integration · per-job token policy · automatic rotation

**Concept.** Nomad job specs reference `secret "nomad/creds/forge-orchestrator"` but Vault isn't deployed. Today's secrets are env-var-injected at deploy time — compromised node = all credentials exposed.

**Forecasted feedback (R)**
- R11.1 Hardcoded secrets in version control / Terraform state.
- R11.2 No rotation policy.
- R11.3 Vault is the standard answer; alternatives (AWS Secrets Manager) viable but couple to AWS.

**Implications (I)** — *Compliance:* SOC 2 / ISO 27001 require centralised secret management.

**Risks (X)** — X11.1 Node compromise = full breach.

**Mitigations (M)** — M11.1 Pre-launch must-have; phase Vault before public multiplayer launch.

---

### Feature 9 — Nomad cluster

**State:** ✅ 90% · **Effort:** S (close gaps) · **Risk:** Med · **Touches:** [03], [12]
**Sub-features:** Multi-AZ VPC · 3 reserved server nodes · N spot client nodes (90%+ discount) · ASGs with launch templates · IAM instance profiles · CloudWatch CPU-based scaling · 4 parameterised job specs · QUIC port 4433 · S3 binary download

**Concept.** Terraform spins a multi-AZ cluster; Nomad servers run consensus + scheduler; clients run game / physics / AI workloads. Game-server job downloads binary from S3 on start, registers with Consul, exposes metrics on port 9100+.

**Forecasted feedback (R)**
- R9.1 Spot interruption handler systemd service not in Terraform.
- R9.2 Nomad ACLs are commented-out TODO.
- R9.3 No graceful drain on spot termination → mid-game disconnect for affected players.
- R9.4 Single-region today.

**Implications (I)**
- *Operational cost:* 80% saving over reserved-only.
- *Strategic:* Forge's central marketing claim depends on this working at scale.

**Risks (X)** — X9.1 Spot reclaim mid-match without graceful drain = bad player experience.

**Mitigations (M)**
- M9.1 Spot interruption handler (2-min warning) drains + migrates players.
- M9.2 Enable Nomad ACLs before public launch.

---

### Feature 12 — Prometheus / Grafana / Alerting

**State:** 🔴 · **Effort:** L (1 week) · **Risk:** High (no observability) · **Touches:** [10], [12]
**Sub-features:** `/metrics` Prometheus endpoint · scrape config · Grafana dashboards (cluster, regional, server, player) · AlertManager rules · paging integration (PagerDuty / Opsgenie)

**Concept.** Nomad jobs expose port 9100+ metrics (already done). Add Prometheus scraper, Grafana dashboards (templates in `infrastructure/forge/grafana/`), AlertManager rules (high latency, server failures, capacity, queue buildup), pager integration.

**Forecasted feedback (R)**
- R12.1 Endpoints exist; no scraper.
- R12.2 No dashboards.
- R12.3 No alerts → outages discovered only via user reports.

**Implications (I)** — *Operational:* unshippable for production-scale without it.

**Risks (X)** — X12.1 Blind ops at scale = catastrophic outages with no advance warning.

**Mitigations (M)** — M12.1 Prometheus + Grafana stack in `infrastructure/monitoring/` Terraform module.

---

### Feature 13 — Multi-region failover

**State:** 🔴 0% · **Effort:** XL · **Risk:** Med (latency) · **Touches:** [03], [12]
**Sub-features:** Terraform per-region modules · Consul federation (WAN gossip) · Global Accelerator vs. DNS-based routing · region-aware client connect · cross-region replication

**Concept.** Today's Terraform is single-region. Multi-region requires Consul WAN federation, region modules, and either AWS Global Accelerator (anycast) or geo-DNS for client routing.

**Forecasted feedback (R)** — R13.1 Phased to Q2 2026 in the roadmap; no code yet.

**Implications (I)** — *Strategic:* APAC + EU growth gated by latency.

**Risks (X)** — X13.1 Single-region outage = global outage.

**Mitigations (M)** — M13.1 Add a second region (eu-west) with active-passive failover before active-active.

---

### Feature 1 — CI build coverage

**State:** 🟡 partial · **Effort:** S · **Risk:** Med · **Touches:** [12]
**Sub-features:** cargo-deny security scan · WGSL validation · **MISSING: desktop engine build** (the former WASM-demo / pnpm-web / Playwright jobs targeted directories no longer in the repo and were removed 2026-06-09)

**Concept.** CI builds the WASM demo + the website + runs lint/security; the desktop engine binary is **not built in CI** (only on release-tag). This means PRs can land that break the engine without CI catching it.

**Forecasted feedback (R)**
- R1.1 Engine builds only on release → release-day surprises.
- R1.2 No vendored deps → supply-chain attack risk.
- R1.3 No integration test against a real Universe.

**Implications (I)** — *Operational:* every PR should at minimum `cargo check` the engine.

**Risks (X)** — X1.1 Broken `main` between releases.

**Mitigations (M)** — M1.1 Add `cargo build --release --bin eustress-engine` to PR CI.

---

## Wiring / import gaps (top 12)

1. macOS notarisation step in `release.yml`
2. Windows authenticode signing in `release.yml`
3. HashiCorp Vault cluster Terraform + Nomad integration
4. Consul config files (ACLs, TLS, federation prep)
5. Prometheus scrape config + Grafana dashboards
6. Multi-region Terraform modules
7. Spot interruption handler systemd service
8. Backup / DR runbooks (Nomad state, Consul snapshots, R2 restore)
9. Linux .deb installer + systemd service
10. In-app auto-update binary fetch + apply
11. Desktop engine build in PR CI
12. `cargo vendor` for reproducible builds

---

## Cross-system dependencies

- **[01_CLIENT]** delivered via release pipeline + auto-update.
- **[03_MULTIPLAYER]** runs on Nomad cluster; Forge SDK is the integration.
- **[04_ASSETS]** R2 buckets store paks + thumbnails.
- **[06_WEBSITE]** downloads page consumes `latest.json`.
- **[08_IDENTITY]** secrets management for JWT / Stripe / Steam.
- **[10_TELEMETRY]** Prometheus + Grafana + Sentry hosting.

---

## Open questions

- Q12.1 Cloud lock-in (AWS) vs. hybrid (on-prem + AWS burst)?
- Q12.2 Multi-region routing: Global Accelerator vs. geo-DNS?
- Q12.3 On-prem / colocation support for enterprise customers?
- Q12.4 Build reproducibility — `cargo vendor` strategy?
- Q12.5 Code-signing cert budget (EV ~$400/yr + Apple $99/yr)?
- Q12.6 Backup strategy for Nomad RocksDB state — snapshot + replication?
- Q12.7 Nomad upgrade path — zero-downtime via new ASG per version?
- Q12.8 Consul encrypt stanza in job specs (TLS Nomad ↔ Consul)?
- Q12.9 Forge SDK production-readiness gate — tests + CI integration?
- Q12.10 Historical build artifacts retention (S3 lifecycle)?
- Q12.11 Rollback SLA — automatic canary vs. manual?
